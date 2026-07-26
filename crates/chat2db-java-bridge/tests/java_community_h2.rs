use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use chat2db_java_bridge::{
    COMMUNITY_OBJECT_METADATA_CAPABILITY, COMMUNITY_PLUGIN_CATALOG_CAPABILITY,
    COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY, COMMUNITY_RELATION_METADATA_CAPABILITY,
    COMMUNITY_SCHEMA_METADATA_CAPABILITY, COMMUNITY_SQL_BUILDER_CAPABILITY,
    COMMUNITY_SQL_FORMATTER_CAPABILITY, COMMUNITY_SQL_PARSER_CAPABILITY,
    COMMUNITY_SQL_VALIDATION_CAPABILITY, CommunityClasspath, CommunityClient,
    CommunityPluginCatalog, CommunitySchema, DriverArtifact, DriverSpec, EngineCommand,
    EngineConfig, EngineState, EngineSupervisor, Session, SessionConfig, UpdateRequest,
};
use tempfile::TempDir;

const COMMUNITY_COMMIT: &str = "f63cbf4a8334b45d9b1fbb268116e4dfc1fad1d7";
const H2_DRIVER_CLASS: &str = "org.h2.Driver";
const COMMUNITY_CLASSPATH_LOCK: &str =
    include_str!("../../../third_party/community-h2-classpath.lock");

#[tokio::test]
async fn invokes_real_community_h2_spi_metadata_builder_and_parser() {
    let engine_jar = required_path("CHAT2DB_JAVA_ENGINE_JAR");
    let h2_jar = required_path("CHAT2DB_H2_DRIVER_JAR");
    let community_directory = required_path("CHAT2DB_COMMUNITY_CLASSPATH_DIR");
    let classpath =
        CommunityClasspath::from_locked_directory(&community_directory, COMMUNITY_CLASSPATH_LOCK)
            .expect("Community classpath must exactly match the fixed lock");

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
    .expect("Java engine with Community compatibility must start");
    let EngineState::Ready { identity, .. } = supervisor.client().state() else {
        panic!("engine must be ready after spawn");
    };
    for capability in [
        COMMUNITY_PLUGIN_CATALOG_CAPABILITY,
        COMMUNITY_SCHEMA_METADATA_CAPABILITY,
        COMMUNITY_OBJECT_METADATA_CAPABILITY,
        COMMUNITY_RELATION_METADATA_CAPABILITY,
        COMMUNITY_PROGRAMMABILITY_METADATA_CAPABILITY,
        COMMUNITY_SQL_BUILDER_CAPABILITY,
        COMMUNITY_SQL_PARSER_CAPABILITY,
        COMMUNITY_SQL_VALIDATION_CAPABILITY,
        COMMUNITY_SQL_FORMATTER_CAPABILITY,
    ] {
        assert!(
            identity
                .capabilities
                .iter()
                .any(|value| value == capability)
        );
    }

    let community = supervisor
        .client()
        .community_client()
        .expect("ready engine must expose a Community client");
    let catalog = community
        .list_plugins()
        .await
        .expect("real Community plugins must be discovered");
    assert_catalog(&catalog);

    let driver_client = supervisor
        .client()
        .driver_client()
        .expect("ready engine must expose a JDBC client");
    let trigger_directory = TempDir::new().expect("temporary trigger directory must open");
    let trigger_jar = build_h2_trigger_jar(trigger_directory.path(), &h2_jar);
    let loaded = driver_client
        .load_driver(DriverSpec {
            driver_class: H2_DRIVER_CLASS.to_owned(),
            artifacts: vec![
                DriverArtifact::from_path(h2_jar).expect("H2 driver must satisfy artifact limits"),
                DriverArtifact::from_path(trigger_jar)
                    .expect("H2 trigger fixture must satisfy artifact limits"),
            ],
        })
        .await
        .expect("external H2 driver must load independently of the plugin classpath");
    let session = driver_client
        .open_session(SessionConfig {
            driver_id: loaded.driver_id.clone(),
            jdbc_url: "jdbc:h2:mem:community_spi;DB_CLOSE_DELAY=-1;DATABASE_TO_UPPER=TRUE"
                .to_owned(),
            properties: Vec::new(),
            read_only: false,
        })
        .await
        .expect("H2 session must open");

    create_and_verify_schema(&community, &session).await;
    verify_object_tree(&community, &session).await;
    verify_parser(&community).await;

    session.close().await.expect("H2 session must close");
    driver_client
        .unload_driver(loaded.driver_id)
        .await
        .expect("external H2 driver must unload");
    let exit = supervisor
        .shutdown()
        .await
        .expect("Java engine must shut down cleanly");
    assert!(exit.success);
}

fn assert_catalog(catalog: &CommunityPluginCatalog) {
    assert_eq!(catalog.source_commit, COMMUNITY_COMMIT);
    let h2 = catalog
        .plugins
        .iter()
        .find(|plugin| plugin.database_type == "H2")
        .expect("H2 ServiceLoader plugin must be present");
    assert_eq!(h2.name, "H2");
    assert!(h2.behavior.supports_database);
    assert!(h2.behavior.supports_schema);
    assert!(h2.services.metadata_available);
    assert!(h2.services.sql_builder_available);
    assert!(h2.services.sql_parser_available);
    assert!(
        h2.drivers
            .iter()
            .any(|driver| driver.jdbc_driver_class == H2_DRIVER_CLASS && driver.default_driver)
    );
    assert!(
        catalog
            .plugins
            .iter()
            .any(|plugin| plugin.database_type == "MYSQL")
    );
}

async fn create_and_verify_schema(community: &CommunityClient, session: &Session) {
    let built = community
        .build_create_schema(
            "H2",
            CommunitySchema {
                name: "APP".to_owned(),
                ..CommunitySchema::default()
            },
        )
        .await
        .expect("H2SqlBuilder must build CREATE SCHEMA");
    assert_eq!(built, "CREATE SCHEMA \"APP\";");
    execute_update(session, &built).await;
    execute_update(
        session,
        "CREATE TABLE APP.items (id BIGINT PRIMARY KEY, label VARCHAR(64) NOT NULL)",
    )
    .await;
    execute_update(
        session,
        "CREATE UNIQUE INDEX idx_items_label ON APP.items(label)",
    )
    .await;
    execute_update(
        session,
        "CREATE TABLE APP.parents (id BIGINT NOT NULL, CONSTRAINT pk_parents PRIMARY KEY (id))",
    )
    .await;
    execute_update(
        session,
        "CREATE TABLE APP.children (id BIGINT NOT NULL, parent_id BIGINT NOT NULL, CONSTRAINT pk_children PRIMARY KEY (id), CONSTRAINT fk_children_parent FOREIGN KEY (parent_id) REFERENCES APP.parents(id))",
    )
    .await;
    execute_update(
        session,
        "CREATE VIEW APP.item_view AS SELECT id, label FROM APP.items",
    )
    .await;
    execute_update(
        session,
        "CREATE ALIAS APP.add_one AS 'int addOne(int value) { return value + 1; }'",
    )
    .await;
    execute_update(
        session,
        "CREATE ALIAS APP.record_event AS 'void recordEvent(int value) { }'",
    )
    .await;
    execute_update(
        session,
        "CREATE TABLE APP.programmability_events (event_id BIGINT PRIMARY KEY)",
    )
    .await;
    execute_update(
        session,
        "CREATE TRIGGER APP.audit_trigger BEFORE INSERT ON APP.programmability_events CALL 'ai.chat2db.rust.compat.fixture.AuditTrigger'",
    )
    .await;

    let schemas = community
        .list_schemas(session, "H2", "", None)
        .await
        .expect("H2Meta must list schemas through the existing JDBC session");
    assert!(schemas.iter().any(|schema| schema.name == "APP"));
}

async fn verify_object_tree(community: &CommunityClient, session: &Session) {
    let databases = community
        .list_databases(session, "H2", None)
        .await
        .expect("H2Meta must list databases");
    let database = databases
        .iter()
        .find(|database| !database.name.is_empty())
        .expect("H2 must expose its current catalog");

    let tables = community
        .list_tables(session, "H2", &database.name, "APP", "%", None)
        .await
        .expect("H2Meta must list tables");
    let table = tables
        .iter()
        .find(|table| table.name.eq_ignore_ascii_case("items"))
        .expect("created H2 table must be projected");
    assert_eq!(table.schema_name, "APP");

    let columns = community
        .list_columns(session, "H2", &database.name, "APP", &table.name, None)
        .await
        .expect("H2Meta must list columns");
    assert!(
        columns
            .iter()
            .any(|column| column.name.eq_ignore_ascii_case("id"))
    );
    assert!(columns.iter().any(|column| {
        column.name.eq_ignore_ascii_case("label") && column.column_type == "CHARACTER VARYING"
    }));

    let indexes = community
        .list_indexes(session, "H2", &database.name, "APP", &table.name, None)
        .await
        .expect("H2Meta must list indexes");
    let index = indexes
        .iter()
        .find(|index| index.name.eq_ignore_ascii_case("idx_items_label"))
        .expect("created H2 index must be projected");
    assert_eq!(index.unique, Some(true));
    assert!(
        index
            .columns
            .iter()
            .any(|column| column.column_name.eq_ignore_ascii_case("label"))
    );
    assert!(indexes.iter().any(|index| {
        index.unique == Some(true)
            && index
                .columns
                .iter()
                .any(|column| column.column_name.eq_ignore_ascii_case("id"))
    }));

    verify_relation_metadata(community, session, &database.name).await;
    verify_programmability_metadata(community, session, &database.name).await;
}

async fn verify_programmability_metadata(
    community: &CommunityClient,
    session: &Session,
    database_name: &str,
) {
    let functions = community
        .list_functions(session, "H2", database_name, "APP", None)
        .await
        .expect("H2Meta must list functions");
    assert!(
        functions.is_empty(),
        "H2 exposes Java aliases only through JDBC procedure-list metadata"
    );
    let function = community
        .get_function(session, "H2", database_name, "APP", "ADD_ONE", None)
        .await
        .expect("H2Meta must read the created function alias");
    assert_eq!(function.database_name, database_name);
    assert_eq!(function.schema_name, "APP");
    assert!(function.name.eq_ignore_ascii_case("add_one"));
    assert!(function.body.contains("addOne"));
    let function_parameters = community
        .list_function_parameters(session, "H2", database_name, "APP", "ADD_ONE", None)
        .await
        .expect("H2Meta must list function parameters");
    assert!(function_parameters.is_empty());

    let procedures = community
        .list_procedures(session, "H2", database_name, "APP", None)
        .await
        .expect("H2Meta must list procedures");
    assert!(
        procedures
            .iter()
            .any(|procedure| procedure.name.eq_ignore_ascii_case("record_event"))
    );
    let procedure = community
        .get_procedure(session, "H2", database_name, "APP", "RECORD_EVENT", None)
        .await
        .expect("H2Meta must read the created procedure alias");
    assert_eq!(procedure.database_name, database_name);
    assert_eq!(procedure.schema_name, "APP");
    assert!(procedure.name.eq_ignore_ascii_case("record_event"));
    assert!(procedure.body.contains("recordEvent"));
    let procedure_parameters = community
        .list_procedure_parameters(session, "H2", database_name, "APP", "RECORD_EVENT", None)
        .await
        .expect("H2Meta must list procedure parameters");
    assert!(procedure_parameters.iter().any(|parameter| {
        parameter
            .procedure_name
            .eq_ignore_ascii_case("record_event")
            && parameter.procedure_schema.eq_ignore_ascii_case("APP")
    }));

    let triggers = community
        .list_triggers(session, "H2", database_name, "APP", None)
        .await
        .expect("H2Meta must list triggers");
    assert!(
        triggers
            .iter()
            .any(|trigger| trigger.name.eq_ignore_ascii_case("audit_trigger"))
    );
    let trigger = community
        .get_trigger(session, "H2", database_name, "APP", "AUDIT_TRIGGER", None)
        .await
        .expect("H2Meta must read the created trigger");
    assert_eq!(trigger.database_name, database_name);
    assert_eq!(trigger.schema_name, "APP");
    assert!(trigger.name.eq_ignore_ascii_case("audit_trigger"));
    assert!(trigger.body.contains("AuditTrigger"));
}

fn build_h2_trigger_jar(directory: &Path, h2_jar: &Path) -> PathBuf {
    let source = directory.join("src/ai/chat2db/rust/compat/fixture/AuditTrigger.java");
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
    let classes = directory.join("classes");
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

async fn verify_relation_metadata(
    community: &CommunityClient,
    session: &Session,
    database_name: &str,
) {
    let views = community
        .list_views(session, "H2", database_name, "APP", "%", None)
        .await
        .expect("H2Meta must list views");
    let view = views
        .iter()
        .find(|view| view.name.eq_ignore_ascii_case("item_view"))
        .expect("created H2 view must be projected");
    assert_eq!(view.schema_name, "APP");
    assert!(view.table_type.eq_ignore_ascii_case("VIEW"));

    let imported = community
        .list_imported_keys(session, "H2", database_name, "APP", "CHILDREN", None)
        .await
        .expect("H2Meta must list imported keys");
    let imported_key = imported
        .iter()
        .find(|key| {
            key.foreign_key_name
                .eq_ignore_ascii_case("fk_children_parent")
        })
        .unwrap_or_else(|| panic!("named child foreign key must be imported: {imported:#?}"));
    assert!(
        imported_key
            .primary_table_name
            .eq_ignore_ascii_case("parents")
    );
    assert!(imported_key.primary_column_name.eq_ignore_ascii_case("id"));
    assert!(
        imported_key
            .foreign_table_name
            .eq_ignore_ascii_case("children")
    );
    assert!(
        imported_key
            .foreign_column_name
            .eq_ignore_ascii_case("parent_id")
    );
    assert_eq!(imported_key.key_sequence, 1);

    let exported = community
        .list_exported_keys(session, "H2", database_name, "APP", "PARENTS", None)
        .await
        .expect("H2Meta must list exported keys");
    assert!(exported.iter().any(|key| {
        key.foreign_key_name
            .eq_ignore_ascii_case("fk_children_parent")
            && key.primary_table_name.eq_ignore_ascii_case("parents")
            && key.foreign_table_name.eq_ignore_ascii_case("children")
    }));

    let primary = community
        .list_primary_keys(session, "H2", database_name, "APP", "PARENTS", None)
        .await
        .expect("H2Meta must list primary keys");
    assert!(primary.iter().any(|key| {
        key.name.eq_ignore_ascii_case("pk_parents")
            && key.table_name.eq_ignore_ascii_case("parents")
            && key.column_name.eq_ignore_ascii_case("id")
    }));
}

async fn verify_parser(community: &CommunityClient) {
    let select = community
        .parse_sql("H2", "SELECT id, label FROM APP.items;")
        .await
        .expect("H2 syntax plugin must invoke the retained MySQL ANTLR parser");
    assert!(select.is_select);
    assert_eq!(select.statements.len(), 1);
    assert!(select.statements[0].sql.contains("SELECT"));

    let script = community
        .parse_sql(
            "H2",
            "SELECT id FROM APP.items; UPDATE APP.items SET label = 'x' WHERE id = 1;",
        )
        .await
        .expect("Community parser must split a bounded SQL script");
    assert_eq!(script.statements.len(), 2);

    let valid = community
        .validate_sql("H2", "SELECT id, label FROM APP.items;")
        .await
        .expect("Community parser must validate well-formed SQL");
    assert!(valid.valid);
    assert_eq!(valid.statements.len(), 1);
    assert!(valid.diagnostics.is_empty());

    let invalid = community
        .validate_sql("H2", "SELECT FROM;")
        .await
        .expect("Community parser must return bounded syntax diagnostics");
    assert!(!invalid.valid);
    assert!(!invalid.diagnostics.is_empty());
    assert!(
        invalid
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.is_empty())
    );

    let formatted = community
        .format_sql("H2", "select id,name from items where id=1")
        .await
        .expect("Community formatter must format SQL without a JDBC session");
    assert!(formatted.sql.contains('\n'));
    assert!(formatted.sql.contains("from\n  items"));

    let blank = community
        .format_sql("H2", " \n\t")
        .await
        .expect_err("blank formatter input must fail before transport");
    assert!(blank.to_string().contains("SQL is required"));

    let oversized = community
        .format_sql("H2", "x".repeat(1_048_577))
        .await
        .expect_err("formatter input above one MiB must fail before transport");
    assert!(oversized.to_string().contains("1048576 UTF-8 bytes"));

    let token_dense = community
        .format_sql("H2", "a,".repeat(8_193))
        .await
        .expect_err("formatter complexity must fail before entering the Java engine");
    assert!(token_dense.to_string().contains("16384 units"));
}

async fn execute_update(session: &Session, sql: &str) {
    session
        .execute_update(UpdateRequest {
            sql: sql.to_owned(),
            parameters: Vec::new(),
            transaction_id: None,
        })
        .await
        .expect("H2 update must complete");
}

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name).map_or_else(
        || panic!("{name} must point to the integration fixture"),
        PathBuf::from,
    )
}
