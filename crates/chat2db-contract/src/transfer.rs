//! Transport-neutral import, export, task, and generated-code contracts.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Supported tabular or SQL file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum TransferFileFormat {
    Csv,
    Xls,
    Xlsx,
    Sql,
}

/// Controls whether tabular import cells are interpreted as ordinary text or
/// as `Chat2DB`'s lossless NULL/binary transfer envelope.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TabularImportEncoding {
    /// Preserve every present cell as ordinary external-file text.
    #[default]
    Plain,
    /// Decode cells emitted by `Chat2DB` tabular exports.
    Chat2dbV1,
}

impl TransferFileFormat {
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Xls => "xls",
            Self::Xlsx => "xlsx",
            Self::Sql => "sql",
        }
    }
}

/// Community SQL export scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum TransferSqlScope {
    /// Export object definitions and table data.
    All,
    /// Export object definitions only.
    Schema,
    /// Export table data only.
    Table,
}

/// Durable transfer operation category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransferTaskKind {
    ImportFile,
    ExportSql,
    ExportFile,
}

/// Durable transfer lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransferTaskStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

/// Starts an asynchronous file import into one table or executes a SQL file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportFileRequest {
    pub datasource_id: String,
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_name: Option<String>,
    pub file_path: String,
    pub format: TransferFileFormat,
    #[serde(default = "default_true")]
    pub contains_header: bool,
    #[serde(default)]
    pub tabular_encoding: TabularImportEncoding,
}

/// Starts an asynchronous SQL dump export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SqlFileExportRequest {
    pub datasource_id: String,
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
    #[serde(default)]
    pub table_names: Vec<String>,
    pub scope: TransferSqlScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_path: Option<String>,
}

/// Starts an asynchronous CSV, XLS, XLSX, or SQL table export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OtherFileExportRequest {
    pub datasource_id: String,
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
    pub table_names: Vec<String>,
    pub format: TransferFileFormat,
    #[serde(default = "default_true")]
    pub contains_header: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_path: Option<String>,
}

/// DML result export window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DmlExportSize {
    CurrentPage,
    All,
}

/// DML result export encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum DmlExportFormat {
    Csv,
    Xlsx,
    Insert,
}

/// Synchronously exports one selected result set into a managed artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DmlExportRequest {
    pub datasource_id: String,
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
    #[serde(default)]
    pub sql: String,
    pub original_sql: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_set_id: Option<u32>,
    pub export_size: DmlExportSize,
    pub format: DmlExportFormat,
}

/// Generates Java entity, Mapper, and Mapper XML files for one table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateMysqlClassRequest {
    pub datasource_id: String,
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
    pub table_name: String,
    pub export_path: String,
}

/// Asynchronous transfer acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferTaskAccepted {
    pub task_id: i64,
}

/// Durable task projection shared by HTTP and Desktop adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferTask {
    pub id: i64,
    pub datasource_id: String,
    pub database_name: String,
    pub schema_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_name: Option<String>,
    pub kind: TransferTaskKind,
    pub status: TransferTaskStatus,
    pub task_name: String,
    pub progress_current: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_total: Option<String>,
    pub progress_description: String,
    pub info_log: String,
    pub error_log: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    pub cancel_requested: bool,
    pub created_at_ms: String,
    pub updated_at_ms: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<String>,
}

/// Bounded task list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferTaskPage {
    pub items: Vec<TransferTask>,
    pub total: u64,
    pub page_no: u32,
    pub page_size: u32,
}

/// Secret-free artifact metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferArtifact {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<i64>,
    pub file_name: String,
    pub media_type: String,
    pub format: String,
    pub byte_count: String,
    pub sha256: String,
    pub created_at_ms: String,
}

/// Files emitted by Java class generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedMysqlClassSet {
    pub output_directory: String,
    pub files: Vec<String>,
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::{TransferFileFormat, TransferTaskStatus};

    #[test]
    fn community_facing_enums_keep_stable_uppercase_values() {
        assert_eq!(
            serde_json::to_string(&TransferFileFormat::Xlsx).expect("format serializes"),
            "\"XLSX\""
        );
        assert_eq!(
            serde_json::to_string(&TransferTaskStatus::Interrupted).expect("status serializes"),
            "\"INTERRUPTED\""
        );
    }
}
