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
const MAX_COMMUNITY_PLUGINS: usize = wire::CommunityCountLimit::MaxPlugins as usize;
const MAX_COMMUNITY_DRIVERS: usize = wire::CommunityCountLimit::MaxDriverConfigs as usize;
const MAX_COMMUNITY_DOWNLOAD_URLS: usize =
    wire::CommunityDownloadUrlLimit::MaxDownloadUrls as usize;
const MAX_COMMUNITY_SCHEMAS: usize = wire::CommunitySchemaCountLimit::MaxSchemas as usize;
const MAX_COMMUNITY_DATABASES: usize = wire::CommunityDatabaseCountLimit::MaxDatabases as usize;
const MAX_COMMUNITY_TABLES: usize = wire::CommunityTableCountLimit::MaxTables as usize;
const MAX_COMMUNITY_VIEWS: usize = wire::CommunityViewCountLimit::MaxViews as usize;
const MAX_COMMUNITY_KEYS: usize = wire::CommunityKeyCountLimit::MaxKeys as usize;
const MAX_COMMUNITY_FUNCTIONS: usize = wire::CommunityFunctionCountLimit::MaxFunctions as usize;
const MAX_COMMUNITY_PROCEDURES: usize = wire::CommunityProcedureCountLimit::MaxProcedures as usize;
const MAX_COMMUNITY_TRIGGERS: usize = wire::CommunityTriggerCountLimit::MaxTriggers as usize;
const MAX_COMMUNITY_ROUTINE_PARAMETERS: usize =
    wire::CommunityRoutineParameterCountLimit::MaxParameters as usize;
const MAX_COMMUNITY_COLUMNS: usize = wire::CommunityColumnCountLimit::MaxColumns as usize;
const MAX_COMMUNITY_INDEXES: usize = wire::CommunityIndexCountLimit::MaxIndexes as usize;
const MAX_COMMUNITY_INDEX_COLUMNS: usize =
    wire::CommunityIndexColumnCountLimit::MaxIndexColumns as usize;
const MAX_COMMUNITY_STATEMENTS: usize = wire::CommunityCountLimit::MaxStatements as usize;
const MAX_COMMUNITY_SQL_DIAGNOSTICS: usize =
    wire::CommunitySqlDiagnosticCountLimit::MaxDiagnostics as usize;
const MAX_COMMUNITY_SQL_COMPLETION_CANDIDATES: usize =
    wire::CommunitySqlCompletionCandidateCountLimit::MaxCandidates as usize;
const MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINTS: usize =
    wire::CommunitySqlCompletionEditorHintCountLimit::MaxEditorHints as usize;
const MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINT_ITEMS: usize =
    wire::CommunitySqlCompletionEditorHintItemCountLimit::MaxEditorHintItems as usize;
const MAX_COMMUNITY_SQL_COMPLETION_SNIPPET_SLOTS: usize =
    wire::CommunitySqlCompletionSnippetSlotCountLimit::MaxSnippetSlots as usize;
const COMMUNITY_RESPONSE_TAGS: std::ops::RangeInclusive<u32> = 200..=224;
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
    let mut counts = CommunityWireCounts::default();
    while cursor < payload.len() {
        let (field_number, wire_type) = read_key(payload, &mut cursor)?;
        if COMMUNITY_RESPONSE_TAGS.contains(&field_number) && wire_type != 2 {
            return Err(format!(
                "Community response field {field_number} used non-length-delimited wire type {wire_type}"
            ));
        }
        if wire_type == 2 {
            let value = read_length_delimited(payload, &mut cursor)?;
            if COMMUNITY_RESPONSE_TAGS.contains(&field_number) {
                community_bytes = community_bytes
                    .checked_add(value.len())
                    .ok_or_else(|| "Community response wire byte count overflowed".to_owned())?;
                if community_bytes > MAX_COMMUNITY_RESPONSE_BYTES {
                    return Err(format!(
                        "Community response wire payloads total {community_bytes} bytes; maximum is {MAX_COMMUNITY_RESPONSE_BYTES}"
                    ));
                }
                validate_community_response_wire_counts(field_number, value, &mut counts)?;
            }
        } else {
            skip_wire_value(payload, &mut cursor, field_number, wire_type, 0)?;
        }
    }
    Ok(())
}

#[derive(Default)]
struct CommunityWireCounts {
    plugins: usize,
    schemas: usize,
    databases: usize,
    tables: usize,
    columns: usize,
    indexes: usize,
    index_columns: usize,
    views: usize,
    foreign_keys: usize,
    primary_keys: usize,
    functions: usize,
    procedures: usize,
    triggers: usize,
    routine_parameters: usize,
    statements: usize,
    diagnostics: usize,
    completion_candidates: usize,
    completion_editor_hints: usize,
    completion_editor_hint_items: usize,
    completion_snippet_slots: usize,
}

fn validate_community_response_wire_counts(
    response_tag: u32,
    payload: &[u8],
    counts: &mut CommunityWireCounts,
) -> Result<(), String> {
    match response_tag {
        200 => scan_community_plugin_catalog(payload, counts),
        201 => scan_bounded_repeated_field(
            payload,
            1,
            &mut counts.schemas,
            MAX_COMMUNITY_SCHEMAS,
            "schema",
        ),
        203 => scan_bounded_repeated_field(
            payload,
            2,
            &mut counts.statements,
            MAX_COMMUNITY_STATEMENTS,
            "statement",
        ),
        204 => scan_bounded_repeated_field(
            payload,
            1,
            &mut counts.databases,
            MAX_COMMUNITY_DATABASES,
            "database",
        ),
        205 => scan_bounded_repeated_field(
            payload,
            1,
            &mut counts.tables,
            MAX_COMMUNITY_TABLES,
            "table",
        ),
        206 => scan_bounded_repeated_field(
            payload,
            1,
            &mut counts.columns,
            MAX_COMMUNITY_COLUMNS,
            "column",
        ),
        207 => scan_community_index_list(payload, counts),
        208 => {
            scan_bounded_repeated_field(payload, 1, &mut counts.views, MAX_COMMUNITY_VIEWS, "view")
        }
        209 | 210 => scan_bounded_repeated_field(
            payload,
            1,
            &mut counts.foreign_keys,
            MAX_COMMUNITY_KEYS,
            "foreign-key",
        ),
        211 => scan_bounded_repeated_field(
            payload,
            1,
            &mut counts.primary_keys,
            MAX_COMMUNITY_KEYS,
            "primary-key",
        ),
        212 => scan_bounded_repeated_field(
            payload,
            1,
            &mut counts.functions,
            MAX_COMMUNITY_FUNCTIONS,
            "function",
        ),
        214 | 217 => scan_bounded_repeated_field(
            payload,
            1,
            &mut counts.routine_parameters,
            MAX_COMMUNITY_ROUTINE_PARAMETERS,
            "routine-parameter",
        ),
        215 => scan_bounded_repeated_field(
            payload,
            1,
            &mut counts.procedures,
            MAX_COMMUNITY_PROCEDURES,
            "procedure",
        ),
        218 => scan_bounded_repeated_field(
            payload,
            1,
            &mut counts.triggers,
            MAX_COMMUNITY_TRIGGERS,
            "trigger",
        ),
        220 => scan_community_sql_validation(payload, counts),
        222 => scan_community_sql_completion(payload, counts),
        _ => Ok(()),
    }
}

fn scan_community_sql_completion(
    payload: &[u8],
    counts: &mut CommunityWireCounts,
) -> Result<(), String> {
    scan_message_fields(
        payload,
        |field_number, wire_type, value| match field_number {
            4 => {
                let candidate = require_length_delimited(
                    wire_type,
                    value,
                    "Community SQL-completion candidate",
                )?;
                add_wire_count(
                    &mut counts.completion_candidates,
                    MAX_COMMUNITY_SQL_COMPLETION_CANDIDATES,
                    "completion-candidate",
                )?;
                scan_community_sql_completion_candidate(
                    candidate,
                    &mut counts.completion_snippet_slots,
                )
            }
            5 => {
                let hint = require_length_delimited(
                    wire_type,
                    value,
                    "Community SQL-completion editor hint",
                )?;
                add_wire_count(
                    &mut counts.completion_editor_hints,
                    MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINTS,
                    "completion-editor-hint",
                )?;
                scan_community_sql_completion_editor_hint(
                    hint,
                    &mut counts.completion_editor_hint_items,
                )
            }
            _ => Ok(()),
        },
    )
}

fn scan_community_sql_completion_candidate(
    payload: &[u8],
    snippet_slots: &mut usize,
) -> Result<(), String> {
    scan_message_fields(payload, |field_number, wire_type, value| {
        if field_number != 23 {
            return Ok(());
        }
        require_length_delimited(wire_type, value, "Community SQL-completion snippet slot")?;
        add_wire_count(
            snippet_slots,
            MAX_COMMUNITY_SQL_COMPLETION_SNIPPET_SLOTS,
            "completion-snippet-slot",
        )
    })
}

fn scan_community_sql_completion_editor_hint(
    payload: &[u8],
    items: &mut usize,
) -> Result<(), String> {
    scan_message_fields(payload, |field_number, wire_type, value| {
        if field_number != 5 {
            return Ok(());
        }
        require_length_delimited(
            wire_type,
            value,
            "Community SQL-completion editor-hint item",
        )?;
        add_wire_count(
            items,
            MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINT_ITEMS,
            "completion-editor-hint-item",
        )
    })
}

fn scan_community_sql_validation(
    payload: &[u8],
    counts: &mut CommunityWireCounts,
) -> Result<(), String> {
    scan_bounded_repeated_field(
        payload,
        2,
        &mut counts.statements,
        MAX_COMMUNITY_STATEMENTS,
        "statement",
    )?;
    scan_bounded_repeated_field(
        payload,
        3,
        &mut counts.diagnostics,
        MAX_COMMUNITY_SQL_DIAGNOSTICS,
        "diagnostic",
    )
}

fn scan_community_plugin_catalog(
    payload: &[u8],
    counts: &mut CommunityWireCounts,
) -> Result<(), String> {
    scan_message_fields(payload, |field_number, wire_type, value| {
        if field_number != 2 {
            return Ok(());
        }
        let plugin = require_length_delimited(wire_type, value, "Community plugin")?;
        add_wire_count(&mut counts.plugins, MAX_COMMUNITY_PLUGINS, "plugin")?;
        scan_community_plugin(plugin)
    })
}

fn scan_community_plugin(payload: &[u8]) -> Result<(), String> {
    let mut drivers = 0_usize;
    scan_message_fields(payload, |field_number, wire_type, value| {
        if field_number != 6 {
            return Ok(());
        }
        let driver = require_length_delimited(wire_type, value, "Community driver")?;
        add_wire_count(&mut drivers, MAX_COMMUNITY_DRIVERS, "driver")?;
        scan_community_driver(driver)
    })
}

fn scan_community_driver(payload: &[u8]) -> Result<(), String> {
    let mut download_urls = 0_usize;
    scan_message_fields(payload, |field_number, wire_type, value| {
        if field_number != 4 {
            return Ok(());
        }
        require_length_delimited(wire_type, value, "Community driver download URL")?;
        add_wire_count(
            &mut download_urls,
            MAX_COMMUNITY_DOWNLOAD_URLS,
            "download-URL",
        )
    })
}

fn scan_community_index_list(
    payload: &[u8],
    counts: &mut CommunityWireCounts,
) -> Result<(), String> {
    scan_message_fields(payload, |field_number, wire_type, value| {
        if field_number != 1 {
            return Ok(());
        }
        let index = require_length_delimited(wire_type, value, "Community index")?;
        add_wire_count(&mut counts.indexes, MAX_COMMUNITY_INDEXES, "index")?;
        scan_community_index(index, &mut counts.index_columns)
    })
}

fn scan_community_index(payload: &[u8], index_columns: &mut usize) -> Result<(), String> {
    scan_message_fields(payload, |field_number, wire_type, value| {
        if field_number != 8 && field_number != 13 {
            return Ok(());
        }
        require_length_delimited(wire_type, value, "Community index column")?;
        add_wire_count(index_columns, MAX_COMMUNITY_INDEX_COLUMNS, "index-column")
    })
}

fn scan_bounded_repeated_field(
    payload: &[u8],
    repeated_field: u32,
    count: &mut usize,
    maximum: usize,
    label: &str,
) -> Result<(), String> {
    scan_message_fields(payload, |field_number, wire_type, value| {
        if field_number != repeated_field {
            return Ok(());
        }
        require_length_delimited(wire_type, value, label)?;
        add_wire_count(count, maximum, label)
    })
}

fn scan_message_fields(
    payload: &[u8],
    mut inspect: impl FnMut(u32, u8, Option<&[u8]>) -> Result<(), String>,
) -> Result<(), String> {
    let mut cursor = 0;
    while cursor < payload.len() {
        let (field_number, wire_type) = read_key(payload, &mut cursor)?;
        if wire_type == 2 {
            let value = read_length_delimited(payload, &mut cursor)?;
            inspect(field_number, wire_type, Some(value))?;
        } else {
            inspect(field_number, wire_type, None)?;
            skip_wire_value(payload, &mut cursor, field_number, wire_type, 0)?;
        }
    }
    Ok(())
}

fn require_length_delimited<'a>(
    wire_type: u8,
    value: Option<&'a [u8]>,
    label: &str,
) -> Result<&'a [u8], String> {
    if wire_type != 2 {
        return Err(format!(
            "{label} used non-length-delimited wire type {wire_type} before Protobuf decode"
        ));
    }
    value.ok_or_else(|| format!("{label} had no length-delimited value before Protobuf decode"))
}

fn add_wire_count(count: &mut usize, maximum: usize, label: &str) -> Result<(), String> {
    *count = count
        .checked_add(1)
        .ok_or_else(|| format!("Community response wire {label} count overflowed"))?;
    if *count > maximum {
        return Err(format!(
            "Community response wire exceeded the {maximum}-{label} limit before Protobuf decode"
        ));
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

fn read_length_delimited<'a>(payload: &'a [u8], cursor: &mut usize) -> Result<&'a [u8], String> {
    let length = read_length(payload, cursor)?;
    let end = cursor
        .checked_add(length)
        .ok_or_else(|| "process frame Protobuf cursor overflowed".to_owned())?;
    let value = payload
        .get(*cursor..end)
        .ok_or_else(|| "process frame Protobuf contained a truncated field".to_owned())?;
    *cursor = end;
    Ok(value)
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
        MAX_COMMUNITY_COLUMNS, MAX_COMMUNITY_DATABASES, MAX_COMMUNITY_DOWNLOAD_URLS,
        MAX_COMMUNITY_DRIVERS, MAX_COMMUNITY_FUNCTIONS, MAX_COMMUNITY_INDEX_COLUMNS,
        MAX_COMMUNITY_INDEXES, MAX_COMMUNITY_KEYS, MAX_COMMUNITY_PLUGINS, MAX_COMMUNITY_PROCEDURES,
        MAX_COMMUNITY_RESPONSE_BYTES, MAX_COMMUNITY_ROUTINE_PARAMETERS, MAX_COMMUNITY_SCHEMAS,
        MAX_COMMUNITY_SQL_COMPLETION_CANDIDATES, MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINT_ITEMS,
        MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINTS, MAX_COMMUNITY_SQL_COMPLETION_SNIPPET_SLOTS,
        MAX_COMMUNITY_SQL_DIAGNOSTICS, MAX_COMMUNITY_STATEMENTS, MAX_COMMUNITY_TABLES,
        MAX_COMMUNITY_TRIGGERS, MAX_COMMUNITY_VIEWS, ReaderEvent, WriterCommand, WriterEvent,
        reader_loop, validate_community_response_wire_budget, writer_loop,
    };

    const COMMUNITY_PLUGIN_CATALOG_TAG: u32 = 200;
    const COMMUNITY_SCHEMA_LIST_TAG: u32 = 201;
    const COMMUNITY_BUILT_SQL_TAG: u32 = 202;
    const COMMUNITY_SQL_ANALYSIS_TAG: u32 = 203;
    const COMMUNITY_OBJECT_METADATA_TAGS: std::ops::RangeInclusive<u32> = 204..=207;
    const COMMUNITY_DATABASE_LIST_TAG: u32 = 204;
    const COMMUNITY_TABLE_LIST_TAG: u32 = 205;
    const COMMUNITY_COLUMN_LIST_TAG: u32 = 206;
    const COMMUNITY_INDEX_LIST_TAG: u32 = 207;
    const COMMUNITY_RELATION_METADATA_TAGS: std::ops::RangeInclusive<u32> = 208..=211;
    const COMMUNITY_VIEW_LIST_TAG: u32 = 208;
    const COMMUNITY_IMPORTED_KEY_LIST_TAG: u32 = 209;
    const COMMUNITY_EXPORTED_KEY_LIST_TAG: u32 = 210;
    const COMMUNITY_PRIMARY_KEY_LIST_TAG: u32 = 211;
    const COMMUNITY_PROGRAMMABILITY_METADATA_TAGS: std::ops::RangeInclusive<u32> = 212..=219;
    const COMMUNITY_FUNCTION_LIST_TAG: u32 = 212;
    const COMMUNITY_FUNCTION_PARAMETER_LIST_TAG: u32 = 214;
    const COMMUNITY_PROCEDURE_LIST_TAG: u32 = 215;
    const COMMUNITY_PROCEDURE_PARAMETER_LIST_TAG: u32 = 217;
    const COMMUNITY_TRIGGER_LIST_TAG: u32 = 218;
    const COMMUNITY_SQL_VALIDATION_TAG: u32 = 220;
    const COMMUNITY_FORMATTED_SQL_TAG: u32 = 221;
    const COMMUNITY_SQL_COMPLETION_TAG: u32 = 222;
    const COMMUNITY_BUILT_DML_TAG: u32 = 223;
    const COMMUNITY_BUILT_NAMESPACE_SQL_TAG: u32 = 224;
    const NON_COMMUNITY_TAG: u32 = 225;

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

    fn repeated_empty_fields(field_number: u32, count: usize) -> Vec<u8> {
        let mut message = Vec::with_capacity(count.saturating_mul(2));
        for _ in 0..count {
            push_length_delimited_field(field_number, &[], &mut message);
        }
        message
    }

    fn community_response(response_tag: u32, value: &[u8]) -> Vec<u8> {
        let mut envelope = Vec::with_capacity(value.len() + 8);
        push_length_delimited_field(response_tag, value, &mut envelope);
        envelope
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
    async fn reader_applies_the_exact_community_budget_to_all_object_metadata_tags() {
        for field_number in COMMUNITY_OBJECT_METADATA_TAGS
            .chain(COMMUNITY_RELATION_METADATA_TAGS)
            .chain(COMMUNITY_PROGRAMMABILITY_METADATA_TAGS)
        {
            let mut exact = Vec::new();
            push_length_delimited_field(
                field_number,
                &unknown_nested_message(MAX_COMMUNITY_RESPONSE_BYTES),
                &mut exact,
            );
            assert!(
                matches!(read_payload(exact).await, ReaderEvent::Frame(_)),
                "Community response tag {field_number} must accept the exact budget"
            );

            let mut oversized = Vec::new();
            push_length_delimited_field(
                field_number,
                &unknown_nested_message(MAX_COMMUNITY_RESPONSE_BYTES + 1),
                &mut oversized,
            );
            assert!(
                matches!(
                    read_payload(oversized).await,
                    ReaderEvent::Failed(message)
                        if message.contains("8388609 bytes")
                            && message.contains("maximum is 8388608")
                ),
                "Community response tag {field_number} must reject one byte over budget"
            );
        }
    }

    #[test]
    fn raw_scanner_enforces_object_collection_limits_before_protobuf_decode() {
        for (response_tag, maximum, label) in [
            (
                COMMUNITY_DATABASE_LIST_TAG,
                MAX_COMMUNITY_DATABASES,
                "database",
            ),
            (COMMUNITY_TABLE_LIST_TAG, MAX_COMMUNITY_TABLES, "table"),
            (COMMUNITY_COLUMN_LIST_TAG, MAX_COMMUNITY_COLUMNS, "column"),
            (COMMUNITY_INDEX_LIST_TAG, MAX_COMMUNITY_INDEXES, "index"),
        ] {
            let exact = community_response(response_tag, &repeated_empty_fields(1, maximum));
            validate_community_response_wire_budget(&exact)
                .unwrap_or_else(|error| panic!("exact {label} limit must pass: {error}"));

            let oversized =
                community_response(response_tag, &repeated_empty_fields(1, maximum + 1));
            let error = validate_community_response_wire_budget(&oversized)
                .expect_err("limit plus one must fail before decode");
            assert!(error.contains(&format!("{maximum}-{label} limit")));
            assert!(error.contains("before Protobuf decode"));
        }
    }

    #[test]
    fn raw_scanner_enforces_relation_collection_limits_before_protobuf_decode() {
        for (response_tag, maximum, label) in [
            (COMMUNITY_VIEW_LIST_TAG, MAX_COMMUNITY_VIEWS, "view"),
            (
                COMMUNITY_IMPORTED_KEY_LIST_TAG,
                MAX_COMMUNITY_KEYS,
                "foreign-key",
            ),
            (
                COMMUNITY_PRIMARY_KEY_LIST_TAG,
                MAX_COMMUNITY_KEYS,
                "primary-key",
            ),
        ] {
            let exact = community_response(response_tag, &repeated_empty_fields(1, maximum));
            validate_community_response_wire_budget(&exact)
                .unwrap_or_else(|error| panic!("exact {label} limit must pass: {error}"));

            let oversized =
                community_response(response_tag, &repeated_empty_fields(1, maximum + 1));
            let error = validate_community_response_wire_budget(&oversized)
                .expect_err("limit plus one must fail before decode");
            assert!(error.contains(&format!("{maximum}-{label} limit")));
            assert!(error.contains("before Protobuf decode"));
        }

        let mut combined = community_response(
            COMMUNITY_IMPORTED_KEY_LIST_TAG,
            &repeated_empty_fields(1, MAX_COMMUNITY_KEYS),
        );
        push_length_delimited_field(
            COMMUNITY_EXPORTED_KEY_LIST_TAG,
            &repeated_empty_fields(1, 1),
            &mut combined,
        );
        let error = validate_community_response_wire_budget(&combined)
            .expect_err("imported and exported keys must share the raw foreign-key limit");
        assert!(error.contains(&format!("{MAX_COMMUNITY_KEYS}-foreign-key limit")));
    }

    #[test]
    fn raw_scanner_enforces_programmability_collection_limits_before_protobuf_decode() {
        for (response_tag, maximum, label) in [
            (
                COMMUNITY_FUNCTION_LIST_TAG,
                MAX_COMMUNITY_FUNCTIONS,
                "function",
            ),
            (
                COMMUNITY_PROCEDURE_LIST_TAG,
                MAX_COMMUNITY_PROCEDURES,
                "procedure",
            ),
            (
                COMMUNITY_TRIGGER_LIST_TAG,
                MAX_COMMUNITY_TRIGGERS,
                "trigger",
            ),
            (
                COMMUNITY_FUNCTION_PARAMETER_LIST_TAG,
                MAX_COMMUNITY_ROUTINE_PARAMETERS,
                "routine-parameter",
            ),
            (
                COMMUNITY_PROCEDURE_PARAMETER_LIST_TAG,
                MAX_COMMUNITY_ROUTINE_PARAMETERS,
                "routine-parameter",
            ),
        ] {
            let exact = community_response(response_tag, &repeated_empty_fields(1, maximum));
            validate_community_response_wire_budget(&exact)
                .unwrap_or_else(|error| panic!("exact {label} limit must pass: {error}"));

            let oversized =
                community_response(response_tag, &repeated_empty_fields(1, maximum + 1));
            let error = validate_community_response_wire_budget(&oversized)
                .expect_err("limit plus one must fail before decode");
            assert!(error.contains(&format!("{maximum}-{label} limit")));
            assert!(error.contains("before Protobuf decode"));
        }
    }

    #[test]
    fn raw_scanner_accumulates_duplicate_and_cross_tag_programmability_counts() {
        let mut duplicate_functions = community_response(
            COMMUNITY_FUNCTION_LIST_TAG,
            &repeated_empty_fields(1, MAX_COMMUNITY_FUNCTIONS),
        );
        push_length_delimited_field(
            COMMUNITY_FUNCTION_LIST_TAG,
            &repeated_empty_fields(1, 1),
            &mut duplicate_functions,
        );
        let error = validate_community_response_wire_budget(&duplicate_functions)
            .expect_err("duplicate function-list tags must share the function limit");
        assert!(error.contains(&format!("{MAX_COMMUNITY_FUNCTIONS}-function limit")));

        let mut cross_tag_parameters = community_response(
            COMMUNITY_FUNCTION_PARAMETER_LIST_TAG,
            &repeated_empty_fields(1, MAX_COMMUNITY_ROUTINE_PARAMETERS),
        );
        push_length_delimited_field(
            COMMUNITY_PROCEDURE_PARAMETER_LIST_TAG,
            &repeated_empty_fields(1, 1),
            &mut cross_tag_parameters,
        );
        let error = validate_community_response_wire_budget(&cross_tag_parameters)
            .expect_err("function and procedure parameters must share the routine limit");
        assert!(error.contains(&format!(
            "{MAX_COMMUNITY_ROUTINE_PARAMETERS}-routine-parameter limit"
        )));
    }

    #[test]
    fn raw_scanner_enforces_combined_index_column_limit_before_protobuf_decode() {
        let exact_index = repeated_empty_fields(8, MAX_COMMUNITY_INDEX_COLUMNS);
        let exact_list = community_response(
            COMMUNITY_INDEX_LIST_TAG,
            &community_response(1, &exact_index),
        );
        validate_community_response_wire_budget(&exact_list)
            .expect("exact index-column limit must pass before decode");

        let mut oversized_index = exact_index;
        push_length_delimited_field(13, &[], &mut oversized_index);
        let oversized_list = community_response(
            COMMUNITY_INDEX_LIST_TAG,
            &community_response(1, &oversized_index),
        );
        let error = validate_community_response_wire_budget(&oversized_list)
            .expect_err("combined index-column limit plus one must fail before decode");
        assert!(error.contains(&format!("{MAX_COMMUNITY_INDEX_COLUMNS}-index-column limit")));
        assert!(error.contains("before Protobuf decode"));
    }

    #[test]
    fn raw_scanner_enforces_existing_community_collection_limits_before_decode() {
        for (response_tag, repeated_field, maximum, label) in [
            (
                COMMUNITY_PLUGIN_CATALOG_TAG,
                2,
                MAX_COMMUNITY_PLUGINS,
                "plugin",
            ),
            (
                COMMUNITY_SCHEMA_LIST_TAG,
                1,
                MAX_COMMUNITY_SCHEMAS,
                "schema",
            ),
            (
                COMMUNITY_SQL_ANALYSIS_TAG,
                2,
                MAX_COMMUNITY_STATEMENTS,
                "statement",
            ),
        ] {
            let exact = community_response(
                response_tag,
                &repeated_empty_fields(repeated_field, maximum),
            );
            validate_community_response_wire_budget(&exact)
                .unwrap_or_else(|error| panic!("exact {label} limit must pass: {error}"));

            let oversized = community_response(
                response_tag,
                &repeated_empty_fields(repeated_field, maximum + 1),
            );
            let error = validate_community_response_wire_budget(&oversized)
                .expect_err("limit plus one must fail before decode");
            assert!(error.contains(&format!("{maximum}-{label} limit")));
        }

        let exact_drivers = repeated_empty_fields(6, MAX_COMMUNITY_DRIVERS);
        let exact_catalog = community_response(
            COMMUNITY_PLUGIN_CATALOG_TAG,
            &community_response(2, &exact_drivers),
        );
        validate_community_response_wire_budget(&exact_catalog)
            .expect("exact driver limit must pass before decode");

        let oversized_drivers = repeated_empty_fields(6, MAX_COMMUNITY_DRIVERS + 1);
        let oversized_catalog = community_response(
            COMMUNITY_PLUGIN_CATALOG_TAG,
            &community_response(2, &oversized_drivers),
        );
        assert!(
            validate_community_response_wire_budget(&oversized_catalog)
                .expect_err("driver limit plus one must fail before decode")
                .contains(&format!("{MAX_COMMUNITY_DRIVERS}-driver limit"))
        );

        let exact_urls = repeated_empty_fields(4, MAX_COMMUNITY_DOWNLOAD_URLS);
        let exact_plugin = community_response(6, &exact_urls);
        let exact_catalog = community_response(
            COMMUNITY_PLUGIN_CATALOG_TAG,
            &community_response(2, &exact_plugin),
        );
        validate_community_response_wire_budget(&exact_catalog)
            .expect("exact download-URL limit must pass before decode");

        let oversized_urls = repeated_empty_fields(4, MAX_COMMUNITY_DOWNLOAD_URLS + 1);
        let oversized_plugin = community_response(6, &oversized_urls);
        let oversized_catalog = community_response(
            COMMUNITY_PLUGIN_CATALOG_TAG,
            &community_response(2, &oversized_plugin),
        );
        assert!(
            validate_community_response_wire_budget(&oversized_catalog)
                .expect_err("download-URL limit plus one must fail before decode")
                .contains(&format!("{MAX_COMMUNITY_DOWNLOAD_URLS}-download-URL limit"))
        );
    }

    #[test]
    fn raw_scanner_enforces_sql_validation_limits_before_protobuf_decode() {
        let mut exact_validation = repeated_empty_fields(2, MAX_COMMUNITY_STATEMENTS);
        exact_validation.extend(repeated_empty_fields(3, MAX_COMMUNITY_SQL_DIAGNOSTICS));
        validate_community_response_wire_budget(&community_response(
            COMMUNITY_SQL_VALIDATION_TAG,
            &exact_validation,
        ))
        .expect("exact validation collection limits must pass before decode");

        for (field_number, maximum, label) in [
            (2, MAX_COMMUNITY_STATEMENTS, "statement"),
            (3, MAX_COMMUNITY_SQL_DIAGNOSTICS, "diagnostic"),
        ] {
            let oversized = community_response(
                COMMUNITY_SQL_VALIDATION_TAG,
                &repeated_empty_fields(field_number, maximum + 1),
            );
            let error = validate_community_response_wire_budget(&oversized)
                .expect_err("validation limit plus one must fail before decode");
            assert!(error.contains(&format!("{maximum}-{label} limit")));
            assert!(error.contains("before Protobuf decode"));
        }

        let mut cross_payload_statements = community_response(
            COMMUNITY_SQL_ANALYSIS_TAG,
            &repeated_empty_fields(2, MAX_COMMUNITY_STATEMENTS),
        );
        push_length_delimited_field(
            COMMUNITY_SQL_VALIDATION_TAG,
            &repeated_empty_fields(2, 1),
            &mut cross_payload_statements,
        );
        assert!(
            validate_community_response_wire_budget(&cross_payload_statements)
                .expect_err("analysis and validation must share the statement limit")
                .contains(&format!("{MAX_COMMUNITY_STATEMENTS}-statement limit"))
        );

        let mut duplicate_diagnostics = community_response(
            COMMUNITY_SQL_VALIDATION_TAG,
            &repeated_empty_fields(3, MAX_COMMUNITY_SQL_DIAGNOSTICS),
        );
        push_length_delimited_field(
            COMMUNITY_SQL_VALIDATION_TAG,
            &repeated_empty_fields(3, 1),
            &mut duplicate_diagnostics,
        );
        assert!(
            validate_community_response_wire_budget(&duplicate_diagnostics)
                .expect_err("duplicate validation payloads must share the diagnostic limit")
                .contains(&format!("{MAX_COMMUNITY_SQL_DIAGNOSTICS}-diagnostic limit"))
        );
    }

    #[test]
    fn raw_scanner_enforces_formatter_budget_across_duplicate_oneof_payloads() {
        let exact = community_response(
            COMMUNITY_FORMATTED_SQL_TAG,
            &unknown_nested_message(MAX_COMMUNITY_RESPONSE_BYTES),
        );
        validate_community_response_wire_budget(&exact)
            .expect("formatter payload exactly at the Community byte budget must pass");

        let oversized = community_response(
            COMMUNITY_FORMATTED_SQL_TAG,
            &unknown_nested_message(MAX_COMMUNITY_RESPONSE_BYTES + 1),
        );
        assert!(
            validate_community_response_wire_budget(&oversized)
                .expect_err("formatter payload above the Community byte budget must fail")
                .contains(&format!("{} bytes", MAX_COMMUNITY_RESPONSE_BYTES + 1))
        );

        let first_length = MAX_COMMUNITY_RESPONSE_BYTES / 2;
        let second_length = MAX_COMMUNITY_RESPONSE_BYTES - first_length + 1;
        let mut duplicate = community_response(
            COMMUNITY_FORMATTED_SQL_TAG,
            &unknown_nested_message(first_length),
        );
        push_length_delimited_field(
            COMMUNITY_FORMATTED_SQL_TAG,
            &unknown_nested_message(second_length),
            &mut duplicate,
        );
        assert!(
            validate_community_response_wire_budget(&duplicate)
                .expect_err("duplicate formatter payloads must share the Community byte budget")
                .contains(&format!("{} bytes", MAX_COMMUNITY_RESPONSE_BYTES + 1))
        );
    }

    #[test]
    fn raw_scanner_enforces_completion_top_level_counts_across_duplicate_payloads() {
        for (field_number, maximum, label) in [
            (
                4,
                MAX_COMMUNITY_SQL_COMPLETION_CANDIDATES,
                "completion-candidate",
            ),
            (
                5,
                MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINTS,
                "completion-editor-hint",
            ),
        ] {
            let exact = community_response(
                COMMUNITY_SQL_COMPLETION_TAG,
                &repeated_empty_fields(field_number, maximum),
            );
            validate_community_response_wire_budget(&exact)
                .unwrap_or_else(|error| panic!("exact {label} limit must pass: {error}"));

            let mut duplicate = exact;
            push_length_delimited_field(
                COMMUNITY_SQL_COMPLETION_TAG,
                &repeated_empty_fields(field_number, 1),
                &mut duplicate,
            );
            let error = validate_community_response_wire_budget(&duplicate)
                .expect_err("duplicate completion payloads must share top-level counts");
            assert!(error.contains(&format!("{maximum}-{label} limit")));
            assert!(error.contains("before Protobuf decode"));
        }
    }

    #[test]
    fn raw_scanner_enforces_completion_nested_counts_across_duplicate_payloads() {
        let exact_slots = repeated_empty_fields(23, MAX_COMMUNITY_SQL_COMPLETION_SNIPPET_SLOTS);
        let exact_candidate = community_response(4, &exact_slots);
        let mut duplicate_slots =
            community_response(COMMUNITY_SQL_COMPLETION_TAG, &exact_candidate);
        push_length_delimited_field(
            COMMUNITY_SQL_COMPLETION_TAG,
            &community_response(4, &repeated_empty_fields(23, 1)),
            &mut duplicate_slots,
        );
        assert!(
            validate_community_response_wire_budget(&duplicate_slots)
                .expect_err("duplicate completion payloads must share snippet-slot counts")
                .contains(&format!(
                    "{MAX_COMMUNITY_SQL_COMPLETION_SNIPPET_SLOTS}-completion-snippet-slot limit"
                ))
        );

        let exact_items = repeated_empty_fields(5, MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINT_ITEMS);
        let exact_hint = community_response(5, &exact_items);
        let mut duplicate_items = community_response(COMMUNITY_SQL_COMPLETION_TAG, &exact_hint);
        push_length_delimited_field(
            COMMUNITY_SQL_COMPLETION_TAG,
            &community_response(5, &repeated_empty_fields(5, 1)),
            &mut duplicate_items,
        );
        assert!(
            validate_community_response_wire_budget(&duplicate_items)
                .expect_err("duplicate completion payloads must share editor-hint-item counts")
                .contains(&format!(
                    "{MAX_COMMUNITY_SQL_COMPLETION_EDITOR_HINT_ITEMS}-completion-editor-hint-item limit"
                ))
        );
    }

    #[test]
    fn raw_scanner_enforces_completion_byte_budget_across_duplicate_payloads() {
        let first_length = MAX_COMMUNITY_RESPONSE_BYTES / 2;
        let second_length = MAX_COMMUNITY_RESPONSE_BYTES - first_length + 1;
        let mut duplicate = community_response(
            COMMUNITY_SQL_COMPLETION_TAG,
            &unknown_nested_message(first_length),
        );
        push_length_delimited_field(
            COMMUNITY_SQL_COMPLETION_TAG,
            &unknown_nested_message(second_length),
            &mut duplicate,
        );
        assert!(
            validate_community_response_wire_budget(&duplicate)
                .expect_err("duplicate completion payloads must share the Community byte budget")
                .contains(&format!("{} bytes", MAX_COMMUNITY_RESPONSE_BYTES + 1))
        );
    }

    #[test]
    fn raw_scanner_enforces_dml_byte_budget_across_duplicate_payloads() {
        let exact = community_response(
            COMMUNITY_BUILT_DML_TAG,
            &unknown_nested_message(MAX_COMMUNITY_RESPONSE_BYTES),
        );
        validate_community_response_wire_budget(&exact)
            .expect("DML payload exactly at the Community byte budget must pass");

        let first_length = MAX_COMMUNITY_RESPONSE_BYTES / 2;
        let second_length = MAX_COMMUNITY_RESPONSE_BYTES - first_length + 1;
        let mut duplicate = community_response(
            COMMUNITY_BUILT_DML_TAG,
            &unknown_nested_message(first_length),
        );
        push_length_delimited_field(
            COMMUNITY_BUILT_DML_TAG,
            &unknown_nested_message(second_length),
            &mut duplicate,
        );
        assert!(
            validate_community_response_wire_budget(&duplicate)
                .expect_err("duplicate DML payloads must share the Community byte budget")
                .contains(&format!("{} bytes", MAX_COMMUNITY_RESPONSE_BYTES + 1))
        );
    }

    #[test]
    fn raw_scanner_enforces_namespace_byte_budget_across_duplicate_payloads() {
        let exact = community_response(
            COMMUNITY_BUILT_NAMESPACE_SQL_TAG,
            &unknown_nested_message(MAX_COMMUNITY_RESPONSE_BYTES),
        );
        validate_community_response_wire_budget(&exact)
            .expect("namespace payload exactly at the Community byte budget must pass");

        let first_length = MAX_COMMUNITY_RESPONSE_BYTES / 2;
        let second_length = MAX_COMMUNITY_RESPONSE_BYTES - first_length + 1;
        let mut duplicate = community_response(
            COMMUNITY_BUILT_NAMESPACE_SQL_TAG,
            &unknown_nested_message(first_length),
        );
        push_length_delimited_field(
            COMMUNITY_BUILT_NAMESPACE_SQL_TAG,
            &unknown_nested_message(second_length),
            &mut duplicate,
        );
        assert!(
            validate_community_response_wire_budget(&duplicate)
                .expect_err("duplicate namespace payloads must share the Community byte budget")
                .contains(&format!("{} bytes", MAX_COMMUNITY_RESPONSE_BYTES + 1))
        );

        let mut cross_operation = community_response(
            COMMUNITY_BUILT_DML_TAG,
            &unknown_nested_message(first_length),
        );
        push_length_delimited_field(
            COMMUNITY_BUILT_NAMESPACE_SQL_TAG,
            &unknown_nested_message(second_length),
            &mut cross_operation,
        );
        assert!(
            validate_community_response_wire_budget(&cross_operation)
                .expect_err("DML and namespace payloads must share the Community byte budget")
                .contains(&format!("{} bytes", MAX_COMMUNITY_RESPONSE_BYTES + 1))
        );
    }

    #[tokio::test]
    async fn reader_rejects_oversized_empty_column_list_without_decoding_it() {
        let payload = community_response(
            COMMUNITY_COLUMN_LIST_TAG,
            &repeated_empty_fields(1, MAX_COMMUNITY_COLUMNS + 1),
        );
        assert!(matches!(
            read_payload(payload).await,
            ReaderEvent::Failed(message)
                if message.contains(&format!("{MAX_COMMUNITY_COLUMNS}-column limit"))
                    && message.contains("before Protobuf decode")
        ));
    }

    #[tokio::test]
    async fn reader_keeps_the_sixteen_megabyte_frame_budget_for_non_community_fields() {
        let mut payload = Vec::new();
        push_length_delimited_field(
            NON_COMMUNITY_TAG,
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
