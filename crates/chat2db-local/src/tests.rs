use std::{fs, sync::Arc, time::Duration};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chat2db_contract::{
    ApiError, CreateDatasourceRequest, DatabaseWriteState, ExecuteDatabaseWriteRequest, JdbcValue,
    QueryLimits, ResultMetadata, ResultPage, ResultPageRequest, ResultRow, StartQueryRequest,
};
use chat2db_core::Application;
use chat2db_storage::{SecretRef, SecretValue, SecretVault, SecretVaultError, Storage};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt as _;

use crate::{
    AttachmentOutcome, AttachmentRequest, AttachmentResponse, EndpointMetadata, LOCK_FILE,
    LocalClient, LocalError, LocalServer, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, METADATA_FILE,
    PROTOCOL_VERSION, RemoteError, bound_result_page,
    server::{MAX_CONNECTIONS, enforce_response_limit},
    transport,
};

#[cfg(any(unix, windows))]
use crate::Endpoint;
#[cfg(unix)]
use crate::SOCKET_FILE;

struct EmptyVault;

impl SecretVault for EmptyVault {
    fn probe(&self) -> Result<(), SecretVaultError> {
        Ok(())
    }

    fn create(&self, _reference: &SecretRef, _value: &SecretValue) -> Result<(), SecretVaultError> {
        Ok(())
    }

    fn get(&self, _reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
        Ok(None)
    }

    fn delete(&self, _reference: &SecretRef) -> Result<(), SecretVaultError> {
        Ok(())
    }
}

fn setup() -> (TempDir, Application) {
    let directory = TempDir::new().expect("temp dir");
    let storage = Storage::open(directory.path(), Arc::new(EmptyVault)).expect("storage opens");
    (directory, Application::with_storage(storage))
}

fn page_with_text_rows(values: &[&str]) -> ResultPage {
    ResultPage {
        metadata: ResultMetadata {
            id: "result-1".to_owned(),
            row_count: values.len().to_string(),
            byte_count: "0".to_owned(),
            truncated_by_max_rows: false,
            truncated_by_max_result_bytes: false,
            created_at_ms: "1".to_owned(),
            expires_at_ms: "2".to_owned(),
        },
        columns: Vec::new(),
        offset: "0".to_owned(),
        rows: values
            .iter()
            .map(|value| ResultRow {
                values: vec![JdbcValue::Text {
                    value: (*value).to_owned(),
                }],
            })
            .collect(),
        has_more: false,
    }
}

#[tokio::test]
async fn serves_real_application_state_and_cleans_discovery_files() {
    let (directory, application) = setup();
    application
        .create_datasource(CreateDatasourceRequest {
            name: "Local test".to_owned(),
            driver_id: "driver-1".to_owned(),
            connection: None,
        })
        .await
        .expect("datasource creates");
    let mut server = LocalServer::start(application).expect("server starts");
    let client = LocalClient::new(directory.path());

    let health = client.health().await.expect("health reads");
    assert!(
        health
            .components
            .iter()
            .any(|component| component.id == "local-storage" && component.detail.contains("ready"))
    );
    let datasources = client.list_datasources().await.expect("datasources list");
    assert_eq!(datasources.items.len(), 1);
    assert_eq!(datasources.items[0].name, "Local test");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        for path in [
            directory.path().join(LOCK_FILE),
            directory.path().join(METADATA_FILE),
            directory.path().join(SOCKET_FILE),
        ] {
            assert_eq!(
                fs::symlink_metadata(path)
                    .expect("endpoint metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[cfg(windows)]
    {
        chat2db_local_ipc_windows::verify_owner_only_directory(directory.path())
            .expect("data directory is owner-only");
        drop(
            chat2db_local_ipc_windows::open_owner_only_file(&directory.path().join(LOCK_FILE))
                .expect("lock is owner-only"),
        );
        drop(
            chat2db_local_ipc_windows::open_owner_only_file(&directory.path().join(METADATA_FILE))
                .expect("metadata is owner-only"),
        );
    }

    server.shutdown().await.expect("server shuts down");
    assert!(!directory.path().join(METADATA_FILE).exists());
    #[cfg(unix)]
    assert!(!directory.path().join(SOCKET_FILE).exists());
}

#[tokio::test]
async fn local_runtime_enforces_database_write_confirmation() {
    let (directory, application) = setup();
    let mut server = LocalServer::start(application).expect("server starts");
    let result = LocalClient::new(directory.path())
        .execute_database_write(ExecuteDatabaseWriteRequest {
            datasource_id: "missing-datasource".to_owned(),
            sql: "UPDATE items SET label = 'changed'".to_owned(),
            confirmed: false,
        })
        .await;

    assert_eq!(result.state, DatabaseWriteState::NotStarted);
    assert_eq!(
        result.error.as_ref().map(|error| error.code.as_str()),
        Some("database_write_confirmation_required")
    );
    server.shutdown().await.expect("server shuts down");
}

#[tokio::test]
async fn rejects_a_second_listener_for_the_same_runtime() {
    let (_directory, application) = setup();
    let mut first = LocalServer::start(application.clone()).expect("first server starts");
    let second = LocalServer::start(application);
    assert!(matches!(second, Err(LocalError::Unavailable(_))));
    first.shutdown().await.expect("first server shuts down");
}

#[tokio::test]
async fn repeated_shutdown_and_drop_do_not_remove_replacement_metadata() {
    let (directory, application) = setup();
    let mut old = LocalServer::start(application.clone()).expect("old server starts");
    old.shutdown().await.expect("old server shuts down");

    let mut replacement = LocalServer::start(application).expect("replacement server starts");
    old.shutdown().await.expect("old shutdown is idempotent");
    drop(old);

    LocalClient::new(directory.path())
        .health()
        .await
        .expect("replacement metadata remains discoverable");
    replacement
        .shutdown()
        .await
        .expect("replacement server shuts down");
}

#[test]
fn starting_without_a_tokio_runtime_leaves_no_discovery_material() {
    let (directory, application) = setup();
    let error = LocalServer::start(application).expect_err("runtime is required");
    assert!(matches!(error, LocalError::Unavailable(_)));
    assert!(!directory.path().join(LOCK_FILE).exists());
    assert!(!directory.path().join(METADATA_FILE).exists());
    #[cfg(unix)]
    assert!(!directory.path().join(SOCKET_FILE).exists());
}

#[tokio::test]
async fn shutdown_cancels_a_saturated_connection_set() {
    let (directory, application) = setup();
    let mut server = LocalServer::start(application).expect("server starts");
    let metadata: EndpointMetadata = serde_json::from_slice(
        &fs::read(directory.path().join(METADATA_FILE)).expect("metadata reads"),
    )
    .expect("metadata decodes");
    let mut stalled = Vec::with_capacity(MAX_CONNECTIONS);
    for _ in 0..MAX_CONNECTIONS {
        stalled.push(
            tokio::time::timeout(
                Duration::from_secs(2),
                transport::connect(&metadata.endpoint, metadata.process_id),
            )
            .await
            .expect("stalled connection opens before its deadline")
            .expect("stalled connection opens"),
        );
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    tokio::time::timeout(Duration::from_secs(2), server.shutdown())
        .await
        .expect("saturated shutdown remains cancellable")
        .expect("server shuts down cleanly");
    drop(stalled);
}

#[cfg(unix)]
#[tokio::test]
async fn publishes_an_absolute_socket_path_for_a_relative_data_directory() {
    let directory = TempDir::new_in(".").expect("relative temp dir");
    let current_directory = std::env::current_dir().expect("current directory");
    let relative_directory = directory
        .path()
        .strip_prefix(&current_directory)
        .expect("temp dir is below the current directory");
    assert!(relative_directory.is_relative());
    let storage = Storage::open(relative_directory, Arc::new(EmptyVault)).expect("storage opens");
    let mut server = LocalServer::start(Application::with_storage(storage)).expect("server starts");
    let metadata: EndpointMetadata = serde_json::from_slice(
        &fs::read(directory.path().join(METADATA_FILE)).expect("metadata reads"),
    )
    .expect("metadata decodes");
    let Endpoint::UnixSocket { path } = metadata.endpoint else {
        panic!("expected Unix endpoint");
    };
    assert!(path.is_absolute());
    server.shutdown().await.expect("server shuts down");
}

#[tokio::test]
async fn rejects_forged_tokens_without_exposing_product_state() {
    let (directory, application) = setup();
    let mut server = LocalServer::start(application).expect("server starts");
    let path = directory.path().join(METADATA_FILE);
    let mut metadata: EndpointMetadata =
        serde_json::from_slice(&fs::read(&path).expect("metadata reads"))
            .expect("metadata decodes");
    metadata.token = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    fs::write(
        &path,
        serde_json::to_vec(&metadata).expect("metadata encodes"),
    )
    .expect("metadata rewrites");

    let error = LocalClient::new(directory.path())
        .health()
        .await
        .expect_err("forged token must fail");
    let LocalError::Remote(error) = error else {
        panic!("unexpected error: {error}");
    };
    let RemoteError(error) = *error;
    assert_eq!(error.code, "local_attachment_unauthorized");
    server.shutdown().await.expect("server shuts down");
}

#[cfg(windows)]
#[tokio::test]
async fn rejects_forged_server_process_ids_before_writing_requests() {
    use std::{ffi::OsStr, io::Write as _};

    use chat2db_local_ipc_windows::{
        PipeInstanceKind, create_new_owner_only_file, create_owner_only_named_pipe,
        secure_owner_only_directory,
    };
    use tokio::io::AsyncReadExt as _;

    let directory = TempDir::new().expect("temp dir");
    secure_owner_only_directory(directory.path()).expect("data directory is owner-only");
    let name = format!(r"\\.\pipe\chat2db-rust-pid-test-{}", uuid::Uuid::new_v4());
    let mut pipe = create_owner_only_named_pipe(OsStr::new(&name), PipeInstanceKind::First, 1)
        .expect("test pipe creates");
    let path = directory.path().join(METADATA_FILE);
    let actual_process_id = std::process::id();
    let forged_process_id = if actual_process_id == u32::MAX {
        actual_process_id - 1
    } else {
        actual_process_id + 1
    };
    let metadata = EndpointMetadata {
        protocol_version: PROTOCOL_VERSION,
        endpoint: Endpoint::WindowsNamedPipe { name },
        token: URL_SAFE_NO_PAD.encode([7_u8; 32]),
        process_id: forged_process_id,
    };
    let mut metadata_file = create_new_owner_only_file(&path).expect("metadata creates");
    metadata_file
        .write_all(&serde_json::to_vec(&metadata).expect("metadata encodes"))
        .expect("metadata writes");
    metadata_file.sync_all().expect("metadata syncs");
    drop(metadata_file);

    let received = tokio::spawn(async move {
        pipe.connect().await.expect("test pipe accepts client");
        let mut byte = [0_u8; 1];
        match tokio::time::timeout(Duration::from_secs(2), pipe.read(&mut byte)).await {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::UnexpectedEof
                ) =>
            {
                0
            }
            Ok(Err(error)) => panic!("read forged-pid client: {error}"),
            Err(error) => panic!("forged-pid client did not disconnect: {error}"),
        }
    });
    tokio::task::yield_now().await;

    let error = LocalClient::new(directory.path())
        .health()
        .await
        .expect_err("forged server pid must fail before request write");
    let LocalError::Io { operation, source } = error else {
        panic!("unexpected error: {error}");
    };
    assert_eq!(operation, "authenticate Windows named pipe");
    assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied);
    let bytes = received.await.expect("test pipe task joins");
    assert_eq!(bytes, 0, "the client must authenticate before writing");
}

#[cfg(windows)]
#[tokio::test]
async fn retries_a_busy_named_pipe_after_one_connection_is_released() {
    use std::ffi::OsStr;

    use chat2db_local_ipc_windows::{
        PipeInstanceKind, create_owner_only_named_pipe, is_named_pipe_busy,
    };
    use tokio::net::windows::named_pipe::ClientOptions;

    let name = format!(r"\\.\pipe\chat2db-rust-busy-test-{}", uuid::Uuid::new_v4());
    let server = create_owner_only_named_pipe(OsStr::new(&name), PipeInstanceKind::First, 1)
        .expect("test pipe creates");
    let blocker = ClientOptions::new()
        .open(&name)
        .expect("first client occupies the only pipe instance");
    server.connect().await.expect("server accepts blocker");
    let Err(busy_error) = ClientOptions::new().open(&name) else {
        panic!("a second client must observe a saturated named pipe");
    };
    assert!(is_named_pipe_busy(&busy_error));

    let endpoint = Endpoint::WindowsNamedPipe { name };
    let mut connecting = Box::pin(transport::connect(&endpoint, std::process::id()));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut connecting)
            .await
            .is_err(),
        "the connection future must remain pending while the pipe is busy"
    );

    drop(blocker);
    server.disconnect().expect("occupied instance disconnects");
    let (accepted, recovered) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(server.connect(), &mut connecting)
    })
    .await
    .expect("busy retry and server accept recover before their deadline");
    accepted.expect("server accepts the retried client");
    let recovered = recovered.expect("connection opens after the blocker is released");
    drop(recovered);
}

#[tokio::test]
async fn enforces_local_page_bounds_before_storage_access() {
    let (directory, application) = setup();
    let mut server = LocalServer::start(application).expect("server starts");
    let error = LocalClient::new(directory.path())
        .result_page(
            "missing-result",
            ResultPageRequest {
                offset: "0".to_owned(),
                max_rows: "1001".to_owned(),
                max_bytes: "1".to_owned(),
            },
        )
        .await
        .expect_err("oversized page must fail");
    let LocalError::Remote(error) = error else {
        panic!("unexpected error: {error}");
    };
    let RemoteError(error) = *error;
    assert_eq!(error.code, "invalid_result_page");
    server.shutdown().await.expect("server shuts down");
}

#[tokio::test]
async fn accepts_cli_page_budget_before_storage_access() {
    let (directory, application) = setup();
    let mut server = LocalServer::start(application).expect("server starts");
    let error = LocalClient::new(directory.path())
        .result_page(
            "missing-result",
            ResultPageRequest {
                offset: "0".to_owned(),
                max_rows: "100".to_owned(),
                max_bytes: "262144".to_owned(),
            },
        )
        .await
        .expect_err("missing result must fail after local validation");
    let LocalError::Remote(error) = error else {
        panic!("unexpected error: {error}");
    };
    let RemoteError(error) = *error;
    assert_eq!(error.code, "result_not_found");
    server.shutdown().await.expect("server shuts down");
}

#[test]
fn bounds_result_rows_by_their_actual_json_size() {
    let page = page_with_text_rows(&["first", "second"]);
    let first_row_bytes = u64::try_from(serde_json::to_vec(&page.rows[0]).unwrap().len())
        .expect("row length fits u64");

    let bounded = bound_result_page(page, first_row_bytes).expect("first row fits");
    assert_eq!(bounded.rows.len(), 1);
    assert!(bounded.has_more);
}

#[test]
fn rejects_a_single_result_row_over_the_local_budget() {
    let page = page_with_text_rows(&["too large"]);
    let row_bytes = u64::try_from(serde_json::to_vec(&page.rows[0]).unwrap().len())
        .expect("row length fits u64");

    let error = bound_result_page(page, row_bytes - 1).expect_err("row must not be truncated");
    assert_eq!(error.code, "local_result_row_too_large");
}

#[test]
fn replaces_an_oversized_response_with_a_structured_error() {
    let response = AttachmentResponse {
        protocol_version: PROTOCOL_VERSION,
        request_id: "request-1".to_owned(),
        outcome: AttachmentOutcome::Error(Box::new(ApiError::new(
            "oversized_fixture",
            "x".repeat(MAX_RESPONSE_BYTES),
        ))),
    };

    let bounded = enforce_response_limit(response);
    let AttachmentOutcome::Error(error) = &bounded.outcome else {
        panic!("oversized response must become an error");
    };
    assert_eq!(error.code, "local_response_too_large");
    assert!(serde_json::to_vec(&bounded).unwrap().len() <= MAX_RESPONSE_BYTES);
}

#[tokio::test]
async fn read_query_never_falls_back_to_an_unconfigured_engine() {
    let (directory, application) = setup();
    let mut server = LocalServer::start(application).expect("server starts");
    let error = LocalClient::new(directory.path())
        .start_read_query(StartQueryRequest {
            datasource_id: "datasource-1".to_owned(),
            sql: "select 1".to_owned(),
            parameters: Vec::new(),
            limits: QueryLimits {
                max_rows: "10".to_owned(),
                max_result_bytes: "1024".to_owned(),
                batch_rows: 10,
                batch_bytes: 1024,
                result_ttl_seconds: 60,
            },
        })
        .await
        .expect_err("missing engine must fail");
    let LocalError::Remote(error) = error else {
        panic!("unexpected error: {error}");
    };
    let RemoteError(error) = *error;
    assert_eq!(error.code, "database_engine_unavailable");
    server.shutdown().await.expect("server shuts down");
}

#[tokio::test]
async fn rejects_zero_length_oversized_and_truncated_frames() {
    for length in [
        0_u32,
        u32::try_from(MAX_REQUEST_BYTES + 1).expect("bound fits u32"),
    ] {
        let (mut writer, mut reader) = tokio::io::duplex(16);
        writer
            .write_all(&length.to_be_bytes())
            .await
            .expect("header writes");
        let Err(error) =
            transport::read_message::<AttachmentRequest>(&mut reader, MAX_REQUEST_BYTES).await
        else {
            panic!("invalid frame must fail");
        };
        assert!(matches!(error, LocalError::Protocol(_)));
    }

    let (mut writer, mut reader) = tokio::io::duplex(16);
    writer
        .write_all(&5_u32.to_be_bytes())
        .await
        .expect("header writes");
    writer.write_all(b"{}").await.expect("partial body writes");
    writer.shutdown().await.expect("writer closes");
    let Err(error) =
        transport::read_message::<AttachmentRequest>(&mut reader, MAX_REQUEST_BYTES).await
    else {
        panic!("truncated frame must fail");
    };
    assert!(matches!(error, LocalError::Io { .. }));
}
