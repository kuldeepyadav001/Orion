import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export function LockScreen({ lockStatus, onUnlocked }) {
  const [passcode, setPasscode] = useState("");
  const [error, setError] = useState("");
  const [showHint, setShowHint] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  const handleUnlock = async (e) => {
    e?.preventDefault();
    if (!passcode.trim() || submitting) return;
    setSubmitting(true);
    setError("");

    try {
      const ok = await invoke("unlock_with_passcode", { passcode });
      if (ok) {
        setPasscode("");
        onUnlocked?.();
      } else {
        setError("Incorrect passcode. Access denied.");
      }
    } catch (err) {
      setError(String(err || "Failed to verify passcode"));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="lock-overlay" role="dialog" aria-modal="true" aria-label="Orion Lock Screen">
      <div className="lock-card">
        <div className="lock-icon-badge">
          <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
            <path d="M7 11V7a5 5 0 0 1 10 0v4" />
          </svg>
        </div>

        <h2 className="lock-title">Orion is Locked</h2>
        <p className="lock-subtitle">
          Protected by master passcode. Enter your security PIN to access models, local data, and conversations.
        </p>

        <form onSubmit={handleUnlock} className="lock-form">
          <div className="lock-input-wrap">
            <input
              type="password"
              className="lock-input"
              placeholder="Enter master passcode"
              value={passcode}
              onChange={(e) => {
                setPasscode(e.target.value);
                if (error) setError("");
              }}
              autoFocus
              autoComplete="current-password"
            />
            <button
              type="submit"
              className="lock-submit-btn"
              disabled={submitting || !passcode}
            >
              {submitting ? "Verifying…" : "Unlock"}
            </button>
          </div>

          {error && <div className="lock-error-msg">{error}</div>}

          {lockStatus?.hint && (
            <div className="lock-hint-section">
              {showHint ? (
                <div className="lock-hint-text">
                  <span className="lock-hint-label">Passcode Hint:</span> {lockStatus.hint}
                </div>
              ) : (
                <button
                  type="button"
                  className="lock-hint-btn"
                  onClick={() => setShowHint(true)}
                >
                  Forgot passcode? Show hint
                </button>
              )}
            </div>
          )}
        </form>
      </div>
    </div>
  );
}

export function LockSettingsModal({ lockStatus, onClose, onStatusChanged }) {
  const [activeTab, setActiveTab] = useState(lockStatus?.enabled ? "change" : "set");
  const [currentPass, setCurrentPass] = useState("");
  const [newPass, setNewPass] = useState("");
  const [confirmPass, setConfirmPass] = useState("");
  const [hint, setHint] = useState("");
  const [msg, setMsg] = useState(null);
  const [loading, setLoading] = useState(false);

  const handleSetPasscode = async (e) => {
    e.preventDefault();
    if (newPass.length < 4) {
      setMsg({ type: "error", text: "Passcode must be at least 4 characters long." });
      return;
    }
    if (newPass !== confirmPass) {
      setMsg({ type: "error", text: "New passcodes do not match." });
      return;
    }

    setLoading(true);
    setMsg(null);
    try {
      await invoke("set_master_lock", {
        passcode: newPass,
        hint: hint.trim() ? hint.trim() : null,
      });
      setMsg({ type: "success", text: "Master passcode configured successfully!" });
      setNewPass("");
      setConfirmPass("");
      setHint("");
      onStatusChanged?.();
      setTimeout(() => onClose?.(), 1200);
    } catch (err) {
      setMsg({ type: "error", text: String(err || "Failed to configure passcode") });
    } finally {
      setLoading(false);
    }
  };

  const handleRemovePasscode = async (e) => {
    e.preventDefault();
    if (!currentPass) {
      setMsg({ type: "error", text: "Please enter current passcode to disable protection." });
      return;
    }

    setLoading(true);
    setMsg(null);
    try {
      const ok = await invoke("remove_master_lock", { currentPasscode: currentPass });
      if (ok) {
        setMsg({ type: "success", text: "Master lock disabled." });
        setCurrentPass("");
        onStatusChanged?.();
        setTimeout(() => onClose?.(), 1000);
      } else {
        setMsg({ type: "error", text: "Incorrect current passcode." });
      }
    } catch (err) {
      setMsg({ type: "error", text: String(err || "Failed to remove passcode") });
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="lock-settings-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <div>
            <h3>Security & Master Lock</h3>
            <p className="modal-subtitle">
              Prevent unauthorized physical access on shared or test computers.
            </p>
          </div>
          <button className="btn-close" onClick={onClose} aria-label="Close">
            ✕
          </button>
        </div>

        {msg && (
          <div className={`status-banner ${msg.type === "error" ? "status-error" : "status-success"}`}>
            {msg.text}
          </div>
        )}

        <div className="lock-settings-tabs">
          <button
            className={`tab-btn ${activeTab === (lockStatus?.enabled ? "change" : "set") ? "active" : ""}`}
            onClick={() => setActiveTab(lockStatus?.enabled ? "change" : "set")}
          >
            {lockStatus?.enabled ? "Update Passcode" : "Enable Master Lock"}
          </button>
          {lockStatus?.enabled && (
            <button
              className={`tab-btn ${activeTab === "remove" ? "active" : ""}`}
              onClick={() => setActiveTab("remove")}
            >
              Disable Lock
            </button>
          )}
        </div>

        {(activeTab === "set" || activeTab === "change") && (
          <form onSubmit={handleSetPasscode} className="lock-settings-form">
            <div className="form-group">
              <label htmlFor="new-passcode">
                {lockStatus?.enabled ? "New Master Passcode" : "Create Master Passcode"}
              </label>
              <input
                id="new-passcode"
                type="password"
                className="text-input"
                placeholder="At least 4 characters"
                value={newPass}
                onChange={(e) => setNewPass(e.target.value)}
                required
              />
            </div>
            <div className="form-group">
              <label htmlFor="confirm-passcode">Confirm Passcode</label>
              <input
                id="confirm-passcode"
                type="password"
                className="text-input"
                placeholder="Re-enter passcode"
                value={confirmPass}
                onChange={(e) => setConfirmPass(e.target.value)}
                required
              />
            </div>
            <div className="form-group">
              <label htmlFor="hint-input">Security Hint (Optional)</label>
              <input
                id="hint-input"
                type="text"
                className="text-input"
                placeholder="e.g. Favorite childhood street"
                value={hint}
                onChange={(e) => setHint(e.target.value)}
              />
            </div>
            <div className="modal-actions">
              <button type="button" className="btn-secondary" onClick={onClose}>
                Cancel
              </button>
              <button type="submit" className="btn-primary" disabled={loading}>
                {loading ? "Saving…" : "Save Passcode"}
              </button>
            </div>
          </form>
        )}

        {activeTab === "remove" && (
          <form onSubmit={handleRemovePasscode} className="lock-settings-form">
            <p className="warning-notice">
              Removing the master passcode allows anyone sitting at this computer to open Orion and access conversations and documents without entering a PIN.
            </p>
            <div className="form-group">
              <label htmlFor="curr-passcode">Enter Current Passcode to Confirm</label>
              <input
                id="curr-passcode"
                type="password"
                className="text-input"
                placeholder="Current master passcode"
                value={currentPass}
                onChange={(e) => setCurrentPass(e.target.value)}
                required
              />
            </div>
            <div className="modal-actions">
              <button type="button" className="btn-secondary" onClick={onClose}>
                Cancel
              </button>
              <button type="submit" className="btn-danger" disabled={loading}>
                {loading ? "Removing…" : "Disable Protection"}
              </button>
            </div>
          </form>
        )}
      </div>
    </div>
  );
}
