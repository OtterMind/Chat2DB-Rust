use std::fmt::{Debug, Formatter};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{CommunityDatabase, Datasource, SshTunnelEditProjection};

/// Request to clone one datasource under a new opaque id and vault reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CloneDatasourceRequest {
    /// Source datasource id.
    pub id: String,
    /// Optional replacement name. The source name plus ` Copy` is used when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Request to export selected datasources, or every datasource when the list is empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportCommunityDatasourcesRequest {
    /// Selected opaque datasource ids. Empty selects all datasources.
    #[serde(default)]
    pub datasource_ids: Vec<String>,
}

/// Portable non-sensitive connection property.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PortableDatasourceProperty {
    /// Driver property name.
    pub key: String,
    /// Non-sensitive value. Import rejects credential-like keys.
    pub value: String,
}

impl Debug for PortableDatasourceProperty {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PortableDatasourceProperty")
            .field("key", &self.key)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

/// Credential-free connection descriptor suitable for an explicit export file.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PortableDatasourceConnection {
    /// JDBC URL with userinfo and sensitive query parameters removed.
    pub jdbc_url: String,
    /// Ordered non-sensitive connection properties.
    pub properties: Vec<PortableDatasourceProperty>,
    /// Whether the imported datasource should remain read-only.
    pub read_only: bool,
    /// Optional non-secret SSH endpoint and authentication mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshTunnelEditProjection>,
}

impl Debug for PortableDatasourceConnection {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PortableDatasourceConnection")
            .field("jdbc_url", &"[REDACTED]")
            .field("properties", &self.properties)
            .field("read_only", &self.read_only)
            .field("ssh", &self.ssh)
            .finish()
    }
}

/// One datasource in the portable Community JSON document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PortableCommunityDatasource {
    /// Original id retained for traceability only. Import never updates this id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    /// User-visible datasource name.
    #[serde(alias = "alias")]
    pub name: String,
    /// Rust/native or compatibility driver identity.
    pub driver_id: String,
    /// Optional credential-free connection descriptor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<PortableDatasourceConnection>,
}

/// Versioned, secret-safe Community datasource export document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityDatasourceExport {
    /// Document schema version. Version `1` is the only currently accepted value.
    pub schema_version: u32,
    /// Export time as Unix epoch milliseconds encoded as a decimal integer.
    pub exported_at_ms: String,
    /// Selected datasource definitions.
    pub datasources: Vec<PortableCommunityDatasource>,
}

/// Result of importing a portable Community datasource document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityDatasourceImportResult {
    /// Number of new datasource records created.
    pub count: u32,
    /// Secret-free metadata for the new records.
    pub created: Vec<Datasource>,
}

/// Connection ownership model exposed to the retained Community client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DatasourceSessionMode {
    /// Every operation owns and closes its database connection.
    Ephemeral,
}

/// Result of Community's datasource `connect` compatibility operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasourceConnectResult {
    /// Opaque datasource id that was connected and inspected.
    pub datasource_id: String,
    /// Explicit connection ownership model.
    pub session_mode: DatasourceSessionMode,
    /// Databases returned by the real ephemeral connection.
    pub databases: Vec<CommunityDatabase>,
}

/// Result of Community's Console `connect` compatibility operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleConnectResult {
    /// Opaque datasource id whose connection was verified.
    pub datasource_id: String,
    /// Explicit connection ownership model.
    pub session_mode: DatasourceSessionMode,
    /// True only after a real database connection and ping succeeded.
    pub verified: bool,
}

/// Result of explicit datasource or generic connection close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasourceCloseResult {
    /// Opaque datasource id accepted by the close operation.
    pub datasource_id: String,
    /// Explicit connection ownership model.
    pub session_mode: DatasourceSessionMode,
    /// Always zero while connections are operation-scoped and already closed.
    pub closed_connections: u32,
}

/// Frontend-requested mutation of a driver artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NativeDriverAction {
    /// Download an advertised driver.
    Download,
    /// Save selected custom artifacts.
    Save,
    /// Delete selected custom artifacts.
    Delete,
}

/// Explicit response for driver actions satisfied by a native Rust implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NativeDriverCompatibility {
    /// Normalized Community database type.
    pub database_type: String,
    /// Stable Rust driver identity.
    pub driver_id: String,
    /// Requested frontend action.
    pub action: NativeDriverAction,
    /// Native implementation name.
    pub implementation: String,
    /// Always false: native `MySQL` never needs a Java JAR.
    pub artifact_required: bool,
    /// Always false: the native implementation is immutable at runtime.
    pub changed: bool,
}

#[cfg(test)]
mod tests {
    use super::{PortableDatasourceConnection, PortableDatasourceProperty};

    #[test]
    fn portable_connection_debug_redacts_values() {
        let connection = PortableDatasourceConnection {
            jdbc_url: "jdbc:mysql://localhost/demo".to_owned(),
            properties: vec![PortableDatasourceProperty {
                key: "user".to_owned(),
                value: "sentinel-user".to_owned(),
            }],
            read_only: false,
            ssh: None,
        };
        let debug = format!("{connection:?}");
        assert!(!debug.contains("sentinel-user"));
        assert!(!debug.contains("jdbc:mysql"));
    }
}
