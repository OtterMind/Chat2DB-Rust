use std::{collections::HashSet, fmt};

use async_trait::async_trait;
use reqwest::{
    Client,
    header::{ACCEPT, HeaderName, HeaderValue},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::{
    MAX_PROVIDER_TOOL_CALLS_PER_RESPONSE, default_budget, ensure_unique_tools, estimate,
    remote_error, require_json_object, response_bytes, secret_header, send, status_reason,
};
use crate::{
    ConfigError, ContextBudget, ContextUsage, HttpProviderConfig, MAX_PROVIDER_TOOL_CALL_ID_BYTES,
    MAX_TOOL_ARGUMENT_BYTES, MessageBlock, Provider, ProviderError, ProviderEvent,
    ProviderEventStream, ProviderKind, ProviderRequest, Role, StopReason, ToolCall, Usage,
    sse::{SseAssembler, decode_sse},
};

const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 4096;
const ANTHROPIC_VERSION: HeaderName = HeaderName::from_static("anthropic-version");
const X_API_KEY: HeaderName = HeaderName::from_static("x-api-key");

/// Direct adapter for Anthropic's Messages streaming API.
pub struct AnthropicProvider {
    client: Client,
    config: HttpProviderConfig,
    endpoint: Url,
    budget: ContextBudget,
    max_output_tokens: u32,
}

impl AnthropicProvider {
    /// Uses `<base_url>/messages`, `x-api-key`, and the stable Messages version.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint or hardened HTTP client cannot be built.
    pub fn new(config: HttpProviderConfig) -> Result<Self, ConfigError> {
        let client = config.client()?;
        let endpoint = config.endpoint("messages")?;
        Ok(Self {
            client,
            config,
            endpoint,
            budget: default_budget(200_000),
            max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
        })
    }

    #[must_use]
    pub fn with_context_budget(mut self, budget: ContextBudget) -> Self {
        self.budget = budget;
        self
    }

    /// Sets the non-zero `max_tokens` request bound.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::ZeroOutputTokens`] for zero.
    pub fn with_max_output_tokens(mut self, max_output_tokens: u32) -> Result<Self, ConfigError> {
        if max_output_tokens == 0 {
            return Err(ConfigError::ZeroOutputTokens);
        }
        self.max_output_tokens = max_output_tokens;
        Ok(self)
    }

    fn body(&self, request: &ProviderRequest) -> Result<Value, ProviderError> {
        ensure_unique_tools(ProviderKind::Anthropic, request.tools())?;
        let mut system = String::new();
        let mut messages: Vec<AnthropicMessage> = Vec::new();

        for message in request.messages() {
            if message.role() == Role::System {
                for block in message.blocks() {
                    let MessageBlock::Text(text) = block else {
                        return Err(ProviderError::protocol(
                            ProviderKind::Anthropic,
                            "system message contains a tool block",
                        ));
                    };
                    if !system.is_empty() {
                        system.push_str("\n\n");
                    }
                    system.push_str(text);
                }
                continue;
            }

            let (role, content) = anthropic_content(message)?;
            if let Some(previous) = messages.last_mut().filter(|item| item.role == role) {
                previous.content.extend(content);
            } else {
                messages.push(AnthropicMessage { role, content });
            }
        }

        let tools = request
            .tools()
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "input_schema": tool.input_schema(),
                })
            })
            .collect::<Vec<_>>();
        let messages = messages
            .into_iter()
            .map(|message| json!({"role": message.role, "content": message.content}))
            .collect::<Vec<_>>();
        let mut body = json!({
            "model": self.config.model,
            "messages": messages,
            "max_tokens": self.max_output_tokens,
            "stream": true,
        });
        if !system.is_empty() {
            body["system"] = Value::String(system);
        }
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
        }
        Ok(body)
    }
}

impl fmt::Debug for AnthropicProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AnthropicProvider")
            .field("config", &self.config)
            .field("endpoint", &self.endpoint)
            .field("budget", &self.budget)
            .field("max_output_tokens", &self.max_output_tokens)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    fn model(&self) -> &str {
        &self.config.model
    }

    fn context_budget(&self) -> ContextBudget {
        self.budget
    }

    fn estimate_context(&self, request: &ProviderRequest) -> Result<ContextUsage, ProviderError> {
        let body = self.body(request)?;
        estimate(
            ProviderKind::Anthropic,
            &body,
            4,
            3,
            request.messages(),
            request.tools(),
        )
    }

    async fn stream(
        &self,
        request: ProviderRequest,
        cancellation: CancellationToken,
    ) -> Result<ProviderEventStream, ProviderError> {
        let body = self.body(&request)?;
        let api_key = secret_header(&self.config.api_key, "x-api-key")
            .map_err(|error| ProviderError::transport(ProviderKind::Anthropic, error))?;
        let response = send(
            ProviderKind::Anthropic,
            self.client
                .post(self.endpoint.clone())
                .header(X_API_KEY, api_key)
                .header(ANTHROPIC_VERSION, HeaderValue::from_static("2023-06-01"))
                .header(ACCEPT, "text/event-stream")
                .json(&body),
            &cancellation,
        )
        .await?;
        Ok(decode_sse(
            response_bytes(ProviderKind::Anthropic, response),
            AnthropicAssembler::default(),
            cancellation,
            self.budget.max_serialized_bytes(),
        ))
    }
}

struct AnthropicMessage {
    role: &'static str,
    content: Vec<Value>,
}

fn anthropic_content(
    message: &crate::Message,
) -> Result<(&'static str, Vec<Value>), ProviderError> {
    let mut content = Vec::new();
    let role = match message.role() {
        Role::User => {
            for block in message.blocks() {
                let MessageBlock::Text(text) = block else {
                    return Err(ProviderError::protocol(
                        ProviderKind::Anthropic,
                        "user message contains a tool block",
                    ));
                };
                content.push(json!({"type": "text", "text": text}));
            }
            "user"
        }
        Role::Assistant => {
            for block in message.blocks() {
                match block {
                    MessageBlock::Text(text) => {
                        content.push(json!({"type": "text", "text": text}));
                    }
                    MessageBlock::ToolCall(call) => content.push(json!({
                        "type": "tool_use",
                        "id": call.id(),
                        "name": call.name(),
                        "input": call.arguments(),
                    })),
                    MessageBlock::ToolResult(_) => {
                        return Err(ProviderError::protocol(
                            ProviderKind::Anthropic,
                            "assistant message contains a tool result",
                        ));
                    }
                    MessageBlock::ProviderContinuation(continuation) => {
                        if continuation.provider() == ProviderKind::Anthropic {
                            return Err(ProviderError::protocol(
                                ProviderKind::Anthropic,
                                "Anthropic message contains an unsupported continuation",
                            ));
                        }
                    }
                }
            }
            "assistant"
        }
        Role::Tool => {
            let [MessageBlock::ToolResult(result)] = message.blocks() else {
                return Err(ProviderError::protocol(
                    ProviderKind::Anthropic,
                    "tool message must contain exactly one tool result",
                ));
            };
            let model_value = result.output().model_value();
            let text = match model_value {
                Value::String(text) => text,
                value => serde_json::to_string(&value).map_err(|error| {
                    ProviderError::serialization(ProviderKind::Anthropic, error)
                })?,
            };
            content.push(json!({
                "type": "tool_result",
                "tool_use_id": result.call_id(),
                "content": text,
            }));
            "user"
        }
        Role::System => unreachable!("system messages handled by caller"),
    };
    Ok((role, content))
}

#[derive(Default)]
struct AnthropicAssembler {
    next_index: usize,
    active: Option<ActiveBlock>,
    call_ids: HashSet<String>,
    saw_tool_call: bool,
    stop_reason: Option<StopReason>,
    completed: bool,
}

enum ActiveBlock {
    Text {
        index: usize,
    },
    Tool {
        index: usize,
        id: String,
        name: String,
        arguments: String,
        initial_input: bool,
    },
    Hidden {
        index: usize,
    },
}

impl ActiveBlock {
    const fn index(&self) -> usize {
        match self {
            Self::Text { index } | Self::Tool { index, .. } | Self::Hidden { index } => *index,
        }
    }
}

impl SseAssembler for AnthropicAssembler {
    fn provider(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    fn push(
        &mut self,
        event: eventsource_stream::Event,
    ) -> Result<Vec<ProviderEvent>, ProviderError> {
        if event.data.trim() == "[DONE]" {
            if !self.completed {
                return Err(ProviderError::protocol(
                    ProviderKind::Anthropic,
                    "[DONE] arrived before message_stop",
                ));
            }
            return Ok(Vec::new());
        }
        if self.completed {
            return Err(ProviderError::protocol(
                ProviderKind::Anthropic,
                "received data after message_stop",
            ));
        }

        let value: Value = serde_json::from_str(&event.data).map_err(|error| {
            ProviderError::protocol(
                ProviderKind::Anthropic,
                format!("invalid JSON event: {error}"),
            )
        })?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(event.event.as_str());
        match kind {
            "ping" => Ok(Vec::new()),
            "message_start" => {
                let start: MessageStart = serde_json::from_value(value).map_err(|error| {
                    ProviderError::protocol(
                        ProviderKind::Anthropic,
                        format!("invalid message_start: {error}"),
                    )
                })?;
                let input = start.message.usage.input_tokens;
                Ok(vec![ProviderEvent::Usage(Usage {
                    input_tokens: input,
                    output_tokens: 0,
                    total_tokens: input,
                })])
            }
            "content_block_start" => self.start_block(value),
            "content_block_delta" => self.delta_block(value),
            "content_block_stop" => self.stop_block(value),
            "message_delta" => self.message_delta(value),
            "message_stop" => self.message_stop(),
            "error" => {
                let envelope: ErrorEnvelope = serde_json::from_value(value).map_err(|error| {
                    ProviderError::protocol(
                        ProviderKind::Anthropic,
                        format!("invalid provider error event: {error}"),
                    )
                })?;
                Err(remote_error(
                    ProviderKind::Anthropic,
                    envelope.error.kind,
                    envelope.error.message,
                ))
            }
            other => Err(ProviderError::protocol(
                ProviderKind::Anthropic,
                format!("unknown event type {other}"),
            )),
        }
    }

    fn finish(&mut self) -> Result<Vec<ProviderEvent>, ProviderError> {
        if self.completed {
            Ok(Vec::new())
        } else {
            Err(ProviderError::protocol(
                ProviderKind::Anthropic,
                "stream ended before message_stop",
            ))
        }
    }
}

impl AnthropicAssembler {
    #[allow(clippy::too_many_lines)]
    fn start_block(&mut self, value: Value) -> Result<Vec<ProviderEvent>, ProviderError> {
        if self.active.is_some() {
            return Err(ProviderError::protocol(
                ProviderKind::Anthropic,
                "content blocks overlap",
            ));
        }
        let start: BlockStart = serde_json::from_value(value).map_err(|error| {
            ProviderError::protocol(
                ProviderKind::Anthropic,
                format!("invalid content_block_start: {error}"),
            )
        })?;
        if start.index != self.next_index {
            return Err(ProviderError::protocol(
                ProviderKind::Anthropic,
                format!(
                    "content block index {} arrived before index {}",
                    start.index, self.next_index
                ),
            ));
        }
        let mut events = Vec::new();
        self.active = Some(match start.content_block.kind.as_str() {
            "text" => {
                if let Some(text) = start.content_block.text.filter(|text| !text.is_empty()) {
                    events.push(ProviderEvent::TextDelta(text));
                }
                ActiveBlock::Text { index: start.index }
            }
            "tool_use" => {
                let id = start
                    .content_block
                    .id
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        ProviderError::protocol(
                            ProviderKind::Anthropic,
                            "tool_use block is missing its id",
                        )
                    })?;
                if id.len() > MAX_PROVIDER_TOOL_CALL_ID_BYTES {
                    return Err(ProviderError::protocol(
                        ProviderKind::Anthropic,
                        format!("tool call id exceeds {MAX_PROVIDER_TOOL_CALL_ID_BYTES} bytes"),
                    ));
                }
                if self.call_ids.len() == MAX_PROVIDER_TOOL_CALLS_PER_RESPONSE {
                    return Err(ProviderError::protocol(
                        ProviderKind::Anthropic,
                        format!(
                            "response exceeds {MAX_PROVIDER_TOOL_CALLS_PER_RESPONSE} tool calls"
                        ),
                    ));
                }
                if !self.call_ids.insert(id.clone()) {
                    return Err(ProviderError::protocol(
                        ProviderKind::Anthropic,
                        format!("duplicate tool call id {id}"),
                    ));
                }
                let name = start
                    .content_block
                    .name
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        ProviderError::protocol(
                            ProviderKind::Anthropic,
                            "tool_use block is missing its name",
                        )
                    })?;
                let input = start.content_block.input.unwrap_or_else(|| json!({}));
                let initial_input = input.as_object().is_some_and(|value| !value.is_empty());
                let arguments = if initial_input {
                    let arguments = serde_json::to_string(&input).map_err(|error| {
                        ProviderError::serialization(ProviderKind::Anthropic, error)
                    })?;
                    if arguments.len() > MAX_TOOL_ARGUMENT_BYTES {
                        return Err(ProviderError::protocol(
                            ProviderKind::Anthropic,
                            format!(
                                "tool call {id} arguments exceed {MAX_TOOL_ARGUMENT_BYTES} bytes"
                            ),
                        ));
                    }
                    arguments
                } else {
                    String::new()
                };
                ActiveBlock::Tool {
                    index: start.index,
                    id,
                    name,
                    arguments,
                    initial_input,
                }
            }
            "thinking" | "redacted_thinking" => ActiveBlock::Hidden { index: start.index },
            other => {
                return Err(ProviderError::protocol(
                    ProviderKind::Anthropic,
                    format!("unknown content block type {other}"),
                ));
            }
        });
        Ok(events)
    }

    fn delta_block(&mut self, value: Value) -> Result<Vec<ProviderEvent>, ProviderError> {
        let delta: BlockDelta = serde_json::from_value(value).map_err(|error| {
            ProviderError::protocol(
                ProviderKind::Anthropic,
                format!("invalid content_block_delta: {error}"),
            )
        })?;
        let active = self.active.as_mut().ok_or_else(|| {
            ProviderError::protocol(
                ProviderKind::Anthropic,
                "content block delta arrived without a start",
            )
        })?;
        if active.index() != delta.index {
            return Err(ProviderError::protocol(
                ProviderKind::Anthropic,
                format!("delta for out-of-order content block {}", delta.index),
            ));
        }
        match (active, delta.delta.kind.as_str()) {
            (ActiveBlock::Text { .. }, "text_delta") => Ok(delta
                .delta
                .text
                .filter(|text| !text.is_empty())
                .map_or_else(Vec::new, |text| vec![ProviderEvent::TextDelta(text)])),
            (
                ActiveBlock::Tool {
                    arguments,
                    initial_input,
                    ..
                },
                "input_json_delta",
            ) => {
                if *initial_input {
                    return Err(ProviderError::protocol(
                        ProviderKind::Anthropic,
                        "tool_use sent JSON deltas after a complete initial input",
                    ));
                }
                let partial = delta.delta.partial_json.ok_or_else(|| {
                    ProviderError::protocol(
                        ProviderKind::Anthropic,
                        "input_json_delta is missing partial_json",
                    )
                })?;
                if arguments.len().saturating_add(partial.len()) > MAX_TOOL_ARGUMENT_BYTES {
                    return Err(ProviderError::protocol(
                        ProviderKind::Anthropic,
                        format!("tool call arguments exceed {MAX_TOOL_ARGUMENT_BYTES} bytes"),
                    ));
                }
                arguments.push_str(&partial);
                Ok(Vec::new())
            }
            (ActiveBlock::Hidden { .. }, "thinking_delta" | "signature_delta") => Ok(Vec::new()),
            _ => Err(ProviderError::protocol(
                ProviderKind::Anthropic,
                "delta type does not match its content block",
            )),
        }
    }

    fn stop_block(&mut self, value: Value) -> Result<Vec<ProviderEvent>, ProviderError> {
        let stop: BlockStop = serde_json::from_value(value).map_err(|error| {
            ProviderError::protocol(
                ProviderKind::Anthropic,
                format!("invalid content_block_stop: {error}"),
            )
        })?;
        let active = self.active.take().ok_or_else(|| {
            ProviderError::protocol(
                ProviderKind::Anthropic,
                "content block stop arrived without a start",
            )
        })?;
        if active.index() != stop.index {
            return Err(ProviderError::protocol(
                ProviderKind::Anthropic,
                format!("stop for out-of-order content block {}", stop.index),
            ));
        }
        self.next_index += 1;
        match active {
            ActiveBlock::Tool {
                id,
                name,
                arguments,
                ..
            } => {
                let raw = if arguments.is_empty() {
                    "{}"
                } else {
                    &arguments
                };
                let arguments = require_json_object(ProviderKind::Anthropic, &id, raw)?;
                let call = ToolCall::new(id, name, arguments)
                    .map_err(|message| ProviderError::protocol(ProviderKind::Anthropic, message))?;
                self.saw_tool_call = true;
                Ok(vec![ProviderEvent::ToolCall(call)])
            }
            ActiveBlock::Text { .. } | ActiveBlock::Hidden { .. } => Ok(Vec::new()),
        }
    }

    fn message_delta(&mut self, value: Value) -> Result<Vec<ProviderEvent>, ProviderError> {
        if self.active.is_some() || self.stop_reason.is_some() {
            return Err(ProviderError::protocol(
                ProviderKind::Anthropic,
                "message_delta arrived before block completion or repeated its stop reason",
            ));
        }
        let delta: MessageDelta = serde_json::from_value(value).map_err(|error| {
            ProviderError::protocol(
                ProviderKind::Anthropic,
                format!("invalid message_delta: {error}"),
            )
        })?;
        let reason = status_reason(&delta.delta.stop_reason);
        let output = delta.usage.output_tokens;
        self.stop_reason = Some(reason);
        Ok(vec![ProviderEvent::Usage(Usage {
            input_tokens: 0,
            output_tokens: output,
            total_tokens: output,
        })])
    }

    fn message_stop(&mut self) -> Result<Vec<ProviderEvent>, ProviderError> {
        if self.active.is_some() {
            return Err(ProviderError::protocol(
                ProviderKind::Anthropic,
                "message_stop arrived with an open content block",
            ));
        }
        let reason = self.stop_reason.take().ok_or_else(|| {
            ProviderError::protocol(
                ProviderKind::Anthropic,
                "message_stop arrived without a stop reason",
            )
        })?;
        if (reason == StopReason::ToolCalls) != self.saw_tool_call {
            return Err(ProviderError::protocol(
                ProviderKind::Anthropic,
                "stop reason does not match assembled tool calls",
            ));
        }
        self.completed = true;
        Ok(vec![ProviderEvent::Completed(reason)])
    }
}

#[derive(Deserialize)]
struct MessageStart {
    message: StartedMessage,
}

#[derive(Deserialize)]
struct StartedMessage {
    usage: InputUsage,
}

#[derive(Deserialize)]
struct InputUsage {
    #[serde(default)]
    input_tokens: u64,
}

#[derive(Deserialize)]
struct BlockStart {
    index: usize,
    content_block: WireContentBlock,
}

#[derive(Deserialize)]
struct WireContentBlock {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
    id: Option<String>,
    name: Option<String>,
    input: Option<Value>,
}

#[derive(Deserialize)]
struct BlockDelta {
    index: usize,
    delta: WireDelta,
}

#[derive(Deserialize)]
struct WireDelta {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
    partial_json: Option<String>,
}

#[derive(Deserialize)]
struct BlockStop {
    index: usize,
}

#[derive(Deserialize)]
struct MessageDelta {
    delta: StopDelta,
    usage: OutputUsage,
}

#[derive(Deserialize)]
struct StopDelta {
    stop_reason: String,
}

#[derive(Deserialize)]
struct OutputUsage {
    #[serde(default)]
    output_tokens: u64,
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: WireError,
}

#[derive(Deserialize)]
struct WireError {
    #[serde(rename = "type")]
    kind: String,
    message: String,
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;
    use serde_json::json;

    use super::{AnthropicAssembler, AnthropicProvider};
    use crate::{
        ApiKey, HttpProviderConfig, MAX_TOOL_ARGUMENT_BYTES, Message, ProviderEvent,
        ProviderRequest, StopReason,
        sse::{SseAssembler, fixture_stream},
    };

    const FIXTURE: &str = include_str!("../../tests/fixtures/anthropic_tool_stream.sse");

    #[tokio::test]
    async fn assembles_blocks_and_never_emits_thinking() {
        let utf8 = FIXTURE.find('好').expect("fixture contains UTF-8");
        let events = fixture_stream(
            FIXTURE,
            &[2, utf8 + 1, utf8 + 2, FIXTURE.len() / 2],
            AnthropicAssembler::default(),
        )
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("valid stream");

        assert!(matches!(&events[1], ProviderEvent::TextDelta(text) if text == "好"));
        assert!(!format!("{events:?}").contains("hidden"));
        assert!(events.iter().any(|event| matches!(event, ProviderEvent::ToolCall(call) if call.arguments()["sql"] == "select 1")));
        assert_eq!(
            events.last(),
            Some(&ProviderEvent::Completed(StopReason::ToolCalls))
        );
    }

    #[tokio::test]
    async fn rejects_out_of_order_block() {
        let fixture = "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"bad\"}}\n\n";
        let errors = fixture_stream(fixture, &[], AnthropicAssembler::default())
            .filter_map(|item| async move { item.err() })
            .collect::<Vec<_>>()
            .await;
        assert_eq!(errors.len(), 1);
        assert!(errors[0].to_string().contains("before index 0"));
    }

    #[tokio::test]
    async fn surfaces_provider_error() {
        let fixture = "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"busy\"}}\n\n";
        let errors = fixture_stream(fixture, &[], AnthropicAssembler::default())
            .filter_map(|item| async move { item.err() })
            .collect::<Vec<_>>()
            .await;
        assert_eq!(errors.len(), 1);
        assert!(errors[0].to_string().contains("overloaded_error"));
    }

    #[tokio::test]
    async fn rejects_abnormal_eof() {
        let fixture = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1}}}\n\n";
        let errors = fixture_stream(fixture, &[], AnthropicAssembler::default())
            .filter_map(|item| async move { item.err() })
            .collect::<Vec<_>>()
            .await;
        assert_eq!(errors.len(), 1);
        assert!(errors[0].to_string().contains("message_stop"));
    }

    #[test]
    fn request_body_carries_the_configured_output_limit() {
        let provider = AnthropicProvider::new(
            HttpProviderConfig::new(
                "https://example.test/v1",
                "model",
                ApiKey::new("key").expect("valid key"),
            )
            .expect("valid config"),
        )
        .expect("provider builds")
        .with_max_output_tokens(888)
        .expect("non-zero output limit");
        let body = provider
            .body(&ProviderRequest::new(vec![Message::user("hello")], vec![]))
            .expect("body serializes");
        assert_eq!(body["max_tokens"], 888);
        assert!(provider.with_max_output_tokens(0).is_err());
    }

    #[test]
    fn argument_limit_is_enforced_during_input_json_delta() {
        let mut assembler = AnthropicAssembler::default();
        assembler
            .push(eventsource_stream::Event {
                data: json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "tool_use",
                        "id": "tool-1",
                        "name": "query",
                        "input": {}
                    }
                })
                .to_string(),
                ..eventsource_stream::Event::default()
            })
            .expect("tool block starts");
        let error = assembler
            .push(eventsource_stream::Event {
                data: json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": "x".repeat(MAX_TOOL_ARGUMENT_BYTES + 1)
                    }
                })
                .to_string(),
                ..eventsource_stream::Event::default()
            })
            .expect_err("oversized fragment fails immediately");
        assert!(error.to_string().contains("65536"));
    }
}
