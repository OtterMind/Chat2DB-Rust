use std::{path::PathBuf, time::Duration};

#[cfg(unix)]
use std::fs;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chat2db_contract::{
    ApiError, CancelOperationResponse, DatabaseWriteResult, DatabaseWriteState, DatasourceList,
    ExecuteDatabaseWriteRequest, HealthResponse, OperationSnapshot, QueryAccepted, ResultPage,
    ResultPageRequest, StartQueryRequest,
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

    /// Executes one explicitly confirmed database write in the attached runtime.
    ///
    /// Transport failures after request delivery are returned as an `unknown`
    /// write outcome so callers never retry a potentially committed statement.
    pub async fn execute_database_write(
        &self,
        request: ExecuteDatabaseWriteRequest,
    ) -> DatabaseWriteResult {
        let probe = AttachmentRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: "0".repeat(64),
            token: "0".repeat(43),
            command: AttachmentCommand::ExecuteDatabaseWrite { request },
        };
        if transport::encode_message(&probe, MAX_REQUEST_BYTES).is_err() {
            return write_failure(
                DatabaseWriteState::NotStarted,
                ApiError::new(
                    "invalid_database_write",
                    "The database write request exceeds the local transport limit",
                ),
            );
        }
        match self.call(probe.command).await {
            Ok(payload) => match *payload {
                AttachmentPayload::DatabaseWrite(value) => *value,
                _ => unknown_write_result(),
            },
            Err(error) => local_write_failure(error),
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

fn local_write_failure(error: LocalError) -> DatabaseWriteResult {
    match error {
        LocalError::Remote(error) if remote_rejected_before_dispatch(&error.0.code) => {
            write_failure(DatabaseWriteState::NotStarted, error.0)
        }
        LocalError::Unavailable(_) => write_failure(
            DatabaseWriteState::NotStarted,
            retryable_error(
                "local_runtime_unavailable",
                "The Chat2DB local runtime is unavailable",
            ),
        ),
        LocalError::Timeout("connect") => write_failure(
            DatabaseWriteState::NotStarted,
            retryable_error(
                "local_runtime_timeout",
                "The Chat2DB local runtime could not be reached in time",
            ),
        ),
        LocalError::Io { operation, .. } if write_was_not_dispatched(operation) => write_failure(
            DatabaseWriteState::NotStarted,
            retryable_error(
                "local_runtime_io_error",
                "The Chat2DB local runtime could not be reached",
            ),
        ),
        LocalError::Remote(_)
        | LocalError::Timeout(_)
        | LocalError::Io { .. }
        | LocalError::Protocol(_)
        | LocalError::Json(_)
        | LocalError::Task(_) => unknown_write_result(),
    }
}

fn remote_rejected_before_dispatch(code: &str) -> bool {
    matches!(
        code,
        "local_protocol_version_mismatch"
            | "invalid_local_request"
            | "local_attachment_unauthorized"
    )
}

fn write_was_not_dispatched(operation: &str) -> bool {
    operation.contains("metadata")
        || operation.contains("data directory")
        || operation.contains("connect")
        || operation.contains("socket")
        || operation.contains("named pipe")
}

fn unknown_write_result() -> DatabaseWriteResult {
    write_failure(
        DatabaseWriteState::Unknown,
        ApiError::new(
            "database_write_outcome_unknown",
            "The database write outcome is unknown; do not retry it blindly",
        ),
    )
}

fn write_failure(state: DatabaseWriteState, error: ApiError) -> DatabaseWriteResult {
    DatabaseWriteResult {
        state,
        affected_rows: None,
        error: Some(error),
    }
}

fn retryable_error(code: &'static str, message: &'static str) -> ApiError {
    let mut error = ApiError::new(code, message);
    error.retryable = true;
    error
}

#[cfg(test)]
mod tests {
    use chat2db_contract::{ApiError, DatabaseWriteState, ExecuteDatabaseWriteRequest};

    use super::{LocalClient, LocalError, MAX_REQUEST_BYTES, RemoteError, local_write_failure};

    #[test]
    fn write_transport_timeout_distinguishes_before_and_after_dispatch() {
        let before_dispatch = local_write_failure(LocalError::Timeout("connect"));
        assert_eq!(before_dispatch.state, DatabaseWriteState::NotStarted);
        assert_eq!(
            before_dispatch
                .error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some("local_runtime_timeout")
        );

        let after_dispatch = local_write_failure(LocalError::Timeout("response read"));
        assert_eq!(after_dispatch.state, DatabaseWriteState::Unknown);
        assert_eq!(
            after_dispatch
                .error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some("database_write_outcome_unknown")
        );
        assert!(!after_dispatch.error.unwrap().retryable);
    }

    #[test]
    fn only_known_local_rejections_are_classified_as_not_started() {
        let unauthorized =
            local_write_failure(LocalError::Remote(Box::new(RemoteError(ApiError::new(
                "local_attachment_unauthorized",
                "Local attachment authentication failed",
            )))));
        assert_eq!(unauthorized.state, DatabaseWriteState::NotStarted);

        let oversized_response =
            local_write_failure(LocalError::Remote(Box::new(RemoteError(ApiError::new(
                "local_response_too_large",
                "The local response exceeds the maximum transport frame",
            )))));
        assert_eq!(oversized_response.state, DatabaseWriteState::Unknown);
    }

    #[tokio::test]
    async fn oversized_write_is_rejected_before_local_delivery() {
        let result = LocalClient::new("missing-runtime")
            .execute_database_write(ExecuteDatabaseWriteRequest {
                datasource_id: "datasource-1".to_owned(),
                sql: format!(
                    "UPDATE items SET label = '{}';",
                    "x".repeat(MAX_REQUEST_BYTES)
                ),
                confirmed: true,
            })
            .await;
        assert_eq!(result.state, DatabaseWriteState::NotStarted);
        assert_eq!(
            result.error.as_ref().map(|error| error.code.as_str()),
            Some("invalid_database_write")
        );
    }
}
