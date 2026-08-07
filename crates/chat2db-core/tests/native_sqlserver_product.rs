use std::{panic::AssertUnwindSafe, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    BuildCommunityCreateSchemaRequest, BuildCommunityDmlRequest, BuildCommunityNamespaceSqlRequest,
    CommunityDmlAssignment, CommunityDmlColumn, CommunityDmlRow, CommunityDmlStatement,
    CommunityDmlTarget, CommunityDmlTemporalKind, CommunityDmlValue,
    CommunityNamespaceSqlOperation, CommunitySchema, ComponentState, CreateDatasourceRequest,
    DatasourceConnection, DatasourceConnectionProperty, GetCommunityFunctionRequest,
    GetCommunityProcedureRequest, GetCommunityTriggerRequest, JdbcValue,
    ListCommunityColumnsRequest, ListCommunityDatabasesRequest, ListCommunityFunctionsRequest,
    ListCommunityIndexesRequest, ListCommunityProceduresRequest, ListCommunitySchemasRequest,
    ListCommunityTableKeysRequest, ListCommunityTablesRequest, ListCommunityTriggersRequest,
    ListCommunityViewsRequest, OperationEvent, QueryLimits, QueryParameter, ResultMetadata,
    ResultPageRequest, StartCommunityTablePreviewRequest, StartQueryRequest,
};
use chat2db_core::{
    Application, NativeConsoleCancellation, NativeConsoleRequest, RuntimeConfig, RuntimeHost,
};
use chat2db_java_bridge::{EngineCommand, EngineConfig};
use futures_util::{FutureExt as _, TryStreamExt as _};
use tempfile::TempDir;
use tiberius::{AuthMethod, Client, Config, EncryptionLevel};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt as _};
use uuid::Uuid;

const DATABASE_TYPE: &str = "SQLSERVER";
const EVENT_TIMEOUT: Duration = Duration::from_secs(20);

type DirectClient = Client<Compat<TcpStream>>;

struct SqlServerTestConfig {
    host: String,
    port: u16,
    user: String,
    password: String,
}

impl SqlServerTestConfig {
    fn from_environment() -> Self {
        let host = required_env("SQLSERVER_TEST_HOST");
        assert!(
            !host.trim().is_empty(),
            "SQLSERVER_TEST_HOST cannot be empty"
        );
        let port = required_env("SQLSERVER_TEST_PORT")
            .parse::<u16>()
            .expect("SQLSERVER_TEST_PORT must be a TCP port");
        assert_ne!(port, 0, "SQLSERVER_TEST_PORT cannot be zero");
        Self {
            host,
            port,
            user: required_env("SQLSERVER_TEST_USER"),
            password: required_env("SQLSERVER_TEST_PASSWORD"),
        }
    }

    fn connection(&self) -> DatasourceConnection {
        let host = if self.host.contains(':')
            && !(self.host.starts_with('[') && self.host.ends_with(']'))
        {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        DatasourceConnection {
            jdbc_url: format!(
                "jdbc:sqlserver://{host}:{};databaseName=master;encrypt=false;trustServerCertificate=true",
                self.port
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

    async fn direct_client(&self) -> Result<DirectClient, String> {
        let mut config = Config::new();
        config.host(&self.host);
        config.port(self.port);
        config.database("master");
        config.authentication(AuthMethod::sql_server(&self.user, &self.password));
        config.encryption(EncryptionLevel::NotSupported);
        config.trust_cert();
        let address = config.get_addr();
        let tcp = TcpStream::connect(address)
            .await
            .map_err(|error| error.to_string())?;
        tcp.set_nodelay(true).map_err(|error| error.to_string())?;
        Client::connect(config, tcp.compat_write())
            .await
            .map_err(|error| error.to_string())
    }
}

#[tokio::test]
#[ignore = "requires SQLSERVER_TEST_HOST, SQLSERVER_TEST_PORT, SQLSERVER_TEST_USER, and SQLSERVER_TEST_PASSWORD"]
async fn native_sqlserver_product_paths_keep_java_dormant() {
    let config = SqlServerTestConfig::from_environment();
    let database_name = format!("chat2db_native_it_{}", Uuid::new_v4().simple());
    provision_database(&config, &database_name).await;

    let verification = AssertUnwindSafe(verify_native_product(&config, &database_name))
        .catch_unwind()
        .await;
    let cleanup = cleanup_database(&config, &database_name).await;
    if let Err(payload) = verification {
        if let Err(error) = cleanup {
            eprintln!("native SQL Server cleanup also failed: {error}");
        }
        std::panic::resume_unwind(payload);
    }
    cleanup.expect("native SQL Server fixture database must be removed");
}

async fn verify_native_product(config: &SqlServerTestConfig, database_name: &str) {
    let directory = TempDir::new().expect("temporary native SQL Server runtime");
    let missing_java = directory.path().join("missing-java");
    let runtime = RuntimeConfig::new(EngineConfig::new(EngineCommand::new(missing_java)))
        .with_data_dir(directory.path().join("data"))
        .with_vault_master_key_base64(STANDARD.encode([0x73; 32]));
    let mut host = RuntimeHost::open(runtime)
        .await
        .expect("native SQL Server runtime must open without Java");
    let application = host.application();
    assert_java_dormant(&application);

    let driver = application
        .list_drivers()
        .items
        .into_iter()
        .find(|driver| driver.driver_id == "sqlserver")
        .expect("native SQL Server driver must be present");
    assert_eq!(driver.driver_class, "rust:tiberius");
    assert_eq!(driver.artifact_count, 0);
    application
        .test_datasource_connection("sqlserver", config.connection())
        .await
        .expect("native SQL Server connection test must succeed");
    let mut tls_conflict = config.connection();
    tls_conflict.properties.push(DatasourceConnectionProperty {
        key: "trustServerCertificateCA".to_owned(),
        value: "/tmp/sqlserver-ca.pem".to_owned(),
        sensitive: false,
    });
    let conflict = application
        .test_datasource_connection("sqlserver", tls_conflict)
        .await
        .expect_err("conflicting SQL Server trust settings must fail without panicking");
    assert_eq!(conflict.api_error().code, "invalid_sqlserver_connection");
    assert_java_dormant(&application);

    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Native SQL Server".to_owned(),
            driver_id: "sqlserver".to_owned(),
            connection: Some(config.connection()),
        })
        .await
        .expect("native SQL Server datasource must persist");

    verify_native_dialect_builders(&application, &datasource.id, database_name).await;
    verify_database_and_table_metadata(&application, &datasource.id, database_name).await;
    verify_object_metadata(&application, &datasource.id, database_name).await;
    verify_console_query_and_preview(&application, &datasource.id, database_name).await;
    verify_console_preflight_compatibility(&application, &datasource.id, database_name).await;
    verify_fail_closed_query_safety(&application, &datasource.id, database_name).await;
    verify_cancellation_and_recovery(&application, &datasource.id, database_name).await;
    assert_java_dormant(&application);
    host.shutdown()
        .await
        .expect("native-only SQL Server runtime must shut down cleanly");
}

async fn verify_native_dialect_builders(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let schema = application
        .build_community_create_schema(BuildCommunityCreateSchemaRequest {
            database_type: DATABASE_TYPE.to_owned(),
            schema: CommunitySchema {
                database_name: database_name.to_owned(),
                name: "native_builder".to_owned(),
                comment: "native builder's schema".to_owned(),
                owner: "dbo".to_owned(),
                system: false,
            },
        })
        .await
        .expect("native SQL Server CREATE SCHEMA must build without Java");
    assert!(schema.sql.contains("CREATE SCHEMA [native_builder]"));
    execute_product_console(application, datasource_id, database_name, &schema.sql).await;

    let use_database = application
        .build_community_namespace_sql(BuildCommunityNamespaceSqlRequest {
            database_type: DATABASE_TYPE.to_owned(),
            operation: CommunityNamespaceSqlOperation::UseDatabase {
                database_name: database_name.to_owned(),
            },
        })
        .await
        .expect("native SQL Server USE must build without Java");
    assert_eq!(use_database.sql, format!("USE [{database_name}];"));
    let rename = application
        .build_community_namespace_sql(BuildCommunityNamespaceSqlRequest {
            database_type: DATABASE_TYPE.to_owned(),
            operation: CommunityNamespaceSqlOperation::AlterSchema {
                old_schema_name: "native_builder".to_owned(),
                new_schema_name: "native_builder_renamed".to_owned(),
            },
        })
        .await
        .expect_err("SQL Server schema rename must fail explicitly without Java fallback");
    assert_eq!(
        rename.api_error().code,
        "sqlserver_schema_rename_unsupported"
    );

    execute_product_console(
        application,
        datasource_id,
        database_name,
        "CREATE TABLE native_builder.items (id int NOT NULL PRIMARY KEY, label nvarchar(80) NOT NULL, amount decimal(12,2) NOT NULL, active bit NOT NULL, created_at datetimeoffset NOT NULL, payload varbinary(16) NOT NULL)",
    )
    .await;
    verify_native_dml_builders(application, datasource_id, database_name).await;
    assert_java_dormant(application);
}

async fn verify_native_dml_builders(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let columns = vec![
        dml_column("id", "int"),
        dml_column("label", "nvarchar"),
        dml_column("amount", "decimal"),
        dml_column("active", "bit"),
        dml_column("created_at", "datetimeoffset"),
        dml_column("payload", "varbinary"),
    ];
    let insert = application
        .build_community_dml(BuildCommunityDmlRequest {
            database_type: DATABASE_TYPE.to_owned(),
            target: dml_target(database_name),
            statement: CommunityDmlStatement::SingleInsert {
                columns,
                row: CommunityDmlRow {
                    values: vec![
                        CommunityDmlValue::Decimal {
                            value: "1".to_owned(),
                        },
                        CommunityDmlValue::String {
                            value: "O'Brien".to_owned(),
                        },
                        CommunityDmlValue::Decimal {
                            value: "12.50".to_owned(),
                        },
                        CommunityDmlValue::Boolean { value: true },
                        CommunityDmlValue::Temporal {
                            temporal_kind: CommunityDmlTemporalKind::OffsetDatetime,
                            value: "2026-08-07T12:30:45.1234567+08:00".to_owned(),
                        },
                        CommunityDmlValue::Binary {
                            base64: STANDARD.encode([0, 255]),
                        },
                    ],
                },
            },
        })
        .await
        .expect("native SQL Server INSERT must build without Java");
    assert!(insert.sql.contains("N'O''Brien'"));
    assert!(insert.sql.contains("0x00ff"));
    execute_product_console(application, datasource_id, database_name, &insert.sql).await;

    let update = application
        .build_community_dml(BuildCommunityDmlRequest {
            database_type: DATABASE_TYPE.to_owned(),
            target: dml_target(database_name),
            statement: CommunityDmlStatement::Update {
                assignments: vec![CommunityDmlAssignment {
                    column: dml_column("label", "nvarchar"),
                    value: CommunityDmlValue::String {
                        value: "updated".to_owned(),
                    },
                }],
                predicates: vec![CommunityDmlAssignment {
                    column: dml_column("id", "int"),
                    value: CommunityDmlValue::Decimal {
                        value: "1".to_owned(),
                    },
                }],
            },
        })
        .await
        .expect("native SQL Server UPDATE must build without Java");
    execute_product_console(application, datasource_id, database_name, &update.sql).await;
    let selected = application
        .execute_native_console(
            NativeConsoleRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                sql: "SELECT label FROM native_builder.items WHERE id = 1".to_owned(),
                page_no: 1,
                page_size: 20,
                result_set_id: None,
                single: true,
                page_size_all: false,
                explain: false,
                error_continue: false,
            },
            NativeConsoleCancellation::new(),
        )
        .await
        .expect("native SQL Server builder result must be queryable");
    assert!(matches!(
        selected
            .iter()
            .find_map(|result| result.rows.first())
            .and_then(|row| row.values.first()),
        Some(JdbcValue::Text { value }) if value == "updated"
    ));
}

async fn execute_product_console(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
    sql: &str,
) {
    let results = application
        .execute_native_console(
            NativeConsoleRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                sql: sql.to_owned(),
                page_no: 1,
                page_size: 20,
                result_set_id: None,
                single: false,
                page_size_all: false,
                explain: false,
                error_continue: false,
            },
            NativeConsoleCancellation::new(),
        )
        .await
        .unwrap_or_else(|error| panic!("native SQL Server Console failed for {sql}: {error}"));
    assert!(
        results.iter().all(|result| result.success),
        "native SQL Server Console returned failed results for {sql}: {results:#?}"
    );
}

fn dml_target(database_name: &str) -> CommunityDmlTarget {
    CommunityDmlTarget {
        database_name: Some(database_name.to_owned()),
        schema_name: Some("native_builder".to_owned()),
        table_name: "items".to_owned(),
    }
}

fn dml_column(name: &str, data_type_name: &str) -> CommunityDmlColumn {
    CommunityDmlColumn {
        name: name.to_owned(),
        data_type_name: data_type_name.to_owned(),
        precision: None,
        scale: None,
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the product smoke checks every SQL Server table metadata projection together"
)]
async fn verify_database_and_table_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let databases = application
        .list_community_databases(ListCommunityDatabasesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: DATABASE_TYPE.to_owned(),
        })
        .await
        .expect("native SQL Server databases must list");
    assert!(
        databases
            .items
            .iter()
            .any(|item| item.name == database_name)
    );
    assert!(
        databases
            .items
            .iter()
            .find(|item| item.name == "master")
            .is_some_and(|item| item.system)
    );

    let schemas = application
        .list_community_schemas(ListCommunitySchemasRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
        })
        .await
        .expect("native SQL Server schemas must list");
    assert!(schemas.items.iter().any(|item| item.name == "dbo"));

    let tables = application
        .list_community_tables(ListCommunityTablesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: "dbo".to_owned(),
            table_name_pattern: String::new(),
        })
        .await
        .expect("native SQL Server tables must list");
    assert!(tables.items.iter().any(|item| item.name == "native_parent"));
    assert!(tables.items.iter().any(|item| item.name == "native_child"));
    assert!(tables.items.iter().all(|item| item.name != "native_view"));

    let columns = application
        .list_community_columns(ListCommunityColumnsRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: "dbo".to_owned(),
            table_name: "native_child".to_owned(),
        })
        .await
        .expect("native SQL Server columns must list");
    let identity = columns
        .items
        .iter()
        .find(|column| column.name == "id")
        .expect("identity column must exist");
    assert_eq!(identity.auto_increment, Some(true));
    assert_eq!(identity.seed, Some(10));
    assert_eq!(identity.increment, Some(5));
    assert!(
        columns
            .items
            .iter()
            .find(|column| column.name == "note_lower")
            .is_some_and(|column| column.generated_column == Some(true))
    );

    let indexes = application
        .list_community_indexes(ListCommunityIndexesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: "dbo".to_owned(),
            table_name: "native_child".to_owned(),
        })
        .await
        .expect("native SQL Server indexes must list");
    assert!(
        indexes
            .items
            .iter()
            .any(|index| index.name == "PK_native_child")
    );
    let filtered = indexes
        .items
        .iter()
        .find(|index| index.name == "IX_native_child_note")
        .expect("filtered index must exist");
    assert!(
        filtered
            .columns
            .iter()
            .any(|column| column.column_name == "note")
    );
    assert!(
        filtered
            .columns
            .iter()
            .any(|column| column.column_name == "parent_id" && column.column_type == "INCLUDED")
    );

    let child_keys = table_keys_request(datasource_id, database_name, "native_child");
    let imported = application
        .list_community_imported_keys(child_keys.clone())
        .await
        .expect("native SQL Server imported keys must list");
    assert!(imported.items.iter().any(|key| {
        key.foreign_key_name == "FK_native_child_parent"
            && key.primary_table_name == "native_parent"
            && key.foreign_table_name == "native_child"
    }));
    let primary = application
        .list_community_primary_keys(child_keys)
        .await
        .expect("native SQL Server primary keys must list");
    assert!(
        primary
            .items
            .iter()
            .any(|key| key.name == "PK_native_child" && key.column_name == "id")
    );
    let exported = application
        .list_community_exported_keys(table_keys_request(
            datasource_id,
            database_name,
            "native_parent",
        ))
        .await
        .expect("native SQL Server exported keys must list");
    assert!(
        exported
            .items
            .iter()
            .any(|key| key.foreign_key_name == "FK_native_child_parent")
    );

    let ddl = application
        .table_ddl(datasource_id, database_name, "dbo", "native_child")
        .await
        .expect("native SQL Server table DDL must render");
    assert!(ddl.starts_with(&format!(
        "CREATE TABLE [{database_name}].[dbo].[native_child]"
    )));
    assert!(ddl.contains("IDENTITY(10,5)"));
    assert!(ddl.contains("CONSTRAINT [FK_native_child_parent] FOREIGN KEY"));
    assert!(ddl.contains("CREATE NONCLUSTERED INDEX [IX_native_child_note]"));
    let invalid = application
        .table_ddl(datasource_id, database_name, "dbo", "")
        .await
        .expect_err("empty SQL Server table names must be rejected");
    assert_eq!(
        invalid.api_error().code,
        "invalid_sqlserver_metadata_request"
    );
    assert_java_dormant(application);
}

#[allow(
    clippy::too_many_lines,
    reason = "the product smoke checks every SQL Server programmable-object projection together"
)]
async fn verify_object_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let views_request = ListCommunityViewsRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: DATABASE_TYPE.to_owned(),
        database_name: database_name.to_owned(),
        schema_name: "dbo".to_owned(),
        view_name_pattern: "native_view".to_owned(),
    };
    let views = application
        .list_community_views(views_request.clone())
        .await
        .expect("native SQL Server views must list");
    assert!(views.items.iter().any(|view| view.name == "native_view"));
    let view = application
        .get_community_view(views_request)
        .await
        .expect("native SQL Server view detail must load");
    assert!(view.ddl.contains("native_view"));

    let functions = application
        .list_community_functions(ListCommunityFunctionsRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: "dbo".to_owned(),
        })
        .await
        .expect("native SQL Server functions must list");
    assert!(
        functions
            .items
            .iter()
            .any(|item| item.name == "native_function")
    );
    let function_request = GetCommunityFunctionRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: DATABASE_TYPE.to_owned(),
        database_name: database_name.to_owned(),
        schema_name: "dbo".to_owned(),
        function_name: "native_function".to_owned(),
    };
    let function = application
        .get_community_function(function_request.clone())
        .await
        .expect("native SQL Server function detail must load");
    assert!(function.body.contains("native_function"));
    let function_parameters = application
        .list_community_function_parameters(function_request)
        .await
        .expect("native SQL Server function parameters must list");
    assert!(
        function_parameters
            .items
            .iter()
            .any(|parameter| parameter.column_name == "@value")
    );

    let procedures = application
        .list_community_procedures(ListCommunityProceduresRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: "dbo".to_owned(),
        })
        .await
        .expect("native SQL Server procedures must list");
    assert!(
        procedures
            .items
            .iter()
            .any(|item| item.name == "native_procedure")
    );
    let procedure_request = GetCommunityProcedureRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: DATABASE_TYPE.to_owned(),
        database_name: database_name.to_owned(),
        schema_name: "dbo".to_owned(),
        procedure_name: "native_procedure".to_owned(),
    };
    let procedure = application
        .get_community_procedure(procedure_request.clone())
        .await
        .expect("native SQL Server procedure detail must load");
    assert!(procedure.body.contains("native_procedure"));
    let procedure_parameters = application
        .list_community_procedure_parameters(procedure_request)
        .await
        .expect("native SQL Server procedure parameters must list");
    assert!(
        procedure_parameters
            .items
            .iter()
            .any(|parameter| parameter.column_name == "@value")
    );

    let triggers = application
        .list_community_triggers(ListCommunityTriggersRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: "dbo".to_owned(),
        })
        .await
        .expect("native SQL Server triggers must list");
    assert!(
        triggers
            .items
            .iter()
            .any(|item| item.name == "native_trigger")
    );
    let trigger = application
        .get_community_trigger(GetCommunityTriggerRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: "dbo".to_owned(),
            trigger_name: "native_trigger".to_owned(),
        })
        .await
        .expect("native SQL Server trigger detail must load");
    assert!(trigger.event_manipulation.contains("INSERT"));
    assert!(trigger.body.contains("native_trigger"));
    assert_java_dormant(application);
}

async fn verify_console_query_and_preview(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    verify_datetimeoffset_projection(application, datasource_id, database_name).await;

    let console = application
        .execute_native_console(
            NativeConsoleRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                sql: "SELECT parent_id, note FROM dbo.native_child ORDER BY id".to_owned(),
                page_no: 1,
                page_size: 20,
                result_set_id: None,
                single: true,
                page_size_all: false,
                explain: false,
                error_continue: false,
            },
            NativeConsoleCancellation::new(),
        )
        .await
        .expect("native SQL Server Console SELECT must execute");
    let tabular = console
        .iter()
        .find(|result| !result.columns.is_empty())
        .expect("Console must return one tabular result");
    assert!(tabular.success);
    assert_eq!(tabular.rows.len(), 2);
    assert!(matches!(
        tabular.rows[0].values.as_slice(),
        [JdbcValue::SignedInteger { value: parent_id }, JdbcValue::Text { value: note }]
            if parent_id == "7" && note == "alpha"
    ));

    let query = application
        .start_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: format!(
                "SELECT parent_id, note FROM [{database_name}].[dbo].[native_child] WHERE parent_id = ? ORDER BY id"
            ),
            parameters: vec![QueryParameter {
                position: 1,
                value: JdbcValue::SignedInteger {
                    value: "7".to_owned(),
                },
            }],
            limits: query_limits("10"),
        })
        .await
        .expect("native SQL Server retained query must be accepted");
    let query_result = wait_for_result(application, &query.operation_id).await;
    assert_eq!(query_result.row_count, "2");
    assert_eq!(result_page(application, &query_result).await.rows.len(), 2);

    let preview = application
        .start_community_table_preview(StartCommunityTablePreviewRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: DATABASE_TYPE.to_owned(),
            database_name: database_name.to_owned(),
            schema_name: "dbo".to_owned(),
            table_name: "native_child".to_owned(),
            row_limit: Some(1),
        })
        .await
        .expect("native SQL Server table preview must be accepted");
    assert_eq!(
        preview.sql,
        format!("SELECT TOP (1) * FROM [{database_name}].[dbo].[native_child]")
    );
    let preview_result = wait_for_result(application, &preview.operation_id).await;
    assert_eq!(preview_result.row_count, "1");
    assert_eq!(
        result_page(application, &preview_result).await.rows.len(),
        1
    );
    assert_java_dormant(application);
}

async fn verify_datetimeoffset_projection(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let datetime_offset = application
        .execute_native_console(
            NativeConsoleRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                sql: "SELECT CAST('2026-08-07T12:34:56.123456+08:00' AS datetimeoffset(6)) AS exact_offset".to_owned(),
                page_no: 1,
                page_size: 20,
                result_set_id: None,
                single: true,
                page_size_all: false,
                explain: false,
                error_continue: false,
            },
            NativeConsoleCancellation::new(),
        )
        .await
        .expect("native SQL Server datetimeoffset SELECT must execute");
    let value = datetime_offset
        .iter()
        .find_map(|result| result.rows.first())
        .and_then(|row| row.values.first())
        .expect("datetimeoffset SELECT must return one value");
    assert_eq!(
        value,
        &JdbcValue::TimestampWithTimeZone {
            value: "2026-08-07T12:34:56.123456+08:00".to_owned(),
        }
    );
}

async fn verify_console_preflight_compatibility(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    verify_temporary_table_preflight(application, datasource_id, database_name).await;
    verify_select_into_preflight(application, datasource_id, database_name).await;
    verify_limited_reader_preflight(application, datasource_id, database_name).await;
    assert_java_dormant(application);
}

async fn verify_temporary_table_preflight(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let temporary_table = application
        .execute_native_console(
            NativeConsoleRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                sql: "CREATE TABLE #chat2db_native_temp(id int NOT NULL);\n\
                      INSERT INTO #chat2db_native_temp(id) VALUES (1), (2);\n\
                      WITH target AS (SELECT id FROM #chat2db_native_temp WHERE id = 2)\n\
                      DELETE FROM target OUTPUT deleted.id;\n\
                      SELECT id FROM #chat2db_native_temp ORDER BY id;\n\
                      DROP TABLE #chat2db_native_temp"
                    .to_owned(),
                page_no: 1,
                page_size: 20,
                result_set_id: None,
                single: false,
                page_size_all: false,
                explain: false,
                error_continue: false,
            },
            NativeConsoleCancellation::new(),
        )
        .await
        .expect("temporary-table and CTE DML Console statements must execute");
    assert!(
        temporary_table.iter().all(|result| result.success),
        "temporary-table Console statements failed: {temporary_table:#?}"
    );
    let returned_values = temporary_table
        .iter()
        .filter_map(|result| result.rows.first())
        .filter_map(|row| row.values.first())
        .filter_map(|value| match value {
            JdbcValue::SignedInteger { value } => Some(value.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(returned_values, ["2", "1"]);
}

async fn verify_select_into_preflight(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let select_into = application
        .execute_native_console(
            NativeConsoleRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                sql: "SELECT id INTO dbo.native_select_into FROM dbo.native_child;\n\
                      SELECT COUNT_BIG(*) AS copied FROM dbo.native_select_into;\n\
                      DROP TABLE dbo.native_select_into"
                    .to_owned(),
                page_no: 1,
                page_size: 20,
                result_set_id: None,
                single: false,
                page_size_all: false,
                explain: false,
                error_continue: false,
            },
            NativeConsoleCancellation::new(),
        )
        .await
        .expect("SELECT INTO must execute as a Console write");
    assert!(
        select_into.iter().all(|result| result.success),
        "SELECT INTO Console statements failed: {select_into:#?}"
    );
    assert!(select_into.iter().any(|result| {
        matches!(
            result.rows.first().map(|row| row.values.as_slice()),
            Some([JdbcValue::SignedInteger { value }]) if value == "2"
        )
    }));
}

async fn verify_limited_reader_preflight(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let limited_reader = application
        .execute_native_console(
            NativeConsoleRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                sql: "CREATE USER chat2db_native_reader WITHOUT LOGIN;\n\
                      GRANT SELECT ON dbo.native_child TO chat2db_native_reader;\n\
                      EXECUTE AS USER = 'chat2db_native_reader';\n\
                      SELECT COUNT_BIG(*) AS visible_rows FROM dbo.native_child;\n\
                      REVERT;\n\
                      DROP USER chat2db_native_reader"
                    .to_owned(),
                page_no: 1,
                page_size: 20,
                result_set_id: None,
                single: false,
                page_size_all: false,
                explain: false,
                error_continue: false,
            },
            NativeConsoleCancellation::new(),
        )
        .await
        .expect("a SELECT-only database user must pass safe result preflight");
    assert!(
        limited_reader.iter().all(|result| result.success),
        "limited-reader Console statements failed: {limited_reader:#?}"
    );
    assert!(limited_reader.iter().any(|result| {
        matches!(
            result.rows.first().map(|row| row.values.as_slice()),
            Some([JdbcValue::SignedInteger { value }]) if value == "2"
        )
    }));
}

async fn verify_cancellation_and_recovery(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let cancellation = NativeConsoleCancellation::new();
    let task_application = application.clone();
    let task_cancellation = cancellation.clone();
    let request = NativeConsoleRequest {
        datasource_id: datasource_id.to_owned(),
        database_name: database_name.to_owned(),
        sql: "SELECT SUM(CONVERT(bigint, a.object_id % 2)) AS total FROM sys.all_objects AS a CROSS JOIN sys.all_objects AS b CROSS JOIN sys.all_objects AS c CROSS JOIN sys.all_objects AS d".to_owned(),
        page_no: 1,
        page_size: 20,
        result_set_id: None,
        single: true,
        page_size_all: false,
        explain: false,
        error_continue: false,
    };
    let task = tokio::spawn(async move {
        task_application
            .execute_native_console(request, task_cancellation)
            .await
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(cancellation.cancel(Some("SQL Server product smoke".to_owned())));
    let error = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("cancelled SQL Server Console must stop before timeout")
        .expect("cancelled SQL Server Console task must join")
        .expect_err("cancelled SQL Server Console must return an error");
    assert_eq!(error.api_error().code, "sqlserver_console_cancelled");

    let write_cancellation = NativeConsoleCancellation::new();
    let write_application = application.clone();
    let task_cancellation = write_cancellation.clone();
    let write_request = NativeConsoleRequest {
        datasource_id: datasource_id.to_owned(),
        database_name: database_name.to_owned(),
        sql: "WAITFOR DELAY '00:00:03'; INSERT INTO dbo.native_parent(id,label) VALUES (99,N'write-cancelled')".to_owned(),
        page_no: 1,
        page_size: 20,
        result_set_id: None,
        single: true,
        page_size_all: false,
        explain: false,
        error_continue: false,
    };
    let write_task = tokio::spawn(async move {
        write_application
            .execute_native_console(write_request, task_cancellation)
            .await
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(write_cancellation.cancel(Some("SQL Server write cancellation".to_owned())));
    let write_error = tokio::time::timeout(Duration::from_secs(10), write_task)
        .await
        .expect("cancelled SQL Server Console write must stop before timeout")
        .expect("cancelled SQL Server Console write task must join")
        .expect_err("a dispatched Console write cancellation must be outcome-unknown");
    assert_eq!(
        write_error.api_error().code,
        "database_write_outcome_unknown"
    );
    verify_authoritative_server_rejections(application, datasource_id, database_name).await;

    let recovered = application
        .execute_native_console(
            NativeConsoleRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                sql: "SELECT 1 AS recovered".to_owned(),
                page_no: 1,
                page_size: 20,
                result_set_id: None,
                single: true,
                page_size_all: false,
                explain: false,
                error_continue: false,
            },
            NativeConsoleCancellation::new(),
        )
        .await
        .expect("SQL Server Console must recover after session cancellation");
    assert!(
        recovered
            .iter()
            .any(|result| result.success && result.row_count == 1)
    );
    assert_java_dormant(application);
}

async fn verify_authoritative_server_rejections(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    for sql in [
        "INSERT INTO dbo.native_parent(id,label) VALUES (7,N'duplicate')",
        "WITH duplicate AS (SELECT 7 AS id, N'duplicate' AS label) \
         INSERT INTO dbo.native_parent(id,label) OUTPUT inserted.id \
         SELECT id,label FROM duplicate",
    ] {
        let results = application
            .execute_native_console(
                NativeConsoleRequest {
                    datasource_id: datasource_id.to_owned(),
                    database_name: database_name.to_owned(),
                    sql: sql.to_owned(),
                    page_no: 1,
                    page_size: 20,
                    result_set_id: None,
                    single: true,
                    page_size_all: false,
                    explain: false,
                    error_continue: false,
                },
                NativeConsoleCancellation::new(),
            )
            .await
            .expect("an authoritative SQL Server rejection must remain a statement result");
        let error = results
            .first()
            .and_then(|result| result.error.as_ref())
            .expect("the rejected SQL Server write must expose its server error");
        assert_eq!(error.code, "sqlserver_query_failed");
    }
}

async fn verify_fail_closed_query_safety(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    verify_retained_query_fail_closed(application, datasource_id, database_name).await;
    verify_console_results_fail_closed(application, datasource_id, database_name).await;

    let rows = application
        .execute_native_console(
            NativeConsoleRequest {
                datasource_id: datasource_id.to_owned(),
                database_name: database_name.to_owned(),
                sql: "SELECT COUNT_BIG(*) AS row_count FROM dbo.native_child".to_owned(),
                page_no: 1,
                page_size: 20,
                result_set_id: None,
                single: true,
                page_size_all: false,
                explain: false,
                error_continue: false,
            },
            NativeConsoleCancellation::new(),
        )
        .await
        .expect("SQL Server fixture row count must remain readable");
    assert!(matches!(
        rows.first()
            .and_then(|result| result.rows.first())
            .map(|row| row.values.as_slice()),
        Some([JdbcValue::SignedInteger { value }]) if value == "2"
    ));
    assert_java_dormant(application);
}

async fn verify_retained_query_fail_closed(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let cte_write = application
        .start_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: format!(
                "WITH target AS (SELECT id FROM [{database_name}].[dbo].[native_child]) DELETE FROM target WHERE id = 10"
            ),
            parameters: Vec::new(),
            limits: query_limits("10"),
        })
        .await
        .expect_err("a data-changing CTE must be rejected by the native read path");
    assert_eq!(
        cte_write.api_error().code,
        "sqlserver_native_query_unsupported"
    );

    let unsafe_query = application
        .start_query(StartQueryRequest {
            datasource_id: datasource_id.to_owned(),
            sql: "SELECT CONVERT(sql_variant, 7) AS unsafe_variant".to_owned(),
            parameters: Vec::new(),
            limits: query_limits("10"),
        })
        .await
        .expect("unsafe SQL Server result query must be accepted as an operation");
    assert_eq!(
        wait_for_failure_code(application, &unsafe_query.operation_id).await,
        "sqlserver_result_type_unsupported"
    );
}

async fn verify_console_results_fail_closed(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    for (sql, expected_codes) in [
        (
            "SELECT CONVERT(money, 1.23) AS unsafe_money",
            &["sqlserver_result_type_unsupported"][..],
        ),
        (
            "SELECT CONVERT(sql_variant, 7) AS unsafe_variant",
            &["sqlserver_result_type_unsupported"][..],
        ),
        (
            "SELECT hierarchyid::Parse('/1/') AS unsafe_udt",
            &[
                "sqlserver_result_type_unsupported",
                "sqlserver_result_description_failed",
            ][..],
        ),
        (
            "SELECT REPLICATE(CONVERT(varchar(max), 'x'), 4194305) AS oversized_value",
            &["sqlserver_scalar_too_large"][..],
        ),
    ] {
        let results = application
            .execute_native_console(
                NativeConsoleRequest {
                    datasource_id: datasource_id.to_owned(),
                    database_name: database_name.to_owned(),
                    sql: sql.to_owned(),
                    page_no: 1,
                    page_size: 20,
                    result_set_id: None,
                    single: true,
                    page_size_all: false,
                    explain: false,
                    error_continue: false,
                },
                NativeConsoleCancellation::new(),
            )
            .await
            .expect("fail-closed SQL Server Console checks must terminate normally");
        let error = results
            .first()
            .and_then(|result| result.error.as_ref())
            .expect("unsafe SQL Server result must return a statement error");
        assert!(
            expected_codes.contains(&error.code.as_str()),
            "unexpected error {} for SQL: {sql}",
            error.code
        );
    }
}

fn table_keys_request(
    datasource_id: &str,
    database_name: &str,
    table_name: &str,
) -> ListCommunityTableKeysRequest {
    ListCommunityTableKeysRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: DATABASE_TYPE.to_owned(),
        database_name: database_name.to_owned(),
        schema_name: "dbo".to_owned(),
        table_name: table_name.to_owned(),
    }
}

fn query_limits(max_rows: &str) -> QueryLimits {
    QueryLimits {
        max_rows: max_rows.to_owned(),
        max_result_bytes: (8_u64 * 1024 * 1024).to_string(),
        batch_rows: 2,
        batch_bytes: 1024 * 1024,
        result_ttl_seconds: 60,
    }
}

async fn wait_for_result(application: &Application, operation_id: &str) -> ResultMetadata {
    let mut subscription = application
        .subscribe_operation(operation_id, None)
        .await
        .expect("SQL Server query operation must be subscribable");
    tokio::time::timeout(EVENT_TIMEOUT, async {
        while let Some(envelope) = subscription
            .next_event()
            .await
            .expect("SQL Server operation event must decode")
        {
            match envelope.event {
                OperationEvent::Completed { result } => return result,
                OperationEvent::Failed { error } => {
                    panic!("native SQL Server query failed: {error:?}")
                }
                OperationEvent::Cancelled { reason } => {
                    panic!("native SQL Server query was cancelled: {reason:?}")
                }
                OperationEvent::Started | OperationEvent::Progress { .. } => {}
            }
        }
        panic!("native SQL Server operation ended without a terminal event")
    })
    .await
    .expect("native SQL Server query must finish before timeout")
}

async fn wait_for_failure_code(application: &Application, operation_id: &str) -> String {
    let mut subscription = application
        .subscribe_operation(operation_id, None)
        .await
        .expect("SQL Server query operation must be subscribable");
    tokio::time::timeout(EVENT_TIMEOUT, async {
        while let Some(envelope) = subscription
            .next_event()
            .await
            .expect("SQL Server operation event must decode")
        {
            match envelope.event {
                OperationEvent::Failed { error } => return error.code,
                OperationEvent::Completed { result } => {
                    panic!("unsafe SQL Server query unexpectedly completed: {result:?}")
                }
                OperationEvent::Cancelled { reason } => {
                    panic!("unsafe SQL Server query was cancelled: {reason:?}")
                }
                OperationEvent::Started | OperationEvent::Progress { .. } => {}
            }
        }
        panic!("unsafe SQL Server query ended without a terminal event")
    })
    .await
    .expect("unsafe SQL Server query must fail before timeout")
}

async fn result_page(
    application: &Application,
    result: &ResultMetadata,
) -> chat2db_contract::ResultPage {
    application
        .result_page(
            &result.id,
            ResultPageRequest {
                offset: "0".to_owned(),
                max_rows: "20".to_owned(),
                max_bytes: (8_u64 * 1024 * 1024).to_string(),
            },
        )
        .await
        .expect("native SQL Server result page must be retained")
}

async fn provision_database(config: &SqlServerTestConfig, database_name: &str) {
    let mut client = config
        .direct_client()
        .await
        .expect("SQL Server fixture connection must open");
    execute_batch(&mut client, &format!("CREATE DATABASE [{database_name}]")).await;
    execute_batch(&mut client, &format!("USE [{database_name}]")).await;
    execute_batch(
        &mut client,
        "CREATE TABLE dbo.native_parent (id int NOT NULL, label nvarchar(80) NULL, CONSTRAINT PK_native_parent PRIMARY KEY (id))",
    )
    .await;
    execute_batch(
        &mut client,
        "CREATE TABLE dbo.native_child (id int IDENTITY(10,5) NOT NULL, parent_id int NOT NULL, note nvarchar(80) NULL, note_lower AS LOWER(note), CONSTRAINT PK_native_child PRIMARY KEY (id), CONSTRAINT FK_native_child_parent FOREIGN KEY (parent_id) REFERENCES dbo.native_parent(id) ON UPDATE CASCADE ON DELETE CASCADE)",
    )
    .await;
    execute_batch(
        &mut client,
        "CREATE INDEX IX_native_child_note ON dbo.native_child(note DESC) INCLUDE(parent_id) WHERE note IS NOT NULL",
    )
    .await;
    execute_batch(
        &mut client,
        "CREATE VIEW dbo.native_view AS SELECT id, label FROM dbo.native_parent",
    )
    .await;
    execute_batch(
        &mut client,
        "CREATE FUNCTION dbo.native_function(@value int) RETURNS int AS BEGIN RETURN @value + 1; END",
    )
    .await;
    execute_batch(
        &mut client,
        "CREATE PROCEDURE dbo.native_procedure @value int AS SELECT @value AS value",
    )
    .await;
    execute_batch(
        &mut client,
        "CREATE TRIGGER dbo.native_trigger ON dbo.native_child AFTER INSERT AS BEGIN SET NOCOUNT ON; END",
    )
    .await;
    execute_batch(
        &mut client,
        "INSERT INTO dbo.native_parent(id,label) VALUES (7,N'native-rust'); INSERT INTO dbo.native_child(parent_id,note) VALUES (7,N'alpha'),(7,N'beta')",
    )
    .await;
}

async fn cleanup_database(config: &SqlServerTestConfig, database_name: &str) -> Result<(), String> {
    let mut client = config.direct_client().await?;
    let sql = format!(
        "IF DB_ID(N'{database_name}') IS NOT NULL BEGIN ALTER DATABASE [{database_name}] SET SINGLE_USER WITH ROLLBACK IMMEDIATE; DROP DATABASE [{database_name}]; END"
    );
    execute_batch_result(&mut client, &sql).await
}

async fn execute_batch(client: &mut DirectClient, sql: &str) {
    execute_batch_result(client, sql)
        .await
        .unwrap_or_else(|error| panic!("SQL Server fixture statement failed: {error}; SQL: {sql}"));
}

async fn execute_batch_result(client: &mut DirectClient, sql: &str) -> Result<(), String> {
    let mut stream = client
        .simple_query(sql)
        .await
        .map_err(|error| error.to_string())?;
    while stream
        .try_next()
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {}
    Ok(())
}

fn assert_java_dormant(application: &Application) {
    let engine = application
        .health()
        .components
        .into_iter()
        .find(|component| component.id == "database-engine")
        .expect("database engine health must be present");
    assert_eq!(engine.state, ComponentState::Ready);
    assert_eq!(engine.detail, "Available on demand; Java is not running");
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be configured"))
}
