//! Accessibility (AX) DOM Tree Processing (Milestone 8).
//!
//! Philosophy: Accessibility trees, not screenshots.
//! 1. 2–5 KB of structured text vs 500 KB–2 MB per screenshot.
//! 2. Deterministic element references (`ref: e12`, `ref: e14`) that small local models
//!    (3B/4B quantized) can accurately select without coordinate vision hallucination.
//! 3. Zero vision token overhead; runs on 8 GB RAM within CPU budget.

use serde::{Deserialize, Serialize};

/// A normalized accessibility element in Orion's semantic tree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AxElement {
    /// Deterministic reference ID (e.g. "e1", "e2", "e14") used for tool actions.
    pub ref_id: String,
    /// Accessibility role (button, link, heading, input, checkbox, table, etc.).
    pub role: String,
    /// Accessible name / label.
    pub name: String,
    /// Current value for inputs/sliders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Heading level (1-6) when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u32>,
    /// Whether element is disabled.
    pub disabled: bool,
    /// Whether element is currently focused.
    pub focused: bool,
    /// Child semantic elements.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<AxElement>,
}

/// A complete accessibility snapshot of a page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxSnapshot {
    pub url: String,
    pub title: String,
    /// Flat list of all interactive elements accessible by `ref_id`.
    pub interactive_refs: Vec<InteractiveRef>,
    /// Compact formatted tree string ready for LLM prompt context.
    pub semantic_tree_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveRef {
    pub ref_id: String,
    pub role: String,
    pub name: String,
    pub value: Option<String>,
}

/// Compacts and formats an `AxElement` tree into clean text for prompt injection.
pub fn format_ax_tree_to_text(root: &AxElement, indent: usize) -> String {
    let mut out = String::new();
    let spaces = "  ".repeat(indent);

    let mut desc = format!("{spaces}[ref: {}] {}", root.ref_id, root.role);
    if !root.name.trim().is_empty() {
        desc.push_str(&format!(" \"{}\"", root.name.trim()));
    }
    if let Some(val) = &root.value {
        if !val.trim().is_empty() {
            desc.push_str(&format!(" (value: \"{}\")", val.trim()));
        }
    }
    if let Some(lvl) = root.level {
        desc.push_str(&format!(" (h{lvl})"));
    }
    if root.disabled {
        desc.push_str(" [disabled]");
    }
    if root.focused {
        desc.push_str(" [focused]");
    }
    out.push_str(&desc);
    out.push('\n');

    for child in &root.children {
        out.push_str(&format_ax_tree_to_text(child, indent + 1));
    }

    out
}

/// Flatten all interactive actionable elements (buttons, links, inputs) into a lookup index.
pub fn collect_interactive_refs(root: &AxElement, out: &mut Vec<InteractiveRef>) {
    let is_interactive = matches!(
        root.role.to_lowercase().as_str(),
        "button"
            | "link"
            | "textbox"
            | "input"
            | "searchbox"
            | "combobox"
            | "checkbox"
            | "radio"
            | "menuitem"
            | "tab"
            | "switch"
    );

    if is_interactive && !root.ref_id.is_empty() {
        out.push(InteractiveRef {
            ref_id: root.ref_id.clone(),
            role: root.role.clone(),
            name: root.name.clone(),
            value: root.value.clone(),
        });
    }

    for child in &root.children {
        collect_interactive_refs(child, out);
    }
}

/// Helper to parse raw CDP / Playwright accessibility JSON nodes into an Orion `AxSnapshot`.
pub fn parse_cdp_ax_tree(url: &str, title: &str, raw_nodes: &[serde_json::Value]) -> AxSnapshot {
    let mut counter = 1usize;
    let mut root_elements = Vec::new();

    for node in raw_nodes {
        if let Some(el) = parse_cdp_node(node, &mut counter) {
            root_elements.push(el);
        }
    }

    let root = if root_elements.len() == 1 {
        root_elements.remove(0)
    } else {
        AxElement {
            ref_id: "e0".into(),
            role: "root".into(),
            name: title.to_string(),
            value: None,
            level: None,
            disabled: false,
            focused: false,
            children: root_elements,
        }
    };

    let mut interactive = Vec::new();
    collect_interactive_refs(&root, &mut interactive);

    let tree_text = format!(
        "[Page Title: {} | URL: {}]\n{}\n",
        title,
        url,
        format_ax_tree_to_text(&root, 0)
    );

    AxSnapshot {
        url: url.to_string(),
        title: title.to_string(),
        interactive_refs: interactive,
        semantic_tree_text: tree_text,
    }
}

fn parse_cdp_node(node: &serde_json::Value, counter: &mut usize) -> Option<AxElement> {
    let role = node["role"]["value"].as_str().unwrap_or("generic").to_string();
    let name = node["name"]["value"].as_str().unwrap_or("").to_string();
    let value = node["value"]["value"].as_str().map(|s| s.to_string());

    // Skip ignorable generic containers with no name and no interactive role
    let is_ignorable = (role == "generic" || role == "none" || role == "StaticText") && name.trim().is_empty();

    let mut children = Vec::new();
    if let Some(child_ids) = node["childIds"].as_array() {
        // Child IDs can be joined if full graph is passed
        let _ = child_ids;
    }
    if let Some(child_nodes) = node["children"].as_array() {
        for c in child_nodes {
            if let Some(parsed_child) = parse_cdp_node(c, counter) {
                children.push(parsed_child);
            }
        }
    }

    if is_ignorable && children.is_empty() {
        return None;
    }

    let ref_id = format!("e{}", *counter);
    *counter += 1;

    Some(AxElement {
        ref_id,
        role,
        name,
        value,
        level: None,
        disabled: false,
        focused: false,
        children,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_ax_tree_cleanly() {
        let tree = AxElement {
            ref_id: "e1".into(),
            role: "navigation".into(),
            name: "Main Header".into(),
            value: None,
            level: None,
            disabled: false,
            focused: false,
            children: vec![
                AxElement {
                    ref_id: "e2".into(),
                    role: "link".into(),
                    name: "Home".into(),
                    value: None,
                    level: None,
                    disabled: false,
                    focused: false,
                    children: vec![],
                },
                AxElement {
                    ref_id: "e3".into(),
                    role: "button".into(),
                    name: "Search".into(),
                    value: None,
                    level: None,
                    disabled: false,
                    focused: true,
                    children: vec![],
                },
            ],
        };

        let formatted = format_ax_tree_to_text(&tree, 0);
        assert!(formatted.contains("[ref: e1] navigation \"Main Header\""));
        assert!(formatted.contains("[ref: e2] link \"Home\""));
        assert!(formatted.contains("[ref: e3] button \"Search\" [focused]"));

        let mut refs = Vec::new();
        collect_interactive_refs(&tree, &mut refs);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].ref_id, "e2");
        assert_eq!(refs[1].ref_id, "e3");
    }
}
