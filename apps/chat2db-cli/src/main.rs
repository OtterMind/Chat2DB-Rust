use std::{env, ffi::OsString, path::PathBuf};

use chat2db_contract::{
    DatabaseWriteState, ExecuteDatabaseWriteRequest, QueryLimits, ResultPageRequest,
    StartQueryRequest,
};
use chat2db_local::LocalClient;
use clap::{Parser, Subcommand};

const DATA_DIR_ENV: &str = "CHAT2DB_DATA_DIR";

#[derive(Debug, Parser)]
#[command(name = "chat2db", version, about = "Chat2DB Rust command line")]
struct Cli {
    /// Override the per-user `Chat2DB` data directory used for local attachment.
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print health from the running local product host.
    Status,
    /// List secret-free datasource metadata.
    Datasources,
    /// Start, inspect, or cancel a forced-read-only database query.
    Query {
        #[command(subcommand)]
        command: QueryCommand,
    },
    /// Execute one explicitly confirmed `MySQL` write statement.
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let client = local_client(cli.data_dir)?;
    let mut command_succeeded = true;

    let output = match cli.command {
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
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    if !command_succeeded {
        return Err(std::io::Error::other("database write did not succeed").into());
    }
    Ok(())
}

fn local_client(data_dir: Option<PathBuf>) -> Result<LocalClient, Box<dyn std::error::Error>> {
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

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::PathBuf};

    use clap::Parser;

    use super::{Cli, Command, QueryCommand, WriteCommand, attachment_data_dir};

    #[test]
    fn parses_status_command() {
        let cli = Cli::try_parse_from(["chat2db", "status"]).expect("status must parse");
        assert!(matches!(cli.command, Command::Status));
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
        ])
        .expect("result page must parse");
        assert!(matches!(cli.command, Command::Result { .. }));
    }

    #[test]
    fn database_write_requires_an_explicit_confirmation_flag_value() {
        let confirmed = Cli::try_parse_from([
            "chat2db",
            "write",
            "execute",
            "--datasource-id",
            "datasource-1",
            "--sql",
            "UPDATE items SET label = 'changed' WHERE id = 1",
            "--confirm-write",
        ])
        .expect("confirmed write must parse");
        assert!(matches!(
            confirmed.command,
            Command::Write {
                command: WriteCommand::Execute {
                    confirm_write: true,
                    ..
                }
            }
        ));

        let unconfirmed = Cli::try_parse_from([
            "chat2db",
            "write",
            "execute",
            "--datasource-id",
            "datasource-1",
            "--sql",
            "DELETE FROM items WHERE id = 1",
        ])
        .expect("unconfirmed write parses so the runtime can fail closed");
        assert!(matches!(
            unconfirmed.command,
            Command::Write {
                command: WriteCommand::Execute {
                    confirm_write: false,
                    ..
                }
            }
        ));
    }

    #[test]
    fn rejects_empty_data_directory_sources() {
        assert!(attachment_data_dir(Some(PathBuf::new()), None).is_err());
        assert!(attachment_data_dir(None, Some(OsString::new())).is_err());
    }
}
