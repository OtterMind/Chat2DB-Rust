//! Compatibility adapter for the retained Community frontend HTTP contract.
//!
//! The functions in this module are transport-neutral apart from the thin
//! Axum handlers at the bottom. Desktop IPC can reuse the same request and
//! response translations without duplicating datasource or JDBC behavior.

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    future::Future,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Query, State},
    http::{StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use chat2db_contract::{
    ApiError, ColumnNullability, CommunityFunction, CommunityProcedure, CommunityTable,
    CommunityTableColumn, CommunityTableIndex, CommunityTableIndexColumn, CommunityTrigger,
    Datasource, DatasourceConnection, DatasourceConnectionProperty, DatasourceSecretChange,
    GetCommunityFunctionRequest, GetCommunityProcedureRequest, GetCommunityTriggerRequest,
    JdbcDriver, JdbcValue, JdbcValueType, ListCommunityColumnsRequest,
    ListCommunityDatabasesRequest, ListCommunityFunctionsRequest, ListCommunityIndexesRequest,
    ListCommunityProceduresRequest, ListCommunitySchemasRequest, ListCommunityTablesRequest,
    ListCommunityTriggersRequest, ListCommunityViewsRequest, OperationEvent, QueryAccepted,
    QueryLimits, ResultColumn, ResultMetadata, ResultPageRequest,
    StartCommunityTablePreviewRequest, StartQueryRequest, UpdateDatasourceRequest,
};
use chat2db_core::{
    AppError, Application, LargeValueChunk, LargeValueEncoding, LargeValuePreview, LargeValueType,
    MysqlConsoleCancellation, MysqlConsoleRequest, MysqlConsoleResult,
    mysql_ddl::{
        MysqlColumnAlter, MysqlColumnDefinition, MysqlColumnPosition, MysqlDatabaseDefinition,
        MysqlIndexAlter, MysqlIndexColumn, MysqlIndexDefinition, MysqlIndexKind, MysqlIndexMethod,
        MysqlQualifiedName, MysqlResultGridCopyOperation, MysqlResultGridCopyOperationType,
        MysqlResultGridHeader, MysqlResultGridOperation, MysqlResultGridOperationType,
        MysqlSortOrder, MysqlTableAlter, MysqlTableCopy, MysqlTableDefinition,
        MysqlTableEditorMeta, MysqlViewAlgorithm, MysqlViewCheckOption, MysqlViewDefiner,
        MysqlViewDefinition, MysqlViewSecurity, build_mysql_alter_table, build_mysql_copy_table,
        build_mysql_count_query, build_mysql_create_database, build_mysql_create_schema,
        build_mysql_create_table, build_mysql_create_view, build_mysql_drop_database,
        build_mysql_drop_schema, build_mysql_drop_table, build_mysql_drop_view,
        build_mysql_external_in_values, build_mysql_result_grid_copy_sql,
        build_mysql_result_grid_in_values, build_mysql_result_grid_script,
        build_mysql_truncate_table, mysql_table_editor_meta,
    },
};
use chat2db_storage::{
    CreateOperationLog, CreateSavedConsole, OperationLogListQuery, OperationLogRecord,
    SavedConsoleListQuery, SavedConsoleRecord, Storage, StorageError, UpdateSavedConsole,
};
use chrono::{Local, TimeZone as _};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};

const DEFAULT_PAGE_NO: u32 = 1;
const DEFAULT_PAGE_SIZE: u32 = 20;
const DEFAULT_PREVIEW_PAGE_SIZE: u32 = 200;
const DEFAULT_SQL_PAGE_SIZE: u32 = 200;
const MAX_METADATA_PAGE_SIZE: u32 = 100_000;
const MAX_PREVIEW_ROWS: u32 = 1_000;
const MAX_SQL_ROWS: u32 = 10_000;
const LARGE_VALUE_CHUNK_SIZE: u32 = 256 * 1024;
const LARGE_VALUE_FALLBACK_PREVIEW_BYTES: usize = 64 * 1024;
const RESULT_PAGE_MAX_BYTES: u64 = 8 * 1024 * 1024;
const LARGE_VALUE_PREVIEW_PREFIX: &str = "CHAT2DB_LARGE_VALUE_PREVIEW:";
const PREVIEW_TIMEOUT: Duration = Duration::from_secs(30);
const SQL_EXECUTION_TIMEOUT: Duration = Duration::from_secs(60);
const SQL_CANCELLATION_GRACE: Duration = Duration::from_secs(10);

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

/// Historical list wrapper used by metadata screens that request the complete
/// response instead of only its `data` field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyCountedEnvelope<T> {
    pub success: bool,
    pub data: Option<Vec<T>>,
    pub total: Option<usize>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

impl<T> LegacyCountedEnvelope<T> {
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

    fn failure(error: LegacyFailure) -> Self {
        Self {
            success: false,
            data: None,
            total: None,
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
pub struct LegacySimpleTable {
    pub name: String,
    pub comment: String,
    pub table_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LegacyColumn {
    pub old_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub table_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub column_type: String,
    pub data_type: Option<i32>,
    pub default_value: Option<String>,
    pub auto_increment: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub comment: String,
    pub primary_key: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub primary_key_name: String,
    #[serde(default, deserialize_with = "deserialize_i32_or_default")]
    pub primary_key_order: i32,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub schema_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_name: String,
    pub type_name: Option<String>,
    pub column_size: Option<i32>,
    pub buffer_length: Option<i32>,
    pub decimal_digits: Option<i32>,
    pub num_prec_radix: Option<i32>,
    pub nullable_int: Option<i32>,
    pub sql_data_type: Option<i32>,
    pub sql_datetime_sub: Option<i32>,
    pub char_octet_length: Option<i32>,
    pub ordinal_position: Option<i32>,
    pub nullable: Option<i32>,
    pub generated_column: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub extent: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub char_set_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub collation_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub value: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub unit: String,
    pub sparse: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub default_constraint_name: String,
    pub seed: Option<i32>,
    pub increment: Option<i32>,
    pub on_update_current_timestamp: Option<bool>,
    pub edit_status: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LegacyIndexColumn {
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub index_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub table_name: String,
    #[serde(
        rename = "type",
        default,
        deserialize_with = "deserialize_string_or_default"
    )]
    pub index_type: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub comment: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub column_name: String,
    pub ordinal_position: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub collation: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub schema_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_name: String,
    pub non_unique: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub index_qualifier: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub asc_or_desc: String,
    pub cardinality: Option<i64>,
    pub pages: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub filter_condition: String,
    pub sub_part: Option<i64>,
    pub edit_status: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LegacyIndex {
    pub columns: Option<String>,
    pub old_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub table_name: String,
    #[serde(
        rename = "type",
        default,
        deserialize_with = "deserialize_string_or_default"
    )]
    pub index_type: String,
    pub unique: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub comment: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub schema_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_name: String,
    #[serde(default, deserialize_with = "deserialize_vec_or_default")]
    pub column_list: Vec<LegacyIndexColumn>,
    pub edit_status: Option<String>,
    pub concurrently: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub method: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub foreign_schema_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub foreign_table_name: String,
    #[serde(default, deserialize_with = "deserialize_vec_or_default")]
    pub foreign_column_namelist: Vec<String>,
}

/// Full table projection consumed by Community's retained table editor.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LegacyEditableTable {
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub comment: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub schema_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_name: String,
    #[serde(
        rename = "type",
        default,
        deserialize_with = "deserialize_string_or_default"
    )]
    pub table_type: String,
    #[serde(default, deserialize_with = "deserialize_vec_or_default")]
    pub column_list: Vec<LegacyColumn>,
    #[serde(default, deserialize_with = "deserialize_vec_or_default")]
    pub index_list: Vec<LegacyIndex>,
    #[serde(default, deserialize_with = "deserialize_vec_or_default")]
    pub foreign_key_list: Vec<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub db_type: String,
    pub pinned: bool,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub ddl: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub engine: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub charset: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub collate: String,
    pub increment_value: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub partition: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub tablespace: String,
    pub rows: Option<String>,
    pub data_length: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub create_time: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub update_time: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyTableModifyRequest {
    pub data_source_id: LegacyIdentifier,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub schema_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_type: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub table_name: String,
    #[serde(default)]
    pub old_table: Option<LegacyEditableTable>,
    pub new_table: LegacyEditableTable,
}

/// Historical `{ sql }` object returned by DDL-preview routes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySqlResponse {
    pub sql: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyGridHeaderRequest {
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub column_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub column_type: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub data_type: String,
    #[serde(default, deserialize_with = "deserialize_boolish_or_default")]
    pub primary_key: bool,
    #[serde(default, deserialize_with = "deserialize_boolish_or_default")]
    pub auto_increment: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyGridOperationRequest {
    #[serde(
        rename = "type",
        default,
        deserialize_with = "deserialize_string_or_default"
    )]
    pub operation_type: String,
    #[serde(default, deserialize_with = "deserialize_optional_string_vec")]
    pub data_list: Vec<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_string_vec")]
    pub old_data_list: Vec<Option<String>>,
    #[serde(default)]
    pub select_cols: Vec<usize>,
    #[serde(default)]
    pub selected_cell: Option<LegacyResultCell>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyGridUpdateRequest {
    pub data_source_id: LegacyIdentifier,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub schema_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_type: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub table_name: String,
    #[serde(default)]
    pub console_id: Option<LegacyIdentifier>,
    #[serde(default)]
    pub header_list: Vec<LegacyGridHeaderRequest>,
    #[serde(default)]
    pub operations: Vec<LegacyGridOperationRequest>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub source_type: String,
    #[serde(default)]
    pub external_values: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyTableOperationRequest {
    pub data_source_id: LegacyIdentifier,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub schema_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_type: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub table_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyTableCopyRequest {
    pub data_source_id: LegacyIdentifier,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub schema_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_type: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub table_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub new_name: String,
    #[serde(default)]
    pub copy_data: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyDatabaseDefinitionRequest {
    pub data_source_id: LegacyIdentifier,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_type: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub charset: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub collation: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySchemaDefinitionRequest {
    pub data_source_id: LegacyIdentifier,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_type: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub schema_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyDeleteObjectRequest {
    pub data_source_id: LegacyIdentifier,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_type: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub schema_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub confirm_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyDeletePrepareResponse {
    pub confirm_name: String,
    pub sql_preview: String,
    pub object_type: String,
    pub db_type: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyViewOperationRequest {
    pub data_source_id: LegacyIdentifier,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_type: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub schema_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub table_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub view_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub view_body: String,
    #[serde(default)]
    pub view_attributes: Vec<String>,
    #[serde(default)]
    pub use_or_replace: bool,
    #[serde(default)]
    pub use_if_not_exists: bool,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub algorithm: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub definer: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub security: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub check_option: String,
    #[serde(default)]
    pub is_modify: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyViewMetaResponse {
    pub configurations: Vec<serde_json::Value>,
    pub sql: String,
    pub preview_sql: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyFunction {
    pub database_name: String,
    pub schema_name: String,
    pub function_name: String,
    pub remarks: String,
    pub function_type: Option<i32>,
    pub specific_name: String,
    pub function_body: String,
    pub function_template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyProcedure {
    pub database_name: String,
    pub schema_name: String,
    pub procedure_name: String,
    pub remarks: String,
    pub procedure_type: Option<i32>,
    pub specific_name: String,
    pub procedure_body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyTrigger {
    pub database_name: String,
    pub schema_name: String,
    pub trigger_name: String,
    pub event_manipulation: String,
    pub trigger_body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyResultHeader {
    pub data_type: String,
    pub name: String,
    pub column_name: String,
    pub column_type: String,
    pub table_name: Option<String>,
    pub database_name: Option<String>,
    pub schema_name: Option<String>,
    pub primary_key: bool,
    pub comment: Option<String>,
    pub default_value: Option<String>,
    pub auto_increment: Option<i32>,
    pub nullable: bool,
    pub column_size: Option<u32>,
    pub decimal_digits: Option<i32>,
    pub editor_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyResultCell {
    pub value: Option<String>,
    pub large_value: bool,
    pub large_value_id: Option<String>,
    pub value_type: String,
    pub sql_type: i32,
    pub column_type: String,
    pub size_bytes: Option<u64>,
    pub size_chars: Option<u64>,
    pub loaded_bytes: Option<u64>,
    pub loaded_chars: Option<u64>,
    pub truncated: bool,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyLargeCellValueRequest {
    pub large_value_id: String,
    #[serde(default)]
    pub offset: u64,
    #[serde(default = "default_large_value_chunk_size")]
    pub limit: u32,
    #[serde(default = "default_large_value_format")]
    pub format: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyLargeCellDownloadRequest {
    pub large_value_id: String,
    #[serde(default = "default_large_value_download_format")]
    pub format: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyLargeCellChunk {
    pub value: String,
    pub offset: u64,
    pub next_offset: u64,
    pub eof: bool,
    pub size_bytes: u64,
    pub size_chars: Option<u64>,
    pub encoding: String,
    pub content_type: String,
    pub display_mode: LargeValueType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyExecutionMetrics {
    pub started_at_epoch_ms: u64,
    pub finished_at_epoch_ms: u64,
    pub total_duration_ms: u64,
    pub execute_duration_ms: u64,
    pub fetch_duration_ms: u64,
    pub fetched_row_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyExecutionContext {
    pub database_name: Option<String>,
    pub schema_name: Option<String>,
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
    pub execute_sql_params: LegacySqlExecuteRequest,
    pub extra: serde_json::Value,
    pub comment: Option<String>,
    pub result_set_id: Option<u32>,
    pub statement_sequence: Option<u32>,
    pub execution_metrics: Option<LegacyExecutionMetrics>,
    pub execution_context: Option<LegacyExecutionContext>,
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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySavedConsoleCreateRequest {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub name: String,
    #[serde(default)]
    pub data_source_id: Option<LegacyIdentifier>,
    #[serde(default)]
    pub data_source_name: Option<String>,
    #[serde(default)]
    pub database_name: Option<String>,
    #[serde(default)]
    pub schema_name: Option<String>,
    #[serde(rename = "type", default)]
    pub database_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub ddl: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub status: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub tab_opened: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub operation_type: String,
}

#[derive(Debug, Clone, Default)]
pub enum LegacyPatch<T> {
    #[default]
    Unset,
    Set(Option<T>),
}

impl<'de, T> Deserialize<'de> for LegacyPatch<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<T>::deserialize(deserializer).map(Self::Set)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySavedConsoleUpdateRequest {
    pub id: i64,
    #[serde(default)]
    pub name: LegacyPatch<String>,
    #[serde(default)]
    pub data_source_id: LegacyPatch<LegacyIdentifier>,
    #[serde(default)]
    pub data_source_name: LegacyPatch<String>,
    #[serde(default)]
    pub database_name: LegacyPatch<String>,
    #[serde(default)]
    pub schema_name: LegacyPatch<String>,
    #[serde(rename = "type", default)]
    pub database_type: LegacyPatch<String>,
    #[serde(default)]
    pub ddl: LegacyPatch<String>,
    #[serde(default)]
    pub status: LegacyPatch<String>,
    #[serde(default)]
    pub tab_opened: LegacyPatch<String>,
    #[serde(default)]
    pub operation_type: LegacyPatch<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySavedConsoleListQuery {
    #[serde(default = "default_page_no")]
    pub page_no: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    #[serde(default)]
    pub data_source_id: Option<LegacyIdentifier>,
    #[serde(default)]
    pub database_name: Option<String>,
    #[serde(default)]
    pub schema_name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub tab_opened: Option<String>,
    #[serde(default)]
    pub operation_type: Option<String>,
    #[serde(default)]
    pub search_key: Option<String>,
    #[serde(default)]
    pub order_by_desc: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySavedConsoleResponse {
    pub id: i64,
    pub name: String,
    pub data_source_id: Option<String>,
    pub data_source_name: Option<String>,
    pub connectable: bool,
    pub database_name: Option<String>,
    pub schema_name: Option<String>,
    #[serde(rename = "type")]
    pub database_type: Option<String>,
    pub ddl: String,
    pub status: String,
    pub tab_opened: String,
    pub operation_type: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyOperationLogCreateRequest {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub name: String,
    #[serde(default)]
    pub data_source_id: Option<LegacyIdentifier>,
    #[serde(default)]
    pub data_source_name: Option<String>,
    #[serde(default)]
    pub connectable: Option<bool>,
    #[serde(default)]
    pub database_name: Option<String>,
    #[serde(rename = "type", default)]
    pub database_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub ddl: String,
    #[serde(default)]
    pub more: bool,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub operation_rows: Option<u64>,
    #[serde(default)]
    pub use_time: Option<u64>,
    #[serde(default)]
    pub extend_info: Option<String>,
    #[serde(default)]
    pub schema_name: Option<String>,
    #[serde(default)]
    pub organization_id: Option<i64>,
    #[serde(default)]
    pub user_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyOperationLogListQuery {
    #[serde(default = "default_page_no")]
    pub page_no: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub search_key: String,
    #[serde(default)]
    pub data_source_id: Option<LegacyIdentifier>,
    #[serde(default)]
    pub database_name: Option<String>,
    #[serde(default)]
    pub schema_name: Option<String>,
    #[serde(default)]
    pub organization_id: Option<i64>,
    #[serde(default)]
    pub operation_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyOperationLogResponse {
    pub id: i64,
    pub gmt_create: String,
    pub gmt_modified: String,
    pub name: String,
    pub data_source_id: Option<String>,
    pub data_source_name: Option<String>,
    pub connectable: Option<bool>,
    pub database_name: Option<String>,
    #[serde(rename = "type")]
    pub database_type: Option<String>,
    pub ddl: String,
    pub more: bool,
    pub status: Option<String>,
    pub operation_rows: Option<u64>,
    pub use_time: Option<u64>,
    pub extend_info: Option<String>,
    pub schema_name: Option<String>,
    pub organization_id: Option<i64>,
    pub user_name: Option<String>,
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
pub struct LegacyTableDdlExampleQuery {
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyTableDetailQuery {
    pub data_source_id: LegacyIdentifier,
    #[serde(default)]
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
    #[serde(default)]
    pub database_type: String,
    #[serde(default)]
    pub table_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyFunctionDetailQuery {
    pub data_source_id: LegacyIdentifier,
    #[serde(default)]
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
    #[serde(default)]
    pub database_type: String,
    #[serde(default)]
    pub function_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyProcedureDetailQuery {
    pub data_source_id: LegacyIdentifier,
    #[serde(default)]
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
    #[serde(default)]
    pub database_type: String,
    #[serde(default)]
    pub procedure_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyTriggerDetailQuery {
    pub data_source_id: LegacyIdentifier,
    #[serde(default)]
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
    #[serde(default)]
    pub database_type: String,
    #[serde(default)]
    pub trigger_name: String,
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

/// Community's synchronous SQL execution payload. Extra frontend fields are
/// intentionally ignored by Serde so this remains compatible across pinned UI
/// revisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySqlExecuteRequest {
    pub data_source_id: LegacyIdentifier,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub data_source_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub schema_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub database_type: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub table_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_default")]
    pub sql: String,
    #[serde(default)]
    pub single: bool,
    #[serde(default = "default_page_no")]
    pub page_no: u32,
    #[serde(default = "default_sql_page_size")]
    pub page_size: u32,
    #[serde(default)]
    pub page_size_all: bool,
    #[serde(default)]
    pub console_id: Option<LegacyIdentifier>,
    #[serde(default)]
    pub apply_id: Option<LegacyIdentifier>,
    #[serde(default)]
    pub result_set_id: Option<u32>,
    #[serde(default)]
    pub error_continue: Option<bool>,
    #[serde(default)]
    pub explain: bool,
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

fn deserialize_i32_or_default<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<i32>::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_vec_or_default<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_boolish_or_default<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Bool(value)) => value,
        Some(serde_json::Value::Number(value)) => value.as_i64().is_some_and(|value| value != 0),
        Some(serde_json::Value::String(value)) => {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        }
        None
        | Some(
            serde_json::Value::Null | serde_json::Value::Array(_) | serde_json::Value::Object(_),
        ) => false,
    })
}

fn deserialize_optional_string_vec<'de, D>(deserializer: D) -> Result<Vec<Option<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Vec<serde_json::Value>>::deserialize(deserializer)?
        .unwrap_or_default()
        .into_iter()
        .map(|value| match value {
            serde_json::Value::Null => Ok(None),
            serde_json::Value::String(value) => Ok(Some(value)),
            serde_json::Value::Bool(value) => Ok(Some(value.to_string())),
            serde_json::Value::Number(value) => Ok(Some(value.to_string())),
            _ => Err(serde::de::Error::custom(
                "grid values must be strings, numbers, booleans, or null",
            )),
        })
        .collect()
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

fn default_sql_page_size() -> u32 {
    DEFAULT_SQL_PAGE_SIZE
}

const fn default_large_value_chunk_size() -> u32 {
    LARGE_VALUE_CHUNK_SIZE
}

fn default_large_value_format() -> String {
    "base64".to_owned()
}

fn default_large_value_download_format() -> String {
    "raw".to_owned()
}

impl From<&LegacyTablePreviewRequest> for LegacySqlExecuteRequest {
    fn from(request: &LegacyTablePreviewRequest) -> Self {
        Self {
            data_source_id: request.data_source_id.clone(),
            data_source_name: String::new(),
            database_name: request.database_name.clone(),
            schema_name: request.schema_name.clone(),
            database_type: request.database_type.clone(),
            table_name: request.table_name.clone(),
            sql: request.sql.clone(),
            single: true,
            page_no: request.page_no,
            page_size: request.page_size,
            page_size_all: false,
            console_id: None,
            apply_id: None,
            result_set_id: None,
            error_continue: None,
            explain: false,
        }
    }
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

pub(crate) async fn create_saved_console(
    application: &Application,
    request: &LegacySavedConsoleCreateRequest,
) -> LegacyResult<i64> {
    let storage = legacy_storage(application)?;
    let input = CreateSavedConsole {
        id: request.id,
        name: request.name.clone(),
        data_source_id: request
            .data_source_id
            .as_ref()
            .map(LegacyIdentifier::as_string),
        data_source_name: request.data_source_name.clone(),
        database_name: request.database_name.clone(),
        schema_name: request.schema_name.clone(),
        database_type: request.database_type.clone(),
        ddl: request.ddl.clone(),
        status: default_if_blank(&request.status, "DRAFT"),
        // Community always opens a newly created Console.
        tab_opened: "y".to_owned(),
        operation_type: default_if_blank(&request.operation_type, "console"),
    };
    legacy_storage_call(move || storage.create_saved_console(input))
        .await
        .map(|record| record.id)
}

pub(crate) async fn get_saved_console(
    application: &Application,
    id: i64,
) -> LegacyResult<Option<LegacySavedConsoleResponse>> {
    let storage = legacy_storage(application)?;
    legacy_storage_call(move || storage.get_saved_console(id))
        .await
        .map(|record| record.map(saved_console_response))
}

pub(crate) async fn list_saved_consoles(
    application: &Application,
    query: &LegacySavedConsoleListQuery,
) -> LegacyResult<LegacyPage<LegacySavedConsoleResponse>> {
    let storage = legacy_storage(application)?;
    let storage_query = SavedConsoleListQuery {
        data_source_id: query
            .data_source_id
            .as_ref()
            .map(LegacyIdentifier::as_string),
        database_name: query.database_name.clone(),
        schema_name: query.schema_name.clone(),
        status: query.status.clone(),
        tab_opened: query.tab_opened.clone(),
        operation_type: query.operation_type.clone(),
        search_key: query.search_key.clone(),
        page_no: query.page_no,
        page_size: query.page_size,
        order_by_desc: query.order_by_desc,
    };
    legacy_storage_call(move || storage.list_saved_consoles(&storage_query))
        .await
        .map(|page| {
            let total = usize::try_from(page.total).unwrap_or(usize::MAX);
            let data = page
                .records
                .into_iter()
                .map(saved_console_response)
                .collect::<Vec<_>>();
            LegacyPage {
                has_next_page: u64::from(page.page_no) * u64::from(page.page_size) < page.total,
                data,
                page_no: page.page_no,
                page_size: page.page_size,
                total,
            }
        })
}

pub(crate) async fn update_saved_console(
    application: &Application,
    request: LegacySavedConsoleUpdateRequest,
) -> LegacyResult<()> {
    let storage = legacy_storage(application)?;
    let input = UpdateSavedConsole {
        name: required_string_patch(request.name),
        data_source_id: identifier_patch(request.data_source_id),
        data_source_name: nullable_string_patch(request.data_source_name),
        database_name: nullable_string_patch(request.database_name),
        schema_name: nullable_string_patch(request.schema_name),
        database_type: nullable_string_patch(request.database_type),
        ddl: required_string_patch(request.ddl),
        status: required_string_patch(request.status),
        tab_opened: required_string_patch(request.tab_opened),
        operation_type: required_string_patch(request.operation_type),
    };
    legacy_storage_call(move || storage.update_saved_console(request.id, input))
        .await
        .map(|_| ())
}

pub(crate) async fn delete_saved_console(application: &Application, id: i64) -> LegacyResult<()> {
    let storage = legacy_storage(application)?;
    legacy_storage_call(move || storage.delete_saved_console(id))
        .await
        .map(|_| ())
}

pub(crate) async fn create_operation_log(
    application: &Application,
    request: &LegacyOperationLogCreateRequest,
) -> LegacyResult<i64> {
    if request.ddl.trim().is_empty() {
        return Err(LegacyFailure::invalid(
            "invalid_operation_log",
            "ddl is required",
        ));
    }
    let operation_rows = request
        .operation_rows
        .map(i64::try_from)
        .transpose()
        .map_err(|_| {
            LegacyFailure::invalid(
                "invalid_operation_log",
                "operationRows is outside the supported range",
            )
        })?;
    let use_time = request
        .use_time
        .map(i64::try_from)
        .transpose()
        .map_err(|_| {
            LegacyFailure::invalid(
                "invalid_operation_log",
                "useTime is outside the supported range",
            )
        })?;
    let data_source_id = request
        .data_source_id
        .as_ref()
        .map(LegacyIdentifier::as_string);
    let storage = legacy_storage(application)?;
    let input = CreateOperationLog {
        name: non_blank(&request.name),
        connectable: request
            .connectable
            .or_else(|| data_source_id.as_ref().map(|id| !id.trim().is_empty())),
        data_source_id,
        data_source_name: request.data_source_name.clone(),
        database_name: request.database_name.clone(),
        database_type: request.database_type.clone(),
        ddl: request.ddl.clone(),
        status: request
            .status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("SUCCESS")
            .to_owned(),
        operation_rows,
        use_time,
        extend_info: request.extend_info.clone(),
        schema_name: request.schema_name.clone(),
        organization_id: request.organization_id,
        user_name: request.user_name.clone(),
        more: request.more,
        operation_type: "SQL_EXECUTE".to_owned(),
    };
    legacy_storage_call(move || storage.create_operation_log(input))
        .await
        .map(|record| record.id)
}

pub(crate) async fn get_operation_log(
    application: &Application,
    id: i64,
) -> LegacyResult<LegacyOperationLogResponse> {
    let storage = legacy_storage(application)?;
    let record = legacy_storage_call(move || storage.get_operation_log(id))
        .await?
        .ok_or_else(|| {
            LegacyFailure::invalid(
                "operation_log_not_found",
                "The operation log does not exist",
            )
        })?;
    Ok(operation_log_response(record, false))
}

pub(crate) async fn list_operation_logs(
    application: &Application,
    query: &LegacyOperationLogListQuery,
) -> LegacyResult<LegacyPage<LegacyOperationLogResponse>> {
    let storage = legacy_storage(application)?;
    let storage_query = OperationLogListQuery {
        data_source_id: query
            .data_source_id
            .as_ref()
            .map(LegacyIdentifier::as_string),
        database_name: query.database_name.clone(),
        schema_name: query.schema_name.clone(),
        operation_type: query.operation_type.clone(),
        search_key: non_blank(&query.search_key),
        page_no: query.page_no,
        page_size: query.page_size,
    };
    legacy_storage_call(move || storage.list_operation_logs(&storage_query))
        .await
        .map(|page| {
            let total = usize::try_from(page.total).unwrap_or(usize::MAX);
            LegacyPage {
                data: page
                    .records
                    .into_iter()
                    .map(|record| operation_log_response(record, true))
                    .collect(),
                page_no: page.page_no,
                page_size: page.page_size,
                total,
                has_next_page: u64::from(page.page_no) * u64::from(page.page_size) < page.total,
            }
        })
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
    validate_metadata_page(query)?;
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
    items.retain(|item| table_matches_search(item, &query.search_key));
    Ok(paginate(items, query.page_no, query.page_size))
}

/// Lists the compact table projection used by autocomplete and table pickers.
pub(crate) async fn list_simple_tables(
    application: &Application,
    query: &LegacyTableListQuery,
) -> LegacyResult<Vec<LegacySimpleTable>> {
    validate_metadata_page(query)?;
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &query.database_type).await?;
    Ok(application
        .list_community_tables(ListCommunityTablesRequest {
            datasource_id,
            database_type,
            database_name: query.database_name.clone(),
            schema_name: query.schema_name.clone(),
            table_name_pattern: String::new(),
        })
        .await?
        .items
        .into_iter()
        .map(simple_table_response)
        .collect())
}

/// Lists table or view columns in the historical `ColumnResponse` shape.
pub(crate) async fn list_columns(
    application: &Application,
    query: &LegacyTableDetailQuery,
) -> LegacyResult<Vec<LegacyColumn>> {
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &query.database_type).await?;
    Ok(application
        .list_community_columns(ListCommunityColumnsRequest {
            datasource_id,
            database_type,
            database_name: query.database_name.clone(),
            schema_name: query.schema_name.clone(),
            table_name: query.table_name.clone(),
        })
        .await?
        .items
        .into_iter()
        .map(column_response)
        .collect())
}

/// Lists table indexes in the historical `IndexResponse` shape.
pub(crate) async fn list_indexes(
    application: &Application,
    query: &LegacyTableDetailQuery,
) -> LegacyResult<Vec<LegacyIndex>> {
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &query.database_type).await?;
    Ok(application
        .list_community_indexes(ListCommunityIndexesRequest {
            datasource_id,
            database_type,
            database_name: query.database_name.clone(),
            schema_name: query.schema_name.clone(),
            table_name: query.table_name.clone(),
        })
        .await?
        .items
        .into_iter()
        .map(index_response)
        .collect())
}

/// Community's historical key endpoint is an alias of its index metadata.
pub(crate) async fn list_keys(
    application: &Application,
    query: &LegacyTableDetailQuery,
) -> LegacyResult<Vec<LegacyIndex>> {
    list_indexes(application, query).await
}

/// Returns the native `SHOW CREATE TABLE` result used by Community's export action.
pub(crate) async fn export_table_ddl(
    application: &Application,
    query: &LegacyTableDetailQuery,
) -> LegacyResult<String> {
    let datasource_id = query.data_source_id.as_string();
    resolve_mysql_database_type(application, &datasource_id, &query.database_type).await?;
    Ok(application
        .table_ddl(
            &datasource_id,
            &query.database_name,
            &query.schema_name,
            &query.table_name,
        )
        .await?)
}

/// Preserves Community `MySQL`'s null create/alter example configuration.
pub(crate) fn mysql_table_ddl_example(
    query: &LegacyTableDdlExampleQuery,
) -> LegacyResult<Option<String>> {
    if normalize_database_type(&query.db_type) != "MYSQL" {
        return Err(LegacyFailure {
            code: "unsupported_database_type".to_owned(),
            message: "This Community compatibility route currently supports MySQL only".to_owned(),
        });
    }
    Ok(None)
}

/// Lists views in the same page wrapper used by the retained tree.
pub(crate) async fn list_views(
    application: &Application,
    query: &LegacyTableListQuery,
) -> LegacyResult<LegacyPage<LegacyTable>> {
    validate_metadata_page(query)?;
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &query.database_type).await?;
    let items = application
        .list_community_views(ListCommunityViewsRequest {
            datasource_id,
            database_type,
            database_name: query.database_name.clone(),
            schema_name: query.schema_name.clone(),
            view_name_pattern: String::new(),
        })
        .await?
        .items
        .into_iter()
        .map(table_response)
        .collect();
    Ok(full_page(items))
}

/// Reads one view, including its DDL, through the exact Core detail path.
pub(crate) async fn get_view(
    application: &Application,
    query: &LegacyTableDetailQuery,
) -> LegacyResult<LegacyTable> {
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &query.database_type).await?;
    Ok(table_response(
        application
            .get_community_view(ListCommunityViewsRequest {
                datasource_id,
                database_type,
                database_name: query.database_name.clone(),
                schema_name: query.schema_name.clone(),
                view_name_pattern: query.table_name.clone(),
            })
            .await?,
    ))
}

/// Reads the full table projection required by Community's table editor.
pub(crate) async fn get_editable_table(
    application: &Application,
    query: &LegacyTableDetailQuery,
) -> LegacyResult<LegacyEditableTable> {
    if query.table_name.trim().is_empty() {
        return Err(LegacyFailure::invalid(
            "invalid_table_query",
            "tableName is required",
        ));
    }
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_mysql_database_type(application, &datasource_id, &query.database_type).await?;
    let tables = application
        .list_community_tables(ListCommunityTablesRequest {
            datasource_id: datasource_id.clone(),
            database_type: database_type.clone(),
            database_name: query.database_name.clone(),
            schema_name: query.schema_name.clone(),
            table_name_pattern: query.table_name.clone(),
        })
        .await?
        .items;
    let table = tables
        .into_iter()
        .find(|table| table.name.eq_ignore_ascii_case(&query.table_name))
        .ok_or_else(|| LegacyFailure {
            code: "table_not_found".to_owned(),
            message: format!("Table {} does not exist", query.table_name),
        })?;
    let (columns, indexes) = tokio::try_join!(
        application.list_community_columns(ListCommunityColumnsRequest {
            datasource_id: datasource_id.clone(),
            database_type: database_type.clone(),
            database_name: query.database_name.clone(),
            schema_name: query.schema_name.clone(),
            table_name: query.table_name.clone(),
        }),
        application.list_community_indexes(ListCommunityIndexesRequest {
            datasource_id,
            database_type: database_type.clone(),
            database_name: query.database_name.clone(),
            schema_name: query.schema_name.clone(),
            table_name: query.table_name.clone(),
        }),
    )?;
    Ok(editable_table_response(
        table,
        columns.items,
        indexes.items,
        database_type,
    ))
}

/// Reads the full view projection used by Community's retained view editor.
pub(crate) async fn get_editable_view(
    application: &Application,
    query: &LegacyTableDetailQuery,
) -> LegacyResult<LegacyEditableTable> {
    if query.table_name.trim().is_empty() {
        return Err(LegacyFailure::invalid(
            "invalid_view_query",
            "tableName is required",
        ));
    }
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_mysql_database_type(application, &datasource_id, &query.database_type).await?;
    let (view, columns) = tokio::try_join!(
        application.get_community_view(ListCommunityViewsRequest {
            datasource_id: datasource_id.clone(),
            database_type: database_type.clone(),
            database_name: query.database_name.clone(),
            schema_name: query.schema_name.clone(),
            view_name_pattern: query.table_name.clone(),
        }),
        application.list_community_columns(ListCommunityColumnsRequest {
            datasource_id,
            database_type: database_type.clone(),
            database_name: query.database_name.clone(),
            schema_name: query.schema_name.clone(),
            table_name: query.table_name.clone(),
        }),
    )?;
    Ok(editable_table_response(
        view,
        columns.items,
        Vec::new(),
        database_type,
    ))
}

/// Returns the `MySQL` type and option inventory used by the retained table editor.
pub(crate) async fn table_editor_meta(
    application: &Application,
    query: &LegacyMetadataQuery,
) -> LegacyResult<MysqlTableEditorMeta> {
    let datasource_id = query.data_source_id.as_string();
    resolve_mysql_database_type(application, &datasource_id, &query.database_type).await?;
    Ok(mysql_table_editor_meta())
}

/// Builds a `MySQL` script for Community result-grid create, update, and delete operations.
pub(crate) async fn build_grid_update_sql(
    application: &Application,
    request: &LegacyGridUpdateRequest,
) -> LegacyResult<String> {
    let datasource_id = request.data_source_id.as_string();
    resolve_mysql_database_type(application, &datasource_id, &request.database_type).await?;
    if request.table_name.trim().is_empty() {
        return Err(LegacyFailure::invalid(
            "invalid_mysql_result_grid",
            "tableName is required",
        ));
    }
    let headers = request
        .header_list
        .iter()
        .map(mysql_grid_header)
        .collect::<LegacyResult<Vec<_>>>()?;
    let operations = request
        .operations
        .iter()
        .map(mysql_grid_operation)
        .collect::<LegacyResult<Vec<_>>>()?;
    reject_legacy_partial_large_values(&operations)?;
    Ok(build_mysql_result_grid_script(
        &mysql_qualified_name(
            &request.database_name,
            &request.schema_name,
            &request.table_name,
        ),
        &headers,
        &operations,
    )?)
}

/// Builds Community's copy-as-INSERT, copy-as-UPDATE, or copy-as-WHERE SQL.
pub(crate) async fn build_grid_copy_sql(
    application: &Application,
    request: &LegacyGridUpdateRequest,
) -> LegacyResult<String> {
    let datasource_id = request.data_source_id.as_string();
    resolve_mysql_database_type(application, &datasource_id, &request.database_type).await?;
    let headers = request
        .header_list
        .iter()
        .map(mysql_grid_header)
        .collect::<LegacyResult<Vec<_>>>()?;
    let operations = request
        .operations
        .iter()
        .map(mysql_grid_copy_operation)
        .collect::<LegacyResult<Vec<_>>>()?;
    Ok(build_mysql_result_grid_copy_sql(
        &mysql_qualified_name(
            &request.database_name,
            &request.schema_name,
            &required_name(&request.table_name, "tableName")?,
        ),
        &headers,
        &operations,
    )?)
}

/// Builds Community's clipboard SQL `IN` list for result cells or external text.
pub(crate) async fn build_grid_in_values(
    application: &Application,
    request: &LegacyGridUpdateRequest,
) -> LegacyResult<String> {
    let datasource_id = request.data_source_id.as_string();
    resolve_mysql_database_type(application, &datasource_id, &request.database_type).await?;
    match request.source_type.trim().to_ascii_uppercase().as_str() {
        "EXTERNAL_TEXT" => Ok(build_mysql_external_in_values(&request.external_values)?),
        "RESULT_SET" => {
            let headers = request
                .header_list
                .iter()
                .map(mysql_grid_header)
                .collect::<LegacyResult<Vec<_>>>()?;
            reject_unsupported_copy_cells(&request.operations)?;
            let operations = request
                .operations
                .iter()
                .map(mysql_grid_copy_operation)
                .collect::<LegacyResult<Vec<_>>>()?;
            Ok(build_mysql_result_grid_in_values(&headers, &operations)?)
        }
        _ => Err(LegacyFailure::invalid(
            "invalid_mysql_result_grid",
            "sourceType must be RESULT_SET or EXTERNAL_TEXT",
        )),
    }
}

/// Builds CREATE or ALTER TABLE statements in the historical `{ sql }[]` shape.
pub(crate) async fn build_table_modify_sql(
    application: &Application,
    request: &LegacyTableModifyRequest,
) -> LegacyResult<Vec<LegacySqlResponse>> {
    let datasource_id = request.data_source_id.as_string();
    resolve_mysql_database_type(application, &datasource_id, &request.database_type).await?;
    let sql = if let Some(old_table) = request.old_table.as_ref() {
        let reordered_columns = mysql_reordered_column_names(old_table, &request.new_table);
        if !reordered_columns.is_empty() {
            application
                .validate_native_mysql_column_reorder(
                    &datasource_id,
                    &first_non_blank(&request.database_name, &old_table.database_name),
                    &required_name(&old_table.name, "oldTable.name")?,
                    &reordered_columns,
                )
                .await?;
        }
        build_mysql_alter_table(&mysql_table_alter(
            old_table,
            &request.new_table,
            &request.database_name,
            &request.schema_name,
        )?)?
    } else {
        build_mysql_create_table(&mysql_table_definition(
            &request.new_table,
            &request.database_name,
            &request.schema_name,
        )?)?
    };
    Ok(vec![LegacySqlResponse { sql }])
}

/// Builds a CREATE DATABASE preview without executing it.
pub(crate) async fn build_create_database_sql(
    application: &Application,
    request: &LegacyDatabaseDefinitionRequest,
) -> LegacyResult<LegacySqlResponse> {
    let datasource_id = request.data_source_id.as_string();
    resolve_mysql_database_type(application, &datasource_id, &request.database_type).await?;
    let name = first_non_blank(&request.name, &request.database_name);
    Ok(LegacySqlResponse {
        sql: build_mysql_create_database(&MysqlDatabaseDefinition {
            name,
            if_not_exists: false,
            charset: non_blank(&request.charset),
            collation: non_blank(&request.collation),
        })?,
    })
}

/// `MySQL` treats Community schemas as database aliases.
pub(crate) async fn build_create_schema_sql(
    application: &Application,
    request: &LegacySchemaDefinitionRequest,
) -> LegacyResult<LegacySqlResponse> {
    let datasource_id = request.data_source_id.as_string();
    resolve_mysql_database_type(application, &datasource_id, &request.database_type).await?;
    let name = first_non_blank(&request.name, &request.schema_name);
    Ok(LegacySqlResponse {
        sql: build_mysql_create_schema(&MysqlDatabaseDefinition {
            name,
            if_not_exists: false,
            charset: None,
            collation: None,
        })?,
    })
}

pub(crate) async fn prepare_database_delete(
    application: &Application,
    request: &LegacyDeleteObjectRequest,
) -> LegacyResult<LegacyDeletePrepareResponse> {
    let datasource_id = request.data_source_id.as_string();
    let database_type =
        resolve_mysql_database_type(application, &datasource_id, &request.database_type).await?;
    let confirm_name = required_name(&request.database_name, "databaseName")?;
    Ok(LegacyDeletePrepareResponse {
        sql_preview: build_mysql_drop_database(&confirm_name, false)?,
        confirm_name,
        object_type: "DATABASE".to_owned(),
        db_type: database_type,
    })
}

pub(crate) async fn prepare_schema_delete(
    application: &Application,
    request: &LegacyDeleteObjectRequest,
) -> LegacyResult<LegacyDeletePrepareResponse> {
    let datasource_id = request.data_source_id.as_string();
    let database_type =
        resolve_mysql_database_type(application, &datasource_id, &request.database_type).await?;
    let target = first_non_blank(&request.schema_name, &request.database_name);
    let confirm_name = required_name(&target, "schemaName")?;
    Ok(LegacyDeletePrepareResponse {
        sql_preview: build_mysql_drop_schema(&confirm_name, false)?,
        confirm_name,
        object_type: "SCHEMA".to_owned(),
        db_type: database_type,
    })
}

pub(crate) async fn execute_database_delete(
    application: &Application,
    request: &LegacyDeleteObjectRequest,
) -> LegacyResult<()> {
    let prepared = prepare_database_delete(application, request).await?;
    validate_delete_confirmation(&prepared.confirm_name, &request.confirm_name)?;
    execute_generated_action(
        application,
        request.data_source_id.clone(),
        &prepared.confirm_name,
        "",
        "",
        prepared.sql_preview,
    )
    .await
}

pub(crate) async fn execute_schema_delete(
    application: &Application,
    request: &LegacyDeleteObjectRequest,
) -> LegacyResult<()> {
    let prepared = prepare_schema_delete(application, request).await?;
    validate_delete_confirmation(&prepared.confirm_name, &request.confirm_name)?;
    execute_generated_action(
        application,
        request.data_source_id.clone(),
        &prepared.confirm_name,
        "",
        "",
        prepared.sql_preview,
    )
    .await
}

pub(crate) async fn drop_table(
    application: &Application,
    request: &LegacyTableOperationRequest,
) -> LegacyResult<()> {
    let datasource_id = request.data_source_id.as_string();
    resolve_mysql_database_type(application, &datasource_id, &request.database_type).await?;
    let sql = build_mysql_drop_table(
        &mysql_qualified_name(
            &request.database_name,
            &request.schema_name,
            &request.table_name,
        ),
        false,
    )?;
    execute_generated_action(
        application,
        request.data_source_id.clone(),
        &request.database_name,
        &request.schema_name,
        &request.table_name,
        sql,
    )
    .await
}

pub(crate) async fn truncate_table(
    application: &Application,
    request: &LegacyTableOperationRequest,
) -> LegacyResult<()> {
    let datasource_id = request.data_source_id.as_string();
    resolve_mysql_database_type(application, &datasource_id, &request.database_type).await?;
    let sql = build_mysql_truncate_table(&mysql_qualified_name(
        &request.database_name,
        &request.schema_name,
        &request.table_name,
    ))?;
    execute_generated_action(
        application,
        request.data_source_id.clone(),
        &request.database_name,
        &request.schema_name,
        &request.table_name,
        sql,
    )
    .await
}

pub(crate) async fn copy_table(
    application: &Application,
    request: &LegacyTableCopyRequest,
) -> LegacyResult<()> {
    let datasource_id = request.data_source_id.as_string();
    resolve_mysql_database_type(application, &datasource_id, &request.database_type).await?;
    let new_name = if request.new_name.trim().is_empty() {
        format!("{}_copy", request.table_name.trim())
    } else {
        request.new_name.trim().to_owned()
    };
    let statements = build_mysql_copy_table(&MysqlTableCopy {
        source: mysql_qualified_name(
            &request.database_name,
            &request.schema_name,
            &request.table_name,
        ),
        target: mysql_qualified_name(&request.database_name, &request.schema_name, &new_name),
        if_not_exists: false,
        copy_data: request.copy_data,
    })?;
    for sql in statements {
        execute_generated_action(
            application,
            request.data_source_id.clone(),
            &request.database_name,
            &request.schema_name,
            &new_name,
            sql,
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn build_view_modify_sql(
    application: &Application,
    request: &LegacyViewOperationRequest,
) -> LegacyResult<String> {
    let datasource_id = request.data_source_id.as_string();
    resolve_mysql_database_type(application, &datasource_id, &request.database_type).await?;
    Ok(build_mysql_create_view(&mysql_view_definition(request)?)?)
}

pub(crate) async fn drop_view(
    application: &Application,
    request: &LegacyViewOperationRequest,
) -> LegacyResult<()> {
    let datasource_id = request.data_source_id.as_string();
    resolve_mysql_database_type(application, &datasource_id, &request.database_type).await?;
    let view_name = first_non_blank(&request.view_name, &request.table_name);
    let sql = build_mysql_drop_view(
        &mysql_qualified_name(&request.database_name, &request.schema_name, &view_name),
        false,
    )?;
    execute_generated_action(
        application,
        request.data_source_id.clone(),
        &request.database_name,
        &request.schema_name,
        &view_name,
        sql,
    )
    .await
}

pub(crate) async fn view_editor_meta(
    application: &Application,
    request: &LegacyViewOperationRequest,
) -> LegacyResult<LegacyViewMetaResponse> {
    let datasource_id = request.data_source_id.as_string();
    resolve_mysql_database_type(application, &datasource_id, &request.database_type).await?;
    let sql = "select * from table_name".to_owned();
    let preview_name = if request.database_name.trim().is_empty() {
        "`undefined`".to_owned()
    } else {
        format!("`{}`.`undefined`", request.database_name.replace('`', "``"))
    };
    let preview_sql = format!("create view {preview_name} AS \n{sql};");
    Ok(LegacyViewMetaResponse {
        configurations: mysql_view_configurations(),
        preview_sql,
        sql,
    })
}

fn mysql_view_configurations() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "labelName": "算法",
            "name": "algorithm",
            "inputType": "select",
            "defaultValue": "3",
            "required": false,
            "multiple": false,
            "display": null,
            "selects": [
                { "label": "UNDEFINED", "value": 0 },
                { "label": "MERGE", "value": 1 },
                { "label": "TEMPTABLE", "value": 2 },
                { "label": null, "value": 3 }
            ]
        }),
        serde_json::json!({
            "labelName": "检查选项",
            "name": "checkOption",
            "inputType": "select",
            "defaultValue": "2",
            "required": false,
            "multiple": false,
            "display": null,
            "selects": [
                { "label": "CASCADED", "value": 0 },
                { "label": "LOCAL", "value": 1 },
                { "label": null, "value": 2 }
            ]
        }),
        serde_json::json!({
            "labelName": "SQL 安全性",
            "name": "security",
            "inputType": "select",
            "defaultValue": "2",
            "required": false,
            "multiple": false,
            "display": null,
            "selects": [
                { "label": "DEFINER", "value": 0 },
                { "label": "INVOKER", "value": 1 },
                { "label": null, "value": 2 }
            ]
        }),
        serde_json::json!({
            "labelName": "视图名称",
            "name": "viewName",
            "inputType": "input",
            "defaultValue": null,
            "required": false,
            "multiple": false,
            "display": null,
            "selects": null
        }),
        serde_json::json!({
            "labelName": "定义者",
            "name": "definer",
            "inputType": "input",
            "defaultValue": null,
            "required": false,
            "multiple": false,
            "display": null,
            "selects": null
        }),
        serde_json::json!({
            "labelName": "use or replace",
            "name": "useOrReplace",
            "inputType": "checkbox",
            "defaultValue": "false",
            "required": false,
            "multiple": false,
            "display": null,
            "selects": null
        }),
    ]
}

/// Lists stored functions in the historical paged metadata shape.
pub(crate) async fn list_functions(
    application: &Application,
    query: &LegacyTableListQuery,
) -> LegacyResult<LegacyPage<LegacyFunction>> {
    validate_metadata_page(query)?;
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &query.database_type).await?;
    let items = application
        .list_community_functions(ListCommunityFunctionsRequest {
            datasource_id,
            database_type,
            database_name: query.database_name.clone(),
            schema_name: query.schema_name.clone(),
        })
        .await?
        .items
        .into_iter()
        .map(function_response)
        .collect();
    Ok(full_page(items))
}

/// Reads one stored function in the historical metadata shape.
pub(crate) async fn get_function(
    application: &Application,
    query: &LegacyFunctionDetailQuery,
) -> LegacyResult<LegacyFunction> {
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &query.database_type).await?;
    Ok(function_response(
        application
            .get_community_function(GetCommunityFunctionRequest {
                datasource_id,
                database_type,
                database_name: query.database_name.clone(),
                schema_name: query.schema_name.clone(),
                function_name: query.function_name.clone(),
            })
            .await?,
    ))
}

/// Lists stored procedures in the historical paged metadata shape.
pub(crate) async fn list_procedures(
    application: &Application,
    query: &LegacyTableListQuery,
) -> LegacyResult<LegacyPage<LegacyProcedure>> {
    validate_metadata_page(query)?;
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &query.database_type).await?;
    let items = application
        .list_community_procedures(ListCommunityProceduresRequest {
            datasource_id,
            database_type,
            database_name: query.database_name.clone(),
            schema_name: query.schema_name.clone(),
        })
        .await?
        .items
        .into_iter()
        .map(procedure_response)
        .collect();
    Ok(full_page(items))
}

/// Reads one stored procedure in the historical metadata shape.
pub(crate) async fn get_procedure(
    application: &Application,
    query: &LegacyProcedureDetailQuery,
) -> LegacyResult<LegacyProcedure> {
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &query.database_type).await?;
    Ok(procedure_response(
        application
            .get_community_procedure(GetCommunityProcedureRequest {
                datasource_id,
                database_type,
                database_name: query.database_name.clone(),
                schema_name: query.schema_name.clone(),
                procedure_name: query.procedure_name.clone(),
            })
            .await?,
    ))
}

/// Lists triggers in the historical paged metadata shape.
pub(crate) async fn list_triggers(
    application: &Application,
    query: &LegacyTableListQuery,
) -> LegacyResult<LegacyPage<LegacyTrigger>> {
    validate_metadata_page(query)?;
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &query.database_type).await?;
    let items = application
        .list_community_triggers(ListCommunityTriggersRequest {
            datasource_id,
            database_type,
            database_name: query.database_name.clone(),
            schema_name: query.schema_name.clone(),
        })
        .await?
        .items
        .into_iter()
        .map(trigger_response)
        .collect();
    Ok(full_page(items))
}

/// Reads one trigger in the historical metadata shape.
pub(crate) async fn get_trigger(
    application: &Application,
    query: &LegacyTriggerDetailQuery,
) -> LegacyResult<LegacyTrigger> {
    let datasource_id = query.data_source_id.as_string();
    let database_type =
        resolve_database_type(application, &datasource_id, &query.database_type).await?;
    Ok(trigger_response(
        application
            .get_community_trigger(GetCommunityTriggerRequest {
                datasource_id,
                database_type,
                database_name: query.database_name.clone(),
                schema_name: query.schema_name.clone(),
                trigger_name: query.trigger_name.clone(),
            })
            .await?,
    ))
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
    let editable_columns = application
        .list_community_columns(ListCommunityColumnsRequest {
            datasource_id: datasource_id.clone(),
            database_type: database_type.clone(),
            database_name: request.database_name.clone(),
            schema_name: request.schema_name.clone(),
            table_name: request.table_name.clone(),
        })
        .await?
        .items;
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
        wait_for_sql_execution(application, &accepted.operation_id),
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
    let mut headers: Vec<LegacyResultHeader> = page.columns.iter().map(result_header).collect();
    enrich_headers_from_columns(&mut headers, &editable_columns);
    let large_value_owner = application.create_large_value_owner();
    let mut data_list: Vec<Vec<LegacyResultCell>> = page
        .rows
        .into_iter()
        .map(|row| {
            row.values
                .into_iter()
                .zip(page.columns.iter())
                .map(|(value, column)| result_cell(application, &large_value_owner, value, column))
                .collect()
        })
        .collect();
    prepend_synthetic_row_numbers(&mut headers, &mut data_list, u64::from(offset));
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
        can_edit: true,
        table_name: request.table_name.clone(),
        sql_type: "SELECT".to_owned(),
        refresh_targets: Vec::new(),
        page_no: request.page_no,
        page_size: request.page_size,
        fuzzy_total: page.metadata.row_count,
        has_next_page,
        execute_sql_params: LegacySqlExecuteRequest::from(request),
        extra: serde_json::json!({}),
        comment: None,
        result_set_id: None,
        statement_sequence: Some(1),
        execution_metrics: None,
        execution_context: Some(LegacyExecutionContext {
            database_name: (!request.database_name.is_empty())
                .then(|| request.database_name.clone()),
            schema_name: (!request.schema_name.is_empty()).then(|| request.schema_name.clone()),
        }),
    }])
}

/// Starts one Community Console query through Core and returns the opaque
/// operation id used by both HTTP and desktop transports.
///
/// # Errors
///
/// Returns request validation, datasource, storage, or engine failures before
/// the operation is accepted.
pub async fn start_sql_execution(
    application: &Application,
    request: &LegacySqlExecuteRequest,
) -> LegacyResult<QueryAccepted> {
    let (datasource_id, row_limit) = validate_sql_execute_request(request)?;
    application
        .start_query(StartQueryRequest {
            datasource_id,
            sql: request.sql.clone(),
            parameters: Vec::new(),
            limits: QueryLimits {
                max_rows: row_limit.to_string(),
                max_result_bytes: RESULT_PAGE_MAX_BYTES.to_string(),
                batch_rows: request.page_size.min(512),
                batch_bytes: 1024 * 1024,
                result_ttl_seconds: 60,
            },
        })
        .await
        .map_err(Into::into)
}

/// Waits for a Core query terminal event without translating away its error
/// code or message. Desktop streaming can subscribe independently and use
/// this as the final retained-result barrier.
///
/// # Errors
///
/// Returns subscription, database execution, or cancellation failures.
pub async fn wait_for_sql_execution(
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
                    "sql_execution_cancelled",
                    "The SQL execution was cancelled",
                ));
            }
            OperationEvent::Started | OperationEvent::Progress { .. } => {}
        }
    }
    Err(LegacyFailure::invalid(
        "sql_execution_incomplete",
        "The SQL execution ended without a result",
    ))
}

/// Reads and translates a retained Core result into Community's historical
/// result-grid shape. This is shared by synchronous HTTP and desktop IPC.
///
/// # Errors
///
/// Returns invalid paging or retained-result read failures.
pub async fn read_sql_result(
    application: &Application,
    request: &LegacySqlExecuteRequest,
    metadata: &ResultMetadata,
    duration: u64,
) -> LegacyResult<LegacyManageResult> {
    let (offset, _) = sql_page_window(request)?;
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
    let fuzzy_total =
        if page.metadata.truncated_by_max_rows || page.metadata.truncated_by_max_result_bytes {
            format!("{}+", page.metadata.row_count)
        } else {
            page.metadata.row_count.clone()
        };
    let header_list = page.columns.iter().map(result_header).collect();
    let large_value_owner = application.create_large_value_owner();
    let data_list = page
        .rows
        .into_iter()
        .map(|row| {
            row.values
                .into_iter()
                .zip(page.columns.iter())
                .map(|(value, column)| result_cell(application, &large_value_owner, value, column))
                .collect()
        })
        .collect();
    Ok(LegacyManageResult {
        data_list,
        header_list,
        description: "Query executed successfully".to_owned(),
        message: String::new(),
        sql: request.sql.clone(),
        original_sql: request.sql.clone(),
        success: true,
        duration,
        update_count: 0,
        can_edit: false,
        table_name: request.table_name.clone(),
        sql_type: legacy_sql_type(&request.sql).to_owned(),
        refresh_targets: Vec::new(),
        page_no: request.page_no,
        page_size: request.page_size,
        fuzzy_total,
        has_next_page,
        execute_sql_params: request.clone(),
        extra: serde_json::json!({}),
        comment: None,
        result_set_id: request.result_set_id,
        statement_sequence: Some(1),
        execution_metrics: None,
        execution_context: Some(LegacyExecutionContext {
            database_name: (!request.database_name.is_empty())
                .then(|| request.database_name.clone()),
            schema_name: (!request.schema_name.is_empty()).then(|| request.schema_name.clone()),
        }),
    })
}

/// Executes the synchronous Community web contract while retaining Core's
/// asynchronous operation and result-store lifecycle internally.
///
/// # Errors
///
/// Returns request validation or failures that occur before Core accepts the
/// query. Failures after acceptance are returned as Community result items.
pub async fn execute_sql(
    application: &Application,
    request: &LegacySqlExecuteRequest,
) -> LegacyResult<Vec<LegacyManageResult>> {
    let _ = validate_sql_execute_request(request)?;
    if uses_native_mysql_console(application, request).await? {
        let execution_id = application.create_large_value_owner();
        return execute_mysql_sql(
            application,
            request,
            MysqlConsoleCancellation::new(),
            &execution_id,
            "SQL_EDITOR_HTTP",
        )
        .await;
    }
    let started_at = Instant::now();
    let accepted = start_sql_execution(application, request).await?;
    let terminal = tokio::time::timeout(
        SQL_EXECUTION_TIMEOUT,
        wait_for_sql_execution(application, &accepted.operation_id),
    )
    .await;
    let duration = elapsed_millis(started_at);
    match terminal {
        Ok(Ok(metadata)) => read_sql_result(application, request, &metadata, duration)
            .await
            .map(|result| vec![result]),
        Ok(Err(error)) => Ok(vec![sql_failure_result(request, &error, duration)]),
        Err(_) => {
            application.cancel_operation(&accepted.operation_id).await;
            Ok(vec![sql_failure_result(
                request,
                &LegacyFailure::invalid(
                    "sql_execution_timeout",
                    "The SQL execution did not finish in time",
                ),
                duration,
            )])
        }
    }
}

/// Executes the Community single-result DDL contract.
///
/// # Errors
///
/// Returns request, datasource, execution, or missing-result failures.
pub async fn execute_ddl(
    application: &Application,
    request: &LegacySqlExecuteRequest,
) -> LegacyResult<LegacyManageResult> {
    execute_sql(application, request)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| {
            LegacyFailure::invalid(
                "sql_execution_incomplete",
                "The SQL execution ended without a result",
            )
        })
}

/// Counts the rows produced by one `MySQL` query for Community's total-row control.
pub(crate) async fn count_mysql_rows(
    application: &Application,
    request: &LegacySqlExecuteRequest,
) -> LegacyResult<u64> {
    let (datasource_id, _) = validate_sql_execute_request(request)?;
    if !uses_native_mysql_console(application, request).await? {
        return Err(LegacyFailure::invalid(
            "unsupported_database_type",
            "The historical count route currently supports native MySQL only",
        ));
    }
    if request.sql.trim().is_empty() {
        return Ok(0);
    }
    let count_sql = build_mysql_count_query(&request.sql)?;
    let results = application
        .execute_mysql_console(
            MysqlConsoleRequest {
                datasource_id,
                database_name: request.database_name.clone(),
                sql: count_sql,
                page_no: 1,
                page_size: 1,
                result_set_id: None,
                single: true,
                page_size_all: false,
                explain: false,
                error_continue: false,
            },
            MysqlConsoleCancellation::new(),
        )
        .await?;
    let result = results.into_iter().next().ok_or_else(|| {
        LegacyFailure::invalid(
            "mysql_count_failed",
            "The MySQL count query returned no result",
        )
    })?;
    if !result.success {
        return Err(LegacyFailure {
            code: "mysql_count_failed".to_owned(),
            message: result.error.map_or(result.message, |error| error.message),
        });
    }
    let value = result
        .rows
        .first()
        .and_then(|row| row.values.first())
        .ok_or_else(|| {
            LegacyFailure::invalid(
                "mysql_count_failed",
                "The MySQL count query returned no value",
            )
        })?;
    let (JdbcValue::SignedInteger { value }
    | JdbcValue::UnsignedInteger { value }
    | JdbcValue::Decimal { value }
    | JdbcValue::Text { value }) = value
    else {
        return Err(LegacyFailure::invalid(
            "mysql_count_failed",
            "The MySQL count query returned a non-integer value",
        ));
    };
    value.parse::<u64>().map_err(|_| {
        LegacyFailure::invalid(
            "mysql_count_failed",
            "The MySQL count query returned an invalid integer",
        )
    })
}

/// Executes the native `MySQL` Console contract with a caller-owned cancellation
/// source. Desktop keeps this source by execution id while HTTP uses it for the
/// synchronous timeout boundary.
///
/// # Errors
///
/// Returns validation or datasource failures that occur before execution.
pub async fn execute_mysql_sql(
    application: &Application,
    request: &LegacySqlExecuteRequest,
    cancellation: MysqlConsoleCancellation,
    execution_id: &str,
    history_source: &str,
) -> LegacyResult<Vec<LegacyManageResult>> {
    let (datasource_id, _) = validate_sql_execute_request(request)?;
    let editable_columns = if request.table_name.trim().is_empty() {
        None
    } else {
        let database_type =
            resolve_database_type(application, &datasource_id, &request.database_type).await?;
        application
            .list_community_columns(ListCommunityColumnsRequest {
                datasource_id: datasource_id.clone(),
                database_type,
                database_name: request.database_name.clone(),
                schema_name: request.schema_name.clone(),
                table_name: request.table_name.clone(),
            })
            .await
            .ok()
            .map(|columns| columns.items)
    };

    let execution = application.execute_mysql_console(
        MysqlConsoleRequest {
            datasource_id,
            database_name: request.database_name.clone(),
            sql: request.sql.clone(),
            page_no: request.page_no,
            page_size: request.page_size,
            result_set_id: request.result_set_id,
            single: request.single,
            page_size_all: request.page_size_all,
            explain: request.explain,
            error_continue: request.error_continue.unwrap_or(true),
        },
        cancellation.clone(),
    );
    let large_value_owner = application.create_large_value_owner();
    tokio::pin!(execution);
    let timed_out = tokio::select! {
        result = &mut execution => Some(result),
        () = tokio::time::sleep(SQL_EXECUTION_TIMEOUT) => None,
    };
    let results = match timed_out {
        Some(Ok(results)) => results
            .into_iter()
            .map(|result| {
                mysql_console_result(
                    application,
                    &large_value_owner,
                    request,
                    result,
                    editable_columns.as_deref(),
                )
            })
            .collect(),
        Some(Err(error)) => {
            let error = LegacyFailure::from(error);
            vec![sql_failure_result(request, &error, 0)]
        }
        None => {
            let _ = cancellation.cancel(Some("The SQL execution timed out".to_owned()));
            if tokio::time::timeout(SQL_CANCELLATION_GRACE, &mut execution)
                .await
                .is_err()
            {
                tracing::warn!(
                    "MySQL Console execution did not finish during the timeout cleanup window"
                );
            }
            vec![sql_failure_result(
                request,
                &LegacyFailure::invalid(
                    "sql_execution_timeout",
                    "The SQL execution did not finish in time",
                ),
                u64::try_from(SQL_EXECUTION_TIMEOUT.as_millis()).unwrap_or(u64::MAX),
            )]
        }
    };
    record_mysql_console_history_best_effort(
        application,
        request,
        &results,
        execution_id,
        history_source,
    )
    .await;
    Ok(results)
}

async fn record_mysql_console_history_best_effort(
    application: &Application,
    request: &LegacySqlExecuteRequest,
    results: &[LegacyManageResult],
    execution_id: &str,
    history_source: &str,
) {
    let Some(storage) = application.storage().cloned() else {
        return;
    };
    let mut statements = BTreeMap::<u32, Vec<&LegacyManageResult>>::new();
    for (index, result) in results.iter().enumerate() {
        let fallback = u32::try_from(index).unwrap_or(u32::MAX).saturating_add(1);
        statements
            .entry(result.statement_sequence.unwrap_or(fallback))
            .or_default()
            .push(result);
    }
    for statement_results in statements.into_values() {
        let Some(first) = statement_results.first().copied() else {
            continue;
        };
        let operation_rows = statement_results
            .iter()
            .map(|result| result.update_count)
            .fold(0_u64, u64::saturating_add);
        let use_time = statement_results
            .iter()
            .map(|result| result.duration)
            .fold(0_u64, u64::saturating_add);
        let message = statement_results
            .iter()
            .find(|result| !result.success && !result.message.trim().is_empty())
            .map_or("", |result| result.message.as_str());
        let extend_info = serde_json::to_string(&serde_json::json!({
            "source": history_source,
            "sqlType": first.sql_type,
            "executionId": execution_id,
            "statementSequence": first.statement_sequence.unwrap_or(1),
            "message": message,
        }))
        .ok();
        let input = CreateOperationLog {
            name: None,
            data_source_id: Some(request.data_source_id.as_string()),
            data_source_name: non_blank(&request.data_source_name),
            connectable: Some(true),
            database_name: non_blank(&request.database_name),
            database_type: Some(default_if_blank(&request.database_type, "MYSQL")),
            ddl: first.original_sql.clone(),
            status: mysql_console_history_status(statement_results.as_slice()).to_owned(),
            operation_rows: i64::try_from(operation_rows).ok(),
            use_time: i64::try_from(use_time).ok(),
            extend_info,
            schema_name: non_blank(&request.schema_name),
            organization_id: None,
            user_name: None,
            more: first.original_sql.chars().count() > 200,
            operation_type: "SQL_EXECUTE".to_owned(),
        };
        let result = tokio::task::spawn_blocking({
            let storage = storage.clone();
            move || storage.create_operation_log(input)
        })
        .await;
        match result {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::warn!(%error, "MySQL Console history write failed"),
            Err(error) => tracing::warn!(%error, "MySQL Console history task failed"),
        }
    }
}

fn mysql_console_history_status(results: &[&LegacyManageResult]) -> &'static str {
    if results.iter().any(|result| {
        result
            .extra
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|message| message.get("errorCode"))
            .filter_map(serde_json::Value::as_str)
            .any(|code| matches!(code, "mysql_console_cancelled" | "sql_execution_cancelled"))
    }) {
        "cancelled"
    } else if results.iter().all(|result| result.success) {
        "success"
    } else {
        "fail"
    }
}

/// Reads one bounded retained large-cell chunk in the requested display format.
///
/// # Errors
///
/// Returns validation, expired-token, range, decoding, or unsupported-format failures.
pub fn read_large_cell_value(
    application: &Application,
    request: &LegacyLargeCellValueRequest,
) -> LegacyResult<LegacyLargeCellChunk> {
    if request.large_value_id.trim().is_empty() {
        return Err(LegacyFailure::invalid(
            "invalid_large_cell_value_request",
            "largeValueId is required",
        ));
    }
    let encoded = matches!(
        request.format.trim().to_ascii_lowercase().as_str(),
        "base64" | "hex"
    );
    let chunk = if encoded {
        application.read_large_value_encoded_chunk(
            &request.large_value_id,
            request.offset,
            request.limit,
        )
    } else {
        application.read_large_value_chunk(&request.large_value_id, request.offset, request.limit)
    }
    .map_err(LegacyFailure::from)?;
    format_large_value_chunk(chunk, &request.format)
}

/// Writes one complete retained large-cell value to a unique temporary download path.
///
/// # Errors
///
/// Returns validation, expired-token, decoding, task, directory, or file-write failures.
pub async fn download_large_cell_value_to_path(
    application: &Application,
    request: &LegacyLargeCellDownloadRequest,
) -> LegacyResult<String> {
    let application = application.clone();
    let request = request.clone();
    tokio::task::spawn_blocking(move || {
        let download = prepare_large_cell_download(&application, &request)?;
        let directory = std::env::temp_dir().join("chat2db").join("downloads");
        fs::create_dir_all(&directory).map_err(|_| LegacyFailure {
            code: "large_cell_download_failed".to_owned(),
            message: "The large cell download directory could not be created".to_owned(),
        })?;
        let path = unique_large_cell_download_path(
            &directory,
            &request.large_value_id,
            download.extension,
        );
        fs::write(&path, download.bytes).map_err(|_| LegacyFailure {
            code: "large_cell_download_failed".to_owned(),
            message: "The large cell value could not be written to disk".to_owned(),
        })?;
        Ok(path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|_| LegacyFailure {
        code: "large_cell_download_failed".to_owned(),
        message: "The large cell download task did not finish".to_owned(),
    })?
}

struct PreparedLargeCellDownload {
    bytes: Vec<u8>,
    content_type: &'static str,
    extension: &'static str,
}

fn prepare_large_cell_download(
    application: &Application,
    request: &LegacyLargeCellDownloadRequest,
) -> LegacyResult<PreparedLargeCellDownload> {
    if request.large_value_id.trim().is_empty() {
        return Err(LegacyFailure::invalid(
            "invalid_large_cell_value_request",
            "largeValueId is required",
        ));
    }
    let (raw, value_type) = read_complete_large_value(application, &request.large_value_id)?;
    match request.format.trim().to_ascii_lowercase().as_str() {
        "" | "raw" => Ok(PreparedLargeCellDownload {
            bytes: raw,
            content_type: if value_type == LargeValueType::Text {
                "text/plain; charset=utf-8"
            } else {
                "application/octet-stream"
            },
            extension: if value_type == LargeValueType::Text {
                "txt"
            } else {
                "bin"
            },
        }),
        "text" => Ok(PreparedLargeCellDownload {
            bytes: String::from_utf8_lossy(&raw).into_owned().into_bytes(),
            content_type: "text/plain; charset=utf-8",
            extension: "txt",
        }),
        "hex" => Ok(PreparedLargeCellDownload {
            bytes: encode_hex(&raw).into_bytes(),
            content_type: "text/plain; charset=utf-8",
            extension: "hex",
        }),
        _ => Err(LegacyFailure::invalid(
            "invalid_large_cell_value_request",
            "format must be raw, text, or hex",
        )),
    }
}

fn read_complete_large_value(
    application: &Application,
    large_value_id: &str,
) -> LegacyResult<(Vec<u8>, LargeValueType)> {
    let mut offset = 0_u64;
    let mut output = Vec::new();
    let mut value_type = None;
    loop {
        let chunk = application
            .read_large_value_chunk(large_value_id, offset, LARGE_VALUE_CHUNK_SIZE)
            .map_err(LegacyFailure::from)?;
        value_type.get_or_insert(chunk.display_mode);
        output.extend(decode_large_value_chunk(&chunk)?);
        if chunk.eof {
            return Ok((output, value_type.unwrap_or(LargeValueType::Binary)));
        }
        if chunk.next_offset <= offset {
            return Err(LegacyFailure {
                code: "large_cell_download_failed".to_owned(),
                message: "The large cell value did not advance while downloading".to_owned(),
            });
        }
        offset = chunk.next_offset;
    }
}

fn format_large_value_chunk(
    chunk: LargeValueChunk,
    format: &str,
) -> LegacyResult<LegacyLargeCellChunk> {
    let normalized = format.trim().to_ascii_lowercase();
    let (value, encoding) = match normalized.as_str() {
        "" | "auto" => (
            chunk.value.clone(),
            match chunk.encoding {
                LargeValueEncoding::Utf8 => "utf-8",
                LargeValueEncoding::Base64 => "base64",
            },
        ),
        "base64" => (
            BASE64_STANDARD.encode(decode_large_value_chunk(&chunk)?),
            "base64",
        ),
        "text" => (
            String::from_utf8_lossy(&decode_large_value_chunk(&chunk)?).into_owned(),
            "utf-8",
        ),
        "hex" => (encode_hex(&decode_large_value_chunk(&chunk)?), "hex"),
        _ => {
            return Err(LegacyFailure::invalid(
                "invalid_large_cell_value_request",
                "format must be text, hex, base64, or auto",
            ));
        }
    };
    Ok(LegacyLargeCellChunk {
        value,
        offset: chunk.offset,
        next_offset: chunk.next_offset,
        eof: chunk.eof,
        size_bytes: chunk.size_bytes,
        size_chars: chunk.size_chars,
        encoding: encoding.to_owned(),
        content_type: chunk.content_type,
        display_mode: chunk.display_mode,
    })
}

fn decode_large_value_chunk(chunk: &LargeValueChunk) -> LegacyResult<Vec<u8>> {
    match chunk.encoding {
        LargeValueEncoding::Utf8 => Ok(chunk.value.as_bytes().to_vec()),
        LargeValueEncoding::Base64 => {
            BASE64_STANDARD
                .decode(chunk.value.as_bytes())
                .map_err(|_| LegacyFailure {
                    code: "large_cell_value_invalid".to_owned(),
                    message: "The retained binary value could not be decoded".to_owned(),
                })
        }
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn unique_large_cell_download_path(
    directory: &std::path::Path,
    large_value_id: &str,
    extension: &str,
) -> PathBuf {
    let token_fragment = large_value_id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect::<String>();
    directory.join(format!(
        "chat2db-cell-{}-{token_fragment}.{extension}",
        unix_epoch_millis()
    ))
}

/// Reports whether the request resolves to the native `MySQL` driver.
///
/// # Errors
///
/// Returns a datasource lookup failure when the requested datasource is absent.
pub async fn uses_native_mysql_console(
    application: &Application,
    request: &LegacySqlExecuteRequest,
) -> LegacyResult<bool> {
    if request.database_type.eq_ignore_ascii_case("mysql") {
        return Ok(true);
    }
    let datasource = application
        .get_datasource(&request.data_source_id.as_string())
        .await?;
    Ok(datasource.driver_id.eq_ignore_ascii_case("mysql")
        || datasource.driver_id.to_ascii_lowercase().contains("mysql"))
}

fn mysql_console_result(
    application: &Application,
    large_value_owner: &str,
    request: &LegacySqlExecuteRequest,
    result: MysqlConsoleResult,
    editable_columns: Option<&[CommunityTableColumn]>,
) -> LegacyManageResult {
    let MysqlConsoleResult {
        statement_sequence,
        result_set_id,
        sql,
        success,
        message,
        update_count,
        columns,
        rows,
        row_count,
        has_more,
        duration_ms,
        error,
    } = result;
    let mut header_list: Vec<_> = columns.iter().map(result_header).collect();
    let can_edit = editable_columns.is_some_and(|editable_columns| {
        enrich_direct_table_headers(&mut header_list, editable_columns, &request.table_name)
    });
    let mut data_list: Vec<Vec<LegacyResultCell>> = rows
        .into_iter()
        .map(|row| {
            row.values
                .into_iter()
                .zip(columns.iter())
                .map(|(value, column)| result_cell(application, large_value_owner, value, column))
                .collect()
        })
        .collect();
    if can_edit {
        let offset = u64::from(request.page_no.saturating_sub(1))
            .saturating_mul(u64::from(request.page_size));
        prepend_synthetic_row_numbers(&mut header_list, &mut data_list, offset);
    }
    let sql_type = legacy_sql_type(&sql).to_owned();
    let extra = error.map_or_else(
        || serde_json::json!({}),
        |error| {
            serde_json::json!({
                "messages": [{
                    "level": "ERROR",
                    "message": error.message,
                    "source": "database",
                    "errorCode": error.code,
                    "resultSetId": result_set_id,
                }]
            })
        },
    );
    let finished_at_epoch_ms = unix_epoch_millis();
    LegacyManageResult {
        data_list,
        header_list,
        description: if success {
            "Query executed successfully".to_owned()
        } else {
            String::new()
        },
        message,
        sql: sql.clone(),
        original_sql: sql,
        success,
        duration: duration_ms,
        update_count,
        can_edit,
        table_name: request.table_name.clone(),
        refresh_targets: refresh_targets(request, &sql_type, success),
        sql_type,
        page_no: request.page_no,
        page_size: request.page_size,
        fuzzy_total: row_count.to_string(),
        has_next_page: has_more,
        execute_sql_params: request.clone(),
        extra,
        comment: None,
        result_set_id,
        statement_sequence: Some(statement_sequence),
        execution_metrics: Some(LegacyExecutionMetrics {
            started_at_epoch_ms: finished_at_epoch_ms.saturating_sub(duration_ms),
            finished_at_epoch_ms,
            total_duration_ms: duration_ms,
            execute_duration_ms: duration_ms,
            fetch_duration_ms: 0,
            fetched_row_count: row_count,
        }),
        execution_context: Some(LegacyExecutionContext {
            database_name: non_blank(&request.database_name),
            schema_name: non_blank(&request.schema_name),
        }),
    }
}

fn refresh_targets(
    request: &LegacySqlExecuteRequest,
    sql_type: &str,
    success: bool,
) -> Vec<serde_json::Value> {
    if !success
        || !matches!(
            sql_type,
            "INSERT" | "UPDATE" | "DELETE" | "CREATE" | "ALTER" | "DROP" | "TRUNCATE"
        )
    {
        return Vec::new();
    }
    vec![serde_json::json!({
        "dataSourceId": request.data_source_id.as_string(),
        "databaseName": request.database_name,
        "schemaName": request.schema_name,
        "tableName": request.table_name,
    })]
}

fn sql_page_window(request: &LegacySqlExecuteRequest) -> LegacyResult<(u32, u32)> {
    if request.page_no == 0 || request.page_size == 0 {
        return Err(LegacyFailure::invalid(
            "invalid_sql_execute_request",
            "pageNo and pageSize must be positive",
        ));
    }
    let offset = request
        .page_no
        .saturating_sub(1)
        .checked_mul(request.page_size)
        .ok_or_else(|| {
            LegacyFailure::invalid(
                "invalid_sql_execute_request",
                "requested page is outside the SQL result window",
            )
        })?;
    let row_limit = offset.checked_add(request.page_size).ok_or_else(|| {
        LegacyFailure::invalid(
            "invalid_sql_execute_request",
            "requested page is outside the SQL result window",
        )
    })?;
    if offset >= MAX_SQL_ROWS || row_limit > MAX_SQL_ROWS {
        return Err(LegacyFailure::invalid(
            "invalid_sql_execute_request",
            "SQL results are limited to the first 10000 rows",
        ));
    }
    Ok((offset, row_limit))
}

fn validate_sql_execute_request(request: &LegacySqlExecuteRequest) -> LegacyResult<(String, u32)> {
    let (_, row_limit) = sql_page_window(request)?;
    let datasource_id = request.data_source_id.as_string();
    if datasource_id.trim().is_empty() {
        return Err(LegacyFailure::invalid(
            "invalid_sql_execute_request",
            "dataSourceId is required",
        ));
    }
    if request.sql.trim().is_empty() {
        return Err(LegacyFailure::invalid(
            "invalid_sql_execute_request",
            "sql is required",
        ));
    }
    Ok((datasource_id, row_limit))
}

/// Builds the Community result-grid error item used for accepted operations
/// that fail asynchronously. Keeping this public prevents desktop IPC from
/// inventing a second error projection.
#[must_use]
pub fn sql_failure_result(
    request: &LegacySqlExecuteRequest,
    error: &LegacyFailure,
    duration: u64,
) -> LegacyManageResult {
    let message = error.message.clone();
    LegacyManageResult {
        data_list: Vec::new(),
        header_list: Vec::new(),
        description: String::new(),
        message: message.clone(),
        sql: request.sql.clone(),
        original_sql: request.sql.clone(),
        success: false,
        duration,
        update_count: 0,
        can_edit: false,
        table_name: request.table_name.clone(),
        sql_type: legacy_sql_type(&request.sql).to_owned(),
        refresh_targets: Vec::new(),
        page_no: request.page_no,
        page_size: request.page_size,
        fuzzy_total: "0".to_owned(),
        has_next_page: false,
        execute_sql_params: request.clone(),
        extra: serde_json::json!({
            "messages": [{
                "level": "ERROR",
                "message": message,
                "source": "database",
                "errorCode": error.code,
            }]
        }),
        comment: None,
        result_set_id: request.result_set_id,
        statement_sequence: Some(1),
        execution_metrics: None,
        execution_context: Some(LegacyExecutionContext {
            database_name: (!request.database_name.is_empty())
                .then(|| request.database_name.clone()),
            schema_name: (!request.schema_name.is_empty()).then(|| request.schema_name.clone()),
        }),
    }
}

fn elapsed_millis(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn unix_epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn legacy_sql_type(sql: &str) -> &'static str {
    let keyword = sql
        .trim_start()
        .split(|character: char| !character.is_ascii_alphabetic())
        .next()
        .unwrap_or_default();
    if ["select", "with", "show", "describe", "desc", "explain"]
        .iter()
        .any(|candidate| keyword.eq_ignore_ascii_case(candidate))
    {
        return "SELECT";
    }
    [
        ("insert", "INSERT"),
        ("replace", "REPLACE_INTO"),
        ("update", "UPDATE"),
        ("delete", "DELETE"),
        ("create", "CREATE"),
        ("alter", "ALTER"),
        ("drop", "DROP"),
        ("truncate", "TRUNCATE"),
        ("call", "CALL"),
        ("use", "USE_DATABASE"),
        ("set", "SET"),
        ("start", "START_TRANSACTION"),
        ("begin", "BEGIN_WORK"),
        ("commit", "COMMIT"),
        ("rollback", "ROLLBACK"),
    ]
    .into_iter()
    .find_map(|(candidate, sql_type)| keyword.eq_ignore_ascii_case(candidate).then_some(sql_type))
    .unwrap_or("OTHER")
}

fn operation_log_response(record: OperationLogRecord, preview: bool) -> LegacyOperationLogResponse {
    let (ddl, more) = if preview {
        operation_log_preview(&record.ddl, record.more)
    } else {
        (record.ddl, record.more)
    };
    LegacyOperationLogResponse {
        id: record.id,
        gmt_create: format_operation_log_time(record.created_at_ms),
        gmt_modified: format_operation_log_time(record.updated_at_ms),
        name: record.name.unwrap_or_default(),
        data_source_id: record.data_source_id,
        data_source_name: record.data_source_name,
        connectable: record.connectable,
        database_name: record.database_name,
        database_type: record.database_type,
        ddl,
        more,
        status: Some(record.status),
        operation_rows: record
            .operation_rows
            .and_then(|value| value.try_into().ok()),
        use_time: record.use_time.and_then(|value| value.try_into().ok()),
        extend_info: record.extend_info,
        schema_name: record.schema_name,
        organization_id: record.organization_id,
        user_name: record.user_name,
    }
}

fn operation_log_preview(ddl: &str, persisted_more: bool) -> (String, bool) {
    const PREVIEW_CHARS: usize = 200;
    let mut characters = ddl.chars();
    let mut preview = characters.by_ref().take(PREVIEW_CHARS).collect::<String>();
    let truncated = characters.next().is_some();
    if truncated {
        preview.push_str("...");
    }
    (preview, persisted_more || truncated)
}

fn format_operation_log_time(timestamp_ms: i64) -> String {
    Local
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .map(|timestamp| timestamp.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

fn non_blank(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn saved_console_response(record: SavedConsoleRecord) -> LegacySavedConsoleResponse {
    let connectable = record
        .data_source_id
        .as_ref()
        .is_some_and(|id| !id.trim().is_empty());
    LegacySavedConsoleResponse {
        id: record.id,
        name: record.name,
        data_source_id: record.data_source_id,
        data_source_name: record.data_source_name,
        connectable,
        database_name: record.database_name,
        schema_name: record.schema_name,
        database_type: record.database_type,
        ddl: record.ddl,
        status: record.status,
        tab_opened: record.tab_opened,
        operation_type: record.operation_type,
    }
}

fn required_string_patch(patch: LegacyPatch<String>) -> Option<String> {
    match patch {
        LegacyPatch::Unset | LegacyPatch::Set(None) => None,
        LegacyPatch::Set(Some(value)) => Some(value),
    }
}

#[allow(clippy::option_option)]
fn nullable_string_patch(patch: LegacyPatch<String>) -> Option<Option<String>> {
    match patch {
        LegacyPatch::Unset => None,
        LegacyPatch::Set(value) => Some(value),
    }
}

#[allow(clippy::option_option)]
fn identifier_patch(patch: LegacyPatch<LegacyIdentifier>) -> Option<Option<String>> {
    match patch {
        LegacyPatch::Unset => None,
        LegacyPatch::Set(value) => Some(value.map(|id| id.as_string())),
    }
}

fn legacy_console_id(id: &LegacyIdentifier) -> LegacyResult<i64> {
    let parsed = id.as_string().parse::<i64>().map_err(|_| {
        LegacyFailure::invalid(
            "invalid_saved_console",
            "id must be a positive signed 64-bit integer",
        )
    })?;
    if parsed <= 0 {
        return Err(LegacyFailure::invalid(
            "invalid_saved_console",
            "id must be a positive signed 64-bit integer",
        ));
    }
    Ok(parsed)
}

fn default_if_blank(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

fn legacy_storage(application: &Application) -> LegacyResult<Storage> {
    application.storage().cloned().ok_or_else(|| {
        LegacyFailure::invalid(
            "storage_unavailable",
            "Local product storage is not configured",
        )
    })
}

async fn legacy_storage_call<T, F>(operation: F) -> LegacyResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, StorageError> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| LegacyFailure {
            code: "internal_error".to_owned(),
            message: "The operation could not be completed".to_owned(),
        })?
        .map_err(storage_failure)
}

fn storage_failure(error: StorageError) -> LegacyFailure {
    match error {
        StorageError::SavedConsoleNotFound(id) => LegacyFailure {
            code: "saved_console_not_found".to_owned(),
            message: format!("Saved Console {id} does not exist"),
        },
        StorageError::InvalidSavedConsole(message) => LegacyFailure {
            code: "invalid_saved_console".to_owned(),
            message: message.to_owned(),
        },
        StorageError::InvalidOperationLog(message) => LegacyFailure {
            code: "invalid_operation_log".to_owned(),
            message: message.to_owned(),
        },
        other => LegacyFailure::from(AppError::from(other)),
    }
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

fn simple_table_response(table: CommunityTable) -> LegacySimpleTable {
    LegacySimpleTable {
        name: table.name,
        comment: table.comment,
        // The original service built `SimpleTable` without setting this field.
        table_type: None,
    }
}

fn column_response(column: CommunityTableColumn) -> LegacyColumn {
    let old_name = Some(column.name.clone());
    let value = mysql_enum_set_editor_value(&column.extent);
    LegacyColumn {
        old_name,
        name: column.name,
        table_name: column.table_name,
        column_type: column.column_type,
        data_type: column.data_type,
        default_value: column.default_value,
        auto_increment: column.auto_increment,
        comment: column.comment,
        primary_key: column.primary_key,
        primary_key_name: column.primary_key_name,
        primary_key_order: column.primary_key_order,
        schema_name: column.schema_name,
        database_name: column.database_name,
        type_name: None,
        column_size: column.column_size,
        buffer_length: column.buffer_length,
        decimal_digits: column.decimal_digits,
        num_prec_radix: column.num_prec_radix,
        nullable_int: None,
        sql_data_type: column.sql_data_type,
        sql_datetime_sub: column.sql_datetime_sub,
        char_octet_length: column.char_octet_length,
        ordinal_position: column.ordinal_position,
        nullable: column.nullable,
        generated_column: column.generated_column,
        extent: column.extent,
        char_set_name: column.charset,
        collation_name: column.collation,
        value,
        unit: column.unit,
        sparse: column.sparse,
        default_constraint_name: column.default_constraint_name,
        seed: column.seed,
        increment: column.increment,
        on_update_current_timestamp: column.on_update_current_timestamp,
        edit_status: None,
    }
}

fn index_response(index: CommunityTableIndex) -> LegacyIndex {
    LegacyIndex {
        columns: None,
        old_name: Some(index.name.clone()),
        name: index.name,
        table_name: index.table_name,
        index_type: index.index_type,
        unique: index.unique,
        comment: index.comment,
        schema_name: index.schema_name,
        database_name: index.database_name,
        column_list: index
            .columns
            .into_iter()
            .map(index_column_response)
            .collect(),
        edit_status: None,
        concurrently: index.concurrently,
        method: index.method,
        foreign_schema_name: index.foreign_schema_name,
        foreign_table_name: index.foreign_table_name,
        foreign_column_namelist: index.foreign_column_names,
    }
}

fn editable_index_response(index: CommunityTableIndex) -> LegacyIndex {
    let method = index.index_type.clone();
    let index_type = if index.name.eq_ignore_ascii_case("PRIMARY") {
        "Primary"
    } else if method.eq_ignore_ascii_case("FULLTEXT") {
        "Fulltext"
    } else if method.eq_ignore_ascii_case("SPATIAL") {
        "Spatial"
    } else if index.unique == Some(true) {
        "Unique"
    } else {
        "Normal"
    }
    .to_owned();
    let mut response = index_response(index);
    response.index_type = index_type;
    response.method = method;
    response
}

fn index_column_response(column: CommunityTableIndexColumn) -> LegacyIndexColumn {
    LegacyIndexColumn {
        index_name: column.index_name,
        table_name: column.table_name,
        index_type: column.column_type,
        comment: column.comment,
        column_name: column.column_name,
        ordinal_position: column.ordinal_position,
        collation: column.collation,
        schema_name: column.schema_name,
        database_name: column.database_name,
        non_unique: column.non_unique,
        index_qualifier: column.index_qualifier,
        asc_or_desc: column.sort_order,
        cardinality: decimal_i64(column.cardinality.as_deref()),
        pages: decimal_i64(column.pages.as_deref()),
        filter_condition: column.filter_condition,
        sub_part: decimal_i64(column.sub_part.as_deref()),
        edit_status: None,
    }
}

fn editable_table_response(
    table: CommunityTable,
    columns: Vec<CommunityTableColumn>,
    indexes: Vec<CommunityTableIndex>,
    database_type: String,
) -> LegacyEditableTable {
    LegacyEditableTable {
        name: table.name,
        comment: table.comment,
        schema_name: table.schema_name,
        database_name: table.database_name,
        table_type: table.table_type,
        column_list: columns.into_iter().map(column_response).collect(),
        index_list: indexes.into_iter().map(editable_index_response).collect(),
        foreign_key_list: Vec::new(),
        db_type: database_type,
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

fn function_response(function: CommunityFunction) -> LegacyFunction {
    LegacyFunction {
        database_name: function.database_name,
        schema_name: function.schema_name,
        function_name: function.name,
        remarks: function.remarks,
        function_type: function.function_type,
        specific_name: function.specific_name,
        function_body: function.body,
        function_template: function.template,
    }
}

fn procedure_response(procedure: CommunityProcedure) -> LegacyProcedure {
    LegacyProcedure {
        database_name: procedure.database_name,
        schema_name: procedure.schema_name,
        procedure_name: procedure.name,
        remarks: procedure.remarks,
        procedure_type: procedure.procedure_type,
        specific_name: procedure.specific_name,
        procedure_body: procedure.body,
    }
}

fn trigger_response(trigger: CommunityTrigger) -> LegacyTrigger {
    LegacyTrigger {
        database_name: trigger.database_name,
        schema_name: trigger.schema_name,
        trigger_name: trigger.name,
        event_manipulation: trigger.event_manipulation,
        trigger_body: trigger.body,
    }
}

fn decimal_i64(value: Option<&str>) -> Option<i64> {
    value.and_then(|value| value.parse().ok())
}

fn result_header(column: &ResultColumn) -> LegacyResultHeader {
    LegacyResultHeader {
        data_type: legacy_data_type(column.value_type).to_owned(),
        name: column.label.clone(),
        column_name: column.name.clone(),
        column_type: column.jdbc_type_name.clone(),
        table_name: column.table_name.clone(),
        database_name: column.catalog_name.clone(),
        schema_name: column.schema_name.clone(),
        primary_key: false,
        comment: None,
        default_value: None,
        auto_increment: None,
        nullable: column.nullability != ColumnNullability::NoNulls,
        column_size: column.precision.or(column.display_size),
        decimal_digits: column.scale,
        editor_type: None,
    }
}

fn enrich_direct_table_headers(
    headers: &mut [LegacyResultHeader],
    columns: &[CommunityTableColumn],
    expected_table_name: &str,
) -> bool {
    if headers.is_empty() || expected_table_name.trim().is_empty() {
        return false;
    }
    let mut direct_table = None::<&str>;
    for header in headers.iter() {
        let Some(table_name) = header.table_name.as_deref().filter(|name| !name.is_empty()) else {
            return false;
        };
        if !table_name.eq_ignore_ascii_case(expected_table_name) {
            return false;
        }
        if direct_table.is_some_and(|current| !current.eq_ignore_ascii_case(table_name)) {
            return false;
        }
        direct_table = Some(table_name);
    }

    enrich_headers_from_columns(headers, columns)
}

fn enrich_headers_from_columns(
    headers: &mut [LegacyResultHeader],
    columns: &[CommunityTableColumn],
) -> bool {
    for header in headers.iter_mut() {
        let metadata = columns.iter().find(|column| {
            column.name.eq_ignore_ascii_case(&header.column_name)
                || column.name.eq_ignore_ascii_case(&header.name)
        });
        let Some(metadata) = metadata else {
            return false;
        };
        header.primary_key = metadata.primary_key.unwrap_or(false);
        if !metadata.column_type.trim().is_empty() {
            header.column_type.clone_from(&metadata.column_type);
        }
        header.table_name = non_blank(&metadata.table_name);
        header.database_name = non_blank(&metadata.database_name);
        header.schema_name = non_blank(&metadata.schema_name);
        header.comment = Some(metadata.comment.clone());
        header.default_value.clone_from(&metadata.default_value);
        header.auto_increment = metadata.auto_increment.map(i32::from);
        header.nullable = metadata.nullable.unwrap_or(1) != 0;
        header.column_size = metadata.column_size.and_then(|size| size.try_into().ok());
        header.decimal_digits = metadata.decimal_digits;
        header.editor_type = Some(result_editor_type(&metadata.column_type).to_owned());
    }
    true
}

fn result_editor_type(column_type: &str) -> &'static str {
    let base_type = column_type
        .split(|character: char| character == '(' || character.is_ascii_whitespace())
        .next()
        .unwrap_or_default();
    if base_type.eq_ignore_ascii_case("DATE") {
        "DATE"
    } else if base_type.eq_ignore_ascii_case("TIME") {
        "TIME"
    } else if base_type.eq_ignore_ascii_case("DATETIME") {
        "DATETIME"
    } else if base_type.eq_ignore_ascii_case("TIMESTAMP") {
        "TIMESTAMP"
    } else {
        "TEXT"
    }
}

fn prepend_synthetic_row_numbers(
    headers: &mut Vec<LegacyResultHeader>,
    rows: &mut [Vec<LegacyResultCell>],
    offset: u64,
) {
    headers.insert(
        0,
        LegacyResultHeader {
            data_type: "CHAT2DB_ROW_NUMBER".to_owned(),
            name: "CHAT2DB_ROW_NUMBER".to_owned(),
            column_name: "CHAT2DB_ROW_NUMBER".to_owned(),
            column_type: "BIGINT".to_owned(),
            table_name: None,
            database_name: None,
            schema_name: None,
            primary_key: false,
            comment: None,
            default_value: None,
            auto_increment: None,
            nullable: false,
            column_size: Some(20),
            decimal_digits: Some(0),
            editor_type: None,
        },
    );
    for (index, row) in rows.iter_mut().enumerate() {
        let row_number = offset
            .saturating_add(u64::try_from(index).unwrap_or(u64::MAX))
            .saturating_add(1);
        row.insert(
            0,
            LegacyResultCell {
                value: Some(row_number.to_string()),
                large_value: false,
                large_value_id: None,
                value_type: "UNKNOWN".to_owned(),
                sql_type: -5,
                column_type: "BIGINT".to_owned(),
                size_bytes: None,
                size_chars: None,
                loaded_bytes: None,
                loaded_chars: None,
                truncated: false,
                unsupported_reason: None,
            },
        );
    }
}

fn result_cell(
    application: &Application,
    large_value_owner: &str,
    value: JdbcValue,
    column: &ResultColumn,
) -> LegacyResultCell {
    match value {
        JdbcValue::Text { value } => {
            retained_text_cell(application, large_value_owner, value, "TEXT", column)
        }
        JdbcValue::Json { value } => {
            retained_text_cell(application, large_value_owner, value, "JSON", column)
        }
        JdbcValue::Binary { value } => match BASE64_STANDARD.decode(value.as_bytes()) {
            Ok(value) => retained_binary_cell(application, large_value_owner, value, column),
            Err(_) => unsupported_cell(
                Some(value),
                "BINARY",
                column,
                None,
                None,
                "The binary cell payload could not be decoded".to_owned(),
            ),
        },
        value => scalar_result_cell(value, column),
    }
}

fn retained_text_cell(
    application: &Application,
    owner_id: &str,
    value: String,
    value_type: &str,
    column: &ResultColumn,
) -> LegacyResultCell {
    let size_bytes = value.len() as u64;
    let size_chars = value.chars().count() as u64;
    let fallback = bounded_utf8_preview(&value, LARGE_VALUE_FALLBACK_PREVIEW_BYTES);
    match application.retain_large_text(owner_id, value) {
        Ok(preview) => retained_preview_cell(preview, value_type, column),
        Err(error) => unsupported_cell(
            Some(fallback),
            value_type,
            column,
            Some(size_bytes),
            Some(size_chars),
            error.api_error().message,
        ),
    }
}

fn retained_binary_cell(
    application: &Application,
    owner_id: &str,
    value: Vec<u8>,
    column: &ResultColumn,
) -> LegacyResultCell {
    let size_bytes = value.len() as u64;
    let fallback =
        BASE64_STANDARD.encode(&value[..value.len().min(LARGE_VALUE_FALLBACK_PREVIEW_BYTES)]);
    match application.retain_large_binary(owner_id, value) {
        Ok(preview) => retained_preview_cell(preview, "BINARY", column),
        Err(error) => unsupported_cell(
            Some(fallback),
            "BINARY",
            column,
            Some(size_bytes),
            None,
            error.api_error().message,
        ),
    }
}

fn retained_preview_cell(
    preview: LargeValuePreview,
    value_type: &str,
    column: &ResultColumn,
) -> LegacyResultCell {
    LegacyResultCell {
        value: Some(preview.value),
        large_value: preview.large_value,
        large_value_id: preview.large_value_id,
        value_type: value_type.to_owned(),
        sql_type: column.jdbc_type,
        column_type: column.jdbc_type_name.clone(),
        size_bytes: Some(preview.size_bytes),
        size_chars: preview.size_chars,
        loaded_bytes: Some(preview.loaded_bytes),
        loaded_chars: preview.loaded_chars,
        truncated: preview.truncated,
        unsupported_reason: None,
    }
}

fn unsupported_cell(
    value: Option<String>,
    value_type: &str,
    column: &ResultColumn,
    size_bytes: Option<u64>,
    size_chars: Option<u64>,
    reason: String,
) -> LegacyResultCell {
    LegacyResultCell {
        loaded_bytes: value.as_ref().map(|value| value.len() as u64),
        loaded_chars: value.as_ref().map(|value| value.chars().count() as u64),
        value,
        large_value: true,
        large_value_id: None,
        value_type: value_type.to_owned(),
        sql_type: column.jdbc_type,
        column_type: column.jdbc_type_name.clone(),
        size_bytes,
        size_chars,
        truncated: true,
        unsupported_reason: Some(reason),
    }
}

fn scalar_result_cell(value: JdbcValue, column: &ResultColumn) -> LegacyResultCell {
    let value = match value {
        JdbcValue::Null => None,
        JdbcValue::Boolean { value } => Some(value.to_string()),
        JdbcValue::SignedInteger { value }
        | JdbcValue::UnsignedInteger { value }
        | JdbcValue::Float32 { value }
        | JdbcValue::Float64 { value }
        | JdbcValue::Decimal { value }
        | JdbcValue::Date { value }
        | JdbcValue::Time { value }
        | JdbcValue::Timestamp { value }
        | JdbcValue::TimestampWithTimeZone { value }
        | JdbcValue::Uuid { value } => Some(value),
        JdbcValue::Opaque { display_value, .. } => Some(display_value),
        JdbcValue::Text { .. } | JdbcValue::Binary { .. } | JdbcValue::Json { .. } => {
            unreachable!("large values are handled before scalar conversion")
        }
    };
    LegacyResultCell {
        value,
        large_value: false,
        large_value_id: None,
        value_type: "UNKNOWN".to_owned(),
        sql_type: column.jdbc_type,
        column_type: column.jdbc_type_name.clone(),
        size_bytes: None,
        size_chars: None,
        loaded_bytes: None,
        loaded_chars: None,
        truncated: false,
        unsupported_reason: None,
    }
}

fn bounded_utf8_preview(value: &str, max_bytes: usize) -> String {
    let mut end = value.len().min(max_bytes);
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
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

async fn resolve_mysql_database_type(
    application: &Application,
    datasource_id: &str,
    explicit: &str,
) -> LegacyResult<String> {
    let database_type = resolve_database_type(application, datasource_id, explicit).await?;
    if database_type == "MYSQL" {
        Ok(database_type)
    } else {
        Err(LegacyFailure {
            code: "unsupported_database_type".to_owned(),
            message: "This Community compatibility route currently supports MySQL only".to_owned(),
        })
    }
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

fn mysql_grid_header(header: &LegacyGridHeaderRequest) -> LegacyResult<MysqlResultGridHeader> {
    let name = first_non_blank(&header.column_name, &header.name);
    Ok(MysqlResultGridHeader {
        name: required_name(&name, "headerList.name")?,
        column_type: header.column_type.clone(),
        data_type: header.data_type.clone(),
        primary_key: header.primary_key,
        auto_increment: header.auto_increment,
    })
}

fn mysql_grid_operation(
    operation: &LegacyGridOperationRequest,
) -> LegacyResult<MysqlResultGridOperation> {
    let operation_type = match operation
        .operation_type
        .trim()
        .to_ascii_uppercase()
        .as_str()
    {
        "CREATE" => MysqlResultGridOperationType::Create,
        "UPDATE" => MysqlResultGridOperationType::Update,
        "DELETE" => MysqlResultGridOperationType::Delete,
        _ => {
            return Err(LegacyFailure::invalid(
                "invalid_mysql_result_grid",
                "operation type must be CREATE, UPDATE, or DELETE",
            ));
        }
    };
    Ok(MysqlResultGridOperation {
        operation_type,
        data_list: operation.data_list.clone(),
        old_data_list: operation.old_data_list.clone(),
    })
}

fn mysql_grid_copy_operation(
    operation: &LegacyGridOperationRequest,
) -> LegacyResult<MysqlResultGridCopyOperation> {
    let operation_type = match operation
        .operation_type
        .trim()
        .to_ascii_uppercase()
        .as_str()
    {
        "CREATE" => MysqlResultGridCopyOperationType::Create,
        "UPDATE_COPY" => MysqlResultGridCopyOperationType::UpdateCopy,
        "WHERE" | "IN_VALUES" => MysqlResultGridCopyOperationType::Where,
        _ => {
            return Err(LegacyFailure::invalid(
                "invalid_mysql_result_grid",
                "copy operation type must be CREATE, UPDATE_COPY, WHERE, or IN_VALUES",
            ));
        }
    };
    Ok(MysqlResultGridCopyOperation {
        operation_type,
        data_list: operation.data_list.clone(),
        select_cols: operation.select_cols.clone(),
    })
}

fn reject_unsupported_copy_cells(operations: &[LegacyGridOperationRequest]) -> LegacyResult<()> {
    if operations.iter().any(|operation| {
        operation.selected_cell.as_ref().is_some_and(|cell| {
            cell.large_value
                || cell.truncated
                || cell.large_value_id.is_some()
                || matches!(cell.value_type.as_str(), "BINARY" | "BLOB" | "BYTES")
        })
    }) {
        return Err(LegacyFailure::invalid(
            "mysql_partial_large_value_rejected",
            "Binary and partial large values cannot be copied as SQL IN values",
        ));
    }
    Ok(())
}

fn reject_legacy_partial_large_values(operations: &[MysqlResultGridOperation]) -> LegacyResult<()> {
    let has_partial = operations.iter().any(|operation| {
        operation
            .data_list
            .iter()
            .chain(&operation.old_data_list)
            .flatten()
            .any(|value| {
                value
                    .to_ascii_uppercase()
                    .starts_with(LARGE_VALUE_PREVIEW_PREFIX)
            })
    });
    if has_partial {
        Err(LegacyFailure::invalid(
            "mysql_partial_large_value_rejected",
            "Partial large-value previews cannot be written back",
        ))
    } else {
        Ok(())
    }
}

fn mysql_qualified_name(database_name: &str, schema_name: &str, name: &str) -> MysqlQualifiedName {
    MysqlQualifiedName {
        database_name: non_blank(database_name),
        schema_name: non_blank(schema_name),
        name: name.trim().to_owned(),
    }
}

fn mysql_table_definition(
    table: &LegacyEditableTable,
    database_name: &str,
    schema_name: &str,
) -> LegacyResult<MysqlTableDefinition> {
    let columns = table
        .column_list
        .iter()
        .filter(|column| !has_edit_status(column.edit_status.as_deref(), "DELETE"))
        .map(mysql_column_definition)
        .collect::<LegacyResult<Vec<_>>>()?;
    let mut indexes = table
        .index_list
        .iter()
        .filter(|index| !has_edit_status(index.edit_status.as_deref(), "DELETE"))
        .map(mysql_index_definition)
        .collect::<LegacyResult<Vec<_>>>()?;
    if !indexes
        .iter()
        .any(|index| index.kind == MysqlIndexKind::Primary)
    {
        let primary_columns = table
            .column_list
            .iter()
            .filter(|column| column.primary_key == Some(true))
            .map(|column| {
                Ok(MysqlIndexColumn {
                    name: required_name(&column.name, "columnList.name")?,
                    prefix_length: None,
                    order: None,
                })
            })
            .collect::<LegacyResult<Vec<_>>>()?;
        if !primary_columns.is_empty() {
            indexes.push(MysqlIndexDefinition {
                kind: MysqlIndexKind::Primary,
                name: None,
                columns: primary_columns,
                method: Some(MysqlIndexMethod::Btree),
                comment: None,
            });
        }
    }
    let table_database = first_non_blank(database_name, &table.database_name);
    let table_schema = first_non_blank(schema_name, &table.schema_name);
    Ok(MysqlTableDefinition {
        name: mysql_qualified_name(&table_database, &table_schema, &table.name),
        if_not_exists: false,
        columns,
        indexes,
        engine: non_blank(&table.engine),
        charset: non_blank(&table.charset),
        collation: non_blank(&table.collate),
        comment: Some(table.comment.clone()),
        auto_increment: parse_auto_increment(table.increment_value.as_deref())?,
    })
}

#[allow(clippy::too_many_lines)]
fn mysql_table_alter(
    old_table: &LegacyEditableTable,
    new_table: &LegacyEditableTable,
    database_name: &str,
    schema_name: &str,
) -> LegacyResult<MysqlTableAlter> {
    let table_database = first_non_blank(database_name, &old_table.database_name);
    let table_schema = first_non_blank(schema_name, &old_table.schema_name);
    let table = mysql_qualified_name(&table_database, &table_schema, &old_table.name);
    let rename_to = (!new_table.name.eq_ignore_ascii_case(&old_table.name)).then(|| {
        mysql_qualified_name(
            &first_non_blank(database_name, &new_table.database_name),
            &first_non_blank(schema_name, &new_table.schema_name),
            &new_table.name,
        )
    });
    let mut columns = Vec::new();
    let active_columns = new_table
        .column_list
        .iter()
        .filter(|column| !has_edit_status(column.edit_status.as_deref(), "DELETE"))
        .collect::<Vec<_>>();
    for column in &new_table.column_list {
        let status = column
            .edit_status
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_ascii_uppercase();
        match status.as_str() {
            "ADD" => columns.push(MysqlColumnAlter::Add {
                column: mysql_column_definition(column)?,
                position: mysql_column_position(&active_columns, column),
            }),
            "MODIFY" => columns.push(MysqlColumnAlter::Modify {
                old_name: required_name(
                    column.old_name.as_deref().unwrap_or(&column.name),
                    "columnList.oldName",
                )?,
                column: mysql_column_definition(column)?,
                position: mysql_column_position(&active_columns, column),
            }),
            "DELETE" => columns.push(MysqlColumnAlter::Delete {
                name: required_name(
                    column.old_name.as_deref().unwrap_or(&column.name),
                    "columnList.oldName",
                )?,
            }),
            _ if mysql_column_moved(old_table, &active_columns, column) => {
                columns.push(MysqlColumnAlter::Modify {
                    old_name: required_name(
                        column.old_name.as_deref().unwrap_or(&column.name),
                        "columnList.oldName",
                    )?,
                    column: mysql_column_definition(column)?,
                    position: mysql_column_position(&active_columns, column),
                });
            }
            _ => {}
        }
    }

    let mut indexes = Vec::new();
    let has_explicit_primary_key_change = has_explicit_primary_key_change(new_table);
    for index in &new_table.index_list {
        let status = index
            .edit_status
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_ascii_uppercase();
        let old_index = old_table.index_list.iter().find(|candidate| {
            candidate
                .name
                .eq_ignore_ascii_case(index.old_name.as_deref().unwrap_or(index.name.as_str()))
        });
        match status.as_str() {
            "ADD" => indexes.push(MysqlIndexAlter::Add {
                index: mysql_index_definition(index)?,
            }),
            "MODIFY" => indexes.push(MysqlIndexAlter::Modify {
                old_kind: old_index.map_or_else(|| mysql_index_kind(index), mysql_index_kind),
                old_name: mysql_index_name(old_index.map_or(index, |old_index| old_index))?,
                index: mysql_index_definition(index)?,
            }),
            "DELETE" => {
                let old_index = old_index.unwrap_or(index);
                indexes.push(MysqlIndexAlter::Delete {
                    kind: mysql_index_kind(old_index),
                    name: mysql_index_name(old_index)?,
                });
            }
            _ => {}
        }
    }
    if !has_explicit_primary_key_change
        && let Some(primary_key_change) = inferred_primary_key_change(old_table, new_table)?
    {
        indexes.push(primary_key_change);
    }
    Ok(MysqlTableAlter {
        table,
        rename_to,
        columns,
        indexes,
        engine: changed_non_blank(&old_table.engine, &new_table.engine),
        charset: changed_non_blank(&old_table.charset, &new_table.charset),
        collation: changed_non_blank(&old_table.collate, &new_table.collate),
        comment: (old_table.comment != new_table.comment).then(|| new_table.comment.clone()),
        auto_increment: if old_table.increment_value == new_table.increment_value {
            None
        } else {
            parse_auto_increment(new_table.increment_value.as_deref())?
        },
    })
}

fn has_explicit_primary_key_change(table: &LegacyEditableTable) -> bool {
    table.index_list.iter().any(|index| {
        mysql_index_kind(index) == MysqlIndexKind::Primary
            && matches!(
                index
                    .edit_status
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .to_ascii_uppercase()
                    .as_str(),
                "ADD" | "MODIFY" | "DELETE"
            )
    })
}

fn inferred_primary_key_change(
    old_table: &LegacyEditableTable,
    new_table: &LegacyEditableTable,
) -> LegacyResult<Option<MysqlIndexAlter>> {
    let old_columns = ordered_primary_key_columns(old_table);
    let new_columns = ordered_primary_key_columns(new_table);
    let unchanged = old_columns.len() == new_columns.len()
        && old_columns
            .iter()
            .zip(&new_columns)
            .all(|(old, new)| old.name.eq_ignore_ascii_case(&new.name));
    if unchanged {
        return Ok(None);
    }

    let new_index = || -> LegacyResult<MysqlIndexDefinition> {
        Ok(MysqlIndexDefinition {
            kind: MysqlIndexKind::Primary,
            name: None,
            columns: new_columns
                .iter()
                .map(|column| {
                    Ok(MysqlIndexColumn {
                        name: required_name(&column.name, "columnList.name")?,
                        prefix_length: None,
                        order: None,
                    })
                })
                .collect::<LegacyResult<Vec<_>>>()?,
            method: Some(MysqlIndexMethod::Btree),
            comment: None,
        })
    };

    match (old_columns.is_empty(), new_columns.is_empty()) {
        (true, false) => Ok(Some(MysqlIndexAlter::Add {
            index: new_index()?,
        })),
        (false, true) => Ok(Some(MysqlIndexAlter::Delete {
            kind: MysqlIndexKind::Primary,
            name: None,
        })),
        (false, false) => Ok(Some(MysqlIndexAlter::Modify {
            old_kind: MysqlIndexKind::Primary,
            old_name: None,
            index: new_index()?,
        })),
        (true, true) => Ok(None),
    }
}

fn ordered_primary_key_columns(table: &LegacyEditableTable) -> Vec<&LegacyColumn> {
    let mut columns = table
        .column_list
        .iter()
        .filter(|column| {
            column.primary_key == Some(true)
                && !has_edit_status(column.edit_status.as_deref(), "DELETE")
        })
        .collect::<Vec<_>>();
    columns.sort_by_key(|column| column.primary_key_order);
    columns
}

fn mysql_column_definition(column: &LegacyColumn) -> LegacyResult<MysqlColumnDefinition> {
    let type_name = required_name(&column.column_type, "columnList.columnType")?;
    let (length, scale) =
        mysql_column_dimensions(&type_name, column.column_size, column.decimal_digits);
    Ok(MysqlColumnDefinition {
        name: required_name(&column.name, "columnList.name")?,
        unsigned: type_name.to_ascii_uppercase().contains("UNSIGNED"),
        type_name,
        length,
        scale,
        nullable: column.nullable.unwrap_or(1) != 0,
        default_value: column.default_value.clone(),
        auto_increment: column.auto_increment.unwrap_or(false),
        charset: non_blank(&column.char_set_name),
        collation: non_blank(&column.collation_name),
        comment: non_blank(&column.comment),
        enum_values: parse_enum_values(&column.value),
        on_update_current_timestamp: column.on_update_current_timestamp.unwrap_or(false),
    })
}

fn mysql_column_dimensions(
    type_name: &str,
    column_size: Option<i32>,
    decimal_digits: Option<i32>,
) -> (Option<u32>, Option<u32>) {
    let base_type = type_name
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    match base_type.as_str() {
        "CHAR" | "VARCHAR" | "BINARY" | "VARBINARY" | "BIT" => (positive_u32(column_size), None),
        "DECIMAL" | "NUMERIC" => (
            positive_u32(column_size),
            positive_or_zero_u32(decimal_digits),
        ),
        "DATETIME" | "TIMESTAMP" | "TIME" => (positive_or_zero_u32(decimal_digits), None),
        _ => (None, None),
    }
}

fn mysql_index_definition(index: &LegacyIndex) -> LegacyResult<MysqlIndexDefinition> {
    let kind = mysql_index_kind(index);
    let columns = index
        .column_list
        .iter()
        .filter(|column| !has_edit_status(column.edit_status.as_deref(), "DELETE"))
        .map(|column| {
            Ok(MysqlIndexColumn {
                name: required_name(&column.column_name, "indexList.columnList.columnName")?,
                prefix_length: column.sub_part.and_then(|value| value.try_into().ok()),
                order: match column.asc_or_desc.trim().to_ascii_uppercase().as_str() {
                    "A" | "ASC" => Some(MysqlSortOrder::Asc),
                    "D" | "DESC" => Some(MysqlSortOrder::Desc),
                    _ => None,
                },
            })
        })
        .collect::<LegacyResult<Vec<_>>>()?;
    Ok(MysqlIndexDefinition {
        kind,
        name: mysql_index_name(index)?,
        columns,
        method: match index.method.trim().to_ascii_uppercase().as_str() {
            "BTREE" => Some(MysqlIndexMethod::Btree),
            "HASH" => Some(MysqlIndexMethod::Hash),
            _ => None,
        },
        comment: non_blank(&index.comment),
    })
}

fn mysql_index_kind(index: &LegacyIndex) -> MysqlIndexKind {
    if index.name.eq_ignore_ascii_case("PRIMARY")
        || index.index_type.eq_ignore_ascii_case("PRIMARY")
    {
        MysqlIndexKind::Primary
    } else if index.index_type.eq_ignore_ascii_case("UNIQUE") || index.unique == Some(true) {
        MysqlIndexKind::Unique
    } else if index.index_type.eq_ignore_ascii_case("FULLTEXT") {
        MysqlIndexKind::Fulltext
    } else if index.index_type.eq_ignore_ascii_case("SPATIAL") {
        MysqlIndexKind::Spatial
    } else {
        MysqlIndexKind::Normal
    }
}

fn mysql_index_name(index: &LegacyIndex) -> LegacyResult<Option<String>> {
    if mysql_index_kind(index) == MysqlIndexKind::Primary {
        Ok(None)
    } else {
        Ok(Some(required_name(&index.name, "indexList.name")?))
    }
}

fn mysql_column_position(
    active_columns: &[&LegacyColumn],
    column: &LegacyColumn,
) -> Option<MysqlColumnPosition> {
    let index = active_columns.iter().position(|candidate| {
        std::ptr::eq(*candidate, column) || candidate.name.eq_ignore_ascii_case(&column.name)
    })?;
    if index == 0 {
        Some(MysqlColumnPosition::First)
    } else {
        Some(MysqlColumnPosition::After(
            active_columns[index - 1].name.clone(),
        ))
    }
}

fn mysql_column_moved(
    old_table: &LegacyEditableTable,
    active_columns: &[&LegacyColumn],
    column: &LegacyColumn,
) -> bool {
    let original_name = legacy_column_original_name(column);
    let old_columns = old_table
        .column_list
        .iter()
        .filter(|candidate| !has_edit_status(candidate.edit_status.as_deref(), "DELETE"))
        .collect::<Vec<_>>();
    let Some(old_index) = old_columns.iter().position(|candidate| {
        candidate.name.eq_ignore_ascii_case(original_name)
            || legacy_column_original_name(candidate).eq_ignore_ascii_case(original_name)
    }) else {
        return false;
    };
    let Some(new_index) = active_columns.iter().position(|candidate| {
        std::ptr::eq(*candidate, column)
            || legacy_column_original_name(candidate).eq_ignore_ascii_case(original_name)
    }) else {
        return false;
    };

    let old_previous = old_index
        .checked_sub(1)
        .map(|index| legacy_column_original_name(old_columns[index]));
    let new_previous = new_index
        .checked_sub(1)
        .map(|index| legacy_column_original_name(active_columns[index]));
    match (old_previous, new_previous) {
        (None, None) => false,
        (Some(old), Some(new)) => !old.eq_ignore_ascii_case(new),
        _ => true,
    }
}

fn mysql_reordered_column_names(
    old_table: &LegacyEditableTable,
    new_table: &LegacyEditableTable,
) -> Vec<String> {
    let active_columns = new_table
        .column_list
        .iter()
        .filter(|column| !has_edit_status(column.edit_status.as_deref(), "DELETE"))
        .collect::<Vec<_>>();
    new_table
        .column_list
        .iter()
        .filter(|column| {
            !matches!(
                column
                    .edit_status
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .to_ascii_uppercase()
                    .as_str(),
                "ADD" | "MODIFY" | "DELETE"
            ) && mysql_column_moved(old_table, &active_columns, column)
        })
        .map(|column| legacy_column_original_name(column).to_owned())
        .collect()
}

fn legacy_column_original_name(column: &LegacyColumn) -> &str {
    column
        .old_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(&column.name)
}

fn mysql_view_definition(
    request: &LegacyViewOperationRequest,
) -> LegacyResult<MysqlViewDefinition> {
    if request.use_if_not_exists {
        return Err(LegacyFailure::invalid(
            "invalid_mysql_ddl",
            "MySQL does not support CREATE VIEW IF NOT EXISTS",
        ));
    }
    let view_name = first_non_blank(&request.view_name, &request.table_name);
    Ok(MysqlViewDefinition {
        name: mysql_qualified_name(&request.database_name, &request.schema_name, &view_name),
        columns: request.view_attributes.clone(),
        use_or_replace: request.use_or_replace || request.is_modify.unwrap_or(false),
        algorithm: match request.algorithm.trim().to_ascii_uppercase().as_str() {
            "" => None,
            "UNDEFINED" => Some(MysqlViewAlgorithm::Undefined),
            "MERGE" => Some(MysqlViewAlgorithm::Merge),
            "TEMPTABLE" => Some(MysqlViewAlgorithm::Temptable),
            _ => {
                return Err(LegacyFailure::invalid(
                    "invalid_mysql_ddl",
                    "algorithm must be UNDEFINED, MERGE, or TEMPTABLE",
                ));
            }
        },
        definer: parse_view_definer(&request.definer)?,
        sql_security: match request.security.trim().to_ascii_uppercase().as_str() {
            "" => None,
            "DEFINER" => Some(MysqlViewSecurity::Definer),
            "INVOKER" => Some(MysqlViewSecurity::Invoker),
            _ => {
                return Err(LegacyFailure::invalid(
                    "invalid_mysql_ddl",
                    "security must be DEFINER or INVOKER",
                ));
            }
        },
        check_option: match request.check_option.trim().to_ascii_uppercase().as_str() {
            "" | "NONE" => None,
            "CASCADED" => Some(MysqlViewCheckOption::Cascaded),
            "LOCAL" => Some(MysqlViewCheckOption::Local),
            _ => {
                return Err(LegacyFailure::invalid(
                    "invalid_mysql_ddl",
                    "checkOption must be CASCADED or LOCAL",
                ));
            }
        },
        body: required_name(&request.view_body, "viewBody")?,
    })
}

fn parse_view_definer(value: &str) -> LegacyResult<Option<MysqlViewDefiner>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let Some((user, host)) = value.split_once('@') else {
        return Err(LegacyFailure::invalid(
            "invalid_mysql_ddl",
            "definer must use the user@host form",
        ));
    };
    let trim_part = |part: &str| part.trim().trim_matches(['\'', '"', '`']).to_owned();
    Ok(Some(MysqlViewDefiner {
        user: required_name(&trim_part(user), "definer user")?,
        host: required_name(&trim_part(host), "definer host")?,
    }))
}

async fn execute_generated_action(
    application: &Application,
    data_source_id: LegacyIdentifier,
    database_name: &str,
    schema_name: &str,
    table_name: &str,
    sql: String,
) -> LegacyResult<()> {
    let result = execute_ddl(
        application,
        &LegacySqlExecuteRequest {
            data_source_id,
            data_source_name: String::new(),
            database_name: database_name.to_owned(),
            schema_name: schema_name.to_owned(),
            database_type: "MYSQL".to_owned(),
            table_name: table_name.to_owned(),
            sql,
            single: true,
            page_no: DEFAULT_PAGE_NO,
            page_size: DEFAULT_SQL_PAGE_SIZE,
            page_size_all: false,
            console_id: None,
            apply_id: None,
            result_set_id: None,
            error_continue: Some(false),
            explain: false,
        },
    )
    .await?;
    if result.success {
        Ok(())
    } else {
        Err(LegacyFailure {
            code: "mysql_ddl_execution_failed".to_owned(),
            message: if result.message.trim().is_empty() {
                "The MySQL DDL statement failed".to_owned()
            } else {
                result.message
            },
        })
    }
}

fn validate_delete_confirmation(expected: &str, actual: &str) -> LegacyResult<()> {
    if expected == actual.trim() {
        Ok(())
    } else {
        Err(LegacyFailure::invalid(
            "database_object_delete_confirmation_mismatch",
            "confirmName must exactly match the database object name",
        ))
    }
}

fn required_name(value: &str, field: &'static str) -> LegacyResult<String> {
    let value = value.trim();
    if value.is_empty() {
        Err(LegacyFailure {
            code: "invalid_legacy_request".to_owned(),
            message: format!("{field} is required"),
        })
    } else {
        Ok(value.to_owned())
    }
}

fn first_non_blank(primary: &str, fallback: &str) -> String {
    if primary.trim().is_empty() {
        fallback.trim().to_owned()
    } else {
        primary.trim().to_owned()
    }
}

fn has_edit_status(status: Option<&str>, expected: &str) -> bool {
    status.is_some_and(|status| status.trim().eq_ignore_ascii_case(expected))
}

fn changed_non_blank(old: &str, new: &str) -> Option<String> {
    (old != new)
        .then(|| new.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn positive_u32(value: Option<i32>) -> Option<u32> {
    value
        .filter(|value| *value > 0)
        .and_then(|value| value.try_into().ok())
}

fn positive_or_zero_u32(value: Option<i32>) -> Option<u32> {
    value
        .filter(|value| *value >= 0)
        .and_then(|value| value.try_into().ok())
}

fn parse_auto_increment(value: Option<&str>) -> LegacyResult<Option<u64>> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value.trim().parse::<u64>().map_err(|_| {
                LegacyFailure::invalid(
                    "invalid_mysql_ddl",
                    "incrementValue must be a positive integer",
                )
            })
        })
        .transpose()
}

fn parse_enum_values(value: &str) -> Vec<String> {
    let value = value.trim();
    let value = value
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
        .unwrap_or(value);
    let mut values = Vec::new();
    let mut chars = value.chars().peekable();
    while chars.peek().is_some() {
        while matches!(chars.peek(), Some(character) if character.is_whitespace() || *character == ',')
        {
            chars.next();
        }
        let Some(first) = chars.next() else {
            break;
        };
        let mut parsed = String::new();
        let quoted = matches!(first, '\'' | '"');
        if quoted {
            let quote = first;
            while let Some(character) = chars.next() {
                if character == '\\' {
                    if let Some(escaped) = chars.next() {
                        parsed.push(escaped);
                    }
                } else if character == quote {
                    if chars.peek() == Some(&quote) {
                        chars.next();
                        parsed.push(quote);
                    } else {
                        break;
                    }
                } else {
                    parsed.push(character);
                }
            }
            while matches!(chars.peek(), Some(character) if character.is_whitespace()) {
                chars.next();
            }
            if chars.peek() == Some(&',') {
                chars.next();
            }
        } else {
            parsed.push(first);
            for character in chars.by_ref() {
                if character == ',' {
                    break;
                }
                parsed.push(character);
            }
            parsed.truncate(parsed.trim_end().len());
        }
        if quoted || !parsed.is_empty() {
            values.push(parsed);
        }
    }
    values
}

fn mysql_enum_set_editor_value(extent: &str) -> String {
    let extent = extent.trim();
    extent
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
        .unwrap_or(extent)
        .to_owned()
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

fn table_matches_search(table: &LegacyTable, search_key: &str) -> bool {
    if search_key.trim().is_empty() {
        return true;
    }
    let search_key = search_key.to_lowercase();
    table.name.to_lowercase().contains(&search_key)
        || (!table.comment.trim().is_empty() && table.comment.to_lowercase().contains(&search_key))
}

fn validate_metadata_page(query: &LegacyTableListQuery) -> LegacyResult<()> {
    if !(1..=MAX_METADATA_PAGE_SIZE).contains(&query.page_size) {
        return Err(LegacyFailure::invalid(
            "invalid_legacy_request",
            "pageSize must be between 1 and 100000",
        ));
    }
    Ok(())
}

fn full_page<T>(items: Vec<T>) -> LegacyPage<T> {
    let total = items.len();
    LegacyPage {
        data: items,
        page_no: 1,
        page_size: u32::try_from(total).unwrap_or(u32::MAX),
        total,
        has_next_page: false,
    }
}

/// Dispatches a historical Community request without depending on Axum.
///
/// Tauri IPC can pass its `requestUrl`, `method`, and `message` fields here and
/// return the resulting JSON value unchanged.
pub fn dispatch(
    application: &Application,
    request: LegacyDispatchRequest,
) -> impl Future<Output = serde_json::Value> + Send + '_ {
    Box::pin(dispatch_inner(application, request))
}

#[allow(clippy::too_many_lines)]
async fn dispatch_inner(
    application: &Application,
    request: LegacyDispatchRequest,
) -> serde_json::Value {
    let path = request
        .request_url
        .split('?')
        .next()
        .unwrap_or(request.request_url.as_str());
    let method = request.method.to_ascii_lowercase();
    let counted_response = LEGACY_COUNTED_PATHS.contains(&path);
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
        ("post", "/api/operation/saved/create") => {
            match decode::<LegacySavedConsoleCreateRequest>(request.message) {
                Ok(body) => serialized(create_saved_console(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/operation/saved/list") => {
            match decode::<LegacySavedConsoleListQuery>(request.message) {
                Ok(query) => serialized(list_saved_consoles(application, &query).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/operation/saved") => match decode::<LegacyIdQuery>(request.message) {
            Ok(query) => match legacy_console_id(&query.id) {
                Ok(id) => serialized(get_saved_console(application, id).await),
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        },
        ("post" | "put", "/api/operation/saved/update") => {
            match decode::<LegacySavedConsoleUpdateRequest>(request.message) {
                Ok(body) => serialized(update_saved_console(application, body).await),
                Err(error) => Err(error),
            }
        }
        ("delete", "/api/operation/saved") => match decode::<LegacyIdQuery>(request.message) {
            Ok(query) => match legacy_console_id(&query.id) {
                Ok(id) => serialized(delete_saved_console(application, id).await),
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        },
        ("post", "/api/operation/log/create") => {
            match decode::<LegacyOperationLogCreateRequest>(request.message) {
                Ok(body) => serialized(create_operation_log(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/operation/log/list") => {
            match decode::<LegacyOperationLogListQuery>(request.message) {
                Ok(query) => serialized(list_operation_logs(application, &query).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/operation/log") => match decode::<LegacyIdQuery>(request.message) {
            Ok(query) => match legacy_console_id(&query.id) {
                Ok(id) => serialized(get_operation_log(application, id).await),
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        },
        ("get", "/api/namespaces/tree_list") => serialized(namespace_tree(application).await),
        ("get", "/api/rdb/database/list") => match decode::<LegacyMetadataQuery>(request.message) {
            Ok(query) => serialized(list_databases(application, &query).await),
            Err(error) => Err(error),
        },
        ("get", "/api/rdb/schema/list") => match decode::<LegacyMetadataQuery>(request.message) {
            Ok(query) => serialized(list_schemas(application, &query).await),
            Err(error) => Err(error),
        },
        ("get", "/api/rdb/table/table_meta") => {
            match decode::<LegacyMetadataQuery>(request.message) {
                Ok(query) => serialized(table_editor_meta(application, &query).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/rdb/table/query") => {
            match decode::<LegacyTableDetailQuery>(request.message) {
                Ok(query) => serialized(Box::pin(get_editable_table(application, &query)).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/rdb/ddl/export" | "/api/rdb/table/export") => {
            match decode::<LegacyTableDetailQuery>(request.message) {
                Ok(query) => serialized(export_table_ddl(application, &query).await),
                Err(error) => Err(error),
            }
        }
        (
            "get",
            "/api/rdb/ddl/create/example"
            | "/api/rdb/ddl/update/example"
            | "/api/rdb/table/create/example"
            | "/api/rdb/table/update/example",
        ) => match decode::<LegacyTableDdlExampleQuery>(request.message) {
            Ok(query) => serialized(mysql_table_ddl_example(&query)),
            Err(error) => Err(error),
        },
        ("get", "/api/rdb/table/list") => match decode::<LegacyTableListQuery>(request.message) {
            Ok(query) => serialized(list_tables(application, &query).await),
            Err(error) => Err(error),
        },
        ("get", "/api/rdb/table/table_list") => {
            match decode::<LegacyTableListQuery>(request.message) {
                Ok(query) => serialized(list_simple_tables(application, &query).await),
                Err(error) => Err(error),
            }
        }
        (
            "get",
            "/api/rdb/table/column_list" | "/api/rdb/ddl/column_list" | "/api/rdb/view/column_list",
        ) => match decode::<LegacyTableDetailQuery>(request.message) {
            Ok(query) => serialized(list_columns(application, &query).await),
            Err(error) => Err(error),
        },
        ("get", "/api/rdb/table/index_list" | "/api/rdb/ddl/index_list") => {
            match decode::<LegacyTableDetailQuery>(request.message) {
                Ok(query) => serialized(list_indexes(application, &query).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/rdb/table/key_list" | "/api/rdb/ddl/key_list") => {
            match decode::<LegacyTableDetailQuery>(request.message) {
                Ok(query) => serialized(list_keys(application, &query).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/rdb/view/list") => match decode::<LegacyTableListQuery>(request.message) {
            Ok(query) => serialized(list_views(application, &query).await),
            Err(error) => Err(error),
        },
        ("get", "/api/rdb/view/detail") => {
            match decode::<LegacyTableDetailQuery>(request.message) {
                Ok(query) => serialized(get_view(application, &query).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/rdb/view/query") => match decode::<LegacyTableDetailQuery>(request.message) {
            Ok(query) => serialized(Box::pin(get_editable_view(application, &query)).await),
            Err(error) => Err(error),
        },
        ("get", "/api/rdb/view/view_meta") => {
            match decode::<LegacyViewOperationRequest>(request.message) {
                Ok(query) => serialized(Box::pin(view_editor_meta(application, &query)).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/rdb/function/list") => {
            match decode::<LegacyTableListQuery>(request.message) {
                Ok(query) => serialized(list_functions(application, &query).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/rdb/function/detail") => {
            match decode::<LegacyFunctionDetailQuery>(request.message) {
                Ok(query) => serialized(get_function(application, &query).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/rdb/procedure/list") => {
            match decode::<LegacyTableListQuery>(request.message) {
                Ok(query) => serialized(list_procedures(application, &query).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/rdb/procedure/detail") => {
            match decode::<LegacyProcedureDetailQuery>(request.message) {
                Ok(query) => serialized(get_procedure(application, &query).await),
                Err(error) => Err(error),
            }
        }
        ("get", "/api/rdb/trigger/list") => match decode::<LegacyTableListQuery>(request.message) {
            Ok(query) => serialized(list_triggers(application, &query).await),
            Err(error) => Err(error),
        },
        ("get", "/api/rdb/trigger/detail") => {
            match decode::<LegacyTriggerDetailQuery>(request.message) {
                Ok(query) => serialized(get_trigger(application, &query).await),
                Err(error) => Err(error),
            }
        }
        ("post" | "put", "/api/rdb/dml/execute_table") => {
            match decode::<LegacyTablePreviewRequest>(request.message) {
                Ok(body) => serialized(preview_table(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post" | "put", "/api/rdb/dml/execute") => {
            match decode::<LegacySqlExecuteRequest>(request.message) {
                Ok(body) => serialized(execute_sql(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post" | "put", "/api/rdb/dml/execute_ddl" | "/api/rdb/dml/execute_update") => {
            match decode::<LegacySqlExecuteRequest>(request.message) {
                Ok(body) => serialized(execute_ddl(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post" | "put", "/api/rdb/dml/get_update_sql") => {
            match decode::<LegacyGridUpdateRequest>(request.message) {
                Ok(body) => serialized(build_grid_update_sql(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post" | "put", "/api/rdb/dml/copy_update_sql") => {
            match decode::<LegacyGridUpdateRequest>(request.message) {
                Ok(body) => serialized(build_grid_copy_sql(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post" | "put", "/api/rdb/dml/copy_in_values_sql") => {
            match decode::<LegacyGridUpdateRequest>(request.message) {
                Ok(body) => serialized(build_grid_in_values(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post" | "put", "/api/rdb/dml/count") => {
            match decode::<LegacySqlExecuteRequest>(request.message) {
                Ok(body) => serialized(count_mysql_rows(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/rdb/table/modify/sql") => {
            match decode::<LegacyTableModifyRequest>(request.message) {
                Ok(body) => serialized(build_table_modify_sql(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/rdb/ddl/delete") => {
            match decode::<LegacyTableOperationRequest>(request.message) {
                Ok(body) => serialized(drop_table(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/rdb/table/truncate") => {
            match decode::<LegacyTableOperationRequest>(request.message) {
                Ok(body) => serialized(truncate_table(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/rdb/table/copy") => {
            match decode::<LegacyTableCopyRequest>(request.message) {
                Ok(body) => serialized(copy_table(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/rdb/database/create_database_sql") => {
            match decode::<LegacyDatabaseDefinitionRequest>(request.message) {
                Ok(body) => serialized(build_create_database_sql(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/rdb/schema/create_schema_sql") => {
            match decode::<LegacySchemaDefinitionRequest>(request.message) {
                Ok(body) => serialized(build_create_schema_sql(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/rdb/delete/database/prepare") => {
            match decode::<LegacyDeleteObjectRequest>(request.message) {
                Ok(body) => serialized(prepare_database_delete(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/rdb/delete/database/execute") => {
            match decode::<LegacyDeleteObjectRequest>(request.message) {
                Ok(body) => serialized(execute_database_delete(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/rdb/delete/schema/prepare") => {
            match decode::<LegacyDeleteObjectRequest>(request.message) {
                Ok(body) => serialized(prepare_schema_delete(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/rdb/delete/schema/execute") => {
            match decode::<LegacyDeleteObjectRequest>(request.message) {
                Ok(body) => serialized(execute_schema_delete(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/rdb/view/modify/sql") => {
            match decode::<LegacyViewOperationRequest>(request.message) {
                Ok(body) => serialized(build_view_modify_sql(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/rdb/view/drop") => {
            match decode::<LegacyViewOperationRequest>(request.message) {
                Ok(body) => serialized(drop_view(application, &body).await),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/rdb/cell/value") => {
            match decode::<LegacyLargeCellValueRequest>(request.message) {
                Ok(body) => serialized(read_large_cell_value(application, &body)),
                Err(error) => Err(error),
            }
        }
        ("post", "/api/rdb/cell/download_path") => {
            match decode::<LegacyLargeCellDownloadRequest>(request.message) {
                Ok(body) => serialized(download_large_cell_value_to_path(application, &body).await),
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
    if counted_response {
        counted_envelope_value(result)
    } else {
        envelope_value(result)
    }
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
    "/api/operation/saved/create",
    "/api/operation/saved/list",
    "/api/operation/saved",
    "/api/operation/saved/update",
    "/api/operation/log/create",
    "/api/operation/log/list",
    "/api/operation/log",
    "/api/namespaces/tree_list",
    "/api/rdb/database/list",
    "/api/rdb/database/create_database_sql",
    "/api/rdb/schema/list",
    "/api/rdb/schema/create_schema_sql",
    "/api/rdb/table/list",
    "/api/rdb/table/table_meta",
    "/api/rdb/table/query",
    "/api/rdb/table/export",
    "/api/rdb/table/create/example",
    "/api/rdb/table/update/example",
    "/api/rdb/table/modify/sql",
    "/api/rdb/table/truncate",
    "/api/rdb/table/copy",
    "/api/rdb/table/table_list",
    "/api/rdb/table/column_list",
    "/api/rdb/table/index_list",
    "/api/rdb/table/key_list",
    "/api/rdb/ddl/column_list",
    "/api/rdb/ddl/index_list",
    "/api/rdb/ddl/key_list",
    "/api/rdb/ddl/export",
    "/api/rdb/ddl/create/example",
    "/api/rdb/ddl/update/example",
    "/api/rdb/ddl/delete",
    "/api/rdb/delete/database/prepare",
    "/api/rdb/delete/database/execute",
    "/api/rdb/delete/schema/prepare",
    "/api/rdb/delete/schema/execute",
    "/api/rdb/view/list",
    "/api/rdb/view/column_list",
    "/api/rdb/view/detail",
    "/api/rdb/view/query",
    "/api/rdb/view/view_meta",
    "/api/rdb/view/modify/sql",
    "/api/rdb/view/drop",
    "/api/rdb/function/list",
    "/api/rdb/function/detail",
    "/api/rdb/procedure/list",
    "/api/rdb/procedure/detail",
    "/api/rdb/trigger/list",
    "/api/rdb/trigger/detail",
    "/api/rdb/dml/execute",
    "/api/rdb/dml/execute_ddl",
    "/api/rdb/dml/execute_update",
    "/api/rdb/dml/get_update_sql",
    "/api/rdb/dml/copy_update_sql",
    "/api/rdb/dml/copy_in_values_sql",
    "/api/rdb/dml/count",
    "/api/rdb/dml/execute_table",
    "/api/rdb/cell/value",
    "/api/rdb/cell/download",
    "/api/rdb/cell/download_path",
];

const LEGACY_COUNTED_PATHS: &[&str] = &[
    "/api/rdb/ddl/column_list",
    "/api/rdb/ddl/index_list",
    "/api/rdb/ddl/key_list",
    "/api/rdb/view/column_list",
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

fn counted_envelope_value(result: LegacyResult<serde_json::Value>) -> serde_json::Value {
    let envelope: LegacyCountedEnvelope<serde_json::Value> = match result {
        Ok(serde_json::Value::Array(data)) => LegacyCountedEnvelope::success(data),
        Ok(_) => LegacyCountedEnvelope::failure(LegacyFailure {
            code: "internal_error".to_owned(),
            message: "The operation could not be completed".to_owned(),
        }),
        Err(error) => LegacyCountedEnvelope::failure(error),
    };
    serde_json::to_value(envelope).unwrap_or_else(|_| {
        serde_json::json!({
            "success": false,
            "data": null,
            "total": null,
            "errorCode": "internal_error",
            "errorMessage": "The operation could not be completed"
        })
    })
}

#[allow(clippy::too_many_lines)]
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
        .route(
            "/api/operation/saved/create",
            post(create_saved_console_handler),
        )
        .route(
            "/api/operation/saved/list",
            get(list_saved_consoles_handler),
        )
        .route(
            "/api/operation/saved",
            get(get_saved_console_handler).delete(delete_saved_console_handler),
        )
        .route(
            "/api/operation/saved/update",
            post(update_saved_console_handler).put(update_saved_console_handler),
        )
        .route(
            "/api/operation/log/create",
            post(create_operation_log_handler),
        )
        .route("/api/operation/log/list", get(list_operation_logs_handler))
        .route("/api/operation/log", get(get_operation_log_handler))
        .route("/api/namespaces/tree_list", get(namespace_tree_handler))
        .route("/api/rdb/database/list", get(database_list_handler))
        .route(
            "/api/rdb/database/create_database_sql",
            post(create_database_sql_handler),
        )
        .route("/api/rdb/schema/list", get(schema_list_handler))
        .route(
            "/api/rdb/schema/create_schema_sql",
            post(create_schema_sql_handler),
        )
        .route("/api/rdb/table/list", get(table_list_handler))
        .route("/api/rdb/table/table_meta", get(table_meta_handler))
        .route("/api/rdb/table/query", get(table_query_handler))
        .route("/api/rdb/table/export", get(table_ddl_export_handler))
        .route(
            "/api/rdb/table/create/example",
            get(table_ddl_example_handler),
        )
        .route(
            "/api/rdb/table/update/example",
            get(table_ddl_example_handler),
        )
        .route("/api/rdb/table/modify/sql", post(table_modify_sql_handler))
        .route("/api/rdb/table/truncate", post(table_truncate_handler))
        .route("/api/rdb/table/copy", post(table_copy_handler))
        .route("/api/rdb/table/table_list", get(simple_table_list_handler))
        .route("/api/rdb/table/column_list", get(table_column_list_handler))
        .route("/api/rdb/table/index_list", get(table_index_list_handler))
        .route("/api/rdb/table/key_list", get(table_key_list_handler))
        .route("/api/rdb/ddl/column_list", get(ddl_column_list_handler))
        .route("/api/rdb/ddl/index_list", get(ddl_index_list_handler))
        .route("/api/rdb/ddl/key_list", get(ddl_key_list_handler))
        .route("/api/rdb/ddl/export", get(table_ddl_export_handler))
        .route(
            "/api/rdb/ddl/create/example",
            get(table_ddl_example_handler),
        )
        .route(
            "/api/rdb/ddl/update/example",
            get(table_ddl_example_handler),
        )
        .route("/api/rdb/ddl/delete", post(table_drop_handler))
        .route(
            "/api/rdb/delete/database/prepare",
            post(database_delete_prepare_handler),
        )
        .route(
            "/api/rdb/delete/database/execute",
            post(database_delete_execute_handler),
        )
        .route(
            "/api/rdb/delete/schema/prepare",
            post(schema_delete_prepare_handler),
        )
        .route(
            "/api/rdb/delete/schema/execute",
            post(schema_delete_execute_handler),
        )
        .route("/api/rdb/view/list", get(view_list_handler))
        .route("/api/rdb/view/column_list", get(view_column_list_handler))
        .route("/api/rdb/view/detail", get(view_detail_handler))
        .route("/api/rdb/view/query", get(view_query_handler))
        .route("/api/rdb/view/view_meta", get(view_meta_handler))
        .route("/api/rdb/view/modify/sql", post(view_modify_sql_handler))
        .route("/api/rdb/view/drop", post(view_drop_handler))
        .route("/api/rdb/function/list", get(function_list_handler))
        .route("/api/rdb/function/detail", get(function_detail_handler))
        .route("/api/rdb/procedure/list", get(procedure_list_handler))
        .route("/api/rdb/procedure/detail", get(procedure_detail_handler))
        .route("/api/rdb/trigger/list", get(trigger_list_handler))
        .route("/api/rdb/trigger/detail", get(trigger_detail_handler))
        .route(
            "/api/rdb/dml/execute",
            post(sql_execute_handler).put(sql_execute_handler),
        )
        .route(
            "/api/rdb/dml/execute_ddl",
            post(sql_execute_ddl_handler).put(sql_execute_ddl_handler),
        )
        .route(
            "/api/rdb/dml/execute_update",
            post(sql_execute_ddl_handler).put(sql_execute_ddl_handler),
        )
        .route(
            "/api/rdb/dml/get_update_sql",
            post(grid_update_sql_handler).put(grid_update_sql_handler),
        )
        .route(
            "/api/rdb/dml/copy_update_sql",
            post(grid_copy_sql_handler).put(grid_copy_sql_handler),
        )
        .route(
            "/api/rdb/dml/copy_in_values_sql",
            post(grid_in_values_handler).put(grid_in_values_handler),
        )
        .route(
            "/api/rdb/dml/count",
            post(sql_count_handler).put(sql_count_handler),
        )
        .route(
            "/api/rdb/dml/execute_table",
            post(table_preview_handler).put(table_preview_handler),
        )
        .route("/api/rdb/cell/value", post(large_cell_value_handler))
        .route("/api/rdb/cell/download", post(large_cell_download_handler))
        .route(
            "/api/rdb/cell/download_path",
            post(large_cell_download_path_handler),
        )
        .layer(middleware::map_response(legacy_bad_request_envelope))
}

fn envelope<T>(result: LegacyResult<T>) -> Json<LegacyEnvelope<T>> {
    Json(match result {
        Ok(data) => LegacyEnvelope::success(data),
        Err(error) => LegacyEnvelope::failure(error),
    })
}

async fn legacy_bad_request_envelope(response: Response) -> Response {
    if !matches!(
        response.status(),
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
    ) {
        return response;
    }
    (
        StatusCode::OK,
        envelope::<serde_json::Value>(Err(LegacyFailure::invalid(
            "invalid_legacy_request",
            "The Community request query is invalid",
        ))),
    )
        .into_response()
}

fn counted_envelope<T>(result: LegacyResult<Vec<T>>) -> Json<LegacyCountedEnvelope<T>> {
    Json(match result {
        Ok(data) => LegacyCountedEnvelope::success(data),
        Err(error) => LegacyCountedEnvelope::failure(error),
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

async fn create_saved_console_handler(
    State(application): State<Application>,
    Json(request): Json<LegacySavedConsoleCreateRequest>,
) -> Json<LegacyEnvelope<i64>> {
    envelope(create_saved_console(&application, &request).await)
}

async fn list_saved_consoles_handler(
    State(application): State<Application>,
    Query(query): Query<LegacySavedConsoleListQuery>,
) -> Json<LegacyEnvelope<LegacyPage<LegacySavedConsoleResponse>>> {
    envelope(list_saved_consoles(&application, &query).await)
}

async fn get_saved_console_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyIdQuery>,
) -> Json<LegacyEnvelope<Option<LegacySavedConsoleResponse>>> {
    envelope(match legacy_console_id(&query.id) {
        Ok(id) => get_saved_console(&application, id).await,
        Err(error) => Err(error),
    })
}

async fn update_saved_console_handler(
    State(application): State<Application>,
    Json(request): Json<LegacySavedConsoleUpdateRequest>,
) -> Json<LegacyEnvelope<()>> {
    envelope(update_saved_console(&application, request).await)
}

async fn delete_saved_console_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyIdQuery>,
) -> Json<LegacyEnvelope<()>> {
    envelope(match legacy_console_id(&query.id) {
        Ok(id) => delete_saved_console(&application, id).await,
        Err(error) => Err(error),
    })
}

async fn create_operation_log_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyOperationLogCreateRequest>,
) -> Json<LegacyEnvelope<i64>> {
    envelope(create_operation_log(&application, &request).await)
}

async fn list_operation_logs_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyOperationLogListQuery>,
) -> Json<LegacyEnvelope<LegacyPage<LegacyOperationLogResponse>>> {
    envelope(list_operation_logs(&application, &query).await)
}

async fn get_operation_log_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyIdQuery>,
) -> Json<LegacyEnvelope<LegacyOperationLogResponse>> {
    envelope(match legacy_console_id(&query.id) {
        Ok(id) => get_operation_log(&application, id).await,
        Err(error) => Err(error),
    })
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

async fn create_database_sql_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyDatabaseDefinitionRequest>,
) -> Json<LegacyEnvelope<LegacySqlResponse>> {
    envelope(build_create_database_sql(&application, &request).await)
}

async fn schema_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyMetadataQuery>,
) -> Json<LegacyEnvelope<Vec<LegacySchema>>> {
    envelope(list_schemas(&application, &query).await)
}

async fn create_schema_sql_handler(
    State(application): State<Application>,
    Json(request): Json<LegacySchemaDefinitionRequest>,
) -> Json<LegacyEnvelope<LegacySqlResponse>> {
    envelope(build_create_schema_sql(&application, &request).await)
}

async fn table_meta_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyMetadataQuery>,
) -> Json<LegacyEnvelope<MysqlTableEditorMeta>> {
    envelope(table_editor_meta(&application, &query).await)
}

async fn table_query_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableDetailQuery>,
) -> Json<LegacyEnvelope<LegacyEditableTable>> {
    envelope(Box::pin(get_editable_table(&application, &query)).await)
}

async fn table_ddl_export_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableDetailQuery>,
) -> Json<LegacyEnvelope<String>> {
    envelope(export_table_ddl(&application, &query).await)
}

async fn table_ddl_example_handler(
    Query(query): Query<LegacyTableDdlExampleQuery>,
) -> Json<LegacyEnvelope<Option<String>>> {
    envelope(mysql_table_ddl_example(&query))
}

async fn table_modify_sql_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyTableModifyRequest>,
) -> Json<LegacyEnvelope<Vec<LegacySqlResponse>>> {
    envelope(build_table_modify_sql(&application, &request).await)
}

async fn table_drop_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyTableOperationRequest>,
) -> Json<LegacyEnvelope<()>> {
    envelope(drop_table(&application, &request).await)
}

async fn table_truncate_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyTableOperationRequest>,
) -> Json<LegacyEnvelope<()>> {
    envelope(truncate_table(&application, &request).await)
}

async fn table_copy_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyTableCopyRequest>,
) -> Json<LegacyEnvelope<()>> {
    envelope(copy_table(&application, &request).await)
}

async fn database_delete_prepare_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyDeleteObjectRequest>,
) -> Json<LegacyEnvelope<LegacyDeletePrepareResponse>> {
    envelope(prepare_database_delete(&application, &request).await)
}

async fn database_delete_execute_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyDeleteObjectRequest>,
) -> Json<LegacyEnvelope<()>> {
    envelope(execute_database_delete(&application, &request).await)
}

async fn schema_delete_prepare_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyDeleteObjectRequest>,
) -> Json<LegacyEnvelope<LegacyDeletePrepareResponse>> {
    envelope(prepare_schema_delete(&application, &request).await)
}

async fn schema_delete_execute_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyDeleteObjectRequest>,
) -> Json<LegacyEnvelope<()>> {
    envelope(execute_schema_delete(&application, &request).await)
}

async fn table_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableListQuery>,
) -> Json<LegacyEnvelope<LegacyPage<LegacyTable>>> {
    envelope(list_tables(&application, &query).await)
}

async fn simple_table_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableListQuery>,
) -> Json<LegacyEnvelope<Vec<LegacySimpleTable>>> {
    envelope(list_simple_tables(&application, &query).await)
}

async fn table_column_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableDetailQuery>,
) -> Json<LegacyEnvelope<Vec<LegacyColumn>>> {
    envelope(list_columns(&application, &query).await)
}

async fn table_index_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableDetailQuery>,
) -> Json<LegacyEnvelope<Vec<LegacyIndex>>> {
    envelope(list_indexes(&application, &query).await)
}

async fn table_key_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableDetailQuery>,
) -> Json<LegacyEnvelope<Vec<LegacyIndex>>> {
    envelope(list_keys(&application, &query).await)
}

async fn ddl_column_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableDetailQuery>,
) -> Json<LegacyCountedEnvelope<LegacyColumn>> {
    counted_envelope(list_columns(&application, &query).await)
}

async fn ddl_index_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableDetailQuery>,
) -> Json<LegacyCountedEnvelope<LegacyIndex>> {
    counted_envelope(list_indexes(&application, &query).await)
}

async fn ddl_key_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableDetailQuery>,
) -> Json<LegacyCountedEnvelope<LegacyIndex>> {
    counted_envelope(list_keys(&application, &query).await)
}

async fn view_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableListQuery>,
) -> Json<LegacyEnvelope<LegacyPage<LegacyTable>>> {
    envelope(list_views(&application, &query).await)
}

async fn view_column_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableDetailQuery>,
) -> Json<LegacyCountedEnvelope<LegacyColumn>> {
    counted_envelope(list_columns(&application, &query).await)
}

async fn view_detail_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableDetailQuery>,
) -> Json<LegacyEnvelope<LegacyTable>> {
    envelope(get_view(&application, &query).await)
}

async fn view_query_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableDetailQuery>,
) -> Json<LegacyEnvelope<LegacyEditableTable>> {
    envelope(Box::pin(get_editable_view(&application, &query)).await)
}

async fn view_meta_handler(
    State(application): State<Application>,
    Query(request): Query<LegacyViewOperationRequest>,
) -> Json<LegacyEnvelope<LegacyViewMetaResponse>> {
    envelope(Box::pin(view_editor_meta(&application, &request)).await)
}

async fn view_modify_sql_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyViewOperationRequest>,
) -> Json<LegacyEnvelope<String>> {
    envelope(build_view_modify_sql(&application, &request).await)
}

async fn view_drop_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyViewOperationRequest>,
) -> Json<LegacyEnvelope<()>> {
    envelope(drop_view(&application, &request).await)
}

async fn function_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableListQuery>,
) -> Json<LegacyEnvelope<LegacyPage<LegacyFunction>>> {
    envelope(list_functions(&application, &query).await)
}

async fn function_detail_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyFunctionDetailQuery>,
) -> Json<LegacyEnvelope<LegacyFunction>> {
    envelope(get_function(&application, &query).await)
}

async fn procedure_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableListQuery>,
) -> Json<LegacyEnvelope<LegacyPage<LegacyProcedure>>> {
    envelope(list_procedures(&application, &query).await)
}

async fn procedure_detail_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyProcedureDetailQuery>,
) -> Json<LegacyEnvelope<LegacyProcedure>> {
    envelope(get_procedure(&application, &query).await)
}

async fn trigger_list_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTableListQuery>,
) -> Json<LegacyEnvelope<LegacyPage<LegacyTrigger>>> {
    envelope(list_triggers(&application, &query).await)
}

async fn trigger_detail_handler(
    State(application): State<Application>,
    Query(query): Query<LegacyTriggerDetailQuery>,
) -> Json<LegacyEnvelope<LegacyTrigger>> {
    envelope(get_trigger(&application, &query).await)
}

async fn table_preview_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyTablePreviewRequest>,
) -> Json<LegacyEnvelope<Vec<LegacyManageResult>>> {
    envelope(preview_table(&application, &request).await)
}

async fn sql_execute_handler(
    State(application): State<Application>,
    Json(request): Json<LegacySqlExecuteRequest>,
) -> Json<LegacyEnvelope<Vec<LegacyManageResult>>> {
    envelope(execute_sql(&application, &request).await)
}

async fn sql_execute_ddl_handler(
    State(application): State<Application>,
    Json(request): Json<LegacySqlExecuteRequest>,
) -> Json<LegacyEnvelope<LegacyManageResult>> {
    envelope(execute_ddl(&application, &request).await)
}

async fn grid_update_sql_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyGridUpdateRequest>,
) -> Json<LegacyEnvelope<String>> {
    envelope(build_grid_update_sql(&application, &request).await)
}

async fn grid_copy_sql_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyGridUpdateRequest>,
) -> Json<LegacyEnvelope<String>> {
    envelope(build_grid_copy_sql(&application, &request).await)
}

async fn grid_in_values_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyGridUpdateRequest>,
) -> Json<LegacyEnvelope<String>> {
    envelope(build_grid_in_values(&application, &request).await)
}

async fn sql_count_handler(
    State(application): State<Application>,
    Json(request): Json<LegacySqlExecuteRequest>,
) -> Json<LegacyEnvelope<u64>> {
    envelope(count_mysql_rows(&application, &request).await)
}

async fn large_cell_value_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyLargeCellValueRequest>,
) -> Json<LegacyEnvelope<LegacyLargeCellChunk>> {
    envelope(read_large_cell_value(&application, &request))
}

async fn large_cell_download_path_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyLargeCellDownloadRequest>,
) -> Json<LegacyEnvelope<String>> {
    envelope(download_large_cell_value_to_path(&application, &request).await)
}

async fn large_cell_download_handler(
    State(application): State<Application>,
    Json(request): Json<LegacyLargeCellDownloadRequest>,
) -> Response {
    let task =
        tokio::task::spawn_blocking(move || prepare_large_cell_download(&application, &request))
            .await;
    let download = match task {
        Ok(Ok(download)) => download,
        Ok(Err(error)) => return Json(LegacyEnvelope::<()>::failure(error)).into_response(),
        Err(_) => {
            return Json(LegacyEnvelope::<()>::failure(LegacyFailure {
                code: "large_cell_download_failed".to_owned(),
                message: "The large cell download task did not finish".to_owned(),
            }))
            .into_response();
        }
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, download.content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=chat2db-cell.{}", download.extension),
        )
        .body(Body::from(download.bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    use super::*;

    const REQUIRED_METADATA_PATHS: &[&str] = &[
        "/api/rdb/table/table_list",
        "/api/rdb/table/column_list",
        "/api/rdb/table/index_list",
        "/api/rdb/table/key_list",
        "/api/rdb/ddl/column_list",
        "/api/rdb/ddl/index_list",
        "/api/rdb/ddl/key_list",
        "/api/rdb/view/list",
        "/api/rdb/view/column_list",
        "/api/rdb/view/detail",
        "/api/rdb/function/list",
        "/api/rdb/function/detail",
        "/api/rdb/procedure/list",
        "/api/rdb/procedure/detail",
        "/api/rdb/trigger/list",
        "/api/rdb/trigger/detail",
    ];

    const REQUIRED_EDITABLE_PATHS: &[(&str, &str)] = &[
        ("get", "/api/rdb/table/table_meta"),
        ("get", "/api/rdb/table/query"),
        ("get", "/api/rdb/table/export"),
        ("get", "/api/rdb/table/create/example"),
        ("get", "/api/rdb/table/update/example"),
        ("get", "/api/rdb/ddl/export"),
        ("get", "/api/rdb/ddl/create/example"),
        ("get", "/api/rdb/ddl/update/example"),
        ("post", "/api/rdb/table/modify/sql"),
        ("post", "/api/rdb/ddl/delete"),
        ("post", "/api/rdb/table/truncate"),
        ("post", "/api/rdb/table/copy"),
        ("post", "/api/rdb/database/create_database_sql"),
        ("post", "/api/rdb/schema/create_schema_sql"),
        ("post", "/api/rdb/delete/database/prepare"),
        ("post", "/api/rdb/delete/database/execute"),
        ("post", "/api/rdb/delete/schema/prepare"),
        ("post", "/api/rdb/delete/schema/execute"),
        ("get", "/api/rdb/view/query"),
        ("get", "/api/rdb/view/view_meta"),
        ("post", "/api/rdb/view/modify/sql"),
        ("post", "/api/rdb/view/drop"),
        ("post", "/api/rdb/dml/get_update_sql"),
        ("put", "/api/rdb/dml/get_update_sql"),
        ("post", "/api/rdb/dml/copy_update_sql"),
        ("put", "/api/rdb/dml/copy_update_sql"),
        ("post", "/api/rdb/dml/copy_in_values_sql"),
        ("put", "/api/rdb/dml/copy_in_values_sql"),
        ("post", "/api/rdb/dml/count"),
        ("put", "/api/rdb/dml/count"),
        ("post", "/api/rdb/dml/execute_update"),
        ("put", "/api/rdb/dml/execute_update"),
    ];

    fn metadata_message(path: &str) -> serde_json::Value {
        let mut message = serde_json::json!({
            "dataSourceId": 1,
            "databaseName": "inventory",
            "schemaName": ""
        });
        let object = message
            .as_object_mut()
            .expect("metadata message must be an object");
        match path {
            "/api/rdb/table/table_list"
            | "/api/rdb/view/list"
            | "/api/rdb/function/list"
            | "/api/rdb/procedure/list"
            | "/api/rdb/trigger/list" => {
                object.insert("pageNo".to_owned(), serde_json::json!(1));
                object.insert("pageSize".to_owned(), serde_json::json!(20));
                object.insert("searchKey".to_owned(), serde_json::json!(""));
            }
            "/api/rdb/function/detail" => {
                object.insert(
                    "functionName".to_owned(),
                    serde_json::json!("double_amount"),
                );
            }
            "/api/rdb/procedure/detail" => {
                object.insert("procedureName".to_owned(), serde_json::json!("count_items"));
            }
            "/api/rdb/trigger/detail" => {
                object.insert(
                    "triggerName".to_owned(),
                    serde_json::json!("items_trim_label"),
                );
            }
            _ => {
                object.insert("tableName".to_owned(), serde_json::json!("items"));
            }
        }
        message
    }

    fn metadata_query(path: &str) -> String {
        let object_name = match path {
            "/api/rdb/function/detail" => "&functionName=double_amount",
            "/api/rdb/procedure/detail" => "&procedureName=count_items",
            "/api/rdb/trigger/detail" => "&triggerName=items_trim_label",
            "/api/rdb/table/table_list"
            | "/api/rdb/view/list"
            | "/api/rdb/function/list"
            | "/api/rdb/procedure/list"
            | "/api/rdb/trigger/list" => "&pageNo=1&pageSize=20&searchKey=",
            _ => "&tableName=items",
        };
        format!("{path}?dataSourceId=1&databaseName=inventory&schemaName={object_name}")
    }

    #[tokio::test]
    async fn retained_large_text_round_trips_through_chunk_and_download_contracts() {
        let application = Application::new();
        let owner = application.create_large_value_owner();
        let value = "数据🙂".repeat(24_000);
        let preview = application
            .retain_large_text(&owner, value.clone())
            .expect("large text must be retained");
        assert!(preview.truncated);
        let large_value_id = preview
            .large_value_id
            .expect("truncated text must receive a scoped token");

        let chunk = read_large_cell_value(
            &application,
            &LegacyLargeCellValueRequest {
                large_value_id: large_value_id.clone(),
                offset: 0,
                limit: 128,
                format: "base64".to_owned(),
            },
        )
        .expect("large text chunk must load");
        let decoded = BASE64_STANDARD
            .decode(chunk.value)
            .expect("chunk must use frontend-compatible base64");
        assert_eq!(decoded, value.as_bytes()[..128]);
        assert_eq!(chunk.offset, 0);
        assert_eq!(chunk.next_offset, 128);
        assert_eq!(chunk.encoding, "base64");
        assert_eq!(chunk.display_mode, LargeValueType::Text);

        let next = read_large_cell_value(
            &application,
            &LegacyLargeCellValueRequest {
                large_value_id: large_value_id.clone(),
                offset: chunk.next_offset,
                limit: 128,
                format: "base64".to_owned(),
            },
        )
        .expect("second large text chunk must load");
        let next_decoded = BASE64_STANDARD
            .decode(next.value)
            .expect("second chunk must use frontend-compatible base64");
        assert_eq!(next_decoded, value.as_bytes()[128..256]);
        assert_eq!(next.offset, 128);
        assert_eq!(next.next_offset, 256);

        let path = download_large_cell_value_to_path(
            &application,
            &LegacyLargeCellDownloadRequest {
                large_value_id,
                format: "text".to_owned(),
            },
        )
        .await
        .expect("large text must download to a local temporary file");
        assert_eq!(
            std::fs::read_to_string(&path).expect("download must be readable"),
            value
        );
        std::fs::remove_file(path).expect("temporary test download must be removed");
    }

    #[test]
    fn mysql_console_history_distinguishes_cancellation_from_failure() {
        let request = serde_json::from_value(serde_json::json!({
            "dataSourceId": "datasource-1",
            "databaseType": "MYSQL",
            "sql": "SELECT SLEEP(30)",
            "pageNo": 1,
            "pageSize": 200
        }))
        .expect("legacy SQL request must deserialize");
        let cancelled = sql_failure_result(
            &request,
            &LegacyFailure {
                code: "mysql_console_cancelled".to_owned(),
                message: "The SQL execution was cancelled".to_owned(),
            },
            12,
        );
        assert_eq!(mysql_console_history_status(&[&cancelled]), "cancelled");

        let failed = sql_failure_result(
            &request,
            &LegacyFailure {
                code: "database.query_failed".to_owned(),
                message: "Query failed".to_owned(),
            },
            12,
        );
        assert_eq!(mysql_console_history_status(&[&failed]), "fail");

        let mut succeeded = failed;
        succeeded.success = true;
        succeeded.extra = serde_json::json!({});
        assert_eq!(mysql_console_history_status(&[&succeeded]), "success");
    }

    #[test]
    fn counted_metadata_envelope_keeps_total_at_the_top_level() {
        let body = counted_envelope_value(Ok(serde_json::json!([
            { "name": "id" },
            { "name": "created_at" }
        ])));

        assert_eq!(body["success"], true);
        assert_eq!(body["total"], 2);
        assert_eq!(body["data"][0]["name"], "id");
        assert!(body["data"].get("total").is_none());
    }

    #[test]
    fn editable_preview_prepends_page_aware_row_numbers_and_column_metadata() {
        let mut headers = vec![LegacyResultHeader {
            data_type: "NUMERIC".to_owned(),
            name: "id".to_owned(),
            column_name: "id".to_owned(),
            column_type: "BIGINT".to_owned(),
            table_name: Some("items".to_owned()),
            database_name: Some("inventory".to_owned()),
            schema_name: None,
            primary_key: false,
            comment: None,
            default_value: None,
            auto_increment: None,
            nullable: true,
            column_size: None,
            decimal_digits: None,
            editor_type: None,
        }];
        let metadata = vec![CommunityTableColumn {
            database_name: "inventory".to_owned(),
            table_name: "items".to_owned(),
            name: "id".to_owned(),
            column_type: "BIGINT".to_owned(),
            default_value: Some("0".to_owned()),
            auto_increment: Some(true),
            comment: "primary id".to_owned(),
            primary_key: Some(true),
            column_size: Some(20),
            decimal_digits: Some(0),
            nullable: Some(0),
            ..CommunityTableColumn::default()
        }];
        assert!(enrich_direct_table_headers(
            &mut headers,
            &metadata,
            "items"
        ));
        let mut rows = vec![vec![LegacyResultCell {
            value: Some("7".to_owned()),
            large_value: false,
            large_value_id: None,
            value_type: "UNKNOWN".to_owned(),
            sql_type: -5,
            column_type: "BIGINT".to_owned(),
            size_bytes: None,
            size_chars: None,
            loaded_bytes: None,
            loaded_chars: None,
            truncated: false,
            unsupported_reason: None,
        }]];
        prepend_synthetic_row_numbers(&mut headers, &mut rows, 20);

        assert_eq!(headers[0].name, "CHAT2DB_ROW_NUMBER");
        assert_eq!(headers[0].data_type, "CHAT2DB_ROW_NUMBER");
        assert_eq!(rows[0][0].value.as_deref(), Some("21"));
        assert_eq!(rows[0][1].value.as_deref(), Some("7"));
        assert!(headers[1].primary_key);
        assert_eq!(headers[1].default_value.as_deref(), Some("0"));
        assert_eq!(headers[1].auto_increment, Some(1));
        assert!(!headers[1].nullable);
        assert_eq!(headers[1].comment.as_deref(), Some("primary id"));
        assert_eq!(headers[1].editor_type.as_deref(), Some("TEXT"));
    }

    #[tokio::test]
    async fn grid_update_mapping_builds_sql_and_rejects_partial_large_values() {
        let request: LegacyGridUpdateRequest = serde_json::from_value(serde_json::json!({
            "dataSourceId": "mysql-local",
            "databaseType": "MYSQL",
            "databaseName": "inventory",
            "schemaName": "",
            "tableName": "items",
            "headerList": [
                { "name": "CHAT2DB_ROW_NUMBER", "columnType": "BIGINT" },
                { "name": "id", "columnType": "BIGINT", "primaryKey": true, "autoIncrement": 1 },
                { "name": "label", "columnType": "VARCHAR" }
            ],
            "operations": [{
                "type": "UPDATE",
                "dataList": ["1", "7", "new label"],
                "oldDataList": ["1", "7", "old label"]
            }]
        }))
        .expect("frontend grid request must deserialize");
        let sql = build_grid_update_sql(&Application::new(), &request)
            .await
            .expect("grid SQL must build without opening a datasource");
        assert_eq!(
            sql,
            "UPDATE `inventory`.`items` SET `label` = 'new label' WHERE `id` = 7;"
        );

        let mut partial = request;
        partial.operations[0].old_data_list[2] =
            Some("CHAT2DB_LARGE_VALUE_PREVIEW:PARTIAL".to_owned());
        let error = build_grid_update_sql(&Application::new(), &partial)
            .await
            .expect_err("partial large-value previews must never enter DML");
        assert_eq!(error.code, "mysql_partial_large_value_rejected");
    }

    #[test]
    fn table_alter_derives_primary_key_changes_from_editor_columns() {
        let old_table = LegacyEditableTable {
            name: "items".to_owned(),
            database_name: "inventory".to_owned(),
            column_list: vec![
                LegacyColumn {
                    name: "id".to_owned(),
                    column_type: "BIGINT".to_owned(),
                    primary_key: Some(true),
                    primary_key_order: 1,
                    nullable: Some(0),
                    ..LegacyColumn::default()
                },
                LegacyColumn {
                    name: "code".to_owned(),
                    column_type: "VARCHAR".to_owned(),
                    column_size: Some(64),
                    nullable: Some(0),
                    ..LegacyColumn::default()
                },
            ],
            index_list: vec![LegacyIndex {
                name: "PRIMARY".to_owned(),
                index_type: "Primary".to_owned(),
                column_list: vec![LegacyIndexColumn {
                    column_name: "id".to_owned(),
                    ..LegacyIndexColumn::default()
                }],
                ..LegacyIndex::default()
            }],
            ..LegacyEditableTable::default()
        };
        let mut new_table = old_table.clone();
        new_table.column_list[0].primary_key = Some(false);
        new_table.column_list[0].primary_key_order = 0;
        new_table.column_list[0].edit_status = Some("MODIFY".to_owned());
        new_table.column_list[1].primary_key = Some(true);
        new_table.column_list[1].primary_key_order = 1;
        new_table.column_list[1].edit_status = Some("MODIFY".to_owned());

        let alter = mysql_table_alter(&old_table, &new_table, "inventory", "")
            .expect("column primary-key edits must normalize");
        let sql = build_mysql_alter_table(&alter).expect("primary-key ALTER must build");

        assert_eq!(sql.matches("DROP PRIMARY KEY").count(), 1);
        assert_eq!(
            sql.matches("ADD PRIMARY KEY (`code`) USING BTREE").count(),
            1
        );
    }

    #[test]
    fn table_editor_metadata_keeps_only_type_appropriate_dimensions() {
        let definition = |column_type: &str, column_size, decimal_digits| {
            mysql_column_definition(&LegacyColumn {
                name: "value".to_owned(),
                column_type: column_type.to_owned(),
                column_size,
                decimal_digits,
                ..LegacyColumn::default()
            })
            .expect("metadata column must normalize")
        };

        let varchar = definition("VARCHAR", Some(128), Some(0));
        assert_eq!((varchar.length, varchar.scale), (Some(128), None));
        let text = definition("TEXT", Some(65_535), Some(0));
        assert_eq!((text.length, text.scale), (None, None));
        let decimal = definition("DECIMAL", Some(12), Some(3));
        assert_eq!((decimal.length, decimal.scale), (Some(12), Some(3)));
        let timestamp = definition("TIMESTAMP", Some(26), Some(6));
        assert_eq!((timestamp.length, timestamp.scale), (Some(6), None));
    }

    #[test]
    fn table_editor_preserves_mysql_enum_and_set_values() {
        let column = column_response(CommunityTableColumn {
            name: "state".to_owned(),
            column_type: "ENUM".to_owned(),
            extent: "('','draft','needs,review','O''Reilly','close)later')".to_owned(),
            ..CommunityTableColumn::default()
        });

        assert_eq!(
            column.value,
            "'','draft','needs,review','O''Reilly','close)later'"
        );
        let definition = mysql_column_definition(&column)
            .expect("the retained enum definition must normalize for DDL");
        assert_eq!(
            definition.enum_values,
            vec!["", "draft", "needs,review", "O'Reilly", "close)later"]
        );
    }

    #[test]
    fn table_editor_accepts_null_heavy_frontend_rows() {
        let request: LegacyTableModifyRequest = serde_json::from_str(
            r#"{
            "dataSourceId": "mysql-local",
            "databaseType": "MYSQL",
            "databaseName": "inventory",
            "newTable": {
                "name": "items",
                "comment": null,
                "schemaName": null,
                "type": null,
                "dbType": null,
                "ddl": null,
                "engine": null,
                "charset": null,
                "collate": null,
                "partition": null,
                "tablespace": null,
                "createTime": null,
                "updateTime": null,
                "columnList": [{
                    "oldName": null,
                    "name": "state",
                    "tableName": null,
                    "columnType": "VARCHAR",
                    "dataType": null,
                    "defaultValue": null,
                    "autoIncrement": null,
                    "comment": null,
                    "primaryKey": null,
                    "primaryKeyName": null,
                    "primaryKeyOrder": null,
                    "schemaName": null,
                    "databaseName": null,
                    "typeName": null,
                    "columnSize": 32,
                    "bufferLength": null,
                    "decimalDigits": null,
                    "numPrecRadix": null,
                    "nullableInt": null,
                    "sqlDataType": null,
                    "sqlDatetimeSub": null,
                    "charOctetLength": null,
                    "ordinalPosition": null,
                    "nullable": 1,
                    "generatedColumn": null,
                    "extent": null,
                    "charSetName": null,
                    "collationName": null,
                    "value": null,
                    "unit": null,
                    "defaultConstraintName": null,
                    "editStatus": "ADD"
                }],
                "indexList": [{
                    "name": "",
                    "type": null,
                    "comment": null,
                    "schemaName": null,
                    "databaseName": null,
                    "method": null,
                    "foreignSchemaName": null,
                    "foreignTableName": null,
                    "foreignColumnNamelist": null,
                    "columnList": [{
                        "indexName": null,
                        "tableName": null,
                        "type": null,
                        "comment": null,
                        "columnName": "state",
                        "collation": null,
                        "schemaName": null,
                        "databaseName": null,
                        "indexQualifier": null,
                        "ascOrDesc": null,
                        "filterCondition": null
                    }],
                    "editStatus": "ADD"
                }]
            }
        }"#,
        )
        .expect("the retained Community editor payload must accept explicit nulls");

        let column = &request.new_table.column_list[0];
        assert_eq!(column.primary_key_order, 0);
        assert!(column.comment.is_empty());
        assert!(column.char_set_name.is_empty());
        let index = &request.new_table.index_list[0];
        assert!(index.index_type.is_empty());
        assert!(index.comment.is_empty());
        assert!(index.column_list[0].index_name.is_empty());
    }

    #[test]
    fn result_grid_in_values_accepts_the_frontend_operation_name() {
        let operation: LegacyGridOperationRequest = serde_json::from_value(serde_json::json!({
            "type": "IN_VALUES",
            "dataList": ["1", "active"],
            "selectCols": [1]
        }))
        .expect("the IN-values operation must deserialize");

        let operation = mysql_grid_copy_operation(&operation)
            .expect("the retained frontend operation name must normalize");
        assert_eq!(
            operation.operation_type,
            MysqlResultGridCopyOperationType::Where
        );
    }

    #[test]
    fn table_alter_detects_column_order_from_array_position() {
        let column = |name: &str| LegacyColumn {
            old_name: Some(name.to_owned()),
            name: name.to_owned(),
            column_type: "INT".to_owned(),
            nullable: Some(1),
            ..LegacyColumn::default()
        };
        let old_table = LegacyEditableTable {
            name: "items".to_owned(),
            database_name: "inventory".to_owned(),
            column_list: vec![column("a"), column("b"), column("c")],
            ..LegacyEditableTable::default()
        };
        let mut new_table = old_table.clone();
        new_table.column_list = vec![
            old_table.column_list[2].clone(),
            old_table.column_list[0].clone(),
            old_table.column_list[1].clone(),
        ];

        assert_eq!(
            mysql_reordered_column_names(&old_table, &new_table),
            ["c", "a"]
        );

        let alter = mysql_table_alter(&old_table, &new_table, "inventory", "")
            .expect("a drag-only reorder must normalize");
        let sql = build_mysql_alter_table(&alter).expect("a drag-only reorder must build");

        assert_eq!(sql.matches("MODIFY COLUMN").count(), 2);
        assert!(sql.contains("MODIFY COLUMN `c` INT NULL FIRST"));
        assert!(sql.contains("MODIFY COLUMN `a` INT NULL AFTER `c`"));
    }

    #[tokio::test]
    async fn view_meta_returns_the_community_creation_template() {
        let request: LegacyViewOperationRequest = serde_json::from_value(serde_json::json!({
            "dataSourceId": "mysql-local",
            "databaseType": "MYSQL",
            "databaseName": "inventory",
            "schemaName": "ignored_schema",
            "viewName": ""
        }))
        .expect("view metadata request must deserialize");
        let metadata = view_editor_meta(&Application::new(), &request)
            .await
            .expect("view metadata must not require an existing view");

        assert_eq!(metadata.sql, "select * from table_name");
        assert_eq!(metadata.configurations.len(), 6);
        assert_eq!(
            metadata
                .configurations
                .iter()
                .map(|configuration| configuration["name"].as_str().unwrap_or_default())
                .collect::<Vec<_>>(),
            vec![
                "algorithm",
                "checkOption",
                "security",
                "viewName",
                "definer",
                "useOrReplace"
            ]
        );
        assert!(metadata.preview_sql.contains("`inventory`.`undefined`"));
        assert!(!metadata.preview_sql.contains("ignored_schema"));
    }

    #[tokio::test]
    async fn table_and_view_editor_payloads_map_to_core_builders() {
        let table_request: LegacyTableModifyRequest = serde_json::from_value(serde_json::json!({
            "dataSourceId": "mysql-local",
            "databaseType": "MYSQL",
            "databaseName": "inventory",
            "newTable": {
                "name": "items",
                "comment": "stock",
                "engine": "InnoDB",
                "charset": "utf8mb4",
                "columnList": [
                    {
                        "name": "id",
                        "columnType": "BIGINT",
                        "nullable": 0,
                        "autoIncrement": true,
                        "primaryKey": true
                    },
                    {
                        "name": "label",
                        "columnType": "VARCHAR",
                        "columnSize": 255,
                        "nullable": 0
                    }
                ],
                "indexList": []
            }
        }))
        .expect("table editor request must deserialize");
        let table_sql = build_table_modify_sql(&Application::new(), &table_request)
            .await
            .expect("table SQL must build");
        assert_eq!(table_sql.len(), 1);
        assert!(
            table_sql[0]
                .sql
                .starts_with("CREATE TABLE `inventory`.`items`")
        );
        assert!(table_sql[0].sql.contains("PRIMARY KEY (`id`) USING BTREE"));
        assert!(table_sql[0].sql.contains("`label` VARCHAR(255) NOT NULL"));

        let view_request: LegacyViewOperationRequest = serde_json::from_value(serde_json::json!({
            "dataSourceId": "mysql-local",
            "databaseType": "MYSQL",
            "databaseName": "inventory",
            "viewName": "active_items",
            "viewBody": "SELECT id FROM items WHERE active = 1",
            "useOrReplace": true,
            "algorithm": "MERGE",
            "definer": "reporter@localhost",
            "security": "INVOKER",
            "checkOption": "LOCAL"
        }))
        .expect("view editor request must deserialize");
        let view_sql = build_view_modify_sql(&Application::new(), &view_request)
            .await
            .expect("view SQL must build");
        assert!(view_sql.starts_with("CREATE OR REPLACE ALGORITHM = MERGE"));
        assert!(view_sql.contains("DEFINER = 'reporter'@'localhost'"));
        assert!(view_sql.ends_with("WITH LOCAL CHECK OPTION"));
    }

    #[tokio::test]
    async fn editable_paths_are_registered_for_dispatch_and_axum() {
        let router = routes().with_state(Application::new());
        for (method, path) in REQUIRED_EDITABLE_PATHS {
            assert!(LEGACY_PATHS.contains(path), "missing dispatch path: {path}");
            let response = dispatch(
                &Application::new(),
                LegacyDispatchRequest {
                    request_url: (*path).to_owned(),
                    method: (*method).to_owned(),
                    message: serde_json::Value::Null,
                },
            )
            .await;
            assert_eq!(
                response["errorCode"], "invalid_legacy_request",
                "missing desktop dispatch branch: {method} {path}"
            );

            let http_method = method
                .to_ascii_uppercase()
                .parse::<axum::http::Method>()
                .expect("method must be valid");
            let mut builder = Request::builder().method(http_method).uri(*path);
            let body = if *method == "get" {
                Body::empty()
            } else {
                builder = builder.header("content-type", "application/json");
                Body::from("null")
            };
            let response = router
                .clone()
                .oneshot(builder.body(body).expect("request must build"))
                .await
                .expect("router must respond");
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "missing Axum route: {method} {path}"
            );
        }
    }

    #[tokio::test]
    async fn mysql_ddl_example_aliases_preserve_community_null_data() {
        let application = Application::new();
        let router = routes().with_state(application.clone());
        for path in [
            "/api/rdb/ddl/create/example",
            "/api/rdb/ddl/update/example",
            "/api/rdb/table/create/example",
            "/api/rdb/table/update/example",
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::get(format!("{path}?dbType=MYSQL"))
                        .body(Body::empty())
                        .expect("request must build"),
                )
                .await
                .expect("router must respond");
            assert_eq!(response.status(), StatusCode::OK);
            let body = response
                .into_body()
                .collect()
                .await
                .expect("response body must collect")
                .to_bytes();
            let http: serde_json::Value =
                serde_json::from_slice(&body).expect("response must be JSON");
            assert_eq!(http["success"], true, "HTTP alias failed: {path}");
            assert!(http["data"].is_null(), "HTTP alias returned SQL: {path}");
            assert!(http["errorCode"].is_null());
            assert!(http["errorMessage"].is_null());

            let desktop = dispatch(
                &application,
                LegacyDispatchRequest {
                    request_url: path.to_owned(),
                    method: "get".to_owned(),
                    message: serde_json::json!({"dbType": "MYSQL"}),
                },
            )
            .await;
            assert_eq!(desktop, http, "desktop alias diverged: {path}");
        }
    }

    #[test]
    fn metadata_projections_use_the_historical_frontend_field_names() {
        let column = column_response(CommunityTableColumn {
            database_name: "catalog".to_owned(),
            schema_name: "public".to_owned(),
            table_name: "users".to_owned(),
            name: "id".to_owned(),
            column_type: "BIGINT".to_owned(),
            data_type: Some(-5),
            primary_key: Some(true),
            nullable: Some(0),
            ..CommunityTableColumn::default()
        });
        let column = serde_json::to_value(column).expect("column projection must serialize");
        assert_eq!(column["oldName"], "id");
        assert_eq!(column["tableName"], "users");
        assert_eq!(column["columnType"], "BIGINT");
        assert_eq!(column["primaryKey"], true);
        assert_eq!(column["nullable"], 0);
        assert!(column["defaultValue"].is_null());

        let empty_default = column_response(CommunityTableColumn {
            default_value: Some(String::new()),
            ..CommunityTableColumn::default()
        });
        let empty_default =
            serde_json::to_value(empty_default).expect("empty default must serialize");
        assert_eq!(empty_default["defaultValue"], "");

        let index = index_response(CommunityTableIndex {
            name: "PRIMARY".to_owned(),
            index_type: "BTREE".to_owned(),
            columns: vec![CommunityTableIndexColumn {
                index_name: "PRIMARY".to_owned(),
                table_name: "users".to_owned(),
                column_name: "id".to_owned(),
                cardinality: Some("42".to_owned()),
                sort_order: "A".to_owned(),
                ..CommunityTableIndexColumn::default()
            }],
            ..CommunityTableIndex::default()
        });
        let index = serde_json::to_value(index).expect("index projection must serialize");
        assert_eq!(index["type"], "BTREE");
        assert_eq!(index["columnList"][0]["columnName"], "id");
        assert_eq!(index["columnList"][0]["cardinality"], 42);
        assert_eq!(index["columnList"][0]["ascOrDesc"], "A");

        let function = function_response(CommunityFunction {
            name: "calculate_total".to_owned(),
            body: "RETURN 1".to_owned(),
            template: "CREATE FUNCTION calculate_total".to_owned(),
            ..CommunityFunction::default()
        });
        let function = serde_json::to_value(function).expect("function projection must serialize");
        assert_eq!(function["functionName"], "calculate_total");
        assert_eq!(function["functionBody"], "RETURN 1");
        assert_eq!(
            function["functionTemplate"],
            "CREATE FUNCTION calculate_total"
        );

        let view = table_response(CommunityTable {
            name: "active_users".to_owned(),
            table_type: "VIEW".to_owned(),
            ddl: "CREATE VIEW active_users AS SELECT 1".to_owned(),
            ..CommunityTable::default()
        });
        let view = serde_json::to_value(view).expect("view projection must serialize");
        assert_eq!(view["name"], "active_users");
        assert_eq!(view["tableType"], "VIEW");
        assert_eq!(view["ddl"], "CREATE VIEW active_users AS SELECT 1");

        let procedure = procedure_response(CommunityProcedure {
            name: "refresh_users".to_owned(),
            body: "CREATE PROCEDURE refresh_users() SELECT 1".to_owned(),
            ..CommunityProcedure::default()
        });
        let procedure =
            serde_json::to_value(procedure).expect("procedure projection must serialize");
        assert_eq!(procedure["procedureName"], "refresh_users");
        assert_eq!(
            procedure["procedureBody"],
            "CREATE PROCEDURE refresh_users() SELECT 1"
        );

        let trigger = trigger_response(CommunityTrigger {
            name: "users_before_insert".to_owned(),
            event_manipulation: "INSERT".to_owned(),
            body: "CREATE TRIGGER users_before_insert".to_owned(),
            ..CommunityTrigger::default()
        });
        let trigger = serde_json::to_value(trigger).expect("trigger projection must serialize");
        assert_eq!(trigger["triggerName"], "users_before_insert");
        assert_eq!(trigger["eventManipulation"], "INSERT");
        assert_eq!(trigger["triggerBody"], "CREATE TRIGGER users_before_insert");

        let page = full_page(vec!["one", "two", "three"]);
        assert_eq!(page.page_no, 1);
        assert_eq!(page.page_size, 3);
        assert_eq!(page.total, 3);
        assert!(!page.has_next_page);
    }

    #[test]
    fn table_search_matches_community_name_and_comment_behavior() {
        let table = LegacyTable {
            name: "orders".to_owned(),
            comment: "Invoice archive".to_owned(),
            table_type: "TABLE".to_owned(),
            pinned: false,
            ddl: String::new(),
            engine: String::new(),
            charset: String::new(),
            collate: String::new(),
            increment_value: None,
            partition: String::new(),
            tablespace: String::new(),
            rows: None,
            data_length: None,
            create_time: String::new(),
            update_time: String::new(),
        };

        assert!(table_matches_search(&table, "ORDER"));
        assert!(table_matches_search(&table, "invoice"));
        assert!(table_matches_search(&table, "  "));
        assert!(!table_matches_search(&table, "customer"));
    }

    #[test]
    fn required_metadata_paths_are_known_to_desktop_dispatch() {
        let unique: HashSet<&str> = REQUIRED_METADATA_PATHS.iter().copied().collect();
        assert_eq!(unique.len(), REQUIRED_METADATA_PATHS.len());
        for path in REQUIRED_METADATA_PATHS {
            assert!(LEGACY_PATHS.contains(path), "missing dispatch path: {path}");
        }
        for path in LEGACY_COUNTED_PATHS {
            assert!(REQUIRED_METADATA_PATHS.contains(path));
        }
    }

    #[tokio::test]
    async fn required_metadata_paths_reach_their_desktop_dispatch_branches() {
        for path in REQUIRED_METADATA_PATHS {
            let response = dispatch(
                &Application::new(),
                LegacyDispatchRequest {
                    request_url: (*path).to_owned(),
                    method: "get".to_owned(),
                    message: serde_json::Value::Null,
                },
            )
            .await;
            assert_eq!(
                response["errorCode"], "invalid_legacy_request",
                "missing desktop dispatch branch: {path}"
            );
            if LEGACY_COUNTED_PATHS.contains(path) {
                assert!(response.get("total").is_some());
            }
        }
    }

    #[tokio::test]
    async fn required_metadata_paths_accept_community_dispatch_fields() {
        let application = Application::new();
        for path in REQUIRED_METADATA_PATHS {
            let response = dispatch(
                &application,
                LegacyDispatchRequest {
                    request_url: (*path).to_owned(),
                    method: "get".to_owned(),
                    message: metadata_message(path),
                },
            )
            .await;
            assert_eq!(response["success"], false, "unexpected success: {path}");
            assert_ne!(
                response["errorCode"], "invalid_legacy_request",
                "Community fields did not decode: {path}"
            );
            assert_ne!(
                response["errorCode"], "route_not_found",
                "dispatch route is missing: {path}"
            );
            if LEGACY_COUNTED_PATHS.contains(path) {
                assert!(response.get("total").is_some(), "missing total: {path}");
            }
        }
    }

    #[tokio::test]
    async fn required_metadata_paths_accept_community_axum_query_fields() {
        let router = routes().with_state(Application::new());
        for path in REQUIRED_METADATA_PATHS {
            let response = router
                .clone()
                .oneshot(
                    Request::get(metadata_query(path))
                        .body(Body::empty())
                        .expect("request must build"),
                )
                .await
                .expect("router must respond");
            assert_eq!(response.status(), StatusCode::OK, "invalid route: {path}");
            let body = response
                .into_body()
                .collect()
                .await
                .expect("response body must collect")
                .to_bytes();
            let body: serde_json::Value =
                serde_json::from_slice(&body).expect("response body must be JSON");
            assert_eq!(body["success"], false, "unexpected success: {path}");
            assert!(body.get("errorCode").is_some(), "missing envelope: {path}");
            if LEGACY_COUNTED_PATHS.contains(path) {
                assert!(body.get("total").is_some(), "missing total: {path}");
            }
        }
    }

    #[tokio::test]
    async fn invalid_axum_query_uses_the_community_json_envelope() {
        let response = routes()
            .with_state(Application::new())
            .oneshot(
                Request::get("/api/rdb/table/table_list?pageNo=1&pageSize=20")
                    .body(Body::empty())
                    .expect("request must build"),
            )
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("response body must collect")
            .to_bytes();
        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body must be JSON");
        assert_eq!(body["success"], false);
        assert_eq!(body["data"], serde_json::Value::Null);
        assert_eq!(body["errorCode"], "invalid_legacy_request");
    }

    #[tokio::test]
    async fn metadata_page_size_matches_the_community_range() {
        let application = Application::new();
        for page_size in [0, MAX_METADATA_PAGE_SIZE + 1] {
            let response = dispatch(
                &application,
                LegacyDispatchRequest {
                    request_url: "/api/rdb/view/list".to_owned(),
                    method: "get".to_owned(),
                    message: serde_json::json!({
                        "dataSourceId": 1,
                        "databaseName": "inventory",
                        "schemaName": "",
                        "pageNo": 1,
                        "pageSize": page_size,
                        "searchKey": "ignored"
                    }),
                },
            )
            .await;
            assert_eq!(response["success"], false);
            assert_eq!(response["errorCode"], "invalid_legacy_request");
            assert_eq!(
                response["errorMessage"],
                "pageSize must be between 1 and 100000"
            );
        }
    }
}
