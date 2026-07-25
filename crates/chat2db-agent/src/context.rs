use std::{collections::HashSet, fmt, ops::Range, sync::Arc};

use async_trait::async_trait;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::{
    AgentError, CompactionCause, CompactionEvent, CompactionStrategy, ContextUsage, Message,
    MessageBlock, Provider, ProviderRequest, Role, ToolDefinition,
};

const MAX_SUMMARY_BYTES: usize = 64 * 1024;

/// Failure returned by an optional context summarizer.
#[derive(Clone, Error, PartialEq, Eq)]
#[error("context summary failed")]
pub struct SummaryError {
    message: String,
}

impl SummaryError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Debug for SummaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SummaryError")
            .field("message", &"[REDACTED]")
            .finish()
    }
}

/// Optional summarization boundary used only for complete historical turns.
#[async_trait]
pub trait ContextCompactor: Send + Sync {
    async fn summarize(
        &self,
        messages: &[Message],
        cancellation: CancellationToken,
    ) -> Result<String, SummaryError>;
}

/// Applies provider-aware token and serialized-byte context budgets.
#[derive(Clone, Default)]
pub struct ContextManager {
    compactor: Option<Arc<dyn ContextCompactor>>,
}

impl ContextManager {
    #[must_use]
    pub const fn new() -> Self {
        Self { compactor: None }
    }

    #[must_use]
    pub fn with_compactor(compactor: Arc<dyn ContextCompactor>) -> Self {
        Self {
            compactor: Some(compactor),
        }
    }

    /// Compacts only removable complete turns when the provider reaches 80% of
    /// either configured context dimension.
    ///
    /// # Errors
    ///
    /// Returns an error when estimation fails, cancellation wins, or mandatory
    /// context alone exceeds the provider budget.
    pub async fn prepare(
        &self,
        provider: &dyn Provider,
        messages: &mut Vec<Message>,
        tools: &[ToolDefinition],
        cancellation: CancellationToken,
    ) -> Result<Option<CompactionEvent>, AgentError> {
        let budget = provider.context_budget();
        let before = estimate(provider, messages, tools)?;
        if !budget.threshold_reached(before) {
            return Ok(None);
        }

        let cause = compaction_cause(budget, before);
        let ranges = compactable_turns(messages);
        if ranges.is_empty() {
            if budget.exceeded(before) {
                return Err(AgentError::ContextBudgetExceeded);
            }
            return Ok(Some(CompactionEvent {
                cause,
                strategy: CompactionStrategy::DeterministicTrim,
                removed_turns: 0,
                summary_failed: false,
                before,
                after: before,
                replacement_summary: None,
                compacted_message_range: None,
            }));
        }

        let original = messages.clone();
        let mut summary_failed = false;
        if let Some(compactor) = &self.compactor {
            let Some((first, remaining)) = ranges.split_first() else {
                return Ok(None);
            };
            let last = remaining.last().unwrap_or(first);
            let span = first.start..last.end;
            let summary_input = messages[span.clone()].to_vec();
            let summary = tokio::select! {
                () = cancellation.cancelled() => return Err(AgentError::Cancelled),
                result = compactor.summarize(&summary_input, cancellation.clone()) => result,
            };
            match summary {
                Ok(summary) if !summary.trim().is_empty() && summary.len() <= MAX_SUMMARY_BYTES => {
                    messages.splice(
                        span.clone(),
                        [Message::system(format!("Conversation summary:\n{summary}"))],
                    );
                    let after = estimate(provider, messages, tools)?;
                    if !budget.threshold_reached(after) {
                        return Ok(Some(CompactionEvent {
                            cause,
                            strategy: CompactionStrategy::Summary,
                            removed_turns: ranges.len(),
                            summary_failed: false,
                            before,
                            after,
                            replacement_summary: Some(summary),
                            compacted_message_range: Some(span),
                        }));
                    }
                    messages.clone_from(&original);
                    summary_failed = true;
                }
                Ok(_) | Err(_) => summary_failed = true,
            }
        }

        let mut removed_turns = 0;
        let compacted_start = ranges.first().map(|range| range.start);
        let mut removed_messages = 0_usize;
        loop {
            let usage = estimate(provider, messages, tools)?;
            if !budget.threshold_reached(usage) {
                break;
            }
            let Some(range) = compactable_turns(messages).into_iter().next() else {
                break;
            };
            removed_messages = removed_messages
                .checked_add(range.len())
                .ok_or(AgentError::ContextBudgetExceeded)?;
            messages.drain(range);
            removed_turns += 1;
        }
        let after = estimate(provider, messages, tools)?;
        if budget.exceeded(after) {
            return Err(AgentError::ContextBudgetExceeded);
        }
        Ok(Some(CompactionEvent {
            cause,
            strategy: CompactionStrategy::DeterministicTrim,
            removed_turns,
            summary_failed,
            before,
            after,
            replacement_summary: None,
            compacted_message_range: compacted_start
                .filter(|_| removed_messages > 0)
                .map(|start| start..start + removed_messages),
        }))
    }
}

impl fmt::Debug for ContextManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextManager")
            .field("compactor_configured", &self.compactor.is_some())
            .finish()
    }
}

fn estimate(
    provider: &dyn Provider,
    messages: &[Message],
    tools: &[ToolDefinition],
) -> Result<ContextUsage, AgentError> {
    provider
        .estimate_context(&ProviderRequest::new(messages.to_vec(), tools.to_vec()))
        .map_err(AgentError::Provider)
}

fn compaction_cause(budget: crate::ContextBudget, usage: ContextUsage) -> CompactionCause {
    let threshold = usize::from(budget.compaction_threshold_percent());
    let token = budget.max_tokens().is_some_and(|limit| {
        usage.estimated_tokens.saturating_mul(100) >= limit.saturating_mul(threshold)
    });
    let bytes = usage.serialized_bytes.saturating_mul(100)
        >= budget.max_serialized_bytes().saturating_mul(threshold);
    match (token, bytes) {
        (true, true) => CompactionCause::BothThresholds,
        (true, false) => CompactionCause::TokenThreshold,
        (false, true | false) => CompactionCause::ByteThreshold,
    }
}

fn compactable_turns(messages: &[Message]) -> Vec<Range<usize>> {
    let users = messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| (message.role() == Role::User).then_some(index))
        .collect::<Vec<_>>();
    let mut ranges = Vec::new();
    for pair in users.windows(2) {
        let range = pair[0]..pair[1];
        let turn = &messages[range.clone()];
        if turn.iter().any(|message| message.role() == Role::System) || !turn_is_complete(turn) {
            break;
        }
        ranges.push(range);
    }
    ranges
}

fn turn_is_complete(messages: &[Message]) -> bool {
    messages
        .last()
        .is_some_and(|message| message.role() == Role::Assistant)
        && tool_pairs_complete(messages)
}

fn tool_pairs_complete(messages: &[Message]) -> bool {
    let mut pending = HashSet::new();
    for block in messages.iter().flat_map(Message::blocks) {
        match block {
            MessageBlock::ToolCall(call) => {
                if !pending.insert(call.id()) {
                    return false;
                }
            }
            MessageBlock::ToolResult(result) => {
                if !pending.remove(result.call_id()) {
                    return false;
                }
            }
            MessageBlock::Text(_) | MessageBlock::ProviderContinuation(_) => {}
        }
    }
    pending.is_empty()
}

#[cfg(test)]
mod tests {
    use std::{pin::Pin, sync::Arc};

    use async_trait::async_trait;
    use futures_util::{Stream, stream};
    use tokio_util::sync::CancellationToken;

    use super::{ContextCompactor, ContextManager, SummaryError, compactable_turns};
    use crate::{
        ContextBudget, ContextUsage, Message, MessageBlock, Provider, ProviderError, ProviderEvent,
        ProviderEventStream, ProviderKind, ProviderRequest, Role, ToolCall, ToolOutput, ToolResult,
    };

    struct ByteProvider;

    #[async_trait]
    impl Provider for ByteProvider {
        fn kind(&self) -> ProviderKind {
            ProviderKind::OpenAi
        }

        fn model(&self) -> &'static str {
            "test"
        }

        fn context_budget(&self) -> ContextBudget {
            ContextBudget::new(None, 100, 80).expect("valid budget")
        }

        fn estimate_context(
            &self,
            request: &ProviderRequest,
        ) -> Result<ContextUsage, ProviderError> {
            let bytes = request
                .messages()
                .iter()
                .flat_map(Message::blocks)
                .map(|block| match block {
                    MessageBlock::Text(text) => text.len(),
                    MessageBlock::ToolCall(call) => call.id().len() + 20,
                    MessageBlock::ToolResult(result) => result.call_id().len() + 20,
                    MessageBlock::ProviderContinuation(continuation) => continuation.value().len(),
                })
                .sum();
            Ok(ContextUsage {
                estimated_tokens: bytes / 4,
                serialized_bytes: bytes,
            })
        }

        async fn stream(
            &self,
            _request: ProviderRequest,
            _cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            let stream: Pin<Box<dyn Stream<Item = Result<ProviderEvent, ProviderError>> + Send>> =
                Box::pin(stream::empty());
            Ok(stream)
        }
    }

    struct FailingCompactor;

    #[async_trait]
    impl ContextCompactor for FailingCompactor {
        async fn summarize(
            &self,
            _messages: &[Message],
            _cancellation: CancellationToken,
        ) -> Result<String, SummaryError> {
            Err(SummaryError::new("offline"))
        }
    }

    struct StaticCompactor;

    #[async_trait]
    impl ContextCompactor for StaticCompactor {
        async fn summarize(
            &self,
            _messages: &[Message],
            _cancellation: CancellationToken,
        ) -> Result<String, SummaryError> {
            Ok("short".to_owned())
        }
    }

    #[tokio::test]
    async fn successful_summary_preserves_system_and_latest_user() {
        let manager = ContextManager::with_compactor(Arc::new(StaticCompactor));
        let mut messages = vec![
            Message::system("system"),
            Message::user("a".repeat(50)),
            Message::text(Role::Assistant, "old answer"),
            Message::user("b".repeat(20)),
        ];
        let event = manager
            .prepare(&ByteProvider, &mut messages, &[], CancellationToken::new())
            .await
            .expect("compaction succeeds")
            .expect("event emitted");

        assert_eq!(event.strategy, crate::CompactionStrategy::Summary);
        assert_eq!(event.removed_turns, 1);
        assert_eq!(event.replacement_summary(), Some("short"));
        assert_eq!(event.compacted_message_range(), Some(1..3));
        let serialized = serde_json::to_string(&event).expect("compaction event serializes");
        assert!(!serialized.contains("short"));
        assert!(!serialized.contains("compactedMessageRange"));
        let debug = format!("{event:?}");
        assert!(!debug.contains("short"));
        assert!(!debug.contains("compacted_message_range"));
        assert!(matches!(&messages[0].blocks()[0], MessageBlock::Text(text) if text == "system"));
        assert!(
            matches!(&messages[1].blocks()[0], MessageBlock::Text(text) if text.contains("short"))
        );
        assert!(
            matches!(&messages[2].blocks()[0], MessageBlock::Text(text) if text.starts_with('b'))
        );
    }

    #[tokio::test]
    async fn summary_failure_trims_whole_oldest_turn_and_emits_event() {
        let manager = ContextManager::with_compactor(Arc::new(FailingCompactor));
        let mut messages = vec![
            Message::system("system"),
            Message::user("a".repeat(50)),
            Message::text(Role::Assistant, "old answer"),
            Message::user("b".repeat(50)),
        ];
        let event = manager
            .prepare(&ByteProvider, &mut messages, &[], CancellationToken::new())
            .await
            .expect("compaction succeeds")
            .expect("event emitted");

        assert!(event.summary_failed);
        assert_eq!(event.removed_turns, 1);
        assert_eq!(event.compacted_message_range(), Some(1..3));
        assert_eq!(messages[0].role(), Role::System);
        assert_eq!(messages[1].role(), Role::User);
        assert!(
            matches!(&messages[1].blocks()[0], MessageBlock::Text(text) if text.starts_with('b'))
        );
    }

    #[tokio::test]
    async fn deterministic_trim_reports_one_original_range_for_multiple_turns() {
        let mut messages = vec![
            Message::system("s"),
            Message::user("a".repeat(50)),
            Message::text(Role::Assistant, "a".repeat(10)),
            Message::user("b".repeat(50)),
            Message::text(Role::Assistant, "b".repeat(10)),
            Message::user("l".repeat(20)),
        ];

        let event = ContextManager::new()
            .prepare(&ByteProvider, &mut messages, &[], CancellationToken::new())
            .await
            .expect("compaction succeeds")
            .expect("event emitted");

        assert_eq!(event.removed_turns, 2);
        assert_eq!(event.compacted_message_range(), Some(1..5));
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role(), Role::System);
        assert_eq!(messages[1].role(), Role::User);
    }

    #[tokio::test]
    async fn threshold_without_a_complete_turn_has_no_compaction_range() {
        let mut messages = vec![
            Message::system("s".repeat(10)),
            Message::user("u".repeat(70)),
        ];

        let event = ContextManager::new()
            .prepare(&ByteProvider, &mut messages, &[], CancellationToken::new())
            .await
            .expect("threshold observation succeeds")
            .expect("event emitted");

        assert_eq!(event.removed_turns, 0);
        assert_eq!(event.compacted_message_range(), None);
        assert_eq!(messages.len(), 2);
    }

    #[tokio::test]
    async fn incomplete_tool_pair_is_a_non_removable_boundary() {
        let call = ToolCall::new("call-1", "query", serde_json::json!({})).expect("valid call");
        let output = ToolOutput::content("done").expect("bounded output");
        let mut messages = vec![
            Message::system("s"),
            Message::user("a".repeat(70)),
            Message::new(Role::Assistant, vec![MessageBlock::ToolCall(call)]),
            Message::user("latest"),
            Message::new(
                Role::Tool,
                vec![MessageBlock::ToolResult(ToolResult::new("call-1", output))],
            ),
        ];
        let error = ContextManager::new()
            .prepare(&ByteProvider, &mut messages, &[], CancellationToken::new())
            .await
            .expect_err("unresolved historical call must not be trimmed");
        assert!(matches!(error, crate::AgentError::ContextBudgetExceeded));
        assert_eq!(messages[1].role(), Role::User);
    }

    #[test]
    fn adjacent_users_are_not_a_complete_compaction_turn() {
        let messages = vec![Message::system("s"), Message::user("a"), Message::user("b")];
        assert!(compactable_turns(&messages).is_empty());
    }

    #[test]
    fn tool_result_requires_a_following_assistant_before_compaction() {
        let call = ToolCall::new("call-1", "query", serde_json::json!({})).expect("valid call");
        let output = ToolOutput::content("done").expect("bounded output");
        let messages = vec![
            Message::user("old"),
            Message::new(Role::Assistant, vec![MessageBlock::ToolCall(call)]),
            Message::new(
                Role::Tool,
                vec![MessageBlock::ToolResult(ToolResult::new("call-1", output))],
            ),
            Message::user("latest"),
        ];
        assert!(compactable_turns(&messages).is_empty());
    }

    #[test]
    fn complete_terminal_tool_turn_is_compactable() {
        let call = ToolCall::new("call-1", "query", serde_json::json!({})).expect("valid call");
        let output = ToolOutput::content("done").expect("bounded output");
        let messages = vec![
            Message::user("old"),
            Message::new(Role::Assistant, vec![MessageBlock::ToolCall(call)]),
            Message::new(
                Role::Tool,
                vec![MessageBlock::ToolResult(ToolResult::new("call-1", output))],
            ),
            Message::text(Role::Assistant, "terminal"),
            Message::user("latest"),
        ];
        let ranges = compactable_turns(&messages);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[0].end, 4);
    }

    #[test]
    fn terminal_marker_does_not_hide_missing_or_duplicate_tool_results() {
        let missing_call =
            ToolCall::new("missing", "query", serde_json::json!({})).expect("valid call");
        let missing = vec![
            Message::user("old"),
            Message::new(Role::Assistant, vec![MessageBlock::ToolCall(missing_call)]),
            Message::text(Role::Assistant, "terminal"),
            Message::user("latest"),
        ];
        assert!(compactable_turns(&missing).is_empty());

        let duplicate_call =
            ToolCall::new("duplicate", "query", serde_json::json!({})).expect("valid call");
        let output = ToolOutput::content("done").expect("bounded output");
        let duplicate_result =
            MessageBlock::ToolResult(ToolResult::new("duplicate", output.clone()));
        let duplicate = vec![
            Message::user("old"),
            Message::new(
                Role::Assistant,
                vec![MessageBlock::ToolCall(duplicate_call)],
            ),
            Message::new(Role::Tool, vec![duplicate_result]),
            Message::new(
                Role::Tool,
                vec![MessageBlock::ToolResult(ToolResult::new(
                    "duplicate",
                    output,
                ))],
            ),
            Message::text(Role::Assistant, "terminal"),
            Message::user("latest"),
        ];
        assert!(compactable_turns(&duplicate).is_empty());
    }

    #[test]
    fn summary_error_debug_redacts_compactor_message() {
        const SENTINEL: &str = "PRIVATE_SUMMARY_PROMPT_364f7c";
        let error = SummaryError::new(SENTINEL);
        assert!(!format!("{error:?}").contains(SENTINEL));
        assert!(!error.to_string().contains(SENTINEL));
        assert_eq!(error.message(), SENTINEL);
    }
}
