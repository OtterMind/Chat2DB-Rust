use std::process::ExitStatus;

use chat2db_engine_protocol::{
    MAX_FRAME_BYTES, read_frame_with_limit, wire, write_frame_with_limit,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    process::Child,
    sync::mpsc,
};

use crate::{ProcessExit, StderrSnapshot};

pub(super) enum WriterCommand {
    Frame(Box<wire::ClientEnvelope>),
    SetMaxFrameBytes(usize),
    Close,
}

pub(super) enum WriterEvent {
    Closed,
    Failed(String),
}

pub(super) enum ReaderEvent {
    Frame(Box<wire::ServerEnvelope>),
    Eof,
    Failed(String),
}

pub(super) enum ChildControl {
    Kill,
}

pub(super) async fn child_loop(
    mut child: Child,
    mut controls: mpsc::UnboundedReceiver<ChildControl>,
    events: mpsc::UnboundedSender<Result<ExitStatus, std::io::Error>>,
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

pub(super) async fn reader_loop<R>(
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
            Ok(Some(frame)) => ReaderEvent::Frame(Box::new(frame)),
            Ok(None) => ReaderEvent::Eof,
            Err(error) => ReaderEvent::Failed(error.to_string()),
        };
        let terminal = !matches!(event, ReaderEvent::Frame(_));
        if events.send(event).await.is_err() || terminal {
            return;
        }
    }
}

pub(super) async fn writer_loop<W>(
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

pub(super) fn process_exit(
    status: Result<ExitStatus, std::io::Error>,
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

#[cfg(test)]
mod tests {
    use chat2db_engine_protocol::{MAX_FRAME_BYTES, MIN_FRAME_BYTES, wire};
    use tokio::{io::duplex, sync::mpsc};

    use super::{WriterCommand, WriterEvent, writer_loop};

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
}
