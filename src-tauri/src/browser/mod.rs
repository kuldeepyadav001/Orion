//! Sandboxed Browser Automation Engine (Milestone 8).
//!
//! Enforces:
//! 1. Isolated Dedicated Profile: Runs with `--user-data-dir=~/.orion/browser_profile`.
//!    Personal cookies, passwords, and sessions are never accessible.
//! 2. Accessibility Tree Navigation: Reads the semantic AX tree, not screenshots.
//! 3. Two-Domain Boundary: External web content is strictly `Domain::Untrusted`.
//!    Indirect prompt injections in web pages cannot trigger tool actions.
//! 4. Capability Broker Gating: Navigation and web actions logged in SQLite `audit_log` (T2).

pub mod ax;
pub mod driver;

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::browser::ax::AxSnapshot;
use crate::browser::driver::BrowserDriver;
use crate::error::Result;

/// Status summary of the sandboxed browser subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserStatus {
    pub is_active: bool,
    pub profile_path: String,
    pub current_url: String,
    pub browser_binary: Option<String>,
}

/// Coordinates sandboxed browser automation under Capability Broker supervision.
pub struct BrowserManager {
    driver: Arc<BrowserDriver>,
    history: Mutex<Vec<String>>,
}

impl Default for BrowserManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserManager {
    pub fn new() -> Self {
        Self {
            driver: Arc::new(BrowserDriver::new()),
            history: Mutex::new(Vec::new()),
        }
    }

    /// Retrieve active status of the headless browser.
    pub async fn status(&self) -> BrowserStatus {
        let binary_path = driver::find_system_browser()
            .map(|p| p.to_string_lossy().to_string());

        let snap = self.driver.current_snapshot().await.unwrap_or(AxSnapshot {
            url: "about:blank".into(),
            title: "".into(),
            interactive_refs: Vec::new(),
            semantic_tree_text: "".into(),
        });

        BrowserStatus {
            is_active: self.driver.is_running(),
            profile_path: self.driver.profile_path().to_string_lossy().to_string(),
            current_url: snap.url,
            browser_binary: binary_path,
        }
    }

    /// Navigate to a target web page and return its semantic Accessibility tree snapshot.
    pub async fn navigate(&self, url: &str) -> Result<AxSnapshot> {
        let snapshot = self.driver.navigate(url).await?;
        self.history.lock().await.push(url.to_string());
        Ok(snapshot)
    }

    /// Retrieve the current Accessibility tree snapshot.
    pub async fn snapshot(&self) -> Result<AxSnapshot> {
        self.driver.current_snapshot().await
    }

    /// Close active session.
    pub async fn close(&self) -> Result<()> {
        self.driver.close().await
    }
}
