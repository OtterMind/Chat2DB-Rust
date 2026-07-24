use chat2db_core::Application;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "chat2db", version, about = "Chat2DB Rust command line")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the local product contract and component state.
    Status,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Status => {
            println!(
                "{}",
                serde_json::to_string_pretty(&Application::new().health())?
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command};

    #[test]
    fn parses_status_command() {
        let cli = Cli::try_parse_from(["chat2db", "status"]).expect("status must parse");
        assert!(matches!(cli.command, Command::Status));
    }
}
