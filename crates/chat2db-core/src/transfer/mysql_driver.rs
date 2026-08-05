use std::path::Path;

use chat2db_contract::{ImportFileRequest, SqlFileExportRequest, TransferArtifact};

use super::{
    QueryResultExportRequest, TableFileExportRequest, TransferJobKind, TransferJobSpec, mysql,
    single_table, transfer_artifact, validate_import_request, validate_transfer_scope,
};
use crate::{AppError, Application, native_mysql};

pub(crate) async fn import_file(
    application: &Application,
    request: ImportFileRequest,
) -> Result<TransferJobSpec, AppError> {
    validate_import_request(&request)?;
    validate_mysql_database(&request.database_name)?;
    native_mysql::resolve_native_connection(application, &request.datasource_id).await?;
    let file_name = Path::new(&request.file_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    Ok(TransferJobSpec::new(
        request.datasource_id.clone(),
        request.database_name.clone(),
        request.schema_name.clone(),
        request.table_name.clone(),
        TransferJobKind::ImportFile,
        format!("Import {file_name}"),
        move |application, context| async move {
            mysql::import_file(&application, request, &context).await
        },
    ))
}

pub(crate) async fn export_sql_file(
    application: &Application,
    request: SqlFileExportRequest,
) -> Result<TransferJobSpec, AppError> {
    validate_transfer_scope(&request.datasource_id, request.export_path.as_deref())?;
    validate_mysql_database(&request.database_name)?;
    native_mysql::resolve_native_connection(application, &request.datasource_id).await?;
    Ok(TransferJobSpec::new(
        request.datasource_id.clone(),
        request.database_name.clone(),
        request.schema_name.clone(),
        single_table(&request.table_names),
        TransferJobKind::ExportSql,
        format!("Export SQL {}", request.database_name),
        move |application, context| async move {
            mysql::export_sql(&application, request, &context).await
        },
    ))
}

pub(crate) async fn export_table_file(
    application: &Application,
    request: TableFileExportRequest,
) -> Result<TransferJobSpec, AppError> {
    validate_transfer_scope(&request.datasource_id, request.export_path.as_deref())?;
    validate_mysql_database(&request.database_name)?;
    if request.table_names.is_empty() {
        return Err(AppError::invalid(
            "missing_export_tables",
            "tableNames must contain at least one table",
        ));
    }
    native_mysql::resolve_native_connection(application, &request.datasource_id).await?;
    Ok(TransferJobSpec::new(
        request.datasource_id.clone(),
        request.database_name.clone(),
        request.schema_name.clone(),
        single_table(&request.table_names),
        TransferJobKind::ExportFile,
        format!(
            "Export {} {} table(s)",
            request.format.extension().to_ascii_uppercase(),
            request.table_names.len()
        ),
        move |application, context| async move {
            mysql::export_table_file(&application, request, &context).await
        },
    ))
}

pub(crate) async fn export_query_result(
    application: &Application,
    request: QueryResultExportRequest,
) -> Result<TransferArtifact, AppError> {
    native_mysql::resolve_native_connection(application, &request.datasource_id).await?;
    mysql::export_query_result(application, request)
        .await
        .map(transfer_artifact)
}

fn validate_mysql_database(database_name: &str) -> Result<(), AppError> {
    native_mysql::quote_identifier(database_name, "databaseName").map(|_| ())
}
