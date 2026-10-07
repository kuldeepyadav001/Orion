import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * Shows what Orion detected about this machine, which tier it chose, and why.
 *
 * The reasoning is deliberately visible rather than hidden behind a spinner:
 * a user whose machine was profiled as T1 should be able to see that it was
 * free RAM, not an arbitrary decision, that produced the recommendation.
 */
export default function SystemPanel({ onClose, onPersonaChanged }) {
  const [profile, setProfile] = useState(null);
  const [rec, setRec] = useState(null);
  const [models, setModels] = useState([]);
  const [active, setActive] = useState(null);
  const [personas, setPersonas] = useState([]);
  const [activePersona, setActivePersona] = useState(null);
  const [specialistPersona, setSpecialistPersona] = useState({
    id: "developer",
    name: "Software Developer",
    icon: "💻",
    tagline: "Architectural precision, idiomatic syntax, zero boilerplate",
  });
  const [audits, setAudits] = useState([]);
  const [showAudits, setShowAudits] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  useEffect(() => {
    (async () => {
      try {
        const [p, r, m, a, plist, actP] = await Promise.all([
          invoke("hardware_profile"),
          invoke("tier_recommendation"),
          invoke("list_models"),
          invoke("active_tier"),
          invoke("list_personas"),
          invoke("get_active_persona"),
        ]);
        setProfile(p);
        setRec(r);
        setModels(m);
        setActive(a);
        setPersonas(plist || []);
        setActivePersona(actP);

        // Load saved specialist preference from localStorage or active non-general persona
        const savedSpec = localStorage.getItem("orion_specialist_persona");
        if (savedSpec) {
          try {
            setSpecialistPersona(JSON.parse(savedSpec));
          } catch {
            /* ignore parse err */
          }
        } else if (actP && actP.id !== "general") {
          setSpecialistPersona(actP);
        } else if (plist && plist.length > 0) {
          const firstNonGeneral = plist.find((item) => item.id !== "general");
          if (firstNonGeneral) setSpecialistPersona(firstNonGeneral);
        }
      } catch (e) {
        setError(String(e));
      }
    })();
  }, []);

  const chooseTier = async (tier) => {
    setBusy(true);
    try {
      const next = await invoke("set_active_tier", { tier });
      setActive(next);
      setModels(await invoke("list_models"));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const choosePersona = async (id) => {
    try {
      const updated = await invoke("set_active_persona", { persona: id });
      setActivePersona(updated);
      onPersonaChanged?.(updated);
    } catch (e) {
      setError(String(e));
    }
  };

  const loadAudits = async () => {
    try {
      const list = await invoke("broker_audit_log", { limit: 15 });
      setAudits(list);
      setShowAudits((v) => !v);
    } catch (e) {
      setError(String(e));
    }
  };

  const stateLabel = {
    installed: "Installed",
    notinstalled: "Not installed",
    downloading: "Downloading…",
    corrupt: "Corrupt",
  };

  return (
    <div className="panel-backdrop" onClick={onClose}>
      <div className="panel" onClick={(e) => e.stopPropagation()}>
        <div className="panel-head">
          <h2>System, Personas &amp; Security</h2>
          <button className="ghost" onClick={onClose}>
            Close
          </button>
        </div>

        {error && <div className="alert err">{error}</div>}

        {profile?.simulated && (
          <div className="alert warn">
            <strong>Simulated hardware profile.</strong> These figures come from
            <code> ORION_FORCE_PROFILE</code>, not from this machine.
          </div>
        )}

        <section>
          <h3>Workload Persona (Dual-Mode Architecture)</h3>
          <p className="muted">
            Orion strictly operates in two modes: General Assistant and your dedicated Specialist.
          </p>
          <div className="persona-grid-compact two-modes-grid">
            {/* Mode 1: General Assistant */}
            <div
              className={`persona-card-compact ${activePersona?.id === "general" ? "active" : ""}`}
              onClick={() => choosePersona("general")}
            >
              <div className="persona-compact-head">
                <span className="persona-icon">⚡</span>
                <strong>General Assistant</strong>
                {activePersona?.id === "general" && <span className="active-pill">In use</span>}
              </div>
              <p className="persona-tagline-compact">
                Fast front-door router, daily tasks, balanced reasoning &amp; document tools
              </p>
            </div>

            {/* Mode 2: Dedicated Specialist */}
            <div
              className={`persona-card-compact ${activePersona?.id !== "general" ? "active" : ""}`}
              onClick={() => choosePersona(specialistPersona.id)}
            >
              <div className="persona-compact-head">
                <span className="persona-icon">{specialistPersona.icon}</span>
                <strong>{specialistPersona.name} (Specialist)</strong>
                {activePersona?.id !== "general" && <span className="active-pill">In use</span>}
              </div>
              <p className="persona-tagline-compact">{specialistPersona.tagline}</p>
            </div>
          </div>

          <div className="specialist-reconfigure-box">
            <span className="specialist-reconfigure-label">Assigned Specialist Domain:</span>
            <div className="specialist-pill-group">
              {personas
                .filter((p) => p.id !== "general")
                .map((sp) => (
                  <button
                    key={sp.id}
                    type="button"
                    className={`specialist-pill-btn ${specialistPersona.id === sp.id ? "selected" : ""}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      setSpecialistPersona(sp);
                      localStorage.setItem("orion_specialist_persona", JSON.stringify(sp));
                      if (activePersona?.id !== "general") {
                        choosePersona(sp.id);
                      }
                    }}
                  >
                    <span>{sp.icon}</span>
                    <span>{sp.name}</span>
                  </button>
                ))}
            </div>
          </div>

          <div className="persona-weights-guidance">
            <div className="weights-guidance-title">
              <span>💡</span>
              <strong>Sequential Dual-Model Architecture</strong>
            </div>
            <p className="weights-guidance-desc">
              Orion maintains your General Model and your chosen Dedicated Model on disk, swapping them in RAM sequentially so memory stays safely under your 5.7 GB limit.
            </p>
            <div className="weights-download-box">
              <button
                type="button"
                className="btn-system-download"
                onClick={() => {
                  onClose?.();
                  if (typeof window !== "undefined") {
                    window.dispatchEvent(new CustomEvent("open-resource-downloader"));
                  }
                }}
              >
                📥 Download &amp; Verify All Offline Models &amp; Voices
              </button>
              <span className="download-subtext">
                Downloads both General &amp; Dedicated models directly inside Orion with a real-time progress bar. Zero terminal commands needed.
              </span>
            </div>
          </div>
        </section>

        {profile && (
          <section>
            <h3>This machine</h3>
            <dl className="kv">
              <div>
                <dt>Memory</dt>
                <dd>
                  {profile.available_ram_gib.toFixed(1)} GiB free of{" "}
                  {profile.total_ram_gib.toFixed(1)} GiB
                </dd>
              </div>
              <div>
                <dt>CPU</dt>
                <dd>
                  {profile.cpu_cores} cores · {profile.cpu_brand}
                </dd>
              </div>
              <div>
                <dt>GPU</dt>
                <dd>
                  {profile.gpu_vendor
                    ? `${profile.gpu_vendor}${
                        profile.gpu_vram_gib
                          ? ` · ${profile.gpu_vram_gib.toFixed(1)} GiB VRAM`
                          : " · unified memory"
                      }`
                    : "None detected — CPU inference"}
                </dd>
              </div>
              <div>
                <dt>Disk</dt>
                <dd>{profile.free_disk_gib.toFixed(0)} GiB free</dd>
              </div>
              <div>
                <dt>Platform</dt>
                <dd>
                  {profile.os} · {profile.arch}
                </dd>
              </div>
            </dl>
          </section>
        )}

        {rec && (
          <section>
            <h3>
              Recommended tier: <span className="tier">{rec.tier}</span> {rec.label}
            </h3>
            <ul className="reasons">
              {rec.reasons.map((r, i) => (
                <li key={i}>{r}</li>
              ))}
            </ul>
            {rec.warnings.length > 0 && (
              <ul className="reasons warn-list">
                {rec.warnings.map((w, i) => (
                  <li key={i}>{w}</li>
                ))}
              </ul>
            )}
          </section>
        )}

        <section>
          <h3>Models</h3>
          <p className="muted">
            Orion picked <strong>{active ?? "—"}</strong>. You can override it; the change
            applies the next time the engine starts.
          </p>
          <table className="models">
            <thead>
              <tr>
                <th>Model</th>
                <th>Tier</th>
                <th>Size</th>
                <th>Licence</th>
                <th>State</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {models.map((m) => (
                <tr key={m.spec.id} className={m.spec.tier === active ? "active" : ""}>
                  <td>{m.spec.name}</td>
                  <td>
                    <span className="tier small">{m.spec.tier}</span>
                  </td>
                  <td>{m.spec.size_gib.toFixed(1)} GiB</td>
                  <td className="muted">{m.spec.license}</td>
                  <td>
                    <span className={`badge ${m.state}`}>
                      {stateLabel[m.state] ?? m.state}
                    </span>
                  </td>
                  <td>
                    <button
                      className="ghost small"
                      disabled={busy || m.spec.tier === active}
                      onClick={() => chooseTier(m.spec.tier)}
                    >
                      {m.spec.tier === active ? "In use" : "Use"}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="muted tiny">
            Models are downloaded separately with <code>scripts/fetch-model.sh</code>. Only
            permissively licensed weights are bundled in offline builds.
          </p>
        </section>

        <section>
          <div className="section-head-split">
            <h3>Capability Broker &amp; Audit Log</h3>
            <button className="ghost small" onClick={loadAudits}>
              {showAudits ? "Hide Audit Trail" : "View Audit Trail"}
            </button>
          </div>
          <p className="muted">
            Two-domain isolation and path containment (<code>RESOLVE_BENEATH</code>) are active.
            All tool interactions are recorded in an append-only audit log.
          </p>

          {showAudits && (
            <div className="audit-log-container">
              {audits.length === 0 ? (
                <p className="muted tiny">No audit records logged yet.</p>
              ) : (
                <table className="models audit-table">
                  <thead>
                    <tr>
                      <th>Time</th>
                      <th>Domain</th>
                      <th>Action</th>
                      <th>Tier</th>
                      <th>Decision</th>
                    </tr>
                  </thead>
                  <tbody>
                    {audits.map((a) => (
                      <tr key={a.id}>
                        <td className="muted tiny">{new Date(a.timestamp).toLocaleTimeString()}</td>
                        <td>
                          <span className={`badge ${a.domain}`}>{a.domain}</span>
                        </td>
                        <td className="tiny">{a.action}</td>
                        <td>
                          <span className="tier small">{a.tier}</span>
                        </td>
                        <td>
                          <span className={`badge ${a.decision.toLowerCase()}`}>
                            {a.decision}
                          </span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
