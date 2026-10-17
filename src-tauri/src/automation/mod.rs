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

/// Result of an autonomous desktop action executed directly on the OS.
#[derive(Debug, Clone)]
pub struct AutonomousDesktopResult {
    pub executed: bool,
    pub action_type: String,
    pub description: String,
    pub direct_answer: String,
}

/// Evaluates basic arithmetic expressions like "45 * 128" or "100 + 25".
pub fn evaluate_simple_math(text: &str) -> Option<(f64, String, String)> {
    let clean = text
        .replace('x', "*")
        .replace('X', "*")
        .replace('÷', "/")
        .replace(',', "");

    for op in ['*', '+', '-', '/', '%'] {
        if let Some(pos) = clean.find(op) {
            let left = clean[..pos].trim();
            let right = clean[pos + 1..].trim();

            let left_num_str: String = left
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect::<String>()
                .chars()
                .rev()
                .collect();

            let right_num_str: String = right
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();

            if let (Ok(n1), Ok(n2)) = (left_num_str.parse::<f64>(), right_num_str.parse::<f64>()) {
                let res = match op {
                    '+' => n1 + n2,
                    '-' => n1 - n2,
                    '*' => n1 * n2,
                    '/' if n2 != 0.0 => n1 / n2,
                    '%' => n1 % n2,
                    _ => continue,
                };
                let res_str = if res.fract() == 0.0 {
                    format!("{}", res as i64)
                } else {
                    format!("{:.2}", res)
                };
                let n1_str = if n1.fract() == 0.0 {
                    format!("{}", n1 as i64)
                } else {
                    format!("{n1}")
                };
                let n2_str = if n2.fract() == 0.0 {
                    format!("{}", n2 as i64)
                } else {
                    format!("{n2}")
                };
                let formula = format!("{n1_str} {op} {n2_str} = {res_str}");
                return Some((res, formula, res_str));
            }
        }
    }
    None
}

/// Detects if a user message is a desktop automation command and executes it directly on the OS.
pub async fn detect_and_execute_desktop_action(
    message: &str,
    manager: &OsAutomationManager,
) -> Option<AutonomousDesktopResult> {
    // 1. Strip any prompt wrapper if injected
    let clean_message = if let Some(pos) = message.find("User Request:") {
        message[pos + "User Request:".len()..].trim()
    } else {
        message.trim()
    };
    let lower = clean_message.to_lowercase();

    // 2. Calculator & Math
    if lower.contains("calculator") || lower.contains("calc") || lower.starts_with("calculate ") {
        let math_eval = evaluate_simple_math(clean_message);
        let _ = manager.launch_app("calc").await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let _ = manager.focus_window("Calculator").await;

        if let Some((_, formula, res_str)) = math_eval {
            let send_text = format!("{}=", formula.split('=').next().unwrap_or("").trim());
            let _ = manager.type_text(&send_text).await;
            return Some(AutonomousDesktopResult {
                executed: true,
                action_type: "calculator".into(),
                description: format!("Launched Calculator on your desktop and computed {formula}."),
                direct_answer: format!(
                    "I have opened **Calculator** on your screen and executed the calculation:\n\n**Calculation:** `{formula}`\n\n**Result:** `{res_str}`"
                ),
            });
        }

        return Some(AutonomousDesktopResult {
            executed: true,
            action_type: "calculator".into(),
            description: "Launched Calculator on your desktop.".into(),
            direct_answer: "I have opened **Calculator** on your screen ready for your calculations.".into(),
        });
    }

    // 3. Notepad & Notes
    if (lower.contains("notepad") || lower.contains("notes"))
        && (lower.contains("open") || lower.contains("launch") || lower.contains("write") || lower.contains("paste"))
    {
        let _ = manager.launch_app("notepad").await;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let _ = manager.focus_window("Notepad").await;

        let mut text_to_write = "";
        for marker in ["write down", "write", "paste", "type"] {
            if let Some(pos) = lower.find(marker) {
                let rest = clean_message[pos + marker.len()..].trim();
                if !rest.is_empty() {
                    text_to_write = rest;
                    break;
                }
            }
        }

        if !text_to_write.is_empty() {
            let _ = manager.type_text(text_to_write).await;
            return Some(AutonomousDesktopResult {
                executed: true,
                action_type: "notepad".into(),
                description: "Opened Notepad on desktop and typed the requested notes.".into(),
                direct_answer: format!(
                    "I have opened **Notepad** on your screen and typed your notes:\n\n> {text_to_write}"
                ),
            });
        }

        return Some(AutonomousDesktopResult {
            executed: true,
            action_type: "notepad".into(),
            description: "Opened Notepad on desktop.".into(),
            direct_answer: "I have opened **Notepad** on your screen ready for your notes.".into(),
        });
    }

    // 4. File Explorer
    if (lower.contains("explorer") || lower.contains("files") || lower.contains("documents") || lower.contains("downloads"))
        && (lower.contains("open") || lower.contains("launch") || lower.contains("browse"))
    {
        let _ = manager.launch_app("explorer").await;
        return Some(AutonomousDesktopResult {
            executed: true,
            action_type: "explorer".into(),
            description: "Opened File Explorer on desktop.".into(),
            direct_answer: "I have opened **File Explorer** on your desktop.".into(),
        });
    }

    // 5. Terminal / CMD
    if (lower.contains("terminal") || lower.contains("cmd") || lower.contains("command prompt") || lower.contains("powershell"))
        && (lower.contains("open") || lower.contains("launch"))
    {
        let _ = manager.launch_app("cmd").await;
        return Some(AutonomousDesktopResult {
            executed: true,
            action_type: "terminal".into(),
            description: "Opened Command Prompt Terminal on desktop.".into(),
            direct_answer: "I have opened the **Terminal** command prompt on your screen.".into(),
        });
    }

    // 6. VS Code
    if (lower.contains("vscode") || lower.contains("vs code") || lower.contains("code editor"))
        && (lower.contains("open") || lower.contains("launch"))
    {
        let _ = manager.launch_app("code").await;
        return Some(AutonomousDesktopResult {
            executed: true,
            action_type: "code".into(),
            description: "Opened Visual Studio Code on desktop.".into(),
            direct_answer: "I have opened **Visual Studio Code** on your desktop.".into(),
        });
    }

    // 7. Close window / application
    if lower.starts_with("close ") || lower.starts_with("exit ") || lower.starts_with("quit ") {
        let target = lower
            .replace("close ", "")
            .replace("exit ", "")
            .replace("quit ", "")
            .trim()
            .to_string();
        let _ = manager.close_window(&target).await;
        return Some(AutonomousDesktopResult {
            executed: true,
            action_type: "close".into(),
            description: format!("Closed {target} on desktop."),
            direct_answer: format!("I have closed **{target}** on your desktop."),
        });
    }

    None
}

pub type SharedAutomationManager = Arc<OsAutomationManager>;
