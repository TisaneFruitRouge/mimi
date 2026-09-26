//! Typed client for the hearth daemon's local API. Every frontend (desktop, CLI, TUI)
//! goes through this crate, which keeps them at feature parity by construction.

use hearth_protocol::{API_PREFIX, Discovery, Health, Paths, Status};
use serde::de::DeserializeOwned;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the hearth daemon is not running")]
    NotRunning,
    #[error("the daemon rejected our credentials")]
    Unauthorized,
    #[error(transparent)]
    Paths(#[from] hearth_protocol::paths::NoHomeDir),
    #[error("request to the daemon failed: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Client {
    /// Connects to the daemon running on this machine for the current user.
    pub fn local() -> Result<Self, Error> {
        Self::from_paths(&Paths::resolve()?)
    }

    pub fn from_paths(paths: &Paths) -> Result<Self, Error> {
        let raw = std::fs::read(paths.discovery_file()).map_err(|_| Error::NotRunning)?;
        let discovery: Discovery = serde_json::from_slice(&raw).map_err(|_| Error::NotRunning)?;
        Ok(Self {
            http: reqwest::Client::new(),
            base: format!("http://127.0.0.1:{}", discovery.port),
            token: discovery.token,
        })
    }

    pub async fn health(&self) -> Result<Health, Error> {
        self.get(&format!("{}/health", self.base), false).await
    }

    pub async fn status(&self) -> Result<Status, Error> {
        self.get(&format!("{}{API_PREFIX}/status", self.base), true)
            .await
    }

    async fn get<T: DeserializeOwned>(&self, url: &str, auth: bool) -> Result<T, Error> {
        let mut req = self.http.get(url);
        if auth {
            req = req.bearer_auth(&self.token);
        }
        let res = req.send().await.map_err(|e| {
            if e.is_connect() {
                Error::NotRunning
            } else {
                Error::Http(e)
            }
        })?;
        if res.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::Unauthorized);
        }
        Ok(res.error_for_status()?.json().await?)
    }
}
