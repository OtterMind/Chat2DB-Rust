use std::{collections::HashMap, fmt};

use async_trait::async_trait;
use reqwest::{
    Client,
    header::{ACCEPT, HeaderName},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

use super::{
    MAX_PROVIDER_TOOL_CALLS_PER_RESPONSE, default_budget, ensure_unique_tools, estimate,
    remote_error, response_bytes, secret_header, send, status_reason,
};
use crate::{
    ConfigError, ContextBudget, ContextUsage, HttpProviderConfig, MAX_TOOL_ARGUMENT_BYTES,
    MessageBlock, Provider, ProviderContinuation, ProviderContinuationKind,
    ProviderContinuationPlacement, ProviderError, ProviderEvent, ProviderEventStream, ProviderKind,
    ProviderRequest, ProviderToolCallIdentity, Role, StopReason, ToolCall, Usage,
    sse::{SseAssembler, decode_sse},
};

const X_GOOG_API_KEY: HeaderName = HeaderName::from_static("x-goog-api-key");

/// Direct adapter for Gemini's `streamGenerateContent` SSE API.
pub struct GeminiProvider {
    client: Client,
    config: HttpProviderConfig,
    endpoint: Url,
    budget: ContextBudget,
    max_output_tokens: u32,
}

impl GeminiProvider {
    /// Builds `<base_url>/models/<model>:streamGenerateContent?alt=sse`.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint or hardened HTTP client cannot be built.
    pub fn new(config: HttpProviderConfig) -> Result<Self, ConfigError> {
        let client = config.client()?;
        let mut endpoint = config.base_url.clone();
        let model = config
            .model
            .strip_prefix("models/")
            .unwrap_or(&config.model);
        endpoint
            .path_segments_mut()
            .map_err(|()| ConfigError::InvalidBaseUrl(url::ParseError::RelativeUrlWithoutBase))?
            .pop_if_empty()
            .push("models")
            .push(&format!("{model}:streamGenerateContent"));
        endpoint.query_pairs_mut().append_pair("alt", "sse");
        Ok(Self {
            client,
            config,
            endpoint,
            budget: default_budget(1_000_000),
            max_output_tokens: 4096,
        })
    }

    #[must_use]
    pub fn with_context_budget(mut self, budget: ContextBudget) -> Self {
        self.budget = budget;
        self
    }

    /// Sets the non-zero `generationConfig.maxOutputTokens` request bound.
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
        ensure_unique_tools(ProviderKind::Gemini, request.tools())?;
        let mut calls_by_id = HashMap::new();
        for call in request
            .messages()
            .iter()
            .flat_map(crate::Message::blocks)
            .filter_map(|block| match block {
                MessageBlock::ToolCall(call) => Some(call),
                MessageBlock::Text(_)
                | MessageBlock::ToolResult(_)
                | MessageBlock::ProviderContinuation(_) => None,
            })
        {
            if calls_by_id.insert(call.id(), call).is_some() {
                return Err(ProviderError::protocol(
                    ProviderKind::Gemini,
                    format!("duplicate tool call id {} in context", call.id()),
                ));
            }
        }

        let mut system = String::new();
        let mut contents: Vec<GeminiContent> = Vec::new();
        for message in request.messages() {
            if message.role() == Role::System {
                for block in message.blocks() {
                    let MessageBlock::Text(text) = block else {
                        return Err(ProviderError::protocol(
                            ProviderKind::Gemini,
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
            let (role, parts) = gemini_parts(message, &calls_by_id)?;
            if let Some(previous) = contents.last_mut().filter(|item| item.role == role) {
                previous.parts.extend(parts);
            } else {
                contents.push(GeminiContent { role, parts });
            }
        }
        let contents = contents
            .into_iter()
            .map(|content| json!({"role": content.role, "parts": content.parts}))
            .collect::<Vec<_>>();
        let declarations = request
            .tools()
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "parameters": tool.input_schema(),
                })
            })
            .collect::<Vec<_>>();
        let mut body = json!({
            "contents": contents,
            "generationConfig": {"maxOutputTokens": self.max_output_tokens},
        });
        if !system.is_empty() {
            body["systemInstruction"] = json!({"parts": [{"text": system}]});
        }
        if !declarations.is_empty() {
            body["tools"] = json!([{"functionDeclarations": declarations}]);
        }
        Ok(body)
    }
}

impl fmt::Debug for GeminiProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GeminiProvider")
            .field("config", &self.config)
            .field("endpoint", &self.endpoint)
            .field("budget", &self.budget)
            .field("max_output_tokens", &self.max_output_tokens)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gemini
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
            ProviderKind::Gemini,
            &body,
            4,
            2,
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
        let api_key = secret_header(&self.config.api_key, "x-goog-api-key")
            .map_err(|error| ProviderError::transport(ProviderKind::Gemini, error))?;
        let response = send(
            ProviderKind::Gemini,
            self.client
                .post(self.endpoint.clone())
                .header(X_GOOG_API_KEY, api_key)
                .header(ACCEPT, "text/event-stream")
                .json(&body),
            &cancellation,
        )
        .await?;
        Ok(decode_sse(
            response_bytes(ProviderKind::Gemini, response),
            GeminiAssembler::default(),
            cancellation,
            self.budget.max_serialized_bytes(),
        ))
    }
}

struct GeminiContent {
    role: &'static str,
    parts: Vec<Value>,
}

fn gemini_wire_call_id(call: &ToolCall) -> Option<&str> {
    match call.provider_identity() {
        Some(identity) if identity.provider() == ProviderKind::Gemini => identity.wire_id(),
        Some(_) | None => Some(call.id()),
    }
}

fn gemini_parts(
    message: &crate::Message,
    calls_by_id: &HashMap<&str, &ToolCall>,
) -> Result<(&'static str, Vec<Value>), ProviderError> {
    let mut parts = Vec::new();
    let role = match message.role() {
        Role::User => {
            for block in message.blocks() {
                let MessageBlock::Text(text) = block else {
                    return Err(ProviderError::protocol(
                        ProviderKind::Gemini,
                        "user message contains a tool block",
                    ));
                };
                parts.push(json!({"text": text}));
            }
            "user"
        }
        Role::Assistant => {
            for block in message.blocks() {
                match block {
                    MessageBlock::Text(text) => parts.push(json!({"text": text})),
                    MessageBlock::ToolCall(call) => {
                        let mut function_call = json!({
                            "name": call.name(),
                            "args": call.arguments(),
                        });
                        if let Some(wire_id) = gemini_wire_call_id(call) {
                            function_call["id"] = Value::String(wire_id.to_owned());
                        }
                        parts.push(json!({"functionCall": function_call}));
                    }
                    MessageBlock::ToolResult(_) => {
                        return Err(ProviderError::protocol(
                            ProviderKind::Gemini,
                            "assistant message contains a tool result",
                        ));
                    }
                    MessageBlock::ProviderContinuation(continuation) => {
                        if continuation.provider() != ProviderKind::Gemini {
                            continue;
                        }
                        if continuation.kind() != ProviderContinuationKind::GeminiThoughtSignature {
                            return Err(ProviderError::protocol(
                                ProviderKind::Gemini,
                                "Gemini message contains an unsupported continuation",
                            ));
                        }
                        let signature = Value::String(continuation.value().to_owned());
                        match continuation.placement() {
                            ProviderContinuationPlacement::AttachedToPreviousPart => {
                                let Some(previous) = parts.last_mut() else {
                                    return Err(ProviderError::protocol(
                                        ProviderKind::Gemini,
                                        "attached continuation has no preceding content part",
                                    ));
                                };
                                if previous.get("thoughtSignature").is_some() {
                                    return Err(ProviderError::protocol(
                                        ProviderKind::Gemini,
                                        "content part has duplicate continuations",
                                    ));
                                }
                                previous["thoughtSignature"] = signature;
                            }
                            ProviderContinuationPlacement::StandalonePart => {
                                parts.push(json!({"thoughtSignature": signature}));
                            }
                        }
                    }
                }
            }
            "model"
        }
        Role::Tool => {
            let [MessageBlock::ToolResult(result)] = message.blocks() else {
                return Err(ProviderError::protocol(
                    ProviderKind::Gemini,
                    "tool message must contain exactly one tool result",
                ));
            };
            let call = calls_by_id.get(result.call_id()).ok_or_else(|| {
                ProviderError::protocol(
                    ProviderKind::Gemini,
                    format!("tool result {} has no matching call", result.call_id()),
                )
            })?;
            let response = match result.output().model_value() {
                value @ Value::Object(_) => value,
                value => json!({"content": value}),
            };
            let mut function_response = json!({
                "name": call.name(),
                "response": response,
            });
            if let Some(wire_id) = gemini_wire_call_id(call) {
                function_response["id"] = Value::String(wire_id.to_owned());
            }
            parts.push(json!({"functionResponse": function_response}));
            "user"
        }
        Role::System => unreachable!("system messages handled by caller"),
    };
    Ok((role, parts))
}

#[derive(Default)]
struct GeminiAssembler {
    call_ids: std::collections::HashSet<String>,
    saw_tool_call: bool,
    completion: Option<StopReason>,
    done: bool,
}

impl SseAssembler for GeminiAssembler {
    fn provider(&self) -> ProviderKind {
        ProviderKind::Gemini
    }

    #[allow(clippy::too_many_lines)]
    fn push(
        &mut self,
        event: eventsource_stream::Event,
    ) -> Result<Vec<ProviderEvent>, ProviderError> {
        if event.data.trim() == "[DONE]" {
            let reason = self.completion.take().ok_or_else(|| {
                ProviderError::protocol(
                    ProviderKind::Gemini,
                    "[DONE] arrived before a finish reason",
                )
            })?;
            self.done = true;
            return Ok(vec![ProviderEvent::Completed(reason)]);
        }
        if self.done {
            return Err(ProviderError::protocol(
                ProviderKind::Gemini,
                "received data after a finish reason",
            ));
        }

        let value: Value = serde_json::from_str(&event.data).map_err(|error| {
            ProviderError::protocol(ProviderKind::Gemini, format!("invalid JSON event: {error}"))
        })?;
        if let Some(error) = value.get("error") {
            let error: ErrorBody = serde_json::from_value(error.clone()).map_err(|parse| {
                ProviderError::protocol(
                    ProviderKind::Gemini,
                    format!("invalid provider error event: {parse}"),
                )
            })?;
            return Err(remote_error(
                ProviderKind::Gemini,
                error.status.unwrap_or_else(|| {
                    error
                        .code
                        .map_or_else(|| "remote_error".to_owned(), |code| code.to_string())
                }),
                error.message,
            ));
        }
        let chunk: GeminiChunk = serde_json::from_value(value).map_err(|error| {
            ProviderError::protocol(
                ProviderKind::Gemini,
                format!("invalid streamGenerateContent event: {error}"),
            )
        })?;
        if chunk.candidates.len() > 1 {
            return Err(ProviderError::protocol(
                ProviderKind::Gemini,
                "multiple candidates are not supported",
            ));
        }
        let mut events = Vec::new();
        if let Some(usage) = chunk.usage_metadata {
            events.push(ProviderEvent::Usage(Usage {
                input_tokens: usage.input,
                output_tokens: usage.output,
                total_tokens: usage.total,
            }));
        }
        let Some(candidate) = chunk.candidates.into_iter().next() else {
            return Ok(events);
        };
        if self.completion.is_some() {
            return Err(ProviderError::protocol(
                ProviderKind::Gemini,
                "received candidate data after a finish reason",
            ));
        }
        if candidate.index != 0 {
            return Err(ProviderError::protocol(
                ProviderKind::Gemini,
                "received a non-zero candidate index",
            ));
        }
        if let Some(content) = candidate.content {
            for part in content.parts {
                let WirePart {
                    text,
                    thought,
                    thought_signature,
                    function_call,
                } = part;
                if thought.unwrap_or(false) {
                    if function_call.is_some() {
                        return Err(ProviderError::protocol(
                            ProviderKind::Gemini,
                            "thought part contains a function call",
                        ));
                    }
                    if let Some(signature) = thought_signature {
                        events.push(gemini_continuation(
                            signature,
                            ProviderContinuationPlacement::StandalonePart,
                        )?);
                    }
                    continue;
                }
                match (text, function_call) {
                    (Some(_), Some(_)) => {
                        return Err(ProviderError::protocol(
                            ProviderKind::Gemini,
                            "candidate part contains multiple content variants",
                        ));
                    }
                    (Some(text), None) => {
                        let has_text = !text.is_empty();
                        if has_text {
                            events.push(ProviderEvent::TextDelta(text));
                        }
                        if let Some(signature) = thought_signature {
                            events.push(gemini_continuation(
                                signature,
                                if has_text {
                                    ProviderContinuationPlacement::AttachedToPreviousPart
                                } else {
                                    ProviderContinuationPlacement::StandalonePart
                                },
                            )?);
                        }
                    }
                    (None, Some(call)) => {
                        let argument_bytes = serde_json::to_vec(&call.args).map_err(|error| {
                            ProviderError::serialization(ProviderKind::Gemini, error)
                        })?;
                        if argument_bytes.len() > MAX_TOOL_ARGUMENT_BYTES {
                            return Err(ProviderError::protocol(
                                ProviderKind::Gemini,
                                format!(
                                    "tool call arguments exceed {MAX_TOOL_ARGUMENT_BYTES} bytes"
                                ),
                            ));
                        }
                        let wire_id = call.id;
                        let id = wire_id
                            .clone()
                            .unwrap_or_else(|| format!("chat2db-gemini-{}", Uuid::new_v4()));
                        if self.call_ids.len() == MAX_PROVIDER_TOOL_CALLS_PER_RESPONSE {
                            return Err(ProviderError::protocol(
                                ProviderKind::Gemini,
                                format!(
                                    "response exceeds {MAX_PROVIDER_TOOL_CALLS_PER_RESPONSE} tool calls"
                                ),
                            ));
                        }
                        if id.is_empty() || !self.call_ids.insert(id.clone()) {
                            return Err(ProviderError::protocol(
                                ProviderKind::Gemini,
                                format!("duplicate or empty tool call id {id}"),
                            ));
                        }
                        let identity = ProviderToolCallIdentity::new(ProviderKind::Gemini, wire_id)
                            .map_err(|message| {
                                ProviderError::protocol(ProviderKind::Gemini, message)
                            })?;
                        let call = ToolCall::new(id, call.name, call.args)
                            .map_err(|message| {
                                ProviderError::protocol(ProviderKind::Gemini, message)
                            })?
                            .with_provider_identity(identity);
                        self.saw_tool_call = true;
                        events.push(ProviderEvent::ToolCall(call));
                        if let Some(signature) = thought_signature {
                            events.push(gemini_continuation(
                                signature,
                                ProviderContinuationPlacement::AttachedToPreviousPart,
                            )?);
                        }
                    }
                    (None, None) => {
                        let signature = thought_signature.ok_or_else(|| {
                            ProviderError::protocol(
                                ProviderKind::Gemini,
                                "candidate contains an unknown content part",
                            )
                        })?;
                        events.push(gemini_continuation(
                            signature,
                            ProviderContinuationPlacement::StandalonePart,
                        )?);
                    }
                }
            }
        }
        if let Some(reason) = candidate.finish_reason {
            let mut reason = status_reason(&reason);
            if self.saw_tool_call && reason == StopReason::Stop {
                reason = StopReason::ToolCalls;
            }
            self.completion = Some(reason);
        }
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<ProviderEvent>, ProviderError> {
        if self.done {
            return Ok(Vec::new());
        }
        let reason = self.completion.take().ok_or_else(|| {
            ProviderError::protocol(ProviderKind::Gemini, "stream ended before a finish reason")
        })?;
        self.done = true;
        Ok(vec![ProviderEvent::Completed(reason)])
    }
}

fn gemini_continuation(
    signature: String,
    placement: ProviderContinuationPlacement,
) -> Result<ProviderEvent, ProviderError> {
    ProviderContinuation::new(
        ProviderKind::Gemini,
        ProviderContinuationKind::GeminiThoughtSignature,
        signature,
        placement,
    )
    .map(ProviderEvent::ProviderContinuation)
    .map_err(|message| ProviderError::protocol(ProviderKind::Gemini, message))
}

#[derive(Deserialize)]
struct GeminiChunk {
    #[serde(default)]
    candidates: Vec<Candidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<WireUsage>,
}

#[derive(Deserialize)]
struct Candidate {
    #[serde(default)]
    index: usize,
    content: Option<WireContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct WireContent {
    #[serde(default)]
    parts: Vec<WirePart>,
}

#[derive(Deserialize)]
struct WirePart {
    text: Option<String>,
    thought: Option<bool>,
    #[serde(rename = "thoughtSignature")]
    thought_signature: Option<String>,
    #[serde(rename = "functionCall")]
    function_call: Option<WireFunctionCall>,
}

#[derive(Deserialize)]
struct WireFunctionCall {
    id: Option<String>,
    name: String,
    args: Value,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(default, rename = "promptTokenCount")]
    input: u64,
    #[serde(default, rename = "candidatesTokenCount")]
    output: u64,
    #[serde(default, rename = "totalTokenCount")]
    total: u64,
}

#[derive(Deserialize)]
struct ErrorBody {
    code: Option<u16>,
    message: String,
    status: Option<String>,
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;
    use serde_json::json;

    use super::{GeminiAssembler, GeminiProvider};
    use crate::{
        ApiKey, HttpProviderConfig, MAX_TOOL_ARGUMENT_BYTES, Message, MessageBlock,
        ProviderContinuationPlacement, ProviderEvent, ProviderKind, ProviderRequest, Role,
        StopReason, ToolOutput, ToolResult,
        sse::{SseAssembler, fixture_stream},
    };

    const FIXTURE: &str = include_str!("../../tests/fixtures/gemini_tool_stream.sse");

    #[tokio::test]
    async fn normalizes_text_function_call_usage_and_hides_thoughts() {
        let utf8 = FIXTURE.find('结').expect("fixture contains UTF-8");
        let events = fixture_stream(
            FIXTURE,
            &[1, utf8 + 1, utf8 + 2, FIXTURE.len() / 2],
            GeminiAssembler::default(),
        )
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("valid stream");

        assert!(matches!(&events[1], ProviderEvent::TextDelta(text) if text == "结果"));
        assert!(!format!("{events:?}").contains("private"));
        assert!(
            events.iter().any(
                |event| matches!(event, ProviderEvent::ToolCall(call) if call.name() == "query")
            )
        );
        assert_eq!(
            events.last(),
            Some(&ProviderEvent::Completed(StopReason::ToolCalls))
        );
    }

    #[tokio::test]
    async fn rejects_abnormal_eof() {
        let fixture = "data: {\"candidates\":[{\"index\":0,\"content\":{\"parts\":[{\"text\":\"partial\"}]}}]}\n\n";
        let errors = fixture_stream(fixture, &[], GeminiAssembler::default())
            .filter_map(|item| async move { item.err() })
            .collect::<Vec<_>>()
            .await;
        assert_eq!(errors.len(), 1);
        assert!(errors[0].to_string().contains("finish reason"));
    }

    #[tokio::test]
    async fn surfaces_provider_error() {
        let fixture = "data: {\"error\":{\"code\":429,\"message\":\"quota\",\"status\":\"RESOURCE_EXHAUSTED\"}}\n\n";
        let errors = fixture_stream(fixture, &[], GeminiAssembler::default())
            .filter_map(|item| async move { item.err() })
            .collect::<Vec<_>>()
            .await;
        assert_eq!(errors.len(), 1);
        assert!(errors[0].to_string().contains("RESOURCE_EXHAUSTED"));
    }

    #[test]
    fn request_body_carries_the_configured_output_limit() {
        let provider = GeminiProvider::new(
            HttpProviderConfig::new(
                "https://example.test/v1beta",
                "model",
                ApiKey::new("key").expect("valid key"),
            )
            .expect("valid config"),
        )
        .expect("provider builds")
        .with_max_output_tokens(999)
        .expect("non-zero output limit");
        let body = provider
            .body(&ProviderRequest::new(vec![Message::user("hello")], vec![]))
            .expect("body serializes");
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 999);
        assert!(provider.with_max_output_tokens(0).is_err());
    }

    #[test]
    fn request_replays_thought_signature_and_wraps_text_tool_output() {
        let provider = GeminiProvider::new(
            HttpProviderConfig::new(
                "https://example.test/v1beta",
                "model",
                ApiKey::new("key").expect("valid key"),
            )
            .expect("valid config"),
        )
        .expect("provider builds");
        let mut assembler = GeminiAssembler::default();
        let events = assembler
            .push(eventsource_stream::Event {
                data: json!({
                    "candidates": [{"index": 0, "content": {"parts": [{
                        "thoughtSignature": "opaque-signature",
                        "functionCall": {
                            "name": "query",
                            "args": {"sql": "select 1"}
                        }
                    }]}}]
                })
                .to_string(),
                ..eventsource_stream::Event::default()
            })
            .expect("valid Gemini function call");
        let call = match &events[0] {
            ProviderEvent::ToolCall(call) => call.clone(),
            event => panic!("expected tool call, got {event:?}"),
        };
        let continuation = match &events[1] {
            ProviderEvent::ProviderContinuation(continuation) => continuation.clone(),
            event => panic!("expected continuation, got {event:?}"),
        };
        assert_eq!(continuation.provider(), ProviderKind::Gemini);
        assert_eq!(continuation.value(), "opaque-signature");
        assert_eq!(
            continuation.placement(),
            ProviderContinuationPlacement::AttachedToPreviousPart
        );
        let internal_call_id = call.id().to_owned();

        let body = provider
            .body(&ProviderRequest::new(
                vec![
                    Message::new(
                        Role::Assistant,
                        vec![
                            MessageBlock::ToolCall(call),
                            MessageBlock::ProviderContinuation(continuation),
                        ],
                    ),
                    Message::new(
                        Role::Tool,
                        vec![MessageBlock::ToolResult(ToolResult::new(
                            internal_call_id,
                            ToolOutput::content("ok").expect("bounded output"),
                        ))],
                    ),
                ],
                vec![],
            ))
            .expect("body serializes");
        assert_eq!(
            body["contents"][0]["parts"][0]["thoughtSignature"],
            "opaque-signature"
        );
        assert!(
            body["contents"][0]["parts"][0]["functionCall"]
                .get("id")
                .is_none(),
            "synthetic internal ids must not alter signed Gemini parts"
        );
        assert_eq!(
            body["contents"][1]["parts"][0]["functionResponse"]["response"],
            json!({"content": "ok"})
        );
        assert!(
            body["contents"][1]["parts"][0]["functionResponse"]
                .get("id")
                .is_none(),
            "the response must preserve an absent Gemini wire id"
        );
    }

    #[test]
    fn preserves_text_attached_and_standalone_signatures_in_order() {
        let provider = GeminiProvider::new(
            HttpProviderConfig::new(
                "https://example.test/v1beta",
                "model",
                ApiKey::new("key").expect("valid key"),
            )
            .expect("valid config"),
        )
        .expect("provider builds");
        let mut assembler = GeminiAssembler::default();
        let events = assembler
            .push(eventsource_stream::Event {
                data: json!({
                    "candidates": [{"index": 0, "content": {"parts": [
                        {"text": "answer", "thoughtSignature": "text-signature"},
                        {"thoughtSignature": "standalone-signature"}
                    ]}}]
                })
                .to_string(),
                ..eventsource_stream::Event::default()
            })
            .expect("valid signed text parts");
        let blocks = events
            .into_iter()
            .map(|event| match event {
                ProviderEvent::TextDelta(text) => MessageBlock::Text(text),
                ProviderEvent::ProviderContinuation(continuation) => {
                    MessageBlock::ProviderContinuation(continuation)
                }
                event => panic!("unexpected event {event:?}"),
            })
            .collect();
        let body = provider
            .body(&ProviderRequest::new(
                vec![Message::new(Role::Assistant, blocks)],
                vec![],
            ))
            .expect("body serializes");
        assert_eq!(
            body["contents"][0]["parts"].as_array().map(Vec::len),
            Some(2)
        );
        assert_eq!(
            body["contents"][0]["parts"][0],
            json!({"text": "answer", "thoughtSignature": "text-signature"})
        );
        assert_eq!(
            body["contents"][0]["parts"][1],
            json!({"thoughtSignature": "standalone-signature"})
        );
    }

    #[test]
    fn atomic_function_call_is_bounded_and_duplicate_ids_fail() {
        let mut assembler = GeminiAssembler::default();
        let oversized = eventsource_stream::Event {
            data: json!({
                "candidates": [{"index": 0, "content": {"parts": [{
                    "functionCall": {
                        "id": "tool-1",
                        "name": "query",
                        "args": {"value": "x".repeat(MAX_TOOL_ARGUMENT_BYTES)}
                    }
                }]}}]
            })
            .to_string(),
            ..eventsource_stream::Event::default()
        };
        assert!(assembler.push(oversized).is_err());

        let mut assembler = GeminiAssembler::default();
        for occurrence in 0..2 {
            let result = assembler.push(eventsource_stream::Event {
                data: json!({
                    "candidates": [{"index": 0, "content": {"parts": [{
                        "functionCall": {
                            "id": "same",
                            "name": "query",
                            "args": {"occurrence": occurrence}
                        }
                    }]}}]
                })
                .to_string(),
                ..eventsource_stream::Event::default()
            });
            if occurrence == 0 {
                assert!(result.is_ok());
            } else {
                assert!(result.is_err());
            }
        }
    }

    #[test]
    fn missing_wire_ids_receive_unique_internal_ids_across_responses() {
        let event = || eventsource_stream::Event {
            data: json!({
                "candidates": [{"index": 0, "content": {"parts": [{
                    "functionCall": {"name": "query", "args": {}}
                }]}}]
            })
            .to_string(),
            ..eventsource_stream::Event::default()
        };
        let ids = (0..2)
            .map(|_| {
                let events = GeminiAssembler::default()
                    .push(event())
                    .expect("valid function call");
                match &events[0] {
                    ProviderEvent::ToolCall(call) => {
                        assert_eq!(
                            call.provider_identity()
                                .and_then(|identity| identity.wire_id()),
                            None
                        );
                        call.id().to_owned()
                    }
                    other => panic!("expected tool call, got {other:?}"),
                }
            })
            .collect::<Vec<_>>();
        assert_ne!(ids[0], ids[1]);
    }

    #[tokio::test]
    async fn usage_after_finish_reason_precedes_normalized_completion() {
        let fixture = concat!(
            "data: {\"candidates\":[{\"index\":0,\"content\":{\"parts\":[{\"text\":\"done\"}]},\"finishReason\":\"STOP\"}]}\n\n",
            "data: {\"candidates\":[],\"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":1,\"totalTokenCount\":4}}\n\n",
        );
        let events = fixture_stream(fixture, &[], GeminiAssembler::default())
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
