use std::collections::HashMap;

use chat2db_contract::{
    ApiError, CommunityChart, CommunityDashboard, CommunityDashboardListQuery,
    CommunityDashboardPage, CreateCommunityChartRequest, CreateCommunityDashboardRequest,
    JdbcValue, ResultColumn, UpdateCommunityChartRequest, UpdateCommunityDashboardRequest,
};
use chat2db_storage::CreateOperationLog;
use serde_json::{Map, Value, json};
use sqlparser::{
    ast::{Expr, SelectItem, SetExpr, Statement, TableFactor},
    dialect::MySqlDialect,
    parser::Parser,
};

use crate::{
    AppError, AppErrorKind, Application, NativeConsoleCancellation, NativeConsoleRequest,
    NativeConsoleResult,
    native_driver_types::{ColumnMetadata, ListColumnsRequest, MetadataScope, TableRef},
    now_millis, storage_call,
};

const CHART_PAGE_SIZE: u32 = 200;
const MAX_CHART_METADATA_BYTES: usize = 8 * 1024 * 1024;

impl Application {
    /// Lists durable Community dashboards using the historical stable paging contract.
    ///
    /// # Errors
    ///
    /// Returns validation, availability, or durable-storage failures.
    pub async fn list_community_dashboards(
        &self,
        query: CommunityDashboardListQuery,
    ) -> Result<CommunityDashboardPage, AppError> {
        let storage = self.require_storage()?;
        storage_call(move || storage.list_community_dashboards(&query)).await
    }

    /// Returns one dashboard or `None` when its id is absent.
    ///
    /// # Errors
    ///
    /// Returns validation, availability, or durable-storage failures.
    pub async fn get_community_dashboard(
        &self,
        id: i64,
    ) -> Result<Option<CommunityDashboard>, AppError> {
        let storage = self.require_storage()?;
        storage_call(move || storage.get_community_dashboard(id)).await
    }

    /// Creates one durable Community dashboard and returns its numeric id.
    ///
    /// # Errors
    ///
    /// Returns validation, availability, or durable-storage failures.
    pub async fn create_community_dashboard(
        &self,
        request: CreateCommunityDashboardRequest,
    ) -> Result<i64, AppError> {
        let storage = self.require_storage()?;
        storage_call(move || storage.create_community_dashboard(request)).await
    }

    /// Applies Community's non-null partial dashboard update.
    ///
    /// # Errors
    ///
    /// Returns validation, not-found, availability, or durable-storage failures.
    pub async fn update_community_dashboard(
        &self,
        id: i64,
        request: UpdateCommunityDashboardRequest,
    ) -> Result<(), AppError> {
        let storage = self.require_storage()?;
        storage_call(move || storage.update_community_dashboard(id, request)).await
    }

    /// Deletes a dashboard and its chart relations.
    ///
    /// # Errors
    ///
    /// Returns validation, availability, or durable-storage failures.
    pub async fn delete_community_dashboard(&self, id: i64) -> Result<bool, AppError> {
        let storage = self.require_storage()?;
        storage_call(move || storage.delete_community_dashboard(id)).await
    }

    /// Returns one durable Community chart without executing its SQL.
    ///
    /// # Errors
    ///
    /// Returns validation, availability, or durable-storage failures.
    pub async fn get_community_chart(&self, id: i64) -> Result<Option<CommunityChart>, AppError> {
        let storage = self.require_storage()?;
        storage_call(move || storage.get_community_chart(id)).await
    }

    /// Returns a detached chart copy, optionally refreshing its result through a native driver.
    ///
    /// # Errors
    ///
    /// Returns validation, datasource, native-driver, result-limit, or durable-storage failures.
    pub async fn get_community_chart_detail(
        &self,
        id: i64,
        refresh: bool,
    ) -> Result<Option<CommunityChart>, AppError> {
        let Some(mut chart) = self.get_community_chart(id).await? else {
            return Ok(None);
        };
        if !refresh {
            return Ok(Some(chart));
        }
        let Some(context) = chart_refresh_context(&chart) else {
            return Ok(Some(chart));
        };

        let database_type = match self.native_chart_database_type(&context).await {
            Ok(database_type) => database_type,
            Err(error) => {
                self.record_chart_history(&chart, &context, None, None, Some(&error))
                    .await;
                return Err(error);
            }
        };

        let execution = self
            .execute_native_read_console(
                NativeConsoleRequest {
                    datasource_id: context.datasource_id.clone(),
                    database_name: context.database_name.clone().unwrap_or_default(),
                    sql: context.sql.clone(),
                    page_no: 1,
                    page_size: CHART_PAGE_SIZE,
                    result_set_id: None,
                    single: true,
                    page_size_all: false,
                    explain: false,
                    error_continue: false,
                },
                NativeConsoleCancellation::new(),
            )
            .await;

        let result = match execution {
            Ok(results) => {
                let Some(result) = results.into_iter().next() else {
                    let error = AppError::unavailable(
                        "chart_query_incomplete",
                        "The chart query completed without a result",
                    );
                    self.record_chart_history(
                        &chart,
                        &context,
                        Some(&database_type),
                        None,
                        Some(&error),
                    )
                    .await;
                    return Err(error);
                };
                if result.success {
                    result
                } else {
                    let error = AppError::invalid(
                        "chart_query_failed",
                        result
                            .error
                            .as_ref()
                            .map_or_else(|| result.message.clone(), |error| error.message.clone()),
                    );
                    self.record_chart_history(
                        &chart,
                        &context,
                        Some(&database_type),
                        Some(&result),
                        Some(&error),
                    )
                    .await;
                    return Err(error);
                }
            }
            Err(error) => {
                self.record_chart_history(
                    &chart,
                    &context,
                    Some(&database_type),
                    None,
                    Some(&error),
                )
                .await;
                return Err(error);
            }
        };
        let header_metadata = self.chart_header_metadata(&context).await;
        chart.meta_data = Some(chart_metadata(&result, header_metadata.as_ref())?);
        self.record_chart_history(&chart, &context, Some(&database_type), Some(&result), None)
            .await;
        Ok(Some(chart))
    }

    async fn native_chart_database_type(
        &self,
        context: &ChartRefreshContext,
    ) -> Result<String, AppError> {
        self.require_native_driver_for_datasource(&context.datasource_id)
            .await?
            .descriptor()
            .database_types
            .first()
            .copied()
            .map(str::to_owned)
            .ok_or_else(AppError::internal)
    }

    /// Creates one durable Community chart and returns its numeric id.
    ///
    /// # Errors
    ///
    /// Returns validation, availability, or durable-storage failures.
    pub async fn create_community_chart(
        &self,
        request: CreateCommunityChartRequest,
    ) -> Result<i64, AppError> {
        let storage = self.require_storage()?;
        storage_call(move || storage.create_community_chart(request)).await
    }

    /// Applies Community's non-null partial chart update.
    ///
    /// # Errors
    ///
    /// Returns validation, not-found, availability, or durable-storage failures.
    pub async fn update_community_chart(
        &self,
        id: i64,
        request: UpdateCommunityChartRequest,
    ) -> Result<(), AppError> {
        let storage = self.require_storage()?;
        storage_call(move || storage.update_community_chart(id, request)).await
    }

    /// Deletes one durable Community chart.
    ///
    /// # Errors
    ///
    /// Returns validation, availability, or durable-storage failures.
    pub async fn delete_community_chart(&self, id: i64) -> Result<bool, AppError> {
        let storage = self.require_storage()?;
        storage_call(move || storage.delete_community_chart(id)).await
    }

    async fn record_chart_history(
        &self,
        chart: &CommunityChart,
        context: &ChartRefreshContext,
        database_type: Option<&str>,
        result: Option<&NativeConsoleResult>,
        error: Option<&AppError>,
    ) {
        let Some(storage) = self.storage().cloned() else {
            return;
        };
        let extend_info = serde_json::to_string(&json!({
            "source": "CHART",
            "chartId": chart.id,
            "consoleId": context.console_id,
            "message": error.map(|error| error.api_error().message),
        }))
        .ok();
        let input = CreateOperationLog {
            name: chart.name.clone(),
            data_source_id: Some(context.datasource_id.clone()),
            data_source_name: chart.data_source_name.clone(),
            connectable: Some(true),
            database_name: context.database_name.clone(),
            database_type: database_type.map(str::to_owned),
            ddl: context.sql.clone(),
            status: if error.is_none() { "success" } else { "fail" }.to_owned(),
            operation_rows: result.and_then(|result| i64::try_from(result.row_count).ok()),
            use_time: result.and_then(|result| i64::try_from(result.duration_ms).ok()),
            extend_info,
            schema_name: context.schema_name.clone(),
            organization_id: None,
            user_name: None,
            more: context.sql.chars().count() > 200,
            operation_type: "SQL_EXECUTE".to_owned(),
        };
        let write = tokio::task::spawn_blocking(move || storage.create_operation_log(input)).await;
        match write {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::warn!(%error, "chart query history write failed"),
            Err(error) => tracing::warn!(%error, "chart query history task failed"),
        }
    }

    async fn chart_header_metadata(
        &self,
        context: &ChartRefreshContext,
    ) -> Option<HashMap<String, ColumnMetadata>> {
        let table = chart_editable_table(&context.sql)?;
        let database_name = table
            .database_name
            .or_else(|| context.database_name.clone())?;
        let schema_name = context
            .schema_name
            .clone()
            .unwrap_or_else(|| database_name.clone());
        let columns = match async {
            let driver = self
                .require_native_driver_for_datasource(&context.datasource_id)
                .await?;
            let metadata = driver.metadata().ok_or_else(|| {
                AppError::invalid(
                    "native_metadata_capability_not_available",
                    "The native Rust driver does not implement metadata operations",
                )
            })?;
            metadata
                .list_columns(
                    self,
                    ListColumnsRequest {
                        table: TableRef {
                            scope: MetadataScope {
                                datasource_id: context.datasource_id.clone(),
                                database_name,
                                schema_name,
                            },
                            table_name: table.table_name,
                        },
                    },
                )
                .await
        }
        .await
        {
            Ok(columns) => columns,
            Err(error) => {
                tracing::warn!(%error, "chart header metadata enhancement failed");
                return None;
            }
        };
        Some(
            columns
                .items
                .into_iter()
                .map(|column| (column.name.to_ascii_lowercase(), column))
                .collect(),
        )
    }
}

#[derive(Debug)]
struct ChartRefreshContext {
    datasource_id: String,
    database_name: Option<String>,
    schema_name: Option<String>,
    sql: String,
    console_id: String,
}

#[derive(Debug, PartialEq, Eq)]
struct ChartEditableTable {
    database_name: Option<String>,
    table_name: String,
}

fn chart_refresh_context(chart: &CommunityChart) -> Option<ChartRefreshContext> {
    let database_info = json_object(chart.database_info.as_ref()?)?;
    let datasource_id = json_identifier(database_info.get("dataSourceId")?)?;
    let sql = database_info.get("sql")?.as_str()?.trim();
    if sql.is_empty() {
        return None;
    }
    let database_name = json_non_blank(database_info.get("databaseName"));
    let schema_name = json_non_blank(database_info.get("schemaName"));
    let console_id = database_info
        .get("consoleId")
        .and_then(json_identifier)
        .unwrap_or_else(|| now_millis().unwrap_or_default().to_string());
    Some(ChartRefreshContext {
        datasource_id,
        database_name,
        schema_name,
        sql: sql.to_owned(),
        console_id,
    })
}

fn json_object(value: &Value) -> Option<&Map<String, Value>> {
    match value {
        Value::Object(object) => Some(object),
        _ => None,
    }
}

fn json_identifier(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn json_non_blank(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn chart_editable_table(sql: &str) -> Option<ChartEditableTable> {
    let statements = Parser::parse_sql(&MySqlDialect {}, sql).ok()?;
    let [Statement::Query(query)] = statements.as_slice() else {
        return None;
    };
    if query.with.is_some() {
        return None;
    }
    let SetExpr::Select(select) = query.body.as_ref() else {
        return None;
    };
    let [from] = select.from.as_slice() else {
        return None;
    };
    if !from.joins.is_empty() || select.projection.iter().any(non_editable_projection) {
        return None;
    }
    let TableFactor::Table { name, args, .. } = &from.relation else {
        return None;
    };
    if args.is_some() {
        return None;
    }
    let identifiers = name
        .0
        .iter()
        .filter_map(|part| part.as_ident())
        .map(|identifier| identifier.value.clone())
        .collect::<Vec<_>>();
    let table_name = identifiers.last()?.clone();
    let database_name = identifiers
        .len()
        .checked_sub(2)
        .and_then(|index| identifiers.get(index).cloned());
    Some(ChartEditableTable {
        database_name,
        table_name,
    })
}

fn non_editable_projection(item: &SelectItem) -> bool {
    match item {
        SelectItem::ExprWithAlias { .. } | SelectItem::ExprWithAliases { .. } => true,
        SelectItem::UnnamedExpr(Expr::Function(function)) => function
            .name
            .0
            .last()
            .and_then(|part| part.as_ident())
            .is_some_and(|identifier| identifier.value.eq_ignore_ascii_case("count")),
        SelectItem::UnnamedExpr(_)
        | SelectItem::QualifiedWildcard(_, _)
        | SelectItem::Wildcard(_) => false,
    }
}

fn chart_metadata(
    result: &NativeConsoleResult,
    header_metadata: Option<&HashMap<String, ColumnMetadata>>,
) -> Result<Value, AppError> {
    let metadata = json!({
        "dataList": result
            .rows
            .iter()
            .map(|row| row.values.iter().map(chart_value).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        "headerList": result
            .columns
            .iter()
            .map(|column| {
                let metadata = header_metadata
                    .and_then(|columns| columns.get(&column.name.to_ascii_lowercase()));
                chart_header(column, metadata)
            })
            .collect::<Vec<_>>(),
    });
    let encoded_bytes = serde_json::to_vec(&metadata)
        .map_err(|_| AppError::internal())?
        .len();
    if encoded_bytes > MAX_CHART_METADATA_BYTES {
        return Err(AppError::new(
            AppErrorKind::ResourceExhausted,
            ApiError::new(
                "chart_result_too_large",
                "The chart result exceeds the 8 MiB response limit",
            ),
        ));
    }
    Ok(metadata)
}

fn chart_header(column: &ResultColumn, metadata: Option<&ColumnMetadata>) -> Value {
    let column_type = metadata.map_or(column.jdbc_type_name.as_str(), |column| {
        column.column_type.as_str()
    });
    json!({
        "dataType": chart_data_type(column),
        "name": column.label,
        "columnName": column.name,
        "columnType": column_type,
        "tableName": column.table_name,
        "databaseName": column.catalog_name,
        "schemaName": column.schema_name,
        "primaryKey": metadata.and_then(|column| column.primary_key),
        "comment": metadata.map(|column| column.comment.as_str()),
        "defaultValue": metadata.and_then(|column| column.default_value.as_deref()),
        "autoIncrement": metadata
            .and_then(|column| column.auto_increment)
            .map_or(0, i32::from),
        "nullable": metadata.and_then(|column| column.nullable),
        "columnSize": metadata.and_then(|column| column.column_size),
        "decimalDigits": metadata.and_then(|column| column.decimal_digits),
        "editorType": chart_editor_type(column_type, column.jdbc_type),
    })
}

fn chart_value(value: &JdbcValue) -> Value {
    match value {
        JdbcValue::Null => Value::Null,
        JdbcValue::Boolean { value } => Value::String(value.to_string()),
        JdbcValue::SignedInteger { value }
        | JdbcValue::UnsignedInteger { value }
        | JdbcValue::Float32 { value }
        | JdbcValue::Float64 { value }
        | JdbcValue::Decimal { value }
        | JdbcValue::Text { value }
        | JdbcValue::Binary { value }
        | JdbcValue::Date { value }
        | JdbcValue::Time { value }
        | JdbcValue::Timestamp { value }
        | JdbcValue::TimestampWithTimeZone { value }
        | JdbcValue::Json { value }
        | JdbcValue::Uuid { value } => Value::String(value.clone()),
        JdbcValue::Opaque { display_value, .. } => Value::String(display_value.clone()),
    }
}

fn chart_data_type(column: &ResultColumn) -> &'static str {
    let type_name = column.jdbc_type_name.to_ascii_uppercase();
    let jdbc_type = match column.jdbc_type {
        12 | 1111 if type_name == "BLOB" => 2004,
        12 | 1111 if type_name == "CLOB" => 2005,
        12 | 1111 if type_name == "NCLOB" => 2011,
        -7 if type_name == "TINYINT" => -6,
        value => value,
    };
    match jdbc_type {
        16 => "BOOLEAN",
        1 | 12 | -9 | -1 | -16 => "STRING",
        -5 | 3 | 8 | 6 | 4 | 2 | 7 | 5 => "NUMERIC",
        -7 => "BIT",
        -6 if type_name.contains("BOOL") => "BOOLEAN",
        -6 => "NUMERIC",
        91 | 92 | 93 | 2013 | 2014 => "DATETIME",
        -4..=-2 => "BINARY",
        2004 | 2005 | 2011 | 2009 => "CONTENT",
        2002 => "STRUCT",
        2003 => "ARRAY",
        -8 => "ROWID",
        2006 => "REFERENCE",
        1111 => "OBJECT",
        _ => "UNKNOWN",
    }
}

fn chart_editor_type(type_name: &str, jdbc_type: i32) -> &'static str {
    let normalized = type_name
        .split(['(', ' '])
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    match normalized.as_str() {
        "DATE" => "DATE",
        "TIME" => "TIME",
        "DATETIME" => "DATETIME",
        "TIMESTAMP" => "TIMESTAMP",
        _ => match jdbc_type {
            91 => "DATE",
            92 => "TIME",
            93 => "TIMESTAMP",
            _ => "TEXT",
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chat2db_contract::{ColumnNullability, JdbcValue, JdbcValueType, ResultColumn, ResultRow};
    use serde_json::json;

    use crate::native_driver_types::ColumnMetadata;

    use super::{chart_data_type, chart_editable_table, chart_metadata, chart_refresh_context};

    #[test]
    fn chart_context_accepts_numeric_community_ids_and_selected_database() {
        let chart = chat2db_contract::CommunityChart {
            id: 7,
            gmt_create: 1,
            gmt_modified: 1,
            name: None,
            description: None,
            schema: None,
            data_source_id: Some(42),
            data_source_name: None,
            schema_name: None,
            r#type: None,
            database_name: None,
            ddl: None,
            deleted: None,
            user_id: None,
            chart_schema: None,
            meta_data: None,
            database_info: Some(json!({
                "dataSourceId": 42,
                "databaseName": "analytics",
                "schemaName": "analytics",
                "consoleId": "9007199254740993",
                "sql": "SELECT 1"
            })),
            refresh_type: None,
            refresh_cycle: None,
        };
        let context = chart_refresh_context(&chart).expect("chart context");
        assert_eq!(context.datasource_id, "42");
        assert_eq!(context.database_name.as_deref(), Some("analytics"));
        assert_eq!(context.console_id, "9007199254740993");
    }

    #[test]
    fn chart_context_never_falls_back_to_stale_top_level_names() {
        let chart = chat2db_contract::CommunityChart {
            id: 8,
            gmt_create: 1,
            gmt_modified: 1,
            name: None,
            description: None,
            schema: None,
            data_source_id: Some(42),
            data_source_name: None,
            database_name: Some("stale_database".to_owned()),
            schema_name: Some("stale_schema".to_owned()),
            r#type: None,
            ddl: None,
            deleted: None,
            user_id: None,
            chart_schema: None,
            meta_data: None,
            database_info: Some(json!({
                "dataSourceId": 42,
                "sql": "SELECT 1"
            })),
            refresh_type: None,
            refresh_cycle: None,
        };
        let context = chart_refresh_context(&chart).expect("chart context");
        assert_eq!(context.database_name, None);
        assert_eq!(context.schema_name, None);
    }

    #[test]
    fn chart_metadata_matches_community_display_shape() {
        let result = crate::NativeConsoleResult {
            statement_sequence: 1,
            result_set_id: Some(1),
            sql: "SELECT amount, note".to_owned(),
            success: true,
            message: String::new(),
            update_count: 0,
            columns: vec![ResultColumn {
                ordinal: 1,
                label: "amount".to_owned(),
                name: "amount".to_owned(),
                jdbc_type: 3,
                jdbc_type_name: "DECIMAL".to_owned(),
                value_type: JdbcValueType::Decimal,
                nullability: ColumnNullability::Nullable,
                precision: Some(10),
                scale: Some(2),
                display_size: Some(12),
                signed: Some(true),
                catalog_name: Some("analytics".to_owned()),
                schema_name: None,
                table_name: Some("metrics".to_owned()),
            }],
            rows: vec![ResultRow {
                values: vec![JdbcValue::Decimal {
                    value: "42.50".to_owned(),
                }],
            }],
            row_count: 1,
            has_more: false,
            duration_ms: 3,
            error: None,
        };
        let header_metadata = HashMap::from([(
            "amount".to_owned(),
            ColumnMetadata {
                name: "amount".to_owned(),
                column_type: "DECIMAL".to_owned(),
                auto_increment: Some(false),
                comment: "Invoice amount".to_owned(),
                primary_key: Some(false),
                column_size: Some(10),
                decimal_digits: Some(2),
                nullable: Some(1),
                ..ColumnMetadata::default()
            },
        )]);
        let metadata = chart_metadata(&result, Some(&header_metadata)).expect("chart metadata");
        assert_eq!(metadata["dataList"], json!([["42.50"]]));
        assert_eq!(metadata["headerList"][0]["name"], "amount");
        assert_eq!(metadata["headerList"][0]["dataType"], "NUMERIC");
        assert_eq!(metadata["headerList"][0]["nullable"], 1);
        assert_eq!(metadata["headerList"][0]["autoIncrement"], 0);
        assert_eq!(metadata["headerList"][0]["primaryKey"], false);
        assert_eq!(metadata["headerList"][0]["comment"], "Invoice amount");
        assert_eq!(metadata["headerList"][0]["editorType"], "TEXT");
    }

    #[test]
    fn chart_jdbc_type_projection_matches_community() {
        let mut column = result_column(-7, "BIT", JdbcValueType::Boolean);
        assert_eq!(chart_data_type(&column), "BIT");

        column.jdbc_type = -1;
        column.jdbc_type_name = "JSON".to_owned();
        column.value_type = JdbcValueType::Json;
        assert_eq!(chart_data_type(&column), "STRING");

        column.jdbc_type = 93;
        column.jdbc_type_name = "DATETIME".to_owned();
        assert_eq!(
            super::chart_editor_type(&column.jdbc_type_name, column.jdbc_type),
            "DATETIME"
        );

        let result = crate::NativeConsoleResult {
            statement_sequence: 1,
            result_set_id: Some(1),
            sql: "SELECT CAST('2024-01-02' AS DATETIME)".to_owned(),
            success: true,
            message: String::new(),
            update_count: 0,
            columns: vec![column],
            rows: Vec::new(),
            row_count: 0,
            has_more: false,
            duration_ms: 1,
            error: None,
        };
        let metadata = chart_metadata(&result, None).expect("basic chart metadata");
        assert_eq!(metadata["headerList"][0]["primaryKey"], json!(null));
        assert_eq!(metadata["headerList"][0]["nullable"], json!(null));
        assert_eq!(metadata["headerList"][0]["autoIncrement"], 0);
        assert_eq!(metadata["headerList"][0]["editorType"], "DATETIME");
    }

    #[test]
    fn chart_header_enhancement_is_limited_to_simple_editable_tables() {
        let table = chart_editable_table("SELECT id, label FROM analytics.metrics")
            .expect("simple table query");
        assert_eq!(table.database_name.as_deref(), Some("analytics"));
        assert_eq!(table.table_name, "metrics");
        assert!(chart_editable_table("SELECT id AS value FROM metrics").is_none());
        assert!(chart_editable_table("SELECT COUNT(id) FROM metrics").is_none());
        assert!(
            chart_editable_table("WITH cte AS (SELECT id FROM metrics) SELECT id FROM cte")
                .is_none()
        );
        assert!(
            chart_editable_table(
                "SELECT metrics.id FROM metrics JOIN tags ON tags.id = metrics.id"
            )
            .is_none()
        );
    }

    fn result_column(
        jdbc_type: i32,
        jdbc_type_name: &str,
        value_type: JdbcValueType,
    ) -> ResultColumn {
        ResultColumn {
            ordinal: 1,
            label: "value".to_owned(),
            name: "value".to_owned(),
            jdbc_type,
            jdbc_type_name: jdbc_type_name.to_owned(),
            value_type,
            nullability: ColumnNullability::Nullable,
            precision: None,
            scale: None,
            display_size: None,
            signed: None,
            catalog_name: None,
            schema_name: None,
            table_name: None,
        }
    }
}
