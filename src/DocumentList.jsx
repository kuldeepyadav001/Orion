import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * The indexed library: what is stored, and how to remove it.
 *
 * This exists because the previous panel showed a count and nothing else. A
 * user could see "1 document" and never learn which file it was, when it was
 * added, or how to delete it — and documents persist in SQLite across
 * restarts, so something added weeks ago stayed invisible and unremovable.
 *
 * For a product whose entire promise is "your files never leave this
 * machine", being unable to see or delete what it holds is a trust problem,
 * not a convenience one.
 */
export default function DocumentList({ onCountChange }) {
  const [docs, setDocs] = useState([]);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState(null);
  const [error, setError] = useState(null);
  const [confirming, setConfirming] = useState(null);

  const refresh = useCallback(async () => {
    try {
      const list = await invoke("list_documents");
      setDocs(list);
      onCountChange?.(list.length);
    } catch (e) {
      setError(String(e));
    }
  }, [onCountChange]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const addFiles = async () => {
    setError(null);
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
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
      setBusy(true);

      // Indexing a large PDF takes seconds and the window would otherwise
      // look frozen. Report which file, and how far through the batch.
      for (let i = 0; i < paths.length; i += 1) {
        const name = paths[i].split(/[\\/]/).pop();
        setProgress({ name, index: i + 1, total: paths.length });
        try {
          await invoke("add_documents", { paths: [paths[i]] });
        } catch (e) {
          setError(String(e));
        }
        await refresh();
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };

  const remove = async (id) => {
    setError(null);
    try {
      await invoke("forget_document", { documentId: id });
      setConfirming(null);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  const fmtDate = (iso) => {
    try {
      return new Date(iso).toLocaleDateString(undefined, {
        day: "numeric",
        month: "short",
      });
    } catch {
      return "";
    }
  };

  return (
    <div className="doclist">
      <div className="doclist-actions">
        <button className="btn-primary" onClick={addFiles} disabled={busy}>
          {busy ? "Indexing…" : "Add files"}
        </button>
      </div>

      {progress && (
        <div className="progress-block">
          <div className="progress-label">
            <span className="truncate">{progress.name}</span>
            <span className="muted">
              {progress.index} / {progress.total}
            </span>
          </div>
          <div className="progress-track">
            <div
              className="progress-fill"
              style={{
                width: `${(progress.index / progress.total) * 100}%`,
              }}
            />
          </div>
        </div>
      )}

      {error && <div className="alert">{error}</div>}

      {docs.length === 0 && !busy && (
        <div className="empty-lib">
          <p>No documents yet.</p>
          <p className="muted small">
            Add a PDF, Word file or spreadsheet and Orion will answer from it,
            with citations. Everything stays on this machine.
          </p>
        </div>
      )}

      <ul className="doc-items">
        {docs.map((d) => (
          <li key={d.id} className="doc-item">
            <div className="doc-icon">{(d.format || "?").slice(0, 3)}</div>
            <div className="doc-meta">
              <div className="doc-name truncate" title={d.name}>
                {d.name}
              </div>
              <div className="doc-sub">
                {d.chunks} passages
                {d.pages ? ` · ${d.pages} pages` : ""}
                {d.indexed_at ? ` · ${fmtDate(d.indexed_at)}` : ""}
              </div>
            </div>

            {confirming === d.id ? (
              <div className="doc-confirm">
                <button className="btn-danger" onClick={() => remove(d.id)}>
                  Delete
                </button>
                <button
                  className="btn-ghost"
                  onClick={() => setConfirming(null)}
                >
                  Cancel
                </button>
              </div>
            ) : (
              <button
                className="doc-remove"
                title="Remove from library"
                aria-label={`Remove ${d.name}`}
                onClick={() => setConfirming(d.id)}
              >
                ×
              </button>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
