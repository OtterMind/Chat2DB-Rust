//! Canonical product contracts shared by every `Chat2DB` Rust delivery surface.

pub mod datasource;
pub mod error;
pub mod operation;
pub mod query;
pub mod result;
pub mod system;

pub use datasource::{
    CreateDatasourceRequest, Datasource, DatasourceConnection, DatasourceConnectionProperty,
    DatasourceList, DatasourceSecretChange, UpdateDatasourceRequest,
};
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
        ApiError, ApiErrorDetails, CancelDisposition, CancelOperationResponse, ColumnNullability,
        ComponentHealth, ComponentState, CreateDatasourceRequest, Datasource, DatasourceConnection,
        DatasourceConnectionProperty, DatasourceList, DatasourceSecretChange, HealthResponse,
        JdbcValue, JdbcValueType, OperationEvent, OperationEventEnvelope, OperationSnapshot,
        OperationStatus, OperationStreamMessage, OperationSubscriptionAccepted, ProductInfo,
        QueryAccepted, QueryLimits, QueryParameter, ResultColumn, ResultMetadata, ResultPage,
        ResultPageRequest, ResultRow, RuntimeStatus, StartQueryRequest, UpdateDatasourceRequest,
    };

    #[derive(OpenApi)]
    #[openapi(components(schemas(
        ApiError,
        ApiErrorDetails,
        CancelDisposition,
        CancelOperationResponse,
        ColumnNullability,
        ComponentHealth,
        ComponentState,
        CreateDatasourceRequest,
        DatasourceConnection,
        DatasourceConnectionProperty,
        Datasource,
        DatasourceList,
        DatasourceSecretChange,
        HealthResponse,
        JdbcValue,
        JdbcValueType,
        OperationEvent,
        OperationEventEnvelope,
        OperationSnapshot,
        OperationStatus,
        OperationStreamMessage,
        OperationSubscriptionAccepted,
        ProductInfo,
        QueryAccepted,
        QueryLimits,
        QueryParameter,
        ResultColumn,
        ResultMetadata,
        ResultPageRequest,
        ResultPage,
        ResultRow,
        RuntimeStatus,
        StartQueryRequest,
        UpdateDatasourceRequest
    )))]
    struct ContractDocument;

    #[test]
    fn every_public_contract_can_be_registered_in_openapi() {
        let document = serde_json::to_value(ContractDocument::openapi())
            .expect("contract OpenAPI document must serialize");
        let schemas = document["components"]["schemas"]
            .as_object()
            .expect("OpenAPI components must contain schemas");

        for schema in [
            "ApiError",
            "Datasource",
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
