//! A small, provider-neutral agent runtime with strict resource bounds.
//!
//! The crate talks to model vendors directly over `reqwest`. Vendor wire
//! formats stay private to [`providers`]; callers only see normalized messages,
//! provider events, run events, and errors.

mod config;
mod context;
mod error;
mod provider;
mod runner;
mod sse;
mod types;

pub mod providers;

pub use config::{ApiKey, HttpProviderConfig};
pub use context::{ContextCompactor, ContextManager, SummaryError};
pub use error::{AgentError, ConfigError, ProviderError, ToolExecutionError, ToolOutputError};
pub use provider::{Provider, ProviderEventStream};
pub use runner::{AgentLimits, AgentRunner, ToolExecutor};
pub use types::{
    AgentInput, CompactionCause, CompactionEvent, CompactionStrategy, ContextBudget, ContextUsage,
    ExecutionOutcome, MAX_PROVIDER_CONTINUATION_BYTES, MAX_PROVIDER_TOOL_CALL_ID_BYTES,
    MAX_TOOL_ARGUMENT_BYTES, MAX_TOOL_HANDLE_METADATA_BYTES, MAX_TOOL_OUTPUT_CONTENT_BYTES,
    Message, MessageBlock, ProviderContinuation, ProviderContinuationKind,
    ProviderContinuationPlacement, ProviderEvent, ProviderKind, ProviderRequest,
    ProviderToolCallIdentity, Role, RunEvent, RunResult, StopReason, ToolCall, ToolDefinition,
    ToolInvocation, ToolOutput, ToolOutputHandle, ToolResult, Usage,
};
