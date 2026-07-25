//! Bounded MCP tools attached to the running local `Chat2DB` product host.

use chat2db_contract::{ApiError, QueryLimits, ResultPageRequest, StartQueryRequest};
use chat2db_local::{LocalClient, LocalError};
use rmcp::{
    ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};

const MAX_QUERY_ROWS: u64 = 10_000;
const MAX_QUERY_RESULT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_QUERY_RESULT_TTL_SECONDS: u32 = 900;
const QUERY_BATCH_ROWS: u32 = 256;
const QUERY_BATCH_BYTES: u32 = 1024 * 1024;
const DEFAULT_PAGE_ROWS: u64 = 100;
const DEFAULT_PAGE_BYTES: u64 = 256 * 1024;
const MAX_PAGE_ROWS: u64 = 1_000;
const MAX_PAGE_BYTES: u64 = 512 * 1024;

/// MCP service backed by the same application instance used by Web and desktop.
#[derive(Debug, Clone)]
pub struct McpServer {
    local: LocalClient,
}

impl McpServer {
    #[must_use]
    pub const fn new(local: LocalClient) -> Self {
        Self { local }
    }
}

#[tool_router]
impl McpServer {
    /// List configured datasources without connection secrets.
    #[tool(
        name = "list_datasources",
        description = "List configured Chat2DB datasources without returning connection secrets.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn list_datasources(&self) -> CallToolResult {
        match self.local.list_datasources().await {
            Ok(value) => structured_success(&value),
            Err(error) => structured_local_error(error),
        }
    }

    /// Start one forced-read-only asynchronous database query.
    #[tool(
        name = "query_database",
        description = "Start a forced-read-only database query. Returns only an operationId; poll inspect_query_operation and page the final result with inspect_query_result.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn query_database(
        &self,
        Parameters(input): Parameters<QueryDatabaseInput>,
    ) -> CallToolResult {
        let limits = match query_limits(&input) {
            Ok(limits) => limits,
            Err(error) => return structured_api_error(*error),
        };
        let request = StartQueryRequest {
            datasource_id: input.datasource_id,
            sql: input.sql,
            parameters: Vec::new(),
            limits,
        };
        match self.local.start_read_query(request).await {
            Ok(value) => structured_success(&value),
            Err(error) => structured_local_error(error),
        }
    }

    /// Inspect the current lifecycle state of one query operation.
    #[tool(
        name = "inspect_query_operation",
        description = "Inspect an asynchronous query operation. A completed operation includes the result id required by inspect_query_result.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn inspect_query_operation(
        &self,
        Parameters(input): Parameters<OperationInput>,
    ) -> CallToolResult {
        match self.local.operation_snapshot(input.operation_id).await {
            Ok(value) => structured_success(&value),
            Err(error) => structured_local_error(error),
        }
    }

    /// Request idempotent cancellation of one query operation.
    #[tool(
        name = "cancel_database_query",
        description = "Request idempotent cancellation of an asynchronous database query.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn cancel_database_query(
        &self,
        Parameters(input): Parameters<OperationInput>,
    ) -> CallToolResult {
        match self.local.cancel_operation(input.operation_id).await {
            Ok(value) => structured_success(&value),
            Err(error) => structured_local_error(error),
        }
    }

    /// Read one row- and byte-bounded retained-result page.
    #[tool(
        name = "inspect_query_result",
        description = "Read one bounded page of a completed query result. Use offset and hasMore to retrieve additional pages.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn inspect_query_result(
        &self,
        Parameters(input): Parameters<ResultPageInput>,
    ) -> CallToolResult {
        let request = ResultPageRequest {
            offset: input.offset.unwrap_or_default().to_string(),
            max_rows: input.max_rows.unwrap_or(DEFAULT_PAGE_ROWS).to_string(),
            max_bytes: input.max_bytes.unwrap_or(DEFAULT_PAGE_BYTES).to_string(),
        };
        match self.local.result_page(input.result_id, request).await {
            Ok(value) => structured_success(&value),
            Err(error) => structured_local_error(error),
        }
    }
}

#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Use asynchronous read-only queries, poll operations, and page retained results.",
            )
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryDatabaseInput {
    /// Opaque datasource id returned by `list_datasources`.
    datasource_id: String,
    /// SQL text. The product runtime independently enforces read-only execution.
    sql: String,
    /// Maximum retained rows; defaults to 10000.
    #[schemars(range(min = 1, max = MAX_QUERY_ROWS))]
    max_rows: Option<u64>,
    /// Maximum retained encoded result bytes; defaults to 16777216.
    #[schemars(range(min = 1, max = MAX_QUERY_RESULT_BYTES))]
    max_result_bytes: Option<u64>,
    /// Retained-result lifetime in seconds; defaults to 900.
    #[schemars(range(min = 1, max = MAX_QUERY_RESULT_TTL_SECONDS))]
    result_ttl_seconds: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OperationInput {
    /// Opaque operation id returned by `query_database`.
    operation_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResultPageInput {
    /// Opaque result id returned by a completed operation.
    result_id: String,
    /// Zero-based row offset; defaults to 0.
    #[schemars(range(min = 0))]
    offset: Option<u64>,
    /// Maximum rows in this page; defaults to 100 and cannot exceed 1000.
    #[schemars(range(min = 1, max = MAX_PAGE_ROWS))]
    max_rows: Option<u64>,
    /// Maximum encoded row bytes; defaults to 262144 and cannot exceed 524288.
    #[schemars(range(min = 1, max = MAX_PAGE_BYTES))]
    max_bytes: Option<u64>,
}

fn query_limits(input: &QueryDatabaseInput) -> Result<QueryLimits, Box<ApiError>> {
    let max_rows = input.max_rows.unwrap_or(MAX_QUERY_ROWS);
    if !(1..=MAX_QUERY_ROWS).contains(&max_rows) {
        return Err(Box::new(ApiError::new(
            "invalid_query_limits",
            format!("maxRows must be between 1 and {MAX_QUERY_ROWS}"),
        )));
    }
    let max_result_bytes = input.max_result_bytes.unwrap_or(MAX_QUERY_RESULT_BYTES);
    if !(1..=MAX_QUERY_RESULT_BYTES).contains(&max_result_bytes) {
        return Err(Box::new(ApiError::new(
            "invalid_query_limits",
            format!("maxResultBytes must be between 1 and {MAX_QUERY_RESULT_BYTES}"),
        )));
    }
    let result_ttl_seconds = input
        .result_ttl_seconds
        .unwrap_or(MAX_QUERY_RESULT_TTL_SECONDS);
    if !(1..=MAX_QUERY_RESULT_TTL_SECONDS).contains(&result_ttl_seconds) {
        return Err(Box::new(ApiError::new(
            "invalid_query_limits",
            format!("resultTtlSeconds must be between 1 and {MAX_QUERY_RESULT_TTL_SECONDS}"),
        )));
    }
    Ok(QueryLimits {
        max_rows: max_rows.to_string(),
        max_result_bytes: max_result_bytes.to_string(),
        batch_rows: QUERY_BATCH_ROWS,
        batch_bytes: QUERY_BATCH_BYTES,
        result_ttl_seconds,
    })
}

fn structured_success(value: &impl Serialize) -> CallToolResult {
    match serde_json::to_value(value) {
        Ok(value) => CallToolResult::structured(value),
        Err(_) => encoding_failure(),
    }
}

fn structured_local_error(error: LocalError) -> CallToolResult {
    structured_api_error(safe_local_error(error))
}

fn structured_api_error(error: ApiError) -> CallToolResult {
    match serde_json::to_value(error) {
        Ok(value) => CallToolResult::structured_error(value),
        Err(_) => encoding_failure(),
    }
}

fn safe_local_error(error: LocalError) -> ApiError {
    match error {
        LocalError::Remote(error) => (*error).0,
        LocalError::Unavailable(_) | LocalError::Task(_) => runtime_error(
            "local_runtime_unavailable",
            "The Chat2DB local runtime is unavailable",
            true,
        ),
        LocalError::Timeout(_) => runtime_error(
            "local_runtime_timeout",
            "The Chat2DB local runtime did not respond in time",
            true,
        ),
        LocalError::Io { .. } => runtime_error(
            "local_runtime_io_error",
            "The Chat2DB local runtime could not be reached",
            true,
        ),
        LocalError::Protocol(_) | LocalError::Json(_) => runtime_error(
            "local_runtime_protocol_error",
            "The Chat2DB local runtime returned an invalid response",
            false,
        ),
    }
}

fn runtime_error(code: &'static str, message: &'static str, retryable: bool) -> ApiError {
    let mut error = ApiError::new(code, message);
    error.retryable = retryable;
    error
}

fn encoding_failure() -> CallToolResult {
    CallToolResult::structured_error(serde_json::json!({
        "code": "mcp_response_encoding_failed",
        "message": "The MCP response could not be encoded",
        "retryable": false
    }))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chat2db_contract::CreateDatasourceRequest;
    use chat2db_core::Application;
    use chat2db_local::LocalServer;
    use chat2db_storage::{SecretRef, SecretValue, SecretVault, SecretVaultError, Storage};
    use rmcp::ServerHandler as _;
    use rmcp::handler::server::wrapper::Parameters;
    use tempfile::TempDir;

    use super::{McpServer, OperationInput, QueryDatabaseInput, ResultPageInput};

    struct EmptyVault;

    impl SecretVault for EmptyVault {
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

    struct Fixture {
        server: LocalServer,
        application: Application,
        mcp: McpServer,
        _directory: TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = TempDir::new().expect("temp dir");
            let storage =
                Storage::open(directory.path(), Arc::new(EmptyVault)).expect("storage opens");
            let application = Application::with_storage(storage);
            let server = LocalServer::start(application.clone()).expect("local server starts");
            let mcp = McpServer::new(chat2db_local::LocalClient::new(directory.path()));
            Self {
                server,
                application,
                mcp,
                _directory: directory,
            }
        }

        async fn shutdown(mut self) {
            self.server.shutdown().await.expect("server shuts down");
        }
    }

    #[tokio::test]
    async fn exposes_all_tools_through_the_real_local_attachment() {
        let fixture = Fixture::new();
        fixture
            .application
            .create_datasource(CreateDatasourceRequest {
                name: "MCP test".to_owned(),
                driver_id: "driver-1".to_owned(),
                connection: None,
            })
            .await
            .expect("datasource creates");

        let datasources = fixture.mcp.list_datasources().await;
        assert_eq!(datasources.is_error, Some(false));
        assert_eq!(
            datasources.structured_content.as_ref().unwrap()["items"][0]["name"],
            "MCP test"
        );

        let query = fixture
            .mcp
            .query_database(Parameters(QueryDatabaseInput {
                datasource_id: "datasource-1".to_owned(),
                sql: "select 1".to_owned(),
                max_rows: None,
                max_result_bytes: None,
                result_ttl_seconds: None,
            }))
            .await;
        assert_error_code(&query, "database_engine_unavailable");

        let operation = fixture
            .mcp
            .inspect_query_operation(Parameters(OperationInput {
                operation_id: "missing-operation".to_owned(),
            }))
            .await;
        assert_error_code(&operation, "operation_not_found");

        let cancellation = fixture
            .mcp
            .cancel_database_query(Parameters(OperationInput {
                operation_id: "missing-operation".to_owned(),
            }))
            .await;
        assert_eq!(cancellation.is_error, Some(false));
        assert_eq!(
            cancellation.structured_content.as_ref().unwrap()["disposition"],
            "unknown_operation"
        );

        let result = fixture
            .mcp
            .inspect_query_result(Parameters(ResultPageInput {
                result_id: "missing-result".to_owned(),
                offset: None,
                max_rows: None,
                max_bytes: None,
            }))
            .await;
        assert_error_code(&result, "result_not_found");

        fixture.shutdown().await;
    }

    #[tokio::test]
    async fn preserves_remote_errors_and_redacts_local_failures() {
        let fixture = Fixture::new();
        let oversized = fixture
            .mcp
            .inspect_query_result(Parameters(ResultPageInput {
                result_id: "missing-result".to_owned(),
                offset: Some(0),
                max_rows: Some(1_001),
                max_bytes: Some(1),
            }))
            .await;
        assert_error_code(&oversized, "invalid_result_page");
        fixture.shutdown().await;

        let missing = McpServer::new(chat2db_local::LocalClient::new("missing-runtime"));
        let unavailable = missing.list_datasources().await;
        assert_error_code(&unavailable, "local_runtime_io_error");
        let text = unavailable.content[0].as_text().unwrap().text.as_str();
        assert!(!text.contains("missing-runtime"));
    }

    #[tokio::test]
    async fn rejects_query_retention_outside_mcp_hard_bounds() {
        let mcp = McpServer::new(chat2db_local::LocalClient::new("unused"));
        let invalid = [
            QueryDatabaseInput {
                datasource_id: "datasource-1".to_owned(),
                sql: "select 1".to_owned(),
                max_rows: Some(0),
                max_result_bytes: None,
                result_ttl_seconds: None,
            },
            QueryDatabaseInput {
                datasource_id: "datasource-1".to_owned(),
                sql: "select 1".to_owned(),
                max_rows: Some(super::MAX_QUERY_ROWS + 1),
                max_result_bytes: None,
                result_ttl_seconds: None,
            },
            QueryDatabaseInput {
                datasource_id: "datasource-1".to_owned(),
                sql: "select 1".to_owned(),
                max_rows: None,
                max_result_bytes: Some(super::MAX_QUERY_RESULT_BYTES + 1),
                result_ttl_seconds: None,
            },
            QueryDatabaseInput {
                datasource_id: "datasource-1".to_owned(),
                sql: "select 1".to_owned(),
                max_rows: None,
                max_result_bytes: None,
                result_ttl_seconds: Some(super::MAX_QUERY_RESULT_TTL_SECONDS + 1),
            },
        ];

        for input in invalid {
            let result = mcp.query_database(Parameters(input)).await;
            assert_error_code(&result, "invalid_query_limits");
        }
    }

    #[test]
    fn publishes_stable_tool_names_and_safety_annotations() {
        let info = McpServer::new(chat2db_local::LocalClient::new("unused")).get_info();
        assert_eq!(info.server_info.name, "chat2db-mcp");
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
        assert!(info.capabilities.tools.is_some());

        let tools = McpServer::tool_router().list_all();
        assert_eq!(
            tools
                .iter()
                .map(|tool| tool.name.as_ref())
                .collect::<Vec<_>>(),
            [
                "cancel_database_query",
                "inspect_query_operation",
                "inspect_query_result",
                "list_datasources",
                "query_database",
            ]
        );

        for tool in &tools {
            let annotations = tool.annotations.as_ref().expect("annotations exist");
            assert_eq!(annotations.destructive_hint, Some(false));
            assert_eq!(annotations.open_world_hint, Some(false));
            assert_eq!(
                annotations.read_only_hint,
                Some(tool.name != "cancel_database_query")
            );
        }
        let cancellation = tools
            .iter()
            .find(|tool| tool.name == "cancel_database_query")
            .unwrap();
        assert_eq!(
            cancellation.annotations.as_ref().unwrap().idempotent_hint,
            Some(true)
        );

        let query = tools
            .iter()
            .find(|tool| tool.name == "query_database")
            .unwrap();
        let properties = query.input_schema["properties"].as_object().unwrap();
        assert_eq!(
            properties["maxRows"]["maximum"],
            serde_json::json!(super::MAX_QUERY_ROWS)
        );
        assert_eq!(
            properties["maxResultBytes"]["maximum"],
            serde_json::json!(super::MAX_QUERY_RESULT_BYTES)
        );
        assert_eq!(
            properties["resultTtlSeconds"]["maximum"],
            serde_json::json!(super::MAX_QUERY_RESULT_TTL_SECONDS)
        );
    }

    fn assert_error_code(result: &rmcp::model::CallToolResult, expected: &str) {
        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            result.structured_content.as_ref().unwrap()["code"],
            expected
        );
    }
}
