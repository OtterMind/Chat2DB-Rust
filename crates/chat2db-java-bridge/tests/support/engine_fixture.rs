use std::{
    collections::HashMap, fmt::Write as _, fs::OpenOptions, io::Write, path::PathBuf,
    process::ExitCode, time::Duration,
};

use chat2db_engine_protocol::{MAX_FRAME_BYTES, current_version, read_frame, wire, write_frame};
use prost::Message;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

const PING_CAPABILITY: &str = "lifecycle.ping.v1";
const SHUTDOWN_CAPABILITY: &str = "lifecycle.shutdown.v1";
const DRIVER_ID_DOMAIN_SEPARATOR: &[u8] = b"chat2db-jdbc-driver-v1\0";
const JDBC_CAPABILITIES: [&str; 7] = [
    "driver.external-jar.v1",
    "session.jdbc.v1",
    "query.typed-batches.v1",
    "flow.credit.v1",
    "operation.cancel.v1",
    "update.jdbc.v1",
    "transaction.local.v1",
];
const COMMUNITY_CAPABILITIES: [&str; 8] = [
    "community.plugin-catalog.v1",
    "community.metadata.schemas.v1",
    "community.metadata.objects.v1",
    "community.metadata.relations.v1",
    "community.metadata.programmability.v1",
    "community.sql-builder.v1",
    "community.sql-parser.v1",
    "community.sql-validation.v1",
];

#[derive(Default)]
struct Options {
    handshake: HandshakeBehavior,
    ping: PingBehavior,
    shutdown: ShutdownBehavior,
    stderr_bytes: usize,
    reverse_pings: usize,
    max_receive_frame_bytes: Option<u32>,
    jdbc: Option<JdbcBehavior>,
    community: CommunityBehavior,
    exit_on_update: bool,
    exit_on_commit: bool,
    hang: HangBehavior,
    write_journal: Option<PathBuf>,
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

#[derive(Default, PartialEq, Eq)]
enum HangBehavior {
    #[default]
    None,
    Update,
    GrantCredits,
    Cancel,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum CommunityBehavior {
    #[default]
    None,
    WrongCommit,
    HangCatalog,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JdbcBehavior {
    Normal,
    Gap,
    Duplicate,
    RowBeforeStarted,
    MultipleTerminal,
    AfterTerminal,
    WrongTrace,
    StartedTerminal,
    CompletedNonTerminal,
    WrongOffset,
    WrongColumnCount,
    Paused,
    AwaitControl,
    CancelCompletes,
    CancelHangs,
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

#[allow(clippy::too_many_lines)]
async fn run(options: Options) -> Result<u8, Box<dyn std::error::Error>> {
    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();
    if let HandshakeResult::Exit(code) = perform_handshake(
        &mut input,
        &mut output,
        options.max_receive_frame_bytes,
        options.jdbc.is_some(),
        options.community != CommunityBehavior::None,
    )
    .await?
    {
        return Ok(code);
    }
    if options.handshake == HandshakeBehavior::ExitAfterAck {
        return Ok(42);
    }

    let mut reversed = Vec::new();
    let mut ping_count = 0_u64;
    let mut active_queries = HashMap::<String, wire::RequestMeta>::new();
    let mut cancel_count = 0_u64;
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
            Some(wire::client_envelope::Payload::LoadDriver(load)) => {
                let driver_id = derive_driver_id(&load);
                write_response(
                    &mut output,
                    &meta,
                    wire::server_envelope::Payload::DriverLoaded(wire::DriverLoaded {
                        driver_id,
                        driver_class: load.driver_class,
                        artifact_count: u32::try_from(load.artifacts.len()).unwrap_or(u32::MAX),
                    }),
                    0,
                    true,
                    None,
                )
                .await?;
            }
            Some(wire::client_envelope::Payload::UnloadDriver(unload)) => {
                write_response(
                    &mut output,
                    &meta,
                    wire::server_envelope::Payload::DriverUnloaded(wire::DriverUnloaded {
                        driver_id: unload.driver_id,
                    }),
                    0,
                    true,
                    None,
                )
                .await?;
            }
            Some(wire::client_envelope::Payload::OpenSession(open)) => {
                write_response(
                    &mut output,
                    &meta,
                    wire::server_envelope::Payload::SessionOpened(wire::SessionOpened {
                        session_id: "fixture-session".to_owned(),
                        database: Some(wire::DatabaseProduct {
                            name: "FixtureDB".to_owned(),
                            version: "1".to_owned(),
                            driver_name: open.driver_id,
                            driver_version: "1".to_owned(),
                        }),
                        read_only: open.read_only,
                        session_state: wire::SessionState::AutoCommit as i32,
                    }),
                    0,
                    true,
                    None,
                )
                .await?;
            }
            Some(wire::client_envelope::Payload::CloseSession(_)) => {
                write_response(
                    &mut output,
                    &meta,
                    wire::server_envelope::Payload::SessionClosed(wire::SessionClosed {
                        session_state: wire::SessionState::Closed as i32,
                    }),
                    0,
                    true,
                    None,
                )
                .await?;
            }
            Some(wire::client_envelope::Payload::BeginTransaction(begin)) => {
                write_response(
                    &mut output,
                    &meta,
                    wire::server_envelope::Payload::TransactionStarted(wire::TransactionStarted {
                        transaction_id: "fixture-transaction".to_owned(),
                        isolation: begin.isolation,
                        read_only: begin.read_only,
                        session_state: wire::SessionState::TransactionActive as i32,
                    }),
                    0,
                    true,
                    None,
                )
                .await?;
            }
            Some(wire::client_envelope::Payload::CommitTransaction(commit)) => {
                if options.exit_on_commit {
                    append_journal(&options, "commit")?;
                    return Ok(42);
                }
                write_response(
                    &mut output,
                    &meta,
                    wire::server_envelope::Payload::TransactionCommitted(
                        wire::TransactionCommitted {
                            transaction_id: commit.transaction_id,
                            session_state: wire::SessionState::AutoCommit as i32,
                        },
                    ),
                    0,
                    true,
                    None,
                )
                .await?;
            }
            Some(wire::client_envelope::Payload::RollbackTransaction(rollback)) => {
                write_response(
                    &mut output,
                    &meta,
                    wire::server_envelope::Payload::TransactionRolledBack(
                        wire::TransactionRolledBack {
                            transaction_id: rollback.transaction_id,
                            session_state: wire::SessionState::AutoCommit as i32,
                        },
                    ),
                    0,
                    true,
                    None,
                )
                .await?;
            }
            Some(wire::client_envelope::Payload::ExecuteUpdate(_)) => {
                if options.hang == HangBehavior::Update {
                    eprintln!("fixture received hanging update");
                    continue;
                }
                if options.exit_on_update {
                    append_journal(&options, "update")?;
                    return Ok(42);
                }
                write_response(
                    &mut output,
                    &meta,
                    wire::server_envelope::Payload::UpdateCompleted(wire::UpdateCompleted {
                        affected_rows: 3,
                    }),
                    0,
                    true,
                    None,
                )
                .await?;
            }
            Some(wire::client_envelope::Payload::ExecuteQuery(_)) => {
                let behavior = options.jdbc.unwrap_or(JdbcBehavior::Normal);
                if behavior == JdbcBehavior::AwaitControl {
                    eprintln!("fixture received await-control query {}", meta.request_id);
                }
                write_query_fixture(&mut output, &meta, behavior).await?;
                if matches!(
                    behavior,
                    JdbcBehavior::Paused
                        | JdbcBehavior::AwaitControl
                        | JdbcBehavior::CancelCompletes
                        | JdbcBehavior::CancelHangs
                ) {
                    active_queries.insert(meta.request_id.clone(), meta);
                }
            }
            Some(wire::client_envelope::Payload::GrantCredits(grant)) => {
                let query_meta = active_queries
                    .get(&grant.target_request_id)
                    .cloned()
                    .ok_or("credit request did not have an active query")?;
                if grant.target_request_id != query_meta.request_id {
                    return Err("credit request targeted another query".into());
                }
                if options.hang == HangBehavior::GrantCredits {
                    eprintln!("fixture received hanging credit grant");
                    continue;
                }
                write_response(
                    &mut output,
                    &meta,
                    wire::server_envelope::Payload::CreditsGranted(wire::CreditsGranted {
                        accepted_batch_credits: grant.batch_credits,
                    }),
                    0,
                    true,
                    None,
                )
                .await?;
                if options.jdbc == Some(JdbcBehavior::AwaitControl) {
                    eprintln!("fixture received credit grant before query started");
                    write_query_started(&mut output, &query_meta, false, None).await?;
                }
                write_row_batch(&mut output, &query_meta, 1, 0, 1, None).await?;
                write_query_completed(&mut output, &query_meta, 2, true, 1).await?;
                active_queries.remove(&grant.target_request_id);
            }
            Some(wire::client_envelope::Payload::CancelOperation(cancel)) => {
                let query_meta = active_queries
                    .get(&cancel.target_request_id)
                    .cloned()
                    .ok_or("cancel request did not have an active query")?;
                if options.hang == HangBehavior::Cancel {
                    eprintln!(
                        "fixture received hanging cancel {}",
                        cancel.target_request_id
                    );
                    continue;
                }
                cancel_count = cancel_count.saturating_add(1);
                eprintln!("fixture query cancel count {cancel_count}");
                write_response(
                    &mut output,
                    &meta,
                    wire::server_envelope::Payload::OperationCancelled(wire::OperationCancelled {
                        disposition: wire::CancelDisposition::Accepted as i32,
                    }),
                    0,
                    true,
                    None,
                )
                .await?;
                if matches!(
                    options.jdbc,
                    Some(JdbcBehavior::CancelCompletes | JdbcBehavior::AwaitControl)
                ) {
                    active_queries.remove(&cancel.target_request_id);
                    if options.jdbc == Some(JdbcBehavior::AwaitControl) {
                        eprintln!("fixture received cancel before query started");
                        write_query_started(&mut output, &query_meta, false, None).await?;
                    }
                    write_query_completed(&mut output, &query_meta, 1, true, 0).await?;
                }
            }
            Some(wire::client_envelope::Payload::ListCommunityPlugins(_)) => {
                match options.community {
                    CommunityBehavior::WrongCommit => {
                        write_response(
                            &mut output,
                            &meta,
                            wire::server_envelope::Payload::CommunityPluginCatalog(
                                wire::CommunityPluginCatalog {
                                    source_commit: "1111111111111111111111111111111111111111"
                                        .to_owned(),
                                    plugins: Vec::new(),
                                },
                            ),
                            0,
                            true,
                            None,
                        )
                        .await?;
                    }
                    CommunityBehavior::HangCatalog => {
                        eprintln!("fixture received hanging Community catalog request");
                    }
                    CommunityBehavior::None => {
                        write_error(
                            &mut output,
                            &meta,
                            "community.not_configured",
                            "the fixture does not provide Community compatibility",
                        )
                        .await?;
                    }
                }
            }
            Some(
                wire::client_envelope::Payload::ListCommunitySchemas(_)
                | wire::client_envelope::Payload::ListCommunityDatabases(_)
                | wire::client_envelope::Payload::ListCommunityTables(_)
                | wire::client_envelope::Payload::ListCommunityColumns(_)
                | wire::client_envelope::Payload::ListCommunityIndexes(_)
                | wire::client_envelope::Payload::ListCommunityViews(_)
                | wire::client_envelope::Payload::ListCommunityImportedKeys(_)
                | wire::client_envelope::Payload::ListCommunityExportedKeys(_)
                | wire::client_envelope::Payload::ListCommunityPrimaryKeys(_)
                | wire::client_envelope::Payload::ListCommunityFunctions(_)
                | wire::client_envelope::Payload::GetCommunityFunction(_)
                | wire::client_envelope::Payload::ListCommunityFunctionParameters(_)
                | wire::client_envelope::Payload::ListCommunityProcedures(_)
                | wire::client_envelope::Payload::GetCommunityProcedure(_)
                | wire::client_envelope::Payload::ListCommunityProcedureParameters(_)
                | wire::client_envelope::Payload::ListCommunityTriggers(_)
                | wire::client_envelope::Payload::GetCommunityTrigger(_)
                | wire::client_envelope::Payload::BuildCommunityCreateSchema(_)
                | wire::client_envelope::Payload::ParseCommunitySql(_)
                | wire::client_envelope::Payload::ValidateCommunitySql(_),
            ) => {
                write_error(
                    &mut output,
                    &meta,
                    "community.not_configured",
                    "the fixture does not provide Community compatibility",
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
    jdbc_enabled: bool,
    community_enabled: bool,
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
    let mut capabilities = vec![PING_CAPABILITY.to_owned(), SHUTDOWN_CAPABILITY.to_owned()];
    if jdbc_enabled {
        capabilities.extend(JDBC_CAPABILITIES.map(str::to_owned));
    }
    if community_enabled {
        capabilities.extend(COMMUNITY_CAPABILITIES.map(str::to_owned));
    }
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

async fn write_query_fixture<W>(
    output: &mut W,
    meta: &wire::RequestMeta,
    behavior: JdbcBehavior,
) -> Result<(), chat2db_engine_protocol::FrameError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    match behavior {
        JdbcBehavior::AwaitControl => {}
        JdbcBehavior::RowBeforeStarted => {
            write_row_batch(output, meta, 0, 0, 1, None).await?;
        }
        JdbcBehavior::StartedTerminal => {
            write_query_started(output, meta, true, None).await?;
        }
        JdbcBehavior::WrongTrace => {
            write_query_started(output, meta, false, Some("wrong-trace")).await?;
        }
        behavior => {
            write_query_started(output, meta, false, None).await?;
            match behavior {
                JdbcBehavior::Normal => {
                    write_row_batch(output, meta, 1, 0, 1, None).await?;
                    write_query_completed(output, meta, 2, true, 1).await?;
                }
                JdbcBehavior::Gap => {
                    write_row_batch(output, meta, 2, 0, 1, None).await?;
                }
                JdbcBehavior::Duplicate => {
                    write_row_batch(output, meta, 1, 0, 1, None).await?;
                    write_row_batch(output, meta, 1, 1, 1, None).await?;
                }
                JdbcBehavior::MultipleTerminal => {
                    write_query_completed(output, meta, 1, true, 0).await?;
                    write_query_completed(output, meta, 2, true, 0).await?;
                }
                JdbcBehavior::AfterTerminal => {
                    write_query_completed(output, meta, 1, true, 0).await?;
                    write_row_batch(output, meta, 2, 0, 1, None).await?;
                }
                JdbcBehavior::CompletedNonTerminal => {
                    write_query_completed(output, meta, 1, false, 0).await?;
                }
                JdbcBehavior::WrongOffset => {
                    write_row_batch(output, meta, 1, 1, 1, None).await?;
                }
                JdbcBehavior::WrongColumnCount => {
                    write_row_batch(output, meta, 1, 0, 0, None).await?;
                }
                JdbcBehavior::Paused
                | JdbcBehavior::AwaitControl
                | JdbcBehavior::CancelCompletes
                | JdbcBehavior::CancelHangs => {}
                JdbcBehavior::RowBeforeStarted
                | JdbcBehavior::StartedTerminal
                | JdbcBehavior::WrongTrace => {
                    unreachable!("these behaviors are handled before query-started")
                }
            }
        }
    }
    Ok(())
}

async fn write_query_started<W>(
    output: &mut W,
    meta: &wire::RequestMeta,
    terminal: bool,
    trace_id: Option<&str>,
) -> Result<(), chat2db_engine_protocol::FrameError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    write_response(
        output,
        meta,
        wire::server_envelope::Payload::QueryStarted(wire::QueryStarted {
            columns: vec![wire::JdbcColumn {
                ordinal: 1,
                label: "value".to_owned(),
                name: "value".to_owned(),
                jdbc_type: 12,
                jdbc_type_name: "VARCHAR".to_owned(),
                value_type: wire::JdbcValueType::Text as i32,
                nullability: wire::ColumnNullability::Nullable as i32,
                precision: None,
                scale: None,
                display_size: None,
                signed: None,
                catalog_name: None,
                schema_name: None,
                table_name: None,
            }],
        }),
        0,
        terminal,
        trace_id,
    )
    .await
}

async fn write_row_batch<W>(
    output: &mut W,
    meta: &wire::RequestMeta,
    sequence: u64,
    start_row_offset: u64,
    value_count: usize,
    trace_id: Option<&str>,
) -> Result<(), chat2db_engine_protocol::FrameError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    write_response(
        output,
        meta,
        wire::server_envelope::Payload::RowBatch(wire::RowBatch {
            start_row_offset,
            rows: vec![wire::JdbcRow {
                values: (0..value_count)
                    .map(|_| wire::JdbcValue {
                        value: Some(wire::jdbc_value::Value::TextValue("fixture-row".to_owned())),
                    })
                    .collect(),
            }],
        }),
        sequence,
        false,
        trace_id,
    )
    .await
}

async fn write_query_completed<W>(
    output: &mut W,
    meta: &wire::RequestMeta,
    sequence: u64,
    terminal: bool,
    row_count: u64,
) -> Result<(), chat2db_engine_protocol::FrameError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    write_response(
        output,
        meta,
        wire::server_envelope::Payload::QueryCompleted(wire::QueryCompleted {
            row_count,
            truncated_by_max_rows: false,
            truncated_by_max_result_bytes: false,
        }),
        sequence,
        terminal,
        None,
    )
    .await
}

async fn write_response<W>(
    output: &mut W,
    request: &wire::RequestMeta,
    payload: wire::server_envelope::Payload,
    sequence: u64,
    terminal: bool,
    trace_id: Option<&str>,
) -> Result<(), chat2db_engine_protocol::FrameError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    write_frame(
        output,
        &wire::ServerEnvelope {
            meta: Some(wire::ResponseMeta {
                request_id: request.request_id.clone(),
                trace_id: trace_id.unwrap_or(&request.trace_id).to_owned(),
                sequence,
                terminal,
            }),
            payload: Some(payload),
        },
    )
    .await
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
                database_error: None,
                session_state: None,
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
                "--exit-on-update" => options.exit_on_update = true,
                "--exit-on-commit" => options.exit_on_commit = true,
                "--hang-on-update" => options.hang = HangBehavior::Update,
                "--hang-on-grant-credits" => options.hang = HangBehavior::GrantCredits,
                "--hang-on-cancel" => options.hang = HangBehavior::Cancel,
                _ => {
                    if let Some(value) = argument.strip_prefix("--stderr-bytes=") {
                        options.stderr_bytes = value.parse().expect("stderr byte count must parse");
                    } else if let Some(value) = argument.strip_prefix("--reverse-pings=") {
                        options.reverse_pings = value.parse().expect("ping count must parse");
                    } else if let Some(value) = argument.strip_prefix("--peer-max-frame-bytes=") {
                        options.max_receive_frame_bytes =
                            Some(value.parse().expect("frame byte limit must parse"));
                    } else if let Some(value) = argument.strip_prefix("--jdbc-stream=") {
                        options.jdbc = Some(match value {
                            "normal" => JdbcBehavior::Normal,
                            "gap" => JdbcBehavior::Gap,
                            "duplicate" => JdbcBehavior::Duplicate,
                            "row-before-started" => JdbcBehavior::RowBeforeStarted,
                            "multiple-terminal" => JdbcBehavior::MultipleTerminal,
                            "after-terminal" => JdbcBehavior::AfterTerminal,
                            "wrong-trace" => JdbcBehavior::WrongTrace,
                            "started-terminal" => JdbcBehavior::StartedTerminal,
                            "completed-nonterminal" => JdbcBehavior::CompletedNonTerminal,
                            "wrong-offset" => JdbcBehavior::WrongOffset,
                            "wrong-column-count" => JdbcBehavior::WrongColumnCount,
                            "paused" => JdbcBehavior::Paused,
                            "await-control" => JdbcBehavior::AwaitControl,
                            "cancel-completes" => JdbcBehavior::CancelCompletes,
                            "cancel-hangs" => JdbcBehavior::CancelHangs,
                            _ => panic!("unknown JDBC fixture behavior: {value}"),
                        });
                    } else if let Some(value) = argument.strip_prefix("--community=") {
                        options.community = match value {
                            "wrong-commit" => CommunityBehavior::WrongCommit,
                            "hang-catalog" => CommunityBehavior::HangCatalog,
                            _ => panic!("unknown Community fixture behavior: {value}"),
                        };
                    } else if let Some(value) = argument.strip_prefix("--write-journal=") {
                        options.write_journal = Some(PathBuf::from(value));
                    } else {
                        panic!("unknown engine fixture argument: {argument}");
                    }
                }
            }
        }
        options
    }
}

fn derive_driver_id(request: &wire::LoadDriverRequest) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DRIVER_ID_DOMAIN_SEPARATOR);
    hasher.update(request.driver_class.as_bytes());
    hasher.update([0]);
    for artifact in &request.artifacts {
        hasher.update(&artifact.sha256);
    }

    let digest = hasher.finalize();
    let mut driver_id = String::with_capacity("sha256:".len() + digest.len() * 2);
    driver_id.push_str("sha256:");
    for byte in digest {
        write!(&mut driver_id, "{byte:02x}").expect("writing to String cannot fail");
    }
    driver_id
}

fn append_journal(options: &Options, operation: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = options
        .write_journal
        .as_ref()
        .ok_or("exit-on-write fixture requires --write-journal")?;
    let mut journal = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(journal, "{operation}")?;
    journal.sync_all()?;
    Ok(())
}
