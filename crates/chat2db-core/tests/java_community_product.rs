use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use chat2db_contract::{
    BuildCommunityCreateSchemaRequest, ComponentState, CreateDatasourceRequest,
    DatasourceConnection, GetCommunityFunctionRequest, GetCommunityProcedureRequest,
    GetCommunityTriggerRequest, ListCommunityColumnsRequest, ListCommunityDatabasesRequest,
    ListCommunityFunctionsRequest, ListCommunityIndexesRequest, ListCommunityProceduresRequest,
    ListCommunitySchemasRequest, ListCommunityTableKeysRequest, ListCommunityTablesRequest,
    ListCommunityTriggersRequest, ListCommunityViewsRequest, ParseCommunitySqlRequest,
};
use chat2db_core::{
    AppError, AppErrorKind, Application, RuntimeHost, load_fixed_community_classpath,
};
use chat2db_java_bridge::{
    DriverArtifact, DriverClient, DriverSpec, EngineCommand, EngineConfig, EngineSupervisor,
    SessionConfig, UpdateRequest,
};
use chat2db_storage::{EncryptedFileVault, Storage};
use tempfile::TempDir;

const COMMUNITY_COMMIT: &str = "f63cbf4a8334b45d9b1fbb268116e4dfc1fad1d7";
const H2_DRIVER_CLASS: &str = "org.h2.Driver";

#[tokio::test]
async fn product_services_invoke_the_fixed_community_h2_compatibility_slice() {
    let (_directory, mut host, driver, driver_id, jdbc_url) = start_product().await;
    let application = host.application();

    verify_community_catalog(&application).await;

    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Community H2".to_owned(),
            driver_id: driver_id.clone(),
            connection: Some(DatasourceConnection {
                jdbc_url: jdbc_url.to_owned(),
                properties: Vec::new(),
                read_only: false,
            }),
        })
        .await
        .expect("product datasource must be stored");
    let built = application
        .build_community_create_schema(BuildCommunityCreateSchemaRequest {
            database_type: "H2".to_owned(),
            schema: chat2db_contract::CommunitySchema {
                name: "APP".to_owned(),
                ..chat2db_contract::CommunitySchema::default()
            },
        })
        .await
        .expect("Core must invoke the retained H2 SQL builder");
    assert_eq!(built.sql, "CREATE SCHEMA \"APP\";");

    create_metadata_fixture(&driver, &driver_id, jdbc_url, &built.sql).await;

    let schemas = application
        .list_community_schemas(ListCommunitySchemasRequest {
            datasource_id: datasource.id.clone(),
            database_type: "H2".to_owned(),
            database_name: String::new(),
        })
        .await
        .expect("Core metadata service must open a forced-read-only datasource session");
    assert!(schemas.items.iter().any(|schema| schema.name == "APP"));

    verify_object_metadata(&application, &datasource.id).await;

    let analysis = application
        .parse_community_sql(ParseCommunitySqlRequest {
            database_type: "H2".to_owned(),
            sql: "SELECT 1; UPDATE APP.items SET id = 2;".to_owned(),
        })
        .await
        .expect("Core must invoke the retained H2 parser");
    assert_eq!(analysis.statements.len(), 2);

    driver
        .unload_driver(driver_id)
        .await
        .expect("metadata session cleanup must release the H2 driver");
    host.shutdown()
        .await
        .expect("product host must shut down cleanly");
}

async fn create_metadata_fixture(
    driver: &DriverClient,
    driver_id: &str,
    jdbc_url: &str,
    schema_sql: &str,
) {
    let setup_session = driver
        .open_session(SessionConfig {
            driver_id: driver_id.to_owned(),
            jdbc_url: jdbc_url.to_owned(),
            properties: Vec::new(),
            read_only: false,
        })
        .await
        .expect("setup JDBC session must open");
    setup_session
        .execute_update(UpdateRequest {
            sql: schema_sql.to_owned(),
            parameters: Vec::new(),
            transaction_id: None,
        })
        .await
        .expect("built schema SQL must execute");
    setup_session
        .execute_update(UpdateRequest {
            sql: "CREATE TABLE APP.items (id BIGINT PRIMARY KEY, label VARCHAR(64) NOT NULL)"
                .to_owned(),
            parameters: Vec::new(),
            transaction_id: None,
        })
        .await
        .expect("test table must be created");
    setup_session
        .execute_update(UpdateRequest {
            sql: "CREATE UNIQUE INDEX idx_items_label ON APP.items(label)".to_owned(),
            parameters: Vec::new(),
            transaction_id: None,
        })
        .await
        .expect("test index must be created");
    for sql in [
        "CREATE TABLE APP.parents (id BIGINT NOT NULL, CONSTRAINT pk_parents PRIMARY KEY (id))",
        "CREATE TABLE APP.children (id BIGINT NOT NULL, parent_id BIGINT NOT NULL, CONSTRAINT pk_children PRIMARY KEY (id), CONSTRAINT fk_children_parent FOREIGN KEY (parent_id) REFERENCES APP.parents(id))",
        "CREATE VIEW APP.item_view AS SELECT id, label FROM APP.items",
        "CREATE ALIAS APP.add_one AS 'int addOne(int value) { return value + 1; }'",
        "CREATE ALIAS APP.record_event AS 'void recordEvent(int value) { }'",
        "CREATE TABLE APP.programmability_events (event_id BIGINT PRIMARY KEY)",
        "CREATE TRIGGER APP.audit_trigger BEFORE INSERT ON APP.programmability_events CALL 'ai.chat2db.rust.compat.fixture.AuditTrigger'",
    ] {
        setup_session
            .execute_update(UpdateRequest {
                sql: sql.to_owned(),
                parameters: Vec::new(),
                transaction_id: None,
            })
            .await
            .unwrap_or_else(|error| panic!("relation fixture SQL must execute: {error}"));
    }
    setup_session
        .close()
        .await
        .expect("setup JDBC session must close");
}

async fn verify_community_catalog(application: &Application) {
    let community_health = application
        .health()
        .components
        .into_iter()
        .find(|component| component.id == "community-compatibility")
        .expect("Community compatibility health must be published");
    assert_eq!(community_health.state, ComponentState::Ready);

    let catalog = application
        .list_community_plugins()
        .await
        .expect("Core must expose the real Community plugin catalog");
    assert_eq!(catalog.source_commit, COMMUNITY_COMMIT);
    let h2 = catalog
        .plugins
        .iter()
        .find(|plugin| plugin.database_type == "H2")
        .expect("real H2 plugin must be present");
    assert!(h2.services.metadata_available);
    assert!(h2.services.sql_builder_available);
    assert!(h2.services.sql_parser_available);
}

async fn verify_object_metadata(application: &Application, datasource_id: &str) {
    let databases = application
        .list_community_databases(ListCommunityDatabasesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: "H2".to_owned(),
        })
        .await
        .expect("Core must expose Community database metadata");
    let database = databases
        .items
        .iter()
        .find(|database| !database.name.is_empty())
        .expect("H2 must expose its current catalog");
    let tables = application
        .list_community_tables(ListCommunityTablesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: "H2".to_owned(),
            database_name: database.name.clone(),
            schema_name: "APP".to_owned(),
            table_name_pattern: "%".to_owned(),
        })
        .await
        .expect("Core must expose Community table metadata");
    let table = tables
        .items
        .iter()
        .find(|table| table.name.eq_ignore_ascii_case("items"))
        .expect("created table must be present");
    let columns = application
        .list_community_columns(ListCommunityColumnsRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: "H2".to_owned(),
            database_name: database.name.clone(),
            schema_name: "APP".to_owned(),
            table_name: table.name.clone(),
        })
        .await
        .expect("Core must expose Community column metadata");
    assert!(
        columns
            .items
            .iter()
            .any(|column| column.name.eq_ignore_ascii_case("id"))
    );
    let indexes = application
        .list_community_indexes(ListCommunityIndexesRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: "H2".to_owned(),
            database_name: database.name.clone(),
            schema_name: "APP".to_owned(),
            table_name: table.name.clone(),
        })
        .await
        .expect("Core must expose Community index metadata");
    assert!(indexes.items.iter().any(|index| {
        index.name.eq_ignore_ascii_case("idx_items_label")
            && index.unique == Some(true)
            && index
                .columns
                .iter()
                .any(|column| column.column_name.eq_ignore_ascii_case("label"))
    }));
    assert!(indexes.items.iter().any(|index| {
        index.unique == Some(true)
            && index
                .columns
                .iter()
                .any(|column| column.column_name.eq_ignore_ascii_case("id"))
    }));

    verify_relation_metadata(application, datasource_id, &database.name).await;
    verify_programmability_metadata(application, datasource_id, &database.name).await;
}

async fn verify_programmability_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    verify_function_metadata(application, datasource_id, database_name).await;
    verify_procedure_metadata(application, datasource_id, database_name).await;
    verify_trigger_metadata(application, datasource_id, database_name).await;
}

async fn verify_function_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let function_list = application
        .list_community_functions(ListCommunityFunctionsRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: "H2".to_owned(),
            database_name: database_name.to_owned(),
            schema_name: "APP".to_owned(),
        })
        .await
        .expect("Core must expose Community function metadata");
    assert!(
        function_list.items.is_empty(),
        "H2 exposes Java aliases only through JDBC procedure-list metadata"
    );

    let function_request = GetCommunityFunctionRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: "H2".to_owned(),
        database_name: database_name.to_owned(),
        schema_name: "APP".to_owned(),
        function_name: "ADD_ONE".to_owned(),
    };
    let function = application
        .get_community_function(function_request.clone())
        .await
        .expect("Core must expose Community function detail");
    assert_eq!(function.database_name, database_name);
    assert_eq!(function.schema_name, "APP");
    assert!(function.name.eq_ignore_ascii_case("add_one"));
    assert!(function.body.contains("addOne"));
    let function_parameters = application
        .list_community_function_parameters(function_request)
        .await
        .expect("Core must expose Community function parameters");
    assert!(
        function_parameters.items.is_empty(),
        "H2 does not expose Java-alias parameters as JDBC function parameters"
    );

    for function_name in ["MISSING_FUNCTION", "ADD_ONE' OR '1'='1"] {
        let error = application
            .get_community_function(GetCommunityFunctionRequest {
                datasource_id: datasource_id.to_owned(),
                database_type: "H2".to_owned(),
                database_name: database_name.to_owned(),
                schema_name: "APP".to_owned(),
                function_name: function_name.to_owned(),
            })
            .await
            .expect_err("missing or injected H2 function detail must fail");
        assert_community_error(&error, "community.function_not_found");
    }
    let error = application
        .get_community_function(GetCommunityFunctionRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: "H2".to_owned(),
            database_name: "WRONG_CATALOG".to_owned(),
            schema_name: "APP".to_owned(),
            function_name: "ADD_ONE".to_owned(),
        })
        .await
        .expect_err("H2 function detail must reject a mismatched catalog");
    assert_community_error(&error, "community.catalog_mismatch");
}

async fn verify_procedure_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let procedure_list = application
        .list_community_procedures(ListCommunityProceduresRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: "H2".to_owned(),
            database_name: database_name.to_owned(),
            schema_name: "APP".to_owned(),
        })
        .await
        .expect("Core must expose Community procedure metadata");
    let procedure = procedure_list
        .items
        .iter()
        .find(|procedure| procedure.name.eq_ignore_ascii_case("record_event"))
        .expect("created H2 procedure alias must be listed");
    assert_eq!(procedure.database_name, database_name);
    assert_eq!(procedure.schema_name, "APP");

    let procedure_request = GetCommunityProcedureRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: "H2".to_owned(),
        database_name: database_name.to_owned(),
        schema_name: "APP".to_owned(),
        procedure_name: "RECORD_EVENT".to_owned(),
    };
    let procedure = application
        .get_community_procedure(procedure_request.clone())
        .await
        .expect("Core must expose Community procedure detail");
    assert_eq!(procedure.database_name, database_name);
    assert_eq!(procedure.schema_name, "APP");
    assert!(procedure.name.eq_ignore_ascii_case("record_event"));
    assert!(procedure.body.contains("recordEvent"));
    let procedure_parameters = application
        .list_community_procedure_parameters(procedure_request)
        .await
        .expect("Core must expose Community procedure parameters");
    assert!(procedure_parameters.items.iter().any(|parameter| {
        parameter
            .procedure_name
            .eq_ignore_ascii_case("record_event")
            && parameter.procedure_schema.eq_ignore_ascii_case("APP")
    }));

    for procedure_name in ["MISSING_PROCEDURE", "RECORD_EVENT' OR '1'='1"] {
        let error = application
            .get_community_procedure(GetCommunityProcedureRequest {
                datasource_id: datasource_id.to_owned(),
                database_type: "H2".to_owned(),
                database_name: database_name.to_owned(),
                schema_name: "APP".to_owned(),
                procedure_name: procedure_name.to_owned(),
            })
            .await
            .expect_err("missing or injected H2 procedure detail must fail");
        assert_community_error(&error, "community.procedure_not_found");
    }
}

async fn verify_trigger_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let trigger_list = application
        .list_community_triggers(ListCommunityTriggersRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: "H2".to_owned(),
            database_name: database_name.to_owned(),
            schema_name: "APP".to_owned(),
        })
        .await
        .expect("Core must expose Community trigger metadata");
    let trigger = trigger_list
        .items
        .iter()
        .find(|trigger| trigger.name.eq_ignore_ascii_case("audit_trigger"))
        .expect("created H2 trigger must be listed");
    assert_eq!(trigger.database_name, database_name);
    assert_eq!(trigger.schema_name, "APP");

    let trigger = application
        .get_community_trigger(GetCommunityTriggerRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: "H2".to_owned(),
            database_name: database_name.to_owned(),
            schema_name: "APP".to_owned(),
            trigger_name: "AUDIT_TRIGGER".to_owned(),
        })
        .await
        .expect("Core must expose Community trigger detail");
    assert_eq!(trigger.database_name, database_name);
    assert_eq!(trigger.schema_name, "APP");
    assert!(trigger.name.eq_ignore_ascii_case("audit_trigger"));
    assert!(trigger.body.contains("AuditTrigger"));

    let injected_list = application
        .list_community_triggers(ListCommunityTriggersRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: "H2".to_owned(),
            database_name: database_name.to_owned(),
            schema_name: "APP'; DROP TABLE APP.programmability_events; --".to_owned(),
        })
        .await
        .expect("escaped H2 trigger-list identifiers must remain a safe metadata query");
    assert!(injected_list.items.is_empty());
    for trigger_name in ["MISSING_TRIGGER", "AUDIT_TRIGGER' OR '1'='1"] {
        let error = application
            .get_community_trigger(GetCommunityTriggerRequest {
                datasource_id: datasource_id.to_owned(),
                database_type: "H2".to_owned(),
                database_name: database_name.to_owned(),
                schema_name: "APP".to_owned(),
                trigger_name: trigger_name.to_owned(),
            })
            .await
            .expect_err("missing or injected H2 trigger detail must fail");
        assert_community_error(&error, "community.trigger_not_found");
    }
}

fn assert_community_error(error: &AppError, expected_code: &str) {
    assert_eq!(error.kind(), AppErrorKind::InvalidRequest);
    assert_eq!(error.api_error().code, expected_code);
}

async fn verify_relation_metadata(
    application: &Application,
    datasource_id: &str,
    database_name: &str,
) {
    let views = application
        .list_community_views(ListCommunityViewsRequest {
            datasource_id: datasource_id.to_owned(),
            database_type: "H2".to_owned(),
            database_name: database_name.to_owned(),
            schema_name: "APP".to_owned(),
            view_name_pattern: "%".to_owned(),
        })
        .await
        .expect("Core must expose Community view metadata");
    let view = views
        .items
        .iter()
        .find(|view| view.name.eq_ignore_ascii_case("item_view"))
        .expect("created H2 view must be present");
    assert_eq!(view.schema_name, "APP");
    assert!(view.table_type.eq_ignore_ascii_case("VIEW"));

    let child_request = ListCommunityTableKeysRequest {
        datasource_id: datasource_id.to_owned(),
        database_type: "H2".to_owned(),
        database_name: database_name.to_owned(),
        schema_name: "APP".to_owned(),
        table_name: "CHILDREN".to_owned(),
    };
    let imported = application
        .list_community_imported_keys(child_request.clone())
        .await
        .expect("Core must expose imported-key metadata");
    assert!(imported.items.iter().any(|key| {
        key.foreign_key_name
            .eq_ignore_ascii_case("fk_children_parent")
            && key.primary_table_name.eq_ignore_ascii_case("parents")
            && key.primary_column_name.eq_ignore_ascii_case("id")
            && key.foreign_table_name.eq_ignore_ascii_case("children")
            && key.foreign_column_name.eq_ignore_ascii_case("parent_id")
            && key.key_sequence == 1
    }));

    let parent_request = ListCommunityTableKeysRequest {
        table_name: "PARENTS".to_owned(),
        ..child_request
    };
    let exported = application
        .list_community_exported_keys(parent_request.clone())
        .await
        .expect("Core must expose exported-key metadata");
    assert!(exported.items.iter().any(|key| {
        key.foreign_key_name
            .eq_ignore_ascii_case("fk_children_parent")
            && key.primary_table_name.eq_ignore_ascii_case("parents")
            && key.foreign_table_name.eq_ignore_ascii_case("children")
    }));

    let primary = application
        .list_community_primary_keys(parent_request)
        .await
        .expect("Core must expose primary-key metadata");
    assert!(primary.items.iter().any(|key| {
        key.name.eq_ignore_ascii_case("pk_parents")
            && key.table_name.eq_ignore_ascii_case("parents")
            && key.column_name.eq_ignore_ascii_case("id")
    }));
}

async fn start_product() -> (TempDir, RuntimeHost, DriverClient, String, &'static str) {
    let engine_jar = required_path("CHAT2DB_JAVA_ENGINE_JAR");
    let h2_jar = required_path("CHAT2DB_H2_DRIVER_JAR");
    let community_directory = required_path("CHAT2DB_COMMUNITY_CLASSPATH_DIR");
    let classpath = load_fixed_community_classpath(&community_directory)
        .expect("Community distribution must exactly match the embedded lock");
    let directory = TempDir::new().expect("temporary product data directory");
    let trigger_jar = build_h2_trigger_jar(directory.path(), &h2_jar);
    let vault = Arc::new(
        EncryptedFileVault::new(directory.path(), [0x6b; 32])
            .expect("encrypted product vault must open"),
    );
    let storage = Storage::open(directory.path(), vault).expect("product storage must open");
    let supervisor = EngineSupervisor::spawn(
        EngineConfig::new(EngineCommand::java_jar("java", engine_jar))
            .with_community_classpath(classpath)
            .with_timeouts(
                Duration::from_secs(15),
                Duration::from_secs(15),
                Duration::from_secs(5),
            ),
    )
    .await
    .expect("fixed Community engine must start");
    let driver = supervisor
        .client()
        .driver_client()
        .expect("ready engine must expose JDBC");
    let loaded = driver
        .load_driver(DriverSpec {
            driver_class: H2_DRIVER_CLASS.to_owned(),
            artifacts: vec![
                DriverArtifact::from_path(h2_jar).expect("external H2 driver must be valid"),
                DriverArtifact::from_path(trigger_jar)
                    .expect("test H2 trigger artifact must be valid"),
            ],
        })
        .await
        .expect("external H2 driver must load outside the Community classpath");
    let jdbc_url = "jdbc:h2:mem:community_product;DB_CLOSE_DELAY=-1;DATABASE_TO_UPPER=TRUE";
    let host = RuntimeHost::from_supervisor(storage, supervisor);
    (directory, host, driver, loaded.driver_id, jdbc_url)
}

fn build_h2_trigger_jar(directory: &Path, h2_jar: &Path) -> PathBuf {
    let source = directory.join("trigger-src/ai/chat2db/rust/compat/fixture/AuditTrigger.java");
    fs::create_dir_all(source.parent().expect("trigger source must have a parent"))
        .expect("trigger source directory must be created");
    fs::write(
        &source,
        concat!(
            "package ai.chat2db.rust.compat.fixture;\n",
            "public final class AuditTrigger implements org.h2.api.Trigger {\n",
            "  public void fire(java.sql.Connection connection, ",
            "Object[] oldRow, Object[] newRow) { }\n",
            "}\n"
        ),
    )
    .expect("trigger source must be written");
    let classes = directory.join("trigger-classes");
    fs::create_dir(&classes).expect("trigger classes directory must be created");
    run_fixture_tool(
        Command::new("javac")
            .arg("-cp")
            .arg(h2_jar)
            .arg("-d")
            .arg(&classes)
            .arg(&source),
        "javac",
    );
    let trigger_jar = directory.join("h2-trigger-fixture.jar");
    run_fixture_tool(
        Command::new("jar")
            .arg("--create")
            .arg("--file")
            .arg(&trigger_jar)
            .arg("-C")
            .arg(&classes)
            .arg("."),
        "jar",
    );
    trigger_jar
}

fn run_fixture_tool(command: &mut Command, name: &str) {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{name} must start: {error}"));
    assert!(
        output.status.success(),
        "{name} must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name).map_or_else(
        || panic!("{name} must point to the integration fixture"),
        PathBuf::from,
    )
}
