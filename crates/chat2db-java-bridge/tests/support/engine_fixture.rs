use std::{collections::HashMap, io::Write, process::ExitCode, time::Duration};

use chat2db_engine_protocol::{MAX_FRAME_BYTES, current_version, read_frame, wire, write_frame};
use prost::Message;
use tokio::io::AsyncWriteExt;

const PING_CAPABILITY: &str = "lifecycle.ping.v1";
const SHUTDOWN_CAPABILITY: &str = "lifecycle.shutdown.v1";

#[derive(Default)]
struct Options {
    handshake: HandshakeBehavior,
    ping: PingBehavior,
    shutdown: ShutdownBehavior,
    stderr_bytes: usize,
    reverse_pings: usize,
    max_receive_frame_bytes: Option<u32>,
}

#[derive(Default, PartialEq, Eq)]
enum HandshakeBehavior {
    #[default]
    Normal,
    Hang,
    ExitAfterAck,
}

#[derive(Default, PartialEq, Eq)]
enum PingBehavior {
    #[default]
    Normal,
    Exit,
    WrongResponse,
    SplitResponse,
    IgnoreFirst,
    LateNonTerminal,
    NonZeroSequence,
}

#[derive(Default, PartialEq, Eq)]
enum ShutdownBehavior {
    #[default]
    Normal,
    HangAfterAck,
    WrongResponse,
    IgnoreBeforeAck,
}

#[tokio::main]
async fn main() -> ExitCode {
    let options = Options::parse();
    if options.stderr_bytes > 0 {
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(&vec![b'x'; options.stderr_bytes]);
        let _ = stderr.write_all(b"\n");
        let _ = stderr.flush();
    } else {
        eprintln!("chat2db engine fixture started");
    }
    if options.handshake == HandshakeBehavior::Hang {
        tokio::time::sleep(Duration::from_secs(60)).await;
        return ExitCode::SUCCESS;
    }

    match run(options).await {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("engine fixture failed: {error}");
            ExitCode::from(2)
        }
    }
}

async fn run(options: Options) -> Result<u8, Box<dyn std::error::Error>> {
    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();
    if let HandshakeResult::Exit(code) =
        perform_handshake(&mut input, &mut output, options.max_receive_frame_bytes).await?
    {
        return Ok(code);
    }
    if options.handshake == HandshakeBehavior::ExitAfterAck {
        return Ok(42);
    }

    let mut reversed = Vec::new();
    let mut ping_count = 0_u64;
    while let Some(request) = read_frame::<_, wire::ClientEnvelope>(&mut input).await? {
        let meta = request.meta.clone().ok_or("request metadata is missing")?;
        match request.payload {
            Some(wire::client_envelope::Payload::Ping(ping)) => {
                ping_count = ping_count.saturating_add(1);
                if options.ping == PingBehavior::Exit {
                    return Ok(42);
                }
                if options.ping == PingBehavior::IgnoreFirst && ping_count == 1 {
                    eprintln!("fixture ignored first ping");
                    continue;
                }
                if options.ping == PingBehavior::LateNonTerminal {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    write_pong_with_meta(&mut output, &meta, ping.nonce, 0, false, false).await?;
                    continue;
                }
                if options.reverse_pings > 0 {
                    reversed.push((meta, ping));
                    if reversed.len() < options.reverse_pings {
                        continue;
                    }
                    for (ping_meta, ping) in reversed.drain(..).rev() {
                        write_pong(&mut output, &ping_meta, ping.nonce).await?;
                    }
                } else {
                    let response_nonce = if options.ping == PingBehavior::WrongResponse {
                        ping.nonce.wrapping_add(1)
                    } else {
                        ping.nonce
                    };
                    let sequence = u64::from(options.ping == PingBehavior::NonZeroSequence);
                    write_pong_with_meta(
                        &mut output,
                        &meta,
                        response_nonce,
                        sequence,
                        true,
                        options.ping == PingBehavior::SplitResponse,
                    )
                    .await?;
                }
            }
            Some(wire::client_envelope::Payload::Shutdown(_)) => {
                return handle_shutdown(&mut output, &meta, &options.shutdown).await;
            }
            Some(wire::client_envelope::Payload::Hello(_)) => {
                write_error(
                    &mut output,
                    &meta,
                    "protocol.already_handshaken",
                    "the protocol handshake is already complete",
                )
                .await?;
            }
            None => {
                write_error(
                    &mut output,
                    &meta,
                    "protocol.invalid_request",
                    "the request payload is missing",
                )
                .await?;
            }
        }
    }
    Ok(0)
}

async fn handle_shutdown<W>(
    output: &mut W,
    meta: &wire::RequestMeta,
    behavior: &ShutdownBehavior,
) -> Result<u8, Box<dyn std::error::Error>>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if *behavior == ShutdownBehavior::IgnoreBeforeAck {
        tokio::time::sleep(Duration::from_secs(60)).await;
        return Ok(0);
    }
    if *behavior == ShutdownBehavior::WrongResponse {
        write_pong(output, meta, 0).await?;
    } else {
        write_frame(
            output,
            &wire::ServerEnvelope {
                meta: Some(response_meta(meta)),
                payload: Some(wire::server_envelope::Payload::ShutdownAck(
                    wire::ShutdownAck {},
                )),
            },
        )
        .await?;
    }
    if *behavior != ShutdownBehavior::Normal {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
    Ok(0)
}

enum HandshakeResult {
    Ready,
    Exit(u8),
}

async fn perform_handshake<R, W>(
    input: &mut R,
    output: &mut W,
    max_receive_frame_bytes: Option<u32>,
) -> Result<HandshakeResult, Box<dyn std::error::Error>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let Some(first): Option<wire::ClientEnvelope> = read_frame(input).await? else {
        return Ok(HandshakeResult::Exit(0));
    };
    let first_meta = first.meta.clone().ok_or("hello metadata is missing")?;
    let Some(wire::client_envelope::Payload::Hello(hello)) = first.payload else {
        write_error(
            output,
            &first_meta,
            "protocol.handshake_required",
            "the first request must be a client hello",
        )
        .await?;
        return Ok(HandshakeResult::Exit(3));
    };

    let supported = current_version();
    if !hello.supported_versions.contains(&supported) {
        write_error(
            output,
            &first_meta,
            "protocol.unsupported_version",
            "the fixture and host have no common protocol version",
        )
        .await?;
        return Ok(HandshakeResult::Exit(3));
    }
    let capabilities = vec![PING_CAPABILITY.to_owned(), SHUTDOWN_CAPABILITY.to_owned()];
    if let Some(missing) = hello
        .required_capabilities
        .iter()
        .find(|capability| !capabilities.contains(capability))
    {
        write_error(
            output,
            &first_meta,
            "protocol.unsupported_capability",
            &format!("unsupported required capability: {missing}"),
        )
        .await?;
        return Ok(HandshakeResult::Exit(3));
    }

    write_frame(
        output,
        &wire::ServerEnvelope {
            meta: Some(response_meta(&first_meta)),
            payload: Some(wire::server_envelope::Payload::Hello(wire::ServerHello {
                engine_name: "chat2db-engine-fixture".to_owned(),
                engine_version: env!("CARGO_PKG_VERSION").to_owned(),
                engine_instance_id: format!("fixture-{}", std::process::id()),
                selected_version: Some(supported),
                capabilities,
                max_receive_frame_bytes: max_receive_frame_bytes
                    .unwrap_or_else(|| u32::try_from(MAX_FRAME_BYTES).unwrap_or(u32::MAX)),
            })),
        },
    )
    .await?;
    Ok(HandshakeResult::Ready)
}

async fn write_pong<W>(
    output: &mut W,
    meta: &wire::RequestMeta,
    nonce: u64,
) -> Result<(), chat2db_engine_protocol::FrameError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    write_pong_with_meta(output, meta, nonce, 0, true, false).await
}

async fn write_pong_with_meta<W>(
    output: &mut W,
    meta: &wire::RequestMeta,
    nonce: u64,
    sequence: u64,
    terminal: bool,
    split: bool,
) -> Result<(), chat2db_engine_protocol::FrameError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let response = wire::ServerEnvelope {
        meta: Some(wire::ResponseMeta {
            request_id: meta.request_id.clone(),
            trace_id: meta.trace_id.clone(),
            sequence,
            terminal,
        }),
        payload: Some(wire::server_envelope::Payload::Pong(wire::Pong {
            nonce,
            uptime_millis: 1,
        })),
    };
    if split {
        return write_split_frame(output, &response).await;
    }
    write_frame(output, &response).await
}

async fn write_split_frame<W, M>(
    output: &mut W,
    message: &M,
) -> Result<(), chat2db_engine_protocol::FrameError>
where
    W: tokio::io::AsyncWrite + Unpin,
    M: Message,
{
    let payload_length = message.encoded_len();
    let encoded_length = u32::try_from(payload_length).map_err(|_| {
        chat2db_engine_protocol::FrameError::TooLarge {
            actual: payload_length,
            maximum: MAX_FRAME_BYTES,
        }
    })?;
    output.write_all(&encoded_length.to_be_bytes()).await?;
    output.flush().await?;
    eprintln!("fixture wrote split frame header");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let payload = message.encode_to_vec();
    output.write_all(&payload).await?;
    output.flush().await?;
    Ok(())
}

async fn write_error<W>(
    output: &mut W,
    meta: &wire::RequestMeta,
    code: &str,
    message: &str,
) -> Result<(), chat2db_engine_protocol::FrameError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    write_frame(
        output,
        &wire::ServerEnvelope {
            meta: Some(response_meta(meta)),
            payload: Some(wire::server_envelope::Payload::Error(wire::EngineError {
                code: code.to_owned(),
                message: message.to_owned(),
                category: wire::ErrorCategory::Protocol as i32,
                retryable: false,
                fatal: true,
                outcome: wire::OperationOutcome::NotStarted as i32,
                metadata: HashMap::new(),
            })),
        },
    )
    .await
}

fn response_meta(request: &wire::RequestMeta) -> wire::ResponseMeta {
    wire::ResponseMeta {
        request_id: request.request_id.clone(),
        trace_id: request.trace_id.clone(),
        sequence: 0,
        terminal: true,
    }
}

impl Options {
    fn parse() -> Self {
        let mut options = Self::default();
        for argument in std::env::args().skip(1) {
            match argument.as_str() {
                "--hang-before-handshake" => options.handshake = HandshakeBehavior::Hang,
                "--exit-after-handshake" => {
                    options.handshake = HandshakeBehavior::ExitAfterAck;
                }
                "--exit-on-ping" => options.ping = PingBehavior::Exit,
                "--hang-after-shutdown-ack" => {
                    options.shutdown = ShutdownBehavior::HangAfterAck;
                }
                "--wrong-pong" => options.ping = PingBehavior::WrongResponse,
                "--split-pong" => options.ping = PingBehavior::SplitResponse,
                "--ignore-first-ping" => options.ping = PingBehavior::IgnoreFirst,
                "--late-nonterminal-pong" => options.ping = PingBehavior::LateNonTerminal,
                "--non-zero-sequence" => options.ping = PingBehavior::NonZeroSequence,
                "--wrong-shutdown-response" => {
                    options.shutdown = ShutdownBehavior::WrongResponse;
                }
                "--ignore-shutdown" => options.shutdown = ShutdownBehavior::IgnoreBeforeAck,
                _ => {
                    if let Some(value) = argument.strip_prefix("--stderr-bytes=") {
                        options.stderr_bytes = value.parse().expect("stderr byte count must parse");
                    } else if let Some(value) = argument.strip_prefix("--reverse-pings=") {
                        options.reverse_pings = value.parse().expect("ping count must parse");
                    } else if let Some(value) = argument.strip_prefix("--peer-max-frame-bytes=") {
                        options.max_receive_frame_bytes =
                            Some(value.parse().expect("frame byte limit must parse"));
                    } else {
                        panic!("unknown engine fixture argument: {argument}");
                    }
                }
            }
        }
        options
    }
}
