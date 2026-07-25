use std::{env, ffi::OsString, path::PathBuf};

use chat2db_local::LocalClient;
use chat2db_mcp::McpServer;
use clap::Parser;
use rmcp::ServiceExt as _;
use tracing_subscriber::{
    Layer as _, filter::LevelFilter, filter::Targets, layer::SubscriberExt as _,
    util::SubscriberInitExt as _,
};

const DATA_DIR_ENV: &str = "CHAT2DB_DATA_DIR";
const LOG_LEVEL_ENV: &str = "CHAT2DB_MCP_LOG";

#[derive(Debug, Parser)]
#[command(name = "chat2db-mcp", version, about = "Chat2DB Rust MCP server")]
struct Cli {
    /// Override the per-user `Chat2DB` data directory used for local attachment.
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let application_log_level = application_log_level(env::var_os(LOG_LEVEL_ENV))?;
    let log_filter = Targets::new()
        .with_default(LevelFilter::WARN)
        .with_target(env!("CARGO_CRATE_NAME"), application_log_level);
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(log_filter),
        )
        .try_init()?;

    let cli = Cli::parse();
    let server = McpServer::new(local_client(cli.data_dir)?);
    server
        .serve(rmcp::transport::stdio())
        .await?
        .waiting()
        .await?;
    Ok(())
}

fn local_client(
    data_dir: Option<PathBuf>,
) -> Result<LocalClient, Box<dyn std::error::Error + Send + Sync>> {
    match attachment_data_dir(data_dir, env::var_os(DATA_DIR_ENV))? {
        Some(path) => Ok(LocalClient::new(path)),
        None => Ok(LocalClient::discover_default()?),
    }
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

fn application_log_level(environment: Option<OsString>) -> Result<LevelFilter, String> {
    let Some(value) = environment else {
        return Ok(LevelFilter::WARN);
    };
    let value = value
        .into_string()
        .map_err(|_| format!("{LOG_LEVEL_ENV} must contain valid UTF-8"))?;
    value
        .parse()
        .map_err(|_| format!("{LOG_LEVEL_ENV} must be a valid tracing level"))
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::PathBuf};

    use clap::Parser as _;

    use tracing_subscriber::filter::LevelFilter;

    use super::{Cli, application_log_level, attachment_data_dir};

    #[test]
    fn parses_data_directory_override() {
        let cli = Cli::try_parse_from(["chat2db-mcp", "--data-dir", "/tmp/chat2db-test"])
            .expect("arguments parse");
        assert_eq!(cli.data_dir, Some(PathBuf::from("/tmp/chat2db-test")));
    }

    #[test]
    fn rejects_empty_data_directory_sources() {
        assert!(attachment_data_dir(Some(PathBuf::new()), None).is_err());
        assert!(attachment_data_dir(None, Some(OsString::new())).is_err());
    }

    #[test]
    fn accepts_only_a_single_application_log_level() {
        assert_eq!(application_log_level(None).unwrap(), LevelFilter::WARN);
        assert_eq!(
            application_log_level(Some(OsString::from("debug"))).unwrap(),
            LevelFilter::DEBUG
        );
        assert!(application_log_level(Some(OsString::from("rmcp=debug"))).is_err());
    }
}
