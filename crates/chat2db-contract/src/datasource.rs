use std::fmt::{Debug, Formatter};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::SshTunnelConfig;

/// Complete connection descriptor accepted only at a secret-handling boundary.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasourceConnection {
    /// JDBC connection URL.
    pub jdbc_url: String,
    /// Ordered driver properties, including their sensitivity classification.
    pub properties: Vec<DatasourceConnectionProperty>,
    /// Whether sessions opened from this descriptor must be read-only.
    pub read_only: bool,
    /// Optional SSH local-forward settings stored with the encrypted connection descriptor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshTunnelConfig>,
}

impl Debug for DatasourceConnection {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DatasourceConnection")
            .field("jdbc_url", &"[REDACTED]")
            .field("properties", &self.properties)
            .field("read_only", &self.read_only)
            .field("ssh_configured", &self.ssh.is_some())
            .finish()
    }
}

/// One JDBC connection property supplied by the user.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasourceConnectionProperty {
    /// Driver property name.
    pub key: String,
    /// Driver property value. Sensitive values must never be echoed.
    pub value: String,
    /// Whether the value is credential or secret material.
    pub sensitive: bool,
}

impl Debug for DatasourceConnectionProperty {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DatasourceConnectionProperty")
            .field("key", &self.key)
            .field("value", &"[REDACTED]")
            .field("sensitive", &self.sensitive)
            .finish()
    }
}

/// Request to create one datasource and optionally install its connection secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDatasourceRequest {
    /// User-visible datasource name.
    pub name: String,
    /// Compatibility-engine driver id.
    pub driver_id: String,
    /// Complete connection descriptor, omitted when credentials are installed later.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<DatasourceConnection>,
}

/// Explicit connection-secret mutation for a datasource update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DatasourceSecretChange {
    /// Retain the current immutable vault value.
    Keep,
    /// Remove the current connection descriptor.
    Clear,
    /// Replace the current descriptor with a newly staged immutable value.
    Replace {
        /// Complete replacement descriptor.
        connection: DatasourceConnection,
    },
}

/// Optimistic datasource replacement request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDatasourceRequest {
    /// Expected revision encoded as a decimal integer.
    pub expected_revision: String,
    /// User-visible datasource name.
    pub name: String,
    /// Compatibility-engine driver id.
    pub driver_id: String,
    /// Explicit keep, clear, or replace action for the secret descriptor.
    pub secret_change: DatasourceSecretChange,
}

/// Secret-free datasource representation returned to external callers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Datasource {
    /// Opaque datasource id.
    pub id: String,
    /// User-visible datasource name.
    pub name: String,
    /// Compatibility-engine driver id.
    pub driver_id: String,
    /// Whether the datasource has a connection descriptor in the vault.
    pub has_secret: bool,
    /// Monotonic revision encoded as a decimal integer.
    pub revision: String,
    /// Unix epoch milliseconds encoded as a decimal integer.
    pub created_at_ms: String,
    /// Unix epoch milliseconds encoded as a decimal integer.
    pub updated_at_ms: String,
}

/// Stable datasource collection returned by list APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasourceList {
    /// Datasources in stable creation order.
    pub items: Vec<Datasource>,
}

#[cfg(test)]
mod tests {
    use super::{
        CreateDatasourceRequest, Datasource, DatasourceConnection, DatasourceConnectionProperty,
        DatasourceSecretChange,
    };

    #[test]
    fn datasource_response_never_contains_secret_or_connection_material() {
        let response = Datasource {
            id: "datasource-1".to_owned(),
            name: "Production".to_owned(),
            driver_id: "postgresql".to_owned(),
            has_secret: true,
            revision: "9007199254740993".to_owned(),
            created_at_ms: "1784900000000".to_owned(),
            updated_at_ms: "1784900000001".to_owned(),
        };

        let json = serde_json::to_string(&response).expect("response must serialize");
        for forbidden in [
            "secretRef",
            "jdbcUrl",
            "properties",
            "password",
            "connection",
        ] {
            assert!(!json.contains(forbidden), "response leaked {forbidden}");
        }
    }

    #[test]
    fn connection_update_is_explicitly_tagged() {
        let update = DatasourceSecretChange::Replace {
            connection: DatasourceConnection {
                jdbc_url: "jdbc:postgresql://localhost/db".to_owned(),
                properties: vec![DatasourceConnectionProperty {
                    key: "password".to_owned(),
                    value: "sentinel-secret".to_owned(),
                    sensitive: true,
                }],
                read_only: true,
                ssh: None,
            },
        };

        let json = serde_json::to_value(&update).expect("update must serialize");
        assert_eq!(json["action"], "replace");
        assert_eq!(
            serde_json::from_value::<DatasourceSecretChange>(json)
                .expect("update must deserialize"),
            update
        );
    }

    #[test]
    fn connection_debug_output_is_always_redacted() {
        let request = CreateDatasourceRequest {
            name: "Production".to_owned(),
            driver_id: "postgresql".to_owned(),
            connection: Some(DatasourceConnection {
                jdbc_url: "jdbc:postgresql://user:sentinel-password@localhost/db".to_owned(),
                properties: vec![DatasourceConnectionProperty {
                    key: "password".to_owned(),
                    value: "sentinel-password".to_owned(),
                    sensitive: true,
                }],
                read_only: false,
                ssh: None,
            }),
        };

        let debug = format!("{request:?}");
        assert!(!debug.contains("sentinel-password"));
        assert!(!debug.contains("jdbc:postgresql"));
        assert!(debug.contains("[REDACTED]"));
    }
}
