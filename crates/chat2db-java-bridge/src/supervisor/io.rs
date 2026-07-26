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

const MAX_COMMUNITY_RESPONSE_BYTES: usize = wire::CommunityByteLimit::MaxResponseBytes as usize;
const COMMUNITY_RESPONSE_TAGS: std::ops::RangeInclusive<u32> = 200..=203;
const MAX_PROTOBUF_FIELD_NUMBER: u64 = (1 << 29) - 1;
const MAX_PROTOBUF_GROUP_DEPTH: usize = 100;

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
    validate_community_response_wire_budget(payload)?;
    wire::ServerEnvelope::decode(payload)
        .map_err(|error| format!("process frame Protobuf decode failed: {error}"))
}

fn validate_community_response_wire_budget(payload: &[u8]) -> Result<(), String> {
    let mut cursor = 0;
    let mut community_bytes = 0_usize;
    while cursor < payload.len() {
        let (field_number, wire_type) = read_key(payload, &mut cursor)?;
        if COMMUNITY_RESPONSE_TAGS.contains(&field_number) && wire_type != 2 {
            return Err(format!(
                "Community response field {field_number} used non-length-delimited wire type {wire_type}"
            ));
        }
        if wire_type == 2 {
            let length = read_length(payload, &mut cursor)?;
            if COMMUNITY_RESPONSE_TAGS.contains(&field_number) {
                community_bytes = community_bytes
                    .checked_add(length)
                    .ok_or_else(|| "Community response wire byte count overflowed".to_owned())?;
                if community_bytes > MAX_COMMUNITY_RESPONSE_BYTES {
                    return Err(format!(
                        "Community response wire payloads total {community_bytes} bytes; maximum is {MAX_COMMUNITY_RESPONSE_BYTES}"
                    ));
                }
            }
            advance(payload, &mut cursor, length)?;
        } else {
            skip_wire_value(payload, &mut cursor, field_number, wire_type, 0)?;
        }
    }
    Ok(())
}

fn read_key(payload: &[u8], cursor: &mut usize) -> Result<(u32, u8), String> {
    let key = read_varint(payload, cursor)?;
    let field_number = key >> 3;
    if field_number == 0 || field_number > MAX_PROTOBUF_FIELD_NUMBER {
        return Err(format!(
            "process frame Protobuf contained invalid field number {field_number}"
        ));
    }
    Ok((
        u32::try_from(field_number)
            .map_err(|_| "process frame Protobuf field number overflowed".to_owned())?,
        u8::try_from(key & 0x07)
            .map_err(|_| "process frame Protobuf wire type overflowed".to_owned())?,
    ))
}

fn read_length(payload: &[u8], cursor: &mut usize) -> Result<usize, String> {
    usize::try_from(read_varint(payload, cursor)?)
        .map_err(|_| "process frame Protobuf field length overflowed".to_owned())
}

fn read_varint(payload: &[u8], cursor: &mut usize) -> Result<u64, String> {
    let mut value = 0_u64;
    for shift in (0_u32..=63).step_by(7) {
        let byte = *payload
            .get(*cursor)
            .ok_or_else(|| "process frame Protobuf contained a truncated varint".to_owned())?;
        *cursor = cursor
            .checked_add(1)
            .ok_or_else(|| "process frame Protobuf cursor overflowed".to_owned())?;
        if shift == 63 && byte > 1 {
            return Err("process frame Protobuf varint overflowed u64".to_owned());
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("process frame Protobuf varint exceeded ten bytes".to_owned())
}

fn skip_wire_value(
    payload: &[u8],
    cursor: &mut usize,
    field_number: u32,
    wire_type: u8,
    depth: usize,
) -> Result<(), String> {
    match wire_type {
        0 => {
            read_varint(payload, cursor)?;
            Ok(())
        }
        1 => advance(payload, cursor, 8),
        2 => {
            let length = read_length(payload, cursor)?;
            advance(payload, cursor, length)
        }
        3 => skip_group(payload, cursor, field_number, depth),
        4 => Err(format!(
            "process frame Protobuf contained unexpected end-group field {field_number}"
        )),
        5 => advance(payload, cursor, 4),
        _ => Err(format!(
            "process frame Protobuf contained invalid wire type {wire_type}"
        )),
    }
}

fn skip_group(
    payload: &[u8],
    cursor: &mut usize,
    start_field: u32,
    depth: usize,
) -> Result<(), String> {
    if depth >= MAX_PROTOBUF_GROUP_DEPTH {
        return Err(format!(
            "process frame Protobuf group nesting exceeded {MAX_PROTOBUF_GROUP_DEPTH}"
        ));
    }
    loop {
        if *cursor == payload.len() {
            return Err(format!(
                "process frame Protobuf group {start_field} was not terminated"
            ));
        }
        let (field_number, wire_type) = read_key(payload, cursor)?;
        if wire_type == 4 {
            return if field_number == start_field {
                Ok(())
            } else {
                Err(format!(
                    "process frame Protobuf group {start_field} ended with field {field_number}"
                ))
            };
        }
        skip_wire_value(payload, cursor, field_number, wire_type, depth + 1)?;
    }
}

fn advance(payload: &[u8], cursor: &mut usize, bytes: usize) -> Result<(), String> {
    let end = cursor
        .checked_add(bytes)
        .ok_or_else(|| "process frame Protobuf cursor overflowed".to_owned())?;
    if end > payload.len() {
        return Err("process frame Protobuf contained a truncated field".to_owned());
    }
    *cursor = end;
    Ok(())
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

    use super::{
        MAX_COMMUNITY_RESPONSE_BYTES, ReaderEvent, WriterCommand, WriterEvent, reader_loop,
        writer_loop,
    };

    const COMMUNITY_BUILT_SQL_TAG: u32 = 202;

    fn encode_varint(mut value: u64, output: &mut Vec<u8>) {
        loop {
            let mut byte = u8::try_from(value & 0x7f).expect("seven bits must fit u8");
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            output.push(byte);
            if value == 0 {
                return;
            }
        }
    }

    fn varint_len(mut value: usize) -> usize {
        let mut length = 1;
        while value >= 0x80 {
            length += 1;
            value >>= 7;
        }
        length
    }

    fn unknown_nested_message(encoded_length: usize) -> Vec<u8> {
        const UNKNOWN_LENGTH_DELIMITED_KEY: u64 = (15 << 3) | 2;
        let mut value_length = encoded_length;
        loop {
            let adjusted = encoded_length
                .checked_sub(1 + varint_len(value_length))
                .expect("test message must leave room for field framing");
            if adjusted == value_length {
                break;
            }
            value_length = adjusted;
        }
        let mut message = Vec::with_capacity(encoded_length);
        encode_varint(UNKNOWN_LENGTH_DELIMITED_KEY, &mut message);
        encode_varint(
            u64::try_from(value_length).expect("test value length must fit u64"),
            &mut message,
        );
        message.resize(encoded_length, 0);
        assert_eq!(message.len(), encoded_length);
        message
    }

    fn push_length_delimited_field(field_number: u32, value: &[u8], output: &mut Vec<u8>) {
        encode_varint(u64::from((field_number << 3) | 2), output);
        encode_varint(
            u64::try_from(value.len()).expect("test value length must fit u64"),
            output,
        );
        output.extend_from_slice(value);
    }

    async fn read_payload(payload: Vec<u8>) -> ReaderEvent {
        let mut frame = Vec::with_capacity(payload.len() + 4);
        frame.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("test payload length must fit u32")
                .to_be_bytes(),
        );
        frame.extend_from_slice(&payload);
        let (events, mut receiver) = mpsc::channel(2);
        reader_loop(frame.as_slice(), events, MAX_FRAME_BYTES).await;
        receiver
            .recv()
            .await
            .expect("reader must emit one frame event")
    }

    #[tokio::test]
    async fn reader_counts_unknown_bytes_in_raw_community_payloads_at_the_exact_boundary() {
        let mut exact = Vec::new();
        push_length_delimited_field(
            COMMUNITY_BUILT_SQL_TAG,
            &unknown_nested_message(MAX_COMMUNITY_RESPONSE_BYTES),
            &mut exact,
        );
        assert!(matches!(read_payload(exact).await, ReaderEvent::Frame(_)));

        let mut oversized = Vec::new();
        push_length_delimited_field(
            COMMUNITY_BUILT_SQL_TAG,
            &unknown_nested_message(MAX_COMMUNITY_RESPONSE_BYTES + 1),
            &mut oversized,
        );
        assert!(matches!(
            read_payload(oversized).await,
            ReaderEvent::Failed(message)
                if message.contains("8388609 bytes") && message.contains("maximum is 8388608")
        ));
    }

    #[tokio::test]
    async fn reader_accumulates_duplicate_community_oneof_fields_before_decode() {
        let first_length = MAX_COMMUNITY_RESPONSE_BYTES / 2;
        let second_length = MAX_COMMUNITY_RESPONSE_BYTES - first_length + 1;
        let mut payload = Vec::new();
        push_length_delimited_field(
            COMMUNITY_BUILT_SQL_TAG,
            &unknown_nested_message(first_length),
            &mut payload,
        );
        push_length_delimited_field(
            COMMUNITY_BUILT_SQL_TAG,
            &unknown_nested_message(second_length),
            &mut payload,
        );

        assert!(matches!(
            read_payload(payload).await,
            ReaderEvent::Failed(message) if message.contains("8388609 bytes")
        ));
    }

    #[tokio::test]
    async fn reader_keeps_the_sixteen_megabyte_frame_budget_for_non_community_fields() {
        let mut payload = Vec::new();
        push_length_delimited_field(
            204,
            &vec![0; MAX_COMMUNITY_RESPONSE_BYTES + 1],
            &mut payload,
        );
        assert!(matches!(read_payload(payload).await, ReaderEvent::Frame(_)));
    }

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
