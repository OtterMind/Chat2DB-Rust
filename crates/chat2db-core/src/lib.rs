//! Transport-neutral product services.

mod agent;
mod community;
mod convert;
mod datasource_compatibility;
mod datasource_converter;
mod datasource_edit;
mod datasource_session;
mod driver_pack;
mod engine_manager;
mod error;
mod large_value;
mod legacy_community_import;
mod mysql_account;
mod mysql_dashboard;
pub mod mysql_ddl;
mod mysql_schema_diff;
mod mysql_workspace;
mod native_driver;
mod native_mysql;
mod operation;
mod query;
mod ssh;
mod transfer;
mod workspace;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chat2db_contract::{
    CancelOperationResponse, ComponentHealth, ComponentState, CreateDatasourceRequest, Datasource,
    DatasourceConnection, DatasourceList, DatasourceSecretChange, HealthResponse, JdbcDriver,
    JdbcDriverList, OperationSnapshot, ProductInfo, ResultPage, ResultPageRequest, RuntimeStatus,
    UpdateDatasourceRequest,
};
use chat2db_java_bridge::{
    COMMUNITY_DQL_BUILDER_CAPABILITY, COMMUNITY_OBJECT_METADATA_CAPABILITY,
    COMMUNITY_PLUGIN_CATALOG_CAPABILITY, COMMUNITY_RELATION_METADATA_CAPABILITY,
    COMMUNITY_SCHEMA_METADATA_CAPABILITY, COMMUNITY_SQL_BUILDER_CAPABILITY,
    COMMUNITY_SQL_PARSER_CAPABILITY, EngineClient, EngineConfig, EngineState, EngineSupervisor,
};
use chat2db_storage::{
    CreateDatasource, EncryptedFileVault, PageRequest, SecretChange, SecretValue, SecretVault,
    Storage, StorageError, UpdateDatasource,
};
use tokio::{sync::Mutex, task::JoinHandle};

use agent::AgentRunHub;
pub use agent::AgentRunSubscription;
pub use community::load_fixed_community_classpath;
pub use engine_manager::EngineLease;
pub use error::{AppError, AppErrorKind};
pub use large_value::{
    LargeValueChunk, LargeValueEncoding, LargeValueError, LargeValuePreview, LargeValueStoreStats,
    LargeValueType,
};
pub use legacy_community_import::LegacyCommunityImportOutcome;
pub use operation::OperationSubscription;
pub use query::{MysqlConsoleCancellation, MysqlConsoleRequest, MysqlConsoleResult};
pub use query::{NativeConsoleCancellation, NativeConsoleRequest, NativeConsoleResult};
pub use transfer::TransferArtifactDownload;

use engine_manager::{
    DEFAULT_ENGINE_IDLE_TIMEOUT, EngineManagerOwner, EngineManagerStatus, EngineProvider,
    EngineStopReason,
};
use operation::OperationHub;

const TASK_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

/// Shared application service root used by every delivery adapter.
#[derive(Clone)]
pub struct Application {
    pub(crate) inner: Arc<ApplicationInner>,
}

pub(crate) struct ApplicationInner {
    started_at: Instant,
    runtime_status: RuntimeStatus,
    storage: Option<Storage>,
    engine: EngineProvider,
    drivers: Vec<JdbcDriver>,
    managed_driver_ids: Option<HashSet<String>>,
    native_drivers: native_driver::NativeDriverRegistry,
    large_values: large_value::LargeValueStore,
    agent_runs: AgentRunHub,
    operations: OperationHub,
    transfer_tasks: transfer::TransferTaskHub,
    account_previews: mysql_account::AccountPreviewRegistry,
    accepting_work: Mutex<bool>,
    shutdown_agent_run_ids: Mutex<Vec<String>>,
    tasks: Mutex<HashMap<String, JoinHandle<()>>>,
}

/// Owns the Java process generation while transports hold cloneable applications.
pub struct RuntimeHost {
    application: Application,
    supervisor: Option<EngineSupervisor>,
    engine_manager: Option<EngineManagerOwner>,
}

/// Inputs required to open durable storage and start one Java engine generation.
pub struct RuntimeConfig {
    data_dir: Option<PathBuf>,
    driver_pack_dir: Option<PathBuf>,
    vault_master_key_base64: Option<String>,
    engine: EngineConfig,
    engine_idle_timeout: Duration,
}

impl RuntimeConfig {
    #[must_use]
    pub const fn new(engine: EngineConfig) -> Self {
        Self {
            data_dir: None,
            driver_pack_dir: None,
            vault_master_key_base64: None,
            engine,
            engine_idle_timeout: DEFAULT_ENGINE_IDLE_TIMEOUT,
        }
    }

    #[must_use]
    pub fn with_data_dir(mut self, data_dir: impl Into<PathBuf>) -> Self {
        self.data_dir = Some(data_dir.into());
        self
    }

    /// Overrides the directory scanned for managed local JDBC driver packs.
    #[must_use]
    pub fn with_driver_pack_dir(mut self, driver_pack_dir: impl Into<PathBuf>) -> Self {
        self.driver_pack_dir = Some(driver_pack_dir.into());
        self
    }

    #[must_use]
    pub fn with_vault_master_key_base64(mut self, master_key: impl Into<String>) -> Self {
        self.vault_master_key_base64 = Some(master_key.into());
        self
    }

    /// Overrides how long an unused managed Java generation remains resident.
    #[must_use]
    pub const fn with_engine_idle_timeout(mut self, timeout: Duration) -> Self {
        self.engine_idle_timeout = timeout;
        self
    }
}

impl std::fmt::Debug for RuntimeConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeConfig")
            .field("data_dir", &self.data_dir)
            .field("driver_pack_dir", &self.driver_pack_dir)
            .field(
                "vault_master_key_base64",
                &self.vault_master_key_base64.as_ref().map(|_| "[REDACTED]"),
            )
            .field("engine", &self.engine)
            .field("engine_idle_timeout", &self.engine_idle_timeout)
            .finish()
    }
}

impl std::fmt::Debug for Application {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Application")
            .field("runtime_status", &self.inner.runtime_status)
            .field("storage_configured", &self.inner.storage.is_some())
            .field("engine_configured", &self.inner.engine.is_configured())
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for RuntimeHost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeHost")
            .field("application", &self.application)
            .field("supervisor_owned", &self.supervisor.is_some())
            .field("engine_manager_owned", &self.engine_manager.is_some())
            .finish()
    }
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}

impl Application {
    /// Creates a product service root with optional components disabled.
    #[must_use]
    pub fn new() -> Self {
        Self::compose(RuntimeStatus::Ready, None, EngineProvider::Disabled, None)
    }

    /// Creates a service root with an explicit readiness state.
    #[must_use]
    pub fn with_runtime_status(runtime_status: RuntimeStatus) -> Self {
        Self::compose(runtime_status, None, EngineProvider::Disabled, None)
    }

    /// Creates a ready service root around fully opened local storage.
    #[must_use]
    pub fn with_storage(storage: Storage) -> Self {
        Self::compose(
            RuntimeStatus::Ready,
            Some(storage),
            EngineProvider::Disabled,
            None,
        )
    }

    /// Creates the complete product service root around storage and one engine generation.
    #[must_use]
    pub fn with_services(storage: Storage, engine: EngineClient) -> Self {
        Self::compose(
            RuntimeStatus::Ready,
            Some(storage),
            EngineProvider::Static(engine),
            None,
        )
    }

    fn with_managed_services(
        storage: Storage,
        engine: engine_manager::EngineManagerHandle,
        drivers: Vec<JdbcDriver>,
    ) -> Self {
        Self::compose(
            RuntimeStatus::Ready,
            Some(storage),
            EngineProvider::Managed(engine),
            Some(drivers),
        )
    }

    fn compose(
        runtime_status: RuntimeStatus,
        storage: Option<Storage>,
        engine: EngineProvider,
        drivers: Option<Vec<JdbcDriver>>,
    ) -> Self {
        let managed_driver_ids = drivers.as_ref().map(|drivers| {
            drivers
                .iter()
                .map(|driver| driver.driver_id.clone())
                .collect()
        });
        let drivers = drivers.unwrap_or_default();
        let native_drivers = native_driver::NativeDriverRegistry::built_in();
        Self {
            inner: Arc::new(ApplicationInner {
                started_at: Instant::now(),
                runtime_status,
                storage,
                engine,
                drivers,
                managed_driver_ids,
                native_drivers,
                large_values: large_value::LargeValueStore::default(),
                agent_runs: AgentRunHub::new(),
                operations: OperationHub::new(),
                transfer_tasks: transfer::TransferTaskHub::new(),
                account_previews: mysql_account::AccountPreviewRegistry::default(),
                accepting_work: Mutex::new(true),
                shutdown_agent_run_ids: Mutex::new(Vec::new()),
                tasks: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Returns local storage only when runtime composition initialized it.
    #[must_use]
    pub fn storage(&self) -> Option<&Storage> {
        self.inner.storage.as_ref()
    }

    /// Creates one unpredictable scope for large values returned by a Console execution.
    #[must_use]
    pub fn create_large_value_owner(&self) -> String {
        large_value::new_owner_id()
    }

    /// Retains a potentially large UTF-8 cell and returns its bounded preview.
    ///
    /// # Errors
    ///
    /// Returns a closed validation or capacity failure without exposing retained values.
    pub fn retain_large_text(
        &self,
        owner_id: &str,
        value: String,
    ) -> Result<LargeValuePreview, AppError> {
        self.inner
            .large_values
            .retain_text(owner_id, value)
            .map(|preview| large_value::scope_preview(owner_id, preview))
            .map_err(large_value_app_error)
    }

    /// Retains a potentially large binary cell and returns its base64 preview.
    ///
    /// # Errors
    ///
    /// Returns a closed validation or capacity failure without exposing retained values.
    pub fn retain_large_binary(
        &self,
        owner_id: &str,
        value: Vec<u8>,
    ) -> Result<LargeValuePreview, AppError> {
        self.inner
            .large_values
            .retain_binary(owner_id, value)
            .map(|preview| large_value::scope_preview(owner_id, preview))
            .map_err(large_value_app_error)
    }

    /// Reads one bounded chunk through an opaque owner-scoped token.
    ///
    /// # Errors
    ///
    /// Returns a closed invalid, expired, inaccessible, or range error.
    pub fn read_large_value_chunk(
        &self,
        large_value_id: &str,
        offset: u64,
        limit: u32,
    ) -> Result<LargeValueChunk, AppError> {
        let (owner_id, token) =
            large_value::scoped_token(large_value_id).map_err(large_value_app_error)?;
        self.inner
            .large_values
            .read_chunk(owner_id, token, offset, limit)
            .map_err(large_value_app_error)
    }

    /// Reads one base64 chunk whose offsets and limit are measured in raw bytes.
    ///
    /// # Errors
    ///
    /// Returns a closed invalid, expired, inaccessible, or range error.
    pub fn read_large_value_encoded_chunk(
        &self,
        large_value_id: &str,
        offset: u64,
        limit: u32,
    ) -> Result<LargeValueChunk, AppError> {
        let (owner_id, token) =
            large_value::scoped_token(large_value_id).map_err(large_value_app_error)?;
        self.inner
            .large_values
            .read_encoded_chunk(owner_id, token, offset, limit)
            .map_err(large_value_app_error)
    }

    /// Removes one retained value addressed by its opaque token.
    ///
    /// # Errors
    ///
    /// Returns a closed invalid, expired, or inaccessible token error.
    pub fn remove_large_value(&self, large_value_id: &str) -> Result<(), AppError> {
        let (owner_id, token) =
            large_value::scoped_token(large_value_id).map_err(large_value_app_error)?;
        self.inner
            .large_values
            .remove_token(owner_id, token)
            .map_err(large_value_app_error)
    }

    /// Removes all retained values associated with one execution owner.
    #[must_use]
    pub fn remove_large_value_owner(&self, owner_id: &str) -> usize {
        self.inner.large_values.remove_owner(owner_id)
    }

    /// Releases expired retained values.
    #[must_use]
    pub fn cleanup_expired_large_values(&self) -> usize {
        self.inner.large_values.cleanup_expired()
    }

    /// Returns current retained-value usage after expiry cleanup.
    #[must_use]
    pub fn large_value_stats(&self) -> LargeValueStoreStats {
        self.inner.large_values.stats()
    }

    pub(crate) fn require_storage(&self) -> Result<Storage, AppError> {
        self.inner.storage.clone().ok_or_else(|| {
            AppError::unavailable(
                "storage_unavailable",
                "Local product storage is not configured",
            )
        })
    }

    /// Acquires a generation-scoped lease, starting Java on first use.
    ///
    /// # Errors
    ///
    /// Returns an availability or startup error when the engine cannot be acquired.
    pub async fn acquire_engine(&self) -> Result<EngineLease, AppError> {
        self.inner.engine.acquire().await
    }

    pub(crate) async fn require_engine(&self) -> Result<EngineLease, AppError> {
        self.acquire_engine().await
    }

    /// Returns health from the shared product boundary.
    #[must_use]
    pub fn health(&self) -> HealthResponse {
        let engine_component = engine_health(&self.inner.engine);
        let status = if self.inner.runtime_status == RuntimeStatus::Unavailable {
            RuntimeStatus::Unavailable
        } else if engine_component.state == ComponentState::Unavailable {
            RuntimeStatus::Degraded
        } else {
            self.inner.runtime_status
        };
        let community_component = community_health(&self.inner.engine);
        HealthResponse {
            product: ProductInfo::community(env!("CARGO_PKG_VERSION")),
            status,
            uptime_seconds: self.inner.started_at.elapsed().as_secs(),
            components: vec![
                ComponentHealth {
                    id: "product-core".to_owned(),
                    label: "Product core".to_owned(),
                    state: ComponentState::Ready,
                    detail: "Ready".to_owned(),
                },
                engine_component,
                community_component,
                if self.inner.drivers.is_empty() {
                    ComponentHealth {
                        id: "jdbc-drivers".to_owned(),
                        label: "JDBC drivers".to_owned(),
                        state: ComponentState::Disabled,
                        detail: "No managed driver packs discovered".to_owned(),
                    }
                } else {
                    ComponentHealth {
                        id: "jdbc-drivers".to_owned(),
                        label: "JDBC drivers".to_owned(),
                        state: ComponentState::Ready,
                        detail: format!(
                            "{} managed driver packs available",
                            self.inner.drivers.len()
                        ),
                    }
                },
                self.inner.storage.as_ref().map_or_else(
                    || ComponentHealth {
                        id: "local-storage".to_owned(),
                        label: "Local storage".to_owned(),
                        state: ComponentState::Disabled,
                        detail: "Not configured by this delivery adapter".to_owned(),
                    },
                    |_| ComponentHealth {
                        id: "local-storage".to_owned(),
                        label: "Local storage".to_owned(),
                        state: ComponentState::Ready,
                        detail: "SQLite, retained results, and secret vault ready".to_owned(),
                    },
                ),
                ComponentHealth {
                    id: "ai-agent".to_owned(),
                    label: "AI agent".to_owned(),
                    state: ComponentState::Disabled,
                    detail: "Not enabled in Stage 5".to_owned(),
                },
            ],
        }
    }

    /// Returns the immutable driver inventory loaded during host startup.
    #[must_use]
    pub fn list_drivers(&self) -> JdbcDriverList {
        let mut items = self.inner.drivers.clone();
        for descriptor in self.inner.native_drivers.descriptors() {
            if !items
                .iter()
                .any(|driver| driver.driver_id.eq_ignore_ascii_case(&descriptor.driver_id))
            {
                items.push(descriptor);
            }
        }
        JdbcDriverList { items }
    }

    /// Opens and immediately closes an ephemeral JDBC session without
    /// persisting the supplied connection descriptor.
    ///
    /// # Errors
    ///
    /// Returns validation, driver, or engine errors when the connection cannot
    /// be established.
    pub async fn test_datasource_connection(
        &self,
        driver_id: &str,
        connection: DatasourceConnection,
    ) -> Result<(), AppError> {
        if driver_id.trim().is_empty() {
            return Err(AppError::invalid(
                "invalid_datasource_connection",
                "driverId cannot be empty",
            ));
        }
        self.require_managed_driver(driver_id)?;
        if let Some(driver) = self.native_driver_for_driver_id(driver_id) {
            return driver.connection().test_connection(&connection).await;
        }
        let engine = self.require_engine().await?;
        let session = datasource_session::open_datasource_session(
            &engine,
            datasource_session::ResolvedDatasourceConnection {
                datasource_id: "connection-test".to_owned(),
                datasource_revision: 0,
                driver_id: driver_id.to_owned(),
                datasource_name: "Connection test".to_owned(),
                connection,
            },
            datasource_session::SessionReadOnly::Configured,
        )
        .await?;
        session.close().await.map_err(AppError::from)
    }

    /// Lists secret-free datasource metadata.
    ///
    /// # Errors
    ///
    /// Returns an availability or durable-storage error.
    pub async fn list_datasources(&self) -> Result<DatasourceList, AppError> {
        let storage = self.require_storage()?;
        let records = storage_call(move || storage.list_datasources()).await?;
        Ok(DatasourceList {
            items: records.into_iter().map(convert::datasource).collect(),
        })
    }

    /// Creates datasource metadata and atomically installs its connection descriptor.
    ///
    /// # Errors
    ///
    /// Returns validation, vault, or durable-storage errors.
    pub async fn create_datasource(
        &self,
        request: CreateDatasourceRequest,
    ) -> Result<Datasource, AppError> {
        self.require_managed_driver(&request.driver_id)?;
        let storage = self.require_storage()?;
        let secret = request
            .connection
            .map(|connection| {
                serde_json::to_vec(&connection)
                    .map(SecretValue::new)
                    .map_err(|_| AppError::internal())
            })
            .transpose()?;
        let input = CreateDatasource {
            name: request.name,
            driver_id: request.driver_id,
        };
        storage_call(move || storage.create_datasource(input, secret))
            .await
            .map(convert::datasource)
    }

    /// Returns one secret-free datasource.
    ///
    /// # Errors
    ///
    /// Returns not-found, availability, or durable-storage errors.
    pub async fn get_datasource(&self, id: &str) -> Result<Datasource, AppError> {
        let storage = self.require_storage()?;
        let id = id.to_owned();
        let record = storage_call(move || storage.get_datasource(&id)).await?;
        record.map(convert::datasource).ok_or_else(|| {
            AppError::not_found("datasource_not_found", "The datasource does not exist")
        })
    }

    /// Replaces datasource metadata and applies an explicit secret action using revision CAS.
    ///
    /// # Errors
    ///
    /// Returns validation, revision-conflict, vault, or durable-storage errors.
    pub async fn update_datasource(
        &self,
        id: &str,
        request: UpdateDatasourceRequest,
    ) -> Result<Datasource, AppError> {
        let storage = self.require_storage()?;
        self.require_managed_driver_for_update(&storage, id, &request.driver_id)
            .await?;
        let id = id.to_owned();
        let expected_revision = parse_u64(&request.expected_revision, "expectedRevision")?;
        let secret_change = match request.secret_change {
            DatasourceSecretChange::Keep => SecretChange::Keep,
            DatasourceSecretChange::Clear => SecretChange::Clear,
            DatasourceSecretChange::Replace { connection } => SecretChange::Replace(
                serde_json::to_vec(&connection)
                    .map(SecretValue::new)
                    .map_err(|_| AppError::internal())?,
            ),
        };
        let input = UpdateDatasource {
            name: request.name,
            driver_id: request.driver_id,
        };
        storage_call(move || {
            storage.update_datasource(&id, expected_revision, input, secret_change)
        })
        .await
        .map(convert::datasource)
    }

    fn require_managed_driver(&self, driver_id: &str) -> Result<(), AppError> {
        if self.native_driver_for_driver_id(driver_id).is_some() {
            return Ok(());
        }
        match &self.inner.managed_driver_ids {
            Some(driver_ids) if !driver_ids.contains(driver_id) => Err(driver_not_installed()),
            _ => Ok(()),
        }
    }

    pub(crate) fn native_driver_for_driver_id(
        &self,
        driver_id: &str,
    ) -> Option<Arc<dyn native_driver::NativeDriver>> {
        self.inner
            .native_drivers
            .driver_for_driver_id(driver_id, &self.inner.drivers)
    }

    pub(crate) fn native_driver_for_database_type(
        &self,
        database_type: &str,
    ) -> Option<Arc<dyn native_driver::NativeDriver>> {
        self.inner
            .native_drivers
            .driver_for_database_type(database_type)
    }

    pub(crate) fn native_database_type_for_driver(&self, driver_id: &str) -> Option<String> {
        self.native_driver_for_driver_id(driver_id)
            .and_then(|driver| driver.database_types().first().copied())
            .map(str::to_owned)
    }

    async fn require_managed_driver_for_update(
        &self,
        storage: &Storage,
        datasource_id: &str,
        driver_id: &str,
    ) -> Result<(), AppError> {
        if self.native_driver_for_driver_id(driver_id).is_some() {
            return Ok(());
        }
        let Some(driver_ids) = &self.inner.managed_driver_ids else {
            return Ok(());
        };
        if driver_ids.contains(driver_id) {
            return Ok(());
        }

        let storage = storage.clone();
        let datasource_id = datasource_id.to_owned();
        let existing = storage_call(move || storage.get_datasource(&datasource_id)).await?;
        match existing {
            Some(existing) if existing.driver_id == driver_id => Ok(()),
            Some(_) => Err(driver_not_installed()),
            None => Err(AppError::not_found(
                "datasource_not_found",
                "The datasource does not exist",
            )),
        }
    }

    /// Deletes a datasource using revision CAS.
    ///
    /// # Errors
    ///
    /// Returns validation, not-found, revision-conflict, vault, or storage errors.
    pub async fn delete_datasource(
        &self,
        id: &str,
        expected_revision: &str,
    ) -> Result<(), AppError> {
        let storage = self.require_storage()?;
        let id = id.to_owned();
        let expected_revision = parse_u64(expected_revision, "expectedRevision")?;
        storage_call(move || storage.delete_datasource(&id, expected_revision)).await
    }

    /// Returns the current materialized operation state.
    ///
    /// # Errors
    ///
    /// Returns not-found when the operation id is unknown.
    pub async fn operation_snapshot(&self, id: &str) -> Result<OperationSnapshot, AppError> {
        self.inner.operations.snapshot(id).await
    }

    pub async fn cancel_operation(&self, id: &str) -> CancelOperationResponse {
        self.inner.operations.cancel(id).await
    }

    /// Atomically obtains replay events and a live operation subscription.
    ///
    /// # Errors
    ///
    /// Returns not-found, invalid-cursor, or replay-window errors.
    pub async fn subscribe_operation(
        &self,
        id: &str,
        after_sequence: Option<u64>,
    ) -> Result<OperationSubscription, AppError> {
        self.inner.operations.subscribe(id, after_sequence).await
    }

    /// Reads one row- and byte-bounded retained-result page.
    ///
    /// # Errors
    ///
    /// Returns validation, not-found, corruption, or durable-storage errors.
    pub async fn result_page(
        &self,
        id: &str,
        request: ResultPageRequest,
    ) -> Result<ResultPage, AppError> {
        let storage = self.require_storage()?;
        let id = id.to_owned();
        let page_request = PageRequest {
            offset: parse_u64(&request.offset, "offset")?,
            max_rows: parse_u32(&request.max_rows, "maxRows")?,
            max_bytes: parse_u64(&request.max_bytes, "maxBytes")?,
        };
        let page = storage_call(move || storage.read_result_page(&id, page_request)).await?;
        convert::result_page(page)
    }

    /// Stops work admission and requests cancellation of active queries and agent runs.
    pub async fn begin_shutdown(&self) {
        let mut accepting_work = self.inner.accepting_work.lock().await;
        if !*accepting_work {
            return;
        }
        *accepting_work = false;
        let agent_run_ids = self.inner.agent_runs.run_ids();
        self.inner
            .shutdown_agent_run_ids
            .lock()
            .await
            .clone_from(&agent_run_ids);
        self.inner
            .operations
            .cancel_all("Chat2DB runtime is shutting down")
            .await;
        self.persist_agent_shutdown_cancellations(&agent_run_ids)
            .await;
        self.inner.agent_runs.cancel_all().await;
        self.begin_transfer_shutdown().await;
    }

    async fn join_tasks(&self) {
        let query_tasks = self.join_query_tasks();
        let agent_tasks = self.inner.agent_runs.join_tasks(TASK_SHUTDOWN_TIMEOUT);
        let transfer_tasks = self.join_transfer_tasks(TASK_SHUTDOWN_TIMEOUT);
        let ((), mut agent_run_ids, ()) = tokio::join!(query_tasks, agent_tasks, transfer_tasks);
        agent_run_ids.extend(std::mem::take(
            &mut *self.inner.shutdown_agent_run_ids.lock().await,
        ));
        agent_run_ids.sort_unstable();
        agent_run_ids.dedup();
        self.reconcile_agent_shutdown_runs(agent_run_ids).await;
    }

    async fn join_query_tasks(&self) {
        let tasks = std::mem::take(&mut *self.inner.tasks.lock().await);
        let deadline = tokio::time::Instant::now() + TASK_SHUTDOWN_TIMEOUT;
        for (operation_id, mut task) in tasks {
            match tokio::time::timeout_at(deadline, &mut task).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {
                    let _ = self
                        .inner
                        .operations
                        .failed(&operation_id, AppError::internal().api_error())
                        .await;
                }
                Err(_) => {
                    task.abort();
                    // Abort is cooperative, so awaiting again would defeat the hard deadline.
                    let _ = self
                        .inner
                        .operations
                        .cancelled(
                            &operation_id,
                            Some("Query task was stopped during runtime shutdown".to_owned()),
                        )
                        .await;
                }
            }
        }
    }
}

fn large_value_app_error(error: LargeValueError) -> AppError {
    match error {
        LargeValueError::NotFound | LargeValueError::Expired | LargeValueError::OwnerMismatch => {
            AppError::not_found(
                "largeCellValue.tokenExpired",
                "The large cell value is no longer available",
            )
        }
        LargeValueError::CapacityExceeded { .. } => AppError::new(
            AppErrorKind::ResourceExhausted,
            chat2db_contract::ApiError::new(
                "largeCellValue.fullValueUnsupported",
                "The large cell value exceeds the retained-value limit",
            ),
        ),
        LargeValueError::InvalidOwner
        | LargeValueError::InvalidToken
        | LargeValueError::InvalidLimit
        | LargeValueError::InvalidRange { .. } => AppError::invalid(
            "invalid_large_cell_value_request",
            "The large cell value request is invalid",
        ),
    }
}

impl RuntimeHost {
    /// Opens production storage and discovers drivers without starting Java.
    ///
    /// An explicit base64 master key selects the headless encrypted-file path.
    /// Without one, supported desktop platforms require their OS keyring.
    ///
    /// # Errors
    ///
    /// Returns an error if the data directory, vault, storage, or driver catalog cannot open.
    pub async fn open(config: RuntimeConfig) -> Result<Self, AppError> {
        let RuntimeConfig {
            data_dir,
            driver_pack_dir,
            vault_master_key_base64,
            engine,
            engine_idle_timeout,
        } = config;
        if engine_idle_timeout.is_zero() {
            return Err(AppError::invalid(
                "invalid_engine_idle_timeout",
                "The engine idle timeout must be greater than zero",
            ));
        }
        let storage = tokio::task::spawn_blocking(move || {
            let data_dir = data_dir.map_or_else(Storage::default_data_dir, Ok)?;
            let vault: Arc<dyn SecretVault> = match vault_master_key_base64 {
                Some(master_key) => Arc::new(
                    EncryptedFileVault::from_base64_master_key(&data_dir, &master_key)
                        .map_err(|_| vault_unavailable())?,
                ),
                None => production_os_vault(&data_dir)?,
            };
            Storage::open(data_dir, vault).map_err(AppError::from)
        })
        .await
        .map_err(|_| AppError::internal())??;
        let driver_pack_dir = driver_pack_dir.unwrap_or_else(|| {
            storage
                .data_dir()
                .join(driver_pack::DEFAULT_DRIVER_PACK_DIRECTORY)
        });
        let data_dir = storage.data_dir().to_path_buf();
        let (driver_runtime_directory, prepared_packs) = tokio::task::spawn_blocking(move || {
            let runtime_directory = driver_pack::reset_runtime_directory(&data_dir)?;
            let packs = driver_pack::discover(&driver_pack_dir, &runtime_directory)?;
            Ok::<_, driver_pack::DriverPackError>((runtime_directory, packs))
        })
        .await
        .map_err(|_| AppError::internal())?
        .map_err(driver_pack::DriverPackError::into_app_error)?;
        let engine = engine.with_driver_snapshot_parent(driver_runtime_directory);
        let manager =
            EngineManagerOwner::with_idle_timeout(engine, prepared_packs, engine_idle_timeout);
        let application =
            Application::with_managed_services(storage, manager.handle(), manager.inventory());
        Ok(Self {
            application,
            supervisor: None,
            engine_manager: Some(manager),
        })
    }

    /// Spawns and owns a validated Java engine generation.
    ///
    /// # Errors
    ///
    /// Returns an engine startup, handshake, or configuration error.
    pub async fn spawn(storage: Storage, config: EngineConfig) -> Result<Self, AppError> {
        let data_dir = storage.data_dir().to_path_buf();
        let driver_runtime_directory =
            tokio::task::spawn_blocking(move || driver_pack::reset_runtime_directory(&data_dir))
                .await
                .map_err(|_| AppError::internal())?
                .map_err(driver_pack::DriverPackError::into_app_error)?;
        let config = config.with_driver_snapshot_parent(driver_runtime_directory);
        let supervisor = EngineSupervisor::spawn(config)
            .await
            .map_err(AppError::from)?;
        Ok(Self::from_supervisor(storage, supervisor))
    }

    /// Composes a host around an already-started supervisor.
    #[must_use]
    pub fn from_supervisor(storage: Storage, supervisor: EngineSupervisor) -> Self {
        let application = Application::with_services(storage, supervisor.client());
        Self {
            application,
            supervisor: Some(supervisor),
            engine_manager: None,
        }
    }

    #[must_use]
    pub fn application(&self) -> Application {
        self.application.clone()
    }

    /// Acquires a generation-scoped lease, starting managed Java on demand.
    ///
    /// # Errors
    ///
    /// Returns an availability or startup error when the engine cannot be acquired.
    pub async fn acquire_engine(&self) -> Result<EngineLease, AppError> {
        self.application.acquire_engine().await
    }

    /// Cancels active operations, shuts down the Java process, and joins tasks.
    ///
    /// # Errors
    ///
    /// Returns an error if the supervised Java process cannot shut down cleanly.
    pub async fn shutdown(&mut self) -> Result<(), AppError> {
        self.application.begin_shutdown().await;
        self.application.join_tasks().await;
        if let Some(supervisor) = self.supervisor.take() {
            supervisor
                .shutdown()
                .await
                .map(|_| ())
                .map_err(AppError::from)?;
        }
        if let Some(mut manager) = self.engine_manager.take() {
            manager.shutdown().await?;
        }
        Ok(())
    }
}

fn engine_health(engine: &EngineProvider) -> ComponentHealth {
    let (state, detail) = match engine.status() {
        None => (
            ComponentState::Disabled,
            "Not configured by this delivery adapter".to_owned(),
        ),
        Some(EngineManagerStatus::Idle) => (
            ComponentState::Ready,
            "Available on demand; Java is not running".to_owned(),
        ),
        Some(EngineManagerStatus::Starting) => {
            (ComponentState::Ready, "Starting on demand".to_owned())
        }
        Some(EngineManagerStatus::Ready(EngineState::Ready { .. })) => {
            (ComponentState::Ready, "Ready".to_owned())
        }
        Some(EngineManagerStatus::Stopping {
            reason: EngineStopReason::Idle,
            ..
        }) => (
            ComponentState::Ready,
            "Releasing the idle Java process".to_owned(),
        ),
        Some(EngineManagerStatus::Failed(_)) => (
            ComponentState::Unavailable,
            "The last Java startup or generation failed; the next request can retry".to_owned(),
        ),
        Some(EngineManagerStatus::Ready(state)) => (
            ComponentState::Unavailable,
            format!("Compatibility engine is {}", state.label()),
        ),
        Some(
            EngineManagerStatus::Stopping { .. }
            | EngineManagerStatus::ShuttingDown
            | EngineManagerStatus::Stopped,
        ) => (
            ComponentState::Unavailable,
            "Compatibility engine is shutting down".to_owned(),
        ),
    };
    ComponentHealth {
        id: "database-engine".to_owned(),
        label: "Database engine".to_owned(),
        state,
        detail,
    }
}

fn community_health(engine: &EngineProvider) -> ComponentHealth {
    if !engine.community_compatibility_configured() {
        return ComponentHealth {
            id: "community-compatibility".to_owned(),
            label: "Community compatibility".to_owned(),
            state: ComponentState::Disabled,
            detail: "Fixed Community classpath not configured".to_owned(),
        };
    }
    let (state, detail) = match engine.status() {
        Some(EngineManagerStatus::Idle) => (
            ComponentState::Ready,
            "Available on demand; Java is not running".to_owned(),
        ),
        Some(EngineManagerStatus::Starting) => {
            (ComponentState::Ready, "Starting on demand".to_owned())
        }
        Some(EngineManagerStatus::Ready(EngineState::Ready { identity, .. }))
            if [
                COMMUNITY_PLUGIN_CATALOG_CAPABILITY,
                COMMUNITY_SCHEMA_METADATA_CAPABILITY,
                COMMUNITY_OBJECT_METADATA_CAPABILITY,
                COMMUNITY_RELATION_METADATA_CAPABILITY,
                COMMUNITY_DQL_BUILDER_CAPABILITY,
                COMMUNITY_SQL_BUILDER_CAPABILITY,
                COMMUNITY_SQL_PARSER_CAPABILITY,
            ]
            .iter()
            .all(|required| {
                identity
                    .capabilities
                    .iter()
                    .any(|capability| capability == required)
            }) =>
        {
            (
                ComponentState::Ready,
                "Fixed Community plugin, metadata, builder, and parser services ready".to_owned(),
            )
        }
        Some(EngineManagerStatus::Stopping {
            reason: EngineStopReason::Idle,
            ..
        }) => (
            ComponentState::Ready,
            "Releasing the idle Java process".to_owned(),
        ),
        Some(EngineManagerStatus::Ready(EngineState::Ready { .. })) => (
            ComponentState::Unavailable,
            "Required Community capabilities are unavailable".to_owned(),
        ),
        Some(EngineManagerStatus::Failed(_)) => (
            ComponentState::Unavailable,
            "The last Community engine startup failed; the next request can retry".to_owned(),
        ),
        Some(EngineManagerStatus::Ready(state)) => (
            ComponentState::Unavailable,
            format!("Compatibility engine is {}", state.label()),
        ),
        Some(
            EngineManagerStatus::Stopping { .. }
            | EngineManagerStatus::ShuttingDown
            | EngineManagerStatus::Stopped,
        )
        | None => (
            ComponentState::Unavailable,
            "Compatibility engine is shutting down".to_owned(),
        ),
    };
    ComponentHealth {
        id: "community-compatibility".to_owned(),
        label: "Community compatibility".to_owned(),
        state,
        detail,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn production_os_vault(data_dir: &std::path::Path) -> Result<Arc<dyn SecretVault>, AppError> {
    chat2db_storage::OsSecretVault::new(data_dir)
        .map(|vault| Arc::new(vault) as Arc<dyn SecretVault>)
        .map_err(|_| vault_unavailable())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn production_os_vault(_data_dir: &std::path::Path) -> Result<Arc<dyn SecretVault>, AppError> {
    Err(vault_unavailable())
}

fn vault_unavailable() -> AppError {
    AppError::unavailable(
        "secret_vault_unavailable",
        "A production datasource secret vault is required",
    )
}

fn driver_not_installed() -> AppError {
    AppError::invalid(
        "driver_not_installed",
        "The requested JDBC driver is not installed in the managed driver inventory",
    )
}

fn parse_u64(value: &str, field: &str) -> Result<u64, AppError> {
    value.parse().map_err(|_| {
        AppError::invalid(
            "invalid_numeric_value",
            format!("{field} must be an unsigned decimal integer"),
        )
    })
}

fn parse_u32(value: &str, field: &str) -> Result<u32, AppError> {
    value.parse().map_err(|_| {
        AppError::invalid(
            "invalid_numeric_value",
            format!("{field} must be an unsigned 32-bit decimal integer"),
        )
    })
}

async fn storage_call<T, F>(operation: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, StorageError> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| AppError::internal())?
        .map_err(AppError::from)
}

pub(crate) fn now_millis() -> Result<i64, AppError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AppError::internal())?;
    i64::try_from(elapsed.as_millis()).map_err(|_| AppError::internal())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use chat2db_contract::{
        ComponentState, CreateDatasourceRequest, DatabaseWriteState, DatasourceConnection,
        DatasourceConnectionProperty, DatasourceSecretChange, ExecuteDatabaseWriteRequest,
        RuntimeStatus, UpdateDatasourceRequest,
    };
    use chat2db_storage::{SecretRef, SecretValue, SecretVault, SecretVaultError, Storage};
    use tempfile::TempDir;

    use super::Application;

    #[test]
    fn bootstrap_runtime_reports_owned_component_states() {
        let health = Application::new().health();

        assert_eq!(health.status, RuntimeStatus::Ready);
        assert_eq!(health.product.edition, "community");
        assert_eq!(health.components[0].state, ComponentState::Ready);
        assert!(
            health
                .components
                .iter()
                .any(|component| component.id == "database-engine")
        );
        let community = health
            .components
            .iter()
            .find(|component| component.id == "community-compatibility")
            .expect("Community compatibility health must be explicit");
        assert_eq!(community.state, ComponentState::Disabled);
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

    #[derive(Debug, Default)]
    struct RoundTripVault(Mutex<HashMap<String, Vec<u8>>>);

    impl SecretVault for RoundTripVault {
        fn probe(&self) -> Result<(), SecretVaultError> {
            Ok(())
        }

        fn create(
            &self,
            reference: &SecretRef,
            value: &SecretValue,
        ) -> Result<(), SecretVaultError> {
            self.0
                .lock()
                .map_err(|_| SecretVaultError::Backend)?
                .insert(
                    reference.as_str().to_owned(),
                    value.expose_secret().to_vec(),
                );
            Ok(())
        }

        fn get(&self, reference: &SecretRef) -> Result<Option<SecretValue>, SecretVaultError> {
            Ok(self
                .0
                .lock()
                .map_err(|_| SecretVaultError::Backend)?
                .get(reference.as_str())
                .cloned()
                .map(SecretValue::new))
        }

        fn delete(&self, reference: &SecretRef) -> Result<(), SecretVaultError> {
            self.0
                .lock()
                .map_err(|_| SecretVaultError::Backend)?
                .remove(reference.as_str());
            Ok(())
        }
    }

    #[test]
    fn composed_storage_is_ready_without_enabling_the_database_engine() {
        let directory = TempDir::new().expect("temp dir");
        let storage =
            Storage::open(directory.path(), Arc::new(TestVault)).expect("local storage must open");
        let application = Application::with_storage(storage);
        let health = application.health();

        assert_eq!(
            health
                .components
                .iter()
                .find(|component| component.id == "local-storage")
                .expect("local storage health")
                .state,
            ComponentState::Ready
        );
        assert_eq!(
            health
                .components
                .iter()
                .find(|component| component.id == "database-engine")
                .expect("database engine health")
                .state,
            ComponentState::Disabled
        );
        assert!(application.storage().is_some());
    }

    #[tokio::test]
    async fn confirmed_write_rejects_non_mysql_before_engine_start() {
        let directory = TempDir::new().expect("temp dir");
        let storage = Storage::open(directory.path(), Arc::new(RoundTripVault::default()))
            .expect("local storage must open");
        let application = Application::with_storage(storage);
        let datasource = application
            .create_datasource(CreateDatasourceRequest {
                name: "Local H2".to_owned(),
                driver_id: "h2".to_owned(),
                connection: Some(DatasourceConnection {
                    jdbc_url: "jdbc:h2:mem:write-boundary".to_owned(),
                    properties: vec![DatasourceConnectionProperty {
                        key: "user".to_owned(),
                        value: "sa".to_owned(),
                        sensitive: false,
                    }],
                    read_only: false,
                    ssh: None,
                }),
            })
            .await
            .expect("unmanaged storage accepts a legacy H2 datasource");

        let result = application
            .execute_confirmed_database_write(ExecuteDatabaseWriteRequest {
                datasource_id: datasource.id,
                sql: "UPDATE items SET label = 'blocked'".to_owned(),
                confirmed: true,
            })
            .await;

        assert_eq!(result.state, DatabaseWriteState::NotStarted);
        assert_eq!(
            result.error.as_ref().map(|error| error.code.as_str()),
            Some("mysql_driver_mismatch")
        );
        assert_eq!(
            application
                .health()
                .components
                .iter()
                .find(|component| component.id == "database-engine")
                .expect("database engine health")
                .state,
            ComponentState::Disabled
        );
    }

    #[tokio::test]
    async fn managed_inventory_rejects_unknown_driver_changes_but_preserves_stale_ids() {
        let directory = TempDir::new().expect("temp dir");
        let storage =
            Storage::open(directory.path(), Arc::new(TestVault)).expect("local storage must open");
        let unmanaged = Application::with_storage(storage.clone());
        let datasource = unmanaged
            .create_datasource(CreateDatasourceRequest {
                name: "Legacy datasource".to_owned(),
                driver_id: "sha256:legacy".to_owned(),
                connection: None,
            })
            .await
            .expect("non-managed composition must preserve external driver compatibility");
        let managed = Application::compose(
            RuntimeStatus::Ready,
            Some(storage),
            super::EngineProvider::Disabled,
            Some(Vec::new()),
        );

        let create_error = managed
            .create_datasource(CreateDatasourceRequest {
                name: "Unknown driver".to_owned(),
                driver_id: "sha256:unknown".to_owned(),
                connection: None,
            })
            .await
            .expect_err("managed create must reject an unknown driver");
        assert_eq!(create_error.api_error().code, "driver_not_installed");

        let update_error = managed
            .update_datasource(
                &datasource.id,
                UpdateDatasourceRequest {
                    expected_revision: datasource.revision.clone(),
                    name: datasource.name.clone(),
                    driver_id: "sha256:unknown".to_owned(),
                    secret_change: DatasourceSecretChange::Keep,
                },
            )
            .await
            .expect_err("managed update must reject switching to an unknown driver");
        assert_eq!(update_error.api_error().code, "driver_not_installed");

        let retained = managed
            .update_datasource(
                &datasource.id,
                UpdateDatasourceRequest {
                    expected_revision: datasource.revision,
                    name: "Renamed legacy datasource".to_owned(),
                    driver_id: datasource.driver_id,
                    secret_change: DatasourceSecretChange::Keep,
                },
            )
            .await
            .expect("managed update must allow retaining an existing stale driver id");
        assert_eq!(retained.name, "Renamed legacy datasource");
        assert_eq!(retained.driver_id, "sha256:legacy");
    }
}
