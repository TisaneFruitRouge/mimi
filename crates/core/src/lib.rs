//! The hearth daemon: owns the agent loop, memory, models and tools, and serves them to
//! clients over an authenticated local API.

use std::time::Instant;

use hearth_protocol::{KeyStorage, Paths};

pub mod api;
pub mod chat;
pub mod connections;
pub mod daemon;
pub mod db;
pub mod events;
mod fsutil;
pub mod hardware;
pub mod integrations;
pub mod keys;
pub mod memory;
pub mod people;
pub mod providers;
pub mod settings;
pub mod tools;
pub mod web;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct AppState {
    pub paths: Paths,
    pub token: String,
    pub started: Instant,
    pub db: db::Db,
    pub key_storage: KeyStorage,
    pub events: events::EventBus,
    /// Shared HTTP client for talking to model providers.
    pub http: reqwest::Client,
    pub generations: chat::Generations,
    /// The port actually bound, for login links and the Host/Origin checks.
    pub port: u16,
    pub login_codes: web::LoginCodes,
    pub pulls: providers::pull::Pulls,
    /// Where each reply's tools come from.
    pub tool_sources: tools::ToolSources,
    /// Approval cards waiting for the user.
    pub approvals: tools::Approvals,
    pub connections: connections::Connections,
    /// Conversations waiting to be learned from once they go quiet.
    pub learner: memory::learn::Learner,
    /// The people directory: contact sources and the sync trigger.
    pub people: people::People,
}

impl AppState {
    #[cfg(test)]
    pub fn for_tests(token: &str) -> Self {
        Self::for_tests_on(token, 7437)
    }

    #[cfg(test)]
    pub fn for_tests_on(token: &str, port: u16) -> Self {
        Self {
            paths: Paths {
                data_dir: "/nonexistent".into(),
                config_dir: "/nonexistent".into(),
            },
            token: token.to_owned(),
            started: Instant::now(),
            db: db::Db::open_in_memory().unwrap(),
            key_storage: KeyStorage::File,
            events: events::EventBus::new(),
            http: http_client(),
            generations: Default::default(),
            port,
            login_codes: Default::default(),
            pulls: Default::default(),
            tool_sources: Default::default(),
            approvals: Default::default(),
            connections: Default::default(),
            learner: Default::default(),
            people: Default::default(),
        }
    }
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        // Local models can take minutes to load before the first token.
        .read_timeout(std::time::Duration::from_secs(300))
        .user_agent(concat!("hearth/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("building the HTTP client")
}
