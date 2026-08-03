use std::fmt::{Debug, Formatter};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{PortableDatasourceProperty, SshTunnelEditProjection};

/// Secret-safe datasource details used to populate an edit form.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasourceEditProjection {
    /// Opaque datasource id.
    pub id: String,
    /// User-visible datasource name.
    pub name: String,
    /// Rust/native or compatibility driver identity.
    pub driver_id: String,
    /// JDBC URL with userinfo, fragments, and sensitive query values removed.
    pub jdbc_url: String,
    /// Non-secret database username, when one is configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Ordered non-sensitive properties, excluding the separately projected username.
    pub properties: Vec<PortableDatasourceProperty>,
    /// Whether sessions opened from this datasource must be read-only.
    pub read_only: bool,
    /// Optional non-secret SSH settings used to populate the retained connection form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshTunnelEditProjection>,
    /// Whether a complete connection descriptor exists in the vault.
    pub has_secret: bool,
    /// Monotonic revision encoded as a decimal integer.
    pub revision: String,
}

impl Debug for DatasourceEditProjection {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DatasourceEditProjection")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("driver_id", &self.driver_id)
            .field("jdbc_url", &"[REDACTED]")
            .field("username", &self.username.as_ref().map(|_| "[REDACTED]"))
            .field("properties", &self.properties)
            .field("read_only", &self.read_only)
            .field("ssh", &self.ssh)
            .field("has_secret", &self.has_secret)
            .field("revision", &self.revision)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::DatasourceEditProjection;
    use crate::PortableDatasourceProperty;

    #[test]
    fn edit_projection_serializes_only_explicitly_safe_connection_fields() {
        let projection = DatasourceEditProjection {
            id: "datasource-1".to_owned(),
            name: "Production".to_owned(),
            driver_id: "mysql".to_owned(),
            jdbc_url: "jdbc:mysql://localhost/demo?useSSL=false".to_owned(),
            username: Some("sentinel-user".to_owned()),
            properties: vec![PortableDatasourceProperty {
                key: "connectionTimeZone".to_owned(),
                value: "sentinel-zone".to_owned(),
            }],
            read_only: false,
            ssh: None,
            has_secret: true,
            revision: "3".to_owned(),
        };

        let json = serde_json::to_string(&projection).expect("projection serializes");
        assert!(json.contains("sentinel-user"));
        assert!(json.contains("sentinel-zone"));
        for forbidden in ["password", "token", "credential", "privateKey"] {
            assert!(
                !json
                    .to_ascii_lowercase()
                    .contains(&forbidden.to_ascii_lowercase())
            );
        }

        let debug = format!("{projection:?}");
        assert!(!debug.contains("sentinel-user"));
        assert!(!debug.contains("sentinel-zone"));
        assert!(!debug.contains("jdbc:mysql"));
    }
}
