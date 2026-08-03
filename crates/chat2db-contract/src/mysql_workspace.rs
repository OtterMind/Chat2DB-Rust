use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Community-compatible locator used by `MySQL` table pin operations.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityPinnedTableRequest {
    pub data_source_id: String,
    #[serde(default)]
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
    #[serde(default)]
    pub table_name: String,
}

/// Stable pinned-table-name collection for one `MySQL` database/schema scope.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityPinnedTableList {
    pub items: Vec<String>,
}

/// Community-compatible request for one `MySQL` ER model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityErQueryRequest {
    pub data_source_id: String,
    #[serde(default)]
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
}

/// Community-compatible request that persists an ER canvas layout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityErPositionRequest {
    pub data_source_id: String,
    #[serde(default)]
    pub database_name: String,
    #[serde(default)]
    pub schema_name: String,
    #[serde(default)]
    pub position: String,
}

/// Column projection consumed by the retained Community ER canvas.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityErColumn {
    pub name: String,
    pub column_type: String,
    pub primary_key: bool,
    pub comment: String,
}

/// Foreign-key edge projection consumed by the retained Community ER canvas.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityErForeignKey {
    pub pk_table_name: String,
    pub pk_column_name: String,
    pub fk_table_name: String,
    pub fk_column_name: String,
}

/// One table node in the retained Community ER canvas.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityErTable {
    pub name: String,
    pub comment: String,
    pub column_list: Vec<CommunityErColumn>,
    pub foreign_key_list: Vec<CommunityErForeignKey>,
}

/// Complete `MySQL` ER metadata plus the caller's last persisted canvas layout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityErModel {
    pub tables: Vec<CommunityErTable>,
    pub position: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{CommunityErColumn, CommunityErForeignKey, CommunityErModel, CommunityErTable};

    #[test]
    fn er_model_uses_the_exact_retained_frontend_field_names() {
        let model = CommunityErModel {
            tables: vec![CommunityErTable {
                name: "orders".to_owned(),
                comment: String::new(),
                column_list: vec![CommunityErColumn {
                    name: "id".to_owned(),
                    column_type: "BIGINT".to_owned(),
                    primary_key: true,
                    comment: String::new(),
                }],
                foreign_key_list: vec![CommunityErForeignKey {
                    pk_table_name: "users".to_owned(),
                    pk_column_name: "id".to_owned(),
                    fk_table_name: "orders".to_owned(),
                    fk_column_name: "user_id".to_owned(),
                }],
            }],
            position: None,
        };
        let value = serde_json::to_value(model).expect("ER model serializes");
        assert_eq!(value["tables"][0]["columnList"][0]["primaryKey"], true);
        assert_eq!(
            value["tables"][0]["foreignKeyList"][0]["pkTableName"],
            "users"
        );
        assert!(value["position"].is_null());
    }
}
