//! Canonical product contracts shared by every `Chat2DB` Rust delivery surface.

pub mod agent;
pub mod community;
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
pub use community::{
    BuildCommunityCreateSchemaRequest, BuildCommunityDmlRequest, BuildCommunityNamespaceSqlRequest,
    CommunityBuiltSql, CommunityDatabase, CommunityDatabaseList, CommunityDmlAssignment,
    CommunityDmlColumn, CommunityDmlRow, CommunityDmlStatement, CommunityDmlTarget,
    CommunityDmlTemporalKind, CommunityDmlValue, CommunityDriverConfig, CommunityForeignKey,
    CommunityForeignKeyList, CommunityFormattedSql, CommunityFunction, CommunityFunctionList,
    CommunityFunctionParameter, CommunityFunctionParameterList, CommunityNamespaceSqlOperation,
    CommunityParsedStatement, CommunityPlugin, CommunityPluginBehavior, CommunityPluginCatalog,
    CommunityPluginServices, CommunityPrimaryKey, CommunityPrimaryKeyList, CommunityProcedure,
    CommunityProcedureList, CommunityProcedureParameter, CommunityProcedureParameterList,
    CommunityRoutineInvocationPreview, CommunitySchema, CommunitySchemaList, CommunitySqlAnalysis,
    CommunitySqlCompletion, CommunitySqlCompletionActiveSnippetSlot,
    CommunitySqlCompletionCandidate, CommunitySqlCompletionEditorHint,
    CommunitySqlCompletionEditorHintItem, CommunitySqlCompletionRange, CommunitySqlDiagnostic,
    CommunitySqlValidation, CommunityTable, CommunityTableColumn, CommunityTableColumnList,
    CommunityTableIndex, CommunityTableIndexColumn, CommunityTableIndexList, CommunityTableList,
    CommunityTablePreviewAccepted, CommunityTrigger, CommunityTriggerList, CommunityViewList,
    CompleteCommunitySqlRequest, FormatCommunitySqlRequest, GetCommunityFunctionRequest,
    GetCommunityProcedureRequest, GetCommunityTriggerRequest, ListCommunityColumnsRequest,
    ListCommunityDatabasesRequest, ListCommunityFunctionsRequest, ListCommunityIndexesRequest,
    ListCommunityProceduresRequest, ListCommunitySchemasRequest, ListCommunityTableKeysRequest,
    ListCommunityTablesRequest, ListCommunityTriggersRequest, ListCommunityViewsRequest,
    ParseCommunitySqlRequest, PreviewCommunityRoutineInvocationRequest,
    StartCommunityTablePreviewRequest, ValidateCommunitySqlRequest,
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
        ApiErrorDetails, BuildCommunityCreateSchemaRequest, CancelAgentRunResponse,
        CancelDisposition, CancelOperationResponse, ColumnNullability, CommunityBuiltSql,
        CommunityDatabase, CommunityDatabaseList, CommunityDriverConfig, CommunityForeignKey,
        CommunityForeignKeyList, CommunityFormattedSql, CommunityParsedStatement, CommunityPlugin,
        CommunityPluginBehavior, CommunityPluginCatalog, CommunityPluginServices,
        CommunityPrimaryKey, CommunityPrimaryKeyList, CommunitySchema, CommunitySchemaList,
        CommunitySqlAnalysis, CommunitySqlCompletion, CommunitySqlCompletionActiveSnippetSlot,
        CommunitySqlCompletionCandidate, CommunitySqlCompletionEditorHint,
        CommunitySqlCompletionEditorHintItem, CommunitySqlCompletionRange, CommunitySqlDiagnostic,
        CommunitySqlValidation, CommunityTable, CommunityTableColumn, CommunityTableColumnList,
        CommunityTableIndex, CommunityTableIndexColumn, CommunityTableIndexList,
        CommunityTableList, CommunityTablePreviewAccepted, CommunityViewList,
        CompleteCommunitySqlRequest, ComponentHealth, ComponentState, ContextCompactionStrategy,
        CreateAgentSessionRequest, CreateDatasourceRequest, CreateProviderProfileRequest,
        Datasource, DatasourceConnection, DatasourceConnectionProperty, DatasourceList,
        DatasourceSecretChange, DecideAgentPermissionRequest, FormatCommunitySqlRequest,
        HealthResponse, JdbcDriver, JdbcDriverList, JdbcValue, JdbcValueType,
        ListCommunityColumnsRequest, ListCommunityDatabasesRequest, ListCommunityIndexesRequest,
        ListCommunitySchemasRequest, ListCommunityTableKeysRequest, ListCommunityTablesRequest,
        ListCommunityViewsRequest, OperationEvent, OperationEventEnvelope, OperationSnapshot,
        OperationStatus, OperationStreamMessage, OperationSubscriptionAccepted,
        ParseCommunitySqlRequest, ProductInfo, ProviderCredentials, ProviderKind, ProviderProfile,
        ProviderProfileList, ProviderSecretChange, QueryAccepted, QueryLimits, QueryParameter,
        ResultColumn, ResultMetadata, ResultPage, ResultPageRequest, ResultRow, RuntimeStatus,
        SqlPermissionMode, StartAgentRunRequest, StartCommunityTablePreviewRequest,
        StartQueryRequest, UpdateAgentSessionRequest, UpdateDatasourceRequest,
        UpdateProviderProfileRequest, ValidateCommunitySqlRequest,
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
        CommunityBuiltSql,
        CommunityDatabase,
        CommunityDatabaseList,
        CommunityDriverConfig,
        CommunityForeignKey,
        CommunityForeignKeyList,
        CommunityFormattedSql,
        CommunityParsedStatement,
        CommunityPlugin,
        CommunityPluginBehavior,
        CommunityPluginCatalog,
        CommunityPluginServices,
        CommunityPrimaryKey,
        CommunityPrimaryKeyList,
        CommunitySchema,
        CommunitySchemaList,
        CommunitySqlAnalysis,
        CommunitySqlDiagnostic,
        CommunitySqlValidation,
        CommunitySqlCompletion,
        CommunitySqlCompletionActiveSnippetSlot,
        CommunitySqlCompletionCandidate,
        CommunitySqlCompletionEditorHint,
        CommunitySqlCompletionEditorHintItem,
        CommunitySqlCompletionRange,
        CommunityTable,
        CommunityTableColumn,
        CommunityTableColumnList,
        CommunityTableIndex,
        CommunityTableIndexColumn,
        CommunityTableIndexList,
        CommunityTableList,
        CommunityTablePreviewAccepted,
        CommunityViewList,
        ColumnNullability,
        ComponentHealth,
        ComponentState,
        ContextCompactionStrategy,
        BuildCommunityCreateSchemaRequest,
        CreateAgentSessionRequest,
        CreateDatasourceRequest,
        CreateProviderProfileRequest,
        DatasourceConnection,
        DatasourceConnectionProperty,
        Datasource,
        DatasourceList,
        DatasourceSecretChange,
        DecideAgentPermissionRequest,
        FormatCommunitySqlRequest,
        HealthResponse,
        JdbcDriver,
        JdbcDriverList,
        JdbcValue,
        JdbcValueType,
        ListCommunityColumnsRequest,
        ListCommunityDatabasesRequest,
        ListCommunityIndexesRequest,
        ListCommunitySchemasRequest,
        ListCommunityTableKeysRequest,
        ListCommunityTablesRequest,
        ListCommunityViewsRequest,
        OperationEvent,
        OperationEventEnvelope,
        OperationSnapshot,
        OperationStatus,
        OperationStreamMessage,
        OperationSubscriptionAccepted,
        ParseCommunitySqlRequest,
        ValidateCommunitySqlRequest,
        CompleteCommunitySqlRequest,
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
        StartCommunityTablePreviewRequest,
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
            "CommunityPluginCatalog",
            "CommunityDatabaseList",
            "CommunitySchemaList",
            "CommunitySqlAnalysis",
            "CommunitySqlDiagnostic",
            "CommunitySqlValidation",
            "CommunitySqlCompletion",
            "CommunitySqlCompletionCandidate",
            "CommunitySqlCompletionEditorHint",
            "CompleteCommunitySqlRequest",
            "ValidateCommunitySqlRequest",
            "CommunityTableList",
            "CommunityTableColumnList",
            "CommunityTableIndexList",
            "CommunityTablePreviewAccepted",
            "CommunityViewList",
            "CommunityForeignKeyList",
            "CommunityFormattedSql",
            "CommunityPrimaryKeyList",
            "FormatCommunitySqlRequest",
            "JdbcDriverList",
            "JdbcValue",
            "OperationEventEnvelope",
            "OperationStreamMessage",
            "ResultPage",
            "StartQueryRequest",
            "StartCommunityTablePreviewRequest",
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
        assert!(
            encoded_document.contains("\"databaseType\"")
                && encoded_document.contains("\"datasourceId\""),
            "Community fields must use camelCase in OpenAPI"
        );
        assert!(
            !encoded_document.contains("\"database_type\"")
                && !encoded_document.contains("\"datasource_id\""),
            "Community fields must not expose snake_case in OpenAPI"
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
