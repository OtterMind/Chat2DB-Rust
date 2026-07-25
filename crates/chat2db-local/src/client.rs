use std::{path::PathBuf, time::Duration};

#[cfg(unix)]
use std::fs;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chat2db_contract::{
    CancelOperationResponse, DatasourceList, HealthResponse, OperationSnapshot, QueryAccepted,
    ResultPage, ResultPageRequest, StartQueryRequest,
};
use uuid::Uuid;

use crate::{
    AttachmentCommand, AttachmentOutcome, AttachmentPayload, AttachmentRequest, AttachmentResponse,
    EndpointMetadata, LocalError, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, METADATA_FILE,
    PROTOCOL_VERSION, RemoteError, transport,
};

const ATTACHMENT_IO_TIMEOUT: Duration = Duration::from_secs(10);

/// Client for the currently running product host in one `Chat2DB` data directory.
#[derive(Debug, Clone)]
pub struct LocalClient {
    data_dir: PathBuf,
}

impl LocalClient {
    #[must_use]
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    /// Discovers the operating system's standard `Chat2DB` data directory.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform has no application data directory.
    pub fn discover_default() -> Result<Self, LocalError> {
        chat2db_storage::Storage::default_data_dir()
            .map(Self::new)
            .map_err(|error| LocalError::Unavailable(error.to_string()))
    }

    /// Reads health from the attached product runtime.
    ///
    /// # Errors
    ///
    /// Returns discovery, authentication, transport, protocol, or product errors.
    pub async fn health(&self) -> Result<HealthResponse, LocalError> {
        match *self.call(AttachmentCommand::Health).await? {
            AttachmentPayload::Health(value) => Ok(*value),
            _ => Err(unexpected_payload()),
        }
    }

    /// Lists secret-free datasource metadata from the attached runtime.
    ///
    /// # Errors
    ///
    /// Returns discovery, authentication, transport, protocol, or product errors.
    pub async fn list_datasources(&self) -> Result<DatasourceList, LocalError> {
        match *self.call(AttachmentCommand::ListDatasources).await? {
            AttachmentPayload::Datasources(value) => Ok(*value),
            _ => Err(unexpected_payload()),
        }
    }

    /// Starts a forced-read-only database query in the attached runtime.
    ///
    /// # Errors
    ///
    /// Returns discovery, authentication, transport, validation, or product errors.
    pub async fn start_read_query(
        &self,
        request: StartQueryRequest,
    ) -> Result<QueryAccepted, LocalError> {
        match *self
            .call(AttachmentCommand::StartReadQuery { request })
            .await?
        {
            AttachmentPayload::QueryAccepted(value) => Ok(*value),
            _ => Err(unexpected_payload()),
        }
    }

    /// Reads the current state of one attached database operation.
    ///
    /// # Errors
    ///
    /// Returns discovery, authentication, transport, protocol, or product errors.
    pub async fn operation_snapshot(
        &self,
        operation_id: impl Into<String>,
    ) -> Result<OperationSnapshot, LocalError> {
        match *self
            .call(AttachmentCommand::OperationSnapshot {
                operation_id: operation_id.into(),
            })
            .await?
        {
            AttachmentPayload::OperationSnapshot(value) => Ok(*value),
            _ => Err(unexpected_payload()),
        }
    }

    /// Requests idempotent cancellation of an attached database operation.
    ///
    /// # Errors
    ///
    /// Returns discovery, authentication, transport, protocol, or product errors.
    pub async fn cancel_operation(
        &self,
        operation_id: impl Into<String>,
    ) -> Result<CancelOperationResponse, LocalError> {
        match *self
            .call(AttachmentCommand::CancelOperation {
                operation_id: operation_id.into(),
            })
            .await?
        {
            AttachmentPayload::CancelOperation(value) => Ok(*value),
            _ => Err(unexpected_payload()),
        }
    }

    /// Reads one row- and byte-bounded retained-result page.
    ///
    /// # Errors
    ///
    /// Returns discovery, authentication, transport, validation, or product errors.
    pub async fn result_page(
        &self,
        result_id: impl Into<String>,
        request: ResultPageRequest,
    ) -> Result<ResultPage, LocalError> {
        match *self
            .call(AttachmentCommand::ResultPage {
                result_id: result_id.into(),
                request,
            })
            .await?
        {
            AttachmentPayload::ResultPage(value) => Ok(*value),
            _ => Err(unexpected_payload()),
        }
    }

    async fn call(&self, command: AttachmentCommand) -> Result<Box<AttachmentPayload>, LocalError> {
        let metadata = self.load_metadata()?;
        let request_id = Uuid::new_v4().to_string();
        let request = AttachmentRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.clone(),
            token: metadata.token,
            command,
        };
        let mut io = tokio::time::timeout(
            ATTACHMENT_IO_TIMEOUT,
            transport::connect(&metadata.endpoint, metadata.process_id),
        )
        .await
        .map_err(|_| LocalError::Timeout("connect"))??;
        tokio::time::timeout(
            ATTACHMENT_IO_TIMEOUT,
            transport::write_message(&mut io, &request, MAX_REQUEST_BYTES),
        )
        .await
        .map_err(|_| LocalError::Timeout("request write"))??;
        let response: AttachmentResponse = tokio::time::timeout(
            ATTACHMENT_IO_TIMEOUT,
            transport::read_message(&mut io, MAX_RESPONSE_BYTES),
        )
        .await
        .map_err(|_| LocalError::Timeout("response read"))??;
        if response.protocol_version != PROTOCOL_VERSION {
            return Err(LocalError::Protocol(format!(
                "runtime protocol version {} does not match client version {PROTOCOL_VERSION}",
                response.protocol_version
            )));
        }
        if response.request_id != request_id {
            return Err(LocalError::Protocol(
                "runtime response request id does not match".to_owned(),
            ));
        }
        match response.outcome {
            AttachmentOutcome::Success(payload) => Ok(payload),
            AttachmentOutcome::Error(error) => Err(RemoteError(*error).into()),
        }
    }

    fn load_metadata(&self) -> Result<EndpointMetadata, LocalError> {
        let path = self.data_dir.join(METADATA_FILE);
        #[cfg(unix)]
        let encoded = {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};

            let file_metadata = fs::symlink_metadata(&path)
                .map_err(|error| LocalError::io("read endpoint metadata", error))?;
            if !file_metadata.file_type().is_file() {
                return Err(LocalError::Unavailable(
                    "endpoint metadata is not a regular file".to_owned(),
                ));
            }
            let expected_uid = rustix::process::geteuid().as_raw();
            let directory = fs::symlink_metadata(&self.data_dir)
                .map_err(|error| LocalError::io("inspect data directory", error))?;
            if !directory.file_type().is_dir()
                || directory.uid() != expected_uid
                || file_metadata.uid() != expected_uid
                || directory.permissions().mode() & 0o077 != 0
                || file_metadata.permissions().mode() & 0o077 != 0
            {
                return Err(LocalError::Unavailable(
                    "endpoint metadata is not owner-only".to_owned(),
                ));
            }
            fs::read(&path).map_err(|error| LocalError::io("read endpoint metadata", error))?
        };
        #[cfg(windows)]
        let encoded = {
            use std::io::Read as _;

            chat2db_local_ipc_windows::verify_owner_only_directory(&self.data_dir)
                .map_err(|error| LocalError::io("verify owner-only data directory", error))?;
            let mut file = chat2db_local_ipc_windows::open_owner_only_file(&path)
                .map_err(|error| LocalError::io("open owner-only endpoint metadata", error))?;
            let mut encoded = Vec::new();
            file.read_to_end(&mut encoded)
                .map_err(|error| LocalError::io("read owner-only endpoint metadata", error))?;
            encoded
        };
        let metadata: EndpointMetadata = serde_json::from_slice(&encoded)?;
        if metadata.protocol_version != PROTOCOL_VERSION {
            return Err(LocalError::Protocol(format!(
                "runtime metadata version {} does not match client version {PROTOCOL_VERSION}",
                metadata.protocol_version
            )));
        }
        let token = URL_SAFE_NO_PAD.decode(&metadata.token).map_err(|_| {
            LocalError::Protocol("endpoint token is not valid base64url".to_owned())
        })?;
        if token.len() != 32 {
            return Err(LocalError::Protocol(
                "endpoint token must contain exactly 32 bytes".to_owned(),
            ));
        }
        Ok(metadata)
    }
}

fn unexpected_payload() -> LocalError {
    LocalError::Protocol("runtime returned an unexpected payload type".to_owned())
}
