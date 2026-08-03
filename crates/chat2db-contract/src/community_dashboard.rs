use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

/// One Community dashboard persisted in the local workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityDashboard {
    pub id: i64,
    pub gmt_create: i64,
    pub gmt_modified: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub data_source_collection_id: Option<i64>,
    #[serde(default)]
    pub chart_ids: Vec<i64>,
    pub schema: Option<String>,
    pub refresh_type: Option<String>,
    pub refresh_cycle: Option<Value>,
    pub user_id: Option<i64>,
}

/// Community-compatible dashboard list query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityDashboardListQuery {
    #[serde(default = "default_page_no")]
    pub page_no: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    #[serde(default)]
    pub search_key: Option<String>,
}

impl Default for CommunityDashboardListQuery {
    fn default() -> Self {
        Self {
            page_no: default_page_no(),
            page_size: default_page_size(),
            search_key: None,
        }
    }
}

/// One stable page of Community dashboards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityDashboardPage {
    pub data: Vec<CommunityDashboard>,
    pub total: u64,
    pub page_no: u32,
    pub page_size: u32,
    pub has_next_page: bool,
}

/// Fields accepted when a Community dashboard is created.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCommunityDashboardRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub data_source_collection_id: Option<i64>,
    #[serde(default)]
    pub chart_ids: Vec<i64>,
    pub schema: Option<String>,
    pub refresh_type: Option<String>,
    pub refresh_cycle: Option<Value>,
    pub user_id: Option<i64>,
}

/// Non-null partial Community dashboard update.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCommunityDashboardRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub data_source_collection_id: Option<i64>,
    pub chart_ids: Option<Vec<i64>>,
    pub schema: Option<String>,
    pub refresh_type: Option<String>,
    pub refresh_cycle: Option<Value>,
    pub user_id: Option<i64>,
}

/// One Community chart persisted in the local workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityChart {
    pub id: i64,
    pub gmt_create: i64,
    pub gmt_modified: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub schema: Option<String>,
    pub data_source_id: Option<i64>,
    pub data_source_name: Option<String>,
    pub schema_name: Option<String>,
    pub r#type: Option<String>,
    pub database_name: Option<String>,
    pub ddl: Option<String>,
    pub deleted: Option<String>,
    pub user_id: Option<i64>,
    pub chart_schema: Option<Value>,
    pub meta_data: Option<Value>,
    pub database_info: Option<Value>,
    pub refresh_type: Option<String>,
    pub refresh_cycle: Option<Value>,
}

/// Fields accepted when a Community chart is created.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCommunityChartRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub schema: Option<String>,
    pub data_source_id: Option<i64>,
    pub data_source_name: Option<String>,
    pub schema_name: Option<String>,
    pub r#type: Option<String>,
    pub database_name: Option<String>,
    pub ddl: Option<String>,
    pub deleted: Option<String>,
    pub user_id: Option<i64>,
    pub chart_schema: Option<Value>,
    pub meta_data: Option<Value>,
    pub database_info: Option<Value>,
    pub refresh_type: Option<String>,
    pub refresh_cycle: Option<Value>,
}

/// Non-null partial Community chart update.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCommunityChartRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub schema: Option<String>,
    pub data_source_id: Option<i64>,
    pub data_source_name: Option<String>,
    pub schema_name: Option<String>,
    pub r#type: Option<String>,
    pub database_name: Option<String>,
    pub ddl: Option<String>,
    pub deleted: Option<String>,
    pub user_id: Option<i64>,
    pub chart_schema: Option<Value>,
    pub meta_data: Option<Value>,
    pub database_info: Option<Value>,
    pub refresh_type: Option<String>,
    pub refresh_cycle: Option<Value>,
}

/// Community chart-detail query, including the optional SQL refresh switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityChartDetailQuery {
    pub chart_id: i64,
    #[serde(default)]
    pub refresh: bool,
}

const fn default_page_no() -> u32 {
    1
}

const fn default_page_size() -> u32 {
    20
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CommunityChart, CommunityDashboardListQuery, CreateCommunityDashboardRequest};

    #[test]
    fn dashboard_contract_uses_community_camel_case_and_defaults() {
        let query: CommunityDashboardListQuery =
            serde_json::from_value(json!({})).expect("default query decodes");
        assert_eq!(query.page_no, 1);
        assert_eq!(query.page_size, 20);

        let request: CreateCommunityDashboardRequest = serde_json::from_value(json!({
            "name": "Sales",
            "refreshCycle": {"unit": "seconds", "value": 30}
        }))
        .expect("dashboard request decodes");
        assert!(request.chart_ids.is_empty());
        let encoded = serde_json::to_value(request).expect("dashboard request encodes");
        assert_eq!(encoded["name"], "Sales");
        assert_eq!(encoded["refreshCycle"]["value"], 30);
        assert!(encoded.get("chart_ids").is_none());
    }

    #[test]
    fn chart_json_fields_round_trip_without_stringification() {
        let source = json!({
            "id": 9,
            "gmtCreate": 10,
            "gmtModified": 11,
            "name": "Revenue",
            "description": null,
            "schema": null,
            "dataSourceId": 12,
            "dataSourceName": "MySQL",
            "schemaName": null,
            "type": "BAR",
            "databaseName": "analytics",
            "ddl": "select 1",
            "deleted": "N",
            "userId": null,
            "chartSchema": {"title": "Revenue", "series": [1, 2]},
            "metaData": {"dataList": [{"amount": 42}]},
            "databaseInfo": {"sql": "select 1"},
            "refreshType": "MANUAL",
            "refreshCycle": {"cron": "0 * * * *"}
        });
        let chart: CommunityChart = serde_json::from_value(source.clone()).expect("chart decodes");
        assert_eq!(
            serde_json::to_value(chart).expect("chart re-encodes"),
            source
        );
    }
}
