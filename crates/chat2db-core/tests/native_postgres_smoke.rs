use std::{error::Error, fs, panic::AssertUnwindSafe, path::Path, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    CommunityErQueryRequest, ComponentState, CreateDatasourceRequest, DatasourceConnection,
    DatasourceConnectionProperty, GetCommunityFunctionRequest, GetCommunityProcedureRequest,
    GetCommunityTriggerRequest, JdbcValue, ListCommunityColumnsRequest,
    ListCommunityDatabasesRequest, ListCommunityFunctionsRequest, ListCommunityIndexesRequest,
    ListCommunityProceduresRequest, ListCommunitySchemasRequest, ListCommunityTableKeysRequest,
    ListCommunityTablesRequest, ListCommunityTriggersRequest, ListCommunityViewsRequest,
    OperationEvent, QueryLimits, QueryParameter, ResultMetadata, ResultPageRequest,
    StartCommunityTablePreviewRequest, StartQueryRequest,
};
use chat2db_core::{
    Application, NativeConsoleCancellation, NativeConsoleRequest, RuntimeConfig, RuntimeHost,
};
use chat2db_engine_protocol::wire;
use chat2db_java_bridge::{EngineCommand, EngineConfig};
use futures_util::FutureExt as _;
use tempfile::TempDir;
use tokio_postgres::{Config, NoTls};
use uuid::Uuid;

const POSTGRES_DATABASE_TYPE: &str = "POSTGRESQL";
const EVENT_TIMEOUT: Duration = Duration::from_secs(15);

struct PostgresTestConfig {
    host: String,
    port: u16,
    database: String,
    username: String,
    password: String,
}

impl PostgresTestConfig {
    fn from_environment() -> Self {
        Self {
            host: std::env::var("CHAT2DB_POSTGRES_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned()),
            port: std::env::var("CHAT2DB_POSTGRES_PORT")
                .unwrap_or_else(|_| "5432".to_owned())
                .parse()
                .expect("CHAT2DB_POSTGRES_PORT must be a TCP port"),
            database: std::env::var("CHAT2DB_POSTGRES_DATABASE")
                .unwrap_or_else(|_| "app".to_owned()),
            username: std::env::var("CHAT2DB_POSTGRES_USER")
                .unwrap_or_else(|_| "postgres".to_owned()),
            password: std::env::var("CHAT2DB_POSTGRES_PASSWORD")
                .unwrap_or_else(|_| "postgres".to_owned()),
        }
    }

    fn driver_config(&self) -> Config {
        let mut config = Config::new();
        config
            .host(&self.host)
            .port(self.port)
            .dbname(&self.database)
            .user(&self.username)
            .password(&self.password);
        config
    }

    fn connection(&self) -> DatasourceConnection {
        DatasourceConnection {
            jdbc_url: format!(
                "jdbc:postgresql://{}:{}/{}?sslmode=disable",
                self.host, self.port, self.database
            ),
            properties: vec![
                DatasourceConnectionProperty {
                    key: "user".to_owned(),
                    value: self.username.clone(),
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

    fn read_only_connection(&self) -> DatasourceConnection {
        let mut connection = self.connection();
        connection.read_only = true;
        connection
    }
}

struct PostgresFixture {
    schema: String,
    parent_table: String,
    table: String,
    index: String,
    foreign_key: String,
    view: String,
    function: String,
    procedure: String,
    trigger: String,
    trigger_function: String,
}

impl PostgresFixture {
    fn unique() -> Self {
        let suffix = Uuid::new_v4().simple().to_string();
        let suffix = &suffix[..8];
        Self {
            schema: format!("c2s_{suffix}"),
            parent_table: format!("c2p_{suffix}"),
            table: format!("c2t_{suffix}"),
            index: format!("c2i_{suffix}"),
            foreign_key: format!("c2k_{suffix}"),
            view: format!("c2v_{suffix}"),
            function: format!("c2f_{suffix}"),
            procedure: format!("c2r_{suffix}"),
            trigger: format!("c2g_{suffix}"),
            trigger_function: format!("c2tf_{suffix}"),
        }
    }
}

#[tokio::test]
#[ignore = "requires a reachable PostgreSQL database"]
async fn native_postgres_product_paths_keep_java_dormant() {
    let config = PostgresTestConfig::from_environment();
    let fixture = PostgresFixture::unique();
    provision_fixture(&config, &fixture).await;

    let verification = AssertUnwindSafe(verify_native_product(&config, &fixture))
        .catch_unwind()
        .await;
    let cleanup = cleanup_fixture(&config, &fixture).await;
    if let Err(payload) = verification {
        if let Err(error) = cleanup {
            eprintln!("native PostgreSQL cleanup also failed: {error}");
        }
        std::panic::resume_unwind(payload);
    }
    cleanup.expect("native PostgreSQL fixture must be removed");
    assert_fixture_residue_zero(&config).await;
}

async fn verify_native_product(config: &PostgresTestConfig, fixture: &PostgresFixture) {
    let directory = TempDir::new().expect("temporary native PostgreSQL runtime");
    let missing_java = directory.path().join("missing-java");
    let data_dir = directory.path().join("data");
    let runtime = RuntimeConfig::new(EngineConfig::new(EngineCommand::new(missing_java)))
        .with_data_dir(data_dir.clone())
        .with_vault_master_key_base64(STANDARD.encode([0x70; 32]));
    let mut host = RuntimeHost::open(runtime)
        .await
        .expect("native PostgreSQL runtime must open without Java");
    let application = host.application();
    assert_java_dormant(&application);

    let drivers = application.list_drivers();
    let driver = drivers
        .items
        .iter()
        .find(|driver| driver.driver_id == "postgresql")
        .expect("native PostgreSQL driver must be present");
    assert_eq!(driver.driver_class, "rust:tokio-postgres");
    assert_eq!(driver.artifact_count, 0);

    application
        .test_datasource_connection("postgresql", config.connection())
        .await
        .expect("native PostgreSQL connection test must avoid Java");
    assert_java_dormant(&application);

    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Native PostgreSQL".to_owned(),
            driver_id: "postgresql".to_owned(),
            connection: Some(config.connection()),
        })
        .await
        .expect("native PostgreSQL datasource must persist without a JDBC pack");

    verify_oversized_scalar_cleanup(&application, &datasource.id, &data_dir).await;
    verify_read_only_console(&application, config, fixture).await;
    verify_database_schema_and_tables(&application, &datasource.id, config, fixture).await;
    verify_views_routines_and_triggers(&application, &datasource.id, config, fixture).await;
    verify_query_console_preview_and_er(&application, &datasource.id, config, fixture).await;
    verify_console_write_outcomes(&application, &datasource.id, config, fixture).await;
    assert_java_dormant(&application);

    host.shutdown()
        .await
        .expect("native-only PostgreSQL runtime must shut down cleanly");
}

async fn verify_oversized_scalar_cleanup(
    application: &Application,
    datasource_id: &str,
    data_dir: &Path,
) {
    assert!(retained_result_files(data_dir).is_empty());
    let scalar_bytes = wire::JdbcProtocolLimit::MaxScalarBytes as usize + 1;
    let query = application
        .start_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: format!("SELECT repeat('x', {scalar_bytes})"),
            parameters: Vec::new(),
            limits: query_limits("10"),
        })
        .await
        .expect("oversized PostgreSQL scalar query must be accepted before row decoding");
    let error = wait_for_failure(application, &query.operation_id).await;
    assert_eq!(error.code, "postgres_scalar_too_large");
    assert!(
        retained_result_files(data_dir).is_empty(),
        "an aborted oversized result must not leave a retained result file"
    );
    assert_java_dormant(application);
}

async fn verify_read_only_console(
    application: &Application,
    config: &PostgresTestConfig,
    fixture: &PostgresFixture,
) {
    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Native PostgreSQL read only".to_owned(),
            driver_id: "postgresql".to_owned(),
            connection: Some(config.read_only_connection()),
        })
        .await
        .expect("read-only native PostgreSQL datasource must persist");
    let request = |sql: String| NativeConsoleRequest {
        datasource_id: datasource.id.clone(),
        database_name: config.database.clone(),
        sql,
        page_no: 1,
        page_size: 10,
        result_set_id: None,
        single: false,
        page_size_all: false,
        explain: false,
        error_continue: false,
    };
    let results = application
        .execute_native_console(
            request("SELECT 1 AS first_value; VALUES (2)".to_owned()),
            NativeConsoleCancellation::new(),
        )
        .await
        .expect("a read-only Console must allow multiple read statements");
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|result| result.success));

    let error = application
        .execute_native_console(
            request(format!(
                "SELECT 1; UPDATE \"{}\".\"{}\" SET label = 'changed' WHERE id = 1",
                fixture.schema, fixture.table
            )),
            NativeConsoleCancellation::new(),
        )
        .await
        .expect_err("read-only validation must reject the entire script before dispatch");
    assert_eq!(error.api_error().code, "postgres_console_must_be_read_only");

    let label = query_single_text(
        config,
        &format!(
            "SELECT label FROM \"{}\".\"{}\" WHERE id = 1",
            fixture.schema, fixture.table
        ),
    )
    .await;
    assert_eq!(label, "alpha");
    assert_java_dormant(application);
}

fn retained_result_files(data_dir: &Path) -> Vec<String> {
    fs::read_dir(data_dir.join("results"))
        .expect("retained result directory")
        .map(|entry| {
            entry
                .expect("retained result directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

async fn verify_database_schema_and_tables(
    application: &Application,
    datasource_id: &str,
    config: &PostgresTestConfig,
    fixture: &PostgresFixture,
) {
    let databases = application
        .list_community_databases(ListCommunityDatabasesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: POSTGRES_DATABASE_TYPE.to_owned(),
        })
        .await
        .expect("native PostgreSQL databases must list");
    assert!(
        databases
            .items
            .iter()
            .any(|item| item.name == config.database)
    );

    let schemas = application
        .list_community_schemas(ListCommunitySchemasRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: POSTGRES_DATABASE_TYPE.to_owned(),
            database_name: config.database.clone(),
        })
        .await
        .expect("native PostgreSQL schemas must list");
    assert!(schemas.items.iter().any(|item| item.name == fixture.schema));

    let tables = application
        .list_community_tables(ListCommunityTablesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: POSTGRES_DATABASE_TYPE.to_owned(),
            database_name: config.database.clone(),
            schema_name: fixture.schema.clone(),
            table_name_pattern: fixture.table.clone(),
        })
        .await
        .expect("native PostgreSQL tables must list");
    assert!(tables.items.iter().any(|item| item.name == fixture.table));

    let columns = application
        .list_community_columns(ListCommunityColumnsRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: POSTGRES_DATABASE_TYPE.to_owned(),
            database_name: config.database.clone(),
            schema_name: fixture.schema.clone(),
            table_name: fixture.table.clone(),
        })
        .await
        .expect("native PostgreSQL columns must list");
    assert!(columns.items.iter().any(|column| {
        column.name == "id" && column.column_type == "bigint" && column.primary_key == Some(true)
    }));
    assert!(columns.items.iter().any(|column| column.name == "label"));

    let indexes = application
        .list_community_indexes(ListCommunityIndexesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: POSTGRES_DATABASE_TYPE.to_owned(),
            database_name: config.database.clone(),
            schema_name: fixture.schema.clone(),
            table_name: fixture.table.clone(),
        })
        .await
        .expect("native PostgreSQL indexes must list");
    assert!(
        indexes
            .items
            .iter()
            .any(|index| index.name == fixture.index)
    );

    verify_table_keys_and_ddl(application, datasource_id, config, fixture).await;
}

async fn verify_table_keys_and_ddl(
    application: &Application,
    datasource_id: &str,
    config: &PostgresTestConfig,
    fixture: &PostgresFixture,
) {
    let keys = table_keys(datasource_id, config, fixture, &fixture.table);
    let primary = application
        .list_community_primary_keys(keys.clone())
        .await
        .expect("native PostgreSQL primary keys must list");
    assert!(primary.items.iter().any(|key| key.column_name == "id"));
    let imported = application
        .list_community_imported_keys(keys)
        .await
        .expect("native PostgreSQL imported keys must list");
    assert!(imported.items.iter().any(|key| {
        key.foreign_key_name == fixture.foreign_key
            && key.primary_table_name == fixture.parent_table
            && key.foreign_table_name == fixture.table
    }));
    let exported = application
        .list_community_exported_keys(table_keys(
            datasource_id,
            config,
            fixture,
            &fixture.parent_table,
        ))
        .await
        .expect("native PostgreSQL exported keys must list");
    assert!(
        exported
            .items
            .iter()
            .any(|key| key.foreign_key_name == fixture.foreign_key)
    );

    let ddl = application
        .table_ddl(
            datasource_id,
            &config.database,
            &fixture.schema,
            &fixture.table,
        )
        .await
        .expect("native PostgreSQL table DDL must load");
    assert!(ddl.contains("CREATE TABLE"));
    assert!(ddl.contains("FOREIGN KEY"));
    assert!(ddl.contains(&fixture.index));
}

async fn verify_views_routines_and_triggers(
    application: &Application,
    datasource_id: &str,
    config: &PostgresTestConfig,
    fixture: &PostgresFixture,
) {
    let views_request = ListCommunityViewsRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: POSTGRES_DATABASE_TYPE.to_owned(),
        database_name: config.database.clone(),
        schema_name: fixture.schema.clone(),
        view_name_pattern: fixture.view.clone(),
    };
    let views = application
        .list_community_views(views_request.clone())
        .await
        .expect("native PostgreSQL views must list");
    assert!(views.items.iter().any(|view| view.name == fixture.view));
    let view = application
        .get_community_view(views_request)
        .await
        .expect("native PostgreSQL view detail must load");
    assert!(view.ddl.contains(&fixture.view));

    let functions = application
        .list_community_functions(ListCommunityFunctionsRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: POSTGRES_DATABASE_TYPE.to_owned(),
            database_name: config.database.clone(),
            schema_name: fixture.schema.clone(),
        })
        .await
        .expect("native PostgreSQL functions must list");
    assert!(
        functions
            .items
            .iter()
            .any(|item| item.name == fixture.function)
    );
    let function_request = GetCommunityFunctionRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: POSTGRES_DATABASE_TYPE.to_owned(),
        database_name: config.database.clone(),
        schema_name: fixture.schema.clone(),
        function_name: fixture.function.clone(),
    };
    let function = application
        .get_community_function(function_request.clone())
        .await
        .expect("native PostgreSQL function detail must load");
    assert!(function.body.contains(&fixture.function));
    let parameters = application
        .list_community_function_parameters(function_request)
        .await
        .expect("native PostgreSQL function parameters must list");
    assert!(
        parameters
            .items
            .iter()
            .any(|parameter| parameter.column_name == "p_value")
    );

    verify_procedures_and_triggers(application, datasource_id, config, fixture).await;
}

async fn verify_procedures_and_triggers(
    application: &Application,
    datasource_id: &str,
    config: &PostgresTestConfig,
    fixture: &PostgresFixture,
) {
    let procedures = application
        .list_community_procedures(ListCommunityProceduresRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: POSTGRES_DATABASE_TYPE.to_owned(),
            database_name: config.database.clone(),
            schema_name: fixture.schema.clone(),
        })
        .await
        .expect("native PostgreSQL procedures must list");
    assert!(
        procedures
            .items
            .iter()
            .any(|item| item.name == fixture.procedure)
    );
    let procedure_request = GetCommunityProcedureRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: POSTGRES_DATABASE_TYPE.to_owned(),
        database_name: config.database.clone(),
        schema_name: fixture.schema.clone(),
        procedure_name: fixture.procedure.clone(),
    };
    let procedure = application
        .get_community_procedure(procedure_request.clone())
        .await
        .expect("native PostgreSQL procedure detail must load");
    assert!(procedure.body.contains(&fixture.procedure));
    let parameters = application
        .list_community_procedure_parameters(procedure_request)
        .await
        .expect("native PostgreSQL procedure parameters must list");
    assert!(
        parameters
            .items
            .iter()
            .any(|parameter| parameter.column_name == "p_output")
    );

    let triggers = application
        .list_community_triggers(ListCommunityTriggersRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: POSTGRES_DATABASE_TYPE.to_owned(),
            database_name: config.database.clone(),
            schema_name: fixture.schema.clone(),
        })
        .await
        .expect("native PostgreSQL triggers must list");
    assert!(
        triggers
            .items
            .iter()
            .any(|item| item.name == fixture.trigger)
    );
    let trigger = application
        .get_community_trigger(GetCommunityTriggerRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: POSTGRES_DATABASE_TYPE.to_owned(),
            database_name: config.database.clone(),
            schema_name: fixture.schema.clone(),
            trigger_name: fixture.trigger.clone(),
        })
        .await
        .expect("native PostgreSQL trigger detail must load");
    assert!(trigger.event_manipulation.contains("INSERT"));
    assert!(trigger.body.contains(&fixture.trigger));
}

async fn verify_query_console_preview_and_er(
    application: &Application,
    datasource_id: &str,
    config: &PostgresTestConfig,
    fixture: &PostgresFixture,
) {
    let query = application
        .start_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: format!(
                "SELECT label, score FROM \"{}\".\"{}\" WHERE id = $1",
                fixture.schema, fixture.table
            ),
            parameters: vec![QueryParameter {
                position: 1,
                value: JdbcValue::SignedInteger {
                    value: "1".to_owned(),
                },
            }],
            limits: query_limits("10"),
        })
        .await
        .expect("native PostgreSQL retained query must be accepted");
    let result = wait_for_result(application, &query.operation_id).await;
    assert_eq!(result.row_count, "1");
    let page = result_page(application, &result).await;
    assert!(matches!(
        &page.rows[0].values[0],
        JdbcValue::Text { value } if value == "alpha"
    ));

    let console = application
        .execute_native_console(
            NativeConsoleRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: config.database.clone(),
                sql: format!(
                    "SELECT label FROM \"{}\".\"{}\" ORDER BY id",
                    fixture.schema, fixture.table
                ),
                page_no: 1,
                page_size: 10,
                result_set_id: None,
                single: false,
                page_size_all: false,
                explain: false,
                error_continue: false,
            },
            NativeConsoleCancellation::new(),
        )
        .await
        .expect("native PostgreSQL Console must execute");
    assert_eq!(console.len(), 1);
    assert!(console[0].success);
    assert_eq!(console[0].row_count, 2);

    verify_extended_binary_types(application, datasource_id, config).await;

    let preview = application
        .start_community_table_preview(StartCommunityTablePreviewRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: POSTGRES_DATABASE_TYPE.to_owned(),
            database_name: config.database.clone(),
            schema_name: fixture.schema.clone(),
            table_name: fixture.table.clone(),
            row_limit: Some(2),
        })
        .await
        .expect("native PostgreSQL table preview must be accepted");
    let preview_result = wait_for_result(application, &preview.operation_id).await;
    assert_eq!(preview_result.row_count, "2");
    assert_eq!(
        result_page(application, &preview_result).await.rows.len(),
        2
    );

    let er = application
        .community_mysql_er_model(CommunityErQueryRequest {
            data_source_id: datasource_id.to_owned(),
            database_name: config.database.clone(),
            schema_name: fixture.schema.clone(),
        })
        .await
        .expect("native PostgreSQL ER metadata must load through the table SPI");
    let child = er
        .tables
        .iter()
        .find(|table| table.name == fixture.table)
        .expect("ER metadata must include the child table");
    assert!(child.column_list.iter().any(|column| column.name == "id"));
    assert!(child.foreign_key_list.iter().any(|key| {
        key.pk_table_name == fixture.parent_table && key.fk_table_name == fixture.table
    }));
}

async fn verify_extended_binary_types(
    application: &Application,
    datasource_id: &str,
    config: &PostgresTestConfig,
) {
    let types = application
        .execute_native_console(
            NativeConsoleRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: config.database.clone(),
                sql: concat!(
                    "SELECT inet '192.168.4.7/24', ",
                    "cidr '2001:db8::/64', ",
                    "12.34::money, ",
                    "ARRAY[['a,b', 'NULL'], ['brace{', 'white space']]::text[][]"
                )
                .to_owned(),
                page_no: 1,
                page_size: 10,
                result_set_id: None,
                single: true,
                page_size_all: false,
                explain: false,
                error_continue: false,
            },
            NativeConsoleCancellation::new(),
        )
        .await
        .expect("native PostgreSQL extended binary types must decode");
    let values = &types[0].rows[0].values;
    assert!(matches!(
        &values[0],
        JdbcValue::Text { value } if value == "192.168.4.7/24"
    ));
    assert!(matches!(
        &values[1],
        JdbcValue::Text { value } if value == "2001:db8::/64"
    ));
    assert!(matches!(
        &values[2],
        JdbcValue::Opaque { type_name, display_value }
            if type_name == "money" && display_value == "raw_units=1234"
    ));
    assert!(matches!(
        &values[3],
        JdbcValue::Opaque { display_value, .. }
            if display_value == "{{\"a,b\",\"NULL\"},{\"brace{\",\"white space\"}}"
    ));
}

async fn verify_console_write_outcomes(
    application: &Application,
    datasource_id: &str,
    config: &PostgresTestConfig,
    fixture: &PostgresFixture,
) {
    let request = |sql: String| NativeConsoleRequest {
        datasource_id: datasource_id.to_owned(),
        database_name: config.database.clone(),
        sql,
        page_no: 1,
        page_size: 10,
        result_set_id: None,
        single: true,
        page_size_all: false,
        explain: false,
        error_continue: false,
    };

    let rejected = application
        .execute_native_console(
            request(format!(
                "INSERT INTO \"{}\".\"{}\" (id, parent_id, label) VALUES (1, 1, 'duplicate') RETURNING id",
                fixture.schema, fixture.table
            )),
            NativeConsoleCancellation::new(),
        )
        .await
        .expect("an explicit PostgreSQL rejection must remain a statement failure");
    assert_eq!(rejected.len(), 1);
    assert!(!rejected[0].success);
    assert_eq!(
        rejected[0]
            .error
            .as_ref()
            .expect("the rejected write must include a safe error")
            .code,
        "postgres_query_rejected"
    );

    let cancellation = NativeConsoleCancellation::new();
    let cancellation_control = cancellation.clone();
    let cancellation_marker = format!("chat2db_cancel_{}", Uuid::new_v4().simple());
    let cancelled = tokio::time::timeout(EVENT_TIMEOUT, async {
        tokio::join!(
            application.execute_native_console(
                request(long_running_insert(
                    fixture,
                    &cancellation_marker,
                    "cancelled"
                )),
                cancellation,
            ),
            async {
                wait_for_active_statement(config, &cancellation_marker).await;
                assert!(
                    cancellation_control
                        .cancel(Some("cancel after PostgreSQL dispatch".to_owned()))
                );
            }
        )
    })
    .await
    .expect("the cancelled PostgreSQL Console write must terminate")
    .0
    .expect_err("a write cancelled after dispatch must not report a definite outcome");
    assert_eq!(cancelled.api_error().code, "database_write_outcome_unknown");

    let termination_marker = format!("chat2db_terminate_{}", Uuid::new_v4().simple());
    let terminated = tokio::time::timeout(EVENT_TIMEOUT, async {
        tokio::join!(
            application.execute_native_console(
                request(long_running_insert(
                    fixture,
                    &termination_marker,
                    "terminated"
                )),
                NativeConsoleCancellation::new(),
            ),
            async {
                let backend_pid = wait_for_active_statement(config, &termination_marker).await;
                terminate_backend(config, backend_pid).await;
            }
        )
    })
    .await
    .expect("the terminated PostgreSQL Console write must finish")
    .0
    .expect_err("a transport failure after write dispatch must have an unknown outcome");
    assert_eq!(
        terminated.api_error().code,
        "database_write_outcome_unknown"
    );
}

fn long_running_insert(fixture: &PostgresFixture, marker: &str, label: &str) -> String {
    format!(
        "/* {marker} */ INSERT INTO \"{}\".\"{}\" (parent_id, label, score) SELECT 1, '{label}', 1 FROM pg_sleep(30) RETURNING id",
        fixture.schema, fixture.table
    )
}

async fn wait_for_active_statement(config: &PostgresTestConfig, marker: &str) -> i32 {
    let (client, connection) = config
        .driver_config()
        .connect(NoTls)
        .await
        .expect("PostgreSQL activity monitor connection");
    let task = tokio::spawn(connection);
    let query_pattern = format!("%{marker}%");
    let backend_pid = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(row) = client
                .query_opt(
                    "SELECT pid FROM pg_stat_activity WHERE pid <> pg_backend_pid() AND state = 'active' AND wait_event_type = 'Timeout' AND wait_event = 'PgSleep' AND query LIKE $1 ORDER BY query_start DESC LIMIT 1",
                    &[&query_pattern],
                )
                .await
                .expect("PostgreSQL activity lookup")
            {
                return row.try_get(0).expect("PostgreSQL backend pid");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the marked PostgreSQL statement must reach the server");
    drop(client);
    task.await
        .expect("PostgreSQL activity monitor task")
        .expect("PostgreSQL activity monitor shutdown");
    backend_pid
}

async fn terminate_backend(config: &PostgresTestConfig, backend_pid: i32) {
    let (client, connection) = config
        .driver_config()
        .connect(NoTls)
        .await
        .expect("PostgreSQL termination connection");
    let task = tokio::spawn(connection);
    let terminated: bool = client
        .query_one("SELECT pg_terminate_backend($1)", &[&backend_pid])
        .await
        .expect("PostgreSQL backend termination")
        .try_get(0)
        .expect("PostgreSQL backend termination result");
    assert!(terminated, "the marked PostgreSQL backend must terminate");
    drop(client);
    task.await
        .expect("PostgreSQL termination task")
        .expect("PostgreSQL termination connection shutdown");
}

fn table_keys(
    datasource_id: &str,
    config: &PostgresTestConfig,
    fixture: &PostgresFixture,
    table_name: &str,
) -> ListCommunityTableKeysRequest {
    ListCommunityTableKeysRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: POSTGRES_DATABASE_TYPE.to_owned(),
        database_name: config.database.clone(),
        schema_name: fixture.schema.clone(),
        table_name: table_name.to_owned(),
    }
}

fn query_limits(max_rows: &str) -> QueryLimits {
    QueryLimits {
        max_rows: max_rows.to_owned(),
        max_result_bytes: (8_u64 * 1024 * 1024).to_string(),
        batch_rows: 2,
        batch_bytes: 1024 * 1024,
        result_ttl_seconds: 60,
    }
}

async fn wait_for_result(application: &Application, operation_id: &str) -> ResultMetadata {
    let mut subscription = application
        .subscribe_operation(operation_id, None)
        .await
        .expect("native PostgreSQL operation must be subscribable");
    tokio::time::timeout(EVENT_TIMEOUT, async {
        while let Some(envelope) = subscription
            .next_event()
            .await
            .expect("native PostgreSQL operation event must decode")
        {
            match envelope.event {
                OperationEvent::Completed { result } => return result,
                OperationEvent::Failed { error } => {
                    panic!("native PostgreSQL query failed: {error:?}")
                }
                OperationEvent::Cancelled { reason } => {
                    panic!("native PostgreSQL query was cancelled: {reason:?}")
                }
                OperationEvent::Started | OperationEvent::Progress { .. } => {}
            }
        }
        panic!("native PostgreSQL operation ended without a terminal event")
    })
    .await
    .expect("native PostgreSQL query must finish before timeout")
}

async fn wait_for_failure(
    application: &Application,
    operation_id: &str,
) -> chat2db_contract::ApiError {
    let mut subscription = application
        .subscribe_operation(operation_id, None)
        .await
        .expect("failed native PostgreSQL operation must be subscribable");
    tokio::time::timeout(EVENT_TIMEOUT, async {
        while let Some(envelope) = subscription
            .next_event()
            .await
            .expect("native PostgreSQL operation event must decode")
        {
            match envelope.event {
                OperationEvent::Failed { error } => return error,
                OperationEvent::Completed { result } => {
                    panic!("native PostgreSQL query unexpectedly completed: {result:?}")
                }
                OperationEvent::Cancelled { reason } => {
                    panic!("native PostgreSQL query was cancelled: {reason:?}")
                }
                OperationEvent::Started | OperationEvent::Progress { .. } => {}
            }
        }
        panic!("native PostgreSQL operation ended without a failure event")
    })
    .await
    .expect("native PostgreSQL query must fail before timeout")
}

async fn result_page(
    application: &Application,
    result: &ResultMetadata,
) -> chat2db_contract::ResultPage {
    application
        .result_page(
            &result.id,
            ResultPageRequest {
                offset: "0".to_owned(),
                max_rows: "20".to_owned(),
                max_bytes: (8_u64 * 1024 * 1024).to_string(),
            },
        )
        .await
        .expect("native PostgreSQL result page must be retained")
}

fn assert_java_dormant(application: &Application) {
    let engine = application
        .health()
        .components
        .into_iter()
        .find(|component| component.id == "database-engine")
        .expect("database engine health must be present");
    assert_eq!(engine.state, ComponentState::Ready);
    assert_eq!(engine.detail, "Available on demand; Java is not running");
}

async fn provision_fixture(config: &PostgresTestConfig, fixture: &PostgresFixture) {
    let sql = format!(
        r#"
        CREATE SCHEMA "{schema}";
        CREATE TABLE "{schema}"."{parent}" (
            id BIGSERIAL PRIMARY KEY,
            label TEXT NOT NULL
        );
        CREATE TABLE "{schema}"."{table}" (
            id BIGSERIAL PRIMARY KEY,
            parent_id BIGINT NOT NULL,
            label VARCHAR(64) NOT NULL,
            score NUMERIC(12, 4),
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            CONSTRAINT "{foreign_key}" FOREIGN KEY (parent_id)
                REFERENCES "{schema}"."{parent}"(id)
        );
        CREATE INDEX "{index}" ON "{schema}"."{table}"(label);
        CREATE VIEW "{schema}"."{view}" AS
            SELECT id, label FROM "{schema}"."{table}";
        CREATE FUNCTION "{schema}"."{function}"(p_value integer) RETURNS integer
            LANGUAGE SQL IMMUTABLE AS $function_body$ SELECT p_value + 1 $function_body$;
        CREATE PROCEDURE "{schema}"."{procedure}"(IN p_value integer, OUT p_output integer)
            LANGUAGE plpgsql AS $procedure_body$ BEGIN p_output := p_value + 1; END $procedure_body$;
        CREATE FUNCTION "{schema}"."{trigger_function}"() RETURNS trigger
            LANGUAGE plpgsql AS $trigger_body$
            BEGIN NEW.created_at := clock_timestamp(); RETURN NEW; END
            $trigger_body$;
        CREATE TRIGGER "{trigger}" BEFORE INSERT ON "{schema}"."{table}"
            FOR EACH ROW EXECUTE FUNCTION "{schema}"."{trigger_function}"();
        INSERT INTO "{schema}"."{parent}"(label) VALUES ('parent');
        INSERT INTO "{schema}"."{table}"(parent_id, label, score)
            VALUES (1, 'alpha', 12.3400), (1, 'beta', 56.7800);
        "#,
        schema = fixture.schema,
        parent = fixture.parent_table,
        table = fixture.table,
        foreign_key = fixture.foreign_key,
        index = fixture.index,
        view = fixture.view,
        function = fixture.function,
        procedure = fixture.procedure,
        trigger_function = fixture.trigger_function,
        trigger = fixture.trigger,
    );
    execute_fixture_sql(config, &sql)
        .await
        .expect("native PostgreSQL fixture must be provisioned");
}

async fn cleanup_fixture(
    config: &PostgresTestConfig,
    fixture: &PostgresFixture,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    execute_fixture_sql(
        config,
        &format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", fixture.schema),
    )
    .await
}

async fn assert_fixture_residue_zero(config: &PostgresTestConfig) {
    let (client, connection) = config
        .driver_config()
        .connect(NoTls)
        .await
        .expect("PostgreSQL residue verification connection");
    let task = tokio::spawn(connection);
    let count: i64 = client
        .query_one(
            "SELECT count(*) FROM pg_namespace \
             WHERE nspname LIKE 'c2s\\_%' ESCAPE '\\' \
                OR nspname LIKE 'chat2db\\_native\\_smoke\\_%' ESCAPE '\\'",
            &[],
        )
        .await
        .expect("PostgreSQL residue verification query")
        .try_get(0)
        .expect("PostgreSQL residue count");
    drop(client);
    task.await
        .expect("PostgreSQL residue connection task")
        .expect("PostgreSQL residue connection shutdown");
    assert_eq!(count, 0, "native PostgreSQL smoke schemas must be removed");
}

async fn execute_fixture_sql(
    config: &PostgresTestConfig,
    sql: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let (client, connection) = config.driver_config().connect(NoTls).await?;
    let task = tokio::spawn(connection);
    let execution = client.batch_execute(sql).await;
    drop(client);
    task.await??;
    execution?;
    Ok(())
}

async fn query_single_text(config: &PostgresTestConfig, sql: &str) -> String {
    let (client, connection) = config
        .driver_config()
        .connect(NoTls)
        .await
        .expect("PostgreSQL verification connection");
    let task = tokio::spawn(connection);
    let value = client
        .query_one(sql, &[])
        .await
        .expect("PostgreSQL verification query")
        .try_get(0)
        .expect("PostgreSQL text value");
    drop(client);
    task.await
        .expect("PostgreSQL verification connection task")
        .expect("PostgreSQL verification connection shutdown");
    value
}
