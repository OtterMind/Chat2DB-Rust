//! Direct HTTP adapters for supported model APIs.

mod anthropic;
mod gemini;
mod openai;

pub use anthropic::AnthropicProvider;
pub use gemini::GeminiProvider;
pub use openai::OpenAiProvider;

use std::collections::HashSet;

use reqwest::{RequestBuilder, Response, header::HeaderValue};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{
    ApiKey, ContextBudget, ContextUsage, Message, ProviderError, ProviderKind, ToolDefinition,
};

const DEFAULT_MAX_SERIALIZED_BYTES: usize = 16 * 1024 * 1024;
const MAX_PROVIDER_TOOL_CALLS_PER_RESPONSE: usize = 8;

fn default_budget(max_tokens: usize) -> ContextBudget {
    ContextBudget::new(Some(max_tokens), DEFAULT_MAX_SERIALIZED_BYTES, 80)
        .expect("static provider context budget must be valid")
}

fn bearer(api_key: &ApiKey) -> Result<HeaderValue, crate::ConfigError> {
    let mut value = HeaderValue::from_str(&format!("Bearer {}", api_key.expose()))
        .map_err(|_| crate::ConfigError::InvalidHeaderValue("authorization".to_owned()))?;
    value.set_sensitive(true);
    Ok(value)
}

fn secret_header(api_key: &ApiKey, name: &'static str) -> Result<HeaderValue, crate::ConfigError> {
    let mut value = HeaderValue::from_str(api_key.expose())
        .map_err(|_| crate::ConfigError::InvalidHeaderValue(name.to_owned()))?;
    value.set_sensitive(true);
    Ok(value)
}

async fn send(
    provider: ProviderKind,
    request: RequestBuilder,
    cancellation: &CancellationToken,
) -> Result<Response, ProviderError> {
    let response = tokio::select! {
        () = cancellation.cancelled() => return Err(ProviderError::Cancelled),
        response = request.send() => response.map_err(|error| ProviderError::transport(provider, error))?,
    };
    if !response.status().is_success() {
        return Err(ProviderError::HttpStatus {
            provider,
            status: response.status().as_u16(),
        });
    }
    Ok(response)
}

fn estimate(
    provider: ProviderKind,
    body: &Value,
    bytes_per_token: usize,
    fixed_message_tokens: usize,
    messages: &[Message],
    tools: &[ToolDefinition],
) -> Result<ContextUsage, ProviderError> {
    let serialized =
        serde_json::to_vec(body).map_err(|error| ProviderError::serialization(provider, error))?;
    let structural = messages
        .len()
        .saturating_mul(fixed_message_tokens)
        .saturating_add(tools.len().saturating_mul(8));
    Ok(ContextUsage {
        estimated_tokens: serialized
            .len()
            .div_ceil(bytes_per_token)
            .saturating_add(structural),
        serialized_bytes: serialized.len(),
    })
}

fn ensure_unique_tools(
    provider: ProviderKind,
    tools: &[ToolDefinition],
) -> Result<(), ProviderError> {
    let mut names = HashSet::with_capacity(tools.len());
    if let Some(duplicate) = tools
        .iter()
        .map(ToolDefinition::name)
        .find(|name| !names.insert((*name).to_owned()))
    {
        return Err(ProviderError::protocol(
            provider,
            format!("duplicate tool definition {duplicate}"),
        ));
    }
    Ok(())
}

fn remote_error(
    provider: ProviderKind,
    code: impl Into<String>,
    message: impl Into<String>,
) -> ProviderError {
    ProviderError::Remote {
        provider,
        code: code.into(),
        message: message.into(),
    }
}

fn status_reason(value: &str) -> crate::StopReason {
    match value {
        "stop" | "STOP" | "end_turn" => crate::StopReason::Stop,
        "tool_calls" | "tool_use" => crate::StopReason::ToolCalls,
        "length" | "MAX_TOKENS" | "max_tokens" => crate::StopReason::Length,
        "content_filter" | "SAFETY" | "BLOCKLIST" | "PROHIBITED_CONTENT" => {
            crate::StopReason::ContentFilter
        }
        other => crate::StopReason::Other(other.to_owned()),
    }
}

fn response_bytes(
    provider: ProviderKind,
    response: Response,
) -> impl futures_util::Stream<Item = Result<bytes::Bytes, ProviderError>> + Send + 'static {
    use futures_util::StreamExt;

    response
        .bytes_stream()
        .map(move |item| item.map_err(|error| ProviderError::transport(provider, error)))
}

fn require_json_object(
    provider: ProviderKind,
    call_id: &str,
    raw: &str,
) -> Result<Value, ProviderError> {
    let arguments: Value = serde_json::from_str(raw).map_err(|error| {
        ProviderError::protocol(
            provider,
            format!("tool call {call_id} contains invalid JSON arguments: {error}"),
        )
    })?;
    if !arguments.is_object() {
        return Err(ProviderError::protocol(
            provider,
            format!("tool call {call_id} arguments are not an object"),
        ));
    }
    Ok(arguments)
}
