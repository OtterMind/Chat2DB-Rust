use std::{io, path::PathBuf};

use thiserror::Error;

use crate::SecretVaultError;

/// Failures at the local durable-storage boundary.
#[derive(Debug, Error)]
pub enum StorageError {
    /// A data directory is already owned by another process.
    #[error("Chat2DB data directory is already open: {0}")]
    AlreadyOpen(PathBuf),
    /// The operating system did not expose an application data directory.
    #[error("the operating system did not provide a Chat2DB data directory")]
    DataDirectoryUnavailable,
    /// A filesystem operation failed.
    #[error("storage filesystem operation failed for {path}: {source}")]
    Io {
        /// File or directory involved in the failure.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// `SQLite` rejected an operation.
    #[error("SQLite storage operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The database schema is newer than this binary.
    #[error("storage schema version {found} is newer than supported version {supported}")]
    UnsupportedSchema {
        /// Version found in `SQLite`.
        found: i64,
        /// Latest version implemented by this binary.
        supported: i64,
    },
    /// A retained result uses a format newer than this binary can read.
    #[error("retained result {id} uses format {found}; maximum supported format is {supported}")]
    UnsupportedResultFormat {
        /// Opaque result id.
        id: String,
        /// Format found in `SQLite`.
        found: i64,
        /// Latest format implemented by this binary.
        supported: i64,
    },
    /// `SQLite`'s startup integrity check failed.
    #[error("SQLite integrity check failed: {0}")]
    Integrity(String),
    /// The requested datasource does not exist.
    #[error("datasource not found: {0}")]
    DatasourceNotFound(String),
    /// A datasource update lost an optimistic-concurrency race.
    #[error("datasource revision conflict for {id}: expected {expected}, actual {actual:?}")]
    RevisionConflict {
        /// Datasource id.
        id: String,
        /// Revision supplied by the caller.
        expected: u64,
        /// Current revision, or `None` when the record was deleted.
        actual: Option<u64>,
    },
    /// A datasource field violates the durable contract.
    #[error("invalid datasource: {0}")]
    InvalidDatasource(&'static str),
    /// The external secret vault rejected an operation.
    #[error("secret vault {operation} failed: {source}")]
    SecretVault {
        /// Non-sensitive operation name.
        operation: &'static str,
        /// Vault-provided safe error classification.
        #[source]
        source: SecretVaultError,
    },
    /// A failed datasource mutation also failed to remove its staged secret.
    #[error(
        "datasource mutation failed and staged-secret compensation also failed: {compensation}"
    )]
    SecretCompensation {
        /// Original storage failure, with no secret material.
        primary: Box<Self>,
        /// Failure while deleting the staged secret.
        compensation: SecretVaultError,
    },
    /// The requested retained result does not exist or has expired.
    #[error("retained result not found: {0}")]
    ResultNotFound(String),
    /// A result write or page request violates the storage contract.
    #[error("invalid retained result: {0}")]
    InvalidResult(&'static str),
    /// A completed result file or index is damaged.
    #[error("retained result is corrupt: {result_id}: {reason}")]
    CorruptResult {
        /// Opaque result id.
        result_id: String,
        /// Non-sensitive corruption reason.
        reason: &'static str,
    },
    /// A numeric value cannot be represented by `SQLite` or the local platform.
    #[error("storage numeric value is out of range: {0}")]
    NumericRange(&'static str),
    /// Retained-result data would exceed the configured disk budget.
    #[error(
        "retained-result quota exceeded: requested {requested} bytes with {available} available"
    )]
    QuotaExceeded {
        /// Additional bytes requested by the write.
        requested: u64,
        /// Remaining configured bytes.
        available: u64,
    },
    /// A durable commit may or may not have reached disk and could not be reconciled.
    #[error("storage outcome is unknown for {operation} {id}")]
    OutcomeUnknown {
        /// Non-sensitive operation name.
        operation: &'static str,
        /// Opaque datasource or result id.
        id: String,
    },
}

impl StorageError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
