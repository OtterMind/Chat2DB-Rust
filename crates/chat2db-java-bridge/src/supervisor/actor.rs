use std::{
    future::pending,
    io,
    process::ExitStatus,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use chat2db_engine_protocol::{MAX_FRAME_BYTES, wire};
use tokio::{
    process::{Child, ChildStdin, ChildStdout},
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
    time::{Instant, timeout},
};

use crate::{
    BridgeError, DeliveryOutcome, EngineIdentity, EngineState, error::PendingFailure,
    stderr_tail::StderrTail,
};

use super::{
    io::{
        ChildControl, ReaderEvent, WriterCommand, WriterEvent, child_loop, process_exit,
        reader_loop, writer_loop,
    },
    pending::{PendingLane, PendingRequests, PendingSink, fail_sink},
};

const TASK_JOIN_TIMEOUT: Duration = Duration::from_secs(1);

pub(super) enum ActorCommand {
    Request(Box<RequestCommand>),
    PromoteReady {
        identity: EngineIdentity,
        response: oneshot::Sender<Result<(), BridgeError>>,
    },
    CloseInput,
}

pub(super) enum ActorControl {
    Retire(String),
    AbandonStream {
        request_id: String,
        session_id: String,
        cancel: Option<Box<RequestCommand>>,
    },
    Terminate {
        disposition: FinalDisposition,
    },
}

pub(super) struct RequestCommand {
    pub(super) request: wire::ClientEnvelope,
    pub(super) response: PendingSink,
    pub(super) begins_shutdown: bool,
    pub(super) deadline: Instant,
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) lane: PendingLane,
    pub(super) registration: Option<oneshot::Sender<Result<(), PendingFailure>>>,
}

#[derive(Clone, Debug)]
pub(super) enum FinalDisposition {
    Stopped,
    Crashed,
    Failed(String),
}

pub(super) struct ActorContext {
    pub(super) generation: u64,
    pub(super) child: Child,
    pub(super) stdin: ChildStdin,
    pub(super) stdout: ChildStdout,
    pub(super) stderr_task: JoinHandle<std::io::Result<()>>,
    pub(super) stderr_tail: StderrTail,
    pub(super) state: watch::Sender<EngineState>,
    pub(super) commands: mpsc::Receiver<ActorCommand>,
    pub(super) control_commands: mpsc::Receiver<ActorCommand>,
    pub(super) controls: mpsc::UnboundedReceiver<ActorControl>,
    pub(super) max_in_flight: usize,
    pub(super) control_lane_capacity: usize,
    pub(super) registration_ack_delay: Duration,
    pub(super) max_receive_frame_bytes: usize,
}

struct ActorSession {
    generation: u64,
    max_in_flight: usize,
    max_control_in_flight: usize,
    registration_ack_delay: Duration,
    pending: PendingRequests,
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
    fn new(
        generation: u64,
        max_in_flight: usize,
        max_control_in_flight: usize,
        registration_ack_delay: Duration,
    ) -> Self {
        Self {
            generation,
            max_in_flight,
            max_control_in_flight,
            registration_ack_delay,
            pending: PendingRequests::new(),
            phase: SessionPhase::Handshaking,
            writer_finished: false,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn accept_request(
        &mut self,
        command: Box<RequestCommand>,
        writer: &mpsc::Sender<WriterCommand>,
        state: &watch::Sender<EngineState>,
        registration_failures: &mpsc::UnboundedSender<FinalDisposition>,
    ) -> Option<FinalDisposition> {
        let RequestCommand {
            request,
            response,
            begins_shutdown,
            deadline,
            cancelled,
            lane,
            mut registration,
        } = *command;
        if cancelled.load(Ordering::Acquire) {
            return None;
        }
        if self.phase == SessionPhase::Stopping
            || (lane.consumes_normal_slot() && self.pending.normal_count() >= self.max_in_flight)
            || (!lane.consumes_normal_slot()
                && self.pending.control_count() >= self.max_control_in_flight)
        {
            reject_registration(
                registration,
                response,
                "engine is not accepting another request",
            );
            return None;
        }
        let is_handshake = matches!(
            request.payload,
            Some(wire::client_envelope::Payload::Hello(_))
        );
        if (self.phase == SessionPhase::Handshaking && !is_handshake)
            || (self.phase == SessionPhase::Ready && is_handshake)
        {
            reject_registration(
                registration,
                response,
                "request is invalid for the current engine phase",
            );
            return None;
        }
        let Some(meta) = request.meta.as_ref() else {
            reject_registration(registration, response, "request metadata is missing");
            return None;
        };
        if meta.request_id.is_empty() || self.pending.contains(&meta.request_id) {
            reject_registration(
                registration,
                response,
                "request id is empty or already in flight",
            );
            return None;
        }

        let request_id = meta.request_id.clone();
        let trace_id = meta.trace_id.clone();
        if deadline <= Instant::now() {
            let failure = PendingFailure::Timeout {
                request_id,
                outcome: DeliveryOutcome::NotSent,
            };
            if let Some(registration) = registration.take() {
                let _ = registration.send(Err(failure.clone()));
            }
            fail_sink(response, failure);
            return None;
        }
        let session_id = meta.session_id.clone();
        if let Err((response, failure)) = self.pending.insert(
            request_id.clone(),
            trace_id,
            session_id,
            response,
            deadline,
            lane,
        ) {
            if let Some(registration) = registration.take() {
                let _ = registration.send(Err(failure.clone()));
            }
            fail_sink(response, failure);
            return None;
        }
        if cancelled.load(Ordering::Acquire) {
            let failure = PendingFailure::Unavailable {
                message: "request was cancelled before writer enqueue".to_owned(),
                outcome: DeliveryOutcome::NotSent,
            };
            if let Some(registration) = registration.take() {
                let _ = registration.send(Err(failure.clone()));
            }
            self.pending.reject_with_failure(&request_id, failure);
            return None;
        }
        if deadline <= Instant::now() {
            let failure = PendingFailure::Timeout {
                request_id: request_id.clone(),
                outcome: DeliveryOutcome::NotSent,
            };
            if let Some(registration) = registration.take() {
                let _ = registration.send(Err(failure.clone()));
            }
            self.pending.reject_with_failure(&request_id, failure);
            return None;
        }
        if writer
            .try_send(WriterCommand::Frame(Box::new(request)))
            .is_err()
        {
            let failure = PendingFailure::Unavailable {
                message: "engine writer queue is unavailable".to_owned(),
                outcome: DeliveryOutcome::NotSent,
            };
            if let Some(registration) = registration.take() {
                let _ = registration.send(Err(failure.clone()));
            }
            self.pending.reject_with_failure(&request_id, failure);
            return None;
        }
        if let Some(registration) = registration {
            let failure = FinalDisposition::Failed(format!(
                "request {request_id} entered the writer queue but its registration acknowledgement was abandoned"
            ));
            if self.registration_ack_delay.is_zero() {
                if registration.send(Ok(())).is_err() {
                    return Some(failure);
                }
            } else {
                let delay = self.registration_ack_delay;
                let failures = registration_failures.clone();
                let send_failure = failure.clone();
                tokio::spawn(async move {
                    let mut registration = registration;
                    tokio::select! {
                        () = tokio::time::sleep(delay) => {
                            if registration.send(Ok(())).is_err() {
                                let _ = failures.send(send_failure);
                            }
                        }
                        () = registration.closed() => {
                            let _ = failures.send(failure);
                        }
                        () = failures.closed() => {}
                    }
                });
            }
        }
        if begins_shutdown {
            self.phase = SessionPhase::Stopping;
            state.send_replace(EngineState::Stopping {
                generation: self.generation,
            });
        }
        None
    }

    fn retire(&mut self, request_id: String) -> Option<FinalDisposition> {
        self.pending
            .retire(request_id)
            .map(FinalDisposition::Failed)
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.pending.next_deadline()
    }

    fn expire_requests(&mut self, now: Instant) -> Option<FinalDisposition> {
        self.pending.expire(now).map(FinalDisposition::Failed)
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
            Some(ReaderEvent::Frame(response)) => self
                .pending
                .route_response(*response)
                .err()
                .map(FinalDisposition::Failed),
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
        registration_failures: &mpsc::UnboundedSender<FinalDisposition>,
    ) -> Option<FinalDisposition> {
        match command {
            ActorCommand::Request(request) => {
                self.accept_request(request, writer, state, registration_failures)
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

    fn handle_control(
        &mut self,
        control: ActorControl,
        writer: &mpsc::Sender<WriterCommand>,
        state: &watch::Sender<EngineState>,
        registration_failures: &mpsc::UnboundedSender<FinalDisposition>,
    ) -> Option<FinalDisposition> {
        match control {
            ActorControl::Retire(request_id) => self.retire(request_id),
            ActorControl::AbandonStream {
                request_id,
                session_id,
                cancel,
            } => {
                self.pending.abandon_stream(&request_id, &session_id);
                if let Some(cancel) = cancel {
                    return self.accept_request(cancel, writer, state, registration_failures);
                }
                None
            }
            ActorControl::Terminate { disposition } => Some(disposition),
        }
    }
}

fn reject_registration(
    registration: Option<oneshot::Sender<Result<(), PendingFailure>>>,
    response: PendingSink,
    message: &str,
) {
    let failure = PendingFailure::Unavailable {
        message: message.to_owned(),
        outcome: DeliveryOutcome::NotSent,
    };
    if let Some(registration) = registration {
        let _ = registration.send(Err(failure.clone()));
    }
    fail_sink(response, failure);
}

#[allow(clippy::too_many_lines)]
pub(super) async fn run_actor(context: ActorContext) -> Result<(), String> {
    let ActorContext {
        generation,
        child,
        stdin,
        stdout,
        stderr_task,
        stderr_tail,
        state,
        mut commands,
        mut control_commands,
        mut controls,
        max_in_flight,
        control_lane_capacity,
        registration_ack_delay,
        max_receive_frame_bytes,
    } = context;
    let (writer_sender, writer_receiver) =
        mpsc::channel(max_in_flight.saturating_add(control_lane_capacity));
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
    let (registration_failures_sender, mut registration_failures) = mpsc::unbounded_channel();
    let mut session = ActorSession::new(
        generation,
        max_in_flight,
        control_lane_capacity,
        registration_ack_delay,
    );
    state.send_replace(EngineState::Handshaking { generation });
    let mut child_status = None;
    let mut control_commands_open = true;

    let disposition = loop {
        let next_deadline = session.next_deadline();
        tokio::select! {
            biased;
            control = controls.recv() => {
                match control {
                    Some(control) => {
                        if let Some(disposition) =
                            session.handle_control(
                                control,
                                &writer_sender,
                                &state,
                                &registration_failures_sender,
                            )
                        {
                            break disposition;
                        }
                    }
                    None => break FinalDisposition::Stopped,
                }
            }
            child_event = child_events.recv(), if child_status.is_none() => {
                let status = child_event.unwrap_or_else(|| {
                    Err(io::Error::other(
                        "compatibility-engine child monitor stopped before reporting exit",
                    ))
                });
                let drain_shutdown_output =
                    session.phase == SessionPhase::Stopping && status.is_ok();
                child_status = Some(status);
                if !drain_shutdown_output {
                    break if session.phase == SessionPhase::Stopping {
                        FinalDisposition::Stopped
                    } else {
                        FinalDisposition::Crashed
                    };
                }
            }
            () = wait_for_deadline(next_deadline) => {
                if let Some(disposition) = session.expire_requests(Instant::now()) {
                    break disposition;
                }
            }
            registration_failure = registration_failures.recv() => {
                if let Some(disposition) = registration_failure {
                    break disposition;
                }
            }
            control_command = control_commands.recv(), if control_commands_open => {
                match control_command {
                    Some(command) => {
                        if let Some(disposition) =
                            session.handle_command(
                                command,
                                &writer_sender,
                                &state,
                                &registration_failures_sender,
                            )
                        {
                            break disposition;
                        }
                    }
                    None => control_commands_open = false,
                }
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
                    session.handle_command(
                        command,
                        &writer_sender,
                        &state,
                        &registration_failures_sender,
                    )
                {
                    break disposition;
                }
            }
        }
    };
    drop(commands);
    drop(control_commands);
    drop(controls);
    drop(reader_events);
    drop(writer_events);
    drop(registration_failures);
    drop(registration_failures_sender);

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
    .await
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
    pending: PendingRequests,
    disposition: FinalDisposition,
}

impl ActorCompletion {
    async fn finish(self) -> Result<(), String> {
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
        let process_cleanup_error = status_result.as_ref().err().map(ToString::to_string);
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
        pending.fail_all(failure_message);

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
        process_cleanup_error.map_or(Ok(()), Err)
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

#[cfg(test)]
mod tests {
    use std::{process::Stdio, time::Duration};

    use chat2db_engine_protocol::MAX_FRAME_BYTES;
    use tokio::{
        process::Command,
        sync::{mpsc, watch},
    };

    use crate::{EngineState, stderr_tail::StderrTail};

    use super::{ActorContext, run_actor};

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
        let (_control_commands, control_command_receiver) = mpsc::channel(2);
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
            control_commands: control_command_receiver,
            controls: control_receiver,
            max_in_flight: 1,
            control_lane_capacity: 2,
            registration_ack_delay: Duration::ZERO,
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
        actor
            .await
            .expect("actor task must join after reaping")
            .expect("actor must reap the child successfully");
    }
}
