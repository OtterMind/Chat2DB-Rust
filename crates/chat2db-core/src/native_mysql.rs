use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use chat2db_contract::{
    ApiError, DatasourceConnection, JdbcValue, JdbcValueType, QueryLimits, ResultColumn,
    ResultMetadata, ResultRow, StartQueryRequest,
};
use chat2db_engine_protocol::wire;
use chat2db_storage::Storage;
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Timelike, Utc};
use mysql_async::{
    Column, Conn, DriverError, Error as MysqlError, Opts, OptsBuilder, Params, Row, SslOpts, Value,
    consts::{ColumnFlags, ColumnType},
    prelude::{FromRow, FromValue, Queryable},
};
use prost::Message;
use sqlparser::{ast::Statement, dialect::MySqlDialect, parser::Parser};
use std::{
    collections::HashMap,
    future::Future,
    mem::size_of,
    ops::{Deref, DerefMut},
    time::{Duration, Instant},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    AppError, AppErrorKind, Application,
    datasource_session::{ResolvedDatasourceConnection, resolve_datasource_connection},
    native_driver_types::{
        ColumnList, ColumnMetadata, DatabaseList, DatabaseMetadata, EntityRelationColumn,
        EntityRelationForeignKey, EntityRelationTable, ForeignKeyList, ForeignKeyMetadata,
        FunctionList, FunctionMetadata, FunctionParameterList, FunctionParameterMetadata,
        IndexColumnMetadata, IndexList, IndexMetadata, PrimaryKeyList, PrimaryKeyMetadata,
        ProcedureList, ProcedureMetadata, ProcedureParameterList, ProcedureParameterMetadata,
        RoutineInvocationPreview, RoutineInvocationRequest, RoutineMigrationExecution,
        RoutineMigrationRequest, SchemaList, TableList, TableMetadata, TablePreviewAccepted,
        TablePreviewRequest, TriggerList, TriggerMetadata, ViewList,
    },
    operation::CancellationRequest,
    query::{
        DatabaseValue, DatabaseWriteError, NativeConsoleRequest, NativeConsoleResult,
        PreparedQuery, QueryExecutionOptions, QueryParameter, QueryTaskError, RetainedWriter,
    },
    ssh::{SshTunnel, SshTunnelIdentity, mysql_target, rewrite_mysql_target},
};

const MYSQL_SCHEME: &str = "mysql://";
const JDBC_MYSQL_SCHEME: &str = "jdbc:mysql://";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
const METADATA_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_BATCH_ROWS: u32 = 256;
const DEFAULT_BATCH_BYTES: u32 = 256 * 1024;
const DEFAULT_RESULT_BYTES: u64 = wire::JdbcResultByteLimit::DefaultResultBytes as u64;
const MAX_RESULT_BYTES: u64 = wire::JdbcResultByteLimit::MaxResultBytes as u64;
const MAX_BATCH_ROWS: u32 = wire::JdbcProtocolLimit::MaxBatchRows as u32;
const MAX_BATCH_BYTES: u32 = wire::JdbcProtocolLimit::MaxBatchBytes as u32;
const MAX_COLUMNS: usize = wire::JdbcProtocolLimit::MaxColumns as usize;
const MAX_PARAMETERS: usize = wire::JdbcProtocolLimit::MaxParameters as usize;
const MAX_SQL_BYTES: usize = wire::JdbcProtocolLimit::MaxSqlBytes as usize;
const MAX_SCALAR_BYTES: usize = wire::JdbcProtocolLimit::MaxScalarBytes as usize;
const MAX_CONSOLE_VALUE_BYTES: usize = 32 * 1024 * 1024;
const MAX_CONSOLE_RESULT_BYTES: u64 = DEFAULT_RESULT_BYTES;
const MAX_CONSOLE_STATEMENTS: usize = 1_000;
const MAX_IDENTIFIER_BYTES: usize = 256;
const MAX_CONSOLE_PAGE_SIZE: u32 = 100_000;
const ER_TABLE_QUERY: &str = "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE, \
                              COALESCE(TABLE_COMMENT, ''), COALESCE(ENGINE, ''), \
                              COALESCE(TABLE_COLLATION, ''), CAST(AUTO_INCREMENT AS CHAR), \
                              CAST(TABLE_ROWS AS CHAR), CAST(DATA_LENGTH AS CHAR), \
                              DATE_FORMAT(CREATE_TIME, '%Y-%m-%dT%H:%i:%s'), \
                              DATE_FORMAT(UPDATE_TIME, '%Y-%m-%dT%H:%i:%s') \
                              FROM information_schema.TABLES \
                              WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE' \
                              ORDER BY TABLE_NAME";
const ER_COLUMN_QUERY: &str = "SELECT c.TABLE_NAME AS table_name, c.COLUMN_NAME AS name, \
                               c.DATA_TYPE AS data_type, c.COLUMN_DEFAULT AS default_value, \
                               COALESCE(c.EXTRA, '') AS extra, \
                               COALESCE(c.COLUMN_COMMENT, '') AS comment, \
                               COALESCE(c.COLUMN_KEY, '') AS column_key, \
                               c.IS_NULLABLE AS is_nullable, \
                               c.ORDINAL_POSITION AS ordinal_position, \
                               c.NUMERIC_SCALE AS numeric_scale, \
                               c.COLUMN_TYPE AS column_definition, \
                               c.CHARACTER_SET_NAME AS charset, c.COLLATION_NAME AS collation, \
                               CAST(COALESCE(pk.SEQ_IN_INDEX, 0) AS SIGNED) AS primary_key_order \
                               FROM information_schema.COLUMNS AS c \
                               LEFT JOIN information_schema.STATISTICS AS pk \
                                 ON pk.TABLE_SCHEMA = c.TABLE_SCHEMA \
                                AND pk.TABLE_NAME = c.TABLE_NAME \
                                AND pk.COLUMN_NAME = c.COLUMN_NAME \
                                AND pk.INDEX_NAME = 'PRIMARY' \
                               WHERE c.TABLE_SCHEMA = ? \
                               ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION";
const ER_FOREIGN_KEY_QUERY: &str = "SELECT kcu.REFERENCED_TABLE_SCHEMA, \
                                    kcu.REFERENCED_TABLE_NAME, kcu.REFERENCED_COLUMN_NAME, \
                                    kcu.TABLE_SCHEMA, kcu.TABLE_NAME, kcu.COLUMN_NAME, \
                                    kcu.ORDINAL_POSITION, rc.UPDATE_RULE, rc.DELETE_RULE, \
                                    kcu.CONSTRAINT_NAME, rc.UNIQUE_CONSTRAINT_NAME \
                                    FROM information_schema.KEY_COLUMN_USAGE kcu \
                                    JOIN information_schema.REFERENTIAL_CONSTRAINTS rc \
                                      ON rc.CONSTRAINT_SCHEMA = kcu.CONSTRAINT_SCHEMA \
                                     AND rc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME \
                                     AND rc.TABLE_NAME = kcu.TABLE_NAME \
                                    WHERE kcu.TABLE_SCHEMA = ? \
                                      AND kcu.REFERENCED_TABLE_NAME IS NOT NULL \
                                    ORDER BY kcu.TABLE_NAME, kcu.CONSTRAINT_NAME, \
                                             kcu.ORDINAL_POSITION";
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
#[derive(FromRow)]
#[mysql(crate_name = "mysql_async")]
struct ColumnRow {
    name: String,
    data_type: String,
    default_value: Option<String>,
    extra: String,
    comment: String,
    column_key: String,
    is_nullable: String,
    ordinal_position: i32,
    numeric_scale: Option<i32>,
    column_definition: String,
    charset: Option<String>,
    collation: Option<String>,
    primary_key_order: i32,
}
#[derive(FromRow)]
#[mysql(crate_name = "mysql_async")]
struct ErColumnRow {
    table_name: String,
    name: String,
    data_type: String,
    default_value: Option<String>,
    extra: String,
    comment: String,
    column_key: String,
    is_nullable: String,
    ordinal_position: i32,
    numeric_scale: Option<i32>,
    column_definition: String,
    charset: Option<String>,
    collation: Option<String>,
    primary_key_order: i32,
}
type IndexRow = (
    String,
    String,
    u8,
    String,
    String,
    i32,
    Option<String>,
    Option<String>,
    Option<u64>,
    Option<u64>,
    String,
    String,
);
type ForeignKeyRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    i32,
    String,
    String,
    String,
    Option<String>,
);
type RoutineParameterRow = (
    String,
    String,
    i32,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u32>,
    Option<u32>,
);
type RoutineInvocationParameterRow = (i32, Option<String>, Option<String>, String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MysqlRoutineType {
    Function,
    Procedure,
}

impl MysqlRoutineType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Function => "FUNCTION",
            Self::Procedure => "PROCEDURE",
        }
    }

    fn command(self) -> &'static str {
        match self {
            Self::Function => "select",
            Self::Procedure => "call",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RoutineInvocationParameter {
    name: String,
    mode: String,
    data_type: String,
    ordinal_position: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RoutineMigrationPlan {
    routine_type: MysqlRoutineType,
    database_name: String,
    routine_name: String,
    drop_sql: String,
    create_sql: String,
    preview_sql: String,
}

#[derive(Debug, PartialEq, Eq)]
enum SqlToken {
    Word(String),
    Semicolon,
}

pub(crate) struct ManagedMysqlConnection {
    connection: Option<Conn>,
    tunnel: Option<SshTunnel>,
}

impl ManagedMysqlConnection {
    fn new(connection: Conn, tunnel: Option<SshTunnel>) -> Self {
        Self {
            connection: Some(connection),
            tunnel,
        }
    }

    fn local_tunnel_port(&self) -> Option<u16> {
        self.tunnel.as_ref().map(SshTunnel::local_port)
    }
}

impl Deref for ManagedMysqlConnection {
    type Target = Conn;

    fn deref(&self) -> &Self::Target {
        self.connection
            .as_ref()
            .expect("managed MySQL connection must exist until cleanup")
    }
}

impl DerefMut for ManagedMysqlConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.connection
            .as_mut()
            .expect("managed MySQL connection must exist until cleanup")
    }
}

struct PreparedMysqlConnection {
    options: Opts,
    tunnel: Option<SshTunnel>,
}

pub(crate) async fn test_connection(connection: &DatasourceConnection) -> Result<(), AppError> {
    test_connection_with_local_port(connection)
        .await
        .map(|_| ())
}

pub(crate) async fn test_connection_with_local_port(
    connection: &DatasourceConnection,
) -> Result<Option<u16>, AppError> {
    let mut conn = open_connection(connection).await?;
    let local_port = conn.local_tunnel_port();
    let result = conn.ping().await.map_err(mysql_connection_error);
    finish_connection(conn, result).await?;
    Ok(local_port)
}

pub(crate) async fn list_databases(
    application: &Application,
    datasource_id: &str,
) -> Result<DatabaseList, AppError> {
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let result = metadata_query(conn.query::<(String, String, String), _>(
        "SELECT SCHEMA_NAME, DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME \
             FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME",
    ))
    .await
    .map(|rows| DatabaseList {
        items: rows
            .into_iter()
            .map(|(name, charset, collation)| DatabaseMetadata {
                system: is_system_database(&name),
                name,
                charset,
                collation,
                ..DatabaseMetadata::default()
            })
            .collect(),
    });
    finish_connection(conn, result).await
}

pub(crate) async fn list_schemas(
    application: &Application,
    datasource_id: &str,
) -> Result<SchemaList, AppError> {
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let conn = open_resolved_connection(&resolved).await?;
    finish_connection(conn, Ok(SchemaList::default())).await
}

pub(crate) async fn list_tables(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    table_name_pattern: &str,
) -> Result<TableList, AppError> {
    if database_name.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_mysql_metadata_request",
            "databaseName cannot be empty",
        ));
    }
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let query = "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE, COALESCE(TABLE_COMMENT, ''), \
                 COALESCE(ENGINE, ''), COALESCE(TABLE_COLLATION, ''), \
                 CAST(AUTO_INCREMENT AS CHAR), CAST(TABLE_ROWS AS CHAR), \
                 CAST(DATA_LENGTH AS CHAR), \
                 DATE_FORMAT(CREATE_TIME, '%Y-%m-%dT%H:%i:%s'), \
                 DATE_FORMAT(UPDATE_TIME, '%Y-%m-%dT%H:%i:%s') \
                 FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = ? \
                 AND TABLE_TYPE IN ('BASE TABLE', 'SYSTEM VIEW') \
                 AND (? = '' OR TABLE_NAME LIKE ?) \
                 ORDER BY TABLE_NAME";
    let pattern = table_name_pattern.trim().to_owned();
    let result = metadata_query(
        conn.exec::<TableRow, _, _>(query, (database_name.to_owned(), pattern.clone(), pattern)),
    )
    .await
    .map(|rows| TableList {
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
                )| TableMetadata {
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
                    ..TableMetadata::default()
                },
            )
            .collect(),
    });
    finish_connection(conn, result).await
}

pub(crate) async fn list_columns(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> Result<ColumnList, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    validate_metadata_identifier(table_name, "tableName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let query = "SELECT c.COLUMN_NAME AS name, c.DATA_TYPE AS data_type, \
                 c.COLUMN_DEFAULT AS default_value, COALESCE(c.EXTRA, '') AS extra, \
                 COALESCE(c.COLUMN_COMMENT, '') AS comment, \
                 COALESCE(c.COLUMN_KEY, '') AS column_key, c.IS_NULLABLE AS is_nullable, \
                 c.ORDINAL_POSITION AS ordinal_position, c.NUMERIC_SCALE AS numeric_scale, \
                 c.COLUMN_TYPE AS column_definition, c.CHARACTER_SET_NAME AS charset, \
                 c.COLLATION_NAME AS collation, \
                 CAST(COALESCE(pk.SEQ_IN_INDEX, 0) AS SIGNED) AS primary_key_order \
                 FROM information_schema.COLUMNS AS c \
                 LEFT JOIN information_schema.STATISTICS AS pk \
                   ON pk.TABLE_SCHEMA = c.TABLE_SCHEMA AND pk.TABLE_NAME = c.TABLE_NAME \
                  AND pk.COLUMN_NAME = c.COLUMN_NAME AND pk.INDEX_NAME = 'PRIMARY' \
                 WHERE c.TABLE_SCHEMA = ? AND c.TABLE_NAME = ? ORDER BY c.ORDINAL_POSITION";
    let result = metadata_query(
        conn.exec::<ColumnRow, _, _>(query, (database_name.to_owned(), table_name.to_owned())),
    )
    .await
    .map(|rows| ColumnList {
        items: rows
            .into_iter()
            .map(|row| column_metadata(database_name, schema_name, table_name, row))
            .collect(),
    });
    finish_connection(conn, result).await
}

pub(crate) async fn load_er_tables(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
) -> Result<Vec<EntityRelationTable>, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let result = async {
        let table_rows = metadata_query(
            conn.exec::<TableRow, _, _>(ER_TABLE_QUERY, (database_name.to_owned(),)),
        )
        .await?;
        let column_rows = metadata_query(
            conn.exec::<ErColumnRow, _, _>(ER_COLUMN_QUERY, (database_name.to_owned(),)),
        )
        .await?;
        let foreign_key_rows = metadata_query(
            conn.exec::<ForeignKeyRow, _, _>(ER_FOREIGN_KEY_QUERY, (database_name.to_owned(),)),
        )
        .await?;

        let mut columns_by_table = HashMap::<String, Vec<EntityRelationColumn>>::new();
        for row in column_rows {
            let ErColumnRow {
                table_name,
                name,
                data_type,
                default_value,
                extra,
                comment,
                column_key,
                is_nullable,
                ordinal_position,
                numeric_scale,
                column_definition,
                charset,
                collation,
                primary_key_order,
            } = row;
            let column = column_metadata(
                database_name,
                schema_name,
                &table_name,
                ColumnRow {
                    name,
                    data_type,
                    default_value,
                    extra,
                    comment,
                    column_key,
                    is_nullable,
                    ordinal_position,
                    numeric_scale,
                    column_definition,
                    charset,
                    collation,
                    primary_key_order,
                },
            );
            columns_by_table
                .entry(table_name)
                .or_default()
                .push(EntityRelationColumn {
                    name: column.name,
                    column_type: column.column_type,
                    primary_key: column.primary_key.unwrap_or(false),
                    comment: column.comment,
                });
        }

        let mut foreign_keys_by_table = HashMap::<String, Vec<EntityRelationForeignKey>>::new();
        for row in foreign_key_rows {
            let table_name = row.4.clone();
            foreign_keys_by_table
                .entry(table_name)
                .or_default()
                .push(EntityRelationForeignKey {
                    primary_table: row.1,
                    primary_column: row.2,
                    foreign_table: row.4,
                    foreign_column: row.5,
                });
        }

        Ok(table_rows
            .into_iter()
            .map(|row| {
                let table = table_metadata(row, schema_name);
                let name = table.name;
                EntityRelationTable {
                    comment: table.comment,
                    columns: columns_by_table.remove(&name).unwrap_or_default(),
                    foreign_keys: foreign_keys_by_table.remove(&name).unwrap_or_default(),
                    name,
                }
            })
            .collect())
    }
    .await;
    finish_connection(conn, result).await
}

pub(crate) async fn validate_column_reorder(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    table_name: &str,
    column_names: &[String],
) -> Result<(), AppError> {
    if column_names.is_empty() {
        return Ok(());
    }
    validate_metadata_identifier(database_name, "databaseName")?;
    validate_metadata_identifier(table_name, "tableName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let query = "SELECT COLUMN_NAME, COLUMN_TYPE, COALESCE(EXTRA, ''), \
                 COALESCE(GENERATION_EXPRESSION, '') \
                 FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?";
    let result = metadata_query(conn.exec::<(String, String, String, String), _, _>(
        query,
        (database_name.to_owned(), table_name.to_owned()),
    ))
    .await
    .and_then(|rows| {
        for column_name in column_names {
            let Some((_, column_type, extra, generation_expression)) = rows
                .iter()
                .find(|(name, _, _, _)| name.eq_ignore_ascii_case(column_name))
            else {
                return Err(AppError::invalid(
                    "invalid_mysql_ddl",
                    format!(
                        "Cannot safely reorder column {column_name}: its live metadata changed"
                    ),
                ));
            };
            if let Some(reason) =
                mysql_column_reorder_hazard(column_type, extra, generation_expression)
            {
                return Err(AppError::invalid(
                    "invalid_mysql_ddl",
                    format!("Cannot safely reorder column {column_name}: {reason}"),
                ));
            }
        }
        Ok(())
    });
    finish_connection(conn, result).await
}

pub(crate) async fn list_indexes(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> Result<IndexList, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    validate_metadata_identifier(table_name, "tableName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let query = "SELECT TABLE_SCHEMA, TABLE_NAME, NON_UNIQUE, INDEX_SCHEMA, INDEX_NAME, \
                 SEQ_IN_INDEX, COLUMN_NAME, COLLATION, CARDINALITY, SUB_PART, \
                 INDEX_TYPE, COALESCE(INDEX_COMMENT, '') \
                 FROM information_schema.STATISTICS \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                 ORDER BY INDEX_NAME, SEQ_IN_INDEX";
    let result = metadata_query(
        conn.exec::<IndexRow, _, _>(query, (database_name.to_owned(), table_name.to_owned())),
    )
    .await
    .map(|rows| IndexList {
        items: index_metadata(rows, schema_name),
    });
    finish_connection(conn, result).await
}

pub(crate) async fn list_views(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    view_name_pattern: &str,
) -> Result<ViewList, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let query = "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE, COALESCE(TABLE_COMMENT, ''), \
                 COALESCE(ENGINE, ''), COALESCE(TABLE_COLLATION, ''), \
                 CAST(AUTO_INCREMENT AS CHAR), CAST(TABLE_ROWS AS CHAR), \
                 CAST(DATA_LENGTH AS CHAR), \
                 DATE_FORMAT(CREATE_TIME, '%Y-%m-%dT%H:%i:%s'), \
                 DATE_FORMAT(UPDATE_TIME, '%Y-%m-%dT%H:%i:%s') \
                 FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'VIEW' \
                 AND (? = '' OR TABLE_NAME LIKE ?) ORDER BY TABLE_NAME";
    let pattern = view_name_pattern.trim().to_owned();
    let result = metadata_query(
        conn.exec::<TableRow, _, _>(query, (database_name.to_owned(), pattern.clone(), pattern)),
    )
    .await
    .map(|rows| ViewList {
        items: rows
            .into_iter()
            .map(|row| table_metadata(row, schema_name))
            .collect(),
    });
    finish_connection(conn, result).await
}

pub(crate) async fn get_view(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    view_name: &str,
) -> Result<TableMetadata, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    validate_metadata_identifier(view_name, "viewName")?;
    let qualified_name =
        qualified_identifier(database_name, "databaseName", view_name, "viewName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let result =
        metadata_query(conn.query_first::<Row, _>(format!("SHOW CREATE VIEW {qualified_name}")))
            .await
            .and_then(|row| {
                let row =
                    row.ok_or_else(|| metadata_not_found("view", database_name, view_name))?;
                Ok(TableMetadata {
                    database_name: database_name.to_owned(),
                    schema_name: schema_name.to_owned(),
                    name: view_name.to_owned(),
                    table_type: "VIEW".to_owned(),
                    database_type: "MYSQL".to_owned(),
                    ddl: row_string_at(&row, 1)?,
                    ..TableMetadata::default()
                })
            });
    finish_connection(conn, result).await
}

pub(crate) async fn table_ddl(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    _schema_name: &str,
    table_name: &str,
) -> Result<String, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    validate_metadata_identifier(table_name, "tableName")?;
    let qualified_name =
        qualified_identifier(database_name, "databaseName", table_name, "tableName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let result =
        metadata_query(conn.query_first::<Row, _>(format!("SHOW CREATE TABLE {qualified_name}")))
            .await
            .and_then(|row| {
                let row =
                    row.ok_or_else(|| metadata_not_found("table", database_name, table_name))?;
                row_string_at(&row, 1).map(|ddl| format!("{ddl};"))
            });
    finish_connection(conn, result).await
}

pub(crate) async fn list_imported_keys(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    table_name: &str,
) -> Result<ForeignKeyList, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    validate_metadata_identifier(table_name, "tableName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let query = "SELECT kcu.REFERENCED_TABLE_SCHEMA, kcu.REFERENCED_TABLE_NAME, \
                 kcu.REFERENCED_COLUMN_NAME, kcu.TABLE_SCHEMA, kcu.TABLE_NAME, \
                 kcu.COLUMN_NAME, kcu.ORDINAL_POSITION, rc.UPDATE_RULE, rc.DELETE_RULE, \
                 kcu.CONSTRAINT_NAME, rc.UNIQUE_CONSTRAINT_NAME \
                 FROM information_schema.KEY_COLUMN_USAGE kcu \
                 JOIN information_schema.REFERENTIAL_CONSTRAINTS rc \
                   ON rc.CONSTRAINT_SCHEMA = kcu.CONSTRAINT_SCHEMA \
                  AND rc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME \
                  AND rc.TABLE_NAME = kcu.TABLE_NAME \
                 WHERE kcu.TABLE_SCHEMA = ? AND kcu.TABLE_NAME = ? \
                   AND kcu.REFERENCED_TABLE_NAME IS NOT NULL \
                 ORDER BY kcu.CONSTRAINT_NAME, kcu.ORDINAL_POSITION";
    let result = metadata_query(
        conn.exec::<ForeignKeyRow, _, _>(query, (database_name.to_owned(), table_name.to_owned())),
    )
    .await
    .map(|rows| ForeignKeyList {
        items: rows.into_iter().map(foreign_key_metadata).collect(),
    });
    finish_connection(conn, result).await
}

pub(crate) async fn list_exported_keys(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    table_name: &str,
) -> Result<ForeignKeyList, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    validate_metadata_identifier(table_name, "tableName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let query = "SELECT kcu.REFERENCED_TABLE_SCHEMA, kcu.REFERENCED_TABLE_NAME, \
                 kcu.REFERENCED_COLUMN_NAME, kcu.TABLE_SCHEMA, kcu.TABLE_NAME, \
                 kcu.COLUMN_NAME, kcu.ORDINAL_POSITION, rc.UPDATE_RULE, rc.DELETE_RULE, \
                 kcu.CONSTRAINT_NAME, rc.UNIQUE_CONSTRAINT_NAME \
                 FROM information_schema.KEY_COLUMN_USAGE kcu \
                 JOIN information_schema.REFERENTIAL_CONSTRAINTS rc \
                   ON rc.CONSTRAINT_SCHEMA = kcu.CONSTRAINT_SCHEMA \
                  AND rc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME \
                  AND rc.TABLE_NAME = kcu.TABLE_NAME \
                 WHERE kcu.REFERENCED_TABLE_SCHEMA = ? AND kcu.REFERENCED_TABLE_NAME = ? \
                 ORDER BY kcu.TABLE_SCHEMA, kcu.TABLE_NAME, kcu.CONSTRAINT_NAME, \
                          kcu.ORDINAL_POSITION";
    let result = metadata_query(
        conn.exec::<ForeignKeyRow, _, _>(query, (database_name.to_owned(), table_name.to_owned())),
    )
    .await
    .map(|rows| ForeignKeyList {
        items: rows.into_iter().map(foreign_key_metadata).collect(),
    });
    finish_connection(conn, result).await
}

pub(crate) async fn list_primary_keys(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> Result<PrimaryKeyList, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    validate_metadata_identifier(table_name, "tableName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let query = "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, CONSTRAINT_NAME \
                 FROM information_schema.KEY_COLUMN_USAGE \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND CONSTRAINT_NAME = 'PRIMARY' \
                 ORDER BY ORDINAL_POSITION";
    let result = metadata_query(conn.exec::<(String, String, String, String), _, _>(
        query,
        (database_name.to_owned(), table_name.to_owned()),
    ))
    .await
    .map(|rows| PrimaryKeyList {
        items: rows
            .into_iter()
            .map(
                |(database_name, table_name, column_name, name)| PrimaryKeyMetadata {
                    database_name,
                    schema_name: schema_name.to_owned(),
                    table_name,
                    column_name,
                    name,
                },
            )
            .collect(),
    });
    finish_connection(conn, result).await
}

pub(crate) async fn list_functions(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
) -> Result<FunctionList, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let query = "SELECT ROUTINE_SCHEMA, ROUTINE_NAME, SPECIFIC_NAME, \
                 COALESCE(ROUTINE_COMMENT, '') \
                 FROM information_schema.ROUTINES \
                 WHERE ROUTINE_SCHEMA = ? AND ROUTINE_TYPE = 'FUNCTION' \
                 ORDER BY ROUTINE_NAME";
    let result = metadata_query(
        conn.exec::<(String, String, String, String), _, _>(query, (database_name.to_owned(),)),
    )
    .await
    .map(|rows| FunctionList {
        items: rows
            .into_iter()
            .map(
                |(database_name, name, specific_name, remarks)| FunctionMetadata {
                    database_name,
                    schema_name: schema_name.to_owned(),
                    name,
                    remarks,
                    function_type: Some(1),
                    specific_name,
                    ..FunctionMetadata::default()
                },
            )
            .collect(),
    });
    finish_connection(conn, result).await
}

pub(crate) async fn get_function(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    function_name: &str,
) -> Result<FunctionMetadata, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    validate_metadata_identifier(function_name, "functionName")?;
    let qualified_name =
        qualified_identifier(database_name, "databaseName", function_name, "functionName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let result = async {
        let metadata = metadata_query(conn.exec_first::<(String, String, String, String), _, _>(
            "SELECT ROUTINE_SCHEMA, ROUTINE_NAME, SPECIFIC_NAME, \
                 COALESCE(ROUTINE_COMMENT, '') FROM information_schema.ROUTINES \
                 WHERE ROUTINE_SCHEMA = ? AND ROUTINE_NAME = ? \
                   AND ROUTINE_TYPE = 'FUNCTION'",
            (database_name.to_owned(), function_name.to_owned()),
        ))
        .await?
        .ok_or_else(|| metadata_not_found("function", database_name, function_name))?;
        let row = metadata_query(
            conn.query_first::<Row, _>(format!("SHOW CREATE FUNCTION {qualified_name}")),
        )
        .await?
        .ok_or_else(|| metadata_not_found("function", database_name, function_name))?;
        Ok(FunctionMetadata {
            database_name: metadata.0,
            schema_name: schema_name.to_owned(),
            name: metadata.1,
            remarks: metadata.3,
            function_type: Some(1),
            specific_name: metadata.2,
            body: row_string_at(&row, 2)?,
            template: String::new(),
        })
    }
    .await;
    finish_connection(conn, result).await
}

pub(crate) async fn list_function_parameters(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    function_name: &str,
) -> Result<FunctionParameterList, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    validate_metadata_identifier(function_name, "functionName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let query = "SELECT SPECIFIC_SCHEMA, SPECIFIC_NAME, ORDINAL_POSITION, PARAMETER_MODE, \
                 PARAMETER_NAME, DATA_TYPE, DTD_IDENTIFIER, CHARACTER_MAXIMUM_LENGTH, \
                 CHARACTER_OCTET_LENGTH, NUMERIC_PRECISION, NUMERIC_SCALE, \
                 DATETIME_PRECISION FROM information_schema.PARAMETERS \
                 WHERE SPECIFIC_SCHEMA = ? AND SPECIFIC_NAME = ? \
                   AND ROUTINE_TYPE = 'FUNCTION' ORDER BY ORDINAL_POSITION";
    let result = metadata_query(conn.exec::<RoutineParameterRow, _, _>(
        query,
        (database_name.to_owned(), function_name.to_owned()),
    ))
    .await
    .map(|rows| FunctionParameterList {
        items: rows
            .into_iter()
            .map(|row| function_parameter_metadata(row, schema_name))
            .collect(),
    });
    finish_connection(conn, result).await
}

pub(crate) async fn list_procedures(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
) -> Result<ProcedureList, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let query = "SELECT ROUTINE_SCHEMA, ROUTINE_NAME, SPECIFIC_NAME, \
                 COALESCE(ROUTINE_COMMENT, '') \
                 FROM information_schema.ROUTINES \
                 WHERE ROUTINE_SCHEMA = ? AND ROUTINE_TYPE = 'PROCEDURE' \
                 ORDER BY ROUTINE_NAME";
    let result = metadata_query(
        conn.exec::<(String, String, String, String), _, _>(query, (database_name.to_owned(),)),
    )
    .await
    .map(|rows| ProcedureList {
        items: rows
            .into_iter()
            .map(
                |(database_name, name, specific_name, remarks)| ProcedureMetadata {
                    database_name,
                    schema_name: schema_name.to_owned(),
                    name,
                    remarks,
                    procedure_type: Some(2),
                    specific_name,
                    body: String::new(),
                },
            )
            .collect(),
    });
    finish_connection(conn, result).await
}

pub(crate) async fn get_procedure(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    procedure_name: &str,
) -> Result<ProcedureMetadata, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    validate_metadata_identifier(procedure_name, "procedureName")?;
    let qualified_name = qualified_identifier(
        database_name,
        "databaseName",
        procedure_name,
        "procedureName",
    )?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let result = async {
        let metadata = metadata_query(conn.exec_first::<(String, String, String, String), _, _>(
            "SELECT ROUTINE_SCHEMA, ROUTINE_NAME, SPECIFIC_NAME, \
                 COALESCE(ROUTINE_COMMENT, '') FROM information_schema.ROUTINES \
                 WHERE ROUTINE_SCHEMA = ? AND ROUTINE_NAME = ? \
                   AND ROUTINE_TYPE = 'PROCEDURE'",
            (database_name.to_owned(), procedure_name.to_owned()),
        ))
        .await?
        .ok_or_else(|| metadata_not_found("procedure", database_name, procedure_name))?;
        let row = metadata_query(
            conn.query_first::<Row, _>(format!("SHOW CREATE PROCEDURE {qualified_name}")),
        )
        .await?
        .ok_or_else(|| metadata_not_found("procedure", database_name, procedure_name))?;
        Ok(ProcedureMetadata {
            database_name: metadata.0,
            schema_name: schema_name.to_owned(),
            name: metadata.1,
            remarks: metadata.3,
            procedure_type: Some(2),
            specific_name: metadata.2,
            body: row_string_at(&row, 2)?,
        })
    }
    .await;
    finish_connection(conn, result).await
}

pub(crate) async fn list_procedure_parameters(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    procedure_name: &str,
) -> Result<ProcedureParameterList, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    validate_metadata_identifier(procedure_name, "procedureName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let query = "SELECT SPECIFIC_SCHEMA, SPECIFIC_NAME, ORDINAL_POSITION, PARAMETER_MODE, \
                 PARAMETER_NAME, DATA_TYPE, DTD_IDENTIFIER, CHARACTER_MAXIMUM_LENGTH, \
                 CHARACTER_OCTET_LENGTH, NUMERIC_PRECISION, NUMERIC_SCALE, \
                 DATETIME_PRECISION FROM information_schema.PARAMETERS \
                 WHERE SPECIFIC_SCHEMA = ? AND SPECIFIC_NAME = ? \
                   AND ROUTINE_TYPE = 'PROCEDURE' ORDER BY ORDINAL_POSITION";
    let result = metadata_query(conn.exec::<RoutineParameterRow, _, _>(
        query,
        (database_name.to_owned(), procedure_name.to_owned()),
    ))
    .await
    .map(|rows| ProcedureParameterList {
        items: rows
            .into_iter()
            .map(|row| procedure_parameter_metadata(row, schema_name))
            .collect(),
    });
    finish_connection(conn, result).await
}

pub(crate) async fn preview_routine_invocation(
    application: &Application,
    request: RoutineInvocationRequest,
) -> Result<RoutineInvocationPreview, AppError> {
    let routine_type = normalize_mysql_routine_type(&request.routine_type)?;
    let routine_name = request.routine_name.trim().to_owned();
    let routine_lookup_name = mysql_routine_lookup_name(&routine_name);
    let database_name = request.scope.database_name.trim().to_owned();
    validate_metadata_identifier(&routine_name, "routineName")?;
    validate_metadata_identifier(&database_name, "databaseName")?;
    let resolved = resolve_native_connection(application, &request.scope.datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let query = "SELECT ORDINAL_POSITION, PARAMETER_MODE, PARAMETER_NAME, DATA_TYPE \
                 FROM information_schema.PARAMETERS \
                 WHERE SPECIFIC_SCHEMA = ? AND SPECIFIC_NAME = ? AND ROUTINE_TYPE = ? \
                 ORDER BY ORDINAL_POSITION";
    let result = metadata_query(conn.exec::<RoutineInvocationParameterRow, _, _>(
        query,
        (database_name, routine_lookup_name, routine_type.as_str()),
    ))
    .await
    .map(|rows| {
        let mut parameters = rows
            .into_iter()
            .filter_map(|row| routine_invocation_parameter(routine_type, row))
            .collect::<Vec<_>>();
        parameters.sort_by_key(|parameter| parameter.ordinal_position);
        RoutineInvocationPreview {
            sql: render_routine_invocation_preview(routine_type, &routine_name, &parameters),
        }
    });
    finish_connection(conn, result).await
}

pub(crate) fn preview_routine_migration(
    request: &RoutineMigrationRequest,
) -> Result<RoutineInvocationPreview, AppError> {
    let plan = routine_migration_plan(request)?;
    Ok(RoutineInvocationPreview {
        sql: plan.preview_sql,
    })
}

pub(crate) async fn execute_routine_migration(
    application: &Application,
    request: RoutineMigrationRequest,
) -> Result<RoutineMigrationExecution, AppError> {
    let plan = routine_migration_plan(&request)?;
    let resolved = resolve_native_connection(application, &request.scope.datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let result = execute_routine_migration_with_connection(&mut conn, &plan).await;
    finish_connection(conn, Ok(result)).await
}

async fn execute_routine_migration_with_connection(
    conn: &mut Conn,
    plan: &RoutineMigrationPlan,
) -> RoutineMigrationExecution {
    let selected_database = quote_identifier(&plan.database_name, "databaseName")
        .expect("validated migration database name must remain valid");
    if let Err(error) = metadata_query(conn.query_drop(format!("USE {selected_database}"))).await {
        return routine_migration_failure(
            plan,
            migration_error(&error),
            "BEFORE_IMAGE",
            false,
            false,
        );
    }

    let previous = match capture_previous_routine(conn, plan).await {
        Ok(previous) => previous,
        Err(error) => {
            return routine_migration_failure(
                plan,
                format!(
                    "Routine migration was rejected because the existing routine definition could not be captured before DROP: {}",
                    migration_error(&error)
                ),
                "BEFORE_IMAGE",
                false,
                false,
            );
        }
    };

    if let Err(error) = metadata_query(conn.query_drop(&plan.drop_sql)).await {
        return routine_migration_failure(
            plan,
            format!(
                "Routine migration failed before the previous routine was dropped. Original error: {}",
                migration_error(&error)
            ),
            "DROP",
            false,
            false,
        );
    }

    match metadata_query(conn.query_drop(&plan.create_sql)).await {
        Ok(()) => RoutineMigrationExecution {
            success: true,
            message: "Statement executed successfully".to_owned(),
            sql: plan.preview_sql.clone(),
            failure_stage: None,
            restore_attempted: false,
            restore_succeeded: false,
        },
        Err(create_error) => {
            let Some(previous) = previous else {
                return routine_migration_failure(
                    plan,
                    format!(
                        "Routine migration failed. No previous routine definition existed. Original error: {}",
                        migration_error(&create_error)
                    ),
                    "APPLY",
                    false,
                    false,
                );
            };
            let restore = async {
                metadata_query(conn.query_drop(&plan.drop_sql)).await?;
                metadata_query(conn.query_drop(previous)).await
            }
            .await;
            match restore {
                Ok(()) => routine_migration_failure(
                    plan,
                    format!(
                        "Routine migration failed. The previous routine definition was restored. Original error: {}",
                        migration_error(&create_error)
                    ),
                    "APPLY",
                    true,
                    true,
                ),
                Err(restore_error) => routine_migration_failure(
                    plan,
                    format!(
                        "Routine migration failed after the previous routine was dropped, and automatic restore failed. Original error: {}; restore error: {}",
                        migration_error(&create_error),
                        migration_error(&restore_error)
                    ),
                    "APPLY",
                    true,
                    false,
                ),
            }
        }
    }
}

async fn capture_previous_routine(
    conn: &mut Conn,
    plan: &RoutineMigrationPlan,
) -> Result<Option<String>, AppError> {
    let exists = metadata_query(conn.exec_first::<u8, _, _>(
        "SELECT 1 FROM information_schema.ROUTINES \
         WHERE ROUTINE_SCHEMA = ? AND ROUTINE_NAME = ? AND ROUTINE_TYPE = ? LIMIT 1",
        (
            plan.database_name.clone(),
            plan.routine_name.clone(),
            plan.routine_type.as_str(),
        ),
    ))
    .await?
    .is_some();
    if !exists {
        return Ok(None);
    }
    let qualified_name = qualified_identifier(
        &plan.database_name,
        "databaseName",
        &plan.routine_name,
        "routineName",
    )?;
    let row = metadata_query(conn.query_first::<Row, _>(format!(
        "SHOW CREATE {} {qualified_name}",
        plan.routine_type.as_str()
    )))
    .await?
    .ok_or_else(AppError::internal)?;
    let mut ddl = row_string_at(&row, 2)?;
    ensure_sql_terminated(&mut ddl);
    Ok(Some(ddl))
}

fn routine_migration_plan(
    request: &RoutineMigrationRequest,
) -> Result<RoutineMigrationPlan, AppError> {
    let routine_type = normalize_mysql_routine_type(&request.routine_type)?;
    let database_name = request.scope.database_name.trim().to_owned();
    let routine_name = mysql_routine_lookup_name(request.routine_name.trim());
    validate_metadata_identifier(&database_name, "databaseName")?;
    validate_metadata_identifier(&routine_name, "routineName")?;
    let ddl = request.ddl.trim();
    if ddl.is_empty() || ddl.len() > MAX_SQL_BYTES || ddl.contains('\0') {
        return Err(AppError::invalid(
            "invalid_routine_migration_request",
            "ddl is invalid",
        ));
    }
    let qualified_name =
        qualified_identifier(&database_name, "databaseName", &routine_name, "routineName")?;
    let drop_sql = format!("DROP {} IF EXISTS {qualified_name}", routine_type.as_str());
    let mut create_sql = ddl.to_owned();
    ensure_sql_terminated(&mut create_sql);
    let preview_sql = format!("{drop_sql};\n\n{create_sql}");
    Ok(RoutineMigrationPlan {
        routine_type,
        database_name,
        routine_name,
        drop_sql,
        create_sql,
        preview_sql,
    })
}

fn ensure_sql_terminated(sql: &mut String) {
    if !sql.trim_end().ends_with(';') {
        sql.push(';');
    }
}

fn routine_migration_failure(
    plan: &RoutineMigrationPlan,
    message: String,
    failure_stage: &str,
    restore_attempted: bool,
    restore_succeeded: bool,
) -> RoutineMigrationExecution {
    RoutineMigrationExecution {
        success: false,
        message,
        sql: plan.preview_sql.clone(),
        failure_stage: Some(failure_stage.to_owned()),
        restore_attempted,
        restore_succeeded,
    }
}

fn migration_error(error: &AppError) -> String {
    error.api_error().message
}

pub(crate) async fn list_triggers(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
) -> Result<TriggerList, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let query = "SELECT TRIGGER_SCHEMA, TRIGGER_NAME, EVENT_MANIPULATION \
                 FROM information_schema.TRIGGERS \
                 WHERE TRIGGER_SCHEMA = ? ORDER BY TRIGGER_NAME";
    let result = metadata_query(
        conn.exec::<(String, String, String), _, _>(query, (database_name.to_owned(),)),
    )
    .await
    .map(|rows| TriggerList {
        items: rows
            .into_iter()
            .map(
                |(database_name, name, event_manipulation)| TriggerMetadata {
                    database_name,
                    schema_name: schema_name.to_owned(),
                    name,
                    event_manipulation,
                    body: String::new(),
                },
            )
            .collect(),
    });
    finish_connection(conn, result).await
}

pub(crate) async fn get_trigger(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    trigger_name: &str,
) -> Result<TriggerMetadata, AppError> {
    validate_metadata_identifier(database_name, "databaseName")?;
    validate_metadata_identifier(trigger_name, "triggerName")?;
    let qualified_name =
        qualified_identifier(database_name, "databaseName", trigger_name, "triggerName")?;
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved).await?;
    let result = async {
        let metadata = metadata_query(conn.exec_first::<(String, String, String), _, _>(
            "SELECT TRIGGER_SCHEMA, TRIGGER_NAME, EVENT_MANIPULATION \
                 FROM information_schema.TRIGGERS \
                 WHERE TRIGGER_SCHEMA = ? AND TRIGGER_NAME = ?",
            (database_name.to_owned(), trigger_name.to_owned()),
        ))
        .await?
        .ok_or_else(|| metadata_not_found("trigger", database_name, trigger_name))?;
        let row = metadata_query(
            conn.query_first::<Row, _>(format!("SHOW CREATE TRIGGER {qualified_name}")),
        )
        .await?
        .ok_or_else(|| metadata_not_found("trigger", database_name, trigger_name))?;
        Ok(TriggerMetadata {
            database_name: metadata.0,
            schema_name: schema_name.to_owned(),
            name: metadata.1,
            event_manipulation: metadata.2,
            body: row_string_at(&row, 2)?,
        })
    }
    .await;
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
    let _ = mysql_query_parameters(&query.parameters)?;
    validate_read_sql(&query.sql)?;
    validate_query_options(query.options)
}

fn mysql_query_parameters(parameters: &[QueryParameter]) -> Result<Params, AppError> {
    if parameters.is_empty() {
        return Ok(Params::Empty);
    }
    if parameters.len() > MAX_PARAMETERS {
        return Err(AppError::invalid(
            "invalid_query_parameter_count",
            format!("MySQL queries accept at most {MAX_PARAMETERS} parameters"),
        ));
    }

    let mut ordered = parameters.iter().collect::<Vec<_>>();
    ordered.sort_unstable_by_key(|parameter| parameter.position);
    let mut values = Vec::with_capacity(ordered.len());
    for (index, parameter) in ordered.into_iter().enumerate() {
        let expected = u32::try_from(index + 1).map_err(|_| AppError::internal())?;
        if parameter.position != expected {
            return Err(AppError::invalid(
                "invalid_query_parameter",
                "MySQL parameter positions must be unique and contiguous from 1",
            ));
        }
        values.push(mysql_query_value(&parameter.value)?);
    }
    Ok(Params::Positional(values))
}

fn mysql_query_value(value: &DatabaseValue) -> Result<Value, AppError> {
    match value {
        DatabaseValue::Null => Ok(Value::NULL),
        DatabaseValue::Boolean(value) => Ok(Value::Int(i64::from(*value))),
        DatabaseValue::SignedInteger(value) => Ok(Value::Int(*value)),
        DatabaseValue::UnsignedInteger(value) => Ok(Value::UInt(*value)),
        DatabaseValue::Float32(value) => Ok(Value::Float(*value)),
        DatabaseValue::Float64(value) => Ok(Value::Double(*value)),
        DatabaseValue::Decimal(value) => {
            validate_mysql_decimal(value)?;
            mysql_query_bytes(value.as_bytes(), "decimal")
        }
        DatabaseValue::Text(value) => mysql_query_bytes(value.as_bytes(), "text"),
        DatabaseValue::Binary(value) => mysql_query_bytes(value, "binary"),
        DatabaseValue::Date(value) => mysql_date_parameter(value),
        DatabaseValue::Time(value) => mysql_time_parameter(value),
        DatabaseValue::Timestamp(value) => mysql_timestamp_parameter(value),
        DatabaseValue::TimestampWithTimeZone(value) => {
            mysql_timestamp_with_time_zone_parameter(value)
        }
        DatabaseValue::Json(value) => mysql_query_bytes(value.as_bytes(), "JSON"),
        DatabaseValue::Uuid(value) => mysql_query_bytes(value.as_bytes(), "UUID"),
    }
}

fn mysql_query_bytes(value: &[u8], label: &str) -> Result<Value, AppError> {
    if value.len() > MAX_SCALAR_BYTES {
        return Err(AppError::invalid(
            "invalid_query_parameter",
            format!("The MySQL {label} parameter exceeds {MAX_SCALAR_BYTES} bytes"),
        ));
    }
    Ok(Value::Bytes(value.to_vec()))
}

fn validate_mysql_decimal(value: &str) -> Result<(), AppError> {
    let unsigned = value.strip_prefix(['+', '-']).unwrap_or(value);
    let mut digits = 0_usize;
    let mut decimal_points = 0_u8;
    for byte in unsigned.bytes() {
        if byte.is_ascii_digit() {
            digits += 1;
        } else if byte == b'.' {
            decimal_points += 1;
        } else {
            return Err(mysql_temporal_parameter_error("decimal"));
        }
    }
    if digits == 0 || decimal_points > 1 {
        return Err(mysql_temporal_parameter_error("decimal"));
    }
    Ok(())
}

fn mysql_date_parameter(value: &str) -> Result<Value, AppError> {
    mysql_query_bytes(value.as_bytes(), "date")?;
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| mysql_temporal_parameter_error("date"))?;
    let datetime = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| mysql_temporal_parameter_error("date"))?;
    mysql_datetime_value(datetime, "date")
}

fn mysql_time_parameter(value: &str) -> Result<Value, AppError> {
    mysql_query_bytes(value.as_bytes(), "time")?;
    let (negative, unsigned) = value
        .strip_prefix('-')
        .map_or((false, value), |value| (true, value));
    let unsigned = unsigned.strip_prefix('+').unwrap_or(unsigned);
    let mut parts = unsigned.split(':');
    let hours = parse_mysql_time_part::<u32>(parts.next(), "time")?;
    let minutes = parse_mysql_time_part::<u8>(parts.next(), "time")?;
    let seconds = parts
        .next()
        .ok_or_else(|| mysql_temporal_parameter_error("time"))?;
    if parts.next().is_some() || hours > 838 || minutes > 59 {
        return Err(mysql_temporal_parameter_error("time"));
    }
    let (seconds, micros) = parse_mysql_seconds(seconds)?;
    Ok(Value::Time(
        negative,
        hours / 24,
        u8::try_from(hours % 24).map_err(|_| AppError::internal())?,
        minutes,
        seconds,
        micros,
    ))
}

fn parse_mysql_time_part<T>(value: Option<&str>, label: &str) -> Result<T, AppError>
where
    T: std::str::FromStr,
{
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| mysql_temporal_parameter_error(label))?
        .parse::<T>()
        .map_err(|_| mysql_temporal_parameter_error(label))
}

fn parse_mysql_seconds(value: &str) -> Result<(u8, u32), AppError> {
    let (seconds, fraction) = value
        .split_once('.')
        .map_or((value, None), |(seconds, fraction)| {
            (seconds, Some(fraction))
        });
    let seconds = parse_mysql_time_part::<u8>(Some(seconds), "time")?;
    if seconds > 59 {
        return Err(mysql_temporal_parameter_error("time"));
    }
    let Some(fraction) = fraction else {
        return Ok((seconds, 0));
    };
    if fraction.is_empty()
        || fraction.len() > 6
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(mysql_temporal_parameter_error("time"));
    }
    let parsed = fraction
        .parse::<u32>()
        .map_err(|_| mysql_temporal_parameter_error("time"))?;
    let padding = u32::try_from(6 - fraction.len()).map_err(|_| AppError::internal())?;
    Ok((seconds, parsed * 10_u32.pow(padding)))
}

fn mysql_timestamp_parameter(value: &str) -> Result<Value, AppError> {
    mysql_query_bytes(value.as_bytes(), "timestamp")?;
    let datetime = ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"]
        .into_iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
        .ok_or_else(|| mysql_temporal_parameter_error("timestamp"))?;
    mysql_datetime_value(datetime, "timestamp")
}

fn mysql_timestamp_with_time_zone_parameter(value: &str) -> Result<Value, AppError> {
    mysql_query_bytes(value.as_bytes(), "timestamp with time zone")?;
    let datetime = DateTime::parse_from_rfc3339(value)
        .map_err(|_| mysql_temporal_parameter_error("timestamp with time zone"))?
        .with_timezone(&Utc)
        .naive_utc();
    mysql_datetime_value(datetime, "timestamp with time zone")
}

fn mysql_datetime_value(datetime: NaiveDateTime, label: &str) -> Result<Value, AppError> {
    let year = u16::try_from(datetime.year())
        .ok()
        .filter(|year| *year > 0 && *year <= 9_999)
        .ok_or_else(|| mysql_temporal_parameter_error(label))?;
    Ok(Value::Date(
        year,
        u8::try_from(datetime.month()).map_err(|_| AppError::internal())?,
        u8::try_from(datetime.day()).map_err(|_| AppError::internal())?,
        u8::try_from(datetime.hour()).map_err(|_| AppError::internal())?,
        u8::try_from(datetime.minute()).map_err(|_| AppError::internal())?,
        u8::try_from(datetime.second()).map_err(|_| AppError::internal())?,
        datetime.nanosecond() / 1_000,
    ))
}

fn mysql_temporal_parameter_error(label: &str) -> AppError {
    AppError::invalid(
        "invalid_query_parameter",
        format!("The MySQL {label} parameter is invalid"),
    )
}

fn validate_read_sql(sql: &str) -> Result<(), AppError> {
    let tokens = read_policy_tokens(
        sql,
        "mysql_native_query_unsupported",
        "Native MySQL read queries do not accept executable comments",
    )?;
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

struct SqlLexemes {
    tokens: Vec<SqlToken>,
    executable_comment: bool,
}

fn sql_tokens(sql: &str) -> Result<Vec<SqlToken>, AppError> {
    Ok(sql_lexemes(sql)?.tokens)
}

fn read_policy_tokens(
    sql: &str,
    error_code: &'static str,
    error_message: &'static str,
) -> Result<Vec<SqlToken>, AppError> {
    let lexemes = sql_lexemes(sql)?;
    if lexemes.executable_comment {
        return Err(AppError::invalid(error_code, error_message));
    }
    Ok(lexemes.tokens)
}

fn sql_lexemes(sql: &str) -> Result<SqlLexemes, AppError> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut executable_comment = false;
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
                executable_comment |= bytes.get(index + 2) == Some(&b'!')
                    || (bytes
                        .get(index + 2)
                        .is_some_and(|byte| matches!(byte, b'm' | b'M'))
                        && bytes.get(index + 3) == Some(&b'!'));
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
    Ok(SqlLexemes {
        tokens,
        executable_comment,
    })
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
    request: TablePreviewRequest,
    row_limit: u32,
) -> Result<TablePreviewAccepted, AppError> {
    let database_name = quote_identifier(&request.table.scope.database_name, "databaseName")?;
    let table_name = quote_identifier(&request.table.table_name, "tableName")?;
    let sql = format!("SELECT * FROM {database_name}.{table_name} LIMIT {row_limit}");
    let accepted = application
        .start_read_query(StartQueryRequest {
            datasource_id: request.table.scope.datasource_id,
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
    Ok(TablePreviewAccepted {
        operation_id: accepted.operation_id,
        sql,
        row_limit,
    })
}

struct ConsoleStatementExecution {
    results: Vec<NativeConsoleResult>,
    failure: Option<AppError>,
}

enum ConsoleExecutionError {
    Cancelled(Option<String>),
    Fatal(AppError),
}

pub(crate) async fn execute_console(
    application: &Application,
    request: NativeConsoleRequest,
    mut cancellation: watch::Receiver<CancellationRequest>,
    force_read_only: bool,
) -> Result<Vec<NativeConsoleResult>, AppError> {
    let (statements, page_offset, page_end) = prepare_console_statements(&request)?;

    let initial_cancellation = { cancellation.borrow().clone() };
    if let CancellationRequest::Requested { reason } = initial_cancellation {
        return Err(mysql_console_cancelled(reason));
    }
    if force_read_only {
        validate_forced_read_console(&statements)?;
    }
    let resolved = resolve_native_connection(application, &request.datasource_id).await?;
    if resolved.connection.read_only && !force_read_only {
        validate_read_only_console(&statements)?;
    }
    let prepared = prepare_resolved_connection(&resolved).await?;
    let options = prepared.options.clone();
    let mut conn = match open_query_connection(prepared, &mut cancellation).await {
        Ok(conn) => conn,
        Err(QueryTaskError::Cancelled(reason)) => return Err(mysql_console_cancelled(reason)),
        Err(QueryTaskError::Failed(error)) => return Err(error),
    };
    let connection_id = conn.id();

    if !request.database_name.trim().is_empty() {
        let database_name = quote_console_identifier(&request.database_name, "databaseName")?;
        if let Err(error) = execute_console_control(
            &mut conn,
            options.clone(),
            connection_id,
            &format!("USE {database_name}"),
            &mut cancellation,
        )
        .await
        {
            return finish_console_error(conn, options, connection_id, error).await;
        }
    }
    if force_read_only && let Err(error) = start_read_only_transaction(&mut conn).await {
        disconnect_quietly(conn).await;
        return Err(error);
    }

    let mut results = Vec::new();
    let mut retained_result_bytes = 0_u64;
    for (index, statement) in statements.into_iter().enumerate() {
        let statement_sequence = u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1))
            .ok_or_else(AppError::internal)?;
        let started = Instant::now();
        let execution = execute_console_statement(
            &mut conn,
            options.clone(),
            connection_id,
            &statement,
            statement_sequence,
            page_offset,
            page_end,
            request.result_set_id,
            &mut retained_result_bytes,
            &mut cancellation,
        )
        .await;
        match execution {
            Ok(mut execution) => {
                results.append(&mut execution.results);
                if let Some(error) = execution.failure {
                    results.push(console_failure_result(
                        statement_sequence,
                        statement,
                        &error,
                        elapsed_millis(started),
                    ));
                    if !request.error_continue {
                        break;
                    }
                }
            }
            Err(error) => {
                return finish_console_error(conn, options, connection_id, error).await;
            }
        }
    }

    if force_read_only {
        finish_read_only_connection_quietly(conn).await;
    } else {
        disconnect_connection(conn).await?;
    }
    Ok(results)
}

fn prepare_console_statements(
    request: &NativeConsoleRequest,
) -> Result<(Vec<String>, u64, u64), AppError> {
    let (page_offset, page_end) = validate_console_request(request)?;
    let mut statements = if request.single {
        vec![request.sql.trim().to_owned()]
    } else {
        split_mysql_script(&request.sql)?
    };
    if statements.is_empty() {
        return Err(AppError::invalid(
            "invalid_mysql_console_request",
            "sql must contain at least one MySQL statement",
        ));
    }
    if request.explain {
        for statement in &mut statements {
            *statement = format!("EXPLAIN {statement}");
        }
    }
    Ok((statements, page_offset, page_end))
}

pub(crate) async fn execute_update(
    resolved: ResolvedDatasourceConnection,
    sql: String,
    cancellation: CancellationToken,
) -> Result<u64, DatabaseWriteError> {
    if cancellation.is_cancelled() {
        return Err(DatabaseWriteError::not_started(AppError::new(
            AppErrorKind::Conflict,
            ApiError::new(
                "database_write_cancelled",
                "The database write was cancelled before dispatch",
            ),
        )));
    }
    let sql = validate_single_write_sql(&sql).map_err(DatabaseWriteError::not_started)?;
    if resolved.connection.read_only {
        return Err(DatabaseWriteError::not_started(AppError::new(
            AppErrorKind::Conflict,
            ApiError::new(
                "datasource_read_only",
                "The datasource connection is configured as read-only",
            ),
        )));
    }

    let prepared = prepare_resolved_connection(&resolved)
        .await
        .map_err(DatabaseWriteError::not_started)?;
    let options = prepared.options.clone();
    let open = open_prepared_connection(prepared);
    tokio::pin!(open);
    let mut conn = tokio::select! {
        biased;
        () = cancellation.cancelled() => {
            return Err(DatabaseWriteError::not_started(AppError::new(
                AppErrorKind::Conflict,
                ApiError::new(
                    "database_write_cancelled",
                    "The database write was cancelled before dispatch",
                ),
            )));
        }
        result = &mut open => result.map_err(DatabaseWriteError::not_started)?,
    };
    if cancellation.is_cancelled() {
        disconnect_quietly(conn).await;
        return Err(DatabaseWriteError::not_started(AppError::new(
            AppErrorKind::Conflict,
            ApiError::new(
                "database_write_cancelled",
                "The database write was cancelled before dispatch",
            ),
        )));
    }

    let connection_id = conn.id();
    let result = {
        // Prepared statements are a second boundary against multi-statement
        // execution even if a future parser regression accepts a script.
        let query = conn.exec_drop(sql, ());
        tokio::pin!(query);
        tokio::select! {
            biased;
            () = cancellation.cancelled() => None,
            result = &mut query => Some(result),
        }
    };
    let Some(result) = result else {
        terminate_connection_quietly(options, connection_id).await;
        drop(conn);
        return Err(DatabaseWriteError::unknown(AppError::new(
            AppErrorKind::Unavailable,
            ApiError::new(
                "database_write_outcome_unknown",
                "The database write was interrupted after dispatch; do not retry it blindly",
            ),
        )));
    };

    match result {
        Ok(()) => {
            let affected_rows = conn.affected_rows();
            disconnect_quietly(conn).await;
            Ok(affected_rows)
        }
        Err(error @ MysqlError::Server(_)) => {
            disconnect_quietly(conn).await;
            tracing::warn!(
                error = %error,
                "MySQL rejected a dispatched write whose partial effects cannot be excluded"
            );
            Err(DatabaseWriteError::unknown(AppError::new(
                AppErrorKind::Unavailable,
                ApiError::new(
                    "database_write_outcome_unknown",
                    "MySQL reported an error after write dispatch; partial effects cannot be excluded, so do not retry it blindly",
                ),
            )))
        }
        Err(error) => {
            terminate_connection_quietly(options, connection_id).await;
            drop(conn);
            Err(DatabaseWriteError::unknown(mysql_query_error(error)))
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn execute_console_statement(
    conn: &mut Conn,
    options: Opts,
    connection_id: u32,
    statement: &str,
    statement_sequence: u32,
    page_offset: u64,
    page_end: u64,
    selected_result_set_id: Option<u32>,
    retained_result_bytes: &mut u64,
    cancellation: &mut watch::Receiver<CancellationRequest>,
) -> Result<ConsoleStatementExecution, ConsoleExecutionError> {
    let statement_started = Instant::now();
    let query = conn.query_iter(statement);
    tokio::pin!(query);
    let mut cancellation_open = true;
    let mut query_result = loop {
        tokio::select! {
            biased;
            changed = cancellation.changed(), if cancellation_open => {
                if changed.is_err() {
                    cancellation_open = false;
                    continue;
                }
                let request = { cancellation.borrow().clone() };
                if let CancellationRequest::Requested { reason } = request {
                    cancel_console_connection(options, connection_id).await
                        .map_err(ConsoleExecutionError::Fatal)?;
                    return Err(ConsoleExecutionError::Cancelled(reason));
                }
            }
            result = &mut query => {
                match result {
                    Ok(result) => break result,
                    Err(error) if matches!(&error, MysqlError::Server(_)) => {
                        return Ok(ConsoleStatementExecution {
                            results: Vec::new(),
                            failure: Some(mysql_query_error(error)),
                        });
                    }
                    Err(error) => {
                        return Err(ConsoleExecutionError::Fatal(mysql_query_error(error)));
                    }
                }
            }
        }
    };

    let mut results = Vec::new();
    let mut result_set_id = 0_u32;
    while let Some(columns) = query_result.columns() {
        let result_started = Instant::now();
        let columns = columns.to_vec();
        let tabular = !columns.is_empty();
        let current_result_set_id = if tabular {
            result_set_id = result_set_id
                .checked_add(1)
                .ok_or_else(|| ConsoleExecutionError::Fatal(AppError::internal()))?;
            Some(result_set_id)
        } else {
            None
        };
        let retain = current_result_set_id
            .is_none_or(|id| selected_result_set_id.is_none_or(|selected| selected == id));
        let update_count = query_result.affected_rows();
        let info = query_result.info().into_owned();
        let converted_columns = if tabular && retain {
            if columns.len() > MAX_COLUMNS {
                return Err(ConsoleExecutionError::Fatal(resource_error(
                    "mysql_result_too_wide",
                    format!("MySQL returned more than {MAX_COLUMNS} columns"),
                )));
            }
            columns
                .iter()
                .enumerate()
                .map(|(index, column)| console_column(index, column))
                .collect::<Result<Vec<_>, _>>()
                .map_err(ConsoleExecutionError::Fatal)?
        } else {
            Vec::new()
        };
        let mut rows = Vec::new();
        let mut row_count = 0_u64;
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
                        cancel_console_connection(options.clone(), connection_id).await
                            .map_err(ConsoleExecutionError::Fatal)?;
                        return Err(ConsoleExecutionError::Cancelled(reason));
                    }
                    continue;
                }
                row = query_result.next() => row,
            };
            let row = match next {
                Ok(Some(row)) => row,
                Ok(None) => break,
                Err(error) if matches!(&error, MysqlError::Server(_)) => {
                    return Ok(ConsoleStatementExecution {
                        results,
                        failure: Some(mysql_query_error(error)),
                    });
                }
                Err(error) => {
                    return Err(ConsoleExecutionError::Fatal(mysql_query_error(error)));
                }
            };
            if retain && (page_offset..page_end).contains(&row_count) {
                let row = console_row(row, &columns).map_err(ConsoleExecutionError::Fatal)?;
                reserve_console_result_bytes(retained_result_bytes, &row)
                    .map_err(ConsoleExecutionError::Fatal)?;
                rows.push(row);
            }
            row_count = row_count
                .checked_add(1)
                .ok_or_else(|| ConsoleExecutionError::Fatal(AppError::internal()))?;
        }

        if retain {
            results.push(NativeConsoleResult {
                statement_sequence,
                result_set_id: current_result_set_id,
                sql: statement.to_owned(),
                success: true,
                message: if info.is_empty() {
                    "Statement executed successfully".to_owned()
                } else {
                    info
                },
                update_count: if tabular { 0 } else { update_count },
                columns: converted_columns,
                rows,
                row_count,
                has_more: row_count > page_end,
                duration_ms: elapsed_millis(result_started),
                error: None,
            });
        }
    }

    if results.is_empty() && selected_result_set_id.is_none() {
        results.push(NativeConsoleResult {
            statement_sequence,
            result_set_id: None,
            sql: statement.to_owned(),
            success: true,
            message: "Statement executed successfully".to_owned(),
            update_count: query_result.affected_rows(),
            columns: Vec::new(),
            rows: Vec::new(),
            row_count: 0,
            has_more: false,
            duration_ms: elapsed_millis(statement_started),
            error: None,
        });
    }
    Ok(ConsoleStatementExecution {
        results,
        failure: None,
    })
}

async fn execute_console_control(
    conn: &mut Conn,
    options: Opts,
    connection_id: u32,
    sql: &str,
    cancellation: &mut watch::Receiver<CancellationRequest>,
) -> Result<(), ConsoleExecutionError> {
    let query = conn.query_drop(sql);
    tokio::pin!(query);
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
                    cancel_console_connection(options, connection_id).await
                        .map_err(ConsoleExecutionError::Fatal)?;
                    return Err(ConsoleExecutionError::Cancelled(reason));
                }
            }
            result = &mut query => {
                return result
                    .map_err(mysql_query_error)
                    .map_err(ConsoleExecutionError::Fatal);
            }
        }
    }
}

async fn finish_console_error<T>(
    conn: ManagedMysqlConnection,
    options: Opts,
    connection_id: u32,
    error: ConsoleExecutionError,
) -> Result<T, AppError> {
    match error {
        ConsoleExecutionError::Cancelled(reason) => {
            drop(conn);
            Err(mysql_console_cancelled(reason))
        }
        ConsoleExecutionError::Fatal(error) => {
            terminate_connection_quietly(options, connection_id).await;
            drop(conn);
            Err(error)
        }
    }
}

fn console_failure_result(
    statement_sequence: u32,
    sql: String,
    error: &AppError,
    duration_ms: u64,
) -> NativeConsoleResult {
    let api_error = error.api_error();
    NativeConsoleResult {
        statement_sequence,
        result_set_id: None,
        sql,
        success: false,
        message: api_error.message.clone(),
        update_count: 0,
        columns: Vec::new(),
        rows: Vec::new(),
        row_count: 0,
        has_more: false,
        duration_ms,
        error: Some(api_error),
    }
}

fn validate_console_request(request: &NativeConsoleRequest) -> Result<(u64, u64), AppError> {
    if request.datasource_id.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_mysql_console_request",
            "dataSourceId cannot be empty",
        ));
    }
    if request.sql.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_mysql_console_request",
            "sql cannot be empty",
        ));
    }
    if request.sql.len() > MAX_SQL_BYTES {
        return Err(resource_error(
            "mysql_console_script_too_large",
            format!("MySQL Console scripts are limited to {MAX_SQL_BYTES} bytes"),
        ));
    }
    if !request.database_name.trim().is_empty() {
        quote_console_identifier(&request.database_name, "databaseName")?;
    }
    if request.page_no == 0 || request.page_size == 0 || request.page_size > MAX_CONSOLE_PAGE_SIZE {
        return Err(AppError::invalid(
            "invalid_mysql_console_request",
            format!(
                "pageNo must be positive and pageSize must be between 1 and {MAX_CONSOLE_PAGE_SIZE}"
            ),
        ));
    }
    if request.result_set_id == Some(0) {
        return Err(AppError::invalid(
            "invalid_mysql_console_request",
            "resultSetId must be a positive one-based integer",
        ));
    }
    let (page_offset, page_end) = if request.page_size_all {
        (0, u64::from(MAX_CONSOLE_PAGE_SIZE))
    } else {
        let page_offset = u64::from(request.page_no - 1) * u64::from(request.page_size);
        let page_end = page_offset
            .checked_add(u64::from(request.page_size))
            .ok_or_else(AppError::internal)?;
        (page_offset, page_end)
    };
    Ok((page_offset, page_end))
}

fn validate_read_only_console(statements: &[String]) -> Result<(), AppError> {
    for statement in statements {
        let tokens = read_policy_tokens(
            statement,
            "datasource_read_only",
            "Read-only datasource connections do not accept executable comments",
        )?;
        let words = tokens
            .iter()
            .filter_map(|token| match token {
                SqlToken::Word(word) => Some(word.as_str()),
                SqlToken::Semicolon => None,
            })
            .collect::<Vec<_>>();
        let allowed = match words.as_slice() {
            ["SELECT", ..] => validate_read_sql(statement).is_ok(),
            [
                "SHOW" | "DESCRIBE" | "DESC" | "EXPLAIN" | "USE" | "COMMIT" | "ROLLBACK",
                ..,
            ]
            | ["START", "TRANSACTION", "READ", "ONLY", ..] => true,
            _ => false,
        };
        if !allowed {
            return Err(AppError::new(
                AppErrorKind::Conflict,
                ApiError::new(
                    "datasource_read_only",
                    "The datasource connection is configured as read-only",
                ),
            ));
        }
    }
    Ok(())
}

fn validate_forced_read_console(statements: &[String]) -> Result<(), AppError> {
    let [statement] = statements else {
        return Err(AppError::invalid(
            "chart_query_must_be_read_only",
            "Chart refresh accepts exactly one MySQL SELECT statement",
        ));
    };
    let tokens = read_policy_tokens(
        statement,
        "chart_query_must_be_read_only",
        "Chart refresh SQL must not use MySQL executable comments",
    )?;
    let parsed = Parser::parse_sql(&MySqlDialect {}, statement).map_err(|_| {
        AppError::invalid(
            "chart_query_must_be_read_only",
            "Chart refresh SQL must be one valid MySQL SELECT statement",
        )
    })?;
    if !matches!(parsed.as_slice(), [Statement::Query(_)]) {
        return Err(AppError::invalid(
            "chart_query_must_be_read_only",
            "Chart refresh accepts exactly one MySQL SELECT statement",
        ));
    }

    if !matches!(
        tokens.first(),
        Some(SqlToken::Word(keyword)) if keyword == "SELECT" || keyword == "WITH"
    ) {
        return Err(AppError::invalid(
            "chart_query_must_be_read_only",
            "Chart refresh accepts SELECT statements and SELECT CTEs only",
        ));
    }
    let words = tokens
        .iter()
        .filter_map(|token| match token {
            SqlToken::Word(word) => Some(word.as_str()),
            SqlToken::Semicolon => None,
        })
        .collect::<Vec<_>>();
    let mutating = words.iter().any(|word| {
        matches!(
            *word,
            "INSERT"
                | "UPDATE"
                | "DELETE"
                | "REPLACE"
                | "CREATE"
                | "ALTER"
                | "DROP"
                | "TRUNCATE"
                | "RENAME"
                | "CALL"
                | "GRANT"
                | "REVOKE"
                | "LOAD"
        )
    });
    let unsafe_select = words.windows(2).any(|window| {
        matches!(
            window,
            ["INTO", "OUTFILE" | "DUMPFILE"] | ["FOR", "UPDATE" | "SHARE"]
        )
    }) || words
        .windows(4)
        .any(|window| matches!(window, ["LOCK", "IN", "SHARE", "MODE"]));
    if mutating || unsafe_select {
        return Err(AppError::invalid(
            "chart_query_must_be_read_only",
            "Chart refresh SQL must not write data, lock rows, or write server files",
        ));
    }
    Ok(())
}

fn validate_single_write_sql(sql: &str) -> Result<String, AppError> {
    if contains_delimiter_directive(sql) {
        return Err(AppError::invalid(
            "invalid_database_write",
            "DELIMITER is not accepted by the confirmed MySQL write surface",
        ));
    }
    let mut statements = split_mysql_script(sql)?;
    if statements.len() != 1 {
        return Err(AppError::invalid(
            "invalid_database_write",
            "Exactly one MySQL write statement is required",
        ));
    }
    let statement = statements.pop().expect("length checked above");
    let tokens = sql_tokens(&statement)?;
    let first_word = tokens.iter().find_map(|token| match token {
        SqlToken::Word(word) => Some(word.as_str()),
        SqlToken::Semicolon => None,
    });
    if !matches!(
        first_word,
        Some(
            "INSERT"
                | "UPDATE"
                | "DELETE"
                | "REPLACE"
                | "CREATE"
                | "ALTER"
                | "DROP"
                | "TRUNCATE"
                | "RENAME"
                | "GRANT"
                | "REVOKE"
                | "ANALYZE"
                | "OPTIMIZE"
                | "REPAIR"
                | "CALL"
        )
    ) {
        return Err(AppError::invalid(
            "database_write_statement_required",
            "The confirmed MySQL write surface accepts one DML, DDL, grant, or routine statement",
        ));
    }
    Ok(statement)
}

fn contains_delimiter_directive(sql: &str) -> bool {
    const KEYWORD: &str = "delimiter";

    sql.lines().any(|line| {
        let line = line.trim_start();
        line.get(..KEYWORD.len()).is_some_and(|prefix| {
            prefix.eq_ignore_ascii_case(KEYWORD)
                && line
                    .as_bytes()
                    .get(KEYWORD.len())
                    .is_none_or(u8::is_ascii_whitespace)
        })
    })
}

fn mysql_console_cancelled(reason: Option<String>) -> AppError {
    AppError::new(
        AppErrorKind::Conflict,
        ApiError::new(
            "mysql_console_cancelled",
            reason.unwrap_or_else(|| "The MySQL Console execution was cancelled".to_owned()),
        ),
    )
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn console_column(index: usize, column: &Column) -> Result<ResultColumn, AppError> {
    let column = mysql_column(index, column)?;
    let value_type = match wire::JdbcValueType::try_from(column.value_type) {
        Ok(wire::JdbcValueType::Boolean) => JdbcValueType::Boolean,
        Ok(wire::JdbcValueType::SignedInteger) => JdbcValueType::SignedInteger,
        Ok(wire::JdbcValueType::UnsignedInteger) => JdbcValueType::UnsignedInteger,
        Ok(wire::JdbcValueType::Float32) => JdbcValueType::Float32,
        Ok(wire::JdbcValueType::Float64) => JdbcValueType::Float64,
        Ok(wire::JdbcValueType::Decimal) => JdbcValueType::Decimal,
        Ok(wire::JdbcValueType::Text) => JdbcValueType::Text,
        Ok(wire::JdbcValueType::Binary) => JdbcValueType::Binary,
        Ok(wire::JdbcValueType::Date) => JdbcValueType::Date,
        Ok(wire::JdbcValueType::Time) => JdbcValueType::Time,
        Ok(wire::JdbcValueType::Timestamp) => JdbcValueType::Timestamp,
        Ok(wire::JdbcValueType::TimestampWithTimeZone) => JdbcValueType::TimestampWithTimeZone,
        Ok(wire::JdbcValueType::Json) => JdbcValueType::Json,
        Ok(wire::JdbcValueType::Uuid) => JdbcValueType::Uuid,
        Ok(wire::JdbcValueType::Opaque) => JdbcValueType::Opaque,
        Ok(wire::JdbcValueType::Unspecified) | Err(_) => return Err(AppError::internal()),
    };
    let nullability = match wire::ColumnNullability::try_from(column.nullability) {
        Ok(wire::ColumnNullability::Unknown) => chat2db_contract::ColumnNullability::Unknown,
        Ok(wire::ColumnNullability::NoNulls) => chat2db_contract::ColumnNullability::NoNulls,
        Ok(wire::ColumnNullability::Nullable) => chat2db_contract::ColumnNullability::Nullable,
        Err(_) => return Err(AppError::internal()),
    };
    Ok(ResultColumn {
        ordinal: column.ordinal,
        label: column.label,
        name: column.name,
        jdbc_type: column.jdbc_type,
        jdbc_type_name: column.jdbc_type_name,
        value_type,
        nullability,
        precision: column.precision,
        scale: column.scale,
        display_size: column.display_size,
        signed: column.signed,
        catalog_name: column.catalog_name,
        schema_name: column.schema_name,
        table_name: column.table_name,
    })
}

fn console_row(row: Row, columns: &[Column]) -> Result<ResultRow, AppError> {
    if row.len() != columns.len() {
        return Err(AppError::internal());
    }
    Ok(ResultRow {
        values: row
            .unwrap()
            .into_iter()
            .zip(columns)
            .map(|(value, column)| console_mysql_value(value, column))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn reserve_console_result_bytes(total: &mut u64, row: &ResultRow) -> Result<(), AppError> {
    let row_bytes = console_row_retained_bytes(row);
    let next = total.saturating_add(row_bytes);
    if next > MAX_CONSOLE_RESULT_BYTES {
        return Err(resource_error(
            "mysql_console_result_too_large",
            format!(
                "MySQL Console results are limited to {MAX_CONSOLE_RESULT_BYTES} retained bytes"
            ),
        ));
    }
    *total = next;
    Ok(())
}

fn console_row_retained_bytes(row: &ResultRow) -> u64 {
    let mut bytes = u64::try_from(size_of::<ResultRow>()).unwrap_or(u64::MAX);
    bytes = bytes.saturating_add(
        u64::try_from(row.values.capacity())
            .unwrap_or(u64::MAX)
            .saturating_mul(u64::try_from(size_of::<JdbcValue>()).unwrap_or(u64::MAX)),
    );
    for value in &row.values {
        let value_bytes = match value {
            JdbcValue::Null | JdbcValue::Boolean { .. } => 0,
            JdbcValue::SignedInteger { value }
            | JdbcValue::UnsignedInteger { value }
            | JdbcValue::Float32 { value }
            | JdbcValue::Float64 { value }
            | JdbcValue::Decimal { value }
            | JdbcValue::Text { value }
            | JdbcValue::Binary { value }
            | JdbcValue::Date { value }
            | JdbcValue::Time { value }
            | JdbcValue::Timestamp { value }
            | JdbcValue::TimestampWithTimeZone { value }
            | JdbcValue::Json { value }
            | JdbcValue::Uuid { value } => value.capacity(),
            JdbcValue::Opaque {
                type_name,
                display_value,
            } => type_name
                .capacity()
                .saturating_add(display_value.capacity()),
        };
        bytes = bytes.saturating_add(u64::try_from(value_bytes).unwrap_or(u64::MAX));
    }
    bytes
}

fn console_mysql_value(value: Value, column: &Column) -> Result<JdbcValue, AppError> {
    if matches!(value, Value::NULL) {
        return Ok(JdbcValue::Null);
    }
    match mysql_value_type(column) {
        wire::JdbcValueType::Text => Ok(JdbcValue::Text {
            value: mysql_text_with_limit(value, MAX_CONSOLE_VALUE_BYTES)?,
        }),
        wire::JdbcValueType::Binary => Ok(JdbcValue::Binary {
            value: BASE64_STANDARD.encode(mysql_binary_with_limit(value, MAX_CONSOLE_VALUE_BYTES)?),
        }),
        wire::JdbcValueType::Json => Ok(JdbcValue::Json {
            value: mysql_text_with_limit(value, MAX_CONSOLE_VALUE_BYTES)?,
        }),
        _ => console_value(mysql_value(value, column)?),
    }
}

fn console_value(value: wire::JdbcValue) -> Result<JdbcValue, AppError> {
    use wire::jdbc_value::Value as WireValue;

    Ok(match value.value.ok_or_else(AppError::internal)? {
        WireValue::NullValue(_) => JdbcValue::Null,
        WireValue::BooleanValue(value) => JdbcValue::Boolean { value },
        WireValue::SignedIntegerValue(value) => JdbcValue::SignedInteger {
            value: value.to_string(),
        },
        WireValue::UnsignedIntegerValue(value) => JdbcValue::UnsignedInteger {
            value: value.to_string(),
        },
        WireValue::Float32Value(value) => JdbcValue::Float32 {
            value: console_float32(value),
        },
        WireValue::Float64Value(value) => JdbcValue::Float64 {
            value: console_float64(value),
        },
        WireValue::DecimalValue(value) => JdbcValue::Decimal { value },
        WireValue::TextValue(value) => JdbcValue::Text { value },
        WireValue::BinaryValue(value) => JdbcValue::Binary {
            value: BASE64_STANDARD.encode(value),
        },
        WireValue::DateValue(value) => JdbcValue::Date { value },
        WireValue::TimeValue(value) => JdbcValue::Time { value },
        WireValue::TimestampValue(value) => JdbcValue::Timestamp { value },
        WireValue::TimestampWithTimeZoneValue(value) => JdbcValue::TimestampWithTimeZone { value },
        WireValue::JsonValue(value) => JdbcValue::Json { value },
        WireValue::UuidValue(value) => JdbcValue::Uuid { value },
        WireValue::OpaqueValue(value) => JdbcValue::Opaque {
            type_name: value.type_name,
            display_value: value.display_value,
        },
    })
}

fn console_float32(value: f32) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value == f32::INFINITY {
        "Infinity".to_owned()
    } else if value == f32::NEG_INFINITY {
        "-Infinity".to_owned()
    } else {
        value.to_string()
    }
}

fn console_float64(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value == f64::INFINITY {
        "Infinity".to_owned()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".to_owned()
    } else {
        value.to_string()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScriptState {
    Normal,
    SingleQuote,
    DoubleQuote,
    Backtick,
    LineComment,
    BlockComment,
}

pub(crate) fn split_mysql_script(script: &str) -> Result<Vec<String>, AppError> {
    let bytes = script.as_bytes();
    let mut delimiter = b";".to_vec();
    let mut statements = Vec::new();
    let mut state = ScriptState::Normal;
    let mut statement_start = 0_usize;
    let mut index = 0_usize;
    let mut at_line_start = true;

    while index < bytes.len() {
        if state == ScriptState::Normal
            && at_line_start
            && bytes[statement_start..index]
                .iter()
                .all(u8::is_ascii_whitespace)
            && let Some((new_delimiter, next_line)) = delimiter_directive(script, index)?
        {
            delimiter = new_delimiter.into_bytes();
            statement_start = next_line;
            index = next_line;
            at_line_start = true;
            continue;
        }

        match state {
            ScriptState::Normal => {
                if bytes[index..].starts_with(&delimiter) {
                    push_mysql_statement(&mut statements, &script[statement_start..index])?;
                    index += delimiter.len();
                    statement_start = index;
                    at_line_start = false;
                    continue;
                }
                match bytes[index] {
                    b'\'' => state = ScriptState::SingleQuote,
                    b'"' => state = ScriptState::DoubleQuote,
                    b'`' => state = ScriptState::Backtick,
                    b'#' => state = ScriptState::LineComment,
                    b'-' if is_mysql_dash_comment(bytes, index) => {
                        state = ScriptState::LineComment;
                        index += 1;
                    }
                    b'/' if bytes.get(index + 1) == Some(&b'*') => {
                        state = ScriptState::BlockComment;
                        index += 1;
                    }
                    b'\n' => at_line_start = true,
                    byte if byte.is_ascii_whitespace() && at_line_start => {}
                    _ => at_line_start = false,
                }
            }
            ScriptState::SingleQuote | ScriptState::DoubleQuote | ScriptState::Backtick => {
                let quote = match state {
                    ScriptState::SingleQuote => b'\'',
                    ScriptState::DoubleQuote => b'"',
                    ScriptState::Backtick => b'`',
                    _ => unreachable!("quoted state is matched above"),
                };
                if bytes[index] == b'\\' && state != ScriptState::Backtick {
                    index = index.saturating_add(1);
                } else if bytes[index] == quote {
                    if bytes.get(index + 1) == Some(&quote) {
                        index += 1;
                    } else {
                        state = ScriptState::Normal;
                    }
                }
            }
            ScriptState::LineComment => {
                if bytes[index] == b'\n' {
                    state = ScriptState::Normal;
                    at_line_start = true;
                }
            }
            ScriptState::BlockComment => {
                if bytes[index] == b'\n' {
                    at_line_start = true;
                } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    state = ScriptState::Normal;
                    index += 1;
                }
            }
        }
        index += 1;
    }

    match state {
        ScriptState::Normal | ScriptState::LineComment => {}
        ScriptState::SingleQuote => {
            return Err(invalid_console_script("unterminated string literal"));
        }
        ScriptState::DoubleQuote => {
            return Err(invalid_console_script("unterminated quoted string literal"));
        }
        ScriptState::Backtick => {
            return Err(invalid_console_script("unterminated quoted identifier"));
        }
        ScriptState::BlockComment => {
            return Err(invalid_console_script("unterminated block comment"));
        }
    }
    push_mysql_statement(&mut statements, &script[statement_start..])?;
    Ok(statements)
}

fn delimiter_directive(
    script: &str,
    line_start: usize,
) -> Result<Option<(String, usize)>, AppError> {
    const KEYWORD: &str = "delimiter";

    let line_end = script.as_bytes()[line_start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(script.len(), |offset| line_start + offset);
    let line = script[line_start..line_end].trim();
    let Some(prefix) = line.get(..KEYWORD.len()) else {
        return Ok(None);
    };
    if !prefix.eq_ignore_ascii_case(KEYWORD)
        || line
            .as_bytes()
            .get(KEYWORD.len())
            .is_some_and(|byte| !byte.is_ascii_whitespace())
    {
        return Ok(None);
    }
    let delimiter = line[KEYWORD.len()..].trim();
    if delimiter.is_empty()
        || delimiter.len() > 16
        || delimiter.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return Err(invalid_console_script(
            "DELIMITER must name one non-whitespace token of at most 16 bytes",
        ));
    }
    let next_line = if line_end < script.len() {
        line_end + 1
    } else {
        line_end
    };
    Ok(Some((delimiter.to_owned(), next_line)))
}

fn is_mysql_dash_comment(bytes: &[u8], index: usize) -> bool {
    bytes.get(index + 1) == Some(&b'-')
        && bytes
            .get(index + 2)
            .is_none_or(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
}

fn push_mysql_statement(statements: &mut Vec<String>, statement: &str) -> Result<(), AppError> {
    let statement = statement.trim();
    if !statement.is_empty() {
        if statements.len() >= MAX_CONSOLE_STATEMENTS {
            return Err(resource_error(
                "mysql_console_too_many_statements",
                format!("MySQL Console scripts are limited to {MAX_CONSOLE_STATEMENTS} statements"),
            ));
        }
        statements.push(statement.to_owned());
    }
    Ok(())
}

fn invalid_console_script(detail: &str) -> AppError {
    AppError::invalid(
        "invalid_mysql_console_script",
        format!("The MySQL Console script contains an {detail}"),
    )
}

fn quote_console_identifier(value: &str, field: &str) -> Result<String, AppError> {
    if value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES || value.contains('\0') {
        return Err(AppError::invalid(
            "invalid_mysql_console_request",
            format!("{field} is invalid"),
        ));
    }
    Ok(format!("`{}`", value.replace('`', "``")))
}

async fn cancel_console_connection(options: Opts, connection_id: u32) -> Result<(), AppError> {
    let mut control = open_connection_with_opts(options).await?;
    let query_cancel =
        kill_console_target(&mut control, format!("KILL QUERY {connection_id}")).await;
    let connection_cancel =
        kill_console_target(&mut control, format!("KILL CONNECTION {connection_id}")).await;
    disconnect_raw_quietly(control).await;
    match (query_cancel, connection_cancel) {
        (_, Ok(())) => Ok(()),
        (Ok(()), Err(error)) => Err(error),
        (Err(query_error), Err(connection_error)) => {
            tracing::warn!(error = %query_error, "native MySQL query cancellation failed before connection termination");
            Err(connection_error)
        }
    }
}

async fn kill_console_target(conn: &mut Conn, sql: String) -> Result<(), AppError> {
    let result = tokio::time::timeout(CONTROL_TIMEOUT, conn.query_drop(sql))
        .await
        .map_err(|_| {
            AppError::unavailable(
                "mysql_termination_timeout",
                "The MySQL Console connection could not be terminated in time",
            )
        })?;
    match result {
        Ok(()) => Ok(()),
        Err(MysqlError::Server(server)) if server.code == 1094 => Ok(()),
        Err(error) => Err(mysql_query_error(error)),
    }
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

    let parameters = mysql_query_parameters(&query.parameters)?;
    let prepared = prepare_resolved_connection(&resolved).await?;
    let options = prepared.options.clone();
    let mut conn = open_query_connection(prepared, &mut cancellation).await?;
    let connection_id = conn.id();
    if let Err(error) = start_read_only_transaction(&mut conn).await {
        disconnect_quietly(conn).await;
        return Err(error.into());
    }
    let query_result = {
        let query_future = conn.exec_iter(query.sql, parameters);
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
    mysql_text_with_limit(value, MAX_SCALAR_BYTES)
}

fn mysql_text_with_limit(value: Value, max_bytes: usize) -> Result<String, AppError> {
    match value {
        Value::Bytes(value) => mysql_utf8_with_limit(value, max_bytes),
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
    mysql_binary_with_limit(value, MAX_SCALAR_BYTES)
}

fn mysql_binary_with_limit(value: Value, max_bytes: usize) -> Result<Vec<u8>, AppError> {
    match value {
        Value::Bytes(value) if value.len() <= max_bytes => Ok(value),
        Value::Bytes(_) => Err(resource_error(
            "mysql_scalar_too_large",
            format!("A MySQL value exceeds {max_bytes} bytes"),
        )),
        other => Ok(mysql_text_with_limit(other, max_bytes)?.into_bytes()),
    }
}

fn mysql_utf8(value: Vec<u8>) -> Result<String, AppError> {
    mysql_utf8_with_limit(value, MAX_SCALAR_BYTES)
}

fn mysql_utf8_with_limit(value: Vec<u8>, max_bytes: usize) -> Result<String, AppError> {
    if value.len() > max_bytes {
        return Err(resource_error(
            "mysql_scalar_too_large",
            format!("A MySQL value exceeds {max_bytes} bytes"),
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

fn table_metadata(
    (
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
    ): TableRow,
    schema_name: &str,
) -> TableMetadata {
    TableMetadata {
        database_name,
        schema_name: schema_name.to_owned(),
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
        ..TableMetadata::default()
    }
}

fn column_metadata(
    database_name: &str,
    schema_name: &str,
    table_name: &str,
    ColumnRow {
        name,
        data_type,
        default_value,
        extra,
        comment,
        column_key,
        is_nullable,
        ordinal_position,
        numeric_scale,
        column_definition,
        charset,
        collation,
        primary_key_order,
    }: ColumnRow,
) -> ColumnMetadata {
    let data_type = data_type.to_ascii_uppercase();
    let (column_size, decimal_digits) =
        mysql_column_size(&data_type, &column_definition, numeric_scale);
    ColumnMetadata {
        database_name: database_name.to_owned(),
        schema_name: schema_name.to_owned(),
        table_name: table_name.to_owned(),
        name,
        column_type: mysql_metadata_column_type(&data_type, &column_definition),
        default_value,
        auto_increment: Some(extra.contains("auto_increment")),
        comment,
        primary_key: Some(column_key.eq_ignore_ascii_case("PRI")),
        primary_key_order,
        column_size,
        decimal_digits,
        ordinal_position: Some(ordinal_position),
        nullable: Some(i32::from(is_nullable.eq_ignore_ascii_case("YES"))),
        extent: mysql_enum_set_extent(&data_type, &column_definition),
        charset: charset.unwrap_or_default(),
        collation: collation.unwrap_or_default(),
        on_update_current_timestamp: Some(extra.contains("on update CURRENT_TIMESTAMP")),
        ..ColumnMetadata::default()
    }
}

fn mysql_metadata_column_type(data_type: &str, column_definition: &str) -> String {
    let mut projected = data_type.to_owned();
    for modifier in ["UNSIGNED", "ZEROFILL"] {
        if mysql_column_has_modifier(column_definition, modifier) {
            projected.push(' ');
            projected.push_str(modifier);
        }
    }
    projected
}

fn mysql_column_has_modifier(column_definition: &str, expected: &str) -> bool {
    mysql_column_modifier_suffix(column_definition)
        .split_ascii_whitespace()
        .any(|modifier| modifier.eq_ignore_ascii_case(expected))
}

fn mysql_column_modifier_suffix(column_definition: &str) -> &str {
    if let Some(close) = column_definition.rfind(')') {
        &column_definition[close + 1..]
    } else if let Some(separator) = column_definition.find(char::is_whitespace) {
        &column_definition[separator..]
    } else {
        ""
    }
}

fn mysql_column_reorder_hazard(
    column_definition: &str,
    extra: &str,
    generation_expression: &str,
) -> Option<&'static str> {
    let normalized_extra = extra
        .split_ascii_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase();
    if !generation_expression.trim().is_empty()
        || normalized_extra
            .split_ascii_whitespace()
            .any(|part| part == "GENERATED")
    {
        return Some("generated-column expressions are not represented by the editor");
    }
    if normalized_extra
        .split_ascii_whitespace()
        .any(|part| part == "INVISIBLE")
    {
        return Some("the INVISIBLE attribute is not represented by the editor");
    }
    if mysql_column_has_modifier(column_definition, "ZEROFILL") {
        return Some("the ZEROFILL attribute is not represented by the editor");
    }
    if !matches!(
        normalized_extra.as_str(),
        "" | "AUTO_INCREMENT" | "ON UPDATE CURRENT_TIMESTAMP"
    ) {
        return Some("one or more MySQL column attributes are not represented by the editor");
    }
    None
}

fn mysql_enum_set_extent(data_type: &str, column_definition: &str) -> String {
    if !matches!(data_type, "ENUM" | "SET") {
        return String::new();
    }
    let Some(open) = column_definition.find('(') else {
        return String::new();
    };
    let Some(close) = column_definition.rfind(')') else {
        return String::new();
    };
    if close <= open {
        return String::new();
    }
    column_definition[open..=close].to_owned()
}

fn mysql_column_size(
    data_type: &str,
    column_definition: &str,
    numeric_scale: Option<i32>,
) -> (Option<i32>, Option<i32>) {
    let mut decimal_digits = Some(numeric_scale.unwrap_or_default());
    let Some(open) = column_definition.find('(') else {
        return (None, decimal_digits);
    };
    let Some(close_offset) = column_definition[open + 1..].find(')') else {
        return (None, decimal_digits);
    };
    if matches!(data_type, "ENUM" | "SET") {
        return (None, decimal_digits);
    }
    let size = &column_definition[open + 1..open + 1 + close_offset];
    let mut parts = size.split(',').map(str::trim);
    let column_size = parts.next().and_then(|value| value.parse::<i32>().ok());
    if let Some(scale) = parts.next().and_then(|value| value.parse::<i32>().ok()) {
        decimal_digits = Some(scale);
    }
    (column_size, decimal_digits)
}

fn index_metadata(rows: Vec<IndexRow>, schema_name: &str) -> Vec<IndexMetadata> {
    let mut indexes: Vec<IndexMetadata> = Vec::new();
    for (
        database_name,
        table_name,
        non_unique,
        index_schema,
        index_name,
        ordinal_position,
        column_name,
        collation,
        cardinality,
        sub_part,
        method,
        comment,
    ) in rows
    {
        let non_unique = non_unique != 0;
        let column = IndexColumnMetadata {
            database_name: database_name.clone(),
            schema_name: schema_name.to_owned(),
            table_name: table_name.clone(),
            index_name: index_name.clone(),
            column_name: column_name.unwrap_or_default(),
            ordinal_position: Some(ordinal_position),
            collation: collation.clone().unwrap_or_default(),
            non_unique: Some(non_unique),
            index_qualifier: index_schema,
            sort_order: mysql_index_sort_order(collation.as_deref()),
            cardinality: cardinality.map(|value| value.to_string()),
            sub_part: sub_part.map(|value| value.to_string()),
            ..IndexColumnMetadata::default()
        };
        if let Some(index) = indexes.iter_mut().find(|index| index.name == index_name) {
            index.columns.push(column);
            continue;
        }
        let unique = !non_unique;
        indexes.push(IndexMetadata {
            database_name,
            schema_name: schema_name.to_owned(),
            table_name,
            name: index_name.clone(),
            index_type: mysql_index_type(&index_name, unique, &method).to_owned(),
            unique: Some(unique),
            comment,
            columns: vec![column],
            method,
            ..IndexMetadata::default()
        });
    }
    indexes
}

fn mysql_index_type(index_name: &str, unique: bool, method: &str) -> &'static str {
    if index_name.eq_ignore_ascii_case("PRIMARY") {
        "Primary"
    } else if unique {
        "Unique"
    } else if method.eq_ignore_ascii_case("SPATIAL") {
        "Spatial"
    } else if method.eq_ignore_ascii_case("FULLTEXT") {
        "Fulltext"
    } else {
        "Normal"
    }
}

fn mysql_index_sort_order(collation: Option<&str>) -> String {
    match collation {
        Some(value) if value.eq_ignore_ascii_case("A") => "ASC".to_owned(),
        Some(value) if value.eq_ignore_ascii_case("D") => "DESC".to_owned(),
        _ => String::new(),
    }
}

fn foreign_key_metadata(
    (
        primary_table_database,
        primary_table_name,
        primary_column_name,
        foreign_table_database,
        foreign_table_name,
        foreign_column_name,
        key_sequence,
        update_rule,
        delete_rule,
        foreign_key_name,
        primary_key_name,
    ): ForeignKeyRow,
) -> ForeignKeyMetadata {
    ForeignKeyMetadata {
        primary_table_database,
        primary_table_name,
        primary_column_name,
        foreign_table_database,
        foreign_table_name,
        foreign_column_name,
        key_sequence,
        update_rule: mysql_referential_rule(&update_rule),
        delete_rule: mysql_referential_rule(&delete_rule),
        foreign_key_name,
        primary_key_name: primary_key_name.unwrap_or_default(),
        deferrability: 7,
        ..ForeignKeyMetadata::default()
    }
}

fn mysql_referential_rule(rule: &str) -> i32 {
    match rule.trim().to_ascii_uppercase().as_str() {
        "CASCADE" => 0,
        "RESTRICT" => 1,
        "SET NULL" => 2,
        "SET DEFAULT" => 4,
        _ => 3,
    }
}

fn normalize_mysql_routine_type(routine_type: &str) -> Result<MysqlRoutineType, AppError> {
    match routine_type.trim().to_ascii_uppercase().as_str() {
        "FUNCTION" => Ok(MysqlRoutineType::Function),
        "PROCEDURE" => Ok(MysqlRoutineType::Procedure),
        _ => Err(AppError::invalid(
            "invalid_routine_invocation_request",
            "routineType must be FUNCTION or PROCEDURE",
        )),
    }
}

fn routine_invocation_parameter(
    routine_type: MysqlRoutineType,
    (ordinal_position, mode, name, data_type): RoutineInvocationParameterRow,
) -> Option<RoutineInvocationParameter> {
    if routine_type == MysqlRoutineType::Function && ordinal_position == 0 {
        return None;
    }
    let name = name?;
    if name.trim().is_empty() {
        return None;
    }
    let mode = match mode.as_deref().map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("OUT") => "OUT",
        Some(value) if value.eq_ignore_ascii_case("INOUT") => "INOUT",
        _ => "IN",
    };
    Some(RoutineInvocationParameter {
        name,
        mode: mode.to_owned(),
        data_type: data_type.trim().to_ascii_uppercase(),
        ordinal_position,
    })
}

fn render_routine_invocation_preview(
    routine_type: MysqlRoutineType,
    routine_name: &str,
    parameters: &[RoutineInvocationParameter],
) -> String {
    let mut setup_sql = Vec::new();
    let mut argument_sql = Vec::new();
    let mut output_variables = Vec::new();
    let mut variable_name_counts = HashMap::new();

    for parameter in parameters {
        let variable = format!(
            "@{}",
            mysql_routine_variable_name(
                &parameter.name,
                argument_sql.len() + 1,
                &mut variable_name_counts,
            )
        );
        argument_sql.push(variable.clone());
        if routine_parameter_is_input(&parameter.mode) {
            setup_sql.push(format!(
                "set {variable} = {};",
                mysql_routine_default_value(&parameter.data_type)
            ));
        }
        if routine_parameter_is_output(&parameter.mode) {
            output_variables.push(variable);
        }
    }

    let mut sql = String::new();
    if !setup_sql.is_empty() {
        sql.push_str(&setup_sql.join("\n"));
        sql.push_str("\n\n");
    }
    sql.push_str(&render_routine_invocation(
        routine_type.command(),
        &mysql_routine_invocation_name(routine_name),
        &argument_sql,
    ));
    if routine_type == MysqlRoutineType::Procedure && !output_variables.is_empty() {
        sql.push_str("\nselect ");
        sql.push_str(&output_variables.join(", "));
        sql.push(';');
    }
    sql
}

fn render_routine_invocation(command: &str, routine_name: &str, arguments: &[String]) -> String {
    if arguments.is_empty() {
        return format!("{command} {routine_name}();");
    }

    let mut sql = format!("{command} {routine_name}(\n");
    for (index, argument) in arguments.iter().enumerate() {
        sql.push_str("    ");
        sql.push_str(argument);
        if index + 1 < arguments.len() {
            sql.push(',');
        }
        sql.push('\n');
    }
    sql.push_str(");");
    sql
}

fn mysql_routine_invocation_name(name: &str) -> String {
    let trimmed = name.trim();
    let mut characters = trimmed.chars();
    let simple = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '$'));
    if simple || mysql_identifier_is_backtick_quoted(trimmed) {
        trimmed.to_owned()
    } else {
        format!("`{}`", trimmed.replace('`', "``"))
    }
}

fn mysql_routine_lookup_name(name: &str) -> String {
    let trimmed = name.trim();
    match trimmed
        .strip_prefix('`')
        .and_then(|value| value.strip_suffix('`'))
    {
        Some(inner) if mysql_identifier_is_backtick_quoted(trimmed) => inner.replace("``", "`"),
        _ => trimmed.to_owned(),
    }
}

fn mysql_identifier_is_backtick_quoted(identifier: &str) -> bool {
    let Some(inner) = identifier
        .strip_prefix('`')
        .and_then(|value| value.strip_suffix('`'))
    else {
        return false;
    };
    if inner.is_empty() {
        return false;
    }

    let mut characters = inner.chars();
    while let Some(character) = characters.next() {
        if character == '`' && characters.next() != Some('`') {
            return false;
        }
    }
    true
}

fn mysql_routine_variable_name(
    parameter_name: &str,
    index: usize,
    variable_name_counts: &mut HashMap<String, usize>,
) -> String {
    let parameter_name = if parameter_name.trim().is_empty() {
        format!("p{index}")
    } else {
        parameter_name.to_owned()
    };
    let mut normalized = parameter_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if normalized.is_empty()
        || !normalized
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
    {
        normalized = format!("p_{index}");
    }
    let count = variable_name_counts.entry(normalized.clone()).or_default();
    *count += 1;
    if *count == 1 {
        normalized
    } else {
        format!("{normalized}_{count}")
    }
}

fn routine_parameter_is_input(mode: &str) -> bool {
    mode.eq_ignore_ascii_case("IN") || mode.eq_ignore_ascii_case("INOUT")
}

fn routine_parameter_is_output(mode: &str) -> bool {
    mode.eq_ignore_ascii_case("OUT") || mode.eq_ignore_ascii_case("INOUT")
}

fn mysql_routine_default_value(data_type: &str) -> &'static str {
    match data_type.trim().to_ascii_uppercase().as_str() {
        "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "INTEGER" | "BIGINT" | "DECIMAL"
        | "NUMERIC" | "FLOAT" | "DOUBLE" | "REAL" | "BIT" | "BOOL" | "BOOLEAN" => "0",
        "CHAR" | "VARCHAR" | "TINYTEXT" | "TEXT" | "MEDIUMTEXT" | "LONGTEXT" | "ENUM" | "SET" => {
            "''"
        }
        "DATE" => "CURRENT_DATE",
        "TIME" => "CURRENT_TIME",
        "DATETIME" | "TIMESTAMP" => "CURRENT_TIMESTAMP",
        "YEAR" => "YEAR(CURRENT_DATE)",
        "JSON" => "'{}'",
        "BINARY" | "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" => "X''",
        _ => "NULL",
    }
}

fn function_parameter_metadata(
    row: RoutineParameterRow,
    schema_name: &str,
) -> FunctionParameterMetadata {
    let RoutineParameterProjection {
        database_name,
        routine_name,
        ordinal_position,
        mode,
        parameter_name,
        data_type,
        precision,
        length,
        scale,
        radix,
        char_octet_length,
    } = routine_parameter_projection(row);
    FunctionParameterMetadata {
        function_database: database_name,
        function_schema: schema_name.to_owned(),
        function_name: routine_name.clone(),
        column_name: parameter_name,
        column_type: Some(mysql_function_column_type(
            mode.as_deref(),
            ordinal_position,
        )),
        data_type: Some(mysql_metadata_jdbc_type(&data_type)),
        type_name: data_type.to_ascii_uppercase(),
        precision,
        length,
        scale,
        radix,
        nullable: Some(1),
        char_octet_length,
        ordinal_position: Some(ordinal_position),
        is_nullable: "YES".to_owned(),
        specific_name: routine_name,
        ..FunctionParameterMetadata::default()
    }
}

fn procedure_parameter_metadata(
    row: RoutineParameterRow,
    schema_name: &str,
) -> ProcedureParameterMetadata {
    let RoutineParameterProjection {
        database_name,
        routine_name,
        ordinal_position,
        mode,
        parameter_name,
        data_type,
        precision,
        length,
        scale,
        radix,
        char_octet_length,
    } = routine_parameter_projection(row);
    ProcedureParameterMetadata {
        procedure_database: database_name,
        procedure_schema: schema_name.to_owned(),
        procedure_name: routine_name.clone(),
        column_name: parameter_name,
        column_type: Some(mysql_procedure_column_type(mode.as_deref())),
        data_type: Some(mysql_metadata_jdbc_type(&data_type)),
        type_name: data_type.to_ascii_uppercase(),
        precision,
        length,
        scale,
        radix,
        nullable: Some(1),
        char_octet_length,
        ordinal_position: Some(ordinal_position),
        is_nullable: "YES".to_owned(),
        specific_name: routine_name,
        ..ProcedureParameterMetadata::default()
    }
}

struct RoutineParameterProjection {
    database_name: String,
    routine_name: String,
    ordinal_position: i32,
    mode: Option<String>,
    parameter_name: String,
    data_type: String,
    precision: Option<i32>,
    length: Option<i32>,
    scale: Option<i32>,
    radix: Option<i32>,
    char_octet_length: Option<i32>,
}

fn routine_parameter_projection(
    (
        database_name,
        routine_name,
        ordinal_position,
        mode,
        parameter_name,
        data_type,
        _column_definition,
        character_length,
        character_octet_length,
        numeric_precision,
        numeric_scale,
        datetime_precision,
    ): RoutineParameterRow,
) -> RoutineParameterProjection {
    let temporal_size = mysql_temporal_display_size(&data_type, datetime_precision);
    let precision =
        optional_metadata_i32_clamped(numeric_precision.or(character_length).or(temporal_size));
    let length = optional_metadata_i32_clamped(
        character_octet_length
            .or(character_length)
            .or(temporal_size),
    );
    let scale = numeric_scale.map(|value| i32::try_from(value).unwrap_or(i32::MAX));
    RoutineParameterProjection {
        database_name,
        routine_name,
        ordinal_position,
        mode,
        parameter_name: parameter_name.unwrap_or_default(),
        data_type,
        precision,
        length,
        scale,
        radix: Some(10),
        char_octet_length: optional_metadata_i32_clamped(character_octet_length),
    }
}

fn optional_metadata_i32_clamped(value: Option<u64>) -> Option<i32> {
    value.map(|value| i32::try_from(value).unwrap_or(i32::MAX))
}

fn mysql_temporal_display_size(data_type: &str, fractional_seconds: Option<u32>) -> Option<u64> {
    let fractional = fractional_seconds
        .filter(|value| *value > 0)
        .map_or(0, |value| u64::from(value) + 1);
    match data_type.trim().to_ascii_uppercase().as_str() {
        "DATE" => Some(10),
        "TIME" => Some(8 + fractional),
        "DATETIME" | "TIMESTAMP" => Some(19 + fractional),
        "YEAR" => Some(4),
        _ => None,
    }
}

fn mysql_function_column_type(mode: Option<&str>, ordinal_position: i32) -> i32 {
    match mode.map(str::trim) {
        None if ordinal_position == 0 => 4,
        Some(value) if value.eq_ignore_ascii_case("IN") => 1,
        Some(value) if value.eq_ignore_ascii_case("INOUT") => 2,
        Some(value) if value.eq_ignore_ascii_case("OUT") => 3,
        _ => 0,
    }
}

fn mysql_procedure_column_type(mode: Option<&str>) -> i32 {
    match mode.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("IN") => 1,
        Some(value) if value.eq_ignore_ascii_case("INOUT") => 2,
        Some(value) if value.eq_ignore_ascii_case("OUT") => 4,
        _ => 0,
    }
}

fn mysql_metadata_jdbc_type(data_type: &str) -> i32 {
    match data_type.trim().to_ascii_uppercase().as_str() {
        "BIT" => -7,
        "TINYINT" => -6,
        "SMALLINT" => 5,
        "MEDIUMINT" | "INT" | "INTEGER" => 4,
        "BIGINT" => -5,
        "FLOAT" => 7,
        "DOUBLE" | "DOUBLE PRECISION" | "REAL" => 8,
        "DECIMAL" | "NUMERIC" => 3,
        "DATE" | "YEAR" => 91,
        "TIME" => 92,
        "DATETIME" | "TIMESTAMP" => 93,
        "CHAR" | "ENUM" | "SET" => 1,
        "VARCHAR" => 12,
        "BINARY" => -2,
        "VARBINARY" => -3,
        "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" | "GEOMETRY" => -4,
        "TINYTEXT" | "TEXT" | "MEDIUMTEXT" | "LONGTEXT" | "JSON" => -1,
        "BOOLEAN" | "BOOL" => 16,
        _ => 1111,
    }
}

fn row_string_at(row: &Row, index: usize) -> Result<String, AppError> {
    row_value_at(row, index)
}

fn row_value_at<T: FromValue>(row: &Row, index: usize) -> Result<T, AppError> {
    row.get_opt::<T, _>(index)
        .ok_or_else(result_decode_error)?
        .map_err(|_| result_decode_error())
}

fn validate_metadata_identifier(value: &str, field: &str) -> Result<(), AppError> {
    if value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES || value.contains('\0') {
        return Err(AppError::invalid(
            "invalid_mysql_metadata_request",
            format!("{field} is invalid"),
        ));
    }
    Ok(())
}

fn qualified_identifier(
    database_name: &str,
    database_field: &str,
    object_name: &str,
    object_field: &str,
) -> Result<String, AppError> {
    Ok(format!(
        "{}.{}",
        quote_identifier(database_name, database_field)?,
        quote_identifier(object_name, object_field)?
    ))
}

fn metadata_not_found(kind: &str, database_name: &str, object_name: &str) -> AppError {
    AppError::invalid(
        "mysql_metadata_not_found",
        format!("MySQL {kind} {database_name}.{object_name} does not exist"),
    )
}

fn validate_query_options(options: QueryExecutionOptions) -> Result<(), AppError> {
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

pub(crate) fn quote_identifier(value: &str, field: &str) -> Result<String, AppError> {
    if value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES || value.contains('\0') {
        return Err(AppError::invalid(
            "invalid_table_preview_request",
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
            disconnect_raw_quietly(control).await;
            Ok(())
        }
        Err(MysqlError::Server(server)) if server.code == 1094 => {
            disconnect_raw_quietly(control).await;
            Ok(())
        }
        Err(error) => {
            disconnect_raw_quietly(control).await;
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
    prepared: PreparedMysqlConnection,
    cancellation: &mut watch::Receiver<CancellationRequest>,
) -> Result<ManagedMysqlConnection, QueryTaskError> {
    let open = open_prepared_connection(prepared);
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

async fn finish_read_only_connection_quietly(mut conn: ManagedMysqlConnection) {
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

async fn disconnect_connection(mut conn: ManagedMysqlConnection) -> Result<(), AppError> {
    let connection = conn
        .connection
        .take()
        .expect("managed MySQL connection must exist until cleanup");
    let database_result = disconnect_raw_connection(connection).await;
    let tunnel_result = match conn.tunnel.take() {
        Some(tunnel) => tunnel.close().await,
        None => Ok(()),
    };
    match database_result {
        Ok(()) => tunnel_result,
        Err(error) => {
            if let Err(tunnel_error) = tunnel_result {
                tracing::warn!(error = %tunnel_error, "SSH tunnel cleanup failed after MySQL disconnect failure");
            }
            Err(error)
        }
    }
}

async fn disconnect_raw_connection(conn: Conn) -> Result<(), AppError> {
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

async fn disconnect_quietly(conn: ManagedMysqlConnection) {
    if let Err(error) = disconnect_connection(conn).await {
        tracing::warn!(error = %error, "native MySQL connection cleanup failed");
    }
}

async fn disconnect_raw_quietly(conn: Conn) {
    if let Err(error) = disconnect_raw_connection(conn).await {
        tracing::warn!(error = %error, "native MySQL control connection cleanup failed");
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

pub(crate) async fn resolve_native_connection(
    application: &Application,
    datasource_id: &str,
) -> Result<ResolvedDatasourceConnection, AppError> {
    let storage = application.require_storage()?;
    let resolved = resolve_datasource_connection(&storage, datasource_id).await?;
    if application
        .native_driver_for_datasource_driver_id(&resolved.driver_id)
        .is_none_or(|driver| driver.descriptor().id != "mysql")
    {
        return Err(AppError::invalid(
            "mysql_driver_mismatch",
            "The datasource is not configured with a MySQL driver",
        ));
    }
    Ok(resolved)
}

pub(crate) async fn open_connection(
    connection: &DatasourceConnection,
) -> Result<ManagedMysqlConnection, AppError> {
    open_prepared_connection(prepare_connection(connection).await?).await
}

pub(crate) async fn open_resolved_connection(
    resolved: &ResolvedDatasourceConnection,
) -> Result<ManagedMysqlConnection, AppError> {
    open_prepared_connection(prepare_resolved_connection(resolved).await?).await
}

async fn prepare_connection(
    connection: &DatasourceConnection,
) -> Result<PreparedMysqlConnection, AppError> {
    prepare_connection_with_identity(connection, SshTunnelIdentity::Ephemeral).await
}

async fn prepare_resolved_connection(
    resolved: &ResolvedDatasourceConnection,
) -> Result<PreparedMysqlConnection, AppError> {
    prepare_connection_with_identity(
        &resolved.connection,
        SshTunnelIdentity::Datasource {
            datasource_id: &resolved.datasource_id,
            revision: resolved.datasource_revision,
        },
    )
    .await
}

async fn prepare_connection_with_identity(
    connection: &DatasourceConnection,
    identity: SshTunnelIdentity<'_>,
) -> Result<PreparedMysqlConnection, AppError> {
    let Some(ssh) = connection.ssh.as_ref() else {
        return Ok(PreparedMysqlConnection {
            options: connection_opts(connection)?,
            tunnel: None,
        });
    };
    let (target_host, target_port) = mysql_target(&connection.jdbc_url)?;
    let tunnel = SshTunnel::open(identity, ssh, target_host, target_port).await?;
    let mut forwarded = connection.clone();
    forwarded.jdbc_url = rewrite_mysql_target(&forwarded.jdbc_url, tunnel.local_port())?;
    forwarded.ssh = None;
    let options = match connection_opts(&forwarded) {
        Ok(options) => options,
        Err(error) => {
            if tunnel.close().await.is_err() {
                tracing::warn!("SSH tunnel cleanup failed after MySQL option validation failure");
            }
            return Err(error);
        }
    };
    Ok(PreparedMysqlConnection {
        options,
        tunnel: Some(tunnel),
    })
}

async fn open_prepared_connection(
    mut prepared: PreparedMysqlConnection,
) -> Result<ManagedMysqlConnection, AppError> {
    match open_connection_with_opts(prepared.options).await {
        Ok(connection) => Ok(ManagedMysqlConnection::new(connection, prepared.tunnel)),
        Err(error) => {
            if let Some(tunnel) = prepared.tunnel.take()
                && tunnel.close().await.is_err()
            {
                tracing::warn!("SSH tunnel cleanup failed after MySQL connection failure");
            }
            Err(error)
        }
    }
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

async fn metadata_query<T, F>(query: F) -> Result<T, AppError>
where
    F: Future<Output = Result<T, MysqlError>>,
{
    tokio::time::timeout(METADATA_TIMEOUT, query)
        .await
        .map_err(|_| {
            AppError::unavailable(
                "mysql_metadata_timeout",
                "The MySQL metadata query did not finish in time",
            )
        })?
        .map_err(mysql_query_error)
}

pub(crate) async fn finish_connection<T>(
    conn: ManagedMysqlConnection,
    result: Result<T, AppError>,
) -> Result<T, AppError> {
    let close = disconnect_connection(conn).await;
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
        MysqlError::Driver(DriverError::StmtParamsMismatch { required, supplied }) => {
            AppError::invalid(
                "invalid_query_parameter_count",
                format!(
                    "The MySQL statement expects {required} parameters but {supplied} were supplied"
                ),
            )
        }
        MysqlError::Driver(DriverError::StmtParamsNumberExceedsLimit { supplied }) => {
            AppError::invalid(
                "invalid_query_parameter_count",
                format!("The MySQL statement cannot accept {supplied} parameters"),
            )
        }
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
    use chat2db_contract::{
        DatasourceConnection, DatasourceConnectionProperty, JdbcValue, ResultRow,
    };
    use mysql_async::{Conn, Opts};
    use tokio::sync::watch;

    use super::{
        ColumnRow, ConsoleExecutionError, ConsoleStatementExecution, MAX_CONSOLE_PAGE_SIZE,
        MAX_CONSOLE_RESULT_BYTES, column_metadata, connection_opts, execute_console_statement,
        foreign_key_metadata, function_parameter_metadata, index_metadata,
        is_native_read_candidate, mysql_column_reorder_hazard, mysql_identifier_is_backtick_quoted,
        mysql_metadata_column_type, mysql_routine_default_value, mysql_routine_invocation_name,
        mysql_routine_lookup_name, normalize_mysql_routine_type, normalize_table_type,
        open_connection_with_opts, procedure_parameter_metadata, qualified_identifier,
        quote_identifier, render_routine_invocation_preview, reserve_console_result_bytes,
        routine_invocation_parameter, routine_migration_plan, split_mysql_script,
        validate_console_request, validate_forced_read_console, validate_read_only_console,
        validate_read_sql, validate_single_write_sql,
    };
    use super::{MysqlRoutineType, RoutineInvocationParameter};
    use crate::native_driver_types::{MetadataScope, RoutineMigrationRequest};
    use crate::{NativeConsoleRequest, operation::CancellationRequest};

    #[test]
    fn mysql_console_splitter_respects_literals_identifiers_and_comments() {
        let statements = split_mysql_script(
            "SELECT ';' AS value; SELECT `odd;name` FROM items; -- keep ; here\nSELECT 3",
        )
        .expect("valid script should split");

        assert_eq!(statements.len(), 3);
        assert_eq!(statements[0], "SELECT ';' AS value");
        assert_eq!(statements[1], "SELECT `odd;name` FROM items");
        assert!(statements[2].ends_with("SELECT 3"));
    }

    #[test]
    fn mysql_console_splitter_supports_delimiter_routine_scripts() {
        let statements = split_mysql_script(
            "DELIMITER $$\nCREATE PROCEDURE load_items()\nBEGIN\n  SELECT ';' AS semi;\n  SELECT '$$' AS marker;\nEND$$\nDELIMITER ;\nCALL load_items();",
        )
        .expect("routine script should split on client delimiters");

        assert_eq!(statements.len(), 2);
        assert!(statements[0].starts_with("CREATE PROCEDURE load_items()"));
        assert!(statements[0].ends_with("END"));
        assert_eq!(statements[1], "CALL load_items()");
    }

    #[test]
    fn mysql_console_splitter_does_not_treat_arithmetic_dashes_as_comments() {
        let statements = split_mysql_script("SELECT 4--2; SELECT 3")
            .expect("arithmetic dashes should remain SQL");

        assert_eq!(statements, ["SELECT 4--2", "SELECT 3"]);
    }

    #[test]
    fn mysql_console_splitter_rejects_unterminated_lexemes_and_bad_delimiters() {
        for script in [
            "SELECT 'unterminated",
            "SELECT `unterminated",
            "SELECT 1 /* unterminated",
            "DELIMITER\nSELECT 1",
            "DELIMITER too long delimiter\nSELECT 1",
        ] {
            let error = split_mysql_script(script).expect_err("script must be rejected");
            assert!(
                matches!(
                    error.api_error().code.as_str(),
                    "invalid_mysql_console_script"
                ),
                "unexpected error for {script}: {error}"
            );
        }
    }

    #[test]
    fn mysql_console_splitter_rejects_excessive_statement_counts() {
        let script = std::iter::repeat_n("SELECT 1", 1_001)
            .collect::<Vec<_>>()
            .join(";");
        let error = split_mysql_script(&script).expect_err("statement flood must be rejected");

        assert_eq!(error.api_error().code, "mysql_console_too_many_statements");
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    #[ignore = "requires MYSQL_TEST_HOST, MYSQL_TEST_PORT, MYSQL_TEST_USER, and MYSQL_TEST_PASSWORD"]
    async fn live_mysql_console_kernel_preserves_session_results_and_cancellation() {
        let host = std::env::var("MYSQL_TEST_HOST").expect("MYSQL_TEST_HOST is required");
        let port = std::env::var("MYSQL_TEST_PORT")
            .expect("MYSQL_TEST_PORT is required")
            .parse::<u16>()
            .expect("MYSQL_TEST_PORT must be a TCP port");
        let user = std::env::var("MYSQL_TEST_USER").expect("MYSQL_TEST_USER is required");
        let password =
            std::env::var("MYSQL_TEST_PASSWORD").expect("MYSQL_TEST_PASSWORD is required");
        let options = connection_opts(&DatasourceConnection {
            jdbc_url: format!("mysql://{host}:{port}?sslMode=DISABLED"),
            properties: vec![
                DatasourceConnectionProperty {
                    key: "user".to_owned(),
                    value: user,
                    sensitive: false,
                },
                DatasourceConnectionProperty {
                    key: "password".to_owned(),
                    value: password,
                    sensitive: true,
                },
            ],
            read_only: false,
            ssh: None,
        })
        .expect("live MySQL options should build");
        let mut conn = open_connection_with_opts(options.clone())
            .await
            .expect("live MySQL should connect");
        let connection_id = conn.id();
        let (_sender, mut cancellation) = watch::channel(CancellationRequest::Waiting);

        live_console_statement(
            &mut conn,
            &options,
            connection_id,
            "USE mysql",
            1,
            None,
            &mut cancellation,
        )
        .await;
        live_console_statement(
            &mut conn,
            &options,
            connection_id,
            "CREATE TEMPORARY TABLE chat2db_console_kernel (id INT PRIMARY KEY)",
            2,
            None,
            &mut cancellation,
        )
        .await;
        live_console_statement(
            &mut conn,
            &options,
            connection_id,
            "START TRANSACTION",
            3,
            None,
            &mut cancellation,
        )
        .await;
        let insert = live_console_statement(
            &mut conn,
            &options,
            connection_id,
            "INSERT INTO chat2db_console_kernel VALUES (1)",
            4,
            None,
            &mut cancellation,
        )
        .await;
        assert!(
            !insert.results.is_empty(),
            "insert returned no results: {:?}",
            insert.failure
        );
        assert_eq!(insert.results[0].update_count, 1);

        let inside_transaction = live_console_statement(
            &mut conn,
            &options,
            connection_id,
            "SELECT COUNT(*) AS item_count FROM chat2db_console_kernel",
            5,
            None,
            &mut cancellation,
        )
        .await;
        assert_console_integer(&inside_transaction, "1");
        live_console_statement(
            &mut conn,
            &options,
            connection_id,
            "ROLLBACK",
            6,
            None,
            &mut cancellation,
        )
        .await;
        let after_rollback = live_console_statement(
            &mut conn,
            &options,
            connection_id,
            "SELECT COUNT(*) AS item_count FROM chat2db_console_kernel",
            7,
            None,
            &mut cancellation,
        )
        .await;
        assert_console_integer(&after_rollback, "0");

        let selected_result = live_console_statement(
            &mut conn,
            &options,
            connection_id,
            "SELECT 11 AS value; SELECT 22 AS value",
            8,
            Some(2),
            &mut cancellation,
        )
        .await;
        assert_eq!(selected_result.results.len(), 1);
        assert_eq!(selected_result.results[0].result_set_id, Some(2));
        assert_console_integer(&selected_result, "22");

        let failed = live_console_statement(
            &mut conn,
            &options,
            connection_id,
            "SELECT missing_column FROM chat2db_console_kernel",
            9,
            None,
            &mut cancellation,
        )
        .await;
        assert_eq!(
            failed
                .failure
                .expect("invalid statement should be represented as a recoverable failure")
                .api_error()
                .code,
            "mysql_query_failed"
        );
        let after_failure = live_console_statement(
            &mut conn,
            &options,
            connection_id,
            "SELECT 33 AS value",
            10,
            None,
            &mut cancellation,
        )
        .await;
        assert_console_integer(&after_failure, "33");

        let (cancel, mut cancellation) = watch::channel(CancellationRequest::Waiting);
        let mut retained_result_bytes = 0;
        let cancel_task = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let _ = cancel.send(CancellationRequest::Requested {
                reason: Some("live cancellation".to_owned()),
            });
        });
        let cancelled = execute_console_statement(
            &mut conn,
            options.clone(),
            connection_id,
            "SELECT SLEEP(30)",
            11,
            0,
            1,
            None,
            &mut retained_result_bytes,
            &mut cancellation,
        )
        .await;
        assert!(matches!(
            cancelled,
            Err(ConsoleExecutionError::Cancelled(reason))
                if reason.as_deref() == Some("live cancellation")
        ));
        cancel_task.await.expect("cancellation task should join");
        drop(conn);
    }

    async fn live_console_statement(
        conn: &mut Conn,
        options: &Opts,
        connection_id: u32,
        sql: &str,
        statement_sequence: u32,
        result_set_id: Option<u32>,
        cancellation: &mut watch::Receiver<CancellationRequest>,
    ) -> ConsoleStatementExecution {
        let mut retained_result_bytes = 0;
        match execute_console_statement(
            conn,
            options.clone(),
            connection_id,
            sql,
            statement_sequence,
            0,
            100,
            result_set_id,
            &mut retained_result_bytes,
            cancellation,
        )
        .await
        {
            Ok(execution) => execution,
            Err(ConsoleExecutionError::Cancelled(_)) => {
                panic!("live statement was unexpectedly cancelled")
            }
            Err(ConsoleExecutionError::Fatal(error)) => {
                panic!("live statement failed fatally: {error}")
            }
        }
    }

    #[test]
    fn mysql_console_result_budget_is_global_and_closed() {
        let row = ResultRow {
            values: vec![JdbcValue::Text {
                value: "bounded".to_owned(),
            }],
        };
        let mut retained = 0;
        reserve_console_result_bytes(&mut retained, &row).expect("a small Console row must fit");
        assert!(retained > 0);

        retained = MAX_CONSOLE_RESULT_BYTES;
        let error = reserve_console_result_bytes(&mut retained, &row)
            .expect_err("the global Console result budget must be enforced");
        assert_eq!(error.api_error().code, "mysql_console_result_too_large");
        assert_eq!(retained, MAX_CONSOLE_RESULT_BYTES);
    }

    fn assert_console_integer(execution: &ConsoleStatementExecution, expected: &str) {
        let value = &execution.results[0].rows[0].values[0];
        match value {
            chat2db_contract::JdbcValue::SignedInteger { value }
            | chat2db_contract::JdbcValue::UnsignedInteger { value } => assert_eq!(value, expected),
            other => panic!("expected an integer Console value, got {other:?}"),
        }
    }

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
            ssh: None,
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
    fn mysql_table_types_are_closed() {
        assert_eq!(normalize_table_type("VIEW"), "VIEW");
        assert_eq!(normalize_table_type("BASE TABLE"), "TABLE");
    }

    #[test]
    fn mysql_routine_preview_filters_returns_and_maps_modes() {
        assert_eq!(
            normalize_mysql_routine_type(" function ").expect("FUNCTION must normalize"),
            MysqlRoutineType::Function
        );
        let unsupported = normalize_mysql_routine_type("TRIGGER")
            .expect_err("unsupported routine types must fail closed");
        assert_eq!(
            unsupported.api_error().code,
            "invalid_routine_invocation_request"
        );
        assert!(
            routine_invocation_parameter(
                MysqlRoutineType::Function,
                (
                    0,
                    None,
                    Some("RETURN_VALUE".to_owned()),
                    "decimal".to_owned(),
                ),
            )
            .is_none(),
            "a named FUNCTION return row must not become an invocation argument"
        );
        assert!(
            routine_invocation_parameter(
                MysqlRoutineType::Function,
                (1, Some("IN".to_owned()), None, "int".to_owned()),
            )
            .is_none(),
            "unnamed metadata rows must not become invocation arguments"
        );

        for (mode, expected) in [
            (Some("IN"), "IN"),
            (Some("out"), "OUT"),
            (Some(" InOut "), "INOUT"),
            (Some("UNKNOWN"), "IN"),
            (None, "IN"),
        ] {
            let parameter = routine_invocation_parameter(
                MysqlRoutineType::Procedure,
                (
                    1,
                    mode.map(str::to_owned),
                    Some("value".to_owned()),
                    "varchar".to_owned(),
                ),
            )
            .expect("named procedure parameter must map");
            assert_eq!(parameter.mode, expected);
            assert_eq!(parameter.data_type, "VARCHAR");
        }
    }

    #[test]
    fn mysql_routine_preview_matches_native_sql_rendering() {
        let parameters = vec![
            RoutineInvocationParameter {
                name: "input-value".to_owned(),
                mode: "IN".to_owned(),
                data_type: "INT".to_owned(),
                ordinal_position: 1,
            },
            RoutineInvocationParameter {
                name: "input value".to_owned(),
                mode: "INOUT".to_owned(),
                data_type: "JSON".to_owned(),
                ordinal_position: 2,
            },
            RoutineInvocationParameter {
                name: "2result".to_owned(),
                mode: "OUT".to_owned(),
                data_type: "BLOB".to_owned(),
                ordinal_position: 3,
            },
        ];
        assert_eq!(
            render_routine_invocation_preview(
                MysqlRoutineType::Procedure,
                "`odd``routine`",
                &parameters,
            ),
            "set @input_value = 0;\nset @input_value_2 = '{}';\n\n\
             call `odd``routine`(\n    @input_value,\n    @input_value_2,\n    @p_3\n);\n\
             select @input_value_2, @p_3;"
        );
        assert_eq!(
            render_routine_invocation_preview(MysqlRoutineType::Function, "zero_arg", &[]),
            "select zero_arg();"
        );
        assert_eq!(
            render_routine_invocation_preview(MysqlRoutineType::Procedure, "unknown_proc", &[]),
            "call unknown_proc();"
        );
    }

    #[test]
    fn mysql_routine_preview_quotes_identifiers_and_uses_compatibility_defaults() {
        assert!(mysql_identifier_is_backtick_quoted("`odd``name`"));
        assert!(!mysql_identifier_is_backtick_quoted("``"));
        assert!(!mysql_identifier_is_backtick_quoted("`odd`name`"));
        assert_eq!(
            mysql_routine_invocation_name(" `odd``name` "),
            "`odd``name`"
        );
        assert_eq!(mysql_routine_invocation_name("odd`name"), "`odd``name`");
        assert_eq!(mysql_routine_invocation_name("simple$name"), "simple$name");
        assert_eq!(mysql_routine_lookup_name(" `odd``name` "), "odd`name");
        assert_eq!(mysql_routine_lookup_name("odd`name"), "odd`name");

        for (data_type, expected) in [
            ("decimal", "0"),
            ("varchar", "''"),
            ("date", "CURRENT_DATE"),
            ("time", "CURRENT_TIME"),
            ("timestamp", "CURRENT_TIMESTAMP"),
            ("year", "YEAR(CURRENT_DATE)"),
            ("json", "'{}'"),
            ("longblob", "X''"),
            ("geometry", "NULL"),
        ] {
            assert_eq!(mysql_routine_default_value(data_type), expected);
        }
    }

    #[test]
    fn mysql_routine_migration_preview_is_qualified_and_terminated() {
        let plan = routine_migration_plan(&RoutineMigrationRequest {
            scope: MetadataScope {
                datasource_id: "mysql-local".to_owned(),
                database_name: "inventory".to_owned(),
                schema_name: String::new(),
            },
            database_type: "MYSQL".to_owned(),
            routine_type: " function ".to_owned(),
            routine_name: "`odd``name`".to_owned(),
            ddl: "CREATE FUNCTION `odd``name`() RETURNS INT RETURN 2".to_owned(),
        })
        .expect("valid migration must render");

        assert_eq!(plan.routine_name, "odd`name");
        assert_eq!(
            plan.preview_sql,
            "DROP FUNCTION IF EXISTS `inventory`.`odd``name`;\n\nCREATE FUNCTION `odd``name`() RETURNS INT RETURN 2;"
        );
    }

    #[test]
    fn mysql_routine_migration_rejects_missing_ddl() {
        let error = routine_migration_plan(&RoutineMigrationRequest {
            scope: MetadataScope {
                datasource_id: "mysql-local".to_owned(),
                database_name: "inventory".to_owned(),
                schema_name: String::new(),
            },
            database_type: "MYSQL".to_owned(),
            routine_type: "PROCEDURE".to_owned(),
            routine_name: "refresh_items".to_owned(),
            ddl: "  ".to_owned(),
        })
        .expect_err("empty ddl must fail");
        assert_eq!(error.api_error().code, "invalid_routine_migration_request");
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
            ssh: None,
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
                ssh: None,
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
    fn confirmed_write_policy_accepts_one_write_and_rejects_reads_or_scripts() {
        for sql in [
            "INSERT INTO items(label) VALUES ('new')",
            "UPDATE items SET label = 'changed' WHERE id = 1",
            "DELETE FROM items WHERE id = 1",
            "CREATE TABLE created_by_cli(id BIGINT PRIMARY KEY)",
            "ALTER TABLE items ADD COLUMN note TEXT",
            "GRANT SELECT ON app.* TO 'reader'@'localhost'",
            "CALL mutating_procedure()",
        ] {
            validate_single_write_sql(sql)
                .unwrap_or_else(|error| panic!("{sql} should be accepted: {error}"));
        }
        for sql in [
            "SELECT 1",
            "SHOW TABLES",
            "START TRANSACTION",
            "UPDATE items SET label = 'one'; DELETE FROM items WHERE id = 2",
        ] {
            assert!(
                validate_single_write_sql(sql).is_err(),
                "{sql} should be rejected"
            );
        }

        for sql in [
            "DELIMITER $$\nCREATE PROCEDURE mutate_item()\nBEGIN\n  UPDATE items SET label = 'changed' WHERE id = 1;\nEND$$\nDELIMITER ;",
            "DELIMITER $$\nUPDATE items SET label = 'changed'; DELETE FROM items$$\nDELIMITER ;",
            "  delimiter //\nDELETE FROM items//",
        ] {
            let error = validate_single_write_sql(sql)
                .expect_err("confirmed writes must reject client delimiter directives");
            assert_eq!(error.api_error().code, "invalid_database_write");
        }
    }

    #[test]
    fn console_read_only_policy_allows_inspection_and_rejects_writes() {
        for sql in [
            "SELECT 1",
            "SHOW TABLES",
            "DESCRIBE items",
            "EXPLAIN SELECT * FROM items",
            "USE inventory",
            "START TRANSACTION READ ONLY",
            "COMMIT",
            "ROLLBACK",
        ] {
            validate_read_only_console(&[sql.to_owned()])
                .unwrap_or_else(|error| panic!("{sql} should be read-only: {error}"));
        }
        for sql in [
            "UPDATE items SET label = 'changed'",
            "DELETE FROM items",
            "CALL mutating_procedure()",
            "SELECT * FROM items FOR UPDATE",
            "WITH cte AS (SELECT 1) SELECT * FROM cte",
        ] {
            let error = validate_read_only_console(&[sql.to_owned()])
                .expect_err("writes and ambiguous statements must fail closed");
            assert_eq!(error.api_error().code, "datasource_read_only");
        }
    }

    #[test]
    fn chart_refresh_policy_accepts_select_ctes_and_rejects_side_effects() {
        for sql in [
            "SELECT 1",
            "SELECT '/*! FOR SHARE */' AS harmless_text",
            "WITH values_cte AS (SELECT 1 AS value) SELECT value FROM values_cte",
        ] {
            validate_forced_read_console(&[sql.to_owned()])
                .unwrap_or_else(|error| panic!("{sql} should be chart-safe: {error}"));
        }
        for statements in [
            vec!["UPDATE items SET label = 'changed'".to_owned()],
            vec!["SELECT * FROM items FOR UPDATE".to_owned()],
            vec!["SELECT * FROM items FOR SHARE".to_owned()],
            vec!["SELECT 1 /*! INTO OUTFILE '/tmp/chart' */".to_owned()],
            vec!["SELECT * FROM items /*M! FOR SHARE */".to_owned()],
            vec!["SELECT 1 INTO OUTFILE '/tmp/chart'".to_owned()],
            vec!["SELECT 1".to_owned(), "SELECT 2".to_owned()],
            vec!["WITH ids AS (SELECT 1) UPDATE items SET label = 'changed'".to_owned()],
        ] {
            let error = validate_forced_read_console(&statements)
                .expect_err("chart refresh must fail closed on non-read-only SQL");
            assert_eq!(error.api_error().code, "chart_query_must_be_read_only");
        }
    }

    #[test]
    fn console_page_size_all_uses_the_bounded_complete_window() {
        let mut request = NativeConsoleRequest {
            datasource_id: "datasource-1".to_owned(),
            database_name: "inventory".to_owned(),
            sql: "SELECT 1".to_owned(),
            page_no: 2,
            page_size: 10,
            result_set_id: None,
            single: false,
            page_size_all: false,
            explain: false,
            error_continue: true,
        };
        assert_eq!(
            validate_console_request(&request).expect("paged request must validate"),
            (10, 20)
        );
        request.page_size_all = true;
        assert_eq!(
            validate_console_request(&request).expect("all-rows request must validate"),
            (0, u64::from(MAX_CONSOLE_PAGE_SIZE))
        );
    }

    #[test]
    fn mysql_column_metadata_preserves_native_projection() {
        let column = column_metadata(
            "inventory",
            "",
            "items",
            ColumnRow {
                name: "amount".to_owned(),
                data_type: "decimal".to_owned(),
                default_value: Some("0.00".to_owned()),
                extra: "DEFAULT_GENERATED on update CURRENT_TIMESTAMP".to_owned(),
                comment: "Money".to_owned(),
                column_key: "PRI".to_owned(),
                is_nullable: "NO".to_owned(),
                ordinal_position: 2,
                numeric_scale: Some(2),
                column_definition: "decimal(12,2) unsigned".to_owned(),
                charset: None,
                collation: None,
                primary_key_order: 2,
            },
        );

        assert_eq!(column.database_name, "inventory");
        assert_eq!(column.table_name, "items");
        assert_eq!(column.column_type, "DECIMAL UNSIGNED");
        assert_eq!(column.default_value.as_deref(), Some("0.00"));
        assert_eq!(column.column_size, Some(12));
        assert_eq!(column.decimal_digits, Some(2));
        assert_eq!(column.ordinal_position, Some(2));
        assert_eq!(column.nullable, Some(0));
        assert_eq!(column.primary_key, Some(true));
        assert_eq!(column.primary_key_order, 2);
        assert_eq!(column.auto_increment, Some(false));
        assert_eq!(column.on_update_current_timestamp, Some(true));
    }

    #[test]
    fn mysql_column_metadata_preserves_enum_and_set_definitions() {
        for (data_type, definition, expected_extent) in [
            (
                "enum",
                "enum('','draft','not UNSIGNED value','O''Reilly')",
                "('','draft','not UNSIGNED value','O''Reilly')",
            ),
            (
                "set",
                "set('read','write','close)later')",
                "('read','write','close)later')",
            ),
        ] {
            let column = column_metadata(
                "inventory",
                "",
                "items",
                ColumnRow {
                    name: "permissions".to_owned(),
                    data_type: data_type.to_owned(),
                    default_value: None,
                    extra: String::new(),
                    comment: String::new(),
                    column_key: String::new(),
                    is_nullable: "YES".to_owned(),
                    ordinal_position: 3,
                    numeric_scale: None,
                    column_definition: definition.to_owned(),
                    charset: Some("utf8mb4".to_owned()),
                    collation: Some("utf8mb4_0900_ai_ci".to_owned()),
                    primary_key_order: 0,
                },
            );

            assert_eq!(column.column_type, data_type.to_ascii_uppercase());
            assert_eq!(column.extent, expected_extent);
            assert_eq!(column.column_size, None);
            assert_eq!(column.primary_key_order, 0);
        }
    }

    #[test]
    fn mysql_column_metadata_reads_only_real_type_modifiers() {
        assert_eq!(
            mysql_metadata_column_type("ENUM", "enum('','active','not UNSIGNED value')"),
            "ENUM"
        );
        assert_eq!(
            mysql_metadata_column_type("SET", "set('UNSIGNED','read')"),
            "SET"
        );
        assert_eq!(
            mysql_metadata_column_type("DECIMAL", "decimal(12,2) unsigned"),
            "DECIMAL UNSIGNED"
        );
        assert_eq!(
            mysql_metadata_column_type("INT", "int unsigned zerofill"),
            "INT UNSIGNED ZEROFILL"
        );
    }

    #[test]
    fn mysql_column_reorder_rejects_unrepresented_attributes() {
        assert!(
            mysql_column_reorder_hazard("int", "STORED GENERATED", "(`base` + 1)")
                .is_some_and(|reason| reason.contains("generated-column"))
        );
        assert!(
            mysql_column_reorder_hazard("int", "INVISIBLE", "")
                .is_some_and(|reason| reason.contains("INVISIBLE"))
        );
        assert!(
            mysql_column_reorder_hazard("int unsigned zerofill", "", "")
                .is_some_and(|reason| reason.contains("ZEROFILL"))
        );
        assert_eq!(
            mysql_column_reorder_hazard("enum('UNSIGNED','active')", "", ""),
            None
        );
        assert_eq!(
            mysql_column_reorder_hazard("bigint unsigned", "AUTO_INCREMENT", ""),
            None
        );
    }

    #[test]
    fn mysql_index_metadata_groups_columns_and_uses_native_types() {
        let indexes = index_metadata(
            vec![
                (
                    "inventory".to_owned(),
                    "items".to_owned(),
                    0,
                    "inventory".to_owned(),
                    "PRIMARY".to_owned(),
                    1,
                    Some("id".to_owned()),
                    Some("A".to_owned()),
                    Some(2),
                    None,
                    "BTREE".to_owned(),
                    String::new(),
                ),
                (
                    "inventory".to_owned(),
                    "items".to_owned(),
                    1,
                    "inventory".to_owned(),
                    "idx_label_amount".to_owned(),
                    1,
                    Some("label".to_owned()),
                    Some("A".to_owned()),
                    Some(2),
                    Some(16),
                    "BTREE".to_owned(),
                    "Lookup".to_owned(),
                ),
                (
                    "inventory".to_owned(),
                    "items".to_owned(),
                    1,
                    "inventory".to_owned(),
                    "idx_label_amount".to_owned(),
                    2,
                    Some("amount".to_owned()),
                    Some("D".to_owned()),
                    Some(2),
                    None,
                    "BTREE".to_owned(),
                    "Lookup".to_owned(),
                ),
            ],
            "",
        );

        assert_eq!(indexes.len(), 2);
        assert_eq!(indexes[0].index_type, "Primary");
        assert_eq!(indexes[0].unique, Some(true));
        assert_eq!(indexes[1].index_type, "Normal");
        assert_eq!(indexes[1].columns.len(), 2);
        assert_eq!(indexes[1].columns[0].sort_order, "ASC");
        assert_eq!(indexes[1].columns[0].sub_part.as_deref(), Some("16"));
        assert_eq!(indexes[1].columns[1].sort_order, "DESC");
    }

    #[test]
    fn mysql_relation_and_routine_metadata_match_jdbc_constants() {
        let key = foreign_key_metadata((
            "inventory".to_owned(),
            "parent".to_owned(),
            "id".to_owned(),
            "inventory".to_owned(),
            "child".to_owned(),
            "parent_id".to_owned(),
            1,
            "CASCADE".to_owned(),
            "SET NULL".to_owned(),
            "fk_child_parent".to_owned(),
            Some("PRIMARY".to_owned()),
        ));
        assert_eq!(key.update_rule, 0);
        assert_eq!(key.delete_rule, 2);
        assert_eq!(key.deferrability, 7);

        let function_return = function_parameter_metadata(
            (
                "inventory".to_owned(),
                "calculate_total".to_owned(),
                0,
                None,
                None,
                "decimal".to_owned(),
                "decimal(12,2)".to_owned(),
                None,
                None,
                Some(12),
                Some(2),
                None,
            ),
            "",
        );
        assert_eq!(function_return.column_type, Some(4));
        assert_eq!(function_return.data_type, Some(3));
        assert_eq!(function_return.precision, Some(12));
        assert_eq!(function_return.scale, Some(2));

        let procedure_output = procedure_parameter_metadata(
            (
                "inventory".to_owned(),
                "load_total".to_owned(),
                1,
                Some("OUT".to_owned()),
                Some("total".to_owned()),
                "int".to_owned(),
                "int".to_owned(),
                None,
                None,
                Some(10),
                Some(0),
                None,
            ),
            "",
        );
        assert_eq!(procedure_output.column_type, Some(4));
        assert_eq!(procedure_output.data_type, Some(4));
        assert_eq!(procedure_output.radix, Some(10));

        let long_text_input = procedure_parameter_metadata(
            (
                "inventory".to_owned(),
                "store_text".to_owned(),
                1,
                Some("IN".to_owned()),
                Some("content".to_owned()),
                "longtext".to_owned(),
                "longtext".to_owned(),
                Some(4_294_967_295),
                Some(4_294_967_295),
                None,
                None,
                None,
            ),
            "",
        );
        assert_eq!(long_text_input.precision, Some(i32::MAX));
        assert_eq!(long_text_input.length, Some(i32::MAX));
        assert_eq!(long_text_input.char_octet_length, Some(i32::MAX));
        assert_eq!(long_text_input.nullable, Some(1));
        assert_eq!(long_text_input.is_nullable, "YES");
    }

    #[test]
    fn preview_identifiers_are_quoted_as_one_mysql_identifier() {
        assert_eq!(
            quote_identifier("odd`name", "tableName").expect("identifier should quote"),
            "`odd``name`"
        );
        assert_eq!(
            qualified_identifier("odd`db", "databaseName", "run`me", "procedureName")
                .expect("qualified identifier should quote each component"),
            "`odd``db`.`run``me`"
        );
        assert!(quote_identifier("", "tableName").is_err());
        assert!(quote_identifier("bad\0name", "tableName").is_err());
    }
}
