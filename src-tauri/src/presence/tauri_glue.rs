//! The thin Tauri layer over the tested presence logic.
//!
//! Everything here needs a real window, a real tray and a real desktop
//! session, so **none of it can be unit-tested in CI**. That is precisely why
//! it is thin: every decision is made in `window`, `hotkey`, `filedrop` or
//! `startup`, which are pure and tested. This file only translates those
//! decisions into Tauri calls.
//!
//! If you find yourself adding an `if` here, it probably belongs in one of
//! those modules instead.

use std::time::Instant;

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime, WebviewWindow,
};

use super::hotkey::{self, Hotkey};
use super::startup::{Phase, StartupTrace};
use super::window::{decide, Activation, TrayItem, WindowAction, WindowState};

pub const MAIN_WINDOW: &str = "main";

/// Read the current window state for the decision function.
pub fn read_state<R: Runtime>(window: &WebviewWindow<R>) -> WindowState {
    WindowState {
        visible: window.is_visible().unwrap_or(false),
        focused: window.is_focused().unwrap_or(false),
        minimised: window.is_minimized().unwrap_or(false),
    }
}

/// Apply a decided action to a window.
pub fn apply<R: Runtime>(window: &WebviewWindow<R>, action: WindowAction) {
    match action {
        WindowAction::ShowAndFocus => {
            // Order matters: unminimise before show, or some window managers
            // restore the window off-screen.
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        WindowAction::FocusOnly => {
            let _ = window.set_focus();
        }
        WindowAction::Hide => {
            let _ = window.hide();
        }
        WindowAction::Nothing => {}
    }
}

/// Handle any activation source in one place.
pub fn activate<R: Runtime>(app: &AppHandle<R>, source: Activation) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        tracing::warn!("activation with no main window");
        return;
    };
    let state = read_state(&window);
    let action = decide(state, source);
    tracing::debug!(?source, ?state, ?action, "window activation");
    apply(&window, action);
}

/// Build the tray icon and menu.
/// Build the tray icon and menu.
///
/// Ownership here is subtle and I got it wrong twice, so it is written down.
///
/// `TrayIconBuilder::build` calls `icon.register(...)` internally, which
/// stores the icon in the app's resource table, and `TrayIconBuilder::menu`
/// takes the menu by `inner_context_owned()`. **Tauri owns both** from the
/// moment `build()` returns, and releases them in `cleanup_before_exit`,
/// which clears `manager.tray.icons` and every resource table.
///
/// So the correct thing is to build and let the local handles drop. Keeping a
/// copy in `AppState` is actively harmful: Tauri drops its reference during
/// cleanup, then the managed copy drops afterwards against already-freed
/// platform state, and the refcount assertion in `alloc::rc` aborts the
/// process with 0xc0000409.
///
/// That is exactly the crash this function caused in its previous form, and
/// the "fix" of returning a handle to `app.manage()` reproduced it from the
/// other direction.
pub fn build_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let mut items: Vec<Box<dyn tauri::menu::IsMenuItem<R>>> = Vec::new();

    for item in super::window::tray_menu() {
        if item.is_separator() {
            items.push(Box::new(PredefinedMenuItem::separator(app)?));
        } else {
            items.push(Box::new(MenuItem::with_id(
                app,
                item.id(),
                item.label(),
                true,
                None::<&str>,
            )?));
        }
    }

    let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = items.iter().map(|b| b.as_ref()).collect();
    let menu = Menu::with_items(app, &refs)?;

    TrayIconBuilder::with_id("orion-tray")
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or_else(|| tauri::Error::AssetNotFound("default window icon missing".into()))?,
        )
        .tooltip("Orion")
        .menu(&menu)
        // The menu must not open on a left click, or the toggle never fires.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            id if id == TrayItem::Open.id() => activate(app, Activation::TrayMenuOpen),
            id if id == TrayItem::NewChat.id() => {
                activate(app, Activation::TrayMenuOpen);
                let _ = emit_all(app, "tray://new-chat");
            }
            id if id == TrayItem::AddFiles.id() => {
                activate(app, Activation::TrayMenuOpen);
                let _ = emit_all(app, "tray://add-files");
            }
            id if id == TrayItem::Settings.id() => {
                activate(app, Activation::TrayMenuOpen);
                let _ = emit_all(app, "tray://settings");
            }
            id if id == TrayItem::Quit.id() => {
                tracing::info!("quit requested from tray");
                app.exit(0);
            }
            other => tracing::warn!(id = other, "unknown tray menu id"),
        })
        .on_tray_icon_event(|tray, event| {
            // Left click up only. Down-events fire twice on some platforms.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                activate(tray.app_handle(), Activation::TrayClick);
            }
        })
        .build(app)?;

    Ok(())
}

fn emit_all<R: Runtime>(app: &AppHandle<R>, event: &str) -> tauri::Result<()> {
    use tauri::Emitter;
    app.emit(event, ())
}

/// Register the global hotkey, falling back through the chain when the
/// preferred chord is already owned by another application.
///
/// Returns the chord that was actually registered, or `None` when every
/// candidate failed. A failure here is **not** fatal — Orion still works from
/// the tray — but it must be surfaced, because the user was promised `Win+O`
/// and silence would leave them pressing a dead key.
pub fn register_hotkey<R: Runtime>(
    app: &AppHandle<R>,
    preferred: Option<Hotkey>,
) -> Option<Hotkey> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    let mut candidates = Vec::new();
    if let Some(p) = preferred {
        candidates.push(p);
    }
    for fallback in hotkey::fallback_chain() {
        if !candidates.contains(&fallback) {
            candidates.push(fallback);
        }
    }

    for chord in candidates {
        let accelerator = chord.to_accelerator();
        let handle = app.clone();

        let result = app.global_shortcut().on_shortcut(
            accelerator.as_str(),
            move |_app, _shortcut, event| {
                // Fire on press, not release: a chord delivers both events,
                // and reacting to each would toggle twice per keypress.
                //
                // `state` is a public field, not a method — ShortcutEvent is
                // a re-export of global_hotkey::GlobalHotKeyEvent.
                if event.state == ShortcutState::Pressed {
                    activate(&handle, Activation::Hotkey);
                }
            },
        );

        match result {
            Ok(()) => {
                tracing::info!(chord = %chord.canonical(), "global hotkey registered");
                return Some(chord);
            }
            Err(e) => {
                tracing::warn!(
                    chord = %chord.canonical(),
                    error = %e,
                    "hotkey unavailable, trying the next candidate"
                );
            }
        }
    }

    tracing::error!("no global hotkey could be registered; tray only");
    None
}

/// Time a startup phase into the trace.
pub fn timed<T, F: FnOnce() -> T>(trace: &mut StartupTrace, phase: Phase, f: F) -> T {
    let start = Instant::now();
    let out = f();
    trace.record(phase, start.elapsed());
    out
}
