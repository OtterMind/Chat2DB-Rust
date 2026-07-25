use std::{
    fmt::{Debug, Formatter},
    time::Duration,
};

use chat2db_contract::{
    AgentMessageContent as ContractMessageContent, AgentToolCall as ContractToolCall,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::Value;
use uuid::Uuid;

use crate::{Storage, StorageError, now_millis};

/// Maximum bytes accepted for one canonical message JSON value.
pub const MAX_AGENT_MESSAGE_BYTES: u64 = 256 * 1024;
/// Maximum durable messages in one session.
pub const MAX_AGENT_MESSAGES_PER_SESSION: u64 = 4096;
/// Maximum cumulative canonical message bytes in one session.
pub const MAX_AGENT_MESSAGE_BYTES_PER_SESSION: u64 = 32 * 1024 * 1024;
/// Maximum messages returned by one storage page.
pub const MAX_AGENT_MESSAGE_PAGE_SIZE: u32 = 512;
/// Maximum UTF-8 bytes accepted for a session title.
pub const MAX_AGENT_SESSION_TITLE_BYTES: usize = 512;

const MAX_TOOL_CALL_ID_BYTES: usize = 512;
const MAX_TOOL_NAME_BYTES: usize = 512;
const MAX_PERMISSION_SUMMARY_BYTES: usize = 4096;
const MAX_RUN_ERROR_CODE_BYTES: usize = 128;
const MAX_RUN_ERROR_MESSAGE_BYTES: usize = 4096;

/// Recovery actions applied to durable agent state before storage is exposed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentRecoveryReport {
    /// Active runs moved to `Failed` before storage became available.
    pub runs_failed: usize,
    /// Failed runs whose in-flight database write outcome is unknowable.
    pub write_outcomes_unknown: usize,
    /// Pending or approved one-shot permissions revoked.
    pub permissions_revoked: usize,
    /// Expired result handles removed.
    pub result_handles_removed: usize,
}

/// Fields required to create a durable agent session.
#[derive(Clone, PartialEq, Eq)]
pub struct CreateAgentSession {
    /// User-visible session title.
    pub title: String,
    /// Provider profile selected for the session.
    pub provider_id: String,
    /// Optional datasource bound to database tools in the session.
    pub datasource_id: Option<String>,
    /// Optional plain-text system instruction inserted as ordinal zero.
    pub system_prompt: Option<String>,
}

/// Revisioned session ownership update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateAgentSession {
    /// User-visible session title.
    pub title: String,
    /// Provider profile selected for subsequent runs.
    pub provider_id: String,
    /// Optional datasource bound to database tools.
    pub datasource_id: Option<String>,
}

/// Durable agent-session ownership record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionRecord {
    /// Opaque session id.
    pub id: String,
    /// User-visible session title.
    pub title: String,
    /// Bound provider profile id.
    pub provider_id: String,
    /// Optional bound datasource id.
    pub datasource_id: Option<String>,
    /// Monotonic optimistic-concurrency revision.
    pub revision: u64,
    /// Creation time as Unix epoch milliseconds.
    pub created_at_ms: i64,
    /// Last mutation time as Unix epoch milliseconds.
    pub updated_at_ms: i64,
}

/// Roles accepted by the canonical visible-message store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMessageRole {
    /// Runtime/system instructions that are safe to persist.
    System,
    /// User-authored visible content.
    User,
    /// Assistant-authored visible content.
    Assistant,
    /// Durable tool request/result envelopes.
    Tool,
    /// Deterministic or provider-generated context summary.
    Summary,
}

impl AgentMessageRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
            Self::Summary => "summary",
        }
    }

    fn from_persisted(value: &str) -> Result<Self, StorageError> {
        match value {
            "system" => Ok(Self::System),
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            "tool" => Ok(Self::Tool),
            "summary" => Ok(Self::Summary),
            _ => Err(StorageError::InvalidAgent(
                "persisted message role is invalid",
            )),
        }
    }
}

/// One session-owned canonical message append request.
///
/// Run-owned messages are accepted only by the dedicated atomic run lifecycle
/// APIs so transcript writes cannot outlive or interleave with their run.
#[derive(Clone, PartialEq, Eq)]
pub struct AppendAgentMessage {
    /// Visible message role.
    pub role: AgentMessageRole,
    /// Last original message represented by a summary message.
    pub summary_through_ordinal: Option<u64>,
    /// Strict JSON object containing the visible message/tool envelope.
    pub content_json: String,
}

/// One ordinal-addressed durable canonical message.
#[derive(Clone, PartialEq, Eq)]
pub struct AgentMessageRecord {
    /// Opaque stable message id.
    pub id: String,
    /// Owning session id.
    pub session_id: String,
    /// Run that produced or consumed the message, when applicable.
    pub run_id: Option<String>,
    /// Contiguous zero-based ordinal.
    pub ordinal: u64,
    /// Visible message role.
    pub role: AgentMessageRole,
    /// Last original message represented by this summary.
    pub summary_through_ordinal: Option<u64>,
    /// SQLite-normalized strict JSON. Raw provider deltas and hidden reasoning
    /// have no dedicated durable representation.
    pub content_json: String,
    /// Canonical UTF-8 byte length.
    pub content_bytes: u64,
    /// Creation time as Unix epoch milliseconds.
    pub created_at_ms: i64,
}

/// SQL capability granted to one durable agent run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SqlPermissionMode {
    /// Only read-only database tools may execute.
    #[default]
    ReadOnly,
    /// Write tools may request an exact, one-shot approval.
    AskBeforeWrite,
}

impl SqlPermissionMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::AskBeforeWrite => "ask_before_write",
        }
    }

    fn from_persisted(value: &str) -> Result<Self, StorageError> {
        match value {
            "read_only" => Ok(Self::ReadOnly),
            "ask_before_write" => Ok(Self::AskBeforeWrite),
            _ => Err(StorageError::InvalidAgent(
                "persisted SQL permission mode is invalid",
            )),
        }
    }
}

/// Input atomically persisted when one run starts.
#[derive(Clone, PartialEq, Eq)]
pub struct StartAgentRun {
    /// New plain-text user message.
    pub user_message: String,
    /// SQL capability fixed for the lifetime of this run.
    pub sql_permission_mode: SqlPermissionMode,
}

/// Durable run and its atomically inserted user message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedAgentRun {
    /// Running snapshot created before the caller acknowledges the request.
    pub run: AgentRunRecord,
    /// User message owned by the run.
    pub user_message: AgentMessageRecord,
}

/// Durable agent-run lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRunStatus {
    /// Executing provider/tool work.
    Running,
    /// Paused at an external permission gate.
    WaitingPermission,
    /// Successfully completed.
    Completed,
    /// Failed with a safe persisted error.
    Failed,
    /// Explicitly cancelled.
    Cancelled,
}

impl AgentRunStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::WaitingPermission => "waiting_permission",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn from_persisted(value: &str) -> Result<Self, StorageError> {
        match value {
            "running" => Ok(Self::Running),
            "waiting_permission" => Ok(Self::WaitingPermission),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(StorageError::InvalidAgent(
                "persisted run status is invalid",
            )),
        }
    }

    fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    fn can_transition_to(self, next: Self) -> bool {
        if self == next {
            return !self.is_terminal();
        }
        matches!(
            (self, next),
            (
                Self::WaitingPermission,
                Self::Running | Self::Failed | Self::Cancelled
            ) | (
                Self::Running,
                Self::WaitingPermission | Self::Completed | Self::Failed | Self::Cancelled
            )
        )
    }
}

/// Durable run progress and token-usage replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunUpdate {
    /// Desired next lifecycle state.
    pub status: AgentRunStatus,
    /// Latest event sequence represented by this snapshot.
    pub last_sequence: u64,
    /// Completed model rounds.
    pub model_rounds: u64,
    /// Started tool calls.
    pub tool_calls: u64,
    /// Accumulated provider input tokens.
    pub input_tokens: u64,
    /// Accumulated provider output tokens.
    pub output_tokens: u64,
    /// Accumulated provider total tokens.
    pub total_tokens: u64,
    /// Already-durable compaction count; ordinary progress cannot change it.
    pub compaction_count: u64,
    /// Already-durable compaction coverage; ordinary progress cannot change it.
    pub compacted_through_ordinal: Option<u64>,
}

/// Exact durable effect of one context-compaction event.
#[derive(Clone, PartialEq, Eq)]
pub enum AgentCompaction {
    /// The threshold fired but no complete historical turn was removable.
    NoOp,
    /// Complete historical turns were discarded without a replacement message.
    DeterministicTrim {
        /// Greatest durable ordinal removed by this pass.
        compacted_through_ordinal: u64,
    },
    /// Complete historical turns were replaced by one bounded summary.
    Summary {
        /// Greatest durable ordinal represented by the summary.
        compacted_through_ordinal: u64,
        /// Canonical visible summary content.
        content_json: String,
    },
}

/// One context-compaction event persisted with its exact durable effect.
#[derive(Clone, PartialEq, Eq)]
pub struct CompactAgentRun {
    /// Exact next event sequence represented by this compaction.
    pub last_sequence: u64,
    /// Completed model rounds.
    pub model_rounds: u64,
    /// Started tool calls.
    pub tool_calls: u64,
    /// Accumulated provider input tokens.
    pub input_tokens: u64,
    /// Accumulated provider output tokens.
    pub output_tokens: u64,
    /// Accumulated provider total tokens.
    pub total_tokens: u64,
    /// Exact no-op, deterministic-trim, or summary effect.
    pub compaction: AgentCompaction,
}

/// Durable compaction progress and its optional atomically inserted summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactedAgentRun {
    /// Updated durable run snapshot.
    pub run: AgentRunRecord,
    /// Summary appended by the same transaction, if summary compaction won.
    pub summary_message: Option<AgentMessageRecord>,
}

/// One complete provider-neutral message appended as part of a run transaction.
#[derive(Clone, PartialEq, Eq)]
pub struct AgentRunMessage {
    /// Assistant or tool role. Summary messages use [`CompactAgentRun`].
    pub role: AgentMessageRole,
    /// Reserved for the role contract and required to be absent here.
    pub summary_through_ordinal: Option<u64>,
    /// Canonical provider-neutral message-content JSON array.
    pub content_json: String,
}

/// Successful run progress and the final canonical assistant message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteAgentRun {
    /// Latest event sequence represented by the completed snapshot.
    pub last_sequence: u64,
    /// Completed model rounds.
    pub model_rounds: u64,
    /// Started tool calls.
    pub tool_calls: u64,
    /// Accumulated provider input tokens.
    pub input_tokens: u64,
    /// Accumulated provider output tokens.
    pub output_tokens: u64,
    /// Accumulated provider total tokens.
    pub total_tokens: u64,
    /// Ordered complete messages produced by the run; the last must be assistant.
    pub messages: Vec<AgentRunMessage>,
    /// Completed compaction passes.
    pub compaction_count: u64,
    /// Latest message ordinal represented by compacted context.
    pub compacted_through_ordinal: Option<u64>,
}

/// Durable run state, counters, usage, cancellation, and compaction metadata.
#[derive(Clone, PartialEq, Eq)]
pub struct AgentRunRecord {
    /// Opaque run id.
    pub id: String,
    /// Owning session id.
    pub session_id: String,
    /// Current lifecycle state.
    pub status: AgentRunStatus,
    /// SQL capability fixed when the run starts.
    pub sql_permission_mode: SqlPermissionMode,
    /// Latest event sequence represented by this snapshot.
    pub last_sequence: u64,
    /// Completed model rounds.
    pub model_rounds: u64,
    /// Started tool calls.
    pub tool_calls: u64,
    /// Accumulated provider input tokens.
    pub input_tokens: u64,
    /// Accumulated provider output tokens.
    pub output_tokens: u64,
    /// Accumulated provider total tokens.
    pub total_tokens: u64,
    /// Durable final assistant message id.
    pub message_id: Option<String>,
    /// Safe failure code, if any.
    pub error_code: Option<String>,
    /// Safe failure text, if any.
    pub error_message: Option<String>,
    /// Durable cancellation bit.
    pub cancel_requested: bool,
    /// Tool call whose approved database write may currently be executing.
    pub write_in_flight_tool_call_id: Option<String>,
    /// Exact argument digest for the write-dispatch fence.
    pub write_in_flight_arguments_sha256: Option<[u8; 32]>,
    /// Completed compaction passes.
    pub compaction_count: u64,
    /// Latest compacted message ordinal.
    pub compacted_through_ordinal: Option<u64>,
    /// Creation time as Unix epoch milliseconds.
    pub created_at_ms: i64,
    /// Last mutation time as Unix epoch milliseconds.
    pub updated_at_ms: i64,
    /// First execution time.
    pub started_at_ms: Option<i64>,
    /// Terminal time.
    pub finished_at_ms: Option<i64>,
}

/// Successful terminal transition and its atomically inserted assistant message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedAgentRun {
    /// Completed durable run snapshot.
    pub run: AgentRunRecord,
    /// Ordered messages appended by the terminal transaction.
    pub messages: Vec<AgentMessageRecord>,
}

/// Failed run progress, safe error, and any complete messages produced first.
#[derive(Clone, PartialEq, Eq)]
pub struct FailAgentRun {
    /// Latest event sequence represented by the failed snapshot.
    pub last_sequence: u64,
    /// Completed model rounds.
    pub model_rounds: u64,
    /// Started tool calls.
    pub tool_calls: u64,
    /// Accumulated provider input tokens.
    pub input_tokens: u64,
    /// Accumulated provider output tokens.
    pub output_tokens: u64,
    /// Accumulated provider total tokens.
    pub total_tokens: u64,
    /// Safe, bounded machine-readable failure code.
    pub error_code: String,
    /// Optional safe, bounded user-visible failure text.
    pub error_message: Option<String>,
    /// Complete assistant/tool messages available before failure.
    pub messages: Vec<AgentRunMessage>,
    /// Completed compaction passes.
    pub compaction_count: u64,
    /// Latest message ordinal represented by compacted context.
    pub compacted_through_ordinal: Option<u64>,
}

/// Failed durable run and messages atomically appended with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedAgentRun {
    /// Failed durable run snapshot.
    pub run: AgentRunRecord,
    /// Ordered messages appended by the terminal transaction.
    pub messages: Vec<AgentMessageRecord>,
}

/// Cancelled run progress and complete messages produced before cancellation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelAgentRun {
    /// Latest event sequence represented by the cancelled snapshot.
    pub last_sequence: u64,
    /// Completed model rounds.
    pub model_rounds: u64,
    /// Started tool calls.
    pub tool_calls: u64,
    /// Accumulated provider input tokens.
    pub input_tokens: u64,
    /// Accumulated provider output tokens.
    pub output_tokens: u64,
    /// Accumulated provider total tokens.
    pub total_tokens: u64,
    /// Complete assistant/tool messages available before cancellation.
    pub messages: Vec<AgentRunMessage>,
    /// Completed compaction passes.
    pub compaction_count: u64,
    /// Latest message ordinal represented by compacted context.
    pub compacted_through_ordinal: Option<u64>,
}

/// Cancelled durable run and messages atomically appended with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelledAgentRun {
    /// Cancelled durable run snapshot.
    pub run: AgentRunRecord,
    /// Ordered messages appended by the terminal transaction.
    pub messages: Vec<AgentMessageRecord>,
}

/// Progress and complete messages for a database write with an unknown outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownAgentWrite {
    /// Latest event sequence represented by the failed snapshot.
    pub last_sequence: u64,
    /// Completed model rounds.
    pub model_rounds: u64,
    /// Started tool calls.
    pub tool_calls: u64,
    /// Accumulated provider input tokens.
    pub input_tokens: u64,
    /// Accumulated provider output tokens.
    pub output_tokens: u64,
    /// Accumulated provider total tokens.
    pub total_tokens: u64,
    /// Complete assistant/tool messages available before the unknown outcome.
    pub messages: Vec<AgentRunMessage>,
    /// Completed compaction passes.
    pub compaction_count: u64,
    /// Latest message ordinal represented by compacted context.
    pub compacted_through_ordinal: Option<u64>,
}

/// Human decision applied to one pending tool permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPermissionDecision {
    /// Allow exactly one matching execution.
    Approve,
    /// Permanently deny execution.
    Deny,
}

/// Durable one-shot permission states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPermissionStatus {
    /// Awaiting a decision.
    Pending,
    /// Approved but not yet consumed.
    Approved,
    /// Explicitly denied.
    Denied,
    /// Atomically consumed for one matching execution.
    Consumed,
    /// Expired before execution.
    Expired,
    /// Revoked by cancellation or recovery.
    Revoked,
}

/// Exact write operation placed behind a one-shot approval gate.
#[derive(Clone, PartialEq, Eq)]
pub struct RequestToolPermission {
    /// Caller-selected tool-call id.
    pub tool_call_id: String,
    /// Registered write-tool name.
    pub tool_name: String,
    /// SHA-256 of the exact canonical arguments.
    pub arguments_sha256: [u8; 32],
    /// Bounded user-visible operation summary.
    pub summary: String,
    /// Event sequence for the permission-requested snapshot.
    pub last_sequence: u64,
    /// Maximum time this approval can remain executable.
    pub retention: Duration,
}

impl ToolPermissionStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Consumed => "consumed",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
        }
    }

    fn from_persisted(value: &str) -> Result<Self, StorageError> {
        match value {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "denied" => Ok(Self::Denied),
            "consumed" => Ok(Self::Consumed),
            "expired" => Ok(Self::Expired),
            "revoked" => Ok(Self::Revoked),
            _ => Err(StorageError::InvalidAgent(
                "persisted tool permission status is invalid",
            )),
        }
    }
}

/// Durable permission bound to one run, tool call, and argument digest.
#[derive(Clone, PartialEq, Eq)]
pub struct ToolPermissionRecord {
    /// Opaque permission id.
    pub id: String,
    /// Owning run id.
    pub run_id: String,
    /// Caller-selected tool-call id.
    pub tool_call_id: String,
    /// Registered tool name displayed at the approval boundary.
    pub tool_name: String,
    /// SHA-256 of the exact canonical arguments authorized by the user.
    pub arguments_sha256: [u8; 32],
    /// Bounded user-visible operation summary.
    pub summary: String,
    /// Current one-shot state.
    pub status: ToolPermissionStatus,
    /// Monotonic CAS revision.
    pub revision: u64,
    /// Expiry time as Unix epoch milliseconds.
    pub expires_at_ms: i64,
    /// Creation time.
    pub created_at_ms: i64,
    /// Last mutation time.
    pub updated_at_ms: i64,
    /// Successful consume time.
    pub consumed_at_ms: Option<i64>,
}

/// Opaque, ownership-bound mapping to an existing retained result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentResultHandle {
    /// Opaque handle id exposed to the agent/tool layer.
    pub id: String,
    /// Owning session id.
    pub session_id: String,
    /// Owning run id.
    pub run_id: String,
    /// Existing retained-result id; no result bytes are duplicated here.
    pub result_id: String,
    /// Creation time.
    pub created_at_ms: i64,
    /// Effective expiry, never later than the retained result.
    pub expires_at_ms: i64,
}

impl Debug for CreateAgentSession {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CreateAgentSession")
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

impl Debug for AppendAgentMessage {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AppendAgentMessage")
            .field("role", &self.role)
            .field("summary_through_ordinal", &self.summary_through_ordinal)
            .field("content_json_bytes", &self.content_json.len())
            .finish()
    }
}

impl Debug for AgentMessageRecord {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentMessageRecord")
            .field("id", &self.id)
            .field("session_id", &self.session_id)
            .field("run_id", &self.run_id)
            .field("ordinal", &self.ordinal)
            .field("role", &self.role)
            .field("summary_through_ordinal", &self.summary_through_ordinal)
            .field("content_json_bytes", &self.content_json.len())
            .field("content_bytes", &self.content_bytes)
            .field("created_at_ms", &self.created_at_ms)
            .finish()
    }
}

impl Debug for StartAgentRun {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StartAgentRun")
            .field("user_message_bytes", &self.user_message.len())
            .field("sql_permission_mode", &self.sql_permission_mode)
            .finish()
    }
}

impl Debug for AgentRunMessage {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentRunMessage")
            .field("role", &self.role)
            .field("summary_through_ordinal", &self.summary_through_ordinal)
            .field("content_json_bytes", &self.content_json.len())
            .finish()
    }
}

impl Debug for CompactAgentRun {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompactAgentRun")
            .field("last_sequence", &self.last_sequence)
            .field("model_rounds", &self.model_rounds)
            .field("tool_calls", &self.tool_calls)
            .field("input_tokens", &self.input_tokens)
            .field("output_tokens", &self.output_tokens)
            .field("total_tokens", &self.total_tokens)
            .field("compaction", &self.compaction)
            .finish()
    }
}

impl Debug for AgentCompaction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoOp => formatter.write_str("NoOp"),
            Self::DeterministicTrim {
                compacted_through_ordinal,
            } => formatter
                .debug_struct("DeterministicTrim")
                .field("compacted_through_ordinal", compacted_through_ordinal)
                .finish(),
            Self::Summary {
                compacted_through_ordinal,
                content_json,
            } => formatter
                .debug_struct("Summary")
                .field("compacted_through_ordinal", compacted_through_ordinal)
                .field("content_json_bytes", &content_json.len())
                .finish(),
        }
    }
}

impl Debug for AgentRunRecord {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentRunRecord")
            .field("id", &self.id)
            .field("session_id", &self.session_id)
            .field("status", &self.status)
            .field("sql_permission_mode", &self.sql_permission_mode)
            .field("last_sequence", &self.last_sequence)
            .field("model_rounds", &self.model_rounds)
            .field("tool_calls", &self.tool_calls)
            .field("input_tokens", &self.input_tokens)
            .field("output_tokens", &self.output_tokens)
            .field("total_tokens", &self.total_tokens)
            .field("message_id", &self.message_id)
            .field("error_code", &self.error_code)
            .field(
                "error_message_bytes",
                &self.error_message.as_ref().map_or(0, String::len),
            )
            .field("cancel_requested", &self.cancel_requested)
            .field(
                "write_in_flight_tool_call_id",
                &self.write_in_flight_tool_call_id,
            )
            .field(
                "has_write_in_flight_arguments_sha256",
                &self.write_in_flight_arguments_sha256.is_some(),
            )
            .field("compaction_count", &self.compaction_count)
            .field("compacted_through_ordinal", &self.compacted_through_ordinal)
            .field("created_at_ms", &self.created_at_ms)
            .field("updated_at_ms", &self.updated_at_ms)
            .field("started_at_ms", &self.started_at_ms)
            .field("finished_at_ms", &self.finished_at_ms)
            .finish()
    }
}

impl Debug for FailAgentRun {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FailAgentRun")
            .field("last_sequence", &self.last_sequence)
            .field("model_rounds", &self.model_rounds)
            .field("tool_calls", &self.tool_calls)
            .field("input_tokens", &self.input_tokens)
            .field("output_tokens", &self.output_tokens)
            .field("total_tokens", &self.total_tokens)
            .field("error_code", &self.error_code)
            .field(
                "error_message_bytes",
                &self.error_message.as_ref().map_or(0, String::len),
            )
            .field("messages", &self.messages)
            .field("compaction_count", &self.compaction_count)
            .field("compacted_through_ordinal", &self.compacted_through_ordinal)
            .finish()
    }
}

impl Debug for RequestToolPermission {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RequestToolPermission")
            .field("tool_call_id", &self.tool_call_id)
            .field("tool_name", &self.tool_name)
            .field("arguments_sha256", &self.arguments_sha256)
            .field("summary_bytes", &self.summary.len())
            .field("last_sequence", &self.last_sequence)
            .field("retention", &self.retention)
            .finish()
    }
}

impl Debug for ToolPermissionRecord {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ToolPermissionRecord")
            .field("id", &self.id)
            .field("run_id", &self.run_id)
            .field("tool_call_id", &self.tool_call_id)
            .field("tool_name", &self.tool_name)
            .field("arguments_sha256", &self.arguments_sha256)
            .field("summary_bytes", &self.summary.len())
            .field("status", &self.status)
            .field("revision", &self.revision)
            .field("expires_at_ms", &self.expires_at_ms)
            .field("created_at_ms", &self.created_at_ms)
            .field("updated_at_ms", &self.updated_at_ms)
            .field("consumed_at_ms", &self.consumed_at_ms)
            .finish()
    }
}

impl Storage {
    /// Creates a session after validating provider and optional datasource ownership.
    ///
    /// # Errors
    ///
    /// Returns validation, not-found, numeric-range, or `SQLite` failures.
    pub fn create_agent_session(
        &self,
        input: CreateAgentSession,
    ) -> Result<AgentSessionRecord, StorageError> {
        let CreateAgentSession {
            title,
            provider_id,
            datasource_id,
            system_prompt,
        } = input;
        validate_session_title(&title)?;
        let timestamp = now_millis()?;
        let mut connection = self.connection()?;
        let system_prompt = system_prompt
            .as_deref()
            .map(|prompt| text_message_json(&connection, prompt))
            .transpose()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !row_exists(
            &transaction,
            "SELECT EXISTS(SELECT 1 FROM provider_profiles WHERE id = ?1)",
            &provider_id,
        )? {
            return Err(StorageError::ProviderNotFound(provider_id));
        }
        if let Some(datasource_id) = datasource_id.as_deref()
            && !row_exists(
                &transaction,
                "SELECT EXISTS(SELECT 1 FROM datasources WHERE id = ?1)",
                datasource_id,
            )?
        {
            return Err(StorageError::DatasourceNotFound(datasource_id.to_owned()));
        }
        let record = AgentSessionRecord {
            id: Uuid::new_v4().to_string(),
            title,
            provider_id,
            datasource_id,
            revision: 1,
            created_at_ms: timestamp,
            updated_at_ms: timestamp,
        };
        transaction.execute(
            "INSERT INTO agent_sessions (
                id, title, provider_id, datasource_id, revision, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)",
            params![
                record.id,
                record.title,
                record.provider_id,
                record.datasource_id,
                timestamp
            ],
        )?;
        if let Some((canonical, canonical_bytes)) = system_prompt.as_ref() {
            append_message_in_transaction(
                &transaction,
                &record.id,
                None,
                AgentMessageRole::System,
                None,
                canonical,
                *canonical_bytes,
                timestamp,
            )?;
        }
        transaction.commit()?;
        Ok(record)
    }

    /// Loads one agent session.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or persisted-data validation failures.
    pub fn get_agent_session(&self, id: &str) -> Result<Option<AgentSessionRecord>, StorageError> {
        load_session(&self.connection()?, id)
    }

    /// Lists sessions in reverse update order.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or persisted-data validation failures.
    pub fn list_agent_sessions(&self) -> Result<Vec<AgentSessionRecord>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, title, provider_id, datasource_id, revision, created_at_ms, updated_at_ms
             FROM agent_sessions ORDER BY updated_at_ms DESC, id DESC",
        )?;
        statement
            .query_map([], raw_session)?
            .map(|row| decode_session(row?))
            .collect()
    }

    /// Rebinds a session using revision CAS.
    ///
    /// # Errors
    ///
    /// Returns not-found, revision-conflict, validation, or `SQLite` failures.
    pub fn update_agent_session(
        &self,
        id: &str,
        expected_revision: u64,
        input: UpdateAgentSession,
    ) -> Result<AgentSessionRecord, StorageError> {
        validate_session_title(&input.title)?;
        let next_revision = expected_revision
            .checked_add(1)
            .ok_or(StorageError::NumericRange("agent session revision"))?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_session(&transaction, id)?
            .ok_or_else(|| StorageError::AgentSessionNotFound(id.to_owned()))?;
        if current.revision != expected_revision {
            return Err(StorageError::AgentSessionRevisionConflict {
                id: id.to_owned(),
                expected: expected_revision,
                actual: Some(current.revision),
            });
        }
        if (current.provider_id != input.provider_id
            || current.datasource_id != input.datasource_id)
            && row_exists(
                &transaction,
                "SELECT EXISTS(
                    SELECT 1 FROM agent_runs
                    WHERE session_id = ?1 AND status IN ('running', 'waiting_permission')
                 )",
                id,
            )?
        {
            return Err(StorageError::AgentSessionBusy(id.to_owned()));
        }
        if !row_exists(
            &transaction,
            "SELECT EXISTS(SELECT 1 FROM provider_profiles WHERE id = ?1)",
            &input.provider_id,
        )? {
            return Err(StorageError::ProviderNotFound(input.provider_id));
        }
        if let Some(datasource_id) = input.datasource_id.as_deref()
            && !row_exists(
                &transaction,
                "SELECT EXISTS(SELECT 1 FROM datasources WHERE id = ?1)",
                datasource_id,
            )?
        {
            return Err(StorageError::DatasourceNotFound(datasource_id.to_owned()));
        }
        let timestamp = now_millis()?;
        let changed = transaction.execute(
            "UPDATE agent_sessions
             SET title = ?1, provider_id = ?2, datasource_id = ?3,
                 revision = ?4, updated_at_ms = ?5
             WHERE id = ?6 AND revision = ?7",
            params![
                input.title,
                input.provider_id,
                input.datasource_id,
                to_sql_i64(next_revision, "agent session revision")?,
                timestamp,
                id,
                to_sql_i64(expected_revision, "agent session revision")?,
            ],
        )?;
        if changed != 1 {
            return Err(StorageError::AgentSessionRevisionConflict {
                id: id.to_owned(),
                expected: expected_revision,
                actual: load_session(&transaction, id)?.map(|record| record.revision),
            });
        }
        transaction.commit()?;
        load_session(&connection, id)?.ok_or_else(|| StorageError::OutcomeUnknown {
            operation: "update agent session",
            id: id.to_owned(),
        })
    }

    /// Deletes a session and all session-owned agent state using revision CAS.
    ///
    /// Provider profiles, datasources, and retained results are shared resources
    /// and are not removed. A running or permission-waiting run keeps the
    /// session alive until that run reaches a terminal state.
    ///
    /// # Errors
    ///
    /// Returns not-found, revision-conflict, busy-session, numeric-range, or
    /// `SQLite` failures.
    pub fn delete_agent_session(
        &self,
        id: &str,
        expected_revision: u64,
    ) -> Result<(), StorageError> {
        let expected_revision_sql = to_sql_i64(expected_revision, "agent session revision")?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_session(&transaction, id)?
            .ok_or_else(|| StorageError::AgentSessionNotFound(id.to_owned()))?;
        if current.revision != expected_revision {
            return Err(StorageError::AgentSessionRevisionConflict {
                id: id.to_owned(),
                expected: expected_revision,
                actual: Some(current.revision),
            });
        }
        if row_exists(
            &transaction,
            "SELECT EXISTS(
                SELECT 1 FROM agent_runs
                WHERE session_id = ?1 AND status IN ('running', 'waiting_permission')
             )",
            id,
        )? {
            return Err(StorageError::AgentSessionBusy(id.to_owned()));
        }
        let changed = transaction.execute(
            "DELETE FROM agent_sessions WHERE id = ?1 AND revision = ?2",
            params![id, expected_revision_sql],
        )?;
        if changed != 1 {
            return Err(StorageError::AgentSessionRevisionConflict {
                id: id.to_owned(),
                expected: expected_revision,
                actual: load_session(&transaction, id)?.map(|record| record.revision),
            });
        }
        transaction.commit()?;
        Ok(())
    }

    /// Appends one strict, normalized session message with a contiguous ordinal.
    ///
    /// A session with an active run rejects the append. Run-owned messages use
    /// the dedicated atomic start, complete, fail, cancellation, or unknown
    /// write-outcome APIs instead.
    ///
    /// # Errors
    ///
    /// Returns not-found, invalid-JSON, quota, numeric-range, or `SQLite` failures.
    pub fn append_agent_message(
        &self,
        session_id: &str,
        input: AppendAgentMessage,
    ) -> Result<AgentMessageRecord, StorageError> {
        let AppendAgentMessage {
            role,
            summary_through_ordinal,
            content_json,
        } = input;
        if role == AgentMessageRole::Summary {
            return Err(StorageError::InvalidAgent(
                "summary messages require the dedicated atomic compaction API",
            ));
        }
        let mut connection = self.connection()?;
        let (canonical, canonical_bytes) = normalize_message_json(&connection, &content_json)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if row_exists(
            &transaction,
            "SELECT EXISTS(
                SELECT 1 FROM agent_runs
                WHERE session_id = ?1 AND status IN ('running', 'waiting_permission')
             )",
            session_id,
        )? {
            return Err(StorageError::AgentSessionBusy(session_id.to_owned()));
        }
        let timestamp = now_millis()?;
        let record = append_message_in_transaction(
            &transaction,
            session_id,
            None,
            role,
            summary_through_ordinal,
            &canonical,
            canonical_bytes,
            timestamp,
        )?;
        transaction.commit()?;
        Ok(record)
    }

    /// Reads one bounded ordinal page of canonical messages.
    ///
    /// # Errors
    ///
    /// Returns not-found, invalid-limit, persisted-data, or `SQLite` failures.
    pub fn list_agent_messages(
        &self,
        session_id: &str,
        start_ordinal: u64,
        limit: u32,
    ) -> Result<Vec<AgentMessageRecord>, StorageError> {
        if limit == 0 || limit > MAX_AGENT_MESSAGE_PAGE_SIZE {
            return Err(StorageError::InvalidAgent(
                "message page size must be between 1 and 512",
            ));
        }
        let connection = self.connection()?;
        if !row_exists(
            &connection,
            "SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE id = ?1)",
            session_id,
        )? {
            return Err(StorageError::AgentSessionNotFound(session_id.to_owned()));
        }
        let mut statement = connection.prepare(
            "SELECT id, session_id, run_id, ordinal, role, summary_through_ordinal,
                    content_json, content_bytes, created_at_ms
             FROM agent_messages
             WHERE session_id = ?1 AND ordinal >= ?2
             ORDER BY ordinal LIMIT ?3",
        )?;
        statement
            .query_map(
                params![
                    session_id,
                    to_sql_i64(start_ordinal, "agent message ordinal")?,
                    i64::from(limit)
                ],
                raw_message,
            )?
            .map(|row| decode_message(row?))
            .collect()
    }

    /// Returns the greatest durable message ordinal covered by compaction in one session.
    ///
    /// Coverage includes deterministic trimming recorded on runs and summaries
    /// recorded as canonical messages.
    ///
    /// # Errors
    ///
    /// Returns not-found, persisted-data, or `SQLite` failures.
    pub fn get_agent_session_compaction_coverage(
        &self,
        session_id: &str,
    ) -> Result<Option<u64>, StorageError> {
        let connection = self.connection()?;
        load_session_compaction_coverage(&connection, session_id)
    }

    /// Atomically persists a running run and its initiating user message.
    ///
    /// # Errors
    ///
    /// Returns not-found, busy-session, validation, quota, clock, or `SQLite` failures.
    #[allow(clippy::needless_pass_by_value)]
    pub fn start_agent_run(
        &self,
        session_id: &str,
        input: StartAgentRun,
    ) -> Result<StartedAgentRun, StorageError> {
        let timestamp = now_millis()?;
        let mut connection = self.connection()?;
        let (canonical, canonical_bytes) = text_message_json(&connection, &input.user_message)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !row_exists(
            &transaction,
            "SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE id = ?1)",
            session_id,
        )? {
            return Err(StorageError::AgentSessionNotFound(session_id.to_owned()));
        }
        if row_exists(
            &transaction,
            "SELECT EXISTS(
                SELECT 1 FROM agent_runs
                WHERE session_id = ?1 AND status IN ('running', 'waiting_permission')
             )",
            session_id,
        )? {
            return Err(StorageError::AgentSessionBusy(session_id.to_owned()));
        }
        let run = AgentRunRecord {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_owned(),
            status: AgentRunStatus::Running,
            sql_permission_mode: input.sql_permission_mode,
            last_sequence: 1,
            model_rounds: 0,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            message_id: None,
            error_code: None,
            error_message: None,
            cancel_requested: false,
            write_in_flight_tool_call_id: None,
            write_in_flight_arguments_sha256: None,
            compaction_count: 0,
            compacted_through_ordinal: None,
            created_at_ms: timestamp,
            updated_at_ms: timestamp,
            started_at_ms: Some(timestamp),
            finished_at_ms: None,
        };
        transaction.execute(
            "INSERT INTO agent_runs (
                id, session_id, sql_permission_mode, status, last_sequence,
                created_at_ms, updated_at_ms, started_at_ms
             ) VALUES (?1, ?2, ?3, 'running', 1, ?4, ?4, ?4)",
            params![
                run.id,
                run.session_id,
                run.sql_permission_mode.as_str(),
                timestamp
            ],
        )?;
        let user_message = append_message_in_transaction(
            &transaction,
            session_id,
            Some(&run.id),
            AgentMessageRole::User,
            None,
            &canonical,
            canonical_bytes,
            timestamp,
        )?;
        transaction.commit()?;
        Ok(StartedAgentRun { run, user_message })
    }

    /// Loads one durable run.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or persisted-data validation failures.
    pub fn get_agent_run(&self, id: &str) -> Result<Option<AgentRunRecord>, StorageError> {
        load_run(&self.connection()?, id)
    }

    /// Replaces monotonic run progress under a lifecycle-state CAS.
    ///
    /// # Errors
    ///
    /// Returns not-found, state-conflict, validation, or `SQLite` failures.
    #[allow(clippy::needless_pass_by_value)]
    pub fn update_agent_run(
        &self,
        id: &str,
        expected_status: AgentRunStatus,
        update: AgentRunUpdate,
    ) -> Result<AgentRunRecord, StorageError> {
        validate_run_update(&update)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_run(&transaction, id)?
            .ok_or_else(|| StorageError::AgentRunNotFound(id.to_owned()))?;
        if current.status != expected_status {
            return Err(run_state_conflict(id, expected_status, current.status));
        }
        if current.write_in_flight_tool_call_id.is_some() {
            return Err(StorageError::InvalidAgent(
                "agent run progress cannot change while a database write is in flight",
            ));
        }
        if current.status != update.status {
            return Err(StorageError::InvalidAgent(
                "run status transitions require their dedicated atomic API",
            ));
        }
        validate_compaction_unchanged(
            &current,
            update.compaction_count,
            update.compacted_through_ordinal,
        )?;
        validate_run_progress(
            &current,
            update.last_sequence,
            update.model_rounds,
            update.tool_calls,
            update.input_tokens,
            update.output_tokens,
            update.total_tokens,
            update.compaction_count,
            update.compacted_through_ordinal,
        )?;
        validate_run_compaction_boundary(&transaction, id, update.compacted_through_ordinal)?;
        let timestamp = now_millis()?;
        let changed = transaction.execute(
            "UPDATE agent_runs
             SET status = ?1, last_sequence = ?2, model_rounds = ?3, tool_calls = ?4,
                 input_tokens = ?5, output_tokens = ?6, total_tokens = ?7,
                 compaction_count = ?8, compacted_through_ordinal = ?9,
                 updated_at_ms = ?10
             WHERE id = ?11 AND status = ?12 AND cancel_requested = 0",
            params![
                update.status.as_str(),
                to_sql_i64(update.last_sequence, "agent event sequence")?,
                to_sql_i64(update.model_rounds, "agent model rounds")?,
                to_sql_i64(update.tool_calls, "agent tool calls")?,
                to_sql_i64(update.input_tokens, "agent input tokens")?,
                to_sql_i64(update.output_tokens, "agent output tokens")?,
                to_sql_i64(update.total_tokens, "agent total tokens")?,
                to_sql_i64(update.compaction_count, "agent compaction count")?,
                update
                    .compacted_through_ordinal
                    .map(|value| to_sql_i64(value, "compacted message ordinal"))
                    .transpose()?,
                timestamp,
                id,
                expected_status.as_str(),
            ],
        )?;
        if changed != 1 {
            return Err(StorageError::AgentStateConflict {
                id: id.to_owned(),
                expected: expected_status.as_str(),
                actual: current.status.as_str(),
            });
        }
        transaction.commit()?;
        load_run(&connection, id)?.ok_or_else(|| StorageError::OutcomeUnknown {
            operation: "update agent run",
            id: id.to_owned(),
        })
    }

    /// Atomically persists one context-compaction event and its optional summary.
    ///
    /// Summary compaction appends the canonical summary and advances run
    /// sequence, count, and coverage in one `IMMEDIATE` transaction.
    /// Deterministic trim advances the same run fields without a message. A
    /// no-op advances only the sequence and ordinary counters.
    ///
    /// # Errors
    ///
    /// Returns not-found, state-conflict, cancellation/write-fence, validation,
    /// quota, numeric-range, unknown-outcome, or `SQLite` failures.
    #[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
    pub fn compact_agent_run(
        &self,
        id: &str,
        expected_status: AgentRunStatus,
        input: CompactAgentRun,
    ) -> Result<CompactedAgentRun, StorageError> {
        if expected_status != AgentRunStatus::Running {
            return Err(StorageError::InvalidAgent(
                "agent context compaction requires a running run",
            ));
        }

        let (requested_coverage, summary_content_json) = match &input.compaction {
            AgentCompaction::NoOp => (None, None),
            AgentCompaction::DeterministicTrim {
                compacted_through_ordinal,
            } => (Some(*compacted_through_ordinal), None),
            AgentCompaction::Summary {
                compacted_through_ordinal,
                content_json,
            } => (
                Some(*compacted_through_ordinal),
                Some(content_json.as_str()),
            ),
        };

        let mut connection = self.connection()?;
        let normalized_summary = summary_content_json
            .map(|content_json| {
                let (content_json, content_bytes) =
                    normalize_message_json(&connection, content_json)?;
                validate_message_role(AgentMessageRole::Summary, &content_json)?;
                Ok::<_, StorageError>(NormalizedAgentRunMessage {
                    role: AgentMessageRole::Summary,
                    summary_through_ordinal: requested_coverage,
                    content_json,
                    content_bytes,
                })
            })
            .transpose()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_run(&transaction, id)?
            .ok_or_else(|| StorageError::AgentRunNotFound(id.to_owned()))?;
        if current.status != expected_status {
            return Err(run_state_conflict(id, expected_status, current.status));
        }
        if current.cancel_requested {
            return Err(StorageError::InvalidAgent(
                "agent context cannot compact after cancellation is requested",
            ));
        }
        if current.write_in_flight_tool_call_id.is_some() {
            return Err(StorageError::InvalidAgent(
                "agent context cannot compact while a database write is in flight",
            ));
        }
        if current.last_sequence.checked_add(1) != Some(input.last_sequence) {
            return Err(StorageError::InvalidAgent(
                "agent compaction sequence must be the next event",
            ));
        }

        let (compaction_count, compacted_through_ordinal) =
            if let Some(coverage) = requested_coverage {
                if load_session_compaction_coverage(&transaction, &current.session_id)?
                    .is_some_and(|current_coverage| coverage <= current_coverage)
                {
                    return Err(StorageError::InvalidAgent(
                        "agent compaction coverage must advance the session",
                    ));
                }
                (
                    current
                        .compaction_count
                        .checked_add(1)
                        .ok_or(StorageError::NumericRange("agent compaction count"))?,
                    Some(coverage),
                )
            } else {
                (current.compaction_count, current.compacted_through_ordinal)
            };
        validate_run_progress(
            &current,
            input.last_sequence,
            input.model_rounds,
            input.tool_calls,
            input.input_tokens,
            input.output_tokens,
            input.total_tokens,
            compaction_count,
            compacted_through_ordinal,
        )?;
        validate_run_compaction_boundary(&transaction, id, requested_coverage)?;

        let timestamp = now_millis()?;
        let summary_message = normalized_summary
            .map(|summary| {
                append_message_in_transaction(
                    &transaction,
                    &current.session_id,
                    Some(id),
                    summary.role,
                    summary.summary_through_ordinal,
                    &summary.content_json,
                    summary.content_bytes,
                    timestamp,
                )
            })
            .transpose()?;
        let changed = transaction.execute(
            "UPDATE agent_runs
             SET last_sequence = ?1, model_rounds = ?2, tool_calls = ?3,
                 input_tokens = ?4, output_tokens = ?5, total_tokens = ?6,
                 compaction_count = ?7, compacted_through_ordinal = ?8,
                 updated_at_ms = ?9
             WHERE id = ?10 AND status = 'running' AND cancel_requested = 0
                   AND write_in_flight_tool_call_id IS NULL",
            params![
                to_sql_i64(input.last_sequence, "agent event sequence")?,
                to_sql_i64(input.model_rounds, "agent model rounds")?,
                to_sql_i64(input.tool_calls, "agent tool calls")?,
                to_sql_i64(input.input_tokens, "agent input tokens")?,
                to_sql_i64(input.output_tokens, "agent output tokens")?,
                to_sql_i64(input.total_tokens, "agent total tokens")?,
                to_sql_i64(compaction_count, "agent compaction count")?,
                compacted_through_ordinal
                    .map(|value| to_sql_i64(value, "compacted message ordinal"))
                    .transpose()?,
                timestamp,
                id,
            ],
        )?;
        if changed != 1 {
            return Err(run_state_conflict(id, expected_status, current.status));
        }
        let prior_run = current.clone();
        let mut expected_run = current;
        expected_run.last_sequence = input.last_sequence;
        expected_run.model_rounds = input.model_rounds;
        expected_run.tool_calls = input.tool_calls;
        expected_run.input_tokens = input.input_tokens;
        expected_run.output_tokens = input.output_tokens;
        expected_run.total_tokens = input.total_tokens;
        expected_run.compaction_count = compaction_count;
        expected_run.compacted_through_ordinal = compacted_through_ordinal;
        expected_run.updated_at_ms = timestamp;
        #[cfg(test)]
        if crate::take_fault(crate::FaultPoint::AgentCompactionBeforeCommit) {
            return Err(crate::injected_commit_error());
        }
        #[cfg(test)]
        let commit_failure = crate::take_fault(crate::FaultPoint::AgentCompactionCommitFailure);
        #[cfg(test)]
        let commit_error = if commit_failure {
            drop(transaction);
            Some(crate::injected_commit_error())
        } else {
            transaction.commit().err().map(StorageError::Sqlite)
        };
        #[cfg(not(test))]
        let commit_error = transaction.commit().err().map(StorageError::Sqlite);
        #[cfg(test)]
        let post_commit_failure = crate::take_fault(crate::FaultPoint::AgentCompactionAfterCommit);
        #[cfg(not(test))]
        let post_commit_failure = false;
        if commit_error.is_some() || post_commit_failure {
            return self.reconcile_agent_compaction_commit(
                id,
                &prior_run,
                &expected_run,
                summary_message.as_ref(),
                commit_error.unwrap_or_else(|| StorageError::OutcomeUnknown {
                    operation: "compact agent run",
                    id: id.to_owned(),
                }),
            );
        }
        Ok(CompactedAgentRun {
            run: expected_run,
            summary_message,
        })
    }

    fn reconcile_agent_compaction_commit(
        &self,
        id: &str,
        prior_run: &AgentRunRecord,
        expected_run: &AgentRunRecord,
        expected_summary: Option<&AgentMessageRecord>,
        original_error: StorageError,
    ) -> Result<CompactedAgentRun, StorageError> {
        let unknown = || StorageError::OutcomeUnknown {
            operation: "compact agent run",
            id: id.to_owned(),
        };
        #[cfg(test)]
        if crate::take_fault(crate::FaultPoint::AgentCompactionReadback) {
            return Err(unknown());
        }
        let connection = self.connection().map_err(|_| unknown())?;
        let actual_run = load_run(&connection, id).map_err(|_| unknown())?;
        let actual_summary = if let Some(expected) = expected_summary {
            load_message(&connection, &expected.id).map_err(|_| unknown())?
        } else {
            None
        };
        let summary_applied =
            expected_summary.is_none() || actual_summary.as_ref() == expected_summary;
        if actual_run.as_ref() == Some(expected_run) && summary_applied {
            return Ok(CompactedAgentRun {
                run: expected_run.clone(),
                summary_message: actual_summary,
            });
        }
        let summary_absent = expected_summary.is_none() || actual_summary.is_none();
        if actual_run.as_ref() == Some(prior_run) && summary_absent {
            return Err(original_error);
        }
        Err(unknown())
    }

    /// Atomically appends the final assistant message and completes its run.
    ///
    /// # Errors
    ///
    /// Returns not-found, state-conflict, validation, quota, or `SQLite` failures.
    #[allow(clippy::needless_pass_by_value)]
    pub fn complete_agent_run(
        &self,
        id: &str,
        expected_status: AgentRunStatus,
        input: CompleteAgentRun,
    ) -> Result<CompletedAgentRun, StorageError> {
        validate_complete_agent_run(&input)?;
        let mut connection = self.connection()?;
        let normalized = normalize_agent_run_messages(&connection, &input.messages, true)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_run(&transaction, id)?
            .ok_or_else(|| StorageError::AgentRunNotFound(id.to_owned()))?;
        if current.status != expected_status {
            return Err(run_state_conflict(id, expected_status, current.status));
        }
        if current.cancel_requested
            || current.write_in_flight_tool_call_id.is_some()
            || !current.status.can_transition_to(AgentRunStatus::Completed)
        {
            return Err(StorageError::InvalidAgent(
                "agent run cannot complete from its current state",
            ));
        }
        validate_compaction_unchanged(
            &current,
            input.compaction_count,
            input.compacted_through_ordinal,
        )?;
        validate_run_progress(
            &current,
            input.last_sequence,
            input.model_rounds,
            input.tool_calls,
            input.input_tokens,
            input.output_tokens,
            input.total_tokens,
            input.compaction_count,
            input.compacted_through_ordinal,
        )?;
        validate_run_compaction_boundary(&transaction, id, input.compacted_through_ordinal)?;
        let timestamp = now_millis()?;
        let mut messages = Vec::with_capacity(normalized.len());
        for message in normalized {
            messages.push(append_message_in_transaction(
                &transaction,
                &current.session_id,
                Some(id),
                message.role,
                message.summary_through_ordinal,
                &message.content_json,
                message.content_bytes,
                timestamp,
            )?);
        }
        let message_id = messages
            .last()
            .ok_or(StorageError::InvalidAgent(
                "completed agent run requires a final assistant message",
            ))?
            .id
            .clone();
        let changed = transaction.execute(
            "UPDATE agent_runs
             SET status = 'completed', last_sequence = ?1, model_rounds = ?2,
                 tool_calls = ?3, input_tokens = ?4, output_tokens = ?5,
                 total_tokens = ?6, message_id = ?7, error_code = NULL,
                 error_message = NULL, compaction_count = ?8,
                 compacted_through_ordinal = ?9, updated_at_ms = ?10,
                 finished_at_ms = ?10
             WHERE id = ?11 AND status = ?12 AND cancel_requested = 0",
            params![
                to_sql_i64(input.last_sequence, "agent event sequence")?,
                to_sql_i64(input.model_rounds, "agent model rounds")?,
                to_sql_i64(input.tool_calls, "agent tool calls")?,
                to_sql_i64(input.input_tokens, "agent input tokens")?,
                to_sql_i64(input.output_tokens, "agent output tokens")?,
                to_sql_i64(input.total_tokens, "agent total tokens")?,
                message_id,
                to_sql_i64(input.compaction_count, "agent compaction count")?,
                input
                    .compacted_through_ordinal
                    .map(|value| to_sql_i64(value, "compacted message ordinal"))
                    .transpose()?,
                timestamp,
                id,
                expected_status.as_str(),
            ],
        )?;
        if changed != 1 {
            return Err(run_state_conflict(id, expected_status, current.status));
        }
        transaction.execute(
            "UPDATE tool_permissions
             SET status = 'revoked', revision = revision + 1, updated_at_ms = ?1
             WHERE run_id = ?2 AND status IN ('pending', 'approved')",
            params![timestamp, id],
        )?;
        transaction.commit()?;
        let run = load_run(&connection, id)?.ok_or_else(|| StorageError::OutcomeUnknown {
            operation: "complete agent run",
            id: id.to_owned(),
        })?;
        Ok(CompletedAgentRun { run, messages })
    }

    /// Atomically appends complete run messages and records a safe failure.
    ///
    /// # Errors
    ///
    /// Returns not-found, state-conflict, validation, quota, or `SQLite` failures.
    #[allow(clippy::needless_pass_by_value)]
    pub fn fail_agent_run(
        &self,
        id: &str,
        expected_status: AgentRunStatus,
        input: FailAgentRun,
    ) -> Result<FailedAgentRun, StorageError> {
        validate_fail_agent_run(&input)?;
        let mut connection = self.connection()?;
        let normalized = normalize_agent_run_messages(&connection, &input.messages, false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_run(&transaction, id)?
            .ok_or_else(|| StorageError::AgentRunNotFound(id.to_owned()))?;
        if current.status != expected_status {
            return Err(run_state_conflict(id, expected_status, current.status));
        }
        if current.cancel_requested
            || current.write_in_flight_tool_call_id.is_some()
            || !current.status.can_transition_to(AgentRunStatus::Failed)
        {
            return Err(StorageError::InvalidAgent(
                "agent run cannot fail from its current state",
            ));
        }
        validate_compaction_unchanged(
            &current,
            input.compaction_count,
            input.compacted_through_ordinal,
        )?;
        validate_run_progress(
            &current,
            input.last_sequence,
            input.model_rounds,
            input.tool_calls,
            input.input_tokens,
            input.output_tokens,
            input.total_tokens,
            input.compaction_count,
            input.compacted_through_ordinal,
        )?;
        validate_run_compaction_boundary(&transaction, id, input.compacted_through_ordinal)?;
        let timestamp = now_millis()?;
        let mut messages = Vec::with_capacity(normalized.len());
        for message in normalized {
            messages.push(append_message_in_transaction(
                &transaction,
                &current.session_id,
                Some(id),
                message.role,
                message.summary_through_ordinal,
                &message.content_json,
                message.content_bytes,
                timestamp,
            )?);
        }
        let changed = transaction.execute(
            "UPDATE agent_runs
             SET status = 'failed', last_sequence = ?1, model_rounds = ?2,
                 tool_calls = ?3, input_tokens = ?4, output_tokens = ?5,
                 total_tokens = ?6, message_id = NULL, error_code = ?7,
                 error_message = ?8, compaction_count = ?9,
                 compacted_through_ordinal = ?10, updated_at_ms = ?11,
                 finished_at_ms = ?11
             WHERE id = ?12 AND status = ?13 AND cancel_requested = 0
               AND write_in_flight_tool_call_id IS NULL",
            params![
                to_sql_i64(input.last_sequence, "agent event sequence")?,
                to_sql_i64(input.model_rounds, "agent model rounds")?,
                to_sql_i64(input.tool_calls, "agent tool calls")?,
                to_sql_i64(input.input_tokens, "agent input tokens")?,
                to_sql_i64(input.output_tokens, "agent output tokens")?,
                to_sql_i64(input.total_tokens, "agent total tokens")?,
                input.error_code,
                input.error_message,
                to_sql_i64(input.compaction_count, "agent compaction count")?,
                input
                    .compacted_through_ordinal
                    .map(|value| to_sql_i64(value, "compacted message ordinal"))
                    .transpose()?,
                timestamp,
                id,
                expected_status.as_str(),
            ],
        )?;
        if changed != 1 {
            return Err(run_state_conflict(id, expected_status, current.status));
        }
        transaction.execute(
            "UPDATE tool_permissions
             SET status = 'revoked', revision = revision + 1, updated_at_ms = ?1
             WHERE run_id = ?2 AND status IN ('pending', 'approved')",
            params![timestamp, id],
        )?;
        transaction.commit()?;
        let run = load_run(&connection, id)?.ok_or_else(|| StorageError::OutcomeUnknown {
            operation: "fail agent run",
            id: id.to_owned(),
        })?;
        Ok(FailedAgentRun { run, messages })
    }

    /// Requests cancellation and revokes every unconsumed permission.
    ///
    /// # Errors
    ///
    /// Returns not-found, clock, `SQLite`, or unknown-outcome failures.
    pub fn request_agent_run_cancellation(&self, id: &str) -> Result<AgentRunRecord, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_run(&transaction, id)?
            .ok_or_else(|| StorageError::AgentRunNotFound(id.to_owned()))?;
        let timestamp = now_millis()?;
        if !current.status.is_terminal() {
            transaction.execute(
                "UPDATE agent_runs
                 SET cancel_requested = 1, updated_at_ms = ?1
                 WHERE id = ?2 AND status = ?3",
                params![timestamp, id, current.status.as_str()],
            )?;
        }
        transaction.execute(
            "UPDATE tool_permissions
             SET status = 'revoked', revision = revision + 1, updated_at_ms = ?1
             WHERE run_id = ?2 AND status IN ('pending', 'approved')",
            params![timestamp, id],
        )?;
        transaction.commit()?;
        load_run(&connection, id)?.ok_or_else(|| StorageError::OutcomeUnknown {
            operation: "request agent run cancellation",
            id: id.to_owned(),
        })
    }

    /// Atomically appends complete messages and finalizes a requested cancellation.
    ///
    /// # Errors
    ///
    /// Returns not-found, state-conflict, validation, quota, or `SQLite` failures.
    #[allow(clippy::needless_pass_by_value)]
    pub fn finish_cancelled_agent_run(
        &self,
        id: &str,
        expected_status: AgentRunStatus,
        input: CancelAgentRun,
    ) -> Result<CancelledAgentRun, StorageError> {
        validate_cancel_agent_run(&input)?;
        let mut connection = self.connection()?;
        let normalized = normalize_agent_run_messages(&connection, &input.messages, false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_run(&transaction, id)?
            .ok_or_else(|| StorageError::AgentRunNotFound(id.to_owned()))?;
        if current.status != expected_status {
            return Err(run_state_conflict(id, expected_status, current.status));
        }
        if !current.cancel_requested
            || current.write_in_flight_tool_call_id.is_some()
            || current.status.is_terminal()
        {
            return Err(StorageError::InvalidAgent(
                "agent run cancellation cannot finish from its current state",
            ));
        }
        validate_compaction_unchanged(
            &current,
            input.compaction_count,
            input.compacted_through_ordinal,
        )?;
        validate_run_progress(
            &current,
            input.last_sequence,
            input.model_rounds,
            input.tool_calls,
            input.input_tokens,
            input.output_tokens,
            input.total_tokens,
            input.compaction_count,
            input.compacted_through_ordinal,
        )?;
        validate_run_compaction_boundary(&transaction, id, input.compacted_through_ordinal)?;
        let timestamp = now_millis()?;
        let mut messages = Vec::with_capacity(normalized.len());
        for message in normalized {
            messages.push(append_message_in_transaction(
                &transaction,
                &current.session_id,
                Some(id),
                message.role,
                message.summary_through_ordinal,
                &message.content_json,
                message.content_bytes,
                timestamp,
            )?);
        }
        let changed = transaction.execute(
            "UPDATE agent_runs
             SET status = 'cancelled', last_sequence = ?1, model_rounds = ?2,
                 tool_calls = ?3, input_tokens = ?4, output_tokens = ?5,
                 total_tokens = ?6, message_id = NULL, error_code = NULL,
                 error_message = NULL, compaction_count = ?7,
                 compacted_through_ordinal = ?8, updated_at_ms = ?9,
                 finished_at_ms = ?9
             WHERE id = ?10 AND status = ?11 AND cancel_requested = 1
               AND write_in_flight_tool_call_id IS NULL",
            params![
                to_sql_i64(input.last_sequence, "agent event sequence")?,
                to_sql_i64(input.model_rounds, "agent model rounds")?,
                to_sql_i64(input.tool_calls, "agent tool calls")?,
                to_sql_i64(input.input_tokens, "agent input tokens")?,
                to_sql_i64(input.output_tokens, "agent output tokens")?,
                to_sql_i64(input.total_tokens, "agent total tokens")?,
                to_sql_i64(input.compaction_count, "agent compaction count")?,
                input
                    .compacted_through_ordinal
                    .map(|value| to_sql_i64(value, "compacted message ordinal"))
                    .transpose()?,
                timestamp,
                id,
                expected_status.as_str(),
            ],
        )?;
        if changed != 1 {
            return Err(run_state_conflict(id, expected_status, current.status));
        }
        transaction.commit()?;
        let run = load_run(&connection, id)?.ok_or_else(|| StorageError::OutcomeUnknown {
            operation: "finish cancelled agent run",
            id: id.to_owned(),
        })?;
        Ok(CancelledAgentRun { run, messages })
    }

    /// Creates a pending, one-shot tool permission.
    ///
    /// # Errors
    ///
    /// Returns run-state, validation, numeric-range, or `SQLite` failures.
    #[allow(clippy::needless_pass_by_value)]
    pub fn create_tool_permission(
        &self,
        run_id: &str,
        input: RequestToolPermission,
    ) -> Result<ToolPermissionRecord, StorageError> {
        if input.retention.is_zero() {
            return Err(StorageError::InvalidAgent(
                "tool permission retention must be greater than zero",
            ));
        }
        let timestamp = now_millis()?;
        let retention_ms = i64::try_from(input.retention.as_millis())
            .map_err(|_| StorageError::NumericRange("tool permission retention"))?;
        let expires_at_ms = timestamp
            .checked_add(retention_ms)
            .ok_or(StorageError::NumericRange("tool permission expiry"))?;
        self.create_tool_permission_at(run_id, input, timestamp, expires_at_ms)
    }

    #[allow(clippy::needless_pass_by_value)]
    fn create_tool_permission_at(
        &self,
        run_id: &str,
        input: RequestToolPermission,
        timestamp: i64,
        expires_at_ms: i64,
    ) -> Result<ToolPermissionRecord, StorageError> {
        validate_permission_request(&input)?;
        if input.last_sequence == 0 {
            return Err(StorageError::InvalidAgent(
                "permission event sequence must be greater than zero",
            ));
        }
        if input.tool_call_id.trim().is_empty() || input.tool_call_id.len() > MAX_TOOL_CALL_ID_BYTES
        {
            return Err(StorageError::InvalidAgent(
                "tool call id must be non-empty and at most 512 UTF-8 bytes",
            ));
        }
        if expires_at_ms <= timestamp {
            return Err(StorageError::InvalidAgent(
                "tool permission expiry must be after creation",
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = load_run(&transaction, run_id)?
            .ok_or_else(|| StorageError::AgentRunNotFound(run_id.to_owned()))?;
        if run.cancel_requested
            || run.status != AgentRunStatus::Running
            || run.sql_permission_mode != SqlPermissionMode::AskBeforeWrite
            || run.write_in_flight_tool_call_id.is_some()
        {
            return Err(StorageError::PermissionNotExecutable {
                id: run_id.to_owned(),
                reason: "owning run is not executable",
            });
        }
        if input.last_sequence <= run.last_sequence {
            return Err(StorageError::InvalidAgent(
                "permission event sequence must advance the run snapshot",
            ));
        }
        let id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO tool_permissions (
                id, run_id, tool_call_id, tool_name, arguments_sha256, summary,
                status, revision, expires_at_ms, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', 1, ?7, ?8, ?8)",
            params![
                id,
                run_id,
                input.tool_call_id,
                input.tool_name,
                input.arguments_sha256.as_slice(),
                input.summary,
                expires_at_ms,
                timestamp,
            ],
        )?;
        let changed = transaction.execute(
            "UPDATE agent_runs
             SET status = 'waiting_permission', last_sequence = ?1, updated_at_ms = ?2
             WHERE id = ?3 AND status = 'running' AND cancel_requested = 0
               AND write_in_flight_tool_call_id IS NULL",
            params![
                to_sql_i64(input.last_sequence, "agent event sequence")?,
                timestamp,
                run_id
            ],
        )?;
        if changed != 1 {
            return Err(run_state_conflict(
                run_id,
                AgentRunStatus::Running,
                run.status,
            ));
        }
        transaction.commit()?;
        load_permission(&connection, &id)?.ok_or_else(|| StorageError::OutcomeUnknown {
            operation: "create tool permission",
            id,
        })
    }

    /// Loads one tool permission.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or persisted-data validation failures.
    pub fn get_tool_permission(
        &self,
        id: &str,
    ) -> Result<Option<ToolPermissionRecord>, StorageError> {
        load_permission(&self.connection()?, id)
    }

    /// Loads the single pending or approved permission for a run.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or persisted-data validation failures.
    pub fn get_active_tool_permission_for_run(
        &self,
        run_id: &str,
    ) -> Result<Option<ToolPermissionRecord>, StorageError> {
        let connection = self.connection()?;
        let raw = connection
            .query_row(
                "SELECT id, run_id, tool_call_id, tool_name, arguments_sha256,
                        summary, status, revision, expires_at_ms, created_at_ms,
                        updated_at_ms, consumed_at_ms
                 FROM tool_permissions
                 WHERE run_id = ?1 AND status IN ('pending', 'approved')",
                [run_id],
                raw_permission,
            )
            .optional()?;
        raw.map(decode_permission).transpose()
    }

    /// Approves or denies a pending permission using revision CAS.
    ///
    /// # Errors
    ///
    /// Returns not-found, revision-conflict, expiry, state, or `SQLite` failures.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub fn decide_tool_permission(
        &self,
        id: &str,
        expected_revision: u64,
        run_id: &str,
        tool_call_id: &str,
        arguments_sha256: [u8; 32],
        last_sequence: u64,
        decision: ToolPermissionDecision,
    ) -> Result<ToolPermissionRecord, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_permission(&transaction, id)?
            .ok_or_else(|| StorageError::PermissionNotFound(id.to_owned()))?;
        if current.revision != expected_revision {
            return Err(permission_revision_conflict(
                id,
                expected_revision,
                Some(current.revision),
            ));
        }
        if current.run_id != run_id
            || current.tool_call_id != tool_call_id
            || current.arguments_sha256 != arguments_sha256
        {
            return Err(StorageError::PermissionNotExecutable {
                id: id.to_owned(),
                reason: "permission binding does not match",
            });
        }
        if current.status != ToolPermissionStatus::Pending {
            return Err(StorageError::PermissionNotExecutable {
                id: id.to_owned(),
                reason: "permission is no longer pending",
            });
        }
        let run = load_run(&transaction, run_id)?
            .ok_or_else(|| StorageError::AgentRunNotFound(run_id.to_owned()))?;
        if run.cancel_requested
            || run.status != AgentRunStatus::WaitingPermission
            || run.write_in_flight_tool_call_id.is_some()
        {
            return Err(StorageError::PermissionNotExecutable {
                id: id.to_owned(),
                reason: "owning run is not waiting for this permission",
            });
        }
        if last_sequence <= run.last_sequence {
            return Err(StorageError::InvalidAgent(
                "permission decision sequence must advance the run snapshot",
            ));
        }
        let timestamp = now_millis()?;
        if current.expires_at_ms <= timestamp {
            expire_permission(&transaction, &current, timestamp)?;
            transaction.execute(
                "UPDATE agent_runs SET status = 'running', updated_at_ms = ?1
                 WHERE id = ?2 AND status = 'waiting_permission'",
                params![timestamp, run_id],
            )?;
            transaction.commit()?;
            return Err(StorageError::PermissionNotExecutable {
                id: id.to_owned(),
                reason: "permission expired",
            });
        }
        let status = match decision {
            ToolPermissionDecision::Approve => ToolPermissionStatus::Approved,
            ToolPermissionDecision::Deny => ToolPermissionStatus::Denied,
        };
        let next_revision = current
            .revision
            .checked_add(1)
            .ok_or(StorageError::NumericRange("tool permission revision"))?;
        let changed = transaction.execute(
            "UPDATE tool_permissions
             SET status = ?1, revision = ?2, updated_at_ms = ?3
             WHERE id = ?4 AND revision = ?5 AND status = 'pending'
               AND run_id = ?6 AND tool_call_id = ?7 AND arguments_sha256 = ?8",
            params![
                status.as_str(),
                to_sql_i64(next_revision, "tool permission revision")?,
                timestamp,
                id,
                to_sql_i64(expected_revision, "tool permission revision")?,
                run_id,
                tool_call_id,
                arguments_sha256.as_slice(),
            ],
        )?;
        if changed != 1 {
            return Err(StorageError::PermissionNotExecutable {
                id: id.to_owned(),
                reason: "permission decision CAS failed",
            });
        }
        let run_status = match decision {
            ToolPermissionDecision::Approve => AgentRunStatus::WaitingPermission,
            ToolPermissionDecision::Deny => AgentRunStatus::Running,
        };
        let run_changed = transaction.execute(
            "UPDATE agent_runs
             SET status = ?1, last_sequence = ?2, updated_at_ms = ?3
             WHERE id = ?4 AND status = 'waiting_permission'
               AND cancel_requested = 0 AND last_sequence < ?2
               AND write_in_flight_tool_call_id IS NULL",
            params![
                run_status.as_str(),
                to_sql_i64(last_sequence, "agent event sequence")?,
                timestamp,
                run_id,
            ],
        )?;
        if run_changed != 1 {
            return Err(StorageError::PermissionNotExecutable {
                id: id.to_owned(),
                reason: "owning run changed during the permission decision",
            });
        }
        transaction.commit()?;
        load_permission(&connection, id)?.ok_or_else(|| StorageError::OutcomeUnknown {
            operation: "decide tool permission",
            id: id.to_owned(),
        })
    }

    /// Atomically consumes one exact approval and marks its database write in flight.
    ///
    /// # Errors
    ///
    /// Returns not-found, revision-conflict, binding, expiry, run-state, or
    /// `SQLite` failures. The caller must not dispatch the write before this
    /// method commits successfully, and must later call [`Self::settle_agent_write`].
    #[allow(clippy::too_many_arguments)]
    pub fn consume_tool_permission(
        &self,
        id: &str,
        expected_revision: u64,
        run_id: &str,
        tool_call_id: &str,
        arguments_sha256: [u8; 32],
        last_sequence: u64,
    ) -> Result<ToolPermissionRecord, StorageError> {
        self.consume_tool_permission_at(
            id,
            expected_revision,
            run_id,
            tool_call_id,
            arguments_sha256,
            last_sequence,
            now_millis()?,
        )
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn consume_tool_permission_at(
        &self,
        id: &str,
        expected_revision: u64,
        run_id: &str,
        tool_call_id: &str,
        arguments_sha256: [u8; 32],
        last_sequence: u64,
        timestamp: i64,
    ) -> Result<ToolPermissionRecord, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_permission(&transaction, id)?
            .ok_or_else(|| StorageError::PermissionNotFound(id.to_owned()))?;
        if current.revision != expected_revision {
            return Err(permission_revision_conflict(
                id,
                expected_revision,
                Some(current.revision),
            ));
        }
        if current.run_id != run_id
            || current.tool_call_id != tool_call_id
            || current.arguments_sha256 != arguments_sha256
        {
            return Err(StorageError::PermissionNotExecutable {
                id: id.to_owned(),
                reason: "permission binding does not match",
            });
        }
        if current.status != ToolPermissionStatus::Approved {
            return Err(StorageError::PermissionNotExecutable {
                id: id.to_owned(),
                reason: "permission is not approved",
            });
        }
        if current.expires_at_ms <= timestamp {
            expire_permission(&transaction, &current, timestamp)?;
            transaction.execute(
                "UPDATE agent_runs SET status = 'running', updated_at_ms = ?1
                 WHERE id = ?2 AND status = 'waiting_permission'",
                params![timestamp, run_id],
            )?;
            transaction.commit()?;
            return Err(StorageError::PermissionNotExecutable {
                id: id.to_owned(),
                reason: "permission expired",
            });
        }
        let run = load_run(&transaction, run_id)?
            .ok_or_else(|| StorageError::AgentRunNotFound(run_id.to_owned()))?;
        if run.cancel_requested
            || run.status != AgentRunStatus::WaitingPermission
            || run.sql_permission_mode != SqlPermissionMode::AskBeforeWrite
            || run.write_in_flight_tool_call_id.is_some()
        {
            return Err(StorageError::PermissionNotExecutable {
                id: id.to_owned(),
                reason: "owning run is not executable",
            });
        }
        if last_sequence <= run.last_sequence {
            return Err(StorageError::InvalidAgent(
                "tool-start sequence must advance the run snapshot",
            ));
        }
        let next_revision = current
            .revision
            .checked_add(1)
            .ok_or(StorageError::NumericRange("tool permission revision"))?;
        let changed = transaction.execute(
            "UPDATE tool_permissions
             SET status = 'consumed', revision = ?1,
                 updated_at_ms = ?2, consumed_at_ms = ?2
             WHERE id = ?3 AND revision = ?4 AND status = 'approved'
               AND run_id = ?5 AND tool_call_id = ?6 AND arguments_sha256 = ?7
               AND expires_at_ms > ?2
               AND EXISTS (
                   SELECT 1 FROM agent_runs
                   WHERE id = ?5 AND cancel_requested = 0
                     AND status IN ('running', 'waiting_permission')
               )",
            params![
                to_sql_i64(next_revision, "tool permission revision")?,
                timestamp,
                id,
                to_sql_i64(expected_revision, "tool permission revision")?,
                run_id,
                tool_call_id,
                arguments_sha256.as_slice(),
            ],
        )?;
        if changed != 1 {
            return Err(StorageError::PermissionNotExecutable {
                id: id.to_owned(),
                reason: "permission consume CAS failed",
            });
        }
        let run_changed = transaction.execute(
            "UPDATE agent_runs
             SET status = 'running', last_sequence = ?1,
                 write_in_flight_tool_call_id = ?2,
                 write_in_flight_arguments_sha256 = ?3, updated_at_ms = ?4
             WHERE id = ?5 AND status = 'waiting_permission'
               AND cancel_requested = 0 AND last_sequence < ?1
               AND write_in_flight_tool_call_id IS NULL",
            params![
                to_sql_i64(last_sequence, "agent event sequence")?,
                tool_call_id,
                arguments_sha256.as_slice(),
                timestamp,
                run_id
            ],
        )?;
        if run_changed != 1 {
            return Err(StorageError::PermissionNotExecutable {
                id: id.to_owned(),
                reason: "database write dispatch fence could not be installed",
            });
        }
        transaction.commit()?;
        load_permission(&connection, id)?.ok_or_else(|| StorageError::OutcomeUnknown {
            operation: "consume tool permission",
            id: id.to_owned(),
        })
    }

    /// Clears one exact database-write dispatch fence after a known outcome.
    ///
    /// # Errors
    ///
    /// Returns not-found, binding, numeric-range, or `SQLite` failures.
    pub fn settle_agent_write(
        &self,
        run_id: &str,
        tool_call_id: &str,
        arguments_sha256: [u8; 32],
    ) -> Result<AgentRunRecord, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_run(&transaction, run_id)?
            .ok_or_else(|| StorageError::AgentRunNotFound(run_id.to_owned()))?;
        if current.status != AgentRunStatus::Running
            || current.write_in_flight_tool_call_id.as_deref() != Some(tool_call_id)
            || current.write_in_flight_arguments_sha256 != Some(arguments_sha256)
        {
            return Err(StorageError::PermissionNotExecutable {
                id: run_id.to_owned(),
                reason: "database write dispatch fence does not match",
            });
        }
        let timestamp = now_millis()?;
        let changed = transaction.execute(
            "UPDATE agent_runs
             SET write_in_flight_tool_call_id = NULL,
                 write_in_flight_arguments_sha256 = NULL, updated_at_ms = ?1
             WHERE id = ?2 AND write_in_flight_tool_call_id = ?3
               AND write_in_flight_arguments_sha256 = ?4",
            params![timestamp, run_id, tool_call_id, arguments_sha256.as_slice(),],
        )?;
        if changed != 1 {
            return Err(StorageError::PermissionNotExecutable {
                id: run_id.to_owned(),
                reason: "database write dispatch fence settlement CAS failed",
            });
        }
        transaction.commit()?;
        load_run(&connection, run_id)?.ok_or_else(|| StorageError::OutcomeUnknown {
            operation: "settle agent write",
            id: run_id.to_owned(),
        })
    }

    /// Atomically records an unknowable database-write outcome without clearing its fence.
    ///
    /// The exact marker is retained as durable evidence and this transition must
    /// never be followed by an automatic retry.
    ///
    /// # Errors
    ///
    /// Returns not-found, binding, validation, quota, or `SQLite` failures.
    #[allow(clippy::needless_pass_by_value)]
    pub fn fail_agent_write_outcome_unknown(
        &self,
        run_id: &str,
        tool_call_id: &str,
        arguments_sha256: [u8; 32],
        input: UnknownAgentWrite,
    ) -> Result<FailedAgentRun, StorageError> {
        validate_unknown_agent_write(&input)?;
        let mut connection = self.connection()?;
        let normalized = normalize_agent_run_messages(&connection, &input.messages, false)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_run(&transaction, run_id)?
            .ok_or_else(|| StorageError::AgentRunNotFound(run_id.to_owned()))?;
        if current.status != AgentRunStatus::Running
            || current.write_in_flight_tool_call_id.as_deref() != Some(tool_call_id)
            || current.write_in_flight_arguments_sha256 != Some(arguments_sha256)
        {
            return Err(StorageError::PermissionNotExecutable {
                id: run_id.to_owned(),
                reason: "database write dispatch fence does not match",
            });
        }
        validate_compaction_unchanged(
            &current,
            input.compaction_count,
            input.compacted_through_ordinal,
        )?;
        validate_run_progress(
            &current,
            input.last_sequence,
            input.model_rounds,
            input.tool_calls,
            input.input_tokens,
            input.output_tokens,
            input.total_tokens,
            input.compaction_count,
            input.compacted_through_ordinal,
        )?;
        validate_run_compaction_boundary(&transaction, run_id, input.compacted_through_ordinal)?;
        let timestamp = now_millis()?;
        let mut messages = Vec::with_capacity(normalized.len());
        for message in normalized {
            messages.push(append_message_in_transaction(
                &transaction,
                &current.session_id,
                Some(run_id),
                message.role,
                message.summary_through_ordinal,
                &message.content_json,
                message.content_bytes,
                timestamp,
            )?);
        }
        let changed = transaction.execute(
            "UPDATE agent_runs
             SET status = 'failed', last_sequence = ?1, model_rounds = ?2,
                 tool_calls = ?3, input_tokens = ?4, output_tokens = ?5,
                 total_tokens = ?6, message_id = NULL,
                 error_code = 'database_outcome_unknown',
                 error_message = 'Database write outcome is unknown and must not be retried',
                 compaction_count = ?7, compacted_through_ordinal = ?8,
                 updated_at_ms = ?9, finished_at_ms = ?9
             WHERE id = ?10 AND status = 'running'
               AND write_in_flight_tool_call_id = ?11
               AND write_in_flight_arguments_sha256 = ?12",
            params![
                to_sql_i64(input.last_sequence, "agent event sequence")?,
                to_sql_i64(input.model_rounds, "agent model rounds")?,
                to_sql_i64(input.tool_calls, "agent tool calls")?,
                to_sql_i64(input.input_tokens, "agent input tokens")?,
                to_sql_i64(input.output_tokens, "agent output tokens")?,
                to_sql_i64(input.total_tokens, "agent total tokens")?,
                to_sql_i64(input.compaction_count, "agent compaction count")?,
                input
                    .compacted_through_ordinal
                    .map(|value| to_sql_i64(value, "compacted message ordinal"))
                    .transpose()?,
                timestamp,
                run_id,
                tool_call_id,
                arguments_sha256.as_slice(),
            ],
        )?;
        if changed != 1 {
            return Err(StorageError::PermissionNotExecutable {
                id: run_id.to_owned(),
                reason: "database write outcome transition CAS failed",
            });
        }
        transaction.commit()?;
        let run = load_run(&connection, run_id)?.ok_or_else(|| StorageError::OutcomeUnknown {
            operation: "fail unknown agent write",
            id: run_id.to_owned(),
        })?;
        Ok(FailedAgentRun { run, messages })
    }

    /// Creates an ownership-bound handle to one live completed retained result.
    ///
    /// # Errors
    ///
    /// Returns owner/result not-found, expiry, numeric-range, or `SQLite` failures.
    pub fn create_agent_result_handle(
        &self,
        session_id: &str,
        run_id: &str,
        result_id: &str,
        retention: Duration,
    ) -> Result<AgentResultHandle, StorageError> {
        if retention.is_zero() {
            return Err(StorageError::InvalidAgent(
                "result handle retention must be greater than zero",
            ));
        }
        let timestamp = now_millis()?;
        let retention_ms = i64::try_from(retention.as_millis())
            .map_err(|_| StorageError::NumericRange("result handle retention"))?;
        let requested_expiry = timestamp
            .checked_add(retention_ms)
            .ok_or(StorageError::NumericRange("result handle expiry"))?;
        self.create_agent_result_handle_at(
            session_id,
            run_id,
            result_id,
            timestamp,
            requested_expiry,
        )
    }

    fn create_agent_result_handle_at(
        &self,
        session_id: &str,
        run_id: &str,
        result_id: &str,
        timestamp: i64,
        requested_expiry: i64,
    ) -> Result<AgentResultHandle, StorageError> {
        if requested_expiry <= timestamp {
            return Err(StorageError::InvalidAgent(
                "result handle expiry must be after creation",
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let owner: Option<String> = transaction
            .query_row(
                "SELECT session_id FROM agent_runs WHERE id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .optional()?;
        match owner {
            None => return Err(StorageError::AgentRunNotFound(run_id.to_owned())),
            Some(owner) if owner != session_id => {
                return Err(StorageError::ResultHandleNotFound(run_id.to_owned()));
            }
            Some(_) => {}
        }
        let result_expiry: Option<i64> = transaction
            .query_row(
                "SELECT expires_at_ms FROM retained_results
                 WHERE id = ?1 AND state = 'complete' AND expires_at_ms > ?2",
                params![result_id, timestamp],
                |row| row.get(0),
            )
            .optional()?;
        let result_expiry =
            result_expiry.ok_or_else(|| StorageError::ResultNotFound(result_id.to_owned()))?;
        let expires_at_ms = requested_expiry.min(result_expiry);
        let handle = AgentResultHandle {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_owned(),
            run_id: run_id.to_owned(),
            result_id: result_id.to_owned(),
            created_at_ms: timestamp,
            expires_at_ms,
        };
        transaction.execute(
            "INSERT INTO agent_result_handles (
                id, session_id, run_id, result_id, created_at_ms, expires_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                handle.id,
                handle.session_id,
                handle.run_id,
                handle.result_id,
                handle.created_at_ms,
                handle.expires_at_ms,
            ],
        )?;
        transaction.commit()?;
        Ok(handle)
    }

    /// Resolves a handle only for its exact session/run owner while both the
    /// handle and retained result remain live.
    ///
    /// # Errors
    ///
    /// Returns not-found/expired, clock, or `SQLite` failures.
    pub fn resolve_agent_result_handle(
        &self,
        id: &str,
        session_id: &str,
        run_id: &str,
    ) -> Result<AgentResultHandle, StorageError> {
        self.resolve_agent_result_handle_at(id, session_id, run_id, now_millis()?)
    }

    fn resolve_agent_result_handle_at(
        &self,
        id: &str,
        session_id: &str,
        run_id: &str,
        timestamp: i64,
    ) -> Result<AgentResultHandle, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let handle = load_result_handle(&transaction, id)?
            .filter(|handle| handle.session_id == session_id && handle.run_id == run_id)
            .ok_or_else(|| StorageError::ResultHandleNotFound(id.to_owned()))?;
        let result_live = row_exists_two(
            &transaction,
            "SELECT EXISTS(
                SELECT 1 FROM retained_results
                WHERE id = ?1 AND state = 'complete' AND expires_at_ms > ?2
             )",
            &handle.result_id,
            timestamp,
        )?;
        if handle.expires_at_ms <= timestamp || !result_live {
            transaction.execute("DELETE FROM agent_result_handles WHERE id = ?1", [id])?;
            transaction.commit()?;
            return Err(StorageError::ResultHandleNotFound(id.to_owned()));
        }
        transaction.commit()?;
        Ok(handle)
    }

    /// Removes expired agent result handles without touching retained results.
    ///
    /// # Errors
    ///
    /// Returns clock or `SQLite` failures.
    pub fn purge_expired_agent_result_handles(&self) -> Result<usize, StorageError> {
        let timestamp = now_millis()?;
        let connection = self.connection()?;
        connection
            .execute(
                "DELETE FROM agent_result_handles WHERE expires_at_ms <= ?1",
                [timestamp],
            )
            .map_err(Into::into)
    }

    pub(crate) fn recover_agents_at(
        &self,
        timestamp: i64,
    ) -> Result<AgentRecoveryReport, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let write_outcomes_unknown = transaction.execute(
            "UPDATE agent_runs
             SET status = 'failed', error_code = 'database_outcome_unknown',
                 error_message = 'Database write outcome is unknown after runtime restart',
                 last_sequence = CASE
                     WHEN last_sequence < 9223372036854775807 THEN last_sequence + 1
                     ELSE last_sequence
                 END,
                 updated_at_ms = ?1, finished_at_ms = ?1
             WHERE status IN ('running', 'waiting_permission')
               AND write_in_flight_tool_call_id IS NOT NULL",
            [timestamp],
        )?;
        let restarted_runs_failed = transaction.execute(
            "UPDATE agent_runs
             SET status = 'failed', error_code = 'runtime_restarted',
                 error_message = 'Agent run interrupted by runtime restart',
                 last_sequence = CASE
                     WHEN last_sequence < 9223372036854775807 THEN last_sequence + 1
                     ELSE last_sequence
                 END,
                 updated_at_ms = ?1, finished_at_ms = ?1
             WHERE status IN ('running', 'waiting_permission')
               AND write_in_flight_tool_call_id IS NULL",
            [timestamp],
        )?;
        let permissions_revoked = transaction.execute(
            "UPDATE tool_permissions
             SET status = 'revoked', revision = revision + 1, updated_at_ms = ?1
             WHERE status IN ('pending', 'approved')",
            [timestamp],
        )?;
        let result_handles_removed = transaction.execute(
            "DELETE FROM agent_result_handles
             WHERE expires_at_ms <= ?1
                OR NOT EXISTS (
                    SELECT 1 FROM retained_results r
                    WHERE r.id = agent_result_handles.result_id
                      AND r.state = 'complete' AND r.expires_at_ms > ?1
                )",
            [timestamp],
        )?;
        transaction.commit()?;
        Ok(AgentRecoveryReport {
            runs_failed: write_outcomes_unknown + restarted_runs_failed,
            write_outcomes_unknown,
            permissions_revoked,
            result_handles_removed,
        })
    }
}

type RawSession = (String, String, String, Option<String>, i64, i64, i64);
type RawMessage = (
    String,
    String,
    Option<String>,
    i64,
    String,
    Option<i64>,
    String,
    i64,
    i64,
);
type RawRun = (
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,
    Option<String>,
    Option<Vec<u8>>,
    i64,
    Option<i64>,
    i64,
    i64,
    Option<i64>,
    Option<i64>,
);
type RawPermission = (
    String,
    String,
    String,
    String,
    Vec<u8>,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    Option<i64>,
);
type RawResultHandle = (String, String, String, String, i64, i64);

fn load_session(
    connection: &Connection,
    id: &str,
) -> Result<Option<AgentSessionRecord>, StorageError> {
    connection
        .query_row(
            "SELECT id, title, provider_id, datasource_id, revision, created_at_ms, updated_at_ms
             FROM agent_sessions WHERE id = ?1",
            [id],
            raw_session,
        )
        .optional()
        .map_err(StorageError::from)?
        .map(decode_session)
        .transpose()
}

fn raw_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawSession> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

fn decode_session(raw: RawSession) -> Result<AgentSessionRecord, StorageError> {
    Ok(AgentSessionRecord {
        id: raw.0,
        title: raw.1,
        provider_id: raw.2,
        datasource_id: raw.3,
        revision: from_sql_u64(raw.4, "agent session revision")?,
        created_at_ms: raw.5,
        updated_at_ms: raw.6,
    })
}

fn raw_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawMessage> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ))
}

fn decode_message(raw: RawMessage) -> Result<AgentMessageRecord, StorageError> {
    Ok(AgentMessageRecord {
        id: raw.0,
        session_id: raw.1,
        run_id: raw.2,
        ordinal: from_sql_u64(raw.3, "agent message ordinal")?,
        role: AgentMessageRole::from_persisted(&raw.4)?,
        summary_through_ordinal: raw
            .5
            .map(|value| from_sql_u64(value, "summary coverage ordinal"))
            .transpose()?,
        content_json: raw.6,
        content_bytes: from_sql_u64(raw.7, "agent message bytes")?,
        created_at_ms: raw.8,
    })
}

fn load_message(
    connection: &Connection,
    id: &str,
) -> Result<Option<AgentMessageRecord>, StorageError> {
    let raw = connection
        .query_row(
            "SELECT id, session_id, run_id, ordinal, role, summary_through_ordinal,
                    content_json, content_bytes, created_at_ms
             FROM agent_messages WHERE id = ?1",
            [id],
            raw_message,
        )
        .optional()?;
    raw.map(decode_message).transpose()
}

fn raw_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRun> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
        row.get(18)?,
        row.get(19)?,
        row.get(20)?,
        row.get(21)?,
    ))
}

fn decode_run(raw: RawRun) -> Result<AgentRunRecord, StorageError> {
    let write_in_flight_arguments_sha256 = raw
        .15
        .map(|digest| {
            digest.try_into().map_err(|_| {
                StorageError::InvalidAgent("database write argument digest is invalid")
            })
        })
        .transpose()?;
    Ok(AgentRunRecord {
        id: raw.0,
        session_id: raw.1,
        sql_permission_mode: SqlPermissionMode::from_persisted(&raw.2)?,
        status: AgentRunStatus::from_persisted(&raw.3)?,
        last_sequence: from_sql_u64(raw.4, "agent event sequence")?,
        model_rounds: from_sql_u64(raw.5, "agent model rounds")?,
        tool_calls: from_sql_u64(raw.6, "agent tool calls")?,
        input_tokens: from_sql_u64(raw.7, "agent input tokens")?,
        output_tokens: from_sql_u64(raw.8, "agent output tokens")?,
        total_tokens: from_sql_u64(raw.9, "agent total tokens")?,
        message_id: raw.10,
        error_code: raw.11,
        error_message: raw.12,
        cancel_requested: raw.13,
        write_in_flight_tool_call_id: raw.14,
        write_in_flight_arguments_sha256,
        compaction_count: from_sql_u64(raw.16, "agent compaction count")?,
        compacted_through_ordinal: raw
            .17
            .map(|value| from_sql_u64(value, "compacted message ordinal"))
            .transpose()?,
        created_at_ms: raw.18,
        updated_at_ms: raw.19,
        started_at_ms: raw.20,
        finished_at_ms: raw.21,
    })
}

fn load_run(connection: &Connection, id: &str) -> Result<Option<AgentRunRecord>, StorageError> {
    let raw = connection
        .query_row(
            "SELECT id, session_id, sql_permission_mode, status, last_sequence,
                    model_rounds, tool_calls, input_tokens, output_tokens, total_tokens,
                    message_id, error_code, error_message, cancel_requested,
                    write_in_flight_tool_call_id, write_in_flight_arguments_sha256,
                    compaction_count, compacted_through_ordinal, created_at_ms,
                    updated_at_ms, started_at_ms, finished_at_ms
             FROM agent_runs WHERE id = ?1",
            [id],
            raw_run,
        )
        .optional()?;
    raw.map(decode_run).transpose()
}

fn raw_permission(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawPermission> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
    ))
}

fn decode_permission(raw: RawPermission) -> Result<ToolPermissionRecord, StorageError> {
    let arguments_sha256: [u8; 32] = raw
        .4
        .try_into()
        .map_err(|_| StorageError::InvalidAgent("permission argument digest is invalid"))?;
    Ok(ToolPermissionRecord {
        id: raw.0,
        run_id: raw.1,
        tool_call_id: raw.2,
        tool_name: raw.3,
        arguments_sha256,
        summary: raw.5,
        status: ToolPermissionStatus::from_persisted(&raw.6)?,
        revision: from_sql_u64(raw.7, "tool permission revision")?,
        expires_at_ms: raw.8,
        created_at_ms: raw.9,
        updated_at_ms: raw.10,
        consumed_at_ms: raw.11,
    })
}

fn load_permission(
    connection: &Connection,
    id: &str,
) -> Result<Option<ToolPermissionRecord>, StorageError> {
    let raw = connection
        .query_row(
            "SELECT id, run_id, tool_call_id, tool_name, arguments_sha256,
                    summary, status, revision, expires_at_ms, created_at_ms,
                    updated_at_ms, consumed_at_ms
             FROM tool_permissions WHERE id = ?1",
            [id],
            raw_permission,
        )
        .optional()?;
    raw.map(decode_permission).transpose()
}

fn load_result_handle(
    connection: &Connection,
    id: &str,
) -> Result<Option<AgentResultHandle>, StorageError> {
    let raw: Option<RawResultHandle> = connection
        .query_row(
            "SELECT id, session_id, run_id, result_id, created_at_ms, expires_at_ms
             FROM agent_result_handles WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()?;
    Ok(raw.map(|raw| AgentResultHandle {
        id: raw.0,
        session_id: raw.1,
        run_id: raw.2,
        result_id: raw.3,
        created_at_ms: raw.4,
        expires_at_ms: raw.5,
    }))
}

struct NormalizedAgentRunMessage {
    role: AgentMessageRole,
    summary_through_ordinal: Option<u64>,
    content_json: String,
    content_bytes: u64,
}

fn validate_session_title(title: &str) -> Result<(), StorageError> {
    if title.trim().is_empty() || title.len() > MAX_AGENT_SESSION_TITLE_BYTES {
        return Err(StorageError::InvalidAgent(
            "session title must be non-empty and at most 512 UTF-8 bytes",
        ));
    }
    Ok(())
}

fn text_message_json(connection: &Connection, text: &str) -> Result<(String, u64), StorageError> {
    if text.trim().is_empty() {
        return Err(StorageError::InvalidAgent(
            "agent text message must be non-empty",
        ));
    }
    let content_json: String = connection.query_row(
        "SELECT json_array(json_object('type', 'text', 'text', ?1))",
        [text],
        |row| row.get(0),
    )?;
    normalize_message_json(connection, &content_json)
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn append_message_in_transaction(
    connection: &Connection,
    session_id: &str,
    run_id: Option<&str>,
    role: AgentMessageRole,
    summary_through_ordinal: Option<u64>,
    content_json: &str,
    content_bytes: u64,
    timestamp: i64,
) -> Result<AgentMessageRecord, StorageError> {
    if !row_exists(
        connection,
        "SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE id = ?1)",
        session_id,
    )? {
        return Err(StorageError::AgentSessionNotFound(session_id.to_owned()));
    }
    if let Some(run_id) = run_id {
        let owned: bool = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM agent_runs WHERE id = ?1 AND session_id = ?2
             )",
            params![run_id, session_id],
            |row| row.get(0),
        )?;
        if !owned {
            return Err(StorageError::AgentRunNotFound(run_id.to_owned()));
        }
    }
    match (role, summary_through_ordinal) {
        (AgentMessageRole::Summary, Some(_))
        | (
            AgentMessageRole::System
            | AgentMessageRole::User
            | AgentMessageRole::Assistant
            | AgentMessageRole::Tool,
            None,
        ) => {}
        _ => {
            return Err(StorageError::InvalidAgent(
                "summary coverage is required only for summary messages",
            ));
        }
    }
    validate_message_role(role, content_json)?;
    let (count, bytes, maximum): (i64, i64, Option<i64>) = connection.query_row(
        "SELECT COUNT(*), COALESCE(SUM(content_bytes), 0), MAX(ordinal)
         FROM agent_messages WHERE session_id = ?1",
        [session_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let count = from_sql_u64(count, "agent message count")?;
    let bytes = from_sql_u64(bytes, "agent message total bytes")?;
    if count >= MAX_AGENT_MESSAGES_PER_SESSION {
        return Err(StorageError::AgentQuotaExceeded {
            resource: "session message count",
            limit: MAX_AGENT_MESSAGES_PER_SESSION,
        });
    }
    if bytes
        .checked_add(content_bytes)
        .ok_or(StorageError::NumericRange("agent message total bytes"))?
        > MAX_AGENT_MESSAGE_BYTES_PER_SESSION
    {
        return Err(StorageError::AgentQuotaExceeded {
            resource: "session message bytes",
            limit: MAX_AGENT_MESSAGE_BYTES_PER_SESSION,
        });
    }
    let ordinal = maximum.map_or(Ok(0_u64), |value| {
        from_sql_u64(value, "agent message ordinal")?
            .checked_add(1)
            .ok_or(StorageError::NumericRange("agent message ordinal"))
    })?;
    if summary_through_ordinal.is_some_and(|coverage| coverage >= ordinal) {
        return Err(StorageError::InvalidAgent(
            "summary coverage must precede the summary message",
        ));
    }
    let record = AgentMessageRecord {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.to_owned(),
        run_id: run_id.map(ToOwned::to_owned),
        ordinal,
        role,
        summary_through_ordinal,
        content_json: content_json.to_owned(),
        content_bytes,
        created_at_ms: timestamp,
    };
    connection.execute(
        "INSERT INTO agent_messages (
            id, session_id, run_id, ordinal, role, summary_through_ordinal,
            content_json, content_bytes, created_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            record.id,
            record.session_id,
            record.run_id,
            to_sql_i64(record.ordinal, "agent message ordinal")?,
            record.role.as_str(),
            record
                .summary_through_ordinal
                .map(|value| to_sql_i64(value, "summary coverage ordinal"))
                .transpose()?,
            record.content_json,
            to_sql_i64(record.content_bytes, "agent message bytes")?,
            record.created_at_ms,
        ],
    )?;
    connection.execute(
        "UPDATE agent_sessions SET updated_at_ms = ?1 WHERE id = ?2",
        params![timestamp, session_id],
    )?;
    Ok(record)
}

fn normalize_agent_run_messages(
    connection: &Connection,
    messages: &[AgentRunMessage],
    require_final_assistant: bool,
) -> Result<Vec<NormalizedAgentRunMessage>, StorageError> {
    if require_final_assistant
        && messages.last().map(|message| message.role) != Some(AgentMessageRole::Assistant)
    {
        return Err(StorageError::InvalidAgent(
            "completed agent run requires a final assistant message",
        ));
    }
    messages
        .iter()
        .map(|message| {
            if matches!(
                message.role,
                AgentMessageRole::System | AgentMessageRole::User | AgentMessageRole::Summary
            ) {
                return Err(StorageError::InvalidAgent(
                    "run message batches may contain only assistant or tool messages",
                ));
            }
            let (content_json, content_bytes) =
                normalize_message_json(connection, &message.content_json)?;
            Ok(NormalizedAgentRunMessage {
                role: message.role,
                summary_through_ordinal: message.summary_through_ordinal,
                content_json,
                content_bytes,
            })
        })
        .collect()
}

fn validate_run_update(update: &AgentRunUpdate) -> Result<(), StorageError> {
    if update.status.is_terminal() {
        return Err(StorageError::InvalidAgent(
            "terminal agent runs require an atomic terminal-message API",
        ));
    }
    validate_progress_values(
        update.last_sequence,
        update.model_rounds,
        update.tool_calls,
        update.input_tokens,
        update.output_tokens,
        update.total_tokens,
        update.compaction_count,
        update.compacted_through_ordinal,
    )
}

fn validate_complete_agent_run(input: &CompleteAgentRun) -> Result<(), StorageError> {
    validate_progress_values(
        input.last_sequence,
        input.model_rounds,
        input.tool_calls,
        input.input_tokens,
        input.output_tokens,
        input.total_tokens,
        input.compaction_count,
        input.compacted_through_ordinal,
    )
}

fn validate_fail_agent_run(input: &FailAgentRun) -> Result<(), StorageError> {
    validate_safe_error(&input.error_code, input.error_message.as_deref())?;
    validate_progress_values(
        input.last_sequence,
        input.model_rounds,
        input.tool_calls,
        input.input_tokens,
        input.output_tokens,
        input.total_tokens,
        input.compaction_count,
        input.compacted_through_ordinal,
    )
}

fn validate_cancel_agent_run(input: &CancelAgentRun) -> Result<(), StorageError> {
    validate_progress_values(
        input.last_sequence,
        input.model_rounds,
        input.tool_calls,
        input.input_tokens,
        input.output_tokens,
        input.total_tokens,
        input.compaction_count,
        input.compacted_through_ordinal,
    )
}

fn validate_unknown_agent_write(input: &UnknownAgentWrite) -> Result<(), StorageError> {
    validate_progress_values(
        input.last_sequence,
        input.model_rounds,
        input.tool_calls,
        input.input_tokens,
        input.output_tokens,
        input.total_tokens,
        input.compaction_count,
        input.compacted_through_ordinal,
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_progress_values(
    last_sequence: u64,
    model_rounds: u64,
    tool_calls: u64,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    compaction_count: u64,
    compacted_through_ordinal: Option<u64>,
) -> Result<(), StorageError> {
    for (value, label) in [
        (last_sequence, "agent event sequence"),
        (model_rounds, "agent model rounds"),
        (tool_calls, "agent tool calls"),
        (input_tokens, "agent input tokens"),
        (output_tokens, "agent output tokens"),
        (total_tokens, "agent total tokens"),
        (compaction_count, "agent compaction count"),
    ] {
        to_sql_i64(value, label)?;
    }
    if last_sequence == 0 {
        return Err(StorageError::InvalidAgent(
            "agent event sequence must be greater than zero",
        ));
    }
    let minimum_total = input_tokens
        .checked_add(output_tokens)
        .ok_or(StorageError::NumericRange("agent total tokens"))?;
    if total_tokens < minimum_total {
        return Err(StorageError::InvalidAgent(
            "agent total tokens cannot be less than input plus output tokens",
        ));
    }
    if let Some(ordinal) = compacted_through_ordinal {
        to_sql_i64(ordinal, "compacted message ordinal")?;
    }
    if (compaction_count == 0) != compacted_through_ordinal.is_none() {
        return Err(StorageError::InvalidAgent(
            "agent compaction count and coverage ordinal must be present together",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_run_progress(
    current: &AgentRunRecord,
    last_sequence: u64,
    model_rounds: u64,
    tool_calls: u64,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    compaction_count: u64,
    compacted_through_ordinal: Option<u64>,
) -> Result<(), StorageError> {
    validate_progress_values(
        last_sequence,
        model_rounds,
        tool_calls,
        input_tokens,
        output_tokens,
        total_tokens,
        compaction_count,
        compacted_through_ordinal,
    )?;
    if last_sequence <= current.last_sequence
        || model_rounds < current.model_rounds
        || tool_calls < current.tool_calls
        || input_tokens < current.input_tokens
        || output_tokens < current.output_tokens
        || total_tokens < current.total_tokens
        || compaction_count < current.compaction_count
        || current.compacted_through_ordinal.is_some()
            && compacted_through_ordinal < current.compacted_through_ordinal
    {
        return Err(StorageError::InvalidAgent(
            "agent run sequence, counters, usage, and compaction must be monotonic",
        ));
    }
    Ok(())
}

fn validate_compaction_unchanged(
    current: &AgentRunRecord,
    compaction_count: u64,
    compacted_through_ordinal: Option<u64>,
) -> Result<(), StorageError> {
    if compaction_count != current.compaction_count
        || compacted_through_ordinal != current.compacted_through_ordinal
    {
        return Err(StorageError::InvalidAgent(
            "agent compaction state requires the dedicated atomic API",
        ));
    }
    Ok(())
}

fn load_session_compaction_coverage(
    connection: &Connection,
    session_id: &str,
) -> Result<Option<u64>, StorageError> {
    let (exists, maximum): (bool, Option<i64>) = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE id = ?1), MAX(coverage)
         FROM (
            SELECT compacted_through_ordinal AS coverage
            FROM agent_runs WHERE session_id = ?1
            UNION ALL
            SELECT summary_through_ordinal AS coverage
            FROM agent_messages WHERE session_id = ?1
         )",
        [session_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if !exists {
        return Err(StorageError::AgentSessionNotFound(session_id.to_owned()));
    }
    maximum
        .map(|value| from_sql_u64(value, "compacted message ordinal"))
        .transpose()
}

fn validate_run_compaction_boundary(
    connection: &Connection,
    run_id: &str,
    compacted_through_ordinal: Option<u64>,
) -> Result<(), StorageError> {
    let Some(compacted_through_ordinal) = compacted_through_ordinal else {
        return Ok(());
    };
    let (count, initiating_ordinal): (i64, Option<i64>) = connection.query_row(
        "SELECT COUNT(*), MIN(ordinal)
         FROM agent_messages WHERE run_id = ?1 AND role = 'user'",
        [run_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if count != 1 {
        return Err(StorageError::InvalidAgent(
            "persisted agent run initiating message is invalid",
        ));
    }
    let initiating_ordinal = initiating_ordinal
        .ok_or(StorageError::InvalidAgent(
            "persisted agent run initiating message is invalid",
        ))
        .and_then(|value| from_sql_u64(value, "agent message ordinal"))?;
    if compacted_through_ordinal >= initiating_ordinal {
        return Err(StorageError::InvalidAgent(
            "agent compaction coverage must precede its initiating user message",
        ));
    }
    Ok(())
}

fn validate_safe_error(code: &str, message: Option<&str>) -> Result<(), StorageError> {
    if code.trim().is_empty()
        || code.len() > MAX_RUN_ERROR_CODE_BYTES
        || message.is_some_and(|value| value.len() > MAX_RUN_ERROR_MESSAGE_BYTES)
    {
        return Err(StorageError::InvalidAgent(
            "agent run error code must be non-empty and error fields must be bounded",
        ));
    }
    Ok(())
}

fn validate_permission_request(input: &RequestToolPermission) -> Result<(), StorageError> {
    if input.tool_call_id.trim().is_empty() || input.tool_call_id.len() > MAX_TOOL_CALL_ID_BYTES {
        return Err(StorageError::InvalidAgent(
            "tool call id must be non-empty and at most 512 UTF-8 bytes",
        ));
    }
    if input.tool_name.trim().is_empty() || input.tool_name.len() > MAX_TOOL_NAME_BYTES {
        return Err(StorageError::InvalidAgent(
            "tool name must be non-empty and at most 512 UTF-8 bytes",
        ));
    }
    if input.summary.trim().is_empty() || input.summary.len() > MAX_PERMISSION_SUMMARY_BYTES {
        return Err(StorageError::InvalidAgent(
            "permission summary must be non-empty and at most 4096 UTF-8 bytes",
        ));
    }
    Ok(())
}

fn normalize_message_json(
    connection: &Connection,
    content_json: &str,
) -> Result<(String, u64), StorageError> {
    let raw_bytes = to_u64(content_json.len(), "agent message bytes")?;
    if raw_bytes == 0 || raw_bytes > MAX_AGENT_MESSAGE_BYTES {
        return Err(StorageError::AgentQuotaExceeded {
            resource: "single message bytes",
            limit: MAX_AGENT_MESSAGE_BYTES,
        });
    }
    let valid: i64 =
        connection.query_row("SELECT json_valid(?1)", [content_json], |row| row.get(0))?;
    if valid != 1 {
        return Err(StorageError::InvalidAgent(
            "message must be a strict visible JSON array without hidden reasoning or raw deltas",
        ));
    }
    let visible: i64 = connection.query_row(
        "SELECT json_type(?1) = 'array'
                AND json_array_length(?1) > 0
                AND NOT EXISTS (
                    SELECT 1 FROM json_each(?1)
                    WHERE json_type(value) <> 'object'
                       OR json_extract(value, '$.type') NOT IN (
                           'text', 'tool_calls', 'tool_result'
                       )
                )
                AND NOT EXISTS (
                    SELECT 1 FROM json_tree(?1)
                    WHERE lower(CAST(key AS TEXT)) IN (
                        'hidden_reasoning', 'reasoning_content',
                        'raw_provider_delta', 'provider_delta',
                        'rows', 'result_rows', 'full_result',
                        'api_key', 'apikey', 'authorization'
                    )
                )",
        [content_json],
        |row| row.get(0),
    )?;
    if visible != 1 {
        return Err(StorageError::InvalidAgent(
            "message must be a strict visible JSON array without hidden reasoning or raw deltas",
        ));
    }
    let parsed: Value = serde_json::from_str(content_json).map_err(|_| {
        StorageError::InvalidAgent("message content does not match the canonical contract")
    })?;
    let blocks: Vec<ContractMessageContent> =
        serde_json::from_value(parsed.clone()).map_err(|_| {
            StorageError::InvalidAgent("message content does not match the canonical contract")
        })?;
    let normalized = serde_json::to_value(&blocks).map_err(|error| {
        StorageError::Integrity(format!(
            "canonical agent message serialization failed: {error}"
        ))
    })?;
    if normalized != parsed {
        return Err(StorageError::InvalidAgent(
            "message contains fields outside the canonical contract",
        ));
    }
    let canonical = serde_json::to_string(&blocks).map_err(|error| {
        StorageError::Integrity(format!(
            "canonical agent message serialization failed: {error}"
        ))
    })?;
    let canonical_bytes = to_u64(canonical.len(), "agent message bytes")?;
    if canonical_bytes == 0 || canonical_bytes > MAX_AGENT_MESSAGE_BYTES {
        return Err(StorageError::AgentQuotaExceeded {
            resource: "single message bytes",
            limit: MAX_AGENT_MESSAGE_BYTES,
        });
    }
    Ok((canonical, canonical_bytes))
}

fn validate_message_role(role: AgentMessageRole, content_json: &str) -> Result<(), StorageError> {
    let blocks: Vec<ContractMessageContent> = serde_json::from_str(content_json).map_err(|_| {
        StorageError::InvalidAgent("message content does not match the canonical contract")
    })?;
    let valid = !blocks.is_empty() && match role {
        AgentMessageRole::System | AgentMessageRole::User => {
            blocks.iter().all(
                |block| matches!(block, ContractMessageContent::Text { text } if !text.is_empty()),
            )
        }
        AgentMessageRole::Summary => blocks.iter().all(
            |block| matches!(block, ContractMessageContent::Text { text } if !text.trim().is_empty()),
        ),
        AgentMessageRole::Assistant => blocks.iter().all(|block| match block {
            ContractMessageContent::Text { text } => !text.is_empty(),
            ContractMessageContent::ToolCalls { calls } => {
                !calls.is_empty() && calls.iter().all(valid_contract_tool_call)
            }
            ContractMessageContent::ToolResult { .. } => false,
        }),
        AgentMessageRole::Tool => matches!(
            blocks.as_slice(),
            [ContractMessageContent::ToolResult {
                tool_call_id,
                name,
                ..
            }] if !tool_call_id.trim().is_empty() && !name.trim().is_empty()
        ),
    };
    if !valid {
        return Err(StorageError::InvalidAgent(
            "message content blocks do not match the persisted role",
        ));
    }
    Ok(())
}

fn valid_contract_tool_call(call: &ContractToolCall) -> bool {
    !call.id.trim().is_empty()
        && !call.name.trim().is_empty()
        && serde_json::from_str::<Value>(&call.arguments_json)
            .is_ok_and(|arguments| arguments.is_object())
}

fn expire_permission(
    connection: &Connection,
    current: &ToolPermissionRecord,
    timestamp: i64,
) -> Result<(), StorageError> {
    if matches!(
        current.status,
        ToolPermissionStatus::Pending | ToolPermissionStatus::Approved
    ) {
        connection.execute(
            "UPDATE tool_permissions
             SET status = 'expired', revision = revision + 1, updated_at_ms = ?1
             WHERE id = ?2 AND revision = ?3 AND status IN ('pending', 'approved')",
            params![
                timestamp,
                current.id,
                to_sql_i64(current.revision, "tool permission revision")?,
            ],
        )?;
    }
    Ok(())
}

fn row_exists(connection: &Connection, sql: &str, value: &str) -> Result<bool, StorageError> {
    connection
        .query_row(sql, [value], |row| row.get::<_, bool>(0))
        .map_err(Into::into)
}

fn row_exists_two(
    connection: &Connection,
    sql: &str,
    value: &str,
    second: i64,
) -> Result<bool, StorageError> {
    connection
        .query_row(sql, params![value, second], |row| row.get::<_, bool>(0))
        .map_err(Into::into)
}

fn run_state_conflict(id: &str, expected: AgentRunStatus, actual: AgentRunStatus) -> StorageError {
    StorageError::AgentStateConflict {
        id: id.to_owned(),
        expected: expected.as_str(),
        actual: actual.as_str(),
    }
}

fn permission_revision_conflict(id: &str, expected: u64, actual: Option<u64>) -> StorageError {
    StorageError::PermissionRevisionConflict {
        id: id.to_owned(),
        expected,
        actual,
    }
}

fn to_sql_i64(value: u64, label: &'static str) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::NumericRange(label))
}

fn from_sql_u64(value: i64, label: &'static str) -> Result<u64, StorageError> {
    u64::try_from(value).map_err(|_| StorageError::NumericRange(label))
}

fn to_u64(value: usize, label: &'static str) -> Result<u64, StorageError> {
    u64::try_from(value).map_err(|_| StorageError::NumericRange(label))
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Barrier},
        thread,
        time::Duration,
    };

    use chat2db_engine_protocol::wire;
    use tempfile::TempDir;

    use super::{
        AgentCompaction, AgentMessageRole, AgentRunMessage, AgentRunStatus, AgentRunUpdate,
        AppendAgentMessage, CancelAgentRun, CompactAgentRun, CompleteAgentRun, CreateAgentSession,
        FailAgentRun, MAX_AGENT_MESSAGE_BYTES, MAX_AGENT_MESSAGE_BYTES_PER_SESSION,
        MAX_AGENT_MESSAGES_PER_SESSION, RequestToolPermission, SqlPermissionMode, StartAgentRun,
        ToolPermissionDecision, ToolPermissionStatus, UnknownAgentWrite, UpdateAgentSession,
    };
    use crate::{
        CreateDatasource, CreateProviderProfile, PageRequest, ProviderKind, SecretChange,
        SecretRef, SecretValue, SecretVault, SecretVaultError, Storage, StorageError,
        UpdateDatasource,
    };

    #[derive(Debug)]
    struct EmptyVault;

    impl SecretVault for EmptyVault {
        fn probe(&self) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn create(
            &self,
            _reference: &SecretRef,
            _value: &SecretValue,
        ) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn get(&self, _reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
            Ok(None)
        }

        fn delete(&self, _reference: &SecretRef) -> Result<(), SecretVaultError> {
            Ok(())
        }
    }

    fn provider_input(name: &str) -> CreateProviderProfile {
        CreateProviderProfile {
            name: name.to_owned(),
            kind: ProviderKind::OpenAiCompatible,
            base_url: "https://provider.example/v1".to_owned(),
            model: "model-1".to_owned(),
            context_window_tokens: 128_000,
            max_output_tokens: 8_192,
        }
    }

    fn session_input(provider_id: &str) -> CreateAgentSession {
        CreateAgentSession {
            title: "Session".to_owned(),
            provider_id: provider_id.to_owned(),
            datasource_id: None,
            system_prompt: None,
        }
    }

    fn setup(directory: &TempDir) -> (Storage, String) {
        let storage = Storage::open(directory.path(), Arc::new(EmptyVault)).expect("storage opens");
        let provider = storage
            .create_provider_profile(provider_input("primary"), None)
            .expect("provider creates");
        (storage, provider.id)
    }

    fn create_running_run(storage: &Storage, session_id: &str) -> super::AgentRunRecord {
        storage
            .start_agent_run(
                session_id,
                StartAgentRun {
                    user_message: "hello".to_owned(),
                    sql_permission_mode: SqlPermissionMode::AskBeforeWrite,
                },
            )
            .expect("run starts")
            .run
    }

    fn create_compactable_run(storage: &Storage, session_id: &str) -> super::StartedAgentRun {
        for (role, text) in [
            (AgentMessageRole::User, "previous question"),
            (AgentMessageRole::Assistant, "previous answer"),
        ] {
            storage
                .append_agent_message(
                    session_id,
                    AppendAgentMessage {
                        role,
                        summary_through_ordinal: None,
                        content_json: message_json(text),
                    },
                )
                .expect("historical message appends");
        }
        storage
            .start_agent_run(
                session_id,
                StartAgentRun {
                    user_message: "current question".to_owned(),
                    sql_permission_mode: SqlPermissionMode::AskBeforeWrite,
                },
            )
            .expect("run starts")
    }

    fn permission_input(
        tool_call_id: &str,
        digest: [u8; 32],
        last_sequence: u64,
    ) -> RequestToolPermission {
        RequestToolPermission {
            tool_call_id: tool_call_id.to_owned(),
            tool_name: "sql_write".to_owned(),
            arguments_sha256: digest,
            summary: "Execute one SQL write".to_owned(),
            last_sequence,
            retention: Duration::from_secs(60),
        }
    }

    fn message_json(text: &str) -> String {
        format!(r#"[{{"type":"text","text":"{text}"}}]"#)
    }

    fn compaction_input(
        last_sequence: u64,
        coverage: Option<u64>,
        summary: Option<&str>,
    ) -> CompactAgentRun {
        let compaction = match (coverage, summary) {
            (None, None) => AgentCompaction::NoOp,
            (Some(compacted_through_ordinal), None) => AgentCompaction::DeterministicTrim {
                compacted_through_ordinal,
            },
            (Some(compacted_through_ordinal), Some(summary)) => AgentCompaction::Summary {
                compacted_through_ordinal,
                content_json: message_json(summary),
            },
            (None, Some(_)) => panic!("summary compaction requires coverage"),
        };
        CompactAgentRun {
            last_sequence,
            model_rounds: 0,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            compaction,
        }
    }

    fn result_schema() -> wire::QueryStarted {
        wire::QueryStarted {
            columns: vec![wire::JdbcColumn {
                ordinal: 1,
                label: "value".to_owned(),
                name: "value".to_owned(),
                jdbc_type: 12,
                jdbc_type_name: "VARCHAR".to_owned(),
                value_type: wire::JdbcValueType::Text as i32,
                nullability: wire::ColumnNullability::Nullable as i32,
                ..Default::default()
            }],
        }
    }

    fn completed_result(storage: &Storage) -> String {
        storage
            .begin_result(&result_schema(), Duration::from_secs(60))
            .expect("result begins")
            .finish(&wire::QueryCompleted {
                row_count: 0,
                truncated_by_max_rows: false,
                truncated_by_max_result_bytes: false,
            })
            .expect("result completes")
            .id
    }

    #[test]
    fn session_system_prompt_is_ordinal_zero_and_revision_cas_is_enforced() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let second_provider = storage
            .create_provider_profile(provider_input("secondary"), None)
            .expect("second provider creates");
        let session = storage
            .create_agent_session(CreateAgentSession {
                title: "Initial title".to_owned(),
                provider_id,
                datasource_id: None,
                system_prompt: Some("rules".to_owned()),
            })
            .expect("session creates");
        assert_eq!(session.revision, 1);
        let messages = storage
            .list_agent_messages(&session.id, 0, 10)
            .expect("messages list");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].ordinal, 0);
        assert_eq!(messages[0].role, AgentMessageRole::System);
        assert_eq!(messages[0].content_json, message_json("rules"));
        assert!(!messages[0].id.is_empty());
        assert!(messages[0].run_id.is_none());

        let appended = storage
            .append_agent_message(
                &session.id,
                AppendAgentMessage {
                    role: AgentMessageRole::User,
                    summary_through_ordinal: None,
                    content_json: " [ { \"type\" : \"text\", \"text\" : \"hello\" } ] ".to_owned(),
                },
            )
            .expect("message appends");
        assert_eq!(appended.ordinal, 1);
        assert_eq!(appended.content_json, message_json("hello"));

        let updated = storage
            .update_agent_session(
                &session.id,
                session.revision,
                UpdateAgentSession {
                    title: "Renamed".to_owned(),
                    provider_id: second_provider.id,
                    datasource_id: None,
                },
            )
            .expect("session updates");
        assert_eq!(updated.revision, 2);
        let stale = storage
            .update_agent_session(
                &session.id,
                session.revision,
                UpdateAgentSession {
                    title: "Stale".to_owned(),
                    provider_id: updated.provider_id.clone(),
                    datasource_id: None,
                },
            )
            .expect_err("stale session revision fails");
        assert!(matches!(
            stale,
            StorageError::AgentSessionRevisionConflict {
                actual: Some(2),
                ..
            }
        ));
    }

    #[test]
    fn session_delete_enforces_revision_and_active_run_guards() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        let updated = storage
            .update_agent_session(
                &session.id,
                session.revision,
                UpdateAgentSession {
                    title: "Updated".to_owned(),
                    provider_id: provider_id.clone(),
                    datasource_id: None,
                },
            )
            .expect("session updates");

        assert!(matches!(
            storage.delete_agent_session(&session.id, session.revision),
            Err(StorageError::AgentSessionRevisionConflict {
                actual: Some(2),
                ..
            })
        ));

        let run = create_running_run(&storage, &session.id);
        assert!(matches!(
            storage.delete_agent_session(&session.id, updated.revision),
            Err(StorageError::AgentSessionBusy(_))
        ));
        assert!(
            storage
                .get_agent_session(&session.id)
                .expect("session reads")
                .is_some()
        );
        assert!(storage.get_agent_run(&run.id).expect("run reads").is_some());
    }

    #[test]
    fn concurrent_session_delete_has_one_winner_and_a_not_found_loser() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        let barrier = Arc::new(Barrier::new(3));

        let outcomes = thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..2 {
                let storage = storage.clone();
                let barrier = barrier.clone();
                let session_id = session.id.clone();
                handles.push(scope.spawn(move || {
                    barrier.wait();
                    storage.delete_agent_session(&session_id, session.revision)
                }));
            }
            barrier.wait();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("delete thread joins"))
                .collect::<Vec<_>>()
        });

        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| {
                    matches!(outcome, Err(StorageError::AgentSessionNotFound(id)) if id == &session.id)
                })
                .count(),
            1
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn session_delete_cascades_agent_state_but_keeps_shared_resources() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let datasource = storage
            .create_datasource(
                CreateDatasource {
                    name: "Shared database".to_owned(),
                    driver_id: "driver-a".to_owned(),
                },
                None,
            )
            .expect("datasource creates");
        let session = storage
            .create_agent_session(CreateAgentSession {
                title: "Deletable session".to_owned(),
                provider_id: provider_id.clone(),
                datasource_id: Some(datasource.id.clone()),
                system_prompt: Some("private system prompt".to_owned()),
            })
            .expect("session creates");
        let run = create_running_run(&storage, &session.id);
        let result_id = completed_result(&storage);
        let handle = storage
            .create_agent_result_handle(&session.id, &run.id, &result_id, Duration::from_secs(60))
            .expect("result handle creates");
        let digest = [11_u8; 32];
        let permission = storage
            .create_tool_permission(&run.id, permission_input("delete-cascade", digest, 2))
            .expect("permission creates");
        storage
            .decide_tool_permission(
                &permission.id,
                permission.revision,
                &run.id,
                "delete-cascade",
                digest,
                3,
                ToolPermissionDecision::Deny,
            )
            .expect("permission denies");
        storage
            .complete_agent_run(
                &run.id,
                AgentRunStatus::Running,
                CompleteAgentRun {
                    last_sequence: 4,
                    model_rounds: 1,
                    tool_calls: 1,
                    input_tokens: 1,
                    output_tokens: 1,
                    total_tokens: 2,
                    messages: vec![AgentRunMessage {
                        role: AgentMessageRole::Assistant,
                        summary_through_ordinal: None,
                        content_json: message_json("done"),
                    }],
                    compaction_count: 0,
                    compacted_through_ordinal: None,
                },
            )
            .expect("run completes");

        storage
            .delete_agent_session(&session.id, session.revision)
            .expect("session deletes");

        let counts: (i64, i64, i64, i64, i64) = storage
            .connection()
            .expect("connection opens")
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM agent_sessions WHERE id = ?1),
                    (SELECT COUNT(*) FROM agent_messages WHERE session_id = ?1),
                    (SELECT COUNT(*) FROM agent_runs WHERE session_id = ?1),
                    (SELECT COUNT(*) FROM tool_permissions WHERE run_id = ?2),
                    (SELECT COUNT(*) FROM agent_result_handles WHERE session_id = ?1)",
                rusqlite::params![session.id, run.id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("cascade counts read");
        assert_eq!(counts, (0, 0, 0, 0, 0));
        assert!(
            storage
                .get_provider_profile(&provider_id)
                .expect("provider reads")
                .is_some()
        );
        assert!(
            storage
                .get_datasource(&datasource.id)
                .expect("datasource reads")
                .is_some()
        );
        assert!(
            storage
                .result_metadata(&result_id)
                .expect("result metadata reads")
                .is_some()
        );
        assert!(matches!(
            storage.resolve_agent_result_handle(&handle.id, &session.id, &run.id),
            Err(StorageError::ResultHandleNotFound(_))
        ));
        assert!(matches!(
            storage.delete_agent_session(&session.id, session.revision),
            Err(StorageError::AgentSessionNotFound(_))
        ));
    }

    #[test]
    fn session_message_append_cannot_interleave_with_an_active_run() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        let run = create_running_run(&storage, &session.id);

        assert!(matches!(
            storage.append_agent_message(
                &session.id,
                AppendAgentMessage {
                    role: AgentMessageRole::Assistant,
                    summary_through_ordinal: None,
                    content_json: message_json("must not interleave"),
                },
            ),
            Err(StorageError::AgentSessionBusy(_))
        ));

        storage
            .complete_agent_run(
                &run.id,
                AgentRunStatus::Running,
                CompleteAgentRun {
                    last_sequence: 2,
                    model_rounds: 1,
                    tool_calls: 0,
                    input_tokens: 1,
                    output_tokens: 1,
                    total_tokens: 2,
                    messages: vec![AgentRunMessage {
                        role: AgentMessageRole::Assistant,
                        summary_through_ordinal: None,
                        content_json: message_json("done"),
                    }],
                    compaction_count: 0,
                    compacted_through_ordinal: None,
                },
            )
            .expect("run completes atomically");

        let appended = storage
            .append_agent_message(
                &session.id,
                AppendAgentMessage {
                    role: AgentMessageRole::User,
                    summary_through_ordinal: None,
                    content_json: message_json("after the run"),
                },
            )
            .expect("session append resumes after the run");
        assert!(appended.run_id.is_none());
        assert_eq!(appended.ordinal, 2);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn active_run_freezes_session_provider_and_datasource_targets() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let provider = storage
            .get_provider_profile(&provider_id)
            .expect("provider reads")
            .expect("provider exists");
        let second_provider = storage
            .create_provider_profile(provider_input("secondary"), None)
            .expect("second provider creates");
        let datasource = storage
            .create_datasource(
                CreateDatasource {
                    name: "Primary database".to_owned(),
                    driver_id: "driver-a".to_owned(),
                },
                None,
            )
            .expect("datasource creates");
        let second_datasource = storage
            .create_datasource(
                CreateDatasource {
                    name: "Second database".to_owned(),
                    driver_id: "driver-b".to_owned(),
                },
                None,
            )
            .expect("second datasource creates");
        let session = storage
            .create_agent_session(CreateAgentSession {
                title: "Session".to_owned(),
                provider_id: provider_id.clone(),
                datasource_id: Some(datasource.id.clone()),
                system_prompt: None,
            })
            .expect("session creates");
        let run = create_running_run(&storage, &session.id);

        let renamed = storage
            .update_agent_session(
                &session.id,
                session.revision,
                UpdateAgentSession {
                    title: "Renamed while running".to_owned(),
                    provider_id: provider_id.clone(),
                    datasource_id: Some(datasource.id.clone()),
                },
            )
            .expect("title-only update preserves the execution target");
        assert!(matches!(
            storage.update_agent_session(
                &session.id,
                renamed.revision,
                UpdateAgentSession {
                    title: "Unsafe rebind".to_owned(),
                    provider_id: second_provider.id.clone(),
                    datasource_id: Some(second_datasource.id.clone()),
                },
            ),
            Err(StorageError::AgentSessionBusy(_))
        ));
        assert!(matches!(
            storage.update_provider_profile(
                &provider.id,
                provider.revision,
                provider_input("changed while running"),
                SecretChange::Keep,
            ),
            Err(StorageError::AgentDependencyBusy {
                resource: "provider profile",
                ..
            })
        ));
        assert!(matches!(
            storage.update_datasource(
                &datasource.id,
                datasource.revision,
                UpdateDatasource {
                    name: "Changed while running".to_owned(),
                    driver_id: "driver-c".to_owned(),
                },
                SecretChange::Keep,
            ),
            Err(StorageError::AgentDependencyBusy {
                resource: "datasource",
                ..
            })
        ));
        assert!(matches!(
            storage.delete_datasource(&datasource.id, datasource.revision),
            Err(StorageError::AgentDependencyBusy {
                resource: "datasource",
                ..
            })
        ));

        storage
            .request_agent_run_cancellation(&run.id)
            .expect("cancellation requests");
        storage
            .finish_cancelled_agent_run(
                &run.id,
                AgentRunStatus::Running,
                CancelAgentRun {
                    last_sequence: 2,
                    model_rounds: 0,
                    tool_calls: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    total_tokens: 0,
                    messages: Vec::new(),
                    compaction_count: 0,
                    compacted_through_ordinal: None,
                },
            )
            .expect("cancelled run finishes");

        storage
            .update_provider_profile(
                &provider.id,
                provider.revision,
                provider_input("changed after run"),
                SecretChange::Keep,
            )
            .expect("terminal run releases provider mutation");
        storage
            .update_datasource(
                &datasource.id,
                datasource.revision,
                UpdateDatasource {
                    name: "Changed after run".to_owned(),
                    driver_id: "driver-c".to_owned(),
                },
                SecretChange::Keep,
            )
            .expect("terminal run releases datasource mutation");
        storage
            .update_agent_session(
                &session.id,
                renamed.revision,
                UpdateAgentSession {
                    title: "Rebound after run".to_owned(),
                    provider_id: second_provider.id,
                    datasource_id: Some(second_datasource.id),
                },
            )
            .expect("terminal run releases session rebind");
    }

    #[test]
    fn message_roles_reject_incompatible_content_blocks() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");

        for (role, content_json) in [
            (
                AgentMessageRole::User,
                "[{\"type\":\"tool_calls\",\"calls\":[]}]",
            ),
            (
                AgentMessageRole::Assistant,
                "[{\"type\":\"tool_result\",\"toolCallId\":\"call-1\"}]",
            ),
            (
                AgentMessageRole::Tool,
                "[{\"type\":\"text\",\"text\":\"bad\"}]",
            ),
        ] {
            let error = storage
                .append_agent_message(
                    &session.id,
                    AppendAgentMessage {
                        role,
                        summary_through_ordinal: None,
                        content_json: content_json.to_owned(),
                    },
                )
                .expect_err("role/content mismatch must fail");
            assert!(matches!(error, StorageError::InvalidAgent(_)));
        }
        assert!(
            storage
                .list_agent_messages(&session.id, 0, 10)
                .expect("messages list")
                .is_empty()
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn agent_storage_debug_redacts_prompts_sql_outputs_and_permission_summaries() {
        const SENTINEL: &str = "PRIVATE_STORAGE_AGENT_PAYLOAD_98c641";

        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let create_session = CreateAgentSession {
            title: "Session".to_owned(),
            provider_id,
            datasource_id: None,
            system_prompt: Some(SENTINEL.to_owned()),
        };
        let mut debug_values = vec![format!("{create_session:?}")];
        let session = storage
            .create_agent_session(create_session)
            .expect("session creates");

        let start = StartAgentRun {
            user_message: SENTINEL.to_owned(),
            sql_permission_mode: SqlPermissionMode::AskBeforeWrite,
        };
        debug_values.push(format!("{start:?}"));
        let started = storage
            .start_agent_run(&session.id, start)
            .expect("run starts");
        debug_values.push(format!("{started:?}"));

        let append = AppendAgentMessage {
            role: AgentMessageRole::Assistant,
            summary_through_ordinal: None,
            content_json: message_json(SENTINEL),
        };
        debug_values.push(format!("{append:?}"));

        let permission_request = RequestToolPermission {
            summary: SENTINEL.to_owned(),
            ..permission_input("call-1", [7_u8; 32], 2)
        };
        debug_values.push(format!("{permission_request:?}"));
        let permission = storage
            .create_tool_permission(&started.run.id, permission_request)
            .expect("permission creates");
        debug_values.push(format!("{permission:?}"));
        storage
            .decide_tool_permission(
                &permission.id,
                permission.revision,
                &started.run.id,
                &permission.tool_call_id,
                permission.arguments_sha256,
                3,
                ToolPermissionDecision::Deny,
            )
            .expect("permission denial resumes the run");

        let failure = FailAgentRun {
            last_sequence: 4,
            model_rounds: 1,
            tool_calls: 1,
            input_tokens: 1,
            output_tokens: 1,
            total_tokens: 2,
            error_code: "provider_error".to_owned(),
            error_message: Some(SENTINEL.to_owned()),
            messages: vec![AgentRunMessage {
                role: AgentMessageRole::Assistant,
                summary_through_ordinal: None,
                content_json: message_json(SENTINEL),
            }],
            compaction_count: 0,
            compacted_through_ordinal: None,
        };
        debug_values.push(format!("{failure:?}"));
        let failed = storage
            .fail_agent_run(&started.run.id, AgentRunStatus::Running, failure)
            .expect("run fails");
        debug_values.push(format!("{failed:?}"));

        for debug in debug_values {
            assert!(!debug.contains(SENTINEL), "sensitive Debug output: {debug}");
        }
    }

    #[test]
    fn run_start_and_completion_atomically_persist_the_transcript_and_snapshot() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        let started = storage
            .start_agent_run(
                &session.id,
                StartAgentRun {
                    user_message: "show recent orders".to_owned(),
                    sql_permission_mode: SqlPermissionMode::ReadOnly,
                },
            )
            .expect("run starts");
        assert_eq!(started.run.status, AgentRunStatus::Running);
        assert_eq!(started.run.last_sequence, 1);
        assert_eq!(started.run.started_at_ms, Some(started.run.created_at_ms));
        assert_eq!(
            started.user_message.run_id.as_deref(),
            Some(started.run.id.as_str())
        );
        assert!(matches!(
            storage.start_agent_run(
                &session.id,
                StartAgentRun {
                    user_message: "overlap".to_owned(),
                    sql_permission_mode: SqlPermissionMode::ReadOnly,
                },
            ),
            Err(StorageError::AgentSessionBusy(_))
        ));

        let completed = storage
            .complete_agent_run(
                &started.run.id,
                AgentRunStatus::Running,
                CompleteAgentRun {
                    last_sequence: 6,
                    model_rounds: 2,
                    tool_calls: 1,
                    input_tokens: 10,
                    output_tokens: 4,
                    total_tokens: 14,
                    messages: vec![
                        AgentRunMessage {
                            role: AgentMessageRole::Assistant,
                            summary_through_ordinal: None,
                            content_json: "[{\"type\":\"tool_calls\",\"calls\":[{\"id\":\"call-1\",\"name\":\"query\",\"argumentsJson\":\"{}\"}]}]".to_owned(),
                        },
                        AgentRunMessage {
                            role: AgentMessageRole::Tool,
                            summary_through_ordinal: None,
                            content_json: "[{\"type\":\"tool_result\",\"toolCallId\":\"call-1\",\"name\":\"query\",\"output\":{\"type\":\"text\",\"content\":\"ok\",\"truncated\":false}}]".to_owned(),
                        },
                        AgentRunMessage {
                            role: AgentMessageRole::Assistant,
                            summary_through_ordinal: None,
                            content_json: message_json("done"),
                        },
                    ],
                    compaction_count: 0,
                    compacted_through_ordinal: None,
                },
            )
            .expect("run completes");
        assert_eq!(completed.run.status, AgentRunStatus::Completed);
        assert_eq!(completed.run.model_rounds, 2);
        assert_eq!(completed.run.total_tokens, 14);
        assert_eq!(completed.messages.len(), 3);
        assert_eq!(
            completed.run.message_id.as_deref(),
            completed.messages.last().map(|message| message.id.as_str())
        );
        let transcript = storage
            .list_agent_messages(&session.id, 0, 10)
            .expect("transcript reads");
        assert_eq!(transcript.len(), 4);
        assert!(
            transcript
                .windows(2)
                .all(|pair| pair[1].ordinal == pair[0].ordinal + 1)
        );
        storage
            .start_agent_run(
                &session.id,
                StartAgentRun {
                    user_message: "next question".to_owned(),
                    sql_permission_mode: SqlPermissionMode::ReadOnly,
                },
            )
            .expect("terminal run releases the session");
    }

    #[test]
    fn failed_run_atomically_persists_safe_error_and_complete_messages() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        let run = create_running_run(&storage, &session.id);
        let failed = storage
            .fail_agent_run(
                &run.id,
                AgentRunStatus::Running,
                FailAgentRun {
                    last_sequence: 2,
                    model_rounds: 1,
                    tool_calls: 0,
                    input_tokens: 4,
                    output_tokens: 1,
                    total_tokens: 5,
                    error_code: "provider_unavailable".to_owned(),
                    error_message: Some("Provider unavailable".to_owned()),
                    messages: vec![AgentRunMessage {
                        role: AgentMessageRole::Assistant,
                        summary_through_ordinal: None,
                        content_json: message_json("partial response"),
                    }],
                    compaction_count: 0,
                    compacted_through_ordinal: None,
                },
            )
            .expect("run fails");
        assert_eq!(failed.run.status, AgentRunStatus::Failed);
        assert_eq!(
            failed.run.error_code.as_deref(),
            Some("provider_unavailable")
        );
        assert_eq!(failed.messages.len(), 1);
        assert_eq!(
            storage
                .list_agent_messages(&session.id, 0, 10)
                .expect("messages list")
                .len(),
            2
        );
    }

    #[test]
    fn session_message_append_cannot_bypass_compaction_transaction() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        storage
            .append_agent_message(
                &session.id,
                AppendAgentMessage {
                    role: AgentMessageRole::User,
                    summary_through_ordinal: None,
                    content_json: message_json("original"),
                },
            )
            .expect("original message appends");
        let summary = storage
            .append_agent_message(
                &session.id,
                AppendAgentMessage {
                    role: AgentMessageRole::Summary,
                    summary_through_ordinal: Some(0),
                    content_json: message_json("summary"),
                },
            )
            .expect_err("standalone summary is rejected");
        assert!(matches!(summary, StorageError::InvalidAgent(_)));
        assert!(matches!(
            storage.append_agent_message(
                &session.id,
                AppendAgentMessage {
                    role: AgentMessageRole::User,
                    summary_through_ordinal: Some(0),
                    content_json: message_json("invalid coverage"),
                },
            ),
            Err(StorageError::InvalidAgent(_))
        ));
        assert_eq!(
            storage
                .list_agent_messages(&session.id, 0, 10)
                .expect("messages list")
                .len(),
            1
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn session_compaction_coverage_uses_run_and_summary_maxima() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        assert_eq!(
            storage
                .get_agent_session_compaction_coverage(&session.id)
                .expect("empty coverage loads"),
            None
        );
        assert!(matches!(
            storage.get_agent_session_compaction_coverage("missing"),
            Err(StorageError::AgentSessionNotFound(id)) if id == "missing"
        ));

        for (role, text) in [
            (AgentMessageRole::User, "original question"),
            (AgentMessageRole::Assistant, "original answer"),
        ] {
            storage
                .append_agent_message(
                    &session.id,
                    AppendAgentMessage {
                        role,
                        summary_through_ordinal: None,
                        content_json: message_json(text),
                    },
                )
                .expect("original message appends");
        }
        let first_summary = message_json("first historical summary");
        storage
            .connection()
            .expect("connection opens")
            .execute(
                "INSERT INTO agent_messages (
                    id, session_id, ordinal, role, summary_through_ordinal,
                    content_json, content_bytes, created_at_ms
                 ) VALUES ('historical-summary-1', ?1, 2, 'summary', 1, ?2, ?3, 1)",
                rusqlite::params![
                    session.id,
                    first_summary,
                    i64::try_from(first_summary.len()).expect("summary length fits")
                ],
            )
            .expect("historical summary inserts");
        assert_eq!(
            storage
                .get_agent_session_compaction_coverage(&session.id)
                .expect("summary coverage loads"),
            Some(1)
        );

        let started = storage
            .start_agent_run(
                &session.id,
                StartAgentRun {
                    user_message: "next question".to_owned(),
                    sql_permission_mode: SqlPermissionMode::ReadOnly,
                },
            )
            .expect("run starts");
        let compacted = storage
            .compact_agent_run(
                &started.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(2), None),
            )
            .expect("deterministic trim persists");
        assert_eq!(compacted.run.compaction_count, 1);
        assert!(compacted.summary_message.is_none());
        storage
            .complete_agent_run(
                &started.run.id,
                AgentRunStatus::Running,
                CompleteAgentRun {
                    last_sequence: 3,
                    model_rounds: 1,
                    tool_calls: 0,
                    input_tokens: 1,
                    output_tokens: 1,
                    total_tokens: 2,
                    messages: vec![AgentRunMessage {
                        role: AgentMessageRole::Assistant,
                        summary_through_ordinal: None,
                        content_json: message_json("next answer"),
                    }],
                    compaction_count: 1,
                    compacted_through_ordinal: Some(2),
                },
            )
            .expect("run completes");
        assert_eq!(
            storage
                .get_agent_session_compaction_coverage(&session.id)
                .expect("run coverage loads"),
            Some(2)
        );

        let later_summary = message_json("later historical summary");
        storage
            .connection()
            .expect("connection opens")
            .execute(
                "INSERT INTO agent_messages (
                    id, session_id, ordinal, role, summary_through_ordinal,
                    content_json, content_bytes, created_at_ms
                 ) VALUES ('historical-summary-2', ?1, 5, 'summary', 4, ?2, ?3, 2)",
                rusqlite::params![
                    session.id,
                    later_summary,
                    i64::try_from(later_summary.len()).expect("summary length fits")
                ],
            )
            .expect("later historical summary inserts");
        assert_eq!(
            storage
                .get_agent_session_compaction_coverage(&session.id)
                .expect("maximum coverage loads"),
            Some(4)
        );
    }

    #[test]
    fn run_compaction_coverage_must_precede_its_initiating_user() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        for (role, text) in [
            (AgentMessageRole::User, "previous question"),
            (AgentMessageRole::Assistant, "previous answer"),
        ] {
            storage
                .append_agent_message(
                    &session.id,
                    AppendAgentMessage {
                        role,
                        summary_through_ordinal: None,
                        content_json: message_json(text),
                    },
                )
                .expect("previous message appends");
        }
        let started = storage
            .start_agent_run(
                &session.id,
                StartAgentRun {
                    user_message: "current question".to_owned(),
                    sql_permission_mode: SqlPermissionMode::ReadOnly,
                },
            )
            .expect("run starts");
        assert_eq!(started.user_message.ordinal, 2);

        let error = storage
            .compact_agent_run(
                &started.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(started.user_message.ordinal), None),
            )
            .expect_err("coverage cannot include the initiating user");
        assert!(matches!(error, StorageError::InvalidAgent(_)));

        let updated = storage
            .compact_agent_run(
                &started.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(started.user_message.ordinal - 1), None),
            )
            .expect("the same sequence remains reusable after validation failure");
        assert_eq!(updated.run.compacted_through_ordinal, Some(1));
        assert!(updated.summary_message.is_none());
    }

    #[test]
    fn summary_compaction_is_atomic_and_no_op_does_not_fabricate_coverage() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        for (role, text) in [
            (AgentMessageRole::User, "previous question"),
            (AgentMessageRole::Assistant, "previous answer"),
        ] {
            storage
                .append_agent_message(
                    &session.id,
                    AppendAgentMessage {
                        role,
                        summary_through_ordinal: None,
                        content_json: message_json(text),
                    },
                )
                .expect("historical message appends");
        }
        let started = storage
            .start_agent_run(
                &session.id,
                StartAgentRun {
                    user_message: "current question".to_owned(),
                    sql_permission_mode: SqlPermissionMode::ReadOnly,
                },
            )
            .expect("run starts");

        let compacted = storage
            .compact_agent_run(
                &started.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(1), Some("bounded summary")),
            )
            .expect("summary compaction commits");
        assert_eq!(compacted.run.last_sequence, 2);
        assert_eq!(compacted.run.compaction_count, 1);
        assert_eq!(compacted.run.compacted_through_ordinal, Some(1));
        let summary = compacted.summary_message.expect("summary is returned");
        assert_eq!(summary.run_id.as_deref(), Some(started.run.id.as_str()));
        assert_eq!(summary.role, AgentMessageRole::Summary);
        assert_eq!(summary.summary_through_ordinal, Some(1));
        assert_eq!(summary.content_json, message_json("bounded summary"));

        let no_op = storage
            .compact_agent_run(
                &started.run.id,
                AgentRunStatus::Running,
                compaction_input(3, None, None),
            )
            .expect("no-op compaction persists its event sequence");
        assert_eq!(no_op.run.last_sequence, 3);
        assert_eq!(no_op.run.compaction_count, 1);
        assert_eq!(no_op.run.compacted_through_ordinal, Some(1));
        assert!(no_op.summary_message.is_none());
        let messages = storage
            .list_agent_messages(&session.id, 0, 10)
            .expect("messages list");
        assert_eq!(messages.len(), 4);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.ordinal)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn compaction_requires_exact_sequence_and_session_wide_forward_coverage() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        let started = create_compactable_run(&storage, &session.id);

        for sequence in [1, 3] {
            assert!(matches!(
                storage.compact_agent_run(
                    &started.run.id,
                    AgentRunStatus::Running,
                    compaction_input(sequence, Some(1), None),
                ),
                Err(StorageError::InvalidAgent(_))
            ));
        }
        assert_eq!(
            storage
                .get_agent_run(&started.run.id)
                .expect("run reloads")
                .expect("run exists"),
            started.run
        );

        let compacted = storage
            .compact_agent_run(
                &started.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(1), None),
            )
            .expect("exact next sequence compacts");
        assert_eq!(compacted.run.compaction_count, 1);
        for coverage in [0, 1] {
            assert!(matches!(
                storage.compact_agent_run(
                    &started.run.id,
                    AgentRunStatus::Running,
                    compaction_input(3, Some(coverage), None),
                ),
                Err(StorageError::InvalidAgent(_))
            ));
        }
        let no_op = storage
            .compact_agent_run(
                &started.run.id,
                AgentRunStatus::Running,
                compaction_input(3, None, None),
            )
            .expect("failed coverage attempts do not consume the sequence");
        assert_eq!(no_op.run.compaction_count, 1);
        storage
            .complete_agent_run(
                &started.run.id,
                AgentRunStatus::Running,
                CompleteAgentRun {
                    last_sequence: 4,
                    model_rounds: 1,
                    tool_calls: 0,
                    input_tokens: 1,
                    output_tokens: 1,
                    total_tokens: 2,
                    messages: vec![AgentRunMessage {
                        role: AgentMessageRole::Assistant,
                        summary_through_ordinal: None,
                        content_json: message_json("first run answer"),
                    }],
                    compaction_count: 1,
                    compacted_through_ordinal: Some(1),
                },
            )
            .expect("first run completes");

        let second = storage
            .start_agent_run(
                &session.id,
                StartAgentRun {
                    user_message: "second run".to_owned(),
                    sql_permission_mode: SqlPermissionMode::ReadOnly,
                },
            )
            .expect("second run starts");
        assert!(matches!(
            storage.compact_agent_run(
                &second.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(1), None),
            ),
            Err(StorageError::InvalidAgent(_))
        ));
        let advanced = storage
            .compact_agent_run(
                &second.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(second.user_message.ordinal - 1), None),
            )
            .expect("new run advances session-wide coverage");
        assert_eq!(
            advanced.run.compacted_through_ordinal,
            Some(second.user_message.ordinal - 1)
        );
    }

    #[test]
    fn compaction_rejects_waiting_cancelled_and_write_fenced_runs() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);

        let waiting_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("waiting session creates");
        let waiting = create_running_run(&storage, &waiting_session.id);
        storage
            .create_tool_permission(&waiting.id, permission_input("waiting", [1; 32], 2))
            .expect("permission creates");
        assert!(matches!(
            storage.compact_agent_run(
                &waiting.id,
                AgentRunStatus::Running,
                compaction_input(3, None, None),
            ),
            Err(StorageError::AgentStateConflict { .. })
        ));

        let cancelled_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("cancelled session creates");
        let cancelled = create_running_run(&storage, &cancelled_session.id);
        let cancelled = storage
            .request_agent_run_cancellation(&cancelled.id)
            .expect("cancellation requests");
        assert!(matches!(
            storage.compact_agent_run(
                &cancelled.id,
                AgentRunStatus::Running,
                compaction_input(2, None, None),
            ),
            Err(StorageError::InvalidAgent(_))
        ));
        assert_eq!(
            storage
                .get_agent_run(&cancelled.id)
                .expect("cancelled run reloads")
                .expect("cancelled run exists"),
            cancelled
        );

        let fenced_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("fenced session creates");
        let fenced = create_running_run(&storage, &fenced_session.id);
        let digest = [2; 32];
        let permission = storage
            .create_tool_permission(&fenced.id, permission_input("write", digest, 2))
            .expect("permission creates");
        let permission = storage
            .decide_tool_permission(
                &permission.id,
                permission.revision,
                &fenced.id,
                "write",
                digest,
                3,
                ToolPermissionDecision::Approve,
            )
            .expect("permission approves");
        storage
            .consume_tool_permission(
                &permission.id,
                permission.revision,
                &fenced.id,
                "write",
                digest,
                4,
            )
            .expect("permission installs write fence");
        let before = storage
            .get_agent_run(&fenced.id)
            .expect("fenced run reloads")
            .expect("fenced run exists");
        assert!(matches!(
            storage.compact_agent_run(
                &fenced.id,
                AgentRunStatus::Running,
                compaction_input(5, None, None),
            ),
            Err(StorageError::InvalidAgent(_))
        ));
        assert_eq!(
            storage
                .get_agent_run(&fenced.id)
                .expect("fenced run reloads")
                .expect("fenced run exists"),
            before
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn invalid_or_quota_blocked_summary_leaves_progress_reusable_for_trim() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        let started = create_compactable_run(&storage, &session.id);
        let oversized = message_json(
            &"x".repeat(usize::try_from(MAX_AGENT_MESSAGE_BYTES).expect("limit fits usize")),
        );
        for content_json in [
            "[]".to_owned(),
            message_json(""),
            message_json("   "),
            r#"[{"type":"tool_calls","calls":[{"id":"call-1","name":"sql_read","argumentsJson":"{}"}]}]"#.to_owned(),
            r#"[{"type":"text","text":"visible","hiddenReasoning":"secret"}]"#.to_owned(),
            oversized,
        ] {
            let input = CompactAgentRun {
                last_sequence: 2,
                model_rounds: 0,
                tool_calls: 0,
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                compaction: AgentCompaction::Summary {
                    compacted_through_ordinal: 1,
                    content_json,
                },
            };
            assert!(storage
                .compact_agent_run(&started.run.id, AgentRunStatus::Running, input)
                .is_err());
            assert_eq!(
                storage
                    .get_agent_run(&started.run.id)
                    .expect("run reloads")
                    .expect("run exists"),
                started.run
            );
        }
        let trimmed = storage
            .compact_agent_run(
                &started.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(1), None),
            )
            .expect("invalid summaries leave sequence reusable for trim");
        assert_eq!(trimmed.run.compaction_count, 1);
        assert!(trimmed.summary_message.is_none());
        assert_eq!(
            storage
                .list_agent_messages(&session.id, 0, 10)
                .expect("messages list")
                .len(),
            3
        );

        let full_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("full session creates");
        let full_started = create_compactable_run(&storage, &full_session.id);
        storage
            .connection()
            .expect("connection opens")
            .execute(
                "WITH RECURSIVE sequence(value) AS (
                    SELECT 3 UNION ALL SELECT value + 1 FROM sequence WHERE value + 1 < ?2
                 )
                 INSERT INTO agent_messages (
                    id, session_id, ordinal, role, content_json, content_bytes, created_at_ms
                 ) SELECT printf('%s-fill-%d', ?1, value), ?1, value, 'assistant',
                          '[{\"type\":\"text\",\"text\":\"x\"}]',
                          length(CAST('[{\"type\":\"text\",\"text\":\"x\"}]' AS BLOB)), 0
                   FROM sequence",
                rusqlite::params![
                    full_session.id,
                    i64::try_from(MAX_AGENT_MESSAGES_PER_SESSION).expect("limit fits i64")
                ],
            )
            .expect("message-limit fixture inserts");
        let quota_error = storage
            .compact_agent_run(
                &full_started.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(1), Some("cannot append")),
            )
            .expect_err("summary respects the session message quota");
        assert!(matches!(
            quota_error,
            StorageError::AgentQuotaExceeded {
                resource: "session message count",
                ..
            }
        ));
        assert_eq!(
            storage
                .get_agent_run(&full_started.run.id)
                .expect("full run reloads")
                .expect("full run exists"),
            full_started.run
        );
        let trimmed = storage
            .compact_agent_run(
                &full_started.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(1), None),
            )
            .expect("trim succeeds without inserting into a full session");
        assert_eq!(trimmed.run.compaction_count, 1);
        assert!(trimmed.summary_message.is_none());
        assert_eq!(
            storage
                .list_agent_messages(&full_session.id, MAX_AGENT_MESSAGES_PER_SESSION - 1, 1,)
                .expect("last message remains")
                .len(),
            1
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn compaction_rolls_back_and_reconciles_or_reports_post_commit_outcomes() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);

        let create_run_with_history = |title: &str| {
            let session = storage
                .create_agent_session(CreateAgentSession {
                    title: title.to_owned(),
                    provider_id: provider_id.clone(),
                    datasource_id: None,
                    system_prompt: None,
                })
                .expect("session creates");
            for (role, text) in [
                (AgentMessageRole::User, "previous question"),
                (AgentMessageRole::Assistant, "previous answer"),
            ] {
                storage
                    .append_agent_message(
                        &session.id,
                        AppendAgentMessage {
                            role,
                            summary_through_ordinal: None,
                            content_json: message_json(text),
                        },
                    )
                    .expect("historical message appends");
            }
            let started = storage
                .start_agent_run(
                    &session.id,
                    StartAgentRun {
                        user_message: "current question".to_owned(),
                        sql_permission_mode: SqlPermissionMode::ReadOnly,
                    },
                )
                .expect("run starts");
            (session, started)
        };

        let (rollback_session, rollback_started) = create_run_with_history("rollback");
        crate::inject_faults(&[crate::FaultPoint::AgentCompactionBeforeCommit]);
        let error = storage
            .compact_agent_run(
                &rollback_started.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(1), Some("must roll back")),
            )
            .expect_err("pre-commit fault fails");
        assert!(matches!(error, StorageError::Integrity(_)));
        let rolled_back = storage
            .get_agent_run(&rollback_started.run.id)
            .expect("run reloads")
            .expect("run exists");
        assert_eq!(rolled_back.last_sequence, 1);
        assert_eq!(rolled_back.compaction_count, 0);
        assert_eq!(rolled_back.compacted_through_ordinal, None);
        assert_eq!(
            storage
                .list_agent_messages(&rollback_session.id, 0, 10)
                .expect("messages list")
                .len(),
            3
        );
        crate::inject_faults(&[crate::FaultPoint::AgentCompactionCommitFailure]);
        let error = storage
            .compact_agent_run(
                &rollback_started.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(1), Some("commit failure rolls back")),
            )
            .expect_err("commit failure reconciles as not applied");
        assert!(matches!(error, StorageError::Integrity(_)));
        assert_eq!(
            storage
                .get_agent_run(&rollback_started.run.id)
                .expect("run reloads")
                .expect("run exists")
                .last_sequence,
            1
        );
        assert_eq!(
            storage
                .list_agent_messages(&rollback_session.id, 0, 10)
                .expect("messages list")
                .len(),
            3
        );
        storage
            .compact_agent_run(
                &rollback_started.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(1), None),
            )
            .expect("the rolled-back sequence remains reusable");

        let (reconciled_session, reconciled_started) = create_run_with_history("reconciled");
        crate::inject_faults(&[crate::FaultPoint::AgentCompactionAfterCommit]);
        let reconciled = storage
            .compact_agent_run(
                &reconciled_started.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(1), Some("durable and reconciled")),
            )
            .expect("exact readback reconciles the post-commit failure");
        assert_eq!(reconciled.run.last_sequence, 2);
        assert!(reconciled.summary_message.is_some());
        assert_eq!(
            storage
                .list_agent_messages(&reconciled_session.id, 0, 10)
                .expect("messages list")
                .iter()
                .filter(|message| message.role == AgentMessageRole::Summary)
                .count(),
            1
        );

        let (unknown_session, unknown_started) = create_run_with_history("unknown");
        crate::inject_faults(&[
            crate::FaultPoint::AgentCompactionAfterCommit,
            crate::FaultPoint::AgentCompactionReadback,
        ]);
        let error = storage
            .compact_agent_run(
                &unknown_started.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(1), Some("durable but unreadable")),
            )
            .expect_err("unavailable readback leaves an unknown outcome");
        assert!(matches!(
            error,
            StorageError::OutcomeUnknown {
                operation: "compact agent run",
                ..
            }
        ));
        let durable = storage
            .get_agent_run(&unknown_started.run.id)
            .expect("run reloads")
            .expect("run exists");
        assert_eq!(durable.last_sequence, 2);
        assert_eq!(durable.compaction_count, 1);
        assert_eq!(durable.compacted_through_ordinal, Some(1));
        assert_eq!(
            storage
                .list_agent_messages(&unknown_session.id, 0, 10)
                .expect("messages list")
                .iter()
                .filter(|message| message.role == AgentMessageRole::Summary)
                .count(),
            1
        );
        assert!(matches!(
            storage.compact_agent_run(
                &unknown_started.run.id,
                AgentRunStatus::Running,
                compaction_input(2, Some(1), Some("must not duplicate")),
            ),
            Err(StorageError::InvalidAgent(_))
        ));
    }

    #[test]
    fn compaction_state_cannot_bypass_the_dedicated_transaction() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        for (role, text) in [
            (AgentMessageRole::User, "previous question"),
            (AgentMessageRole::Assistant, "previous answer"),
        ] {
            storage
                .append_agent_message(
                    &session.id,
                    AppendAgentMessage {
                        role,
                        summary_through_ordinal: None,
                        content_json: message_json(text),
                    },
                )
                .expect("historical message appends");
        }
        let started = storage
            .start_agent_run(
                &session.id,
                StartAgentRun {
                    user_message: "current question".to_owned(),
                    sql_permission_mode: SqlPermissionMode::ReadOnly,
                },
            )
            .expect("run starts");
        let update = AgentRunUpdate {
            status: AgentRunStatus::Running,
            last_sequence: 2,
            model_rounds: 0,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            compaction_count: 1,
            compacted_through_ordinal: Some(1),
        };
        assert!(matches!(
            storage.update_agent_run(&started.run.id, AgentRunStatus::Running, update),
            Err(StorageError::InvalidAgent(_))
        ));
        assert!(matches!(
            storage.complete_agent_run(
                &started.run.id,
                AgentRunStatus::Running,
                CompleteAgentRun {
                    last_sequence: 2,
                    model_rounds: 1,
                    tool_calls: 0,
                    input_tokens: 1,
                    output_tokens: 1,
                    total_tokens: 2,
                    messages: vec![AgentRunMessage {
                        role: AgentMessageRole::Assistant,
                        summary_through_ordinal: None,
                        content_json: message_json("answer"),
                    }],
                    compaction_count: 1,
                    compacted_through_ordinal: Some(1),
                },
            ),
            Err(StorageError::InvalidAgent(_))
        ));
        let current = storage
            .get_agent_run(&started.run.id)
            .expect("run reloads")
            .expect("run exists");
        assert_eq!(current.last_sequence, 1);
        assert_eq!(current.compaction_count, 0);
        assert_eq!(
            storage
                .list_agent_messages(&session.id, 0, 10)
                .expect("messages list")
                .len(),
            3
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn terminal_apis_and_message_batches_cannot_bypass_compaction_transaction() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);

        let failed_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("failed session creates");
        let failed = create_compactable_run(&storage, &failed_session.id);
        let summary_batch = FailAgentRun {
            last_sequence: 2,
            model_rounds: 0,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            error_code: "provider_failed".to_owned(),
            error_message: None,
            messages: vec![AgentRunMessage {
                role: AgentMessageRole::Summary,
                summary_through_ordinal: Some(1),
                content_json: message_json("bypass"),
            }],
            compaction_count: 0,
            compacted_through_ordinal: None,
        };
        assert!(matches!(
            storage.fail_agent_run(&failed.run.id, AgentRunStatus::Running, summary_batch,),
            Err(StorageError::InvalidAgent(_))
        ));
        let forged_failure = FailAgentRun {
            last_sequence: 2,
            model_rounds: 0,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            error_code: "provider_failed".to_owned(),
            error_message: None,
            messages: Vec::new(),
            compaction_count: 1,
            compacted_through_ordinal: Some(1),
        };
        assert!(matches!(
            storage.fail_agent_run(&failed.run.id, AgentRunStatus::Running, forged_failure,),
            Err(StorageError::InvalidAgent(_))
        ));
        assert_eq!(
            storage
                .get_agent_run(&failed.run.id)
                .expect("failed run reloads")
                .expect("failed run exists"),
            failed.run
        );

        let cancelled_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("cancelled session creates");
        let cancelled = create_compactable_run(&storage, &cancelled_session.id);
        let cancelled = storage
            .request_agent_run_cancellation(&cancelled.run.id)
            .expect("cancellation requests");
        assert!(matches!(
            storage.finish_cancelled_agent_run(
                &cancelled.id,
                AgentRunStatus::Running,
                CancelAgentRun {
                    last_sequence: 2,
                    model_rounds: 0,
                    tool_calls: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    total_tokens: 0,
                    messages: Vec::new(),
                    compaction_count: 1,
                    compacted_through_ordinal: Some(1),
                },
            ),
            Err(StorageError::InvalidAgent(_))
        ));
        assert_eq!(
            storage
                .get_agent_run(&cancelled.id)
                .expect("cancelled run reloads")
                .expect("cancelled run exists"),
            cancelled
        );

        let unknown_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("unknown session creates");
        let unknown = create_compactable_run(&storage, &unknown_session.id);
        let digest = [3; 32];
        let permission = storage
            .create_tool_permission(&unknown.run.id, permission_input("write", digest, 2))
            .expect("permission creates");
        let permission = storage
            .decide_tool_permission(
                &permission.id,
                permission.revision,
                &unknown.run.id,
                "write",
                digest,
                3,
                ToolPermissionDecision::Approve,
            )
            .expect("permission approves");
        storage
            .consume_tool_permission(
                &permission.id,
                permission.revision,
                &unknown.run.id,
                "write",
                digest,
                4,
            )
            .expect("permission installs write fence");
        let before_unknown = storage
            .get_agent_run(&unknown.run.id)
            .expect("unknown run reloads")
            .expect("unknown run exists");
        assert!(matches!(
            storage.fail_agent_write_outcome_unknown(
                &unknown.run.id,
                "write",
                digest,
                UnknownAgentWrite {
                    last_sequence: 5,
                    model_rounds: 0,
                    tool_calls: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    total_tokens: 0,
                    messages: Vec::new(),
                    compaction_count: 1,
                    compacted_through_ordinal: Some(1),
                },
            ),
            Err(StorageError::InvalidAgent(_))
        ));
        assert_eq!(
            storage
                .get_agent_run(&unknown.run.id)
                .expect("unknown run reloads")
                .expect("unknown run exists"),
            before_unknown
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn message_single_and_count_limits_are_hard_and_reserved_payloads_are_rejected() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");

        let oversized = format!(
            "[{{\"type\":\"text\",\"text\":\"{}\"}}]",
            "x".repeat(usize::try_from(MAX_AGENT_MESSAGE_BYTES).expect("limit fits usize"))
        );
        let error = storage
            .append_agent_message(
                &session.id,
                AppendAgentMessage {
                    role: AgentMessageRole::User,
                    summary_through_ordinal: None,
                    content_json: oversized,
                },
            )
            .expect_err("oversized message fails");
        assert!(matches!(error, StorageError::AgentQuotaExceeded { .. }));
        let hidden = storage
            .append_agent_message(
                &session.id,
                AppendAgentMessage {
                    role: AgentMessageRole::Assistant,
                    summary_through_ordinal: None,
                    content_json: "[{\"type\":\"text\",\"hidden_reasoning\":\"never persist\"}]"
                        .to_owned(),
                },
            )
            .expect_err("hidden reasoning fails");
        assert!(matches!(hidden, StorageError::InvalidAgent(_)));
        for content_json in [
            r#"[{"type":"text","text":"ok","hiddenReasoning":"SECRET"}]"#,
            r#"[{"type":"text","text":"ok","rawProviderDelta":"SECRET"}]"#,
            r#"[{"type":"text","text":"ok","apiKey":"SECRET"}]"#,
            r#"[{"type":"text","text":"ok","unexpected":"SECRET"}]"#,
        ] {
            let unknown = storage
                .append_agent_message(
                    &session.id,
                    AppendAgentMessage {
                        role: AgentMessageRole::Assistant,
                        summary_through_ordinal: None,
                        content_json: content_json.to_owned(),
                    },
                )
                .expect_err("unknown canonical fields fail closed");
            assert!(matches!(unknown, StorageError::InvalidAgent(_)));
        }
        assert!(
            storage
                .list_agent_messages(&session.id, 0, 10)
                .expect("messages list")
                .is_empty()
        );

        storage
            .connection()
            .expect("connection opens")
            .execute(
                "WITH RECURSIVE sequence(value) AS (
                    SELECT 0 UNION ALL SELECT value + 1 FROM sequence WHERE value + 1 < 64
                 )
                 INSERT INTO agent_messages (
                    id, session_id, ordinal, role, content_json, content_bytes, created_at_ms
                 ) SELECT printf('%s-%d', ?1, left_side.value * 64 + right_side.value),
                          ?1, left_side.value * 64 + right_side.value,
                          'user', '[{\"type\":\"text\",\"text\":\"x\"}]',
                          length(CAST('[{\"type\":\"text\",\"text\":\"x\"}]' AS BLOB)), 0
                   FROM sequence left_side CROSS JOIN sequence right_side",
                [&session.id],
            )
            .expect("limit fixture inserts");
        let count_error = storage
            .append_agent_message(
                &session.id,
                AppendAgentMessage {
                    role: AgentMessageRole::User,
                    summary_through_ordinal: None,
                    content_json: message_json("x"),
                },
            )
            .expect_err("message count limit fails");
        assert!(matches!(
            count_error,
            StorageError::AgentQuotaExceeded { .. }
        ));

        let byte_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("byte-limit session creates");
        let maximum = usize::try_from(MAX_AGENT_MESSAGE_BYTES).expect("limit fits usize");
        let empty_message_bytes = message_json("").len();
        let canonical = message_json(&"x".repeat(maximum - empty_message_bytes));
        assert_eq!(canonical.len(), maximum);
        let rows = MAX_AGENT_MESSAGE_BYTES_PER_SESSION / MAX_AGENT_MESSAGE_BYTES;
        storage
            .connection()
            .expect("connection opens")
            .execute(
                "WITH RECURSIVE sequence(value) AS (
                    SELECT 0 UNION ALL SELECT value + 1 FROM sequence WHERE value + 1 < ?4
                 )
                 INSERT INTO agent_messages (
                    id, session_id, ordinal, role, content_json, content_bytes, created_at_ms
                 ) SELECT printf('%s-%d', ?1, value), ?1, value, 'user', ?2, ?3, 0
                   FROM sequence",
                rusqlite::params![
                    byte_session.id,
                    canonical,
                    i64::try_from(MAX_AGENT_MESSAGE_BYTES).expect("limit fits i64"),
                    i64::try_from(rows).expect("row count fits i64"),
                ],
            )
            .expect("byte-limit fixture inserts");
        let bytes_error = storage
            .append_agent_message(
                &byte_session.id,
                AppendAgentMessage {
                    role: AgentMessageRole::User,
                    summary_through_ordinal: None,
                    content_json: message_json("x"),
                },
            )
            .expect_err("message byte limit fails");
        assert!(matches!(
            bytes_error,
            StorageError::AgentQuotaExceeded {
                resource: "session message bytes",
                ..
            }
        ));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn approved_permission_is_bound_and_can_be_consumed_exactly_once() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        let run = create_running_run(&storage, &session.id);
        let digest = [7_u8; 32];
        let pending = storage
            .create_tool_permission(&run.id, permission_input("tool-call-1", digest, 2))
            .expect("permission creates");
        for (run_id, tool_call_id, candidate_digest) in [
            ("wrong-run", "tool-call-1", digest),
            (run.id.as_str(), "wrong-call", digest),
            (run.id.as_str(), "tool-call-1", [8_u8; 32]),
        ] {
            let error = storage
                .decide_tool_permission(
                    &pending.id,
                    pending.revision,
                    run_id,
                    tool_call_id,
                    candidate_digest,
                    3,
                    ToolPermissionDecision::Approve,
                )
                .expect_err("misrouted decision must fail");
            assert!(matches!(
                error,
                StorageError::PermissionNotExecutable { .. }
            ));
        }
        let approved = storage
            .decide_tool_permission(
                &pending.id,
                pending.revision,
                &run.id,
                "tool-call-1",
                digest,
                3,
                ToolPermissionDecision::Approve,
            )
            .expect("permission approves");
        let mismatch = storage
            .consume_tool_permission(
                &approved.id,
                approved.revision,
                &run.id,
                "tool-call-1",
                [8_u8; 32],
                4,
            )
            .expect_err("argument mismatch cannot consume");
        assert!(matches!(
            mismatch,
            StorageError::PermissionNotExecutable { .. }
        ));

        let barrier = Arc::new(Barrier::new(3));
        let outcomes = thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..2 {
                let storage = storage.clone();
                let barrier = barrier.clone();
                let permission_id = approved.id.clone();
                let run_id = run.id.clone();
                handles.push(scope.spawn(move || {
                    barrier.wait();
                    storage.consume_tool_permission(
                        &permission_id,
                        approved.revision,
                        &run_id,
                        "tool-call-1",
                        digest,
                        4,
                    )
                }));
            }
            barrier.wait();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("consume thread joins"))
                .collect::<Vec<_>>()
        });
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        let consumed = storage
            .get_tool_permission(&approved.id)
            .expect("permission reads")
            .expect("permission exists");
        assert_eq!(consumed.status, ToolPermissionStatus::Consumed);
        let fenced = storage
            .get_agent_run(&run.id)
            .expect("run reads")
            .expect("run exists");
        assert_eq!(
            fenced.write_in_flight_tool_call_id.as_deref(),
            Some("tool-call-1")
        );
        storage
            .settle_agent_write(&run.id, "tool-call-1", digest)
            .expect("known write outcome clears fence");
        assert!(
            storage
                .get_agent_run(&run.id)
                .expect("run reads")
                .expect("run exists")
                .write_in_flight_tool_call_id
                .is_none()
        );
    }

    #[test]
    fn expired_denied_permission_cannot_resume_a_new_permission_wait() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);

        let denied_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("denied session creates");
        let denied_run = create_running_run(&storage, &denied_session.id);
        let denied_digest = [21_u8; 32];
        let denied = storage
            .create_tool_permission(
                &denied_run.id,
                permission_input("old-denied", denied_digest, 2),
            )
            .expect("old denied permission creates");
        let denied = storage
            .decide_tool_permission(
                &denied.id,
                denied.revision,
                &denied_run.id,
                "old-denied",
                denied_digest,
                3,
                ToolPermissionDecision::Deny,
            )
            .expect("old permission denies");
        let new_denied_wait = storage
            .create_tool_permission(
                &denied_run.id,
                permission_input("new-after-denied", [22_u8; 32], 4),
            )
            .expect("new pending permission creates");
        storage
            .connection()
            .expect("connection opens")
            .execute(
                "UPDATE tool_permissions SET expires_at_ms = 0 WHERE id = ?1",
                [&denied.id],
            )
            .expect("old denied permission expires");
        assert!(matches!(
            storage.decide_tool_permission(
                &denied.id,
                denied.revision,
                &denied_run.id,
                "old-denied",
                denied_digest,
                5,
                ToolPermissionDecision::Deny,
            ),
            Err(StorageError::PermissionNotExecutable { .. })
        ));
        assert_eq!(
            storage
                .get_active_tool_permission_for_run(&denied_run.id)
                .expect("active permission reads")
                .expect("new permission remains active")
                .id,
            new_denied_wait.id
        );
        assert_eq!(
            storage
                .get_agent_run(&denied_run.id)
                .expect("run reads")
                .expect("run exists")
                .status,
            AgentRunStatus::WaitingPermission
        );
    }

    #[test]
    fn expired_consumed_permission_cannot_resume_a_new_permission_wait() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let consumed_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("consumed session creates");
        let consumed_run = create_running_run(&storage, &consumed_session.id);
        let consumed_digest = [23_u8; 32];
        let consumed = storage
            .create_tool_permission(
                &consumed_run.id,
                permission_input("old-consumed", consumed_digest, 2),
            )
            .expect("old consumed permission creates");
        let consumed = storage
            .decide_tool_permission(
                &consumed.id,
                consumed.revision,
                &consumed_run.id,
                "old-consumed",
                consumed_digest,
                3,
                ToolPermissionDecision::Approve,
            )
            .expect("old consumed permission approves");
        let consumed = storage
            .consume_tool_permission(
                &consumed.id,
                consumed.revision,
                &consumed_run.id,
                "old-consumed",
                consumed_digest,
                4,
            )
            .expect("old approval consumes");
        storage
            .settle_agent_write(&consumed_run.id, "old-consumed", consumed_digest)
            .expect("old write settles");
        let new_consumed_wait = storage
            .create_tool_permission(
                &consumed_run.id,
                permission_input("new-after-consumed", [24_u8; 32], 5),
            )
            .expect("new pending permission creates");
        storage
            .connection()
            .expect("connection opens")
            .execute(
                "UPDATE tool_permissions SET expires_at_ms = 0 WHERE id = ?1",
                [&consumed.id],
            )
            .expect("old consumed permission expires");
        assert!(matches!(
            storage.consume_tool_permission_at(
                &consumed.id,
                consumed.revision,
                &consumed_run.id,
                "old-consumed",
                consumed_digest,
                6,
                1,
            ),
            Err(StorageError::PermissionNotExecutable { .. })
        ));
        assert_eq!(
            storage
                .get_active_tool_permission_for_run(&consumed_run.id)
                .expect("active permission reads")
                .expect("new permission remains active")
                .id,
            new_consumed_wait.id
        );
        assert_eq!(
            storage
                .get_agent_run(&consumed_run.id)
                .expect("run reads")
                .expect("run exists")
                .status,
            AgentRunStatus::WaitingPermission
        );
    }

    #[test]
    fn cancellation_waits_for_a_known_write_outcome_then_commits_messages_atomically() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        let run = create_running_run(&storage, &session.id);
        let digest = [9_u8; 32];
        let pending = storage
            .create_tool_permission(&run.id, permission_input("write", digest, 2))
            .expect("permission creates");
        let approved = storage
            .decide_tool_permission(
                &pending.id,
                pending.revision,
                &run.id,
                "write",
                digest,
                3,
                ToolPermissionDecision::Approve,
            )
            .expect("permission approves");
        storage
            .consume_tool_permission(&approved.id, approved.revision, &run.id, "write", digest, 4)
            .expect("write fence installs");
        let requested = storage
            .request_agent_run_cancellation(&run.id)
            .expect("cancellation requests");
        assert!(requested.cancel_requested);
        assert_eq!(requested.status, AgentRunStatus::Running);

        let cancellation = CancelAgentRun {
            last_sequence: 5,
            model_rounds: 1,
            tool_calls: 1,
            input_tokens: 1,
            output_tokens: 1,
            total_tokens: 2,
            messages: vec![AgentRunMessage {
                role: AgentMessageRole::Tool,
                summary_through_ordinal: None,
                content_json: "[{\"type\":\"tool_result\",\"toolCallId\":\"write\",\"name\":\"sql_write\",\"output\":{\"type\":\"text\",\"content\":\"unknown\",\"truncated\":false}}]".to_owned(),
            }],
            compaction_count: 0,
            compacted_through_ordinal: None,
        };
        assert!(matches!(
            storage.finish_cancelled_agent_run(
                &run.id,
                AgentRunStatus::Running,
                cancellation.clone(),
            ),
            Err(StorageError::InvalidAgent(_))
        ));
        storage
            .settle_agent_write(&run.id, "write", digest)
            .expect("known outcome clears fence");
        let cancelled = storage
            .finish_cancelled_agent_run(&run.id, AgentRunStatus::Running, cancellation)
            .expect("cancellation and transcript commit");
        assert_eq!(cancelled.run.status, AgentRunStatus::Cancelled);
        assert_eq!(cancelled.messages.len(), 1);
        assert_eq!(
            storage
                .list_agent_messages(&session.id, 0, 10)
                .expect("messages list")
                .len(),
            2
        );
    }

    #[test]
    fn unknown_write_outcome_fails_atomically_and_retains_the_exact_fence() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        let run = create_running_run(&storage, &session.id);
        let digest = [10_u8; 32];
        let pending = storage
            .create_tool_permission(&run.id, permission_input("uncertain", digest, 2))
            .expect("permission creates");
        let approved = storage
            .decide_tool_permission(
                &pending.id,
                pending.revision,
                &run.id,
                "uncertain",
                digest,
                3,
                ToolPermissionDecision::Approve,
            )
            .expect("permission approves");
        storage
            .consume_tool_permission(
                &approved.id,
                approved.revision,
                &run.id,
                "uncertain",
                digest,
                4,
            )
            .expect("write fence installs");
        let failed = storage
            .fail_agent_write_outcome_unknown(
                &run.id,
                "uncertain",
                digest,
                UnknownAgentWrite {
                    last_sequence: 5,
                    model_rounds: 1,
                    tool_calls: 1,
                    input_tokens: 1,
                    output_tokens: 0,
                    total_tokens: 1,
                    messages: Vec::new(),
                    compaction_count: 0,
                    compacted_through_ordinal: None,
                },
            )
            .expect("unknown outcome fails closed");
        assert_eq!(failed.run.status, AgentRunStatus::Failed);
        assert_eq!(
            failed.run.error_code.as_deref(),
            Some("database_outcome_unknown")
        );
        assert_eq!(
            failed.run.write_in_flight_tool_call_id.as_deref(),
            Some("uncertain")
        );
        assert!(
            storage
                .settle_agent_write(&run.id, "uncertain", digest)
                .is_err()
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn denied_expired_and_cancelled_permissions_never_execute() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");

        let denied_run = create_running_run(&storage, &session.id);
        let denied = storage
            .create_tool_permission(&denied_run.id, permission_input("denied", [1_u8; 32], 2))
            .expect("permission creates");
        let denied = storage
            .decide_tool_permission(
                &denied.id,
                denied.revision,
                &denied_run.id,
                "denied",
                [1_u8; 32],
                3,
                ToolPermissionDecision::Deny,
            )
            .expect("permission denies");
        assert!(
            storage
                .consume_tool_permission(
                    &denied.id,
                    denied.revision,
                    &denied_run.id,
                    "denied",
                    [1_u8; 32],
                    4,
                )
                .is_err()
        );

        let cancelled_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("cancelled session creates");
        let cancelled_run = create_running_run(&storage, &cancelled_session.id);
        let approved = storage
            .create_tool_permission(
                &cancelled_run.id,
                permission_input("cancelled", [2_u8; 32], 2),
            )
            .and_then(|permission| {
                storage.decide_tool_permission(
                    &permission.id,
                    permission.revision,
                    &cancelled_run.id,
                    "cancelled",
                    [2_u8; 32],
                    3,
                    ToolPermissionDecision::Approve,
                )
            })
            .expect("permission approves");
        storage
            .request_agent_run_cancellation(&cancelled_run.id)
            .expect("run cancellation requests");
        let revoked = storage
            .get_tool_permission(&approved.id)
            .expect("permission reads")
            .expect("permission exists");
        assert_eq!(revoked.status, ToolPermissionStatus::Revoked);
        assert!(
            storage
                .consume_tool_permission(
                    &revoked.id,
                    revoked.revision,
                    &cancelled_run.id,
                    "cancelled",
                    [2_u8; 32],
                    4,
                )
                .is_err()
        );

        let expiry_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("expiry session creates");
        let expiry_run = create_running_run(&storage, &expiry_session.id);
        let expired = storage
            .create_tool_permission(&expiry_run.id, permission_input("expired", [3_u8; 32], 2))
            .and_then(|permission| {
                storage.decide_tool_permission(
                    &permission.id,
                    permission.revision,
                    &expiry_run.id,
                    "expired",
                    [3_u8; 32],
                    3,
                    ToolPermissionDecision::Approve,
                )
            })
            .expect("permission approves");
        storage
            .connection()
            .expect("connection opens")
            .execute(
                "UPDATE tool_permissions SET expires_at_ms = 20 WHERE id = ?1",
                [&expired.id],
            )
            .expect("permission expires");
        let expiry_error = storage
            .consume_tool_permission_at(
                &expired.id,
                expired.revision,
                &expiry_run.id,
                "expired",
                [3_u8; 32],
                4,
                20,
            )
            .expect_err("expired permission fails");
        assert!(matches!(
            expiry_error,
            StorageError::PermissionNotExecutable { .. }
        ));
        assert_eq!(
            storage
                .get_tool_permission(&expired.id)
                .expect("permission reads")
                .expect("permission exists")
                .status,
            ToolPermissionStatus::Expired
        );
    }

    #[test]
    fn result_handles_enforce_owner_and_expiry_without_copying_results() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let first_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("first session creates");
        let second_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("second session creates");
        let first_run = create_running_run(&storage, &first_session.id);
        let second_run = create_running_run(&storage, &second_session.id);
        let result_id = completed_result(&storage);
        let timestamp = crate::now_millis().expect("clock");
        let handle = storage
            .create_agent_result_handle_at(
                &first_session.id,
                &first_run.id,
                &result_id,
                timestamp,
                timestamp + 10,
            )
            .expect("handle creates");
        assert_eq!(
            storage
                .resolve_agent_result_handle_at(
                    &handle.id,
                    &first_session.id,
                    &first_run.id,
                    timestamp,
                )
                .expect("owner resolves")
                .result_id,
            result_id
        );
        assert!(matches!(
            storage.resolve_agent_result_handle_at(
                &handle.id,
                &second_session.id,
                &second_run.id,
                timestamp,
            ),
            Err(StorageError::ResultHandleNotFound(_))
        ));
        assert!(matches!(
            storage.resolve_agent_result_handle_at(
                &handle.id,
                &first_session.id,
                &first_run.id,
                timestamp + 10,
            ),
            Err(StorageError::ResultHandleNotFound(_))
        ));
        assert!(
            storage
                .read_result_page(&result_id, PageRequest::default())
                .is_ok()
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn startup_interrupts_runs_revokes_permissions_and_removes_expired_handles() {
        let directory = TempDir::new().expect("temp dir");
        let (storage, provider_id) = setup(&directory);
        let session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("session creates");
        let running = create_running_run(&storage, &session.id);
        let waiting_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("waiting session creates");
        let waiting = create_running_run(&storage, &waiting_session.id);
        let pending = storage
            .create_tool_permission(&running.id, permission_input("pending", [4_u8; 32], 2))
            .expect("pending permission creates");
        let approved = storage
            .create_tool_permission(&waiting.id, permission_input("approved", [5_u8; 32], 2))
            .and_then(|permission| {
                storage.decide_tool_permission(
                    &permission.id,
                    permission.revision,
                    &waiting.id,
                    "approved",
                    [5_u8; 32],
                    3,
                    ToolPermissionDecision::Approve,
                )
            })
            .expect("permission approves");
        let write_session = storage
            .create_agent_session(session_input(&provider_id))
            .expect("write session creates");
        let write_run = create_running_run(&storage, &write_session.id);
        let write_digest = [6_u8; 32];
        let write_permission = storage
            .create_tool_permission(
                &write_run.id,
                permission_input("in-flight", write_digest, 2),
            )
            .and_then(|permission| {
                storage.decide_tool_permission(
                    &permission.id,
                    permission.revision,
                    &write_run.id,
                    "in-flight",
                    write_digest,
                    3,
                    ToolPermissionDecision::Approve,
                )
            })
            .and_then(|permission| {
                storage.consume_tool_permission(
                    &permission.id,
                    permission.revision,
                    &write_run.id,
                    "in-flight",
                    write_digest,
                    4,
                )
            })
            .expect("write becomes in flight");
        assert_eq!(write_permission.status, ToolPermissionStatus::Consumed);
        let result_id = completed_result(&storage);
        let handle = storage
            .create_agent_result_handle(
                &session.id,
                &running.id,
                &result_id,
                Duration::from_secs(60),
            )
            .expect("handle creates");
        storage
            .connection()
            .expect("connection opens")
            .execute(
                "UPDATE agent_result_handles SET expires_at_ms = 0 WHERE id = ?1",
                [&handle.id],
            )
            .expect("handle expires");
        drop(storage);

        let reopened =
            Storage::open(directory.path(), Arc::new(EmptyVault)).expect("storage reopens");
        let report = reopened.startup_report().agents;
        assert_eq!(report.runs_failed, 3);
        assert_eq!(report.write_outcomes_unknown, 1);
        assert_eq!(report.permissions_revoked, 2);
        assert_eq!(report.result_handles_removed, 1);
        assert_eq!(
            reopened
                .get_agent_run(&running.id)
                .expect("run reads")
                .expect("run exists")
                .status,
            AgentRunStatus::Failed
        );
        assert_eq!(
            reopened
                .get_agent_run(&running.id)
                .expect("run reads")
                .expect("run exists")
                .error_code
                .as_deref(),
            Some("runtime_restarted")
        );
        let recovered_write = reopened
            .get_agent_run(&write_run.id)
            .expect("write run reads")
            .expect("write run exists");
        assert_eq!(recovered_write.status, AgentRunStatus::Failed);
        assert_eq!(
            recovered_write.error_code.as_deref(),
            Some("database_outcome_unknown")
        );
        assert_eq!(
            recovered_write.write_in_flight_tool_call_id.as_deref(),
            Some("in-flight")
        );
        for permission_id in [&pending.id, &approved.id] {
            assert_eq!(
                reopened
                    .get_tool_permission(permission_id)
                    .expect("permission reads")
                    .expect("permission exists")
                    .status,
                ToolPermissionStatus::Revoked
            );
        }
        assert!(matches!(
            reopened.resolve_agent_result_handle(&handle.id, &session.id, &running.id),
            Err(StorageError::ResultHandleNotFound(_))
        ));
    }
}
