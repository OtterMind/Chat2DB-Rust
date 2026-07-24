use chat2db_engine_protocol::wire::ProtocolVersion;

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
