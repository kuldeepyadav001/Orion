import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * Document library panel.
 *
 * Files are chosen through a native dialog, so the webview never handles
 * filesystem paths it invented itself — the Rust side decides what is
 * readable and what is refused.
 *
 * Semantic search state is shown honestly. When the embedding model is
 * missing, Orion still searches by keyword, and saying so is better than
 * letting the user wonder why paraphrased questions miss.
 */
export default function Documents({ open, onClose }) {
  const [status, setStatus] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);
  const [added, setAdded] = useState([]);

  const refresh = useCallback(async () => {
    try {
      setStatus(await invoke("library_status"));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    refresh();
    // The embedder warms up after the window appears, so poll briefly rather
    // than showing "unavailable" for the first few seconds of every launch.
    const t = setInterval(refresh, 2000);
    return () => clearInterval(t);
  }, [open, refresh]);

  const addFiles = async () => {
    setError(null);
    setBusy(true);
    try {
      const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
      const picked = await openDialog({
        multiple: true,
        filters: [
          {
            name: "Documents",
            extensions: [
              "pdf", "docx", "xlsx", "csv", "tsv", "md", "markdown",
              "html", "htm", "txt", "log", "rs", "py", "js", "ts",
              "json", "toml", "yaml", "yml",
            ],
          },
        ],
      });
      if (!picked) return;

      const paths = Array.isArray(picked) ? picked : [picked];
      const summaries = await invoke("add_documents", { paths });
      setAdded(summaries);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  if (!open) return null;

  const embedState = status?.embed_state ?? "idle";
  const embedLabel = {
    idle: "Starting…",
    starting: "Starting…",
    ready: "Semantic search on",
    unavailable: "Keyword search only",
  }[embedState];

  return (
    <div className="panel-backdrop" onClick={onClose}>
      <div className="panel" onClick={(e) => e.stopPropagation()}>
        <header className="panel-head">
          <h2>Documents</h2>
          <button className="ghost" onClick={onClose}>
            Close
          </button>
        </header>

        <section>
          <p className="muted">
            Files are read, split and indexed on this machine. Nothing is
            uploaded. Ask a question and Orion will answer from them, with
            citations.
          </p>

          <div className="lib-stats">
            <div>
              <strong>{status?.documents ?? 0}</strong>
              <span>documents</span>
            </div>
            <div>
              <strong>{status?.chunks ?? 0}</strong>
              <span>passages</span>
            </div>
            <div>
              <span className={`badge ${embedState}`}>{embedLabel}</span>
            </div>
          </div>

          {embedState === "unavailable" && (
            <p className="muted small">
              {status?.embed_detail}
              <br />
              Run <code>scripts/fetch-embed-model.sh</code> to enable semantic
              search. Keyword search works without it.
            </p>
          )}

          <button className="primary" onClick={addFiles} disabled={busy}>
            {busy ? "Indexing…" : "Add files…"}
          </button>

          {error && <p className="error">{error}</p>}

          {added.length > 0 && (
            <ul className="added">
              {added.map((d) => (
                <li key={d.document_id}>
                  <strong>{d.name}</strong> — {d.chunks} passages
                  {d.pages ? `, ${d.pages} pages` : ""}
                  {!d.embedded && (
                    <span className="muted"> (keyword only)</span>
                  )}
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </div>
  );
}
