use std::{collections::BTreeMap, fmt};

use async_trait::async_trait;
use reqwest::{
    Client,
    header::{ACCEPT, AUTHORIZATION},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::{
    MAX_PROVIDER_TOOL_CALLS_PER_RESPONSE, bearer, default_budget, ensure_unique_tools, estimate,
    remote_error, require_json_object, response_bytes, send, status_reason,
};
use crate::{
    ConfigError, ContextBudget, ContextUsage, HttpProviderConfig, MAX_PROVIDER_TOOL_CALL_ID_BYTES,
    MAX_TOOL_ARGUMENT_BYTES, MessageBlock, Provider, ProviderError, ProviderEvent,
    ProviderEventStream, ProviderKind, ProviderRequest, Role, StopReason, ToolCall, Usage,
    sse::{SseAssembler, decode_sse},
};

/// Direct adapter for OpenAI-compatible Chat Completions streaming APIs.
pub struct OpenAiProvider {
    client: Client,
    config: HttpProviderConfig,
    endpoint: Url,
    budget: ContextBudget,
    max_output_tokens: u32,
}

impl OpenAiProvider {
    /// Uses `<base_url>/chat/completions` and the official bearer header.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint or hardened HTTP client cannot be built.
    pub fn new(config: HttpProviderConfig) -> Result<Self, ConfigError> {
        let client = config.client()?;
        let endpoint = config.endpoint("chat/completions")?;
        Ok(Self {
            client,
            config,
            endpoint,
            budget: default_budget(128_000),
            max_output_tokens: 4096,
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
        ensure_unique_tools(ProviderKind::OpenAi, request.tools())?;
        let messages = request
            .messages()
            .iter()
            .map(openai_message)
            .collect::<Result<Vec<_>, _>>()?;
        let tools = request
            .tools()
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name(),
                        "description": tool.description(),
                        "parameters": tool.input_schema(),
                    }
                })
            })
            .collect::<Vec<_>>();

        let mut body = json!({
            "model": self.config.model,
            "messages": messages,
            "stream": true,
            "stream_options": {"include_usage": true},
            "max_tokens": self.max_output_tokens,
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
        }
        Ok(body)
    }
}

impl fmt::Debug for OpenAiProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenAiProvider")
            .field("config", &self.config)
            .field("endpoint", &self.endpoint)
            .field("budget", &self.budget)
            .field("max_output_tokens", &self.max_output_tokens)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAi
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
            ProviderKind::OpenAi,
            &body,
            4,
            4,
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
        let authorization = bearer(&self.config.api_key)
            .map_err(|error| ProviderError::transport(ProviderKind::OpenAi, error))?;
        let response = send(
            ProviderKind::OpenAi,
            self.client
                .post(self.endpoint.clone())
                .header(AUTHORIZATION, authorization)
                .header(ACCEPT, "text/event-stream")
                .json(&body),
            &cancellation,
        )
        .await?;
        Ok(decode_sse(
            response_bytes(ProviderKind::OpenAi, response),
            OpenAiAssembler::default(),
            cancellation,
            self.budget.max_serialized_bytes(),
        ))
    }
}

fn openai_message(message: &crate::Message) -> Result<Value, ProviderError> {
    let protocol = |detail| ProviderError::protocol(ProviderKind::OpenAi, detail);
    match message.role() {
        Role::System | Role::User => {
            let mut content = String::new();
            for block in message.blocks() {
                match block {
                    MessageBlock::Text(text) => content.push_str(text),
                    _ => return Err(protocol("system/user message contains a tool block")),
                }
            }
            Ok(json!({
                "role": if message.role() == Role::System { "system" } else { "user" },
                "content": content,
            }))
        }
        Role::Assistant => {
            let mut content = String::new();
            let mut tool_calls = Vec::new();
            for block in message.blocks() {
                match block {
                    MessageBlock::Text(text) => content.push_str(text),
                    MessageBlock::ToolCall(call) => tool_calls.push(json!({
                        "id": call.id(),
                        "type": "function",
                        "function": {
                            "name": call.name(),
                            "arguments": serde_json::to_string(call.arguments()).map_err(|error| {
                                ProviderError::serialization(ProviderKind::OpenAi, error)
                            })?,
                        }
                    })),
                    MessageBlock::ToolResult(_) => {
                        return Err(protocol("assistant message contains a tool result"));
                    }
                    MessageBlock::ProviderContinuation(continuation) => {
                        if continuation.provider() == ProviderKind::OpenAi {
                            return Err(protocol(
                                "OpenAI message contains an unsupported continuation",
                            ));
                        }
                    }
                }
            }
            let mut value = json!({"role": "assistant", "content": content});
            if !tool_calls.is_empty() {
                value["tool_calls"] = Value::Array(tool_calls);
            }
            Ok(value)
        }
        Role::Tool => {
            let [MessageBlock::ToolResult(result)] = message.blocks() else {
                return Err(protocol(
                    "tool message must contain exactly one tool result",
                ));
            };
            let content = match result.output().model_value() {
                Value::String(content) => content,
                value => serde_json::to_string(&value)
                    .map_err(|error| ProviderError::serialization(ProviderKind::OpenAi, error))?,
            };
            Ok(json!({
                "role": "tool",
                "tool_call_id": result.call_id(),
                "content": content,
            }))
        }
    }
}

#[derive(Default)]
struct OpenAiAssembler {
    calls: BTreeMap<usize, PartialCall>,
    completion: Option<StopReason>,
    done: bool,
}

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

impl SseAssembler for OpenAiAssembler {
    fn provider(&self) -> ProviderKind {
        ProviderKind::OpenAi
    }

    #[allow(clippy::too_many_lines)]
    fn push(
        &mut self,
        event: eventsource_stream::Event,
    ) -> Result<Vec<ProviderEvent>, ProviderError> {
        if self.done {
            return Err(ProviderError::protocol(
                ProviderKind::OpenAi,
                "received data after [DONE]",
            ));
        }
        if event.data.trim() == "[DONE]" {
            let reason = self.completion.take().ok_or_else(|| {
                ProviderError::protocol(
                    ProviderKind::OpenAi,
                    "[DONE] arrived before a finish reason",
                )
            })?;
            self.done = true;
            return Ok(vec![ProviderEvent::Completed(reason)]);
        }

        let value: Value = serde_json::from_str(&event.data).map_err(|error| {
            ProviderError::protocol(ProviderKind::OpenAi, format!("invalid JSON event: {error}"))
        })?;
        if let Some(error) = value.get("error") {
            let error: WireError = serde_json::from_value(error.clone()).map_err(|parse| {
                ProviderError::protocol(
                    ProviderKind::OpenAi,
                    format!("invalid provider error event: {parse}"),
                )
            })?;
            return Err(remote_error(
                ProviderKind::OpenAi,
                error
                    .code
                    .or(error.kind)
                    .unwrap_or_else(|| "remote_error".to_owned()),
                error.message,
            ));
        }

        let chunk: Chunk = serde_json::from_value(value).map_err(|error| {
            ProviderError::protocol(
                ProviderKind::OpenAi,
                format!("invalid chat completion event: {error}"),
            )
        })?;
        let mut events = Vec::new();
        if let Some(usage) = chunk.usage {
            events.push(ProviderEvent::Usage(Usage {
                input_tokens: usage.input,
                output_tokens: usage.output,
                total_tokens: usage.total,
            }));
        }
        if chunk.choices.len() > 1 {
            return Err(ProviderError::protocol(
                ProviderKind::OpenAi,
                "multiple choices are not supported",
            ));
        }
        let Some(choice) = chunk.choices.into_iter().next() else {
            return Ok(events);
        };
        if choice.index != 0 {
            return Err(ProviderError::protocol(
                ProviderKind::OpenAi,
                "received a non-zero choice index",
            ));
        }
        if self.completion.is_some() {
            return Err(ProviderError::protocol(
                ProviderKind::OpenAi,
                "received content after the finish reason",
            ));
        }
        if let Some(content) = choice.delta.content
            && !content.is_empty()
        {
            events.push(ProviderEvent::TextDelta(content));
        }
        for delta in choice.delta.tool_calls {
            self.push_tool_delta(delta)?;
        }
        if let Some(reason) = choice.finish_reason {
            let reason = status_reason(&reason);
            let has_calls = !self.calls.is_empty();
            if (reason == StopReason::ToolCalls) != has_calls {
                return Err(ProviderError::protocol(
                    ProviderKind::OpenAi,
                    "finish reason does not match assembled tool calls",
                ));
            }
            if has_calls {
                let calls = std::mem::take(&mut self.calls);
                let mut ids = std::collections::HashSet::new();
                for (_, call) in calls {
                    if !ids.insert(call.id.clone()) {
                        return Err(ProviderError::protocol(
                            ProviderKind::OpenAi,
                            format!("duplicate tool call id {}", call.id),
                        ));
                    }
                    let arguments =
                        require_json_object(ProviderKind::OpenAi, &call.id, &call.arguments)?;
                    let call = ToolCall::new(call.id, call.name, arguments).map_err(|message| {
                        ProviderError::protocol(ProviderKind::OpenAi, message)
                    })?;
                    events.push(ProviderEvent::ToolCall(call));
                }
            }
            self.completion = Some(reason);
        }
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<ProviderEvent>, ProviderError> {
        if !self.done {
            return Err(ProviderError::protocol(
                ProviderKind::OpenAi,
                if self.completion.is_some() {
                    "stream ended before [DONE]"
                } else {
                    "stream ended before a finish reason"
                },
            ));
        }
        Ok(Vec::new())
    }
}

impl OpenAiAssembler {
    #[allow(clippy::map_entry)]
    #[allow(clippy::too_many_lines)]
    fn push_tool_delta(&mut self, delta: ToolCallDelta) -> Result<(), ProviderError> {
        let expected_next = self.calls.len();
        if !self.calls.contains_key(&delta.index) {
            if expected_next == MAX_PROVIDER_TOOL_CALLS_PER_RESPONSE {
                return Err(ProviderError::protocol(
                    ProviderKind::OpenAi,
                    format!("response exceeds {MAX_PROVIDER_TOOL_CALLS_PER_RESPONSE} tool calls"),
                ));
            }
            if delta.index != expected_next {
                return Err(ProviderError::protocol(
                    ProviderKind::OpenAi,
                    format!(
                        "tool call index {} arrived before index {expected_next}",
                        delta.index
                    ),
                ));
            }
            if delta.kind.as_deref().is_some_and(|kind| kind != "function") {
                return Err(ProviderError::protocol(
                    ProviderKind::OpenAi,
                    "unsupported tool call type",
                ));
            }
            let id = delta.id.filter(|value| !value.is_empty()).ok_or_else(|| {
                ProviderError::protocol(ProviderKind::OpenAi, "tool call start is missing its id")
            })?;
            if id.len() > MAX_PROVIDER_TOOL_CALL_ID_BYTES {
                return Err(ProviderError::protocol(
                    ProviderKind::OpenAi,
                    format!("tool call id exceeds {MAX_PROVIDER_TOOL_CALL_ID_BYTES} bytes"),
                ));
            }
            let function = delta.function.ok_or_else(|| {
                ProviderError::protocol(
                    ProviderKind::OpenAi,
                    "tool call start is missing its function",
                )
            })?;
            let name = function
                .name
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    ProviderError::protocol(
                        ProviderKind::OpenAi,
                        "tool call start is missing its function name",
                    )
                })?;
            let arguments = function.arguments.unwrap_or_default();
            if arguments.len() > MAX_TOOL_ARGUMENT_BYTES {
                return Err(ProviderError::protocol(
                    ProviderKind::OpenAi,
                    format!(
                        "tool call index {} arguments exceed {MAX_TOOL_ARGUMENT_BYTES} bytes",
                        delta.index
                    ),
                ));
            }
            self.calls.insert(
                delta.index,
                PartialCall {
                    id,
                    name,
                    arguments,
                },
            );
            return Ok(());
        }

        let existing = self
            .calls
            .get_mut(&delta.index)
            .expect("tool call existence checked");
        if delta.id.as_deref().is_some_and(|id| id != existing.id)
            || delta.kind.as_deref().is_some_and(|kind| kind != "function")
        {
            return Err(ProviderError::protocol(
                ProviderKind::OpenAi,
                format!("tool call index {} changed its start metadata", delta.index),
            ));
        }
        let function = delta.function.ok_or_else(|| {
            ProviderError::protocol(
                ProviderKind::OpenAi,
                format!("tool call index {} delta has no function", delta.index),
            )
        })?;
        if function
            .name
            .as_deref()
            .is_some_and(|name| name != existing.name)
        {
            return Err(ProviderError::protocol(
                ProviderKind::OpenAi,
                format!("tool call index {} changed its function name", delta.index),
            ));
        }
        let arguments = function.arguments.ok_or_else(|| {
            ProviderError::protocol(
                ProviderKind::OpenAi,
                format!("tool call index {} delta has no arguments", delta.index),
            )
        })?;
        if existing.arguments.len().saturating_add(arguments.len()) > MAX_TOOL_ARGUMENT_BYTES {
            return Err(ProviderError::protocol(
                ProviderKind::OpenAi,
                format!(
                    "tool call index {} arguments exceed {MAX_TOOL_ARGUMENT_BYTES} bytes",
                    delta.index
                ),
            ));
        }
        existing.arguments.push_str(&arguments);
        Ok(())
    }
}

#[derive(Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<Choice>,
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct Choice {
    #[serde(default)]
    index: usize,
    #[serde(default)]
    delta: Delta,
    finish_reason: Option<String>,
}

#[derive(Default, Deserialize)]
struct Delta {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCallDelta>,
}

#[derive(Deserialize)]
struct ToolCallDelta {
    index: usize,
    id: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    function: Option<FunctionDelta>,
}

#[derive(Deserialize)]
struct FunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(default, rename = "prompt_tokens")]
    input: u64,
    #[serde(default, rename = "completion_tokens")]
    output: u64,
    #[serde(default, rename = "total_tokens")]
    total: u64,
}

#[derive(Deserialize)]
struct WireError {
    message: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    code: Option<String>,
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;
    use serde_json::json;

    use super::{OpenAiAssembler, OpenAiProvider};
    use crate::{
        ApiKey, HttpProviderConfig, MAX_TOOL_ARGUMENT_BYTES, Message, ProviderEvent,
        ProviderRequest, StopReason,
        sse::{SseAssembler, fixture_stream},
    };

    const FIXTURE: &str = include_str!("../../tests/fixtures/openai_tool_stream.sse");

    #[tokio::test]
    async fn assembles_fragmented_utf8_json_tool_call_and_usage() {
        let chinese = FIXTURE.find("你好").expect("fixture contains UTF-8");
        let cuts = [1, chinese + 1, chinese + 2, FIXTURE.len() / 2];
        let events = fixture_stream(FIXTURE, &cuts, OpenAiAssembler::default())
            .collect::<Vec<_>>()
            .await;
        let events = events
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .expect("valid stream");

        assert!(matches!(&events[0], ProviderEvent::TextDelta(text) if text == "你好"));
        assert!(
            events.iter().any(
                |event| matches!(event, ProviderEvent::Usage(usage) if usage.total_tokens == 15)
            )
        );
        assert!(events.iter().any(|event| matches!(event, ProviderEvent::ToolCall(call) if call.arguments()["sql"] == "select 1")));
        assert_eq!(
            events.last(),
            Some(&ProviderEvent::Completed(StopReason::ToolCalls))
        );
    }

    #[tokio::test]
    async fn rejects_abnormal_eof() {
        let fixture = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n";
        let errors = fixture_stream(fixture, &[], OpenAiAssembler::default())
            .filter_map(|item| async move { item.err() })
            .collect::<Vec<_>>()
            .await;
        assert_eq!(errors.len(), 1);
        assert!(errors[0].to_string().contains("finish reason"));
    }

    #[tokio::test]
    async fn surfaces_provider_error_event() {
        let fixture = "data: {\"error\":{\"message\":\"quota\",\"type\":\"rate_limit\",\"code\":\"rate_limit_exceeded\"}}\n\n";
        let errors = fixture_stream(fixture, &[], OpenAiAssembler::default())
            .filter_map(|item| async move { item.err() })
            .collect::<Vec<_>>()
            .await;
        assert_eq!(errors.len(), 1);
        assert!(errors[0].to_string().contains("rate_limit_exceeded"));
    }

    #[test]
    fn request_body_carries_the_configured_output_limit() {
        let provider = OpenAiProvider::new(
            HttpProviderConfig::new(
                "https://example.test/v1",
                "model",
                ApiKey::new("key").expect("valid key"),
            )
            .expect("valid config"),
        )
        .expect("provider builds")
        .with_max_output_tokens(777)
        .expect("non-zero output limit");
        let body = provider
            .body(&ProviderRequest::new(vec![Message::user("hello")], vec![]))
            .expect("body serializes");
        assert_eq!(body["max_tokens"], 777);
        assert!(provider.with_max_output_tokens(0).is_err());
    }

    #[test]
    fn repeated_compatible_metadata_is_allowed_but_conflicts_fail() {
        let mut assembler = OpenAiAssembler::default();
        let first = eventsource_stream::Event {
            data: json!({
                "choices": [{"index": 0, "delta": {"tool_calls": [{
                    "index": 0,
                    "id": "call-1",
                    "type": "function",
                    "function": {"name": "query", "arguments": "{"}
                }]}, "finish_reason": null}]
            })
            .to_string(),
            ..eventsource_stream::Event::default()
        };
        assembler.push(first).expect("call starts");

        let repeated = eventsource_stream::Event {
            data: json!({
                "choices": [{"index": 0, "delta": {"tool_calls": [{
                    "index": 0,
                    "id": "call-1",
                    "type": "function",
                    "function": {"name": "query", "arguments": "}"}
                }]}, "finish_reason": null}]
            })
            .to_string(),
            ..eventsource_stream::Event::default()
        };
        assembler
            .push(repeated)
            .expect("same metadata may repeat on compatible APIs");

        let conflicting = eventsource_stream::Event {
            data: json!({
                "choices": [{"index": 0, "delta": {"tool_calls": [{
                    "index": 0,
                    "id": "different",
                    "function": {"arguments": ""}
                }]}, "finish_reason": null}]
            })
            .to_string(),
            ..eventsource_stream::Event::default()
        };
        assert!(assembler.push(conflicting).is_err());
    }

    #[test]
    fn argument_limit_is_enforced_before_json_assembly_finishes() {
        let mut assembler = OpenAiAssembler::default();
        let event = eventsource_stream::Event {
            data: json!({
                "choices": [{"index": 0, "delta": {"tool_calls": [{
                    "index": 0,
                    "id": "call-1",
                    "type": "function",
                    "function": {
                        "name": "query",
                        "arguments": "x".repeat(MAX_TOOL_ARGUMENT_BYTES + 1)
                    }
                }]}, "finish_reason": null}]
            })
            .to_string(),
            ..eventsource_stream::Event::default()
        };
        let error = assembler
            .push(event)
            .expect_err("oversized delta fails immediately");
        assert!(error.to_string().contains("65536"));
    }

    #[tokio::test]
    async fn invalid_json_arguments_fail_closed() {
        let fixture = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"query\",\"arguments\":\"{\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        );
        let errors = fixture_stream(fixture, &[], OpenAiAssembler::default())
            .filter_map(|item| async move { item.err() })
            .collect::<Vec<_>>()
            .await;
        assert_eq!(errors.len(), 1);
        assert!(errors[0].to_string().contains("invalid JSON arguments"));
    }

    #[tokio::test]
    async fn usage_after_finish_reason_precedes_normalized_completion() {
        let fixture = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"done\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1,\"total_tokens\":4}}\n\n",
            "data: [DONE]\n\n",
        );
        let events = fixture_stream(fixture, &[], OpenAiAssembler::default())
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .expect("valid stream");
        assert!(matches!(events[1], ProviderEvent::Usage(_)));
        assert_eq!(
            events.last(),
            Some(&ProviderEvent::Completed(StopReason::Stop))
        );
    }
}
