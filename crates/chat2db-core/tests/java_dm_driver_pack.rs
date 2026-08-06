use std::{path::PathBuf, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    CreateDatasourceRequest, DatasourceConnection, DatasourceConnectionProperty, JdbcDriver,
    JdbcValue, OperationEvent, QueryLimits, ResultPageRequest, StartQueryRequest,
};
use chat2db_core::{Application, RuntimeConfig, RuntimeHost};
use chat2db_java_bridge::{EngineCommand, EngineConfig};
use tempfile::TempDir;

const DM_DRIVER_CLASS: &str = "dm.jdbc.driver.DmDriver";
const DM_DRIVER_VERSION: &str = "8.1.2.141";
const EVENT_TIMEOUT: Duration = Duration::from_secs(20);
const REQUIRED_LIVE_ENV: [&str; 4] = [
    "DM_TEST_HOST",
    "DM_TEST_PORT",
    "DM_TEST_USER",
    "DM_TEST_PASSWORD",
];

struct DmHarness {
    _directory: TempDir,
    host: RuntimeHost,
    application: Application,
    driver: JdbcDriver,
}

struct LiveDmConfig {
    jdbc_url: String,
    user: String,
    password: String,
}

impl DmHarness {
    async fn start() -> Self {
        let engine_jar = required_file("CHAT2DB_JAVA_ENGINE_JAR");
        let driver_pack_dir = required_directory("DM_TEST_DRIVER_PACK_DIR");
        let directory = TempDir::new().expect("temporary DM product directory");
        let engine = EngineConfig::new(EngineCommand::java_jar("java", engine_jar)).with_timeouts(
            Duration::from_secs(15),
            Duration::from_secs(15),
            Duration::from_secs(5),
        );
        let runtime = RuntimeConfig::new(engine)
            .with_data_dir(directory.path().join("data"))
            .with_driver_pack_dir(driver_pack_dir)
            .with_vault_master_key_base64(STANDARD.encode([0x44; 32]));
        let host = RuntimeHost::open(runtime)
            .await
            .expect("runtime must discover the managed DM driver pack");
        let application = host.application();
        let mut matches = application
            .list_drivers()
            .items
            .into_iter()
            .filter(|driver| driver.pack_id == "dm");
        let driver = matches
            .next()
            .expect("managed DM driver must be present in the inventory");
        assert!(matches.next().is_none(), "DM driver pack must be unique");
        assert_eq!(driver.name, "DM");
        assert_eq!(driver.version, DM_DRIVER_VERSION);
        assert_eq!(driver.driver_class, DM_DRIVER_CLASS);
        assert_eq!(driver.artifact_count, 1);
        assert_eq!(driver.artifact_bytes, "1030636");
        assert!(driver.driver_id.starts_with("sha256:"));

        let lease = host
            .acquire_engine()
            .await
            .expect("Java 17 must load the managed DM driver class");
        drop(lease);

        Self {
            _directory: directory,
            host,
            application,
            driver,
        }
    }

    async fn finish(mut self) {
        self.host
            .shutdown()
            .await
            .expect("DM driver runtime must shut down cleanly");
    }
}

impl LiveDmConfig {
    fn from_environment() -> Option<Self> {
        let configured = REQUIRED_LIVE_ENV
            .iter()
            .filter(|name| std::env::var_os(name).is_some())
            .count();
        let required = std::env::var("DM_TEST_REQUIRED").is_ok_and(|value| value == "1");
        if configured == 0 {
            assert!(
                !required,
                "DM_TEST_REQUIRED is enabled but the live DM endpoint variables are absent"
            );
            eprintln!("skipping live DM connection/query; DM_TEST_* endpoint variables are absent");
            return None;
        }
        assert_eq!(
            configured,
            REQUIRED_LIVE_ENV.len(),
            "live DM integration is partially configured; set every required DM_TEST_* variable"
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
        let jdbc_url = std::env::var("DM_TEST_JDBC_URL")
            .unwrap_or_else(|_| format!("jdbc:dm://{host}:{port}"));
        assert!(
            jdbc_url.starts_with("jdbc:dm://") && !jdbc_url.chars().any(char::is_control),
            "DM_TEST_JDBC_URL must be a valid DM JDBC URL"
        );

        Some(Self {
            jdbc_url,
            user: required_text("DM_TEST_USER"),
            password: required_text("DM_TEST_PASSWORD"),
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
            read_only: true,
            ssh: None,
        }
    }
}

#[tokio::test]
async fn managed_dm_pack_is_discovered_and_loaded_without_community_metadata() {
    DmHarness::start().await.finish().await;
}

#[tokio::test]
async fn managed_dm_pack_connects_and_streams_a_query_when_endpoint_is_configured() {
    let Some(config) = LiveDmConfig::from_environment() else {
        return;
    };
    let harness = DmHarness::start().await;
    harness
        .application
        .test_datasource_connection(&harness.driver.driver_id, config.connection())
        .await
        .expect("managed DM driver must open and close a real JDBC connection");
    let datasource = harness
        .application
        .create_datasource(CreateDatasourceRequest {
            name: "DM JDBC integration".to_owned(),
            driver_id: harness.driver.driver_id.clone(),
            connection: Some(config.connection()),
        })
        .await
        .expect("DM datasource must be persisted through the generic datasource contract");
    let accepted = harness
        .application
        .start_query(StartQueryRequest {
            datasource_id: datasource.id,
            sql: "SELECT CAST(1 AS BIGINT) AS ID, 'dm-ok' AS LABEL FROM DUAL".to_owned(),
            parameters: Vec::new(),
            limits: QueryLimits {
                max_rows: "10".to_owned(),
                max_result_bytes: (1024_u64 * 1024).to_string(),
                batch_rows: 16,
                batch_bytes: 16 * 1024,
                result_ttl_seconds: 60,
            },
        })
        .await
        .expect("DM query must be accepted through the generic JDBC product path");
    let result_id = wait_for_result(&harness.application, &accepted.operation_id).await;
    let page = harness
        .application
        .result_page(
            &result_id,
            ResultPageRequest {
                offset: "0".to_owned(),
                max_rows: "10".to_owned(),
                max_bytes: (1024_u64 * 1024).to_string(),
            },
        )
        .await
        .expect("DM retained result must be readable");
    assert_eq!(page.rows.len(), 1);
    assert!(matches!(
        page.rows[0].values.as_slice(),
        [JdbcValue::SignedInteger { value: id }, JdbcValue::Text { value: label }]
            if id == "1" && label == "dm-ok"
    ));
    harness.finish().await;
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
            OperationEvent::Failed { error } => panic!("DM query failed: {error:?}"),
            OperationEvent::Cancelled { reason } => panic!("DM query was cancelled: {reason:?}"),
        }
    }
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
