use std::fmt::Formatter;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use uuid::Uuid;

use crate::{SecretChange, SecretRef, SecretValue, Storage, StorageError, now_millis};

const MAX_PROVIDER_NAME_BYTES: usize = 512;
const MAX_PROVIDER_URL_BYTES: usize = 4096;
const MAX_PROVIDER_MODEL_BYTES: usize = 512;

/// Provider protocols supported by the durable agent profile contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// OpenAI-compatible HTTP APIs, including private compatible endpoints.
    OpenAiCompatible,
    /// Anthropic Messages API.
    Anthropic,
    /// Google Gemini API.
    Gemini,
}

impl ProviderKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai_compatible",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }

    fn from_persisted(value: &str) -> Result<Self, StorageError> {
        match value {
            "openai_compatible" => Ok(Self::OpenAiCompatible),
            "anthropic" => Ok(Self::Anthropic),
            "gemini" => Ok(Self::Gemini),
            _ => Err(StorageError::InvalidProvider(
                "persisted provider kind is invalid",
            )),
        }
    }
}

/// Public fields required to create a provider profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateProviderProfile {
    /// User-visible profile name.
    pub name: String,
    /// Provider protocol family.
    pub kind: ProviderKind,
    /// Provider endpoint base URL.
    pub base_url: String,
    /// Default model identifier.
    pub model: String,
    /// Maximum model context window used by the agent budget.
    pub context_window_tokens: u64,
    /// Maximum completion tokens requested from the provider.
    pub max_output_tokens: u64,
}

/// Public provider fields replaced by a revisioned update.
pub type UpdateProviderProfile = CreateProviderProfile;

/// Durable provider metadata. API-key bytes never enter this value.
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderProfileRecord {
    /// Opaque provider profile id.
    pub id: String,
    /// User-visible profile name.
    pub name: String,
    /// Provider protocol family.
    pub kind: ProviderKind,
    /// Provider endpoint base URL.
    pub base_url: String,
    /// Default model identifier.
    pub model: String,
    /// Maximum model context window used by the agent budget.
    pub context_window_tokens: u64,
    /// Maximum completion tokens requested from the provider.
    pub max_output_tokens: u64,
    /// Opaque vault reference for the API key.
    pub secret_ref: Option<SecretRef>,
    /// Monotonic optimistic-concurrency revision.
    pub revision: u64,
    /// Creation time as Unix epoch milliseconds.
    pub created_at_ms: i64,
    /// Last update time as Unix epoch milliseconds.
    pub updated_at_ms: i64,
}

impl std::fmt::Debug for ProviderProfileRecord {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderProfileRecord")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("context_window_tokens", &self.context_window_tokens)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("secret_ref", &self.secret_ref)
            .field("revision", &self.revision)
            .field("created_at_ms", &self.created_at_ms)
            .field("updated_at_ms", &self.updated_at_ms)
            .finish()
    }
}

impl Storage {
    /// Creates a provider profile and stages its optional API key in the vault.
    ///
    /// # Errors
    ///
    /// Returns validation, vault, `SQLite`, compensation, or unknown-outcome failures.
    pub fn create_provider_profile(
        &self,
        input: CreateProviderProfile,
        api_key: Option<SecretValue>,
    ) -> Result<ProviderProfileRecord, StorageError> {
        validate_provider(&input)?;
        let storage = self.clone();
        let _secret_guard = storage.lock_secrets()?;
        let secret_ref = api_key.map(|value| self.stage_secret(&value)).transpose()?;
        let timestamp = now_millis()?;
        let expected = ProviderProfileRecord {
            id: Uuid::new_v4().to_string(),
            name: input.name,
            kind: input.kind,
            base_url: input.base_url,
            model: input.model,
            context_window_tokens: input.context_window_tokens,
            max_output_tokens: input.max_output_tokens,
            secret_ref,
            revision: 1,
            created_at_ms: timestamp,
            updated_at_ms: timestamp,
        };

        let mutation = (|| -> Result<(), StorageError> {
            let mut connection = self.connection()?;
            let transaction = connection.transaction()?;
            transaction.execute(
                "INSERT INTO provider_profiles (
                    id, name, kind, base_url, model, context_window_tokens,
                    max_output_tokens, secret_ref, revision, created_at_ms, updated_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?9)",
                params![
                    expected.id,
                    expected.name,
                    expected.kind.as_str(),
                    expected.base_url,
                    expected.model,
                    to_sql_i64(expected.context_window_tokens, "provider context window")?,
                    to_sql_i64(expected.max_output_tokens, "provider max output tokens")?,
                    expected.secret_ref.as_ref().map(SecretRef::as_str),
                    timestamp,
                ],
            )?;
            if let Some(reference) = &expected.secret_ref {
                transaction.execute(
                    "DELETE FROM secret_cleanup_queue WHERE secret_ref = ?1",
                    [reference.as_str()],
                )?;
            }
            transaction.commit()?;
            #[cfg(test)]
            if crate::take_fault(crate::FaultPoint::ProviderCreateAfterCommit) {
                return Err(crate::injected_commit_error());
            }
            Ok(())
        })();

        match mutation {
            Ok(()) => Ok(expected),
            Err(error) => match self.get_provider_profile(&expected.id) {
                Ok(Some(actual)) if actual == expected => Ok(actual),
                Ok(None | Some(_)) => match &expected.secret_ref {
                    Some(reference) => Err(self.cleanup_staged_secret(reference, error)),
                    None => Err(error),
                },
                Err(_) => Err(StorageError::OutcomeUnknown {
                    operation: "create provider profile",
                    id: expected.id,
                }),
            },
        }
    }

    /// Loads one provider profile by opaque id.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or persisted-data validation failures.
    pub fn get_provider_profile(
        &self,
        id: &str,
    ) -> Result<Option<ProviderProfileRecord>, StorageError> {
        load_provider(&self.connection()?, id)
    }

    /// Lists provider profiles in stable creation order.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or persisted-data validation failures.
    pub fn list_provider_profiles(&self) -> Result<Vec<ProviderProfileRecord>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, kind, base_url, model, context_window_tokens,
                    max_output_tokens, secret_ref, revision, created_at_ms, updated_at_ms
             FROM provider_profiles ORDER BY created_at_ms, id",
        )?;
        statement
            .query_map([], raw_provider)?
            .map(|row| decode_provider(row?))
            .collect()
    }

    /// Replaces public fields and applies an API-key change using revision CAS.
    ///
    /// # Errors
    ///
    /// Returns validation, not-found, revision-conflict, vault, compensation,
    /// `SQLite`, or unknown-outcome failures.
    #[allow(clippy::too_many_lines)]
    pub fn update_provider_profile(
        &self,
        id: &str,
        expected_revision: u64,
        input: UpdateProviderProfile,
        secret_change: SecretChange,
    ) -> Result<ProviderProfileRecord, StorageError> {
        validate_provider(&input)?;
        let next_revision = expected_revision
            .checked_add(1)
            .ok_or(StorageError::NumericRange("provider revision"))?;
        let next_revision_sql = to_sql_i64(next_revision, "provider revision")?;
        let expected_revision_sql = to_sql_i64(expected_revision, "provider revision")?;
        let storage = self.clone();
        let _secret_guard = storage.lock_secrets()?;
        let current = self
            .get_provider_profile(id)?
            .ok_or_else(|| StorageError::ProviderNotFound(id.to_owned()))?;
        if current.revision != expected_revision {
            return Err(provider_revision_conflict(
                id,
                expected_revision,
                Some(current.revision),
            ));
        }

        let (staged_reference, next_secret_ref) = match secret_change {
            SecretChange::Keep => (None, current.secret_ref.clone()),
            SecretChange::Replace(value) => {
                let reference = self.stage_secret(&value)?;
                (Some(reference.clone()), Some(reference))
            }
            SecretChange::Clear => (None, None),
        };
        let timestamp = now_millis()?;
        let expected = ProviderProfileRecord {
            id: id.to_owned(),
            name: input.name,
            kind: input.kind,
            base_url: input.base_url,
            model: input.model,
            context_window_tokens: input.context_window_tokens,
            max_output_tokens: input.max_output_tokens,
            secret_ref: next_secret_ref,
            revision: next_revision,
            created_at_ms: current.created_at_ms,
            updated_at_ms: timestamp,
        };

        let mutation = (|| -> Result<(), StorageError> {
            let mut connection = self.connection()?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            reject_active_provider_mutation(&transaction, id)?;
            let changed = transaction.execute(
                "UPDATE provider_profiles
                 SET name = ?1, kind = ?2, base_url = ?3, model = ?4,
                     context_window_tokens = ?5, max_output_tokens = ?6, secret_ref = ?7,
                     revision = ?8, updated_at_ms = ?9
                 WHERE id = ?10 AND revision = ?11",
                params![
                    expected.name,
                    expected.kind.as_str(),
                    expected.base_url,
                    expected.model,
                    to_sql_i64(expected.context_window_tokens, "provider context window")?,
                    to_sql_i64(expected.max_output_tokens, "provider max output tokens")?,
                    expected.secret_ref.as_ref().map(SecretRef::as_str),
                    next_revision_sql,
                    timestamp,
                    id,
                    expected_revision_sql,
                ],
            )?;
            if changed != 1 {
                return Err(self.provider_revision_conflict(id, expected_revision)?);
            }
            if let Some(reference) = &staged_reference {
                transaction.execute(
                    "DELETE FROM secret_cleanup_queue WHERE secret_ref = ?1",
                    [reference.as_str()],
                )?;
            }
            if current.secret_ref != expected.secret_ref
                && let Some(reference) = &current.secret_ref
            {
                transaction.execute(
                    "INSERT OR IGNORE INTO secret_cleanup_queue (secret_ref, enqueued_at_ms)
                     VALUES (?1, ?2)",
                    params![reference.as_str(), timestamp],
                )?;
            }
            transaction.commit()?;
            #[cfg(test)]
            if crate::take_fault(crate::FaultPoint::ProviderUpdateAfterCommit) {
                return Err(crate::injected_commit_error());
            }
            Ok(())
        })();

        match mutation {
            Ok(()) => {
                let _ = self.reconcile_secrets_locked();
                Ok(expected)
            }
            Err(error @ StorageError::ProviderRevisionConflict { .. }) => {
                match staged_reference.as_ref() {
                    Some(reference) => Err(self.cleanup_staged_secret(reference, error)),
                    None => Err(error),
                }
            }
            Err(error) => match self.get_provider_profile(id) {
                Ok(Some(actual)) if actual == expected => {
                    let _ = self.reconcile_secrets_locked();
                    Ok(actual)
                }
                Ok(Some(actual)) if actual == current => match staged_reference.as_ref() {
                    Some(reference) => Err(self.cleanup_staged_secret(reference, error)),
                    None => Err(error),
                },
                _ => Err(StorageError::OutcomeUnknown {
                    operation: "update provider profile",
                    id: id.to_owned(),
                }),
            },
        }
    }

    /// Deletes a provider profile using revision CAS.
    ///
    /// # Errors
    ///
    /// Returns not-found, revision-conflict, referential-integrity, vault,
    /// `SQLite`, or unknown-outcome failures.
    pub fn delete_provider_profile(
        &self,
        id: &str,
        expected_revision: u64,
    ) -> Result<(), StorageError> {
        let expected_revision_sql = to_sql_i64(expected_revision, "provider revision")?;
        let storage = self.clone();
        let _secret_guard = storage.lock_secrets()?;
        let current = self
            .get_provider_profile(id)?
            .ok_or_else(|| StorageError::ProviderNotFound(id.to_owned()))?;
        if current.revision != expected_revision {
            return Err(provider_revision_conflict(
                id,
                expected_revision,
                Some(current.revision),
            ));
        }
        let timestamp = now_millis()?;
        let mutation = (|| -> Result<(), StorageError> {
            let mut connection = self.connection()?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            reject_active_provider_mutation(&transaction, id)?;
            reject_provider_delete_if_in_use(&transaction, id)?;
            let changed = transaction.execute(
                "DELETE FROM provider_profiles WHERE id = ?1 AND revision = ?2",
                params![id, expected_revision_sql],
            )?;
            if changed != 1 {
                return Err(self.provider_revision_conflict(id, expected_revision)?);
            }
            if let Some(reference) = &current.secret_ref {
                transaction.execute(
                    "INSERT OR IGNORE INTO secret_cleanup_queue (secret_ref, enqueued_at_ms)
                     VALUES (?1, ?2)",
                    params![reference.as_str(), timestamp],
                )?;
            }
            transaction.commit()?;
            #[cfg(test)]
            if crate::take_fault(crate::FaultPoint::ProviderDeleteAfterCommit) {
                return Err(crate::injected_commit_error());
            }
            Ok(())
        })();

        match mutation {
            Ok(()) => {
                let _ = self.reconcile_secrets_locked();
                Ok(())
            }
            Err(error @ StorageError::ProviderRevisionConflict { .. }) => Err(error),
            Err(error) => match self.get_provider_profile(id) {
                Ok(None) => {
                    let _ = self.reconcile_secrets_locked();
                    Ok(())
                }
                Ok(Some(actual)) if actual == current => Err(error),
                _ => Err(StorageError::OutcomeUnknown {
                    operation: "delete provider profile",
                    id: id.to_owned(),
                }),
            },
        }
    }

    /// Loads provider metadata and the matching API-key revision under one gate.
    ///
    /// # Errors
    ///
    /// Returns not-found, `SQLite`, persisted-data, or vault failures.
    pub fn get_provider_profile_with_secret(
        &self,
        id: &str,
    ) -> Result<(ProviderProfileRecord, Option<SecretValue>), StorageError> {
        let storage = self.clone();
        let _secret_guard = storage.lock_secrets()?;
        let record = self
            .get_provider_profile(id)?
            .ok_or_else(|| StorageError::ProviderNotFound(id.to_owned()))?;
        let Some(reference) = record.secret_ref.as_ref() else {
            return Ok((record, None));
        };
        let secret = self
            .inner
            .vault
            .get(reference)
            .map_err(|source| StorageError::SecretVault {
                operation: "get",
                source,
            })?
            .ok_or(StorageError::SecretVault {
                operation: "get",
                source: crate::SecretVaultError::Backend,
            })?;
        Ok((record, Some(secret)))
    }

    fn provider_revision_conflict(
        &self,
        id: &str,
        expected: u64,
    ) -> Result<StorageError, StorageError> {
        Ok(provider_revision_conflict(
            id,
            expected,
            self.get_provider_profile(id)?.map(|record| record.revision),
        ))
    }
}

fn reject_active_provider_mutation(connection: &Connection, id: &str) -> Result<(), StorageError> {
    let active: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1
            FROM agent_sessions s
            JOIN agent_runs r ON r.session_id = s.id
            WHERE s.provider_id = ?1 AND r.status IN ('running', 'waiting_permission')
         )",
        [id],
        |row| row.get(0),
    )?;
    if active {
        return Err(StorageError::AgentDependencyBusy {
            resource: "provider profile",
            id: id.to_owned(),
        });
    }
    Ok(())
}

fn reject_provider_delete_if_in_use(connection: &Connection, id: &str) -> Result<(), StorageError> {
    let in_use: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE provider_id = ?1)",
        [id],
        |row| row.get(0),
    )?;
    if in_use {
        return Err(StorageError::ProviderInUse(id.to_owned()));
    }
    Ok(())
}

fn validate_provider(input: &CreateProviderProfile) -> Result<(), StorageError> {
    if input.name.trim().is_empty() || input.name.len() > MAX_PROVIDER_NAME_BYTES {
        return Err(StorageError::InvalidProvider(
            "name must be non-empty and at most 512 UTF-8 bytes",
        ));
    }
    if input.base_url.trim().is_empty() || input.base_url.len() > MAX_PROVIDER_URL_BYTES {
        return Err(StorageError::InvalidProvider(
            "base URL must be non-empty and at most 4096 UTF-8 bytes",
        ));
    }
    let authority_and_path = input
        .base_url
        .strip_prefix("https://")
        .or_else(|| input.base_url.strip_prefix("http://"))
        .ok_or(StorageError::InvalidProvider(
            "base URL must use HTTP or HTTPS",
        ))?;
    let authority = authority_and_path
        .split('/')
        .next()
        .ok_or(StorageError::InvalidProvider(
            "base URL authority is missing",
        ))?;
    if authority.is_empty()
        || authority.contains('@')
        || input.base_url.contains('?')
        || input.base_url.contains('#')
    {
        return Err(StorageError::InvalidProvider(
            "base URL cannot contain credentials, query, or fragment data",
        ));
    }
    if input.model.trim().is_empty() || input.model.len() > MAX_PROVIDER_MODEL_BYTES {
        return Err(StorageError::InvalidProvider(
            "model must be non-empty and at most 512 UTF-8 bytes",
        ));
    }
    to_sql_i64(input.context_window_tokens, "provider context window")?;
    if input.context_window_tokens == 0 {
        return Err(StorageError::InvalidProvider(
            "context window must be greater than zero",
        ));
    }
    to_sql_i64(input.max_output_tokens, "provider max output tokens")?;
    if input.max_output_tokens == 0 {
        return Err(StorageError::InvalidProvider(
            "maximum output tokens must be greater than zero",
        ));
    }
    if input.max_output_tokens > input.context_window_tokens {
        return Err(StorageError::InvalidProvider(
            "maximum output tokens cannot exceed the context window",
        ));
    }
    Ok(())
}

type RawProvider = (
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    Option<String>,
    i64,
    i64,
    i64,
);

fn raw_provider(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawProvider> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
    ))
}

fn decode_provider(raw: RawProvider) -> Result<ProviderProfileRecord, StorageError> {
    Ok(ProviderProfileRecord {
        id: raw.0,
        name: raw.1,
        kind: ProviderKind::from_persisted(&raw.2)?,
        base_url: raw.3,
        model: raw.4,
        context_window_tokens: from_sql_u64(raw.5, "provider context window")?,
        max_output_tokens: from_sql_u64(raw.6, "provider max output tokens")?,
        secret_ref: raw.7.map(SecretRef::from_persisted).transpose()?,
        revision: from_sql_u64(raw.8, "provider revision")?,
        created_at_ms: raw.9,
        updated_at_ms: raw.10,
    })
}

fn load_provider(
    connection: &Connection,
    id: &str,
) -> Result<Option<ProviderProfileRecord>, StorageError> {
    let raw = connection
        .query_row(
            "SELECT id, name, kind, base_url, model, context_window_tokens,
                    max_output_tokens, secret_ref, revision, created_at_ms, updated_at_ms
             FROM provider_profiles WHERE id = ?1",
            [id],
            raw_provider,
        )
        .optional()?;
    raw.map(decode_provider).transpose()
}

fn provider_revision_conflict(id: &str, expected: u64, actual: Option<u64>) -> StorageError {
    StorageError::ProviderRevisionConflict {
        id: id.to_owned(),
        expected,
        actual,
    }
}

fn to_sql_i64(value: u64, label: &'static str) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::NumericRange(label))
}

fn from_sql_u64(value: i64, label: &'static str) -> Result<u64, StorageError> {
    u64::try_from(value).map_err(|_| StorageError::NumericRange(label))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        fs,
        sync::{Arc, Mutex},
    };

    use tempfile::TempDir;

    use super::{CreateProviderProfile, ProviderKind};
    use crate::{
        CreateAgentSession, LOCK_FILE, SecretChange, SecretRef, SecretValue, SecretVault,
        SecretVaultError, Storage, StorageError,
    };

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

    fn input(name: &str) -> CreateProviderProfile {
        CreateProviderProfile {
            name: name.to_owned(),
            kind: ProviderKind::OpenAiCompatible,
            base_url: "https://provider.example/v1".to_owned(),
            model: "model-1".to_owned(),
            context_window_tokens: 128_000,
            max_output_tokens: 8_192,
        }
    }

    #[test]
    fn provider_key_never_reaches_sqlite_or_debug_and_rotation_cleans_old_key() {
        let directory = TempDir::new().expect("temp dir");
        let vault = Arc::new(MemoryVault::default());
        let storage = Storage::open(directory.path(), vault.clone()).expect("storage opens");
        let sentinel = "provider-key-must-never-reach-sqlite";
        let created = storage
            .create_provider_profile(
                input("primary"),
                Some(SecretValue::new(sentinel.as_bytes().to_vec())),
            )
            .expect("provider creates");
        assert!(!format!("{created:?}").contains(sentinel));
        assert_eq!(created.max_output_tokens, 8_192);

        for entry in fs::read_dir(directory.path()).expect("data dir reads") {
            let path = entry.expect("directory entry").path();
            if path.is_file() && path.file_name().is_some_and(|name| name != LOCK_FILE) {
                let bytes = fs::read(path).expect("storage file reads");
                assert!(
                    !bytes
                        .windows(sentinel.len())
                        .any(|window| window == sentinel.as_bytes())
                );
            }
        }

        let old_reference = created.secret_ref.clone().expect("old key ref");
        let mut replacement = input("primary-updated");
        replacement.kind = ProviderKind::Anthropic;
        replacement.max_output_tokens = 4_096;
        let updated = storage
            .update_provider_profile(
                &created.id,
                created.revision,
                replacement,
                SecretChange::Replace(SecretValue::new(b"replacement-key".to_vec())),
            )
            .expect("provider updates");
        assert_eq!(updated.revision, 2);
        assert_eq!(updated.max_output_tokens, 4_096);
        assert!(vault.get(&old_reference).expect("old key reads").is_none());
        let active_reference = updated.secret_ref.clone().expect("active key ref");
        storage
            .connection()
            .expect("connection opens")
            .execute(
                "INSERT INTO secret_cleanup_queue (secret_ref, enqueued_at_ms) VALUES (?1, 0)",
                [active_reference.as_str()],
            )
            .expect("active cleanup fixture inserts");
        let cleanup = storage.reconcile_secrets().expect("cleanup reconciles");
        assert_eq!(cleanup.deleted, 0);
        assert_eq!(cleanup.pending, 1);
        assert!(
            vault
                .get(&active_reference)
                .expect("active key reads")
                .is_some()
        );

        let stale = storage
            .update_provider_profile(
                &created.id,
                created.revision,
                input("stale"),
                SecretChange::Keep,
            )
            .expect_err("stale revision fails");
        assert!(matches!(
            stale,
            StorageError::ProviderRevisionConflict {
                actual: Some(2),
                ..
            }
        ));
    }

    #[test]
    fn provider_delete_reports_a_structured_conflict_while_a_session_uses_it() {
        let directory = TempDir::new().expect("temp dir");
        let storage = Storage::open(directory.path(), Arc::new(MemoryVault::default()))
            .expect("storage opens");
        let provider = storage
            .create_provider_profile(input("primary"), None)
            .expect("provider creates");
        let session = storage
            .create_agent_session(CreateAgentSession {
                title: "Session".to_owned(),
                provider_id: provider.id.clone(),
                datasource_id: None,
                system_prompt: None,
            })
            .expect("session creates");

        assert!(matches!(
            storage.delete_provider_profile(&provider.id, provider.revision),
            Err(StorageError::ProviderInUse(id)) if id == provider.id
        ));
        assert!(
            storage
                .get_provider_profile(&provider.id)
                .expect("provider reads")
                .is_some()
        );

        storage
            .delete_agent_session(&session.id, session.revision)
            .expect("session deletes");
        storage
            .delete_provider_profile(&provider.id, provider.revision)
            .expect("unused provider deletes");
    }

    #[test]
    fn provider_mutations_reconcile_post_commit_failures_and_cleanup_keys() {
        let directory = TempDir::new().expect("temp dir");
        let vault = Arc::new(MemoryVault::default());
        let storage = Storage::open(directory.path(), vault.clone()).expect("storage opens");
        crate::inject_faults(&[
            crate::FaultPoint::ProviderCreateAfterCommit,
            crate::FaultPoint::ProviderUpdateAfterCommit,
            crate::FaultPoint::ProviderDeleteAfterCommit,
        ]);

        let created = storage
            .create_provider_profile(
                input("primary"),
                Some(SecretValue::new(b"first-key".to_vec())),
            )
            .expect("committed create reconciles");
        let updated = storage
            .update_provider_profile(
                &created.id,
                created.revision,
                input("updated"),
                SecretChange::Replace(SecretValue::new(b"second-key".to_vec())),
            )
            .expect("committed update reconciles");
        storage
            .delete_provider_profile(&updated.id, updated.revision)
            .expect("committed delete reconciles");

        assert!(
            storage
                .get_provider_profile(&updated.id)
                .expect("provider reads")
                .is_none()
        );
        assert!(vault.values.lock().expect("vault lock").is_empty());
        assert_eq!(storage.reconcile_secrets().expect("queue reads").pending, 0);
    }
}
