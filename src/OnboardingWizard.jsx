import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import Logo from "./Logo";

/**
 * First-run onboarding wizard (M6).
 *
 * Guides new users through hardware profiling, privacy boundaries,
 * workload persona selection, and in-app automated resource downloading.
 */
export default function OnboardingWizard({ onComplete }) {
  const [step, setStep] = useState(1);
  const [profile, setProfile] = useState(null);
  const [personas, setPersonas] = useState([]);
  const [selectedPersona, setSelectedPersona] = useState("general");
  const [saving, setSaving] = useState(false);

  // Resource downloader state
  const [resStatus, setResStatus] = useState(null);
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState(null);
  const [downloadError, setDownloadError] = useState(null);

  useEffect(() => {
    (async () => {
      try {
        const [p, personaList] = await Promise.all([
          invoke("hardware_profile"),
          invoke("list_personas"),
        ]);
        setProfile(p);
        setPersonas(personaList);
      } catch (e) {
        console.error("failed loading onboarding data:", e);
      }
    })();
  }, []);

  const refreshResources = useCallback(async (persona) => {
    try {
      const status = await invoke("check_resource_status", {
        persona: persona || selectedPersona,
      });
      setResStatus(status);
    } catch (e) {
      console.error("failed checking resource status:", e);
    }
  }, [selectedPersona]);

  useEffect(() => {
    if (step === 3) {
      refreshResources(selectedPersona);
    }
  }, [step, selectedPersona, refreshResources]);

  // Listen to download progress events
  useEffect(() => {
    let unlistenProgress;
    let unlistenDone;

    listen("download://progress", (event) => {
      const p = event.payload;
      setProgress(p);
      if (p.status === "error") {
        setDownloadError(p.error_msg || "Download failed");
        setDownloading(false);
      }
    }).then((un) => {
      unlistenProgress = un;
    });

    listen("download://all_complete", () => {
      setDownloading(false);
      setProgress(null);
      refreshResources(selectedPersona);
    }).then((un) => {
      unlistenDone = un;
    });

    return () => {
      unlistenProgress?.();
      unlistenDone?.();
    };
  }, [selectedPersona, refreshResources]);

  const startDownload = async () => {
    setDownloading(true);
    setDownloadError(null);
    try {
      await invoke("start_resource_download", { persona: selectedPersona });
    } catch (e) {
      setDownloadError(String(e));
      setDownloading(false);
    }
  };

  const cancelDownload = async () => {
    try {
      await invoke("cancel_resource_download");
      setDownloading(false);
      setProgress(null);
    } catch (e) {
      console.error("failed to cancel download:", e);
    }
  };

  const handleFinish = async () => {
    setSaving(true);
    try {
      await invoke("complete_onboarding", { persona: selectedPersona });
      onComplete?.(selectedPersona);
    } catch (e) {
      console.error("failed completing onboarding:", e);
      onComplete?.(selectedPersona);
    }
  };

  return (
    <div className="panel-backdrop">
      <div className="panel onboarding-panel" onClick={(e) => e.stopPropagation()}>
        <div className="onboarding-header">
          <Logo size={36} />
          <div className="onboarding-titles">
            <h2>Welcome to Orion</h2>
            <p className="muted">Your private, offline-first personal AI workspace</p>
          </div>
        </div>

        <div className="onboarding-progress">
          <div className={`step-dot ${step >= 1 ? "active" : ""}`} />
          <div className={`step-line ${step >= 2 ? "active" : ""}`} />
          <div className={`step-dot ${step >= 2 ? "active" : ""}`} />
          <div className={`step-line ${step >= 3 ? "active" : ""}`} />
          <div className={`step-dot ${step >= 3 ? "active" : ""}`} />
        </div>

        {step === 1 && (
          <div className="onboarding-step">
            <h3>Hardware Profiling &amp; Privacy</h3>
            <p className="onboarding-desc">
              Orion runs 100% locally on your computer. Your conversations, documents,
              and voice audio never leave your device.
            </p>

            <div className="profile-summary">
              <div className="profile-card">
                <span className="profile-icon">🧠</span>
                <div className="profile-info">
                  <strong>Memory</strong>
                  <span>
                    {profile ? `${profile.available_ram_gib.toFixed(1)} GiB free (${profile.total_ram_gib.toFixed(1)} GiB total)` : "Detecting…"}
                  </span>
                </div>
              </div>
              <div className="profile-card">
                <span className="profile-icon">⚡</span>
                <div className="profile-info">
                  <strong>Inference Engine</strong>
                  <span>
                    {profile?.gpu_vendor ? `${profile.gpu_vendor} Acceleration` : "CPU Native Engine"}
                  </span>
                </div>
              </div>
              <div className="profile-card">
                <span className="profile-icon">🛡️</span>
                <div className="profile-info">
                  <strong>Security Boundary</strong>
                  <span>Capability Broker &amp; Two-Domain Isolation Active</span>
                </div>
              </div>
            </div>

            <div className="onboarding-actions">
              <span className="step-hint">Step 1 of 3</span>
              <button className="btn-primary" onClick={() => setStep(2)}>
                Next: Select Persona →
              </button>
            </div>
          </div>
        )}

        {step === 2 && (
          <div className="onboarding-step">
            <h3>Choose Your Workload Persona</h3>
            <p className="onboarding-desc">
              Tailors Orion's reasoning depth, tone, and syntax specialization to your workflow.
              You can change this at any time.
            </p>

            <div className="persona-grid">
              {personas.map((p) => {
                const isSelected = p.id === selectedPersona;
                return (
                  <div
                    key={p.id}
                    className={`persona-card ${isSelected ? "selected" : ""}`}
                    onClick={() => setSelectedPersona(p.id)}
                  >
                    <div className="persona-head">
                      <span className="persona-icon">{p.icon}</span>
                      <span className="persona-title">{p.name}</span>
                    </div>
                    <p className="persona-tagline">{p.tagline}</p>
                    <p className="persona-desc">{p.description}</p>
                  </div>
                );
              })}
            </div>

            <p className="onboarding-persona-note">
              💡 <strong>Dual-Model Setup:</strong> Orion equips your computer with a fast General Model plus your chosen Dedicated Professional Model, swapping them in RAM sequentially without freezing memory!
            </p>

            <div className="onboarding-actions">
              <button className="ghost" onClick={() => setStep(1)}>
                ← Back
              </button>
              <button className="btn-primary" onClick={() => setStep(3)}>
                Next: Setup Resources →
              </button>
            </div>
          </div>
        )}

        {step === 3 && (
          <div className="onboarding-step">
            {resStatus && !resStatus.all_ready ? (
              <div className="resource-setup-section">
                <h3>Download Offline Resources</h3>
                <p className="onboarding-desc">
                  To operate 100% offline without cloud servers, Orion needs to download the foundational models to your computer:
                </p>

                <div className="resource-download-list">
                  {resStatus.missing_items.map((item) => (
                    <div key={item.id} className="resource-download-item">
                      <div className="res-meta">
                        <span className="res-icon">
                          {item.category === "llm" ? "🧠" : item.category === "coder" ? "💻" : item.category === "whisper" ? "🎙️" : "🔊"}
                        </span>
                        <div>
                          <strong>{item.name}</strong>
                          <span className="res-size">~{(item.approx_size_mb / 1024).toFixed(1)} GB</span>
                        </div>
                      </div>
                      <span className="badge-pending">Pending</span>
                    </div>
                  ))}

                  {resStatus.installed_items.map((item) => (
                    <div key={item.id} className="resource-download-item completed">
                      <div className="res-meta">
                        <span className="res-icon">✓</span>
                        <div>
                          <strong>{item.name}</strong>
                          <span className="res-size">Installed</span>
                        </div>
                      </div>
                      <span className="badge-installed">Ready</span>
                    </div>
                  ))}
                </div>

                {downloadError && (
                  <div className="alert-error">
                    {downloadError}
                  </div>
                )}

                {downloading && progress ? (
                  <div className="download-progress-container">
                    <div className="progress-label-row">
                      <span className="progress-item-name">{progress.item_name}</span>
                      <span className="progress-pct">{progress.percentage.toFixed(0)}%</span>
                    </div>
                    <div className="progress-bar-track">
                      <div
                        className="progress-bar-fill"
                        style={{ width: `${progress.percentage}%` }}
                      />
                    </div>
                    <div className="progress-metrics-row">
                      <span>
                        {(progress.current_bytes / (1024 * 1024)).toFixed(0)} MB / {(progress.total_bytes / (1024 * 1024)).toFixed(0)} MB
                      </span>
                      <span>{progress.speed_mb_s.toFixed(1)} MB/s</span>
                      <span>Item {progress.current_step} of {progress.total_steps}</span>
                    </div>
                  </div>
                ) : null}

                <div className="onboarding-actions">
                  <button className="ghost" onClick={() => setStep(2)} disabled={downloading}>
                    ← Back
                  </button>
                  {downloading ? (
                    <button className="btn-secondary" onClick={cancelDownload}>
                      Cancel Download
                    </button>
                  ) : (
                    <button className="btn-primary" onClick={startDownload}>
                      Download &amp; Install Models ({((resStatus.total_download_mb || 2200) / 1024).toFixed(1)} GB)
                    </button>
                  )}
                </div>
              </div>
            ) : (
              <div className="resource-ready-section">
                <div className="ready-badge-banner">
                  <span className="ready-check-icon">✓</span>
                  <h3>All Models &amp; Resources Installed</h3>
                </div>
                <p className="onboarding-desc">
                  Your offline intelligence engine is fully configured. Everything is stored locally in your user profile:
                </p>

                <ul className="readiness-list">
                  <li>
                    <strong>🔄 Sequential Dual-Model Engine:</strong> General Model &amp; Dedicated {selectedPersona} Model operate sequentially in RAM under your 5.7 GB memory limit.
                  </li>
                  <li>
                    <strong>⌨️ Global Hotkey:</strong> Press <kbd>Ctrl+Shift+0</kbd> anywhere on your computer to summon or hide Orion.
                  </li>
                  <li>
                    <strong>🎙️ Voice Talking Mode:</strong> Offline Whisper STT &amp; Piper neural voice ready for conversational speech.
                  </li>
                  <li>
                    <strong>📂 Document Intelligence:</strong> Drag &amp; drop PDFs, Word files, or notes for cited local RAG search.
                  </li>
                </ul>

                <div className="onboarding-actions">
                  <button className="ghost" onClick={() => setStep(2)} disabled={saving}>
                    ← Back
                  </button>
                  <button className="btn-primary" onClick={handleFinish} disabled={saving}>
                    {saving ? "Launching…" : "Launch Orion 🚀"}
                  </button>
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
