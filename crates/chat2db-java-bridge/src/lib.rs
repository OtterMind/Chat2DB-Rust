//! Supervised access to the private Java database compatibility engine.

mod command;
mod error;
mod state;
mod stderr_tail;
mod supervisor;

pub use command::EngineCommand;
pub use error::{BridgeError, DeliveryOutcome, RemoteEngineError};
pub use state::{EngineIdentity, EngineState, PingReply, ProcessExit, StderrSnapshot};
pub use supervisor::{EngineClient, EngineConfig, EngineSupervisor};
