use chat2db_contract::{
    ApiError, CommunityDatabase, CommunityDatabaseList, CommunitySchemaList, CommunityTable,
    CommunityTableList, CommunityTablePreviewAccepted, DatasourceConnection, QueryLimits,
    ResultMetadata, StartCommunityTablePreviewRequest, StartQueryRequest,
};
use chat2db_engine_protocol::wire;
use chat2db_java_bridge::QueryOptions;
use chat2db_storage::Storage;
use mysql_async::{
    Column, Conn, Error as MysqlError, Opts, OptsBuilder, Row, SslOpts, Value,
    consts::{ColumnFlags, ColumnType},
    prelude::Queryable,
};
use prost::Message;
use std::time::Duration;
use tokio::sync::watch;
use url::Url;

use crate::{
    AppError, AppErrorKind, Application,
    datasource_session::{ResolvedDatasourceConnection, resolve_datasource_connection},
    operation::CancellationRequest,
    query::{PreparedQuery, QueryTaskError, RetainedWriter},
};

const MYSQL_SCHEME: &str = "mysql://";
const JDBC_MYSQL_SCHEME: &str = "jdbc:mysql://";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_BATCH_ROWS: u32 = 256;
const DEFAULT_BATCH_BYTES: u32 = 256 * 1024;
const DEFAULT_RESULT_BYTES: u64 = wire::JdbcResultByteLimit::DefaultResultBytes as u64;
const MAX_RESULT_BYTES: u64 = wire::JdbcResultByteLimit::MaxResultBytes as u64;
const MAX_BATCH_ROWS: u32 = wire::JdbcProtocolLimit::MaxBatchRows as u32;
const MAX_BATCH_BYTES: u32 = wire::JdbcProtocolLimit::MaxBatchBytes as u32;
const MAX_COLUMNS: usize = wire::JdbcProtocolLimit::MaxColumns as usize;
const MAX_SQL_BYTES: usize = wire::JdbcProtocolLimit::MaxSqlBytes as usize;
const MAX_SCALAR_BYTES: usize = wire::JdbcProtocolLimit::MaxScalarBytes as usize;
const MAX_IDENTIFIER_BYTES: usize = 256;
type TableRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

#[derive(Debug, PartialEq, Eq)]
enum SqlToken {
    Word(String),
    Semicolon,
}

pub(crate) fn is_mysql_database_type(database_type: &str) -> bool {
    database_type.trim().eq_ignore_ascii_case("mysql")
}

pub(crate) async fn test_connection(connection: &DatasourceConnection) -> Result<(), AppError> {
    let mut conn = open_connection(connection).await?;
    let result = conn.ping().await.map_err(mysql_connection_error);
    let close = conn.disconnect().await.map_err(mysql_connection_error);
    result.and(close)
}

pub(crate) async fn list_databases(
    application: &Application,
    datasource_id: &str,
) -> Result<CommunityDatabaseList, AppError> {
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_connection(&resolved.connection).await?;
    let result = conn
        .query::<(String, String, String), _>(
            "SELECT SCHEMA_NAME, DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME \
             FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME",
        )
        .await
        .map(|rows| CommunityDatabaseList {
            items: rows
                .into_iter()
                .map(|(name, charset, collation)| CommunityDatabase {
                    system: is_system_database(&name),
                    name,
                    charset,
                    collation,
                    ..CommunityDatabase::default()
                })
                .collect(),
        })
        .map_err(mysql_query_error);
    finish_connection(conn, result).await
}

pub(crate) async fn list_schemas(
    application: &Application,
    datasource_id: &str,
) -> Result<CommunitySchemaList, AppError> {
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let conn = open_connection(&resolved.connection).await?;
    finish_connection(conn, Ok(CommunitySchemaList::default())).await
}

pub(crate) async fn list_tables(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    table_name_pattern: &str,
) -> Result<CommunityTableList, AppError> {
    if database_name.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_mysql_metadata_request",
            "databaseName cannot be empty",
        ));
    }
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_connection(&resolved.connection).await?;
    let query = "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE, COALESCE(TABLE_COMMENT, ''), \
                 COALESCE(ENGINE, ''), COALESCE(TABLE_COLLATION, ''), \
                 CAST(AUTO_INCREMENT AS CHAR), CAST(TABLE_ROWS AS CHAR), \
                 CAST(DATA_LENGTH AS CHAR), \
                 DATE_FORMAT(CREATE_TIME, '%Y-%m-%dT%H:%i:%s'), \
                 DATE_FORMAT(UPDATE_TIME, '%Y-%m-%dT%H:%i:%s') \
                 FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = ? AND (? = '' OR TABLE_NAME LIKE ?) \
                 ORDER BY TABLE_NAME";
    let pattern = table_name_pattern.trim().to_owned();
    let result = conn
        .exec::<TableRow, _, _>(query, (database_name.to_owned(), pattern.clone(), pattern))
        .await
        .map(|rows| CommunityTableList {
            items: rows
                .into_iter()
                .map(
                    |(
                        database_name,
                        name,
                        table_type,
                        comment,
                        engine,
                        collation,
                        increment_value,
                        rows,
                        data_length,
                        create_time,
                        update_time,
                    )| CommunityTable {
                        database_name,
                        name,
                        table_type: normalize_table_type(&table_type).to_owned(),
                        comment,
                        database_type: "MYSQL".to_owned(),
                        engine,
                        charset: collation
                            .split_once('_')
                            .map_or_else(String::new, |(charset, _)| charset.to_owned()),
                        collation,
                        increment_value,
                        rows,
                        data_length,
                        create_time: create_time.unwrap_or_default(),
                        update_time: update_time.unwrap_or_default(),
                        ..CommunityTable::default()
                    },
                )
                .collect(),
        })
        .map_err(mysql_query_error);
    finish_connection(conn, result).await
}

pub(crate) fn is_native_read_candidate(sql: &str) -> Result<bool, AppError> {
    Ok(matches!(
        sql_tokens(sql)?.first(),
        Some(SqlToken::Word(keyword)) if keyword == "SELECT" || keyword == "WITH"
    ))
}

pub(crate) fn validate_query(query: &PreparedQuery) -> Result<(), AppError> {
    if query.sql.len() > MAX_SQL_BYTES {
        return Err(AppError::invalid(
            "invalid_query_request",
            format!("SQL cannot exceed {MAX_SQL_BYTES} UTF-8 bytes"),
        ));
    }
    if !query.parameters.is_empty() {
        return Err(AppError::invalid(
            "invalid_query_request",
            "Native MySQL SELECT does not accept parameters yet",
        ));
    }
    validate_read_sql(&query.sql)?;
    validate_query_options(query.options)
}

fn validate_read_sql(sql: &str) -> Result<(), AppError> {
    let tokens = sql_tokens(sql)?;
    if !matches!(tokens.first(), Some(SqlToken::Word(keyword)) if keyword == "SELECT") {
        return Err(AppError::invalid(
            "mysql_native_query_unsupported",
            "Native MySQL currently supports single SELECT statements that begin with SELECT",
        ));
    }
    if let Some(index) = tokens
        .iter()
        .position(|token| matches!(token, SqlToken::Semicolon))
        && (index + 1 != tokens.len()
            || tokens[..index]
                .iter()
                .any(|token| matches!(token, SqlToken::Semicolon)))
    {
        return Err(AppError::invalid(
            "mysql_native_query_unsupported",
            "Native MySQL accepts exactly one SELECT statement",
        ));
    }
    let words = tokens
        .iter()
        .filter_map(|token| match token {
            SqlToken::Word(word) => Some(word.as_str()),
            SqlToken::Semicolon => None,
        })
        .collect::<Vec<_>>();
    let forbidden = words
        .windows(2)
        .any(|window| matches!(window, ["INTO", "OUTFILE" | "DUMPFILE"] | ["FOR", "UPDATE"]))
        || words
            .windows(4)
            .any(|window| matches!(window, ["LOCK", "IN", "SHARE", "MODE"]));
    if forbidden {
        return Err(AppError::invalid(
            "mysql_native_query_unsupported",
            "Native MySQL does not accept locking or server-file SELECT variants",
        ));
    }
    Ok(())
}

fn sql_tokens(sql: &str) -> Result<Vec<SqlToken>, AppError> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            byte if byte.is_ascii_whitespace() => index += 1,
            b'#' => skip_line_comment(bytes, &mut index),
            b'-' if bytes.get(index + 1) == Some(&b'-')
                && bytes.get(index + 2).is_none_or(u8::is_ascii_whitespace) =>
            {
                skip_line_comment(bytes, &mut index);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                let mut terminated = false;
                while index + 1 < bytes.len() {
                    if bytes[index] == b'*' && bytes[index + 1] == b'/' {
                        index += 2;
                        terminated = true;
                        break;
                    }
                    index += 1;
                }
                if !terminated {
                    return Err(invalid_sql_lexeme("unterminated block comment"));
                }
            }
            quote @ (b'\'' | b'"' | b'`') => {
                index += 1;
                let mut terminated = false;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else if bytes[index] == quote {
                        if bytes.get(index + 1) == Some(&quote) {
                            index += 2;
                        } else {
                            index += 1;
                            terminated = true;
                            break;
                        }
                    } else {
                        index += 1;
                    }
                }
                if !terminated {
                    return Err(invalid_sql_lexeme("unterminated quoted value"));
                }
            }
            b';' => {
                tokens.push(SqlToken::Semicolon);
                index += 1;
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = index;
                index += 1;
                while bytes
                    .get(index)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
                {
                    index += 1;
                }
                tokens.push(SqlToken::Word(sql[start..index].to_ascii_uppercase()));
            }
            _ => index += 1,
        }
    }
    Ok(tokens)
}

fn skip_line_comment(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && !matches!(bytes[*index], b'\r' | b'\n') {
        *index += 1;
    }
}

fn invalid_sql_lexeme(detail: &str) -> AppError {
    AppError::invalid(
        "invalid_query_request",
        format!("MySQL SQL contains an {detail}"),
    )
}

pub(crate) async fn start_table_preview(
    application: &Application,
    request: StartCommunityTablePreviewRequest,
    row_limit: u32,
) -> Result<CommunityTablePreviewAccepted, AppError> {
    let database_name = quote_identifier(&request.database_name, "databaseName")?;
    let table_name = quote_identifier(&request.table_name, "tableName")?;
    let sql = format!("SELECT * FROM {database_name}.{table_name} LIMIT {row_limit}");
    let accepted = application
        .start_read_query(StartQueryRequest {
            datasource_id: request.datasource_id,
            sql: sql.clone(),
            parameters: Vec::new(),
            limits: QueryLimits {
                max_rows: row_limit.to_string(),
                max_result_bytes: (8 * 1024 * 1024_u64).to_string(),
                batch_rows: row_limit.min(200),
                batch_bytes: 1024 * 1024,
                result_ttl_seconds: 60 * 60,
            },
        })
        .await?;
    Ok(CommunityTablePreviewAccepted {
        operation_id: accepted.operation_id,
        sql,
        row_limit,
    })
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn execute_query_task(
    application: &Application,
    operation_id: &str,
    mut cancellation: watch::Receiver<CancellationRequest>,
    query: PreparedQuery,
    storage: Storage,
    resolved: ResolvedDatasourceConnection,
) -> Result<ResultMetadata, QueryTaskError> {
    if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
        return Err(QueryTaskError::Cancelled(reason));
    }

    let options = connection_opts(&resolved.connection)?;
    let mut conn = open_query_connection(options.clone(), &mut cancellation).await?;
    let connection_id = conn.id();
    if let Err(error) = start_read_only_transaction(&mut conn).await {
        disconnect_quietly(conn).await;
        return Err(error.into());
    }
    let query_result = {
        let query_future = conn.exec_iter(query.sql, ());
        tokio::pin!(query_future);
        let mut cancellation_open = true;
        loop {
            tokio::select! {
                biased;
                changed = cancellation.changed(), if cancellation_open => {
                    if changed.is_err() {
                        cancellation_open = false;
                        continue;
                    }
                    let request = { cancellation.borrow().clone() };
                    if let CancellationRequest::Requested { reason } = request {
                        return match terminate_connection(options.clone(), connection_id).await {
                            Ok(()) => Err(QueryTaskError::Cancelled(reason)),
                            Err(error) => Err(QueryTaskError::Failed(error)),
                        };
                    }
                }
                result = &mut query_future => break result.map_err(mysql_query_error)?,
            }
        }
    };
    let columns = query_result.columns_ref().to_vec();
    let schema = (|| {
        if columns.len() > MAX_COLUMNS {
            return Err(resource_error(
                "mysql_result_too_wide",
                format!("MySQL returned more than {MAX_COLUMNS} columns"),
            ));
        }
        Ok(wire::QueryStarted {
            columns: columns
                .iter()
                .enumerate()
                .map(|(index, column)| mysql_column(index, column))
                .collect::<Result<_, _>>()?,
        })
    })();
    let schema = match schema {
        Ok(schema) => schema,
        Err(error) => {
            drop(query_result);
            terminate_connection_quietly(options, connection_id).await;
            drop(conn);
            return Err(error.into());
        }
    };
    let mut writer = match RetainedWriter::begin(storage, schema, query.retention).await {
        Ok(writer) => writer,
        Err(error) => {
            drop(query_result);
            terminate_connection_quietly(options, connection_id).await;
            drop(conn);
            return Err(error.into());
        }
    };
    if let Err(error) = application.inner.operations.started(operation_id).await {
        drop(query_result);
        abort_writer(&mut writer).await;
        terminate_connection_quietly(options, connection_id).await;
        drop(conn);
        return Err(error.into());
    }

    let max_rows = query.options.max_rows;
    let max_result_bytes = if query.options.max_result_bytes == 0 {
        DEFAULT_RESULT_BYTES
    } else {
        query.options.max_result_bytes
    };
    let batch_rows = if query.options.target_batch_rows == 0 {
        DEFAULT_BATCH_ROWS
    } else {
        query.options.target_batch_rows
    };
    let batch_bytes = if query.options.target_batch_bytes == 0 {
        DEFAULT_BATCH_BYTES
    } else {
        query.options.target_batch_bytes
    };
    let mut result = query_result;
    let mut pending_rows = Vec::new();
    let mut pending_bytes = 0_u64;
    let mut row_count = 0_u64;
    let mut result_bytes = 0_u64;
    let mut cancellation_open = true;
    let consumption: Result<(wire::QueryCompleted, bool), QueryTaskError> = async {
        loop {
            let next = tokio::select! {
                biased;
                changed = cancellation.changed(), if cancellation_open => {
                    if changed.is_err() {
                        cancellation_open = false;
                        continue;
                    }
                    let request = { cancellation.borrow().clone() };
                    if let CancellationRequest::Requested { reason } = request {
                        return Err(QueryTaskError::Cancelled(reason));
                    }
                    continue;
                }
                row = result.next() => row,
            };
            let Some(row) = next.map_err(mysql_query_error)? else {
                return Ok((
                    wire::QueryCompleted {
                        row_count,
                        truncated_by_max_rows: false,
                        truncated_by_max_result_bytes: false,
                    },
                    false,
                ));
            };

            if max_rows != 0 && row_count >= max_rows {
                return Ok((
                    wire::QueryCompleted {
                        row_count,
                        truncated_by_max_rows: true,
                        truncated_by_max_result_bytes: false,
                    },
                    true,
                ));
            }
            let row = mysql_row(row, &columns)?;
            let row_bytes = u64::try_from(row.encoded_len())
                .map_err(|_| QueryTaskError::Failed(AppError::internal()))?;
            if result_bytes.saturating_add(row_bytes) > max_result_bytes {
                return Ok((
                    wire::QueryCompleted {
                        row_count,
                        truncated_by_max_rows: false,
                        truncated_by_max_result_bytes: true,
                    },
                    true,
                ));
            }
            let entry_bytes = row_batch_entry_bytes(&row)?;
            let candidate_bytes = pending_bytes
                .saturating_add(if pending_rows.is_empty() {
                    row_batch_prefix_bytes(row_count)
                } else {
                    0
                })
                .saturating_add(entry_bytes);
            if !pending_rows.is_empty()
                && (pending_rows.len()
                    >= usize::try_from(batch_rows)
                        .map_err(|_| QueryTaskError::Failed(AppError::internal()))?
                    || candidate_bytes > u64::from(batch_bytes))
            {
                flush_rows(
                    application,
                    operation_id,
                    &mut writer,
                    &mut pending_rows,
                    row_count,
                )
                .await?;
                pending_bytes = 0;
            }
            if pending_rows.is_empty() {
                pending_bytes = row_batch_prefix_bytes(row_count);
            }
            pending_rows.push(row);
            pending_bytes = pending_bytes.saturating_add(entry_bytes);
            row_count = row_count
                .checked_add(1)
                .ok_or_else(|| QueryTaskError::Failed(AppError::internal()))?;
            result_bytes = result_bytes
                .checked_add(row_bytes)
                .ok_or_else(|| QueryTaskError::Failed(AppError::internal()))?;
        }
    }
    .await;
    drop(result);

    let (completion, requires_termination) = match consumption {
        Ok(completion) => completion,
        Err(primary) => {
            let termination = terminate_connection(options, connection_id).await;
            abort_writer(&mut writer).await;
            drop(conn);
            if let Err(error) = termination {
                if matches!(primary, QueryTaskError::Cancelled(_)) {
                    return Err(error.into());
                }
                tracing::warn!(error = %error, "native MySQL connection termination failed");
            }
            return Err(primary);
        }
    };
    if requires_termination && let Err(error) = terminate_connection(options, connection_id).await {
        abort_writer(&mut writer).await;
        drop(conn);
        return Err(error.into());
    }

    let finalized = async {
        flush_rows(
            application,
            operation_id,
            &mut writer,
            &mut pending_rows,
            row_count,
        )
        .await?;
        writer
            .finish(completion)
            .await
            .map_err(QueryTaskError::from)
    }
    .await;
    let metadata = match finalized {
        Ok(metadata) => metadata,
        Err(error) => {
            abort_writer(&mut writer).await;
            if requires_termination {
                drop(conn);
            } else {
                finish_read_only_connection_quietly(conn).await;
            }
            return Err(error);
        }
    };
    if requires_termination {
        drop(conn);
    } else {
        finish_read_only_connection_quietly(conn).await;
    }
    Ok(metadata)
}

fn row_batch_prefix_bytes(start_row_offset: u64) -> u64 {
    if start_row_offset == 0 {
        0
    } else {
        1_u64.saturating_add(
            u64::try_from(prost::encoding::encoded_len_varint(start_row_offset))
                .unwrap_or(u64::MAX),
        )
    }
}

fn row_batch_entry_bytes(row: &wire::JdbcRow) -> Result<u64, QueryTaskError> {
    let row_bytes = row.encoded_len();
    let length_bytes = prost::encoding::length_delimiter_len(row_bytes);
    u64::try_from(
        1_usize
            .saturating_add(length_bytes)
            .saturating_add(row_bytes),
    )
    .map_err(|_| QueryTaskError::Failed(AppError::internal()))
}

async fn flush_rows(
    application: &Application,
    operation_id: &str,
    writer: &mut RetainedWriter,
    rows: &mut Vec<wire::JdbcRow>,
    row_count: u64,
) -> Result<(), QueryTaskError> {
    if rows.is_empty() {
        return Ok(());
    }
    let row_len = u64::try_from(rows.len()).map_err(|_| AppError::internal())?;
    let start_row_offset = row_count
        .checked_sub(row_len)
        .ok_or_else(AppError::internal)?;
    let batch = wire::RowBatch {
        start_row_offset,
        rows: std::mem::take(rows),
    };
    if batch.encoded_len() > usize::try_from(MAX_BATCH_BYTES).unwrap_or(usize::MAX) {
        return Err(resource_error(
            "mysql_result_batch_too_large",
            "One MySQL result row exceeds the retained-result batch limit",
        )
        .into());
    }
    let byte_count = writer.append(batch).await?;
    application
        .inner
        .operations
        .progress(operation_id, row_count, byte_count)
        .await?;
    Ok(())
}

async fn abort_writer(writer: &mut RetainedWriter) {
    if let Err(error) = writer.abort().await {
        tracing::warn!(error = %error, "native MySQL retained-result cleanup failed");
    }
}

fn mysql_column(index: usize, column: &Column) -> Result<wire::JdbcColumn, AppError> {
    let ordinal = u32::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .ok_or_else(AppError::internal)?;
    let label = column.name_str().into_owned();
    let original_name = column.org_name_str().into_owned();
    let table_name = column.org_table_str().into_owned();
    let catalog_name = column.schema_str().into_owned();
    let column_type = column.column_type();
    let flags = column.flags();
    let value_type = mysql_value_type(column);
    Ok(wire::JdbcColumn {
        ordinal,
        name: if original_name.is_empty() {
            label.clone()
        } else {
            original_name
        },
        label,
        jdbc_type: mysql_jdbc_type(column),
        jdbc_type_name: mysql_type_name(column_type).to_owned(),
        value_type: value_type as i32,
        nullability: if flags.contains(ColumnFlags::NOT_NULL_FLAG) {
            wire::ColumnNullability::NoNulls as i32
        } else {
            wire::ColumnNullability::Nullable as i32
        },
        precision: numeric_type(column_type).then_some(column.column_length()),
        scale: numeric_type(column_type).then(|| i32::from(column.decimals())),
        display_size: Some(column.column_length()),
        signed: numeric_type(column_type).then_some(!flags.contains(ColumnFlags::UNSIGNED_FLAG)),
        catalog_name: (!catalog_name.is_empty()).then_some(catalog_name),
        schema_name: None,
        table_name: (!table_name.is_empty()).then_some(table_name),
    })
}

fn mysql_row(row: Row, columns: &[Column]) -> Result<wire::JdbcRow, AppError> {
    if row.len() != columns.len() {
        return Err(AppError::internal());
    }
    Ok(wire::JdbcRow {
        values: row
            .unwrap()
            .into_iter()
            .zip(columns)
            .map(|(value, column)| mysql_value(value, column))
            .collect::<Result<_, _>>()?,
    })
}

fn mysql_value(value: Value, column: &Column) -> Result<wire::JdbcValue, AppError> {
    use wire::jdbc_value::Value as WireValue;
    if matches!(value, Value::NULL) {
        return Ok(wire_value(WireValue::NullValue(wire::JdbcNull {})));
    }
    let value_type = mysql_value_type(column);
    let value = match value_type {
        wire::JdbcValueType::Boolean => WireValue::BooleanValue(mysql_bool(value)?),
        wire::JdbcValueType::SignedInteger => WireValue::SignedIntegerValue(mysql_i64(value)?),
        wire::JdbcValueType::UnsignedInteger => WireValue::UnsignedIntegerValue(mysql_u64(value)?),
        wire::JdbcValueType::Float32 => WireValue::Float32Value(mysql_f32(value)?),
        wire::JdbcValueType::Float64 => WireValue::Float64Value(mysql_f64(value)?),
        wire::JdbcValueType::Decimal => WireValue::DecimalValue(mysql_text(value)?),
        wire::JdbcValueType::Text => WireValue::TextValue(mysql_text(value)?),
        wire::JdbcValueType::Binary => WireValue::BinaryValue(mysql_binary(value)?),
        wire::JdbcValueType::Date => match value {
            Value::Date(0, 0, 0, _, _, _, _) => WireValue::NullValue(wire::JdbcNull {}),
            Value::Date(year, month, day, _, _, _, _) => {
                WireValue::DateValue(format!("{year:04}-{month:02}-{day:02}"))
            }
            other => WireValue::DateValue(mysql_text(other)?),
        },
        wire::JdbcValueType::Time => WireValue::TimeValue(mysql_time(value)?),
        wire::JdbcValueType::Timestamp => match value {
            Value::Date(0, 0, 0, _, _, _, _) => WireValue::NullValue(wire::JdbcNull {}),
            Value::Date(year, month, day, hour, minute, second, micros) => {
                WireValue::TimestampValue(format_timestamp(
                    year, month, day, hour, minute, second, micros,
                ))
            }
            other => WireValue::TimestampValue(mysql_text(other)?),
        },
        wire::JdbcValueType::Json => WireValue::JsonValue(mysql_text(value)?),
        wire::JdbcValueType::Opaque
        | wire::JdbcValueType::TimestampWithTimeZone
        | wire::JdbcValueType::Uuid
        | wire::JdbcValueType::Unspecified => WireValue::OpaqueValue(wire::OpaqueValue {
            type_name: mysql_type_name(column.column_type()).to_owned(),
            display_value: mysql_display(value),
        }),
    };
    Ok(wire_value(value))
}

fn wire_value(value: wire::jdbc_value::Value) -> wire::JdbcValue {
    wire::JdbcValue { value: Some(value) }
}

fn mysql_value_type(column: &Column) -> wire::JdbcValueType {
    use ColumnType as Type;
    if is_binary_column(column) {
        return wire::JdbcValueType::Binary;
    }
    match column.column_type() {
        Type::MYSQL_TYPE_TINY
        | Type::MYSQL_TYPE_SHORT
        | Type::MYSQL_TYPE_LONG
        | Type::MYSQL_TYPE_LONGLONG
        | Type::MYSQL_TYPE_INT24
        | Type::MYSQL_TYPE_YEAR => {
            if column.flags().contains(ColumnFlags::UNSIGNED_FLAG) {
                wire::JdbcValueType::UnsignedInteger
            } else {
                wire::JdbcValueType::SignedInteger
            }
        }
        Type::MYSQL_TYPE_FLOAT => wire::JdbcValueType::Float32,
        Type::MYSQL_TYPE_DOUBLE => wire::JdbcValueType::Float64,
        Type::MYSQL_TYPE_DECIMAL | Type::MYSQL_TYPE_NEWDECIMAL => wire::JdbcValueType::Decimal,
        Type::MYSQL_TYPE_BIT if column.column_length() == 1 => wire::JdbcValueType::Boolean,
        Type::MYSQL_TYPE_BIT | Type::MYSQL_TYPE_GEOMETRY | Type::MYSQL_TYPE_VECTOR => {
            wire::JdbcValueType::Binary
        }
        Type::MYSQL_TYPE_DATE | Type::MYSQL_TYPE_NEWDATE => wire::JdbcValueType::Date,
        Type::MYSQL_TYPE_TIME | Type::MYSQL_TYPE_TIME2 => wire::JdbcValueType::Time,
        Type::MYSQL_TYPE_TIMESTAMP
        | Type::MYSQL_TYPE_TIMESTAMP2
        | Type::MYSQL_TYPE_DATETIME
        | Type::MYSQL_TYPE_DATETIME2 => wire::JdbcValueType::Timestamp,
        Type::MYSQL_TYPE_JSON => wire::JdbcValueType::Json,
        Type::MYSQL_TYPE_VARCHAR
        | Type::MYSQL_TYPE_VAR_STRING
        | Type::MYSQL_TYPE_STRING
        | Type::MYSQL_TYPE_ENUM
        | Type::MYSQL_TYPE_SET
        | Type::MYSQL_TYPE_TINY_BLOB
        | Type::MYSQL_TYPE_MEDIUM_BLOB
        | Type::MYSQL_TYPE_LONG_BLOB
        | Type::MYSQL_TYPE_BLOB => wire::JdbcValueType::Text,
        Type::MYSQL_TYPE_NULL | Type::MYSQL_TYPE_TYPED_ARRAY | Type::MYSQL_TYPE_UNKNOWN => {
            wire::JdbcValueType::Opaque
        }
    }
}

fn mysql_jdbc_type(column: &Column) -> i32 {
    use ColumnType as Type;
    match column.column_type() {
        Type::MYSQL_TYPE_BIT => -7,
        Type::MYSQL_TYPE_TINY => -6,
        Type::MYSQL_TYPE_SHORT | Type::MYSQL_TYPE_YEAR => 5,
        Type::MYSQL_TYPE_LONG | Type::MYSQL_TYPE_INT24 => 4,
        Type::MYSQL_TYPE_LONGLONG => -5,
        Type::MYSQL_TYPE_FLOAT => 6,
        Type::MYSQL_TYPE_DOUBLE => 8,
        Type::MYSQL_TYPE_DECIMAL | Type::MYSQL_TYPE_NEWDECIMAL => 3,
        Type::MYSQL_TYPE_DATE | Type::MYSQL_TYPE_NEWDATE => 91,
        Type::MYSQL_TYPE_TIME | Type::MYSQL_TYPE_TIME2 => 92,
        Type::MYSQL_TYPE_TIMESTAMP
        | Type::MYSQL_TYPE_TIMESTAMP2
        | Type::MYSQL_TYPE_DATETIME
        | Type::MYSQL_TYPE_DATETIME2 => 93,
        Type::MYSQL_TYPE_VARCHAR | Type::MYSQL_TYPE_VAR_STRING => 12,
        Type::MYSQL_TYPE_STRING | Type::MYSQL_TYPE_ENUM | Type::MYSQL_TYPE_SET => 1,
        Type::MYSQL_TYPE_TINY_BLOB
        | Type::MYSQL_TYPE_MEDIUM_BLOB
        | Type::MYSQL_TYPE_LONG_BLOB
        | Type::MYSQL_TYPE_BLOB => {
            if mysql_value_type(column) == wire::JdbcValueType::Binary {
                -4
            } else {
                -1
            }
        }
        Type::MYSQL_TYPE_JSON => -1,
        Type::MYSQL_TYPE_GEOMETRY | Type::MYSQL_TYPE_VECTOR => -4,
        Type::MYSQL_TYPE_NULL => 0,
        Type::MYSQL_TYPE_TYPED_ARRAY | Type::MYSQL_TYPE_UNKNOWN => 1111,
    }
}

fn mysql_type_name(column_type: ColumnType) -> &'static str {
    use ColumnType as Type;
    match column_type {
        Type::MYSQL_TYPE_DECIMAL | Type::MYSQL_TYPE_NEWDECIMAL => "DECIMAL",
        Type::MYSQL_TYPE_TINY => "TINYINT",
        Type::MYSQL_TYPE_SHORT => "SMALLINT",
        Type::MYSQL_TYPE_LONG => "INT",
        Type::MYSQL_TYPE_FLOAT => "FLOAT",
        Type::MYSQL_TYPE_DOUBLE => "DOUBLE",
        Type::MYSQL_TYPE_NULL => "NULL",
        Type::MYSQL_TYPE_TIMESTAMP | Type::MYSQL_TYPE_TIMESTAMP2 => "TIMESTAMP",
        Type::MYSQL_TYPE_LONGLONG => "BIGINT",
        Type::MYSQL_TYPE_INT24 => "MEDIUMINT",
        Type::MYSQL_TYPE_DATE | Type::MYSQL_TYPE_NEWDATE => "DATE",
        Type::MYSQL_TYPE_TIME | Type::MYSQL_TYPE_TIME2 => "TIME",
        Type::MYSQL_TYPE_DATETIME | Type::MYSQL_TYPE_DATETIME2 => "DATETIME",
        Type::MYSQL_TYPE_YEAR => "YEAR",
        Type::MYSQL_TYPE_VARCHAR => "VARCHAR",
        Type::MYSQL_TYPE_BIT => "BIT",
        Type::MYSQL_TYPE_TYPED_ARRAY => "TYPED_ARRAY",
        Type::MYSQL_TYPE_VECTOR => "VECTOR",
        Type::MYSQL_TYPE_UNKNOWN => "UNKNOWN",
        Type::MYSQL_TYPE_JSON => "JSON",
        Type::MYSQL_TYPE_ENUM => "ENUM",
        Type::MYSQL_TYPE_SET => "SET",
        Type::MYSQL_TYPE_TINY_BLOB => "TINYBLOB",
        Type::MYSQL_TYPE_MEDIUM_BLOB => "MEDIUMBLOB",
        Type::MYSQL_TYPE_LONG_BLOB => "LONGBLOB",
        Type::MYSQL_TYPE_BLOB => "BLOB",
        Type::MYSQL_TYPE_VAR_STRING => "VAR_STRING",
        Type::MYSQL_TYPE_STRING => "STRING",
        Type::MYSQL_TYPE_GEOMETRY => "GEOMETRY",
    }
}

fn numeric_type(column_type: ColumnType) -> bool {
    column_type.is_numeric_type()
}

fn is_binary_column(column: &Column) -> bool {
    matches!(
        column.column_type(),
        ColumnType::MYSQL_TYPE_GEOMETRY | ColumnType::MYSQL_TYPE_VECTOR
    ) || (matches!(
        column.column_type(),
        ColumnType::MYSQL_TYPE_TINY_BLOB
            | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
            | ColumnType::MYSQL_TYPE_LONG_BLOB
            | ColumnType::MYSQL_TYPE_BLOB
            | ColumnType::MYSQL_TYPE_VARCHAR
            | ColumnType::MYSQL_TYPE_VAR_STRING
            | ColumnType::MYSQL_TYPE_STRING
    ) && (column.character_set() == 63 || column.flags().contains(ColumnFlags::BINARY_FLAG)))
}

fn mysql_bool(value: Value) -> Result<bool, AppError> {
    match value {
        Value::Int(value) => Ok(value != 0),
        Value::UInt(value) => Ok(value != 0),
        Value::Bytes(value) => Ok(value.iter().any(|byte| *byte != 0)),
        _ => Err(result_decode_error()),
    }
}

fn mysql_i64(value: Value) -> Result<i64, AppError> {
    match value {
        Value::Int(value) => Ok(value),
        Value::UInt(value) => i64::try_from(value).map_err(|_| result_decode_error()),
        Value::Bytes(value) => mysql_utf8(value)?
            .parse()
            .map_err(|_| result_decode_error()),
        _ => Err(result_decode_error()),
    }
}

fn mysql_u64(value: Value) -> Result<u64, AppError> {
    match value {
        Value::UInt(value) => Ok(value),
        Value::Int(value) => u64::try_from(value).map_err(|_| result_decode_error()),
        Value::Bytes(value) => mysql_utf8(value)?
            .parse()
            .map_err(|_| result_decode_error()),
        _ => Err(result_decode_error()),
    }
}

fn mysql_f32(value: Value) -> Result<f32, AppError> {
    match value {
        Value::Float(value) => Ok(value),
        Value::Bytes(value) => mysql_utf8(value)?
            .parse()
            .map_err(|_| result_decode_error()),
        _ => Err(result_decode_error()),
    }
}

fn mysql_f64(value: Value) -> Result<f64, AppError> {
    match value {
        Value::Double(value) => Ok(value),
        Value::Float(value) => Ok(f64::from(value)),
        Value::Bytes(value) => mysql_utf8(value)?
            .parse()
            .map_err(|_| result_decode_error()),
        _ => Err(result_decode_error()),
    }
}

fn mysql_text(value: Value) -> Result<String, AppError> {
    match value {
        Value::Bytes(value) => mysql_utf8(value),
        Value::Int(value) => Ok(value.to_string()),
        Value::UInt(value) => Ok(value.to_string()),
        Value::Float(value) => Ok(value.to_string()),
        Value::Double(value) => Ok(value.to_string()),
        Value::Date(year, month, day, hour, minute, second, micros) => Ok(format_timestamp(
            year, month, day, hour, minute, second, micros,
        )),
        Value::Time(negative, days, hours, minutes, seconds, micros) => {
            Ok(format_time(negative, days, hours, minutes, seconds, micros))
        }
        Value::NULL => Err(result_decode_error()),
    }
}

fn mysql_binary(value: Value) -> Result<Vec<u8>, AppError> {
    match value {
        Value::Bytes(value) if value.len() <= MAX_SCALAR_BYTES => Ok(value),
        Value::Bytes(_) => Err(resource_error(
            "mysql_scalar_too_large",
            format!("A MySQL value exceeds {MAX_SCALAR_BYTES} bytes"),
        )),
        other => Ok(mysql_text(other)?.into_bytes()),
    }
}

fn mysql_utf8(value: Vec<u8>) -> Result<String, AppError> {
    if value.len() > MAX_SCALAR_BYTES {
        return Err(resource_error(
            "mysql_scalar_too_large",
            format!("A MySQL value exceeds {MAX_SCALAR_BYTES} bytes"),
        ));
    }
    String::from_utf8(value).map_err(|_| result_decode_error())
}

fn mysql_time(value: Value) -> Result<String, AppError> {
    match value {
        Value::Time(negative, days, hours, minutes, seconds, micros) => {
            Ok(format_time(negative, days, hours, minutes, seconds, micros))
        }
        other => mysql_text(other),
    }
}

fn format_timestamp(
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    micros: u32,
) -> String {
    let base = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}");
    if micros == 0 {
        base
    } else {
        format!("{base}.{micros:06}")
    }
}

fn format_time(
    negative: bool,
    days: u32,
    hours: u8,
    minutes: u8,
    seconds: u8,
    micros: u32,
) -> String {
    let sign = if negative { "-" } else { "" };
    let hours = u64::from(days) * 24 + u64::from(hours);
    let base = format!("{sign}{hours:02}:{minutes:02}:{seconds:02}");
    if micros == 0 {
        base
    } else {
        format!("{base}.{micros:06}")
    }
}

fn mysql_display(value: Value) -> String {
    mysql_text(value).unwrap_or_else(|_| "[unavailable]".to_owned())
}

fn validate_query_options(options: QueryOptions) -> Result<(), AppError> {
    if options.target_batch_rows > MAX_BATCH_ROWS {
        return Err(AppError::invalid(
            "invalid_query_limits",
            format!("batchRows must be at most {MAX_BATCH_ROWS}"),
        ));
    }
    if options.target_batch_bytes != 0
        && !(1024..=MAX_BATCH_BYTES).contains(&options.target_batch_bytes)
    {
        return Err(AppError::invalid(
            "invalid_query_limits",
            format!("batchBytes must be zero or between 1024 and {MAX_BATCH_BYTES}"),
        ));
    }
    if options.max_result_bytes > MAX_RESULT_BYTES {
        return Err(AppError::invalid(
            "invalid_query_limits",
            format!("maxResultBytes must be at most {MAX_RESULT_BYTES}"),
        ));
    }
    Ok(())
}

fn quote_identifier(value: &str, field: &str) -> Result<String, AppError> {
    if value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES || value.contains('\0') {
        return Err(AppError::invalid(
            "invalid_community_table_preview_request",
            format!("{field} is invalid"),
        ));
    }
    Ok(format!("`{}`", value.replace('`', "``")))
}

async fn terminate_connection(options: Opts, connection_id: u32) -> Result<(), AppError> {
    let mut control = open_connection_with_opts(options).await?;
    let Ok(result) = tokio::time::timeout(
        CONTROL_TIMEOUT,
        control.query_drop(format!("KILL CONNECTION {connection_id}")),
    )
    .await
    else {
        drop(control);
        return Err(AppError::unavailable(
            "mysql_termination_timeout",
            "The MySQL query connection could not be terminated in time",
        ));
    };
    match result {
        Ok(()) => {
            disconnect_quietly(control).await;
            Ok(())
        }
        Err(MysqlError::Server(server)) if server.code == 1094 => {
            disconnect_quietly(control).await;
            Ok(())
        }
        Err(error) => {
            disconnect_quietly(control).await;
            Err(mysql_query_error(error))
        }
    }
}

async fn terminate_connection_quietly(options: Opts, connection_id: u32) {
    if let Err(error) = terminate_connection(options, connection_id).await {
        tracing::warn!(error = %error, "native MySQL connection termination failed");
    }
}

async fn open_query_connection(
    options: Opts,
    cancellation: &mut watch::Receiver<CancellationRequest>,
) -> Result<Conn, QueryTaskError> {
    let open = open_connection_with_opts(options);
    tokio::pin!(open);
    let mut cancellation_open = true;
    loop {
        tokio::select! {
            biased;
            changed = cancellation.changed(), if cancellation_open => {
                if changed.is_err() {
                    cancellation_open = false;
                    continue;
                }
                let request = { cancellation.borrow().clone() };
                if let CancellationRequest::Requested { reason } = request {
                    return Err(QueryTaskError::Cancelled(reason));
                }
            }
            result = &mut open => return result.map_err(QueryTaskError::from),
        }
    }
}

async fn start_read_only_transaction(conn: &mut Conn) -> Result<(), AppError> {
    tokio::time::timeout(
        CONTROL_TIMEOUT,
        conn.query_drop("START TRANSACTION READ ONLY"),
    )
    .await
    .map_err(|_| {
        AppError::unavailable(
            "mysql_transaction_timeout",
            "The MySQL read-only transaction did not start in time",
        )
    })?
    .map_err(mysql_query_error)
}

async fn finish_read_only_connection_quietly(mut conn: Conn) {
    let rollback = tokio::time::timeout(CONTROL_TIMEOUT, conn.query_drop("ROLLBACK")).await;
    match rollback {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let error = mysql_query_error(error);
            tracing::warn!(error = %error, "native MySQL rollback failed");
        }
        Err(_) => tracing::warn!("native MySQL rollback timed out"),
    }
    disconnect_quietly(conn).await;
}

async fn disconnect_connection(conn: Conn) -> Result<(), AppError> {
    tokio::time::timeout(DISCONNECT_TIMEOUT, conn.disconnect())
        .await
        .map_err(|_| {
            AppError::unavailable(
                "mysql_disconnect_timeout",
                "The MySQL connection did not close in time",
            )
        })?
        .map_err(mysql_connection_error)
}

async fn disconnect_quietly(conn: Conn) {
    if let Err(error) = disconnect_connection(conn).await {
        tracing::warn!(error = %error, "native MySQL connection cleanup failed");
    }
}

fn resource_error(code: impl Into<String>, message: impl Into<String>) -> AppError {
    AppError::new(
        AppErrorKind::ResourceExhausted,
        ApiError::new(code, message),
    )
}

fn result_decode_error() -> AppError {
    AppError::new(
        AppErrorKind::Unavailable,
        ApiError::new(
            "mysql_result_decode_failed",
            "A MySQL result value could not be decoded safely",
        ),
    )
}

async fn resolve_native_connection(
    application: &Application,
    datasource_id: &str,
) -> Result<ResolvedDatasourceConnection, AppError> {
    let storage = application.require_storage()?;
    let resolved = resolve_datasource_connection(&storage, datasource_id).await?;
    if !application.is_native_mysql_driver(&resolved.driver_id) {
        return Err(AppError::invalid(
            "mysql_driver_mismatch",
            "The datasource is not configured with a MySQL driver",
        ));
    }
    Ok(resolved)
}

async fn open_connection(connection: &DatasourceConnection) -> Result<Conn, AppError> {
    let opts = connection_opts(connection)?;
    open_connection_with_opts(opts).await
}

async fn open_connection_with_opts(opts: Opts) -> Result<Conn, AppError> {
    tokio::time::timeout(CONNECT_TIMEOUT, Conn::new(opts))
        .await
        .map_err(|_| {
            AppError::unavailable(
                "mysql_connection_timeout",
                "The MySQL connection attempt timed out",
            )
        })?
        .map_err(mysql_connection_error)
}

async fn finish_connection<T>(conn: Conn, result: Result<T, AppError>) -> Result<T, AppError> {
    let close = conn.disconnect().await.map_err(mysql_connection_error);
    match result {
        Ok(value) => close.map(|()| value),
        Err(error) => {
            if let Err(close_error) = close {
                tracing::warn!(error = %close_error, "native MySQL connection cleanup failed");
            }
            Err(error)
        }
    }
}

fn connection_opts(connection: &DatasourceConnection) -> Result<Opts, AppError> {
    let url = normalize_mysql_url(&connection.jdbc_url)?;
    let mut parsed = Url::parse(&url).map_err(|_| invalid_connection_url())?;
    if parsed.scheme() != "mysql" || parsed.host_str().is_none() {
        return Err(invalid_connection_url());
    }
    let query_properties = parsed
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    parsed.set_query(None);
    parsed.set_fragment(None);
    let base = Opts::from_url(parsed.as_str()).map_err(|_| invalid_connection_url())?;
    let mut builder = OptsBuilder::from_opts(base).prefer_socket(Some(false));

    let mut ssl = None;
    for (key, value) in query_properties
        .iter()
        .map(|(key, value)| (key, value))
        .chain(
            connection
                .properties
                .iter()
                .map(|property| (&property.key, &property.value)),
        )
    {
        match key.trim().to_ascii_lowercase().as_str() {
            "user" | "username" => builder = builder.user(Some(value.to_owned())),
            "password" => builder = builder.pass(Some(value.to_owned())),
            "database" | "databasename" => builder = builder.db_name(Some(value.to_owned())),
            "usessl" | "requiressl" => {
                ssl = parse_bool(value).then(SslOpts::default);
            }
            "sslmode" => {
                ssl = match value.trim().to_ascii_lowercase().as_str() {
                    "disable" | "disabled" | "false" | "preferred" => None,
                    "require" | "required" | "true" => Some(
                        SslOpts::default()
                            .with_danger_accept_invalid_certs(true)
                            .with_danger_skip_domain_validation(true),
                    ),
                    "verify_ca" => {
                        Some(SslOpts::default().with_danger_skip_domain_validation(true))
                    }
                    "verify_identity" => Some(SslOpts::default()),
                    _ => return Err(invalid_connection_property("sslMode")),
                };
            }
            "verifyservercertificate" if !parse_bool(value) && ssl.is_some() => {
                ssl = ssl.map(|options| {
                    options
                        .with_danger_accept_invalid_certs(true)
                        .with_danger_skip_domain_validation(true)
                });
            }
            _ => {}
        }
    }
    Ok(builder.ssl_opts(ssl).into())
}

fn normalize_mysql_url(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value
        .get(..JDBC_MYSQL_SCHEME.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(JDBC_MYSQL_SCHEME))
    {
        return Ok(format!(
            "{MYSQL_SCHEME}{}",
            &value[JDBC_MYSQL_SCHEME.len()..]
        ));
    }
    if value
        .get(..MYSQL_SCHEME.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(MYSQL_SCHEME))
    {
        return Ok(format!("{MYSQL_SCHEME}{}", &value[MYSQL_SCHEME.len()..]));
    }
    Err(invalid_connection_url())
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "required"
    )
}

fn normalize_table_type(value: &str) -> &str {
    if value.eq_ignore_ascii_case("VIEW") {
        "VIEW"
    } else {
        "TABLE"
    }
}

fn is_system_database(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "information_schema" | "mysql" | "performance_schema" | "sys"
    )
}

fn invalid_connection_url() -> AppError {
    AppError::invalid(
        "invalid_mysql_connection",
        "A valid jdbc:mysql:// or mysql:// connection URL is required",
    )
}

fn invalid_connection_property(property: &str) -> AppError {
    AppError::invalid(
        "invalid_mysql_connection",
        format!("The MySQL connection property {property} is invalid"),
    )
}

fn mysql_connection_error(error: MysqlError) -> AppError {
    match error {
        MysqlError::Server(server) => AppError::new(
            AppErrorKind::InvalidRequest,
            ApiError::new("mysql_connection_rejected", server.message),
        ),
        _ => AppError::unavailable(
            "mysql_connection_failed",
            "The MySQL server could not be reached",
        ),
    }
}

fn mysql_query_error(error: MysqlError) -> AppError {
    match error {
        MysqlError::Server(server) => AppError::new(
            AppErrorKind::InvalidRequest,
            ApiError::new("mysql_query_failed", server.message),
        ),
        _ => AppError::unavailable(
            "mysql_connection_failed",
            "The MySQL connection ended before the operation completed",
        ),
    }
}

#[cfg(test)]
mod tests {
    use chat2db_contract::{DatasourceConnection, DatasourceConnectionProperty};

    use super::{
        connection_opts, is_mysql_database_type, is_native_read_candidate, normalize_table_type,
        quote_identifier, validate_read_sql,
    };

    #[test]
    fn jdbc_url_and_properties_build_native_options_without_exposing_jdbc() {
        let opts = connection_opts(&DatasourceConnection {
            jdbc_url: "jdbc:mysql://db.example:3307/app?useSSL=false&serverTimezone=UTC".to_owned(),
            properties: vec![
                DatasourceConnectionProperty {
                    key: "user".to_owned(),
                    value: "chat2db".to_owned(),
                    sensitive: false,
                },
                DatasourceConnectionProperty {
                    key: "password".to_owned(),
                    value: "secret".to_owned(),
                    sensitive: true,
                },
            ],
            read_only: false,
        })
        .expect("JDBC URL should convert");

        assert_eq!(opts.ip_or_hostname(), "db.example");
        assert_eq!(opts.tcp_port(), 3307);
        assert_eq!(opts.db_name(), Some("app"));
        assert_eq!(opts.user(), Some("chat2db"));
        assert_eq!(opts.pass(), Some("secret"));
        assert!(opts.ssl_opts().is_none());
        assert!(!opts.prefer_socket());
    }

    #[test]
    fn mysql_detection_and_table_types_are_closed() {
        assert!(is_mysql_database_type(" mysql "));
        assert!(!is_mysql_database_type("mariadb"));
        assert_eq!(normalize_table_type("VIEW"), "VIEW");
        assert_eq!(normalize_table_type("BASE TABLE"), "TABLE");
    }

    #[test]
    fn explicit_properties_override_url_values_and_ssl_modes_are_mapped() {
        let opts = connection_opts(&DatasourceConnection {
            jdbc_url: "mysql://url-user:url-pass@localhost/url_db?sslMode=VERIFY_IDENTITY"
                .to_owned(),
            properties: vec![
                DatasourceConnectionProperty {
                    key: "user".to_owned(),
                    value: "property-user".to_owned(),
                    sensitive: false,
                },
                DatasourceConnectionProperty {
                    key: "password".to_owned(),
                    value: "property-pass".to_owned(),
                    sensitive: true,
                },
            ],
            read_only: false,
        })
        .expect("native URL should convert");

        assert_eq!(opts.user(), Some("property-user"));
        assert_eq!(opts.pass(), Some("property-pass"));
        assert_eq!(opts.db_name(), Some("url_db"));
        assert!(opts.ssl_opts().is_some());
    }

    #[test]
    fn invalid_and_non_mysql_urls_are_rejected_without_panicking() {
        for jdbc_url in [
            "",
            "jdbc:postgresql://localhost/app",
            "数mysql://localhost/app",
        ] {
            let error = connection_opts(&DatasourceConnection {
                jdbc_url: jdbc_url.to_owned(),
                properties: Vec::new(),
                read_only: false,
            })
            .expect_err("non-MySQL URLs must fail");
            assert_eq!(error.api_error().code, "invalid_mysql_connection");
        }
    }

    #[test]
    fn native_query_selection_is_read_only_and_conservative() {
        for sql in [
            "SELECT 1",
            "  select * from items;",
            "/* leading comment */ SELECT ' FOR UPDATE'",
        ] {
            assert!(is_native_read_candidate(sql).expect("SQL should tokenize"));
            validate_read_sql(sql).expect("plain SELECT should be native");
        }
        assert!(
            !is_native_read_candidate("UPDATE items SET label = 'changed'")
                .expect("SQL should tokenize")
        );
        assert!(
            is_native_read_candidate("WITH cte AS (SELECT 1) SELECT * FROM cte")
                .expect("SQL should tokenize")
        );

        for sql in [
            "WITH cte AS (SELECT 1) SELECT * FROM cte",
            "SELECT * FROM items FOR\nUPDATE",
            "SELECT * FROM items FOR/**/UPDATE",
            "SELECT 1 INTO/**/OUTFILE '/tmp/result'",
            "SELECT 1; UPDATE items SET label = 'changed'",
        ] {
            assert!(validate_read_sql(sql).is_err(), "{sql} must be rejected");
        }
    }

    #[test]
    fn preview_identifiers_are_quoted_as_one_mysql_identifier() {
        assert_eq!(
            quote_identifier("odd`name", "tableName").expect("identifier should quote"),
            "`odd``name`"
        );
        assert!(quote_identifier("", "tableName").is_err());
        assert!(quote_identifier("bad\0name", "tableName").is_err());
    }
}
