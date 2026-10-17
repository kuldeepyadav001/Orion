import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  IconMonitor,
  IconZap,
  IconX,
  IconRotateCw,
} from "./Icons";

const QUICK_APPS = [
  { id: "notepad", name: "Notepad", icon: "📝", desc: "Text editor" },
  { id: "calc", name: "Calculator", icon: "🧮", desc: "Math & calculations" },
  { id: "explorer", name: "File Explorer", icon: "📁", desc: "Browse files" },
  { id: "cmd", name: "Terminal", icon: "💻", desc: "Command prompt" },
  { id: "code", name: "VS Code", icon: "⚡", desc: "Code editor" },
];

const SUGGESTIONS = [
  "Open Notepad and write down my meeting notes",
  "Open Calculator and calculate 45 * 128",
  "Open File Explorer in my Documents folder",
  "Summarize our active chat and paste into Notepad",
];

export default function DesktopAutomationStudio({ isOpen, onClose, onInjectContextToChat }) {
  const [snapshot, setSnapshot] = useState(null);
  const [loading, setLoading] = useState(false);
  const [taskInput, setTaskInput] = useState("");
  const [textToType, setTextToType] = useState("");
  const [statusMessage, setStatusMessage] = useState(null);

  const refreshDesktop = useCallback(async () => {
    setLoading(true);
    try {
      const snap = await invoke("desktop_snapshot");
      if (snap) {
        setSnapshot(snap);
      }
    } catch {
      // Fallback preview
      setSnapshot({
        active_window: "Untitled - Notepad",
        active_process: "notepad.exe",
        visible_windows: [
          { handle_id: "w-1", title: "Untitled - Notepad", process_name: "notepad.exe", is_active: true },
          { handle_id: "w-2", title: "Calculator", process_name: "calc.exe", is_active: false },
          { handle_id: "w-3", title: "File Explorer", process_name: "explorer.exe", is_active: false },
        ],
        semantic_tree_text: "[Active: Untitled - Notepad]",
      });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (isOpen) {
      refreshDesktop();
    }
  }, [isOpen, refreshDesktop]);

  const handleLaunchApp = async (appId, appName) => {
    setLoading(true);
    setStatusMessage(null);
    try {
      await invoke("desktop_launch_app", { appName: appId });
      setStatusMessage(`Opened ${appName}`);
      setTimeout(() => refreshDesktop(), 800);
    } catch (err) {
      setStatusMessage(`Could not open ${appName}: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const handleTypeText = async () => {
    if (!textToType.trim()) return;
    setLoading(true);
    setStatusMessage(null);
    try {
      await invoke("desktop_type", { text: textToType });
      setStatusMessage(`Typed "${textToType.slice(0, 30)}..." into active app`);
      setTextToType("");
    } catch (err) {
      setStatusMessage(`Typing failed: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const handleHotkey = async (keys, label) => {
    setLoading(true);
    setStatusMessage(null);
    try {
      await invoke("desktop_hotkey", { keys });
      setStatusMessage(`Sent ${label} to active app`);
    } catch (err) {
      setStatusMessage(`Hotkey failed: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const handleFocusWindow = async (target) => {
    setLoading(true);
    setStatusMessage(null);
    try {
      await invoke("desktop_focus_window", { target });
      setStatusMessage(`Brought "${target}" to front`);
      setTimeout(() => refreshDesktop(), 500);
    } catch (err) {
      setStatusMessage(`Could not focus ${target}: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const handleCloseWindow = async (target) => {
    setLoading(true);
    setStatusMessage(null);
    try {
      await invoke("desktop_close_window", { target });
      setStatusMessage(`Closed "${target}"`);
      setTimeout(() => refreshDesktop(), 600);
    } catch (err) {
      setStatusMessage(`Could not close ${target}: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const handleRunTask = () => {
    if (!onInjectContextToChat) return;
    const task = taskInput.trim() || "Analyze open desktop windows and automate the next action.";
    onInjectContextToChat(task);
    onClose();
  };

  if (!isOpen) return null;

  return (
    <div className="email-modal-backdrop" onClick={onClose}>
      <div
        className="email-modal-dialog modern-browser-dialog"
        onClick={(e) => e.stopPropagation()}
        style={{ maxWidth: "980px", height: "80vh", maxHeight: "740px" }}
      >
        {/* Clean Human-Friendly Header */}
        <div className="modern-browser-header">
          <div className="modern-browser-header-left">
            <div className="modern-browser-icon-wrap" style={{ background: "rgba(6, 182, 212, 0.15)", color: "#38bdf8" }}>
              <IconMonitor size={16} />
            </div>
            <div>
              <div className="modern-browser-title">Desktop Assistant</div>
              <div className="modern-browser-subtitle">
                Launch apps, type notes, and control your PC with Orion
              </div>
            </div>
          </div>

          <div className="modern-browser-header-actions">
            <button
              type="button"
              className="email-secondary-btn"
              onClick={refreshDesktop}
              disabled={loading}
              title="Refresh open windows"
              style={{ display: "inline-flex", alignItems: "center", gap: "6px" }}
            >
              <IconRotateCw size={12} className={loading ? "spin" : ""} />
              <span>{loading ? "Scanning..." : "Refresh"}</span>
            </button>

            <button
              type="button"
              className="email-close-btn"
              onClick={onClose}
              aria-label="Close"
            >
              <IconX size={15} />
            </button>
          </div>
        </div>

        {/* Quick Launch Apps Strip */}
        <div className="modern-browser-nav-bar" style={{ padding: "10px 18px" }}>
          <div style={{ display: "flex", alignItems: "center", gap: "10px", width: "100%", overflowX: "auto" }}>
            <span style={{ fontSize: "12px", color: "var(--text-dim)", fontWeight: 500, flexShrink: 0 }}>
              Launch:
            </span>
            {QUICK_APPS.map((app) => (
              <button
                key={app.id}
                type="button"
                className="modern-shortcut-chip"
                onClick={() => handleLaunchApp(app.id, app.name)}
                disabled={loading}
                title={app.desc}
                style={{ display: "inline-flex", alignItems: "center", gap: "6px", padding: "5px 12px" }}
              >
                <span>{app.icon}</span>
                <span>{app.name}</span>
              </button>
            ))}
          </div>
        </div>

        {/* Status notification banner */}
        {statusMessage && (
          <div style={{
            margin: "8px 18px 0",
            padding: "8px 14px",
            borderRadius: "8px",
            background: "rgba(6, 182, 212, 0.12)",
            border: "1px solid rgba(6, 182, 212, 0.28)",
            color: "#38bdf8",
            fontSize: "12px",
            fontWeight: 500,
          }}>
            ✦ {statusMessage}
          </div>
        )}

        {/* Main 2-Column Clean Layout */}
        <div className="email-split-container" style={{ flex: 1, overflow: "hidden" }}>
          {/* Left Column: Running Apps List */}
          <div className="email-list-pane" style={{ width: "320px", borderRight: "1px solid var(--border-soft)", display: "flex", flexDirection: "column" }}>
            <div style={{ padding: "12px 16px", borderBottom: "1px solid var(--border-subtle)", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
              <span style={{ fontSize: "12px", fontWeight: 600, color: "var(--text)" }}>
                Open Windows
              </span>
              <span style={{ fontSize: "11px", color: "var(--text-dim)" }}>
                {snapshot?.visible_windows?.length || 0} active
              </span>
            </div>

            <div style={{ flex: 1, overflowY: "auto", padding: "8px" }}>
              {snapshot?.visible_windows && snapshot.visible_windows.length > 0 ? (
                snapshot.visible_windows.map((win, idx) => {
                  const isActive = win.is_active || win.title === snapshot.active_window;
                  const isNote = win.process_name?.toLowerCase().includes("notepad");
                  const isCalc = win.process_name?.toLowerCase().includes("calc");
                  const isExp = win.process_name?.toLowerCase().includes("explorer");
                  const isCode = win.process_name?.toLowerCase().includes("code");

                  return (
                    <div
                      key={win.handle_id || idx}
                      style={{
                        padding: "10px 12px",
                        borderRadius: "10px",
                        marginBottom: "6px",
                        background: isActive ? "rgba(6, 182, 212, 0.08)" : "rgba(255, 255, 255, 0.02)",
                        border: isActive ? "1px solid rgba(6, 182, 212, 0.3)" : "1px solid rgba(255, 255, 255, 0.05)",
                        display: "flex",
                        alignItems: "center",
                        gap: "8px",
                        transition: "all 0.15s ease",
                      }}
                    >
                      <div style={{ fontSize: "16px", flexShrink: 0 }}>
                        {isNote ? "📝" : isCalc ? "🧮" : isExp ? "📁" : isCode ? "💻" : "🪟"}
                      </div>
                      <div style={{ flex: 1, minWidth: 0 }}>
                        <div style={{ fontSize: "12px", fontWeight: 600, color: "var(--text)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                          {win.title || win.process_name}
                        </div>
                        <div style={{ fontSize: "10px", color: "var(--text-dim)" }}>
                          {win.process_name}
                        </div>
                      </div>
                      
                      <div style={{ display: "flex", alignItems: "center", gap: "4px", flexShrink: 0 }}>
                        {isActive ? (
                          <span style={{
                            fontSize: "9.5px",
                            fontWeight: 600,
                            padding: "2px 5px",
                            borderRadius: "6px",
                            background: "rgba(56, 189, 248, 0.2)",
                            color: "#38bdf8",
                          }}>
                            Active
                          </span>
                        ) : (
                          <button
                            type="button"
                            onClick={() => handleFocusWindow(win.title || win.process_name)}
                            disabled={loading}
                            title="Bring window to front"
                            style={{
                              background: "rgba(255, 255, 255, 0.04)",
                              border: "1px solid rgba(255, 255, 255, 0.1)",
                              borderRadius: "4px",
                              padding: "2px 6px",
                              color: "var(--text-dim)",
                              fontSize: "10px",
                              cursor: "pointer",
                            }}
                          >
                            Focus
                          </button>
                        )}
                        <button
                          type="button"
                          onClick={() => handleCloseWindow(win.title || win.process_name)}
                          disabled={loading}
                          title="Close window"
                          style={{
                            background: "none",
                            border: "none",
                            color: "var(--text-faint)",
                            padding: "2px 4px",
                            cursor: "pointer",
                            fontSize: "11px",
                            borderRadius: "4px",
                          }}
                          onMouseEnter={(e) => { e.currentTarget.style.color = "#ef4444"; }}
                          onMouseLeave={(e) => { e.currentTarget.style.color = "var(--text-faint)"; }}
                        >
                          ✕
                        </button>
                      </div>
                    </div>
                  );
                })
              ) : (
                <div style={{ padding: "24px 16px", textAlign: "center", color: "var(--text-dim)", fontSize: "12px" }}>
                  No open windows detected. Launch an app above!
                </div>
              )}
            </div>
          </div>

          {/* Right Column: Actions & Natural Language Command */}
          <div className="email-detail-pane" style={{ flex: 1, display: "flex", flexDirection: "column", padding: "18px 22px", gap: "16px", overflowY: "auto" }}>
            {/* Active App Target Banner */}
            <div style={{
              padding: "12px 16px",
              borderRadius: "12px",
              background: "rgba(255, 255, 255, 0.03)",
              border: "1px solid rgba(255, 255, 255, 0.08)",
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
            }}>
              <div>
                <div style={{ fontSize: "11px", color: "var(--text-dim)", textTransform: "uppercase", letterSpacing: "0.05em", fontWeight: 600 }}>
                  Active Window Target
                </div>
                <div style={{ fontSize: "14px", fontWeight: 600, color: "#f1f5f9", marginTop: "2px" }}>
                  {snapshot?.active_window || "Desktop Workspace"}
                </div>
              </div>
              <div style={{ fontSize: "11px", color: "#38bdf8", background: "rgba(6, 182, 212, 0.1)", padding: "3px 8px", borderRadius: "6px" }}>
                Ready for input
              </div>
            </div>

            {/* Natural Language Task Box (JARVIS Mode) */}
            <div style={{
              padding: "14px 16px",
              borderRadius: "12px",
              background: "rgba(15, 23, 42, 0.55)",
              border: "1px solid rgba(56, 189, 248, 0.2)",
              display: "flex",
              flexDirection: "column",
              gap: "10px",
            }}>
              <div style={{ display: "flex", alignItems: "center", gap: "6px" }}>
                <IconZap size={14} className="text-cyan" />
                <span style={{ fontSize: "12.5px", fontWeight: 600, color: "#38bdf8" }}>
                  Ask Orion to Automate on Desktop
                </span>
              </div>

              <div style={{ display: "flex", gap: "8px" }}>
                <input
                  type="text"
                  className="modern-task-input"
                  value={taskInput}
                  onChange={(e) => setTaskInput(e.target.value)}
                  placeholder="e.g. 'Open Notepad and paste my notes' or 'Calculate 25 * 40'..."
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      handleRunTask();
                    }
                  }}
                  style={{ flex: 1 }}
                />
                <button
                  type="button"
                  className="modern-btn-analyze-primary"
                  onClick={handleRunTask}
                  style={{ padding: "8px 16px", fontSize: "12px" }}
                >
                  <span>Ask Orion</span>
                </button>
              </div>

              {/* Suggestion Chips */}
              <div style={{ display: "flex", flexWrap: "wrap", gap: "6px", marginTop: "4px" }}>
                {SUGGESTIONS.map((sug, i) => (
                  <button
                    key={i}
                    type="button"
                    onClick={() => setTaskInput(sug)}
                    style={{
                      background: "rgba(255, 255, 255, 0.03)",
                      border: "1px solid rgba(255, 255, 255, 0.08)",
                      borderRadius: "6px",
                      padding: "3px 8px",
                      color: "var(--text-dim)",
                      fontSize: "11px",
                      cursor: "pointer",
                      transition: "all 0.12s ease",
                    }}
                    onMouseEnter={(e) => { e.currentTarget.style.color = "#f1f5f9"; e.currentTarget.style.borderColor = "rgba(56, 189, 248, 0.3)"; }}
                    onMouseLeave={(e) => { e.currentTarget.style.color = "var(--text-dim)"; e.currentTarget.style.borderColor = "rgba(255, 255, 255, 0.08)"; }}
                  >
                    ✦ {sug}
                  </button>
                ))}
              </div>
            </div>

            {/* Direct Keystrokes & Typing */}
            <div style={{
              padding: "14px 16px",
              borderRadius: "12px",
              background: "rgba(255, 255, 255, 0.02)",
              border: "1px solid rgba(255, 255, 255, 0.06)",
              display: "flex",
              flexDirection: "column",
              gap: "10px",
            }}>
              <div style={{ fontSize: "12.5px", fontWeight: 600, color: "var(--text)" }}>
                Type Into Active Window:
              </div>

              <div style={{ display: "flex", gap: "8px" }}>
                <input
                  type="text"
                  className="modern-task-input"
                  value={textToType}
                  onChange={(e) => setTextToType(e.target.value)}
                  placeholder="Enter text to type directly into the focused app..."
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      handleTypeText();
                    }
                  }}
                  style={{ flex: 1 }}
                />
                <button
                  type="button"
                  className="modern-task-submit"
                  onClick={handleTypeText}
                  disabled={loading || !textToType.trim()}
                  style={{ padding: "8px 14px", fontSize: "12px" }}
                >
                  <span>Type Text</span>
                </button>
              </div>

              {/* Quick Useful Hotkeys */}
              <div style={{ display: "flex", alignItems: "center", gap: "8px", flexWrap: "wrap", marginTop: "4px" }}>
                <span style={{ fontSize: "11px", color: "var(--text-dim)" }}>Shortcuts:</span>
                {[
                  { label: "Enter ↵", keys: ["Enter"] },
                  { label: "Save (Ctrl+S)", keys: ["Control", "s"] },
                  { label: "Copy (Ctrl+C)", keys: ["Control", "c"] },
                  { label: "Paste (Ctrl+V)", keys: ["Control", "v"] },
                  { label: "Escape", keys: ["Escape"] },
                  { label: "Tab ⇥", keys: ["Tab"] },
                ].map((hk) => (
                  <button
                    key={hk.label}
                    type="button"
                    className="modern-shortcut-chip"
                    onClick={() => handleHotkey(hk.keys, hk.label)}
                    disabled={loading}
                    style={{ fontSize: "11px", padding: "3px 10px" }}
                  >
                    {hk.label}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>

        {/* Clean Footer */}
        <div className="email-modal-footer" style={{ padding: "10px 18px", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <div style={{ fontSize: "11.5px", color: "var(--text-dim)", display: "flex", alignItems: "center", gap: "6px" }}>
            <span style={{ color: "#34d399" }}>●</span>
            <span>Desktop automation ready. Actions execute locally on your PC.</span>
          </div>

          <button
            type="button"
            className="email-done-btn"
            onClick={onClose}
          >
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
