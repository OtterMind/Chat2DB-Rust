use chat2db_contract::{
    CommunityChart, CommunityDashboard, CommunityDashboardListQuery, CommunityDashboardPage,
    CreateCommunityChartRequest, CreateCommunityDashboardRequest, UpdateCommunityChartRequest,
    UpdateCommunityDashboardRequest,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::Value;

use crate::{Storage, StorageError, now_millis};

const MAX_NAME_BYTES: usize = 1_024;
const MAX_DESCRIPTION_BYTES: usize = 1024 * 1024;
const MAX_SCOPE_BYTES: usize = 1_024;
const MAX_SHORT_TEXT_BYTES: usize = 256;
const MAX_DELETED_BYTES: usize = 32;
const MAX_LARGE_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_CHART_IDS: usize = 100_000;
const MAX_CHART_IDS_JSON_BYTES: usize = 2 * 1024 * 1024;
const MAX_SEARCH_KEY_BYTES: usize = 1_024;
const MAX_PAGE_SIZE: u32 = 1_000;

const DASHBOARD_COLUMNS: &str = "id, gmt_create_ms, gmt_modified_ms, name, description,
     data_source_collection_id, chart_ids_json, schema_text, refresh_type,
     refresh_cycle_json, user_id";

const CHART_COLUMNS: &str = "id, gmt_create_ms, gmt_modified_ms, name, description, schema_text,
     data_source_id, data_source_name, schema_name, chart_type, database_name,
     ddl, deleted, user_id, chart_schema_json, meta_data_json,
     database_info_json, refresh_type, refresh_cycle_json";

impl Storage {
    /// Lists Community dashboards ordered by most recent modification.
    ///
    /// # Errors
    ///
    /// Returns validation, numeric-range, persisted-data, or `SQLite` failures.
    pub fn list_community_dashboards(
        &self,
        query: &CommunityDashboardListQuery,
    ) -> Result<CommunityDashboardPage, StorageError> {
        let page_no = query.page_no.max(1);
        let page_size = query.page_size.max(1);
        if page_size > MAX_PAGE_SIZE {
            return Err(StorageError::InvalidCommunityDashboard(
                "page size must be between 1 and 1000",
            ));
        }
        if query
            .search_key
            .as_ref()
            .is_some_and(|value| value.len() > MAX_SEARCH_KEY_BYTES)
        {
            return Err(StorageError::InvalidCommunityDashboard(
                "search key must be at most 1024 UTF-8 bytes",
            ));
        }

        let offset = u64::from(page_no - 1)
            .checked_mul(u64::from(page_size))
            .ok_or(StorageError::NumericRange(
                "Community dashboard page offset",
            ))?;
        let offset = i64::try_from(offset)
            .map_err(|_| StorageError::NumericRange("Community dashboard page offset"))?;
        let search_pattern = query
            .search_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(like_pattern);
        let connection = self.connection()?;
        let total: i64 = connection.query_row(
            "SELECT COUNT(*) FROM community_dashboards
             WHERE ?1 IS NULL
                OR name COLLATE NOCASE LIKE ?1 ESCAPE '\\'
                OR description COLLATE NOCASE LIKE ?1 ESCAPE '\\'",
            [search_pattern.as_deref()],
            |row| row.get(0),
        )?;
        let sql = format!(
            "SELECT {DASHBOARD_COLUMNS} FROM community_dashboards
             WHERE ?1 IS NULL
                OR name COLLATE NOCASE LIKE ?1 ESCAPE '\\'
                OR description COLLATE NOCASE LIKE ?1 ESCAPE '\\'
             ORDER BY gmt_modified_ms DESC, id DESC
             LIMIT ?2 OFFSET ?3"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(
            params![search_pattern.as_deref(), i64::from(page_size), offset],
            raw_dashboard,
        )?;
        let mut data = Vec::new();
        for row in rows {
            data.push(decode_dashboard(row?)?);
        }
        let total = u64::try_from(total)
            .map_err(|_| StorageError::NumericRange("Community dashboard total"))?;
        let consumed = u64::from(page_no).checked_mul(u64::from(page_size)).ok_or(
            StorageError::NumericRange("Community dashboard page boundary"),
        )?;
        Ok(CommunityDashboardPage {
            data,
            total,
            page_no,
            page_size,
            // The historical controller re-wraps this page through
            // WebPageResult, whose exact-boundary behavior uses `<=`.
            has_next_page: consumed <= total,
        })
    }

    /// Loads one Community dashboard by id, or `None` when it is absent.
    ///
    /// # Errors
    ///
    /// Returns persisted-data or `SQLite` failures.
    pub fn get_community_dashboard(
        &self,
        id: i64,
    ) -> Result<Option<CommunityDashboard>, StorageError> {
        if id <= 0 {
            return Ok(None);
        }
        load_dashboard(&self.connection()?, id)
    }

    /// Creates one Community dashboard and returns its generated id.
    ///
    /// # Errors
    ///
    /// Returns validation, clock, numeric-range, or `SQLite` failures.
    pub fn create_community_dashboard(
        &self,
        input: CreateCommunityDashboardRequest,
    ) -> Result<i64, StorageError> {
        let CreateCommunityDashboardRequest {
            name,
            description,
            data_source_collection_id,
            chart_ids,
            schema,
            refresh_type,
            refresh_cycle,
            user_id,
        } = input;
        validate_dashboard_fields(
            name.as_deref(),
            description.as_deref(),
            &chart_ids,
            schema.as_deref(),
            refresh_type.as_deref(),
            refresh_cycle.as_ref(),
        )?;
        let chart_ids_json = encode_chart_ids(&chart_ids)?;
        let refresh_cycle_json = encode_dashboard_json(refresh_cycle.as_ref(), "refresh cycle")?;
        let timestamp = now_millis()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO community_dashboards (
                gmt_create_ms, gmt_modified_ms, name, description,
                data_source_collection_id, chart_ids_json, schema_text,
                refresh_type, refresh_cycle_json, user_id
             ) VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                timestamp,
                name,
                description,
                data_source_collection_id,
                chart_ids_json,
                schema,
                refresh_type,
                refresh_cycle_json,
                user_id,
            ],
        )?;
        let id = transaction.last_insert_rowid();
        if id <= 0 {
            return Err(StorageError::Integrity(
                "Community dashboard generated a non-positive id".to_owned(),
            ));
        }
        transaction.commit()?;
        Ok(id)
    }

    /// Applies a non-null partial update to one Community dashboard.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::CommunityDashboardNotFound`] when absent, or
    /// validation, clock, numeric-range, and `SQLite` failures.
    pub fn update_community_dashboard(
        &self,
        id: i64,
        input: UpdateCommunityDashboardRequest,
    ) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_dashboard(&transaction, id)?
            .ok_or(StorageError::CommunityDashboardNotFound(id))?;
        let next = CommunityDashboard {
            id,
            gmt_create: current.gmt_create,
            gmt_modified: next_modified(current.gmt_modified)?,
            name: input.name.or(current.name),
            description: input.description.or(current.description),
            data_source_collection_id: input
                .data_source_collection_id
                .or(current.data_source_collection_id),
            chart_ids: input.chart_ids.unwrap_or(current.chart_ids),
            schema: input.schema.or(current.schema),
            refresh_type: input.refresh_type.or(current.refresh_type),
            refresh_cycle: input.refresh_cycle.or(current.refresh_cycle),
            user_id: input.user_id.or(current.user_id),
        };
        validate_dashboard(&next)?;
        let chart_ids_json = encode_chart_ids(&next.chart_ids)?;
        let refresh_cycle_json =
            encode_dashboard_json(next.refresh_cycle.as_ref(), "refresh cycle")?;
        let changed = transaction.execute(
            "UPDATE community_dashboards
             SET gmt_modified_ms = ?1, name = ?2, description = ?3,
                 data_source_collection_id = ?4, chart_ids_json = ?5,
                 schema_text = ?6, refresh_type = ?7, refresh_cycle_json = ?8,
                 user_id = ?9
             WHERE id = ?10",
            params![
                next.gmt_modified,
                next.name,
                next.description,
                next.data_source_collection_id,
                chart_ids_json,
                next.schema,
                next.refresh_type,
                refresh_cycle_json,
                next.user_id,
                id,
            ],
        )?;
        if changed != 1 {
            return Err(StorageError::CommunityDashboardNotFound(id));
        }
        transaction.commit()?;
        Ok(())
    }

    /// Deletes one Community dashboard and every chart id it references.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` failures. The dashboard and chart deletes are atomic.
    pub fn delete_community_dashboard(&self, id: i64) -> Result<bool, StorageError> {
        if id <= 0 {
            return Ok(false);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let deleted =
            transaction.execute("DELETE FROM community_dashboards WHERE id = ?1", [id])?;
        transaction.commit()?;
        Ok(deleted == 1)
    }

    /// Loads one Community chart by id, or `None` when it is absent.
    ///
    /// # Errors
    ///
    /// Returns persisted-data or `SQLite` failures.
    pub fn get_community_chart(&self, id: i64) -> Result<Option<CommunityChart>, StorageError> {
        if id <= 0 {
            return Ok(None);
        }
        load_chart(&self.connection()?, id)
    }

    /// Creates one Community chart and returns its generated id.
    ///
    /// Blank names fall back to `chartSchema.title`, then
    /// `chartSchema.summary`, matching Community.
    ///
    /// # Errors
    ///
    /// Returns validation, clock, numeric-range, or `SQLite` failures.
    pub fn create_community_chart(
        &self,
        mut input: CreateCommunityChartRequest,
    ) -> Result<i64, StorageError> {
        input.name = chart_name_for_create(input.name.take(), input.chart_schema.as_ref());
        validate_chart_fields(
            input.name.as_deref(),
            input.description.as_deref(),
            input.schema.as_deref(),
            input.data_source_name.as_deref(),
            input.schema_name.as_deref(),
            input.r#type.as_deref(),
            input.database_name.as_deref(),
            input.ddl.as_deref(),
            input.deleted.as_deref(),
            input.chart_schema.as_ref(),
            input.meta_data.as_ref(),
            input.database_info.as_ref(),
            input.refresh_type.as_deref(),
            input.refresh_cycle.as_ref(),
        )?;
        let chart_schema_json = encode_chart_json(input.chart_schema.as_ref(), "chart schema")?;
        let meta_data_json = encode_chart_json(input.meta_data.as_ref(), "metadata")?;
        let database_info_json = encode_chart_json(input.database_info.as_ref(), "database info")?;
        let refresh_cycle_json = encode_chart_json(input.refresh_cycle.as_ref(), "refresh cycle")?;
        let timestamp = now_millis()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO community_charts (
                gmt_create_ms, gmt_modified_ms, name, description, schema_text,
                data_source_id, data_source_name, schema_name, chart_type,
                database_name, ddl, deleted, user_id, chart_schema_json,
                meta_data_json, database_info_json, refresh_type,
                refresh_cycle_json
             ) VALUES (
                ?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                ?13, ?14, ?15, ?16, ?17
             )",
            params![
                timestamp,
                input.name,
                input.description,
                input.schema,
                input.data_source_id,
                input.data_source_name,
                input.schema_name,
                input.r#type,
                input.database_name,
                input.ddl,
                input.deleted,
                input.user_id,
                chart_schema_json,
                meta_data_json,
                database_info_json,
                input.refresh_type,
                refresh_cycle_json,
            ],
        )?;
        let id = transaction.last_insert_rowid();
        if id <= 0 {
            return Err(StorageError::Integrity(
                "Community chart generated a non-positive id".to_owned(),
            ));
        }
        transaction.commit()?;
        Ok(id)
    }

    /// Applies a non-null partial update to one Community chart.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::CommunityChartNotFound`] when absent, or
    /// validation, clock, numeric-range, and `SQLite` failures.
    pub fn update_community_chart(
        &self,
        id: i64,
        input: UpdateCommunityChartRequest,
    ) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current =
            load_chart(&transaction, id)?.ok_or(StorageError::CommunityChartNotFound(id))?;
        let name = chart_name_for_update(
            input.name,
            input.chart_schema.as_ref(),
            current.name.clone(),
        );
        let next = CommunityChart {
            id,
            gmt_create: current.gmt_create,
            gmt_modified: next_modified(current.gmt_modified)?,
            name,
            description: input.description.or(current.description),
            schema: input.schema.or(current.schema),
            data_source_id: input.data_source_id.or(current.data_source_id),
            data_source_name: input.data_source_name.or(current.data_source_name),
            schema_name: input.schema_name.or(current.schema_name),
            r#type: input.r#type.or(current.r#type),
            database_name: input.database_name.or(current.database_name),
            ddl: input.ddl.or(current.ddl),
            deleted: input.deleted.or(current.deleted),
            user_id: input.user_id.or(current.user_id),
            chart_schema: input.chart_schema.or(current.chart_schema),
            meta_data: input.meta_data.or(current.meta_data),
            database_info: input.database_info.or(current.database_info),
            refresh_type: input.refresh_type.or(current.refresh_type),
            refresh_cycle: input.refresh_cycle.or(current.refresh_cycle),
        };
        validate_chart(&next)?;
        let chart_schema_json = encode_chart_json(next.chart_schema.as_ref(), "chart schema")?;
        let meta_data_json = encode_chart_json(next.meta_data.as_ref(), "metadata")?;
        let database_info_json = encode_chart_json(next.database_info.as_ref(), "database info")?;
        let refresh_cycle_json = encode_chart_json(next.refresh_cycle.as_ref(), "refresh cycle")?;
        let changed = transaction.execute(
            "UPDATE community_charts
             SET gmt_modified_ms = ?1, name = ?2, description = ?3,
                 schema_text = ?4, data_source_id = ?5, data_source_name = ?6,
                 schema_name = ?7, chart_type = ?8, database_name = ?9,
                 ddl = ?10, deleted = ?11, user_id = ?12,
                 chart_schema_json = ?13, meta_data_json = ?14,
                 database_info_json = ?15, refresh_type = ?16,
                 refresh_cycle_json = ?17
             WHERE id = ?18",
            params![
                next.gmt_modified,
                next.name,
                next.description,
                next.schema,
                next.data_source_id,
                next.data_source_name,
                next.schema_name,
                next.r#type,
                next.database_name,
                next.ddl,
                next.deleted,
                next.user_id,
                chart_schema_json,
                meta_data_json,
                database_info_json,
                next.refresh_type,
                refresh_cycle_json,
                id,
            ],
        )?;
        if changed != 1 {
            return Err(StorageError::CommunityChartNotFound(id));
        }
        transaction.commit()?;
        Ok(())
    }

    /// Deletes one Community chart, returning whether it existed.
    ///
    /// # Errors
    ///
    /// Returns `SQLite` failures.
    pub fn delete_community_chart(&self, id: i64) -> Result<bool, StorageError> {
        if id <= 0 {
            return Ok(false);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let deleted = transaction.execute("DELETE FROM community_charts WHERE id = ?1", [id])?;
        transaction.commit()?;
        Ok(deleted == 1)
    }
}

fn next_modified(current: i64) -> Result<i64, StorageError> {
    let next = current.checked_add(1).ok_or(StorageError::NumericRange(
        "Community modification timestamp",
    ))?;
    Ok(now_millis()?.max(next))
}

fn chart_name_for_create(name: Option<String>, chart_schema: Option<&Value>) -> Option<String> {
    match name {
        Some(name) if !name.trim().is_empty() => Some(name),
        _ => chart_fallback_name(chart_schema),
    }
}

fn chart_name_for_update(
    name: Option<String>,
    update_chart_schema: Option<&Value>,
    current_name: Option<String>,
) -> Option<String> {
    match name {
        Some(name) if !name.trim().is_empty() => Some(name),
        _ => chart_fallback_name(update_chart_schema).or(current_name),
    }
}

fn chart_fallback_name(chart_schema: Option<&Value>) -> Option<String> {
    let schema = chart_schema?.as_object()?;
    for field in ["title", "summary"] {
        if let Some(value) = schema
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            return Some(value.to_owned());
        }
    }
    None
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

fn encode_chart_ids(chart_ids: &[i64]) -> Result<String, StorageError> {
    if chart_ids.len() > MAX_CHART_IDS {
        return Err(StorageError::InvalidCommunityDashboard(
            "chartIds must contain at most 100000 ids",
        ));
    }
    let encoded = serde_json::to_string(chart_ids).map_err(|_| {
        StorageError::Integrity("failed to encode Community dashboard chartIds".to_owned())
    })?;
    if encoded.len() > MAX_CHART_IDS_JSON_BYTES {
        return Err(StorageError::InvalidCommunityDashboard(
            "encoded chartIds exceeds the 2 MiB limit",
        ));
    }
    Ok(encoded)
}

fn encode_dashboard_json(
    value: Option<&Value>,
    field: &'static str,
) -> Result<Option<String>, StorageError> {
    encode_json(
        value,
        MAX_JSON_BYTES,
        StorageError::InvalidCommunityDashboard,
        field,
    )
}

fn encode_chart_json(
    value: Option<&Value>,
    field: &'static str,
) -> Result<Option<String>, StorageError> {
    encode_json(
        value,
        MAX_JSON_BYTES,
        StorageError::InvalidCommunityChart,
        field,
    )
}

fn encode_json<F>(
    value: Option<&Value>,
    limit: usize,
    invalid: F,
    field: &'static str,
) -> Result<Option<String>, StorageError>
where
    F: FnOnce(&'static str) -> StorageError,
{
    let Some(value) = value else {
        return Ok(None);
    };
    let encoded = serde_json::to_string(value)
        .map_err(|_| StorageError::Integrity(format!("failed to encode Community {field}")))?;
    if encoded.len() > limit {
        return Err(invalid(match field {
            "chart schema" => "chartSchema exceeds the 16 MiB encoded JSON limit",
            "metadata" => "metaData exceeds the 16 MiB encoded JSON limit",
            "database info" => "databaseInfo exceeds the 16 MiB encoded JSON limit",
            _ => "refreshCycle exceeds the 16 MiB encoded JSON limit",
        }));
    }
    Ok(Some(encoded))
}

fn decode_json(value: Option<String>, field: &'static str) -> Result<Option<Value>, StorageError> {
    value
        .map(|encoded| {
            serde_json::from_str(&encoded).map_err(|_| {
                StorageError::Integrity(format!("persisted Community {field} JSON is invalid"))
            })
        })
        .transpose()
}

fn validate_dashboard(dashboard: &CommunityDashboard) -> Result<(), StorageError> {
    if dashboard.id <= 0 {
        return Err(StorageError::InvalidCommunityDashboard(
            "persisted id must be a positive signed 64-bit integer",
        ));
    }
    if dashboard.gmt_create < 0 || dashboard.gmt_modified < dashboard.gmt_create {
        return Err(StorageError::InvalidCommunityDashboard(
            "persisted timestamps are invalid",
        ));
    }
    validate_dashboard_fields(
        dashboard.name.as_deref(),
        dashboard.description.as_deref(),
        &dashboard.chart_ids,
        dashboard.schema.as_deref(),
        dashboard.refresh_type.as_deref(),
        dashboard.refresh_cycle.as_ref(),
    )
}

fn validate_dashboard_fields(
    name: Option<&str>,
    description: Option<&str>,
    chart_ids: &[i64],
    schema: Option<&str>,
    refresh_type: Option<&str>,
    refresh_cycle: Option<&Value>,
) -> Result<(), StorageError> {
    validate_dashboard_text(
        name,
        MAX_NAME_BYTES,
        "name must be at most 1024 UTF-8 bytes",
    )?;
    validate_dashboard_text(
        description,
        MAX_DESCRIPTION_BYTES,
        "description must be at most 1 MiB",
    )?;
    validate_dashboard_text(
        schema,
        MAX_LARGE_TEXT_BYTES,
        "schema must be at most 16 MiB",
    )?;
    validate_dashboard_text(
        refresh_type,
        MAX_SHORT_TEXT_BYTES,
        "refreshType must be at most 256 UTF-8 bytes",
    )?;
    encode_chart_ids(chart_ids)?;
    encode_dashboard_json(refresh_cycle, "refresh cycle")?;
    Ok(())
}

fn validate_dashboard_text(
    value: Option<&str>,
    limit: usize,
    message: &'static str,
) -> Result<(), StorageError> {
    if value.is_some_and(|value| value.len() > limit || value.contains('\0')) {
        return Err(StorageError::InvalidCommunityDashboard(message));
    }
    Ok(())
}

fn validate_chart(chart: &CommunityChart) -> Result<(), StorageError> {
    if chart.id <= 0 {
        return Err(StorageError::InvalidCommunityChart(
            "persisted id must be a positive signed 64-bit integer",
        ));
    }
    if chart.gmt_create < 0 || chart.gmt_modified < chart.gmt_create {
        return Err(StorageError::InvalidCommunityChart(
            "persisted timestamps are invalid",
        ));
    }
    validate_chart_fields(
        chart.name.as_deref(),
        chart.description.as_deref(),
        chart.schema.as_deref(),
        chart.data_source_name.as_deref(),
        chart.schema_name.as_deref(),
        chart.r#type.as_deref(),
        chart.database_name.as_deref(),
        chart.ddl.as_deref(),
        chart.deleted.as_deref(),
        chart.chart_schema.as_ref(),
        chart.meta_data.as_ref(),
        chart.database_info.as_ref(),
        chart.refresh_type.as_deref(),
        chart.refresh_cycle.as_ref(),
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_chart_fields(
    name: Option<&str>,
    description: Option<&str>,
    schema: Option<&str>,
    data_source_name: Option<&str>,
    schema_name: Option<&str>,
    chart_type: Option<&str>,
    database_name: Option<&str>,
    ddl: Option<&str>,
    deleted: Option<&str>,
    chart_schema: Option<&Value>,
    meta_data: Option<&Value>,
    database_info: Option<&Value>,
    refresh_type: Option<&str>,
    refresh_cycle: Option<&Value>,
) -> Result<(), StorageError> {
    validate_chart_text(
        name,
        MAX_NAME_BYTES,
        "name must be at most 1024 UTF-8 bytes",
    )?;
    validate_chart_text(
        description,
        MAX_DESCRIPTION_BYTES,
        "description must be at most 1 MiB",
    )?;
    validate_chart_text(
        schema,
        MAX_LARGE_TEXT_BYTES,
        "schema must be at most 16 MiB",
    )?;
    for value in [data_source_name, schema_name, database_name] {
        validate_chart_text(
            value,
            MAX_SCOPE_BYTES,
            "datasource, schema, and database names must be at most 1024 UTF-8 bytes",
        )?;
    }
    validate_chart_text(
        chart_type,
        MAX_SHORT_TEXT_BYTES,
        "type must be at most 256 UTF-8 bytes",
    )?;
    validate_chart_text(ddl, MAX_LARGE_TEXT_BYTES, "ddl must be at most 16 MiB")?;
    validate_chart_text(
        deleted,
        MAX_DELETED_BYTES,
        "deleted must be at most 32 UTF-8 bytes",
    )?;
    validate_chart_text(
        refresh_type,
        MAX_SHORT_TEXT_BYTES,
        "refreshType must be at most 256 UTF-8 bytes",
    )?;
    encode_chart_json(chart_schema, "chart schema")?;
    encode_chart_json(meta_data, "metadata")?;
    encode_chart_json(database_info, "database info")?;
    encode_chart_json(refresh_cycle, "refresh cycle")?;
    Ok(())
}

fn validate_chart_text(
    value: Option<&str>,
    limit: usize,
    message: &'static str,
) -> Result<(), StorageError> {
    if value.is_some_and(|value| value.len() > limit || value.contains('\0')) {
        return Err(StorageError::InvalidCommunityChart(message));
    }
    Ok(())
}

struct RawDashboard {
    id: i64,
    gmt_create: i64,
    gmt_modified: i64,
    name: Option<String>,
    description: Option<String>,
    data_source_collection_id: Option<i64>,
    chart_ids_json: String,
    schema: Option<String>,
    refresh_type: Option<String>,
    refresh_cycle_json: Option<String>,
    user_id: Option<i64>,
}

fn raw_dashboard(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawDashboard> {
    Ok(RawDashboard {
        id: row.get(0)?,
        gmt_create: row.get(1)?,
        gmt_modified: row.get(2)?,
        name: row.get(3)?,
        description: row.get(4)?,
        data_source_collection_id: row.get(5)?,
        chart_ids_json: row.get(6)?,
        schema: row.get(7)?,
        refresh_type: row.get(8)?,
        refresh_cycle_json: row.get(9)?,
        user_id: row.get(10)?,
    })
}

fn decode_dashboard(raw: RawDashboard) -> Result<CommunityDashboard, StorageError> {
    let chart_ids = serde_json::from_str(&raw.chart_ids_json).map_err(|_| {
        StorageError::Integrity("persisted Community dashboard chartIds JSON is invalid".to_owned())
    })?;
    let dashboard = CommunityDashboard {
        id: raw.id,
        gmt_create: raw.gmt_create,
        gmt_modified: raw.gmt_modified,
        name: raw.name,
        description: raw.description,
        data_source_collection_id: raw.data_source_collection_id,
        chart_ids,
        schema: raw.schema,
        refresh_type: raw.refresh_type,
        refresh_cycle: decode_json(raw.refresh_cycle_json, "dashboard refreshCycle")?,
        user_id: raw.user_id,
    };
    validate_dashboard(&dashboard)?;
    Ok(dashboard)
}

fn load_dashboard(
    connection: &Connection,
    id: i64,
) -> Result<Option<CommunityDashboard>, StorageError> {
    let sql = format!("SELECT {DASHBOARD_COLUMNS} FROM community_dashboards WHERE id = ?1");
    connection
        .query_row(&sql, [id], raw_dashboard)
        .optional()?
        .map(decode_dashboard)
        .transpose()
}

struct RawChart {
    id: i64,
    gmt_create: i64,
    gmt_modified: i64,
    name: Option<String>,
    description: Option<String>,
    schema: Option<String>,
    data_source_id: Option<i64>,
    data_source_name: Option<String>,
    schema_name: Option<String>,
    chart_type: Option<String>,
    database_name: Option<String>,
    ddl: Option<String>,
    deleted: Option<String>,
    user_id: Option<i64>,
    chart_schema_json: Option<String>,
    meta_data_json: Option<String>,
    database_info_json: Option<String>,
    refresh_type: Option<String>,
    refresh_cycle_json: Option<String>,
}

fn raw_chart(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawChart> {
    Ok(RawChart {
        id: row.get(0)?,
        gmt_create: row.get(1)?,
        gmt_modified: row.get(2)?,
        name: row.get(3)?,
        description: row.get(4)?,
        schema: row.get(5)?,
        data_source_id: row.get(6)?,
        data_source_name: row.get(7)?,
        schema_name: row.get(8)?,
        chart_type: row.get(9)?,
        database_name: row.get(10)?,
        ddl: row.get(11)?,
        deleted: row.get(12)?,
        user_id: row.get(13)?,
        chart_schema_json: row.get(14)?,
        meta_data_json: row.get(15)?,
        database_info_json: row.get(16)?,
        refresh_type: row.get(17)?,
        refresh_cycle_json: row.get(18)?,
    })
}

fn decode_chart(raw: RawChart) -> Result<CommunityChart, StorageError> {
    let chart = CommunityChart {
        id: raw.id,
        gmt_create: raw.gmt_create,
        gmt_modified: raw.gmt_modified,
        name: raw.name,
        description: raw.description,
        schema: raw.schema,
        data_source_id: raw.data_source_id,
        data_source_name: raw.data_source_name,
        schema_name: raw.schema_name,
        r#type: raw.chart_type,
        database_name: raw.database_name,
        ddl: raw.ddl,
        deleted: raw.deleted,
        user_id: raw.user_id,
        chart_schema: decode_json(raw.chart_schema_json, "chartSchema")?,
        meta_data: decode_json(raw.meta_data_json, "metaData")?,
        database_info: decode_json(raw.database_info_json, "databaseInfo")?,
        refresh_type: raw.refresh_type,
        refresh_cycle: decode_json(raw.refresh_cycle_json, "chart refreshCycle")?,
    };
    validate_chart(&chart)?;
    Ok(chart)
}

fn load_chart(connection: &Connection, id: i64) -> Result<Option<CommunityChart>, StorageError> {
    let sql = format!("SELECT {CHART_COLUMNS} FROM community_charts WHERE id = ?1");
    connection
        .query_row(&sql, [id], raw_chart)
        .optional()?
        .map(decode_chart)
        .transpose()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use tempfile::TempDir;

    use super::MAX_JSON_BYTES;
    use crate::{
        CommunityDashboardListQuery, CreateCommunityChartRequest, CreateCommunityDashboardRequest,
        SecretRef, SecretValue, SecretVault, SecretVaultError, Storage, StorageError,
        UpdateCommunityChartRequest, UpdateCommunityDashboardRequest,
    };

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

    fn chart(name: Option<&str>, chart_schema: Value) -> CreateCommunityChartRequest {
        CreateCommunityChartRequest {
            name: name.map(str::to_owned),
            description: Some("Revenue by region".to_owned()),
            schema: Some(r#"{"legacy":true}"#.to_owned()),
            data_source_id: Some(42),
            data_source_name: Some("Local MySQL".to_owned()),
            schema_name: Some("analytics".to_owned()),
            r#type: Some("BAR".to_owned()),
            database_name: Some("warehouse".to_owned()),
            ddl: Some("select region, sum(amount) from sales group by region".to_owned()),
            deleted: Some("N".to_owned()),
            user_id: Some(7),
            chart_schema: Some(chart_schema),
            meta_data: Some(json!({"rows": [{"region": "east", "amount": 10.50}]})),
            database_info: Some(json!({
                "dataSourceId": 42,
                "databaseName": "warehouse",
                "schemaName": "analytics",
                "sql": "select 1"
            })),
            refresh_type: Some("AUTO".to_owned()),
            refresh_cycle: Some(json!({"unit": "seconds", "value": 30})),
        }
    }

    #[test]
    fn dashboard_and_chart_round_trip_across_restart_with_partial_updates() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let chart_id = storage
            .create_community_chart(chart(Some("  "), json!({"title": "Revenue"})))
            .expect("chart creates");
        let dashboard_id = storage
            .create_community_dashboard(CreateCommunityDashboardRequest {
                name: Some("Executive".to_owned()),
                description: Some("Initial description".to_owned()),
                data_source_collection_id: Some(12),
                chart_ids: vec![chart_id],
                schema: Some(r#"[{"i":"chart"}]"#.to_owned()),
                refresh_type: Some("MANUAL".to_owned()),
                refresh_cycle: Some(json!({"cron": "0 * * * *"})),
                user_id: Some(7),
            })
            .expect("dashboard creates");
        let before = storage
            .get_community_dashboard(dashboard_id)
            .expect("dashboard reads")
            .expect("dashboard exists");
        storage
            .update_community_dashboard(
                dashboard_id,
                UpdateCommunityDashboardRequest {
                    description: Some("Updated description".to_owned()),
                    ..UpdateCommunityDashboardRequest::default()
                },
            )
            .expect("dashboard updates");
        storage
            .update_community_chart(
                chart_id,
                UpdateCommunityChartRequest {
                    name: Some(String::new()),
                    chart_schema: Some(json!({"summary": "Revenue summary"})),
                    ..UpdateCommunityChartRequest::default()
                },
            )
            .expect("chart updates");
        drop(storage);

        let reopened = open(&directory);
        let dashboard = reopened
            .get_community_dashboard(dashboard_id)
            .expect("dashboard rereads")
            .expect("dashboard survives restart");
        assert_eq!(dashboard.name.as_deref(), Some("Executive"));
        assert_eq!(
            dashboard.description.as_deref(),
            Some("Updated description")
        );
        assert_eq!(dashboard.chart_ids, vec![chart_id]);
        assert_eq!(dashboard.refresh_cycle, Some(json!({"cron": "0 * * * *"})));
        assert!(dashboard.gmt_modified > before.gmt_modified);

        let chart = reopened
            .get_community_chart(chart_id)
            .expect("chart rereads")
            .expect("chart survives restart");
        assert_eq!(chart.name.as_deref(), Some("Revenue summary"));
        assert_eq!(chart.description.as_deref(), Some("Revenue by region"));
        assert_eq!(
            chart.chart_schema,
            Some(json!({"summary": "Revenue summary"}))
        );
        assert_eq!(
            chart.meta_data,
            Some(json!({"rows": [{"region": "east", "amount": 10.50}]}))
        );
        assert_eq!(
            chart
                .database_info
                .as_ref()
                .and_then(|value| value.get("sql")),
            Some(&json!("select 1"))
        );
    }

    #[test]
    fn dashboard_list_is_modified_desc_paged_and_case_insensitive() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let first = storage
            .create_community_dashboard(CreateCommunityDashboardRequest {
                name: Some("Alpha".to_owned()),
                description: Some("Quarterly SALES".to_owned()),
                ..CreateCommunityDashboardRequest::default()
            })
            .expect("first creates");
        let second = storage
            .create_community_dashboard(CreateCommunityDashboardRequest {
                name: Some("Beta".to_owned()),
                description: Some("Operations".to_owned()),
                ..CreateCommunityDashboardRequest::default()
            })
            .expect("second creates");
        let third = storage
            .create_community_dashboard(CreateCommunityDashboardRequest {
                name: Some("Gamma".to_owned()),
                description: Some("Sales forecast".to_owned()),
                ..CreateCommunityDashboardRequest::default()
            })
            .expect("third creates");
        storage
            .update_community_dashboard(
                first,
                UpdateCommunityDashboardRequest {
                    description: Some("Quarterly SALES updated".to_owned()),
                    ..UpdateCommunityDashboardRequest::default()
                },
            )
            .expect("first becomes newest");

        let page = storage
            .list_community_dashboards(&CommunityDashboardListQuery {
                page_no: 1,
                page_size: 2,
                search_key: None,
            })
            .expect("page lists");
        assert_eq!(page.total, 3);
        assert_eq!(page.data.len(), 2);
        assert!(page.has_next_page);
        assert_eq!(page.data[0].id, first);
        assert_eq!(page.data[1].id, third);

        let exact_page = storage
            .list_community_dashboards(&CommunityDashboardListQuery {
                page_no: 1,
                page_size: 3,
                search_key: None,
            })
            .expect("exact boundary page lists");
        assert_eq!(exact_page.total, 3);
        assert_eq!(exact_page.data.len(), 3);
        assert!(
            exact_page.has_next_page,
            "Community WebPageResult reports another page at an exact boundary"
        );

        let search = storage
            .list_community_dashboards(&CommunityDashboardListQuery {
                search_key: Some("sAlEs".to_owned()),
                ..CommunityDashboardListQuery::default()
            })
            .expect("search lists");
        assert_eq!(search.total, 2);
        assert_eq!(search.data[0].id, first);
        assert_eq!(search.data[1].id, third);

        let literal_wildcard = storage
            .list_community_dashboards(&CommunityDashboardListQuery {
                search_key: Some("%".to_owned()),
                ..CommunityDashboardListQuery::default()
            })
            .expect("literal wildcard searches");
        assert_eq!(literal_wildcard.total, 0);
        assert_ne!(second, third);
    }

    #[test]
    fn dashboard_delete_cascades_only_referenced_charts() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let first = storage
            .create_community_chart(chart(None, json!({"title": "First"})))
            .expect("first chart creates");
        let second = storage
            .create_community_chart(chart(None, json!({"summary": "Second"})))
            .expect("second chart creates");
        let unrelated = storage
            .create_community_chart(chart(Some("Unrelated"), json!({})))
            .expect("unrelated chart creates");
        let dashboard = storage
            .create_community_dashboard(CreateCommunityDashboardRequest {
                chart_ids: vec![first, second, first],
                ..CreateCommunityDashboardRequest::default()
            })
            .expect("dashboard creates");

        assert!(
            storage
                .delete_community_dashboard(dashboard)
                .expect("dashboard deletes")
        );
        assert!(
            storage
                .get_community_dashboard(dashboard)
                .expect("dashboard absence reads")
                .is_none()
        );
        assert!(
            storage
                .get_community_chart(first)
                .expect("first absence reads")
                .is_none()
        );
        assert!(
            storage
                .get_community_chart(second)
                .expect("second absence reads")
                .is_none()
        );
        assert!(
            storage
                .get_community_chart(unrelated)
                .expect("unrelated reads")
                .is_some()
        );
        assert!(
            !storage
                .delete_community_dashboard(dashboard)
                .expect("missing delete is idempotent")
        );
    }

    #[test]
    fn defaults_missing_records_and_json_limits_are_enforced() {
        let directory = TempDir::new().expect("temp dir");
        let storage = open(&directory);
        let dashboard = storage
            .create_community_dashboard(CreateCommunityDashboardRequest::default())
            .expect("default dashboard creates");
        assert!(
            storage
                .get_community_dashboard(dashboard)
                .expect("dashboard reads")
                .expect("dashboard exists")
                .chart_ids
                .is_empty()
        );
        assert!(
            storage
                .get_community_dashboard(9_999_999)
                .expect("missing dashboard reads")
                .is_none()
        );
        assert!(
            storage
                .get_community_chart(9_999_999)
                .expect("missing chart reads")
                .is_none()
        );
        assert!(matches!(
            storage
                .update_community_dashboard(9_999_999, UpdateCommunityDashboardRequest::default()),
            Err(StorageError::CommunityDashboardNotFound(9_999_999))
        ));
        assert!(matches!(
            storage.update_community_chart(9_999_999, UpdateCommunityChartRequest::default()),
            Err(StorageError::CommunityChartNotFound(9_999_999))
        ));

        let oversized = "x".repeat(MAX_JSON_BYTES + 1);
        let error = storage
            .create_community_chart(CreateCommunityChartRequest {
                chart_schema: Some(json!({"value": oversized})),
                ..CreateCommunityChartRequest::default()
            })
            .expect_err("oversized JSON is rejected");
        assert!(matches!(error, StorageError::InvalidCommunityChart(_)));
    }
}
