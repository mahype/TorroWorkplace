//! Measures this computer for `ai.recommend` (task MW.4, issue #325): main memory and processor
//! through `sysinfo`, the largest graphics card through what each system offers. The types live in
//! `torro_core::ai::hardware`; the tiers of models that read them in `account::recommend` (task MW.5).
//!
//! - **macOS on Apple silicon:** memory is shared ([`Hardware::unified_memory`]), the graphics
//!   are part of the chip, so the card is named after the processor and has no video memory.
//! - **Linux:** NVIDIA cards through `nvidia-smi`, AMD cards through the kernel's
//!   `/sys/class/drm/card*/device/mem_info_vram_total`.
//! - **Windows:** NVIDIA cards through `nvidia-smi`; without one the display adapters in the
//!   registry (`DriverDesc`, `HardwareInformation.qwMemorySize`).
//!
//! Measuring never fails: whatever cannot be read stays `None`, and the recommendation then takes
//! the smallest tier rather than a model the computer cannot run. The readers of what the tools
//! print ([`parse_nvidia_smi`], [`read_drm`]) are pure, so they are tested with fixtures instead of
//! real hardware.

use std::path::Path;

use torro_core::ai::BoxFuture;
use torro_core::ai::hardware::{Gpu, Hardware};

/// Measures this computer. The core holds one as `Box<dyn Probe>` ([`SystemProbe`] in the
/// service, a fixed value in tests), measures once per run and keeps the result (task MW.6).
pub trait Probe: Send + Sync {
    /// What this computer offers a local model.
    ///
    /// An implementation must:
    /// - never fail and never panic: what it cannot measure is `None` (or `false` for
    ///   [`Hardware::unified_memory`]);
    /// - not block the async runtime: blocking reads run on a blocking thread;
    /// - finish within a few seconds, even when a tool it asks hangs (each one has a time limit).
    fn measure(&self) -> BoxFuture<'_, Hardware>;
}

/// The real computer: `sysinfo` for processor and memory, the system's own ways for the card.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemProbe;

impl Probe for SystemProbe {
    fn measure(&self) -> BoxFuture<'_, Hardware> {
        Box::pin(measure_system())
    }
}

/// Processor and graphics share the main memory on Apple silicon, and only there.
const UNIFIED_MEMORY: bool = cfg!(all(target_os = "macos", target_arch = "aarch64"));

async fn measure_system() -> Hardware {
    // `sysinfo` reads files and asks the kernel synchronously.
    let (cpu, memory_bytes) = tokio::task::spawn_blocking(cpu_and_memory).await.unwrap_or_default();
    let cards = if UNIFIED_MEMORY { Vec::new() } else { cards().await };
    hardware(cpu, memory_bytes, UNIFIED_MEMORY, cards)
}

/// Puts the measured parts together: with shared memory the graphics are the chip itself (named
/// like the processor, without video memory of its own), otherwise the largest card found.
fn hardware(cpu: Option<String>, memory_bytes: Option<u64>, unified_memory: bool, cards: Vec<Gpu>) -> Hardware {
    let gpu = if unified_memory { Some(Gpu { name: cpu.clone(), vram_bytes: None }) } else { largest(cards) };
    Hardware { cpu, memory_bytes, gpu, unified_memory }
}

/// The card with the most video memory: that is the one a model would run on. A card whose memory
/// is unknown counts as none, so it only wins when no other card says more.
fn largest(cards: Vec<Gpu>) -> Option<Gpu> {
    cards.into_iter().max_by_key(|card| card.vram_bytes.unwrap_or(0))
}

/// The processor's name and the main memory in bytes, `None` where the system says nothing.
fn cpu_and_memory() -> (Option<String>, Option<u64>) {
    use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

    let refresh =
        RefreshKind::nothing().with_memory(MemoryRefreshKind::nothing().with_ram()).with_cpu(CpuRefreshKind::nothing());
    let system = System::new_with_specifics(refresh);
    let cpu = system.cpus().first().and_then(|cpu| named(cpu.brand()));
    let memory = Some(system.total_memory()).filter(|&bytes| bytes > 0);
    (cpu, memory)
}

/// A name as a tool printed it: trimmed, `None` when nothing is left.
fn named(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(target_os = "linux")]
async fn cards() -> Vec<Gpu> {
    let mut cards = nvidia_smi().await;
    // Reading sysfs is a blocking read like any file (review of #325).
    let drm = tokio::task::spawn_blocking(|| read_drm(Path::new("/sys/class/drm")));
    cards.extend(drm.await.unwrap_or_default());
    cards
}

#[cfg(windows)]
async fn cards() -> Vec<Gpu> {
    let cards = nvidia_smi().await;
    if !cards.is_empty() {
        return cards;
    }
    // The registry also lists NVIDIA cards, but caps the size of some at 4 GB; `nvidia-smi` knows better.
    tokio::task::spawn_blocking(registry_cards).await.unwrap_or_default()
}

/// Intel Macs and other systems: no way to ask is wired up, so no card is known.
#[cfg(not(any(target_os = "linux", windows)))]
async fn cards() -> Vec<Gpu> {
    Vec::new()
}

/// How long `nvidia-smi` may take; a hanging driver must not hold up the recommendation. The first
/// call after boot can take a few seconds while the driver loads; a measurement that found no card
/// is taken again later (`Core::hardware`).
#[cfg(any(target_os = "linux", windows))]
const NVIDIA_SMI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The NVIDIA cards as `nvidia-smi` lists them; none when it is missing, fails or hangs.
#[cfg(any(target_os = "linux", windows))]
async fn nvidia_smi() -> Vec<Gpu> {
    let mut command = tokio::process::Command::new("nvidia-smi");
    command
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        // When the time limit drops the future, the process goes with it.
        .kill_on_drop(true);
    // The service has no console: without this flag one would flash up while measuring.
    #[cfg(windows)]
    command.creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0);
    match tokio::time::timeout(NVIDIA_SMI_TIMEOUT, command.output()).await {
        Ok(Ok(output)) if output.status.success() => parse_nvidia_smi(&String::from_utf8_lossy(&output.stdout)),
        _ => Vec::new(),
    }
}

/// Reads what `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits` prints: one
/// line per card, `name, MiB`. A line without a comma or without a name is skipped (a driver
/// message, an empty line); a card whose memory is not a number (`[N/A]`) keeps its name and has
/// `vram_bytes: None`.
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
pub fn parse_nvidia_smi(output: &str) -> Vec<Gpu> {
    output
        .lines()
        .filter_map(|line| {
            let (name, mib) = line.split_once(',')?;
            let name = named(name)?;
            let vram_bytes = mib.trim().parse::<u64>().ok().and_then(|mib| mib.checked_mul(1 << 20));
            Some(Gpu { name: Some(name), vram_bytes })
        })
        .collect()
}

/// Reads the AMD cards under `root` (on Linux `/sys/class/drm`): every `card<N>` whose
/// `device/mem_info_vram_total` holds its video memory in bytes, named by `device/product_name`
/// when the driver writes one. Connectors (`card0-DP-1`) and cards without that file (Intel,
/// NVIDIA) are skipped; a missing `root` gives none. Sorted by the card's directory name.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn read_drm(root: &Path) -> Vec<Gpu> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            name.strip_prefix("card").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect();
    names.sort();
    names
        .into_iter()
        .filter_map(|name| {
            let device = root.join(name).join("device");
            let vram = std::fs::read_to_string(device.join("mem_info_vram_total")).ok()?;
            let vram_bytes = vram.trim().parse::<u64>().ok()?;
            let name = std::fs::read_to_string(device.join("product_name")).ok().and_then(|text| named(&text));
            Some(Gpu { name, vram_bytes: Some(vram_bytes) })
        })
        .collect()
}

/// The display adapters Windows lists under the class of display devices, one subkey `00NN` each.
#[cfg(windows)]
fn registry_cards() -> Vec<Gpu> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};

    const DISPLAY: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let Ok(class) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(DISPLAY, KEY_READ) else {
        return Vec::new();
    };
    class
        .enum_keys()
        .flatten()
        // `Properties` and the like cannot be read and are no adapters.
        .filter(|name| name.len() == 4 && name.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|name| class.open_subkey_with_flags(name, KEY_READ).ok())
        .map(|adapter| {
            let name = adapter.get_value::<String, _>("DriverDesc").ok().and_then(|text| named(&text));
            // `qwMemorySize` is a QWORD; older drivers only write the DWORD `MemorySize`, capped at 4 GB.
            let vram_bytes = adapter
                .get_value::<u64, _>("HardwareInformation.qwMemorySize")
                .ok()
                .or_else(|| adapter.get_value::<u32, _>("HardwareInformation.MemorySize").ok().map(u64::from))
                .filter(|&bytes| bytes > 0);
            Gpu { name, vram_bytes }
        })
        .filter(|card| card.name.is_some() || card.vram_bytes.is_some())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const MIB: u64 = 1 << 20;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hardware").join(name)
    }

    fn nvidia(name: &str) -> Vec<Gpu> {
        let text = std::fs::read_to_string(fixture(name)).expect("fixture");
        parse_nvidia_smi(&text)
    }

    fn card(name: &str, vram_bytes: Option<u64>) -> Gpu {
        Gpu { name: Some(name.into()), vram_bytes }
    }

    #[test]
    fn nvidia_smi_with_one_card_gives_its_name_and_memory_in_bytes() {
        assert_eq!(nvidia("nvidia-smi-one.csv"), vec![card("NVIDIA GeForce RTX 4070", Some(12282 * MIB))]);
    }

    #[test]
    fn nvidia_smi_with_two_cards_gives_both_and_the_larger_wins() {
        let cards = nvidia("nvidia-smi-two.csv");
        assert_eq!(
            cards,
            vec![
                card("NVIDIA GeForce RTX 3060", Some(12288 * MIB)),
                card("NVIDIA GeForce RTX 4090", Some(24564 * MIB))
            ]
        );
        assert_eq!(largest(cards), Some(card("NVIDIA GeForce RTX 4090", Some(24564 * MIB))));
    }

    #[test]
    fn nvidia_smi_without_cards_gives_none() {
        assert_eq!(parse_nvidia_smi(""), Vec::new());
        assert_eq!(parse_nvidia_smi("\n  \n"), Vec::new());
        assert_eq!(largest(Vec::new()), None);
    }

    #[test]
    fn broken_lines_of_nvidia_smi_are_skipped_and_unknown_memory_stays_none() {
        assert_eq!(
            nvidia("nvidia-smi-broken.csv"),
            vec![
                card("NVIDIA GeForce RTX 4070", Some(12282 * MIB)),
                card("NVIDIA GH200 480GB", None),
                card("NVIDIA RTX A2000", None),
            ]
        );
    }

    #[test]
    fn drm_reads_cards_with_video_memory_and_skips_connectors_and_others() {
        assert_eq!(
            read_drm(&fixture("drm")),
            vec![
                card("AMD Radeon RX 7900 XT", Some(17_163_091_968)),
                Gpu { name: None, vram_bytes: Some(536_870_912) },
            ]
        );
    }

    #[test]
    fn drm_without_amd_cards_or_without_the_directory_gives_none() {
        assert_eq!(read_drm(&fixture("drm-none")), Vec::new());
        assert_eq!(read_drm(&fixture("does-not-exist")), Vec::new());
    }

    #[test]
    fn a_card_with_unknown_memory_only_wins_alone() {
        let unknown = card("NVIDIA GH200 480GB", None);
        let known = card("AMD Radeon RX 7900 XT", Some(20 << 30));
        assert_eq!(largest(vec![unknown.clone(), known.clone()]), Some(known));
        assert_eq!(largest(vec![unknown.clone()]), Some(unknown));
    }

    #[test]
    fn shared_memory_names_the_graphics_after_the_chip_without_video_memory() {
        let mac = hardware(Some("Apple M3 Max".into()), Some(64 << 30), true, Vec::new());
        assert_eq!(
            mac,
            Hardware {
                cpu: Some("Apple M3 Max".into()),
                memory_bytes: Some(64 << 30),
                gpu: Some(Gpu { name: Some("Apple M3 Max".into()), vram_bytes: None }),
                unified_memory: true,
            }
        );
    }

    #[test]
    fn without_shared_memory_the_largest_card_is_the_gpu_and_nothing_measured_stays_none() {
        let pc = hardware(None, None, false, nvidia("nvidia-smi-two.csv"));
        assert_eq!(pc.gpu, Some(card("NVIDIA GeForce RTX 4090", Some(24564 * MIB))));
        assert_eq!(hardware(None, None, false, Vec::new()), Hardware::default());
    }
}
