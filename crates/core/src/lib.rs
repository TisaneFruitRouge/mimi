//! The hearth daemon: owns the agent loop, memory, models and tools, and serves them to
//! clients over an authenticated local API.

use std::time::Instant;

use hearth_protocol::{KeyStorage, Paths};

pub mod api;
pub mod daemon;
pub mod db;
pub mod events;
mod fsutil;
pub mod keys;
pub mod settings;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct AppState {
    pub paths: Paths,
    pub token: String,
    pub started: Instant,
    pub db: db::Db,
    pub key_storage: KeyStorage,
    pub events: events::EventBus,
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
        }
    }
}
