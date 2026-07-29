use std::time::Duration;

use chat2db_contract::{
    ApiError, QueryAccepted, QueryLimits, ResultColumn, ResultRow, StartQueryRequest,
};
use chat2db_engine_protocol::wire;
use chat2db_java_bridge::{
    BridgeError, CancelDisposition as BridgeCancelDisposition, ConnectionProperty, JdbcParameter,
    QueryEvent, QueryOptions, QueryRequest, QueryStream, SessionConfig, UpdateRequest,
};
use chat2db_storage::{ResultWriter, Storage, StorageError};
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::{
    AppError, AppErrorKind, Application, convert,
    datasource_session::{
        ResolvedDatasourceConnection, SessionReadOnly, open_datasource_session,
        resolve_datasource_connection,
    },
    engine_manager::EngineLease,
    operation::CancellationRequest,
};

pub(crate) struct PreparedQuery {
    pub(crate) datasource_id: String,
    pub(crate) sql: String,
    pub(crate) parameters: Vec<JdbcParameter>,
    pub(crate) options: QueryOptions,
    pub(crate) retention: Duration,
    pub(crate) force_read_only: bool,
}

/// One native `MySQL` Console execution request.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[allow(clippy::struct_excessive_bools)]
#[serde(rename_all = "camelCase")]
pub struct MysqlConsoleRequest {
    /// Opaque datasource id resolved by Core.
    pub datasource_id: String,
    /// Optional `MySQL` database selected on the Console connection.
    #[serde(default)]
    pub database_name: String,
    /// One statement or a semicolon-delimited `MySQL` script.
    pub sql: String,
    /// One-based result page number.
    pub page_no: u32,
    /// Number of rows retained for each tabular result set.
    pub page_size: u32,
    /// Optional one-based tabular result-set id to retain per statement.
    #[serde(default)]
    pub result_set_id: Option<u32>,
    /// Whether the submitted SQL must be dispatched as one preserved statement.
    #[serde(default)]
    pub single: bool,
    /// Whether the bounded all-rows window is used instead of the requested page.
    #[serde(default)]
    pub page_size_all: bool,
    /// Whether each parsed statement is executed through `EXPLAIN`.
    #[serde(default)]
    pub explain: bool,
    /// Whether execution proceeds to the next statement after a database error.
    #[serde(default = "default_console_error_continue")]
    pub error_continue: bool,
}

/// One statement result emitted by native `MySQL` Console execution.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MysqlConsoleResult {
    /// One-based statement position in the submitted script.
    pub statement_sequence: u32,
    /// One-based tabular result-set position within the statement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_set_id: Option<u32>,
    /// The exact statement sent to `MySQL`.
    pub sql: String,
    /// Whether this individual statement result succeeded.
    pub success: bool,
    /// Safe database or execution message.
    pub message: String,
    /// Server-reported affected rows for non-tabular results.
    pub update_count: u64,
    /// Portable result columns in display order.
    pub columns: Vec<ResultColumn>,
    /// The requested page of portable result rows.
    pub rows: Vec<ResultRow>,
    /// Exact rows observed in the complete tabular result set.
    pub row_count: u64,
    /// Whether rows exist beyond the requested page.
    pub has_more: bool,
    /// Wall-clock execution and fetch time in milliseconds.
    pub duration_ms: u64,
    /// Safe statement failure, present only when `success` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}

/// Cloneable cancellation source for one native `MySQL` Console execution.
#[derive(Debug, Clone)]
pub struct MysqlConsoleCancellation {
    sender: watch::Sender<CancellationRequest>,
}

impl MysqlConsoleCancellation {
    /// Creates an active cancellation source.
    #[must_use]
    pub fn new() -> Self {
        let (sender, _receiver) = watch::channel(CancellationRequest::Waiting);
        Self { sender }
    }

    /// Requests cancellation once and preserves the first supplied reason.
    #[must_use]
    pub fn cancel(&self, reason: Option<String>) -> bool {
        self.sender.send_if_modified(|state| {
            if *state != CancellationRequest::Waiting {
                return false;
            }
            *state = CancellationRequest::Requested { reason };
            true
        })
    }

    /// Reports whether cancellation was already requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        *self.sender.borrow() != CancellationRequest::Waiting
    }

    fn subscribe(&self) -> watch::Receiver<CancellationRequest> {
        self.sender.subscribe()
    }
}

impl Default for MysqlConsoleCancellation {
    fn default() -> Self {
        Self::new()
    }
}

const fn default_console_error_continue() -> bool {
    true
}

enum QueryBackend {
    Java {
        engine: EngineLease,
        resolved: ResolvedDatasourceConnection,
    },
    NativeMysql {
        resolved: ResolvedDatasourceConnection,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DatabaseWriteOutcome {
    NotStarted,
    Failed,
    Unknown,
}

pub(crate) struct DatabaseWriteError {
    pub(crate) error: AppError,
    pub(crate) outcome: DatabaseWriteOutcome,
}

pub(crate) enum QueryTaskError {
    Failed(AppError),
    Cancelled(Option<String>),
}

impl From<AppError> for QueryTaskError {
    fn from(error: AppError) -> Self {
        Self::Failed(error)
    }
}

pub(crate) struct RetainedWriter {
    inner: Option<ResultWriter>,
}

impl Application {
    /// Executes an arbitrary `MySQL` Console script natively on one connection.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, connection, cancellation, or unrecoverable
    /// protocol failures. Recoverable `MySQL` statement errors are represented in
    /// the returned result list so `error_continue` can be honored.
    pub async fn execute_mysql_console(
        &self,
        request: MysqlConsoleRequest,
        cancellation: MysqlConsoleCancellation,
    ) -> Result<Vec<MysqlConsoleResult>, AppError> {
        crate::native_mysql::execute_console(self, request, cancellation.subscribe()).await
    }

    /// Accepts a query for asynchronous execution and returns its operation id.
    ///
    /// # Errors
    ///
    /// Returns a validation or component-availability error before acceptance.
    pub async fn start_query(&self, request: StartQueryRequest) -> Result<QueryAccepted, AppError> {
        let prepared = PreparedQuery::try_from(request)?;
        self.start_prepared_query(prepared).await
    }

    /// Accepts a query for asynchronous execution through a forced read-only
    /// JDBC session, regardless of the datasource's interactive setting.
    ///
    /// # Errors
    ///
    /// Returns a validation or component-availability error before acceptance.
    pub async fn start_read_query(
        &self,
        request: StartQueryRequest,
    ) -> Result<QueryAccepted, AppError> {
        let mut prepared = PreparedQuery::try_from(request)?;
        prepared.force_read_only = true;
        self.start_prepared_query(prepared).await
    }

    pub(crate) async fn start_agent_read_query(
        &self,
        datasource_id: String,
        sql: String,
        limits: QueryLimits,
    ) -> Result<QueryAccepted, AppError> {
        self.start_read_query(StartQueryRequest {
            datasource_id,
            sql,
            parameters: Vec::new(),
            limits,
        })
        .await
    }

    async fn start_prepared_query(
        &self,
        prepared: PreparedQuery,
    ) -> Result<QueryAccepted, AppError> {
        let storage = self.require_storage()?;
        if !self.inner.engine.is_configured() {
            let _engine = self.require_engine().await?;
        }
        let resolved = resolve_datasource_connection(&storage, &prepared.datasource_id).await?;
        let backend = if self.is_native_mysql_driver(&resolved.driver_id)
            && crate::native_mysql::is_native_read_candidate(&prepared.sql)?
        {
            crate::native_mysql::validate_query(&prepared)?;
            QueryBackend::NativeMysql { resolved }
        } else {
            QueryBackend::Java {
                engine: self.require_engine().await?,
                resolved,
            }
        };
        let accepting_work = self.inner.accepting_work.lock().await;
        if !*accepting_work {
            return Err(AppError::unavailable(
                "runtime_shutting_down",
                "The Chat2DB runtime is shutting down",
            ));
        }
        let operation = self.inner.operations.create().await?;
        let operation_id = operation.id.clone();
        let application = self.clone();
        let run_operation_id = operation_id.clone();
        let cleanup_operation_id = operation_id.clone();
        let (registered, wait_for_registration) = oneshot::channel();
        let task = tokio::spawn(async move {
            if wait_for_registration.await.is_err() {
                return;
            }
            application
                .run_query_task(
                    run_operation_id,
                    operation.cancellation,
                    prepared,
                    storage,
                    backend,
                )
                .await;
            application
                .inner
                .tasks
                .lock()
                .await
                .remove(&cleanup_operation_id);
        });
        let replaced = self
            .inner
            .tasks
            .lock()
            .await
            .insert(operation_id.clone(), task);
        debug_assert!(replaced.is_none(), "operation ids must be unique");
        if registered.send(()).is_err() {
            if let Some(task) = self.inner.tasks.lock().await.remove(&operation_id) {
                task.abort();
            }
            self.inner
                .operations
                .failed(&operation_id, AppError::internal().api_error())
                .await?;
            return Err(AppError::internal());
        }
        drop(accepting_work);
        Ok(QueryAccepted { operation_id })
    }

    async fn run_query_task(
        &self,
        operation_id: String,
        cancellation: watch::Receiver<CancellationRequest>,
        query: PreparedQuery,
        storage: Storage,
        backend: QueryBackend,
    ) {
        let outcome = match backend {
            QueryBackend::Java { engine, resolved } => {
                self.execute_query_task(
                    &operation_id,
                    cancellation,
                    query,
                    storage,
                    engine,
                    resolved,
                )
                .await
            }
            QueryBackend::NativeMysql { resolved } => {
                crate::native_mysql::execute_query_task(
                    self,
                    &operation_id,
                    cancellation,
                    query,
                    storage,
                    resolved,
                )
                .await
            }
        };
        match outcome {
            Ok(result) => {
                let _ = self.inner.operations.completed(&operation_id, result).await;
            }
            Err(QueryTaskError::Cancelled(reason)) => {
                let _ = self.inner.operations.cancelled(&operation_id, reason).await;
            }
            Err(QueryTaskError::Failed(error)) => {
                let _ = self
                    .inner
                    .operations
                    .failed(&operation_id, error.api_error())
                    .await;
            }
        }
    }

    async fn execute_query_task(
        &self,
        operation_id: &str,
        mut cancellation: watch::Receiver<CancellationRequest>,
        query: PreparedQuery,
        storage: Storage,
        engine: EngineLease,
        resolved: ResolvedDatasourceConnection,
    ) -> Result<chat2db_contract::ResultMetadata, QueryTaskError> {
        if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
            return Err(QueryTaskError::Cancelled(reason));
        }

        let read_only = if query.force_read_only {
            SessionReadOnly::Forced
        } else {
            SessionReadOnly::Configured
        };
        let session = open_datasource_session(&engine, resolved, read_only).await?;

        let cancellation_request = { cancellation.borrow().clone() };
        if let CancellationRequest::Requested { reason } = cancellation_request {
            return preserve_primary_outcome(
                operation_id,
                "close_session_after_cancellation",
                Err(QueryTaskError::Cancelled(reason)),
                session.close().await.map_err(AppError::from),
            );
        }

        let stream = match session
            .execute_query(QueryRequest {
                sql: query.sql,
                parameters: query.parameters,
                transaction_id: None,
                options: query.options,
            })
            .await
        {
            Ok(stream) => stream,
            Err(error) => {
                return preserve_primary_outcome(
                    operation_id,
                    "close_session_after_execute_failure",
                    Err(QueryTaskError::Failed(error.into())),
                    session.close().await.map_err(AppError::from),
                );
            }
        };

        let result = self
            .consume_stream(
                operation_id,
                &mut cancellation,
                stream,
                storage,
                query.retention,
            )
            .await;
        let close_result = session.close().await.map_err(AppError::from);
        preserve_primary_outcome(
            operation_id,
            "close_session_after_query",
            result,
            close_result,
        )
    }

    async fn consume_stream(
        &self,
        operation_id: &str,
        cancellation: &mut watch::Receiver<CancellationRequest>,
        mut stream: QueryStream,
        storage: Storage,
        retention: Duration,
    ) -> Result<chat2db_contract::ResultMetadata, QueryTaskError> {
        let mut writer: Option<RetainedWriter> = None;
        let mut cancellation_accepted = CancellationRequest::Waiting;

        let result = async {
            loop {
                let event = tokio::select! {
                biased;
                changed = cancellation.changed(), if cancellation_accepted == CancellationRequest::Waiting => {
                    if changed.is_err() {
                        stream.next_event().await
                    } else {
                        let requested = { cancellation.borrow().clone() };
                        if let CancellationRequest::Requested { reason } = requested {
                            match stream.cancel(reason.clone()).await {
                                Ok(BridgeCancelDisposition::Accepted) => {
                                    cancellation_accepted = CancellationRequest::Requested { reason };
                                }
                                Ok(BridgeCancelDisposition::AlreadyTerminal | BridgeCancelDisposition::UnknownRequest) => {}
                                Err(error) => break Err(QueryTaskError::Failed(error.into())),
                            }
                        }
                        continue;
                    }
                }
                event = stream.next_event() => event,
                };

                let event = match event {
                Ok(Some(event)) => event,
                Ok(None) => {
                    break Err(QueryTaskError::Failed(AppError::new(
                        AppErrorKind::Internal,
                        ApiError::new(
                            "database_stream_incomplete",
                            "The database stream ended without a terminal event",
                        ),
                    )));
                }
                Err(error)
                    if cancellation_accepted != CancellationRequest::Waiting
                        && is_cancelled(&error) =>
                {
                    let CancellationRequest::Requested { reason } = cancellation_accepted else {
                        unreachable!("guard proves cancellation was requested");
                    };
                    break Err(QueryTaskError::Cancelled(reason));
                }
                Err(error) => break Err(QueryTaskError::Failed(error.into())),
                };

                match event {
                QueryEvent::Started(started) => {
                    if writer.is_some() {
                        break Err(QueryTaskError::Failed(AppError::internal()));
                    }
                    let schema = wire::QueryStarted::from(started);
                    writer = Some(RetainedWriter::begin(storage.clone(), schema, retention).await?);
                    self.inner.operations.started(operation_id).await?;
                    grant_next_credit(&stream).await?;
                }
                QueryEvent::Batch(batch) => {
                    let Some(active_writer) = writer.as_mut() else {
                        break Err(QueryTaskError::Failed(AppError::internal()));
                    };
                    let row_count = batch
                        .start_row_offset
                        .checked_add(
                            u64::try_from(batch.rows.len())
                                .map_err(|_| QueryTaskError::Failed(AppError::internal()))?,
                        )
                        .ok_or_else(|| QueryTaskError::Failed(AppError::internal()))?;
                    let byte_count = active_writer.append(wire::RowBatch::from(batch)).await?;
                    self.inner
                        .operations
                        .progress(operation_id, row_count, byte_count)
                        .await?;
                    grant_next_credit(&stream).await?;
                }
                QueryEvent::Completed(completed) => {
                    let Some(mut active_writer) = writer.take() else {
                        break Err(QueryTaskError::Failed(AppError::internal()));
                    };
                    break active_writer
                        .finish(wire::QueryCompleted::from(completed))
                        .await
                        .map_err(QueryTaskError::from);
                }
                }
            }
        }
        .await;

        finish_stream_consumption(operation_id, result, &mut stream, writer).await
    }

    pub(crate) async fn execute_agent_update(
        &self,
        datasource_id: String,
        sql: String,
        cancellation: CancellationToken,
    ) -> Result<u64, DatabaseWriteError> {
        if cancellation.is_cancelled() {
            return Err(DatabaseWriteError::not_started(AppError::new(
                AppErrorKind::Conflict,
                ApiError::new(
                    "agent_tool_cancelled",
                    "The database write was cancelled before dispatch",
                ),
            )));
        }
        let storage = self
            .require_storage()
            .map_err(DatabaseWriteError::not_started)?;
        let engine = self
            .require_engine()
            .await
            .map_err(DatabaseWriteError::not_started)?;
        let ResolvedDatasourceConnection {
            driver_id,
            datasource_name: _,
            connection,
        } = resolve_datasource_connection(&storage, &datasource_id)
            .await
            .map_err(DatabaseWriteError::not_started)?;
        if connection.read_only {
            return Err(DatabaseWriteError::not_started(AppError::new(
                AppErrorKind::Conflict,
                ApiError::new(
                    "datasource_read_only",
                    "The datasource connection is configured as read-only",
                ),
            )));
        }
        let driver = engine
            .driver_client()
            .map_err(AppError::from)
            .map_err(DatabaseWriteError::not_started)?;
        let session = driver
            .open_session(SessionConfig {
                driver_id,
                jdbc_url: connection.jdbc_url,
                properties: connection
                    .properties
                    .into_iter()
                    .map(|property| ConnectionProperty {
                        key: property.key,
                        value: property.value,
                        sensitive: property.sensitive,
                    })
                    .collect(),
                read_only: false,
            })
            .await
            .map_err(|error| DatabaseWriteError::from_bridge(error, false))?;
        if cancellation.is_cancelled() {
            let _ = session.close().await;
            return Err(DatabaseWriteError::not_started(AppError::new(
                AppErrorKind::Conflict,
                ApiError::new(
                    "agent_tool_cancelled",
                    "The database write was cancelled before dispatch",
                ),
            )));
        }

        let result = session
            .execute_update(UpdateRequest {
                sql,
                parameters: Vec::new(),
                transaction_id: None,
            })
            .await;
        let close_result = session.close().await;
        if let Err(close_error) = close_result {
            tracing::warn!(
                error = %close_error,
                "database write session cleanup failed after the outcome was determined"
            );
        }
        result
            .map(|completed| completed.affected_rows)
            .map_err(|error| DatabaseWriteError::from_bridge(error, true))
    }
}

impl DatabaseWriteError {
    fn not_started(error: AppError) -> Self {
        Self {
            error,
            outcome: DatabaseWriteOutcome::NotStarted,
        }
    }

    fn from_bridge(error: BridgeError, dispatched: bool) -> Self {
        let outcome = database_write_outcome(&error, dispatched);
        Self {
            error: error.into(),
            outcome,
        }
    }
}

impl TryFrom<StartQueryRequest> for PreparedQuery {
    type Error = AppError;

    fn try_from(request: StartQueryRequest) -> Result<Self, Self::Error> {
        if request.datasource_id.trim().is_empty() {
            return Err(AppError::invalid(
                "invalid_query_request",
                "datasourceId cannot be empty",
            ));
        }
        if request.sql.trim().is_empty() {
            return Err(AppError::invalid(
                "invalid_query_request",
                "SQL cannot be empty",
            ));
        }
        let QueryLimits {
            max_rows,
            max_result_bytes,
            batch_rows,
            batch_bytes,
            result_ttl_seconds,
        } = request.limits;
        if result_ttl_seconds == 0 {
            return Err(AppError::invalid(
                "invalid_query_limits",
                "resultTtlSeconds must be greater than zero",
            ));
        }
        Ok(Self {
            datasource_id: request.datasource_id,
            sql: request.sql,
            parameters: request
                .parameters
                .into_iter()
                .map(convert::query_parameter)
                .collect::<Result<_, _>>()?,
            options: QueryOptions {
                max_rows: parse_u64(&max_rows, "maxRows")?,
                target_batch_rows: batch_rows,
                target_batch_bytes: batch_bytes,
                initial_batch_credits: 0,
                max_result_bytes: parse_u64(&max_result_bytes, "maxResultBytes")?,
            },
            retention: Duration::from_secs(u64::from(result_ttl_seconds)),
            force_read_only: false,
        })
    }
}

impl RetainedWriter {
    pub(crate) async fn begin(
        storage: Storage,
        schema: wire::QueryStarted,
        retention: Duration,
    ) -> Result<Self, AppError> {
        let writer = run_blocking(move || storage.begin_result(&schema, retention)).await?;
        Ok(Self {
            inner: Some(writer),
        })
    }

    pub(crate) async fn append(&mut self, batch: wire::RowBatch) -> Result<u64, AppError> {
        let mut writer = self.inner.take().ok_or_else(AppError::internal)?;
        let (returned, result) = tokio::task::spawn_blocking(move || {
            let result = writer.append_batch(&batch);
            (writer, result)
        })
        .await
        .map_err(|_| AppError::internal())?;
        self.inner = Some(returned);
        result.map_err(AppError::from)?;
        Ok(self
            .inner
            .as_ref()
            .expect("writer restored after append")
            .persisted_bytes())
    }

    pub(crate) async fn finish(
        &mut self,
        completed: wire::QueryCompleted,
    ) -> Result<chat2db_contract::ResultMetadata, AppError> {
        let writer = self.inner.take().ok_or_else(AppError::internal)?;
        let metadata = run_blocking(move || writer.finish(&completed)).await?;
        Ok(convert::result_metadata(metadata))
    }

    pub(crate) async fn abort(&mut self) -> Result<(), AppError> {
        if let Some(writer) = self.inner.take() {
            tokio::task::spawn_blocking(move || writer.abort())
                .await
                .map_err(|_| AppError::internal())?
                .map_err(AppError::from)?;
        }
        Ok(())
    }
}

fn database_write_outcome(error: &BridgeError, dispatched: bool) -> DatabaseWriteOutcome {
    use chat2db_engine_protocol::wire::OperationOutcome;
    use chat2db_java_bridge::DeliveryOutcome;

    match error {
        BridgeError::Remote(remote) => match remote.outcome {
            OperationOutcome::NotApplicable | OperationOutcome::NotStarted => {
                DatabaseWriteOutcome::NotStarted
            }
            OperationOutcome::KnownFailed => DatabaseWriteOutcome::Failed,
            OperationOutcome::Unknown | OperationOutcome::Unspecified => {
                DatabaseWriteOutcome::Unknown
            }
        },
        BridgeError::CommandChannelClosed { outcome }
        | BridgeError::RequestTimeout { outcome, .. }
        | BridgeError::ProcessUnavailable { outcome, .. } => match outcome {
            DeliveryOutcome::NotSent => DatabaseWriteOutcome::NotStarted,
            DeliveryOutcome::Unknown => DatabaseWriteOutcome::Unknown,
        },
        BridgeError::Protocol(_)
        | BridgeError::UnexpectedResponse(_)
        | BridgeError::Frame(_)
        | BridgeError::SupervisorTask(_)
        | BridgeError::ShutdownTimeout
            if dispatched =>
        {
            DatabaseWriteOutcome::Unknown
        }
        _ if dispatched => DatabaseWriteOutcome::Failed,
        _ => DatabaseWriteOutcome::NotStarted,
    }
}

fn parse_u64(value: &str, field: &str) -> Result<u64, AppError> {
    value.parse().map_err(|_| {
        AppError::invalid(
            "invalid_query_limits",
            format!("{field} must be an unsigned decimal integer"),
        )
    })
}

fn is_cancelled(error: &BridgeError) -> bool {
    matches!(
        error,
        BridgeError::Remote(remote)
            if remote.category == wire::ErrorCategory::Cancelled
                || remote.code == "database.operation_cancelled"
    )
}

async fn grant_next_credit(stream: &QueryStream) -> Result<(), AppError> {
    match stream.grant_credits(1).await {
        Ok(accepted) => validate_credit_grant(accepted),
        Err(error) if is_inactive_credit_grant_error(&error) => {
            // The engine can retire an exhausted query immediately after its
            // last batch. The already-queued terminal event remains mandatory.
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn validate_credit_grant(accepted: u32) -> Result<(), AppError> {
    if accepted == 1 {
        return Ok(());
    }
    Err(AppError::new(
        AppErrorKind::ResourceExhausted,
        ApiError::new(
            "database_flow_control_exhausted",
            "The database engine accepted no query batch credit",
        ),
    ))
}

async fn settle_query_stream(stream: &mut QueryStream) -> Result<(), AppError> {
    match stream
        .cancel(Some(
            "Chat2DB stopped the query after local result handling failed".to_owned(),
        ))
        .await
    {
        Ok(
            BridgeCancelDisposition::Accepted
            | BridgeCancelDisposition::AlreadyTerminal
            | BridgeCancelDisposition::UnknownRequest,
        ) => {}
        Err(BridgeError::InvalidRequest(message)) if is_inactive_stream_message(&message) => {}
        Err(error) => return Err(error.into()),
    }

    loop {
        match stream.next_event().await {
            Ok(Some(QueryEvent::Started(_) | QueryEvent::Batch(_))) => {}
            Ok(Some(QueryEvent::Completed(_)) | None) => return Ok(()),
            Err(error) if is_cancelled(&error) => return Ok(()),
            Err(error) => return Err(error.into()),
        }
    }
}

async fn finish_stream_consumption(
    operation_id: &str,
    result: Result<chat2db_contract::ResultMetadata, QueryTaskError>,
    stream: &mut QueryStream,
    writer: Option<RetainedWriter>,
) -> Result<chat2db_contract::ResultMetadata, QueryTaskError> {
    let mut outcome = result;
    if outcome.is_err() {
        outcome = preserve_primary_outcome(
            operation_id,
            "settle_database_query",
            outcome,
            settle_query_stream(stream).await,
        );
    }
    if let Some(mut active_writer) = writer {
        outcome = preserve_primary_outcome(
            operation_id,
            "abort_retained_result",
            outcome,
            active_writer.abort().await,
        );
    }
    outcome
}

fn preserve_primary_outcome<T>(
    operation_id: &str,
    cleanup_phase: &'static str,
    outcome: Result<T, QueryTaskError>,
    cleanup: Result<(), AppError>,
) -> Result<T, QueryTaskError> {
    match (outcome, cleanup) {
        (outcome, Ok(())) => outcome,
        (Ok(_), Err(error)) => Err(QueryTaskError::Failed(error)),
        (Err(primary), Err(cleanup_error)) => {
            tracing::warn!(
                operation_id,
                cleanup_phase,
                cleanup_error = %cleanup_error,
                "query cleanup failed after the primary outcome was determined"
            );
            Err(primary)
        }
    }
}

fn is_inactive_stream_message(message: &str) -> bool {
    message.starts_with("target query stream ") && message.ends_with(" is not active")
}

fn is_inactive_credit_grant_error(error: &BridgeError) -> bool {
    match error {
        BridgeError::InvalidRequest(message) => is_inactive_stream_message(message),
        BridgeError::Remote(remote) => {
            remote.category == wire::ErrorCategory::Validation
                && remote.code == "operation.not_active"
        }
        _ => false,
    }
}

async fn run_blocking<T, F>(operation: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, StorageError> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| AppError::internal())?
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chat2db_contract::ApiError;
    use chat2db_engine_protocol::wire;
    use chat2db_java_bridge::{BridgeError, RemoteEngineError};

    use super::{
        AppError, MysqlConsoleCancellation, MysqlConsoleRequest, QueryTaskError,
        is_inactive_credit_grant_error, preserve_primary_outcome, validate_credit_grant,
    };

    #[test]
    fn mysql_console_cancellation_preserves_the_first_reason() {
        let cancellation = MysqlConsoleCancellation::new();
        let receiver = cancellation.subscribe();

        assert!(cancellation.cancel(Some("first".to_owned())));
        assert!(!cancellation.cancel(Some("second".to_owned())));
        assert!(cancellation.is_cancelled());
        assert!(matches!(
            &*receiver.borrow(),
            super::CancellationRequest::Requested { reason }
                if reason.as_deref() == Some("first")
        ));
    }

    #[test]
    fn mysql_console_json_defaults_to_continuing_after_statement_errors() {
        let request: MysqlConsoleRequest = serde_json::from_value(serde_json::json!({
            "datasourceId": "mysql-1",
            "sql": "SELECT 1",
            "pageNo": 1,
            "pageSize": 200
        }))
        .expect("console request should deserialize");

        assert!(request.error_continue);
        assert_eq!(request.result_set_id, None);
        assert!(request.database_name.is_empty());
    }

    #[test]
    fn query_progress_requires_the_requested_credit_to_be_accepted() {
        validate_credit_grant(1).expect("one accepted credit makes progress");
        let error = validate_credit_grant(0).expect_err("zero credits cannot make progress");
        assert_eq!(error.api_error().code, "database_flow_control_exhausted");
    }

    #[test]
    fn exhausted_query_accepts_local_inactive_credit_error() {
        let error =
            BridgeError::InvalidRequest("target query stream request-1 is not active".to_owned());

        assert!(is_inactive_credit_grant_error(&error));
    }

    #[test]
    fn exhausted_query_accepts_remote_inactive_credit_error() {
        let error = remote_credit_error(wire::ErrorCategory::Validation, "operation.not_active");

        assert!(is_inactive_credit_grant_error(&error));
    }

    #[test]
    fn active_credit_errors_are_not_suppressed() {
        let wrong_category =
            remote_credit_error(wire::ErrorCategory::Internal, "operation.not_active");
        let wrong_code =
            remote_credit_error(wire::ErrorCategory::Validation, "operation.invalid_credit");

        assert!(!is_inactive_credit_grant_error(&wrong_category));
        assert!(!is_inactive_credit_grant_error(&wrong_code));
    }

    #[test]
    fn cleanup_failure_preserves_the_primary_query_failure() {
        let primary = AppError::invalid("primary_query_failure", "primary");
        let outcome: Result<(), QueryTaskError> = Err(QueryTaskError::Failed(primary));

        let error = preserve_primary_outcome(
            "operation-1",
            "test_cleanup",
            outcome,
            Err(AppError::internal()),
        )
        .expect_err("the primary failure remains visible");

        let QueryTaskError::Failed(error) = error else {
            panic!("the primary failure changed category");
        };
        assert_eq!(error.api_error().code, "primary_query_failure");
    }

    #[test]
    fn cleanup_failure_preserves_cancellation() {
        let outcome: Result<(), QueryTaskError> =
            Err(QueryTaskError::Cancelled(Some("requested".to_owned())));

        let error = preserve_primary_outcome(
            "operation-2",
            "test_cleanup",
            outcome,
            Err(AppError::internal()),
        )
        .expect_err("cancellation remains visible");

        let QueryTaskError::Cancelled(reason) = error else {
            panic!("cancellation changed category");
        };
        assert_eq!(reason.as_deref(), Some("requested"));
    }

    #[test]
    fn cleanup_failure_is_reported_when_the_primary_work_succeeded() {
        let cleanup = AppError::invalid("cleanup_failure", "cleanup");

        let error = preserve_primary_outcome("operation-3", "test_cleanup", Ok(()), Err(cleanup))
            .expect_err("cleanup failure must fail an otherwise successful operation");

        let QueryTaskError::Failed(error) = error else {
            panic!("cleanup failure changed category");
        };
        assert_eq!(
            error.api_error(),
            ApiError::new("cleanup_failure", "cleanup")
        );
    }

    fn remote_credit_error(category: wire::ErrorCategory, code: &str) -> BridgeError {
        BridgeError::Remote(Box::new(RemoteEngineError {
            code: code.to_owned(),
            message: "credit grant failed".to_owned(),
            category,
            retryable: false,
            fatal: false,
            outcome: wire::OperationOutcome::NotStarted,
            metadata: HashMap::new(),
            database_error: None,
            session_state: None,
        }))
    }
}
