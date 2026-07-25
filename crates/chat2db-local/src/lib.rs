//! Owner-only local attachment transport shared by CLI and MCP adapters.

mod client;
mod server;
mod transport;

use std::{fmt, io};

use chat2db_contract::{
    ApiError, CancelOperationResponse, DatasourceList, HealthResponse, OperationSnapshot,
    QueryAccepted, ResultPage, ResultPageRequest, StartQueryRequest,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use client::LocalClient;
pub use server::LocalServer;

const PROTOCOL_VERSION: u16 = 1;
const METADATA_FILE: &str = "local-attachment-v1.json";
const LOCK_FILE: &str = "local-attachment-v1.lock";
#[cfg(unix)]
const SOCKET_FILE: &str = "local-attachment-v1.sock";
const MAX_REQUEST_BYTES: usize = 256 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_LOCAL_PAGE_ROWS: u64 = 1_000;
const MAX_LOCAL_PAGE_BYTES: u64 = 512 * 1024;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EndpointMetadata {
    protocol_version: u16,
    endpoint: Endpoint,
    token: String,
    process_id: u32,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Endpoint {
    UnixSocket { path: std::path::PathBuf },
    WindowsNamedPipe { name: String },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AttachmentRequest {
    protocol_version: u16,
    request_id: String,
    token: String,
    command: AttachmentCommand,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
enum AttachmentCommand {
    Health,
    ListDatasources,
    StartReadQuery {
        request: StartQueryRequest,
    },
    OperationSnapshot {
        operation_id: String,
    },
    CancelOperation {
        operation_id: String,
    },
    ResultPage {
        result_id: String,
        request: ResultPageRequest,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AttachmentResponse {
    protocol_version: u16,
    request_id: String,
    outcome: AttachmentOutcome,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum AttachmentOutcome {
    Success(Box<AttachmentPayload>),
    Error(Box<ApiError>),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum AttachmentPayload {
    Health(Box<HealthResponse>),
    Datasources(Box<DatasourceList>),
    QueryAccepted(Box<QueryAccepted>),
    OperationSnapshot(Box<OperationSnapshot>),
    CancelOperation(Box<CancelOperationResponse>),
    ResultPage(Box<ResultPage>),
}

/// Safe error returned by the local attachment client or server lifecycle.
#[derive(Debug, Error)]
pub enum LocalError {
    #[error("local attachment is unavailable: {0}")]
    Unavailable(String),
    #[error("local attachment protocol error: {0}")]
    Protocol(String),
    #[error("local attachment timed out during {0}")]
    Timeout(&'static str),
    #[error("local attachment I/O failed during {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("local attachment JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Remote(Box<RemoteError>),
    #[error("local attachment task failed: {0}")]
    Task(String),
}

impl LocalError {
    fn io(operation: &'static str, source: io::Error) -> Self {
        Self::Io { operation, source }
    }
}

impl From<RemoteError> for LocalError {
    fn from(error: RemoteError) -> Self {
        Self::Remote(Box::new(error))
    }
}

/// Product error returned by the attached runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteError(pub ApiError);

impl fmt::Display for RemoteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.0.code, self.0.message)
    }
}

impl std::error::Error for RemoteError {}

fn validate_page_request(request: &ResultPageRequest) -> Result<u64, Box<ApiError>> {
    let max_rows = request.max_rows.parse::<u64>().map_err(|_| {
        Box::new(ApiError::new(
            "invalid_result_page",
            "maxRows must be an unsigned decimal integer",
        ))
    })?;
    let max_bytes = request.max_bytes.parse::<u64>().map_err(|_| {
        Box::new(ApiError::new(
            "invalid_result_page",
            "maxBytes must be an unsigned decimal integer",
        ))
    })?;
    if max_rows == 0 || max_rows > MAX_LOCAL_PAGE_ROWS {
        return Err(Box::new(ApiError::new(
            "invalid_result_page",
            format!("maxRows must be between 1 and {MAX_LOCAL_PAGE_ROWS}"),
        )));
    }
    if max_bytes == 0 || max_bytes > MAX_LOCAL_PAGE_BYTES {
        return Err(Box::new(ApiError::new(
            "invalid_result_page",
            format!("maxBytes must be between 1 and {MAX_LOCAL_PAGE_BYTES}"),
        )));
    }
    Ok(max_bytes)
}

fn bound_result_page(mut page: ResultPage, max_bytes: u64) -> Result<ResultPage, Box<ApiError>> {
    let original_rows = page.rows.len();
    let mut encoded_rows = 0_u64;
    let mut retained_rows = 0_usize;

    for row in &page.rows {
        let row_bytes = serde_json::to_vec(row).map_err(|_| {
            Box::new(ApiError::new(
                "local_response_encoding_failed",
                "The local result row could not be encoded",
            ))
        })?;
        let separator_bytes = u64::from(retained_rows > 0);
        let row_bytes = u64::try_from(row_bytes.len()).map_err(|_| {
            Box::new(ApiError::new(
                "local_response_too_large",
                "The local result row is too large for this platform",
            ))
        })?;
        let Some(next_size) = encoded_rows
            .checked_add(separator_bytes)
            .and_then(|size| size.checked_add(row_bytes))
        else {
            break;
        };
        if next_size > max_bytes {
            break;
        }
        encoded_rows = next_size;
        retained_rows += 1;
    }

    if retained_rows == 0 && original_rows > 0 {
        return Err(Box::new(ApiError::new(
            "local_result_row_too_large",
            "The next result row exceeds the requested local page byte budget",
        )));
    }
    if retained_rows < original_rows {
        page.rows.truncate(retained_rows);
        page.has_more = true;
    }
    Ok(page)
}

#[cfg(test)]
mod tests;
