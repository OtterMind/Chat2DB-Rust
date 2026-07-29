use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{Storage, StorageError, now_millis};

const MAX_NAME_BYTES: usize = 512;
const MAX_DATASOURCE_ID_BYTES: usize = 512;
const MAX_SCOPE_BYTES: usize = 1_024;
const MAX_DATABASE_TYPE_BYTES: usize = 255;
const MAX_STATE_BYTES: usize = 255;
const MAX_DDL_BYTES: usize = 16 * 1024 * 1024;
const MAX_SEARCH_KEY_BYTES: usize = 256;
const MAX_PAGE_SIZE: u32 = 1_000;

const SAVED_CONSOLE_COLUMNS: &str = "id, name, data_source_id, data_source_name, database_name, schema_name,\
     database_type, ddl, status, tab_opened, operation_type, created_at_ms, updated_at_ms";

/// Fields used to create a durable Community SQL Console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateSavedConsole {
    /// Existing numeric identity to restore, or `None` to allocate one.
    pub id: Option<i64>,
    /// User-visible Console name.
    pub name: String,
    /// Opaque Rust datasource id used by the compatibility layer.
    pub data_source_id: Option<String>,
    /// Datasource alias captured for the retained Community UI.
    pub data_source_name: Option<String>,
    /// Bound database/catalog name.
    pub database_name: Option<String>,
    /// Bound schema name.
    pub schema_name: Option<String>,
    /// Community database type code, exposed as the historical `type` field.
    pub database_type: Option<String>,
    /// SQL editor contents.
    pub ddl: String,
    /// Community saved-state value, normally `DRAFT` or `RELEASE`.
    pub status: String,
    /// Community tab-open flag, either `y` or `n`.
    pub tab_opened: String,
    /// Community workspace operation type, normally `console`.
    pub operation_type: String,
}

/// Partial update for a durable Community SQL Console.
///
/// Nested options distinguish an unchanged nullable field (`None`) from a
/// field that should be cleared (`Some(None)`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateSavedConsole {
    pub name: Option<String>,
    pub data_source_id: Option<Option<String>>,
    pub data_source_name: Option<Option<String>>,
    pub database_name: Option<Option<String>>,
    pub schema_name: Option<Option<String>>,
    pub database_type: Option<Option<String>>,
    pub ddl: Option<String>,
    pub status: Option<String>,
    pub tab_opened: Option<String>,
    pub operation_type: Option<String>,
}

/// Filters and stable paging controls for saved Consoles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedConsoleListQuery {
    pub data_source_id: Option<String>,
    pub database_name: Option<String>,
    pub schema_name: Option<String>,
    pub status: Option<String>,
    pub tab_opened: Option<String>,
    pub operation_type: Option<String>,
    pub search_key: Option<String>,
    pub page_no: u32,
    pub page_size: u32,
    /// Sort by update time descending instead of creation time ascending.
    pub order_by_desc: bool,
}

impl Default for SavedConsoleListQuery {
    fn default() -> Self {
        Self {
            data_source_id: None,
            database_name: None,
            schema_name: None,
            status: None,
            tab_opened: None,
            operation_type: None,
            search_key: None,
            page_no: 1,
            page_size: 20,
            order_by_desc: false,
        }
    }
}

/// One durable saved Console record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedConsoleRecord {
    pub id: i64,
    pub name: String,
    pub data_source_id: Option<String>,
    pub data_source_name: Option<String>,
    pub database_name: Option<String>,
    pub schema_name: Option<String>,
    pub database_type: Option<String>,
    pub ddl: String,
    pub status: String,
    pub tab_opened: String,
    pub operation_type: String,
    /// Creation time as Unix epoch milliseconds.
    pub created_at_ms: i64,
    /// Last update time as Unix epoch milliseconds.
    pub updated_at_ms: i64,
}

/// One stable page of durable saved Consoles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedConsolePage {
    pub records: Vec<SavedConsoleRecord>,
    pub total: u64,
    pub page_no: u32,
    pub page_size: u32,
}

impl Storage {
    /// Creates a saved Console and returns its durable record.
    ///
    /// A positive requested id is preserved for Community's manual-save
    /// recovery flow. Otherwise `SQLite` allocates a non-reused numeric id.
    ///
    /// # Errors
    ///
    /// Returns validation or `SQLite` failures, including an id conflict.
    pub fn create_saved_console(
        &self,
        input: CreateSavedConsole,
    ) -> Result<SavedConsoleRecord, StorageError> {
        validate_create(&input)?;
        let CreateSavedConsole {
            id: requested_id,
            name,
            data_source_id,
            data_source_name,
            database_name,
            schema_name,
            database_type,
            ddl,
            status,
            tab_opened,
            operation_type,
        } = input;
        let timestamp = now_millis()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

        let id = if let Some(id) = requested_id {
            transaction.execute(
                "INSERT INTO saved_consoles (
                    id, name, data_source_id, data_source_name, database_name, schema_name,
                    database_type, ddl, status, tab_opened, operation_type,
                    created_at_ms, updated_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
                params![
                    id,
                    name,
                    data_source_id,
                    data_source_name,
                    database_name,
                    schema_name,
                    database_type,
                    ddl,
                    status,
                    tab_opened,
                    operation_type,
                    timestamp,
                ],
            )?;
            id
        } else {
            transaction.execute(
                "INSERT INTO saved_consoles (
                    name, data_source_id, data_source_name, database_name, schema_name,
                    database_type, ddl, status, tab_opened, operation_type,
                    created_at_ms, updated_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
                params![
                    name,
                    data_source_id,
                    data_source_name,
                    database_name,
                    schema_name,
                    database_type,
                    ddl,
                    status,
                    tab_opened,
                    operation_type,
                    timestamp,
                ],
            )?;
            transaction.last_insert_rowid()
        };

        let record = load_saved_console(&transaction, id)?.ok_or_else(|| {
            StorageError::Integrity("saved Console disappeared before create commit".to_owned())
        })?;
        transaction.commit()?;
        Ok(record)
    }

    /// Loads one saved Console by numeric Community id.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or persisted-data validation failures.
    pub fn get_saved_console(&self, id: i64) -> Result<Option<SavedConsoleRecord>, StorageError> {
        if id <= 0 {
            return Ok(None);
        }
        load_saved_console(&self.connection()?, id)
    }

    /// Lists saved Consoles with Community-compatible filters and stable paging.
    ///
    /// # Errors
    ///
    /// Returns validation, `SQLite`, numeric-range, or persisted-data failures.
    pub fn list_saved_consoles(
        &self,
        query: &SavedConsoleListQuery,
    ) -> Result<SavedConsolePage, StorageError> {
        validate_list_query(query)?;
        let offset = u64::from(query.page_no - 1)
            .checked_mul(u64::from(query.page_size))
            .ok_or(StorageError::NumericRange("saved Console page offset"))?;
        let offset = i64::try_from(offset)
            .map_err(|_| StorageError::NumericRange("saved Console page offset"))?;
        let limit = i64::from(query.page_size);
        let search_pattern = query
            .search_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(like_pattern);
        let connection = self.connection()?;

        let total: i64 = connection.query_row(
            "SELECT COUNT(*) FROM saved_consoles
             WHERE (?1 IS NULL OR data_source_id = ?1)
               AND (?2 IS NULL OR database_name = ?2)
               AND (?3 IS NULL OR schema_name = ?3)
               AND (?4 IS NULL OR status = ?4)
               AND (?5 IS NULL OR tab_opened = ?5)
               AND (?6 IS NULL OR operation_type = ?6)
               AND (
                   ?7 IS NULL
                   OR name COLLATE NOCASE LIKE ?7 ESCAPE '\\'
                   OR COALESCE(data_source_name, '') COLLATE NOCASE LIKE ?7 ESCAPE '\\'
                   OR COALESCE(database_name, '') COLLATE NOCASE LIKE ?7 ESCAPE '\\'
                   OR COALESCE(schema_name, '') COLLATE NOCASE LIKE ?7 ESCAPE '\\'
               )",
            params![
                query.data_source_id.as_deref(),
                query.database_name.as_deref(),
                query.schema_name.as_deref(),
                query.status.as_deref(),
                query.tab_opened.as_deref(),
                query.operation_type.as_deref(),
                search_pattern.as_deref(),
            ],
            |row| row.get(0),
        )?;

        let order = if query.order_by_desc {
            "updated_at_ms DESC, id DESC"
        } else {
            "created_at_ms ASC, id ASC"
        };
        let sql = format!(
            "SELECT {SAVED_CONSOLE_COLUMNS} FROM saved_consoles
             WHERE (?1 IS NULL OR data_source_id = ?1)
               AND (?2 IS NULL OR database_name = ?2)
               AND (?3 IS NULL OR schema_name = ?3)
               AND (?4 IS NULL OR status = ?4)
               AND (?5 IS NULL OR tab_opened = ?5)
               AND (?6 IS NULL OR operation_type = ?6)
               AND (
                   ?7 IS NULL
                   OR name COLLATE NOCASE LIKE ?7 ESCAPE '\\'
                   OR COALESCE(data_source_name, '') COLLATE NOCASE LIKE ?7 ESCAPE '\\'
                   OR COALESCE(database_name, '') COLLATE NOCASE LIKE ?7 ESCAPE '\\'
                   OR COALESCE(schema_name, '') COLLATE NOCASE LIKE ?7 ESCAPE '\\'
               )
             ORDER BY {order}
             LIMIT ?8 OFFSET ?9"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(
            params![
                query.data_source_id.as_deref(),
                query.database_name.as_deref(),
                query.schema_name.as_deref(),
                query.status.as_deref(),
                query.tab_opened.as_deref(),
                query.operation_type.as_deref(),
                search_pattern.as_deref(),
                limit,
                offset,
            ],
            raw_saved_console,
        )?;
        let mut records = Vec::new();
        for row in rows {
            records.push(decode_saved_console(row?)?);
        }

        Ok(SavedConsolePage {
            records,
            total: u64::try_from(total)
                .map_err(|_| StorageError::NumericRange("saved Console total"))?,
            page_no: query.page_no,
            page_size: query.page_size,
        })
    }

    /// Applies a partial update and returns the complete durable record.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::SavedConsoleNotFound`] when the id is absent,
    /// or validation and `SQLite` failures otherwise.
    pub fn update_saved_console(
        &self,
        id: i64,
        input: UpdateSavedConsole,
    ) -> Result<SavedConsoleRecord, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current =
            load_saved_console(&transaction, id)?.ok_or(StorageError::SavedConsoleNotFound(id))?;

        let timestamp = now_millis()?.max(current.updated_at_ms);
        let next = SavedConsoleRecord {
            id,
            name: input.name.unwrap_or(current.name),
            data_source_id: input.data_source_id.unwrap_or(current.data_source_id),
            data_source_name: input.data_source_name.unwrap_or(current.data_source_name),
            database_name: input.database_name.unwrap_or(current.database_name),
            schema_name: input.schema_name.unwrap_or(current.schema_name),
            database_type: input.database_type.unwrap_or(current.database_type),
            ddl: input.ddl.unwrap_or(current.ddl),
            status: input.status.unwrap_or(current.status),
            tab_opened: input.tab_opened.unwrap_or(current.tab_opened),
            operation_type: input.operation_type.unwrap_or(current.operation_type),
            created_at_ms: current.created_at_ms,
            updated_at_ms: timestamp,
        };
        validate_record(&next)?;

        let changed = transaction.execute(
            "UPDATE saved_consoles
             SET name = ?1, data_source_id = ?2, data_source_name = ?3,
                 database_name = ?4, schema_name = ?5, database_type = ?6,
                 ddl = ?7, status = ?8, tab_opened = ?9, operation_type = ?10,
                 updated_at_ms = ?11
             WHERE id = ?12",
            params![
                next.name,
                next.data_source_id,
                next.data_source_name,
                next.database_name,
                next.schema_name,
                next.database_type,
                next.ddl,
                next.status,
                next.tab_opened,
                next.operation_type,
                next.updated_at_ms,
                id,
            ],
        )?;
        if changed != 1 {
            return Err(StorageError::SavedConsoleNotFound(id));
        }
        transaction.commit()?;
        Ok(next)
    }

    /// Deletes a saved Console, returning whether a record existed.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` failures.
    pub fn delete_saved_console(&self, id: i64) -> Result<bool, StorageError> {
        if id <= 0 {
            return Ok(false);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let deleted = transaction.execute("DELETE FROM saved_consoles WHERE id = ?1", [id])?;
        transaction.commit()?;
        Ok(deleted == 1)
    }
}

fn validate_create(input: &CreateSavedConsole) -> Result<(), StorageError> {
    if input.id.is_some_and(|id| id <= 0) {
        return Err(StorageError::InvalidSavedConsole(
            "id must be a positive signed 64-bit integer",
        ));
    }
    validate_fields(
        &input.name,
        input.data_source_id.as_deref(),
        input.data_source_name.as_deref(),
        input.database_name.as_deref(),
        input.schema_name.as_deref(),
        input.database_type.as_deref(),
        &input.ddl,
        &input.status,
        &input.tab_opened,
        &input.operation_type,
    )
}

fn validate_record(record: &SavedConsoleRecord) -> Result<(), StorageError> {
    if record.id <= 0 {
        return Err(StorageError::InvalidSavedConsole(
            "persisted id must be a positive signed 64-bit integer",
        ));
    }
    if record.created_at_ms < 0 || record.updated_at_ms < record.created_at_ms {
        return Err(StorageError::InvalidSavedConsole(
            "persisted timestamps are invalid",
        ));
    }
    validate_fields(
        &record.name,
        record.data_source_id.as_deref(),
        record.data_source_name.as_deref(),
        record.database_name.as_deref(),
        record.schema_name.as_deref(),
        record.database_type.as_deref(),
        &record.ddl,
        &record.status,
        &record.tab_opened,
        &record.operation_type,
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_fields(
    name: &str,
    data_source_id: Option<&str>,
    data_source_name: Option<&str>,
    database_name: Option<&str>,
    schema_name: Option<&str>,
    database_type: Option<&str>,
    ddl: &str,
    status: &str,
    tab_opened: &str,
    operation_type: &str,
) -> Result<(), StorageError> {
    if name.len() > MAX_NAME_BYTES {
        return Err(StorageError::InvalidSavedConsole(
            "name must be at most 512 UTF-8 bytes",
        ));
    }
    if data_source_id.is_some_and(|value| value.len() > MAX_DATASOURCE_ID_BYTES) {
        return Err(StorageError::InvalidSavedConsole(
            "datasource id must be at most 512 UTF-8 bytes",
        ));
    }
    for value in [data_source_name, database_name, schema_name]
        .into_iter()
        .flatten()
    {
        if value.len() > MAX_SCOPE_BYTES {
            return Err(StorageError::InvalidSavedConsole(
                "datasource, database, and schema names must be at most 1024 UTF-8 bytes",
            ));
        }
    }
    if database_type.is_some_and(|value| value.len() > MAX_DATABASE_TYPE_BYTES) {
        return Err(StorageError::InvalidSavedConsole(
            "database type must be at most 255 UTF-8 bytes",
        ));
    }
    if ddl.len() > MAX_DDL_BYTES {
        return Err(StorageError::InvalidSavedConsole(
            "SQL text must be at most 16 MiB",
        ));
    }
    if status.is_empty() || status.len() > MAX_STATE_BYTES {
        return Err(StorageError::InvalidSavedConsole(
            "status must be non-empty and at most 255 UTF-8 bytes",
        ));
    }
    if !matches!(tab_opened, "y" | "n") {
        return Err(StorageError::InvalidSavedConsole(
            "tab-opened must be either y or n",
        ));
    }
    if operation_type.is_empty() || operation_type.len() > MAX_STATE_BYTES {
        return Err(StorageError::InvalidSavedConsole(
            "operation type must be non-empty and at most 255 UTF-8 bytes",
        ));
    }
    Ok(())
}

fn validate_list_query(query: &SavedConsoleListQuery) -> Result<(), StorageError> {
    if query.page_no == 0 {
        return Err(StorageError::InvalidSavedConsole(
            "page number must be greater than zero",
        ));
    }
    if query.page_size == 0 || query.page_size > MAX_PAGE_SIZE {
        return Err(StorageError::InvalidSavedConsole(
            "page size must be between 1 and 1000",
        ));
    }
    if query
        .search_key
        .as_ref()
        .is_some_and(|value| value.len() > MAX_SEARCH_KEY_BYTES)
    {
        return Err(StorageError::InvalidSavedConsole(
            "search key must be at most 256 UTF-8 bytes",
        ));
    }
    Ok(())
}

fn like_pattern(value: &str) -> String {
    let mut pattern = String::with_capacity(value.len() + 2);
    pattern.push('%');
    for character in value.chars() {
        if matches!(character, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push('%');
    pattern
}

struct RawSavedConsole {
    id: i64,
    name: String,
    data_source_id: Option<String>,
    data_source_name: Option<String>,
    database_name: Option<String>,
    schema_name: Option<String>,
    database_type: Option<String>,
    ddl: String,
    status: String,
    tab_opened: String,
    operation_type: String,
    created_at_ms: i64,
    updated_at_ms: i64,
}

fn raw_saved_console(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawSavedConsole> {
    Ok(RawSavedConsole {
        id: row.get(0)?,
        name: row.get(1)?,
        data_source_id: row.get(2)?,
        data_source_name: row.get(3)?,
        database_name: row.get(4)?,
        schema_name: row.get(5)?,
        database_type: row.get(6)?,
        ddl: row.get(7)?,
        status: row.get(8)?,
        tab_opened: row.get(9)?,
        operation_type: row.get(10)?,
        created_at_ms: row.get(11)?,
        updated_at_ms: row.get(12)?,
    })
}

fn decode_saved_console(raw: RawSavedConsole) -> Result<SavedConsoleRecord, StorageError> {
    let record = SavedConsoleRecord {
        id: raw.id,
        name: raw.name,
        data_source_id: raw.data_source_id,
        data_source_name: raw.data_source_name,
        database_name: raw.database_name,
        schema_name: raw.schema_name,
        database_type: raw.database_type,
        ddl: raw.ddl,
        status: raw.status,
        tab_opened: raw.tab_opened,
        operation_type: raw.operation_type,
        created_at_ms: raw.created_at_ms,
        updated_at_ms: raw.updated_at_ms,
    };
    validate_record(&record)?;
    Ok(record)
}

fn load_saved_console(
    connection: &Connection,
    id: i64,
) -> Result<Option<SavedConsoleRecord>, StorageError> {
    let sql = format!("SELECT {SAVED_CONSOLE_COLUMNS} FROM saved_consoles WHERE id = ?1");
    connection
        .query_row(&sql, [id], raw_saved_console)
        .optional()?
        .map(decode_saved_console)
        .transpose()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::TempDir;

    use super::{CreateSavedConsole, SavedConsoleListQuery, UpdateSavedConsole};
    use crate::{SecretRef, SecretValue, SecretVault, SecretVaultError, Storage, StorageError};

    #[derive(Debug)]
    struct EmptyVault;

    impl SecretVault for EmptyVault {
        fn probe(&self) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn create(
            &self,
            _reference: &SecretRef,
            _value: &SecretValue,
        ) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn get(&self, _reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
            Ok(None)
        }

        fn delete(&self, _reference: &SecretRef) -> Result<(), SecretVaultError> {
            Ok(())
        }
    }

    fn open(directory: &TempDir) -> Storage {
        Storage::open(directory.path(), Arc::new(EmptyVault)).expect("storage opens")
    }

    fn input(name: &str, datasource_id: &str) -> CreateSavedConsole {
        CreateSavedConsole {
            id: None,
            name: name.to_owned(),
            data_source_id: Some(datasource_id.to_owned()),
            data_source_name: Some("Local MySQL".to_owned()),
            database_name: Some("chat2db".to_owned()),
            schema_name: Some("public".to_owned()),
            database_type: Some("MYSQL".to_owned()),
            ddl: "SELECT 1".to_owned(),
            status: "DRAFT".to_owned(),
            tab_opened: "y".to_owned(),
            operation_type: "console".to_owned(),
        }
    }

    #[test]
    fn saved_console_crud_survives_reopen() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let created = storage
            .create_saved_console(input("orders", "datasource-1"))
            .expect("Console creates");
        assert!(created.id > 0);
        assert_eq!(
            storage
                .get_saved_console(created.id)
                .expect("Console reads"),
            Some(created.clone())
        );

        let updated = storage
            .update_saved_console(
                created.id,
                UpdateSavedConsole {
                    name: Some("orders-release".to_owned()),
                    schema_name: Some(None),
                    ddl: Some("SELECT * FROM orders".to_owned()),
                    status: Some("RELEASE".to_owned()),
                    tab_opened: Some("n".to_owned()),
                    ..UpdateSavedConsole::default()
                },
            )
            .expect("Console updates");
        assert_eq!(updated.name, "orders-release");
        assert_eq!(updated.schema_name, None);
        assert_eq!(updated.status, "RELEASE");
        assert!(updated.updated_at_ms >= created.updated_at_ms);

        drop(storage);
        let reopened = open(&directory);
        assert_eq!(
            reopened
                .get_saved_console(created.id)
                .expect("reopened Console reads"),
            Some(updated)
        );
        assert!(
            reopened
                .delete_saved_console(created.id)
                .expect("Console deletes")
        );
        assert!(
            !reopened
                .delete_saved_console(created.id)
                .expect("missing Console delete is idempotent")
        );
        assert_eq!(
            reopened
                .get_saved_console(created.id)
                .expect("deleted Console reads"),
            None
        );
    }

    #[test]
    fn requested_numeric_identity_can_be_restored_without_reuse() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let mut requested = input("restored", "datasource-1");
        requested.id = Some(42);
        let first = storage
            .create_saved_console(requested.clone())
            .expect("requested id creates");
        assert_eq!(first.id, 42);
        assert!(storage.delete_saved_console(42).expect("record deletes"));

        let restored = storage
            .create_saved_console(requested)
            .expect("same deleted id restores");
        assert_eq!(restored.id, 42);
        let allocated = storage
            .create_saved_console(input("next", "datasource-1"))
            .expect("next id allocates");
        assert!(allocated.id > 42);
    }

    #[test]
    fn list_filters_pages_searches_and_sorts_stably() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let alpha = storage
            .create_saved_console(input("Alpha report", "datasource-a"))
            .expect("alpha creates");
        let mut beta_input = input("Beta report", "datasource-a");
        beta_input.database_name = Some("analytics".to_owned());
        beta_input.status = "RELEASE".to_owned();
        beta_input.tab_opened = "n".to_owned();
        let beta = storage
            .create_saved_console(beta_input)
            .expect("beta creates");
        let gamma = storage
            .create_saved_console(input("Gamma", "datasource-b"))
            .expect("gamma creates");

        let first_page = storage
            .list_saved_consoles(&SavedConsoleListQuery {
                page_size: 2,
                ..SavedConsoleListQuery::default()
            })
            .expect("first page lists");
        assert_eq!(first_page.total, 3);
        assert_eq!(
            first_page
                .records
                .iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec![alpha.id, beta.id]
        );

        let descending = storage
            .list_saved_consoles(&SavedConsoleListQuery {
                page_size: 3,
                order_by_desc: true,
                ..SavedConsoleListQuery::default()
            })
            .expect("descending page lists");
        assert_eq!(
            descending
                .records
                .iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec![gamma.id, beta.id, alpha.id]
        );

        let filtered = storage
            .list_saved_consoles(&SavedConsoleListQuery {
                data_source_id: Some("datasource-a".to_owned()),
                database_name: Some("analytics".to_owned()),
                status: Some("RELEASE".to_owned()),
                tab_opened: Some("n".to_owned()),
                operation_type: Some("console".to_owned()),
                search_key: Some("beta".to_owned()),
                ..SavedConsoleListQuery::default()
            })
            .expect("filtered page lists");
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.records[0].id, beta.id);
    }

    #[test]
    fn invalid_updates_and_pages_fail_closed() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let record = storage
            .create_saved_console(input("query", "datasource-1"))
            .expect("Console creates");

        let invalid_update = storage
            .update_saved_console(
                record.id,
                UpdateSavedConsole {
                    tab_opened: Some("maybe".to_owned()),
                    ..UpdateSavedConsole::default()
                },
            )
            .expect_err("invalid tab flag must fail");
        assert!(matches!(
            invalid_update,
            StorageError::InvalidSavedConsole(_)
        ));
        assert_eq!(
            storage
                .get_saved_console(record.id)
                .expect("record remains readable")
                .expect("record remains present")
                .tab_opened,
            "y"
        );

        let invalid_page = storage
            .list_saved_consoles(&SavedConsoleListQuery {
                page_no: 0,
                ..SavedConsoleListQuery::default()
            })
            .expect_err("zero page must fail");
        assert!(matches!(invalid_page, StorageError::InvalidSavedConsole(_)));
        assert!(matches!(
            storage
                .update_saved_console(9_999, UpdateSavedConsole::default())
                .expect_err("missing update must fail"),
            StorageError::SavedConsoleNotFound(9_999)
        ));
    }
}
