use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de::Visitor};
use utoipa::ToSchema;

/// One source or target namespace selected by the retained Community schema-sync UI.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunitySchemaDiffEndpoint {
    #[serde(
        default,
        rename = "dataSourceId",
        alias = "datasourceId",
        deserialize_with = "deserialize_datasource_id"
    )]
    pub datasource_id: String,
    #[serde(default)]
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
}

/// Historical `/api/diff/sql` request, directed from the desired source to the target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunitySchemaDiffRequest {
    pub source: CommunitySchemaDiffEndpoint,
    pub target: CommunitySchemaDiffEndpoint,
}

/// SQL preview returned by the historical endpoint as a JSON string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(transparent)]
pub struct CommunitySchemaDiffSql(pub String);

impl CommunitySchemaDiffSql {
    #[must_use]
    pub fn new(sql: impl Into<String>) -> Self {
        Self(sql.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

fn deserialize_datasource_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    struct DatasourceIdVisitor;

    impl Visitor<'_> for DatasourceIdVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a string or integer datasource id")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
            Ok(value.to_owned())
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
            Ok(value)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
            Ok(value.to_string())
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
            Ok(value.to_string())
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(String::new())
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(String::new())
        }
    }

    deserializer.deserialize_any(DatasourceIdVisitor)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CommunitySchemaDiffRequest, CommunitySchemaDiffSql};

    #[test]
    fn historical_numeric_datasource_ids_deserialize_to_canonical_strings() {
        let request: CommunitySchemaDiffRequest = serde_json::from_value(json!({
            "source": {
                "dataSourceId": 42,
                "databaseName": "source_db",
                "schemaName": ""
            },
            "target": {
                "dataSourceId": "target-uuid",
                "databaseName": "target_db"
            }
        }))
        .expect("historical schema diff request must deserialize");

        assert_eq!(request.source.datasource_id, "42");
        assert_eq!(request.target.datasource_id, "target-uuid");
        assert_eq!(request.target.schema_name, "");
    }

    #[test]
    fn schema_diff_sql_retains_the_historical_json_string_shape() {
        assert_eq!(
            serde_json::to_value(CommunitySchemaDiffSql::new("ALTER TABLE `t` ADD `c` INT;"))
                .expect("schema diff SQL must serialize"),
            json!("ALTER TABLE `t` ADD `c` INT;")
        );
    }
}
