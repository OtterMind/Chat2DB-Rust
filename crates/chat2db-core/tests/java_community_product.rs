use std::{path::PathBuf, sync::Arc, time::Duration};

use chat2db_contract::{
    BuildCommunityCreateSchemaRequest, ComponentState, CreateDatasourceRequest,
    DatasourceConnection, ListCommunityColumnsRequest, ListCommunityDatabasesRequest,
    ListCommunityIndexesRequest, ListCommunitySchemasRequest, ListCommunityTablesRequest,
    ParseCommunitySqlRequest,
};
use chat2db_core::{Application, RuntimeHost, load_fixed_community_classpath};
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

    let setup_session = driver
        .open_session(SessionConfig {
            driver_id: driver_id.clone(),
            jdbc_url: jdbc_url.to_owned(),
            properties: Vec::new(),
            read_only: false,
        })
        .await
        .expect("setup JDBC session must open");
    setup_session
        .execute_update(UpdateRequest {
            sql: built.sql,
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
    setup_session
        .close()
        .await
        .expect("setup JDBC session must close");

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
}

async fn start_product() -> (TempDir, RuntimeHost, DriverClient, String, &'static str) {
    let engine_jar = required_path("CHAT2DB_JAVA_ENGINE_JAR");
    let h2_jar = required_path("CHAT2DB_H2_DRIVER_JAR");
    let community_directory = required_path("CHAT2DB_COMMUNITY_CLASSPATH_DIR");
    let classpath = load_fixed_community_classpath(&community_directory)
        .expect("Community distribution must exactly match the embedded lock");
    let directory = TempDir::new().expect("temporary product data directory");
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
            ],
        })
        .await
        .expect("external H2 driver must load outside the Community classpath");
    let jdbc_url = "jdbc:h2:mem:community_product;DB_CLOSE_DELAY=-1;DATABASE_TO_UPPER=TRUE";
    let host = RuntimeHost::from_supervisor(storage, supervisor);
    (directory, host, driver, loaded.driver_id, jdbc_url)
}

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name).map_or_else(
        || panic!("{name} must point to the integration fixture"),
        PathBuf::from,
    )
}
