use std::{path::PathBuf, time::Duration};

use chat2db_java_bridge::{
    COMMUNITY_PLUGIN_CATALOG_CAPABILITY, COMMUNITY_SCHEMA_METADATA_CAPABILITY,
    COMMUNITY_SQL_BUILDER_CAPABILITY, COMMUNITY_SQL_PARSER_CAPABILITY, CommunityClasspath,
    CommunityClient, CommunityPluginCatalog, CommunitySchema, DriverArtifact, DriverSpec,
    EngineCommand, EngineConfig, EngineState, EngineSupervisor, Session, SessionConfig,
    UpdateRequest,
};

const COMMUNITY_COMMIT: &str = "f63cbf4a8334b45d9b1fbb268116e4dfc1fad1d7";
const H2_DRIVER_CLASS: &str = "org.h2.Driver";

#[tokio::test]
async fn invokes_real_community_h2_spi_metadata_builder_and_parser() {
    let engine_jar = required_path("CHAT2DB_JAVA_ENGINE_JAR");
    let h2_jar = required_path("CHAT2DB_H2_DRIVER_JAR");
    let community_directory = required_path("CHAT2DB_COMMUNITY_CLASSPATH_DIR");
    let classpath =
        CommunityClasspath::from_paths(COMMUNITY_COMMIT, classpath_jars(&community_directory))
            .expect("Community classpath must satisfy the process contract");

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
        COMMUNITY_SQL_BUILDER_CAPABILITY,
        COMMUNITY_SQL_PARSER_CAPABILITY,
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
    let loaded = driver_client
        .load_driver(DriverSpec {
            driver_class: H2_DRIVER_CLASS.to_owned(),
            artifacts: vec![
                DriverArtifact::from_path(h2_jar).expect("H2 driver must satisfy artifact limits"),
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

    let schemas = community
        .list_schemas(session, "H2", "", None)
        .await
        .expect("H2Meta must list schemas through the existing JDBC session");
    assert!(schemas.iter().any(|schema| schema.name == "APP"));
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

fn classpath_jars(directory: &PathBuf) -> Vec<PathBuf> {
    let mut jars = std::fs::read_dir(directory)
        .expect("Community classpath directory must exist")
        .map(|entry| entry.expect("classpath entry must be readable").path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("jar"))
        })
        .collect::<Vec<_>>();
    jars.sort();
    assert!(!jars.is_empty(), "Community classpath cannot be empty");
    jars
}
