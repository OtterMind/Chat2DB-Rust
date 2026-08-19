use std::sync::Mutex;

use chat2db_engine_protocol::wire::{self, ProtocolVersion};

/// JDBC session state reported after lifecycle and transaction operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    AutoCommit,
    TransactionActive,
    RollbackRequired,
    Broken,
    Closed,
}

impl SessionState {
    pub(crate) fn from_wire(value: i32) -> Result<Self, String> {
        use chat2db_engine_protocol::wire;

        match wire::SessionState::try_from(value) {
            Ok(wire::SessionState::AutoCommit) => Ok(Self::AutoCommit),
            Ok(wire::SessionState::TransactionActive) => Ok(Self::TransactionActive),
            Ok(wire::SessionState::RollbackRequired) => Ok(Self::RollbackRequired),
            Ok(wire::SessionState::Broken) => Ok(Self::Broken),
            Ok(wire::SessionState::Closed) => Ok(Self::Closed),
            Ok(wire::SessionState::Unspecified) | Err(_) => {
                Err(format!("unknown JDBC session state {value}"))
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct SessionStateCell(Mutex<SessionState>);

impl SessionStateCell {
    pub(crate) const fn new(state: SessionState) -> Self {
        Self(Mutex::new(state))
    }

    pub(crate) fn get(&self) -> SessionState {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn set(&self, state: SessionState) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
    }
}

/// Engine details proven by a successful protocol handshake.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineIdentity {
    pub name: String,
    pub version: String,
    pub instance_id: String,
    pub protocol_version: ProtocolVersion,
    pub capabilities: Vec<String>,
    pub max_frame_bytes: u32,
}

/// Bounded diagnostic bytes captured from the engine's stderr stream.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StderrSnapshot {
    pub bytes: Vec<u8>,
    pub total_bytes: u64,
    pub truncated: bool,
}

impl StderrSnapshot {
    /// Returns a replacement-character-safe diagnostic rendering.
    #[must_use]
    pub fn to_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

/// Reaped operating-system process status plus its bounded diagnostic tail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessExit {
    pub code: Option<i32>,
    pub success: bool,
    pub stderr: StderrSnapshot,
}

/// Observable lifecycle of one engine generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineState {
    Starting {
        generation: u64,
    },
    Handshaking {
        generation: u64,
    },
    Ready {
        generation: u64,
        identity: EngineIdentity,
    },
    Stopping {
        generation: u64,
    },
    Stopped {
        generation: u64,
        exit: ProcessExit,
    },
    Failed {
        generation: u64,
        reason: String,
        exit: ProcessExit,
    },
    Crashed {
        generation: u64,
        exit: ProcessExit,
    },
}

impl EngineState {
    #[must_use]
    pub const fn generation(&self) -> u64 {
        match self {
            Self::Starting { generation }
            | Self::Handshaking { generation }
            | Self::Ready { generation, .. }
            | Self::Stopping { generation }
            | Self::Stopped { generation, .. }
            | Self::Failed { generation, .. }
            | Self::Crashed { generation, .. } => *generation,
        }
    }

    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Stopped { .. } | Self::Failed { .. } | Self::Crashed { .. }
        )
    }

    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Starting { .. } => "starting",
            Self::Handshaking { .. } => "handshaking",
            Self::Ready { .. } => "ready",
            Self::Stopping { .. } => "stopping",
            Self::Stopped { .. } => "stopped",
            Self::Failed { .. } => "failed",
            Self::Crashed { .. } => "crashed",
        }
    }
}

/// Successful ping response from the current engine generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PingReply {
    pub nonce: u64,
    pub uptime_millis: u64,
}

/// Java-owned work that must be absent before an engine generation can park.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuiescenceSnapshot {
    pub active_sessions: u32,
    pub active_operations: u32,
    pub active_control_tasks: u32,
    pub queued_control_tasks: u32,
    pub pending_cancellations: u32,
    pub pending_outbound_frames: u32,
}

impl From<wire::QuiescenceSnapshot> for QuiescenceSnapshot {
    fn from(snapshot: wire::QuiescenceSnapshot) -> Self {
        Self {
            active_sessions: snapshot.active_sessions,
            active_operations: snapshot.active_operations,
            active_control_tasks: snapshot.active_control_tasks,
            queued_control_tasks: snapshot.queued_control_tasks,
            pending_cancellations: snapshot.pending_cancellations,
            pending_outbound_frames: snapshot.pending_outbound_frames,
        }
    }
}
