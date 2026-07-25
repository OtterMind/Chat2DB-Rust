use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One hash-verified JDBC driver pack loaded by the compatibility engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JdbcDriver {
    /// Stable package identifier declared by the local manifest.
    pub pack_id: String,
    /// User-visible driver name.
    pub name: String,
    /// Driver-pack version declared by the manifest.
    pub version: String,
    /// Engine-derived identifier stored by datasource records.
    pub driver_id: String,
    /// JDBC driver implementation class.
    pub driver_class: String,
    /// Number of ordered JAR artifacts in this pack.
    pub artifact_count: u32,
    /// Total verified artifact bytes encoded as an unsigned decimal integer.
    pub artifact_bytes: String,
}

/// Stable inventory of drivers loaded during runtime startup.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JdbcDriverList {
    /// Drivers in deterministic manifest-directory order.
    pub items: Vec<JdbcDriver>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{JdbcDriver, JdbcDriverList};

    #[test]
    fn inventory_uses_portable_names_without_local_paths() {
        let inventory = JdbcDriverList {
            items: vec![JdbcDriver {
                pack_id: "postgresql".to_owned(),
                name: "PostgreSQL".to_owned(),
                version: "42.7.7".to_owned(),
                driver_id: "sha256:driver".to_owned(),
                driver_class: "org.postgresql.Driver".to_owned(),
                artifact_count: 1,
                artifact_bytes: "1099511627776".to_owned(),
            }],
        };

        let value = serde_json::to_value(inventory).expect("inventory must serialize");
        assert_eq!(
            value,
            json!({
                "items": [{
                    "packId": "postgresql",
                    "name": "PostgreSQL",
                    "version": "42.7.7",
                    "driverId": "sha256:driver",
                    "driverClass": "org.postgresql.Driver",
                    "artifactCount": 1,
                    "artifactBytes": "1099511627776"
                }]
            })
        );
        let encoded = value.to_string();
        assert!(!encoded.contains("/Users/"));
        assert!(!encoded.contains("artifactPath"));
        assert!(!encoded.contains("sha256Digest"));
    }
}
