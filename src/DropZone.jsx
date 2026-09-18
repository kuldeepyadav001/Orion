import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/**
 * File drop overlay.
 *
 * The renderer deliberately does NOT decide what is acceptable. It hands the
 * paths to Rust, which applies the drop policy (extension allowlist, size cap,
 * symlink refusal, directory walking) and returns a verdict. Trusting the
 * webview to filter would mean an XSS or a compromised dependency could feed
 * arbitrary paths into ingestion.
 *
 * Rejections are shown rather than swallowed. Silently ignoring a dropped file
 * is the worst outcome: the user believes Orion has their document and only
 * finds out otherwise when an answer is wrong.
 */
export default function DropZone({ onAccepted }) {
  const [dragging, setDragging] = useState(false);
  const [result, setResult] = useState(null);

  const handlePaths = useCallback(
    async (paths) => {
      if (!paths || paths.length === 0) return;
      try {
        const assessment = await invoke("assess_dropped_files", { paths });
        setResult(assessment);
        if (assessment.accepted.length > 0) {
          onAccepted?.(assessment.accepted);
        }
      } catch (e) {
        setResult({
          summary: "Those files could not be read",
          accepted: [],
          rejected: [["", String(e)]],
          truncated: false,
        });
      }
    },
    [onAccepted],
  );

  /* Tauri's drag-drop events carry real filesystem paths; the DOM drop event
     does not, which is why we listen here rather than on window. */
  useEffect(() => {
    const unlisten = [];

    listen("tauri://drag-enter", () => setDragging(true)).then((u) =>
      unlisten.push(u),
    );
    listen("tauri://drag-leave", () => setDragging(false)).then((u) =>
      unlisten.push(u),
    );
    listen("tauri://drag-drop", (event) => {
      setDragging(false);
      handlePaths(event.payload?.paths ?? []);
    }).then((u) => unlisten.push(u));

    // Files opened from the tray menu, the command line, or a second launch.
    listen("files://opened", (event) => {
      handlePaths(event.payload ?? []);
    }).then((u) => unlisten.push(u));

    return () => unlisten.forEach((u) => u());
  }, [handlePaths]);

  /* Auto-dismiss a clean result; keep failures on screen until acknowledged,
     because those are the ones the user needs to act on. */
  useEffect(() => {
    if (!result) return;
    if (result.rejected.length > 0) return;
    const t = setTimeout(() => setResult(null), 4000);
    return () => clearTimeout(t);
  }, [result]);

  return (
    <>
      {dragging && (
        <div className="dropzone-overlay" role="presentation">
          <div className="dropzone-inner">
            <div className="dropzone-icon">＋</div>
            <p className="dropzone-title">Drop files to add them to Orion</p>
            <p className="dropzone-hint">
              PDF, Word, Excel, CSV, Markdown, text and code
            </p>
          </div>
        </div>
      )}

      {result && (
        <div
          className={`drop-result ${result.rejected.length > 0 ? "has-errors" : ""}`}
          role="status"
          aria-live="polite"
        >
          <div className="drop-result-head">
            <span>{result.summary}</span>
            <button
              type="button"
              className="drop-result-close"
              onClick={() => setResult(null)}
              aria-label="Dismiss"
            >
              ×
            </button>
          </div>

          {result.rejected.length > 0 && (
            <ul className="drop-rejected">
              {result.rejected.slice(0, 8).map(([name, reason], i) => (
                <li key={`${name}-${i}`}>{reason}</li>
              ))}
              {result.rejected.length > 8 && (
                <li className="drop-more">
                  and {result.rejected.length - 8} more
                </li>
              )}
            </ul>
          )}
        </div>
      )}
    </>
  );
}
