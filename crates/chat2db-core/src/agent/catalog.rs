use chat2db_contract::{
    AgentMessageList, AgentSession, AgentSessionList, CreateAgentSessionRequest,
    CreateProviderProfileRequest, ProviderProfile, ProviderProfileList, ProviderSecretChange,
    UpdateAgentSessionRequest, UpdateProviderProfileRequest,
};
use chat2db_storage::{
    CreateAgentSession, CreateProviderProfile, SecretChange, SecretValue, StorageError,
    UpdateAgentSession, UpdateProviderProfile,
};

use crate::{AppError, Application, convert, parse_u32, parse_u64, storage_call};

impl Application {
    /// Lists provider profiles without loading or exposing their credentials.
    ///
    /// # Errors
    ///
    /// Returns an availability or durable-storage error.
    pub async fn list_provider_profiles(&self) -> Result<ProviderProfileList, AppError> {
        let storage = self.require_storage()?;
        let records = storage_call(move || storage.list_provider_profiles()).await?;
        Ok(ProviderProfileList {
            items: records.into_iter().map(convert::provider_profile).collect(),
        })
    }

    /// Creates a provider profile and installs its optional API key in the vault.
    ///
    /// # Errors
    ///
    /// Returns validation, vault, or durable-storage errors.
    pub async fn create_provider_profile(
        &self,
        request: CreateProviderProfileRequest,
    ) -> Result<ProviderProfile, AppError> {
        let CreateProviderProfileRequest {
            name,
            kind,
            base_url,
            model,
            context_window_tokens,
            max_output_tokens,
            credentials,
        } = request;
        let api_key =
            credentials.map(|credentials| SecretValue::new(credentials.api_key.into_bytes()));
        let storage = self.require_storage()?;
        let input = CreateProviderProfile {
            name,
            kind: convert::provider_kind_to_storage(kind),
            base_url,
            model,
            context_window_tokens: parse_u64(&context_window_tokens, "contextWindowTokens")?,
            max_output_tokens: parse_u64(&max_output_tokens, "maxOutputTokens")?,
        };
        storage_call(move || storage.create_provider_profile(input, api_key))
            .await
            .map(convert::provider_profile)
    }

    /// Returns one provider profile without loading or exposing its credential.
    ///
    /// # Errors
    ///
    /// Returns not-found, availability, or durable-storage errors.
    pub async fn get_provider_profile(&self, id: &str) -> Result<ProviderProfile, AppError> {
        let storage = self.require_storage()?;
        let id = id.to_owned();
        let record = storage_call(move || storage.get_provider_profile(&id)).await?;
        record.map(convert::provider_profile).ok_or_else(|| {
            AppError::not_found("provider_not_found", "The provider profile does not exist")
        })
    }

    /// Replaces provider metadata and applies an explicit credential action using revision CAS.
    ///
    /// # Errors
    ///
    /// Returns validation, revision-conflict, dependency, vault, or durable-storage errors.
    pub async fn update_provider_profile(
        &self,
        id: &str,
        request: UpdateProviderProfileRequest,
    ) -> Result<ProviderProfile, AppError> {
        let id = id.to_owned();
        let UpdateProviderProfileRequest {
            expected_revision,
            name,
            kind,
            base_url,
            model,
            context_window_tokens,
            max_output_tokens,
            secret_change,
        } = request;
        let secret_change = match secret_change {
            ProviderSecretChange::Keep => SecretChange::Keep,
            ProviderSecretChange::Clear => SecretChange::Clear,
            ProviderSecretChange::Replace { credentials } => {
                SecretChange::Replace(SecretValue::new(credentials.api_key.into_bytes()))
            }
        };
        let storage = self.require_storage()?;
        let expected_revision = parse_u64(&expected_revision, "expectedRevision")?;
        let input = UpdateProviderProfile {
            name,
            kind: convert::provider_kind_to_storage(kind),
            base_url,
            model,
            context_window_tokens: parse_u64(&context_window_tokens, "contextWindowTokens")?,
            max_output_tokens: parse_u64(&max_output_tokens, "maxOutputTokens")?,
        };
        storage_call(move || {
            storage.update_provider_profile(&id, expected_revision, input, secret_change)
        })
        .await
        .map(convert::provider_profile)
    }

    /// Deletes a provider profile using revision CAS.
    ///
    /// # Errors
    ///
    /// Returns not-found, revision-conflict, dependency, vault, or storage errors.
    pub async fn delete_provider_profile(
        &self,
        id: &str,
        expected_revision: &str,
    ) -> Result<(), AppError> {
        let storage = self.require_storage()?;
        let id = id.to_owned();
        let expected_revision = parse_u64(expected_revision, "expectedRevision")?;
        storage_call(move || storage.delete_provider_profile(&id, expected_revision)).await
    }

    /// Lists durable agent sessions in reverse update order.
    ///
    /// # Errors
    ///
    /// Returns an availability or durable-storage error.
    pub async fn list_agent_sessions(&self) -> Result<AgentSessionList, AppError> {
        let storage = self.require_storage()?;
        let records = storage_call(move || storage.list_agent_sessions()).await?;
        Ok(AgentSessionList {
            items: records.into_iter().map(convert::agent_session).collect(),
        })
    }

    /// Creates a durable agent session and its optional system message.
    ///
    /// # Errors
    ///
    /// Returns validation, dependency, availability, or durable-storage errors.
    pub async fn create_agent_session(
        &self,
        request: CreateAgentSessionRequest,
    ) -> Result<AgentSession, AppError> {
        let storage = self.require_storage()?;
        let input = CreateAgentSession {
            title: request.title,
            provider_id: request.provider_id,
            datasource_id: request.datasource_id,
            system_prompt: request.system_prompt,
        };
        storage_call(move || storage.create_agent_session(input))
            .await
            .map(convert::agent_session)
    }

    /// Returns one durable agent session.
    ///
    /// # Errors
    ///
    /// Returns not-found, availability, or durable-storage errors.
    pub async fn get_agent_session(&self, id: &str) -> Result<AgentSession, AppError> {
        let storage = self.require_storage()?;
        let id = id.to_owned();
        let record = storage_call(move || storage.get_agent_session(&id)).await?;
        record.map(convert::agent_session).ok_or_else(|| {
            AppError::not_found(
                "agent_session_not_found",
                "The agent session does not exist",
            )
        })
    }

    /// Replaces mutable agent-session settings using revision CAS.
    ///
    /// # Errors
    ///
    /// Returns validation, revision-conflict, dependency, busy-session, or storage errors.
    pub async fn update_agent_session(
        &self,
        id: &str,
        request: UpdateAgentSessionRequest,
    ) -> Result<AgentSession, AppError> {
        let storage = self.require_storage()?;
        let id = id.to_owned();
        let expected_revision = parse_u64(&request.expected_revision, "expectedRevision")?;
        let input = UpdateAgentSession {
            title: request.title,
            provider_id: request.provider_id,
            datasource_id: request.datasource_id,
        };
        storage_call(move || storage.update_agent_session(&id, expected_revision, input))
            .await
            .map(convert::agent_session)
    }

    /// Deletes a durable session and its session-owned transcript and run state using revision CAS.
    ///
    /// # Errors
    ///
    /// Returns not-found, revision-conflict, busy-session, or durable-storage errors.
    pub async fn delete_agent_session(
        &self,
        id: &str,
        expected_revision: &str,
    ) -> Result<(), AppError> {
        let storage = self.require_storage()?;
        let id = id.to_owned();
        let expected_revision = parse_u64(expected_revision, "expectedRevision")?;
        storage_call(move || storage.delete_agent_session(&id, expected_revision)).await
    }

    /// Reads a bounded forward page of canonical messages starting at one session ordinal.
    ///
    /// # Errors
    ///
    /// Returns validation, not-found, availability, corruption, or durable-storage errors.
    pub async fn list_agent_messages(
        &self,
        session_id: &str,
        start_ordinal: &str,
        limit: &str,
    ) -> Result<AgentMessageList, AppError> {
        let storage = self.require_storage()?;
        let session_id = session_id.to_owned();
        let start_ordinal = parse_u64(start_ordinal, "startOrdinal")?;
        let limit = parse_u32(limit, "limit")?;
        let expected_len = usize::try_from(limit).map_err(|_| AppError::internal())?;
        let (records, has_more) = storage_call(move || {
            let records = storage.list_agent_messages(&session_id, start_ordinal, limit)?;
            let has_more = if records.len() == expected_len {
                let next_ordinal = records
                    .last()
                    .map(|record| record.ordinal)
                    .and_then(|ordinal| ordinal.checked_add(1))
                    .ok_or(StorageError::NumericRange("agent message ordinal"))?;
                !storage
                    .list_agent_messages(&session_id, next_ordinal, 1)?
                    .is_empty()
            } else {
                false
            };
            Ok((records, has_more))
        })
        .await?;
        Ok(AgentMessageList {
            items: records
                .into_iter()
                .map(convert::agent_message)
                .collect::<Result<_, _>>()?,
            has_more,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use chat2db_contract::{
        AgentMessageContent, AgentToolCall, AgentToolOutput, ApiErrorDetails,
        CreateAgentSessionRequest, CreateProviderProfileRequest, ProviderCredentials, ProviderKind,
        ProviderProfile, ProviderSecretChange, UpdateAgentSessionRequest,
        UpdateProviderProfileRequest,
    };
    use chat2db_storage::{
        AgentMessageRecord, AgentMessageRole, AppendAgentMessage, SecretRef, SecretValue,
        SecretVault, SecretVaultError, SqlPermissionMode as StorageSqlPermissionMode,
        StartAgentRun, Storage,
    };
    use tempfile::TempDir;

    use crate::{AppErrorKind, Application, convert};

    #[derive(Default)]
    struct MemoryVault {
        values: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MemoryVault {
        fn contains(&self, expected: &[u8]) -> bool {
            self.values
                .lock()
                .expect("vault lock")
                .values()
                .any(|value| value == expected)
        }

        fn is_empty(&self) -> bool {
            self.values.lock().expect("vault lock").is_empty()
        }
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

    fn setup() -> (TempDir, Application, Arc<MemoryVault>) {
        let directory = TempDir::new().expect("temp dir");
        let vault = Arc::new(MemoryVault::default());
        let storage = Storage::open(directory.path(), vault.clone()).expect("storage opens");
        (directory, Application::with_storage(storage), vault)
    }

    fn provider_request(name: &str, api_key: Option<&str>) -> CreateProviderProfileRequest {
        CreateProviderProfileRequest {
            name: name.to_owned(),
            kind: ProviderKind::OpenAiCompatible,
            base_url: "https://provider.example/v1".to_owned(),
            model: "model-1".to_owned(),
            context_window_tokens: "9007199254740993".to_owned(),
            max_output_tokens: "8192".to_owned(),
            credentials: api_key.map(|api_key| ProviderCredentials {
                api_key: api_key.to_owned(),
            }),
        }
    }

    fn provider_update(
        profile: &ProviderProfile,
        secret_change: ProviderSecretChange,
    ) -> UpdateProviderProfileRequest {
        UpdateProviderProfileRequest {
            expected_revision: profile.revision.clone(),
            name: profile.name.clone(),
            kind: profile.kind,
            base_url: profile.base_url.clone(),
            model: profile.model.clone(),
            context_window_tokens: profile.context_window_tokens.clone(),
            max_output_tokens: profile.max_output_tokens.clone(),
            secret_change,
        }
    }

    #[tokio::test]
    async fn provider_crud_keeps_api_keys_in_the_vault_and_revision_conflicts_portable() {
        const FIRST_SECRET: &str = "provider-secret-first";
        const SECOND_SECRET: &str = "provider-secret-second";

        let (_directory, application, vault) = setup();
        let created = application
            .create_provider_profile(provider_request("Primary", Some(FIRST_SECRET)))
            .await
            .expect("provider creates");
        assert!(created.has_secret);
        assert!(vault.contains(FIRST_SECRET.as_bytes()));
        assert_eq!(created.context_window_tokens, "9007199254740993");

        let fetched = application
            .get_provider_profile(&created.id)
            .await
            .expect("provider loads");
        let listed = application
            .list_provider_profiles()
            .await
            .expect("providers list");
        let external = serde_json::to_string(&(created.clone(), fetched, listed))
            .expect("provider responses serialize");
        for forbidden in [FIRST_SECRET, "apiKey", "secretRef"] {
            assert!(!external.contains(forbidden));
        }
        let provider_json = serde_json::to_value(&created).expect("provider serializes");
        for field in [
            "contextWindowTokens",
            "maxOutputTokens",
            "revision",
            "createdAtMs",
            "updatedAtMs",
        ] {
            assert!(provider_json[field].is_string(), "{field} must be a string");
        }

        let replaced = application
            .update_provider_profile(
                &created.id,
                provider_update(
                    &created,
                    ProviderSecretChange::Replace {
                        credentials: ProviderCredentials {
                            api_key: SECOND_SECRET.to_owned(),
                        },
                    },
                ),
            )
            .await
            .expect("provider secret replaces");
        assert!(!vault.contains(FIRST_SECRET.as_bytes()));
        assert!(vault.contains(SECOND_SECRET.as_bytes()));

        let conflict = application
            .update_provider_profile(
                &created.id,
                provider_update(&created, ProviderSecretChange::Keep),
            )
            .await
            .expect_err("stale provider revision must fail");
        assert_eq!(conflict.kind(), AppErrorKind::Conflict);
        assert!(matches!(
            conflict.api_error().details,
            Some(ApiErrorDetails::RevisionConflict {
                expected_revision,
                actual_revision: Some(actual_revision),
            }) if expected_revision == "1" && actual_revision == "2"
        ));

        let cleared = application
            .update_provider_profile(
                &replaced.id,
                provider_update(&replaced, ProviderSecretChange::Clear),
            )
            .await
            .expect("provider secret clears");
        assert!(!cleared.has_secret);
        assert!(vault.is_empty());
        application
            .delete_provider_profile(&cleared.id, &cleared.revision)
            .await
            .expect("provider deletes");
        let missing = application
            .get_provider_profile(&cleared.id)
            .await
            .expect_err("deleted provider is absent");
        assert_eq!(missing.api_error().code, "provider_not_found");
    }

    #[tokio::test]
    async fn session_crud_persists_system_message_and_delete_removes_the_catalog_entry() {
        let (_directory, application, _vault) = setup();
        let provider = application
            .create_provider_profile(provider_request("Primary", None))
            .await
            .expect("provider creates");
        let created = application
            .create_agent_session(CreateAgentSessionRequest {
                title: "First title".to_owned(),
                provider_id: provider.id.clone(),
                datasource_id: None,
                system_prompt: Some("Keep answers bounded".to_owned()),
            })
            .await
            .expect("session creates");
        assert_eq!(
            application
                .get_agent_session(&created.id)
                .await
                .expect("session loads"),
            created
        );
        assert_eq!(
            application
                .list_agent_sessions()
                .await
                .expect("sessions list")
                .items,
            vec![created.clone()]
        );
        let provider_in_use = application
            .delete_provider_profile(&provider.id, &provider.revision)
            .await
            .expect_err("provider selected by a session must not delete");
        assert_eq!(provider_in_use.api_error().code, "provider_in_use");
        let session_json = serde_json::to_value(&created).expect("session serializes");
        for field in ["revision", "createdAtMs", "updatedAtMs"] {
            assert!(session_json[field].is_string(), "{field} must be a string");
        }

        let updated = application
            .update_agent_session(
                &created.id,
                UpdateAgentSessionRequest {
                    expected_revision: created.revision,
                    title: "Updated title".to_owned(),
                    provider_id: provider.id.clone(),
                    datasource_id: None,
                },
            )
            .await
            .expect("session updates");
        assert_eq!(updated.title, "Updated title");
        let messages = application
            .list_agent_messages(&updated.id, "0", "10")
            .await
            .expect("system message loads");
        assert_eq!(messages.items.len(), 1);
        assert!(matches!(
            messages.items[0].content.as_slice(),
            [AgentMessageContent::Text { text }] if text == "Keep answers bounded"
        ));

        application
            .delete_agent_session(&updated.id, &updated.revision)
            .await
            .expect("session deletes");
        assert!(
            application
                .list_agent_sessions()
                .await
                .expect("sessions list after delete")
                .items
                .is_empty()
        );
        let missing = application
            .list_agent_messages(&updated.id, "0", "10")
            .await
            .expect_err("deleted transcript is absent");
        assert_eq!(missing.api_error().code, "agent_session_not_found");
    }

    #[tokio::test]
    async fn active_run_freezes_provider_rebinding_and_session_deletion() {
        let (_directory, application, _vault) = setup();
        let primary = application
            .create_provider_profile(provider_request("Primary", None))
            .await
            .expect("primary provider creates");
        let secondary = application
            .create_provider_profile(provider_request("Secondary", None))
            .await
            .expect("secondary provider creates");
        let session = application
            .create_agent_session(CreateAgentSessionRequest {
                title: "Session".to_owned(),
                provider_id: primary.id.clone(),
                datasource_id: None,
                system_prompt: None,
            })
            .await
            .expect("session creates");
        application
            .storage()
            .expect("storage")
            .start_agent_run(
                &session.id,
                StartAgentRun {
                    user_message: "Run".to_owned(),
                    sql_permission_mode: StorageSqlPermissionMode::ReadOnly,
                },
            )
            .expect("run starts");

        let provider_error = application
            .update_provider_profile(
                &primary.id,
                provider_update(&primary, ProviderSecretChange::Keep),
            )
            .await
            .expect_err("active provider mutation must fail");
        assert_eq!(provider_error.api_error().code, "agent_dependency_busy");

        let rebind_error = application
            .update_agent_session(
                &session.id,
                UpdateAgentSessionRequest {
                    expected_revision: session.revision.clone(),
                    title: session.title.clone(),
                    provider_id: secondary.id,
                    datasource_id: None,
                },
            )
            .await
            .expect_err("active session rebind must fail");
        assert_eq!(rebind_error.api_error().code, "agent_session_busy");

        let delete_error = application
            .delete_agent_session(&session.id, &session.revision)
            .await
            .expect_err("active session delete must fail");
        assert_eq!(delete_error.api_error().code, "agent_session_busy");
    }

    #[tokio::test]
    async fn message_pages_decode_typed_content_and_probe_exact_full_pages() {
        let (_directory, application, _vault) = setup();
        let provider = application
            .create_provider_profile(provider_request("Primary", None))
            .await
            .expect("provider creates");
        let session = application
            .create_agent_session(CreateAgentSessionRequest {
                title: "Session".to_owned(),
                provider_id: provider.id,
                datasource_id: None,
                system_prompt: Some("System".to_owned()),
            })
            .await
            .expect("session creates");
        let storage = application.storage().expect("storage");
        for (role, content) in [
            (
                AgentMessageRole::User,
                vec![AgentMessageContent::Text {
                    text: "Question".to_owned(),
                }],
            ),
            (
                AgentMessageRole::Assistant,
                vec![AgentMessageContent::ToolCalls {
                    calls: vec![AgentToolCall {
                        id: "call-1".to_owned(),
                        name: "sql_read".to_owned(),
                        arguments_json: "{\"sql\":\"select 1\"}".to_owned(),
                    }],
                }],
            ),
            (
                AgentMessageRole::Tool,
                vec![AgentMessageContent::ToolResult {
                    tool_call_id: "call-1".to_owned(),
                    name: "sql_read".to_owned(),
                    output: AgentToolOutput::Text {
                        content: "1".to_owned(),
                        truncated: false,
                    },
                }],
            ),
            (
                AgentMessageRole::Assistant,
                vec![AgentMessageContent::Text {
                    text: "Answer".to_owned(),
                }],
            ),
        ] {
            storage
                .append_agent_message(
                    &session.id,
                    AppendAgentMessage {
                        role,
                        summary_through_ordinal: None,
                        content_json: serde_json::to_string(&content)
                            .expect("message content serializes"),
                    },
                )
                .expect("message appends");
        }

        let first = application
            .list_agent_messages(&session.id, "0", "2")
            .await
            .expect("first page loads");
        assert_eq!(
            first
                .items
                .iter()
                .map(|message| message.ordinal.as_str())
                .collect::<Vec<_>>(),
            ["0", "1"]
        );
        assert!(first.has_more);

        let middle = application
            .list_agent_messages(&session.id, "2", "2")
            .await
            .expect("middle page loads");
        assert!(middle.has_more);
        assert!(matches!(
            middle.items[0].content.as_slice(),
            [AgentMessageContent::ToolCalls { calls }] if calls[0].name == "sql_read"
        ));
        assert!(matches!(
            middle.items[1].content.as_slice(),
            [AgentMessageContent::ToolResult { tool_call_id, .. }] if tool_call_id == "call-1"
        ));

        let exact_final = application
            .list_agent_messages(&session.id, "3", "2")
            .await
            .expect("exact final page loads");
        assert_eq!(exact_final.items.len(), 2);
        assert!(!exact_final.has_more);
        let message_json =
            serde_json::to_value(&exact_final.items[0]).expect("external message serializes");
        assert!(message_json["ordinal"].is_string());
        assert!(message_json["createdAtMs"].is_string());
    }

    #[test]
    fn malformed_persisted_message_content_maps_to_safe_internal_error() {
        let error = convert::agent_message(AgentMessageRecord {
            id: "message-1".to_owned(),
            session_id: "session-1".to_owned(),
            run_id: None,
            ordinal: 9_007_199_254_740_993,
            role: AgentMessageRole::Assistant,
            summary_through_ordinal: None,
            content_json: "{not-json".to_owned(),
            content_bytes: 9,
            created_at_ms: 1_784_900_000_000,
        })
        .expect_err("malformed durable content must fail closed");

        assert_eq!(error.kind(), AppErrorKind::Internal);
        assert_eq!(error.api_error().code, "internal_error");
        assert!(!error.to_string().contains("not-json"));
    }
}
