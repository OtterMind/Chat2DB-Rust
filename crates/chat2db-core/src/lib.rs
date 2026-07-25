//! Transport-neutral product services.

mod agent;
mod convert;
mod driver_pack;
mod error;
mod operation;
mod query;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chat2db_contract::{
    CancelOperationResponse, ComponentHealth, ComponentState, CreateDatasourceRequest, Datasource,
    DatasourceList, DatasourceSecretChange, HealthResponse, JdbcDriver, JdbcDriverList,
    OperationSnapshot, ProductInfo, ResultPage, ResultPageRequest, RuntimeStatus,
    UpdateDatasourceRequest,
};
use chat2db_java_bridge::{EngineClient, EngineConfig, EngineState, EngineSupervisor};
use chat2db_storage::{
    CreateDatasource, EncryptedFileVault, PageRequest, SecretChange, SecretValue, SecretVault,
    Storage, StorageError, UpdateDatasource,
};
use tokio::{sync::Mutex, task::JoinHandle};

use agent::AgentRunHub;
pub use agent::AgentRunSubscription;
pub use error::{AppError, AppErrorKind};
pub use operation::OperationSubscription;

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
    engine: Option<EngineClient>,
    drivers: Vec<JdbcDriver>,
    managed_driver_ids: Option<HashSet<String>>,
    agent_runs: AgentRunHub,
    operations: OperationHub,
    accepting_work: Mutex<bool>,
    shutdown_agent_run_ids: Mutex<Vec<String>>,
    tasks: Mutex<HashMap<String, JoinHandle<()>>>,
}

/// Owns the Java process generation while transports hold cloneable applications.
pub struct RuntimeHost {
    application: Application,
    supervisor: Option<EngineSupervisor>,
}

/// Inputs required to open durable storage and start one Java engine generation.
pub struct RuntimeConfig {
    data_dir: Option<PathBuf>,
    driver_pack_dir: Option<PathBuf>,
    vault_master_key_base64: Option<String>,
    engine: EngineConfig,
}

impl RuntimeConfig {
    #[must_use]
    pub const fn new(engine: EngineConfig) -> Self {
        Self {
            data_dir: None,
            driver_pack_dir: None,
            vault_master_key_base64: None,
            engine,
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
            .finish()
    }
}

impl std::fmt::Debug for Application {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Application")
            .field("runtime_status", &self.inner.runtime_status)
            .field("storage_configured", &self.inner.storage.is_some())
            .field("engine_configured", &self.inner.engine.is_some())
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for RuntimeHost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeHost")
            .field("application", &self.application)
            .field("supervisor_owned", &self.supervisor.is_some())
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
        Self::compose(RuntimeStatus::Ready, None, None, None)
    }

    /// Creates a service root with an explicit readiness state.
    #[must_use]
    pub fn with_runtime_status(runtime_status: RuntimeStatus) -> Self {
        Self::compose(runtime_status, None, None, None)
    }

    /// Creates a ready service root around fully opened local storage.
    #[must_use]
    pub fn with_storage(storage: Storage) -> Self {
        Self::compose(RuntimeStatus::Ready, Some(storage), None, None)
    }

    /// Creates the complete product service root around storage and one engine generation.
    #[must_use]
    pub fn with_services(storage: Storage, engine: EngineClient) -> Self {
        Self::compose(RuntimeStatus::Ready, Some(storage), Some(engine), None)
    }

    fn with_services_and_drivers(
        storage: Storage,
        engine: EngineClient,
        drivers: Vec<JdbcDriver>,
    ) -> Self {
        Self::compose(
            RuntimeStatus::Ready,
            Some(storage),
            Some(engine),
            Some(drivers),
        )
    }

    fn compose(
        runtime_status: RuntimeStatus,
        storage: Option<Storage>,
        engine: Option<EngineClient>,
        drivers: Option<Vec<JdbcDriver>>,
    ) -> Self {
        let managed_driver_ids = drivers.as_ref().map(|drivers| {
            drivers
                .iter()
                .map(|driver| driver.driver_id.clone())
                .collect()
        });
        let drivers = drivers.unwrap_or_default();
        Self {
            inner: Arc::new(ApplicationInner {
                started_at: Instant::now(),
                runtime_status,
                storage,
                engine,
                drivers,
                managed_driver_ids,
                agent_runs: AgentRunHub::new(),
                operations: OperationHub::new(),
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

    /// Returns the current compatibility-engine client when configured.
    #[must_use]
    pub fn engine_client(&self) -> Option<EngineClient> {
        self.inner.engine.clone()
    }

    pub(crate) fn require_storage(&self) -> Result<Storage, AppError> {
        self.inner.storage.clone().ok_or_else(|| {
            AppError::unavailable(
                "storage_unavailable",
                "Local product storage is not configured",
            )
        })
    }

    pub(crate) fn require_engine(&self) -> Result<EngineClient, AppError> {
        self.inner.engine.clone().ok_or_else(|| {
            AppError::unavailable(
                "database_engine_unavailable",
                "The database compatibility engine is not configured",
            )
        })
    }

    /// Returns health from the shared product boundary.
    #[must_use]
    pub fn health(&self) -> HealthResponse {
        let engine_component = self.inner.engine.as_ref().map_or_else(
            || ComponentHealth {
                id: "database-engine".to_owned(),
                label: "Database engine".to_owned(),
                state: ComponentState::Disabled,
                detail: "Not configured by this delivery adapter".to_owned(),
            },
            |engine| match engine.state() {
                EngineState::Ready { .. } => ComponentHealth {
                    id: "database-engine".to_owned(),
                    label: "Database engine".to_owned(),
                    state: ComponentState::Ready,
                    detail: "Ready".to_owned(),
                },
                state => ComponentHealth {
                    id: "database-engine".to_owned(),
                    label: "Database engine".to_owned(),
                    state: ComponentState::Unavailable,
                    detail: format!("Compatibility engine is {}", state.label()),
                },
            },
        );
        let status = if self.inner.runtime_status == RuntimeStatus::Unavailable {
            RuntimeStatus::Unavailable
        } else if engine_component.state == ComponentState::Unavailable {
            RuntimeStatus::Degraded
        } else {
            self.inner.runtime_status
        };
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
                if self.inner.drivers.is_empty() {
                    ComponentHealth {
                        id: "jdbc-drivers".to_owned(),
                        label: "JDBC drivers".to_owned(),
                        state: ComponentState::Disabled,
                        detail: "No managed driver packs loaded".to_owned(),
                    }
                } else {
                    ComponentHealth {
                        id: "jdbc-drivers".to_owned(),
                        label: "JDBC drivers".to_owned(),
                        state: ComponentState::Ready,
                        detail: format!("{} managed driver packs loaded", self.inner.drivers.len()),
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
        JdbcDriverList {
            items: self.inner.drivers.clone(),
        }
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
        match &self.inner.managed_driver_ids {
            Some(driver_ids) if !driver_ids.contains(driver_id) => Err(driver_not_installed()),
            _ => Ok(()),
        }
    }

    async fn require_managed_driver_for_update(
        &self,
        storage: &Storage,
        datasource_id: &str,
        driver_id: &str,
    ) -> Result<(), AppError> {
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
    }

    async fn join_tasks(&self) {
        let query_tasks = self.join_query_tasks();
        let agent_tasks = self.inner.agent_runs.join_tasks(TASK_SHUTDOWN_TIMEOUT);
        let ((), mut agent_run_ids) = tokio::join!(query_tasks, agent_tasks);
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

impl RuntimeHost {
    /// Opens the production vault and storage before spawning the Java engine.
    ///
    /// An explicit base64 master key selects the headless encrypted-file path.
    /// Without one, supported desktop platforms require their OS keyring.
    ///
    /// # Errors
    ///
    /// Returns an error if the data directory, vault, storage, or engine cannot open.
    pub async fn open(config: RuntimeConfig) -> Result<Self, AppError> {
        let RuntimeConfig {
            data_dir,
            driver_pack_dir,
            vault_master_key_base64,
            engine,
        } = config;
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
        Self::spawn_with_driver_packs(storage, engine, prepared_packs).await
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

    async fn spawn_with_driver_packs(
        storage: Storage,
        config: EngineConfig,
        packs: driver_pack::PreparedDriverPacks,
    ) -> Result<Self, AppError> {
        let supervisor = EngineSupervisor::spawn(config)
            .await
            .map_err(AppError::from)?;
        let drivers = match driver_pack::preload(&supervisor, packs).await {
            Ok(drivers) => drivers,
            Err(error) => {
                tracing::error!(%error, "managed JDBC driver preload failed");
                let application_error = error.into_app_error();
                if let Err(shutdown_error) = supervisor.shutdown().await {
                    tracing::error!(%shutdown_error, "Java cleanup failed after driver preload error");
                }
                return Err(application_error);
            }
        };
        let application =
            Application::with_services_and_drivers(storage, supervisor.client(), drivers);
        Ok(Self {
            application,
            supervisor: Some(supervisor),
        })
    }

    /// Composes a host around an already-started supervisor.
    #[must_use]
    pub fn from_supervisor(storage: Storage, supervisor: EngineSupervisor) -> Self {
        let application = Application::with_services(storage, supervisor.client());
        Self {
            application,
            supervisor: Some(supervisor),
        }
    }

    #[must_use]
    pub fn application(&self) -> Application {
        self.application.clone()
    }

    #[must_use]
    pub fn engine_client(&self) -> Option<EngineClient> {
        self.application.engine_client()
    }

    /// Cancels active operations, shuts down the Java process, and joins tasks.
    ///
    /// # Errors
    ///
    /// Returns an error if the supervised Java process cannot shut down cleanly.
    pub async fn shutdown(&mut self) -> Result<(), AppError> {
        self.application.begin_shutdown().await;
        self.application.join_tasks().await;
        match self.supervisor.take() {
            Some(supervisor) => supervisor
                .shutdown()
                .await
                .map(|_| ())
                .map_err(AppError::from),
            None => Ok(()),
        }
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
    use std::sync::Arc;

    use chat2db_contract::{
        ComponentState, CreateDatasourceRequest, DatasourceSecretChange, RuntimeStatus,
        UpdateDatasourceRequest,
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
        let managed =
            Application::compose(RuntimeStatus::Ready, Some(storage), None, Some(Vec::new()));

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
