use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{ApiError, ResultMetadata};

/// One replayable operation event with a monotonically increasing sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationEventEnvelope {
    /// Opaque operation id.
    pub operation_id: String,
    /// Monotonic per-operation sequence encoded as a decimal integer.
    pub sequence: String,
    /// Event creation time as Unix epoch milliseconds encoded as a decimal integer.
    pub occurred_at_ms: String,
    /// Typed lifecycle event.
    pub event: OperationEvent,
}

/// One desktop operation-channel message.
///
/// Unlike an SSE connection, a Tauri channel has no implicit EOF signal. This
/// tagged envelope therefore makes events, failures, and clean completion
/// explicit while preserving the same canonical event and error contracts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationStreamMessage {
    /// One replayable lifecycle event.
    Event {
        /// Event delivered by the operation journal.
        event: OperationEventEnvelope,
    },
    /// The subscription failed and cannot produce more events.
    Error {
        /// Safe external subscription failure.
        error: ApiError,
    },
    /// The subscription reached a clean end.
    End,
}

/// Immediate acknowledgement for an established desktop observer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationSubscriptionAccepted {
    /// Opaque observer id used only to release this subscription.
    pub subscription_id: String,
}

/// Replayable asynchronous query lifecycle event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationEvent {
    /// Execution has started.
    Started,
    /// Bounded progress counters have advanced.
    Progress {
        /// Observed row count encoded as a decimal integer.
        #[serde(rename = "rowCount")]
        row_count: String,
        /// Persisted encoded byte count encoded as a decimal integer.
        #[serde(rename = "byteCount")]
        byte_count: String,
    },
    /// Execution completed and the retained result is immutable.
    Completed {
        /// Durable result metadata.
        result: ResultMetadata,
    },
    /// Execution failed with a safe external error.
    Failed {
        /// Terminal failure.
        error: ApiError,
    },
    /// Execution was cancelled.
    Cancelled {
        /// Optional safe cancellation reason.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

/// Current operation state returned by snapshot APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    /// Execution is active.
    Running,
    /// Execution completed successfully.
    Completed,
    /// Execution failed.
    Failed,
    /// Execution was cancelled.
    Cancelled,
}

/// Materialized operation state for reconnect and polling clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationSnapshot {
    /// Opaque operation id.
    pub operation_id: String,
    /// Current lifecycle state.
    pub status: OperationStatus,
    /// Latest emitted sequence encoded as a decimal integer.
    pub last_sequence: String,
    /// Operation start time as Unix epoch milliseconds encoded as a decimal integer.
    pub started_at_ms: String,
    /// Latest state-change time as Unix epoch milliseconds encoded as a decimal integer.
    pub updated_at_ms: String,
    /// Latest observed row count encoded as a decimal integer.
    pub row_count: String,
    /// Latest persisted encoded byte count encoded as a decimal integer.
    pub byte_count: String,
    /// Immutable result metadata after successful completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ResultMetadata>,
    /// Terminal error after failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}

/// Result of an idempotent cancellation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CancelDisposition {
    /// Cancellation was accepted for an active operation.
    Accepted,
    /// The operation was already terminal.
    AlreadyTerminal,
    /// No operation exists for the supplied id.
    UnknownOperation,
}

/// Cancellation response shared by Web and desktop transports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelOperationResponse {
    /// Opaque operation id supplied by the caller.
    pub operation_id: String,
    /// Idempotent cancellation disposition.
    pub disposition: CancelDisposition,
}

#[cfg(test)]
mod tests {
    use crate::ApiError;

    use super::{OperationEvent, OperationEventEnvelope, OperationStreamMessage};

    #[test]
    fn event_envelope_round_trips_with_string_sequence_and_timestamp() {
        let event = OperationEventEnvelope {
            operation_id: "operation-1".to_owned(),
            sequence: "9007199254740993".to_owned(),
            occurred_at_ms: "1784900000000".to_owned(),
            event: OperationEvent::Progress {
                row_count: "9007199254740994".to_owned(),
                byte_count: "9007199254740995".to_owned(),
            },
        };

        let json = serde_json::to_value(&event).expect("event must serialize");
        assert_eq!(json["sequence"], "9007199254740993");
        assert_eq!(json["event"]["type"], "progress");
        assert_eq!(
            serde_json::from_value::<OperationEventEnvelope>(json).expect("event must deserialize"),
            event
        );
    }

    #[test]
    fn desktop_stream_messages_have_explicit_tagged_outcomes() {
        let event = OperationEventEnvelope {
            operation_id: "operation-1".to_owned(),
            sequence: "1".to_owned(),
            occurred_at_ms: "1784900000000".to_owned(),
            event: OperationEvent::Started,
        };
        let messages = [
            OperationStreamMessage::Event { event },
            OperationStreamMessage::Error {
                error: ApiError::new("stream_failed", "The observer failed"),
            },
            OperationStreamMessage::End,
        ];

        let json = messages
            .iter()
            .map(|message| serde_json::to_value(message).expect("message must serialize"))
            .collect::<Vec<_>>();
        assert_eq!(json[0]["type"], "event");
        assert_eq!(json[0]["event"]["sequence"], "1");
        assert_eq!(json[1]["type"], "error");
        assert_eq!(json[1]["error"]["code"], "stream_failed");
        assert_eq!(json[2]["type"], "end");

        for (value, expected) in json.into_iter().zip(messages) {
            assert_eq!(
                serde_json::from_value::<OperationStreamMessage>(value)
                    .expect("message must deserialize"),
                expected
            );
        }
    }
}
