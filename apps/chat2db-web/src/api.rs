use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    Extension, Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response, sse::Event, sse::KeepAlive, sse::Sse},
};
use chat2db_contract::{
    AgentEvent, AgentEventEnvelope, AgentMessage, AgentMessageContent, AgentMessageList,
    AgentMessageRole, AgentPermissionDecision, AgentPermissionRequest, AgentPermissionResponse,
    AgentPermissionStatus, AgentResultHandle, AgentRunAccepted, AgentRunSnapshot, AgentRunStatus,
    AgentSession, AgentSessionList, AgentStreamMessage, AgentSubscriptionAccepted, AgentToolCall,
    AgentToolOutput, AgentUsage, ApiError, ApiErrorDetails, CancelAgentRunResponse,
    CancelDisposition, CancelOperationResponse, ColumnNullability, ComponentHealth, ComponentState,
    ContextCompactionStrategy, CreateAgentSessionRequest, CreateDatasourceRequest,
    CreateProviderProfileRequest, Datasource, DatasourceConnection, DatasourceConnectionProperty,
    DatasourceList, DatasourceSecretChange, DecideAgentPermissionRequest, HealthResponse,
    JdbcDriver, JdbcDriverList, JdbcValue, JdbcValueType, OperationEvent, OperationEventEnvelope,
    OperationSnapshot, OperationStatus, OperationStreamMessage, OperationSubscriptionAccepted,
    ProductInfo, ProviderCredentials, ProviderKind, ProviderProfile, ProviderProfileList,
    ProviderSecretChange, QueryAccepted, QueryLimits, QueryParameter, ResultColumn, ResultMetadata,
    ResultPage, ResultPageRequest, ResultRow, RuntimeStatus, SqlPermissionMode,
    StartAgentRunRequest, StartQueryRequest, UpdateAgentSessionRequest, UpdateDatasourceRequest,
    UpdateProviderProfileRequest,
};
use chat2db_core::{AppError, Application};
use futures_util::{Stream, stream};
use serde::Deserialize;
use utoipa::{OpenApi, openapi::OpenApi as OpenApiDocument};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    error::{WebError, response as error_response},
    extract::{ApiJson, ApiPath, ApiQuery},
};

const SSE_KEEP_ALIVE_SECONDS: u64 = 15;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Chat2DB API",
        version = env!("CARGO_PKG_VERSION"),
        description = "Transport contract for the Chat2DB Rust Community runtime"
    ),
    tags(
        (name = "system", description = "Runtime identity and readiness"),
        (name = "drivers", description = "Hash-verified JDBC driver inventory"),
        (name = "datasources", description = "Secret-safe datasource lifecycle"),
        (name = "queries", description = "Asynchronous query submission"),
        (name = "operations", description = "Query progress, replay, and cancellation"),
        (name = "results", description = "Bounded retained-result paging"),
        (name = "agents", description = "AI provider profiles, durable sessions, and transcripts")
    ),
    components(schemas(
        AgentMessage,
        AgentMessageContent,
        AgentMessageList,
        AgentMessageRole,
        AgentEvent,
        AgentEventEnvelope,
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
        ApiError,
        ApiErrorDetails,
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
        Datasource,
        DatasourceConnection,
        DatasourceConnectionProperty,
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
        ResultPage,
        ResultPageRequest,
        ResultRow,
        RuntimeStatus,
        SqlPermissionMode,
        StartAgentRunRequest,
        StartQueryRequest,
        UpdateAgentSessionRequest,
        UpdateProviderProfileRequest,
        UpdateDatasourceRequest
    ))
)]
struct ApiDocument;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedRevisionQuery {
    expected_revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentMessagePageQuery {
    start_ordinal: String,
    limit: String,
}

fn documented_router() -> OpenApiRouter<Application> {
    OpenApiRouter::<Application>::with_openapi(ApiDocument::openapi())
        .routes(routes!(health))
        .routes(routes!(info))
        .routes(routes!(list_drivers))
        .routes(routes!(list_datasources, create_datasource))
        .routes(routes!(
            get_datasource,
            update_datasource,
            delete_datasource
        ))
        .routes(routes!(list_provider_profiles, create_provider_profile))
        .routes(routes!(
            get_provider_profile,
            update_provider_profile,
            delete_provider_profile
        ))
        .routes(routes!(list_agent_sessions, create_agent_session))
        .routes(routes!(
            get_agent_session,
            update_agent_session,
            delete_agent_session
        ))
        .routes(routes!(list_agent_messages))
        .routes(routes!(start_agent_run))
        .routes(routes!(agent_run_snapshot))
        .routes(routes!(cancel_agent_run))
        .routes(routes!(decide_agent_permission))
        .routes(routes!(agent_run_events))
        .routes(routes!(start_query))
        .routes(routes!(operation_snapshot))
        .routes(routes!(cancel_operation))
        .routes(routes!(operation_events))
        .routes(routes!(result_page))
        .routes(routes!(openapi_document))
}

pub(crate) fn openapi() -> OpenApiDocument {
    documented_router().into_openapi()
}

pub(crate) fn router(application: Application) -> Router {
    let (router, document) = documented_router().split_for_parts();

    router
        .layer(Extension(Arc::new(document)))
        .with_state(application)
}

#[utoipa::path(
    get,
    path = "/api/v1/system/health",
    tag = "system",
    responses(
        (status = 200, description = "Runtime is available", body = HealthResponse),
        (status = 503, description = "Runtime is unavailable", body = HealthResponse)
    )
)]
async fn health(State(application): State<Application>) -> Response {
    let health = application.health();
    let status = if health.status == RuntimeStatus::Unavailable {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    (status, Json(health)).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/system/info",
    tag = "system",
    responses((status = 200, description = "Product identity", body = ProductInfo))
)]
async fn info(State(application): State<Application>) -> Json<ProductInfo> {
    Json(application.health().product)
}

#[utoipa::path(
    get,
    path = "/api/v1/drivers",
    tag = "drivers",
    responses((status = 200, description = "Loaded JDBC driver inventory", body = JdbcDriverList))
)]
async fn list_drivers(State(application): State<Application>) -> Json<JdbcDriverList> {
    Json(application.list_drivers())
}

#[utoipa::path(
    get,
    path = "/api/v1/datasources",
    tag = "datasources",
    responses(
        (status = 200, description = "Secret-free datasource list", body = DatasourceList),
        (status = 503, description = "Datasource storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected datasource failure", body = ApiError)
    )
)]
async fn list_datasources(
    State(application): State<Application>,
) -> Result<Json<DatasourceList>, WebError> {
    application
        .list_datasources()
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/datasources",
    tag = "datasources",
    request_body = CreateDatasourceRequest,
    responses(
        (status = 201, description = "Datasource created", body = Datasource),
        (status = 400, description = "Invalid datasource request", body = ApiError),
        (status = 409, description = "Datasource conflicts with existing state", body = ApiError),
        (status = 503, description = "Datasource storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected datasource failure", body = ApiError)
    )
)]
async fn create_datasource(
    State(application): State<Application>,
    ApiJson(request): ApiJson<CreateDatasourceRequest>,
) -> Result<(StatusCode, Json<Datasource>), WebError> {
    application
        .create_datasource(request)
        .await
        .map(|datasource| (StatusCode::CREATED, Json(datasource)))
        .map_err(Into::into)
}

#[utoipa::path(
    get,
    path = "/api/v1/datasources/{datasource_id}",
    tag = "datasources",
    params(("datasource_id" = String, Path, description = "Opaque datasource id")),
    responses(
        (status = 200, description = "Secret-free datasource", body = Datasource),
        (status = 404, description = "Datasource does not exist", body = ApiError),
        (status = 503, description = "Datasource storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected datasource failure", body = ApiError)
    )
)]
async fn get_datasource(
    State(application): State<Application>,
    ApiPath(datasource_id): ApiPath<String>,
) -> Result<Json<Datasource>, WebError> {
    application
        .get_datasource(&datasource_id)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    put,
    path = "/api/v1/datasources/{datasource_id}",
    tag = "datasources",
    params(("datasource_id" = String, Path, description = "Opaque datasource id")),
    request_body = UpdateDatasourceRequest,
    responses(
        (status = 200, description = "Datasource updated", body = Datasource),
        (status = 400, description = "Invalid datasource request", body = ApiError),
        (status = 404, description = "Datasource does not exist", body = ApiError),
        (status = 409, description = "Datasource revision conflict", body = ApiError),
        (status = 503, description = "Datasource storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected datasource failure", body = ApiError)
    )
)]
async fn update_datasource(
    State(application): State<Application>,
    ApiPath(datasource_id): ApiPath<String>,
    ApiJson(request): ApiJson<UpdateDatasourceRequest>,
) -> Result<Json<Datasource>, WebError> {
    application
        .update_datasource(&datasource_id, request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    delete,
    path = "/api/v1/datasources/{datasource_id}",
    tag = "datasources",
    params(
        ("datasource_id" = String, Path, description = "Opaque datasource id"),
        ("expectedRevision" = String, Query, description = "Expected monotonic revision")
    ),
    responses(
        (status = 204, description = "Datasource deleted"),
        (status = 400, description = "Invalid expected revision", body = ApiError),
        (status = 404, description = "Datasource does not exist", body = ApiError),
        (status = 409, description = "Datasource revision conflict", body = ApiError),
        (status = 503, description = "Datasource storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected datasource failure", body = ApiError)
    )
)]
async fn delete_datasource(
    State(application): State<Application>,
    ApiPath(datasource_id): ApiPath<String>,
    ApiQuery(query): ApiQuery<ExpectedRevisionQuery>,
) -> Result<StatusCode, WebError> {
    application
        .delete_datasource(&datasource_id, &query.expected_revision)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/agent/providers",
    tag = "agents",
    responses(
        (status = 200, description = "Secret-free provider profile list", body = ProviderProfileList),
        (status = 503, description = "Provider storage or secret vault is unavailable", body = ApiError),
        (status = 500, description = "Unexpected provider failure", body = ApiError)
    )
)]
async fn list_provider_profiles(
    State(application): State<Application>,
) -> Result<Json<ProviderProfileList>, WebError> {
    application
        .list_provider_profiles()
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/agent/providers",
    tag = "agents",
    request_body = CreateProviderProfileRequest,
    responses(
        (status = 201, description = "Provider profile created", body = ProviderProfile),
        (status = 400, description = "Invalid provider request", body = ApiError),
        (status = 409, description = "Provider conflicts with existing state", body = ApiError),
        (status = 503, description = "Provider storage or secret vault is unavailable", body = ApiError),
        (status = 500, description = "Unexpected provider failure", body = ApiError)
    )
)]
async fn create_provider_profile(
    State(application): State<Application>,
    ApiJson(request): ApiJson<CreateProviderProfileRequest>,
) -> Result<(StatusCode, Json<ProviderProfile>), WebError> {
    application
        .create_provider_profile(request)
        .await
        .map(|profile| (StatusCode::CREATED, Json(profile)))
        .map_err(Into::into)
}

#[utoipa::path(
    get,
    path = "/api/v1/agent/providers/{provider_id}",
    tag = "agents",
    params(("provider_id" = String, Path, description = "Opaque provider profile id")),
    responses(
        (status = 200, description = "Secret-free provider profile", body = ProviderProfile),
        (status = 404, description = "Provider profile does not exist", body = ApiError),
        (status = 503, description = "Provider storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected provider failure", body = ApiError)
    )
)]
async fn get_provider_profile(
    State(application): State<Application>,
    ApiPath(provider_id): ApiPath<String>,
) -> Result<Json<ProviderProfile>, WebError> {
    application
        .get_provider_profile(&provider_id)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    put,
    path = "/api/v1/agent/providers/{provider_id}",
    tag = "agents",
    params(("provider_id" = String, Path, description = "Opaque provider profile id")),
    request_body = UpdateProviderProfileRequest,
    responses(
        (status = 200, description = "Provider profile updated", body = ProviderProfile),
        (status = 400, description = "Invalid provider request", body = ApiError),
        (status = 404, description = "Provider profile does not exist", body = ApiError),
        (status = 409, description = "Provider revision or dependency conflict", body = ApiError),
        (status = 503, description = "Provider storage or secret vault is unavailable", body = ApiError),
        (status = 500, description = "Unexpected provider failure", body = ApiError)
    )
)]
async fn update_provider_profile(
    State(application): State<Application>,
    ApiPath(provider_id): ApiPath<String>,
    ApiJson(request): ApiJson<UpdateProviderProfileRequest>,
) -> Result<Json<ProviderProfile>, WebError> {
    application
        .update_provider_profile(&provider_id, request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    delete,
    path = "/api/v1/agent/providers/{provider_id}",
    tag = "agents",
    params(
        ("provider_id" = String, Path, description = "Opaque provider profile id"),
        ("expectedRevision" = String, Query, description = "Expected monotonic revision")
    ),
    responses(
        (status = 204, description = "Provider profile deleted"),
        (status = 400, description = "Invalid expected revision", body = ApiError),
        (status = 404, description = "Provider profile does not exist", body = ApiError),
        (status = 409, description = "Provider revision or dependency conflict", body = ApiError),
        (status = 503, description = "Provider storage or secret vault is unavailable", body = ApiError),
        (status = 500, description = "Unexpected provider failure", body = ApiError)
    )
)]
async fn delete_provider_profile(
    State(application): State<Application>,
    ApiPath(provider_id): ApiPath<String>,
    ApiQuery(query): ApiQuery<ExpectedRevisionQuery>,
) -> Result<StatusCode, WebError> {
    application
        .delete_provider_profile(&provider_id, &query.expected_revision)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/agent/sessions",
    tag = "agents",
    responses(
        (status = 200, description = "Durable agent session list", body = AgentSessionList),
        (status = 503, description = "Agent storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected agent-session failure", body = ApiError)
    )
)]
async fn list_agent_sessions(
    State(application): State<Application>,
) -> Result<Json<AgentSessionList>, WebError> {
    application
        .list_agent_sessions()
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/agent/sessions",
    tag = "agents",
    request_body = CreateAgentSessionRequest,
    responses(
        (status = 201, description = "Agent session created", body = AgentSession),
        (status = 400, description = "Invalid agent-session request", body = ApiError),
        (status = 404, description = "Selected provider or datasource does not exist", body = ApiError),
        (status = 507, description = "Agent message resource limits are exhausted", body = ApiError),
        (status = 503, description = "Agent storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected agent-session failure", body = ApiError)
    )
)]
async fn create_agent_session(
    State(application): State<Application>,
    ApiJson(request): ApiJson<CreateAgentSessionRequest>,
) -> Result<(StatusCode, Json<AgentSession>), WebError> {
    application
        .create_agent_session(request)
        .await
        .map(|session| (StatusCode::CREATED, Json(session)))
        .map_err(Into::into)
}

#[utoipa::path(
    get,
    path = "/api/v1/agent/sessions/{session_id}",
    tag = "agents",
    params(("session_id" = String, Path, description = "Opaque agent session id")),
    responses(
        (status = 200, description = "Durable agent session", body = AgentSession),
        (status = 404, description = "Agent session does not exist", body = ApiError),
        (status = 503, description = "Agent storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected agent-session failure", body = ApiError)
    )
)]
async fn get_agent_session(
    State(application): State<Application>,
    ApiPath(session_id): ApiPath<String>,
) -> Result<Json<AgentSession>, WebError> {
    application
        .get_agent_session(&session_id)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    put,
    path = "/api/v1/agent/sessions/{session_id}",
    tag = "agents",
    params(("session_id" = String, Path, description = "Opaque agent session id")),
    request_body = UpdateAgentSessionRequest,
    responses(
        (status = 200, description = "Agent session updated", body = AgentSession),
        (status = 400, description = "Invalid agent-session request", body = ApiError),
        (status = 404, description = "Agent session, provider, or datasource does not exist", body = ApiError),
        (status = 409, description = "Agent-session revision or active-run conflict", body = ApiError),
        (status = 503, description = "Agent storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected agent-session failure", body = ApiError)
    )
)]
async fn update_agent_session(
    State(application): State<Application>,
    ApiPath(session_id): ApiPath<String>,
    ApiJson(request): ApiJson<UpdateAgentSessionRequest>,
) -> Result<Json<AgentSession>, WebError> {
    application
        .update_agent_session(&session_id, request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    delete,
    path = "/api/v1/agent/sessions/{session_id}",
    tag = "agents",
    params(
        ("session_id" = String, Path, description = "Opaque agent session id"),
        ("expectedRevision" = String, Query, description = "Expected monotonic revision")
    ),
    responses(
        (status = 204, description = "Agent session and owned state deleted"),
        (status = 400, description = "Invalid expected revision", body = ApiError),
        (status = 404, description = "Agent session does not exist", body = ApiError),
        (status = 409, description = "Agent-session revision or active-run conflict", body = ApiError),
        (status = 503, description = "Agent storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected agent-session failure", body = ApiError)
    )
)]
async fn delete_agent_session(
    State(application): State<Application>,
    ApiPath(session_id): ApiPath<String>,
    ApiQuery(query): ApiQuery<ExpectedRevisionQuery>,
) -> Result<StatusCode, WebError> {
    application
        .delete_agent_session(&session_id, &query.expected_revision)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/agent/sessions/{session_id}/messages",
    tag = "agents",
    params(
        ("session_id" = String, Path, description = "Opaque agent session id"),
        ("startOrdinal" = String, Query, description = "Inclusive first message ordinal"),
        ("limit" = String, Query, description = "Maximum number of messages from 1 through 512")
    ),
    responses(
        (status = 200, description = "Bounded forward page of canonical messages", body = AgentMessageList),
        (status = 400, description = "Invalid message-page bounds", body = ApiError),
        (status = 404, description = "Agent session does not exist", body = ApiError),
        (status = 503, description = "Agent storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected agent-message failure", body = ApiError)
    )
)]
async fn list_agent_messages(
    State(application): State<Application>,
    ApiPath(session_id): ApiPath<String>,
    ApiQuery(query): ApiQuery<AgentMessagePageQuery>,
) -> Result<Json<AgentMessageList>, WebError> {
    application
        .list_agent_messages(&session_id, &query.start_ordinal, &query.limit)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/agent/runs",
    tag = "agents",
    request_body = StartAgentRunRequest,
    responses(
        (status = 202, description = "Agent run accepted", body = AgentRunAccepted),
        (status = 400, description = "Invalid agent-run request", body = ApiError),
        (status = 404, description = "Agent session, provider, or datasource does not exist", body = ApiError),
        (status = 409, description = "Agent session already has an active run", body = ApiError),
        (status = 507, description = "Agent run resource limits are exhausted", body = ApiError),
        (status = 503, description = "Agent runtime or storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected agent-run failure", body = ApiError)
    )
)]
async fn start_agent_run(
    State(application): State<Application>,
    ApiJson(request): ApiJson<StartAgentRunRequest>,
) -> Result<(StatusCode, Json<AgentRunAccepted>), WebError> {
    application
        .start_agent_run(request)
        .await
        .map(|accepted| (StatusCode::ACCEPTED, Json(accepted)))
        .map_err(Into::into)
}

#[utoipa::path(
    get,
    path = "/api/v1/agent/runs/{run_id}",
    tag = "agents",
    params(("run_id" = String, Path, description = "Opaque agent run id")),
    responses(
        (status = 200, description = "Current agent-run snapshot", body = AgentRunSnapshot),
        (status = 404, description = "Agent run does not exist", body = ApiError),
        (status = 503, description = "Agent storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected agent-run failure", body = ApiError)
    )
)]
async fn agent_run_snapshot(
    State(application): State<Application>,
    ApiPath(run_id): ApiPath<String>,
) -> Result<Json<AgentRunSnapshot>, WebError> {
    application
        .agent_run_snapshot(&run_id)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/agent/runs/{run_id}/cancel",
    tag = "agents",
    params(("run_id" = String, Path, description = "Opaque agent run id")),
    responses(
        (status = 200, description = "Idempotent agent-run cancellation disposition", body = CancelAgentRunResponse),
        (status = 503, description = "Agent runtime or storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected agent-run cancellation failure", body = ApiError)
    )
)]
async fn cancel_agent_run(
    State(application): State<Application>,
    ApiPath(run_id): ApiPath<String>,
) -> Result<Json<CancelAgentRunResponse>, WebError> {
    application
        .cancel_agent_run(&run_id)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/agent/runs/{run_id}/permissions/{permission_id}/decision",
    tag = "agents",
    params(
        ("run_id" = String, Path, description = "Opaque agent run id"),
        ("permission_id" = String, Path, description = "Opaque pending permission id")
    ),
    request_body = DecideAgentPermissionRequest,
    responses(
        (status = 200, description = "Permission decision recorded", body = AgentPermissionResponse),
        (status = 400, description = "Invalid or mismatched permission decision", body = ApiError),
        (status = 404, description = "Agent run or permission does not exist", body = ApiError),
        (status = 409, description = "Permission is stale or no longer executable", body = ApiError),
        (status = 503, description = "Agent runtime or storage is unavailable", body = ApiError),
        (status = 500, description = "Unexpected permission-decision failure", body = ApiError)
    )
)]
async fn decide_agent_permission(
    State(application): State<Application>,
    ApiPath((run_id, permission_id)): ApiPath<(String, String)>,
    ApiJson(request): ApiJson<DecideAgentPermissionRequest>,
) -> Result<Json<AgentPermissionResponse>, WebError> {
    if request.run_id != run_id {
        return Err(WebError::bad_request(
            "agent_run_id_mismatch",
            "The permission request runId must match the route run id",
        ));
    }
    application
        .decide_agent_permission(&permission_id, request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    get,
    path = "/api/v1/agent/runs/{run_id}/events",
    tag = "agents",
    params(
        ("run_id" = String, Path, description = "Opaque agent run id"),
        ("Last-Event-ID" = Option<String>, Header, description = "Last consumed numeric event sequence")
    ),
    responses(
        (status = 200, description = "Replay followed by live agent-run events", body = String, content_type = "text/event-stream"),
        (status = 400, description = "Invalid Last-Event-ID", body = ApiError),
        (status = 404, description = "Agent run does not exist", body = ApiError),
        (status = 409, description = "Requested event fell outside the replay window", body = ApiError),
        (status = 503, description = "Agent event stream is unavailable", body = ApiError),
        (status = 500, description = "Unexpected subscription failure", body = ApiError)
    )
)]
async fn agent_run_events(
    State(application): State<Application>,
    ApiPath(run_id): ApiPath<String>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, WebError> {
    let last_event_id = parse_last_event_id(&headers)?;
    let subscription = application
        .subscribe_agent_run(&run_id, last_event_id)
        .await?;
    let events = stream::unfold(
        (subscription, false),
        |(mut subscription, finished)| async move {
            if finished {
                return None;
            }

            match subscription.next_event().await {
                Ok(Some(envelope)) => Some((Ok(agent_sse_event(envelope)), (subscription, false))),
                Ok(None) => None,
                Err(error) => Some((Ok(error_sse_event(&error)), (subscription, true))),
            }
        },
    );

    Ok(Sse::new(events).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(SSE_KEEP_ALIVE_SECONDS))
            .text("keep-alive"),
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/queries",
    tag = "queries",
    request_body = StartQueryRequest,
    responses(
        (status = 202, description = "Query operation accepted", body = QueryAccepted),
        (status = 400, description = "Invalid query request", body = ApiError),
        (status = 404, description = "Datasource does not exist", body = ApiError),
        (status = 507, description = "Query resource limits are exhausted", body = ApiError),
        (status = 503, description = "Query engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected query failure", body = ApiError)
    )
)]
async fn start_query(
    State(application): State<Application>,
    ApiJson(request): ApiJson<StartQueryRequest>,
) -> Result<(StatusCode, Json<QueryAccepted>), WebError> {
    application
        .start_query(request)
        .await
        .map(|accepted| (StatusCode::ACCEPTED, Json(accepted)))
        .map_err(Into::into)
}

#[utoipa::path(
    get,
    path = "/api/v1/operations/{operation_id}",
    tag = "operations",
    params(("operation_id" = String, Path, description = "Opaque operation id")),
    responses(
        (status = 200, description = "Current operation snapshot", body = OperationSnapshot),
        (status = 404, description = "Operation does not exist", body = ApiError),
        (status = 500, description = "Unexpected operation failure", body = ApiError)
    )
)]
async fn operation_snapshot(
    State(application): State<Application>,
    ApiPath(operation_id): ApiPath<String>,
) -> Result<Json<OperationSnapshot>, WebError> {
    application
        .operation_snapshot(&operation_id)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/operations/{operation_id}/cancel",
    tag = "operations",
    params(("operation_id" = String, Path, description = "Opaque operation id")),
    responses(
        (status = 200, description = "Idempotent cancellation disposition", body = CancelOperationResponse)
    )
)]
async fn cancel_operation(
    State(application): State<Application>,
    ApiPath(operation_id): ApiPath<String>,
) -> Json<CancelOperationResponse> {
    Json(application.cancel_operation(&operation_id).await)
}

#[utoipa::path(
    get,
    path = "/api/v1/operations/{operation_id}/events",
    tag = "operations",
    params(
        ("operation_id" = String, Path, description = "Opaque operation id"),
        ("Last-Event-ID" = Option<String>, Header, description = "Last consumed numeric event sequence")
    ),
    responses(
        (status = 200, description = "Replay followed by live operation events", body = String, content_type = "text/event-stream"),
        (status = 400, description = "Invalid Last-Event-ID", body = ApiError),
        (status = 404, description = "Operation does not exist", body = ApiError),
        (status = 409, description = "Requested event fell outside the replay window", body = ApiError),
        (status = 500, description = "Unexpected subscription failure", body = ApiError)
    )
)]
async fn operation_events(
    State(application): State<Application>,
    ApiPath(operation_id): ApiPath<String>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, WebError> {
    let last_event_id = parse_last_event_id(&headers)?;
    let subscription = application
        .subscribe_operation(&operation_id, last_event_id)
        .await?;
    let events = stream::unfold(
        (subscription, false),
        |(mut subscription, finished)| async move {
            if finished {
                return None;
            }

            match subscription.next_event().await {
                Ok(Some(envelope)) => {
                    Some((Ok(operation_sse_event(envelope)), (subscription, false)))
                }
                Ok(None) => None,
                Err(error) => Some((Ok(error_sse_event(&error)), (subscription, true))),
            }
        },
    );

    Ok(Sse::new(events).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(SSE_KEEP_ALIVE_SECONDS))
            .text("keep-alive"),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/results/{result_id}",
    tag = "results",
    params(
        ("result_id" = String, Path, description = "Opaque retained-result id"),
        ("offset" = String, Query, description = "Zero-based row offset"),
        ("maxRows" = String, Query, description = "Maximum returned rows"),
        ("maxBytes" = String, Query, description = "Maximum returned encoded bytes")
    ),
    responses(
        (status = 200, description = "Bounded retained-result page", body = ResultPage),
        (status = 400, description = "Invalid page bounds", body = ApiError),
        (status = 404, description = "Retained result does not exist", body = ApiError),
        (status = 507, description = "Page resource limits are exhausted", body = ApiError),
        (status = 500, description = "Unexpected result failure", body = ApiError)
    )
)]
async fn result_page(
    State(application): State<Application>,
    ApiPath(result_id): ApiPath<String>,
    ApiQuery(request): ApiQuery<ResultPageRequest>,
) -> Result<Json<ResultPage>, WebError> {
    application
        .result_page(&result_id, request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    get,
    path = "/api/v1/openapi.json",
    tag = "system",
    responses((status = 200, description = "OpenAPI document for the registered HTTP handlers"))
)]
async fn openapi_document(
    Extension(document): Extension<Arc<OpenApiDocument>>,
) -> Json<OpenApiDocument> {
    Json((*document).clone())
}

fn parse_last_event_id(headers: &HeaderMap) -> Result<Option<u64>, WebError> {
    let Some(value) = headers.get("last-event-id") else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|_| {
        WebError::bad_request(
            "invalid_last_event_id",
            "Last-Event-ID must be an unsigned decimal integer",
        )
    })?;
    let sequence = value.parse::<u64>().map_err(|_| {
        WebError::bad_request(
            "invalid_last_event_id",
            "Last-Event-ID must be an unsigned decimal integer",
        )
    })?;
    Ok(Some(sequence))
}

fn operation_sse_event(envelope: OperationEventEnvelope) -> Event {
    let event_name = match &envelope.event {
        OperationEvent::Started => "started",
        OperationEvent::Progress { .. } => "progress",
        OperationEvent::Completed { .. } => "completed",
        OperationEvent::Failed { .. } => "failed",
        OperationEvent::Cancelled { .. } => "cancelled",
    };
    let sequence = envelope.sequence.clone();
    Event::default()
        .event(event_name)
        .id(sequence)
        .json_data(envelope)
        .unwrap_or_else(|error| {
            tracing::error!(%error, "failed to serialize operation SSE event");
            serialization_error_event()
        })
}

fn agent_sse_event(envelope: AgentEventEnvelope) -> Event {
    let event_name = match &envelope.event {
        AgentEvent::Started => "started",
        AgentEvent::TextDelta { .. } => "text_delta",
        AgentEvent::ToolStarted { .. } => "tool_started",
        AgentEvent::ToolCompleted { .. } => "tool_completed",
        AgentEvent::ToolFailed { .. } => "tool_failed",
        AgentEvent::PermissionRequested { .. } => "permission_requested",
        AgentEvent::PermissionResolved { .. } => "permission_resolved",
        AgentEvent::ContextCompacted { .. } => "context_compacted",
        AgentEvent::Usage { .. } => "usage",
        AgentEvent::Completed { .. } => "completed",
        AgentEvent::Failed { .. } => "failed",
        AgentEvent::Cancelled { .. } => "cancelled",
    };
    let sequence = envelope.sequence.clone();
    Event::default()
        .event(event_name)
        .id(sequence)
        .json_data(envelope)
        .unwrap_or_else(|error| {
            tracing::error!(%error, "failed to serialize agent SSE event");
            serialization_error_event()
        })
}

fn error_sse_event(error: &AppError) -> Event {
    let error = error.api_error();
    Event::default()
        .event("error")
        .json_data(error)
        .unwrap_or_else(|serialization_error| {
            tracing::error!(%serialization_error, "failed to serialize SSE error event");
            serialization_error_event()
        })
}

fn serialization_error_event() -> Event {
    Event::default().event("error").data(
        r#"{"code":"serialization_error","message":"Unable to encode stream event","retryable":false}"#,
    )
}

pub(crate) async fn not_found() -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        "route_not_found",
        "The requested route does not exist",
    )
}

pub(crate) async fn method_not_allowed() -> Response {
    error_response(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "The requested method is not allowed for this route",
    )
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use axum::{
        http::{HeaderMap, HeaderValue, header},
        response::{IntoResponse, sse::Sse},
    };
    use chat2db_contract::{
        AgentEvent, AgentEventEnvelope, OperationEvent, OperationEventEnvelope,
    };
    use futures_util::stream;
    use http_body_util::BodyExt;

    use super::{agent_sse_event, operation_sse_event, parse_last_event_id};

    #[test]
    fn last_event_id_is_parsed_as_the_shared_numeric_sequence() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "last-event-id",
            HeaderValue::from_static("9007199254740993"),
        );

        assert_eq!(
            parse_last_event_id(&headers).expect("header must parse"),
            Some(9_007_199_254_740_993)
        );
    }

    #[tokio::test]
    async fn operation_event_is_encoded_with_sse_id_name_and_json_data() {
        let event = operation_sse_event(OperationEventEnvelope {
            operation_id: "operation-1".to_owned(),
            sequence: "9007199254740993".to_owned(),
            occurred_at_ms: "1784900000000".to_owned(),
            event: OperationEvent::Started,
        });
        let response = Sse::new(stream::iter([Ok::<_, Infallible>(event)])).into_response();

        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        let body = response
            .into_body()
            .collect()
            .await
            .expect("SSE body must collect")
            .to_bytes();
        let body = std::str::from_utf8(&body).expect("SSE body must be UTF-8");
        assert!(body.contains("id: 9007199254740993"));
        assert!(body.contains("event: started"));
        assert!(body.contains("\"operationId\":\"operation-1\""));
        assert!(body.contains("\"sequence\":\"9007199254740993\""));
    }

    #[tokio::test]
    async fn agent_event_is_encoded_with_sse_id_name_and_json_data() {
        let event = agent_sse_event(AgentEventEnvelope {
            run_id: "run-1".to_owned(),
            sequence: "9007199254740993".to_owned(),
            occurred_at_ms: "1784900000000".to_owned(),
            event: AgentEvent::TextDelta {
                delta: "bounded".to_owned(),
            },
        });
        let response = Sse::new(stream::iter([Ok::<_, Infallible>(event)])).into_response();

        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        let body = response
            .into_body()
            .collect()
            .await
            .expect("SSE body must collect")
            .to_bytes();
        let body = std::str::from_utf8(&body).expect("SSE body must be UTF-8");
        assert!(body.contains("id: 9007199254740993"));
        assert!(body.contains("event: text_delta"));
        assert!(body.contains("\"runId\":\"run-1\""));
        assert!(body.contains("\"delta\":\"bounded\""));
    }
}
