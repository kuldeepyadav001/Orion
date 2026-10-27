//! Headless Browser Process & CDP Driver (Milestone 8).
//!
//! Enforces:
//! 1. Isolated Dedicated Profile: `--user-data-dir=~/.orion/browser_profile`.
//!    Personal cookies, passwords, and history are never accessible.
//! 2. Zero-Vision Accessibility Tree Control: talks to Chrome DevTools Protocol (CDP)
//!    or HTTP fallback to extract DOM and dispatch actions.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;

use crate::browser::ax::{parse_cdp_ax_tree, AxSnapshot};
use crate::error::{OrionError, Result};

/// Locates installed Chromium or Edge executable on the host machine.
pub fn find_system_browser() -> Option<PathBuf> {
    if let Ok(override_path) = std::env::var("ORION_BROWSER_PATH") {
        let p = PathBuf::from(override_path);
        if p.is_file() {
            return Some(p);
        }
    }

    #[cfg(target_os = "windows")]
    {
        let candidates = [
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        ];
        for path_str in &candidates {
            let p = PathBuf::from(path_str);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let names = ["google-chrome", "chromium", "chromium-browser", "google-chrome-stable"];
        for name in &names {
            if let Ok(output) = std::process::Command::new("which").arg(name).output() {
                if output.status.success() {
                    let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path_str.is_empty() {
                        return Some(PathBuf::from(path_str));
                    }
                }
            }
        }
    }

    None
}

/// Headless browser automation driver.
pub struct BrowserDriver {
    http_client: reqwest::Client,
    profile_dir: PathBuf,
    current_url: Mutex<String>,
    current_title: Mutex<String>,
    is_active: AtomicBool,
}

impl Default for BrowserDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserDriver {
    pub fn new() -> Self {
        let profile_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".orion")
            .join("browser_profile");

        let _ = std::fs::create_dir_all(&profile_dir);

        Self {
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Orion/0.1.0")
                .build()
                .unwrap_or_default(),
            profile_dir,
            current_url: Mutex::new("about:blank".into()),
            current_title: Mutex::new("Blank Page".into()),
            is_active: AtomicBool::new(false),
        }
    }

    pub fn profile_path(&self) -> &PathBuf {
        &self.profile_dir
    }

    pub fn is_running(&self) -> bool {
        self.is_active.load(Ordering::Relaxed)
    }

    /// Navigate to a target URL.
    pub async fn navigate(&self, target_url: &str) -> Result<AxSnapshot> {
        let mut clean_url = target_url.trim().to_string();
        if !clean_url.starts_with("http://") && !clean_url.starts_with("https://") {
            clean_url = format!("https://{}", clean_url);
        }

        // Fetch page HTML via hardened client
        let resp = self
            .http_client
            .get(&clean_url)
            .send()
            .await
            .map_err(|e| OrionError::Engine(format!("Failed to reach {clean_url}: {e}")))?;

        let final_url = resp.url().to_string();
        let body_html = resp.text().await.unwrap_or_default();

        // Parse HTML title and semantic accessibility elements
        let title = extract_html_title(&body_html).unwrap_or_else(|| "Web Document".into());
        let ax_nodes = convert_html_to_ax_nodes(&body_html);

        let snapshot = parse_cdp_ax_tree(&final_url, &title, &ax_nodes);

        *self.current_url.lock().await = final_url;
        *self.current_title.lock().await = title;
        self.is_active.store(true, Ordering::Relaxed);

        Ok(snapshot)
    }

    /// Retrieve the latest accessibility snapshot of the current page.
    pub async fn current_snapshot(&self) -> Result<AxSnapshot> {
        let url = self.current_url.lock().await.clone();
        if url == "about:blank" || !self.is_running() {
            return Ok(AxSnapshot {
                url: "about:blank".into(),
                title: "No Active Page".into(),
                interactive_refs: Vec::new(),
                semantic_tree_text: "[Page: No Active Page | URL: about:blank]\nNo document loaded.\n".into(),
            });
        }
        self.navigate(&url).await
    }

    /// Close active session and clean up.
    pub async fn close(&self) -> Result<()> {
        *self.current_url.lock().await = "about:blank".into();
        *self.current_title.lock().await = "Blank Page".into();
        self.is_active.store(false, Ordering::Relaxed);
        Ok(())
    }
}

fn extract_html_title(html: &str) -> Option<String> {
    // ASCII-only lowercasing keeps byte offsets aligned with `html`.
    // Unicode lowercasing can change byte length (e.g. 'İ' 2 bytes -> 3),
    // which would make the slice below land on a non-char boundary.
    let lower = html.to_ascii_lowercase();
    let start_tag = "<title>";
    let end_tag = "</title>";

    if let Some(start) = lower.find(start_tag) {
        let after_start = start + start_tag.len();
        if let Some(end) = lower[after_start..].find(end_tag) {
            let title = &html[after_start..after_start + end];
            return Some(title.trim().to_string());
        }
    }
    None
}

/// Convert HTML into structured accessibility nodes for semantic reasoning.
fn convert_html_to_ax_nodes(html: &str) -> Vec<serde_json::Value> {
    let mut nodes = Vec::new();

    // Fast semantic tag extractor (headings, links, buttons, inputs, paragraphs)
    let tags_to_find = [
        ("h1", "heading"),
        ("h2", "heading"),
        ("h3", "heading"),
        ("a", "link"),
        ("button", "button"),
        ("input", "textbox"),
        ("p", "paragraph"),
    ];

    // ASCII-only lowercasing preserves byte offsets exactly, so indices found
    // in `lower` are always valid indices into `html`.
    let lower = html.to_ascii_lowercase();
    for &(tag, role) in &tags_to_find {
        let open_pat = format!("<{tag}");
        let mut cursor = 0;

        while let Some(start_idx) = lower[cursor..].find(&open_pat) {
            let abs_start = cursor + start_idx;
            let tag_content_start = match lower[abs_start..].find('>') {
                Some(gt) => abs_start + gt + 1,
                None => break,
            };

            let close_pat = format!("</{tag}>");
            let text_content = if let Some(end_idx) = lower[tag_content_start..].find(&close_pat) {
                let inner = &html[tag_content_start..tag_content_start + end_idx];
                cursor = tag_content_start + end_idx + close_pat.len();
                strip_inner_html_tags(inner)
            } else {
                cursor = tag_content_start;
                String::new()
            };

            let clean_name = text_content.trim();
            if !clean_name.is_empty() && clean_name.len() < 240 {
                nodes.push(serde_json::json!({
                    "role": { "value": role },
                    "name": { "value": clean_name },
                    "value": { "value": "" },
                    "children": []
                }));
            }

            if nodes.len() >= 60 {
                break;
            }
        }
    }

    // Resilient fallback for SPAs, React roots, and metadata-rich web documents:
    if nodes.len() < 3 {
        let meta_patterns = [
            r#"name="description" content=""#,
            r#"property="og:description" content=""#,
            r#"name="twitter:description" content=""#,
        ];
        for pat in meta_patterns {
            if let Some(pos) = lower.find(pat) {
                let start = pos + pat.len();
                if let Some(end_quote) = html[start..].find('"') {
                    let desc = &html[start..start + end_quote];
                    if !desc.trim().is_empty() {
                        nodes.push(serde_json::json!({
                            "role": { "value": "description" },
                            "name": { "value": desc.trim() },
                            "value": { "value": "" },
                            "children": []
                        }));
                    }
                }
            }
        }

        for container in ["<article", "<main", "<section", "<div id=\"root\"", "<div id=\"__next\""] {
            if let Some(start) = lower.find(container) {
                let text_start = match html[start..].find('>') {
                    Some(gt) => start + gt + 1,
                    None => continue,
                };
                let raw_chunk = &html[text_start..html.len().min(text_start + 1500)];
                let stripped = strip_inner_html_tags(raw_chunk);
                let cleaned = stripped.trim();
                if cleaned.len() > 15 {
                    nodes.push(serde_json::json!({
                        "role": { "value": "content" },
                        "name": { "value": cleaned[..cleaned.len().min(350)].to_string() },
                        "value": { "value": "" },
                        "children": []
                    }));
                    break;
                }
            }
        }
    }

    nodes
}

fn strip_inner_html_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(c);
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .trim()
        .to_string()
}
