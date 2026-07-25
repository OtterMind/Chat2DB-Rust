//! Canonical product contracts shared by every `Chat2DB` Rust delivery surface.

pub mod agent;
pub mod datasource;
pub mod driver;
pub mod error;
pub mod operation;
pub mod query;
pub mod result;
pub mod system;

pub use agent::{
    AgentEvent, AgentEventEnvelope, AgentMessage, AgentMessageContent, AgentMessageList,
    AgentMessageRole, AgentPermissionDecision, AgentPermissionRequest, AgentPermissionResponse,
    AgentPermissionStatus, AgentResultHandle, AgentRunAccepted, AgentRunSnapshot, AgentRunStatus,
    AgentSession, AgentSessionList, AgentStreamMessage, AgentSubscriptionAccepted, AgentToolCall,
    AgentToolOutput, AgentUsage, CancelAgentRunResponse, ContextCompactionStrategy,
    CreateAgentSessionRequest, CreateProviderProfileRequest, DecideAgentPermissionRequest,
    ProviderCredentials, ProviderKind, ProviderProfile, ProviderProfileList, ProviderSecretChange,
    SqlPermissionMode, StartAgentRunRequest, UpdateAgentSessionRequest,
    UpdateProviderProfileRequest,
};
pub use datasource::{
    CreateDatasourceRequest, Datasource, DatasourceConnection, DatasourceConnectionProperty,
    DatasourceList, DatasourceSecretChange, UpdateDatasourceRequest,
};
pub use driver::{JdbcDriver, JdbcDriverList};
pub use error::{ApiError, ApiErrorDetails};
pub use operation::{
    CancelDisposition, CancelOperationResponse, OperationEvent, OperationEventEnvelope,
    OperationSnapshot, OperationStatus, OperationStreamMessage, OperationSubscriptionAccepted,
};
pub use query::{JdbcValue, QueryAccepted, QueryLimits, QueryParameter, StartQueryRequest};
pub use result::{
    ColumnNullability, JdbcValueType, ResultColumn, ResultMetadata, ResultPage, ResultPageRequest,
    ResultRow,
};
pub use system::{ComponentHealth, ComponentState, HealthResponse, ProductInfo, RuntimeStatus};

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use utoipa::OpenApi;

    use crate::{
        AgentEvent, AgentEventEnvelope, AgentMessage, AgentMessageContent, AgentMessageList,
        AgentMessageRole, AgentPermissionDecision, AgentPermissionRequest, AgentPermissionResponse,
        AgentPermissionStatus, AgentResultHandle, AgentRunAccepted, AgentRunSnapshot,
        AgentRunStatus, AgentSession, AgentSessionList, AgentStreamMessage,
        AgentSubscriptionAccepted, AgentToolCall, AgentToolOutput, AgentUsage, ApiError,
        ApiErrorDetails, CancelAgentRunResponse, CancelDisposition, CancelOperationResponse,
        ColumnNullability, ComponentHealth, ComponentState, ContextCompactionStrategy,
        CreateAgentSessionRequest, CreateDatasourceRequest, CreateProviderProfileRequest,
        Datasource, DatasourceConnection, DatasourceConnectionProperty, DatasourceList,
        DatasourceSecretChange, DecideAgentPermissionRequest, HealthResponse, JdbcDriver,
        JdbcDriverList, JdbcValue, JdbcValueType, OperationEvent, OperationEventEnvelope,
        OperationSnapshot, OperationStatus, OperationStreamMessage, OperationSubscriptionAccepted,
        ProductInfo, ProviderCredentials, ProviderKind, ProviderProfile, ProviderProfileList,
        ProviderSecretChange, QueryAccepted, QueryLimits, QueryParameter, ResultColumn,
        ResultMetadata, ResultPage, ResultPageRequest, ResultRow, RuntimeStatus, SqlPermissionMode,
        StartAgentRunRequest, StartQueryRequest, UpdateAgentSessionRequest,
        UpdateDatasourceRequest, UpdateProviderProfileRequest,
    };

    #[derive(OpenApi)]
    #[openapi(components(schemas(
        ApiError,
        ApiErrorDetails,
        AgentEvent,
        AgentEventEnvelope,
        AgentMessage,
        AgentMessageContent,
        AgentMessageList,
        AgentMessageRole,
        AgentPermissionDecision,
        AgentPermissionRequest,
        AgentPermissionResponse,
        AgentPermissionStatus,
        AgentResultHandle,
        AgentRunAccepted,
        AgentRunSnapshot,
        AgentRunStatus,
        AgentSession,
        AgentSessionList,
        AgentStreamMessage,
        AgentSubscriptionAccepted,
        AgentToolCall,
        AgentToolOutput,
        AgentUsage,
        CancelAgentRunResponse,
        CancelDisposition,
        CancelOperationResponse,
        ColumnNullability,
        ComponentHealth,
        ComponentState,
        ContextCompactionStrategy,
        CreateAgentSessionRequest,
        CreateDatasourceRequest,
        CreateProviderProfileRequest,
        DatasourceConnection,
        DatasourceConnectionProperty,
        Datasource,
        DatasourceList,
        DatasourceSecretChange,
        DecideAgentPermissionRequest,
        HealthResponse,
        JdbcDriver,
        JdbcDriverList,
        JdbcValue,
        JdbcValueType,
        OperationEvent,
        OperationEventEnvelope,
        OperationSnapshot,
        OperationStatus,
        OperationStreamMessage,
        OperationSubscriptionAccepted,
        ProductInfo,
        ProviderCredentials,
        ProviderKind,
        ProviderProfile,
        ProviderProfileList,
        ProviderSecretChange,
        QueryAccepted,
        QueryLimits,
        QueryParameter,
        ResultColumn,
        ResultMetadata,
        ResultPageRequest,
        ResultPage,
        ResultRow,
        RuntimeStatus,
        SqlPermissionMode,
        StartAgentRunRequest,
        StartQueryRequest,
        UpdateAgentSessionRequest,
        UpdateProviderProfileRequest,
        UpdateDatasourceRequest
    )))]
    struct ContractDocument;

    #[test]
    fn every_public_contract_can_be_registered_in_openapi() {
        let document = serde_json::to_value(ContractDocument::openapi())
            .expect("contract OpenAPI document must serialize");
        let encoded_document =
            serde_json::to_string(&document).expect("contract OpenAPI document must encode");
        let schemas = document["components"]["schemas"]
            .as_object()
            .expect("OpenAPI components must contain schemas");

        for schema in [
            "ApiError",
            "AgentEventEnvelope",
            "AgentRunSnapshot",
            "Datasource",
            "JdbcDriverList",
            "JdbcValue",
            "OperationEventEnvelope",
            "OperationStreamMessage",
            "ResultPage",
            "StartQueryRequest",
        ] {
            assert!(
                schemas.contains_key(schema),
                "missing OpenAPI schema {schema}"
            );
        }

        assert_schema_property_is_string(schemas, "Datasource", "revision");
        assert_schema_property_is_string(schemas, "OperationEventEnvelope", "sequence");
        assert_schema_property_is_string(schemas, "ResultPageRequest", "offset");
        assert_schema_property_is_string(schemas, "ResultMetadata", "rowCount");
        assert_eq!(
            schemas["ProviderCredentials"]["properties"]["apiKey"]["writeOnly"], true,
            "provider credentials must remain write-only in OpenAPI"
        );
        assert!(
            encoded_document.contains("\"toolCallId\""),
            "agent tagged fields must use camelCase in OpenAPI"
        );
        assert!(
            !encoded_document.contains("\"tool_call_id\""),
            "agent tagged fields must not expose snake_case in OpenAPI"
        );
    }

    fn assert_schema_property_is_string(
        schemas: &serde_json::Map<String, Value>,
        schema: &str,
        property: &str,
    ) {
        assert_eq!(
            schemas[schema]["properties"][property]["type"], "string",
            "{schema}.{property} must remain a JSON string"
        );
    }
}
