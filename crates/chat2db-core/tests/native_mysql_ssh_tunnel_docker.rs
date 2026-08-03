use std::{net::Ipv4Addr, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chat2db_contract::{
    ComponentState, CreateDatasourceRequest, DatasourceConnection, DatasourceConnectionProperty,
    SshAuthentication, SshHostKeyVerification, SshTunnelConfig,
};
use chat2db_core::{
    Application, MysqlConsoleCancellation, MysqlConsoleRequest, MysqlConsoleResult, RuntimeConfig,
    RuntimeHost,
};
use chat2db_java_bridge::{EngineCommand, EngineConfig};
use tempfile::TempDir;
use tokio::net::{TcpListener, TcpStream};

const QUERY_TIMEOUT: Duration = Duration::from_secs(30);
const PORT_STATE_TIMEOUT: Duration = Duration::from_secs(10);

struct MysqlSshTestConfig {
    mysql_host: String,
    mysql_port: u16,
    mysql_user: String,
    mysql_password: String,
    ssh: SshTunnelConfig,
}

impl MysqlSshTestConfig {
    fn from_environment() -> Self {
        let mysql_host = required_host("CHAT2DB_TEST_MYSQL_HOST");
        let mysql_port = required_port("CHAT2DB_TEST_MYSQL_PORT");
        let mysql_user = required_env("CHAT2DB_TEST_MYSQL_USER");
        assert!(
            !mysql_user.is_empty(),
            "CHAT2DB_TEST_MYSQL_USER cannot be empty"
        );

        let ssh_host = required_host("CHAT2DB_TEST_SSH_HOST");
        let ssh_port = required_port("CHAT2DB_TEST_SSH_PORT");
        let ssh_user = required_env("CHAT2DB_TEST_SSH_USER");
        assert!(
            !ssh_user.is_empty(),
            "CHAT2DB_TEST_SSH_USER cannot be empty"
        );
        let local_port = required_port("CHAT2DB_TEST_SSH_LOCAL_PORT");
        let authentication = ssh_authentication();

        Self {
            mysql_host,
            mysql_port,
            mysql_user,
            mysql_password: required_env("CHAT2DB_TEST_MYSQL_PASSWORD"),
            ssh: SshTunnelConfig {
                host_name: ssh_host,
                port: ssh_port,
                user_name: ssh_user,
                authentication,
                host_key_verification: SshHostKeyVerification::KnownHosts,
                local_port: Some(local_port),
            },
        }
    }

    fn connection(&self) -> DatasourceConnection {
        let host = url_host(&self.mysql_host);
        DatasourceConnection {
            jdbc_url: format!(
                "jdbc:mysql://{host}:{}/?useSSL=false&serverTimezone=UTC",
                self.mysql_port
            ),
            properties: vec![
                DatasourceConnectionProperty {
                    key: "user".to_owned(),
                    value: self.mysql_user.clone(),
                    sensitive: false,
                },
                DatasourceConnectionProperty {
                    key: "password".to_owned(),
                    value: self.mysql_password.clone(),
                    sensitive: true,
                },
            ],
            read_only: false,
            ssh: Some(self.ssh.clone()),
        }
    }

    fn local_port(&self) -> u16 {
        self.ssh
            .local_port
            .expect("test requires a fixed local port")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires CHAT2DB_TEST_MYSQL_* and CHAT2DB_TEST_SSH_* endpoints plus a known_hosts entry"]
async fn native_mysql_concurrent_queries_share_one_fixed_ssh_tunnel() {
    let config = MysqlSshTestConfig::from_environment();
    assert_port_available(config.local_port()).await;

    let directory = TempDir::new().expect("temporary native MySQL SSH runtime");
    let missing_java = directory.path().join("missing-java");
    let runtime = RuntimeConfig::new(EngineConfig::new(EngineCommand::new(missing_java)))
        .with_data_dir(directory.path().join("data"))
        .with_vault_master_key_base64(STANDARD.encode([0x73; 32]));
    let mut host = RuntimeHost::open(runtime)
        .await
        .expect("native MySQL SSH runtime opens without Java");
    let application = host.application();
    let datasource = application
        .create_datasource(CreateDatasourceRequest {
            name: "Native MySQL shared SSH tunnel".to_owned(),
            driver_id: "mysql".to_owned(),
            connection: Some(config.connection()),
        })
        .await
        .expect("SSH-backed native MySQL datasource persists");
    assert_java_dormant(&application);

    let first = spawn_sleep_query(application.clone(), datasource.id.clone());
    wait_for_port_bound(config.local_port()).await;
    let second = spawn_sleep_query(application.clone(), datasource.id);

    let first_results = await_query(first, "first").await;
    let second_results = await_query(second, "second").await;
    assert_query_succeeded(&first_results);
    assert_query_succeeded(&second_results);
    wait_for_port_released(config.local_port()).await;
    assert_java_dormant(&application);

    host.shutdown()
        .await
        .expect("native-only SSH runtime shuts down cleanly");
}

fn spawn_sleep_query(
    application: Application,
    datasource_id: String,
) -> tokio::task::JoinHandle<Result<Vec<MysqlConsoleResult>, chat2db_core::AppError>> {
    tokio::spawn(async move {
        application
            .execute_mysql_console(
                MysqlConsoleRequest {
                    datasource_id,
                    database_name: String::new(),
                    sql: "SELECT SLEEP(2), CONNECTION_ID()".to_owned(),
                    page_no: 1,
                    page_size: 10,
                    result_set_id: None,
                    single: false,
                    page_size_all: false,
                    explain: false,
                    error_continue: false,
                },
                MysqlConsoleCancellation::new(),
            )
            .await
    })
}

async fn await_query(
    task: tokio::task::JoinHandle<Result<Vec<MysqlConsoleResult>, chat2db_core::AppError>>,
    label: &str,
) -> Vec<MysqlConsoleResult> {
    tokio::time::timeout(QUERY_TIMEOUT, task)
        .await
        .unwrap_or_else(|_| panic!("{label} tunneled query timed out"))
        .unwrap_or_else(|error| panic!("{label} tunneled query task panicked: {error}"))
        .unwrap_or_else(|error| panic!("{label} tunneled query failed: {error}"))
}

fn assert_query_succeeded(results: &[MysqlConsoleResult]) {
    assert_eq!(results.len(), 1);
    assert!(results[0].success);
    assert_eq!(results[0].rows.len(), 1);
}

async fn assert_port_available(port: u16) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap_or_else(|error| {
            panic!("CHAT2DB_TEST_SSH_LOCAL_PORT {port} is unavailable: {error}")
        });
    drop(listener);
}

async fn wait_for_port_bound(port: u16) {
    tokio::time::timeout(PORT_STATE_TIMEOUT, async move {
        loop {
            match TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await {
                Ok(stream) => {
                    drop(stream);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) => panic!("could not inspect SSH tunnel port {port}: {error}"),
            }
        }
    })
    .await
    .expect("SSH tunnel did not bind its configured local port");
}

async fn wait_for_port_released(port: u16) {
    tokio::time::timeout(PORT_STATE_TIMEOUT, async move {
        loop {
            match TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
                Ok(listener) => {
                    drop(listener);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                    tokio::task::yield_now().await;
                }
                Err(error) => panic!("could not inspect released SSH tunnel port {port}: {error}"),
            }
        }
    })
    .await
    .expect("SSH tunnel did not release its configured local port");
}

fn ssh_authentication() -> SshAuthentication {
    let password = optional_env("CHAT2DB_TEST_SSH_PASSWORD");
    let private_key = optional_env("CHAT2DB_TEST_SSH_PRIVATE_KEY");
    match (password, private_key) {
        (Some(password), None) if !password.is_empty() => SshAuthentication::Password { password },
        (None, Some(key_file)) if !key_file.trim().is_empty() => SshAuthentication::PrivateKey {
            key_file,
            passphrase: optional_env("CHAT2DB_TEST_SSH_PRIVATE_KEY_PASSPHRASE"),
        },
        (Some(_), Some(_)) => panic!(
            "configure only one of CHAT2DB_TEST_SSH_PASSWORD or CHAT2DB_TEST_SSH_PRIVATE_KEY"
        ),
        _ => panic!(
            "configure a non-empty CHAT2DB_TEST_SSH_PASSWORD or CHAT2DB_TEST_SSH_PRIVATE_KEY"
        ),
    }
}

fn url_host(host: &str) -> String {
    if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

fn required_host(name: &str) -> String {
    let value = required_env(name);
    assert!(
        !value.trim().is_empty()
            && !value.chars().any(char::is_control)
            && !value.contains(['/', '?', '#']),
        "{name} is invalid"
    );
    value
}

fn required_port(name: &str) -> u16 {
    let port = required_env(name)
        .parse::<u16>()
        .unwrap_or_else(|_| panic!("{name} must be a TCP port"));
    assert_ne!(port, 0, "{name} cannot be zero");
    port
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be configured"))
}

fn optional_env(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if value.trim().is_empty() => None,
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => panic!("{name} must be valid UTF-8"),
    }
}

fn assert_java_dormant(application: &Application) {
    let engine = application
        .health()
        .components
        .into_iter()
        .find(|component| component.id == "database-engine")
        .expect("database engine health is present");
    assert_eq!(engine.state, ComponentState::Ready);
    assert_eq!(engine.detail, "Available on demand; Java is not running");
}
