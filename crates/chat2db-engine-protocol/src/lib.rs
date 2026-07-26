//! Shared Rust-Java protocol types and length-prefixed frame encoding.

use std::io;

use prost::Message;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Current process-protocol major version.
pub const PROTOCOL_MAJOR: u32 = 1;
/// Current process-protocol minor version.
pub const PROTOCOL_MINOR: u32 = 0;
/// Minimum configurable receive limit for one process frame.
pub const MIN_FRAME_BYTES: usize = 1024;
/// Maximum accepted Protobuf payload size for one process frame.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Protobuf-generated process contract.
#[allow(clippy::all, clippy::pedantic)]
pub mod wire {
    include!(concat!(env!("OUT_DIR"), "/chat2db.compat.v1.rs"));
}

/// Returns the protocol version implemented by this build.
#[must_use]
pub const fn current_version() -> wire::ProtocolVersion {
    wire::ProtocolVersion {
        major: PROTOCOL_MAJOR,
        minor: PROTOCOL_MINOR,
    }
}

/// A malformed, oversized, or interrupted process frame.
#[derive(Debug, Error)]
pub enum FrameError {
    /// Reading from or writing to the process pipe failed.
    #[error("process frame I/O failed: {0}")]
    Io(#[from] io::Error),
    /// A zero-length payload cannot contain an envelope.
    #[error("process frame payload cannot be empty")]
    Empty,
    /// The peer advertised a payload larger than the process limit.
    #[error("process frame payload is {actual} bytes; maximum is {maximum}")]
    TooLarge { actual: usize, maximum: usize },
    /// The payload was not a valid message of the expected type.
    #[error("process frame Protobuf decode failed: {0}")]
    Decode(#[from] prost::DecodeError),
}

/// Reads one four-byte big-endian length-prefixed Protobuf frame.
///
/// A clean EOF before the next frame returns `Ok(None)`. EOF inside a header or
/// payload is an error so a crashed engine cannot look like a clean shutdown.
///
/// # Errors
///
/// Returns [`FrameError`] when the pipe fails, the length is invalid, the frame
/// is truncated, or the payload cannot be decoded as the requested message.
pub async fn read_frame<R, M>(reader: &mut R) -> Result<Option<M>, FrameError>
where
    R: AsyncRead + Unpin,
    M: Message + Default,
{
    read_frame_with_limit(reader, MAX_FRAME_BYTES).await
}

/// Reads one frame while enforcing the configured local receive limit.
///
/// The configured limit is always capped by [`MAX_FRAME_BYTES`].
///
/// # Errors
///
/// Returns [`FrameError`] when the pipe fails, the length violates either
/// receive limit, the frame is truncated, or the payload cannot be decoded.
pub async fn read_frame_with_limit<R, M>(
    reader: &mut R,
    local_maximum: usize,
) -> Result<Option<M>, FrameError>
where
    R: AsyncRead + Unpin,
    M: Message + Default,
{
    let Some(payload) = read_frame_payload_with_limit(reader, local_maximum).await? else {
        return Ok(None);
    };
    Ok(Some(M::decode(payload.as_slice())?))
}

/// Reads one frame and returns its undecoded Protobuf payload.
///
/// This is intended for callers that must inspect the original wire data
/// before decoding can discard unknown or duplicate fields. The configured
/// limit is always capped by [`MAX_FRAME_BYTES`].
///
/// # Errors
///
/// Returns [`FrameError`] when the pipe fails, the length violates either
/// receive limit, or the frame is truncated.
pub async fn read_frame_payload_with_limit<R>(
    reader: &mut R,
    local_maximum: usize,
) -> Result<Option<Vec<u8>>, FrameError>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0_u8; 4];
    let bytes_read = reader.read(&mut header[..1]).await?;
    if bytes_read == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut header[1..]).await?;

    let payload_length = u32::from_be_bytes(header) as usize;
    validate_payload_length_with_limit(payload_length, local_maximum.min(MAX_FRAME_BYTES))?;

    let mut payload = vec![0_u8; payload_length];
    reader.read_exact(&mut payload).await?;
    Ok(Some(payload))
}

/// Writes one four-byte big-endian length-prefixed Protobuf frame.
///
/// # Errors
///
/// Returns [`FrameError`] when the encoded payload violates the frame limit or
/// the complete header and payload cannot be flushed to the process pipe.
pub async fn write_frame<W, M>(writer: &mut W, message: &M) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
    M: Message,
{
    write_frame_with_limit(writer, message, MAX_FRAME_BYTES).await
}

/// Writes one frame while honoring a negotiated peer receive limit.
///
/// The encoded size is checked before allocating the payload buffer. The
/// negotiated limit is always capped by [`MAX_FRAME_BYTES`].
///
/// # Errors
///
/// Returns [`FrameError`] when the encoded payload violates either frame limit
/// or the complete frame cannot be written to the process pipe.
pub async fn write_frame_with_limit<W, M>(
    writer: &mut W,
    message: &M,
    peer_maximum: usize,
) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
    M: Message,
{
    let maximum = peer_maximum.min(MAX_FRAME_BYTES);
    let payload_length = message.encoded_len();
    validate_payload_length_with_limit(payload_length, maximum)?;
    let encoded_length = u32::try_from(payload_length).map_err(|_| FrameError::TooLarge {
        actual: payload_length,
        maximum,
    })?;
    let payload = message.encode_to_vec();

    writer.write_all(&encoded_length.to_be_bytes()).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

fn validate_payload_length_with_limit(
    payload_length: usize,
    maximum: usize,
) -> Result<(), FrameError> {
    if payload_length == 0 {
        return Err(FrameError::Empty);
    }
    if payload_length > maximum {
        return Err(FrameError::TooLarge {
            actual: payload_length,
            maximum,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use prost::Message;
    use tokio::io::{AsyncWriteExt, duplex};

    use super::{
        FrameError, MAX_FRAME_BYTES, current_version, read_frame, read_frame_payload_with_limit,
        read_frame_with_limit, wire, write_frame, write_frame_with_limit,
    };

    #[tokio::test]
    async fn frames_round_trip_with_a_big_endian_length_prefix() {
        let (mut writer, mut reader) = duplex(256);
        let request = wire::ClientEnvelope {
            meta: Some(wire::RequestMeta {
                request_id: "request-7".to_owned(),
                trace_id: "trace-7".to_owned(),
                ..Default::default()
            }),
            payload: Some(wire::client_envelope::Payload::Ping(wire::Ping {
                nonce: 7,
            })),
        };

        write_frame(&mut writer, &request)
            .await
            .expect("frame must encode");
        let decoded: wire::ClientEnvelope = read_frame(&mut reader)
            .await
            .expect("frame must decode")
            .expect("frame must be present");

        assert_eq!(decoded, request);
        assert_eq!(current_version().major, 1);
    }

    #[tokio::test]
    async fn clean_eof_is_distinct_from_a_truncated_header() {
        let (writer, mut reader) = duplex(16);
        drop(writer);
        assert!(
            read_frame::<_, wire::ClientEnvelope>(&mut reader)
                .await
                .expect("clean EOF must not fail")
                .is_none()
        );

        let (mut writer, mut reader) = duplex(16);
        writer.write_all(&[0, 0]).await.expect("header must write");
        drop(writer);
        assert!(matches!(
            read_frame::<_, wire::ClientEnvelope>(&mut reader).await,
            Err(FrameError::Io(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof
        ));
    }

    #[tokio::test]
    async fn oversized_frames_are_rejected_before_allocating_the_payload() {
        let (mut writer, mut reader) = duplex(16);
        let oversized = u32::try_from(MAX_FRAME_BYTES + 1).expect("test size must fit u32");
        writer
            .write_all(&oversized.to_be_bytes())
            .await
            .expect("header must write");

        assert!(matches!(
            read_frame::<_, wire::ClientEnvelope>(&mut reader).await,
            Err(FrameError::TooLarge { actual, maximum })
                if actual == MAX_FRAME_BYTES + 1 && maximum == MAX_FRAME_BYTES
        ));
    }

    #[tokio::test]
    async fn configured_receive_limit_is_enforced_before_allocating_the_payload() {
        let (mut writer, mut reader) = duplex(16);
        let configured_maximum = 1_024;
        let oversized = u32::try_from(configured_maximum + 1).expect("test size must fit u32");
        writer
            .write_all(&oversized.to_be_bytes())
            .await
            .expect("header must write");

        assert!(matches!(
            read_frame_with_limit::<_, wire::ClientEnvelope>(
                &mut reader,
                configured_maximum
            )
            .await,
            Err(FrameError::TooLarge { actual, maximum })
                if actual == configured_maximum + 1 && maximum == configured_maximum
        ));
    }

    #[tokio::test]
    async fn raw_frame_reader_preserves_the_exact_protobuf_payload() {
        let payload = wire::ClientEnvelope {
            meta: Some(wire::RequestMeta {
                request_id: "raw-frame".to_owned(),
                trace_id: "raw-frame".to_owned(),
                ..Default::default()
            }),
            payload: Some(wire::client_envelope::Payload::Ping(wire::Ping {
                nonce: 9,
            })),
        }
        .encode_to_vec();
        let (mut writer, mut reader) = duplex(256);
        writer
            .write_all(
                &u32::try_from(payload.len())
                    .expect("test payload length must fit u32")
                    .to_be_bytes(),
            )
            .await
            .expect("header must write");
        writer
            .write_all(&payload)
            .await
            .expect("payload must write");

        let raw = read_frame_payload_with_limit(&mut reader, MAX_FRAME_BYTES)
            .await
            .expect("raw frame must read")
            .expect("raw frame must be present");
        assert_eq!(raw, payload);
    }

    #[tokio::test]
    async fn negotiated_peer_limit_is_enforced_before_writing() {
        let (mut writer, _reader) = duplex(256);
        let request = wire::ClientEnvelope {
            meta: Some(wire::RequestMeta {
                request_id: "request-8".to_owned(),
                trace_id: "trace-8".to_owned(),
                ..Default::default()
            }),
            payload: Some(wire::client_envelope::Payload::Ping(wire::Ping {
                nonce: 8,
            })),
        };
        let actual = request.encoded_len();

        assert!(matches!(
            write_frame_with_limit(&mut writer, &request, actual - 1).await,
            Err(FrameError::TooLarge { actual: encoded, maximum })
                if encoded == actual && maximum == actual - 1
        ));
    }
}
