use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    ComponentState, CreateDatasourceRequest, DatabaseWriteState, DatasourceConnection,
    DatasourceConnectionProperty, ExecuteDatabaseWriteRequest, JdbcValue, OperationStatus,
    QueryLimits, ResultPageRequest, StartQueryRequest,
};
use chat2db_core::{Application, RuntimeConfig, RuntimeHost};
use chat2db_java_bridge::{EngineCommand, EngineConfig};
use chat2db_local::{LocalClient, LocalServer};
use tempfile::TempDir;
use uuid::Uuid;

const REQUIRED_MYSQL_ENV: [&str; 4] = [
    "MYSQL_TEST_HOST",
    "MYSQL_TEST_PORT",
    "MYSQL_TEST_USER",
    "MYSQL_TEST_PASSWORD",
];
const QUERY_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
struct MysqlTestConfig {
    host: String,
    port: u16,
    user: String,
    password: String,
}

struct AutomationDatasources {
    writable: String,
    read_only: String,
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
            eprintln!("skipping local automation MySQL test; MYSQL_TEST_* variables are absent");
            return None;
        }
        assert_eq!(
            configured,
            REQUIRED_MYSQL_ENV.len(),
            "local automation MySQL integration is partially configured"
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

    fn connection(&self, database_name: &str, read_only: bool) -> DatasourceConnection {
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
            read_only,
            ssh: None,
        }
    }
}

#[tokio::test]
async fn local_automation_writes_real_mysql_and_keeps_java_dormant() {
    let Some(config) = MysqlTestConfig::from_environment() else {
        return;
    };
    let database_name = format!("chat2db_local_it_{}", Uuid::new_v4().simple());
    let directory = TempDir::new().expect("temporary local automation runtime");
    let runtime = RuntimeConfig::new(EngineConfig::new(EngineCommand::new(
        directory.path().join("missing-java"),
    )))
    .with_data_dir(directory.path().join("data"))
    .with_vault_master_key_base64(STANDARD.encode([0x4c; 32]));
    let mut host = RuntimeHost::open(runtime)
        .await
        .expect("local automation runtime must open without Java");
    let application = host.application();
    assert_java_dormant(&application);
    let admin_datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Local automation MySQL administrator".to_owned(),
            driver_id: "mysql".to_owned(),
            connection: Some(config.connection("mysql", false)),
        })
        .await
        .expect("native MySQL administrator datasource must persist");
    let data_dir = application
        .storage()
        .expect("runtime storage must be configured")
        .data_dir()
        .to_path_buf();
    let mut server = LocalServer::start(application.clone()).expect("local server must start");
    let client = LocalClient::new(data_dir);
    assert_java_dormant(&application);

    let verification = tokio::spawn(verify_local_automation(
        config,
        database_name.clone(),
        application.clone(),
        client.clone(),
        admin_datasource.id.clone(),
    ))
    .await;
    let cleanup = client
        .execute_database_write(ExecuteDatabaseWriteRequest {
            datasource_id: admin_datasource.id,
            sql: format!("DROP DATABASE IF EXISTS `{database_name}`"),
            confirmed: true,
        })
        .await;
    if cleanup.state != DatabaseWriteState::Succeeded {
        eprintln!("local automation MySQL cleanup failed: {cleanup:?}");
    }
    assert_java_dormant(&application);
    server.shutdown().await.expect("local server must stop");
    assert_java_dormant(&application);
    host.shutdown()
        .await
        .expect("native-only runtime must shut down cleanly");

    match verification {
        Ok(()) => assert_eq!(cleanup.state, DatabaseWriteState::Succeeded),
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(error) => panic!("local automation verification task failed: {error}"),
    }
}

async fn verify_local_automation(
    config: MysqlTestConfig,
    database_name: String,
    application: Application,
    client: LocalClient,
    admin_datasource_id: String,
) {
    let version = read_text(&client, &admin_datasource_id, "SELECT VERSION()").await;
    assert!(
        version.starts_with("8.4."),
        "local automation acceptance requires MySQL 8.4, found {version}"
    );
    let datasources = provision_fixture(
        &config,
        &database_name,
        &application,
        &client,
        admin_datasource_id,
    )
    .await;
    let update_sql = "UPDATE `automation_write_probe` SET `label` = 'written-through-local-server' WHERE `id` = 1";

    let unconfirmed = client
        .execute_database_write(ExecuteDatabaseWriteRequest {
            datasource_id: datasources.writable.clone(),
            sql: update_sql.to_owned(),
            confirmed: false,
        })
        .await;
    assert_eq!(unconfirmed.state, DatabaseWriteState::NotStarted);
    assert_eq!(
        unconfirmed.error.as_ref().map(|error| error.code.as_str()),
        Some("database_write_confirmation_required")
    );
    assert_eq!(
        read_text(
            &client,
            &datasources.writable,
            "SELECT `label` FROM `automation_write_probe` WHERE `id` = 1",
        )
        .await,
        "initial"
    );
    assert_java_dormant(&application);

    let read_only = client
        .execute_database_write(ExecuteDatabaseWriteRequest {
            datasource_id: datasources.read_only,
            sql: update_sql.to_owned(),
            confirmed: true,
        })
        .await;
    assert_eq!(read_only.state, DatabaseWriteState::NotStarted);
    assert_eq!(
        read_only.error.as_ref().map(|error| error.code.as_str()),
        Some("datasource_read_only")
    );
    assert_eq!(
        read_text(
            &client,
            &datasources.writable,
            "SELECT `label` FROM `automation_write_probe` WHERE `id` = 1",
        )
        .await,
        "initial"
    );
    assert_java_dormant(&application);

    let written = client
        .execute_database_write(ExecuteDatabaseWriteRequest {
            datasource_id: datasources.writable.clone(),
            sql: update_sql.to_owned(),
            confirmed: true,
        })
        .await;
    assert_eq!(written.state, DatabaseWriteState::Succeeded);
    assert_eq!(written.affected_rows.as_deref(), Some("1"));
    assert!(written.error.is_none());
    assert_eq!(
        read_text(
            &client,
            &datasources.writable,
            "SELECT `label` FROM `automation_write_probe` WHERE `id` = 1",
        )
        .await,
        "written-through-local-server"
    );
    assert_java_dormant(&application);
}

async fn provision_fixture(
    config: &MysqlTestConfig,
    database_name: &str,
    application: &Application,
    client: &LocalClient,
    admin_datasource_id: String,
) -> AutomationDatasources {
    let created_database = client
        .execute_database_write(ExecuteDatabaseWriteRequest {
            datasource_id: admin_datasource_id,
            sql: format!(
                "CREATE DATABASE `{database_name}` CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci"
            ),
            confirmed: true,
        })
        .await;
    assert_write_succeeded(&created_database, "fixture database creation");
    assert_java_dormant(application);

    let writable = application
        .create_datasource(CreateDatasourceRequest {
            name: "Local automation MySQL".to_owned(),
            driver_id: "mysql".to_owned(),
            connection: Some(config.connection(database_name, false)),
        })
        .await
        .expect("writable native MySQL datasource must persist");
    let read_only = application
        .create_datasource(CreateDatasourceRequest {
            name: "Local automation read-only MySQL".to_owned(),
            driver_id: "mysql".to_owned(),
            connection: Some(config.connection(database_name, true)),
        })
        .await
        .expect("read-only native MySQL datasource must persist");
    let created_table = client
        .execute_database_write(ExecuteDatabaseWriteRequest {
            datasource_id: writable.id.clone(),
            sql: "CREATE TABLE `automation_write_probe` (`id` BIGINT NOT NULL, `label` VARCHAR(128) NOT NULL, PRIMARY KEY (`id`)) ENGINE=InnoDB".to_owned(),
            confirmed: true,
        })
        .await;
    assert_write_succeeded(&created_table, "fixture table creation");
    let inserted_row = client
        .execute_database_write(ExecuteDatabaseWriteRequest {
            datasource_id: writable.id.clone(),
            sql: "INSERT INTO `automation_write_probe` VALUES (1, 'initial')".to_owned(),
            confirmed: true,
        })
        .await;
    assert_write_succeeded(&inserted_row, "fixture row insertion");
    AutomationDatasources {
        writable: writable.id,
        read_only: read_only.id,
    }
}

async fn read_text(client: &LocalClient, datasource_id: &str, sql: &str) -> String {
    let accepted = client
        .start_read_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: sql.to_owned(),
            parameters: Vec::new(),
            limits: QueryLimits {
                max_rows: "10".to_owned(),
                max_result_bytes: "1048576".to_owned(),
                batch_rows: 10,
                batch_bytes: 65_536,
                result_ttl_seconds: 60,
            },
        })
        .await
        .expect("local read query must be accepted");
    let snapshot = tokio::time::timeout(QUERY_TIMEOUT, async {
        loop {
            let snapshot = client
                .operation_snapshot(&accepted.operation_id)
                .await
                .expect("local read query snapshot must remain available");
            match snapshot.status {
                OperationStatus::Running => tokio::time::sleep(Duration::from_millis(20)).await,
                OperationStatus::Completed => break snapshot,
                OperationStatus::Failed | OperationStatus::Cancelled => {
                    panic!("local read query ended unexpectedly: {snapshot:?}")
                }
            }
        }
    })
    .await
    .expect("local read query must complete before timeout");
    let result = snapshot
        .result
        .expect("completed local read query must retain a result");
    let page = client
        .result_page(
            result.id,
            ResultPageRequest {
                offset: "0".to_owned(),
                max_rows: "10".to_owned(),
                max_bytes: "262144".to_owned(),
            },
        )
        .await
        .expect("local read result must be pageable");
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].values.len(), 1);
    let JdbcValue::Text { value } = &page.rows[0].values[0] else {
        panic!("local read result must contain text: {:?}", page.rows[0]);
    };
    value.clone()
}

fn assert_write_succeeded(result: &chat2db_contract::DatabaseWriteResult, operation: &str) {
    assert_eq!(
        result.state,
        DatabaseWriteState::Succeeded,
        "{operation} must succeed: {result:?}"
    );
    assert!(result.error.is_none(), "{operation} returned an error");
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
