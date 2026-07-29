use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{Storage, StorageError, now_millis};

const MAX_NAME_BYTES: usize = 512;
const MAX_DATASOURCE_ID_BYTES: usize = 512;
const MAX_SCOPE_BYTES: usize = 1_024;
const MAX_DATABASE_TYPE_BYTES: usize = 255;
const MAX_STATE_BYTES: usize = 255;
const MAX_DDL_BYTES: usize = 16 * 1024 * 1024;
const MAX_EXTEND_INFO_BYTES: usize = 16 * 1024 * 1024;
const MAX_SEARCH_KEY_BYTES: usize = 256;
const MAX_PAGE_SIZE: u32 = 1_000;

const OPERATION_LOG_COLUMNS: &str = "id, name, data_source_id, data_source_name, connectable,\
     database_name, database_type, ddl, status, operation_rows, use_time, extend_info,\
     schema_name, organization_id, user_name, more, operation_type, created_at_ms, updated_at_ms";

/// Fields captured for one durable Community SQL execution-history entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateOperationLog {
    pub name: Option<String>,
    /// Opaque Rust datasource id corresponding to Community's `dataSourceId`.
    pub data_source_id: Option<String>,
    pub data_source_name: Option<String>,
    pub connectable: Option<bool>,
    pub database_name: Option<String>,
    /// Community database type code, exposed as the historical `type` field.
    pub database_type: Option<String>,
    pub ddl: String,
    pub status: String,
    pub operation_rows: Option<i64>,
    /// Execution duration in milliseconds, exposed as the historical `useTime` field.
    pub use_time: Option<i64>,
    /// Opaque JSON-like metadata retained without interpretation.
    pub extend_info: Option<String>,
    pub schema_name: Option<String>,
    pub organization_id: Option<i64>,
    pub user_name: Option<String>,
    /// Whether the list representation's SQL text is truncated.
    pub more: bool,
    /// Community history discriminator, normally `SQL_EXECUTE` or `SQL_AUDIT`.
    pub operation_type: String,
}

/// Community-compatible filters and stable paging controls for execution history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationLogListQuery {
    pub data_source_id: Option<String>,
    pub database_name: Option<String>,
    pub schema_name: Option<String>,
    pub operation_type: Option<String>,
    pub search_key: Option<String>,
    pub page_no: u32,
    pub page_size: u32,
}

impl Default for OperationLogListQuery {
    fn default() -> Self {
        Self {
            data_source_id: None,
            database_name: None,
            schema_name: None,
            operation_type: None,
            search_key: None,
            page_no: 1,
            page_size: 20,
        }
    }
}

/// One durable Community SQL execution-history entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationLogRecord {
    pub id: i64,
    pub name: Option<String>,
    pub data_source_id: Option<String>,
    pub data_source_name: Option<String>,
    pub connectable: Option<bool>,
    pub database_name: Option<String>,
    pub database_type: Option<String>,
    pub ddl: String,
    pub status: String,
    pub operation_rows: Option<i64>,
    pub use_time: Option<i64>,
    pub extend_info: Option<String>,
    pub schema_name: Option<String>,
    pub organization_id: Option<i64>,
    pub user_name: Option<String>,
    pub more: bool,
    pub operation_type: String,
    /// Creation time as Unix epoch milliseconds.
    pub created_at_ms: i64,
    /// Last update time as Unix epoch milliseconds.
    pub updated_at_ms: i64,
}

/// One newest-first stable page of durable execution history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationLogPage {
    pub records: Vec<OperationLogRecord>,
    pub total: u64,
    pub page_no: u32,
    pub page_size: u32,
}

impl Storage {
    /// Creates one execution-history entry and returns its durable record.
    ///
    /// # Errors
    ///
    /// Returns validation or `SQLite` failures.
    pub fn create_operation_log(
        &self,
        input: CreateOperationLog,
    ) -> Result<OperationLogRecord, StorageError> {
        validate_create(&input)?;
        let CreateOperationLog {
            name,
            data_source_id,
            data_source_name,
            connectable,
            database_name,
            database_type,
            ddl,
            status,
            operation_rows,
            use_time,
            extend_info,
            schema_name,
            organization_id,
            user_name,
            more,
            operation_type,
        } = input;
        let timestamp = now_millis()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO operation_logs (
                name, data_source_id, data_source_name, connectable, database_name,
                database_type, ddl, status, operation_rows, use_time, extend_info,
                schema_name, organization_id, user_name, more, operation_type,
                created_at_ms, updated_at_ms
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                ?15, ?16, ?17, ?17
             )",
            params![
                name,
                data_source_id,
                data_source_name,
                connectable,
                database_name,
                database_type,
                ddl,
                status,
                operation_rows,
                use_time,
                extend_info,
                schema_name,
                organization_id,
                user_name,
                more,
                operation_type,
                timestamp,
            ],
        )?;
        let id = transaction.last_insert_rowid();
        let record = load_operation_log(&transaction, id)?.ok_or_else(|| {
            StorageError::Integrity("operation log disappeared before create commit".to_owned())
        })?;
        transaction.commit()?;
        Ok(record)
    }

    /// Loads one execution-history entry by its numeric Community id.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` or persisted-data validation failures.
    pub fn get_operation_log(&self, id: i64) -> Result<Option<OperationLogRecord>, StorageError> {
        if id <= 0 {
            return Ok(None);
        }
        load_operation_log(&self.connection()?, id)
    }

    /// Lists execution history newest-first with exact scope filters and stable paging.
    ///
    /// `search_key` is a case-insensitive literal substring match over SQL and
    /// the user-visible names and state stored with the entry.
    ///
    /// # Errors
    ///
    /// Returns validation, `SQLite`, numeric-range, or persisted-data failures.
    pub fn list_operation_logs(
        &self,
        query: &OperationLogListQuery,
    ) -> Result<OperationLogPage, StorageError> {
        validate_list_query(query)?;
        let offset = u64::from(query.page_no - 1)
            .checked_mul(u64::from(query.page_size))
            .ok_or(StorageError::NumericRange("operation log page offset"))?;
        let offset = i64::try_from(offset)
            .map_err(|_| StorageError::NumericRange("operation log page offset"))?;
        let limit = i64::from(query.page_size);
        let search_pattern = query
            .search_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(like_pattern);
        let connection = self.connection()?;

        let total: i64 = connection.query_row(
            "SELECT COUNT(*) FROM operation_logs
             WHERE (?1 IS NULL OR data_source_id = ?1)
               AND (?2 IS NULL OR database_name = ?2)
               AND (?3 IS NULL OR schema_name = ?3)
               AND (?4 IS NULL OR operation_type = ?4)
               AND (
                   ?5 IS NULL
                   OR COALESCE(name, '') COLLATE NOCASE LIKE ?5 ESCAPE '\\'
                   OR COALESCE(data_source_name, '') COLLATE NOCASE LIKE ?5 ESCAPE '\\'
                   OR COALESCE(database_name, '') COLLATE NOCASE LIKE ?5 ESCAPE '\\'
                   OR COALESCE(schema_name, '') COLLATE NOCASE LIKE ?5 ESCAPE '\\'
                   OR ddl COLLATE NOCASE LIKE ?5 ESCAPE '\\'
                   OR status COLLATE NOCASE LIKE ?5 ESCAPE '\\'
                   OR COALESCE(database_type, '') COLLATE NOCASE LIKE ?5 ESCAPE '\\'
                   OR COALESCE(user_name, '') COLLATE NOCASE LIKE ?5 ESCAPE '\\'
               )",
            params![
                query.data_source_id.as_deref(),
                query.database_name.as_deref(),
                query.schema_name.as_deref(),
                query.operation_type.as_deref(),
                search_pattern.as_deref(),
            ],
            |row| row.get(0),
        )?;

        let sql = format!(
            "SELECT {OPERATION_LOG_COLUMNS} FROM operation_logs
             WHERE (?1 IS NULL OR data_source_id = ?1)
               AND (?2 IS NULL OR database_name = ?2)
               AND (?3 IS NULL OR schema_name = ?3)
               AND (?4 IS NULL OR operation_type = ?4)
               AND (
                   ?5 IS NULL
                   OR COALESCE(name, '') COLLATE NOCASE LIKE ?5 ESCAPE '\\'
                   OR COALESCE(data_source_name, '') COLLATE NOCASE LIKE ?5 ESCAPE '\\'
                   OR COALESCE(database_name, '') COLLATE NOCASE LIKE ?5 ESCAPE '\\'
                   OR COALESCE(schema_name, '') COLLATE NOCASE LIKE ?5 ESCAPE '\\'
                   OR ddl COLLATE NOCASE LIKE ?5 ESCAPE '\\'
                   OR status COLLATE NOCASE LIKE ?5 ESCAPE '\\'
                   OR COALESCE(database_type, '') COLLATE NOCASE LIKE ?5 ESCAPE '\\'
                   OR COALESCE(user_name, '') COLLATE NOCASE LIKE ?5 ESCAPE '\\'
               )
             ORDER BY created_at_ms DESC, id DESC
             LIMIT ?6 OFFSET ?7"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(
            params![
                query.data_source_id.as_deref(),
                query.database_name.as_deref(),
                query.schema_name.as_deref(),
                query.operation_type.as_deref(),
                search_pattern.as_deref(),
                limit,
                offset,
            ],
            raw_operation_log,
        )?;
        let mut records = Vec::new();
        for row in rows {
            records.push(decode_operation_log(row?)?);
        }

        Ok(OperationLogPage {
            records,
            total: u64::try_from(total)
                .map_err(|_| StorageError::NumericRange("operation log total"))?,
            page_no: query.page_no,
            page_size: query.page_size,
        })
    }
}

fn validate_create(input: &CreateOperationLog) -> Result<(), StorageError> {
    validate_fields(
        input.name.as_deref(),
        input.data_source_id.as_deref(),
        input.data_source_name.as_deref(),
        input.database_name.as_deref(),
        input.schema_name.as_deref(),
        input.database_type.as_deref(),
        input.user_name.as_deref(),
        &input.ddl,
        &input.status,
        input.operation_rows,
        input.use_time,
        input.extend_info.as_deref(),
        &input.operation_type,
    )
}

fn validate_record(record: &OperationLogRecord) -> Result<(), StorageError> {
    if record.id <= 0 {
        return Err(StorageError::InvalidOperationLog(
            "persisted id must be a positive signed 64-bit integer",
        ));
    }
    if record.created_at_ms < 0 || record.updated_at_ms < record.created_at_ms {
        return Err(StorageError::InvalidOperationLog(
            "persisted timestamps are invalid",
        ));
    }
    validate_fields(
        record.name.as_deref(),
        record.data_source_id.as_deref(),
        record.data_source_name.as_deref(),
        record.database_name.as_deref(),
        record.schema_name.as_deref(),
        record.database_type.as_deref(),
        record.user_name.as_deref(),
        &record.ddl,
        &record.status,
        record.operation_rows,
        record.use_time,
        record.extend_info.as_deref(),
        &record.operation_type,
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_fields(
    name: Option<&str>,
    data_source_id: Option<&str>,
    data_source_name: Option<&str>,
    database_name: Option<&str>,
    schema_name: Option<&str>,
    database_type: Option<&str>,
    user_name: Option<&str>,
    ddl: &str,
    status: &str,
    operation_rows: Option<i64>,
    use_time: Option<i64>,
    extend_info: Option<&str>,
    operation_type: &str,
) -> Result<(), StorageError> {
    if name.is_some_and(|value| value.len() > MAX_NAME_BYTES) {
        return Err(StorageError::InvalidOperationLog(
            "name must be at most 512 UTF-8 bytes",
        ));
    }
    if data_source_id.is_some_and(|value| value.len() > MAX_DATASOURCE_ID_BYTES) {
        return Err(StorageError::InvalidOperationLog(
            "datasource id must be at most 512 UTF-8 bytes",
        ));
    }
    for value in [data_source_name, database_name, schema_name, user_name]
        .into_iter()
        .flatten()
    {
        if value.len() > MAX_SCOPE_BYTES {
            return Err(StorageError::InvalidOperationLog(
                "datasource, database, schema, and user names must be at most 1024 UTF-8 bytes",
            ));
        }
    }
    if database_type.is_some_and(|value| value.len() > MAX_DATABASE_TYPE_BYTES) {
        return Err(StorageError::InvalidOperationLog(
            "database type must be at most 255 UTF-8 bytes",
        ));
    }
    if ddl.trim().is_empty() || ddl.len() > MAX_DDL_BYTES {
        return Err(StorageError::InvalidOperationLog(
            "SQL text must be non-empty and at most 16 MiB",
        ));
    }
    if status.is_empty() || status.len() > MAX_STATE_BYTES {
        return Err(StorageError::InvalidOperationLog(
            "status must be non-empty and at most 255 UTF-8 bytes",
        ));
    }
    if operation_rows.is_some_and(|value| value < 0) {
        return Err(StorageError::InvalidOperationLog(
            "operation rows must be non-negative",
        ));
    }
    if use_time.is_some_and(|value| value < 0) {
        return Err(StorageError::InvalidOperationLog(
            "use time must be non-negative",
        ));
    }
    if extend_info.is_some_and(|value| value.len() > MAX_EXTEND_INFO_BYTES) {
        return Err(StorageError::InvalidOperationLog(
            "extended information must be at most 16 MiB",
        ));
    }
    if operation_type.is_empty() || operation_type.len() > MAX_STATE_BYTES {
        return Err(StorageError::InvalidOperationLog(
            "operation type must be non-empty and at most 255 UTF-8 bytes",
        ));
    }
    Ok(())
}

fn validate_list_query(query: &OperationLogListQuery) -> Result<(), StorageError> {
    if query.page_no == 0 {
        return Err(StorageError::InvalidOperationLog(
            "page number must be greater than zero",
        ));
    }
    if query.page_size == 0 || query.page_size > MAX_PAGE_SIZE {
        return Err(StorageError::InvalidOperationLog(
            "page size must be between 1 and 1000",
        ));
    }
    if query
        .search_key
        .as_ref()
        .is_some_and(|value| value.len() > MAX_SEARCH_KEY_BYTES)
    {
        return Err(StorageError::InvalidOperationLog(
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

struct RawOperationLog {
    id: i64,
    name: Option<String>,
    data_source_id: Option<String>,
    data_source_name: Option<String>,
    connectable: Option<bool>,
    database_name: Option<String>,
    database_type: Option<String>,
    ddl: String,
    status: String,
    operation_rows: Option<i64>,
    use_time: Option<i64>,
    extend_info: Option<String>,
    schema_name: Option<String>,
    organization_id: Option<i64>,
    user_name: Option<String>,
    more: bool,
    operation_type: String,
    created_at_ms: i64,
    updated_at_ms: i64,
}

fn raw_operation_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawOperationLog> {
    Ok(RawOperationLog {
        id: row.get(0)?,
        name: row.get(1)?,
        data_source_id: row.get(2)?,
        data_source_name: row.get(3)?,
        connectable: row.get(4)?,
        database_name: row.get(5)?,
        database_type: row.get(6)?,
        ddl: row.get(7)?,
        status: row.get(8)?,
        operation_rows: row.get(9)?,
        use_time: row.get(10)?,
        extend_info: row.get(11)?,
        schema_name: row.get(12)?,
        organization_id: row.get(13)?,
        user_name: row.get(14)?,
        more: row.get(15)?,
        operation_type: row.get(16)?,
        created_at_ms: row.get(17)?,
        updated_at_ms: row.get(18)?,
    })
}

fn decode_operation_log(raw: RawOperationLog) -> Result<OperationLogRecord, StorageError> {
    let record = OperationLogRecord {
        id: raw.id,
        name: raw.name,
        data_source_id: raw.data_source_id,
        data_source_name: raw.data_source_name,
        connectable: raw.connectable,
        database_name: raw.database_name,
        database_type: raw.database_type,
        ddl: raw.ddl,
        status: raw.status,
        operation_rows: raw.operation_rows,
        use_time: raw.use_time,
        extend_info: raw.extend_info,
        schema_name: raw.schema_name,
        organization_id: raw.organization_id,
        user_name: raw.user_name,
        more: raw.more,
        operation_type: raw.operation_type,
        created_at_ms: raw.created_at_ms,
        updated_at_ms: raw.updated_at_ms,
    };
    validate_record(&record)?;
    Ok(record)
}

fn load_operation_log(
    connection: &Connection,
    id: i64,
) -> Result<Option<OperationLogRecord>, StorageError> {
    let sql = format!("SELECT {OPERATION_LOG_COLUMNS} FROM operation_logs WHERE id = ?1");
    connection
        .query_row(&sql, [id], raw_operation_log)
        .optional()?
        .map(decode_operation_log)
        .transpose()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::TempDir;

    use super::{CreateOperationLog, OperationLogListQuery};
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

    fn input(name: &str, datasource_id: &str, ddl: &str) -> CreateOperationLog {
        CreateOperationLog {
            name: Some(name.to_owned()),
            data_source_id: Some(datasource_id.to_owned()),
            data_source_name: Some("Local MySQL".to_owned()),
            connectable: Some(true),
            database_name: Some("chat2db".to_owned()),
            database_type: Some("MYSQL".to_owned()),
            ddl: ddl.to_owned(),
            status: "SUCCESS".to_owned(),
            operation_rows: Some(1),
            use_time: Some(12),
            extend_info: Some(r#"{"source":"console","result":null}"#.to_owned()),
            schema_name: Some("public".to_owned()),
            organization_id: None,
            user_name: Some("local-user".to_owned()),
            more: false,
            operation_type: "SQL_EXECUTE".to_owned(),
        }
    }

    #[test]
    fn operation_log_round_trip_survives_reopen() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let mut create = input("orders", "datasource-1", "SELECT * FROM orders");
        create.connectable = None;
        create.operation_rows = None;
        create.organization_id = Some(7);
        create.more = true;
        let created = storage
            .create_operation_log(create)
            .expect("operation log creates");
        assert!(created.id > 0);
        assert_eq!(created.connectable, None);
        assert_eq!(created.operation_rows, None);
        assert_eq!(created.organization_id, Some(7));
        assert!(created.more);
        assert_eq!(created.created_at_ms, created.updated_at_ms);
        assert_eq!(
            storage
                .get_operation_log(created.id)
                .expect("operation log reads"),
            Some(created.clone())
        );

        drop(storage);
        let reopened = open(&directory);
        assert_eq!(
            reopened
                .get_operation_log(created.id)
                .expect("reopened operation log reads"),
            Some(created)
        );
        assert_eq!(
            reopened
                .get_operation_log(0)
                .expect("invalid identity is absent"),
            None
        );
    }

    #[test]
    fn list_filters_searches_and_pages_in_stable_newest_first_order() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let alpha = storage
            .create_operation_log(input("Alpha", "datasource-a", "SELECT alpha"))
            .expect("alpha creates");

        let mut beta_input = input("Beta", "datasource-a", "SELECT beta");
        beta_input.database_name = Some("analytics".to_owned());
        beta_input.schema_name = Some("reporting".to_owned());
        beta_input.user_name = Some("Alice".to_owned());
        let beta = storage
            .create_operation_log(beta_input)
            .expect("beta creates");

        let mut gamma_input = input("Gamma", "datasource-b", "DELETE FROM audit_events");
        gamma_input.operation_type = "SQL_AUDIT".to_owned();
        let gamma = storage
            .create_operation_log(gamma_input)
            .expect("gamma creates");

        let delta = storage
            .create_operation_log(input("literal %_ marker", "datasource-a", "SELECT delta"))
            .expect("delta creates");

        let first_page = storage
            .list_operation_logs(&OperationLogListQuery {
                page_size: 2,
                ..OperationLogListQuery::default()
            })
            .expect("first page lists");
        assert_eq!(first_page.total, 4);
        assert_eq!(
            first_page
                .records
                .iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec![delta.id, gamma.id]
        );

        let second_page = storage
            .list_operation_logs(&OperationLogListQuery {
                page_no: 2,
                page_size: 2,
                ..OperationLogListQuery::default()
            })
            .expect("second page lists");
        assert_eq!(
            second_page
                .records
                .iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec![beta.id, alpha.id]
        );

        let filtered = storage
            .list_operation_logs(&OperationLogListQuery {
                data_source_id: Some("datasource-a".to_owned()),
                database_name: Some("analytics".to_owned()),
                schema_name: Some("reporting".to_owned()),
                operation_type: Some("SQL_EXECUTE".to_owned()),
                search_key: Some("ALICE".to_owned()),
                ..OperationLogListQuery::default()
            })
            .expect("combined filters list");
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.records[0].id, beta.id);

        let by_datasource = storage
            .list_operation_logs(&OperationLogListQuery {
                data_source_id: Some("datasource-b".to_owned()),
                ..OperationLogListQuery::default()
            })
            .expect("datasource filters");
        assert_eq!(by_datasource.records, vec![gamma.clone()]);

        let by_database = storage
            .list_operation_logs(&OperationLogListQuery {
                database_name: Some("analytics".to_owned()),
                ..OperationLogListQuery::default()
            })
            .expect("database filters");
        assert_eq!(by_database.records, vec![beta.clone()]);

        let by_schema = storage
            .list_operation_logs(&OperationLogListQuery {
                schema_name: Some("reporting".to_owned()),
                ..OperationLogListQuery::default()
            })
            .expect("schema filters");
        assert_eq!(by_schema.records, vec![beta]);

        let audit = storage
            .list_operation_logs(&OperationLogListQuery {
                operation_type: Some("SQL_AUDIT".to_owned()),
                ..OperationLogListQuery::default()
            })
            .expect("operation type filters");
        assert_eq!(audit.records, vec![gamma]);

        let escaped_search = storage
            .list_operation_logs(&OperationLogListQuery {
                search_key: Some("%_".to_owned()),
                ..OperationLogListQuery::default()
            })
            .expect("wildcards search literally");
        assert_eq!(escaped_search.records, vec![delta]);
    }

    #[test]
    fn invalid_operation_logs_and_pages_fail_closed() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let mut invalid = input("invalid", "datasource-1", "SELECT 1");
        invalid.operation_rows = Some(-1);
        assert!(matches!(
            storage
                .create_operation_log(invalid)
                .expect_err("negative operation row count must fail"),
            StorageError::InvalidOperationLog(_)
        ));

        let mut invalid = input("invalid", "datasource-1", "SELECT 1");
        invalid.use_time = Some(-1);
        assert!(matches!(
            storage
                .create_operation_log(invalid)
                .expect_err("negative duration must fail"),
            StorageError::InvalidOperationLog(_)
        ));

        let invalid_page = storage
            .list_operation_logs(&OperationLogListQuery {
                page_no: 0,
                ..OperationLogListQuery::default()
            })
            .expect_err("zero page must fail");
        assert!(matches!(invalid_page, StorageError::InvalidOperationLog(_)));
    }
}
