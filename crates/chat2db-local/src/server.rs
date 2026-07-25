use std::{fs, io, path::Path, time::Duration};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chat2db_contract::ApiError;
use chat2db_core::Application;
use rand::RngCore;
use subtle::ConstantTimeEq;
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::{
    AttachmentCommand, AttachmentOutcome, AttachmentPayload, AttachmentRequest, AttachmentResponse,
    EndpointMetadata, LOCK_FILE, LocalError, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, METADATA_FILE,
    PROTOCOL_VERSION, bound_result_page, transport, validate_page_request,
};

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const MAX_CONNECTIONS: usize = 32;

/// Product-host-owned local attachment listener.
pub struct LocalServer {
    cancellation: CancellationToken,
    task: Option<JoinHandle<Result<(), LocalError>>>,
    data_dir: std::path::PathBuf,
    metadata: EndpointMetadata,
    lock: Option<fs::File>,
}

impl std::fmt::Debug for LocalServer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalServer")
            .field("data_dir", &self.data_dir)
            .finish_non_exhaustive()
    }
}

impl LocalServer {
    /// Starts an owner-only listener beside the configured product storage.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no active Tokio runtime, storage is
    /// unavailable, the endpoint cannot be secured, or endpoint discovery
    /// metadata cannot be published atomically.
    pub fn start(application: Application) -> Result<Self, LocalError> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| {
            LocalError::Unavailable("local attachment must start inside a Tokio runtime".to_owned())
        })?;
        let storage = application.storage().ok_or_else(|| {
            LocalError::Unavailable("product storage is not configured".to_owned())
        })?;
        let data_dir = fs::canonicalize(storage.data_dir())
            .map_err(|error| LocalError::io("resolve attachment directory", error))?;
        #[cfg(windows)]
        chat2db_local_ipc_windows::secure_owner_only_directory(&data_dir)
            .map_err(|error| LocalError::io("secure attachment directory", error))?;
        let lock = acquire_lock(&data_dir)?;
        let mut token = [0_u8; 32];
        rand::rng().fill_bytes(&mut token);
        let encoded_token = URL_SAFE_NO_PAD.encode(token);
        let (listener, endpoint) = transport::Listener::bind(&data_dir)?;
        let metadata = EndpointMetadata {
            protocol_version: PROTOCOL_VERSION,
            endpoint: endpoint.clone(),
            token: encoded_token,
            process_id: std::process::id(),
        };
        if let Err(error) = publish_metadata(&data_dir, &metadata) {
            listener.cleanup();
            cleanup_metadata(&data_dir, &metadata);
            return Err(error);
        }
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let task_metadata = metadata.clone();
        let task = runtime.spawn(run(
            listener,
            application,
            token,
            task_cancellation,
            data_dir.clone(),
            task_metadata,
        ));
        Ok(Self {
            cancellation,
            task: Some(task),
            data_dir,
            metadata,
            lock: Some(lock),
        })
    }

    /// Stops accepting clients, terminates active attachment requests, and
    /// removes discovery material.
    ///
    /// # Errors
    ///
    /// Returns a listener or task failure observed during shutdown.
    pub async fn shutdown(&mut self) -> Result<(), LocalError> {
        if self.lock.is_none() {
            return Ok(());
        }
        self.cancellation.cancel();
        let result = match self.task.take() {
            Some(mut task) => match tokio::time::timeout(SHUTDOWN_TIMEOUT, &mut task).await {
                Ok(Ok(result)) => result,
                Ok(Err(error)) => Err(LocalError::Task(error.to_string())),
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    Err(LocalError::Timeout("server shutdown"))
                }
            },
            None => Ok(()),
        };
        cleanup_metadata(&self.data_dir, &self.metadata);
        drop(self.lock.take());
        result
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if self.lock.is_some() {
            cleanup_metadata(&self.data_dir, &self.metadata);
        }
    }
}

async fn run(
    mut listener: transport::Listener,
    application: Application,
    token: [u8; 32],
    cancellation: CancellationToken,
    data_dir: std::path::PathBuf,
    metadata: EndpointMetadata,
) -> Result<(), LocalError> {
    let _discovery = DiscoveryGuard { data_dir, metadata };
    let mut connections = JoinSet::new();
    'accept: loop {
        while connections.len() >= MAX_CONNECTIONS {
            tokio::select! {
                () = cancellation.cancelled() => break 'accept,
                _ = connections.join_next() => {}
            }
        }
        let accepted = tokio::select! {
            () = cancellation.cancelled() => break,
            accepted = listener.accept() => accepted,
        }?;
        let application = application.clone();
        connections.spawn(async move {
            if let Err(error) = handle_connection(accepted, application, token).await {
                tracing::warn!(%error, "local attachment request failed");
            }
        });
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    listener.cleanup();
    Ok(())
}

async fn handle_connection(
    mut io: transport::BoxedIo,
    application: Application,
    token: [u8; 32],
) -> Result<(), LocalError> {
    let request: AttachmentRequest = tokio::time::timeout(
        CONNECTION_TIMEOUT,
        transport::read_message(&mut io, MAX_REQUEST_BYTES),
    )
    .await
    .map_err(|_| LocalError::Timeout("request read"))??;
    let response = response_for(request, application, token).await;
    tokio::time::timeout(
        CONNECTION_TIMEOUT,
        transport::write_message(&mut io, &response, MAX_RESPONSE_BYTES),
    )
    .await
    .map_err(|_| LocalError::Timeout("response write"))?
}

async fn response_for(
    request: AttachmentRequest,
    application: Application,
    token: [u8; 32],
) -> AttachmentResponse {
    let request_id = request.request_id;
    let outcome = if request.protocol_version != PROTOCOL_VERSION {
        AttachmentOutcome::Error(Box::new(ApiError::new(
            "local_protocol_version_mismatch",
            format!(
                "Local protocol version {} is not supported",
                request.protocol_version
            ),
        )))
    } else if request_id.is_empty() || request_id.len() > 64 {
        AttachmentOutcome::Error(Box::new(ApiError::new(
            "invalid_local_request",
            "requestId must contain between 1 and 64 bytes",
        )))
    } else if !valid_token(&request.token, &token) {
        AttachmentOutcome::Error(Box::new(ApiError::new(
            "local_attachment_unauthorized",
            "Local attachment authentication failed",
        )))
    } else {
        dispatch(application, request.command).await
    };
    enforce_response_limit(AttachmentResponse {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        outcome,
    })
}

pub(crate) fn enforce_response_limit(mut response: AttachmentResponse) -> AttachmentResponse {
    let fits = transport::encode_message(&response, MAX_RESPONSE_BYTES).is_ok();
    if !fits {
        response.outcome = AttachmentOutcome::Error(Box::new(ApiError::new(
            "local_response_too_large",
            "The local response exceeds the maximum transport frame",
        )));
    }
    response
}

async fn dispatch(application: Application, command: AttachmentCommand) -> AttachmentOutcome {
    let result = match command {
        AttachmentCommand::Health => Ok(AttachmentPayload::Health(Box::new(application.health()))),
        AttachmentCommand::ListDatasources => application
            .list_datasources()
            .await
            .map(|value| AttachmentPayload::Datasources(Box::new(value))),
        AttachmentCommand::StartReadQuery { request } => application
            .start_read_query(request)
            .await
            .map(|value| AttachmentPayload::QueryAccepted(Box::new(value))),
        AttachmentCommand::OperationSnapshot { operation_id } => application
            .operation_snapshot(&operation_id)
            .await
            .map(|value| AttachmentPayload::OperationSnapshot(Box::new(value))),
        AttachmentCommand::CancelOperation { operation_id } => {
            Ok(AttachmentPayload::CancelOperation(Box::new(
                application.cancel_operation(&operation_id).await,
            )))
        }
        AttachmentCommand::ResultPage { result_id, request } => {
            match validate_page_request(&request) {
                Ok(max_bytes) => {
                    let mut storage_request = request;
                    storage_request.max_bytes = chat2db_storage::MIN_RESULT_PAGE_BYTES.to_string();
                    match application.result_page(&result_id, storage_request).await {
                        Ok(value) => match bound_result_page(value, max_bytes) {
                            Ok(value) => Ok(AttachmentPayload::ResultPage(Box::new(value))),
                            Err(error) => return AttachmentOutcome::Error(error),
                        },
                        Err(error) => Err(error),
                    }
                }
                Err(error) => return AttachmentOutcome::Error(error),
            }
        }
    };
    match result {
        Ok(payload) => AttachmentOutcome::Success(Box::new(payload)),
        Err(error) => AttachmentOutcome::Error(Box::new(error.api_error())),
    }
}

fn valid_token(encoded: &str, expected: &[u8; 32]) -> bool {
    URL_SAFE_NO_PAD
        .decode(encoded)
        .ok()
        .filter(|token| token.len() == expected.len())
        .is_some_and(|token| bool::from(expected.ct_eq(token.as_slice())))
}

fn acquire_lock(data_dir: &Path) -> Result<fs::File, LocalError> {
    let path = data_dir.join(LOCK_FILE);
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;

        let mut options = fs::OpenOptions::new();
        options.create(true).read(true).write(true).truncate(false);
        options.mode(0o600);
        options
            .open(&path)
            .map_err(|error| LocalError::io("open attachment lock", error))?
    };
    #[cfg(windows)]
    let file = chat2db_local_ipc_windows::open_or_create_owner_only_file(&path)
        .map_err(|error| LocalError::io("open owner-only attachment lock", error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let metadata = file
            .metadata()
            .map_err(|error| LocalError::io("inspect attachment lock", error))?;
        if metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(LocalError::Unavailable(
                "attachment lock is not owner-only".to_owned(),
            ));
        }
    }
    fs2::FileExt::try_lock_exclusive(&file).map_err(|error| {
        if lock_is_contended(&error) {
            LocalError::Unavailable("another local attachment listener is active".to_owned())
        } else {
            LocalError::io("lock local attachment", error)
        }
    })?;
    Ok(file)
}

fn lock_is_contended(error: &io::Error) -> bool {
    let expected = fs2::lock_contended_error();
    match (error.raw_os_error(), expected.raw_os_error()) {
        (Some(actual), Some(expected)) => actual == expected,
        _ => error.kind() == expected.kind(),
    }
}

fn publish_metadata(data_dir: &Path, metadata: &EndpointMetadata) -> Result<(), LocalError> {
    use std::io::Write as _;

    let destination = data_dir.join(METADATA_FILE);
    if let Ok(existing) = fs::symlink_metadata(&destination)
        && !existing.file_type().is_file()
    {
        return Err(LocalError::Unavailable(format!(
            "refusing to replace non-file endpoint metadata at {}",
            destination.display()
        )));
    }
    let temporary = data_dir.join(format!(".{METADATA_FILE}.{}.tmp", uuid::Uuid::new_v4()));
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;

        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        options.mode(0o600);
        options
            .open(&temporary)
            .map_err(|error| LocalError::io("create endpoint metadata", error))?
    };
    #[cfg(windows)]
    let mut file = chat2db_local_ipc_windows::create_new_owner_only_file(&temporary)
        .map_err(|error| LocalError::io("create owner-only endpoint metadata", error))?;
    let encoded = serde_json::to_vec(metadata)?;
    let result = (|| {
        file.write_all(&encoded)
            .map_err(|error| LocalError::io("write endpoint metadata", error))?;
        file.sync_all()
            .map_err(|error| LocalError::io("sync endpoint metadata", error))?;
        drop(file);
        publish_metadata_file(&temporary, &destination)
            .map_err(|error| LocalError::io("publish endpoint metadata", error))?;
        sync_directory(data_dir)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(unix)]
fn publish_metadata_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn publish_metadata_file(source: &Path, destination: &Path) -> io::Result<()> {
    chat2db_local_ipc_windows::replace_file(source, destination)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), LocalError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| LocalError::io("sync endpoint directory", error))
}

#[cfg(windows)]
#[allow(
    clippy::unnecessary_wraps,
    reason = "keeps the platform-specific metadata publication contract uniform"
)]
fn sync_directory(_path: &Path) -> Result<(), LocalError> {
    Ok(())
}

struct DiscoveryGuard {
    data_dir: std::path::PathBuf,
    metadata: EndpointMetadata,
}

impl Drop for DiscoveryGuard {
    fn drop(&mut self) {
        cleanup_metadata(&self.data_dir, &self.metadata);
    }
}

fn cleanup_metadata(data_dir: &Path, expected: &EndpointMetadata) {
    let metadata = data_dir.join(METADATA_FILE);
    match fs::read(&metadata) {
        Ok(encoded) => match serde_json::from_slice::<EndpointMetadata>(&encoded) {
            Ok(current) if current == *expected => {
                if let Err(error) = fs::remove_file(&metadata)
                    && error.kind() != io::ErrorKind::NotFound
                {
                    tracing::warn!(%error, path = %metadata.display(), "failed to remove endpoint metadata");
                }
            }
            Ok(_) => {
                tracing::warn!(path = %metadata.display(), "refusing to remove replaced endpoint metadata");
            }
            Err(error) => {
                tracing::warn!(%error, path = %metadata.display(), "refusing to remove malformed endpoint metadata");
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(%error, path = %metadata.display(), "failed to inspect endpoint metadata during cleanup");
        }
    }
}
