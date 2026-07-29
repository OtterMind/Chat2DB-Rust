use std::panic::AssertUnwindSafe;

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    ComponentState, CreateDatasourceRequest, DatasourceConnection, DatasourceConnectionProperty,
};
use chat2db_core::{Application, RuntimeConfig, RuntimeHost};
use chat2db_java_bridge::{EngineCommand, EngineConfig};
use futures_util::FutureExt as _;
use http_body_util::BodyExt as _;
use mysql_async::{Conn, Opts, OptsBuilder, prelude::Queryable as _};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt as _;
use uuid::Uuid;

const REQUIRED_MYSQL_ENV: [&str; 4] = [
    "MYSQL_TEST_HOST",
    "MYSQL_TEST_PORT",
    "MYSQL_TEST_USER",
    "MYSQL_TEST_PASSWORD",
];

struct MysqlTestConfig {
    host: String,
    port: u16,
    user: String,
    password: String,
}

impl MysqlTestConfig {
    fn from_environment() -> Option<Self> {
        let configured = REQUIRED_MYSQL_ENV
            .iter()
            .filter(|name| std::env::var_os(name).is_some())
            .count();
        if configured == 0 {
            eprintln!("skipping editable MySQL Web test; MYSQL_TEST_* variables are absent");
            return None;
        }
        assert_eq!(configured, REQUIRED_MYSQL_ENV.len());
        Some(Self {
            host: required_env("MYSQL_TEST_HOST"),
            port: required_env("MYSQL_TEST_PORT")
                .parse()
                .expect("MYSQL_TEST_PORT must be valid"),
            user: required_env("MYSQL_TEST_USER"),
            password: required_env("MYSQL_TEST_PASSWORD"),
        })
    }

    fn options(&self) -> Opts {
        OptsBuilder::default()
            .ip_or_hostname(self.host.clone())
            .tcp_port(self.port)
            .user(Some(self.user.clone()))
            .pass(Some(self.password.clone()))
            .prefer_socket(Some(false))
            .into()
    }

    fn connection(&self, database_name: Option<&str>) -> DatasourceConnection {
        let host = if self.host.contains(':')
            && !(self.host.starts_with('[') && self.host.ends_with(']'))
        {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        DatasourceConnection {
            jdbc_url: format!(
                "jdbc:mysql://{host}:{}/{database}?useSSL=false&serverTimezone=UTC",
                self.port,
                database = database_name.unwrap_or_default()
            ),
            properties: vec![
                DatasourceConnectionProperty {
                    key: "user".to_owned(),
                    value: self.user.clone(),
                    sensitive: false,
                },
                DatasourceConnectionProperty {
                    key: "password".to_owned(),
                    value: self.password.clone(),
                    sensitive: true,
                },
            ],
            read_only: false,
        }
    }
}

#[tokio::test]
#[ignore = "requires an external MySQL service"]
async fn community_http_editable_grid_and_ddl_keep_java_dormant() {
    let Some(config) = MysqlTestConfig::from_environment() else {
        return;
    };
    let database_name = format!("chat2db_web_it_{}", Uuid::new_v4().simple());
    let verification = AssertUnwindSafe(verify_product_vertical(&config, &database_name))
        .catch_unwind()
        .await;
    let cleanup = cleanup_database(&config, &database_name).await;
    if let Err(payload) = verification {
        if let Err(error) = cleanup {
            eprintln!("editable MySQL cleanup also failed: {error}");
        }
        std::panic::resume_unwind(payload);
    }
    cleanup.expect("editable MySQL fixture must be removed");
}

#[allow(clippy::too_many_lines)]
async fn verify_product_vertical(config: &MysqlTestConfig, database_name: &str) {
    let directory = TempDir::new().expect("temporary Web runtime");
    let runtime = RuntimeConfig::new(EngineConfig::new(EngineCommand::new(
        directory.path().join("missing-java"),
    )))
    .with_data_dir(directory.path().join("data"))
    .with_vault_master_key_base64(STANDARD.encode([0x75; 32]));
    let mut host = RuntimeHost::open(runtime)
        .await
        .expect("native MySQL Web runtime must open");
    let application = host.application();
    let router = chat2db_web::router(application.clone());
    assert_java_dormant(&application);

    let root = create_datasource(&application, config, None, "MySQL root").await;
    let create_database = post(
        &router,
        "/api/rdb/database/create_database_sql",
        json!({
            "dataSourceId": root,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "charset": "utf8mb4",
            "collation": "utf8mb4_0900_ai_ci"
        }),
    )
    .await;
    execute_ddl(
        &router,
        &root,
        "",
        "",
        create_database["sql"].as_str().expect("database SQL"),
    )
    .await;
    assert_eq!(
        database_options(config, database_name).await,
        ("utf8mb4".to_owned(), "utf8mb4_0900_ai_ci".to_owned())
    );
    assert_java_dormant(&application);

    let datasource =
        create_datasource(&application, config, Some(database_name), "MySQL editable").await;
    let editor_meta = get(
        &router,
        &format!("/api/rdb/table/table_meta?dataSourceId={datasource}&databaseType=MYSQL"),
    )
    .await;
    assert!(
        editor_meta["columnTypes"]
            .as_array()
            .expect("column type inventory")
            .iter()
            .any(|item| item["typeName"] == "VARCHAR")
    );
    assert!(
        editor_meta["engineTypes"]
            .as_array()
            .expect("engine inventory")
            .iter()
            .any(|item| item["name"] == "InnoDB")
    );
    let table_sql = post(
        &router,
        "/api/rdb/table/modify/sql",
        json!({
            "dataSourceId": datasource,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "newTable": {
                "name": "items",
                "databaseName": database_name,
                "type": "TABLE",
                "engine": "InnoDB",
                "charset": "utf8mb4",
                "collate": "utf8mb4_0900_ai_ci",
                "columnList": [
                    {"name": "id", "columnType": "BIGINT", "nullable": 0,
                     "autoIncrement": true, "primaryKey": true},
                    {"name": "label", "columnType": "VARCHAR", "columnSize": 128,
                     "nullable": 0, "comment": "editable label"},
                    {"name": "score", "columnType": "INT", "nullable": 0,
                     "defaultValue": "7"}
                ],
                "indexList": []
            }
        }),
    )
    .await;
    execute_ddl(
        &router,
        &datasource,
        database_name,
        "items",
        table_sql[0]["sql"].as_str().expect("table SQL"),
    )
    .await;

    let old_table = get(
        &router,
        &format!(
            "/api/rdb/table/query?dataSourceId={datasource}&databaseType=MYSQL&databaseName={database_name}&tableName=items"
        ),
    )
    .await;
    let mut new_table = old_table.clone();
    new_table["columnList"]
        .as_array_mut()
        .expect("table columns")
        .push(json!({
            "name": "note",
            "columnType": "TEXT",
            "nullable": 1,
            "comment": "nullable note",
            "editStatus": "ADD"
        }));
    let alter_sql = post(
        &router,
        "/api/rdb/table/modify/sql",
        json!({
            "dataSourceId": datasource,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "oldTable": old_table,
            "newTable": new_table
        }),
    )
    .await;
    execute_ddl(
        &router,
        &datasource,
        database_name,
        "items",
        alter_sql[0]["sql"].as_str().expect("alter table SQL"),
    )
    .await;
    assert!(column_exists(config, database_name, "items", "note").await);

    let old_table = get(
        &router,
        &format!(
            "/api/rdb/table/query?dataSourceId={datasource}&databaseType=MYSQL&databaseName={database_name}&tableName=items"
        ),
    )
    .await;
    let mut new_table = old_table.clone();
    let label = new_table["columnList"]
        .as_array_mut()
        .expect("table columns")
        .iter_mut()
        .find(|column| column["name"] == "label")
        .expect("label column");
    label["primaryKey"] = json!(true);
    label["primaryKeyOrder"] = json!(2);
    label["editStatus"] = json!("MODIFY");
    let primary_key_sql = post(
        &router,
        "/api/rdb/table/modify/sql",
        json!({
            "dataSourceId": datasource,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "oldTable": old_table,
            "newTable": new_table
        }),
    )
    .await;
    execute_ddl(
        &router,
        &datasource,
        database_name,
        "items",
        primary_key_sql[0]["sql"].as_str().expect("primary key SQL"),
    )
    .await;
    assert_eq!(
        primary_key_columns(config, database_name, "items").await,
        ["id", "label"]
    );
    assert_java_dormant(&application);

    let empty_preview = preview(&router, &datasource, database_name, "items").await;
    assert_eq!(empty_preview["canEdit"], true);
    assert_eq!(
        empty_preview["headerList"][0]["dataType"],
        "CHAT2DB_ROW_NUMBER"
    );
    assert_eq!(empty_preview["headerList"][1]["primaryKey"], true);
    assert_eq!(empty_preview["headerList"][2]["primaryKey"], true);
    assert_eq!(empty_preview["headerList"][1]["autoIncrement"], 1);
    let headers = empty_preview["headerList"].clone();

    let insert_sql = grid_sql(
        &router,
        "/api/rdb/dml/get_update_sql",
        &datasource,
        database_name,
        headers.clone(),
        json!([{
            "type": "CREATE",
            "dataList": ["new-row", "CHAT2DB_UPDATE_TABLE_DATA_USER_FILLED_GENERATED",
                         "O'Reilly\\path", "CHAT2DB_UPDATE_TABLE_DATA_USER_FILLED_DEFAULT",
                         null]
        }]),
    )
    .await;
    execute_update(&router, &datasource, database_name, &insert_sql).await;
    assert_java_dormant(&application);

    let inserted_preview = preview(&router, &datasource, database_name, "items").await;
    let old_row = cell_values(&inserted_preview["dataList"][0]);
    assert_eq!(old_row, json!(["1", "1", "O'Reilly\\path", "7", null]));
    let mut updated_row = old_row.as_array().expect("row values").clone();
    updated_row[2] = json!("更新后的值");
    updated_row[3] = json!(9);
    updated_row[4] = json!("引号 ' 与反斜杠 \\ 都保留");
    let update_sql = grid_sql(
        &router,
        "/api/rdb/dml/get_update_sql",
        &datasource,
        database_name,
        headers.clone(),
        json!([{
            "type": "UPDATE",
            "dataList": updated_row,
            "oldDataList": old_row
        }]),
    )
    .await;
    execute_update(&router, &datasource, database_name, &update_sql).await;
    assert_eq!(
        read_item(config, database_name).await,
        (
            "更新后的值".to_owned(),
            9,
            Some("引号 ' 与反斜杠 \\ 都保留".to_owned())
        )
    );

    let count = post(
        &router,
        "/api/rdb/dml/count",
        sql_request(&datasource, database_name, "items", "SELECT * FROM `items`"),
    )
    .await;
    assert_eq!(count, 1);

    let updated_preview = preview(&router, &datasource, database_name, "items").await;
    let updated_values = cell_values(&updated_preview["dataList"][0]);
    let copy_update = grid_sql(
        &router,
        "/api/rdb/dml/copy_update_sql",
        &datasource,
        database_name,
        headers.clone(),
        json!([{"type": "UPDATE_COPY", "dataList": updated_values, "selectCols": [2]}]),
    )
    .await;
    assert!(copy_update.contains("SET `label` = '更新后的值' WHERE `id` = 1"));
    let in_values = post(
        &router,
        "/api/rdb/dml/copy_in_values_sql",
        json!({
            "dataSourceId": datasource,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "headerList": headers,
            "sourceType": "RESULT_SET",
            "operations": [{
                "type": "WHERE",
                "dataList": cell_values(&updated_preview["dataList"][0]),
                "selectCols": [2],
                "selectedCell": updated_preview["dataList"][0][2]
            }]
        }),
    )
    .await;
    assert_eq!(in_values, "('更新后的值')");
    assert_java_dormant(&application);

    post(
        &router,
        "/api/rdb/table/copy",
        json!({
            "dataSourceId": datasource,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "tableName": "items",
            "newName": "items_copy",
            "copyData": true
        }),
    )
    .await;
    assert_eq!(scalar_count(config, database_name, "items_copy").await, 1);
    post(
        &router,
        "/api/rdb/table/truncate",
        object_request(&datasource, database_name, "items_copy"),
    )
    .await;
    assert_eq!(scalar_count(config, database_name, "items_copy").await, 0);

    let view_sql = post(
        &router,
        "/api/rdb/view/modify/sql",
        json!({
            "dataSourceId": datasource,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "viewName": "item_labels",
            "viewBody": "SELECT id, label FROM items",
            "useOrReplace": true,
            "algorithm": "MERGE",
            "security": "INVOKER"
        }),
    )
    .await;
    execute_ddl(
        &router,
        &datasource,
        database_name,
        "item_labels",
        view_sql.as_str().expect("view SQL"),
    )
    .await;
    assert_eq!(scalar_count(config, database_name, "item_labels").await, 1);
    let view_detail = get(
        &router,
        &format!(
            "/api/rdb/view/query?dataSourceId={datasource}&databaseType=MYSQL&databaseName={database_name}&tableName=item_labels"
        ),
    )
    .await;
    assert_eq!(view_detail["name"], "item_labels");
    assert!(
        view_detail["ddl"]
            .as_str()
            .expect("view DDL")
            .to_ascii_uppercase()
            .contains("SELECT")
    );
    let replace_view_sql = post(
        &router,
        "/api/rdb/view/modify/sql",
        json!({
            "dataSourceId": datasource,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "viewName": "item_labels",
            "viewBody": "SELECT id, label FROM items WHERE score > 100",
            "useOrReplace": true,
            "algorithm": "MERGE",
            "security": "INVOKER"
        }),
    )
    .await;
    execute_ddl(
        &router,
        &datasource,
        database_name,
        "item_labels",
        replace_view_sql.as_str().expect("replace view SQL"),
    )
    .await;
    assert_eq!(scalar_count(config, database_name, "item_labels").await, 0);
    post(
        &router,
        "/api/rdb/view/drop",
        json!({
            "dataSourceId": datasource,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "viewName": "item_labels"
        }),
    )
    .await;
    assert!(!object_exists(config, database_name, "item_labels").await);
    assert_java_dormant(&application);

    let delete_sql = grid_sql(
        &router,
        "/api/rdb/dml/get_update_sql",
        &datasource,
        database_name,
        updated_preview["headerList"].clone(),
        json!([{
            "type": "DELETE",
            "oldDataList": cell_values(&updated_preview["dataList"][0])
        }]),
    )
    .await;
    execute_update(&router, &datasource, database_name, &delete_sql).await;
    assert_eq!(scalar_count(config, database_name, "items").await, 0);

    for table in ["items_copy", "items"] {
        post(
            &router,
            "/api/rdb/ddl/delete",
            object_request(&datasource, database_name, table),
        )
        .await;
    }
    let prepared = post(
        &router,
        "/api/rdb/delete/database/prepare",
        json!({
            "dataSourceId": root,
            "databaseType": "MYSQL",
            "databaseName": database_name
        }),
    )
    .await;
    assert_eq!(prepared["confirmName"], database_name);
    let delete_error = post_failure(
        &router,
        "/api/rdb/delete/database/execute",
        json!({
            "dataSourceId": root,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "confirmName": "wrong-name"
        }),
    )
    .await;
    assert_eq!(
        delete_error["errorCode"],
        "database_object_delete_confirmation_mismatch"
    );
    assert!(database_exists(config, database_name).await);
    post(
        &router,
        "/api/rdb/delete/database/execute",
        json!({
            "dataSourceId": root,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "confirmName": database_name
        }),
    )
    .await;
    assert!(!database_exists(config, database_name).await);
    assert_java_dormant(&application);

    host.shutdown()
        .await
        .expect("native-only Web runtime must shut down");
}

async fn create_datasource(
    application: &Application,
    config: &MysqlTestConfig,
    database_name: Option<&str>,
    name: &str,
) -> String {
    application
        .create_datasource(CreateDatasourceRequest {
            name: name.to_owned(),
            driver_id: "mysql".to_owned(),
            connection: Some(config.connection(database_name)),
        })
        .await
        .expect("native MySQL datasource must create")
        .id
}

async fn post(router: &Router, path: &str, payload: Value) -> Value {
    let envelope = request(router, Method::POST, path, Some(payload)).await;
    assert_eq!(
        envelope["success"], true,
        "route failed: {path}: {envelope}"
    );
    envelope["data"].clone()
}

async fn get(router: &Router, path: &str) -> Value {
    let envelope = request(router, Method::GET, path, None).await;
    assert_eq!(
        envelope["success"], true,
        "route failed: {path}: {envelope}"
    );
    envelope["data"].clone()
}

async fn post_failure(router: &Router, path: &str, payload: Value) -> Value {
    let envelope = request(router, Method::POST, path, Some(payload)).await;
    assert_eq!(
        envelope["success"], false,
        "route unexpectedly succeeded: {path}: {envelope}"
    );
    envelope
}

async fn request(router: &Router, method: Method, path: &str, payload: Option<Value>) -> Value {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(payload.map_or_else(Body::empty, |payload| {
            Body::from(serde_json::to_vec(&payload).expect("request must encode"))
        }))
        .expect("request must build");
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("Web route must respond");
    assert_eq!(response.status(), StatusCode::OK, "route failed: {path}");
    let body = response
        .into_body()
        .collect()
        .await
        .expect("response body must collect")
        .to_bytes();
    let envelope: Value = serde_json::from_slice(&body).expect("response must be JSON");
    envelope
}

async fn execute_ddl(
    router: &Router,
    datasource_id: &str,
    database_name: &str,
    table_name: &str,
    sql: &str,
) {
    let result = post(
        router,
        "/api/rdb/dml/execute_ddl",
        sql_request(datasource_id, database_name, table_name, sql),
    )
    .await;
    assert_eq!(result["success"], true, "DDL failed: {result}");
}

async fn execute_update(router: &Router, datasource_id: &str, database_name: &str, sql: &str) {
    let result = post(
        router,
        "/api/rdb/dml/execute_update",
        sql_request(datasource_id, database_name, "items", sql),
    )
    .await;
    assert_eq!(result["success"], true, "grid update failed: {result}");
    assert_eq!(
        result["updateCount"], 1,
        "unexpected update count: {result}"
    );
}

async fn grid_sql(
    router: &Router,
    path: &str,
    datasource_id: &str,
    database_name: &str,
    headers: Value,
    operations: Value,
) -> String {
    post(
        router,
        path,
        json!({
            "dataSourceId": datasource_id,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "tableName": "items",
            "headerList": headers,
            "operations": operations
        }),
    )
    .await
    .as_str()
    .expect("grid SQL must be a string")
    .to_owned()
}

async fn preview(
    router: &Router,
    datasource_id: &str,
    database_name: &str,
    table_name: &str,
) -> Value {
    post(
        router,
        "/api/rdb/dml/execute_table",
        json!({
            "dataSourceId": datasource_id,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "tableName": table_name,
            "pageNo": 1,
            "pageSize": 20
        }),
    )
    .await[0]
        .clone()
}

fn sql_request(datasource_id: &str, database_name: &str, table_name: &str, sql: &str) -> Value {
    json!({
        "dataSourceId": datasource_id,
        "databaseType": "MYSQL",
        "databaseName": database_name,
        "tableName": table_name,
        "sql": sql,
        "single": true,
        "pageNo": 1,
        "pageSize": 20
    })
}

fn object_request(datasource_id: &str, database_name: &str, table_name: &str) -> Value {
    json!({
        "dataSourceId": datasource_id,
        "databaseType": "MYSQL",
        "databaseName": database_name,
        "tableName": table_name
    })
}

fn cell_values(row: &Value) -> Value {
    Value::Array(
        row.as_array()
            .expect("preview row must be an array")
            .iter()
            .map(|cell| cell["value"].clone())
            .collect(),
    )
}

async fn scalar_count(config: &MysqlTestConfig, database_name: &str, object_name: &str) -> u64 {
    let mut conn = Conn::new(config.options())
        .await
        .expect("verification connection must open");
    let value = conn
        .query_first::<u64, _>(format!(
            "SELECT COUNT(*) FROM `{database_name}`.`{object_name}`"
        ))
        .await
        .expect("verification count must execute")
        .expect("verification count must return");
    conn.disconnect()
        .await
        .expect("verification connection must close");
    value
}

async fn database_options(config: &MysqlTestConfig, database_name: &str) -> (String, String) {
    let mut conn = Conn::new(config.options())
        .await
        .expect("database options connection must open");
    let options = conn
        .exec_first::<(String, String), _, _>(
            "SELECT DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME \
             FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?",
            (database_name,),
        )
        .await
        .expect("database options query must execute")
        .expect("created database must exist");
    conn.disconnect()
        .await
        .expect("database options connection must close");
    options
}

async fn column_exists(
    config: &MysqlTestConfig,
    database_name: &str,
    table_name: &str,
    column_name: &str,
) -> bool {
    let mut conn = Conn::new(config.options())
        .await
        .expect("column probe must connect");
    let count = conn
        .exec_first::<u64, _, _>(
            "SELECT COUNT(*) FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND COLUMN_NAME = ?",
            (database_name, table_name, column_name),
        )
        .await
        .expect("column probe must execute")
        .unwrap_or_default();
    conn.disconnect().await.expect("column probe must close");
    count > 0
}

async fn primary_key_columns(
    config: &MysqlTestConfig,
    database_name: &str,
    table_name: &str,
) -> Vec<String> {
    let mut conn = Conn::new(config.options())
        .await
        .expect("primary-key probe must connect");
    let columns = conn
        .exec::<String, _, _>(
            "SELECT COLUMN_NAME FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND INDEX_NAME = 'PRIMARY' \
             ORDER BY SEQ_IN_INDEX",
            (database_name, table_name),
        )
        .await
        .expect("primary-key probe must execute");
    conn.disconnect()
        .await
        .expect("primary-key probe must close");
    columns
}

async fn object_exists(config: &MysqlTestConfig, database_name: &str, object_name: &str) -> bool {
    let mut conn = Conn::new(config.options())
        .await
        .expect("object probe must connect");
    let count = conn
        .exec_first::<u64, _, _>(
            "SELECT COUNT(*) FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
            (database_name, object_name),
        )
        .await
        .expect("object probe must execute")
        .unwrap_or_default();
    conn.disconnect().await.expect("object probe must close");
    count > 0
}

async fn read_item(config: &MysqlTestConfig, database_name: &str) -> (String, i32, Option<String>) {
    let mut conn = Conn::new(config.options())
        .await
        .expect("row verification connection must open");
    let row = conn
        .query_first::<(String, i32, Option<String>), _>(format!(
            "SELECT `label`, `score`, `note` FROM `{database_name}`.`items` WHERE `id` = 1"
        ))
        .await
        .expect("row verification query must execute")
        .expect("updated row must exist");
    conn.disconnect()
        .await
        .expect("row verification connection must close");
    row
}

async fn database_exists(config: &MysqlTestConfig, database_name: &str) -> bool {
    let mut conn = Conn::new(config.options())
        .await
        .expect("database probe must connect");
    let count = conn
        .exec_first::<u64, _, _>(
            "SELECT COUNT(*) FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?",
            (database_name,),
        )
        .await
        .expect("database probe must execute")
        .unwrap_or_default();
    conn.disconnect().await.expect("database probe must close");
    count > 0
}

async fn cleanup_database(config: &MysqlTestConfig, database_name: &str) -> Result<(), String> {
    let mut conn = Conn::new(config.options())
        .await
        .map_err(|error| error.to_string())?;
    conn.query_drop(format!("DROP DATABASE IF EXISTS `{database_name}`"))
        .await
        .map_err(|error| error.to_string())?;
    conn.disconnect().await.map_err(|error| error.to_string())
}

fn assert_java_dormant(application: &Application) {
    let engine = application
        .health()
        .components
        .into_iter()
        .find(|component| component.id == "database-engine")
        .expect("database engine health must exist");
    assert_eq!(engine.state, ComponentState::Ready);
    assert_eq!(engine.detail, "Available on demand; Java is not running");
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be configured"))
}
