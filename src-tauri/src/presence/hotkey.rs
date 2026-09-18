//! Global hotkey parsing and validation.
//!
//! G5 locks `Win+O` as the always-on global hotkey. This module owns the
//! parsing, validation and conflict rules so the Tauri registration code is a
//! thin shell that cannot be unit-tested in this sandbox.
//!
//! A global hotkey is a system-wide capture: whatever the user is doing, that
//! chord goes to Orion instead of the focused app. Getting this wrong is not
//! cosmetic — binding a bare letter or a common editing chord makes the user's
//! machine feel broken, and they will not connect it to us.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A parsed hotkey chord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotkey {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// Windows key / Command / Super.
    pub meta: bool,
    pub key: Key,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Key {
    Char(char),
    Function(u8),
    Space,
    Enter,
    Escape,
    Tab,
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Char(c) => write!(f, "{}", c.to_ascii_uppercase()),
            Key::Function(n) => write!(f, "F{n}"),
            Key::Space => write!(f, "Space"),
            Key::Enter => write!(f, "Enter"),
            Key::Escape => write!(f, "Escape"),
            Key::Tab => write!(f, "Tab"),
        }
    }
}

/// Why a chord was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    Empty,
    /// No modifier at all — would swallow a plain keystroke system-wide.
    NoModifier,
    /// Reserved by the OS or so common that stealing it breaks the desktop.
    Reserved(&'static str),
    UnknownKey(String),
    UnknownModifier(String),
    /// Modifiers but no actual key.
    NoKey,
}

impl fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HotkeyError::Empty => write!(f, "no shortcut given"),
            HotkeyError::NoModifier => write!(
                f,
                "a global shortcut needs at least one modifier \
                 (Ctrl, Alt, Shift or Win), otherwise it captures that key everywhere"
            ),
            HotkeyError::Reserved(why) => write!(f, "that shortcut is reserved: {why}"),
            HotkeyError::UnknownKey(k) => write!(f, "unrecognised key: {k}"),
            HotkeyError::UnknownModifier(m) => write!(f, "unrecognised modifier: {m}"),
            HotkeyError::NoKey => write!(f, "a shortcut needs a key, not just modifiers"),
        }
    }
}

impl Hotkey {
    /// The G5 default: `Win+O`.
    ///
    /// Chosen because Win+letter combinations are largely unclaimed on
    /// Windows outside a small reserved set, and `O` is mnemonic for Orion.
    pub fn default_global() -> Self {
        Hotkey {
            ctrl: false,
            shift: false,
            alt: false,
            meta: true,
            key: Key::Char('O'),
        }
    }

    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.shift || self.alt || self.meta
    }

    /// Canonical string, always in the same modifier order so that two
    /// spellings of one chord compare equal in config and in tests.
    pub fn canonical(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.meta {
            parts.push("Super".to_string());
        }
        parts.push(self.key.to_string());
        parts.join("+")
    }

    /// Format for Tauri's global-shortcut plugin.
    pub fn to_accelerator(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("CommandOrControl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.meta {
            parts.push("Super".to_string());
        }
        parts.push(match &self.key {
            Key::Char(c) => format!("Key{}", c.to_ascii_uppercase()),
            Key::Function(n) => format!("F{n}"),
            Key::Space => "Space".to_string(),
            Key::Enter => "Enter".to_string(),
            Key::Escape => "Escape".to_string(),
            Key::Tab => "Tab".to_string(),
        });
        parts.join("+")
    }

    /// Human-readable, platform-appropriate label for the UI.
    pub fn display_for(&self, platform: Platform) -> String {
        let mut parts = Vec::new();
        match platform {
            Platform::MacOs => {
                if self.ctrl {
                    parts.push("⌃".to_string());
                }
                if self.alt {
                    parts.push("⌥".to_string());
                }
                if self.shift {
                    parts.push("⇧".to_string());
                }
                if self.meta {
                    parts.push("⌘".to_string());
                }
                return format!("{}{}", parts.join(""), self.key);
            }
            Platform::Windows => {
                if self.ctrl {
                    parts.push("Ctrl".to_string());
                }
                if self.alt {
                    parts.push("Alt".to_string());
                }
                if self.shift {
                    parts.push("Shift".to_string());
                }
                if self.meta {
                    parts.push("Win".to_string());
                }
            }
            Platform::Linux => {
                if self.ctrl {
                    parts.push("Ctrl".to_string());
                }
                if self.alt {
                    parts.push("Alt".to_string());
                }
                if self.shift {
                    parts.push("Shift".to_string());
                }
                if self.meta {
                    parts.push("Super".to_string());
                }
            }
        }
        parts.push(self.key.to_string());
        parts.join("+")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    MacOs,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
}

/// Parse a chord such as `"Ctrl+Shift+O"` or `"Win+O"`.
///
/// Accepts the many spellings users and config files actually contain, then
/// validates. Being liberal in parsing and strict in validation is deliberate:
/// a typo should produce a clear message, not a silently different binding.
pub fn parse(input: &str) -> Result<Hotkey, HotkeyError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(HotkeyError::Empty);
    }

    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut meta = false;
    let mut key: Option<Key> = None;

    for raw in input.split('+') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let lower = token.to_ascii_lowercase();

        match lower.as_str() {
            "ctrl" | "control" | "ctl" | "commandorcontrol" | "cmdorctrl" => ctrl = true,
            "shift" => shift = true,
            "alt" | "option" | "opt" => alt = true,
            "win" | "super" | "meta" | "cmd" | "command" | "windows" => meta = true,
            _ => {
                let parsed = parse_key(token)?;
                // A second key means the chord is malformed; keep the first
                // and treat the rest as an error rather than guessing.
                if key.is_some() {
                    return Err(HotkeyError::UnknownKey(token.to_string()));
                }
                key = Some(parsed);
            }
        }
    }

    let key = key.ok_or(HotkeyError::NoKey)?;
    let hk = Hotkey {
        ctrl,
        shift,
        alt,
        meta,
        key,
    };

    validate(&hk)?;
    Ok(hk)
}

fn parse_key(token: &str) -> Result<Key, HotkeyError> {
    let lower = token.to_ascii_lowercase();

    // Function keys
    if let Some(rest) = lower.strip_prefix('f') {
        if let Ok(n) = rest.parse::<u8>() {
            if (1..=24).contains(&n) {
                return Ok(Key::Function(n));
            }
            return Err(HotkeyError::UnknownKey(token.to_string()));
        }
    }

    match lower.as_str() {
        "space" | "spacebar" => return Ok(Key::Space),
        "enter" | "return" => return Ok(Key::Enter),
        "esc" | "escape" => return Ok(Key::Escape),
        "tab" => return Ok(Key::Tab),
        _ => {}
    }

    let mut chars = token.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => Ok(Key::Char(c.to_ascii_uppercase())),
        _ => Err(HotkeyError::UnknownKey(token.to_string())),
    }
}

/// Reject chords that are unsafe to capture globally.
pub fn validate(hk: &Hotkey) -> Result<(), HotkeyError> {
    if !hk.has_modifier() {
        return Err(HotkeyError::NoModifier);
    }

    // Shift alone is not a real modifier for this purpose: Shift+K is just K.
    if hk.shift && !hk.ctrl && !hk.alt && !hk.meta {
        if let Key::Char(_) = hk.key {
            return Err(HotkeyError::NoModifier);
        }
    }

    let c = match hk.key {
        Key::Char(c) => Some(c.to_ascii_uppercase()),
        _ => None,
    };

    // OS-reserved and desktop-critical chords. Capturing any of these makes
    // the machine feel broken in a way the user will not attribute to us.
    if hk.ctrl && hk.alt && c == Some('D') {
        return Err(HotkeyError::Reserved("Ctrl+Alt+D is used by some desktops"));
    }
    // Ctrl+Shift+Esc, not Ctrl+Shift+E. The first draft blocked the letter,
    // which both refused a perfectly good chord and missed the real one.
    if hk.ctrl && hk.shift && hk.key == Key::Escape {
        return Err(HotkeyError::Reserved("Ctrl+Shift+Esc opens Task Manager"));
    }
    if hk.ctrl && hk.alt && hk.key == Key::Tab {
        return Err(HotkeyError::Reserved("Ctrl+Alt+Tab is a window switcher"));
    }
    if hk.alt && hk.key == Key::Tab {
        return Err(HotkeyError::Reserved("Alt+Tab switches windows"));
    }
    if hk.alt && hk.key == Key::Escape {
        return Err(HotkeyError::Reserved("Alt+Esc cycles windows"));
    }
    if hk.ctrl && hk.alt && hk.key == Key::Escape {
        return Err(HotkeyError::Reserved("Ctrl+Alt+Esc is reserved"));
    }
    if hk.meta && !hk.ctrl && !hk.alt && !hk.shift {
        // Win+letter is mostly free, but a handful are taken by Windows.
        if let Some(c) = c {
            const WINDOWS_RESERVED: &[(char, &str)] = &[
                ('L', "Win+L locks the workstation"),
                ('D', "Win+D shows the desktop"),
                ('E', "Win+E opens File Explorer"),
                ('R', "Win+R opens Run"),
                ('X', "Win+X opens the power-user menu"),
                ('I', "Win+I opens Settings"),
                ('S', "Win+S opens Search"),
                ('A', "Win+A opens Action Center"),
                ('P', "Win+P opens display projection"),
                ('U', "Win+U opens Accessibility settings"),
                ('V', "Win+V opens clipboard history"),
                ('G', "Win+G opens Game Bar"),
            ];
            if let Some((_, why)) = WINDOWS_RESERVED.iter().find(|(k, _)| *k == c) {
                return Err(HotkeyError::Reserved(why));
            }
        }
    }
    // Ctrl+Alt+Del is intercepted below the application layer everywhere.
    if hk.ctrl && hk.alt && c == Some('\u{7f}') {
        return Err(HotkeyError::Reserved("Ctrl+Alt+Del is handled by the OS"));
    }

    Ok(())
}

/// Fallbacks tried in order when the preferred chord cannot be registered,
/// which happens when another application already owns it.
///
/// Registration failure is normal, not exceptional — the user may well be
/// running something that took `Win+O` first. Silently doing nothing would
/// leave the headline feature dead with no explanation.
pub fn fallback_chain() -> Vec<Hotkey> {
    vec![
        Hotkey::default_global(),
        parse("Ctrl+Shift+O").expect("static fallback must be valid"),
        parse("Ctrl+Alt+O").expect("static fallback must be valid"),
        parse("Ctrl+Shift+Space").expect("static fallback must be valid"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /* ---------- the locked default ---------- */

    #[test]
    fn the_default_is_win_o_per_gate_g5() {
        let hk = Hotkey::default_global();
        assert!(hk.meta, "G5 specifies the Windows key");
        assert_eq!(hk.key, Key::Char('O'));
        assert!(!hk.ctrl && !hk.alt && !hk.shift);
    }

    #[test]
    fn the_default_survives_its_own_validation() {
        // An embarrassing but easy mistake: adding Win+O to the reserved list.
        assert!(validate(&Hotkey::default_global()).is_ok());
    }

    #[test]
    fn every_fallback_is_valid_and_distinct() {
        let chain = fallback_chain();
        assert!(chain.len() >= 3, "need real alternatives");
        for hk in &chain {
            assert!(validate(hk).is_ok(), "invalid fallback: {}", hk.canonical());
        }
        let mut seen: Vec<String> = chain.iter().map(|h| h.canonical()).collect();
        let before = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), before, "duplicate fallback in the chain");
    }

    #[test]
    fn the_first_fallback_is_the_default() {
        assert_eq!(fallback_chain()[0], Hotkey::default_global());
    }

    /* ---------- parsing ---------- */

    #[test]
    fn common_spellings_all_parse_to_the_same_chord() {
        let expect = Hotkey::default_global();
        for s in ["Win+O", "win+o", "Super+O", "Meta+O", "Cmd+O", " WIN + O "] {
            assert_eq!(parse(s).unwrap(), expect, "failed on {s:?}");
        }
    }

    #[test]
    fn modifier_order_does_not_matter() {
        let a = parse("Ctrl+Shift+O").unwrap();
        let b = parse("Shift+Ctrl+O").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.canonical(), b.canonical());
    }

    #[test]
    fn function_keys_parse() {
        assert_eq!(parse("Ctrl+F5").unwrap().key, Key::Function(5));
        assert_eq!(parse("Alt+F12").unwrap().key, Key::Function(12));
        assert_eq!(parse("Ctrl+F24").unwrap().key, Key::Function(24));
    }

    #[test]
    fn named_keys_parse() {
        assert_eq!(parse("Ctrl+Space").unwrap().key, Key::Space);
        assert_eq!(parse("Ctrl+Enter").unwrap().key, Key::Enter);
        assert_eq!(parse("Ctrl+Return").unwrap().key, Key::Enter);
        assert!(
            parse("Ctrl+Shift+Escape").is_err(),
            "Ctrl+Shift+Esc is Task Manager"
        );
        assert!(
            parse("Ctrl+Shift+E").is_ok(),
            "Ctrl+Shift+E is an ordinary chord and must not be confused with Esc"
        );
    }

    #[test]
    fn digits_are_valid_keys() {
        assert_eq!(parse("Ctrl+1").unwrap().key, Key::Char('1'));
    }

    /* ---------- rejections ---------- */

    #[test]
    fn a_bare_key_is_refused() {
        // The worst possible bug: capturing "O" system-wide means the user
        // cannot type the letter O anywhere.
        assert_eq!(parse("O"), Err(HotkeyError::NoModifier));
        assert_eq!(parse("F5"), Err(HotkeyError::NoModifier));
        assert_eq!(parse("Space"), Err(HotkeyError::NoModifier));
    }

    #[test]
    fn shift_alone_is_not_a_modifier_for_letters() {
        // Shift+O is just typing a capital O.
        assert_eq!(parse("Shift+O"), Err(HotkeyError::NoModifier));
    }

    #[test]
    fn modifiers_without_a_key_are_refused() {
        assert_eq!(parse("Ctrl"), Err(HotkeyError::NoKey));
        assert_eq!(parse("Ctrl+Shift"), Err(HotkeyError::NoKey));
    }

    #[test]
    fn empty_input_is_refused() {
        assert_eq!(parse(""), Err(HotkeyError::Empty));
        assert_eq!(parse("   "), Err(HotkeyError::Empty));
        assert_eq!(parse("+++"), Err(HotkeyError::NoKey));
    }

    #[test]
    fn unknown_keys_are_named_in_the_error() {
        match parse("Ctrl+Banana") {
            Err(HotkeyError::UnknownKey(k)) => assert_eq!(k, "Banana"),
            other => panic!("expected UnknownKey, got {other:?}"),
        }
    }

    #[test]
    fn two_keys_in_one_chord_are_refused() {
        assert!(parse("Ctrl+O+P").is_err());
    }

    #[test]
    fn out_of_range_function_keys_are_refused() {
        assert!(parse("Ctrl+F0").is_err());
        assert!(parse("Ctrl+F25").is_err());
        assert!(parse("Ctrl+F99").is_err());
    }

    /* ---------- reserved chords ---------- */

    #[test]
    fn windows_reserved_chords_are_refused_with_a_reason() {
        for (chord, must_mention) in [
            ("Win+L", "lock"),
            ("Win+D", "desktop"),
            ("Win+E", "Explorer"),
            ("Win+R", "Run"),
            ("Win+I", "Settings"),
            ("Win+S", "Search"),
            ("Win+V", "clipboard"),
        ] {
            match parse(chord) {
                Err(HotkeyError::Reserved(why)) => assert!(
                    why.to_lowercase().contains(&must_mention.to_lowercase()),
                    "{chord}: reason {why:?} does not explain itself"
                ),
                other => panic!("{chord} should be reserved, got {other:?}"),
            }
        }
    }

    #[test]
    fn window_management_chords_are_refused() {
        assert!(matches!(parse("Alt+Tab"), Err(HotkeyError::Reserved(_))));
        assert!(matches!(parse("Alt+Escape"), Err(HotkeyError::Reserved(_))));
        assert!(matches!(
            parse("Ctrl+Alt+Tab"),
            Err(HotkeyError::Reserved(_))
        ));
    }

    #[test]
    fn a_reserved_letter_is_fine_with_extra_modifiers() {
        // Win+L is reserved, but Ctrl+Shift+L is not a system chord.
        assert!(parse("Ctrl+Shift+L").is_ok());
        assert!(
            parse("Ctrl+Alt+D").is_err(),
            "but this one genuinely clashes"
        );
    }

    /* ---------- formatting ---------- */

    #[test]
    fn canonical_form_is_stable_and_round_trips() {
        for s in ["Win+O", "Ctrl+Shift+O", "Ctrl+Alt+F5", "Ctrl+Space"] {
            let hk = parse(s).unwrap();
            let canon = hk.canonical();
            assert_eq!(parse(&canon).unwrap(), hk, "round trip failed for {s:?}");
        }
    }

    #[test]
    fn accelerator_format_matches_what_tauri_expects() {
        assert_eq!(Hotkey::default_global().to_accelerator(), "Super+KeyO");
        assert_eq!(
            parse("Ctrl+Shift+O").unwrap().to_accelerator(),
            "CommandOrControl+Shift+KeyO"
        );
        assert_eq!(
            parse("Ctrl+F5").unwrap().to_accelerator(),
            "CommandOrControl+F5"
        );
    }

    #[test]
    fn accelerators_use_only_tokens_the_hotkey_parser_accepts() {
        // Verified against global-hotkey 0.7 parse_hotkey/parse_key, which is
        // what tauri-plugin-global-shortcut re-exports. Getting a token wrong
        // fails at *runtime* registration, not at compile time, so the hotkey
        // would silently never fire.
        const MODIFIERS: &[&str] = &[
            "OPTION",
            "ALT",
            "CONTROL",
            "CTRL",
            "COMMAND",
            "CMD",
            "SUPER",
            "SHIFT",
            "COMMANDORCONTROL",
            "COMMANDORCTRL",
            "CMDORCTRL",
            "CMDORCONTROL",
        ];

        for hk in fallback_chain() {
            let accel = hk.to_accelerator();
            let tokens: Vec<&str> = accel.split('+').collect();
            let (key, mods) = tokens.split_last().unwrap();

            for m in mods {
                assert!(
                    MODIFIERS.contains(&m.to_uppercase().as_str()),
                    "modifier token {m:?} in {accel:?} is not accepted by global-hotkey"
                );
            }

            let k = key.to_uppercase();
            let valid_key = k.starts_with("KEY")
                || k.starts_with("DIGIT")
                || k.starts_with('F') && k[1..].parse::<u8>().is_ok()
                || matches!(k.as_str(), "SPACE" | "ENTER" | "ESCAPE" | "TAB");
            assert!(
                valid_key,
                "key token {key:?} in {accel:?} is not recognised"
            );
        }
    }

    #[test]
    fn display_uses_platform_conventions() {
        let hk = parse("Ctrl+Shift+O").unwrap();
        assert_eq!(hk.display_for(Platform::Windows), "Ctrl+Shift+O");
        assert_eq!(hk.display_for(Platform::Linux), "Ctrl+Shift+O");
        assert_eq!(hk.display_for(Platform::MacOs), "⌃⇧O");

        let meta = Hotkey::default_global();
        assert_eq!(meta.display_for(Platform::Windows), "Win+O");
        assert_eq!(meta.display_for(Platform::Linux), "Super+O");
        assert_eq!(meta.display_for(Platform::MacOs), "⌘O");
    }

    #[test]
    fn parsing_never_panics_on_hostile_input() {
        for junk in [
            "+",
            "++++",
            "\u{0}",
            "🙂+🙂",
            &"a+".repeat(500),
            "Ctrl+",
            "+O",
            "ctrl+ctrl+ctrl+O",
            &"F".repeat(200),
        ] {
            let _ = parse(junk);
        }
    }
}
