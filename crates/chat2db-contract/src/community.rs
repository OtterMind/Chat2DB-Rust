use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One JDBC driver declaration retained from a Community plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityDriverConfig {
    /// JDBC URL template declared by the plugin.
    pub url: String,
    /// Driver artifact name declared by Community.
    pub jdbc_driver: String,
    /// JDBC driver implementation class.
    pub jdbc_driver_class: String,
    /// Ordered upstream download locations declared by Community.
    pub download_urls: Vec<String>,
    /// Whether this is a user-supplied driver declaration.
    pub custom: bool,
    /// Whether this is the plugin's default driver declaration.
    pub default_driver: bool,
}

/// Database-model and script behavior declared by one Community plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityPluginBehavior {
    /// Whether the plugin models databases as metadata containers.
    pub supports_database: bool,
    /// Whether the plugin models schemas as metadata containers.
    pub supports_schema: bool,
    /// Whether the plugin preserves script batches during execution.
    pub preserves_script_batch_execution: bool,
}

/// Optional Community services exposed by one plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityPluginServices {
    /// Whether schema metadata is available.
    pub metadata_available: bool,
    /// Whether dialect SQL building is available.
    pub sql_builder_available: bool,
    /// Whether retained SQL parsing is available.
    pub sql_parser_available: bool,
}

/// Stable product projection of one Community plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityPlugin {
    /// Community database type used to select the plugin.
    pub database_type: String,
    /// User-visible plugin name.
    pub name: String,
    /// Database-model and script behavior.
    pub behavior: CommunityPluginBehavior,
    /// JDBC driver declarations in Community order.
    pub drivers: Vec<CommunityDriverConfig>,
    /// Optional retained Community services.
    pub services: CommunityPluginServices,
}

/// Community plugin inventory for one fixed source commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityPluginCatalog {
    /// Full source commit backing the compatibility classpath.
    pub source_commit: String,
    /// Plugins in deterministic discovery order.
    pub plugins: Vec<CommunityPlugin>,
}

/// Secret-free database schema metadata returned by Community.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunitySchema {
    /// Database containing the schema.
    pub database_name: String,
    /// Schema name.
    pub name: String,
    /// Schema comment when supplied by the database.
    pub comment: String,
    /// Schema owner when supplied by the database.
    pub owner: String,
    /// Whether Community classifies the schema as system-owned.
    pub system: bool,
}

/// Stable schema collection returned by Community metadata APIs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunitySchemaList {
    /// Schemas in compatibility-engine order.
    pub items: Vec<CommunitySchema>,
}

/// Request to list schemas through one datasource connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListCommunitySchemasRequest {
    /// Datasource whose installed connection descriptor is used.
    pub datasource_id: String,
    /// Community database type used to select the metadata plugin.
    pub database_type: String,
    /// Database name supplied to Community metadata.
    pub database_name: String,
}

/// Secret-free database metadata returned by Community.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityDatabase {
    pub name: String,
    pub comment: String,
    pub charset: String,
    pub collation: String,
    pub owner: String,
    pub system: bool,
}

/// Stable database collection returned by Community metadata APIs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityDatabaseList {
    pub items: Vec<CommunityDatabase>,
}

/// Request to list databases through one datasource connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListCommunityDatabasesRequest {
    pub datasource_id: String,
    pub database_type: String,
}

/// Secret-free table metadata without nested column or index payloads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityTable {
    pub database_name: String,
    pub schema_name: String,
    pub name: String,
    pub table_type: String,
    pub comment: String,
    pub database_type: String,
    pub pinned: bool,
    pub ddl: String,
    pub engine: String,
    pub charset: String,
    pub collation: String,
    /// Auto-increment value encoded as a decimal integer string.
    pub increment_value: Option<String>,
    pub partition: String,
    pub tablespace: String,
    /// Row count or estimate encoded as a decimal integer string.
    pub rows: Option<String>,
    /// Data length encoded as a decimal integer string.
    pub data_length: Option<String>,
    pub create_time: String,
    pub update_time: String,
}

/// Stable table collection returned by Community metadata APIs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityTableList {
    pub items: Vec<CommunityTable>,
}

/// Request to list tables through one datasource connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListCommunityTablesRequest {
    pub datasource_id: String,
    pub database_type: String,
    pub database_name: String,
    pub schema_name: String,
    pub table_name_pattern: String,
}

/// Secret-free Community table-column metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityTableColumn {
    pub database_name: String,
    pub schema_name: String,
    pub table_name: String,
    pub name: String,
    pub column_type: String,
    pub data_type: Option<i32>,
    pub default_value: String,
    pub auto_increment: Option<bool>,
    pub comment: String,
    pub primary_key: Option<bool>,
    pub primary_key_name: String,
    pub primary_key_order: i32,
    pub column_size: Option<i32>,
    pub buffer_length: Option<i32>,
    pub decimal_digits: Option<i32>,
    pub num_prec_radix: Option<i32>,
    pub sql_data_type: Option<i32>,
    pub sql_datetime_sub: Option<i32>,
    pub char_octet_length: Option<i32>,
    pub ordinal_position: Option<i32>,
    pub nullable: Option<i32>,
    pub generated_column: Option<bool>,
    pub extent: String,
    pub charset: String,
    pub collation: String,
    pub unit: String,
    pub sparse: Option<bool>,
    pub default_constraint_name: String,
    pub seed: Option<i32>,
    pub increment: Option<i32>,
    pub on_update_current_timestamp: Option<bool>,
}

/// Stable column collection returned by Community metadata APIs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityTableColumnList {
    pub items: Vec<CommunityTableColumn>,
}

/// Request to list columns for one table through Community metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListCommunityColumnsRequest {
    pub datasource_id: String,
    pub database_type: String,
    pub database_name: String,
    pub schema_name: String,
    pub table_name: String,
}

/// Secret-free metadata for one indexed column.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityTableIndexColumn {
    pub database_name: String,
    pub schema_name: String,
    pub table_name: String,
    pub index_name: String,
    pub column_name: String,
    pub column_type: String,
    pub comment: String,
    pub ordinal_position: Option<i32>,
    pub collation: String,
    pub non_unique: Option<bool>,
    pub index_qualifier: String,
    pub sort_order: String,
    /// Index cardinality encoded as a decimal integer string.
    pub cardinality: Option<String>,
    /// Index page count encoded as a decimal integer string.
    pub pages: Option<String>,
    pub filter_condition: String,
    /// Indexed prefix length encoded as a decimal integer string.
    pub sub_part: Option<String>,
}

/// Secret-free Community table-index metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityTableIndex {
    pub database_name: String,
    pub schema_name: String,
    pub table_name: String,
    pub name: String,
    pub index_type: String,
    pub unique: Option<bool>,
    pub comment: String,
    pub columns: Vec<CommunityTableIndexColumn>,
    pub concurrently: Option<bool>,
    pub method: String,
    pub foreign_schema_name: String,
    pub foreign_table_name: String,
    pub foreign_column_names: Vec<String>,
}

/// Stable index collection returned by Community metadata APIs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityTableIndexList {
    pub items: Vec<CommunityTableIndex>,
}

/// Request to list indexes for one table through Community metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListCommunityIndexesRequest {
    pub datasource_id: String,
    pub database_type: String,
    pub database_name: String,
    pub schema_name: String,
    pub table_name: String,
}

/// Request to build dialect-specific `CREATE SCHEMA` SQL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildCommunityCreateSchemaRequest {
    /// Community database type used to select the SQL builder.
    pub database_type: String,
    /// Schema description passed to the retained builder.
    pub schema: CommunitySchema,
}

/// SQL built by a retained Community dialect service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityBuiltSql {
    /// Complete SQL generated by Community.
    pub sql: String,
}

/// Request to analyze SQL through a retained Community parser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParseCommunitySqlRequest {
    /// Community database type used to select the parser.
    pub database_type: String,
    /// SQL text to analyze.
    pub sql: String,
}

/// One statement returned by the retained Community parser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunityParsedStatement {
    /// Statement SQL projected by Community.
    pub sql: String,
    /// Community statement type.
    pub statement_type: String,
    /// Bounded parser-specific statement kind.
    pub kind: String,
}

/// Bounded Community parser projection for one SQL input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommunitySqlAnalysis {
    /// Whether Community classifies the complete input as a select operation.
    pub is_select: bool,
    /// Parsed statements in source order.
    pub statements: Vec<CommunityParsedStatement>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        BuildCommunityCreateSchemaRequest, CommunityBuiltSql, CommunityDatabase,
        CommunityDatabaseList, CommunityDriverConfig, CommunityParsedStatement, CommunityPlugin,
        CommunityPluginBehavior, CommunityPluginCatalog, CommunityPluginServices, CommunitySchema,
        CommunitySchemaList, CommunitySqlAnalysis, CommunityTable, CommunityTableColumn,
        CommunityTableColumnList, CommunityTableIndex, CommunityTableIndexColumn,
        CommunityTableIndexList, CommunityTableList, ListCommunityColumnsRequest,
        ListCommunityDatabasesRequest, ListCommunityIndexesRequest, ListCommunitySchemasRequest,
        ListCommunityTablesRequest, ParseCommunitySqlRequest,
    };

    #[test]
    fn community_contracts_use_exact_camel_case_json() {
        let catalog = CommunityPluginCatalog {
            source_commit: "f63cbf4".to_owned(),
            plugins: vec![CommunityPlugin {
                database_type: "H2".to_owned(),
                name: "H2".to_owned(),
                behavior: CommunityPluginBehavior {
                    supports_database: true,
                    supports_schema: true,
                    preserves_script_batch_execution: false,
                },
                drivers: vec![CommunityDriverConfig {
                    url: "jdbc:h2:mem:test".to_owned(),
                    jdbc_driver: "h2.jar".to_owned(),
                    jdbc_driver_class: "org.h2.Driver".to_owned(),
                    download_urls: vec!["https://example.invalid/h2.jar".to_owned()],
                    custom: false,
                    default_driver: true,
                }],
                services: CommunityPluginServices {
                    metadata_available: true,
                    sql_builder_available: true,
                    sql_parser_available: true,
                },
            }],
        };

        let value = serde_json::to_value(&catalog).expect("catalog must serialize");
        assert_eq!(value["sourceCommit"], "f63cbf4");
        assert_eq!(value["plugins"][0]["databaseType"], "H2");
        assert_eq!(value["plugins"][0]["behavior"]["supportsSchema"], true);
        assert_eq!(
            value["plugins"][0]["behavior"]["preservesScriptBatchExecution"],
            false
        );
        assert_eq!(
            value["plugins"][0]["drivers"][0]["jdbcDriverClass"],
            "org.h2.Driver"
        );
        assert_eq!(value["plugins"][0]["services"]["sqlParserAvailable"], true);
        assert_eq!(
            serde_json::from_value::<CommunityPluginCatalog>(value)
                .expect("catalog must deserialize"),
            catalog
        );
    }

    #[test]
    fn community_requests_and_responses_round_trip() {
        let schema = CommunitySchema {
            database_name: "inventory".to_owned(),
            name: "reporting".to_owned(),
            comment: "Reporting objects".to_owned(),
            owner: "app".to_owned(),
            system: false,
        };
        let list_request = ListCommunitySchemasRequest {
            datasource_id: "datasource-1".to_owned(),
            database_type: "H2".to_owned(),
            database_name: "inventory".to_owned(),
        };
        let build_request = BuildCommunityCreateSchemaRequest {
            database_type: "H2".to_owned(),
            schema: schema.clone(),
        };
        let parse_request = ParseCommunitySqlRequest {
            database_type: "H2".to_owned(),
            sql: "select 1".to_owned(),
        };
        let schema_list = CommunitySchemaList {
            items: vec![schema],
        };
        let built = CommunityBuiltSql {
            sql: "CREATE SCHEMA reporting".to_owned(),
        };
        let analysis = CommunitySqlAnalysis {
            is_select: true,
            statements: vec![CommunityParsedStatement {
                sql: "select 1".to_owned(),
                statement_type: "SELECT".to_owned(),
                kind: "Select".to_owned(),
            }],
        };

        assert_eq!(
            serde_json::to_value(&list_request).expect("request must serialize"),
            json!({
                "datasourceId": "datasource-1",
                "databaseType": "H2",
                "databaseName": "inventory"
            })
        );
        assert_round_trip(&build_request);
        assert_round_trip(&parse_request);
        assert_round_trip(&schema_list);
        assert_round_trip(&built);
        assert_round_trip(&analysis);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn community_object_metadata_contracts_use_exact_camel_case_and_round_trip() {
        let database_request = ListCommunityDatabasesRequest {
            datasource_id: "datasource-1".to_owned(),
            database_type: "H2".to_owned(),
        };
        let table_request = ListCommunityTablesRequest {
            datasource_id: "datasource-1".to_owned(),
            database_type: "H2".to_owned(),
            database_name: "inventory".to_owned(),
            schema_name: "APP".to_owned(),
            table_name_pattern: "item%".to_owned(),
        };
        let column_request = ListCommunityColumnsRequest {
            datasource_id: "datasource-1".to_owned(),
            database_type: "H2".to_owned(),
            database_name: "inventory".to_owned(),
            schema_name: "APP".to_owned(),
            table_name: "items".to_owned(),
        };
        let index_request = ListCommunityIndexesRequest {
            datasource_id: "datasource-1".to_owned(),
            database_type: "H2".to_owned(),
            database_name: "inventory".to_owned(),
            schema_name: "APP".to_owned(),
            table_name: "items".to_owned(),
        };

        assert_eq!(
            serde_json::to_value(&database_request).expect("database request must serialize"),
            json!({"datasourceId": "datasource-1", "databaseType": "H2"})
        );
        assert_eq!(
            serde_json::to_value(&table_request).expect("table request must serialize"),
            json!({
                "datasourceId": "datasource-1",
                "databaseType": "H2",
                "databaseName": "inventory",
                "schemaName": "APP",
                "tableNamePattern": "item%"
            })
        );
        assert_eq!(
            serde_json::to_value(&column_request).expect("column request must serialize"),
            json!({
                "datasourceId": "datasource-1",
                "databaseType": "H2",
                "databaseName": "inventory",
                "schemaName": "APP",
                "tableName": "items"
            })
        );
        assert_eq!(
            serde_json::to_value(&index_request).expect("index request must serialize"),
            json!({
                "datasourceId": "datasource-1",
                "databaseType": "H2",
                "databaseName": "inventory",
                "schemaName": "APP",
                "tableName": "items"
            })
        );

        let databases = CommunityDatabaseList {
            items: vec![database_fixture()],
        };
        let tables = CommunityTableList {
            items: vec![table_fixture()],
        };
        let columns = CommunityTableColumnList {
            items: vec![column_fixture()],
        };
        let indexes = CommunityTableIndexList {
            items: vec![index_fixture()],
        };

        assert_eq!(
            serde_json::to_value(&databases).expect("database list must serialize"),
            json!({"items": [{
                "name": "inventory",
                "comment": "Inventory catalog",
                "charset": "UTF-8",
                "collation": "en_US",
                "owner": "app",
                "system": false
            }]})
        );
        assert_eq!(
            serde_json::to_value(&tables).expect("table list must serialize"),
            json!({"items": [{
                "databaseName": "inventory",
                "schemaName": "APP",
                "name": "items",
                "tableType": "TABLE",
                "comment": "Inventory items",
                "databaseType": "H2",
                "pinned": true,
                "ddl": "CREATE TABLE APP.items (...) ",
                "engine": "MVStore",
                "charset": "UTF-8",
                "collation": "en_US",
                "incrementValue": "9007199254740993",
                "partition": "HASH(id)",
                "tablespace": "main",
                "rows": "9007199254740994",
                "dataLength": "9223372036854775807",
                "createTime": "2026-07-26T10:00:00Z",
                "updateTime": "2026-07-26T11:00:00Z"
            }]})
        );
        assert_eq!(
            serde_json::to_value(&columns).expect("column list must serialize"),
            json!({"items": [{
                "databaseName": "inventory",
                "schemaName": "APP",
                "tableName": "items",
                "name": "id",
                "columnType": "BIGINT",
                "dataType": -5,
                "defaultValue": "NEXT VALUE FOR seq_items",
                "autoIncrement": true,
                "comment": "Primary identifier",
                "primaryKey": true,
                "primaryKeyName": "pk_items",
                "primaryKeyOrder": 1,
                "columnSize": 64,
                "bufferLength": 8,
                "decimalDigits": 0,
                "numPrecRadix": 10,
                "sqlDataType": -5,
                "sqlDatetimeSub": 0,
                "charOctetLength": 8,
                "ordinalPosition": 1,
                "nullable": 0,
                "generatedColumn": false,
                "extent": "8",
                "charset": "UTF-8",
                "collation": "en_US",
                "unit": "bytes",
                "sparse": false,
                "defaultConstraintName": "df_items_id",
                "seed": 1,
                "increment": 2,
                "onUpdateCurrentTimestamp": false
            }]})
        );
        assert_eq!(
            serde_json::to_value(&indexes).expect("index list must serialize"),
            json!({"items": [{
                "databaseName": "inventory",
                "schemaName": "APP",
                "tableName": "items",
                "name": "idx_items_label",
                "indexType": "BTREE",
                "unique": true,
                "comment": "Unique item label",
                "columns": [{
                    "databaseName": "inventory",
                    "schemaName": "APP",
                    "tableName": "items",
                    "indexName": "idx_items_label",
                    "columnName": "label",
                    "columnType": "VARCHAR",
                    "comment": "Indexed label",
                    "ordinalPosition": 1,
                    "collation": "A",
                    "nonUnique": false,
                    "indexQualifier": "inventory",
                    "sortOrder": "ASC",
                    "cardinality": "9007199254740995",
                    "pages": "9007199254740996",
                    "filterCondition": "label IS NOT NULL",
                    "subPart": "9007199254740997"
                }],
                "concurrently": false,
                "method": "BTREE",
                "foreignSchemaName": "PUBLIC",
                "foreignTableName": "labels",
                "foreignColumnNames": ["id"]
            }]})
        );

        for request in [
            serde_json::to_value(&database_request).expect("database request must serialize"),
            serde_json::to_value(&table_request).expect("table request must serialize"),
            serde_json::to_value(&column_request).expect("column request must serialize"),
            serde_json::to_value(&index_request).expect("index request must serialize"),
        ] {
            assert!(request.get("datasource_id").is_none());
            assert!(request.get("database_type").is_none());
        }
        assert_round_trip(&database_request);
        assert_round_trip(&table_request);
        assert_round_trip(&column_request);
        assert_round_trip(&index_request);
        assert_round_trip(&databases);
        assert_round_trip(&tables);
        assert_round_trip(&columns);
        assert_round_trip(&indexes);
    }

    #[test]
    fn community_metadata_contracts_never_expose_connection_secrets() {
        let responses = json!([
            CommunitySchemaList {
                items: vec![CommunitySchema {
                    database_name: "inventory".to_owned(),
                    name: "public".to_owned(),
                    comment: String::new(),
                    owner: "app".to_owned(),
                    system: false,
                }],
            },
            CommunityDatabaseList {
                items: vec![database_fixture()],
            },
            CommunityTableList {
                items: vec![table_fixture()],
            },
            CommunityTableColumnList {
                items: vec![column_fixture()],
            },
            CommunityTableIndexList {
                items: vec![index_fixture()],
            }
        ]);

        let encoded = serde_json::to_string(&responses).expect("metadata lists must serialize");
        for forbidden in [
            "\"connection\"",
            "\"jdbcUrl\"",
            "\"password\"",
            "\"properties\"",
            "\"secretRef\"",
        ] {
            assert!(!encoded.contains(forbidden), "response leaked {forbidden}");
        }
    }

    fn assert_round_trip<T>(value: &T)
    where
        T: std::fmt::Debug + PartialEq + serde::Serialize + serde::de::DeserializeOwned,
    {
        let encoded = serde_json::to_value(value).expect("contract must serialize");
        let decoded = serde_json::from_value::<T>(encoded).expect("contract must deserialize");
        assert_eq!(&decoded, value);
    }

    fn database_fixture() -> CommunityDatabase {
        CommunityDatabase {
            name: "inventory".to_owned(),
            comment: "Inventory catalog".to_owned(),
            charset: "UTF-8".to_owned(),
            collation: "en_US".to_owned(),
            owner: "app".to_owned(),
            system: false,
        }
    }

    fn table_fixture() -> CommunityTable {
        CommunityTable {
            database_name: "inventory".to_owned(),
            schema_name: "APP".to_owned(),
            name: "items".to_owned(),
            table_type: "TABLE".to_owned(),
            comment: "Inventory items".to_owned(),
            database_type: "H2".to_owned(),
            pinned: true,
            ddl: "CREATE TABLE APP.items (...) ".to_owned(),
            engine: "MVStore".to_owned(),
            charset: "UTF-8".to_owned(),
            collation: "en_US".to_owned(),
            increment_value: Some("9007199254740993".to_owned()),
            partition: "HASH(id)".to_owned(),
            tablespace: "main".to_owned(),
            rows: Some("9007199254740994".to_owned()),
            data_length: Some("9223372036854775807".to_owned()),
            create_time: "2026-07-26T10:00:00Z".to_owned(),
            update_time: "2026-07-26T11:00:00Z".to_owned(),
        }
    }

    fn column_fixture() -> CommunityTableColumn {
        CommunityTableColumn {
            database_name: "inventory".to_owned(),
            schema_name: "APP".to_owned(),
            table_name: "items".to_owned(),
            name: "id".to_owned(),
            column_type: "BIGINT".to_owned(),
            data_type: Some(-5),
            default_value: "NEXT VALUE FOR seq_items".to_owned(),
            auto_increment: Some(true),
            comment: "Primary identifier".to_owned(),
            primary_key: Some(true),
            primary_key_name: "pk_items".to_owned(),
            primary_key_order: 1,
            column_size: Some(64),
            buffer_length: Some(8),
            decimal_digits: Some(0),
            num_prec_radix: Some(10),
            sql_data_type: Some(-5),
            sql_datetime_sub: Some(0),
            char_octet_length: Some(8),
            ordinal_position: Some(1),
            nullable: Some(0),
            generated_column: Some(false),
            extent: "8".to_owned(),
            charset: "UTF-8".to_owned(),
            collation: "en_US".to_owned(),
            unit: "bytes".to_owned(),
            sparse: Some(false),
            default_constraint_name: "df_items_id".to_owned(),
            seed: Some(1),
            increment: Some(2),
            on_update_current_timestamp: Some(false),
        }
    }

    fn index_fixture() -> CommunityTableIndex {
        CommunityTableIndex {
            database_name: "inventory".to_owned(),
            schema_name: "APP".to_owned(),
            table_name: "items".to_owned(),
            name: "idx_items_label".to_owned(),
            index_type: "BTREE".to_owned(),
            unique: Some(true),
            comment: "Unique item label".to_owned(),
            columns: vec![CommunityTableIndexColumn {
                database_name: "inventory".to_owned(),
                schema_name: "APP".to_owned(),
                table_name: "items".to_owned(),
                index_name: "idx_items_label".to_owned(),
                column_name: "label".to_owned(),
                column_type: "VARCHAR".to_owned(),
                comment: "Indexed label".to_owned(),
                ordinal_position: Some(1),
                collation: "A".to_owned(),
                non_unique: Some(false),
                index_qualifier: "inventory".to_owned(),
                sort_order: "ASC".to_owned(),
                cardinality: Some("9007199254740995".to_owned()),
                pages: Some("9007199254740996".to_owned()),
                filter_condition: "label IS NOT NULL".to_owned(),
                sub_part: Some("9007199254740997".to_owned()),
            }],
            concurrently: Some(false),
            method: "BTREE".to_owned(),
            foreign_schema_name: "PUBLIC".to_owned(),
            foreign_table_name: "labels".to_owned(),
            foreign_column_names: vec!["id".to_owned()],
        }
    }
}
