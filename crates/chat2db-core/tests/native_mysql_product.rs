use std::{panic::AssertUnwindSafe, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    CancelDisposition, ComponentState, CreateDatasourceRequest, DatasourceConnection,
    DatasourceConnectionProperty, JdbcValue, ListCommunityDatabasesRequest,
    ListCommunitySchemasRequest, ListCommunityTablesRequest, OperationEvent, OperationStatus,
    QueryLimits, QueryParameter, ResultMetadata, ResultPageRequest,
    StartCommunityTablePreviewRequest, StartQueryRequest,
};
use chat2db_core::{Application, RuntimeConfig, RuntimeHost};
use chat2db_java_bridge::{EngineCommand, EngineConfig};
use futures_util::FutureExt as _;
use mysql_async::{Conn, Opts, OptsBuilder, prelude::Queryable};
use tempfile::TempDir;
use uuid::Uuid;

const MYSQL_DATABASE_TYPE: &str = "MYSQL";
const EVENT_TIMEOUT: Duration = Duration::from_secs(15);
const REQUIRED_MYSQL_ENV: [&str; 4] = [
    "MYSQL_TEST_HOST",
    "MYSQL_TEST_PORT",
    "MYSQL_TEST_USER",
    "MYSQL_TEST_PASSWORD",
];

struct MysqlTestConfig {
    host: String,
    port: u16,
    user: String,
    password: String,
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
                "MYSQL_TEST_REQUIRED is enabled but the MySQL endpoint is absent"
            );
            eprintln!("skipping native MySQL product test; MYSQL_TEST_* variables are absent");
            return None;
        }
        assert_eq!(
            configured,
            REQUIRED_MYSQL_ENV.len(),
            "native MySQL integration is partially configured"
        );
        let host = required_env("MYSQL_TEST_HOST");
        assert!(
            !host.trim().is_empty()
                && !host.chars().any(char::is_control)
                && !host.contains(['/', '?', '#']),
            "MYSQL_TEST_HOST is invalid"
        );
        let port = required_env("MYSQL_TEST_PORT")
            .parse::<u16>()
            .expect("MYSQL_TEST_PORT must be a TCP port");
        assert_ne!(port, 0, "MYSQL_TEST_PORT cannot be zero");
        let user = required_env("MYSQL_TEST_USER");
        assert!(!user.is_empty(), "MYSQL_TEST_USER cannot be empty");
        Some(Self {
            host,
            port,
            user,
            password: required_env("MYSQL_TEST_PASSWORD"),
        })
    }

    fn native_options(&self) -> Opts {
        OptsBuilder::default()
            .ip_or_hostname(self.host.clone())
            .tcp_port(self.port)
            .user(Some(self.user.clone()))
            .pass(Some(self.password.clone()))
            .prefer_socket(Some(false))
            .into()
    }

    fn connection(&self, database_name: Option<&str>) -> DatasourceConnection {
        let host = if self.host.contains(':')
            && !(self.host.starts_with('[') && self.host.ends_with(']'))
        {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        DatasourceConnection {
            jdbc_url: format!(
                "jdbc:mysql://{host}:{}/{database}?useSSL=false&serverTimezone=UTC",
                self.port,
                database = database_name.unwrap_or_default()
            ),
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
        }
    }
}

#[tokio::test]
async fn native_mysql_product_paths_keep_java_dormant() {
    let Some(config) = MysqlTestConfig::from_environment() else {
        return;
    };
    let database_name = format!("chat2db_native_it_{}", Uuid::new_v4().simple());
    provision_database(&config, &database_name).await;

    let verification = AssertUnwindSafe(verify_native_product(&config, &database_name))
        .catch_unwind()
        .await;
    let cleanup = cleanup_database(&config, &database_name).await;
    if let Err(payload) = verification {
        if let Err(error) = cleanup {
            eprintln!("native MySQL cleanup also failed: {error}");
        }
        std::panic::resume_unwind(payload);
    }
    cleanup.expect("native MySQL fixture must be removed");
}

async fn verify_native_product(config: &MysqlTestConfig, database_name: &str) {
    let directory = TempDir::new().expect("temporary native MySQL runtime");
    let missing_java = directory.path().join("missing-java");
    let runtime = RuntimeConfig::new(EngineConfig::new(EngineCommand::new(missing_java)))
        .with_data_dir(directory.path().join("data"))
        .with_vault_master_key_base64(STANDARD.encode([0x6d; 32]));
    let mut host = RuntimeHost::open(runtime)
        .await
        .expect("native MySQL runtime must open without Java");
    let application = host.application();
    assert_java_dormant(&application);
    assert!(application.list_drivers().items.is_empty());

    let unknown_driver = application
        .test_datasource_connection("notmysql", config.connection(None))
        .await
        .expect_err("unknown driver ids must not enter the native MySQL path");
    assert_eq!(unknown_driver.api_error().code, "driver_not_installed");
    assert_java_dormant(&application);

    application
        .test_datasource_connection("mysql", config.connection(None))
        .await
        .expect("native MySQL connection test must succeed without a JDBC pack");
    assert_java_dormant(&application);

    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Native MySQL".to_owned(),
            driver_id: "mysql".to_owned(),
            connection: Some(config.connection(Some(database_name))),
        })
        .await
        .expect("native MySQL datasource must persist without a JDBC pack");

    verify_native_metadata(&application, &datasource.id, database_name).await;
    verify_native_preview(&application, &datasource.id, database_name).await;
    verify_native_console(&application, &datasource.id).await;
    verify_rejected_native_selects(&application, &datasource.id).await;
    verify_native_truncation(&application, &datasource.id).await;
    verify_native_cancellation(&application, &datasource.id, config, database_name).await;

    host.shutdown()
        .await
        .expect("native-only runtime must shut down cleanly");
}

async fn verify_native_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let databases = application
        .list_community_databases(ListCommunityDatabasesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
        })
        .await
        .expect("native MySQL databases must list");
    assert!(
        databases
            .items
            .iter()
            .any(|database| database.name == database_name)
    );
    assert!(
        databases
            .items
            .iter()
            .find(|database| database.name == "information_schema")
            .is_some_and(|database| database.system)
    );
    assert_java_dormant(application);

    let schemas = application
        .list_community_schemas(ListCommunitySchemasRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
        })
        .await
        .expect("MySQL schema route must stay native");
    assert!(schemas.items.is_empty());

    let tables = application
        .list_community_tables(ListCommunityTablesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            table_name_pattern: String::new(),
        })
        .await
        .expect("native MySQL tables must list");
    let table = tables
        .items
        .iter()
        .find(|table| table.name == "items")
        .expect("fixture table must be visible");
    assert_eq!(table.database_name, database_name);
    assert_eq!(table.table_type, "TABLE");
    assert_eq!(table.engine, "InnoDB");
    assert_java_dormant(application);
}

async fn verify_native_preview(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let preview = application
        .start_community_table_preview(StartCommunityTablePreviewRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: MYSQL_DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: String::new(),
            table_name: "items".to_owned(),
            row_limit: Some(2),
        })
        .await
        .expect("native MySQL preview must be accepted");
    assert_eq!(
        preview.sql,
        format!("SELECT * FROM `{database_name}`.`items` LIMIT 2")
    );
    let preview_result = wait_for_result(application, &preview.operation_id).await;
    assert_eq!(preview_result.row_count, "2");
    assert!(!preview_result.truncated_by_max_rows);
    let preview_page = result_page(application, &preview_result).await;
    assert_eq!(preview_page.rows.len(), 2);
    assert_java_dormant(application);
}

async fn verify_native_console(application: &Application, datasource_id: &str) {
    let query = application
        .start_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: "SELECT id, label, amount, active, created_at FROM items ORDER BY id".to_owned(),
            parameters: Vec::new(),
            limits: query_limits("10"),
        })
        .await
        .expect("native MySQL Console SELECT must be accepted");
    let query_result = wait_for_result(application, &query.operation_id).await;
    assert_eq!(query_result.row_count, "3");
    let page = result_page(application, &query_result).await;
    assert_eq!(page.rows.len(), 3);
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
            && created_at == "2026-07-27T12:34:56"
    ));
    assert_java_dormant(application);
}

async fn verify_rejected_native_selects(application: &Application, datasource_id: &str) {
    let parameterized = application
        .start_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: "SELECT ?".to_owned(),
            parameters: vec![QueryParameter {
                position: 1,
                value: JdbcValue::SignedInteger {
                    value: "1".to_owned(),
                },
            }],
            limits: query_limits("10"),
        })
        .await
        .expect_err("parameterized native SELECT must fail without starting Java");
    assert_eq!(parameterized.api_error().code, "invalid_query_request");

    let cte = application
        .start_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: "WITH selected AS (SELECT 1) SELECT * FROM selected".to_owned(),
            parameters: Vec::new(),
            limits: query_limits("10"),
        })
        .await
        .expect_err("unsupported native SELECT must fail without starting Java");
    assert_eq!(cte.api_error().code, "mysql_native_query_unsupported");
    assert_java_dormant(application);
}

async fn verify_native_truncation(application: &Application, datasource_id: &str) {
    let query = application
        .start_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: "SELECT id FROM items ORDER BY id".to_owned(),
            parameters: Vec::new(),
            limits: query_limits("1"),
        })
        .await
        .expect("bounded native SELECT must be accepted");
    let result = wait_for_result(application, &query.operation_id).await;
    assert_eq!(result.row_count, "1");
    assert!(result.truncated_by_max_rows);
    assert_eq!(result_page(application, &result).await.rows.len(), 1);
    assert_java_dormant(application);
}

async fn verify_native_cancellation(
    application: &Application,
    datasource_id: &str,
    config: &MysqlTestConfig,
    database_name: &str,
) {
    let query = application
        .start_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: "SELECT SLEEP(30)".to_owned(),
            parameters: Vec::new(),
            limits: query_limits("1"),
        })
        .await
        .expect("cancellable native SELECT must be accepted");
    let mut subscription = application
        .subscribe_operation(&query.operation_id, Some(0))
        .await
        .expect("native cancellation operation must be subscribable");
    wait_for_active_sleep(config, database_name).await;

    let cancellation = application.cancel_operation(&query.operation_id).await;
    assert_eq!(cancellation.disposition, CancelDisposition::Accepted);
    tokio::time::timeout(EVENT_TIMEOUT, async {
        loop {
            let envelope = subscription
                .next_event()
                .await
                .expect("operation event must decode")
                .expect("operation must emit a cancelled event");
            if matches!(envelope.event, OperationEvent::Cancelled { .. }) {
                break;
            }
        }
    })
    .await
    .expect("native MySQL query must cancel before timeout");
    let snapshot = application
        .operation_snapshot(&query.operation_id)
        .await
        .expect("cancelled native operation must remain inspectable");
    assert_eq!(snapshot.status, OperationStatus::Cancelled);
    assert!(snapshot.result.is_none());
    assert_java_dormant(application);
}

async fn wait_for_active_sleep(config: &MysqlTestConfig, database_name: &str) {
    let mut conn = Conn::new(config.native_options())
        .await
        .expect("process-list probe must connect");
    tokio::time::timeout(EVENT_TIMEOUT, async {
        loop {
            let active = conn
                .exec_first::<u64, _, _>(
                    "SELECT COUNT(*) FROM information_schema.PROCESSLIST \
                     WHERE DB = ? AND INFO LIKE 'SELECT SLEEP(30)%'",
                    (database_name,),
                )
                .await
                .expect("process-list probe must succeed")
                .unwrap_or_default();
            if active > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("sleep query must become active before cancellation");
    conn.disconnect()
        .await
        .expect("process-list probe must disconnect");
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
        .expect("query operation must be subscribable");
    tokio::time::timeout(EVENT_TIMEOUT, async {
        while let Some(envelope) = subscription
            .next_event()
            .await
            .expect("operation event must decode")
        {
            match envelope.event {
                OperationEvent::Completed { result } => return result,
                OperationEvent::Failed { error } => panic!("native MySQL query failed: {error:?}"),
                OperationEvent::Cancelled { reason } => {
                    panic!("native MySQL query was cancelled: {reason:?}")
                }
                OperationEvent::Started | OperationEvent::Progress { .. } => {}
            }
        }
        panic!("native MySQL operation ended without a terminal event")
    })
    .await
    .expect("native MySQL query must finish before timeout")
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
        .expect("native MySQL result page must be retained")
}

async fn provision_database(config: &MysqlTestConfig, database_name: &str) {
    let mut conn = Conn::new(config.native_options())
        .await
        .expect("native MySQL fixture connection must open");
    conn.query_drop(format!(
        "CREATE DATABASE `{database_name}` CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci"
    ))
    .await
    .expect("native MySQL fixture database must create");
    conn.query_drop(format!(
        "CREATE TABLE `{database_name}`.`items` (\
         `id` BIGINT NOT NULL, `label` VARCHAR(128) NOT NULL, \
         `amount` DECIMAL(12,2) NOT NULL, `active` BOOLEAN NOT NULL, \
         `created_at` DATETIME NOT NULL, PRIMARY KEY (`id`)\
         ) ENGINE=InnoDB"
    ))
    .await
    .expect("native MySQL fixture table must create");
    conn.query_drop(format!(
        "INSERT INTO `{database_name}`.`items` VALUES \
         (1, 'mysql-ready', 99.99, TRUE, '2026-07-27 12:34:56'), \
         (2, 'second', 2.50, FALSE, '2026-07-28 01:02:03'), \
         (3, 'third', 3.75, TRUE, '2026-07-29 04:05:06')"
    ))
    .await
    .expect("native MySQL fixture rows must insert");
    conn.disconnect()
        .await
        .expect("native MySQL fixture connection must close");
}

async fn cleanup_database(config: &MysqlTestConfig, database_name: &str) -> Result<(), String> {
    let mut conn = Conn::new(config.native_options())
        .await
        .map_err(|error| error.to_string())?;
    conn.query_drop(format!("DROP DATABASE IF EXISTS `{database_name}`"))
        .await
        .map_err(|error| error.to_string())?;
    conn.disconnect().await.map_err(|error| error.to_string())
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
