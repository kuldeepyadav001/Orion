import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * Shows what Orion detected about this machine, which tier it chose, and why.
 *
 * The reasoning is deliberately visible rather than hidden behind a spinner:
 * a user whose machine was profiled as T1 should be able to see that it was
 * free RAM, not an arbitrary decision, that produced the recommendation.
 */
export default function SystemPanel({ onClose }) {
  const [profile, setProfile] = useState(null);
  const [rec, setRec] = useState(null);
  const [models, setModels] = useState([]);
  const [active, setActive] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  useEffect(() => {
    (async () => {
      try {
        const [p, r, m, a] = await Promise.all([
          invoke("hardware_profile"),
          invoke("tier_recommendation"),
          invoke("list_models"),
          invoke("active_tier"),
        ]);
        setProfile(p);
        setRec(r);
        setModels(m);
        setActive(a);
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
          <h2>System &amp; models</h2>
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
      </div>
    </div>
  );
}
