use std::{
    collections::{HashMap, HashSet},
    fmt,
    future::Future,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use tokio::{sync::mpsc, time::Instant};
use tokio_util::sync::CancellationToken;

use crate::{
    AgentError, AgentInput, ContextManager, ExecutionOutcome, Message, MessageBlock, Provider,
    ProviderEvent, ProviderRequest, Role, RunEvent, RunResult, StopReason, ToolExecutionError,
    ToolInvocation, ToolOutput, ToolResult, Usage,
};

const DEFAULT_MAX_MODEL_ROUNDS: usize = 12;
const DEFAULT_MAX_TOTAL_TOOL_CALLS: usize = 32;
const DEFAULT_MAX_TOOL_CALLS_PER_ROUND: usize = 8;
const DEFAULT_MAX_TOOL_ARGUMENT_BYTES: usize = crate::MAX_TOOL_ARGUMENT_BYTES;
const DEFAULT_MAX_DURATION: Duration = Duration::from_secs(5 * 60);
const SETTLEMENT_EVENT_GRACE: Duration = Duration::from_secs(1);
const TERMINAL_TOOL_TURN_MARKER: &str = "The tool turn ended without another model response.";

/// Hard limits for one agent run.
#[allow(clippy::struct_field_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentLimits {
    max_model_rounds: usize,
    max_total_tool_calls: usize,
    max_tool_calls_per_round: usize,
    max_tool_argument_bytes: usize,
    max_duration: Duration,
}

impl AgentLimits {
    /// Creates a fully explicit set of non-zero run limits.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::InvalidInput`] if any limit is zero.
    pub fn new(
        max_model_rounds: usize,
        max_total_tool_calls: usize,
        max_tool_calls_per_round: usize,
        max_tool_argument_bytes: usize,
        max_duration: Duration,
    ) -> Result<Self, AgentError> {
        if max_model_rounds == 0
            || max_total_tool_calls == 0
            || max_tool_calls_per_round == 0
            || max_tool_argument_bytes == 0
            || max_duration.is_zero()
        {
            return Err(AgentError::InvalidInput(
                "every agent limit must be greater than zero".to_owned(),
            ));
        }
        Ok(Self {
            max_model_rounds,
            max_total_tool_calls,
            max_tool_calls_per_round,
            max_tool_argument_bytes,
            max_duration,
        })
    }

    #[must_use]
    pub const fn max_model_rounds(&self) -> usize {
        self.max_model_rounds
    }

    #[must_use]
    pub const fn max_total_tool_calls(&self) -> usize {
        self.max_total_tool_calls
    }

    #[must_use]
    pub const fn max_tool_calls_per_round(&self) -> usize {
        self.max_tool_calls_per_round
    }

    #[must_use]
    pub const fn max_tool_argument_bytes(&self) -> usize {
        self.max_tool_argument_bytes
    }

    #[must_use]
    pub const fn max_duration(&self) -> Duration {
        self.max_duration
    }
}

impl Default for AgentLimits {
    fn default() -> Self {
        Self {
            max_model_rounds: DEFAULT_MAX_MODEL_ROUNDS,
            max_total_tool_calls: DEFAULT_MAX_TOTAL_TOOL_CALLS,
            max_tool_calls_per_round: DEFAULT_MAX_TOOL_CALLS_PER_ROUND,
            max_tool_argument_bytes: DEFAULT_MAX_TOOL_ARGUMENT_BYTES,
            max_duration: DEFAULT_MAX_DURATION,
        }
    }
}

/// Host boundary for executing one already-validated tool call.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Implementations receive cancellation and must return one bounded
    /// [`ToolOutput`]. The runtime never retries this operation, including when
    /// the failure outcome is unknown. Before dispatch, implementations must
    /// honor an already-cancelled token. After a side effect may have been
    /// dispatched, they must settle to a known or unknown terminal outcome;
    /// the runner deliberately does not drop this future on cancellation.
    async fn execute(
        &self,
        invocation: ToolInvocation,
        cancellation: CancellationToken,
    ) -> Result<ToolOutput, ToolExecutionError>;
}

/// Runs a provider and tool executor through a bounded serial loop.
pub struct AgentRunner {
    provider: Arc<dyn Provider>,
    executor: Arc<dyn ToolExecutor>,
    context: ContextManager,
    limits: AgentLimits,
}

impl AgentRunner {
    #[must_use]
    pub fn new(provider: Arc<dyn Provider>, executor: Arc<dyn ToolExecutor>) -> Self {
        Self {
            provider,
            executor,
            context: ContextManager::new(),
            limits: AgentLimits::default(),
        }
    }

    #[must_use]
    pub fn with_context_manager(mut self, context: ContextManager) -> Self {
        self.context = context;
        self
    }

    #[must_use]
    pub const fn with_limits(mut self, limits: AgentLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Runs without a live event receiver.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError`] on cancellation, deadline, invalid model output,
    /// tool failure, provider failure, or any configured resource limit.
    pub async fn run(
        &self,
        input: AgentInput,
        cancellation: CancellationToken,
    ) -> Result<RunResult, AgentError> {
        self.run_inner(input, cancellation, None).await
    }

    /// Runs while sending normalized trace events through a caller-owned,
    /// bounded channel. Backpressure counts against the total run duration;
    /// a disconnected receiver is ignored and never changes the product result.
    ///
    /// # Errors
    ///
    /// Returns the same run failures as [`Self::run`].
    pub async fn run_with_events(
        &self,
        input: AgentInput,
        cancellation: CancellationToken,
        events: mpsc::Sender<RunEvent>,
    ) -> Result<RunResult, AgentError> {
        self.run_inner(input, cancellation, Some(events)).await
    }

    async fn run_inner(
        &self,
        input: AgentInput,
        cancellation: CancellationToken,
        events: Option<mpsc::Sender<RunEvent>>,
    ) -> Result<RunResult, AgentError> {
        let run_token = cancellation.child_token();
        let deadline = Instant::now() + self.limits.max_duration;
        let result = self
            .execute_run(input, &run_token, deadline, events.as_ref())
            .await
            .map_err(|error| normalize_provider_cancellation(error, &run_token, deadline));
        if let Err(error) = &result {
            emit_best_effort(
                events.as_ref(),
                RunEvent::RunFailed {
                    code: error_code(error).to_owned(),
                },
            );
        }
        result
    }

    #[allow(clippy::too_many_lines)]
    async fn execute_run(
        &self,
        input: AgentInput,
        run_token: &CancellationToken,
        deadline: Instant,
        events: Option<&mpsc::Sender<RunEvent>>,
    ) -> Result<RunResult, AgentError> {
        validate_input(&input, self.limits)?;
        emit(events, RunEvent::RunStarted, run_token, deadline).await?;

        let mut messages = input.messages().to_vec();
        let tools = input.tools().to_vec();
        let tool_names = tools
            .iter()
            .map(|tool| tool.name().to_owned())
            .collect::<HashSet<_>>();
        let mut used_call_ids = transcript_call_ids(&messages);
        let mut total_tool_calls = 0;
        let mut total_usage = Usage::default();
        let mut generated_messages = Vec::new();
        let max_round_text_bytes = self.provider.context_budget().max_serialized_bytes();

        for round in 1..=self.limits.max_model_rounds {
            let compaction = guarded(
                self.context.prepare(
                    self.provider.as_ref(),
                    &mut messages,
                    &tools,
                    run_token.clone(),
                ),
                run_token,
                deadline,
            )
            .await??;
            if let Some(compaction) = compaction {
                emit(
                    events,
                    RunEvent::ContextCompacted { compaction },
                    run_token,
                    deadline,
                )
                .await?;
            }
            emit(
                events,
                RunEvent::ModelRoundStarted { round },
                run_token,
                deadline,
            )
            .await?;

            let request = ProviderRequest::new(messages.clone(), tools.clone());
            let mut stream = guarded(
                self.provider.stream(request, run_token.clone()),
                run_token,
                deadline,
            )
            .await??;
            let mut text = String::new();
            let mut calls = Vec::new();
            let mut assistant_blocks = Vec::new();
            let mut completion = None;
            let mut round_usage = Usage::default();

            loop {
                let item = guarded(stream.next(), run_token, deadline).await?;
                let Some(item) = item else { break };
                let event = item?;
                if completion.is_some() {
                    return Err(match event {
                        ProviderEvent::Completed(_) => AgentError::DuplicateProviderCompletion,
                        _ => AgentError::InconsistentProviderCompletion,
                    });
                }
                match event {
                    ProviderEvent::TextDelta(delta) => {
                        let next_text_bytes = text.len().checked_add(delta.len());
                        if !matches!(next_text_bytes, Some(bytes) if bytes <= max_round_text_bytes)
                        {
                            return Err(AgentError::ModelTextTooLarge {
                                round,
                                limit: max_round_text_bytes,
                            });
                        }
                        text.push_str(&delta);
                        match assistant_blocks.last_mut() {
                            Some(MessageBlock::Text(previous)) => previous.push_str(&delta),
                            Some(
                                MessageBlock::ToolCall(_)
                                | MessageBlock::ToolResult(_)
                                | MessageBlock::ProviderContinuation(_),
                            )
                            | None => assistant_blocks.push(MessageBlock::Text(delta.clone())),
                        }
                        emit(
                            events,
                            RunEvent::TextDelta { round, text: delta },
                            run_token,
                            deadline,
                        )
                        .await?;
                    }
                    ProviderEvent::ToolCall(call) => {
                        validate_model_call(&call, &tool_names, &mut used_call_ids, self.limits)?;
                        if calls.len() == self.limits.max_tool_calls_per_round {
                            return Err(AgentError::RoundToolLimit {
                                round,
                                limit: self.limits.max_tool_calls_per_round,
                            });
                        }
                        if total_tool_calls + calls.len() == self.limits.max_total_tool_calls {
                            return Err(AgentError::TotalToolLimit(
                                self.limits.max_total_tool_calls,
                            ));
                        }
                        assistant_blocks.push(MessageBlock::ToolCall(call.clone()));
                        calls.push(call);
                    }
                    ProviderEvent::ProviderContinuation(continuation) => {
                        assistant_blocks.push(MessageBlock::ProviderContinuation(continuation));
                    }
                    ProviderEvent::Usage(usage) => {
                        round_usage.merge(usage);
                        emit(
                            events,
                            RunEvent::Usage { round, usage },
                            run_token,
                            deadline,
                        )
                        .await?;
                    }
                    ProviderEvent::Completed(reason) => completion = Some(reason),
                }
            }

            let reason = completion.ok_or(AgentError::IncompleteProviderStream)?;
            total_usage.accumulate(round_usage);
            if (reason == StopReason::ToolCalls) == calls.is_empty() {
                return Err(AgentError::InconsistentProviderCompletion);
            }
            emit(
                events,
                RunEvent::ModelRoundCompleted {
                    round,
                    reason: reason.clone(),
                },
                run_token,
                deadline,
            )
            .await?;

            let assistant_message = Message::new(Role::Assistant, assistant_blocks);

            if calls.is_empty() {
                let turn_messages = vec![assistant_message];
                emit(
                    events,
                    RunEvent::TranscriptMessages {
                        round,
                        messages: turn_messages.clone(),
                    },
                    run_token,
                    deadline,
                )
                .await?;
                messages.extend(turn_messages.iter().cloned());
                generated_messages.extend(turn_messages);
                emit(
                    events,
                    RunEvent::RunCompleted {
                        rounds: round,
                        tool_calls: total_tool_calls,
                    },
                    run_token,
                    deadline,
                )
                .await?;
                return Ok(RunResult {
                    messages,
                    generated_messages,
                    final_text: text,
                    usage: total_usage,
                    model_rounds: round,
                    tool_calls: total_tool_calls,
                });
            }
            if round == self.limits.max_model_rounds {
                return Err(AgentError::ModelRoundLimit(self.limits.max_model_rounds));
            }

            let mut turn_messages = vec![assistant_message];
            let mut terminal_error = None;
            let mut calls = calls.into_iter();
            while let Some(call) = calls.next() {
                if let Err(error) = emit(
                    events,
                    RunEvent::ToolStarted {
                        round,
                        call_id: call.id().to_owned(),
                        name: call.name().to_owned(),
                        arguments_sha256: ToolInvocation::from(&call).arguments_sha256(),
                    },
                    run_token,
                    deadline,
                )
                .await
                {
                    turn_messages.push(terminal_tool_message(&call, ExecutionOutcome::NotStarted));
                    terminal_error = Some(error);
                }
                if terminal_error.is_none()
                    && let Err(error) = ensure_active(run_token, deadline)
                {
                    turn_messages.push(terminal_tool_message(&call, ExecutionOutcome::NotStarted));
                    terminal_error = Some(error);
                }
                if terminal_error.is_some() {
                    turn_messages.extend(calls.map(|remaining| {
                        terminal_tool_message(&remaining, ExecutionOutcome::NotStarted)
                    }));
                    break;
                }

                let invocation = ToolInvocation::from(&call);
                total_tool_calls += 1;
                let settlement = self
                    .execute_tool_to_settlement(invocation, run_token, deadline)
                    .await;
                match settlement {
                    Ok(output) => {
                        let completed = RunEvent::ToolCompleted {
                            round,
                            call_id: call.id().to_owned(),
                            name: call.name().to_owned(),
                            output: output.clone(),
                        };
                        turn_messages.push(Message::new(
                            Role::Tool,
                            vec![MessageBlock::ToolResult(ToolResult::named(
                                call.id(),
                                call.name(),
                                output,
                            ))],
                        ));
                        if let Err(error) =
                            emit_after_settlement(events, completed, run_token, deadline).await
                        {
                            terminal_error = Some(error);
                        }
                    }
                    Err(source) if source.outcome() == ExecutionOutcome::Unknown => {
                        emit_with_settlement_grace(
                            events,
                            RunEvent::ToolFailed {
                                round,
                                call_id: call.id().to_owned(),
                                name: call.name().to_owned(),
                                code: source.code().to_owned(),
                                message: source.message().to_owned(),
                                outcome: source.outcome(),
                            },
                        )
                        .await;
                        turn_messages.push(terminal_tool_message(&call, ExecutionOutcome::Unknown));
                        terminal_error = Some(AgentError::Tool {
                            tool: call.name().to_owned(),
                            source,
                        });
                    }
                    Err(source) => {
                        let outcome = source.outcome();
                        emit_with_settlement_grace(
                            events,
                            RunEvent::ToolFailed {
                                round,
                                call_id: call.id().to_owned(),
                                name: call.name().to_owned(),
                                code: source.code().to_owned(),
                                message: source.message().to_owned(),
                                outcome,
                            },
                        )
                        .await;
                        turn_messages.push(terminal_tool_message(&call, outcome));
                        terminal_error = Some(match ensure_active(run_token, deadline) {
                            Err(interrupted) => interrupted,
                            Ok(()) => AgentError::Tool {
                                tool: call.name().to_owned(),
                                source,
                            },
                        });
                    }
                }
                if terminal_error.is_some() {
                    turn_messages.extend(calls.map(|remaining| {
                        terminal_tool_message(&remaining, ExecutionOutcome::NotStarted)
                    }));
                    break;
                }
            }
            if terminal_error.is_some() {
                turn_messages.push(terminal_tool_turn_marker());
            }
            let transcript = RunEvent::TranscriptMessages {
                round,
                messages: turn_messages.clone(),
            };
            if let Some(error) = terminal_error {
                emit_with_settlement_grace(events, transcript).await;
                return Err(error);
            }
            if let Err(error) = emit(events, transcript.clone(), run_token, deadline).await {
                turn_messages.push(terminal_tool_turn_marker());
                emit_with_settlement_grace(
                    events,
                    RunEvent::TranscriptMessages {
                        round,
                        messages: turn_messages,
                    },
                )
                .await;
                return Err(error);
            }
            messages.extend(turn_messages.iter().cloned());
            generated_messages.extend(turn_messages);
        }

        Err(AgentError::ModelRoundLimit(self.limits.max_model_rounds))
    }

    async fn execute_tool_to_settlement(
        &self,
        invocation: ToolInvocation,
        cancellation: &CancellationToken,
        deadline: Instant,
    ) -> Result<ToolOutput, ToolExecutionError> {
        let tool_cancellation = cancellation.child_token();
        let execution = self.executor.execute(invocation, tool_cancellation.clone());
        tokio::pin!(execution);

        tokio::select! {
            result = &mut execution => result,
            () = cancellation.cancelled() => {
                tool_cancellation.cancel();
                execution.await
            }
            () = tokio::time::sleep_until(deadline) => {
                tool_cancellation.cancel();
                execution.await
            }
        }
    }
}

impl fmt::Debug for AgentRunner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRunner")
            .field("provider", &self.provider.kind())
            .field("model", &self.provider.model())
            .field("context", &self.context)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

async fn guarded<F, T>(
    future: F,
    cancellation: &CancellationToken,
    deadline: Instant,
) -> Result<T, AgentError>
where
    F: Future<Output = T>,
{
    tokio::select! {
        () = cancellation.cancelled() => Err(AgentError::Cancelled),
        () = tokio::time::sleep_until(deadline) => {
            cancellation.cancel();
            Err(AgentError::DeadlineExceeded)
        }
        output = future => Ok(output),
    }
}

fn ensure_active(cancellation: &CancellationToken, deadline: Instant) -> Result<(), AgentError> {
    if Instant::now() >= deadline {
        cancellation.cancel();
        return Err(AgentError::DeadlineExceeded);
    }
    if cancellation.is_cancelled() {
        return Err(AgentError::Cancelled);
    }
    Ok(())
}

async fn emit(
    sender: Option<&mpsc::Sender<RunEvent>>,
    event: RunEvent,
    cancellation: &CancellationToken,
    deadline: Instant,
) -> Result<(), AgentError> {
    let Some(sender) = sender else {
        return Ok(());
    };
    let _ = guarded(sender.send(event), cancellation, deadline).await?;
    Ok(())
}

async fn emit_after_settlement(
    sender: Option<&mpsc::Sender<RunEvent>>,
    event: RunEvent,
    cancellation: &CancellationToken,
    deadline: Instant,
) -> Result<(), AgentError> {
    if let Err(error) = ensure_active(cancellation, deadline) {
        emit_with_settlement_grace(sender, event).await;
        return Err(error);
    }
    if let Err(error) = emit(sender, event.clone(), cancellation, deadline).await {
        emit_with_settlement_grace(sender, event).await;
        return Err(error);
    }
    Ok(())
}

async fn emit_with_settlement_grace(sender: Option<&mpsc::Sender<RunEvent>>, event: RunEvent) {
    let Some(sender) = sender else {
        return;
    };
    let _ = tokio::time::timeout(SETTLEMENT_EVENT_GRACE, sender.send(event)).await;
}

fn emit_best_effort(sender: Option<&mpsc::Sender<RunEvent>>, event: RunEvent) {
    let Some(sender) = sender else {
        return;
    };
    let _ = sender.try_send(event);
}

fn normalize_provider_cancellation(
    error: AgentError,
    cancellation: &CancellationToken,
    deadline: Instant,
) -> AgentError {
    if !matches!(error, AgentError::Provider(crate::ProviderError::Cancelled)) {
        return error;
    }
    if Instant::now() >= deadline {
        cancellation.cancel();
        AgentError::DeadlineExceeded
    } else if cancellation.is_cancelled() {
        AgentError::Cancelled
    } else {
        error
    }
}

fn terminal_tool_message(call: &crate::ToolCall, outcome: ExecutionOutcome) -> Message {
    let content = match outcome {
        ExecutionOutcome::NotStarted => "Tool execution did not start because the run stopped.",
        ExecutionOutcome::Failed => "Tool execution failed with a known outcome.",
        ExecutionOutcome::Unknown => {
            "Tool execution outcome is unknown and the operation was not retried."
        }
    };
    let output = ToolOutput::content(content).expect("static terminal tool output is bounded");
    Message::new(
        Role::Tool,
        vec![MessageBlock::ToolResult(ToolResult::named(
            call.id(),
            call.name(),
            output,
        ))],
    )
}

fn terminal_tool_turn_marker() -> Message {
    Message::text(Role::Assistant, TERMINAL_TOOL_TURN_MARKER)
}

fn validate_input(input: &AgentInput, limits: AgentLimits) -> Result<(), AgentError> {
    if input.messages().is_empty() {
        return Err(AgentError::InvalidInput(
            "at least one message is required".to_owned(),
        ));
    }
    if !input
        .messages()
        .iter()
        .any(|message| message.role() == Role::User)
    {
        return Err(AgentError::InvalidInput(
            "at least one user message is required".to_owned(),
        ));
    }

    let mut tool_names = HashSet::new();
    for tool in input.tools() {
        if !tool_names.insert(tool.name()) {
            return Err(AgentError::InvalidInput(format!(
                "duplicate tool definition {}",
                tool.name()
            )));
        }
    }

    let mut calls = HashMap::new();
    let mut results = HashSet::new();
    for message in input.messages() {
        match message.role() {
            Role::System | Role::User => {
                if message
                    .blocks()
                    .iter()
                    .any(|block| !matches!(block, MessageBlock::Text(_)))
                {
                    return Err(AgentError::InvalidInput(
                        "system/user messages may contain only text".to_owned(),
                    ));
                }
            }
            Role::Assistant => {
                for (index, block) in message.blocks().iter().enumerate() {
                    match block {
                        MessageBlock::Text(_) => {}
                        MessageBlock::ToolCall(call) => {
                            validate_argument_size(call, limits)?;
                            if calls.insert(call.id(), call.name()).is_some() {
                                return Err(AgentError::DuplicateToolCall(call.id().to_owned()));
                            }
                        }
                        MessageBlock::ToolResult(_) => {
                            return Err(AgentError::InvalidInput(
                                "assistant message contains a tool result".to_owned(),
                            ));
                        }
                        MessageBlock::ProviderContinuation(continuation) => {
                            if continuation.placement()
                                == crate::ProviderContinuationPlacement::AttachedToPreviousPart
                                && !index.checked_sub(1).is_some_and(|previous| {
                                    matches!(
                                        message.blocks()[previous],
                                        MessageBlock::Text(_) | MessageBlock::ToolCall(_)
                                    )
                                })
                            {
                                return Err(AgentError::InvalidInput(
                                    "attached provider continuation has no preceding content part"
                                        .to_owned(),
                                ));
                            }
                        }
                    }
                }
            }
            Role::Tool => {
                let [MessageBlock::ToolResult(result)] = message.blocks() else {
                    return Err(AgentError::InvalidInput(
                        "tool message must contain exactly one tool result".to_owned(),
                    ));
                };
                if !calls.contains_key(result.call_id()) || !results.insert(result.call_id()) {
                    return Err(AgentError::InvalidInput(format!(
                        "tool result {} is missing, duplicated, or out of order",
                        result.call_id()
                    )));
                }
            }
        }
    }
    if let Some(unresolved) = calls.keys().find(|id| !results.contains(**id)) {
        return Err(AgentError::InvalidInput(format!(
            "tool call {unresolved} has no result"
        )));
    }
    Ok(())
}

fn validate_model_call(
    call: &crate::ToolCall,
    tool_names: &HashSet<String>,
    used_call_ids: &mut HashSet<String>,
    limits: AgentLimits,
) -> Result<(), AgentError> {
    if !tool_names.contains(call.name()) {
        return Err(AgentError::UnknownTool(call.name().to_owned()));
    }
    validate_argument_size(call, limits)?;
    if !used_call_ids.insert(call.id().to_owned()) {
        return Err(AgentError::DuplicateToolCall(call.id().to_owned()));
    }
    Ok(())
}

fn validate_argument_size(call: &crate::ToolCall, limits: AgentLimits) -> Result<(), AgentError> {
    let bytes =
        serde_json::to_vec(call.arguments()).map_err(|error| AgentError::InvalidToolArguments {
            call_id: call.id().to_owned(),
            message: error.to_string(),
        })?;
    if bytes.len() > limits.max_tool_argument_bytes {
        return Err(AgentError::ToolArgumentsTooLarge {
            call_id: call.id().to_owned(),
            limit: limits.max_tool_argument_bytes,
        });
    }
    Ok(())
}

fn transcript_call_ids(messages: &[Message]) -> HashSet<String> {
    messages
        .iter()
        .flat_map(Message::blocks)
        .filter_map(|block| match block {
            MessageBlock::ToolCall(call) => Some(call.id().to_owned()),
            MessageBlock::Text(_)
            | MessageBlock::ToolResult(_)
            | MessageBlock::ProviderContinuation(_) => None,
        })
        .collect()
}

const fn error_code(error: &AgentError) -> &'static str {
    match error {
        AgentError::Cancelled => "cancelled",
        AgentError::DeadlineExceeded => "deadline_exceeded",
        AgentError::Provider(_) => "provider_error",
        AgentError::UnknownTool(_) => "unknown_tool",
        AgentError::DuplicateToolCall(_) => "duplicate_tool_call",
        AgentError::InvalidToolArguments { .. } => "invalid_tool_arguments",
        AgentError::ToolArgumentsTooLarge { .. } => "tool_arguments_too_large",
        AgentError::ModelTextTooLarge { .. } => "model_text_too_large",
        AgentError::RoundToolLimit { .. } => "round_tool_limit",
        AgentError::TotalToolLimit(_) => "total_tool_limit",
        AgentError::ModelRoundLimit(_) => "model_round_limit",
        AgentError::IncompleteProviderStream => "incomplete_provider_stream",
        AgentError::DuplicateProviderCompletion => "duplicate_provider_completion",
        AgentError::InconsistentProviderCompletion => "inconsistent_provider_completion",
        AgentError::Tool { .. } => "tool_error",
        AgentError::ContextBudgetExceeded => "context_budget_exceeded",
        AgentError::InvalidInput(_) => "invalid_input",
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;
    use futures_util::{Stream, stream};
    use tokio::sync::{Mutex, Notify, mpsc, oneshot};
    use tokio_util::sync::CancellationToken;

    use super::{AgentLimits, AgentRunner, TERMINAL_TOOL_TURN_MARKER, ToolExecutor};
    use crate::{
        AgentError, AgentInput, ContextBudget, ContextManager, ContextUsage, ExecutionOutcome,
        Message, MessageBlock, Provider, ProviderError, ProviderEvent, ProviderEventStream,
        ProviderKind, ProviderRequest, Role, StopReason, ToolCall, ToolDefinition,
        ToolExecutionError, ToolInvocation, ToolOutput, Usage,
    };

    struct ScriptedProvider {
        rounds: Mutex<VecDeque<Vec<Result<ProviderEvent, ProviderError>>>>,
        budget: ContextBudget,
    }

    impl ScriptedProvider {
        fn new(rounds: Vec<Vec<Result<ProviderEvent, ProviderError>>>) -> Self {
            Self::with_budget(
                rounds,
                ContextBudget::new(Some(1_000_000), 1_000_000, 80).expect("valid budget"),
            )
        }

        fn with_budget(
            rounds: Vec<Vec<Result<ProviderEvent, ProviderError>>>,
            budget: ContextBudget,
        ) -> Self {
            Self {
                rounds: Mutex::new(rounds.into()),
                budget,
            }
        }
    }

    #[async_trait]
    impl Provider for ScriptedProvider {
        fn kind(&self) -> ProviderKind {
            ProviderKind::OpenAi
        }

        fn model(&self) -> &'static str {
            "test"
        }

        fn context_budget(&self) -> ContextBudget {
            self.budget
        }

        fn estimate_context(
            &self,
            request: &ProviderRequest,
        ) -> Result<ContextUsage, ProviderError> {
            Ok(ContextUsage {
                estimated_tokens: request.messages().len() * 10,
                serialized_bytes: request.messages().len() * 40,
            })
        }

        async fn stream(
            &self,
            _request: ProviderRequest,
            _cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            let items = self
                .rounds
                .lock()
                .await
                .pop_front()
                .expect("scripted round exists");
            Ok(Box::pin(stream::iter(items)))
        }
    }

    struct RecordingExecutor {
        calls: Mutex<Vec<String>>,
        active: AtomicUsize,
        maximum_active: AtomicUsize,
        fail: bool,
    }

    impl RecordingExecutor {
        fn new(fail: bool) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                active: AtomicUsize::new(0),
                maximum_active: AtomicUsize::new(0),
                fail,
            }
        }
    }

    #[async_trait]
    impl ToolExecutor for RecordingExecutor {
        async fn execute(
            &self,
            invocation: ToolInvocation,
            _cancellation: CancellationToken,
        ) -> Result<ToolOutput, ToolExecutionError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum_active.fetch_max(active, Ordering::SeqCst);
            self.calls
                .lock()
                .await
                .push(invocation.call_id().to_owned());
            self.active.fetch_sub(1, Ordering::SeqCst);
            if self.fail {
                return Err(ToolExecutionError::new(
                    "write_unknown",
                    "delivery outcome is unknown",
                    ExecutionOutcome::Unknown,
                ));
            }
            ToolOutput::content(format!("result for {}", invocation.call_id())).map_err(|error| {
                ToolExecutionError::new("output", error.to_string(), ExecutionOutcome::Failed)
            })
        }
    }

    fn tool() -> ToolDefinition {
        ToolDefinition::new(
            "query",
            "Run a query",
            serde_json::json!({"type": "object"}),
        )
        .expect("valid tool")
    }

    fn call(id: &str) -> ToolCall {
        ToolCall::new(id, "query", serde_json::json!({"sql": "select 1"})).expect("valid call")
    }

    #[tokio::test]
    async fn executes_tools_serially_then_returns_final_model_text() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            vec![
                Ok(ProviderEvent::ToolCall(call("a"))),
                Ok(ProviderEvent::ToolCall(call("b"))),
                Ok(ProviderEvent::Completed(StopReason::ToolCalls)),
            ],
            vec![
                Ok(ProviderEvent::TextDelta("done".to_owned())),
                Ok(ProviderEvent::Usage(Usage {
                    input_tokens: 10,
                    output_tokens: 2,
                    total_tokens: 12,
                })),
                Ok(ProviderEvent::Completed(StopReason::Stop)),
            ],
        ]));
        let executor = Arc::new(RecordingExecutor::new(false));
        let runner = AgentRunner::new(provider, executor.clone());
        let result = runner
            .run(
                AgentInput::new(vec![Message::user("go")], vec![tool()]),
                CancellationToken::new(),
            )
            .await
            .expect("run succeeds");

        assert_eq!(result.final_text, "done");
        assert_eq!(result.tool_calls, 2);
        assert_eq!(*executor.calls.lock().await, ["a", "b"]);
        assert_eq!(executor.maximum_active.load(Ordering::SeqCst), 1);
        assert_eq!(result.generated_messages.len(), 4);
        assert_eq!(
            result
                .generated_messages
                .iter()
                .map(Message::role)
                .collect::<Vec<_>>(),
            [Role::Assistant, Role::Tool, Role::Tool, Role::Assistant]
        );
        assert!(matches!(
            result.generated_messages[0].blocks(),
            [MessageBlock::ToolCall(first), MessageBlock::ToolCall(second)]
                if first.id() == "a" && second.id() == "b"
        ));
        assert!(matches!(
            result.generated_messages[1].blocks(),
            [MessageBlock::ToolResult(tool_result)] if tool_result.call_id() == "a"
        ));
        assert!(matches!(
            result.generated_messages[2].blocks(),
            [MessageBlock::ToolResult(tool_result)] if tool_result.call_id() == "b"
        ));
        assert!(matches!(
            result.generated_messages[3].blocks(),
            [MessageBlock::Text(text)] if text == "done"
        ));
    }

    #[tokio::test]
    async fn preserves_interleaved_provider_content_order() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            vec![
                Ok(ProviderEvent::TextDelta("before".to_owned())),
                Ok(ProviderEvent::ToolCall(call("a"))),
                Ok(ProviderEvent::TextDelta("after".to_owned())),
                Ok(ProviderEvent::Completed(StopReason::ToolCalls)),
            ],
            vec![
                Ok(ProviderEvent::TextDelta("done".to_owned())),
                Ok(ProviderEvent::Completed(StopReason::Stop)),
            ],
        ]));
        let result = AgentRunner::new(provider, Arc::new(RecordingExecutor::new(false)))
            .run(
                AgentInput::new(vec![Message::user("go")], vec![tool()]),
                CancellationToken::new(),
            )
            .await
            .expect("run completes");

        assert!(matches!(
            result.generated_messages[0].blocks(),
            [MessageBlock::Text(before), MessageBlock::ToolCall(call), MessageBlock::Text(after)]
                if before == "before" && call.id() == "a" && after == "after"
        ));
    }

    #[tokio::test]
    async fn unknown_tool_fails_before_execution() {
        let unknown = ToolCall::new("a", "missing", serde_json::json!({})).expect("valid call");
        let provider = Arc::new(ScriptedProvider::new(vec![vec![
            Ok(ProviderEvent::ToolCall(unknown)),
            Ok(ProviderEvent::Completed(StopReason::ToolCalls)),
        ]]));
        let executor = Arc::new(RecordingExecutor::new(false));
        let error = AgentRunner::new(provider, executor.clone())
            .run(
                AgentInput::new(vec![Message::user("go")], vec![tool()]),
                CancellationToken::new(),
            )
            .await
            .expect_err("unknown tool must fail");

        assert!(matches!(error, AgentError::UnknownTool(_)));
        assert!(executor.calls.lock().await.is_empty());
    }

    #[tokio::test]
    async fn completed_historical_tool_call_does_not_require_current_registration() {
        let historical_call = ToolCall::new(
            "old-call",
            "write_query",
            serde_json::json!({"sql": "update old_table set value = 1"}),
        )
        .expect("valid historical call");
        let historical_result = ToolOutput::content("updated").expect("bounded historical result");
        let messages = vec![
            Message::user("change it"),
            Message::new(
                Role::Assistant,
                vec![MessageBlock::ToolCall(historical_call)],
            ),
            Message::new(
                Role::Tool,
                vec![MessageBlock::ToolResult(crate::ToolResult::new(
                    "old-call",
                    historical_result,
                ))],
            ),
            Message::text(Role::Assistant, "done"),
            Message::user("continue in read-only mode"),
        ];
        let provider = Arc::new(ScriptedProvider::new(vec![vec![
            Ok(ProviderEvent::TextDelta("continued".to_owned())),
            Ok(ProviderEvent::Completed(StopReason::Stop)),
        ]]));
        let executor = Arc::new(RecordingExecutor::new(false));

        let result = AgentRunner::new(provider, executor.clone())
            .run(
                AgentInput::new(messages, Vec::new()),
                CancellationToken::new(),
            )
            .await
            .expect("historical tools are independent of the current registry");

        assert_eq!(result.final_text, "continued");
        assert!(executor.calls.lock().await.is_empty());
    }

    #[tokio::test]
    async fn unknown_tool_outcome_is_not_retried() {
        let provider = Arc::new(ScriptedProvider::new(vec![vec![
            Ok(ProviderEvent::ToolCall(call("a"))),
            Ok(ProviderEvent::Completed(StopReason::ToolCalls)),
        ]]));
        let executor = Arc::new(RecordingExecutor::new(true));
        let error = AgentRunner::new(provider, executor.clone())
            .run(
                AgentInput::new(vec![Message::user("go")], vec![tool()]),
                CancellationToken::new(),
            )
            .await
            .expect_err("tool failure must stop the run");

        assert!(
            matches!(error, AgentError::Tool { source, .. } if source.outcome() == ExecutionOutcome::Unknown)
        );
        assert_eq!(executor.calls.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn failed_tool_settlement_emits_bounded_failure_details() {
        let provider = Arc::new(ScriptedProvider::new(vec![vec![
            Ok(ProviderEvent::ToolCall(call("a"))),
            Ok(ProviderEvent::Completed(StopReason::ToolCalls)),
        ]]));
        let (sender, mut receiver) = mpsc::channel(32);
        let error = AgentRunner::new(provider, Arc::new(RecordingExecutor::new(true)))
            .run_with_events(
                AgentInput::new(vec![Message::user("go")], vec![tool()]),
                CancellationToken::new(),
                sender,
            )
            .await
            .expect_err("unknown settlement stops the run");
        assert!(matches!(error, AgentError::Tool { .. }));

        let events = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
        assert!(events.iter().any(|event| matches!(
            event,
            crate::RunEvent::ToolStarted {
                call_id,
                arguments_sha256,
                ..
            } if call_id == "a" && *arguments_sha256 == ToolInvocation::from(&call("a")).arguments_sha256()
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            crate::RunEvent::ToolFailed {
                call_id,
                code,
                message,
                outcome: ExecutionOutcome::Unknown,
                ..
            } if call_id == "a"
                && code == "write_unknown"
                && message == "delivery outcome is unknown"
        )));
    }

    #[tokio::test]
    async fn failed_multi_tool_round_persists_a_complete_terminal_transcript() {
        struct FailSecondExecutor {
            calls: AtomicUsize,
        }

        #[async_trait]
        impl ToolExecutor for FailSecondExecutor {
            async fn execute(
                &self,
                _invocation: ToolInvocation,
                _cancellation: CancellationToken,
            ) -> Result<ToolOutput, ToolExecutionError> {
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    return ToolOutput::content("first result").map_err(|error| {
                        ToolExecutionError::new(
                            "output",
                            error.to_string(),
                            ExecutionOutcome::Failed,
                        )
                    });
                }
                Err(ToolExecutionError::new(
                    "database_outcome_unknown",
                    "unknown",
                    ExecutionOutcome::Unknown,
                ))
            }
        }

        let provider = Arc::new(ScriptedProvider::with_budget(
            vec![vec![
                Ok(ProviderEvent::ToolCall(call("a"))),
                Ok(ProviderEvent::ToolCall(call("b"))),
                Ok(ProviderEvent::ToolCall(call("c"))),
                Ok(ProviderEvent::Completed(StopReason::ToolCalls)),
            ]],
            ContextBudget::new(None, 200, 80).expect("valid budget"),
        ));
        let (sender, mut receiver) = mpsc::channel(32);
        let error = AgentRunner::new(
            provider.clone(),
            Arc::new(FailSecondExecutor {
                calls: AtomicUsize::new(0),
            }),
        )
        .run_with_events(
            AgentInput::new(vec![Message::user("go")], vec![tool()]),
            CancellationToken::new(),
            sender,
        )
        .await
        .expect_err("unknown second result stops the run");
        assert!(matches!(
            error,
            AgentError::Tool { source, .. } if source.outcome() == ExecutionOutcome::Unknown
        ));
        let transcript =
            std::iter::from_fn(|| receiver.try_recv().ok()).find_map(|event| match event {
                crate::RunEvent::TranscriptMessages { messages, .. } => Some(messages),
                _ => None,
            });
        let messages = transcript.expect("a complete terminal tool turn is emitted");
        assert_eq!(messages.len(), 5);
        assert!(matches!(
            messages[0].blocks(),
            [MessageBlock::ToolCall(first), MessageBlock::ToolCall(second), MessageBlock::ToolCall(third)]
                if first.id() == "a" && second.id() == "b" && third.id() == "c"
        ));
        assert!(
            messages[1..4]
                .iter()
                .all(|message| matches!(message.blocks(), [MessageBlock::ToolResult(_)]))
        );
        assert!(matches!(
            messages[4].blocks(),
            [MessageBlock::Text(text)] if text == TERMINAL_TOOL_TURN_MARKER
        ));

        let mut history = vec![Message::system("system"), Message::user("old")];
        history.extend(messages);
        history.push(Message::user("latest"));
        let compaction = ContextManager::new()
            .prepare(
                provider.as_ref(),
                &mut history,
                &[],
                CancellationToken::new(),
            )
            .await
            .expect("terminal tool turn can be compacted")
            .expect("tight context emits compaction metadata");
        assert_eq!(compaction.removed_turns, 1);
        assert_eq!(
            history.iter().map(Message::role).collect::<Vec<_>>(),
            [Role::System, Role::User]
        );
    }

    #[tokio::test]
    async fn cancellation_waits_for_dispatched_tool_to_settle_without_retrying() {
        struct SettlingExecutor {
            attempts: AtomicUsize,
            dispatched: Mutex<Option<oneshot::Sender<()>>>,
            cancellation_observed: Mutex<Option<oneshot::Sender<()>>>,
            release: Notify,
        }

        #[async_trait]
        impl ToolExecutor for SettlingExecutor {
            async fn execute(
                &self,
                _invocation: ToolInvocation,
                cancellation: CancellationToken,
            ) -> Result<ToolOutput, ToolExecutionError> {
                self.attempts.fetch_add(1, Ordering::SeqCst);
                if let Some(dispatched) = self.dispatched.lock().await.take() {
                    let _ = dispatched.send(());
                }
                cancellation.cancelled().await;
                if let Some(observed) = self.cancellation_observed.lock().await.take() {
                    let _ = observed.send(());
                }
                self.release.notified().await;
                Err(ToolExecutionError::new(
                    "write_unknown",
                    "delivery outcome is unknown",
                    ExecutionOutcome::Unknown,
                ))
            }
        }

        let provider = Arc::new(ScriptedProvider::new(vec![vec![
            Ok(ProviderEvent::ToolCall(call("a"))),
            Ok(ProviderEvent::Completed(StopReason::ToolCalls)),
        ]]));
        let (dispatched_sender, dispatched_receiver) = oneshot::channel();
        let (observed_sender, observed_receiver) = oneshot::channel();
        let executor = Arc::new(SettlingExecutor {
            attempts: AtomicUsize::new(0),
            dispatched: Mutex::new(Some(dispatched_sender)),
            cancellation_observed: Mutex::new(Some(observed_sender)),
            release: Notify::new(),
        });
        let runner = AgentRunner::new(provider, executor.clone());
        let cancellation = CancellationToken::new();
        let run_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            runner
                .run(
                    AgentInput::new(vec![Message::user("go")], vec![tool()]),
                    run_cancellation,
                )
                .await
        });

        tokio::time::timeout(std::time::Duration::from_secs(1), dispatched_receiver)
            .await
            .expect("tool dispatch is observed")
            .expect("dispatch signal remains open");
        cancellation.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(1), observed_receiver)
            .await
            .expect("executor observes cancellation")
            .expect("cancellation signal remains open");
        assert!(!task.is_finished(), "runner must wait for tool settlement");

        executor.release.notify_one();
        let error = task
            .await
            .expect("task joins")
            .expect_err("unknown tool outcome stops the run");
        assert!(
            matches!(error, AgentError::Tool { source, .. } if source.outcome() == ExecutionOutcome::Unknown)
        );
        assert_eq!(executor.attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancellation_wins_after_a_dispatched_tool_reports_known_failure() {
        struct KnownFailureExecutor {
            dispatched: Mutex<Option<oneshot::Sender<()>>>,
        }

        #[async_trait]
        impl ToolExecutor for KnownFailureExecutor {
            async fn execute(
                &self,
                _invocation: ToolInvocation,
                cancellation: CancellationToken,
            ) -> Result<ToolOutput, ToolExecutionError> {
                if let Some(dispatched) = self.dispatched.lock().await.take() {
                    let _ = dispatched.send(());
                }
                cancellation.cancelled().await;
                Err(ToolExecutionError::new(
                    "cancelled_before_effect",
                    "known failure",
                    ExecutionOutcome::Failed,
                ))
            }
        }

        let provider = Arc::new(ScriptedProvider::new(vec![vec![
            Ok(ProviderEvent::ToolCall(call("a"))),
            Ok(ProviderEvent::Completed(StopReason::ToolCalls)),
        ]]));
        let (dispatched_sender, dispatched_receiver) = oneshot::channel();
        let runner = AgentRunner::new(
            provider,
            Arc::new(KnownFailureExecutor {
                dispatched: Mutex::new(Some(dispatched_sender)),
            }),
        );
        let cancellation = CancellationToken::new();
        let run_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            runner
                .run(
                    AgentInput::new(vec![Message::user("go")], vec![tool()]),
                    run_cancellation,
                )
                .await
        });
        dispatched_receiver
            .await
            .expect("tool dispatch signal remains open");
        cancellation.cancel();
        let error = task
            .await
            .expect("task joins")
            .expect_err("run is cancelled after known settlement");
        assert!(matches!(error, AgentError::Cancelled));
    }

    #[tokio::test]
    async fn successful_settlement_after_cancellation_remains_observable() {
        struct SuccessfulSettlementExecutor {
            dispatched: Mutex<Option<oneshot::Sender<()>>>,
        }

        #[async_trait]
        impl ToolExecutor for SuccessfulSettlementExecutor {
            async fn execute(
                &self,
                _invocation: ToolInvocation,
                cancellation: CancellationToken,
            ) -> Result<ToolOutput, ToolExecutionError> {
                if let Some(dispatched) = self.dispatched.lock().await.take() {
                    let _ = dispatched.send(());
                }
                cancellation.cancelled().await;
                ToolOutput::content("write committed").map_err(|error| {
                    ToolExecutionError::new("output", error.to_string(), ExecutionOutcome::Failed)
                })
            }
        }

        let provider = Arc::new(ScriptedProvider::new(vec![vec![
            Ok(ProviderEvent::ToolCall(call("a"))),
            Ok(ProviderEvent::Completed(StopReason::ToolCalls)),
        ]]));
        let (dispatched_sender, dispatched_receiver) = oneshot::channel();
        let runner = AgentRunner::new(
            provider,
            Arc::new(SuccessfulSettlementExecutor {
                dispatched: Mutex::new(Some(dispatched_sender)),
            }),
        );
        let cancellation = CancellationToken::new();
        let run_cancellation = cancellation.clone();
        let (sender, mut receiver) = mpsc::channel(32);
        let task = tokio::spawn(async move {
            runner
                .run_with_events(
                    AgentInput::new(vec![Message::user("go")], vec![tool()]),
                    run_cancellation,
                    sender,
                )
                .await
        });
        dispatched_receiver
            .await
            .expect("tool dispatch signal remains open");
        cancellation.cancel();
        let error = task
            .await
            .expect("task joins")
            .expect_err("the run remains cancelled");
        assert!(matches!(error, AgentError::Cancelled));

        let events = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
        assert!(events.iter().any(|event| matches!(
            event,
            crate::RunEvent::ToolCompleted { call_id, .. } if call_id == "a"
        )));
        let messages = events
            .iter()
            .find_map(|event| match event {
                crate::RunEvent::TranscriptMessages { messages, .. } => Some(messages),
                _ => None,
            })
            .expect("successful settlement is persisted as a complete turn");
        assert!(matches!(
            messages[1].blocks(),
            [MessageBlock::ToolResult(result)]
                if result.output().inline_content() == Some("write committed")
        ));
        assert!(matches!(
            messages[2].blocks(),
            [MessageBlock::Text(text)] if text == TERMINAL_TOOL_TURN_MARKER
        ));
    }

    #[tokio::test]
    async fn tool_child_token_closes_the_pre_poll_dispatch_race() {
        struct DispatchGateExecutor {
            dispatches: AtomicUsize,
        }

        #[async_trait]
        impl ToolExecutor for DispatchGateExecutor {
            async fn execute(
                &self,
                _invocation: ToolInvocation,
                cancellation: CancellationToken,
            ) -> Result<ToolOutput, ToolExecutionError> {
                if cancellation.is_cancelled() {
                    return Err(ToolExecutionError::new(
                        "cancelled_before_dispatch",
                        "not started",
                        ExecutionOutcome::NotStarted,
                    ));
                }
                self.dispatches.fetch_add(1, Ordering::SeqCst);
                ToolOutput::content("dispatched").map_err(|error| {
                    ToolExecutionError::new("output", error.to_string(), ExecutionOutcome::Failed)
                })
            }
        }

        let executor = Arc::new(DispatchGateExecutor {
            dispatches: AtomicUsize::new(0),
        });
        let runner = AgentRunner::new(Arc::new(ScriptedProvider::new(vec![])), executor.clone());
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let result = runner
            .execute_tool_to_settlement(
                ToolInvocation::from(&call("a")),
                &cancellation,
                tokio::time::Instant::now() + std::time::Duration::from_secs(1),
            )
            .await;

        assert!(matches!(
            result,
            Err(error) if error.outcome() == ExecutionOutcome::NotStarted
        ));
        assert_eq!(executor.dispatches.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cancellation_interrupts_a_pending_provider_stream() {
        struct PendingProvider;

        #[async_trait]
        impl Provider for PendingProvider {
            fn kind(&self) -> ProviderKind {
                ProviderKind::OpenAi
            }
            fn model(&self) -> &'static str {
                "pending"
            }
            fn context_budget(&self) -> ContextBudget {
                ContextBudget::new(None, 10_000, 80).expect("valid budget")
            }
            fn estimate_context(
                &self,
                _request: &ProviderRequest,
            ) -> Result<ContextUsage, ProviderError> {
                Ok(ContextUsage::default())
            }
            async fn stream(
                &self,
                _request: ProviderRequest,
                _cancellation: CancellationToken,
            ) -> Result<ProviderEventStream, ProviderError> {
                let pending: Pin<
                    Box<dyn Stream<Item = Result<ProviderEvent, ProviderError>> + Send>,
                > = Box::pin(stream::pending());
                Ok(pending)
            }
        }

        let token = CancellationToken::new();
        let cancelled = token.clone();
        let runner = AgentRunner::new(
            Arc::new(PendingProvider),
            Arc::new(RecordingExecutor::new(false)),
        );
        let task = tokio::spawn(async move {
            runner
                .run(
                    AgentInput::new(vec![Message::user("go")], Vec::new()),
                    cancelled,
                )
                .await
        });
        tokio::task::yield_now().await;
        token.cancel();
        let error = task
            .await
            .expect("task joins")
            .expect_err("run is cancelled");
        assert!(matches!(error, AgentError::Cancelled));
    }

    #[tokio::test]
    async fn provider_cancelled_race_normalizes_to_run_cancellation() {
        struct CancellingProvider;

        #[async_trait]
        impl Provider for CancellingProvider {
            fn kind(&self) -> ProviderKind {
                ProviderKind::OpenAi
            }
            fn model(&self) -> &'static str {
                "cancel-race"
            }
            fn context_budget(&self) -> ContextBudget {
                ContextBudget::new(None, 10_000, 80).expect("valid budget")
            }
            fn estimate_context(
                &self,
                _request: &ProviderRequest,
            ) -> Result<ContextUsage, ProviderError> {
                Ok(ContextUsage::default())
            }
            async fn stream(
                &self,
                _request: ProviderRequest,
                cancellation: CancellationToken,
            ) -> Result<ProviderEventStream, ProviderError> {
                cancellation.cancelled().await;
                Err(ProviderError::Cancelled)
            }
        }

        for _ in 0..16 {
            let token = CancellationToken::new();
            let run_token = token.clone();
            let runner = AgentRunner::new(
                Arc::new(CancellingProvider),
                Arc::new(RecordingExecutor::new(false)),
            );
            let task = tokio::spawn(async move {
                runner
                    .run(
                        AgentInput::new(vec![Message::user("go")], Vec::new()),
                        run_token,
                    )
                    .await
            });
            tokio::task::yield_now().await;
            token.cancel();
            let error = task
                .await
                .expect("task joins")
                .expect_err("run is cancelled");
            assert!(matches!(error, AgentError::Cancelled));
        }
    }

    #[tokio::test]
    async fn model_text_is_hard_capped_even_for_a_non_http_provider() {
        struct OversizedTextProvider;

        #[async_trait]
        impl Provider for OversizedTextProvider {
            fn kind(&self) -> ProviderKind {
                ProviderKind::OpenAi
            }

            fn model(&self) -> &'static str {
                "oversized"
            }

            fn context_budget(&self) -> ContextBudget {
                ContextBudget::new(None, 4, 80).expect("valid budget")
            }

            fn estimate_context(
                &self,
                _request: &ProviderRequest,
            ) -> Result<ContextUsage, ProviderError> {
                Ok(ContextUsage::default())
            }

            async fn stream(
                &self,
                _request: ProviderRequest,
                _cancellation: CancellationToken,
            ) -> Result<ProviderEventStream, ProviderError> {
                Ok(Box::pin(stream::iter([
                    Ok(ProviderEvent::TextDelta("123".to_owned())),
                    Ok(ProviderEvent::TextDelta("45".to_owned())),
                    Ok(ProviderEvent::Completed(StopReason::Stop)),
                ])))
            }
        }

        let runner = AgentRunner::new(
            Arc::new(OversizedTextProvider),
            Arc::new(RecordingExecutor::new(false)),
        );
        let (sender, mut receiver) = mpsc::channel(8);
        let error = runner
            .run_with_events(
                AgentInput::new(vec![Message::user("go")], Vec::new()),
                CancellationToken::new(),
                sender,
            )
            .await
            .expect_err("oversized model text must fail");

        assert!(matches!(
            error,
            AgentError::ModelTextTooLarge { round: 1, limit: 4 }
        ));
        let mut emitted = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            emitted.push(event);
        }
        assert!(emitted.iter().any(|event| matches!(
            event,
            crate::RunEvent::RunFailed { code } if code == "model_text_too_large"
        )));
        assert_eq!(
            emitted
                .iter()
                .filter(|event| matches!(event, crate::RunEvent::TextDelta { .. }))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn disconnected_trace_observer_does_not_change_run_outcome() {
        let provider = Arc::new(ScriptedProvider::new(vec![vec![
            Ok(ProviderEvent::TextDelta("done".to_owned())),
            Ok(ProviderEvent::Completed(StopReason::Stop)),
        ]]));
        let runner = AgentRunner::new(provider, Arc::new(RecordingExecutor::new(false)));
        let (sender, receiver) = mpsc::channel(1);
        drop(receiver);

        let result = runner
            .run_with_events(
                AgentInput::new(vec![Message::user("go")], Vec::new()),
                CancellationToken::new(),
                sender,
            )
            .await
            .expect("observer disconnect is non-fatal");
        assert_eq!(result.final_text, "done");
    }

    #[tokio::test]
    async fn bounded_trace_channel_applies_backpressure_without_losing_events() {
        let provider = Arc::new(ScriptedProvider::new(vec![vec![
            Ok(ProviderEvent::TextDelta("done".to_owned())),
            Ok(ProviderEvent::Completed(StopReason::Stop)),
        ]]));
        let runner = AgentRunner::new(provider, Arc::new(RecordingExecutor::new(false)));
        let (sender, mut receiver) = mpsc::channel(1);
        let consumer = tokio::spawn(async move {
            let mut events = Vec::new();
            while let Some(event) = receiver.recv().await {
                events.push(event);
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
            events
        });

        let result = runner
            .run_with_events(
                AgentInput::new(vec![Message::user("go")], Vec::new()),
                CancellationToken::new(),
                sender,
            )
            .await
            .expect("bounded trace delivery succeeds");
        assert_eq!(result.final_text, "done");
        let events = consumer.await.expect("consumer joins");
        assert_eq!(events.len(), 6);
        assert!(matches!(
            events.last(),
            Some(crate::RunEvent::RunCompleted { .. })
        ));
    }

    #[tokio::test]
    async fn unconsumed_trace_channel_cannot_outlive_the_run_deadline() {
        let provider = Arc::new(ScriptedProvider::new(vec![vec![
            Ok(ProviderEvent::TextDelta("done".to_owned())),
            Ok(ProviderEvent::Completed(StopReason::Stop)),
        ]]));
        let limits = AgentLimits::new(2, 2, 2, 1024, std::time::Duration::from_millis(10))
            .expect("valid limits");
        let runner =
            AgentRunner::new(provider, Arc::new(RecordingExecutor::new(false))).with_limits(limits);
        let (sender, _receiver) = mpsc::channel(1);

        let error = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            runner.run_with_events(
                AgentInput::new(vec![Message::user("go")], Vec::new()),
                CancellationToken::new(),
                sender,
            ),
        )
        .await
        .expect("runner must not hang on terminal event delivery")
        .expect_err("trace backpressure consumes the run deadline");
        assert!(matches!(error, AgentError::DeadlineExceeded));
    }

    #[tokio::test]
    async fn failed_trace_contains_only_a_stable_code() {
        let provider = Arc::new(ScriptedProvider::new(vec![vec![Err(
            ProviderError::Remote {
                provider: ProviderKind::OpenAi,
                code: "bad_request".to_owned(),
                message: "secret prompt fragment".to_owned(),
            },
        )]]));
        let runner = AgentRunner::new(provider, Arc::new(RecordingExecutor::new(false)));
        let (sender, mut receiver) = mpsc::channel(8);
        let error = runner
            .run_with_events(
                AgentInput::new(vec![Message::user("go")], Vec::new()),
                CancellationToken::new(),
                sender,
            )
            .await
            .expect_err("provider failure propagates");
        assert!(matches!(error, AgentError::Provider(_)));

        let mut emitted = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            emitted.push(event);
        }
        let debug = format!("{emitted:?}");
        assert!(debug.contains("provider_error"));
        assert!(!debug.contains("secret prompt fragment"));
    }

    #[tokio::test]
    async fn per_round_tool_limit_fails_before_any_tool_runs() {
        let events = (0..9)
            .map(|index| Ok(ProviderEvent::ToolCall(call(&format!("call-{index}")))))
            .chain([Ok(ProviderEvent::Completed(StopReason::ToolCalls))])
            .collect();
        let provider = Arc::new(ScriptedProvider::new(vec![events]));
        let executor = Arc::new(RecordingExecutor::new(false));
        let error = AgentRunner::new(provider, executor.clone())
            .run(
                AgentInput::new(vec![Message::user("go")], vec![tool()]),
                CancellationToken::new(),
            )
            .await
            .expect_err("ninth call must fail");
        assert!(matches!(error, AgentError::RoundToolLimit { limit: 8, .. }));
        assert!(executor.calls.lock().await.is_empty());
    }

    #[tokio::test]
    async fn total_tool_limit_is_enforced_across_rounds() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            vec![
                Ok(ProviderEvent::ToolCall(call("a"))),
                Ok(ProviderEvent::Completed(StopReason::ToolCalls)),
            ],
            vec![
                Ok(ProviderEvent::ToolCall(call("b"))),
                Ok(ProviderEvent::Completed(StopReason::ToolCalls)),
            ],
        ]));
        let executor = Arc::new(RecordingExecutor::new(false));
        let limits = AgentLimits::new(3, 1, 8, 64 * 1024, std::time::Duration::from_secs(300))
            .expect("valid limits");
        let error = AgentRunner::new(provider, executor.clone())
            .with_limits(limits)
            .run(
                AgentInput::new(vec![Message::user("go")], vec![tool()]),
                CancellationToken::new(),
            )
            .await
            .expect_err("second call exceeds total limit");
        assert!(matches!(error, AgentError::TotalToolLimit(1)));
        assert_eq!(*executor.calls.lock().await, ["a"]);
    }

    #[tokio::test]
    async fn final_allowed_round_does_not_start_unobservable_tool_side_effects() {
        let provider = Arc::new(ScriptedProvider::new(vec![vec![
            Ok(ProviderEvent::ToolCall(call("a"))),
            Ok(ProviderEvent::Completed(StopReason::ToolCalls)),
        ]]));
        let executor = Arc::new(RecordingExecutor::new(false));
        let limits = AgentLimits::new(1, 32, 8, 64 * 1024, std::time::Duration::from_secs(300))
            .expect("valid limits");
        let (sender, mut receiver) = mpsc::channel(16);
        let error = AgentRunner::new(provider, executor.clone())
            .with_limits(limits)
            .run_with_events(
                AgentInput::new(vec![Message::user("go")], vec![tool()]),
                CancellationToken::new(),
                sender,
            )
            .await
            .expect_err("tool request needs another model round");
        assert!(matches!(error, AgentError::ModelRoundLimit(1)));
        assert!(executor.calls.lock().await.is_empty());
        while let Ok(event) = receiver.try_recv() {
            assert!(
                !matches!(event, crate::RunEvent::TranscriptMessages { .. }),
                "an unexecutable final-round tool call must not enter the transcript"
            );
        }
    }

    #[tokio::test]
    async fn runner_rechecks_argument_limit_for_custom_providers() {
        let oversized = ToolCall::new(
            "a",
            "query",
            serde_json::json!({"value": "x".repeat(64 * 1024)}),
        )
        .expect("structurally valid call");
        let provider = Arc::new(ScriptedProvider::new(vec![vec![
            Ok(ProviderEvent::ToolCall(oversized)),
            Ok(ProviderEvent::Completed(StopReason::ToolCalls)),
        ]]));
        let executor = Arc::new(RecordingExecutor::new(false));
        let error = AgentRunner::new(provider, executor.clone())
            .run(
                AgentInput::new(vec![Message::user("go")], vec![tool()]),
                CancellationToken::new(),
            )
            .await
            .expect_err("oversized arguments must fail");
        assert!(matches!(
            error,
            AgentError::ToolArgumentsTooLarge { limit: 65536, .. }
        ));
        assert!(executor.calls.lock().await.is_empty());
    }

    #[test]
    fn defaults_match_the_runtime_contract() {
        let limits = AgentLimits::default();
        assert_eq!(limits.max_model_rounds(), 12);
        assert_eq!(limits.max_total_tool_calls(), 32);
        assert_eq!(limits.max_tool_calls_per_round(), 8);
        assert_eq!(limits.max_tool_argument_bytes(), 64 * 1024);
        assert_eq!(limits.max_duration(), std::time::Duration::from_secs(300));
    }
}
