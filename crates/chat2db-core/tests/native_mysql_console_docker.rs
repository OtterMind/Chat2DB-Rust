use std::{panic::AssertUnwindSafe, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    ComponentState, CreateDatasourceRequest, DatasourceConnection, DatasourceConnectionProperty,
    JdbcValue,
};
use chat2db_core::{
    Application, MysqlConsoleCancellation, MysqlConsoleRequest, MysqlConsoleResult, RuntimeConfig,
    RuntimeHost,
};
use chat2db_java_bridge::{EngineCommand, EngineConfig};
use futures_util::FutureExt as _;
use mysql_async::{Conn, Opts, OptsBuilder, prelude::Queryable};
use tempfile::TempDir;
use uuid::Uuid;

const MYSQL_SLEEP_SQL: &str = "SELECT SLEEP(30)";
const MYSQL_WAIT_TIMEOUT: Duration = Duration::from_secs(15);

struct MysqlTestConfig {
    host: String,
    port: u16,
    user: String,
    password: String,
}

impl MysqlTestConfig {
    fn from_environment() -> Self {
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
        Self {
            host,
            port,
            user,
            password: required_env("MYSQL_TEST_PASSWORD"),
        }
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

    fn connection(&self, database_name: &str) -> DatasourceConnection {
        let host = if self.host.contains(':')
            && !(self.host.starts_with('[') && self.host.ends_with(']'))
        {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        DatasourceConnection {
            jdbc_url: format!(
                "jdbc:mysql://{host}:{}/{database_name}?useSSL=false&serverTimezone=UTC",
                self.port
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
            ssh: None,
        }
    }
}

#[tokio::test]
#[ignore = "requires MYSQL_TEST_* variables and a real MySQL server"]
async fn native_mysql_console_matches_community_execution_semantics() {
    let config = MysqlTestConfig::from_environment();
    let suffix = Uuid::new_v4().simple().to_string();
    let database_name = format!("chat2db_console_it_{suffix}");
    let table_name = format!("console_items_{}", &suffix[..8]);
    let procedure_name = format!("console_results_{}", &suffix[..8]);
    provision_database(&config, &database_name).await;

    let verification = AssertUnwindSafe(verify_console(
        &config,
        &database_name,
        &table_name,
        &procedure_name,
    ))
    .catch_unwind()
    .await;
    let cleanup = cleanup_database(&config, &database_name).await;
    if let Err(payload) = verification {
        if let Err(error) = cleanup {
            eprintln!("native MySQL Console cleanup also failed: {error}");
        }
        std::panic::resume_unwind(payload);
    }
    cleanup.expect("native MySQL Console fixture must be removed");
}

async fn verify_console(
    config: &MysqlTestConfig,
    database_name: &str,
    table_name: &str,
    procedure_name: &str,
) {
    let directory = TempDir::new().expect("temporary native MySQL Console runtime");
    let missing_java = directory.path().join("missing-java");
    let runtime = RuntimeConfig::new(EngineConfig::new(EngineCommand::new(missing_java)))
        .with_data_dir(directory.path().join("data"))
        .with_vault_master_key_base64(STANDARD.encode([0x6e; 32]));
    let mut host = RuntimeHost::open(runtime)
        .await
        .expect("native MySQL Console runtime must open without Java");
    let application = host.application();
    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Native MySQL Console Docker".to_owned(),
            driver_id: "mysql".to_owned(),
            connection: Some(config.connection(database_name)),
        })
        .await
        .expect("native MySQL datasource must persist without a JDBC driver pack");
    assert_java_dormant(&application);

    verify_ddl_and_dml(&application, &datasource.id, database_name, table_name).await;
    create_multi_result_procedure(&application, &datasource.id, database_name, procedure_name)
        .await;
    verify_multi_result_sets(&application, &datasource.id, database_name, procedure_name).await;
    verify_error_continue(&application, &datasource.id, database_name, table_name).await;
    verify_transactions(&application, &datasource.id, database_name, table_name).await;
    verify_large_value(&application, &datasource.id, database_name, table_name).await;
    verify_console_options(&application, &datasource.id, database_name).await;
    verify_read_only(config, &application, database_name, table_name).await;
    verify_cancellation(config, &application, &datasource.id, database_name).await;
    assert_java_dormant(&application);

    host.shutdown()
        .await
        .expect("native-only runtime must shut down cleanly");
}

async fn verify_ddl_and_dml(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    table_name: &str,
) {
    let ddl = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!(
                "CREATE TABLE `{table_name}` (\
                 `id` BIGINT NOT NULL, `label` VARCHAR(128) NOT NULL, \
                 `score` INT NOT NULL, PRIMARY KEY (`id`)\
                 ) ENGINE=InnoDB"
            ),
            false,
        ),
    )
    .await;
    assert_single_success(&ddl, 0);

    let insert = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!(
                "INSERT INTO `{table_name}` (`id`, `label`, `score`) \
                 VALUES (1, 'first', 10), (2, 'second', 20)"
            ),
            false,
        ),
    )
    .await;
    assert_single_success(&insert, 2);

    let update = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!("UPDATE `{table_name}` SET `score` = 11 WHERE `id` = 1"),
            false,
        ),
    )
    .await;
    assert_single_success(&update, 1);

    let delete = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!("DELETE FROM `{table_name}` WHERE `id` = 2"),
            false,
        ),
    )
    .await;
    assert_single_success(&delete, 1);

    let multi = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!(
                "INSERT INTO `{table_name}` VALUES (3, 'third', 30); \
                 UPDATE `{table_name}` SET `score` = 31 WHERE `id` = 3; \
                 SELECT `id`, `label`, `score` FROM `{table_name}` ORDER BY `id`"
            ),
            false,
        ),
    )
    .await;
    assert_eq!(
        multi
            .iter()
            .map(|result| result.statement_sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(multi.iter().all(|result| result.success));
    let selected = multi.last().expect("multi-statement SELECT result");
    assert_eq!(selected.result_set_id, Some(1));
    assert_eq!(selected.row_count, 2);
    assert_eq!(scalar_text(&selected.rows[0].values[1]), "first");
    assert_eq!(scalar_text(&selected.rows[1].values[1]), "third");
    assert_eq!(scalar_text(&selected.rows[1].values[2]), "31");
}

async fn verify_large_value(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    table_name: &str,
) {
    let alter = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!("ALTER TABLE `{table_name}` ADD COLUMN `payload` LONGTEXT NULL"),
            false,
        ),
    )
    .await;
    assert_single_success(&alter, 0);

    let update = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!(
                "UPDATE `{table_name}` SET `payload` = REPEAT('large-value-', 500000) \
                 WHERE `id` = 1"
            ),
            false,
        ),
    )
    .await;
    assert_single_success(&update, 1);

    let result = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!("SELECT `payload` FROM `{table_name}` WHERE `id` = 1"),
            false,
        ),
    )
    .await;
    let payload = statement_value(result.first().expect("large-value SELECT result"));
    assert_eq!(payload.len(), 6_000_000);
    assert!(payload.starts_with("large-value-"));
    assert!(payload.ends_with("large-value-"));
}

async fn verify_multi_result_sets(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    procedure_name: &str,
) {
    let all_results = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!("CALL `{procedure_name}`()"),
            false,
        ),
    )
    .await;
    let tabular = all_results
        .iter()
        .filter(|result| result.result_set_id.is_some())
        .collect::<Vec<_>>();
    assert_eq!(tabular.len(), 2);
    assert_eq!(tabular[0].result_set_id, Some(1));
    assert_eq!(scalar_text(&tabular[0].rows[0].values[0]), "11");
    assert_eq!(tabular[1].result_set_id, Some(2));
    assert_eq!(scalar_text(&tabular[1].rows[0].values[0]), "22");

    let mut selected_request = request(
        datasource_id,
        database_name,
        format!("CALL `{procedure_name}`()"),
        false,
    );
    selected_request.result_set_id = Some(2);
    let selected = execute(application, selected_request).await;
    let selected_tabular = selected
        .iter()
        .filter(|result| result.result_set_id.is_some())
        .collect::<Vec<_>>();
    assert_eq!(selected_tabular.len(), 1);
    assert_eq!(selected_tabular[0].result_set_id, Some(2));
    assert_eq!(scalar_text(&selected_tabular[0].rows[0].values[0]), "22");
}

async fn verify_error_continue(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    table_name: &str,
) {
    let stop = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!(
                "INSERT INTO `{table_name}` VALUES (100, 'stop-first', 100); \
                 INSERT INTO `{table_name}` VALUES (100, 'duplicate', 100); \
                 INSERT INTO `{table_name}` VALUES (101, 'must-not-run', 101)"
            ),
            false,
        ),
    )
    .await;
    assert_eq!(stop.len(), 2);
    assert!(stop[0].success);
    assert!(!stop[1].success);
    assert_eq!(stop[0].statement_sequence, 1);
    assert_eq!(stop[1].statement_sequence, 2);
    assert!(stop[1].error.is_some());
    assert_eq!(
        query_count(application, datasource_id, database_name, table_name, 101).await,
        "0"
    );

    let proceed = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!(
                "INSERT INTO `{table_name}` VALUES (200, 'continue-first', 200); \
                 INSERT INTO `{table_name}` VALUES (200, 'duplicate', 200); \
                 INSERT INTO `{table_name}` VALUES (201, 'did-run', 201)"
            ),
            true,
        ),
    )
    .await;
    assert_eq!(proceed.len(), 3);
    assert!(proceed[0].success);
    assert!(!proceed[1].success);
    assert!(proceed[2].success);
    assert_eq!(
        proceed
            .iter()
            .map(|result| result.statement_sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(
        query_count(application, datasource_id, database_name, table_name, 201).await,
        "1"
    );
}

async fn verify_transactions(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    table_name: &str,
) {
    let rollback = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!(
                "BEGIN; INSERT INTO `{table_name}` VALUES (300, 'rollback', 300); \
                 ROLLBACK; SELECT COUNT(*) FROM `{table_name}` WHERE `id` = 300"
            ),
            false,
        ),
    )
    .await;
    assert!(rollback.iter().all(|result| result.success));
    assert_eq!(rollback.last().map(statement_value), Some("0"));

    let commit = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!(
                "BEGIN; INSERT INTO `{table_name}` VALUES (301, 'commit', 301); \
                 COMMIT; SELECT COUNT(*) FROM `{table_name}` WHERE `id` = 301"
            ),
            false,
        ),
    )
    .await;
    assert!(commit.iter().all(|result| result.success));
    assert_eq!(commit.last().map(statement_value), Some("1"));
    assert_eq!(
        query_count(application, datasource_id, database_name, table_name, 301).await,
        "1"
    );
}

async fn verify_console_options(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let mut single = request(
        datasource_id,
        database_name,
        "SELECT 7 AS `first_value`; SELECT 8 AS `second_value`".to_owned(),
        false,
    );
    single.single = true;
    let single_results = execute(application, single).await;
    let tabular = single_results
        .iter()
        .filter(|result| result.result_set_id.is_some())
        .collect::<Vec<_>>();
    assert_eq!(tabular.len(), 2);
    assert!(tabular.iter().all(|result| result.statement_sequence == 1));
    assert_eq!(tabular[0].result_set_id, Some(1));
    assert_eq!(tabular[1].result_set_id, Some(2));
    assert_eq!(statement_value(tabular[0]), "7");
    assert_eq!(statement_value(tabular[1]), "8");

    let mut explain = request(
        datasource_id,
        database_name,
        "SELECT 1 AS `value`".to_owned(),
        false,
    );
    explain.explain = true;
    let explained = execute(application, explain).await;
    assert_eq!(explained.len(), 1);
    assert!(explained[0].success);
    assert!(explained[0].sql.starts_with("EXPLAIN SELECT 1"));
    assert!(
        explained[0]
            .columns
            .iter()
            .any(|column| column.label.eq_ignore_ascii_case("select_type"))
    );

    let series_sql = "WITH RECURSIVE `seq` (`n`) AS (\
                      SELECT 1 UNION ALL SELECT `n` + 1 FROM `seq` WHERE `n` < 25\
                      ) SELECT `n` FROM `seq` ORDER BY `n`";
    let mut paged = request(datasource_id, database_name, series_sql.to_owned(), false);
    paged.page_no = 2;
    paged.page_size = 5;
    let page = execute(application, paged.clone()).await;
    assert_eq!(page[0].rows.len(), 5);
    assert_eq!(scalar_text(&page[0].rows[0].values[0]), "6");
    assert!(page[0].has_more);

    paged.page_size_all = true;
    let all_rows = execute(application, paged).await;
    assert_eq!(all_rows[0].rows.len(), 25);
    assert_eq!(scalar_text(&all_rows[0].rows[0].values[0]), "1");
    assert_eq!(scalar_text(&all_rows[0].rows[24].values[0]), "25");
    assert!(!all_rows[0].has_more);
}

async fn verify_read_only(
    config: &MysqlTestConfig,
    application: &Application,
    database_name: &str,
    table_name: &str,
) {
    let mut connection = config.connection(database_name);
    connection.read_only = true;
    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Native MySQL Read Only Docker".to_owned(),
            driver_id: "mysql".to_owned(),
            connection: Some(connection),
        })
        .await
        .expect("read-only native MySQL datasource must persist");

    let inspected = execute(
        application,
        request(
            &datasource.id,
            database_name,
            format!("SELECT COUNT(*) FROM `{table_name}`"),
            false,
        ),
    )
    .await;
    assert!(inspected[0].success);

    let error = application
        .execute_mysql_console(
            request(
                &datasource.id,
                database_name,
                format!("INSERT INTO `{table_name}` (`id`, `label`, `score`) VALUES (900, 'blocked', 900)"),
                false,
            ),
            MysqlConsoleCancellation::new(),
        )
        .await
        .expect_err("read-only datasource must reject writes");
    assert_eq!(error.api_error().code, "datasource_read_only");
    assert_eq!(
        query_count(application, &datasource.id, database_name, table_name, 900).await,
        "0"
    );
}

async fn verify_cancellation(
    config: &MysqlTestConfig,
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let cancellation = MysqlConsoleCancellation::new();
    let execution_cancellation = cancellation.clone();
    let execution_application = application.clone();
    let execution_request = request(
        datasource_id,
        database_name,
        MYSQL_SLEEP_SQL.to_owned(),
        false,
    );
    let execution = tokio::spawn(async move {
        execution_application
            .execute_mysql_console(execution_request, execution_cancellation)
            .await
    });

    wait_for_active_sleep(config, database_name).await;
    assert!(cancellation.cancel(Some("Docker integration test cancellation".to_owned())));
    let error = tokio::time::timeout(MYSQL_WAIT_TIMEOUT, execution)
        .await
        .expect("native MySQL Console cancellation must finish before timeout")
        .expect("native MySQL Console cancellation task must not panic")
        .expect_err("cancelled native MySQL Console execution must fail");
    assert_eq!(error.api_error().code, "mysql_console_cancelled");
}

async fn query_count(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    table_name: &str,
    id: u64,
) -> String {
    let results = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!("SELECT COUNT(*) FROM `{table_name}` WHERE `id` = {id}"),
            false,
        ),
    )
    .await;
    statement_value(results.first().expect("COUNT result")).to_owned()
}

async fn execute(
    application: &Application,
    request: MysqlConsoleRequest,
) -> Vec<MysqlConsoleResult> {
    application
        .execute_mysql_console(request, MysqlConsoleCancellation::new())
        .await
        .expect("native MySQL Console request must complete")
}

fn request(
    datasource_id: &str,
    database_name: &str,
    sql: String,
    error_continue: bool,
) -> MysqlConsoleRequest {
    MysqlConsoleRequest {
        datasource_id: datasource_id.to_owned(),
        database_name: database_name.to_owned(),
        sql,
        page_no: 1,
        page_size: 100,
        result_set_id: None,
        single: false,
        page_size_all: false,
        explain: false,
        error_continue,
    }
}

fn assert_single_success(results: &[MysqlConsoleResult], update_count: u64) {
    assert_eq!(results.len(), 1);
    assert!(results[0].success);
    assert_eq!(results[0].statement_sequence, 1);
    assert_eq!(results[0].update_count, update_count);
    assert!(results[0].error.is_none());
}

fn statement_value(result: &MysqlConsoleResult) -> &str {
    let row = result.rows.first().expect("result must contain one row");
    let value = row.values.first().expect("result must contain one column");
    scalar_text(value)
}

fn scalar_text(value: &JdbcValue) -> &str {
    match value {
        JdbcValue::SignedInteger { value }
        | JdbcValue::UnsignedInteger { value }
        | JdbcValue::Float32 { value }
        | JdbcValue::Float64 { value }
        | JdbcValue::Decimal { value }
        | JdbcValue::Text { value }
        | JdbcValue::Binary { value }
        | JdbcValue::Date { value }
        | JdbcValue::Time { value }
        | JdbcValue::Timestamp { value }
        | JdbcValue::TimestampWithTimeZone { value }
        | JdbcValue::Json { value }
        | JdbcValue::Uuid { value } => value,
        JdbcValue::Opaque { display_value, .. } => display_value,
        JdbcValue::Null | JdbcValue::Boolean { .. } => {
            panic!("expected a scalar value with a textual representation")
        }
    }
}

async fn provision_database(config: &MysqlTestConfig, database_name: &str) {
    let mut conn = Conn::new(config.native_options())
        .await
        .expect("native MySQL Console fixture connection must open");
    conn.query_drop(format!(
        "CREATE DATABASE `{database_name}` CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci"
    ))
    .await
    .expect("native MySQL Console fixture database must create");
    conn.disconnect()
        .await
        .expect("native MySQL Console fixture connection must close");
}

async fn create_multi_result_procedure(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    procedure_name: &str,
) {
    let results = execute(
        application,
        request(
            datasource_id,
            database_name,
            format!(
                "DELIMITER $$\nCREATE PROCEDURE `{procedure_name}`()\n\
                 BEGIN\nSELECT 11 AS `first_value`; SELECT 22 AS `second_value`;\nEND$$\n\
                 DELIMITER ;"
            ),
            false,
        ),
    )
    .await;
    assert_single_success(&results, 0);
}

async fn wait_for_active_sleep(config: &MysqlTestConfig, database_name: &str) {
    let mut conn = Conn::new(config.native_options())
        .await
        .expect("native MySQL process-list probe must connect");
    tokio::time::timeout(MYSQL_WAIT_TIMEOUT, async {
        loop {
            let active = conn
                .exec_first::<u64, _, _>(
                    "SELECT COUNT(*) FROM information_schema.PROCESSLIST \
                     WHERE DB = ? AND INFO LIKE 'SELECT SLEEP(30)%'",
                    (database_name,),
                )
                .await
                .expect("native MySQL process-list probe must succeed")
                .unwrap_or_default();
            if active > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("native MySQL sleep query must become active before cancellation");
    conn.disconnect()
        .await
        .expect("native MySQL process-list probe must disconnect");
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
