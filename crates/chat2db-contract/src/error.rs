use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Stable error envelope used at every external transport boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    /// Stable machine-readable code.
    pub code: String,
    /// Safe user-facing message.
    pub message: String,
    /// Whether the caller may retry this operation under the same policy.
    #[serde(default)]
    pub retryable: bool,
    /// Optional, bounded diagnostic context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<ApiErrorDetails>,
}

impl ApiError {
    /// Creates a non-retryable error without structured details.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
            details: None,
        }
    }
}

/// Bounded structured details for externally visible API failures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApiErrorDetails {
    /// Optimistic concurrency rejected a stale datasource revision.
    RevisionConflict {
        /// Revision supplied by the caller, encoded as a decimal integer.
        #[serde(rename = "expectedRevision")]
        expected_revision: String,
        /// Current revision, or absent when the record no longer exists.
        #[serde(rename = "actualRevision", skip_serializing_if = "Option::is_none")]
        actual_revision: Option<String>,
    },
    /// Safe database diagnostics without SQL text, parameters, or credentials.
    Database {
        /// Vendor-neutral SQLSTATE when one was supplied by the driver.
        #[serde(rename = "sqlState", skip_serializing_if = "Option::is_none")]
        sql_state: Option<String>,
        /// Vendor-specific integer error code.
        #[serde(rename = "vendorCode", skip_serializing_if = "Option::is_none")]
        vendor_code: Option<i32>,
        /// Constraint name when it is safe and available.
        #[serde(rename = "constraintName", skip_serializing_if = "Option::is_none")]
        constraint_name: Option<String>,
        /// One-based statement position when the driver supplies it.
        #[serde(rename = "statementPosition", skip_serializing_if = "Option::is_none")]
        statement_position: Option<u32>,
    },
    /// A requested operation-event sequence has fallen outside replay retention.
    ReplayWindow {
        /// Requested sequence encoded as a decimal integer.
        #[serde(rename = "requestedSequence")]
        requested_sequence: String,
        /// Oldest retained sequence encoded as a decimal integer.
        #[serde(rename = "oldestAvailableSequence")]
        oldest_available_sequence: String,
        /// Latest retained sequence encoded as a decimal integer.
        #[serde(rename = "latestSequence")]
        latest_sequence: String,
    },
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ApiError, ApiErrorDetails};

    #[test]
    fn prior_error_without_retryable_still_deserializes() {
        let error: ApiError = serde_json::from_value(json!({
            "code": "route_not_found",
            "message": "missing"
        }))
        .expect("the original error envelope must remain readable");

        assert!(!error.retryable);
        assert!(error.details.is_none());
    }

    #[test]
    fn tagged_error_details_round_trip_with_string_revisions() {
        let error = ApiError {
            code: "revision_conflict".to_owned(),
            message: "Datasource changed".to_owned(),
            retryable: true,
            details: Some(ApiErrorDetails::RevisionConflict {
                expected_revision: "9007199254740993".to_owned(),
                actual_revision: Some("9007199254740994".to_owned()),
            }),
        };

        let json = serde_json::to_value(&error).expect("error must serialize");
        assert_eq!(json["details"]["type"], "revision_conflict");
        assert_eq!(json["details"]["expectedRevision"], "9007199254740993");
        assert_eq!(
            serde_json::from_value::<ApiError>(json).expect("error must deserialize"),
            error
        );
    }
}
