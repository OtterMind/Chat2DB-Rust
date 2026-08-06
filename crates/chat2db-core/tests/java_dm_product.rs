use std::{panic::AssertUnwindSafe, path::PathBuf, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    ComponentState, CreateDatasourceRequest, Datasource, DatasourceConnection,
    DatasourceConnectionProperty, JdbcDriver, JdbcValue, NativeDriverAction, OperationEvent,
    ResultPageRequest,
};
use chat2db_core::{
    Application, ListColumnsRequest, ListDatabasesRequest, ListSchemasRequest, ListTablesRequest,
    MetadataScope, RuntimeConfig, RuntimeHost, TablePreviewRequest, TableRef,
};
use chat2db_java_bridge::{
    BridgeError, ConnectionProperty, DriverClient, EngineCommand, EngineConfig, Session,
    SessionConfig, UpdateRequest,
};
use futures_util::FutureExt as _;
use tempfile::TempDir;
use uuid::Uuid;

const DM_DATABASE_TYPE: &str = "DM";
const DM_DRIVER_CLASS: &str = "dm.jdbc.driver.DmDriver";
const DM_DRIVER_VERSION: &str = "8.1.2.141";
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);
const DM_RUNTIME_PREREQUISITE_ENV: [&str; 2] =
    ["CHAT2DB_JAVA_ENGINE_JAR", "DM_TEST_DRIVER_PACK_DIR"];
const REQUIRED_DM_ENDPOINT_ENV: [&str; 4] = [
    "DM_TEST_HOST",
    "DM_TEST_PORT",
    "DM_TEST_USER",
    "DM_TEST_PASSWORD",
];

struct DmProductHarness {
    _directory: TempDir,
    host: RuntimeHost,
    application: Application,
}

struct DmTestConfig {
    user: String,
    password: String,
    jdbc_url: String,
    database_name: String,
    schema_name: String,
}

struct DmFixture {
    datasource: Datasource,
    session: Session,
    schema_name: String,
    table_name: String,
}

impl DmProductHarness {
    async fn start() -> Self {
        let engine_jar = required_file("CHAT2DB_JAVA_ENGINE_JAR");
        let driver_pack_dir = required_directory("DM_TEST_DRIVER_PACK_DIR");
        let directory = TempDir::new().expect("temporary DM product directory");
        let engine = EngineConfig::new(EngineCommand::java_jar("java", engine_jar)).with_timeouts(
            Duration::from_secs(20),
            Duration::from_secs(20),
            Duration::from_secs(10),
        );
        let runtime = RuntimeConfig::new(engine)
            .with_data_dir(directory.path().join("data"))
            .with_driver_pack_dir(driver_pack_dir)
            .with_vault_master_key_base64(STANDARD.encode([0x44; 32]));
        let host = RuntimeHost::open(runtime)
            .await
            .expect("DM product runtime must discover the managed driver pack");
        let application = host.application();
        Self {
            _directory: directory,
            host,
            application,
        }
    }

    async fn shutdown(mut self) {
        self.host
            .shutdown()
            .await
            .expect("DM product runtime must shut down cleanly");
    }
}

impl DmTestConfig {
    fn from_environment() -> Option<Self> {
        let required = dm_test_required();
        let configured = REQUIRED_DM_ENDPOINT_ENV
            .iter()
            .filter(|name| std::env::var_os(name).is_some())
            .count();
        if configured == 0 {
            assert!(
                !required,
                "DM_TEST_REQUIRED is enabled but the real DM endpoint variables are absent"
            );
            eprintln!(
                "skipping real DM product integration test; DM_TEST_* endpoint variables are absent"
            );
            return None;
        }
        assert_eq!(
            configured,
            REQUIRED_DM_ENDPOINT_ENV.len(),
            "real DM integration is partially configured; set every required DM_TEST_* endpoint variable"
        );

        let host = required_text("DM_TEST_HOST");
        assert!(
            !host.trim().is_empty()
                && !host.chars().any(char::is_control)
                && !host.contains(['/', '?', '#']),
            "DM_TEST_HOST must contain only a JDBC host name or address"
        );
        let port = required_text("DM_TEST_PORT")
            .parse::<u16>()
            .expect("DM_TEST_PORT must be a decimal TCP port");
        assert_ne!(port, 0, "DM_TEST_PORT cannot be zero");
        let user = required_text("DM_TEST_USER");
        assert!(!user.is_empty(), "DM_TEST_USER cannot be empty");
        let jdbc_host = if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
            format!("[{host}]")
        } else {
            host.clone()
        };
        let jdbc_url = std::env::var("DM_TEST_JDBC_URL")
            .unwrap_or_else(|_| format!("jdbc:dm://{jdbc_host}:{port}/"));
        assert!(
            jdbc_url.starts_with("jdbc:dm://") && !jdbc_url.chars().any(char::is_control),
            "DM_TEST_JDBC_URL must be a valid DM JDBC URL"
        );
        let database_name = optional_text("DM_TEST_DATABASE").unwrap_or_default();
        assert!(
            !database_name.chars().any(char::is_control),
            "DM_TEST_DATABASE cannot contain control characters"
        );
        let schema_name =
            optional_text("DM_TEST_SCHEMA").unwrap_or_else(|| user.to_ascii_uppercase());
        assert_identifier(&schema_name, "DM_TEST_SCHEMA");

        Some(Self {
            user,
            password: required_text("DM_TEST_PASSWORD"),
            jdbc_url,
            database_name,
            schema_name,
        })
    }

    fn connection(&self) -> DatasourceConnection {
        DatasourceConnection {
            jdbc_url: self.jdbc_url.clone(),
            properties: vec![
                DatasourceConnectionProperty {
                    key: "user".to_owned(),
                    value: self.user.clone(),
                    sensitive: false,
                },
                DatasourceConnectionProperty {
                    key: "password".to_owned(),
                    value: self.password.clone(),
                    sensitive: true,
                },
            ],
            read_only: false,
            ssh: None,
        }
    }

    fn bridge_properties(&self) -> Vec<ConnectionProperty> {
        vec![
            ConnectionProperty {
                key: "user".to_owned(),
                value: self.user.clone(),
                sensitive: false,
            },
            ConnectionProperty {
                key: "password".to_owned(),
                value: self.password.clone(),
                sensitive: true,
            },
        ]
    }
}

#[tokio::test]
async fn managed_dm_pack_maps_to_the_jdbc_backed_spi() {
    if !dm_runtime_prerequisites_available() {
        return;
    }
    let harness = DmProductHarness::start().await;
    let driver = managed_dm_driver(&harness.application);
    verify_dm_driver_compatibility(&harness.application, &driver);
    verify_legacy_compatibility_disabled(&harness.application);
    harness.shutdown().await;
}

#[tokio::test]
async fn managed_dm_pack_exercises_real_metadata_preview_and_retained_result() {
    let Some(config) = DmTestConfig::from_environment() else {
        return;
    };
    let harness = DmProductHarness::start().await;
    let driver = managed_dm_driver(&harness.application);
    verify_dm_driver_compatibility(&harness.application, &driver);
    verify_legacy_compatibility_disabled(&harness.application);
    if let Err(error) = harness
        .application
        .test_datasource_connection(&driver.driver_id, config.connection())
        .await
    {
        harness.shutdown().await;
        panic!("managed DM driver must open and close a real JDBC connection: {error}");
    }
    let engine_lease = match harness.host.acquire_engine().await {
        Ok(engine_lease) => engine_lease,
        Err(error) => {
            harness.shutdown().await;
            panic!("real DM integration must start the Java engine: {error}");
        }
    };
    let driver_client = engine_lease
        .driver_client()
        .expect("running Java engine must expose JDBC");
    let fixture =
        match provision_fixture(&config, &harness.application, &driver_client, &driver).await {
            Ok(fixture) => fixture,
            Err(error) => {
                drop(driver_client);
                drop(engine_lease);
                harness.shutdown().await;
                panic!("real DM fixture setup failed: {error}");
            }
        };
    let verification =
        AssertUnwindSafe(verify_live_product(&config, &harness.application, &fixture))
            .catch_unwind()
            .await;
    let cleanup_errors = cleanup_fixture(fixture).await;
    drop(driver_client);
    drop(engine_lease);
    harness.shutdown().await;

    if let Err(payload) = verification {
        for error in cleanup_errors {
            eprintln!("DM integration cleanup also failed: {error}");
        }
        std::panic::resume_unwind(payload);
    }
    assert!(
        cleanup_errors.is_empty(),
        "DM integration cleanup failed: {}",
        cleanup_errors.join("; ")
    );
}

fn managed_dm_driver(application: &Application) -> JdbcDriver {
    let mut drivers = application
        .list_drivers()
        .items
        .into_iter()
        .filter(|driver| driver.pack_id == "dm");
    let driver = drivers
        .next()
        .expect("managed DM driver pack must be discovered");
    assert!(drivers.next().is_none(), "DM driver pack must be unique");
    assert_eq!(driver.version, DM_DRIVER_VERSION);
    assert_eq!(driver.driver_class, DM_DRIVER_CLASS);
    assert_eq!(driver.artifact_count, 1);
    assert!(driver.driver_id.starts_with("sha256:"));
    driver
}

fn verify_dm_driver_compatibility(application: &Application, driver: &JdbcDriver) {
    let compatibility = application
        .native_driver_compatibility(DM_DATABASE_TYPE, NativeDriverAction::Download)
        .expect("DM must resolve through the JDBC-backed driver SPI");
    assert_eq!(compatibility.database_type, DM_DATABASE_TYPE);
    assert_eq!(compatibility.driver_id, driver.driver_id);
    assert_eq!(compatibility.implementation, "dm-jdbc");
    assert!(compatibility.artifact_required);
    assert!(!compatibility.changed);
}

fn verify_legacy_compatibility_disabled(application: &Application) {
    let compatibility = application
        .health()
        .components
        .into_iter()
        .find(|component| component.id == "community-compatibility")
        .expect("legacy compatibility health must be explicit");
    assert_eq!(compatibility.state, ComponentState::Disabled);
}

async fn provision_fixture(
    config: &DmTestConfig,
    application: &Application,
    driver_client: &DriverClient,
    driver: &JdbcDriver,
) -> Result<DmFixture, String> {
    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "DM product integration".to_owned(),
            driver_id: driver.driver_id.clone(),
            connection: Some(config.connection()),
        })
        .await
        .map_err(|error| format!("persist DM datasource: {error}"))?;
    let session = driver_client
        .open_session(SessionConfig {
            driver_id: driver.driver_id.clone(),
            jdbc_url: config.jdbc_url.clone(),
            properties: config.bridge_properties(),
            read_only: false,
        })
        .await
        .map_err(|error| format!("open fixture session: {error}"))?;
    let table_name = format!("CHAT2DB_DM_IT_{}", Uuid::new_v4().simple()).to_ascii_uppercase();
    let table = qualified_name(&config.schema_name, &table_name);
    let create_sql = format!(
        "CREATE TABLE {table} (\"ID\" BIGINT NOT NULL PRIMARY KEY, \"LABEL\" VARCHAR(128) NOT NULL)"
    );
    if let Err(error) = try_execute_update(&session, &create_sql).await {
        let close_error = session.close().await.err();
        return Err(format!(
            "create fixture table: {error}; session cleanup: {close_error:?}"
        ));
    }
    let insert_sql = format!("INSERT INTO {table} (\"ID\", \"LABEL\") VALUES (1, 'dm-fixture')");
    if let Err(error) = try_execute_update(&session, &insert_sql).await {
        let drop_error = try_execute_update(&session, &format!("DROP TABLE {table}"))
            .await
            .err();
        let close_error = session.close().await.err();
        return Err(format!(
            "insert fixture row: {error}; table cleanup: {drop_error:?}; session cleanup: {close_error:?}"
        ));
    }
    Ok(DmFixture {
        datasource,
        session,
        schema_name: config.schema_name.clone(),
        table_name,
    })
}

async fn verify_live_product(
    config: &DmTestConfig,
    application: &Application,
    fixture: &DmFixture,
) {
    let databases = application
        .list_native_databases(
            DM_DATABASE_TYPE,
            ListDatabasesRequest {
                datasource_id: fixture.datasource.id.clone(),
            },
        )
        .await
        .expect("DM driver SPI must list the current database directly");
    assert!(
        !databases.items.is_empty()
            && databases
                .items
                .iter()
                .all(|database| !database.name.trim().is_empty()),
        "DM database metadata must contain a named current database"
    );

    let schemas = application
        .list_native_schemas(
            DM_DATABASE_TYPE,
            ListSchemasRequest {
                datasource_id: fixture.datasource.id.clone(),
                database_name: config.database_name.clone(),
            },
        )
        .await
        .expect("DM driver SPI must list real schemas directly");
    assert!(
        schemas
            .items
            .iter()
            .any(|schema| schema.name.eq_ignore_ascii_case(&config.schema_name)),
        "DM metadata omitted configured schema {}",
        config.schema_name
    );

    let tables = application
        .list_native_tables(
            DM_DATABASE_TYPE,
            ListTablesRequest {
                scope: MetadataScope {
                    datasource_id: fixture.datasource.id.clone(),
                    database_name: config.database_name.clone(),
                    schema_name: config.schema_name.clone(),
                },
                name_pattern: fixture.table_name.clone(),
            },
        )
        .await
        .expect("DM driver SPI must list the fixture table directly");
    assert!(
        tables
            .items
            .iter()
            .any(|table| table.name.eq_ignore_ascii_case(&fixture.table_name)),
        "DM metadata omitted fixture table {}",
        fixture.table_name
    );

    let columns = application
        .list_native_columns(
            DM_DATABASE_TYPE,
            ListColumnsRequest {
                table: TableRef {
                    scope: MetadataScope {
                        datasource_id: fixture.datasource.id.clone(),
                        database_name: config.database_name.clone(),
                        schema_name: config.schema_name.clone(),
                    },
                    table_name: fixture.table_name.clone(),
                },
            },
        )
        .await
        .expect("DM driver SPI must list fixture columns directly");
    for expected in ["ID", "LABEL"] {
        assert!(
            columns
                .items
                .iter()
                .any(|column| column.name.eq_ignore_ascii_case(expected)),
            "DM metadata omitted fixture column {expected}"
        );
    }

    verify_table_preview(config, application, fixture).await;
}

async fn verify_table_preview(
    config: &DmTestConfig,
    application: &Application,
    fixture: &DmFixture,
) {
    let table = qualified_name(&config.schema_name, &fixture.table_name);
    let preview = application
        .start_native_table_preview(
            DM_DATABASE_TYPE,
            TablePreviewRequest {
                table: TableRef {
                    scope: MetadataScope {
                        datasource_id: fixture.datasource.id.clone(),
                        database_name: config.database_name.clone(),
                        schema_name: config.schema_name.clone(),
                    },
                    table_name: fixture.table_name.clone(),
                },
            },
            1,
        )
        .await
        .expect("DM driver SPI must accept a bounded table preview directly");
    assert_eq!(preview.row_limit, 1);
    assert!(preview.sql.contains(&table));
    assert!(preview.sql.to_ascii_uppercase().contains("LIMIT 1"));

    let result_id = wait_for_result(application, &preview.operation_id).await;
    let page = application
        .result_page(
            &result_id,
            ResultPageRequest {
                offset: "0".to_owned(),
                max_rows: "1".to_owned(),
                max_bytes: (1024_u64 * 1024).to_string(),
            },
        )
        .await
        .expect("DM table preview result must be retained");
    assert_eq!(page.metadata.row_count, "1");
    assert_eq!(page.rows.len(), 1);
    assert!(matches!(
        page.rows[0].values.as_slice(),
        [JdbcValue::SignedInteger { value: id }, JdbcValue::Text { value: label }]
            if id == "1" && label == "dm-fixture"
    ));
}

async fn cleanup_fixture(fixture: DmFixture) -> Vec<String> {
    let mut errors = Vec::new();
    let table = qualified_name(&fixture.schema_name, &fixture.table_name);
    if let Err(error) = try_execute_update(&fixture.session, &format!("DROP TABLE {table}")).await {
        errors.push(format!("drop fixture table: {error}"));
    }
    if let Err(error) = fixture.session.close().await {
        errors.push(format!("close fixture session: {error}"));
    }
    errors
}

async fn wait_for_result(application: &Application, operation_id: &str) -> String {
    let mut events = application
        .subscribe_operation(operation_id, Some(0))
        .await
        .expect("DM operation subscription must open");
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, events.next_event())
            .await
            .expect("DM operation event must arrive")
            .expect("DM operation event stream must remain valid")
            .expect("DM operation must emit a terminal event");
        match event.event {
            OperationEvent::Started | OperationEvent::Progress { .. } => {}
            OperationEvent::Completed { result } => return result.id,
            OperationEvent::Failed { error } => panic!("DM preview failed: {error:?}"),
            OperationEvent::Cancelled { reason } => {
                panic!("DM preview was cancelled: {reason:?}")
            }
        }
    }
}

async fn try_execute_update(session: &Session, sql: &str) -> Result<u64, BridgeError> {
    session
        .execute_update(UpdateRequest {
            sql: sql.to_owned(),
            parameters: Vec::new(),
            transaction_id: None,
        })
        .await
        .map(|result| result.affected_rows)
}

fn qualified_name(schema_name: &str, object_name: &str) -> String {
    format!(
        "{}.{}",
        quote_identifier(schema_name),
        quote_identifier(object_name)
    )
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn assert_identifier(value: &str, variable: &str) {
    assert!(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'#')),
        "{variable} must be a non-empty DM identifier"
    );
}

fn dm_test_required() -> bool {
    match std::env::var("DM_TEST_REQUIRED") {
        Err(std::env::VarError::NotPresent) => false,
        Ok(value) if value == "1" || value.eq_ignore_ascii_case("true") => true,
        Ok(value) if value == "0" || value.eq_ignore_ascii_case("false") => false,
        Ok(_) | Err(std::env::VarError::NotUnicode(_)) => {
            panic!("DM_TEST_REQUIRED must be 1, 0, true, or false")
        }
    }
}

fn dm_runtime_prerequisites_available() -> bool {
    let missing = DM_RUNTIME_PREREQUISITE_ENV
        .iter()
        .copied()
        .filter(|variable| std::env::var_os(variable).is_none())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return true;
    }
    eprintln!(
        "skipping managed DM pack integration test; missing runtime prerequisites: {}",
        missing.join(", ")
    );
    false
}

fn optional_text(variable: &str) -> Option<String> {
    std::env::var(variable)
        .ok()
        .filter(|value| !value.is_empty())
}

fn required_file(variable: &str) -> PathBuf {
    let path = std::env::var_os(variable).map_or_else(
        || panic!("{variable} must point to a packaged JAR"),
        PathBuf::from,
    );
    assert!(path.is_file(), "{variable} does not point to a file");
    path
}

fn required_directory(variable: &str) -> PathBuf {
    let path = std::env::var_os(variable).map_or_else(
        || panic!("{variable} must point to a driver-pack directory"),
        PathBuf::from,
    );
    assert!(path.is_dir(), "{variable} does not point to a directory");
    path
}

fn required_text(variable: &str) -> String {
    std::env::var(variable).unwrap_or_else(|_| panic!("{variable} must be configured"))
}
