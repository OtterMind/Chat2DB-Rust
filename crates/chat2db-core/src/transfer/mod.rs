mod class_generation;
mod format;
mod mysql;
pub(crate) mod mysql_driver;

use std::{
    collections::HashMap, fmt::Write as _, fs::File, future::Future, path::PathBuf, pin::Pin,
    sync::Arc, time::Duration,
};

use chat2db_contract::{
    DmlExportFormat, DmlExportRequest, DmlExportSize, GenerateMysqlClassRequest,
    GeneratedMysqlClassSet, ImportFileRequest, OtherFileExportRequest, SqlFileExportRequest,
    TransferArtifact, TransferFileFormat, TransferTask, TransferTaskAccepted, TransferTaskKind,
    TransferTaskPage, TransferTaskStatus,
};
use chat2db_storage::{
    CreateTransferTask, ResolvedTransferArtifact, Storage, StorageError, StoredTransferTaskKind,
    StoredTransferTaskStatus, TransferArtifactRecord, TransferArtifactWriter, TransferTaskRecord,
};
use tokio::{
    sync::{Mutex, oneshot},
    task::{AbortHandle, JoinHandle},
};
use tokio_util::sync::CancellationToken;

use crate::{AppError, Application, storage_call};

const MAX_TASK_PAGE_SIZE: u32 = 100;
const MAX_TRANSFER_FAILURE_MESSAGE_BYTES: usize = 64 * 1024;
const TRANSFER_FAILURE_TRUNCATION_SUFFIX: &str = "\n[truncated]";
const TERMINAL_RETRY_INITIAL_DELAY: Duration = Duration::from_millis(25);
const TERMINAL_RETRY_MAX_DELAY: Duration = Duration::from_secs(1);
const TERMINAL_RECOVERY_MESSAGE: &str =
    "Transfer terminal state will be recovered when the runtime restarts";

pub(crate) struct TransferTaskHub {
    tasks: Mutex<HashMap<i64, ActiveTransferTask>>,
}

struct ActiveTransferTask {
    control: TransferTaskControl,
    handle: JoinHandle<()>,
}

struct AbortTaskOnDrop(AbortHandle);

impl Drop for AbortTaskOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Clone)]
struct TransferTaskControl {
    cancellation: CancellationToken,
    terminal_gate: Arc<Mutex<()>>,
}

impl TransferTaskControl {
    fn new() -> Self {
        Self {
            cancellation: CancellationToken::new(),
            terminal_gate: Arc::new(Mutex::new(())),
        }
    }
}

pub struct TransferArtifactDownload {
    pub artifact: TransferArtifact,
    pub path: PathBuf,
    pub file: File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableFileExportRequest {
    pub(crate) datasource_id: String,
    pub(crate) database_name: String,
    pub(crate) schema_name: String,
    pub(crate) table_names: Vec<String>,
    pub(crate) format: TransferFileFormat,
    pub(crate) contains_header: bool,
    pub(crate) export_path: Option<String>,
}

impl From<OtherFileExportRequest> for TableFileExportRequest {
    fn from(request: OtherFileExportRequest) -> Self {
        Self {
            datasource_id: request.datasource_id,
            database_name: request.database_name,
            schema_name: request.schema_name,
            table_names: request.table_names,
            format: request.format,
            contains_header: request.contains_header,
            export_path: request.export_path,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryResultExportScope {
    CurrentPage,
    All,
}

impl From<DmlExportSize> for QueryResultExportScope {
    fn from(scope: DmlExportSize) -> Self {
        match scope {
            DmlExportSize::CurrentPage => Self::CurrentPage,
            DmlExportSize::All => Self::All,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryResultExportFormat {
    Csv,
    Xlsx,
    Insert,
}

impl From<DmlExportFormat> for QueryResultExportFormat {
    fn from(format: DmlExportFormat) -> Self {
        match format {
            DmlExportFormat::Csv => Self::Csv,
            DmlExportFormat::Xlsx => Self::Xlsx,
            DmlExportFormat::Insert => Self::Insert,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueryResultExportRequest {
    pub(crate) datasource_id: String,
    pub(crate) database_name: String,
    pub(crate) schema_name: String,
    pub(crate) sql: String,
    pub(crate) original_sql: String,
    pub(crate) result_set_id: Option<u32>,
    pub(crate) scope: QueryResultExportScope,
    pub(crate) format: QueryResultExportFormat,
}

impl From<DmlExportRequest> for QueryResultExportRequest {
    fn from(request: DmlExportRequest) -> Self {
        Self {
            datasource_id: request.datasource_id,
            database_name: request.database_name,
            schema_name: request.schema_name,
            sql: request.sql,
            original_sql: request.original_sql,
            result_set_id: request.result_set_id,
            scope: request.export_size.into(),
            format: request.format.into(),
        }
    }
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
        control: TransferTaskControl,
        handle: JoinHandle<()>,
    ) -> Option<ActiveTransferTask> {
        self.tasks
            .lock()
            .await
            .insert(task_id, ActiveTransferTask { control, handle })
    }

    async fn remove(&self, task_id: i64) {
        self.tasks.lock().await.remove(&task_id);
    }

    async fn control(&self, task_id: i64) -> Option<TransferTaskControl> {
        self.tasks
            .lock()
            .await
            .get(&task_id)
            .map(|task| task.control.clone())
    }

    async fn controls(&self) -> Vec<(i64, TransferTaskControl)> {
        self.tasks
            .lock()
            .await
            .iter()
            .map(|(task_id, task)| (*task_id, task.control.clone()))
            .collect()
    }

    async fn take_all(&self) -> HashMap<i64, ActiveTransferTask> {
        std::mem::take(&mut *self.tasks.lock().await)
    }
}

pub(crate) struct TransferContext {
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

    pub(crate) fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    pub(crate) fn check_cancelled(&self) -> Result<(), TransferRunError> {
        if self.cancellation.is_cancelled() {
            Err(TransferRunError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub(crate) fn begin_artifact(
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

    pub(crate) async fn progress(
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

pub(crate) enum TransferRunError {
    Cancelled,
    Failed(AppError),
}

impl TransferRunError {
    pub(crate) fn into_app_error(self) -> AppError {
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

pub(crate) enum TaskCompletion {
    WithoutArtifact(String),
    Artifact(PendingTransferArtifact),
}

type TransferJobFuture =
    Pin<Box<dyn Future<Output = Result<TaskCompletion, TransferRunError>> + Send + 'static>>;

type TransferJobRunner =
    Box<dyn FnOnce(Application, TransferContext) -> TransferJobFuture + Send + 'static>;

type TransferArtifactFuture =
    Pin<Box<dyn Future<Output = Result<TransferArtifactRecord, AppError>> + Send + 'static>>;

type TransferArtifactFinalizer = Box<dyn FnOnce() -> TransferArtifactFuture + Send + 'static>;

pub(crate) struct PendingTransferArtifact {
    finalizer: TransferArtifactFinalizer,
}

impl PendingTransferArtifact {
    pub(crate) fn new<Finalizer, FinalizerFuture>(finalizer: Finalizer) -> Self
    where
        Finalizer: FnOnce() -> FinalizerFuture + Send + 'static,
        FinalizerFuture: Future<Output = Result<TransferArtifactRecord, AppError>> + Send + 'static,
    {
        Self {
            finalizer: Box::new(move || Box::pin(finalizer())),
        }
    }

    async fn finalize(self) -> Result<TransferArtifactRecord, AppError> {
        (self.finalizer)().await
    }
}

#[derive(Clone, Copy)]
pub(crate) enum TransferJobKind {
    ImportFile,
    ExportSql,
    ExportFile,
}

pub(crate) struct TransferJobSpec {
    datasource_id: String,
    database_name: String,
    schema_name: String,
    table_name: Option<String>,
    kind: TransferJobKind,
    task_name: String,
    runner: TransferJobRunner,
}

impl TransferJobSpec {
    pub(crate) fn new<Runner, RunnerFuture>(
        datasource_id: String,
        database_name: String,
        schema_name: String,
        table_name: Option<String>,
        kind: TransferJobKind,
        task_name: String,
        runner: Runner,
    ) -> Self
    where
        Runner: FnOnce(Application, TransferContext) -> RunnerFuture + Send + 'static,
        RunnerFuture: Future<Output = Result<TaskCompletion, TransferRunError>> + Send + 'static,
    {
        Self {
            datasource_id,
            database_name,
            schema_name,
            table_name,
            kind,
            task_name,
            runner: Box::new(move |application, context| Box::pin(runner(application, context))),
        }
    }

    fn into_parts(self) -> (CreateTransferTask, TransferJobRunner) {
        let kind = match self.kind {
            TransferJobKind::ImportFile => StoredTransferTaskKind::ImportFile,
            TransferJobKind::ExportSql => StoredTransferTaskKind::ExportSql,
            TransferJobKind::ExportFile => StoredTransferTaskKind::ExportFile,
        };
        (
            CreateTransferTask {
                datasource_id: self.datasource_id,
                database_name: self.database_name,
                schema_name: self.schema_name,
                table_name: self.table_name,
                kind,
                task_name: self.task_name,
            },
            self.runner,
        )
    }
}

enum TransferTerminalState {
    Succeeded(String),
    Artifact(PendingTransferArtifact),
    Cancelled(String),
    Failed(String),
}

impl Application {
    /// Starts a durable import task through the datasource's native driver.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, storage, or runtime-shutdown failures.
    pub async fn import_file(
        &self,
        request: ImportFileRequest,
    ) -> Result<TransferTaskAccepted, AppError> {
        let driver = self
            .require_native_driver_for_datasource(&request.datasource_id)
            .await?;
        let transfer = driver.transfer().ok_or_else(|| {
            AppError::invalid(
                "native_transfer_capability_not_available",
                "The native Rust driver does not implement import and export operations",
            )
        })?;
        let spec = transfer.import_file(self, request).await?;
        self.start_transfer_job(spec).await
    }

    /// Retained `MySQL` compatibility name for [`Self::import_file`].
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, storage, or runtime-shutdown failures.
    pub async fn import_mysql_file(
        &self,
        request: ImportFileRequest,
    ) -> Result<TransferTaskAccepted, AppError> {
        self.import_file(request).await
    }

    /// Starts a durable SQL dump export through the datasource's native driver.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, storage, or runtime-shutdown failures.
    pub async fn export_sql_file(
        &self,
        request: SqlFileExportRequest,
    ) -> Result<TransferTaskAccepted, AppError> {
        let driver = self
            .require_native_driver_for_datasource(&request.datasource_id)
            .await?;
        let transfer = driver.transfer().ok_or_else(|| {
            AppError::invalid(
                "native_transfer_capability_not_available",
                "The native Rust driver does not implement import and export operations",
            )
        })?;
        let spec = transfer.export_sql_file(self, request).await?;
        self.start_transfer_job(spec).await
    }

    /// Retained `MySQL` compatibility name for [`Self::export_sql_file`].
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, storage, or runtime-shutdown failures.
    pub async fn export_mysql_sql_file(
        &self,
        request: SqlFileExportRequest,
    ) -> Result<TransferTaskAccepted, AppError> {
        self.export_sql_file(request).await
    }

    /// Starts a durable table-file export through the datasource's native driver.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, storage, or runtime-shutdown failures.
    pub(crate) async fn export_table_file(
        &self,
        request: TableFileExportRequest,
    ) -> Result<TransferTaskAccepted, AppError> {
        let driver = self
            .require_native_driver_for_datasource(&request.datasource_id)
            .await?;
        let transfer = driver.transfer().ok_or_else(|| {
            AppError::invalid(
                "native_transfer_capability_not_available",
                "The native Rust driver does not implement import and export operations",
            )
        })?;
        let spec = transfer.export_table_file(self, request).await?;
        self.start_transfer_job(spec).await
    }

    /// Retained compatibility entry point for [`Self::export_table_file`].
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, storage, or runtime-shutdown failures.
    pub async fn export_other_file(
        &self,
        request: OtherFileExportRequest,
    ) -> Result<TransferTaskAccepted, AppError> {
        self.export_table_file(request.into()).await
    }

    /// Retained `MySQL` compatibility name for [`Self::export_table_file`].
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, storage, or runtime-shutdown failures.
    pub async fn export_mysql_other_file(
        &self,
        request: OtherFileExportRequest,
    ) -> Result<TransferTaskAccepted, AppError> {
        self.export_other_file(request).await
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
    /// pagination so legacy task tabs keep accurate totals.
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
        if let Some(control) = self.inner.transfer_tasks.control(task_id).await {
            control.cancellation.cancel();
        }
        persist_cancel_request(&storage, task_id).await?;
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

    /// Streams one query result into a temporary managed CSV, XLSX, or INSERT artifact.
    ///
    /// # Errors
    ///
    /// Returns SQL analysis, datasource, query, format, or storage failures.
    pub(crate) async fn export_query_result(
        &self,
        request: QueryResultExportRequest,
    ) -> Result<TransferArtifact, AppError> {
        let driver = self
            .require_native_driver_for_datasource(&request.datasource_id)
            .await?;
        let transfer = driver.transfer().ok_or_else(|| {
            AppError::invalid(
                "native_transfer_capability_not_available",
                "The native Rust driver does not implement import and export operations",
            )
        })?;
        transfer.export_query_result(self, request).await
    }

    /// Retained compatibility entry point for [`Self::export_query_result`].
    ///
    /// # Errors
    ///
    /// Returns SQL analysis, datasource, query, format, or storage failures.
    pub async fn export_dml(
        &self,
        request: DmlExportRequest,
    ) -> Result<TransferArtifact, AppError> {
        self.export_query_result(request.into()).await
    }

    /// Retained `MySQL` compatibility name for [`Self::export_query_result`].
    ///
    /// # Errors
    ///
    /// Returns SQL analysis, datasource, query, format, or storage failures.
    pub async fn export_mysql_dml(
        &self,
        request: DmlExportRequest,
    ) -> Result<TransferArtifact, AppError> {
        self.export_dml(request).await
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
        self.require_native_driver_for_datasource(&request.datasource_id)
            .await?;
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
        self.require_native_driver_for_datasource(&request.datasource_id)
            .await?;
        class_generation::generate_archive(self, request)
            .await
            .map(transfer_artifact)
    }

    pub(crate) async fn start_transfer_job(
        &self,
        spec: TransferJobSpec,
    ) -> Result<TransferTaskAccepted, AppError> {
        let accepting_work = self.inner.accepting_work.lock().await;
        if !*accepting_work {
            return Err(AppError::unavailable(
                "runtime_shutting_down",
                "The Chat2DB runtime is shutting down",
            ));
        }
        let (task, runner) = spec.into_parts();
        let storage = self.require_storage()?;
        let task_record = storage_call({
            let storage = storage.clone();
            move || storage.create_transfer_task(&task)
        })
        .await?;
        let task_id = task_record.id;
        let control = TransferTaskControl::new();
        let run_control = control.clone();
        let run_storage = storage.clone();
        let application = self.clone();
        let (registered, wait_for_registration) = oneshot::channel();
        let handle = tokio::spawn(async move {
            if wait_for_registration.await.is_err() {
                return;
            }
            let run_application = application.clone();
            let recovery_storage = run_storage.clone();
            let recovery_control = run_control.clone();
            let mut worker = tokio::spawn(async move {
                run_application
                    .run_transfer_task(task_id, runner, run_storage, run_control)
                    .await;
            });
            let _abort_worker = AbortTaskOnDrop(worker.abort_handle());
            match (&mut worker).await {
                Ok(()) => {}
                Err(error) if error.is_panic() => {
                    tracing::error!(task_id, "transfer worker panicked");
                    finalize_transfer_task(
                        &recovery_storage,
                        task_id,
                        &recovery_control,
                        TransferTerminalState::Failed("Transfer worker panicked".to_owned()),
                        None,
                    )
                    .await;
                }
                Err(error) => {
                    tracing::error!(task_id, %error, "transfer worker stopped unexpectedly");
                    finalize_transfer_task(
                        &recovery_storage,
                        task_id,
                        &recovery_control,
                        TransferTerminalState::Failed(
                            "Transfer worker stopped unexpectedly".to_owned(),
                        ),
                        None,
                    )
                    .await;
                }
            }
            application.inner.transfer_tasks.remove(task_id).await;
        });
        let replaced = self
            .inner
            .transfer_tasks
            .insert(task_id, control.clone(), handle)
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
                let _ = task.handle.await;
            }
            finalize_transfer_task(
                &storage,
                task_id,
                &control,
                TransferTerminalState::Failed("Transfer task registration failed".to_owned()),
                None,
            )
            .await;
            return Err(AppError::internal());
        }
        drop(accepting_work);
        Ok(TransferTaskAccepted { task_id })
    }

    async fn run_transfer_task(
        &self,
        task_id: i64,
        runner: TransferJobRunner,
        storage: Storage,
        control: TransferTaskControl,
    ) {
        if control.cancellation.is_cancelled() {
            finalize_transfer_task(
                &storage,
                task_id,
                &control,
                TransferTerminalState::Cancelled("Transfer cancelled before startup".to_owned()),
                None,
            )
            .await;
            return;
        }
        let start_storage = storage.clone();
        if let Err(error) = storage_call(move || start_storage.start_transfer_task(task_id)).await {
            tracing::warn!(task_id, %error, "transfer task could not enter running state");
            finalize_transfer_task(
                &storage,
                task_id,
                &control,
                TransferTerminalState::Failed("Transfer task could not start".to_owned()),
                None,
            )
            .await;
            return;
        }
        let context = TransferContext::new(storage.clone(), task_id, control.cancellation.clone());
        let result = runner(self.clone(), context).await;
        let terminal = match result {
            Ok(TaskCompletion::WithoutArtifact(message)) => {
                TransferTerminalState::Succeeded(message)
            }
            Ok(TaskCompletion::Artifact(artifact)) => TransferTerminalState::Artifact(artifact),
            Err(TransferRunError::Cancelled) => {
                TransferTerminalState::Cancelled("Transfer cancelled by request".to_owned())
            }
            Err(TransferRunError::Failed(error)) => {
                let error = error.api_error();
                tracing::warn!(task_id, code = %error.code, "transfer task failed");
                TransferTerminalState::Failed(truncate_transfer_failure_message(error.message))
            }
        };
        finalize_transfer_task(&storage, task_id, &control, terminal, None).await;
    }

    pub(crate) async fn begin_transfer_shutdown(&self) {
        let controls = self.inner.transfer_tasks.controls().await;
        for (_, control) in &controls {
            control.cancellation.cancel();
        }
    }

    pub(crate) async fn join_transfer_tasks(&self, timeout: Duration) {
        let tasks = self.inner.transfer_tasks.take_all().await;
        let deadline = tokio::time::Instant::now() + timeout;
        let storage = self.storage().cloned();
        for (task_id, mut task) in tasks {
            let terminal = match tokio::time::timeout_at(deadline, &mut task.handle).await {
                Ok(Ok(())) => None,
                Ok(Err(error)) => {
                    tracing::error!(task_id, %error, "transfer worker monitor stopped unexpectedly");
                    Some(TransferTerminalState::Failed(
                        "Transfer worker stopped unexpectedly".to_owned(),
                    ))
                }
                Err(_) => {
                    task.handle.abort();
                    let _ = task.handle.await;
                    Some(TransferTerminalState::Cancelled(
                        "Transfer stopped during runtime shutdown".to_owned(),
                    ))
                }
            };
            if let (Some(storage), Some(terminal)) = (storage.as_ref(), terminal) {
                let finalized = finalize_transfer_task(
                    storage,
                    task_id,
                    &task.control,
                    terminal,
                    Some(deadline),
                )
                .await;
                if !finalized {
                    tracing::error!(task_id, "{TERMINAL_RECOVERY_MESSAGE}");
                }
            }
        }
    }
}

async fn persist_cancel_request(storage: &Storage, task_id: i64) -> Result<bool, AppError> {
    let storage = storage.clone();
    storage_call(move || storage.request_transfer_cancel(task_id)).await
}

async fn load_transfer_task(
    storage: &Storage,
    task_id: i64,
) -> Result<Option<TransferTaskRecord>, AppError> {
    let storage = storage.clone();
    storage_call(move || storage.get_transfer_task(task_id)).await
}

const fn stored_transfer_is_terminal(status: StoredTransferTaskStatus) -> bool {
    matches!(
        status,
        StoredTransferTaskStatus::Succeeded
            | StoredTransferTaskStatus::Failed
            | StoredTransferTaskStatus::Cancelled
            | StoredTransferTaskStatus::Interrupted
    )
}

async fn finalize_transfer_task(
    storage: &Storage,
    task_id: i64,
    control: &TransferTaskControl,
    mut terminal: TransferTerminalState,
    retry_deadline: Option<tokio::time::Instant>,
) -> bool {
    let mut attempt = 0_u32;
    let mut retry_delay = TERMINAL_RETRY_INITIAL_DELAY;
    loop {
        attempt = attempt.saturating_add(1);
        let finalization = async {
            let _terminal = control.terminal_gate.lock().await;
            finalize_transfer_once(storage, task_id, control, &mut terminal).await
        };
        let result = if let Some(deadline) = retry_deadline {
            match tokio::time::timeout_at(deadline, finalization).await {
                Ok(result) => result,
                Err(_) => return false,
            }
        } else {
            finalization.await
        };
        match result {
            Ok(()) => return true,
            Err(error) => {
                tracing::warn!(
                    task_id,
                    attempt,
                    %error,
                    "transfer terminal state could not be persisted"
                );
                if retry_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
                    return false;
                }
            }
        }
        let delay = retry_deadline.map_or(retry_delay, |deadline| {
            retry_delay.min(deadline.saturating_duration_since(tokio::time::Instant::now()))
        });
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        retry_delay = retry_delay.saturating_mul(2).min(TERMINAL_RETRY_MAX_DELAY);
    }
}

fn truncate_transfer_failure_message(mut message: String) -> String {
    if message.len() <= MAX_TRANSFER_FAILURE_MESSAGE_BYTES {
        return message;
    }
    let mut boundary =
        MAX_TRANSFER_FAILURE_MESSAGE_BYTES - TRANSFER_FAILURE_TRUNCATION_SUFFIX.len();
    while !message.is_char_boundary(boundary) {
        boundary -= 1;
    }
    message.truncate(boundary);
    message.push_str(TRANSFER_FAILURE_TRUNCATION_SUFFIX);
    message
}

async fn finalize_transfer_once(
    storage: &Storage,
    task_id: i64,
    control: &TransferTaskControl,
    terminal: &mut TransferTerminalState,
) -> Result<(), AppError> {
    let Some(task) = load_transfer_task(storage, task_id).await? else {
        tracing::warn!(task_id, "transfer task disappeared before finalization");
        return Ok(());
    };
    if stored_transfer_is_terminal(task.status) {
        return Ok(());
    }
    if control.cancellation.is_cancelled() || task.cancel_requested {
        *terminal = TransferTerminalState::Cancelled(
            "Transfer cancelled before terminal persistence".to_owned(),
        );
    }

    match terminal {
        TransferTerminalState::Succeeded(message) => {
            let storage = storage.clone();
            let message = message.clone();
            let result =
                storage_call(move || storage.complete_transfer_task(task_id, &message)).await;
            if result.is_err() {
                *terminal = TransferTerminalState::Failed(
                    "Transfer completed but its success state could not be persisted".to_owned(),
                );
            }
            result
        }
        TransferTerminalState::Artifact(_) => {
            let TransferTerminalState::Artifact(artifact) = std::mem::replace(
                terminal,
                TransferTerminalState::Failed(
                    "Transfer artifact could not be finalized".to_owned(),
                ),
            ) else {
                unreachable!("artifact terminal state must contain an artifact finalizer");
            };
            let record = artifact.finalize().await?;
            if record.task_id != Some(task_id) {
                return Err(AppError::internal());
            }
            let task = load_transfer_task(storage, task_id)
                .await?
                .ok_or_else(AppError::internal)?;
            if stored_transfer_is_terminal(task.status) {
                Ok(())
            } else {
                Err(AppError::internal())
            }
        }
        TransferTerminalState::Cancelled(message) => {
            persist_cancel_request(storage, task_id).await?;
            let Some(task) = load_transfer_task(storage, task_id).await? else {
                return Ok(());
            };
            if stored_transfer_is_terminal(task.status) {
                return Ok(());
            }
            let storage = storage.clone();
            let message = message.clone();
            storage_call(move || storage.cancel_transfer_task(task_id, &message)).await
        }
        TransferTerminalState::Failed(message) => {
            let storage = storage.clone();
            let message = message.clone();
            storage_call(move || storage.fail_transfer_task(task_id, &message)).await
        }
    }
}

fn validate_import_request(request: &ImportFileRequest) -> Result<(), AppError> {
    validate_transfer_scope(&request.datasource_id, None)?;
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

fn validate_transfer_scope(datasource_id: &str, export_path: Option<&str>) -> Result<(), AppError> {
    if datasource_id.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_transfer_request",
            "datasourceId cannot be empty",
        ));
    }
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
    use std::{future::Future, path::Path, time::Duration};

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use chat2db_contract::{
        DmlExportFormat, DmlExportRequest, DmlExportSize, OtherFileExportRequest,
        TransferFileFormat,
    };
    use chat2db_java_bridge::{EngineCommand, EngineConfig};
    use chat2db_storage::{StoredTransferTaskKind, StoredTransferTaskStatus, TransferTaskRecord};
    use tempfile::TempDir;
    use tokio::sync::oneshot;

    use super::{
        MAX_TRANSFER_FAILURE_MESSAGE_BYTES, QueryResultExportFormat, QueryResultExportRequest,
        QueryResultExportScope, TableFileExportRequest, TaskCompletion, TransferContext,
        TransferJobKind, TransferJobSpec, TransferRunError, TransferTaskControl,
        TransferTerminalState, finalize_transfer_task, transfer_task,
    };
    use crate::{AppError, Application, RuntimeConfig, RuntimeHost};

    #[test]
    fn transfer_job_spec_accepts_a_send_async_runner() {
        fn assert_send<T: Send>(_: &T) {}

        let spec = test_job("send runner", |_application, _context| async {
            Ok(TaskCompletion::WithoutArtifact("done".to_owned()))
        });
        assert_send(&spec);
    }

    #[test]
    fn compatibility_table_file_request_maps_every_field() {
        let request = TableFileExportRequest::from(OtherFileExportRequest {
            datasource_id: "datasource".to_owned(),
            database_name: "database".to_owned(),
            schema_name: "schema".to_owned(),
            table_names: vec!["first".to_owned(), "second".to_owned()],
            format: TransferFileFormat::Xlsx,
            contains_header: false,
            export_path: Some("/tmp/export".to_owned()),
        });

        assert_eq!(request.datasource_id, "datasource");
        assert_eq!(request.database_name, "database");
        assert_eq!(request.schema_name, "schema");
        assert_eq!(request.table_names, ["first", "second"]);
        assert_eq!(request.format, TransferFileFormat::Xlsx);
        assert!(!request.contains_header);
        assert_eq!(request.export_path.as_deref(), Some("/tmp/export"));
    }

    #[test]
    fn compatibility_query_result_request_maps_every_field() {
        let request = QueryResultExportRequest::from(DmlExportRequest {
            datasource_id: "datasource".to_owned(),
            database_name: "database".to_owned(),
            schema_name: "schema".to_owned(),
            sql: "SELECT * FROM table LIMIT 10".to_owned(),
            original_sql: "SELECT * FROM table".to_owned(),
            result_set_id: Some(2),
            export_size: DmlExportSize::CurrentPage,
            format: DmlExportFormat::Insert,
        });

        assert_eq!(request.datasource_id, "datasource");
        assert_eq!(request.database_name, "database");
        assert_eq!(request.schema_name, "schema");
        assert_eq!(request.sql, "SELECT * FROM table LIMIT 10");
        assert_eq!(request.original_sql, "SELECT * FROM table");
        assert_eq!(request.result_set_id, Some(2));
        assert_eq!(request.scope, QueryResultExportScope::CurrentPage);
        assert_eq!(request.format, QueryResultExportFormat::Insert);
    }

    #[test]
    fn compatibility_query_result_enums_map_exhaustively() {
        assert_eq!(
            QueryResultExportScope::from(DmlExportSize::CurrentPage),
            QueryResultExportScope::CurrentPage
        );
        assert_eq!(
            QueryResultExportScope::from(DmlExportSize::All),
            QueryResultExportScope::All
        );
        assert_eq!(
            QueryResultExportFormat::from(DmlExportFormat::Csv),
            QueryResultExportFormat::Csv
        );
        assert_eq!(
            QueryResultExportFormat::from(DmlExportFormat::Xlsx),
            QueryResultExportFormat::Xlsx
        );
        assert_eq!(
            QueryResultExportFormat::from(DmlExportFormat::Insert),
            QueryResultExportFormat::Insert
        );
    }

    #[tokio::test]
    async fn cancellation_wins_over_a_successful_runner_completion() {
        let directory = TempDir::new().expect("temporary transfer runtime");
        let mut host = RuntimeHost::open(test_runtime_config(directory.path()))
            .await
            .expect("transfer runtime must open");
        let application = host.application();

        let (cancel_started, cancel_ready) = oneshot::channel();
        let (release_success, wait_for_success) = oneshot::channel();
        let cancelled = application
            .start_transfer_job(test_job(
                "explicit cancel",
                move |_application, _context| async move {
                    let _ = cancel_started.send(());
                    let _ = wait_for_success.await;
                    Ok(TaskCompletion::WithoutArtifact("done".to_owned()))
                },
            ))
            .await
            .expect("generic transfer must start");
        wait_until_started(cancel_ready).await;
        application
            .stop_transfer_task(cancelled.task_id)
            .await
            .expect("generic transfer must accept cancellation");
        release_success
            .send(())
            .expect("successful runner must be released");
        wait_for_status(
            &application,
            cancelled.task_id,
            chat2db_contract::TransferTaskStatus::Cancelled,
        )
        .await;
        wait_for_hub_removal(&application, cancelled.task_id).await;

        host.shutdown()
            .await
            .expect("transfer runtime must shut down cleanly");
    }

    #[tokio::test]
    async fn panicking_runner_is_failed_and_removed_from_the_hub() {
        let directory = TempDir::new().expect("temporary transfer runtime");
        let mut host = RuntimeHost::open(test_runtime_config(directory.path()))
            .await
            .expect("transfer runtime must open");
        let application = host.application();

        let failed = application
            .start_transfer_job(test_job("panic", panicking_runner))
            .await
            .expect("panicking transfer must be admitted");
        wait_for_status(
            &application,
            failed.task_id,
            chat2db_contract::TransferTaskStatus::Failed,
        )
        .await;
        wait_for_hub_removal(&application, failed.task_id).await;
        let task = application
            .transfer_task(failed.task_id)
            .await
            .expect("failed transfer must remain readable");
        assert!(task.error_log.contains("Transfer worker panicked"));

        host.shutdown()
            .await
            .expect("transfer runtime must shut down cleanly");
    }

    #[tokio::test]
    async fn success_persistence_error_falls_back_to_a_failed_terminal_state() {
        let directory = TempDir::new().expect("temporary transfer runtime");
        let mut host = RuntimeHost::open(test_runtime_config(directory.path()))
            .await
            .expect("transfer runtime must open");
        let application = host.application();

        let failed = application
            .start_transfer_job(test_job(
                "invalid completion",
                |_application, _context| async {
                    Ok(TaskCompletion::WithoutArtifact("x".repeat(300 * 1024)))
                },
            ))
            .await
            .expect("transfer with invalid completion text must be admitted");
        wait_for_status(
            &application,
            failed.task_id,
            chat2db_contract::TransferTaskStatus::Failed,
        )
        .await;
        wait_for_hub_removal(&application, failed.task_id).await;
        let task = application
            .transfer_task(failed.task_id)
            .await
            .expect("fallback failure must remain readable");
        assert!(
            task.error_log
                .contains("success state could not be persisted")
        );

        host.shutdown()
            .await
            .expect("transfer runtime must shut down cleanly");
    }

    #[tokio::test]
    async fn oversized_runner_failure_is_bounded_and_removed_from_the_hub() {
        let directory = TempDir::new().expect("temporary transfer runtime");
        let mut host = RuntimeHost::open(test_runtime_config(directory.path()))
            .await
            .expect("transfer runtime must open");
        let application = host.application();

        let failed = application
            .start_transfer_job(test_job(
                "oversized failure",
                |_application, _context| async {
                    Err(TransferRunError::Failed(AppError::invalid(
                        "transfer_test_failed",
                        "x".repeat(300 * 1024),
                    )))
                },
            ))
            .await
            .expect("failing transfer must be admitted");
        wait_for_status(
            &application,
            failed.task_id,
            chat2db_contract::TransferTaskStatus::Failed,
        )
        .await;
        wait_for_hub_removal(&application, failed.task_id).await;
        let task = application
            .transfer_task(failed.task_id)
            .await
            .expect("failed transfer must remain readable");
        assert!(task.error_log.len() <= MAX_TRANSFER_FAILURE_MESSAGE_BYTES);
        assert!(task.error_log.ends_with("[truncated]"));

        host.shutdown()
            .await
            .expect("transfer runtime must shut down cleanly");
    }

    #[tokio::test]
    async fn terminal_deadline_covers_waiting_for_the_terminal_gate() {
        let directory = TempDir::new().expect("temporary transfer runtime");
        let mut host = RuntimeHost::open(test_runtime_config(directory.path()))
            .await
            .expect("transfer runtime must open");
        let application = host.application();
        let storage = application
            .storage()
            .cloned()
            .expect("transfer runtime must have storage");
        let control = TransferTaskControl::new();
        let terminal_guard = control.terminal_gate.lock().await;
        let deadline = tokio::time::Instant::now() + Duration::from_millis(25);

        let finalized = tokio::time::timeout(
            Duration::from_secs(1),
            finalize_transfer_task(
                &storage,
                -1,
                &control,
                TransferTerminalState::Failed("shutdown".to_owned()),
                Some(deadline),
            ),
        )
        .await
        .expect("terminal finalization must respect its deadline");
        assert!(!finalized);
        drop(terminal_guard);

        host.shutdown()
            .await
            .expect("transfer runtime must shut down cleanly");
    }

    #[tokio::test]
    async fn generic_transfer_runner_preserves_shutdown_semantics() {
        let directory = TempDir::new().expect("temporary transfer runtime");
        let mut host = RuntimeHost::open(test_runtime_config(directory.path()))
            .await
            .expect("transfer runtime must open");
        let application = host.application();

        let (shutdown_started, shutdown_ready) = oneshot::channel();
        let interrupted = application
            .start_transfer_job(waiting_job("shutdown cancel", shutdown_started))
            .await
            .expect("generic transfer must start before shutdown");
        wait_until_started(shutdown_ready).await;
        host.shutdown()
            .await
            .expect("transfer runtime must shut down cleanly");
        let task = application
            .transfer_task(interrupted.task_id)
            .await
            .expect("shutdown transfer task must remain readable");
        assert_eq!(task.status, chat2db_contract::TransferTaskStatus::Cancelled);
        assert!(
            !application
                .inner
                .transfer_tasks
                .tasks
                .lock()
                .await
                .contains_key(&interrupted.task_id)
        );
    }

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

    fn test_runtime_config(directory: &Path) -> RuntimeConfig {
        RuntimeConfig::new(EngineConfig::new(EngineCommand::new(
            directory.join("missing-java"),
        )))
        .with_data_dir(directory.join("data"))
        .with_vault_master_key_base64(STANDARD.encode([0x74; 32]))
    }

    fn test_job<Runner, RunnerFuture>(task_name: &str, runner: Runner) -> TransferJobSpec
    where
        Runner: FnOnce(Application, TransferContext) -> RunnerFuture + Send + 'static,
        RunnerFuture: Future<Output = Result<TaskCompletion, TransferRunError>> + Send + 'static,
    {
        TransferJobSpec::new(
            "test-driver".to_owned(),
            "test-database".to_owned(),
            String::new(),
            None,
            TransferJobKind::ImportFile,
            task_name.to_owned(),
            runner,
        )
    }

    fn waiting_job(task_name: &str, started: oneshot::Sender<()>) -> TransferJobSpec {
        test_job(task_name, move |_application, context| async move {
            let _ = started.send(());
            context.cancellation().cancelled().await;
            Err(TransferRunError::Cancelled)
        })
    }

    async fn panicking_runner(
        _application: Application,
        _context: TransferContext,
    ) -> Result<TaskCompletion, TransferRunError> {
        panic!("intentional transfer runner panic");
    }

    async fn wait_until_started(started: oneshot::Receiver<()>) {
        tokio::time::timeout(Duration::from_secs(2), started)
            .await
            .expect("generic transfer must start before timeout")
            .expect("generic transfer runner must signal startup");
    }

    async fn wait_for_status(
        application: &Application,
        task_id: i64,
        expected: chat2db_contract::TransferTaskStatus,
    ) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let task = application
                    .transfer_task(task_id)
                    .await
                    .expect("generic transfer task must remain readable");
                if task.status == expected {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("generic transfer status must settle before timeout");
    }

    async fn wait_for_hub_removal(application: &Application, task_id: i64) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if !application
                    .inner
                    .transfer_tasks
                    .tasks
                    .lock()
                    .await
                    .contains_key(&task_id)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("terminal transfer must leave the active hub before timeout");
    }
}
