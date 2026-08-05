use std::{
    collections::HashSet,
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use chat2db_contract::{
    DmlExportFormat, DmlExportRequest, DmlExportSize, ImportFileRequest, OtherFileExportRequest,
    SqlFileExportRequest, TabularImportEncoding, TransferFileFormat, TransferSqlScope,
};
use chat2db_storage::{TransferArtifactRecord, TransferArtifactWriter};
use mysql_async::{
    Column, Conn, Error as MysqlError, Params, Row, TxOpts, Value,
    consts::{ColumnFlags, ColumnType},
    prelude::Queryable,
};
use sqlparser::{
    ast::{ObjectName, Query, SetExpr, Statement, TableFactor},
    dialect::MySqlDialect,
    parser::Parser,
};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use super::{PendingTransferArtifact, TaskCompletion, TransferContext, TransferRunError, format};
use crate::{AppError, AppErrorKind, Application, native_mysql};

const IMPORT_BATCH_ROWS: usize = 256;
const PROGRESS_ROW_INTERVAL: u64 = 250;
const DML_ARTIFACT_TTL_MS: i64 = 24 * 60 * 60 * 1_000;

pub(super) async fn import_file(
    application: &Application,
    request: ImportFileRequest,
    context: &TransferContext,
) -> Result<TaskCompletion, TransferRunError> {
    context.check_cancelled()?;
    let path = PathBuf::from(&request.file_path);
    let input = match request.format {
        TransferFileFormat::Sql => ImportInput::Sql(read_sql_file(path).await?),
        format => ImportInput::Table(
            read_table_file(
                path,
                format,
                request.contains_header,
                request.tabular_encoding,
            )
            .await?,
        ),
    };
    context
        .progress(0, input.total(), "Import file validated", None)
        .await?;

    let mut conn = open_connection(application, &request.datasource_id, true).await?;
    let result = match input {
        ImportInput::Sql(statements) => {
            import_sql(&mut conn, &request.database_name, statements, context).await
        }
        ImportInput::Table(table) => {
            let table_name = request.table_name.as_deref().ok_or_else(|| {
                TransferRunError::from(AppError::invalid(
                    "missing_import_table",
                    "tableName is required for tabular imports",
                ))
            })?;
            import_table(
                &mut conn,
                &request.database_name,
                table_name,
                table,
                context,
            )
            .await
        }
    };
    finish_connection(conn, result).await?;
    Ok(TaskCompletion::WithoutArtifact(
        "Import completed successfully".to_owned(),
    ))
}

pub(super) async fn export_sql(
    application: &Application,
    request: SqlFileExportRequest,
    context: &TransferContext,
) -> Result<TaskCompletion, TransferRunError> {
    validate_database_name(&request.database_name).map_err(TransferRunError::into_app_error)?;
    let mut conn = open_connection(application, &request.datasource_id, false).await?;
    let table_names =
        resolve_tables(&mut conn, &request.database_name, &request.table_names).await?;
    let full_database = request.table_names.is_empty();
    let file_name = timestamped_file_name(&request.database_name, "sql");
    let mut writer =
        context.begin_artifact(&file_name, "application/sql; charset=utf-8", "SQL", "sql")?;
    let write_result = write_sql_export(
        &mut conn,
        writer.file_mut(),
        &request.database_name,
        &table_names,
        request.scope,
        full_database,
        context,
    )
    .await;
    finish_connection(conn, write_result).await?;
    publish_task_artifact(writer, request.export_path.as_deref(), &file_name, context).await
}

pub(super) async fn export_other(
    application: &Application,
    request: OtherFileExportRequest,
    context: &TransferContext,
) -> Result<TaskCompletion, TransferRunError> {
    validate_database_name(&request.database_name).map_err(TransferRunError::into_app_error)?;
    if request.table_names.is_empty() {
        return Err(AppError::invalid(
            "missing_export_tables",
            "tableNames must contain at least one table",
        )
        .into());
    }
    let mut conn = open_connection(application, &request.datasource_id, false).await?;
    let table_names =
        resolve_tables(&mut conn, &request.database_name, &request.table_names).await?;
    let (file_name, media_type, artifact_format, extension) = if table_names.len() == 1 {
        let extension = request.format.extension();
        (
            timestamped_file_name(&table_names[0], extension),
            media_type(request.format),
            request.format.extension().to_ascii_uppercase(),
            extension,
        )
    } else {
        (
            timestamped_file_name(&request.database_name, "zip"),
            "application/zip",
            "ZIP".to_owned(),
            "zip",
        )
    };
    let mut writer = context.begin_artifact(&file_name, media_type, &artifact_format, extension)?;
    let write_result = if request.format == TransferFileFormat::Sql && table_names.len() == 1 {
        write_sql_data_export(
            &mut conn,
            writer.file_mut(),
            &request.database_name,
            &table_names,
            context,
        )
        .await
    } else if request.format == TransferFileFormat::Sql {
        write_sql_zip(
            &mut conn,
            writer.file_mut(),
            &request.database_name,
            &table_names,
            context,
        )
        .await
    } else if table_names.len() == 1 {
        write_table_tabular(
            &mut conn,
            writer.file_mut(),
            &request.database_name,
            &table_names[0],
            request.format,
            request.contains_header,
            0,
            context.cancellation(),
            Some(context),
        )
        .await
        .map(|_| ())
    } else {
        write_tabular_zip(
            &mut conn,
            writer.file_mut(),
            &request.database_name,
            &table_names,
            request.format,
            request.contains_header,
            context,
        )
        .await
    };
    finish_connection(conn, write_result).await?;
    publish_task_artifact(writer, request.export_path.as_deref(), &file_name, context).await
}

pub(super) async fn export_dml(
    application: &Application,
    request: DmlExportRequest,
) -> Result<TransferArtifactRecord, AppError> {
    validate_database_name(&request.database_name).map_err(TransferRunError::into_app_error)?;
    let sql = match request.export_size {
        DmlExportSize::CurrentPage if !request.sql.trim().is_empty() => request.sql.trim(),
        DmlExportSize::CurrentPage | DmlExportSize::All => request.original_sql.trim(),
    };
    let table_name = select_table_name(sql)?;
    let (format, extension, media_type) = match request.format {
        DmlExportFormat::Csv => (TransferFileFormat::Csv, "csv", "text/csv; charset=utf-8"),
        DmlExportFormat::Xlsx => (
            TransferFileFormat::Xlsx,
            "xlsx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ),
        DmlExportFormat::Insert => (
            TransferFileFormat::Sql,
            "sql",
            "application/sql; charset=utf-8",
        ),
    };
    let stem = table_name
        .as_deref()
        .unwrap_or(request.database_name.as_str());
    let file_name = timestamped_file_name(stem, extension);
    let storage = application.require_storage()?;
    let expires_at = now_millis()?.saturating_add(DML_ARTIFACT_TTL_MS);
    let mut writer = storage
        .begin_transfer_artifact(
            None,
            &file_name,
            media_type,
            &extension.to_ascii_uppercase(),
            extension,
            Some(expires_at),
        )
        .map_err(AppError::from)?;
    let cancellation = CancellationToken::new();
    let mut conn = open_connection(application, &request.datasource_id, false)
        .await
        .map_err(TransferRunError::into_app_error)?;
    let selected_result_set = request.result_set_id.unwrap_or(0);
    let write_result = match request.format {
        DmlExportFormat::Csv | DmlExportFormat::Xlsx => write_query_tabular(
            &mut conn,
            writer.file_mut(),
            sql,
            format,
            true,
            selected_result_set,
            &cancellation,
            None,
        )
        .await
        .map(|_| ()),
        DmlExportFormat::Insert => {
            let table_name = table_name.ok_or_else(|| {
                AppError::invalid(
                    "sql_analysis_error",
                    "INSERT export requires a SELECT from a table",
                )
            })?;
            write_query_inserts(
                &mut conn,
                writer.file_mut(),
                sql,
                &request.database_name,
                &table_name,
                selected_result_set,
                &cancellation,
                None,
            )
            .await
            .map(|_| ())
        }
    };
    finish_connection(conn, write_result)
        .await
        .map_err(TransferRunError::into_app_error)?;
    writer.finish().map_err(AppError::from)
}

enum ImportInput {
    Sql(Vec<String>),
    Table(format::ImportedTable),
}

impl ImportInput {
    fn total(&self) -> Option<u64> {
        match self {
            Self::Sql(statements) => u64::try_from(statements.len()).ok(),
            Self::Table(table) => u64::try_from(table.rows.len()).ok(),
        }
    }
}

async fn read_sql_file(path: PathBuf) -> Result<Vec<String>, TransferRunError> {
    tokio::task::spawn_blocking(move || {
        format::validate_import_file(&path)?;
        let mut file = File::open(&path).map_err(import_file_error)?;
        let mut script = String::new();
        file.read_to_string(&mut script).map_err(|error| {
            tracing::warn!(%error, "SQL import file could not be decoded");
            AppError::invalid(
                "invalid_sql_import_file",
                "The SQL import file must contain UTF-8 text",
            )
        })?;
        native_mysql::split_mysql_script(&script)
    })
    .await
    .map_err(|_| TransferRunError::from(AppError::internal()))?
    .map_err(TransferRunError::from)
}

async fn read_table_file(
    path: PathBuf,
    format: TransferFileFormat,
    contains_header: bool,
    tabular_encoding: TabularImportEncoding,
) -> Result<format::ImportedTable, TransferRunError> {
    tokio::task::spawn_blocking(move || {
        format::read_tabular_file(&path, format, contains_header, tabular_encoding)
    })
    .await
    .map_err(|_| TransferRunError::from(AppError::internal()))?
    .map_err(TransferRunError::from)
}

async fn import_sql(
    conn: &mut Conn,
    database_name: &str,
    statements: Vec<String>,
    context: &TransferContext,
) -> Result<(), TransferRunError> {
    let database_name = native_mysql::quote_identifier(database_name, "databaseName")?;
    conn.query_drop(format!("USE {database_name}"))
        .await
        .map_err(mysql_error)?;
    let total = u64::try_from(statements.len()).map_err(|_| AppError::internal())?;
    for (index, statement) in statements.into_iter().enumerate() {
        context.check_cancelled()?;
        let query = conn.query_drop(statement);
        tokio::pin!(query);
        tokio::select! {
            () = context.cancellation().cancelled() => return Err(TransferRunError::Cancelled),
            result = &mut query => result.map_err(mysql_error)?,
        }
        let current = u64::try_from(index + 1).map_err(|_| AppError::internal())?;
        context
            .progress(
                current,
                Some(total),
                "Executing SQL import",
                (current == total).then_some("SQL statements imported"),
            )
            .await?;
    }
    Ok(())
}

async fn import_table(
    conn: &mut Conn,
    database_name: &str,
    table_name: &str,
    table: format::ImportedTable,
    context: &TransferContext,
) -> Result<(), TransferRunError> {
    validate_database_name(database_name)?;
    let available_columns = table_columns(conn, database_name, table_name).await?;
    if available_columns.is_empty() {
        return Err(AppError::not_found(
            "mysql_table_not_found",
            "The selected MySQL table does not exist",
        )
        .into());
    }
    let columns = match table.columns {
        Some(headers) => canonical_import_columns(headers, &available_columns)?,
        None => available_columns,
    };
    if table.rows.iter().any(|row| row.len() != columns.len()) {
        return Err(AppError::invalid(
            "invalid_tabular_row",
            "Every imported row must match the target column count",
        )
        .into());
    }
    let qualified = qualified_table(database_name, table_name)?;
    let placeholders = std::iter::repeat_n("?", columns.len())
        .collect::<Vec<_>>()
        .join(", ");
    let columns = columns
        .iter()
        .map(|column| native_mysql::quote_identifier(column, "columnName"))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let sql = format!("INSERT INTO {qualified} ({columns}) VALUES ({placeholders})");
    let total = u64::try_from(table.rows.len()).map_err(|_| AppError::internal())?;
    let mut transaction = conn
        .start_transaction(TxOpts::default())
        .await
        .map_err(mysql_error)?;
    let statement = match transaction.prep(sql).await {
        Ok(statement) => statement,
        Err(error) => {
            let _ = transaction.rollback().await;
            return Err(mysql_error(error));
        }
    };
    let mut current = 0_u64;
    for rows in table.rows.chunks(IMPORT_BATCH_ROWS) {
        if context.cancellation().is_cancelled() {
            let _ = transaction.rollback().await;
            return Err(TransferRunError::Cancelled);
        }
        let parameters = rows.iter().map(|row| {
            Params::Positional(
                row.iter()
                    .map(|value| match value {
                        format::TabularValue::Null => Value::NULL,
                        format::TabularValue::Text(value) => {
                            Value::Bytes(value.as_bytes().to_vec())
                        }
                        format::TabularValue::Bytes(value) => Value::Bytes(value.clone()),
                    })
                    .collect(),
            )
        });
        if let Err(error) = transaction.exec_batch(&statement, parameters).await {
            let _ = transaction.rollback().await;
            return Err(mysql_error(error));
        }
        current = current
            .checked_add(u64::try_from(rows.len()).map_err(|_| AppError::internal())?)
            .ok_or_else(AppError::internal)?;
        if let Err(error) = context
            .progress(current, Some(total), "Importing table rows", None)
            .await
        {
            let _ = transaction.rollback().await;
            return Err(error);
        }
    }
    if context.cancellation().is_cancelled() {
        let _ = transaction.rollback().await;
        return Err(TransferRunError::Cancelled);
    }
    transaction.commit().await.map_err(mysql_error)?;
    Ok(())
}

async fn write_sql_export(
    conn: &mut Conn,
    output: &mut File,
    database_name: &str,
    table_names: &[String],
    scope: TransferSqlScope,
    full_database: bool,
    context: &TransferContext,
) -> Result<(), TransferRunError> {
    writeln!(output, "-- Chat2DB MySQL export").map_err(export_file_error)?;
    writeln!(output, "SET FOREIGN_KEY_CHECKS=0;").map_err(export_file_error)?;
    writeln!(
        output,
        "USE {};",
        native_mysql::quote_identifier(database_name, "databaseName")?
    )
    .map_err(export_file_error)?;

    if matches!(scope, TransferSqlScope::All | TransferSqlScope::Schema) {
        write_table_definitions(conn, output, database_name, table_names, context).await?;
        if full_database {
            write_database_objects(conn, output, database_name, context).await?;
        }
    }
    if matches!(scope, TransferSqlScope::All | TransferSqlScope::Table) {
        write_table_data(conn, output, database_name, table_names, context).await?;
    }
    writeln!(output, "SET FOREIGN_KEY_CHECKS=1;").map_err(export_file_error)?;
    output.flush().map_err(export_file_error)
}

async fn write_sql_data_export(
    conn: &mut Conn,
    output: &mut File,
    database_name: &str,
    table_names: &[String],
    context: &TransferContext,
) -> Result<(), TransferRunError> {
    writeln!(output, "-- Chat2DB MySQL table-data export").map_err(export_file_error)?;
    writeln!(output, "SET FOREIGN_KEY_CHECKS=0;").map_err(export_file_error)?;
    write_table_data(conn, output, database_name, table_names, context).await?;
    writeln!(output, "SET FOREIGN_KEY_CHECKS=1;").map_err(export_file_error)?;
    output.flush().map_err(export_file_error)
}

async fn write_table_definitions(
    conn: &mut Conn,
    output: &mut File,
    database_name: &str,
    table_names: &[String],
    context: &TransferContext,
) -> Result<(), TransferRunError> {
    for (index, table_name) in table_names.iter().enumerate() {
        context.check_cancelled()?;
        let qualified = qualified_table(database_name, table_name)?;
        let create = show_create(conn, &format!("SHOW CREATE TABLE {qualified}"), 1).await?;
        writeln!(
            output,
            "\nDROP TABLE IF EXISTS {};",
            native_mysql::quote_identifier(table_name, "tableName")?
        )
        .and_then(|()| writeln!(output, "{create};"))
        .map_err(export_file_error)?;
        context
            .progress(
                u64::try_from(index + 1).map_err(|_| AppError::internal())?,
                u64::try_from(table_names.len()).ok(),
                "Exporting table definitions",
                None,
            )
            .await?;
    }
    Ok(())
}

async fn write_database_objects(
    conn: &mut Conn,
    output: &mut File,
    database_name: &str,
    context: &TransferContext,
) -> Result<(), TransferRunError> {
    let views: Vec<String> = conn
        .exec(
            "SELECT TABLE_NAME FROM information_schema.VIEWS WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (database_name,),
        )
        .await
        .map_err(mysql_error)?;
    for view in views {
        context.check_cancelled()?;
        let create = show_create(
            conn,
            &format!(
                "SHOW CREATE VIEW {}",
                qualified_table(database_name, &view)?
            ),
            1,
        )
        .await?;
        writeln!(
            output,
            "\nDROP VIEW IF EXISTS {};\n{create};",
            native_mysql::quote_identifier(&view, "viewName")?
        )
        .map_err(export_file_error)?;
    }

    let routines: Vec<(String, String)> = conn
        .exec(
            "SELECT ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES \
             WHERE ROUTINE_SCHEMA = ? ORDER BY ROUTINE_TYPE, ROUTINE_NAME",
            (database_name,),
        )
        .await
        .map_err(mysql_error)?;
    for (name, kind) in routines {
        context.check_cancelled()?;
        let kind = if kind.eq_ignore_ascii_case("FUNCTION") {
            "FUNCTION"
        } else {
            "PROCEDURE"
        };
        let create = show_create(
            conn,
            &format!(
                "SHOW CREATE {kind} {}",
                qualified_table(database_name, &name)?
            ),
            2,
        )
        .await?;
        write_delimited_object(output, kind, &name, &create)?;
    }

    let triggers: Vec<String> = conn
        .exec(
            "SELECT TRIGGER_NAME FROM information_schema.TRIGGERS \
             WHERE TRIGGER_SCHEMA = ? ORDER BY TRIGGER_NAME",
            (database_name,),
        )
        .await
        .map_err(mysql_error)?;
    for trigger in triggers {
        context.check_cancelled()?;
        let create = show_create(
            conn,
            &format!(
                "SHOW CREATE TRIGGER {}",
                qualified_table(database_name, &trigger)?
            ),
            2,
        )
        .await?;
        write_delimited_object(output, "TRIGGER", &trigger, &create)?;
    }
    Ok(())
}

fn write_delimited_object(
    output: &mut File,
    kind: &str,
    name: &str,
    create: &str,
) -> Result<(), TransferRunError> {
    writeln!(output, "\nDELIMITER $$")
        .and_then(|()| {
            writeln!(
                output,
                "DROP {kind} IF EXISTS {}$$",
                native_mysql::quote_identifier(name, "objectName")
                    .map_err(|_| std::io::Error::other("invalid MySQL object name"))?
            )
        })
        .and_then(|()| writeln!(output, "{create}$$"))
        .and_then(|()| writeln!(output, "DELIMITER ;"))
        .map_err(export_file_error)
}

async fn write_table_data(
    conn: &mut Conn,
    output: &mut File,
    database_name: &str,
    table_names: &[String],
    context: &TransferContext,
) -> Result<(), TransferRunError> {
    let cancellation = context.cancellation();
    let mut exported_rows = 0_u64;
    for table_name in table_names {
        context.check_cancelled()?;
        writeln!(output, "\n-- Data for table {table_name}").map_err(export_file_error)?;
        let sql = format!(
            "SELECT * FROM {}",
            qualified_table(database_name, table_name)?
        );
        let rows = write_query_inserts(
            conn,
            output,
            &sql,
            database_name,
            table_name,
            0,
            cancellation,
            Some(context),
        )
        .await?;
        exported_rows = exported_rows.saturating_add(rows);
        context
            .progress(
                exported_rows,
                None,
                "Exporting table data",
                Some(&format!("Exported table {table_name}")),
            )
            .await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn write_query_inserts(
    conn: &mut Conn,
    output: &mut File,
    sql: &str,
    database_name: &str,
    table_name: &str,
    selected_result_set: u32,
    cancellation: &CancellationToken,
    context: Option<&TransferContext>,
) -> Result<u64, TransferRunError> {
    let no_backslash_escapes = session_no_backslash_escapes(conn).await?;
    let mut result = conn.query_iter(sql).await.map_err(mysql_error)?;
    let mut result_set = 0_u32;
    let mut found = false;
    let mut row_count = 0_u64;
    let qualified = qualified_table(database_name, table_name)?;
    loop {
        if result.is_empty() {
            break;
        }
        let columns = result
            .columns_ref()
            .iter()
            .map(|column| column.name_str().into_owned())
            .collect::<Vec<_>>();
        let selected = !columns.is_empty() && result_set == selected_result_set;
        if selected {
            found = true;
        }
        loop {
            let row = tokio::select! {
                () = cancellation.cancelled() => return Err(TransferRunError::Cancelled),
                row = result.next() => row.map_err(mysql_error)?,
            };
            let Some(row) = row else {
                break;
            };
            if !selected {
                continue;
            }
            let values = row.unwrap();
            if values.len() != columns.len() {
                return Err(AppError::internal().into());
            }
            let column_sql = columns
                .iter()
                .map(|column| native_mysql::quote_identifier(column, "columnName"))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            let value_sql = values
                .iter()
                .map(|value| value.as_sql(no_backslash_escapes))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(
                output,
                "INSERT INTO {qualified} ({column_sql}) VALUES ({value_sql});"
            )
            .map_err(export_file_error)?;
            row_count = row_count.saturating_add(1);
            if row_count.is_multiple_of(PROGRESS_ROW_INTERVAL)
                && let Some(context) = context
            {
                context
                    .progress(row_count, None, "Exporting table rows", None)
                    .await?;
            }
        }
        if !columns.is_empty() {
            result_set = result_set.saturating_add(1);
            if selected {
                break;
            }
        }
    }
    drop(result);
    if !found {
        return Err(AppError::invalid(
            "result_set_not_found",
            "The selected result set does not exist",
        )
        .into());
    }
    Ok(row_count)
}

#[allow(clippy::too_many_arguments)]
async fn write_table_tabular(
    conn: &mut Conn,
    output: &mut File,
    database_name: &str,
    table_name: &str,
    format: TransferFileFormat,
    contains_header: bool,
    selected_result_set: u32,
    cancellation: &CancellationToken,
    context: Option<&TransferContext>,
) -> Result<u64, TransferRunError> {
    let sql = format!(
        "SELECT * FROM {}",
        qualified_table(database_name, table_name)?
    );
    write_query_tabular(
        conn,
        output,
        &sql,
        format,
        contains_header,
        selected_result_set,
        cancellation,
        context,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn write_query_tabular(
    conn: &mut Conn,
    output: &mut File,
    sql: &str,
    format: TransferFileFormat,
    contains_header: bool,
    selected_result_set: u32,
    cancellation: &CancellationToken,
    context: Option<&TransferContext>,
) -> Result<u64, TransferRunError> {
    let mut result = conn.query_iter(sql).await.map_err(mysql_error)?;
    let mut sink = format::tabular_sink(format, output, contains_header)?;
    let mut result_set = 0_u32;
    let mut found = false;
    let mut row_count = 0_u64;
    loop {
        if result.is_empty() {
            break;
        }
        let column_metadata = result.columns_ref().to_vec();
        let columns = column_metadata
            .iter()
            .map(|column| column.name_str().into_owned())
            .collect::<Vec<_>>();
        let binary_columns = column_metadata
            .iter()
            .map(is_binary_tabular_column)
            .collect::<Vec<_>>();
        let selected = !columns.is_empty() && result_set == selected_result_set;
        if selected {
            sink.write_header(&columns)?;
            found = true;
        }
        loop {
            let row = tokio::select! {
                () = cancellation.cancelled() => return Err(TransferRunError::Cancelled),
                row = result.next() => row.map_err(mysql_error)?,
            };
            let Some(row) = row else {
                break;
            };
            if !selected {
                continue;
            }
            let values = row
                .unwrap()
                .into_iter()
                .zip(binary_columns.iter().copied())
                .map(|(value, binary)| tabular_value(value, binary))
                .collect::<Vec<_>>();
            sink.write_row(&values)?;
            row_count = row_count.saturating_add(1);
            if row_count.is_multiple_of(PROGRESS_ROW_INTERVAL)
                && let Some(context) = context
            {
                context
                    .progress(row_count, None, "Exporting table rows", None)
                    .await?;
            }
        }
        if !columns.is_empty() {
            result_set = result_set.saturating_add(1);
            if selected {
                break;
            }
        }
    }
    drop(result);
    if !found {
        return Err(AppError::invalid(
            "result_set_not_found",
            "The selected result set does not exist",
        )
        .into());
    }
    sink.finish()?;
    Ok(row_count)
}

async fn write_tabular_zip(
    conn: &mut Conn,
    output: &mut File,
    database_name: &str,
    table_names: &[String],
    format: TransferFileFormat,
    contains_header: bool,
    context: &TransferContext,
) -> Result<(), TransferRunError> {
    let temporary = TempDir::new().map_err(export_file_error)?;
    let cancellation = context.cancellation();
    let mut paths = Vec::with_capacity(table_names.len());
    for (index, table_name) in table_names.iter().enumerate() {
        context.check_cancelled()?;
        let name = format!("{}.{}", safe_file_stem(table_name), format.extension());
        let path = temporary.path().join(&name);
        let mut file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(export_file_error)?;
        write_table_tabular(
            conn,
            &mut file,
            database_name,
            table_name,
            format,
            contains_header,
            0,
            cancellation,
            Some(context),
        )
        .await?;
        file.sync_all().map_err(export_file_error)?;
        paths.push((name, path));
        context
            .progress(
                u64::try_from(index + 1).map_err(|_| AppError::internal())?,
                u64::try_from(table_names.len()).ok(),
                "Preparing table archive",
                None,
            )
            .await?;
    }
    context.check_cancelled()?;
    write_zip_entries(output, paths)
}

async fn write_sql_zip(
    conn: &mut Conn,
    output: &mut File,
    database_name: &str,
    table_names: &[String],
    context: &TransferContext,
) -> Result<(), TransferRunError> {
    let temporary = TempDir::new().map_err(export_file_error)?;
    let mut paths = Vec::with_capacity(table_names.len());
    for (index, table_name) in table_names.iter().enumerate() {
        context.check_cancelled()?;
        let name = format!("{}.sql", safe_file_stem(table_name));
        let path = temporary.path().join(&name);
        let mut file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(export_file_error)?;
        write_sql_data_export(
            conn,
            &mut file,
            database_name,
            std::slice::from_ref(table_name),
            context,
        )
        .await?;
        file.sync_all().map_err(export_file_error)?;
        paths.push((name, path));
        context
            .progress(
                u64::try_from(index + 1).map_err(|_| AppError::internal())?,
                u64::try_from(table_names.len()).ok(),
                "Preparing SQL table archive",
                None,
            )
            .await?;
    }
    context.check_cancelled()?;
    write_zip_entries(output, paths)
}

fn write_zip_entries(
    output: &mut File,
    paths: Vec<(String, PathBuf)>,
) -> Result<(), TransferRunError> {
    let mut zip = ZipWriter::new(output);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, path) in paths {
        zip.start_file(name, options).map_err(zip_error)?;
        let mut source = File::open(path).map_err(export_file_error)?;
        std::io::copy(&mut source, &mut zip).map_err(export_file_error)?;
    }
    zip.finish().map_err(zip_error)?;
    Ok(())
}

async fn session_no_backslash_escapes(conn: &mut Conn) -> Result<bool, TransferRunError> {
    let sql_mode = conn
        .query_first::<String, _>("SELECT @@SESSION.sql_mode")
        .await
        .map_err(mysql_error)?
        .unwrap_or_default();
    Ok(sql_mode_has_no_backslash_escapes(&sql_mode))
}

fn sql_mode_has_no_backslash_escapes(sql_mode: &str) -> bool {
    sql_mode
        .split(',')
        .any(|mode| mode.trim().eq_ignore_ascii_case("NO_BACKSLASH_ESCAPES"))
}

async fn resolve_tables(
    conn: &mut Conn,
    database_name: &str,
    requested: &[String],
) -> Result<Vec<String>, TransferRunError> {
    let available: Vec<String> = conn
        .exec(
            "SELECT TABLE_NAME FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE' ORDER BY TABLE_NAME",
            (database_name,),
        )
        .await
        .map_err(mysql_error)?;
    if requested.is_empty() {
        return Ok(available);
    }
    let available_set = available.iter().collect::<HashSet<_>>();
    let mut seen = HashSet::with_capacity(requested.len());
    let mut selected = Vec::with_capacity(requested.len());
    for table in requested {
        if !available_set.contains(table) {
            return Err(AppError::not_found(
                "mysql_table_not_found",
                format!("MySQL table {database_name}.{table} does not exist"),
            )
            .into());
        }
        if seen.insert(table) {
            selected.push(table.clone());
        }
    }
    Ok(selected)
}

async fn table_columns(
    conn: &mut Conn,
    database_name: &str,
    table_name: &str,
) -> Result<Vec<String>, TransferRunError> {
    conn.exec(
        "SELECT COLUMN_NAME FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
        (database_name, table_name),
    )
    .await
    .map_err(mysql_error)
}

fn canonical_import_columns(
    requested: Vec<String>,
    available: &[String],
) -> Result<Vec<String>, TransferRunError> {
    requested
        .into_iter()
        .map(|column| {
            available
                .iter()
                .find(|available| available.eq_ignore_ascii_case(&column))
                .cloned()
                .ok_or_else(|| {
                    AppError::invalid(
                        "unknown_import_column",
                        format!("Import column {column} does not exist in the target table"),
                    )
                    .into()
                })
        })
        .collect()
}

async fn show_create(
    conn: &mut Conn,
    sql: &str,
    value_index: usize,
) -> Result<String, TransferRunError> {
    let row = conn
        .query_first::<Row, _>(sql)
        .await
        .map_err(mysql_error)?
        .ok_or_else(|| {
            TransferRunError::from(AppError::not_found(
                "mysql_object_not_found",
                "The MySQL object no longer exists",
            ))
        })?;
    row.get_opt::<String, _>(value_index)
        .ok_or_else(|| TransferRunError::from(AppError::internal()))?
        .map_err(|_| TransferRunError::from(AppError::internal()))
}

async fn open_connection(
    application: &Application,
    datasource_id: &str,
    writable: bool,
) -> Result<native_mysql::ManagedMysqlConnection, TransferRunError> {
    let resolved = native_mysql::resolve_native_connection(application, datasource_id).await?;
    if writable && resolved.connection.read_only {
        return Err(AppError::new(
            AppErrorKind::Conflict,
            chat2db_contract::ApiError::new(
                "datasource_read_only",
                "The datasource is configured as read-only",
            ),
        )
        .into());
    }
    native_mysql::open_resolved_connection(&resolved)
        .await
        .map_err(TransferRunError::from)
}

async fn finish_connection<T>(
    conn: native_mysql::ManagedMysqlConnection,
    result: Result<T, TransferRunError>,
) -> Result<T, TransferRunError> {
    match result {
        Ok(value) => native_mysql::finish_connection(conn, Ok(value))
            .await
            .map_err(TransferRunError::from),
        Err(TransferRunError::Cancelled) => {
            drop(conn);
            Err(TransferRunError::Cancelled)
        }
        Err(TransferRunError::Failed(error)) => native_mysql::finish_connection(conn, Err(error))
            .await
            .map_err(TransferRunError::from),
    }
}

async fn publish_task_artifact(
    mut writer: TransferArtifactWriter,
    export_path: Option<&str>,
    file_name: &str,
    context: &TransferContext,
) -> Result<TaskCompletion, TransferRunError> {
    writer.file_mut().flush().map_err(export_file_error)?;
    writer.file_mut().sync_all().map_err(export_file_error)?;
    let pending = match export_path {
        Some(path) => Some(
            stage_user_copy(
                writer.path().to_owned(),
                path.to_owned(),
                file_name.to_owned(),
            )
            .await?,
        ),
        None => None,
    };
    context.check_cancelled()?;
    Ok(TaskCompletion::Artifact(PendingTransferArtifact::new(
        move || async move {
            let artifact = tokio::task::spawn_blocking(move || writer.finish())
                .await
                .map_err(|_| AppError::internal())?
                .map_err(AppError::from)?;
            if let Some(pending) = pending
                && let Err(error) = pending.publish().await
            {
                tracing::warn!(%error, artifact_id = %artifact.id, "managed transfer succeeded but exportPath publication failed");
            }
            Ok(artifact)
        },
    )))
}

struct PendingUserCopy {
    part_path: PathBuf,
    final_path: PathBuf,
    published: bool,
}

impl PendingUserCopy {
    async fn publish(mut self) -> Result<(), AppError> {
        let part_path = self.part_path.clone();
        let final_path = self.final_path.clone();
        tokio::task::spawn_blocking(move || {
            fs::rename(&part_path, &final_path).map_err(export_file_app_error)?;
            sync_parent(&final_path)
        })
        .await
        .map_err(|_| AppError::internal())??;
        self.published = true;
        Ok(())
    }
}

impl Drop for PendingUserCopy {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.part_path);
        }
    }
}

async fn stage_user_copy(
    source: PathBuf,
    export_path: String,
    file_name: String,
) -> Result<PendingUserCopy, TransferRunError> {
    tokio::task::spawn_blocking(move || {
        let directory = PathBuf::from(export_path);
        fs::create_dir_all(&directory).map_err(export_file_app_error)?;
        let directory = fs::canonicalize(directory).map_err(export_file_app_error)?;
        let final_path = directory.join(&file_name);
        let part_path = directory.join(format!(".{file_name}.{}.part", Uuid::new_v4()));
        let result: Result<PendingUserCopy, AppError> = (|| {
            let mut input = File::open(source).map_err(export_file_app_error)?;
            let mut output = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&part_path)
                .map_err(export_file_app_error)?;
            std::io::copy(&mut input, &mut output).map_err(export_file_app_error)?;
            output.sync_all().map_err(export_file_app_error)?;
            Ok(PendingUserCopy {
                part_path: part_path.clone(),
                final_path,
                published: false,
            })
        })();
        if result.is_err() {
            let _ = fs::remove_file(part_path);
        }
        result
    })
    .await
    .map_err(|_| TransferRunError::from(AppError::internal()))?
    .map_err(TransferRunError::from)
}

fn select_table_name(sql: &str) -> Result<Option<String>, AppError> {
    if sql.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_dml_export_sql",
            "A SELECT statement is required for export",
        ));
    }
    let statements = Parser::parse_sql(&MySqlDialect {}, sql).map_err(|_| {
        AppError::invalid(
            "sql_analysis_error",
            "The export SQL could not be parsed as one MySQL SELECT statement",
        )
    })?;
    let [Statement::Query(query)] = statements.as_slice() else {
        return Err(AppError::invalid(
            "sql_analysis_error",
            "The export SQL must be exactly one SELECT statement",
        ));
    };
    Ok(first_query_table(query))
}

fn first_query_table(query: &Query) -> Option<String> {
    query
        .with
        .as_ref()
        .and_then(|with| {
            with.cte_tables
                .iter()
                .find_map(|cte| first_query_table(&cte.query))
        })
        .or_else(|| first_set_table(&query.body))
}

fn first_set_table(expression: &SetExpr) -> Option<String> {
    match expression {
        SetExpr::Select(select) => select
            .from
            .iter()
            .find_map(|table| table_factor_name(&table.relation)),
        SetExpr::Query(query) => first_query_table(query),
        SetExpr::SetOperation { left, right, .. } => {
            first_set_table(left).or_else(|| first_set_table(right))
        }
        SetExpr::Table(table) => table.table_name.as_ref().map(ToString::to_string),
        SetExpr::Values(_)
        | SetExpr::Insert(_)
        | SetExpr::Update(_)
        | SetExpr::Delete(_)
        | SetExpr::Merge(_) => None,
    }
}

fn table_factor_name(factor: &TableFactor) -> Option<String> {
    match factor {
        TableFactor::Table { name, .. } => object_name_last(name),
        TableFactor::Derived { subquery, .. } => first_query_table(subquery),
        _ => None,
    }
}

fn object_name_last(name: &ObjectName) -> Option<String> {
    name.0
        .last()
        .and_then(|part| part.as_ident())
        .map(|identifier| identifier.value.clone())
}

fn tabular_value(value: Value, binary: bool) -> format::TabularValue {
    match value {
        Value::NULL => format::TabularValue::Null,
        Value::Bytes(bytes) if binary => format::TabularValue::Bytes(bytes),
        Value::Bytes(bytes) => match String::from_utf8(bytes) {
            Ok(value) => format::TabularValue::Text(value),
            Err(error) => format::TabularValue::Bytes(error.into_bytes()),
        },
        Value::Int(value) => format::TabularValue::Text(value.to_string()),
        Value::UInt(value) => format::TabularValue::Text(value.to_string()),
        Value::Float(value) => format::TabularValue::Text(value.to_string()),
        Value::Double(value) => format::TabularValue::Text(value.to_string()),
        Value::Date(year, month, day, hour, minute, second, micros) => {
            let mut value = format!("{year:04}-{month:02}-{day:02}");
            if hour != 0 || minute != 0 || second != 0 || micros != 0 {
                let _ = write!(value, " {hour:02}:{minute:02}:{second:02}");
                if micros != 0 {
                    let _ = write!(value, ".{micros:06}");
                }
            }
            format::TabularValue::Text(value)
        }
        Value::Time(negative, days, hours, minutes, seconds, micros) => {
            let total_hours = days.saturating_mul(24).saturating_add(u32::from(hours));
            let sign = if negative { "-" } else { "" };
            let mut value = format!("{sign}{total_hours:02}:{minutes:02}:{seconds:02}");
            if micros != 0 {
                let _ = write!(value, ".{micros:06}");
            }
            format::TabularValue::Text(value)
        }
    }
}

fn is_binary_tabular_column(column: &Column) -> bool {
    use ColumnType as Type;

    matches!(
        column.column_type(),
        Type::MYSQL_TYPE_BIT | Type::MYSQL_TYPE_GEOMETRY | Type::MYSQL_TYPE_VECTOR
    ) || (matches!(
        column.column_type(),
        Type::MYSQL_TYPE_VARCHAR
            | Type::MYSQL_TYPE_VAR_STRING
            | Type::MYSQL_TYPE_STRING
            | Type::MYSQL_TYPE_TINY_BLOB
            | Type::MYSQL_TYPE_MEDIUM_BLOB
            | Type::MYSQL_TYPE_LONG_BLOB
            | Type::MYSQL_TYPE_BLOB
    ) && (column.character_set() == 63 || column.flags().contains(ColumnFlags::BINARY_FLAG)))
}

fn qualified_table(database_name: &str, table_name: &str) -> Result<String, AppError> {
    Ok(format!(
        "{}.{}",
        native_mysql::quote_identifier(database_name, "databaseName")?,
        native_mysql::quote_identifier(table_name, "tableName")?
    ))
}

fn validate_database_name(database_name: &str) -> Result<(), TransferRunError> {
    native_mysql::quote_identifier(database_name, "databaseName")?;
    Ok(())
}

fn media_type(format: TransferFileFormat) -> &'static str {
    match format {
        TransferFileFormat::Csv => "text/csv; charset=utf-8",
        TransferFileFormat::Xls => "application/vnd.ms-excel",
        TransferFileFormat::Xlsx => {
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        }
        TransferFileFormat::Sql => "application/sql; charset=utf-8",
    }
}

fn timestamped_file_name(stem: &str, extension: &str) -> String {
    format!(
        "{}_{}.{}",
        safe_file_stem(stem),
        now_millis().unwrap_or(0),
        extension
    )
}

fn safe_file_stem(value: &str) -> String {
    let output = value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(128)
        .collect::<String>();
    if output.is_empty() {
        "mysql_export".to_owned()
    } else {
        output
    }
}

fn now_millis() -> Result<i64, AppError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AppError::internal())?
        .as_millis();
    i64::try_from(millis).map_err(|_| AppError::internal())
}

fn sync_parent(path: &Path) -> Result<(), AppError> {
    File::open(path.parent().ok_or_else(AppError::internal)?)
        .and_then(|directory| directory.sync_all())
        .map_err(export_file_app_error)
}

fn import_file_error(error: std::io::Error) -> AppError {
    tracing::warn!(%error, "MySQL import file could not be opened");
    drop(error);
    AppError::not_found(
        "import_file_not_found",
        "The selected import file could not be opened",
    )
}

fn export_file_error(error: std::io::Error) -> TransferRunError {
    export_file_app_error(error).into()
}

fn export_file_app_error(error: std::io::Error) -> AppError {
    tracing::warn!(%error, "MySQL export file operation failed");
    drop(error);
    AppError::unavailable(
        "transfer_file_write_failed",
        "The export file could not be written",
    )
}

fn zip_error(error: zip::result::ZipError) -> TransferRunError {
    tracing::warn!(%error, "MySQL export archive operation failed");
    drop(error);
    AppError::unavailable(
        "transfer_archive_failed",
        "The export archive could not be written",
    )
    .into()
}

fn mysql_error(error: MysqlError) -> TransferRunError {
    let (kind, code, message) = match &error {
        MysqlError::Server(server) => (
            AppErrorKind::InvalidRequest,
            "mysql_transfer_query_failed",
            format!(
                "MySQL rejected the transfer operation (server error {})",
                server.code
            ),
        ),
        _ => (
            AppErrorKind::Unavailable,
            "mysql_transfer_unavailable",
            "The MySQL transfer operation could not be completed".to_owned(),
        ),
    };
    tracing::warn!(%error, "native MySQL transfer operation failed");
    drop(error);
    AppError::new(kind, chat2db_contract::ApiError::new(code, message)).into()
}

#[cfg(test)]
mod tests {
    use super::{safe_file_stem, select_table_name, sql_mode_has_no_backslash_escapes};

    #[test]
    fn insert_export_uses_parser_for_the_select_table() {
        assert_eq!(
            select_table_name("SELECT u.id FROM `app`.`user_account` AS u")
                .expect("select parses")
                .as_deref(),
            Some("user_account")
        );
        assert!(select_table_name("UPDATE user_account SET name = 'x'").is_err());
        assert!(select_table_name("SELECT 1; SELECT 2").is_err());
    }

    #[test]
    fn artifact_file_stems_do_not_create_paths() {
        assert_eq!(safe_file_stem("../../audit/log"), "______audit_log");
    }

    #[test]
    fn sql_mode_detection_is_case_insensitive_and_token_based() {
        assert!(sql_mode_has_no_backslash_escapes(
            "STRICT_TRANS_TABLES,NO_BACKSLASH_ESCAPES"
        ));
        assert!(sql_mode_has_no_backslash_escapes(" no_backslash_escapes "));
        assert!(!sql_mode_has_no_backslash_escapes(
            "STRICT_TRANS_TABLES,NO_BACKSLASH_ESCAPES_EXTRA"
        ));
    }
}
