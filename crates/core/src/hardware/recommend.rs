use std::sync::LazyLock;

use mimi_protocol::{
    CatalogModel, DetectedServer, GpuKind, GpuVendor, HardwareInfo, HardwareTier, InstalledModel,
    Locality, ModelRef, Recommendations,
};
use serde::Deserialize;

use super::GIB;

#[derive(Deserialize)]
struct Catalog {
    models: Vec<CatalogEntry>,
}

#[derive(Deserialize)]
struct CatalogEntry {
    id: String,
    name: String,
    description: String,
    download_gb: f64,
    moe: bool,
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("catalog.json")).expect("catalog.json is valid")
});

/// Dense models above this get painfully slow without a GPU or unified memory.
const CPU_DENSE_LIMIT: u64 = 8 * GIB;

/// How much memory models can use here, and whether a GPU accelerates them.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub bytes: u64,
    pub accelerated: bool,
}

pub fn budget(hw: &HardwareInfo) -> Budget {
    let best_vram = hw
        .gpus
        .iter()
        .filter(|g| g.kind == GpuKind::Discrete)
        .filter_map(|g| g.vram_bytes)
        .max();
    if let Some(vram) = best_vram.filter(|v| *v >= 4 * GIB) {
        // Leave room for the display and the runtime.
        return Budget {
            bytes: vram.saturating_sub(GIB),
            accelerated: true,
        };
    }
    if hw.gpus.iter().any(|g| g.vendor == GpuVendor::Apple) {
        // macOS lets the GPU use roughly two thirds of unified memory.
        return Budget {
            bytes: hw.total_memory_bytes / 100 * 65,
            accelerated: true,
        };
    }
    // Leave the rest for the OS and the user's other apps.
    Budget {
        bytes: hw.total_memory_bytes / 100 * 60,
        accelerated: false,
    }
}

fn fits(size: u64, moe: bool, b: Budget) -> bool {
    // Weights plus runtime overhead and a conversation's worth of context.
    let needed = size / 10 * 11 + GIB / 2;
    needed <= b.bytes && (b.accelerated || moe || needed <= CPU_DENSE_LIMIT)
}

fn tier(b: Budget) -> HardwareTier {
    let usable = if b.accelerated {
        b.bytes
    } else {
        b.bytes.min(CPU_DENSE_LIMIT)
    };
    match usable {
        n if n < 3 * GIB => HardwareTier::Minimal,
        n if n < 6 * GIB => HardwareTier::Light,
        n if n < 12 * GIB => HardwareTier::Standard,
        n if n < 24 * GIB => HardwareTier::Strong,
        _ => HardwareTier::Workstation,
    }
}

/// A model offered by a configured, non-cloud provider.
pub struct AvailableModel {
    pub model: ModelRef,
    pub provider_name: String,
    pub locality: Locality,
    pub size_bytes: Option<u64>,
}

pub fn recommend(
    hardware: HardwareInfo,
    available: Vec<AvailableModel>,
    detected_servers: Vec<DetectedServer>,
) -> Recommendations {
    let b = budget(&hardware);
    let tier = tier(b);
    let is_moe = |id: &str| CATALOG.models.iter().any(|m| m.id == id && m.moe);

    let mut installed: Vec<InstalledModel> = available
        .into_iter()
        .map(|a| InstalledModel {
            // Models on another machine don't use this one's memory.
            fits: a.locality == Locality::Network
                || a.size_bytes
                    .is_none_or(|s| fits(s, is_moe(&a.model.model), b)),
            size_bytes: a.size_bytes,
            model: a.model,
            provider_name: a.provider_name,
        })
        .collect();
    installed.sort_by(|x, y| y.fits.cmp(&x.fits).then(y.size_bytes.cmp(&x.size_bytes)));

    let mut suggested: Vec<&CatalogEntry> = CATALOG
        .models
        .iter()
        .filter(|m| fits(gb(m.download_gb), m.moe, b))
        .filter(|m| !installed.iter().any(|i| i.model.model == m.id))
        .collect();
    suggested.sort_by(|x, y| y.download_gb.total_cmp(&x.download_gb));
    let suggested = suggested
        .into_iter()
        .take(3)
        .map(|m| CatalogModel {
            id: m.id.clone(),
            name: m.name.clone(),
            description: m.description.clone(),
            download_bytes: gb(m.download_gb),
        })
        .collect();

    Recommendations {
        summary: summary(&hardware, tier),
        prefer_cloud: tier == HardwareTier::Minimal,
        model_budget_bytes: b.bytes,
        hardware,
        tier,
        installed,
        suggested,
        detected_servers,
        download_provider_id: None,
    }
}

pub fn catalog_models() -> Vec<CatalogModel> {
    CATALOG
        .models
        .iter()
        .map(|m| CatalogModel {
            id: m.id.clone(),
            name: m.name.clone(),
            description: m.description.clone(),
            download_bytes: gb(m.download_gb),
        })
        .collect()
}

fn gb(n: f64) -> u64 {
    (n * 1e9) as u64
}

fn summary(hw: &HardwareInfo, tier: HardwareTier) -> String {
    let memory = format!(
        "{} GB of memory",
        (hw.total_memory_bytes as f64 / GIB as f64).round()
    );
    let gpu = hw
        .gpus
        .iter()
        .find(|g| g.kind == GpuKind::Discrete)
        .or_else(|| hw.gpus.first());
    let machine = match gpu {
        Some(g) if g.vendor == GpuVendor::Apple => format!("{} with {memory}", g.name),
        Some(g) if g.kind == GpuKind::Discrete => match g.vram_bytes {
            Some(v) => format!(
                "{memory} and a {} with {} GB of video memory",
                g.name,
                (v as f64 / GIB as f64).round()
            ),
            None => format!("{memory} and a {}", g.name),
        },
        _ => format!("{memory} and no dedicated graphics card"),
    };
    let verdict = match tier {
        HardwareTier::Minimal => {
            "Local models will be slow and give basic answers here, so a cloud model is the practical choice. You can still use a small local model when privacy matters most."
        }
        HardwareTier::Light => {
            "It can run small models locally. They handle everyday questions well but struggle with complex tasks."
        }
        HardwareTier::Standard => {
            "It can run mid-sized models locally, which are good for most everyday tasks."
        }
        HardwareTier::Strong => {
            "It can run large models locally, good enough for most tasks without the cloud."
        }
        HardwareTier::Workstation => {
            "It can run very large models locally, close to what cloud services offer."
        }
    };
    format!("This computer has {machine}. {verdict}")
}

#[cfg(test)]
mod tests {
    use mimi_protocol::GpuInfo;

    use super::*;

    fn hw(ram_gib: u64, gpus: Vec<GpuInfo>) -> HardwareInfo {
        HardwareInfo {
            os: "test".into(),
            arch: "x86_64".into(),
            cpu_name: "cpu".into(),
            cpu_cores: 8,
            total_memory_bytes: ram_gib * GIB,
            available_memory_bytes: ram_gib * GIB / 2,
            gpus,
        }
    }

    fn gpu(vendor: GpuVendor, kind: GpuKind, vram_gib: Option<u64>) -> GpuInfo {
        GpuInfo {
            name: "gpu".into(),
            vendor,
            kind,
            vram_bytes: vram_gib.map(|v| v * GIB),
        }
    }

    fn ids(r: &Recommendations) -> Vec<&str> {
        r.suggested.iter().map(|m| m.id.as_str()).collect()
    }

    #[test]
    fn potato_prefers_cloud() {
        let r = recommend(hw(4, vec![]), vec![], vec![]);
        assert_eq!(r.tier, HardwareTier::Minimal);
        assert!(r.prefer_cloud);
        assert!(ids(&r).iter().all(|id| *id == "qwen3:1.7b"));
    }

    #[test]
    fn laptop_without_gpu_is_capped_to_small_dense_models() {
        let r = recommend(
            hw(32, vec![gpu(GpuVendor::Amd, GpuKind::Integrated, None)]),
            vec![],
            vec![],
        );
        assert_eq!(r.tier, HardwareTier::Standard);
        assert!(!r.prefer_cloud);
        // ~19 GiB budget: dense models up to the CPU limit, plus the MoE that fits.
        assert_eq!(ids(&r), ["gpt-oss:20b", "qwen3:8b", "gemma3:4b"]);
    }

    #[test]
    fn big_gpu_gets_big_models() {
        let r = recommend(
            hw(
                64,
                vec![gpu(GpuVendor::Nvidia, GpuKind::Discrete, Some(24))],
            ),
            vec![],
            vec![],
        );
        assert_eq!(r.tier, HardwareTier::Strong);
        assert_eq!(ids(&r)[0], "qwen3:32b");
    }

    #[test]
    fn apple_silicon_uses_unified_memory() {
        let r = recommend(
            hw(128, vec![gpu(GpuVendor::Apple, GpuKind::Integrated, None)]),
            vec![],
            vec![],
        );
        assert_eq!(r.tier, HardwareTier::Workstation);
        assert_eq!(ids(&r)[0], "gpt-oss:120b");
    }

    #[test]
    fn installed_models_rank_fitting_first_and_are_not_resuggested() {
        let provider_id = uuid::Uuid::now_v7();
        let model = |name: &str, gb_: f64, locality| AvailableModel {
            model: ModelRef {
                provider_id,
                model: name.into(),
            },
            provider_name: "Ollama".into(),
            locality,
            size_bytes: Some(gb(gb_)),
        };
        let r = recommend(
            hw(16, vec![]),
            vec![
                model("llama3.3:70b", 43.0, Locality::Device),
                model("qwen3:8b", 5.2, Locality::Device),
                model("huge-on-server", 90.0, Locality::Network),
            ],
            vec![],
        );
        let installed: Vec<_> = r
            .installed
            .iter()
            .map(|i| (i.model.model.as_str(), i.fits))
            .collect();
        assert_eq!(
            installed,
            [
                ("huge-on-server", true),
                ("qwen3:8b", true),
                ("llama3.3:70b", false)
            ]
        );
        assert!(!ids(&r).contains(&"qwen3:8b"));
    }
}
