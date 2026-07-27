//! Tauri IPC delivery adapter for the `Chat2DB` desktop product.

use std::{
    collections::HashMap,
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use chat2db_contract::{
    AgentEventEnvelope, AgentMessageList, AgentPermissionResponse, AgentRunAccepted,
    AgentRunSnapshot, AgentSession, AgentSessionList, AgentStreamMessage,
    AgentSubscriptionAccepted, ApiError, BuildCommunityCreateSchemaRequest,
    BuildCommunityDmlRequest, BuildCommunityNamespaceSqlRequest, CancelAgentRunResponse,
    CancelOperationResponse, CommunityBuiltSql, CommunityDatabaseList, CommunityForeignKeyList,
    CommunityFormattedSql, CommunityFunction, CommunityFunctionList,
    CommunityFunctionParameterList, CommunityPluginCatalog, CommunityPrimaryKeyList,
    CommunityProcedure, CommunityProcedureList, CommunityProcedureParameterList,
    CommunitySchemaList, CommunitySqlAnalysis, CommunitySqlCompletion, CommunitySqlValidation,
    CommunityTableColumnList, CommunityTableIndexList, CommunityTableList,
    CommunityTablePreviewAccepted, CommunityTrigger, CommunityTriggerList, CommunityViewList,
    CompleteCommunitySqlRequest, CreateAgentSessionRequest, CreateDatasourceRequest,
    CreateProviderProfileRequest, Datasource, DatasourceList, DecideAgentPermissionRequest,
    FormatCommunitySqlRequest, GetCommunityFunctionRequest, GetCommunityProcedureRequest,
    GetCommunityTriggerRequest, HealthResponse, JdbcDriverList, ListCommunityColumnsRequest,
    ListCommunityDatabasesRequest, ListCommunityFunctionsRequest, ListCommunityIndexesRequest,
    ListCommunityProceduresRequest, ListCommunitySchemasRequest, ListCommunityTableKeysRequest,
    ListCommunityTablesRequest, ListCommunityTriggersRequest, ListCommunityViewsRequest,
    OperationEventEnvelope, OperationSnapshot, OperationStreamMessage,
    OperationSubscriptionAccepted, ParseCommunitySqlRequest, ProviderProfile, ProviderProfileList,
    QueryAccepted, ResultPage, ResultPageRequest, StartAgentRunRequest,
    StartCommunityTablePreviewRequest, StartQueryRequest, UpdateAgentSessionRequest,
    UpdateDatasourceRequest, UpdateProviderProfileRequest, ValidateCommunitySqlRequest,
};
use chat2db_core::{
    AppError, Application, RuntimeConfig, RuntimeHost, load_fixed_community_classpath,
};
use chat2db_java_bridge::{BridgeError, EngineCommand, EngineConfig};
use chat2db_local::{LocalError, LocalServer};
use tauri::{State, ipc::Channel};
use tokio::sync::{Mutex, oneshot};

const DATA_DIR_ENV: &str = "CHAT2DB_DATA_DIR";
const DRIVER_PACK_DIR_ENV: &str = "CHAT2DB_DRIVER_PACK_DIR";
const COMMUNITY_CLASSPATH_DIR_ENV: &str = "CHAT2DB_COMMUNITY_CLASSPATH_DIR";
const JAVA_BIN_ENV: &str = "CHAT2DB_JAVA_BIN";
const JAVA_ENGINE_JAR_ENV: &str = "CHAT2DB_JAVA_ENGINE_JAR";
const VAULT_MASTER_KEY_ENV: &str = "CHAT2DB_VAULT_MASTER_KEY";

struct DesktopState {
    application: Application,
    local_server: Mutex<Option<LocalServer>>,
    runtime_host: Mutex<Option<RuntimeHost>>,
    subscriptions: SubscriptionRegistry,
    next_subscription_id: AtomicU64,
}

#[derive(Default)]
struct SubscriptionRegistry {
    controls: Mutex<HashMap<String, SubscriptionControl>>,
}

struct SubscriptionControl {
    stop: oneshot::Sender<()>,
    finished: oneshot::Receiver<()>,
}

impl SubscriptionRegistry {
    async fn insert(
        &self,
        subscription_id: String,
        stop: oneshot::Sender<()>,
        finished: oneshot::Receiver<()>,
    ) {
        let previous = self
            .controls
            .lock()
            .await
            .insert(subscription_id, SubscriptionControl { stop, finished });
        debug_assert!(previous.is_none(), "subscription ids must be unique");
        if let Some(previous) = previous {
            let _ = previous.stop.send(());
        }
    }

    async fn remove(&self, subscription_id: &str) {
        self.controls.lock().await.remove(subscription_id);
    }

    async fn unsubscribe(&self, subscription_id: &str) -> bool {
        let control = self.controls.lock().await.remove(subscription_id);
        control.is_some_and(|control| control.stop.send(()).is_ok())
    }

    async fn release_all(&self) {
        let controls = self
            .controls
            .lock()
            .await
            .drain()
            .map(|(_, control)| control)
            .collect::<Vec<_>>();
        let mut completions = Vec::with_capacity(controls.len());
        for control in controls {
            let _ = control.stop.send(());
            completions.push(control.finished);
        }
        for completion in completions {
            let _ = completion.await;
        }
    }

    #[cfg(test)]
    async fn active_count(&self) -> usize {
        self.controls.lock().await.len()
    }
}

impl DesktopState {
    async fn open_from_environment() -> Result<Self, DesktopError> {
        let runtime_config = runtime_config_from_environment()?;
        let mut runtime_host = RuntimeHost::open(runtime_config)
            .await
            .map_err(DesktopError::runtime)?;
        let application = runtime_host.application();
        let local_server = match LocalServer::start(application.clone()) {
            Ok(server) => server,
            Err(error) => {
                if let Err(shutdown_error) = runtime_host.shutdown().await {
                    tracing::error!(%shutdown_error, "runtime cleanup failed after local attachment startup error");
                }
                return Err(DesktopError::local(error));
            }
        };
        Ok(Self {
            application,
            local_server: Mutex::new(Some(local_server)),
            runtime_host: Mutex::new(Some(runtime_host)),
            subscriptions: SubscriptionRegistry::default(),
            next_subscription_id: AtomicU64::new(1),
        })
    }

    async fn shutdown(&self) -> Result<(), DesktopError> {
        self.subscriptions.release_all().await;
        let local_server = self.local_server.lock().await.take();
        let local_result = match local_server {
            Some(mut local_server) => local_server.shutdown().await.map_err(DesktopError::local),
            None => Ok(()),
        };
        let runtime_host = self.runtime_host.lock().await.take();
        let runtime_result = match runtime_host {
            Some(mut runtime_host) => runtime_host.shutdown().await.map_err(DesktopError::runtime),
            None => Ok(()),
        };
        if let Err(error) = local_result {
            if let Err(runtime_error) = runtime_result {
                tracing::error!(%runtime_error, "runtime cleanup also failed after local attachment shutdown error");
            }
            return Err(error);
        }
        runtime_result
    }
}

/// Startup or shutdown failure for the desktop host.
#[derive(Debug)]
pub enum DesktopError {
    MissingJavaEngineJar,
    EmptyEnvironmentVariable(&'static str),
    InvalidJavaEngineJar(PathBuf),
    JavaEngineJarMetadata {
        path: PathBuf,
        source: std::io::Error,
    },
    InvalidVaultMasterKeyEncoding,
    CommunityClasspath(Box<BridgeError>),
    Local(Box<LocalError>),
    Runtime(Box<AppError>),
    Tauri(Box<tauri::Error>),
}

impl DesktopError {
    fn runtime(error: AppError) -> Self {
        Self::Runtime(Box::new(error))
    }

    fn local(error: LocalError) -> Self {
        Self::Local(Box::new(error))
    }

    fn tauri(error: tauri::Error) -> Self {
        Self::Tauri(Box::new(error))
    }
}

impl std::fmt::Display for DesktopError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingJavaEngineJar => write!(
                formatter,
                "{JAVA_ENGINE_JAR_ENV} is required and must point to the compatibility-engine JAR"
            ),
            Self::EmptyEnvironmentVariable(name) => {
                write!(formatter, "{name} must not be empty when configured")
            }
            Self::InvalidJavaEngineJar(path) => write!(
                formatter,
                "{JAVA_ENGINE_JAR_ENV} does not point to a regular file: {}",
                path.display()
            ),
            Self::JavaEngineJarMetadata { path, source } => write!(
                formatter,
                "unable to inspect {JAVA_ENGINE_JAR_ENV} at {}: {source}",
                path.display()
            ),
            Self::InvalidVaultMasterKeyEncoding => write!(
                formatter,
                "{VAULT_MASTER_KEY_ENV} must be UTF-8 standard base64 for exactly 32 bytes"
            ),
            Self::CommunityClasspath(error) => {
                write!(
                    formatter,
                    "fixed Community classpath failed validation: {error}"
                )
            }
            Self::Local(error) => write!(formatter, "local attachment failed: {error}"),
            Self::Runtime(error) => write!(formatter, "Chat2DB runtime failed: {error}"),
            Self::Tauri(error) => write!(formatter, "Tauri desktop failed: {error}"),
        }
    }
}

impl std::error::Error for DesktopError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::JavaEngineJarMetadata { source, .. } => Some(source),
            Self::CommunityClasspath(error) => Some(error.as_ref()),
            Self::Local(error) => Some(error.as_ref()),
            Self::Runtime(error) => Some(error.as_ref()),
            Self::Tauri(error) => Some(error.as_ref()),
            Self::MissingJavaEngineJar
            | Self::EmptyEnvironmentVariable(_)
            | Self::InvalidJavaEngineJar(_)
            | Self::InvalidVaultMasterKeyEncoding => None,
        }
    }
}

/// Runs the desktop event loop and gracefully shuts down its Java generation.
///
/// # Errors
///
/// Fails closed when the vault, storage, Java engine, or Tauri runtime cannot
/// initialize, or when the owned Java generation cannot shut down cleanly.
pub fn run() -> Result<i32, DesktopError> {
    let state = Arc::new(tauri::async_runtime::block_on(
        DesktopState::open_from_environment(),
    )?);
    let managed_state = Arc::clone(&state);
    let application = tauri::Builder::default()
        .manage(managed_state)
        .invoke_handler(tauri::generate_handler![
            health,
            list_drivers,
            list_community_plugins,
            list_community_schemas,
            list_community_databases,
            list_community_tables,
            list_community_columns,
            list_community_indexes,
            list_community_views,
            list_community_imported_keys,
            list_community_exported_keys,
            list_community_primary_keys,
            list_community_functions,
            get_community_function,
            list_community_function_parameters,
            list_community_procedures,
            get_community_procedure,
            list_community_procedure_parameters,
            list_community_triggers,
            get_community_trigger,
            build_community_create_schema,
            build_community_namespace_sql,
            build_community_dml,
            start_community_table_preview,
            parse_community_sql,
            validate_community_sql,
            format_community_sql,
            complete_community_sql,
            list_datasources,
            create_datasource,
            get_datasource,
            update_datasource,
            delete_datasource,
            list_provider_profiles,
            create_provider_profile,
            get_provider_profile,
            update_provider_profile,
            delete_provider_profile,
            list_agent_sessions,
            create_agent_session,
            get_agent_session,
            update_agent_session,
            delete_agent_session,
            list_agent_messages,
            start_agent_run,
            agent_run_snapshot,
            cancel_agent_run,
            decide_agent_permission,
            subscribe_agent_run,
            unsubscribe_agent_run,
            start_query,
            operation_snapshot,
            cancel_operation,
            subscribe_operation,
            unsubscribe_operation,
            result_page,
        ])
        .build(tauri::generate_context!());
    let application = match application {
        Ok(application) => application,
        Err(error) => {
            tauri::async_runtime::block_on(state.shutdown())?;
            return Err(DesktopError::tauri(error));
        }
    };

    let exit_code = application.run_return(|_, _| {});
    tauri::async_runtime::block_on(state.shutdown())?;
    Ok(exit_code)
}

fn runtime_config_from_environment() -> Result<RuntimeConfig, DesktopError> {
    let engine_jar = required_java_engine_jar()?;
    let java = optional_nonempty_os_env(JAVA_BIN_ENV)?.unwrap_or_else(|| OsString::from("java"));
    let mut engine = EngineConfig::new(EngineCommand::java_jar(java, engine_jar));
    if let Some(community_classpath_dir) = optional_nonempty_os_env(COMMUNITY_CLASSPATH_DIR_ENV)? {
        let classpath = load_fixed_community_classpath(PathBuf::from(community_classpath_dir))
            .map_err(|error| DesktopError::CommunityClasspath(Box::new(error)))?;
        engine = engine.with_community_classpath(classpath);
    }
    let mut config = RuntimeConfig::new(engine);

    if let Some(data_dir) = optional_nonempty_os_env(DATA_DIR_ENV)? {
        config = config.with_data_dir(PathBuf::from(data_dir));
    }
    if let Some(driver_pack_dir) = optional_nonempty_os_env(DRIVER_PACK_DIR_ENV)? {
        config = config.with_driver_pack_dir(PathBuf::from(driver_pack_dir));
    }
    match env::var(VAULT_MASTER_KEY_ENV) {
        Ok(master_key) => config = config.with_vault_master_key_base64(master_key),
        Err(env::VarError::NotPresent) => {}
        Err(env::VarError::NotUnicode(_)) => {
            return Err(DesktopError::InvalidVaultMasterKeyEncoding);
        }
    }
    Ok(config)
}

fn required_java_engine_jar() -> Result<PathBuf, DesktopError> {
    let path = optional_nonempty_os_env(JAVA_ENGINE_JAR_ENV)?
        .map(PathBuf::from)
        .ok_or(DesktopError::MissingJavaEngineJar)?;
    validate_java_engine_jar(&path)?;
    Ok(path)
}

fn optional_nonempty_os_env(name: &'static str) -> Result<Option<OsString>, DesktopError> {
    validate_optional_os_env(name, env::var_os(name))
}

fn validate_optional_os_env(
    name: &'static str,
    value: Option<OsString>,
) -> Result<Option<OsString>, DesktopError> {
    match value {
        Some(value) if value.is_empty() => Err(DesktopError::EmptyEnvironmentVariable(name)),
        value => Ok(value),
    }
}

fn validate_java_engine_jar(path: &Path) -> Result<(), DesktopError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(DesktopError::InvalidJavaEngineJar(path.to_path_buf())),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Err(DesktopError::InvalidJavaEngineJar(path.to_path_buf()))
        }
        Err(source) => Err(DesktopError::JavaEngineJarMetadata {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn api_error(error: &AppError) -> ApiError {
    error.api_error()
}

fn parse_after_sequence(value: Option<String>) -> Result<Option<u64>, Box<ApiError>> {
    value
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                Box::new(ApiError::new(
                    "invalid_last_event_id",
                    "Last-Event-ID must be an unsigned decimal integer",
                ))
            })
        })
        .transpose()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn health(state: State<'_, Arc<DesktopState>>) -> HealthResponse {
    state.application.health()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn list_drivers(state: State<'_, Arc<DesktopState>>) -> JdbcDriverList {
    state.application.list_drivers()
}

#[tauri::command]
async fn list_community_plugins(
    state: State<'_, Arc<DesktopState>>,
) -> Result<CommunityPluginCatalog, ApiError> {
    state
        .application
        .list_community_plugins()
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_community_schemas(
    state: State<'_, Arc<DesktopState>>,
    request: ListCommunitySchemasRequest,
) -> Result<CommunitySchemaList, ApiError> {
    state
        .application
        .list_community_schemas(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_community_databases(
    state: State<'_, Arc<DesktopState>>,
    request: ListCommunityDatabasesRequest,
) -> Result<CommunityDatabaseList, ApiError> {
    state
        .application
        .list_community_databases(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_community_tables(
    state: State<'_, Arc<DesktopState>>,
    request: ListCommunityTablesRequest,
) -> Result<CommunityTableList, ApiError> {
    state
        .application
        .list_community_tables(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_community_columns(
    state: State<'_, Arc<DesktopState>>,
    request: ListCommunityColumnsRequest,
) -> Result<CommunityTableColumnList, ApiError> {
    state
        .application
        .list_community_columns(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_community_indexes(
    state: State<'_, Arc<DesktopState>>,
    request: ListCommunityIndexesRequest,
) -> Result<CommunityTableIndexList, ApiError> {
    state
        .application
        .list_community_indexes(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_community_views(
    state: State<'_, Arc<DesktopState>>,
    request: ListCommunityViewsRequest,
) -> Result<CommunityViewList, ApiError> {
    state
        .application
        .list_community_views(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_community_imported_keys(
    state: State<'_, Arc<DesktopState>>,
    request: ListCommunityTableKeysRequest,
) -> Result<CommunityForeignKeyList, ApiError> {
    state
        .application
        .list_community_imported_keys(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_community_exported_keys(
    state: State<'_, Arc<DesktopState>>,
    request: ListCommunityTableKeysRequest,
) -> Result<CommunityForeignKeyList, ApiError> {
    state
        .application
        .list_community_exported_keys(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_community_primary_keys(
    state: State<'_, Arc<DesktopState>>,
    request: ListCommunityTableKeysRequest,
) -> Result<CommunityPrimaryKeyList, ApiError> {
    state
        .application
        .list_community_primary_keys(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_community_functions(
    state: State<'_, Arc<DesktopState>>,
    request: ListCommunityFunctionsRequest,
) -> Result<CommunityFunctionList, ApiError> {
    state
        .application
        .list_community_functions(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn get_community_function(
    state: State<'_, Arc<DesktopState>>,
    request: GetCommunityFunctionRequest,
) -> Result<CommunityFunction, ApiError> {
    state
        .application
        .get_community_function(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_community_function_parameters(
    state: State<'_, Arc<DesktopState>>,
    request: GetCommunityFunctionRequest,
) -> Result<CommunityFunctionParameterList, ApiError> {
    state
        .application
        .list_community_function_parameters(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_community_procedures(
    state: State<'_, Arc<DesktopState>>,
    request: ListCommunityProceduresRequest,
) -> Result<CommunityProcedureList, ApiError> {
    state
        .application
        .list_community_procedures(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn get_community_procedure(
    state: State<'_, Arc<DesktopState>>,
    request: GetCommunityProcedureRequest,
) -> Result<CommunityProcedure, ApiError> {
    state
        .application
        .get_community_procedure(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_community_procedure_parameters(
    state: State<'_, Arc<DesktopState>>,
    request: GetCommunityProcedureRequest,
) -> Result<CommunityProcedureParameterList, ApiError> {
    state
        .application
        .list_community_procedure_parameters(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_community_triggers(
    state: State<'_, Arc<DesktopState>>,
    request: ListCommunityTriggersRequest,
) -> Result<CommunityTriggerList, ApiError> {
    state
        .application
        .list_community_triggers(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn get_community_trigger(
    state: State<'_, Arc<DesktopState>>,
    request: GetCommunityTriggerRequest,
) -> Result<CommunityTrigger, ApiError> {
    state
        .application
        .get_community_trigger(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn build_community_create_schema(
    state: State<'_, Arc<DesktopState>>,
    request: BuildCommunityCreateSchemaRequest,
) -> Result<CommunityBuiltSql, ApiError> {
    state
        .application
        .build_community_create_schema(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn build_community_namespace_sql(
    state: State<'_, Arc<DesktopState>>,
    request: BuildCommunityNamespaceSqlRequest,
) -> Result<CommunityBuiltSql, ApiError> {
    build_community_namespace_sql_for(&state.application, request).await
}

async fn build_community_namespace_sql_for(
    application: &Application,
    request: BuildCommunityNamespaceSqlRequest,
) -> Result<CommunityBuiltSql, ApiError> {
    application
        .build_community_namespace_sql(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn build_community_dml(
    state: State<'_, Arc<DesktopState>>,
    request: BuildCommunityDmlRequest,
) -> Result<CommunityBuiltSql, ApiError> {
    build_community_dml_for(&state.application, request).await
}

async fn build_community_dml_for(
    application: &Application,
    request: BuildCommunityDmlRequest,
) -> Result<CommunityBuiltSql, ApiError> {
    application
        .build_community_dml(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn start_community_table_preview(
    state: State<'_, Arc<DesktopState>>,
    request: StartCommunityTablePreviewRequest,
) -> Result<CommunityTablePreviewAccepted, ApiError> {
    start_community_table_preview_for(&state.application, request).await
}

async fn start_community_table_preview_for(
    application: &Application,
    request: StartCommunityTablePreviewRequest,
) -> Result<CommunityTablePreviewAccepted, ApiError> {
    application
        .start_community_table_preview(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn parse_community_sql(
    state: State<'_, Arc<DesktopState>>,
    request: ParseCommunitySqlRequest,
) -> Result<CommunitySqlAnalysis, ApiError> {
    state
        .application
        .parse_community_sql(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn validate_community_sql(
    state: State<'_, Arc<DesktopState>>,
    request: ValidateCommunitySqlRequest,
) -> Result<CommunitySqlValidation, ApiError> {
    validate_community_sql_for(&state.application, request).await
}

async fn validate_community_sql_for(
    application: &Application,
    request: ValidateCommunitySqlRequest,
) -> Result<CommunitySqlValidation, ApiError> {
    application
        .validate_community_sql(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn format_community_sql(
    state: State<'_, Arc<DesktopState>>,
    request: FormatCommunitySqlRequest,
) -> Result<CommunityFormattedSql, ApiError> {
    format_community_sql_for(&state.application, request).await
}

async fn format_community_sql_for(
    application: &Application,
    request: FormatCommunitySqlRequest,
) -> Result<CommunityFormattedSql, ApiError> {
    application
        .format_community_sql(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn complete_community_sql(
    state: State<'_, Arc<DesktopState>>,
    request: CompleteCommunitySqlRequest,
) -> Result<CommunitySqlCompletion, ApiError> {
    complete_community_sql_for(&state.application, request).await
}

async fn complete_community_sql_for(
    application: &Application,
    request: CompleteCommunitySqlRequest,
) -> Result<CommunitySqlCompletion, ApiError> {
    application
        .complete_community_sql(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_datasources(state: State<'_, Arc<DesktopState>>) -> Result<DatasourceList, ApiError> {
    state
        .application
        .list_datasources()
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn create_datasource(
    state: State<'_, Arc<DesktopState>>,
    request: CreateDatasourceRequest,
) -> Result<Datasource, ApiError> {
    state
        .application
        .create_datasource(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn get_datasource(
    state: State<'_, Arc<DesktopState>>,
    datasource_id: String,
) -> Result<Datasource, ApiError> {
    state
        .application
        .get_datasource(&datasource_id)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn update_datasource(
    state: State<'_, Arc<DesktopState>>,
    datasource_id: String,
    request: UpdateDatasourceRequest,
) -> Result<Datasource, ApiError> {
    state
        .application
        .update_datasource(&datasource_id, request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn delete_datasource(
    state: State<'_, Arc<DesktopState>>,
    datasource_id: String,
    expected_revision: String,
) -> Result<(), ApiError> {
    state
        .application
        .delete_datasource(&datasource_id, &expected_revision)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_provider_profiles(
    state: State<'_, Arc<DesktopState>>,
) -> Result<ProviderProfileList, ApiError> {
    state
        .application
        .list_provider_profiles()
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn create_provider_profile(
    state: State<'_, Arc<DesktopState>>,
    request: CreateProviderProfileRequest,
) -> Result<ProviderProfile, ApiError> {
    state
        .application
        .create_provider_profile(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn get_provider_profile(
    state: State<'_, Arc<DesktopState>>,
    provider_id: String,
) -> Result<ProviderProfile, ApiError> {
    state
        .application
        .get_provider_profile(&provider_id)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn update_provider_profile(
    state: State<'_, Arc<DesktopState>>,
    provider_id: String,
    request: UpdateProviderProfileRequest,
) -> Result<ProviderProfile, ApiError> {
    state
        .application
        .update_provider_profile(&provider_id, request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn delete_provider_profile(
    state: State<'_, Arc<DesktopState>>,
    provider_id: String,
    expected_revision: String,
) -> Result<(), ApiError> {
    state
        .application
        .delete_provider_profile(&provider_id, &expected_revision)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_agent_sessions(
    state: State<'_, Arc<DesktopState>>,
) -> Result<AgentSessionList, ApiError> {
    state
        .application
        .list_agent_sessions()
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn create_agent_session(
    state: State<'_, Arc<DesktopState>>,
    request: CreateAgentSessionRequest,
) -> Result<AgentSession, ApiError> {
    state
        .application
        .create_agent_session(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn get_agent_session(
    state: State<'_, Arc<DesktopState>>,
    session_id: String,
) -> Result<AgentSession, ApiError> {
    state
        .application
        .get_agent_session(&session_id)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn update_agent_session(
    state: State<'_, Arc<DesktopState>>,
    session_id: String,
    request: UpdateAgentSessionRequest,
) -> Result<AgentSession, ApiError> {
    state
        .application
        .update_agent_session(&session_id, request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn delete_agent_session(
    state: State<'_, Arc<DesktopState>>,
    session_id: String,
    expected_revision: String,
) -> Result<(), ApiError> {
    state
        .application
        .delete_agent_session(&session_id, &expected_revision)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn list_agent_messages(
    state: State<'_, Arc<DesktopState>>,
    session_id: String,
    start_ordinal: String,
    limit: String,
) -> Result<AgentMessageList, ApiError> {
    state
        .application
        .list_agent_messages(&session_id, &start_ordinal, &limit)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn start_agent_run(
    state: State<'_, Arc<DesktopState>>,
    request: StartAgentRunRequest,
) -> Result<AgentRunAccepted, ApiError> {
    state
        .application
        .start_agent_run(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn agent_run_snapshot(
    state: State<'_, Arc<DesktopState>>,
    run_id: String,
) -> Result<AgentRunSnapshot, ApiError> {
    state
        .application
        .agent_run_snapshot(&run_id)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn cancel_agent_run(
    state: State<'_, Arc<DesktopState>>,
    run_id: String,
) -> Result<CancelAgentRunResponse, ApiError> {
    state
        .application
        .cancel_agent_run(&run_id)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn decide_agent_permission(
    state: State<'_, Arc<DesktopState>>,
    permission_id: String,
    request: DecideAgentPermissionRequest,
) -> Result<AgentPermissionResponse, ApiError> {
    state
        .application
        .decide_agent_permission(&permission_id, request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn subscribe_agent_run(
    state: State<'_, Arc<DesktopState>>,
    run_id: String,
    after_sequence: Option<String>,
    on_event: Channel<AgentStreamMessage>,
) -> Result<AgentSubscriptionAccepted, ApiError> {
    let after_sequence = parse_after_sequence(after_sequence).map_err(|error| *error)?;
    let subscription = state
        .application
        .subscribe_agent_run(&run_id, after_sequence)
        .await
        .map_err(|error| api_error(&error))?;
    let state = Arc::clone(state.inner());
    let subscription_id = state
        .next_subscription_id
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map(|id| format!("subscription-{id}"))
        .map_err(|_| {
            ApiError::new(
                "subscription_capacity_exhausted",
                "No subscription ids remain",
            )
        })?;
    let (stop, stopped) = oneshot::channel();
    let (finished, completion) = oneshot::channel();
    state
        .subscriptions
        .insert(subscription_id.clone(), stop, completion)
        .await;
    let task_subscription_id = subscription_id.clone();
    tauri::async_runtime::spawn(async move {
        forward_agent_subscription(
            state,
            task_subscription_id,
            subscription,
            stopped,
            finished,
            on_event,
        )
        .await;
    });
    Ok(AgentSubscriptionAccepted { subscription_id })
}

#[tauri::command]
async fn unsubscribe_agent_run(
    state: State<'_, Arc<DesktopState>>,
    subscription_id: String,
) -> Result<(), ApiError> {
    state.subscriptions.unsubscribe(&subscription_id).await;
    Ok(())
}

async fn forward_agent_subscription(
    state: Arc<DesktopState>,
    subscription_id: String,
    mut subscription: chat2db_core::AgentRunSubscription,
    mut stopped: oneshot::Receiver<()>,
    finished: oneshot::Sender<()>,
    on_event: Channel<AgentStreamMessage>,
) {
    loop {
        let next = tokio::select! {
            biased;
            _ = &mut stopped => break,
            next = subscription.next_event() => next,
        };
        let (message, finished) = agent_stream_message(next);
        if on_event.send(message).is_err() || finished {
            break;
        }
    }
    state.subscriptions.remove(&subscription_id).await;
    let _ = finished.send(());
}

fn agent_stream_message(
    next: Result<Option<AgentEventEnvelope>, AppError>,
) -> (AgentStreamMessage, bool) {
    match next {
        Ok(Some(event)) => (AgentStreamMessage::Event { event }, false),
        Ok(None) => (AgentStreamMessage::End, true),
        Err(error) => (
            AgentStreamMessage::Error {
                error: error.api_error(),
            },
            true,
        ),
    }
}

#[tauri::command]
async fn start_query(
    state: State<'_, Arc<DesktopState>>,
    request: StartQueryRequest,
) -> Result<QueryAccepted, ApiError> {
    state
        .application
        .start_query(request)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn operation_snapshot(
    state: State<'_, Arc<DesktopState>>,
    operation_id: String,
) -> Result<OperationSnapshot, ApiError> {
    state
        .application
        .operation_snapshot(&operation_id)
        .await
        .map_err(|error| api_error(&error))
}

#[tauri::command]
async fn cancel_operation(
    state: State<'_, Arc<DesktopState>>,
    operation_id: String,
) -> Result<CancelOperationResponse, ApiError> {
    Ok(state.application.cancel_operation(&operation_id).await)
}

#[tauri::command]
async fn subscribe_operation(
    state: State<'_, Arc<DesktopState>>,
    operation_id: String,
    after_sequence: Option<String>,
    on_event: Channel<OperationStreamMessage>,
) -> Result<OperationSubscriptionAccepted, ApiError> {
    let after_sequence = parse_after_sequence(after_sequence).map_err(|error| *error)?;
    let subscription = state
        .application
        .subscribe_operation(&operation_id, after_sequence)
        .await
        .map_err(|error| api_error(&error))?;
    let state = Arc::clone(state.inner());
    let subscription_id = state
        .next_subscription_id
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map(|id| format!("subscription-{id}"))
        .map_err(|_| {
            ApiError::new(
                "subscription_capacity_exhausted",
                "No subscription ids remain",
            )
        })?;
    let (stop, stopped) = oneshot::channel();
    let (finished, completion) = oneshot::channel();
    state
        .subscriptions
        .insert(subscription_id.clone(), stop, completion)
        .await;
    let task_subscription_id = subscription_id.clone();
    tauri::async_runtime::spawn(async move {
        forward_operation_subscription(
            state,
            task_subscription_id,
            subscription,
            stopped,
            finished,
            on_event,
        )
        .await;
    });
    Ok(OperationSubscriptionAccepted { subscription_id })
}

#[tauri::command]
async fn unsubscribe_operation(
    state: State<'_, Arc<DesktopState>>,
    subscription_id: String,
) -> Result<(), ApiError> {
    state.subscriptions.unsubscribe(&subscription_id).await;
    Ok(())
}

async fn forward_operation_subscription(
    state: Arc<DesktopState>,
    subscription_id: String,
    mut subscription: chat2db_core::OperationSubscription,
    mut stopped: oneshot::Receiver<()>,
    finished: oneshot::Sender<()>,
    on_event: Channel<OperationStreamMessage>,
) {
    loop {
        let next = tokio::select! {
            biased;
            _ = &mut stopped => break,
            next = subscription.next_event() => next,
        };
        let (message, finished) = operation_stream_message(next);
        if on_event.send(message).is_err() || finished {
            break;
        }
    }
    state.subscriptions.remove(&subscription_id).await;
    let _ = finished.send(());
}

fn operation_stream_message(
    next: Result<Option<OperationEventEnvelope>, AppError>,
) -> (OperationStreamMessage, bool) {
    match next {
        Ok(Some(event)) => (OperationStreamMessage::Event { event }, false),
        Ok(None) => (OperationStreamMessage::End, true),
        Err(error) => (
            OperationStreamMessage::Error {
                error: error.api_error(),
            },
            true,
        ),
    }
}

#[tauri::command]
async fn result_page(
    state: State<'_, Arc<DesktopState>>,
    result_id: String,
    request: ResultPageRequest,
) -> Result<ResultPage, ApiError> {
    state
        .application
        .result_page(&result_id, request)
        .await
        .map_err(|error| api_error(&error))
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, fs::File, sync::Arc};

    use chat2db_contract::{
        AgentEvent, AgentEventEnvelope, AgentStreamMessage, BuildCommunityDmlRequest,
        BuildCommunityNamespaceSqlRequest, CommunityDmlColumn, CommunityDmlRow,
        CommunityDmlStatement, CommunityDmlTarget, CommunityDmlValue,
        CommunityNamespaceSqlOperation, CompleteCommunitySqlRequest, FormatCommunitySqlRequest,
        OperationEvent, OperationEventEnvelope, OperationStreamMessage,
        StartCommunityTablePreviewRequest, ValidateCommunitySqlRequest,
    };
    use chat2db_core::{AppError, Application};
    use tokio::sync::oneshot;

    use super::{
        DesktopError, SubscriptionRegistry, agent_stream_message, build_community_dml_for,
        build_community_namespace_sql_for, complete_community_sql_for, format_community_sql_for,
        operation_stream_message, parse_after_sequence, start_community_table_preview_for,
        validate_community_sql_for, validate_java_engine_jar, validate_optional_os_env,
    };

    #[tokio::test]
    async fn namespace_builder_command_maps_unavailable_engine_errors() {
        let error = build_community_namespace_sql_for(
            &Application::new(),
            BuildCommunityNamespaceSqlRequest {
                database_type: "H2".to_owned(),
                operation: CommunityNamespaceSqlOperation::DropSchema {
                    schema_name: "APP".to_owned(),
                },
            },
        )
        .await
        .expect_err("namespace generation without an engine must fail");

        assert_eq!(error.code, "database_engine_unavailable");
    }

    #[tokio::test]
    async fn dml_builder_command_maps_unavailable_engine_errors() {
        let error = build_community_dml_for(
            &Application::new(),
            BuildCommunityDmlRequest {
                database_type: "H2".to_owned(),
                target: CommunityDmlTarget {
                    database_name: None,
                    schema_name: Some("APP".to_owned()),
                    table_name: "items".to_owned(),
                },
                statement: CommunityDmlStatement::SingleInsert {
                    columns: vec![CommunityDmlColumn {
                        name: "label".to_owned(),
                        data_type_name: "VARCHAR".to_owned(),
                        precision: Some(255),
                        scale: None,
                    }],
                    row: CommunityDmlRow {
                        values: vec![CommunityDmlValue::String {
                            value: "O'Brien".to_owned(),
                        }],
                    },
                },
            },
        )
        .await
        .expect_err("DML generation without an engine must fail");

        assert_eq!(error.code, "database_engine_unavailable");
    }

    #[tokio::test]
    async fn table_preview_command_maps_unavailable_engine_errors() {
        let error = start_community_table_preview_for(
            &Application::new(),
            StartCommunityTablePreviewRequest {
                datasource_id: "datasource-1".to_owned(),
                database_type: "H2".to_owned(),
                database_name: String::new(),
                schema_name: "APP".to_owned(),
                table_name: "items".to_owned(),
                row_limit: Some(200),
            },
        )
        .await
        .expect_err("table preview without an engine must fail");

        assert_eq!(error.code, "database_engine_unavailable");
    }

    #[tokio::test]
    async fn sql_validation_command_maps_unavailable_engine_errors() {
        let error = validate_community_sql_for(
            &Application::new(),
            ValidateCommunitySqlRequest {
                database_type: "H2".to_owned(),
                sql: "select from".to_owned(),
            },
        )
        .await
        .expect_err("validation without an engine must fail");

        assert_eq!(error.code, "database_engine_unavailable");
    }

    #[tokio::test]
    async fn sql_formatter_command_maps_unavailable_engine_errors() {
        let error = format_community_sql_for(
            &Application::new(),
            FormatCommunitySqlRequest {
                database_type: "H2".to_owned(),
                sql: "select 1".to_owned(),
            },
        )
        .await
        .expect_err("formatting without an engine must fail");

        assert_eq!(error.code, "database_engine_unavailable");
    }

    #[tokio::test]
    async fn sql_completion_command_maps_unavailable_storage_errors() {
        let error = complete_community_sql_for(
            &Application::new(),
            CompleteCommunitySqlRequest {
                datasource_id: "datasource-1".to_owned(),
                database_type: "H2".to_owned(),
                database_name: "inventory".to_owned(),
                schema_name: "PUBLIC".to_owned(),
                sql: "select * from ".to_owned(),
                cursor_utf16: 14,
                min_prefix_length: 0,
                need_full_name: false,
                keyword_case: "UPPER".to_owned(),
                active_snippet_slot: None,
            },
        )
        .await
        .expect_err("completion without storage must fail");

        assert_eq!(error.code, "storage_unavailable");
    }

    #[test]
    fn operation_sequence_matches_web_transport_validation() {
        assert_eq!(
            parse_after_sequence(Some("9007199254740993".to_owned())).expect("sequence must parse"),
            Some(9_007_199_254_740_993)
        );

        let error = parse_after_sequence(Some("invalid".to_owned()))
            .expect_err("invalid sequence must fail");
        assert_eq!(error.code, "invalid_last_event_id");
    }

    #[test]
    fn java_engine_path_must_be_a_regular_file() {
        let directory = tempfile::tempdir().expect("temporary directory");
        assert!(matches!(
            validate_java_engine_jar(directory.path()),
            Err(DesktopError::InvalidJavaEngineJar(_))
        ));

        let jar = directory.path().join("engine.jar");
        File::create(&jar).expect("engine fixture");
        validate_java_engine_jar(&jar).expect("regular file must pass");

        assert!(matches!(
            validate_java_engine_jar(&directory.path().join("missing-engine.jar")),
            Err(DesktopError::InvalidJavaEngineJar(_))
        ));
    }

    #[test]
    fn optional_path_environment_rejects_explicit_empty_values() {
        assert!(matches!(
            validate_optional_os_env("CHAT2DB_DRIVER_PACK_DIR", Some(OsString::new())),
            Err(DesktopError::EmptyEnvironmentVariable(
                "CHAT2DB_DRIVER_PACK_DIR"
            ))
        ));
        assert_eq!(
            validate_optional_os_env("CHAT2DB_DRIVER_PACK_DIR", None)
                .expect("missing optional variable must be accepted"),
            None
        );
    }

    #[test]
    fn stream_result_maps_events_errors_and_clean_end() {
        let event = OperationEventEnvelope {
            operation_id: "operation-1".to_owned(),
            sequence: "1".to_owned(),
            occurred_at_ms: "1784900000000".to_owned(),
            event: OperationEvent::Started,
        };
        assert_eq!(
            operation_stream_message(Ok(Some(event.clone()))),
            (OperationStreamMessage::Event { event }, false)
        );
        assert_eq!(
            operation_stream_message(Ok(None)),
            (OperationStreamMessage::End, true)
        );

        let (message, finished) = operation_stream_message(Err(AppError::invalid(
            "operation_replay_window_expired",
            "The requested operation event is no longer retained",
        )));
        assert!(finished);
        assert!(matches!(
            message,
            OperationStreamMessage::Error { error }
                if error.code == "operation_replay_window_expired"
        ));
    }

    #[test]
    fn agent_stream_result_maps_events_errors_and_clean_end() {
        let event = AgentEventEnvelope {
            run_id: "run-1".to_owned(),
            sequence: "1".to_owned(),
            occurred_at_ms: "1784900000000".to_owned(),
            event: AgentEvent::Started,
        };
        assert_eq!(
            agent_stream_message(Ok(Some(event.clone()))),
            (AgentStreamMessage::Event { event }, false)
        );
        assert_eq!(
            agent_stream_message(Ok(None)),
            (AgentStreamMessage::End, true)
        );

        let (message, finished) = agent_stream_message(Err(AppError::invalid(
            "agent_replay_window_expired",
            "The requested agent event is no longer retained",
        )));
        assert!(finished);
        assert!(matches!(
            message,
            AgentStreamMessage::Error { error }
                if error.code == "agent_replay_window_expired"
        ));
    }

    #[tokio::test]
    async fn unsubscribe_releases_only_the_registered_observer() {
        let registry = Arc::new(SubscriptionRegistry::default());
        let (stop, stopped) = oneshot::channel();
        let (finished, completion) = oneshot::channel();
        registry
            .insert("subscription-1".to_owned(), stop, completion)
            .await;
        let task_registry = Arc::clone(&registry);
        let forwarder = tokio::spawn(async move {
            stopped
                .await
                .expect("unsubscribe must signal the forwarder");
            task_registry.remove("subscription-1").await;
            let _ = finished.send(());
        });

        assert_eq!(registry.active_count().await, 1);
        assert!(registry.unsubscribe("subscription-1").await);
        forwarder.await.expect("forwarder task");
        assert_eq!(registry.active_count().await, 0);
        assert!(!registry.unsubscribe("subscription-1").await);
    }

    #[tokio::test]
    async fn shutdown_releases_every_registered_observer() {
        let registry = SubscriptionRegistry::default();
        let (first_stop, first_stopped) = oneshot::channel();
        let (second_stop, second_stopped) = oneshot::channel();
        let (first_finished, first_completion) = oneshot::channel();
        let (second_finished, second_completion) = oneshot::channel();
        registry
            .insert("subscription-1".to_owned(), first_stop, first_completion)
            .await;
        registry
            .insert("subscription-2".to_owned(), second_stop, second_completion)
            .await;
        let first = tokio::spawn(async move {
            first_stopped.await.expect("first observer must stop");
            let _ = first_finished.send(());
        });
        let second = tokio::spawn(async move {
            second_stopped.await.expect("second observer must stop");
            let _ = second_finished.send(());
        });

        registry.release_all().await;

        first.await.expect("first forwarder");
        second.await.expect("second forwarder");
        assert_eq!(registry.active_count().await, 0);
    }
}
