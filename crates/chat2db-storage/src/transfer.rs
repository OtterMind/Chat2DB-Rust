use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{Storage, StorageError, now_millis, secure_file, sync_directory};

const MAX_RETAINED_TASKS: usize = 20;
const MAX_TASK_NAME_BYTES: usize = 1_024;
const MAX_SCOPE_BYTES: usize = 1_024;
const MAX_LOG_BYTES: usize = 256 * 1024;
const MAX_LOG_BYTES_I64: i64 = 256 * 1024;
const MAX_FILE_NAME_BYTES: usize = 1_024;
const MAX_MEDIA_TYPE_BYTES: usize = 255;

/// Durable transfer category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredTransferTaskKind {
    ImportFile,
    ExportSql,
    ExportFile,
}

impl StoredTransferTaskKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ImportFile => "import_file",
            Self::ExportSql => "export_sql",
            Self::ExportFile => "export_file",
        }
    }

    fn parse(value: &str) -> Result<Self, StorageError> {
        match value {
            "import_file" => Ok(Self::ImportFile),
            "export_sql" => Ok(Self::ExportSql),
            "export_file" => Ok(Self::ExportFile),
            _ => Err(StorageError::Integrity(
                "transfer task has an invalid kind".to_owned(),
            )),
        }
    }
}

/// Durable transfer lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredTransferTaskStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

impl StoredTransferTaskStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    fn parse(value: &str) -> Result<Self, StorageError> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(StorageError::Integrity(
                "transfer task has an invalid status".to_owned(),
            )),
        }
    }

    const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }
}

/// Input for one durable task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateTransferTask {
    pub datasource_id: String,
    pub database_name: String,
    pub schema_name: String,
    pub table_name: Option<String>,
    pub kind: StoredTransferTaskKind,
    pub task_name: String,
}

/// Durable transfer task record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferTaskRecord {
    pub id: i64,
    pub datasource_id: String,
    pub database_name: String,
    pub schema_name: String,
    pub table_name: Option<String>,
    pub kind: StoredTransferTaskKind,
    pub status: StoredTransferTaskStatus,
    pub task_name: String,
    pub progress_current: u64,
    pub progress_total: Option<u64>,
    pub progress_description: String,
    pub info_log: String,
    pub error_log: String,
    pub cancel_requested: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub finished_at_ms: Option<i64>,
    pub artifact_id: Option<String>,
}

/// Durable artifact metadata without a filesystem path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferArtifactRecord {
    pub id: String,
    pub task_id: Option<i64>,
    pub file_name: String,
    pub media_type: String,
    pub format: String,
    pub byte_count: u64,
    pub sha256: [u8; 32],
    pub created_at_ms: i64,
    pub expires_at_ms: Option<i64>,
}

/// Owner-only path resolved from a durable artifact record.
#[derive(Debug)]
pub struct ResolvedTransferArtifact {
    pub record: TransferArtifactRecord,
    pub path: PathBuf,
    pub file: File,
}

/// Startup cleanup and task recovery report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransferRecoveryReport {
    pub interrupted_tasks: usize,
    pub expired_artifacts: usize,
    pub partial_files_removed: usize,
    pub orphan_files_removed: usize,
}

/// Poison-on-failure writer for one managed transfer artifact.
pub struct TransferArtifactWriter {
    storage: Storage,
    file: Option<File>,
    id: String,
    task_id: Option<i64>,
    part_path: PathBuf,
    final_path: PathBuf,
    storage_name: String,
    file_name: String,
    media_type: String,
    format: String,
    expires_at_ms: Option<i64>,
    cleanup_on_drop: bool,
}

impl std::fmt::Debug for TransferArtifactWriter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TransferArtifactWriter")
            .field("id", &self.id)
            .field("task_id", &self.task_id)
            .finish_non_exhaustive()
    }
}

impl Write for TransferArtifactWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.file_mut().write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file_mut().flush()
    }
}

impl Drop for TransferArtifactWriter {
    fn drop(&mut self) {
        self.file.take();
        if self.cleanup_on_drop {
            let _ = fs::remove_file(&self.part_path);
            let _ = sync_directory(&self.storage.inner.artifacts_dir);
        }
    }
}

impl TransferArtifactWriter {
    /// Returns the private partial path for format writers requiring seekable files.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.part_path
    }

    /// Returns the seekable partial artifact file.
    ///
    /// # Panics
    ///
    /// Panics only when called after the writer has already been consumed by [`Self::finish`].
    pub fn file_mut(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("artifact file remains open until finish")
    }

    /// Atomically publishes the file and its metadata. For task artifacts this
    /// also transitions the task to `succeeded` in the same `SQLite` transaction.
    ///
    /// # Errors
    ///
    /// Returns filesystem, integrity, task-state, clock, or `SQLite` failures.
    pub fn finish(mut self) -> Result<TransferArtifactRecord, StorageError> {
        let mut file = self.file.take().ok_or(StorageError::InvalidTransfer(
            "artifact writer is already closed",
        ))?;
        file.flush()
            .map_err(|error| StorageError::io(&self.part_path, error))?;
        file.sync_all()
            .map_err(|error| StorageError::io(&self.part_path, error))?;
        let byte_count = file
            .metadata()
            .map_err(|error| StorageError::io(&self.part_path, error))?
            .len();
        file.seek(SeekFrom::Start(0))
            .map_err(|error| StorageError::io(&self.part_path, error))?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| StorageError::io(&self.part_path, error))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let sha256: [u8; 32] = hasher.finalize().into();
        drop(file);

        fs::rename(&self.part_path, &self.final_path)
            .map_err(|error| StorageError::io(&self.final_path, error))?;
        sync_directory(&self.storage.inner.artifacts_dir)?;

        let created_at_ms = now_millis()?;
        let result = (|| {
            let mut connection = self.storage.connection()?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute(
                "INSERT INTO transfer_artifacts (
                    id, task_id, storage_name, file_name, media_type, format,
                    byte_count, sha256, created_at_ms, expires_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    self.id,
                    self.task_id,
                    self.storage_name,
                    self.file_name,
                    self.media_type,
                    self.format,
                    i64::try_from(byte_count)
                        .map_err(|_| StorageError::NumericRange("artifact byte count"))?,
                    sha256.as_slice(),
                    created_at_ms,
                    self.expires_at_ms,
                ],
            )?;
            if let Some(task_id) = self.task_id {
                let updated = transaction.execute(
                    "UPDATE transfer_tasks
                     SET status = 'succeeded', progress_description = 'Completed',
                         cancel_requested = 0, updated_at_ms = ?2, finished_at_ms = ?2
                     WHERE id = ?1 AND status IN ('queued', 'running') AND cancel_requested = 0",
                    params![task_id, created_at_ms],
                )?;
                if updated != 1 {
                    return Err(StorageError::InvalidTransfer(
                        "task cannot publish an artifact in its current state",
                    ));
                }
            }
            transaction.commit()?;
            Ok(TransferArtifactRecord {
                id: self.id.clone(),
                task_id: self.task_id,
                file_name: self.file_name.clone(),
                media_type: self.media_type.clone(),
                format: self.format.clone(),
                byte_count,
                sha256,
                created_at_ms,
                expires_at_ms: self.expires_at_ms,
            })
        })();

        if result.is_err() {
            let _ = fs::remove_file(&self.final_path);
            let _ = sync_directory(&self.storage.inner.artifacts_dir);
        } else {
            self.cleanup_on_drop = false;
            if self.task_id.is_some() {
                let _ = self.storage.prune_transfer_tasks();
            }
        }
        result
    }
}

impl Storage {
    /// Creates one queued task and performs recoverable best-effort retention cleanup.
    ///
    /// # Errors
    ///
    /// Returns validation, clock, or pre-commit `SQLite` failures.
    pub fn create_transfer_task(
        &self,
        input: &CreateTransferTask,
    ) -> Result<TransferTaskRecord, StorageError> {
        validate_create_task(input)?;
        let timestamp = now_millis()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO transfer_tasks (
                datasource_id, database_name, schema_name, table_name, kind, status,
                task_name, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?7, ?7)",
            params![
                input.datasource_id,
                input.database_name,
                input.schema_name,
                input.table_name,
                input.kind.as_str(),
                input.task_name,
                timestamp,
            ],
        )?;
        let id = transaction.last_insert_rowid();
        transaction.commit()?;
        let record = TransferTaskRecord {
            id,
            datasource_id: input.datasource_id.clone(),
            database_name: input.database_name.clone(),
            schema_name: input.schema_name.clone(),
            table_name: input.table_name.clone(),
            kind: input.kind,
            status: StoredTransferTaskStatus::Queued,
            task_name: input.task_name.clone(),
            progress_current: 0,
            progress_total: None,
            progress_description: String::new(),
            info_log: String::new(),
            error_log: String::new(),
            cancel_requested: false,
            created_at_ms: timestamp,
            updated_at_ms: timestamp,
            finished_at_ms: None,
            artifact_id: None,
        };

        // The queued task is durable after commit. Retention cleanup is recoverable
        // maintenance and must not make Core believe that no worker should start.
        let _ = self.prune_transfer_tasks();
        Ok(record)
    }

    /// Lists at most the 20 retained tasks, newest first.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or durable-record decoding failures.
    pub fn list_transfer_tasks(&self) -> Result<Vec<TransferTaskRecord>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT t.id, t.datasource_id, t.database_name, t.schema_name, t.table_name,
                    t.kind, t.status, t.task_name, t.progress_current, t.progress_total,
                    t.progress_description, t.info_log, t.error_log, t.cancel_requested,
                    t.created_at_ms, t.updated_at_ms, t.finished_at_ms, a.id
             FROM transfer_tasks t
             LEFT JOIN transfer_artifacts a ON a.task_id = t.id
             ORDER BY t.created_at_ms DESC, t.id DESC
             LIMIT 20",
        )?;
        let rows = statement.query_map([], raw_task)?;
        rows.map(|row| row.map_err(StorageError::from)).collect()
    }

    /// Gets one task and its optional artifact id.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or durable-record decoding failures.
    pub fn get_transfer_task(&self, id: i64) -> Result<Option<TransferTaskRecord>, StorageError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT t.id, t.datasource_id, t.database_name, t.schema_name, t.table_name,
                        t.kind, t.status, t.task_name, t.progress_current, t.progress_total,
                        t.progress_description, t.info_log, t.error_log, t.cancel_requested,
                        t.created_at_ms, t.updated_at_ms, t.finished_at_ms, a.id
                 FROM transfer_tasks t
                 LEFT JOIN transfer_artifacts a ON a.task_id = t.id
                 WHERE t.id = ?1",
                [id],
                raw_task,
            )
            .optional()
            .map_err(StorageError::from)
    }

    /// Transitions a queued task to running.
    ///
    /// # Errors
    ///
    /// Returns not-found, invalid-state, clock, or `SQLite` failures.
    pub fn start_transfer_task(&self, id: i64) -> Result<(), StorageError> {
        let timestamp = now_millis()?;
        let connection = self.connection()?;
        let updated = connection.execute(
            "UPDATE transfer_tasks
             SET status = 'running', progress_description = 'Running', updated_at_ms = ?2
             WHERE id = ?1 AND status = 'queued' AND cancel_requested = 0",
            params![id, timestamp],
        )?;
        if updated == 1 {
            return Ok(());
        }
        ensure_task_exists(&connection, id)?;
        Err(StorageError::InvalidTransfer(
            "task cannot start in its current state",
        ))
    }

    /// Persists bounded, monotonic task progress and appends a bounded log line.
    ///
    /// # Errors
    ///
    /// Returns validation, numeric-range, not-found, invalid-state, clock, or `SQLite` failures.
    pub fn update_transfer_progress(
        &self,
        id: i64,
        current: u64,
        total: Option<u64>,
        description: &str,
        info: Option<&str>,
    ) -> Result<(), StorageError> {
        validate_text(description, MAX_TASK_NAME_BYTES, "progress description")?;
        if let Some(info) = info {
            validate_text(info, MAX_LOG_BYTES, "task info log entry")?;
        }
        let timestamp = now_millis()?;
        let current =
            i64::try_from(current).map_err(|_| StorageError::NumericRange("transfer progress"))?;
        let total = total
            .map(|value| {
                i64::try_from(value)
                    .map_err(|_| StorageError::NumericRange("transfer progress total"))
            })
            .transpose()?;
        let connection = self.connection()?;
        let updated = connection.execute(
            "UPDATE transfer_tasks
             SET progress_current = MAX(progress_current, ?2),
                 progress_total = COALESCE(?3, progress_total),
                 progress_description = ?4,
                 info_log = CASE WHEN ?5 IS NULL THEN info_log
                    ELSE substr(info_log || CASE WHEN info_log = '' THEN '' ELSE char(10) END || ?5, ?6)
                 END,
                 updated_at_ms = ?7
             WHERE id = ?1 AND status = 'running'",
            params![
                id,
                current,
                total,
                description,
                info,
                -MAX_LOG_BYTES_I64,
                timestamp,
            ],
        )?;
        if updated == 1 {
            return Ok(());
        }
        ensure_task_exists(&connection, id)?;
        Err(StorageError::InvalidTransfer(
            "task progress can only update while running",
        ))
    }

    /// Requests cooperative cancellation. Queued tasks become cancelled immediately.
    ///
    /// # Errors
    ///
    /// Returns not-found, clock, or `SQLite` failures.
    pub fn request_transfer_cancel(&self, id: i64) -> Result<bool, StorageError> {
        let timestamp = now_millis()?;
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE transfer_tasks
             SET cancel_requested = 1,
                 status = CASE WHEN status = 'queued' THEN 'cancelled' ELSE status END,
                 progress_description = CASE WHEN status = 'queued' THEN 'Cancelled' ELSE progress_description END,
                 finished_at_ms = CASE WHEN status = 'queued' THEN ?2 ELSE finished_at_ms END,
                 updated_at_ms = ?2
             WHERE id = ?1 AND status IN ('queued', 'running') AND cancel_requested = 0",
            params![id, timestamp],
        )?;
        if changed == 1 {
            return Ok(true);
        }
        ensure_task_exists(&connection, id)?;
        Ok(false)
    }

    /// Reports whether cancellation was durably requested.
    ///
    /// # Errors
    ///
    /// Returns not-found or `SQLite` failures.
    pub fn transfer_cancel_requested(&self, id: i64) -> Result<bool, StorageError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT cancel_requested FROM transfer_tasks WHERE id = ?1",
                [id],
                |row| row.get::<_, bool>(0),
            )
            .optional()?
            .ok_or(StorageError::TransferTaskNotFound(id))
    }

    /// Marks a running task cancelled after cooperative cleanup completes.
    ///
    /// # Errors
    ///
    /// Returns validation, not-found, invalid-state, clock, or pre-commit `SQLite` failures.
    pub fn cancel_transfer_task(&self, id: i64, message: &str) -> Result<(), StorageError> {
        self.finish_transfer_without_artifact(
            id,
            StoredTransferTaskStatus::Cancelled,
            "Cancelled",
            message,
        )
    }

    /// Marks a queued or running task failed.
    ///
    /// # Errors
    ///
    /// Returns validation, not-found, invalid-state, clock, or pre-commit `SQLite` failures.
    pub fn fail_transfer_task(&self, id: i64, message: &str) -> Result<(), StorageError> {
        self.finish_transfer_without_artifact(
            id,
            StoredTransferTaskStatus::Failed,
            "Failed",
            message,
        )
    }

    /// Marks a running task complete when the operation does not produce an artifact.
    ///
    /// # Errors
    ///
    /// Returns validation, not-found, invalid-state, clock, or pre-commit `SQLite` failures.
    pub fn complete_transfer_task(&self, id: i64, message: &str) -> Result<(), StorageError> {
        validate_text(message, MAX_LOG_BYTES, "task completion message")?;
        let timestamp = now_millis()?;
        let connection = self.connection()?;
        let updated = connection.execute(
            "UPDATE transfer_tasks
             SET status = 'succeeded', progress_description = 'Completed',
                 info_log = CASE WHEN ?2 = '' THEN info_log
                    ELSE substr(info_log || CASE WHEN info_log = '' THEN '' ELSE char(10) END || ?2, ?3)
                 END,
                 cancel_requested = 0, updated_at_ms = ?4, finished_at_ms = ?4
             WHERE id = ?1 AND status = 'running' AND cancel_requested = 0",
            params![
                id,
                message,
                -MAX_LOG_BYTES_I64,
                timestamp,
            ],
        )?;
        if updated != 1 {
            ensure_task_exists(&connection, id)?;
            return Err(StorageError::InvalidTransfer(
                "task cannot complete in its current state",
            ));
        }
        // The terminal state is already durable. Retention is recoverable
        // maintenance and must not make callers retry a committed transition.
        let _ = self.prune_transfer_tasks();
        Ok(())
    }

    fn finish_transfer_without_artifact(
        &self,
        id: i64,
        status: StoredTransferTaskStatus,
        description: &str,
        message: &str,
    ) -> Result<(), StorageError> {
        if !status.is_terminal() || status == StoredTransferTaskStatus::Succeeded {
            return Err(StorageError::InvalidTransfer("invalid terminal task state"));
        }
        validate_text(message, MAX_LOG_BYTES, "task terminal message")?;
        let timestamp = now_millis()?;
        let connection = self.connection()?;
        let updated = connection.execute(
            "UPDATE transfer_tasks
             SET status = ?2, progress_description = ?3, error_log = ?4,
                 updated_at_ms = ?5, finished_at_ms = ?5
             WHERE id = ?1 AND status IN ('queued', 'running')",
            params![id, status.as_str(), description, message, timestamp],
        )?;
        if updated != 1 {
            ensure_task_exists(&connection, id)?;
            return Err(StorageError::InvalidTransfer(
                "task is already in a terminal state",
            ));
        }
        // The terminal state is already durable. Retention is recoverable
        // maintenance and must not make callers retry a committed transition.
        let _ = self.prune_transfer_tasks();
        Ok(())
    }

    /// Begins a private managed artifact. Dropping the writer removes its `.part` file.
    ///
    /// # Errors
    ///
    /// Returns validation, not-found, invalid-state, filesystem, or `SQLite` failures.
    pub fn begin_transfer_artifact(
        &self,
        task_id: Option<i64>,
        file_name: &str,
        media_type: &str,
        format: &str,
        extension: &str,
        expires_at_ms: Option<i64>,
    ) -> Result<TransferArtifactWriter, StorageError> {
        validate_artifact_fields(file_name, media_type, format, extension)?;
        if let Some(task_id) = task_id {
            let task = self
                .get_transfer_task(task_id)?
                .ok_or(StorageError::TransferTaskNotFound(task_id))?;
            if !matches!(
                task.status,
                StoredTransferTaskStatus::Queued | StoredTransferTaskStatus::Running
            ) || task.cancel_requested
            {
                return Err(StorageError::InvalidTransfer(
                    "task cannot create an artifact in its current state",
                ));
            }
        }
        let id = Uuid::new_v4().to_string();
        let storage_name = format!("{id}.{extension}");
        let part_path = self.inner.artifacts_dir.join(format!("{id}.part"));
        let final_path = self.inner.artifacts_dir.join(&storage_name);
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&part_path)
            .map_err(|error| StorageError::io(&part_path, error))?;
        secure_file(&part_path)?;
        Ok(TransferArtifactWriter {
            storage: self.clone(),
            file: Some(file),
            id,
            task_id,
            part_path,
            final_path,
            storage_name,
            file_name: file_name.to_owned(),
            media_type: media_type.to_owned(),
            format: format.to_owned(),
            expires_at_ms,
            cleanup_on_drop: true,
        })
    }

    /// Resolves an artifact to an owner-only regular file under the managed directory.
    ///
    /// # Errors
    ///
    /// Returns not-found, expiry, integrity, filesystem, clock, or `SQLite` failures.
    pub fn resolve_transfer_artifact(
        &self,
        id: &str,
    ) -> Result<ResolvedTransferArtifact, StorageError> {
        let connection = self.connection()?;
        let stored = connection
            .query_row(
                "SELECT id, task_id, storage_name, file_name, media_type, format,
                        byte_count, sha256, created_at_ms, expires_at_ms
                 FROM transfer_artifacts WHERE id = ?1",
                [id],
                raw_artifact_with_storage_name,
            )
            .optional()?
            .ok_or_else(|| StorageError::TransferArtifactNotFound(id.to_owned()))?;
        if stored
            .record
            .expires_at_ms
            .is_some_and(|expiry| expiry <= now_millis().unwrap_or(i64::MAX))
        {
            return Err(StorageError::TransferArtifactNotFound(id.to_owned()));
        }
        validate_storage_name(&stored.storage_name)?;
        let path = self.inner.artifacts_dir.join(stored.storage_name);
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| StorageError::io(&path, error))?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(StorageError::Integrity(
                "transfer artifact is not a regular managed file".to_owned(),
            ));
        }
        let canonical_directory = fs::canonicalize(&self.inner.artifacts_dir)
            .map_err(|error| StorageError::io(&self.inner.artifacts_dir, error))?;
        let canonical_path =
            fs::canonicalize(&path).map_err(|error| StorageError::io(&path, error))?;
        if !canonical_path.starts_with(canonical_directory) {
            return Err(StorageError::Integrity(
                "transfer artifact escaped the managed directory".to_owned(),
            ));
        }
        let file = open_verified_transfer_artifact(&canonical_path, &stored.record)?;
        Ok(ResolvedTransferArtifact {
            record: stored.record,
            path: canonical_path,
            file,
        })
    }

    /// Deletes an unmanaged temporary artifact and its durable metadata.
    ///
    /// Task-owned output artifacts are deliberately excluded so a delivery
    /// adapter cannot remove a retained download by mistake.
    ///
    /// # Errors
    ///
    /// Returns integrity, filesystem, or `SQLite` failures.
    pub fn delete_temporary_transfer_artifact(&self, id: &str) -> Result<bool, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let storage_name = transaction
            .query_row(
                "SELECT storage_name FROM transfer_artifacts
                 WHERE id = ?1 AND task_id IS NULL",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(storage_name) = storage_name else {
            transaction.commit()?;
            return Ok(false);
        };
        validate_storage_name(&storage_name)?;
        let path = self.inner.artifacts_dir.join(storage_name);
        match fs::remove_file(&path) {
            Ok(()) => sync_directory(&self.inner.artifacts_dir)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(StorageError::io(path, error)),
        }
        transaction.execute(
            "DELETE FROM transfer_artifacts WHERE id = ?1 AND task_id IS NULL",
            [id],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    pub(crate) fn recover_transfers_at(
        &self,
        timestamp_ms: i64,
    ) -> Result<TransferRecoveryReport, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let interrupted_tasks = transaction.execute(
            "UPDATE transfer_tasks
             SET status = 'interrupted', progress_description = 'Interrupted',
                 error_log = CASE WHEN error_log = '' THEN 'Application stopped before the task completed'
                                  ELSE error_log END,
                 updated_at_ms = ?1, finished_at_ms = ?1
             WHERE status IN ('queued', 'running')",
            [timestamp_ms],
        )?;
        let expired_names = {
            let mut statement = transaction.prepare(
                "SELECT storage_name FROM transfer_artifacts
                 WHERE task_id IS NULL AND expires_at_ms IS NOT NULL AND expires_at_ms <= ?1",
            )?;
            let rows = statement.query_map([timestamp_ms], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let expired_artifacts = transaction.execute(
            "DELETE FROM transfer_artifacts
             WHERE task_id IS NULL AND expires_at_ms IS NOT NULL AND expires_at_ms <= ?1",
            [timestamp_ms],
        )?;
        let known_names = {
            let mut statement =
                transaction.prepare("SELECT storage_name FROM transfer_artifacts")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };
        transaction.commit()?;

        for name in expired_names {
            if validate_storage_name(&name).is_ok() {
                let _ = fs::remove_file(self.inner.artifacts_dir.join(name));
            }
        }
        let mut partial_files_removed = 0;
        let mut orphan_files_removed = 0;
        for entry in fs::read_dir(&self.inner.artifacts_dir)
            .map_err(|error| StorageError::io(&self.inner.artifacts_dir, error))?
        {
            let entry =
                entry.map_err(|error| StorageError::io(&self.inner.artifacts_dir, error))?;
            let file_type = entry
                .file_type()
                .map_err(|error| StorageError::io(entry.path(), error))?;
            if !file_type.is_file() || file_type.is_symlink() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let remove = if Path::new(&name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("part"))
            {
                partial_files_removed += 1;
                true
            } else if !known_names.contains(&name) {
                orphan_files_removed += 1;
                true
            } else {
                false
            };
            if remove {
                fs::remove_file(entry.path())
                    .map_err(|error| StorageError::io(entry.path(), error))?;
            }
        }
        sync_directory(&self.inner.artifacts_dir)?;
        self.prune_transfer_tasks()?;
        Ok(TransferRecoveryReport {
            interrupted_tasks,
            expired_artifacts,
            partial_files_removed,
            orphan_files_removed,
        })
    }

    fn prune_transfer_tasks(&self) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let candidates = {
            let mut statement = transaction.prepare(
                "SELECT id, status FROM transfer_tasks
                 ORDER BY created_at_ms DESC, id DESC LIMIT -1 OFFSET ?1",
            )?;
            let rows = statement.query_map(
                [i64::try_from(MAX_RETAINED_TASKS).expect("task bound fits i64")],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter_map(|(id, status)| {
                    StoredTransferTaskStatus::parse(&status)
                        .ok()
                        .filter(|status| status.is_terminal())
                        .map(|_| id)
                })
                .collect::<Vec<_>>()
        };
        if candidates.is_empty() {
            transaction.commit()?;
            return Ok(());
        }
        let mut storage_names = Vec::new();
        for id in &candidates {
            if let Some(name) = transaction
                .query_row(
                    "SELECT storage_name FROM transfer_artifacts WHERE task_id = ?1",
                    [id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
            {
                storage_names.push(name);
            }
            transaction.execute("DELETE FROM transfer_tasks WHERE id = ?1", [id])?;
        }
        transaction.commit()?;
        for name in storage_names {
            validate_storage_name(&name)?;
            let path = self.inner.artifacts_dir.join(name);
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(StorageError::io(path, error)),
            }
        }
        sync_directory(&self.inner.artifacts_dir)
    }
}

struct StoredArtifactRow {
    record: TransferArtifactRecord,
    storage_name: String,
}

fn open_verified_transfer_artifact(
    path: &Path,
    record: &TransferArtifactRecord,
) -> Result<File, StorageError> {
    let mut file = File::open(path).map_err(|error| StorageError::io(path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| StorageError::io(path, error))?;
    if metadata.len() != record.byte_count {
        return Err(StorageError::Integrity(
            "transfer artifact size does not match durable metadata".to_owned(),
        ));
    }

    let mut byte_count = 0_u64;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| StorageError::io(path, error))?;
        if read == 0 {
            break;
        }
        byte_count = byte_count
            .checked_add(u64::try_from(read).expect("read buffer length fits u64"))
            .ok_or(StorageError::NumericRange("artifact byte count"))?;
        hasher.update(&buffer[..read]);
    }
    if byte_count != record.byte_count {
        return Err(StorageError::Integrity(
            "transfer artifact size changed while it was verified".to_owned(),
        ));
    }
    let sha256: [u8; 32] = hasher.finalize().into();
    if sha256 != record.sha256 {
        return Err(StorageError::Integrity(
            "transfer artifact SHA-256 does not match durable metadata".to_owned(),
        ));
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| StorageError::io(path, error))?;
    Ok(file)
}

fn raw_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<TransferTaskRecord> {
    let kind: String = row.get(5)?;
    let status: String = row.get(6)?;
    let progress_current: i64 = row.get(8)?;
    let progress_total: Option<i64> = row.get(9)?;
    Ok(TransferTaskRecord {
        id: row.get(0)?,
        datasource_id: row.get(1)?,
        database_name: row.get(2)?,
        schema_name: row.get(3)?,
        table_name: row.get(4)?,
        kind: StoredTransferTaskKind::parse(&kind).map_err(storage_decode_error)?,
        status: StoredTransferTaskStatus::parse(&status).map_err(storage_decode_error)?,
        task_name: row.get(7)?,
        progress_current: u64::try_from(progress_current)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(8, progress_current))?,
        progress_total: progress_total
            .map(|value| {
                u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(9, value))
            })
            .transpose()?,
        progress_description: row.get(10)?,
        info_log: row.get(11)?,
        error_log: row.get(12)?,
        cancel_requested: row.get(13)?,
        created_at_ms: row.get(14)?,
        updated_at_ms: row.get(15)?,
        finished_at_ms: row.get(16)?,
        artifact_id: row.get(17)?,
    })
}

fn raw_artifact_with_storage_name(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredArtifactRow> {
    let digest: Vec<u8> = row.get(7)?;
    let sha256: [u8; 32] = digest.try_into().map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            32,
            rusqlite::types::Type::Blob,
            "artifact digest is not SHA-256".into(),
        )
    })?;
    let byte_count: i64 = row.get(6)?;
    Ok(StoredArtifactRow {
        record: TransferArtifactRecord {
            id: row.get(0)?,
            task_id: row.get(1)?,
            file_name: row.get(3)?,
            media_type: row.get(4)?,
            format: row.get(5)?,
            byte_count: u64::try_from(byte_count)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(6, byte_count))?,
            sha256,
            created_at_ms: row.get(8)?,
            expires_at_ms: row.get(9)?,
        },
        storage_name: row.get(2)?,
    })
}

fn validate_create_task(input: &CreateTransferTask) -> Result<(), StorageError> {
    validate_nonempty_text(&input.datasource_id, MAX_SCOPE_BYTES, "datasource id")?;
    validate_nonempty_text(&input.database_name, MAX_SCOPE_BYTES, "database name")?;
    validate_text(&input.schema_name, MAX_SCOPE_BYTES, "schema name")?;
    if let Some(table_name) = &input.table_name {
        validate_nonempty_text(table_name, MAX_SCOPE_BYTES, "table name")?;
    }
    validate_nonempty_text(&input.task_name, MAX_TASK_NAME_BYTES, "task name")
}

fn validate_artifact_fields(
    file_name: &str,
    media_type: &str,
    format: &str,
    extension: &str,
) -> Result<(), StorageError> {
    validate_nonempty_text(file_name, MAX_FILE_NAME_BYTES, "artifact file name")?;
    if Path::new(file_name)
        .file_name()
        .and_then(|name| name.to_str())
        != Some(file_name)
    {
        return Err(StorageError::InvalidTransfer(
            "artifact file name must not contain a path",
        ));
    }
    validate_nonempty_text(media_type, MAX_MEDIA_TYPE_BYTES, "artifact media type")?;
    validate_nonempty_text(format, 32, "artifact format")?;
    if extension.is_empty()
        || extension.len() > 16
        || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(StorageError::InvalidTransfer(
            "artifact extension is invalid",
        ));
    }
    Ok(())
}

fn validate_storage_name(value: &str) -> Result<(), StorageError> {
    if value.is_empty()
        || value.len() > 128
        || value.contains(['/', '\\'])
        || value == "."
        || value == ".."
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(StorageError::Integrity(
            "transfer artifact storage name is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn validate_nonempty_text(
    value: &str,
    maximum: usize,
    field: &'static str,
) -> Result<(), StorageError> {
    if value.trim().is_empty() {
        return Err(StorageError::InvalidTransfer(field));
    }
    validate_text(value, maximum, field)
}

fn validate_text(value: &str, maximum: usize, field: &'static str) -> Result<(), StorageError> {
    if value.len() > maximum {
        return Err(StorageError::InvalidTransfer(field));
    }
    Ok(())
}

fn ensure_task_exists(connection: &rusqlite::Connection, id: i64) -> Result<(), StorageError> {
    let exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM transfer_tasks WHERE id = ?1)",
        [id],
        |row| row.get::<_, bool>(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(StorageError::TransferTaskNotFound(id))
    }
}

fn storage_decode_error(error: StorageError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Read as _, Write as _},
        sync::Arc,
    };

    use tempfile::TempDir;

    use crate::{SecretRef, SecretValue, SecretVault, SecretVaultError, now_millis};

    use super::{
        CreateTransferTask, Storage, StorageError, StoredTransferTaskKind, StoredTransferTaskStatus,
    };

    #[derive(Debug)]
    struct EmptyVault;

    impl SecretVault for EmptyVault {
        fn probe(&self) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn create(
            &self,
            _reference: &SecretRef,
            _value: &SecretValue,
        ) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn get(&self, _reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
            Ok(None)
        }

        fn delete(&self, _reference: &SecretRef) -> Result<(), SecretVaultError> {
            Ok(())
        }
    }

    fn open(directory: &TempDir) -> Storage {
        Storage::open(directory.path(), Arc::new(EmptyVault)).expect("storage opens")
    }

    fn task(storage: &Storage, suffix: usize) -> i64 {
        storage
            .create_transfer_task(&CreateTransferTask {
                datasource_id: "mysql-local".to_owned(),
                database_name: "inventory".to_owned(),
                schema_name: String::new(),
                table_name: Some(format!("items_{suffix}")),
                kind: StoredTransferTaskKind::ExportFile,
                task_name: format!("Export items {suffix}"),
            })
            .expect("task creates")
            .id
    }

    #[test]
    fn artifact_publish_is_atomic_and_completes_the_task() {
        let directory = TempDir::new().expect("temp directory");
        let storage = open(&directory);
        let id = task(&storage, 1);
        storage.start_transfer_task(id).expect("task starts");
        let mut writer = storage
            .begin_transfer_artifact(Some(id), "items.csv", "text/csv", "CSV", "csv", None)
            .expect("artifact begins");
        writer
            .write_all(b"id,name\n1,alpha\n")
            .expect("artifact writes");
        let artifact = writer.finish().expect("artifact finishes");

        let task = storage
            .get_transfer_task(id)
            .expect("task reads")
            .expect("task exists");
        assert_eq!(task.status, StoredTransferTaskStatus::Succeeded);
        assert_eq!(task.artifact_id.as_deref(), Some(artifact.id.as_str()));
        let resolved = storage
            .resolve_transfer_artifact(&artifact.id)
            .expect("artifact resolves");
        assert_eq!(
            fs::read(resolved.path).expect("artifact reads"),
            b"id,name\n1,alpha\n"
        );
    }

    #[test]
    fn artifact_display_names_never_create_paths() {
        let directory = TempDir::new().expect("temp directory");
        let storage = open(&directory);

        assert!(matches!(
            storage
                .begin_transfer_artifact(None, "../outside.csv", "text/csv", "CSV", "csv", None,),
            Err(StorageError::InvalidTransfer(
                "artifact file name must not contain a path"
            ))
        ));
        assert_eq!(
            fs::read_dir(directory.path().join("artifacts"))
                .expect("artifact directory reads")
                .count(),
            0
        );
    }

    #[test]
    fn artifact_resolution_rejects_tampered_and_truncated_files() {
        let directory = TempDir::new().expect("temp directory");
        let storage = open(&directory);

        let mut tampered = storage
            .begin_transfer_artifact(None, "tampered.csv", "text/csv", "CSV", "csv", None)
            .expect("artifact begins");
        tampered
            .write_all(b"id,name\n1,alpha\n")
            .expect("artifact writes");
        let tampered = tampered.finish().expect("artifact finishes");
        let tampered_path = storage
            .resolve_transfer_artifact(&tampered.id)
            .expect("untampered artifact resolves")
            .path;
        fs::write(&tampered_path, b"id,name\n1,omega\n").expect("artifact is tampered");
        assert!(matches!(
            storage.resolve_transfer_artifact(&tampered.id),
            Err(StorageError::Integrity(message)) if message.contains("SHA-256")
        ));

        let mut truncated = storage
            .begin_transfer_artifact(None, "truncated.csv", "text/csv", "CSV", "csv", None)
            .expect("artifact begins");
        truncated
            .write_all(b"id,name\n1,alpha\n")
            .expect("artifact writes");
        let truncated = truncated.finish().expect("artifact finishes");
        let truncated_path = storage
            .resolve_transfer_artifact(&truncated.id)
            .expect("complete artifact resolves")
            .path;
        fs::write(&truncated_path, b"id\n").expect("artifact is truncated");
        assert!(matches!(
            storage.resolve_transfer_artifact(&truncated.id),
            Err(StorageError::Integrity(message)) if message.contains("size")
        ));
    }

    #[test]
    fn temporary_artifact_deletion_removes_metadata_and_file() {
        let directory = TempDir::new().expect("temp directory");
        let storage = open(&directory);
        let mut writer = storage
            .begin_transfer_artifact(None, "upload.csv", "text/csv", "CSV", "csv", None)
            .expect("artifact begins");
        writer.write_all(b"id\n1\n").expect("artifact writes");
        let artifact = writer.finish().expect("artifact finishes");
        let path = storage
            .resolve_transfer_artifact(&artifact.id)
            .expect("artifact resolves")
            .path;

        assert!(
            storage
                .delete_temporary_transfer_artifact(&artifact.id)
                .expect("temporary artifact deletes")
        );
        assert!(!path.exists());
        assert!(
            !storage
                .delete_temporary_transfer_artifact(&artifact.id)
                .expect("repeated deletion is idempotent")
        );
        assert!(matches!(
            storage.resolve_transfer_artifact(&artifact.id),
            Err(StorageError::TransferArtifactNotFound(_))
        ));

        let task_id = task(&storage, 90);
        storage.start_transfer_task(task_id).expect("task starts");
        let mut writer = storage
            .begin_transfer_artifact(
                Some(task_id),
                "retained.csv",
                "text/csv",
                "CSV",
                "csv",
                None,
            )
            .expect("task artifact begins");
        writer.write_all(b"id\n2\n").expect("artifact writes");
        let retained = writer.finish().expect("task artifact finishes");
        let retained_path = storage
            .resolve_transfer_artifact(&retained.id)
            .expect("task artifact resolves")
            .path;
        assert!(
            !storage
                .delete_temporary_transfer_artifact(&retained.id)
                .expect("task artifact is not eligible for temporary cleanup")
        );
        assert!(retained_path.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn resolved_artifact_keeps_the_verified_file_when_the_path_is_replaced() {
        let directory = TempDir::new().expect("temp directory");
        let storage = open(&directory);
        let mut writer = storage
            .begin_transfer_artifact(None, "download.csv", "text/csv", "CSV", "csv", None)
            .expect("artifact begins");
        writer
            .write_all(b"verified-content")
            .expect("artifact writes");
        let artifact = writer.finish().expect("artifact finishes");
        let mut resolved = storage
            .resolve_transfer_artifact(&artifact.id)
            .expect("artifact resolves");
        let replaced = resolved.path.with_extension("replaced");
        fs::rename(&resolved.path, &replaced).expect("verified inode is renamed");
        fs::write(&resolved.path, b"different-content").expect("path is replaced");

        let mut content = Vec::new();
        resolved
            .file
            .read_to_end(&mut content)
            .expect("verified descriptor reads");
        assert_eq!(content, b"verified-content");
    }

    fn terminal_transition_with_broken_prune(
        transition: impl FnOnce(&Storage, i64) -> Result<(), StorageError>,
    ) {
        let directory = TempDir::new().expect("temp directory");
        let storage = open(&directory);
        let oldest = task(&storage, 0);
        for suffix in 1..21 {
            task(&storage, suffix);
        }
        let artifacts = directory.path().join("artifacts");
        let displaced = directory.path().join("artifacts-displaced");
        fs::rename(&artifacts, &displaced).expect("artifact directory is displaced");

        let result = transition(&storage, oldest);

        fs::rename(&displaced, &artifacts).expect("artifact directory is restored");
        result.expect("post-commit pruning cannot reverse a terminal transition");
    }

    #[test]
    fn terminal_transitions_ignore_post_commit_prune_failures() {
        terminal_transition_with_broken_prune(|storage, id| {
            storage.start_transfer_task(id)?;
            storage.complete_transfer_task(id, "done")
        });
        terminal_transition_with_broken_prune(|storage, id| {
            storage.fail_transfer_task(id, "expected failure")
        });
        terminal_transition_with_broken_prune(|storage, id| {
            storage.cancel_transfer_task(id, "cancelled")
        });
    }

    #[test]
    fn committed_task_survives_post_commit_prune_cleanup_failure() {
        let directory = TempDir::new().expect("temp directory");
        let storage = open(&directory);
        let oldest = task(&storage, 100);
        storage.start_transfer_task(oldest).expect("task starts");
        let mut writer = storage
            .begin_transfer_artifact(Some(oldest), "old.csv", "text/csv", "CSV", "csv", None)
            .expect("artifact begins");
        writer.write_all(b"id\n1\n").expect("artifact writes");
        let artifact = writer.finish().expect("artifact finishes");
        let artifact_path = storage
            .resolve_transfer_artifact(&artifact.id)
            .expect("artifact resolves")
            .path;
        fs::remove_file(&artifact_path).expect("artifact file removes");
        fs::create_dir(&artifact_path).expect("artifact path becomes an unremovable directory");

        for suffix in 101..120 {
            task(&storage, suffix);
        }
        let accepted = storage
            .create_transfer_task(&CreateTransferTask {
                datasource_id: "mysql-local".to_owned(),
                database_name: "inventory".to_owned(),
                schema_name: String::new(),
                table_name: Some("items_120".to_owned()),
                kind: StoredTransferTaskKind::ExportFile,
                task_name: "Export items 120".to_owned(),
            })
            .expect("committed task remains accepted when cleanup fails");

        assert_eq!(accepted.status, StoredTransferTaskStatus::Queued);
        assert_eq!(
            storage
                .get_transfer_task(accepted.id)
                .expect("task reads")
                .expect("task exists"),
            accepted
        );
        assert!(
            artifact_path.is_dir(),
            "cleanup failure fixture must remain isolated from task acceptance"
        );
    }

    #[test]
    fn dropping_writer_removes_partial_file_and_restart_interrupts_running_tasks() {
        let directory = TempDir::new().expect("temp directory");
        let storage = open(&directory);
        let id = task(&storage, 2);
        storage.start_transfer_task(id).expect("task starts");
        let writer = storage
            .begin_transfer_artifact(Some(id), "items.sql", "application/sql", "SQL", "sql", None)
            .expect("artifact begins");
        let partial = writer.path().to_path_buf();
        drop(writer);
        assert!(!partial.exists());
        drop(storage);

        let reopened = open(&directory);
        assert_eq!(reopened.startup_report().transfers.interrupted_tasks, 1);
        assert_eq!(
            reopened
                .get_transfer_task(id)
                .expect("task reads")
                .expect("task exists")
                .status,
            StoredTransferTaskStatus::Interrupted
        );
    }

    #[test]
    fn failed_artifact_publish_removes_the_renamed_file() {
        let directory = TempDir::new().expect("temp directory");
        let storage = open(&directory);
        let id = task(&storage, 3);
        storage.start_transfer_task(id).expect("task starts");
        let mut writer = storage
            .begin_transfer_artifact(Some(id), "items.csv", "text/csv", "CSV", "csv", None)
            .expect("artifact begins");
        writer.write_all(b"id\n1\n").expect("artifact writes");
        storage
            .request_transfer_cancel(id)
            .expect("running task cancellation records");

        assert!(writer.finish().is_err(), "cancelled task cannot publish");
        assert_eq!(
            fs::read_dir(directory.path().join("artifacts"))
                .expect("artifact directory reads")
                .count(),
            0,
            "failed publication must remove partial and final files"
        );
    }

    #[test]
    fn recovery_removes_expired_partial_and_orphan_artifacts() {
        let directory = TempDir::new().expect("temp directory");
        let storage = open(&directory);
        let timestamp = now_millis().expect("clock reads");
        let mut writer = storage
            .begin_transfer_artifact(
                None,
                "temporary.csv",
                "text/csv",
                "CSV",
                "csv",
                Some(timestamp + 10_000),
            )
            .expect("temporary artifact begins");
        writer.write_all(b"id\n1\n").expect("artifact writes");
        let artifact = writer.finish().expect("temporary artifact finishes");
        let resolved = storage
            .resolve_transfer_artifact(&artifact.id)
            .expect("temporary artifact resolves");
        let artifact_path = resolved.path;
        let artifacts = directory.path().join("artifacts");
        let partial = artifacts.join("stranded.part");
        let orphan = artifacts.join("orphan.bin");
        fs::write(&partial, b"partial").expect("partial fixture writes");
        fs::write(&orphan, b"orphan").expect("orphan fixture writes");

        let report = storage
            .recover_transfers_at(timestamp + 20_000)
            .expect("transfer recovery succeeds");
        assert_eq!(report.expired_artifacts, 1);
        assert_eq!(report.partial_files_removed, 1);
        assert_eq!(report.orphan_files_removed, 1);
        assert!(!artifact_path.exists());
        assert!(!partial.exists());
        assert!(!orphan.exists());
    }

    #[test]
    fn only_twenty_terminal_tasks_are_retained_with_their_artifacts() {
        let directory = TempDir::new().expect("temp directory");
        let storage = open(&directory);
        for suffix in 0..21 {
            let id = task(&storage, suffix);
            storage.start_transfer_task(id).expect("task starts");
            storage
                .fail_transfer_task(id, "expected test failure")
                .expect("task fails");
        }
        let tasks = storage.list_transfer_tasks().expect("tasks list");
        assert_eq!(tasks.len(), 20);
        assert!(tasks.iter().all(|task| task.id != 1));
    }
}
