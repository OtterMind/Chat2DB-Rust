use std::{collections::BTreeMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use chat2db_agent::{
    ExecutionOutcome, ToolDefinition, ToolExecutionError, ToolExecutor, ToolInvocation, ToolOutput,
    ToolOutputHandle, Usage,
};
use chat2db_contract::{
    AgentEvent, AgentPermissionStatus, AgentResultHandle as ContractResultHandle, ApiError,
    OperationEvent, QueryLimits, ResultColumn, ResultPage, ResultPageRequest, ResultRow,
};
use chat2db_storage::{
    MAX_RESULT_PAGE_BYTES, RequestToolPermission, SqlPermissionMode, Storage, ToolPermissionRecord,
    ToolPermissionStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::{
    execution::{blocking_transition, snapshot_from_run},
    hub::{
        AgentPermissionWaitOutcome, AgentRunHub, AgentTransitionFailure, DurableAgentTransition,
    },
};
use crate::{
    Application,
    query::{DatabaseWriteError, DatabaseWriteOutcome},
    storage_call,
};

pub(super) const SQL_QUERY_TOOL: &str = "query_database";
pub(super) const SQL_WRITE_TOOL: &str = "execute_database_write";
pub(super) const INSPECT_RESULT_TOOL: &str = "inspect_query_result";

const AGENT_QUERY_MAX_ROWS: u64 = 10_000;
const AGENT_QUERY_MAX_BYTES: u64 = 16 * 1024 * 1024;
const AGENT_QUERY_BATCH_ROWS: u32 = 256;
const AGENT_QUERY_BATCH_BYTES: u32 = 1024 * 1024;
const AGENT_RESULT_TTL: Duration = Duration::from_secs(15 * 60);
const TOOL_PERMISSION_TTL: Duration = Duration::from_secs(5 * 60);
const MODEL_PREVIEW_BYTES: usize = 48 * 1024;
const RESULT_SAMPLE_ROWS: u32 = 20;
const MAX_INSPECTION_ROWS: u32 = 100;
const RESULT_HANDLE_MEDIA_TYPE: &str = "application/vnd.chat2db.result+json";

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ToolProgress {
    pub(super) model_rounds: u64,
    pub(super) tool_calls: u64,
    pub(super) usage: Usage,
    pub(super) compaction_count: u64,
    pub(super) compacted_through_ordinal: Option<u64>,
}

pub(super) type SharedToolProgress = Arc<Mutex<ToolProgress>>;

#[derive(Debug, Clone)]
pub(super) struct UnknownWrite {
    pub(super) tool_call_id: String,
    pub(super) arguments_sha256: [u8; 32],
}

pub(super) struct SqlToolExecutor {
    application: Application,
    storage: Storage,
    hub: AgentRunHub,
    run_id: String,
    session_id: String,
    datasource_id: String,
    permission_mode: SqlPermissionMode,
    progress: SharedToolProgress,
    unknown_write: Mutex<Option<UnknownWrite>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SqlArguments {
    sql: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InspectArguments {
    handle_id: String,
    #[serde(default)]
    offset: Option<String>,
    #[serde(default)]
    max_rows: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResultInspection {
    handle_id: String,
    offset: String,
    next_offset: String,
    columns: Vec<ResultColumn>,
    columns_truncated: bool,
    rows: Vec<ResultRow>,
    rows_truncated: bool,
    has_more: bool,
}

impl SqlToolExecutor {
    pub(super) fn new(
        application: Application,
        storage: Storage,
        run_id: String,
        session_id: String,
        datasource_id: String,
        permission_mode: SqlPermissionMode,
        progress: SharedToolProgress,
    ) -> Self {
        let hub = application.inner.agent_runs.clone();
        Self {
            application,
            storage,
            hub,
            run_id,
            session_id,
            datasource_id,
            permission_mode,
            progress,
            unknown_write: Mutex::new(None),
        }
    }

    pub(super) async fn take_unknown_write(&self) -> Option<UnknownWrite> {
        self.unknown_write.lock().await.take()
    }

    async fn execute_query(
        &self,
        invocation: &ToolInvocation,
        cancellation: CancellationToken,
    ) -> Result<ToolOutput, ToolExecutionError> {
        let arguments: SqlArguments = parse_arguments(invocation)?;
        let accepted = self
            .application
            .start_agent_read_query(
                self.datasource_id.clone(),
                arguments.sql,
                QueryLimits {
                    max_rows: AGENT_QUERY_MAX_ROWS.to_string(),
                    max_result_bytes: AGENT_QUERY_MAX_BYTES.to_string(),
                    batch_rows: AGENT_QUERY_BATCH_ROWS,
                    batch_bytes: AGENT_QUERY_BATCH_BYTES,
                    result_ttl_seconds: u32::try_from(AGENT_RESULT_TTL.as_secs())
                        .expect("agent result TTL fits u32"),
                },
            )
            .await
            .map_err(|error| tool_error(error.api_error(), ExecutionOutcome::NotStarted))?;
        let operation_id = accepted.operation_id;
        let mut subscription = self
            .application
            .subscribe_operation(&operation_id, None)
            .await
            .map_err(|error| tool_error(error.api_error(), ExecutionOutcome::Failed))?;
        let mut cancellation_requested = false;

        loop {
            let next = tokio::select! {
                biased;
                () = cancellation.cancelled(), if !cancellation_requested => {
                    cancellation_requested = true;
                    let _ = self.application.cancel_operation(&operation_id).await;
                    continue;
                }
                next = subscription.next_event() => next,
            };
            match next {
                Ok(Some(envelope)) => match envelope.event {
                    OperationEvent::Completed { result } => {
                        return self.result_output(result).await;
                    }
                    OperationEvent::Failed { error } => {
                        return Err(tool_error(error, ExecutionOutcome::Failed));
                    }
                    OperationEvent::Cancelled { .. } => {
                        return Err(ToolExecutionError::new(
                            "database_query_cancelled",
                            "The database query was cancelled",
                            ExecutionOutcome::Failed,
                        ));
                    }
                    OperationEvent::Started | OperationEvent::Progress { .. } => {}
                },
                Ok(None) => {
                    return Err(ToolExecutionError::new(
                        "database_query_incomplete",
                        "The database query ended without a terminal result",
                        ExecutionOutcome::Failed,
                    ));
                }
                Err(error) => {
                    return Err(tool_error(error.api_error(), ExecutionOutcome::Failed));
                }
            }
        }
    }

    async fn result_output(
        &self,
        metadata: chat2db_contract::ResultMetadata,
    ) -> Result<ToolOutput, ToolExecutionError> {
        let storage = self.storage.clone();
        let session_id = self.session_id.clone();
        let run_id = self.run_id.clone();
        let result_id = metadata.id.clone();
        let handle = storage_call(move || {
            storage.create_agent_result_handle(&session_id, &run_id, &result_id, AGENT_RESULT_TTL)
        })
        .await
        .map_err(|error| tool_error(error.api_error(), ExecutionOutcome::Failed))?;
        let page = self
            .application
            .result_page(
                &metadata.id,
                ResultPageRequest {
                    offset: "0".to_owned(),
                    max_rows: RESULT_SAMPLE_ROWS.to_string(),
                    max_bytes: MAX_RESULT_PAGE_BYTES.to_string(),
                },
            )
            .await
            .map_err(|error| tool_error(error.api_error(), ExecutionOutcome::Failed))?;
        let handle = bounded_result_handle(
            handle.id,
            metadata,
            page,
            handle.created_at_ms,
            handle.expires_at_ms,
        );
        let content = serde_json::to_string(&handle).map_err(|_| internal_tool_error())?;
        let mut handle_metadata = BTreeMap::new();
        handle_metadata.insert("rowCount".to_owned(), handle.row_count.clone());
        handle_metadata.insert("byteCount".to_owned(), handle.byte_count.clone());
        handle_metadata.insert("expiresAtMs".to_owned(), handle.expires_at_ms.clone());
        let output_handle = ToolOutputHandle::new(
            handle.handle_id.clone(),
            Some(RESULT_HANDLE_MEDIA_TYPE.to_owned()),
            handle_metadata,
        )
        .map_err(|_| internal_tool_error())?;
        ToolOutput::content_and_handle(content, output_handle).map_err(|_| internal_tool_error())
    }

    async fn inspect_result(
        &self,
        invocation: &ToolInvocation,
    ) -> Result<ToolOutput, ToolExecutionError> {
        let arguments: InspectArguments = parse_arguments(invocation)?;
        if arguments.handle_id.trim().is_empty() {
            return Err(invalid_arguments("handleId cannot be empty"));
        }
        let offset = arguments.offset.unwrap_or_else(|| "0".to_owned());
        let _: u64 = offset
            .parse()
            .map_err(|_| invalid_arguments("offset must be an unsigned decimal integer"))?;
        let max_rows = arguments.max_rows.unwrap_or(RESULT_SAMPLE_ROWS);
        if max_rows == 0 || max_rows > MAX_INSPECTION_ROWS {
            return Err(invalid_arguments("maxRows must be between 1 and 100"));
        }
        let storage = self.storage.clone();
        let handle_id = arguments.handle_id.clone();
        let session_id = self.session_id.clone();
        let run_id = self.run_id.clone();
        let handle = storage_call(move || {
            storage.resolve_agent_result_handle(&handle_id, &session_id, &run_id)
        })
        .await
        .map_err(|error| tool_error(error.api_error(), ExecutionOutcome::Failed))?;
        let page = self
            .application
            .result_page(
                &handle.result_id,
                ResultPageRequest {
                    offset,
                    max_rows: max_rows.to_string(),
                    max_bytes: MAX_RESULT_PAGE_BYTES.to_string(),
                },
            )
            .await
            .map_err(|error| tool_error(error.api_error(), ExecutionOutcome::Failed))?;
        let inspection = bounded_inspection(arguments.handle_id, page);
        let content = serde_json::to_string(&inspection).map_err(|_| internal_tool_error())?;
        ToolOutput::content(content).map_err(|_| internal_tool_error())
    }

    async fn execute_write(
        &self,
        invocation: &ToolInvocation,
        cancellation: CancellationToken,
    ) -> Result<ToolOutput, ToolExecutionError> {
        if self.permission_mode != SqlPermissionMode::AskBeforeWrite {
            return Err(ToolExecutionError::new(
                "database_write_not_allowed",
                "This agent run does not allow database writes",
                ExecutionOutcome::NotStarted,
            ));
        }
        let arguments: SqlArguments = parse_arguments(invocation)?;
        if arguments.sql.trim().is_empty() {
            return Err(invalid_arguments("sql cannot be empty"));
        }
        let digest = invocation.arguments_sha256();
        let permission = self
            .request_write_permission(invocation, &arguments.sql, digest)
            .await?;
        let status = self
            .wait_for_permission(&permission, cancellation.clone())
            .await?;
        if status != AgentPermissionStatus::Approved {
            return Err(permission_error(status));
        }
        let permission = self.load_permission(&permission.id).await?;
        self.consume_permission(&permission, digest).await?;

        match self
            .application
            .execute_agent_update(self.datasource_id.clone(), arguments.sql, cancellation)
            .await
        {
            Ok(affected_rows) => {
                self.settle_write(invocation.call_id(), digest).await?;
                ToolOutput::content(
                    json!({ "affectedRows": affected_rows.to_string() }).to_string(),
                )
                .map_err(|_| internal_tool_error())
            }
            Err(error) if error.outcome == DatabaseWriteOutcome::Unknown => {
                *self.unknown_write.lock().await = Some(UnknownWrite {
                    tool_call_id: invocation.call_id().to_owned(),
                    arguments_sha256: digest,
                });
                Err(database_write_error(&error))
            }
            Err(error) => {
                self.settle_write(invocation.call_id(), digest).await?;
                Err(database_write_error(&error))
            }
        }
    }

    async fn request_write_permission(
        &self,
        invocation: &ToolInvocation,
        sql: &str,
        digest: [u8; 32],
    ) -> Result<ToolPermissionRecord, ToolExecutionError> {
        let progress = *self.progress.lock().await;
        let storage = self.storage.clone();
        let run_id = self.run_id.clone();
        let tool_call_id = invocation.call_id().to_owned();
        let tool_name = invocation.name().to_owned();
        let summary = write_summary(sql);
        let transition = self
            .hub
            .transition(&self.run_id, move |sequence| async move {
                let commit_storage = storage.clone();
                let commit_run_id = run_id.clone();
                let permission = blocking_transition(move || {
                    commit_storage.create_tool_permission(
                        &commit_run_id,
                        RequestToolPermission {
                            tool_call_id,
                            tool_name,
                            arguments_sha256: digest,
                            summary,
                            last_sequence: sequence,
                            model_rounds: progress.model_rounds,
                            tool_calls: progress.tool_calls,
                            input_tokens: progress.usage.input_tokens,
                            output_tokens: progress.usage.output_tokens,
                            total_tokens: progress.usage.total_tokens,
                            compaction_count: progress.compaction_count,
                            compacted_through_ordinal: progress.compacted_through_ordinal,
                            retention: TOOL_PERMISSION_TTL,
                        },
                    )
                })
                .await?;
                let run = permission_run(&permission, &run_id, &storage).await?;
                let snapshot = snapshot_from_run(run, Some(&permission))
                    .map_err(AgentTransitionFailure::indeterminate)?;
                let request = super::execution::permission_request(&permission)
                    .map_err(AgentTransitionFailure::indeterminate)?;
                Ok(DurableAgentTransition::new(
                    snapshot,
                    AgentEvent::PermissionRequested {
                        permission: request,
                    },
                ))
            })
            .await
            .map_err(|error| tool_error(error.api_error(), ExecutionOutcome::NotStarted))?;
        let permission_id = transition
            .pending_permission
            .as_ref()
            .ok_or_else(internal_tool_error)?
            .permission_id
            .clone();
        self.load_permission(&permission_id).await
    }

    async fn wait_for_permission(
        &self,
        permission: &ToolPermissionRecord,
        cancellation: CancellationToken,
    ) -> Result<AgentPermissionStatus, ToolExecutionError> {
        let waiter = self
            .hub
            .install_permission_waiter(&self.run_id, &permission.id)
            .await
            .map_err(|error| tool_error(error.api_error(), ExecutionOutcome::NotStarted))?;
        let expiry = permission_expiry_delay(permission.expires_at_ms);
        tokio::select! {
            biased;
            () = cancellation.cancelled() => {
                self.revoke_after_local_cancellation(permission).await?;
                Err(ToolExecutionError::new(
                    "agent_tool_cancelled",
                    "The database write was cancelled before dispatch",
                    ExecutionOutcome::NotStarted,
                ))
            }
            outcome = waiter.wait() => match outcome {
                AgentPermissionWaitOutcome::Resolved(status) => Ok(status),
                AgentPermissionWaitOutcome::Cancelled => Err(ToolExecutionError::new(
                    "agent_tool_cancelled",
                    "The database write was cancelled before dispatch",
                    ExecutionOutcome::NotStarted,
                )),
            },
            () = tokio::time::sleep(expiry) => {
                self.expire_permission(permission).await?;
                Ok(AgentPermissionStatus::Expired)
            }
        }
    }

    async fn consume_permission(
        &self,
        permission: &ToolPermissionRecord,
        digest: [u8; 32],
    ) -> Result<(), ToolExecutionError> {
        let storage = self.storage.clone();
        let run_id = self.run_id.clone();
        let permission_id = permission.id.clone();
        let tool_call_id = permission.tool_call_id.clone();
        let tool_name = permission.tool_name.clone();
        let revision = permission.revision;
        self.hub
            .transition(&self.run_id, move |sequence| async move {
                let commit_storage = storage.clone();
                let commit_run_id = run_id.clone();
                let commit_tool_call_id = tool_call_id.clone();
                let consumed = blocking_transition(move || {
                    commit_storage.consume_tool_permission(
                        &permission_id,
                        revision,
                        &commit_run_id,
                        &commit_tool_call_id,
                        digest,
                        sequence,
                    )
                })
                .await?;
                let run = permission_run(&consumed, &run_id, &storage).await?;
                let snapshot =
                    snapshot_from_run(run, None).map_err(AgentTransitionFailure::indeterminate)?;
                Ok(DurableAgentTransition::new(
                    snapshot,
                    AgentEvent::ToolStarted {
                        tool_call_id,
                        name: tool_name,
                        arguments_sha256: super::execution::hex_digest(digest),
                    },
                ))
            })
            .await
            .map(|_| ())
            .map_err(|error| tool_error(error.api_error(), ExecutionOutcome::NotStarted))
    }

    async fn settle_write(
        &self,
        tool_call_id: &str,
        digest: [u8; 32],
    ) -> Result<(), ToolExecutionError> {
        let tool_call_id = tool_call_id.to_owned();
        let mut last_error = None;
        for _attempt in 0..2 {
            let storage = self.storage.clone();
            let run_id = self.run_id.clone();
            let call_id = tool_call_id.clone();
            match storage_call(move || storage.settle_agent_write(&run_id, &call_id, digest)).await
            {
                Ok(_) => return Ok(()),
                Err(error) => last_error = Some(error),
            }
            let storage = self.storage.clone();
            let run_id = self.run_id.clone();
            let run = match storage_call(move || storage.get_agent_run(&run_id)).await {
                Ok(Some(run)) => run,
                Ok(None) => {
                    *self.unknown_write.lock().await = Some(UnknownWrite {
                        tool_call_id,
                        arguments_sha256: digest,
                    });
                    return Err(write_settlement_unknown());
                }
                Err(error) => {
                    *self.unknown_write.lock().await = Some(UnknownWrite {
                        tool_call_id,
                        arguments_sha256: digest,
                    });
                    return Err(tool_error(error.api_error(), ExecutionOutcome::Unknown));
                }
            };
            if run.write_in_flight_tool_call_id.is_none()
                && run.write_in_flight_arguments_sha256.is_none()
            {
                return Ok(());
            }
            if run.write_in_flight_tool_call_id.as_deref() != Some(tool_call_id.as_str())
                || run.write_in_flight_arguments_sha256 != Some(digest)
            {
                break;
            }
        }
        *self.unknown_write.lock().await = Some(UnknownWrite {
            tool_call_id,
            arguments_sha256: digest,
        });
        let error = last_error.ok_or_else(internal_tool_error)?;
        Err(tool_error(error.api_error(), ExecutionOutcome::Unknown))
    }

    async fn load_permission(
        &self,
        permission_id: &str,
    ) -> Result<ToolPermissionRecord, ToolExecutionError> {
        let storage = self.storage.clone();
        let permission_id = permission_id.to_owned();
        storage_call(move || storage.get_tool_permission(&permission_id))
            .await
            .map_err(|error| tool_error(error.api_error(), ExecutionOutcome::NotStarted))?
            .ok_or_else(|| {
                ToolExecutionError::new(
                    "tool_permission_not_found",
                    "The database write permission no longer exists",
                    ExecutionOutcome::NotStarted,
                )
            })
    }

    async fn expire_permission(
        &self,
        permission: &ToolPermissionRecord,
    ) -> Result<(), ToolExecutionError> {
        self.abandon_permission(permission, ToolPermissionStatus::Expired)
            .await
    }

    async fn revoke_after_local_cancellation(
        &self,
        permission: &ToolPermissionRecord,
    ) -> Result<(), ToolExecutionError> {
        let storage = self.storage.clone();
        let run_id = self.run_id.clone();
        let cancelled = storage_call(move || storage.get_agent_run(&run_id))
            .await
            .map_err(|error| tool_error(error.api_error(), ExecutionOutcome::NotStarted))?
            .is_some_and(|run| run.cancel_requested);
        if cancelled {
            return Ok(());
        }
        let current = self.load_permission(&permission.id).await?;
        if matches!(
            current.status,
            ToolPermissionStatus::Pending | ToolPermissionStatus::Approved
        ) {
            self.abandon_permission(&current, ToolPermissionStatus::Revoked)
                .await?;
        }
        Ok(())
    }

    async fn abandon_permission(
        &self,
        permission: &ToolPermissionRecord,
        status: ToolPermissionStatus,
    ) -> Result<(), ToolExecutionError> {
        let storage = self.storage.clone();
        let run_id = self.run_id.clone();
        let permission_id = permission.id.clone();
        let tool_call_id = permission.tool_call_id.clone();
        let digest = permission.arguments_sha256;
        let revision = permission.revision;
        let contract_status = permission_status(status);
        self.hub
            .transition(&self.run_id, move |sequence| async move {
                let commit_storage = storage.clone();
                let commit_run_id = run_id.clone();
                let commit_permission_id = permission_id.clone();
                let abandoned = blocking_transition(move || match status {
                    ToolPermissionStatus::Expired => commit_storage.expire_tool_permission(
                        &commit_permission_id,
                        revision,
                        &commit_run_id,
                        &tool_call_id,
                        digest,
                        sequence,
                    ),
                    ToolPermissionStatus::Revoked => commit_storage.revoke_tool_permission(
                        &commit_permission_id,
                        revision,
                        &commit_run_id,
                        &tool_call_id,
                        digest,
                        sequence,
                    ),
                    _ => Err(chat2db_storage::StorageError::InvalidAgent(
                        "permission abandonment status is invalid",
                    )),
                })
                .await?;
                let run = permission_run(&abandoned, &run_id, &storage).await?;
                let snapshot =
                    snapshot_from_run(run, None).map_err(AgentTransitionFailure::indeterminate)?;
                Ok(DurableAgentTransition::new(
                    snapshot,
                    AgentEvent::PermissionResolved {
                        permission_id,
                        status: contract_status,
                    },
                ))
            })
            .await
            .map(|_| ())
            .map_err(|error| tool_error(error.api_error(), ExecutionOutcome::NotStarted))
    }
}

#[async_trait]
impl ToolExecutor for SqlToolExecutor {
    async fn execute(
        &self,
        invocation: ToolInvocation,
        cancellation: CancellationToken,
    ) -> Result<ToolOutput, ToolExecutionError> {
        if cancellation.is_cancelled() {
            return Err(ToolExecutionError::new(
                "agent_tool_cancelled",
                "The tool was cancelled before execution",
                ExecutionOutcome::NotStarted,
            ));
        }
        match invocation.name() {
            SQL_QUERY_TOOL => self.execute_query(&invocation, cancellation).await,
            SQL_WRITE_TOOL => self.execute_write(&invocation, cancellation).await,
            INSPECT_RESULT_TOOL => self.inspect_result(&invocation).await,
            _ => Err(ToolExecutionError::new(
                "tool_unavailable",
                "The requested tool is not available",
                ExecutionOutcome::NotStarted,
            )),
        }
    }
}

pub(super) fn tool_definitions(permission_mode: SqlPermissionMode) -> Vec<ToolDefinition> {
    let mut tools = vec![
        ToolDefinition::new(
            SQL_QUERY_TOOL,
            "Execute one read-only SQL query against the session datasource. Returns a bounded sample and a retained-result handle.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": { "sql": { "type": "string" } },
                "required": ["sql"]
            }),
        )
        .expect("static query tool definition is valid"),
        ToolDefinition::new(
            INSPECT_RESULT_TOOL,
            "Read a bounded page from a retained query result created by this run.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "handleId": { "type": "string" },
                    "offset": { "type": "string", "pattern": "^(0|[1-9][0-9]*)$" },
                    "maxRows": { "type": "integer", "minimum": 1, "maximum": MAX_INSPECTION_ROWS }
                },
                "required": ["handleId"]
            }),
        )
        .expect("static result-inspection definition is valid"),
    ];
    if permission_mode == SqlPermissionMode::AskBeforeWrite {
        tools.push(
            ToolDefinition::new(
                SQL_WRITE_TOOL,
                "Execute exactly one SQL write after explicit user approval. Never retry an unknown outcome.",
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": { "sql": { "type": "string" } },
                    "required": ["sql"]
                }),
            )
            .expect("static write tool definition is valid"),
        );
    }
    tools
}

pub(super) fn is_write_tool(name: &str) -> bool {
    name == SQL_WRITE_TOOL
}

fn parse_arguments<T: for<'de> Deserialize<'de>>(
    invocation: &ToolInvocation,
) -> Result<T, ToolExecutionError> {
    serde_json::from_value::<T>(invocation.arguments().clone())
        .map_err(|_| invalid_arguments("The tool arguments do not match the required schema"))
}

fn invalid_arguments(message: &'static str) -> ToolExecutionError {
    ToolExecutionError::new(
        "invalid_tool_arguments",
        message,
        ExecutionOutcome::NotStarted,
    )
}

fn internal_tool_error() -> ToolExecutionError {
    ToolExecutionError::new(
        "agent_tool_internal_error",
        "The tool could not produce a bounded result",
        ExecutionOutcome::Failed,
    )
}

fn write_settlement_unknown() -> ToolExecutionError {
    ToolExecutionError::new(
        "agent_write_settlement_unknown",
        "The durable write fence could not be reconciled",
        ExecutionOutcome::Unknown,
    )
}

fn tool_error(error: ApiError, outcome: ExecutionOutcome) -> ToolExecutionError {
    ToolExecutionError::new(error.code, error.message, outcome)
}

fn database_write_error(error: &DatabaseWriteError) -> ToolExecutionError {
    let outcome = match error.outcome {
        DatabaseWriteOutcome::NotStarted => ExecutionOutcome::NotStarted,
        DatabaseWriteOutcome::Unknown => ExecutionOutcome::Unknown,
    };
    tool_error(error.error.api_error(), outcome)
}

fn permission_error(status: AgentPermissionStatus) -> ToolExecutionError {
    let (code, message) = match status {
        AgentPermissionStatus::Denied => (
            "database_write_denied",
            "The user denied this database write",
        ),
        AgentPermissionStatus::Expired => (
            "database_write_permission_expired",
            "The database write permission expired",
        ),
        AgentPermissionStatus::Revoked => (
            "database_write_permission_revoked",
            "The database write permission was revoked",
        ),
        _ => (
            "database_write_permission_invalid",
            "The database write permission is not executable",
        ),
    };
    ToolExecutionError::new(code, message, ExecutionOutcome::NotStarted)
}

pub(super) const fn permission_status(status: ToolPermissionStatus) -> AgentPermissionStatus {
    match status {
        ToolPermissionStatus::Pending => AgentPermissionStatus::Pending,
        ToolPermissionStatus::Approved => AgentPermissionStatus::Approved,
        ToolPermissionStatus::Denied => AgentPermissionStatus::Denied,
        ToolPermissionStatus::Consumed => AgentPermissionStatus::Consumed,
        ToolPermissionStatus::Expired => AgentPermissionStatus::Expired,
        ToolPermissionStatus::Revoked => AgentPermissionStatus::Revoked,
    }
}

async fn permission_run(
    _permission: &ToolPermissionRecord,
    run_id: &str,
    storage: &Storage,
) -> Result<chat2db_storage::AgentRunRecord, AgentTransitionFailure> {
    let storage = storage.clone();
    let run_id = run_id.to_owned();
    match tokio::task::spawn_blocking(move || {
        let run = storage.get_agent_run(&run_id)?;
        run.ok_or(chat2db_storage::StorageError::AgentRunNotFound(run_id))
    })
    .await
    {
        Ok(Ok(run)) => Ok(run),
        Ok(Err(error)) => Err(AgentTransitionFailure::indeterminate(error.into())),
        Err(_) => Err(AgentTransitionFailure::indeterminate(
            crate::AppError::internal(),
        )),
    }
}

fn write_summary(sql: &str) -> String {
    const MAX_SUMMARY_BYTES: usize = 512;
    let normalized = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut summary = format!("Execute SQL write: {normalized}");
    while summary.len() > MAX_SUMMARY_BYTES {
        summary.pop();
    }
    summary
}

fn permission_expiry_delay(expires_at_ms: i64) -> Duration {
    let now = crate::now_millis().unwrap_or(i64::MAX);
    let remaining = expires_at_ms.saturating_sub(now);
    Duration::from_millis(u64::try_from(remaining).unwrap_or_default())
}

fn bounded_result_handle(
    handle_id: String,
    metadata: chat2db_contract::ResultMetadata,
    page: ResultPage,
    created_at_ms: i64,
    expires_at_ms: i64,
) -> ContractResultHandle {
    let original_columns = page.columns.len();
    let original_rows = page.rows.len();
    let mut handle = ContractResultHandle {
        handle_id,
        row_count: metadata.row_count,
        byte_count: metadata.byte_count,
        truncated_by_max_rows: metadata.truncated_by_max_rows,
        truncated_by_max_result_bytes: metadata.truncated_by_max_result_bytes,
        created_at_ms: created_at_ms.to_string(),
        expires_at_ms: expires_at_ms.to_string(),
        columns: page.columns,
        columns_truncated: false,
        sample_rows: page.rows,
        sample_truncated: page.has_more,
    };
    while serialized_bytes(&handle) > MODEL_PREVIEW_BYTES && !handle.sample_rows.is_empty() {
        handle.sample_rows.pop();
    }
    while serialized_bytes(&handle) > MODEL_PREVIEW_BYTES && !handle.columns.is_empty() {
        handle.columns.pop();
    }
    handle.columns_truncated = handle.columns.len() < original_columns;
    handle.sample_truncated |= handle.sample_rows.len() < original_rows;
    handle
}

fn bounded_inspection(handle_id: String, page: ResultPage) -> ResultInspection {
    let original_columns = page.columns.len();
    let original_rows = page.rows.len();
    let offset = page.offset.parse::<u64>().unwrap_or_default();
    let mut inspection = ResultInspection {
        handle_id,
        offset: page.offset,
        next_offset: offset
            .saturating_add(u64::try_from(original_rows).unwrap_or(u64::MAX))
            .to_string(),
        columns: page.columns,
        columns_truncated: false,
        rows: page.rows,
        rows_truncated: false,
        has_more: page.has_more,
    };
    while serialized_bytes(&inspection) > MODEL_PREVIEW_BYTES && !inspection.rows.is_empty() {
        inspection.rows.pop();
    }
    while serialized_bytes(&inspection) > MODEL_PREVIEW_BYTES && !inspection.columns.is_empty() {
        inspection.columns.pop();
    }
    inspection.columns_truncated = inspection.columns.len() < original_columns;
    inspection.rows_truncated = inspection.rows.len() < original_rows;
    inspection.has_more |= inspection.rows_truncated;
    inspection.next_offset = offset
        .saturating_add(u64::try_from(inspection.rows.len()).unwrap_or(u64::MAX))
        .to_string();
    inspection
}

fn serialized_bytes<T: Serialize>(value: &T) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use chat2db_agent::{ExecutionOutcome, ToolCall, ToolDefinition, ToolInvocation};
    use chat2db_contract::{
        AgentPermissionStatus, ApiError, ColumnNullability, JdbcValue, JdbcValueType, ResultColumn,
        ResultMetadata, ResultPage, ResultRow,
    };
    use chat2db_engine_protocol::wire;
    use chat2db_storage::{
        CreateAgentSession, CreateProviderProfile, ProviderKind, RequestToolPermission, SecretRef,
        SecretValue, SecretVault, SecretVaultError, SqlPermissionMode, StartAgentRun, Storage,
        ToolPermissionDecision, ToolPermissionStatus,
    };
    use serde_json::json;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    use super::{
        INSPECT_RESULT_TOOL, MAX_INSPECTION_ROWS, MODEL_PREVIEW_BYTES, SQL_QUERY_TOOL,
        SQL_WRITE_TOOL, SqlToolExecutor, ToolProgress, bounded_inspection, bounded_result_handle,
        database_write_error, permission_status, serialized_bytes, tool_definitions, write_summary,
    };
    use crate::{
        AppError, AppErrorKind, Application,
        query::{DatabaseWriteError, DatabaseWriteOutcome},
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

    fn result_metadata() -> ResultMetadata {
        ResultMetadata {
            id: "result-1".to_owned(),
            row_count: "100".to_owned(),
            byte_count: "100000".to_owned(),
            truncated_by_max_rows: false,
            truncated_by_max_result_bytes: false,
            created_at_ms: "1".to_owned(),
            expires_at_ms: "1000".to_owned(),
        }
    }

    fn result_column(ordinal: u32, padding_bytes: usize) -> ResultColumn {
        ResultColumn {
            ordinal,
            label: format!("column-{ordinal}-{}", "x".repeat(padding_bytes)),
            name: format!("column-{ordinal}"),
            jdbc_type: 12,
            jdbc_type_name: "VARCHAR".to_owned(),
            value_type: JdbcValueType::Text,
            nullability: ColumnNullability::Nullable,
            precision: None,
            scale: None,
            display_size: None,
            signed: None,
            catalog_name: None,
            schema_name: None,
            table_name: None,
        }
    }

    fn result_row(value_bytes: usize) -> ResultRow {
        ResultRow {
            values: vec![JdbcValue::Text {
                value: "x".repeat(value_bytes),
            }],
        }
    }

    fn result_page(
        columns: Vec<ResultColumn>,
        rows: Vec<ResultRow>,
        offset: u64,
        has_more: bool,
    ) -> ResultPage {
        ResultPage {
            metadata: result_metadata(),
            columns,
            offset: offset.to_string(),
            rows,
            has_more,
        }
    }

    fn setup_storage(directory: &TempDir) -> (Storage, String) {
        let storage =
            Storage::open(directory.path(), Arc::new(EmptyVault)).expect("test storage must open");
        let provider = storage
            .create_provider_profile(
                CreateProviderProfile {
                    name: "provider".to_owned(),
                    kind: ProviderKind::OpenAiCompatible,
                    base_url: "https://provider.example/v1".to_owned(),
                    model: "model-1".to_owned(),
                    context_window_tokens: 128_000,
                    max_output_tokens: 8_192,
                },
                None,
            )
            .expect("provider must create");
        (storage, provider.id)
    }

    fn create_session_and_run(storage: &Storage, provider_id: &str) -> (String, String) {
        let session = storage
            .create_agent_session(CreateAgentSession {
                title: "Session".to_owned(),
                provider_id: provider_id.to_owned(),
                datasource_id: None,
                system_prompt: None,
            })
            .expect("session must create");
        let run = storage
            .start_agent_run(
                &session.id,
                StartAgentRun {
                    user_message: "inspect the database".to_owned(),
                    sql_permission_mode: SqlPermissionMode::AskBeforeWrite,
                },
            )
            .expect("run must start")
            .run;
        (session.id, run.id)
    }

    fn completed_result(storage: &Storage) -> String {
        let schema = wire::QueryStarted {
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
        };
        storage
            .begin_result(&schema, Duration::from_secs(60))
            .expect("result must begin")
            .finish(&wire::QueryCompleted {
                row_count: 0,
                truncated_by_max_rows: false,
                truncated_by_max_result_bytes: false,
            })
            .expect("result must complete")
            .id
    }

    fn executor(storage: &Storage, session_id: &str, run_id: &str) -> SqlToolExecutor {
        SqlToolExecutor::new(
            Application::with_storage(storage.clone()),
            storage.clone(),
            run_id.to_owned(),
            session_id.to_owned(),
            "datasource-1".to_owned(),
            SqlPermissionMode::AskBeforeWrite,
            Arc::new(Mutex::new(ToolProgress::default())),
        )
    }

    fn install_write_fence(storage: &Storage, run_id: &str, tool_call_id: &str, digest: [u8; 32]) {
        let pending = storage
            .create_tool_permission(
                run_id,
                RequestToolPermission {
                    tool_call_id: tool_call_id.to_owned(),
                    tool_name: SQL_WRITE_TOOL.to_owned(),
                    arguments_sha256: digest,
                    summary: "Execute one SQL write".to_owned(),
                    last_sequence: 2,
                    model_rounds: 0,
                    tool_calls: 1,
                    input_tokens: 0,
                    output_tokens: 0,
                    total_tokens: 0,
                    compaction_count: 0,
                    compacted_through_ordinal: None,
                    retention: Duration::from_secs(60),
                },
            )
            .expect("permission must create");
        let approved = storage
            .decide_tool_permission(
                &pending.id,
                pending.revision,
                run_id,
                tool_call_id,
                digest,
                3,
                ToolPermissionDecision::Approve,
            )
            .expect("permission must approve");
        storage
            .consume_tool_permission(
                &approved.id,
                approved.revision,
                run_id,
                tool_call_id,
                digest,
                4,
            )
            .expect("write fence must install");
    }

    #[test]
    fn write_summaries_are_bounded_and_whitespace_normalized() {
        let summary = write_summary(&format!("UPDATE   t\nSET value = '{}'", "x".repeat(1024)));
        assert!(summary.starts_with("Execute SQL write: UPDATE t SET value"));
        assert!(summary.len() <= 512);
    }

    #[test]
    fn every_permission_status_has_an_external_projection() {
        assert_eq!(
            permission_status(ToolPermissionStatus::Approved),
            AgentPermissionStatus::Approved
        );
        assert_eq!(
            permission_status(ToolPermissionStatus::Revoked),
            AgentPermissionStatus::Revoked
        );
    }

    #[test]
    fn result_preview_is_bounded_and_reports_row_and_column_truncation() {
        let columns = (1..=4)
            .map(|ordinal| result_column(ordinal, 20 * 1024))
            .collect();
        let rows = (0..64).map(|_| result_row(1024)).collect();
        let handle = bounded_result_handle(
            "handle-1".to_owned(),
            result_metadata(),
            result_page(columns, rows, 0, false),
            1,
            1000,
        );

        assert!(serialized_bytes(&handle) <= MODEL_PREVIEW_BYTES);
        assert!(handle.columns_truncated);
        assert!(handle.sample_truncated);
        assert!(handle.columns.len() < 4);
        assert!(handle.sample_rows.len() < 64);
    }

    #[test]
    fn inspection_cursor_advances_only_over_rows_in_the_bounded_page() {
        let inspection = bounded_inspection(
            "handle-1".to_owned(),
            result_page(
                vec![result_column(1, 0)],
                (0..100).map(|_| result_row(1024)).collect(),
                7,
                false,
            ),
        );

        assert!(serialized_bytes(&inspection) <= MODEL_PREVIEW_BYTES);
        assert_eq!(inspection.offset, "7");
        assert_eq!(
            inspection.next_offset,
            (7 + inspection.rows.len()).to_string()
        );
        assert!(inspection.rows_truncated);
        assert!(inspection.has_more);
        assert!(!inspection.columns_truncated);
    }

    #[test]
    fn tool_definitions_expose_writes_only_for_ask_before_write_runs() {
        let read_only = tool_definitions(SqlPermissionMode::ReadOnly);
        assert_eq!(
            read_only
                .iter()
                .map(ToolDefinition::name)
                .collect::<Vec<_>>(),
            vec![SQL_QUERY_TOOL, INSPECT_RESULT_TOOL]
        );
        let inspect = read_only
            .iter()
            .find(|tool| tool.name() == INSPECT_RESULT_TOOL)
            .expect("inspect tool must exist");
        assert_eq!(
            inspect.input_schema()["properties"]["offset"]["pattern"],
            json!("^(0|[1-9][0-9]*)$")
        );
        assert_eq!(
            inspect.input_schema()["properties"]["maxRows"]["maximum"],
            json!(MAX_INSPECTION_ROWS)
        );

        let writable = tool_definitions(SqlPermissionMode::AskBeforeWrite);
        assert_eq!(
            writable
                .iter()
                .map(ToolDefinition::name)
                .collect::<Vec<_>>(),
            vec![SQL_QUERY_TOOL, INSPECT_RESULT_TOOL, SQL_WRITE_TOOL]
        );
        let write = writable
            .iter()
            .find(|tool| tool.name() == SQL_WRITE_TOOL)
            .expect("write tool must exist");
        assert!(write.description().contains("explicit user approval"));
        assert!(
            write
                .description()
                .contains("Never retry an unknown outcome")
        );
    }

    #[tokio::test]
    async fn result_handles_are_bound_to_the_exact_session_and_run() {
        let directory = TempDir::new().expect("temporary directory must create");
        let (storage, provider_id) = setup_storage(&directory);
        let (owner_session_id, owner_run_id) = create_session_and_run(&storage, &provider_id);
        let (other_session_id, other_run_id) = create_session_and_run(&storage, &provider_id);
        let result_id = completed_result(&storage);
        let handle = storage
            .create_agent_result_handle(
                &owner_session_id,
                &owner_run_id,
                &result_id,
                Duration::from_secs(60),
            )
            .expect("result handle must create");
        let call = ToolCall::new(
            "inspect-1",
            INSPECT_RESULT_TOOL,
            json!({ "handleId": handle.id, "offset": "0", "maxRows": 10 }),
        )
        .expect("tool call must create");
        let invocation = ToolInvocation::from(&call);

        executor(&storage, &owner_session_id, &owner_run_id)
            .inspect_result(&invocation)
            .await
            .expect("exact owner must resolve the handle");
        for (session_id, run_id) in [
            (&other_session_id, &owner_run_id),
            (&owner_session_id, &other_run_id),
        ] {
            let error = executor(&storage, session_id, run_id)
                .inspect_result(&invocation)
                .await
                .expect_err("a session or run mismatch must not resolve the handle");
            assert_eq!(error.code(), "agent_result_handle_not_found");
            assert_eq!(error.outcome(), ExecutionOutcome::Failed);
        }
    }

    #[tokio::test]
    async fn inspection_rejects_invalid_offsets_and_row_limits() {
        let directory = TempDir::new().expect("temporary directory must create");
        let (storage, provider_id) = setup_storage(&directory);
        let (session_id, run_id) = create_session_and_run(&storage, &provider_id);
        let executor = executor(&storage, &session_id, &run_id);

        for arguments in [
            json!({ "handleId": "handle-1", "offset": "invalid" }),
            json!({ "handleId": "handle-1", "maxRows": 0 }),
            json!({ "handleId": "handle-1", "maxRows": MAX_INSPECTION_ROWS + 1 }),
        ] {
            let call = ToolCall::new("inspect-1", INSPECT_RESULT_TOOL, arguments)
                .expect("tool call must create");
            let invocation = ToolInvocation::from(&call);
            let error = executor
                .inspect_result(&invocation)
                .await
                .expect_err("invalid pagination must fail before handle resolution");
            assert_eq!(error.code(), "invalid_tool_arguments");
            assert_eq!(error.outcome(), ExecutionOutcome::NotStarted);
        }
    }

    #[tokio::test]
    async fn write_settlement_clears_the_exact_fence_and_is_idempotent() {
        let directory = TempDir::new().expect("temporary directory must create");
        let (storage, provider_id) = setup_storage(&directory);
        let (session_id, run_id) = create_session_and_run(&storage, &provider_id);
        let digest = [7_u8; 32];
        install_write_fence(&storage, &run_id, "write-1", digest);
        let executor = executor(&storage, &session_id, &run_id);

        executor
            .settle_write("write-1", digest)
            .await
            .expect("exact write fence must settle");
        executor
            .settle_write("write-1", digest)
            .await
            .expect("settlement readback must make retry idempotent");
        let run = storage
            .get_agent_run(&run_id)
            .expect("run must read")
            .expect("run must exist");
        assert!(run.write_in_flight_tool_call_id.is_none());
        assert!(run.write_in_flight_arguments_sha256.is_none());
        assert!(executor.take_unknown_write().await.is_none());
    }

    #[tokio::test]
    async fn unreconciled_write_settlement_fails_closed_with_the_exact_identity() {
        let directory = TempDir::new().expect("temporary directory must create");
        let (storage, provider_id) = setup_storage(&directory);
        let (session_id, run_id) = create_session_and_run(&storage, &provider_id);
        install_write_fence(&storage, &run_id, "write-1", [7_u8; 32]);
        let executor = executor(&storage, &session_id, &run_id);

        let error = executor
            .settle_write("write-2", [8_u8; 32])
            .await
            .expect_err("a mismatched fence must not be cleared");
        assert_eq!(error.outcome(), ExecutionOutcome::Unknown);
        let unknown = executor
            .take_unknown_write()
            .await
            .expect("unknown write identity must be retained");
        assert_eq!(unknown.tool_call_id, "write-2");
        assert_eq!(unknown.arguments_sha256, [8_u8; 32]);
        let run = storage
            .get_agent_run(&run_id)
            .expect("run must read")
            .expect("run must exist");
        assert_eq!(run.write_in_flight_tool_call_id.as_deref(), Some("write-1"));
        assert_eq!(run.write_in_flight_arguments_sha256, Some([7_u8; 32]));
    }

    #[test]
    fn database_write_errors_preserve_dispatch_certainty() {
        for (database_outcome, execution_outcome) in [
            (
                DatabaseWriteOutcome::NotStarted,
                ExecutionOutcome::NotStarted,
            ),
            (DatabaseWriteOutcome::Unknown, ExecutionOutcome::Unknown),
        ] {
            let error = DatabaseWriteError {
                error: AppError::new(
                    AppErrorKind::Unavailable,
                    ApiError::new("database_write_failed", "Database write failed"),
                ),
                outcome: database_outcome,
            };
            let mapped = database_write_error(&error);
            assert_eq!(mapped.code(), "database_write_failed");
            assert_eq!(mapped.outcome(), execution_outcome);
        }
    }
}
