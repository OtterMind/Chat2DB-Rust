use std::{collections::BTreeMap, future::Future, sync::Arc};

use async_trait::async_trait;
use chat2db_agent::{
    AgentError, AgentInput, AgentRunner, ExecutionOutcome, Message, MessageBlock, Provider, Role,
    RunEvent, RunResult, ToolCall, ToolExecutionError, ToolExecutor, ToolInvocation, ToolOutput,
    ToolOutputHandle, ToolResult, Usage,
};
use chat2db_contract::{
    AgentEvent, AgentMessageContent, AgentPermissionDecision, AgentPermissionRequest,
    AgentPermissionResponse, AgentRunAccepted, AgentRunSnapshot,
    AgentRunStatus as ContractRunStatus, AgentToolCall, AgentToolOutput, AgentUsage, ApiError,
    CancelAgentRunResponse, CancelDisposition, DecideAgentPermissionRequest, StartAgentRunRequest,
};
use chat2db_storage::{
    AgentMessageRecord, AgentMessageRole, AgentRunMessage, AgentRunRecord,
    AgentRunStatus as StorageRunStatus, AgentRunUpdate, CancelAgentRun, CompleteAgentRun,
    FailAgentRun, MAX_AGENT_MESSAGE_BYTES, MAX_AGENT_MESSAGE_PAGE_SIZE, SqlPermissionMode,
    StartAgentRun, Storage, StorageError, ToolPermissionDecision, ToolPermissionRecord,
    ToolPermissionStatus, UnknownAgentWrite,
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{
    AgentRunSubscription,
    hub::{AgentRunHub, AgentTransitionFailure, DurableAgentTransition},
    runtime::{RunProgress, map_context_compaction, persist_context_compaction},
    sql_tools::{
        SharedToolProgress, SqlToolExecutor, ToolProgress, UnknownWrite, is_write_tool,
        permission_status, tool_definitions,
    },
    transcript::CompactionOrdinalMap,
};
use crate::{AppError, Application, storage_call};

const RUN_EVENT_CHANNEL_CAPACITY: usize = 32;
const MAX_TERMINAL_ERROR_CODE_BYTES: usize = 128;
const MAX_TERMINAL_ERROR_MESSAGE_BYTES: usize = 4096;
const MAX_DURABLE_COUNTER: u64 = i64::MAX as u64;
const RESULT_HANDLE_MEDIA_TYPE: &str = "application/vnd.chat2db.result+json";

struct PreparedAgentRun {
    provider_id: String,
    datasource_id: Option<String>,
    permission_mode: SqlPermissionMode,
    messages: Vec<Message>,
    ordinals: CompactionOrdinalMap,
}

struct AgentWorkerState {
    progress: RunProgress,
    completed_usage: Usage,
    round_usage: Usage,
    active_round: Option<u64>,
    generated_messages: Vec<Message>,
    compaction_count: u64,
    compacted_through_ordinal: Option<u64>,
}

enum WorkerExit {
    Completed(RunResult),
    Failed {
        error: AppError,
        terminal_allowed: bool,
    },
    Cancelled,
    UnknownWrite {
        error: AppError,
        write: UnknownWrite,
    },
}

struct NoTools;

#[async_trait]
impl ToolExecutor for NoTools {
    async fn execute(
        &self,
        _invocation: ToolInvocation,
        _cancellation: CancellationToken,
    ) -> Result<ToolOutput, ToolExecutionError> {
        Err(ToolExecutionError::new(
            "tool_unavailable",
            "No tools are enabled for this agent run",
            ExecutionOutcome::NotStarted,
        ))
    }
}

impl Application {
    /// Accepts one durable conversation turn and starts its bounded provider loop.
    ///
    /// The initial run and user message commit before `Started` is registered.
    /// Provider credentials and transcript state are resolved by the owned worker
    /// after acceptance, so every later failure has a durable run to terminate.
    ///
    /// # Errors
    ///
    /// Returns validation, availability, capacity, or durable-storage errors
    /// that happen before the run can be accepted.
    pub async fn start_agent_run(
        &self,
        request: StartAgentRunRequest,
    ) -> Result<AgentRunAccepted, AppError> {
        self.start_agent_run_with_resolver(request, |application, provider_id| async move {
            application.resolve_agent_provider(&provider_id).await
        })
        .await
    }

    async fn start_agent_run_with_resolver<R, Fut>(
        &self,
        request: StartAgentRunRequest,
        resolver: R,
    ) -> Result<AgentRunAccepted, AppError>
    where
        R: FnOnce(Application, String) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Arc<dyn Provider>, AppError>> + Send + 'static,
    {
        let application = self.clone();
        let (response_sender, response_receiver) = oneshot::channel();
        tokio::spawn(async move {
            let response = application
                .coordinate_agent_run_start(request, resolver)
                .await;
            let _ = response_sender.send(response);
        });
        response_receiver
            .await
            .unwrap_or_else(|_| Err(AppError::internal()))
    }

    async fn coordinate_agent_run_start<R, Fut>(
        &self,
        request: StartAgentRunRequest,
        resolver: R,
    ) -> Result<AgentRunAccepted, AppError>
    where
        R: FnOnce(Application, String) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Arc<dyn Provider>, AppError>> + Send + 'static,
    {
        let storage = self.require_storage()?;
        let accepting_work = self.inner.accepting_work.lock().await;
        if !*accepting_work {
            return Err(AppError::unavailable(
                "runtime_shutting_down",
                "The Chat2DB runtime is shutting down",
            ));
        }
        let reservation = self.inner.agent_runs.reserve().await?;
        let session_id = request.session_id;
        let user_message = request.message;
        let sql_permission_mode = match request.sql_permission_mode {
            chat2db_contract::SqlPermissionMode::ReadOnly => SqlPermissionMode::ReadOnly,
            chat2db_contract::SqlPermissionMode::AskBeforeWrite => {
                SqlPermissionMode::AskBeforeWrite
            }
        };
        let input = StartAgentRun {
            user_message: user_message.clone(),
            sql_permission_mode,
        };
        let run_id = Uuid::new_v4().to_string();
        let user_message_id = Uuid::new_v4().to_string();
        let started = start_agent_run_durable(
            storage.clone(),
            &run_id,
            &user_message_id,
            &session_id,
            input,
            &user_message,
            sql_permission_mode,
        )
        .await?;
        let snapshot = match snapshot_from_run(started.run.clone(), None) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                fail_unowned_started_run(storage, started, error.api_error().clone()).await;
                return Err(error);
            }
        };
        let registered = match reservation.register_started(snapshot) {
            Ok(registered) => registered,
            Err(error) => {
                fail_unowned_started_run(storage, started, error.api_error().clone()).await;
                return Err(error);
            }
        };
        let run_id = registered.run_id().to_owned();
        let cancellation = registered.cancellation_token();
        let application = self.clone();
        let worker_storage = storage.clone();
        let worker_started = started.clone();
        let (start_gate, start_signal) = oneshot::channel();
        let worker = tokio::spawn(async move {
            if start_signal.await.is_err() {
                return;
            }
            application
                .run_agent_worker(worker_started, worker_storage, cancellation, resolver)
                .await;
        });
        if let Err(error) = self.inner.agent_runs.attach_task(&run_id, worker) {
            self.inner.agent_runs.abandon(&run_id);
            fail_unowned_started_run(storage, started, error.api_error().clone()).await;
            return Err(error);
        }
        if start_gate.send(()).is_err() {
            self.inner.agent_runs.abandon(&run_id);
            let error = AppError::internal();
            fail_unowned_started_run(storage, started, error.api_error().clone()).await;
            return Err(error);
        }
        drop(accepting_work);
        Ok(AgentRunAccepted { run_id, session_id })
    }

    /// Reads the authoritative durable state for one agent run.
    ///
    /// # Errors
    ///
    /// Returns not-found, availability, or persisted-data failures.
    pub async fn agent_run_snapshot(&self, run_id: &str) -> Result<AgentRunSnapshot, AppError> {
        let storage = self.require_storage()?;
        let run_id = run_id.to_owned();
        let loaded = storage_call(move || {
            let Some(run) = storage.get_agent_run(&run_id)? else {
                return Ok(None);
            };
            let permission = if run.status == StorageRunStatus::WaitingPermission {
                storage.get_active_tool_permission_for_run(&run_id)?
            } else {
                None
            };
            Ok(Some((run, permission)))
        })
        .await?;
        let (run, permission) = loaded.ok_or_else(|| {
            AppError::not_found("agent_run_not_found", "The agent run does not exist")
        })?;
        snapshot_from_run(run, permission.as_ref())
    }

    /// Atomically obtains retained replay followed by live events.
    ///
    /// Durable snapshots remain available when the process-local replay window
    /// is unavailable.
    ///
    /// # Errors
    ///
    /// Returns an invalid-cursor, replay-window, or stream-availability error.
    pub async fn subscribe_agent_run(
        &self,
        run_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<AgentRunSubscription, AppError> {
        self.inner
            .agent_runs
            .subscribe(run_id, after_sequence)
            .await
    }

    /// Resolves one exact pending permission and wakes only its bound tool call.
    ///
    /// The durable decision continues even if the delivery adapter disconnects.
    ///
    /// # Errors
    ///
    /// Returns invalid-input, not-found, conflict, expiry, or availability failures.
    pub async fn decide_agent_permission(
        &self,
        permission_id: &str,
        request: DecideAgentPermissionRequest,
    ) -> Result<AgentPermissionResponse, AppError> {
        let application = self.clone();
        let permission_id = permission_id.to_owned();
        let (response_sender, response_receiver) = oneshot::channel();
        tokio::spawn(async move {
            let response = application
                .coordinate_agent_permission_decision(&permission_id, request)
                .await;
            let _ = response_sender.send(response);
        });
        response_receiver
            .await
            .unwrap_or_else(|_| Err(AppError::internal()))
    }

    async fn coordinate_agent_permission_decision(
        &self,
        permission_id: &str,
        request: DecideAgentPermissionRequest,
    ) -> Result<AgentPermissionResponse, AppError> {
        let storage = self.require_storage()?;
        let digest = parse_hex_digest(&request.arguments_sha256)?;
        let decision = match request.decision {
            AgentPermissionDecision::AllowOnce => ToolPermissionDecision::Approve,
            AgentPermissionDecision::Deny => ToolPermissionDecision::Deny,
        };
        let run_id = request.run_id;
        let tool_call_id = request.tool_call_id;
        let permission_id = permission_id.to_owned();
        let response_permission_id = permission_id.clone();
        let transition_run_id = run_id.clone();
        let (status_sender, status_receiver) = oneshot::channel();
        self.inner
            .agent_runs
            .transition(&run_id, move |sequence| async move {
                let commit_storage = storage.clone();
                let commit_permission_id = permission_id.clone();
                let commit_run_id = transition_run_id.clone();
                let commit_tool_call_id = tool_call_id.clone();
                let permission = blocking_transition(move || {
                    let current = commit_storage
                        .get_tool_permission(&commit_permission_id)?
                        .ok_or_else(|| {
                            StorageError::PermissionNotFound(commit_permission_id.clone())
                        })?;
                    commit_storage.decide_tool_permission(
                        &commit_permission_id,
                        current.revision,
                        &commit_run_id,
                        &commit_tool_call_id,
                        digest,
                        sequence,
                        decision,
                    )
                })
                .await?;
                let status = permission_status(permission.status);
                if !matches!(
                    status,
                    chat2db_contract::AgentPermissionStatus::Approved
                        | chat2db_contract::AgentPermissionStatus::Denied
                        | chat2db_contract::AgentPermissionStatus::Expired
                ) {
                    return Err(AgentTransitionFailure::indeterminate(AppError::internal()));
                }
                let load_storage = storage;
                let load_run_id = transition_run_id;
                let missing_run_id = load_run_id.clone();
                let run =
                    tokio::task::spawn_blocking(move || load_storage.get_agent_run(&load_run_id))
                        .await
                        .map_err(|_| AgentTransitionFailure::indeterminate(AppError::internal()))?
                        .map_err(|error| {
                            AgentTransitionFailure::indeterminate(AppError::from(error))
                        })?
                        .ok_or_else(|| {
                            AgentTransitionFailure::indeterminate(AppError::from(
                                StorageError::AgentRunNotFound(missing_run_id),
                            ))
                        })?;
                let permission_for_snapshot =
                    (permission.status == ToolPermissionStatus::Approved).then_some(&permission);
                let mut snapshot = snapshot_from_run(run, permission_for_snapshot)
                    .map_err(AgentTransitionFailure::indeterminate)?;
                snapshot.status = ContractRunStatus::Running;
                snapshot.pending_permission = None;
                let durable = DurableAgentTransition::new(
                    snapshot,
                    AgentEvent::PermissionResolved {
                        permission_id,
                        status,
                    },
                );
                let _ = status_sender.send(status);
                Ok(durable)
            })
            .await?;
        let status = status_receiver.await.map_err(|_| AppError::internal())?;
        Ok(AgentPermissionResponse {
            permission_id: response_permission_id,
            status,
        })
    }

    /// Durably requests cancellation before signalling the owned worker.
    ///
    /// # Errors
    ///
    /// Returns availability or durable-storage failures. Unknown run ids are
    /// represented by an idempotent response rather than an error.
    pub async fn cancel_agent_run(&self, run_id: &str) -> Result<CancelAgentRunResponse, AppError> {
        let application = self.clone();
        let run_id = run_id.to_owned();
        let (response_sender, response_receiver) = oneshot::channel();
        tokio::spawn(async move {
            let response = application.coordinate_agent_run_cancellation(&run_id).await;
            let _ = response_sender.send(response);
        });
        response_receiver
            .await
            .unwrap_or_else(|_| Err(AppError::internal()))
    }

    async fn coordinate_agent_run_cancellation(
        &self,
        run_id: &str,
    ) -> Result<CancelAgentRunResponse, AppError> {
        let storage = self.require_storage()?;
        let durable = match request_agent_run_cancellation_durable(storage.clone(), run_id).await {
            Ok(Some(run)) => run,
            Ok(None) => {
                return Ok(CancelAgentRunResponse {
                    run_id: run_id.to_owned(),
                    disposition: CancelDisposition::UnknownOperation,
                });
            }
            Err(error) => {
                match self.inner.agent_runs.request_cancellation(run_id).await {
                    Err(signal_error)
                        if signal_error.api_error().code == "agent_event_stream_unavailable" =>
                    {
                        if let Ok(run) =
                            ensure_cancellation_requested(storage.clone(), run_id).await
                        {
                            finish_orphaned_cancellation(storage, run).await?;
                            return Ok(CancelAgentRunResponse {
                                run_id: run_id.to_owned(),
                                disposition: CancelDisposition::Accepted,
                            });
                        }
                    }
                    Ok(_) | Err(_) => {}
                }
                return Err(error);
            }
        };
        if is_storage_terminal(durable.status) {
            return Ok(CancelAgentRunResponse {
                run_id: run_id.to_owned(),
                disposition: CancelDisposition::AlreadyTerminal,
            });
        }

        match self.inner.agent_runs.request_cancellation(run_id).await {
            Ok(true) => {}
            Ok(false) => {
                let latest = self.agent_run_snapshot(run_id).await?;
                if !is_contract_terminal(latest.status) {
                    return Err(AppError::internal());
                }
                return Ok(CancelAgentRunResponse {
                    run_id: run_id.to_owned(),
                    disposition: CancelDisposition::AlreadyTerminal,
                });
            }
            Err(error) if error.api_error().code == "agent_event_stream_unavailable" => {
                finish_orphaned_cancellation(storage, durable).await?;
            }
            Err(error) => return Err(error),
        }
        Ok(CancelAgentRunResponse {
            run_id: run_id.to_owned(),
            disposition: CancelDisposition::Accepted,
        })
    }

    pub(crate) async fn persist_agent_shutdown_cancellations(&self, run_ids: &[String]) {
        let Ok(storage) = self.require_storage() else {
            return;
        };
        for run_id in run_ids {
            if let Err(error) =
                request_agent_run_cancellation_durable(storage.clone(), run_id).await
            {
                tracing::warn!(
                    run_id,
                    error_code = %error.api_error().code,
                    "agent shutdown cancellation could not be confirmed before signalling"
                );
            }
        }
    }

    pub(crate) async fn reconcile_agent_shutdown_runs(&self, run_ids: Vec<String>) {
        let Ok(storage) = self.require_storage() else {
            return;
        };
        for run_id in run_ids {
            let result = async {
                let run = load_agent_run_required(storage.clone(), &run_id).await?;
                if is_storage_terminal(run.status) {
                    return Ok(());
                }
                finish_orphaned_cancellation(storage.clone(), run).await
            }
            .await;
            if let Err(error) = result {
                tracing::warn!(
                    run_id,
                    error_code = %error.api_error().code,
                    "agent shutdown left a run for startup recovery"
                );
            }
        }
    }

    async fn run_agent_worker<R, Fut>(
        &self,
        started: chat2db_storage::StartedAgentRun,
        storage: Storage,
        cancellation: CancellationToken,
        resolver: R,
    ) where
        R: FnOnce(Application, String) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Arc<dyn Provider>, AppError>> + Send + 'static,
    {
        let run_id = started.run.id.clone();
        let mut state = AgentWorkerState::from_run(&started.run);
        let outcome = self
            .execute_agent_worker(
                &started,
                storage.clone(),
                cancellation.clone(),
                resolver,
                &mut state,
            )
            .await;
        let outcome = match outcome {
            WorkerExit::Completed(_result) if cancellation.is_cancelled() => WorkerExit::Cancelled,
            WorkerExit::Completed(result) => match state.apply_result(&result) {
                Ok(()) => WorkerExit::Completed(result),
                Err(error) => WorkerExit::Failed {
                    error,
                    terminal_allowed: true,
                },
            },
            WorkerExit::Failed {
                error,
                terminal_allowed,
            } => match durable_cancellation_requested(storage.clone(), &run_id).await {
                Ok(true) => WorkerExit::Cancelled,
                Ok(false) | Err(_) => WorkerExit::Failed {
                    error,
                    terminal_allowed,
                },
            },
            WorkerExit::Cancelled => WorkerExit::Cancelled,
            WorkerExit::UnknownWrite { error, write } => WorkerExit::UnknownWrite { error, write },
        };

        let terminal_result = match outcome {
            WorkerExit::Completed(result) => {
                self.finish_completed_worker(storage, &run_id, &state, &result)
                    .await
            }
            WorkerExit::Failed {
                error,
                terminal_allowed,
            } => {
                self.finish_failed_worker(storage, &run_id, &state, error, terminal_allowed)
                    .await
            }
            WorkerExit::Cancelled => {
                let terminal_allowed = hub_entry_available(&self.inner.agent_runs, &run_id).await;
                self.finish_cancelled_worker(storage, &run_id, &state, terminal_allowed)
                    .await
            }
            WorkerExit::UnknownWrite { error, write } => {
                self.finish_unknown_write_worker(storage, &run_id, &state, write, error)
                    .await
            }
        };
        if let Err(error) = terminal_result {
            tracing::warn!(
                run_id,
                error_code = %error.api_error().code,
                "agent worker could not persist its terminal state"
            );
        }
    }

    async fn finish_completed_worker(
        &self,
        storage: Storage,
        run_id: &str,
        state: &AgentWorkerState,
        result: &RunResult,
    ) -> Result<(), AppError> {
        let messages = match serialize_run_messages(&result.generated_messages) {
            Ok(messages) => messages,
            Err(error) => {
                return self
                    .finish_failed_worker(storage, run_id, state, error, true)
                    .await;
            }
        };
        let Err(error) = persist_completed_run(
            &self.inner.agent_runs,
            storage.clone(),
            run_id,
            state,
            messages,
        )
        .await
        else {
            return Ok(());
        };
        let terminal_allowed = hub_entry_available(&self.inner.agent_runs, run_id).await;
        if durable_cancellation_requested(storage.clone(), run_id)
            .await
            .unwrap_or(false)
        {
            self.finish_cancelled_worker(storage, run_id, state, terminal_allowed)
                .await
        } else {
            self.finish_failed_worker(storage, run_id, state, error, terminal_allowed)
                .await
        }
    }

    async fn execute_agent_worker<R, Fut>(
        &self,
        started: &chat2db_storage::StartedAgentRun,
        storage: Storage,
        cancellation: CancellationToken,
        resolver: R,
        state: &mut AgentWorkerState,
    ) -> WorkerExit
    where
        R: FnOnce(Application, String) -> Fut + Send,
        Fut: Future<Output = Result<Arc<dyn Provider>, AppError>> + Send,
    {
        if cancellation.is_cancelled() {
            return WorkerExit::Cancelled;
        }
        let prepared = match load_prepared_agent_run(storage.clone(), started).await {
            Ok(prepared) => prepared,
            Err(error) => {
                return WorkerExit::Failed {
                    error,
                    terminal_allowed: true,
                };
            }
        };
        if cancellation.is_cancelled() {
            return WorkerExit::Cancelled;
        }
        let resolution = resolver(self.clone(), prepared.provider_id);
        tokio::pin!(resolution);
        let provider = match tokio::select! {
            biased;
            () = cancellation.cancelled() => return WorkerExit::Cancelled,
            provider_result = &mut resolution => provider_result,
        } {
            Ok(provider) => provider,
            Err(error) => {
                return WorkerExit::Failed {
                    error,
                    terminal_allowed: true,
                };
            }
        };
        if cancellation.is_cancelled() {
            return WorkerExit::Cancelled;
        }
        let shared_progress = Arc::new(tokio::sync::Mutex::new(state.tool_progress()));
        let sql_executor = prepared.datasource_id.map(|datasource_id| {
            Arc::new(SqlToolExecutor::new(
                self.clone(),
                storage.clone(),
                started.run.id.clone(),
                started.run.session_id.clone(),
                datasource_id,
                prepared.permission_mode,
                Arc::clone(&shared_progress),
            ))
        });
        let executor: Arc<dyn ToolExecutor> = sql_executor.as_ref().map_or_else(
            || Arc::new(NoTools) as Arc<dyn ToolExecutor>,
            |executor| executor.clone(),
        );
        let tools = sql_executor
            .as_ref()
            .map_or_else(Vec::new, |_| tool_definitions(prepared.permission_mode));
        let runner = AgentRunner::new(provider, executor);
        let input = AgentInput::new(prepared.messages, tools);
        let driven = drive_agent_runner(
            runner,
            input,
            cancellation.clone(),
            &self.inner.agent_runs,
            storage,
            &started.run.id,
            state,
            shared_progress,
            prepared.ordinals,
        )
        .await;
        if let Some(write) = match &sql_executor {
            Some(executor) => executor.take_unknown_write().await,
            None => None,
        } {
            let error = match driven {
                Err(DriveFailure::Agent(error)) => AppError::from(error),
                Err(DriveFailure::Host { error, .. }) => error,
                Ok(_) => AppError::internal(),
            };
            return WorkerExit::UnknownWrite { error, write };
        }
        match driven {
            Ok(result) => WorkerExit::Completed(result),
            Err(DriveFailure::Agent(AgentError::Cancelled)) if cancellation.is_cancelled() => {
                WorkerExit::Cancelled
            }
            Err(DriveFailure::Agent(error)) => WorkerExit::Failed {
                error: AppError::from(error),
                terminal_allowed: true,
            },
            Err(DriveFailure::Host {
                error,
                terminal_allowed,
            }) => WorkerExit::Failed {
                error,
                terminal_allowed,
            },
        }
    }

    async fn finish_failed_worker(
        &self,
        storage: Storage,
        run_id: &str,
        state: &AgentWorkerState,
        error: AppError,
        terminal_allowed: bool,
    ) -> Result<(), AppError> {
        if !terminal_allowed {
            return Err(error);
        }
        let messages = serialize_run_messages(&state.generated_messages).unwrap_or_default();
        let safe_error = normalize_terminal_error(error.api_error());
        let first = persist_failed_run(
            &self.inner.agent_runs,
            storage.clone(),
            run_id,
            state,
            messages,
            safe_error.clone(),
        )
        .await;
        if first.is_ok() {
            return Ok(());
        }
        self.recover_failed_terminal(
            storage,
            run_id,
            safe_error,
            first.expect_err("checked error"),
        )
        .await
    }

    async fn finish_unknown_write_worker(
        &self,
        storage: Storage,
        run_id: &str,
        state: &AgentWorkerState,
        write: UnknownWrite,
        source: AppError,
    ) -> Result<(), AppError> {
        let messages = serialize_run_messages(&state.generated_messages).unwrap_or_default();
        let run_id_owned = run_id.to_owned();
        let progress = state.progress;
        let compaction_count = state.compaction_count;
        let compacted_through_ordinal = state.compacted_through_ordinal;
        let tool_call_id = write.tool_call_id;
        let arguments_sha256 = write.arguments_sha256;
        let error = ApiError::new(
            "database_outcome_unknown",
            "Database write outcome is unknown and must not be retried",
        );
        let event_error = error.clone();
        let result = self
            .inner
            .agent_runs
            .transition(run_id, move |sequence| async move {
                let failed = blocking_transition(move || {
                    storage.fail_agent_write_outcome_unknown(
                        &run_id_owned,
                        &tool_call_id,
                        arguments_sha256,
                        UnknownAgentWrite {
                            last_sequence: sequence,
                            model_rounds: progress.model_rounds,
                            tool_calls: progress.tool_calls,
                            input_tokens: progress.usage.input_tokens,
                            output_tokens: progress.usage.output_tokens,
                            total_tokens: progress.usage.total_tokens,
                            messages,
                            compaction_count,
                            compacted_through_ordinal,
                        },
                    )
                })
                .await?;
                let snapshot = snapshot_from_run(failed.run, None)
                    .map_err(AgentTransitionFailure::indeterminate)?;
                Ok(DurableAgentTransition::new(
                    snapshot,
                    AgentEvent::Failed { error: event_error },
                ))
            })
            .await
            .map(|_| ());
        result.map_err(|_| source)
    }

    async fn recover_failed_terminal(
        &self,
        storage: Storage,
        run_id: &str,
        error: ApiError,
        original_error: AppError,
    ) -> Result<(), AppError> {
        let durable = load_agent_run_required(storage.clone(), run_id).await?;
        if is_storage_terminal(durable.status) {
            return Ok(());
        }
        let durable_state = AgentWorkerState::from_run(&durable);
        if durable.cancel_requested {
            return self
                .finish_cancelled_from_durable(storage, durable, &durable_state)
                .await;
        }
        if hub_entry_available(&self.inner.agent_runs, run_id).await {
            let retry = persist_failed_run(
                &self.inner.agent_runs,
                storage.clone(),
                run_id,
                &durable_state,
                Vec::new(),
                error.clone(),
            )
            .await;
            if retry.is_ok() {
                return Ok(());
            }
            let latest = load_agent_run_required(storage.clone(), run_id).await?;
            if is_storage_terminal(latest.status) {
                return Ok(());
            }
            if latest.cancel_requested {
                let latest_state = AgentWorkerState::from_run(&latest);
                return self
                    .finish_cancelled_from_durable(storage, latest, &latest_state)
                    .await;
            }
            if hub_entry_available(&self.inner.agent_runs, run_id).await {
                return retry;
            }
            return finish_orphaned_failure(storage, latest, error).await;
        }
        finish_orphaned_failure(storage, durable, error)
            .await
            .map_err(|_| original_error)
    }

    async fn finish_cancelled_worker(
        &self,
        storage: Storage,
        run_id: &str,
        state: &AgentWorkerState,
        terminal_allowed: bool,
    ) -> Result<(), AppError> {
        if !terminal_allowed {
            return Err(AppError::internal());
        }
        let durable = ensure_cancellation_requested(storage.clone(), run_id).await?;
        if is_storage_terminal(durable.status) {
            return Ok(());
        }
        let messages = serialize_run_messages(&state.generated_messages).unwrap_or_default();
        let first = persist_cancelled_run(
            &self.inner.agent_runs,
            storage.clone(),
            run_id,
            state,
            messages,
        )
        .await;
        if first.is_ok() {
            return Ok(());
        }
        let latest = load_agent_run_required(storage.clone(), run_id).await?;
        if is_storage_terminal(latest.status) {
            return Ok(());
        }
        let latest_state = AgentWorkerState::from_run(&latest);
        self.finish_cancelled_from_durable(storage, latest, &latest_state)
            .await
    }

    async fn finish_cancelled_from_durable(
        &self,
        storage: Storage,
        run: AgentRunRecord,
        state: &AgentWorkerState,
    ) -> Result<(), AppError> {
        if hub_entry_available(&self.inner.agent_runs, &run.id).await {
            let result = persist_cancelled_run(
                &self.inner.agent_runs,
                storage.clone(),
                &run.id,
                state,
                Vec::new(),
            )
            .await;
            if result.is_ok() {
                return Ok(());
            }
            let latest = load_agent_run_required(storage.clone(), &run.id).await?;
            if is_storage_terminal(latest.status) {
                return Ok(());
            }
            if hub_entry_available(&self.inner.agent_runs, &run.id).await {
                return result;
            }
            return finish_orphaned_cancellation(storage, latest).await;
        }
        finish_orphaned_cancellation(storage, run).await
    }
}

async fn start_agent_run_durable(
    storage: Storage,
    run_id: &str,
    user_message_id: &str,
    session_id: &str,
    input: StartAgentRun,
    expected_message: &str,
    expected_permission_mode: SqlPermissionMode,
) -> Result<chat2db_storage::StartedAgentRun, AppError> {
    let start_storage = storage.clone();
    let start_run_id = run_id.to_owned();
    let start_message_id = user_message_id.to_owned();
    let start_session_id = session_id.to_owned();
    let outcome = tokio::task::spawn_blocking(move || {
        start_storage.start_agent_run_with_ids(
            &start_run_id,
            &start_message_id,
            &start_session_id,
            input,
        )
    })
    .await;
    let original_error = match outcome {
        Ok(Ok(started)) => return Ok(started),
        Ok(Err(error)) => AppError::from(error),
        Err(_) => AppError::internal(),
    };

    let readback_run_id = run_id.to_owned();
    let readback_message_id = user_message_id.to_owned();
    let readback_storage = storage;
    let readback = storage_call(move || {
        readback_storage.get_started_agent_run(&readback_run_id, &readback_message_id)
    })
    .await;
    match readback {
        Ok(Some(started))
            if started_run_matches(
                &started,
                run_id,
                user_message_id,
                session_id,
                expected_message,
                expected_permission_mode,
            ) =>
        {
            Ok(started)
        }
        Ok(_) => Err(original_error),
        Err(_) => Err(AppError::from(StorageError::OutcomeUnknown {
            operation: "start agent run",
            id: run_id.to_owned(),
        })),
    }
}

fn started_run_matches(
    started: &chat2db_storage::StartedAgentRun,
    run_id: &str,
    user_message_id: &str,
    session_id: &str,
    expected_message: &str,
    expected_permission_mode: SqlPermissionMode,
) -> bool {
    let content =
        serde_json::from_str::<Vec<AgentMessageContent>>(&started.user_message.content_json);
    started.run.id == run_id
        && started.run.session_id == session_id
        && started.run.status == StorageRunStatus::Running
        && started.run.sql_permission_mode == expected_permission_mode
        && started.run.last_sequence == 1
        && !started.run.cancel_requested
        && started.user_message.id == user_message_id
        && started.user_message.run_id.as_deref() == Some(run_id)
        && started.user_message.session_id == session_id
        && started.user_message.role == AgentMessageRole::User
        && matches!(
            content.as_deref(),
            Ok([AgentMessageContent::Text { text }]) if text == expected_message
        )
}

async fn fail_unowned_started_run(
    storage: Storage,
    started: chat2db_storage::StartedAgentRun,
    error: ApiError,
) {
    let run = started.run;
    let Some(sequence) = run.last_sequence.checked_add(1) else {
        return;
    };
    let safe_error = normalize_terminal_error(error);
    let run_id = run.id.clone();
    let result = storage_call(move || {
        storage.fail_agent_run(
            &run.id,
            run.status,
            FailAgentRun {
                last_sequence: sequence,
                model_rounds: run.model_rounds,
                tool_calls: run.tool_calls,
                input_tokens: run.input_tokens,
                output_tokens: run.output_tokens,
                total_tokens: run.total_tokens,
                error_code: safe_error.code,
                error_message: Some(safe_error.message),
                messages: Vec::new(),
                compaction_count: run.compaction_count,
                compacted_through_ordinal: run.compacted_through_ordinal,
            },
        )
    })
    .await;
    if let Err(persist_error) = result {
        tracing::warn!(
            run_id,
            error_code = %persist_error.api_error().code,
            "agent start handoff failure could not be finalized"
        );
    }
}

enum DriveFailure {
    Agent(AgentError),
    Host {
        error: AppError,
        terminal_allowed: bool,
    },
}

#[allow(clippy::too_many_arguments)]
async fn drive_agent_runner(
    runner: AgentRunner,
    input: AgentInput,
    cancellation: CancellationToken,
    hub: &AgentRunHub,
    storage: Storage,
    run_id: &str,
    state: &mut AgentWorkerState,
    tool_progress: SharedToolProgress,
    mut ordinals: CompactionOrdinalMap,
) -> Result<RunResult, DriveFailure> {
    let runner_cancellation = cancellation.child_token();
    let (events, mut receiver) = mpsc::channel(RUN_EVENT_CHANNEL_CAPACITY);
    let run = runner.run_with_events(input, runner_cancellation.clone(), events);
    tokio::pin!(run);

    let result = loop {
        tokio::select! {
            biased;
            event = receiver.recv() => {
                let Some(event) = event else {
                    break run.await;
                };
                if let Err(error) = process_run_event(
                    hub,
                    storage.clone(),
                    run_id,
                    state,
                    &tool_progress,
                    &mut ordinals,
                    event,
                ).await {
                    runner_cancellation.cancel();
                    drop(receiver);
                    let _ = run.await;
                    return Err(DriveFailure::Host {
                        terminal_allowed: hub_entry_available(hub, run_id).await,
                        error,
                    });
                }
            }
            result = &mut run => break result,
        }
    };

    while let Some(event) = receiver.recv().await {
        if let Err(error) = process_run_event(
            hub,
            storage.clone(),
            run_id,
            state,
            &tool_progress,
            &mut ordinals,
            event,
        )
        .await
        {
            return Err(DriveFailure::Host {
                terminal_allowed: hub_entry_available(hub, run_id).await,
                error,
            });
        }
    }
    result.map_err(DriveFailure::Agent)
}

async fn process_run_event(
    hub: &AgentRunHub,
    storage: Storage,
    run_id: &str,
    state: &mut AgentWorkerState,
    tool_progress: &SharedToolProgress,
    ordinals: &mut CompactionOrdinalMap,
    event: RunEvent,
) -> Result<(), AppError> {
    match event {
        RunEvent::RunStarted | RunEvent::RunFailed { .. } => Ok(()),
        RunEvent::ContextCompacted { compaction } => {
            let mapped = map_context_compaction(ordinals, &compaction)?;
            let (increments_count, coverage) = mapped.durable_effect();
            persist_context_compaction(hub, storage, run_id, state.progress, mapped).await?;
            if increments_count {
                state.compaction_count = state
                    .compaction_count
                    .checked_add(1)
                    .ok_or_else(AppError::internal)?;
                state.compacted_through_ordinal = coverage;
            }
            *tool_progress.lock().await = state.tool_progress();
            Ok(())
        }
        RunEvent::ModelRoundStarted { round } => state.start_round(round),
        RunEvent::TextDelta { round, text } => {
            state.require_active_round(round)?;
            persist_running_event(
                hub,
                storage,
                run_id,
                state,
                AgentEvent::TextDelta { delta: text },
            )
            .await
        }
        RunEvent::Usage { round, usage } => {
            let prior_progress = state.progress;
            let prior_round_usage = state.round_usage;
            state.merge_usage(round, usage)?;
            let persisted = persist_running_event(
                hub,
                storage,
                run_id,
                state,
                AgentEvent::Usage {
                    usage: contract_usage(state.progress.usage),
                },
            )
            .await;
            if persisted.is_err() {
                state.progress = prior_progress;
                state.round_usage = prior_round_usage;
            }
            persisted
        }
        RunEvent::ModelRoundCompleted { round, .. } => state.complete_round(round),
        RunEvent::TranscriptMessages { round, messages } => {
            if state.progress.model_rounds != usize_to_u64(round)? {
                return Err(AppError::internal());
            }
            ordinals.append_transient_messages(messages.len())?;
            state.generated_messages.extend(messages);
            Ok(())
        }
        RunEvent::RunCompleted { rounds, tool_calls } => {
            if state.progress.model_rounds != usize_to_u64(rounds)?
                || state.progress.tool_calls != usize_to_u64(tool_calls)?
            {
                return Err(AppError::internal());
            }
            Ok(())
        }
        event @ (RunEvent::ToolStarted { .. }
        | RunEvent::ToolCompleted { .. }
        | RunEvent::ToolFailed { .. }) => {
            process_tool_event(hub, storage, run_id, state, tool_progress, event).await
        }
    }
}

async fn process_tool_event(
    hub: &AgentRunHub,
    storage: Storage,
    run_id: &str,
    state: &mut AgentWorkerState,
    tool_progress: &SharedToolProgress,
    event: RunEvent,
) -> Result<(), AppError> {
    match event {
        RunEvent::ToolStarted {
            round,
            call_id,
            name,
            arguments_sha256,
        } => {
            state.start_tool_call(round)?;
            *tool_progress.lock().await = state.tool_progress();
            if is_write_tool(&name) {
                return Ok(());
            }
            persist_running_event(
                hub,
                storage,
                run_id,
                state,
                AgentEvent::ToolStarted {
                    tool_call_id: call_id,
                    name,
                    arguments_sha256: hex_digest(arguments_sha256),
                },
            )
            .await
        }
        RunEvent::ToolCompleted {
            round,
            call_id,
            name,
            output,
        } => {
            state.require_completed_round(round)?;
            let output = tool_output_to_contract(&output)?;
            persist_running_event(
                hub,
                storage,
                run_id,
                state,
                AgentEvent::ToolCompleted {
                    tool_call_id: call_id,
                    name,
                    output,
                },
            )
            .await
        }
        RunEvent::ToolFailed {
            round,
            call_id,
            name,
            code,
            message,
            outcome,
        } => {
            state.require_completed_round(round)?;
            if outcome == ExecutionOutcome::Unknown {
                return Ok(());
            }
            let error = normalize_terminal_error(ApiError::new(code, message));
            persist_running_event(
                hub,
                storage,
                run_id,
                state,
                AgentEvent::ToolFailed {
                    tool_call_id: call_id,
                    name,
                    error,
                },
            )
            .await
        }
        RunEvent::RunStarted
        | RunEvent::ContextCompacted { .. }
        | RunEvent::ModelRoundStarted { .. }
        | RunEvent::TextDelta { .. }
        | RunEvent::Usage { .. }
        | RunEvent::ModelRoundCompleted { .. }
        | RunEvent::TranscriptMessages { .. }
        | RunEvent::RunCompleted { .. }
        | RunEvent::RunFailed { .. } => Err(AppError::internal()),
    }
}

impl AgentWorkerState {
    fn from_run(run: &AgentRunRecord) -> Self {
        Self {
            progress: RunProgress {
                model_rounds: run.model_rounds,
                tool_calls: run.tool_calls,
                usage: Usage {
                    input_tokens: run.input_tokens,
                    output_tokens: run.output_tokens,
                    total_tokens: run.total_tokens,
                },
            },
            completed_usage: Usage {
                input_tokens: run.input_tokens,
                output_tokens: run.output_tokens,
                total_tokens: run.total_tokens,
            },
            round_usage: Usage::default(),
            active_round: None,
            generated_messages: Vec::new(),
            compaction_count: run.compaction_count,
            compacted_through_ordinal: run.compacted_through_ordinal,
        }
    }

    fn start_round(&mut self, round: usize) -> Result<(), AppError> {
        let round = durable_count_from_usize(round)?;
        let next_round = self
            .progress
            .model_rounds
            .checked_add(1)
            .filter(|value| *value <= MAX_DURABLE_COUNTER)
            .ok_or_else(|| provider_protocol_error("The AI provider exceeded durable counters"))?;
        if self.active_round.is_some() || round != next_round {
            return Err(AppError::internal());
        }
        self.active_round = Some(round);
        self.round_usage = Usage::default();
        Ok(())
    }

    fn require_completed_round(&self, round: usize) -> Result<(), AppError> {
        if self.active_round.is_none() && self.progress.model_rounds == usize_to_u64(round)? {
            Ok(())
        } else {
            Err(AppError::internal())
        }
    }

    fn start_tool_call(&mut self, round: usize) -> Result<(), AppError> {
        self.require_completed_round(round)?;
        self.progress.tool_calls = self
            .progress
            .tool_calls
            .checked_add(1)
            .filter(|value| *value <= MAX_DURABLE_COUNTER)
            .ok_or_else(|| provider_protocol_error("The AI provider exceeded durable counters"))?;
        Ok(())
    }

    fn tool_progress(&self) -> ToolProgress {
        ToolProgress {
            model_rounds: self.progress.model_rounds,
            tool_calls: self.progress.tool_calls,
            usage: self.progress.usage,
            compaction_count: self.compaction_count,
            compacted_through_ordinal: self.compacted_through_ordinal,
        }
    }

    fn require_active_round(&self, round: usize) -> Result<(), AppError> {
        if self.active_round == Some(usize_to_u64(round)?) {
            Ok(())
        } else {
            Err(AppError::internal())
        }
    }

    fn merge_usage(&mut self, round: usize, usage: Usage) -> Result<(), AppError> {
        self.require_active_round(round)?;
        let merged = normalize_durable_usage(Usage {
            input_tokens: self.round_usage.input_tokens.max(usage.input_tokens),
            output_tokens: self.round_usage.output_tokens.max(usage.output_tokens),
            total_tokens: self.round_usage.total_tokens.max(usage.total_tokens),
        })?;
        let accumulated = add_usage(self.completed_usage, merged)?;
        self.round_usage = merged;
        self.progress.usage = accumulated;
        Ok(())
    }

    fn complete_round(&mut self, round: usize) -> Result<(), AppError> {
        self.require_active_round(round)?;
        let round = durable_count_from_usize(round)?;
        self.progress.model_rounds = round;
        self.completed_usage = self.progress.usage;
        self.round_usage = Usage::default();
        self.active_round = None;
        Ok(())
    }

    fn apply_result(&mut self, result: &RunResult) -> Result<(), AppError> {
        let model_rounds = durable_count_from_usize(result.model_rounds)?;
        let tool_calls = durable_count_from_usize(result.tool_calls)?;
        let usage = normalize_durable_usage(result.usage)?;
        self.progress.model_rounds = model_rounds;
        self.progress.tool_calls = tool_calls;
        self.progress.usage = usage;
        self.completed_usage = usage;
        self.active_round = None;
        self.round_usage = Usage::default();
        Ok(())
    }
}

fn normalize_durable_usage(usage: Usage) -> Result<Usage, AppError> {
    let minimum_total = usage
        .input_tokens
        .checked_add(usage.output_tokens)
        .filter(|value| *value <= MAX_DURABLE_COUNTER)
        .ok_or_else(|| provider_protocol_error("The AI provider returned invalid usage"))?;
    if usage.input_tokens > MAX_DURABLE_COUNTER
        || usage.output_tokens > MAX_DURABLE_COUNTER
        || usage.total_tokens > MAX_DURABLE_COUNTER
    {
        return Err(provider_protocol_error(
            "The AI provider returned invalid usage",
        ));
    }
    Ok(Usage {
        total_tokens: usage.total_tokens.max(minimum_total),
        ..usage
    })
}

fn add_usage(left: Usage, right: Usage) -> Result<Usage, AppError> {
    normalize_durable_usage(Usage {
        input_tokens: left
            .input_tokens
            .checked_add(right.input_tokens)
            .ok_or_else(|| provider_protocol_error("The AI provider returned invalid usage"))?,
        output_tokens: left
            .output_tokens
            .checked_add(right.output_tokens)
            .ok_or_else(|| provider_protocol_error("The AI provider returned invalid usage"))?,
        total_tokens: left
            .total_tokens
            .checked_add(right.total_tokens)
            .ok_or_else(|| provider_protocol_error("The AI provider returned invalid usage"))?,
    })
}

async fn load_prepared_agent_run(
    storage: Storage,
    started: &chat2db_storage::StartedAgentRun,
) -> Result<PreparedAgentRun, AppError> {
    let session_id = started.run.session_id.clone();
    let expected_user_message = started.user_message.clone();
    let loaded = storage_call(move || {
        let session = storage
            .get_agent_session(&session_id)?
            .ok_or_else(|| StorageError::AgentSessionNotFound(session_id.clone()))?;
        let mut messages = Vec::new();
        let mut start_ordinal = 0_u64;
        loop {
            let page = storage.list_agent_messages(
                &session_id,
                start_ordinal,
                MAX_AGENT_MESSAGE_PAGE_SIZE,
            )?;
            let page_is_full = page.len()
                == usize::try_from(MAX_AGENT_MESSAGE_PAGE_SIZE)
                    .map_err(|_| StorageError::NumericRange("agent message page size"))?;
            let next_ordinal = page
                .last()
                .map(|message| message.ordinal)
                .and_then(|ordinal| ordinal.checked_add(1));
            messages.extend(page);
            if !page_is_full {
                break;
            }
            start_ordinal =
                next_ordinal.ok_or(StorageError::NumericRange("agent message ordinal"))?;
        }
        let coverage = storage.get_agent_session_compaction_coverage(&session_id)?;
        Ok((session, messages, coverage))
    })
    .await?;
    let (session, records, coverage) = loaded;
    if records.last() != Some(&expected_user_message) {
        return Err(AppError::internal());
    }
    let (messages, ordinals) = prepare_transcript(&records, coverage)?;
    Ok(PreparedAgentRun {
        provider_id: session.provider_id,
        datasource_id: session.datasource_id,
        permission_mode: started.run.sql_permission_mode,
        messages,
        ordinals,
    })
}

fn prepare_transcript(
    records: &[AgentMessageRecord],
    coverage: Option<u64>,
) -> Result<(Vec<Message>, CompactionOrdinalMap), AppError> {
    let summary_index = coverage.and_then(|coverage| {
        records
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                record.role == AgentMessageRole::Summary
                    && record
                        .summary_through_ordinal
                        .is_some_and(|through| through <= coverage)
            })
            .max_by_key(|(_, record)| record.ordinal)
            .map(|(index, _)| index)
    });
    let mut messages = Vec::new();
    let mut ordinals = Vec::new();

    for record in records
        .iter()
        .filter(|record| record.role == AgentMessageRole::System)
    {
        messages.push(message_from_record(record)?);
        ordinals.push(record.ordinal);
    }
    if let Some(index) = summary_index {
        let summary = summary_message(&records[index])?;
        messages.push(summary);
        ordinals.push(records[index].ordinal);
    }
    for (index, record) in records.iter().enumerate() {
        if record.role == AgentMessageRole::System
            || record.role == AgentMessageRole::Summary
            || coverage.is_some_and(|coverage| record.ordinal <= coverage)
            || Some(index) == summary_index
        {
            continue;
        }
        messages.push(message_from_record(record)?);
        ordinals.push(record.ordinal);
    }
    if messages.is_empty() {
        return Err(AppError::internal());
    }
    Ok((messages, CompactionOrdinalMap::new(ordinals)))
}

fn summary_message(record: &AgentMessageRecord) -> Result<Message, AppError> {
    let content = decode_message_content(record)?;
    let mut text = String::new();
    for block in content {
        let AgentMessageContent::Text { text: block } = block else {
            return Err(AppError::internal());
        };
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&block);
    }
    if text.trim().is_empty() {
        return Err(AppError::internal());
    }
    Ok(Message::system(format!("Conversation summary:\n{text}")))
}

fn message_from_record(record: &AgentMessageRecord) -> Result<Message, AppError> {
    let content = decode_message_content(record)?;
    match record.role {
        AgentMessageRole::System | AgentMessageRole::User => {
            let role = if record.role == AgentMessageRole::System {
                Role::System
            } else {
                Role::User
            };
            let blocks = content
                .into_iter()
                .map(|block| match block {
                    AgentMessageContent::Text { text } if !text.is_empty() => {
                        Ok(MessageBlock::Text(text))
                    }
                    _ => Err(AppError::internal()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Message::new(role, blocks))
        }
        AgentMessageRole::Assistant => {
            let mut blocks = Vec::new();
            for block in content {
                match block {
                    AgentMessageContent::Text { text } if !text.is_empty() => {
                        blocks.push(MessageBlock::Text(text));
                    }
                    AgentMessageContent::ToolCalls { calls } if !calls.is_empty() => {
                        for call in calls {
                            let arguments = serde_json::from_str(&call.arguments_json)
                                .map_err(|_| AppError::internal())?;
                            let call = ToolCall::new(call.id, call.name, arguments)
                                .map_err(|_| AppError::internal())?;
                            blocks.push(MessageBlock::ToolCall(call));
                        }
                    }
                    _ => return Err(AppError::internal()),
                }
            }
            Ok(Message::new(Role::Assistant, blocks))
        }
        AgentMessageRole::Tool => {
            let [
                AgentMessageContent::ToolResult {
                    tool_call_id,
                    name,
                    output,
                },
            ] = content.as_slice()
            else {
                return Err(AppError::internal());
            };
            let output = tool_output_from_contract(output)?;
            Ok(Message::new(
                Role::Tool,
                vec![MessageBlock::ToolResult(ToolResult::named(
                    tool_call_id,
                    name,
                    output,
                ))],
            ))
        }
        AgentMessageRole::Summary => Err(AppError::internal()),
    }
}

fn decode_message_content(
    record: &AgentMessageRecord,
) -> Result<Vec<AgentMessageContent>, AppError> {
    serde_json::from_str(&record.content_json).map_err(|_| AppError::internal())
}

fn tool_output_from_contract(output: &AgentToolOutput) -> Result<ToolOutput, AppError> {
    match output {
        AgentToolOutput::Text { content, .. } => {
            ToolOutput::content(content.clone()).map_err(|_| AppError::internal())
        }
        AgentToolOutput::Result { handle } => {
            let mut metadata = BTreeMap::new();
            metadata.insert("rowCount".to_owned(), handle.row_count.clone());
            metadata.insert("byteCount".to_owned(), handle.byte_count.clone());
            metadata.insert("expiresAtMs".to_owned(), handle.expires_at_ms.clone());
            let output_handle = ToolOutputHandle::new(
                handle.handle_id.clone(),
                Some(RESULT_HANDLE_MEDIA_TYPE.to_owned()),
                metadata,
            )
            .map_err(|_| AppError::internal())?;
            let preview = serde_json::to_string(handle).map_err(|_| AppError::internal())?;
            ToolOutput::content_and_handle(preview, output_handle).map_err(|_| AppError::internal())
        }
    }
}

fn tool_output_to_contract(output: &ToolOutput) -> Result<AgentToolOutput, AppError> {
    let Some(handle) = output.output_handle() else {
        return Ok(AgentToolOutput::Text {
            content: output
                .inline_content()
                .ok_or_else(AppError::internal)?
                .to_owned(),
            truncated: false,
        });
    };
    if handle.media_type() != Some(RESULT_HANDLE_MEDIA_TYPE) {
        return Err(AppError::internal());
    }
    let result = serde_json::from_str::<chat2db_contract::AgentResultHandle>(
        output.inline_content().ok_or_else(AppError::internal)?,
    )
    .map_err(|_| AppError::internal())?;
    if result.handle_id != handle.id() {
        return Err(AppError::internal());
    }
    Ok(AgentToolOutput::Result {
        handle: Box::new(result),
    })
}

fn serialize_run_messages(messages: &[Message]) -> Result<Vec<AgentRunMessage>, AppError> {
    messages.iter().map(serialize_run_message).collect()
}

fn serialize_run_message(message: &Message) -> Result<AgentRunMessage, AppError> {
    let (role, content) = match message.role() {
        Role::Assistant => {
            let mut content = Vec::new();
            for block in message.blocks() {
                match block {
                    MessageBlock::Text(text) if !text.is_empty() => {
                        content.push(AgentMessageContent::Text { text: text.clone() });
                    }
                    MessageBlock::ToolCall(call) => {
                        content.push(AgentMessageContent::ToolCalls {
                            calls: vec![AgentToolCall {
                                id: call.id().to_owned(),
                                name: call.name().to_owned(),
                                arguments_json: serde_json::to_string(call.arguments())
                                    .map_err(|_| AppError::internal())?,
                            }],
                        });
                    }
                    MessageBlock::ProviderContinuation(_) => {}
                    MessageBlock::Text(_) | MessageBlock::ToolResult(_) => {
                        return Err(AppError::internal());
                    }
                }
            }
            (AgentMessageRole::Assistant, content)
        }
        Role::Tool => {
            let [MessageBlock::ToolResult(result)] = message.blocks() else {
                return Err(AppError::internal());
            };
            let name = result.name().ok_or_else(AppError::internal)?;
            (
                AgentMessageRole::Tool,
                vec![AgentMessageContent::ToolResult {
                    tool_call_id: result.call_id().to_owned(),
                    name: name.to_owned(),
                    output: tool_output_to_contract(result.output())?,
                }],
            )
        }
        Role::System | Role::User => return Err(AppError::internal()),
    };
    if content.is_empty() {
        return Err(AppError::new(
            crate::AppErrorKind::Unavailable,
            ApiError::new(
                "provider_protocol_error",
                "The AI provider returned no visible assistant content",
            ),
        ));
    }
    let content_json = serde_json::to_string(&content).map_err(|_| AppError::internal())?;
    if content_json.len() > usize::try_from(MAX_AGENT_MESSAGE_BYTES).unwrap_or(usize::MAX) {
        return Err(provider_protocol_error(
            "The AI provider response exceeds the durable message limit",
        ));
    }
    Ok(AgentRunMessage {
        role,
        summary_through_ordinal: None,
        content_json,
    })
}

async fn persist_running_event(
    hub: &AgentRunHub,
    storage: Storage,
    run_id: &str,
    state: &AgentWorkerState,
    event: AgentEvent,
) -> Result<(), AppError> {
    let run_id_owned = run_id.to_owned();
    let progress = state.progress;
    let compaction_count = state.compaction_count;
    let compacted_through_ordinal = state.compacted_through_ordinal;
    hub.transition(run_id, move |sequence| async move {
        let updated = blocking_transition(move || {
            storage.update_agent_run(
                &run_id_owned,
                StorageRunStatus::Running,
                AgentRunUpdate {
                    status: StorageRunStatus::Running,
                    last_sequence: sequence,
                    model_rounds: progress.model_rounds,
                    tool_calls: progress.tool_calls,
                    input_tokens: progress.usage.input_tokens,
                    output_tokens: progress.usage.output_tokens,
                    total_tokens: progress.usage.total_tokens,
                    compaction_count,
                    compacted_through_ordinal,
                },
            )
        })
        .await?;
        let snapshot =
            snapshot_from_run(updated, None).map_err(AgentTransitionFailure::indeterminate)?;
        Ok(DurableAgentTransition::new(snapshot, event))
    })
    .await
    .map(|_| ())
}

async fn persist_completed_run(
    hub: &AgentRunHub,
    storage: Storage,
    run_id: &str,
    state: &AgentWorkerState,
    messages: Vec<AgentRunMessage>,
) -> Result<(), AppError> {
    let run_id_owned = run_id.to_owned();
    let progress = state.progress;
    let compaction_count = state.compaction_count;
    let compacted_through_ordinal = state.compacted_through_ordinal;
    hub.transition(run_id, move |sequence| async move {
        let completed = blocking_transition(move || {
            storage.complete_agent_run(
                &run_id_owned,
                StorageRunStatus::Running,
                CompleteAgentRun {
                    last_sequence: sequence,
                    model_rounds: progress.model_rounds,
                    tool_calls: progress.tool_calls,
                    input_tokens: progress.usage.input_tokens,
                    output_tokens: progress.usage.output_tokens,
                    total_tokens: progress.usage.total_tokens,
                    messages,
                    compaction_count,
                    compacted_through_ordinal,
                },
            )
        })
        .await?;
        let message_id = completed
            .run
            .message_id
            .clone()
            .ok_or_else(|| AgentTransitionFailure::indeterminate(AppError::internal()))?;
        let snapshot = snapshot_from_run(completed.run, None)
            .map_err(AgentTransitionFailure::indeterminate)?;
        Ok(DurableAgentTransition::new(
            snapshot,
            AgentEvent::Completed { message_id },
        ))
    })
    .await
    .map(|_| ())
}

async fn persist_failed_run(
    hub: &AgentRunHub,
    storage: Storage,
    run_id: &str,
    state: &AgentWorkerState,
    messages: Vec<AgentRunMessage>,
    error: ApiError,
) -> Result<(), AppError> {
    let run_id_owned = run_id.to_owned();
    let progress = state.progress;
    let compaction_count = state.compaction_count;
    let compacted_through_ordinal = state.compacted_through_ordinal;
    let stored_error = error.clone();
    hub.transition(run_id, move |sequence| async move {
        let failed = blocking_transition(move || {
            storage.fail_agent_run(
                &run_id_owned,
                StorageRunStatus::Running,
                FailAgentRun {
                    last_sequence: sequence,
                    model_rounds: progress.model_rounds,
                    tool_calls: progress.tool_calls,
                    input_tokens: progress.usage.input_tokens,
                    output_tokens: progress.usage.output_tokens,
                    total_tokens: progress.usage.total_tokens,
                    error_code: stored_error.code,
                    error_message: Some(stored_error.message),
                    messages,
                    compaction_count,
                    compacted_through_ordinal,
                },
            )
        })
        .await?;
        let snapshot =
            snapshot_from_run(failed.run, None).map_err(AgentTransitionFailure::indeterminate)?;
        Ok(DurableAgentTransition::new(
            snapshot,
            AgentEvent::Failed { error },
        ))
    })
    .await
    .map(|_| ())
}

async fn persist_cancelled_run(
    hub: &AgentRunHub,
    storage: Storage,
    run_id: &str,
    state: &AgentWorkerState,
    messages: Vec<AgentRunMessage>,
) -> Result<(), AppError> {
    let run_id_owned = run_id.to_owned();
    let progress = state.progress;
    let compaction_count = state.compaction_count;
    let compacted_through_ordinal = state.compacted_through_ordinal;
    hub.transition(run_id, move |sequence| async move {
        let cancelled = blocking_transition(move || {
            storage.finish_cancelled_agent_run(
                &run_id_owned,
                StorageRunStatus::Running,
                CancelAgentRun {
                    last_sequence: sequence,
                    model_rounds: progress.model_rounds,
                    tool_calls: progress.tool_calls,
                    input_tokens: progress.usage.input_tokens,
                    output_tokens: progress.usage.output_tokens,
                    total_tokens: progress.usage.total_tokens,
                    messages,
                    compaction_count,
                    compacted_through_ordinal,
                },
            )
        })
        .await?;
        let snapshot = snapshot_from_run(cancelled.run, None)
            .map_err(AgentTransitionFailure::indeterminate)?;
        Ok(DurableAgentTransition::new(
            snapshot,
            AgentEvent::Cancelled { reason: None },
        ))
    })
    .await
    .map(|_| ())
}

pub(super) async fn blocking_transition<T, F>(operation: F) -> Result<T, AgentTransitionFailure>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, StorageError> + Send + 'static,
{
    match tokio::task::spawn_blocking(operation).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(classify_storage_transition(error)),
        Err(_) => Err(AgentTransitionFailure::indeterminate(AppError::internal())),
    }
}

fn classify_storage_transition(error: StorageError) -> AgentTransitionFailure {
    let definitely_not_committed = matches!(
        error,
        StorageError::AgentRunNotFound(_)
            | StorageError::AgentStateConflict { .. }
            | StorageError::InvalidAgent(_)
            | StorageError::AgentQuotaExceeded { .. }
            | StorageError::PermissionNotFound(_)
            | StorageError::PermissionRevisionConflict { .. }
            | StorageError::PermissionNotExecutable { .. }
    );
    let error = AppError::from(error);
    if definitely_not_committed {
        AgentTransitionFailure::definitely_not_committed(error)
    } else {
        AgentTransitionFailure::indeterminate(error)
    }
}

async fn request_agent_run_cancellation_durable(
    storage: Storage,
    run_id: &str,
) -> Result<Option<AgentRunRecord>, AppError> {
    let requested_id = run_id.to_owned();
    let cancel_storage = storage.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        cancel_storage.request_agent_run_cancellation(&requested_id)
    })
    .await;
    let original_error = match outcome {
        Ok(Ok(run)) => return Ok(Some(run)),
        Ok(Err(StorageError::AgentRunNotFound(_))) => return Ok(None),
        Ok(Err(error)) => AppError::from(error),
        Err(_) => AppError::internal(),
    };

    let readback_id = run_id.to_owned();
    let readback = storage_call(move || storage.get_agent_run(&readback_id)).await;
    match readback {
        Ok(Some(run)) if is_storage_terminal(run.status) || run.cancel_requested => Ok(Some(run)),
        Ok(_) => Err(original_error),
        Err(_) => Err(AppError::from(StorageError::OutcomeUnknown {
            operation: "request agent run cancellation",
            id: run_id.to_owned(),
        })),
    }
}

async fn ensure_cancellation_requested(
    storage: Storage,
    run_id: &str,
) -> Result<AgentRunRecord, AppError> {
    let requested_id = run_id.to_owned();
    let request_storage = storage.clone();
    let result = storage_call(move || {
        let run_id = requested_id;
        let run = request_storage
            .get_agent_run(&run_id)?
            .ok_or_else(|| StorageError::AgentRunNotFound(run_id.clone()))?;
        if is_storage_terminal(run.status) || run.cancel_requested {
            Ok(run)
        } else {
            request_storage.request_agent_run_cancellation(&run_id)
        }
    })
    .await;
    match result {
        Ok(run) => Ok(run),
        Err(original_error) => {
            let latest = load_agent_run_required(storage, run_id).await?;
            if is_storage_terminal(latest.status) || latest.cancel_requested {
                Ok(latest)
            } else {
                Err(original_error)
            }
        }
    }
}

async fn durable_cancellation_requested(storage: Storage, run_id: &str) -> Result<bool, AppError> {
    let run_id = run_id.to_owned();
    storage_call(move || storage.get_agent_run(&run_id))
        .await
        .map(|run| run.is_some_and(|run| run.cancel_requested || is_storage_terminal(run.status)))
}

async fn finish_orphaned_cancellation(
    storage: Storage,
    mut run: AgentRunRecord,
) -> Result<(), AppError> {
    if !run.cancel_requested && !is_storage_terminal(run.status) {
        run = ensure_cancellation_requested(storage.clone(), &run.id).await?;
    }
    if is_storage_terminal(run.status) {
        return Ok(());
    }
    let sequence = run
        .last_sequence
        .checked_add(1)
        .ok_or_else(AppError::internal)?;
    let run_id = run.id.clone();
    let readback_storage = storage.clone();
    let result = storage_call(move || {
        storage.finish_cancelled_agent_run(
            &run.id,
            run.status,
            CancelAgentRun {
                last_sequence: sequence,
                model_rounds: run.model_rounds,
                tool_calls: run.tool_calls,
                input_tokens: run.input_tokens,
                output_tokens: run.output_tokens,
                total_tokens: run.total_tokens,
                messages: Vec::new(),
                compaction_count: run.compaction_count,
                compacted_through_ordinal: run.compacted_through_ordinal,
            },
        )
    })
    .await;
    match result {
        Ok(_) => Ok(()),
        Err(original_error) => {
            let latest = load_agent_run_required(readback_storage.clone(), &run_id).await?;
            if is_storage_terminal(latest.status) {
                Ok(())
            } else {
                Err(original_error)
            }
        }
    }
}

async fn finish_orphaned_failure(
    storage: Storage,
    run: AgentRunRecord,
    error: ApiError,
) -> Result<(), AppError> {
    if is_storage_terminal(run.status) {
        return Ok(());
    }
    if run.cancel_requested {
        return finish_orphaned_cancellation(storage, run).await;
    }
    let sequence = run
        .last_sequence
        .checked_add(1)
        .ok_or_else(AppError::internal)?;
    let safe_error = normalize_terminal_error(error);
    let run_id = run.id.clone();
    let readback_storage = storage.clone();
    let result = storage_call(move || {
        storage.fail_agent_run(
            &run.id,
            run.status,
            FailAgentRun {
                last_sequence: sequence,
                model_rounds: run.model_rounds,
                tool_calls: run.tool_calls,
                input_tokens: run.input_tokens,
                output_tokens: run.output_tokens,
                total_tokens: run.total_tokens,
                error_code: safe_error.code,
                error_message: Some(safe_error.message),
                messages: Vec::new(),
                compaction_count: run.compaction_count,
                compacted_through_ordinal: run.compacted_through_ordinal,
            },
        )
    })
    .await;
    match result {
        Ok(_) => Ok(()),
        Err(original_error) => {
            let latest = load_agent_run_required(readback_storage.clone(), &run_id).await?;
            if is_storage_terminal(latest.status) {
                Ok(())
            } else if latest.cancel_requested {
                finish_orphaned_cancellation(readback_storage, latest).await
            } else {
                Err(original_error)
            }
        }
    }
}

async fn load_agent_run_required(
    storage: Storage,
    run_id: &str,
) -> Result<AgentRunRecord, AppError> {
    let run_id = run_id.to_owned();
    storage_call(move || {
        storage
            .get_agent_run(&run_id)?
            .ok_or(StorageError::AgentRunNotFound(run_id))
    })
    .await
}

async fn hub_entry_available(hub: &AgentRunHub, run_id: &str) -> bool {
    hub.cached_snapshot(run_id).await.is_ok()
}

pub(super) fn snapshot_from_run(
    run: AgentRunRecord,
    permission: Option<&ToolPermissionRecord>,
) -> Result<AgentRunSnapshot, AppError> {
    let status = match run.status {
        StorageRunStatus::Running => ContractRunStatus::Running,
        StorageRunStatus::WaitingPermission => ContractRunStatus::WaitingForPermission,
        StorageRunStatus::Completed => ContractRunStatus::Completed,
        StorageRunStatus::Failed => ContractRunStatus::Failed,
        StorageRunStatus::Cancelled => ContractRunStatus::Cancelled,
    };
    let pending_permission = permission.map(permission_request).transpose()?;
    if (run.status == StorageRunStatus::WaitingPermission) != pending_permission.is_some() {
        return Err(AppError::internal());
    }
    let error = match run.status {
        StorageRunStatus::Failed => Some(ApiError::new(
            run.error_code.clone().ok_or_else(AppError::internal)?,
            run.error_message
                .clone()
                .unwrap_or_else(|| "The agent run failed".to_owned()),
        )),
        _ => None,
    };
    let valid_terminal_fields = match run.status {
        StorageRunStatus::Running | StorageRunStatus::WaitingPermission => {
            run.message_id.is_none()
                && run.error_code.is_none()
                && run.error_message.is_none()
                && run.finished_at_ms.is_none()
        }
        StorageRunStatus::Completed => {
            run.message_id.is_some()
                && run.error_code.is_none()
                && run.error_message.is_none()
                && run.finished_at_ms.is_some()
        }
        StorageRunStatus::Failed => {
            run.message_id.is_none() && run.error_code.is_some() && run.finished_at_ms.is_some()
        }
        StorageRunStatus::Cancelled => {
            run.message_id.is_none()
                && run.error_code.is_none()
                && run.error_message.is_none()
                && run.finished_at_ms.is_some()
        }
    };
    if !valid_terminal_fields {
        return Err(AppError::internal());
    }
    Ok(AgentRunSnapshot {
        run_id: run.id,
        session_id: run.session_id,
        status,
        last_sequence: run.last_sequence.to_string(),
        started_at_ms: run
            .started_at_ms
            .ok_or_else(AppError::internal)?
            .to_string(),
        updated_at_ms: run.updated_at_ms.to_string(),
        model_rounds: run.model_rounds.to_string(),
        tool_calls: run.tool_calls.to_string(),
        usage: AgentUsage {
            input_tokens: run.input_tokens.to_string(),
            output_tokens: run.output_tokens.to_string(),
            total_tokens: run.total_tokens.to_string(),
        },
        pending_permission,
        message_id: run.message_id,
        error,
    })
}

pub(super) fn permission_request(
    permission: &ToolPermissionRecord,
) -> Result<AgentPermissionRequest, AppError> {
    if !matches!(
        permission.status,
        ToolPermissionStatus::Pending | ToolPermissionStatus::Approved
    ) {
        return Err(AppError::internal());
    }
    Ok(AgentPermissionRequest {
        permission_id: permission.id.clone(),
        run_id: permission.run_id.clone(),
        tool_call_id: permission.tool_call_id.clone(),
        tool_name: permission.tool_name.clone(),
        arguments_sha256: hex_digest(permission.arguments_sha256),
        summary: permission.summary.clone(),
        requested_at_ms: permission.created_at_ms.to_string(),
        expires_at_ms: permission.expires_at_ms.to_string(),
    })
}

pub(super) fn hex_digest(bytes: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn parse_hex_digest(value: &str) -> Result<[u8; 32], AppError> {
    if value.len() != 64
        || !value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(AppError::invalid(
            "invalid_permission_digest",
            "argumentsSha256 must be a lowercase hexadecimal SHA-256 digest",
        ));
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        digest[index] = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
    }
    Ok(digest)
}

const fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => 0,
    }
}

fn normalize_terminal_error(error: ApiError) -> ApiError {
    let code = if error.code.trim().is_empty() {
        "agent_failed".to_owned()
    } else {
        truncate_utf8(error.code, MAX_TERMINAL_ERROR_CODE_BYTES)
    };
    ApiError::new(
        code,
        truncate_utf8(error.message, MAX_TERMINAL_ERROR_MESSAGE_BYTES),
    )
}

fn truncate_utf8(mut value: String, limit: usize) -> String {
    if value.len() <= limit {
        return value;
    }
    let mut boundary = limit;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value
}

fn contract_usage(usage: Usage) -> AgentUsage {
    AgentUsage {
        input_tokens: usage.input_tokens.to_string(),
        output_tokens: usage.output_tokens.to_string(),
        total_tokens: usage.total_tokens.to_string(),
    }
}

fn provider_protocol_error(message: &'static str) -> AppError {
    AppError::new(
        crate::AppErrorKind::Unavailable,
        ApiError::new("provider_protocol_error", message),
    )
}

fn durable_count_from_usize(value: usize) -> Result<u64, AppError> {
    usize_to_u64(value).and_then(|value| {
        (value <= MAX_DURABLE_COUNTER)
            .then_some(value)
            .ok_or_else(|| provider_protocol_error("The AI provider exceeded durable counters"))
    })
}

fn usize_to_u64(value: usize) -> Result<u64, AppError> {
    u64::try_from(value).map_err(|_| AppError::internal())
}

const fn is_storage_terminal(status: StorageRunStatus) -> bool {
    matches!(
        status,
        StorageRunStatus::Completed | StorageRunStatus::Failed | StorageRunStatus::Cancelled
    )
}

const fn is_contract_terminal(status: ContractRunStatus) -> bool {
    matches!(
        status,
        ContractRunStatus::Completed | ContractRunStatus::Failed | ContractRunStatus::Cancelled
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use async_trait::async_trait;
    use chat2db_agent::{
        CompactionStrategy, ContextBudget, ContextUsage, ProviderError, ProviderEvent,
        ProviderEventStream, ProviderKind as AgentProviderKind, ProviderRequest, StopReason,
    };
    use chat2db_contract::{
        AgentEvent, AgentEventEnvelope, AgentMessageContent, AgentPermissionDecision,
        AgentPermissionStatus, AgentRunStatus, DecideAgentPermissionRequest, SqlPermissionMode,
        StartAgentRunRequest,
    };
    use chat2db_storage::{
        AgentMessageRole, AppendAgentMessage, CreateAgentSession, CreateProviderProfile,
        ProviderKind, RequestToolPermission, SecretRef, SecretValue, SecretVault, SecretVaultError,
        Storage, ToolPermissionRecord, ToolPermissionStatus,
    };
    use futures_util::{poll, stream};
    use tempfile::TempDir;

    use super::*;

    #[derive(Default)]
    struct MemoryVault {
        values: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SecretVault for MemoryVault {
        fn probe(&self) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn create(
            &self,
            reference: &SecretRef,
            value: &SecretValue,
        ) -> Result<(), SecretVaultError> {
            self.values.lock().expect("vault lock").insert(
                reference.as_str().to_owned(),
                value.expose_secret().to_vec(),
            );
            Ok(())
        }

        fn get(&self, reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
            Ok(self
                .values
                .lock()
                .expect("vault lock")
                .get(reference.as_str())
                .cloned()
                .map(SecretValue::new))
        }

        fn delete(&self, reference: &SecretRef) -> Result<(), SecretVaultError> {
            self.values
                .lock()
                .expect("vault lock")
                .remove(reference.as_str());
            Ok(())
        }
    }

    enum ProviderResponse {
        Events(Mutex<Option<Vec<Result<ProviderEvent, ProviderError>>>>),
        Pending,
    }

    struct ScriptedProvider {
        response: ProviderResponse,
        budget: ContextBudget,
        requests: Mutex<Vec<Vec<Message>>>,
    }

    impl ScriptedProvider {
        fn events(events: Vec<ProviderEvent>) -> Arc<Self> {
            Self::events_with_budget(
                events,
                ContextBudget::new(None, 1024 * 1024, 80).expect("budget is valid"),
            )
        }

        fn events_with_budget(events: Vec<ProviderEvent>, budget: ContextBudget) -> Arc<Self> {
            Arc::new(Self {
                response: ProviderResponse::Events(Mutex::new(Some(
                    events.into_iter().map(Ok).collect(),
                ))),
                budget,
                requests: Mutex::new(Vec::new()),
            })
        }

        fn pending() -> Arc<Self> {
            Arc::new(Self {
                response: ProviderResponse::Pending,
                budget: ContextBudget::new(None, 1024 * 1024, 80).expect("budget is valid"),
                requests: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl Provider for ScriptedProvider {
        fn kind(&self) -> AgentProviderKind {
            AgentProviderKind::OpenAi
        }

        fn model(&self) -> &'static str {
            "scripted"
        }

        fn context_budget(&self) -> ContextBudget {
            self.budget
        }

        fn estimate_context(
            &self,
            request: &ProviderRequest,
        ) -> Result<ContextUsage, ProviderError> {
            let serialized_bytes = request
                .messages()
                .iter()
                .flat_map(Message::blocks)
                .map(|block| match block {
                    MessageBlock::Text(text) => text.len(),
                    MessageBlock::ToolCall(call) => call.id().len(),
                    MessageBlock::ToolResult(result) => result.call_id().len(),
                    MessageBlock::ProviderContinuation(continuation) => continuation.value().len(),
                })
                .sum();
            Ok(ContextUsage {
                estimated_tokens: serialized_bytes / 4,
                serialized_bytes,
            })
        }

        async fn stream(
            &self,
            request: ProviderRequest,
            _cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            self.requests
                .lock()
                .expect("request lock")
                .push(request.messages().to_vec());
            let events: ProviderEventStream = match &self.response {
                ProviderResponse::Events(events) => Box::pin(stream::iter(
                    events
                        .lock()
                        .expect("event lock")
                        .take()
                        .expect("provider is called once"),
                )),
                ProviderResponse::Pending => Box::pin(stream::pending()),
            };
            Ok(events)
        }
    }

    struct Fixture {
        _directory: TempDir,
        storage: Storage,
        application: Application,
        session_id: String,
    }

    fn setup(system_prompt: Option<&str>) -> Fixture {
        let directory = TempDir::new().expect("temp dir");
        let storage = Storage::open(directory.path(), Arc::new(MemoryVault::default()))
            .expect("storage opens");
        let provider = storage
            .create_provider_profile(
                CreateProviderProfile {
                    name: "primary".to_owned(),
                    kind: ProviderKind::OpenAiCompatible,
                    base_url: "https://provider.example/v1".to_owned(),
                    model: "model-1".to_owned(),
                    context_window_tokens: 128_000,
                    max_output_tokens: 8_192,
                },
                Some(SecretValue::new(b"provider-key".to_vec())),
            )
            .expect("provider creates");
        let session = storage
            .create_agent_session(CreateAgentSession {
                title: "Session".to_owned(),
                provider_id: provider.id,
                datasource_id: None,
                system_prompt: system_prompt.map(str::to_owned),
            })
            .expect("session creates");
        Fixture {
            _directory: directory,
            storage: storage.clone(),
            application: Application::with_storage(storage),
            session_id: session.id,
        }
    }

    fn append_text(storage: &Storage, session_id: &str, role: AgentMessageRole, text: &str) {
        storage
            .append_agent_message(
                session_id,
                AppendAgentMessage {
                    role,
                    summary_through_ordinal: None,
                    content_json: serde_json::to_string(&vec![AgentMessageContent::Text {
                        text: text.to_owned(),
                    }])
                    .expect("message serializes"),
                },
            )
            .expect("message appends");
    }

    async fn start_with_provider(
        application: &Application,
        session_id: &str,
        provider: Arc<dyn Provider>,
    ) -> AgentRunAccepted {
        start_with_provider_and_mode(
            application,
            session_id,
            provider,
            SqlPermissionMode::ReadOnly,
        )
        .await
    }

    async fn start_with_provider_and_mode(
        application: &Application,
        session_id: &str,
        provider: Arc<dyn Provider>,
        sql_permission_mode: SqlPermissionMode,
    ) -> AgentRunAccepted {
        application
            .start_agent_run_with_resolver(
                StartAgentRunRequest {
                    session_id: session_id.to_owned(),
                    message: "current question".to_owned(),
                    sql_permission_mode,
                },
                move |_application, _provider_id| async move { Ok(provider) },
            )
            .await
            .expect("run starts")
    }

    async fn install_pending_permission(
        fixture: &Fixture,
        run_id: &str,
        digest: [u8; 32],
        retention: Duration,
    ) -> ToolPermissionRecord {
        let storage = fixture.storage.clone();
        let transition_run_id = run_id.to_owned();
        fixture
            .application
            .inner
            .agent_runs
            .transition(run_id, move |sequence| async move {
                let commit_storage = storage.clone();
                let commit_run_id = transition_run_id.clone();
                let permission = blocking_transition(move || {
                    commit_storage.create_tool_permission(
                        &commit_run_id,
                        RequestToolPermission {
                            tool_call_id: "write-1".to_owned(),
                            tool_name: crate::agent::sql_tools::SQL_WRITE_TOOL.to_owned(),
                            arguments_sha256: digest,
                            summary: "Execute SQL write: UPDATE example SET value = 1".to_owned(),
                            last_sequence: sequence,
                            model_rounds: 0,
                            tool_calls: 1,
                            input_tokens: 0,
                            output_tokens: 0,
                            total_tokens: 0,
                            compaction_count: 0,
                            compacted_through_ordinal: None,
                            retention,
                        },
                    )
                })
                .await?;
                let load_storage = storage;
                let load_run_id = transition_run_id;
                let run = tokio::task::spawn_blocking(move || {
                    load_storage
                        .get_agent_run(&load_run_id)?
                        .ok_or(StorageError::AgentRunNotFound(load_run_id))
                })
                .await
                .map_err(|_| AgentTransitionFailure::indeterminate(AppError::internal()))?
                .map_err(|error| AgentTransitionFailure::indeterminate(AppError::from(error)))?;
                let request = permission_request(&permission)
                    .map_err(AgentTransitionFailure::indeterminate)?;
                let snapshot = snapshot_from_run(run, Some(&permission))
                    .map_err(AgentTransitionFailure::indeterminate)?;
                Ok(DurableAgentTransition::new(
                    snapshot,
                    AgentEvent::PermissionRequested {
                        permission: request,
                    },
                ))
            })
            .await
            .expect("permission request persists");
        fixture
            .storage
            .get_active_tool_permission_for_run(run_id)
            .expect("permission reads")
            .expect("permission is active")
    }

    fn permission_decision(
        run_id: &str,
        decision: AgentPermissionDecision,
        digest: [u8; 32],
    ) -> DecideAgentPermissionRequest {
        DecideAgentPermissionRequest {
            run_id: run_id.to_owned(),
            tool_call_id: "write-1".to_owned(),
            decision,
            arguments_sha256: hex_digest(digest),
        }
    }

    async fn expire_pending_permission(
        fixture: &Fixture,
        permission: &ToolPermissionRecord,
    ) -> Result<(), AppError> {
        let storage = fixture.storage.clone();
        let run_id = permission.run_id.clone();
        let transition_run_id = run_id.clone();
        let permission_id = permission.id.clone();
        let tool_call_id = permission.tool_call_id.clone();
        let digest = permission.arguments_sha256;
        let revision = permission.revision;
        fixture
            .application
            .inner
            .agent_runs
            .transition(&run_id, move |sequence| async move {
                let commit_storage = storage.clone();
                let commit_run_id = transition_run_id.clone();
                let commit_permission_id = permission_id.clone();
                blocking_transition(move || {
                    commit_storage.expire_tool_permission(
                        &commit_permission_id,
                        revision,
                        &commit_run_id,
                        &tool_call_id,
                        digest,
                        sequence,
                    )
                })
                .await?;
                let load_storage = storage;
                let load_run_id = transition_run_id;
                let run = tokio::task::spawn_blocking(move || {
                    load_storage
                        .get_agent_run(&load_run_id)?
                        .ok_or(StorageError::AgentRunNotFound(load_run_id))
                })
                .await
                .map_err(|_| AgentTransitionFailure::indeterminate(AppError::internal()))?
                .map_err(|error| AgentTransitionFailure::indeterminate(AppError::from(error)))?;
                let snapshot =
                    snapshot_from_run(run, None).map_err(AgentTransitionFailure::indeterminate)?;
                Ok(DurableAgentTransition::new(
                    snapshot,
                    AgentEvent::PermissionResolved {
                        permission_id,
                        status: AgentPermissionStatus::Expired,
                    },
                ))
            })
            .await
            .map(|_| ())
    }

    async fn wait_for_terminal_snapshot(
        application: &Application,
        run_id: &str,
    ) -> AgentRunSnapshot {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let snapshot = application
                    .agent_run_snapshot(run_id)
                    .await
                    .expect("snapshot reads");
                if is_contract_terminal(snapshot.status) {
                    break snapshot;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("run reaches a terminal state")
    }

    async fn collect_events(mut subscription: AgentRunSubscription) -> Vec<AgentEventEnvelope> {
        let mut events = Vec::new();
        loop {
            let event = tokio::time::timeout(Duration::from_secs(2), subscription.next_event())
                .await
                .expect("event arrives before timeout")
                .expect("subscription remains available");
            let Some(event) = event else { break };
            events.push(event);
        }
        events
    }

    #[tokio::test]
    async fn permission_approval_commits_before_waking_the_exact_waiter() {
        let fixture = setup(None);
        let accepted = start_with_provider_and_mode(
            &fixture.application,
            &fixture.session_id,
            ScriptedProvider::pending(),
            SqlPermissionMode::AskBeforeWrite,
        )
        .await;
        let digest = [7_u8; 32];
        let permission =
            install_pending_permission(&fixture, &accepted.run_id, digest, Duration::from_secs(60))
                .await;
        let waiter = fixture
            .application
            .inner
            .agent_runs
            .install_permission_waiter(&accepted.run_id, &permission.id)
            .await
            .expect("waiter installs");

        let response = fixture
            .application
            .decide_agent_permission(
                &permission.id,
                permission_decision(&accepted.run_id, AgentPermissionDecision::AllowOnce, digest),
            )
            .await
            .expect("permission approves");
        assert_eq!(response.status, AgentPermissionStatus::Approved);
        assert_eq!(
            waiter.wait().await,
            super::super::hub::AgentPermissionWaitOutcome::Resolved(
                AgentPermissionStatus::Approved
            )
        );
        let cached = fixture
            .application
            .inner
            .agent_runs
            .cached_snapshot(&accepted.run_id)
            .await
            .expect("live snapshot remains available");
        assert_eq!(cached.status, AgentRunStatus::Running);
        assert!(cached.pending_permission.is_none());
        let durable = fixture
            .storage
            .get_tool_permission(&permission.id)
            .expect("permission reads")
            .expect("permission exists");
        assert_eq!(durable.status, ToolPermissionStatus::Approved);

        fixture
            .application
            .cancel_agent_run(&accepted.run_id)
            .await
            .expect("run cancels");
    }

    #[tokio::test]
    async fn concurrent_permission_decisions_have_one_durable_winner() {
        let fixture = setup(None);
        let accepted = start_with_provider_and_mode(
            &fixture.application,
            &fixture.session_id,
            ScriptedProvider::pending(),
            SqlPermissionMode::AskBeforeWrite,
        )
        .await;
        let digest = [8_u8; 32];
        let permission =
            install_pending_permission(&fixture, &accepted.run_id, digest, Duration::from_secs(60))
                .await;
        let allow_request =
            permission_decision(&accepted.run_id, AgentPermissionDecision::AllowOnce, digest);
        let deny_request =
            permission_decision(&accepted.run_id, AgentPermissionDecision::Deny, digest);
        let (allowed, denied) = tokio::join!(
            fixture
                .application
                .decide_agent_permission(&permission.id, allow_request),
            fixture
                .application
                .decide_agent_permission(&permission.id, deny_request),
        );
        assert_ne!(allowed.is_ok(), denied.is_ok());
        let winner = allowed
            .as_ref()
            .or(denied.as_ref())
            .expect("one decision succeeds")
            .status;
        let durable = fixture
            .storage
            .get_tool_permission(&permission.id)
            .expect("permission reads")
            .expect("permission exists");
        assert_eq!(permission_status(durable.status), winner);
        fixture
            .application
            .inner
            .agent_runs
            .cached_snapshot(&accepted.run_id)
            .await
            .expect("losing CAS does not invalidate the live run");

        fixture
            .application
            .cancel_agent_run(&accepted.run_id)
            .await
            .expect("run cancels");
    }

    #[tokio::test]
    async fn cancellation_racing_approval_revokes_the_write_permission() {
        let fixture = setup(None);
        let accepted = start_with_provider_and_mode(
            &fixture.application,
            &fixture.session_id,
            ScriptedProvider::pending(),
            SqlPermissionMode::AskBeforeWrite,
        )
        .await;
        let digest = [9_u8; 32];
        let permission =
            install_pending_permission(&fixture, &accepted.run_id, digest, Duration::from_secs(60))
                .await;
        let (decision, cancellation) = tokio::join!(
            fixture.application.decide_agent_permission(
                &permission.id,
                permission_decision(&accepted.run_id, AgentPermissionDecision::AllowOnce, digest,),
            ),
            fixture.application.cancel_agent_run(&accepted.run_id),
        );
        cancellation.expect("cancellation is durable");
        if let Ok(response) = decision {
            assert_eq!(response.status, AgentPermissionStatus::Approved);
        }
        let terminal = wait_for_terminal_snapshot(&fixture.application, &accepted.run_id).await;
        assert_eq!(terminal.status, AgentRunStatus::Cancelled);
        let durable = fixture
            .storage
            .get_tool_permission(&permission.id)
            .expect("permission reads")
            .expect("permission exists");
        assert_eq!(durable.status, ToolPermissionStatus::Revoked);
    }

    #[tokio::test]
    async fn permission_expiry_racing_a_decision_has_one_resolution_event() {
        let fixture = setup(None);
        let accepted = start_with_provider_and_mode(
            &fixture.application,
            &fixture.session_id,
            ScriptedProvider::pending(),
            SqlPermissionMode::AskBeforeWrite,
        )
        .await;
        let digest = [10_u8; 32];
        let permission = install_pending_permission(
            &fixture,
            &accepted.run_id,
            digest,
            Duration::from_millis(1),
        )
        .await;
        let waiter = fixture
            .application
            .inner
            .agent_runs
            .install_permission_waiter(&accepted.run_id, &permission.id)
            .await
            .expect("waiter installs");
        tokio::time::sleep(Duration::from_millis(5)).await;
        let (decision, expiry) = tokio::join!(
            fixture.application.decide_agent_permission(
                &permission.id,
                permission_decision(&accepted.run_id, AgentPermissionDecision::AllowOnce, digest,),
            ),
            expire_pending_permission(&fixture, &permission),
        );
        assert_ne!(decision.is_ok(), expiry.is_ok());
        if let Ok(response) = decision {
            assert_eq!(response.status, AgentPermissionStatus::Expired);
        }
        assert_eq!(
            waiter.wait().await,
            super::super::hub::AgentPermissionWaitOutcome::Resolved(AgentPermissionStatus::Expired)
        );
        assert_eq!(
            fixture
                .storage
                .get_tool_permission(&permission.id)
                .expect("permission reads")
                .expect("permission exists")
                .status,
            ToolPermissionStatus::Expired
        );

        fixture
            .application
            .cancel_agent_run(&accepted.run_id)
            .await
            .expect("run cancels");
    }

    #[test]
    fn permission_digest_parser_requires_canonical_lowercase_sha256() {
        assert_eq!(
            parse_hex_digest(&"ab".repeat(32)).expect("digest parses"),
            [0xab; 32]
        );
        for invalid in ["ab", &"AB".repeat(32), &"gg".repeat(32)] {
            assert_eq!(
                parse_hex_digest(invalid)
                    .expect_err("digest is rejected")
                    .api_error()
                    .code,
                "invalid_permission_digest"
            );
        }
    }

    #[tokio::test]
    async fn successful_run_persists_each_public_event_before_publication() {
        let fixture = setup(Some("Keep answers concise"));
        let provider = ScriptedProvider::events(vec![
            ProviderEvent::TextDelta("hello".to_owned()),
            ProviderEvent::Usage(Usage {
                input_tokens: 3,
                output_tokens: 2,
                total_tokens: 5,
            }),
            ProviderEvent::Completed(StopReason::Stop),
        ]);
        let accepted = start_with_provider(
            &fixture.application,
            &fixture.session_id,
            provider as Arc<dyn Provider>,
        )
        .await;
        let mut subscription = fixture
            .application
            .subscribe_agent_run(&accepted.run_id, None)
            .await
            .expect("subscription opens");
        let mut events = Vec::new();
        loop {
            let event = tokio::time::timeout(Duration::from_secs(2), subscription.next_event())
                .await
                .expect("event arrives")
                .expect("stream remains available");
            let Some(event) = event else { break };
            let durable = fixture
                .storage
                .get_agent_run(&accepted.run_id)
                .expect("run reloads")
                .expect("run exists");
            assert!(
                durable.last_sequence
                    >= event.sequence.parse::<u64>().expect("sequence is numeric")
            );
            events.push(event);
        }

        assert_eq!(
            events
                .iter()
                .map(|event| event.sequence.as_str())
                .collect::<Vec<_>>(),
            ["1", "2", "3", "4"]
        );
        assert!(matches!(events[0].event, AgentEvent::Started));
        assert!(matches!(
            &events[1].event,
            AgentEvent::TextDelta { delta } if delta == "hello"
        ));
        assert!(matches!(
            &events[2].event,
            AgentEvent::Usage { usage } if usage.total_tokens == "5"
        ));
        assert!(matches!(events[3].event, AgentEvent::Completed { .. }));

        let snapshot = fixture
            .application
            .agent_run_snapshot(&accepted.run_id)
            .await
            .expect("snapshot loads");
        assert_eq!(snapshot.status, AgentRunStatus::Completed);
        assert_eq!(snapshot.model_rounds, "1");
        assert_eq!(snapshot.usage.total_tokens, "5");
        let messages = fixture
            .storage
            .list_agent_messages(&fixture.session_id, 0, 10)
            .expect("messages list");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[2].role, AgentMessageRole::Assistant);
        let content: Vec<AgentMessageContent> =
            serde_json::from_str(&messages[2].content_json).expect("content decodes");
        assert!(matches!(
            content.as_slice(),
            [AgentMessageContent::Text { text }] if text == "hello"
        ));
    }

    #[tokio::test]
    async fn provider_resolution_failure_becomes_a_replayable_failed_run() {
        let fixture = setup(None);
        let accepted = fixture
            .application
            .start_agent_run_with_resolver(
                StartAgentRunRequest {
                    session_id: fixture.session_id.clone(),
                    message: "question".to_owned(),
                    sql_permission_mode: SqlPermissionMode::ReadOnly,
                },
                |_application, _provider_id| async {
                    Err::<Arc<dyn Provider>, AppError>(AppError::invalid(
                        "provider_credentials_missing",
                        "The provider profile does not have an API key",
                    ))
                },
            )
            .await
            .expect("run is accepted before provider resolution");
        let events = collect_events(
            fixture
                .application
                .subscribe_agent_run(&accepted.run_id, None)
                .await
                .expect("subscription opens"),
        )
        .await;

        assert_eq!(events.len(), 2);
        assert!(matches!(events[0].event, AgentEvent::Started));
        assert!(matches!(
            &events[1].event,
            AgentEvent::Failed { error } if error.code == "provider_credentials_missing"
        ));
        let snapshot = fixture
            .application
            .agent_run_snapshot(&accepted.run_id)
            .await
            .expect("failed snapshot loads");
        assert_eq!(snapshot.status, AgentRunStatus::Failed);
        assert_eq!(snapshot.last_sequence, "2");
    }

    #[tokio::test]
    async fn owned_start_coordinator_survives_caller_future_drop() {
        let fixture = setup(None);
        let mut start = Box::pin(fixture.application.start_agent_run_with_resolver(
            StartAgentRunRequest {
                session_id: fixture.session_id.clone(),
                message: "caller dropped".to_owned(),
                sql_permission_mode: SqlPermissionMode::ReadOnly,
            },
            |_application, _provider_id| async {
                Err::<Arc<dyn Provider>, AppError>(provider_protocol_error("stop"))
            },
        ));
        assert!(poll!(start.as_mut()).is_pending());
        drop(start);

        let durable = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let run_id = fixture
                    .storage
                    .list_agent_messages(&fixture.session_id, 0, 32)
                    .expect("messages list")
                    .into_iter()
                    .find(|message| message.content_json.contains("caller dropped"))
                    .and_then(|message| message.run_id);
                if let Some(run) = run_id
                    .as_deref()
                    .and_then(|run_id| fixture.storage.get_agent_run(run_id).ok().flatten())
                    && is_storage_terminal(run.status)
                {
                    break run;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("owned start coordinator reaches a terminal state");
        assert_eq!(durable.status, StorageRunStatus::Failed);
    }

    #[tokio::test]
    async fn oversized_assistant_message_fails_without_leaving_the_session_busy() {
        let fixture = setup(None);
        let oversized = "x".repeat(
            usize::try_from(MAX_AGENT_MESSAGE_BYTES).expect("message limit fits usize") + 1,
        );
        let accepted = start_with_provider(
            &fixture.application,
            &fixture.session_id,
            ScriptedProvider::events(vec![
                ProviderEvent::TextDelta(oversized),
                ProviderEvent::Completed(StopReason::Stop),
            ]) as Arc<dyn Provider>,
        )
        .await;
        let events = collect_events(
            fixture
                .application
                .subscribe_agent_run(&accepted.run_id, None)
                .await
                .expect("subscription opens"),
        )
        .await;
        assert!(matches!(
            events.last().map(|event| &event.event),
            Some(AgentEvent::Failed { error }) if error.code == "provider_protocol_error"
        ));
        let durable = fixture
            .storage
            .get_agent_run(&accepted.run_id)
            .expect("run reads")
            .expect("run exists");
        assert_eq!(durable.status, StorageRunStatus::Failed);

        let next = fixture
            .application
            .start_agent_run_with_resolver(
                StartAgentRunRequest {
                    session_id: fixture.session_id.clone(),
                    message: "next question".to_owned(),
                    sql_permission_mode: SqlPermissionMode::ReadOnly,
                },
                |_application, _provider_id| async {
                    Err::<Arc<dyn Provider>, AppError>(provider_protocol_error("stop"))
                },
            )
            .await
            .expect("terminal failure releases the session");
        let _ = collect_events(
            fixture
                .application
                .subscribe_agent_run(&next.run_id, None)
                .await
                .expect("second subscription opens"),
        )
        .await;
    }

    #[tokio::test]
    async fn session_message_quota_falls_back_to_an_empty_failed_terminal() {
        let fixture = setup(None);
        let historical = "h".repeat(261_900);
        for _ in 0..128 {
            append_text(
                &fixture.storage,
                &fixture.session_id,
                AgentMessageRole::User,
                &historical,
            );
        }
        let accepted = start_with_provider(
            &fixture.application,
            &fixture.session_id,
            ScriptedProvider::events_with_budget(
                vec![
                    ProviderEvent::TextDelta("a".repeat(32_000)),
                    ProviderEvent::Completed(StopReason::Stop),
                ],
                ContextBudget::new(None, 64 * 1024 * 1024, 80).expect("budget is valid"),
            ) as Arc<dyn Provider>,
        )
        .await;
        let events = collect_events(
            fixture
                .application
                .subscribe_agent_run(&accepted.run_id, None)
                .await
                .expect("subscription opens"),
        )
        .await;
        assert!(
            matches!(
                events.last().map(|event| &event.event),
                Some(AgentEvent::Failed { error }) if error.code == "agent_quota_exceeded"
            ),
            "unexpected events: {events:?}"
        );
        let durable = fixture
            .storage
            .get_agent_run(&accepted.run_id)
            .expect("run reads")
            .expect("run exists");
        assert_eq!(durable.status, StorageRunStatus::Failed);

        let next = fixture.storage.start_agent_run(
            &fixture.session_id,
            StartAgentRun {
                user_message: "next".to_owned(),
                sql_permission_mode: chat2db_storage::SqlPermissionMode::ReadOnly,
            },
        );
        assert!(
            next.is_ok() || matches!(next, Err(StorageError::AgentQuotaExceeded { .. })),
            "a terminal run must not leave the session busy"
        );
    }

    #[tokio::test]
    async fn invalid_provider_usage_becomes_a_replayable_failed_run() {
        let fixture = setup(None);
        let accepted = start_with_provider(
            &fixture.application,
            &fixture.session_id,
            ScriptedProvider::events(vec![
                ProviderEvent::Usage(Usage {
                    input_tokens: u64::MAX,
                    output_tokens: 0,
                    total_tokens: u64::MAX,
                }),
                ProviderEvent::Completed(StopReason::Stop),
            ]) as Arc<dyn Provider>,
        )
        .await;
        let events = collect_events(
            fixture
                .application
                .subscribe_agent_run(&accepted.run_id, None)
                .await
                .expect("subscription opens"),
        )
        .await;
        assert!(matches!(
            events.last().map(|event| &event.event),
            Some(AgentEvent::Failed { error }) if error.code == "provider_protocol_error"
        ));
        let durable = fixture
            .storage
            .get_agent_run(&accepted.run_id)
            .expect("run reads")
            .expect("run exists");
        assert_eq!(durable.status, StorageRunStatus::Failed);
        assert_eq!(durable.input_tokens, 0);
        assert_eq!(durable.output_tokens, 0);
        assert_eq!(durable.total_tokens, 0);
    }

    #[tokio::test]
    async fn failed_terminal_race_with_durable_cancellation_finishes_cancelled() {
        let fixture = setup(None);
        let accepted = start_with_provider(
            &fixture.application,
            &fixture.session_id,
            ScriptedProvider::pending() as Arc<dyn Provider>,
        )
        .await;
        let subscription = fixture
            .application
            .subscribe_agent_run(&accepted.run_id, None)
            .await
            .expect("subscription opens");
        let run = fixture
            .storage
            .request_agent_run_cancellation(&accepted.run_id)
            .expect("cancellation commits without signalling the hub");
        let state = AgentWorkerState::from_run(&run);
        fixture
            .application
            .finish_failed_worker(
                fixture.storage.clone(),
                &accepted.run_id,
                &state,
                provider_protocol_error("provider failed"),
                true,
            )
            .await
            .expect("failed transition reconciles the cancellation race");

        let events = collect_events(subscription).await;
        assert!(matches!(
            events.last().map(|event| &event.event),
            Some(AgentEvent::Cancelled { .. })
        ));
        let durable = fixture
            .storage
            .get_agent_run(&accepted.run_id)
            .expect("run reads")
            .expect("run exists");
        assert_eq!(durable.status, StorageRunStatus::Cancelled);
        fixture.application.begin_shutdown().await;
        fixture.application.join_tasks().await;
    }

    #[tokio::test]
    async fn cancellation_is_durable_before_the_worker_publishes_cancelled() {
        let fixture = setup(None);
        let accepted = start_with_provider(
            &fixture.application,
            &fixture.session_id,
            ScriptedProvider::pending() as Arc<dyn Provider>,
        )
        .await;
        let subscription = fixture
            .application
            .subscribe_agent_run(&accepted.run_id, None)
            .await
            .expect("subscription opens");
        let response = fixture
            .application
            .cancel_agent_run(&accepted.run_id)
            .await
            .expect("cancellation persists");
        assert_eq!(response.disposition, CancelDisposition::Accepted);
        let events = collect_events(subscription).await;
        assert!(matches!(
            events.last().map(|event| &event.event),
            Some(AgentEvent::Cancelled { .. })
        ));
        let durable = fixture
            .storage
            .get_agent_run(&accepted.run_id)
            .expect("run reloads")
            .expect("run exists");
        assert_eq!(durable.status, StorageRunStatus::Cancelled);
        assert!(durable.cancel_requested);
    }

    #[tokio::test]
    async fn owned_cancel_coordinator_survives_caller_future_drop() {
        let fixture = setup(None);
        let accepted = start_with_provider(
            &fixture.application,
            &fixture.session_id,
            ScriptedProvider::pending() as Arc<dyn Provider>,
        )
        .await;
        let subscription = fixture
            .application
            .subscribe_agent_run(&accepted.run_id, None)
            .await
            .expect("subscription opens");
        let mut cancel = Box::pin(fixture.application.cancel_agent_run(&accepted.run_id));
        assert!(poll!(cancel.as_mut()).is_pending());
        drop(cancel);

        let events = collect_events(subscription).await;
        assert!(matches!(
            events.last().map(|event| &event.event),
            Some(AgentEvent::Cancelled { .. })
        ));
        let durable = fixture
            .storage
            .get_agent_run(&accepted.run_id)
            .expect("run reloads")
            .expect("run exists");
        assert_eq!(durable.status, StorageRunStatus::Cancelled);
        assert!(durable.cancel_requested);
    }

    #[tokio::test]
    async fn shutdown_persists_cancellation_and_joins_the_agent_worker() {
        let fixture = setup(None);
        let accepted = start_with_provider(
            &fixture.application,
            &fixture.session_id,
            ScriptedProvider::pending() as Arc<dyn Provider>,
        )
        .await;
        let subscription = fixture
            .application
            .subscribe_agent_run(&accepted.run_id, None)
            .await
            .expect("subscription opens");

        fixture.application.begin_shutdown().await;
        fixture.application.join_tasks().await;

        let events = collect_events(subscription).await;
        assert!(matches!(
            events.last().map(|event| &event.event),
            Some(AgentEvent::Cancelled { .. })
        ));
        let durable = fixture
            .storage
            .get_agent_run(&accepted.run_id)
            .expect("run reloads")
            .expect("run exists");
        assert_eq!(durable.status, StorageRunStatus::Cancelled);
        assert!(durable.finished_at_ms.is_some());
    }

    #[tokio::test]
    async fn shutdown_cancels_a_pending_provider_resolution_before_joining() {
        let fixture = setup(None);
        let accepted = fixture
            .application
            .start_agent_run_with_resolver(
                StartAgentRunRequest {
                    session_id: fixture.session_id.clone(),
                    message: "question".to_owned(),
                    sql_permission_mode: SqlPermissionMode::ReadOnly,
                },
                |_application, _provider_id| {
                    std::future::pending::<Result<Arc<dyn Provider>, AppError>>()
                },
            )
            .await
            .expect("run starts");
        let subscription = fixture
            .application
            .subscribe_agent_run(&accepted.run_id, None)
            .await
            .expect("subscription opens");

        fixture.application.begin_shutdown().await;
        tokio::time::timeout(Duration::from_secs(2), fixture.application.join_tasks())
            .await
            .expect("shutdown joins without the hard timeout");

        let events = collect_events(subscription).await;
        assert!(matches!(
            events.last().map(|event| &event.event),
            Some(AgentEvent::Cancelled { .. })
        ));
        let durable = fixture
            .storage
            .get_agent_run(&accepted.run_id)
            .expect("run reloads")
            .expect("run exists");
        assert_eq!(durable.status, StorageRunStatus::Cancelled);
    }

    #[tokio::test]
    async fn shutdown_reconciles_a_run_removed_from_the_hub_before_join() {
        let fixture = setup(None);
        let accepted = start_with_provider(
            &fixture.application,
            &fixture.session_id,
            ScriptedProvider::pending() as Arc<dyn Provider>,
        )
        .await;

        fixture.application.begin_shutdown().await;
        fixture
            .application
            .inner
            .agent_runs
            .abandon(&accepted.run_id);
        fixture.application.join_tasks().await;

        let durable = fixture
            .storage
            .get_agent_run(&accepted.run_id)
            .expect("run reloads")
            .expect("run exists");
        assert_eq!(durable.status, StorageRunStatus::Cancelled);
        assert!(durable.finished_at_ms.is_some());
    }

    #[tokio::test]
    async fn deterministic_compaction_uses_real_ordinals_and_is_replayable() {
        let fixture = setup(None);
        append_text(
            &fixture.storage,
            &fixture.session_id,
            AgentMessageRole::User,
            &"u".repeat(60),
        );
        append_text(
            &fixture.storage,
            &fixture.session_id,
            AgentMessageRole::Assistant,
            &"a".repeat(20),
        );
        let provider = ScriptedProvider::events_with_budget(
            vec![
                ProviderEvent::TextDelta("done".to_owned()),
                ProviderEvent::Completed(StopReason::Stop),
            ],
            ContextBudget::new(None, 100, 80).expect("budget is valid"),
        );
        let provider_for_assertion = Arc::clone(&provider);
        let accepted = fixture
            .application
            .start_agent_run_with_resolver(
                StartAgentRunRequest {
                    session_id: fixture.session_id.clone(),
                    message: "now".to_owned(),
                    sql_permission_mode: SqlPermissionMode::ReadOnly,
                },
                move |_application, _provider_id| {
                    let provider: Arc<dyn Provider> = provider;
                    async move { Ok(provider) }
                },
            )
            .await
            .expect("run starts");
        let events = collect_events(
            fixture
                .application
                .subscribe_agent_run(&accepted.run_id, None)
                .await
                .expect("subscription opens"),
        )
        .await;

        assert_eq!(
            events
                .iter()
                .map(|event| event.sequence.as_str())
                .collect::<Vec<_>>(),
            ["1", "2", "3", "4"]
        );
        assert!(matches!(
            &events[1].event,
            AgentEvent::ContextCompacted { dropped_turns, .. } if dropped_turns == "1"
        ));
        let durable = fixture
            .storage
            .get_agent_run(&accepted.run_id)
            .expect("run reloads")
            .expect("run exists");
        assert_eq!(durable.compaction_count, 1);
        assert_eq!(durable.compacted_through_ordinal, Some(1));
        assert_eq!(
            fixture
                .storage
                .get_agent_session_compaction_coverage(&fixture.session_id)
                .expect("coverage loads"),
            Some(1)
        );
        let requests = provider_for_assertion
            .requests
            .lock()
            .expect("request lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].len(), 1);
        assert!(matches!(
            requests[0][0].blocks(),
            [MessageBlock::Text(text)] if text == "now"
        ));
    }

    #[test]
    fn transcript_restoration_keeps_system_and_latest_effective_summary() {
        let records = vec![
            message_record(0, AgentMessageRole::System, None, "system"),
            message_record(1, AgentMessageRole::User, None, "old question"),
            message_record(4, AgentMessageRole::Assistant, None, "old answer"),
            message_record(8, AgentMessageRole::Summary, Some(4), "condensed"),
            message_record(9, AgentMessageRole::Summary, Some(99), "not effective"),
            message_record(10, AgentMessageRole::User, None, "latest question"),
        ];

        let (messages, mut ordinals) =
            prepare_transcript(&records, Some(4)).expect("transcript prepares");

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role(), Role::System);
        assert_eq!(messages[1].role(), Role::System);
        assert_eq!(messages[2].role(), Role::User);
        assert!(matches!(
            messages[1].blocks(),
            [MessageBlock::Text(text)] if text == "Conversation summary:\ncondensed"
        ));
        assert!(matches!(
            messages[2].blocks(),
            [MessageBlock::Text(text)] if text == "latest question"
        ));
        assert_eq!(
            ordinals
                .apply_compaction(Some(2..3), CompactionStrategy::DeterministicTrim)
                .expect("ordinal range maps"),
            Some(10)
        );
    }

    #[test]
    fn transition_classification_invalidates_unknown_outcomes_only() {
        assert!(matches!(
            classify_storage_transition(StorageError::OutcomeUnknown {
                operation: "test",
                id: "run-1".to_owned(),
            }),
            AgentTransitionFailure::Indeterminate(_)
        ));
        assert!(matches!(
            classify_storage_transition(StorageError::InvalidAgent("invalid")),
            AgentTransitionFailure::DefinitelyNotCommitted(_)
        ));
    }

    #[test]
    fn durable_usage_validation_rejects_field_sum_and_cross_round_overflow() {
        for usage in [
            Usage {
                input_tokens: u64::MAX,
                output_tokens: 0,
                total_tokens: u64::MAX,
            },
            Usage {
                input_tokens: MAX_DURABLE_COUNTER,
                output_tokens: 1,
                total_tokens: MAX_DURABLE_COUNTER,
            },
        ] {
            assert_eq!(
                normalize_durable_usage(usage)
                    .expect_err("invalid usage is rejected")
                    .api_error()
                    .code,
                "provider_protocol_error"
            );
        }
        assert_eq!(
            add_usage(
                Usage {
                    input_tokens: MAX_DURABLE_COUNTER,
                    output_tokens: 0,
                    total_tokens: MAX_DURABLE_COUNTER,
                },
                Usage {
                    input_tokens: 1,
                    output_tokens: 0,
                    total_tokens: 1,
                },
            )
            .expect_err("cross-round usage cannot overflow")
            .api_error()
            .code,
            "provider_protocol_error"
        );
    }

    fn message_record(
        ordinal: u64,
        role: AgentMessageRole,
        summary_through_ordinal: Option<u64>,
        text: &str,
    ) -> AgentMessageRecord {
        let content_json = serde_json::to_string(&vec![AgentMessageContent::Text {
            text: text.to_owned(),
        }])
        .expect("message serializes");
        AgentMessageRecord {
            id: format!("message-{ordinal}"),
            session_id: "session-1".to_owned(),
            run_id: None,
            ordinal,
            role,
            summary_through_ordinal,
            content_bytes: u64::try_from(content_json.len()).expect("content length fits"),
            content_json,
            created_at_ms: i64::try_from(ordinal).expect("ordinal fits"),
        }
    }
}
