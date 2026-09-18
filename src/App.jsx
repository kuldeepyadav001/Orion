import { useCallback, useEffect, useRef, useState } from "react";
import Markdown from "react-markdown";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import SystemPanel from "./SystemPanel";
import Documents from "./Documents";

/**
 * Orion M0 shell.
 *
 * Streaming works via Tauri events rather than HTTP: the Rust core owns the
 * connection to llama-server and emits `chat://token` events as tokens arrive.
 * The webview never talks to the model directly — that keeps the sidecar's
 * port and auth token out of the frontend entirely.
 */
export default function App() {
  const [messages, setMessages] = useState([]);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [engine, setEngine] = useState({ state: "starting", detail: "" });
  const [showSystem, setShowSystem] = useState(false);
  const [showDocs, setShowDocs] = useState(false);
  const [citations, setCitations] = useState([]);

  const chatRef = useRef(null);
  const taRef = useRef(null);
  // Buffers the in-flight assistant reply so we aren't re-rendering off state
  // that may lag behind a fast token stream.
  const pending = useRef("");

  /* ---------- engine status ---------- */

  // Citations arrive just before the answer streams, so the sources can be
  // shown alongside the reply rather than after it finishes.
  useEffect(() => {
    let un;
    listen("chat://citations", (e) => setCitations(e.payload ?? [])).then(
      (u) => {
        un = u;
      },
    );
    return () => un?.();
  }, []);

  useEffect(() => {
    let alive = true;

    const poll = async () => {
      try {
        const s = await invoke("engine_status");
        if (alive) setEngine(s);
      } catch (e) {
        if (alive) setEngine({ state: "error", detail: String(e) });
      }
    };

    poll();
    const id = setInterval(poll, 2000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  /* ---------- token stream ---------- */

  useEffect(() => {
    const unlistenToken = listen("chat://token", (e) => {
      pending.current += e.payload;
      setMessages((prev) => {
        const next = [...prev];
        const last = next[next.length - 1];
        if (last?.role === "assistant") last.content = pending.current;
        return next;
      });
    });

    const unlistenDone = listen("chat://done", () => {
      pending.current = "";
      setStreaming(false);
    });

    const unlistenErr = listen("chat://error", (e) => {
      pending.current = "";
      setStreaming(false);
      setMessages((prev) => {
        const next = [...prev];
        const last = next[next.length - 1];
        const msg = `⚠️ ${e.payload}`;
        if (last?.role === "assistant" && !last.content) last.content = msg;
        else next.push({ role: "assistant", content: msg });
        return next;
      });
    });

    return () => {
      unlistenToken.then((f) => f());
      unlistenDone.then((f) => f());
      unlistenErr.then((f) => f());
    };
  }, []);

  /* ---------- autoscroll ---------- */

  useEffect(() => {
    const el = chatRef.current;
    if (!el) return;
    // Only pin to the bottom if the user is already near it.
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 120;
    if (nearBottom) el.scrollTop = el.scrollHeight;
  }, [messages]);

  /* ---------- send ---------- */

  const send = useCallback(async () => {
    const text = input.trim();
    if (!text || streaming || engine.state !== "ready") return;

    setInput("");
    pending.current = "";
    setMessages((prev) => [
      ...prev,
      { role: "user", content: text },
      { role: "assistant", content: "" },
    ]);
    setCitations([]);
    setStreaming(true);

    try {
      await invoke("send_message", { message: text });
    } catch (e) {
      pending.current = "";
      setStreaming(false);
      setMessages((prev) => {
        const next = [...prev];
        next[next.length - 1] = { role: "assistant", content: `⚠️ ${e}` };
        return next;
      });
    }
  }, [input, streaming, engine.state]);

  const stop = useCallback(async () => {
    try {
      await invoke("cancel_generation");
    } catch {
      /* cancelling a finished stream is not an error worth surfacing */
    }
    setStreaming(false);
  }, []);

  const onKeyDown = (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  };

  // Grow the textarea with its content, up to the CSS max-height.
  const onInput = (e) => {
    setInput(e.target.value);
    const el = taRef.current;
    if (el) {
      el.style.height = "auto";
      el.style.height = `${Math.min(el.scrollHeight, 180)}px`;
    }
  };

  const label = {
    starting: "Starting engine…",
    loading: "Loading model…",
    ready: "Ready",
    error: "Engine error",
  }[engine.state] ?? engine.state;

  const dotClass =
    engine.state === "ready" ? "ready" : engine.state === "error" ? "error" : "loading";

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          ORI<span>O</span>N
        </div>
        <div className="spacer" />
        <button className="ghost small" onClick={() => setShowDocs(true)}>
          Documents
        </button>
        <button className="ghost small" onClick={() => setShowSystem(true)}>
          System
        </button>
        <div className="status" title={engine.detail || label}>
          <span className={`dot ${dotClass}`} />
          {label}
        </div>
      </header>

      <main className="chat" ref={chatRef}>
        {messages.length === 0 ? (
          <div className="empty">
            <h1>Orion</h1>
            <div className="hint">
              Everything runs on this machine. Nothing leaves it.
            </div>
            <div className="hint">
              Press <kbd>Enter</kbd> to send · <kbd>Shift</kbd>+<kbd>Enter</kbd> for a new
              line
            </div>
          </div>
        ) : (
          <div className="chat-inner">
            {messages.map((m, i) => (
              <div key={i} className={`msg ${m.role}`}>
                <div className="role">{m.role === "user" ? "You" : "O"}</div>
                <div className="body">
                  {m.role === "assistant" ? (
                    m.content ? (
                      <Markdown>{m.content}</Markdown>
                    ) : (
                      <span style={{ color: "var(--text-dim)" }}>…</span>
                    )
                  ) : (
                    m.content
                  )}
                </div>
              </div>
            ))}

            {citations.length > 0 && (
              <div className="citations">
                <div className="citations-head">Sources</div>
                <ol>
                  {citations.map((c) => (
                    <li key={c.marker}>
                      <span className="cite-doc">{c.document_name}</span>
                      {c.page ? `, p. ${c.page}` : ""}
                      {c.breadcrumb ? ` — ${c.breadcrumb}` : ""}
                    </li>
                  ))}
                </ol>
              </div>
            )}
          </div>
        )}
      </main>

      <footer className="composer">
        <div className="composer-inner">
          <textarea
            ref={taRef}
            rows={1}
            value={input}
            onChange={onInput}
            onKeyDown={onKeyDown}
            disabled={engine.state !== "ready"}
            placeholder={
              engine.state === "ready"
                ? "Ask Orion anything…"
                : engine.state === "error"
                  ? "Engine unavailable — see status"
                  : "Waiting for the engine…"
            }
          />
          {streaming ? (
            <button className="stop" onClick={stop}>
              Stop
            </button>
          ) : (
            <button onClick={send} disabled={!input.trim() || engine.state !== "ready"}>
              Send
            </button>
          )}
        </div>
        <div className="footnote">
          Orion runs entirely offline · pre-alpha M0
        </div>
      </footer>

      {showSystem && <SystemPanel onClose={() => setShowSystem(false)} />}
      <Documents open={showDocs} onClose={() => setShowDocs(false)} />
    </div>
  );
}
