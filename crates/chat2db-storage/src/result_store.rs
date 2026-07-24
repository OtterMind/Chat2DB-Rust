use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use chat2db_engine_protocol::{MAX_FRAME_BYTES, wire};
use prost::Message;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{Storage, StorageError, now_millis, secure_file, sync_directory};

const RESULT_FORMAT_VERSION: i64 = 1;
const RESULT_FILE_EXTENSION: &str = "c2result";
const FRAME_PREFIX_BYTES: usize = 4;
const MAX_BATCH_ROWS: usize = wire::JdbcProtocolLimit::MaxBatchRows as usize;
const MAX_BATCH_BYTES: usize = wire::JdbcProtocolLimit::MaxBatchBytes as usize;
const MAX_COLUMNS: usize = wire::JdbcProtocolLimit::MaxColumns as usize;

/// Maximum rows returned by one retained-result page.
pub const MAX_RESULT_PAGE_ROWS: u32 = 4096;
/// Maximum decoded row bytes returned by one retained-result page.
pub const MAX_RESULT_PAGE_BYTES: u64 = 16 * 1024 * 1024;
/// Minimum page budget, chosen so every protocol-valid single row can fit.
pub const MIN_RESULT_PAGE_BYTES: u64 = MAX_BATCH_BYTES as u64;

struct PendingResultFile {
    file: Option<File>,
    path: PathBuf,
    directory: PathBuf,
    cleanup_on_drop: bool,
}

impl PendingResultFile {
    fn new(file: File, path: PathBuf, directory: PathBuf) -> Self {
        Self {
            file: Some(file),
            path,
            directory,
            cleanup_on_drop: true,
        }
    }

    fn file_mut(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("pending result file remains open until it is indexed")
    }

    fn preserve(&mut self) {
        self.cleanup_on_drop = false;
    }

    fn into_file(mut self) -> File {
        self.cleanup_on_drop = false;
        self.file
            .take()
            .expect("indexed result keeps its open file handle")
    }
}

impl Drop for PendingResultFile {
    fn drop(&mut self) {
        self.file.take();
        if self.cleanup_on_drop {
            let _ = remove_file_if_exists(&self.path);
            let _ = sync_directory(&self.directory);
        }
    }
}

/// Bounded retained-result page request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest {
    /// Zero-based row offset.
    pub offset: u64,
    /// Maximum number of returned rows.
    pub max_rows: u32,
    /// Maximum cumulative encoded `JdbcRow` bytes.
    pub max_bytes: u64,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            offset: 0,
            max_rows: MAX_RESULT_PAGE_ROWS,
            max_bytes: MAX_RESULT_PAGE_BYTES,
        }
    }
}

/// Durable completion metadata for one retained query result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultMetadata {
    /// Opaque result id.
    pub id: String,
    /// Number of retained rows.
    pub row_count: u64,
    /// Whether the JDBC engine observed another row beyond `max_rows`.
    pub truncated_by_max_rows: bool,
    /// Whether the JDBC engine omitted a row beyond its result-byte budget.
    pub truncated_by_max_result_bytes: bool,
    /// Creation time as Unix epoch milliseconds.
    pub created_at_ms: i64,
    /// Expiry time as Unix epoch milliseconds.
    pub expires_at_ms: i64,
}

/// One bounded page plus the schema required to interpret its rows.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultPage {
    /// Durable completion metadata.
    pub metadata: ResultMetadata,
    /// Column schema emitted before the first row batch.
    pub schema: wire::QueryStarted,
    /// Actual zero-based offset of the first returned row.
    pub offset: u64,
    /// Retained rows, bounded by both request limits.
    pub rows: Vec<wire::JdbcRow>,
    /// Whether another retained row exists after this page.
    pub has_more: bool,
}

/// Expired-result cleanup summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PurgeReport {
    /// Expired result records removed from `SQLite`.
    pub results_removed: usize,
    /// Indexed bytes made reclaimable.
    pub indexed_bytes_removed: u64,
    /// Unindexed result files removed from the results directory.
    pub orphan_files_removed: usize,
}

/// Idempotent retained-result startup recovery summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Incomplete writing results removed.
    pub writing_removed: usize,
    /// Expired completed results removed.
    pub expired_removed: usize,
    /// Completed results evicted after validation failed.
    pub corrupt_removed: usize,
    /// Unreferenced result files removed.
    pub orphan_files_removed: usize,
    /// Completed files whose unindexed tail was truncated.
    pub tails_truncated: usize,
}

/// Poison-on-failure writer for one retained result.
pub struct ResultWriter {
    storage: Storage,
    file: Option<File>,
    id: String,
    schema_columns: usize,
    committed_length: u64,
    next_row: u64,
    next_ordinal: u64,
    created_at_ms: i64,
    expires_at_ms: i64,
    poisoned: bool,
    cleanup_on_drop: bool,
}

impl std::fmt::Debug for ResultWriter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResultWriter")
            .field("id", &self.id)
            .field("committed_length", &self.committed_length)
            .field("next_row", &self.next_row)
            .field("next_ordinal", &self.next_ordinal)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}

impl ResultWriter {
    /// Returns the opaque id immediately, allowing conservative outcome checks.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Persists one contiguous row batch using file-sync-before-index ordering.
    ///
    /// # Errors
    ///
    /// Returns validation, quota, filesystem, `SQLite`, or unknown-outcome
    /// failures. Any failure poisons this writer permanently.
    pub fn append_batch(&mut self, batch: &wire::RowBatch) -> Result<(), StorageError> {
        let result = self.append_batch_inner(batch);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    fn append_batch_inner(&mut self, batch: &wire::RowBatch) -> Result<(), StorageError> {
        if self.poisoned {
            return Err(StorageError::InvalidResult("result writer is poisoned"));
        }
        validate_batch(batch, self.next_row, self.schema_columns)?;
        let frame = encode_frame(batch, MAX_BATCH_BYTES)?;
        let frame_length = u64::try_from(frame.len())
            .map_err(|_| StorageError::NumericRange("result frame length"))?;
        let next_length = self
            .committed_length
            .checked_add(frame_length)
            .ok_or(StorageError::NumericRange("result file length"))?;
        let batch_rows = u64::try_from(batch.rows.len())
            .map_err(|_| StorageError::NumericRange("batch row count"))?;
        let next_row = self
            .next_row
            .checked_add(batch_rows)
            .ok_or(StorageError::NumericRange("result row count"))?;

        let storage = self.storage.clone();
        let _gate = storage.lock_results()?;
        storage.check_result_quota(frame_length)?;

        let path = storage.result_path(&self.id)?;
        let file = self
            .file
            .as_mut()
            .ok_or(StorageError::InvalidResult("result writer file is closed"))?;
        file.seek(SeekFrom::Start(self.committed_length))
            .and_then(|_| file.write_all(&frame))
            .and_then(|()| file.sync_data())
            .map_err(|error| StorageError::io(path, error))?;

        let frame_hash = sha256(&frame);
        let commit = storage.commit_chunk(
            &self.id,
            self.next_ordinal,
            self.committed_length,
            frame_length,
            self.next_row,
            next_row,
            frame_hash.as_slice(),
            next_length,
        );
        match commit {
            Ok(()) => {}
            Err(error) => match storage.verify_chunk_commit(
                &self.id,
                self.next_ordinal,
                self.committed_length,
                frame_length,
                self.next_row,
                next_row,
                frame_hash.as_slice(),
                next_length,
            ) {
                Ok(CommitState::Applied) => {}
                Ok(CommitState::NotApplied) => return Err(error),
                Err(_) | Ok(CommitState::Diverged) => {
                    return Err(StorageError::OutcomeUnknown {
                        operation: "append result chunk",
                        id: self.id.clone(),
                    });
                }
            },
        }

        self.committed_length = next_length;
        self.next_row = next_row;
        self.next_ordinal = self
            .next_ordinal
            .checked_add(1)
            .ok_or(StorageError::NumericRange("result chunk ordinal"))?;
        Ok(())
    }

    /// Atomically marks this result complete after validating final statistics.
    ///
    /// # Errors
    ///
    /// Returns an error when the writer is poisoned, row counts disagree, or
    /// `SQLite` cannot establish a known completion outcome.
    pub fn finish(
        mut self,
        completed: &wire::QueryCompleted,
    ) -> Result<ResultMetadata, StorageError> {
        if self.poisoned {
            return Err(StorageError::InvalidResult("result writer is poisoned"));
        }
        if completed.row_count != self.next_row {
            self.poisoned = true;
            return Err(StorageError::InvalidResult(
                "completion row count does not match indexed chunks",
            ));
        }

        let storage = self.storage.clone();
        let mut result_guard = storage.lock_results()?;
        let commit = storage.commit_completion(&self.id, completed, self.committed_length);
        let metadata = match commit {
            Ok(()) => ResultMetadata {
                id: self.id.clone(),
                row_count: completed.row_count,
                truncated_by_max_rows: completed.truncated_by_max_rows,
                truncated_by_max_result_bytes: completed.truncated_by_max_result_bytes,
                created_at_ms: self.created_at_ms,
                expires_at_ms: self.expires_at_ms,
            },
            Err(error) => {
                #[cfg(test)]
                let readback = if crate::take_fault(crate::FaultPoint::ResultCompletionReadback) {
                    Err(crate::injected_commit_error())
                } else {
                    storage.load_stored_result(&self.id)
                };
                #[cfg(not(test))]
                let readback = storage.load_stored_result(&self.id);

                match readback {
                    Ok(Some(actual))
                        if actual.matches_completion(completed, self.committed_length) =>
                    {
                        actual.metadata()?
                    }
                    Ok(Some(actual))
                        if actual.matches_writing(self.next_row, self.committed_length) =>
                    {
                        drop(result_guard);
                        return Err(error);
                    }
                    Ok(None) => {
                        drop(result_guard);
                        return Err(error);
                    }
                    Err(_) | Ok(Some(_)) => {
                        result_guard.remove(&self.id);
                        self.cleanup_on_drop = false;
                        self.file.take();
                        return Err(StorageError::OutcomeUnknown {
                            operation: "complete retained result",
                            id: self.id.clone(),
                        });
                    }
                }
            }
        };
        result_guard.remove(&self.id);
        self.cleanup_on_drop = false;
        self.file.take();
        self.poisoned = true;
        Ok(metadata)
    }

    /// Abandons an incomplete result and removes its index and file.
    ///
    /// # Errors
    ///
    /// Returns an error when `SQLite` deletion or filesystem cleanup fails.
    pub fn abort(mut self) -> Result<(), StorageError> {
        let storage = self.storage.clone();
        self.file.take();
        let mut result_guard = storage.lock_results()?;
        let deletion = storage.delete_result(&self.id);
        result_guard.remove(&self.id);
        deletion?;
        self.cleanup_on_drop = false;
        self.poisoned = true;
        Ok(())
    }
}

impl Drop for ResultWriter {
    fn drop(&mut self) {
        if !self.cleanup_on_drop {
            return;
        }
        self.file.take();
        let storage = self.storage.clone();
        if let Ok(mut result_guard) = storage.lock_results() {
            result_guard.remove(&self.id);
            let _ = storage.delete_result(&self.id);
        }
    }
}

impl Storage {
    /// Starts a disk-backed result with a caller-selected retention duration.
    ///
    /// # Errors
    ///
    /// Returns validation, quota, time, filesystem, `SQLite`, or unknown-outcome
    /// failures.
    pub fn begin_result(
        &self,
        schema: &wire::QueryStarted,
        retention: Duration,
    ) -> Result<ResultWriter, StorageError> {
        if retention.is_zero() {
            return Err(StorageError::InvalidResult(
                "result retention must be greater than zero",
            ));
        }
        let created_at_ms = now_millis()?;
        let retention_ms = i64::try_from(retention.as_millis())
            .map_err(|_| StorageError::NumericRange("result retention"))?;
        let expires_at_ms = created_at_ms
            .checked_add(retention_ms)
            .ok_or(StorageError::NumericRange("result expiry"))?;
        self.begin_result_at(schema, created_at_ms, expires_at_ms)
    }

    fn begin_result_at(
        &self,
        schema: &wire::QueryStarted,
        created_at_ms: i64,
        expires_at_ms: i64,
    ) -> Result<ResultWriter, StorageError> {
        if expires_at_ms <= created_at_ms {
            return Err(StorageError::InvalidResult(
                "result expiry must be after creation",
            ));
        }
        validate_schema(schema)?;
        let frame = encode_frame(schema, MAX_FRAME_BYTES)?;
        let frame_length = u64::try_from(frame.len())
            .map_err(|_| StorageError::NumericRange("schema frame length"))?;
        let id = Uuid::new_v4().to_string();
        let path = self.result_path(&id)?;
        let storage = self.clone();
        let mut result_guard = storage.lock_results()?;
        storage.check_result_quota(frame_length)?;

        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| StorageError::io(&path, error))?;
        let mut pending_file =
            PendingResultFile::new(file, path.clone(), storage.inner.results_dir.clone());
        secure_file(&path)?;
        pending_file
            .file_mut()
            .write_all(&frame)
            .map_err(|error| StorageError::io(&path, error))?;
        pending_file
            .file_mut()
            .sync_data()
            .map_err(|error| StorageError::io(&path, error))?;
        sync_directory(&storage.inner.results_dir)?;

        let schema_hash = sha256(&frame);
        let insert = storage.insert_writing_result(
            &id,
            frame_length,
            schema_hash.as_slice(),
            created_at_ms,
            expires_at_ms,
        );
        if let Err(error) = insert {
            match storage.load_stored_result(&id) {
                Ok(Some(actual))
                    if actual.state == ResultState::Writing
                        && actual.committed_length == frame_length
                        && actual.schema_sha256 == schema_hash.as_slice() => {}
                Ok(None) => {
                    return Err(error);
                }
                _ => {
                    pending_file.preserve();
                    return Err(StorageError::OutcomeUnknown {
                        operation: "begin retained result",
                        id,
                    });
                }
            }
        }

        result_guard.insert(id.clone());
        let file = pending_file.into_file();
        drop(result_guard);
        Ok(ResultWriter {
            storage,
            file: Some(file),
            id,
            schema_columns: schema.columns.len(),
            committed_length: frame_length,
            next_row: 0,
            next_ordinal: 0,
            created_at_ms,
            expires_at_ms,
            poisoned: false,
            cleanup_on_drop: true,
        })
    }

    /// Returns completion metadata only for a live completed result.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or persisted numeric-validation failures.
    pub fn result_metadata(&self, id: &str) -> Result<Option<ResultMetadata>, StorageError> {
        self.result_metadata_at(id, now_millis()?)
    }

    fn result_metadata_at(
        &self,
        id: &str,
        timestamp_ms: i64,
    ) -> Result<Option<ResultMetadata>, StorageError> {
        let stored = self.load_stored_result(id)?;
        match stored {
            Some(result)
                if result.state == ResultState::Complete && result.expires_at_ms > timestamp_ms =>
            {
                Ok(Some(result.metadata()?))
            }
            _ => Ok(None),
        }
    }

    /// Reads one row- and byte-bounded page from a live completed result.
    ///
    /// # Errors
    ///
    /// Returns not-found, invalid request, filesystem, corruption, `SQLite`, or
    /// numeric-validation failures.
    pub fn read_result_page(
        &self,
        id: &str,
        request: PageRequest,
    ) -> Result<ResultPage, StorageError> {
        self.read_result_page_at(id, request, now_millis()?)
    }

    fn read_result_page_at(
        &self,
        id: &str,
        request: PageRequest,
        timestamp_ms: i64,
    ) -> Result<ResultPage, StorageError> {
        validate_page_request(request)?;
        let storage = self.clone();
        let _gate = storage.lock_results()?;
        let stored = storage
            .load_stored_result(id)?
            .filter(|result| {
                result.state == ResultState::Complete && result.expires_at_ms > timestamp_ms
            })
            .ok_or_else(|| StorageError::ResultNotFound(id.to_owned()))?;
        if stored.format_version != RESULT_FORMAT_VERSION {
            return Err(StorageError::UnsupportedResultFormat {
                id: id.to_owned(),
                found: stored.format_version,
                supported: RESULT_FORMAT_VERSION,
            });
        }
        let path = storage.result_path(id)?;
        let mut file = open_result_file(&path, id)?;
        ensure_exact_file_length(&file, stored.committed_length, id)?;
        let schema = read_schema(&mut file, &path, &stored)?;
        let metadata = stored.metadata()?;

        if request.offset >= metadata.row_count {
            return Ok(ResultPage {
                metadata,
                schema,
                offset: request.offset,
                rows: Vec::new(),
                has_more: false,
            });
        }
        let requested_end = request
            .offset
            .saturating_add(u64::from(request.max_rows))
            .min(metadata.row_count);
        let chunks = storage.load_page_chunks(id, request.offset, requested_end)?;
        let rows = read_page_rows(
            &mut file,
            &path,
            id,
            schema.columns.len(),
            request,
            requested_end,
            chunks,
        )?;

        if rows.is_empty() {
            return Err(corrupt(id, "live page range produced no rows"));
        }
        let returned =
            u64::try_from(rows.len()).map_err(|_| StorageError::NumericRange("page row count"))?;
        let has_more = request.offset.saturating_add(returned) < metadata.row_count;
        Ok(ResultPage {
            metadata,
            schema,
            offset: request.offset,
            rows,
            has_more,
        })
    }

    /// Removes expired completed results and abandoned writing results. Active
    /// writers retain a process-local lease and are never purged.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or filesystem cleanup failures.
    pub fn purge_expired(&self) -> Result<PurgeReport, StorageError> {
        self.purge_expired_at(now_millis()?)
    }

    fn purge_expired_at(&self, timestamp_ms: i64) -> Result<PurgeReport, StorageError> {
        let storage = self.clone();
        let result_guard = storage.lock_results()?;
        let connection = storage.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, committed_length, state FROM retained_results
             WHERE expires_at_ms <= ?1
             ORDER BY id",
        )?;
        let expired = statement
            .query_map([timestamp_ms], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);

        let mut report = PurgeReport::default();
        for (id, bytes, state) in expired {
            if state == "writing" && result_guard.contains(&id) {
                continue;
            }
            storage.delete_result_with_connection(&connection, &id)?;
            report.results_removed += 1;
            report.indexed_bytes_removed = report
                .indexed_bytes_removed
                .saturating_add(u64::try_from(bytes).unwrap_or_default());
        }

        let mut referenced_files = HashSet::new();
        let mut statement = connection.prepare("SELECT id FROM retained_results")?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        for id in ids {
            referenced_files.insert(result_file_name(&id)?);
        }
        for entry in fs::read_dir(&storage.inner.results_dir)
            .map_err(|error| StorageError::io(&storage.inner.results_dir, error))?
        {
            let entry =
                entry.map_err(|error| StorageError::io(&storage.inner.results_dir, error))?;
            let file_type = entry
                .file_type()
                .map_err(|error| StorageError::io(entry.path(), error))?;
            if file_type.is_file()
                && is_result_file_name(&entry.file_name())
                && !referenced_files.contains(&entry.file_name())
            {
                remove_file_if_exists(&entry.path())?;
                report.orphan_files_removed += 1;
            }
        }
        if report.orphan_files_removed > 0 {
            sync_directory(&storage.inner.results_dir)?;
        }
        Ok(report)
    }

    pub(crate) fn recover_at(&self, timestamp_ms: i64) -> Result<RecoveryReport, StorageError> {
        let storage = self.clone();
        let _gate = storage.lock_results()?;
        let connection = storage.connection()?;
        let unsupported = connection
            .query_row(
                "SELECT id, format_version FROM retained_results
                 WHERE format_version != ?1 ORDER BY id LIMIT 1",
                [RESULT_FORMAT_VERSION],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        if let Some((id, found)) = unsupported {
            return Err(StorageError::UnsupportedResultFormat {
                id,
                found,
                supported: RESULT_FORMAT_VERSION,
            });
        }
        let results = load_all_results(&connection)?;
        let mut report = RecoveryReport::default();
        let mut referenced_files = HashSet::new();

        for result in results {
            if result.state == ResultState::Writing {
                storage.delete_result_with_connection(&connection, &result.id)?;
                report.writing_removed += 1;
                continue;
            }
            if result.expires_at_ms <= timestamp_ms {
                storage.delete_result_with_connection(&connection, &result.id)?;
                report.expired_removed += 1;
                continue;
            }

            match storage.validate_completed_result(&connection, &result) {
                Ok(tail_truncated) => {
                    if tail_truncated {
                        report.tails_truncated += 1;
                    }
                    referenced_files.insert(result_file_name(&result.id)?);
                }
                Err(StorageError::CorruptResult { .. }) => {
                    storage.delete_result_with_connection(&connection, &result.id)?;
                    report.corrupt_removed += 1;
                }
                Err(error) => return Err(error),
            }
        }

        for entry in fs::read_dir(&storage.inner.results_dir)
            .map_err(|error| StorageError::io(&storage.inner.results_dir, error))?
        {
            let entry =
                entry.map_err(|error| StorageError::io(&storage.inner.results_dir, error))?;
            let file_type = entry
                .file_type()
                .map_err(|error| StorageError::io(entry.path(), error))?;
            if !file_type.is_file() || !is_result_file_name(&entry.file_name()) {
                continue;
            }
            if !referenced_files.contains(&entry.file_name()) {
                remove_file_if_exists(&entry.path())?;
                report.orphan_files_removed += 1;
            }
        }
        sync_directory(&storage.inner.results_dir)?;
        Ok(report)
    }

    fn validate_completed_result(
        &self,
        connection: &Connection,
        result: &StoredResult,
    ) -> Result<bool, StorageError> {
        if result.format_version != RESULT_FORMAT_VERSION {
            return Err(StorageError::UnsupportedResultFormat {
                id: result.id.clone(),
                found: result.format_version,
                supported: RESULT_FORMAT_VERSION,
            });
        }
        let path = self.result_path(&result.id)?;
        let mut file = open_result_file(&path, &result.id)?;
        let actual_length = file
            .metadata()
            .map_err(|error| StorageError::io(&path, error))?
            .len();
        if actual_length < result.committed_length {
            return Err(corrupt(&result.id, "result file is shorter than its index"));
        }
        let tail_truncated = actual_length > result.committed_length;
        if tail_truncated {
            file.set_len(result.committed_length)
                .and_then(|()| file.sync_data())
                .map_err(|error| StorageError::io(&path, error))?;
        }

        let schema = read_schema(&mut file, &path, result)?;
        let chunks = load_all_chunks(connection, &result.id)?;
        let mut expected_ordinal = 0_u64;
        let mut expected_offset = result.schema_frame_length;
        let mut expected_row = 0_u64;
        for chunk in chunks {
            if chunk.ordinal != expected_ordinal
                || chunk.file_offset != expected_offset
                || chunk.start_row != expected_row
            {
                return Err(corrupt(&result.id, "result chunk index is not contiguous"));
            }
            let batch: wire::RowBatch = read_indexed_frame(
                &mut file,
                &path,
                &result.id,
                chunk.file_offset,
                chunk.frame_length,
                &chunk.sha256,
                MAX_BATCH_BYTES,
            )?;
            validate_indexed_batch(&batch, &chunk, schema.columns.len(), &result.id)?;
            expected_ordinal = expected_ordinal
                .checked_add(1)
                .ok_or(StorageError::NumericRange("result chunk ordinal"))?;
            expected_offset = expected_offset
                .checked_add(chunk.frame_length)
                .ok_or(StorageError::NumericRange("result file offset"))?;
            expected_row = chunk.end_row_exclusive;
        }
        if expected_offset != result.committed_length || expected_row != result.row_count {
            return Err(corrupt(
                &result.id,
                "result aggregate does not match its chunk index",
            ));
        }
        Ok(tail_truncated)
    }

    fn result_path(&self, id: &str) -> Result<PathBuf, StorageError> {
        Ok(self.inner.results_dir.join(result_file_name(id)?))
    }

    fn check_result_quota(&self, requested: u64) -> Result<(), StorageError> {
        let connection = self.connection()?;
        let mut statement =
            connection.prepare("SELECT id, committed_length FROM retained_results")?;
        let mut indexed = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .map(|row| {
                let (id, bytes) = row?;
                Ok((id, from_sql_u64(bytes, "indexed result bytes")?))
            })
            .collect::<Result<HashMap<_, _>, StorageError>>()?;
        drop(statement);

        let mut retained = 0_u64;
        for entry in fs::read_dir(&self.inner.results_dir)
            .map_err(|error| StorageError::io(&self.inner.results_dir, error))?
        {
            let entry = entry.map_err(|error| StorageError::io(&self.inner.results_dir, error))?;
            let file_type = entry
                .file_type()
                .map_err(|error| StorageError::io(entry.path(), error))?;
            if !file_type.is_file() || !is_result_file_name(&entry.file_name()) {
                continue;
            }
            let physical = entry
                .metadata()
                .map_err(|error| StorageError::io(entry.path(), error))?
                .len();
            let id = entry
                .path()
                .file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .ok_or(StorageError::InvalidResult("result file name is invalid"))?
                .to_owned();
            let accounted = indexed
                .remove(&id)
                .map_or(physical, |bytes| bytes.max(physical));
            retained = retained
                .checked_add(accounted)
                .ok_or(StorageError::NumericRange("retained result bytes"))?;
        }
        for bytes in indexed.into_values() {
            retained = retained
                .checked_add(bytes)
                .ok_or(StorageError::NumericRange("retained result bytes"))?;
        }

        let available = self.inner.max_retained_bytes.saturating_sub(retained);
        if requested > available {
            return Err(StorageError::QuotaExceeded {
                requested,
                available,
            });
        }
        Ok(())
    }

    fn insert_writing_result(
        &self,
        id: &str,
        schema_frame_length: u64,
        schema_sha256: &[u8],
        created_at_ms: i64,
        expires_at_ms: i64,
    ) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO retained_results (
                id, format_version, state, schema_frame_length, schema_sha256,
                committed_length, row_count, created_at_ms, expires_at_ms
             ) VALUES (?1, ?2, 'writing', ?3, ?4, ?3, 0, ?5, ?6)",
            params![
                id,
                RESULT_FORMAT_VERSION,
                to_sql_i64(schema_frame_length, "schema frame length")?,
                schema_sha256,
                created_at_ms,
                expires_at_ms,
            ],
        )?;
        #[cfg(test)]
        if crate::take_fault(crate::FaultPoint::ResultBeginAfterCommit) {
            return Err(crate::injected_commit_error());
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_chunk(
        &self,
        id: &str,
        ordinal: u64,
        file_offset: u64,
        frame_length: u64,
        start_row: u64,
        end_row_exclusive: u64,
        frame_hash: &[u8],
        committed_length: u64,
    ) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO result_chunks (
                result_id, ordinal, file_offset, frame_length, start_row,
                end_row_exclusive, row_count, sha256
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                to_sql_i64(ordinal, "chunk ordinal")?,
                to_sql_i64(file_offset, "chunk file offset")?,
                to_sql_i64(frame_length, "chunk frame length")?,
                to_sql_i64(start_row, "chunk start row")?,
                to_sql_i64(end_row_exclusive, "chunk end row")?,
                to_sql_i64(end_row_exclusive - start_row, "chunk row count")?,
                frame_hash,
            ],
        )?;
        let changed = transaction.execute(
            "UPDATE retained_results
             SET committed_length = ?1, row_count = ?2
             WHERE id = ?3 AND state = 'writing'
               AND committed_length = ?4 AND row_count = ?5",
            params![
                to_sql_i64(committed_length, "result committed length")?,
                to_sql_i64(end_row_exclusive, "result row count")?,
                id,
                to_sql_i64(file_offset, "prior committed length")?,
                to_sql_i64(start_row, "prior result row count")?,
            ],
        )?;
        if changed != 1 {
            return Err(StorageError::InvalidResult(
                "result writer no longer owns the writing index",
            ));
        }
        #[cfg(test)]
        if crate::take_fault(crate::FaultPoint::ResultChunkBeforeCommit) {
            return Err(crate::injected_commit_error());
        }
        transaction.commit()?;
        #[cfg(test)]
        if crate::take_fault(crate::FaultPoint::ResultChunkAfterCommit) {
            return Err(crate::injected_commit_error());
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn verify_chunk_commit(
        &self,
        id: &str,
        ordinal: u64,
        file_offset: u64,
        frame_length: u64,
        start_row: u64,
        end_row_exclusive: u64,
        frame_hash: &[u8],
        committed_length: u64,
    ) -> Result<CommitState, StorageError> {
        let connection = self.connection()?;
        let result = load_stored_result_with_connection(&connection, id)?;
        let chunk = load_chunk(&connection, id, ordinal)?;
        match (result, chunk) {
            (Some(result), Some(chunk))
                if result.state == ResultState::Writing
                    && result.committed_length == committed_length
                    && result.row_count == end_row_exclusive
                    && chunk.file_offset == file_offset
                    && chunk.frame_length == frame_length
                    && chunk.start_row == start_row
                    && chunk.end_row_exclusive == end_row_exclusive
                    && chunk.sha256 == frame_hash =>
            {
                Ok(CommitState::Applied)
            }
            (Some(result), None)
                if result.state == ResultState::Writing
                    && result.committed_length == file_offset
                    && result.row_count == start_row =>
            {
                Ok(CommitState::NotApplied)
            }
            _ => Ok(CommitState::Diverged),
        }
    }

    fn commit_completion(
        &self,
        id: &str,
        completed: &wire::QueryCompleted,
        committed_length: u64,
    ) -> Result<(), StorageError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE retained_results
             SET state = 'complete', truncated_by_max_rows = ?1,
                 truncated_by_max_result_bytes = ?2
             WHERE id = ?3 AND state = 'writing' AND row_count = ?4
               AND committed_length = ?5",
            params![
                completed.truncated_by_max_rows,
                completed.truncated_by_max_result_bytes,
                id,
                to_sql_i64(completed.row_count, "completion row count")?,
                to_sql_i64(committed_length, "completion committed length")?,
            ],
        )?;
        if changed != 1 {
            return Err(StorageError::InvalidResult(
                "result completion no longer owns the writing index",
            ));
        }
        #[cfg(test)]
        if crate::take_fault(crate::FaultPoint::ResultCompletionAfterCommit) {
            return Err(crate::injected_commit_error());
        }
        Ok(())
    }

    fn load_stored_result(&self, id: &str) -> Result<Option<StoredResult>, StorageError> {
        let connection = self.connection()?;
        load_stored_result_with_connection(&connection, id)
    }

    fn load_page_chunks(
        &self,
        id: &str,
        start_row: u64,
        end_row: u64,
    ) -> Result<Vec<ChunkIndex>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT ordinal, file_offset, frame_length, start_row,
                    end_row_exclusive, row_count, sha256
             FROM result_chunks
             WHERE result_id = ?1 AND start_row < ?2 AND end_row_exclusive > ?3
             ORDER BY ordinal",
        )?;
        let rows = statement.query_map(
            params![
                id,
                to_sql_i64(end_row, "page end row")?,
                to_sql_i64(start_row, "page start row")?,
            ],
            raw_chunk,
        )?;
        rows.map(|row| decode_chunk(row?)).collect()
    }

    fn delete_result(&self, id: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        self.delete_result_with_connection(&connection, id)
    }

    fn delete_result_with_connection(
        &self,
        connection: &Connection,
        id: &str,
    ) -> Result<(), StorageError> {
        connection.execute("DELETE FROM retained_results WHERE id = ?1", [id])?;
        if let Ok(path) = self.result_path(id) {
            remove_file_if_exists(&path)?;
            sync_directory(&self.inner.results_dir)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultState {
    Writing,
    Complete,
}

#[derive(Debug)]
struct StoredResult {
    id: String,
    format_version: i64,
    state: ResultState,
    schema_frame_length: u64,
    schema_sha256: Vec<u8>,
    committed_length: u64,
    row_count: u64,
    truncated_by_max_rows: bool,
    truncated_by_max_result_bytes: bool,
    created_at_ms: i64,
    expires_at_ms: i64,
}

impl StoredResult {
    fn metadata(&self) -> Result<ResultMetadata, StorageError> {
        if self.state != ResultState::Complete {
            return Err(StorageError::InvalidResult("result is not complete"));
        }
        Ok(ResultMetadata {
            id: self.id.clone(),
            row_count: self.row_count,
            truncated_by_max_rows: self.truncated_by_max_rows,
            truncated_by_max_result_bytes: self.truncated_by_max_result_bytes,
            created_at_ms: self.created_at_ms,
            expires_at_ms: self.expires_at_ms,
        })
    }

    fn matches_completion(&self, completed: &wire::QueryCompleted, committed_length: u64) -> bool {
        self.state == ResultState::Complete
            && self.row_count == completed.row_count
            && self.committed_length == committed_length
            && self.truncated_by_max_rows == completed.truncated_by_max_rows
            && self.truncated_by_max_result_bytes == completed.truncated_by_max_result_bytes
    }

    fn matches_writing(&self, row_count: u64, committed_length: u64) -> bool {
        self.state == ResultState::Writing
            && self.row_count == row_count
            && self.committed_length == committed_length
    }
}

#[derive(Debug)]
struct ChunkIndex {
    ordinal: u64,
    file_offset: u64,
    frame_length: u64,
    start_row: u64,
    end_row_exclusive: u64,
    row_count: u64,
    sha256: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitState {
    Applied,
    NotApplied,
    Diverged,
}

type RawResult = (
    String,
    i64,
    String,
    i64,
    Vec<u8>,
    i64,
    i64,
    bool,
    bool,
    i64,
    i64,
);
type RawChunk = (i64, i64, i64, i64, i64, i64, Vec<u8>);

fn raw_result(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawResult> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
    ))
}

fn decode_result(raw: RawResult) -> Result<StoredResult, StorageError> {
    let (
        id,
        format_version,
        state,
        schema_frame_length,
        schema_sha256,
        committed_length,
        row_count,
        truncated_rows,
        truncated_bytes,
        created_at_ms,
        expires_at_ms,
    ) = raw;
    let state = match state.as_str() {
        "writing" => ResultState::Writing,
        "complete" => ResultState::Complete,
        _ => return Err(corrupt(&id, "invalid result state")),
    };
    Ok(StoredResult {
        id,
        format_version,
        state,
        schema_frame_length: from_sql_u64(schema_frame_length, "schema frame length")?,
        schema_sha256,
        committed_length: from_sql_u64(committed_length, "result committed length")?,
        row_count: from_sql_u64(row_count, "result row count")?,
        truncated_by_max_rows: truncated_rows,
        truncated_by_max_result_bytes: truncated_bytes,
        created_at_ms,
        expires_at_ms,
    })
}

fn raw_chunk(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawChunk> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

fn decode_chunk(raw: RawChunk) -> Result<ChunkIndex, StorageError> {
    Ok(ChunkIndex {
        ordinal: from_sql_u64(raw.0, "chunk ordinal")?,
        file_offset: from_sql_u64(raw.1, "chunk file offset")?,
        frame_length: from_sql_u64(raw.2, "chunk frame length")?,
        start_row: from_sql_u64(raw.3, "chunk start row")?,
        end_row_exclusive: from_sql_u64(raw.4, "chunk end row")?,
        row_count: from_sql_u64(raw.5, "chunk row count")?,
        sha256: raw.6,
    })
}

fn load_stored_result_with_connection(
    connection: &Connection,
    id: &str,
) -> Result<Option<StoredResult>, StorageError> {
    let raw = connection
        .query_row(
            "SELECT id, format_version, state, schema_frame_length, schema_sha256,
                    committed_length, row_count, truncated_by_max_rows,
                    truncated_by_max_result_bytes, created_at_ms, expires_at_ms
             FROM retained_results WHERE id = ?1",
            [id],
            raw_result,
        )
        .optional()?;
    raw.map(decode_result).transpose()
}

fn load_all_results(connection: &Connection) -> Result<Vec<StoredResult>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT id, format_version, state, schema_frame_length, schema_sha256,
                committed_length, row_count, truncated_by_max_rows,
                truncated_by_max_result_bytes, created_at_ms, expires_at_ms
         FROM retained_results ORDER BY id",
    )?;
    statement
        .query_map([], raw_result)?
        .map(|row| decode_result(row?))
        .collect()
}

fn load_all_chunks(connection: &Connection, id: &str) -> Result<Vec<ChunkIndex>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT ordinal, file_offset, frame_length, start_row,
                end_row_exclusive, row_count, sha256
         FROM result_chunks WHERE result_id = ?1 ORDER BY ordinal",
    )?;
    statement
        .query_map([id], raw_chunk)?
        .map(|row| decode_chunk(row?))
        .collect()
}

fn load_chunk(
    connection: &Connection,
    id: &str,
    ordinal: u64,
) -> Result<Option<ChunkIndex>, StorageError> {
    let raw = connection
        .query_row(
            "SELECT ordinal, file_offset, frame_length, start_row,
                    end_row_exclusive, row_count, sha256
             FROM result_chunks WHERE result_id = ?1 AND ordinal = ?2",
            params![id, to_sql_i64(ordinal, "chunk ordinal")?],
            raw_chunk,
        )
        .optional()?;
    raw.map(decode_chunk).transpose()
}

fn validate_schema(schema: &wire::QueryStarted) -> Result<(), StorageError> {
    if schema.columns.len() > MAX_COLUMNS {
        return Err(StorageError::InvalidResult(
            "query schema exceeds the protocol column limit",
        ));
    }
    Ok(())
}

fn validate_batch(
    batch: &wire::RowBatch,
    expected_start: u64,
    schema_columns: usize,
) -> Result<(), StorageError> {
    if batch.start_row_offset != expected_start {
        return Err(StorageError::InvalidResult(
            "row batch is not contiguous with prior chunks",
        ));
    }
    if batch.rows.is_empty() || batch.rows.len() > MAX_BATCH_ROWS {
        return Err(StorageError::InvalidResult(
            "row batch count is outside protocol limits",
        ));
    }
    if batch.encoded_len() > MAX_BATCH_BYTES {
        return Err(StorageError::InvalidResult(
            "row batch exceeds the protocol byte limit",
        ));
    }
    if batch
        .rows
        .iter()
        .any(|row| row.values.len() != schema_columns)
    {
        return Err(StorageError::InvalidResult(
            "row width does not match query schema",
        ));
    }
    Ok(())
}

fn validate_indexed_batch(
    batch: &wire::RowBatch,
    chunk: &ChunkIndex,
    schema_columns: usize,
    id: &str,
) -> Result<(), StorageError> {
    if batch.start_row_offset != chunk.start_row
        || u64::try_from(batch.rows.len()).ok() != Some(chunk.row_count)
        || chunk.end_row_exclusive.checked_sub(chunk.start_row) != Some(chunk.row_count)
    {
        return Err(corrupt(id, "row batch does not match its chunk index"));
    }
    validate_batch(batch, chunk.start_row, schema_columns)
        .map_err(|_| corrupt(id, "indexed row batch violates protocol limits"))
}

fn validate_page_request(request: PageRequest) -> Result<(), StorageError> {
    if request.max_rows == 0 || request.max_rows > MAX_RESULT_PAGE_ROWS {
        return Err(StorageError::InvalidResult(
            "page row limit must be between 1 and 4096",
        ));
    }
    if !(MIN_RESULT_PAGE_BYTES..=MAX_RESULT_PAGE_BYTES).contains(&request.max_bytes) {
        return Err(StorageError::InvalidResult(
            "page byte limit must be between 8 MiB and 16 MiB",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn read_page_rows(
    file: &mut File,
    path: &Path,
    id: &str,
    schema_columns: usize,
    request: PageRequest,
    requested_end: u64,
    chunks: Vec<ChunkIndex>,
) -> Result<Vec<wire::JdbcRow>, StorageError> {
    if chunks.is_empty() {
        return Err(corrupt(id, "result page has no indexed chunks"));
    }
    let mut rows = Vec::with_capacity(request.max_rows as usize);
    let mut row_bytes = 0_u64;
    let mut expected_chunk_start = None;
    'chunks: for chunk in chunks {
        match expected_chunk_start {
            None if chunk.start_row > request.offset
                || chunk.end_row_exclusive <= request.offset =>
            {
                return Err(corrupt(
                    id,
                    "first result page chunk does not cover its offset",
                ));
            }
            Some(expected) if chunk.start_row != expected => {
                return Err(corrupt(id, "result page chunk rows are not contiguous"));
            }
            None | Some(_) => {}
        }
        let batch: wire::RowBatch = read_indexed_frame(
            file,
            path,
            id,
            chunk.file_offset,
            chunk.frame_length,
            &chunk.sha256,
            MAX_BATCH_BYTES,
        )?;
        validate_indexed_batch(&batch, &chunk, schema_columns, id)?;
        expected_chunk_start = Some(chunk.end_row_exclusive);

        for (index, row) in batch.rows.into_iter().enumerate() {
            let global_row = batch
                .start_row_offset
                .checked_add(
                    u64::try_from(index)
                        .map_err(|_| StorageError::NumericRange("result page row index"))?,
                )
                .ok_or(StorageError::NumericRange("result page row offset"))?;
            if global_row < request.offset {
                continue;
            }
            if global_row >= requested_end || rows.len() >= request.max_rows as usize {
                break 'chunks;
            }
            let encoded = u64::try_from(row.encoded_len())
                .map_err(|_| StorageError::NumericRange("page row bytes"))?;
            if encoded > request.max_bytes {
                return Err(corrupt(id, "one row exceeds the minimum page byte budget"));
            }
            if row_bytes.saturating_add(encoded) > request.max_bytes {
                break 'chunks;
            }
            row_bytes += encoded;
            rows.push(row);
        }
    }
    Ok(rows)
}

fn encode_frame<M: Message>(message: &M, maximum: usize) -> Result<Vec<u8>, StorageError> {
    let payload_length = message.encoded_len();
    if payload_length == 0 || payload_length > maximum {
        return Err(StorageError::InvalidResult(
            "protobuf payload is empty or exceeds its frame limit",
        ));
    }
    let encoded_length = u32::try_from(payload_length)
        .map_err(|_| StorageError::NumericRange("protobuf payload length"))?;
    let mut frame = Vec::with_capacity(FRAME_PREFIX_BYTES + payload_length);
    frame.extend_from_slice(&encoded_length.to_be_bytes());
    message
        .encode(&mut frame)
        .map_err(|_| StorageError::InvalidResult("protobuf frame encoding failed"))?;
    Ok(frame)
}

fn read_schema(
    file: &mut File,
    path: &Path,
    result: &StoredResult,
) -> Result<wire::QueryStarted, StorageError> {
    let schema: wire::QueryStarted = read_indexed_frame(
        file,
        path,
        &result.id,
        0,
        result.schema_frame_length,
        &result.schema_sha256,
        MAX_FRAME_BYTES,
    )?;
    validate_schema(&schema).map_err(|_| corrupt(&result.id, "stored schema is invalid"))?;
    Ok(schema)
}

#[allow(clippy::too_many_arguments)]
fn read_indexed_frame<M: Message + Default>(
    file: &mut File,
    path: &Path,
    id: &str,
    offset: u64,
    frame_length: u64,
    expected_hash: &[u8],
    maximum_payload: usize,
) -> Result<M, StorageError> {
    let frame_length_usize = usize::try_from(frame_length)
        .map_err(|_| StorageError::NumericRange("indexed frame length"))?;
    if frame_length_usize <= FRAME_PREFIX_BYTES
        || frame_length_usize > FRAME_PREFIX_BYTES + maximum_payload
        || expected_hash.len() != 32
    {
        return Err(corrupt(id, "indexed frame metadata is invalid"));
    }
    let mut frame = vec![0_u8; frame_length_usize];
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| file.read_exact(&mut frame))
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                corrupt(id, "indexed frame is truncated")
            } else {
                StorageError::io(path, error)
            }
        })?;
    if sha256(&frame).as_slice() != expected_hash {
        return Err(corrupt(id, "indexed frame hash mismatch"));
    }
    let payload_length = u32::from_be_bytes(
        frame[..FRAME_PREFIX_BYTES]
            .try_into()
            .map_err(|_| corrupt(id, "indexed frame length prefix is missing"))?,
    ) as usize;
    if payload_length != frame_length_usize - FRAME_PREFIX_BYTES || payload_length > maximum_payload
    {
        return Err(corrupt(id, "indexed frame length prefix is invalid"));
    }
    M::decode(&frame[FRAME_PREFIX_BYTES..])
        .map_err(|_| corrupt(id, "indexed protobuf payload cannot be decoded"))
}

fn open_result_file(path: &Path, id: &str) -> Result<File, StorageError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                corrupt(id, "result file is missing")
            } else {
                StorageError::io(path, error)
            }
        })
}

fn ensure_exact_file_length(file: &File, expected: u64, id: &str) -> Result<(), StorageError> {
    let actual = file
        .metadata()
        .map_err(|_| corrupt(id, "result file metadata cannot be read"))?
        .len();
    if actual != expected {
        return Err(corrupt(id, "completed result file length changed"));
    }
    Ok(())
}

fn result_file_name(id: &str) -> Result<std::ffi::OsString, StorageError> {
    let uuid =
        Uuid::parse_str(id).map_err(|_| StorageError::InvalidResult("result id is invalid"))?;
    Ok(format!("{uuid}.{RESULT_FILE_EXTENSION}").into())
}

fn is_result_file_name(name: &std::ffi::OsStr) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension == RESULT_FILE_EXTENSION)
        && Path::new(name)
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|stem| Uuid::parse_str(stem).is_ok())
}

fn remove_file_if_exists(path: &Path) -> Result<(), StorageError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StorageError::io(path, error)),
    }
}

fn sha256(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}

fn to_sql_i64(value: u64, label: &'static str) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::NumericRange(label))
}

fn from_sql_u64(value: i64, label: &'static str) -> Result<u64, StorageError> {
    u64::try_from(value).map_err(|_| StorageError::NumericRange(label))
}

fn corrupt(id: &str, reason: &'static str) -> StorageError {
    StorageError::CorruptResult {
        result_id: id.to_owned(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        sync::Arc,
        time::Duration,
    };

    use chat2db_engine_protocol::wire;
    use prost::Message;
    use tempfile::TempDir;

    use super::{MIN_RESULT_PAGE_BYTES, PageRequest, RESULT_FILE_EXTENSION, RecoveryReport};
    use crate::{
        SecretRef, SecretValue, SecretVault, SecretVaultError, Storage, StorageError,
        StorageOptions,
    };

    #[derive(Debug, Default)]
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

    fn schema() -> wire::QueryStarted {
        wire::QueryStarted {
            columns: vec![wire::JdbcColumn {
                ordinal: 1,
                label: "value".to_owned(),
                name: "value".to_owned(),
                jdbc_type: 12,
                jdbc_type_name: "VARCHAR".to_owned(),
                value_type: wire::JdbcValueType::Text as i32,
                nullability: wire::ColumnNullability::Nullable as i32,
                ..Default::default()
            }],
        }
    }

    fn rows(start: u64, values: &[&str]) -> wire::RowBatch {
        wire::RowBatch {
            start_row_offset: start,
            rows: values
                .iter()
                .map(|value| wire::JdbcRow {
                    values: vec![wire::JdbcValue {
                        value: Some(wire::jdbc_value::Value::TextValue((*value).to_owned())),
                    }],
                })
                .collect(),
        }
    }

    fn completed(row_count: u64) -> wire::QueryCompleted {
        wire::QueryCompleted {
            row_count,
            truncated_by_max_rows: false,
            truncated_by_max_result_bytes: false,
        }
    }

    #[test]
    fn pages_across_chunks_and_inside_batch_boundaries() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let mut writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("result begins");
        writer
            .append_batch(&rows(0, &["zero", "one", "two"]))
            .expect("first batch appends");
        writer
            .append_batch(&rows(3, &["three", "four", "five"]))
            .expect("second batch appends");
        let metadata = writer.finish(&completed(6)).expect("result completes");

        let page = storage
            .read_result_page(
                &metadata.id,
                PageRequest {
                    offset: 2,
                    max_rows: 3,
                    ..PageRequest::default()
                },
            )
            .expect("page reads");
        assert_eq!(page.rows.len(), 3);
        assert_eq!(page.rows[0], rows(0, &["two"]).rows[0]);
        assert_eq!(page.rows[2], rows(0, &["four"]).rows[0]);
        assert!(page.has_more);
    }

    #[test]
    fn page_byte_budget_stops_before_the_next_large_row() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let value = "x".repeat(4_194_300);
        let mut writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("result begins");
        writer
            .append_batch(&rows(0, &[&value]))
            .expect("first large batch appends");
        writer
            .append_batch(&rows(1, &[&value]))
            .expect("second large batch appends");
        let metadata = writer.finish(&completed(2)).expect("result completes");

        let page = storage
            .read_result_page(
                &metadata.id,
                PageRequest {
                    max_bytes: MIN_RESULT_PAGE_BYTES,
                    ..PageRequest::default()
                },
            )
            .expect("bounded page reads");
        assert_eq!(page.rows.len(), 1);
        assert!(page.has_more);
    }

    #[test]
    fn quota_failure_poisons_the_writer_before_file_growth() {
        let directory = TempDir::new().expect("temp dir");
        let storage = Storage::open_with_options(
            directory.path(),
            StorageOptions {
                max_retained_bytes: 256,
            },
            Arc::new(EmptyVault),
        )
        .expect("storage opens");
        let mut writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("schema fits quota");
        let path = storage.result_path(writer.id()).expect("result path");
        let initial_length = fs::metadata(&path).expect("result metadata").len();
        let error = writer
            .append_batch(&rows(0, &[&"x".repeat(1024)]))
            .expect_err("batch must exceed quota");
        assert!(matches!(error, StorageError::QuotaExceeded { .. }));
        assert_eq!(
            fs::metadata(path).expect("result metadata").len(),
            initial_length
        );
        let poisoned = writer
            .append_batch(&rows(0, &["small"]))
            .expect_err("quota failure poisons writer");
        assert!(matches!(
            poisoned,
            StorageError::InvalidResult("result writer is poisoned")
        ));
    }

    #[test]
    fn empty_completed_result_retains_its_schema() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("result begins");
        let metadata = writer.finish(&completed(0)).expect("result completes");

        let page = storage
            .read_result_page(&metadata.id, PageRequest::default())
            .expect("empty page reads");
        assert_eq!(page.schema, schema());
        assert!(page.rows.is_empty());
        assert!(!page.has_more);
    }

    #[test]
    fn result_writes_reconcile_every_post_commit_failure() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        crate::inject_faults(&[
            crate::FaultPoint::ResultBeginAfterCommit,
            crate::FaultPoint::ResultChunkAfterCommit,
            crate::FaultPoint::ResultCompletionAfterCommit,
        ]);

        let mut writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("committed begin reconciles");
        writer
            .append_batch(&rows(0, &["one"]))
            .expect("committed chunk reconciles");
        let metadata = writer
            .finish(&completed(1))
            .expect("committed completion reconciles");
        let page = storage
            .read_result_page(&metadata.id, PageRequest::default())
            .expect("reconciled result reads");
        assert_eq!(page.rows, rows(0, &["one"]).rows);
    }

    #[test]
    fn result_chunk_pre_commit_failure_is_proven_rolled_back_and_reclaimed() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let mut writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("result begins");
        let id = writer.id().to_owned();
        let path = storage.result_path(&id).expect("result path");
        crate::inject_faults(&[crate::FaultPoint::ResultChunkBeforeCommit]);

        let error = writer
            .append_batch(&rows(0, &["one"]))
            .expect_err("pre-commit failure must surface");
        assert!(matches!(error, StorageError::Integrity(_)));
        drop(writer);
        assert!(!path.exists());
        assert!(
            storage
                .load_stored_result(&id)
                .expect("result index reads")
                .is_none()
        );
    }

    #[test]
    fn unreadable_completion_outcome_is_preserved_for_later_recovery() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let mut writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("result begins");
        writer
            .append_batch(&rows(0, &["one"]))
            .expect("batch appends");
        let id = writer.id().to_owned();
        crate::inject_faults(&[
            crate::FaultPoint::ResultCompletionAfterCommit,
            crate::FaultPoint::ResultCompletionReadback,
        ]);

        let error = writer
            .finish(&completed(1))
            .expect_err("unreadable commit outcome must stay unknown");
        assert!(matches!(
            error,
            StorageError::OutcomeUnknown {
                operation: "complete retained result",
                ..
            }
        ));
        assert!(
            storage
                .result_metadata(&id)
                .expect("later readback succeeds")
                .is_some()
        );
    }

    #[test]
    fn failed_append_poisons_the_writer() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let mut writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("result begins");

        let error = writer
            .append_batch(&rows(9, &["gap"]))
            .expect_err("gap must fail");
        assert!(matches!(error, StorageError::InvalidResult(_)));
        let poisoned = writer
            .append_batch(&rows(0, &["cannot-continue"]))
            .expect_err("poisoned writer must fail");
        assert!(matches!(
            poisoned,
            StorageError::InvalidResult("result writer is poisoned")
        ));
    }

    #[test]
    fn startup_removes_writing_results_and_orphan_files() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let mut writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("result begins");
        let writing_id = writer.id().to_owned();
        writer.cleanup_on_drop = false;
        storage
            .lock_results()
            .expect("result gate")
            .remove(&writing_id);
        drop(writer);
        drop(storage);

        let orphan = directory.path().join("results").join(format!(
            "{}.{}",
            uuid::Uuid::new_v4(),
            RESULT_FILE_EXTENSION
        ));
        fs::write(&orphan, b"orphan").expect("orphan writes");
        let recovered = open(&directory);
        assert!(
            recovered
                .result_metadata(&writing_id)
                .expect("metadata reads")
                .is_none()
        );
        assert_eq!(recovered.startup_report().results.writing_removed, 1);
        assert_eq!(recovered.startup_report().results.orphan_files_removed, 1);
        assert!(!orphan.exists());
    }

    #[test]
    fn dropping_or_failing_a_writer_reclaims_its_file_and_quota() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("result begins");
        let dropped_id = writer.id().to_owned();
        let dropped_path = storage.result_path(&dropped_id).expect("result path");
        drop(writer);
        assert!(!dropped_path.exists());
        assert!(
            storage
                .load_stored_result(&dropped_id)
                .expect("result index reads")
                .is_none()
        );

        let mut writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("second result begins");
        writer
            .append_batch(&rows(0, &["one"]))
            .expect("batch appends");
        let failed_id = writer.id().to_owned();
        let failed_path = storage.result_path(&failed_id).expect("result path");
        let error = writer
            .finish(&completed(2))
            .expect_err("mismatched completion must fail");
        assert!(matches!(error, StorageError::InvalidResult(_)));
        assert!(!failed_path.exists());
        assert!(
            storage
                .load_stored_result(&failed_id)
                .expect("result index reads")
                .is_none()
        );
    }

    #[test]
    fn physical_orphan_bytes_count_toward_the_global_quota() {
        let directory = TempDir::new().expect("temp dir");
        let storage = Storage::open_with_options(
            directory.path(),
            StorageOptions {
                max_retained_bytes: 256,
            },
            Arc::new(EmptyVault),
        )
        .expect("storage opens");
        let orphan = directory.path().join("results").join(format!(
            "{}.{}",
            uuid::Uuid::new_v4(),
            RESULT_FILE_EXTENSION
        ));
        fs::write(&orphan, vec![0_u8; 256]).expect("orphan writes");

        let error = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect_err("physical bytes must exhaust quota");
        assert!(matches!(error, StorageError::QuotaExceeded { .. }));

        let report = storage.purge_expired().expect("runtime purge succeeds");
        assert_eq!(report.orphan_files_removed, 1);
        drop(
            storage
                .begin_result(&schema(), Duration::from_secs(60))
                .expect("quota is available after orphan cleanup"),
        );
    }

    #[test]
    fn startup_truncates_unindexed_tail_but_keeps_complete_result() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let mut writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("result begins");
        writer
            .append_batch(&rows(0, &["one"]))
            .expect("batch appends");
        let metadata = writer.finish(&completed(1)).expect("result completes");
        let path = storage.result_path(&metadata.id).expect("result path");
        let indexed_length = fs::metadata(&path).expect("metadata").len();
        drop(storage);

        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("result opens")
            .write_all(b"unindexed-tail")
            .expect("tail appends");
        let recovered = open(&directory);
        assert_eq!(recovered.startup_report().results.tails_truncated, 1);
        assert_eq!(fs::metadata(&path).expect("metadata").len(), indexed_length);
        assert!(
            recovered
                .result_metadata(&metadata.id)
                .expect("metadata reads")
                .is_some()
        );
    }

    #[test]
    fn startup_evicts_missing_truncated_and_hash_corrupt_results() {
        for damage in ["missing", "truncated", "hash"] {
            let directory = TempDir::new().expect("temp dir");
            let storage = open(&directory);
            let mut writer = storage
                .begin_result(&schema(), Duration::from_secs(60))
                .expect("result begins");
            writer
                .append_batch(&rows(0, &["one"]))
                .expect("batch appends");
            let metadata = writer.finish(&completed(1)).expect("result completes");
            let path = storage.result_path(&metadata.id).expect("result path");
            drop(storage);

            match damage {
                "missing" => fs::remove_file(&path).expect("result removes"),
                "truncated" => {
                    let length = fs::metadata(&path).expect("metadata").len();
                    OpenOptions::new()
                        .write(true)
                        .open(&path)
                        .expect("result opens")
                        .set_len(length - 1)
                        .expect("result truncates");
                }
                "hash" => {
                    let mut bytes = fs::read(&path).expect("result reads");
                    let last = bytes.last_mut().expect("result nonempty");
                    *last ^= 0xff;
                    fs::write(&path, bytes).expect("result rewrites");
                }
                _ => unreachable!(),
            }

            let recovered = open(&directory);
            assert_eq!(recovered.startup_report().results.corrupt_removed, 1);
            assert!(
                recovered
                    .result_metadata(&metadata.id)
                    .expect("metadata reads")
                    .is_none()
            );
        }
    }

    #[test]
    fn startup_preserves_every_newer_result_state_and_fails_closed() {
        for state in ["complete", "writing", "expired"] {
            let directory = TempDir::new().expect("temp dir");
            let storage = open(&directory);
            let writer = storage
                .begin_result(&schema(), Duration::from_secs(60))
                .expect("result begins");
            let metadata = writer.finish(&completed(0)).expect("result completes");
            let path = storage.result_path(&metadata.id).expect("result path");
            let update = match state {
                "complete" => "UPDATE retained_results SET format_version = 2 WHERE id = ?1",
                "writing" => {
                    "UPDATE retained_results SET format_version = 2, state = 'writing' WHERE id = ?1"
                }
                "expired" => {
                    "UPDATE retained_results SET format_version = 2, expires_at_ms = 0 WHERE id = ?1"
                }
                _ => unreachable!(),
            };
            storage
                .connection()
                .expect("connection opens")
                .execute(update, [&metadata.id])
                .expect("future format changes");
            drop(storage);

            let error = Storage::open(directory.path(), Arc::new(EmptyVault))
                .expect_err("newer result format must stop startup");
            assert!(matches!(
                error,
                StorageError::UnsupportedResultFormat {
                    found: 2,
                    supported: 1,
                    ..
                }
            ));
            let retained: i64 =
                rusqlite::Connection::open(directory.path().join("chat2db.sqlite3"))
                    .expect("database opens")
                    .query_row(
                        "SELECT COUNT(*) FROM retained_results WHERE id = ?1",
                        [&metadata.id],
                        |row| row.get(0),
                    )
                    .expect("retained result count reads");
            assert_eq!(retained, 1, "state {state}");
            assert!(path.exists(), "state {state}");
        }
    }

    #[test]
    fn startup_evicts_a_noncontiguous_chunk_index_idempotently() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let mut writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("result begins");
        writer
            .append_batch(&rows(0, &["one"]))
            .expect("batch appends");
        let metadata = writer.finish(&completed(1)).expect("result completes");
        storage
            .connection()
            .expect("connection opens")
            .execute(
                "UPDATE result_chunks
                 SET start_row = 1, end_row_exclusive = 2
                 WHERE result_id = ?1 AND ordinal = 0",
                [&metadata.id],
            )
            .expect("chunk index changes");
        drop(storage);

        let recovered = open(&directory);
        assert_eq!(recovered.startup_report().results.corrupt_removed, 1);
        assert!(
            recovered
                .result_metadata(&metadata.id)
                .expect("metadata reads")
                .is_none()
        );
        drop(recovered);
        let second = open(&directory);
        assert_eq!(second.startup_report().results, RecoveryReport::default());
    }

    #[test]
    fn expired_complete_results_are_purged_idempotently() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let created = crate::now_millis().expect("clock");
        let writer = storage
            .begin_result_at(&schema(), created, created + 10)
            .expect("result begins");
        let metadata = writer.finish(&completed(0)).expect("result completes");

        let report = storage
            .purge_expired_at(created + 10)
            .expect("expiry purges");
        assert_eq!(report.results_removed, 1);
        assert!(
            storage
                .result_metadata_at(&metadata.id, created + 10)
                .expect("metadata")
                .is_none()
        );
        assert_eq!(
            storage
                .purge_expired_at(created + 10)
                .expect("second purge")
                .results_removed,
            0
        );
    }

    #[test]
    fn runtime_expiry_does_not_delete_an_active_writer() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let created = crate::now_millis().expect("clock");
        let mut writer = storage
            .begin_result_at(&schema(), created, created + 1)
            .expect("result begins");

        assert_eq!(
            storage
                .purge_expired_at(created + 1)
                .expect("expiry checks")
                .results_removed,
            0
        );
        writer
            .append_batch(&rows(0, &["still-owned"]))
            .expect("active writer remains valid");
    }

    #[test]
    fn runtime_expiry_reclaims_an_inactive_writing_result() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let created = crate::now_millis().expect("clock");
        let mut writer = storage
            .begin_result_at(&schema(), created, created + 1)
            .expect("result begins");
        let id = writer.id().to_owned();
        let path = storage.result_path(&id).expect("result path");
        writer.cleanup_on_drop = false;
        storage.lock_results().expect("result gate").remove(&id);
        drop(writer);

        let report = storage
            .purge_expired_at(created + 1)
            .expect("inactive writing result purges");
        assert_eq!(report.results_removed, 1);
        assert!(!path.exists());
    }

    #[test]
    fn page_request_rejects_an_unbounded_or_too_small_budget() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("result begins");
        let metadata = writer.finish(&completed(0)).expect("result completes");

        let error = storage
            .read_result_page(
                &metadata.id,
                PageRequest {
                    max_bytes: MIN_RESULT_PAGE_BYTES - 1,
                    ..PageRequest::default()
                },
            )
            .expect_err("small byte budget must fail");
        assert!(matches!(error, StorageError::InvalidResult(_)));
    }

    #[test]
    fn completion_rejects_mismatched_row_count() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let mut writer = storage
            .begin_result(&schema(), Duration::from_secs(60))
            .expect("result begins");
        writer
            .append_batch(&rows(0, &["one"]))
            .expect("batch appends");
        let error = writer.finish(&completed(2)).expect_err("count must fail");
        assert!(matches!(error, StorageError::InvalidResult(_)));
    }

    #[test]
    fn file_format_is_big_endian_length_prefixed_protobuf() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let schema = schema();
        let writer = storage
            .begin_result(&schema, Duration::from_secs(60))
            .expect("result begins");
        let path = storage.result_path(writer.id()).expect("result path");
        let bytes = fs::read(path).expect("result reads");
        let payload_length = u32::from_be_bytes(bytes[..4].try_into().expect("prefix")) as usize;
        assert_eq!(payload_length, schema.encoded_len());
        assert_eq!(
            wire::QueryStarted::decode(&bytes[4..]).expect("schema decodes"),
            schema
        );
    }
}
