use std::{collections::BTreeMap, fmt};

use serde::Serialize;
use serde_json::{Value, json};

use crate::{ConfigError, ToolOutputError};

/// Maximum inline content accepted from one tool result.
pub const MAX_TOOL_OUTPUT_CONTENT_BYTES: usize = 64 * 1024;
/// Absolute maximum JSON argument bytes accepted from one provider tool call.
pub const MAX_TOOL_ARGUMENT_BYTES: usize = 64 * 1024;
/// Maximum combined key/value bytes accepted as handle metadata.
pub const MAX_TOOL_HANDLE_METADATA_BYTES: usize = 16 * 1024;
/// Maximum bytes retained for opaque provider continuation state.
pub const MAX_PROVIDER_CONTINUATION_BYTES: usize = 64 * 1024;
/// Maximum bytes accepted for one provider-native tool-call identifier.
pub const MAX_PROVIDER_TOOL_CALL_ID_BYTES: usize = 512;

/// Supported model API families.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
    Gemini,
}

/// Provider-neutral chat roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// One normalized message in the provider context.
#[derive(Clone, PartialEq)]
pub struct Message {
    role: Role,
    blocks: Vec<MessageBlock>,
}

impl Message {
    #[must_use]
    pub fn new(role: Role, blocks: Vec<MessageBlock>) -> Self {
        Self { role, blocks }
    }

    #[must_use]
    pub fn text(role: Role, content: impl Into<String>) -> Self {
        Self::new(role, vec![MessageBlock::Text(content.into())])
    }

    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self::text(Role::System, content)
    }

    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self::text(Role::User, content)
    }

    #[must_use]
    pub const fn role(&self) -> Role {
        self.role
    }

    #[must_use]
    pub fn blocks(&self) -> &[MessageBlock] {
        &self.blocks
    }
}

/// Provider-neutral message content. There is deliberately no reasoning block.
#[derive(Clone, PartialEq)]
pub enum MessageBlock {
    Text(String),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    ProviderContinuation(ProviderContinuation),
}

/// A fully assembled and parsed tool request.
#[derive(Clone, PartialEq)]
pub struct ToolCall {
    id: String,
    name: String,
    arguments: Value,
    provider_identity: Option<ProviderToolCallIdentity>,
}

impl ToolCall {
    /// Creates a complete tool call from already parsed arguments.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty identifier/name or non-object arguments.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: Value,
    ) -> Result<Self, String> {
        let id = id.into();
        let name = name.into();
        if id.is_empty() {
            return Err("tool call id must not be empty".to_owned());
        }
        if name.is_empty() {
            return Err("tool name must not be empty".to_owned());
        }
        if !arguments.is_object() {
            return Err("tool arguments must be a JSON object".to_owned());
        }
        Ok(Self {
            id,
            name,
            arguments,
            provider_identity: None,
        })
    }

    /// Attaches the original provider identity without changing the internal id.
    #[must_use]
    pub fn with_provider_identity(mut self, identity: ProviderToolCallIdentity) -> Self {
        self.provider_identity = Some(identity);
        self
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn arguments(&self) -> &Value {
        &self.arguments
    }

    #[must_use]
    pub const fn provider_identity(&self) -> Option<&ProviderToolCallIdentity> {
        self.provider_identity.as_ref()
    }
}

/// Provider-native tool-call identity retained separately from the stable internal id.
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderToolCallIdentity {
    provider: ProviderKind,
    wire_id: Option<String>,
}

impl ProviderToolCallIdentity {
    /// Records an optional provider-native id exactly as received.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty or oversized present id.
    pub fn new(provider: ProviderKind, wire_id: Option<String>) -> Result<Self, String> {
        if wire_id.as_ref().is_some_and(String::is_empty) {
            return Err("provider tool-call id must not be empty".to_owned());
        }
        if wire_id
            .as_ref()
            .is_some_and(|id| id.len() > MAX_PROVIDER_TOOL_CALL_ID_BYTES)
        {
            return Err(format!(
                "provider tool-call id exceeds {MAX_PROVIDER_TOOL_CALL_ID_BYTES} bytes"
            ));
        }
        Ok(Self { provider, wire_id })
    }

    #[must_use]
    pub const fn provider(&self) -> ProviderKind {
        self.provider
    }

    #[must_use]
    pub fn wire_id(&self) -> Option<&str> {
        self.wire_id.as_deref()
    }
}

/// Position of opaque continuation state in the originating provider part list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderContinuationPlacement {
    /// The continuation was metadata on the immediately preceding content part.
    AttachedToPreviousPart,
    /// The continuation occupied its own provider part.
    StandalonePart,
}

/// Provider-specific opaque continuation contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderContinuationKind {
    /// Gemini `Part.thoughtSignature` replay state.
    GeminiThoughtSignature,
}

impl ProviderContinuationKind {
    #[must_use]
    pub const fn provider(self) -> ProviderKind {
        match self {
            Self::GeminiThoughtSignature => ProviderKind::Gemini,
        }
    }
}

/// Bounded opaque state that a provider requires on a later request.
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderContinuation {
    provider: ProviderKind,
    kind: ProviderContinuationKind,
    value: String,
    placement: ProviderContinuationPlacement,
}

impl ProviderContinuation {
    /// Creates provider continuation state for exact replay.
    ///
    /// # Errors
    ///
    /// Returns an error for empty or oversized state.
    pub fn new(
        provider: ProviderKind,
        kind: ProviderContinuationKind,
        value: impl Into<String>,
        placement: ProviderContinuationPlacement,
    ) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty() {
            return Err("provider continuation must not be empty".to_owned());
        }
        if value.len() > MAX_PROVIDER_CONTINUATION_BYTES {
            return Err(format!(
                "provider continuation exceeds {MAX_PROVIDER_CONTINUATION_BYTES} bytes"
            ));
        }
        if kind.provider() != provider {
            return Err("provider continuation kind does not match its provider".to_owned());
        }
        Ok(Self {
            provider,
            kind,
            value,
            placement,
        })
    }

    #[must_use]
    pub const fn provider(&self) -> ProviderKind {
        self.provider
    }

    #[must_use]
    pub const fn kind(&self) -> ProviderContinuationKind {
        self.kind
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    #[must_use]
    pub const fn placement(&self) -> ProviderContinuationPlacement {
        self.placement
    }
}

/// A model-visible result paired to a tool call.
#[derive(Clone, PartialEq)]
pub struct ToolResult {
    call_id: String,
    output: ToolOutput,
}

impl ToolResult {
    #[must_use]
    pub fn new(call_id: impl Into<String>, output: ToolOutput) -> Self {
        Self {
            call_id: call_id.into(),
            output,
        }
    }

    #[must_use]
    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    #[must_use]
    pub const fn output(&self) -> &ToolOutput {
        &self.output
    }
}

/// A bounded tool result. Raw byte buffers cannot enter the agent context.
#[derive(Clone, PartialEq)]
pub struct ToolOutput {
    content: Option<String>,
    handle: Option<ToolOutputHandle>,
}

impl ToolOutput {
    /// Creates a bounded inline tool result.
    ///
    /// # Errors
    ///
    /// Returns an error when `content` exceeds the inline byte limit.
    pub fn content(content: impl Into<String>) -> Result<Self, ToolOutputError> {
        Self::new(Some(content.into()), None)
    }

    /// Creates a result containing only an external handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the result would be empty.
    pub fn handle(handle: ToolOutputHandle) -> Result<Self, ToolOutputError> {
        Self::new(None, Some(handle))
    }

    /// Creates a result with bounded preview content and an external handle.
    ///
    /// # Errors
    ///
    /// Returns an error when `content` exceeds the inline byte limit.
    pub fn content_and_handle(
        content: impl Into<String>,
        handle: ToolOutputHandle,
    ) -> Result<Self, ToolOutputError> {
        Self::new(Some(content.into()), Some(handle))
    }

    fn new(
        content: Option<String>,
        handle: Option<ToolOutputHandle>,
    ) -> Result<Self, ToolOutputError> {
        if content.is_none() && handle.is_none() {
            return Err(ToolOutputError::Empty);
        }
        if content
            .as_ref()
            .is_some_and(|value| value.len() > MAX_TOOL_OUTPUT_CONTENT_BYTES)
        {
            return Err(ToolOutputError::ContentTooLarge {
                limit: MAX_TOOL_OUTPUT_CONTENT_BYTES,
            });
        }
        Ok(Self { content, handle })
    }

    #[must_use]
    pub fn inline_content(&self) -> Option<&str> {
        self.content.as_deref()
    }

    #[must_use]
    pub const fn output_handle(&self) -> Option<&ToolOutputHandle> {
        self.handle.as_ref()
    }

    #[must_use]
    pub fn model_value(&self) -> Value {
        match (&self.content, &self.handle) {
            (Some(content), None) => Value::String(content.clone()),
            (content, handle) => json!({
                "content": content,
                "handle": handle.as_ref().map(ToolOutputHandle::model_value),
            }),
        }
    }

    #[must_use]
    pub fn inline_bytes(&self) -> usize {
        self.content.as_ref().map_or(0, String::len)
    }
}

/// Metadata pointing at a bounded result stored outside the model context.
#[derive(Clone, PartialEq, Eq)]
pub struct ToolOutputHandle {
    id: String,
    media_type: Option<String>,
    metadata: BTreeMap<String, String>,
}

impl ToolOutputHandle {
    /// Creates bounded metadata for externally stored tool output.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty id or oversized combined metadata.
    pub fn new(
        id: impl Into<String>,
        media_type: Option<String>,
        metadata: BTreeMap<String, String>,
    ) -> Result<Self, ToolOutputError> {
        let id = id.into();
        if id.is_empty() {
            return Err(ToolOutputError::EmptyHandle);
        }
        let bytes = id.len()
            + media_type.as_ref().map_or(0, String::len)
            + metadata
                .iter()
                .map(|(key, value)| key.len() + value.len())
                .sum::<usize>();
        if bytes > MAX_TOOL_HANDLE_METADATA_BYTES {
            return Err(ToolOutputError::MetadataTooLarge {
                limit: MAX_TOOL_HANDLE_METADATA_BYTES,
            });
        }
        Ok(Self {
            id,
            media_type,
            metadata,
        })
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn media_type(&self) -> Option<&str> {
        self.media_type.as_deref()
    }

    #[must_use]
    pub const fn metadata(&self) -> &BTreeMap<String, String> {
        &self.metadata
    }

    fn model_value(&self) -> Value {
        json!({
            "id": self.id,
            "media_type": self.media_type,
            "metadata": self.metadata,
        })
    }
}

/// A tool exposed to a model.
#[derive(Clone, PartialEq)]
pub struct ToolDefinition {
    name: String,
    description: String,
    input_schema: Value,
}

impl ToolDefinition {
    /// Creates a provider-neutral function definition.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty name or non-object JSON schema.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Result<Self, String> {
        let name = name.into();
        if name.is_empty() {
            return Err("tool name must not be empty".to_owned());
        }
        if !input_schema.is_object() {
            return Err("tool input schema must be a JSON object".to_owned());
        }
        Ok(Self {
            name,
            description: description.into(),
            input_schema,
        })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    #[must_use]
    pub const fn input_schema(&self) -> &Value {
        &self.input_schema
    }
}

/// One invocation passed to the host's tool executor.
#[derive(Clone, PartialEq)]
pub struct ToolInvocation {
    call_id: String,
    name: String,
    arguments: Value,
}

impl From<&ToolCall> for ToolInvocation {
    fn from(call: &ToolCall) -> Self {
        Self {
            call_id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        }
    }
}

impl ToolInvocation {
    #[must_use]
    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn arguments(&self) -> &Value {
        &self.arguments
    }
}

/// The certainty a tool has about a failed side effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOutcome {
    NotStarted,
    Failed,
    Unknown,
}

/// A provider request containing only normalized types.
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderRequest {
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
}

impl ProviderRequest {
    #[must_use]
    pub fn new(messages: Vec<Message>, tools: Vec<ToolDefinition>) -> Self {
        Self { messages, tools }
    }

    #[must_use]
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    #[must_use]
    pub fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }
}

/// Normalized model usage, independent of vendor field names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

impl Usage {
    pub(crate) fn merge(&mut self, newer: Self) {
        self.input_tokens = self.input_tokens.max(newer.input_tokens);
        self.output_tokens = self.output_tokens.max(newer.output_tokens);
        self.total_tokens = self
            .total_tokens
            .max(newer.total_tokens)
            .max(self.input_tokens.saturating_add(self.output_tokens));
    }

    pub(crate) fn accumulate(&mut self, completed_round: Self) {
        self.input_tokens = self
            .input_tokens
            .saturating_add(completed_round.input_tokens);
        self.output_tokens = self
            .output_tokens
            .saturating_add(completed_round.output_tokens);
        self.total_tokens = self
            .total_tokens
            .saturating_add(completed_round.total_tokens);
    }
}

/// Why a provider ended one model response.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    Other(String),
}

/// Streaming events emitted by every provider adapter.
#[derive(Clone, PartialEq)]
pub enum ProviderEvent {
    TextDelta(String),
    ToolCall(ToolCall),
    ProviderContinuation(ProviderContinuation),
    Usage(Usage),
    Completed(StopReason),
}

/// Limits used to decide when context compaction starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextBudget {
    max_tokens: Option<usize>,
    max_serialized_bytes: usize,
    compaction_threshold_percent: u8,
}

impl ContextBudget {
    /// Creates token and serialized-byte context limits.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::InvalidContextBudget`] for zero limits or a
    /// threshold outside `1..=100`.
    pub fn new(
        max_tokens: Option<usize>,
        max_serialized_bytes: usize,
        compaction_threshold_percent: u8,
    ) -> Result<Self, ConfigError> {
        if max_serialized_bytes == 0
            || max_tokens == Some(0)
            || !(1..=100).contains(&compaction_threshold_percent)
        {
            return Err(ConfigError::InvalidContextBudget);
        }
        Ok(Self {
            max_tokens,
            max_serialized_bytes,
            compaction_threshold_percent,
        })
    }

    #[must_use]
    pub const fn max_tokens(&self) -> Option<usize> {
        self.max_tokens
    }

    #[must_use]
    pub const fn max_serialized_bytes(&self) -> usize {
        self.max_serialized_bytes
    }

    #[must_use]
    pub const fn compaction_threshold_percent(&self) -> u8 {
        self.compaction_threshold_percent
    }

    #[must_use]
    pub fn threshold_reached(&self, usage: ContextUsage) -> bool {
        let percent = usize::from(self.compaction_threshold_percent);
        usage.serialized_bytes.saturating_mul(100)
            >= self.max_serialized_bytes.saturating_mul(percent)
            || self.max_tokens.is_some_and(|limit| {
                usage.estimated_tokens.saturating_mul(100) >= limit.saturating_mul(percent)
            })
    }

    #[must_use]
    pub fn exceeded(&self, usage: ContextUsage) -> bool {
        usage.serialized_bytes > self.max_serialized_bytes
            || self
                .max_tokens
                .is_some_and(|limit| usage.estimated_tokens > limit)
    }
}

/// Provider-aware estimate for one serialized request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub estimated_tokens: usize,
    pub serialized_bytes: usize,
}

/// A request to execute an agent run.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentInput {
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
}

impl AgentInput {
    #[must_use]
    pub fn new(messages: Vec<Message>, tools: Vec<ToolDefinition>) -> Self {
        Self { messages, tools }
    }

    #[must_use]
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    #[must_use]
    pub fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }
}

/// Why compaction was requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionCause {
    TokenThreshold,
    ByteThreshold,
    BothThresholds,
}

/// How older complete turns were compacted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionStrategy {
    Summary,
    DeterministicTrim,
}

/// Observable context-compaction metadata without provider wire details.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionEvent {
    pub cause: CompactionCause,
    pub strategy: CompactionStrategy,
    pub removed_turns: usize,
    pub summary_failed: bool,
    pub before: ContextUsage,
    pub after: ContextUsage,
    #[serde(skip)]
    pub(crate) replacement_summary: Option<String>,
}

impl CompactionEvent {
    /// Returns summary replacement text for controlled internal persistence.
    /// This value is omitted from serialization and redacted from `Debug`.
    #[must_use]
    pub fn replacement_summary(&self) -> Option<&str> {
        self.replacement_summary.as_deref()
    }
}

impl std::fmt::Debug for CompactionEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompactionEvent")
            .field("cause", &self.cause)
            .field("strategy", &self.strategy)
            .field("removed_turns", &self.removed_turns)
            .field("summary_failed", &self.summary_failed)
            .field("before", &self.before)
            .field("after", &self.after)
            .field(
                "replacement_summary",
                &self.replacement_summary.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

/// Stable trace events emitted by [`crate::AgentRunner`].
#[derive(Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    RunStarted,
    ContextCompacted {
        compaction: CompactionEvent,
    },
    ModelRoundStarted {
        round: usize,
    },
    TextDelta {
        round: usize,
        text: String,
    },
    Usage {
        round: usize,
        usage: Usage,
    },
    ModelRoundCompleted {
        round: usize,
        reason: StopReason,
    },
    ToolStarted {
        round: usize,
        call_id: String,
        name: String,
    },
    ToolCompleted {
        round: usize,
        call_id: String,
        name: String,
        inline_bytes: usize,
        handle_id: Option<String>,
    },
    TranscriptMessages {
        round: usize,
        #[serde(skip_serializing)]
        messages: Vec<Message>,
    },
    RunCompleted {
        rounds: usize,
        tool_calls: usize,
    },
    RunFailed {
        code: String,
    },
}

/// Successful final state of one run.
#[derive(Clone, PartialEq)]
pub struct RunResult {
    /// Final provider context after ephemeral compaction. Summary messages in
    /// this vector are not durable conversation messages.
    pub messages: Vec<Message>,
    /// Assistant and tool messages produced by this run in append order.
    /// Persist this vector while retaining the original input history.
    /// Replacement summaries are delivered separately through the trusted
    /// compaction event path and are never included in serialized traces.
    pub generated_messages: Vec<Message>,
    pub final_text: String,
    pub usage: Usage,
    pub model_rounds: usize,
    pub tool_calls: usize,
}

impl fmt::Debug for Message {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Message")
            .field("role", &self.role)
            .field("blocks", &self.blocks)
            .finish()
    }
}

impl fmt::Debug for MessageBlock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => formatter
                .debug_struct("Text")
                .field("bytes", &text.len())
                .finish(),
            Self::ToolCall(call) => formatter.debug_tuple("ToolCall").field(call).finish(),
            Self::ToolResult(result) => formatter.debug_tuple("ToolResult").field(result).finish(),
            Self::ProviderContinuation(continuation) => formatter
                .debug_tuple("ProviderContinuation")
                .field(continuation)
                .finish(),
        }
    }
}

impl fmt::Debug for ToolCall {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolCall")
            .field("id", &self.id)
            .field("name", &self.name)
            .field(
                "argument_fields",
                &self.arguments.as_object().map_or(0, serde_json::Map::len),
            )
            .field(
                "provider_identity",
                &self.provider_identity.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

impl fmt::Debug for ProviderToolCallIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderToolCallIdentity")
            .field("provider", &self.provider)
            .field("has_wire_id", &self.wire_id.is_some())
            .field(
                "wire_id_bytes",
                &self.wire_id.as_ref().map_or(0, String::len),
            )
            .finish()
    }
}

impl fmt::Debug for ProviderContinuation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderContinuation")
            .field("provider", &self.provider)
            .field("kind", &self.kind)
            .field("bytes", &self.value.len())
            .field("placement", &self.placement)
            .finish()
    }
}

impl fmt::Debug for ToolResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolResult")
            .field("call_id", &self.call_id)
            .field("output", &self.output)
            .finish()
    }
}

impl fmt::Debug for ToolOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolOutput")
            .field("content", &self.content.as_ref().map(String::len))
            .field("handle", &self.handle.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

impl fmt::Debug for ToolOutputHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolOutputHandle")
            .field("id_bytes", &self.id.len())
            .field("has_media_type", &self.media_type.is_some())
            .field("metadata_entries", &self.metadata.len())
            .finish()
    }
}

impl fmt::Debug for ToolDefinition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolDefinition")
            .field("name", &self.name)
            .field("description_bytes", &self.description.len())
            .field(
                "schema_fields",
                &self
                    .input_schema
                    .as_object()
                    .map_or(0, serde_json::Map::len),
            )
            .finish()
    }
}

impl fmt::Debug for ToolInvocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolInvocation")
            .field("call_id", &self.call_id)
            .field("name", &self.name)
            .field(
                "argument_fields",
                &self.arguments.as_object().map_or(0, serde_json::Map::len),
            )
            .finish()
    }
}

impl fmt::Debug for ProviderEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TextDelta(text) => formatter
                .debug_struct("TextDelta")
                .field("bytes", &text.len())
                .finish(),
            Self::ToolCall(call) => formatter.debug_tuple("ToolCall").field(call).finish(),
            Self::ProviderContinuation(continuation) => formatter
                .debug_tuple("ProviderContinuation")
                .field(continuation)
                .finish(),
            Self::Usage(usage) => formatter.debug_tuple("Usage").field(usage).finish(),
            Self::Completed(reason) => formatter.debug_tuple("Completed").field(reason).finish(),
        }
    }
}

impl fmt::Debug for StopReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stop => formatter.write_str("Stop"),
            Self::ToolCalls => formatter.write_str("ToolCalls"),
            Self::Length => formatter.write_str("Length"),
            Self::ContentFilter => formatter.write_str("ContentFilter"),
            Self::Other(_) => formatter.debug_tuple("Other").field(&"[REDACTED]").finish(),
        }
    }
}

impl fmt::Debug for RunEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RunStarted => formatter.write_str("RunStarted"),
            Self::ContextCompacted { compaction } => formatter
                .debug_struct("ContextCompacted")
                .field("compaction", compaction)
                .finish(),
            Self::ModelRoundStarted { round } => formatter
                .debug_struct("ModelRoundStarted")
                .field("round", round)
                .finish(),
            Self::TextDelta { round, text } => formatter
                .debug_struct("TextDelta")
                .field("round", round)
                .field("bytes", &text.len())
                .finish(),
            Self::Usage { round, usage } => formatter
                .debug_struct("Usage")
                .field("round", round)
                .field("usage", usage)
                .finish(),
            Self::ModelRoundCompleted { round, reason } => formatter
                .debug_struct("ModelRoundCompleted")
                .field("round", round)
                .field("reason", reason)
                .finish(),
            Self::ToolStarted {
                round,
                call_id,
                name,
            } => formatter
                .debug_struct("ToolStarted")
                .field("round", round)
                .field("call_id", call_id)
                .field("name", name)
                .finish(),
            Self::ToolCompleted {
                round,
                call_id,
                name,
                inline_bytes,
                handle_id,
            } => formatter
                .debug_struct("ToolCompleted")
                .field("round", round)
                .field("call_id", call_id)
                .field("name", name)
                .field("inline_bytes", inline_bytes)
                .field("has_handle", &handle_id.is_some())
                .finish(),
            Self::TranscriptMessages { round, messages } => formatter
                .debug_struct("TranscriptMessages")
                .field("round", round)
                .field("messages", messages)
                .finish(),
            Self::RunCompleted { rounds, tool_calls } => formatter
                .debug_struct("RunCompleted")
                .field("rounds", rounds)
                .field("tool_calls", tool_calls)
                .finish(),
            Self::RunFailed { code } => formatter
                .debug_struct("RunFailed")
                .field("code", code)
                .finish(),
        }
    }
}

impl fmt::Debug for RunResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunResult")
            .field("message_count", &self.messages.len())
            .field("generated_message_count", &self.generated_messages.len())
            .field("final_text_bytes", &self.final_text.len())
            .field("usage", &self.usage)
            .field("model_rounds", &self.model_rounds)
            .field("tool_calls", &self.tool_calls)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        AgentInput, MAX_PROVIDER_CONTINUATION_BYTES, MAX_PROVIDER_TOOL_CALL_ID_BYTES,
        MAX_TOOL_HANDLE_METADATA_BYTES, MAX_TOOL_OUTPUT_CONTENT_BYTES, Message, MessageBlock,
        ProviderContinuation, ProviderContinuationKind, ProviderContinuationPlacement,
        ProviderEvent, ProviderKind, ProviderRequest, ProviderToolCallIdentity, Role, RunEvent,
        RunResult, StopReason, ToolCall, ToolDefinition, ToolInvocation, ToolOutput,
        ToolOutputHandle, ToolResult, Usage,
    };

    #[test]
    fn tool_output_rejects_unbounded_inline_content() {
        let error = ToolOutput::content("x".repeat(MAX_TOOL_OUTPUT_CONTENT_BYTES + 1))
            .expect_err("oversized output must fail");
        assert_eq!(error.to_string(), "tool output content exceeds 65536 bytes");
    }

    #[test]
    fn tool_handle_rejects_unbounded_metadata() {
        let metadata =
            BTreeMap::from([("key".to_owned(), "x".repeat(MAX_TOOL_HANDLE_METADATA_BYTES))]);
        assert!(ToolOutputHandle::new("result", None, metadata).is_err());
    }

    #[test]
    fn provider_continuation_is_bounded_and_debug_redacted() {
        const SENTINEL: &str = "PRIVATE_PROVIDER_CONTINUATION_95cf47";

        let placement = ProviderContinuationPlacement::AttachedToPreviousPart;
        let kind = ProviderContinuationKind::GeminiThoughtSignature;
        assert!(ProviderContinuation::new(ProviderKind::Gemini, kind, "", placement).is_err());
        assert!(
            ProviderContinuation::new(
                ProviderKind::Gemini,
                kind,
                "x".repeat(MAX_PROVIDER_CONTINUATION_BYTES + 1),
                placement,
            )
            .is_err()
        );
        let continuation =
            ProviderContinuation::new(ProviderKind::Gemini, kind, SENTINEL, placement)
                .expect("bounded continuation");
        assert!(!format!("{continuation:?}").contains(SENTINEL));
        assert!(
            !format!(
                "{:?}",
                MessageBlock::ProviderContinuation(continuation.clone())
            )
            .contains(SENTINEL)
        );
        assert!(
            !format!("{:?}", ProviderEvent::ProviderContinuation(continuation)).contains(SENTINEL)
        );
    }

    #[test]
    fn provider_tool_call_identity_preserves_absence_and_is_bounded() {
        let absent = ProviderToolCallIdentity::new(ProviderKind::Gemini, None)
            .expect("an absent wire id is meaningful");
        assert_eq!(absent.wire_id(), None);
        assert!(
            ProviderToolCallIdentity::new(
                ProviderKind::Gemini,
                Some("x".repeat(MAX_PROVIDER_TOOL_CALL_ID_BYTES + 1)),
            )
            .is_err()
        );
    }

    #[test]
    fn debug_output_redacts_conversation_and_tool_payloads_transitively() {
        const SENTINEL: &str = "PRIVATE_SQL_select_secret_cell_49e68f";

        let call = ToolCall::new("call-1", "query", serde_json::json!({"sql": SENTINEL}))
            .expect("valid call");
        let invocation = ToolInvocation::from(&call);
        let handle = ToolOutputHandle::new(
            "result-1",
            Some("application/json".to_owned()),
            BTreeMap::from([("private".to_owned(), SENTINEL.to_owned())]),
        )
        .expect("valid handle");
        let output =
            ToolOutput::content_and_handle(SENTINEL, handle.clone()).expect("bounded output");
        let tool_result = ToolResult::new("call-1", output.clone());
        let user_message = Message::user(SENTINEL);
        let assistant_message =
            Message::new(Role::Assistant, vec![MessageBlock::ToolCall(call.clone())]);
        let tool_message = Message::new(
            Role::Tool,
            vec![MessageBlock::ToolResult(tool_result.clone())],
        );
        let definition = ToolDefinition::new(
            "query",
            SENTINEL,
            serde_json::json!({"type": "object", "example": SENTINEL}),
        )
        .expect("valid definition");
        let request = ProviderRequest::new(vec![user_message.clone()], vec![definition.clone()]);
        let input = AgentInput::new(vec![user_message.clone()], vec![definition.clone()]);
        let transcript = RunEvent::TranscriptMessages {
            round: 1,
            messages: vec![assistant_message.clone(), tool_message.clone()],
        };
        let text_delta = RunEvent::TextDelta {
            round: 1,
            text: SENTINEL.to_owned(),
        };
        let result = RunResult {
            messages: vec![user_message.clone(), assistant_message.clone()],
            generated_messages: vec![tool_message.clone()],
            final_text: SENTINEL.to_owned(),
            usage: Usage::default(),
            model_rounds: 1,
            tool_calls: 1,
        };

        let debug_outputs = [
            format!("{call:?}"),
            format!("{invocation:?}"),
            format!("{output:?}"),
            format!("{handle:?}"),
            format!("{tool_result:?}"),
            format!("{user_message:?}"),
            format!("{assistant_message:?}"),
            format!("{tool_message:?}"),
            format!("{:?}", MessageBlock::Text(SENTINEL.to_owned())),
            format!("{definition:?}"),
            format!("{request:?}"),
            format!("{input:?}"),
            format!("{:?}", ProviderEvent::TextDelta(SENTINEL.to_owned())),
            format!(
                "{:?}",
                ProviderEvent::Completed(StopReason::Other(SENTINEL.to_owned()))
            ),
            format!("{:?}", StopReason::Other(SENTINEL.to_owned())),
            format!("{transcript:?}"),
            format!("{text_delta:?}"),
            format!(
                "{:?}",
                RunEvent::ModelRoundCompleted {
                    round: 1,
                    reason: StopReason::Other(SENTINEL.to_owned()),
                }
            ),
            format!("{result:?}"),
        ];

        for debug in debug_outputs {
            assert!(!debug.contains(SENTINEL), "sensitive Debug output: {debug}");
        }
    }
}
