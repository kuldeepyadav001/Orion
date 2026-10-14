import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  IconMonitor,
  IconShield,
  IconZap,
  IconX,
  IconLock,
  IconRotateCw,
  IconCheck,
  IconCopy,
  IconFileText,
} from "./Icons";

const SAMPLE_SNAPSHOT = {
  active_window: "Untitled - Notepad",
  active_process: "notepad.exe",
  visible_windows: [
    {
      handle_id: "w-101",
      title: "Untitled - Notepad",
      process_name: "notepad.exe",
      is_active: true,
      rect: [120, 80, 860, 580],
    },
    {
      handle_id: "w-102",
      title: "Calculator",
      process_name: "calc.exe",
      is_active: false,
      rect: [1020, 140, 360, 520],
    },
    {
      handle_id: "w-103",
      title: "File Explorer - Documents",
      process_name: "explorer.exe",
      is_active: false,
      rect: [200, 220, 940, 640],
    },
  ],
  interactive_refs: [
    {
      ref_id: "@d1",
      role: "Window",
      name: "Untitled - Notepad",
      window_title: "Untitled - Notepad",
      center_coords: [550, 370],
    },
    {
      ref_id: "@d2",
      role: "Edit",
      name: "Text Editor",
      window_title: "Untitled - Notepad",
      center_coords: [550, 380],
    },
    {
      ref_id: "@d3",
      role: "Button",
      name: "File Menu",
      window_title: "Untitled - Notepad",
      center_coords: [147, 96],
    },
    {
      ref_id: "@d4",
      role: "Window",
      name: "Calculator",
      window_title: "Calculator",
      center_coords: [1200, 400],
    },
    {
      ref_id: "@d5",
      role: "Edit",
      name: "Display",
      window_title: "Calculator",
      center_coords: [1200, 194],
    },
  ],
  semantic_tree_text: `[Active Desktop | Window: Untitled - Notepad | Process: notepad.exe]
[ref: @d0] Desktop "Desktop Workspace" (at: 0,0 1920x1080)
  [ref: @d1] Window "Untitled - Notepad" (at: 120,80 860x580) [focused]
    [ref: @d2] Edit "Text Editor" (at: 125,110 850x540) [focused]
    [ref: @d3] Button "File Menu" (at: 125,85 45x22)
  [ref: @d4] Window "Calculator" (at: 1020,140 360x520)
    [ref: @d5] Edit "Display" (at: 1030,170 340x48)`,
};

export default function DesktopAutomationStudio({ isOpen, onClose, onInjectContextToChat }) {
  const [snapshot, setSnapshot] = useState(SAMPLE_SNAPSHOT);
  const [loading, setLoading] = useState(false);
  const [taskInput, setTaskInput] = useState("");
  const [typeTextInput, setTypeTextInput] = useState("");
  const [statusMessage, setStatusMessage] = useState(null);
  const [copiedRef, setCopiedRef] = useState(null);
  const [auditLogEntry, setAuditLogEntry] = useState({
    time: "Just now",
    action: "desktop_snapshot",
    tier: "T0",
    target: "desktop",
    outcome: "ALLOWED",
  });

  const refreshSnapshot = useCallback(async () => {
    setLoading(true);
    try {
      const snap = await invoke("desktop_snapshot");
      if (snap) {
        setSnapshot(snap);
        setAuditLogEntry({
          time: new Date().toLocaleTimeString(),
          action: "desktop_snapshot",
          tier: "T0",
          target: snap.active_window || "desktop",
          outcome: "ALLOWED",
        });
      }
    } catch (err) {
      console.warn("Using sample desktop snapshot in fallback mode:", err);
      setSnapshot(SAMPLE_SNAPSHOT);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (isOpen) {
      refreshSnapshot();
    }
  }, [isOpen, refreshSnapshot]);

  const handleLaunchApp = async (appName) => {
    setLoading(true);
    try {
      const res = await invoke("desktop_launch_app", { appName });
      setStatusMessage(res);
      setAuditLogEntry({
        time: new Date().toLocaleTimeString(),
        action: "desktop_launch_app",
        tier: "T2",
        target: appName,
        outcome: "ALLOWED",
      });
      setTimeout(() => refreshSnapshot(), 800);
    } catch (err) {
      setStatusMessage(`Error: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const handleClickElement = async (refId, coords) => {
    setLoading(true);
    try {
      const x = coords ? coords[0] : null;
      const y = coords ? coords[1] : null;
      const res = await invoke("desktop_click", {
        elementRef: refId,
        x,
        y,
        button: "left",
      });
      setStatusMessage(res);
      setAuditLogEntry({
        time: new Date().toLocaleTimeString(),
        action: "desktop_click",
        tier: "T2",
        target: refId,
        outcome: "ALLOWED",
      });
    } catch (err) {
      setStatusMessage(`Click failed: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const handleSendType = async () => {
    if (!typeTextInput.trim()) return;
    setLoading(true);
    try {
      const res = await invoke("desktop_type", { text: typeTextInput });
      setStatusMessage(res);
      setTypeTextInput("");
      setAuditLogEntry({
        time: new Date().toLocaleTimeString(),
        action: "desktop_type",
        tier: "T2",
        target: snapshot.active_window,
        outcome: "ALLOWED",
      });
    } catch (err) {
      setStatusMessage(`Type failed: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const handleSendHotkey = async (keys) => {
    setLoading(true);
    try {
      const res = await invoke("desktop_hotkey", { keys });
      setStatusMessage(res);
      setAuditLogEntry({
        time: new Date().toLocaleTimeString(),
        action: "desktop_hotkey",
        tier: "T2",
        target: keys.join("+"),
        outcome: "ALLOWED",
      });
    } catch (err) {
      setStatusMessage(`Hotkey failed: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const handleDelegateToOrion = () => {
    if (!onInjectContextToChat) return;
    const task = taskInput.trim() || "Analyze active desktop windows and recommend automated next actions.";
    const contextPrompt = `[OS Desktop Accessibility Snapshot]\n${snapshot.semantic_tree_text}\n\nUser Request: ${task}`;
    onInjectContextToChat(contextPrompt);
    onClose();
  };

  if (!isOpen) return null;

  return (
    <div className="email-modal-backdrop" onClick={onClose}>
      <div className="email-modal-dialog modern-browser-dialog" onClick={(e) => e.stopPropagation()}>
        {/* Header Bar */}
        <div className="modern-browser-header">
          <div className="modern-browser-header-left">
            <div className="modern-browser-icon-wrap">
              <IconMonitor size={16} />
            </div>
            <div>
              <div className="modern-browser-title">OS UI Automation Studio</div>
              <div className="modern-browser-subtitle">
                Zero-Vision Accessibility Trees • Single-Resident RAM Compliant (&le;2.0 GB)
              </div>
            </div>
          </div>

          <div className="modern-browser-header-actions">
            <span className="modern-security-pill">
              <IconShield size={12} />
              <span>Broker Tier 2 • Audited</span>
            </span>

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

        {/* Quick Launch & Action Strip */}
        <div className="modern-browser-nav-bar">
          <div className="modern-omnibar-wrap" style={{ display: "flex", gap: "8px", alignItems: "center" }}>
            <span style={{ fontSize: "12px", color: "var(--text-dim)", fontWeight: 500, paddingLeft: "4px" }}>
              Quick Launch:
            </span>
            <button
              type="button"
              className="modern-shortcut-chip"
              onClick={() => handleLaunchApp("notepad")}
              disabled={loading}
            >
              Notepad
            </button>
            <button
              type="button"
              className="modern-shortcut-chip"
              onClick={() => handleLaunchApp("calc")}
              disabled={loading}
            >
              Calculator
            </button>
            <button
              type="button"
              className="modern-shortcut-chip"
              onClick={() => handleLaunchApp("explorer")}
              disabled={loading}
            >
              File Explorer
            </button>
            <button
              type="button"
              className="modern-shortcut-chip"
              onClick={() => handleLaunchApp("cmd")}
              disabled={loading}
            >
              Terminal
            </button>
            <button
              type="button"
              className="modern-shortcut-chip"
              onClick={() => handleLaunchApp("code")}
              disabled={loading}
            >
              VS Code
            </button>

            <button
              type="button"
              className="email-secondary-btn"
              style={{ marginLeft: "auto", display: "inline-flex", alignItems: "center", gap: "6px" }}
              onClick={refreshSnapshot}
              disabled={loading}
              title="Refresh desktop window tree"
            >
              <IconRotateCw size={12} className={loading ? "spin" : ""} />
              <span>{loading ? "Scanning..." : "Scan Desktop"}</span>
            </button>
          </div>
        </div>

        {/* Split Container */}
        <div className="email-split-container">
          {/* Left: Active Windows & Accessible Control Tree */}
          <div className="email-list-pane browser-table-pane">
            <div className="browser-table-top" style={{ padding: "10px 14px", borderBottom: "1px solid var(--border-subtle)" }}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <span style={{ fontSize: "12px", fontWeight: 600, color: "var(--text)" }}>
                  Visible Windows ({snapshot.visible_windows?.length || 0})
                </span>
                <span style={{ fontSize: "11px", color: "#38bdf8" }}>
                  Active: {snapshot.active_window || "None"}
                </span>
              </div>
            </div>

            <div className="browser-table-scroll">
              <table className="browser-elements-table">
                <thead>
                  <tr>
                    <th style={{ width: "55px" }}>Ref</th>
                    <th style={{ width: "75px" }}>Role</th>
                    <th>Control Name / Window</th>
                    <th style={{ width: "70px", textAlign: "right" }}>Action</th>
                  </tr>
                </thead>
                <tbody>
                  {snapshot.interactive_refs?.map((item) => (
                    <tr key={item.ref_id} className="browser-elem-row">
                      <td className="browser-ref-code">
                        <code style={{ color: "#38bdf8" }}>{item.ref_id}</code>
                      </td>
                      <td>
                        <span className={`role-tag role-${item.role.toLowerCase()}`}>
                          {item.role}
                        </span>
                      </td>
                      <td className="browser-elem-name" title={item.name}>
                        <div style={{ fontWeight: 500 }}>{item.name || item.role}</div>
                        <div style={{ fontSize: "10px", color: "var(--text-dim)" }}>
                          {item.window_title} {item.center_coords ? `(${item.center_coords[0]},${item.center_coords[1]})` : ""}
                        </div>
                      </td>
                      <td style={{ textAlign: "right" }}>
                        <button
                          type="button"
                          className="browser-btn-ref-copy"
                          onClick={() => handleClickElement(item.ref_id, item.center_coords)}
                          disabled={loading}
                          title="Click this OS control"
                        >
                          <IconZap size={11} className="text-cyan" />
                          <span>Click</span>
                        </button>
                      </td>
                    </tr>
                  ))}
                  {(!snapshot.interactive_refs || snapshot.interactive_refs.length === 0) && (
                    <tr>
                      <td colSpan={4} className="browser-empty-cell">
                        No accessible controls discovered.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>

          {/* Right: Live Interactive Agent Control & Direct Input */}
          <div className="email-detail-pane browser-detail-pane">
            <div className="email-detail-scroll" style={{ display: "flex", flexDirection: "column", gap: "14px" }}>
              {/* Active Window Context Card */}
              <div className="browser-boundary-card" style={{ margin: 0 }}>
                <div className="browser-boundary-icon">
                  <IconFileText size={16} />
                </div>
                <div className="browser-boundary-text">
                  <strong>Active Desktop Target:</strong>
                  <p>
                    Window: <code>{snapshot.active_window}</code> | Process: <code>{snapshot.active_process}</code>
                  </p>
                </div>
              </div>

              {/* Status Message Notification */}
              {statusMessage && (
                <div style={{
                  padding: "8px 12px",
                  borderRadius: "8px",
                  background: "rgba(6, 182, 212, 0.12)",
                  border: "1px solid rgba(6, 182, 212, 0.3)",
                  fontSize: "12px",
                  color: "#38bdf8",
                }}>
                  {statusMessage}
                </div>
              )}

              {/* Direct Keyboard Input Simulation */}
              <div style={{
                background: "rgba(255, 255, 255, 0.025)",
                border: "1px solid var(--border-subtle)",
                borderRadius: "10px",
                padding: "12px 14px",
              }}>
                <div style={{ fontSize: "12px", fontWeight: 600, color: "var(--text)", marginBottom: "8px" }}>
                  Direct Keyboard Input to Active Window:
                </div>
                <div style={{ display: "flex", gap: "8px" }}>
                  <input
                    type="text"
                    className="modern-task-input"
                    value={typeTextInput}
                    onChange={(e) => setTypeTextInput(e.target.value)}
                    placeholder="Enter text to type into active window..."
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        handleSendType();
                      }
                    }}
                  />
                  <button
                    type="button"
                    className="modern-task-submit"
                    onClick={handleSendType}
                    disabled={loading || !typeTextInput.trim()}
                  >
                    <span>Type Text</span>
                  </button>
                </div>

                {/* Quick Hotkeys Strip */}
                <div style={{ display: "flex", gap: "6px", flexWrap: "wrap", marginTop: "10px" }}>
                  <span style={{ fontSize: "11px", color: "var(--text-dim)", alignSelf: "center", marginRight: "4px" }}>
                    Hotkeys:
                  </span>
                  {["Enter", "Tab", "Esc", "Backspace"].map((hk) => (
                    <button
                      key={hk}
                      type="button"
                      className="modern-shortcut-chip"
                      onClick={() => handleSendHotkey([hk])}
                      style={{ fontSize: "11px", padding: "3px 8px" }}
                    >
                      {hk}
                    </button>
                  ))}
                  {[
                    { label: "Ctrl+S", keys: ["Control", "s"] },
                    { label: "Ctrl+C", keys: ["Control", "c"] },
                    { label: "Ctrl+V", keys: ["Control", "v"] },
                    { label: "Alt+Tab", keys: ["Alt", "Tab"] },
                  ].map((hk) => (
                    <button
                      key={hk.label}
                      type="button"
                      className="modern-shortcut-chip"
                      onClick={() => handleSendHotkey(hk.keys)}
                      style={{ fontSize: "11px", padding: "3px 8px" }}
                    >
                      {hk.label}
                    </button>
                  ))}
                </div>
              </div>

              {/* Semantic Accessibility Tree Context Box */}
              <div style={{
                background: "rgba(10, 14, 24, 0.7)",
                border: "1px solid var(--border-subtle)",
                borderRadius: "10px",
                padding: "12px",
                flex: 1,
                minHeight: "160px",
                display: "flex",
                flexDirection: "column",
              }}>
                <div style={{ display: "flex", justifyContent: "space-between", marginBottom: "6px" }}>
                  <span style={{ fontSize: "11.5px", fontWeight: 600, color: "var(--text-dim)" }}>
                    Normalized OS Accessibility Hierarchy (Zero-Vision LLM Payload):
                  </span>
                  <button
                    type="button"
                    style={{ background: "none", border: "none", color: "#38bdf8", fontSize: "11px", cursor: "pointer", display: "inline-flex", gap: "4px", alignItems: "center" }}
                    onClick={() => {
                      navigator.clipboard.writeText(snapshot.semantic_tree_text);
                      setCopiedRef("tree");
                      setTimeout(() => setCopiedRef(null), 1500);
                    }}
                  >
                    {copiedRef === "tree" ? <IconCheck size={11} className="text-emerald" /> : <IconCopy size={11} />}
                    <span>{copiedRef === "tree" ? "Copied" : "Copy Tree"}</span>
                  </button>
                </div>
                <pre style={{
                  fontFamily: "monospace",
                  fontSize: "11px",
                  color: "#cbd5e1",
                  whiteSpace: "pre-wrap",
                  lineHeight: "1.45",
                  margin: 0,
                  overflowY: "auto",
                  maxHeight: "180px",
                }}>
                  {snapshot.semantic_tree_text}
                </pre>
              </div>

              {/* Autonomous Natural Language Desktop Task Bar */}
              <div className="modern-browser-task-bar" style={{ marginTop: "auto" }}>
                <input
                  type="text"
                  className="modern-task-input"
                  value={taskInput}
                  onChange={(e) => setTaskInput(e.target.value)}
                  placeholder="Tell Orion what to automate (e.g. 'Open Notepad and paste the financial summary')..."
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
                      e.preventDefault();
                      handleDelegateToOrion();
                    }
                  }}
                />
                <button
                  type="button"
                  className="modern-btn-analyze-primary"
                  onClick={handleDelegateToOrion}
                  title="Delegate desktop task to Orion Chat"
                >
                  <IconZap size={13} />
                  <span>Ask Orion</span>
                </button>
              </div>
            </div>
          </div>
        </div>

        {/* Capability Broker Audit Footer */}
        <div className="email-modal-footer">
          <div className="email-footer-rule-wrap">
            <IconLock size={12} className="text-cyan" />
            <span className="email-footer-rule-text">
              Capability Broker Audit:
            </span>
            <code className="browser-footer-code">
              [{auditLogEntry.time}] {auditLogEntry.action} • Tier: {auditLogEntry.tier} • Target: {auditLogEntry.target} • Outcome: {auditLogEntry.outcome}
            </code>
          </div>
          <button
            type="button"
            className="email-done-btn"
            onClick={onClose}
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
