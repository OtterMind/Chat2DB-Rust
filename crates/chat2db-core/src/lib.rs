//! Transport-neutral product services.

use std::time::Instant;

use chat2db_contract::{
    ComponentHealth, ComponentState, HealthResponse, ProductInfo, RuntimeStatus,
};

/// Shared application service root used by every delivery adapter.
#[derive(Debug, Clone)]
pub struct Application {
    started_at: Instant,
    runtime_status: RuntimeStatus,
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
        }
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
                ComponentHealth {
                    id: "local-storage".to_owned(),
                    label: "Local storage".to_owned(),
                    state: ComponentState::Disabled,
                    detail: "Not enabled in the bootstrap build".to_owned(),
                },
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
    use chat2db_contract::{ComponentState, RuntimeStatus};

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
}
