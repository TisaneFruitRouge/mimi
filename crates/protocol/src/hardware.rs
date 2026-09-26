use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ModelRef;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HardwareInfo {
    pub os: String,
    pub arch: String,
    pub cpu_name: String,
    pub cpu_cores: u32,
    #[ts(type = "number")]
    pub total_memory_bytes: u64,
    #[ts(type = "number")]
    pub available_memory_bytes: u64,
    pub gpus: Vec<GpuInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: GpuVendor,
    pub kind: GpuKind,
    /// Dedicated video memory, when known. Integrated GPUs share system memory.
    #[ts(type = "number | null")]
    pub vram_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum GpuKind {
    Discrete,
    Integrated,
}

/// How capable this machine is at running models locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HardwareTier {
    /// Local models will be slow and basic; a cloud model is the practical choice.
    Minimal,
    Light,
    Standard,
    Strong,
    Workstation,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Recommendations {
    pub hardware: HardwareInfo,
    pub tier: HardwareTier,
    /// Roughly how large a model (weights plus working memory) runs comfortably here.
    #[ts(type = "number")]
    pub model_budget_bytes: u64,
    /// Plain-language explanation of what this machine can do.
    pub summary: String,
    /// Whether a cloud model is the better default on this machine.
    pub prefer_cloud: bool,
    /// Models already available from private providers, best first.
    pub installed: Vec<InstalledModel>,
    /// Models worth downloading for this machine, best first.
    pub suggested: Vec<CatalogModel>,
    /// Local model servers found running on this computer.
    pub detected_servers: Vec<DetectedServer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InstalledModel {
    pub model: ModelRef,
    pub provider_name: String,
    #[ts(type = "number | null")]
    pub size_bytes: Option<u64>,
    /// Whether it should run comfortably. Always true for models on another machine.
    pub fits: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CatalogModel {
    /// Ollama tag, e.g. `qwen3:8b`.
    pub id: String,
    pub name: String,
    pub description: String,
    #[ts(type = "number")]
    pub download_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DetectedServer {
    pub preset_id: String,
    pub name: String,
    pub base_url: String,
    pub model_count: u32,
    /// A provider with this address is already configured.
    pub already_added: bool,
}
