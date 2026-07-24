//! Transport-neutral product services.

use std::time::Instant;

use chat2db_contract::{
    ComponentHealth, ComponentState, HealthResponse, ProductInfo, RuntimeStatus,
};
use chat2db_storage::Storage;

/// Shared application service root used by every delivery adapter.
#[derive(Debug, Clone)]
pub struct Application {
    started_at: Instant,
    runtime_status: RuntimeStatus,
    storage: Option<Storage>,
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}

impl Application {
    /// Creates a product service root for the current process.
    #[must_use]
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
            runtime_status: RuntimeStatus::Ready,
            storage: None,
        }
    }

    /// Creates a service root with an explicit readiness state.
    ///
    /// Runtime composition uses this when an enabled critical component cannot
    /// serve requests. Delivery adapters must map `Unavailable` to their native
    /// readiness failure signal.
    #[must_use]
    pub fn with_runtime_status(runtime_status: RuntimeStatus) -> Self {
        Self {
            started_at: Instant::now(),
            runtime_status,
            storage: None,
        }
    }

    /// Creates a ready service root around fully opened local storage.
    #[must_use]
    pub fn with_storage(storage: Storage) -> Self {
        Self {
            started_at: Instant::now(),
            runtime_status: RuntimeStatus::Ready,
            storage: Some(storage),
        }
    }

    /// Returns local storage only when runtime composition initialized it.
    #[must_use]
    pub fn storage(&self) -> Option<&Storage> {
        self.storage.as_ref()
    }

    /// Returns health from the shared product boundary.
    #[must_use]
    pub fn health(&self) -> HealthResponse {
        HealthResponse {
            product: ProductInfo::community(env!("CARGO_PKG_VERSION")),
            status: self.runtime_status,
            uptime_seconds: self.started_at.elapsed().as_secs(),
            components: vec![
                ComponentHealth {
                    id: "product-core".to_owned(),
                    label: "Product core".to_owned(),
                    state: ComponentState::Ready,
                    detail: "Ready".to_owned(),
                },
                ComponentHealth {
                    id: "database-engine".to_owned(),
                    label: "Database engine".to_owned(),
                    state: ComponentState::Disabled,
                    detail: "Not enabled in the bootstrap build".to_owned(),
                },
                self.storage.as_ref().map_or_else(
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
                        detail: "SQLite, result files, recovery, and secret vault ready".to_owned(),
                    },
                ),
                ComponentHealth {
                    id: "ai-agent".to_owned(),
                    label: "AI agent".to_owned(),
                    state: ComponentState::Disabled,
                    detail: "Not enabled in the bootstrap build".to_owned(),
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chat2db_contract::{ComponentState, RuntimeStatus};
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
}
