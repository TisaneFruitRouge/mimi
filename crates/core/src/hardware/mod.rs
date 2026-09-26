//! What this machine is, and which models suit it.

use hearth_protocol::{GpuInfo, GpuKind, GpuVendor, HardwareInfo};
use sysinfo::{CpuRefreshKind, System};

pub mod recommend;

const GIB: u64 = 1024 * 1024 * 1024;

/// Blocking: reads sysfs and may run `nvidia-smi`.
pub fn detect() -> HardwareInfo {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_list(CpuRefreshKind::nothing());
    let cpu_name = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_owned())
        .unwrap_or_default();
    let arch = System::cpu_arch();
    let mut gpus = detect_gpus();
    // AMD APUs name their graphics in the CPU brand ("… w/ Radeon 780M Graphics"),
    // which beats the PCI database's codename.
    if let Some((_, igpu)) = cpu_name.split_once(" w/ ") {
        for g in gpus.iter_mut().filter(|g| g.kind == GpuKind::Integrated) {
            g.name = igpu.trim().to_owned();
        }
    }
    if cfg!(target_os = "macos") && matches!(arch.as_str(), "arm64" | "aarch64") {
        // Apple Silicon: the GPU shares the (fast, unified) system memory.
        gpus.push(GpuInfo {
            name: cpu_name.clone(),
            vendor: GpuVendor::Apple,
            kind: GpuKind::Integrated,
            vram_bytes: None,
        });
    }
    HardwareInfo {
        os: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.to_owned()),
        arch,
        cpu_name,
        cpu_cores: System::physical_core_count().unwrap_or(sys.cpus().len()) as u32,
        total_memory_bytes: sys.total_memory(),
        available_memory_bytes: sys.available_memory(),
        gpus,
    }
}

#[cfg(target_os = "linux")]
fn detect_gpus() -> Vec<GpuInfo> {
    use std::fs;

    let pci_ids = ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids"]
        .iter()
        .find_map(|p| fs::read_to_string(p).ok());
    let mut gpus = Vec::new();
    let mut saw_nvidia = false;
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return gpus;
    };
    let mut cards: Vec<_> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        // `card0`, not connectors like `card0-DP-1`.
        .filter(|n| n.starts_with("card") && n[4..].chars().all(|c| c.is_ascii_digit()))
        .collect();
    cards.sort();
    for card in cards {
        let dev = format!("/sys/class/drm/{card}/device");
        let read_hex = |f: &str| {
            fs::read_to_string(format!("{dev}/{f}"))
                .ok()
                .and_then(|s| u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        };
        let (Some(vendor_id), Some(device_id)) = (read_hex("vendor"), read_hex("device")) else {
            continue;
        };
        let vram = fs::read_to_string(format!("{dev}/mem_info_vram_total"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok());
        let name = pci_ids
            .as_deref()
            .and_then(|db| pci_device_name(db, vendor_id, device_id));
        let (vendor, kind) = match vendor_id {
            0x10de => {
                saw_nvidia = true;
                (GpuVendor::Nvidia, GpuKind::Discrete)
            }
            // APUs get a small carve-out (≤ 2 GiB) and otherwise share system memory.
            0x1002 if vram.is_some_and(|v| v > 2 * GIB) => (GpuVendor::Amd, GpuKind::Discrete),
            0x1002 => (GpuVendor::Amd, GpuKind::Integrated),
            0x8086 => (GpuVendor::Intel, GpuKind::Integrated),
            _ => (GpuVendor::Other, GpuKind::Integrated),
        };
        if vendor == GpuVendor::Nvidia {
            continue; // Reported below with VRAM from nvidia-smi.
        }
        gpus.push(GpuInfo {
            name: name.unwrap_or_else(|| format!("{vendor:?} graphics")),
            vendor,
            kind,
            vram_bytes: vram.filter(|_| kind == GpuKind::Discrete),
        });
    }
    if saw_nvidia || std::path::Path::new("/proc/driver/nvidia").exists() {
        gpus.extend(nvidia_gpus());
    }
    gpus
}

#[cfg(not(target_os = "linux"))]
fn detect_gpus() -> Vec<GpuInfo> {
    // macOS: Apple Silicon is handled by the caller. Intel Macs are treated as CPU-only.
    Vec::new()
}

#[cfg(target_os = "linux")]
fn nvidia_gpus() -> Vec<GpuInfo> {
    let out = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output();
    let Some(out) = out.ok().filter(|o| o.status.success()) else {
        return vec![GpuInfo {
            name: "NVIDIA graphics (driver not loaded)".to_owned(),
            vendor: GpuVendor::Nvidia,
            kind: GpuKind::Discrete,
            vram_bytes: None,
        }];
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let (name, mib) = line.rsplit_once(',')?;
            Some(GpuInfo {
                name: name.trim().to_owned(),
                vendor: GpuVendor::Nvidia,
                kind: GpuKind::Discrete,
                vram_bytes: mib.trim().parse::<u64>().ok().map(|m| m * 1024 * 1024),
            })
        })
        .collect()
}

/// Looks a device up in the `pci.ids` database shipped by most distros.
#[cfg(target_os = "linux")]
fn pci_device_name(db: &str, vendor: u32, device: u32) -> Option<String> {
    let vendor_prefix = format!("{vendor:04x}  ");
    let device_prefix = format!("\t{device:04x}  ");
    let mut lines = db.lines().skip_while(|l| !l.starts_with(&vendor_prefix));
    lines.next()?;
    lines
        .take_while(|l| l.starts_with('\t') || l.starts_with('#') || l.is_empty())
        .find_map(|l| l.strip_prefix(&device_prefix))
        .map(|name| {
            // "Phoenix1 [Radeon 780M]" reads better as the bracketed marketing name.
            match (name.find('['), name.rfind(']')) {
                (Some(a), Some(b)) if a < b => name[a + 1..b].to_owned(),
                _ => name.to_owned(),
            }
        })
}
