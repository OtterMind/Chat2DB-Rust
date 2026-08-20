use std::{
    collections::HashMap,
    fmt::Write as _,
    panic::{AssertUnwindSafe, catch_unwind},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use chat2db_contract::{
    ApiError, ColumnNullability, DatasourceConnection, JdbcValue, JdbcValueType, QueryLimits,
    ResultColumn, ResultMetadata, ResultRow, StartQueryRequest,
};
use chat2db_engine_protocol::wire;
use chat2db_storage::Storage;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use futures_util::{FutureExt as _, stream::TryStreamExt};
use prost::Message;
use sqlparser::{
    ast::{Query as SqlQuery, SetExpr, Statement},
    dialect::MsSqlDialect,
    parser::Parser,
};
use tiberius::{
    Client, Column, ColumnData, ColumnType, Config, Query, QueryItem, Row,
    error::Error as TiberiusError, numeric::Numeric,
};
use tokio::{net::TcpStream, sync::watch};
use tokio_util::{
    compat::{Compat, TokioAsyncWriteCompatExt},
    sync::CancellationToken,
};

use crate::{
    AppError, AppErrorKind, Application,
    datasource_session::{ResolvedDatasourceConnection, resolve_datasource_connection},
    native_driver::{
        NativeConnectionDriver, NativeDialectDriver, NativeDriver, NativeMetadataDriver,
        NativeQueryDriver, NativeTableDriver,
    },
    native_driver_types::{
        BuiltSql, ColumnList, ColumnMetadata, CreateSchemaSqlRequest, DatabaseDefinition,
        DatabaseList, DatabaseMetadata, DmlAssignment, DmlColumn, DmlRow, DmlSqlRequest,
        DmlStatement, DmlTarget, DmlTemporalKind, DmlValue, EntityRelationColumn,
        EntityRelationForeignKey, EntityRelationTable, ForeignKeyList, ForeignKeyMetadata,
        FunctionList, FunctionMetadata, FunctionParameterList, FunctionParameterMetadata,
        IndexColumnMetadata, IndexList, IndexMetadata, ListColumnsRequest, ListDatabasesRequest,
        ListIndexesRequest, ListRoutinesRequest, ListSchemasRequest, ListTableKeysRequest,
        ListTablesRequest, ListTriggersRequest, ListViewsRequest, MetadataObjectRef, MetadataScope,
        NamespaceSqlOperation, NamespaceSqlRequest, NativeDriverDescriptor, PrimaryKeyList,
        PrimaryKeyMetadata, ProcedureList, ProcedureMetadata, ProcedureParameterList,
        ProcedureParameterMetadata, SchemaDefinition, SchemaList, SchemaMetadata, TableList,
        TableMetadata, TablePreviewAccepted, TablePreviewRequest, TableRef, TriggerList,
        TriggerMetadata, ViewList,
    },
    operation::CancellationRequest,
    query::{
        DatabaseValue, DatabaseWriteError, NativeConsoleRequest, NativeConsoleResult,
        PreparedQuery, QueryExecutionOptions, QueryParameter, QueryTaskError, RetainedWriter,
    },
    ssh::{SshTunnel, SshTunnelIdentity},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
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
const MAX_CONSOLE_PAGE_SIZE: u32 = 100_000;
const MAX_CONSOLE_STATEMENTS: usize = 1_000;
const MAX_CONSOLE_RESULT_BYTES: u64 = DEFAULT_RESULT_BYTES;
const MAX_IDENTIFIER_BYTES: usize = 256;
const JDBC_SQLSERVER_PREFIX: &str = "jdbc:sqlserver://";
const NATIVE_SQLSERVER_PREFIX: &str = "sqlserver://";

pub(crate) const SQLSERVER_DRIVER_DESCRIPTOR: NativeDriverDescriptor = NativeDriverDescriptor {
    id: "sqlserver",
    implementation: "tiberius",
    database_types: &["SQLSERVER", "MSSQL", "SQL_SERVER"],
    compatibility_aliases: &[
        "com.microsoft.sqlserver.jdbc.SQLServerDriver",
        "microsoft-sql-server",
        "sql-server",
    ],
};

pub(crate) struct SqlServerNativeDriver;

#[async_trait]
impl NativeConnectionDriver for SqlServerNativeDriver {
    async fn test_connection(&self, connection: &DatasourceConnection) -> Result<(), AppError> {
        let conn = open_connection(connection).await?;
        finish_connection(conn, Ok(())).await
    }

    async fn test_connection_with_local_port(
        &self,
        connection: &DatasourceConnection,
    ) -> Result<Option<u16>, AppError> {
        let conn = open_connection(connection).await?;
        let local_port = conn.tunnel.as_ref().map(SshTunnel::local_port);
        finish_connection(conn, Ok(local_port)).await
    }
}

#[async_trait]
impl NativeQueryDriver for SqlServerNativeDriver {
    fn is_read_candidate(&self, sql: &str) -> Result<bool, AppError> {
        is_read_candidate(sql)
    }

    fn validate_query(&self, query: &PreparedQuery) -> Result<(), AppError> {
        validate_query(query)
    }

    async fn execute_query_task(
        &self,
        application: &Application,
        operation_id: &str,
        cancellation: watch::Receiver<CancellationRequest>,
        query: PreparedQuery,
        storage: Storage,
        resolved: ResolvedDatasourceConnection,
    ) -> Result<ResultMetadata, QueryTaskError> {
        match AssertUnwindSafe(execute_query_task(
            application,
            operation_id,
            cancellation,
            query,
            storage,
            resolved,
        ))
        .catch_unwind()
        .await
        {
            Ok(result) => result,
            Err(_) => Err(QueryTaskError::Failed(sqlserver_driver_failure())),
        }
    }

    async fn execute_update(
        &self,
        resolved: ResolvedDatasourceConnection,
        sql: String,
        cancellation: CancellationToken,
    ) -> Result<u64, DatabaseWriteError> {
        match AssertUnwindSafe(execute_update(resolved, sql, cancellation))
            .catch_unwind()
            .await
        {
            Ok(result) => result,
            Err(_) => Err(DatabaseWriteError::unknown(sqlserver_driver_failure())),
        }
    }

    async fn execute_console(
        &self,
        application: &Application,
        request: NativeConsoleRequest,
        cancellation: watch::Receiver<CancellationRequest>,
        force_read_only: bool,
    ) -> Result<Vec<NativeConsoleResult>, AppError> {
        match AssertUnwindSafe(execute_console(
            application,
            request,
            cancellation,
            force_read_only,
        ))
        .catch_unwind()
        .await
        {
            Ok(result) => result,
            Err(_) => Err(sqlserver_driver_failure()),
        }
    }
}

#[async_trait]
impl NativeMetadataDriver for SqlServerNativeDriver {
    async fn list_schemas(
        &self,
        application: &Application,
        request: ListSchemasRequest,
    ) -> Result<SchemaList, AppError> {
        list_schemas(application, &request.datasource_id, &request.database_name).await
    }

    async fn list_databases(
        &self,
        application: &Application,
        request: ListDatabasesRequest,
    ) -> Result<DatabaseList, AppError> {
        list_databases(application, &request.datasource_id).await
    }

    async fn list_tables(
        &self,
        application: &Application,
        request: ListTablesRequest,
    ) -> Result<TableList, AppError> {
        list_tables(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.name_pattern,
        )
        .await
    }

    async fn list_columns(
        &self,
        application: &Application,
        request: ListColumnsRequest,
    ) -> Result<ColumnList, AppError> {
        list_columns(
            application,
            &request.table.scope.datasource_id,
            &request.table.scope.database_name,
            &request.table.scope.schema_name,
            &request.table.table_name,
        )
        .await
    }

    async fn list_indexes(
        &self,
        application: &Application,
        request: ListIndexesRequest,
    ) -> Result<IndexList, AppError> {
        list_indexes(
            application,
            &request.table.scope.datasource_id,
            &request.table.scope.database_name,
            &request.table.scope.schema_name,
            &request.table.table_name,
        )
        .await
    }

    async fn list_views(
        &self,
        application: &Application,
        request: ListViewsRequest,
    ) -> Result<ViewList, AppError> {
        list_views(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.name_pattern,
        )
        .await
    }

    async fn get_view(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<TableMetadata, AppError> {
        get_view(
            application,
            &request.scope.datasource_id,
            &request.scope.database_name,
            &request.scope.schema_name,
            &request.object_name,
        )
        .await
    }

    async fn list_imported_keys(
        &self,
        application: &Application,
        request: ListTableKeysRequest,
    ) -> Result<ForeignKeyList, AppError> {
        list_foreign_keys(application, &request, false).await
    }

    async fn list_exported_keys(
        &self,
        application: &Application,
        request: ListTableKeysRequest,
    ) -> Result<ForeignKeyList, AppError> {
        list_foreign_keys(application, &request, true).await
    }

    async fn list_primary_keys(
        &self,
        application: &Application,
        request: ListTableKeysRequest,
    ) -> Result<PrimaryKeyList, AppError> {
        list_primary_keys(application, &request).await
    }

    async fn list_functions(
        &self,
        application: &Application,
        request: ListRoutinesRequest,
    ) -> Result<FunctionList, AppError> {
        list_functions(application, &request).await
    }

    async fn get_function(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<FunctionMetadata, AppError> {
        get_function(application, &request).await
    }

    async fn list_function_parameters(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<FunctionParameterList, AppError> {
        list_function_parameters(application, &request).await
    }

    async fn list_procedures(
        &self,
        application: &Application,
        request: ListRoutinesRequest,
    ) -> Result<ProcedureList, AppError> {
        list_procedures(application, &request).await
    }

    async fn get_procedure(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<ProcedureMetadata, AppError> {
        get_procedure(application, &request).await
    }

    async fn list_procedure_parameters(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<ProcedureParameterList, AppError> {
        list_procedure_parameters(application, &request).await
    }

    async fn list_triggers(
        &self,
        application: &Application,
        request: ListTriggersRequest,
    ) -> Result<TriggerList, AppError> {
        list_triggers(application, &request).await
    }

    async fn get_trigger(
        &self,
        application: &Application,
        request: MetadataObjectRef,
    ) -> Result<TriggerMetadata, AppError> {
        get_trigger(application, &request).await
    }
}

#[async_trait]
impl NativeTableDriver for SqlServerNativeDriver {
    async fn load_er_tables(
        &self,
        application: &Application,
        datasource_id: &str,
        database_name: &str,
        schema_name: &str,
    ) -> Result<Vec<EntityRelationTable>, AppError> {
        load_er_tables(application, datasource_id, database_name, schema_name).await
    }

    async fn validate_column_reorder(
        &self,
        _application: &Application,
        _datasource_id: &str,
        _database_name: &str,
        _table_name: &str,
        column_names: &[String],
    ) -> Result<(), AppError> {
        if column_names.is_empty() {
            return Ok(());
        }
        Err(AppError::invalid(
            "sqlserver_column_reorder_not_supported",
            "SQL Server cannot reorder existing columns with an in-place ALTER TABLE operation",
        ))
    }

    async fn table_ddl(
        &self,
        application: &Application,
        datasource_id: &str,
        database_name: &str,
        schema_name: &str,
        table_name: &str,
    ) -> Result<String, AppError> {
        table_ddl(
            application,
            datasource_id,
            database_name,
            schema_name,
            table_name,
        )
        .await
    }

    async fn start_table_preview(
        &self,
        application: &Application,
        request: TablePreviewRequest,
        row_limit: u32,
    ) -> Result<TablePreviewAccepted, AppError> {
        start_table_preview(application, request, row_limit).await
    }
}

impl NativeDialectDriver for SqlServerNativeDriver {
    fn build_create_schema(&self, request: CreateSchemaSqlRequest) -> Result<BuiltSql, AppError> {
        build_create_schema(request)
    }

    fn build_namespace_sql(&self, request: NamespaceSqlRequest) -> Result<BuiltSql, AppError> {
        build_namespace_sql(request)
    }

    fn build_dml(&self, request: DmlSqlRequest) -> Result<BuiltSql, AppError> {
        build_dml(request)
    }
}

impl NativeDriver for SqlServerNativeDriver {
    fn descriptor(&self) -> &'static NativeDriverDescriptor {
        &SQLSERVER_DRIVER_DESCRIPTOR
    }

    fn connection(&self) -> Option<&dyn NativeConnectionDriver> {
        Some(self)
    }

    fn query(&self) -> Option<&dyn NativeQueryDriver> {
        Some(self)
    }

    fn metadata(&self) -> Option<&dyn NativeMetadataDriver> {
        Some(self)
    }

    fn tables(&self) -> Option<&dyn NativeTableDriver> {
        Some(self)
    }

    fn dialect(&self) -> Option<&dyn NativeDialectDriver> {
        Some(self)
    }
}

fn build_create_schema(request: CreateSchemaSqlRequest) -> Result<BuiltSql, AppError> {
    let SchemaDefinition {
        database_name,
        name,
        comment,
        owner,
        system: _,
    } = request.schema;
    let quoted_name = quote_dialect_identifier(&name, "schemaName")?;
    let mut statements = Vec::new();
    if !database_name.trim().is_empty() {
        statements.push(format!(
            "USE {};",
            quote_dialect_identifier(&database_name, "databaseName")?
        ));
    }
    let authorization = if owner.trim().is_empty() {
        String::new()
    } else {
        format!(
            " AUTHORIZATION {}",
            quote_dialect_identifier(&owner, "owner")?
        )
    };
    let create_schema = format!("CREATE SCHEMA {quoted_name}{authorization}");
    statements.push(dynamic_statement(&create_schema)?);
    if !comment.is_empty() {
        statements.push(format!(
            "EXEC sys.sp_addextendedproperty @name = N'MS_Description', @value = {}, @level0type = N'SCHEMA', @level0name = {};",
            quote_dialect_literal(&comment, "schema comment")?,
            quote_dialect_literal(&name, "schemaName")?
        ));
    }
    Ok(BuiltSql {
        sql: statements.join("\n"),
    })
}

fn build_namespace_sql(request: NamespaceSqlRequest) -> Result<BuiltSql, AppError> {
    let sql = match request.operation {
        NamespaceSqlOperation::CreateDatabase { database } => build_create_database(&database)?,
        NamespaceSqlOperation::AlterDatabase {
            old_database,
            new_database,
        } => build_alter_database(&old_database, &new_database)?,
        NamespaceSqlOperation::DropDatabase { database_name } => format!(
            "DROP DATABASE {};",
            quote_dialect_identifier(&database_name, "databaseName")?
        ),
        NamespaceSqlOperation::UseDatabase { database_name } => format!(
            "USE {};",
            quote_dialect_identifier(&database_name, "databaseName")?
        ),
        NamespaceSqlOperation::CreateSchema { schema } => {
            return build_create_schema(CreateSchemaSqlRequest { schema });
        }
        NamespaceSqlOperation::AlterSchema {
            old_schema_name,
            new_schema_name,
        } => {
            quote_dialect_identifier(&old_schema_name, "schemaName")?;
            quote_dialect_identifier(&new_schema_name, "schemaName")?;
            return Err(AppError::invalid(
                "sqlserver_schema_rename_unsupported",
                "SQL Server has no direct schema rename operation; create a new schema and transfer objects explicitly",
            ));
        }
        NamespaceSqlOperation::DropSchema { schema_name } => format!(
            "DROP SCHEMA {};",
            quote_dialect_identifier(&schema_name, "schemaName")?
        ),
    };
    Ok(BuiltSql { sql })
}

fn build_create_database(database: &DatabaseDefinition) -> Result<String, AppError> {
    reject_database_charset(&database.charset)?;
    let name = quote_dialect_identifier(&database.name, "databaseName")?;
    let mut create = format!("CREATE DATABASE {name}");
    if !database.collation.trim().is_empty() {
        write!(
            &mut create,
            " COLLATE {}",
            validate_collation(&database.collation)?
        )
        .map_err(|_| AppError::internal())?;
    }
    create.push(';');
    let mut statements = vec![create];
    if !database.owner.trim().is_empty() {
        statements.push(format!(
            "ALTER AUTHORIZATION ON DATABASE::{name} TO {};",
            quote_dialect_identifier(&database.owner, "owner")?
        ));
    }
    if !database.comment.is_empty() {
        statements.push(database_comment_statement(
            &database.name,
            DatabaseCommentOperation::Add,
            &database.comment,
        )?);
    }
    Ok(statements.join("\n"))
}

fn build_alter_database(
    old_database: &DatabaseDefinition,
    new_database: &DatabaseDefinition,
) -> Result<String, AppError> {
    if old_database.charset != new_database.charset {
        return Err(database_charset_unsupported());
    }
    reject_database_charset(&new_database.charset)?;
    let old_name = quote_dialect_identifier(&old_database.name, "databaseName")?;
    let new_name = quote_dialect_identifier(&new_database.name, "databaseName")?;
    let mut statements = Vec::new();
    if old_database.name != new_database.name {
        statements.push(format!(
            "ALTER DATABASE {old_name} MODIFY NAME = {new_name};"
        ));
    }
    let active_name = if old_database.name == new_database.name {
        &old_database.name
    } else {
        &new_database.name
    };
    let quoted_active_name = quote_dialect_identifier(active_name, "databaseName")?;
    if old_database.collation != new_database.collation {
        if new_database.collation.trim().is_empty() {
            return Err(AppError::invalid(
                "sqlserver_database_alter_unsupported",
                "SQL Server database collation cannot be cleared",
            ));
        }
        statements.push(format!(
            "ALTER DATABASE {quoted_active_name} COLLATE {};",
            validate_collation(&new_database.collation)?
        ));
    }
    if old_database.owner != new_database.owner {
        if new_database.owner.trim().is_empty() {
            return Err(AppError::invalid(
                "sqlserver_database_alter_unsupported",
                "SQL Server database ownership cannot be cleared",
            ));
        }
        statements.push(format!(
            "ALTER AUTHORIZATION ON DATABASE::{quoted_active_name} TO {};",
            quote_dialect_identifier(&new_database.owner, "owner")?
        ));
    }
    if old_database.comment != new_database.comment {
        let operation = match (
            old_database.comment.is_empty(),
            new_database.comment.is_empty(),
        ) {
            (true, false) => DatabaseCommentOperation::Add,
            (false, true) => DatabaseCommentOperation::Drop,
            (false, false) => DatabaseCommentOperation::Update,
            (true, true) => unreachable!("equal empty comments were filtered above"),
        };
        statements.push(database_comment_statement(
            active_name,
            operation,
            &new_database.comment,
        )?);
    }
    if statements.is_empty() {
        return Err(AppError::invalid(
            "sqlserver_database_alter_empty",
            "The SQL Server database definition has no supported changes",
        ));
    }
    Ok(statements.join("\n"))
}

#[derive(Clone, Copy)]
enum DatabaseCommentOperation {
    Add,
    Update,
    Drop,
}

fn database_comment_statement(
    database_name: &str,
    operation: DatabaseCommentOperation,
    comment: &str,
) -> Result<String, AppError> {
    let database_name = quote_dialect_identifier(database_name, "databaseName")?;
    let procedure = match operation {
        DatabaseCommentOperation::Add => "sp_addextendedproperty",
        DatabaseCommentOperation::Update => "sp_updateextendedproperty",
        DatabaseCommentOperation::Drop => "sp_dropextendedproperty",
    };
    let value = if matches!(operation, DatabaseCommentOperation::Drop) {
        String::new()
    } else {
        format!(
            ", @value = {}",
            quote_dialect_literal(comment, "database comment")?
        )
    };
    Ok(format!(
        "EXEC {database_name}.sys.{procedure} @name = N'MS_Description'{value};"
    ))
}

fn dynamic_statement(sql: &str) -> Result<String, AppError> {
    Ok(format!(
        "EXEC({});",
        quote_dialect_literal(sql, "dynamic SQL statement")?
    ))
}

fn reject_database_charset(charset: &str) -> Result<(), AppError> {
    if charset.trim().is_empty() {
        Ok(())
    } else {
        Err(database_charset_unsupported())
    }
}

fn database_charset_unsupported() -> AppError {
    AppError::invalid(
        "sqlserver_database_charset_unsupported",
        "SQL Server selects character encoding through collation and has no separate database charset option",
    )
}

fn validate_collation(collation: &str) -> Result<&str, AppError> {
    if collation.is_empty()
        || collation.len() > MAX_IDENTIFIER_BYTES
        || !collation
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(AppError::invalid(
            "invalid_sqlserver_dialect_request",
            "SQL Server collation must contain only ASCII letters, digits, and underscores",
        ));
    }
    Ok(collation)
}

fn build_dml(request: DmlSqlRequest) -> Result<BuiltSql, AppError> {
    let target = dml_target(&request.target)?;
    let sql = match request.statement {
        DmlStatement::SingleInsert { columns, row } => {
            insert_sql(&target, &columns, std::slice::from_ref(&row))?
        }
        DmlStatement::MultiInsert { columns, rows } => insert_sql(&target, &columns, &rows)?,
        DmlStatement::Update {
            assignments,
            predicates,
        } => update_sql(&target, &assignments, &predicates)?,
    };
    Ok(BuiltSql { sql })
}

fn dml_target(target: &DmlTarget) -> Result<String, AppError> {
    let table = quote_dialect_identifier(&target.table_name, "tableName")?;
    let database = target
        .database_name
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    let schema = target
        .schema_name
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    match (database, schema) {
        (Some(database), Some(schema)) => Ok(format!(
            "{}.{}.{}",
            quote_dialect_identifier(database, "databaseName")?,
            quote_dialect_identifier(schema, "schemaName")?,
            table
        )),
        (Some(database), None) => Ok(format!(
            "{}..{}",
            quote_dialect_identifier(database, "databaseName")?,
            table
        )),
        (None, Some(schema)) => Ok(format!(
            "{}.{}",
            quote_dialect_identifier(schema, "schemaName")?,
            table
        )),
        (None, None) => Ok(table),
    }
}

fn insert_sql(target: &str, columns: &[DmlColumn], rows: &[DmlRow]) -> Result<String, AppError> {
    if columns.is_empty() || rows.is_empty() {
        return Err(invalid_dml(
            "SQL Server INSERT requires at least one column and row",
        ));
    }
    let columns_sql = columns
        .iter()
        .map(|column| quote_dialect_identifier(&column.name, "columnName"))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let rows_sql = rows
        .iter()
        .map(|row| {
            if row.values.len() != columns.len() {
                return Err(invalid_dml(
                    "Each SQL Server INSERT row must match the selected column count",
                ));
            }
            row.values
                .iter()
                .zip(columns)
                .map(|(value, column)| dml_value(value, column))
                .collect::<Result<Vec<_>, _>>()
                .map(|values| format!("({})", values.join(", ")))
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(",\n");
    Ok(format!(
        "INSERT INTO {target} ({columns_sql}) VALUES\n{rows_sql};"
    ))
}

fn update_sql(
    target: &str,
    assignments: &[DmlAssignment],
    predicates: &[DmlAssignment],
) -> Result<String, AppError> {
    if assignments.is_empty() || predicates.is_empty() {
        return Err(invalid_dml(
            "SQL Server UPDATE requires assignments and key predicates",
        ));
    }
    let assignments = assignments
        .iter()
        .map(|assignment| {
            Ok(format!(
                "{} = {}",
                quote_dialect_identifier(&assignment.column.name, "columnName")?,
                dml_value(&assignment.value, &assignment.column)?
            ))
        })
        .collect::<Result<Vec<_>, AppError>>()?
        .join(", ");
    let predicates = predicates
        .iter()
        .map(|predicate| {
            let column = quote_dialect_identifier(&predicate.column.name, "columnName")?;
            match predicate.value {
                DmlValue::Null => Ok(format!("{column} IS NULL")),
                _ => Ok(format!(
                    "{column} = {}",
                    dml_value(&predicate.value, &predicate.column)?
                )),
            }
        })
        .collect::<Result<Vec<_>, AppError>>()?
        .join(" AND ");
    Ok(format!(
        "UPDATE {target} SET {assignments} WHERE {predicates};"
    ))
}

fn dml_value(value: &DmlValue, column: &DmlColumn) -> Result<String, AppError> {
    match value {
        DmlValue::Null => Ok("NULL".to_owned()),
        DmlValue::String(value) => {
            let literal = quote_dialect_literal(value, "DML string")?;
            if column
                .data_type_name
                .eq_ignore_ascii_case("uniqueidentifier")
            {
                tiberius::Uuid::parse_str(value)
                    .map_err(|_| invalid_dml("The SQL Server uniqueidentifier value is invalid"))?;
                Ok(format!("CAST({literal} AS uniqueidentifier)"))
            } else {
                Ok(literal)
            }
        }
        DmlValue::Decimal(value) => parse_decimal(value)
            .map(format_numeric)
            .map_err(|_| invalid_dml("The SQL Server decimal value is invalid")),
        DmlValue::Boolean(value) => Ok(if *value { "1" } else { "0" }.to_owned()),
        DmlValue::Temporal { kind, iso8601 } => {
            match kind {
                DmlTemporalKind::Date => NaiveDate::parse_from_str(iso8601, "%Y-%m-%d")
                    .map(|_| ())
                    .map_err(|_| invalid_dml("The SQL Server date value is invalid"))?,
                DmlTemporalKind::Time => NaiveTime::parse_from_str(iso8601, "%H:%M:%S%.f")
                    .map(|_| ())
                    .map_err(|_| invalid_dml("The SQL Server time value is invalid"))?,
                DmlTemporalKind::LocalDatetime => parse_timestamp(iso8601)
                    .map(|_| ())
                    .map_err(|_| invalid_dml("The SQL Server datetime2 value is invalid"))?,
                DmlTemporalKind::OffsetDatetime => DateTime::parse_from_rfc3339(iso8601)
                    .map(|_| ())
                    .map_err(|_| invalid_dml("The SQL Server datetimeoffset value is invalid"))?,
            }
            let data_type = match kind {
                DmlTemporalKind::Date => "date",
                DmlTemporalKind::Time => "time",
                DmlTemporalKind::LocalDatetime => "datetime2",
                DmlTemporalKind::OffsetDatetime => "datetimeoffset",
            };
            Ok(format!(
                "CAST({} AS {data_type})",
                quote_dialect_literal(iso8601, "DML temporal value")?
            ))
        }
        DmlValue::Binary(value) => {
            if value.len() > MAX_SCALAR_BYTES {
                return Err(invalid_dml(
                    "The SQL Server binary value exceeds the scalar limit",
                ));
            }
            Ok(format!("0x{}", hex::encode(value)))
        }
    }
}

fn quote_dialect_identifier(value: &str, field: &str) -> Result<String, AppError> {
    if value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES || value.contains('\0') {
        return Err(AppError::invalid(
            "invalid_sqlserver_dialect_request",
            format!("{field} is invalid"),
        ));
    }
    Ok(format!("[{}]", value.replace(']', "]]")))
}

fn quote_dialect_literal(value: &str, field: &str) -> Result<String, AppError> {
    if value.len() > MAX_SCALAR_BYTES || value.contains('\0') {
        return Err(AppError::invalid(
            "invalid_sqlserver_dialect_request",
            format!("{field} is invalid"),
        ));
    }
    Ok(format!("N'{}'", value.replace('\'', "''")))
}

fn invalid_dml(message: impl Into<String>) -> AppError {
    AppError::invalid("invalid_sqlserver_dml", message.into())
}

type SqlServerClient = Client<Compat<TcpStream>>;

struct PreparedSqlServerConnection {
    config: Config,
    connect_addr: String,
    tunnel: Option<SshTunnel>,
}

struct ManagedSqlServerConnection {
    client: SqlServerClient,
    tunnel: Option<SshTunnel>,
}

enum QueryOpenError {
    Cancelled(Option<String>),
    Failed(AppError),
}

async fn resolve_native_connection(
    application: &Application,
    datasource_id: &str,
) -> Result<ResolvedDatasourceConnection, AppError> {
    let storage = application.require_storage()?;
    let resolved = resolve_datasource_connection(&storage, datasource_id).await?;
    if application
        .native_driver_for_datasource_driver_id(&resolved.driver_id)
        .is_none_or(|driver| driver.descriptor().id != SQLSERVER_DRIVER_DESCRIPTOR.id)
    {
        return Err(AppError::invalid(
            "sqlserver_driver_mismatch",
            "The datasource is not configured with the native SQL Server driver",
        ));
    }
    Ok(resolved)
}

async fn open_connection(
    connection: &DatasourceConnection,
) -> Result<ManagedSqlServerConnection, AppError> {
    open_prepared_connection(prepare_connection(connection, SshTunnelIdentity::Ephemeral).await?)
        .await
}

async fn open_resolved_connection(
    resolved: &ResolvedDatasourceConnection,
    database_name: Option<&str>,
) -> Result<ManagedSqlServerConnection, AppError> {
    let identity = SshTunnelIdentity::Datasource {
        datasource_id: &resolved.datasource_id,
        revision: resolved.datasource_revision,
    };
    let mut prepared = prepare_connection(&resolved.connection, identity).await?;
    if let Some(database_name) = database_name.filter(|value| !value.trim().is_empty()) {
        validate_identifier(database_name, "databaseName")?;
        prepared.config.database(database_name);
    }
    open_prepared_connection(prepared).await
}

async fn prepare_connection(
    connection: &DatasourceConnection,
    identity: SshTunnelIdentity<'_>,
) -> Result<PreparedSqlServerConnection, AppError> {
    let config = connection_config(connection)?;
    let direct_addr = config.get_addr();
    let Some(ssh) = connection.ssh.as_ref() else {
        return Ok(PreparedSqlServerConnection {
            config,
            connect_addr: direct_addr,
            tunnel: None,
        });
    };
    let (target_host, target_port) = sqlserver_target(&connection.jdbc_url)?;
    let tunnel = SshTunnel::open(identity, ssh, target_host, target_port).await?;
    Ok(PreparedSqlServerConnection {
        config,
        connect_addr: format!("127.0.0.1:{}", tunnel.local_port()),
        tunnel: Some(tunnel),
    })
}

async fn open_prepared_connection(
    mut prepared: PreparedSqlServerConnection,
) -> Result<ManagedSqlServerConnection, AppError> {
    let config = prepared.config.clone();
    let connect_addr = prepared.connect_addr.clone();
    let open = async {
        let tcp = TcpStream::connect(&connect_addr)
            .await
            .map_err(|_| sqlserver_connection_failed())?;
        tcp.set_nodelay(true)
            .map_err(|_| sqlserver_connection_failed())?;
        Client::connect(config.clone(), tcp.compat_write())
            .await
            .map_err(|_| sqlserver_connection_failed())
    };
    let client = match tokio::time::timeout(CONNECT_TIMEOUT, open).await {
        Ok(Ok(client)) => client,
        Ok(Err(error)) => {
            close_tunnel_quietly(prepared.tunnel.take()).await;
            return Err(error);
        }
        Err(_) => {
            close_tunnel_quietly(prepared.tunnel.take()).await;
            return Err(AppError::unavailable(
                "sqlserver_connection_timeout",
                "The SQL Server connection attempt timed out",
            ));
        }
    };
    Ok(ManagedSqlServerConnection {
        client,
        tunnel: prepared.tunnel,
    })
}

async fn finish_connection<T>(
    conn: ManagedSqlServerConnection,
    result: Result<T, AppError>,
) -> Result<T, AppError> {
    let ManagedSqlServerConnection { client, tunnel, .. } = conn;
    drop(client);
    let close = match tunnel {
        Some(tunnel) => tunnel.close().await,
        None => Ok(()),
    };
    match result {
        Ok(value) => close.map(|()| value),
        Err(error) => {
            if let Err(close_error) = close {
                tracing::warn!(error = %close_error, "SQL Server SSH tunnel cleanup failed");
            }
            Err(error)
        }
    }
}

async fn close_tunnel_quietly(tunnel: Option<SshTunnel>) {
    if let Some(tunnel) = tunnel
        && let Err(error) = tunnel.close().await
    {
        tracing::warn!(error = %error, "SQL Server SSH tunnel cleanup failed");
    }
}

async fn discard_connection(conn: ManagedSqlServerConnection) {
    let ManagedSqlServerConnection { client, tunnel } = conn;
    drop(client);
    close_tunnel_quietly(tunnel).await;
}

fn connection_config(connection: &DatasourceConnection) -> Result<Config, AppError> {
    let mut jdbc_url = normalize_sqlserver_url(&connection.jdbc_url)?;
    for property in &connection.properties {
        let key = property.key.trim();
        if key.is_empty() || key.contains([';', '=', '\0']) {
            return Err(AppError::invalid(
                "invalid_sqlserver_connection",
                "A SQL Server connection property name is invalid",
            ));
        }
        if !jdbc_url.ends_with(';') {
            jdbc_url.push(';');
        }
        jdbc_url.push_str(key);
        jdbc_url.push('=');
        jdbc_url.push_str(&encode_jdbc_property(&property.value));
    }
    validate_tls_connection_properties(&jdbc_url)?;
    let Ok(Ok(mut config)) = catch_unwind(AssertUnwindSafe(|| Config::from_jdbc_string(&jdbc_url)))
    else {
        return Err(AppError::invalid(
            "invalid_sqlserver_connection",
            "A valid jdbc:sqlserver:// connection URL is required",
        ));
    };
    config.readonly(connection.read_only);
    config.application_name("Chat2DB Rust");
    Ok(config)
}

fn validate_tls_connection_properties(jdbc_url: &str) -> Result<(), AppError> {
    let mut trust_server_certificate = false;
    let mut trust_server_certificate_ca = false;
    let mut start = jdbc_url.find(';').map_or(jdbc_url.len(), |index| index + 1);
    let mut in_braces = false;
    let mut chars = jdbc_url[start..].char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        match ch {
            '{' if !in_braces => in_braces = true,
            '}' if in_braces => {
                if chars.peek().is_some_and(|(_, next)| *next == '}') {
                    chars.next();
                } else {
                    in_braces = false;
                }
            }
            ';' if !in_braces => {
                record_tls_property(
                    &jdbc_url[start..start + offset],
                    &mut trust_server_certificate,
                    &mut trust_server_certificate_ca,
                );
                start += offset + 1;
                chars = jdbc_url[start..].char_indices().peekable();
            }
            _ => {}
        }
    }
    record_tls_property(
        &jdbc_url[start..],
        &mut trust_server_certificate,
        &mut trust_server_certificate_ca,
    );
    if trust_server_certificate && trust_server_certificate_ca {
        return Err(AppError::invalid(
            "invalid_sqlserver_connection",
            "trustServerCertificate and trustServerCertificateCA cannot be configured together",
        ));
    }
    Ok(())
}

fn record_tls_property(segment: &str, trust_certificate: &mut bool, trust_ca: &mut bool) {
    let key = segment
        .split_once('=')
        .map_or(segment, |(key, _)| key)
        .trim();
    if key.eq_ignore_ascii_case("trustServerCertificate") {
        *trust_certificate = true;
    } else if key.eq_ignore_ascii_case("trustServerCertificateCA") {
        *trust_ca = true;
    }
}

fn normalize_sqlserver_url(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value
        .get(..JDBC_SQLSERVER_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(JDBC_SQLSERVER_PREFIX))
    {
        return Ok(format!(
            "{JDBC_SQLSERVER_PREFIX}{}",
            &value[JDBC_SQLSERVER_PREFIX.len()..]
        ));
    }
    if value
        .get(..NATIVE_SQLSERVER_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(NATIVE_SQLSERVER_PREFIX))
    {
        return Ok(format!(
            "{JDBC_SQLSERVER_PREFIX}{}",
            &value[NATIVE_SQLSERVER_PREFIX.len()..]
        ));
    }
    Err(AppError::invalid(
        "invalid_sqlserver_connection",
        "A valid jdbc:sqlserver:// or sqlserver:// connection URL is required",
    ))
}

fn encode_jdbc_property(value: &str) -> String {
    if value.contains([';', '=', '{', '}']) || value.starts_with(' ') || value.ends_with(' ') {
        format!("{{{}}}", value.replace('}', "}}"))
    } else {
        value.to_owned()
    }
}

fn sqlserver_target(value: &str) -> Result<(String, u16), AppError> {
    let normalized = normalize_sqlserver_url(value)?;
    let authority = normalized
        .strip_prefix("jdbc:sqlserver://")
        .expect("normalization establishes the SQL Server prefix")
        .split(';')
        .next()
        .unwrap_or_default();
    if authority.is_empty() || authority.contains('\\') {
        return Err(AppError::invalid(
            "invalid_sqlserver_ssh_url",
            "SQL Server SSH forwarding requires an explicit host and TCP port",
        ));
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, port) = rest
            .split_once("]: ")
            .or_else(|| rest.split_once("]:"))
            .ok_or_else(invalid_sqlserver_ssh_url)?;
        return Ok((host.to_owned(), parse_sqlserver_port(port)?));
    }
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(invalid_sqlserver_ssh_url)?;
    if host.trim().is_empty() || host.contains(':') {
        return Err(invalid_sqlserver_ssh_url());
    }
    Ok((host.to_owned(), parse_sqlserver_port(port)?))
}

fn parse_sqlserver_port(value: &str) -> Result<u16, AppError> {
    value
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(invalid_sqlserver_ssh_url)
}

fn invalid_sqlserver_ssh_url() -> AppError {
    AppError::invalid(
        "invalid_sqlserver_ssh_url",
        "SQL Server SSH forwarding requires an explicit host and TCP port",
    )
}

fn sqlserver_connection_failed() -> AppError {
    AppError::unavailable(
        "sqlserver_connection_failed",
        "The SQL Server instance could not be reached or rejected the connection",
    )
}

fn sqlserver_driver_failure() -> AppError {
    AppError::unavailable(
        "sqlserver_driver_failure",
        "The native SQL Server driver could not decode the server response safely",
    )
}

fn sqlserver_query_error(error: impl std::fmt::Display) -> AppError {
    AppError::invalid("sqlserver_query_failed", error.to_string())
}

fn metadata_timeout() -> AppError {
    AppError::unavailable(
        "sqlserver_metadata_timeout",
        "The SQL Server metadata query did not finish in time",
    )
}

fn validate_identifier(value: &str, field: &str) -> Result<(), AppError> {
    if value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES || value.contains('\0') {
        return Err(AppError::invalid(
            "invalid_sqlserver_metadata_request",
            format!("{field} is invalid"),
        ));
    }
    Ok(())
}

fn quote_identifier(value: &str, field: &str) -> Result<String, AppError> {
    validate_identifier(value, field)?;
    Ok(format!("[{}]", value.replace(']', "]]")))
}

fn qualified_table(
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> Result<String, AppError> {
    Ok(format!(
        "{}.{}.{}",
        quote_identifier(database_name, "databaseName")?,
        quote_identifier(schema_name, "schemaName")?,
        quote_identifier(table_name, "tableName")?
    ))
}

fn is_read_candidate(sql: &str) -> Result<bool, AppError> {
    Ok(matches!(
        sql_lexemes(sql)?.words.first().map(String::as_str),
        Some("SELECT" | "WITH")
    ))
}

fn validate_query(query: &PreparedQuery) -> Result<(), AppError> {
    if query.sql.len() > MAX_SQL_BYTES {
        return Err(AppError::invalid(
            "invalid_query_request",
            format!("SQL cannot exceed {MAX_SQL_BYTES} UTF-8 bytes"),
        ));
    }
    validate_query_options(query.options)?;
    ordered_parameters(&query.parameters)?;
    validate_read_sql(&query.sql)
}

fn validate_read_sql(sql: &str) -> Result<(), AppError> {
    let lexemes = sql_lexemes(sql)?;
    if !matches!(
        lexemes.words.first().map(String::as_str),
        Some("SELECT" | "WITH")
    ) {
        return Err(AppError::invalid(
            "sqlserver_native_query_unsupported",
            "Native SQL Server read queries must be a single SELECT statement",
        ));
    }
    if lexemes.words.iter().any(|word| {
        matches!(
            word.as_str(),
            "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "INTO"
        )
    }) {
        return Err(AppError::invalid(
            "sqlserver_native_query_unsupported",
            "Data-changing SQL is not allowed on the native SQL Server read path",
        ));
    }
    let statements = Parser::parse_sql(&MsSqlDialect {}, sql).map_err(|_| {
        AppError::invalid(
            "sqlserver_native_query_unsupported",
            "The SQL Server read query could not be parsed safely",
        )
    })?;
    let [Statement::Query(query)] = statements.as_slice() else {
        return Err(AppError::invalid(
            "sqlserver_native_query_unsupported",
            "Native SQL Server read queries must contain exactly one SELECT statement",
        ));
    };
    if !query_tree_is_read_only(query) {
        return Err(AppError::invalid(
            "sqlserver_native_query_unsupported",
            "Data-changing CTEs are not allowed on the native SQL Server read path",
        ));
    }
    Ok(())
}

fn query_tree_is_read_only(query: &SqlQuery) -> bool {
    query.with.as_ref().is_none_or(|with| {
        with.cte_tables
            .iter()
            .all(|cte| query_tree_is_read_only(&cte.query))
    }) && set_expr_is_read_only(&query.body)
}

fn set_expr_is_read_only(expression: &SetExpr) -> bool {
    match expression {
        SetExpr::Select(_) => true,
        SetExpr::Query(query) => query_tree_is_read_only(query),
        SetExpr::SetOperation { left, right, .. } => {
            set_expr_is_read_only(left) && set_expr_is_read_only(right)
        }
        SetExpr::Insert(_)
        | SetExpr::Update(_)
        | SetExpr::Delete(_)
        | SetExpr::Merge(_)
        | SetExpr::Values(_)
        | SetExpr::Table(_) => false,
    }
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

fn ordered_parameters(parameters: &[QueryParameter]) -> Result<Vec<&DatabaseValue>, AppError> {
    if parameters.len() > MAX_PARAMETERS {
        return Err(AppError::invalid(
            "invalid_query_parameter_count",
            format!("SQL Server queries accept at most {MAX_PARAMETERS} parameters"),
        ));
    }
    let mut ordered = parameters.iter().collect::<Vec<_>>();
    ordered.sort_unstable_by_key(|parameter| parameter.position);
    for (index, parameter) in ordered.iter().enumerate() {
        let expected = u32::try_from(index + 1).map_err(|_| AppError::internal())?;
        if parameter.position != expected {
            return Err(AppError::invalid(
                "invalid_query_parameter",
                "SQL Server parameter positions must be unique and contiguous from 1",
            ));
        }
        validate_parameter(&parameter.value)?;
    }
    Ok(ordered
        .into_iter()
        .map(|parameter| &parameter.value)
        .collect())
}

fn validate_parameter(value: &DatabaseValue) -> Result<(), AppError> {
    let length = match value {
        DatabaseValue::Decimal(value)
        | DatabaseValue::Text(value)
        | DatabaseValue::Date(value)
        | DatabaseValue::Time(value)
        | DatabaseValue::Timestamp(value)
        | DatabaseValue::TimestampWithTimeZone(value)
        | DatabaseValue::Json(value)
        | DatabaseValue::Uuid(value) => value.len(),
        DatabaseValue::Binary(value) => value.len(),
        _ => 0,
    };
    if length > MAX_SCALAR_BYTES {
        return Err(AppError::invalid(
            "invalid_query_parameter",
            format!("A SQL Server parameter exceeds {MAX_SCALAR_BYTES} bytes"),
        ));
    }
    match value {
        DatabaseValue::UnsignedInteger(value) if *value > i64::MAX as u64 => {
            Err(AppError::invalid(
                "invalid_query_parameter",
                "SQL Server does not support unsigned integers larger than BIGINT",
            ))
        }
        DatabaseValue::Decimal(value) => parse_decimal(value).map(|_| ()),
        DatabaseValue::Date(value) => NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map(|_| ())
            .map_err(|_| invalid_temporal_parameter("date")),
        DatabaseValue::Time(value) => NaiveTime::parse_from_str(value, "%H:%M:%S%.f")
            .map(|_| ())
            .map_err(|_| invalid_temporal_parameter("time")),
        DatabaseValue::Timestamp(value) => parse_timestamp(value).map(|_| ()),
        DatabaseValue::TimestampWithTimeZone(value) => DateTime::parse_from_rfc3339(value)
            .map(|_| ())
            .map_err(|_| invalid_temporal_parameter("timestamp with time zone")),
        DatabaseValue::Uuid(value) => tiberius::Uuid::parse_str(value).map(|_| ()).map_err(|_| {
            AppError::invalid(
                "invalid_query_parameter",
                "The SQL Server UUID parameter is invalid",
            )
        }),
        _ => Ok(()),
    }
}

fn parse_timestamp(value: &str) -> Result<NaiveDateTime, AppError> {
    ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"]
        .into_iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
        .ok_or_else(|| invalid_temporal_parameter("timestamp"))
}

fn invalid_temporal_parameter(label: &str) -> AppError {
    AppError::invalid(
        "invalid_query_parameter",
        format!("The SQL Server {label} parameter is invalid"),
    )
}

fn parse_decimal(value: &str) -> Result<Numeric, AppError> {
    let (negative, unsigned) = value
        .strip_prefix('-')
        .map_or((false, value), |value| (true, value));
    let unsigned = unsigned.strip_prefix('+').unwrap_or(unsigned);
    let (whole, fraction) = unsigned
        .split_once('.')
        .map_or((unsigned, ""), |(whole, fraction)| (whole, fraction));
    if (whole.is_empty() && fraction.is_empty())
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() >= 38
    {
        return Err(AppError::invalid(
            "invalid_query_parameter",
            "The SQL Server decimal parameter is invalid",
        ));
    }
    let digits = format!("{whole}{fraction}");
    if digits.len() > 38 {
        return Err(AppError::invalid(
            "invalid_query_parameter",
            "SQL Server decimal parameters cannot exceed 38 digits",
        ));
    }
    let mut integer = digits.parse::<i128>().map_err(|_| {
        AppError::invalid(
            "invalid_query_parameter",
            "The SQL Server decimal parameter is invalid",
        )
    })?;
    if negative {
        integer = -integer;
    }
    Ok(Numeric::new_with_scale(
        integer,
        u8::try_from(fraction.len()).map_err(|_| AppError::internal())?,
    ))
}

fn bind_query(sql: &str, parameters: &[QueryParameter]) -> Result<Query<'static>, AppError> {
    let ordered = ordered_parameters(parameters)?;
    let sql = rewrite_positional_parameters(sql, ordered.len())?;
    let mut query = Query::new(sql);
    for value in ordered {
        match value {
            DatabaseValue::Null => query.bind(Option::<String>::None),
            DatabaseValue::Boolean(value) => query.bind(*value),
            DatabaseValue::SignedInteger(value) => query.bind(*value),
            DatabaseValue::UnsignedInteger(value) => {
                query.bind(i64::try_from(*value).map_err(|_| {
                    AppError::invalid(
                        "invalid_query_parameter",
                        "SQL Server does not support unsigned integers larger than BIGINT",
                    )
                })?);
            }
            DatabaseValue::Float32(value) => query.bind(*value),
            DatabaseValue::Float64(value) => query.bind(*value),
            DatabaseValue::Decimal(value) => query.bind(parse_decimal(value)?),
            DatabaseValue::Text(value) | DatabaseValue::Json(value) => query.bind(value.clone()),
            DatabaseValue::Binary(value) => query.bind(value.clone()),
            DatabaseValue::Date(value) => query.bind(
                NaiveDate::parse_from_str(value, "%Y-%m-%d")
                    .map_err(|_| invalid_temporal_parameter("date"))?,
            ),
            DatabaseValue::Time(value) => query.bind(
                NaiveTime::parse_from_str(value, "%H:%M:%S%.f")
                    .map_err(|_| invalid_temporal_parameter("time"))?,
            ),
            DatabaseValue::Timestamp(value) => query.bind(parse_timestamp(value)?),
            DatabaseValue::TimestampWithTimeZone(value) => query.bind(
                DateTime::<FixedOffset>::parse_from_rfc3339(value)
                    .map_err(|_| invalid_temporal_parameter("timestamp with time zone"))?,
            ),
            DatabaseValue::Uuid(value) => {
                query.bind(tiberius::Uuid::parse_str(value).map_err(|_| {
                    AppError::invalid(
                        "invalid_query_parameter",
                        "The SQL Server UUID parameter is invalid",
                    )
                })?);
            }
        }
    }
    Ok(query)
}

fn described_query(
    sql: &str,
    parameters: &[QueryParameter],
) -> Result<(String, Option<String>), AppError> {
    let ordered = ordered_parameters(parameters)?;
    let sql = rewrite_positional_parameters(sql, ordered.len())?;
    if ordered.is_empty() {
        return Ok((sql, None));
    }
    let declarations = ordered
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            Ok(format!(
                "@P{} {}",
                index + 1,
                sqlserver_parameter_declaration(value)?
            ))
        })
        .collect::<Result<Vec<_>, AppError>>()?
        .join(", ");
    Ok((sql, Some(declarations)))
}

fn sqlserver_parameter_declaration(value: &DatabaseValue) -> Result<String, AppError> {
    Ok(match value {
        DatabaseValue::Null | DatabaseValue::Text(_) | DatabaseValue::Json(_) => {
            "nvarchar(max)".to_owned()
        }
        DatabaseValue::Boolean(_) => "bit".to_owned(),
        DatabaseValue::SignedInteger(_) | DatabaseValue::UnsignedInteger(_) => "bigint".to_owned(),
        DatabaseValue::Float32(_) => "real".to_owned(),
        DatabaseValue::Float64(_) => "float".to_owned(),
        DatabaseValue::Decimal(value) => {
            format!("decimal(38,{})", parse_decimal(value)?.scale())
        }
        DatabaseValue::Binary(_) => "varbinary(max)".to_owned(),
        DatabaseValue::Date(_) => "date".to_owned(),
        DatabaseValue::Time(_) => "time(7)".to_owned(),
        DatabaseValue::Timestamp(_) => "datetime2(7)".to_owned(),
        DatabaseValue::TimestampWithTimeZone(_) => "datetimeoffset(7)".to_owned(),
        DatabaseValue::Uuid(_) => "uniqueidentifier".to_owned(),
    })
}

async fn validate_result_set(
    client: &mut SqlServerClient,
    sql: &str,
    parameter_declarations: Option<&str>,
) -> Result<(), AppError> {
    let mut query = Query::new(
        "SELECT CONVERT(int, system_type_id) AS system_type_id, system_type_name, \
         user_type_name, CONVERT(bigint, max_length) AS max_length, error_number, error_message \
         FROM sys.dm_exec_describe_first_result_set(@P1, @P2, 0) \
         ORDER BY column_ordinal",
    );
    query.bind(sql.to_owned());
    query.bind(parameter_declarations.map(str::to_owned));
    let stream = AssertUnwindSafe(query.query(client))
        .catch_unwind()
        .await
        .map_err(|_| sqlserver_driver_failure())?
        .map_err(sqlserver_query_error)?;
    let rows = AssertUnwindSafe(stream.into_first_result())
        .catch_unwind()
        .await
        .map_err(|_| sqlserver_driver_failure())?
        .map_err(sqlserver_query_error)?;
    for row in rows {
        if row
            .try_get::<i32, _>(4)
            .map_err(sqlserver_query_error)?
            .is_some()
        {
            return Err(AppError::invalid(
                "sqlserver_result_description_failed",
                "SQL Server could not describe the statement result safely",
            ));
        }
        let system_type_id = row.try_get::<i32, _>(0).map_err(sqlserver_query_error)?;
        let system_type_name = row
            .try_get::<&str, _>(1)
            .map_err(sqlserver_query_error)?
            .unwrap_or_default();
        let user_type_name = row
            .try_get::<&str, _>(2)
            .map_err(sqlserver_query_error)?
            .unwrap_or_default();
        validate_described_result_type(system_type_id, system_type_name, user_type_name)?;
        if row
            .try_get::<i64, _>(3)
            .map_err(sqlserver_query_error)?
            .is_some_and(|length| length > i64::try_from(MAX_SCALAR_BYTES).unwrap_or(i64::MAX))
        {
            return Err(resource_error(
                "sqlserver_scalar_too_large",
                format!("A SQL Server scalar cannot exceed {MAX_SCALAR_BYTES} bytes"),
            ));
        }
    }
    Ok(())
}

enum ControlledResultSetValidationError {
    Cancelled(Option<String>),
    TimedOut(AppError),
    Failed(AppError),
}

async fn validate_result_set_with_control(
    client: &mut SqlServerClient,
    sql: &str,
    parameter_declarations: Option<&str>,
    cancellation: &mut watch::Receiver<CancellationRequest>,
) -> Result<(), ControlledResultSetValidationError> {
    let validation = validate_result_set(client, sql, parameter_declarations);
    tokio::pin!(validation);
    let deadline = tokio::time::sleep(METADATA_TIMEOUT);
    tokio::pin!(deadline);
    let mut cancellation_open = true;
    loop {
        tokio::select! {
            biased;
            changed = cancellation.changed(), if cancellation_open => {
                if changed.is_err() {
                    cancellation_open = false;
                    continue;
                }
                if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
                    return Err(ControlledResultSetValidationError::Cancelled(reason));
                }
            }
            () = &mut deadline => {
                return Err(ControlledResultSetValidationError::TimedOut(
                    AppError::unavailable(
                        "sqlserver_result_description_timeout",
                        "SQL Server did not describe the statement result before the deadline",
                    ),
                ));
            }
            result = &mut validation => {
                return result.map_err(ControlledResultSetValidationError::Failed);
            }
        }
    }
}

fn validate_described_result_type(
    system_type_id: Option<i32>,
    system_type_name: &str,
    user_type_name: &str,
) -> Result<(), AppError> {
    let normalized = system_type_name.trim().to_ascii_lowercase();
    if system_type_id == Some(240)
        || matches!(normalized.as_str(), "money" | "smallmoney" | "sql_variant")
    {
        let type_name = if user_type_name.trim().is_empty() {
            system_type_name
        } else {
            user_type_name
        };
        return Err(unsupported_result_type(type_name));
    }
    Ok(())
}

fn unsupported_result_type(type_name: &str) -> AppError {
    AppError::invalid(
        "sqlserver_result_type_unsupported",
        format!("The native SQL Server driver cannot safely decode result type {type_name}"),
    )
}

fn rewrite_positional_parameters(sql: &str, parameter_count: usize) -> Result<String, AppError> {
    let mut output = String::with_capacity(sql.len() + parameter_count.saturating_mul(3));
    let mut chars = sql.char_indices().peekable();
    let mut replaced = 0_usize;
    while let Some((_, ch)) = chars.next() {
        match ch {
            '\'' => copy_quoted(&mut output, &mut chars, '\'', '\''),
            '"' => copy_quoted(&mut output, &mut chars, '"', '"'),
            '[' => copy_quoted(&mut output, &mut chars, '[', ']'),
            '-' if chars.peek().is_some_and(|(_, next)| *next == '-') => {
                output.push('-');
                output.push('-');
                chars.next();
                for (_, next) in chars.by_ref() {
                    output.push(next);
                    if next == '\n' {
                        break;
                    }
                }
            }
            '/' if chars.peek().is_some_and(|(_, next)| *next == '*') => {
                output.push('/');
                output.push('*');
                chars.next();
                let mut previous = '\0';
                for (_, next) in chars.by_ref() {
                    output.push(next);
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
            }
            '?' => {
                replaced += 1;
                output.push_str("@P");
                output.push_str(&replaced.to_string());
            }
            _ => output.push(ch),
        }
    }
    if replaced != 0 && replaced != parameter_count {
        return Err(AppError::invalid(
            "invalid_query_parameter_count",
            format!(
                "The SQL Server statement has {replaced} positional markers but {parameter_count} parameters were supplied"
            ),
        ));
    }
    Ok(output)
}

fn copy_quoted<I>(
    output: &mut String,
    chars: &mut std::iter::Peekable<I>,
    opening: char,
    closing: char,
) where
    I: Iterator<Item = (usize, char)>,
{
    output.push(opening);
    while let Some((_, ch)) = chars.next() {
        output.push(ch);
        if ch == closing {
            if chars.peek().is_some_and(|(_, next)| *next == closing) {
                output.push(closing);
                chars.next();
            } else {
                break;
            }
        }
    }
}

struct SqlLexemes {
    words: Vec<String>,
}

fn sql_lexemes(sql: &str) -> Result<SqlLexemes, AppError> {
    if sql.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_query_request",
            "SQL cannot be empty",
        ));
    }
    let mut words = Vec::new();
    let mut word = String::new();
    let mut chars = sql.chars().peekable();
    let mut statement_terminators = 0_usize;
    while let Some(ch) = chars.next() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            word.push(ch.to_ascii_uppercase());
            continue;
        }
        if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        match ch {
            '\'' | '"' => skip_quoted_chars(&mut chars, ch),
            '[' => skip_quoted_chars(&mut chars, ']'),
            '-' if chars.peek().is_some_and(|next| *next == '-') => {
                chars.next();
                for next in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
            }
            '/' if chars.peek().is_some_and(|next| *next == '*') => {
                chars.next();
                let mut previous = '\0';
                let mut closed = false;
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        closed = true;
                        break;
                    }
                    previous = next;
                }
                if !closed {
                    return Err(AppError::invalid(
                        "sqlserver_native_query_unsupported",
                        "The SQL Server statement contains an unterminated comment",
                    ));
                }
            }
            ';' => statement_terminators += 1,
            _ => {}
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    if statement_terminators > 1 {
        return Err(AppError::invalid(
            "sqlserver_native_query_unsupported",
            "Native SQL Server accepts exactly one statement",
        ));
    }
    Ok(SqlLexemes { words })
}

fn skip_quoted_chars<I>(chars: &mut std::iter::Peekable<I>, closing: char)
where
    I: Iterator<Item = char>,
{
    while let Some(ch) = chars.next() {
        if ch == closing {
            if chars.peek().is_some_and(|next| *next == closing) {
                chars.next();
            } else {
                break;
            }
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the query stream owns one connection, cancellation session, and retained writer lifecycle"
)]
async fn execute_query_task(
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
    validate_query(&query)?;
    let (described_sql, parameter_declarations) = described_query(&query.sql, &query.parameters)?;
    let bound = bind_query(&query.sql, &query.parameters)?;
    let open = open_resolved_connection(&resolved, None);
    tokio::pin!(open);
    let mut conn = loop {
        tokio::select! {
            biased;
            changed = cancellation.changed() => {
                if changed.is_ok()
                    && let CancellationRequest::Requested { reason } = cancellation.borrow().clone()
                {
                    return Err(QueryTaskError::Cancelled(reason));
                }
            }
            result = &mut open => break result?,
        }
    };
    let validation = validate_result_set_with_control(
        &mut conn.client,
        &described_sql,
        parameter_declarations.as_deref(),
        &mut cancellation,
    )
    .await;
    match validation {
        Ok(()) => {}
        Err(ControlledResultSetValidationError::Cancelled(reason)) => {
            discard_connection(conn).await;
            return Err(QueryTaskError::Cancelled(reason));
        }
        Err(
            ControlledResultSetValidationError::TimedOut(error)
            | ControlledResultSetValidationError::Failed(error),
        ) => {
            discard_connection(conn).await;
            return Err(error.into());
        }
    }
    let cancellation_request = cancellation.borrow().clone();
    if let CancellationRequest::Requested { reason } = cancellation_request {
        discard_connection(conn).await;
        return Err(QueryTaskError::Cancelled(reason));
    }
    let opened: Result<_, QueryOpenError> = {
        let query_open = bound.query(&mut conn.client);
        tokio::pin!(query_open);
        loop {
            tokio::select! {
                biased;
                changed = cancellation.changed() => {
                    if changed.is_ok()
                        && let CancellationRequest::Requested { reason } = cancellation.borrow().clone()
                    {
                        break Err(QueryOpenError::Cancelled(reason));
                    }
                }
                result = &mut query_open => {
                    break result.map_err(sqlserver_query_error).map_err(QueryOpenError::Failed);
                }
            }
        }
    };
    let mut stream = opened.map_err(|error| match error {
        QueryOpenError::Failed(error) => QueryTaskError::Failed(error),
        QueryOpenError::Cancelled(reason) => QueryTaskError::Cancelled(reason),
    })?;

    let first = next_query_item(&mut stream, &mut cancellation).await;
    let metadata = match first {
        Ok(Some(QueryItem::Metadata(metadata))) => metadata,
        Ok(Some(QueryItem::Row(_)) | None) => {
            drop(stream);
            discard_connection(conn).await;
            return Err(AppError::invalid(
                "sqlserver_query_has_no_result_set",
                "The SQL Server read query did not return a tabular result set",
            )
            .into());
        }
        Err(QueryTaskError::Cancelled(reason)) => {
            drop(stream);
            discard_connection(conn).await;
            return Err(QueryTaskError::Cancelled(reason));
        }
        Err(error) => {
            drop(stream);
            discard_connection(conn).await;
            return Err(error);
        }
    };
    let columns = metadata.columns().to_vec();
    if columns.len() > MAX_COLUMNS {
        drop(stream);
        discard_connection(conn).await;
        return Err(resource_error(
            "sqlserver_result_too_wide",
            format!("SQL Server returned more than {MAX_COLUMNS} columns"),
        )
        .into());
    }
    let schema_columns = columns
        .iter()
        .enumerate()
        .map(|(index, column)| wire_column(index, column))
        .collect::<Result<Vec<_>, _>>();
    let schema = match schema_columns {
        Ok(columns) => wire::QueryStarted { columns },
        Err(error) => {
            drop(stream);
            discard_connection(conn).await;
            return Err(error.into());
        }
    };
    let mut writer = match RetainedWriter::begin(storage, schema, query.retention).await {
        Ok(writer) => writer,
        Err(error) => {
            drop(stream);
            discard_connection(conn).await;
            return Err(error.into());
        }
    };
    if let Err(error) = application.inner.operations.started(operation_id).await {
        drop(stream);
        abort_writer(&mut writer).await;
        discard_connection(conn).await;
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
    let mut pending_rows = Vec::new();
    let mut pending_bytes = 0_u64;
    let mut row_count = 0_u64;
    let mut result_bytes = 0_u64;
    let mut requires_discard = false;
    let consumption: Result<wire::QueryCompleted, QueryTaskError> = async {
        loop {
            let item = next_query_item(&mut stream, &mut cancellation).await?;
            let Some(item) = item else {
                return Ok(wire::QueryCompleted {
                    row_count,
                    truncated_by_max_rows: false,
                    truncated_by_max_result_bytes: false,
                });
            };
            let QueryItem::Row(row) = item else {
                return Err(AppError::invalid(
                    "sqlserver_query_multiple_results",
                    "Native SQL Server retained queries accept one result set",
                )
                .into());
            };
            if max_rows != 0 && row_count >= max_rows {
                requires_discard = true;
                return Ok(wire::QueryCompleted {
                    row_count,
                    truncated_by_max_rows: true,
                    truncated_by_max_result_bytes: false,
                });
            }
            let row = wire_row(row)?;
            let row_bytes = u64::try_from(row.encoded_len())
                .map_err(|_| QueryTaskError::Failed(AppError::internal()))?;
            if result_bytes.saturating_add(row_bytes) > max_result_bytes {
                requires_discard = true;
                return Ok(wire::QueryCompleted {
                    row_count,
                    truncated_by_max_rows: false,
                    truncated_by_max_result_bytes: true,
                });
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
                && (pending_rows.len() >= usize::try_from(batch_rows).unwrap_or(usize::MAX)
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
    drop(stream);

    let completion = match consumption {
        Ok(completion) => completion,
        Err(error) => {
            abort_writer(&mut writer).await;
            discard_connection(conn).await;
            return Err(error);
        }
    };
    if requires_discard {
        discard_connection(conn).await;
    } else if let Err(error) = finish_connection(conn, Ok(())).await {
        abort_writer(&mut writer).await;
        return Err(error.into());
    }
    if let Err(error) = flush_rows(
        application,
        operation_id,
        &mut writer,
        &mut pending_rows,
        row_count,
    )
    .await
    {
        abort_writer(&mut writer).await;
        return Err(error);
    }
    let metadata = match writer.finish(completion).await {
        Ok(metadata) => metadata,
        Err(error) => {
            abort_writer(&mut writer).await;
            return Err(error.into());
        }
    };
    Ok(metadata)
}

async fn next_query_item(
    stream: &mut tiberius::QueryStream<'_>,
    cancellation: &mut watch::Receiver<CancellationRequest>,
) -> Result<Option<QueryItem>, QueryTaskError> {
    let mut cancellation_open = true;
    loop {
        tokio::select! {
            biased;
            changed = cancellation.changed(), if cancellation_open => {
                if changed.is_err() {
                    cancellation_open = false;
                    continue;
                }
                if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
                    return Err(QueryTaskError::Cancelled(reason));
                }
            }
            item = AssertUnwindSafe(stream.try_next()).catch_unwind() => {
                return match item {
                    Ok(item) => item.map_err(sqlserver_query_error).map_err(QueryTaskError::from),
                    Err(_) => Err(QueryTaskError::Failed(sqlserver_driver_failure())),
                };
            }
        }
    }
}

fn wire_column(index: usize, column: &Column) -> Result<wire::JdbcColumn, AppError> {
    validate_supported_column_type(column.column_type())?;
    let ordinal = u32::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .ok_or_else(AppError::internal)?;
    let value_type = sqlserver_value_type(column.column_type());
    Ok(wire::JdbcColumn {
        ordinal,
        name: column.name().to_owned(),
        label: column.name().to_owned(),
        jdbc_type: sqlserver_jdbc_type(column.column_type()),
        jdbc_type_name: sqlserver_type_name(column.column_type()).to_owned(),
        value_type: value_type as i32,
        nullability: wire::ColumnNullability::Unknown as i32,
        precision: None,
        scale: None,
        display_size: None,
        signed: sqlserver_numeric_type(column.column_type()).then_some(true),
        catalog_name: None,
        schema_name: None,
        table_name: None,
    })
}

fn wire_row(row: Row) -> Result<wire::JdbcRow, AppError> {
    Ok(wire::JdbcRow {
        values: row
            .into_iter()
            .map(wire_value)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn wire_value(value: ColumnData<'static>) -> Result<wire::JdbcValue, AppError> {
    use wire::jdbc_value::Value as WireValue;
    let value = match value {
        ColumnData::U8(None)
        | ColumnData::I16(None)
        | ColumnData::I32(None)
        | ColumnData::I64(None)
        | ColumnData::F32(None)
        | ColumnData::F64(None)
        | ColumnData::Bit(None)
        | ColumnData::String(None)
        | ColumnData::Guid(None)
        | ColumnData::Binary(None)
        | ColumnData::Numeric(None)
        | ColumnData::Xml(None)
        | ColumnData::DateTime(None)
        | ColumnData::SmallDateTime(None)
        | ColumnData::Time(None)
        | ColumnData::Date(None)
        | ColumnData::DateTime2(None)
        | ColumnData::DateTimeOffset(None) => WireValue::NullValue(wire::JdbcNull {}),
        ColumnData::U8(Some(value)) => WireValue::UnsignedIntegerValue(u64::from(value)),
        ColumnData::I16(Some(value)) => WireValue::SignedIntegerValue(i64::from(value)),
        ColumnData::I32(Some(value)) => WireValue::SignedIntegerValue(i64::from(value)),
        ColumnData::I64(Some(value)) => WireValue::SignedIntegerValue(value),
        ColumnData::F32(Some(value)) => WireValue::Float32Value(value),
        ColumnData::F64(Some(value)) => WireValue::Float64Value(value),
        ColumnData::Bit(Some(value)) => WireValue::BooleanValue(value),
        ColumnData::String(Some(value)) => {
            let value = value.into_owned();
            validate_scalar_bytes(value.len())?;
            WireValue::TextValue(value)
        }
        ColumnData::Guid(Some(value)) => WireValue::UuidValue(value.to_string()),
        ColumnData::Binary(Some(value)) => {
            let value = value.into_owned();
            validate_scalar_bytes(value.len())?;
            WireValue::BinaryValue(value)
        }
        ColumnData::Numeric(Some(value)) => WireValue::DecimalValue(format_numeric(value)),
        ColumnData::Xml(Some(value)) => {
            let display_value = value.to_string();
            validate_scalar_bytes(display_value.len())?;
            WireValue::OpaqueValue(wire::OpaqueValue {
                type_name: "xml".to_owned(),
                display_value,
            })
        }
        ColumnData::DateTime(Some(value)) => {
            WireValue::TimestampValue(format_datetime(value.days(), value.seconds_fragments())?)
        }
        ColumnData::SmallDateTime(Some(value)) => WireValue::TimestampValue(format_small_datetime(
            value.days(),
            value.seconds_fragments(),
        )?),
        ColumnData::Time(Some(value)) => WireValue::TimeValue(format_time(value)?),
        ColumnData::Date(Some(value)) => WireValue::DateValue(format_date(value)?),
        ColumnData::DateTime2(Some(value)) => WireValue::TimestampValue(format_datetime2(value)?),
        ColumnData::DateTimeOffset(Some(value)) => {
            WireValue::TimestampWithTimeZoneValue(format_datetime_offset(value)?)
        }
    };
    Ok(wire::JdbcValue { value: Some(value) })
}

fn validate_scalar_bytes(length: usize) -> Result<(), AppError> {
    if length > MAX_SCALAR_BYTES {
        return Err(resource_error(
            "sqlserver_scalar_too_large",
            format!("A SQL Server scalar cannot exceed {MAX_SCALAR_BYTES} bytes"),
        ));
    }
    Ok(())
}

fn validate_supported_column_type(column_type: ColumnType) -> Result<(), AppError> {
    if matches!(
        column_type,
        ColumnType::Money | ColumnType::Money4 | ColumnType::Udt | ColumnType::SSVariant
    ) {
        return Err(unsupported_result_type(sqlserver_type_name(column_type)));
    }
    Ok(())
}

fn sqlserver_value_type(column_type: ColumnType) -> wire::JdbcValueType {
    match column_type {
        ColumnType::Bit | ColumnType::Bitn => wire::JdbcValueType::Boolean,
        ColumnType::Int1 => wire::JdbcValueType::UnsignedInteger,
        ColumnType::Int2 | ColumnType::Int4 | ColumnType::Int8 | ColumnType::Intn => {
            wire::JdbcValueType::SignedInteger
        }
        ColumnType::Float4 | ColumnType::Money4 => wire::JdbcValueType::Float32,
        ColumnType::Float8 | ColumnType::Floatn | ColumnType::Money => wire::JdbcValueType::Float64,
        ColumnType::Decimaln | ColumnType::Numericn => wire::JdbcValueType::Decimal,
        ColumnType::Guid => wire::JdbcValueType::Uuid,
        ColumnType::BigVarBin | ColumnType::BigBinary | ColumnType::Image | ColumnType::Udt => {
            wire::JdbcValueType::Binary
        }
        ColumnType::Daten => wire::JdbcValueType::Date,
        ColumnType::Timen => wire::JdbcValueType::Time,
        ColumnType::Datetime4
        | ColumnType::Datetime
        | ColumnType::Datetimen
        | ColumnType::Datetime2 => wire::JdbcValueType::Timestamp,
        ColumnType::DatetimeOffsetn => wire::JdbcValueType::TimestampWithTimeZone,
        ColumnType::Null => wire::JdbcValueType::Unspecified,
        ColumnType::Xml | ColumnType::SSVariant => wire::JdbcValueType::Opaque,
        ColumnType::BigVarChar
        | ColumnType::BigChar
        | ColumnType::NVarchar
        | ColumnType::NChar
        | ColumnType::Text
        | ColumnType::NText => wire::JdbcValueType::Text,
    }
}

fn sqlserver_jdbc_type(column_type: ColumnType) -> i32 {
    match column_type {
        ColumnType::Bit | ColumnType::Bitn => -7,
        ColumnType::Int1 => -6,
        ColumnType::Int2 => 5,
        ColumnType::Int4 | ColumnType::Intn => 4,
        ColumnType::Int8 => -5,
        ColumnType::Float4 => 7,
        ColumnType::Float8 | ColumnType::Floatn => 8,
        ColumnType::Money | ColumnType::Money4 | ColumnType::Decimaln | ColumnType::Numericn => 3,
        ColumnType::Guid => -11,
        ColumnType::BigVarBin => -3,
        ColumnType::BigBinary => -2,
        ColumnType::Image => -4,
        ColumnType::Daten => 91,
        ColumnType::Timen => 92,
        ColumnType::Datetime4
        | ColumnType::Datetime
        | ColumnType::Datetimen
        | ColumnType::Datetime2 => 93,
        ColumnType::DatetimeOffsetn => 2014,
        ColumnType::BigVarChar => 12,
        ColumnType::BigChar => 1,
        ColumnType::NVarchar => -9,
        ColumnType::NChar => -15,
        ColumnType::Text => -1,
        ColumnType::NText => -16,
        ColumnType::Xml => 2009,
        ColumnType::Udt | ColumnType::SSVariant | ColumnType::Null => 1111,
    }
}

fn sqlserver_type_name(column_type: ColumnType) -> &'static str {
    match column_type {
        ColumnType::Null => "null",
        ColumnType::Bit | ColumnType::Bitn => "bit",
        ColumnType::Int1 => "tinyint",
        ColumnType::Int2 => "smallint",
        ColumnType::Int4 | ColumnType::Intn => "int",
        ColumnType::Int8 => "bigint",
        ColumnType::Datetime4 => "smalldatetime",
        ColumnType::Float4 => "real",
        ColumnType::Float8 | ColumnType::Floatn => "float",
        ColumnType::Money => "money",
        ColumnType::Datetime | ColumnType::Datetimen => "datetime",
        ColumnType::Money4 => "smallmoney",
        ColumnType::Guid => "uniqueidentifier",
        ColumnType::Decimaln => "decimal",
        ColumnType::Numericn => "numeric",
        ColumnType::Daten => "date",
        ColumnType::Timen => "time",
        ColumnType::Datetime2 => "datetime2",
        ColumnType::DatetimeOffsetn => "datetimeoffset",
        ColumnType::BigVarBin => "varbinary",
        ColumnType::BigVarChar => "varchar",
        ColumnType::BigBinary => "binary",
        ColumnType::BigChar => "char",
        ColumnType::NVarchar => "nvarchar",
        ColumnType::NChar => "nchar",
        ColumnType::Xml => "xml",
        ColumnType::Udt => "udt",
        ColumnType::Text => "text",
        ColumnType::Image => "image",
        ColumnType::NText => "ntext",
        ColumnType::SSVariant => "sql_variant",
    }
}

fn sqlserver_numeric_type(column_type: ColumnType) -> bool {
    matches!(
        column_type,
        ColumnType::Int1
            | ColumnType::Int2
            | ColumnType::Int4
            | ColumnType::Int8
            | ColumnType::Intn
            | ColumnType::Float4
            | ColumnType::Float8
            | ColumnType::Floatn
            | ColumnType::Money
            | ColumnType::Money4
            | ColumnType::Decimaln
            | ColumnType::Numericn
    )
}

fn format_numeric(value: Numeric) -> String {
    let scale = usize::from(value.scale());
    if scale == 0 {
        return value.value().to_string();
    }
    let negative = value.value().is_negative();
    let digits = value.value().unsigned_abs().to_string();
    let padded = if digits.len() <= scale {
        format!("{}{}", "0".repeat(scale + 1 - digits.len()), digits)
    } else {
        digits
    };
    let split = padded.len() - scale;
    format!(
        "{}{}.{}",
        if negative { "-" } else { "" },
        &padded[..split],
        &padded[split..]
    )
}

fn format_datetime(days: i32, fragments: u32) -> Result<String, AppError> {
    let date = NaiveDate::from_ymd_opt(1900, 1, 1)
        .and_then(|date| date.checked_add_signed(chrono::Duration::days(i64::from(days))))
        .ok_or_else(AppError::internal)?;
    let nanos = u64::from(fragments) * 1_000_000_000 / 300;
    let time = NaiveTime::from_num_seconds_from_midnight_opt(
        u32::try_from(nanos / 1_000_000_000).map_err(|_| AppError::internal())?,
        u32::try_from(nanos % 1_000_000_000).map_err(|_| AppError::internal())?,
    )
    .ok_or_else(AppError::internal)?;
    Ok(format_naive_datetime(NaiveDateTime::new(date, time)))
}

fn format_small_datetime(days: u16, minutes: u16) -> Result<String, AppError> {
    let date = NaiveDate::from_ymd_opt(1900, 1, 1)
        .and_then(|date| date.checked_add_signed(chrono::Duration::days(i64::from(days))))
        .ok_or_else(AppError::internal)?;
    let time = NaiveTime::from_num_seconds_from_midnight_opt(u32::from(minutes) * 60, 0)
        .ok_or_else(AppError::internal)?;
    Ok(format_naive_datetime(NaiveDateTime::new(date, time)))
}

fn format_time(value: tiberius::time::Time) -> Result<String, AppError> {
    let nanos = value
        .increments()
        .checked_mul(10_u64.pow(u32::from(9_u8.saturating_sub(value.scale()))))
        .ok_or_else(AppError::internal)?;
    let seconds = nanos / 1_000_000_000;
    let nanos = u32::try_from(nanos % 1_000_000_000).map_err(|_| AppError::internal())?;
    NaiveTime::from_num_seconds_from_midnight_opt(
        u32::try_from(seconds).map_err(|_| AppError::internal())?,
        nanos,
    )
    .map(|time| time.format("%H:%M:%S%.f").to_string())
    .ok_or_else(AppError::internal)
}

fn format_date(value: tiberius::time::Date) -> Result<String, AppError> {
    NaiveDate::from_ymd_opt(1, 1, 1)
        .and_then(|date| date.checked_add_signed(chrono::Duration::days(i64::from(value.days()))))
        .map(|date| date.format("%Y-%m-%d").to_string())
        .ok_or_else(AppError::internal)
}

fn datetime2_as_naive(value: tiberius::time::DateTime2) -> Result<NaiveDateTime, AppError> {
    let date = NaiveDate::from_ymd_opt(1, 1, 1)
        .and_then(|date| {
            date.checked_add_signed(chrono::Duration::days(i64::from(value.date().days())))
        })
        .ok_or_else(AppError::internal)?;
    if date > NaiveDate::from_ymd_opt(9999, 12, 31).ok_or_else(AppError::internal)? {
        return Err(AppError::internal());
    }
    let time = value.time();
    let precision = 9_u32
        .checked_sub(u32::from(time.scale()))
        .ok_or_else(AppError::internal)?;
    let nanos = time
        .increments()
        .checked_mul(10_u64.pow(precision))
        .ok_or_else(AppError::internal)?;
    let seconds = u32::try_from(nanos / 1_000_000_000).map_err(|_| AppError::internal())?;
    let nanos = u32::try_from(nanos % 1_000_000_000).map_err(|_| AppError::internal())?;
    let time = NaiveTime::from_num_seconds_from_midnight_opt(seconds, nanos)
        .ok_or_else(AppError::internal)?;
    Ok(NaiveDateTime::new(date, time))
}

fn format_datetime2(value: tiberius::time::DateTime2) -> Result<String, AppError> {
    datetime2_as_naive(value).map(format_naive_datetime)
}

fn format_datetime_offset(value: tiberius::time::DateTimeOffset) -> Result<String, AppError> {
    let offset = value.offset();
    if !(-840..=840).contains(&offset) {
        return Err(AppError::internal());
    }
    let datetime = datetime2_as_naive(value.datetime2())?
        .checked_add_signed(chrono::Duration::minutes(i64::from(offset)))
        .ok_or_else(AppError::internal)?;
    let minimum_date = NaiveDate::from_ymd_opt(1, 1, 1).ok_or_else(AppError::internal)?;
    let maximum_date = NaiveDate::from_ymd_opt(9999, 12, 31).ok_or_else(AppError::internal)?;
    if !(minimum_date..=maximum_date).contains(&datetime.date()) {
        return Err(AppError::internal());
    }
    let datetime = format_naive_datetime(datetime);
    let sign = if offset < 0 { '-' } else { '+' };
    let minutes = offset.unsigned_abs();
    Ok(format!(
        "{datetime}{sign}{:02}:{:02}",
        minutes / 60,
        minutes % 60
    ))
}

fn format_naive_datetime(value: NaiveDateTime) -> String {
    value.format("%Y-%m-%dT%H:%M:%S%.f").to_string()
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
            "sqlserver_result_batch_too_large",
            "One SQL Server result row exceeds the retained-result batch limit",
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
        tracing::warn!(error = %error, "native SQL Server retained-result cleanup failed");
    }
}

fn resource_error(code: impl Into<String>, message: impl Into<String>) -> AppError {
    AppError::new(
        AppErrorKind::ResourceExhausted,
        ApiError::new(code, message),
    )
}

async fn execute_update(
    resolved: ResolvedDatasourceConnection,
    sql: String,
    cancellation: CancellationToken,
) -> Result<u64, DatabaseWriteError> {
    if cancellation.is_cancelled() {
        return Err(DatabaseWriteError::not_started(
            write_cancelled_before_dispatch(),
        ));
    }
    let sql = validate_single_statement(&sql).map_err(DatabaseWriteError::not_started)?;
    if resolved.connection.read_only {
        return Err(DatabaseWriteError::not_started(AppError::new(
            AppErrorKind::Conflict,
            ApiError::new(
                "datasource_read_only",
                "The datasource connection is configured as read-only",
            ),
        )));
    }
    let open = open_resolved_connection(&resolved, None);
    tokio::pin!(open);
    let mut conn = tokio::select! {
        biased;
        () = cancellation.cancelled() => {
            return Err(DatabaseWriteError::not_started(write_cancelled_before_dispatch()));
        }
        result = &mut open => result.map_err(DatabaseWriteError::not_started)?,
    };
    if cancellation.is_cancelled() {
        discard_connection(conn).await;
        return Err(DatabaseWriteError::not_started(
            write_cancelled_before_dispatch(),
        ));
    }
    let result = {
        let execution = conn.client.execute(sql, &[]);
        tokio::pin!(execution);
        tokio::select! {
            biased;
            () = cancellation.cancelled() => None,
            result = &mut execution => Some(result),
        }
    };
    let Some(result) = result else {
        discard_connection(conn).await;
        return Err(DatabaseWriteError::unknown(write_outcome_unknown(
            "The SQL Server write was interrupted after dispatch; do not retry it blindly",
        )));
    };
    match result {
        Ok(result) => {
            let affected = result.total();
            finish_connection(conn, Ok(()))
                .await
                .map_err(DatabaseWriteError::unknown)?;
            Ok(affected)
        }
        Err(error) => {
            tracing::warn!(error = %error, "SQL Server rejected a dispatched write");
            discard_connection(conn).await;
            Err(DatabaseWriteError::unknown(write_outcome_unknown(
                "SQL Server reported an error after write dispatch; partial effects cannot be excluded, so do not retry it blindly",
            )))
        }
    }
}

fn validate_single_statement(sql: &str) -> Result<String, AppError> {
    if sql.len() > MAX_SQL_BYTES || sql.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_database_write",
            "SQL must be non-empty and within the configured size limit",
        ));
    }
    let statements = split_sqlserver_script(sql)?;
    if statements.len() != 1 {
        return Err(AppError::invalid(
            "invalid_database_write",
            "Exactly one SQL Server statement is required",
        ));
    }
    Ok(statements.into_iter().next().expect("one statement"))
}

fn write_cancelled_before_dispatch() -> AppError {
    AppError::new(
        AppErrorKind::Conflict,
        ApiError::new(
            "database_write_cancelled",
            "The database write was cancelled before dispatch",
        ),
    )
}

fn write_outcome_unknown(message: &'static str) -> AppError {
    AppError::new(
        AppErrorKind::Unavailable,
        ApiError::new("database_write_outcome_unknown", message),
    )
}

enum ConsoleExecutionError {
    Cancelled(Option<String>),
    ConnectionUnusable(AppError),
    WriteOutcomeUnknown,
    Statement(AppError),
}

struct ConsolePending {
    id: u32,
    started: Instant,
    columns: Vec<ResultColumn>,
    rows: Vec<ResultRow>,
    row_count: u64,
    retain: bool,
    page_end: u64,
}

async fn execute_console(
    application: &Application,
    request: NativeConsoleRequest,
    mut cancellation: watch::Receiver<CancellationRequest>,
    force_read_only: bool,
) -> Result<Vec<NativeConsoleResult>, AppError> {
    let (mut statements, page_offset, page_end) = prepare_console_request(&request)?;
    if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
        return Err(console_cancelled(reason));
    }
    let resolved = resolve_native_connection(application, &request.datasource_id).await?;
    if force_read_only || resolved.connection.read_only {
        for statement in &statements {
            validate_read_sql(statement).map_err(|_| {
                AppError::new(
                    AppErrorKind::Conflict,
                    ApiError::new(
                        "datasource_read_only",
                        "The SQL Server datasource accepts read-only Console statements",
                    ),
                )
            })?;
        }
    }
    if request.explain {
        statements = statements
            .into_iter()
            .map(|statement| format!("SET SHOWPLAN_ALL ON; {statement}; SET SHOWPLAN_ALL OFF"))
            .collect();
    }
    let open = open_resolved_connection(
        &resolved,
        (!request.database_name.trim().is_empty()).then_some(request.database_name.as_str()),
    );
    tokio::pin!(open);
    let mut conn = loop {
        tokio::select! {
            biased;
            changed = cancellation.changed() => {
                if changed.is_ok()
                    && let CancellationRequest::Requested { reason } = cancellation.borrow().clone()
                {
                    return Err(console_cancelled(reason));
                }
            }
            result = &mut open => break result?,
        }
    };
    let mut results = Vec::new();
    let mut retained_bytes = 0_u64;
    for (index, statement) in statements.into_iter().enumerate() {
        let sequence = u32::try_from(index)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(AppError::internal)?;
        let started = Instant::now();
        let execution = execute_console_statement(
            &mut conn.client,
            &statement,
            sequence,
            page_offset,
            page_end,
            request.result_set_id,
            &mut retained_bytes,
            &mut cancellation,
            request.explain,
        )
        .await;
        match execution {
            Ok(mut statement_results) => results.append(&mut statement_results),
            Err(ConsoleExecutionError::Statement(error)) => {
                results.push(console_failure_result(
                    sequence,
                    statement,
                    &error,
                    elapsed_millis(started),
                ));
                if !request.error_continue {
                    break;
                }
            }
            Err(ConsoleExecutionError::Cancelled(reason)) => {
                discard_connection(conn).await;
                return Err(console_cancelled(reason));
            }
            Err(ConsoleExecutionError::ConnectionUnusable(error)) => {
                discard_connection(conn).await;
                return Err(error);
            }
            Err(ConsoleExecutionError::WriteOutcomeUnknown) => {
                discard_connection(conn).await;
                return Err(write_outcome_unknown(
                    "The SQL Server Console write was interrupted after dispatch; do not retry it blindly",
                ));
            }
        }
    }
    finish_connection(conn, Ok(())).await?;
    Ok(results)
}

fn prepare_console_request(
    request: &NativeConsoleRequest,
) -> Result<(Vec<String>, u64, u64), AppError> {
    if request.sql.trim().is_empty() || request.sql.len() > MAX_SQL_BYTES {
        return Err(AppError::invalid(
            "invalid_sqlserver_console_request",
            "sql must be non-empty and within the configured size limit",
        ));
    }
    if request.page_no == 0 || request.page_size == 0 || request.page_size > MAX_CONSOLE_PAGE_SIZE {
        return Err(AppError::invalid(
            "invalid_sqlserver_console_request",
            format!(
                "pageNo and pageSize must be positive, and pageSize cannot exceed {MAX_CONSOLE_PAGE_SIZE}"
            ),
        ));
    }
    let statements = if request.single {
        vec![request.sql.trim().to_owned()]
    } else {
        split_sqlserver_script(&request.sql)?
    };
    if statements.is_empty() || statements.len() > MAX_CONSOLE_STATEMENTS {
        return Err(AppError::invalid(
            "invalid_sqlserver_console_request",
            format!(
                "A Console request must contain between 1 and {MAX_CONSOLE_STATEMENTS} statements"
            ),
        ));
    }
    let page_size = if request.page_size_all {
        u64::from(MAX_CONSOLE_PAGE_SIZE)
    } else {
        u64::from(request.page_size)
    };
    let page_offset = if request.page_size_all {
        0
    } else {
        u64::from(request.page_no - 1)
            .checked_mul(page_size)
            .ok_or_else(AppError::internal)?
    };
    let page_end = page_offset
        .checked_add(page_size)
        .ok_or_else(AppError::internal)?;
    Ok((statements, page_offset, page_end))
}

#[allow(clippy::too_many_arguments)]
async fn execute_console_statement(
    client: &mut SqlServerClient,
    statement: &str,
    statement_sequence: u32,
    page_offset: u64,
    page_end: u64,
    selected_result_set_id: Option<u32>,
    retained_bytes: &mut u64,
    cancellation: &mut watch::Receiver<CancellationRequest>,
    force_tabular: bool,
) -> Result<Vec<NativeConsoleResult>, ConsoleExecutionError> {
    if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
        return Err(ConsoleExecutionError::Cancelled(reason));
    }
    let read_only = validate_read_sql(statement).is_ok();
    let creates_local_temp_table = creates_local_temp_table(statement);
    if force_tabular || is_tabular_statement(statement) || creates_local_temp_table {
        return execute_console_query(
            client,
            statement,
            statement_sequence,
            page_offset,
            page_end,
            selected_result_set_id,
            retained_bytes,
            cancellation,
            read_only,
            !force_tabular && !creates_local_temp_table,
        )
        .await;
    }
    let started = Instant::now();
    let execution = AssertUnwindSafe(client.execute(statement, &[])).catch_unwind();
    tokio::pin!(execution);
    let result = loop {
        tokio::select! {
            biased;
            changed = cancellation.changed() => {
                if changed.is_ok()
                    && let CancellationRequest::Requested { reason } = cancellation.borrow().clone()
                {
                    return Err(console_execution_interrupted(read_only, reason));
                }
            }
            result = &mut execution => {
                break result.map_err(|_| console_driver_interrupted(read_only))?;
            }
        }
    }
    .map_err(|error| console_dispatched_driver_error(read_only, error))?;
    Ok(vec![NativeConsoleResult {
        statement_sequence,
        result_set_id: None,
        sql: statement.to_owned(),
        success: true,
        message: "Statement executed successfully".to_owned(),
        update_count: result.total(),
        columns: Vec::new(),
        rows: Vec::new(),
        row_count: 0,
        has_more: false,
        duration_ms: elapsed_millis(started),
        error: None,
    }])
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn execute_console_query(
    client: &mut SqlServerClient,
    statement: &str,
    statement_sequence: u32,
    page_offset: u64,
    page_end: u64,
    selected_result_set_id: Option<u32>,
    retained_bytes: &mut u64,
    cancellation: &mut watch::Receiver<CancellationRequest>,
    read_only: bool,
    preflight_result: bool,
) -> Result<Vec<NativeConsoleResult>, ConsoleExecutionError> {
    let statement_started = Instant::now();
    if preflight_result {
        match validate_result_set_with_control(client, statement, None, cancellation).await {
            Ok(()) => {}
            Err(ControlledResultSetValidationError::Cancelled(reason)) => {
                return Err(ConsoleExecutionError::Cancelled(reason));
            }
            Err(ControlledResultSetValidationError::TimedOut(error)) => {
                return Err(ConsoleExecutionError::ConnectionUnusable(error));
            }
            Err(ControlledResultSetValidationError::Failed(error)) => {
                return Err(ConsoleExecutionError::Statement(error));
            }
        }
    }
    if let CancellationRequest::Requested { reason } = cancellation.borrow().clone() {
        return Err(ConsoleExecutionError::Cancelled(reason));
    }
    let open = AssertUnwindSafe(client.simple_query(statement)).catch_unwind();
    tokio::pin!(open);
    let mut stream = loop {
        tokio::select! {
            biased;
            changed = cancellation.changed() => {
                if changed.is_ok()
                    && let CancellationRequest::Requested { reason } = cancellation.borrow().clone()
                {
                    return Err(console_execution_interrupted(read_only, reason));
                }
            }
            result = &mut open => {
                break result.map_err(|_| console_driver_interrupted(read_only))?;
            }
        }
    }
    .map_err(|error| console_dispatched_driver_error(read_only, error))?;

    let mut results = Vec::new();
    let mut pending: Option<ConsolePending> = None;
    let mut result_set_id = 0_u32;
    loop {
        let item = loop {
            tokio::select! {
                biased;
                changed = cancellation.changed() => {
                    if changed.is_ok()
                        && let CancellationRequest::Requested { reason } = cancellation.borrow().clone()
                    {
                        return Err(console_execution_interrupted(read_only, reason));
                    }
                }
                item = AssertUnwindSafe(stream.try_next()).catch_unwind() => {
                    break item.map_err(|_| console_driver_interrupted(read_only))?;
                },
            }
        }
        .map_err(|error| console_dispatched_driver_error(read_only, error))?;
        let Some(item) = item else {
            break;
        };
        match item {
            QueryItem::Metadata(metadata) => {
                if let Some(previous) = pending.take() {
                    push_console_result(&mut results, statement_sequence, statement, previous);
                }
                result_set_id = result_set_id
                    .checked_add(1)
                    .ok_or_else(|| console_post_dispatch_error(read_only, AppError::internal()))?;
                let retain =
                    selected_result_set_id.is_none_or(|selected| selected == result_set_id);
                let columns = if retain {
                    if metadata.columns().len() > MAX_COLUMNS {
                        return Err(console_post_dispatch_error(
                            read_only,
                            resource_error(
                                "sqlserver_result_too_wide",
                                format!("SQL Server returned more than {MAX_COLUMNS} columns"),
                            ),
                        ));
                    }
                    metadata
                        .columns()
                        .iter()
                        .enumerate()
                        .map(|(index, column)| portable_column(index, column))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|error| console_post_dispatch_error(read_only, error))?
                } else {
                    Vec::new()
                };
                pending = Some(ConsolePending {
                    id: result_set_id,
                    started: Instant::now(),
                    columns,
                    rows: Vec::new(),
                    row_count: 0,
                    retain,
                    page_end,
                });
            }
            QueryItem::Row(row) => {
                let current = pending
                    .as_mut()
                    .ok_or_else(|| console_post_dispatch_error(read_only, AppError::internal()))?;
                if current.retain && (page_offset..page_end).contains(&current.row_count) {
                    let row = portable_row(row)
                        .map_err(|error| console_post_dispatch_error(read_only, error))?;
                    reserve_console_bytes(retained_bytes, &row)
                        .map_err(|error| console_post_dispatch_error(read_only, error))?;
                    current.rows.push(row);
                }
                current.row_count = current
                    .row_count
                    .checked_add(1)
                    .ok_or_else(|| console_post_dispatch_error(read_only, AppError::internal()))?;
            }
        }
    }
    if let Some(previous) = pending {
        push_console_result(&mut results, statement_sequence, statement, previous);
    }
    if results.is_empty() && selected_result_set_id.is_none() {
        results.push(NativeConsoleResult {
            statement_sequence,
            result_set_id: None,
            sql: statement.to_owned(),
            success: true,
            message: "Statement executed successfully".to_owned(),
            update_count: 0,
            columns: Vec::new(),
            rows: Vec::new(),
            row_count: 0,
            has_more: false,
            duration_ms: elapsed_millis(statement_started),
            error: None,
        });
    }
    Ok(results)
}

fn console_execution_interrupted(read_only: bool, reason: Option<String>) -> ConsoleExecutionError {
    if read_only {
        ConsoleExecutionError::Cancelled(reason)
    } else {
        ConsoleExecutionError::WriteOutcomeUnknown
    }
}

fn console_driver_interrupted(read_only: bool) -> ConsoleExecutionError {
    if read_only {
        ConsoleExecutionError::Statement(sqlserver_driver_failure())
    } else {
        ConsoleExecutionError::WriteOutcomeUnknown
    }
}

fn console_dispatched_driver_error(read_only: bool, error: TiberiusError) -> ConsoleExecutionError {
    if read_only || matches!(error, TiberiusError::Server(_)) {
        ConsoleExecutionError::Statement(sqlserver_query_error(error))
    } else {
        ConsoleExecutionError::WriteOutcomeUnknown
    }
}

fn console_post_dispatch_error(read_only: bool, error: AppError) -> ConsoleExecutionError {
    if read_only {
        ConsoleExecutionError::Statement(error)
    } else {
        ConsoleExecutionError::WriteOutcomeUnknown
    }
}

fn push_console_result(
    output: &mut Vec<NativeConsoleResult>,
    statement_sequence: u32,
    statement: &str,
    pending: ConsolePending,
) {
    if !pending.retain {
        return;
    }
    output.push(NativeConsoleResult {
        statement_sequence,
        result_set_id: Some(pending.id),
        sql: statement.to_owned(),
        success: true,
        message: "Statement executed successfully".to_owned(),
        update_count: 0,
        columns: pending.columns,
        rows: pending.rows,
        row_count: pending.row_count,
        has_more: pending.row_count > pending.page_end,
        duration_ms: elapsed_millis(pending.started),
        error: None,
    });
}

fn portable_column(index: usize, column: &Column) -> Result<ResultColumn, AppError> {
    validate_supported_column_type(column.column_type())?;
    let ordinal = u32::try_from(index)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(AppError::internal)?;
    Ok(ResultColumn {
        ordinal,
        label: column.name().to_owned(),
        name: column.name().to_owned(),
        jdbc_type: sqlserver_jdbc_type(column.column_type()),
        jdbc_type_name: sqlserver_type_name(column.column_type()).to_owned(),
        value_type: portable_value_type(column.column_type()),
        nullability: ColumnNullability::Unknown,
        precision: None,
        scale: None,
        display_size: None,
        signed: sqlserver_numeric_type(column.column_type()).then_some(true),
        catalog_name: None,
        schema_name: None,
        table_name: None,
    })
}

fn portable_value_type(column_type: ColumnType) -> JdbcValueType {
    match sqlserver_value_type(column_type) {
        wire::JdbcValueType::Boolean => JdbcValueType::Boolean,
        wire::JdbcValueType::SignedInteger => JdbcValueType::SignedInteger,
        wire::JdbcValueType::UnsignedInteger => JdbcValueType::UnsignedInteger,
        wire::JdbcValueType::Float32 => JdbcValueType::Float32,
        wire::JdbcValueType::Float64 => JdbcValueType::Float64,
        wire::JdbcValueType::Decimal => JdbcValueType::Decimal,
        wire::JdbcValueType::Text => JdbcValueType::Text,
        wire::JdbcValueType::Binary => JdbcValueType::Binary,
        wire::JdbcValueType::Date => JdbcValueType::Date,
        wire::JdbcValueType::Time => JdbcValueType::Time,
        wire::JdbcValueType::Timestamp => JdbcValueType::Timestamp,
        wire::JdbcValueType::TimestampWithTimeZone => JdbcValueType::TimestampWithTimeZone,
        wire::JdbcValueType::Json => JdbcValueType::Json,
        wire::JdbcValueType::Uuid => JdbcValueType::Uuid,
        wire::JdbcValueType::Opaque | wire::JdbcValueType::Unspecified => JdbcValueType::Opaque,
    }
}

fn portable_row(row: Row) -> Result<ResultRow, AppError> {
    Ok(ResultRow {
        values: row
            .into_iter()
            .map(portable_value)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn portable_value(value: ColumnData<'static>) -> Result<JdbcValue, AppError> {
    use wire::jdbc_value::Value as WireValue;

    let value = wire_value(value)?;
    let Some(value) = value.value else {
        return Err(AppError::internal());
    };
    Ok(match value {
        WireValue::NullValue(_) => JdbcValue::Null,
        WireValue::BooleanValue(value) => JdbcValue::Boolean { value },
        WireValue::SignedIntegerValue(value) => JdbcValue::SignedInteger {
            value: value.to_string(),
        },
        WireValue::UnsignedIntegerValue(value) => JdbcValue::UnsignedInteger {
            value: value.to_string(),
        },
        WireValue::Float32Value(value) => JdbcValue::Float32 {
            value: display_f32(value),
        },
        WireValue::Float64Value(value) => JdbcValue::Float64 {
            value: display_f64(value),
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

fn reserve_console_bytes(total: &mut u64, row: &ResultRow) -> Result<(), AppError> {
    let bytes = u64::try_from(
        serde_json::to_vec(row)
            .map_err(|_| AppError::internal())?
            .len(),
    )
    .map_err(|_| AppError::internal())?;
    *total = total.checked_add(bytes).ok_or_else(AppError::internal)?;
    if *total > MAX_CONSOLE_RESULT_BYTES {
        return Err(resource_error(
            "sqlserver_console_result_too_large",
            "The retained SQL Server Console result exceeded the configured byte limit",
        ));
    }
    Ok(())
}

fn display_f32(value: f32) -> String {
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

fn display_f64(value: f64) -> String {
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

fn console_failure_result(
    statement_sequence: u32,
    sql: String,
    error: &AppError,
    duration_ms: u64,
) -> NativeConsoleResult {
    NativeConsoleResult {
        statement_sequence,
        result_set_id: None,
        sql,
        success: false,
        message: error.api_error().message.clone(),
        update_count: 0,
        columns: Vec::new(),
        rows: Vec::new(),
        row_count: 0,
        has_more: false,
        duration_ms,
        error: Some(error.api_error()),
    }
}

fn console_cancelled(reason: Option<String>) -> AppError {
    AppError::new(
        AppErrorKind::Conflict,
        ApiError::new(
            "sqlserver_console_cancelled",
            reason.unwrap_or_else(|| "The SQL Server Console execution was cancelled".to_owned()),
        ),
    )
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn is_tabular_statement(statement: &str) -> bool {
    sql_lexemes(statement).is_ok_and(|lexemes| {
        if is_extended_property_procedure(&lexemes.words) {
            return false;
        }
        matches!(
            lexemes.words.first().map(String::as_str),
            Some("SELECT" | "WITH" | "EXEC" | "EXECUTE" | "DBCC")
        )
    })
}

fn is_extended_property_procedure(words: &[String]) -> bool {
    let [command, procedure @ ..] = words else {
        return false;
    };
    if !matches!(command.as_str(), "EXEC" | "EXECUTE") {
        return false;
    }
    let procedure = match procedure {
        [system_schema, procedure, ..] if system_schema == "SYS" => procedure.as_str(),
        [procedure, ..] => procedure.as_str(),
        [] => return false,
    };
    matches!(
        procedure,
        "SP_ADDEXTENDEDPROPERTY" | "SP_UPDATEEXTENDEDPROPERTY" | "SP_DROPEXTENDEDPROPERTY"
    )
}

fn creates_local_temp_table(statement: &str) -> bool {
    let Ok(statements) = Parser::parse_sql(&MsSqlDialect {}, statement) else {
        return false;
    };
    let [Statement::CreateTable(table)] = statements.as_slice() else {
        return false;
    };
    table
        .name
        .0
        .last()
        .and_then(sqlparser::ast::ObjectNamePart::as_ident)
        .is_some_and(|name| name.value.starts_with('#') && !name.value.starts_with("##"))
}

fn split_sqlserver_script(script: &str) -> Result<Vec<String>, AppError> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut chars = script.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\'' | '"' => {
                current.push(ch);
                copy_quoted_chars(&mut current, &mut chars, ch)?;
            }
            '[' => {
                current.push(ch);
                copy_quoted_chars(&mut current, &mut chars, ']')?;
            }
            '-' if chars.peek().is_some_and(|next| *next == '-') => {
                current.push('-');
                current.push('-');
                chars.next();
                for next in chars.by_ref() {
                    current.push(next);
                    if next == '\n' {
                        break;
                    }
                }
            }
            '/' if chars.peek().is_some_and(|next| *next == '*') => {
                current.push('/');
                current.push('*');
                chars.next();
                let mut previous = '\0';
                let mut closed = false;
                for next in chars.by_ref() {
                    current.push(next);
                    if previous == '*' && next == '/' {
                        closed = true;
                        break;
                    }
                    previous = next;
                }
                if !closed {
                    return Err(AppError::invalid(
                        "invalid_sqlserver_console_request",
                        "The SQL Server script contains an unterminated comment",
                    ));
                }
            }
            ';' => push_statement(&mut statements, &mut current),
            _ => current.push(ch),
        }
    }
    push_statement(&mut statements, &mut current);
    Ok(statements)
}

fn copy_quoted_chars<I>(
    output: &mut String,
    chars: &mut std::iter::Peekable<I>,
    closing: char,
) -> Result<(), AppError>
where
    I: Iterator<Item = char>,
{
    while let Some(ch) = chars.next() {
        output.push(ch);
        if ch == closing {
            if chars.peek().is_some_and(|next| *next == closing) {
                output.push(closing);
                chars.next();
            } else {
                return Ok(());
            }
        }
    }
    Err(AppError::invalid(
        "invalid_sqlserver_console_request",
        "The SQL Server script contains an unterminated quoted value",
    ))
}

fn push_statement(statements: &mut Vec<String>, current: &mut String) {
    let statement = current.trim();
    if !statement.is_empty() {
        statements.push(statement.to_owned());
    }
    current.clear();
}

async fn metadata_rows(
    application: &Application,
    datasource_id: &str,
    database_name: Option<&str>,
    sql: &str,
    values: Vec<DatabaseValue>,
) -> Result<Vec<Row>, AppError> {
    let resolved = resolve_native_connection(application, datasource_id).await?;
    let mut conn = open_resolved_connection(&resolved, database_name).await?;
    let parameters = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| QueryParameter {
            position: u32::try_from(index + 1).unwrap_or(u32::MAX),
            value,
        })
        .collect::<Vec<_>>();
    let query = bind_query(sql, &parameters)?;
    let result = tokio::time::timeout(METADATA_TIMEOUT, async {
        query
            .query(&mut conn.client)
            .await
            .map_err(sqlserver_query_error)?
            .into_first_result()
            .await
            .map_err(sqlserver_query_error)
    })
    .await
    .map_err(|_| metadata_timeout())?;
    finish_connection(conn, result).await
}

fn row_string(row: &Row, index: usize) -> Result<String, AppError> {
    Ok(row
        .try_get::<&str, _>(index)
        .map_err(sqlserver_query_error)?
        .unwrap_or_default()
        .to_owned())
}

fn row_optional_string(row: &Row, index: usize) -> Result<Option<String>, AppError> {
    row.try_get::<&str, _>(index)
        .map(|value| value.map(ToOwned::to_owned))
        .map_err(sqlserver_query_error)
}

fn row_i32(row: &Row, index: usize) -> Result<i32, AppError> {
    row.try_get::<i32, _>(index)
        .map_err(sqlserver_query_error)?
        .ok_or_else(AppError::internal)
}

fn row_bool(row: &Row, index: usize) -> Result<bool, AppError> {
    row.try_get::<bool, _>(index)
        .map_err(sqlserver_query_error)?
        .ok_or_else(AppError::internal)
}

fn row_optional_i32(row: &Row, index: usize) -> Result<Option<i32>, AppError> {
    row.try_get::<i32, _>(index).map_err(sqlserver_query_error)
}

async fn list_databases(
    application: &Application,
    datasource_id: &str,
) -> Result<DatabaseList, AppError> {
    let rows = metadata_rows(
        application,
        datasource_id,
        None,
        "SELECT d.name, COALESCE(CONVERT(nvarchar(128), DATABASEPROPERTYEX(d.name, 'Collation')), ''), \
         COALESCE(SUSER_SNAME(d.owner_sid), ''), CONVERT(int, d.database_id) \
         FROM sys.databases d WHERE d.state = 0 ORDER BY d.name",
        Vec::new(),
    )
    .await?;
    rows.into_iter()
        .map(|row| {
            let name = row_string(&row, 0)?;
            Ok(DatabaseMetadata {
                name,
                collation: row_string(&row, 1)?,
                owner: row_string(&row, 2)?,
                system: row_i32(&row, 3)? <= 4,
                ..DatabaseMetadata::default()
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|items| DatabaseList { items })
}

async fn list_schemas(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) -> Result<SchemaList, AppError> {
    validate_identifier(database_name, "databaseName")?;
    let rows = metadata_rows(
        application,
        datasource_id,
        Some(database_name),
        "SELECT s.name, COALESCE(p.name, ''), CONVERT(int, s.schema_id) \
         FROM sys.schemas s LEFT JOIN sys.database_principals p ON p.principal_id = s.principal_id \
         ORDER BY s.name",
        Vec::new(),
    )
    .await?;
    rows.into_iter()
        .map(|row| {
            let name = row_string(&row, 0)?;
            let schema_id = row_i32(&row, 2)?;
            Ok(SchemaMetadata {
                database_name: database_name.to_owned(),
                system: schema_id <= 4
                    || matches!(
                        name.as_str(),
                        "sys" | "INFORMATION_SCHEMA" | "db_owner" | "db_accessadmin"
                    ),
                name,
                owner: row_string(&row, 1)?,
                ..SchemaMetadata::default()
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|items| SchemaList { items })
}

fn effective_schema(schema_name: &str) -> &str {
    if schema_name.trim().is_empty() {
        "dbo"
    } else {
        schema_name
    }
}

async fn list_tables(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    name_pattern: &str,
) -> Result<TableList, AppError> {
    validate_identifier(database_name, "databaseName")?;
    let schema_name = effective_schema(schema_name);
    validate_identifier(schema_name, "schemaName")?;
    let rows = metadata_rows(
        application,
        datasource_id,
        Some(database_name),
        "SELECT s.name, t.name, COALESCE(CONVERT(nvarchar(max), ep.value), ''), \
         COALESCE(CONVERT(nvarchar(40), SUM(CASE WHEN p.index_id IN (0,1) THEN p.rows ELSE 0 END)), '0'), \
         COALESCE(CONVERT(nvarchar(40), SUM(CASE WHEN p.index_id IN (0,1) THEN a.total_pages * 8192 ELSE 0 END)), '0'), \
         CONVERT(nvarchar(33), t.create_date, 126), CONVERT(nvarchar(33), t.modify_date, 126) \
         FROM sys.tables t JOIN sys.schemas s ON s.schema_id=t.schema_id \
         LEFT JOIN sys.partitions p ON p.object_id=t.object_id \
         LEFT JOIN sys.allocation_units a ON a.container_id=p.partition_id \
         LEFT JOIN sys.extended_properties ep ON ep.major_id=t.object_id AND ep.minor_id=0 AND ep.name='MS_Description' \
         WHERE s.name=@P1 AND (@P2='' OR t.name LIKE @P2) \
         GROUP BY s.name,t.name,ep.value,t.create_date,t.modify_date ORDER BY t.name",
        vec![
            DatabaseValue::Text(schema_name.to_owned()),
            DatabaseValue::Text(name_pattern.trim().to_owned()),
        ],
    )
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(TableMetadata {
                database_name: database_name.to_owned(),
                schema_name: row_string(&row, 0)?,
                name: row_string(&row, 1)?,
                table_type: "TABLE".to_owned(),
                comment: row_string(&row, 2)?,
                database_type: "SQLSERVER".to_owned(),
                rows: Some(row_string(&row, 3)?),
                data_length: Some(row_string(&row, 4)?),
                create_time: row_string(&row, 5)?,
                update_time: row_string(&row, 6)?,
                ..TableMetadata::default()
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|items| TableList { items })
}

async fn list_columns(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> Result<ColumnList, AppError> {
    validate_identifier(database_name, "databaseName")?;
    let schema_name = effective_schema(schema_name);
    validate_identifier(schema_name, "schemaName")?;
    validate_identifier(table_name, "tableName")?;
    let rows = metadata_rows(
        application,
        datasource_id,
        Some(database_name),
        "SELECT c.name, ty.name, \
         ty.name + CASE WHEN ty.name IN ('varchar','char','varbinary','binary') THEN '(' + CASE WHEN c.max_length=-1 THEN 'max' ELSE CONVERT(varchar(10),c.max_length) END + ')' \
                      WHEN ty.name IN ('nvarchar','nchar') THEN '(' + CASE WHEN c.max_length=-1 THEN 'max' ELSE CONVERT(varchar(10),c.max_length/2) END + ')' \
                      WHEN ty.name IN ('decimal','numeric') THEN '('+CONVERT(varchar(10),c.precision)+','+CONVERT(varchar(10),c.scale)+')' \
                      WHEN ty.name IN ('datetime2','datetimeoffset','time') THEN '('+CONVERT(varchar(10),c.scale)+')' ELSE '' END, \
         dc.definition, COALESCE(CONVERT(nvarchar(max),ep.value),''), c.is_nullable, CONVERT(int,c.column_id), \
         CONVERT(int,c.max_length), CONVERT(int,c.precision), CONVERT(int,c.scale), c.is_identity, \
         TRY_CONVERT(int,ic.seed_value), TRY_CONVERT(int,ic.increment_value), c.is_computed, cc.definition, c.is_sparse, \
         COALESCE(dc.name,''), COALESCE(c.collation_name,''), COALESCE(kc.name,''), COALESCE(CONVERT(int,icx.key_ordinal),0) \
         FROM sys.columns c JOIN sys.tables t ON t.object_id=c.object_id \
         JOIN sys.schemas s ON s.schema_id=t.schema_id JOIN sys.types ty ON ty.user_type_id=c.user_type_id \
         LEFT JOIN sys.default_constraints dc ON dc.object_id=c.default_object_id \
         LEFT JOIN sys.identity_columns ic ON ic.object_id=c.object_id AND ic.column_id=c.column_id \
         LEFT JOIN sys.computed_columns cc ON cc.object_id=c.object_id AND cc.column_id=c.column_id \
         LEFT JOIN sys.extended_properties ep ON ep.major_id=c.object_id AND ep.minor_id=c.column_id AND ep.name='MS_Description' \
         LEFT JOIN sys.indexes pix ON pix.object_id=t.object_id AND pix.is_primary_key=1 \
         LEFT JOIN sys.index_columns icx ON icx.object_id=t.object_id AND icx.index_id=pix.index_id AND icx.column_id=c.column_id \
         LEFT JOIN sys.key_constraints kc ON kc.parent_object_id=t.object_id AND kc.unique_index_id=pix.index_id AND kc.type='PK' \
         WHERE s.name=@P1 AND t.name=@P2 ORDER BY c.column_id",
        vec![
            DatabaseValue::Text(schema_name.to_owned()),
            DatabaseValue::Text(table_name.to_owned()),
        ],
    )
    .await?;
    rows.into_iter()
        .map(|row| {
            let type_name = row_string(&row, 1)?;
            let generated = row_bool(&row, 13)?;
            let primary_key_order = row_i32(&row, 19)?;
            Ok(ColumnMetadata {
                database_name: database_name.to_owned(),
                schema_name: schema_name.to_owned(),
                table_name: table_name.to_owned(),
                name: row_string(&row, 0)?,
                column_type: row_string(&row, 2)?,
                data_type: Some(sqlserver_metadata_jdbc_type(&type_name)),
                default_value: row_optional_string(&row, 3)?,
                auto_increment: Some(row_bool(&row, 10)?),
                comment: row_string(&row, 4)?,
                primary_key: Some(primary_key_order > 0),
                primary_key_name: row_string(&row, 18)?,
                primary_key_order,
                column_size: Some(row_i32(&row, 7)?),
                buffer_length: Some(row_i32(&row, 7)?),
                decimal_digits: Some(row_i32(&row, 9)?),
                num_prec_radix: sqlserver_numeric_name(&type_name).then_some(10),
                char_octet_length: Some(row_i32(&row, 7)?),
                ordinal_position: Some(row_i32(&row, 6)?),
                nullable: Some(i32::from(row_bool(&row, 5)?)),
                generated_column: Some(generated),
                extent: if generated {
                    row_optional_string(&row, 14)?.unwrap_or_default()
                } else {
                    String::new()
                },
                collation: row_string(&row, 17)?,
                sparse: Some(row_bool(&row, 15)?),
                default_constraint_name: row_string(&row, 16)?,
                seed: row_optional_i32(&row, 11)?,
                increment: row_optional_i32(&row, 12)?,
                ..ColumnMetadata::default()
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|items| ColumnList { items })
}

async fn list_indexes(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> Result<IndexList, AppError> {
    validate_identifier(database_name, "databaseName")?;
    let schema_name = effective_schema(schema_name);
    validate_identifier(schema_name, "schemaName")?;
    validate_identifier(table_name, "tableName")?;
    let rows = metadata_rows(
        application,
        datasource_id,
        Some(database_name),
        "SELECT i.name, i.is_unique, i.type_desc, COALESCE(i.filter_definition,''), \
         CONVERT(int,ic.key_ordinal), ic.is_descending_key, c.name, ic.is_included_column, \
         COALESCE(CONVERT(nvarchar(max),ep.value),'') \
         FROM sys.indexes i JOIN sys.tables t ON t.object_id=i.object_id \
         JOIN sys.schemas s ON s.schema_id=t.schema_id \
         JOIN sys.index_columns ic ON ic.object_id=i.object_id AND ic.index_id=i.index_id \
         JOIN sys.columns c ON c.object_id=ic.object_id AND c.column_id=ic.column_id \
         LEFT JOIN sys.extended_properties ep ON ep.major_id=i.object_id AND ep.minor_id=i.index_id AND ep.name='MS_Description' \
         WHERE s.name=@P1 AND t.name=@P2 AND i.index_id>0 ORDER BY i.index_id,ic.index_column_id",
        vec![
            DatabaseValue::Text(schema_name.to_owned()),
            DatabaseValue::Text(table_name.to_owned()),
        ],
    )
    .await?;
    let mut items = Vec::<IndexMetadata>::new();
    let mut positions = HashMap::<String, usize>::new();
    for row in rows {
        let name = row_string(&row, 0)?;
        let index = if let Some(index) = positions.get(&name).copied() {
            index
        } else {
            let index = items.len();
            positions.insert(name.clone(), index);
            items.push(IndexMetadata {
                database_name: database_name.to_owned(),
                schema_name: schema_name.to_owned(),
                table_name: table_name.to_owned(),
                name: name.clone(),
                index_type: row_string(&row, 2)?,
                unique: Some(row_bool(&row, 1)?),
                comment: row_string(&row, 8)?,
                method: row_string(&row, 2)?,
                ..IndexMetadata::default()
            });
            index
        };
        items[index].columns.push(IndexColumnMetadata {
            database_name: database_name.to_owned(),
            schema_name: schema_name.to_owned(),
            table_name: table_name.to_owned(),
            index_name: name,
            column_name: row_string(&row, 6)?,
            column_type: if row_bool(&row, 7)? {
                "INCLUDED".to_owned()
            } else {
                "KEY".to_owned()
            },
            ordinal_position: Some(row_i32(&row, 4)?),
            non_unique: Some(!row_bool(&row, 1)?),
            sort_order: if row_bool(&row, 5)? { "D" } else { "A" }.to_owned(),
            filter_condition: row_string(&row, 3)?,
            ..IndexColumnMetadata::default()
        });
    }
    Ok(IndexList { items })
}

async fn list_views(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    name_pattern: &str,
) -> Result<ViewList, AppError> {
    let schema_name = effective_schema(schema_name);
    validate_identifier(database_name, "databaseName")?;
    validate_identifier(schema_name, "schemaName")?;
    let rows = metadata_rows(
        application,
        datasource_id,
        Some(database_name),
        "SELECT s.name,v.name,COALESCE(m.definition,''),CONVERT(nvarchar(33),v.create_date,126), \
         CONVERT(nvarchar(33),v.modify_date,126),COALESCE(CONVERT(nvarchar(max),ep.value),'') \
         FROM sys.views v JOIN sys.schemas s ON s.schema_id=v.schema_id \
         LEFT JOIN sys.sql_modules m ON m.object_id=v.object_id \
         LEFT JOIN sys.extended_properties ep ON ep.major_id=v.object_id AND ep.minor_id=0 AND ep.name='MS_Description' \
         WHERE s.name=@P1 AND (@P2='' OR v.name LIKE @P2) ORDER BY v.name",
        vec![
            DatabaseValue::Text(schema_name.to_owned()),
            DatabaseValue::Text(name_pattern.trim().to_owned()),
        ],
    )
    .await?;
    rows.into_iter()
        .map(|row| view_metadata(database_name, &row))
        .collect::<Result<Vec<_>, _>>()
        .map(|items| ViewList { items })
}

async fn get_view(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    view_name: &str,
) -> Result<TableMetadata, AppError> {
    let schema_name = effective_schema(schema_name);
    validate_identifier(view_name, "viewName")?;
    let rows = metadata_rows(
        application,
        datasource_id,
        Some(database_name),
        "SELECT s.name,v.name,COALESCE(m.definition,''),CONVERT(nvarchar(33),v.create_date,126), \
         CONVERT(nvarchar(33),v.modify_date,126),COALESCE(CONVERT(nvarchar(max),ep.value),'') \
         FROM sys.views v JOIN sys.schemas s ON s.schema_id=v.schema_id \
         LEFT JOIN sys.sql_modules m ON m.object_id=v.object_id \
         LEFT JOIN sys.extended_properties ep ON ep.major_id=v.object_id AND ep.minor_id=0 AND ep.name='MS_Description' \
         WHERE s.name=@P1 AND v.name=@P2",
        vec![
            DatabaseValue::Text(schema_name.to_owned()),
            DatabaseValue::Text(view_name.to_owned()),
        ],
    )
    .await?;
    rows.into_iter()
        .next()
        .ok_or_else(|| metadata_not_found("view", database_name, schema_name, view_name))
        .and_then(|row| view_metadata(database_name, &row))
}

fn view_metadata(database_name: &str, row: &Row) -> Result<TableMetadata, AppError> {
    Ok(TableMetadata {
        database_name: database_name.to_owned(),
        schema_name: row_string(row, 0)?,
        name: row_string(row, 1)?,
        table_type: "VIEW".to_owned(),
        database_type: "SQLSERVER".to_owned(),
        ddl: row_string(row, 2)?,
        create_time: row_string(row, 3)?,
        update_time: row_string(row, 4)?,
        comment: row_string(row, 5)?,
        ..TableMetadata::default()
    })
}

async fn list_foreign_keys(
    application: &Application,
    request: &ListTableKeysRequest,
    exported: bool,
) -> Result<ForeignKeyList, AppError> {
    let database_name = &request.table.scope.database_name;
    let schema_name = effective_schema(&request.table.scope.schema_name);
    let table_name = &request.table.table_name;
    validate_identifier(database_name, "databaseName")?;
    validate_identifier(schema_name, "schemaName")?;
    validate_identifier(table_name, "tableName")?;
    let predicate = if exported {
        "rs.name=@P1 AND rt.name=@P2"
    } else {
        "fs.name=@P1 AND ft.name=@P2"
    };
    let sql = format!(
        "SELECT rs.name,rt.name,rc.name,fs.name,ft.name,fc.name,CONVERT(int,fkc.constraint_column_id), \
         fk.update_referential_action_desc,fk.delete_referential_action_desc,fk.name,COALESCE(pk.name,'') \
         FROM sys.foreign_keys fk JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id=fk.object_id \
         JOIN sys.tables ft ON ft.object_id=fk.parent_object_id JOIN sys.schemas fs ON fs.schema_id=ft.schema_id \
         JOIN sys.columns fc ON fc.object_id=ft.object_id AND fc.column_id=fkc.parent_column_id \
         JOIN sys.tables rt ON rt.object_id=fk.referenced_object_id JOIN sys.schemas rs ON rs.schema_id=rt.schema_id \
         JOIN sys.columns rc ON rc.object_id=rt.object_id AND rc.column_id=fkc.referenced_column_id \
         LEFT JOIN sys.key_constraints pk ON pk.parent_object_id=rt.object_id AND pk.type='PK' \
         WHERE {predicate} ORDER BY fk.name,fkc.constraint_column_id"
    );
    let rows = metadata_rows(
        application,
        &request.table.scope.datasource_id,
        Some(database_name),
        &sql,
        vec![
            DatabaseValue::Text(schema_name.to_owned()),
            DatabaseValue::Text(table_name.to_owned()),
        ],
    )
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(ForeignKeyMetadata {
                primary_table_database: database_name.to_owned(),
                primary_table_schema: row_string(&row, 0)?,
                primary_table_name: row_string(&row, 1)?,
                primary_column_name: row_string(&row, 2)?,
                foreign_table_database: database_name.to_owned(),
                foreign_table_schema: row_string(&row, 3)?,
                foreign_table_name: row_string(&row, 4)?,
                foreign_column_name: row_string(&row, 5)?,
                key_sequence: row_i32(&row, 6)?,
                update_rule: referential_rule(&row_string(&row, 7)?),
                delete_rule: referential_rule(&row_string(&row, 8)?),
                foreign_key_name: row_string(&row, 9)?,
                primary_key_name: row_string(&row, 10)?,
                deferrability: 7,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|items| ForeignKeyList { items })
}

async fn list_primary_keys(
    application: &Application,
    request: &ListTableKeysRequest,
) -> Result<PrimaryKeyList, AppError> {
    let database_name = &request.table.scope.database_name;
    let schema_name = effective_schema(&request.table.scope.schema_name);
    let table_name = &request.table.table_name;
    let rows = metadata_rows(
        application,
        &request.table.scope.datasource_id,
        Some(database_name),
        "SELECT c.name,kc.name FROM sys.key_constraints kc \
         JOIN sys.tables t ON t.object_id=kc.parent_object_id JOIN sys.schemas s ON s.schema_id=t.schema_id \
         JOIN sys.index_columns ic ON ic.object_id=t.object_id AND ic.index_id=kc.unique_index_id \
         JOIN sys.columns c ON c.object_id=t.object_id AND c.column_id=ic.column_id \
         WHERE kc.type='PK' AND s.name=@P1 AND t.name=@P2 ORDER BY ic.key_ordinal",
        vec![
            DatabaseValue::Text(schema_name.to_owned()),
            DatabaseValue::Text(table_name.to_owned()),
        ],
    )
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(PrimaryKeyMetadata {
                database_name: database_name.to_owned(),
                schema_name: schema_name.to_owned(),
                table_name: table_name.to_owned(),
                column_name: row_string(&row, 0)?,
                name: row_string(&row, 1)?,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|items| PrimaryKeyList { items })
}

fn referential_rule(value: &str) -> i32 {
    match value.to_ascii_uppercase().as_str() {
        "CASCADE" => 0,
        "SET_NULL" => 2,
        "SET_DEFAULT" => 4,
        _ => 3,
    }
}

fn metadata_not_found(
    kind: &str,
    database_name: &str,
    schema_name: &str,
    object_name: &str,
) -> AppError {
    AppError::not_found(
        "sqlserver_metadata_not_found",
        format!("SQL Server {kind} {database_name}.{schema_name}.{object_name} does not exist"),
    )
}

fn sqlserver_numeric_name(type_name: &str) -> bool {
    matches!(
        type_name.to_ascii_lowercase().as_str(),
        "tinyint"
            | "smallint"
            | "int"
            | "bigint"
            | "real"
            | "float"
            | "decimal"
            | "numeric"
            | "money"
            | "smallmoney"
    )
}

fn sqlserver_metadata_jdbc_type(type_name: &str) -> i32 {
    match type_name.to_ascii_lowercase().as_str() {
        "bit" => -7,
        "tinyint" => -6,
        "smallint" => 5,
        "int" => 4,
        "bigint" => -5,
        "real" => 7,
        "float" => 8,
        "decimal" | "numeric" | "money" | "smallmoney" => 3,
        "date" => 91,
        "time" => 92,
        "datetime" | "datetime2" | "smalldatetime" => 93,
        "datetimeoffset" => 2014,
        "uniqueidentifier" => -11,
        "binary" => -2,
        "varbinary" | "image" | "timestamp" | "rowversion" => -3,
        "char" => 1,
        "varchar" => 12,
        "nchar" => -15,
        "nvarchar" => -9,
        "text" => -1,
        "ntext" => -16,
        "xml" => 2009,
        _ => 1111,
    }
}

async fn list_functions(
    application: &Application,
    request: &ListRoutinesRequest,
) -> Result<FunctionList, AppError> {
    let database_name = &request.scope.database_name;
    let schema_name = effective_schema(&request.scope.schema_name);
    let rows = metadata_rows(
        application,
        &request.scope.datasource_id,
        Some(database_name),
        "SELECT s.name,o.name,o.type,COALESCE(CONVERT(nvarchar(max),ep.value),''),COALESCE(m.definition,'') \
         FROM sys.objects o JOIN sys.schemas s ON s.schema_id=o.schema_id \
         LEFT JOIN sys.sql_modules m ON m.object_id=o.object_id \
         LEFT JOIN sys.extended_properties ep ON ep.major_id=o.object_id AND ep.minor_id=0 AND ep.name='MS_Description' \
         WHERE s.name=@P1 AND o.type IN ('FN','IF','TF','FS','FT') ORDER BY o.name",
        vec![DatabaseValue::Text(schema_name.to_owned())],
    )
    .await?;
    rows.into_iter()
        .map(|row| function_metadata(database_name, &row))
        .collect::<Result<Vec<_>, _>>()
        .map(|items| FunctionList { items })
}

async fn get_function(
    application: &Application,
    request: &MetadataObjectRef,
) -> Result<FunctionMetadata, AppError> {
    let database_name = &request.scope.database_name;
    let schema_name = effective_schema(&request.scope.schema_name);
    validate_identifier(&request.object_name, "functionName")?;
    let rows = metadata_rows(
        application,
        &request.scope.datasource_id,
        Some(database_name),
        "SELECT s.name,o.name,o.type,COALESCE(CONVERT(nvarchar(max),ep.value),''),COALESCE(m.definition,'') \
         FROM sys.objects o JOIN sys.schemas s ON s.schema_id=o.schema_id \
         LEFT JOIN sys.sql_modules m ON m.object_id=o.object_id \
         LEFT JOIN sys.extended_properties ep ON ep.major_id=o.object_id AND ep.minor_id=0 AND ep.name='MS_Description' \
         WHERE s.name=@P1 AND o.name=@P2 AND o.type IN ('FN','IF','TF','FS','FT')",
        vec![
            DatabaseValue::Text(schema_name.to_owned()),
            DatabaseValue::Text(request.object_name.clone()),
        ],
    )
    .await?;
    rows.into_iter()
        .next()
        .ok_or_else(|| {
            metadata_not_found("function", database_name, schema_name, &request.object_name)
        })
        .and_then(|row| function_metadata(database_name, &row))
}

async fn list_function_parameters(
    application: &Application,
    request: &MetadataObjectRef,
) -> Result<FunctionParameterList, AppError> {
    let rows = routine_parameter_rows(application, request, true).await?;
    rows.into_iter()
        .map(|row| {
            let type_name = row_string(&row, 4)?;
            let parameter_id = row_i32(&row, 3)?;
            Ok(FunctionParameterMetadata {
                function_database: request.scope.database_name.clone(),
                function_schema: row_string(&row, 0)?,
                function_name: row_string(&row, 1)?,
                column_name: row_string(&row, 2)?,
                column_type: Some(if parameter_id == 0 {
                    4
                } else if row_bool(&row, 5)? {
                    3
                } else {
                    1
                }),
                data_type: Some(sqlserver_metadata_jdbc_type(&type_name)),
                type_name,
                precision: Some(row_i32(&row, 7)?),
                length: Some(row_i32(&row, 6)?),
                scale: Some(row_i32(&row, 8)?),
                radix: Some(10),
                nullable: Some(1),
                char_octet_length: Some(row_i32(&row, 6)?),
                ordinal_position: Some(parameter_id),
                is_nullable: "YES".to_owned(),
                specific_name: request.object_name.clone(),
                ..FunctionParameterMetadata::default()
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|items| FunctionParameterList { items })
}

async fn list_procedures(
    application: &Application,
    request: &ListRoutinesRequest,
) -> Result<ProcedureList, AppError> {
    let database_name = &request.scope.database_name;
    let schema_name = effective_schema(&request.scope.schema_name);
    let rows = metadata_rows(
        application,
        &request.scope.datasource_id,
        Some(database_name),
        "SELECT s.name,o.name,COALESCE(CONVERT(nvarchar(max),ep.value),''),COALESCE(m.definition,'') \
         FROM sys.objects o JOIN sys.schemas s ON s.schema_id=o.schema_id \
         LEFT JOIN sys.sql_modules m ON m.object_id=o.object_id \
         LEFT JOIN sys.extended_properties ep ON ep.major_id=o.object_id AND ep.minor_id=0 AND ep.name='MS_Description' \
         WHERE s.name=@P1 AND o.type IN ('P','PC','X') ORDER BY o.name",
        vec![DatabaseValue::Text(schema_name.to_owned())],
    )
    .await?;
    rows.into_iter()
        .map(|row| procedure_metadata(database_name, &row))
        .collect::<Result<Vec<_>, _>>()
        .map(|items| ProcedureList { items })
}

async fn get_procedure(
    application: &Application,
    request: &MetadataObjectRef,
) -> Result<ProcedureMetadata, AppError> {
    let database_name = &request.scope.database_name;
    let schema_name = effective_schema(&request.scope.schema_name);
    validate_identifier(&request.object_name, "procedureName")?;
    let rows = metadata_rows(
        application,
        &request.scope.datasource_id,
        Some(database_name),
        "SELECT s.name,o.name,COALESCE(CONVERT(nvarchar(max),ep.value),''),COALESCE(m.definition,'') \
         FROM sys.objects o JOIN sys.schemas s ON s.schema_id=o.schema_id \
         LEFT JOIN sys.sql_modules m ON m.object_id=o.object_id \
         LEFT JOIN sys.extended_properties ep ON ep.major_id=o.object_id AND ep.minor_id=0 AND ep.name='MS_Description' \
         WHERE s.name=@P1 AND o.name=@P2 AND o.type IN ('P','PC','X')",
        vec![
            DatabaseValue::Text(schema_name.to_owned()),
            DatabaseValue::Text(request.object_name.clone()),
        ],
    )
    .await?;
    rows.into_iter()
        .next()
        .ok_or_else(|| {
            metadata_not_found(
                "procedure",
                database_name,
                schema_name,
                &request.object_name,
            )
        })
        .and_then(|row| procedure_metadata(database_name, &row))
}

async fn list_procedure_parameters(
    application: &Application,
    request: &MetadataObjectRef,
) -> Result<ProcedureParameterList, AppError> {
    let rows = routine_parameter_rows(application, request, false).await?;
    rows.into_iter()
        .map(|row| {
            let type_name = row_string(&row, 4)?;
            let output = row_bool(&row, 5)?;
            let parameter_id = row_i32(&row, 3)?;
            Ok(ProcedureParameterMetadata {
                procedure_database: request.scope.database_name.clone(),
                procedure_schema: row_string(&row, 0)?,
                procedure_name: row_string(&row, 1)?,
                column_name: row_string(&row, 2)?,
                column_type: Some(if parameter_id == 0 {
                    5
                } else if output {
                    4
                } else {
                    1
                }),
                data_type: Some(sqlserver_metadata_jdbc_type(&type_name)),
                type_name,
                precision: Some(row_i32(&row, 7)?),
                length: Some(row_i32(&row, 6)?),
                scale: Some(row_i32(&row, 8)?),
                radix: Some(10),
                nullable: Some(1),
                column_default: row_string(&row, 9)?,
                char_octet_length: Some(row_i32(&row, 6)?),
                ordinal_position: Some(parameter_id),
                is_nullable: "YES".to_owned(),
                specific_name: request.object_name.clone(),
                ..ProcedureParameterMetadata::default()
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|items| ProcedureParameterList { items })
}

async fn list_triggers(
    application: &Application,
    request: &ListTriggersRequest,
) -> Result<TriggerList, AppError> {
    let rows = trigger_rows(application, request, None).await?;
    rows.into_iter()
        .map(|row| trigger_metadata(&request.scope.database_name, &row))
        .collect::<Result<Vec<_>, _>>()
        .map(|items| TriggerList { items })
}

async fn get_trigger(
    application: &Application,
    request: &MetadataObjectRef,
) -> Result<TriggerMetadata, AppError> {
    let trigger_request = ListTriggersRequest {
        scope: request.scope.clone(),
    };
    let rows = trigger_rows(application, &trigger_request, Some(&request.object_name)).await?;
    rows.into_iter()
        .next()
        .ok_or_else(|| {
            metadata_not_found(
                "trigger",
                &request.scope.database_name,
                effective_schema(&request.scope.schema_name),
                &request.object_name,
            )
        })
        .and_then(|row| trigger_metadata(&request.scope.database_name, &row))
}

fn function_metadata(database_name: &str, row: &Row) -> Result<FunctionMetadata, AppError> {
    let body = row_string(row, 4)?;
    Ok(FunctionMetadata {
        database_name: database_name.to_owned(),
        schema_name: row_string(row, 0)?,
        name: row_string(row, 1)?,
        function_type: Some(
            if matches!(row_string(row, 2)?.as_str(), "IF" | "TF" | "FT") {
                2
            } else {
                1
            },
        ),
        remarks: row_string(row, 3)?,
        specific_name: row_string(row, 1)?,
        template: body.clone(),
        body,
    })
}

fn procedure_metadata(database_name: &str, row: &Row) -> Result<ProcedureMetadata, AppError> {
    Ok(ProcedureMetadata {
        database_name: database_name.to_owned(),
        schema_name: row_string(row, 0)?,
        name: row_string(row, 1)?,
        remarks: row_string(row, 2)?,
        procedure_type: Some(2),
        specific_name: row_string(row, 1)?,
        body: row_string(row, 3)?,
    })
}

async fn routine_parameter_rows(
    application: &Application,
    request: &MetadataObjectRef,
    function: bool,
) -> Result<Vec<Row>, AppError> {
    let schema_name = effective_schema(&request.scope.schema_name);
    let object_types = if function {
        "'FN','IF','TF','FS','FT'"
    } else {
        "'P','PC','X'"
    };
    let sql = format!(
        "SELECT s.name,o.name,COALESCE(p.name,''),CONVERT(int,p.parameter_id),ty.name,p.is_output, \
         CONVERT(int,p.max_length),CONVERT(int,p.precision),CONVERT(int,p.scale), \
         COALESCE(TRY_CONVERT(nvarchar(max),p.default_value),'') \
         FROM sys.objects o JOIN sys.schemas s ON s.schema_id=o.schema_id \
         JOIN sys.parameters p ON p.object_id=o.object_id JOIN sys.types ty ON ty.user_type_id=p.user_type_id \
         WHERE s.name=@P1 AND o.name=@P2 AND o.type IN ({object_types}) ORDER BY p.parameter_id"
    );
    metadata_rows(
        application,
        &request.scope.datasource_id,
        Some(&request.scope.database_name),
        &sql,
        vec![
            DatabaseValue::Text(schema_name.to_owned()),
            DatabaseValue::Text(request.object_name.clone()),
        ],
    )
    .await
}

async fn trigger_rows(
    application: &Application,
    request: &ListTriggersRequest,
    trigger_name: Option<&str>,
) -> Result<Vec<Row>, AppError> {
    let schema_name = effective_schema(&request.scope.schema_name);
    let name = trigger_name.unwrap_or_default();
    metadata_rows(
        application,
        &request.scope.datasource_id,
        Some(&request.scope.database_name),
        "SELECT s.name,tr.name,COALESCE(STRING_AGG(te.type_desc,','),''),COALESCE(m.definition,'') \
         FROM sys.triggers tr JOIN sys.tables t ON t.object_id=tr.parent_id JOIN sys.schemas s ON s.schema_id=t.schema_id \
         LEFT JOIN sys.trigger_events te ON te.object_id=tr.object_id \
         LEFT JOIN sys.sql_modules m ON m.object_id=tr.object_id \
         WHERE s.name=@P1 AND (@P2='' OR tr.name=@P2) GROUP BY s.name,tr.name,m.definition ORDER BY tr.name",
        vec![
            DatabaseValue::Text(schema_name.to_owned()),
            DatabaseValue::Text(name.to_owned()),
        ],
    )
    .await
}

fn trigger_metadata(database_name: &str, row: &Row) -> Result<TriggerMetadata, AppError> {
    Ok(TriggerMetadata {
        database_name: database_name.to_owned(),
        schema_name: row_string(row, 0)?,
        name: row_string(row, 1)?,
        event_manipulation: row_string(row, 2)?,
        body: row_string(row, 3)?,
    })
}

async fn load_er_tables(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
) -> Result<Vec<EntityRelationTable>, AppError> {
    let schema_name = effective_schema(schema_name).to_owned();
    let tables = list_tables(application, datasource_id, database_name, &schema_name, "").await?;
    let mut result = Vec::with_capacity(tables.items.len());
    for table in tables.items {
        let columns = list_columns(
            application,
            datasource_id,
            database_name,
            &schema_name,
            &table.name,
        )
        .await?;
        let keys_request =
            table_keys_request(datasource_id, database_name, &schema_name, &table.name);
        let foreign_keys = list_foreign_keys(application, &keys_request, false).await?;
        result.push(EntityRelationTable {
            name: table.name,
            comment: table.comment,
            columns: columns
                .items
                .into_iter()
                .map(|column| EntityRelationColumn {
                    name: column.name,
                    column_type: column.column_type,
                    primary_key: column.primary_key.unwrap_or(false),
                    comment: column.comment,
                })
                .collect(),
            foreign_keys: foreign_keys
                .items
                .into_iter()
                .map(|key| EntityRelationForeignKey {
                    primary_table: key.primary_table_name,
                    primary_column: key.primary_column_name,
                    foreign_table: key.foreign_table_name,
                    foreign_column: key.foreign_column_name,
                })
                .collect(),
        });
    }
    Ok(result)
}

async fn table_ddl(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> Result<String, AppError> {
    let schema_name = effective_schema(schema_name);
    let columns = list_columns(
        application,
        datasource_id,
        database_name,
        schema_name,
        table_name,
    )
    .await?;
    if columns.items.is_empty() {
        return Err(metadata_not_found(
            "table",
            database_name,
            schema_name,
            table_name,
        ));
    }
    let keys_request = table_keys_request(datasource_id, database_name, schema_name, table_name);
    let primary_keys = list_primary_keys(application, &keys_request).await?;
    let foreign_keys = list_foreign_keys(application, &keys_request, false).await?;
    let indexes = list_indexes(
        application,
        datasource_id,
        database_name,
        schema_name,
        table_name,
    )
    .await?;
    render_table_ddl(
        database_name,
        schema_name,
        table_name,
        columns,
        &primary_keys,
        foreign_keys,
        indexes,
    )
}

fn table_keys_request(
    datasource_id: &str,
    database_name: &str,
    schema_name: &str,
    table_name: &str,
) -> ListTableKeysRequest {
    ListTableKeysRequest {
        table: TableRef {
            scope: MetadataScope {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                schema_name: schema_name.to_owned(),
            },
            table_name: table_name.to_owned(),
        },
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "SQL Server table DDL is rendered in dependency order from one metadata snapshot"
)]
fn render_table_ddl(
    database_name: &str,
    schema_name: &str,
    table_name: &str,
    columns: ColumnList,
    primary_keys: &PrimaryKeyList,
    foreign_keys: ForeignKeyList,
    indexes: IndexList,
) -> Result<String, AppError> {
    let target = qualified_table(database_name, schema_name, table_name)?;
    let mut definitions = Vec::new();
    for column in columns.items {
        let name = quote_identifier(&column.name, "columnName")?;
        if column.generated_column.unwrap_or(false) {
            if column.extent.trim().is_empty() {
                return Err(AppError::invalid(
                    "sqlserver_ddl_unavailable",
                    format!(
                        "Computed column {} has no recoverable definition",
                        column.name
                    ),
                ));
            }
            definitions.push(format!("    {name} AS {}", column.extent));
            continue;
        }
        let mut definition = format!("    {name} {}", column.column_type);
        if !column.collation.is_empty() && sqlserver_textual_jdbc_type(column.data_type) {
            definition.push_str(" COLLATE ");
            definition.push_str(&quote_identifier(&column.collation, "collation")?);
        }
        if column.sparse.unwrap_or(false) {
            definition.push_str(" SPARSE");
        }
        if column.auto_increment.unwrap_or(false) {
            let _ = write!(
                definition,
                " IDENTITY({},{})",
                column.seed.unwrap_or(1),
                column.increment.unwrap_or(1)
            );
        }
        if let Some(default_value) = column
            .default_value
            .filter(|value| !value.trim().is_empty())
        {
            if !column.default_constraint_name.is_empty() {
                definition.push_str(" CONSTRAINT ");
                definition.push_str(&quote_identifier(
                    &column.default_constraint_name,
                    "defaultConstraintName",
                )?);
            }
            definition.push_str(" DEFAULT ");
            definition.push_str(&default_value);
        }
        definition.push_str(if column.nullable == Some(1) {
            " NULL"
        } else {
            " NOT NULL"
        });
        definitions.push(definition);
    }

    if !primary_keys.items.is_empty() {
        let constraint_name = primary_keys.items[0].name.clone();
        let columns = primary_keys
            .items
            .iter()
            .map(|key| quote_identifier(&key.column_name, "primaryKeyColumn"))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        definitions.push(format!(
            "    CONSTRAINT {} PRIMARY KEY ({columns})",
            quote_identifier(&constraint_name, "primaryKeyName")?
        ));
    }

    let mut foreign_key_order = Vec::<String>::new();
    let mut grouped_foreign_keys = HashMap::<String, Vec<ForeignKeyMetadata>>::new();
    for key in foreign_keys.items {
        if !grouped_foreign_keys.contains_key(&key.foreign_key_name) {
            foreign_key_order.push(key.foreign_key_name.clone());
        }
        grouped_foreign_keys
            .entry(key.foreign_key_name.clone())
            .or_default()
            .push(key);
    }
    for name in foreign_key_order {
        let mut keys = grouped_foreign_keys.remove(&name).unwrap_or_default();
        keys.sort_by_key(|key| key.key_sequence);
        let Some(first) = keys.first() else {
            continue;
        };
        let local_columns = keys
            .iter()
            .map(|key| quote_identifier(&key.foreign_column_name, "foreignColumn"))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let referenced_columns = keys
            .iter()
            .map(|key| quote_identifier(&key.primary_column_name, "primaryColumn"))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let referenced_table = format!(
            "{}.{}.{}",
            quote_identifier(&first.primary_table_database, "primaryDatabase")?,
            quote_identifier(&first.primary_table_schema, "primarySchema")?,
            quote_identifier(&first.primary_table_name, "primaryTable")?
        );
        let mut definition = format!(
            "    CONSTRAINT {} FOREIGN KEY ({local_columns}) REFERENCES {referenced_table} ({referenced_columns})",
            quote_identifier(&name, "foreignKeyName")?
        );
        append_referential_action(&mut definition, "UPDATE", first.update_rule);
        append_referential_action(&mut definition, "DELETE", first.delete_rule);
        definitions.push(definition);
    }

    let mut ddl = format!("CREATE TABLE {target} (\n{}\n);", definitions.join(",\n"));
    let primary_name = primary_keys.items.first().map(|key| key.name.as_str());
    for index in indexes.items {
        if primary_name.is_some_and(|name| name.eq_ignore_ascii_case(&index.name)) {
            continue;
        }
        let key_columns = index
            .columns
            .iter()
            .filter(|column| column.column_type == "KEY")
            .map(|column| {
                quote_identifier(&column.column_name, "indexColumn").map(|name| {
                    format!(
                        "{name} {}",
                        if column.sort_order == "D" {
                            "DESC"
                        } else {
                            "ASC"
                        }
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if key_columns.is_empty() {
            continue;
        }
        ddl.push_str("\n\nCREATE ");
        if index.unique.unwrap_or(false) {
            ddl.push_str("UNIQUE ");
        }
        if index.index_type.contains("CLUSTERED") && !index.index_type.contains("NONCLUSTERED") {
            ddl.push_str("CLUSTERED ");
        } else {
            ddl.push_str("NONCLUSTERED ");
        }
        ddl.push_str("INDEX ");
        ddl.push_str(&quote_identifier(&index.name, "indexName")?);
        ddl.push_str(" ON ");
        ddl.push_str(&target);
        ddl.push_str(" (");
        ddl.push_str(&key_columns.join(", "));
        ddl.push(')');
        let included = index
            .columns
            .iter()
            .filter(|column| column.column_type == "INCLUDED")
            .map(|column| quote_identifier(&column.column_name, "includedColumn"))
            .collect::<Result<Vec<_>, _>>()?;
        if !included.is_empty() {
            ddl.push_str(" INCLUDE (");
            ddl.push_str(&included.join(", "));
            ddl.push(')');
        }
        if let Some(filter) = index
            .columns
            .first()
            .map(|column| column.filter_condition.trim())
            .filter(|filter| !filter.is_empty())
        {
            ddl.push_str(" WHERE ");
            ddl.push_str(filter);
        }
        ddl.push(';');
    }
    Ok(ddl)
}

fn sqlserver_textual_jdbc_type(jdbc_type: Option<i32>) -> bool {
    matches!(jdbc_type, Some(1 | 12 | -1 | -9 | -15 | -16))
}

fn append_referential_action(sql: &mut String, action: &str, rule: i32) {
    let rule = match rule {
        0 => Some("CASCADE"),
        2 => Some("SET NULL"),
        4 => Some("SET DEFAULT"),
        _ => None,
    };
    if let Some(rule) = rule {
        sql.push_str(" ON ");
        sql.push_str(action);
        sql.push(' ');
        sql.push_str(rule);
    }
}

async fn start_table_preview(
    application: &Application,
    request: TablePreviewRequest,
    row_limit: u32,
) -> Result<TablePreviewAccepted, AppError> {
    let table = qualified_table(
        &request.table.scope.database_name,
        effective_schema(&request.table.scope.schema_name),
        &request.table.table_name,
    )?;
    let sql = format!("SELECT TOP ({row_limit}) * FROM {table}");
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

#[cfg(test)]
mod tests {
    use chat2db_contract::DatasourceConnectionProperty;

    use super::*;

    #[test]
    fn descriptor_exposes_only_implemented_native_capabilities() {
        let driver = SqlServerNativeDriver;

        assert_eq!(driver.descriptor(), &SQLSERVER_DRIVER_DESCRIPTOR);
        assert_eq!(driver.descriptor().implementation, "tiberius");
        assert!(driver.connection().is_some());
        assert!(driver.query().is_some());
        assert!(driver.metadata().is_some());
        assert!(driver.tables().is_some());
        assert!(driver.routines().is_none());
        assert!(driver.transfer().is_none());
        assert!(driver.dialect().is_some());
        assert!(driver.administration().is_none());
        assert!(driver.schema_diff().is_none());
    }

    #[test]
    fn namespace_builders_render_safe_sqlserver_batches() {
        let driver = SqlServerNativeDriver;
        let schema = driver
            .build_create_schema(CreateSchemaSqlRequest {
                schema: SchemaDefinition {
                    database_name: "sales]archive".to_owned(),
                    name: "reporting]daily".to_owned(),
                    comment: "owner's notes".to_owned(),
                    owner: "db_owner".to_owned(),
                    system: false,
                },
            })
            .expect("SQL Server schema SQL should render");
        assert_eq!(
            schema.sql,
            "USE [sales]]archive];\nEXEC(N'CREATE SCHEMA [reporting]]daily] AUTHORIZATION [db_owner]');\nEXEC sys.sp_addextendedproperty @name = N'MS_Description', @value = N'owner''s notes', @level0type = N'SCHEMA', @level0name = N'reporting]daily';"
        );

        let database = driver
            .build_namespace_sql(NamespaceSqlRequest {
                operation: NamespaceSqlOperation::CreateDatabase {
                    database: database_definition("inventory]2026", "initial owner's note"),
                },
            })
            .expect("SQL Server database SQL should render");
        assert_eq!(
            database.sql,
            "CREATE DATABASE [inventory]]2026] COLLATE Latin1_General_100_CI_AS_SC_UTF8;\nALTER AUTHORIZATION ON DATABASE::[inventory]]2026] TO [sa];\nEXEC [inventory]]2026].sys.sp_addextendedproperty @name = N'MS_Description', @value = N'initial owner''s note';"
        );

        let use_database = driver
            .build_namespace_sql(NamespaceSqlRequest {
                operation: NamespaceSqlOperation::UseDatabase {
                    database_name: "inventory]2026".to_owned(),
                },
            })
            .expect("SQL Server USE SQL should render");
        assert_eq!(use_database.sql, "USE [inventory]]2026];");
    }

    #[test]
    fn database_alter_builder_covers_rename_collation_owner_and_comment() {
        let driver = SqlServerNativeDriver;
        let old_database = database_definition("inventory", "old note");
        let mut new_database = database_definition("inventory_archive", "new owner's note");
        new_database.collation = "SQL_Latin1_General_CP1_CI_AS".to_owned();
        new_database.owner = "archive_owner".to_owned();
        let built = driver
            .build_namespace_sql(NamespaceSqlRequest {
                operation: NamespaceSqlOperation::AlterDatabase {
                    old_database,
                    new_database,
                },
            })
            .expect("supported SQL Server database changes should render");
        assert_eq!(
            built.sql,
            "ALTER DATABASE [inventory] MODIFY NAME = [inventory_archive];\nALTER DATABASE [inventory_archive] COLLATE SQL_Latin1_General_CP1_CI_AS;\nALTER AUTHORIZATION ON DATABASE::[inventory_archive] TO [archive_owner];\nEXEC [inventory_archive].sys.sp_updateextendedproperty @name = N'MS_Description', @value = N'new owner''s note';"
        );

        let mut clear_comment = database_definition("inventory", "old note");
        clear_comment.comment.clear();
        let built = driver
            .build_namespace_sql(NamespaceSqlRequest {
                operation: NamespaceSqlOperation::AlterDatabase {
                    old_database: database_definition("inventory", "old note"),
                    new_database: clear_comment,
                },
            })
            .expect("SQL Server database comments should be removable");
        assert_eq!(
            built.sql,
            "EXEC [inventory].sys.sp_dropextendedproperty @name = N'MS_Description';"
        );
    }

    #[test]
    fn namespace_builder_rejects_unsupported_or_unsafe_changes() {
        let driver = SqlServerNativeDriver;
        let charset = driver
            .build_namespace_sql(NamespaceSqlRequest {
                operation: NamespaceSqlOperation::CreateDatabase {
                    database: DatabaseDefinition {
                        charset: "UTF-8".to_owned(),
                        ..database_definition("inventory", "")
                    },
                },
            })
            .expect_err("SQL Server has no independent database charset");
        assert_eq!(
            charset.api_error().code,
            "sqlserver_database_charset_unsupported"
        );

        let collation = driver
            .build_namespace_sql(NamespaceSqlRequest {
                operation: NamespaceSqlOperation::CreateDatabase {
                    database: DatabaseDefinition {
                        collation: "Latin1_General_CI_AS; DROP DATABASE master".to_owned(),
                        ..database_definition("inventory", "")
                    },
                },
            })
            .expect_err("unsafe SQL Server collations must be rejected");
        assert_eq!(
            collation.api_error().code,
            "invalid_sqlserver_dialect_request"
        );

        let rename = driver
            .build_namespace_sql(NamespaceSqlRequest {
                operation: NamespaceSqlOperation::AlterSchema {
                    old_schema_name: "old_schema".to_owned(),
                    new_schema_name: "new_schema".to_owned(),
                },
            })
            .expect_err("SQL Server schema rename must not silently degrade");
        assert_eq!(
            rename.api_error().code,
            "sqlserver_schema_rename_unsupported"
        );
    }

    #[test]
    fn typed_dml_builder_renders_three_part_unicode_and_binary_values() {
        let driver = SqlServerNativeDriver;
        let columns = vec![
            dml_column("label", "nvarchar"),
            dml_column("amount", "decimal"),
            dml_column("active", "bit"),
            dml_column("created_at", "datetimeoffset"),
            dml_column("payload", "varbinary"),
        ];
        let built = driver
            .build_dml(DmlSqlRequest {
                target: DmlTarget {
                    database_name: Some("sales]archive".to_owned()),
                    schema_name: Some("dbo".to_owned()),
                    table_name: "order]items".to_owned(),
                },
                statement: DmlStatement::SingleInsert {
                    columns,
                    row: DmlRow {
                        values: vec![
                            DmlValue::String("O'Brien".to_owned()),
                            DmlValue::Decimal("+001.20".to_owned()),
                            DmlValue::Boolean(true),
                            DmlValue::Temporal {
                                kind: DmlTemporalKind::OffsetDatetime,
                                iso8601: "2026-08-07T12:30:45.1234567+08:00".to_owned(),
                            },
                            DmlValue::Binary(vec![0, 255]),
                        ],
                    },
                },
            })
            .expect("typed SQL Server INSERT should render");
        assert_eq!(
            built.sql,
            "INSERT INTO [sales]]archive].[dbo].[order]]items] ([label], [amount], [active], [created_at], [payload]) VALUES\n(N'O''Brien', 1.20, 1, CAST(N'2026-08-07T12:30:45.1234567+08:00' AS datetimeoffset), 0x00ff);"
        );

        let update = driver
            .build_dml(DmlSqlRequest {
                target: DmlTarget {
                    database_name: Some("sales".to_owned()),
                    schema_name: None,
                    table_name: "items".to_owned(),
                },
                statement: DmlStatement::Update {
                    assignments: vec![DmlAssignment {
                        column: dml_column("label", "nvarchar"),
                        value: DmlValue::String("updated".to_owned()),
                    }],
                    predicates: vec![DmlAssignment {
                        column: dml_column("deleted_at", "datetime2"),
                        value: DmlValue::Null,
                    }],
                },
            })
            .expect("typed SQL Server UPDATE should render");
        assert_eq!(
            update.sql,
            "UPDATE [sales]..[items] SET [label] = N'updated' WHERE [deleted_at] IS NULL;"
        );
    }

    #[test]
    fn typed_dml_builder_fails_closed_for_invalid_values_and_shapes() {
        let driver = SqlServerNativeDriver;
        let target = DmlTarget {
            database_name: None,
            schema_name: Some("dbo".to_owned()),
            table_name: "items".to_owned(),
        };
        let decimal = driver
            .build_dml(DmlSqlRequest {
                target: target.clone(),
                statement: DmlStatement::SingleInsert {
                    columns: vec![dml_column("amount", "decimal")],
                    row: DmlRow {
                        values: vec![DmlValue::Decimal("1; DROP TABLE items".to_owned())],
                    },
                },
            })
            .expect_err("invalid SQL Server decimals must be rejected");
        assert_eq!(decimal.api_error().code, "invalid_sqlserver_dml");

        let shape = driver
            .build_dml(DmlSqlRequest {
                target: target.clone(),
                statement: DmlStatement::MultiInsert {
                    columns: vec![dml_column("id", "int")],
                    rows: vec![DmlRow { values: Vec::new() }],
                },
            })
            .expect_err("SQL Server row shape mismatches must be rejected");
        assert_eq!(shape.api_error().code, "invalid_sqlserver_dml");

        let update = driver
            .build_dml(DmlSqlRequest {
                target,
                statement: DmlStatement::Update {
                    assignments: vec![DmlAssignment {
                        column: dml_column("label", "nvarchar"),
                        value: DmlValue::String("unsafe".to_owned()),
                    }],
                    predicates: Vec::new(),
                },
            })
            .expect_err("unbounded SQL Server updates must be rejected");
        assert_eq!(update.api_error().code, "invalid_sqlserver_dml");
    }

    fn database_definition(name: &str, comment: &str) -> DatabaseDefinition {
        DatabaseDefinition {
            name: name.to_owned(),
            comment: comment.to_owned(),
            charset: String::new(),
            collation: "Latin1_General_100_CI_AS_SC_UTF8".to_owned(),
            owner: "sa".to_owned(),
            system: false,
        }
    }

    fn dml_column(name: &str, data_type_name: &str) -> DmlColumn {
        DmlColumn {
            name: name.to_owned(),
            data_type_name: data_type_name.to_owned(),
            precision: None,
            scale: None,
        }
    }

    #[test]
    fn connection_url_properties_and_ssh_target_are_normalized_safely() {
        assert_eq!(
            normalize_sqlserver_url("  JDBC:SQLSERVER://db.example:1433;databaseName=inventory  ")
                .expect("JDBC URL should normalize"),
            "jdbc:sqlserver://db.example:1433;databaseName=inventory"
        );
        assert_eq!(
            normalize_sqlserver_url("sqlserver://db.example:1433")
                .expect("native URL should normalize"),
            "jdbc:sqlserver://db.example:1433"
        );
        assert!(normalize_sqlserver_url("postgres://db.example:5432").is_err());

        assert_eq!(encode_jdbc_property("plain"), "plain");
        assert_eq!(encode_jdbc_property(" a;b=c}"), "{ a;b=c}}}");
        assert_eq!(
            sqlserver_target("jdbc:sqlserver://db.example:15433;databaseName=master")
                .expect("IPv4 target should parse"),
            ("db.example".to_owned(), 15_433)
        );
        assert_eq!(
            sqlserver_target("sqlserver://[2001:db8::10]:1433").expect("IPv6 target should parse"),
            ("2001:db8::10".to_owned(), 1433)
        );
        assert!(sqlserver_target("sqlserver://named-host\\instance").is_err());

        let config = connection_config(&DatasourceConnection {
            jdbc_url: "sqlserver://localhost:1433;encrypt=false".to_owned(),
            properties: vec![
                DatasourceConnectionProperty {
                    key: "user".to_owned(),
                    value: "sa".to_owned(),
                    sensitive: false,
                },
                DatasourceConnectionProperty {
                    key: "password".to_owned(),
                    value: "secret;with=delimiters".to_owned(),
                    sensitive: true,
                },
            ],
            read_only: true,
            ssh: None,
        });
        assert!(config.is_ok());

        let tls_conflict = connection_config(&DatasourceConnection {
            jdbc_url: "sqlserver://localhost:1433;encrypt=true;trustServerCertificate=true"
                .to_owned(),
            properties: vec![DatasourceConnectionProperty {
                key: "trustServerCertificateCA".to_owned(),
                value: "/tmp/sqlserver-ca.pem".to_owned(),
                sensitive: false,
            }],
            read_only: false,
            ssh: None,
        });
        let Err(error) = tls_conflict else {
            panic!("conflicting SQL Server trust properties must be rejected");
        };
        assert_eq!(error.api_error().code, "invalid_sqlserver_connection");
    }

    #[test]
    fn positional_parameters_are_ordered_and_rewritten_outside_literals() {
        let parameters = vec![
            QueryParameter {
                position: 2,
                value: DatabaseValue::Text("second".to_owned()),
            },
            QueryParameter {
                position: 1,
                value: DatabaseValue::SignedInteger(1),
            },
        ];
        let ordered = ordered_parameters(&parameters).expect("parameters should order");
        assert_eq!(ordered[0], &DatabaseValue::SignedInteger(1));
        assert_eq!(ordered[1], &DatabaseValue::Text("second".to_owned()));

        let sql = "SELECT ?, '?', \"?\", [?] -- ?\n, ? /* ? */";
        assert_eq!(
            rewrite_positional_parameters(sql, 2).expect("markers should rewrite"),
            "SELECT @P1, '?', \"?\", [?] -- ?\n, @P2 /* ? */"
        );
        assert!(rewrite_positional_parameters("SELECT ?, ?", 1).is_err());
        assert!(
            ordered_parameters(&[
                QueryParameter {
                    position: 1,
                    value: DatabaseValue::Null,
                },
                QueryParameter {
                    position: 3,
                    value: DatabaseValue::Null,
                },
            ])
            .is_err()
        );
    }

    #[test]
    fn read_policy_and_console_splitter_respect_sql_lexical_boundaries() {
        assert!(validate_read_sql("SELECT 'INTO' AS keyword_value;").is_ok());
        assert!(validate_read_sql("WITH source AS (SELECT 1 AS id) SELECT id FROM source").is_ok());
        assert!(
            validate_read_sql(
                "SELECT SUM(CONVERT(bigint, a.object_id % 2)) FROM sys.all_objects AS a CROSS JOIN sys.all_objects AS b"
            )
            .is_ok()
        );
        assert!(validate_read_sql("SELECT id INTO copied FROM source").is_err());
        assert!(validate_read_sql("UPDATE source SET id = 2").is_err());
        assert!(
            validate_read_sql(
                "WITH target AS (SELECT id FROM source) DELETE FROM target WHERE id = 1"
            )
            .is_err()
        );
        assert!(
            validate_read_sql("WITH target AS (SELECT id FROM source) UPDATE target SET id = 2")
                .is_err()
        );
        assert!(
            validate_read_sql(
                "WITH target AS (SELECT id FROM source) INSERT INTO copied SELECT id FROM target"
            )
            .is_err()
        );
        assert!(
            validate_read_sql(
                "WITH target AS (SELECT id FROM source) MERGE copied AS c USING target AS t ON c.id=t.id WHEN MATCHED THEN DELETE"
            )
            .is_err()
        );
        assert!(validate_read_sql("SELECT 1; SELECT 2").is_err());
        assert!(validate_read_sql("SELECT 1 /* unterminated").is_err());

        let statements = split_sqlserver_script(
            "SELECT ';' AS literal; -- keep ; here\nSELECT [semi;colon] FROM [table]; /* ; */ SELECT 3",
        )
        .expect("console script should split");
        assert_eq!(statements.len(), 3);
        assert_eq!(statements[0], "SELECT ';' AS literal");
        assert!(statements[1].contains("SELECT [semi;colon] FROM [table]"));
        assert!(statements[2].contains("SELECT 3"));
        assert!(split_sqlserver_script("SELECT 'unterminated").is_err());
        assert!(creates_local_temp_table(
            "CREATE TABLE #native_temp(id int NOT NULL)"
        ));
        assert!(creates_local_temp_table(
            "CREATE TABLE [#native temp](id int NOT NULL)"
        ));
        assert!(!creates_local_temp_table(
            "CREATE TABLE dbo.native_temp(id int NOT NULL)"
        ));
        assert!(!creates_local_temp_table(
            "CREATE TABLE ##native_global_temp(id int NOT NULL)"
        ));
        for statement in [
            "EXEC sys.sp_addextendedproperty @name=N'MS_Description', @value=N'note'",
            "EXECUTE sys.sp_updateextendedproperty @name=N'MS_Description', @value=N'note'",
            "EXEC [inventory].sys.sp_dropextendedproperty @name=N'MS_Description'",
        ] {
            assert!(!is_tabular_statement(statement));
        }
        assert!(is_tabular_statement("EXEC dbo.procedure_that_selects"));
        assert!(is_tabular_statement(
            "EXEC dbo.procedure_that_selects @sp_addextendedproperty = 1"
        ));

        for statement in [
            "SELECT id INTO copied FROM source",
            "WITH target AS (SELECT id FROM source) DELETE FROM target OUTPUT deleted.id",
        ] {
            let read_only = validate_read_sql(statement).is_ok();
            assert!(!read_only);
            assert!(matches!(
                console_execution_interrupted(read_only, Some("cancelled".to_owned())),
                ConsoleExecutionError::WriteOutcomeUnknown
            ));
            assert!(matches!(
                console_driver_interrupted(read_only),
                ConsoleExecutionError::WriteOutcomeUnknown
            ));
        }
    }

    #[test]
    fn dispatched_console_failures_preserve_read_errors_and_fence_write_retries() {
        let transport = TiberiusError::Io {
            kind: std::io::ErrorKind::ConnectionReset,
            message: "connection reset after dispatch".to_owned(),
        };
        assert!(matches!(
            console_dispatched_driver_error(false, transport),
            ConsoleExecutionError::WriteOutcomeUnknown
        ));
        assert!(matches!(
            console_dispatched_driver_error(
                false,
                TiberiusError::Protocol("invalid response after dispatch".into())
            ),
            ConsoleExecutionError::WriteOutcomeUnknown
        ));

        let read_error = console_dispatched_driver_error(
            true,
            TiberiusError::Io {
                kind: std::io::ErrorKind::ConnectionReset,
                message: "connection reset".to_owned(),
            },
        );
        assert!(matches!(
            read_error,
            ConsoleExecutionError::Statement(error)
                if error.api_error().code == "sqlserver_query_failed"
        ));
        assert!(matches!(
            console_post_dispatch_error(false, AppError::internal()),
            ConsoleExecutionError::WriteOutcomeUnknown
        ));
        assert!(matches!(
            console_post_dispatch_error(true, AppError::internal()),
            ConsoleExecutionError::Statement(_)
        ));
    }

    #[test]
    fn unsafe_tiberius_result_types_and_oversized_scalars_fail_closed() {
        for (type_id, type_name, user_type_name) in [
            (Some(60), "money", ""),
            (Some(122), "smallmoney", ""),
            (Some(98), "sql_variant", ""),
            (Some(240), "hierarchyid", "hierarchyid"),
        ] {
            let error = validate_described_result_type(type_id, type_name, user_type_name)
                .expect_err("unsafe Tiberius result types must be rejected");
            assert_eq!(error.api_error().code, "sqlserver_result_type_unsupported");
        }
        assert_eq!(
            sqlserver_value_type(ColumnType::Money),
            wire::JdbcValueType::Float64
        );
        assert_eq!(
            sqlserver_value_type(ColumnType::Money4),
            wire::JdbcValueType::Float32
        );
        let oversized = ColumnData::String(Some(std::borrow::Cow::Owned(
            "x".repeat(MAX_SCALAR_BYTES + 1),
        )));
        let error = wire_value(oversized).expect_err("oversized scalar must be rejected");
        assert_eq!(error.api_error().code, "sqlserver_scalar_too_large");
    }

    #[test]
    fn datetimeoffset_values_restore_local_time_and_reject_overflow() {
        fn date(year: i32, month: u32, day: u32) -> tiberius::time::Date {
            let epoch = NaiveDate::from_ymd_opt(1, 1, 1).expect("valid TDS date epoch");
            let value = NaiveDate::from_ymd_opt(year, month, day).expect("valid test date");
            let days = value.signed_duration_since(epoch).num_days();
            tiberius::time::Date::new(u32::try_from(days).expect("positive TDS day count"))
        }

        let exact = tiberius::time::DateTimeOffset::new(
            tiberius::time::DateTime2::new(
                date(2026, 8, 7),
                tiberius::time::Time::new(16_496_123_456, 6),
            ),
            480,
        );
        assert_eq!(
            format_datetime_offset(exact).expect("valid datetimeoffset"),
            "2026-08-07T12:34:56.123456+08:00"
        );

        let next_day = tiberius::time::DateTimeOffset::new(
            tiberius::time::DateTime2::new(
                date(2026, 12, 31),
                tiberius::time::Time::new(73_800, 0),
            ),
            330,
        );
        assert_eq!(
            format_datetime_offset(next_day).expect("valid cross-day datetimeoffset"),
            "2027-01-01T02:00:00+05:30"
        );

        let overflow = tiberius::time::DateTimeOffset::new(
            tiberius::time::DateTime2::new(
                date(9999, 12, 31),
                tiberius::time::Time::new(86_399, 0),
            ),
            60,
        );
        assert!(format_datetime_offset(overflow).is_err());
    }

    #[test]
    fn decimal_and_temporal_parameters_enforce_sqlserver_limits() {
        assert_eq!(
            format_numeric(parse_decimal("-123.450").expect("decimal should parse")),
            "-123.450"
        );
        assert_eq!(
            format_numeric(parse_decimal(".5").expect("fraction should parse")),
            "0.5"
        );
        assert!(parse_decimal("1.2.3").is_err());
        assert!(parse_decimal("123456789012345678901234567890123456789").is_err());
        assert!(parse_timestamp("2026-08-07T11:22:33.1234567").is_ok());
        assert!(parse_timestamp("2026/08/07 11:22:33").is_err());
        assert!(validate_parameter(&DatabaseValue::UnsignedInteger(i64::MAX as u64 + 1)).is_err());
    }

    #[test]
    fn table_ddl_preserves_identity_constraints_foreign_keys_and_indexes() {
        let columns = ColumnList {
            items: vec![
                ColumnMetadata {
                    name: "id".to_owned(),
                    column_type: "int".to_owned(),
                    data_type: Some(4),
                    nullable: Some(0),
                    auto_increment: Some(true),
                    seed: Some(10),
                    increment: Some(5),
                    ..ColumnMetadata::default()
                },
                ColumnMetadata {
                    name: "name".to_owned(),
                    column_type: "nvarchar(80)".to_owned(),
                    data_type: Some(-9),
                    nullable: Some(1),
                    default_value: Some("(N'')".to_owned()),
                    default_constraint_name: "DF_child_name".to_owned(),
                    collation: "Latin1_General_100_CI_AS".to_owned(),
                    ..ColumnMetadata::default()
                },
                ColumnMetadata {
                    name: "slug".to_owned(),
                    generated_column: Some(true),
                    extent: "LOWER([name])".to_owned(),
                    ..ColumnMetadata::default()
                },
            ],
        };
        let primary_keys = PrimaryKeyList {
            items: vec![PrimaryKeyMetadata {
                column_name: "id".to_owned(),
                name: "PK_child".to_owned(),
                ..PrimaryKeyMetadata::default()
            }],
        };
        let foreign_keys = ForeignKeyList {
            items: vec![ForeignKeyMetadata {
                primary_table_database: "master".to_owned(),
                primary_table_schema: "dbo".to_owned(),
                primary_table_name: "parent".to_owned(),
                primary_column_name: "id".to_owned(),
                foreign_column_name: "id".to_owned(),
                key_sequence: 1,
                update_rule: 0,
                delete_rule: 2,
                foreign_key_name: "FK_child_parent".to_owned(),
                ..ForeignKeyMetadata::default()
            }],
        };
        let indexes = IndexList {
            items: vec![IndexMetadata {
                name: "IX_child_name".to_owned(),
                index_type: "NONCLUSTERED".to_owned(),
                columns: vec![
                    IndexColumnMetadata {
                        column_name: "name".to_owned(),
                        column_type: "KEY".to_owned(),
                        sort_order: "D".to_owned(),
                        filter_condition: "[name] <> N''".to_owned(),
                        ..IndexColumnMetadata::default()
                    },
                    IndexColumnMetadata {
                        column_name: "slug".to_owned(),
                        column_type: "INCLUDED".to_owned(),
                        ..IndexColumnMetadata::default()
                    },
                ],
                ..IndexMetadata::default()
            }],
        };

        let ddl = render_table_ddl(
            "master",
            "dbo",
            "child",
            columns,
            &primary_keys,
            foreign_keys,
            indexes,
        )
        .expect("DDL should render");
        assert!(ddl.contains("CREATE TABLE [master].[dbo].[child]"));
        assert!(ddl.contains("[id] int IDENTITY(10,5) NOT NULL"));
        assert!(ddl.contains("[name] nvarchar(80) COLLATE [Latin1_General_100_CI_AS]"));
        assert!(ddl.contains("CONSTRAINT [PK_child] PRIMARY KEY ([id])"));
        assert!(ddl.contains("ON UPDATE CASCADE ON DELETE SET NULL"));
        assert!(ddl.contains("[slug] AS LOWER([name])"));
        assert!(ddl.contains(
            "CREATE NONCLUSTERED INDEX [IX_child_name] ON [master].[dbo].[child] ([name] DESC) INCLUDE ([slug]) WHERE [name] <> N'';"
        ));
    }

    async fn live_execute(client: &mut SqlServerClient, sql: &str) -> Result<(), AppError> {
        let mut stream = client
            .simple_query(sql)
            .await
            .map_err(sqlserver_query_error)?;
        while stream
            .try_next()
            .await
            .map_err(sqlserver_query_error)?
            .is_some()
        {}
        Ok(())
    }

    async fn live_cleanup(client: &mut SqlServerClient) {
        let cleanup = "DROP VIEW IF EXISTS dbo.chat2db_native_view; \
                       DROP FUNCTION IF EXISTS dbo.chat2db_native_function; \
                       DROP PROCEDURE IF EXISTS dbo.chat2db_native_procedure; \
                       DROP TABLE IF EXISTS dbo.chat2db_native_child; \
                       DROP TABLE IF EXISTS dbo.chat2db_native_parent;";
        if let Err(error) = live_execute(client, cleanup).await {
            tracing::warn!(error = %error, "SQL Server smoke cleanup failed");
        }
    }

    #[tokio::test]
    #[ignore = "requires SQLSERVER_TEST_HOST, SQLSERVER_TEST_PORT, SQLSERVER_TEST_USER, and SQLSERVER_TEST_PASSWORD"]
    #[allow(
        clippy::too_many_lines,
        reason = "the product smoke validates one complete SQL Server object lifecycle"
    )]
    async fn live_sqlserver_connection_query_and_catalog_smoke() {
        let host = std::env::var("SQLSERVER_TEST_HOST").expect("SQLSERVER_TEST_HOST is required");
        let port = std::env::var("SQLSERVER_TEST_PORT")
            .expect("SQLSERVER_TEST_PORT is required")
            .parse::<u16>()
            .expect("SQLSERVER_TEST_PORT must be a TCP port");
        let user = std::env::var("SQLSERVER_TEST_USER").expect("SQLSERVER_TEST_USER is required");
        let password =
            std::env::var("SQLSERVER_TEST_PASSWORD").expect("SQLSERVER_TEST_PASSWORD is required");
        let connection = DatasourceConnection {
            jdbc_url: format!(
                "jdbc:sqlserver://{host}:{port};databaseName=master;encrypt=false;trustServerCertificate=true"
            ),
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
        };
        let mut conn = open_connection(&connection)
            .await
            .expect("native SQL Server connection should open");
        live_cleanup(&mut conn.client).await;

        let smoke_result = async {
            live_execute(
                &mut conn.client,
                "CREATE TABLE dbo.chat2db_native_parent (id int NOT NULL CONSTRAINT PK_chat2db_native_parent PRIMARY KEY, label nvarchar(80) NULL);",
            )
            .await?;
            live_execute(
                &mut conn.client,
                "CREATE TABLE dbo.chat2db_native_child (id int NOT NULL CONSTRAINT PK_chat2db_native_child PRIMARY KEY, parent_id int NOT NULL, note nvarchar(80) NULL, CONSTRAINT FK_chat2db_native_child_parent FOREIGN KEY (parent_id) REFERENCES dbo.chat2db_native_parent(id)); CREATE INDEX IX_chat2db_native_child_note ON dbo.chat2db_native_child(note);",
            )
            .await?;
            live_execute(
                &mut conn.client,
                "CREATE VIEW dbo.chat2db_native_view AS SELECT id, label FROM dbo.chat2db_native_parent;",
            )
            .await?;
            live_execute(
                &mut conn.client,
                "CREATE FUNCTION dbo.chat2db_native_function(@value int) RETURNS int AS BEGIN RETURN @value + 1; END;",
            )
            .await?;
            live_execute(
                &mut conn.client,
                "CREATE PROCEDURE dbo.chat2db_native_procedure @value int AS SELECT @value AS value;",
            )
            .await?;
            live_execute(
                &mut conn.client,
                "CREATE TRIGGER dbo.chat2db_native_trigger ON dbo.chat2db_native_child AFTER INSERT AS BEGIN SET NOCOUNT ON; END;",
            )
            .await?;

            bind_query(
                "INSERT INTO dbo.chat2db_native_parent(id, label) VALUES (?, ?)",
                &[
                    QueryParameter {
                        position: 1,
                        value: DatabaseValue::SignedInteger(7),
                    },
                    QueryParameter {
                        position: 2,
                        value: DatabaseValue::Text("native-rust".to_owned()),
                    },
                ],
            )?
            .execute(&mut conn.client)
            .await
            .map_err(sqlserver_query_error)?;
            bind_query(
                "INSERT INTO dbo.chat2db_native_child(id, parent_id, note) VALUES (?, ?, ?)",
                &[
                    QueryParameter {
                        position: 1,
                        value: DatabaseValue::SignedInteger(11),
                    },
                    QueryParameter {
                        position: 2,
                        value: DatabaseValue::SignedInteger(7),
                    },
                    QueryParameter {
                        position: 3,
                        value: DatabaseValue::Text("catalog".to_owned()),
                    },
                ],
            )?
            .execute(&mut conn.client)
            .await
            .map_err(sqlserver_query_error)?;

            let rows = bind_query(
                "SELECT p.id, p.label, c.note FROM dbo.chat2db_native_parent p JOIN dbo.chat2db_native_child c ON c.parent_id=p.id WHERE p.id=?",
                &[QueryParameter {
                    position: 1,
                    value: DatabaseValue::SignedInteger(7),
                }],
            )?
            .query(&mut conn.client)
            .await
            .map_err(sqlserver_query_error)?
            .into_first_result()
            .await
            .map_err(sqlserver_query_error)?;
            assert_eq!(rows.len(), 1);
            assert_eq!(row_i32(&rows[0], 0)?, 7);
            assert_eq!(row_string(&rows[0], 1)?, "native-rust");
            assert_eq!(row_string(&rows[0], 2)?, "catalog");

            let catalog = conn
                .client
                .simple_query(
                    "SELECT CONVERT(int, COUNT(*)) FROM sys.objects WHERE name IN ('chat2db_native_parent','chat2db_native_child','chat2db_native_view','chat2db_native_function','chat2db_native_procedure','chat2db_native_trigger')",
                )
                .await
                .map_err(sqlserver_query_error)?
                .into_first_result()
                .await
                .map_err(sqlserver_query_error)?;
            assert_eq!(row_i32(&catalog[0], 0)?, 6);

            let metadata = conn
                .client
                .simple_query(
                    "SELECT CONVERT(int, COUNT(DISTINCT o.object_id)) FROM sys.objects o JOIN sys.schemas s ON s.schema_id=o.schema_id LEFT JOIN sys.columns c ON c.object_id=o.object_id LEFT JOIN sys.indexes i ON i.object_id=o.object_id LEFT JOIN sys.foreign_keys fk ON fk.parent_object_id=o.object_id WHERE s.name='dbo' AND o.name IN ('chat2db_native_parent','chat2db_native_child') AND c.column_id IS NOT NULL AND i.index_id IS NOT NULL",
                )
                .await
                .map_err(sqlserver_query_error)?
                .into_first_result()
                .await
                .map_err(sqlserver_query_error)?;
            assert_eq!(row_i32(&metadata[0], 0)?, 2);
            Ok::<(), AppError>(())
        }
        .await;

        live_cleanup(&mut conn.client).await;
        finish_connection(conn, smoke_result)
            .await
            .expect("native SQL Server product smoke should pass");
    }
}
