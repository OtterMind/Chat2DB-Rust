use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Lossless JSON representation of one JDBC scalar value.
///
/// Numeric values that could lose precision in JavaScript are decimal strings.
/// Floating-point strings preserve finite values as well as `NaN`, `Infinity`,
/// and `-Infinity`. Binary values are standard padded base64 strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JdbcValue {
    /// SQL `NULL`.
    Null,
    /// Boolean value.
    Boolean {
        /// Scalar value.
        value: bool,
    },
    /// Signed integer encoded as a base-10 string.
    SignedInteger {
        /// Base-10 signed integer.
        value: String,
    },
    /// Unsigned integer encoded as a base-10 string.
    UnsignedInteger {
        /// Base-10 unsigned integer.
        value: String,
    },
    /// IEEE-754 single-precision value encoded losslessly as a string.
    Float32 {
        /// Finite decimal, `NaN`, `Infinity`, or `-Infinity`.
        value: String,
    },
    /// IEEE-754 double-precision value encoded losslessly as a string.
    Float64 {
        /// Finite decimal, `NaN`, `Infinity`, or `-Infinity`.
        value: String,
    },
    /// Arbitrary-precision decimal encoded as a plain base-10 string.
    Decimal {
        /// Plain decimal string without exponent normalization requirements.
        value: String,
    },
    /// UTF-8 text.
    Text {
        /// Text value.
        value: String,
    },
    /// Binary data encoded as standard padded base64.
    Binary {
        /// Standard padded base64 string.
        #[schema(format = Byte)]
        value: String,
    },
    /// ISO-8601 date.
    Date {
        /// Date text.
        value: String,
    },
    /// ISO-8601 time.
    Time {
        /// Time text.
        value: String,
    },
    /// ISO-8601 timestamp without a required offset.
    Timestamp {
        /// Timestamp text.
        value: String,
    },
    /// ISO-8601 timestamp with an explicit UTC offset.
    TimestampWithTimeZone {
        /// Offset timestamp text.
        value: String,
    },
    /// JSON text preserved exactly as returned by JDBC.
    Json {
        /// JSON document text.
        value: String,
    },
    /// UUID text.
    Uuid {
        /// Canonical UUID string.
        value: String,
    },
    /// Output-only vendor value without a portable JDBC representation.
    Opaque {
        /// Vendor type name.
        #[serde(rename = "typeName")]
        type_name: String,
        /// Safe display value.
        #[serde(rename = "displayValue")]
        display_value: String,
    },
}

/// One one-based JDBC statement parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryParameter {
    /// One-based parameter position.
    pub position: u32,
    /// Typed parameter value.
    pub value: JdbcValue,
}

/// Explicit execution and retained-result limits for one query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryLimits {
    /// Maximum emitted rows encoded as a decimal integer; `0` means engine default.
    pub max_rows: String,
    /// Maximum encoded result bytes as a decimal integer; `0` means engine default.
    pub max_result_bytes: String,
    /// Target rows per streamed batch.
    pub batch_rows: u32,
    /// Target encoded bytes per streamed batch.
    pub batch_bytes: u32,
    /// Retained-result lifetime in seconds.
    pub result_ttl_seconds: u32,
}

/// Request to begin one asynchronous query operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartQueryRequest {
    /// Opaque datasource id.
    pub datasource_id: String,
    /// SQL text executed by the compatibility engine.
    pub sql: String,
    /// One-based typed parameters.
    pub parameters: Vec<QueryParameter>,
    /// Query, streaming, and result-retention limits.
    pub limits: QueryLimits,
}

/// Immediate acknowledgement for an accepted asynchronous query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryAccepted {
    /// Opaque operation id used for events, snapshots, and cancellation.
    pub operation_id: String,
}

#[cfg(test)]
mod tests {
    use super::JdbcValue;

    #[test]
    fn every_jdbc_value_variant_round_trips_losslessly() {
        let values = vec![
            JdbcValue::Null,
            JdbcValue::Boolean { value: true },
            JdbcValue::SignedInteger {
                value: "-9223372036854775808".to_owned(),
            },
            JdbcValue::UnsignedInteger {
                value: "18446744073709551615".to_owned(),
            },
            JdbcValue::Float32 {
                value: "NaN".to_owned(),
            },
            JdbcValue::Float64 {
                value: "Infinity".to_owned(),
            },
            JdbcValue::Float64 {
                value: "-Infinity".to_owned(),
            },
            JdbcValue::Decimal {
                value: "1234567890.000001".to_owned(),
            },
            JdbcValue::Text {
                value: "text".to_owned(),
            },
            JdbcValue::Binary {
                value: "AAH/".to_owned(),
            },
            JdbcValue::Date {
                value: "2026-07-25".to_owned(),
            },
            JdbcValue::Time {
                value: "12:34:56".to_owned(),
            },
            JdbcValue::Timestamp {
                value: "2026-07-25T12:34:56".to_owned(),
            },
            JdbcValue::TimestampWithTimeZone {
                value: "2026-07-25T12:34:56+08:00".to_owned(),
            },
            JdbcValue::Json {
                value: "{\"ok\":true}".to_owned(),
            },
            JdbcValue::Uuid {
                value: "550e8400-e29b-41d4-a716-446655440000".to_owned(),
            },
            JdbcValue::Opaque {
                type_name: "vendor.Type".to_owned(),
                display_value: "opaque".to_owned(),
            },
        ];

        for value in values {
            let json = serde_json::to_value(&value).expect("JDBC value must serialize");
            assert!(json["type"].is_string());
            assert_eq!(
                serde_json::from_value::<JdbcValue>(json).expect("JDBC value must deserialize"),
                value
            );
        }
    }

    #[test]
    fn binary_and_non_finite_floats_are_json_strings() {
        let binary = serde_json::to_value(JdbcValue::Binary {
            value: "AAH/".to_owned(),
        })
        .expect("binary must serialize");
        let float = serde_json::to_value(JdbcValue::Float64 {
            value: "NaN".to_owned(),
        })
        .expect("float must serialize");

        assert_eq!(binary["value"], "AAH/");
        assert_eq!(float["value"], "NaN");
    }
}
