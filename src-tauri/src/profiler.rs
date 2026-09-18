//! Hardware profiling and model-tier selection.
//!
//! Orion measures the machine and *recommends* a model tier, showing its
//! reasoning. The user can always override.
//!
//! Two rules that make this honest rather than decorative:
//!
//! * **Budget against available RAM, not total.** An 8 GB laptop with a
//!   browser open has ~4 GB free. Recommending a model sized against the
//!   16 GB sticker value guarantees swapping.
//! * **A tier must be *simulatable*.** The primary dev machine has 8 GB, so
//!   the T2/T3/T4 paths can never be exercised on real hardware there. Without
//!   `ORION_FORCE_PROFILE` those code paths would ship unverified.

use serde::{Deserialize, Serialize};

use crate::error::{OrionError, Result};

/// Bytes in a gibibyte.
const GIB: u64 = 1024 * 1024 * 1024;

/// RAM to leave for the OS, the app, and the user's other work.
/// Below this margin the machine starts swapping and the model feels broken.
const OS_HEADROOM_GIB: f64 = 2.0;

/// Model tiers, ordered by capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
#[serde(rename_all = "UPPERCASE")]
pub enum Tier {
    /// Remote inference — the user's own GPU box or an API. No local weights.
    T0,
    /// ≤8 GB RAM, CPU only. ~4B parameters.
    T1,
    /// ~16 GB RAM. ~8B parameters.
    T2,
    /// ~32 GB RAM or 8–12 GB VRAM. ~14B parameters.
    T3,
    /// ≥24 GB VRAM. ~32B parameters.
    T4,
}

impl Tier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::T0 => "T0",
            Tier::T1 => "T1",
            Tier::T2 => "T2",
            Tier::T3 => "T3",
            Tier::T4 => "T4",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Tier::T0 => "Remote",
            Tier::T1 => "Minimal",
            Tier::T2 => "Standard",
            Tier::T3 => "Performance",
            Tier::T4 => "Workstation",
        }
    }

    /// Approximate resident memory of this tier's chat model, in GiB.
    ///
    /// T1 is **measured**, not estimated: Qwen2.5-3B-Instruct Q4_K_M at a
    /// 4096-token context needs 2002 MiB of weights + 144 MiB KV cache +
    /// 301 MiB compute buffer = ~2.4 GiB resident. The earlier 3.4 figure was
    /// a guess and it was 40% too high, which was enough on its own to push a
    /// perfectly capable 6 GiB laptop down to "no local model possible".
    ///
    /// The others are still scaled estimates and should be replaced with real
    /// measurements as each tier gets run on hardware.
    pub fn model_ram_gib(&self) -> f64 {
        match self {
            Tier::T0 => 0.0,
            Tier::T1 => 2.4,
            Tier::T2 => 4.8,
            Tier::T3 => 8.5,
            Tier::T4 => 18.0,
        }
    }

    pub fn parse(s: &str) -> Option<Tier> {
        match s.trim().to_ascii_uppercase().as_str() {
            "T0" => Some(Tier::T0),
            "T1" => Some(Tier::T1),
            "T2" => Some(Tier::T2),
            "T3" => Some(Tier::T3),
            "T4" => Some(Tier::T4),
            _ => None,
        }
    }

    /// The next tier down, for graceful degradation under memory pressure.
    pub fn step_down(&self) -> Option<Tier> {
        match self {
            Tier::T4 => Some(Tier::T3),
            Tier::T3 => Some(Tier::T2),
            Tier::T2 => Some(Tier::T1),
            Tier::T1 | Tier::T0 => None,
        }
    }
}

/// A snapshot of the machine Orion is running on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareProfile {
    pub total_ram_gib: f64,
    pub available_ram_gib: f64,
    pub cpu_cores: usize,
    pub cpu_brand: String,
    pub gpu_vendor: Option<String>,
    pub gpu_vram_gib: Option<f64>,
    /// True when the GPU shares system RAM rather than having its own memory.
    ///
    /// This matters more than it looks. An integrated GPU's "2 GB dedicated"
    /// pool is carved *out of* system RAM, not added to it: an 8 GB laptop
    /// with a 2 GB iGPU reservation reports ~5.7 GB to the OS. Treating that
    /// VRAM as extra capacity would double-count memory the machine does not
    /// have, and would push the tier above what the box can actually run.
    #[serde(default)]
    pub gpu_integrated: bool,
    pub free_disk_gib: f64,
    pub os: String,
    pub arch: String,
    /// True when this profile came from `ORION_FORCE_PROFILE` rather than
    /// real measurement. Always surfaced in the UI so simulated runs are
    /// never mistaken for real ones.
    pub simulated: bool,
}

/// A tier recommendation plus the reasoning behind it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierRecommendation {
    pub tier: Tier,
    pub label: String,
    /// Human-readable justification, shown in the UI.
    pub reasons: Vec<String>,
    /// Warnings that do not block the choice but the user should see.
    pub warnings: Vec<String>,
    /// Tiers that would fit if the user insists, cheapest first.
    pub alternatives: Vec<Tier>,
}

impl HardwareProfile {
    /// Measure the real machine.
    pub fn detect() -> Self {
        use sysinfo::System;

        let mut sys = System::new_all();
        sys.refresh_memory();
        sys.refresh_cpu_usage();

        let total_ram_gib = sys.total_memory() as f64 / GIB as f64;
        let available_ram_gib = sys.available_memory() as f64 / GIB as f64;

        let cpu_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let cpu_brand = sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".into());

        let (gpu_vendor, gpu_vram_gib, gpu_integrated) = detect_gpu();

        let free_disk_gib = detect_free_disk();

        Self {
            total_ram_gib,
            available_ram_gib,
            cpu_cores,
            cpu_brand,
            gpu_vendor,
            gpu_vram_gib,
            gpu_integrated,
            free_disk_gib,
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            simulated: false,
        }
    }

    /// Detect, unless `ORION_FORCE_PROFILE` asks for a simulated machine.
    ///
    /// Accepted values:
    ///   * a tier name — `T3` — synthesises a typical machine for that tier
    ///   * `ram=32,cores=16,vram=12` — a specific synthetic profile
    ///
    /// This is what makes higher tiers testable from an 8 GB dev laptop.
    pub fn detect_or_forced() -> Self {
        match std::env::var("ORION_FORCE_PROFILE") {
            Ok(spec) if !spec.trim().is_empty() => match Self::from_spec(&spec) {
                Ok(p) => {
                    tracing::warn!(%spec, "using SIMULATED hardware profile");
                    p
                }
                Err(e) => {
                    tracing::error!(%spec, error = %e, "bad ORION_FORCE_PROFILE, detecting instead");
                    Self::detect()
                }
            },
            _ => Self::detect(),
        }
    }

    /// Build a synthetic profile from a spec string.
    pub fn from_spec(spec: &str) -> Result<Self> {
        let spec = spec.trim();

        // Shorthand: a bare tier name.
        if let Some(tier) = Tier::parse(spec) {
            let (ram, cores, vram) = match tier {
                Tier::T0 => (8.0, 4, None),
                Tier::T1 => (8.0, 8, None),
                Tier::T2 => (16.0, 12, None),
                Tier::T3 => (32.0, 16, Some(12.0)),
                Tier::T4 => (64.0, 24, Some(24.0)),
            };
            return Ok(Self {
                total_ram_gib: ram,
                // Simulate a machine with normal background usage.
                available_ram_gib: ram * 0.75,
                cpu_cores: cores,
                cpu_brand: format!("simulated {} cpu", tier.as_str()),
                gpu_vendor: vram.map(|_| "simulated".to_string()),
                gpu_vram_gib: vram,
                gpu_integrated: false,
                free_disk_gib: 200.0,
                os: std::env::consts::OS.to_string(),
                arch: std::env::consts::ARCH.to_string(),
                simulated: true,
            });
        }

        // Long form: key=value pairs.
        let mut ram: Option<f64> = None;
        let mut avail: Option<f64> = None;
        let mut cores: Option<usize> = None;
        let mut vram: Option<f64> = None;
        let mut disk: Option<f64> = None;

        for part in spec.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (k, v) = part
                .split_once('=')
                .ok_or_else(|| OrionError::Config(format!("bad profile fragment: {part}")))?;
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim();
            match k.as_str() {
                "ram" => ram = Some(v.parse().map_err(|_| bad(&k, v))?),
                "avail" | "available" => avail = Some(v.parse().map_err(|_| bad(&k, v))?),
                "cores" => cores = Some(v.parse().map_err(|_| bad(&k, v))?),
                "vram" => vram = Some(v.parse().map_err(|_| bad(&k, v))?),
                "disk" => disk = Some(v.parse().map_err(|_| bad(&k, v))?),
                other => {
                    return Err(OrionError::Config(format!("unknown profile key: {other}")));
                }
            }
        }

        let total = ram.ok_or_else(|| OrionError::Config("profile needs ram=<gib>".into()))?;
        if total <= 0.0 {
            return Err(OrionError::Config("ram must be positive".into()));
        }

        Ok(Self {
            total_ram_gib: total,
            available_ram_gib: avail.unwrap_or(total * 0.75),
            cpu_cores: cores.unwrap_or(4),
            cpu_brand: "simulated cpu".into(),
            gpu_vendor: vram.map(|_| "simulated".to_string()),
            gpu_vram_gib: vram,
            gpu_integrated: false,
            free_disk_gib: disk.unwrap_or(200.0),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            simulated: true,
        })
    }

    /// RAM we are willing to give a model right now.
    ///
    /// This deliberately does **not** treat "available" memory as a hard
    /// ceiling, which an earlier version did and got badly wrong.
    ///
    /// On Windows, and to a lesser degree on Linux and macOS, most of RAM is
    /// usually occupied by file cache and standby pages. The OS reclaims that
    /// the instant a process asks for memory. A real 6 GiB laptop reported
    /// 0.9 GiB "available", which the old rule turned into 0.0 GiB usable and
    /// a recommendation of "no local model possible" — while the very same
    /// machine was already running a 2.4 GiB model at 10 tokens/second.
    ///
    /// So the budget is taken from **total** RAM minus headroom, and the
    /// currently-free figure is used only as a soft signal (see `recommend`)
    /// to warn the user rather than to refuse.
    pub fn usable_ram_gib(&self) -> f64 {
        (self.total_ram_gib - OS_HEADROOM_GIB).max(0.0)
    }

    /// What is genuinely free right this second, after headroom.
    ///
    /// Used for advice, never for gating: see `usable_ram_gib`.
    pub fn free_now_gib(&self) -> f64 {
        (self.available_ram_gib - OS_HEADROOM_GIB).max(0.0)
    }

    /// Recommend a tier, with reasoning.
    pub fn recommend(&self) -> TierRecommendation {
        let mut reasons = Vec::new();
        let mut warnings = Vec::new();

        let usable = self.usable_ram_gib();
        reasons.push(format!(
            "{:.1} GiB RAM installed; budgeting {:.1} GiB for a model after a {:.0} GiB \
             headroom for the system.",
            self.total_ram_gib, usable, OS_HEADROOM_GIB
        ));

        // A discrete GPU with real VRAM lets us punch above the RAM tier,
        // because weights live on the card rather than in system memory.
        // Integrated GPUs contribute ZERO to the budget. Their "dedicated"
        // memory is carved out of system RAM, which the OS has already
        // subtracted from the total we measured. Counting it would be
        // double-counting memory the machine does not have.
        let vram = if self.gpu_integrated {
            0.0
        } else {
            self.gpu_vram_gib.unwrap_or(0.0)
        };

        match (&self.gpu_vendor, self.gpu_integrated) {
            (Some(name), true) => reasons.push(format!(
                "{name} is an integrated GPU; its memory is shared with system RAM and is \
                 already counted above. Inference runs on the CPU."
            )),
            (Some(name), false) if vram > 0.0 => reasons.push(format!(
                "{name} detected with {vram:.1} GiB of dedicated VRAM."
            )),
            (Some(name), false) => reasons.push(format!(
                "{name} detected, but no usable dedicated VRAM was reported; inference will \
                 run on the CPU."
            )),
            (None, _) => reasons.push("No GPU detected; inference will run on the CPU.".into()),
        }

        // `vram` is already zeroed for integrated GPUs above, so this only
        // ever sees dedicated memory. Belt and braces: a misclassified iGPU
        // must not be able to lift the tier, because its "VRAM" is system RAM
        // that has already been subtracted from the total.
        debug_assert!(
            !self.gpu_integrated || vram == 0.0,
            "integrated VRAM leaked into the tier calculation"
        );
        let by_vram = if vram >= 22.0 {
            Some(Tier::T4)
        } else if vram >= 10.0 {
            Some(Tier::T3)
        } else if vram >= 6.0 {
            Some(Tier::T2)
        } else {
            None
        };

        // Two independent caps, and we take the lower of them.
        //
        //  * `by_class` is the machine's *class* from installed RAM. An 8 GiB
        //    laptop is a T1 machine, a 16 GiB desktop is T2, and so on. Fitting
        //    a bigger model into the momentarily-free memory of a 16 GiB box
        //    would leave nothing for the user's actual work.
        //  * `by_free` is what is genuinely available right now. It can only
        //    ever *lower* the recommendation — this is the "budget against
        //    available, not total" rule.
        let by_class = if self.total_ram_gib >= 33.0 {
            Tier::T4
        } else if self.total_ram_gib >= 17.0 {
            Tier::T3
        } else if self.total_ram_gib >= 9.0 {
            Tier::T2
        } else {
            Tier::T1
        };

        let by_free = if usable >= Tier::T4.model_ram_gib() {
            Tier::T4
        } else if usable >= Tier::T3.model_ram_gib() {
            Tier::T3
        } else if usable >= Tier::T2.model_ram_gib() {
            Tier::T2
        } else if usable >= Tier::T1.model_ram_gib() {
            Tier::T1
        } else {
            // Genuinely too small for even the smallest local model. With a
            // 2 GiB headroom and T1 at 2.4 GiB, this needs less than ~4.4 GiB
            // of *installed* RAM, which is a real constraint rather than a
            // transient one.
            warnings.push(format!(
                "This machine has {:.1} GiB of RAM. Orion needs about {:.1} GiB for its \
                 smallest model, so local inference is not possible here.",
                self.total_ram_gib,
                Tier::T1.model_ram_gib() + OS_HEADROOM_GIB
            ));
            return TierRecommendation {
                tier: Tier::T0,
                label: Tier::T0.label().into(),
                reasons,
                warnings,
                alternatives: vec![Tier::T1],
            };
        };

        // Free memory is advisory. The model will still load — the OS evicts
        // cache to make room — but it may swap and feel slow, so say so
        // instead of refusing to run.
        let free_now = self.free_now_gib();
        if free_now < Tier::T1.model_ram_gib() {
            warnings.push(format!(
                "Only {:.1} GiB is free right now. The model will still load, because the \
                 system reclaims cached memory, but closing other applications will make it \
                 noticeably faster.",
                self.available_ram_gib
            ));
        }

        let by_ram = by_class.min(by_free);
        if by_free < by_class {
            reasons.push(format!(
                "Free memory currently limits this to {}, below the {} this machine could \
                 otherwise run.",
                by_free.as_str(),
                by_class.as_str()
            ));
        }

        // A discrete GPU can lift the recommendation, since weights live in
        // VRAM rather than system memory — but only when enough free system
        // RAM remains to stage them.
        let tier = match by_vram {
            Some(v) if v > by_ram && usable >= Tier::T1.model_ram_gib() => {
                reasons.push(format!(
                    "GPU supports {}, which is above the {} the system RAM alone would allow.",
                    v.as_str(),
                    by_ram.as_str()
                ));
                v
            }
            _ => by_ram,
        };

        reasons.push(format!(
            "Recommending {} ({}), which needs about {:.1} GiB.",
            tier.as_str(),
            tier.label(),
            tier.model_ram_gib()
        ));

        if self.cpu_cores <= 2 && vram == 0.0 {
            warnings.push(format!(
                "Only {} CPU cores detected — generation will be slow. Expect a long wait \
                 for each reply.",
                self.cpu_cores
            ));
        }

        // Weights need disk before they need memory.
        let needed_disk = tier.model_ram_gib() * 1.1;
        if self.free_disk_gib < needed_disk {
            warnings.push(format!(
                "Only {:.1} GiB of disk free; this model needs roughly {needed_disk:.1} GiB.",
                self.free_disk_gib
            ));
        }

        // Deliberately no warning here about voice latency or agent modes.
        // Voice is M4 and agent actions are M5; neither exists yet. Warning
        // about unbuilt features teaches the user to ignore warnings, which
        // is exactly when a real one gets missed. Reinstate this when the
        // features ship, with measured numbers rather than guesses.

        if self.simulated {
            warnings.push(
                "This is a SIMULATED hardware profile from ORION_FORCE_PROFILE, not a real \
                 measurement."
                    .into(),
            );
        }

        let alternatives = [Tier::T1, Tier::T2, Tier::T3, Tier::T4]
            .into_iter()
            .filter(|t| *t != tier && t.model_ram_gib() <= usable && *t <= tier.max(by_class))
            .collect();

        TierRecommendation {
            tier,
            label: tier.label().into(),
            reasons,
            warnings,
            alternatives,
        }
    }
}

fn bad(key: &str, value: &str) -> OrionError {
    OrionError::Config(format!("bad value for {key}: {value}"))
}

/// Best-effort GPU detection.
///
/// Deliberately conservative: guessing a GPU that is not usable is worse than
/// missing one, because it leads to a recommendation the machine cannot honour.
/// Detected GPU: vendor label, dedicated VRAM in GiB, and whether it shares
/// system memory.
fn detect_gpu() -> (Option<String>, Option<f64>, bool) {
    // NVIDIA: nvidia-smi is authoritative when present. Discrete, so its VRAM
    // is genuinely additional memory.
    if let Ok(out) = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
    {
        if out.status.success() {
            if let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next() {
                if let Ok(mib) = line.trim().parse::<f64>() {
                    return (Some("NVIDIA".into()), Some(mib / 1024.0), false);
                }
            }
        }
    }

    // Apple Silicon: unified memory, so there is no separate VRAM pool to add.
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        return (Some("Apple Silicon".into()), None, true);
    }

    #[cfg(target_os = "windows")]
    if let Some(gpu) = detect_gpu_windows() {
        return gpu;
    }

    #[cfg(target_os = "linux")]
    if let Some(gpu) = detect_gpu_linux() {
        return gpu;
    }

    (None, None, false)
}

/// Windows GPU via WMIC/CIM. Reports the adapter name and its memory, and
/// classifies integrated parts by name.
///
/// Reporting "None detected" on a machine that plainly has a Radeon is simply
/// wrong, even when the tier outcome is unaffected.
#[cfg(target_os = "windows")]
fn detect_gpu_windows() -> Option<(Option<String>, Option<f64>, bool)> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; \
             Get-CimInstance Win32_VideoController | \
             Select-Object -First 1 -Property Name,AdapterRAM | \
             ForEach-Object { \"$($_.Name)|$($_.AdapterRAM)\" }",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    if !out.status.success() {
        return None;
    }

    let line = String::from_utf8_lossy(&out.stdout);
    let line = line.trim();
    let (name, ram) = line.split_once('|')?;
    let name = tidy_gpu_name(name);
    if name.is_empty() {
        return None;
    }
    let name = name.as_str();

    // AdapterRAM is a 32-bit field and wraps above 4 GiB, so it is only a
    // hint. It is never used for budgeting, only for display.
    let vram = ram
        .trim()
        .parse::<f64>()
        .ok()
        .map(|b| b / GIB as f64)
        .filter(|v| *v > 0.1);

    let integrated = is_integrated_gpu(name);
    Some((Some(name.to_string()), vram, integrated))
}

/// Linux GPU via the DRM sysfs tree, falling back to lspci.
#[cfg(target_os = "linux")]
fn detect_gpu_linux() -> Option<(Option<String>, Option<f64>, bool)> {
    let out = std::process::Command::new("lspci").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find(|l| l.contains("VGA compatible controller") || l.contains("3D controller"))?;
    let name = line.split_once(": ").map(|(_, n)| n.trim())?;
    if name.is_empty() {
        return None;
    }
    let integrated = is_integrated_gpu(name);
    Some((Some(name.to_string()), None, integrated))
}

/// Classify an adapter name as integrated.
///
/// Name matching is crude but the cost of being wrong is small: an integrated
/// part misread as discrete would have its VRAM counted as extra memory, so
/// the list errs toward calling things integrated.
/// Clean up a vendor-reported adapter name for display.
///
/// Strips trademark markers, including the mojibake forms that appear when a
/// code-page byte is decoded as UTF-8, and collapses whitespace.
// Only the Windows probe calls this, but the tests exercise it everywhere.
#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
fn tidy_gpu_name(raw: &str) -> String {
    let mut out = raw.to_string();
    for marker in [
        "(TM)", "(R)", "(tm)", "(r)", "\u{2122}", "\u{00ae}", "\u{fffd}",
    ] {
        out = out.replace(marker, " ");
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_integrated_gpu(name: &str) -> bool {
    // Normalise first. Vendor strings arrive with trademark glyphs in various
    // encodings ("Radeon(TM)", "Radeon(R)", or mojibake such as "RadeonT"
    // when a code-page byte is decoded as UTF-8), and with inconsistent
    // spacing. Strip anything that is not a letter or digit down to single
    // spaces so matching works on the words that matter.
    let n: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        // Drop trademark tokens left behind by the punctuation strip, so
        // "Radeon(TM) Graphics" normalises to "radeon graphics" rather than
        // "radeon tm graphics" and still matches the marker list.
        .filter(|tok| !matches!(*tok, "tm" | "r" | "c"))
        .collect::<Vec<_>>()
        .join(" ");

    // Explicitly discrete families win, because some share brand words with
    // integrated parts ("Radeon RX 7900" vs "Radeon 610M").
    const DISCRETE_MARKERS: &[&str] = &[
        "geforce",
        "quadro",
        "tesla",
        "radeon rx",
        "radeon pro",
        "firepro",
        "arc a",
        "arc b",
        "rtx",
        "gtx",
    ];
    if DISCRETE_MARKERS.iter().any(|m| n.contains(m)) {
        return false;
    }

    const INTEGRATED_MARKERS: &[&str] = &[
        "radeon graphics",
        "vega",
        "uhd graphics",
        "hd graphics",
        "iris",
        "apple",
        "adreno",
        "mali",
        "integrated",
        "microsoft basic display",
    ];
    if INTEGRATED_MARKERS.iter().any(|m| n.contains(m)) {
        return true;
    }

    // AMD's integrated parts are named <number><letter>, e.g. 610M, 680M,
    // 780M, 890M. A bare "radeon" followed by such a token is integrated.
    if n.contains("radeon") {
        let integrated_suffix = n.split_whitespace().any(|tok| {
            let digits = tok.trim_end_matches(|c: char| c.is_ascii_alphabetic());
            let suffix = &tok[digits.len()..];
            digits.len() == 3
                && digits.chars().all(|c| c.is_ascii_digit())
                && matches!(suffix, "m" | "mx")
        });
        if integrated_suffix {
            return true;
        }
    }

    false
}

/// Free space on the volume holding Orion's data directory.
fn detect_free_disk() -> f64 {
    use sysinfo::Disks;

    let target = crate::db::data_dir().ok();
    let disks = Disks::new_with_refreshed_list();

    // Choose the most specific mount point that contains our data dir.
    let mut best: Option<(usize, f64)> = None;
    if let Some(path) = &target {
        for d in disks.list() {
            let mp = d.mount_point();
            if path.starts_with(mp) {
                let depth = mp.components().count();
                let free = d.available_space() as f64 / GIB as f64;
                if best.map(|(bd, _)| depth > bd).unwrap_or(true) {
                    best = Some((depth, free));
                }
            }
        }
    }

    best.map(|(_, f)| f).unwrap_or_else(|| {
        disks
            .list()
            .first()
            .map(|d| d.available_space() as f64 / GIB as f64)
            .unwrap_or(0.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(ram: f64, avail: f64, cores: usize, vram: Option<f64>) -> HardwareProfile {
        HardwareProfile {
            total_ram_gib: ram,
            available_ram_gib: avail,
            cpu_cores: cores,
            cpu_brand: "test".into(),
            gpu_vendor: vram.map(|_| "test-gpu".into()),
            gpu_vram_gib: vram,
            gpu_integrated: false,
            free_disk_gib: 500.0,
            os: "linux".into(),
            arch: "x86_64".into(),
            simulated: true,
        }
    }

    #[test]
    fn detects_real_hardware_without_panicking() {
        let p = HardwareProfile::detect();
        assert!(p.total_ram_gib > 0.0);
        assert!(p.cpu_cores >= 1);
        assert!(!p.simulated);
    }

    /* ---------- tier selection ---------- */

    #[test]
    fn eight_gb_laptop_gets_t1() {
        // The primary dev machine: 8 GB, browser open, no GPU.
        let r = profile(8.0, 6.0, 8, None).recommend();
        assert_eq!(r.tier, Tier::T1);
    }

    #[test]
    fn sixteen_gb_gets_t2() {
        let r = profile(16.0, 12.0, 12, None).recommend();
        assert_eq!(r.tier, Tier::T2);
    }

    #[test]
    fn thirtytwo_gb_gets_t3() {
        let r = profile(32.0, 24.0, 16, None).recommend();
        assert_eq!(r.tier, Tier::T3);
    }

    #[test]
    fn big_vram_gets_t4() {
        let r = profile(64.0, 48.0, 24, Some(24.0)).recommend();
        assert_eq!(r.tier, Tier::T4);
    }

    #[test]
    fn gpu_lifts_tier_above_what_ram_alone_allows() {
        // 16 GB system RAM would give T2, but a 12 GB card supports T3.
        let r = profile(16.0, 13.0, 12, Some(12.0)).recommend();
        assert_eq!(r.tier, Tier::T3);
    }

    /* ---------- the rules that keep it honest ---------- */

    #[test]
    fn busy_machine_still_gets_a_local_model() {
        // This used to assert the opposite, and the opposite was wrong.
        //
        // A 32 GiB workstation with 6 GiB free is not a T1 machine. The OS is
        // using the rest for file cache and will hand it back the moment a
        // model asks. Budgeting against the momentary free figure produced
        // absurdly small recommendations on perfectly capable hardware.
        let r = profile(32.0, 6.0, 16, None).recommend();
        assert!(
            r.tier >= Tier::T3,
            "a 32 GiB machine is a T3+ machine whatever is cached right now, got {:?}",
            r.tier
        );
    }

    #[test]
    fn integrated_vram_never_inflates_the_tier() {
        // A 6 GiB laptop whose iGPU reports 2 GiB "dedicated" memory must not
        // be treated as an 8 GiB machine. That memory was carved out of system
        // RAM and the OS has already subtracted it from the total.
        let mut p = profile(5.74, 1.2, 8, Some(2.0));
        p.gpu_integrated = true;
        p.gpu_vendor = Some("AMD Radeon Graphics".into());

        let integrated = p.recommend();

        let mut d = profile(5.74, 1.2, 8, Some(2.0));
        d.gpu_integrated = false;
        let discrete = d.recommend();

        assert_eq!(
            integrated.tier,
            Tier::T1,
            "integrated VRAM must not lift the tier"
        );
        assert!(
            integrated.tier <= discrete.tier,
            "integrated must never outrank the same machine with a real card"
        );
    }

    #[test]
    fn an_integrated_gpu_is_named_not_denied() {
        // Saying "None detected" on a machine that plainly has a Radeon is
        // wrong, even when the tier outcome is unaffected.
        let mut p = profile(5.74, 1.2, 8, Some(2.0));
        p.gpu_integrated = true;
        p.gpu_vendor = Some("AMD Radeon Graphics".into());

        let r = p.recommend();
        assert!(
            r.reasons.iter().any(|x| x.contains("Radeon")),
            "the GPU should be named: {:?}",
            r.reasons
        );
        assert!(
            r.reasons
                .iter()
                .any(|x| x.contains("shared with system RAM")),
            "should explain why its memory does not count: {:?}",
            r.reasons
        );
        assert!(
            !r.reasons.iter().any(|x| x.contains("No GPU detected")),
            "must not claim there is no GPU: {:?}",
            r.reasons
        );
    }

    #[test]
    fn integrated_gpu_names_are_classified() {
        for name in [
            "AMD Radeon(TM) Graphics",
            "AMD Radeon Graphics",
            "Intel(R) UHD Graphics 620",
            "Intel(R) Iris(R) Xe Graphics",
            "Apple M2",
            "Qualcomm Adreno 740",
            // Reported from a real machine. The old matcher looked for
            // "radeon graphics" and missed this entirely, classifying an
            // integrated part as discrete.
            "AMD Radeon(TM) 610M",
            // The mangled form that actually arrived, before the output
            // encoding was forced to UTF-8.
            "AMD RadeonT 610M",
            "AMD Radeon(TM) 780M Graphics",
            "AMD Radeon 890M",
        ] {
            assert!(is_integrated_gpu(name), "{name} should be integrated");
        }
        for name in [
            "NVIDIA GeForce RTX 4070",
            "AMD Radeon RX 7900 XTX",
            "NVIDIA RTX A4000",
            "AMD Radeon Pro W7900",
            "Intel(R) Arc(TM) A770 Graphics",
        ] {
            assert!(!is_integrated_gpu(name), "{name} should be discrete");
        }
    }

    #[test]
    fn trademark_glyphs_are_stripped_from_display_names() {
        assert_eq!(tidy_gpu_name("AMD Radeon(TM) 610M"), "AMD Radeon 610M");
        assert_eq!(tidy_gpu_name("Intel(R) UHD Graphics"), "Intel UHD Graphics");
        assert_eq!(tidy_gpu_name("  spaced   out  "), "spaced out");
        assert_eq!(
            tidy_gpu_name("NVIDIA GeForce RTX\u{2122} 4070"),
            "NVIDIA GeForce RTX 4070"
        );
    }

    #[test]
    fn a_large_integrated_gpu_cannot_lift_the_tier() {
        // The dangerous case the 610M got away with by luck: modern iGPUs
        // such as the 780M and 890M report 8+ GiB of "dedicated" memory,
        // which is shared system RAM. At the 6 GiB threshold that would have
        // lifted an 8 GiB laptop to T2 with no extra memory to run it.
        let mut p = profile(8.0, 3.0, 8, Some(8.0));
        p.gpu_integrated = true;
        p.gpu_vendor = Some("AMD Radeon 780M Graphics".into());

        let r = p.recommend();
        assert_eq!(
            r.tier,
            Tier::T1,
            "8 GiB of shared iGPU memory must not buy a bigger model"
        );
    }

    #[test]
    fn no_warnings_about_features_that_do_not_exist_yet() {
        // Voice is M4 and agent actions are M5. Warning about them now trains
        // the user to ignore warnings.
        let r = profile(5.74, 1.2, 8, None).recommend();
        let text = r.warnings.join(" ").to_lowercase();
        for word in ["voice", "agent", "propose-only"] {
            assert!(
                !text.contains(word),
                "warning mentions unbuilt feature {word:?}: {:?}",
                r.warnings
            );
        }
    }

    #[test]
    fn the_real_6gib_laptop_is_not_told_to_give_up() {
        // Regression test for an actual machine: AMD Ryzen 5 7520U, 8 cores,
        // 5.74 GiB total, 0.95 GiB reported available, no discrete GPU.
        //
        // The profiler recommended T0 ("too little for a local model") while
        // that very machine was running Qwen2.5-3B at 10.4 tok/s in 2.4 GiB.
        let r = profile(5.739_498, 0.946_495, 8, None).recommend();

        assert_eq!(
            r.tier,
            Tier::T1,
            "6 GiB laptop must get T1; it demonstrably runs a 3B model"
        );
        assert!(
            !r.warnings
                .iter()
                .any(|w| w.contains("too little for a local model")),
            "must not claim local inference is impossible: {:?}",
            r.warnings
        );
        // It should still mention that memory is tight.
        assert!(
            r.warnings.iter().any(|w| w.contains("free right now")),
            "should still advise about low free memory: {:?}",
            r.warnings
        );
    }

    #[test]
    fn low_free_memory_warns_but_does_not_downgrade() {
        let busy = profile(16.0, 1.0, 8, None).recommend();
        let idle = profile(16.0, 14.0, 8, None).recommend();
        assert_eq!(
            busy.tier, idle.tier,
            "free memory must not change the tier, only the advice"
        );
        assert!(busy.warnings.len() > idle.warnings.len());
    }

    #[test]
    fn genuinely_tiny_machines_still_get_t0() {
        // The T0 path must stay reachable, just for real constraints.
        // T1 (2.4) + headroom (2.0) = 4.4 GiB, so 4 GiB cannot run a model.
        let r = profile(4.0, 3.5, 4, None).recommend();
        assert_eq!(r.tier, Tier::T0);
        assert!(r.warnings.iter().any(|w| w.contains("not possible")));
    }

    #[test]
    fn t1_matches_the_memory_actually_measured() {
        // 2002 MiB weights + 144 MiB KV + 301 MiB compute = ~2.39 GiB,
        // measured running Qwen2.5-3B-Instruct Q4_K_M at ctx 4096.
        let measured = (2002.0 + 144.0 + 301.0) / 1024.0;
        assert!(
            Tier::T1.model_ram_gib() >= measured,
            "T1 budget {:.2} is below the measured {:.2} GiB",
            Tier::T1.model_ram_gib(),
            measured
        );
        assert!(
            Tier::T1.model_ram_gib() < measured + 0.5,
            "T1 budget {:.2} is padded well beyond the measured {:.2} GiB",
            Tier::T1.model_ram_gib(),
            measured
        );
    }

    #[test]
    fn leaves_headroom_for_the_os() {
        // Headroom now comes off *total*, not off whatever is free.
        // 8 GiB installed minus 2 GiB headroom = 6 GiB for a model.
        let p = profile(8.0, 5.4, 8, None);
        assert!((p.usable_ram_gib() - 6.0).abs() < 0.01);
        // 8 GiB is still a T1-class machine; the class cap holds it there
        // even though 6 GiB of budget would technically fit T2.
        assert_eq!(p.recommend().tier, Tier::T1);
    }

    #[test]
    fn falls_back_to_remote_when_ram_is_too_small() {
        let r = profile(4.0, 2.5, 4, None).recommend();
        assert_eq!(r.tier, Tier::T0);
        assert!(!r.warnings.is_empty(), "must warn when no local model fits");
    }

    #[test]
    fn recommendation_always_explains_itself() {
        let r = profile(16.0, 12.0, 12, None).recommend();
        assert!(
            !r.reasons.is_empty(),
            "a recommendation must show its reasoning"
        );
    }

    #[test]
    fn warns_on_very_weak_cpu() {
        let r = profile(8.0, 6.0, 2, None).recommend();
        assert!(r.warnings.iter().any(|w| w.contains("cores")));
    }

    #[test]
    fn warns_when_disk_is_too_small() {
        let mut p = profile(32.0, 24.0, 16, None);
        p.free_disk_gib = 2.0;
        let r = p.recommend();
        assert!(r.warnings.iter().any(|w| w.contains("disk")));
    }

    #[test]
    fn simulated_profiles_are_flagged_to_the_user() {
        let r = profile(32.0, 24.0, 16, None).recommend();
        assert!(
            r.warnings.iter().any(|w| w.contains("SIMULATED")),
            "simulated runs must never look like real measurements"
        );
    }

    /* ---------- forced profiles (how we test T2-T4 on an 8 GB laptop) ---------- */

    #[test]
    fn tier_shorthand_spec_produces_that_tier() {
        for t in [Tier::T1, Tier::T2, Tier::T3, Tier::T4] {
            let p = HardwareProfile::from_spec(t.as_str()).unwrap();
            assert!(p.simulated);
            assert_eq!(
                p.recommend().tier,
                t,
                "spec {} must yield {t:?}",
                t.as_str()
            );
        }
    }

    #[test]
    fn long_form_spec_is_parsed() {
        let p = HardwareProfile::from_spec("ram=32,cores=16,vram=12").unwrap();
        assert_eq!(p.total_ram_gib, 32.0);
        assert_eq!(p.cpu_cores, 16);
        assert_eq!(p.gpu_vram_gib, Some(12.0));
        assert!(p.simulated);
    }

    #[test]
    fn spec_defaults_available_to_three_quarters() {
        let p = HardwareProfile::from_spec("ram=16").unwrap();
        assert!((p.available_ram_gib - 12.0).abs() < 0.001);
    }

    #[test]
    fn explicit_available_is_parsed_but_does_not_set_the_tier() {
        // `avail` is still read from the spec and still drives the advisory
        // warning, but it no longer caps the tier. A 32 GiB workstation is a
        // big machine even when the OS is caching most of its memory.
        let p = HardwareProfile::from_spec("ram=32,avail=6").unwrap();
        assert_eq!(p.available_ram_gib, 6.0);
        assert!(
            p.recommend().tier >= Tier::T3,
            "32 GiB installed is a T3+ machine, got {:?}",
            p.recommend().tier
        );
    }

    #[test]
    fn a_busy_workstation_is_warned_not_demoted() {
        // This previously recommended T0 ("connect to a remote engine") for a
        // 32 GiB workstation, purely because 5 GiB happened to be free. That
        // is the bug this whole change exists to fix.
        let p = HardwareProfile::from_spec("ram=32,avail=5").unwrap();
        let r = p.recommend();
        assert_ne!(r.tier, Tier::T0, "must not send a 32 GiB box to remote");
        assert!(
            r.tier >= Tier::T3,
            "32 GiB installed is a T3+ machine, got {:?}",
            r.tier
        );

        // Genuinely tight free memory does warn — 1 GiB free is under the
        // 2.4 GiB a T1 model wants, so the advisory fires.
        let tight = HardwareProfile::from_spec("ram=32,avail=1")
            .unwrap()
            .recommend();
        assert_ne!(tight.tier, Tier::T0, "still a big machine");
        assert!(
            tight.warnings.iter().any(|w| w.contains("free right now")),
            "should warn about tight free memory: {:?}",
            tight.warnings
        );
    }

    #[test]
    fn rejects_malformed_specs() {
        assert!(
            HardwareProfile::from_spec("cores=8").is_err(),
            "ram is required"
        );
        assert!(HardwareProfile::from_spec("ram=abc").is_err());
        assert!(HardwareProfile::from_spec("ram=0").is_err());
        assert!(HardwareProfile::from_spec("nonsense=1").is_err());
        assert!(HardwareProfile::from_spec("ram").is_err());
    }

    /* ---------- tier helpers ---------- */

    #[test]
    fn tier_parsing_is_case_insensitive() {
        assert_eq!(Tier::parse("t3"), Some(Tier::T3));
        assert_eq!(Tier::parse(" T3 "), Some(Tier::T3));
        assert_eq!(Tier::parse("T9"), None);
    }

    #[test]
    fn tiers_step_down_toward_t1() {
        assert_eq!(Tier::T4.step_down(), Some(Tier::T3));
        assert_eq!(Tier::T2.step_down(), Some(Tier::T1));
        assert_eq!(Tier::T1.step_down(), None);
    }

    #[test]
    fn tier_ordering_matches_capability() {
        assert!(Tier::T4 > Tier::T3 && Tier::T3 > Tier::T2 && Tier::T2 > Tier::T1);
    }
}
