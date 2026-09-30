use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProviderKind {
    /// Anything that speaks the OpenAI chat completions API: Ollama, llama.cpp,
    /// LM Studio, vLLM, and most cloud providers.
    OpenaiCompatible,
    /// Mimi's own runtime (llama.cpp), managed by the daemon: models are downloaded into
    /// Mimi and run on this computer. Created automatically when the runtime is present.
    Builtin,
    /// Anthropic's Messages API (Claude models), with the user's own API key.
    Anthropic,
}

/// Where a provider runs, and so where your conversations go when you use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Locality {
    /// On this computer.
    Device,
    /// On a machine in your own network (LAN, Tailscale, …).
    Network,
    /// A third-party service on the internet.
    Cloud,
}

impl Locality {
    /// Whether data sent to this provider stays on machines the user controls.
    pub fn is_private(self) -> bool {
        !matches!(self, Self::Cloud)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Provider {
    pub id: Uuid,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub locality: Locality,
    /// API keys are write-only: clients only learn whether one is set.
    pub has_api_key: bool,
    #[ts(type = "number")]
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewProvider {
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: ProviderKind,
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    /// Derived from `base_url` when omitted.
    #[serde(default)]
    pub locality: Option<Locality>,
}

fn default_kind() -> ProviderKind {
    ProviderKind::OpenaiCompatible
}

/// Fields left `null` are unchanged. An empty `api_key` removes the stored key.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct ProviderUpdate {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub locality: Option<Locality>,
}

/// A well-known provider the user can add with one click.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProviderPreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub locality: Locality,
    pub needs_api_key: bool,
    /// The service's page for creating an API key, for a "Get a key" button.
    pub key_url: Option<String>,
    /// The model to suggest once connected, when the service offers it.
    pub recommended_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelInfo {
    pub id: String,
    /// The provider's own name for the model (e.g. "Claude Sonnet 5"), when it has one.
    #[serde(default)]
    pub name: Option<String>,
    /// On-disk size, when the provider reports it (Ollama does).
    #[ts(type = "number | null")]
    pub size_bytes: Option<u64>,
    /// Whether the model can use tools (calendar, mail, reminders…), when the provider
    /// says (OpenRouter and Anthropic do).
    #[serde(default)]
    pub supports_tools: Option<bool>,
    /// What the provider charges, when it publishes prices (OpenRouter does).
    #[serde(default)]
    pub price: Option<ModelPrice>,
    /// Whether the model can see photos, when the provider says (OpenRouter and
    /// Anthropic do).
    #[serde(default)]
    pub sees_images: Option<bool>,
}

/// A cloud model's price, in US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
}

/// Checks a provider's connection details before saving them.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProbeRequest {
    #[serde(default = "default_kind")]
    pub kind: ProviderKind,
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProbeResult {
    /// The address that worked. May differ from the request, e.g. with `/v1` added.
    pub base_url: String,
    pub locality: Locality,
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PullState {
    Running,
    Done,
    Failed,
    /// Stopped by the user. What was downloaded so far is kept, so starting again resumes.
    Cancelled,
}

/// Progress of a model download through a model source that supports it (the built-in
/// runtime or Ollama).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelPull {
    pub provider_id: Uuid,
    pub model: String,
    pub state: PullState,
    /// The source's own description of the current step.
    pub status: String,
    #[ts(type = "number | null")]
    pub completed_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub total_bytes: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PullRequest {
    pub model: String,
}

/// The built-in model runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuntimeStatus {
    /// Whether this installation includes the runtime.
    pub available: bool,
    pub state: RuntimeState,
    /// The model currently loaded, if any.
    pub model: Option<String>,
    /// Why it last failed to start, in plain language.
    pub error: Option<String>,
    /// Space used by downloaded models.
    #[ts(type = "number")]
    pub models_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RuntimeState {
    /// Nothing loaded; a model starts when it's first needed.
    Idle,
    /// Loading a model into memory.
    Starting,
    Ready,
    Failed,
}
