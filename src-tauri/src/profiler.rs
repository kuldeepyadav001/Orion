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
    pub fn model_ram_gib(&self) -> f64 {
        match self {
            Tier::T0 => 0.0,
            Tier::T1 => 3.4,
            Tier::T2 => 5.5,
            Tier::T3 => 9.0,
            Tier::T4 => 19.0,
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

        let (gpu_vendor, gpu_vram_gib) = detect_gpu();

        let free_disk_gib = detect_free_disk();

        Self {
            total_ram_gib,
            available_ram_gib,
            cpu_cores,
            cpu_brand,
            gpu_vendor,
            gpu_vram_gib,
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
            free_disk_gib: disk.unwrap_or(200.0),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            simulated: true,
        })
    }

    /// RAM we are willing to give a model right now.
    pub fn usable_ram_gib(&self) -> f64 {
        (self.available_ram_gib - OS_HEADROOM_GIB).max(0.0)
    }

    /// Recommend a tier, with reasoning.
    pub fn recommend(&self) -> TierRecommendation {
        let mut reasons = Vec::new();
        let mut warnings = Vec::new();

        let usable = self.usable_ram_gib();
        reasons.push(format!(
            "{:.1} GiB RAM available of {:.1} GiB total; budgeting {:.1} GiB after a {:.0} GiB \
             headroom for the system.",
            self.available_ram_gib, self.total_ram_gib, usable, OS_HEADROOM_GIB
        ));

        // A discrete GPU with real VRAM lets us punch above the RAM tier,
        // because weights live on the card rather than in system memory.
        let vram = self.gpu_vram_gib.unwrap_or(0.0);
        if vram > 0.0 {
            reasons.push(format!(
                "{} GPU detected with {:.1} GiB VRAM.",
                self.gpu_vendor.as_deref().unwrap_or("discrete"),
                vram
            ));
        } else {
            reasons.push("No discrete GPU detected; inference will run on the CPU.".into());
        }

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
            // Not enough memory for even the smallest local model.
            warnings.push(format!(
                "Only {usable:.1} GiB is free — too little for a local model. Close some \
                 applications, or connect Orion to a remote engine."
            ));
            return TierRecommendation {
                tier: Tier::T0,
                label: Tier::T0.label().into(),
                reasons,
                warnings,
                alternatives: vec![Tier::T1],
            };
        };

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

        if tier == Tier::T1 && vram == 0.0 {
            warnings.push(
                "On CPU-only hardware, voice replies will take several seconds and agent \
                 actions run in propose-only mode."
                    .into(),
            );
        }

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
fn detect_gpu() -> (Option<String>, Option<f64>) {
    // NVIDIA: nvidia-smi is authoritative when present.
    if let Ok(out) = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
    {
        if out.status.success() {
            if let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next() {
                if let Ok(mib) = line.trim().parse::<f64>() {
                    return (Some("NVIDIA".into()), Some(mib / 1024.0));
                }
            }
        }
    }

    // Apple Silicon uses unified memory; VRAM is not a separate pool.
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        return (Some("Apple Silicon".into()), None);
    }

    (None, None)
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
    fn budgets_available_ram_not_total() {
        // A 32 GB machine currently using nearly all of it must not be
        // handed a T3 model just because the sticker says 32 GB.
        let r = profile(32.0, 6.0, 16, None).recommend();
        assert_eq!(r.tier, Tier::T1, "must budget against available RAM");
    }

    #[test]
    fn leaves_headroom_for_the_os() {
        // 5.4 GiB free minus 2 GiB headroom = 3.4 GiB, exactly T1.
        let p = profile(8.0, 5.4, 8, None);
        assert!((p.usable_ram_gib() - 3.4).abs() < 0.01);
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
    fn explicit_available_overrides_the_default() {
        // A 32 GiB workstation that is currently busy: 6 GiB free minus the
        // 2 GiB OS headroom leaves 4 GiB, so only T1 fits right now.
        let p = HardwareProfile::from_spec("ram=32,avail=6").unwrap();
        assert_eq!(p.available_ram_gib, 6.0);
        assert_eq!(p.recommend().tier, Tier::T1);
    }

    #[test]
    fn recommends_remote_when_free_memory_cannot_hold_the_smallest_model() {
        // 5 GiB free - 2 GiB headroom = 3.0 GiB, below T1's 3.4 GiB.
        let p = HardwareProfile::from_spec("ram=32,avail=5").unwrap();
        let r = p.recommend();
        assert_eq!(r.tier, Tier::T0);
        assert!(!r.warnings.is_empty());
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
