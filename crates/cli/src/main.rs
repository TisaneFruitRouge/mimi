use clap::{Parser, Subcommand};
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Status => {
            let status = Client::local()?.status().await?;
            println!("hearth {} running", status.version);
            println!("  pid       {}", status.pid);
            println!("  uptime    {}s", status.uptime_secs);
            println!("  data dir  {}", status.data_dir.display());
        }
    }
    Ok(())
}
