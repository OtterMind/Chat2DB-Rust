use std::process::ExitStatus;

use chat2db_engine_protocol::{
    MAX_FRAME_BYTES, read_frame_payload_with_limit, wire, write_frame_with_limit,
};
use prost::Message;
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
        let event = match read_frame_payload_with_limit(&mut stdout, max_receive_frame_bytes).await
        {
            Ok(Some(payload)) => match decode_server_envelope(&payload) {
                Ok(frame) => ReaderEvent::Frame(Box::new(frame)),
                Err(error) => ReaderEvent::Failed(error),
            },
            Ok(None) => ReaderEvent::Eof,
            Err(error) => ReaderEvent::Failed(error.to_string()),
        };
        let terminal = !matches!(event, ReaderEvent::Frame(_));
        if events.send(event).await.is_err() || terminal {
            return;
        }
    }
}

fn decode_server_envelope(payload: &[u8]) -> Result<wire::ServerEnvelope, String> {
    wire::ServerEnvelope::decode(payload)
        .map_err(|error| format!("process frame Protobuf decode failed: {error}"))
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
