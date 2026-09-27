//! Typed client for the mimi daemon's local API. Every frontend (desktop, CLI, TUI)
//! goes through this crate, which keeps them at feature parity by construction.

use futures::StreamExt;
use futures::stream::BoxStream;
use mimi_protocol::{
    API_PREFIX, ApiError, Connection, ConnectionSetup, Conversation, ConversationDetail, Discovery,
    Event, HardwareInfo, Health, ModelInfo, NewConversation, NewProvider, Paths, ProbeRequest,
    ProbeResult, Provider, ProviderPreset, Recommendations, SendMessage, SendMessageResult,
    Settings, Status, WebLoginLink,
};
pub use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio_tungstenite::tungstenite::{self, Message, client::IntoClientRequest};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the mimi daemon is not running")]
    NotRunning,
    #[error("the daemon rejected our credentials")]
    Unauthorized,
    /// The daemon answered with an error it explained.
    #[error("{message}")]
    Api {
        status: u16,
        code: String,
        message: String,
    },
    #[error(transparent)]
    Paths(#[from] mimi_protocol::paths::NoHomeDir),
    #[error("request to the daemon failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("event stream failed: {0}")]
    Events(String),
    #[error("unexpected response from the daemon: {0}")]
    Decode(String),
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    port: u16,
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
            port: discovery.port,
            token: discovery.token,
        })
    }

    pub async fn health(&self) -> Result<Health, Error> {
        let res = self
            .http
            .get(format!("http://127.0.0.1:{}/health", self.port))
            .send()
            .await
            .map_err(connect_error)?;
        decode(res).await
    }

    pub async fn status(&self) -> Result<Status, Error> {
        self.send(Method::GET, "/status", None::<()>).await
    }

    pub async fn settings(&self) -> Result<Settings, Error> {
        self.send(Method::GET, "/settings", None::<()>).await
    }

    pub async fn put_settings(&self, settings: &Settings) -> Result<Settings, Error> {
        self.send(Method::PUT, "/settings", Some(settings)).await
    }

    pub async fn providers(&self) -> Result<Vec<Provider>, Error> {
        self.send(Method::GET, "/providers", None::<()>).await
    }

    pub async fn provider_presets(&self) -> Result<Vec<ProviderPreset>, Error> {
        self.send(Method::GET, "/providers/presets", None::<()>)
            .await
    }

    pub async fn probe_provider(&self, req: &ProbeRequest) -> Result<ProbeResult, Error> {
        self.send(Method::POST, "/providers/probe", Some(req)).await
    }

    pub async fn add_provider(&self, new: &NewProvider) -> Result<Provider, Error> {
        self.send(Method::POST, "/providers", Some(new)).await
    }

    pub async fn remove_provider(&self, id: Uuid) -> Result<(), Error> {
        self.send(Method::DELETE, &format!("/providers/{id}"), None::<()>)
            .await
    }

    pub async fn models(&self, provider_id: Uuid) -> Result<Vec<ModelInfo>, Error> {
        self.send(
            Method::GET,
            &format!("/providers/{provider_id}/models"),
            None::<()>,
        )
        .await
    }

    pub async fn hardware(&self) -> Result<HardwareInfo, Error> {
        self.send(Method::GET, "/hardware", None::<()>).await
    }

    pub async fn recommendations(&self) -> Result<Recommendations, Error> {
        self.send(Method::GET, "/recommendations", None::<()>).await
    }

    pub async fn conversations(&self) -> Result<Vec<Conversation>, Error> {
        self.send(Method::GET, "/conversations", None::<()>).await
    }

    pub async fn create_conversation(&self, new: &NewConversation) -> Result<Conversation, Error> {
        self.send(Method::POST, "/conversations", Some(new)).await
    }

    pub async fn conversation(&self, id: Uuid) -> Result<ConversationDetail, Error> {
        self.send(Method::GET, &format!("/conversations/{id}"), None::<()>)
            .await
    }

    pub async fn delete_conversation(&self, id: Uuid) -> Result<(), Error> {
        self.send(Method::DELETE, &format!("/conversations/{id}"), None::<()>)
            .await
    }

    pub async fn send_message(
        &self,
        conversation_id: Uuid,
        message: &SendMessage,
    ) -> Result<SendMessageResult, Error> {
        self.send(
            Method::POST,
            &format!("/conversations/{conversation_id}/messages"),
            Some(message),
        )
        .await
    }

    pub async fn cancel_reply(&self, conversation_id: Uuid) -> Result<(), Error> {
        self.send(
            Method::POST,
            &format!("/conversations/{conversation_id}/cancel"),
            None::<()>,
        )
        .await
    }

    /// A single-use link that signs a browser in to the web interface.
    pub async fn web_login_link(&self) -> Result<WebLoginLink, Error> {
        self.send(Method::POST, "/web/login-link", None::<()>).await
    }

    /// Approves an action waiting for the user, optionally with edited arguments.
    pub async fn approve_action(
        &self,
        id: Uuid,
        arguments: Option<serde_json::Value>,
    ) -> Result<(), Error> {
        self.send(
            Method::POST,
            &format!("/actions/{id}/approve"),
            Some(mimi_protocol::ApproveAction { arguments }),
        )
        .await
    }

    pub async fn reject_action(&self, id: Uuid) -> Result<(), Error> {
        self.send(Method::POST, &format!("/actions/{id}/reject"), None::<()>)
            .await
    }

    pub async fn connections(&self) -> Result<Vec<Connection>, Error> {
        self.send(Method::GET, "/connections", None::<()>).await
    }

    pub async fn connect(&self, setup: &ConnectionSetup) -> Result<Connection, Error> {
        self.send(Method::POST, "/connections", Some(setup)).await
    }

    pub async fn disconnect(&self, id: Uuid) -> Result<(), Error> {
        self.send(Method::DELETE, &format!("/connections/{id}"), None::<()>)
            .await
    }

    /// Untyped request to any `/v1` route. `path` excludes the `/v1` prefix.
    pub async fn request_json(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, Error> {
        self.send(method, path, body).await
    }

    /// A `/v1` GET whose answer is a file (e.g. an email attachment), as bytes.
    pub async fn get_bytes(&self, path: &str) -> Result<Vec<u8>, Error> {
        let res = self
            .http
            .get(format!("http://127.0.0.1:{}{API_PREFIX}{path}", self.port))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(connect_error)?;
        if !res.status().is_success() {
            return Err(error_of(res).await);
        }
        Ok(res.bytes().await?.to_vec())
    }

    /// Subscribes to daemon events. The stream ends when the daemon goes away.
    /// Events this client version doesn't know are skipped.
    pub async fn events(&self) -> Result<BoxStream<'static, Result<Event, Error>>, Error> {
        let mut req = format!("ws://127.0.0.1:{}{API_PREFIX}/events", self.port)
            .into_client_request()
            .map_err(|e| Error::Events(e.to_string()))?;
        req.headers_mut().insert(
            "authorization",
            format!("Bearer {}", self.token)
                .parse()
                .map_err(|_| Error::Unauthorized)?,
        );
        let (ws, _) = tokio_tungstenite::connect_async(req)
            .await
            .map_err(|e| match e {
                tungstenite::Error::Io(_) => Error::NotRunning,
                tungstenite::Error::Http(res) if res.status() == 401 => Error::Unauthorized,
                other => Error::Events(other.to_string()),
            })?;
        Ok(ws
            .filter_map(|msg| async move {
                match msg {
                    Ok(Message::Text(text)) => serde_json::from_str(&text).ok().map(Ok),
                    Ok(_) => None,
                    Err(e) => Some(Err(Error::Events(e.to_string()))),
                }
            })
            .boxed())
    }

    async fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<impl Serialize>,
    ) -> Result<T, Error> {
        let mut req = self
            .http
            .request(
                method,
                format!("http://127.0.0.1:{}{API_PREFIX}{path}", self.port),
            )
            .bearer_auth(&self.token);
        if let Some(body) = body {
            req = req.json(&body);
        }
        decode(req.send().await.map_err(connect_error)?).await
    }
}

fn connect_error(e: reqwest::Error) -> Error {
    if e.is_connect() {
        Error::NotRunning
    } else {
        Error::Http(e)
    }
}

/// The error an unsuccessful response carries.
async fn error_of(res: reqwest::Response) -> Error {
    let status = res.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Error::Unauthorized;
    }
    let body: Option<ApiError> = res.json().await.ok();
    let (code, message) = match body {
        Some(e) => (e.code, e.message),
        None => (
            "unknown".to_owned(),
            format!("the daemon returned {status}"),
        ),
    };
    Error::Api {
        status: status.as_u16(),
        code,
        message,
    }
}

async fn decode<T: DeserializeOwned>(res: reqwest::Response) -> Result<T, Error> {
    if !res.status().is_success() {
        return Err(error_of(res).await);
    }
    // `()`-returning routes send an empty body.
    let bytes = res.bytes().await?;
    serde_json::from_slice(if bytes.is_empty() { b"null" } else { &bytes })
        .map_err(|e| Error::Decode(e.to_string()))
}
