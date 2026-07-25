use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    sync::{
        Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering},
    },
    time::Duration,
};

use chat2db_contract::{
    AgentEvent, AgentEventEnvelope, AgentPermissionStatus, AgentRunSnapshot, AgentRunStatus,
    ApiError, ApiErrorDetails,
};
use tokio::{
    sync::{Mutex, OwnedSemaphorePermit, Semaphore, broadcast, watch},
    task::{AbortHandle, JoinHandle},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use crate::{AppError, AppErrorKind};

const DEFAULT_REPLAY_CAPACITY: usize = 256;
const DEFAULT_LIVE_CAPACITY: usize = 32;
const DEFAULT_RUN_CAPACITY: usize = 256;
const DEFAULT_ACTIVE_RUN_CAPACITY: usize = 8;

/// Process-local coordinator for live agent runs.
///
/// Durable `SQLite` snapshots remain authoritative. This hub only admits work,
/// serializes durable transitions, and retains a bounded live-event window.
#[derive(Clone)]
pub(crate) struct AgentRunHub {
    inner: Arc<AgentRunHubInner>,
}

struct AgentRunHubInner {
    runs: StdMutex<HashMap<String, RegisteredEntry>>,
    admission: Mutex<()>,
    replay_capacity: usize,
    live_capacity: usize,
    entry_slots: Arc<Semaphore>,
    active_slots: Arc<Semaphore>,
    next_terminal_order: AtomicU64,
}

struct RegisteredEntry {
    entry: Arc<AgentRunEntry>,
    _entry_permit: OwnedSemaphorePermit,
}

struct AgentRunEntry {
    transition: Mutex<()>,
    state: Mutex<AgentRunState>,
    live: broadcast::Sender<AgentEventEnvelope>,
    cancellation: CancellationToken,
    unavailable: CancellationToken,
    permission: Mutex<PermissionState>,
    task: StdMutex<TaskState>,
    task_done: AtomicBool,
    terminal_order: AtomicU64,
}

struct AgentRunState {
    snapshot: AgentRunSnapshot,
    sequence: u64,
    journal: VecDeque<AgentEventEnvelope>,
}

#[derive(Default)]
struct PermissionState {
    current: Option<PermissionSignal>,
}

struct PermissionSignal {
    permission_id: String,
    sender: watch::Sender<PermissionSignalState>,
}

struct TaskState {
    monitor: Option<JoinHandle<()>>,
    worker_abort: Option<AbortHandle>,
    attached: bool,
    done: bool,
    active_permit: Option<OwnedSemaphorePermit>,
}

struct InFlightTransition {
    hub: AgentRunHub,
    run_id: String,
    entry: Arc<AgentRunEntry>,
    armed: bool,
}

/// Capacity reservation acquired before a durable run is created.
///
/// Dropping an unregistered reservation releases both permits.
pub(crate) struct AgentRunReservation {
    hub: AgentRunHub,
    active_permit: Option<OwnedSemaphorePermit>,
    entry_permit: Option<OwnedSemaphorePermit>,
}

/// Process-local controls installed after the durable start transaction commits.
#[derive(Clone)]
pub(crate) struct RegisteredAgentRun {
    run_id: String,
    cancellation: CancellationToken,
}

/// Snapshot and event produced by one successful durable transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DurableAgentTransition {
    pub snapshot: AgentRunSnapshot,
    pub event: AgentEvent,
}

/// Explicit durability classification for a failed transition attempt.
///
/// There is deliberately no `From<AppError>` implementation: every caller must
/// decide whether the same event sequence can be retried safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentTransitionFailure {
    /// The durable operation definitely did not commit, so the sequence is reusable.
    DefinitelyNotCommitted(AppError),
    /// Commit status is unknown, so the process-local entry must be discarded.
    Indeterminate(AppError),
}

/// Atomic replay-plus-live subscription for one process-local run.
pub struct AgentRunSubscription {
    entry: Arc<AgentRunEntry>,
    replay: VecDeque<AgentEventEnvelope>,
    live: broadcast::Receiver<AgentEventEnvelope>,
    cursor: u64,
}

/// Result delivered to the exact pending permission waiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentPermissionWaitOutcome {
    Resolved(AgentPermissionStatus),
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermissionSignalState {
    Waiting,
    Resolved(AgentPermissionStatus),
    Cancelled,
}

/// Receiver bound to one immutable permission id.
pub(crate) struct AgentPermissionWaiter {
    permission_id: String,
    receiver: watch::Receiver<PermissionSignalState>,
    cancellation: CancellationToken,
}

impl std::fmt::Debug for AgentRunHub {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentRunHub")
            .field("replay_capacity", &self.inner.replay_capacity)
            .field("live_capacity", &self.inner.live_capacity)
            .field(
                "entry_capacity",
                &self.inner.entry_slots.available_permits(),
            )
            .field(
                "active_capacity",
                &self.inner.active_slots.available_permits(),
            )
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for AgentRunReservation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentRunReservation")
            .field("active_reserved", &self.active_permit.is_some())
            .field("entry_reserved", &self.entry_permit.is_some())
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for RegisteredAgentRun {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RegisteredAgentRun")
            .field("run_id", &self.run_id)
            .field("cancelled", &self.cancellation.is_cancelled())
            .finish()
    }
}

impl std::fmt::Debug for AgentRunSubscription {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentRunSubscription")
            .field("replay_events", &self.replay.len())
            .field("cursor", &self.cursor)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for AgentPermissionWaiter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentPermissionWaiter")
            .field("permission_id", &self.permission_id)
            .finish_non_exhaustive()
    }
}

impl InFlightTransition {
    fn new(hub: AgentRunHub, run_id: &str, entry: Arc<AgentRunEntry>) -> Self {
        Self {
            hub,
            run_id: run_id.to_owned(),
            entry,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for InFlightTransition {
    fn drop(&mut self) {
        if self.armed {
            self.hub.invalidate_entry(&self.run_id, &self.entry, false);
        }
    }
}

impl Default for AgentRunHub {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRunHub {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::with_capacities(
            DEFAULT_REPLAY_CAPACITY,
            DEFAULT_LIVE_CAPACITY,
            DEFAULT_RUN_CAPACITY,
            DEFAULT_ACTIVE_RUN_CAPACITY,
        )
    }

    fn with_capacities(
        replay_capacity: usize,
        live_capacity: usize,
        run_capacity: usize,
        active_run_capacity: usize,
    ) -> Self {
        assert!(replay_capacity > 0, "replay capacity must be positive");
        assert!(live_capacity > 0, "live capacity must be positive");
        assert!(run_capacity > 0, "run capacity must be positive");
        assert!(
            active_run_capacity > 0,
            "active run capacity must be positive"
        );
        Self {
            inner: Arc::new(AgentRunHubInner {
                runs: StdMutex::new(HashMap::new()),
                admission: Mutex::new(()),
                replay_capacity,
                live_capacity,
                entry_slots: Arc::new(Semaphore::new(run_capacity)),
                active_slots: Arc::new(Semaphore::new(active_run_capacity)),
                next_terminal_order: AtomicU64::new(1),
            }),
        }
    }

    /// Reserves bounded process capacity before the caller starts a `SQLite` transaction.
    ///
    /// # Errors
    ///
    /// Returns a retryable resource-exhausted error when either active or retained
    /// run capacity is full and no completed task can be evicted.
    pub(crate) async fn reserve(&self) -> Result<AgentRunReservation, AppError> {
        let active_permit = Arc::clone(&self.inner.active_slots)
            .try_acquire_owned()
            .map_err(|_| run_capacity_exhausted())?;
        let _admission = self.inner.admission.lock().await;
        let entry_permit =
            if let Ok(permit) = Arc::clone(&self.inner.entry_slots).try_acquire_owned() {
                permit
            } else {
                let evicted = {
                    let mut runs = lock_std(&self.inner.runs);
                    let candidate = runs
                        .iter()
                        .filter_map(|(id, registered)| {
                            registered.entry.eviction_order().map(|order| (order, id))
                        })
                        .min_by(|(left_order, left_id), (right_order, right_id)| {
                            left_order
                                .cmp(right_order)
                                .then_with(|| left_id.cmp(right_id))
                        })
                        .map(|(_, id)| id.clone());
                    candidate.and_then(|id| runs.remove(&id))
                };
                let Some(evicted) = evicted else {
                    return Err(run_capacity_exhausted());
                };
                drop(evicted);
                Arc::clone(&self.inner.entry_slots)
                    .try_acquire_owned()
                    .map_err(|_| AppError::internal())?
            };
        Ok(AgentRunReservation {
            hub: self.clone(),
            active_permit: Some(active_permit),
            entry_permit: Some(entry_permit),
        })
    }

    fn entry(&self, run_id: &str) -> Result<Arc<AgentRunEntry>, AppError> {
        let entry = lock_std(&self.inner.runs)
            .get(run_id)
            .map(|registered| Arc::clone(&registered.entry))
            .ok_or_else(agent_event_stream_unavailable)?;
        if entry.unavailable.is_cancelled() {
            return Err(agent_event_stream_unavailable());
        }
        Ok(entry)
    }

    /// Returns the last caller-supplied durable snapshot cached for stream control.
    /// Polling APIs must still read `SQLite` directly.
    ///
    /// # Errors
    ///
    /// Returns stream-unavailable when the run is not registered in this process.
    pub(crate) async fn cached_snapshot(&self, run_id: &str) -> Result<AgentRunSnapshot, AppError> {
        let entry = self.entry(run_id)?;
        Ok(entry.state.lock().await.snapshot.clone())
    }

    /// Subscribes atomically to retained replay followed by live events.
    ///
    /// # Errors
    ///
    /// Returns a stable stream-unavailable error for unknown process-local runs,
    /// or a structured replay error for an expired or forward cursor.
    pub(crate) async fn subscribe(
        &self,
        run_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<AgentRunSubscription, AppError> {
        let entry = self.entry(run_id)?;
        let state = entry.state.lock().await;
        let live = entry.live.subscribe();
        let cursor = after_sequence.unwrap_or(0);
        validate_replay_cursor(&state, after_sequence)?;
        let replay = state
            .journal
            .iter()
            .filter(|event| event_sequence(event) > cursor)
            .cloned()
            .collect();
        drop(state);
        if entry.unavailable.is_cancelled() {
            return Err(agent_event_stream_unavailable());
        }
        Ok(AgentRunSubscription {
            entry,
            replay,
            live,
            cursor,
        })
    }

    /// Runs one sequence allocation and caller-owned persistence operation in
    /// per-run order, publishing only the returned durable snapshot.
    ///
    /// The persistence future executes while the transition mutex is held but
    /// without the journal state mutex, so unrelated snapshot and subscription
    /// reads remain available while `SQLite` is busy.
    ///
    /// # Errors
    ///
    /// Definitely-not-committed failures preserve the entry without consuming a
    /// sequence. Indeterminate failures and every failure after a reported commit
    /// invalidate the entry so a potentially durable sequence is never reused.
    pub(crate) async fn transition<F, Fut>(
        &self,
        run_id: &str,
        persist: F,
    ) -> Result<AgentRunSnapshot, AppError>
    where
        F: FnOnce(u64) -> Fut,
        Fut: Future<Output = Result<DurableAgentTransition, AgentTransitionFailure>>,
    {
        let entry = self.entry(run_id)?;
        let _transition = entry.transition.lock().await;
        if entry.unavailable.is_cancelled() {
            return Err(agent_event_stream_unavailable());
        }
        let (next_sequence, prior_snapshot) = {
            let state = entry.state.lock().await;
            if is_terminal(state.snapshot.status) {
                return Err(agent_run_already_terminal());
            }
            let next_sequence = state
                .sequence
                .checked_add(1)
                .ok_or_else(AppError::internal)?;
            (next_sequence, state.snapshot.clone())
        };
        let mut in_flight = InFlightTransition::new(self.clone(), run_id, Arc::clone(&entry));

        let durable = match persist(next_sequence).await {
            Ok(durable) => durable,
            Err(AgentTransitionFailure::DefinitelyNotCommitted(error)) => {
                in_flight.disarm();
                return Err(error);
            }
            Err(AgentTransitionFailure::Indeterminate(error)) => {
                self.invalidate_entry(run_id, &entry, false);
                in_flight.disarm();
                return Err(error);
            }
        };
        if let Err(error) =
            validate_durable_transition(run_id, &prior_snapshot, next_sequence, &durable)
        {
            self.invalidate_entry(run_id, &entry, false);
            in_flight.disarm();
            return Err(error);
        }
        if let AgentEvent::PermissionRequested { permission } = &durable.event
            && let Err(error) = install_permission_signal(&entry, &permission.permission_id).await
        {
            self.invalidate_entry(run_id, &entry, false);
            in_flight.disarm();
            return Err(error);
        }
        let DurableAgentTransition { snapshot, event } = durable;
        let terminal = is_terminal(snapshot.status);
        let envelope = AgentEventEnvelope {
            run_id: run_id.to_owned(),
            sequence: next_sequence.to_string(),
            occurred_at_ms: snapshot.updated_at_ms.clone(),
            event,
        };
        let sequence_consistent = {
            let mut state = entry.state.lock().await;
            if state.sequence.checked_add(1) == Some(next_sequence) {
                state.sequence = next_sequence;
                state.snapshot = snapshot.clone();
                if state.journal.len() == self.inner.replay_capacity {
                    state.journal.pop_front();
                }
                state.journal.push_back(envelope.clone());
                let _ = entry.live.send(envelope);
                true
            } else {
                false
            }
        };
        if !sequence_consistent {
            self.invalidate_entry(run_id, &entry, false);
            in_flight.disarm();
            return Err(AppError::internal());
        }

        if terminal {
            cancel_current_permission(&entry).await;
            let order = self
                .inner
                .next_terminal_order
                .fetch_update(AtomicOrdering::AcqRel, AtomicOrdering::Acquire, |current| {
                    current.checked_add(1)
                })
                .unwrap_or(u64::MAX);
            entry
                .terminal_order
                .store(order.max(1), AtomicOrdering::Release);
        }
        in_flight.disarm();
        Ok(snapshot)
    }

    /// Returns a clone of the cooperative cancellation token for one live run.
    ///
    /// # Errors
    ///
    /// Returns stream-unavailable when the run has no process-local entry.
    pub(crate) fn cancellation_token(&self, run_id: &str) -> Result<CancellationToken, AppError> {
        Ok(self.entry(run_id)?.cancellation.clone())
    }

    /// Requests cooperative cancellation and wakes a pending permission waiter.
    /// Returns `false` when the cached durable snapshot is already terminal.
    ///
    /// # Errors
    ///
    /// Returns stream-unavailable when the run has no process-local entry.
    pub(crate) async fn request_cancellation(&self, run_id: &str) -> Result<bool, AppError> {
        let entry = self.entry(run_id)?;
        let terminal = is_terminal(entry.state.lock().await.snapshot.status);
        if terminal {
            return Ok(false);
        }
        entry.cancellation.cancel();
        cancel_current_permission(&entry).await;
        Ok(true)
    }

    /// Installs or re-subscribes to the waiter for one exact permission id.
    ///
    /// # Errors
    ///
    /// Returns a conflict if a different permission is still waiting.
    pub(crate) async fn install_permission_waiter(
        &self,
        run_id: &str,
        permission_id: &str,
    ) -> Result<AgentPermissionWaiter, AppError> {
        if permission_id.is_empty() {
            return Err(AppError::invalid(
                "invalid_agent_permission_id",
                "The agent permission id must not be empty",
            ));
        }
        let entry = self.entry(run_id)?;
        let _transition = entry.transition.lock().await;
        if entry.unavailable.is_cancelled() {
            return Err(agent_event_stream_unavailable());
        }
        if is_terminal(entry.state.lock().await.snapshot.status) {
            return Err(agent_run_already_terminal());
        }
        install_permission_signal(&entry, permission_id).await
    }

    /// Resolves the waiter only when its immutable permission id matches.
    /// Returns `true` only to the first resolver.
    ///
    /// # Errors
    ///
    /// A durable decision may race ahead of process-local signal installation;
    /// in that case the resolved signal is retained for the later waiter.
    pub(crate) async fn resolve_permission(
        &self,
        run_id: &str,
        permission_id: &str,
        status: AgentPermissionStatus,
    ) -> Result<bool, AppError> {
        if status == AgentPermissionStatus::Pending {
            return Err(AppError::invalid(
                "invalid_agent_permission_resolution",
                "A pending permission cannot resolve its waiter",
            ));
        }
        let entry = self.entry(run_id)?;
        settle_permission(
            &entry,
            permission_id,
            PermissionSignalState::Resolved(status),
        )
        .await
    }

    /// Cancels the waiter only when its immutable permission id matches.
    /// Returns `true` only to the first resolver.
    ///
    /// # Errors
    ///
    /// A durable cancellation may race ahead of process-local signal installation;
    /// in that case the cancelled signal is retained for the later waiter.
    pub(crate) async fn cancel_permission(
        &self,
        run_id: &str,
        permission_id: &str,
    ) -> Result<bool, AppError> {
        let entry = self.entry(run_id)?;
        settle_permission(&entry, permission_id, PermissionSignalState::Cancelled).await
    }

    /// Cancels every process-local run and wakes every pending permission waiter.
    pub(crate) async fn cancel_all(&self) {
        let entries = lock_std(&self.inner.runs)
            .values()
            .map(|registered| Arc::clone(&registered.entry))
            .collect::<Vec<_>>();
        for entry in entries {
            entry.cancellation.cancel();
            cancel_current_permission(&entry).await;
        }
    }

    /// Waits for every owned worker until one shared deadline. At the deadline,
    /// it aborts workers and monitors, invalidates their entries, releases their
    /// permits, and returns without another unbounded wait.
    pub(crate) async fn join_tasks(&self, timeout: Duration) {
        let entries = lock_std(&self.inner.runs)
            .iter()
            .map(|(run_id, registered)| (run_id.clone(), Arc::clone(&registered.entry)))
            .collect::<Vec<_>>();
        let mut monitors = VecDeque::new();
        for (run_id, entry) in &entries {
            let monitor = lock_std(&entry.task).monitor.take();
            if let Some(monitor) = monitor {
                monitors.push_back(monitor);
            } else if !entry.task_done.load(AtomicOrdering::Acquire) {
                // A committed registration can briefly exist before task
                // attachment. Shutdown must close that window as well.
                self.invalidate_entry(run_id, entry, true);
            }
        }

        let deadline = Instant::now() + timeout;
        while let Some(mut monitor) = monitors.pop_front() {
            if tokio::time::timeout_at(deadline, &mut monitor)
                .await
                .is_err()
            {
                monitors.push_front(monitor);
                for monitor in monitors {
                    monitor.abort();
                }
                for (run_id, entry) in &entries {
                    if !entry.task_done.load(AtomicOrdering::Acquire) {
                        self.invalidate_entry(run_id, entry, true);
                    }
                }
                return;
            }
        }
    }

    /// Transfers ownership of a worker task to the hub. A monitor awaits the
    /// worker (including panic or abort) before releasing its active-run permit.
    ///
    /// # Errors
    ///
    /// Aborts the supplied task and returns an error for an unknown run or a
    /// second attachment.
    pub(crate) fn attach_task(&self, run_id: &str, task: JoinHandle<()>) -> Result<(), AppError> {
        let entry = match self.entry(run_id) {
            Ok(entry) => entry,
            Err(error) => {
                task.abort();
                return Err(error);
            }
        };
        let mut task_state = lock_std(&entry.task);
        if task_state.attached || task_state.done {
            task.abort();
            return Err(agent_task_already_attached());
        }
        task_state.attached = true;
        task_state.worker_abort = Some(task.abort_handle());
        let hub = self.clone();
        let run_id = run_id.to_owned();
        let monitored_entry = Arc::clone(&entry);
        task_state.monitor = Some(tokio::spawn(async move {
            let _ = task.await;
            hub.task_done(&run_id, &monitored_entry).await;
        }));
        Ok(())
    }

    async fn task_done(&self, run_id: &str, entry: &Arc<AgentRunEntry>) {
        let _transition = entry.transition.lock().await;
        if !entry.unavailable.is_cancelled()
            && !is_terminal(entry.state.lock().await.snapshot.status)
        {
            self.invalidate_entry(run_id, entry, false);
        }
        let (active_permit, monitor, worker_abort) = {
            let mut task = lock_std(&entry.task);
            if task.done {
                return;
            }
            task.done = true;
            (
                task.active_permit.take(),
                task.monitor.take(),
                task.worker_abort.take(),
            )
        };
        drop(active_permit);
        drop(monitor);
        drop(worker_abort);
        entry.task_done.store(true, AtomicOrdering::Release);
        if entry.unavailable.is_cancelled() {
            self.remove_entry(run_id, entry);
        }
    }

    fn invalidate_entry(&self, run_id: &str, entry: &Arc<AgentRunEntry>, force: bool) {
        entry.unavailable.cancel();
        entry.cancellation.cancel();
        if let Ok(permission) = entry.permission.try_lock()
            && let Some(current) = permission.current.as_ref()
            && *current.sender.borrow() == PermissionSignalState::Waiting
        {
            current
                .sender
                .send_replace(PermissionSignalState::Cancelled);
        }

        let (cleanup, remove_now) = {
            let mut task = lock_std(&entry.task);
            if task.done {
                (None, true)
            } else if force || !task.attached {
                if let Some(worker_abort) = task.worker_abort.as_ref() {
                    worker_abort.abort();
                }
                if let Some(monitor) = task.monitor.as_ref() {
                    monitor.abort();
                }
                task.done = true;
                (
                    Some((
                        task.active_permit.take(),
                        task.monitor.take(),
                        task.worker_abort.take(),
                    )),
                    true,
                )
            } else {
                if let Some(worker_abort) = task.worker_abort.as_ref() {
                    worker_abort.abort();
                }
                (None, false)
            }
        };
        if remove_now {
            self.remove_entry(run_id, entry);
        }
        if let Some((active_permit, monitor, worker_abort)) = cleanup {
            drop(active_permit);
            drop(monitor);
            drop(worker_abort);
            entry.task_done.store(true, AtomicOrdering::Release);
        }
    }

    fn remove_entry(&self, run_id: &str, entry: &Arc<AgentRunEntry>) {
        let removed = {
            let mut runs = lock_std(&self.inner.runs);
            let registered_here = runs
                .get(run_id)
                .is_some_and(|registered| Arc::ptr_eq(&registered.entry, entry));
            registered_here.then(|| runs.remove(run_id)).flatten()
        };
        drop(removed);
    }
}

impl AgentRunReservation {
    /// Registers a committed `SQLite` run and publishes its initial `Started` event.
    /// The event sequence is exactly the snapshot's `lastSequence`.
    ///
    /// # Errors
    ///
    /// Returns an internal error for an invalid start snapshot, or a conflict if
    /// the run id is already registered.
    pub(crate) fn register_started(
        mut self,
        snapshot: AgentRunSnapshot,
    ) -> Result<RegisteredAgentRun, AppError> {
        let sequence = validate_started_snapshot(&snapshot)?;
        let active_permit = self.active_permit.take().ok_or_else(AppError::internal)?;
        let entry_permit = self.entry_permit.take().ok_or_else(AppError::internal)?;
        let cancellation = CancellationToken::new();
        let (live, _) = broadcast::channel(self.hub.inner.live_capacity);
        let envelope = AgentEventEnvelope {
            run_id: snapshot.run_id.clone(),
            sequence: sequence.to_string(),
            occurred_at_ms: snapshot.updated_at_ms.clone(),
            event: AgentEvent::Started,
        };
        let mut journal = VecDeque::with_capacity(self.hub.inner.replay_capacity);
        journal.push_back(envelope.clone());
        let entry = Arc::new(AgentRunEntry {
            transition: Mutex::new(()),
            state: Mutex::new(AgentRunState {
                snapshot: snapshot.clone(),
                sequence,
                journal,
            }),
            live,
            cancellation: cancellation.clone(),
            unavailable: CancellationToken::new(),
            permission: Mutex::new(PermissionState::default()),
            task: StdMutex::new(TaskState {
                monitor: None,
                worker_abort: None,
                attached: false,
                done: false,
                active_permit: Some(active_permit),
            }),
            task_done: AtomicBool::new(false),
            terminal_order: AtomicU64::new(0),
        });
        let mut runs = lock_std(&self.hub.inner.runs);
        if runs.contains_key(&snapshot.run_id) {
            return Err(agent_run_already_registered());
        }
        let _ = entry.live.send(envelope);
        runs.insert(
            snapshot.run_id.clone(),
            RegisteredEntry {
                entry,
                _entry_permit: entry_permit,
            },
        );
        Ok(RegisteredAgentRun {
            run_id: snapshot.run_id,
            cancellation,
        })
    }
}

impl RegisteredAgentRun {
    #[must_use]
    pub(crate) fn run_id(&self) -> &str {
        &self.run_id
    }

    #[must_use]
    pub(crate) fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }
}

impl DurableAgentTransition {
    #[must_use]
    pub(crate) const fn new(snapshot: AgentRunSnapshot, event: AgentEvent) -> Self {
        Self { snapshot, event }
    }
}

impl AgentTransitionFailure {
    #[must_use]
    pub(crate) const fn definitely_not_committed(error: AppError) -> Self {
        Self::DefinitelyNotCommitted(error)
    }

    #[must_use]
    pub(crate) const fn indeterminate(error: AppError) -> Self {
        Self::Indeterminate(error)
    }
}

impl AgentRunSubscription {
    /// Returns the next replayed or live event, or `None` after the durable
    /// terminal event has been observed.
    ///
    /// # Errors
    ///
    /// Returns a structured replay-window error if broadcast lag can no longer
    /// be recovered from the bounded journal, or stream-unavailable when the
    /// process-local entry was invalidated.
    pub async fn next_event(&mut self) -> Result<Option<AgentEventEnvelope>, AppError> {
        loop {
            if self.entry.unavailable.is_cancelled() {
                return Err(agent_event_stream_unavailable());
            }
            if let Some(event) = self.replay.pop_front() {
                self.cursor = event_sequence(&event);
                return Ok(Some(event));
            }

            {
                let state = self.entry.state.lock().await;
                if is_terminal(state.snapshot.status) && self.cursor >= state.sequence {
                    return Ok(None);
                }
            }

            let received = tokio::select! {
                biased;
                () = self.entry.unavailable.cancelled() => {
                    return Err(agent_event_stream_unavailable());
                }
                received = self.live.recv() => received,
            };
            match received {
                Ok(event) => {
                    let sequence = event_sequence(&event);
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
                        .filter(|event| event_sequence(event) > self.cursor)
                        .cloned()
                        .collect();
                }
            }
        }
    }
}

impl AgentPermissionWaiter {
    #[must_use]
    pub(crate) fn permission_id(&self) -> &str {
        &self.permission_id
    }

    /// Waits without a lost-wakeup window. A resolution that happened before
    /// the first poll is returned immediately.
    pub(crate) async fn wait(mut self) -> AgentPermissionWaitOutcome {
        loop {
            let signal = *self.receiver.borrow();
            match signal {
                PermissionSignalState::Waiting => {}
                PermissionSignalState::Resolved(status) => {
                    return AgentPermissionWaitOutcome::Resolved(status);
                }
                PermissionSignalState::Cancelled => {
                    return AgentPermissionWaitOutcome::Cancelled;
                }
            }
            tokio::select! {
                biased;
                () = self.cancellation.cancelled() => {
                    return AgentPermissionWaitOutcome::Cancelled;
                }
                changed = self.receiver.changed() => {
                    if changed.is_err() {
                        return AgentPermissionWaitOutcome::Cancelled;
                    }
                }
            }
        }
    }
}

impl AgentRunEntry {
    fn eviction_order(&self) -> Option<u64> {
        let terminal_order = self.terminal_order.load(AtomicOrdering::Acquire);
        (terminal_order != 0 && self.task_done.load(AtomicOrdering::Acquire))
            .then_some(terminal_order)
    }
}

async fn install_permission_signal(
    entry: &AgentRunEntry,
    permission_id: &str,
) -> Result<AgentPermissionWaiter, AppError> {
    let mut permission = entry.permission.lock().await;
    if let Some(current) = permission.current.as_ref() {
        if current.permission_id == permission_id {
            return Ok(AgentPermissionWaiter {
                permission_id: permission_id.to_owned(),
                receiver: current.sender.subscribe(),
                cancellation: entry.cancellation.clone(),
            });
        }
        if *current.sender.borrow() == PermissionSignalState::Waiting {
            return Err(permission_waiter_busy());
        }
    }
    let initial = if entry.cancellation.is_cancelled() {
        PermissionSignalState::Cancelled
    } else {
        PermissionSignalState::Waiting
    };
    let (sender, receiver) = watch::channel(initial);
    permission.current = Some(PermissionSignal {
        permission_id: permission_id.to_owned(),
        sender,
    });
    Ok(AgentPermissionWaiter {
        permission_id: permission_id.to_owned(),
        receiver,
        cancellation: entry.cancellation.clone(),
    })
}

async fn settle_permission(
    entry: &AgentRunEntry,
    permission_id: &str,
    resolution: PermissionSignalState,
) -> Result<bool, AppError> {
    let snapshot = entry.state.lock().await.snapshot.clone();
    if is_terminal(snapshot.status) {
        return Err(agent_run_already_terminal());
    }
    if let Some(pending) = snapshot.pending_permission
        && pending.permission_id != permission_id
    {
        return Err(permission_waiter_mismatch());
    }
    let mut permission = entry.permission.lock().await;
    if let Some(current) = permission.current.as_ref() {
        if current.permission_id == permission_id {
            if *current.sender.borrow() != PermissionSignalState::Waiting {
                return Ok(false);
            }
            current.sender.send_replace(resolution);
            return Ok(true);
        }
        if *current.sender.borrow() == PermissionSignalState::Waiting {
            return Err(permission_waiter_mismatch());
        }
    }
    let (sender, _receiver) = watch::channel(resolution);
    permission.current = Some(PermissionSignal {
        permission_id: permission_id.to_owned(),
        sender,
    });
    Ok(true)
}

async fn cancel_current_permission(entry: &AgentRunEntry) {
    let permission = entry.permission.lock().await;
    if let Some(current) = permission.current.as_ref()
        && *current.sender.borrow() == PermissionSignalState::Waiting
    {
        current
            .sender
            .send_replace(PermissionSignalState::Cancelled);
    }
}

fn validate_started_snapshot(snapshot: &AgentRunSnapshot) -> Result<u64, AppError> {
    let sequence = parse_snapshot_sequence(snapshot)?;
    if snapshot.run_id.is_empty()
        || snapshot.session_id.is_empty()
        || snapshot.status != AgentRunStatus::Running
        || sequence != 1
        || snapshot.pending_permission.is_some()
        || snapshot.message_id.is_some()
        || snapshot.error.is_some()
    {
        return Err(AppError::internal());
    }
    Ok(sequence)
}

fn validate_durable_transition(
    run_id: &str,
    prior: &AgentRunSnapshot,
    expected_sequence: u64,
    durable: &DurableAgentTransition,
) -> Result<(), AppError> {
    let snapshot = &durable.snapshot;
    if snapshot.run_id != run_id
        || snapshot.session_id != prior.session_id
        || parse_snapshot_sequence(snapshot)? != expected_sequence
        || matches!(&durable.event, AgentEvent::Started)
        || !event_matches_snapshot(&durable.event, snapshot, prior)
    {
        return Err(AppError::internal());
    }
    Ok(())
}

fn event_matches_snapshot(
    event: &AgentEvent,
    snapshot: &AgentRunSnapshot,
    prior: &AgentRunSnapshot,
) -> bool {
    match event {
        AgentEvent::Started => false,
        AgentEvent::PermissionRequested { permission } => {
            snapshot.status == AgentRunStatus::WaitingForPermission
                && permission.run_id == snapshot.run_id
                && snapshot.pending_permission.as_ref() == Some(permission)
        }
        AgentEvent::PermissionResolved { permission_id, .. } => {
            // The external contract resumes immediately and clears the prompt.
            // SQLite intentionally remains waiting until approval is consumed
            // and the exact write-dispatch fence has been installed.
            prior
                .pending_permission
                .as_ref()
                .is_some_and(|permission| permission.permission_id == permission_id.as_str())
                && snapshot.status == AgentRunStatus::Running
                && snapshot.pending_permission.is_none()
        }
        AgentEvent::Usage { usage } => !is_terminal(snapshot.status) && &snapshot.usage == usage,
        AgentEvent::Completed { message_id } => {
            snapshot.status == AgentRunStatus::Completed
                && snapshot.message_id.as_deref() == Some(message_id.as_str())
                && snapshot.error.is_none()
        }
        AgentEvent::Failed { error } => {
            snapshot.status == AgentRunStatus::Failed
                && snapshot.error.as_ref() == Some(error)
                && snapshot.message_id.is_none()
        }
        AgentEvent::Cancelled { .. } => snapshot.status == AgentRunStatus::Cancelled,
        AgentEvent::TextDelta { .. }
        | AgentEvent::ToolStarted { .. }
        | AgentEvent::ToolCompleted { .. }
        | AgentEvent::ToolFailed { .. }
        | AgentEvent::ContextCompacted { .. } => !is_terminal(snapshot.status),
    }
}

fn parse_snapshot_sequence(snapshot: &AgentRunSnapshot) -> Result<u64, AppError> {
    snapshot
        .last_sequence
        .parse()
        .map_err(|_| AppError::internal())
}

fn event_sequence(event: &AgentEventEnvelope) -> u64 {
    event
        .sequence
        .parse()
        .expect("agent event sequence is generated from u64")
}

const fn is_terminal(status: AgentRunStatus) -> bool {
    matches!(
        status,
        AgentRunStatus::Completed | AgentRunStatus::Failed | AgentRunStatus::Cancelled
    )
}

fn validate_replay_cursor(state: &AgentRunState, requested: Option<u64>) -> Result<(), AppError> {
    let Some(requested) = requested else {
        return Ok(());
    };
    if requested > state.sequence {
        return Err(AppError::invalid(
            "invalid_agent_sequence",
            "The requested agent event sequence is ahead of the run",
        ));
    }
    let oldest = state.journal.front().map_or(state.sequence, event_sequence);
    if requested.saturating_add(1) < oldest {
        return Err(agent_replay_window(requested, oldest, state.sequence));
    }
    Ok(())
}

fn agent_replay_window(requested: u64, oldest: u64, latest: u64) -> AppError {
    AppError::new(
        AppErrorKind::Conflict,
        ApiError {
            code: "agent_replay_window_expired".to_owned(),
            message: "The requested agent event is no longer retained".to_owned(),
            retryable: false,
            details: Some(ApiErrorDetails::ReplayWindow {
                requested_sequence: requested.to_string(),
                oldest_available_sequence: oldest.to_string(),
                latest_sequence: latest.to_string(),
            }),
        },
    )
}

fn agent_event_stream_unavailable() -> AppError {
    AppError::new(
        AppErrorKind::Unavailable,
        ApiError::new(
            "agent_event_stream_unavailable",
            "Live events for this agent run are unavailable; use the durable run snapshot",
        ),
    )
}

fn run_capacity_exhausted() -> AppError {
    let mut error = ApiError::new(
        "agent_run_capacity_exhausted",
        "Too many agent runs are active or retained; wait for one to finish",
    );
    error.retryable = true;
    AppError::new(AppErrorKind::ResourceExhausted, error)
}

fn agent_run_already_registered() -> AppError {
    AppError::new(
        AppErrorKind::Conflict,
        ApiError::new(
            "agent_run_already_registered",
            "The agent run is already registered in this process",
        ),
    )
}

fn agent_run_already_terminal() -> AppError {
    AppError::new(
        AppErrorKind::Conflict,
        ApiError::new(
            "agent_run_already_terminal",
            "The agent run is already terminal",
        ),
    )
}

fn permission_waiter_busy() -> AppError {
    AppError::new(
        AppErrorKind::Conflict,
        ApiError::new(
            "agent_permission_waiter_busy",
            "A different permission is already waiting for this agent run",
        ),
    )
}

fn permission_waiter_mismatch() -> AppError {
    AppError::new(
        AppErrorKind::Conflict,
        ApiError::new(
            "agent_permission_waiter_mismatch",
            "The permission does not match the active agent waiter",
        ),
    )
}

fn agent_task_already_attached() -> AppError {
    AppError::new(
        AppErrorKind::Conflict,
        ApiError::new(
            "agent_task_already_attached",
            "The agent run already owns a worker task",
        ),
    )
}

fn lock_std<T>(mutex: &StdMutex<T>) -> StdMutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::{
        future::pending,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::Duration,
    };

    use chat2db_contract::{
        AgentEvent, AgentPermissionRequest, AgentPermissionStatus, AgentRunSnapshot,
        AgentRunStatus, AgentUsage, ApiError, ApiErrorDetails,
    };
    use tokio::sync::oneshot;

    use crate::{AppError, AppErrorKind};

    use super::{
        AgentPermissionWaitOutcome, AgentRunHub, AgentRunReservation, AgentTransitionFailure,
        DurableAgentTransition,
    };

    fn snapshot(run_id: &str, status: AgentRunStatus, sequence: u64) -> AgentRunSnapshot {
        AgentRunSnapshot {
            run_id: run_id.to_owned(),
            session_id: "session-1".to_owned(),
            status,
            last_sequence: sequence.to_string(),
            started_at_ms: "1784900000000".to_owned(),
            updated_at_ms: (1_784_900_000_000_u64 + sequence).to_string(),
            model_rounds: "0".to_owned(),
            tool_calls: "0".to_owned(),
            usage: AgentUsage::default(),
            pending_permission: None,
            message_id: None,
            error: None,
        }
    }

    async fn register(hub: &AgentRunHub, run_id: &str) {
        hub.reserve()
            .await
            .expect("capacity reservation")
            .register_started(snapshot(run_id, AgentRunStatus::Running, 1))
            .expect("run registration");
    }

    async fn publish(
        hub: &AgentRunHub,
        run_id: &str,
        event: AgentEvent,
        status: AgentRunStatus,
    ) -> AgentRunSnapshot {
        let owned_run_id = run_id.to_owned();
        hub.transition(run_id, move |sequence| {
            let mut durable_snapshot = snapshot(&owned_run_id, status, sequence);
            match &event {
                AgentEvent::PermissionRequested { permission } => {
                    durable_snapshot.pending_permission = Some(permission.clone());
                }
                AgentEvent::Completed { message_id } => {
                    durable_snapshot.message_id = Some(message_id.clone());
                }
                AgentEvent::Failed { error } => durable_snapshot.error = Some(error.clone()),
                AgentEvent::Usage { usage } => durable_snapshot.usage.clone_from(usage),
                AgentEvent::Started
                | AgentEvent::TextDelta { .. }
                | AgentEvent::ToolStarted { .. }
                | AgentEvent::ToolCompleted { .. }
                | AgentEvent::ToolFailed { .. }
                | AgentEvent::PermissionResolved { .. }
                | AgentEvent::ContextCompacted { .. }
                | AgentEvent::Cancelled { .. } => {}
            }
            async move { Ok(DurableAgentTransition::new(durable_snapshot, event)) }
        })
        .await
        .expect("durable transition")
    }

    fn text_event(text: &str) -> AgentEvent {
        AgentEvent::TextDelta {
            delta: text.to_owned(),
        }
    }

    fn permission(run_id: &str, permission_id: &str) -> AgentPermissionRequest {
        AgentPermissionRequest {
            permission_id: permission_id.to_owned(),
            run_id: run_id.to_owned(),
            tool_call_id: "tool-call-1".to_owned(),
            tool_name: "execute_sql_write".to_owned(),
            arguments_sha256: "00".repeat(32),
            summary: "Update one row".to_owned(),
            requested_at_ms: "1784900000001".to_owned(),
            expires_at_ms: "1784900060001".to_owned(),
        }
    }

    async fn reserve_eventually(hub: &AgentRunHub) -> AgentRunReservation {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Ok(reservation) = hub.reserve().await {
                    return reservation;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("capacity must eventually be released")
    }

    #[tokio::test]
    async fn subscription_replays_then_follows_live_events() {
        let hub = AgentRunHub::with_capacities(8, 2, 4, 4);
        register(&hub, "run-1").await;
        publish(
            &hub,
            "run-1",
            text_event("replayed"),
            AgentRunStatus::Running,
        )
        .await;
        let mut subscription = hub.subscribe("run-1", Some(1)).await.expect("subscription");
        publish(
            &hub,
            "run-1",
            AgentEvent::Usage {
                usage: AgentUsage {
                    input_tokens: "3".to_owned(),
                    output_tokens: "2".to_owned(),
                    total_tokens: "5".to_owned(),
                },
            },
            AgentRunStatus::Running,
        )
        .await;

        let replayed = subscription
            .next_event()
            .await
            .expect("replayed event")
            .expect("replayed event exists");
        assert_eq!(replayed.sequence, "2");
        assert!(matches!(replayed.event, AgentEvent::TextDelta { .. }));
        let live = subscription
            .next_event()
            .await
            .expect("live event")
            .expect("live event exists");
        assert_eq!(live.sequence, "3");
        assert!(matches!(live.event, AgentEvent::Usage { .. }));
    }

    #[tokio::test]
    async fn broadcast_lag_recovers_from_the_larger_journal() {
        let hub = AgentRunHub::with_capacities(8, 1, 4, 4);
        register(&hub, "run-lag").await;
        let mut subscription = hub
            .subscribe("run-lag", Some(1))
            .await
            .expect("subscription");
        for index in 2..=6 {
            publish(
                &hub,
                "run-lag",
                text_event(&format!("event-{index}")),
                AgentRunStatus::Running,
            )
            .await;
        }

        for expected in 2..=6 {
            let event = subscription
                .next_event()
                .await
                .expect("lag recovery")
                .expect("event exists");
            assert_eq!(event.sequence, expected.to_string());
        }
    }

    #[tokio::test]
    async fn stale_and_forward_cursors_have_stable_errors() {
        let hub = AgentRunHub::with_capacities(2, 1, 4, 4);
        register(&hub, "run-cursor").await;
        for index in 0..3 {
            publish(
                &hub,
                "run-cursor",
                text_event(&format!("event-{index}")),
                AgentRunStatus::Running,
            )
            .await;
        }

        let stale = hub
            .subscribe("run-cursor", Some(1))
            .await
            .expect_err("cursor must be outside replay retention");
        assert_eq!(stale.api_error().code, "agent_replay_window_expired");
        assert_eq!(stale.kind(), AppErrorKind::Conflict);
        assert!(matches!(
            stale.api_error().details,
            Some(ApiErrorDetails::ReplayWindow {
                requested_sequence,
                oldest_available_sequence,
                latest_sequence,
            }) if requested_sequence == "1"
                && oldest_available_sequence == "3"
                && latest_sequence == "4"
        ));

        let forward = hub
            .subscribe("run-cursor", Some(5))
            .await
            .expect_err("forward cursor must fail");
        assert_eq!(forward.api_error().code, "invalid_agent_sequence");
        assert_eq!(forward.kind(), AppErrorKind::InvalidRequest);
    }

    #[tokio::test]
    async fn terminal_transition_is_durable_first_and_does_not_hold_state_lock() {
        let hub = AgentRunHub::with_capacities(4, 2, 2, 2);
        register(&hub, "run-terminal").await;
        let mut subscription = hub
            .subscribe("run-terminal", Some(1))
            .await
            .expect("subscription");
        let observed_hub = hub.clone();
        let error = hub
            .transition("run-terminal", move |_sequence| async move {
                let cached = observed_hub
                    .cached_snapshot("run-terminal")
                    .await
                    .expect("state lock remains available during persistence");
                assert_eq!(cached.last_sequence, "1");
                Err(AgentTransitionFailure::definitely_not_committed(
                    AppError::unavailable("sqlite_busy", "SQLite is busy"),
                ))
            })
            .await
            .expect_err("failed persistence must be invisible");
        assert_eq!(error.api_error().code, "sqlite_busy");
        assert_eq!(
            hub.cached_snapshot("run-terminal")
                .await
                .expect("cached snapshot")
                .last_sequence,
            "1"
        );

        publish(
            &hub,
            "run-terminal",
            AgentEvent::Completed {
                message_id: "message-1".to_owned(),
            },
            AgentRunStatus::Completed,
        )
        .await;
        let completed = subscription
            .next_event()
            .await
            .expect("completed event")
            .expect("completed event exists");
        assert_eq!(completed.sequence, "2");
        assert!(matches!(completed.event, AgentEvent::Completed { .. }));
        assert!(
            subscription
                .next_event()
                .await
                .expect("clean end")
                .is_none()
        );
        assert_eq!(
            hub.install_permission_waiter("run-terminal", "permission-too-late")
                .await
                .expect_err("terminal run rejects a new waiter")
                .api_error()
                .code,
            "agent_run_already_terminal"
        );
    }

    #[tokio::test]
    async fn indeterminate_transition_invalidates_sequence_capacity_and_existing_streams() {
        let hub = AgentRunHub::with_capacities(4, 2, 1, 1);
        let registered = hub
            .reserve()
            .await
            .expect("reservation")
            .register_started(snapshot("run-unknown", AgentRunStatus::Running, 1))
            .expect("registration");
        let mut subscription = hub
            .subscribe("run-unknown", Some(1))
            .await
            .expect("subscription");

        let failure = hub
            .transition("run-unknown", |_sequence| async {
                Err(AgentTransitionFailure::indeterminate(
                    AppError::unavailable(
                        "storage_outcome_unknown",
                        "The durable outcome is unknown",
                    ),
                ))
            })
            .await
            .expect_err("unknown commit status invalidates the journal");
        assert_eq!(failure.api_error().code, "storage_outcome_unknown");
        assert!(registered.cancellation_token().is_cancelled());
        assert_eq!(
            hub.cached_snapshot("run-unknown")
                .await
                .expect_err("invalidated sequence cannot be reused")
                .api_error()
                .code,
            "agent_event_stream_unavailable"
        );
        let stream_error = subscription
            .next_event()
            .await
            .expect_err("existing stream is explicitly closed");
        assert_eq!(
            stream_error.api_error().code,
            "agent_event_stream_unavailable"
        );
        assert!(!stream_error.api_error().retryable);

        hub.reserve()
            .await
            .expect("invalidation releases entry and active capacity");
    }

    #[tokio::test]
    async fn queued_transition_rechecks_an_invalidated_entry_after_locking() {
        let hub = AgentRunHub::with_capacities(4, 2, 1, 1);
        register(&hub, "run-queued-invalidation").await;
        let entry = hub
            .entry("run-queued-invalidation")
            .expect("registered entry");
        let transition_guard = entry.transition.lock().await;
        let persist_invoked = Arc::new(AtomicBool::new(false));
        let observed_invocation = Arc::clone(&persist_invoked);
        let mut queued = Box::pin(hub.transition("run-queued-invalidation", move |sequence| {
            observed_invocation.store(true, Ordering::SeqCst);
            async move {
                Ok(DurableAgentTransition::new(
                    snapshot("run-queued-invalidation", AgentRunStatus::Running, sequence),
                    text_event("must not persist"),
                ))
            }
        }));
        tokio::select! {
            biased;
            result = &mut queued => panic!("transition unexpectedly completed: {result:?}"),
            () = tokio::task::yield_now() => {}
        }

        hub.invalidate_entry("run-queued-invalidation", &entry, false);
        drop(transition_guard);
        let error = queued
            .await
            .expect_err("queued transition must observe invalidation");
        assert_eq!(error.api_error().code, "agent_event_stream_unavailable");
        assert!(!persist_invoked.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn dropping_an_in_flight_persist_invalidates_before_a_queued_transition_runs() {
        let hub = AgentRunHub::with_capacities(4, 2, 1, 1);
        register(&hub, "run-dropped-persist").await;
        let (persist_started, wait_for_persist) = oneshot::channel::<()>();
        let first_hub = hub.clone();
        let first = tokio::spawn(async move {
            first_hub
                .transition("run-dropped-persist", move |_sequence| async move {
                    persist_started.send(()).expect("signal persist start");
                    pending::<Result<DurableAgentTransition, AgentTransitionFailure>>().await
                })
                .await
        });
        wait_for_persist.await.expect("persist starts");

        let persist_invoked = Arc::new(AtomicBool::new(false));
        let observed_invocation = Arc::clone(&persist_invoked);
        let mut queued = Box::pin(hub.transition("run-dropped-persist", move |sequence| {
            observed_invocation.store(true, Ordering::SeqCst);
            async move {
                Ok(DurableAgentTransition::new(
                    snapshot("run-dropped-persist", AgentRunStatus::Running, sequence),
                    text_event("must not persist"),
                ))
            }
        }));
        tokio::select! {
            biased;
            result = &mut queued => panic!("transition unexpectedly completed: {result:?}"),
            () = tokio::task::yield_now() => {}
        }

        first.abort();
        let join_error = first.await.expect_err("first transition is aborted");
        assert!(join_error.is_cancelled());
        let error = queued
            .await
            .expect_err("dropped persistence poisons the local sequence");
        assert_eq!(error.api_error().code, "agent_event_stream_unavailable");
        assert!(!persist_invoked.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn terminal_entry_is_evictable_only_after_its_task_exits() {
        let hub = AgentRunHub::with_capacities(4, 2, 1, 2);
        register(&hub, "run-old").await;
        let (release_task, wait_for_release) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = wait_for_release.await;
        });
        hub.attach_task("run-old", task).expect("task attachment");
        publish(
            &hub,
            "run-old",
            AgentEvent::Cancelled { reason: None },
            AgentRunStatus::Cancelled,
        )
        .await;

        let error = hub
            .reserve()
            .await
            .expect_err("running task prevents terminal eviction");
        assert_eq!(error.api_error().code, "agent_run_capacity_exhausted");
        release_task.send(()).expect("release task");
        let reservation = reserve_eventually(&hub).await;
        reservation
            .register_started(snapshot("run-new", AgentRunStatus::Running, 1))
            .expect("replacement registration");

        let unavailable = hub
            .subscribe("run-old", None)
            .await
            .expect_err("evicted run has no process-local journal");
        assert_eq!(
            unavailable.api_error().code,
            "agent_event_stream_unavailable"
        );
        assert_eq!(unavailable.kind(), AppErrorKind::Unavailable);
        assert!(!unavailable.api_error().retryable);
    }

    #[tokio::test]
    async fn active_permit_is_released_only_after_the_owned_task_exits() {
        let hub = AgentRunHub::with_capacities(4, 2, 2, 1);
        register(&hub, "run-task").await;
        let (release_task, wait_for_release) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = wait_for_release.await;
        });
        hub.attach_task("run-task", task).expect("task attachment");

        assert_eq!(
            hub.reserve()
                .await
                .expect_err("active permit remains owned")
                .api_error()
                .code,
            "agent_run_capacity_exhausted"
        );
        release_task.send(()).expect("release task");
        let reservation = reserve_eventually(&hub).await;
        reservation
            .register_started(snapshot("run-after-task", AgentRunStatus::Running, 1))
            .expect("new task is admitted");
    }

    async fn assert_nonterminal_worker_exit_invalidates(mode: &str) {
        let hub = AgentRunHub::with_capacities(4, 2, 1, 1);
        let run_id = format!("run-worker-{mode}");
        let registered = hub
            .reserve()
            .await
            .expect("reservation")
            .register_started(snapshot(&run_id, AgentRunStatus::Running, 1))
            .expect("registration");
        let mut subscription = hub.subscribe(&run_id, Some(1)).await.expect("subscription");
        let (task, abort) = match mode {
            "normal" => (tokio::spawn(async {}), None),
            "panic" => (
                tokio::spawn(async {
                    panic!("intentional worker panic");
                }),
                None,
            ),
            "abort" => {
                let task = tokio::spawn(pending());
                let abort = task.abort_handle();
                (task, Some(abort))
            }
            _ => panic!("unknown worker mode"),
        };
        hub.attach_task(&run_id, task).expect("task attachment");
        if let Some(abort) = abort {
            abort.abort();
        }

        let error = tokio::time::timeout(Duration::from_secs(1), subscription.next_event())
            .await
            .expect("subscriber must be woken")
            .expect_err("nonterminal worker exit invalidates its stream");
        assert_eq!(error.api_error().code, "agent_event_stream_unavailable");
        assert!(registered.cancellation_token().is_cancelled());
        hub.reserve()
            .await
            .expect("worker exit releases both bounded capacities");
    }

    #[tokio::test]
    async fn every_nonterminal_worker_exit_invalidates_its_entry() {
        for mode in ["normal", "panic", "abort"] {
            assert_nonterminal_worker_exit_invalidates(mode).await;
        }
    }

    #[tokio::test]
    async fn dropping_a_subscriber_never_cancels_the_run() {
        let hub = AgentRunHub::with_capacities(4, 2, 2, 2);
        let registered = hub
            .reserve()
            .await
            .expect("reservation")
            .register_started(snapshot("run-drop", AgentRunStatus::Running, 1))
            .expect("registration");
        assert_eq!(registered.run_id(), "run-drop");
        let cancellation = registered.cancellation_token();
        let subscription = hub.subscribe("run-drop", None).await.expect("subscription");

        drop(subscription);
        tokio::task::yield_now().await;
        assert!(!cancellation.is_cancelled());
        assert!(
            hub.request_cancellation("run-drop")
                .await
                .expect("cancellation request")
        );
        assert!(cancellation.is_cancelled());
    }

    #[tokio::test]
    async fn durable_permission_resolution_can_precede_signal_installation() {
        let hub = AgentRunHub::with_capacities(4, 2, 2, 2);
        register(&hub, "run-early-resolution").await;

        assert!(
            hub.resolve_permission(
                "run-early-resolution",
                "permission-1",
                AgentPermissionStatus::Approved,
            )
            .await
            .expect("durable resolution is retained before event publication")
        );
        publish(
            &hub,
            "run-early-resolution",
            AgentEvent::PermissionRequested {
                permission: permission("run-early-resolution", "permission-1"),
            },
            AgentRunStatus::WaitingForPermission,
        )
        .await;
        let waiter = hub
            .install_permission_waiter("run-early-resolution", "permission-1")
            .await
            .expect("late waiter subscribes to retained resolution");
        assert_eq!(
            waiter.wait().await,
            AgentPermissionWaitOutcome::Resolved(AgentPermissionStatus::Approved)
        );

        publish(
            &hub,
            "run-early-resolution",
            AgentEvent::PermissionResolved {
                permission_id: "permission-1".to_owned(),
                status: AgentPermissionStatus::Approved,
            },
            AgentRunStatus::Running,
        )
        .await;
        assert!(
            hub.resolve_permission(
                "run-early-resolution",
                "permission-2",
                AgentPermissionStatus::Denied,
            )
            .await
            .expect("next durable resolution replaces a settled prior signal")
        );
        publish(
            &hub,
            "run-early-resolution",
            AgentEvent::PermissionRequested {
                permission: permission("run-early-resolution", "permission-2"),
            },
            AgentRunStatus::WaitingForPermission,
        )
        .await;
        let waiter = hub
            .install_permission_waiter("run-early-resolution", "permission-2")
            .await
            .expect("next late waiter subscribes to retained resolution");
        assert_eq!(
            waiter.wait().await,
            AgentPermissionWaitOutcome::Resolved(AgentPermissionStatus::Denied)
        );
    }

    #[tokio::test]
    async fn permission_resolution_and_cancellation_are_bound_and_lossless() {
        let hub = AgentRunHub::with_capacities(4, 2, 2, 2);
        register(&hub, "run-permission").await;
        publish(
            &hub,
            "run-permission",
            AgentEvent::PermissionRequested {
                permission: permission("run-permission", "permission-1"),
            },
            AgentRunStatus::WaitingForPermission,
        )
        .await;
        assert!(
            hub.resolve_permission(
                "run-permission",
                "permission-1",
                AgentPermissionStatus::Approved,
            )
            .await
            .expect("permission resolves before runner installs waiter")
        );
        let waiter = hub
            .install_permission_waiter("run-permission", "permission-1")
            .await
            .expect("runner subscribes after resolution");
        assert_eq!(waiter.permission_id(), "permission-1");
        assert_eq!(
            waiter.wait().await,
            AgentPermissionWaitOutcome::Resolved(AgentPermissionStatus::Approved)
        );
        assert!(
            !hub.resolve_permission(
                "run-permission",
                "permission-1",
                AgentPermissionStatus::Approved,
            )
            .await
            .expect("duplicate resolution is idempotent")
        );

        publish(
            &hub,
            "run-permission",
            AgentEvent::PermissionResolved {
                permission_id: "permission-1".to_owned(),
                status: AgentPermissionStatus::Approved,
            },
            AgentRunStatus::Running,
        )
        .await;
        publish(
            &hub,
            "run-permission",
            AgentEvent::PermissionRequested {
                permission: permission("run-permission", "permission-2"),
            },
            AgentRunStatus::WaitingForPermission,
        )
        .await;

        let racing_waiter = hub
            .install_permission_waiter("run-permission", "permission-2")
            .await
            .expect("next waiter installs");
        let resolving = hub.resolve_permission(
            "run-permission",
            "permission-2",
            AgentPermissionStatus::Denied,
        );
        let cancelling = hub.cancel_permission("run-permission", "permission-2");
        let (resolved, cancelled) = tokio::join!(resolving, cancelling);
        let resolved = resolved.expect("resolve race result");
        let cancelled = cancelled.expect("cancel race result");
        assert_ne!(resolved, cancelled, "exactly one race participant wins");
        let expected = if resolved {
            AgentPermissionWaitOutcome::Resolved(AgentPermissionStatus::Denied)
        } else {
            AgentPermissionWaitOutcome::Cancelled
        };
        assert_eq!(racing_waiter.wait().await, expected);

        let mismatch = hub
            .resolve_permission(
                "run-permission",
                "permission-stale",
                AgentPermissionStatus::Approved,
            )
            .await
            .expect_err("stale permission cannot wake current waiter");
        assert_eq!(
            mismatch.api_error().code,
            "agent_permission_waiter_mismatch"
        );
    }

    #[tokio::test]
    async fn approved_permission_projects_running_while_sqlite_keeps_its_fence_wait() {
        let hub = AgentRunHub::with_capacities(4, 2, 2, 2);
        register(&hub, "run-projection").await;
        publish(
            &hub,
            "run-projection",
            AgentEvent::PermissionRequested {
                permission: permission("run-projection", "permission-projection"),
            },
            AgentRunStatus::WaitingForPermission,
        )
        .await;

        // This is the external projection. The storage record deliberately stays
        // waiting until approval consumption atomically installs the write fence.
        let projected = publish(
            &hub,
            "run-projection",
            AgentEvent::PermissionResolved {
                permission_id: "permission-projection".to_owned(),
                status: AgentPermissionStatus::Approved,
            },
            AgentRunStatus::Running,
        )
        .await;
        assert_eq!(projected.status, AgentRunStatus::Running);
        assert!(projected.pending_permission.is_none());
        assert_eq!(projected.last_sequence, "3");
    }

    #[tokio::test]
    async fn cancellation_before_waiter_installation_is_observed() {
        let hub = AgentRunHub::with_capacities(4, 2, 2, 2);
        register(&hub, "run-cancelled").await;
        assert!(
            hub.request_cancellation("run-cancelled")
                .await
                .expect("cancellation request")
        );
        let waiter = hub
            .install_permission_waiter("run-cancelled", "permission-late")
            .await
            .expect("late waiter installs cancelled");
        assert_eq!(waiter.wait().await, AgentPermissionWaitOutcome::Cancelled);
    }

    #[tokio::test]
    async fn post_commit_validation_failure_invalidates_the_local_entry() {
        let hub = AgentRunHub::with_capacities(4, 2, 2, 2);
        register(&hub, "run-binding").await;
        let permission = permission("run-binding", "permission-binding");
        let durable = publish(
            &hub,
            "run-binding",
            AgentEvent::PermissionRequested {
                permission: permission.clone(),
            },
            AgentRunStatus::WaitingForPermission,
        )
        .await;
        assert_eq!(durable.pending_permission, Some(permission));
        let mut subscription = hub
            .subscribe("run-binding", Some(2))
            .await
            .expect("subscription");

        let error = hub
            .transition("run-binding", |sequence| async move {
                Ok(DurableAgentTransition::new(
                    snapshot("run-binding", AgentRunStatus::Failed, sequence),
                    AgentEvent::Failed {
                        error: ApiError::new("provider_failed", "Provider failed"),
                    },
                ))
            })
            .await
            .expect_err("post-commit snapshot must carry its durable error");
        assert_eq!(error.kind(), AppErrorKind::Internal);
        assert_eq!(
            hub.cached_snapshot("run-binding")
                .await
                .expect_err("post-commit local failure invalidates the entry")
                .api_error()
                .code,
            "agent_event_stream_unavailable"
        );
        assert_eq!(
            subscription
                .next_event()
                .await
                .expect_err("existing stream observes invalidation")
                .api_error()
                .code,
            "agent_event_stream_unavailable"
        );
    }

    #[tokio::test]
    async fn cancel_all_wakes_waiters_and_join_timeout_aborts_workers() {
        let hub = AgentRunHub::with_capacities(4, 2, 2, 1);
        let registered = hub
            .reserve()
            .await
            .expect("reservation")
            .register_started(snapshot("run-shutdown", AgentRunStatus::Running, 1))
            .expect("registration");
        let waiter = hub
            .install_permission_waiter("run-shutdown", "permission-shutdown")
            .await
            .expect("waiter installs");
        hub.attach_task("run-shutdown", tokio::spawn(pending()))
            .expect("task attachment");

        hub.cancel_all().await;
        assert!(registered.cancellation_token().is_cancelled());
        assert_eq!(waiter.wait().await, AgentPermissionWaitOutcome::Cancelled);
        hub.join_tasks(Duration::from_millis(10)).await;

        let reservation = reserve_eventually(&hub).await;
        reservation
            .register_started(snapshot("run-after-shutdown", AgentRunStatus::Running, 1))
            .expect("aborted worker released active capacity");
    }

    #[tokio::test]
    async fn shutdown_invalidates_a_registration_without_an_attached_task() {
        let hub = AgentRunHub::with_capacities(4, 2, 1, 1);
        let registered = hub
            .reserve()
            .await
            .expect("reservation")
            .register_started(snapshot("run-unattached", AgentRunStatus::Running, 1))
            .expect("registration");
        let mut subscription = hub
            .subscribe("run-unattached", Some(1))
            .await
            .expect("subscription");

        hub.cancel_all().await;
        hub.join_tasks(Duration::from_millis(10)).await;

        assert!(registered.cancellation_token().is_cancelled());
        assert_eq!(
            subscription
                .next_event()
                .await
                .expect_err("shutdown closes an unattached stream")
                .api_error()
                .code,
            "agent_event_stream_unavailable"
        );
        hub.reserve()
            .await
            .expect("shutdown releases unattached capacity");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn invalidated_attached_worker_remains_tracked_until_shutdown_forces_cleanup() {
        let hub = AgentRunHub::with_capacities(4, 2, 1, 1);
        register(&hub, "run-invalidated-worker").await;
        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let worker = tokio::task::spawn_blocking(move || {
            started_sender.send(()).expect("signal worker start");
            let _ = release_receiver.recv();
        });
        started_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("blocking worker starts");
        hub.attach_task("run-invalidated-worker", worker)
            .expect("task attachment");
        let entry = hub
            .entry("run-invalidated-worker")
            .expect("registered entry");
        hub.invalidate_entry("run-invalidated-worker", &entry, false);

        assert_eq!(
            hub.reserve()
                .await
                .expect_err("unabortable worker still owns active capacity")
                .api_error()
                .code,
            "agent_run_capacity_exhausted"
        );
        tokio::time::timeout(
            Duration::from_millis(200),
            hub.join_tasks(Duration::from_millis(10)),
        )
        .await
        .expect("shutdown retains and force-cleans the invalidated worker");
        hub.reserve()
            .await
            .expect("forced cleanup releases invalidated worker capacity");
        release_sender.send(()).expect("release blocking worker");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn join_timeout_is_a_hard_bound_even_for_an_unabortable_worker() {
        let hub = AgentRunHub::with_capacities(4, 2, 1, 1);
        register(&hub, "run-hard-timeout").await;
        let mut subscription = hub
            .subscribe("run-hard-timeout", Some(1))
            .await
            .expect("subscription");
        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let worker = tokio::task::spawn_blocking(move || {
            started_sender.send(()).expect("signal worker start");
            let _ = release_receiver.recv();
        });
        started_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("blocking worker starts");
        hub.attach_task("run-hard-timeout", worker)
            .expect("task attachment");

        tokio::time::timeout(
            Duration::from_millis(200),
            hub.join_tasks(Duration::from_millis(10)),
        )
        .await
        .expect("join_tasks must not await an unabortable worker after its deadline");
        hub.reserve()
            .await
            .expect("forced timeout cleanup releases capacity");
        assert_eq!(
            subscription
                .next_event()
                .await
                .expect_err("forced cleanup closes existing streams")
                .api_error()
                .code,
            "agent_event_stream_unavailable"
        );
        release_sender.send(()).expect("release blocking worker");
    }

    #[tokio::test]
    async fn fresh_registration_requires_started_sequence_one() {
        let hub = AgentRunHub::with_capacities(4, 2, 2, 1);
        let error = hub
            .reserve()
            .await
            .expect("reservation")
            .register_started(snapshot("run-invalid-start", AgentRunStatus::Running, 2))
            .expect_err("fresh registration cannot skip Started sequence one");
        assert_eq!(error.kind(), AppErrorKind::Internal);

        hub.reserve()
            .await
            .expect("failed registration releases permits")
            .register_started(snapshot("run-valid-start", AgentRunStatus::Running, 1))
            .expect("sequence one registers");
    }
}
