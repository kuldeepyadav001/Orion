import { useCallback, useEffect, useRef, useState } from "react";
import Markdown from "react-markdown";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import SystemPanel from "./SystemPanel";
import DocumentList from "./DocumentList";
import Logo from "./Logo";

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
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [citations, setCitations] = useState([]);
  // null = no answer yet this turn; false = answered without documents.
  // Distinguishing these matters: "no sources shown" previously looked
  // identical whether the library was consulted and missed, or never
  // consulted at all. The latter was a real bug that shipped unnoticed.
  const [grounded, setGrounded] = useState(null);
  const [library, setLibrary] = useState({ documents: 0, chunks: 0 });

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
    listen("chat://citations", (e) => {
      const cites = e.payload ?? [];
      setCitations(cites);
      setGrounded(cites.length > 0);
    }).then(
      (u) => {
        un = u;
      },
    );
    return () => un?.();
  }, []);

  // Keep the header badge current so the user can see at a glance that the
  // library exists and is being consulted.
  useEffect(() => {
    let libAlive = true;
    const tick = async () => {
      try {
        const st = await invoke("library_status");
        if (libAlive) setLibrary(st);
      } catch {
        /* library not ready yet */
      }
    };
    tick();
    const id = setInterval(tick, 5000);
    return () => {
      libAlive = false;
      clearInterval(id);
    };
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
    setGrounded(null);
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

  // These must match the classes in index.css (.dot.ok / .dot.err / .dot.warn).
  // They previously emitted ready/error/loading, which matched nothing, so the
  // indicator was permanently grey however the engine was doing.
  const dotClass =
    engine.state === "ready" ? "ok" : engine.state === "error" ? "err" : "warn";

  // Clears the visible thread. History stays in SQLite; this is a fresh view,
  // not a delete.
  const newChat = () => {
    setMessages([]);
    setCitations([]);
    setGrounded(null);
    setInput("");
  };

  const docCount = library.documents ?? 0;

  return (
    <div className="shell">
      {/* Sidebar: persistent, so the library is always visible rather than
          hidden behind a modal the user has to remember exists. */}
      <aside className={`sidebar ${sidebarOpen ? "" : "collapsed"}`}>
        <div className="side-head">
          <div className="brand">
            <Logo size={22} />
            <span className="brand-text">Orion</span>
          </div>
          <button
            className="icon-btn"
            onClick={() => setSidebarOpen((v) => !v)}
            title={sidebarOpen ? "Collapse" : "Expand"}
            aria-label="Toggle sidebar"
          >
            {sidebarOpen ? "‹" : "›"}
          </button>
        </div>

        <button
          className="btn-new"
          onClick={newChat}
          title={sidebarOpen ? undefined : "New chat"}
        >
          <span className="btn-new-icon">+</span>
          <span className="btn-new-text">New chat</span>
        </button>

        <div className="side-section">
          <div className="side-label">
            Library
            {docCount > 0 && <span className="count">{docCount}</span>}
          </div>
          <DocumentList onCountChange={(n) => setLibrary((l) => ({ ...l, documents: n }))} />
        </div>

        <div className="side-foot">
          <button className="side-link" onClick={() => setShowSystem(true)}>
            System
          </button>
          <div className="engine-chip" title={engine.detail || label}>
            <span className={`dot ${dotClass}`} />
            <span className="truncate">{label}</span>
          </div>
        </div>
      </aside>

      <div className="main">
        <main className="chat" ref={chatRef}>
          {messages.length === 0 ? (
            <div className="hero">
              <div className="orb" aria-hidden="true">
                <div className="orb-ring" />
                <div className="orb-ring slow" />
                <div className="orb-logo">
                  <Logo size={54} />
                </div>
              </div>
              <h1>How can I help?</h1>
              <p className="hero-sub">
                Everything runs on this machine. Nothing leaves it.
              </p>

              <div className="suggestions">
                <button
                  className="suggestion"
                  onClick={() => setInput("Summarise the document I added")}
                  disabled={docCount === 0}
                >
                  <span className="sg-icon">▤</span>
                  <span className="sg-title">Summarise a document</span>
                  <span className="sg-sub">
                    {docCount > 0
                      ? `${docCount} in your library`
                      : "Add a file to enable"}
                  </span>
                </button>
                <button
                  className="suggestion"
                  onClick={() => setInput("What can you do?")}
                >
                  <span className="sg-icon">✦</span>
                  <span className="sg-title">What can you do?</span>
                  <span className="sg-sub">Capabilities and limits</span>
                </button>
                <button
                  className="suggestion"
                  onClick={() => setInput("Draft a short professional email")}
                >
                  <span className="sg-icon">✎</span>
                  <span className="sg-title">Draft something</span>
                  <span className="sg-sub">Email, notes, an outline</span>
                </button>
              </div>
            </div>
          ) : (
            <div className="thread">
              {messages.map((m, i) => (
                <div key={i} className={`msg ${m.role}`}>
                  <div className="msg-role">
                    {m.role === "user" ? "You" : "Orion"}
                  </div>
                  <div className="bubble">
                    {m.role === "assistant" ? (
                      <Markdown>{m.content}</Markdown>
                    ) : (
                      m.content
                    )}
                  </div>
                </div>
              ))}

              {streaming && (
                <div className="thinking">
                  <span className="dot-pulse" />
                  <span className="dot-pulse" />
                  <span className="dot-pulse" />
                </div>
              )}

              {grounded === false && docCount > 0 && (
                <div className="ungrounded">
                  Answered without your documents — nothing in the library
                  matched this question.
                </div>
              )}

              {citations.length > 0 && (
                <div className="citations">
                  <div className="citations-head">
                    Answered from{" "}
                    {new Set(citations.map((c) => c.document_name)).size}{" "}
                    document
                    {new Set(citations.map((c) => c.document_name)).size === 1
                      ? ""
                      : "s"}
                  </div>
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
                  ? "Ask anything…"
                  : engine.state === "error"
                    ? "Engine unavailable — see System"
                    : "Waiting for the engine…"
              }
            />
            {streaming ? (
              <button className="btn-stop" onClick={stop} title="Stop">
                ■
              </button>
            ) : (
              <button
                className="btn-send"
                onClick={send}
                disabled={!input.trim() || engine.state !== "ready"}
                title="Send"
              >
                ↑
              </button>
            )}
          </div>
          <div className="footnote">
            {docCount > 0
              ? `${docCount} document${docCount === 1 ? "" : "s"} indexed · answers are cited`
              : "Runs entirely offline"}
          </div>
        </footer>
      </div>

      {showSystem && <SystemPanel onClose={() => setShowSystem(false)} />}
    </div>
  );
}
