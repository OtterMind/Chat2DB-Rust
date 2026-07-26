//! Axum transport adapter for `Chat2DB` product services.

mod api;
mod error;
mod extract;

use std::{
    error::Error,
    fmt::{Display, Formatter},
    net::SocketAddr,
    path::Path,
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::any,
};
use chat2db_contract::ApiError;
use chat2db_core::Application;
use subtle::ConstantTimeEq;
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

const MINIMUM_ACCESS_TOKEN_BYTES: usize = 32;

/// Validation failure for a non-loopback Web listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessPolicyError {
    /// A non-loopback listener has no bearer token.
    MissingToken,
    /// The configured token is too short for the external boundary.
    WeakToken,
}

impl Display for AccessPolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingToken => formatter
                .write_str("CHAT2DB_ACCESS_TOKEN is required when CHAT2DB_BIND is not loopback"),
            Self::WeakToken => formatter.write_str(
                "CHAT2DB_ACCESS_TOKEN must contain at least 32 bytes when Web is not loopback",
            ),
        }
    }
}

impl Error for AccessPolicyError {}

/// Authentication policy applied before every Web route.
#[derive(Clone, Default)]
pub struct AccessPolicy {
    bearer_token: Option<Arc<[u8]>>,
}

impl std::fmt::Debug for AccessPolicy {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccessPolicy")
            .field("bearer_token_configured", &self.bearer_token.is_some())
            .finish()
    }
}

impl AccessPolicy {
    /// Builds a fail-closed policy for the selected listener.
    ///
    /// # Errors
    ///
    /// Returns [`AccessPolicyError::MissingToken`] or
    /// [`AccessPolicyError::WeakToken`] when a non-loopback listener does not
    /// have a sufficiently strong bearer token.
    pub fn for_bind(
        address: SocketAddr,
        configured_token: Option<String>,
    ) -> Result<Self, AccessPolicyError> {
        if address.ip().is_loopback() {
            return Ok(Self::default());
        }

        let token = configured_token
            .filter(|candidate| !candidate.is_empty())
            .ok_or(AccessPolicyError::MissingToken)?;
        if token.len() < MINIMUM_ACCESS_TOKEN_BYTES {
            return Err(AccessPolicyError::WeakToken);
        }

        Ok(Self {
            bearer_token: Some(Arc::from(token.into_bytes())),
        })
    }
}

/// Builds the loopback Web router around a shared application service root.
pub fn router(application: Application) -> Router {
    router_with_policy(application, AccessPolicy::default())
}

/// Returns the deterministic `OpenAPI` document generated from registered handlers.
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    api::openapi()
}

/// Builds the complete Web router with its listener-derived access policy.
pub fn router_with_policy(application: Application, access_policy: AccessPolicy) -> Router {
    let router = api::router(application)
        .fallback(api::not_found)
        .method_not_allowed_fallback(api::method_not_allowed);
    with_common_layers(router, access_policy)
}

/// Builds the Web router with API isolation and a Vite single-page application.
///
/// Missing frontend paths fall back to `index.html`, while every unknown
/// `/api` path remains a JSON [`ApiError`] response.
pub fn router_with_policy_and_assets(
    application: Application,
    access_policy: AccessPolicy,
    assets_dir: impl AsRef<Path>,
) -> Router {
    let assets_dir = assets_dir.as_ref();
    let spa = ServeDir::new(assets_dir).fallback(ServeFile::new(assets_dir.join("index.html")));
    let router = api::router(application)
        .route("/api", any(api::not_found))
        .route("/api/", any(api::not_found))
        .route("/api/{*path}", any(api::not_found))
        .fallback_service(spa)
        .method_not_allowed_fallback(api::method_not_allowed);
    with_common_layers(router, access_policy)
}

fn with_common_layers(router: Router, access_policy: AccessPolicy) -> Router {
    router
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(access_policy, authorize))
}

async fn authorize(State(policy): State<AccessPolicy>, request: Request, next: Next) -> Response {
    let Some(expected_token) = policy.bearer_token.as_deref() else {
        return next.run(request).await;
    };

    let provided_token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::as_bytes);

    if provided_token.is_some_and(|token| bool::from(expected_token.ct_eq(token))) {
        return next.run(request).await;
    }

    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
        Json(ApiError::new(
            "unauthorized",
            "A valid Chat2DB access token is required",
        )),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::Arc,
        time::Duration,
    };

    use axum::{
        body::Body,
        http::{Method, Request, StatusCode, header},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::{
        AccessPolicy, AccessPolicyError, openapi, router, router_with_policy,
        router_with_policy_and_assets,
    };
    use chat2db_contract::{
        AgentMessageContent, AgentMessageList, AgentRunAccepted, AgentRunSnapshot, AgentRunStatus,
        AgentSession, AgentSessionList, ApiError, ApiErrorDetails, CancelAgentRunResponse,
        CancelDisposition, Datasource, DatasourceList, HealthResponse, JdbcDriverList,
        ProviderProfile, ProviderProfileList, RuntimeStatus,
    };
    use chat2db_core::Application;
    use chat2db_storage::{
        AgentMessageRole, AppendAgentMessage, MAX_AGENT_MESSAGE_BYTES, SecretRef, SecretValue,
        SecretVault, SecretVaultError, Storage,
    };
    use tempfile::TempDir;

    #[tokio::test]
    async fn health_route_uses_the_shared_contract() {
        let response = router(Application::new())
            .oneshot(request(Method::GET, "/api/v1/system/health"))
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::OK);
        let health: HealthResponse = response_json(response).await;
        assert_eq!(health.product.name, "Chat2DB Rust");
    }

    #[tokio::test]
    async fn unavailable_health_maps_to_service_unavailable() {
        let response = router(Application::with_runtime_status(RuntimeStatus::Unavailable))
            .oneshot(request(Method::GET, "/api/v1/system/health"))
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let health: HealthResponse = response_json(response).await;
        assert_eq!(health.status, RuntimeStatus::Unavailable);
    }

    #[tokio::test]
    async fn driver_inventory_route_uses_the_shared_contract() {
        let response = router(Application::new())
            .oneshot(request(Method::GET, "/api/v1/drivers"))
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::OK);
        let inventory: JdbcDriverList = response_json(response).await;
        assert!(inventory.items.is_empty());
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn community_routes_report_an_unavailable_engine() {
        let application = router(Application::new());
        let requests = [
            (
                request(Method::GET, "/api/v1/community/plugins"),
                "database_engine_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/schemas",
                    &serde_json::json!({
                        "datasourceId": "datasource-1",
                        "databaseType": "H2",
                        "databaseName": "inventory"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/databases",
                    &serde_json::json!({
                        "datasourceId": "datasource-1",
                        "databaseType": "H2"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/tables",
                    &serde_json::json!({
                        "datasourceId": "datasource-1",
                        "databaseType": "H2",
                        "databaseName": "inventory",
                        "schemaName": "APP",
                        "tableNamePattern": "item%"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/columns",
                    &serde_json::json!({
                        "datasourceId": "datasource-1",
                        "databaseType": "H2",
                        "databaseName": "inventory",
                        "schemaName": "APP",
                        "tableName": "items"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/indexes",
                    &serde_json::json!({
                        "datasourceId": "datasource-1",
                        "databaseType": "H2",
                        "databaseName": "inventory",
                        "schemaName": "APP",
                        "tableName": "items"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/views",
                    &serde_json::json!({
                        "datasourceId": "datasource-1",
                        "databaseType": "H2",
                        "databaseName": "inventory",
                        "schemaName": "APP",
                        "viewNamePattern": "item%"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/imported-keys",
                    &serde_json::json!({
                        "datasourceId": "datasource-1",
                        "databaseType": "H2",
                        "databaseName": "inventory",
                        "schemaName": "APP",
                        "tableName": "items"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/exported-keys",
                    &serde_json::json!({
                        "datasourceId": "datasource-1",
                        "databaseType": "H2",
                        "databaseName": "inventory",
                        "schemaName": "APP",
                        "tableName": "items"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/primary-keys",
                    &serde_json::json!({
                        "datasourceId": "datasource-1",
                        "databaseType": "H2",
                        "databaseName": "inventory",
                        "schemaName": "APP",
                        "tableName": "items"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/functions",
                    &serde_json::json!({
                        "datasourceId": "datasource-1", "databaseType": "H2",
                        "databaseName": "inventory", "schemaName": "APP"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/function",
                    &serde_json::json!({
                        "datasourceId": "datasource-1", "databaseType": "H2",
                        "databaseName": "inventory", "schemaName": "APP",
                        "functionName": "double_value"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/function-parameters",
                    &serde_json::json!({
                        "datasourceId": "datasource-1", "databaseType": "H2",
                        "databaseName": "inventory", "schemaName": "APP",
                        "functionName": "double_value"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/procedures",
                    &serde_json::json!({
                        "datasourceId": "datasource-1", "databaseType": "H2",
                        "databaseName": "inventory", "schemaName": "APP"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/procedure",
                    &serde_json::json!({
                        "datasourceId": "datasource-1", "databaseType": "H2",
                        "databaseName": "inventory", "schemaName": "APP",
                        "procedureName": "refresh_items"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/procedure-parameters",
                    &serde_json::json!({
                        "datasourceId": "datasource-1", "databaseType": "H2",
                        "databaseName": "inventory", "schemaName": "APP",
                        "procedureName": "refresh_items"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/triggers",
                    &serde_json::json!({
                        "datasourceId": "datasource-1", "databaseType": "H2",
                        "databaseName": "inventory", "schemaName": "APP"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/metadata/trigger",
                    &serde_json::json!({
                        "datasourceId": "datasource-1", "databaseType": "H2",
                        "databaseName": "inventory", "schemaName": "APP",
                        "triggerName": "items_audit"
                    }),
                ),
                "storage_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/sql/build-create-schema",
                    &serde_json::json!({
                        "databaseType": "H2",
                        "schema": {
                            "databaseName": "inventory",
                            "name": "reporting",
                            "comment": "",
                            "owner": "",
                            "system": false
                        }
                    }),
                ),
                "database_engine_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/sql/parse",
                    &serde_json::json!({
                        "databaseType": "H2",
                        "sql": "select 1"
                    }),
                ),
                "database_engine_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/sql/validate",
                    &serde_json::json!({
                        "databaseType": "H2",
                        "sql": "select from"
                    }),
                ),
                "database_engine_unavailable",
            ),
            (
                json_request(
                    Method::POST,
                    "/api/v1/community/sql/format",
                    &serde_json::json!({
                        "databaseType": "H2",
                        "sql": "select 1"
                    }),
                ),
                "database_engine_unavailable",
            ),
        ];

        for (request, expected_code) in requests {
            let response = application
                .clone()
                .oneshot(request)
                .await
                .expect("router must respond");

            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let error: ApiError = response_json(response).await;
            assert_eq!(error.code, expected_code);
        }
    }

    #[tokio::test]
    async fn unknown_routes_use_the_error_contract() {
        let response = router(Application::new())
            .oneshot(request(Method::GET, "/api/v1/missing"))
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let error: ApiError = response_json(response).await;
        assert_eq!(error.code, "route_not_found");
    }

    #[tokio::test]
    async fn unsupported_methods_use_the_error_contract() {
        let response = router(Application::new())
            .oneshot(request(Method::POST, "/api/v1/system/health"))
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        let error: ApiError = response_json(response).await;
        assert_eq!(error.code, "method_not_allowed");
    }

    #[tokio::test]
    async fn malformed_json_uses_the_error_contract() {
        let response = router(Application::new())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/datasources")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":"unfinished"#))
                    .expect("request must build"),
            )
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error: ApiError = response_json(response).await;
        assert_eq!(error.code, "invalid_json");
    }

    #[tokio::test]
    async fn missing_required_query_parameter_uses_the_error_contract() {
        let response = router(Application::new())
            .oneshot(request(Method::DELETE, "/api/v1/datasources/source-1"))
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error: ApiError = response_json(response).await;
        assert_eq!(error.code, "invalid_query");
    }

    #[tokio::test]
    async fn malformed_last_event_id_is_rejected_before_subscribing() {
        for path in [
            "/api/v1/operations/operation-1/events",
            "/api/v1/agent/runs/run-1/events",
        ] {
            let response = router(Application::new())
                .oneshot(
                    Request::builder()
                        .method(Method::GET)
                        .uri(path)
                        .header("last-event-id", "not-a-sequence")
                        .body(Body::empty())
                        .expect("request must build"),
                )
                .await
                .expect("router must respond");

            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let error: ApiError = response_json(response).await;
            assert_eq!(error.code, "invalid_last_event_id");
        }
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn openapi_document_is_generated_from_registered_handlers() {
        let response = router(Application::new())
            .oneshot(request(Method::GET, "/api/v1/openapi.json"))
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::OK);
        let document: serde_json::Value = response_json(response).await;
        assert_eq!(
            document,
            serde_json::to_value(openapi()).expect("direct OpenAPI document must serialize")
        );
        let paths = document["paths"]
            .as_object()
            .expect("OpenAPI document must contain paths");

        for path in [
            "/api/v1/system/health",
            "/api/v1/drivers",
            "/api/v1/community/plugins",
            "/api/v1/community/metadata/schemas",
            "/api/v1/community/metadata/databases",
            "/api/v1/community/metadata/tables",
            "/api/v1/community/metadata/columns",
            "/api/v1/community/metadata/indexes",
            "/api/v1/community/metadata/views",
            "/api/v1/community/metadata/imported-keys",
            "/api/v1/community/metadata/exported-keys",
            "/api/v1/community/metadata/primary-keys",
            "/api/v1/community/metadata/functions",
            "/api/v1/community/metadata/function",
            "/api/v1/community/metadata/function-parameters",
            "/api/v1/community/metadata/procedures",
            "/api/v1/community/metadata/procedure",
            "/api/v1/community/metadata/procedure-parameters",
            "/api/v1/community/metadata/triggers",
            "/api/v1/community/metadata/trigger",
            "/api/v1/community/sql/build-create-schema",
            "/api/v1/community/sql/parse",
            "/api/v1/community/sql/validate",
            "/api/v1/community/sql/format",
            "/api/v1/datasources",
            "/api/v1/datasources/{datasource_id}",
            "/api/v1/agent/providers",
            "/api/v1/agent/providers/{provider_id}",
            "/api/v1/agent/sessions",
            "/api/v1/agent/sessions/{session_id}",
            "/api/v1/agent/sessions/{session_id}/messages",
            "/api/v1/agent/runs",
            "/api/v1/agent/runs/{run_id}",
            "/api/v1/agent/runs/{run_id}/cancel",
            "/api/v1/agent/runs/{run_id}/events",
            "/api/v1/agent/runs/{run_id}/permissions/{permission_id}/decision",
            "/api/v1/queries",
            "/api/v1/operations/{operation_id}",
            "/api/v1/operations/{operation_id}/cancel",
            "/api/v1/operations/{operation_id}/events",
            "/api/v1/results/{result_id}",
            "/api/v1/openapi.json",
        ] {
            assert!(paths.contains_key(path), "missing OpenAPI path {path}");
        }

        assert!(paths["/api/v1/datasources"].get("get").is_some());
        assert!(paths["/api/v1/datasources"].get("post").is_some());
        assert!(paths["/api/v1/community/plugins"].get("get").is_some());
        for path in [
            "/api/v1/community/metadata/schemas",
            "/api/v1/community/metadata/databases",
            "/api/v1/community/metadata/tables",
            "/api/v1/community/metadata/columns",
            "/api/v1/community/metadata/indexes",
            "/api/v1/community/metadata/views",
            "/api/v1/community/metadata/imported-keys",
            "/api/v1/community/metadata/exported-keys",
            "/api/v1/community/metadata/primary-keys",
            "/api/v1/community/metadata/functions",
            "/api/v1/community/metadata/function",
            "/api/v1/community/metadata/function-parameters",
            "/api/v1/community/metadata/procedures",
            "/api/v1/community/metadata/procedure",
            "/api/v1/community/metadata/procedure-parameters",
            "/api/v1/community/metadata/triggers",
            "/api/v1/community/metadata/trigger",
            "/api/v1/community/sql/build-create-schema",
            "/api/v1/community/sql/parse",
            "/api/v1/community/sql/validate",
            "/api/v1/community/sql/format",
        ] {
            assert!(paths[path].get("post").is_some());
        }
        assert!(
            paths["/api/v1/datasources/{datasource_id}"]
                .get("delete")
                .is_some()
        );
        assert!(paths["/api/v1/agent/providers"].get("get").is_some());
        assert!(paths["/api/v1/agent/providers"].get("post").is_some());
        assert!(
            paths["/api/v1/agent/providers/{provider_id}"]
                .get("put")
                .is_some()
        );
        assert!(
            paths["/api/v1/agent/providers/{provider_id}"]
                .get("delete")
                .is_some()
        );
        assert!(paths["/api/v1/agent/sessions"].get("get").is_some());
        assert!(paths["/api/v1/agent/sessions"].get("post").is_some());
        assert!(paths["/api/v1/agent/runs"].get("post").is_some());
        assert!(paths["/api/v1/agent/runs/{run_id}"].get("get").is_some());
        assert!(
            paths["/api/v1/agent/runs/{run_id}/cancel"]
                .get("post")
                .is_some()
        );
        assert!(
            paths["/api/v1/agent/runs/{run_id}/events"]
                .get("get")
                .is_some()
        );
        assert!(
            paths["/api/v1/agent/runs/{run_id}/permissions/{permission_id}/decision"]
                .get("post")
                .is_some()
        );
        assert!(
            paths["/api/v1/agent/sessions/{session_id}"]
                .get("put")
                .is_some()
        );
        assert!(
            paths["/api/v1/agent/sessions/{session_id}"]
                .get("delete")
                .is_some()
        );

        let schemas = document["components"]["schemas"]
            .as_object()
            .expect("OpenAPI document must contain schemas");
        for schema in [
            "AgentMessage",
            "AgentMessageContent",
            "AgentMessageList",
            "AgentEvent",
            "AgentEventEnvelope",
            "AgentPermissionDecision",
            "AgentPermissionRequest",
            "AgentPermissionResponse",
            "AgentPermissionStatus",
            "AgentRunAccepted",
            "AgentRunSnapshot",
            "AgentRunStatus",
            "AgentSession",
            "AgentSessionList",
            "AgentUsage",
            "CancelAgentRunResponse",
            "BuildCommunityCreateSchemaRequest",
            "CommunityBuiltSql",
            "CommunityDatabase",
            "CommunityDatabaseList",
            "CommunityDriverConfig",
            "CommunityForeignKey",
            "CommunityForeignKeyList",
            "CommunityFormattedSql",
            "CommunityFunction",
            "CommunityFunctionList",
            "CommunityFunctionParameter",
            "CommunityFunctionParameterList",
            "CommunityParsedStatement",
            "CommunityPlugin",
            "CommunityPluginBehavior",
            "CommunityPluginCatalog",
            "CommunityPluginServices",
            "CommunityPrimaryKey",
            "CommunityPrimaryKeyList",
            "CommunityProcedure",
            "CommunityProcedureList",
            "CommunityProcedureParameter",
            "CommunityProcedureParameterList",
            "CommunitySchema",
            "CommunitySchemaList",
            "CommunitySqlAnalysis",
            "CommunitySqlDiagnostic",
            "CommunitySqlValidation",
            "CommunityTable",
            "CommunityTableColumn",
            "CommunityTableColumnList",
            "CommunityTableIndex",
            "CommunityTableIndexColumn",
            "CommunityTableIndexList",
            "CommunityTableList",
            "CommunityTrigger",
            "CommunityTriggerList",
            "CommunityViewList",
            "ContextCompactionStrategy",
            "CreateAgentSessionRequest",
            "CreateProviderProfileRequest",
            "DecideAgentPermissionRequest",
            "FormatCommunitySqlRequest",
            "GetCommunityFunctionRequest",
            "GetCommunityProcedureRequest",
            "GetCommunityTriggerRequest",
            "ListCommunitySchemasRequest",
            "ListCommunityTableKeysRequest",
            "ListCommunityColumnsRequest",
            "ListCommunityDatabasesRequest",
            "ListCommunityFunctionsRequest",
            "ListCommunityIndexesRequest",
            "ListCommunityProceduresRequest",
            "ListCommunityTablesRequest",
            "ListCommunityTriggersRequest",
            "ListCommunityViewsRequest",
            "ParseCommunitySqlRequest",
            "ValidateCommunitySqlRequest",
            "ProviderCredentials",
            "ProviderProfile",
            "ProviderProfileList",
            "SqlPermissionMode",
            "StartAgentRunRequest",
            "UpdateAgentSessionRequest",
            "UpdateProviderProfileRequest",
        ] {
            assert!(
                schemas.contains_key(schema),
                "missing OpenAPI schema {schema}"
            );
        }
        assert_eq!(
            schemas["ProviderCredentials"]["properties"]["apiKey"]["writeOnly"],
            true
        );
        assert!(
            schemas["ListCommunityTablesRequest"]["properties"]["tableNamePattern"].is_object()
        );
        assert!(schemas["CommunityTable"]["properties"]["tableType"].is_object());
        assert!(schemas["CommunityTableColumn"]["properties"]["primaryKey"].is_object());
        assert!(schemas["CommunityTableIndex"]["properties"]["foreignColumnNames"].is_object());
        for (schema_name, fields) in [
            ("CommunityTable", ["incrementValue", "rows", "dataLength"]),
            (
                "CommunityTableIndexColumn",
                ["cardinality", "pages", "subPart"],
            ),
        ] {
            for field in fields {
                let property = &schemas[schema_name]["properties"][field];
                assert_eq!(property["type"], serde_json::json!(["string", "null"]));
                assert!(property.get("format").is_none());
            }
        }
        assert!(
            schemas["ListCommunityTablesRequest"]["properties"]
                .get("table_name_pattern")
                .is_none()
        );
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn provider_routes_are_secret_safe_and_enforce_revision_cas() {
        const API_KEY: &str = "provider-secret-sentinel";

        let directory = TempDir::new().expect("temp directory");
        let storage =
            Storage::open(directory.path(), Arc::new(TestVault)).expect("test storage must open");
        let application = router(Application::with_storage(storage));

        let create_response = application
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/v1/agent/providers",
                &serde_json::json!({
                    "name": "Primary",
                    "kind": "open_ai_compatible",
                    "baseUrl": "https://provider.example/v1",
                    "model": "model-1",
                    "contextWindowTokens": "9007199254740993",
                    "maxOutputTokens": "8192",
                    "credentials": { "apiKey": API_KEY }
                }),
            ))
            .await
            .expect("router must respond");
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body = create_response
            .into_body()
            .collect()
            .await
            .expect("body must collect")
            .to_bytes();
        let create_json = std::str::from_utf8(&create_body).expect("response must be UTF-8");
        for forbidden in [API_KEY, "apiKey", "credentials", "secretRef"] {
            assert!(!create_json.contains(forbidden));
        }
        let created: ProviderProfile =
            serde_json::from_slice(&create_body).expect("create response must match contract");
        assert!(created.has_secret);
        assert_eq!(created.context_window_tokens, "9007199254740993");

        let list_response = application
            .clone()
            .oneshot(request(Method::GET, "/api/v1/agent/providers"))
            .await
            .expect("router must respond");
        assert_eq!(list_response.status(), StatusCode::OK);
        let listed: ProviderProfileList = response_json(list_response).await;
        assert_eq!(listed.items, vec![created.clone()]);

        let provider_path = format!("/api/v1/agent/providers/{}", created.id);
        let get_response = application
            .clone()
            .oneshot(dynamic_request(Method::GET, &provider_path))
            .await
            .expect("router must respond");
        assert_eq!(get_response.status(), StatusCode::OK);
        let fetched: ProviderProfile = response_json(get_response).await;
        assert_eq!(fetched, created);

        let update_response = application
            .clone()
            .oneshot(json_request(
                Method::PUT,
                &provider_path,
                &serde_json::json!({
                    "expectedRevision": fetched.revision,
                    "name": "Renamed provider",
                    "kind": "open_ai_compatible",
                    "baseUrl": "https://provider.example/v1",
                    "model": "model-2",
                    "contextWindowTokens": "9007199254740993",
                    "maxOutputTokens": "16384",
                    "secretChange": { "action": "keep" }
                }),
            ))
            .await
            .expect("router must respond");
        assert_eq!(update_response.status(), StatusCode::OK);
        let updated: ProviderProfile = response_json(update_response).await;
        assert_eq!(updated.name, "Renamed provider");
        assert!(updated.has_secret);

        let conflict_response = application
            .clone()
            .oneshot(json_request(
                Method::PUT,
                &provider_path,
                &serde_json::json!({
                    "expectedRevision": created.revision,
                    "name": "Stale provider",
                    "kind": "open_ai_compatible",
                    "baseUrl": "https://provider.example/v1",
                    "model": "model-1",
                    "contextWindowTokens": "9007199254740993",
                    "maxOutputTokens": "8192",
                    "secretChange": { "action": "keep" }
                }),
            ))
            .await
            .expect("router must respond");
        assert_eq!(conflict_response.status(), StatusCode::CONFLICT);
        let conflict: ApiError = response_json(conflict_response).await;
        assert_eq!(conflict.code, "provider_revision_conflict");
        assert!(matches!(
            conflict.details,
            Some(ApiErrorDetails::RevisionConflict {
                expected_revision,
                actual_revision: Some(actual_revision),
            }) if expected_revision == created.revision && actual_revision == updated.revision
        ));

        let delete_path = format!(
            "/api/v1/agent/providers/{}?expectedRevision={}",
            updated.id, updated.revision
        );
        let delete_response = application
            .clone()
            .oneshot(dynamic_request(Method::DELETE, &delete_path))
            .await
            .expect("router must respond");
        assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

        let missing_response = application
            .oneshot(dynamic_request(Method::GET, &provider_path))
            .await
            .expect("router must respond");
        assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);
        let error: ApiError = response_json(missing_response).await;
        assert_eq!(error.code, "provider_not_found");
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn agent_session_routes_cover_lifecycle_and_message_pagination() {
        let directory = TempDir::new().expect("temp directory");
        let storage =
            Storage::open(directory.path(), Arc::new(TestVault)).expect("test storage must open");
        let application = router(Application::with_storage(storage.clone()));

        let provider_response = application
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/v1/agent/providers",
                &serde_json::json!({
                    "name": "Primary",
                    "kind": "anthropic",
                    "baseUrl": "https://provider.example/v1",
                    "model": "model-1",
                    "contextWindowTokens": "200000",
                    "maxOutputTokens": "8192"
                }),
            ))
            .await
            .expect("router must respond");
        assert_eq!(provider_response.status(), StatusCode::CREATED);
        let provider: ProviderProfile = response_json(provider_response).await;

        let oversized_prompt = "x".repeat(
            usize::try_from(MAX_AGENT_MESSAGE_BYTES).expect("message limit fits usize") + 1,
        );
        let quota_response = application
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/v1/agent/sessions",
                &serde_json::json!({
                    "title": "Oversized prompt",
                    "providerId": provider.id,
                    "systemPrompt": oversized_prompt
                }),
            ))
            .await
            .expect("router must respond");
        assert_eq!(quota_response.status(), StatusCode::INSUFFICIENT_STORAGE);
        let quota_error: ApiError = response_json(quota_response).await;
        assert_eq!(quota_error.code, "agent_quota_exceeded");

        let create_response = application
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/v1/agent/sessions",
                &serde_json::json!({
                    "title": "First title",
                    "providerId": provider.id,
                    "systemPrompt": "Keep answers bounded"
                }),
            ))
            .await
            .expect("router must respond");
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let created: AgentSession = response_json(create_response).await;

        let list_response = application
            .clone()
            .oneshot(request(Method::GET, "/api/v1/agent/sessions"))
            .await
            .expect("router must respond");
        assert_eq!(list_response.status(), StatusCode::OK);
        let listed: AgentSessionList = response_json(list_response).await;
        assert_eq!(listed.items, vec![created.clone()]);

        let session_path = format!("/api/v1/agent/sessions/{}", created.id);
        let get_response = application
            .clone()
            .oneshot(dynamic_request(Method::GET, &session_path))
            .await
            .expect("router must respond");
        assert_eq!(get_response.status(), StatusCode::OK);
        let fetched: AgentSession = response_json(get_response).await;
        assert_eq!(fetched, created);

        let update_response = application
            .clone()
            .oneshot(json_request(
                Method::PUT,
                &session_path,
                &serde_json::json!({
                    "expectedRevision": created.revision,
                    "title": "Updated title",
                    "providerId": provider.id
                }),
            ))
            .await
            .expect("router must respond");
        assert_eq!(update_response.status(), StatusCode::OK);
        let updated: AgentSession = response_json(update_response).await;
        assert_eq!(updated.title, "Updated title");

        for (role, text) in [
            (AgentMessageRole::User, "Question"),
            (AgentMessageRole::Assistant, "Answer"),
        ] {
            storage
                .append_agent_message(
                    &updated.id,
                    AppendAgentMessage {
                        role,
                        summary_through_ordinal: None,
                        content_json: serde_json::to_string(&vec![AgentMessageContent::Text {
                            text: text.to_owned(),
                        }])
                        .expect("message content must serialize"),
                    },
                )
                .expect("message must append");
        }

        let first_page_path = format!(
            "/api/v1/agent/sessions/{}/messages?startOrdinal=0&limit=2",
            updated.id
        );
        let first_page_response = application
            .clone()
            .oneshot(dynamic_request(Method::GET, &first_page_path))
            .await
            .expect("router must respond");
        assert_eq!(first_page_response.status(), StatusCode::OK);
        let first_page: AgentMessageList = response_json(first_page_response).await;
        assert_eq!(
            first_page
                .items
                .iter()
                .map(|message| message.ordinal.as_str())
                .collect::<Vec<_>>(),
            ["0", "1"]
        );
        assert!(first_page.has_more);
        assert!(matches!(
            first_page.items[0].content.as_slice(),
            [AgentMessageContent::Text { text }] if text == "Keep answers bounded"
        ));

        let final_page_path = format!(
            "/api/v1/agent/sessions/{}/messages?startOrdinal=2&limit=2",
            updated.id
        );
        let final_page_response = application
            .clone()
            .oneshot(dynamic_request(Method::GET, &final_page_path))
            .await
            .expect("router must respond");
        assert_eq!(final_page_response.status(), StatusCode::OK);
        let final_page: AgentMessageList = response_json(final_page_response).await;
        assert_eq!(final_page.items.len(), 1);
        assert_eq!(final_page.items[0].ordinal, "2");
        assert!(!final_page.has_more);

        let session_json = serde_json::to_value(&updated).expect("session must serialize");
        for field in ["revision", "createdAtMs", "updatedAtMs"] {
            assert!(session_json[field].is_string(), "{field} must be a string");
        }
        let message_json =
            serde_json::to_value(&final_page.items[0]).expect("message must serialize");
        for field in ["ordinal", "createdAtMs"] {
            assert!(message_json[field].is_string(), "{field} must be a string");
        }

        let delete_path = format!(
            "/api/v1/agent/sessions/{}?expectedRevision={}",
            updated.id, updated.revision
        );
        let delete_response = application
            .clone()
            .oneshot(dynamic_request(Method::DELETE, &delete_path))
            .await
            .expect("router must respond");
        assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

        let missing_response = application
            .oneshot(dynamic_request(Method::GET, &session_path))
            .await
            .expect("router must respond");
        assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);
        let error: ApiError = response_json(missing_response).await;
        assert_eq!(error.code, "agent_session_not_found");
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn agent_run_routes_cover_acceptance_snapshot_replay_and_cancellation() {
        let directory = TempDir::new().expect("temp directory");
        let storage =
            Storage::open(directory.path(), Arc::new(TestVault)).expect("test storage must open");
        let application = router(Application::with_storage(storage));

        let provider_response = application
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/v1/agent/providers",
                &serde_json::json!({
                    "name": "Missing credentials",
                    "kind": "open_ai_compatible",
                    "baseUrl": "https://provider.example/v1",
                    "model": "model-1",
                    "contextWindowTokens": "4096",
                    "maxOutputTokens": "1024"
                }),
            ))
            .await
            .expect("router must respond");
        assert_eq!(provider_response.status(), StatusCode::CREATED);
        let provider: ProviderProfile = response_json(provider_response).await;

        let session_response = application
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/v1/agent/sessions",
                &serde_json::json!({
                    "title": "Web run transport",
                    "providerId": provider.id
                }),
            ))
            .await
            .expect("router must respond");
        assert_eq!(session_response.status(), StatusCode::CREATED);
        let session: AgentSession = response_json(session_response).await;

        let start_response = application
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/v1/agent/runs",
                &serde_json::json!({
                    "sessionId": session.id,
                    "message": "Keep this bounded",
                    "sqlPermissionMode": "read_only"
                }),
            ))
            .await
            .expect("router must respond");
        assert_eq!(start_response.status(), StatusCode::ACCEPTED);
        let accepted: AgentRunAccepted = response_json(start_response).await;
        assert_eq!(accepted.session_id, session.id);

        let events_path = format!("/api/v1/agent/runs/{}/events", accepted.run_id);
        let events_response = application
            .clone()
            .oneshot(dynamic_request(Method::GET, &events_path))
            .await
            .expect("router must respond");
        assert_eq!(events_response.status(), StatusCode::OK);
        assert!(
            events_response.headers()[header::CONTENT_TYPE]
                .to_str()
                .expect("content type must be ASCII")
                .starts_with("text/event-stream")
        );
        let events_body = tokio::time::timeout(
            Duration::from_secs(3),
            events_response.into_body().collect(),
        )
        .await
        .expect("terminal agent SSE must close")
        .expect("agent SSE body must collect")
        .to_bytes();
        let events_body = std::str::from_utf8(&events_body).expect("SSE body must be UTF-8");
        assert!(events_body.contains("id: 1"));
        assert!(events_body.contains("event: started"));
        assert!(events_body.contains("event: failed"));
        assert!(events_body.contains(&format!("\"runId\":\"{}\"", accepted.run_id)));

        let snapshot_path = format!("/api/v1/agent/runs/{}", accepted.run_id);
        let snapshot_response = application
            .clone()
            .oneshot(dynamic_request(Method::GET, &snapshot_path))
            .await
            .expect("router must respond");
        assert_eq!(snapshot_response.status(), StatusCode::OK);
        let snapshot: AgentRunSnapshot = response_json(snapshot_response).await;
        assert_eq!(snapshot.status, AgentRunStatus::Failed);
        assert_eq!(snapshot.run_id, accepted.run_id);
        assert!(snapshot.error.is_some());

        let replay_response = application
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(&events_path)
                    .header("last-event-id", "1")
                    .body(Body::empty())
                    .expect("request must build"),
            )
            .await
            .expect("router must respond");
        assert_eq!(replay_response.status(), StatusCode::OK);
        let replay_body = tokio::time::timeout(
            Duration::from_secs(3),
            replay_response.into_body().collect(),
        )
        .await
        .expect("terminal replay must close")
        .expect("replay body must collect")
        .to_bytes();
        let replay_body = std::str::from_utf8(&replay_body).expect("SSE body must be UTF-8");
        assert!(!replay_body.contains("event: started"));
        assert!(replay_body.contains("event: failed"));

        let cancel_path = format!("/api/v1/agent/runs/{}/cancel", accepted.run_id);
        let cancel_response = application
            .oneshot(dynamic_request(Method::POST, &cancel_path))
            .await
            .expect("router must respond");
        assert_eq!(cancel_response.status(), StatusCode::OK);
        let cancelled: CancelAgentRunResponse = response_json(cancel_response).await;
        assert_eq!(cancelled.run_id, accepted.run_id);
        assert_eq!(cancelled.disposition, CancelDisposition::AlreadyTerminal);
    }

    #[tokio::test]
    async fn permission_decision_rejects_a_path_and_body_run_mismatch() {
        let response = router(Application::new())
            .oneshot(json_request(
                Method::POST,
                "/api/v1/agent/runs/path-run/permissions/permission-1/decision",
                &serde_json::json!({
                    "runId": "body-run",
                    "toolCallId": "tool-call-1",
                    "decision": "deny",
                    "argumentsSha256": "00".repeat(32)
                }),
            ))
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error: ApiError = response_json(response).await;
        assert_eq!(error.code, "agent_run_id_mismatch");
    }

    #[tokio::test]
    async fn datasource_routes_cover_the_storage_lifecycle_without_echoing_secrets() {
        let directory = TempDir::new().expect("temp directory");
        let storage =
            Storage::open(directory.path(), Arc::new(TestVault)).expect("test storage must open");
        let application = router(Application::with_storage(storage));

        let create_response = application
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/v1/datasources",
                &serde_json::json!({
                    "name": "Local H2",
                    "driverId": "h2",
                    "connection": {
                        "jdbcUrl": "jdbc:h2:mem:sentinel-url",
                        "properties": [{
                            "key": "password",
                            "value": "sentinel-password",
                            "sensitive": true
                        }],
                        "readOnly": true
                    }
                }),
            ))
            .await
            .expect("router must respond");
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body = create_response
            .into_body()
            .collect()
            .await
            .expect("body must collect")
            .to_bytes();
        let create_json = std::str::from_utf8(&create_body).expect("response must be UTF-8");
        assert!(!create_json.contains("sentinel-url"));
        assert!(!create_json.contains("sentinel-password"));
        assert!(!create_json.contains("jdbcUrl"));
        let created: Datasource =
            serde_json::from_slice(&create_body).expect("create response must match contract");
        assert!(created.has_secret);

        let list_response = application
            .clone()
            .oneshot(request(Method::GET, "/api/v1/datasources"))
            .await
            .expect("router must respond");
        assert_eq!(list_response.status(), StatusCode::OK);
        let listed: DatasourceList = response_json(list_response).await;
        assert_eq!(listed.items, vec![created.clone()]);

        let datasource_path = format!("/api/v1/datasources/{}", created.id);
        let get_response = application
            .clone()
            .oneshot(dynamic_request(Method::GET, &datasource_path))
            .await
            .expect("router must respond");
        assert_eq!(get_response.status(), StatusCode::OK);
        let fetched: Datasource = response_json(get_response).await;
        assert_eq!(fetched, created);

        let update_response = application
            .clone()
            .oneshot(json_request(
                Method::PUT,
                &datasource_path,
                &serde_json::json!({
                    "expectedRevision": fetched.revision,
                    "name": "Renamed H2",
                    "driverId": "h2",
                    "secretChange": { "action": "keep" }
                }),
            ))
            .await
            .expect("router must respond");
        assert_eq!(update_response.status(), StatusCode::OK);
        let updated: Datasource = response_json(update_response).await;
        assert_eq!(updated.name, "Renamed H2");
        assert_ne!(updated.revision, created.revision);

        let delete_path = format!(
            "/api/v1/datasources/{}?expectedRevision={}",
            updated.id, updated.revision
        );
        let delete_response = application
            .clone()
            .oneshot(dynamic_request(Method::DELETE, &delete_path))
            .await
            .expect("router must respond");
        assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

        let missing_response = application
            .oneshot(dynamic_request(Method::GET, &datasource_path))
            .await
            .expect("router must respond");
        assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);
        let error: ApiError = response_json(missing_response).await;
        assert_eq!(error.code, "datasource_not_found");
    }

    #[tokio::test]
    async fn assets_router_serves_vite_files_and_history_without_masking_api_errors() {
        let directory = TempDir::new().expect("temp directory");
        fs::create_dir(directory.path().join("assets")).expect("assets directory must be created");
        fs::write(
            directory.path().join("index.html"),
            "<!doctype html><title>Chat2DB SPA</title>",
        )
        .expect("index fixture must be written");
        fs::write(
            directory.path().join("assets/application.js"),
            "globalThis.chat2dbLoaded = true;",
        )
        .expect("asset fixture must be written");

        let application = router_with_policy_and_assets(
            Application::new(),
            AccessPolicy::default(),
            directory.path(),
        );

        let asset_response = application
            .clone()
            .oneshot(request(Method::GET, "/assets/application.js"))
            .await
            .expect("router must respond");
        assert_eq!(asset_response.status(), StatusCode::OK);
        assert_eq!(
            response_text(asset_response).await,
            "globalThis.chat2dbLoaded = true;"
        );

        for history_path in ["/", "/workspace/query/result-1"] {
            let history_response = application
                .clone()
                .oneshot(request(Method::GET, history_path))
                .await
                .expect("router must respond");
            assert_eq!(history_response.status(), StatusCode::OK);
            assert!(
                response_text(history_response)
                    .await
                    .contains("Chat2DB SPA")
            );
        }

        for api_path in ["/api", "/api/", "/api/v1/missing", "/api/future/route"] {
            let api_response = application
                .clone()
                .oneshot(request(Method::GET, api_path))
                .await
                .expect("router must respond");
            assert_eq!(api_response.status(), StatusCode::NOT_FOUND);
            assert!(
                api_response.headers()[header::CONTENT_TYPE]
                    .to_str()
                    .expect("content type must be ASCII")
                    .starts_with("application/json")
            );
            let error: ApiError = response_json(api_response).await;
            assert_eq!(error.code, "route_not_found");
        }

        let method_response = application
            .oneshot(request(Method::POST, "/api/v1/system/health"))
            .await
            .expect("router must respond");
        assert_eq!(method_response.status(), StatusCode::METHOD_NOT_ALLOWED);
        let error: ApiError = response_json(method_response).await;
        assert_eq!(error.code, "method_not_allowed");
    }

    #[test]
    fn non_loopback_policy_fails_without_a_strong_token() {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 4200);

        assert_eq!(
            AccessPolicy::for_bind(address, None).expect_err("token must be required"),
            AccessPolicyError::MissingToken
        );
        assert_eq!(
            AccessPolicy::for_bind(address, Some("short".to_owned()))
                .expect_err("short token must fail"),
            AccessPolicyError::WeakToken
        );
    }

    #[tokio::test]
    async fn non_loopback_policy_requires_the_configured_bearer_token() {
        let token = "0123456789abcdef0123456789abcdef";
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 4200);
        let policy = AccessPolicy::for_bind(address, Some(token.to_owned()))
            .expect("strong token must build policy");
        let application = router_with_policy(Application::new(), policy);

        let unauthorized = application
            .clone()
            .oneshot(request(Method::GET, "/api/v1/system/health"))
            .await
            .expect("router must respond");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(unauthorized.headers()[header::WWW_AUTHENTICATE], "Bearer");

        let authorized = application
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/system/health")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request must build"),
            )
            .await
            .expect("router must respond");
        assert_eq!(authorized.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn assets_router_preserves_the_configured_bearer_policy() {
        let directory = TempDir::new().expect("temp directory");
        fs::write(directory.path().join("index.html"), "secured SPA")
            .expect("index fixture must be written");
        let token = "0123456789abcdef0123456789abcdef";
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 4200);
        let policy = AccessPolicy::for_bind(address, Some(token.to_owned()))
            .expect("strong token must build policy");
        let application =
            router_with_policy_and_assets(Application::new(), policy, directory.path());

        let unauthorized = application
            .clone()
            .oneshot(request(Method::GET, "/"))
            .await
            .expect("router must respond");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let authorized = application
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request must build"),
            )
            .await
            .expect("router must respond");
        assert_eq!(authorized.status(), StatusCode::OK);
        assert_eq!(response_text(authorized).await, "secured SPA");
    }

    fn request(method: Method, uri: &'static str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request must build")
    }

    fn dynamic_request(method: Method, uri: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request must build")
    }

    fn json_request(method: Method, uri: &str, body: &serde_json::Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&body).expect("request JSON must serialize"),
            ))
            .expect("request must build")
    }

    async fn response_json<T>(response: axum::response::Response) -> T
    where
        T: serde::de::DeserializeOwned,
    {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body must collect")
            .to_bytes();
        serde_json::from_slice(&body).expect("response body must match contract")
    }

    async fn response_text(response: axum::response::Response) -> String {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body must collect")
            .to_bytes();
        String::from_utf8(body.to_vec()).expect("response body must be UTF-8")
    }

    #[derive(Debug)]
    struct TestVault;

    impl SecretVault for TestVault {
        fn probe(&self) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn create(
            &self,
            _reference: &SecretRef,
            _value: &SecretValue,
        ) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn get(&self, _reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
            Ok(None)
        }

        fn delete(&self, _reference: &SecretRef) -> Result<(), SecretVaultError> {
            Ok(())
        }
    }
}
