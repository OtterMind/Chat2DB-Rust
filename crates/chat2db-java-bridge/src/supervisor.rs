use std::{
    collections::{HashMap, HashSet, VecDeque},
    future::pending,
    io,
    process::{ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chat2db_engine_protocol::{
    MAX_FRAME_BYTES, MIN_FRAME_BYTES, current_version, read_frame_with_limit, wire,
    write_frame_with_limit,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    process::{Child, ChildStdin, ChildStdout},
    sync::{Mutex, mpsc, oneshot, watch},
    task::JoinHandle,
    time::{Instant, timeout, timeout_at},
};

use crate::{
    BridgeError, DeliveryOutcome, EngineCommand, EngineIdentity, EngineState, PingReply,
    ProcessExit, StderrSnapshot, error::PendingFailure, stderr_tail::StderrTail,
};

const PING_CAPABILITY: &str = "lifecycle.ping.v1";
const SHUTDOWN_CAPABILITY: &str = "lifecycle.shutdown.v1";
const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_IN_FLIGHT: usize = 256;
const DEFAULT_STDERR_TAIL_BYTES: usize = 64 * 1024;
const MAX_RETIRED_REQUESTS: usize = 1024;
const TASK_JOIN_TIMEOUT: Duration = Duration::from_secs(1);

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Process, protocol, and resource limits for one engine generation.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    command: EngineCommand,
    supported_versions: Vec<wire::ProtocolVersion>,
    required_capabilities: Vec<String>,
    startup_timeout: Duration,
    request_timeout: Duration,
    shutdown_timeout: Duration,
    max_receive_frame_bytes: u32,
    max_in_flight: usize,
    stderr_tail_bytes: usize,
}

impl EngineConfig {
    /// Creates a production-oriented configuration for one process command.
    #[must_use]
    pub fn new(command: EngineCommand) -> Self {
        Self {
            command,
            supported_versions: vec![current_version()],
            required_capabilities: vec![PING_CAPABILITY.to_owned(), SHUTDOWN_CAPABILITY.to_owned()],
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
            max_receive_frame_bytes: u32::try_from(MAX_FRAME_BYTES).unwrap_or(u32::MAX),
            max_in_flight: DEFAULT_MAX_IN_FLIGHT,
            stderr_tail_bytes: DEFAULT_STDERR_TAIL_BYTES,
        }
    }

    /// Overrides the exact protocol versions offered during handshake.
    #[must_use]
    pub fn with_supported_versions(mut self, versions: Vec<wire::ProtocolVersion>) -> Self {
        self.supported_versions = versions;
        self
    }

    /// Overrides startup, normal request, and shutdown deadlines.
    #[must_use]
    pub const fn with_timeouts(
        mut self,
        startup: Duration,
        request: Duration,
        shutdown: Duration,
    ) -> Self {
        self.startup_timeout = startup;
        self.request_timeout = request;
        self.shutdown_timeout = shutdown;
        self
    }

    /// Overrides the bounded stderr tail retained for diagnostics.
    #[must_use]
    pub const fn with_stderr_tail_bytes(mut self, bytes: usize) -> Self {
        self.stderr_tail_bytes = bytes;
        self
    }

    /// Overrides the maximum number of requests accepted by one generation.
    #[must_use]
    pub const fn with_max_in_flight(mut self, maximum: usize) -> Self {
        self.max_in_flight = maximum;
        self
    }

    /// Overrides the maximum frame size advertised to the engine.
    #[must_use]
    pub const fn with_max_receive_frame_bytes(mut self, maximum: u32) -> Self {
        self.max_receive_frame_bytes = maximum;
        self
    }

    fn validate(&self) -> Result<(), BridgeError> {
        if self.supported_versions.is_empty() {
            return Err(BridgeError::InvalidConfig(
                "at least one supported protocol version is required".to_owned(),
            ));
        }
        if usize::try_from(self.max_receive_frame_bytes).unwrap_or(0) < MIN_FRAME_BYTES
            || usize::try_from(self.max_receive_frame_bytes).unwrap_or(usize::MAX) > MAX_FRAME_BYTES
        {
            return Err(BridgeError::InvalidConfig(format!(
                "max_receive_frame_bytes must be between {MIN_FRAME_BYTES} and {MAX_FRAME_BYTES}"
            )));
        }
        if self.max_in_flight == 0 {
            return Err(BridgeError::InvalidConfig(
                "max_in_flight must be greater than zero".to_owned(),
            ));
        }
        if self.stderr_tail_bytes == 0 {
            return Err(BridgeError::InvalidConfig(
                "stderr_tail_bytes must be greater than zero".to_owned(),
            ));
        }
        if [
            self.startup_timeout,
            self.request_timeout,
            self.shutdown_timeout,
        ]
        .contains(&Duration::ZERO)
        {
            return Err(BridgeError::InvalidConfig(
                "process timeouts must be greater than zero".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Single owner of one supervised Java process generation.
pub struct EngineSupervisor {
    client: EngineClient,
    control: mpsc::UnboundedSender<ActorControl>,
    actor_task: Mutex<Option<JoinHandle<()>>>,
}

impl EngineSupervisor {
    /// Launches the process and returns only after a validated handshake.
    ///
    /// # Errors
    ///
    /// Returns an error when configuration is invalid, the child cannot start,
    /// handshake fails, or the selected version/capabilities are unacceptable.
    pub async fn spawn(config: EngineConfig) -> Result<Self, BridgeError> {
        config.validate()?;
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);

        let mut command = config.command.build();
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(BridgeError::Spawn)?;
        let process_id = child.id();
        let stdin = child
            .stdin
            .take()
            .ok_or(BridgeError::MissingPipe("stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(BridgeError::MissingPipe("stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or(BridgeError::MissingPipe("stderr"))?;

        let stderr_tail = StderrTail::new(config.stderr_tail_bytes);
        let drain_tail = stderr_tail.clone();
        let stderr_task = tokio::spawn(async move { drain_tail.drain(stderr).await });
        let (state_sender, state_receiver) = watch::channel(EngineState::Starting { generation });
        let channel_capacity = config.max_in_flight.saturating_add(8);
        let (command_sender, command_receiver) = mpsc::channel(channel_capacity);
        let (control_sender, control_receiver) = mpsc::unbounded_channel();

        let actor_task = tokio::spawn(run_actor(ActorContext {
            generation,
            child,
            stdin,
            stdout,
            stderr_task,
            stderr_tail: stderr_tail.clone(),
            state: state_sender,
            commands: command_receiver,
            controls: control_receiver,
            max_in_flight: config.max_in_flight,
            max_receive_frame_bytes: usize::try_from(config.max_receive_frame_bytes)
                .unwrap_or(MAX_FRAME_BYTES),
        }));

        let inner = Arc::new(EngineInner {
            generation,
            process_id,
            commands: command_sender,
            control: control_sender.clone(),
            state: state_receiver,
            request_counter: AtomicU64::new(1),
            request_timeout: config.request_timeout,
            shutdown_timeout: config.shutdown_timeout,
            stderr_tail,
            shutdown_lock: Mutex::new(()),
        });
        let client = EngineClient { inner };

        let identity = match client.handshake(&config).await {
            Ok(identity) => identity,
            Err(error) => {
                let public_error = if matches!(error, BridgeError::RequestTimeout { .. }) {
                    BridgeError::StartupTimeout
                } else {
                    error
                };
                client
                    .terminate_start_failure(public_error.to_string())
                    .await;
                let _ = actor_task.await;
                return Err(public_error);
            }
        };

        if let Err(error) = client.promote_ready(identity).await {
            client.terminate_start_failure(error.to_string()).await;
            let _ = actor_task.await;
            return Err(error);
        }

        Ok(Self {
            client,
            control: control_sender,
            actor_task: Mutex::new(Some(actor_task)),
        })
    }

    /// Returns a cloneable request handle without exposing process ownership.
    #[must_use]
    pub fn client(&self) -> EngineClient {
        self.client.clone()
    }

    /// Returns the current process state.
    #[must_use]
    pub fn state(&self) -> EngineState {
        self.client.state()
    }

    /// Subscribes to lifecycle changes for health propagation.
    #[must_use]
    pub fn subscribe_state(&self) -> watch::Receiver<EngineState> {
        self.client.subscribe_state()
    }

    /// Returns the operating-system process id when the platform exposes it.
    #[must_use]
    pub fn process_id(&self) -> Option<u32> {
        self.client.inner.process_id
    }

    /// Returns the current bounded stderr diagnostic tail.
    pub async fn stderr_snapshot(&self) -> StderrSnapshot {
        self.client.inner.stderr_tail.snapshot().await
    }

    /// Gracefully shuts down and fully reaps the child process.
    ///
    /// # Errors
    ///
    /// Returns an error when the engine is unavailable, rejects shutdown, or
    /// does not exit before the forced-termination deadline.
    pub async fn shutdown(&self) -> Result<ProcessExit, BridgeError> {
        let result = self.client.shutdown().await;
        self.join_actor().await?;
        result
    }

    async fn join_actor(&self) -> Result<(), BridgeError> {
        let Some(task) = self.actor_task.lock().await.take() else {
            return Ok(());
        };
        task.await
            .map_err(|error| BridgeError::SupervisorTask(error.to_string()))
    }
}

impl Drop for EngineSupervisor {
    fn drop(&mut self) {
        let _ = self.control.send(ActorControl::Terminate {
            disposition: FinalDisposition::Stopped,
        });
    }
}

/// Cloneable lifecycle request handle for the current engine generation.
#[derive(Clone)]
pub struct EngineClient {
    inner: Arc<EngineInner>,
}

struct EngineInner {
    generation: u64,
    process_id: Option<u32>,
    commands: mpsc::Sender<ActorCommand>,
    control: mpsc::UnboundedSender<ActorControl>,
    state: watch::Receiver<EngineState>,
    request_counter: AtomicU64,
    request_timeout: Duration,
    shutdown_timeout: Duration,
    stderr_tail: StderrTail,
    shutdown_lock: Mutex<()>,
}

impl EngineClient {
    /// Returns the current process state.
    #[must_use]
    pub fn state(&self) -> EngineState {
        self.inner.state.borrow().clone()
    }

    /// Subscribes to lifecycle changes for this process generation.
    #[must_use]
    pub fn subscribe_state(&self) -> watch::Receiver<EngineState> {
        self.inner.state.clone()
    }

    /// Sends a correlated ping through the process protocol.
    ///
    /// # Errors
    ///
    /// Returns an error when the engine is not ready, the request times out,
    /// the process exits, or the response violates the negotiated protocol.
    pub async fn ping(&self, nonce: u64) -> Result<PingReply, BridgeError> {
        self.require_ready()?;
        let response = self
            .send_request(
                wire::client_envelope::Payload::Ping(wire::Ping { nonce }),
                self.inner.request_timeout,
                false,
            )
            .await?;
        match response.payload {
            Some(wire::server_envelope::Payload::Pong(pong)) if pong.nonce == nonce => {
                Ok(PingReply {
                    nonce: pong.nonce,
                    uptime_millis: pong.uptime_millis,
                })
            }
            Some(wire::server_envelope::Payload::Pong(_)) => {
                let error = BridgeError::Protocol(
                    "ping response nonce did not match the request".to_owned(),
                );
                self.terminate_protocol_failure(error.to_string()).await;
                Err(error)
            }
            _ => {
                let error = BridgeError::UnexpectedResponse("expected pong");
                self.terminate_protocol_failure(error.to_string()).await;
                Err(error)
            }
        }
    }

    async fn handshake(&self, config: &EngineConfig) -> Result<EngineIdentity, BridgeError> {
        let hello = wire::ClientHello {
            runtime_name: "chat2db-rust".to_owned(),
            runtime_version: env!("CARGO_PKG_VERSION").to_owned(),
            supported_versions: config.supported_versions.clone(),
            required_capabilities: config.required_capabilities.clone(),
            max_receive_frame_bytes: config.max_receive_frame_bytes,
        };
        let response = self
            .send_request(
                wire::client_envelope::Payload::Hello(hello),
                config.startup_timeout,
                false,
            )
            .await?;
        let Some(wire::server_envelope::Payload::Hello(server_hello)) = response.payload else {
            return Err(BridgeError::UnexpectedResponse("expected server hello"));
        };
        let selected = server_hello.selected_version.ok_or_else(|| {
            BridgeError::InvalidHandshake("selected protocol version is missing".to_owned())
        })?;
        if !config.supported_versions.contains(&selected) {
            return Err(BridgeError::UnsupportedVersion {
                major: selected.major,
                minor: selected.minor,
            });
        }
        for required in &config.required_capabilities {
            if !server_hello.capabilities.contains(required) {
                return Err(BridgeError::MissingCapability(required.clone()));
            }
        }
        if server_hello.engine_name.is_empty()
            || server_hello.engine_version.is_empty()
            || server_hello.engine_instance_id.is_empty()
        {
            return Err(BridgeError::InvalidHandshake(
                "engine identity fields cannot be empty".to_owned(),
            ));
        }
        if usize::try_from(server_hello.max_receive_frame_bytes).unwrap_or(0) < MIN_FRAME_BYTES
            || usize::try_from(server_hello.max_receive_frame_bytes).unwrap_or(usize::MAX)
                > MAX_FRAME_BYTES
        {
            return Err(BridgeError::InvalidHandshake(format!(
                "engine max_receive_frame_bytes must be between {MIN_FRAME_BYTES} and {MAX_FRAME_BYTES}"
            )));
        }

        Ok(EngineIdentity {
            name: server_hello.engine_name,
            version: server_hello.engine_version,
            instance_id: server_hello.engine_instance_id,
            protocol_version: selected,
            capabilities: server_hello.capabilities,
            max_frame_bytes: server_hello.max_receive_frame_bytes,
        })
    }

    async fn promote_ready(&self, identity: EngineIdentity) -> Result<(), BridgeError> {
        let (response, completion) = oneshot::channel();
        self.inner
            .commands
            .send(ActorCommand::PromoteReady { identity, response })
            .await
            .map_err(|_| BridgeError::ProcessUnavailable {
                message: "engine exited before the handshake could complete".to_owned(),
                outcome: DeliveryOutcome::Unknown,
            })?;
        completion
            .await
            .map_err(|_| BridgeError::ProcessUnavailable {
                message: "engine exited before entering the ready state".to_owned(),
                outcome: DeliveryOutcome::Unknown,
            })?
    }

    async fn shutdown(&self) -> Result<ProcessExit, BridgeError> {
        let _shutdown_guard = self.inner.shutdown_lock.lock().await;
        match self.state() {
            EngineState::Stopped { exit, .. } => return Ok(exit),
            EngineState::Failed { reason, .. } => {
                return Err(BridgeError::ProcessUnavailable {
                    message: reason,
                    outcome: DeliveryOutcome::Unknown,
                });
            }
            EngineState::Crashed { .. } => {
                return Err(BridgeError::ProcessUnavailable {
                    message: "engine process has crashed".to_owned(),
                    outcome: DeliveryOutcome::Unknown,
                });
            }
            EngineState::Ready { .. } => {}
            state => {
                return Err(BridgeError::NotReady {
                    state: state.label(),
                });
            }
        }

        let mut termination_guard = ShutdownTerminationGuard::new(self.inner.control.clone());
        let shutdown_result = self
            .send_request(
                wire::client_envelope::Payload::Shutdown(wire::Shutdown {
                    reason: "host shutdown".to_owned(),
                }),
                self.inner.shutdown_timeout,
                true,
            )
            .await;
        match shutdown_result {
            Ok(wire::ServerEnvelope {
                payload: Some(wire::server_envelope::Payload::ShutdownAck(_)),
                ..
            }) => {}
            Ok(_) => {
                let error = BridgeError::UnexpectedResponse("expected shutdown ack");
                self.terminate_protocol_failure(error.to_string()).await;
                if self.state().is_terminal() {
                    termination_guard.disarm();
                }
                return Err(error);
            }
            Err(error) => {
                self.force_stop_and_wait().await;
                if self.state().is_terminal() {
                    termination_guard.disarm();
                }
                return Err(error);
            }
        }

        if self
            .inner
            .commands
            .send(ActorCommand::CloseInput)
            .await
            .is_err()
        {
            self.force_stop_and_wait().await;
            if self.state().is_terminal() {
                termination_guard.disarm();
            }
            return Err(BridgeError::CommandChannelClosed {
                outcome: DeliveryOutcome::Unknown,
            });
        }
        let result = match self.wait_for_terminal(self.inner.shutdown_timeout).await {
            Ok(EngineState::Stopped { exit, .. }) => Ok(exit),
            Ok(state) => Err(BridgeError::ProcessUnavailable {
                message: format!("engine ended in {} state", state.label()),
                outcome: DeliveryOutcome::Unknown,
            }),
            Err(BridgeError::RequestTimeout { .. }) => {
                self.force_stop_and_wait().await;
                Err(BridgeError::ShutdownTimeout)
            }
            Err(error) => Err(error),
        };
        if self.state().is_terminal() {
            termination_guard.disarm();
        }
        result
    }

    async fn send_request(
        &self,
        payload: wire::client_envelope::Payload,
        request_timeout: Duration,
        begins_shutdown: bool,
    ) -> Result<wire::ServerEnvelope, BridgeError> {
        let request_number = self.inner.request_counter.fetch_add(1, Ordering::Relaxed);
        let request_id = format!("{}-{request_number}", self.inner.generation);
        let trace_id = format!("engine-{request_id}");
        let deadline = Instant::now() + request_timeout;
        let deadline_unix_millis = unix_millis_after(request_timeout);
        let request = wire::ClientEnvelope {
            meta: Some(wire::RequestMeta {
                request_id: request_id.clone(),
                trace_id,
                deadline_unix_millis: Some(deadline_unix_millis),
                ..Default::default()
            }),
            payload: Some(payload),
        };
        let (response_sender, response_receiver) = oneshot::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let command = ActorCommand::Request(Box::new(RequestCommand {
            request,
            response: response_sender,
            begins_shutdown,
            deadline,
            cancelled: cancelled.clone(),
        }));

        match timeout_at(deadline, self.inner.commands.send(command)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                return Err(BridgeError::CommandChannelClosed {
                    outcome: DeliveryOutcome::NotSent,
                });
            }
            Err(_) => {
                return Err(BridgeError::RequestTimeout {
                    request_id,
                    outcome: DeliveryOutcome::NotSent,
                });
            }
        }

        let mut retire_guard =
            RequestRetireGuard::new(request_id, self.inner.control.clone(), cancelled);
        let response = match response_receiver.await {
            Ok(Ok(response)) => response,
            Ok(Err(failure)) => {
                retire_guard.disarm();
                return Err(failure.into_bridge_error());
            }
            Err(_) => {
                retire_guard.disarm();
                return Err(BridgeError::CommandChannelClosed {
                    outcome: DeliveryOutcome::Unknown,
                });
            }
        };
        retire_guard.disarm();

        if let Some(wire::server_envelope::Payload::Error(error)) = response.payload.clone() {
            return Err(crate::RemoteEngineError::from(error).into());
        }
        Ok(response)
    }

    fn require_ready(&self) -> Result<(), BridgeError> {
        let state = self.state();
        if matches!(state, EngineState::Ready { .. }) {
            Ok(())
        } else {
            Err(BridgeError::NotReady {
                state: state.label(),
            })
        }
    }

    async fn terminate_start_failure(&self, reason: String) {
        self.terminate_protocol_failure(reason).await;
    }

    async fn terminate_protocol_failure(&self, reason: String) {
        let _ = self.inner.control.send(ActorControl::Terminate {
            disposition: FinalDisposition::Failed(reason),
        });
        let _ = self.wait_for_terminal_reaped().await;
    }

    async fn force_stop_and_wait(&self) {
        let _ = self.inner.control.send(ActorControl::Terminate {
            disposition: FinalDisposition::Stopped,
        });
        let _ = self.wait_for_terminal_reaped().await;
    }

    async fn wait_for_terminal(&self, wait: Duration) -> Result<EngineState, BridgeError> {
        let mut receiver = self.inner.state.clone();
        let future = async {
            loop {
                let state = receiver.borrow().clone();
                if state.is_terminal() {
                    return Ok(state);
                }
                receiver
                    .changed()
                    .await
                    .map_err(|_| BridgeError::CommandChannelClosed {
                        outcome: DeliveryOutcome::Unknown,
                    })?;
            }
        };
        timeout(wait, future)
            .await
            .map_err(|_| BridgeError::RequestTimeout {
                request_id: "process-exit".to_owned(),
                outcome: DeliveryOutcome::Unknown,
            })?
    }

    async fn wait_for_terminal_reaped(&self) -> Result<EngineState, BridgeError> {
        let mut receiver = self.inner.state.clone();
        loop {
            let state = receiver.borrow().clone();
            if state.is_terminal() {
                return Ok(state);
            }
            receiver
                .changed()
                .await
                .map_err(|_| BridgeError::CommandChannelClosed {
                    outcome: DeliveryOutcome::Unknown,
                })?;
        }
    }
}

fn unix_millis_after(duration: Duration) -> u64 {
    let deadline = SystemTime::now()
        .checked_add(duration)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let millis = deadline
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

struct RequestRetireGuard {
    request_id: Option<String>,
    control: mpsc::UnboundedSender<ActorControl>,
    cancelled: Arc<AtomicBool>,
}

impl RequestRetireGuard {
    fn new(
        request_id: String,
        control: mpsc::UnboundedSender<ActorControl>,
        cancelled: Arc<AtomicBool>,
    ) -> Self {
        Self {
            request_id: Some(request_id),
            control,
            cancelled,
        }
    }

    fn disarm(&mut self) {
        self.request_id = None;
    }
}

impl Drop for RequestRetireGuard {
    fn drop(&mut self) {
        if let Some(request_id) = self.request_id.take() {
            self.cancelled.store(true, Ordering::Release);
            let _ = self.control.send(ActorControl::Retire(request_id));
        }
    }
}

struct ShutdownTerminationGuard {
    control: Option<mpsc::UnboundedSender<ActorControl>>,
}

impl ShutdownTerminationGuard {
    fn new(control: mpsc::UnboundedSender<ActorControl>) -> Self {
        Self {
            control: Some(control),
        }
    }

    fn disarm(&mut self) {
        self.control = None;
    }
}

impl Drop for ShutdownTerminationGuard {
    fn drop(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.send(ActorControl::Terminate {
                disposition: FinalDisposition::Stopped,
            });
        }
    }
}

enum ActorCommand {
    Request(Box<RequestCommand>),
    PromoteReady {
        identity: EngineIdentity,
        response: oneshot::Sender<Result<(), BridgeError>>,
    },
    CloseInput,
}

enum ActorControl {
    Retire(String),
    Terminate { disposition: FinalDisposition },
}

struct RequestCommand {
    request: wire::ClientEnvelope,
    response: oneshot::Sender<Result<wire::ServerEnvelope, PendingFailure>>,
    begins_shutdown: bool,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}

enum WriterCommand {
    Frame(Box<wire::ClientEnvelope>),
    SetMaxFrameBytes(usize),
    Close,
}

enum WriterEvent {
    Closed,
    Failed(String),
}

enum ReaderEvent {
    Frame(wire::ServerEnvelope),
    Eof,
    Failed(String),
}

enum ChildControl {
    Kill,
}

#[derive(Clone, Debug)]
enum FinalDisposition {
    Stopped,
    Crashed,
    Failed(String),
}

struct PendingRequest {
    trace_id: String,
    response: oneshot::Sender<Result<wire::ServerEnvelope, PendingFailure>>,
    deadline: Instant,
}

struct ActorContext {
    generation: u64,
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    stderr_task: JoinHandle<std::io::Result<()>>,
    stderr_tail: StderrTail,
    state: watch::Sender<EngineState>,
    commands: mpsc::Receiver<ActorCommand>,
    controls: mpsc::UnboundedReceiver<ActorControl>,
    max_in_flight: usize,
    max_receive_frame_bytes: usize,
}

struct ActorSession {
    generation: u64,
    max_in_flight: usize,
    pending: HashMap<String, PendingRequest>,
    retired: RetiredRequests,
    phase: SessionPhase,
    writer_finished: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionPhase {
    Handshaking,
    Ready,
    Stopping,
}

impl ActorSession {
    fn new(generation: u64, max_in_flight: usize) -> Self {
        Self {
            generation,
            max_in_flight,
            pending: HashMap::new(),
            retired: RetiredRequests::default(),
            phase: SessionPhase::Handshaking,
            writer_finished: false,
        }
    }

    fn accept_request(
        &mut self,
        command: Box<RequestCommand>,
        writer: &mpsc::Sender<WriterCommand>,
        state: &watch::Sender<EngineState>,
    ) {
        let RequestCommand {
            request,
            response,
            begins_shutdown,
            deadline,
            cancelled,
        } = *command;
        if cancelled.load(Ordering::Acquire) {
            return;
        }
        if self.phase == SessionPhase::Stopping || self.pending.len() >= self.max_in_flight {
            reject_request(response, "engine is not accepting another request");
            return;
        }
        let is_handshake = matches!(
            request.payload,
            Some(wire::client_envelope::Payload::Hello(_))
        );
        if (self.phase == SessionPhase::Handshaking && !is_handshake)
            || (self.phase == SessionPhase::Ready && is_handshake)
        {
            reject_request(response, "request is invalid for the current engine phase");
            return;
        }
        let Some(meta) = request.meta.as_ref() else {
            reject_request(response, "request metadata is missing");
            return;
        };
        if meta.request_id.is_empty() || self.pending.contains_key(&meta.request_id) {
            reject_request(response, "request id is empty or already in flight");
            return;
        }

        let request_id = meta.request_id.clone();
        let trace_id = meta.trace_id.clone();
        if deadline <= Instant::now() {
            let _ = response.send(Err(PendingFailure::Timeout {
                request_id,
                outcome: DeliveryOutcome::NotSent,
            }));
            return;
        }
        self.pending.insert(
            request_id.clone(),
            PendingRequest {
                trace_id,
                response,
                deadline,
            },
        );
        if writer
            .try_send(WriterCommand::Frame(Box::new(request)))
            .is_err()
        {
            if let Some(pending_request) = self.pending.remove(&request_id) {
                reject_request(
                    pending_request.response,
                    "engine writer queue is unavailable",
                );
            }
            return;
        }
        if begins_shutdown {
            self.phase = SessionPhase::Stopping;
            state.send_replace(EngineState::Stopping {
                generation: self.generation,
            });
        }
    }

    fn retire(&mut self, request_id: String) {
        if self.pending.remove(&request_id).is_some() {
            self.retired.insert(request_id);
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.pending.values().map(|request| request.deadline).min()
    }

    fn expire_requests(&mut self, now: Instant) {
        let expired = self
            .pending
            .iter()
            .filter(|(_, request)| request.deadline <= now)
            .map(|(request_id, _)| request_id.clone())
            .collect::<Vec<_>>();
        for request_id in expired {
            let Some(request) = self.pending.remove(&request_id) else {
                continue;
            };
            self.retired.insert(request_id.clone());
            let _ = request.response.send(Err(PendingFailure::Timeout {
                request_id,
                outcome: DeliveryOutcome::Unknown,
            }));
        }
    }

    fn promote_ready(
        &mut self,
        identity: EngineIdentity,
        response: oneshot::Sender<Result<(), BridgeError>>,
        writer: &mpsc::Sender<WriterCommand>,
        state: &watch::Sender<EngineState>,
    ) -> Option<FinalDisposition> {
        if self.phase != SessionPhase::Handshaking || !self.pending.is_empty() {
            let message = "engine cannot enter ready state from its current protocol phase";
            let _ = response.send(Err(BridgeError::InvalidHandshake(message.to_owned())));
            return Some(FinalDisposition::Failed(message.to_owned()));
        }
        let maximum = usize::try_from(identity.max_frame_bytes).unwrap_or(MAX_FRAME_BYTES);
        if writer
            .try_send(WriterCommand::SetMaxFrameBytes(maximum))
            .is_err()
        {
            let message = "engine writer is unavailable during handshake";
            let _ = response.send(Err(BridgeError::ProcessUnavailable {
                message: message.to_owned(),
                outcome: DeliveryOutcome::NotSent,
            }));
            return Some(FinalDisposition::Failed(message.to_owned()));
        }
        self.phase = SessionPhase::Ready;
        state.send_replace(EngineState::Ready {
            generation: self.generation,
            identity,
        });
        let _ = response.send(Ok(()));
        None
    }

    fn handle_writer_event(&mut self, event: Option<WriterEvent>) -> Option<FinalDisposition> {
        match event {
            Some(WriterEvent::Closed) | None if self.phase == SessionPhase::Stopping => {
                self.writer_finished = true;
                None
            }
            Some(WriterEvent::Closed) => Some(FinalDisposition::Failed(
                "engine stdin closed unexpectedly".to_owned(),
            )),
            Some(WriterEvent::Failed(message)) => Some(FinalDisposition::Failed(message)),
            None => Some(FinalDisposition::Failed(
                "engine writer task stopped unexpectedly".to_owned(),
            )),
        }
    }

    fn handle_reader_event(&mut self, event: Option<ReaderEvent>) -> Option<FinalDisposition> {
        match event {
            Some(ReaderEvent::Frame(response)) => {
                route_response(response, &mut self.pending, &mut self.retired)
                    .err()
                    .map(FinalDisposition::Failed)
            }
            Some(ReaderEvent::Eof) if self.phase == SessionPhase::Stopping => {
                Some(FinalDisposition::Stopped)
            }
            Some(ReaderEvent::Eof) => Some(FinalDisposition::Crashed),
            Some(ReaderEvent::Failed(error)) => Some(FinalDisposition::Failed(error)),
            None => Some(FinalDisposition::Failed(
                "engine reader task stopped unexpectedly".to_owned(),
            )),
        }
    }

    fn handle_command(
        &mut self,
        command: ActorCommand,
        writer: &mpsc::Sender<WriterCommand>,
        state: &watch::Sender<EngineState>,
    ) -> Option<FinalDisposition> {
        match command {
            ActorCommand::Request(request) => {
                self.accept_request(request, writer, state);
                None
            }
            ActorCommand::PromoteReady { identity, response } => {
                self.promote_ready(identity, response, writer, state)
            }
            ActorCommand::CloseInput => writer
                .try_send(WriterCommand::Close)
                .err()
                .map(|_| FinalDisposition::Failed("engine writer is unavailable".to_owned())),
        }
    }
}

async fn run_actor(context: ActorContext) {
    let ActorContext {
        generation,
        child,
        stdin,
        stdout,
        stderr_task,
        stderr_tail,
        state,
        mut commands,
        mut controls,
        max_in_flight,
        max_receive_frame_bytes,
    } = context;
    let (writer_sender, writer_receiver) = mpsc::channel(max_in_flight);
    let (writer_events_sender, mut writer_events) = mpsc::channel(1);
    let writer_task = tokio::spawn(writer_loop(stdin, writer_receiver, writer_events_sender));
    let (reader_events_sender, mut reader_events) = mpsc::channel(max_in_flight.saturating_add(8));
    let reader_task = tokio::spawn(reader_loop(
        stdout,
        reader_events_sender,
        max_receive_frame_bytes,
    ));
    let (child_control, child_controls) = mpsc::unbounded_channel();
    let (child_events_sender, mut child_events) = mpsc::unbounded_channel();
    let child_task = tokio::spawn(child_loop(child, child_controls, child_events_sender));
    let mut session = ActorSession::new(generation, max_in_flight);
    state.send_replace(EngineState::Handshaking { generation });
    let mut child_status = None;

    let disposition = loop {
        let next_deadline = session.next_deadline();
        tokio::select! {
            biased;
            control = controls.recv() => {
                match control {
                    Some(ActorControl::Retire(request_id)) => session.retire(request_id),
                    Some(ActorControl::Terminate { disposition }) => break disposition,
                    None => break FinalDisposition::Stopped,
                }
            }
            child_event = child_events.recv() => {
                child_status = child_event;
                break if session.phase == SessionPhase::Stopping {
                    FinalDisposition::Stopped
                } else {
                    FinalDisposition::Crashed
                };
            }
            () = wait_for_deadline(next_deadline) => {
                session.expire_requests(Instant::now());
            }
            reader_event = reader_events.recv() => {
                if let Some(disposition) = session.handle_reader_event(reader_event) {
                    break disposition;
                }
            }
            writer_event = writer_events.recv(), if !session.writer_finished => {
                if let Some(disposition) = session.handle_writer_event(writer_event) {
                    break disposition;
                }
            }
            command = commands.recv() => {
                let Some(command) = command else {
                    break FinalDisposition::Stopped;
                };
                if let Some(disposition) =
                    session.handle_command(command, &writer_sender, &state)
                {
                    break disposition;
                }
            }
        }
    };
    drop(commands);
    drop(controls);
    drop(reader_events);
    drop(writer_events);

    ActorCompletion {
        generation,
        child_control,
        child_events,
        child_task,
        child_status,
        writer_sender,
        writer_task,
        reader_task,
        stderr_task,
        stderr_tail,
        state,
        pending: session.pending,
        disposition,
    }
    .finish()
    .await;
}

struct ActorCompletion {
    generation: u64,
    child_control: mpsc::UnboundedSender<ChildControl>,
    child_events: mpsc::UnboundedReceiver<Result<ExitStatus, io::Error>>,
    child_task: JoinHandle<()>,
    child_status: Option<Result<ExitStatus, io::Error>>,
    writer_sender: mpsc::Sender<WriterCommand>,
    writer_task: JoinHandle<()>,
    reader_task: JoinHandle<()>,
    stderr_task: JoinHandle<std::io::Result<()>>,
    stderr_tail: StderrTail,
    state: watch::Sender<EngineState>,
    pending: HashMap<String, PendingRequest>,
    disposition: FinalDisposition,
}

impl ActorCompletion {
    async fn finish(self) {
        let Self {
            generation,
            child_control,
            mut child_events,
            child_task,
            child_status,
            writer_sender,
            writer_task,
            reader_task,
            stderr_task,
            stderr_tail,
            state,
            mut pending,
            disposition,
        } = self;
        let status_result = if let Some(status) = child_status {
            status
        } else {
            let _ = child_control.send(ChildControl::Kill);
            child_events.recv().await.unwrap_or_else(|| {
                Err(io::Error::other(
                    "compatibility-engine child monitor stopped before reporting exit",
                ))
            })
        };
        drop(child_control);
        drop(writer_sender);
        settle_task(child_task).await;
        settle_task(writer_task).await;
        settle_task(reader_task).await;
        settle_task(stderr_task).await;
        let stderr = stderr_tail.snapshot().await;
        let exit = process_exit(status_result, stderr);
        let failure_message = match &disposition {
            FinalDisposition::Stopped => "engine stopped before the request completed",
            FinalDisposition::Crashed => "engine process exited unexpectedly",
            FinalDisposition::Failed(reason) => reason,
        };
        fail_pending(&mut pending, failure_message);

        let final_state = match disposition {
            FinalDisposition::Stopped => EngineState::Stopped { generation, exit },
            FinalDisposition::Crashed => EngineState::Crashed { generation, exit },
            FinalDisposition::Failed(reason) => EngineState::Failed {
                generation,
                reason,
                exit,
            },
        };
        state.send_replace(final_state);
    }
}

async fn wait_for_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => pending::<()>().await,
    }
}

async fn settle_task<T>(mut task: JoinHandle<T>) {
    if timeout(TASK_JOIN_TIMEOUT, &mut task).await.is_err() {
        task.abort();
        let _ = task.await;
    }
}

async fn child_loop(
    mut child: Child,
    mut controls: mpsc::UnboundedReceiver<ChildControl>,
    events: mpsc::UnboundedSender<Result<ExitStatus, io::Error>>,
) {
    let status = loop {
        tokio::select! {
            status = child.wait() => break status,
            control = controls.recv() => {
                if matches!(control, Some(ChildControl::Kill) | None)
                    && let Err(kill_error) = child.start_kill()
                {
                    match child.try_wait() {
                        Ok(Some(status)) => break Ok(status),
                        Ok(None) | Err(_) => break Err(kill_error),
                    }
                }
            }
        }
    };
    let _ = events.send(status);
}

async fn reader_loop<R>(
    mut stdout: R,
    events: mpsc::Sender<ReaderEvent>,
    max_receive_frame_bytes: usize,
) where
    R: AsyncRead + Unpin,
{
    loop {
        let event = match read_frame_with_limit::<_, wire::ServerEnvelope>(
            &mut stdout,
            max_receive_frame_bytes,
        )
        .await
        {
            Ok(Some(frame)) => ReaderEvent::Frame(frame),
            Ok(None) => ReaderEvent::Eof,
            Err(error) => ReaderEvent::Failed(error.to_string()),
        };
        let terminal = !matches!(event, ReaderEvent::Frame(_));
        if events.send(event).await.is_err() || terminal {
            return;
        }
    }
}

fn process_exit(
    status: Result<std::process::ExitStatus, std::io::Error>,
    stderr: StderrSnapshot,
) -> ProcessExit {
    match status {
        Ok(status) => ProcessExit {
            code: status.code(),
            success: status.success(),
            stderr,
        },
        Err(error) => ProcessExit {
            code: None,
            success: false,
            stderr: StderrSnapshot {
                bytes: format!("failed to reap compatibility engine: {error}").into_bytes(),
                ..stderr
            },
        },
    }
}

fn reject_request(
    response: oneshot::Sender<Result<wire::ServerEnvelope, PendingFailure>>,
    message: &str,
) {
    let _ = response.send(Err(PendingFailure::Unavailable {
        message: message.to_owned(),
        outcome: DeliveryOutcome::NotSent,
    }));
}

async fn writer_loop<W>(
    mut stdin: W,
    mut frames: mpsc::Receiver<WriterCommand>,
    events: mpsc::Sender<WriterEvent>,
) where
    W: AsyncWrite + Unpin,
{
    let mut max_frame_bytes = MAX_FRAME_BYTES;
    while let Some(command) = frames.recv().await {
        match command {
            WriterCommand::Frame(frame) => {
                if let Err(error) =
                    write_frame_with_limit(&mut stdin, frame.as_ref(), max_frame_bytes).await
                {
                    let _ = events.send(WriterEvent::Failed(error.to_string())).await;
                    return;
                }
            }
            WriterCommand::SetMaxFrameBytes(maximum) => {
                max_frame_bytes = maximum.min(MAX_FRAME_BYTES);
            }
            WriterCommand::Close => {
                if let Err(error) = stdin.shutdown().await {
                    let _ = events.send(WriterEvent::Failed(error.to_string())).await;
                } else {
                    let _ = events.send(WriterEvent::Closed).await;
                }
                return;
            }
        }
    }
}

fn route_response(
    response: wire::ServerEnvelope,
    pending: &mut HashMap<String, PendingRequest>,
    retired: &mut RetiredRequests,
) -> Result<(), String> {
    let meta = response
        .meta
        .as_ref()
        .ok_or_else(|| "response metadata is missing".to_owned())?;
    if meta.request_id.is_empty() {
        return Err("response request id is empty".to_owned());
    }
    if meta.sequence != 0 {
        return Err(format!(
            "lifecycle response {} used non-zero sequence {}",
            meta.request_id, meta.sequence
        ));
    }
    if !meta.terminal {
        return Err(format!(
            "lifecycle response {} was not terminal",
            meta.request_id
        ));
    }
    let Some(pending_request) = pending.remove(&meta.request_id) else {
        if retired.accept(&meta.request_id) {
            return Ok(());
        }
        return Err(format!(
            "response references unknown request {}",
            meta.request_id
        ));
    };
    if meta.trace_id != pending_request.trace_id {
        return Err(format!(
            "response trace id does not match request {}",
            meta.request_id
        ));
    }
    if response.payload.is_none() {
        return Err(format!("response {} has no payload", meta.request_id));
    }
    let _ = pending_request.response.send(Ok(response));
    Ok(())
}

fn fail_pending(pending: &mut HashMap<String, PendingRequest>, message: &str) {
    for (_, request) in pending.drain() {
        let _ = request.response.send(Err(PendingFailure::Unavailable {
            message: message.to_owned(),
            outcome: DeliveryOutcome::Unknown,
        }));
    }
}

#[derive(Default)]
struct RetiredRequests {
    ids: HashSet<String>,
    order: VecDeque<String>,
}

impl RetiredRequests {
    fn insert(&mut self, request_id: String) {
        if !self.ids.insert(request_id.clone()) {
            return;
        }
        self.order.push_back(request_id);
        while self.order.len() > MAX_RETIRED_REQUESTS {
            if let Some(expired) = self.order.pop_front() {
                self.ids.remove(&expired);
            }
        }
    }

    fn accept(&mut self, request_id: &str) -> bool {
        if self.ids.remove(request_id) {
            self.order.retain(|retired| retired != request_id);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{process::Stdio, time::Duration};

    use tokio::{
        io::duplex,
        process::Command,
        sync::{mpsc, watch},
    };

    use super::{
        ActorContext, EngineState, MAX_FRAME_BYTES, MIN_FRAME_BYTES, StderrTail, WriterCommand,
        WriterEvent, run_actor, writer_loop,
    };
    use chat2db_engine_protocol::wire;

    #[tokio::test]
    async fn writer_applies_negotiated_peer_limit_before_writing() {
        let (writer, _reader) = duplex(MIN_FRAME_BYTES * 2);
        let (commands, command_receiver) = mpsc::channel(2);
        let (events, mut event_receiver) = mpsc::channel(1);
        let writer_task = tokio::spawn(writer_loop(writer, command_receiver, events));
        commands
            .send(WriterCommand::SetMaxFrameBytes(MIN_FRAME_BYTES))
            .await
            .expect("writer command channel must remain open");
        commands
            .send(WriterCommand::Frame(Box::new(wire::ClientEnvelope {
                meta: Some(wire::RequestMeta {
                    request_id: "oversized".to_owned(),
                    trace_id: "oversized".to_owned(),
                    ..Default::default()
                }),
                payload: Some(wire::client_envelope::Payload::Hello(wire::ClientHello {
                    runtime_name: "x".repeat(MIN_FRAME_BYTES * 2),
                    runtime_version: "test".to_owned(),
                    supported_versions: Vec::new(),
                    required_capabilities: Vec::new(),
                    max_receive_frame_bytes: u32::try_from(MAX_FRAME_BYTES).unwrap_or(u32::MAX),
                })),
            })))
            .await
            .expect("oversized frame must enter the writer queue");

        let event = event_receiver
            .recv()
            .await
            .expect("writer must report the rejected frame");
        assert!(matches!(
            event,
            WriterEvent::Failed(message) if message.contains("maximum is 1024")
        ));
        writer_task.await.expect("writer task must join");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stdout_eof_from_a_live_child_is_killed_and_reaped() {
        let mut command = Command::new("sh");
        command
            .args(["-c", "exec 1>&-; sleep 60"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().expect("shell fixture must spawn");
        let stdin = child.stdin.take().expect("stdin must be piped");
        let stdout = child.stdout.take().expect("stdout must be piped");
        let stderr = child.stderr.take().expect("stderr must be piped");
        let stderr_tail = StderrTail::new(1024);
        let drain_tail = stderr_tail.clone();
        let stderr_task = tokio::spawn(async move { drain_tail.drain(stderr).await });
        let (state_sender, mut state) = watch::channel(EngineState::Starting { generation: 1 });
        let (_commands, command_receiver) = mpsc::channel(2);
        let (_controls, control_receiver) = mpsc::unbounded_channel();

        let actor = tokio::spawn(run_actor(ActorContext {
            generation: 1,
            child,
            stdin,
            stdout,
            stderr_task,
            stderr_tail,
            state: state_sender,
            commands: command_receiver,
            controls: control_receiver,
            max_in_flight: 1,
            max_receive_frame_bytes: MAX_FRAME_BYTES,
        }));

        let terminal = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let current = state.borrow().clone();
                if current.is_terminal() {
                    return current;
                }
                state.changed().await.expect("actor state must remain open");
            }
        })
        .await
        .expect("stdout EOF must not leave the actor waiting on a live child");
        assert!(matches!(terminal, EngineState::Crashed { .. }));
        actor.await.expect("actor task must join after reaping");
    }
}
