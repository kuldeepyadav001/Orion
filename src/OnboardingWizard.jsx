import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Logo from "./Logo";

/**
 * First-run onboarding wizard (M6).
 *
 * Guides new users through hardware profiling, privacy boundaries,
 * and workload persona selection before their first turn.
 */
export default function OnboardingWizard({ onComplete }) {
  const [step, setStep] = useState(1);
  const [profile, setProfile] = useState(null);
  const [personas, setPersonas] = useState([]);
  const [selectedPersona, setSelectedPersona] = useState("general");
  const [saving, setSaving] = useState(false);

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

            <div className="onboarding-actions">
              <button className="ghost" onClick={() => setStep(1)}>
                ← Back
              </button>
              <button className="btn-primary" onClick={() => setStep(3)}>
                Next: Ready to Use →
              </button>
            </div>
          </div>
        )}

        {step === 3 && (
          <div className="onboarding-step">
            <h3>You Are Ready to Go!</h3>
            <p className="onboarding-desc">
              Here is everything you need to know to get the most out of Orion:
            </p>

            <ul className="readiness-list">
              <li>
                <strong>⌨️ Global Hotkey:</strong> Press <kbd>Ctrl+Shift+0</kbd> anywhere on your computer to summon or hide Orion instantly.
              </li>
              <li>
                <strong>🎙️ Voice Talking Mode:</strong> Click the microphone or speak aloud; Orion listens, transcribes, and answers with natural speech.
              </li>
              <li>
                <strong>📂 Document Intelligence:</strong> Drag and drop PDFs, Word docs, spreadsheets, or text files directly into the window for offline cited RAG search.
              </li>
              <li>
                <strong>🛡️ Permission Broker:</strong> Destructive actions always require your explicit confirmation ticket. Nothing modifies your files without approval.
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
    </div>
  );
}
