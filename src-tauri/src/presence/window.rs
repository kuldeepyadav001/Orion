//! Window visibility state machine for tray and hotkey interaction.
//!
//! The behaviour looks trivial and is not. `Win+O` has to do the right thing
//! from four different starting states, and the obvious implementation
//! ("toggle visible") is wrong in two of them:
//!
//! * Window hidden          → show and focus.
//! * Window visible, focused → hide. (Press again to dismiss.)
//! * Window visible, **not** focused → focus it. **Not** hide it.
//! * Window minimised       → restore and focus.
//!
//! The third case is the one that gets shipped broken everywhere. If the user
//! is in their browser and presses the hotkey, they want Orion in front of
//! them. Treating "visible" as "hide it" means the window is technically on
//! screen behind three other windows, the hotkey hides it, and the user
//! presses again to get it back — a visible flicker and two presses for what
//! should be one.

use serde::{Deserialize, Serialize};

/// What the window is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    pub visible: bool,
    pub focused: bool,
    pub minimised: bool,
}

impl WindowState {
    pub fn hidden() -> Self {
        Self {
            visible: false,
            focused: false,
            minimised: false,
        }
    }
    pub fn foreground() -> Self {
        Self {
            visible: true,
            focused: true,
            minimised: false,
        }
    }
    pub fn background() -> Self {
        Self {
            visible: true,
            focused: false,
            minimised: false,
        }
    }
    pub fn minimised() -> Self {
        Self {
            visible: false,
            focused: false,
            minimised: true,
        }
    }
}

/// What the app should do in response to an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowAction {
    /// Make visible, unminimise, raise and focus.
    ShowAndFocus,
    /// Raise and focus without changing visibility.
    FocusOnly,
    /// Hide to tray. The process keeps running.
    Hide,
    /// Nothing to do.
    Nothing,
}

/// Where an activation came from. The source changes the correct response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// The global hotkey. Toggles.
    Hotkey,
    /// Left click on the tray icon. Toggles, like the hotkey.
    TrayClick,
    /// "Open Orion" in the tray menu. Always shows — the user picked an
    /// explicit verb, so hiding would be perverse.
    TrayMenuOpen,
    /// A second instance was launched, e.g. the user clicked the desktop
    /// icon again. Always shows: they asked for the app.
    SecondInstance,
    /// Files were dropped or passed on the command line. Always shows, so the
    /// user can see what happened to them.
    FileDrop,
}

/// Decide what to do.
pub fn decide(state: WindowState, activation: Activation) -> WindowAction {
    match activation {
        // Explicit "open" verbs never hide.
        Activation::TrayMenuOpen | Activation::SecondInstance | Activation::FileDrop => {
            if state.visible && state.focused && !state.minimised {
                WindowAction::Nothing
            } else {
                WindowAction::ShowAndFocus
            }
        }

        // Toggling inputs.
        Activation::Hotkey | Activation::TrayClick => {
            // Minimised or hidden both mean "not in front of the user".
            if state.minimised || !state.visible {
                WindowAction::ShowAndFocus
            } else if state.focused {
                WindowAction::Hide
            } else {
                // Visible but behind other windows: bring it forward.
                WindowAction::FocusOnly
            }
        }
    }
}

/// What closing the window should do.
///
/// Orion is a tray-resident assistant, so the close button hides rather than
/// quits — otherwise the hotkey stops working and the user has to relaunch,
/// which defeats G5. Quitting is available from the tray menu.
///
/// This surprises people the first time, so the UI shows a one-off notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseBehaviour {
    HideToTray { notify_first_time: bool },
    Quit,
}

pub fn on_close_requested(has_shown_tray_notice: bool) -> CloseBehaviour {
    CloseBehaviour::HideToTray {
        notify_first_time: !has_shown_tray_notice,
    }
}

/// Tray menu items. Kept as data so the labels are testable and so the menu
/// cannot drift out of sync with the commands it triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayItem {
    Open,
    NewChat,
    AddFiles,
    Separator,
    Settings,
    Quit,
}

impl TrayItem {
    pub fn id(&self) -> &'static str {
        match self {
            TrayItem::Open => "open",
            TrayItem::NewChat => "new_chat",
            TrayItem::AddFiles => "add_files",
            TrayItem::Separator => "separator",
            TrayItem::Settings => "settings",
            TrayItem::Quit => "quit",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            TrayItem::Open => "Open Orion",
            TrayItem::NewChat => "New chat",
            TrayItem::AddFiles => "Add files…",
            TrayItem::Separator => "",
            TrayItem::Settings => "Settings",
            TrayItem::Quit => "Quit Orion",
        }
    }

    pub fn is_separator(&self) -> bool {
        matches!(self, TrayItem::Separator)
    }
}

/// The tray menu, in order.
pub fn tray_menu() -> Vec<TrayItem> {
    vec![
        TrayItem::Open,
        TrayItem::NewChat,
        TrayItem::AddFiles,
        TrayItem::Separator,
        TrayItem::Settings,
        TrayItem::Quit,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /* ---------- the four states ---------- */

    #[test]
    fn hotkey_shows_a_hidden_window() {
        assert_eq!(
            decide(WindowState::hidden(), Activation::Hotkey),
            WindowAction::ShowAndFocus
        );
    }

    #[test]
    fn hotkey_hides_a_focused_window() {
        assert_eq!(
            decide(WindowState::foreground(), Activation::Hotkey),
            WindowAction::Hide
        );
    }

    #[test]
    fn hotkey_focuses_a_visible_but_unfocused_window() {
        // The case that is usually shipped broken. The user is in their
        // browser; the hotkey must bring Orion forward, not hide it.
        assert_eq!(
            decide(WindowState::background(), Activation::Hotkey),
            WindowAction::FocusOnly,
        );
    }

    #[test]
    fn hotkey_restores_a_minimised_window() {
        assert_eq!(
            decide(WindowState::minimised(), Activation::Hotkey),
            WindowAction::ShowAndFocus
        );
    }

    #[test]
    fn pressing_the_hotkey_twice_returns_to_the_start() {
        // show → hide, the property a toggle must have.
        let first = decide(WindowState::hidden(), Activation::Hotkey);
        assert_eq!(first, WindowAction::ShowAndFocus);
        let after = WindowState::foreground();
        assert_eq!(decide(after, Activation::Hotkey), WindowAction::Hide);
    }

    /* ---------- tray click matches the hotkey ---------- */

    #[test]
    fn tray_click_behaves_exactly_like_the_hotkey() {
        for state in [
            WindowState::hidden(),
            WindowState::foreground(),
            WindowState::background(),
            WindowState::minimised(),
        ] {
            assert_eq!(
                decide(state, Activation::TrayClick),
                decide(state, Activation::Hotkey),
                "divergence at {state:?}"
            );
        }
    }

    /* ---------- explicit open verbs ---------- */

    #[test]
    fn explicit_open_never_hides() {
        for activation in [
            Activation::TrayMenuOpen,
            Activation::SecondInstance,
            Activation::FileDrop,
        ] {
            for state in [
                WindowState::hidden(),
                WindowState::foreground(),
                WindowState::background(),
                WindowState::minimised(),
            ] {
                let action = decide(state, activation);
                assert_ne!(
                    action,
                    WindowAction::Hide,
                    "{activation:?} hid the window from {state:?}"
                );
            }
        }
    }

    #[test]
    fn explicit_open_on_an_already_focused_window_does_nothing() {
        assert_eq!(
            decide(WindowState::foreground(), Activation::TrayMenuOpen),
            WindowAction::Nothing
        );
    }

    #[test]
    fn explicit_open_raises_a_background_window() {
        assert_eq!(
            decide(WindowState::background(), Activation::SecondInstance),
            WindowAction::ShowAndFocus
        );
    }

    #[test]
    fn dropping_files_always_surfaces_the_window() {
        // The user must be able to see whether their files were accepted.
        for state in [
            WindowState::hidden(),
            WindowState::background(),
            WindowState::minimised(),
        ] {
            assert_eq!(
                decide(state, Activation::FileDrop),
                WindowAction::ShowAndFocus,
                "from {state:?}"
            );
        }
    }

    /* ---------- close behaviour ---------- */

    #[test]
    fn closing_hides_to_tray_rather_than_quitting() {
        // Quitting would kill the global hotkey, breaking gate G5.
        match on_close_requested(false) {
            CloseBehaviour::HideToTray { notify_first_time } => {
                assert!(notify_first_time, "the first close must explain itself");
            }
            CloseBehaviour::Quit => panic!("close must not quit"),
        }
    }

    #[test]
    fn the_tray_notice_is_shown_only_once() {
        match on_close_requested(true) {
            CloseBehaviour::HideToTray { notify_first_time } => assert!(!notify_first_time),
            CloseBehaviour::Quit => panic!("close must not quit"),
        }
    }

    /* ---------- tray menu ---------- */

    #[test]
    fn the_tray_builder_does_not_discard_its_handle() {
        // Structural guard for a crash that reached a user's machine.
        //
        // Tauri's TrayIcon is reference-counted and its docs say plainly:
        // "the icon is removed when the last instance is dropped". build_tray
        // returned Ok(()), so both the icon and its Menu were dropped at the
        // end of the function. On Windows that aborted the process during
        // startup with a refcount violation inside alloc::rc, exit code
        // 0xc0000409.
        //
        // Nothing caught it because every test here covers the pure decision
        // logic, and the Tauri layer cannot be instantiated without a desktop
        // session. A source-level check is crude but it is the only kind
        // available, and it encodes the rule that was broken.
        let glue = include_str!("tauri_glue.rs");

        assert!(
            glue.contains("pub struct TrayHandle"),
            "build_tray must return a handle that keeps the tray and menu alive"
        );
        assert!(
            !glue
                .contains("pub fn build_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()>"),
            "build_tray returns Ok(()), which drops the refcounted TrayIcon \
             and removes the tray"
        );

        // And the caller must actually retain it.
        let lib = include_str!("../lib.rs");
        assert!(
            lib.contains("app.manage(tray)"),
            "the tray handle must be managed by the app, not dropped at the \
             end of setup()"
        );
    }

    #[test]
    fn the_tray_menu_has_open_first_and_quit_last() {
        let menu = tray_menu();
        assert_eq!(menu.first(), Some(&TrayItem::Open));
        assert_eq!(menu.last(), Some(&TrayItem::Quit));
    }

    #[test]
    fn tray_menu_ids_are_unique() {
        let ids: Vec<&str> = tray_menu()
            .iter()
            .filter(|i| !i.is_separator())
            .map(|i| i.id())
            .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate tray item id");
    }

    #[test]
    fn every_tray_item_has_a_label_except_separators() {
        for item in tray_menu() {
            if item.is_separator() {
                assert!(item.label().is_empty());
            } else {
                assert!(!item.label().is_empty(), "{item:?} has no label");
                assert!(!item.id().is_empty());
            }
        }
    }

    #[test]
    fn quitting_is_reachable_from_the_tray() {
        // Since the close button only hides, there must be a real way out or
        // the user has to kill the process.
        assert!(tray_menu().contains(&TrayItem::Quit));
    }
}
