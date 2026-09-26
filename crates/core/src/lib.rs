//! The hearth daemon: owns the agent loop, memory, models and tools, and serves them to
//! clients over an authenticated local API.

use std::time::Instant;

use hearth_protocol::{KeyStorage, Paths};

pub mod api;
pub mod chat;
pub mod daemon;
pub mod db;
pub mod events;
mod fsutil;
pub mod hardware;
pub mod keys;
pub mod providers;
pub mod settings;

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
}

impl AppState {
    #[cfg(test)]
    pub fn for_tests(token: &str) -> Self {
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
