use std::fmt::{Debug, Formatter};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{ApiError, CancelDisposition, ResultColumn, ResultRow};

/// Provider wire protocol selected for one model profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// `OpenAI` Chat Completions or a compatible endpoint.
    OpenAiCompatible,
    /// Anthropic Messages API.
    Anthropic,
    /// Google Gemini generate-content API.
    Gemini,
}

/// Provider credentials accepted only at a secret-handling boundary.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCredentials {
    /// Provider API key.
    #[schema(write_only)]
    pub api_key: String,
}

impl Debug for ProviderCredentials {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderCredentials")
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

/// Request to create one model-provider profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateProviderProfileRequest {
    /// User-visible profile name.
    pub name: String,
    /// Provider protocol.
    pub kind: ProviderKind,
    /// Provider API root URL.
    pub base_url: String,
    /// Provider model identifier.
    pub model: String,
    /// Context window encoded as a decimal token count.
    pub context_window_tokens: String,
    /// Maximum generated output encoded as a decimal token count.
    pub max_output_tokens: String,
    /// Credentials to install in the secret vault.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials: Option<ProviderCredentials>,
}

/// Explicit credential mutation for a provider-profile update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ProviderSecretChange {
    /// Retain the current immutable vault value.
    Keep,
    /// Remove the current credential.
    Clear,
    /// Replace the current credential with a newly staged value.
    Replace {
        /// Complete replacement credential.
        credentials: ProviderCredentials,
    },
}

/// Optimistic provider-profile replacement request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProviderProfileRequest {
    /// Expected revision encoded as a decimal integer.
    pub expected_revision: String,
    /// User-visible profile name.
    pub name: String,
    /// Provider protocol.
    pub kind: ProviderKind,
    /// Provider API root URL.
    pub base_url: String,
    /// Provider model identifier.
    pub model: String,
    /// Context window encoded as a decimal token count.
    pub context_window_tokens: String,
    /// Maximum generated output encoded as a decimal token count.
    pub max_output_tokens: String,
    /// Explicit keep, clear, or replace action for the credential.
    pub secret_change: ProviderSecretChange,
}

/// Secret-free provider profile returned to external callers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfile {
    /// Opaque profile id.
    pub id: String,
    /// User-visible profile name.
    pub name: String,
    /// Provider protocol.
    pub kind: ProviderKind,
    /// Provider API root URL.
    pub base_url: String,
    /// Provider model identifier.
    pub model: String,
    /// Context window encoded as a decimal token count.
    pub context_window_tokens: String,
    /// Maximum generated output encoded as a decimal token count.
    pub max_output_tokens: String,
    /// Whether the profile has a credential in the vault.
    pub has_secret: bool,
    /// Monotonic revision encoded as a decimal integer.
    pub revision: String,
    /// Creation time as Unix epoch milliseconds encoded as a decimal integer.
    pub created_at_ms: String,
    /// Last update time as Unix epoch milliseconds encoded as a decimal integer.
    pub updated_at_ms: String,
}

/// Stable provider-profile collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfileList {
    /// Profiles in stable creation order.
    pub items: Vec<ProviderProfile>,
}

/// SQL capability available to one agent run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SqlPermissionMode {
    /// Only the JDBC read-only tool exists for the run.
    #[default]
    ReadOnly,
    /// Writes may be requested but require one explicit approval each.
    AskBeforeWrite,
}

/// Request to create a durable conversation session.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentSessionRequest {
    /// User-visible session title.
    pub title: String,
    /// Provider profile used by the session.
    pub provider_id: String,
    /// Optional datasource available to database tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datasource_id: Option<String>,
    /// Optional bounded system instruction persisted as the first message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
}

/// Optimistic replacement of mutable session settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAgentSessionRequest {
    /// Expected revision encoded as a decimal integer.
    pub expected_revision: String,
    /// User-visible session title.
    pub title: String,
    /// Provider profile used by later runs.
    pub provider_id: String,
    /// Optional datasource available to later database tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datasource_id: Option<String>,
}

/// Secret-free durable conversation session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    /// Opaque session id.
    pub id: String,
    /// User-visible title.
    pub title: String,
    /// Active provider profile id.
    pub provider_id: String,
    /// Optional datasource id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datasource_id: Option<String>,
    /// Monotonic revision encoded as a decimal integer.
    pub revision: String,
    /// Creation time as Unix epoch milliseconds encoded as a decimal integer.
    pub created_at_ms: String,
    /// Last update time as Unix epoch milliseconds encoded as a decimal integer.
    pub updated_at_ms: String,
}

/// Stable conversation-session collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionList {
    /// Sessions in reverse update order.
    pub items: Vec<AgentSession>,
}

/// Canonical role persisted for one conversation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentMessageRole {
    /// System instruction.
    System,
    /// User input.
    User,
    /// Assistant output or tool request.
    Assistant,
    /// Tool result paired with an assistant request.
    Tool,
    /// Deterministic or model-generated context summary.
    Summary,
}

/// One provider-neutral tool call.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolCall {
    /// Provider-neutral call id.
    pub id: String,
    /// Registered tool name.
    pub name: String,
    /// Canonical JSON arguments.
    pub arguments_json: String,
}

/// Bounded handle exposed instead of an unbounded database result.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentResultHandle {
    /// Opaque handle id scoped to one session and run.
    pub handle_id: String,
    /// Retained row count encoded as a decimal integer.
    pub row_count: String,
    /// Retained encoded byte count encoded as a decimal integer.
    pub byte_count: String,
    /// Whether another row existed beyond the configured row limit.
    pub truncated_by_max_rows: bool,
    /// Whether another row exceeded the configured result-byte limit.
    pub truncated_by_max_result_bytes: bool,
    /// Handle creation time as Unix epoch milliseconds encoded as a decimal integer.
    pub created_at_ms: String,
    /// Handle expiry time as Unix epoch milliseconds encoded as a decimal integer.
    pub expires_at_ms: String,
    /// Result schema.
    pub columns: Vec<ResultColumn>,
    /// Bounded sample rows supplied to the model.
    pub sample_rows: Vec<ResultRow>,
    /// Whether retained rows exist outside the sample.
    pub sample_truncated: bool,
}

/// Bounded provider-neutral tool output.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentToolOutput {
    /// Bounded UTF-8 content.
    Text {
        /// Tool content.
        content: String,
        /// Whether the tool omitted additional content.
        truncated: bool,
    },
    /// Retained database result plus bounded sample.
    Result {
        /// Durable result handle.
        handle: Box<AgentResultHandle>,
    },
}

/// Provider-neutral canonical message content.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessageContent {
    /// Plain bounded text.
    Text {
        /// Text content.
        text: String,
    },
    /// Assistant tool requests.
    ToolCalls {
        /// Ordered calls emitted in one assistant turn.
        calls: Vec<AgentToolCall>,
    },
    /// One tool result paired by call id.
    ToolResult {
        /// Matching tool-call id.
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        /// Registered tool name.
        name: String,
        /// Bounded output.
        output: AgentToolOutput,
    },
}

/// One durable canonical conversation message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    /// Opaque message id.
    pub id: String,
    /// Owning session id.
    pub session_id: String,
    /// Run that produced the message, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Canonical role.
    pub role: AgentMessageRole,
    /// Ordered provider-neutral content.
    pub content: Vec<AgentMessageContent>,
    /// Stable session ordinal encoded as a decimal integer.
    pub ordinal: String,
    /// Creation time as Unix epoch milliseconds encoded as a decimal integer.
    pub created_at_ms: String,
}

/// Bounded page of canonical session messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessageList {
    /// Returned messages.
    pub items: Vec<AgentMessage>,
    /// Whether a higher-ordinal message exists after this forward page.
    pub has_more: bool,
}

/// Request to start one bounded agent run in an existing session.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartAgentRunRequest {
    /// Existing conversation session id.
    pub session_id: String,
    /// New user message.
    pub message: String,
    /// SQL permission policy for this run.
    #[serde(default)]
    pub sql_permission_mode: SqlPermissionMode,
}

/// Immediate acknowledgement for one accepted agent run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunAccepted {
    /// Opaque run id.
    pub run_id: String,
    /// Owning session id.
    pub session_id: String,
}

/// Materialized agent-run lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    /// The bounded loop is active.
    Running,
    /// Execution is paused for one explicit permission decision.
    WaitingForPermission,
    /// The assistant completed successfully.
    Completed,
    /// The run failed.
    Failed,
    /// The run was cancelled.
    Cancelled,
}

/// Provider usage accumulated by one run.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsage {
    /// Input tokens encoded as a decimal integer.
    pub input_tokens: String,
    /// Output tokens encoded as a decimal integer.
    pub output_tokens: String,
    /// Total tokens encoded as a decimal integer.
    pub total_tokens: String,
}

/// One permission that cannot be widened or reused by the model.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentPermissionRequest {
    /// Opaque permission id.
    pub permission_id: String,
    /// Owning run id.
    pub run_id: String,
    /// Exact tool-call id.
    pub tool_call_id: String,
    /// Requested tool name.
    pub tool_name: String,
    /// Lowercase hexadecimal SHA-256 of the canonical arguments.
    pub arguments_sha256: String,
    /// Bounded user-visible operation summary.
    pub summary: String,
    /// Request time as Unix epoch milliseconds encoded as a decimal integer.
    pub requested_at_ms: String,
    /// Expiry time as Unix epoch milliseconds encoded as a decimal integer.
    pub expires_at_ms: String,
}

/// User decision for one pending permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentPermissionDecision {
    /// Approve exactly one execution with the bound arguments.
    AllowOnce,
    /// Reject the requested execution.
    Deny,
}

/// Permission lifecycle state returned to clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentPermissionStatus {
    /// Awaiting a user decision.
    Pending,
    /// Approved but not yet consumed by the exact tool call.
    Approved,
    /// Rejected by the user.
    Denied,
    /// Consumed by one matching execution.
    Consumed,
    /// Expired before use.
    Expired,
    /// Revoked by cancellation or terminal cleanup.
    Revoked,
}

/// Request body used to resolve a pending permission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecideAgentPermissionRequest {
    /// Owning run id displayed with the permission request.
    pub run_id: String,
    /// Exact tool-call id displayed with the permission request.
    pub tool_call_id: String,
    /// User decision.
    pub decision: AgentPermissionDecision,
    /// Digest displayed to the user and bound to the decision.
    pub arguments_sha256: String,
}

/// Result of resolving one permission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentPermissionResponse {
    /// Opaque permission id.
    pub permission_id: String,
    /// Current permission state.
    pub status: AgentPermissionStatus,
}

/// Reason one context compaction was performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContextCompactionStrategy {
    /// A bounded provider summary replaced complete older turns.
    Summary,
    /// Summary failed and deterministic complete-turn trimming was used.
    DeterministicTrim,
}

/// One replayable agent event.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// The run entered the bounded loop.
    Started,
    /// Bounded assistant text increment.
    TextDelta {
        /// Text increment.
        delta: String,
    },
    /// A validated tool call started.
    ToolStarted {
        /// Tool-call id.
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        /// Registered tool name.
        name: String,
        /// Canonical argument digest.
        #[serde(rename = "argumentsSha256")]
        arguments_sha256: String,
    },
    /// A tool returned bounded output.
    ToolCompleted {
        /// Tool-call id.
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        /// Registered tool name.
        name: String,
        /// Bounded output.
        output: AgentToolOutput,
    },
    /// A tool failed without terminating through a provider retry.
    ToolFailed {
        /// Tool-call id.
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        /// Registered tool name.
        name: String,
        /// Safe failure.
        error: ApiError,
    },
    /// The run paused for an external permission decision.
    PermissionRequested {
        /// Exact pending permission.
        permission: AgentPermissionRequest,
    },
    /// A permission decision was recorded.
    PermissionResolved {
        /// Opaque permission id.
        #[serde(rename = "permissionId")]
        permission_id: String,
        /// Resulting state.
        status: AgentPermissionStatus,
    },
    /// Complete older turns were compacted.
    ContextCompacted {
        /// Compaction fallback used.
        strategy: ContextCompactionStrategy,
        /// Removed turn count encoded as a decimal integer.
        #[serde(rename = "droppedTurns")]
        dropped_turns: String,
    },
    /// Provider usage advanced.
    Usage {
        /// Accumulated usage.
        usage: AgentUsage,
    },
    /// The assistant message became durable.
    Completed {
        /// Durable assistant message id.
        #[serde(rename = "messageId")]
        message_id: String,
    },
    /// The run failed.
    Failed {
        /// Safe terminal error.
        error: ApiError,
    },
    /// The run was cancelled.
    Cancelled {
        /// Optional safe cancellation reason.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

/// One replayable event with a monotonically increasing per-run sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentEventEnvelope {
    /// Opaque run id.
    pub run_id: String,
    /// Monotonic sequence encoded as a decimal integer.
    pub sequence: String,
    /// Event creation time as Unix epoch milliseconds encoded as a decimal integer.
    pub occurred_at_ms: String,
    /// Typed run event.
    pub event: AgentEvent,
}

/// One explicit desktop/local-RPC stream outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentStreamMessage {
    /// One replayable run event.
    Event {
        /// Event delivered by the run journal.
        event: AgentEventEnvelope,
    },
    /// The observer failed.
    Error {
        /// Safe external error.
        error: ApiError,
    },
    /// The observer reached a clean end.
    End,
}

/// Immediate acknowledgement for an established run observer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentSubscriptionAccepted {
    /// Opaque observer id used only to release this subscription.
    pub subscription_id: String,
}

/// Materialized state for reconnect and polling clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunSnapshot {
    /// Opaque run id.
    pub run_id: String,
    /// Owning session id.
    pub session_id: String,
    /// Current lifecycle state.
    pub status: AgentRunStatus,
    /// Latest event sequence encoded as a decimal integer.
    pub last_sequence: String,
    /// Run start time as Unix epoch milliseconds encoded as a decimal integer.
    pub started_at_ms: String,
    /// Latest state-change time as Unix epoch milliseconds encoded as a decimal integer.
    pub updated_at_ms: String,
    /// Completed model round count encoded as a decimal integer.
    pub model_rounds: String,
    /// Started tool-call count encoded as a decimal integer.
    pub tool_calls: String,
    /// Accumulated provider usage.
    pub usage: AgentUsage,
    /// Current pending permission, if execution is paused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_permission: Option<AgentPermissionRequest>,
    /// Durable final assistant message id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Terminal failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}

/// Result of an idempotent agent-run cancellation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelAgentRunResponse {
    /// Opaque run id supplied by the caller.
    pub run_id: String,
    /// Idempotent cancellation disposition.
    pub disposition: CancelDisposition,
}

impl Debug for CreateAgentSessionRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CreateAgentSessionRequest")
            .field("title_bytes", &self.title.len())
            .field("provider_id", &self.provider_id)
            .field("datasource_id", &self.datasource_id)
            .field(
                "system_prompt_bytes",
                &self.system_prompt.as_ref().map_or(0, String::len),
            )
            .finish()
    }
}

impl Debug for AgentToolCall {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentToolCall")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("arguments_json_bytes", &self.arguments_json.len())
            .finish()
    }
}

impl Debug for AgentResultHandle {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentResultHandle")
            .field("handle_id", &self.handle_id)
            .field("row_count", &self.row_count)
            .field("byte_count", &self.byte_count)
            .field("truncated_by_max_rows", &self.truncated_by_max_rows)
            .field(
                "truncated_by_max_result_bytes",
                &self.truncated_by_max_result_bytes,
            )
            .field("created_at_ms", &self.created_at_ms)
            .field("expires_at_ms", &self.expires_at_ms)
            .field("column_count", &self.columns.len())
            .field("sample_row_count", &self.sample_rows.len())
            .field("sample_truncated", &self.sample_truncated)
            .finish()
    }
}

impl Debug for AgentToolOutput {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text { content, truncated } => formatter
                .debug_struct("Text")
                .field("content_bytes", &content.len())
                .field("truncated", truncated)
                .finish(),
            Self::Result { handle } => formatter.debug_tuple("Result").field(handle).finish(),
        }
    }
}

impl Debug for AgentMessageContent {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text { text } => formatter
                .debug_struct("Text")
                .field("text_bytes", &text.len())
                .finish(),
            Self::ToolCalls { calls } => formatter
                .debug_struct("ToolCalls")
                .field("calls", calls)
                .finish(),
            Self::ToolResult {
                tool_call_id,
                name,
                output,
            } => formatter
                .debug_struct("ToolResult")
                .field("tool_call_id", tool_call_id)
                .field("name", name)
                .field("output", output)
                .finish(),
        }
    }
}

impl Debug for StartAgentRunRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StartAgentRunRequest")
            .field("session_id", &self.session_id)
            .field("message_bytes", &self.message.len())
            .field("sql_permission_mode", &self.sql_permission_mode)
            .finish()
    }
}

impl Debug for AgentPermissionRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentPermissionRequest")
            .field("permission_id", &self.permission_id)
            .field("run_id", &self.run_id)
            .field("tool_call_id", &self.tool_call_id)
            .field("tool_name", &self.tool_name)
            .field("arguments_sha256", &self.arguments_sha256)
            .field("summary_bytes", &self.summary.len())
            .field("requested_at_ms", &self.requested_at_ms)
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

impl Debug for AgentEvent {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Started => formatter.write_str("Started"),
            Self::TextDelta { delta } => formatter
                .debug_struct("TextDelta")
                .field("delta_bytes", &delta.len())
                .finish(),
            Self::ToolStarted {
                tool_call_id,
                name,
                arguments_sha256,
            } => formatter
                .debug_struct("ToolStarted")
                .field("tool_call_id", tool_call_id)
                .field("name", name)
                .field("arguments_sha256", arguments_sha256)
                .finish(),
            Self::ToolCompleted {
                tool_call_id,
                name,
                output,
            } => formatter
                .debug_struct("ToolCompleted")
                .field("tool_call_id", tool_call_id)
                .field("name", name)
                .field("output", output)
                .finish(),
            Self::ToolFailed {
                tool_call_id,
                name,
                error,
            } => formatter
                .debug_struct("ToolFailed")
                .field("tool_call_id", tool_call_id)
                .field("name", name)
                .field("error_code", &error.code)
                .field("retryable", &error.retryable)
                .finish(),
            Self::PermissionRequested { permission } => formatter
                .debug_struct("PermissionRequested")
                .field("permission", permission)
                .finish(),
            Self::PermissionResolved {
                permission_id,
                status,
            } => formatter
                .debug_struct("PermissionResolved")
                .field("permission_id", permission_id)
                .field("status", status)
                .finish(),
            Self::ContextCompacted {
                strategy,
                dropped_turns,
            } => formatter
                .debug_struct("ContextCompacted")
                .field("strategy", strategy)
                .field("dropped_turns", dropped_turns)
                .finish(),
            Self::Usage { usage } => formatter.debug_tuple("Usage").field(usage).finish(),
            Self::Completed { message_id } => formatter
                .debug_struct("Completed")
                .field("message_id", message_id)
                .finish(),
            Self::Failed { error } => formatter
                .debug_struct("Failed")
                .field("error_code", &error.code)
                .field("retryable", &error.retryable)
                .finish(),
            Self::Cancelled { reason } => formatter
                .debug_struct("Cancelled")
                .field("reason_bytes", &reason.as_ref().map_or(0, String::len))
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AgentEvent, AgentEventEnvelope, AgentMessage, AgentMessageContent, AgentMessageRole,
        AgentPermissionDecision, AgentPermissionRequest, AgentResultHandle, AgentToolCall,
        AgentToolOutput, CreateAgentSessionRequest, CreateProviderProfileRequest,
        DecideAgentPermissionRequest, ProviderCredentials, ProviderKind, ProviderProfile,
        SqlPermissionMode, StartAgentRunRequest,
    };
    use crate::{ApiError, JdbcValue, ResultRow};

    #[test]
    fn provider_credentials_are_redacted_and_never_echoed_by_profiles() {
        let request = CreateProviderProfileRequest {
            name: "Primary".to_owned(),
            kind: ProviderKind::Anthropic,
            base_url: "https://api.anthropic.com".to_owned(),
            model: "claude".to_owned(),
            context_window_tokens: "200000".to_owned(),
            max_output_tokens: "8192".to_owned(),
            credentials: Some(ProviderCredentials {
                api_key: "sentinel-provider-key".to_owned(),
            }),
        };
        assert!(!format!("{request:?}").contains("sentinel-provider-key"));

        let profile = ProviderProfile {
            id: "provider-1".to_owned(),
            name: request.name,
            kind: request.kind,
            base_url: request.base_url,
            model: request.model,
            context_window_tokens: request.context_window_tokens,
            max_output_tokens: request.max_output_tokens,
            has_secret: true,
            revision: "1".to_owned(),
            created_at_ms: "1784900000000".to_owned(),
            updated_at_ms: "1784900000000".to_owned(),
        };
        let json = serde_json::to_string(&profile).expect("profile must serialize");
        assert!(!json.contains("apiKey"));
        assert!(!json.contains("secretRef"));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn agent_contract_debug_redacts_prompts_sql_outputs_and_sample_cells() {
        const SENTINEL: &str = "PRIVATE_AGENT_PAYLOAD_7bb35a";

        let call = AgentToolCall {
            id: "call-1".to_owned(),
            name: "sql_read".to_owned(),
            arguments_json: format!("{{\"sql\":\"{SENTINEL}\"}}"),
        };
        let handle = AgentResultHandle {
            handle_id: "handle-1".to_owned(),
            row_count: "1".to_owned(),
            byte_count: "32".to_owned(),
            truncated_by_max_rows: false,
            truncated_by_max_result_bytes: false,
            created_at_ms: "1".to_owned(),
            expires_at_ms: "2".to_owned(),
            columns: Vec::new(),
            sample_rows: vec![ResultRow {
                values: vec![JdbcValue::Text {
                    value: SENTINEL.to_owned(),
                }],
            }],
            sample_truncated: false,
        };
        let text_output = AgentToolOutput::Text {
            content: SENTINEL.to_owned(),
            truncated: false,
        };
        let result_output = AgentToolOutput::Result {
            handle: Box::new(handle.clone()),
        };
        let message = AgentMessage {
            id: "message-1".to_owned(),
            session_id: "session-1".to_owned(),
            run_id: Some("run-1".to_owned()),
            role: AgentMessageRole::Assistant,
            content: vec![
                AgentMessageContent::Text {
                    text: SENTINEL.to_owned(),
                },
                AgentMessageContent::ToolCalls {
                    calls: vec![call.clone()],
                },
                AgentMessageContent::ToolResult {
                    tool_call_id: "call-1".to_owned(),
                    name: "sql_read".to_owned(),
                    output: text_output.clone(),
                },
            ],
            ordinal: "1".to_owned(),
            created_at_ms: "1".to_owned(),
        };
        let permission = AgentPermissionRequest {
            permission_id: "permission-1".to_owned(),
            run_id: "run-1".to_owned(),
            tool_call_id: "call-1".to_owned(),
            tool_name: "sql_write".to_owned(),
            arguments_sha256: "a".repeat(64),
            summary: SENTINEL.to_owned(),
            requested_at_ms: "1".to_owned(),
            expires_at_ms: "2".to_owned(),
        };
        let debug_values = [
            format!(
                "{:?}",
                CreateAgentSessionRequest {
                    title: "session".to_owned(),
                    provider_id: "provider-1".to_owned(),
                    datasource_id: None,
                    system_prompt: Some(SENTINEL.to_owned()),
                }
            ),
            format!(
                "{:?}",
                StartAgentRunRequest {
                    session_id: "session-1".to_owned(),
                    message: SENTINEL.to_owned(),
                    sql_permission_mode: SqlPermissionMode::ReadOnly,
                }
            ),
            format!("{call:?}"),
            format!("{handle:?}"),
            format!("{text_output:?}"),
            format!("{result_output:?}"),
            format!("{message:?}"),
            format!(
                "{:?}",
                AgentEvent::TextDelta {
                    delta: SENTINEL.to_owned()
                }
            ),
            format!(
                "{:?}",
                AgentEvent::ToolCompleted {
                    tool_call_id: "call-1".to_owned(),
                    name: "sql_read".to_owned(),
                    output: AgentToolOutput::Text {
                        content: SENTINEL.to_owned(),
                        truncated: false,
                    },
                }
            ),
            format!("{:?}", AgentEvent::PermissionRequested { permission }),
            format!(
                "{:?}",
                AgentEvent::Failed {
                    error: ApiError::new("provider_error", SENTINEL),
                }
            ),
        ];

        for debug in debug_values {
            assert!(!debug.contains(SENTINEL), "sensitive Debug output: {debug}");
        }
    }

    #[test]
    fn result_handle_and_run_counters_remain_portable() {
        let output = AgentToolOutput::Result {
            handle: Box::new(AgentResultHandle {
                handle_id: "handle-1".to_owned(),
                row_count: "9007199254740993".to_owned(),
                byte_count: "9007199254740994".to_owned(),
                truncated_by_max_rows: false,
                truncated_by_max_result_bytes: false,
                created_at_ms: "1784900000000".to_owned(),
                expires_at_ms: "1784903600000".to_owned(),
                columns: Vec::new(),
                sample_rows: Vec::new(),
                sample_truncated: true,
            }),
        };
        let event = AgentEventEnvelope {
            run_id: "run-1".to_owned(),
            sequence: "9007199254740995".to_owned(),
            occurred_at_ms: "1784900000001".to_owned(),
            event: AgentEvent::ToolCompleted {
                tool_call_id: "call-1".to_owned(),
                name: "sql_read".to_owned(),
                output,
            },
        };

        let json = serde_json::to_value(event).expect("event must serialize");
        assert_eq!(json["sequence"], "9007199254740995");
        assert_eq!(
            json["event"]["output"]["handle"]["rowCount"],
            "9007199254740993"
        );
    }

    #[test]
    fn tagged_agent_contracts_keep_camel_case_fields_and_full_permission_binding() {
        let content = AgentMessageContent::ToolResult {
            tool_call_id: "call-1".to_owned(),
            name: "sql_write".to_owned(),
            output: AgentToolOutput::Text {
                content: "ok".to_owned(),
                truncated: false,
            },
        };
        let decision = DecideAgentPermissionRequest {
            run_id: "run-1".to_owned(),
            tool_call_id: "call-1".to_owned(),
            decision: AgentPermissionDecision::AllowOnce,
            arguments_sha256: "a".repeat(64),
        };

        let content_json = serde_json::to_value(content).expect("content must serialize");
        assert_eq!(content_json["toolCallId"], "call-1");
        assert!(content_json.get("tool_call_id").is_none());

        let decision_json = serde_json::to_value(decision).expect("decision must serialize");
        assert_eq!(decision_json["runId"], "run-1");
        assert_eq!(decision_json["toolCallId"], "call-1");
        assert_eq!(decision_json["argumentsSha256"], "a".repeat(64));
    }
}
