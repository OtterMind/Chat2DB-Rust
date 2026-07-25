use std::time::Duration;

use chat2db_contract::{ApiError, QueryAccepted, QueryLimits, StartQueryRequest};
use chat2db_engine_protocol::wire;
use chat2db_java_bridge::{
    BridgeError, CancelDisposition as BridgeCancelDisposition, ConnectionProperty, EngineClient,
    JdbcParameter, QueryEvent, QueryOptions, QueryRequest, QueryStream, SessionConfig,
};
use chat2db_storage::{ResultWriter, SecretValue, Storage, StorageError};
use tokio::sync::{oneshot, watch};

use crate::{AppError, AppErrorKind, Application, convert, operation::CancellationRequest};

struct PreparedQuery {
    datasource_id: String,
    sql: String,
    parameters: Vec<JdbcParameter>,
    options: QueryOptions,
    retention: Duration,
}

enum QueryTaskError {
    Failed(AppError),
    Cancelled(Option<String>),
}

impl From<AppError> for QueryTaskError {
    fn from(error: AppError) -> Self {
        Self::Failed(error)
    }
}

struct RetainedWriter {
    inner: Option<ResultWriter>,
}

impl Application {
    /// Accepts a query for asynchronous execution and returns its operation id.
    ///
    /// # Errors
    ///
    /// Returns a validation or component-availability error before acceptance.
    pub async fn start_query(&self, request: StartQueryRequest) -> Result<QueryAccepted, AppError> {
        let prepared = PreparedQuery::try_from(request)?;
        let storage = self.require_storage()?;
        let engine = self.require_engine()?;
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
                    engine,
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
        engine: EngineClient,
    ) {
        let outcome = self
            .execute_query_task(&operation_id, cancellation, query, storage, engine)
            .await;
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
        engine: EngineClient,
    ) -> Result<chat2db_contract::ResultMetadata, QueryTaskError> {
        let datasource_id = query.datasource_id.clone();
        let datasource_storage = storage.clone();
        let (datasource, secret) =
            run_blocking(move || datasource_storage.get_datasource_with_secret(&datasource_id))
                .await?;
        let secret = secret.ok_or_else(|| {
            QueryTaskError::Failed(AppError::new(
                AppErrorKind::Conflict,
                ApiError::new(
                    "datasource_connection_missing",
                    "The datasource has no installed connection descriptor",
                ),
            ))
        })?;
        let connection = decode_connection(&secret)?;

        if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
            return Err(QueryTaskError::Cancelled(reason));
        }

        let driver = engine.driver_client().map_err(AppError::from)?;
        let session = driver
            .open_session(SessionConfig {
                driver_id: datasource.driver_id,
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
                read_only: connection.read_only,
            })
            .await
            .map_err(AppError::from)?;

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
        })
    }
}

impl RetainedWriter {
    async fn begin(
        storage: Storage,
        schema: wire::QueryStarted,
        retention: Duration,
    ) -> Result<Self, AppError> {
        let writer = run_blocking(move || storage.begin_result(&schema, retention)).await?;
        Ok(Self {
            inner: Some(writer),
        })
    }

    async fn append(&mut self, batch: wire::RowBatch) -> Result<u64, AppError> {
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

    async fn finish(
        &mut self,
        completed: wire::QueryCompleted,
    ) -> Result<chat2db_contract::ResultMetadata, AppError> {
        let writer = self.inner.take().ok_or_else(AppError::internal)?;
        let metadata = run_blocking(move || writer.finish(&completed)).await?;
        Ok(convert::result_metadata(metadata))
    }

    async fn abort(&mut self) -> Result<(), AppError> {
        if let Some(writer) = self.inner.take() {
            tokio::task::spawn_blocking(move || writer.abort())
                .await
                .map_err(|_| AppError::internal())?
                .map_err(AppError::from)?;
        }
        Ok(())
    }
}

fn decode_connection(
    secret: &SecretValue,
) -> Result<chat2db_contract::DatasourceConnection, QueryTaskError> {
    serde_json::from_slice(secret.expose_secret()).map_err(|_| {
        QueryTaskError::Failed(AppError::new(
            AppErrorKind::Internal,
            ApiError::new(
                "datasource_connection_invalid",
                "The stored datasource connection descriptor is invalid",
            ),
        ))
    })
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
        AppError, QueryTaskError, is_inactive_credit_grant_error, preserve_primary_outcome,
        validate_credit_grant,
    };

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
