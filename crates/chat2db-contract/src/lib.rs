//! Canonical product contracts shared by every `Chat2DB` Rust delivery surface.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Product identity returned by every runtime surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    /// Product identity.
    pub product: ProductInfo,
    /// Overall runtime state.
    pub status: RuntimeStatus,
    /// Process uptime in seconds.
    pub uptime_seconds: u64,
    /// Owned component states.
    pub components: Vec<ComponentHealth>,
}

/// Stable error envelope used at every external transport boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    /// Stable machine-readable code.
    pub code: String,
    /// Safe user-facing message.
    pub message: String,
    /// Optional structured diagnostic context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl ApiError {
    /// Creates an error without structured details.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ComponentHealth, ComponentState, HealthResponse, ProductInfo, RuntimeStatus};

    #[test]
    fn health_contract_serializes_with_camel_case_fields() {
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

        let json = serde_json::to_value(health).expect("health must serialize");
        assert_eq!(json["uptimeSeconds"], 7);
        assert_eq!(json["components"][0]["state"], "ready");
        assert!(json.get("uptime_seconds").is_none());
    }
}
