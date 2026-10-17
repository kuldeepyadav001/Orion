//! OS Desktop Automation Module (Milestone 9).
//!
//! Exposes:
//! 1. `OsAutomationManager`: High-level desktop automation coordinator
//! 2. Zero-vision OS accessibility tree snapshot generation
//! 3. Two-domain security integration via `CapabilityBroker`

pub mod ax;
pub mod driver;

use std::sync::Arc;
use tokio::sync::Mutex;
use crate::error::Result;
use self::ax::OsSnapshot;
use self::driver::OsDriver;

/// State coordinator for OS desktop UI automation.
pub struct OsAutomationManager {
    driver: OsDriver,
    last_snapshot: Mutex<Option<OsSnapshot>>,
}

impl Default for OsAutomationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl OsAutomationManager {
    pub fn new() -> Self {
        Self {
            driver: OsDriver::new(),
            last_snapshot: Mutex::new(None),
        }
    }

    /// Captures a fresh OS accessibility snapshot and caches it.
    pub async fn snapshot(&self) -> Result<OsSnapshot> {
        let snap = self.driver.capture_snapshot().await?;
        let mut cache = self.last_snapshot.lock().await;
        *cache = Some(snap.clone());
        Ok(snap)
    }

    /// Launches a desktop application by name.
    pub async fn launch_app(&self, app_name: &str) -> Result<String> {
        self.driver.launch_app(app_name).await
    }

    /// Brings a specific window to the foreground.
    pub async fn focus_window(&self, target: &str) -> Result<String> {
        self.driver.focus_window(target).await
    }

    /// Closes a specific window or application.
    pub async fn close_window(&self, target: &str) -> Result<String> {
        self.driver.close_window(target).await
    }

    /// Clicks on an OS accessibility element or desktop coordinates.
    pub async fn click(&self, element_ref: &str, x: Option<i32>, y: Option<i32>, button: Option<&str>) -> Result<String> {
        let btn = button.unwrap_or("left");

        // If coordinates provided, click directly
        if let (Some(cx), Some(cy)) = (x, y) {
            return self.driver.click(cx, cy, btn).await;
        }

        // Otherwise resolve element_ref from cached snapshot
        let coords = {
            let cache = self.last_snapshot.lock().await;
            if let Some(ref snap) = *cache {
                snap.interactive_refs
                    .iter()
                    .find(|r| r.ref_id.eq_ignore_ascii_case(element_ref))
                    .and_then(|r| r.center_coords)
            } else {
                None
            }
        };

        let [click_x, click_y] = coords.unwrap_or([200, 200]);
        self.driver.click(click_x, click_y, btn).await
    }

    /// Types text into the currently active window or focused control.
    pub async fn type_text(&self, text: &str) -> Result<String> {
        self.driver.type_text(text).await
    }

    /// Sends a keyboard hotkey combination.
    pub async fn send_hotkey(&self, keys: &[String]) -> Result<String> {
        self.driver.send_hotkey(keys).await
    }

    /// Executes a sequence of desktop automation steps.
    pub async fn execute_action_plan(&self, steps: Vec<DesktopActionStep>) -> Result<Vec<String>> {
        let mut results = Vec::new();
        for step in steps {
            match step {
                DesktopActionStep::Launch { app } => {
                    results.push(self.launch_app(&app).await?);
                }
                DesktopActionStep::Focus { target } => {
                    results.push(self.focus_window(&target).await?);
                }
                DesktopActionStep::Type { text } => {
                    results.push(self.type_text(&text).await?);
                }
                DesktopActionStep::Hotkey { keys } => {
                    results.push(self.send_hotkey(&keys).await?);
                }
                DesktopActionStep::Click { element_ref, x, y } => {
                    results.push(self.click(element_ref.as_deref().unwrap_or("@d1"), x, y, None).await?);
                }
                DesktopActionStep::Wait { ms } => {
                    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                    results.push(format!("Waited {ms}ms"));
                }
                DesktopActionStep::Close { target } => {
                    results.push(self.close_window(&target).await?);
                }
            }
        }
        Ok(results)
    }
}

/// A single step in an automated multi-step desktop workflow.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum DesktopActionStep {
    Launch { app: String },
    Focus { target: String },
    Type { text: String },
    Hotkey { keys: Vec<String> },
    Click { element_ref: Option<String>, x: Option<i32>, y: Option<i32> },
    Wait { ms: u64 },
    Close { target: String },
}

pub type SharedAutomationManager = Arc<OsAutomationManager>;
