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
        setPersonas(plist);
        setActivePersona(actP);
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
          <h3>Workload Persona</h3>
          <p className="muted">
            Specializes Orion's reasoning depth, tone, and syntax for your immediate domain.
          </p>
          <div className="persona-grid-compact">
            {personas.map((p) => {
              const isActive = activePersona?.id === p.id;
              return (
                <div
                  key={p.id}
                  className={`persona-card-compact ${isActive ? "active" : ""}`}
                  onClick={() => choosePersona(p.id)}
                >
                  <div className="persona-compact-head">
                    <span className="persona-icon">{p.icon}</span>
                    <strong>{p.name}</strong>
                    {isActive && <span className="active-pill">In use</span>}
                  </div>
                  <p className="persona-tagline-compact">{p.tagline}</p>
                </div>
              );
            })}
          </div>

          <div className="persona-weights-guidance">
            <div className="weights-guidance-title">
              <span>💡</span>
              <strong>Persona Conditioning vs. Dedicated Model Weights</strong>
            </div>
            <p className="weights-guidance-desc">
              Selecting <strong>{activePersona?.name || "a persona"}</strong> immediately specializes
              Orion&apos;s expert instructions, reasoning style, and code generation standards on top of your
              active model without requiring extra gigabytes of downloads.
            </p>
            <div className="weights-download-box">
              <span className="download-label">To install dedicated fine-tuned weights (e.g. Qwen2.5-Coder):</span>
              <code className="download-command">./scripts/fetch-model.sh coder</code>
              <span className="download-subtext">
                When downloaded into <code>models/</code>, Orion automatically activates dedicated coder weights whenever Developer mode is selected!
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
