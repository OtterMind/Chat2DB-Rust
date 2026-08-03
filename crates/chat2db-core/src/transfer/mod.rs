mod class_generation;
mod format;
mod mysql;

use std::{
    collections::HashMap,
    fmt::Write as _,
    fs::File,
    path::{Path, PathBuf},
    time::Duration,
};

use chat2db_contract::{
    DmlExportRequest, GenerateMysqlClassRequest, GeneratedMysqlClassSet, ImportFileRequest,
    OtherFileExportRequest, SqlFileExportRequest, TransferArtifact, TransferTask,
    TransferTaskAccepted, TransferTaskKind, TransferTaskPage, TransferTaskStatus,
};
use chat2db_storage::{
    CreateTransferTask, ResolvedTransferArtifact, Storage, StorageError, StoredTransferTaskKind,
    StoredTransferTaskStatus, TransferArtifactRecord, TransferArtifactWriter, TransferTaskRecord,
};
use tokio::{
    sync::{Mutex, oneshot},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{AppError, Application, native_mysql, storage_call};

const MAX_TASK_PAGE_SIZE: u32 = 100;

pub(crate) struct TransferTaskHub {
    tasks: Mutex<HashMap<i64, ActiveTransferTask>>,
}

struct ActiveTransferTask {
    cancellation: CancellationToken,
    handle: JoinHandle<()>,
}

pub struct TransferArtifactDownload {
    pub artifact: TransferArtifact,
    pub path: PathBuf,
    pub file: File,
}

impl std::fmt::Debug for TransferArtifactDownload {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TransferArtifactDownload")
            .field("artifact", &self.artifact)
            .field("path", &self.path)
            .field("file", &self.file)
            .finish()
    }
}

impl TransferTaskHub {
    pub(crate) fn new() -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
        }
    }

    async fn insert(
        &self,
        task_id: i64,
        cancellation: CancellationToken,
        handle: JoinHandle<()>,
    ) -> Option<ActiveTransferTask> {
        self.tasks.lock().await.insert(
            task_id,
            ActiveTransferTask {
                cancellation,
                handle,
            },
        )
    }

    async fn remove(&self, task_id: i64) {
        self.tasks.lock().await.remove(&task_id);
    }

    async fn cancel(&self, task_id: i64) -> bool {
        let tasks = self.tasks.lock().await;
        let Some(task) = tasks.get(&task_id) else {
            return false;
        };
        task.cancellation.cancel();
        true
    }

    async fn cancel_all(&self) -> Vec<i64> {
        let tasks = self.tasks.lock().await;
        for task in tasks.values() {
            task.cancellation.cancel();
        }
        tasks.keys().copied().collect()
    }

    async fn take_all(&self) -> HashMap<i64, ActiveTransferTask> {
        std::mem::take(&mut *self.tasks.lock().await)
    }
}

pub(super) struct TransferContext {
    storage: Storage,
    task_id: i64,
    cancellation: CancellationToken,
}

impl TransferContext {
    fn new(storage: Storage, task_id: i64, cancellation: CancellationToken) -> Self {
        Self {
            storage,
            task_id,
            cancellation,
        }
    }

    pub(super) fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    pub(super) fn check_cancelled(&self) -> Result<(), TransferRunError> {
        if self.cancellation.is_cancelled() {
            Err(TransferRunError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub(super) fn begin_artifact(
        &self,
        file_name: &str,
        media_type: &str,
        format: &str,
        extension: &str,
    ) -> Result<TransferArtifactWriter, TransferRunError> {
        self.check_cancelled()?;
        self.storage
            .begin_transfer_artifact(
                Some(self.task_id),
                file_name,
                media_type,
                format,
                extension,
                None,
            )
            .map_err(TransferRunError::from)
    }

    pub(super) async fn progress(
        &self,
        current: u64,
        total: Option<u64>,
        description: &str,
        info: Option<&str>,
    ) -> Result<(), TransferRunError> {
        self.check_cancelled()?;
        let storage = self.storage.clone();
        let task_id = self.task_id;
        let description = description.to_owned();
        let info = info.map(str::to_owned);
        storage_call(move || {
            storage.update_transfer_progress(task_id, current, total, &description, info.as_deref())
        })
        .await
        .map_err(TransferRunError::from)
    }
}

pub(super) enum TransferRunError {
    Cancelled,
    Failed(AppError),
}

impl TransferRunError {
    pub(super) fn into_app_error(self) -> AppError {
        match self {
            Self::Cancelled => {
                AppError::unavailable("transfer_cancelled", "The transfer operation was cancelled")
            }
            Self::Failed(error) => error,
        }
    }
}

impl From<AppError> for TransferRunError {
    fn from(error: AppError) -> Self {
        Self::Failed(error)
    }
}

impl From<StorageError> for TransferRunError {
    fn from(error: StorageError) -> Self {
        Self::Failed(error.into())
    }
}

pub(super) enum TaskCompletion {
    WithoutArtifact(String),
    Artifact(TransferArtifactRecord),
}

enum TransferJob {
    Import(ImportFileRequest),
    SqlExport(SqlFileExportRequest),
    OtherExport(OtherFileExportRequest),
}

impl Application {
    /// Starts a durable native-MySQL CSV, XLS, XLSX, or SQL import task.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, storage, or runtime-shutdown failures.
    pub async fn import_mysql_file(
        &self,
        request: ImportFileRequest,
    ) -> Result<TransferTaskAccepted, AppError> {
        validate_import_request(&request)?;
        native_mysql::resolve_native_connection(self, &request.datasource_id).await?;
        let file_name = Path::new(&request.file_path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("file");
        self.start_transfer_job(
            CreateTransferTask {
                datasource_id: request.datasource_id.clone(),
                database_name: request.database_name.clone(),
                schema_name: request.schema_name.clone(),
                table_name: request.table_name.clone(),
                kind: StoredTransferTaskKind::ImportFile,
                task_name: format!("Import {file_name}"),
            },
            TransferJob::Import(request),
        )
        .await
    }

    /// Starts a durable native-MySQL SQL dump export task.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, storage, or runtime-shutdown failures.
    pub async fn export_mysql_sql_file(
        &self,
        request: SqlFileExportRequest,
    ) -> Result<TransferTaskAccepted, AppError> {
        validate_transfer_scope(
            &request.datasource_id,
            &request.database_name,
            request.export_path.as_deref(),
        )?;
        native_mysql::resolve_native_connection(self, &request.datasource_id).await?;
        self.start_transfer_job(
            CreateTransferTask {
                datasource_id: request.datasource_id.clone(),
                database_name: request.database_name.clone(),
                schema_name: request.schema_name.clone(),
                table_name: single_table(&request.table_names),
                kind: StoredTransferTaskKind::ExportSql,
                task_name: format!("Export SQL {}", request.database_name),
            },
            TransferJob::SqlExport(request),
        )
        .await
    }

    /// Starts a durable native-MySQL table file export task.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, storage, or runtime-shutdown failures.
    pub async fn export_mysql_other_file(
        &self,
        request: OtherFileExportRequest,
    ) -> Result<TransferTaskAccepted, AppError> {
        validate_transfer_scope(
            &request.datasource_id,
            &request.database_name,
            request.export_path.as_deref(),
        )?;
        if request.table_names.is_empty() {
            return Err(AppError::invalid(
                "missing_export_tables",
                "tableNames must contain at least one table",
            ));
        }
        native_mysql::resolve_native_connection(self, &request.datasource_id).await?;
        self.start_transfer_job(
            CreateTransferTask {
                datasource_id: request.datasource_id.clone(),
                database_name: request.database_name.clone(),
                schema_name: request.schema_name.clone(),
                table_name: single_table(&request.table_names),
                kind: StoredTransferTaskKind::ExportFile,
                task_name: format!(
                    "Export {} {} table(s)",
                    request.format.extension().to_ascii_uppercase(),
                    request.table_names.len()
                ),
            },
            TransferJob::OtherExport(request),
        )
        .await
    }

    /// Lists retained transfer tasks newest first.
    ///
    /// # Errors
    ///
    /// Returns invalid paging or durable-storage failures.
    pub async fn list_transfer_tasks(
        &self,
        page_no: u32,
        page_size: u32,
    ) -> Result<TransferTaskPage, AppError> {
        self.list_transfer_tasks_by_statuses(page_no, page_size, &[])
            .await
    }

    /// Lists retained transfer tasks after applying an optional status set.
    ///
    /// An empty status set selects every task. Filtering happens before
    /// pagination so legacy Community task tabs keep accurate totals.
    ///
    /// # Errors
    ///
    /// Returns invalid paging or durable-storage failures.
    pub async fn list_transfer_tasks_by_statuses(
        &self,
        page_no: u32,
        page_size: u32,
        statuses: &[TransferTaskStatus],
    ) -> Result<TransferTaskPage, AppError> {
        if page_no == 0 || page_size == 0 || page_size > MAX_TASK_PAGE_SIZE {
            return Err(AppError::invalid(
                "invalid_transfer_task_page",
                "pageNo must be positive and pageSize must be between 1 and 100",
            ));
        }
        let storage = self.require_storage()?;
        let tasks: Vec<TransferTask> = storage_call(move || storage.list_transfer_tasks())
            .await?
            .into_iter()
            .map(transfer_task)
            .filter(|task| statuses.is_empty() || statuses.contains(&task.status))
            .collect();
        let total = u64::try_from(tasks.len()).map_err(|_| AppError::internal())?;
        let start = usize::try_from((page_no - 1).saturating_mul(page_size))
            .map_err(|_| AppError::internal())?;
        let items = tasks
            .into_iter()
            .skip(start)
            .take(usize::try_from(page_size).map_err(|_| AppError::internal())?)
            .collect();
        Ok(TransferTaskPage {
            items,
            total,
            page_no,
            page_size,
        })
    }

    /// Reads one retained transfer task.
    ///
    /// # Errors
    ///
    /// Returns not-found or durable-storage failures.
    pub async fn transfer_task(&self, task_id: i64) -> Result<TransferTask, AppError> {
        let storage = self.require_storage()?;
        storage_call(move || storage.get_transfer_task(task_id))
            .await?
            .map(transfer_task)
            .ok_or_else(|| {
                AppError::not_found(
                    "transfer_task_not_found",
                    format!("Transfer task {task_id} does not exist"),
                )
            })
    }

    /// Requests cooperative cancellation of one queued or running transfer.
    ///
    /// # Errors
    ///
    /// Returns not-found or durable-storage failures.
    pub async fn stop_transfer_task(&self, task_id: i64) -> Result<(), AppError> {
        let storage = self.require_storage()?;
        let changed = storage_call(move || storage.request_transfer_cancel(task_id)).await?;
        if changed {
            self.inner.transfer_tasks.cancel(task_id).await;
        }
        Ok(())
    }

    /// Resolves a managed artifact and its owner-only local path for a delivery adapter.
    ///
    /// # Errors
    ///
    /// Returns not-found, expiry, corruption, or durable-storage failures.
    pub async fn transfer_artifact_download(
        &self,
        artifact_id: &str,
    ) -> Result<TransferArtifactDownload, AppError> {
        if artifact_id.trim().is_empty() {
            return Err(AppError::invalid(
                "invalid_transfer_artifact",
                "artifactId cannot be empty",
            ));
        }
        let storage = self.require_storage()?;
        let artifact_id = artifact_id.to_owned();
        let resolved =
            storage_call(move || storage.resolve_transfer_artifact(&artifact_id)).await?;
        Ok(artifact_download(resolved))
    }

    /// Resolves the managed artifact produced by one completed transfer task.
    ///
    /// # Errors
    ///
    /// Returns not-found, incomplete-task, expiry, corruption, or durable-storage failures.
    pub async fn transfer_task_artifact_download(
        &self,
        task_id: i64,
    ) -> Result<TransferArtifactDownload, AppError> {
        let task = self.transfer_task(task_id).await?;
        let artifact_id = task.artifact_id.ok_or_else(|| {
            AppError::not_found(
                "transfer_artifact_not_found",
                format!("Transfer task {task_id} has no downloadable artifact"),
            )
        })?;
        self.transfer_artifact_download(&artifact_id).await
    }

    /// Streams one DML result into a temporary managed CSV, XLSX, or INSERT artifact.
    ///
    /// # Errors
    ///
    /// Returns SQL analysis, datasource, query, format, or storage failures.
    pub async fn export_mysql_dml(
        &self,
        request: DmlExportRequest,
    ) -> Result<TransferArtifact, AppError> {
        native_mysql::resolve_native_connection(self, &request.datasource_id).await?;
        mysql::export_dml(self, request)
            .await
            .map(transfer_artifact)
    }

    /// Generates `MyBatis` Plus entity, Mapper, and Mapper XML files from native `MySQL` metadata.
    ///
    /// # Errors
    ///
    /// Returns validation, metadata, datasource, or filesystem failures.
    pub async fn generate_mysql_classes(
        &self,
        request: GenerateMysqlClassRequest,
    ) -> Result<GeneratedMysqlClassSet, AppError> {
        native_mysql::resolve_native_connection(self, &request.datasource_id).await?;
        class_generation::generate(self, request).await
    }

    /// Generates the same `MyBatis` Plus files as Desktop in a temporary managed ZIP.
    ///
    /// # Errors
    ///
    /// Returns validation, metadata, datasource, archive, or storage failures.
    pub async fn generate_mysql_class_archive(
        &self,
        request: GenerateMysqlClassRequest,
    ) -> Result<TransferArtifact, AppError> {
        native_mysql::resolve_native_connection(self, &request.datasource_id).await?;
        class_generation::generate_archive(self, request)
            .await
            .map(transfer_artifact)
    }

    async fn start_transfer_job(
        &self,
        task: CreateTransferTask,
        job: TransferJob,
    ) -> Result<TransferTaskAccepted, AppError> {
        let accepting_work = self.inner.accepting_work.lock().await;
        if !*accepting_work {
            return Err(AppError::unavailable(
                "runtime_shutting_down",
                "The Chat2DB runtime is shutting down",
            ));
        }
        let storage = self.require_storage()?;
        let task_record = storage_call({
            let storage = storage.clone();
            move || storage.create_transfer_task(&task)
        })
        .await?;
        let task_id = task_record.id;
        let cancellation = CancellationToken::new();
        let run_cancellation = cancellation.clone();
        let application = self.clone();
        let (registered, wait_for_registration) = oneshot::channel();
        let handle = tokio::spawn(async move {
            if wait_for_registration.await.is_err() {
                return;
            }
            application
                .run_transfer_task(task_id, job, run_cancellation)
                .await;
            application.inner.transfer_tasks.remove(task_id).await;
        });
        let replaced = self
            .inner
            .transfer_tasks
            .insert(task_id, cancellation, handle)
            .await;
        debug_assert!(replaced.is_none(), "transfer task ids must be unique");
        if registered.send(()).is_err() {
            if let Some(task) = self
                .inner
                .transfer_tasks
                .tasks
                .lock()
                .await
                .remove(&task_id)
            {
                task.handle.abort();
            }
            let storage = storage.clone();
            let _ = storage_call(move || {
                storage.fail_transfer_task(task_id, "Transfer task registration failed")
            })
            .await;
            return Err(AppError::internal());
        }
        drop(accepting_work);
        Ok(TransferTaskAccepted { task_id })
    }

    async fn run_transfer_task(
        &self,
        task_id: i64,
        job: TransferJob,
        cancellation: CancellationToken,
    ) {
        let Some(storage) = self.storage().cloned() else {
            return;
        };
        if cancellation.is_cancelled() {
            let _ = storage_call(move || storage.request_transfer_cancel(task_id)).await;
            return;
        }
        let start_storage = storage.clone();
        if let Err(error) = storage_call(move || start_storage.start_transfer_task(task_id)).await {
            if !cancellation.is_cancelled() {
                tracing::warn!(task_id, %error, "transfer task could not enter running state");
            }
            return;
        }
        let context = TransferContext::new(storage.clone(), task_id, cancellation.clone());
        let result = match job {
            TransferJob::Import(request) => mysql::import_file(self, request, &context).await,
            TransferJob::SqlExport(request) => mysql::export_sql(self, request, &context).await,
            TransferJob::OtherExport(request) => mysql::export_other(self, request, &context).await,
        };
        match result {
            Ok(TaskCompletion::WithoutArtifact(message)) => {
                let complete_storage = storage.clone();
                if let Err(error) =
                    storage_call(move || complete_storage.complete_transfer_task(task_id, &message))
                        .await
                {
                    tracing::warn!(task_id, %error, "transfer task completion could not be persisted");
                }
            }
            Ok(TaskCompletion::Artifact(artifact)) => {
                debug_assert_eq!(artifact.task_id, Some(task_id));
            }
            Err(TransferRunError::Cancelled) => {
                let cancel_storage = storage.clone();
                let _ = storage_call(move || {
                    cancel_storage.cancel_transfer_task(task_id, "Transfer cancelled by request")
                })
                .await;
            }
            Err(TransferRunError::Failed(error)) => {
                let message = error.api_error().message;
                tracing::warn!(task_id, code = %error.api_error().code, "transfer task failed");
                let fail_storage = storage.clone();
                let _ =
                    storage_call(move || fail_storage.fail_transfer_task(task_id, &message)).await;
            }
        }
    }

    pub(crate) async fn begin_transfer_shutdown(&self) {
        let task_ids = self.inner.transfer_tasks.cancel_all().await;
        let Some(storage) = self.storage().cloned() else {
            return;
        };
        for task_id in task_ids {
            let storage = storage.clone();
            let _ = storage_call(move || storage.request_transfer_cancel(task_id)).await;
        }
    }

    pub(crate) async fn join_transfer_tasks(&self, timeout: Duration) {
        let tasks = self.inner.transfer_tasks.take_all().await;
        let deadline = tokio::time::Instant::now() + timeout;
        let storage = self.storage().cloned();
        for (task_id, mut task) in tasks {
            let terminal_message = match tokio::time::timeout_at(deadline, &mut task.handle).await {
                Ok(Ok(())) => None,
                Ok(Err(_)) => Some("Transfer worker stopped unexpectedly"),
                Err(_) => {
                    task.handle.abort();
                    Some("Transfer stopped during runtime shutdown")
                }
            };
            if let (Some(storage), Some(message)) = (storage.clone(), terminal_message) {
                let _ = storage_call(move || storage.cancel_transfer_task(task_id, message)).await;
            }
        }
    }
}

fn validate_import_request(request: &ImportFileRequest) -> Result<(), AppError> {
    validate_transfer_scope(&request.datasource_id, &request.database_name, None)?;
    if request.file_path.trim().is_empty() || request.file_path.contains('\0') {
        return Err(AppError::invalid(
            "invalid_import_file",
            "filePath cannot be empty",
        ));
    }
    if request.format != chat2db_contract::TransferFileFormat::Sql
        && request.table_name.as_deref().is_none_or(str::is_empty)
    {
        return Err(AppError::invalid(
            "missing_import_table",
            "tableName is required for tabular imports",
        ));
    }
    Ok(())
}

fn validate_transfer_scope(
    datasource_id: &str,
    database_name: &str,
    export_path: Option<&str>,
) -> Result<(), AppError> {
    if datasource_id.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_transfer_request",
            "datasourceId cannot be empty",
        ));
    }
    native_mysql::quote_identifier(database_name, "databaseName")?;
    if export_path.is_some_and(|path| path.trim().is_empty() || path.contains('\0')) {
        return Err(AppError::invalid(
            "invalid_export_path",
            "exportPath must be a local directory",
        ));
    }
    Ok(())
}

fn single_table(table_names: &[String]) -> Option<String> {
    (table_names.len() == 1).then(|| table_names[0].clone())
}

fn transfer_task(record: TransferTaskRecord) -> TransferTask {
    TransferTask {
        id: record.id,
        datasource_id: record.datasource_id,
        database_name: record.database_name,
        schema_name: record.schema_name,
        table_name: record.table_name,
        kind: match record.kind {
            StoredTransferTaskKind::ImportFile => TransferTaskKind::ImportFile,
            StoredTransferTaskKind::ExportSql => TransferTaskKind::ExportSql,
            StoredTransferTaskKind::ExportFile => TransferTaskKind::ExportFile,
        },
        status: match record.status {
            StoredTransferTaskStatus::Queued => TransferTaskStatus::Queued,
            StoredTransferTaskStatus::Running => TransferTaskStatus::Running,
            StoredTransferTaskStatus::Succeeded => TransferTaskStatus::Succeeded,
            StoredTransferTaskStatus::Failed => TransferTaskStatus::Failed,
            StoredTransferTaskStatus::Cancelled => TransferTaskStatus::Cancelled,
            StoredTransferTaskStatus::Interrupted => TransferTaskStatus::Interrupted,
        },
        task_name: record.task_name,
        progress_current: record.progress_current.to_string(),
        progress_total: record.progress_total.map(|value| value.to_string()),
        progress_description: record.progress_description,
        info_log: record.info_log,
        error_log: record.error_log,
        artifact_id: record.artifact_id,
        cancel_requested: record.cancel_requested,
        created_at_ms: record.created_at_ms.to_string(),
        updated_at_ms: record.updated_at_ms.to_string(),
        finished_at_ms: record.finished_at_ms.map(|value| value.to_string()),
    }
}

fn artifact_download(resolved: ResolvedTransferArtifact) -> TransferArtifactDownload {
    TransferArtifactDownload {
        artifact: transfer_artifact(resolved.record),
        path: resolved.path,
        file: resolved.file,
    }
}

fn transfer_artifact(record: TransferArtifactRecord) -> TransferArtifact {
    TransferArtifact {
        id: record.id,
        task_id: record.task_id,
        file_name: record.file_name,
        media_type: record.media_type,
        format: record.format,
        byte_count: record.byte_count.to_string(),
        sha256: sha256_hex(&record.sha256),
        created_at_ms: record.created_at_ms.to_string(),
    }
}

fn sha256_hex(digest: &[u8; 32]) -> String {
    digest
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

#[cfg(test)]
mod tests {
    use chat2db_storage::{StoredTransferTaskKind, StoredTransferTaskStatus, TransferTaskRecord};

    use super::transfer_task;

    #[test]
    fn durable_interrupted_status_is_preserved_for_transport_projection() {
        let projected = transfer_task(TransferTaskRecord {
            id: 7,
            datasource_id: "mysql".to_owned(),
            database_name: "app".to_owned(),
            schema_name: String::new(),
            table_name: None,
            kind: StoredTransferTaskKind::ExportSql,
            status: StoredTransferTaskStatus::Interrupted,
            task_name: "export".to_owned(),
            progress_current: 1,
            progress_total: Some(2),
            progress_description: "Interrupted".to_owned(),
            info_log: String::new(),
            error_log: "stopped".to_owned(),
            cancel_requested: false,
            created_at_ms: 1,
            updated_at_ms: 2,
            finished_at_ms: Some(2),
            artifact_id: None,
        });
        assert_eq!(
            projected.status,
            chat2db_contract::TransferTaskStatus::Interrupted
        );
    }
}
