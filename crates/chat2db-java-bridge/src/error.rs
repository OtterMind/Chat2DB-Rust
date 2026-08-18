use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
    io,
};

use chat2db_engine_protocol::{FrameError, wire};
use thiserror::Error;

use crate::SessionState;

/// One JDBC exception in the causal chain returned by the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatabaseErrorCause {
    pub class_name: String,
    pub message: String,
    pub sql_state: Option<String>,
    pub vendor_code: Option<i32>,
}

/// Structured JDBC diagnostics attached to a database-category engine error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatabaseErrorDetail {
    pub sql_state: Option<String>,
    pub vendor_code: Option<i32>,
    pub constraint_name: Option<String>,
    pub statement_position: Option<u32>,
    pub causes: Vec<DatabaseErrorCause>,
}

/// What can safely be said about a request when local delivery fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryOutcome {
    /// The request was rejected before it could enter the process writer.
    NotSent,
    /// The request entered the process path; whether the engine acted is unknown.
    Unknown,
}

impl Display for DeliveryOutcome {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NotSent => "not sent",
            Self::Unknown => "unknown",
        })
    }
}

/// Typed error returned by the Java engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteEngineError {
    pub code: String,
    pub message: String,
    pub category: wire::ErrorCategory,
    pub retryable: bool,
    pub fatal: bool,
    pub outcome: wire::OperationOutcome,
    pub metadata: HashMap<String, String>,
    pub database_error: Option<DatabaseErrorDetail>,
    pub session_state: Option<SessionState>,
}

impl Display for RemoteEngineError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RemoteEngineError {}

impl TryFrom<wire::EngineError> for RemoteEngineError {
    type Error = String;

    fn try_from(error: wire::EngineError) -> Result<Self, Self::Error> {
        let category = match wire::ErrorCategory::try_from(error.category) {
            Ok(wire::ErrorCategory::Unspecified) | Err(_) => {
                return Err(format!("unknown engine error category {}", error.category));
            }
            Ok(category) => category,
        };
        let outcome = match wire::OperationOutcome::try_from(error.outcome) {
            Ok(wire::OperationOutcome::Unspecified) | Err(_) => {
                return Err(format!(
                    "unknown engine operation outcome {}",
                    error.outcome
                ));
            }
            Ok(outcome) => outcome,
        };
        let session_state = error
            .session_state
            .map(SessionState::from_wire)
            .transpose()?;

        Ok(Self {
            code: error.code,
            message: error.message,
            category,
            retryable: error.retryable,
            fatal: error.fatal,
            outcome,
            metadata: error.metadata,
            database_error: error.database_error.map(DatabaseErrorDetail::from),
            session_state,
        })
    }
}

impl From<wire::DatabaseErrorDetail> for DatabaseErrorDetail {
    fn from(error: wire::DatabaseErrorDetail) -> Self {
        Self {
            sql_state: error.sql_state,
            vendor_code: error.vendor_code,
            constraint_name: error.constraint_name,
            statement_position: error.statement_position,
            causes: error
                .causes
                .into_iter()
                .map(DatabaseErrorCause::from)
                .collect(),
        }
    }
}

impl From<wire::DatabaseErrorCause> for DatabaseErrorCause {
    fn from(error: wire::DatabaseErrorCause) -> Self {
        Self {
            class_name: error.class_name,
            message: error.message,
            sql_state: error.sql_state,
            vendor_code: error.vendor_code,
        }
    }
}

/// Failure to start, communicate with, or stop the compatibility engine.
#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("invalid engine configuration: {0}")]
    InvalidConfig(String),
    #[error("failed to spawn compatibility engine: {0}")]
    Spawn(#[source] io::Error),
    #[error("spawned compatibility engine did not expose {0}")]
    MissingPipe(&'static str),
    #[error("compatibility engine command channel closed; request outcome is {outcome}")]
    CommandChannelClosed { outcome: DeliveryOutcome },
    #[error("invalid JDBC request: {0}")]
    InvalidRequest(String),
    #[error("failed to prepare driver artifact {path}: {source}")]
    DriverArtifact {
        path: std::path::PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("unable to {operation} Community classpath artifact {path}: {source}")]
    CommunityArtifact {
        operation: &'static str,
        path: std::path::PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("unable to {operation} JDBC snapshot directory {path}: {source}")]
    DriverSnapshotDirectory {
        operation: &'static str,
        path: std::path::PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("driver artifact path is not valid UTF-8: {0}")]
    NonUtf8DriverArtifact(std::path::PathBuf),
    #[error("JDBC handle belongs to another engine generation or instance: {0}")]
    StaleHandle(String),
    #[error("compatibility engine is {state}; request was not sent")]
    NotReady { state: &'static str },
    #[error("compatibility engine startup timed out")]
    StartupTimeout,
    #[error("request {request_id} timed out; request outcome is {outcome}")]
    RequestTimeout {
        request_id: String,
        outcome: DeliveryOutcome,
    },
    #[error("compatibility engine became unavailable: {message}; request outcome is {outcome}")]
    ProcessUnavailable {
        message: String,
        outcome: DeliveryOutcome,
    },
    #[error("compatibility protocol violation: {0}")]
    Protocol(String),
    #[error("compatibility engine selected unsupported protocol {major}.{minor}")]
    UnsupportedVersion { major: u32, minor: u32 },
    #[error("compatibility engine is missing required capability {0}")]
    MissingCapability(String),
    #[error("compatibility engine returned an invalid handshake: {0}")]
    InvalidHandshake(String),
    #[error("unexpected compatibility-engine response: {0}")]
    UnexpectedResponse(&'static str),
    #[error("compatibility engine rejected the request: {0}")]
    Remote(Box<RemoteEngineError>),
    #[error("compatibility engine shutdown timed out and the process was killed")]
    ShutdownTimeout,
    #[error("compatibility engine supervisor task failed: {0}")]
    SupervisorTask(String),
    #[error(
        "failed to terminate and reap compatibility engine; retained generation snapshot {retained_snapshot}: {message}"
    )]
    ProcessCleanup {
        retained_snapshot: std::path::PathBuf,
        message: String,
    },
    #[error("{primary}; cleanup after that failure also failed: {cleanup}")]
    CleanupAfterFailure {
        #[source]
        primary: Box<BridgeError>,
        cleanup: Box<BridgeError>,
    },
    #[error(transparent)]
    Frame(#[from] FrameError),
}

impl From<RemoteEngineError> for BridgeError {
    fn from(error: RemoteEngineError) -> Self {
        Self::Remote(Box::new(error))
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PendingFailure {
    Unavailable {
        message: String,
        outcome: DeliveryOutcome,
    },
    Timeout {
        request_id: String,
        outcome: DeliveryOutcome,
    },
    InvalidRequest(String),
    Protocol(String),
    Remote(Box<wire::EngineError>),
}

impl PendingFailure {
    pub(crate) fn into_bridge_error(self) -> BridgeError {
        match self {
            Self::Unavailable { message, outcome } => {
                BridgeError::ProcessUnavailable { message, outcome }
            }
            Self::Timeout {
                request_id,
                outcome,
            } => BridgeError::RequestTimeout {
                request_id,
                outcome,
            },
            Self::InvalidRequest(message) => BridgeError::InvalidRequest(message),
            Self::Protocol(message) => BridgeError::Protocol(message),
            Self::Remote(error) => match RemoteEngineError::try_from(*error) {
                Ok(error) => error.into(),
                Err(message) => BridgeError::Protocol(message),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_wire_error() -> wire::EngineError {
        wire::EngineError {
            code: "database.test".to_owned(),
            message: "test failure".to_owned(),
            category: wire::ErrorCategory::Database as i32,
            retryable: false,
            fatal: false,
            outcome: wire::OperationOutcome::KnownFailed as i32,
            metadata: HashMap::new(),
            database_error: None,
            session_state: Some(wire::SessionState::RollbackRequired as i32),
        }
    }

    #[test]
    fn remote_error_strictly_decodes_wire_enums() {
        let decoded = RemoteEngineError::try_from(valid_wire_error())
            .expect("known wire enum values must decode");

        assert_eq!(decoded.category, wire::ErrorCategory::Database);
        assert_eq!(decoded.outcome, wire::OperationOutcome::KnownFailed);
        assert_eq!(decoded.session_state, Some(SessionState::RollbackRequired));
    }

    #[test]
    fn remote_error_rejects_unknown_category() {
        let mut error = valid_wire_error();
        error.category = i32::MAX;

        let message = RemoteEngineError::try_from(error)
            .expect_err("unknown categories must be protocol errors");
        assert!(message.contains("unknown engine error category"));
    }

    #[test]
    fn remote_error_rejects_unspecified_category() {
        let mut error = valid_wire_error();
        error.category = wire::ErrorCategory::Unspecified as i32;

        let message = RemoteEngineError::try_from(error)
            .expect_err("unspecified categories must be protocol errors");
        assert!(message.contains("unknown engine error category"));
    }

    #[test]
    fn remote_error_rejects_unknown_outcome() {
        let mut error = valid_wire_error();
        error.outcome = i32::MAX;

        let message = RemoteEngineError::try_from(error)
            .expect_err("unknown operation outcomes must be protocol errors");
        assert!(message.contains("unknown engine operation outcome"));
    }

    #[test]
    fn remote_error_rejects_unspecified_outcome() {
        let mut error = valid_wire_error();
        error.outcome = wire::OperationOutcome::Unspecified as i32;

        let message = RemoteEngineError::try_from(error)
            .expect_err("unspecified outcomes must be protocol errors");
        assert!(message.contains("unknown engine operation outcome"));
    }

    #[test]
    fn remote_error_rejects_unknown_session_state() {
        let mut error = valid_wire_error();
        error.session_state = Some(i32::MAX);

        let message = RemoteEngineError::try_from(error)
            .expect_err("unknown session states must be protocol errors");
        assert!(message.contains("unknown JDBC session state"));
    }

    #[test]
    fn pending_remote_decode_failure_propagates_as_protocol_error() {
        let mut error = valid_wire_error();
        error.category = i32::MAX;

        let decoded = PendingFailure::Remote(Box::new(error)).into_bridge_error();
        assert!(matches!(decoded, BridgeError::Protocol(_)));
    }
}
