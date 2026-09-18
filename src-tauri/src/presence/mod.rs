//! System presence: making Orion reachable from anywhere on the machine.
//!
//! M3 covers the tray icon, the `Win+O` global hotkey, file drop, and single
//! instance behaviour. The exit gate is a **cold start under one second** —
//! not because a second is magic, but because a hotkey assistant that takes
//! three seconds to appear stops being used. The user goes back to alt-tab.
//!
//! The logic here is deliberately separated from Tauri. Everything that can
//! be decided without a window handle lives in `hotkey`, `filedrop`, `window`
//! and `startup`, and is unit-tested. The Tauri layer is a thin shell that
//! calls into it, because a webview cannot be started in CI.

pub mod filedrop;
pub mod hotkey;
pub mod startup;
pub mod window;

// The Tauri layer needs a real desktop session, so it is excluded from the
// scratch-crate test builds that verify the logic above it.
#[cfg(feature = "tauri-glue")]
pub mod tauri_glue;

pub use filedrop::{assess, DropAssessment, RealFs};
pub use hotkey::{Hotkey, Platform};
pub use startup::{Phase, StartupBudget, StartupTrace};
pub use window::{decide, Activation, WindowAction, WindowState};
