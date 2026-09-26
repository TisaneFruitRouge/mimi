use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use futures::StreamExt;
use hearth_client::Client;
use hearth_protocol::{Locality, ModelRef, NewProvider, ProbeRequest, Provider};

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
    /// Manage model providers.
    #[command(subcommand)]
    Providers(ProvidersCommand),
    /// List the models a provider offers (all providers if omitted).
    Models { provider: Option<String> },
    /// Describe this computer and suggest models for it.
    Recommend,
    /// Show or change the model used for new messages.
    DefaultModel {
        /// `<provider>/<model>`, where provider is a name or id prefix.
        model: Option<String>,
    },
}

#[derive(Subcommand)]
enum ProvidersCommand {
    /// List configured providers.
    List,
    /// Show providers that can be added with one command.
    Presets,
    /// Add a provider by preset id (e.g. `ollama`) or base URL.
    Add {
        preset_or_url: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        api_key: Option<String>,
    },
    /// Remove a provider by name or id prefix.
    Remove { provider: String },
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
        Command::Providers(cmd) => providers(&client, cmd).await?,
        Command::Models { provider } => {
            let all = client.providers().await?;
            let chosen = match provider {
                Some(p) => vec![find_provider(&all, &p)?.clone()],
                None => all,
            };
            for p in chosen {
                println!("{} ({})", p.name, locality_label(p.locality));
                match client.models(p.id).await {
                    Ok(models) => {
                        for m in models {
                            match m.size_bytes {
                                Some(size) => {
                                    println!("  {:<40} {:.1} GB", m.id, size as f64 / 1e9)
                                }
                                None => println!("  {}", m.id),
                            }
                        }
                    }
                    Err(e) => println!("  unavailable: {e}"),
                }
            }
        }
        Command::Recommend => {
            let r = client.recommendations().await?;
            let hw = &r.hardware;
            println!(
                "{} · {} cores · {:.0} GB RAM",
                hw.cpu_name,
                hw.cpu_cores,
                gb(hw.total_memory_bytes)
            );
            for g in &hw.gpus {
                println!(
                    "{} ({:?}{})",
                    g.name,
                    g.kind,
                    g.vram_bytes
                        .map(|v| format!(", {:.0} GB", gb(v)))
                        .unwrap_or_default()
                );
            }
            println!("\n{}\n", r.summary);
            if !r.installed.is_empty() {
                println!("Models you already have:");
                for m in &r.installed {
                    let note = if m.fits {
                        ""
                    } else {
                        "  (too large for this computer)"
                    };
                    println!("  {}/{}{note}", m.provider_name, m.model.model);
                }
            }
            if !r.suggested.is_empty() {
                println!("Worth downloading:");
                for m in &r.suggested {
                    println!(
                        "  {:<14} {:>5.1} GB  {}",
                        m.id,
                        gb(m.download_bytes),
                        m.description
                    );
                }
            }
            for s in r.detected_servers.iter().filter(|s| !s.already_added) {
                println!(
                    "Found {} running at {}; add it with `hearth providers add {}`.",
                    s.name, s.base_url, s.preset_id
                );
            }
        }
        Command::DefaultModel { model } => {
            let mut settings = client.settings().await?;
            let providers = client.providers().await?;
            if let Some(spec) = model {
                let (provider, model) = spec
                    .split_once('/')
                    .context("expected <provider>/<model>")?;
                let provider = find_provider(&providers, provider)?;
                settings.default_model = Some(ModelRef {
                    provider_id: provider.id,
                    model: model.to_owned(),
                });
                settings = client.put_settings(&settings).await?;
            }
            match settings.default_model {
                Some(m) => {
                    let name = providers
                        .iter()
                        .find(|p| p.id == m.provider_id)
                        .map_or("?", |p| p.name.as_str());
                    println!("{name}/{}", m.model);
                }
                None => println!("no default model set"),
            }
        }
    }
    Ok(())
}

async fn providers(client: &Client, cmd: ProvidersCommand) -> anyhow::Result<()> {
    match cmd {
        ProvidersCommand::List => {
            for p in client.providers().await? {
                println!(
                    "{}  {:<20} {:<9} {}{}",
                    &p.id.to_string()[..8],
                    p.name,
                    locality_label(p.locality),
                    p.base_url,
                    if p.has_api_key { "  (api key set)" } else { "" }
                );
            }
        }
        ProvidersCommand::Presets => {
            for p in client.provider_presets().await? {
                println!("{:<12} {:<18} {}", p.id, p.name, p.description);
            }
        }
        ProvidersCommand::Add {
            preset_or_url,
            name,
            api_key,
        } => {
            let presets = client.provider_presets().await?;
            let preset = presets.iter().find(|p| p.id == preset_or_url);
            let base_url = preset.map_or(preset_or_url.clone(), |p| p.base_url.clone());
            let probe = client
                .probe_provider(&ProbeRequest {
                    base_url,
                    api_key: api_key.clone(),
                })
                .await
                .context("could not connect to the provider")?;
            let name = name
                .or_else(|| preset.map(|p| p.name.clone()))
                .unwrap_or_else(|| probe.base_url.clone());
            let provider = client
                .add_provider(&NewProvider {
                    name,
                    kind: hearth_protocol::ProviderKind::OpenaiCompatible,
                    base_url: probe.base_url,
                    api_key,
                    locality: None,
                })
                .await?;
            println!(
                "added {} ({}), {} models available",
                provider.name,
                locality_label(provider.locality),
                probe.models.len()
            );
        }
        ProvidersCommand::Remove { provider } => {
            let all = client.providers().await?;
            let p = find_provider(&all, &provider)?;
            client.remove_provider(p.id).await?;
            println!("removed {}", p.name);
        }
    }
    Ok(())
}

fn find_provider<'a>(all: &'a [Provider], query: &str) -> anyhow::Result<&'a Provider> {
    let q = query.to_lowercase();
    let matches: Vec<_> = all
        .iter()
        .filter(|p| p.name.to_lowercase() == q || p.id.to_string().starts_with(&q))
        .collect();
    match matches[..] {
        [one] => Ok(one),
        [] => bail!("no provider matches \"{query}\""),
        _ => bail!("\"{query}\" matches several providers; use the id"),
    }
}

fn gb(bytes: u64) -> f64 {
    bytes as f64 / 1e9
}

fn locality_label(l: Locality) -> &'static str {
    match l {
        Locality::Device => "device",
        Locality::Network => "network",
        Locality::Cloud => "cloud",
    }
}
