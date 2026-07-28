use std::{fmt, ops::Deref, sync::Arc, time::Duration};

use chat2db_contract::JdbcDriver;
use chat2db_java_bridge::{EngineClient, EngineConfig, EngineState, EngineSupervisor};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{AppError, driver_pack, driver_pack::PreparedDriverPacks};

pub(crate) const DEFAULT_ENGINE_IDLE_TIMEOUT: Duration = Duration::from_secs(3 * 60);
const DEFAULT_ENGINE_SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

type AcquireReply = oneshot::Sender<Result<EngineLease, AppError>>;
type ShutdownReply = oneshot::Sender<Result<(), AppError>>;

/// Synchronous lifecycle snapshot used by product health reporting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EngineManagerStatus {
    Idle,
    Starting,
    Ready(EngineState),
    Stopping {
        generation: u64,
        reason: EngineStopReason,
    },
    Failed(AppError),
    ShuttingDown,
    Stopped,
}

/// Why the manager stopped a ready engine generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EngineStopReason {
    Idle,
    Crashed,
    HostShutdown,
}

/// Application-facing engine source.
#[derive(Clone)]
pub(crate) enum EngineProvider {
    Disabled,
    Static(EngineClient),
    Managed(EngineManagerHandle),
}

impl EngineProvider {
    pub(crate) async fn acquire(&self) -> Result<EngineLease, AppError> {
        match self {
            Self::Disabled => Err(engine_not_configured()),
            Self::Static(client) => Ok(EngineLease::unmanaged(client.clone())),
            Self::Managed(manager) => manager.acquire().await,
        }
    }

    #[must_use]
    pub(crate) fn status(&self) -> Option<EngineManagerStatus> {
        match self {
            Self::Disabled => None,
            Self::Static(client) => Some(EngineManagerStatus::Ready(client.state())),
            Self::Managed(manager) => Some(manager.status()),
        }
    }

    #[must_use]
    pub(crate) const fn is_configured(&self) -> bool {
        !matches!(self, Self::Disabled)
    }

    #[must_use]
    pub(crate) fn community_compatibility_configured(&self) -> bool {
        match self {
            Self::Disabled => false,
            Self::Static(client) => client.community_compatibility_configured(),
            Self::Managed(manager) => manager.community_compatibility_configured,
        }
    }
}

impl fmt::Debug for EngineProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => formatter.write_str("Disabled"),
            Self::Static(client) => formatter
                .debug_tuple("Static")
                .field(&client.state())
                .finish(),
            Self::Managed(manager) => formatter
                .debug_tuple("Managed")
                .field(&manager.status())
                .finish(),
        }
    }
}

/// One logical claim on a specific compatibility-engine generation.
///
/// Clones share one release token, so the manager releases the claim only
/// after the last clone is dropped.
#[derive(Clone)]
pub struct EngineLease {
    client: EngineClient,
    release: Option<Arc<ReleaseOnDrop>>,
}

impl EngineLease {
    fn managed(
        client: EngineClient,
        generation: u64,
        commands: mpsc::UnboundedSender<ManagerCommand>,
    ) -> Self {
        Self {
            client,
            release: Some(Arc::new(ReleaseOnDrop {
                generation,
                commands,
            })),
        }
    }

    fn unmanaged(client: EngineClient) -> Self {
        Self {
            client,
            release: None,
        }
    }

    #[must_use]
    pub fn client(&self) -> &EngineClient {
        &self.client
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.client.state().generation()
    }
}

impl Deref for EngineLease {
    type Target = EngineClient;

    fn deref(&self) -> &Self::Target {
        self.client()
    }
}

impl AsRef<EngineClient> for EngineLease {
    fn as_ref(&self) -> &EngineClient {
        self.client()
    }
}

impl fmt::Debug for EngineLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EngineLease")
            .field("client", &self.client.state())
            .field(
                "release",
                &self.release.as_ref().map(|_| "[managed release token]"),
            )
            .finish()
    }
}

struct ReleaseOnDrop {
    generation: u64,
    commands: mpsc::UnboundedSender<ManagerCommand>,
}

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        let _ = self.commands.send(ManagerCommand::Release {
            generation: self.generation,
        });
    }
}

/// Cloneable request side of the engine lifecycle actor.
#[derive(Clone)]
pub(crate) struct EngineManagerHandle {
    commands: mpsc::UnboundedSender<ManagerCommand>,
    status: watch::Receiver<EngineManagerStatus>,
    community_compatibility_configured: bool,
}

impl EngineManagerHandle {
    pub(crate) async fn acquire(&self) -> Result<EngineLease, AppError> {
        let (reply, result) = oneshot::channel();
        self.commands
            .send(ManagerCommand::Acquire { reply })
            .map_err(|_| engine_manager_unavailable())?;
        result.await.map_err(|_| engine_manager_unavailable())?
    }

    #[must_use]
    pub(crate) fn status(&self) -> EngineManagerStatus {
        self.status.borrow().clone()
    }

    async fn shutdown(&self) -> Result<(), AppError> {
        let (reply, result) = oneshot::channel();
        if self
            .commands
            .send(ManagerCommand::Shutdown { reply })
            .is_err()
        {
            return if matches!(self.status(), EngineManagerStatus::Stopped) {
                Ok(())
            } else {
                Err(engine_manager_unavailable())
            };
        }
        result.await.map_err(|_| engine_manager_unavailable())?
    }
}

impl fmt::Debug for EngineManagerHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EngineManagerHandle")
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

/// Owns the lifecycle actor and the restartable managed-driver snapshot.
pub(crate) struct EngineManagerOwner {
    handle: EngineManagerHandle,
    task: Option<JoinHandle<()>>,
    inventory: Vec<JdbcDriver>,
}

impl EngineManagerOwner {
    #[must_use]
    pub(crate) fn with_idle_timeout(
        config: EngineConfig,
        drivers: PreparedDriverPacks,
        idle_timeout: Duration,
    ) -> Self {
        let community_compatibility_configured = config.community_compatibility_configured();
        let inventory = drivers.inventory();
        let drivers = Arc::new(drivers);
        let (commands, receiver) = mpsc::unbounded_channel();
        let (status_sender, status) = watch::channel(EngineManagerStatus::Idle);
        let handle = EngineManagerHandle {
            commands: commands.clone(),
            status,
            community_compatibility_configured,
        };
        let task = tokio::spawn(run_manager(ManagerContext {
            config,
            drivers,
            idle_timeout,
            commands,
            receiver,
            status: status_sender,
        }));
        Self {
            handle,
            task: Some(task),
            inventory,
        }
    }

    #[must_use]
    pub(crate) fn handle(&self) -> EngineManagerHandle {
        self.handle.clone()
    }

    #[must_use]
    pub(crate) fn inventory(&self) -> Vec<JdbcDriver> {
        self.inventory.clone()
    }

    pub(crate) async fn shutdown(&mut self) -> Result<(), AppError> {
        let shutdown = self.handle.shutdown().await;
        let joined = match self.task.take() {
            Some(task) => task.await.map_err(|error| {
                tracing::error!(%error, "engine manager task failed to join");
                AppError::internal()
            }),
            None => Ok(()),
        };
        match shutdown {
            Err(error) => Err(error),
            Ok(()) => joined,
        }
    }
}

impl fmt::Debug for EngineManagerOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EngineManagerOwner")
            .field("status", &self.handle.status())
            .field("driver_count", &self.inventory.len())
            .finish_non_exhaustive()
    }
}

impl Drop for EngineManagerOwner {
    fn drop(&mut self) {
        if self.task.is_some() {
            let _ = self.handle.commands.send(ManagerCommand::ForceShutdown);
        }
    }
}

struct ManagerContext {
    config: EngineConfig,
    drivers: Arc<PreparedDriverPacks>,
    idle_timeout: Duration,
    commands: mpsc::UnboundedSender<ManagerCommand>,
    receiver: mpsc::UnboundedReceiver<ManagerCommand>,
    status: watch::Sender<EngineManagerStatus>,
}

#[allow(clippy::large_enum_variant)]
enum ManagerCommand {
    Acquire {
        reply: AcquireReply,
    },
    Release {
        generation: u64,
    },
    StartFinished {
        attempt: u64,
        result: Result<EngineSupervisor, AppError>,
    },
    IdleExpired {
        generation: u64,
        epoch: u64,
    },
    DrainExpired {
        generation: u64,
        epoch: u64,
    },
    GenerationTerminal {
        generation: u64,
        state: EngineState,
    },
    StopFinished {
        generation: u64,
        reason: EngineStopReason,
        result: Result<(), AppError>,
    },
    Shutdown {
        reply: ShutdownReply,
    },
    ForceShutdown,
}

#[allow(clippy::large_enum_variant)]
enum ManagerState {
    Cold,
    Starting {
        attempt: u64,
        waiters: Vec<AcquireReply>,
        cancellation: CancellationToken,
    },
    Running(RunningGeneration),
    Stopping {
        generation: u64,
        reason: EngineStopReason,
        waiters: Vec<AcquireReply>,
    },
    Closed,
}

struct RunningGeneration {
    generation: u64,
    supervisor: EngineSupervisor,
    leases: usize,
    idle_epoch: u64,
}

#[allow(clippy::too_many_lines)]
async fn run_manager(mut context: ManagerContext) {
    let mut state = ManagerState::Cold;
    let mut next_attempt = 1_u64;
    let mut drain_epoch = 0_u64;
    let mut shutting_down = false;
    let mut shutdown_replies = Vec::<ShutdownReply>::new();

    while let Some(command) = context.receiver.recv().await {
        match command {
            ManagerCommand::Acquire { reply } => {
                if shutting_down {
                    let _ = reply.send(Err(runtime_shutting_down()));
                    continue;
                }
                state = match state {
                    ManagerState::Cold => {
                        let attempt = next_attempt;
                        next_attempt = next_attempt.wrapping_add(1).max(1);
                        let cancellation = CancellationToken::new();
                        context.status.send_replace(EngineManagerStatus::Starting);
                        spawn_start(
                            attempt,
                            context.config.clone(),
                            Arc::clone(&context.drivers),
                            cancellation.clone(),
                            context.commands.clone(),
                        );
                        ManagerState::Starting {
                            attempt,
                            waiters: vec![reply],
                            cancellation,
                        }
                    }
                    ManagerState::Starting {
                        attempt,
                        mut waiters,
                        cancellation,
                    } => {
                        waiters.push(reply);
                        ManagerState::Starting {
                            attempt,
                            waiters,
                            cancellation,
                        }
                    }
                    ManagerState::Running(mut running) => {
                        issue_lease(&mut running, reply, &context.commands);
                        ManagerState::Running(running)
                    }
                    ManagerState::Stopping {
                        generation,
                        reason,
                        mut waiters,
                    } => {
                        waiters.push(reply);
                        ManagerState::Stopping {
                            generation,
                            reason,
                            waiters,
                        }
                    }
                    ManagerState::Closed => {
                        let _ = reply.send(Err(engine_manager_unavailable()));
                        ManagerState::Closed
                    }
                };
            }
            ManagerCommand::Release { generation } => {
                state = match state {
                    ManagerState::Running(mut running)
                        if running.generation == generation && running.leases > 0 =>
                    {
                        running.leases -= 1;
                        if running.leases == 0 {
                            running.idle_epoch = running.idle_epoch.wrapping_add(1);
                            if shutting_down {
                                begin_stop(
                                    running,
                                    EngineStopReason::HostShutdown,
                                    context.commands.clone(),
                                    Vec::new(),
                                    &context.status,
                                )
                            } else {
                                schedule_idle(
                                    generation,
                                    running.idle_epoch,
                                    context.idle_timeout,
                                    context.commands.clone(),
                                );
                                ManagerState::Running(running)
                            }
                        } else {
                            ManagerState::Running(running)
                        }
                    }
                    other => other,
                };
            }
            ManagerCommand::StartFinished { attempt, result } => {
                let ManagerState::Starting {
                    attempt: active_attempt,
                    waiters,
                    cancellation: _,
                } = state
                else {
                    if let Ok(supervisor) = result {
                        spawn_stop(
                            supervisor,
                            EngineStopReason::HostShutdown,
                            context.commands.clone(),
                            false,
                        );
                    }
                    continue;
                };
                if active_attempt != attempt {
                    fail_waiters(waiters, &engine_manager_unavailable());
                    if let Ok(supervisor) = result {
                        spawn_stop(
                            supervisor,
                            EngineStopReason::HostShutdown,
                            context.commands.clone(),
                            false,
                        );
                    }
                    state = ManagerState::Cold;
                    continue;
                }

                match result {
                    Ok(supervisor) if shutting_down => {
                        fail_waiters(waiters, &runtime_shutting_down());
                        let generation = supervisor.state().generation();
                        context
                            .status
                            .send_replace(EngineManagerStatus::ShuttingDown);
                        spawn_stop(
                            supervisor,
                            EngineStopReason::HostShutdown,
                            context.commands.clone(),
                            false,
                        );
                        state = ManagerState::Stopping {
                            generation,
                            reason: EngineStopReason::HostShutdown,
                            waiters: Vec::new(),
                        };
                    }
                    Ok(supervisor) => {
                        let engine_state = supervisor.state();
                        let generation = engine_state.generation();
                        spawn_terminal_watcher(
                            generation,
                            supervisor.subscribe_state(),
                            context.commands.clone(),
                        );
                        context
                            .status
                            .send_replace(EngineManagerStatus::Ready(engine_state));
                        let mut running = RunningGeneration {
                            generation,
                            supervisor,
                            leases: 0,
                            idle_epoch: 0,
                        };
                        for waiter in waiters {
                            issue_lease(&mut running, waiter, &context.commands);
                        }
                        if running.leases == 0 {
                            running.idle_epoch = running.idle_epoch.wrapping_add(1);
                            schedule_idle(
                                generation,
                                running.idle_epoch,
                                context.idle_timeout,
                                context.commands.clone(),
                            );
                        }
                        state = ManagerState::Running(running);
                    }
                    Err(error) if shutting_down => {
                        tracing::warn!(%error, "engine startup failed while the host was shutting down");
                        fail_waiters(waiters, &runtime_shutting_down());
                        state = ManagerState::Closed;
                        finish_shutdown(&context.status, &mut shutdown_replies, &Ok(()));
                        break;
                    }
                    Err(error) => {
                        fail_waiters(waiters, &error);
                        context
                            .status
                            .send_replace(EngineManagerStatus::Failed(error));
                        state = ManagerState::Cold;
                    }
                }
            }
            ManagerCommand::IdleExpired { generation, epoch } => {
                state = match state {
                    ManagerState::Running(running)
                        if !shutting_down
                            && running.generation == generation
                            && running.idle_epoch == epoch
                            && running.leases == 0 =>
                    {
                        begin_stop(
                            running,
                            EngineStopReason::Idle,
                            context.commands.clone(),
                            Vec::new(),
                            &context.status,
                        )
                    }
                    other => other,
                };
            }
            ManagerCommand::DrainExpired { generation, epoch } => {
                state = match state {
                    ManagerState::Running(running)
                        if shutting_down
                            && running.generation == generation
                            && drain_epoch == epoch =>
                    {
                        begin_stop(
                            running,
                            EngineStopReason::HostShutdown,
                            context.commands.clone(),
                            Vec::new(),
                            &context.status,
                        )
                    }
                    other => other,
                };
            }
            ManagerCommand::GenerationTerminal {
                generation,
                state: terminal,
            } => {
                state = match state {
                    ManagerState::Running(running) if running.generation == generation => {
                        let error = terminal_engine_error(&terminal);
                        context
                            .status
                            .send_replace(EngineManagerStatus::Failed(error));
                        begin_stop(
                            running,
                            EngineStopReason::Crashed,
                            context.commands.clone(),
                            Vec::new(),
                            &context.status,
                        )
                    }
                    other => other,
                };
            }
            ManagerCommand::StopFinished {
                generation,
                reason,
                result,
            } => {
                let ManagerState::Stopping {
                    generation: active_generation,
                    reason: active_reason,
                    waiters,
                } = state
                else {
                    continue;
                };
                if active_generation != generation || active_reason != reason {
                    fail_waiters(waiters, &engine_manager_unavailable());
                    state = ManagerState::Cold;
                    continue;
                }
                if shutting_down {
                    fail_waiters(waiters, &runtime_shutting_down());
                    let shutdown_result = if reason == EngineStopReason::Crashed {
                        Ok(())
                    } else {
                        result
                    };
                    state = ManagerState::Closed;
                    finish_shutdown(&context.status, &mut shutdown_replies, &shutdown_result);
                    break;
                }

                if waiters.is_empty() {
                    match result {
                        Ok(()) if reason == EngineStopReason::Idle => {
                            context.status.send_replace(EngineManagerStatus::Idle);
                        }
                        Ok(()) if reason == EngineStopReason::Crashed => {
                            context
                                .status
                                .send_replace(EngineManagerStatus::Failed(crashed_engine_error()));
                        }
                        Ok(()) => {}
                        Err(error) => {
                            context
                                .status
                                .send_replace(EngineManagerStatus::Failed(error));
                        }
                    }
                    state = ManagerState::Cold;
                } else {
                    if let Err(error) = result {
                        tracing::warn!(%error, "previous engine generation cleanup failed before restart");
                    }
                    let attempt = next_attempt;
                    next_attempt = next_attempt.wrapping_add(1).max(1);
                    let cancellation = CancellationToken::new();
                    context.status.send_replace(EngineManagerStatus::Starting);
                    spawn_start(
                        attempt,
                        context.config.clone(),
                        Arc::clone(&context.drivers),
                        cancellation.clone(),
                        context.commands.clone(),
                    );
                    state = ManagerState::Starting {
                        attempt,
                        waiters,
                        cancellation,
                    };
                }
            }
            ManagerCommand::Shutdown { reply } => {
                if matches!(state, ManagerState::Closed) {
                    let _ = reply.send(Ok(()));
                    continue;
                }
                shutdown_replies.push(reply);
                if shutting_down {
                    continue;
                }
                shutting_down = true;
                context
                    .status
                    .send_replace(EngineManagerStatus::ShuttingDown);
                state = start_host_shutdown(
                    state,
                    false,
                    &mut drain_epoch,
                    &context.commands,
                    &context.status,
                );
                if matches!(state, ManagerState::Closed) {
                    finish_shutdown(&context.status, &mut shutdown_replies, &Ok(()));
                    break;
                }
            }
            ManagerCommand::ForceShutdown => {
                if matches!(state, ManagerState::Closed) {
                    break;
                }
                shutting_down = true;
                context
                    .status
                    .send_replace(EngineManagerStatus::ShuttingDown);
                state = start_host_shutdown(
                    state,
                    true,
                    &mut drain_epoch,
                    &context.commands,
                    &context.status,
                );
                if matches!(state, ManagerState::Closed) {
                    finish_shutdown(&context.status, &mut shutdown_replies, &Ok(()));
                    break;
                }
            }
        }
    }

    if !matches!(state, ManagerState::Closed) {
        context.status.send_replace(EngineManagerStatus::Stopped);
        for reply in shutdown_replies {
            let _ = reply.send(Err(engine_manager_unavailable()));
        }
    }
}

fn start_host_shutdown(
    state: ManagerState,
    force: bool,
    drain_epoch: &mut u64,
    commands: &mpsc::UnboundedSender<ManagerCommand>,
    status: &watch::Sender<EngineManagerStatus>,
) -> ManagerState {
    match state {
        ManagerState::Cold | ManagerState::Closed => ManagerState::Closed,
        ManagerState::Starting {
            attempt,
            waiters,
            cancellation,
        } => {
            fail_waiters(waiters, &runtime_shutting_down());
            cancellation.cancel();
            ManagerState::Starting {
                attempt,
                waiters: Vec::new(),
                cancellation,
            }
        }
        ManagerState::Running(running) if force || running.leases == 0 => begin_stop(
            running,
            EngineStopReason::HostShutdown,
            commands.clone(),
            Vec::new(),
            status,
        ),
        ManagerState::Running(running) => {
            *drain_epoch = (*drain_epoch).wrapping_add(1);
            schedule_drain(
                running.generation,
                *drain_epoch,
                DEFAULT_ENGINE_SHUTDOWN_DRAIN_TIMEOUT,
                commands.clone(),
            );
            ManagerState::Running(running)
        }
        ManagerState::Stopping {
            generation,
            reason,
            waiters,
        } => {
            fail_waiters(waiters, &runtime_shutting_down());
            ManagerState::Stopping {
                generation,
                reason,
                waiters: Vec::new(),
            }
        }
    }
}

fn issue_lease(
    running: &mut RunningGeneration,
    reply: AcquireReply,
    commands: &mpsc::UnboundedSender<ManagerCommand>,
) {
    if reply.is_closed() {
        return;
    }
    running.idle_epoch = running.idle_epoch.wrapping_add(1);
    running.leases = running.leases.saturating_add(1);
    let lease = EngineLease::managed(
        running.supervisor.client(),
        running.generation,
        commands.clone(),
    );
    let _ = reply.send(Ok(lease));
}

fn begin_stop(
    running: RunningGeneration,
    mut reason: EngineStopReason,
    commands: mpsc::UnboundedSender<ManagerCommand>,
    waiters: Vec<AcquireReply>,
    status: &watch::Sender<EngineManagerStatus>,
) -> ManagerState {
    let generation = running.generation;
    let terminal_observed = running.supervisor.state().is_terminal();
    if terminal_observed && reason == EngineStopReason::Idle {
        reason = EngineStopReason::Crashed;
    }
    status.send_replace(EngineManagerStatus::Stopping { generation, reason });
    spawn_stop(running.supervisor, reason, commands, terminal_observed);
    ManagerState::Stopping {
        generation,
        reason,
        waiters,
    }
}

fn spawn_start(
    attempt: u64,
    config: EngineConfig,
    drivers: Arc<PreparedDriverPacks>,
    cancellation: CancellationToken,
    commands: mpsc::UnboundedSender<ManagerCommand>,
) {
    let startup =
        tokio::spawn(async move { start_generation(config, &drivers, &cancellation).await });
    tokio::spawn(async move {
        let result = startup.await.unwrap_or_else(|error| {
            tracing::error!(%error, attempt, "engine startup task failed to join");
            Err(AppError::internal())
        });
        let _ = commands.send(ManagerCommand::StartFinished { attempt, result });
    });
}

async fn start_generation(
    config: EngineConfig,
    drivers: &PreparedDriverPacks,
    cancellation: &CancellationToken,
) -> Result<EngineSupervisor, AppError> {
    let supervisor = EngineSupervisor::spawn(config)
        .await
        .map_err(AppError::from)?;
    if cancellation.is_cancelled() {
        stop_cancelled_start(&supervisor).await;
        return Err(runtime_shutting_down());
    }
    let preload = tokio::select! {
        biased;
        () = cancellation.cancelled() => {
            stop_cancelled_start(&supervisor).await;
            return Err(runtime_shutting_down());
        }
        result = driver_pack::preload(&supervisor, drivers) => result,
    };
    match preload {
        Ok(_) => Ok(supervisor),
        Err(error) => {
            let primary = error.into_app_error();
            if let Err(cleanup) = supervisor.shutdown().await {
                tracing::error!(
                    error = %cleanup,
                    "Java cleanup failed after managed JDBC driver preload error"
                );
            }
            Err(primary)
        }
    }
}

async fn stop_cancelled_start(supervisor: &EngineSupervisor) {
    if let Err(error) = supervisor.shutdown().await {
        tracing::error!(%error, "Java cleanup failed after engine startup was cancelled");
    }
}

fn spawn_stop(
    supervisor: EngineSupervisor,
    reason: EngineStopReason,
    commands: mpsc::UnboundedSender<ManagerCommand>,
    terminal_observed: bool,
) {
    let generation = supervisor.state().generation();
    let stopping = tokio::spawn(async move {
        supervisor
            .shutdown()
            .await
            .map(|_| ())
            .map_err(AppError::from)
    });
    tokio::spawn(async move {
        let result = stopping.await.unwrap_or_else(|error| {
            tracing::error!(%error, generation, "engine shutdown task failed to join");
            Err(AppError::internal())
        });
        let result = if terminal_observed {
            if let Err(error) = &result {
                tracing::warn!(%error, generation, "reaped a terminal Java generation");
            }
            Ok(())
        } else {
            result
        };
        let _ = commands.send(ManagerCommand::StopFinished {
            generation,
            reason,
            result,
        });
    });
}

fn spawn_terminal_watcher(
    generation: u64,
    mut state: watch::Receiver<EngineState>,
    commands: mpsc::UnboundedSender<ManagerCommand>,
) {
    tokio::spawn(async move {
        loop {
            let current = state.borrow().clone();
            if current.is_terminal() {
                let _ = commands.send(ManagerCommand::GenerationTerminal {
                    generation,
                    state: current,
                });
                return;
            }
            if state.changed().await.is_err() {
                return;
            }
        }
    });
}

fn schedule_idle(
    generation: u64,
    epoch: u64,
    timeout: Duration,
    commands: mpsc::UnboundedSender<ManagerCommand>,
) {
    tokio::spawn(async move {
        tokio::time::sleep(timeout).await;
        let _ = commands.send(ManagerCommand::IdleExpired { generation, epoch });
    });
}

fn schedule_drain(
    generation: u64,
    epoch: u64,
    timeout: Duration,
    commands: mpsc::UnboundedSender<ManagerCommand>,
) {
    tokio::spawn(async move {
        tokio::time::sleep(timeout).await;
        let _ = commands.send(ManagerCommand::DrainExpired { generation, epoch });
    });
}

fn fail_waiters(waiters: Vec<AcquireReply>, error: &AppError) {
    for waiter in waiters {
        let _ = waiter.send(Err(error.clone()));
    }
}

fn finish_shutdown(
    status: &watch::Sender<EngineManagerStatus>,
    replies: &mut Vec<ShutdownReply>,
    result: &Result<(), AppError>,
) {
    status.send_replace(EngineManagerStatus::Stopped);
    for reply in replies.drain(..) {
        let _ = reply.send(result.clone());
    }
}

fn engine_not_configured() -> AppError {
    AppError::unavailable(
        "database_engine_unavailable",
        "The database compatibility engine is not configured",
    )
}

fn engine_manager_unavailable() -> AppError {
    AppError::unavailable(
        "database_engine_unavailable",
        "The database compatibility engine is unavailable",
    )
}

fn runtime_shutting_down() -> AppError {
    AppError::unavailable(
        "runtime_shutting_down",
        "The Chat2DB runtime is shutting down",
    )
}

fn terminal_engine_error(state: &EngineState) -> AppError {
    let message = match state {
        EngineState::Crashed { .. } => "The database compatibility engine crashed",
        EngineState::Failed { .. } => "The database compatibility engine failed",
        EngineState::Stopped { .. } => "The database compatibility engine stopped",
        _ => "The database compatibility engine became unavailable",
    };
    AppError::unavailable("database_engine_unavailable", message)
}

fn crashed_engine_error() -> AppError {
    AppError::unavailable(
        "database_engine_unavailable",
        "The database compatibility engine crashed",
    )
}
