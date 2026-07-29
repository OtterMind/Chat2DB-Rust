//! Tauri IPC delivery adapter for the `Chat2DB` desktop product.

use std::{
    collections::HashMap,
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use chat2db_contract::{
    AgentEventEnvelope, AgentMessageList, AgentPermissionResponse, AgentRunAccepted,
    AgentRunSnapshot, AgentSession, AgentSessionList, AgentStreamMessage,
    AgentSubscriptionAccepted, ApiError, BuildCommunityCreateSchemaRequest,
    BuildCommunityDmlRequest, BuildCommunityNamespaceSqlRequest, CancelAgentRunResponse,
    CancelDisposition, CancelOperationResponse, CommunityBuiltSql, CommunityDatabaseList,
    CommunityForeignKeyList, CommunityFormattedSql, CommunityFunction, CommunityFunctionList,
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
    OperationEvent, OperationEventEnvelope, OperationSnapshot, OperationStreamMessage,
    OperationSubscriptionAccepted, ParseCommunitySqlRequest, ProviderProfile, ProviderProfileList,
    QueryAccepted, ResultPage, ResultPageRequest, StartAgentRunRequest,
    StartCommunityTablePreviewRequest, StartQueryRequest, UpdateAgentSessionRequest,
    UpdateDatasourceRequest, UpdateProviderProfileRequest, ValidateCommunitySqlRequest,
};
use chat2db_core::{
    AppError, Application, MysqlConsoleCancellation, RuntimeConfig, RuntimeHost,
    load_fixed_community_classpath,
};
use chat2db_java_bridge::{BridgeError, EngineCommand, EngineConfig};
use chat2db_local::{LocalError, LocalServer};
use tauri::{Emitter, State, WebviewWindow, ipc::Channel};
use tokio::sync::{Mutex, oneshot};

const DATA_DIR_ENV: &str = "CHAT2DB_DATA_DIR";
const DRIVER_PACK_DIR_ENV: &str = "CHAT2DB_DRIVER_PACK_DIR";
const COMMUNITY_CLASSPATH_DIR_ENV: &str = "CHAT2DB_COMMUNITY_CLASSPATH_DIR";
const JAVA_BIN_ENV: &str = "CHAT2DB_JAVA_BIN";
const JAVA_ENGINE_JAR_ENV: &str = "CHAT2DB_JAVA_ENGINE_JAR";
const VAULT_MASTER_KEY_ENV: &str = "CHAT2DB_VAULT_MASTER_KEY";

const BUNDLED_JAVA_BIN: &str = "Java binary";
const BUNDLED_JAVA_ENGINE_JAR: &str = "compatibility-engine JAR";
const BUNDLED_COMMUNITY_CLASSPATH: &str = "Community classpath";
const BUNDLED_DRIVER_PACKS: &str = "driver packs";
const COMMUNITY_JAVA_MESSAGE_EVENT: &str = "chat2db://java-message";

#[derive(Debug, Default)]
struct RuntimeResourceOverrides {
    java_bin: Option<OsString>,
    java_engine_jar: Option<OsString>,
    community_classpath_dir: Option<OsString>,
    driver_pack_dir: Option<OsString>,
}

#[derive(Debug, PartialEq, Eq)]
struct RuntimeResourcePaths {
    java_bin: OsString,
    java_engine_jar: PathBuf,
    community_classpath_dir: Option<PathBuf>,
    driver_pack_dir: Option<PathBuf>,
}

#[derive(Debug)]
struct BundledRuntimeResources {
    java_bin: PathBuf,
    java_engine_jar: PathBuf,
    community_classpath_dir: PathBuf,
    driver_pack_dir: PathBuf,
}

impl BundledRuntimeResources {
    fn from_executable(executable: &Path) -> Option<Self> {
        let macos_dir = executable.parent()?;
        if macos_dir.file_name() != Some(OsStr::new("MacOS")) {
            return None;
        }
        let contents_dir = macos_dir.parent()?;
        if contents_dir.file_name() != Some(OsStr::new("Contents")) {
            return None;
        }
        let app_dir = contents_dir.parent()?;
        if app_dir.extension() != Some(OsStr::new("app")) {
            return None;
        }

        let resource_root = contents_dir.join("Resources").join("chat2db");
        Some(Self {
            java_bin: resource_root.join("java").join("bin").join("java"),
            java_engine_jar: resource_root
                .join("engine")
                .join("chat2db-compat-runtime.jar"),
            community_classpath_dir: resource_root.join("community-classpath"),
            driver_pack_dir: resource_root.join("driver-packs"),
        })
    }
}

struct DesktopState {
    application: Application,
    local_server: Mutex<Option<LocalServer>>,
    runtime_host: Mutex<Option<RuntimeHost>>,
    legacy_sql_cancellations: LegacySqlCancellationRegistry,
    subscriptions: SubscriptionRegistry,
    next_legacy_execution_id: AtomicU64,
    next_subscription_id: AtomicU64,
}

#[derive(Default)]
struct LegacySqlCancellationRegistry {
    cancellations: Mutex<HashMap<String, MysqlConsoleCancellation>>,
}

impl LegacySqlCancellationRegistry {
    async fn insert(&self, execution_id: String, cancellation: MysqlConsoleCancellation) {
        self.cancellations
            .lock()
            .await
            .insert(execution_id, cancellation);
    }

    async fn remove(&self, execution_id: &str) {
        self.cancellations.lock().await.remove(execution_id);
    }

    async fn cancel(&self, execution_id: &str, reason: Option<String>) -> bool {
        let cancellation = self.cancellations.lock().await.get(execution_id).cloned();
        cancellation.is_some_and(|cancellation| cancellation.cancel(reason))
    }

    async fn cancel_all(&self) {
        let cancellations = self
            .cancellations
            .lock()
            .await
            .drain()
            .map(|(_, cancellation)| cancellation)
            .collect::<Vec<_>>();
        for cancellation in cancellations {
            let _ = cancellation.cancel(Some("The desktop runtime is shutting down".to_owned()));
        }
    }

    #[cfg(test)]
    async fn active_count(&self) -> usize {
        self.cancellations.lock().await.len()
    }
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
            legacy_sql_cancellations: LegacySqlCancellationRegistry::default(),
            subscriptions: SubscriptionRegistry::default(),
            next_legacy_execution_id: AtomicU64::new(1),
            next_subscription_id: AtomicU64::new(1),
        })
    }

    async fn shutdown(&self) -> Result<(), DesktopError> {
        self.legacy_sql_cancellations.cancel_all().await;
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
    InvalidBundledResource {
        resource: &'static str,
        expected: &'static str,
        path: PathBuf,
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
            Self::InvalidBundledResource {
                resource,
                expected,
                path,
            } => write!(
                formatter,
                "bundled {resource} is missing or is not a {expected}: {}",
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
            | Self::InvalidBundledResource { .. }
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
            legacy_request,
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
    let resource_overrides = RuntimeResourceOverrides {
        java_engine_jar: optional_nonempty_os_env(JAVA_ENGINE_JAR_ENV)?,
        java_bin: optional_nonempty_os_env(JAVA_BIN_ENV)?,
        community_classpath_dir: optional_nonempty_os_env(COMMUNITY_CLASSPATH_DIR_ENV)?,
        driver_pack_dir: optional_nonempty_os_env(DRIVER_PACK_DIR_ENV)?,
    };
    let current_executable = env::current_exe().ok();
    let resources =
        resolve_runtime_resource_paths(current_executable.as_deref(), resource_overrides)?;
    let mut engine = EngineConfig::new(EngineCommand::java_jar(
        resources.java_bin,
        resources.java_engine_jar,
    ));
    if let Some(community_classpath_dir) = resources.community_classpath_dir {
        let classpath = load_fixed_community_classpath(community_classpath_dir)
            .map_err(|error| DesktopError::CommunityClasspath(Box::new(error)))?;
        engine = engine.with_community_classpath(classpath);
    }
    let mut config = RuntimeConfig::new(engine);

    if let Some(data_dir) = optional_nonempty_os_env(DATA_DIR_ENV)? {
        config = config.with_data_dir(PathBuf::from(data_dir));
    }
    if let Some(driver_pack_dir) = resources.driver_pack_dir {
        config = config.with_driver_pack_dir(driver_pack_dir);
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

fn resolve_runtime_resource_paths(
    executable: Option<&Path>,
    overrides: RuntimeResourceOverrides,
) -> Result<RuntimeResourcePaths, DesktopError> {
    let bundled = executable.and_then(BundledRuntimeResources::from_executable);

    let java_bin = match overrides.java_bin {
        Some(java_bin) => java_bin,
        None => match bundled.as_ref() {
            Some(resources) => {
                validate_bundled_file(BUNDLED_JAVA_BIN, &resources.java_bin)?;
                resources.java_bin.clone().into_os_string()
            }
            None => OsString::from("java"),
        },
    };
    let java_engine_jar = match overrides.java_engine_jar {
        Some(java_engine_jar) => {
            let path = PathBuf::from(java_engine_jar);
            validate_java_engine_jar(&path)?;
            path
        }
        None => match bundled.as_ref() {
            Some(resources) => {
                validate_bundled_file(BUNDLED_JAVA_ENGINE_JAR, &resources.java_engine_jar)?;
                resources.java_engine_jar.clone()
            }
            None => return Err(DesktopError::MissingJavaEngineJar),
        },
    };
    let community_classpath_dir = match overrides.community_classpath_dir {
        Some(directory) => Some(PathBuf::from(directory)),
        None => match bundled.as_ref() {
            Some(resources) => {
                validate_bundled_directory(
                    BUNDLED_COMMUNITY_CLASSPATH,
                    &resources.community_classpath_dir,
                )?;
                Some(resources.community_classpath_dir.clone())
            }
            None => None,
        },
    };
    let driver_pack_dir = match overrides.driver_pack_dir {
        Some(directory) => Some(PathBuf::from(directory)),
        None => match bundled.as_ref() {
            Some(resources) => {
                validate_bundled_directory(BUNDLED_DRIVER_PACKS, &resources.driver_pack_dir)?;
                Some(resources.driver_pack_dir.clone())
            }
            None => None,
        },
    };

    Ok(RuntimeResourcePaths {
        java_bin,
        java_engine_jar,
        community_classpath_dir,
        driver_pack_dir,
    })
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

fn validate_bundled_file(resource: &'static str, path: &Path) -> Result<(), DesktopError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) | Err(_) => Err(DesktopError::InvalidBundledResource {
            resource,
            expected: "regular file",
            path: path.to_path_buf(),
        }),
    }
}

fn validate_bundled_directory(resource: &'static str, path: &Path) -> Result<(), DesktopError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) | Err(_) => Err(DesktopError::InvalidBundledResource {
            resource,
            expected: "directory",
            path: path.to_path_buf(),
        }),
    }
}

fn api_error(error: &AppError) -> ApiError {
    error.api_error()
}

#[tauri::command]
async fn legacy_request(
    state: State<'_, Arc<DesktopState>>,
    window: WebviewWindow,
    request: String,
) -> Result<String, String> {
    if let Some(response) = legacy_client_command_for(state.inner(), &window, &request).await? {
        return Ok(response);
    }
    legacy_request_for(&state.application, &request).await
}

#[allow(clippy::too_many_lines)]
async fn legacy_client_command_for(
    state: &Arc<DesktopState>,
    window: &WebviewWindow,
    request: &str,
) -> Result<Option<String>, String> {
    let value: serde_json::Value = serde_json::from_str(request)
        .map_err(|_| "Community desktop request must be valid JSON".to_owned())?;
    let request = value
        .as_object()
        .ok_or_else(|| "Community desktop request must be a JSON object".to_owned())?;
    let method = legacy_request_string(request, "method")?;
    if method != "client-command" {
        return Ok(None);
    }
    let request_url = legacy_request_string(request, "requestUrl")?;
    match request_url.as_str() {
        "handle-java-message-is-ready" => {
            Ok(Some(client_command_response(&serde_json::json!(true))))
        }
        "sql-execute" => {
            let request_uuid = legacy_request_string(request, "uuid")?;
            let sql_request = decode_client_message::<chat2db_web::legacy::LegacySqlExecuteRequest>(
                request.get("message"),
            )?;
            if chat2db_web::legacy::uses_native_mysql_console(&state.application, &sql_request)
                .await
                .map_err(|error| legacy_failure_message(&error))?
            {
                let execution_id = state
                    .next_legacy_execution_id
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                        current.checked_add(1)
                    })
                    .map(|id| format!("mysql-console-{id}"))
                    .map_err(|_| "No MySQL Console execution ids remain".to_owned())?;
                let cancellation = MysqlConsoleCancellation::new();
                state
                    .legacy_sql_cancellations
                    .insert(execution_id.clone(), cancellation.clone())
                    .await;
                let task_state = Arc::clone(state);
                let task_window = window.clone();
                let task_execution_id = execution_id.clone();
                tauri::async_runtime::spawn(async move {
                    forward_native_mysql_sql_execution(
                        Arc::clone(&task_state),
                        task_window,
                        request_uuid,
                        task_execution_id.clone(),
                        sql_request,
                        cancellation,
                    )
                    .await;
                    task_state
                        .legacy_sql_cancellations
                        .remove(&task_execution_id)
                        .await;
                });
                return Ok(Some(client_command_response(&serde_json::json!({
                    "executionId": execution_id,
                }))));
            }

            let accepted =
                chat2db_web::legacy::start_sql_execution(&state.application, &sql_request)
                    .await
                    .map_err(|error| legacy_failure_message(&error))?;
            let execution_id = accepted.operation_id.clone();
            let task_application = state.application.clone();
            let task_window = window.clone();
            let task_execution_id = execution_id.clone();
            tauri::async_runtime::spawn(async move {
                forward_legacy_sql_execution(
                    task_application,
                    task_window,
                    request_uuid,
                    task_execution_id,
                    sql_request,
                )
                .await;
            });
            Ok(Some(client_command_response(&serde_json::json!({
                "executionId": execution_id,
            }))))
        }
        "sql-cancel" => {
            let message = decode_client_message::<serde_json::Value>(request.get("message"))?;
            let execution_id = message
                .get("executionId")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "sql-cancel requires a non-empty executionId".to_owned())?;
            let cancelled = if state
                .legacy_sql_cancellations
                .cancel(
                    execution_id,
                    Some("The SQL execution was cancelled".to_owned()),
                )
                .await
            {
                true
            } else {
                state
                    .application
                    .cancel_operation(execution_id)
                    .await
                    .disposition
                    == CancelDisposition::Accepted
            };
            Ok(Some(client_command_response(&serde_json::json!(cancelled))))
        }
        _ => Ok(None),
    }
}

fn decode_client_message<T: serde::de::DeserializeOwned>(
    message: Option<&serde_json::Value>,
) -> Result<T, String> {
    match message {
        Some(serde_json::Value::String(message)) => serde_json::from_str(message),
        Some(message) => serde_json::from_value(message.clone()),
        None => serde_json::from_value(serde_json::Value::Null),
    }
    .map_err(|_| "Community client-command message is invalid".to_owned())
}

fn client_command_response(data: &serde_json::Value) -> String {
    serde_json::json!({ "data": data }).to_string()
}

fn legacy_failure_message(error: &chat2db_web::legacy::LegacyFailure) -> String {
    format!("{}: {}", error.code, error.message)
}

#[allow(clippy::too_many_lines)]
async fn forward_native_mysql_sql_execution(
    state: Arc<DesktopState>,
    window: WebviewWindow,
    request_uuid: String,
    execution_id: String,
    request: chat2db_web::legacy::LegacySqlExecuteRequest,
    cancellation: MysqlConsoleCancellation,
) {
    let started_at = Instant::now();
    let mut sequence = 0_u64;
    if emit_legacy_sql_event(
        &window,
        &request_uuid,
        &execution_id,
        &mut sequence,
        "started",
        None,
        None,
        &serde_json::json!({ "executionId": execution_id }),
    )
    .is_err()
    {
        let _ = cancellation.cancel(Some("The SQL event receiver closed".to_owned()));
        return;
    }

    let results = chat2db_web::legacy::execute_mysql_sql(
        &state.application,
        &request,
        cancellation.clone(),
        &execution_id,
        "SQL_EDITOR_JCEF",
    )
    .await;
    if cancellation.is_cancelled() {
        let _ = emit_legacy_sql_event(
            &window,
            &request_uuid,
            &execution_id,
            &mut sequence,
            "cancelled",
            None,
            None,
            &serde_json::json!({
                "executionId": execution_id,
                "message": "The SQL execution was cancelled",
            }),
        );
        return;
    }
    let results = match results {
        Ok(results) if !results.is_empty() => results,
        Ok(_) => {
            emit_legacy_terminal_error(
                &window,
                &request_uuid,
                &execution_id,
                &mut sequence,
                &ApiError::new(
                    "sql_execution_incomplete",
                    "The SQL execution ended without a result",
                ),
            );
            return;
        }
        Err(error) => {
            emit_legacy_terminal_error(
                &window,
                &request_uuid,
                &execution_id,
                &mut sequence,
                &ApiError::new(error.code, error.message),
            );
            return;
        }
    };

    let mut active_statement = None;
    let mut active_sql = String::new();
    let mut active_duration = 0_u64;
    let mut fallback_statement_sequence = 0_u32;
    let mut fallback_result_sequence = 0_u32;
    for result in results {
        let statement_sequence = result.statement_sequence.unwrap_or_else(|| {
            fallback_statement_sequence = fallback_statement_sequence.saturating_add(1);
            fallback_statement_sequence
        });
        if active_statement != Some(statement_sequence) {
            if let Some(previous_statement) = active_statement {
                let _ = emit_legacy_sql_event(
                    &window,
                    &request_uuid,
                    &execution_id,
                    &mut sequence,
                    "statementFinished",
                    Some(previous_statement),
                    None,
                    &serde_json::json!({
                        "sql": active_sql,
                        "duration": active_duration,
                    }),
                );
            }
            active_statement = Some(statement_sequence);
            active_sql.clone_from(&result.sql);
            active_duration = 0;
            fallback_result_sequence = 0;
            if emit_legacy_sql_event(
                &window,
                &request_uuid,
                &execution_id,
                &mut sequence,
                "statementStarted",
                Some(statement_sequence),
                None,
                &serde_json::json!({
                    "sql": result.sql,
                    "originalSql": result.original_sql,
                    "sequence": statement_sequence,
                }),
            )
            .is_err()
            {
                let _ = cancellation.cancel(Some("The SQL event receiver closed".to_owned()));
                return;
            }
        }
        fallback_result_sequence = fallback_result_sequence.saturating_add(1);
        let result_sequence = result.result_set_id.unwrap_or(fallback_result_sequence);
        active_duration = active_duration.saturating_add(result.duration);
        if emit_legacy_sql_result_events(
            &window,
            &request_uuid,
            &execution_id,
            &mut sequence,
            statement_sequence,
            result_sequence,
            result,
        )
        .is_err()
        {
            let _ = cancellation.cancel(Some("The SQL event receiver closed".to_owned()));
            return;
        }
    }

    if let Some(statement_sequence) = active_statement {
        let _ = emit_legacy_sql_event(
            &window,
            &request_uuid,
            &execution_id,
            &mut sequence,
            "statementFinished",
            Some(statement_sequence),
            None,
            &serde_json::json!({
                "sql": active_sql,
                "duration": active_duration,
            }),
        );
    }
    let duration = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
    let _ = emit_legacy_sql_event(
        &window,
        &request_uuid,
        &execution_id,
        &mut sequence,
        "finished",
        None,
        None,
        &serde_json::json!({
            "executionId": execution_id,
            "duration": duration,
        }),
    );
}

#[allow(clippy::too_many_lines)]
async fn forward_legacy_sql_execution(
    application: Application,
    window: WebviewWindow,
    request_uuid: String,
    execution_id: String,
    request: chat2db_web::legacy::LegacySqlExecuteRequest,
) {
    let started_at = Instant::now();
    let mut sequence = 0_u64;
    if emit_legacy_sql_event(
        &window,
        &request_uuid,
        &execution_id,
        &mut sequence,
        "started",
        None,
        None,
        &serde_json::json!({ "executionId": execution_id }),
    )
    .is_err()
    {
        application.cancel_operation(&execution_id).await;
        return;
    }
    if emit_legacy_sql_event(
        &window,
        &request_uuid,
        &execution_id,
        &mut sequence,
        "statementStarted",
        Some(1),
        None,
        &serde_json::json!({
            "sql": request.sql,
            "originalSql": request.sql,
            "sequence": 1,
        }),
    )
    .is_err()
    {
        application.cancel_operation(&execution_id).await;
        return;
    }

    let mut subscription = match application.subscribe_operation(&execution_id, None).await {
        Ok(subscription) => subscription,
        Err(error) => {
            emit_legacy_terminal_error(
                &window,
                &request_uuid,
                &execution_id,
                &mut sequence,
                &error.api_error(),
            );
            return;
        }
    };
    loop {
        let event = match subscription.next_event().await {
            Ok(Some(event)) => event.event,
            Ok(None) => {
                emit_legacy_terminal_error(
                    &window,
                    &request_uuid,
                    &execution_id,
                    &mut sequence,
                    &ApiError::new(
                        "sql_execution_incomplete",
                        "The SQL execution ended without a result",
                    ),
                );
                return;
            }
            Err(error) => {
                emit_legacy_terminal_error(
                    &window,
                    &request_uuid,
                    &execution_id,
                    &mut sequence,
                    &error.api_error(),
                );
                return;
            }
        };
        match event {
            OperationEvent::Started | OperationEvent::Progress { .. } => {}
            OperationEvent::Completed { result } => {
                let duration = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
                let result = match chat2db_web::legacy::read_sql_result(
                    &application,
                    &request,
                    &result,
                    duration,
                )
                .await
                {
                    Ok(result) => result,
                    Err(error) => {
                        emit_legacy_terminal_error(
                            &window,
                            &request_uuid,
                            &execution_id,
                            &mut sequence,
                            &ApiError::new(error.code, error.message),
                        );
                        return;
                    }
                };
                if emit_legacy_sql_result_events(
                    &window,
                    &request_uuid,
                    &execution_id,
                    &mut sequence,
                    1,
                    1,
                    result,
                )
                .is_err()
                {
                    return;
                }
                let _ = emit_legacy_sql_event(
                    &window,
                    &request_uuid,
                    &execution_id,
                    &mut sequence,
                    "statementFinished",
                    Some(1),
                    None,
                    &serde_json::json!({ "sql": request.sql, "duration": duration }),
                );
                let _ = emit_legacy_sql_event(
                    &window,
                    &request_uuid,
                    &execution_id,
                    &mut sequence,
                    "finished",
                    None,
                    None,
                    &serde_json::json!({ "executionId": execution_id }),
                );
                return;
            }
            OperationEvent::Failed { error } => {
                emit_legacy_terminal_error(
                    &window,
                    &request_uuid,
                    &execution_id,
                    &mut sequence,
                    &error,
                );
                return;
            }
            OperationEvent::Cancelled { reason } => {
                let _ = emit_legacy_sql_event(
                    &window,
                    &request_uuid,
                    &execution_id,
                    &mut sequence,
                    "cancelled",
                    None,
                    None,
                    &serde_json::json!({
                        "executionId": execution_id,
                        "message": reason.unwrap_or_else(|| "The SQL execution was cancelled".to_owned()),
                    }),
                );
                return;
            }
        }
    }
}

fn emit_legacy_sql_result_events(
    window: &WebviewWindow,
    request_uuid: &str,
    execution_id: &str,
    sequence: &mut u64,
    statement_sequence: u32,
    result_sequence: u32,
    result: chat2db_web::legacy::LegacyManageResult,
) -> Result<(), tauri::Error> {
    let tabular = result.result_set_id.is_some() || !result.header_list.is_empty();
    let mut rows = serde_json::to_value(result).unwrap_or_else(|_| serde_json::json!({}));
    if !tabular {
        return emit_legacy_sql_event(
            window,
            request_uuid,
            execution_id,
            sequence,
            "updateCount",
            Some(statement_sequence),
            Some(result_sequence),
            &rows,
        );
    }
    let rowless = legacy_sql_rowless_payload(&mut rows);
    emit_legacy_sql_event(
        window,
        request_uuid,
        execution_id,
        sequence,
        "resultStarted",
        Some(statement_sequence),
        Some(result_sequence),
        &rowless,
    )?;
    emit_legacy_sql_event(
        window,
        request_uuid,
        execution_id,
        sequence,
        "rows",
        Some(statement_sequence),
        Some(result_sequence),
        &rows,
    )?;
    emit_legacy_sql_event(
        window,
        request_uuid,
        execution_id,
        sequence,
        "resultFinished",
        Some(statement_sequence),
        Some(result_sequence),
        &rowless,
    )
}

fn legacy_sql_rowless_payload(rows: &mut serde_json::Value) -> serde_json::Value {
    let Some(rows_object) = rows.as_object_mut() else {
        return serde_json::json!({});
    };
    let data_list = rows_object
        .insert("dataList".to_owned(), serde_json::json!([]))
        .unwrap_or_else(|| serde_json::json!([]));
    let rowless = serde_json::Value::Object(rows_object.clone());
    rows_object.insert("dataList".to_owned(), data_list);
    rowless
}

fn emit_legacy_terminal_error(
    window: &WebviewWindow,
    request_uuid: &str,
    execution_id: &str,
    sequence: &mut u64,
    error: &ApiError,
) {
    let _ = emit_legacy_sql_event(
        window,
        request_uuid,
        execution_id,
        sequence,
        "failed",
        Some(1),
        None,
        &serde_json::json!({
            "executionId": execution_id,
            "message": error.message,
            "errorCode": error.code,
        }),
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_legacy_sql_event(
    window: &WebviewWindow,
    request_uuid: &str,
    execution_id: &str,
    sequence: &mut u64,
    event_type: &str,
    statement_sequence: Option<u32>,
    result_sequence: Option<u32>,
    message: &serde_json::Value,
) -> Result<(), tauri::Error> {
    *sequence = sequence.saturating_add(1);
    let result_key = statement_sequence.map(|statement_sequence| {
        format!(
            "{execution_id}:{statement_sequence}:{}",
            result_sequence.unwrap_or(0)
        )
    });
    window.emit(
        COMMUNITY_JAVA_MESSAGE_EVENT,
        legacy_sql_push_message(
            request_uuid,
            execution_id,
            *sequence,
            event_type,
            statement_sequence,
            result_sequence,
            result_key.as_deref(),
            message,
        ),
    )
}

#[allow(clippy::too_many_arguments)]
fn legacy_sql_push_message(
    request_uuid: &str,
    execution_id: &str,
    event_sequence: u64,
    event_type: &str,
    statement_sequence: Option<u32>,
    result_sequence: Option<u32>,
    result_key: Option<&str>,
    message: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "uuid": request_uuid,
        "actionType": "sql_execution_event",
        "message": {
            "executionId": execution_id,
            "eventSequence": event_sequence,
            "occurredAtEpochMs": unix_epoch_millis(),
            "eventType": event_type,
            "statementSequence": statement_sequence,
            "resultSequence": result_sequence,
            "resultKey": result_key,
            "message": message,
        },
    })
}

fn unix_epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

async fn legacy_request_for(application: &Application, request: &str) -> Result<String, String> {
    let request: serde_json::Value = serde_json::from_str(request)
        .map_err(|_| "Community desktop request must be valid JSON".to_owned())?;
    let request = request
        .as_object()
        .ok_or_else(|| "Community desktop request must be a JSON object".to_owned())?;
    let request_url = legacy_request_string(request, "requestUrl")?;
    let method = legacy_request_string(request, "method")?;
    let uuid = request
        .get("uuid")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let action_type = request
        .get("actionType")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let message = request
        .get("message")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let response = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: request_url.clone(),
            method: method.clone(),
            message,
        },
    )
    .await;

    Ok(serde_json::json!({
        "uuid": uuid,
        "message": response,
        "actionType": action_type,
        "requestUrl": request_url,
        "method": method,
        "param": null,
    })
    .to_string())
}

fn legacy_request_string(
    request: &serde_json::Map<String, serde_json::Value>,
    field: &'static str,
) -> Result<String, String> {
    request
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("Community desktop request requires a non-empty {field}"))
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
    use std::{
        ffi::OsString,
        fs::{self, File},
        path::PathBuf,
        sync::Arc,
    };

    use chat2db_contract::{
        AgentEvent, AgentEventEnvelope, AgentStreamMessage, BuildCommunityDmlRequest,
        BuildCommunityNamespaceSqlRequest, CommunityDmlColumn, CommunityDmlRow,
        CommunityDmlStatement, CommunityDmlTarget, CommunityDmlValue,
        CommunityNamespaceSqlOperation, CompleteCommunitySqlRequest, FormatCommunitySqlRequest,
        OperationEvent, OperationEventEnvelope, OperationStreamMessage,
        StartCommunityTablePreviewRequest, ValidateCommunitySqlRequest,
    };
    use chat2db_core::{AppError, Application, MysqlConsoleCancellation};
    use tokio::sync::oneshot;

    use super::{
        BUNDLED_COMMUNITY_CLASSPATH, BUNDLED_DRIVER_PACKS, BUNDLED_JAVA_BIN,
        BUNDLED_JAVA_ENGINE_JAR, BundledRuntimeResources, DesktopError,
        LegacySqlCancellationRegistry, RuntimeResourceOverrides, SubscriptionRegistry,
        agent_stream_message, build_community_dml_for, build_community_namespace_sql_for,
        client_command_response, complete_community_sql_for, decode_client_message,
        format_community_sql_for, legacy_request_for, legacy_sql_push_message,
        legacy_sql_rowless_payload, operation_stream_message, parse_after_sequence,
        resolve_runtime_resource_paths, start_community_table_preview_for,
        validate_community_sql_for, validate_java_engine_jar, validate_optional_os_env,
    };

    fn complete_app_bundle() -> (tempfile::TempDir, PathBuf, BundledRuntimeResources) {
        let directory = tempfile::tempdir().expect("temporary app bundle");
        let executable = directory
            .path()
            .join("Chat2DB.app")
            .join("Contents")
            .join("MacOS")
            .join("chat2db-desktop");
        fs::create_dir_all(executable.parent().expect("bundle executable parent"))
            .expect("bundle executable directory");
        File::create(&executable).expect("bundle executable");

        let resources = BundledRuntimeResources::from_executable(&executable)
            .expect("synthetic executable must be recognized as an app bundle");
        fs::create_dir_all(resources.java_bin.parent().expect("Java binary parent"))
            .expect("bundled Java directory");
        File::create(&resources.java_bin).expect("bundled Java binary");
        fs::create_dir_all(
            resources
                .java_engine_jar
                .parent()
                .expect("engine JAR parent"),
        )
        .expect("bundled engine directory");
        File::create(&resources.java_engine_jar).expect("bundled engine JAR");
        fs::create_dir_all(&resources.community_classpath_dir)
            .expect("bundled Community classpath");
        fs::create_dir_all(&resources.driver_pack_dir).expect("bundled driver packs");

        (directory, executable, resources)
    }

    #[test]
    fn community_client_command_payloads_decode_and_return_direct_data() {
        let message = serde_json::json!(
            r#"{"dataSourceId":"datasource-1","sql":"SELECT 1","pageNo":1,"pageSize":20}"#
        );
        let request =
            decode_client_message::<chat2db_web::legacy::LegacySqlExecuteRequest>(Some(&message))
                .expect("string client-command payload must decode");
        assert_eq!(request.sql, "SELECT 1");

        let response: serde_json::Value = serde_json::from_str(&client_command_response(
            &serde_json::json!({ "executionId": "operation-1" }),
        ))
        .expect("client-command response must serialize");
        assert_eq!(response["data"]["executionId"], "operation-1");
    }

    #[test]
    fn community_sql_push_message_matches_the_existing_event_bus_contract() {
        let message = legacy_sql_push_message(
            "request-1",
            "operation-1",
            3,
            "resultFinished",
            Some(1),
            Some(1),
            Some("operation-1:1:1"),
            &serde_json::json!({ "success": true }),
        );
        assert_eq!(message["uuid"], "request-1");
        assert_eq!(message["actionType"], "sql_execution_event");
        assert_eq!(message["message"]["executionId"], "operation-1");
        assert_eq!(message["message"]["eventSequence"], 3);
        assert_eq!(message["message"]["eventType"], "resultFinished");
        assert_eq!(message["message"]["resultKey"], "operation-1:1:1");
        assert_eq!(message["message"]["message"]["success"], true);
    }

    #[test]
    fn community_sql_result_rows_are_only_present_in_the_rows_event() {
        let mut rows = serde_json::json!({
            "success": true,
            "headerList": [{ "name": "id" }],
            "dataList": [[{ "value": "1" }], [{ "value": "2" }]],
            "resultSetId": 1,
        });

        let rowless = legacy_sql_rowless_payload(&mut rows);

        assert_eq!(rows["dataList"].as_array().map(Vec::len), Some(2));
        assert_eq!(rowless["dataList"], serde_json::json!([]));
        assert_eq!(rowless["headerList"], rows["headerList"]);
        assert_eq!(rowless["resultSetId"], 1);
    }

    #[tokio::test]
    async fn native_mysql_cancellation_registry_owns_execution_lifecycle() {
        let registry = LegacySqlCancellationRegistry::default();
        let cancellation = MysqlConsoleCancellation::new();
        registry
            .insert("mysql-console-1".to_owned(), cancellation.clone())
            .await;

        assert_eq!(registry.active_count().await, 1);
        assert!(
            registry
                .cancel("mysql-console-1", Some("cancelled by test".to_owned()))
                .await
        );
        assert!(cancellation.is_cancelled());
        assert!(
            !registry
                .cancel("mysql-console-1", Some("second cancellation".to_owned()))
                .await
        );

        registry.remove("mysql-console-1").await;
        assert_eq!(registry.active_count().await, 0);
    }

    #[tokio::test]
    async fn legacy_request_preserves_the_community_jcef_response_shape() {
        let response = legacy_request_for(
            &Application::new(),
            r#"{
                "actionType":"execute",
                "uuid":"request-1",
                "requestUrl":"/api/system",
                "method":"get",
                "message":{}
            }"#,
        )
        .await
        .expect("legacy request must be serialized");
        let response: serde_json::Value =
            serde_json::from_str(&response).expect("legacy response must be JSON");

        assert_eq!(response["uuid"], "request-1");
        assert_eq!(response["actionType"], "execute");
        assert_eq!(response["requestUrl"], "/api/system");
        assert_eq!(response["method"], "get");
        assert!(response["param"].is_null());
        assert_eq!(response["message"]["success"], true);
        assert_eq!(
            response["message"]["data"]["systemUuid"],
            "chat2db-rust-community"
        );
        assert!(response["message"]["errorCode"].is_null());
        assert!(response["message"]["errorMessage"].is_null());
    }

    #[tokio::test]
    async fn legacy_request_keeps_correlation_fields_for_dispatch_failures() {
        let response = legacy_request_for(
            &Application::new(),
            r#"{
                "actionType":"execute",
                "uuid":"request-2",
                "requestUrl":"/api/not-implemented",
                "method":"get",
                "message":null
            }"#,
        )
        .await
        .expect("dispatch failures use the Community envelope");
        let response: serde_json::Value =
            serde_json::from_str(&response).expect("legacy response must be JSON");

        assert_eq!(response["uuid"], "request-2");
        assert_eq!(response["message"]["success"], false);
        assert_eq!(response["message"]["errorCode"], "route_not_found");
    }

    #[tokio::test]
    async fn legacy_request_rejects_invalid_bridge_payloads_without_echoing_them() {
        let error = legacy_request_for(&Application::new(), "sentinel-secret")
            .await
            .expect_err("invalid JSON must fail at the IPC boundary");

        assert_eq!(error, "Community desktop request must be valid JSON");
        assert!(!error.contains("sentinel-secret"));
    }

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
    fn macos_app_bundle_supplies_all_default_runtime_resources() {
        let (_directory, executable, bundled) = complete_app_bundle();

        let resolved =
            resolve_runtime_resource_paths(Some(&executable), RuntimeResourceOverrides::default())
                .expect("complete app bundle must resolve");

        assert_eq!(resolved.java_bin, bundled.java_bin.into_os_string());
        assert_eq!(resolved.java_engine_jar, bundled.java_engine_jar);
        assert_eq!(
            resolved.community_classpath_dir,
            Some(bundled.community_classpath_dir)
        );
        assert_eq!(resolved.driver_pack_dir, Some(bundled.driver_pack_dir));
    }

    #[test]
    fn environment_paths_override_missing_app_bundle_resources() {
        let directory = tempfile::tempdir().expect("temporary app bundle");
        let executable = directory
            .path()
            .join("Chat2DB.app")
            .join("Contents")
            .join("MacOS")
            .join("chat2db-desktop");
        fs::create_dir_all(executable.parent().expect("bundle executable parent"))
            .expect("bundle executable directory");
        File::create(&executable).expect("bundle executable");

        let overrides_root = directory.path().join("overrides");
        let java_bin = overrides_root.join("java");
        let java_engine_jar = overrides_root.join("engine.jar");
        let community_classpath_dir = overrides_root.join("community-classpath");
        let driver_pack_dir = overrides_root.join("driver-packs");
        fs::create_dir_all(&overrides_root).expect("override root");
        File::create(&java_bin).expect("override Java binary");
        File::create(&java_engine_jar).expect("override engine JAR");
        fs::create_dir_all(&community_classpath_dir).expect("override Community classpath");
        fs::create_dir_all(&driver_pack_dir).expect("override driver packs");

        let resolved = resolve_runtime_resource_paths(
            Some(&executable),
            RuntimeResourceOverrides {
                java_bin: Some(java_bin.clone().into_os_string()),
                java_engine_jar: Some(java_engine_jar.clone().into_os_string()),
                community_classpath_dir: Some(community_classpath_dir.clone().into_os_string()),
                driver_pack_dir: Some(driver_pack_dir.clone().into_os_string()),
            },
        )
        .expect("environment overrides must not require bundled fallbacks");

        assert_eq!(resolved.java_bin, java_bin.into_os_string());
        assert_eq!(resolved.java_engine_jar, java_engine_jar);
        assert_eq!(
            resolved.community_classpath_dir,
            Some(community_classpath_dir)
        );
        assert_eq!(resolved.driver_pack_dir, Some(driver_pack_dir));
    }

    #[test]
    fn app_bundle_reports_each_missing_runtime_resource() {
        for missing_resource in [
            BUNDLED_JAVA_BIN,
            BUNDLED_JAVA_ENGINE_JAR,
            BUNDLED_COMMUNITY_CLASSPATH,
            BUNDLED_DRIVER_PACKS,
        ] {
            let (_directory, executable, bundled) = complete_app_bundle();
            let (missing_path, is_directory) = match missing_resource {
                BUNDLED_JAVA_BIN => (bundled.java_bin, false),
                BUNDLED_JAVA_ENGINE_JAR => (bundled.java_engine_jar, false),
                BUNDLED_COMMUNITY_CLASSPATH => (bundled.community_classpath_dir, true),
                BUNDLED_DRIVER_PACKS => (bundled.driver_pack_dir, true),
                _ => unreachable!("all bundled resources are covered"),
            };
            if is_directory {
                fs::remove_dir_all(&missing_path).expect("remove bundled directory");
            } else {
                fs::remove_file(&missing_path).expect("remove bundled file");
            }

            let error = resolve_runtime_resource_paths(
                Some(&executable),
                RuntimeResourceOverrides::default(),
            )
            .expect_err("missing bundled resource must fail closed");
            assert!(matches!(
                error,
                DesktopError::InvalidBundledResource { resource, path, .. }
                    if resource == missing_resource && path == missing_path
            ));
        }
    }

    #[test]
    fn development_executable_still_requires_java_engine_environment() {
        let directory = tempfile::tempdir().expect("temporary development layout");
        let executable = directory
            .path()
            .join("target")
            .join("debug")
            .join("chat2db-desktop");

        assert!(matches!(
            resolve_runtime_resource_paths(Some(&executable), RuntimeResourceOverrides::default()),
            Err(DesktopError::MissingJavaEngineJar)
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
