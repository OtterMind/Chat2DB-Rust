use std::fmt;

use thiserror::Error;

use crate::{ExecutionOutcome, ProviderKind};

/// Invalid local provider or runtime configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("provider base URL is invalid: {0}")]
    InvalidBaseUrl(#[from] url::ParseError),
    #[error("provider base URL must use http or https")]
    UnsupportedBaseUrlScheme,
    #[error("provider base URL must not contain username or password credentials")]
    BaseUrlCredentials,
    #[error("provider base URL must contain a host")]
    BaseUrlHost,
    #[error("provider base URL must not contain a query")]
    BaseUrlQuery,
    #[error("provider base URL must not contain a fragment")]
    BaseUrlFragment,
    #[error("provider model must not be empty")]
    EmptyModel,
    #[error("API key must not be empty")]
    EmptyApiKey,
    #[error("invalid HTTP header name: {0}")]
    InvalidHeaderName(String),
    #[error("invalid HTTP header value for {0}")]
    InvalidHeaderValue(String),
    #[error("HTTP timeout must be greater than zero")]
    ZeroTimeout,
    #[error("maximum output tokens must be greater than zero")]
    ZeroOutputTokens,
    #[error("failed to build provider HTTP client: {0}")]
    HttpClient(String),
    #[error("context budget must have a positive byte limit and a threshold from 1 to 100")]
    InvalidContextBudget,
}

/// A normalized transport, remote, or streaming protocol failure.
#[derive(Error)]
pub enum ProviderError {
    #[error("provider request was cancelled")]
    Cancelled,
    #[error("{provider:?} transport failed")]
    Transport {
        provider: ProviderKind,
        message: String,
    },
    #[error("{provider:?} returned HTTP {status}")]
    HttpStatus { provider: ProviderKind, status: u16 },
    #[error("{provider:?} response exceeded the {limit}-byte limit")]
    ResponseTooLarge {
        provider: ProviderKind,
        limit: usize,
    },
    #[error("{provider:?} rejected the request ({code})")]
    Remote {
        provider: ProviderKind,
        code: String,
        message: String,
    },
    #[error("{provider:?} stream violated the protocol: {message}")]
    Protocol {
        provider: ProviderKind,
        message: String,
    },
    #[error("failed to serialize the {provider:?} request: {message}")]
    Serialization {
        provider: ProviderKind,
        message: String,
    },
}

impl fmt::Debug for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("Cancelled"),
            Self::Transport { provider, message } => formatter
                .debug_struct("Transport")
                .field("provider", provider)
                .field("message_bytes", &message.len())
                .finish(),
            Self::HttpStatus { provider, status } => formatter
                .debug_struct("HttpStatus")
                .field("provider", provider)
                .field("status", status)
                .finish(),
            Self::ResponseTooLarge { provider, limit } => formatter
                .debug_struct("ResponseTooLarge")
                .field("provider", provider)
                .field("limit", limit)
                .finish(),
            Self::Remote {
                provider,
                code,
                message,
            } => formatter
                .debug_struct("Remote")
                .field("provider", provider)
                .field("code", code)
                .field("message_bytes", &message.len())
                .finish(),
            Self::Protocol { provider, message } => formatter
                .debug_struct("Protocol")
                .field("provider", provider)
                .field("message_bytes", &message.len())
                .finish(),
            Self::Serialization { provider, message } => formatter
                .debug_struct("Serialization")
                .field("provider", provider)
                .field("message_bytes", &message.len())
                .finish(),
        }
    }
}

impl ProviderError {
    pub(crate) fn transport(provider: ProviderKind, error: impl std::fmt::Display) -> Self {
        Self::Transport {
            provider,
            message: error.to_string(),
        }
    }

    pub(crate) fn protocol(provider: ProviderKind, message: impl Into<String>) -> Self {
        Self::Protocol {
            provider,
            message: message.into(),
        }
    }

    pub(crate) fn serialization(provider: ProviderKind, error: impl std::fmt::Display) -> Self {
        Self::Serialization {
            provider,
            message: error.to_string(),
        }
    }
}

/// Why a bounded tool output could not be constructed.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ToolOutputError {
    #[error("tool output content exceeds {limit} bytes")]
    ContentTooLarge { limit: usize },
    #[error("tool output handle identifier must not be empty")]
    EmptyHandle,
    #[error("tool output handle metadata exceeds {limit} bytes")]
    MetadataTooLarge { limit: usize },
    #[error("tool output must contain content, a handle, or both")]
    Empty,
}

/// A tool failure reported by a [`crate::ToolExecutor`].
#[derive(Clone, Error, PartialEq, Eq)]
#[error("tool execution failed ({code}, {outcome:?})")]
pub struct ToolExecutionError {
    code: String,
    message: String,
    outcome: ExecutionOutcome,
}

impl fmt::Debug for ToolExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolExecutionError")
            .field("code", &self.code)
            .field("message", &"[REDACTED]")
            .field("outcome", &self.outcome)
            .finish()
    }
}

impl ToolExecutionError {
    #[must_use]
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        outcome: ExecutionOutcome,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            outcome,
        }
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub const fn outcome(&self) -> ExecutionOutcome {
        self.outcome
    }
}

/// A bounded agent run failure.
#[derive(Error)]
pub enum AgentError {
    #[error("agent run was cancelled")]
    Cancelled,
    #[error("agent run exceeded its total duration")]
    DeadlineExceeded,
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("unknown tool requested by the model: {0}")]
    UnknownTool(String),
    #[error("duplicate tool call identifier: {0}")]
    DuplicateToolCall(String),
    #[error("tool call {call_id} has invalid arguments: {message}")]
    InvalidToolArguments { call_id: String, message: String },
    #[error("tool call {call_id} arguments exceed {limit} bytes")]
    ToolArgumentsTooLarge { call_id: String, limit: usize },
    #[error("model round {round} text exceeded the {limit}-byte limit")]
    ModelTextTooLarge { round: usize, limit: usize },
    #[error("model round {round} exceeded the {limit} tool-call limit")]
    RoundToolLimit { round: usize, limit: usize },
    #[error("agent run exceeded the {0} total tool-call limit")]
    TotalToolLimit(usize),
    #[error("agent run exhausted the {0} model-round limit")]
    ModelRoundLimit(usize),
    #[error("provider stream ended without a completion event")]
    IncompleteProviderStream,
    #[error("provider emitted more than one completion event")]
    DuplicateProviderCompletion,
    #[error("provider completion reason is inconsistent with its tool calls")]
    InconsistentProviderCompletion,
    #[error("tool {tool} failed: {source}")]
    Tool {
        tool: String,
        #[source]
        source: ToolExecutionError,
    },
    #[error("context still exceeds its budget after preserving mandatory turns")]
    ContextBudgetExceeded,
    #[error("invalid agent input: {0}")]
    InvalidInput(String),
}

impl fmt::Debug for AgentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("Cancelled"),
            Self::DeadlineExceeded => formatter.write_str("DeadlineExceeded"),
            Self::Provider(source) => formatter.debug_tuple("Provider").field(source).finish(),
            Self::UnknownTool(_) => formatter
                .debug_tuple("UnknownTool")
                .field(&"[REDACTED]")
                .finish(),
            Self::DuplicateToolCall(_) => formatter
                .debug_tuple("DuplicateToolCall")
                .field(&"[REDACTED]")
                .finish(),
            Self::InvalidToolArguments {
                call_id: _,
                message,
            } => formatter
                .debug_struct("InvalidToolArguments")
                .field("call_id", &"[REDACTED]")
                .field("message_bytes", &message.len())
                .finish(),
            Self::ToolArgumentsTooLarge { call_id: _, limit } => formatter
                .debug_struct("ToolArgumentsTooLarge")
                .field("call_id", &"[REDACTED]")
                .field("limit", limit)
                .finish(),
            Self::ModelTextTooLarge { round, limit } => formatter
                .debug_struct("ModelTextTooLarge")
                .field("round", round)
                .field("limit", limit)
                .finish(),
            Self::RoundToolLimit { round, limit } => formatter
                .debug_struct("RoundToolLimit")
                .field("round", round)
                .field("limit", limit)
                .finish(),
            Self::TotalToolLimit(limit) => formatter
                .debug_tuple("TotalToolLimit")
                .field(limit)
                .finish(),
            Self::ModelRoundLimit(limit) => formatter
                .debug_tuple("ModelRoundLimit")
                .field(limit)
                .finish(),
            Self::IncompleteProviderStream => formatter.write_str("IncompleteProviderStream"),
            Self::DuplicateProviderCompletion => formatter.write_str("DuplicateProviderCompletion"),
            Self::InconsistentProviderCompletion => {
                formatter.write_str("InconsistentProviderCompletion")
            }
            Self::Tool { tool: _, source } => formatter
                .debug_struct("Tool")
                .field("tool", &"[REDACTED]")
                .field("source", source)
                .finish(),
            Self::ContextBudgetExceeded => formatter.write_str("ContextBudgetExceeded"),
            Self::InvalidInput(_) => formatter
                .debug_tuple("InvalidInput")
                .field(&"[REDACTED]")
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentError, ProviderError, ToolExecutionError};
    use crate::{ExecutionOutcome, ProviderKind};

    #[test]
    fn debug_output_redacts_provider_and_tool_error_messages() {
        const SENTINEL: &str = "PRIVATE_ERROR_SQL_8c344e";

        let provider = ProviderError::Remote {
            provider: ProviderKind::OpenAi,
            code: "remote_error".to_owned(),
            message: SENTINEL.to_owned(),
        };
        let tool = ToolExecutionError::new("tool_error", SENTINEL, ExecutionOutcome::Unknown);
        let debug_outputs = [
            format!("{provider:?}"),
            format!("{tool:?}"),
            format!(
                "{:?}",
                AgentError::Provider(ProviderError::Protocol {
                    provider: ProviderKind::Gemini,
                    message: SENTINEL.to_owned(),
                })
            ),
            format!(
                "{:?}",
                AgentError::Tool {
                    tool: "query".to_owned(),
                    source: tool,
                }
            ),
            format!("{:?}", AgentError::InvalidInput(SENTINEL.to_owned())),
        ];

        for debug in debug_outputs {
            assert!(!debug.contains(SENTINEL), "sensitive Debug output: {debug}");
        }

        let display_outputs = [
            provider.to_string(),
            ToolExecutionError::new("tool_error", SENTINEL, ExecutionOutcome::Unknown).to_string(),
        ];
        for display in display_outputs {
            assert!(
                !display.contains(SENTINEL),
                "sensitive Display output: {display}"
            );
        }
    }
}
