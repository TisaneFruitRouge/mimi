use clap::{Parser, Subcommand};
use futures::StreamExt;
use hearth_client::Client;

/// Command-line interface to your hearth assistant.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show whether the daemon is running.
    Status,
    /// Print daemon events as JSON lines until interrupted.
    Events,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = Client::local()?;
    match Cli::parse().command {
        Command::Status => {
            let status = client.status().await?;
            println!("hearth {} running", status.version);
            println!("  pid          {}", status.pid);
            println!("  uptime       {}s", status.uptime_secs);
            println!("  data dir     {}", status.data_dir.display());
            println!("  key storage  {:?}", status.key_storage);
        }
        Command::Events => {
            let mut events = client.events().await?;
            while let Some(event) = events.next().await {
                println!("{}", serde_json::to_string(&event?)?);
            }
        }
    }
    Ok(())
}
