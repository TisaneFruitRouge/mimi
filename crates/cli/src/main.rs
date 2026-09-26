use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use futures::StreamExt;
use hearth_client::Client;
use std::io::{BufRead, Write};

use hearth_protocol::{
    ActionStatus, ConnectionSetup, Event, Locality, MessageStatus, ModelRef, NewConversation,
    NewProvider, ProbeRequest, Provider, SendMessage,
};

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
    /// Open the web interface in your browser.
    Open,
    /// Print daemon events as JSON lines until interrupted.
    Events,
    /// Manage model providers.
    #[command(subcommand)]
    Providers(ProvidersCommand),
    /// List the models a provider offers (all providers if omitted).
    Models { provider: Option<String> },
    /// Chat with the assistant. Without a message, starts an interactive session.
    Chat {
        message: Option<String>,
        /// Continue the most recent conversation instead of starting a new one.
        #[arg(long, short)]
        r#continue: bool,
    },
    /// List conversations, most recent first.
    Conversations,
    /// List connected accounts (calendars, Telegram).
    Connections,
    /// Connect an account.
    #[command(subcommand)]
    Connect(ConnectCommand),
    /// Remove a connection by name or id prefix.
    Disconnect { connection: String },
    /// Describe this computer and suggest models for it.
    Recommend,
    /// Show or change the model used for new messages.
    DefaultModel {
        /// `<provider>/<model>`, where provider is a name or id prefix.
        model: Option<String>,
    },
}

#[derive(Subcommand)]
enum ConnectCommand {
    /// A Google calendar, by its "Secret address in iCal format".
    Google { ics_url: String },
    /// A CalDAV account (iCloud, Fastmail, Nextcloud…). The password is read from
    /// HEARTH_CALDAV_PASSWORD.
    Caldav {
        server_url: String,
        username: String,
    },
    /// A Telegram bot token from @BotFather.
    Telegram { bot_token: String },
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
        Command::Open => {
            let link = client.web_login_link().await?;
            println!("{}", link.url);
            let opener = if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            };
            let opened = std::process::Command::new(opener)
                .arg(&link.url)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if !opened {
                eprintln!("Couldn't open a browser; open the link above within two minutes.");
            }
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
        Command::Chat {
            message,
            r#continue,
        } => chat(&client, message, r#continue).await?,
        Command::Conversations => {
            for c in client.conversations().await? {
                println!("{}  {}", &c.id.to_string()[..8], c.title);
            }
        }
        Command::Connections => {
            for c in client.connections().await? {
                println!(
                    "{}  {:<16} {:<22} {:?}  {}{}",
                    &c.id.to_string()[..8],
                    c.integration,
                    c.name,
                    c.status,
                    c.detail,
                    c.action_url
                        .map(|u| format!("\n          → {u}"))
                        .unwrap_or_default()
                );
            }
        }
        Command::Connect(cmd) => {
            let setup = match cmd {
                ConnectCommand::Google { ics_url } => ConnectionSetup::GoogleCalendar { ics_url },
                ConnectCommand::Caldav {
                    server_url,
                    username,
                } => ConnectionSetup::Caldav {
                    server_url,
                    username,
                    password: std::env::var("HEARTH_CALDAV_PASSWORD")
                        .context("set HEARTH_CALDAV_PASSWORD to the account's app password")?,
                },
                ConnectCommand::Telegram { bot_token } => ConnectionSetup::Telegram { bot_token },
            };
            let c = client.connect(&setup).await?;
            println!("connected {} ({})", c.name, c.detail);
            if let Some(url) = c.action_url {
                println!("next: open {url}");
            }
        }
        Command::Disconnect { connection } => {
            let all = client.connections().await?;
            let q = connection.to_lowercase();
            let found: Vec<_> = all
                .iter()
                .filter(|c| c.name.to_lowercase() == q || c.id.to_string().starts_with(&q))
                .collect();
            match found[..] {
                [one] => {
                    client.disconnect(one.id).await?;
                    println!("removed {}", one.name);
                }
                [] => bail!("no connection matches \"{connection}\""),
                _ => bail!("\"{connection}\" matches several connections; use the id"),
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

async fn chat(client: &Client, message: Option<String>, resume: bool) -> anyhow::Result<()> {
    let conversation = match client.conversations().await?.into_iter().next() {
        Some(latest) if resume => latest,
        _ => {
            client
                .create_conversation(&NewConversation::default())
                .await?
        }
    };
    let mut events = client.events().await?;
    let mut send = async |text: String| -> anyhow::Result<()> {
        let sent = client
            .send_message(
                conversation.id,
                &SendMessage {
                    content: text,
                    model: None,
                },
            )
            .await?;
        let id = sent.assistant_message.id;
        let mut thinking = false;
        let mut asked = std::collections::HashSet::new();
        while let Some(event) = events.next().await {
            match event? {
                Event::MessageDelta {
                    message_id,
                    content,
                    reasoning,
                    ..
                } if message_id == id => {
                    if !reasoning.is_empty() && !thinking {
                        thinking = true;
                        eprint!("\x1b[2m(thinking…)\x1b[0m ");
                    }
                    print!("{content}");
                    std::io::stdout().flush()?;
                }
                Event::MessageUpdated { message }
                    if message.id == id && message.status == MessageStatus::Streaming =>
                {
                    for a in &message.actions {
                        if a.status != ActionStatus::PendingApproval || !asked.insert(a.id) {
                            continue;
                        }
                        eprintln!("\n\x1b[1mWaiting for you:\x1b[0m {}", a.summary);
                        if let Some(args) = a.arguments.as_object() {
                            for (k, v) in args {
                                eprintln!(
                                    "  {k}: {}",
                                    v.as_str().map_or_else(|| v.to_string(), str::to_owned)
                                );
                            }
                        }
                        eprint!("Approve? [y/N] ");
                        let mut answer = String::new();
                        std::io::stdin().lock().read_line(&mut answer)?;
                        if answer.trim().eq_ignore_ascii_case("y") {
                            client.approve_action(a.id, None).await?;
                        } else {
                            client.reject_action(a.id).await?;
                        }
                    }
                }
                Event::MessageUpdated { message }
                    if message.id == id && message.status != MessageStatus::Streaming =>
                {
                    for a in message
                        .actions
                        .iter()
                        .filter(|a| a.status == ActionStatus::Done)
                    {
                        eprintln!(
                            "\x1b[2m✓ {}\x1b[0m",
                            a.result.as_deref().unwrap_or(&a.summary)
                        );
                    }
                    println!();
                    if let Some(err) = message.error {
                        eprintln!("error: {err}");
                    }
                    break;
                }
                _ => {}
            }
        }
        Ok(())
    };
    if let Some(text) = message {
        return send(text).await;
    }
    eprintln!("Chatting in \"{}\". Ctrl+D to quit.", conversation.title);
    let stdin = std::io::stdin();
    loop {
        eprint!("\x1b[1m> \x1b[0m");
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            return Ok(());
        }
        if !line.trim().is_empty() {
            send(line).await?;
        }
    }
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
    // Decimal, as download sizes are advertised.
    bytes as f64 / 1e9
}

fn locality_label(l: Locality) -> &'static str {
    match l {
        Locality::Device => "device",
        Locality::Network => "network",
        Locality::Cloud => "cloud",
    }
}
