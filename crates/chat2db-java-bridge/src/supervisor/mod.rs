use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chat2db_engine_protocol::{MAX_FRAME_BYTES, MIN_FRAME_BYTES, current_version, wire};
use prost::Message;
use tokio::{
    sync::{Mutex, mpsc, oneshot, watch},
    task::JoinHandle,
    time::{Instant, timeout, timeout_at},
};

use crate::{
    BridgeError, DeliveryOutcome, EngineCommand, EngineIdentity, EngineState, PingReply,
    ProcessExit, StderrSnapshot, state::SessionStateCell, stderr_tail::StderrTail,
};

use self::actor::{
    ActorCommand, ActorContext, ActorControl, FinalDisposition, RequestCommand, run_actor,
};
use self::{
    jdbc::EngineBinding,
    pending::{ControlEffect, PendingLane, PendingSink, QueryBudgets},
};

mod actor;
mod community;
mod io;
mod jdbc;
mod pending;

pub use community::{
    BuildCommunityDmlRequest, BuildCommunityNamespaceSqlRequest, COMMUNITY_DML_BUILDER_CAPABILITY,
    COMMUNITY_NAMESPACE_BUILDER_CAPABILITY, COMMUNITY_OBJECT_METADATA_CAPABILITY,
    COMMUNITY_PLUGIN_CATALOG_CAPABILITY, COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY,
    COMMUNITY_RELATION_METADATA_CAPABILITY, COMMUNITY_SCHEMA_METADATA_CAPABILITY,
    COMMUNITY_SQL_BUILDER_CAPABILITY, COMMUNITY_SQL_COMPLETION_CAPABILITY,
    COMMUNITY_SQL_FORMATTER_CAPABILITY, COMMUNITY_SQL_PARSER_CAPABILITY,
    COMMUNITY_SQL_VALIDATION_CAPABILITY, CommunityClasspath, CommunityClient, CommunityDatabase,
    CommunityDmlAssignment, CommunityDmlColumn, CommunityDmlRow, CommunityDmlStatement,
    CommunityDmlTarget, CommunityDmlTemporal, CommunityDmlTemporalKind, CommunityDmlValue,
    CommunityDriverConfig, CommunityForeignKey, CommunityFormattedSql, CommunityFunction,
    CommunityFunctionParameter, CommunityNamespaceSqlOperation, CommunityParsedStatement,
    CommunityPlugin, CommunityPluginBehavior, CommunityPluginCatalog, CommunityPluginServices,
    CommunityPrimaryKey, CommunityProcedure, CommunityProcedureParameter, CommunitySchema,
    CommunitySqlAnalysis, CommunitySqlCompletion, CommunitySqlCompletionActiveSnippetSlot,
    CommunitySqlCompletionCandidate, CommunitySqlCompletionEditorHint,
    CommunitySqlCompletionEditorHintItem, CommunitySqlCompletionRange, CommunitySqlDiagnostic,
    CommunitySqlValidation, CommunityTable, CommunityTableColumn, CommunityTableIndex,
    CommunityTableIndexColumn, CommunityTrigger, CompleteCommunitySqlRequest,
};

pub use jdbc::{
    CancelDisposition, ColumnNullability, ConnectionProperty, DRIVER_EXTERNAL_JAR_CAPABILITY,
    DatabaseProduct, DriverArtifact, DriverClient, DriverSpec, FLOW_CREDIT_CAPABILITY, JdbcColumn,
    JdbcParameter, JdbcRow, JdbcValue, JdbcValueType, LoadedDriver, MAX_DRIVER_ARTIFACT_BYTES,
    MAX_DRIVER_ARTIFACTS, MAX_DRIVER_TOTAL_BYTES, OPERATION_CANCEL_CAPABILITY,
    QUERY_TYPED_BATCHES_CAPABILITY, QueryCompleted, QueryEvent, QueryOptions, QueryRequest,
    QueryStarted, QueryStream, RowBatch, SESSION_JDBC_CAPABILITY, Session, SessionConfig,
    TRANSACTION_LOCAL_CAPABILITY, Transaction, TransactionIsolation, TransactionOptions,
    UPDATE_JDBC_CAPABILITY, UpdateRequest, UpdateResult,
};

const PING_CAPABILITY: &str = "lifecycle.ping.v1";
const SHUTDOWN_CAPABILITY: &str = "lifecycle.shutdown.v1";
const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_IN_FLIGHT: usize = 256;
const DEFAULT_STDERR_TAIL_BYTES: usize = 64 * 1024;
const DEFAULT_CONTROL_LANE_CAPACITY: usize = 16;
const DEFAULT_STREAM_EVENT_CAPACITY: usize = 34;
const JDBC_SNAPSHOT_ROOT_ENV: &str = "CHAT2DB_JDBC_SNAPSHOT_ROOT";
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
    stream_event_capacity: usize,
    stderr_tail_bytes: usize,
    registration_ack_delay: Duration,
    driver_snapshot_parent: Option<PathBuf>,
    community_classpath: Option<CommunityClasspath>,
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
            stream_event_capacity: DEFAULT_STREAM_EVENT_CAPACITY,
            stderr_tail_bytes: DEFAULT_STDERR_TAIL_BYTES,
            registration_ack_delay: Duration::ZERO,
            driver_snapshot_parent: None,
            community_classpath: None,
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

    /// Overrides the per-query bounded event channel capacity.
    #[must_use]
    pub const fn with_stream_event_capacity(mut self, capacity: usize) -> Self {
        self.stream_event_capacity = capacity;
        self
    }

    /// Overrides the maximum frame size advertised to the engine.
    #[must_use]
    pub const fn with_max_receive_frame_bytes(mut self, maximum: u32) -> Self {
        self.max_receive_frame_bytes = maximum;
        self
    }

    /// Places generation-owned JDBC snapshots below an application-private directory.
    #[must_use]
    pub fn with_driver_snapshot_parent(mut self, parent: impl Into<PathBuf>) -> Self {
        self.driver_snapshot_parent = Some(parent.into());
        self
    }

    /// Supplies the fixed Community compatibility classpath for this process.
    #[must_use]
    pub fn with_community_classpath(mut self, classpath: CommunityClasspath) -> Self {
        for capability in [
            COMMUNITY_PLUGIN_CATALOG_CAPABILITY,
            COMMUNITY_SCHEMA_METADATA_CAPABILITY,
            COMMUNITY_OBJECT_METADATA_CAPABILITY,
            COMMUNITY_RELATION_METADATA_CAPABILITY,
            COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY,
            COMMUNITY_SQL_BUILDER_CAPABILITY,
            COMMUNITY_SQL_PARSER_CAPABILITY,
            COMMUNITY_SQL_VALIDATION_CAPABILITY,
            COMMUNITY_SQL_FORMATTER_CAPABILITY,
            COMMUNITY_SQL_COMPLETION_CAPABILITY,
            COMMUNITY_DML_BUILDER_CAPABILITY,
            COMMUNITY_NAMESPACE_BUILDER_CAPABILITY,
        ] {
            if !self
                .required_capabilities
                .iter()
                .any(|required| required == capability)
            {
                self.required_capabilities.push(capability.to_owned());
            }
        }
        self.community_classpath = Some(classpath);
        self
    }

    #[cfg(feature = "test-fixture")]
    #[doc(hidden)]
    #[must_use]
    pub const fn with_registration_ack_delay_for_test(mut self, delay: Duration) -> Self {
        self.registration_ack_delay = delay;
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
        if self.stream_event_capacity < 2 {
            return Err(BridgeError::InvalidConfig(
                "stream_event_capacity must reserve query-started and terminal slots".to_owned(),
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

    fn build_process_command(
        &self,
        snapshot_root: &DriverSnapshotRoot,
    ) -> Result<(tokio::process::Command, String), BridgeError> {
        let community_snapshot = self
            .community_classpath
            .as_ref()
            .map(|classpath| classpath.snapshot_into(&snapshot_root.canonical_path))
            .transpose()?;
        let community_source_commit = self
            .community_classpath
            .as_ref()
            .map_or_else(String::new, |classpath| {
                classpath.source_commit().to_owned()
            });

        let mut command = self.command.build();
        command
            .env(JDBC_SNAPSHOT_ROOT_ENV, &snapshot_root.wire_path)
            .env_remove(community::COMMUNITY_CLASSPATH_ENV)
            .env_remove(community::COMMUNITY_SOURCE_COMMIT_ENV)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(snapshot) = community_snapshot {
            command.env(community::COMMUNITY_CLASSPATH_ENV, snapshot);
            command.env(
                community::COMMUNITY_SOURCE_COMMIT_ENV,
                &community_source_commit,
            );
        }
        Ok((command, community_source_commit))
    }
}

/// Single owner of one supervised Java process generation.
pub struct EngineSupervisor {
    client: EngineClient,
    control: mpsc::UnboundedSender<ActorControl>,
    actor_task: Mutex<Option<JoinHandle<Result<(), BridgeError>>>>,
}

struct DriverSnapshotRoot {
    directory: tempfile::TempDir,
    canonical_path: PathBuf,
    wire_path: String,
}

impl DriverSnapshotRoot {
    fn create(parent: Option<&Path>, generation: u64) -> Result<Self, BridgeError> {
        let mut builder = tempfile::Builder::new();
        let prefix = format!("engine-{generation}-");
        builder.prefix(&prefix);
        let directory = parent
            .map_or_else(|| builder.tempdir(), |path| builder.tempdir_in(path))
            .map_err(|source| BridgeError::DriverSnapshotDirectory {
                operation: "create",
                path: parent.map_or_else(std::env::temp_dir, Path::to_path_buf),
                source,
            })?;
        let (canonical_path, wire_path) = match Self::resolve(&directory) {
            Ok(paths) => paths,
            Err(primary) => {
                let cleanup_path = directory.path().to_path_buf();
                let cleanup = close_snapshot_directory(directory, cleanup_path);
                return Err(attach_cleanup_error(primary, cleanup));
            }
        };
        Ok(Self {
            directory,
            canonical_path,
            wire_path,
        })
    }

    fn resolve(directory: &tempfile::TempDir) -> Result<(PathBuf, String), BridgeError> {
        let canonical_path = std::fs::canonicalize(directory.path()).map_err(|source| {
            BridgeError::DriverSnapshotDirectory {
                operation: "resolve",
                path: directory.path().to_path_buf(),
                source,
            }
        })?;
        let canonical_text =
            canonical_path
                .to_str()
                .ok_or_else(|| BridgeError::DriverSnapshotDirectory {
                    operation: "encode",
                    path: canonical_path.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "snapshot path is not valid UTF-8",
                    ),
                })?;
        let wire_path = jdbc::driver_artifact_wire_path(canonical_text)?;
        Ok((canonical_path, wire_path))
    }

    fn close(self) -> Result<(), BridgeError> {
        close_snapshot_directory(self.directory, self.canonical_path)
    }

    fn close_after(self, primary: BridgeError) -> BridgeError {
        attach_cleanup_error(primary, self.close())
    }

    fn retain_after_process_failure(self, message: String) -> BridgeError {
        let retained_snapshot = self.retain();
        BridgeError::ProcessCleanup {
            retained_snapshot,
            message,
        }
    }

    fn retain(self) -> PathBuf {
        let Self {
            directory,
            canonical_path,
            ..
        } = self;
        let _ = directory.keep();
        canonical_path
    }
}

fn close_snapshot_directory(
    directory: tempfile::TempDir,
    path: PathBuf,
) -> Result<(), BridgeError> {
    directory
        .close()
        .map_err(|source| BridgeError::DriverSnapshotDirectory {
            operation: "clean",
            path,
            source,
        })
}

struct SpawnedProcess {
    child: tokio::process::Child,
    process_id: Option<u32>,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
    community_source_commit: String,
    snapshot_root: DriverSnapshotRoot,
}

fn attach_cleanup_error(primary: BridgeError, cleanup: Result<(), BridgeError>) -> BridgeError {
    match cleanup {
        Ok(()) => primary,
        Err(cleanup) => BridgeError::CleanupAfterFailure {
            primary: Box::new(primary),
            cleanup: Box::new(cleanup),
        },
    }
}

fn resolve_with_cleanup<T>(
    result: Result<T, BridgeError>,
    cleanup: Result<(), BridgeError>,
) -> Result<T, BridgeError> {
    match result {
        Ok(value) => cleanup.map(|()| value),
        Err(primary) => Err(attach_cleanup_error(primary, cleanup)),
    }
}

async fn join_actor_task(task: JoinHandle<Result<(), BridgeError>>) -> Result<(), BridgeError> {
    task.await
        .map_err(|error| BridgeError::SupervisorTask(error.to_string()))?
}

async fn resolve_start_failure(
    primary: BridgeError,
    actor_task: JoinHandle<Result<(), BridgeError>>,
) -> BridgeError {
    attach_cleanup_error(primary, join_actor_task(actor_task).await)
}

async fn supervise_actor_task(
    actor_task: JoinHandle<Result<(), String>>,
    snapshot_root: DriverSnapshotRoot,
) -> Result<(), BridgeError> {
    match actor_task.await {
        Ok(Ok(())) => snapshot_root.close(),
        Ok(Err(message)) => Err(snapshot_root.retain_after_process_failure(message)),
        Err(error) => {
            let retained_snapshot = snapshot_root.retain();
            Err(BridgeError::SupervisorTask(format!(
                "{error}; retained generation snapshot {} because child reap was not proven",
                retained_snapshot.display()
            )))
        }
    }
}

async fn terminate_child_before_actor(child: &mut tokio::process::Child) -> Result<(), String> {
    if let Err(kill_error) = child.start_kill() {
        return match child.try_wait() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(format!(
                "failed to kill compatibility engine before supervisor startup: {kill_error}"
            )),
            Err(status_error) => Err(format!(
                "failed to kill compatibility engine before supervisor startup: {kill_error}; status check failed: {status_error}"
            )),
        };
    }
    child.wait().await.map(|_| ()).map_err(|error| {
        format!("failed to reap compatibility engine before supervisor startup: {error}")
    })
}

async fn spawn_process(
    config: &EngineConfig,
    generation: u64,
) -> Result<SpawnedProcess, BridgeError> {
    let snapshot_root =
        DriverSnapshotRoot::create(config.driver_snapshot_parent.as_deref(), generation)?;
    let (mut command, community_source_commit) = match config.build_process_command(&snapshot_root)
    {
        Ok(command) => command,
        Err(error) => return Err(snapshot_root.close_after(error)),
    };
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(source) => return Err(snapshot_root.close_after(BridgeError::Spawn(source))),
    };
    let process_id = child.id();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (stdin, stdout, stderr) = match (stdin, stdout, stderr) {
        (Some(stdin), Some(stdout), Some(stderr)) => (stdin, stdout, stderr),
        (stdin, stdout, stderr) => {
            let missing = if stdin.is_none() {
                "stdin"
            } else if stdout.is_none() {
                "stdout"
            } else {
                debug_assert!(stderr.is_none());
                "stderr"
            };
            let cleanup = match terminate_child_before_actor(&mut child).await {
                Ok(()) => snapshot_root.close(),
                Err(message) => Err(snapshot_root.retain_after_process_failure(message)),
            };
            return Err(attach_cleanup_error(
                BridgeError::MissingPipe(missing),
                cleanup,
            ));
        }
    };
    Ok(SpawnedProcess {
        child,
        process_id,
        stdin,
        stdout,
        stderr,
        community_source_commit,
        snapshot_root,
    })
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
        let SpawnedProcess {
            child,
            process_id,
            stdin,
            stdout,
            stderr,
            community_source_commit,
            snapshot_root,
        } = spawn_process(&config, generation).await?;

        let stderr_tail = StderrTail::new(config.stderr_tail_bytes);
        let drain_tail = stderr_tail.clone();
        let stderr_task = tokio::spawn(async move { drain_tail.drain(stderr).await });
        let (state_sender, state_receiver) = watch::channel(EngineState::Starting { generation });
        let channel_capacity = config.max_in_flight.saturating_add(8);
        let (command_sender, command_receiver) = mpsc::channel(channel_capacity);
        let (control_command_sender, control_command_receiver) =
            mpsc::channel(DEFAULT_CONTROL_LANE_CAPACITY);
        let (control_sender, control_receiver) = mpsc::unbounded_channel();
        let actor_stderr_tail = stderr_tail.clone();

        let actor = tokio::spawn(run_actor(ActorContext {
            generation,
            child,
            stdin,
            stdout,
            stderr_task,
            stderr_tail: actor_stderr_tail,
            state: state_sender,
            commands: command_receiver,
            control_commands: control_command_receiver,
            controls: control_receiver,
            max_in_flight: config.max_in_flight,
            control_lane_capacity: DEFAULT_CONTROL_LANE_CAPACITY,
            registration_ack_delay: config.registration_ack_delay,
            max_receive_frame_bytes: usize::try_from(config.max_receive_frame_bytes)
                .unwrap_or(MAX_FRAME_BYTES),
        }));
        let actor_task = tokio::spawn(supervise_actor_task(actor, snapshot_root));

        let inner = Arc::new(EngineInner {
            generation,
            process_id,
            commands: command_sender,
            control_commands: control_command_sender,
            control: control_sender.clone(),
            state: state_receiver,
            request_counter: AtomicU64::new(1),
            request_timeout: config.request_timeout,
            shutdown_timeout: config.shutdown_timeout,
            stream_event_capacity: config.stream_event_capacity,
            community_source_commit,
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
                return Err(resolve_start_failure(public_error, actor_task).await);
            }
        };

        if let Err(error) = client.promote_ready(identity).await {
            client.terminate_start_failure(error.to_string()).await;
            return Err(resolve_start_failure(error, actor_task).await);
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
        resolve_with_cleanup(result, self.join_actor().await)
    }

    async fn join_actor(&self) -> Result<(), BridgeError> {
        let Some(task) = self.actor_task.lock().await.take() else {
            return Ok(());
        };
        join_actor_task(task).await
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
    control_commands: mpsc::Sender<ActorCommand>,
    control: mpsc::UnboundedSender<ActorControl>,
    state: watch::Receiver<EngineState>,
    request_counter: AtomicU64,
    request_timeout: Duration,
    shutdown_timeout: Duration,
    stream_event_capacity: usize,
    community_source_commit: String,
    stderr_tail: StderrTail,
    shutdown_lock: Mutex<()>,
}

impl EngineClient {
    /// Returns the current process state.
    #[must_use]
    pub fn state(&self) -> EngineState {
        self.inner.state.borrow().clone()
    }

    /// Returns whether this generation was started with a fixed Community
    /// compatibility classpath.
    #[must_use]
    pub fn community_compatibility_configured(&self) -> bool {
        !self.inner.community_source_commit.is_empty()
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

        // The process may exit immediately after flushing the ack. In that case the actor can
        // reach EOF and close this channel before the client resumes; the terminal state is the
        // authoritative shutdown result.
        let _ = self.inner.commands.send(ActorCommand::CloseInput).await;
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
        self.send_request_inner(
            payload,
            None,
            None,
            request_timeout,
            begins_shutdown,
            PendingLane::Retireable,
        )
        .await
    }

    async fn send_bound_request(
        &self,
        binding: &EngineBinding,
        capability: &str,
        session_id: Option<&str>,
        session_state: Option<Arc<SessionStateCell>>,
        payload: wire::client_envelope::Payload,
        lane: PendingLane,
    ) -> Result<wire::ServerEnvelope, BridgeError> {
        self.ensure_bound_capability(binding, capability)?;
        self.send_request_inner(
            payload,
            session_id,
            session_state,
            self.inner.request_timeout,
            false,
            lane,
        )
        .await
    }

    async fn send_request_inner(
        &self,
        payload: wire::client_envelope::Payload,
        session_id: Option<&str>,
        session_state: Option<Arc<SessionStateCell>>,
        request_timeout: Duration,
        begins_shutdown: bool,
        lane: PendingLane,
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
                session_id: session_id.map(str::to_owned),
                deadline_unix_millis: Some(deadline_unix_millis),
                ..Default::default()
            }),
            payload: Some(payload),
        };
        self.validate_outbound_frame(&request)?;
        let (response_sender, response_receiver) = oneshot::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let control_lane = matches!(lane, PendingLane::Control(_));
        let command = ActorCommand::Request(Box::new(RequestCommand {
            request,
            response: PendingSink::Unary {
                response: response_sender,
                session_state,
            },
            begins_shutdown,
            deadline,
            cancelled: cancelled.clone(),
            lane,
            registration: None,
        }));
        let sender = if control_lane {
            &self.inner.control_commands
        } else {
            &self.inner.commands
        };

        match timeout_at(deadline, sender.send(command)).await {
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
            return match crate::RemoteEngineError::try_from(error) {
                Ok(error) => Err(error.into()),
                Err(message) => self.protocol_violation(message).await,
            };
        }
        Ok(response)
    }

    async fn start_bound_query(
        &self,
        binding: &EngineBinding,
        session_id: &str,
        request: wire::ExecuteQueryRequest,
        initial_credits: u32,
        session_state: Arc<SessionStateCell>,
    ) -> Result<QueryStream, BridgeError> {
        self.ensure_bound_capability(binding, QUERY_TYPED_BATCHES_CAPABILITY)?;
        if initial_credits > 0 {
            self.ensure_bound_capability(binding, FLOW_CREDIT_CAPABILITY)?;
        }

        let request_number = self.inner.request_counter.fetch_add(1, Ordering::Relaxed);
        let request_id = format!("{}-{request_number}", self.inner.generation);
        let trace_id = format!("engine-{request_id}");
        let deadline = Instant::now() + self.inner.request_timeout;
        let budgets = request
            .options
            .as_ref()
            .map(QueryBudgets::from_options)
            .ok_or_else(|| BridgeError::InvalidRequest("query options are required".to_owned()))?;
        let (events_sender, events_receiver) = mpsc::channel(self.inner.stream_event_capacity);
        let (registration_sender, registration_receiver) = oneshot::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut stream = QueryStream::new(
            self.clone(),
            binding.clone(),
            session_id.to_owned(),
            request_id.clone(),
            events_receiver,
            session_state.clone(),
            cancelled.clone(),
        );
        let envelope = wire::ClientEnvelope {
            meta: Some(wire::RequestMeta {
                request_id: request_id.clone(),
                trace_id,
                session_id: Some(session_id.to_owned()),
                deadline_unix_millis: Some(unix_millis_after(self.inner.request_timeout)),
                ..Default::default()
            }),
            payload: Some(wire::client_envelope::Payload::ExecuteQuery(request)),
        };
        if let Err(error) = self.validate_outbound_frame(&envelope) {
            stream.disarm();
            return Err(error);
        }
        let command = ActorCommand::Request(Box::new(RequestCommand {
            request: envelope,
            response: PendingSink::Stream {
                events: events_sender,
                event_capacity: self.inner.stream_event_capacity,
                initial_credits,
                session_state,
                budgets,
            },
            begins_shutdown: false,
            deadline,
            cancelled,
            lane: PendingLane::Stream,
            registration: Some(registration_sender),
        }));

        match timeout_at(deadline, self.inner.commands.send(command)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                stream.disarm();
                return Err(BridgeError::CommandChannelClosed {
                    outcome: DeliveryOutcome::NotSent,
                });
            }
            Err(_) => {
                stream.disarm();
                return Err(BridgeError::RequestTimeout {
                    request_id,
                    outcome: DeliveryOutcome::NotSent,
                });
            }
        }

        match timeout_at(deadline, registration_receiver).await {
            Ok(Ok(Ok(()))) => Ok(stream),
            Ok(Ok(Err(failure))) => {
                stream.disarm();
                Err(failure.into_bridge_error())
            }
            Ok(Err(_)) => {
                let error = BridgeError::CommandChannelClosed {
                    outcome: DeliveryOutcome::Unknown,
                };
                self.terminate_protocol_failure(error.to_string()).await;
                stream.disarm();
                Err(error)
            }
            Err(_) => {
                let error = BridgeError::RequestTimeout {
                    request_id,
                    outcome: DeliveryOutcome::Unknown,
                };
                self.terminate_protocol_failure(error.to_string()).await;
                stream.disarm();
                Err(error)
            }
        }
    }

    fn best_effort_abandon_query(
        &self,
        binding: &EngineBinding,
        session_id: &str,
        request_id: &str,
        request_cancelled: &AtomicBool,
        session_state: &Arc<SessionStateCell>,
    ) {
        request_cancelled.store(true, Ordering::Release);
        let cancel = self
            .build_drop_cancel(binding, session_id, request_id, session_state)
            .map(Box::new);
        let _ = self.inner.control.send(ActorControl::AbandonStream {
            request_id: request_id.to_owned(),
            session_id: session_id.to_owned(),
            cancel,
        });
    }

    fn build_drop_cancel(
        &self,
        binding: &EngineBinding,
        session_id: &str,
        target_request_id: &str,
        session_state: &Arc<SessionStateCell>,
    ) -> Option<RequestCommand> {
        self.ensure_bound_capability(binding, OPERATION_CANCEL_CAPABILITY)
            .ok()?;
        let request_number = self.inner.request_counter.fetch_add(1, Ordering::Relaxed);
        let request_id = format!("{}-{request_number}", self.inner.generation);
        let trace_id = format!("engine-{request_id}");
        let deadline = Instant::now() + self.inner.request_timeout;
        let (response, receiver) = oneshot::channel();
        drop(receiver);
        Some(RequestCommand {
            request: wire::ClientEnvelope {
                meta: Some(wire::RequestMeta {
                    request_id,
                    trace_id,
                    session_id: Some(session_id.to_owned()),
                    deadline_unix_millis: Some(unix_millis_after(self.inner.request_timeout)),
                    ..Default::default()
                }),
                payload: Some(wire::client_envelope::Payload::CancelOperation(
                    wire::CancelOperationRequest {
                        target_request_id: target_request_id.to_owned(),
                        reason: Some("query stream dropped by host".to_owned()),
                    },
                )),
            },
            response: PendingSink::Unary {
                response,
                session_state: Some(session_state.clone()),
            },
            begins_shutdown: false,
            deadline,
            cancelled: Arc::new(AtomicBool::new(false)),
            lane: PendingLane::Control(ControlEffect::Cancel {
                target_request_id: target_request_id.to_owned(),
            }),
            registration: None,
        })
    }

    fn capture_binding(&self) -> Result<EngineBinding, BridgeError> {
        match self.state() {
            EngineState::Ready {
                generation,
                identity,
            } => Ok(EngineBinding {
                generation,
                engine_instance_id: identity.instance_id,
            }),
            state => Err(BridgeError::NotReady {
                state: state.label(),
            }),
        }
    }

    fn validate_outbound_frame(&self, request: &wire::ClientEnvelope) -> Result<(), BridgeError> {
        let EngineState::Ready { identity, .. } = self.state() else {
            return Ok(());
        };
        let maximum = usize::try_from(identity.max_frame_bytes).unwrap_or(usize::MAX);
        let encoded = request.encoded_len();
        if encoded > maximum {
            return Err(BridgeError::InvalidRequest(format!(
                "encoded request is {encoded} bytes and exceeds the peer frame limit {maximum}"
            )));
        }
        Ok(())
    }

    fn ensure_bound_capability(
        &self,
        binding: &EngineBinding,
        capability: &str,
    ) -> Result<(), BridgeError> {
        let EngineState::Ready {
            generation,
            identity,
        } = self.state()
        else {
            return Err(BridgeError::StaleHandle(
                "engine generation is no longer ready".to_owned(),
            ));
        };
        if generation != binding.generation || identity.instance_id != binding.engine_instance_id {
            return Err(BridgeError::StaleHandle(format!(
                "expected generation {} instance {}, got generation {generation} instance {}",
                binding.generation, binding.engine_instance_id, identity.instance_id
            )));
        }
        if !identity
            .capabilities
            .iter()
            .any(|provided| provided == capability)
        {
            return Err(BridgeError::MissingCapability(capability.to_owned()));
        }
        Ok(())
    }

    async fn protocol_violation<T>(&self, message: impl Into<String>) -> Result<T, BridgeError> {
        let error = BridgeError::Protocol(message.into());
        self.terminate_protocol_failure(error.to_string()).await;
        Err(error)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn startup_failure_preserves_snapshot_cleanup_failure() {
        let cleanup_path = PathBuf::from("fixture-snapshot");
        let task_path = cleanup_path.clone();
        let actor_task = tokio::spawn(async move {
            Err(BridgeError::DriverSnapshotDirectory {
                operation: "clean",
                path: task_path,
                source: std::io::Error::other("fixture cleanup failure"),
            })
        });

        let error = resolve_start_failure(
            BridgeError::InvalidHandshake("fixture handshake failure".to_owned()),
            actor_task,
        )
        .await;
        let BridgeError::CleanupAfterFailure { primary, cleanup } = error else {
            panic!("startup and cleanup failures must both be retained");
        };
        assert!(matches!(
            *primary,
            BridgeError::InvalidHandshake(message) if message == "fixture handshake failure"
        ));
        assert!(matches!(
            *cleanup,
            BridgeError::DriverSnapshotDirectory {
                operation: "clean",
                path,
                ..
            } if path == cleanup_path
        ));
    }

    #[tokio::test]
    async fn actor_join_failure_is_reported_as_a_supervisor_task_error() {
        let actor_task = tokio::spawn(std::future::pending::<Result<(), BridgeError>>());
        actor_task.abort();

        assert!(matches!(
            join_actor_task(actor_task).await,
            Err(BridgeError::SupervisorTask(_))
        ));
    }

    #[tokio::test]
    async fn process_cleanup_failure_retains_the_generation_snapshot() {
        let parent = tempfile::tempdir().expect("fixture parent must exist");
        let snapshot = DriverSnapshotRoot::create(Some(parent.path()), 1)
            .expect("fixture snapshot must exist");
        let expected_path = snapshot.canonical_path.clone();
        let actor_task =
            tokio::spawn(async { Err("fixture child could not be reaped".to_owned()) });

        let error = supervise_actor_task(actor_task, snapshot)
            .await
            .expect_err("process cleanup failure must surface");
        let BridgeError::ProcessCleanup {
            retained_snapshot,
            message,
        } = error
        else {
            panic!("process cleanup failure must retain its dedicated error type");
        };
        assert_eq!(retained_snapshot, expected_path);
        assert_eq!(message, "fixture child could not be reaped");
        assert!(retained_snapshot.is_dir());
        std::fs::remove_dir_all(retained_snapshot)
            .expect("retained fixture snapshot must be removed");
    }

    #[test]
    fn shutdown_failure_preserves_a_concurrent_actor_cleanup_failure() {
        let cleanup_path = PathBuf::from("fixture-shutdown-snapshot");
        let result: Result<(), BridgeError> = Err(BridgeError::ShutdownTimeout);
        let cleanup = Err(BridgeError::DriverSnapshotDirectory {
            operation: "clean",
            path: cleanup_path.clone(),
            source: std::io::Error::other("fixture cleanup failure"),
        });

        let error = resolve_with_cleanup(result, cleanup)
            .expect_err("shutdown and cleanup failures must both surface");
        let BridgeError::CleanupAfterFailure { primary, cleanup } = error else {
            panic!("shutdown and cleanup failures must use the composite error");
        };
        assert!(matches!(*primary, BridgeError::ShutdownTimeout));
        assert!(matches!(
            *cleanup,
            BridgeError::DriverSnapshotDirectory {
                operation: "clean",
                path,
                ..
            } if path == cleanup_path
        ));
    }
}
