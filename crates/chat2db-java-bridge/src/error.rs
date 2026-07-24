use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
    io,
};

use chat2db_engine_protocol::{FrameError, wire};
use thiserror::Error;

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
}

impl Display for RemoteEngineError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RemoteEngineError {}

impl From<wire::EngineError> for RemoteEngineError {
    fn from(error: wire::EngineError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            category: wire::ErrorCategory::try_from(error.category)
                .unwrap_or(wire::ErrorCategory::Unspecified),
            retryable: error.retryable,
            fatal: error.fatal,
            outcome: wire::OperationOutcome::try_from(error.outcome)
                .unwrap_or(wire::OperationOutcome::Unspecified),
            metadata: error.metadata,
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
    Remote(#[from] RemoteEngineError),
    #[error("compatibility engine shutdown timed out and the process was killed")]
    ShutdownTimeout,
    #[error("compatibility engine supervisor task failed: {0}")]
    SupervisorTask(String),
    #[error(transparent)]
    Frame(#[from] FrameError),
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
        }
    }
}
