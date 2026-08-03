//! Bounded MCP tools attached to the running local `Chat2DB` product host.

use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chat2db_contract::{
    ApiError, DatabaseWriteResult, DatabaseWriteState, ExecuteDatabaseWriteRequest, QueryLimits,
    ResultPageRequest, StartQueryRequest,
};
use chat2db_local::{LocalClient, LocalError};
use rand::RngCore as _;
use rmcp::{
    Peer, RoleServer, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Implementation, ServerCapabilities, ServerInfo},
    schemars,
    service::ElicitationMode,
    tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const MAX_QUERY_ROWS: u64 = 10_000;
const MAX_QUERY_RESULT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_QUERY_RESULT_TTL_SECONDS: u32 = 900;
const QUERY_BATCH_ROWS: u32 = 256;
const QUERY_BATCH_BYTES: u32 = 1024 * 1024;
const DEFAULT_PAGE_ROWS: u64 = 100;
const DEFAULT_PAGE_BYTES: u64 = 256 * 1024;
const MAX_PAGE_ROWS: u64 = 1_000;
const MAX_PAGE_BYTES: u64 = 512 * 1024;
const WRITE_APPROVAL_TTL: Duration = Duration::from_secs(5 * 60);
const WRITE_ELICITATION_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const MAX_PENDING_WRITE_APPROVALS: usize = 256;
const MAX_ELICITATION_DATASOURCE_CHARS: usize = 128;
const MAX_ELICITATION_SQL_CHARS: usize = 512;

/// One externally authorized, exact database-write capability.
pub struct DatabaseWriteApproval {
    approval_id: String,
    datasource_id: String,
    sql_sha256: String,
}

impl DatabaseWriteApproval {
    /// Returns an opaque host-side receipt id that is never accepted by MCP tool arguments.
    #[must_use]
    pub fn approval_id(&self) -> &str {
        &self.approval_id
    }

    /// Returns the datasource id bound to this capability.
    #[must_use]
    pub fn datasource_id(&self) -> &str {
        &self.datasource_id
    }

    /// Returns the lowercase SHA-256 digest of the exact approved SQL bytes.
    #[must_use]
    pub fn sql_sha256(&self) -> &str {
        &self.sql_sha256
    }
}

impl fmt::Debug for DatabaseWriteApproval {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DatabaseWriteApproval")
            .field("approval_id", &"[REDACTED]")
            .field("datasource_id", &self.datasource_id)
            .field("sql_sha256", &self.sql_sha256)
            .finish()
    }
}

/// Trusted host-side authority for minting database-write capabilities.
///
/// This handle is deliberately not exposed as an MCP tool. Product UI or CLI code
/// may hold it after completing an approval interaction, while the model-facing
/// server receives only the consuming half of the registry.
#[derive(Clone)]
pub struct WriteApprovalAuthority {
    registry: WriteApprovalRegistry,
}

impl WriteApprovalAuthority {
    /// Authorizes one exact datasource and SQL byte sequence for one use.
    ///
    /// # Errors
    ///
    /// Returns an error for empty bindings, an unavailable registry, or a full
    /// pending-approval queue.
    pub fn approve_database_write(
        &self,
        datasource_id: &str,
        sql: &str,
    ) -> Result<DatabaseWriteApproval, Box<ApiError>> {
        self.registry.issue(datasource_id, sql)
    }
}

impl fmt::Debug for WriteApprovalAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WriteApprovalAuthority")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Default)]
struct WriteApprovalRegistry {
    pending: Arc<Mutex<HashMap<[u8; 32], WriteApprovalBinding>>>,
}

struct WriteApprovalBinding {
    datasource_id: String,
    sql_sha256: [u8; 32],
    expires_at: Instant,
}

impl fmt::Debug for WriteApprovalRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WriteApprovalRegistry")
            .finish_non_exhaustive()
    }
}

impl WriteApprovalRegistry {
    fn issue(
        &self,
        datasource_id: &str,
        sql: &str,
    ) -> Result<DatabaseWriteApproval, Box<ApiError>> {
        if datasource_id.trim().is_empty() || sql.trim().is_empty() {
            return Err(Box::new(ApiError::new(
                "invalid_database_write_approval",
                "Database write approvals require a datasource and SQL",
            )));
        }
        let now = Instant::now();
        let mut pending = self.pending.lock().map_err(|_| {
            Box::new(ApiError::new(
                "database_write_approval_unavailable",
                "Database write approval is unavailable",
            ))
        })?;
        pending.retain(|_, binding| binding.expires_at > now);
        if pending.len() >= MAX_PENDING_WRITE_APPROVALS {
            return Err(Box::new(ApiError::new(
                "database_write_approval_capacity_reached",
                "Too many database write approvals are pending",
            )));
        }

        let sql_sha256 = sha256(sql.as_bytes());
        loop {
            let mut token_bytes = [0_u8; 32];
            rand::rng().fill_bytes(&mut token_bytes);
            let token = hex::encode(token_bytes);
            let token_sha256 = sha256(token.as_bytes());
            if pending.contains_key(&token_sha256) {
                continue;
            }
            pending.insert(
                token_sha256,
                WriteApprovalBinding {
                    datasource_id: datasource_id.to_owned(),
                    sql_sha256,
                    expires_at: now + WRITE_APPROVAL_TTL,
                },
            );
            return Ok(DatabaseWriteApproval {
                approval_id: token,
                datasource_id: datasource_id.to_owned(),
                sql_sha256: hex::encode(sql_sha256),
            });
        }
    }

    fn consume(&self, token: &str, datasource_id: &str, sql: &str) -> bool {
        if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return false;
        }
        let token_sha256 = sha256(token.as_bytes());
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        let Some(binding) = pending.remove(&token_sha256) else {
            return false;
        };
        binding.expires_at > Instant::now()
            && binding.datasource_id == datasource_id
            && binding.sql_sha256 == sha256(sql.as_bytes())
    }

    fn consume_matching(&self, datasource_id: &str, sql: &str) -> bool {
        let now = Instant::now();
        let sql_sha256 = sha256(sql.as_bytes());
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        pending.retain(|_, binding| binding.expires_at > now);
        let matching_token = pending.iter().find_map(|(token, binding)| {
            (binding.datasource_id == datasource_id && binding.sql_sha256 == sql_sha256)
                .then_some(*token)
        });
        matching_token
            .and_then(|token| pending.remove(&token))
            .is_some()
    }
}

fn sha256(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

/// MCP service backed by the same application instance used by Web and desktop.
#[derive(Debug, Clone)]
pub struct McpServer {
    local: LocalClient,
    write_approvals: WriteApprovalRegistry,
}

impl McpServer {
    #[must_use]
    pub fn new(local: LocalClient) -> Self {
        Self {
            local,
            write_approvals: WriteApprovalRegistry::default(),
        }
    }

    /// Builds a server plus a separate trusted handle that can approve exact writes.
    #[must_use]
    pub fn with_write_approval_authority(local: LocalClient) -> (Self, WriteApprovalAuthority) {
        let registry = WriteApprovalRegistry::default();
        let authority = WriteApprovalAuthority {
            registry: registry.clone(),
        };
        (
            Self {
                local,
                write_approvals: registry,
            },
            authority,
        )
    }

    async fn obtain_external_write_approval(
        &self,
        client: &Peer<RoleServer>,
        input: &WriteDatabaseInput,
    ) -> bool {
        if self
            .write_approvals
            .consume_matching(&input.datasource_id, &input.sql)
        {
            return true;
        }
        if !client
            .supported_elicitation_modes()
            .contains(&ElicitationMode::Form)
        {
            return false;
        }

        let response = client
            .elicit_with_timeout::<DatabaseWriteApprovalForm>(
                database_write_elicitation_message(&input.datasource_id, &input.sql),
                Some(WRITE_ELICITATION_TIMEOUT),
            )
            .await;
        if !matches!(
            response,
            Ok(Some(DatabaseWriteApprovalForm { confirm: true }))
        ) {
            return false;
        }

        let Ok(approval) = self.write_approvals.issue(&input.datasource_id, &input.sql) else {
            return false;
        };
        self.write_approvals
            .consume(approval.approval_id(), &input.datasource_id, &input.sql)
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

    /// Execute one externally approved database write without automatic retries.
    #[tool(
        name = "execute_database_write",
        description = "Request trusted-host approval through MCP elicitation, then execute exactly one MySQL write. Approval is bound to the exact datasource and SQL digest and cannot be supplied by tool arguments. Clients without form elicitation fail closed. Only not_started is safe to retry after correction; never retry failed or unknown blindly.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn execute_database_write(
        &self,
        Parameters(input): Parameters<WriteDatabaseInput>,
        client: Peer<RoleServer>,
    ) -> CallToolResult {
        if !self.obtain_external_write_approval(&client, &input).await {
            return database_write_approval_required();
        }
        let result = self
            .local
            .execute_database_write(ExecuteDatabaseWriteRequest {
                datasource_id: input.datasource_id,
                sql: input.sql,
                confirmed: true,
            })
            .await;
        structured_write_result(&result)
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
                "Use asynchronous read-only queries by default. Database writes trigger trusted-host form elicitation showing the datasource, exact SQL SHA-256, and a bounded SQL preview. Tool arguments cannot approve writes, and clients without elicitation fail closed. Only not_started is safe to retry after correction; never retry failed or unknown blindly.",
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
struct WriteDatabaseInput {
    /// Opaque datasource id returned by `list_datasources`.
    datasource_id: String,
    /// Exactly one `MySQL` DML, DDL, grant, or routine statement.
    sql: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DatabaseWriteApprovalForm {
    /// Set true only after the human has inspected the datasource, SQL digest, and preview.
    confirm: bool,
}

rmcp::elicit_safe!(DatabaseWriteApprovalForm);

fn database_write_elicitation_message(datasource_id: &str, sql: &str) -> String {
    format!(
        "Approve one database write requested by an untrusted model.\nDatasource ID: {}\nExact SQL SHA-256: {}\nBounded SQL preview: {}\nSet confirm to true only after checking all three values.",
        bounded_elicitation_text(datasource_id, MAX_ELICITATION_DATASOURCE_CHARS),
        hex::encode(sha256(sql.as_bytes())),
        bounded_elicitation_text(sql, MAX_ELICITATION_SQL_CHARS),
    )
}

fn bounded_elicitation_text(value: &str, maximum_chars: usize) -> String {
    let mut escaped = value.chars().flat_map(char::escape_default);
    let mut output = escaped.by_ref().take(maximum_chars).collect::<String>();
    if escaped.next().is_some() {
        output.push_str("...");
    }
    output
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

fn structured_write_result(value: &DatabaseWriteResult) -> CallToolResult {
    let succeeded = value.state == DatabaseWriteState::Succeeded;
    match serde_json::to_value(value) {
        Ok(value) if succeeded => CallToolResult::structured(value),
        Ok(value) => CallToolResult::structured_error(value),
        Err(_) => encoding_failure(),
    }
}

fn database_write_approval_required() -> CallToolResult {
    structured_write_result(&DatabaseWriteResult {
        state: DatabaseWriteState::NotStarted,
        affected_rows: None,
        error: Some(ApiError::new(
            "database_write_approval_required",
            "Explicit trusted-host approval bound to this exact datasource and SQL is required",
        )),
    })
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
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use chat2db_contract::CreateDatasourceRequest;
    use chat2db_core::Application;
    use chat2db_local::LocalServer;
    use chat2db_storage::{SecretRef, SecretValue, SecretVault, SecretVaultError, Storage};
    use rmcp::{
        ClientHandler, RoleClient, ServerHandler as _, ServiceExt as _,
        handler::server::wrapper::Parameters,
        model::{
            CallToolRequestParams, ClientCapabilities, ClientInfo, ElicitRequestParams,
            ElicitResult, ElicitationAction,
        },
        service::RequestContext,
    };
    use tempfile::TempDir;

    use super::{
        McpServer, OperationInput, QueryDatabaseInput, ResultPageInput, WriteApprovalAuthority,
        WriteDatabaseInput,
    };

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

    #[derive(Clone)]
    struct ApprovalClient {
        decisions: Arc<Mutex<VecDeque<ApprovalDecision>>>,
        messages: Arc<Mutex<Vec<String>>>,
    }

    #[derive(Clone, Copy)]
    enum ApprovalDecision {
        Confirm(bool),
        Decline,
        Cancel,
        InvalidContent,
        ProtocolError,
    }

    impl ApprovalClient {
        fn new(decisions: impl IntoIterator<Item = bool>) -> Self {
            Self::with_decisions(decisions.into_iter().map(ApprovalDecision::Confirm))
        }

        fn with_decisions(decisions: impl IntoIterator<Item = ApprovalDecision>) -> Self {
            Self {
                decisions: Arc::new(Mutex::new(decisions.into_iter().collect())),
                messages: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn messages(&self) -> Vec<String> {
            self.messages.lock().expect("messages lock").clone()
        }
    }

    impl ClientHandler for ApprovalClient {
        fn get_info(&self) -> ClientInfo {
            let mut info = ClientInfo::default();
            info.capabilities = ClientCapabilities::builder().enable_elicitation().build();
            info
        }

        async fn create_elicitation(
            &self,
            request: ElicitRequestParams,
            _context: RequestContext<RoleClient>,
        ) -> Result<ElicitResult, rmcp::ErrorData> {
            let ElicitRequestParams::FormElicitationParams {
                message,
                requested_schema,
                ..
            } = request
            else {
                return Err(rmcp::ErrorData::invalid_params(
                    "database write approval requires form elicitation",
                    None,
                ));
            };
            assert!(requested_schema.properties.contains_key("confirm"));
            assert!(
                requested_schema
                    .required
                    .as_ref()
                    .is_some_and(|required| required.iter().any(|field| field == "confirm"))
            );
            self.messages.lock().expect("messages lock").push(message);
            let decision = self
                .decisions
                .lock()
                .expect("decisions lock")
                .pop_front()
                .unwrap_or(ApprovalDecision::Cancel);
            if matches!(decision, ApprovalDecision::ProtocolError) {
                return Err(rmcp::ErrorData::internal_error(
                    "elicitation transport failed",
                    None,
                ));
            }
            Ok(match decision {
                ApprovalDecision::Confirm(confirm) => ElicitResult::new(ElicitationAction::Accept)
                    .with_content(serde_json::json!({ "confirm": confirm })),
                ApprovalDecision::Decline => ElicitResult::new(ElicitationAction::Decline),
                ApprovalDecision::Cancel => ElicitResult::new(ElicitationAction::Cancel),
                ApprovalDecision::InvalidContent => ElicitResult::new(ElicitationAction::Accept)
                    .with_content(serde_json::json!({ "confirm": "not-a-boolean" })),
                ApprovalDecision::ProtocolError => unreachable!("returned above"),
            })
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
            Self::build(false).0
        }

        fn with_write_approval_authority() -> (Self, WriteApprovalAuthority) {
            let (fixture, authority) = Self::build(true);
            (
                fixture,
                authority.expect("write approval authority must be constructed"),
            )
        }

        fn build(with_authority: bool) -> (Self, Option<WriteApprovalAuthority>) {
            let directory = TempDir::new().expect("temp dir");
            let storage =
                Storage::open(directory.path(), Arc::new(EmptyVault)).expect("storage opens");
            let application = Application::with_storage(storage);
            let server = LocalServer::start(application.clone()).expect("local server starts");
            let local = chat2db_local::LocalClient::new(directory.path());
            let (mcp, authority) = if with_authority {
                let (mcp, authority) = McpServer::with_write_approval_authority(local);
                (mcp, Some(authority))
            } else {
                (McpServer::new(local), None)
            };
            (
                Self {
                    server,
                    application,
                    mcp,
                    _directory: directory,
                },
                authority,
            )
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
    fn model_confirmation_boolean_cannot_authorize_a_write() {
        let old_input = serde_json::json!({
            "datasourceId": "datasource-1",
            "sql": "UPDATE items SET label = 'changed'",
            "confirm": true
        });
        assert!(serde_json::from_value::<WriteDatabaseInput>(old_input).is_err());

        let forged_token = serde_json::json!({
            "datasourceId": "datasource-1",
            "sql": "UPDATE items SET label = 'changed'",
            "approvalToken": "model-controlled-token"
        });
        assert!(serde_json::from_value::<WriteDatabaseInput>(forged_token).is_err());
    }

    #[test]
    fn external_write_approvals_are_exact_single_use_capabilities() {
        let (server, authority) =
            McpServer::with_write_approval_authority(chat2db_local::LocalClient::new("unused"));
        let datasource_id = "datasource-1";
        let sql = "UPDATE items SET label = 'changed'";

        let changed_sql_approval = authority
            .approve_database_write(datasource_id, sql)
            .expect("trusted host can approve a write");
        assert_eq!(changed_sql_approval.datasource_id(), datasource_id);
        assert_eq!(changed_sql_approval.approval_id().len(), 64);
        assert_eq!(
            changed_sql_approval.sql_sha256(),
            hex::encode(super::sha256(sql.as_bytes()))
        );
        let approval_debug = format!("{changed_sql_approval:?}");
        assert!(!approval_debug.contains(changed_sql_approval.approval_id()));
        assert!(approval_debug.contains("[REDACTED]"));

        assert!(!server.write_approvals.consume(
            changed_sql_approval.approval_id(),
            datasource_id,
            &format!("{sql} ")
        ));
        assert!(!server.write_approvals.consume(
            changed_sql_approval.approval_id(),
            datasource_id,
            sql
        ));

        let datasource_approval = authority
            .approve_database_write(datasource_id, sql)
            .expect("trusted host can approve a second write");
        assert!(!server.write_approvals.consume(
            datasource_approval.approval_id(),
            "datasource-2",
            sql
        ));

        let valid_approval = authority
            .approve_database_write(datasource_id, sql)
            .expect("trusted host can approve an exact write");
        assert!(
            server
                .write_approvals
                .consume(valid_approval.approval_id(), datasource_id, sql)
        );
        assert!(
            !server
                .write_approvals
                .consume(valid_approval.approval_id(), datasource_id, sql)
        );

        let _concurrent_approval = authority
            .approve_database_write(datasource_id, sql)
            .expect("trusted host can approve a concurrent write");
        let first_registry = server.write_approvals.clone();
        let second_registry = first_registry.clone();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let first_barrier = barrier.clone();
        let second_barrier = barrier.clone();
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(move || {
                first_barrier.wait();
                first_registry.consume_matching(datasource_id, sql)
            });
            let second = scope.spawn(move || {
                second_barrier.wait();
                second_registry.consume_matching(datasource_id, sql)
            });
            barrier.wait();
            (
                first.join().expect("first approval consumer"),
                second.join().expect("second approval consumer"),
            )
        });
        assert_eq!(
            usize::from(first) + usize::from(second),
            1,
            "atomic consumption must allow exactly one concurrent caller past approval"
        );
    }

    #[tokio::test]
    async fn stdio_without_elicitation_fails_closed_but_accepts_host_preapproval() {
        let (fixture, authority) = Fixture::with_write_approval_authority();
        let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
        let mcp = fixture.mcp.clone();
        let server_task = tokio::spawn(async move {
            mcp.serve(server_transport)
                .await
                .expect("MCP server handshake")
                .waiting()
                .await
                .expect("MCP server shutdown");
        });
        let client = ().serve(client_transport).await.expect("MCP client handshake");

        let read = client
            .call_tool(CallToolRequestParams::new("list_datasources"))
            .await
            .expect("read tool call");
        assert_eq!(read.is_error, Some(false));

        let datasource_id = "datasource-1";
        let sql = "UPDATE items SET label = 'changed'";
        let unapproved = call_write_tool(&client, datasource_id, sql).await;
        assert_write_error_code(&unapproved, "database_write_approval_required");

        let _approval = authority
            .approve_database_write(datasource_id, sql)
            .expect("trusted embedding host can preapprove");
        let preapproved = call_write_tool(&client, datasource_id, sql).await;
        assert_ne!(
            write_error_code(&preapproved),
            "database_write_approval_required"
        );
        let replay = call_write_tool(&client, datasource_id, sql).await;
        assert_write_error_code(&replay, "database_write_approval_required");

        client.cancel().await.expect("MCP client cancellation");
        server_task.await.expect("MCP server task");
        fixture.shutdown().await;
    }

    #[tokio::test]
    async fn stdio_elicitation_requires_explicit_true_for_the_exact_write() {
        let fixture = Fixture::new();
        let approval_client = ApprovalClient::new([false, true]);
        let observed_client = approval_client.clone();
        let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
        let mcp = fixture.mcp.clone();
        let server_task = tokio::spawn(async move {
            mcp.serve(server_transport)
                .await
                .expect("MCP server handshake")
                .waiting()
                .await
                .expect("MCP server shutdown");
        });
        let client = approval_client
            .serve(client_transport)
            .await
            .expect("elicitation client handshake");

        let datasource_id = "datasource-1";
        let sql = "UPDATE items SET label = 'changed'";
        let declined = call_write_tool(&client, datasource_id, sql).await;
        assert_write_error_code(&declined, "database_write_approval_required");
        let approved = call_write_tool(&client, datasource_id, sql).await;
        assert_ne!(
            write_error_code(&approved),
            "database_write_approval_required",
            "accepted elicitation with confirm=true must cross the approval boundary"
        );

        let messages = observed_client.messages();
        assert_eq!(messages.len(), 2);
        for message in messages {
            assert!(message.contains(datasource_id));
            assert!(message.contains(&hex::encode(super::sha256(sql.as_bytes()))));
            assert!(message.contains(&super::bounded_elicitation_text(
                sql,
                super::MAX_ELICITATION_SQL_CHARS,
            )));
        }

        client.cancel().await.expect("MCP client cancellation");
        server_task.await.expect("MCP server task");
        fixture.shutdown().await;
    }

    #[tokio::test]
    async fn stdio_elicitation_decline_cancel_invalid_content_and_errors_fail_closed() {
        let fixture = Fixture::new();
        let approval_client = ApprovalClient::with_decisions([
            ApprovalDecision::Confirm(false),
            ApprovalDecision::Decline,
            ApprovalDecision::Cancel,
            ApprovalDecision::InvalidContent,
            ApprovalDecision::ProtocolError,
        ]);
        let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
        let mcp = fixture.mcp.clone();
        let server_task = tokio::spawn(async move {
            mcp.serve(server_transport)
                .await
                .expect("MCP server handshake")
                .waiting()
                .await
                .expect("MCP server shutdown");
        });
        let client = approval_client
            .serve(client_transport)
            .await
            .expect("elicitation client handshake");

        for _ in 0..5 {
            let result = call_write_tool(
                &client,
                "datasource-1",
                "UPDATE items SET label = 'changed'",
            )
            .await;
            assert_write_error_code(&result, "database_write_approval_required");
        }

        client.cancel().await.expect("MCP client cancellation");
        server_task.await.expect("MCP server task");
        fixture.shutdown().await;
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
                "execute_database_write",
                "inspect_query_operation",
                "inspect_query_result",
                "list_datasources",
                "query_database",
            ]
        );

        for tool in &tools {
            let annotations = tool.annotations.as_ref().expect("annotations exist");
            assert_eq!(
                annotations.destructive_hint,
                Some(tool.name == "execute_database_write")
            );
            assert_eq!(annotations.open_world_hint, Some(false));
            assert_eq!(
                annotations.read_only_hint,
                Some(!matches!(
                    tool.name.as_ref(),
                    "cancel_database_query" | "execute_database_write"
                ))
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

        let write = tools
            .iter()
            .find(|tool| tool.name == "execute_database_write")
            .unwrap();
        assert_eq!(
            write.annotations.as_ref().unwrap().idempotent_hint,
            Some(false)
        );
        let required = write.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|field| field == "datasourceId"));
        assert!(required.iter().any(|field| field == "sql"));
        assert!(!required.iter().any(|field| field == "confirm"));
        assert!(!required.iter().any(|field| field == "approvalToken"));
        assert!(write.input_schema["properties"].get("confirm").is_none());
        assert!(
            write.input_schema["properties"]
                .get("approvalToken")
                .is_none()
        );
    }

    async fn call_write_tool<H: ClientHandler>(
        client: &rmcp::service::RunningService<RoleClient, H>,
        datasource_id: &str,
        sql: &str,
    ) -> rmcp::model::CallToolResult {
        client
            .call_tool(
                CallToolRequestParams::new("execute_database_write").with_arguments(
                    serde_json::json!({
                        "datasourceId": datasource_id,
                        "sql": sql
                    })
                    .as_object()
                    .expect("write arguments must be an object")
                    .clone(),
                ),
            )
            .await
            .expect("write tool call")
    }

    fn assert_error_code(result: &rmcp::model::CallToolResult, expected: &str) {
        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            result.structured_content.as_ref().unwrap()["code"],
            expected
        );
    }

    fn write_error_code(result: &rmcp::model::CallToolResult) -> &str {
        result.structured_content.as_ref().unwrap()["error"]["code"]
            .as_str()
            .expect("write error code must be a string")
    }

    fn assert_write_error_code(result: &rmcp::model::CallToolResult, expected: &str) {
        assert_eq!(result.is_error, Some(true));
        assert_eq!(write_error_code(result), expected);
    }
}
