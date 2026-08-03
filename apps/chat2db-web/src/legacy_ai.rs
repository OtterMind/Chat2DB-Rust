//! Compatibility facade for the retained Community AI workbench.

use std::{
    collections::BTreeMap,
    convert::Infallible,
    io::{Cursor, Read as _},
    path::{Path, PathBuf},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Query, State},
    response::{IntoResponse, Response, sse::Event, sse::KeepAlive, sse::Sse},
    routing::{get, post},
};
use chat2db_contract::{
    AgentEvent, AgentEventEnvelope, AgentMessage, AgentMessageContent, AgentMessageRole,
    AgentPermissionDecision, AgentToolOutput, CreateAgentSessionRequest,
    CreateProviderProfileRequest, DecideAgentPermissionRequest, ProviderCredentials, ProviderKind,
    ProviderProfile, ProviderSecretChange, SqlPermissionMode, StartAgentRunRequest,
    UpdateAgentSessionRequest, UpdateProviderProfileRequest,
};
use chat2db_core::{AgentRunSubscription, Application};
use chrono::{TimeZone as _, Utc};
use futures_util::{Stream, stream};
use quick_xml::{Reader as XmlReader, events::Event as XmlEvent};
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

const DEFAULT_CONTEXT_WINDOW_TOKENS: &str = "128000";
const DEFAULT_MAX_OUTPUT_TOKENS: &str = "4096";
const LEGACY_MESSAGE_PAGE_SIZE: &str = "512";
const SSE_KEEP_ALIVE_SECONDS: u64 = 15;
const MAX_ATTACHMENT_FILE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ATTACHMENT_CONTENT_CHARS: usize = 12_000;
const MAX_ATTACHMENT_CONTEXT_CHARS: usize = 24_000;
const MAX_SHEET_ROWS: usize = 100;
const MAX_SHEET_COLUMNS: usize = 20;

/// Community's historical AI chat request.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAiChatRequest {
    #[serde(default)]
    pub input: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub data_source_id: Option<LegacyAiIdentifier>,
    #[serde(default)]
    pub database_name: Option<String>,
    #[serde(default)]
    pub schema_name: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub model_config_id: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub attachments: Vec<LegacyAiAttachment>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAiAttachment {
    pub file_name: String,
    pub file_type: String,
    pub content_category: String,
    pub content: String,
    #[serde(default)]
    pub content_length: Option<usize>,
    #[serde(default)]
    pub truncated: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAiLocalAttachmentRequest {
    pub file_path: String,
    #[serde(default)]
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAiModelConfigRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub default_config: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAiModelConfigDeleteRequest {
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAiModelConfig {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub max_tokens: u64,
    pub enabled: bool,
    pub default_config: bool,
    pub has_api_key: bool,
    pub api_key_masked: String,
    pub gmt_modified: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAiModelConfigTestResult {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    pub endpoint: String,
}

/// Legacy datasource ids can be either numeric or opaque Rust ids.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum LegacyAiIdentifier {
    Text(String),
    Unsigned(u64),
    Signed(i64),
}

impl LegacyAiIdentifier {
    fn into_string(self) -> String {
        match self {
            Self::Text(value) => value,
            Self::Unsigned(value) => value.to_string(),
            Self::Signed(value) => value.to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAiSessionDeleteRequest {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyAiMessagesQuery {
    session_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAiSession {
    pub id: String,
    pub title: String,
    pub gmt_create: String,
    pub gmt_modified: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAiMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    pub gmt_create: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAiModelOption {
    pub value: String,
    pub label: String,
    pub provider: String,
    pub model: String,
    pub model_config_id: String,
    pub custom_option: bool,
    pub default_option: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAiModelCatalogItem {
    pub provider: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAiStreamChunk {
    #[serde(rename = "type")]
    pub event_type: String,
    pub message_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

impl LegacyAiStreamChunk {
    #[must_use]
    pub fn event_name(&self) -> &str {
        &self.event_type
    }

    fn session(session_id: String) -> Self {
        Self::new("session", current_epoch_millis()).with_session(session_id)
    }

    fn answer(content: String, ts: u64) -> Self {
        Self::new("answer", ts).with_content(content)
    }

    fn done(session_id: String, ts: u64) -> Self {
        Self::new("done", ts)
            .with_content("[DONE]".to_owned())
            .with_session(session_id)
    }

    fn error(code: impl Into<String>, message: impl Into<String>, ts: u64) -> Self {
        let code = code.into();
        let message = message.into();
        Self {
            error_code: Some(code),
            error_message: Some(message.clone()),
            ..Self::new("error", ts).with_content(message)
        }
    }

    fn new(event_type: &str, ts: u64) -> Self {
        Self {
            event_type: event_type.to_owned(),
            message_type: event_type.to_owned(),
            content: None,
            name: None,
            arguments: None,
            session_id: None,
            ts: Some(ts),
            id: None,
            error_code: None,
            error_message: None,
        }
    }

    fn with_content(mut self, content: String) -> Self {
        self.content = Some(content);
        self
    }

    fn with_session(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }
}

/// A started compatibility run with replay-safe subscription already attached.
pub struct LegacyAiStartedRun {
    pub run_id: String,
    pub session_id: String,
    pub subscription: AgentRunSubscription,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyAiFailure {
    pub code: String,
    pub message: String,
}

impl LegacyAiFailure {
    fn invalid(code: &str, message: &str) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }
}

impl From<chat2db_core::AppError> for LegacyAiFailure {
    fn from(error: chat2db_core::AppError) -> Self {
        let error = error.api_error();
        Self {
            code: error.code,
            message: error.message,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyAiEnvelope<T> {
    success: bool,
    data: Option<T>,
    error_code: Option<String>,
    error_message: Option<String>,
}

impl<T> LegacyAiEnvelope<T> {
    fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error_code: None,
            error_message: None,
        }
    }

    fn failure(error: LegacyAiFailure) -> Self {
        Self {
            success: false,
            data: None,
            error_code: Some(error.code),
            error_message: Some(error.message),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyAiListEnvelope<T> {
    success: bool,
    data: Option<Vec<T>>,
    total: Option<usize>,
    error_code: Option<String>,
    error_message: Option<String>,
}

impl<T> LegacyAiListEnvelope<T> {
    fn success(data: Vec<T>) -> Self {
        let total = data.len();
        Self {
            success: true,
            data: Some(data),
            total: Some(total),
            error_code: None,
            error_message: None,
        }
    }

    fn failure(error: LegacyAiFailure) -> Self {
        Self {
            success: false,
            data: None,
            total: None,
            error_code: Some(error.code),
            error_message: Some(error.message),
        }
    }
}

/// Creates or reuses a Community chat session and starts a read-only run.
///
/// # Errors
///
/// Returns request validation, provider-profile, storage, or run-start failures.
pub async fn start_chat_run(
    application: &Application,
    request: LegacyAiChatRequest,
) -> Result<LegacyAiStartedRun, LegacyAiFailure> {
    let input = request.input.trim();
    if input.is_empty() {
        return Err(LegacyAiFailure::invalid(
            "invalid_ai_request",
            "input must not be empty",
        ));
    }
    let message = build_chat_message(input, &request);

    let requested_datasource = request
        .data_source_id
        .clone()
        .map(LegacyAiIdentifier::into_string)
        .filter(|value| !value.trim().is_empty());
    let session = if let Some(session_id) = nonempty(request.session_id.as_deref()) {
        let current = application.get_agent_session(session_id).await?;
        let provider_id = if has_explicit_provider_selection(&request) {
            resolve_provider_profile(application, &request).await?.id
        } else {
            current.provider_id.clone()
        };
        let datasource_id = requested_datasource.or_else(|| current.datasource_id.clone());
        if provider_id != current.provider_id || datasource_id != current.datasource_id {
            application
                .update_agent_session(
                    &current.id,
                    UpdateAgentSessionRequest {
                        expected_revision: current.revision,
                        title: current.title,
                        provider_id,
                        datasource_id,
                    },
                )
                .await?
        } else {
            current
        }
    } else {
        let provider = resolve_provider_profile(application, &request).await?;
        application
            .create_agent_session(CreateAgentSessionRequest {
                title: bounded_title(input),
                provider_id: provider.id,
                datasource_id: requested_datasource,
                system_prompt: nonempty_owned(request.system_prompt),
            })
            .await?
    };

    let accepted = application
        .start_agent_run(StartAgentRunRequest {
            session_id: session.id.clone(),
            message,
            sql_permission_mode: SqlPermissionMode::ReadOnly,
        })
        .await?;
    let subscription = application
        .subscribe_agent_run(&accepted.run_id, None)
        .await?;
    Ok(LegacyAiStartedRun {
        run_id: accepted.run_id,
        session_id: session.id,
        subscription,
    })
}

/// Returns the next frontend-visible compatibility event.
pub async fn next_stream_chunk(
    application: &Application,
    subscription: &mut AgentRunSubscription,
    session_id: &str,
) -> Option<(LegacyAiStreamChunk, bool)> {
    loop {
        let envelope = match subscription.next_event().await {
            Ok(Some(envelope)) => envelope,
            Ok(None) => return None,
            Err(error) => {
                let error = error.api_error();
                return Some((
                    LegacyAiStreamChunk::error(error.code, error.message, current_epoch_millis()),
                    true,
                ));
            }
        };
        if let AgentEvent::PermissionRequested { permission } = &envelope.event {
            let denial = application
                .decide_agent_permission(
                    &permission.permission_id,
                    DecideAgentPermissionRequest {
                        run_id: permission.run_id.clone(),
                        tool_call_id: permission.tool_call_id.clone(),
                        decision: AgentPermissionDecision::Deny,
                        arguments_sha256: permission.arguments_sha256.clone(),
                    },
                )
                .await;
            let message = if denial.is_ok() {
                "The Community compatibility facade rejected a write permission request"
            } else {
                "The Community compatibility facade could not safely resolve a write permission request"
            };
            return Some((
                LegacyAiStreamChunk::error(
                    "agent_write_permission_denied",
                    message,
                    event_timestamp(&envelope),
                ),
                true,
            ));
        }
        if let Some(projected) = project_agent_event(&envelope, session_id) {
            let terminal = matches!(
                envelope.event,
                AgentEvent::Completed { .. }
                    | AgentEvent::Failed { .. }
                    | AgentEvent::Cancelled { .. }
                    | AgentEvent::ToolFailed { .. }
            );
            return Some((projected, terminal));
        }
    }
}

/// Projects a canonical event without exposing private model reasoning.
#[must_use]
pub fn project_agent_event(
    envelope: &AgentEventEnvelope,
    session_id: &str,
) -> Option<LegacyAiStreamChunk> {
    let ts = event_timestamp(envelope);
    match &envelope.event {
        AgentEvent::Started
        | AgentEvent::PermissionResolved { .. }
        | AgentEvent::ContextCompacted { .. }
        | AgentEvent::Usage { .. } => None,
        AgentEvent::TextDelta { delta } => Some(LegacyAiStreamChunk::answer(delta.clone(), ts)),
        AgentEvent::ToolStarted {
            tool_call_id,
            name,
            arguments_sha256,
        } => Some(LegacyAiStreamChunk {
            name: Some(name.clone()),
            arguments: Some(serde_json::json!({ "argumentsSha256": arguments_sha256 }).to_string()),
            id: Some(tool_call_id.clone()),
            ..LegacyAiStreamChunk::new("tool_call", ts)
        }),
        AgentEvent::ToolCompleted {
            tool_call_id,
            name,
            output,
        } => Some(LegacyAiStreamChunk {
            content: Some(tool_output_json(output)),
            name: Some(name.clone()),
            id: Some(tool_call_id.clone()),
            ..LegacyAiStreamChunk::new("tool_result", ts)
        }),
        AgentEvent::ToolFailed { error, .. } | AgentEvent::Failed { error } => Some(
            LegacyAiStreamChunk::error(error.code.clone(), error.message.clone(), ts),
        ),
        AgentEvent::PermissionRequested { .. } => Some(LegacyAiStreamChunk::error(
            "agent_write_permission_denied",
            "The Community compatibility facade rejected a write permission request",
            ts,
        )),
        AgentEvent::Completed { .. } => Some(LegacyAiStreamChunk::done(session_id.to_owned(), ts)),
        AgentEvent::Cancelled { reason } => Some(LegacyAiStreamChunk::error(
            "agent_run_cancelled",
            reason
                .clone()
                .unwrap_or_else(|| "The AI run was cancelled".to_owned()),
            ts,
        )),
    }
}

/// Lists durable sessions in the shape consumed by the retained frontend.
///
/// # Errors
///
/// Returns storage failures while loading the durable session catalog.
pub async fn list_sessions(
    application: &Application,
) -> Result<Vec<LegacyAiSession>, LegacyAiFailure> {
    Ok(application
        .list_agent_sessions()
        .await?
        .items
        .into_iter()
        .map(|session| LegacyAiSession {
            id: session.id,
            title: session.title,
            gmt_create: legacy_timestamp(&session.created_at_ms),
            gmt_modified: legacy_timestamp(&session.updated_at_ms),
        })
        .collect())
}

/// Lists the complete visible transcript for one Community session.
///
/// # Errors
///
/// Returns validation, storage, or transcript pagination failures.
pub async fn list_messages(
    application: &Application,
    session_id: &str,
) -> Result<Vec<LegacyAiMessage>, LegacyAiFailure> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Err(LegacyAiFailure::invalid(
            "invalid_ai_request",
            "sessionId must not be empty",
        ));
    }
    let mut start_ordinal = "0".to_owned();
    let mut messages = Vec::new();
    loop {
        let page = application
            .list_agent_messages(session_id, &start_ordinal, LEGACY_MESSAGE_PAGE_SIZE)
            .await?;
        let next_ordinal = page
            .items
            .last()
            .and_then(|message| message.ordinal.parse::<u64>().ok())
            .and_then(|ordinal| ordinal.checked_add(1));
        messages.extend(page.items.into_iter().filter_map(project_message));
        if !page.has_more {
            break;
        }
        let Some(next_ordinal) = next_ordinal else {
            return Err(LegacyAiFailure::invalid(
                "agent_message_ordinal_invalid",
                "The AI transcript contains an invalid message ordinal",
            ));
        };
        start_ordinal = next_ordinal.to_string();
    }
    Ok(messages)
}

/// Deletes one durable Community AI session.
///
/// # Errors
///
/// Returns lookup, revision, or storage failures.
pub async fn delete_session(application: &Application, id: &str) -> Result<(), LegacyAiFailure> {
    let session = application.get_agent_session(id.trim()).await?;
    application
        .delete_agent_session(&session.id, &session.revision)
        .await?;
    Ok(())
}

/// Lists provider profiles that have usable credentials as frontend model options.
///
/// # Errors
///
/// Returns storage failures while loading provider profiles.
pub async fn model_options(
    application: &Application,
) -> Result<Vec<LegacyAiModelOption>, LegacyAiFailure> {
    Ok(application
        .list_provider_profiles()
        .await?
        .items
        .into_iter()
        .filter(|profile| profile.has_secret)
        .enumerate()
        .map(|(index, profile)| LegacyAiModelOption {
            value: format!("config:{}", profile.id),
            label: profile.name,
            provider: legacy_provider_name(profile.kind).to_owned(),
            model: profile.model,
            model_config_id: profile.id,
            custom_option: true,
            default_option: index == 0,
        })
        .collect())
}

/// Builds the provider/model catalog represented by saved profiles.
///
/// # Errors
///
/// Returns storage failures while loading provider profiles.
pub async fn model_catalog(
    application: &Application,
) -> Result<Vec<LegacyAiModelCatalogItem>, LegacyAiFailure> {
    let mut catalog = BTreeMap::<String, Vec<String>>::new();
    for profile in application.list_provider_profiles().await?.items {
        let models = catalog
            .entry(legacy_provider_name(profile.kind).to_owned())
            .or_default();
        if !models.contains(&profile.model) {
            models.push(profile.model);
        }
    }
    Ok(catalog
        .into_iter()
        .map(|(provider, models)| LegacyAiModelCatalogItem { provider, models })
        .collect())
}

/// Lists secret-free model configurations for the retained settings UI.
///
/// # Errors
///
/// Returns storage failures while loading provider profiles.
pub async fn list_model_configs(
    application: &Application,
) -> Result<Vec<LegacyAiModelConfig>, LegacyAiFailure> {
    Ok(application
        .list_provider_profiles()
        .await?
        .items
        .into_iter()
        .enumerate()
        .map(|(index, profile)| model_config_projection(profile, index == 0))
        .collect())
}

/// Creates or updates one model configuration without exposing its credential.
///
/// # Errors
///
/// Returns validation, revision, vault, or storage failures.
pub async fn save_model_config(
    application: &Application,
    request: LegacyAiModelConfigRequest,
) -> Result<LegacyAiModelConfig, LegacyAiFailure> {
    let kind = parse_provider_kind(&request.provider)?;
    let model = nonempty(Some(&request.model))
        .ok_or_else(|| LegacyAiFailure::invalid("invalid_ai_model", "model must not be empty"))?;
    let name = nonempty(Some(&request.name)).unwrap_or(model).to_owned();
    let max_output_tokens = request.max_tokens.unwrap_or(4096);
    if max_output_tokens == 0 {
        return Err(LegacyAiFailure::invalid(
            "invalid_ai_model",
            "maxTokens must be greater than zero",
        ));
    }

    let profile = if let Some(id) = nonempty(request.id.as_deref()) {
        let current = application.get_provider_profile(id).await?;
        let base_url = nonempty(request.base_url.as_deref())
            .map(ToOwned::to_owned)
            .unwrap_or(current.base_url);
        let secret_change = match nonempty(request.api_key.as_deref()) {
            Some(api_key) => ProviderSecretChange::Replace {
                credentials: ProviderCredentials {
                    api_key: api_key.to_owned(),
                },
            },
            None => ProviderSecretChange::Keep,
        };
        application
            .update_provider_profile(
                id,
                UpdateProviderProfileRequest {
                    expected_revision: current.revision,
                    name,
                    kind,
                    base_url,
                    model: model.to_owned(),
                    context_window_tokens: current.context_window_tokens,
                    max_output_tokens: max_output_tokens.to_string(),
                    secret_change,
                },
            )
            .await?
    } else {
        let api_key = nonempty(request.api_key.as_deref()).ok_or_else(|| {
            LegacyAiFailure::invalid(
                "provider_credentials_missing",
                "API Key is required when creating an AI model configuration",
            )
        })?;
        application
            .create_provider_profile(CreateProviderProfileRequest {
                name,
                kind,
                base_url: nonempty(request.base_url.as_deref())
                    .map_or_else(|| default_base_url(kind).to_owned(), ToOwned::to_owned),
                model: model.to_owned(),
                context_window_tokens: DEFAULT_CONTEXT_WINDOW_TOKENS.to_owned(),
                max_output_tokens: max_output_tokens.to_string(),
                credentials: Some(ProviderCredentials {
                    api_key: api_key.to_owned(),
                }),
            })
            .await?
    };
    let default_config = request.default_config.unwrap_or(false)
        || application.list_provider_profiles().await?.items.len() == 1;
    Ok(model_config_projection(profile, default_config))
}

/// Deletes one saved model configuration.
///
/// # Errors
///
/// Returns lookup, revision, vault, or storage failures.
pub async fn delete_model_config(
    application: &Application,
    id: &str,
) -> Result<(), LegacyAiFailure> {
    let profile = application.get_provider_profile(id.trim()).await?;
    application
        .delete_provider_profile(&profile.id, &profile.revision)
        .await?;
    Ok(())
}

pub async fn test_model_config(
    request: &LegacyAiModelConfigRequest,
) -> LegacyAiModelConfigTestResult {
    let kind = match parse_provider_kind(&request.provider) {
        Ok(kind) => kind,
        Err(error) => {
            return LegacyAiModelConfigTestResult {
                success: false,
                message: error.message,
                status_code: None,
                endpoint: String::new(),
            };
        }
    };
    if kind != ProviderKind::OpenAiCompatible {
        return LegacyAiModelConfigTestResult {
            success: false,
            message: "Connection test currently supports OpenAI-compatible models only.".to_owned(),
            status_code: None,
            endpoint: String::new(),
        };
    }
    let base_url = nonempty(request.base_url.as_deref()).unwrap_or_else(|| default_base_url(kind));
    let endpoint = format!("{}/chat/completions", normalized_url(base_url));
    let Some(api_key) = nonempty(request.api_key.as_deref()) else {
        return LegacyAiModelConfigTestResult {
            success: false,
            message: "API Key is required for the connection test.".to_owned(),
            status_code: None,
            endpoint,
        };
    };
    let Some(model) = nonempty(Some(&request.model)) else {
        return LegacyAiModelConfigTestResult {
            success: false,
            message: "model must not be empty".to_owned(),
            status_code: None,
            endpoint,
        };
    };
    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return LegacyAiModelConfigTestResult {
                success: false,
                message: bounded_error_message(&error.to_string()),
                status_code: None,
                endpoint,
            };
        }
    };
    let mut payload = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 1,
    });
    if let Some(temperature) = request.temperature.filter(|value| value.is_finite()) {
        payload["temperature"] = serde_json::json!(temperature);
    }
    match client
        .post(&endpoint)
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => LegacyAiModelConfigTestResult {
            success: true,
            message: "Connection test passed".to_owned(),
            status_code: Some(response.status().as_u16()),
            endpoint,
        },
        Ok(response) => {
            let status_code = response.status().as_u16();
            let message = response
                .text()
                .await
                .unwrap_or_else(|error| error.to_string());
            LegacyAiModelConfigTestResult {
                success: false,
                message: bounded_error_message(&message),
                status_code: Some(status_code),
                endpoint,
            }
        }
        Err(error) => LegacyAiModelConfigTestResult {
            success: false,
            message: bounded_error_message(&error.to_string()),
            status_code: error.status().map(|status| status.as_u16()),
            endpoint,
        },
    }
}

/// Reads and parses one attachment selected by its desktop-local path.
///
/// # Errors
///
/// Returns path, size, I/O, unsupported-format, or document parsing failures.
pub async fn parse_local_attachment(
    request: LegacyAiLocalAttachmentRequest,
) -> Result<LegacyAiAttachment, LegacyAiFailure> {
    let file_path = nonempty(Some(&request.file_path)).ok_or_else(|| {
        LegacyAiFailure::invalid("ai_attachment_path_required", "filePath must not be empty")
    })?;
    let path = PathBuf::from(file_path);
    let metadata = tokio::fs::metadata(&path).await.map_err(|_| {
        LegacyAiFailure::invalid(
            "ai_attachment_file_not_found",
            "The selected attachment does not exist",
        )
    })?;
    if !metadata.is_file() {
        return Err(LegacyAiFailure::invalid(
            "ai_attachment_file_not_found",
            "The selected attachment is not a file",
        ));
    }
    if metadata.len() > MAX_ATTACHMENT_FILE_BYTES as u64 {
        return Err(LegacyAiFailure::invalid(
            "ai_attachment_too_large",
            "Attachments are limited to 32 MiB",
        ));
    }
    let bytes = tokio::fs::read(&path).await.map_err(|_| {
        LegacyAiFailure::invalid(
            "ai_attachment_read_failed",
            "The selected attachment could not be read",
        )
    })?;
    let file_name = nonempty(request.file_name.as_deref())
        .map(ToOwned::to_owned)
        .or_else(|| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "attachment".to_owned());
    parse_attachment_bytes(&file_name, &bytes)
}

/// Parses bounded attachment bytes into the historical frontend shape.
///
/// # Errors
///
/// Returns size, unsupported-format, empty-content, or document parsing failures.
pub fn parse_attachment_bytes(
    file_name: &str,
    bytes: &[u8],
) -> Result<LegacyAiAttachment, LegacyAiFailure> {
    if bytes.len() > MAX_ATTACHMENT_FILE_BYTES {
        return Err(LegacyAiFailure::invalid(
            "ai_attachment_too_large",
            "Attachments are limited to 32 MiB",
        ));
    }
    let extension = Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| {
            LegacyAiFailure::invalid(
                "ai_attachment_unsupported",
                "The attachment must have a supported file extension",
            )
        })?;
    let raw_content = match extension.as_str() {
        "md" | "txt" | "json" => String::from_utf8_lossy(bytes).into_owned(),
        "csv" => parse_csv_attachment(bytes),
        "docx" => parse_docx_attachment(bytes)?,
        "xls" | "xlsx" => parse_workbook_attachment(bytes, &extension)?,
        "pdf" | "doc" => extract_binary_text(bytes),
        _ => {
            return Err(LegacyAiFailure::invalid(
                "ai_attachment_unsupported",
                "Supported attachments are PDF, DOC, DOCX, MD, TXT, JSON, CSV, XLS, and XLSX",
            ));
        }
    };
    let normalized = normalize_attachment_text(&raw_content);
    if normalized.is_empty() {
        return Err(LegacyAiFailure::invalid(
            "ai_attachment_empty",
            "The attachment does not contain readable text",
        ));
    }
    let content_length = normalized.chars().count();
    let truncated = content_length > MAX_ATTACHMENT_CONTENT_CHARS;
    let content = normalized
        .chars()
        .take(MAX_ATTACHMENT_CONTENT_CHARS)
        .collect();
    Ok(LegacyAiAttachment {
        file_name: file_name.to_owned(),
        file_type: extension.clone(),
        content_category: if matches!(extension.as_str(), "csv" | "xls" | "xlsx") {
            "TABULAR"
        } else {
            "DOCUMENT"
        }
        .to_owned(),
        content,
        content_length: Some(content_length),
        truncated: Some(truncated),
    })
}

/// Handles non-streaming legacy AI requests for the Tauri `javaQuery` bridge.
#[allow(clippy::too_many_lines)]
pub async fn dispatch(
    application: &Application,
    method: &str,
    request_url: &str,
    message: serde_json::Value,
) -> Option<serde_json::Value> {
    let path = request_url.split('?').next().unwrap_or(request_url);
    let method = method.to_ascii_lowercase();
    match (method.as_str(), path) {
        ("get", "/api/v3/ai/chat/history/sessions") => Some(
            serde_json::to_value(match list_sessions(application).await {
                Ok(data) => LegacyAiListEnvelope::success(data),
                Err(error) => LegacyAiListEnvelope::failure(error),
            })
            .unwrap_or_else(|_| internal_failure_value()),
        ),
        ("get", "/api/v3/ai/chat/history/messages") => {
            let session_id = message
                .get("sessionId")
                .and_then(serde_json::Value::as_str)
                .or_else(|| query_value(request_url, "sessionId"));
            let result = match session_id {
                Some(session_id) => list_messages(application, session_id).await,
                None => Err(LegacyAiFailure::invalid(
                    "invalid_ai_request",
                    "sessionId must not be empty",
                )),
            };
            Some(
                serde_json::to_value(match result {
                    Ok(data) => LegacyAiListEnvelope::success(data),
                    Err(error) => LegacyAiListEnvelope::failure(error),
                })
                .unwrap_or_else(|_| internal_failure_value()),
            )
        }
        ("post", "/api/v3/ai/chat/history/session/delete") => {
            let request = serde_json::from_value::<LegacyAiSessionDeleteRequest>(message);
            let result = match request {
                Ok(request) => delete_session(application, &request.id).await,
                Err(_) => Err(LegacyAiFailure::invalid(
                    "invalid_ai_request",
                    "id must not be empty",
                )),
            };
            Some(
                serde_json::to_value(match result {
                    Ok(()) => LegacyAiEnvelope::success(()),
                    Err(error) => LegacyAiEnvelope::failure(error),
                })
                .unwrap_or_else(|_| internal_failure_value()),
            )
        }
        ("get", "/api/v3/ai/model/options") => Some(
            serde_json::to_value(match model_options(application).await {
                Ok(data) => LegacyAiEnvelope::success(data),
                Err(error) => LegacyAiEnvelope::failure(error),
            })
            .unwrap_or_else(|_| internal_failure_value()),
        ),
        ("get", "/api/v3/ai/model/list") => Some(
            serde_json::to_value(match model_catalog(application).await {
                Ok(data) => LegacyAiEnvelope::success(data),
                Err(error) => LegacyAiEnvelope::failure(error),
            })
            .unwrap_or_else(|_| internal_failure_value()),
        ),
        ("get", "/api/v3/ai/model/config/list") => Some(
            serde_json::to_value(match list_model_configs(application).await {
                Ok(data) => LegacyAiEnvelope::success(data),
                Err(error) => LegacyAiEnvelope::failure(error),
            })
            .unwrap_or_else(|_| internal_failure_value()),
        ),
        ("post", "/api/v3/ai/model/config/save") => {
            let result = match serde_json::from_value::<LegacyAiModelConfigRequest>(message) {
                Ok(request) => save_model_config(application, request).await,
                Err(_) => Err(LegacyAiFailure::invalid(
                    "invalid_ai_request",
                    "The AI model configuration is invalid",
                )),
            };
            Some(
                serde_json::to_value(match result {
                    Ok(data) => LegacyAiEnvelope::success(data),
                    Err(error) => LegacyAiEnvelope::failure(error),
                })
                .unwrap_or_else(|_| internal_failure_value()),
            )
        }
        ("post", "/api/v3/ai/model/config/delete") => {
            let result = match serde_json::from_value::<LegacyAiModelConfigDeleteRequest>(message) {
                Ok(request) => delete_model_config(application, &request.id).await,
                Err(_) => Err(LegacyAiFailure::invalid(
                    "invalid_ai_request",
                    "id must not be empty",
                )),
            };
            Some(
                serde_json::to_value(match result {
                    Ok(()) => LegacyAiEnvelope::success(()),
                    Err(error) => LegacyAiEnvelope::failure(error),
                })
                .unwrap_or_else(|_| internal_failure_value()),
            )
        }
        ("post", "/api/v3/ai/model/config/test") => {
            let result = match serde_json::from_value::<LegacyAiModelConfigRequest>(message) {
                Ok(request) => LegacyAiEnvelope::success(test_model_config(&request).await),
                Err(_) => LegacyAiEnvelope::failure(LegacyAiFailure::invalid(
                    "invalid_ai_request",
                    "The AI model configuration is invalid",
                )),
            };
            Some(serde_json::to_value(result).unwrap_or_else(|_| internal_failure_value()))
        }
        ("post", "/api/v3/ai/chat/attachment/parse/local") => {
            let result = match serde_json::from_value::<LegacyAiLocalAttachmentRequest>(message) {
                Ok(request) => parse_local_attachment(request).await,
                Err(_) => Err(LegacyAiFailure::invalid(
                    "invalid_ai_request",
                    "filePath must not be empty",
                )),
            };
            Some(
                serde_json::to_value(match result {
                    Ok(data) => LegacyAiEnvelope::success(data),
                    Err(error) => LegacyAiEnvelope::failure(error),
                })
                .unwrap_or_else(|_| internal_failure_value()),
            )
        }
        _ => None,
    }
}

pub(crate) fn routes() -> Router<Application> {
    Router::new()
        .route("/api/v3/ai/chat/stream", post(chat_stream_handler))
        .route(
            "/api/v3/ai/chat/history/sessions",
            get(list_sessions_handler),
        )
        .route(
            "/api/v3/ai/chat/history/messages",
            get(list_messages_handler),
        )
        .route(
            "/api/v3/ai/chat/history/session/delete",
            post(delete_session_handler),
        )
        .route("/api/v3/ai/model/options", get(model_options_handler))
        .route("/api/v3/ai/model/list", get(model_catalog_handler))
        .route(
            "/api/v3/ai/model/config/list",
            get(list_model_configs_handler),
        )
        .route(
            "/api/v3/ai/model/config/save",
            post(save_model_config_handler),
        )
        .route(
            "/api/v3/ai/model/config/delete",
            post(delete_model_config_handler),
        )
        .route(
            "/api/v3/ai/model/config/test",
            post(test_model_config_handler),
        )
        .route(
            "/api/v3/ai/chat/attachment/parse/upload",
            post(parse_uploaded_attachment_handler)
                .layer(DefaultBodyLimit::max(MAX_ATTACHMENT_FILE_BYTES)),
        )
}

async fn chat_stream_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyAiChatRequest>,
) -> Response {
    let started = match start_chat_run(&application, request).await {
        Ok(started) => started,
        Err(error) => return Json(LegacyAiEnvelope::<()>::failure(error)).into_response(),
    };
    let events = legacy_ai_stream(application, started);
    Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(SSE_KEEP_ALIVE_SECONDS))
                .text("keep-alive"),
        )
        .into_response()
}

fn legacy_ai_stream(
    application: Application,
    started: LegacyAiStartedRun,
) -> impl Stream<Item = Result<Event, Infallible>> {
    struct StateData {
        application: Application,
        subscription: AgentRunSubscription,
        session_id: String,
        initial: Option<LegacyAiStreamChunk>,
        finished: bool,
    }

    stream::unfold(
        StateData {
            application,
            subscription: started.subscription,
            session_id: started.session_id.clone(),
            initial: Some(LegacyAiStreamChunk::session(started.session_id)),
            finished: false,
        },
        |mut state| async move {
            if state.finished {
                return None;
            }
            if let Some(chunk) = state.initial.take() {
                return Some((Ok(sse_event(&chunk)), state));
            }
            let (chunk, terminal) = next_stream_chunk(
                &state.application,
                &mut state.subscription,
                &state.session_id,
            )
            .await?;
            state.finished = terminal;
            Some((Ok(sse_event(&chunk)), state))
        },
    )
}

fn sse_event(chunk: &LegacyAiStreamChunk) -> Event {
    let data = serde_json::to_string(chunk).unwrap_or_else(|_| {
        r#"{"type":"error","messageType":"error","content":"AI event serialization failed"}"#
            .to_owned()
    });
    Event::default().event(chunk.event_name()).data(data)
}

async fn list_sessions_handler(State(application): State<Application>) -> Json<serde_json::Value> {
    Json(
        serde_json::to_value(match list_sessions(&application).await {
            Ok(data) => LegacyAiListEnvelope::success(data),
            Err(error) => LegacyAiListEnvelope::failure(error),
        })
        .unwrap_or_else(|_| internal_failure_value()),
    )
}

async fn list_messages_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyAiMessagesQuery>,
) -> Json<serde_json::Value> {
    Json(
        serde_json::to_value(match list_messages(&application, &query.session_id).await {
            Ok(data) => LegacyAiListEnvelope::success(data),
            Err(error) => LegacyAiListEnvelope::failure(error),
        })
        .unwrap_or_else(|_| internal_failure_value()),
    )
}

async fn delete_session_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyAiSessionDeleteRequest>,
) -> Json<serde_json::Value> {
    Json(
        serde_json::to_value(match delete_session(&application, &request.id).await {
            Ok(()) => LegacyAiEnvelope::success(()),
            Err(error) => LegacyAiEnvelope::failure(error),
        })
        .unwrap_or_else(|_| internal_failure_value()),
    )
}

async fn model_options_handler(State(application): State<Application>) -> Json<serde_json::Value> {
    Json(
        serde_json::to_value(match model_options(&application).await {
            Ok(data) => LegacyAiEnvelope::success(data),
            Err(error) => LegacyAiEnvelope::failure(error),
        })
        .unwrap_or_else(|_| internal_failure_value()),
    )
}

async fn model_catalog_handler(State(application): State<Application>) -> Json<serde_json::Value> {
    Json(
        serde_json::to_value(match model_catalog(&application).await {
            Ok(data) => LegacyAiEnvelope::success(data),
            Err(error) => LegacyAiEnvelope::failure(error),
        })
        .unwrap_or_else(|_| internal_failure_value()),
    )
}

async fn list_model_configs_handler(
    State(application): State<Application>,
) -> Json<serde_json::Value> {
    Json(
        serde_json::to_value(match list_model_configs(&application).await {
            Ok(data) => LegacyAiEnvelope::success(data),
            Err(error) => LegacyAiEnvelope::failure(error),
        })
        .unwrap_or_else(|_| internal_failure_value()),
    )
}

async fn save_model_config_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyAiModelConfigRequest>,
) -> Json<serde_json::Value> {
    Json(
        serde_json::to_value(match save_model_config(&application, request).await {
            Ok(data) => LegacyAiEnvelope::success(data),
            Err(error) => LegacyAiEnvelope::failure(error),
        })
        .unwrap_or_else(|_| internal_failure_value()),
    )
}

async fn delete_model_config_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyAiModelConfigDeleteRequest>,
) -> Json<serde_json::Value> {
    Json(
        serde_json::to_value(match delete_model_config(&application, &request.id).await {
            Ok(()) => LegacyAiEnvelope::success(()),
            Err(error) => LegacyAiEnvelope::failure(error),
        })
        .unwrap_or_else(|_| internal_failure_value()),
    )
}

async fn test_model_config_handler(
    Json(request): Json<LegacyAiModelConfigRequest>,
) -> Json<LegacyAiEnvelope<LegacyAiModelConfigTestResult>> {
    Json(LegacyAiEnvelope::success(test_model_config(&request).await))
}

async fn parse_uploaded_attachment_handler(mut multipart: Multipart) -> Json<serde_json::Value> {
    let result = async {
        while let Some(field) = multipart.next_field().await.map_err(|_| {
            LegacyAiFailure::invalid(
                "ai_attachment_upload_invalid",
                "The attachment upload is invalid",
            )
        })? {
            if field.name() != Some("file") {
                continue;
            }
            let file_name = field
                .file_name()
                .map_or_else(|| "attachment".to_owned(), ToOwned::to_owned);
            let bytes = field.bytes().await.map_err(|_| {
                LegacyAiFailure::invalid(
                    "ai_attachment_upload_invalid",
                    "The uploaded attachment could not be read",
                )
            })?;
            return parse_attachment_bytes(&file_name, &bytes);
        }
        Err(LegacyAiFailure::invalid(
            "ai_attachment_upload_invalid",
            "The multipart request must include a file field",
        ))
    }
    .await;
    Json(
        serde_json::to_value(match result {
            Ok(data) => LegacyAiEnvelope::success(data),
            Err(error) => LegacyAiEnvelope::failure(error),
        })
        .unwrap_or_else(|_| internal_failure_value()),
    )
}

#[allow(clippy::too_many_lines)]
async fn resolve_provider_profile(
    application: &Application,
    request: &LegacyAiChatRequest,
) -> Result<ProviderProfile, LegacyAiFailure> {
    let profiles = application.list_provider_profiles().await?.items;
    if let Some(profile_id) = nonempty(request.model_config_id.as_deref()) {
        let profile_id = profile_id.strip_prefix("config:").unwrap_or(profile_id);
        let profile = profiles
            .into_iter()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| {
                LegacyAiFailure::invalid(
                    "provider_not_found",
                    "The selected AI model configuration does not exist",
                )
            })?;
        if !profile.has_secret {
            return Err(LegacyAiFailure::invalid(
                "provider_credentials_missing",
                "The selected AI model configuration does not have an API key",
            ));
        }
        return Ok(profile);
    }

    let requested_kind = request
        .provider
        .as_deref()
        .map(parse_provider_kind)
        .transpose()?;
    let requested_model = nonempty(request.model.as_deref());
    let requested_base_url = nonempty(request.base_url.as_deref());
    let mut matching = profiles.into_iter().find(|profile| {
        profile.has_secret
            && requested_kind.is_none_or(|kind| profile.kind == kind)
            && requested_model.is_none_or(|model| profile.model == model)
            && requested_base_url.is_none_or(|base_url| {
                normalized_url(&profile.base_url) == normalized_url(base_url)
            })
    });

    if let Some(api_key) = nonempty(request.api_key.as_deref()) {
        if let Some(profile) = matching.take() {
            return application
                .update_provider_profile(
                    &profile.id,
                    UpdateProviderProfileRequest {
                        expected_revision: profile.revision,
                        name: profile.name,
                        kind: profile.kind,
                        base_url: profile.base_url,
                        model: profile.model,
                        context_window_tokens: profile.context_window_tokens,
                        max_output_tokens: request
                            .max_tokens
                            .map_or(profile.max_output_tokens, |tokens| tokens.to_string()),
                        secret_change: ProviderSecretChange::Replace {
                            credentials: ProviderCredentials {
                                api_key: api_key.to_owned(),
                            },
                        },
                    },
                )
                .await
                .map_err(Into::into);
        }
        let kind = requested_kind.ok_or_else(|| {
            LegacyAiFailure::invalid(
                "invalid_ai_provider",
                "provider is required when creating an AI model configuration",
            )
        })?;
        let model = requested_model.ok_or_else(|| {
            LegacyAiFailure::invalid(
                "invalid_ai_model",
                "model is required when creating an AI model configuration",
            )
        })?;
        let base_url =
            requested_base_url.map_or_else(|| default_base_url(kind).to_owned(), ToOwned::to_owned);
        return application
            .create_provider_profile(CreateProviderProfileRequest {
                name: format!("Community {} {model}", legacy_provider_name(kind)),
                kind,
                base_url,
                model: model.to_owned(),
                context_window_tokens: DEFAULT_CONTEXT_WINDOW_TOKENS.to_owned(),
                max_output_tokens: request.max_tokens.map_or_else(
                    || DEFAULT_MAX_OUTPUT_TOKENS.to_owned(),
                    |tokens| tokens.to_string(),
                ),
                credentials: Some(ProviderCredentials {
                    api_key: api_key.to_owned(),
                }),
            })
            .await
            .map_err(Into::into);
    }

    matching.ok_or_else(|| {
        LegacyAiFailure::invalid(
            "provider_credentials_missing",
            "Configure an AI provider with an API key before starting a chat",
        )
    })
}

fn project_message(message: AgentMessage) -> Option<LegacyAiMessage> {
    let (role, content, reasoning_content) = match message.role {
        AgentMessageRole::User => ("user", text_content(&message.content), None),
        AgentMessageRole::Assistant => (
            "assistant",
            text_content(&message.content),
            trace_content(&message.content),
        ),
        AgentMessageRole::Tool => ("assistant", String::new(), trace_content(&message.content)),
        AgentMessageRole::System | AgentMessageRole::Summary => return None,
    };
    Some(LegacyAiMessage {
        id: message.id,
        session_id: message.session_id,
        role: role.to_owned(),
        content,
        reasoning_content,
        gmt_create: legacy_timestamp(&message.created_at_ms),
    })
}

fn model_config_projection(profile: ProviderProfile, default_config: bool) -> LegacyAiModelConfig {
    LegacyAiModelConfig {
        id: profile.id,
        name: profile.name,
        provider: legacy_provider_name(profile.kind).to_owned(),
        model: profile.model,
        base_url: profile.base_url,
        max_tokens: profile.max_output_tokens.parse().unwrap_or(4096),
        enabled: true,
        default_config,
        has_api_key: profile.has_secret,
        api_key_masked: if profile.has_secret { "****" } else { "" }.to_owned(),
        gmt_modified: legacy_timestamp(&profile.updated_at_ms),
    }
}

fn build_chat_message(input: &str, request: &LegacyAiChatRequest) -> String {
    let mut message = input.to_owned();
    let database = nonempty(request.database_name.as_deref());
    let schema = nonempty(request.schema_name.as_deref());
    if database.is_some() || schema.is_some() {
        message.push_str("\n\nCurrent database context:\n");
        if let Some(database) = database {
            message.push_str("- Database: ");
            message.push_str(database);
            message.push('\n');
        }
        if let Some(schema) = schema {
            message.push_str("- Schema: ");
            message.push_str(schema);
            message.push('\n');
        }
    }
    let mut remaining = MAX_ATTACHMENT_CONTEXT_CHARS;
    for (index, attachment) in request.attachments.iter().enumerate() {
        if remaining == 0 {
            break;
        }
        let normalized = normalize_attachment_text(&attachment.content);
        if normalized.is_empty() {
            continue;
        }
        let content = normalized.chars().take(remaining).collect::<String>();
        let used = content.chars().count();
        remaining = remaining.saturating_sub(used);
        message.push_str("\n\n### Attachment ");
        message.push_str(&(index + 1).to_string());
        message.push_str("\n- File name: ");
        message.push_str(&attachment.file_name);
        message.push_str("\n- File type: ");
        message.push_str(&attachment.file_type);
        message.push_str("\n- Content category: ");
        message.push_str(&attachment.content_category);
        message.push_str("\n- Truncated: ");
        message.push_str(
            if attachment.truncated.unwrap_or(false) || used < normalized.chars().count() {
                "true"
            } else {
                "false"
            },
        );
        message.push_str("\n```text\n");
        message.push_str(&content);
        message.push_str("\n```");
    }
    message
}

fn parse_csv_attachment(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines = text.lines().collect::<Vec<_>>();
    let limit = lines.len().min(MAX_SHEET_ROWS + 1);
    let mut output = String::from("[CSV]\n");
    for line in &lines[..limit] {
        output.push_str(line);
        output.push('\n');
    }
    if lines.len() > limit {
        output.push_str("... omitted ");
        output.push_str(&(lines.len() - limit).to_string());
        output.push_str(" more rows");
    }
    output
}

fn parse_docx_attachment(bytes: &[u8]) -> Result<String, LegacyAiFailure> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| {
        LegacyAiFailure::invalid(
            "ai_attachment_parse_failed",
            "The DOCX attachment is invalid",
        )
    })?;
    let mut document = archive.by_name("word/document.xml").map_err(|_| {
        LegacyAiFailure::invalid(
            "ai_attachment_parse_failed",
            "The DOCX attachment does not contain a document body",
        )
    })?;
    let mut xml = String::new();
    document.read_to_string(&mut xml).map_err(|_| {
        LegacyAiFailure::invalid(
            "ai_attachment_parse_failed",
            "The DOCX attachment could not be read",
        )
    })?;
    let mut reader = XmlReader::from_str(&xml);
    reader.config_mut().trim_text(false);
    let mut output = String::new();
    loop {
        match reader.read_event() {
            Ok(XmlEvent::Text(text)) => {
                let text = text.unescape().map_err(|_| {
                    LegacyAiFailure::invalid(
                        "ai_attachment_parse_failed",
                        "The DOCX attachment contains invalid text",
                    )
                })?;
                output.push_str(&text);
            }
            Ok(XmlEvent::End(element)) => match xml_local_name(element.name().as_ref()) {
                b"p" | b"tr" => output.push('\n'),
                b"tc" => output.push('\t'),
                _ => {}
            },
            Ok(XmlEvent::Eof) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(LegacyAiFailure::invalid(
                    "ai_attachment_parse_failed",
                    "The DOCX attachment contains invalid XML",
                ));
            }
        }
    }
    Ok(output)
}

fn parse_workbook_attachment(bytes: &[u8], extension: &str) -> Result<String, LegacyAiFailure> {
    let cursor = Cursor::new(bytes.to_vec());
    let workbook = if extension == "xls" {
        xls::core::xls::read(cursor)
    } else {
        xls::core::xlsx::read(cursor)
    }
    .map_err(|_| {
        LegacyAiFailure::invalid(
            "ai_attachment_parse_failed",
            "The spreadsheet attachment is invalid",
        )
    })?;
    let mut output = String::new();
    for (sheet_index, sheet) in workbook.sheets.iter().enumerate() {
        output.push_str("[Sheet] ");
        output.push_str(&sheet.name);
        output.push('\n');
        let (rows, columns) = sheet.dimensions();
        let row_limit = usize::try_from(rows)
            .unwrap_or(usize::MAX)
            .min(MAX_SHEET_ROWS);
        let column_limit = usize::try_from(columns)
            .unwrap_or(usize::MAX)
            .min(MAX_SHEET_COLUMNS);
        for row in 0..row_limit {
            output.push_str("Row ");
            output.push_str(&(row + 1).to_string());
            output.push_str(": ");
            for column in 0..column_limit {
                if column > 0 {
                    output.push_str(" | ");
                }
                output.push_str(&workbook.display_cell(
                    sheet_index,
                    u32::try_from(row).unwrap_or(u32::MAX),
                    u32::try_from(column).unwrap_or(u32::MAX),
                ));
            }
            if usize::try_from(columns).unwrap_or(usize::MAX) > MAX_SHEET_COLUMNS {
                output.push_str(" | ...");
            }
            output.push('\n');
        }
        if usize::try_from(rows).unwrap_or(usize::MAX) > MAX_SHEET_ROWS {
            output.push_str("... omitted ");
            output.push_str(
                &(usize::try_from(rows).unwrap_or(usize::MAX) - MAX_SHEET_ROWS).to_string(),
            );
            output.push_str(" more rows\n");
        }
        output.push('\n');
    }
    Ok(output)
}

fn extract_binary_text(bytes: &[u8]) -> String {
    let mut output = String::new();
    let mut ascii = Vec::new();
    for &byte in bytes {
        if byte == b'\n' || byte == b'\r' || byte == b'\t' || (0x20..=0x7e).contains(&byte) {
            ascii.push(byte);
        } else {
            if ascii.len() >= 4 {
                output.push_str(&String::from_utf8_lossy(&ascii));
                output.push('\n');
            }
            ascii.clear();
        }
    }
    if ascii.len() >= 4 {
        output.push_str(&String::from_utf8_lossy(&ascii));
    }
    let utf16 = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    let decoded = String::from_utf16_lossy(&utf16);
    let readable = decoded
        .split(|character: char| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        .filter(|part| part.chars().count() >= 4)
        .collect::<Vec<_>>()
        .join("\n");
    if !readable.is_empty() {
        output.push('\n');
        output.push_str(&readable);
    }
    output
}

fn normalize_attachment_text(content: &str) -> String {
    let mut output = String::with_capacity(content.len());
    let mut previous_newline = false;
    let mut consecutive_newlines = 0_u8;
    for character in content.replace("\r\n", "\n").replace('\r', "\n").chars() {
        let character = match character {
            '\0' => continue,
            '\t' | '\u{000b}' | '\u{000c}' => ' ',
            other => other,
        };
        if character == '\n' {
            consecutive_newlines = consecutive_newlines.saturating_add(1);
            if consecutive_newlines <= 2 {
                output.push(character);
            }
            previous_newline = true;
        } else {
            consecutive_newlines = 0;
            if !(character == ' ' && previous_newline) {
                output.push(character);
            }
            previous_newline = false;
        }
    }
    output.trim().to_owned()
}

fn xml_local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn bounded_error_message(message: &str) -> String {
    message.chars().take(2_000).collect()
}

fn text_content(content: &[AgentMessageContent]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            AgentMessageContent::Text { text } => Some(text.as_str()),
            AgentMessageContent::ToolCalls { .. } | AgentMessageContent::ToolResult { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn trace_content(content: &[AgentMessageContent]) -> Option<String> {
    let events = content
        .iter()
        .flat_map(|block| match block {
            AgentMessageContent::ToolCalls { calls } => calls
                .iter()
                .map(|call| {
                    serde_json::json!({
                        "type": "tool_call",
                        "messageType": "tool_call",
                        "id": call.id,
                        "name": call.name,
                        "arguments": call.arguments_json,
                    })
                })
                .collect::<Vec<_>>(),
            AgentMessageContent::ToolResult {
                tool_call_id,
                name,
                output,
            } => vec![serde_json::json!({
                "type": "tool_result",
                "messageType": "tool_result",
                "id": tool_call_id,
                "name": name,
                "content": tool_output_json(output),
            })],
            AgentMessageContent::Text { .. } => Vec::new(),
        })
        .collect::<Vec<_>>();
    (!events.is_empty()).then(|| serde_json::Value::Array(events).to_string())
}

fn tool_output_json(output: &AgentToolOutput) -> String {
    serde_json::to_string(output).unwrap_or_else(|_| {
        r#"{"type":"text","content":"Tool result unavailable","truncated":true}"#.to_owned()
    })
}

fn has_explicit_provider_selection(request: &LegacyAiChatRequest) -> bool {
    [
        request.model_config_id.as_deref(),
        request.provider.as_deref(),
        request.model.as_deref(),
        request.api_key.as_deref(),
        request.base_url.as_deref(),
    ]
    .into_iter()
    .any(|value| nonempty(value).is_some())
}

fn parse_provider_kind(value: &str) -> Result<ProviderKind, LegacyAiFailure> {
    match value.trim().to_ascii_uppercase().as_str() {
        "OPENAI" | "OPEN_AI" | "OPENAI_COMPATIBLE" | "OPEN_AI_COMPATIBLE" => {
            Ok(ProviderKind::OpenAiCompatible)
        }
        "CLAUDE" | "ANTHROPIC" => Ok(ProviderKind::Anthropic),
        "GEMINI" | "GOOGLE" => Ok(ProviderKind::Gemini),
        _ => Err(LegacyAiFailure::invalid(
            "invalid_ai_provider",
            "provider must be OPENAI, CLAUDE, or GEMINI",
        )),
    }
}

fn legacy_provider_name(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAiCompatible => "OPENAI",
        ProviderKind::Anthropic => "CLAUDE",
        ProviderKind::Gemini => "GEMINI",
    }
}

fn default_base_url(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAiCompatible => "https://api.openai.com/v1",
        ProviderKind::Anthropic => "https://api.anthropic.com/v1",
        ProviderKind::Gemini => "https://generativelanguage.googleapis.com/v1beta",
    }
}

fn bounded_title(input: &str) -> String {
    let title = input.chars().take(80).collect::<String>();
    if title.is_empty() {
        "New chat".to_owned()
    } else {
        title
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn nonempty_owned(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalized_url(value: &str) -> &str {
    value.trim().trim_end_matches('/')
}

fn event_timestamp(envelope: &AgentEventEnvelope) -> u64 {
    envelope
        .occurred_at_ms
        .parse()
        .unwrap_or_else(|_| current_epoch_millis())
}

fn current_epoch_millis() -> u64 {
    u64::try_from(Utc::now().timestamp_millis()).unwrap_or(0)
}

fn legacy_timestamp(epoch_millis: &str) -> String {
    epoch_millis
        .parse::<i64>()
        .ok()
        .and_then(|millis| Utc.timestamp_millis_opt(millis).single())
        .map_or_else(|| epoch_millis.to_owned(), |time| time.to_rfc3339())
}

fn query_value<'a>(request_url: &'a str, name: &str) -> Option<&'a str> {
    request_url
        .split_once('?')
        .map(|(_, query)| query)
        .into_iter()
        .flat_map(|query| query.split('&'))
        .find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then_some(value)
        })
}

fn internal_failure_value() -> serde_json::Value {
    serde_json::json!({
        "success": false,
        "data": null,
        "errorCode": "internal_error",
        "errorMessage": "The operation could not be completed"
    })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use axum::{
        Router,
        body::Body,
        http::{Method, Request, StatusCode, header},
        response::IntoResponse,
        routing::post,
    };
    use chat2db_contract::{
        AgentEvent, AgentEventEnvelope, AgentPermissionRequest, ApiError,
        CreateProviderProfileRequest, ProviderKind,
    };
    use chat2db_core::Application;
    use chat2db_storage::{SecretRef, SecretValue, SecretVault, SecretVaultError, Storage};
    use http_body_util::BodyExt as _;
    use tempfile::TempDir;
    use tokio::{net::TcpListener, task::JoinHandle, time::timeout};
    use tower::ServiceExt as _;

    use super::{LegacyAiChatRequest, dispatch, project_agent_event, routes, start_chat_run};

    const MOCK_OPENAI_STREAM: &str = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello from mock\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );

    #[derive(Default)]
    struct MemoryVault {
        values: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SecretVault for MemoryVault {
        fn probe(&self) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn create(
            &self,
            reference: &SecretRef,
            value: &SecretValue,
        ) -> Result<(), SecretVaultError> {
            self.values.lock().expect("vault lock").insert(
                reference.as_str().to_owned(),
                value.expose_secret().to_vec(),
            );
            Ok(())
        }

        fn get(&self, reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
            Ok(self
                .values
                .lock()
                .expect("vault lock")
                .get(reference.as_str())
                .cloned()
                .map(SecretValue::new))
        }

        fn delete(&self, reference: &SecretRef) -> Result<(), SecretVaultError> {
            self.values
                .lock()
                .expect("vault lock")
                .remove(reference.as_str());
            Ok(())
        }
    }

    struct TestApplication {
        _directory: TempDir,
        application: Application,
    }

    fn test_application() -> TestApplication {
        let directory = TempDir::new().expect("temporary application directory");
        let storage = Storage::open(directory.path(), Arc::new(MemoryVault::default()))
            .expect("test storage must open");
        TestApplication {
            _directory: directory,
            application: Application::with_storage(storage),
        }
    }

    fn json_request(method: Method, uri: &str, body: &serde_json::Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(body).expect("request JSON must serialize"),
            ))
            .expect("request must build")
    }

    fn empty_request(method: Method, uri: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request must build")
    }

    async fn response_bytes(response: axum::response::Response) -> Vec<u8> {
        response
            .into_body()
            .collect()
            .await
            .expect("response body must collect")
            .to_bytes()
            .to_vec()
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        serde_json::from_slice(&response_bytes(response).await)
            .expect("response body must contain JSON")
    }

    async fn mock_openai_response() -> impl IntoResponse {
        (
            [(header::CONTENT_TYPE, "text/event-stream")],
            MOCK_OPENAI_STREAM,
        )
    }

    async fn spawn_mock_openai() -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock provider must bind");
        let address = listener.local_addr().expect("mock provider address");
        let router = Router::new().route("/v1/chat/completions", post(mock_openai_response));
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("mock provider must serve");
        });
        (format!("http://{address}/v1"), server)
    }

    fn sse_payloads(body: &str) -> Vec<serde_json::Value> {
        body.lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .filter(|data| !data.is_empty())
            .map(|data| serde_json::from_str(data).expect("legacy SSE data must be JSON"))
            .collect()
    }

    fn envelope(event: AgentEvent) -> AgentEventEnvelope {
        AgentEventEnvelope {
            run_id: "run-1".to_owned(),
            sequence: "1".to_owned(),
            occurred_at_ms: "1700000000000".to_owned(),
            event,
        }
    }

    #[test]
    fn event_projection_matches_the_retained_frontend_and_never_fakes_reasoning() {
        let answer = project_agent_event(
            &envelope(AgentEvent::TextDelta {
                delta: "hello".to_owned(),
            }),
            "session-1",
        )
        .expect("text delta projects");
        assert_eq!(answer.event_type, "answer");
        assert_eq!(answer.content.as_deref(), Some("hello"));

        let done = project_agent_event(
            &envelope(AgentEvent::Completed {
                message_id: "message-1".to_owned(),
            }),
            "session-1",
        )
        .expect("completion projects");
        assert_eq!(done.event_type, "done");
        assert_eq!(done.session_id.as_deref(), Some("session-1"));

        let failed = project_agent_event(
            &envelope(AgentEvent::Failed {
                error: ApiError::new("provider_failed", "provider failed"),
            }),
            "session-1",
        )
        .expect("failure projects");
        assert_eq!(failed.event_type, "error");
        assert_eq!(failed.error_code.as_deref(), Some("provider_failed"));

        let denied = project_agent_event(
            &envelope(AgentEvent::PermissionRequested {
                permission: AgentPermissionRequest {
                    permission_id: "permission-1".to_owned(),
                    run_id: "run-1".to_owned(),
                    tool_call_id: "tool-1".to_owned(),
                    tool_name: "execute_sql".to_owned(),
                    arguments_sha256: "0".repeat(64),
                    summary: "write data".to_owned(),
                    requested_at_ms: "1700000000000".to_owned(),
                    expires_at_ms: "1700000060000".to_owned(),
                },
            }),
            "session-1",
        )
        .expect("permission request projects to denial");
        assert_eq!(denied.event_type, "error");
        assert_eq!(
            denied.error_code.as_deref(),
            Some("agent_write_permission_denied")
        );

        assert!(project_agent_event(&envelope(AgentEvent::Started), "session-1").is_none());
    }

    #[test]
    fn legacy_chat_request_defaults_to_no_write_permission_surface() {
        let request: LegacyAiChatRequest = serde_json::from_value(serde_json::json!({
            "input": "drop the table",
            "enableTools": true,
            "provider": "OPENAI",
            "model": "model-1",
            "apiKey": "secret"
        }))
        .expect("legacy request decodes");
        assert_eq!(request.input, "drop the table");
    }

    #[tokio::test]
    async fn web_stream_emits_session_answer_done_and_persists_history() {
        let fixture = test_application();
        let (base_url, server) = spawn_mock_openai().await;
        let application = routes().with_state(fixture.application);
        let response = application
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/v3/ai/chat/stream",
                &serde_json::json!({
                    "input": "Say hello",
                    "provider": "OPENAI",
                    "model": "mock-model",
                    "apiKey": "sentinel-api-key",
                    "baseUrl": base_url
                }),
            ))
            .await
            .expect("AI stream route must respond");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers()[header::CONTENT_TYPE]
                .to_str()
                .expect("content type must be ASCII")
                .starts_with("text/event-stream")
        );
        let body = timeout(Duration::from_secs(3), response_bytes(response))
            .await
            .expect("terminal legacy SSE must close");
        let body = String::from_utf8(body).expect("legacy SSE must be UTF-8");
        let payloads = sse_payloads(&body);
        assert_eq!(
            payloads
                .iter()
                .map(|payload| payload["type"].as_str().expect("event type"))
                .collect::<Vec<_>>(),
            ["session", "answer", "done"]
        );
        assert_eq!(payloads[1]["content"], "hello from mock");
        let session_id = payloads[0]["sessionId"]
            .as_str()
            .expect("session event must carry an id");

        let history = application
            .oneshot(empty_request(
                Method::GET,
                &format!("/api/v3/ai/chat/history/messages?sessionId={session_id}"),
            ))
            .await
            .expect("history route must respond");
        let history = response_json(history).await;
        assert_eq!(history["success"], true);
        assert_eq!(history["total"], 2);
        assert_eq!(history["data"][0]["role"], "user");
        assert_eq!(history["data"][0]["content"], "Say hello");
        assert_eq!(history["data"][1]["role"], "assistant");
        assert_eq!(history["data"][1]["content"], "hello from mock");

        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn model_config_routes_cover_crud_options_test_and_secret_retention() {
        let fixture = test_application();
        let (base_url, server) = spawn_mock_openai().await;
        let application = routes().with_state(fixture.application);
        let create_payload = serde_json::json!({
            "name": "Mock OpenAI",
            "provider": "OPENAI",
            "model": "mock-model",
            "apiKey": "sentinel-api-key",
            "baseUrl": base_url,
            "maxTokens": 64,
            "defaultConfig": true
        });
        let created = application
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/v3/ai/model/config/save",
                &create_payload,
            ))
            .await
            .expect("model config save must respond");
        let created = response_json(created).await;
        assert_eq!(created["success"], true);
        assert_eq!(created["data"]["hasApiKey"], true);
        assert!(!created.to_string().contains("sentinel-api-key"));
        let id = created["data"]["id"]
            .as_str()
            .expect("saved config id")
            .to_owned();

        let listed = application
            .clone()
            .oneshot(empty_request(Method::GET, "/api/v3/ai/model/config/list"))
            .await
            .expect("model config list must respond");
        let listed = response_json(listed).await;
        assert_eq!(listed["success"], true);
        assert_eq!(listed["data"][0]["id"], id);
        assert_eq!(listed["data"][0]["apiKeyMasked"], "****");

        let options = application
            .clone()
            .oneshot(empty_request(Method::GET, "/api/v3/ai/model/options"))
            .await
            .expect("model options must respond");
        let options = response_json(options).await;
        assert_eq!(options["success"], true);
        assert_eq!(options["data"][0]["value"], format!("config:{id}"));
        assert_eq!(options["data"][0]["defaultOption"], true);

        let tested = application
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/v3/ai/model/config/test",
                &create_payload,
            ))
            .await
            .expect("model config test must respond");
        let tested = response_json(tested).await;
        assert_eq!(tested["success"], true);
        assert_eq!(tested["data"]["success"], true);
        assert_eq!(tested["data"]["statusCode"], 200);

        let updated = application
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/v3/ai/model/config/save",
                &serde_json::json!({
                    "id": id,
                    "name": "Updated Mock",
                    "provider": "OPENAI",
                    "model": "mock-model",
                    "baseUrl": base_url,
                    "maxTokens": 128
                }),
            ))
            .await
            .expect("model config update must respond");
        let updated = response_json(updated).await;
        assert_eq!(updated["success"], true);
        assert_eq!(updated["data"]["name"], "Updated Mock");
        assert_eq!(updated["data"]["hasApiKey"], true);

        let deleted = application
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/v3/ai/model/config/delete",
                &serde_json::json!({ "id": id }),
            ))
            .await
            .expect("model config delete must respond");
        assert_eq!(response_json(deleted).await["success"], true);
        let listed = application
            .oneshot(empty_request(Method::GET, "/api/v3/ai/model/config/list"))
            .await
            .expect("empty model config list must respond");
        assert_eq!(response_json(listed).await["data"], serde_json::json!([]));

        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn selected_model_config_without_a_secret_is_rejected_before_start() {
        let fixture = test_application();
        let profile = fixture
            .application
            .create_provider_profile(CreateProviderProfileRequest {
                name: "Missing secret".to_owned(),
                kind: ProviderKind::OpenAiCompatible,
                base_url: "https://provider.example/v1".to_owned(),
                model: "mock-model".to_owned(),
                context_window_tokens: "4096".to_owned(),
                max_output_tokens: "1024".to_owned(),
                credentials: None,
            })
            .await
            .expect("secret-free provider profile must be created");
        let result = start_chat_run(
            &fixture.application,
            LegacyAiChatRequest {
                input: "hello".to_owned(),
                model_config_id: Some(profile.id),
                ..LegacyAiChatRequest::default()
            },
        )
        .await;
        let Err(error) = result else {
            panic!("secret-free model config must not start a run");
        };
        assert_eq!(error.code, "provider_credentials_missing");
    }

    #[tokio::test]
    async fn attachment_paths_are_desktop_only_and_http_uses_uploads() {
        let directory = TempDir::new().expect("attachment directory");
        let path = directory.path().join("notes.txt");
        std::fs::write(&path, "first line\nsecond line").expect("local attachment must write");
        let application = routes().with_state(Application::new());
        let local = dispatch(
            &Application::new(),
            "post",
            "/api/v3/ai/chat/attachment/parse/local",
            serde_json::json!({ "filePath": path, "fileName": "notes.txt" }),
        )
        .await
        .expect("Desktop attachment dispatch must handle local paths");
        assert_eq!(local["success"], true);
        assert_eq!(local["data"]["fileType"], "txt");
        assert_eq!(local["data"]["contentCategory"], "DOCUMENT");
        assert_eq!(local["data"]["content"], "first line\nsecond line");

        let local_http = application
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/v3/ai/chat/attachment/parse/local",
                &serde_json::json!({ "filePath": path, "fileName": "notes.txt" }),
            ))
            .await
            .expect("local attachment route must respond");
        assert_eq!(local_http.status(), StatusCode::NOT_FOUND);

        let boundary = "chat2db-test-boundary";
        let multipart = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"rows.csv\"\r\nContent-Type: text/csv\r\n\r\nname,value\r\nalpha,1\r\n--{boundary}--\r\n"
        );
        let upload_request = Request::builder()
            .method(Method::POST)
            .uri("/api/v3/ai/chat/attachment/parse/upload")
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(multipart))
            .expect("multipart request must build");
        let uploaded = application
            .oneshot(upload_request)
            .await
            .expect("upload attachment route must respond");
        let uploaded = response_json(uploaded).await;
        assert_eq!(uploaded["success"], true);
        assert_eq!(uploaded["data"]["fileType"], "csv");
        assert_eq!(uploaded["data"]["contentCategory"], "TABULAR");
        assert!(
            uploaded["data"]["content"]
                .as_str()
                .expect("CSV content")
                .contains("alpha")
        );
    }
}
