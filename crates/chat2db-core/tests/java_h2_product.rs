use std::{path::PathBuf, sync::Arc, time::Duration};

use chat2db_contract::{
    CancelDisposition, CreateDatasourceRequest, DatasourceConnection, JdbcValue, OperationEvent,
    OperationStatus, QueryLimits, ResultPageRequest, StartQueryRequest,
};
use chat2db_core::{Application, RuntimeHost};
use chat2db_java_bridge::{
    DriverArtifact, DriverClient, DriverSpec, EngineCommand, EngineConfig, EngineSupervisor,
};
use chat2db_storage::{EncryptedFileVault, Storage, StorageOptions};
use tempfile::TempDir;

const H2_DRIVER_CLASS: &str = "org.h2.Driver";
const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

struct H2ProductHarness {
    _directory: TempDir,
    host: RuntimeHost,
    application: Application,
    driver: DriverClient,
    driver_id: String,
}

impl H2ProductHarness {
    async fn start() -> Self {
        Self::start_with_storage_options(StorageOptions::default()).await
    }

    async fn start_with_storage_options(storage_options: StorageOptions) -> Self {
        let engine_jar = required_jar("CHAT2DB_JAVA_ENGINE_JAR");
        let h2_jar = required_jar("CHAT2DB_H2_DRIVER_JAR");
        let directory = TempDir::new().expect("temporary data directory");
        let vault = Arc::new(
            EncryptedFileVault::new(directory.path(), [0x5a; 32])
                .expect("encrypted test vault must open"),
        );
        let storage = Storage::open_with_options(directory.path(), storage_options, vault)
            .expect("product storage must open");
        let supervisor = EngineSupervisor::spawn(
            EngineConfig::new(EngineCommand::java_jar("java", engine_jar)).with_timeouts(
                Duration::from_secs(10),
                Duration::from_secs(10),
                Duration::from_secs(5),
            ),
        )
        .await
        .expect("Java engine must handshake");
        let driver = supervisor
            .client()
            .driver_client()
            .expect("ready engine must expose JDBC");
        let loaded = driver
            .load_driver(DriverSpec {
                driver_class: H2_DRIVER_CLASS.to_owned(),
                artifacts: vec![
                    DriverArtifact::from_path(h2_jar).expect("H2 driver artifact must be valid"),
                ],
            })
            .await
            .expect("H2 driver must load");
        let driver_id = loaded.driver_id;
        let host = RuntimeHost::from_supervisor(storage, supervisor);
        let application = host.application();
        Self {
            _directory: directory,
            host,
            application,
            driver,
            driver_id,
        }
    }

    async fn finish(mut self) {
        self.driver
            .unload_driver(self.driver_id)
            .await
            .expect("closed product query must release the H2 driver");
        self.host
            .shutdown()
            .await
            .expect("runtime host must shut down cleanly");
    }
}

#[tokio::test]
async fn jdbc_stream_is_retained_and_read_through_product_services() {
    let harness = H2ProductHarness::start().await;
    let application = &harness.application;

    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Stage 5 H2".to_owned(),
            driver_id: harness.driver_id.clone(),
            connection: Some(DatasourceConnection {
                jdbc_url: "jdbc:h2:mem:stage5_product;DB_CLOSE_DELAY=-1;DATABASE_TO_UPPER=TRUE"
                    .to_owned(),
                properties: Vec::new(),
                read_only: true,
            }),
        })
        .await
        .expect("datasource must be created through the product service");
    assert!(datasource.has_secret);

    let accepted = application
        .start_query(StartQueryRequest {
            datasource_id: datasource.id,
            sql: "SELECT X AS id, CAST('row-' || X AS VARCHAR(16)) AS label, \
                  MOD(X, 2) = 0 AS active, \
                  CAST(X + 0.25 AS DECIMAL(10, 2)) AS amount \
                  FROM SYSTEM_RANGE(1, 5) ORDER BY X"
                .to_owned(),
            parameters: Vec::new(),
            limits: QueryLimits {
                max_rows: "0".to_owned(),
                max_result_bytes: (1024_u64 * 1024).to_string(),
                batch_rows: 2,
                batch_bytes: 1024,
                result_ttl_seconds: 60,
            },
        })
        .await
        .expect("query must be accepted");
    let result_id = wait_for_result(application, &accepted.operation_id).await;
    let snapshot = application
        .operation_snapshot(&accepted.operation_id)
        .await
        .expect("completed snapshot must exist");
    assert_eq!(snapshot.status, OperationStatus::Completed);
    assert_eq!(snapshot.row_count, "5");

    assert_result_page(application, &result_id).await;
    harness.finish().await;
}

#[tokio::test]
async fn active_jdbc_query_is_explicitly_cancelled_through_product_services() {
    let harness = H2ProductHarness::start().await;
    let application = &harness.application;
    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Stage 5 cancellation".to_owned(),
            driver_id: harness.driver_id.clone(),
            connection: Some(DatasourceConnection {
                jdbc_url: "jdbc:h2:mem:stage5_cancel;DB_CLOSE_DELAY=-1".to_owned(),
                properties: Vec::new(),
                read_only: true,
            }),
        })
        .await
        .expect("cancellation datasource must be created");
    let accepted = application
        .start_query(StartQueryRequest {
            datasource_id: datasource.id,
            sql: "SELECT X FROM SYSTEM_RANGE(1, 1000000)".to_owned(),
            parameters: Vec::new(),
            limits: QueryLimits {
                max_rows: "0".to_owned(),
                max_result_bytes: (16_u64 * 1024 * 1024).to_string(),
                batch_rows: 1,
                batch_bytes: 1024,
                result_ttl_seconds: 60,
            },
        })
        .await
        .expect("long query must be accepted");
    let mut events = application
        .subscribe_operation(&accepted.operation_id, Some(0))
        .await
        .expect("cancellation subscription must open");
    wait_until_started(&mut events).await;

    let cancellation = application.cancel_operation(&accepted.operation_id).await;
    assert_eq!(cancellation.disposition, CancelDisposition::Accepted);
    wait_until_cancelled(&mut events).await;
    let snapshot = application
        .operation_snapshot(&accepted.operation_id)
        .await
        .expect("cancelled snapshot must exist");
    assert_eq!(snapshot.status, OperationStatus::Cancelled);
    assert!(snapshot.result.is_none());

    harness.finish().await;
}

#[tokio::test]
async fn local_result_failure_settles_query_before_releasing_session_and_driver() {
    let harness = H2ProductHarness::start_with_storage_options(StorageOptions {
        max_retained_bytes: 2 * 1024,
    })
    .await;
    let application = &harness.application;
    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Stage 5 local failure cleanup".to_owned(),
            driver_id: harness.driver_id.clone(),
            connection: Some(DatasourceConnection {
                jdbc_url: "jdbc:h2:mem:stage5_cleanup;DB_CLOSE_DELAY=-1".to_owned(),
                properties: Vec::new(),
                read_only: true,
            }),
        })
        .await
        .expect("cleanup datasource must be created");
    let accepted = application
        .start_query(StartQueryRequest {
            datasource_id: datasource.id,
            sql: "SELECT X, CAST(REPEAT('x', 8192) AS VARCHAR(8192)) AS payload \
                  FROM SYSTEM_RANGE(1, 2) ORDER BY X"
                .to_owned(),
            parameters: Vec::new(),
            limits: QueryLimits {
                max_rows: "0".to_owned(),
                max_result_bytes: (1024_u64 * 1024).to_string(),
                batch_rows: 1,
                batch_bytes: 16 * 1024,
                result_ttl_seconds: 60,
            },
        })
        .await
        .expect("query must be accepted before local persistence fails");
    let mut events = application
        .subscribe_operation(&accepted.operation_id, Some(0))
        .await
        .expect("failure subscription must open");

    loop {
        match next_operation_event(&mut events).await {
            OperationEvent::Started | OperationEvent::Progress { .. } => {}
            OperationEvent::Failed { error } => {
                assert_eq!(error.code, "result_storage_quota_exceeded");
                break;
            }
            OperationEvent::Completed { .. } => panic!("quota-limited query completed"),
            OperationEvent::Cancelled { reason } => {
                panic!("local failure surfaced as cancellation: {reason:?}")
            }
        }
    }

    harness.finish().await;
}

async fn wait_for_result(application: &Application, operation_id: &str) -> String {
    let mut events = application
        .subscribe_operation(operation_id, Some(0))
        .await
        .expect("operation subscription must open");
    let mut saw_started = false;
    let mut progress_rows = Vec::new();
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, events.next_event())
            .await
            .expect("operation event must arrive")
            .expect("operation event stream must remain valid")
            .expect("operation must emit a terminal event");
        match event.event {
            OperationEvent::Started => saw_started = true,
            OperationEvent::Progress { row_count, .. } => progress_rows.push(row_count),
            OperationEvent::Completed { result } => {
                assert!(saw_started);
                assert_eq!(progress_rows, ["2", "4", "5"]);
                return result.id;
            }
            OperationEvent::Failed { error } => panic!("query failed: {error:?}"),
            OperationEvent::Cancelled { reason } => panic!("query was cancelled: {reason:?}"),
        }
    }
}

async fn wait_until_started(subscription: &mut chat2db_core::OperationSubscription) {
    loop {
        let event = next_operation_event(subscription).await;
        if matches!(&event, OperationEvent::Started) {
            return;
        }
        assert!(
            !matches!(
                &event,
                OperationEvent::Completed { .. }
                    | OperationEvent::Failed { .. }
                    | OperationEvent::Cancelled { .. }
            ),
            "query became terminal before cancellation"
        );
    }
}

async fn wait_until_cancelled(subscription: &mut chat2db_core::OperationSubscription) {
    loop {
        match next_operation_event(subscription).await {
            OperationEvent::Started | OperationEvent::Progress { .. } => {}
            OperationEvent::Cancelled { .. } => return,
            OperationEvent::Completed { .. } => panic!("cancelled query completed"),
            OperationEvent::Failed { error } => panic!("cancelled query failed: {error:?}"),
        }
    }
}

async fn next_operation_event(
    subscription: &mut chat2db_core::OperationSubscription,
) -> OperationEvent {
    tokio::time::timeout(EVENT_TIMEOUT, subscription.next_event())
        .await
        .expect("operation event must arrive")
        .expect("operation event stream must remain valid")
        .expect("operation must not close without a terminal event")
        .event
}

async fn assert_result_page(application: &Application, result_id: &str) {
    let page = application
        .result_page(
            result_id,
            ResultPageRequest {
                offset: "0".to_owned(),
                max_rows: "10".to_owned(),
                max_bytes: (8_u64 * 1024 * 1024).to_string(),
            },
        )
        .await
        .expect("retained result page must be readable");
    assert_eq!(page.metadata.row_count, "5");
    assert_eq!(page.rows.len(), 5);
    assert!(!page.has_more);
    assert!(matches!(
        page.rows[0].values.as_slice(),
        [
            JdbcValue::SignedInteger { value: id },
            JdbcValue::Text { value: label },
            JdbcValue::Boolean { value: false },
            JdbcValue::Decimal { value: amount },
        ] if id == "1" && label == "row-1" && amount == "1.25"
    ));
}

fn required_jar(variable: &str) -> PathBuf {
    let path = std::env::var_os(variable).map_or_else(
        || panic!("{variable} must point to a packaged JAR"),
        PathBuf::from,
    );
    assert!(path.is_file(), "{variable} does not point to a file");
    path
}
