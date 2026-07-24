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

    fn revision_conflict(
        code: &'static str,
        message: &'static str,
        expected: u64,
        actual: Option<u64>,
    ) -> Self {
        Self::new(
            AppErrorKind::Conflict,
            ApiError {
                code: code.to_owned(),
                message: message.to_owned(),
                retryable: false,
                details: Some(ApiErrorDetails::RevisionConflict {
                    expected_revision: expected.to_string(),
                    actual_revision: actual.map(|revision| revision.to_string()),
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
    #[allow(clippy::too_many_lines)]
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
            } => Self::revision_conflict(
                "revision_conflict",
                "The datasource changed before the update was applied",
                expected,
                actual,
            ),
            StorageError::InvalidDatasource(message) => {
                Self::invalid("invalid_datasource", message)
            }
            StorageError::ProviderNotFound(id) => Self::not_found(
                "provider_not_found",
                format!("Provider profile {id} does not exist"),
            ),
            StorageError::ProviderRevisionConflict {
                expected, actual, ..
            } => Self::revision_conflict(
                "provider_revision_conflict",
                "The provider profile changed before the update was applied",
                expected,
                actual,
            ),
            StorageError::InvalidProvider(message) => Self::invalid("invalid_provider", message),
            StorageError::ProviderInUse(_) => Self::new(
                AppErrorKind::Conflict,
                ApiError::new(
                    "provider_in_use",
                    "The provider profile is still selected by an agent session",
                ),
            ),
            StorageError::AgentSessionNotFound(id) => Self::not_found(
                "agent_session_not_found",
                format!("Agent session {id} does not exist"),
            ),
            StorageError::AgentSessionRevisionConflict {
                expected, actual, ..
            } => Self::revision_conflict(
                "agent_session_revision_conflict",
                "The agent session changed before the update was applied",
                expected,
                actual,
            ),
            StorageError::AgentSessionBusy(_) => Self::new(
                AppErrorKind::Conflict,
                ApiError::new(
                    "agent_session_busy",
                    "The agent session already has an active run",
                ),
            ),
            StorageError::AgentDependencyBusy { .. } => Self::new(
                AppErrorKind::Conflict,
                ApiError::new(
                    "agent_dependency_busy",
                    "The provider profile or datasource is bound to an active agent run",
                ),
            ),
            StorageError::AgentRunNotFound(id) => Self::not_found(
                "agent_run_not_found",
                format!("Agent run {id} does not exist"),
            ),
            StorageError::AgentStateConflict { .. } => Self::new(
                AppErrorKind::Conflict,
                ApiError::new(
                    "agent_state_conflict",
                    "The agent run changed before the operation was applied",
                ),
            ),
            StorageError::InvalidAgent(message) => Self::invalid("invalid_agent_request", message),
            StorageError::AgentQuotaExceeded { .. } => Self::new(
                AppErrorKind::ResourceExhausted,
                ApiError::new(
                    "agent_quota_exceeded",
                    "The agent session reached a configured resource limit",
                ),
            ),
            StorageError::PermissionNotFound(id) => Self::not_found(
                "tool_permission_not_found",
                format!("Tool permission {id} does not exist"),
            ),
            StorageError::PermissionRevisionConflict {
                expected, actual, ..
            } => Self::revision_conflict(
                "tool_permission_revision_conflict",
                "The tool permission changed before the decision was applied",
                expected,
                actual,
            ),
            StorageError::PermissionNotExecutable { .. } => Self::new(
                AppErrorKind::Conflict,
                ApiError::new(
                    "tool_permission_not_executable",
                    "The tool permission cannot authorize this execution",
                ),
            ),
            StorageError::ResultHandleNotFound(id) => Self::not_found(
                "agent_result_handle_not_found",
                format!("Agent result handle {id} does not exist or expired"),
            ),
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
                    "The credential vault is unavailable",
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

#[cfg(test)]
mod tests {
    use chat2db_contract::ApiErrorDetails;
    use chat2db_storage::StorageError;

    use super::{AppError, AppErrorKind};

    fn assert_mapping(error: StorageError, expected_kind: AppErrorKind, expected_code: &str) {
        let mapped = AppError::from(error);
        assert_eq!(mapped.kind(), expected_kind);
        assert_eq!(mapped.api_error().code, expected_code);
    }

    #[test]
    fn agent_storage_errors_have_stable_external_categories() {
        for (error, kind, code) in [
            (
                StorageError::ProviderNotFound("provider-1".to_owned()),
                AppErrorKind::NotFound,
                "provider_not_found",
            ),
            (
                StorageError::AgentSessionNotFound("session-1".to_owned()),
                AppErrorKind::NotFound,
                "agent_session_not_found",
            ),
            (
                StorageError::AgentRunNotFound("run-1".to_owned()),
                AppErrorKind::NotFound,
                "agent_run_not_found",
            ),
            (
                StorageError::PermissionNotFound("permission-1".to_owned()),
                AppErrorKind::NotFound,
                "tool_permission_not_found",
            ),
            (
                StorageError::ResultHandleNotFound("handle-1".to_owned()),
                AppErrorKind::NotFound,
                "agent_result_handle_not_found",
            ),
            (
                StorageError::InvalidProvider("invalid provider"),
                AppErrorKind::InvalidRequest,
                "invalid_provider",
            ),
            (
                StorageError::ProviderInUse("provider-1".to_owned()),
                AppErrorKind::Conflict,
                "provider_in_use",
            ),
            (
                StorageError::InvalidAgent("invalid agent"),
                AppErrorKind::InvalidRequest,
                "invalid_agent_request",
            ),
            (
                StorageError::AgentSessionBusy("session-1".to_owned()),
                AppErrorKind::Conflict,
                "agent_session_busy",
            ),
            (
                StorageError::AgentStateConflict {
                    id: "run-1".to_owned(),
                    expected: "running",
                    actual: "failed",
                },
                AppErrorKind::Conflict,
                "agent_state_conflict",
            ),
            (
                StorageError::PermissionNotExecutable {
                    id: "permission-1".to_owned(),
                    reason: "permission expired",
                },
                AppErrorKind::Conflict,
                "tool_permission_not_executable",
            ),
            (
                StorageError::AgentQuotaExceeded {
                    resource: "session message count",
                    limit: 1,
                },
                AppErrorKind::ResourceExhausted,
                "agent_quota_exceeded",
            ),
        ] {
            assert_mapping(error, kind, code);
        }
    }

    #[test]
    fn agent_dependency_busy_is_a_conflict_without_exposing_storage_details() {
        let mapped = AppError::from(StorageError::AgentDependencyBusy {
            resource: "provider profile",
            id: "provider-1".to_owned(),
        });

        assert_eq!(mapped.kind(), AppErrorKind::Conflict);
        let api = mapped.api_error();
        assert_eq!(api.code, "agent_dependency_busy");
        assert!(!api.message.contains("provider-1"));
        assert!(api.details.is_none());
    }

    #[test]
    fn agent_revision_conflicts_keep_portable_revision_details() {
        for (error, code) in [
            (
                StorageError::ProviderRevisionConflict {
                    id: "provider-1".to_owned(),
                    expected: 9_007_199_254_740_993,
                    actual: Some(9_007_199_254_740_994),
                },
                "provider_revision_conflict",
            ),
            (
                StorageError::AgentSessionRevisionConflict {
                    id: "session-1".to_owned(),
                    expected: 41,
                    actual: None,
                },
                "agent_session_revision_conflict",
            ),
            (
                StorageError::PermissionRevisionConflict {
                    id: "permission-1".to_owned(),
                    expected: 7,
                    actual: Some(8),
                },
                "tool_permission_revision_conflict",
            ),
        ] {
            let mapped = AppError::from(error);
            assert_eq!(mapped.kind(), AppErrorKind::Conflict);
            let api = mapped.api_error();
            assert_eq!(api.code, code);
            assert!(matches!(
                api.details,
                Some(ApiErrorDetails::RevisionConflict { .. })
            ));
        }
    }
}
