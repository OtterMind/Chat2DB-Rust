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
    AgentToolOutput, AgentUsage, ApiError, ApiErrorDetails, BuildCommunityCreateSchemaRequest,
    CancelAgentRunResponse, CancelDisposition, CancelOperationResponse, ColumnNullability,
    CommunityBuiltSql, CommunityDatabase, CommunityDatabaseList, CommunityDriverConfig,
    CommunityForeignKey, CommunityForeignKeyList, CommunityFunction, CommunityFunctionList,
    CommunityFunctionParameter, CommunityFunctionParameterList, CommunityParsedStatement,
    CommunityPlugin, CommunityPluginBehavior, CommunityPluginCatalog, CommunityPluginServices,
    CommunityPrimaryKey, CommunityPrimaryKeyList, CommunityProcedure, CommunityProcedureList,
    CommunityProcedureParameter, CommunityProcedureParameterList, CommunitySchema,
    CommunitySchemaList, CommunitySqlAnalysis, CommunityTable, CommunityTableColumn,
    CommunityTableColumnList, CommunityTableIndex, CommunityTableIndexColumn,
    CommunityTableIndexList, CommunityTableList, CommunityTrigger, CommunityTriggerList,
    CommunityViewList, ComponentHealth, ComponentState, ContextCompactionStrategy,
    CreateAgentSessionRequest, CreateDatasourceRequest, CreateProviderProfileRequest, Datasource,
    DatasourceConnection, DatasourceConnectionProperty, DatasourceList, DatasourceSecretChange,
    DecideAgentPermissionRequest, GetCommunityFunctionRequest, GetCommunityProcedureRequest,
    GetCommunityTriggerRequest, HealthResponse, JdbcDriver, JdbcDriverList, JdbcValue,
    JdbcValueType, ListCommunityColumnsRequest, ListCommunityDatabasesRequest,
    ListCommunityFunctionsRequest, ListCommunityIndexesRequest, ListCommunityProceduresRequest,
    ListCommunitySchemasRequest, ListCommunityTableKeysRequest, ListCommunityTablesRequest,
    ListCommunityTriggersRequest, ListCommunityViewsRequest, OperationEvent,
    OperationEventEnvelope, OperationSnapshot, OperationStatus, OperationStreamMessage,
    OperationSubscriptionAccepted, ParseCommunitySqlRequest, ProductInfo, ProviderCredentials,
    ProviderKind, ProviderProfile, ProviderProfileList, ProviderSecretChange, QueryAccepted,
    QueryLimits, QueryParameter, ResultColumn, ResultMetadata, ResultPage, ResultPageRequest,
    ResultRow, RuntimeStatus, SqlPermissionMode, StartAgentRunRequest, StartQueryRequest,
    UpdateAgentSessionRequest, UpdateDatasourceRequest, UpdateProviderProfileRequest,
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
        (name = "community", description = "Community compatibility plugin, metadata, and SQL services"),
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
        BuildCommunityCreateSchemaRequest,
        CommunityBuiltSql,
        CommunityDatabase,
        CommunityDatabaseList,
        CommunityDriverConfig,
        CommunityForeignKey,
        CommunityForeignKeyList,
        CommunityFunction,
        CommunityFunctionList,
        CommunityFunctionParameter,
        CommunityFunctionParameterList,
        CommunityParsedStatement,
        CommunityPlugin,
        CommunityPluginBehavior,
        CommunityPluginCatalog,
        CommunityPluginServices,
        CommunityPrimaryKey,
        CommunityPrimaryKeyList,
        CommunityProcedure,
        CommunityProcedureList,
        CommunityProcedureParameter,
        CommunityProcedureParameterList,
        CommunitySchema,
        CommunitySchemaList,
        CommunitySqlAnalysis,
        CommunityTable,
        CommunityTableColumn,
        CommunityTableColumnList,
        CommunityTableIndex,
        CommunityTableIndexColumn,
        CommunityTableIndexList,
        CommunityTableList,
        CommunityTrigger,
        CommunityTriggerList,
        CommunityViewList,
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
        GetCommunityFunctionRequest,
        GetCommunityProcedureRequest,
        GetCommunityTriggerRequest,
        JdbcDriver,
        JdbcDriverList,
        JdbcValue,
        JdbcValueType,
        ListCommunityColumnsRequest,
        ListCommunityDatabasesRequest,
        ListCommunityFunctionsRequest,
        ListCommunityIndexesRequest,
        ListCommunityProceduresRequest,
        ListCommunitySchemasRequest,
        ListCommunityTableKeysRequest,
        ListCommunityTablesRequest,
        ListCommunityTriggersRequest,
        ListCommunityViewsRequest,
        OperationEvent,
        OperationEventEnvelope,
        OperationSnapshot,
        OperationStatus,
        OperationStreamMessage,
        OperationSubscriptionAccepted,
        ParseCommunitySqlRequest,
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
        .routes(routes!(list_community_plugins))
        .routes(routes!(list_community_schemas))
        .routes(routes!(list_community_databases))
        .routes(routes!(list_community_tables))
        .routes(routes!(list_community_columns))
        .routes(routes!(list_community_indexes))
        .routes(routes!(list_community_views))
        .routes(routes!(list_community_imported_keys))
        .routes(routes!(list_community_exported_keys))
        .routes(routes!(list_community_primary_keys))
        .routes(routes!(list_community_functions))
        .routes(routes!(get_community_function))
        .routes(routes!(list_community_function_parameters))
        .routes(routes!(list_community_procedures))
        .routes(routes!(get_community_procedure))
        .routes(routes!(list_community_procedure_parameters))
        .routes(routes!(list_community_triggers))
        .routes(routes!(get_community_trigger))
        .routes(routes!(build_community_create_schema))
        .routes(routes!(parse_community_sql))
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
    path = "/api/v1/community/plugins",
    tag = "community",
    responses(
        (status = 200, description = "Community plugin catalog", body = CommunityPluginCatalog),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community plugin failure", body = ApiError)
    )
)]
async fn list_community_plugins(
    State(application): State<Application>,
) -> Result<Json<CommunityPluginCatalog>, WebError> {
    application
        .list_community_plugins()
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/schemas",
    tag = "community",
    request_body = ListCommunitySchemasRequest,
    responses(
        (status = 200, description = "Community schema metadata", body = CommunitySchemaList),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn list_community_schemas(
    State(application): State<Application>,
    ApiJson(request): ApiJson<ListCommunitySchemasRequest>,
) -> Result<Json<CommunitySchemaList>, WebError> {
    application
        .list_community_schemas(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/databases",
    tag = "community",
    request_body = ListCommunityDatabasesRequest,
    responses(
        (status = 200, description = "Community database metadata", body = CommunityDatabaseList),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn list_community_databases(
    State(application): State<Application>,
    ApiJson(request): ApiJson<ListCommunityDatabasesRequest>,
) -> Result<Json<CommunityDatabaseList>, WebError> {
    application
        .list_community_databases(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/tables",
    tag = "community",
    request_body = ListCommunityTablesRequest,
    responses(
        (status = 200, description = "Community table metadata", body = CommunityTableList),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn list_community_tables(
    State(application): State<Application>,
    ApiJson(request): ApiJson<ListCommunityTablesRequest>,
) -> Result<Json<CommunityTableList>, WebError> {
    application
        .list_community_tables(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/columns",
    tag = "community",
    request_body = ListCommunityColumnsRequest,
    responses(
        (status = 200, description = "Community column metadata", body = CommunityTableColumnList),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn list_community_columns(
    State(application): State<Application>,
    ApiJson(request): ApiJson<ListCommunityColumnsRequest>,
) -> Result<Json<CommunityTableColumnList>, WebError> {
    application
        .list_community_columns(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/indexes",
    tag = "community",
    request_body = ListCommunityIndexesRequest,
    responses(
        (status = 200, description = "Community index metadata", body = CommunityTableIndexList),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn list_community_indexes(
    State(application): State<Application>,
    ApiJson(request): ApiJson<ListCommunityIndexesRequest>,
) -> Result<Json<CommunityTableIndexList>, WebError> {
    application
        .list_community_indexes(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/views",
    tag = "community",
    request_body = ListCommunityViewsRequest,
    responses(
        (status = 200, description = "Community view metadata", body = CommunityViewList),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn list_community_views(
    State(application): State<Application>,
    ApiJson(request): ApiJson<ListCommunityViewsRequest>,
) -> Result<Json<CommunityViewList>, WebError> {
    application
        .list_community_views(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/imported-keys",
    tag = "community",
    request_body = ListCommunityTableKeysRequest,
    responses(
        (status = 200, description = "Community imported foreign-key metadata", body = CommunityForeignKeyList),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn list_community_imported_keys(
    State(application): State<Application>,
    ApiJson(request): ApiJson<ListCommunityTableKeysRequest>,
) -> Result<Json<CommunityForeignKeyList>, WebError> {
    application
        .list_community_imported_keys(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/exported-keys",
    tag = "community",
    request_body = ListCommunityTableKeysRequest,
    responses(
        (status = 200, description = "Community exported foreign-key metadata", body = CommunityForeignKeyList),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn list_community_exported_keys(
    State(application): State<Application>,
    ApiJson(request): ApiJson<ListCommunityTableKeysRequest>,
) -> Result<Json<CommunityForeignKeyList>, WebError> {
    application
        .list_community_exported_keys(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/primary-keys",
    tag = "community",
    request_body = ListCommunityTableKeysRequest,
    responses(
        (status = 200, description = "Community primary-key metadata", body = CommunityPrimaryKeyList),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn list_community_primary_keys(
    State(application): State<Application>,
    ApiJson(request): ApiJson<ListCommunityTableKeysRequest>,
) -> Result<Json<CommunityPrimaryKeyList>, WebError> {
    application
        .list_community_primary_keys(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/functions",
    tag = "community",
    request_body = ListCommunityFunctionsRequest,
    responses(
        (status = 200, description = "Community function metadata", body = CommunityFunctionList),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn list_community_functions(
    State(application): State<Application>,
    ApiJson(request): ApiJson<ListCommunityFunctionsRequest>,
) -> Result<Json<CommunityFunctionList>, WebError> {
    application
        .list_community_functions(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/function",
    tag = "community",
    request_body = GetCommunityFunctionRequest,
    responses(
        (status = 200, description = "Community function detail", body = CommunityFunction),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn get_community_function(
    State(application): State<Application>,
    ApiJson(request): ApiJson<GetCommunityFunctionRequest>,
) -> Result<Json<CommunityFunction>, WebError> {
    application
        .get_community_function(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/function-parameters",
    tag = "community",
    request_body = GetCommunityFunctionRequest,
    responses(
        (status = 200, description = "Community function parameter metadata", body = CommunityFunctionParameterList),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn list_community_function_parameters(
    State(application): State<Application>,
    ApiJson(request): ApiJson<GetCommunityFunctionRequest>,
) -> Result<Json<CommunityFunctionParameterList>, WebError> {
    application
        .list_community_function_parameters(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/procedures",
    tag = "community",
    request_body = ListCommunityProceduresRequest,
    responses(
        (status = 200, description = "Community procedure metadata", body = CommunityProcedureList),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn list_community_procedures(
    State(application): State<Application>,
    ApiJson(request): ApiJson<ListCommunityProceduresRequest>,
) -> Result<Json<CommunityProcedureList>, WebError> {
    application
        .list_community_procedures(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/procedure",
    tag = "community",
    request_body = GetCommunityProcedureRequest,
    responses(
        (status = 200, description = "Community procedure detail", body = CommunityProcedure),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn get_community_procedure(
    State(application): State<Application>,
    ApiJson(request): ApiJson<GetCommunityProcedureRequest>,
) -> Result<Json<CommunityProcedure>, WebError> {
    application
        .get_community_procedure(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/procedure-parameters",
    tag = "community",
    request_body = GetCommunityProcedureRequest,
    responses(
        (status = 200, description = "Community procedure parameter metadata", body = CommunityProcedureParameterList),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn list_community_procedure_parameters(
    State(application): State<Application>,
    ApiJson(request): ApiJson<GetCommunityProcedureRequest>,
) -> Result<Json<CommunityProcedureParameterList>, WebError> {
    application
        .list_community_procedure_parameters(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/triggers",
    tag = "community",
    request_body = ListCommunityTriggersRequest,
    responses(
        (status = 200, description = "Community trigger metadata", body = CommunityTriggerList),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn list_community_triggers(
    State(application): State<Application>,
    ApiJson(request): ApiJson<ListCommunityTriggersRequest>,
) -> Result<Json<CommunityTriggerList>, WebError> {
    application
        .list_community_triggers(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/metadata/trigger",
    tag = "community",
    request_body = GetCommunityTriggerRequest,
    responses(
        (status = 200, description = "Community trigger detail", body = CommunityTrigger),
        (status = 400, description = "Invalid Community metadata request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community metadata failure", body = ApiError)
    )
)]
async fn get_community_trigger(
    State(application): State<Application>,
    ApiJson(request): ApiJson<GetCommunityTriggerRequest>,
) -> Result<Json<CommunityTrigger>, WebError> {
    application
        .get_community_trigger(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/sql/build-create-schema",
    tag = "community",
    request_body = BuildCommunityCreateSchemaRequest,
    responses(
        (status = 200, description = "Community CREATE SCHEMA SQL", body = CommunityBuiltSql),
        (status = 400, description = "Invalid Community SQL-builder request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community SQL-builder failure", body = ApiError)
    )
)]
async fn build_community_create_schema(
    State(application): State<Application>,
    ApiJson(request): ApiJson<BuildCommunityCreateSchemaRequest>,
) -> Result<Json<CommunityBuiltSql>, WebError> {
    application
        .build_community_create_schema(request)
        .await
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/api/v1/community/sql/parse",
    tag = "community",
    request_body = ParseCommunitySqlRequest,
    responses(
        (status = 200, description = "Community SQL analysis", body = CommunitySqlAnalysis),
        (status = 400, description = "Invalid Community SQL-parser request", body = ApiError),
        (status = 503, description = "Community compatibility engine is unavailable", body = ApiError),
        (status = 500, description = "Unexpected Community SQL-parser failure", body = ApiError)
    )
)]
async fn parse_community_sql(
    State(application): State<Application>,
    ApiJson(request): ApiJson<ParseCommunitySqlRequest>,
) -> Result<Json<CommunitySqlAnalysis>, WebError> {
    application
        .parse_community_sql(request)
        .await
        .map(Json)
        .map_err(Into::into)
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
