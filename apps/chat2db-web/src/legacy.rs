//! Compatibility adapter for the retained Community frontend HTTP contract.
//!
//! The functions in this module are transport-neutral apart from the thin
//! Axum handlers at the bottom. Desktop IPC can reuse the same request and
//! response translations without duplicating datasource or JDBC behavior.

use std::{collections::HashSet, time::Duration};

use axum::{
    Json, Router,
    extract::{Query, State},
    routing::{get, post},
};
use chat2db_contract::{
    ApiError, ColumnNullability, CommunityTable, Datasource, DatasourceConnection,
    DatasourceConnectionProperty, DatasourceSecretChange, JdbcDriver, JdbcValue, JdbcValueType,
    ListCommunityDatabasesRequest, ListCommunitySchemasRequest, ListCommunityTablesRequest,
    OperationEvent, ResultColumn, ResultMetadata, ResultPageRequest,
    StartCommunityTablePreviewRequest, UpdateDatasourceRequest,
};
use chat2db_core::{AppError, Application};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};

const DEFAULT_PAGE_NO: u32 = 1;
const DEFAULT_PAGE_SIZE: u32 = 20;
const DEFAULT_PREVIEW_PAGE_SIZE: u32 = 200;
const MAX_PREVIEW_ROWS: u32 = 1_000;
const RESULT_PAGE_MAX_BYTES: u64 = 8 * 1024 * 1024;
const PREVIEW_TIMEOUT: Duration = Duration::from_secs(30);

/// Community's historical response wrapper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyEnvelope<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

impl<T> LegacyEnvelope<T> {
    fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error_code: None,
            error_message: None,
        }
    }

    fn failure(error: LegacyFailure) -> Self {
        Self {
            success: false,
            data: None,
            error_code: Some(error.code),
            error_message: Some(error.message),
        }
    }
}

/// Safe failure projected into the historical response wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyFailure {
    pub code: String,
    pub message: String,
}

impl LegacyFailure {
    fn invalid(code: &'static str, message: &'static str) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }

    fn from_api(error: ApiError) -> Self {
        Self {
            code: error.code,
            message: error.message,
        }
    }
}

impl From<AppError> for LegacyFailure {
    fn from(error: AppError) -> Self {
        Self::from_api(error.api_error())
    }
}

pub type LegacyResult<T> = Result<T, LegacyFailure>;

/// Identifier accepted as either the old numeric form or the Rust opaque id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LegacyIdentifier {
    Text(String),
    Unsigned(u64),
    Signed(i64),
}

impl LegacyIdentifier {
    fn as_string(&self) -> String {
        match self {
            Self::Text(value) => value.clone(),
            Self::Unsigned(value) => value.to_string(),
            Self::Signed(value) => value.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyEnvironment {
    pub id: u64,
    pub name: String,
    pub short_name: String,
    pub color: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyDriverConfig {
    #[serde(default)]
    pub jdbc_driver: String,
    #[serde(default)]
    pub jdbc_driver_class: String,
    #[serde(default)]
    pub custom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyDriverResponse {
    pub db_type: String,
    pub name: String,
    pub default_driver_config: LegacyDriverConfig,
    pub driver_config_list: Vec<LegacyDriverConfig>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyConnectionProperty {
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub key: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub value: String,
}

/// Superset of the create, update, and connection-test payloads used by the
/// retained Community connection form.
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyDatasourceRequest {
    #[serde(default)]
    pub id: Option<LegacyIdentifier>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub alias: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub url: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub user: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub password: String,
    #[serde(
        rename = "type",
        default,
        deserialize_with = "deserialize_string_or_default"
    )]
    pub database_type: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub driver: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub jdbc: String,
    #[serde(default)]
    pub driver_config: Option<LegacyDriverConfig>,
    #[serde(default, deserialize_with = "deserialize_vec_or_default")]
    pub extend_info: Vec<LegacyConnectionProperty>,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyDatasourceResponse {
    pub id: String,
    pub alias: String,
    #[serde(rename = "type")]
    pub database_type: String,
    pub url: String,
    pub user: String,
    pub password: String,
    pub environment: LegacyEnvironment,
    pub environment_id: u64,
    pub extend_info: Vec<LegacyConnectionPropertyResponse>,
    pub driver_config: LegacyDriverConfig,
    pub storage_type: String,
    pub space_id: u64,
    pub support_database: bool,
    pub support_schema: bool,
    pub has_secret: bool,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyConnectionPropertyResponse {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyPage<T> {
    pub data: Vec<T>,
    pub page_no: u32,
    pub page_size: u32,
    pub total: usize,
    pub has_next_page: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyNamespaceNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub name: String,
    pub data: LegacyDatasourceResponse,
    pub children: Vec<Self>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyDatabase {
    pub name: String,
    pub description: String,
    pub count: u64,
    pub system: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySchema {
    pub name: String,
    pub system: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyTable {
    pub name: String,
    pub comment: String,
    pub table_type: String,
    pub pinned: bool,
    pub ddl: String,
    pub engine: String,
    pub charset: String,
    pub collate: String,
    pub increment_value: Option<String>,
    pub partition: String,
    pub tablespace: String,
    pub rows: Option<String>,
    pub data_length: Option<String>,
    pub create_time: String,
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyResultHeader {
    pub data_type: String,
    pub name: String,
    pub column_name: String,
    pub column_type: String,
    pub table_name: Option<String>,
    pub primary_key: bool,
    pub nullable: bool,
    pub column_size: Option<u32>,
    pub decimal_digits: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyResultCell {
    pub value: Option<String>,
    pub large_value: bool,
    pub value_type: String,
    pub sql_type: i32,
    pub column_type: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyManageResult {
    pub data_list: Vec<Vec<LegacyResultCell>>,
    pub header_list: Vec<LegacyResultHeader>,
    pub description: String,
    pub message: String,
    pub sql: String,
    pub original_sql: String,
    pub success: bool,
    pub duration: u64,
    pub update_count: u64,
    pub can_edit: bool,
    pub table_name: String,
    pub sql_type: String,
    pub refresh_targets: Vec<serde_json::Value>,
    pub page_no: u32,
    pub page_size: u32,
    pub fuzzy_total: String,
    pub has_next_page: bool,
    pub execute_sql_params: LegacyTablePreviewRequest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyPageQuery {
    #[serde(default = "default_page_no")]
    pub page_no: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    #[serde(default)]
    pub search_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyIdQuery {
    pub id: LegacyIdentifier,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyDriverQuery {
    #[serde(default)]
    pub db_type: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyMetadataQuery {
    pub data_source_id: LegacyIdentifier,
    #[serde(default)]
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
    #[serde(default)]
    pub database_type: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyTableListQuery {
    pub data_source_id: LegacyIdentifier,
    #[serde(default)]
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
    #[serde(default)]
    pub database_type: String,
    #[serde(default = "default_page_no")]
    pub page_no: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    #[serde(default)]
    pub search_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyTablePreviewRequest {
    pub data_source_id: LegacyIdentifier,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub schema_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_type: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub table_name: String,
    #[serde(default = "default_page_no")]
    pub page_no: u32,
    #[serde(default = "default_preview_page_size")]
    pub page_size: u32,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub sql: String,
}

/// Transport-neutral request used by the retained desktop command-line bridge.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyDispatchRequest {
    pub request_url: String,
    pub method: String,
    #[serde(default)]
    pub message: serde_json::Value,
}

fn deserialize_string_or_default<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_vec_or_default<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

fn default_page_no() -> u32 {
    DEFAULT_PAGE_NO
}

fn default_page_size() -> u32 {
    DEFAULT_PAGE_SIZE
}

fn default_preview_page_size() -> u32 {
    DEFAULT_PREVIEW_PAGE_SIZE
}

fn default_environment() -> LegacyEnvironment {
    LegacyEnvironment {
        id: 1,
        name: "Default".to_owned(),
        short_name: "DEFAULT".to_owned(),
        color: "#1677ff".to_owned(),
    }
}

/// Lists the fixed local environment expected by the Community form.
#[must_use]
pub fn environments() -> Vec<LegacyEnvironment> {
    vec![default_environment()]
}

/// Adapts the verified driver inventory to Community's driver selector.
#[must_use]
pub fn drivers(application: &Application, requested_type: &str) -> LegacyDriverResponse {
    let requested_type = normalize_database_type(requested_type);
    let inventory = application.list_drivers();
    let matching: Vec<&JdbcDriver> = inventory
        .items
        .iter()
        .filter(|driver| {
            requested_type.is_empty() || database_type_for_driver(driver) == requested_type
        })
        .collect();
    let driver_config_list: Vec<LegacyDriverConfig> = matching
        .iter()
        .map(|driver| LegacyDriverConfig {
            jdbc_driver: driver.driver_id.clone(),
            jdbc_driver_class: driver.driver_class.clone(),
            custom: false,
        })
        .collect();
    let default_driver_config = driver_config_list.first().cloned().unwrap_or_default();
    LegacyDriverResponse {
        db_type: requested_type,
        name: matching
            .first()
            .map_or_else(String::new, |driver| driver.name.clone()),
        default_driver_config,
        driver_config_list,
    }
}

/// Lists and paginates datasource records using the old page DTO.
pub(crate) async fn list_datasources(
    application: &Application,
    query: &LegacyPageQuery,
) -> LegacyResult<LegacyPage<LegacyDatasourceResponse>> {
    let mut items: Vec<LegacyDatasourceResponse> = application
        .list_datasources()
        .await?
        .items
        .into_iter()
        .map(|datasource| datasource_response(application, datasource))
        .collect();
    if !query.search_key.trim().is_empty() {
        let needle = query.search_key.to_lowercase();
        items.retain(|item| item.alias.to_lowercase().contains(&needle));
    }
    Ok(paginate(items, query.page_no, query.page_size))
}

/// Gets one secret-free datasource in the historical shape.
pub(crate) async fn get_datasource(
    application: &Application,
    id: &LegacyIdentifier,
) -> LegacyResult<LegacyDatasourceResponse> {
    let datasource = application.get_datasource(&id.as_string()).await?;
    Ok(datasource_response(application, datasource))
}

/// Creates a datasource while keeping JDBC material inside Core's vault path.
pub(crate) async fn create_datasource(
    application: &Application,
    request: &LegacyDatasourceRequest,
) -> LegacyResult<LegacyDatasourceResponse> {
    let driver_id = resolve_driver_id(application, request)?;
    let connection = datasource_connection(request)?;
    let name = datasource_name(request);
    let datasource = application
        .create_datasource(chat2db_contract::CreateDatasourceRequest {
            name,
            driver_id,
            connection: Some(connection),
        })
        .await?;
    Ok(datasource_response(application, datasource))
}

/// Tests a saved or unsaved datasource without persisting an unsaved request.
pub(crate) async fn pre_connect(
    application: &Application,
    request: &LegacyDatasourceRequest,
) -> LegacyResult<bool> {
    if request.url.trim().is_empty()
        && let Some(id) = &request.id
    {
        let datasource_id = id.as_string();
        let database_type =
            resolve_database_type(application, &datasource_id, &request.database_type).await?;
        application
            .list_community_databases(ListCommunityDatabasesRequest {
                datasource_id,
                database_type,
            })
            .await?;
        return Ok(true);
    }

    let driver_id = resolve_driver_id(application, request)?;
    let connection = datasource_connection(request)?;
    application
        .test_datasource_connection(&driver_id, connection)
        .await?;
    Ok(true)
}

/// Updates a datasource using the latest revision hidden by the old API.
pub(crate) async fn update_datasource(
    application: &Application,
    request: &LegacyDatasourceRequest,
) -> LegacyResult<LegacyDatasourceResponse> {
    let id = request
        .id
        .as_ref()
        .ok_or_else(|| LegacyFailure::invalid("invalid_datasource_request", "id is required"))?
        .as_string();
    let existing = application.get_datasource(&id).await?;
    let driver_id = if has_driver_selection(request) {
        resolve_driver_id(application, request)?
    } else {
        existing.driver_id.clone()
    };
    let secret_change = if request.url.trim().is_empty() {
        DatasourceSecretChange::Keep
    } else {
        DatasourceSecretChange::Replace {
            connection: datasource_connection(request)?,
        }
    };
    let name = if request.alias.trim().is_empty() {
        existing.name
    } else {
        request.alias.trim().to_owned()
    };
    let datasource = application
        .update_datasource(
            &id,
            UpdateDatasourceRequest {
                expected_revision: existing.revision,
                name,
                driver_id,
                secret_change,
            },
        )
        .await?;
    Ok(datasource_response(application, datasource))
}

/// Deletes a datasource using the latest revision hidden by the old API.
pub(crate) async fn delete_datasource(
    application: &Application,
    id: &LegacyIdentifier,
) -> LegacyResult<()> {
    let id = id.as_string();
    let existing = application.get_datasource(&id).await?;
    application
        .delete_datasource(&id, &existing.revision)
        .await?;
    Ok(())
}

/// Builds the flat namespace tree used when no custom grouping exists.
pub(crate) async fn namespace_tree(
    application: &Application,
) -> LegacyResult<Vec<LegacyNamespaceNode>> {
    Ok(application
        .list_datasources()
        .await?
        .items
        .into_iter()
        .map(|datasource| {
            let response = datasource_response(application, datasource);
            LegacyNamespaceNode {
                id: response.id.clone(),
                node_type: "DATA_SOURCE".to_owned(),
                name: response.alias.clone(),
                data: response,
                children: Vec::new(),
            }
        })
        .collect())
}

/// Lists databases through the retained Community metadata implementation.
pub(crate) async fn list_databases(
    application: &Application,
    query: &LegacyMetadataQuery,
) -> LegacyResult<Vec<LegacyDatabase>> {
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &query.database_type).await?;
    Ok(application
        .list_community_databases(ListCommunityDatabasesRequest {
            datasource_id,
            database_type,
        })
        .await?
        .items
        .into_iter()
        .map(|database| LegacyDatabase {
            name: database.name,
            description: database.comment,
            count: 0,
            system: database.system,
        })
        .collect())
}

/// Lists schemas through the retained Community metadata implementation.
pub(crate) async fn list_schemas(
    application: &Application,
    query: &LegacyMetadataQuery,
) -> LegacyResult<Vec<LegacySchema>> {
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &query.database_type).await?;
    Ok(application
        .list_community_schemas(ListCommunitySchemasRequest {
            datasource_id,
            database_type,
            database_name: query.database_name.clone(),
        })
        .await?
        .items
        .into_iter()
        .map(|schema| LegacySchema {
            name: schema.name,
            system: schema.system,
        })
        .collect())
}

/// Lists and paginates tables through Community metadata.
pub(crate) async fn list_tables(
    application: &Application,
    query: &LegacyTableListQuery,
) -> LegacyResult<LegacyPage<LegacyTable>> {
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &query.database_type).await?;
    let mut items: Vec<LegacyTable> = application
        .list_community_tables(ListCommunityTablesRequest {
            datasource_id,
            database_type,
            database_name: query.database_name.clone(),
            schema_name: query.schema_name.clone(),
            // Community's MySQL metadata implementation requires an empty
            // pattern to enumerate all tables.
            table_name_pattern: String::new(),
        })
        .await?
        .items
        .into_iter()
        .map(table_response)
        .collect();
    if !query.search_key.trim().is_empty() {
        let needle = query.search_key.to_lowercase();
        items.retain(|item| item.name.to_lowercase().contains(&needle));
    }
    Ok(paginate(items, query.page_no, query.page_size))
}

/// Runs a table preview through the generated-SQL, forced-read-only Core path
/// and waits for its retained result so the old synchronous frontend can use it.
#[allow(clippy::too_many_lines)]
pub(crate) async fn preview_table(
    application: &Application,
    request: &LegacyTablePreviewRequest,
) -> LegacyResult<Vec<LegacyManageResult>> {
    if request.table_name.trim().is_empty() {
        return Err(LegacyFailure::invalid(
            "invalid_table_preview_request",
            "tableName is required",
        ));
    }
    if request.page_no == 0 || request.page_size == 0 {
        return Err(LegacyFailure::invalid(
            "invalid_table_preview_request",
            "pageNo and pageSize must be positive",
        ));
    }
    let offset = request
        .page_no
        .saturating_sub(1)
        .checked_mul(request.page_size)
        .ok_or_else(|| {
            LegacyFailure::invalid(
                "invalid_table_preview_request",
                "requested page is outside the preview window",
            )
        })?;
    let row_limit = offset.checked_add(request.page_size).ok_or_else(|| {
        LegacyFailure::invalid(
            "invalid_table_preview_request",
            "requested page is outside the preview window",
        )
    })?;
    if offset >= MAX_PREVIEW_ROWS || row_limit > MAX_PREVIEW_ROWS {
        return Err(LegacyFailure::invalid(
            "invalid_table_preview_request",
            "table preview is limited to the first 1000 rows",
        ));
    }

    let datasource_id = request.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &request.database_type).await?;
    let accepted = application
        .start_community_table_preview(StartCommunityTablePreviewRequest {
            datasource_id,
            database_type,
            database_name: request.database_name.clone(),
            schema_name: request.schema_name.clone(),
            table_name: request.table_name.clone(),
            row_limit: Some(row_limit),
        })
        .await?;

    let preview_result = tokio::time::timeout(
        PREVIEW_TIMEOUT,
        wait_for_result(application, &accepted.operation_id),
    )
    .await;
    let Ok(preview_result) = preview_result else {
        application.cancel_operation(&accepted.operation_id).await;
        return Err(LegacyFailure::invalid(
            "table_preview_timeout",
            "The table preview did not finish in time",
        ));
    };
    let metadata = preview_result?;
    let page = application
        .result_page(
            &metadata.id,
            ResultPageRequest {
                offset: offset.to_string(),
                max_rows: request.page_size.to_string(),
                max_bytes: RESULT_PAGE_MAX_BYTES.to_string(),
            },
        )
        .await?;
    let has_next_page = page.has_more
        || page.metadata.truncated_by_max_rows
        || page.metadata.truncated_by_max_result_bytes;
    let headers: Vec<LegacyResultHeader> = page.columns.iter().map(result_header).collect();
    let data_list = page
        .rows
        .into_iter()
        .map(|row| {
            row.values
                .into_iter()
                .zip(page.columns.iter())
                .map(|(value, column)| result_cell(value, column))
                .collect()
        })
        .collect();
    Ok(vec![LegacyManageResult {
        data_list,
        header_list: headers,
        description: "Query executed successfully".to_owned(),
        message: String::new(),
        sql: accepted.sql.clone(),
        original_sql: accepted.sql,
        success: true,
        duration: 0,
        update_count: 0,
        can_edit: false,
        table_name: request.table_name.clone(),
        sql_type: "SELECT".to_owned(),
        refresh_targets: Vec::new(),
        page_no: request.page_no,
        page_size: request.page_size,
        fuzzy_total: page.metadata.row_count,
        has_next_page,
        execute_sql_params: request.clone(),
    }])
}

async fn wait_for_result(
    application: &Application,
    operation_id: &str,
) -> LegacyResult<ResultMetadata> {
    let mut subscription = application.subscribe_operation(operation_id, None).await?;
    while let Some(envelope) = subscription.next_event().await? {
        match envelope.event {
            OperationEvent::Completed { result } => return Ok(result),
            OperationEvent::Failed { error } => return Err(LegacyFailure::from_api(error)),
            OperationEvent::Cancelled { .. } => {
                return Err(LegacyFailure::invalid(
                    "table_preview_cancelled",
                    "The table preview was cancelled",
                ));
            }
            OperationEvent::Started | OperationEvent::Progress { .. } => {}
        }
    }
    Err(LegacyFailure::invalid(
        "table_preview_incomplete",
        "The table preview ended without a result",
    ))
}

fn datasource_response(
    application: &Application,
    datasource: Datasource,
) -> LegacyDatasourceResponse {
    let driver = application
        .list_drivers()
        .items
        .into_iter()
        .find(|driver| driver.driver_id == datasource.driver_id);
    let database_type = driver.as_ref().map_or_else(
        || normalize_database_type(&datasource.driver_id),
        database_type_for_driver,
    );
    let driver_config = driver.map_or_else(
        || LegacyDriverConfig {
            jdbc_driver: datasource.driver_id.clone(),
            jdbc_driver_class: String::new(),
            custom: false,
        },
        |driver| LegacyDriverConfig {
            jdbc_driver: driver.driver_id,
            jdbc_driver_class: driver.driver_class,
            custom: false,
        },
    );
    let support_schema = !matches!(database_type.as_str(), "MYSQL" | "SQLITE");
    LegacyDatasourceResponse {
        id: datasource.id,
        alias: datasource.name,
        database_type,
        // Core deliberately never exposes stored connection material.
        url: String::new(),
        user: String::new(),
        password: String::new(),
        environment: default_environment(),
        environment_id: 1,
        extend_info: Vec::new(),
        driver_config,
        storage_type: "LOCAL".to_owned(),
        space_id: 0,
        support_database: true,
        support_schema,
        has_secret: datasource.has_secret,
        revision: datasource.revision,
    }
}

fn table_response(table: CommunityTable) -> LegacyTable {
    let table_type = if table.table_type.eq_ignore_ascii_case("VIEW") {
        "VIEW"
    } else {
        "TABLE"
    };
    LegacyTable {
        name: table.name,
        comment: table.comment,
        table_type: table_type.to_owned(),
        pinned: table.pinned,
        ddl: table.ddl,
        engine: table.engine,
        charset: table.charset,
        collate: table.collation,
        increment_value: table.increment_value,
        partition: table.partition,
        tablespace: table.tablespace,
        rows: table.rows,
        data_length: table.data_length,
        create_time: table.create_time,
        update_time: table.update_time,
    }
}

fn result_header(column: &ResultColumn) -> LegacyResultHeader {
    LegacyResultHeader {
        data_type: legacy_data_type(column.value_type).to_owned(),
        name: column.label.clone(),
        column_name: column.name.clone(),
        column_type: column.jdbc_type_name.clone(),
        table_name: column.table_name.clone(),
        primary_key: false,
        nullable: column.nullability != ColumnNullability::NoNulls,
        column_size: column.precision.or(column.display_size),
        decimal_digits: column.scale,
    }
}

fn result_cell(value: JdbcValue, column: &ResultColumn) -> LegacyResultCell {
    let (value, value_type) = match value {
        JdbcValue::Null => (None, "UNKNOWN"),
        JdbcValue::Boolean { value } => (Some(value.to_string()), "UNKNOWN"),
        JdbcValue::SignedInteger { value }
        | JdbcValue::UnsignedInteger { value }
        | JdbcValue::Float32 { value }
        | JdbcValue::Float64 { value }
        | JdbcValue::Decimal { value }
        | JdbcValue::Text { value }
        | JdbcValue::Date { value }
        | JdbcValue::Time { value }
        | JdbcValue::Timestamp { value }
        | JdbcValue::TimestampWithTimeZone { value }
        | JdbcValue::Uuid { value } => (Some(value), "UNKNOWN"),
        JdbcValue::Binary { value } => (Some(value), "BINARY"),
        JdbcValue::Json { value } => (Some(value), "JSON"),
        JdbcValue::Opaque { display_value, .. } => (Some(display_value), "UNKNOWN"),
    };
    LegacyResultCell {
        value,
        large_value: false,
        value_type: value_type.to_owned(),
        sql_type: column.jdbc_type,
        column_type: column.jdbc_type_name.clone(),
        truncated: false,
    }
}

const fn legacy_data_type(value_type: JdbcValueType) -> &'static str {
    match value_type {
        JdbcValueType::Boolean => "BOOLEAN",
        JdbcValueType::SignedInteger
        | JdbcValueType::UnsignedInteger
        | JdbcValueType::Float32
        | JdbcValueType::Float64
        | JdbcValueType::Decimal => "NUMERIC",
        JdbcValueType::Binary => "BINARY",
        JdbcValueType::Date
        | JdbcValueType::Time
        | JdbcValueType::Timestamp
        | JdbcValueType::TimestampWithTimeZone => "DATETIME",
        JdbcValueType::Json => "DOCUMENT",
        JdbcValueType::Text | JdbcValueType::Uuid | JdbcValueType::Opaque => "STRING",
    }
}

fn datasource_connection(request: &LegacyDatasourceRequest) -> LegacyResult<DatasourceConnection> {
    let jdbc_url = request.url.trim();
    if jdbc_url.is_empty() {
        return Err(LegacyFailure::invalid(
            "invalid_datasource_request",
            "url is required",
        ));
    }
    let mut keys = HashSet::new();
    let mut properties = Vec::new();
    push_property(&mut properties, &mut keys, "user", &request.user, false);
    push_property(
        &mut properties,
        &mut keys,
        "password",
        &request.password,
        true,
    );
    for property in &request.extend_info {
        let key = property.key.trim();
        if key.is_empty() {
            continue;
        }
        let normalized = key.to_lowercase();
        if keys.insert(normalized) {
            properties.push(DatasourceConnectionProperty {
                key: key.to_owned(),
                value: property.value.clone(),
                sensitive: is_sensitive_property(key),
            });
        }
    }
    Ok(DatasourceConnection {
        jdbc_url: jdbc_url.to_owned(),
        properties,
        read_only: request.read_only,
    })
}

fn push_property(
    properties: &mut Vec<DatasourceConnectionProperty>,
    keys: &mut HashSet<String>,
    key: &str,
    value: &str,
    sensitive: bool,
) {
    if value.is_empty() || !keys.insert(key.to_owned()) {
        return;
    }
    properties.push(DatasourceConnectionProperty {
        key: key.to_owned(),
        value: value.to_owned(),
        sensitive,
    });
}

fn is_sensitive_property(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("password")
        || key.contains("secret")
        || key.contains("token")
        || key.contains("credential")
        || key.contains("keyfile")
}

fn datasource_name(request: &LegacyDatasourceRequest) -> String {
    if request.alias.trim().is_empty() {
        let database_type = normalize_database_type(&request.database_type);
        if database_type.is_empty() {
            "Datasource".to_owned()
        } else {
            database_type
        }
    } else {
        request.alias.trim().to_owned()
    }
}

fn has_driver_selection(request: &LegacyDatasourceRequest) -> bool {
    request
        .driver_config
        .as_ref()
        .is_some_and(|config| !config.jdbc_driver.trim().is_empty())
        || !request.driver.trim().is_empty()
        || !request.jdbc.trim().is_empty()
        || !request.database_type.trim().is_empty()
}

fn resolve_driver_id(
    application: &Application,
    request: &LegacyDatasourceRequest,
) -> LegacyResult<String> {
    let inventory = application.list_drivers().items;
    let selected = request
        .driver_config
        .as_ref()
        .map(|config| config.jdbc_driver.trim())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let value = request.driver.trim();
            (!value.is_empty()).then_some(value)
        })
        .or_else(|| {
            let value = request.jdbc.trim();
            (!value.is_empty()).then_some(value)
        });
    if let Some(selected) = selected
        && (inventory.is_empty() || inventory.iter().any(|driver| driver.driver_id == selected))
    {
        return Ok(selected.to_owned());
    }
    let database_type = normalize_database_type(&request.database_type);
    if let Some(driver) = inventory
        .iter()
        .find(|driver| database_type_for_driver(driver) == database_type)
    {
        return Ok(driver.driver_id.clone());
    }
    if inventory.is_empty() && !database_type.is_empty() {
        return Ok(database_type.to_ascii_lowercase());
    }
    Err(LegacyFailure::invalid(
        "jdbc_driver_not_found",
        "No installed JDBC driver matches the selected database type",
    ))
}

async fn resolve_database_type(
    application: &Application,
    datasource_id: &str,
    explicit: &str,
) -> LegacyResult<String> {
    let explicit = normalize_database_type(explicit);
    if !explicit.is_empty() {
        return Ok(explicit);
    }
    let datasource = application.get_datasource(datasource_id).await?;
    Ok(application
        .list_drivers()
        .items
        .iter()
        .find(|driver| driver.driver_id == datasource.driver_id)
        .map_or_else(
            || normalize_database_type(&datasource.driver_id),
            database_type_for_driver,
        ))
}

fn database_type_for_driver(driver: &JdbcDriver) -> String {
    let identity =
        format!("{} {} {}", driver.pack_id, driver.name, driver.driver_class).to_ascii_lowercase();
    if identity.contains("mysql") {
        "MYSQL".to_owned()
    } else if identity.contains("postgres") {
        "POSTGRESQL".to_owned()
    } else if identity.contains("sqlite") {
        "SQLITE".to_owned()
    } else if identity.contains("h2") {
        "H2".to_owned()
    } else {
        normalize_database_type(&driver.pack_id)
    }
}

fn normalize_database_type(value: &str) -> String {
    let normalized = value.trim().replace(['-', ' '], "_").to_ascii_uppercase();
    if normalized.contains("MYSQL") {
        "MYSQL".to_owned()
    } else if normalized.contains("POSTGRES") {
        "POSTGRESQL".to_owned()
    } else {
        normalized
    }
}

fn paginate<T>(items: Vec<T>, page_no: u32, page_size: u32) -> LegacyPage<T> {
    let page_no = page_no.max(1);
    let page_size = page_size.max(1);
    let total = items.len();
    let start = usize::try_from(page_no.saturating_sub(1))
        .unwrap_or(usize::MAX)
        .saturating_mul(usize::try_from(page_size).unwrap_or(usize::MAX));
    let page_items: Vec<T> = items
        .into_iter()
        .skip(start)
        .take(usize::try_from(page_size).unwrap_or(usize::MAX))
        .collect();
    LegacyPage {
        has_next_page: start.saturating_add(page_items.len()) < total,
        data: page_items,
        page_no,
        page_size,
        total,
    }
}

/// Dispatches a historical Community request without depending on Axum.
///
/// Tauri IPC can pass its `requestUrl`, `method`, and `message` fields here and
/// return the resulting JSON value unchanged.
pub async fn dispatch(
    application: &Application,
    request: LegacyDispatchRequest,
) -> serde_json::Value {
    let path = request
        .request_url
        .split('?')
        .next()
        .unwrap_or(request.request_url.as_str());
    let method = request.method.to_ascii_lowercase();
    let result = match (method.as_str(), path) {
        ("get", "/api/system") => Ok(serde_json::json!({
            "systemUuid": "chat2db-rust-community"
        })),
        ("get", "/api/common/environment/list_all") => serialized(Ok(environments())),
        ("get", "/api/jdbc/driver/list") => decode(request.message)
            .map(|query: LegacyDriverQuery| drivers(application, &query.db_type))
            .and_then(serialize_data),
        ("get", "/api/connection/datasource/list") => {
            match decode::<LegacyPageQuery>(request.message) {
                Ok(query) => serialized(list_datasources(application, &query).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/connection/datasource") => match decode::<LegacyIdQuery>(request.message) {
            Ok(query) => serialized(get_datasource(application, &query.id).await),
            Err(error) => Err(error),
        },
        ("post", "/api/connection/datasource/create") => {
            match decode::<LegacyDatasourceRequest>(request.message) {
                Ok(body) => serialized(create_datasource(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/connection/datasource/pre_connect") => {
            match decode::<LegacyDatasourceRequest>(request.message) {
                Ok(body) => serialized(pre_connect(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post" | "put", "/api/connection/datasource/update") => {
            match decode::<LegacyDatasourceRequest>(request.message) {
                Ok(body) => serialized(update_datasource(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("delete", "/api/connection/datasource") => {
            match decode::<LegacyIdQuery>(request.message) {
                Ok(query) => serialized(delete_datasource(application, &query.id).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/namespaces/tree_list") => serialized(namespace_tree(application).await),
        ("get", "/api/rdb/database/list") => match decode::<LegacyMetadataQuery>(request.message) {
            Ok(query) => serialized(list_databases(application, &query).await),
            Err(error) => Err(error),
        },
        ("get", "/api/rdb/schema/list") => match decode::<LegacyMetadataQuery>(request.message) {
            Ok(query) => serialized(list_schemas(application, &query).await),
            Err(error) => Err(error),
        },
        ("get", "/api/rdb/table/list") => match decode::<LegacyTableListQuery>(request.message) {
            Ok(query) => serialized(list_tables(application, &query).await),
            Err(error) => Err(error),
        },
        ("post" | "put", "/api/rdb/dml/execute_table") => {
            match decode::<LegacyTablePreviewRequest>(request.message) {
                Ok(body) => serialized(preview_table(application, &body).await),
                Err(error) => Err(error),
            }
        }
        (_, known_path) if LEGACY_PATHS.contains(&known_path) => Err(LegacyFailure::invalid(
            "method_not_allowed",
            "The request method is not supported for this route",
        )),
        _ => Err(LegacyFailure::invalid(
            "route_not_found",
            "The requested route does not exist",
        )),
    };
    envelope_value(result)
}

const LEGACY_PATHS: &[&str] = &[
    "/api/system",
    "/api/common/environment/list_all",
    "/api/jdbc/driver/list",
    "/api/connection/datasource/list",
    "/api/connection/datasource",
    "/api/connection/datasource/create",
    "/api/connection/datasource/pre_connect",
    "/api/connection/datasource/update",
    "/api/namespaces/tree_list",
    "/api/rdb/database/list",
    "/api/rdb/schema/list",
    "/api/rdb/table/list",
    "/api/rdb/dml/execute_table",
];

fn decode<T: DeserializeOwned>(message: serde_json::Value) -> LegacyResult<T> {
    serde_json::from_value(message).map_err(|_| {
        LegacyFailure::invalid(
            "invalid_legacy_request",
            "The Community request payload is invalid",
        )
    })
}

fn serialized<T: Serialize>(result: LegacyResult<T>) -> LegacyResult<serde_json::Value> {
    result.and_then(serialize_data)
}

fn serialize_data<T: Serialize>(data: T) -> LegacyResult<serde_json::Value> {
    serde_json::to_value(data).map_err(|_| LegacyFailure {
        code: "internal_error".to_owned(),
        message: "The operation could not be completed".to_owned(),
    })
}

fn envelope_value(result: LegacyResult<serde_json::Value>) -> serde_json::Value {
    let envelope = match result {
        Ok(data) => LegacyEnvelope::success(data),
        Err(error) => LegacyEnvelope::failure(error),
    };
    serde_json::to_value(envelope).unwrap_or_else(|_| {
        serde_json::json!({
            "success": false,
            "data": null,
            "errorCode": "internal_error",
            "errorMessage": "The operation could not be completed"
        })
    })
}

pub(crate) fn routes() -> Router<Application> {
    Router::new()
        .route("/api/system", get(system_handler))
        .route("/api/common/environment/list_all", get(environment_handler))
        .route("/api/jdbc/driver/list", get(driver_handler))
        .route(
            "/api/connection/datasource/list",
            get(list_datasources_handler),
        )
        .route(
            "/api/connection/datasource",
            get(get_datasource_handler).delete(delete_datasource_handler),
        )
        .route(
            "/api/connection/datasource/create",
            post(create_datasource_handler),
        )
        .route(
            "/api/connection/datasource/pre_connect",
            post(pre_connect_handler),
        )
        .route(
            "/api/connection/datasource/update",
            post(update_datasource_handler).put(update_datasource_handler),
        )
        .route("/api/namespaces/tree_list", get(namespace_tree_handler))
        .route("/api/rdb/database/list", get(database_list_handler))
        .route("/api/rdb/schema/list", get(schema_list_handler))
        .route("/api/rdb/table/list", get(table_list_handler))
        .route(
            "/api/rdb/dml/execute_table",
            post(table_preview_handler).put(table_preview_handler),
        )
}

fn envelope<T>(result: LegacyResult<T>) -> Json<LegacyEnvelope<T>> {
    Json(match result {
        Ok(data) => LegacyEnvelope::success(data),
        Err(error) => LegacyEnvelope::failure(error),
    })
}

async fn system_handler() -> Json<LegacyEnvelope<serde_json::Value>> {
    envelope(Ok(serde_json::json!({
        "systemUuid": "chat2db-rust-community"
    })))
}

async fn environment_handler() -> Json<LegacyEnvelope<Vec<LegacyEnvironment>>> {
    envelope(Ok(environments()))
}

async fn driver_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyDriverQuery>,
) -> Json<LegacyEnvelope<LegacyDriverResponse>> {
    envelope(Ok(drivers(&application, &query.db_type)))
}

async fn list_datasources_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyPageQuery>,
) -> Json<LegacyEnvelope<LegacyPage<LegacyDatasourceResponse>>> {
    envelope(list_datasources(&application, &query).await)
}

async fn get_datasource_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyIdQuery>,
) -> Json<LegacyEnvelope<LegacyDatasourceResponse>> {
    envelope(get_datasource(&application, &query.id).await)
}

async fn create_datasource_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyDatasourceRequest>,
) -> Json<LegacyEnvelope<LegacyDatasourceResponse>> {
    envelope(create_datasource(&application, &request).await)
}

async fn pre_connect_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyDatasourceRequest>,
) -> Json<LegacyEnvelope<bool>> {
    envelope(pre_connect(&application, &request).await)
}

async fn update_datasource_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyDatasourceRequest>,
) -> Json<LegacyEnvelope<LegacyDatasourceResponse>> {
    envelope(update_datasource(&application, &request).await)
}

async fn delete_datasource_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyIdQuery>,
) -> Json<LegacyEnvelope<()>> {
    envelope(delete_datasource(&application, &query.id).await)
}

async fn namespace_tree_handler(
    State(application): State<Application>,
) -> Json<LegacyEnvelope<Vec<LegacyNamespaceNode>>> {
    envelope(namespace_tree(&application).await)
}

async fn database_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyMetadataQuery>,
) -> Json<LegacyEnvelope<Vec<LegacyDatabase>>> {
    envelope(list_databases(&application, &query).await)
}

async fn schema_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyMetadataQuery>,
) -> Json<LegacyEnvelope<Vec<LegacySchema>>> {
    envelope(list_schemas(&application, &query).await)
}

async fn table_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableListQuery>,
) -> Json<LegacyEnvelope<LegacyPage<LegacyTable>>> {
    envelope(list_tables(&application, &query).await)
}

async fn table_preview_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyTablePreviewRequest>,
) -> Json<LegacyEnvelope<Vec<LegacyManageResult>>> {
    envelope(preview_table(&application, &request).await)
}
