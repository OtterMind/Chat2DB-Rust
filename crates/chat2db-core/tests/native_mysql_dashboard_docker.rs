use std::panic::AssertUnwindSafe;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    ComponentState, CreateCommunityChartRequest, CreateDatasourceRequest, DatasourceConnection,
    DatasourceConnectionProperty, UpdateCommunityChartRequest,
};
use chat2db_core::{Application, RuntimeConfig, RuntimeHost};
use chat2db_java_bridge::{EngineCommand, EngineConfig};
use chat2db_storage::OperationLogListQuery;
use futures_util::FutureExt as _;
use mysql_async::{Conn, Error as MysqlError, Opts, OptsBuilder, prelude::Queryable};
use serde_json::{Value, json};
use tempfile::TempDir;
use uuid::Uuid;

const REQUIRED_MYSQL_ENV: [&str; 4] = [
    "MYSQL_TEST_HOST",
    "MYSQL_TEST_PORT",
    "MYSQL_TEST_USER",
    "MYSQL_TEST_PASSWORD",
];
const CHART_CONSOLE_ID: &str = "native-dashboard-chart-it";

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
            eprintln!("skipping native MySQL Dashboard test; MYSQL_TEST_* variables are absent");
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
async fn native_mysql_dashboard_refresh_is_bounded_read_only_and_keeps_java_dormant() {
    let Some(config) = MysqlTestConfig::from_environment() else {
        return;
    };
    let suffix = Uuid::new_v4().simple().to_string();
    let default_database = format!("chat2db_dash_default_{}", &suffix[..12]);
    let selected_database = format!("chat2db_dash_selected_{}", &suffix[..12]);

    let verification = AssertUnwindSafe(async {
        provision_databases(&config, &default_database, &selected_database).await;
        verify_dashboard_refresh(&config, &default_database, &selected_database).await;
    })
    .catch_unwind()
    .await;
    let cleanup = cleanup_databases(&config, &default_database, &selected_database).await;
    if let Err(payload) = verification {
        if let Err(error) = cleanup {
            eprintln!("native MySQL Dashboard cleanup also failed: {error}");
        }
        std::panic::resume_unwind(payload);
    }
    cleanup.expect("native MySQL Dashboard fixtures must be removed");
}

#[allow(clippy::too_many_lines)]
async fn verify_dashboard_refresh(
    config: &MysqlTestConfig,
    default_database: &str,
    selected_database: &str,
) {
    let directory = TempDir::new().expect("temporary native MySQL Dashboard runtime");
    let missing_java = directory.path().join("missing-java");
    let runtime = RuntimeConfig::new(EngineConfig::new(EngineCommand::new(missing_java)))
        .with_data_dir(directory.path().join("data"))
        .with_vault_master_key_base64(STANDARD.encode([0x64; 32]));
    let mut host = RuntimeHost::open(runtime)
        .await
        .expect("native MySQL Dashboard runtime must open without Java");
    let application = host.application();
    assert_java_dormant(&application);

    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Native MySQL Dashboard".to_owned(),
            driver_id: "mysql".to_owned(),
            connection: Some(config.connection(default_database)),
        })
        .await
        .expect("native MySQL Dashboard datasource must persist");

    let persisted_metadata = json!({
        "dataList": [["persisted-only"]],
        "headerList": [{"name": "persisted"}]
    });
    let chart_id = application
        .create_community_chart(CreateCommunityChartRequest {
            name: Some("Native MySQL selected-database chart".to_owned()),
            description: Some("real native MySQL Dashboard integration".to_owned()),
            schema: None,
            data_source_id: None,
            data_source_name: Some("Native MySQL Dashboard".to_owned()),
            schema_name: Some(default_database.to_owned()),
            r#type: Some("TABLE".to_owned()),
            database_name: Some(default_database.to_owned()),
            ddl: None,
            deleted: Some("N".to_owned()),
            user_id: None,
            chart_schema: Some(json!({"title": "Native MySQL selected-database chart"})),
            meta_data: Some(persisted_metadata.clone()),
            database_info: Some(database_info(
                &datasource.id,
                selected_database,
                "UPDATE `chart_rows` SET `label` = 'refresh-false-ran' WHERE `id` = 1",
            )),
            refresh_type: Some("MANUAL".to_owned()),
            refresh_cycle: None,
        })
        .await
        .expect("Community chart must persist");

    let without_refresh = application
        .get_community_chart_detail(chart_id, false)
        .await
        .expect("refresh=false chart detail must load")
        .expect("created chart must exist");
    assert_eq!(without_refresh.meta_data, Some(persisted_metadata.clone()));
    assert_target_unchanged(config, selected_database).await;
    assert_java_dormant(&application);

    let select_sql = "SELECT `id`, `label`, `optional_note`, `payload`, `enabled` FROM `chart_rows` ORDER BY `id`";
    update_chart_sql(
        &application,
        chart_id,
        &datasource.id,
        selected_database,
        select_sql,
    )
    .await;
    let refreshed = application
        .get_community_chart_detail(chart_id, true)
        .await
        .expect("selected-database chart refresh must succeed")
        .expect("created chart must exist");
    let metadata = refreshed.meta_data.expect("refreshed metadata");
    let data_list = metadata["dataList"]
        .as_array()
        .expect("refreshed dataList must be an array");
    assert_eq!(data_list.len(), 200, "chart rows must be capped at 200");
    assert_eq!(
        data_list[0],
        json!(["1", "target-001", null, "{\"row\": 1}", "true"])
    );
    assert_eq!(
        data_list[199],
        json!(["200", "target-200", null, "{\"row\": 200}", "true"])
    );
    assert_eq!(metadata["headerList"][0]["name"], "id");
    assert_eq!(metadata["headerList"][0]["dataType"], "NUMERIC");
    assert_eq!(metadata["headerList"][0]["primaryKey"], true);
    assert_eq!(metadata["headerList"][0]["autoIncrement"], 1);
    assert_eq!(metadata["headerList"][0]["nullable"], 0);
    assert_eq!(metadata["headerList"][0]["editorType"], "TEXT");
    assert_eq!(metadata["headerList"][1]["name"], "label");
    assert_eq!(metadata["headerList"][1]["dataType"], "STRING");
    assert_eq!(metadata["headerList"][1]["comment"], "Chart label");
    assert_eq!(metadata["headerList"][1]["defaultValue"], "unset");
    assert_eq!(metadata["headerList"][1]["nullable"], 0);
    assert_eq!(metadata["headerList"][2]["nullable"], 1);
    assert_eq!(metadata["headerList"][3]["name"], "payload");
    assert_eq!(metadata["headerList"][3]["dataType"], "STRING");
    assert_eq!(metadata["headerList"][3]["editorType"], "TEXT");
    assert_eq!(metadata["headerList"][4]["name"], "enabled");
    assert_eq!(metadata["headerList"][4]["dataType"], "BIT");
    assert_eq!(metadata["headerList"][4]["editorType"], "TEXT");

    let persisted_after_refresh = application
        .get_community_chart(chart_id)
        .await
        .expect("persisted chart must load")
        .expect("created chart must remain present");
    assert_eq!(
        persisted_after_refresh.meta_data,
        Some(persisted_metadata.clone()),
        "refreshed metadata must remain response-only"
    );
    assert_java_dormant(&application);

    let cte_sql = "WITH `selected` AS (SELECT `id`, `label` FROM `chart_rows` WHERE `id` = 201) SELECT `id`, `label` FROM `selected`";
    update_chart_sql(
        &application,
        chart_id,
        &datasource.id,
        selected_database,
        cte_sql,
    )
    .await;
    let cte = application
        .get_community_chart_detail(chart_id, true)
        .await
        .expect("SELECT CTE chart refresh must succeed")
        .expect("created chart must exist");
    assert_eq!(
        cte.meta_data.expect("CTE metadata")["dataList"],
        json!([["201", "target-201"]])
    );

    let rejected_sql = [
        "UPDATE `chart_rows` SET `label` = 'mutated' WHERE `id` = 1".to_owned(),
        "SELECT `id` FROM `chart_rows` LIMIT 1; UPDATE `chart_rows` SET `label` = 'mutated' WHERE `id` = 1".to_owned(),
        "SELECT `id` FROM `chart_rows` WHERE `id` = 1 FOR UPDATE".to_owned(),
        "SELECT `id` FROM `chart_rows` WHERE `id` = 1 FOR SHARE".to_owned(),
        format!(
            "SELECT `id` /*! INTO OUTFILE '/tmp/{selected_database}-comment' */ FROM `chart_rows` LIMIT 1"
        ),
        "SELECT `id` FROM `chart_rows` /*M! FOR SHARE */ LIMIT 1".to_owned(),
        format!(
            "SELECT `id` INTO OUTFILE '/tmp/{selected_database}' FROM `chart_rows` LIMIT 1"
        ),
    ];
    for sql in &rejected_sql {
        update_chart_sql(
            &application,
            chart_id,
            &datasource.id,
            selected_database,
            sql,
        )
        .await;
        let error = application
            .get_community_chart_detail(chart_id, true)
            .await
            .expect_err("unsafe chart SQL must fail closed");
        assert_eq!(error.api_error().code, "chart_query_must_be_read_only");
    }
    assert_target_unchanged(config, selected_database).await;
    assert_java_dormant(&application);

    let history = application
        .storage()
        .expect("Dashboard runtime storage")
        .list_operation_logs(&OperationLogListQuery {
            data_source_id: Some(datasource.id.clone()),
            database_name: Some(selected_database.to_owned()),
            schema_name: Some(selected_database.to_owned()),
            operation_type: Some("SQL_EXECUTE".to_owned()),
            search_key: None,
            page_no: 1,
            page_size: 50,
        })
        .expect("chart operation history must load");
    assert_eq!(history.total, 9, "refresh=false must not create history");
    assert_eq!(
        history
            .records
            .iter()
            .filter(|record| record.status == "success")
            .count(),
        2
    );
    assert_eq!(
        history
            .records
            .iter()
            .filter(|record| record.status == "fail")
            .count(),
        7
    );
    for record in &history.records {
        let extend_info: Value = serde_json::from_str(
            record
                .extend_info
                .as_deref()
                .expect("chart history extendInfo"),
        )
        .expect("chart history extendInfo must be JSON");
        assert_eq!(extend_info["source"], "CHART");
        assert_eq!(extend_info["chartId"], chart_id);
        assert_eq!(extend_info["consoleId"], CHART_CONSOLE_ID);
        assert_eq!(
            record.data_source_id.as_deref(),
            Some(datasource.id.as_str())
        );
        assert_eq!(record.database_name.as_deref(), Some(selected_database));
        assert_eq!(record.schema_name.as_deref(), Some(selected_database));
    }
    assert_java_dormant(&application);

    host.shutdown()
        .await
        .expect("native-only Dashboard runtime must shut down cleanly");
}

async fn update_chart_sql(
    application: &Application,
    chart_id: i64,
    datasource_id: &str,
    database_name: &str,
    sql: &str,
) {
    application
        .update_community_chart(
            chart_id,
            UpdateCommunityChartRequest {
                database_info: Some(database_info(datasource_id, database_name, sql)),
                ..UpdateCommunityChartRequest::default()
            },
        )
        .await
        .expect("chart databaseInfo update must persist");
}

fn database_info(datasource_id: &str, database_name: &str, sql: &str) -> Value {
    json!({
        "dataSourceId": datasource_id,
        "databaseName": database_name,
        "schemaName": database_name,
        "consoleId": CHART_CONSOLE_ID,
        "sql": sql,
    })
}

async fn assert_target_unchanged(config: &MysqlTestConfig, database_name: &str) {
    let mut conn = Conn::new(config.native_options())
        .await
        .expect("target-state probe must connect");
    let count = conn
        .query_first::<u64, _>(format!(
            "SELECT COUNT(*) FROM `{database_name}`.`chart_rows`"
        ))
        .await
        .expect("target row count must load")
        .expect("target row count must exist");
    let label = conn
        .query_first::<String, _>(format!(
            "SELECT `label` FROM `{database_name}`.`chart_rows` WHERE `id` = 1"
        ))
        .await
        .expect("target label must load")
        .expect("target row 1 must exist");
    conn.disconnect()
        .await
        .expect("target-state probe must disconnect");
    assert_eq!(count, 201);
    assert_eq!(label, "target-001");
}

async fn provision_databases(
    config: &MysqlTestConfig,
    default_database: &str,
    selected_database: &str,
) {
    cleanup_databases(config, default_database, selected_database)
        .await
        .expect("stale native MySQL Dashboard fixtures must be removable");
    let mut conn = Conn::new(config.native_options())
        .await
        .expect("native MySQL Dashboard fixture must connect");
    conn.query_drop(format!(
        "CREATE DATABASE `{default_database}` CHARACTER SET utf8mb4"
    ))
    .await
    .expect("default fixture database must be created");
    conn.query_drop(format!(
        "CREATE DATABASE `{selected_database}` CHARACTER SET utf8mb4"
    ))
    .await
    .expect("selected fixture database must be created");
    conn.query_drop(format!(
        "CREATE TABLE `{default_database}`.`chart_rows` (\
         `id` BIGINT NOT NULL PRIMARY KEY, `label` VARCHAR(64) NOT NULL, \
         `optional_note` VARCHAR(64) NULL) ENGINE=InnoDB"
    ))
    .await
    .expect("default fixture table must be created");
    conn.query_drop(format!(
        "INSERT INTO `{default_database}`.`chart_rows` (`id`, `label`) VALUES (999, 'decoy')"
    ))
    .await
    .expect("default fixture decoy row must be inserted");
    conn.query_drop(format!(
        "CREATE TABLE `{selected_database}`.`chart_rows` (\
         `id` BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY, \
         `label` VARCHAR(64) NOT NULL DEFAULT 'unset' COMMENT 'Chart label', \
         `optional_note` VARCHAR(64) NULL, `payload` JSON NULL, \
         `enabled` BIT(1) NOT NULL DEFAULT b'1') ENGINE=InnoDB"
    ))
    .await
    .expect("selected fixture table must be created");
    let values = (1_u16..=201)
        .map(|id| format!("({id}, 'target-{id:03}', NULL, JSON_OBJECT('row', {id}), b'1')"))
        .collect::<Vec<_>>()
        .join(", ");
    conn.query_drop(format!(
        "INSERT INTO `{selected_database}`.`chart_rows` \
         (`id`, `label`, `optional_note`, `payload`, `enabled`) VALUES {values}"
    ))
    .await
    .expect("selected fixture rows must be inserted");
    conn.disconnect()
        .await
        .expect("native MySQL Dashboard fixture must disconnect");
}

async fn cleanup_databases(
    config: &MysqlTestConfig,
    default_database: &str,
    selected_database: &str,
) -> Result<(), MysqlError> {
    let mut conn = Conn::new(config.native_options()).await?;
    let selected = conn
        .query_drop(format!("DROP DATABASE IF EXISTS `{selected_database}`"))
        .await;
    let default = conn
        .query_drop(format!("DROP DATABASE IF EXISTS `{default_database}`"))
        .await;
    let disconnect = conn.disconnect().await;
    selected?;
    default?;
    disconnect
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

fn mysql_test_required() -> bool {
    std::env::var("MYSQL_TEST_REQUIRED").is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be configured"))
}
