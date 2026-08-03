use std::fmt::{Display, Formatter};

use chat2db_agent::{AgentError, ConfigError, ExecutionOutcome, ProviderError};
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

impl From<ConfigError> for AppError {
    fn from(error: ConfigError) -> Self {
        match error {
            ConfigError::HttpClient(_) => Self::internal(),
            ConfigError::InvalidBaseUrl(_)
            | ConfigError::UnsupportedBaseUrlScheme
            | ConfigError::BaseUrlCredentials
            | ConfigError::BaseUrlHost
            | ConfigError::BaseUrlQuery
            | ConfigError::BaseUrlFragment
            | ConfigError::EmptyModel
            | ConfigError::EmptyApiKey
            | ConfigError::InvalidHeaderName(_)
            | ConfigError::InvalidHeaderValue(_)
            | ConfigError::ZeroTimeout
            | ConfigError::ZeroOutputTokens
            | ConfigError::InvalidContextBudget => Self::invalid(
                "invalid_provider_config",
                "The provider configuration is invalid",
            ),
        }
    }
}

impl From<ProviderError> for AppError {
    fn from(error: ProviderError) -> Self {
        match error {
            ProviderError::Cancelled => Self::new(
                AppErrorKind::Conflict,
                ApiError::new("agent_cancelled", "The agent run was cancelled"),
            ),
            ProviderError::Transport { .. }
            | ProviderError::HttpStatus {
                status: 408 | 429 | 500..=599,
                ..
            } => Self::new(
                AppErrorKind::Unavailable,
                ApiError::new(
                    "provider_unavailable",
                    "The AI provider is temporarily unavailable",
                ),
            ),
            ProviderError::HttpStatus {
                status: 400..=499, ..
            } => Self::invalid(
                "provider_rejected_request",
                "The AI provider rejected the request",
            ),
            ProviderError::HttpStatus { .. }
            | ProviderError::Remote { .. }
            | ProviderError::Protocol { .. } => Self::new(
                AppErrorKind::Unavailable,
                ApiError::new(
                    "provider_protocol_error",
                    "The AI provider response could not be processed",
                ),
            ),
            ProviderError::ResponseTooLarge { .. } => Self::new(
                AppErrorKind::ResourceExhausted,
                ApiError::new(
                    "agent_resource_limit_exceeded",
                    "The agent run exceeded a configured resource limit",
                ),
            ),
            ProviderError::Serialization { .. } => Self::internal(),
        }
    }
}

impl From<ExecutionOutcome> for AppError {
    fn from(outcome: ExecutionOutcome) -> Self {
        match outcome {
            ExecutionOutcome::Unknown => Self::new(
                AppErrorKind::Unavailable,
                ApiError::new(
                    "tool_outcome_unknown",
                    "The tool execution outcome is unknown and was not retried",
                ),
            ),
            ExecutionOutcome::NotStarted | ExecutionOutcome::Failed => Self::new(
                AppErrorKind::Unavailable,
                ApiError::new("agent_tool_failed", "The agent tool execution failed"),
            ),
        }
    }
}

impl From<AgentError> for AppError {
    fn from(error: AgentError) -> Self {
        match error {
            AgentError::Cancelled => Self::new(
                AppErrorKind::Conflict,
                ApiError::new("agent_cancelled", "The agent run was cancelled"),
            ),
            AgentError::DeadlineExceeded => Self::new(
                AppErrorKind::Unavailable,
                ApiError::new(
                    "agent_deadline_exceeded",
                    "The agent run deadline was exceeded",
                ),
            ),
            AgentError::Provider(source) => source.into(),
            AgentError::UnknownTool(_)
            | AgentError::DuplicateToolCall(_)
            | AgentError::InvalidToolArguments { .. }
            | AgentError::IncompleteProviderStream
            | AgentError::DuplicateProviderCompletion
            | AgentError::InconsistentProviderCompletion => Self::new(
                AppErrorKind::Unavailable,
                ApiError::new(
                    "provider_protocol_error",
                    "The AI provider response could not be processed",
                ),
            ),
            AgentError::ToolArgumentsTooLarge { .. }
            | AgentError::ModelTextTooLarge { .. }
            | AgentError::RoundToolLimit { .. }
            | AgentError::TotalToolLimit(_)
            | AgentError::ModelRoundLimit(_)
            | AgentError::ContextBudgetExceeded => Self::new(
                AppErrorKind::ResourceExhausted,
                ApiError::new(
                    "agent_resource_limit_exceeded",
                    "The agent run exceeded a configured resource limit",
                ),
            ),
            AgentError::Tool { source, .. } => source.outcome().into(),
            AgentError::InvalidInput(_) => Self::internal(),
        }
    }
}

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
            StorageError::WorkspaceNamespaceNotFound(id) => Self::not_found(
                "workspace_namespace_not_found",
                format!("Workspace namespace {id} does not exist"),
            ),
            StorageError::WorkspaceNodeNotFound(id) => Self::not_found(
                "workspace_node_not_found",
                format!("Workspace node {id} does not exist"),
            ),
            StorageError::InvalidWorkspace(message) => {
                Self::invalid("invalid_workspace_operation", message)
            }
            StorageError::CommunityDashboardNotFound(id) => Self::not_found(
                "community_dashboard_not_found",
                format!("Community dashboard {id} does not exist"),
            ),
            StorageError::InvalidCommunityDashboard(message) => {
                Self::invalid("invalid_community_dashboard", message)
            }
            StorageError::CommunityChartNotFound(id) => Self::not_found(
                "community_chart_not_found",
                format!("Community chart {id} does not exist"),
            ),
            StorageError::InvalidCommunityChart(message) => {
                Self::invalid("invalid_community_chart", message)
            }
            StorageError::TransferTaskNotFound(id) => Self::not_found(
                "transfer_task_not_found",
                format!("Transfer task {id} does not exist"),
            ),
            StorageError::TransferArtifactNotFound(id) => Self::not_found(
                "transfer_artifact_not_found",
                format!("Transfer artifact {id} does not exist or expired"),
            ),
            StorageError::InvalidTransfer(message) => {
                Self::invalid("invalid_transfer_operation", message)
            }
            StorageError::SavedConsoleNotFound(id) => Self::not_found(
                "saved_console_not_found",
                format!("Saved Console {id} does not exist"),
            ),
            StorageError::InvalidSavedConsole(message) => {
                Self::invalid("invalid_saved_console", message)
            }
            StorageError::InvalidOperationLog(message) => {
                Self::invalid("invalid_operation_log", message)
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
            | BridgeError::CommunityArtifact { .. }
            | BridgeError::DriverSnapshotDirectory { .. }
            | BridgeError::NonUtf8DriverArtifact(_)
            | BridgeError::StaleHandle(_)
            | BridgeError::Protocol(_)
            | BridgeError::UnsupportedVersion { .. }
            | BridgeError::MissingCapability(_)
            | BridgeError::InvalidHandshake(_)
            | BridgeError::UnexpectedResponse(_)
            | BridgeError::SupervisorTask(_)
            | BridgeError::ProcessCleanup { .. }
            | BridgeError::CleanupAfterFailure { .. }
            | BridgeError::Frame(_) => Self::internal(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chat2db_agent::{
        AgentError, ConfigError, ExecutionOutcome, ProviderError, ProviderKind, ToolExecutionError,
    };
    use chat2db_contract::ApiErrorDetails;
    use chat2db_java_bridge::BridgeError;
    use chat2db_storage::StorageError;

    use super::{AppError, AppErrorKind};

    const SENTINEL: &str = "PRIVATE_AGENT_ERROR_7c2d91";

    fn assert_mapping(error: StorageError, expected_kind: AppErrorKind, expected_code: &str) {
        let mapped = AppError::from(error);
        assert_eq!(mapped.kind(), expected_kind);
        assert_eq!(mapped.api_error().code, expected_code);
    }

    fn assert_agent_mapping(
        error: impl Into<AppError>,
        expected_kind: AppErrorKind,
        expected_code: &str,
        expected_retryable: bool,
    ) -> AppError {
        let mapped = error.into();
        assert_eq!(mapped.kind(), expected_kind);
        let api = mapped.api_error();
        assert_eq!(api.code, expected_code);
        assert_eq!(api.retryable, expected_retryable);
        mapped
    }

    fn assert_no_sentinel(mapped: &AppError) {
        let outputs = [
            mapped.to_string(),
            format!("{mapped:?}"),
            serde_json::to_string(&mapped.api_error()).expect("API error should serialize"),
        ];
        for output in outputs {
            assert!(
                !output.contains(SENTINEL),
                "mapped error exposed sensitive input: {output}"
            );
        }
    }

    #[test]
    fn provider_config_errors_use_safe_stable_categories() {
        for error in [
            ConfigError::EmptyApiKey,
            ConfigError::InvalidHeaderName(SENTINEL.to_owned()),
            ConfigError::InvalidHeaderValue(SENTINEL.to_owned()),
            ConfigError::InvalidContextBudget,
        ] {
            let mapped = assert_agent_mapping(
                error,
                AppErrorKind::InvalidRequest,
                "invalid_provider_config",
                false,
            );
            assert_no_sentinel(&mapped);
        }

        let mapped = assert_agent_mapping(
            ConfigError::HttpClient(SENTINEL.to_owned()),
            AppErrorKind::Internal,
            "internal_error",
            false,
        );
        assert_no_sentinel(&mapped);
    }

    #[test]
    fn community_artifact_errors_are_internal_and_hide_local_paths() {
        let mapped = AppError::from(BridgeError::CommunityArtifact {
            operation: "snapshot",
            path: SENTINEL.into(),
            source: std::io::Error::other(SENTINEL),
        });

        assert_eq!(mapped.kind(), AppErrorKind::Internal);
        assert_eq!(mapped.api_error().code, "internal_error");
        assert_no_sentinel(&mapped);
    }

    #[test]
    fn process_cleanup_errors_are_internal_and_hide_retained_snapshot_paths() {
        let errors = [
            BridgeError::ProcessCleanup {
                retained_snapshot: SENTINEL.into(),
                message: SENTINEL.to_owned(),
            },
            BridgeError::CleanupAfterFailure {
                primary: Box::new(BridgeError::ShutdownTimeout),
                cleanup: Box::new(BridgeError::ProcessCleanup {
                    retained_snapshot: SENTINEL.into(),
                    message: SENTINEL.to_owned(),
                }),
            },
        ];
        for error in errors {
            let mapped = AppError::from(error);
            assert_eq!(mapped.kind(), AppErrorKind::Internal);
            assert_eq!(mapped.api_error().code, "internal_error");
            assert_no_sentinel(&mapped);
        }
    }

    #[test]
    fn cancellations_are_non_retryable_conflicts() {
        for mapped in [
            assert_agent_mapping(
                AgentError::Cancelled,
                AppErrorKind::Conflict,
                "agent_cancelled",
                false,
            ),
            assert_agent_mapping(
                ProviderError::Cancelled,
                AppErrorKind::Conflict,
                "agent_cancelled",
                false,
            ),
        ] {
            assert_no_sentinel(&mapped);
        }
    }

    #[test]
    fn transient_provider_failures_and_deadline_do_not_make_the_run_retryable() {
        let mapped = assert_agent_mapping(
            ProviderError::Transport {
                provider: ProviderKind::OpenAi,
                message: SENTINEL.to_owned(),
            },
            AppErrorKind::Unavailable,
            "provider_unavailable",
            false,
        );
        assert_no_sentinel(&mapped);

        for status in [408, 429, 500, 503, 599] {
            assert_agent_mapping(
                ProviderError::HttpStatus {
                    provider: ProviderKind::OpenAi,
                    status,
                },
                AppErrorKind::Unavailable,
                "provider_unavailable",
                false,
            );
        }

        assert_agent_mapping(
            AgentError::DeadlineExceeded,
            AppErrorKind::Unavailable,
            "agent_deadline_exceeded",
            false,
        );
    }

    #[test]
    fn provider_client_errors_do_not_expose_the_status() {
        for status in [400, 401, 404, 422, 499] {
            let mapped = assert_agent_mapping(
                ProviderError::HttpStatus {
                    provider: ProviderKind::OpenAi,
                    status,
                },
                AppErrorKind::InvalidRequest,
                "provider_rejected_request",
                false,
            );
            let serialized =
                serde_json::to_string(&mapped.api_error()).expect("API error should serialize");
            assert!(!serialized.contains(&status.to_string()));
        }
    }

    #[test]
    fn provider_and_agent_limits_share_one_resource_category() {
        let provider_limit = ProviderError::ResponseTooLarge {
            provider: ProviderKind::OpenAi,
            limit: 7,
        };
        assert_agent_mapping(
            provider_limit,
            AppErrorKind::ResourceExhausted,
            "agent_resource_limit_exceeded",
            false,
        );

        for error in [
            AgentError::ToolArgumentsTooLarge {
                call_id: SENTINEL.to_owned(),
                limit: 7,
            },
            AgentError::ModelTextTooLarge { round: 8, limit: 7 },
            AgentError::RoundToolLimit { round: 8, limit: 7 },
            AgentError::TotalToolLimit(7),
            AgentError::ModelRoundLimit(7),
            AgentError::ContextBudgetExceeded,
        ] {
            let mapped = assert_agent_mapping(
                error,
                AppErrorKind::ResourceExhausted,
                "agent_resource_limit_exceeded",
                false,
            );
            assert_no_sentinel(&mapped);
        }
    }

    #[test]
    fn provider_protocol_failures_never_expose_remote_content() {
        for error in [
            AgentError::Provider(ProviderError::Remote {
                provider: ProviderKind::OpenAi,
                code: SENTINEL.to_owned(),
                message: SENTINEL.to_owned(),
            }),
            AgentError::Provider(ProviderError::Protocol {
                provider: ProviderKind::OpenAi,
                message: SENTINEL.to_owned(),
            }),
            AgentError::UnknownTool(SENTINEL.to_owned()),
            AgentError::DuplicateToolCall(SENTINEL.to_owned()),
            AgentError::InvalidToolArguments {
                call_id: SENTINEL.to_owned(),
                message: SENTINEL.to_owned(),
            },
            AgentError::IncompleteProviderStream,
            AgentError::DuplicateProviderCompletion,
            AgentError::InconsistentProviderCompletion,
        ] {
            let mapped = assert_agent_mapping(
                error,
                AppErrorKind::Unavailable,
                "provider_protocol_error",
                false,
            );
            assert_no_sentinel(&mapped);
        }

        assert_agent_mapping(
            ProviderError::HttpStatus {
                provider: ProviderKind::OpenAi,
                status: 302,
            },
            AppErrorKind::Unavailable,
            "provider_protocol_error",
            false,
        );
    }

    #[test]
    fn serialization_and_invalid_agent_input_are_internal() {
        for error in [
            AgentError::Provider(ProviderError::Serialization {
                provider: ProviderKind::OpenAi,
                message: SENTINEL.to_owned(),
            }),
            AgentError::InvalidInput(SENTINEL.to_owned()),
        ] {
            let mapped =
                assert_agent_mapping(error, AppErrorKind::Internal, "internal_error", false);
            assert_no_sentinel(&mapped);
        }
    }

    #[test]
    fn tool_outcomes_never_make_the_whole_run_retryable() {
        for outcome in [ExecutionOutcome::NotStarted, ExecutionOutcome::Failed] {
            assert_agent_mapping(
                outcome,
                AppErrorKind::Unavailable,
                "agent_tool_failed",
                false,
            );
        }
        assert_agent_mapping(
            ExecutionOutcome::Unknown,
            AppErrorKind::Unavailable,
            "tool_outcome_unknown",
            false,
        );

        for (outcome, expected_code, retryable) in [
            (ExecutionOutcome::NotStarted, "agent_tool_failed", false),
            (ExecutionOutcome::Failed, "agent_tool_failed", false),
            (ExecutionOutcome::Unknown, "tool_outcome_unknown", false),
        ] {
            let mapped = assert_agent_mapping(
                AgentError::Tool {
                    tool: SENTINEL.to_owned(),
                    source: ToolExecutionError::new(SENTINEL, SENTINEL, outcome),
                },
                AppErrorKind::Unavailable,
                expected_code,
                retryable,
            );
            assert_no_sentinel(&mapped);
        }
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
