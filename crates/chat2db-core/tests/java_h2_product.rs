use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    CancelDisposition, ComponentState, CreateDatasourceRequest, DatasourceConnection, JdbcDriver,
    JdbcValue, ListCommunityColumnsRequest, ListCommunityDatabasesRequest,
    ListCommunityIndexesRequest, ListCommunityTablesRequest, OperationEvent, OperationStatus,
    QueryLimits, ResultPageRequest, StartQueryRequest,
};
use chat2db_core::{Application, RuntimeConfig, RuntimeHost};
use chat2db_java_bridge::{
    DriverArtifact, DriverClient, DriverSpec, EngineCommand, EngineConfig, EngineSupervisor,
};
use chat2db_storage::{EncryptedFileVault, Storage, StorageOptions};
use futures_util::future::join_all;
use tempfile::TempDir;

const H2_DRIVER_CLASS: &str = "org.h2.Driver";
const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

fn assert_native_mysql_driver(drivers: &[JdbcDriver]) {
    assert!(
        drivers
            .iter()
            .any(|driver| driver.pack_id == "native:mysql_async"),
        "the native MySQL driver must be discoverable without starting Java"
    );
}

fn managed_driver<'a>(drivers: &'a [JdbcDriver], pack_id: &str) -> &'a JdbcDriver {
    drivers
        .iter()
        .find(|driver| driver.pack_id == pack_id)
        .unwrap_or_else(|| {
            panic!("managed {pack_id} driver must be discovered beside native MySQL")
        })
}

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
        assert_community_disabled(&application).await;
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

async fn assert_community_disabled(application: &Application) {
    let community = application
        .health()
        .components
        .into_iter()
        .find(|component| component.id == "community-compatibility")
        .expect("Community compatibility health must be explicit");
    assert_eq!(community.state, ComponentState::Disabled);
    let community_error = application
        .list_community_plugins()
        .await
        .expect_err("unconfigured Community services must stay disabled");
    assert_eq!(
        community_error.api_error().code,
        "community_compatibility_disabled"
    );
    let disabled_database_error = application
        .list_community_databases(ListCommunityDatabasesRequest {
            datasource_id: "disabled".to_owned(),
            database_type: "H2".to_owned(),
        })
        .await
        .expect_err("unknown datasource must fail before Community engine acquisition");
    assert_eq!(
        disabled_database_error.api_error().code,
        "datasource_not_found"
    );
    let disabled_table_error = application
        .list_community_tables(ListCommunityTablesRequest {
            datasource_id: "disabled".to_owned(),
            database_type: "H2".to_owned(),
            database_name: String::new(),
            schema_name: String::new(),
            table_name_pattern: "%".to_owned(),
        })
        .await
        .expect_err("unknown datasource must fail before Community engine acquisition");
    assert_eq!(
        disabled_table_error.api_error().code,
        "datasource_not_found"
    );
    let disabled_column_error = application
        .list_community_columns(ListCommunityColumnsRequest {
            datasource_id: "disabled".to_owned(),
            database_type: "H2".to_owned(),
            database_name: String::new(),
            schema_name: String::new(),
            table_name: "items".to_owned(),
        })
        .await
        .expect_err("unknown datasource must fail before Community engine acquisition");
    assert_eq!(
        disabled_column_error.api_error().code,
        "datasource_not_found"
    );
    let disabled_index_error = application
        .list_community_indexes(ListCommunityIndexesRequest {
            datasource_id: "disabled".to_owned(),
            database_type: "H2".to_owned(),
            database_name: String::new(),
            schema_name: String::new(),
            table_name: "items".to_owned(),
        })
        .await
        .expect_err("unknown datasource must fail before Community engine acquisition");
    assert_eq!(
        disabled_index_error.api_error().code,
        "datasource_not_found"
    );
}

#[tokio::test]
async fn runtime_host_open_keeps_java_dormant() {
    let directory = TempDir::new().expect("temporary runtime directory");
    let missing_java = directory.path().join("missing-java");
    let engine = EngineConfig::new(EngineCommand::new(&missing_java));
    let config = RuntimeConfig::new(engine)
        .with_data_dir(directory.path().join("data"))
        .with_vault_master_key_base64(STANDARD.encode([0x5a; 32]));

    let mut host = RuntimeHost::open(config)
        .await
        .expect("opening storage must not spawn the missing Java executable");
    assert_engine_available_on_demand(&host.application());
    let inventory = host.application().list_drivers();
    assert_eq!(inventory.items.len(), 4);
    assert_native_mysql_driver(&inventory.items);
    host.shutdown()
        .await
        .expect("a dormant runtime must shut down cleanly");
}

#[tokio::test]
async fn managed_h2_starts_on_demand_and_reloads_after_idle_shutdown() {
    let engine_jar = required_jar("CHAT2DB_JAVA_ENGINE_JAR");
    let h2_jar = required_jar("CHAT2DB_H2_DRIVER_JAR");
    let directory = TempDir::new().expect("temporary runtime directory");
    let driver_pack_root = directory.path().join("driver-packs");
    let pack_directory = driver_pack_root.join("01-h2");
    write_driver_pack(&driver_pack_root, "01-h2", "h2", H2_DRIVER_CLASS, &h2_jar);
    let managed_h2_jar = pack_directory.join("driver.jar");

    let engine = EngineConfig::new(EngineCommand::java_jar("java", engine_jar)).with_timeouts(
        Duration::from_secs(10),
        Duration::from_secs(10),
        Duration::from_secs(5),
    );
    let data_dir = directory.path().join("data");
    let driver_runtime_directory = data_dir.join("jdbc-driver-runtime");
    let config = RuntimeConfig::new(engine)
        .with_data_dir(&data_dir)
        .with_driver_pack_dir(&driver_pack_root)
        .with_vault_master_key_base64(STANDARD.encode([0x5a; 32]))
        .with_engine_idle_timeout(Duration::from_millis(100));
    let mut host = RuntimeHost::open(config)
        .await
        .expect("runtime host must discover the H2 pack without starting Java");
    let application = host.application();
    assert_engine_available_on_demand(&application);
    let inventory = application.list_drivers();
    assert_eq!(inventory.items.len(), 5);
    assert_native_mysql_driver(&inventory.items);
    let installed = managed_driver(&inventory.items, "h2");
    assert_eq!(installed.pack_id, "h2");
    assert_eq!(installed.version, "test");
    assert_eq!(installed.driver_class, H2_DRIVER_CLASS);
    assert_eq!(installed.artifact_count, 1);
    assert_eq!(
        installed.artifact_bytes,
        fs::metadata(&managed_h2_jar)
            .expect("managed H2 metadata")
            .len()
            .to_string()
    );

    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Stage 7 managed H2".to_owned(),
            driver_id: installed.driver_id.clone(),
            connection: Some(DatasourceConnection {
                jdbc_url: "jdbc:h2:mem:stage7_managed;DB_CLOSE_DELAY=-1;DATABASE_TO_UPPER=TRUE"
                    .to_owned(),
                properties: Vec::new(),
                read_only: true,
                ssh: None,
            }),
        })
        .await
        .expect("managed-driver datasource must be created");
    let mut first_leases = join_all((0..16).map(|_| application.acquire_engine()))
        .await
        .into_iter()
        .map(|result| result.expect("concurrent first use must share one successful startup"))
        .collect::<Vec<_>>();
    let first_lease = first_leases
        .pop()
        .expect("at least one first-use lease must exist");
    let first_generation = first_lease.generation();
    assert!(
        first_leases
            .iter()
            .all(|lease| lease.generation() == first_generation),
        "concurrent first use must return one Java generation"
    );
    drop(first_leases);
    let accepted = application
        .start_query(h2_range_query(datasource.id.clone()))
        .await
        .expect("managed H2 query must be accepted");
    let result_id = wait_for_result(&application, &accepted.operation_id).await;
    assert_result_page(&application, &result_id).await;
    let escaped_client = first_lease.client().clone();
    drop(first_lease);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_engine_running(&application);
    drop(escaped_client);

    wait_for_engine_idle(&application).await;
    let second_lease = host
        .acquire_engine()
        .await
        .expect("database use after idle shutdown must start a new Java generation");
    assert_ne!(
        second_lease.generation(),
        first_generation,
        "idle restart must not reuse a stale engine generation"
    );
    let accepted = application
        .start_query(h2_range_query(datasource.id))
        .await
        .expect("the reloaded H2 driver must execute a second query");
    let result_id = wait_for_result(&application, &accepted.operation_id).await;
    assert_result_page(&application, &result_id).await;
    drop(second_lease);

    host.shutdown()
        .await
        .expect("runtime host must shut down with the managed driver");
    assert_directory_empty(&driver_runtime_directory);
}

#[tokio::test]
async fn managed_h2_parks_and_resumes_before_hard_idle_reap() {
    let engine_jar = required_jar("CHAT2DB_JAVA_ENGINE_JAR");
    let h2_jar = required_jar("CHAT2DB_H2_DRIVER_JAR");
    let directory = TempDir::new().expect("temporary runtime directory");
    let driver_pack_root = directory.path().join("driver-packs");
    write_driver_pack(&driver_pack_root, "01-h2", "h2", H2_DRIVER_CLASS, &h2_jar);

    let engine = EngineConfig::new(EngineCommand::java_jar("java", engine_jar)).with_timeouts(
        Duration::from_secs(10),
        Duration::from_secs(10),
        Duration::from_secs(5),
    );
    let config = RuntimeConfig::new(engine)
        .with_data_dir(directory.path().join("data"))
        .with_driver_pack_dir(&driver_pack_root)
        .with_vault_master_key_base64(STANDARD.encode([0x5a; 32]))
        .with_engine_idle_timeout(Duration::from_secs(2));
    let mut host = RuntimeHost::open(config)
        .await
        .expect("managed runtime opens without starting Java");

    let first = host
        .acquire_engine()
        .await
        .expect("preloaded Java generation starts");
    let first_generation = first.generation();
    drop(first);
    wait_for_engine_parked(&host.application()).await;

    let resumed = host
        .acquire_engine()
        .await
        .expect("parked Java generation resumes");
    assert_eq!(
        resumed.generation(),
        first_generation,
        "cooperative resume must preserve the Java generation"
    );
    drop(resumed);

    wait_for_engine_idle(&host.application()).await;
    let restarted = host
        .acquire_engine()
        .await
        .expect("hard-idle reap permits a fresh Java generation");
    assert_ne!(
        restarted.generation(),
        first_generation,
        "hard idle must fully reap rather than revive the old generation"
    );
    drop(restarted);
    host.shutdown().await.expect("managed runtime shuts down");
}

#[tokio::test]
async fn imports_legacy_community_mysql_from_a_read_only_h2_snapshot() {
    let engine_jar = required_jar("CHAT2DB_JAVA_ENGINE_JAR");
    let h2_jar = required_jar("CHAT2DB_H2_DRIVER_JAR");
    let directory = TempDir::new().expect("temporary migration directory");
    let legacy_base = directory.path().join("legacy/chat2db");
    fs::create_dir_all(legacy_base.parent().expect("legacy parent"))
        .expect("legacy directory creates");
    let legacy_url = format!("jdbc:h2:file:{};MODE=MYSQL", legacy_base.to_string_lossy());
    let sql = "CREATE TABLE DATA_SOURCE (\
        ID BIGINT PRIMARY KEY, ALIAS VARCHAR, TYPE VARCHAR, URL VARCHAR, \
        USER_NAME VARCHAR, \"PASSWORD\" VARCHAR, SSH VARCHAR, SSL VARCHAR, \
        DRIVER_CONFIG VARCHAR, EXTEND_INFO VARCHAR, HOST VARCHAR, PORT VARCHAR, \
        JDBC VARCHAR, SERVICE_NAME VARCHAR); \
        INSERT INTO DATA_SOURCE VALUES \
        (1, 'Legacy MySQL', 'MYSQL', 'jdbc:mysql://127.0.0.1:3306/demo', \
         'developer', 'must-not-migrate', NULL, NULL, NULL, NULL, \
         '127.0.0.1', '3306', NULL, 'demo'), \
        (2, 'Legacy PostgreSQL', 'POSTGRESQL', 'jdbc:postgresql://127.0.0.1/demo', \
         'developer', 'ignored', NULL, NULL, NULL, NULL, \
         '127.0.0.1', '5432', NULL, 'demo');";
    let output = Command::new("java")
        .args(["-cp"])
        .arg(&h2_jar)
        .arg("org.h2.tools.Shell")
        .args(["-url", &legacy_url, "-user", "sa", "-sql", sql])
        .output()
        .expect("H2 fixture command starts");
    assert!(
        output.status.success(),
        "H2 fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let legacy_file = legacy_base.with_extension("mv.db");
    assert!(legacy_file.is_file(), "legacy H2 fixture must exist");

    let driver_pack_root = directory.path().join("driver-packs");
    write_driver_pack(
        &driver_pack_root,
        "02-h2-migration",
        "h2-legacy-migration",
        H2_DRIVER_CLASS,
        &h2_jar,
    );
    let config = managed_runtime_config(
        &engine_jar,
        &directory.path().join("data"),
        &driver_pack_root,
        &STANDARD.encode([0x5a; 32]),
    );
    let mut host = RuntimeHost::open(config)
        .await
        .expect("managed migration runtime opens");
    let application = host.application();
    let imported = application
        .import_legacy_community_datasources_from_file(&legacy_file)
        .await
        .expect("legacy MySQL datasource imports");
    assert!(imported.database_found);
    assert_eq!(imported.imported, 1);
    assert_eq!(imported.skipped_unsupported, 1);
    assert_eq!(imported.password_fields_omitted, 1);

    let datasources = application
        .list_datasources()
        .await
        .expect("imported datasource lists");
    assert_eq!(datasources.items.len(), 1);
    assert_eq!(datasources.items[0].name, "Legacy MySQL");
    let storage = application.storage().expect("storage configured");
    let (_, secret) = storage
        .get_datasource_with_secret(&datasources.items[0].id)
        .expect("imported connection reads");
    let connection: DatasourceConnection =
        serde_json::from_slice(secret.expect("imported connection exists").expose_secret())
            .expect("imported connection decodes");
    assert!(
        connection
            .properties
            .iter()
            .any(|property| property.key == "user" && property.value == "developer")
    );
    assert!(
        connection
            .properties
            .iter()
            .all(|property| !property.key.eq_ignore_ascii_case("password"))
    );
    host.shutdown().await.expect("migration runtime shuts down");
}

#[tokio::test]
async fn partial_managed_driver_preload_cleans_generation_and_releases_storage() {
    let engine_jar = required_jar("CHAT2DB_JAVA_ENGINE_JAR");
    let h2_jar = required_jar("CHAT2DB_H2_DRIVER_JAR");
    let directory = TempDir::new().expect("temporary runtime directory");
    let driver_pack_root = directory.path().join("driver-packs");
    write_driver_pack(&driver_pack_root, "01-h2", "h2", H2_DRIVER_CLASS, &h2_jar);
    write_driver_pack(
        &driver_pack_root,
        "02-invalid",
        "invalid",
        "example.MissingDriver",
        &h2_jar,
    );
    let data_dir = directory.path().join("data");
    let driver_runtime_directory = data_dir.join("jdbc-driver-runtime");
    let master_key = STANDARD.encode([0x5a; 32]);

    let mut host = RuntimeHost::open(managed_runtime_config(
        &engine_jar,
        &data_dir,
        &driver_pack_root,
        &master_key,
    ))
    .await
    .expect("driver discovery must not start Java");
    assert_eq!(host.application().list_drivers().items.len(), 6);
    let error = host
        .acquire_engine()
        .await
        .expect_err("invalid second pack must fail first-use preload");
    assert_eq!(error.api_error().code, "driver.load_failed");
    host.shutdown()
        .await
        .expect("a failed lazy generation must already be reaped");
    assert_directory_empty(&driver_runtime_directory);
    drop(host);

    fs::remove_dir_all(driver_pack_root.join("02-invalid"))
        .expect("invalid pack must be removable after failed preload");
    let mut host = RuntimeHost::open(managed_runtime_config(
        &engine_jar,
        &data_dir,
        &driver_pack_root,
        &master_key,
    ))
    .await
    .expect("storage and driver discovery must reopen immediately");
    assert_eq!(host.application().list_drivers().items.len(), 5);
    let lease = host
        .acquire_engine()
        .await
        .expect("the corrected pack must preload on first use");
    drop(lease);
    host.shutdown()
        .await
        .expect("reopened runtime must shut down cleanly");
    assert_directory_empty(&driver_runtime_directory);
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
                ssh: None,
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
                ssh: None,
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
                ssh: None,
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

fn h2_range_query(datasource_id: String) -> StartQueryRequest {
    StartQueryRequest {
        datasource_id,
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
    }
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

fn assert_engine_available_on_demand(application: &Application) {
    let engine = application
        .health()
        .components
        .into_iter()
        .find(|component| component.id == "database-engine")
        .expect("database engine health must be present");
    assert_eq!(engine.state, ComponentState::Ready);
    assert_eq!(engine.detail, "Available on demand; Java is not running");
}

fn assert_engine_running(application: &Application) {
    let engine = application
        .health()
        .components
        .into_iter()
        .find(|component| component.id == "database-engine")
        .expect("database engine health must be present");
    assert_eq!(engine.state, ComponentState::Ready);
    assert_eq!(engine.detail, "Ready");
}

async fn wait_for_engine_idle(application: &Application) {
    tokio::time::timeout(EVENT_TIMEOUT, async {
        loop {
            let idle = application
                .health()
                .components
                .into_iter()
                .find(|component| component.id == "database-engine")
                .is_some_and(|component| {
                    component.detail == "Available on demand; Java is not running"
                });
            if idle {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("managed Java must become idle before the test deadline");
}

async fn wait_for_engine_parked(application: &Application) {
    tokio::time::timeout(EVENT_TIMEOUT, async {
        loop {
            let parked = application
                .health()
                .components
                .into_iter()
                .find(|component| component.id == "database-engine")
                .is_some_and(|component| component.detail == "Java is parked; ready to resume");
            if parked {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("managed Java must enter cooperative park before hard idle");
}

fn managed_runtime_config(
    engine_jar: &Path,
    data_dir: &Path,
    driver_pack_root: &Path,
    master_key: &str,
) -> RuntimeConfig {
    let engine = EngineConfig::new(EngineCommand::java_jar("java", engine_jar)).with_timeouts(
        Duration::from_secs(10),
        Duration::from_secs(10),
        Duration::from_secs(5),
    );
    RuntimeConfig::new(engine)
        .with_data_dir(data_dir)
        .with_driver_pack_dir(driver_pack_root)
        .with_vault_master_key_base64(master_key)
}

fn write_driver_pack(
    root: &Path,
    directory: &str,
    id: &str,
    driver_class: &str,
    source_jar: &Path,
) {
    let pack = root.join(directory);
    fs::create_dir_all(&pack).expect("driver pack directory must be created");
    let artifact_path = pack.join("driver.jar");
    fs::copy(source_jar, &artifact_path).expect("driver artifact must be copied into its pack");
    let artifact = DriverArtifact::from_path(&artifact_path).expect("driver artifact must hash");
    let mut sha256 = String::with_capacity(64);
    for byte in artifact.sha256() {
        write!(&mut sha256, "{byte:02x}").expect("writing to String cannot fail");
    }
    fs::write(
        pack.join("driver-pack.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "id": id,
            "name": id,
            "version": "test",
            "driverClass": driver_class,
            "artifacts": [{"path": "driver.jar", "sha256": sha256}]
        }))
        .expect("driver manifest must serialize"),
    )
    .expect("driver manifest must be written");
}

fn assert_directory_empty(path: &Path) {
    let count = fs::read_dir(path)
        .expect("JDBC runtime directory must exist")
        .count();
    assert_eq!(count, 0, "JDBC runtime directory must be empty");
}

fn required_jar(variable: &str) -> PathBuf {
    let path = std::env::var_os(variable).map_or_else(
        || panic!("{variable} must point to a packaged JAR"),
        PathBuf::from,
    );
    assert!(path.is_file(), "{variable} does not point to a file");
    path
}
