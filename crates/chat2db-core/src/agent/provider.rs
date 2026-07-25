use std::sync::Arc;

use chat2db_agent::{
    ApiKey, ContextBudget, HttpProviderConfig, Provider,
    providers::{AnthropicProvider, GeminiProvider, OpenAiProvider},
};
use chat2db_storage::ProviderKind;

use crate::{AppError, Application, storage_call};

const MAX_SERIALIZED_CONTEXT_BYTES: usize = 16 * 1024 * 1024;
const COMPACTION_THRESHOLD_PERCENT: u8 = 80;

impl Application {
    /// Resolves one durable provider profile into a fresh direct HTTP adapter.
    ///
    /// The profile and credential are loaded together so a concurrent rotation
    /// cannot combine metadata from one revision with a key from another.
    ///
    /// # Errors
    ///
    /// Returns storage, credential, numeric-range, or provider-configuration
    /// failures without exposing credential bytes.
    pub(crate) async fn resolve_agent_provider(
        &self,
        provider_id: &str,
    ) -> Result<Arc<dyn Provider>, AppError> {
        let storage = self.require_storage()?;
        let provider_id = provider_id.to_owned();
        let (profile, secret) =
            storage_call(move || storage.get_provider_profile_with_secret(&provider_id)).await?;
        let secret = secret.ok_or_else(|| {
            AppError::invalid(
                "provider_credentials_missing",
                "The provider profile does not have an API key",
            )
        })?;
        let api_key = ApiKey::new(
            std::str::from_utf8(secret.expose_secret()).map_err(|_| AppError::internal())?,
        )?;
        drop(secret);
        let context_window_tokens = usize::try_from(profile.context_window_tokens)
            .map_err(|_| invalid_provider_config())?;
        let max_output_tokens =
            u32::try_from(profile.max_output_tokens).map_err(|_| invalid_provider_config())?;
        let budget = ContextBudget::new(
            Some(context_window_tokens),
            MAX_SERIALIZED_CONTEXT_BYTES,
            COMPACTION_THRESHOLD_PERCENT,
        )?;
        let config = HttpProviderConfig::new(profile.base_url, profile.model, api_key)?;

        let provider: Arc<dyn Provider> = match profile.kind {
            ProviderKind::OpenAiCompatible => Arc::new(
                OpenAiProvider::new(config)?
                    .with_context_budget(budget)
                    .with_max_output_tokens(max_output_tokens)?,
            ),
            ProviderKind::Anthropic => Arc::new(
                AnthropicProvider::new(config)?
                    .with_context_budget(budget)
                    .with_max_output_tokens(max_output_tokens)?,
            ),
            ProviderKind::Gemini => Arc::new(
                GeminiProvider::new(config)?
                    .with_context_budget(budget)
                    .with_max_output_tokens(max_output_tokens)?,
            ),
        };
        Ok(provider)
    }
}

fn invalid_provider_config() -> AppError {
    AppError::invalid(
        "invalid_provider_config",
        "The provider profile configuration is invalid",
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use chat2db_agent::{ContextBudget, ProviderKind as AgentProviderKind};
    use chat2db_storage::{
        CreateProviderProfile, ProviderKind as StorageProviderKind, SecretRef, SecretValue,
        SecretVault, SecretVaultError, Storage,
    };
    use tempfile::TempDir;

    use super::{COMPACTION_THRESHOLD_PERCENT, MAX_SERIALIZED_CONTEXT_BYTES};
    use crate::{AppErrorKind, Application};

    #[derive(Default)]
    struct MemoryVault {
        values: Mutex<HashMap<String, Vec<u8>>>,
        reads: AtomicUsize,
    }

    impl MemoryVault {
        fn replace(&self, reference: &SecretRef, value: impl Into<Vec<u8>>) {
            self.values
                .lock()
                .expect("vault lock")
                .insert(reference.as_str().to_owned(), value.into());
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
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
            self.replace(reference, value.expose_secret());
            Ok(())
        }

        fn get(&self, reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
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

    fn setup() -> (TempDir, Storage, Application, Arc<MemoryVault>) {
        let directory = TempDir::new().expect("temp dir");
        let vault = Arc::new(MemoryVault::default());
        let storage = Storage::open(directory.path(), vault.clone()).expect("storage opens");
        let application = Application::with_storage(storage.clone());
        (directory, storage, application, vault)
    }

    fn create_provider(
        storage: &Storage,
        kind: StorageProviderKind,
        model: &str,
        context_window_tokens: u64,
        max_output_tokens: u64,
        secret: Option<&[u8]>,
    ) -> chat2db_storage::ProviderProfileRecord {
        storage
            .create_provider_profile(
                CreateProviderProfile {
                    name: format!("{model} profile"),
                    kind,
                    base_url: "https://provider.example/v1".to_owned(),
                    model: model.to_owned(),
                    context_window_tokens,
                    max_output_tokens,
                },
                secret.map(|value| SecretValue::new(value.to_vec())),
            )
            .expect("provider creates")
    }

    #[tokio::test]
    async fn resolves_all_provider_kinds_with_the_persisted_model_and_budget() {
        let (_directory, storage, application, vault) = setup();
        let cases = [
            (
                StorageProviderKind::OpenAiCompatible,
                AgentProviderKind::OpenAi,
                "openai-model",
                128_000,
            ),
            (
                StorageProviderKind::Anthropic,
                AgentProviderKind::Anthropic,
                "anthropic-model",
                200_000,
            ),
            (
                StorageProviderKind::Gemini,
                AgentProviderKind::Gemini,
                "gemini-model",
                1_000_000,
            ),
        ];

        for (storage_kind, agent_kind, model, context_window_tokens) in cases {
            let profile = create_provider(
                &storage,
                storage_kind,
                model,
                context_window_tokens,
                4_096,
                Some(b"provider-key"),
            );
            let provider = application
                .resolve_agent_provider(&profile.id)
                .await
                .expect("provider resolves");

            assert_eq!(provider.kind(), agent_kind);
            assert_eq!(provider.model(), model);
            assert_eq!(
                provider.context_budget(),
                ContextBudget::new(
                    Some(usize::try_from(context_window_tokens).expect("context fits")),
                    MAX_SERIALIZED_CONTEXT_BYTES,
                    COMPACTION_THRESHOLD_PERCENT,
                )
                .expect("budget is valid")
            );
        }
        assert_eq!(vault.reads(), cases.len());
    }

    #[tokio::test]
    async fn rejects_missing_invalid_and_blank_credentials_without_leaking_them() {
        const SENTINEL: &str = "provider-secret-sentinel";

        let (_directory, storage, application, _vault) = setup();
        let missing = create_provider(
            &storage,
            StorageProviderKind::OpenAiCompatible,
            "model",
            128_000,
            4_096,
            None,
        );
        let error = application
            .resolve_agent_provider(&missing.id)
            .await
            .err()
            .expect("missing credential fails");
        assert_eq!(error.kind(), AppErrorKind::InvalidRequest);
        assert_eq!(error.api_error().code, "provider_credentials_missing");

        let mut invalid_bytes = SENTINEL.as_bytes().to_vec();
        invalid_bytes.push(0xff);
        let invalid = create_provider(
            &storage,
            StorageProviderKind::Anthropic,
            "model",
            128_000,
            4_096,
            Some(&invalid_bytes),
        );
        let error = application
            .resolve_agent_provider(&invalid.id)
            .await
            .err()
            .expect("non-UTF-8 credential fails");
        assert_eq!(error.kind(), AppErrorKind::Internal);
        assert_eq!(error.api_error().code, "internal_error");
        assert!(!format!("{error:?}").contains(SENTINEL));
        assert!(!error.to_string().contains(SENTINEL));

        let blank = create_provider(
            &storage,
            StorageProviderKind::Gemini,
            "model",
            128_000,
            4_096,
            Some(b" \t "),
        );
        let error = application
            .resolve_agent_provider(&blank.id)
            .await
            .err()
            .expect("blank credential fails");
        assert_eq!(error.kind(), AppErrorKind::InvalidRequest);
        assert_eq!(error.api_error().code, "invalid_provider_config");
    }

    #[tokio::test]
    async fn rejects_provider_limits_that_do_not_fit_runtime_types() {
        let (_directory, storage, application, _vault) = setup();
        let overflow = u64::from(u32::MAX) + 1;
        let profile = create_provider(
            &storage,
            StorageProviderKind::OpenAiCompatible,
            "model",
            overflow,
            overflow,
            Some(b"provider-key"),
        );

        let error = application
            .resolve_agent_provider(&profile.id)
            .await
            .err()
            .expect("oversized output limit fails");
        assert_eq!(error.kind(), AppErrorKind::InvalidRequest);
        assert_eq!(error.api_error().code, "invalid_provider_config");
    }

    #[tokio::test]
    async fn reloads_rotated_credentials_and_returns_a_fresh_provider() {
        const FIRST_SENTINEL: &str = "first-provider-key-sentinel";
        const SECOND_SENTINEL: &str = "second-provider-key-sentinel";

        let (_directory, storage, application, vault) = setup();
        let profile = create_provider(
            &storage,
            StorageProviderKind::OpenAiCompatible,
            "model",
            128_000,
            4_096,
            Some(FIRST_SENTINEL.as_bytes()),
        );
        let reference = profile.secret_ref.as_ref().expect("secret reference");
        let first = application
            .resolve_agent_provider(&profile.id)
            .await
            .expect("first provider resolves");

        vault.replace(reference, b" \t ");
        let error = application
            .resolve_agent_provider(&profile.id)
            .await
            .err()
            .expect("rotated blank key is observed");
        assert_eq!(error.api_error().code, "invalid_provider_config");
        assert!(!format!("{error:?}").contains(FIRST_SENTINEL));

        vault.replace(reference, SECOND_SENTINEL.as_bytes());
        let second = application
            .resolve_agent_provider(&profile.id)
            .await
            .expect("rotated provider resolves");
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(vault.reads(), 3);
    }
}
