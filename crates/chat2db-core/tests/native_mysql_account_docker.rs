use std::panic::AssertUnwindSafe;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    CommunityAccountAction, CommunityAccountCommandRequest, CommunityAccountGrantsRequest,
    CommunityAccountPrivilegeScope, ComponentState, CreateDatasourceRequest, DatasourceConnection,
    DatasourceConnectionProperty,
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
            eprintln!("skipping native MySQL account test; MYSQL_TEST_* variables are absent");
            return None;
        }
        assert_eq!(
            configured,
            REQUIRED_MYSQL_ENV.len(),
            "native MySQL integration is partially configured"
        );
        let host = required_env("MYSQL_TEST_HOST");
        assert!(
            !host.trim().is_empty()
                && !host.chars().any(char::is_control)
                && !host.contains(['/', '?', '#']),
            "MYSQL_TEST_HOST is invalid"
        );
        let port = required_env("MYSQL_TEST_PORT")
            .parse::<u16>()
            .expect("MYSQL_TEST_PORT must be a TCP port");
        assert_ne!(port, 0, "MYSQL_TEST_PORT cannot be zero");
        let user = required_env("MYSQL_TEST_USER");
        assert!(!user.is_empty(), "MYSQL_TEST_USER cannot be empty");
        Some(Self {
            host,
            port,
            user,
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
async fn native_mysql_account_lifecycle_keeps_java_dormant() {
    let Some(config) = MysqlTestConfig::from_environment() else {
        return;
    };
    let suffix = Uuid::new_v4().simple().to_string();
    let database_name = format!("chat2db_account_{}", &suffix[..12]);
    let account_user = format!("c2d'\\{}", &suffix[..12]);
    let account_host = "%";
    let metadata_user = format!("c2dh{}", &suffix[..12]);
    let metadata_host = format!("h'\\{}", &suffix[..12]);
    provision(
        &config,
        &database_name,
        &account_user,
        account_host,
        &metadata_user,
        &metadata_host,
    )
    .await;

    let verification = AssertUnwindSafe(verify_account_lifecycle(
        &config,
        &database_name,
        &account_user,
        account_host,
        &metadata_user,
        &metadata_host,
    ))
    .catch_unwind()
    .await;
    let cleanup = cleanup(
        &config,
        &database_name,
        &account_user,
        account_host,
        &metadata_user,
        &metadata_host,
    )
    .await;
    if let Err(payload) = verification {
        if let Err(error) = cleanup {
            eprintln!("native MySQL account cleanup also failed: {error}");
        }
        std::panic::resume_unwind(payload);
    }
    cleanup.expect("native MySQL account fixture must be removed");
}

#[allow(clippy::too_many_lines)]
async fn verify_account_lifecycle(
    config: &MysqlTestConfig,
    database_name: &str,
    user: &str,
    host_name: &str,
    metadata_user: &str,
    metadata_host: &str,
) {
    let directory = TempDir::new().expect("temporary native MySQL account runtime");
    let missing_java = directory.path().join("missing-java");
    let runtime = RuntimeConfig::new(EngineConfig::new(EngineCommand::new(missing_java)))
        .with_data_dir(directory.path().join("data"))
        .with_vault_master_key_base64(STANDARD.encode([0x71; 32]));
    let mut host = RuntimeHost::open(runtime)
        .await
        .expect("native MySQL account runtime must open without Java");
    let application = host.application();
    assert_java_dormant(&application);

    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Native MySQL account admin".to_owned(),
            driver_id: "mysql".to_owned(),
            connection: Some(config.connection(database_name)),
        })
        .await
        .expect("native MySQL account datasource must persist");

    let capability = application
        .mysql_account_capability(&datasource.id)
        .await
        .expect("account capability must load");
    assert_eq!(capability.db_type, "MYSQL");
    assert_eq!(capability.editable_privileges.len(), 14);
    assert!(capability.account_list_readable);
    assert!(capability.account_lock_supported);
    assert_eq!(
        capability.connection_user.as_deref(),
        Some(config.user.as_str())
    );
    assert_java_dormant(&application);

    let password = format!("Pa'ss\\word-{user}");
    let mut create = command(
        &datasource.id,
        user,
        host_name,
        CommunityAccountAction::CreateUser,
    );
    create.password = Some(password.clone());
    execute_success(&application, &mut create).await;
    assert_account_metadata(config, user, host_name).await;
    assert_account_login(config, user, host_name, &password).await;
    assert_java_dormant(&application);

    let duplicate = execute(&application, &mut create).await;
    assert!(!duplicate.success);
    assert_eq!(
        duplicate.failure_code.as_deref(),
        Some("mysql.account.executeFailed")
    );
    assert!(duplicate.error_code.is_some());
    assert!(
        !duplicate
            .message
            .as_deref()
            .unwrap_or_default()
            .contains(&password)
    );

    let accounts = application
        .list_mysql_accounts(&datasource.id)
        .await
        .expect("account list must load");
    let created = accounts
        .items
        .iter()
        .find(|account| account.user == user && account.host == host_name)
        .expect("created account must be listed");
    assert_eq!(created.display_name, format!("{user}@{host_name}"));
    assert_eq!(created.locked, Some(false));

    let mut alter = command(
        &datasource.id,
        user,
        host_name,
        CommunityAccountAction::AlterPassword,
    );
    let changed_password = format!("Changed'\\{password}");
    alter.password = Some(changed_password.clone());
    execute_success(&application, &mut alter).await;
    assert_account_login_rejected(config, user, &password).await;
    assert_account_login(config, user, host_name, &changed_password).await;

    let mut special_host = command(
        &datasource.id,
        metadata_user,
        metadata_host,
        CommunityAccountAction::CreateUser,
    );
    special_host.password = Some(format!("Meta'\\{password}"));
    execute_success(&application, &mut special_host).await;
    assert_account_metadata(config, metadata_user, metadata_host).await;
    let mut drop_special_host = command(
        &datasource.id,
        metadata_user,
        metadata_host,
        CommunityAccountAction::DropUser,
    );
    execute_success(&application, &mut drop_special_host).await;

    let mut lock = command(
        &datasource.id,
        user,
        host_name,
        CommunityAccountAction::LockAccount,
    );
    execute_success(&application, &mut lock).await;
    assert_eq!(
        listed_lock_state(&application, &datasource.id, user, host_name).await,
        Some(true)
    );

    let mut unlock = command(
        &datasource.id,
        user,
        host_name,
        CommunityAccountAction::UnlockAccount,
    );
    execute_success(&application, &mut unlock).await;
    assert_eq!(
        listed_lock_state(&application, &datasource.id, user, host_name).await,
        Some(false)
    );

    let mut grant = command(
        &datasource.id,
        user,
        host_name,
        CommunityAccountAction::GrantPrivilege,
    );
    grant.scope = Some(CommunityAccountPrivilegeScope::Table);
    grant.database_name = Some(database_name.to_owned());
    grant.table_name = Some("account_items".to_owned());
    grant.privileges = vec!["SELECT".to_owned(), "UPDATE".to_owned()];
    grant.grant_option = true;
    execute_success(&application, &mut grant).await;

    let grants = application
        .mysql_account_grants(&CommunityAccountGrantsRequest {
            datasource_id: datasource.id.clone(),
            user: user.to_owned(),
            host: host_name.to_owned(),
        })
        .await
        .expect("SHOW GRANTS must succeed");
    assert!(grants.items.iter().any(|grant| {
        grant.contains("SELECT") && grant.contains("UPDATE") && grant.contains(database_name)
    }));

    let mut revoke = grant.clone();
    revoke.action_type = CommunityAccountAction::RevokePrivilege;
    revoke.grant_option = false;
    execute_success(&application, &mut revoke).await;

    let mut drop_account = command(
        &datasource.id,
        user,
        host_name,
        CommunityAccountAction::DropUser,
    );
    execute_success(&application, &mut drop_account).await;
    assert!(
        application
            .list_mysql_accounts(&datasource.id)
            .await
            .expect("account list after drop")
            .items
            .iter()
            .all(|account| account.user != user || account.host != host_name)
    );
    assert_java_dormant(&application);

    host.shutdown()
        .await
        .expect("native-only account runtime must shut down cleanly");
}

async fn execute_success(application: &Application, request: &mut CommunityAccountCommandRequest) {
    let result = execute(application, request).await;
    assert!(result.success, "account operation failed: {result:?}");
}

async fn execute(
    application: &Application,
    request: &mut CommunityAccountCommandRequest,
) -> chat2db_contract::CommunityAccountExecution {
    let preview = application
        .preview_mysql_account(request)
        .expect("account preview must succeed");
    if let Some(password) = request.password.as_deref() {
        assert!(!preview.sql.contains(password));
    }
    request.preview_token = Some(preview.preview_token);
    application
        .execute_mysql_account(request)
        .await
        .expect("authorized account execution must return a structured result")
}

async fn listed_lock_state(
    application: &Application,
    datasource_id: &str,
    user: &str,
    host: &str,
) -> Option<bool> {
    application
        .list_mysql_accounts(datasource_id)
        .await
        .expect("account list must load")
        .items
        .into_iter()
        .find(|account| account.user == user && account.host == host)
        .and_then(|account| account.locked)
}

fn command(
    datasource_id: &str,
    user: &str,
    host: &str,
    action_type: CommunityAccountAction,
) -> CommunityAccountCommandRequest {
    CommunityAccountCommandRequest {
        datasource_id: datasource_id.to_owned(),
        user: user.to_owned(),
        host: host.to_owned(),
        action_type,
        scope: None,
        database_name: None,
        table_name: None,
        privileges: Vec::new(),
        grant_option: false,
        password: None,
        preview_token: None,
    }
}

async fn provision(
    config: &MysqlTestConfig,
    database_name: &str,
    user: &str,
    host: &str,
    metadata_user: &str,
    metadata_host: &str,
) {
    cleanup(
        config,
        database_name,
        user,
        host,
        metadata_user,
        metadata_host,
    )
    .await
    .expect("stale native MySQL account fixture must be removable");
    let mut conn = Conn::new(config.native_options())
        .await
        .expect("native MySQL account fixture must connect");
    conn.query_drop(format!("CREATE DATABASE `{database_name}`"))
        .await
        .expect("account fixture database must be created");
    conn.query_drop(format!(
        "CREATE TABLE `{database_name}`.`account_items` (id BIGINT PRIMARY KEY, note VARCHAR(64))"
    ))
    .await
    .expect("account fixture table must be created");
    conn.disconnect()
        .await
        .expect("account fixture connection must close");
}

async fn cleanup(
    config: &MysqlTestConfig,
    database_name: &str,
    user: &str,
    host: &str,
    metadata_user: &str,
    metadata_host: &str,
) -> Result<(), MysqlError> {
    let mut conn = Conn::new(config.native_options()).await?;
    enforce_no_backslash_escapes(&mut conn).await?;
    let account = format!("{}@{}", mysql_literal(user), mysql_literal(host));
    let metadata_account = format!(
        "{}@{}",
        mysql_literal(metadata_user),
        mysql_literal(metadata_host)
    );
    let account_result = conn
        .query_drop(format!("DROP USER IF EXISTS {account}, {metadata_account}"))
        .await;
    let database_result = conn
        .query_drop(format!("DROP DATABASE IF EXISTS `{database_name}`"))
        .await;
    let disconnect = conn.disconnect().await;
    account_result?;
    database_result?;
    disconnect
}

fn mysql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

async fn enforce_no_backslash_escapes(conn: &mut Conn) -> Result<(), MysqlError> {
    let current = conn
        .query_first::<String, _>("SELECT @@SESSION.sql_mode")
        .await?
        .unwrap_or_default();
    if current
        .split(',')
        .any(|mode| mode.trim().eq_ignore_ascii_case("NO_BACKSLASH_ESCAPES"))
    {
        return Ok(());
    }
    let mode = if current.trim().is_empty() {
        "NO_BACKSLASH_ESCAPES".to_owned()
    } else {
        format!("{},NO_BACKSLASH_ESCAPES", current.trim())
    };
    conn.exec_drop("SET SESSION sql_mode = ?", (mode,)).await
}

async fn assert_account_metadata(config: &MysqlTestConfig, user: &str, host: &str) {
    let mut conn = Conn::new(config.native_options())
        .await
        .expect("metadata verifier must connect");
    let row = conn
        .exec_first::<(String, String), _, _>(
            "SELECT User, Host FROM mysql.user WHERE User = ? AND Host = ?",
            (user, host),
        )
        .await
        .expect("account metadata query must succeed")
        .expect("account metadata must preserve the exact user and host");
    assert_eq!(row, (user.to_owned(), host.to_owned()));
    conn.disconnect()
        .await
        .expect("metadata verifier must disconnect");
}

async fn assert_account_login(config: &MysqlTestConfig, user: &str, host: &str, password: &str) {
    let options: Opts = OptsBuilder::default()
        .ip_or_hostname(config.host.clone())
        .tcp_port(config.port)
        .user(Some(user.to_owned()))
        .pass(Some(password.to_owned()))
        .prefer_socket(Some(false))
        .into();
    let mut conn = Conn::new(options)
        .await
        .expect("the exact generated MySQL credentials must authenticate");
    let current = conn
        .query_first::<String, _>("SELECT CURRENT_USER()")
        .await
        .expect("authenticated account identity query must succeed")
        .expect("authenticated account identity must exist");
    assert_eq!(current, format!("{user}@{host}"));
    conn.disconnect()
        .await
        .expect("authenticated account must disconnect");
}

async fn assert_account_login_rejected(config: &MysqlTestConfig, user: &str, password: &str) {
    let options: Opts = OptsBuilder::default()
        .ip_or_hostname(config.host.clone())
        .tcp_port(config.port)
        .user(Some(user.to_owned()))
        .pass(Some(password.to_owned()))
        .prefer_socket(Some(false))
        .into();
    assert!(
        Conn::new(options).await.is_err(),
        "the superseded password must no longer authenticate"
    );
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
