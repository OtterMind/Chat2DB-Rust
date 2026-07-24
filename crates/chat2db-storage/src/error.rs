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
    /// The requested provider profile does not exist.
    #[error("provider profile not found: {0}")]
    ProviderNotFound(String),
    /// A provider update lost an optimistic-concurrency race.
    #[error("provider revision conflict for {id}: expected {expected}, actual {actual:?}")]
    ProviderRevisionConflict {
        /// Provider profile id.
        id: String,
        /// Revision supplied by the caller.
        expected: u64,
        /// Current revision, or `None` when deleted.
        actual: Option<u64>,
    },
    /// A provider field violates the durable contract.
    #[error("invalid provider profile: {0}")]
    InvalidProvider(&'static str),
    /// The requested agent session does not exist.
    #[error("agent session not found: {0}")]
    AgentSessionNotFound(String),
    /// A session update lost an optimistic-concurrency race.
    #[error("agent session revision conflict for {id}: expected {expected}, actual {actual:?}")]
    AgentSessionRevisionConflict {
        /// Session id.
        id: String,
        /// Revision supplied by the caller.
        expected: u64,
        /// Current revision, or `None` when deleted.
        actual: Option<u64>,
    },
    /// A session already owns a running or permission-waiting agent run.
    #[error("agent session already has an active run: {0}")]
    AgentSessionBusy(String),
    /// A mutable provider or datasource is bound to an active agent run.
    #[error("{resource} is bound to an active agent run: {id}")]
    AgentDependencyBusy {
        /// Stable resource category.
        resource: &'static str,
        /// Provider or datasource id.
        id: String,
    },
    /// The requested agent run does not exist.
    #[error("agent run not found: {0}")]
    AgentRunNotFound(String),
    /// An agent run lifecycle CAS failed.
    #[error("agent run state conflict for {id}: expected {expected}, actual {actual}")]
    AgentStateConflict {
        /// Run id.
        id: String,
        /// Expected persisted state.
        expected: &'static str,
        /// Actual persisted state.
        actual: &'static str,
    },
    /// Agent state or input violates the durable contract.
    #[error("invalid agent state: {0}")]
    InvalidAgent(&'static str),
    /// A bounded session-message resource limit was reached.
    #[error("agent quota exceeded for {resource}: limit {limit}")]
    AgentQuotaExceeded {
        /// Bounded resource name.
        resource: &'static str,
        /// Configured hard limit.
        limit: u64,
    },
    /// The requested tool permission does not exist.
    #[error("tool permission not found: {0}")]
    PermissionNotFound(String),
    /// A tool-permission decision or consume lost its revision CAS.
    #[error("tool permission revision conflict for {id}: expected {expected}, actual {actual:?}")]
    PermissionRevisionConflict {
        /// Permission id.
        id: String,
        /// Expected revision.
        expected: u64,
        /// Current revision, or `None` when deleted.
        actual: Option<u64>,
    },
    /// A permission cannot authorize execution.
    #[error("tool permission {id} is not executable: {reason}")]
    PermissionNotExecutable {
        /// Permission or owning run id.
        id: String,
        /// Non-sensitive rejection reason.
        reason: &'static str,
    },
    /// A result handle does not exist, is expired, or belongs to another owner.
    #[error("agent result handle not found: {0}")]
    ResultHandleNotFound(String),
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
