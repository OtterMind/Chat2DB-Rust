//! Durable `Chat2DB` product state and retained query-result storage.

mod agent;
mod datasource;
mod error;
mod provider;
mod result_store;
mod secret;
mod vault;

use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use directories::ProjectDirs;
use fs2::FileExt;
use rusqlite::{Connection, OpenFlags};

pub use agent::{
    AgentCompaction, AgentMessageRecord, AgentMessageRole, AgentRecoveryReport, AgentResultHandle,
    AgentRunMessage, AgentRunRecord, AgentRunStatus, AgentRunUpdate, AgentSessionRecord,
    AppendAgentMessage, CancelAgentRun, CancelledAgentRun, CompactAgentRun, CompactedAgentRun,
    CompleteAgentRun, CompletedAgentRun, CreateAgentSession, FailAgentRun, FailedAgentRun,
    MAX_AGENT_MESSAGE_BYTES, MAX_AGENT_MESSAGE_BYTES_PER_SESSION, MAX_AGENT_MESSAGE_PAGE_SIZE,
    MAX_AGENT_MESSAGES_PER_SESSION, MAX_AGENT_SESSION_TITLE_BYTES, RequestToolPermission,
    SqlPermissionMode, StartAgentRun, StartedAgentRun, ToolPermissionDecision,
    ToolPermissionRecord, ToolPermissionStatus, UnknownAgentWrite, UpdateAgentSession,
};
pub use datasource::{
    CreateDatasource, DatasourceRecord, SecretChange, SecretCleanupReport, UpdateDatasource,
};
pub use error::StorageError;
pub use provider::{
    CreateProviderProfile, ProviderKind, ProviderProfileRecord, UpdateProviderProfile,
};
pub use result_store::{
    MAX_RESULT_PAGE_BYTES, MAX_RESULT_PAGE_ROWS, MIN_RESULT_PAGE_BYTES, PageRequest, PurgeReport,
    RecoveryReport, ResultMetadata, ResultPage, ResultWriter,
};
pub use secret::{SecretRef, SecretValue, SecretVault, SecretVaultError};
pub use vault::EncryptedFileVault;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub use vault::OsSecretVault;

const DATABASE_FILE: &str = "chat2db.sqlite3";
const LOCK_FILE: &str = ".chat2db.lock";
const RESULTS_DIRECTORY: &str = "results";
const CURRENT_SCHEMA_VERSION: i64 = 2;

#[cfg(test)]
#[derive(Clone, Copy)]
#[repr(u32)]
pub(crate) enum FaultPoint {
    DatasourceCreateAfterCommit = 1 << 0,
    DatasourceUpdateAfterCommit = 1 << 1,
    DatasourceDeleteAfterCommit = 1 << 2,
    ResultBeginAfterCommit = 1 << 3,
    ResultChunkAfterCommit = 1 << 4,
    ResultCompletionAfterCommit = 1 << 5,
    ResultCompletionReadback = 1 << 6,
    DatasourceCreateBeforeCommit = 1 << 7,
    ResultChunkBeforeCommit = 1 << 8,
    ProviderCreateAfterCommit = 1 << 9,
    ProviderUpdateAfterCommit = 1 << 10,
    ProviderDeleteAfterCommit = 1 << 11,
    AgentCompactionBeforeCommit = 1 << 12,
    AgentCompactionAfterCommit = 1 << 13,
    AgentCompactionReadback = 1 << 14,
    AgentCompactionCommitFailure = 1 << 15,
    AgentRunStartAfterCommit = 1 << 16,
    AgentRunStartReadback = 1 << 17,
    AgentRunCancellationAfterCommit = 1 << 18,
    AgentRunCancellationReadback = 1 << 19,
}

#[cfg(test)]
std::thread_local! {
    static FAULT_POINTS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn inject_faults(points: &[FaultPoint]) {
    let mask = points.iter().fold(0, |mask, point| mask | *point as u32);
    FAULT_POINTS.set(mask);
}

#[cfg(test)]
pub(crate) fn take_fault(point: FaultPoint) -> bool {
    FAULT_POINTS.with(|points| {
        let mask = points.get();
        let selected = point as u32;
        if mask & selected == 0 {
            return false;
        }
        points.set(mask & !selected);
        true
    })
}

#[cfg(test)]
pub(crate) fn injected_commit_error() -> StorageError {
    StorageError::Integrity("injected post-commit failure".to_owned())
}

/// Default upper bound for all retained result files in one data directory.
pub const DEFAULT_MAX_RETAINED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Durable storage resource limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageOptions {
    /// Maximum indexed bytes across writing and completed retained results.
    pub max_retained_bytes: u64,
}

/// Recovery work completed before a [`Storage`] handle becomes usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartupReport {
    /// Retained-result filesystem and index recovery.
    pub results: RecoveryReport,
    /// Deferred secret deletions retried against the configured vault.
    pub secrets: SecretCleanupReport,
    /// Agent runs, permissions, and result handles recovered at startup.
    pub agents: AgentRecoveryReport,
}

impl Default for StorageOptions {
    fn default() -> Self {
        Self {
            max_retained_bytes: DEFAULT_MAX_RETAINED_BYTES,
        }
    }
}

/// Cloneable handle to one process-owned `Chat2DB` data directory.
#[derive(Clone)]
pub struct Storage {
    pub(crate) inner: Arc<StorageInner>,
}

pub(crate) struct StorageInner {
    data_dir: PathBuf,
    database_path: PathBuf,
    results_dir: PathBuf,
    secret_gate: Mutex<()>,
    result_gate: Mutex<HashSet<String>>,
    max_retained_bytes: u64,
    vault: Arc<dyn SecretVault>,
    startup_report: OnceLock<StartupReport>,
    _directory_lock: File,
}

impl std::fmt::Debug for Storage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Storage")
            .field("data_dir", &self.inner.data_dir)
            .finish_non_exhaustive()
    }
}

impl Storage {
    /// Returns the operating system's standard per-user `Chat2DB` data directory.
    ///
    /// Hosts use this path to construct the production secret vault before
    /// opening storage over the same directory.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform does not expose an application data
    /// directory.
    pub fn default_data_dir() -> Result<PathBuf, StorageError> {
        let project_dirs = ProjectDirs::from("ai", "Chat2DB", "Chat2DB")
            .ok_or(StorageError::DataDirectoryUnavailable)?;
        Ok(project_dirs.data_local_dir().to_path_buf())
    }

    /// Opens the operating system's standard per-user `Chat2DB` data directory.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform has no data directory, another
    /// process owns it, `SQLite` is damaged or incompatible, or recovery fails.
    pub fn open_default(vault: Arc<dyn SecretVault>) -> Result<Self, StorageError> {
        Self::open(Self::default_data_dir()?, vault)
    }

    /// Opens and exclusively owns a `Chat2DB` data directory.
    ///
    /// Startup configures `SQLite` WAL, foreign keys, full synchronous writes,
    /// explicit migrations, integrity checking, and result-file recovery.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory cannot be secured, is already open,
    /// `SQLite` cannot be initialized, or retained-result recovery fails.
    pub fn open(
        data_dir: impl AsRef<Path>,
        vault: Arc<dyn SecretVault>,
    ) -> Result<Self, StorageError> {
        Self::open_with_options(data_dir, StorageOptions::default(), vault)
    }

    /// Opens a data directory with explicit retained-result limits.
    ///
    /// # Errors
    ///
    /// Returns the same failures as [`Storage::open`], plus invalid resource
    /// limits.
    pub fn open_with_options(
        data_dir: impl AsRef<Path>,
        options: StorageOptions,
        vault: Arc<dyn SecretVault>,
    ) -> Result<Self, StorageError> {
        if options.max_retained_bytes == 0 {
            return Err(StorageError::InvalidResult(
                "retained-result quota must be greater than zero",
            ));
        }
        let data_dir = data_dir.as_ref().to_path_buf();
        fs::create_dir_all(&data_dir).map_err(|error| StorageError::io(&data_dir, error))?;
        secure_directory(&data_dir)?;

        let lock_path = data_dir.join(LOCK_FILE);
        let directory_lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| StorageError::io(&lock_path, error))?;
        secure_file(&lock_path)?;
        if let Err(error) = FileExt::try_lock_exclusive(&directory_lock) {
            if lock_is_contended(&error) {
                return Err(StorageError::AlreadyOpen(data_dir));
            }
            return Err(StorageError::io(lock_path, error));
        }

        let results_dir = data_dir.join(RESULTS_DIRECTORY);
        fs::create_dir_all(&results_dir).map_err(|error| StorageError::io(&results_dir, error))?;
        secure_directory(&results_dir)?;

        let database_path = data_dir.join(DATABASE_FILE);
        let inner = Arc::new(StorageInner {
            data_dir,
            database_path,
            results_dir,
            secret_gate: Mutex::new(()),
            result_gate: Mutex::new(HashSet::new()),
            max_retained_bytes: options.max_retained_bytes,
            vault,
            startup_report: OnceLock::new(),
            _directory_lock: directory_lock,
        });
        let storage = Self { inner };

        let connection = storage.connection()?;
        verify_integrity(&connection)?;
        migrate(&connection)?;
        verify_integrity(&connection)?;
        secure_file(&storage.inner.database_path)?;
        drop(connection);

        storage
            .inner
            .vault
            .probe()
            .map_err(|source| StorageError::SecretVault {
                operation: "probe",
                source,
            })?;

        let timestamp = now_millis()?;
        let recovery = storage.recover_at(timestamp)?;
        let agents = storage.recover_agents_at(timestamp)?;
        let secrets = storage.reconcile_secrets()?;
        storage
            .inner
            .startup_report
            .set(StartupReport {
                results: recovery,
                secrets,
                agents,
            })
            .map_err(|_| {
                StorageError::Integrity("startup recovery initialized twice".to_owned())
            })?;
        Ok(storage)
    }

    /// Returns the process-owned data directory.
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.inner.data_dir
    }

    /// Returns the recovery actions completed before this handle was exposed.
    ///
    /// # Panics
    ///
    /// Panics only if an internal constructor exposes `Storage` before startup
    /// recovery, which the public constructors do not permit.
    #[must_use]
    pub fn startup_report(&self) -> &StartupReport {
        self.inner
            .startup_report
            .get()
            .expect("startup report is set before Storage::open returns")
    }

    pub(crate) fn connection(&self) -> Result<Connection, StorageError> {
        let connection = Connection::open_with_flags(
            &self.inner.database_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        configure_connection(&connection)?;
        Ok(connection)
    }

    pub(crate) fn lock_secrets(&self) -> Result<MutexGuard<'_, ()>, StorageError> {
        self.inner
            .secret_gate
            .lock()
            .map_err(|_| StorageError::Integrity("storage secret lock is poisoned".to_owned()))
    }

    pub(crate) fn lock_results(&self) -> Result<MutexGuard<'_, HashSet<String>>, StorageError> {
        self.inner
            .result_gate
            .lock()
            .map_err(|_| StorageError::Integrity("retained-result lock is poisoned".to_owned()))
    }
}

fn configure_connection(connection: &Connection) -> Result<(), StorageError> {
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    let foreign_keys: i64 =
        connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    let journal_mode: String =
        connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    let synchronous: i64 = connection.pragma_query_value(None, "synchronous", |row| row.get(0))?;
    if foreign_keys != 1 || !journal_mode.eq_ignore_ascii_case("wal") || synchronous != 2 {
        return Err(StorageError::Integrity(
            "required SQLite durability settings were not applied".to_owned(),
        ));
    }
    Ok(())
}

fn migrate(connection: &Connection) -> Result<(), StorageError> {
    let mut version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema {
            found: version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    if version == 0 {
        apply_migration(connection, include_str!("../migrations/001_initial.sql"))?;
        version = 1;
    }
    if version == 1 {
        apply_migration(connection, include_str!("../migrations/002_agent.sql"))?;
    }
    Ok(())
}

fn apply_migration(connection: &Connection, sql: &str) -> Result<(), StorageError> {
    if let Err(error) = connection.execute_batch(sql) {
        let _ = connection.execute_batch("ROLLBACK");
        return Err(error.into());
    }
    Ok(())
}

fn lock_is_contended(error: &io::Error) -> bool {
    let expected = fs2::lock_contended_error();
    match (error.raw_os_error(), expected.raw_os_error()) {
        (Some(actual), Some(expected)) => actual == expected,
        _ => error.kind() == expected.kind(),
    }
}

fn verify_integrity(connection: &Connection) -> Result<(), StorageError> {
    let result: String = connection.query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))?;
    if result != "ok" {
        return Err(StorageError::Integrity(result));
    }
    let mut foreign_key_check = connection.prepare("PRAGMA foreign_key_check")?;
    if foreign_key_check.query([])?.next()?.is_some() {
        return Err(StorageError::Integrity(
            "SQLite foreign key check failed".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn now_millis() -> Result<i64, StorageError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StorageError::NumericRange("system clock is before the Unix epoch"))?;
    i64::try_from(duration.as_millis())
        .map_err(|_| StorageError::NumericRange("timestamp milliseconds"))
}

#[cfg(unix)]
pub(crate) fn secure_directory(path: &Path) -> Result<(), StorageError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| StorageError::io(path, error))
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn secure_directory(_path: &Path) -> Result<(), StorageError> {
    Ok(())
}

#[cfg(unix)]
pub(crate) fn secure_file(path: &Path) -> Result<(), StorageError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| StorageError::io(path, error))
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn secure_file(_path: &Path) -> Result<(), StorageError> {
    Ok(())
}

#[cfg(unix)]
pub(crate) fn sync_directory(path: &Path) -> Result<(), StorageError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| StorageError::io(path, error))
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn sync_directory(_path: &Path) -> Result<(), StorageError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc};

    use rusqlite::Connection;
    use tempfile::TempDir;

    use super::{
        DATABASE_FILE, RecoveryReport, SecretRef, SecretValue, SecretVault, SecretVaultError,
        Storage,
    };
    use crate::StorageError;

    #[derive(Debug)]
    struct TestVault;

    impl SecretVault for TestVault {
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

    fn vault() -> Arc<dyn SecretVault> {
        Arc::new(TestVault)
    }

    #[test]
    fn migration_and_required_pragmas_are_idempotent() {
        let directory = TempDir::new().expect("temp dir");
        let storage = Storage::open(directory.path(), vault()).expect("storage opens");
        let connection = storage.connection().expect("connection opens");
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version reads");
        let foreign_keys: i64 = connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .expect("foreign keys read");
        let journal_mode: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .expect("journal mode reads");
        let synchronous: i64 = connection
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .expect("synchronous reads");
        assert_eq!(version, 2);
        assert_eq!(foreign_keys, 1);
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
        assert_eq!(synchronous, 2);
        drop(connection);
        drop(storage);

        let reopened = Storage::open(directory.path(), vault()).expect("storage reopens");
        assert_eq!(reopened.startup_report().results, RecoveryReport::default());
    }

    #[test]
    fn newer_schema_is_rejected_without_mutation() {
        let directory = TempDir::new().expect("temp dir");
        let storage = Storage::open(directory.path(), vault()).expect("storage opens");
        drop(storage);
        let database = directory.path().join(DATABASE_FILE);
        Connection::open(&database)
            .expect("database opens")
            .execute_batch("PRAGMA user_version = 3")
            .expect("test version updates");

        let error = Storage::open(directory.path(), vault()).expect_err("newer schema must fail");
        assert!(matches!(
            error,
            StorageError::UnsupportedSchema {
                found: 3,
                supported: 2
            }
        ));
        let version: i64 = Connection::open(database)
            .expect("database opens")
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version reads");
        assert_eq!(version, 3);
    }

    #[test]
    fn version_one_upgrades_atomically_and_preserves_existing_state() {
        let directory = TempDir::new().expect("temp dir");
        let database = directory.path().join(DATABASE_FILE);
        let connection = Connection::open(&database).expect("database opens");
        connection
            .execute_batch(include_str!("../migrations/001_initial.sql"))
            .expect("version one schema creates");
        connection
            .execute(
                "INSERT INTO datasources (
                    id, name, driver_id, revision, created_at_ms, updated_at_ms
                 ) VALUES ('existing', 'Existing', 'driver', 1, 1, 1)",
                [],
            )
            .expect("version one state inserts");
        drop(connection);

        let storage = Storage::open(directory.path(), vault()).expect("version one upgrades");
        let connection = storage.connection().expect("connection opens");
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version reads");
        let provider_table: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'provider_profiles'",
                [],
                |row| row.get(0),
            )
            .expect("provider table count reads");
        assert_eq!(version, 2);
        assert_eq!(provider_table, 1);
        assert!(
            storage
                .get_datasource("existing")
                .expect("datasource reads")
                .is_some()
        );
    }

    #[test]
    fn failed_version_two_upgrade_rolls_back_every_new_agent_table() {
        let directory = TempDir::new().expect("temp dir");
        let database = directory.path().join(DATABASE_FILE);
        let connection = Connection::open(&database).expect("database opens");
        connection
            .execute_batch(include_str!("../migrations/001_initial.sql"))
            .expect("version one schema creates");
        connection
            .execute_batch("CREATE TABLE agent_runs (sentinel TEXT)")
            .expect("conflicting table creates");
        drop(connection);

        Storage::open(directory.path(), vault()).expect_err("version two upgrade must fail");
        let connection = Connection::open(database).expect("database reopens");
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version reads");
        let partial_tables: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN (
                    'provider_profiles', 'agent_sessions', 'agent_messages'
                 )",
                [],
                |row| row.get(0),
            )
            .expect("partial table count reads");
        let sentinel_columns: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('agent_runs') WHERE name = 'sentinel'",
                [],
                |row| row.get(0),
            )
            .expect("sentinel column count reads");
        assert_eq!(version, 1);
        assert_eq!(partial_tables, 0);
        assert_eq!(sentinel_columns, 1);
    }

    #[test]
    fn failed_migration_rolls_back_all_prior_statements() {
        let directory = TempDir::new().expect("temp dir");
        let database = directory.path().join(DATABASE_FILE);
        Connection::open(&database)
            .expect("database opens")
            .execute_batch("CREATE TABLE retained_results (sentinel TEXT)")
            .expect("conflicting table creates");

        Storage::open(directory.path(), vault()).expect_err("migration must fail");
        let connection = Connection::open(database).expect("database reopens");
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version reads");
        let created_tables: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN ('datasources', 'secret_cleanup_queue')",
                [],
                |row| row.get(0),
            )
            .expect("created tables count reads");
        let sentinel_tables: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'retained_results'",
                [],
                |row| row.get(0),
            )
            .expect("sentinel table count reads");
        assert_eq!(version, 0);
        assert_eq!(created_tables, 0);
        assert_eq!(sentinel_tables, 1);
    }

    #[test]
    fn corrupt_database_fails_closed_before_recovery() {
        let directory = TempDir::new().expect("temp dir");
        fs::write(
            directory.path().join(DATABASE_FILE),
            b"not-a-sqlite-database",
        )
        .expect("corrupt database writes");

        let error = Storage::open(directory.path(), vault()).expect_err("corruption must fail");
        assert!(matches!(
            error,
            StorageError::Sqlite(_) | StorageError::Integrity(_)
        ));
    }

    #[test]
    fn unavailable_secret_vault_prevents_storage_readiness() {
        #[derive(Debug)]
        struct UnavailableVault;

        impl SecretVault for UnavailableVault {
            fn probe(&self) -> Result<(), SecretVaultError> {
                Err(SecretVaultError::Unavailable)
            }

            fn create(
                &self,
                _reference: &SecretRef,
                _value: &SecretValue,
            ) -> Result<(), SecretVaultError> {
                Err(SecretVaultError::Unavailable)
            }

            fn get(&self, _reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
                Err(SecretVaultError::Unavailable)
            }

            fn delete(&self, _reference: &SecretRef) -> Result<(), SecretVaultError> {
                Err(SecretVaultError::Unavailable)
            }
        }

        let directory = TempDir::new().expect("temp dir");
        let error = Storage::open(directory.path(), Arc::new(UnavailableVault))
            .expect_err("unavailable vault must prevent readiness");
        assert!(matches!(
            error,
            StorageError::SecretVault {
                operation: "probe",
                source: SecretVaultError::Unavailable,
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn data_directory_and_database_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TempDir::new().expect("temp dir");
        let storage = Storage::open(directory.path(), vault()).expect("storage opens");
        let directory_mode = fs::metadata(directory.path())
            .expect("directory metadata")
            .permissions()
            .mode()
            & 0o777;
        let database_mode = fs::metadata(directory.path().join(DATABASE_FILE))
            .expect("database metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(directory_mode, 0o700);
        assert_eq!(database_mode, 0o600);
        drop(storage);
    }
}
