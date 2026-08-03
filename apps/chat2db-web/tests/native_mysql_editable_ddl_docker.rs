use std::{
    fs,
    io::{Cursor, Read as _},
    panic::AssertUnwindSafe,
    path::Path,
    time::Duration,
};

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
        let required = std::env::var("MYSQL_TEST_REQUIRED")
            .ok()
            .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE"));
        let configured = REQUIRED_MYSQL_ENV
            .iter()
            .filter(|name| std::env::var_os(name).is_some())
            .count();
        if configured == 0 {
            assert!(
                !required,
                "MYSQL_TEST_REQUIRED is enabled but the MySQL endpoint is absent"
            );
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
            ssh: None,
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
    verify_workspace_routes(
        &router,
        &application,
        config,
        &datasource,
        database_name,
        directory.path(),
    )
    .await;
    verify_routine_invocation_preview(&router, &application, &datasource, database_name).await;
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
    verify_pin_and_er_routes(&router, &application, &datasource, database_name).await;
    verify_account_routes(&router, &application, &datasource).await;
    verify_schema_diff_routes(&router, &application, &datasource, database_name).await;

    let export_message = json!({
        "dataSourceId": datasource,
        "databaseType": "MYSQL",
        "databaseName": database_name,
        "schemaName": "",
        "tableName": "items"
    });
    let mut exported_ddl = None;
    for path in ["/api/rdb/ddl/export", "/api/rdb/table/export"] {
        let http_ddl = get(
            &router,
            &format!(
                "{path}?dataSourceId={datasource}&databaseType=MYSQL&databaseName={database_name}&schemaName=&tableName=items"
            ),
        )
        .await;
        let http_ddl = http_ddl.as_str().expect("exported DDL must be a string");
        assert!(http_ddl.starts_with("CREATE TABLE `items`"));
        assert!(http_ddl.contains("`label` varchar(128) NOT NULL COMMENT 'editable label'"));
        assert!(http_ddl.ends_with(';'));
        if let Some(expected) = exported_ddl.as_deref() {
            assert_eq!(http_ddl, expected, "HTTP export aliases diverged");
        } else {
            exported_ddl = Some(http_ddl.to_owned());
        }

        let desktop = chat2db_web::legacy::dispatch(
            &application,
            chat2db_web::legacy::LegacyDispatchRequest {
                request_url: path.to_owned(),
                method: "get".to_owned(),
                message: export_message.clone(),
            },
        )
        .await;
        assert_eq!(desktop["success"], true, "desktop export failed: {desktop}");
        assert_eq!(desktop["data"], http_ddl, "desktop export diverged: {path}");
    }
    assert_java_dormant(&application);

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
            "oldName": null,
            "name": "note",
            "tableName": null,
            "columnType": "TEXT",
            "dataType": null,
            "defaultValue": null,
            "autoIncrement": null,
            "nullable": 1,
            "comment": "nullable note",
            "primaryKey": null,
            "primaryKeyName": null,
            "primaryKeyOrder": null,
            "schemaName": null,
            "databaseName": null,
            "typeName": null,
            "columnSize": null,
            "bufferLength": null,
            "decimalDigits": null,
            "numPrecRadix": null,
            "nullableInt": null,
            "sqlDataType": null,
            "sqlDatetimeSub": null,
            "charOctetLength": null,
            "ordinalPosition": null,
            "generatedColumn": null,
            "extent": null,
            "charSetName": null,
            "collationName": null,
            "value": null,
            "unit": null,
            "defaultConstraintName": null,
            "editStatus": "ADD"
        }));
    new_table["indexList"]
        .as_array_mut()
        .expect("table indexes")
        .push(json!({
            "oldName": null,
            "name": "idx_label",
            "tableName": null,
            "type": "Normal",
            "unique": null,
            "comment": null,
            "schemaName": null,
            "databaseName": null,
            "concurrently": null,
            "method": null,
            "foreignSchemaName": null,
            "foreignTableName": null,
            "foreignColumnNamelist": null,
            "columnList": [{
                "indexName": null,
                "tableName": null,
                "type": null,
                "comment": null,
                "columnName": "label",
                "ordinalPosition": null,
                "collation": null,
                "schemaName": null,
                "databaseName": null,
                "nonUnique": null,
                "indexQualifier": null,
                "ascOrDesc": null,
                "cardinality": null,
                "pages": null,
                "filterCondition": null,
                "subPart": null,
                "editStatus": null
            }],
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
    assert!(index_exists(config, database_name, "items", "idx_label").await);

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
    let primary_key_metadata = get(
        &router,
        &format!(
            "/api/rdb/table/query?dataSourceId={datasource}&databaseType=MYSQL&databaseName={database_name}&tableName=items"
        ),
    )
    .await;
    assert_eq!(
        primary_key_metadata["columnList"]
            .as_array()
            .expect("table columns")
            .iter()
            .filter(|column| column["primaryKey"] == true)
            .map(|column| (
                column["name"].as_str().expect("primary-key name"),
                column["primaryKeyOrder"]
                    .as_i64()
                    .expect("primary-key order")
            ))
            .collect::<Vec<_>>(),
        [("id", 1), ("label", 2)]
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
    verify_transfer_routes(
        &router,
        &application,
        config,
        &datasource,
        database_name,
        directory.path(),
    )
    .await;

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
                "type": "IN_VALUES",
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

    let view_meta = get(
        &router,
        &format!(
            "/api/rdb/view/view_meta?dataSourceId={datasource}&databaseType=MYSQL&databaseName={database_name}"
        ),
    )
    .await;
    assert_eq!(
        view_meta["configurations"]
            .as_array()
            .expect("view configurations")
            .iter()
            .map(|configuration| configuration["name"].as_str().expect("configuration name"))
            .collect::<Vec<_>>(),
        [
            "algorithm",
            "checkOption",
            "security",
            "viewName",
            "definer",
            "useOrReplace"
        ]
    );
    assert_eq!(view_meta["sql"], "select * from table_name");
    assert_eq!(
        view_meta["previewSql"],
        format!("create view `{database_name}`.`undefined` AS \nselect * from table_name;")
    );
    let root_view_meta = get(
        &router,
        &format!("/api/rdb/view/view_meta?dataSourceId={root}&databaseType=MYSQL&databaseName="),
    )
    .await;
    assert_eq!(
        root_view_meta["previewSql"],
        "create view `undefined` AS \nselect * from table_name;"
    );

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

    execute_ddl(
        &router,
        &datasource,
        database_name,
        "metadata_items",
        &format!(
            "CREATE TABLE `{database_name}`.`metadata_items` (\
             `b` BIGINT UNSIGNED NOT NULL, \
             `a` INT UNSIGNED NOT NULL, \
             `state` ENUM('','active','not UNSIGNED value','needs,review','O''Reilly') NOT NULL, \
             `permissions` SET('read','write','close)later') NULL, \
             PRIMARY KEY (`b`, `a`)) ENGINE = InnoDB"
        ),
    )
    .await;
    let metadata_table = get(
        &router,
        &format!(
            "/api/rdb/table/query?dataSourceId={datasource}&databaseType=MYSQL&databaseName={database_name}&tableName=metadata_items"
        ),
    )
    .await;
    let metadata_columns = metadata_table["columnList"]
        .as_array()
        .expect("metadata columns");
    assert_eq!(
        metadata_columns
            .iter()
            .find(|column| column["name"] == "b")
            .expect("unsigned column")["columnType"],
        "BIGINT UNSIGNED"
    );
    assert_eq!(
        metadata_columns
            .iter()
            .find(|column| column["name"] == "state")
            .expect("enum column")["value"],
        "'','active','not UNSIGNED value','needs,review','O''Reilly'"
    );
    assert_eq!(
        metadata_columns
            .iter()
            .find(|column| column["name"] == "state")
            .expect("enum column")["columnType"],
        "ENUM"
    );
    assert_eq!(
        metadata_columns
            .iter()
            .find(|column| column["name"] == "permissions")
            .expect("set column")["value"],
        "'read','write','close)later'"
    );
    assert_eq!(
        metadata_columns
            .iter()
            .filter(|column| column["primaryKey"] == true)
            .map(|column| (
                column["name"].as_str().expect("primary-key name"),
                column["primaryKeyOrder"]
                    .as_i64()
                    .expect("primary-key order")
            ))
            .collect::<Vec<_>>(),
        [("b", 1), ("a", 2)]
    );

    let mut reordered_table = metadata_table.clone();
    reordered_table["columnList"] = Value::Array(
        ["permissions", "state", "b", "a"]
            .iter()
            .map(|name| {
                metadata_columns
                    .iter()
                    .find(|column| column["name"] == *name)
                    .unwrap_or_else(|| panic!("missing metadata column {name}"))
                    .clone()
            })
            .collect(),
    );
    let reorder_sql = post(
        &router,
        "/api/rdb/table/modify/sql",
        json!({
            "dataSourceId": datasource,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "oldTable": metadata_table,
            "newTable": reordered_table
        }),
    )
    .await;
    execute_ddl(
        &router,
        &datasource,
        database_name,
        "metadata_items",
        reorder_sql[0]["sql"].as_str().expect("column reorder SQL"),
    )
    .await;
    assert_eq!(
        column_order(config, database_name, "metadata_items").await,
        ["permissions", "state", "b", "a"]
    );
    assert_eq!(
        primary_key_columns(config, database_name, "metadata_items").await,
        ["b", "a"]
    );
    assert_eq!(
        column_definition(config, database_name, "metadata_items", "b").await,
        "bigint unsigned"
    );
    assert_eq!(
        column_definition(config, database_name, "metadata_items", "state").await,
        "enum('','active','not UNSIGNED value','needs,review','O''Reilly')"
    );
    assert_eq!(
        column_definition(config, database_name, "metadata_items", "permissions").await,
        "set('read','write','close)later')"
    );

    execute_ddl(
        &router,
        &datasource,
        database_name,
        "reorder_guard",
        &format!(
            "CREATE TABLE `{database_name}`.`reorder_guard` (\
             `base` INT NOT NULL, \
             `generated_value` INT GENERATED ALWAYS AS (`base` + 1) STORED, \
             `hidden_value` INT INVISIBLE, \
             `tail` INT NOT NULL) ENGINE = InnoDB"
        ),
    )
    .await;
    let guard_table = get(
        &router,
        &format!(
            "/api/rdb/table/query?dataSourceId={datasource}&databaseType=MYSQL&databaseName={database_name}&tableName=reorder_guard"
        ),
    )
    .await;
    let guard_columns = guard_table["columnList"].as_array().expect("guard columns");
    assert!(
        column_extra(config, database_name, "reorder_guard", "generated_value")
            .await
            .contains("STORED GENERATED")
    );
    assert!(
        column_extra(config, database_name, "reorder_guard", "hidden_value")
            .await
            .contains("INVISIBLE")
    );

    let mut generated_reorder = guard_table.clone();
    generated_reorder["columnList"] = Value::Array(
        ["generated_value", "base", "hidden_value", "tail"]
            .iter()
            .map(|name| {
                guard_columns
                    .iter()
                    .find(|column| column["name"] == *name)
                    .unwrap_or_else(|| panic!("missing guard column {name}"))
                    .clone()
            })
            .collect(),
    );
    let generated_failure = post_failure(
        &router,
        "/api/rdb/table/modify/sql",
        json!({
            "dataSourceId": datasource,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "oldTable": guard_table,
            "newTable": generated_reorder
        }),
    )
    .await;
    assert_eq!(generated_failure["errorCode"], "invalid_mysql_ddl");
    assert!(
        generated_failure["errorMessage"]
            .as_str()
            .expect("generated-column failure message")
            .contains("generated-column")
    );

    let mut invisible_reorder = guard_table.clone();
    invisible_reorder["columnList"] = Value::Array(
        ["base", "generated_value", "tail", "hidden_value"]
            .iter()
            .map(|name| {
                guard_columns
                    .iter()
                    .find(|column| column["name"] == *name)
                    .unwrap_or_else(|| panic!("missing guard column {name}"))
                    .clone()
            })
            .collect(),
    );
    let invisible_failure = post_failure(
        &router,
        "/api/rdb/table/modify/sql",
        json!({
            "dataSourceId": datasource,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "oldTable": guard_table,
            "newTable": invisible_reorder
        }),
    )
    .await;
    assert_eq!(invisible_failure["errorCode"], "invalid_mysql_ddl");
    assert!(
        invisible_failure["errorMessage"]
            .as_str()
            .expect("invisible-column failure message")
            .contains("INVISIBLE")
    );
    assert_eq!(
        column_order(config, database_name, "reorder_guard").await,
        ["base", "generated_value", "hidden_value", "tail"]
    );
    assert!(
        column_extra(config, database_name, "reorder_guard", "generated_value")
            .await
            .contains("STORED GENERATED")
    );
    assert!(
        column_extra(config, database_name, "reorder_guard", "hidden_value")
            .await
            .contains("INVISIBLE")
    );
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

    for table in ["reorder_guard", "metadata_items", "items_copy", "items"] {
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

async fn verify_pin_and_er_routes(
    router: &Router,
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let table = object_request(datasource_id, database_name, "items");
    post(router, "/api/pin/table/add", table.clone()).await;
    post(router, "/api/pin/table/add", table.clone()).await;

    let list_path = format!(
        "/api/pin/table/list?dataSourceId={datasource_id}&databaseName={database_name}&schemaName="
    );
    assert_eq!(get(router, &list_path).await, json!(["items"]));
    let desktop_pins = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/pin/table/list".to_owned(),
            method: "get".to_owned(),
            message: json!({
                "dataSourceId": datasource_id,
                "databaseName": database_name,
                "schemaName": ""
            }),
        },
    )
    .await;
    assert_eq!(desktop_pins["success"], true);
    assert_eq!(desktop_pins["data"], json!(["items"]));

    assert_table_is_pinned(router, datasource_id, database_name).await;

    let er_path = format!(
        "/api/er/get_info?dataSourceId={datasource_id}&databaseName={database_name}&schemaName="
    );
    let er = get(router, &er_path).await;
    assert!(er["position"].is_null());
    let items = er["tables"]
        .as_array()
        .and_then(|tables| tables.iter().find(|table| table["name"] == "items"))
        .expect("items table must be present in ER metadata");
    assert!(items["columnList"].as_array().is_some_and(|columns| {
        columns
            .iter()
            .any(|column| column["name"] == "id" && column["primaryKey"] == true)
    }));
    let desktop_er = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/er/get_info".to_owned(),
            method: "get".to_owned(),
            message: json!({
                "dataSourceId": datasource_id,
                "databaseName": database_name,
                "schemaName": ""
            }),
        },
    )
    .await;
    assert_eq!(desktop_er["success"], true);
    assert_eq!(desktop_er["data"], er);

    post(
        router,
        "/api/er/save_position",
        json!({
            "dataSourceId": datasource_id,
            "databaseName": database_name,
            "schemaName": "",
            "position": "{\"version\":1}"
        }),
    )
    .await;
    let desktop_save = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/er/save_position".to_owned(),
            method: "post".to_owned(),
            message: json!({
                "dataSourceId": datasource_id,
                "databaseName": database_name,
                "schemaName": "",
                "position": "{\"version\":2}"
            }),
        },
    )
    .await;
    assert_eq!(desktop_save["success"], true);
    assert_eq!(get(router, &er_path).await["position"], "{\"version\":2}");

    let desktop_delete = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/pin/table/delete".to_owned(),
            method: "post".to_owned(),
            message: table,
        },
    )
    .await;
    assert_eq!(desktop_delete["success"], true);
    assert_eq!(get(router, &list_path).await, json!([]));
    assert_java_dormant(application);
}

async fn assert_table_is_pinned(router: &Router, datasource_id: &str, database_name: &str) {
    let tables = get(
        router,
        &format!(
            "/api/rdb/table/list?dataSourceId={datasource_id}&databaseType=MYSQL&databaseName={database_name}&schemaName=&pageNo=1&pageSize=20&searchKey="
        ),
    )
    .await;
    assert!(tables["data"].as_array().is_some_and(|tables| {
        tables
            .iter()
            .any(|table| table["name"] == "items" && table["pinned"] == true)
    }));
}

async fn verify_account_routes(router: &Router, application: &Application, datasource_id: &str) {
    let capability = get(
        router,
        &format!("/api/rdb/account/capability?dataSourceId={datasource_id}"),
    )
    .await;
    assert_eq!(capability["dbType"], "MYSQL");
    assert_eq!(capability["accountListReadable"], true);
    assert_eq!(
        capability["editablePrivileges"].as_array().map(Vec::len),
        Some(14)
    );
    assert!(
        get(
            router,
            &format!("/api/rdb/account/list?dataSourceId={datasource_id}"),
        )
        .await
        .as_array()
        .is_some_and(|accounts| !accounts.is_empty())
    );

    let current_account = capability["currentUser"]
        .as_str()
        .expect("current MySQL account must be reported");
    let (current_user, current_host) = current_account
        .split_once('@')
        .expect("current MySQL account must contain a host");
    let grants_query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("dataSourceId", datasource_id)
        .append_pair("user", current_user)
        .append_pair("host", current_host)
        .finish();
    assert!(
        get(router, &format!("/api/rdb/account/grants?{grants_query}"))
            .await
            .as_array()
            .is_some_and(|grants| !grants.is_empty())
    );

    let missing_user = format!("c2d_missing_{}", &Uuid::new_v4().simple().to_string()[..10]);
    let password = "MustNotLeak'\\Password";
    let create_preview = post(
        router,
        "/api/rdb/account/preview",
        json!({
            "dataSourceId": datasource_id,
            "user": missing_user,
            "host": "%",
            "actionType": "CREATE_USER",
            "password": password
        }),
    )
    .await;
    assert!(
        create_preview["sql"]
            .as_str()
            .is_some_and(|sql| { sql.contains("******") && !sql.contains(password) })
    );

    let mut drop_request = json!({
        "dataSourceId": datasource_id,
        "user": missing_user,
        "host": "%",
        "actionType": "DROP_USER"
    });
    let drop_preview = post(router, "/api/rdb/account/preview", drop_request.clone()).await;
    drop_request
        .as_object_mut()
        .expect("account request object")
        .insert(
            "previewToken".to_owned(),
            drop_preview["previewToken"].clone(),
        );
    let failed_execution = post(router, "/api/rdb/account/execute", drop_request.clone()).await;
    assert_eq!(failed_execution["success"], false);
    assert_eq!(
        failed_execution["failureCode"],
        "mysql.account.executeFailed"
    );
    verify_account_token_replay_and_desktop(
        application,
        datasource_id,
        &missing_user,
        drop_request,
        password,
    )
    .await;
    assert_java_dormant(application);
}

async fn verify_account_token_replay_and_desktop(
    application: &Application,
    datasource_id: &str,
    missing_user: &str,
    drop_request: Value,
    password: &str,
) {
    let replay = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/rdb/account/execute".to_owned(),
            method: "post".to_owned(),
            message: drop_request.clone(),
        },
    )
    .await;
    assert_eq!(replay["success"], false);
    assert_eq!(replay["errorCode"], "mysql.account.previewTokenMismatch");

    let mut desktop_request = json!({
        "dataSourceId": datasource_id,
        "user": missing_user,
        "host": "%",
        "actionType": "DROP_USER"
    });
    let desktop_preview = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/rdb/account/preview".to_owned(),
            method: "post".to_owned(),
            message: desktop_request.clone(),
        },
    )
    .await;
    assert_eq!(desktop_preview["success"], true);
    desktop_request
        .as_object_mut()
        .expect("desktop account request object")
        .insert(
            "previewToken".to_owned(),
            desktop_preview["data"]["previewToken"].clone(),
        );
    let desktop_execution = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/rdb/account/execute".to_owned(),
            method: "post".to_owned(),
            message: desktop_request,
        },
    )
    .await;
    assert_eq!(desktop_execution["success"], true);
    assert_eq!(desktop_execution["data"]["success"], false);
    assert_eq!(
        desktop_execution["data"]["failureCode"],
        "mysql.account.executeFailed"
    );
    assert!(!desktop_execution.to_string().contains(password));
}

async fn verify_schema_diff_routes(
    router: &Router,
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let request = json!({
        "source": {
            "dataSourceId": datasource_id,
            "databaseName": database_name,
            "schemaName": ""
        },
        "target": {
            "dataSourceId": datasource_id,
            "databaseName": database_name,
            "schemaName": ""
        }
    });
    let http_sql = post(router, "/api/diff/sql", request.clone()).await;
    assert_eq!(http_sql, json!("-- No differences. "));

    let desktop = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/diff/sql".to_owned(),
            method: "post".to_owned(),
            message: request,
        },
    )
    .await;
    assert_eq!(
        desktop["success"], true,
        "desktop schema diff failed: {desktop}"
    );
    assert_eq!(desktop["data"], http_sql);
    assert_java_dormant(application);
}

#[allow(clippy::too_many_lines)]
async fn verify_transfer_routes(
    router: &Router,
    application: &Application,
    config: &MysqlTestConfig,
    datasource_id: &str,
    database_name: &str,
    directory: &Path,
) {
    execute_ddl(
        router,
        datasource_id,
        database_name,
        "transfer_items",
        "CREATE TABLE `transfer_items` (`id` INT PRIMARY KEY, `label` VARCHAR(64) NOT NULL)",
    )
    .await;

    let csv_task = multipart_mysql_import(
        router,
        "/api/import/other_file",
        &[
            ("dataSourceId", datasource_id),
            ("databaseName", database_name),
            ("schemaName", ""),
            ("tableName", "transfer_items"),
            ("importType", "CSV"),
            ("containsHeader", "true"),
        ],
        "transfer-items.csv",
        b"id,label\n1,alpha\n2,beta\n",
    )
    .await
    .as_i64()
    .expect("CSV import task id");
    let csv_task = wait_for_transfer_task(router, csv_task).await;
    assert_eq!(csv_task["taskType"], "UPLOAD_TABLE_DATA");
    assert_eq!(csv_task["taskStatus"], "FINISHED");
    assert_eq!(csv_task["taskProgress"], "100");

    let sql_task = multipart_mysql_import(
        router,
        "/api/import/sql_file",
        &[
            ("dataSourceId", datasource_id),
            ("databaseName", database_name),
            ("schemaName", ""),
        ],
        "transfer-items.sql",
        b"INSERT INTO `transfer_items` (`id`, `label`) VALUES (3, 'gamma');\n",
    )
    .await
    .as_i64()
    .expect("SQL import task id");
    wait_for_transfer_task(router, sql_task).await;
    assert_eq!(
        scalar_count(config, database_name, "transfer_items").await,
        3
    );

    let desktop_sql_path = directory.join("desktop-transfer-items.sql");
    fs::write(
        &desktop_sql_path,
        "INSERT INTO `transfer_items` (`id`, `label`) VALUES (4, 'desktop');\n",
    )
    .expect("desktop SQL import fixture must write");
    let desktop_import = chat2db_web::legacy::dispatch_desktop(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/import/sql_file".to_owned(),
            method: "post".to_owned(),
            message: json!({
                "dataSourceId": datasource_id,
                "databaseName": database_name,
                "schemaName": "",
                "fileName": desktop_sql_path.to_string_lossy()
            }),
        },
    )
    .await;
    assert_eq!(desktop_import["success"], true, "{desktop_import}");
    let desktop_task = desktop_import["data"]
        .as_i64()
        .expect("desktop SQL import task id");
    wait_for_transfer_task(router, desktop_task).await;
    assert_eq!(
        scalar_count(config, database_name, "transfer_items").await,
        4
    );

    let sql_export_task = post(
        router,
        "/api/export/sql_file",
        json!({
            "dataSourceId": datasource_id,
            "databaseName": database_name,
            "schemaName": "",
            "tableNames": ["transfer_items"],
            "scope": "ALL",
            "containData": true,
            "exportPath": ""
        }),
    )
    .await
    .as_i64()
    .expect("SQL export task id");
    let sql_export = wait_for_transfer_task(router, sql_export_task).await;
    assert_eq!(sql_export["taskType"], "DOWNLOAD_TABLE_STRUCTURE");
    assert!(
        sql_export["downloadUrl"]
            .as_str()
            .is_some_and(|url| url.ends_with(&format!("id={sql_export_task}")))
    );
    let sql_dump = download_attachment(
        router,
        Method::GET,
        &format!("/api/task/download?id={sql_export_task}"),
        None,
    )
    .await;
    let sql_dump = String::from_utf8(sql_dump).expect("SQL task download must be UTF-8");
    assert!(sql_dump.contains("CREATE TABLE"));
    assert!(sql_dump.contains("INSERT INTO"));
    assert!(sql_dump.contains("transfer_items"));

    let desktop_download = chat2db_web::legacy::dispatch_desktop(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/task/download".to_owned(),
            method: "get".to_owned(),
            message: json!({"id": sql_export_task}),
        },
    )
    .await;
    assert_eq!(desktop_download["success"], true, "{desktop_download}");
    assert!(
        desktop_download["data"]
            .as_str()
            .is_some_and(|path| Path::new(path).is_file())
    );

    let csv_export_task = post(
        router,
        "/api/export/other_file",
        json!({
            "dataSourceId": datasource_id,
            "databaseName": database_name,
            "schemaName": "",
            "tableNames": ["transfer_items"],
            "exportType": "CSV",
            "containsHeader": true,
            "exportPath": ""
        }),
    )
    .await
    .as_i64()
    .expect("CSV export task id");
    wait_for_transfer_task(router, csv_export_task).await;
    let csv_export = download_attachment(
        router,
        Method::GET,
        &format!("/api/task/download?id={csv_export_task}"),
        None,
    )
    .await;
    let csv_export = String::from_utf8(csv_export).expect("CSV task download must be UTF-8");
    assert!(csv_export.starts_with("id,label"));
    assert!(csv_export.contains("3,gamma"));

    let finished = get(
        router,
        "/api/task/list?pageNo=1&pageSize=20&taskStatus=FINISHED",
    )
    .await;
    assert!(finished["total"].as_u64().is_some_and(|total| total >= 4));
    assert!(finished["data"].as_array().is_some_and(|tasks| {
        tasks
            .iter()
            .any(|task| task["id"] == sql_export_task && task["taskProgress"] == "100")
    }));
    let desktop_finished = chat2db_web::legacy::dispatch_desktop(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/task/list".to_owned(),
            method: "get".to_owned(),
            message: json!({
                "pageNo": 1,
                "pageSize": 20,
                "taskStatus": "FINISHED"
            }),
        },
    )
    .await;
    assert_eq!(desktop_finished["success"], true, "{desktop_finished}");
    assert!(
        desktop_finished["data"]["data"]
            .as_array()
            .is_some_and(|tasks| tasks.iter().any(|task| {
                task["id"] == sql_export_task
                    && task["downloadUrl"]
                        .as_str()
                        .is_some_and(|path| Path::new(path).is_file())
            }))
    );

    let desktop_task = chat2db_web::legacy::dispatch_desktop(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/task/get".to_owned(),
            method: "get".to_owned(),
            message: json!({"id": sql_export_task}),
        },
    )
    .await;
    assert_eq!(desktop_task["success"], true, "{desktop_task}");
    assert!(
        desktop_task["data"]["downloadUrl"]
            .as_str()
            .is_some_and(|path| Path::new(path).is_file())
    );
    assert!(
        get(router, &format!("/api/task/stop?id={csv_export_task}"),)
            .await
            .is_null()
    );

    let dml_request = json!({
        "dataSourceId": datasource_id,
        "databaseName": database_name,
        "schemaName": "",
        "sql": "SELECT id, label FROM transfer_items ORDER BY id",
        "originalSql": "SELECT id, label FROM transfer_items ORDER BY id",
        "resultSetId": 0,
        "exportSize": "ALL",
        "exportType": "CSV"
    });
    let dml_csv = download_attachment(
        router,
        Method::POST,
        "/api/rdb/dml/export",
        Some(dml_request.clone()),
    )
    .await;
    let dml_csv = String::from_utf8(dml_csv).expect("DML CSV must be UTF-8");
    assert!(dml_csv.starts_with("id,label"));
    assert!(dml_csv.contains("2,beta"));
    let desktop_dml = chat2db_web::legacy::dispatch_desktop(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/rdb/dml/export".to_owned(),
            method: "post".to_owned(),
            message: dml_request,
        },
    )
    .await;
    assert_eq!(desktop_dml["success"], true, "{desktop_dml}");
    assert!(
        desktop_dml["data"]
            .as_str()
            .is_some_and(|path| Path::new(path).is_file())
    );

    let class_request = json!({
        "dataSourceId": datasource_id,
        "databaseName": database_name,
        "schemaName": "",
        "tableName": "transfer_items",
        "exportPath": ""
    });
    let archive = download_attachment(
        router,
        Method::POST,
        "/api/rdb/table/generate/class",
        Some(class_request.clone()),
    )
    .await;
    let mut archive = zip::ZipArchive::new(Cursor::new(archive)).expect("class ZIP must open");
    for file_name in [
        "TransferItemsDO.java",
        "TransferItemsMapper.java",
        "TransferItemsMapper.xml",
    ] {
        let mut entry = archive
            .by_name(&format!("transfer_items/{file_name}"))
            .expect("generated class entry must exist");
        let mut contents = String::new();
        entry
            .read_to_string(&mut contents)
            .expect("generated class entry must be UTF-8");
        assert!(!contents.is_empty());
    }

    let generated = directory.join("generated-classes");
    let mut desktop_class_request = class_request;
    desktop_class_request["exportPath"] = json!(generated.to_string_lossy());
    let desktop_generated = chat2db_web::legacy::dispatch_desktop(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/rdb/table/generate/class".to_owned(),
            method: "post".to_owned(),
            message: desktop_class_request,
        },
    )
    .await;
    assert_eq!(desktop_generated["success"], true, "{desktop_generated}");
    let generated_table = generated.join("transfer_items");
    assert!(generated_table.join("TransferItemsDO.java").is_file());
    assert!(generated_table.join("TransferItemsMapper.java").is_file());
    assert!(generated_table.join("TransferItemsMapper.xml").is_file());
    assert_java_dormant(application);
}

async fn wait_for_transfer_task(router: &Router, task_id: i64) -> Value {
    for _ in 0..300 {
        let task = get(router, &format!("/api/task/get?id={task_id}")).await;
        match task["taskStatus"].as_str() {
            Some("FINISHED") => return task,
            Some("ERROR" | "STOP") => panic!("transfer task did not succeed: {task}"),
            _ => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    panic!("transfer task {task_id} did not finish before timeout")
}

async fn download_attachment(
    router: &Router,
    method: Method,
    path: &str,
    payload: Option<Value>,
) -> Vec<u8> {
    let builder = Request::builder().method(method).uri(path);
    let (builder, body) = if let Some(payload) = payload {
        (
            builder.header("content-type", "application/json"),
            Body::from(serde_json::to_vec(&payload).expect("payload must encode")),
        )
    } else {
        (builder, Body::empty())
    };
    let response = router
        .clone()
        .oneshot(builder.body(body).expect("request must build"))
        .await
        .expect("attachment route must respond");
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    assert!(
        response
            .headers()
            .get("content-disposition")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("attachment;")),
        "{path} did not return an attachment"
    );
    response
        .into_body()
        .collect()
        .await
        .expect("attachment body must collect")
        .to_bytes()
        .to_vec()
}

async fn verify_converter_routes(
    router: &Router,
    application: &Application,
    config: &MysqlTestConfig,
    database_name: &str,
    directory: &Path,
) {
    let jdbc_url = mysql_test_jdbc_url(config, database_name);

    let desktop_file = directory.join("desktop-chat2db-import.json");
    fs::write(
        &desktop_file,
        serde_json::to_vec(&json!([{
            "alias": "Desktop imported MySQL",
            "type": "MYSQL",
            "url": jdbc_url.as_str(),
            "user": config.user.as_str(),
            "password": config.password.as_str(),
            "extendInfo": [{"key": "connectionTimeZone", "value": "LOCAL"}]
        }]))
        .expect("desktop import JSON must encode"),
    )
    .expect("desktop import JSON must write");
    let desktop = chat2db_web::legacy::dispatch_desktop(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/converter/chat2db/upload".to_owned(),
            method: "post".to_owned(),
            message: json!({"file": [desktop_file.to_string_lossy()]}),
        },
    )
    .await;
    assert_eq!(desktop["success"], true, "desktop import failed: {desktop}");
    assert_eq!(desktop["data"]["count"], 1);

    let web_document = serde_json::to_vec(&json!([{
        "alias": "Web imported MySQL",
        "type": "MYSQL",
        "url": jdbc_url.as_str(),
        "user": config.user.as_str(),
        "password": "must-not-be-imported"
    }]))
    .expect("Web import JSON must encode");
    let web = multipart_upload(
        router,
        "/api/converter/upload",
        "connections.json",
        &web_document,
    )
    .await;
    assert_eq!(web["count"], 1);

    let datagrip = post(
        router,
        "/api/converter/datagrip/upload",
        json!({
            "text": format!(
                "#DataSourceSettings#\n#BEGIN#\n\
                 <data-source name=\"DataGrip imported MySQL\">\n\
                 <database-info dbms=\"MYSQL\"/>\n\
                 <jdbc-url>{jdbc_url}</jdbc-url>\n\
                 <user-name>{}</user-name>\n\
                 </data-source>\n#END#\n",
                config.user
            )
        }),
    )
    .await;
    assert_eq!(datagrip["count"], 1);

    let list = get(
        router,
        "/api/connection/datasource/list?pageNo=1&pageSize=20",
    )
    .await;
    assert_password_fields_empty(&list);
    for alias in [
        "Desktop imported MySQL",
        "Web imported MySQL",
        "DataGrip imported MySQL",
    ] {
        let imported = list["data"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["alias"] == alias))
            .unwrap_or_else(|| panic!("missing imported datasource {alias}: {list}"));
        assert_eq!(imported["password"], "");
        assert!(
            imported["url"]
                .as_str()
                .is_some_and(|url| url.contains(database_name))
        );
        let id = imported["id"].as_str().expect("imported datasource id");
        let deleted = request(
            router,
            Method::DELETE,
            &format!("/api/connection/datasource?id={id}"),
            None,
        )
        .await;
        assert_eq!(deleted["success"], true, "import cleanup failed: {deleted}");
    }
    assert_java_dormant(application);
}

fn mysql_test_jdbc_url(config: &MysqlTestConfig, database_name: &str) -> String {
    let host = if config.host.contains(':')
        && !(config.host.starts_with('[') && config.host.ends_with(']'))
    {
        format!("[{}]", config.host)
    } else {
        config.host.clone()
    };
    format!(
        "jdbc:mysql://{host}:{}/{database_name}?useSSL=false",
        config.port
    )
}

async fn multipart_mysql_import(
    router: &Router,
    path: &str,
    fields: &[(&str, &str)],
    file_name: &str,
    content: &[u8],
) -> Value {
    let boundary = "chat2db-rust-mysql-import-boundary";
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(path)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .expect("multipart import request must build"),
        )
        .await
        .expect("multipart import route must respond");
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    let envelope: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("multipart import response must collect")
            .to_bytes(),
    )
    .expect("multipart import response must be JSON");
    assert_eq!(envelope["success"], true, "multipart failed: {envelope}");
    envelope["data"].clone()
}

async fn multipart_upload(router: &Router, path: &str, file_name: &str, content: &[u8]) -> Value {
    let boundary = "chat2db-rust-product-boundary";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(path)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .expect("multipart request must build"),
        )
        .await
        .expect("multipart route must respond");
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    let envelope: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("multipart response must collect")
            .to_bytes(),
    )
    .expect("multipart response must be JSON");
    assert_eq!(envelope["success"], true, "multipart failed: {envelope}");
    envelope["data"].clone()
}

async fn verify_workspace_routes(
    router: &Router,
    application: &Application,
    config: &MysqlTestConfig,
    datasource_id: &str,
    database_name: &str,
    directory: &Path,
) {
    verify_native_driver_and_connection_routes(router, application, datasource_id, database_name)
        .await;
    verify_datasource_edit_route(router, config, datasource_id, database_name).await;
    verify_converter_routes(router, application, config, database_name, directory).await;
    let clone_id = clone_and_export_datasource(router, application, datasource_id).await;
    verify_namespace_routes(
        router,
        application,
        config,
        datasource_id,
        database_name,
        &clone_id,
    )
    .await;
    close_and_delete_clone(router, application, datasource_id, &clone_id).await;
    assert_java_dormant(application);
}

async fn verify_native_driver_and_connection_routes(
    router: &Router,
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    assert!(
        get(router, "/api/jdbc/driver/download?dbType=MYSQL")
            .await
            .is_null()
    );
    for (path, method) in [
        ("/api/jdbc/driver/save", "post"),
        ("/api/jdbc/driver/delete", "delete"),
    ] {
        let response = chat2db_web::legacy::dispatch(
            application,
            chat2db_web::legacy::LegacyDispatchRequest {
                request_url: path.to_owned(),
                method: method.to_owned(),
                message: json!({
                    "dbType": "MYSQL",
                    "jdbcDriverClass": "rust:mysql_async",
                    "jdbcDriver": []
                }),
            },
        )
        .await;
        assert_eq!(
            response["success"], true,
            "native driver route failed: {response}"
        );
    }

    let databases = get(
        router,
        &format!("/api/connection/datasource/connect?id={datasource_id}"),
    )
    .await;
    assert!(databases.as_array().is_some_and(|databases| {
        databases
            .iter()
            .any(|database| database["name"] == database_name)
    }));
    let console = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/connection/console/connect".to_owned(),
            method: "get".to_owned(),
            message: json!({
                "consoleId": 1,
                "dataSourceId": datasource_id,
                "databaseName": database_name
            }),
        },
    )
    .await;
    assert_eq!(
        console["success"], true,
        "desktop Console connect failed: {console}"
    );
}

async fn verify_datasource_edit_route(
    router: &Router,
    config: &MysqlTestConfig,
    datasource_id: &str,
    database_name: &str,
) {
    let detail = get(
        router,
        &format!("/api/connection/datasource?id={datasource_id}"),
    )
    .await;
    assert_eq!(detail["user"], config.user.as_str());
    assert_eq!(detail["password"], "");
    assert_eq!(detail["readOnly"], false);
    assert!(
        detail["url"]
            .as_str()
            .is_some_and(|url| url.contains(database_name) && !url.contains(&config.password))
    );
    let updated = post(
        router,
        "/api/connection/datasource/update",
        json!({
            "id": datasource_id,
            "alias": "MySQL editable updated",
            "type": "MYSQL",
            "url": detail["url"],
            "user": detail["user"],
            "password": "",
            "readOnly": false,
            "extendInfo": [{"key": "connectionTimeZone", "value": "LOCAL"}]
        }),
    )
    .await;
    assert_eq!(updated["alias"], "MySQL editable updated");
    assert_eq!(updated["password"], "");
    assert_eq!(updated["extendInfo"][0]["key"], "connectionTimeZone");
    assert_eq!(updated["extendInfo"][0]["value"], "LOCAL");
    assert!(
        get(
            router,
            &format!("/api/connection/datasource/connect?id={datasource_id}"),
        )
        .await
        .as_array()
        .is_some_and(|databases| databases
            .iter()
            .any(|database| database["name"] == database_name)),
        "empty-password edit must preserve the stored MySQL password"
    );
}

async fn clone_and_export_datasource(
    router: &Router,
    application: &Application,
    datasource_id: &str,
) -> String {
    let clone_id = post(
        router,
        "/api/connection/datasource/clone",
        json!({"id": datasource_id, "name": "MySQL editable clone"}),
    )
    .await
    .as_str()
    .expect("clone id")
    .to_owned();
    let clone_connect = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/connection/datasource/connect".to_owned(),
            method: "get".to_owned(),
            message: json!({"id": clone_id}),
        },
    )
    .await;
    assert_eq!(
        clone_connect["success"], true,
        "cloned datasource cannot connect"
    );

    let exported = post(
        router,
        "/api/connection/datasource/export",
        json!({"datasourceIds": [datasource_id]}),
    )
    .await;
    assert_eq!(exported["count"], 1);
    let export_message = exported["message"].as_str().expect("export document");
    let export_document: Value = serde_json::from_str(export_message).expect("export JSON");
    assert_eq!(export_document["schemaVersion"], 1);
    assert_eq!(export_document["datasources"][0]["sourceId"], datasource_id);
    let exported_connection = &export_document["datasources"][0]["connection"];
    assert!(
        exported_connection["jdbcUrl"]
            .as_str()
            .is_some_and(|url| !url.contains('@'))
    );
    assert!(
        exported_connection["properties"]
            .as_array()
            .is_some_and(|properties| {
                properties.iter().all(|property| {
                    property["key"].as_str().is_some_and(|key| {
                        !matches!(
                            key.to_ascii_lowercase().as_str(),
                            "password" | "passwd" | "pwd" | "token" | "secret"
                        )
                    })
                })
            })
    );
    clone_id
}

async fn verify_namespace_routes(
    router: &Router,
    application: &Application,
    config: &MysqlTestConfig,
    datasource_id: &str,
    database_name: &str,
    clone_id: &str,
) {
    let namespace_id = post(
        router,
        "/api/namespaces/create",
        json!({"name": "Integration"}),
    )
    .await
    .as_str()
    .expect("namespace id")
    .to_owned();
    post(
        router,
        "/api/namespaces/update",
        json!({"id": namespace_id, "name": "Integration renamed"}),
    )
    .await;
    post(
        router,
        "/api/namespaces/update_position",
        json!({
            "dragNode": {"id": datasource_id, "type": "DATA_SOURCE"},
            "dropToNode": {"id": namespace_id, "type": "NAMESPACE"},
            "dropPosition": 2
        }),
    )
    .await;
    let desktop_move = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/namespaces/update_position".to_owned(),
            method: "post".to_owned(),
            message: json!({
                "dragNode": {"id": clone_id, "type": "DATA_SOURCE"},
                "dropToNode": {"id": namespace_id, "type": "NAMESPACE"},
                "dropPosition": 2
            }),
        },
    )
    .await;
    assert_eq!(
        desktop_move["success"], true,
        "desktop namespace move failed"
    );

    let tree = get(router, "/api/namespaces/tree_list").await;
    assert_password_fields_empty(&tree);
    let namespace = tree
        .as_array()
        .and_then(|nodes| nodes.iter().find(|node| node["id"] == namespace_id))
        .expect("namespace in tree");
    assert_eq!(namespace["data"]["name"], "Integration renamed");
    assert_eq!(namespace["children"][0]["id"], datasource_id);
    assert_eq!(namespace["children"][1]["id"], clone_id);
    assert_eq!(namespace["children"][0]["data"]["password"], "");
    assert_eq!(
        namespace["children"][0]["data"]["user"],
        config.user.as_str()
    );
    assert!(
        namespace["children"][0]["data"]["url"]
            .as_str()
            .is_some_and(|url| url.contains(database_name))
    );

    post(
        router,
        "/api/namespaces/delete",
        json!({"id": namespace_id}),
    )
    .await;
    let promoted = get(router, "/api/namespaces/tree_list").await;
    assert!(promoted.as_array().is_some_and(|nodes| {
        nodes.iter().any(|node| node["id"] == datasource_id)
            && nodes.iter().any(|node| node["id"] == clone_id)
    }));
}

async fn close_and_delete_clone(
    router: &Router,
    application: &Application,
    datasource_id: &str,
    clone_id: &str,
) {
    let desktop_close = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/connection/close".to_owned(),
            method: "get".to_owned(),
            message: json!({"id": datasource_id}),
        },
    )
    .await;
    assert_eq!(desktop_close["success"], true);
    assert!(
        post(
            router,
            "/api/connection/datasource/close",
            json!({"id": clone_id})
        )
        .await
        .is_null()
    );
    let deleted = request(
        router,
        Method::DELETE,
        &format!("/api/connection/datasource?id={clone_id}"),
        None,
    )
    .await;
    assert_eq!(deleted["success"], true, "clone cleanup failed: {deleted}");
}

fn assert_password_fields_empty(value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if key.eq_ignore_ascii_case("password") {
                    assert!(
                        value.is_null() || value.as_str() == Some(""),
                        "password field leaked in response"
                    );
                } else {
                    assert_password_fields_empty(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                assert_password_fields_empty(value);
            }
        }
        _ => {}
    }
}

async fn verify_routine_invocation_preview(
    router: &Router,
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    install_test_routines(router, datasource_id, database_name).await;
    verify_function_invocation_preview(router, application, datasource_id, database_name).await;
    verify_procedure_invocation_preview(router, application, datasource_id, database_name).await;

    let migration_request = json!({
        "dataSourceId": datasource_id,
        "databaseType": "MYSQL",
        "databaseName": database_name,
        "schemaName": "",
        "routineType": "FUNCTION",
        "routineName": "`routine``add`",
        "ddl": format!(
            "CREATE FUNCTION `{database_name}`.`routine``add`(input_value INT) RETURNS INT \
             DETERMINISTIC NO SQL RETURN input_value + 2"
        )
    });
    let migration_preview = post(
        router,
        "/api/rdb/routine/preview_migration",
        migration_request.clone(),
    )
    .await;
    assert!(
        migration_preview["sql"]
            .as_str()
            .expect("migration preview SQL")
            .starts_with(&format!(
                "DROP FUNCTION IF EXISTS `{database_name}`.`routine``add`;"
            ))
    );
    let desktop_preview = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/rdb/routine/preview_migration".to_owned(),
            method: "post".to_owned(),
            message: migration_request.clone(),
        },
    )
    .await;
    assert_eq!(desktop_preview["success"], true);
    assert_eq!(desktop_preview["data"], migration_preview);

    let migrated = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/rdb/routine/execute_migration".to_owned(),
            method: "post".to_owned(),
            message: migration_request.clone(),
        },
    )
    .await;
    assert_eq!(migrated["success"], true, "desktop migration failed");
    assert_eq!(migrated["data"]["success"], true, "{migrated}");
    let migrated_results = execute_console_sql(
        router,
        datasource_id,
        database_name,
        "SELECT `routine``add`(0)",
    )
    .await;
    assert_eq!(last_console_values(&migrated_results), json!(["2"]));

    let mut failed_migration_request = migration_request.clone();
    failed_migration_request
        .as_object_mut()
        .expect("migration request object")
        .insert(
            "ddl".to_owned(),
            json!(format!(
                "CREATE FUNCTION `{database_name}`.`routine``add`(input_value INT) RETURNS INT RETURN"
            )),
        );
    let failed = post(
        router,
        "/api/rdb/routine/execute_migration",
        failed_migration_request,
    )
    .await;
    assert_eq!(failed["success"], false);
    assert_eq!(failed["failureStage"], "APPLY");
    assert_eq!(failed["restoreAttempted"], true);
    assert_eq!(failed["restoreSucceeded"], true);
    let restored_results = execute_console_sql(
        router,
        datasource_id,
        database_name,
        "SELECT `routine``add`(0)",
    )
    .await;
    assert_eq!(last_console_values(&restored_results), json!(["2"]));
    assert_java_dormant(application);
}

async fn install_test_routines(router: &Router, datasource_id: &str, database_name: &str) {
    execute_ddl(
        router,
        datasource_id,
        database_name,
        "",
        "CREATE FUNCTION `routine``add`(input_value INT) RETURNS INT \
         DETERMINISTIC NO SQL RETURN input_value + 1;",
    )
    .await;
    execute_ddl(
        router,
        datasource_id,
        database_name,
        "",
        "CREATE PROCEDURE routine_mix(\
             IN input_value INT, OUT output_text VARCHAR(32), INOUT running_total BIGINT\
         ) BEGIN \
             SET output_text = CONCAT('v', input_value); \
             SET running_total = running_total + input_value + 7; \
         END;",
    )
    .await;
}

async fn verify_function_invocation_preview(
    router: &Router,
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let function_request = json!({
        "dataSourceId": datasource_id,
        "databaseName": database_name,
        "schemaName": null,
        "routineType": "FUNCTION",
        "routineName": "`routine``add`"
    });
    let function_preview = post(
        router,
        "/api/rdb/routine/preview_invocation",
        function_request.clone(),
    )
    .await;
    assert_eq!(
        function_preview["sql"],
        "set @input_value = 0;\n\nselect `routine``add`(\n    @input_value\n);"
    );
    assert_desktop_routine_preview_matches(application, function_request, &function_preview).await;
    let function_results = execute_console_sql(
        router,
        datasource_id,
        database_name,
        function_preview["sql"]
            .as_str()
            .expect("function preview SQL"),
    )
    .await;
    assert_eq!(last_console_values(&function_results), json!(["1"]));
}

async fn verify_procedure_invocation_preview(
    router: &Router,
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let procedure_request = json!({
        "dataSourceId": datasource_id,
        "databaseType": "MYSQL",
        "databaseName": database_name,
        "schemaName": "",
        "routineType": "PROCEDURE",
        "routineName": "routine_mix"
    });
    let procedure_preview = post(
        router,
        "/api/rdb/routine/preview_invocation",
        procedure_request.clone(),
    )
    .await;
    assert_eq!(
        procedure_preview["sql"],
        "set @input_value = 0;\nset @running_total = 0;\n\n\
         call routine_mix(\n    @input_value,\n    @output_text,\n    @running_total\n);\n\
         select @output_text, @running_total;"
    );
    assert_desktop_routine_preview_matches(application, procedure_request, &procedure_preview)
        .await;
    let procedure_results = execute_console_sql(
        router,
        datasource_id,
        database_name,
        procedure_preview["sql"]
            .as_str()
            .expect("procedure preview SQL"),
    )
    .await;
    assert_eq!(last_console_values(&procedure_results), json!(["v0", "7"]));
}

async fn assert_desktop_routine_preview_matches(
    application: &Application,
    message: Value,
    http_preview: &Value,
) {
    let desktop = chat2db_web::legacy::dispatch(
        application,
        chat2db_web::legacy::LegacyDispatchRequest {
            request_url: "/api/rdb/routine/preview_invocation".to_owned(),
            method: "post".to_owned(),
            message,
        },
    )
    .await;
    assert_eq!(desktop["success"], true, "desktop routine preview failed");
    assert_eq!(desktop["data"], *http_preview, "HTTP/desktop SQL diverged");
}

async fn execute_console_sql(
    router: &Router,
    datasource_id: &str,
    database_name: &str,
    sql: &str,
) -> Value {
    let results = post(
        router,
        "/api/rdb/dml/execute",
        json!({
            "dataSourceId": datasource_id,
            "databaseType": "MYSQL",
            "databaseName": database_name,
            "schemaName": "",
            "tableName": "",
            "sql": sql,
            "single": false,
            "pageNo": 1,
            "pageSize": 20,
            "errorContinue": false
        }),
    )
    .await;
    let items = results.as_array().expect("console result list");
    assert!(!items.is_empty(), "console must return at least one result");
    assert!(
        items.iter().all(|item| item["success"] == true),
        "console execution failed: {results}"
    );
    results
}

fn last_console_values(results: &Value) -> Value {
    let result = results
        .as_array()
        .expect("console result list")
        .iter()
        .rev()
        .find(|result| {
            result["dataList"]
                .as_array()
                .is_some_and(|rows| !rows.is_empty())
        })
        .expect("console must return a row-bearing result");
    cell_values(&result["dataList"][0])
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

async fn index_exists(
    config: &MysqlTestConfig,
    database_name: &str,
    table_name: &str,
    index_name: &str,
) -> bool {
    let mut conn = Conn::new(config.options())
        .await
        .expect("index probe must connect");
    let count = conn
        .exec_first::<u64, _, _>(
            "SELECT COUNT(*) FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND INDEX_NAME = ?",
            (database_name, table_name, index_name),
        )
        .await
        .expect("index probe must execute")
        .unwrap_or_default();
    conn.disconnect().await.expect("index probe must close");
    count > 0
}

async fn column_order(
    config: &MysqlTestConfig,
    database_name: &str,
    table_name: &str,
) -> Vec<String> {
    let mut conn = Conn::new(config.options())
        .await
        .expect("column-order probe must connect");
    let columns = conn
        .exec::<String, _, _>(
            "SELECT COLUMN_NAME FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
            (database_name, table_name),
        )
        .await
        .expect("column-order probe must execute");
    conn.disconnect()
        .await
        .expect("column-order probe must close");
    columns
}

async fn column_definition(
    config: &MysqlTestConfig,
    database_name: &str,
    table_name: &str,
    column_name: &str,
) -> String {
    let mut conn = Conn::new(config.options())
        .await
        .expect("column-definition probe must connect");
    let definition = conn
        .exec_first::<String, _, _>(
            "SELECT COLUMN_TYPE FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND COLUMN_NAME = ?",
            (database_name, table_name, column_name),
        )
        .await
        .expect("column-definition probe must execute")
        .expect("column definition must exist");
    conn.disconnect()
        .await
        .expect("column-definition probe must close");
    definition
}

async fn column_extra(
    config: &MysqlTestConfig,
    database_name: &str,
    table_name: &str,
    column_name: &str,
) -> String {
    let mut conn = Conn::new(config.options())
        .await
        .expect("column-extra probe must connect");
    let extra = conn
        .exec_first::<String, _, _>(
            "SELECT COALESCE(EXTRA, '') FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND COLUMN_NAME = ?",
            (database_name, table_name, column_name),
        )
        .await
        .expect("column-extra probe must execute")
        .expect("column extra must exist");
    conn.disconnect()
        .await
        .expect("column-extra probe must close");
    extra
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
