use std::{panic::AssertUnwindSafe, path::PathBuf, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    BuildCommunityDmlRequest, BuildCommunityNamespaceSqlRequest, CommunityDatabase,
    CommunityDmlAssignment, CommunityDmlColumn, CommunityDmlRow, CommunityDmlStatement,
    CommunityDmlTarget, CommunityDmlTemporalKind, CommunityDmlValue,
    CommunityNamespaceSqlOperation, CompleteCommunitySqlRequest, ComponentState,
    CreateDatasourceRequest, Datasource, DatasourceConnection, DatasourceConnectionProperty,
    DatasourceSecretChange, FormatCommunitySqlRequest, JdbcValue, ListCommunityColumnsRequest,
    ListCommunityDatabasesRequest, ListCommunityIndexesRequest, ListCommunityTablesRequest,
    OperationEvent, ParseCommunitySqlRequest, QueryLimits, ResultPageRequest,
    StartCommunityTablePreviewRequest, StartQueryRequest, UpdateDatasourceRequest,
    ValidateCommunitySqlRequest,
};
use chat2db_core::{Application, RuntimeConfig, RuntimeHost, load_fixed_community_classpath};
use chat2db_java_bridge::{
    BridgeError, ConnectionProperty, DriverClient, EngineCommand, EngineConfig, Session,
    SessionConfig, UpdateRequest,
};
use futures_util::FutureExt as _;
use tempfile::TempDir;
use uuid::Uuid;

const COMMUNITY_COMMIT: &str = "f275e08d774f839612374e991d09c5e6ea2d8b57";
const MYSQL_DATABASE_TYPE: &str = "MYSQL";
const MYSQL_DRIVER_CLASS: &str = "com.mysql.cj.jdbc.Driver";
const MYSQL_DRIVER_VERSION: &str = "8.0.30";
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);
const REQUIRED_MYSQL_ENV: [&str; 5] = [
    "MYSQL_TEST_HOST",
    "MYSQL_TEST_PORT",
    "MYSQL_TEST_USER",
    "MYSQL_TEST_PASSWORD",
    "MYSQL_TEST_DRIVER_PACK_DIR",
];

struct MysqlTestConfig {
    host: String,
    port: u16,
    user: String,
    password: String,
    jdbc_parameters: String,
    driver_pack_dir: PathBuf,
}

struct MysqlProductHarness {
    _directory: TempDir,
    host: RuntimeHost,
    application: Application,
    driver: DriverClient,
    driver_id: String,
}

struct MysqlDatabaseFixture {
    datasource: Datasource,
    database_name: String,
    drop_database_sql: String,
    server_session: Session,
}

impl MysqlTestConfig {
    fn from_environment() -> Option<Self> {
        let required = mysql_test_required();
        let configured = REQUIRED_MYSQL_ENV
            .iter()
            .filter(|name| std::env::var_os(name).is_some())
            .count();
        if configured == 0 {
            assert!(
                !required,
                "MYSQL_TEST_REQUIRED is enabled but the real MySQL endpoint variables are absent"
            );
            eprintln!(
                "skipping real MySQL product integration test; MYSQL_TEST_* endpoint variables are absent"
            );
            return None;
        }
        assert_eq!(
            configured,
            REQUIRED_MYSQL_ENV.len(),
            "real MySQL integration is partially configured; set every required MYSQL_TEST_* variable"
        );

        let host = required_env_text("MYSQL_TEST_HOST");
        assert!(
            !host.trim().is_empty()
                && !host.chars().any(char::is_control)
                && !host.contains(['/', '?', '#']),
            "MYSQL_TEST_HOST must contain only a JDBC host name or address"
        );
        let port_text = required_env_text("MYSQL_TEST_PORT");
        let port = port_text
            .parse::<u16>()
            .expect("MYSQL_TEST_PORT must be a decimal TCP port");
        assert_ne!(port, 0, "MYSQL_TEST_PORT cannot be zero");
        let user = required_env_text("MYSQL_TEST_USER");
        assert!(!user.is_empty(), "MYSQL_TEST_USER cannot be empty");
        let password = required_env_text("MYSQL_TEST_PASSWORD");
        let driver_pack_dir = PathBuf::from(required_env_text("MYSQL_TEST_DRIVER_PACK_DIR"));
        assert!(
            driver_pack_dir.is_dir(),
            "MYSQL_TEST_DRIVER_PACK_DIR must point to a prepared driver-pack root"
        );
        let jdbc_parameters = std::env::var("MYSQL_TEST_JDBC_PARAMETERS").unwrap_or_else(|_| {
            assert!(
                is_loopback_host(&host),
                "MYSQL_TEST_JDBC_PARAMETERS must explicitly configure TLS for a non-loopback host"
            );
            "sslMode=DISABLED&allowPublicKeyRetrieval=true&serverTimezone=UTC&zeroDateTimeBehavior=CONVERT_TO_NULL&tinyInt1isBit=false"
                .to_owned()
        });
        assert!(
            !jdbc_parameters.chars().any(char::is_control),
            "MYSQL_TEST_JDBC_PARAMETERS cannot contain control characters"
        );

        Some(Self {
            host,
            port,
            user,
            password,
            jdbc_parameters,
            driver_pack_dir,
        })
    }

    fn jdbc_url(&self, database_name: Option<&str>) -> String {
        let host = if self.host.contains(':')
            && !(self.host.starts_with('[') && self.host.ends_with(']'))
        {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        let parameters = self.jdbc_parameters.trim().trim_start_matches(['?', '&']);
        let query = if parameters.is_empty() {
            String::new()
        } else {
            format!("?{parameters}")
        };
        format!(
            "jdbc:mysql://{host}:{}/{database_name}{query}",
            self.port,
            database_name = database_name.unwrap_or_default()
        )
    }

    fn product_properties(&self) -> Vec<DatasourceConnectionProperty> {
        vec![
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
        ]
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

impl MysqlProductHarness {
    async fn start(config: &MysqlTestConfig) -> Self {
        let engine_jar = required_path("CHAT2DB_JAVA_ENGINE_JAR");
        let community_directory = required_path("CHAT2DB_COMMUNITY_CLASSPATH_DIR");
        let classpath = load_fixed_community_classpath(&community_directory)
            .expect("Community distribution must exactly match the embedded lock");
        let directory = TempDir::new().expect("temporary product data directory");
        let data_dir = directory.path().join("data");
        let engine = EngineConfig::new(EngineCommand::java_jar("java", engine_jar))
            .with_community_classpath(classpath)
            .with_timeouts(
                Duration::from_secs(20),
                Duration::from_secs(20),
                Duration::from_secs(10),
            );
        let runtime = RuntimeConfig::new(engine)
            .with_data_dir(&data_dir)
            .with_driver_pack_dir(&config.driver_pack_dir)
            .with_vault_master_key_base64(STANDARD.encode([0x4d; 32]));
        let host = RuntimeHost::open(runtime)
            .await
            .expect("managed MySQL product runtime must start");
        let application = host.application();
        let driver = host
            .engine_client()
            .expect("running host must expose the Java engine")
            .driver_client()
            .expect("running engine must expose JDBC");

        let drivers = application.list_drivers();
        assert_eq!(
            drivers.items.len(),
            1,
            "test pack root must contain only MySQL"
        );
        let installed = &drivers.items[0];
        assert_eq!(installed.pack_id, "mysql");
        assert_eq!(installed.version, MYSQL_DRIVER_VERSION);
        assert_eq!(installed.driver_class, MYSQL_DRIVER_CLASS);
        assert_eq!(installed.artifact_count, 1);
        let driver_id = installed.driver_id.clone();

        Self {
            _directory: directory,
            host,
            application,
            driver,
            driver_id,
        }
    }
}

#[tokio::test]
async fn managed_mysql_pack_exercises_the_first_real_database_vertical() {
    let Some(config) = MysqlTestConfig::from_environment() else {
        return;
    };
    let harness = MysqlProductHarness::start(&config).await;
    verify_community_catalog(&harness.application).await;
    let fixture = provision_mysql_database(&config, &harness).await;
    let verification = AssertUnwindSafe(verify_database_vertical(&config, &harness, &fixture))
        .catch_unwind()
        .await;
    let cleanup_errors = cleanup_mysql_database(harness, fixture).await;

    if let Err(payload) = verification {
        for error in cleanup_errors {
            eprintln!("MySQL integration cleanup also failed: {error}");
        }
        std::panic::resume_unwind(payload);
    }
    assert!(
        cleanup_errors.is_empty(),
        "MySQL integration cleanup failed: {}",
        cleanup_errors.join("; ")
    );
}

async fn provision_mysql_database(
    config: &MysqlTestConfig,
    harness: &MysqlProductHarness,
) -> MysqlDatabaseFixture {
    let server_url = config.jdbc_url(None);
    let datasource = harness
        .application
        .create_datasource(CreateDatasourceRequest {
            name: "MySQL first vertical".to_owned(),
            driver_id: harness.driver_id.clone(),
            connection: Some(DatasourceConnection {
                jdbc_url: server_url.clone(),
                properties: config.product_properties(),
                read_only: false,
            }),
        })
        .await
        .expect("managed MySQL datasource must be created");
    assert!(datasource.has_secret);

    let database_name = format!("chat2db_rust_it_{}", Uuid::new_v4().simple());
    assert_database_presence(&harness.application, &datasource.id, &database_name, false).await;
    let (create_database_sql, drop_database_sql) =
        build_mysql_database_sqls(&harness.application, &database_name).await;
    assert_database_presence(&harness.application, &datasource.id, &database_name, false).await;

    let server_session = open_session(
        &harness.driver,
        &harness.driver_id,
        &server_url,
        config.bridge_properties(),
    )
    .await;
    execute_update(&server_session, &create_database_sql).await;

    MysqlDatabaseFixture {
        datasource,
        database_name,
        drop_database_sql,
        server_session,
    }
}

async fn build_mysql_database_sqls(
    application: &Application,
    database_name: &str,
) -> (String, String) {
    let create_database_sql = application
        .build_community_namespace_sql(BuildCommunityNamespaceSqlRequest {
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            operation: CommunityNamespaceSqlOperation::CreateDatabase {
                database: CommunityDatabase {
                    name: database_name.to_owned(),
                    charset: "utf8mb4".to_owned(),
                    ..CommunityDatabase::default()
                },
            },
        })
        .await
        .expect("Core must invoke the retained MySQL database-create builder")
        .sql;
    assert_mysql_create_database_sql(&create_database_sql, database_name);

    let use_database_sql = application
        .build_community_namespace_sql(BuildCommunityNamespaceSqlRequest {
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            operation: CommunityNamespaceSqlOperation::UseDatabase {
                database_name: database_name.to_owned(),
            },
        })
        .await
        .expect("Core must invoke the retained MySQL database-use builder")
        .sql;
    assert_eq!(use_database_sql, format!("USE `{database_name}`"));

    let alter_error = application
        .build_community_namespace_sql(BuildCommunityNamespaceSqlRequest {
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            operation: CommunityNamespaceSqlOperation::AlterDatabase {
                old_database: CommunityDatabase {
                    name: database_name.to_owned(),
                    ..CommunityDatabase::default()
                },
                new_database: CommunityDatabase {
                    name: database_name.to_owned(),
                    charset: "utf8mb4".to_owned(),
                    ..CommunityDatabase::default()
                },
            },
        })
        .await
        .expect_err("fixed Community MySQL explicitly does not implement ALTER DATABASE");
    assert_eq!(
        alter_error.api_error().code,
        "community.namespace_builder_not_supported"
    );

    let drop_database_sql = application
        .build_community_namespace_sql(BuildCommunityNamespaceSqlRequest {
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            operation: CommunityNamespaceSqlOperation::DropDatabase {
                database_name: database_name.to_owned(),
            },
        })
        .await
        .expect("Core must invoke the retained MySQL database-drop builder")
        .sql;
    assert!(drop_database_sql.contains(&format!("`{database_name}`")));
    (create_database_sql, drop_database_sql)
}

async fn verify_database_vertical(
    config: &MysqlTestConfig,
    harness: &MysqlProductHarness,
    fixture: &MysqlDatabaseFixture,
) {
    let datasource = &fixture.datasource;
    let database_name = &fixture.database_name;
    assert_database_presence(&harness.application, &datasource.id, database_name, true).await;

    let database_url = config.jdbc_url(Some(database_name));
    let updated = harness
        .application
        .update_datasource(
            &datasource.id,
            UpdateDatasourceRequest {
                expected_revision: datasource.revision.clone(),
                name: "MySQL first vertical ready".to_owned(),
                driver_id: harness.driver_id.clone(),
                secret_change: DatasourceSecretChange::Replace {
                    connection: DatasourceConnection {
                        jdbc_url: database_url.clone(),
                        properties: config.product_properties(),
                        read_only: false,
                    },
                },
            },
        )
        .await
        .expect("managed MySQL datasource must be updated");
    assert_eq!(updated.name, "MySQL first vertical ready");
    assert_ne!(updated.revision, datasource.revision);
    assert_eq!(
        harness
            .application
            .get_datasource(&datasource.id)
            .await
            .expect("updated datasource must remain queryable"),
        updated
    );

    create_items_table(&fixture.server_session, database_name).await;

    verify_mysql_metadata(&harness.application, &datasource.id, database_name).await;
    verify_typed_dml(&harness.application, &fixture.server_session, database_name).await;
    verify_table_preview(&harness.application, &datasource.id, database_name).await;
    verify_sql_tools(&harness.application, &datasource.id, database_name).await;
    verify_product_query(&harness.application, &datasource.id).await;
}

async fn cleanup_mysql_database(
    mut harness: MysqlProductHarness,
    fixture: MysqlDatabaseFixture,
) -> Vec<String> {
    let mut errors = Vec::new();
    if let Err(error) =
        try_execute_update(&fixture.server_session, &fixture.drop_database_sql).await
    {
        errors.push(format!("drop database: {error}"));
    }
    if let Err(error) = fixture.server_session.close().await {
        errors.push(format!("close server session: {error}"));
    }
    if let Err(error) = harness.host.shutdown().await {
        errors.push(format!("shutdown runtime host: {error}"));
    }
    errors
}

async fn create_items_table(session: &Session, database_name: &str) {
    execute_update(
        session,
        &format!(
            "CREATE TABLE `{database_name}`.`items` (\
             `id` BIGINT NOT NULL, \
             `label` VARCHAR(128) NOT NULL, \
             `amount` DECIMAL(12,2) NOT NULL, \
             `active` BOOLEAN NOT NULL, \
             `created_at` DATETIME NOT NULL, \
             PRIMARY KEY (`id`), \
             UNIQUE KEY `idx_items_label` (`label`)\
         ) ENGINE=InnoDB"
        ),
    )
    .await;
}

fn assert_mysql_create_database_sql(sql: &str, database_name: &str) {
    assert!(sql.contains(&format!("`{database_name}`")));
    assert!(sql.to_ascii_lowercase().contains("utf8mb4"));
}

async fn verify_community_catalog(application: &Application) {
    let health = application
        .health()
        .components
        .into_iter()
        .find(|component| component.id == "community-compatibility")
        .expect("Community compatibility health must be explicit");
    assert_eq!(health.state, ComponentState::Ready);
    let catalog = application
        .list_community_plugins()
        .await
        .expect("Core must expose the fixed Community plugin catalog");
    assert_eq!(catalog.source_commit, COMMUNITY_COMMIT);
    let mysql = catalog
        .plugins
        .iter()
        .find(|plugin| plugin.database_type == MYSQL_DATABASE_TYPE)
        .expect("fixed Community classpath must contain the MySQL plugin");
    assert!(mysql.behavior.supports_database);
    assert!(!mysql.behavior.supports_schema);
    assert!(mysql.services.metadata_available);
    assert!(mysql.services.sql_builder_available);
    assert!(mysql.services.sql_parser_available);
    assert!(mysql.services.dml_builder_available);
    assert!(mysql.services.dql_builder_available);
    assert!(mysql.services.value_processor_available);
    assert!(mysql.services.identifier_processor_available);
}

async fn assert_database_presence(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    expected: bool,
) {
    let databases = application
        .list_community_databases(ListCommunityDatabasesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
        })
        .await
        .expect("Core must list real MySQL databases");
    assert_eq!(
        databases
            .items
            .iter()
            .any(|database| database.name == database_name),
        expected,
        "unexpected MySQL database presence for {database_name}"
    );
}

async fn verify_mysql_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let tables = application
        .list_community_tables(ListCommunityTablesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            table_name_pattern: String::new(),
        })
        .await
        .expect("Core must list real MySQL tables");
    let table = tables
        .items
        .iter()
        .find(|table| table.name.eq_ignore_ascii_case("items"))
        .expect("created MySQL table must be projected");
    assert_eq!(table.database_name, database_name);

    let columns = application
        .list_community_columns(ListCommunityColumnsRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            table_name: table.name.clone(),
        })
        .await
        .expect("Core must list real MySQL columns");
    for expected in ["id", "label", "amount", "active", "created_at"] {
        assert!(
            columns
                .items
                .iter()
                .any(|column| column.name.eq_ignore_ascii_case(expected)),
            "MySQL metadata omitted column {expected}"
        );
    }

    let indexes = application
        .list_community_indexes(ListCommunityIndexesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            table_name: table.name.clone(),
        })
        .await
        .expect("Core must list real MySQL indexes");
    assert!(indexes.items.iter().any(|index| {
        index.name.eq_ignore_ascii_case("primary")
            && index.unique == Some(true)
            && index
                .columns
                .iter()
                .any(|column| column.column_name.eq_ignore_ascii_case("id"))
    }));
    assert!(indexes.items.iter().any(|index| {
        index.name.eq_ignore_ascii_case("idx_items_label")
            && index.unique == Some(true)
            && index
                .columns
                .iter()
                .any(|column| column.column_name.eq_ignore_ascii_case("label"))
    }));
}

async fn verify_typed_dml(application: &Application, session: &Session, database_name: &str) {
    let columns = vec![
        dml_column("id", "BIGINT", None, None),
        dml_column("label", "VARCHAR", Some(128), None),
        dml_column("amount", "DECIMAL", Some(12), Some(2)),
        dml_column("active", "BOOLEAN", None, None),
        dml_column("created_at", "DATETIME", None, None),
    ];
    let target = CommunityDmlTarget {
        database_name: Some(database_name.to_owned()),
        schema_name: None,
        table_name: "items".to_owned(),
    };
    let insert_sql = application
        .build_community_dml(BuildCommunityDmlRequest {
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            target: target.clone(),
            statement: CommunityDmlStatement::SingleInsert {
                columns,
                row: CommunityDmlRow {
                    values: vec![
                        CommunityDmlValue::Decimal {
                            value: "1".to_owned(),
                        },
                        CommunityDmlValue::String {
                            value: "O'Brien".to_owned(),
                        },
                        CommunityDmlValue::Decimal {
                            value: "12.50".to_owned(),
                        },
                        CommunityDmlValue::Boolean { value: true },
                        CommunityDmlValue::Temporal {
                            temporal_kind: CommunityDmlTemporalKind::LocalDatetime,
                            value: "2026-07-27T12:34:56".to_owned(),
                        },
                    ],
                },
            },
        })
        .await
        .expect("Core must generate typed MySQL INSERT SQL")
        .sql;
    assert!(insert_sql.contains("O''Brien"));
    execute_update(session, &insert_sql).await;

    let update_sql = application
        .build_community_dml(BuildCommunityDmlRequest {
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            target,
            statement: CommunityDmlStatement::Update {
                assignments: vec![
                    dml_assignment(
                        "label",
                        "VARCHAR",
                        CommunityDmlValue::String {
                            value: "mysql-ready".to_owned(),
                        },
                    ),
                    dml_assignment(
                        "amount",
                        "DECIMAL",
                        CommunityDmlValue::Decimal {
                            value: "99.99".to_owned(),
                        },
                    ),
                ],
                predicates: vec![dml_assignment(
                    "id",
                    "BIGINT",
                    CommunityDmlValue::Decimal {
                        value: "1".to_owned(),
                    },
                )],
            },
        })
        .await
        .expect("Core must generate typed MySQL UPDATE SQL")
        .sql;
    assert_eq!(execute_update(session, &update_sql).await, 1);
}

async fn verify_sql_tools(application: &Application, datasource_id: &str, database_name: &str) {
    let validation = application
        .validate_community_sql(ValidateCommunitySqlRequest {
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            sql: "SELECT FROM;".to_owned(),
        })
        .await
        .expect("Core must invoke real MySQL SQL validation");
    assert!(!validation.valid);
    assert!(!validation.diagnostics.is_empty());

    let source_sql = "select id,label from items where id=1";
    let formatted = application
        .format_community_sql(FormatCommunitySqlRequest {
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            sql: source_sql.to_owned(),
        })
        .await
        .expect("Core must invoke real MySQL SQL formatting");
    assert_ne!(formatted.sql, source_sql);
    assert!(formatted.sql.to_ascii_lowercase().contains("from"));

    let parsed = application
        .parse_community_sql(ParseCommunitySqlRequest {
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            sql: "SELECT 1; UPDATE items SET active = TRUE WHERE id = 1;".to_owned(),
        })
        .await
        .expect("Core must invoke the real MySQL parser");
    assert_eq!(parsed.statements.len(), 2);

    let table_sql = "select * from ";
    let tables = application
        .complete_community_sql(CompleteCommunitySqlRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            sql: table_sql.to_owned(),
            cursor_utf16: utf16_len(table_sql),
            min_prefix_length: 0,
            need_full_name: false,
            keyword_case: "UPPER".to_owned(),
            active_snippet_slot: None,
        })
        .await
        .expect("Core must expose real MySQL table completion");
    assert_eq!(tables.status, "success");
    assert!(tables.candidates.iter().any(|candidate| {
        candidate.label.eq_ignore_ascii_case("items")
            && candidate.r#type.eq_ignore_ascii_case("table")
    }));

    let column_sql = "select items. from items";
    let columns = application
        .complete_community_sql(CompleteCommunitySqlRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            sql: column_sql.to_owned(),
            cursor_utf16: utf16_len("select items."),
            min_prefix_length: 0,
            need_full_name: false,
            keyword_case: "UPPER".to_owned(),
            active_snippet_slot: None,
        })
        .await
        .expect("Core must expose real MySQL column completion");
    assert_eq!(columns.status, "success");
    for expected in ["id", "label"] {
        assert!(columns.candidates.iter().any(|candidate| {
            candidate.label.eq_ignore_ascii_case(expected)
                && candidate.r#type.eq_ignore_ascii_case("column")
        }));
    }
}

async fn verify_product_query(application: &Application, datasource_id: &str) {
    let accepted = application
        .start_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: "SELECT id, label, amount, active, created_at FROM items ORDER BY id".to_owned(),
            parameters: Vec::new(),
            limits: QueryLimits {
                max_rows: "16".to_owned(),
                max_result_bytes: (1024_u64 * 1024).to_string(),
                batch_rows: 8,
                batch_bytes: 16 * 1024,
                result_ttl_seconds: 60,
            },
        })
        .await
        .expect("managed MySQL product query must be accepted");
    let result_id = wait_for_result(application, &accepted.operation_id).await;
    let page = application
        .result_page(
            &result_id,
            ResultPageRequest {
                offset: "0".to_owned(),
                max_rows: "16".to_owned(),
                max_bytes: (8_u64 * 1024 * 1024).to_string(),
            },
        )
        .await
        .expect("managed MySQL result must be retained");
    assert_eq!(page.metadata.row_count, "1");
    assert_eq!(page.rows.len(), 1);
    assert!(matches!(
        page.rows[0].values.as_slice(),
        [
            JdbcValue::SignedInteger { value: id },
            JdbcValue::Text { value: label },
            JdbcValue::Decimal { value: amount },
            JdbcValue::SignedInteger { value: active },
            JdbcValue::Timestamp { value: created_at },
        ] if id == "1"
            && label == "mysql-ready"
            && amount == "99.99"
            && active == "1"
            && created_at.starts_with("2026-07-27T12:34:56")
    ));
}

async fn verify_table_preview(application: &Application, datasource_id: &str, database_name: &str) {
    let accepted = application
        .start_community_table_preview(StartCommunityTablePreviewRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            table_name: "items".to_owned(),
            row_limit: Some(1),
        })
        .await
        .expect("Core must build, validate, and accept a MySQL table preview");
    assert_eq!(accepted.row_limit, 1);
    assert!(accepted.sql.contains(&format!("`{database_name}`.`items`")));
    assert!(accepted.sql.to_ascii_lowercase().contains("limit"));

    let result_id = wait_for_result(application, &accepted.operation_id).await;
    let page = application
        .result_page(
            &result_id,
            ResultPageRequest {
                offset: "0".to_owned(),
                max_rows: "1".to_owned(),
                max_bytes: (8_u64 * 1024 * 1024).to_string(),
            },
        )
        .await
        .expect("MySQL table preview result must be retained");
    assert_eq!(page.metadata.row_count, "1");
    assert_eq!(page.rows.len(), 1);
    assert!(matches!(
        page.rows[0].values.as_slice(),
        [
            JdbcValue::SignedInteger { value: id },
            JdbcValue::Text { value: label },
            ..
        ] if id == "1" && label == "mysql-ready"
    ));
}

async fn wait_for_result(application: &Application, operation_id: &str) -> String {
    let mut events = application
        .subscribe_operation(operation_id, Some(0))
        .await
        .expect("MySQL query subscription must open");
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, events.next_event())
            .await
            .expect("MySQL query operation event must arrive")
            .expect("MySQL query event stream must remain valid")
            .expect("MySQL query must emit a terminal event");
        match event.event {
            OperationEvent::Started | OperationEvent::Progress { .. } => {}
            OperationEvent::Completed { result } => return result.id,
            OperationEvent::Failed { error } => panic!("managed MySQL query failed: {error:?}"),
            OperationEvent::Cancelled { reason } => {
                panic!("managed MySQL query was cancelled: {reason:?}")
            }
        }
    }
}

async fn open_session(
    driver: &DriverClient,
    driver_id: &str,
    jdbc_url: &str,
    properties: Vec<ConnectionProperty>,
) -> Session {
    driver
        .open_session(SessionConfig {
            driver_id: driver_id.to_owned(),
            jdbc_url: jdbc_url.to_owned(),
            properties,
            read_only: false,
        })
        .await
        .expect("real MySQL JDBC session must open")
}

async fn execute_update(session: &Session, sql: &str) -> u64 {
    try_execute_update(session, sql)
        .await
        .unwrap_or_else(|error| panic!("real MySQL update must execute: {error}; SQL: {sql}"))
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

fn dml_column(
    name: &str,
    data_type_name: &str,
    precision: Option<u32>,
    scale: Option<i32>,
) -> CommunityDmlColumn {
    CommunityDmlColumn {
        name: name.to_owned(),
        data_type_name: data_type_name.to_owned(),
        precision,
        scale,
    }
}

fn dml_assignment(
    name: &str,
    data_type_name: &str,
    value: CommunityDmlValue,
) -> CommunityDmlAssignment {
    CommunityDmlAssignment {
        column: dml_column(name, data_type_name, None, None),
        value,
    }
}

fn utf16_len(value: &str) -> u32 {
    u32::try_from(value.encode_utf16().count()).expect("test SQL UTF-16 length must fit u32")
}

fn required_env_text(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be valid UTF-8"))
}

fn mysql_test_required() -> bool {
    match std::env::var("MYSQL_TEST_REQUIRED") {
        Err(std::env::VarError::NotPresent) => false,
        Ok(value) if value == "1" || value.eq_ignore_ascii_case("true") => true,
        Ok(value) if value == "0" || value.eq_ignore_ascii_case("false") => false,
        Ok(_) | Err(std::env::VarError::NotUnicode(_)) => {
            panic!("MYSQL_TEST_REQUIRED must be 1, 0, true, or false")
        }
    }
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

fn required_path(name: &str) -> PathBuf {
    let path = std::env::var_os(name).map_or_else(
        || panic!("{name} must point to the integration fixture"),
        PathBuf::from,
    );
    assert!(path.exists(), "{name} does not point to an existing path");
    path
}
