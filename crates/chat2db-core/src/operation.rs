use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering as AtomicOrdering},
    },
};

use chat2db_contract::{
    ApiError, CancelDisposition, CancelOperationResponse, OperationEvent, OperationEventEnvelope,
    OperationSnapshot, OperationStatus, ResultMetadata,
};
use tokio::sync::{Mutex, RwLock, broadcast, watch};
use uuid::Uuid;

use crate::{AppError, AppErrorKind, now_millis};

const DEFAULT_REPLAY_CAPACITY: usize = 256;
const DEFAULT_OPERATION_CAPACITY: usize = 256;

#[derive(Debug, Clone)]
pub(crate) struct OperationHub {
    inner: Arc<OperationHubInner>,
}

#[derive(Debug)]
struct OperationHubInner {
    operations: RwLock<HashMap<String, Arc<OperationEntry>>>,
    replay_capacity: usize,
    operation_capacity: usize,
    next_terminal_order: AtomicU64,
}

#[derive(Debug)]
struct OperationEntry {
    state: Mutex<OperationState>,
    live: broadcast::Sender<OperationEventEnvelope>,
    cancel: watch::Sender<CancellationRequest>,
    terminal_order: AtomicU64,
}

#[derive(Debug)]
struct OperationState {
    status: OperationStatus,
    sequence: u64,
    started_at_ms: i64,
    updated_at_ms: i64,
    row_count: u64,
    byte_count: u64,
    result: Option<ResultMetadata>,
    error: Option<ApiError>,
    journal: VecDeque<OperationEventEnvelope>,
}

#[derive(Debug)]
pub(crate) struct NewOperation {
    pub id: String,
    pub cancellation: watch::Receiver<CancellationRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CancellationRequest {
    Waiting,
    Requested { reason: Option<String> },
}

/// Atomic replay-plus-live subscription for one operation.
pub struct OperationSubscription {
    entry: Arc<OperationEntry>,
    replay: VecDeque<OperationEventEnvelope>,
    live: broadcast::Receiver<OperationEventEnvelope>,
    cursor: u64,
}

impl std::fmt::Debug for OperationSubscription {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OperationSubscription")
            .field("replay_events", &self.replay.len())
            .field("cursor", &self.cursor)
            .finish_non_exhaustive()
    }
}

impl OperationHub {
    pub(crate) fn new() -> Self {
        Self::with_capacities(DEFAULT_REPLAY_CAPACITY, DEFAULT_OPERATION_CAPACITY)
    }

    #[cfg(test)]
    fn with_capacity(replay_capacity: usize) -> Self {
        Self::with_capacities(replay_capacity, DEFAULT_OPERATION_CAPACITY)
    }

    fn with_capacities(replay_capacity: usize, operation_capacity: usize) -> Self {
        assert!(replay_capacity > 0, "replay capacity must be positive");
        assert!(
            operation_capacity > 0,
            "operation capacity must be positive"
        );
        Self {
            inner: Arc::new(OperationHubInner {
                operations: RwLock::new(HashMap::new()),
                replay_capacity,
                operation_capacity,
                next_terminal_order: AtomicU64::new(1),
            }),
        }
    }

    pub(crate) async fn create(&self) -> Result<NewOperation, AppError> {
        let id = Uuid::new_v4().to_string();
        let timestamp = now_millis()?;
        let mut operations = self.inner.operations.write().await;
        if operations.len() >= self.inner.operation_capacity {
            let oldest_terminal = operations
                .iter()
                .filter_map(|(id, entry)| {
                    let order = entry.terminal_order.load(AtomicOrdering::Acquire);
                    (order != 0).then_some((order, id))
                })
                .min_by(|(left_order, left_id), (right_order, right_id)| {
                    left_order
                        .cmp(right_order)
                        .then_with(|| left_id.cmp(right_id))
                })
                .map(|(_, id)| id.clone());
            let Some(oldest_terminal) = oldest_terminal else {
                return Err(operation_capacity_exhausted());
            };
            operations.remove(&oldest_terminal);
        }
        let (live, _) = broadcast::channel(self.inner.replay_capacity);
        let (cancel, cancellation) = watch::channel(CancellationRequest::Waiting);
        let entry = Arc::new(OperationEntry {
            state: Mutex::new(OperationState {
                status: OperationStatus::Running,
                sequence: 0,
                started_at_ms: timestamp,
                updated_at_ms: timestamp,
                row_count: 0,
                byte_count: 0,
                result: None,
                error: None,
                journal: VecDeque::with_capacity(self.inner.replay_capacity),
            }),
            live,
            cancel,
            terminal_order: AtomicU64::new(0),
        });
        operations.insert(id.clone(), entry);
        Ok(NewOperation { id, cancellation })
    }

    async fn entry(&self, id: &str) -> Result<Arc<OperationEntry>, AppError> {
        self.inner
            .operations
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| {
                AppError::not_found(
                    "operation_not_found",
                    format!("Operation {id} does not exist"),
                )
            })
    }

    pub(crate) async fn snapshot(&self, id: &str) -> Result<OperationSnapshot, AppError> {
        let entry = self.entry(id).await?;
        let state = entry.state.lock().await;
        Ok(state.snapshot(id))
    }

    pub(crate) async fn subscribe(
        &self,
        id: &str,
        after_sequence: Option<u64>,
    ) -> Result<OperationSubscription, AppError> {
        let entry = self.entry(id).await?;
        let state = entry.state.lock().await;
        let live = entry.live.subscribe();
        let cursor = after_sequence.unwrap_or(0);
        validate_replay_cursor(&state, after_sequence)?;
        let replay = state
            .journal
            .iter()
            .filter(|event| parse_sequence(event) > cursor)
            .cloned()
            .collect();
        drop(state);
        Ok(OperationSubscription {
            entry,
            replay,
            live,
            cursor,
        })
    }

    pub(crate) async fn cancel(&self, id: &str) -> CancelOperationResponse {
        let Some(entry) = self.inner.operations.read().await.get(id).cloned() else {
            return CancelOperationResponse {
                operation_id: id.to_owned(),
                disposition: CancelDisposition::UnknownOperation,
            };
        };
        let state = entry.state.lock().await;
        let disposition = if state.status == OperationStatus::Running {
            entry
                .cancel
                .send_replace(CancellationRequest::Requested { reason: None });
            CancelDisposition::Accepted
        } else {
            CancelDisposition::AlreadyTerminal
        };
        CancelOperationResponse {
            operation_id: id.to_owned(),
            disposition,
        }
    }

    pub(crate) async fn cancel_all(&self, reason: &str) {
        let entries = self
            .inner
            .operations
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for entry in entries {
            let state = entry.state.lock().await;
            if state.status == OperationStatus::Running {
                entry.cancel.send_replace(CancellationRequest::Requested {
                    reason: Some(reason.to_owned()),
                });
            }
        }
    }

    pub(crate) async fn started(&self, id: &str) -> Result<(), AppError> {
        self.emit(id, OperationEvent::Started).await
    }

    pub(crate) async fn progress(
        &self,
        id: &str,
        row_count: u64,
        byte_count: u64,
    ) -> Result<(), AppError> {
        self.emit(
            id,
            OperationEvent::Progress {
                row_count: row_count.to_string(),
                byte_count: byte_count.to_string(),
            },
        )
        .await
    }

    pub(crate) async fn completed(&self, id: &str, result: ResultMetadata) -> Result<(), AppError> {
        self.emit(id, OperationEvent::Completed { result }).await
    }

    pub(crate) async fn failed(&self, id: &str, error: ApiError) -> Result<(), AppError> {
        self.emit(id, OperationEvent::Failed { error }).await
    }

    pub(crate) async fn cancelled(&self, id: &str, reason: Option<String>) -> Result<(), AppError> {
        self.emit(id, OperationEvent::Cancelled { reason }).await
    }

    async fn emit(&self, id: &str, event: OperationEvent) -> Result<(), AppError> {
        let entry = self.entry(id).await?;
        let mut state = entry.state.lock().await;
        if state.status != OperationStatus::Running {
            return Ok(());
        }
        apply_event(&mut state, &event)?;
        state.sequence = state
            .sequence
            .checked_add(1)
            .ok_or_else(AppError::internal)?;
        state.updated_at_ms = now_millis()?;
        let envelope = OperationEventEnvelope {
            operation_id: id.to_owned(),
            sequence: state.sequence.to_string(),
            occurred_at_ms: state.updated_at_ms.to_string(),
            event,
        };
        if state.journal.len() == self.inner.replay_capacity {
            state.journal.pop_front();
        }
        state.journal.push_back(envelope.clone());
        let _ = entry.live.send(envelope);
        if state.status != OperationStatus::Running {
            let terminal_order = self
                .inner
                .next_terminal_order
                .fetch_update(AtomicOrdering::AcqRel, AtomicOrdering::Acquire, |current| {
                    current.checked_add(1)
                })
                .map_err(|_| AppError::internal())?;
            entry
                .terminal_order
                .store(terminal_order, AtomicOrdering::Release);
        }
        Ok(())
    }
}

fn operation_capacity_exhausted() -> AppError {
    let mut error = ApiError::new(
        "operation_capacity_exhausted",
        "Too many query operations are active; wait for one to finish",
    );
    error.retryable = true;
    AppError::new(AppErrorKind::ResourceExhausted, error)
}

impl OperationState {
    fn snapshot(&self, id: &str) -> OperationSnapshot {
        OperationSnapshot {
            operation_id: id.to_owned(),
            status: self.status,
            last_sequence: self.sequence.to_string(),
            started_at_ms: self.started_at_ms.to_string(),
            updated_at_ms: self.updated_at_ms.to_string(),
            row_count: self.row_count.to_string(),
            byte_count: self.byte_count.to_string(),
            result: self.result.clone(),
            error: self.error.clone(),
        }
    }
}

impl OperationSubscription {
    /// Returns the next replayed or live event, or `None` after a terminal event.
    ///
    /// # Errors
    ///
    /// Returns a replay-window error if a lagged subscriber can no longer be recovered.
    pub async fn next_event(&mut self) -> Result<Option<OperationEventEnvelope>, AppError> {
        loop {
            if let Some(event) = self.replay.pop_front() {
                self.cursor = parse_sequence(&event);
                return Ok(Some(event));
            }

            {
                let state = self.entry.state.lock().await;
                if state.status != OperationStatus::Running && self.cursor >= state.sequence {
                    return Ok(None);
                }
            }

            match self.live.recv().await {
                Ok(event) => {
                    let sequence = parse_sequence(&event);
                    if sequence <= self.cursor {
                        continue;
                    }
                    self.cursor = sequence;
                    return Ok(Some(event));
                }
                Err(broadcast::error::RecvError::Closed) => return Ok(None),
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let state = self.entry.state.lock().await;
                    validate_replay_cursor(&state, Some(self.cursor))?;
                    self.replay = state
                        .journal
                        .iter()
                        .filter(|event| parse_sequence(event) > self.cursor)
                        .cloned()
                        .collect();
                }
            }
        }
    }
}

fn validate_replay_cursor(state: &OperationState, requested: Option<u64>) -> Result<(), AppError> {
    let Some(requested) = requested else {
        return Ok(());
    };
    if requested > state.sequence {
        return Err(AppError::invalid(
            "invalid_operation_sequence",
            "The requested operation sequence is ahead of the operation",
        ));
    }
    let oldest = state.journal.front().map_or(state.sequence, parse_sequence);
    if requested.saturating_add(1) < oldest {
        return Err(AppError::replay_window(requested, oldest, state.sequence));
    }
    Ok(())
}

fn apply_event(state: &mut OperationState, event: &OperationEvent) -> Result<(), AppError> {
    match event {
        OperationEvent::Started => {}
        OperationEvent::Progress {
            row_count,
            byte_count,
        } => {
            let rows = row_count.parse::<u64>().map_err(|_| AppError::internal())?;
            let bytes = byte_count
                .parse::<u64>()
                .map_err(|_| AppError::internal())?;
            if rows < state.row_count || bytes < state.byte_count {
                return Err(AppError::internal());
            }
            state.row_count = rows;
            state.byte_count = bytes;
        }
        OperationEvent::Completed { result } => {
            state.status = OperationStatus::Completed;
            state.row_count = parse_contract_u64(&result.row_count)?;
            state.byte_count = parse_contract_u64(&result.byte_count)?;
            state.result = Some(result.clone());
        }
        OperationEvent::Failed { error } => {
            state.status = OperationStatus::Failed;
            state.error = Some(error.clone());
        }
        OperationEvent::Cancelled { .. } => state.status = OperationStatus::Cancelled,
    }
    Ok(())
}

fn parse_contract_u64(value: &str) -> Result<u64, AppError> {
    value.parse().map_err(|_| AppError::internal())
}

fn parse_sequence(event: &OperationEventEnvelope) -> u64 {
    event
        .sequence
        .parse()
        .expect("operation sequence is generated from u64")
}

#[cfg(test)]
mod tests {
    use chat2db_contract::{CancelDisposition, OperationEvent, OperationStatus};

    use crate::AppErrorKind;

    use super::{CancellationRequest, OperationHub};

    #[tokio::test]
    async fn subscription_atomically_replays_then_follows_live_events() {
        let hub = OperationHub::with_capacity(4);
        let operation = hub.create().await.expect("operation");
        hub.started(&operation.id).await.expect("started");
        let mut subscription = hub
            .subscribe(&operation.id, Some(0))
            .await
            .expect("subscription");
        hub.progress(&operation.id, 2, 32).await.expect("progress");

        assert!(matches!(
            subscription.next_event().await.expect("event"),
            Some(event) if matches!(event.event, OperationEvent::Started)
        ));
        assert!(matches!(
            subscription.next_event().await.expect("event"),
            Some(event) if matches!(event.event, OperationEvent::Progress { .. })
        ));
    }

    #[tokio::test]
    async fn replay_window_failure_is_structured() {
        let hub = OperationHub::with_capacity(2);
        let operation = hub.create().await.expect("operation");
        hub.started(&operation.id).await.expect("started");
        hub.progress(&operation.id, 1, 10).await.expect("progress");
        hub.progress(&operation.id, 2, 20).await.expect("progress");

        let error = hub
            .subscribe(&operation.id, Some(0))
            .await
            .expect_err("old cursor must fail");
        assert_eq!(error.api_error().code, "operation_replay_window_expired");
    }

    #[tokio::test]
    async fn exactly_one_terminal_event_wins() {
        let hub = OperationHub::new();
        let operation = hub.create().await.expect("operation");
        hub.cancelled(&operation.id, None).await.expect("cancelled");
        hub.failed(
            &operation.id,
            chat2db_contract::ApiError::new("late", "late"),
        )
        .await
        .expect("late terminal ignored");

        let snapshot = hub.snapshot(&operation.id).await.expect("snapshot");
        assert_eq!(snapshot.status, OperationStatus::Cancelled);
        assert_eq!(snapshot.last_sequence, "1");
        assert!(snapshot.error.is_none());
    }

    #[tokio::test]
    async fn dropping_a_subscription_does_not_cancel_the_operation() {
        let hub = OperationHub::new();
        let operation = hub.create().await.expect("operation");
        let cancellation = operation.cancellation;
        let subscription = hub
            .subscribe(&operation.id, None)
            .await
            .expect("subscription");

        drop(subscription);
        assert_eq!(*cancellation.borrow(), CancellationRequest::Waiting);
        assert_eq!(
            hub.snapshot(&operation.id).await.expect("snapshot").status,
            OperationStatus::Running
        );

        let response = hub.cancel(&operation.id).await;
        assert_eq!(response.disposition, CancelDisposition::Accepted);
        assert_eq!(
            *cancellation.borrow(),
            CancellationRequest::Requested { reason: None }
        );
    }

    #[tokio::test]
    async fn capacity_evicts_the_oldest_terminal_operation() {
        let hub = OperationHub::with_capacities(4, 2);
        let oldest = hub.create().await.expect("oldest operation");
        hub.cancelled(&oldest.id, None)
            .await
            .expect("oldest terminal");
        let mut oldest_subscription = hub
            .subscribe(&oldest.id, Some(0))
            .await
            .expect("terminal subscription");

        let newer = hub.create().await.expect("newer operation");
        hub.cancelled(&newer.id, None)
            .await
            .expect("newer terminal");
        let replacement = hub.create().await.expect("replacement operation");

        assert_eq!(
            hub.snapshot(&oldest.id)
                .await
                .expect_err("oldest terminal must be evicted")
                .api_error()
                .code,
            "operation_not_found"
        );
        assert_eq!(
            hub.snapshot(&newer.id)
                .await
                .expect("newer retained")
                .status,
            OperationStatus::Cancelled
        );
        assert_eq!(
            hub.snapshot(&replacement.id)
                .await
                .expect("replacement retained")
                .status,
            OperationStatus::Running
        );
        assert!(matches!(
            oldest_subscription.next_event().await.expect("replay"),
            Some(event) if matches!(event.event, OperationEvent::Cancelled { .. })
        ));
        assert!(
            oldest_subscription
                .next_event()
                .await
                .expect("terminal completion")
                .is_none()
        );
    }

    #[tokio::test]
    async fn capacity_rejects_creation_when_every_operation_is_running() {
        let hub = OperationHub::with_capacities(4, 2);
        let first = hub.create().await.expect("first operation");
        let second = hub.create().await.expect("second operation");

        let error = hub
            .create()
            .await
            .expect_err("all-running registry must reject admission");
        assert_eq!(error.kind(), AppErrorKind::ResourceExhausted);
        assert_eq!(error.api_error().code, "operation_capacity_exhausted");
        assert!(error.api_error().retryable);
        assert_eq!(
            hub.snapshot(&first.id)
                .await
                .expect("first retained")
                .status,
            OperationStatus::Running
        );
        assert_eq!(
            hub.snapshot(&second.id)
                .await
                .expect("second retained")
                .status,
            OperationStatus::Running
        );
    }
}
