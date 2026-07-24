use std::fmt::{Display, Formatter};

use chat2db_contract::{ApiError, ApiErrorDetails};
use chat2db_engine_protocol::wire;
use chat2db_java_bridge::{BridgeError, DeliveryOutcome};
use chat2db_storage::StorageError;

/// Transport-neutral classification used to select an HTTP status or IPC code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppErrorKind {
    InvalidRequest,
    NotFound,
    Conflict,
    ResourceExhausted,
    Unavailable,
    Internal,
}

/// Safe application failure shared by every delivery adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    kind: AppErrorKind,
    api: Box<ApiError>,
}

impl AppError {
    #[must_use]
    pub fn new(kind: AppErrorKind, api: ApiError) -> Self {
        Self {
            kind,
            api: Box::new(api),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> AppErrorKind {
        self.kind
    }

    #[must_use]
    pub fn api_error(&self) -> ApiError {
        self.api.as_ref().clone()
    }

    #[must_use]
    pub fn invalid(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(AppErrorKind::InvalidRequest, ApiError::new(code, message))
    }

    #[must_use]
    pub fn not_found(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(AppErrorKind::NotFound, ApiError::new(code, message))
    }

    #[must_use]
    pub fn unavailable(code: impl Into<String>, message: impl Into<String>) -> Self {
        let mut api = ApiError::new(code, message);
        api.retryable = true;
        Self::new(AppErrorKind::Unavailable, api)
    }

    #[must_use]
    pub fn internal() -> Self {
        Self::new(
            AppErrorKind::Internal,
            ApiError::new("internal_error", "The operation could not be completed"),
        )
    }

    #[must_use]
    pub(crate) fn replay_window(requested: u64, oldest: u64, latest: u64) -> Self {
        Self::new(
            AppErrorKind::Conflict,
            ApiError {
                code: "operation_replay_window_expired".to_owned(),
                message: "The requested operation event is no longer retained".to_owned(),
                retryable: false,
                details: Some(ApiErrorDetails::ReplayWindow {
                    requested_sequence: requested.to_string(),
                    oldest_available_sequence: oldest.to_string(),
                    latest_sequence: latest.to_string(),
                }),
            },
        )
    }
}

impl Display for AppError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.api.code, self.api.message)
    }
}

impl std::error::Error for AppError {}

impl From<StorageError> for AppError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::DatasourceNotFound(id) => Self::not_found(
                "datasource_not_found",
                format!("Datasource {id} does not exist"),
            ),
            StorageError::ResultNotFound(id) => Self::not_found(
                "result_not_found",
                format!("Result {id} does not exist or expired"),
            ),
            StorageError::RevisionConflict {
                expected, actual, ..
            } => Self::new(
                AppErrorKind::Conflict,
                ApiError {
                    code: "revision_conflict".to_owned(),
                    message: "The datasource changed before the update was applied".to_owned(),
                    retryable: false,
                    details: Some(ApiErrorDetails::RevisionConflict {
                        expected_revision: expected.to_string(),
                        actual_revision: actual.map(|revision| revision.to_string()),
                    }),
                },
            ),
            StorageError::InvalidDatasource(message) => {
                Self::invalid("invalid_datasource", message)
            }
            StorageError::InvalidResult(message) => {
                Self::invalid("invalid_result_request", message)
            }
            StorageError::QuotaExceeded { .. } => Self::new(
                AppErrorKind::ResourceExhausted,
                ApiError::new(
                    "result_storage_quota_exceeded",
                    "The retained-result storage quota is exhausted",
                ),
            ),
            StorageError::SecretVault { .. } | StorageError::SecretCompensation { .. } => {
                Self::unavailable(
                    "secret_vault_unavailable",
                    "The datasource secret vault is unavailable",
                )
            }
            StorageError::OutcomeUnknown { .. } => Self::new(
                AppErrorKind::Unavailable,
                ApiError::new(
                    "storage_outcome_unknown",
                    "The durable storage outcome is unknown and was not retried",
                ),
            ),
            StorageError::AlreadyOpen(_) => Self::new(
                AppErrorKind::Conflict,
                ApiError::new(
                    "storage_already_open",
                    "Another Chat2DB process owns the selected data directory",
                ),
            ),
            StorageError::DataDirectoryUnavailable => Self::unavailable(
                "data_directory_unavailable",
                "The operating system did not provide an application data directory",
            ),
            StorageError::UnsupportedSchema { .. }
            | StorageError::UnsupportedResultFormat { .. }
            | StorageError::Integrity(_)
            | StorageError::CorruptResult { .. }
            | StorageError::Io { .. }
            | StorageError::Sqlite(_)
            | StorageError::NumericRange(_) => Self::internal(),
        }
    }
}

impl From<BridgeError> for AppError {
    fn from(error: BridgeError) -> Self {
        match error {
            BridgeError::InvalidRequest(message) => {
                Self::invalid("invalid_database_request", message)
            }
            BridgeError::Remote(remote) => {
                let details =
                    remote
                        .database_error
                        .as_ref()
                        .map(|database| ApiErrorDetails::Database {
                            sql_state: database.sql_state.clone(),
                            vendor_code: database.vendor_code,
                            constraint_name: database.constraint_name.clone(),
                            statement_position: database.statement_position,
                        });
                let kind = match remote.category {
                    wire::ErrorCategory::Validation => AppErrorKind::InvalidRequest,
                    wire::ErrorCategory::Cancelled => AppErrorKind::Conflict,
                    wire::ErrorCategory::Deadline | wire::ErrorCategory::Unavailable => {
                        AppErrorKind::Unavailable
                    }
                    wire::ErrorCategory::Protocol
                    | wire::ErrorCategory::Database
                    | wire::ErrorCategory::Internal
                    | wire::ErrorCategory::Unspecified => AppErrorKind::Internal,
                };
                let message = if remote.category == wire::ErrorCategory::Database {
                    "The database rejected the operation"
                } else if remote.category == wire::ErrorCategory::Cancelled {
                    "The database operation was cancelled"
                } else {
                    "The compatibility engine rejected the operation"
                };
                Self::new(
                    kind,
                    ApiError {
                        code: remote.code.clone(),
                        message: message.to_owned(),
                        retryable: remote.retryable,
                        details,
                    },
                )
            }
            BridgeError::NotReady { .. }
            | BridgeError::StartupTimeout
            | BridgeError::ShutdownTimeout
            | BridgeError::ProcessUnavailable { .. }
            | BridgeError::RequestTimeout { .. }
            | BridgeError::CommandChannelClosed { .. } => {
                let unknown = matches!(
                    error,
                    BridgeError::ProcessUnavailable {
                        outcome: DeliveryOutcome::Unknown,
                        ..
                    } | BridgeError::RequestTimeout {
                        outcome: DeliveryOutcome::Unknown,
                        ..
                    } | BridgeError::CommandChannelClosed {
                        outcome: DeliveryOutcome::Unknown
                    }
                );
                if unknown {
                    Self::new(
                        AppErrorKind::Unavailable,
                        ApiError::new(
                            "database_outcome_unknown",
                            "The database operation outcome is unknown and was not retried",
                        ),
                    )
                } else {
                    Self::unavailable(
                        "database_engine_unavailable",
                        "The database compatibility engine is unavailable",
                    )
                }
            }
            BridgeError::InvalidConfig(_)
            | BridgeError::Spawn(_)
            | BridgeError::MissingPipe(_)
            | BridgeError::DriverArtifact { .. }
            | BridgeError::NonUtf8DriverArtifact(_)
            | BridgeError::StaleHandle(_)
            | BridgeError::Protocol(_)
            | BridgeError::UnsupportedVersion { .. }
            | BridgeError::MissingCapability(_)
            | BridgeError::InvalidHandshake(_)
            | BridgeError::UnexpectedResponse(_)
            | BridgeError::SupervisorTask(_)
            | BridgeError::Frame(_) => Self::internal(),
        }
    }
}
