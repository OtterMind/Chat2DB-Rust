use std::{panic::AssertUnwindSafe, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    ComponentState, CreateDatasourceRequest, DatasourceConnection, DatasourceConnectionProperty,
    GetCommunityFunctionRequest, GetCommunityProcedureRequest, GetCommunityTriggerRequest,
    JdbcValue, ListCommunityColumnsRequest, ListCommunityDatabasesRequest,
    ListCommunityFunctionsRequest, ListCommunityIndexesRequest, ListCommunityProceduresRequest,
    ListCommunitySchemasRequest, ListCommunityTableKeysRequest, ListCommunityTablesRequest,
    ListCommunityTriggersRequest, ListCommunityViewsRequest, OperationEvent, QueryLimits,
    QueryParameter, ResultMetadata, ResultPageRequest, StartCommunityTablePreviewRequest,
    StartQueryRequest,
};
use chat2db_core::{
    Application, NativeConsoleCancellation, NativeConsoleRequest, RuntimeConfig, RuntimeHost,
};
use chat2db_java_bridge::{EngineCommand, EngineConfig};
use futures_util::FutureExt as _;
use oracle_rs::{Config, Connection, Error as OracleError, Value};
use tempfile::TempDir;
use uuid::Uuid;

const ORACLE_DATABASE_TYPE: &str = "ORACLE";
const EVENT_TIMEOUT: Duration = Duration::from_secs(15);

struct OracleTestConfig {
    host: String,
    port: u16,
    service: String,
    username: String,
    password: String,
}

impl OracleTestConfig {
    fn from_environment() -> Self {
        let host = required_env("CHAT2DB_ORACLE_HOST");
        assert!(
            !host.trim().is_empty()
                && !host.chars().any(char::is_control)
                && !host.contains(['/', '?', '#', ':']),
            "CHAT2DB_ORACLE_HOST must be a valid IPv4 address or hostname"
        );
        let port = std::env::var("CHAT2DB_ORACLE_PORT")
            .unwrap_or_else(|_| "1521".to_owned())
            .parse::<u16>()
            .expect("CHAT2DB_ORACLE_PORT must be a TCP port");
        assert_ne!(port, 0, "CHAT2DB_ORACLE_PORT cannot be zero");
        let service = required_env("CHAT2DB_ORACLE_SERVICE");
        assert!(!service.trim().is_empty(), "Oracle service cannot be empty");
        let username = required_env("CHAT2DB_ORACLE_USERNAME");
        assert!(!username.is_empty(), "Oracle username cannot be empty");
        Self {
            host,
            port,
            service,
            username,
            password: required_env("CHAT2DB_ORACLE_PASSWORD"),
        }
    }

    fn driver_config(&self) -> Config {
        Config::new(
            self.host.clone(),
            self.port,
            self.service.clone(),
            self.username.clone(),
            self.password.clone(),
        )
    }

    fn connection(&self) -> DatasourceConnection {
        DatasourceConnection {
            jdbc_url: format!(
                "jdbc:oracle:thin:@{}:{}/{}",
                self.host, self.port, self.service
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

struct OracleFixture {
    parent_table: String,
    table: String,
    index: String,
    foreign_key: String,
    view: String,
    function: String,
    side_effect_function: String,
    procedure: String,
    trigger: String,
}

impl OracleFixture {
    fn unique() -> Self {
        let suffix = Uuid::new_v4().simple().to_string()[..8].to_ascii_uppercase();
        Self {
            parent_table: format!("C2P_{suffix}"),
            table: format!("C2T_{suffix}"),
            index: format!("C2I_{suffix}"),
            foreign_key: format!("C2K_{suffix}"),
            view: format!("C2V_{suffix}"),
            function: format!("C2F_{suffix}"),
            side_effect_function: format!("C2S_{suffix}"),
            procedure: format!("C2R_{suffix}"),
            trigger: format!("C2G_{suffix}"),
        }
    }
}

#[tokio::test]
#[ignore = "requires a reachable Oracle 12.1+ database"]
async fn connects_and_selects_one() {
    let config = OracleTestConfig::from_environment();
    let connection = Connection::connect_with_config(config.driver_config())
        .await
        .expect("Oracle connection should succeed");
    let result = connection
        .query("SELECT 1 FROM DUAL", &[])
        .await
        .expect("SELECT 1 should succeed");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get(0) {
        Some(Value::Integer(1)) => {}
        Some(Value::String(value)) if value == "1" => {}
        other => panic!("SELECT 1 returned an unexpected value: {other:?}"),
    }
    connection.close().await.expect("connection should close");
}

#[tokio::test]
#[ignore = "requires CHAT2DB_ORACLE_* variables and a reachable Oracle 12.1+ database"]
async fn native_oracle_product_paths_keep_java_dormant() {
    let config = OracleTestConfig::from_environment();
    let fixture = OracleFixture::unique();
    let verification = AssertUnwindSafe(async {
        provision_fixture(&config, &fixture).await;
        verify_native_product(&config, &fixture).await;
    })
    .catch_unwind()
    .await;
    let cleanup = cleanup_fixture(&config, &fixture).await;
    if let Err(payload) = verification {
        if let Err(error) = cleanup {
            eprintln!("native Oracle cleanup also failed: {error}");
        }
        std::panic::resume_unwind(payload);
    }
    cleanup.expect("native Oracle fixture must be removed");
}

async fn verify_native_product(config: &OracleTestConfig, fixture: &OracleFixture) {
    let directory = TempDir::new().expect("temporary native Oracle runtime");
    let missing_java = directory.path().join("missing-java");
    let runtime = RuntimeConfig::new(EngineConfig::new(EngineCommand::new(missing_java)))
        .with_data_dir(directory.path().join("data"))
        .with_vault_master_key_base64(STANDARD.encode([0x72; 32]));
    let mut host = RuntimeHost::open(runtime)
        .await
        .expect("native Oracle runtime must open without Java");
    let application = host.application();
    assert_java_dormant(&application);

    let drivers = application.list_drivers();
    let oracle_driver = drivers
        .items
        .iter()
        .find(|driver| driver.driver_id == "oracle")
        .expect("native Oracle driver must be present in the driver inventory");
    assert_eq!(oracle_driver.driver_class, "rust:oracle-rs");
    assert_eq!(oracle_driver.artifact_count, 0);

    application
        .test_datasource_connection("oracle", config.connection())
        .await
        .expect("native Oracle connection test must succeed without a JDBC pack");
    assert_java_dormant(&application);

    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Native Oracle".to_owned(),
            driver_id: "oracle".to_owned(),
            connection: Some(config.connection()),
        })
        .await
        .expect("native Oracle datasource must persist without a JDBC pack");

    let (database_name, schema_name) =
        verify_database_and_schema_metadata(&application, &datasource.id).await;
    verify_table_metadata(
        &application,
        &datasource.id,
        &database_name,
        &schema_name,
        fixture,
    )
    .await;
    verify_view_routine_and_trigger_metadata(
        &application,
        &datasource.id,
        &database_name,
        &schema_name,
        fixture,
    )
    .await;
    verify_query_console_and_preview(
        &application,
        &datasource.id,
        &database_name,
        &schema_name,
        fixture,
    )
    .await;
    verify_type_matrix(
        &application,
        &datasource.id,
        &database_name,
        &schema_name,
        fixture,
    )
    .await;
    verify_read_only_side_effect(&application, config, &datasource.id, &schema_name, fixture).await;
    assert_java_dormant(&application);

    host.shutdown()
        .await
        .expect("native-only Oracle runtime must shut down cleanly");
}

async fn verify_database_and_schema_metadata(
    application: &Application,
    datasource_id: &str,
) -> (String, String) {
    let databases = application
        .list_community_databases(ListCommunityDatabasesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: ORACLE_DATABASE_TYPE.to_owned(),
        })
        .await
        .expect("native Oracle databases must list");
    let database = databases
        .items
        .first()
        .expect("Oracle must expose its current database");
    assert!(!database.name.is_empty());
    assert!(!database.owner.is_empty());

    let schemas = application
        .list_community_schemas(ListCommunitySchemasRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: ORACLE_DATABASE_TYPE.to_owned(),
            database_name: database.name.clone(),
        })
        .await
        .expect("native Oracle schemas must list");
    assert!(
        schemas
            .items
            .iter()
            .any(|schema| schema.name.eq_ignore_ascii_case(&database.owner))
    );
    (database.name.clone(), database.owner.to_ascii_uppercase())
}

async fn verify_table_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    fixture: &OracleFixture,
) {
    let tables = application
        .list_community_tables(ListCommunityTablesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: ORACLE_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: schema_name.to_owned(),
            table_name_pattern: fixture.table.clone(),
        })
        .await
        .expect("native Oracle tables must list");
    assert!(
        tables
            .items
            .iter()
            .any(|table| table.name == fixture.table && table.table_type == "TABLE")
    );

    verify_column_metadata(
        application,
        datasource_id,
        database_name,
        schema_name,
        fixture,
    )
    .await;

    let indexes = application
        .list_community_indexes(ListCommunityIndexesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: ORACLE_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: schema_name.to_owned(),
            table_name: fixture.table.clone(),
        })
        .await
        .expect("native Oracle indexes must list");
    assert!(indexes.items.iter().any(|index| {
        index.name == fixture.index
            && index
                .columns
                .iter()
                .any(|column| column.column_name == "LABEL")
    }));

    let child_keys = table_keys(datasource_id, database_name, schema_name, &fixture.table);
    let primary = application
        .list_community_primary_keys(child_keys.clone())
        .await
        .expect("native Oracle primary keys must list");
    assert!(primary.items.iter().any(|key| key.column_name == "ID"));
    let imported = application
        .list_community_imported_keys(child_keys)
        .await
        .expect("native Oracle imported keys must list");
    assert!(imported.items.iter().any(|key| {
        key.foreign_key_name == fixture.foreign_key
            && key.primary_table_name == fixture.parent_table
            && key.foreign_table_name == fixture.table
    }));
    let exported = application
        .list_community_exported_keys(table_keys(
            datasource_id,
            database_name,
            schema_name,
            &fixture.parent_table,
        ))
        .await
        .expect("native Oracle exported keys must list");
    assert!(
        exported
            .items
            .iter()
            .any(|key| key.foreign_key_name == fixture.foreign_key)
    );

    let ddl = application
        .table_ddl(datasource_id, database_name, schema_name, &fixture.table)
        .await
        .expect("native Oracle table DDL must load");
    assert!(ddl.to_ascii_uppercase().contains(&fixture.table));
    assert!(ddl.to_ascii_uppercase().contains("CREATE TABLE"));
}

async fn verify_column_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    fixture: &OracleFixture,
) {
    let columns = application
        .list_community_columns(ListCommunityColumnsRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: ORACLE_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: schema_name.to_owned(),
            table_name: fixture.table.clone(),
        })
        .await
        .expect("native Oracle columns must list");
    assert!(columns.items.iter().any(|column| {
        column.name == "ID" && column.column_type == "NUMBER" && column.primary_key == Some(true)
    }));
    assert!(
        columns
            .items
            .iter()
            .any(|column| column.name == "LABEL" && column.column_type == "VARCHAR2")
    );
    assert!(columns.items.iter().any(|column| {
        column.name == "SCORE_X2"
            && column.column_type == "NUMBER"
            && column.generated_column == Some(true)
    }));
    assert!(
        columns
            .items
            .iter()
            .all(|column| column.name != "HIDDEN_VALUE"),
        "Oracle hidden columns must not leak through native metadata"
    );
}

async fn verify_view_routine_and_trigger_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    fixture: &OracleFixture,
) {
    verify_view_metadata(
        application,
        datasource_id,
        database_name,
        schema_name,
        fixture,
    )
    .await;
    verify_function_metadata(
        application,
        datasource_id,
        database_name,
        schema_name,
        fixture,
    )
    .await;
    verify_procedure_metadata(
        application,
        datasource_id,
        database_name,
        schema_name,
        fixture,
    )
    .await;
    verify_trigger_metadata(
        application,
        datasource_id,
        database_name,
        schema_name,
        fixture,
    )
    .await;
}

async fn verify_view_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    fixture: &OracleFixture,
) {
    let views_request = ListCommunityViewsRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: ORACLE_DATABASE_TYPE.to_owned(),
        database_name: database_name.to_owned(),
        schema_name: schema_name.to_owned(),
        view_name_pattern: fixture.view.clone(),
    };
    let views = application
        .list_community_views(views_request.clone())
        .await
        .expect("native Oracle views must list");
    assert!(views.items.iter().any(|view| view.name == fixture.view));
    let view = application
        .get_community_view(views_request)
        .await
        .expect("native Oracle view detail must load");
    assert!(view.ddl.to_ascii_uppercase().contains(&fixture.view));
}

async fn verify_function_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    fixture: &OracleFixture,
) {
    let functions = application
        .list_community_functions(ListCommunityFunctionsRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: ORACLE_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: schema_name.to_owned(),
        })
        .await
        .expect("native Oracle functions must list");
    assert!(
        functions
            .items
            .iter()
            .any(|function| function.name == fixture.function)
    );
    let function_request = GetCommunityFunctionRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: ORACLE_DATABASE_TYPE.to_owned(),
        database_name: database_name.to_owned(),
        schema_name: schema_name.to_owned(),
        function_name: fixture.function.clone(),
    };
    let function = application
        .get_community_function(function_request.clone())
        .await
        .expect("native Oracle function detail must load");
    assert!(
        function
            .body
            .to_ascii_uppercase()
            .contains(&fixture.function)
    );
    let function_parameters = application
        .list_community_function_parameters(function_request)
        .await
        .expect("native Oracle function parameters must list");
    assert!(
        function_parameters
            .items
            .iter()
            .any(|parameter| parameter.column_name == "P_VALUE")
    );
}

async fn verify_procedure_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    fixture: &OracleFixture,
) {
    let procedures = application
        .list_community_procedures(ListCommunityProceduresRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: ORACLE_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: schema_name.to_owned(),
        })
        .await
        .expect("native Oracle procedures must list");
    assert!(
        procedures
            .items
            .iter()
            .any(|procedure| procedure.name == fixture.procedure)
    );
    let procedure_request = GetCommunityProcedureRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: ORACLE_DATABASE_TYPE.to_owned(),
        database_name: database_name.to_owned(),
        schema_name: schema_name.to_owned(),
        procedure_name: fixture.procedure.clone(),
    };
    let procedure = application
        .get_community_procedure(procedure_request.clone())
        .await
        .expect("native Oracle procedure detail must load");
    assert!(
        procedure
            .body
            .to_ascii_uppercase()
            .contains(&fixture.procedure)
    );
    let procedure_parameters = application
        .list_community_procedure_parameters(procedure_request)
        .await
        .expect("native Oracle procedure parameters must list");
    assert!(
        procedure_parameters
            .items
            .iter()
            .any(|parameter| parameter.column_name == "P_OUTPUT")
    );
}

async fn verify_trigger_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    fixture: &OracleFixture,
) {
    let triggers = application
        .list_community_triggers(ListCommunityTriggersRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: ORACLE_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: schema_name.to_owned(),
        })
        .await
        .expect("native Oracle triggers must list");
    assert!(
        triggers
            .items
            .iter()
            .any(|trigger| trigger.name == fixture.trigger)
    );
    let trigger = application
        .get_community_trigger(GetCommunityTriggerRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: ORACLE_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: schema_name.to_owned(),
            trigger_name: fixture.trigger.clone(),
        })
        .await
        .expect("native Oracle trigger detail must load");
    assert!(trigger.event_manipulation.contains("INSERT"));
    assert!(trigger.body.to_ascii_uppercase().contains(&fixture.trigger));
}

async fn verify_query_console_and_preview(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    fixture: &OracleFixture,
) {
    let query = application
        .start_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: format!(
                "SELECT LABEL, SCORE FROM \"{schema_name}\".\"{}\" WHERE ID = :1",
                fixture.table
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
        .expect("native Oracle query must be accepted");
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
                database_name: database_name.to_owned(),
                sql: format!(
                    "SELECT LABEL FROM \"{schema_name}\".\"{}\" ORDER BY ID",
                    fixture.table
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
        .expect("native Oracle Console query must execute");
    assert_eq!(console.len(), 1);
    assert!(console[0].success);
    assert_eq!(console[0].row_count, 2);
    assert!(matches!(
        &console[0].rows[0].values[0],
        JdbcValue::Text { value } if value == "alpha"
    ));

    let preview = application
        .start_community_table_preview(StartCommunityTablePreviewRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: ORACLE_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: schema_name.to_owned(),
            table_name: fixture.table.clone(),
            row_limit: Some(2),
        })
        .await
        .expect("native Oracle table preview must be accepted");
    assert!(preview.sql.contains(&fixture.table));
    let preview_result = wait_for_result(application, &preview.operation_id).await;
    assert_eq!(preview_result.row_count, "2");
    assert_eq!(
        result_page(application, &preview_result).await.rows.len(),
        2
    );
}

async fn verify_type_matrix(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    fixture: &OracleFixture,
) {
    let sql = oracle_type_matrix_sql(schema_name, fixture);
    let query = application
        .start_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: sql.clone(),
            parameters: Vec::new(),
            limits: query_limits("10"),
        })
        .await
        .expect("native Oracle type matrix query must be accepted");
    let result = wait_for_result(application, &query.operation_id).await;
    let page = result_page(application, &result).await;
    assert_eq!(page.rows.len(), 1);
    assert_oracle_type_matrix(&page.rows[0].values);

    let console = application
        .execute_native_console(
            NativeConsoleRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                sql,
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
        .expect("native Oracle Console type matrix must execute");
    assert_eq!(console.len(), 1);
    assert_eq!(console[0].rows.len(), 1);
    assert_oracle_type_matrix(&console[0].rows[0].values);

    verify_unsupported_type_result(
        application,
        datasource_id,
        database_name,
        "SELECT CAST(1.25 AS BINARY_FLOAT), CAST(2.5 AS BINARY_DOUBLE) FROM DUAL",
    )
    .await;
    verify_unsupported_type_result(
        application,
        datasource_id,
        database_name,
        &format!(
            "SELECT TO_CLOB('probe'), ROWID FROM \"{schema_name}\".\"{}\" WHERE ID = 1",
            fixture.table
        ),
    )
    .await;
}

async fn verify_unsupported_type_result(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    sql: &str,
) {
    let query = application
        .start_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: sql.to_owned(),
            parameters: Vec::new(),
            limits: query_limits("10"),
        })
        .await
        .expect("unsupported Oracle type query must be accepted before column describe");
    let error = wait_for_failure(application, &query.operation_id).await;
    assert_eq!(
        error.code, "oracle_result_type_not_supported",
        "unsupported SQL must fail after describe: {sql}"
    );
    let error = application
        .execute_native_console(
            NativeConsoleRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                sql: sql.to_owned(),
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
        .expect_err("Oracle Console must reject lossy native result types");
    assert_eq!(error.api_error().code, "oracle_result_type_not_supported");
}

fn oracle_type_matrix_sql(schema_name: &str, fixture: &OracleFixture) -> String {
    format!(
        "SELECT CAST(123.45 AS NUMBER(10,2)) AS NUMBER_VALUE, \
                HEXTORAW('00FF') AS RAW_VALUE, \
                TO_BLOB(HEXTORAW('0102')) AS BLOB_VALUE, \
                TO_CLOB('clob-value') AS CLOB_VALUE, \
                CAST(DATE '2026-08-07' AS DATE) AS DATE_VALUE, \
                TIMESTAMP '2026-08-07 12:34:56.123456' AS TIMESTAMP_VALUE, \
                TIMESTAMP '2026-08-07 12:34:56.123456 +08:00' AS TIMESTAMP_TZ_VALUE, \
                JSON('{{\"ready\":true}}') AS JSON_VALUE, \
                TRUE AS BOOLEAN_VALUE \
           FROM \"{schema_name}\".\"{}\" WHERE ID = 1",
        fixture.table
    )
}

fn assert_oracle_type_matrix(values: &[JdbcValue]) {
    assert_eq!(values.len(), 9);
    assert!(matches!(
        &values[0],
        JdbcValue::Decimal { value }
            if value.parse::<f64>().is_ok_and(|value| (value - 123.45).abs() < f64::EPSILON)
    ));
    assert!(matches!(&values[1], JdbcValue::Binary { value } if value == "AP8="));
    assert!(matches!(&values[2], JdbcValue::Binary { value } if value == "AQI="));
    assert!(matches!(&values[3], JdbcValue::Text { value } if value == "clob-value"));
    assert!(
        matches!(
            &values[4],
            JdbcValue::Timestamp { value } if value == "2026-08-07T00:00:00"
        ),
        "unexpected Oracle type matrix: {values:?}"
    );
    assert!(matches!(
        &values[5],
        JdbcValue::Timestamp { value } if value == "2026-08-07T12:34:56.123456"
    ));
    assert!(matches!(
        &values[6],
        JdbcValue::TimestampWithTimeZone { value }
            if value == "2026-08-07T12:34:56.123456+08:00"
    ));
    assert!(matches!(
        &values[7],
        JdbcValue::Json { value } if value == "{\"ready\":true}"
    ));
    assert!(matches!(&values[8], JdbcValue::Boolean { value: true }));
}

async fn verify_read_only_side_effect(
    application: &Application,
    config: &OracleTestConfig,
    verification_datasource_id: &str,
    schema_name: &str,
    fixture: &OracleFixture,
) {
    let read_only = application
        .create_datasource(CreateDatasourceRequest {
            name: "Native Oracle read only".to_owned(),
            driver_id: "oracle".to_owned(),
            connection: Some(config.read_only_connection()),
        })
        .await
        .expect("read-only native Oracle datasource must persist");
    let query = application
        .start_query(StartQueryRequest {
            datasource_id: read_only.id,
            sql: format!(
                "SELECT \"{schema_name}\".\"{}\"() FROM DUAL",
                fixture.side_effect_function
            ),
            parameters: Vec::new(),
            limits: query_limits("10"),
        })
        .await
        .expect("read-only Oracle side-effect query must be accepted before dispatch");
    let error = wait_for_failure(application, &query.operation_id).await;
    assert!(
        matches!(
            error.code.as_str(),
            "oracle_query_failed" | "oracle_connection_failed"
        ),
        "read-only Oracle side-effect rejection returned an unexpected error: {error:?}"
    );

    let verification = application
        .start_query(StartQueryRequest {
            datasource_id: verification_datasource_id.to_owned(),
            sql: format!(
                "SELECT SCORE FROM \"{schema_name}\".\"{}\" WHERE ID = 1",
                fixture.table
            ),
            parameters: Vec::new(),
            limits: query_limits("10"),
        })
        .await
        .expect("Oracle side-effect verification query must be accepted");
    let result = wait_for_result(application, &verification.operation_id).await;
    let page = result_page(application, &result).await;
    assert!(matches!(
        &page.rows[0].values[0],
        JdbcValue::Decimal { value }
            if value.parse::<f64>().is_ok_and(|value| (value - 10.5).abs() < f64::EPSILON)
    ));
}

fn table_keys(
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> ListCommunityTableKeysRequest {
    ListCommunityTableKeysRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: ORACLE_DATABASE_TYPE.to_owned(),
        database_name: database_name.to_owned(),
        schema_name: schema_name.to_owned(),
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
        .expect("native Oracle query operation must be subscribable");
    tokio::time::timeout(EVENT_TIMEOUT, async {
        while let Some(envelope) = subscription
            .next_event()
            .await
            .expect("native Oracle operation event must decode")
        {
            match envelope.event {
                OperationEvent::Completed { result } => return result,
                OperationEvent::Failed { error } => {
                    panic!("native Oracle query failed: {error:?}")
                }
                OperationEvent::Cancelled { reason } => {
                    panic!("native Oracle query was cancelled: {reason:?}")
                }
                OperationEvent::Started | OperationEvent::Progress { .. } => {}
            }
        }
        panic!("native Oracle operation ended without a terminal event")
    })
    .await
    .expect("native Oracle query must finish before timeout")
}

async fn wait_for_failure(
    application: &Application,
    operation_id: &str,
) -> chat2db_contract::ApiError {
    let mut subscription = application
        .subscribe_operation(operation_id, None)
        .await
        .expect("failed native Oracle operation must be subscribable");
    tokio::time::timeout(EVENT_TIMEOUT, async {
        while let Some(envelope) = subscription
            .next_event()
            .await
            .expect("native Oracle operation event must decode")
        {
            match envelope.event {
                OperationEvent::Failed { error } => return error,
                OperationEvent::Completed { result } => {
                    panic!("native Oracle query unexpectedly completed: {result:?}")
                }
                OperationEvent::Cancelled { reason } => {
                    panic!("native Oracle query was cancelled: {reason:?}")
                }
                OperationEvent::Started | OperationEvent::Progress { .. } => {}
            }
        }
        panic!("native Oracle operation ended without a failure event")
    })
    .await
    .expect("native Oracle query must fail before timeout")
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
        .expect("native Oracle result page must be retained")
}

async fn provision_fixture(config: &OracleTestConfig, fixture: &OracleFixture) {
    let connection = Connection::connect_with_config(config.driver_config())
        .await
        .expect("native Oracle fixture connection must open");
    for sql in fixture_ddl(fixture) {
        connection
            .execute(&sql, &[])
            .await
            .unwrap_or_else(|error| panic!("Oracle fixture SQL failed: {sql}: {error}"));
    }
    connection
        .execute(
            &format!("INSERT INTO {} (ID) VALUES (1)", fixture.parent_table),
            &[],
        )
        .await
        .expect("Oracle parent fixture row must insert");
    connection
        .execute(
            &format!(
                "INSERT ALL \
                 INTO {} (ID, PARENT_ID, LABEL, SCORE) VALUES (1, 1, ' alpha ', 10.50) \
                 INTO {} (ID, PARENT_ID, LABEL, SCORE) VALUES (2, 1, 'beta', 20.25) \
                 SELECT 1 FROM DUAL",
                fixture.table, fixture.table
            ),
            &[],
        )
        .await
        .expect("Oracle child fixture rows must insert");
    connection
        .commit()
        .await
        .expect("Oracle fixture transaction must commit");
    connection
        .close()
        .await
        .expect("Oracle fixture connection must close");
}

fn fixture_ddl(fixture: &OracleFixture) -> Vec<String> {
    vec![
        format!(
            "CREATE TABLE {} (ID NUMBER(10) CONSTRAINT C2PP_{} PRIMARY KEY)",
            fixture.parent_table,
            fixture_suffix(&fixture.parent_table)
        ),
        format!(
            "CREATE TABLE {} (\
             ID NUMBER(10) CONSTRAINT C2TP_{} PRIMARY KEY, \
             PARENT_ID NUMBER(10) NOT NULL, LABEL VARCHAR2(64) NOT NULL, \
             SCORE NUMBER(10,2), \
             SCORE_X2 NUMBER GENERATED ALWAYS AS (SCORE * 2) VIRTUAL, \
             HIDDEN_VALUE NUMBER INVISIBLE, \
             CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP, \
             CONSTRAINT {} FOREIGN KEY (PARENT_ID) REFERENCES {} (ID))",
            fixture.table,
            fixture_suffix(&fixture.table),
            fixture.foreign_key,
            fixture.parent_table
        ),
        format!(
            "CREATE INDEX {} ON {} (LABEL)",
            fixture.index, fixture.table
        ),
        format!(
            "CREATE VIEW {} AS SELECT ID, LABEL FROM {}",
            fixture.view, fixture.table
        ),
        format!(
            "CREATE FUNCTION {} (P_VALUE IN NUMBER) RETURN NUMBER IS \
             BEGIN RETURN P_VALUE + 1; END;",
            fixture.function
        ),
        format!(
            "CREATE FUNCTION {} RETURN NUMBER IS \
             BEGIN UPDATE {} SET SCORE = SCORE + 100 WHERE ID = 1; \
             RETURN SQL%ROWCOUNT; END;",
            fixture.side_effect_function, fixture.table
        ),
        format!(
            "CREATE PROCEDURE {} (P_INPUT IN NUMBER, P_OUTPUT OUT NUMBER) IS \
             BEGIN P_OUTPUT := P_INPUT + 1; END;",
            fixture.procedure
        ),
        format!(
            "CREATE TRIGGER {} BEFORE INSERT ON {} FOR EACH ROW \
             BEGIN :NEW.LABEL := TRIM(:NEW.LABEL); END;",
            fixture.trigger, fixture.table
        ),
    ]
}

async fn cleanup_fixture(config: &OracleTestConfig, fixture: &OracleFixture) -> Result<(), String> {
    let connection = Connection::connect_with_config(config.driver_config())
        .await
        .map_err(|error| error.to_string())?;
    let drops = [
        format!("DROP TRIGGER {}", fixture.trigger),
        format!("DROP FUNCTION {}", fixture.side_effect_function),
        format!("DROP FUNCTION {}", fixture.function),
        format!("DROP PROCEDURE {}", fixture.procedure),
        format!("DROP VIEW {}", fixture.view),
        format!("DROP TABLE {} PURGE", fixture.table),
        format!("DROP TABLE {} PURGE", fixture.parent_table),
    ];
    let mut cleanup_error = None;
    for sql in drops {
        if let Err(error) = connection.execute(&sql, &[]).await
            && !missing_fixture_object(&error)
            && cleanup_error.is_none()
        {
            cleanup_error = Some(format!("{sql}: {error}"));
        }
    }
    if let Err(error) = connection.close().await
        && cleanup_error.is_none()
    {
        cleanup_error = Some(error.to_string());
    }
    cleanup_error.map_or(Ok(()), Err)
}

fn missing_fixture_object(error: &OracleError) -> bool {
    let message = error.to_string();
    ["ORA-00942", "ORA-04043", "ORA-04080"]
        .iter()
        .any(|code| message.contains(code))
}

fn fixture_suffix(name: &str) -> &str {
    name.rsplit_once('_').map_or(name, |(_, suffix)| suffix)
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

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be configured"))
}
