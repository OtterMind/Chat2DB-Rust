use std::fmt::Formatter;

use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::{SecretRef, SecretValue, Storage, StorageError, now_millis};

const MAX_NAME_BYTES: usize = 512;
const MAX_DRIVER_ID_BYTES: usize = 255;

/// Fields required to create a datasource record.
#[derive(Debug, Clone)]
pub struct CreateDatasource {
    /// User-visible datasource name.
    pub name: String,
    /// Compatibility-engine driver id.
    pub driver_id: String,
}

/// Public fields replaced by an optimistic datasource update.
#[derive(Debug, Clone)]
pub struct UpdateDatasource {
    /// User-visible datasource name.
    pub name: String,
    /// Compatibility-engine driver id.
    pub driver_id: String,
}

/// Requested secret mutation for a datasource update.
pub enum SecretChange {
    /// Keep the existing immutable vault reference.
    Keep,
    /// Stage a new immutable vault reference and retire the old one after CAS.
    Replace(SecretValue),
    /// Remove the active vault reference.
    Clear,
}

impl std::fmt::Debug for SecretChange {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keep => formatter.write_str("Keep"),
            Self::Replace(_) => formatter.write_str("Replace([REDACTED])"),
            Self::Clear => formatter.write_str("Clear"),
        }
    }
}

/// Durable datasource metadata. Secret material is represented only by a ref.
#[derive(Clone, PartialEq, Eq)]
pub struct DatasourceRecord {
    /// Opaque datasource id.
    pub id: String,
    /// User-visible datasource name.
    pub name: String,
    /// Compatibility-engine driver id.
    pub driver_id: String,
    /// Opaque OS-vault reference, never the secret itself.
    pub secret_ref: Option<SecretRef>,
    /// Monotonic optimistic-concurrency revision.
    pub revision: u64,
    /// Creation time as Unix epoch milliseconds.
    pub created_at_ms: i64,
    /// Last update time as Unix epoch milliseconds.
    pub updated_at_ms: i64,
}

impl std::fmt::Debug for DatasourceRecord {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DatasourceRecord")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("driver_id", &self.driver_id)
            .field("secret_ref", &self.secret_ref)
            .field("revision", &self.revision)
            .field("created_at_ms", &self.created_at_ms)
            .field("updated_at_ms", &self.updated_at_ms)
            .finish()
    }
}

/// Result of retrying deferred secret deletions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretCleanupReport {
    /// References successfully removed from the vault and queue.
    pub deleted: usize,
    /// References still awaiting vault deletion.
    pub pending: usize,
}

impl Storage {
    /// Creates a datasource and optionally installs its credential atomically
    /// across the `SQLite`/vault compensation boundary.
    ///
    /// # Errors
    ///
    /// Returns validation, `SQLite`, vault, or unknown-commit failures. A staged
    /// secret is deleted on a proven `SQLite` rollback and otherwise remains in
    /// the durable cleanup queue.
    pub fn create_datasource(
        &self,
        input: CreateDatasource,
        secret: Option<SecretValue>,
    ) -> Result<DatasourceRecord, StorageError> {
        self.create_datasource_with_id(Uuid::new_v4().to_string(), input, secret)
    }

    fn create_datasource_with_id(
        &self,
        id: String,
        input: CreateDatasource,
        secret: Option<SecretValue>,
    ) -> Result<DatasourceRecord, StorageError> {
        validate_datasource(&input.name, &input.driver_id)?;
        let storage = self.clone();
        let _secret_guard = storage.lock_secrets()?;
        let secret_ref = match secret {
            Some(value) => Some(self.stage_secret(&value)?),
            None => None,
        };
        let timestamp = now_millis()?;
        let expected = DatasourceRecord {
            id,
            name: input.name,
            driver_id: input.driver_id,
            secret_ref,
            revision: 1,
            created_at_ms: timestamp,
            updated_at_ms: timestamp,
        };

        let mutation = (|| -> Result<(), StorageError> {
            let mut connection = self.connection()?;
            let transaction = connection.transaction()?;
            transaction.execute(
                "INSERT INTO datasources (
                    id, name, driver_id, secret_ref, revision,
                    created_at_ms, updated_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)",
                params![
                    expected.id,
                    expected.name,
                    expected.driver_id,
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
            #[cfg(test)]
            if crate::take_fault(crate::FaultPoint::DatasourceCreateBeforeCommit) {
                return Err(crate::injected_commit_error());
            }
            transaction.commit()?;
            #[cfg(test)]
            if crate::take_fault(crate::FaultPoint::DatasourceCreateAfterCommit) {
                return Err(crate::injected_commit_error());
            }
            Ok(())
        })();

        match mutation {
            Ok(()) => Ok(expected),
            Err(error) => self.reconcile_create_commit(&expected, error),
        }
    }

    /// Loads one datasource by opaque id.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or persisted-data validation failures.
    pub fn get_datasource(&self, id: &str) -> Result<Option<DatasourceRecord>, StorageError> {
        let connection = self.connection()?;
        load_datasource(&connection, id)
    }

    /// Lists datasource metadata in stable creation order.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or persisted-data validation failures.
    pub fn list_datasources(&self) -> Result<Vec<DatasourceRecord>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, driver_id, secret_ref,
                    revision, created_at_ms, updated_at_ms
             FROM datasources ORDER BY created_at_ms, id",
        )?;
        let rows = statement.query_map([], raw_datasource)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(decode_datasource(row?)?);
        }
        Ok(records)
    }

    /// Replaces public fields and applies a secret change using revision CAS.
    ///
    /// # Errors
    ///
    /// Returns `RevisionConflict` when another writer won, while safely
    /// compensating a newly staged secret.
    pub fn update_datasource(
        &self,
        id: &str,
        expected_revision: u64,
        input: UpdateDatasource,
        secret_change: SecretChange,
    ) -> Result<DatasourceRecord, StorageError> {
        validate_datasource(&input.name, &input.driver_id)?;
        let storage = self.clone();
        let _secret_guard = storage.lock_secrets()?;
        let current = self
            .get_datasource(id)?
            .ok_or_else(|| StorageError::DatasourceNotFound(id.to_owned()))?;
        if current.revision != expected_revision {
            return Err(StorageError::RevisionConflict {
                id: id.to_owned(),
                expected: expected_revision,
                actual: Some(current.revision),
            });
        }

        let (staged_reference, next_secret_ref) = match secret_change {
            SecretChange::Keep => (None, current.secret_ref.clone()),
            SecretChange::Replace(value) => {
                let reference = self.stage_secret(&value)?;
                (Some(reference.clone()), Some(reference))
            }
            SecretChange::Clear => (None, None),
        };
        let next_revision = expected_revision
            .checked_add(1)
            .ok_or(StorageError::NumericRange("datasource revision"))?;
        let next_revision_sql = i64::try_from(next_revision)
            .map_err(|_| StorageError::NumericRange("datasource revision"))?;
        let timestamp = now_millis()?;
        let expected = DatasourceRecord {
            id: id.to_owned(),
            name: input.name,
            driver_id: input.driver_id,
            secret_ref: next_secret_ref,
            revision: next_revision,
            created_at_ms: current.created_at_ms,
            updated_at_ms: timestamp,
        };

        let mutation = (|| -> Result<(), StorageError> {
            let mut connection = self.connection()?;
            let transaction = connection.transaction()?;
            let changed = transaction.execute(
                "UPDATE datasources
                 SET name = ?1, driver_id = ?2, secret_ref = ?3,
                     revision = ?4, updated_at_ms = ?5
                 WHERE id = ?6 AND revision = ?7",
                params![
                    expected.name,
                    expected.driver_id,
                    expected.secret_ref.as_ref().map(SecretRef::as_str),
                    next_revision_sql,
                    timestamp,
                    id,
                    i64::try_from(expected_revision)
                        .map_err(|_| StorageError::NumericRange("datasource revision"))?,
                ],
            )?;
            if changed != 1 {
                return Err(self.revision_conflict(id, expected_revision)?);
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
            if crate::take_fault(crate::FaultPoint::DatasourceUpdateAfterCommit) {
                return Err(crate::injected_commit_error());
            }
            Ok(())
        })();

        match mutation {
            Ok(()) => {
                let _ = self.reconcile_secrets_locked();
                Ok(expected)
            }
            Err(error) => {
                self.reconcile_update_commit(&current, &expected, staged_reference.as_ref(), error)
            }
        }
    }

    /// Deletes a datasource using revision CAS and retires its vault reference.
    ///
    /// # Errors
    ///
    /// Returns not-found, revision-conflict, `SQLite`, or unknown-outcome errors.
    pub fn delete_datasource(&self, id: &str, expected_revision: u64) -> Result<(), StorageError> {
        let storage = self.clone();
        let _secret_guard = storage.lock_secrets()?;
        let current = self
            .get_datasource(id)?
            .ok_or_else(|| StorageError::DatasourceNotFound(id.to_owned()))?;
        if current.revision != expected_revision {
            return Err(StorageError::RevisionConflict {
                id: id.to_owned(),
                expected: expected_revision,
                actual: Some(current.revision),
            });
        }
        let timestamp = now_millis()?;
        let mutation = (|| -> Result<(), StorageError> {
            let mut connection = self.connection()?;
            let transaction = connection.transaction()?;
            let changed = transaction.execute(
                "DELETE FROM datasources WHERE id = ?1 AND revision = ?2",
                params![
                    id,
                    i64::try_from(expected_revision)
                        .map_err(|_| StorageError::NumericRange("datasource revision"))?,
                ],
            )?;
            if changed != 1 {
                return Err(self.revision_conflict(id, expected_revision)?);
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
            if crate::take_fault(crate::FaultPoint::DatasourceDeleteAfterCommit) {
                return Err(crate::injected_commit_error());
            }
            Ok(())
        })();

        match mutation {
            Ok(()) => {
                let _ = self.reconcile_secrets_locked();
                Ok(())
            }
            Err(error @ StorageError::RevisionConflict { .. }) => Err(error),
            Err(error) => match self.get_datasource(id) {
                Ok(None) => {
                    let _ = self.reconcile_secrets_locked();
                    Ok(())
                }
                Ok(Some(actual)) if actual == current => Err(error),
                _ => Err(StorageError::OutcomeUnknown {
                    operation: "delete datasource",
                    id: id.to_owned(),
                }),
            },
        }
    }

    /// Loads a datasource secret from the external vault.
    ///
    /// # Errors
    ///
    /// Returns datasource, `SQLite`, or vault failures. A referenced but missing
    /// vault entry is treated as a backend failure rather than an empty secret.
    pub fn resolve_datasource_secret(&self, id: &str) -> Result<Option<SecretValue>, StorageError> {
        let storage = self.clone();
        let _secret_guard = storage.lock_secrets()?;
        let record = self
            .get_datasource(id)?
            .ok_or_else(|| StorageError::DatasourceNotFound(id.to_owned()))?;
        let Some(reference) = record.secret_ref else {
            return Ok(None);
        };
        self.inner
            .vault
            .get(&reference)
            .map_err(|source| StorageError::SecretVault {
                operation: "get",
                source,
            })?
            .map(Some)
            .ok_or(StorageError::SecretVault {
                operation: "get",
                source: crate::SecretVaultError::Backend,
            })
    }

    /// Retries idempotent deletion of superseded or failed staged secrets.
    ///
    /// Vault failures leave references queued and do not roll back already
    /// committed datasource mutations.
    ///
    /// # Errors
    ///
    /// Returns only `SQLite` failures; individual vault failures are counted as
    /// pending cleanup.
    pub fn reconcile_secrets(&self) -> Result<SecretCleanupReport, StorageError> {
        let storage = self.clone();
        let _secret_guard = storage.lock_secrets()?;
        self.reconcile_secrets_locked()
    }

    fn reconcile_secrets_locked(&self) -> Result<SecretCleanupReport, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT q.secret_ref
             FROM secret_cleanup_queue q
             WHERE NOT EXISTS (
                 SELECT 1 FROM datasources d WHERE d.secret_ref = q.secret_ref
             )
             ORDER BY q.enqueued_at_ms, q.secret_ref",
        )?;
        let references = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);

        let mut deleted = 0_usize;
        for raw_reference in references {
            let reference = SecretRef::from_persisted(raw_reference)?;
            if self.inner.vault.delete(&reference).is_ok() {
                connection.execute(
                    "DELETE FROM secret_cleanup_queue WHERE secret_ref = ?1
                     AND NOT EXISTS (
                         SELECT 1 FROM datasources WHERE secret_ref = ?1
                     )",
                    [reference.as_str()],
                )?;
                deleted += 1;
            }
        }
        let pending: i64 =
            connection.query_row("SELECT COUNT(*) FROM secret_cleanup_queue", [], |row| {
                row.get(0)
            })?;
        Ok(SecretCleanupReport {
            deleted,
            pending: usize::try_from(pending)
                .map_err(|_| StorageError::NumericRange("secret cleanup count"))?,
        })
    }

    fn stage_secret(&self, value: &SecretValue) -> Result<SecretRef, StorageError> {
        let reference = SecretRef::generate();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO secret_cleanup_queue (secret_ref, enqueued_at_ms) VALUES (?1, ?2)",
            params![reference.as_str(), now_millis()?],
        )?;
        self.inner
            .vault
            .create(&reference, value)
            .map_err(|source| StorageError::SecretVault {
                operation: "create",
                source,
            })?;
        Ok(reference)
    }

    fn cleanup_staged_secret(&self, reference: &SecretRef, primary: StorageError) -> StorageError {
        match self.inner.vault.delete(reference) {
            Ok(()) => {
                if let Ok(connection) = self.connection() {
                    let _ = connection.execute(
                        "DELETE FROM secret_cleanup_queue WHERE secret_ref = ?1
                         AND NOT EXISTS (
                             SELECT 1 FROM datasources WHERE secret_ref = ?1
                         )",
                        [reference.as_str()],
                    );
                }
                primary
            }
            Err(compensation) => StorageError::SecretCompensation {
                primary: Box::new(primary),
                compensation,
            },
        }
    }

    fn reconcile_create_commit(
        &self,
        expected: &DatasourceRecord,
        error: StorageError,
    ) -> Result<DatasourceRecord, StorageError> {
        let primary = error;
        match self.get_datasource(&expected.id) {
            Ok(Some(actual)) if actual == *expected => Ok(actual),
            Ok(None | Some(_)) => match &expected.secret_ref {
                Some(reference) => Err(self.cleanup_staged_secret(reference, primary)),
                None => Err(primary),
            },
            Err(_) => Err(StorageError::OutcomeUnknown {
                operation: "create datasource",
                id: expected.id.clone(),
            }),
        }
    }

    fn reconcile_update_commit(
        &self,
        previous: &DatasourceRecord,
        expected: &DatasourceRecord,
        staged_reference: Option<&SecretRef>,
        error: StorageError,
    ) -> Result<DatasourceRecord, StorageError> {
        if matches!(error, StorageError::RevisionConflict { .. }) {
            return match staged_reference {
                Some(reference) => Err(self.cleanup_staged_secret(reference, error)),
                None => Err(error),
            };
        }
        match self.get_datasource(&expected.id) {
            Ok(Some(actual)) if actual == *expected => {
                let _ = self.reconcile_secrets_locked();
                Ok(actual)
            }
            Ok(Some(actual)) if actual == *previous => match staged_reference {
                Some(reference) => Err(self.cleanup_staged_secret(reference, error)),
                None => Err(error),
            },
            _ => Err(StorageError::OutcomeUnknown {
                operation: "update datasource",
                id: expected.id.clone(),
            }),
        }
    }

    fn revision_conflict(&self, id: &str, expected: u64) -> Result<StorageError, StorageError> {
        Ok(StorageError::RevisionConflict {
            id: id.to_owned(),
            expected,
            actual: self.get_datasource(id)?.map(|record| record.revision),
        })
    }
}

fn validate_datasource(name: &str, driver_id: &str) -> Result<(), StorageError> {
    if name.trim().is_empty() || name.len() > MAX_NAME_BYTES {
        return Err(StorageError::InvalidDatasource(
            "name must be non-empty and at most 512 UTF-8 bytes",
        ));
    }
    if driver_id.is_empty() || driver_id.len() > MAX_DRIVER_ID_BYTES {
        return Err(StorageError::InvalidDatasource(
            "driver id must be non-empty and at most 255 UTF-8 bytes",
        ));
    }
    Ok(())
}

type RawDatasource = (String, String, String, Option<String>, i64, i64, i64);

fn raw_datasource(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawDatasource> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

fn decode_datasource(raw: RawDatasource) -> Result<DatasourceRecord, StorageError> {
    let (id, name, driver_id, secret_ref, revision, created, updated) = raw;
    Ok(DatasourceRecord {
        id,
        name,
        driver_id,
        secret_ref: secret_ref.map(SecretRef::from_persisted).transpose()?,
        revision: u64::try_from(revision)
            .map_err(|_| StorageError::NumericRange("datasource revision"))?,
        created_at_ms: created,
        updated_at_ms: updated,
    })
}

fn load_datasource(
    connection: &Connection,
    id: &str,
) -> Result<Option<DatasourceRecord>, StorageError> {
    let raw = connection
        .query_row(
            "SELECT id, name, driver_id, secret_ref,
                    revision, created_at_ms, updated_at_ms
             FROM datasources WHERE id = ?1",
            [id],
            raw_datasource,
        )
        .optional()?;
    raw.map(decode_datasource).transpose()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        fs,
        sync::{
            Arc, Barrier, Mutex, OnceLock, Weak,
            mpsc::{self, SyncSender, TryRecvError},
        },
        thread,
        time::Duration,
    };

    use tempfile::TempDir;

    use super::{CreateDatasource, SecretChange, UpdateDatasource};
    use crate::{SecretRef, SecretValue, SecretVault, SecretVaultError, Storage, StorageError};

    #[derive(Default)]
    struct MemoryVault {
        values: Mutex<HashMap<String, Vec<u8>>>,
        fail_create_after_store: Mutex<bool>,
        fail_delete: Mutex<bool>,
        storage: OnceLock<Weak<crate::StorageInner>>,
    }

    impl MemoryVault {
        fn assert_secret_gate_held(&self) {
            let Some(storage) = self.storage.get().and_then(Weak::upgrade) else {
                return;
            };
            assert!(
                storage.secret_gate.try_lock().is_err(),
                "vault operations must run under the datasource secret gate"
            );
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
            self.assert_secret_gate_held();
            self.values.lock().expect("vault lock").insert(
                reference.as_str().to_owned(),
                value.expose_secret().to_vec(),
            );
            if *self
                .fail_create_after_store
                .lock()
                .expect("create flag lock")
            {
                return Err(SecretVaultError::Backend);
            }
            Ok(())
        }

        fn get(&self, reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
            self.assert_secret_gate_held();
            Ok(self
                .values
                .lock()
                .expect("vault lock")
                .get(reference.as_str())
                .cloned()
                .map(SecretValue::new))
        }

        fn delete(&self, reference: &SecretRef) -> Result<(), SecretVaultError> {
            self.assert_secret_gate_held();
            if *self.fail_delete.lock().expect("delete flag lock") {
                return Err(SecretVaultError::Backend);
            }
            self.values
                .lock()
                .expect("vault lock")
                .remove(reference.as_str());
            Ok(())
        }
    }

    struct BlockingCreateVault {
        values: Mutex<HashMap<String, Vec<u8>>>,
        create_started: Arc<Barrier>,
        release_create: Arc<Barrier>,
        delete_events: SyncSender<()>,
    }

    impl SecretVault for BlockingCreateVault {
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
            self.create_started.wait();
            self.release_create.wait();
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
            let _ = self.delete_events.try_send(());
            Ok(())
        }
    }

    fn input(name: &str) -> CreateDatasource {
        CreateDatasource {
            name: name.to_owned(),
            driver_id: "sha256:driver".to_owned(),
        }
    }

    #[test]
    fn datasource_secret_never_reaches_sqlite_or_debug_output() {
        let directory = TempDir::new().expect("temp dir");
        let vault = Arc::new(MemoryVault::default());
        let storage = Storage::open(directory.path(), vault.clone()).expect("storage opens");
        let sentinel = "stage4-sentinel-password-never-persist";

        let record = storage
            .create_datasource(
                input("warehouse"),
                Some(SecretValue::new(sentinel.as_bytes().to_vec())),
            )
            .expect("datasource creates");
        assert!(!format!("{record:?}").contains(sentinel));
        drop(storage);

        for entry in fs::read_dir(directory.path()).expect("data dir reads") {
            let path = entry.expect("directory entry").path();
            if path.is_file() {
                let bytes = fs::read(path).expect("storage file reads");
                assert!(
                    !bytes
                        .windows(sentinel.len())
                        .any(|window| window == sentinel.as_bytes())
                );
            }
        }
    }

    #[test]
    fn staging_resolution_and_cleanup_share_one_secret_gate() {
        let directory = TempDir::new().expect("temp dir");
        let vault = Arc::new(MemoryVault::default());
        let storage = Storage::open(directory.path(), vault.clone()).expect("storage opens");
        vault
            .storage
            .set(Arc::downgrade(&storage.inner))
            .expect("storage weak reference sets once");

        let created = storage
            .create_datasource(
                input("warehouse"),
                Some(SecretValue::new(b"first-secret".to_vec())),
            )
            .expect("datasource creates");
        assert!(
            storage
                .resolve_datasource_secret(&created.id)
                .expect("secret resolves")
                .is_some()
        );
        let updated = storage
            .update_datasource(
                &created.id,
                created.revision,
                UpdateDatasource {
                    name: "warehouse".to_owned(),
                    driver_id: created.driver_id,
                },
                SecretChange::Replace(SecretValue::new(b"second-secret".to_vec())),
            )
            .expect("secret rotates");
        storage
            .delete_datasource(&updated.id, updated.revision)
            .expect("datasource deletes");

        let orphan = SecretRef::generate();
        vault
            .values
            .lock()
            .expect("vault lock")
            .insert(orphan.as_str().to_owned(), b"orphan".to_vec());
        storage
            .connection()
            .expect("connection opens")
            .execute(
                "INSERT INTO secret_cleanup_queue (secret_ref, enqueued_at_ms) VALUES (?1, 0)",
                [orphan.as_str()],
            )
            .expect("orphan cleanup intent inserts");
        let report = storage.reconcile_secrets().expect("orphan reconciles");
        assert_eq!(report.deleted, 1);
        assert!(vault.values.lock().expect("vault lock").is_empty());
    }

    #[test]
    fn concurrent_cleanup_cannot_delete_a_staged_secret_before_datasource_commit() {
        let directory = TempDir::new().expect("temp dir");
        let create_started = Arc::new(Barrier::new(2));
        let release_create = Arc::new(Barrier::new(2));
        let (delete_sender, delete_receiver) = mpsc::sync_channel(1);
        let vault = Arc::new(BlockingCreateVault {
            values: Mutex::new(HashMap::new()),
            create_started: create_started.clone(),
            release_create: release_create.clone(),
            delete_events: delete_sender,
        });
        let storage = Storage::open(directory.path(), vault).expect("storage opens");

        let create_storage = storage.clone();
        let create = thread::spawn(move || {
            create_storage.create_datasource(
                input("staged"),
                Some(SecretValue::new(b"staged-secret".to_vec())),
            )
        });
        create_started.wait();

        let cleanup_storage = storage.clone();
        let cleanup = thread::spawn(move || cleanup_storage.reconcile_secrets());
        let early_delete = delete_receiver.recv_timeout(Duration::from_secs(1));
        release_create.wait();
        let created = create
            .join()
            .expect("create thread joins")
            .expect("datasource commits");
        let report = cleanup
            .join()
            .expect("cleanup thread joins")
            .expect("cleanup succeeds");
        assert!(matches!(early_delete, Err(mpsc::RecvTimeoutError::Timeout)));
        assert_eq!(report.deleted, 0);
        assert!(
            storage
                .resolve_datasource_secret(&created.id)
                .expect("active secret resolves")
                .is_some()
        );
        assert!(matches!(
            delete_receiver.try_recv(),
            Err(TryRecvError::Empty)
        ));
    }

    #[test]
    fn revisions_detect_stale_updates_and_secret_rotation_is_compensated() {
        let directory = TempDir::new().expect("temp dir");
        let vault = Arc::new(MemoryVault::default());
        let storage = Storage::open(directory.path(), vault.clone()).expect("storage opens");
        let created = storage
            .create_datasource(
                input("warehouse"),
                Some(SecretValue::new(b"old-secret".to_vec())),
            )
            .expect("datasource creates");
        let old_reference = created.secret_ref.clone().expect("secret ref");

        let updated = storage
            .update_datasource(
                &created.id,
                created.revision,
                UpdateDatasource {
                    name: "warehouse-primary".to_owned(),
                    driver_id: created.driver_id.clone(),
                },
                SecretChange::Replace(SecretValue::new(b"new-secret".to_vec())),
            )
            .expect("datasource updates");
        assert_eq!(updated.revision, 2);
        assert_ne!(updated.secret_ref, Some(old_reference.clone()));
        assert!(vault.get(&old_reference).expect("vault reads").is_none());

        let conflict = storage
            .update_datasource(
                &created.id,
                created.revision,
                UpdateDatasource {
                    name: "stale".to_owned(),
                    driver_id: created.driver_id,
                },
                SecretChange::Keep,
            )
            .expect_err("stale revision must fail");
        assert!(matches!(
            conflict,
            StorageError::RevisionConflict {
                actual: Some(2),
                ..
            }
        ));
    }

    #[test]
    fn datasource_mutations_reconcile_post_commit_failures() {
        let directory = TempDir::new().expect("temp dir");
        let vault = Arc::new(MemoryVault::default());
        let storage = Storage::open(directory.path(), vault.clone()).expect("storage opens");
        crate::inject_faults(&[
            crate::FaultPoint::DatasourceCreateAfterCommit,
            crate::FaultPoint::DatasourceUpdateAfterCommit,
            crate::FaultPoint::DatasourceDeleteAfterCommit,
        ]);

        let created = storage
            .create_datasource(
                input("warehouse"),
                Some(SecretValue::new(b"first-secret".to_vec())),
            )
            .expect("committed create reconciles");
        let updated = storage
            .update_datasource(
                &created.id,
                created.revision,
                UpdateDatasource {
                    name: "warehouse-primary".to_owned(),
                    driver_id: created.driver_id,
                },
                SecretChange::Replace(SecretValue::new(b"second-secret".to_vec())),
            )
            .expect("committed update reconciles");
        storage
            .delete_datasource(&updated.id, updated.revision)
            .expect("committed delete reconciles");

        assert!(
            storage
                .get_datasource(&updated.id)
                .expect("datasource reads")
                .is_none()
        );
        assert!(vault.values.lock().expect("vault lock").is_empty());
        assert_eq!(storage.reconcile_secrets().expect("queue reads").pending, 0);
    }

    #[test]
    fn datasource_create_rolls_back_and_compensates_a_pre_commit_failure() {
        let directory = TempDir::new().expect("temp dir");
        let vault = Arc::new(MemoryVault::default());
        let storage = Storage::open(directory.path(), vault.clone()).expect("storage opens");
        crate::inject_faults(&[crate::FaultPoint::DatasourceCreateBeforeCommit]);

        let error = storage
            .create_datasource(
                input("warehouse"),
                Some(SecretValue::new(b"staged-secret".to_vec())),
            )
            .expect_err("pre-commit failure must surface");
        assert!(matches!(error, StorageError::Integrity(_)));
        assert!(
            storage
                .list_datasources()
                .expect("datasources list")
                .is_empty()
        );
        assert!(vault.values.lock().expect("vault lock").is_empty());
        assert_eq!(storage.reconcile_secrets().expect("queue reads").pending, 0);
    }

    #[test]
    fn failed_create_deletes_the_staged_secret_or_leaves_a_retry_intent() {
        let directory = TempDir::new().expect("temp dir");
        let vault = Arc::new(MemoryVault::default());
        let storage = Storage::open(directory.path(), vault.clone()).expect("storage opens");
        let fixed_id = "8df08cac-24e7-446d-92ea-cda948ec9184".to_owned();
        storage
            .create_datasource_with_id(fixed_id.clone(), input("first"), None)
            .expect("first datasource creates");

        *vault.fail_delete.lock().expect("delete flag lock") = true;
        let error = storage
            .create_datasource_with_id(
                fixed_id,
                input("duplicate"),
                Some(SecretValue::new(b"staged-secret".to_vec())),
            )
            .expect_err("duplicate id must fail");
        assert!(matches!(error, StorageError::SecretCompensation { .. }));

        *vault.fail_delete.lock().expect("delete flag lock") = false;
        let report = storage.reconcile_secrets().expect("cleanup retries");
        assert_eq!(report.deleted, 1);
        assert_eq!(report.pending, 0);
    }

    #[test]
    fn startup_retries_pending_secret_compensation_without_touching_active_refs() {
        let directory = TempDir::new().expect("temp dir");
        let vault = Arc::new(MemoryVault::default());
        let storage = Storage::open(directory.path(), vault.clone()).expect("storage opens");
        let record = storage
            .create_datasource(
                input("active"),
                Some(SecretValue::new(b"active-secret".to_vec())),
            )
            .expect("active datasource creates");
        let active_ref = record.secret_ref.clone().expect("active secret ref");
        storage
            .connection()
            .expect("connection opens")
            .execute(
                "INSERT INTO secret_cleanup_queue (secret_ref, enqueued_at_ms) VALUES (?1, 0)",
                [active_ref.as_str()],
            )
            .expect("test cleanup intent inserts");
        drop(storage);

        let reopened = Storage::open(directory.path(), vault.clone()).expect("storage reopens");
        assert_eq!(reopened.startup_report().secrets.deleted, 0);
        assert_eq!(reopened.startup_report().secrets.pending, 1);
        assert!(vault.get(&active_ref).expect("vault reads").is_some());
    }

    #[test]
    fn startup_cleans_a_secret_after_unknown_vault_create_outcome() {
        let directory = TempDir::new().expect("temp dir");
        let vault = Arc::new(MemoryVault::default());
        let storage = Storage::open(directory.path(), vault.clone()).expect("storage opens");
        *vault
            .fail_create_after_store
            .lock()
            .expect("create flag lock") = true;

        let error = storage
            .create_datasource(
                input("failed"),
                Some(SecretValue::new(b"possibly-stored".to_vec())),
            )
            .expect_err("vault create outcome must fail");
        assert!(matches!(
            error,
            StorageError::SecretVault {
                operation: "create",
                ..
            }
        ));
        drop(storage);

        *vault
            .fail_create_after_store
            .lock()
            .expect("create flag lock") = false;
        let reopened = Storage::open(directory.path(), vault.clone()).expect("storage reopens");
        assert_eq!(reopened.startup_report().secrets.deleted, 1);
        assert_eq!(reopened.startup_report().secrets.pending, 0);
        assert!(vault.values.lock().expect("vault lock").is_empty());
    }

    #[test]
    fn concurrent_revision_cas_allows_exactly_one_update() {
        let directory = TempDir::new().expect("temp dir");
        let vault = Arc::new(MemoryVault::default());
        let storage = Storage::open(directory.path(), vault).expect("storage opens");
        let created = storage
            .create_datasource(input("original"), None)
            .expect("datasource creates");
        let barrier = Arc::new(Barrier::new(3));

        let results = thread::scope(|scope| {
            let mut handles = Vec::new();
            for name in ["winner-a", "winner-b"] {
                let storage = storage.clone();
                let barrier = barrier.clone();
                let created = created.clone();
                handles.push(scope.spawn(move || {
                    barrier.wait();
                    storage.update_datasource(
                        &created.id,
                        created.revision,
                        UpdateDatasource {
                            name: name.to_owned(),
                            driver_id: created.driver_id,
                        },
                        SecretChange::Keep,
                    )
                }));
            }
            barrier.wait();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("update thread joins"))
                .collect::<Vec<_>>()
        });

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(StorageError::RevisionConflict { .. })))
                .count(),
            1
        );
    }

    #[test]
    fn process_lock_precedes_recovery_and_migration() {
        let directory = TempDir::new().expect("temp dir");
        let first = Storage::open(directory.path(), Arc::new(MemoryVault::default()))
            .expect("first storage opens");
        let second = Storage::open(directory.path(), Arc::new(MemoryVault::default()))
            .expect_err("second storage must fail");
        assert!(matches!(second, StorageError::AlreadyOpen(_)));
        let retained_clone = first.clone();
        drop(first);
        let still_locked = Storage::open(directory.path(), Arc::new(MemoryVault::default()))
            .expect_err("last clone must retain the lock");
        assert!(matches!(still_locked, StorageError::AlreadyOpen(_)));
        drop(retained_clone);
        Storage::open(directory.path(), Arc::new(MemoryVault::default()))
            .expect("lock releases on drop");
    }
}
