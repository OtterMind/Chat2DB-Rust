//! Headless `Chat2DB` delivery mode and command-line client.

use std::{
    env,
    ffi::OsString,
    path::PathBuf,
    process::{Child, Command as ProcessCommand, Stdio},
    time::Duration,
};

use chat2db_contract::{
    DatabaseWriteState, ExecuteDatabaseWriteRequest, QueryLimits, ResultPageRequest,
    StartQueryRequest,
};
use chat2db_core::{Application, RuntimeHost};
use chat2db_local::{LocalClient, LocalServer};
use chat2db_runtime::{DATA_DIR_ENV, RuntimeOptions, runtime_config_from_environment};
use clap::{Parser, Subcommand, ValueEnum};
use tokio::time::Instant;
use tracing_subscriber::EnvFilter;

const RUNTIME_IDLE_SECONDS_ENV: &str = "CHAT2DB_CLI_RUNTIME_IDLE_SECONDS";
const DEFAULT_RUNTIME_IDLE_SECONDS: u64 = 60;
const RUNTIME_START_TIMEOUT: Duration = Duration::from_secs(15);
const COMPETING_RUNTIME_GRACE: Duration = Duration::from_secs(2);
const RUNTIME_PROBE_INTERVAL: Duration = Duration::from_millis(50);
const IDLE_PROBE_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Parser)]
#[command(name = "chat2db", version, about = "Chat2DB Rust command line")]
struct Cli {
    /// Override the per-user `Chat2DB` data directory.
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    /// Attach only, or automatically start a headless Rust host when needed.
    #[arg(long, global = true, value_enum, default_value_t = HostMode::Auto)]
    host: HostMode,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum HostMode {
    Auto,
    Attach,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print health from the local product host.
    Status,
    /// List secret-free datasource metadata.
    Datasources,
    /// Start, inspect, or cancel a forced-read-only database query.
    Query {
        #[command(subcommand)]
        command: QueryCommand,
    },
    /// Execute one explicitly confirmed database write statement.
    Write {
        #[command(subcommand)]
        command: WriteCommand,
    },
    /// Read one bounded page from a retained query result.
    Result {
        result_id: String,
        #[arg(long, default_value_t = 0)]
        offset: u64,
        #[arg(long, default_value_t = 100)]
        max_rows: u32,
        #[arg(long, default_value_t = 262_144)]
        max_bytes: u64,
    },
    /// Run the shared Rust core without Tauri, `WebKit`, or an HTTP listener.
    Runtime {
        #[command(subcommand)]
        command: RuntimeCommand,
    },
}

#[derive(Debug, Subcommand)]
enum RuntimeCommand {
    /// Serve the owner-only local attachment until interrupted or idle.
    Serve {
        #[arg(long, default_value_t = DEFAULT_RUNTIME_IDLE_SECONDS)]
        idle_seconds: u64,
        #[arg(long, hide = true)]
        background_child: bool,
    },
}

#[derive(Debug, Subcommand)]
enum QueryCommand {
    /// Start a forced-read-only query and return its operation id.
    Start {
        #[arg(long)]
        datasource_id: String,
        #[arg(long)]
        sql: String,
        #[arg(long, default_value_t = 10_000)]
        max_rows: u64,
        #[arg(long, default_value_t = 16_777_216)]
        max_result_bytes: u64,
        #[arg(long, default_value_t = 900)]
        result_ttl_seconds: u32,
    },
    /// Read the current state of a query operation.
    Status { operation_id: String },
    /// Request idempotent cancellation of a query operation.
    Cancel { operation_id: String },
}

#[derive(Debug, Subcommand)]
enum WriteCommand {
    /// Execute exactly one write. Only `not_started` is safe to retry after correction.
    Execute {
        #[arg(long)]
        datasource_id: String,
        #[arg(long)]
        sql: String,
        /// Explicitly confirm that this statement may change the database.
        #[arg(long)]
        confirm_write: bool,
    },
}

/// Parses process arguments and runs either the CLI client or headless host.
///
/// # Errors
///
/// Returns command-line, runtime configuration, transport, or product errors.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run_cli(Cli::parse()))
}

async fn run_cli(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = attachment_data_dir(cli.data_dir, env::var_os(DATA_DIR_ENV))?;
    let client = match data_dir {
        Some(path) => LocalClient::new(path),
        None => LocalClient::discover_default()?,
    };

    match cli.command {
        Command::Runtime { command } => match command {
            RuntimeCommand::Serve {
                idle_seconds,
                background_child,
            } => serve_runtime(client, idle_seconds, background_child).await,
        },
        command => {
            if cli.host == HostMode::Auto {
                ensure_runtime(&client).await?;
            }
            execute_command(client, command).await
        }
    }
}

async fn execute_command(
    client: LocalClient,
    command: Command,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut command_succeeded = true;
    let output = match command {
        Command::Status => serde_json::to_value(client.health().await?)?,
        Command::Datasources => serde_json::to_value(client.list_datasources().await?)?,
        Command::Query { command } => match command {
            QueryCommand::Start {
                datasource_id,
                sql,
                max_rows,
                max_result_bytes,
                result_ttl_seconds,
            } => serde_json::to_value(
                client
                    .start_read_query(StartQueryRequest {
                        datasource_id,
                        sql,
                        parameters: Vec::new(),
                        limits: QueryLimits {
                            max_rows: max_rows.to_string(),
                            max_result_bytes: max_result_bytes.to_string(),
                            batch_rows: 256,
                            batch_bytes: 1024 * 1024,
                            result_ttl_seconds,
                        },
                    })
                    .await?,
            )?,
            QueryCommand::Status { operation_id } => {
                serde_json::to_value(client.operation_snapshot(operation_id).await?)?
            }
            QueryCommand::Cancel { operation_id } => {
                serde_json::to_value(client.cancel_operation(operation_id).await?)?
            }
        },
        Command::Write { command } => match command {
            WriteCommand::Execute {
                datasource_id,
                sql,
                confirm_write,
            } => {
                let result = client
                    .execute_database_write(ExecuteDatabaseWriteRequest {
                        datasource_id,
                        sql,
                        confirmed: confirm_write,
                    })
                    .await;
                command_succeeded = result.state == DatabaseWriteState::Succeeded;
                serde_json::to_value(result)?
            }
        },
        Command::Result {
            result_id,
            offset,
            max_rows,
            max_bytes,
        } => serde_json::to_value(
            client
                .result_page(
                    result_id,
                    ResultPageRequest {
                        offset: offset.to_string(),
                        max_rows: max_rows.to_string(),
                        max_bytes: max_bytes.to_string(),
                    },
                )
                .await?,
        )?,
        Command::Runtime { .. } => unreachable!("runtime commands are dispatched before attach"),
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    if !command_succeeded {
        return Err(std::io::Error::other("database write did not succeed").into());
    }
    Ok(())
}

async fn ensure_runtime(client: &LocalClient) -> Result<(), Box<dyn std::error::Error>> {
    if client.health().await.is_ok() {
        return Ok(());
    }

    let idle_seconds = runtime_idle_seconds(env::var_os(RUNTIME_IDLE_SECONDS_ENV))?;
    let mut child = spawn_runtime_process(client, idle_seconds)?;
    let deadline = Instant::now() + RUNTIME_START_TIMEOUT;
    let mut child_exit = None;
    loop {
        if client.health().await.is_ok() {
            return Ok(());
        }
        if child_exit.is_none() {
            child_exit = child.try_wait()?.map(|status| (status, Instant::now()));
        }
        if let Some((status, exited_at)) = child_exit.as_ref()
            && exited_at.elapsed() >= COMPETING_RUNTIME_GRACE
        {
            return Err(std::io::Error::other(format!(
                "headless Chat2DB runtime exited before becoming ready: {status}"
            ))
            .into());
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "headless Chat2DB runtime did not become ready within 15 seconds",
            )
            .into());
        }
        tokio::time::sleep(RUNTIME_PROBE_INTERVAL).await;
    }
}

fn spawn_runtime_process(
    client: &LocalClient,
    idle_seconds: u64,
) -> Result<Child, Box<dyn std::error::Error>> {
    let executable = env::current_exe()?;
    let mut command = ProcessCommand::new(executable);
    command
        .arg("--data-dir")
        .arg(client.data_dir())
        .arg("runtime")
        .arg("serve")
        .arg("--idle-seconds")
        .arg(idle_seconds.to_string())
        .arg("--background-child")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_background_process(&mut command);
    command.spawn().map_err(Into::into)
}

#[cfg(unix)]
fn configure_background_process(command: &mut ProcessCommand) {
    use std::os::unix::process::CommandExt as _;
    command.process_group(0);
}

#[cfg(windows)]
fn configure_background_process(command: &mut ProcessCommand) {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(any(unix, windows)))]
fn configure_background_process(_command: &mut ProcessCommand) {}

async fn serve_runtime(
    client: LocalClient,
    idle_seconds: u64,
    background_child: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if idle_seconds == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "runtime idle timeout must be greater than zero",
        )
        .into());
    }
    initialize_runtime_logging(background_child);

    let executable = env::current_exe().ok();
    let config = runtime_config_from_environment(RuntimeOptions {
        data_dir: Some(client.data_dir().to_path_buf()),
        executable: executable.as_deref(),
        resource_dir: None,
    })?;
    let mut host = RuntimeHost::open(config).await?;
    let application = host.application();
    let mut server = LocalServer::start(application.clone())?;
    let idle_timeout = Duration::from_secs(idle_seconds);

    tokio::select! {
        () = shutdown_signal() => {}
        () = wait_for_idle(&application, &server, idle_timeout) => {}
    }

    application.begin_shutdown().await;
    let local_result = server.shutdown().await;
    let host_result = host.shutdown().await;
    local_result?;
    host_result?;
    Ok(())
}

async fn wait_for_idle(application: &Application, server: &LocalServer, timeout: Duration) {
    let mut observed_revision = server.activity_revision();
    let mut idle_since = Instant::now();
    loop {
        tokio::time::sleep(IDLE_PROBE_INTERVAL.min(timeout)).await;
        let revision = server.activity_revision();
        let active_operations = application.active_operation_count().await;
        if revision != observed_revision
            || server.active_request_count() > 0
            || active_operations > 0
        {
            observed_revision = revision;
            idle_since = Instant::now();
            continue;
        }
        if idle_since.elapsed() >= timeout {
            return;
        }
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn initialize_runtime_logging(background_child: bool) {
    if background_child {
        return;
    }
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .try_init();
}

fn attachment_data_dir(
    command_line: Option<PathBuf>,
    environment: Option<OsString>,
) -> Result<Option<PathBuf>, String> {
    let selected = command_line.or_else(|| environment.map(PathBuf::from));
    if selected
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(format!("{DATA_DIR_ENV} must not be empty"));
    }
    Ok(selected)
}

fn runtime_idle_seconds(environment: Option<OsString>) -> Result<u64, String> {
    let Some(value) = environment else {
        return Ok(DEFAULT_RUNTIME_IDLE_SECONDS);
    };
    let value = value
        .into_string()
        .map_err(|_| format!("{RUNTIME_IDLE_SECONDS_ENV} must contain valid UTF-8"))?;
    let seconds = value.parse::<u64>().map_err(|_| {
        format!("{RUNTIME_IDLE_SECONDS_ENV} must be a positive integer number of seconds")
    })?;
    if seconds == 0 {
        return Err(format!(
            "{RUNTIME_IDLE_SECONDS_ENV} must be greater than zero"
        ));
    }
    Ok(seconds)
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::PathBuf, time::Duration};

    use clap::Parser as _;

    use super::{
        Cli, Command, HostMode, QueryCommand, WriteCommand, attachment_data_dir,
        runtime_idle_seconds,
    };

    #[test]
    fn defaults_to_auto_headless_host_mode() {
        let cli = Cli::try_parse_from(["chat2db", "status"]).expect("status must parse");
        assert_eq!(cli.host, HostMode::Auto);
        assert!(matches!(cli.command, Command::Status));
    }

    #[test]
    fn attach_only_mode_remains_available() {
        let cli = Cli::try_parse_from(["chat2db", "--host", "attach", "datasources"])
            .expect("attach mode must parse");
        assert_eq!(cli.host, HostMode::Attach);
    }

    #[test]
    fn parses_read_query_lifecycle_commands() {
        let start = Cli::try_parse_from([
            "chat2db",
            "query",
            "start",
            "--datasource-id",
            "datasource-1",
            "--sql",
            "select 1",
        ])
        .expect("query start must parse");
        assert!(matches!(
            start.command,
            Command::Query {
                command: QueryCommand::Start { .. }
            }
        ));

        let cancel = Cli::try_parse_from(["chat2db", "query", "cancel", "operation-1"])
            .expect("query cancel must parse");
        assert!(matches!(
            cancel.command,
            Command::Query {
                command: QueryCommand::Cancel { .. }
            }
        ));
    }

    #[test]
    fn parses_bounded_result_page() {
        let cli = Cli::try_parse_from([
            "chat2db",
            "--data-dir",
            "/tmp/chat2db-test",
            "result",
            "result-1",
            "--offset",
            "20",
            "--max-rows",
            "50",
            "--max-bytes",
            "4096",
        ])
        .expect("result page must parse");
        assert!(matches!(
            cli.command,
            Command::Result {
                offset: 20,
                max_rows: 50,
                max_bytes: 4096,
                ..
            }
        ));
    }

    #[test]
    fn parses_explicitly_confirmed_write() {
        let cli = Cli::try_parse_from([
            "chat2db",
            "write",
            "execute",
            "--datasource-id",
            "datasource-1",
            "--sql",
            "update sample set value = 1",
            "--confirm-write",
        ])
        .expect("write must parse");
        assert!(matches!(
            cli.command,
            Command::Write {
                command: WriteCommand::Execute {
                    confirm_write: true,
                    ..
                }
            }
        ));
    }

    #[test]
    fn command_line_data_directory_wins_over_environment() {
        let selected = attachment_data_dir(
            Some(PathBuf::from("/command-line")),
            Some(OsString::from("/environment")),
        )
        .expect("data directory must resolve");
        assert_eq!(selected, Some(PathBuf::from("/command-line")));
    }

    #[test]
    fn empty_data_directory_sources_are_rejected() {
        assert!(attachment_data_dir(Some(PathBuf::new()), None).is_err());
        assert!(attachment_data_dir(None, Some(OsString::new())).is_err());
    }

    #[test]
    fn runtime_idle_timeout_is_positive() {
        assert_eq!(runtime_idle_seconds(None).expect("default timeout"), 60);
        assert_eq!(
            runtime_idle_seconds(Some(OsString::from("7"))).expect("custom timeout"),
            7
        );
        assert!(runtime_idle_seconds(Some(OsString::from("0"))).is_err());
        assert!(runtime_idle_seconds(Some(OsString::from("invalid"))).is_err());
    }

    #[test]
    fn short_idle_interval_does_not_underflow() {
        assert_eq!(
            super::IDLE_PROBE_INTERVAL.min(Duration::from_millis(1)),
            Duration::from_millis(1)
        );
    }
}
