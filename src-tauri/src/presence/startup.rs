//! Cold-start budgeting and instrumentation.
//!
//! The M3 exit gate is **window visible in under one second from a cold
//! start**. A hotkey assistant that takes three seconds stops being used —
//! the user goes back to alt-tab and Orion becomes a folder of dead code.
//!
//! A budget only means something if it is measured, so startup is broken into
//! named phases with individual allowances. When the gate fails, the trace
//! says which phase blew it rather than leaving someone to guess.
//!
//! The critical design decision this module encodes: **the model does not
//! load during startup.** `llama-server` takes seconds to memory-map a
//! multi-gigabyte GGUF, and blocking the window on that would lose the gate
//! by an order of magnitude. The window appears, and the engine warms up
//! behind it. The UI shows engine state so that a user who types immediately
//! sees "starting the model…" rather than an input box that silently
//! swallows their message.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A named startup phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Phase {
    /// Process start to Tauri runtime ready.
    RuntimeInit,
    /// Opening SQLite and running migrations.
    Database,
    /// Reading settings, hardware profile, model registry.
    Config,
    /// Registering the tray icon and the global hotkey.
    SystemIntegration,
    /// Creating the webview and painting the first frame.
    WindowShow,
}

impl Phase {
    pub fn label(&self) -> &'static str {
        match self {
            Phase::RuntimeInit => "runtime init",
            Phase::Database => "database",
            Phase::Config => "config",
            Phase::SystemIntegration => "tray and hotkey",
            Phase::WindowShow => "window",
        }
    }

    /// Per-phase allowance. These sum to less than the total so there is
    /// slack for scheduling noise on a loaded machine.
    pub fn budget(&self) -> Duration {
        match self {
            Phase::RuntimeInit => Duration::from_millis(250),
            Phase::Database => Duration::from_millis(150),
            Phase::Config => Duration::from_millis(100),
            Phase::SystemIntegration => Duration::from_millis(150),
            Phase::WindowShow => Duration::from_millis(300),
        }
    }

    pub fn all() -> [Phase; 5] {
        [
            Phase::RuntimeInit,
            Phase::Database,
            Phase::Config,
            Phase::SystemIntegration,
            Phase::WindowShow,
        ]
    }
}

/// The M3 exit gate.
pub const COLD_START_BUDGET: Duration = Duration::from_millis(1000);

/// A warm start — the process is already running and the hotkey is showing an
/// existing window. This must feel instant; anything above ~150 ms reads as
/// lag on a keypress.
pub const WARM_SHOW_BUDGET: Duration = Duration::from_millis(150);

/// Accumulated phase timings for one startup.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StartupTrace {
    entries: Vec<(Phase, Duration)>,
}

impl StartupTrace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, phase: Phase, elapsed: Duration) {
        self.entries.push((phase, elapsed));
    }

    pub fn total(&self) -> Duration {
        self.entries.iter().map(|(_, d)| *d).sum()
    }

    pub fn get(&self, phase: Phase) -> Option<Duration> {
        self.entries
            .iter()
            .find(|(p, _)| *p == phase)
            .map(|(_, d)| *d)
    }

    /// Phases that exceeded their individual allowance.
    pub fn over_budget(&self) -> Vec<(Phase, Duration, Duration)> {
        self.entries
            .iter()
            .filter(|(p, d)| *d > p.budget())
            .map(|(p, d)| (*p, *d, p.budget()))
            .collect()
    }

    pub fn within_gate(&self) -> bool {
        self.total() <= COLD_START_BUDGET
    }

    /// Human-readable report for logs and the M3 evidence document.
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("cold start:\n");
        for (phase, d) in &self.entries {
            let budget = phase.budget();
            let flag = if *d > budget { "  OVER" } else { "" };
            s.push_str(&format!(
                "  {:<18} {:>6.1} ms  (budget {:>5.1} ms){}\n",
                phase.label(),
                d.as_secs_f64() * 1000.0,
                budget.as_secs_f64() * 1000.0,
                flag
            ));
        }
        s.push_str(&format!(
            "  {:<18} {:>6.1} ms  (gate   {:>5.1} ms) {}\n",
            "TOTAL",
            self.total().as_secs_f64() * 1000.0,
            COLD_START_BUDGET.as_secs_f64() * 1000.0,
            if self.within_gate() { "PASS" } else { "FAIL" }
        ));
        s
    }
}

/// Static check that the phase budgets are internally consistent.
///
/// This is a design guard, not a runtime measurement: if someone raises a
/// phase budget until the sum exceeds the gate, the gate has been quietly
/// abandoned. Better to fail a test than to discover it in a release.
pub struct StartupBudget;

impl StartupBudget {
    pub fn sum_of_phases() -> Duration {
        Phase::all().iter().map(|p| p.budget()).sum()
    }

    pub fn headroom() -> Duration {
        COLD_START_BUDGET.saturating_sub(Self::sum_of_phases())
    }
}

/// Work that must **not** happen before the window is shown.
///
/// Encoded as data so the rule is testable and so a future contributor adding
/// "just one quick thing" to startup has to confront it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredWork {
    /// Memory-mapping a multi-gigabyte GGUF. Seconds, not milliseconds.
    ModelLoad,
    /// Spawning and health-checking the llama-server sidecar.
    EngineSpawn,
    /// Building or verifying the document index.
    IndexWarmup,
    /// Checking for updates. Network work never blocks the UI, and in an
    /// offline-first product it must never be on the startup path at all.
    UpdateCheck,
}

impl DeferredWork {
    pub fn all() -> [DeferredWork; 4] {
        [
            DeferredWork::ModelLoad,
            DeferredWork::EngineSpawn,
            DeferredWork::IndexWarmup,
            DeferredWork::UpdateCheck,
        ]
    }

    pub fn why(&self) -> &'static str {
        match self {
            DeferredWork::ModelLoad => {
                "mapping a multi-GB model takes seconds; the window must not wait for it"
            }
            DeferredWork::EngineSpawn => {
                "the sidecar health check involves retries; it warms up behind the window"
            }
            DeferredWork::IndexWarmup => {
                "index work scales with the user's corpus and is unbounded at startup"
            }
            DeferredWork::UpdateCheck => {
                "network work must never block the UI, and Orion is offline-first"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /* ---------- budget consistency ---------- */

    #[test]
    fn phase_budgets_fit_inside_the_gate() {
        let sum = StartupBudget::sum_of_phases();
        assert!(
            sum <= COLD_START_BUDGET,
            "phase budgets total {:?}, which exceeds the {:?} gate",
            sum,
            COLD_START_BUDGET
        );
    }

    #[test]
    fn there_is_slack_for_scheduling_noise() {
        // A budget with zero headroom fails on any loaded machine, which
        // means it will be ignored rather than fixed.
        assert!(
            StartupBudget::headroom() >= ms(50),
            "only {:?} of headroom",
            StartupBudget::headroom()
        );
    }

    #[test]
    fn the_gate_is_one_second_as_specified_by_m3() {
        assert_eq!(COLD_START_BUDGET, ms(1000));
    }

    #[test]
    fn every_phase_has_a_nonzero_budget_and_a_label() {
        for p in Phase::all() {
            assert!(p.budget() > Duration::ZERO, "{p:?} has no budget");
            assert!(!p.label().is_empty());
        }
    }

    #[test]
    fn a_warm_show_is_far_stricter_than_a_cold_start() {
        assert!(WARM_SHOW_BUDGET < COLD_START_BUDGET / 4);
    }

    /* ---------- trace behaviour ---------- */

    #[test]
    fn a_fast_startup_passes_the_gate() {
        let mut t = StartupTrace::new();
        t.record(Phase::RuntimeInit, ms(120));
        t.record(Phase::Database, ms(30));
        t.record(Phase::Config, ms(10));
        t.record(Phase::SystemIntegration, ms(60));
        t.record(Phase::WindowShow, ms(180));

        assert!(t.within_gate(), "{}", t.render());
        assert_eq!(t.total(), ms(400));
        assert!(t.over_budget().is_empty());
    }

    #[test]
    fn a_slow_startup_fails_the_gate() {
        let mut t = StartupTrace::new();
        t.record(Phase::RuntimeInit, ms(200));
        // The classic mistake: loading the model on the startup path.
        t.record(Phase::Config, ms(3000));
        assert!(!t.within_gate());
        assert!(t.render().contains("FAIL"));
    }

    #[test]
    fn the_trace_names_the_phase_that_blew_its_budget() {
        let mut t = StartupTrace::new();
        t.record(Phase::RuntimeInit, ms(100));
        t.record(Phase::Database, ms(900));

        let over = t.over_budget();
        assert_eq!(over.len(), 1);
        assert_eq!(over[0].0, Phase::Database);
        assert!(t.render().contains("OVER"));
        assert!(t.render().contains("database"));
    }

    #[test]
    fn a_trace_can_pass_the_gate_while_one_phase_is_over() {
        // Worth knowing about even when the total is fine, because it means
        // the margin has quietly moved.
        let mut t = StartupTrace::new();
        t.record(Phase::Database, ms(200)); // budget 150
        assert!(t.within_gate());
        assert_eq!(t.over_budget().len(), 1);
    }

    #[test]
    fn an_empty_trace_is_zero_not_a_panic() {
        let t = StartupTrace::new();
        assert_eq!(t.total(), Duration::ZERO);
        assert!(t.within_gate());
        assert!(t.get(Phase::Database).is_none());
    }

    #[test]
    fn the_report_is_readable_and_complete() {
        let mut t = StartupTrace::new();
        for p in Phase::all() {
            t.record(p, ms(50));
        }
        let r = t.render();
        for p in Phase::all() {
            assert!(r.contains(p.label()), "{} missing from report", p.label());
        }
        assert!(r.contains("TOTAL"));
        assert!(r.contains("PASS"));
    }

    /* ---------- the deferral rule ---------- */

    #[test]
    fn the_model_is_never_loaded_on_the_startup_path() {
        // The single most important startup decision. If this list loses
        // ModelLoad, the one-second gate is unachievable.
        assert!(DeferredWork::all().contains(&DeferredWork::ModelLoad));
        assert!(DeferredWork::all().contains(&DeferredWork::EngineSpawn));
    }

    #[test]
    fn no_network_work_happens_during_startup() {
        // Offline-first: an update check on the startup path would both slow
        // the window and contradict the product's central claim.
        assert!(DeferredWork::all().contains(&DeferredWork::UpdateCheck));
    }

    #[test]
    fn every_deferred_item_explains_itself() {
        for w in DeferredWork::all() {
            assert!(w.why().len() > 20, "{w:?} has no real justification");
        }
    }

    #[test]
    fn the_startup_phases_do_not_include_deferred_work() {
        // A phase named after deferred work would mean it is back on the
        // critical path.
        let labels: Vec<&str> = Phase::all().iter().map(|p| p.label()).collect();
        for bad in ["model", "engine", "sidecar", "update", "index"] {
            assert!(
                !labels.iter().any(|l| l.contains(bad)),
                "phase list mentions {bad:?}, which should be deferred"
            );
        }
    }
}
