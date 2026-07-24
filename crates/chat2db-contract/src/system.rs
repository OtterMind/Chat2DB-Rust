use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Product identity returned by every runtime surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProductInfo {
    /// Stable product name.
    pub name: String,
    /// Semantic version of the Rust product runtime.
    pub version: String,
    /// Community edition identifier.
    pub edition: String,
}

impl ProductInfo {
    /// Creates Community product identity for the current build.
    #[must_use]
    pub fn community(version: impl Into<String>) -> Self {
        Self {
            name: "Chat2DB Rust".to_owned(),
            version: version.into(),
            edition: "community".to_owned(),
        }
    }
}

/// Overall readiness of the product runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatus {
    /// Every enabled component is ready.
    Ready,
    /// Product services are available but an optional component is not ready.
    Degraded,
    /// The runtime cannot serve normal requests.
    Unavailable,
}

/// Readiness of one runtime component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComponentState {
    /// The component can serve requests.
    Ready,
    /// The component is intentionally not enabled in this build or mode.
    Disabled,
    /// The component is enabled but cannot currently serve requests.
    Unavailable,
}

/// Health information for one owned component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComponentHealth {
    /// Stable machine-readable component id.
    pub id: String,
    /// Human-readable component label.
    pub label: String,
    /// Current readiness state.
    pub state: ComponentState,
    /// Short operational detail without secrets.
    pub detail: String,
}

/// Health response shared by Web, desktop, CLI, and local control adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    /// Product identity.
    pub product: ProductInfo,
    /// Overall runtime state.
    pub status: RuntimeStatus,
    /// Process uptime in seconds.
    ///
    /// This remains a JSON number for compatibility with the Stage 1 contract.
    pub uptime_seconds: u64,
    /// Owned component states.
    pub components: Vec<ComponentHealth>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ComponentHealth, ComponentState, HealthResponse, ProductInfo, RuntimeStatus};

    #[test]
    fn health_json_remains_compatible_with_the_original_contract() {
        let health = HealthResponse {
            product: ProductInfo::community("0.1.0"),
            status: RuntimeStatus::Ready,
            uptime_seconds: 7,
            components: vec![ComponentHealth {
                id: "product-core".to_owned(),
                label: "Product core".to_owned(),
                state: ComponentState::Ready,
                detail: "Ready".to_owned(),
            }],
        };

        assert_eq!(
            serde_json::to_value(health).expect("health must serialize"),
            json!({
                "product": {
                    "name": "Chat2DB Rust",
                    "version": "0.1.0",
                    "edition": "community"
                },
                "status": "ready",
                "uptimeSeconds": 7,
                "components": [{
                    "id": "product-core",
                    "label": "Product core",
                    "state": "ready",
                    "detail": "Ready"
                }]
            })
        );
    }
}
