use std::panic::AssertUnwindSafe;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    CommunitySchemaDiffEndpoint, CommunitySchemaDiffRequest, ComponentState,
    CreateDatasourceRequest, DatasourceConnection, DatasourceConnectionProperty,
};
use chat2db_core::{Application, RuntimeConfig, RuntimeHost};
use chat2db_java_bridge::{EngineCommand, EngineConfig};
use futures_util::FutureExt as _;
use mysql_async::{Conn, Error as MysqlError, Opts, OptsBuilder, prelude::Queryable};
use tempfile::TempDir;
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
        let required = mysql_test_required();
        let configured = REQUIRED_MYSQL_ENV
            .iter()
            .filter(|name| std::env::var_os(name).is_some())
            .count();
        if configured == 0 {
            assert!(
                !required,
                "MYSQL_TEST_REQUIRED is enabled but the MySQL endpoint is absent"
            );
            eprintln!("skipping native MySQL schema diff test; MYSQL_TEST_* variables are absent");
            return None;
        }
        assert_eq!(
            configured,
            REQUIRED_MYSQL_ENV.len(),
            "native MySQL integration is partially configured"
        );
        Some(Self {
            host: required_env("MYSQL_TEST_HOST"),
            port: required_env("MYSQL_TEST_PORT")
                .parse::<u16>()
                .expect("MYSQL_TEST_PORT must be a TCP port"),
            user: required_env("MYSQL_TEST_USER"),
            password: required_env("MYSQL_TEST_PASSWORD"),
        })
    }

    fn native_options(&self) -> Opts {
        OptsBuilder::default()
            .ip_or_hostname(self.host.clone())
            .tcp_port(self.port)
            .user(Some(self.user.clone()))
            .pass(Some(self.password.clone()))
            .prefer_socket(Some(false))
            .into()
    }

    fn connection(&self, database_name: &str) -> DatasourceConnection {
        let host = if self.host.contains(':')
            && !(self.host.starts_with('[') && self.host.ends_with(']'))
        {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        DatasourceConnection {
            jdbc_url: format!(
                "jdbc:mysql://{host}:{}/{database_name}?useSSL=false&serverTimezone=UTC",
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
}

#[tokio::test]
async fn native_mysql_schema_diff_is_preview_only_executable_and_keeps_java_dormant() {
    let Some(config) = MysqlTestConfig::from_environment() else {
        return;
    };
    let suffix = Uuid::new_v4().simple().to_string();
    let source_database = format!("chat2db_diff_src_{}", &suffix[..12]);
    let target_database = format!("chat2db_diff_dst_{}", &suffix[..12]);
    provision(&config, &source_database, &target_database).await;

    let verification = AssertUnwindSafe(verify_schema_diff(
        &config,
        &source_database,
        &target_database,
    ))
    .catch_unwind()
    .await;
    let cleanup = cleanup(&config, &source_database, &target_database).await;
    if let Err(payload) = verification {
        if let Err(error) = cleanup {
            eprintln!("native MySQL schema diff cleanup also failed: {error}");
        }
        std::panic::resume_unwind(payload);
    }
    cleanup.expect("native MySQL schema diff fixtures must be removed");
}

#[allow(clippy::too_many_lines)]
async fn verify_schema_diff(
    config: &MysqlTestConfig,
    source_database: &str,
    target_database: &str,
) {
    let directory = TempDir::new().expect("temporary native MySQL schema diff runtime");
    let missing_java = directory.path().join("missing-java");
    let runtime = RuntimeConfig::new(EngineConfig::new(EngineCommand::new(missing_java)))
        .with_data_dir(directory.path().join("data"))
        .with_vault_master_key_base64(STANDARD.encode([0x53; 32]));
    let mut host = RuntimeHost::open(runtime)
        .await
        .expect("native MySQL schema diff runtime must open without Java");
    let application = host.application();
    assert_java_dormant(&application);

    let source = application
        .create_datasource(CreateDatasourceRequest {
            name: "MySQL schema diff source".to_owned(),
            driver_id: "mysql".to_owned(),
            connection: Some(config.connection(source_database)),
        })
        .await
        .expect("source datasource must persist");
    let target = application
        .create_datasource(CreateDatasourceRequest {
            name: "MySQL schema diff target".to_owned(),
            driver_id: "mysql".to_owned(),
            connection: Some(config.connection(target_database)),
        })
        .await
        .expect("target datasource must persist");
    let request = CommunitySchemaDiffRequest {
        source: CommunitySchemaDiffEndpoint {
            datasource_id: source.id,
            database_name: source_database.to_owned(),
            schema_name: String::new(),
        },
        target: CommunitySchemaDiffEndpoint {
            datasource_id: target.id,
            database_name: target_database.to_owned(),
            schema_name: String::new(),
        },
    };

    let preview = application
        .preview_mysql_schema_diff(&request)
        .await
        .expect("schema diff preview must succeed");
    let sql = preview.as_str();
    assert!(sql.contains(format!("CREATE TABLE `{target_database}`.`added_only`").as_str()));
    assert!(sql.contains(format!("DROP TABLE `{target_database}`.`removed_only`;").as_str()));
    assert!(sql.contains(
        format!("ALTER TABLE `{target_database}`.`changed` DROP INDEX `idx_old`;").as_str()
    ));
    assert!(sql.contains(
        format!("ALTER TABLE `{target_database}`.`changed` DROP COLUMN `old_col`;").as_str()
    ));
    assert!(sql.contains("MODIFY COLUMN `id` bigint NOT NULL FIRST;"));
    assert!(
        sql.contains(
            "MODIFY COLUMN `title` varchar(100) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT '' AFTER `id`;"
        ),
        "generated schema diff:\n{sql}"
    );
    assert!(sql.contains("ADD COLUMN `new_col` int DEFAULT NULL AFTER `title`;"));
    assert!(
        sql.contains(
            format!("ALTER TABLE `{target_database}`.`changed` ADD KEY `idx_title` (`title`);")
                .as_str()
        )
    );
    assert!(
        sql.contains(
            format!(
                "ALTER TABLE `{target_database}`.`relations` DROP FOREIGN KEY `fk_parent_old`;"
            )
            .as_str()
        )
    );
    assert!(sql.contains(
        format!(
            "ALTER TABLE `{target_database}`.`relations` ADD CONSTRAINT `fk_parent` FOREIGN KEY (`parent_id`) REFERENCES `{target_database}`.`parents` (`id`) ON DELETE CASCADE;"
        )
        .as_str()
    ));
    assert!(sql.contains(
        format!(
            "ALTER TABLE `{target_database}`.`options_only` ENGINE=InnoDB, DEFAULT CHARACTER SET=utf8mb4, COLLATE=utf8mb4_unicode_ci, COMMENT='source option';"
        )
        .as_str()
    ));
    assert!(!sql.contains("AUTO_INCREMENT=42"));
    assert!(sql.contains(format!("DROP VIEW `{target_database}`.`removed_view`;").as_str()));
    assert!(sql.contains(format!("CREATE VIEW `{target_database}`.`added_view` AS").as_str()));
    assert!(sql.contains(format!("`{target_database}`.`added_only`").as_str()));
    assert!(sql.contains(
        format!("CREATE OR REPLACE VIEW `{target_database}`.`changed_view` AS").as_str()
    ));
    let parent_view = sql
        .find(format!("CREATE VIEW `{target_database}`.`z_parent_view`").as_str())
        .expect("parent view must be created");
    let child_view = sql
        .find(format!("CREATE VIEW `{target_database}`.`a_child_view`").as_str())
        .expect("dependent view must be created");
    assert!(parent_view < child_view);
    assert!(sql.contains(
        format!(
            "ALTER TABLE `{target_database}`.`pk_swap` DROP PRIMARY KEY, ADD PRIMARY KEY (`code`);"
        )
        .as_str()
    ));
    assert!(!sql.contains(format!("`{source_database}`.").as_str()));
    assert!(!sql.contains("chat2db_database_change_"));
    assert_java_dormant(&application);

    let relation_before = show_create(config, target_database, "relations").await;
    let options_before = show_create(config, target_database, "options_only").await;
    let changed_view_before = view_definition(config, target_database, "changed_view").await;
    let before = target_shape(config, target_database).await;
    assert_eq!(
        before,
        TargetShape {
            added_table: false,
            removed_table: true,
            old_column: true,
            new_column: false,
            old_index: true,
            new_index: false,
        },
        "preview must not mutate the target database"
    );
    assert_eq!(
        relation_before,
        show_create(config, target_database, "relations").await
    );
    assert_eq!(
        options_before,
        show_create(config, target_database, "options_only").await
    );
    assert_eq!(
        changed_view_before,
        view_definition(config, target_database, "changed_view").await
    );

    // Generated DDL must target the requested database even when the connection currently points
    // at a different catalog.
    execute_preview(config, source_database, sql).await;
    let after = target_shape(config, target_database).await;
    assert_eq!(
        after,
        TargetShape {
            added_table: true,
            removed_table: false,
            old_column: false,
            new_column: true,
            old_index: false,
            new_index: true,
        }
    );
    let no_diff = application
        .preview_mysql_schema_diff(&request)
        .await
        .expect("applied target must compare cleanly");
    if no_diff.as_str() != "-- No differences. " {
        let source_added = show_create(config, source_database, "added_only").await;
        let target_added = show_create(config, target_database, "added_only").await;
        let source_changed = show_create(config, source_database, "changed").await;
        let target_changed = show_create(config, target_database, "changed").await;
        panic!(
            "schema diff did not converge:\n{}\nsource added:\n{}\ntarget added:\n{}\nsource changed:\n{}\ntarget changed:\n{}",
            no_diff.as_str(),
            source_added,
            target_added,
            source_changed,
            target_changed
        );
    }
    assert_java_dormant(&application);

    host.shutdown()
        .await
        .expect("native-only schema diff runtime must shut down cleanly");
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, PartialEq, Eq)]
struct TargetShape {
    added_table: bool,
    removed_table: bool,
    old_column: bool,
    new_column: bool,
    old_index: bool,
    new_index: bool,
}

async fn target_shape(config: &MysqlTestConfig, database_name: &str) -> TargetShape {
    let mut conn = Conn::new(config.native_options())
        .await
        .expect("target shape connection");
    let table_exists = async |conn: &mut Conn, table_name: &str| {
        conn.exec_first::<u8, _, _>(
            "SELECT 1 FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND TABLE_TYPE = 'BASE TABLE' LIMIT 1",
            (database_name, table_name),
        )
        .await
        .expect("table shape query")
        .is_some()
    };
    let added_table = table_exists(&mut conn, "added_only").await;
    let removed_table = table_exists(&mut conn, "removed_only").await;
    let old_column = metadata_exists(
        &mut conn,
        "SELECT 1 FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = 'changed' AND COLUMN_NAME = ? LIMIT 1",
        database_name,
        "old_col",
    )
    .await;
    let new_column = metadata_exists(
        &mut conn,
        "SELECT 1 FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = 'changed' AND COLUMN_NAME = ? LIMIT 1",
        database_name,
        "new_col",
    )
    .await;
    let old_index = metadata_exists(
        &mut conn,
        "SELECT 1 FROM information_schema.STATISTICS \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = 'changed' AND INDEX_NAME = ? LIMIT 1",
        database_name,
        "idx_old",
    )
    .await;
    let new_index = metadata_exists(
        &mut conn,
        "SELECT 1 FROM information_schema.STATISTICS \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = 'changed' AND INDEX_NAME = ? LIMIT 1",
        database_name,
        "idx_title",
    )
    .await;
    conn.disconnect()
        .await
        .expect("target shape connection must close");
    TargetShape {
        added_table,
        removed_table,
        old_column,
        new_column,
        old_index,
        new_index,
    }
}

async fn metadata_exists(
    conn: &mut Conn,
    query: &str,
    database_name: &str,
    object_name: &str,
) -> bool {
    conn.exec_first::<u8, _, _>(query, (database_name, object_name))
        .await
        .expect("metadata shape query")
        .is_some()
}

async fn show_create(config: &MysqlTestConfig, database_name: &str, table_name: &str) -> String {
    let mut conn = Conn::new(config.native_options())
        .await
        .expect("show create connection");
    let row = conn
        .query_first::<(String, String), _>(format!(
            "SHOW CREATE TABLE `{database_name}`.`{table_name}`"
        ))
        .await
        .expect("show create query")
        .expect("show create row");
    conn.disconnect()
        .await
        .expect("show create connection must close");
    row.1
}

async fn view_definition(config: &MysqlTestConfig, database_name: &str, view_name: &str) -> String {
    let mut conn = Conn::new(config.native_options())
        .await
        .expect("view definition connection");
    let definition = conn
        .exec_first::<String, _, _>(
            "SELECT VIEW_DEFINITION FROM information_schema.VIEWS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
            (database_name, view_name),
        )
        .await
        .expect("view definition query")
        .expect("view definition row");
    conn.disconnect()
        .await
        .expect("view definition connection must close");
    definition
}

async fn execute_preview(config: &MysqlTestConfig, database_name: &str, sql: &str) {
    let mut conn = Conn::new(config.native_options())
        .await
        .expect("schema diff execution connection");
    conn.query_drop(format!("USE `{database_name}`"))
        .await
        .expect("target database must be selected");
    for statement in sql
        .split(";\n\n")
        .map(str::trim)
        .filter(|sql| !sql.is_empty())
    {
        conn.query_drop(statement).await.unwrap_or_else(|error| {
            panic!("generated schema diff statement failed: {error}: {statement}")
        });
    }
    conn.disconnect()
        .await
        .expect("schema diff execution connection must close");
}

#[allow(clippy::too_many_lines)]
async fn provision(config: &MysqlTestConfig, source_database: &str, target_database: &str) {
    cleanup(config, source_database, target_database)
        .await
        .expect("stale schema diff fixtures must be removable");
    let mut conn = Conn::new(config.native_options())
        .await
        .expect("schema diff fixture connection");
    // MySQL images ship different server collations, so pin the fixture metadata explicitly.
    for database_name in [source_database, target_database] {
        conn.query_drop(format!(
            "CREATE DATABASE `{database_name}` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci"
        ))
        .await
        .expect("schema diff fixture database must be created");
    }
    conn.query_drop(format!(
        "CREATE TABLE `{source_database}`.`added_only` (\
         id BIGINT NOT NULL, label VARCHAR(64) NOT NULL, \
         PRIMARY KEY (id), UNIQUE KEY uq_label (label)) ENGINE=InnoDB"
    ))
    .await
    .expect("source-only table must be created");
    conn.query_drop(format!(
        "CREATE TABLE `{source_database}`.`changed` (\
         id BIGINT NOT NULL, title VARCHAR(100) NOT NULL DEFAULT '', new_col INT DEFAULT NULL, \
         PRIMARY KEY (id), KEY idx_title (title)) ENGINE=InnoDB"
    ))
    .await
    .expect("source changed table must be created");
    conn.query_drop(format!(
        "CREATE TABLE `{target_database}`.`changed` (\
         id INT NOT NULL, old_col VARCHAR(10) DEFAULT NULL, title VARCHAR(20) DEFAULT NULL, \
         PRIMARY KEY (id), KEY idx_old (old_col)) ENGINE=InnoDB"
    ))
    .await
    .expect("target changed table must be created");
    conn.query_drop(format!(
        "CREATE TABLE `{target_database}`.`removed_only` (id BIGINT NOT NULL PRIMARY KEY) ENGINE=InnoDB"
    ))
    .await
    .expect("target-only table must be created");
    for database_name in [source_database, target_database] {
        conn.query_drop(format!(
            "CREATE TABLE `{database_name}`.`same_table` (id BIGINT NOT NULL PRIMARY KEY) ENGINE=InnoDB"
        ))
        .await
        .expect("matching table must be created");
    }
    conn.query_drop(format!(
        "CREATE TABLE `{source_database}`.`parents` (id BIGINT NOT NULL PRIMARY KEY) ENGINE=InnoDB"
    ))
    .await
    .expect("source foreign-key parent table must be created");
    conn.query_drop(format!(
        "CREATE TABLE `{target_database}`.`parents` (id INT NOT NULL PRIMARY KEY) ENGINE=InnoDB"
    ))
    .await
    .expect("target foreign-key parent table must be created");
    let source_relation_sql = format!(
        "CREATE TABLE `{source_database}`.`relations` (\
         id BIGINT NOT NULL PRIMARY KEY, parent_id BIGINT NOT NULL, \
         KEY idx_parent (parent_id), \
         CONSTRAINT `fk_parent` FOREIGN KEY (`parent_id`) REFERENCES `{source_database}`.`parents` (`id`) \
         ON DELETE CASCADE) \
         ENGINE=InnoDB"
    );
    conn.query_drop(&source_relation_sql)
        .await
        .unwrap_or_else(|error| {
            panic!("source foreign-key table must be created: {error}: {source_relation_sql}")
        });
    let target_relation_sql = format!(
        "CREATE TABLE `{target_database}`.`relations` (\
         id BIGINT NOT NULL PRIMARY KEY, parent_id INT NOT NULL, \
         KEY idx_parent (parent_id), \
         CONSTRAINT `fk_parent_old` FOREIGN KEY (`parent_id`) REFERENCES `{target_database}`.`parents` (`id`)) \
         ENGINE=InnoDB"
    );
    conn.query_drop(&target_relation_sql)
        .await
        .unwrap_or_else(|error| {
            panic!("target foreign-key table must be created: {error}: {target_relation_sql}")
        });
    conn.query_drop(format!(
        "CREATE TABLE `{source_database}`.`options_only` (\
         id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY) \
         ENGINE=InnoDB AUTO_INCREMENT=42 DEFAULT CHARSET=utf8mb4 \
         COLLATE=utf8mb4_unicode_ci COMMENT='source option'"
    ))
    .await
    .expect("source table options fixture must be created");
    conn.query_drop(format!(
        "CREATE TABLE `{target_database}`.`options_only` (\
         id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY) \
         ENGINE=MyISAM AUTO_INCREMENT=7 DEFAULT CHARSET=latin1 \
         COLLATE=latin1_swedish_ci COMMENT='target option'"
    ))
    .await
    .expect("target table options fixture must be created");
    conn.query_drop(format!(
        "CREATE TABLE `{source_database}`.`pk_swap` (\
         id BIGINT NOT NULL AUTO_INCREMENT, code BIGINT NOT NULL, \
         PRIMARY KEY (code), KEY idx_id (id)) ENGINE=InnoDB"
    ))
    .await
    .expect("source primary-key replacement fixture must be created");
    conn.query_drop(format!(
        "CREATE TABLE `{target_database}`.`pk_swap` (\
         id BIGINT NOT NULL AUTO_INCREMENT, code BIGINT NOT NULL, \
         PRIMARY KEY (id)) ENGINE=InnoDB"
    ))
    .await
    .expect("target primary-key replacement fixture must be created");
    conn.query_drop(format!(
        "CREATE VIEW `{source_database}`.`added_view` AS \
         SELECT id, label FROM `{source_database}`.`added_only`"
    ))
    .await
    .expect("source-only view must be created");
    conn.query_drop(format!(
        "CREATE VIEW `{source_database}`.`changed_view` AS \
         SELECT id, title FROM `{source_database}`.`changed`"
    ))
    .await
    .expect("source changed view must be created");
    conn.query_drop(format!(
        "CREATE VIEW `{target_database}`.`changed_view` AS \
         SELECT id, old_col FROM `{target_database}`.`changed`"
    ))
    .await
    .expect("target changed view must be created");
    conn.query_drop(format!(
        "CREATE VIEW `{target_database}`.`removed_view` AS \
         SELECT id FROM `{target_database}`.`removed_only`"
    ))
    .await
    .expect("target-only view must be created");
    conn.query_drop(format!(
        "CREATE VIEW `{source_database}`.`z_parent_view` AS \
         SELECT id FROM `{source_database}`.`same_table`"
    ))
    .await
    .expect("source parent view must be created");
    conn.query_drop(format!(
        "CREATE VIEW `{source_database}`.`a_child_view` AS \
         SELECT id FROM `{source_database}`.`z_parent_view`"
    ))
    .await
    .expect("source dependent view must be created");
    conn.disconnect()
        .await
        .expect("schema diff fixture connection must close");
}

async fn cleanup(
    config: &MysqlTestConfig,
    source_database: &str,
    target_database: &str,
) -> Result<(), MysqlError> {
    let mut conn = Conn::new(config.native_options()).await?;
    let source_result = conn
        .query_drop(format!("DROP DATABASE IF EXISTS `{source_database}`"))
        .await;
    let target_result = conn
        .query_drop(format!("DROP DATABASE IF EXISTS `{target_database}`"))
        .await;
    let disconnect = conn.disconnect().await;
    source_result?;
    target_result?;
    disconnect
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

fn mysql_test_required() -> bool {
    std::env::var("MYSQL_TEST_REQUIRED").is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be configured"))
}
