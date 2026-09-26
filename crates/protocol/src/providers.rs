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
    pub base_url: String,
    pub locality: Locality,
    pub needs_api_key: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelInfo {
    pub id: String,
    /// On-disk size, when the provider reports it (Ollama does).
    #[ts(type = "number | null")]
    pub size_bytes: Option<u64>,
}

/// Checks a provider's connection details before saving them.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProbeRequest {
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
