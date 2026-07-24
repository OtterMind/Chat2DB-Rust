use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::JdbcValue;

/// Portable value representation selected for a result column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JdbcValueType {
    /// Boolean values.
    Boolean,
    /// Signed integer strings.
    SignedInteger,
    /// Unsigned integer strings.
    UnsignedInteger,
    /// Single-precision floating-point strings.
    Float32,
    /// Double-precision floating-point strings.
    Float64,
    /// Arbitrary-precision decimal strings.
    Decimal,
    /// UTF-8 text.
    Text,
    /// Base64-encoded binary.
    Binary,
    /// ISO-8601 dates.
    Date,
    /// ISO-8601 times.
    Time,
    /// ISO-8601 timestamps.
    Timestamp,
    /// ISO-8601 offset timestamps.
    TimestampWithTimeZone,
    /// JSON document text.
    Json,
    /// UUID text.
    Uuid,
    /// Output-only vendor display values.
    Opaque,
}

/// JDBC nullability reported for a result column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ColumnNullability {
    /// The driver does not know whether nulls are allowed.
    Unknown,
    /// The column does not allow nulls.
    NoNulls,
    /// The column allows nulls.
    Nullable,
}

/// Portable result-column metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResultColumn {
    /// One-based column ordinal.
    pub ordinal: u32,
    /// Display label.
    pub label: String,
    /// Source column name.
    pub name: String,
    /// `java.sql.Types` integer.
    pub jdbc_type: i32,
    /// Vendor JDBC type name.
    pub jdbc_type_name: String,
    /// Portable scalar representation.
    pub value_type: JdbcValueType,
    /// Driver-reported nullability.
    pub nullability: ColumnNullability,
    /// Driver-reported precision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precision: Option<u32>,
    /// Driver-reported scale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<i32>,
    /// Suggested display width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_size: Option<u32>,
    /// Whether the numeric column is signed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signed: Option<bool>,
    /// Source catalog name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_name: Option<String>,
    /// Source schema name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_name: Option<String>,
    /// Source table name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_name: Option<String>,
}

/// Durable retained-result metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResultMetadata {
    /// Opaque retained-result id.
    pub id: String,
    /// Retained row count encoded as a decimal integer.
    pub row_count: String,
    /// Retained encoded byte count encoded as a decimal integer.
    pub byte_count: String,
    /// Whether another row existed beyond the configured row limit.
    pub truncated_by_max_rows: bool,
    /// Whether another row exceeded the configured result-byte limit.
    pub truncated_by_max_result_bytes: bool,
    /// Unix epoch milliseconds encoded as a decimal integer.
    pub created_at_ms: String,
    /// Unix epoch milliseconds encoded as a decimal integer.
    pub expires_at_ms: String,
}

/// One result row whose values follow the response column order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResultRow {
    /// Typed values in column order.
    pub values: Vec<JdbcValue>,
}

/// Bounded retained-result page request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResultPageRequest {
    /// Zero-based row offset encoded as a decimal integer.
    pub offset: String,
    /// Maximum returned rows encoded as a decimal integer.
    pub max_rows: String,
    /// Maximum cumulative encoded row bytes encoded as a decimal integer.
    pub max_bytes: String,
}

/// One bounded retained-result page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResultPage {
    /// Durable result metadata.
    pub metadata: ResultMetadata,
    /// Column schema required to interpret rows.
    pub columns: Vec<ResultColumn>,
    /// Actual zero-based first-row offset encoded as a decimal integer.
    pub offset: String,
    /// Bounded rows.
    pub rows: Vec<ResultRow>,
    /// Whether another retained row exists after this page.
    pub has_more: bool,
}

#[cfg(test)]
mod tests {
    use super::{ResultMetadata, ResultPageRequest};

    #[test]
    fn result_counts_offsets_and_timestamps_remain_strings() {
        let request = ResultPageRequest {
            offset: "9007199254740993".to_owned(),
            max_rows: "4096".to_owned(),
            max_bytes: "16777216".to_owned(),
        };
        let metadata = ResultMetadata {
            id: "result-1".to_owned(),
            row_count: "9007199254740994".to_owned(),
            byte_count: "9007199254740995".to_owned(),
            truncated_by_max_rows: false,
            truncated_by_max_result_bytes: false,
            created_at_ms: "1784900000000".to_owned(),
            expires_at_ms: "1784903600000".to_owned(),
        };

        let request_json = serde_json::to_value(request).expect("request must serialize");
        let metadata_json = serde_json::to_value(metadata).expect("metadata must serialize");
        assert!(request_json["offset"].is_string());
        assert!(request_json["maxRows"].is_string());
        assert!(metadata_json["rowCount"].is_string());
        assert!(metadata_json["createdAtMs"].is_string());
    }
}
