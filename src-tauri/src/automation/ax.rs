//! OS Desktop Accessibility (AX) Tree and Semantic Normalization (Milestone 9).
//!
//! Enforces:
//! 1. Zero Vision Tokens: Replaces multi-gigabyte vision models with compact, deterministic
//!    accessibility tree text (<10 KB), preserving the single-resident RAM budget (<2.0 GB).
//! 2. Deterministic Handles: References OS controls via `@d1`, `@d2`, `@d3` so quantized local
//!    models (3B) can target buttons, edit fields, and tabs without coordinate hallucinations.
//! 3. Two-Domain Boundary: External apps are inspected through read-only OS accessibility APIs.

use serde::{Deserialize, Serialize};

/// An interactive or structural control within the OS desktop accessibility tree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OsElement {
    /// Deterministic reference ID (e.g. "@d1", "@d2") used for tool actions.
    pub ref_id: String,
    /// Accessibility control type (e.g. "Window", "Button", "Edit", "MenuItem", "Tab", "List").
    pub role: String,
    /// Accessible name or window title.
    pub name: String,
    /// Native window or control class name if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    /// Bounding rectangle on screen: (x, y, width, height).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<[i32; 4]>,
    /// Whether control is interactive and enabled.
    pub enabled: bool,
    /// Whether control currently holds OS input focus.
    pub focused: bool,
    /// Child controls or nested UI elements.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<OsElement>,
}

/// A summary of a top-level desktop window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowSummary {
    pub handle_id: String,
    pub title: String,
    pub process_name: String,
    pub is_active: bool,
    pub rect: [i32; 4],
}

/// A complete desktop snapshot for AI agent reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsSnapshot {
    /// Title of the currently focused desktop window.
    pub active_window: String,
    /// Process name of the currently focused window (e.g. "notepad.exe", "calc.exe").
    pub active_process: String,
    /// List of visible top-level windows on the desktop.
    pub visible_windows: Vec<WindowSummary>,
    /// Flat index of all actionable interactive references (`@d1`, `@d2`, ...).
    pub interactive_refs: Vec<OsInteractiveRef>,
    /// Semantic structured representation ready for LLM prompt context injection.
    pub semantic_tree_text: String,
}

/// An interactive reference entry mapping an ID to control metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsInteractiveRef {
    pub ref_id: String,
    pub role: String,
    pub name: String,
    pub window_title: String,
    pub center_coords: Option<[i32; 2]>,
}

/// Formats an `OsElement` hierarchy into compact semantic text for prompt context.
pub fn format_os_tree_to_text(root: &OsElement, indent: usize) -> String {
    let mut out = String::new();
    let spaces = "  ".repeat(indent);

    let mut desc = format!("{spaces}[ref: {}] {}", root.ref_id, root.role);
    if !root.name.trim().is_empty() {
        desc.push_str(&format!(" \"{}\"", root.name.trim()));
    }
    if let Some([x, y, w, h]) = root.rect {
        desc.push_str(&format!(" (at: {x},{y} {w}x{h})"));
    }
    if !root.enabled {
        desc.push_str(" [disabled]");
    }
    if root.focused {
        desc.push_str(" [focused]");
    }
    out.push_str(&desc);
    out.push('\n');

    for child in &root.children {
        out.push_str(&format_os_tree_to_text(child, indent + 1));
    }

    out
}

/// Flattens interactive elements into a quick lookup list.
pub fn collect_os_interactive_refs(
    root: &OsElement,
    window_title: &str,
    out: &mut Vec<OsInteractiveRef>,
) {
    let is_interactive = matches!(
        root.role.to_lowercase().as_str(),
        "button"
            | "edit"
            | "textbox"
            | "menuitem"
            | "tab"
            | "tabitem"
            | "checkbox"
            | "radiobutton"
            | "combobox"
            | "listitem"
            | "hyperlink"
            | "splitbutton"
            | "window"
    );

    if is_interactive && !root.ref_id.is_empty() {
        let center = root.rect.map(|[x, y, w, h]| [x + w / 2, y + h / 2]);
        out.push(OsInteractiveRef {
            ref_id: root.ref_id.clone(),
            role: root.role.clone(),
            name: root.name.clone(),
            window_title: window_title.to_string(),
            center_coords: center,
        });
    }

    for child in &root.children {
        collect_os_interactive_refs(child, window_title, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_os_tree_accurately() {
        let element = OsElement {
            ref_id: "@d1".into(),
            role: "Window".into(),
            name: "Untitled - Notepad".into(),
            class_name: Some("Notepad".into()),
            rect: Some([100, 100, 800, 600]),
            enabled: true,
            focused: true,
            children: vec![
                OsElement {
                    ref_id: "@d2".into(),
                    role: "Edit".into(),
                    name: "Text Editor".into(),
                    class_name: Some("RichEditD2DPT".into()),
                    rect: Some([105, 130, 790, 560]),
                    enabled: true,
                    focused: true,
                    children: vec![],
                },
                OsElement {
                    ref_id: "@d3".into(),
                    role: "Button".into(),
                    name: "File".into(),
                    class_name: None,
                    rect: Some([105, 105, 40, 20]),
                    enabled: true,
                    focused: false,
                    children: vec![],
                },
            ],
        };

        let formatted = format_os_tree_to_text(&element, 0);
        assert!(formatted.contains("[ref: @d1] Window \"Untitled - Notepad\""));
        assert!(formatted.contains("[ref: @d2] Edit \"Text Editor\""));
        assert!(formatted.contains("[ref: @d3] Button \"File\""));

        let mut refs = Vec::new();
        collect_os_interactive_refs(&element, "Untitled - Notepad", &mut refs);
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[1].ref_id, "@d2");
        assert_eq!(refs[1].center_coords, Some([500, 410]));
    }
}
