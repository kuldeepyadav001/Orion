import { useCallback, useEffect, useRef, useState } from "react";
import Markdown from "react-markdown";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import SystemPanel from "./SystemPanel";
import DocumentList from "./DocumentList";
import DropZone from "./DropZone.jsx";
import Logo from "./Logo";
import VoiceButton from "./VoiceButton";

/**
 * Clean markdown, backticks, code blocks, and citations from text before speech synthesis.
 */
function cleanForSpeech(text) {
  if (!text) return "";
  return text
    .replace(/```[\s\S]*?```/g, "") // strip code blocks
    .replace(/`([^`]+)`/g, "$1") // inline code backticks
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1") // links -> title
    .replace(/\[(?:chunk|citation):[^\]]+\]/g, "") // strip citations
    .replace(/^#+\s+/gm, "") // headings
    .replace(/[*_~]/g, "") // formatting
    .replace(/\s+/g, " ")
    .trim();
}

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
  const [hotkey, setHotkey] = useState("");
  const [speaking, setSpeaking] = useState(false);
  const [autoSpeak, setAutoSpeak] = useState(true);

  const audioPlayerRef = useRef(null);
  const lastSourceRef = useRef("text");
  const autoSpeakRef = useRef(autoSpeak);
  useEffect(() => {
    autoSpeakRef.current = autoSpeak;
  }, [autoSpeak]);

  const speakTextRef = useRef(null);

  // The label is computed in Rust so it matches the chord actually
  // registered, and uses the right glyphs for this platform.
  useEffect(() => {
    invoke("hotkey_label")
      .then(setHotkey)
      .catch(() => {});
  }, []);

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

  /* ---------- TTS speech playback ---------- */

  const stopSpeaking = useCallback(() => {
    if (audioPlayerRef.current) {
      audioPlayerRef.current.pause();
      audioPlayerRef.current = null;
    }
    if (typeof window !== "undefined" && "speechSynthesis" in window) {
      window.speechSynthesis.cancel();
    }
    setSpeaking(false);
  }, []);

  const speakText = useCallback(
    async (rawText) => {
      const cleaned = cleanForSpeech(rawText);
      if (!cleaned) return;

      stopSpeaking();

      // 1. Try local Piper neural TTS first
      try {
        const audioUrl = await invoke("voice_speak", { text: cleaned });
        if (audioUrl) {
          const audio = new Audio(audioUrl);
          audioPlayerRef.current = audio;
          setSpeaking(true);

          audio.onended = () => setSpeaking(false);
          audio.onerror = () => setSpeaking(false);
          audio.onpause = () => setSpeaking(false);

          await audio.play();
          return;
        }
      } catch {
        /* Piper binary or model not installed; fallback immediately to Web Speech API */
      }

      // 2. Guaranteed fallback to native SpeechSynthesis (built into WebView2 / browsers)
      if (typeof window !== "undefined" && "speechSynthesis" in window) {
        window.speechSynthesis.cancel();
        const utterance = new SpeechSynthesisUtterance(cleaned);
        utterance.rate = 1.0;
        utterance.pitch = 1.0;

        const voices = window.speechSynthesis.getVoices();
        if (voices.length > 0) {
          const preferred =
            voices.find(
              (v) =>
                v.lang.startsWith("en") &&
                (v.name.includes("Natural") ||
                  v.name.includes("Online") ||
                  v.name.includes("David") ||
                  v.name.includes("Jenny") ||
                  v.name.includes("Google") ||
                  v.name.includes("Samantha")),
            ) || voices.find((v) => v.lang.startsWith("en"));
          if (preferred) utterance.voice = preferred;
        }

        utterance.onstart = () => setSpeaking(true);
        utterance.onend = () => setSpeaking(false);
        utterance.onerror = () => setSpeaking(false);

        window.speechSynthesis.speak(utterance);
      }
    },
    [stopSpeaking],
  );

  useEffect(() => {
    speakTextRef.current = speakText;
  }, [speakText]);

  useEffect(() => {
    let un;
    listen("voice://speak", (e) => {
      const audioUrl = e.payload;
      if (!audioUrl) return;

      if (audioPlayerRef.current) {
        audioPlayerRef.current.pause();
      }
      if (typeof window !== "undefined" && "speechSynthesis" in window) {
        window.speechSynthesis.cancel();
      }

      const audio = new Audio(audioUrl);
      audioPlayerRef.current = audio;
      setSpeaking(true);

      audio.onended = () => setSpeaking(false);
      audio.onerror = () => setSpeaking(false);
      audio.onpause = () => setSpeaking(false);

      audio.play().catch(() => setSpeaking(false));
    }).then((u) => {
      un = u;
    });

    return () => {
      un?.();
      if (audioPlayerRef.current) {
        audioPlayerRef.current.pause();
      }
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
      const fullReply = pending.current;
      pending.current = "";
      setStreaming(false);

      if ((lastSourceRef.current === "voice" || autoSpeakRef.current) && fullReply.trim()) {
        speakTextRef.current?.(fullReply);
      }
    });

    const unlistenErr = listen("chat://error", (e) => {
      pending.current = "";
      setStreaming(false);
      setMessages((prev) => {
        const next = [...prev];
        const last = next[next.length - 1];
        const msg = `⚠️ ${e.payload}`;
        // The same failure can arrive twice: once here from the spawned
        // generation task, and once as the send_message rejection. Showing it
        // twice made one problem look like two, which is how the duplicate
        // 503 was reported.
        if (last?.role === "assistant" && last.content === msg) return prev;
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

  const sendMessage = useCallback(
    async (textToSend, isVoice = false) => {
      const text = (textToSend ?? input).trim();
      // Deliberately NOT gated on engine.state == "ready". The engine starts on the
      // first message, so it is in standby until someone types. We also allow retrying
      // if an error occurred previously.
      if (!text || streaming) return;

      stopSpeaking();
      lastSourceRef.current = isVoice ? "voice" : "text";
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
          const msg = `⚠️ ${e}`;
          const last = next[next.length - 1];
          // chat://error may already have reported this; do not say it twice.
          if (last?.role === "assistant" && last.content === msg) return prev;
          next[next.length - 1] = { role: "assistant", content: msg };
          return next;
        });
      }
    },
    [input, streaming, stopSpeaking],
  );

  const send = useCallback(() => sendMessage(input, false), [sendMessage, input]);

  const stop = useCallback(async () => {
    stopSpeaking();
    try {
      await invoke("cancel_generation");
    } catch {
      /* cancelling a finished stream is not an error worth surfacing */
    }
    setStreaming(false);
  }, [stopSpeaking]);

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
    // "Idle" means the app is open and ready to load on demand, but weights
    // have not been mapped yet. Calling it "Ready" misled users into thinking
    // inference was instantaneous before the cold-load happened.
    idle: "Standby",
    starting: "Starting engine…",
    loading: "Loading model…",
    ready: "Ready",
    error: "Engine error",
  }[engine.state] ?? engine.state;

  // These must match the classes in index.css (.dot.ok / .dot.err / .dot.warn / .dot.idle).
  // "ready" is solid green; "idle" is neutral standby; "starting"/"loading" is pulsing amber;
  // "error" is red.
  const dotClass =
    engine.state === "ready"
      ? "ok"
      : engine.state === "idle"
        ? "idle"
        : engine.state === "error"
          ? "err"
          : "warn";

  // Clears the visible thread. History stays in SQLite; this is a fresh view,
  // not a delete.
  const newChat = () => {
    setMessages([]);
    setCitations([]);
    setGrounded(null);
    setInput("");
  };

  // Stable identity. An inline arrow here is recreated on every render, and
  // DropZone's listener effect depends on it: that caused one dropped file to
  // be indexed roughly 150 times. DropZone now also holds it behind a ref, so
  // this is belt and braces rather than the sole defence.
  const indexDroppedFiles = useCallback(async (paths) => {
    try {
      await invoke("add_documents", { paths });
      setLibrary(await invoke("library_status"));
    } catch (e) {
      console.error("could not index dropped files:", e);
    }
  }, []);

  // When speech recognition produces a transcript, immediately auto-send the turn.
  // This enables hands-free conversational dialogue: you speak, Orion answers aloud!
  const onTranscript = useCallback(
    (text) => {
      const clean = text.trim();
      if (!clean) return;
      sendMessage(clean, true);
    },
    [sendMessage],
  );

  const docCount = library.documents ?? 0;

  return (
    <div className="shell">
      {/* Drag-and-drop overlay. M3 built the intake path; now that M2 has
          landed, accepted files go straight into the library instead of
          being reported and discarded. */}
      <DropZone onAccepted={indexDroppedFiles} />

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
          <div
            className="engine-chip"
            title={
              engine.detail
                ? `${label}: ${engine.detail}`
                : engine.state === "idle"
                  ? "Standby: model loads into memory on your first message"
                  : label
            }
          >
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
              {messages.map((m, i) => {
                const isLastAssistant =
                  m.role === "assistant" && i === messages.length - 1;
                return (
                  <div key={i} className={`msg ${m.role}`}>
                    <div className="msg-role">
                      <span>{m.role === "user" ? "You" : "Orion"}</span>
                      {m.role === "assistant" && m.content && (
                        <button
                          type="button"
                          className={`btn-msg-speak ${isLastAssistant && speaking ? "speaking" : ""}`}
                          onClick={() => {
                            if (isLastAssistant && speaking) {
                              stopSpeaking();
                            } else {
                              speakText(m.content);
                            }
                          }}
                          title={
                            isLastAssistant && speaking
                              ? "Stop speaking"
                              : "Read aloud"
                          }
                          aria-label="Read aloud"
                        >
                          {isLastAssistant && speaking ? "⏹ Stop" : "🔊"}
                        </button>
                      )}
                    </div>
                    <div className="bubble">
                      {m.role === "assistant" ? (
                        <Markdown>{m.content}</Markdown>
                      ) : (
                        m.content
                      )}
                    </div>
                  </div>
                );
              })}

              {streaming && (
                <div className="thinking">
                  <span className="dot-pulse" />
                  <span className="dot-pulse" />
                  <span className="dot-pulse" />
                  {/* A cold start blocks for ~9 s while the model is mapped
                      into memory. Saying nothing during that looks like a
                      hang, which is exactly the complaint this release is
                      fixing elsewhere. */}
                  {(engine.state === "loading" || engine.state === "starting") && (
                    <span className="thinking-note">
                      Loading the model — first message only
                    </span>
                  )}
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
          <VoiceButton
            onTranscript={onTranscript}
            speaking={speaking}
            onStopSpeaking={stopSpeaking}
          />
          <div className="composer-inner">
            <textarea
              ref={taRef}
              rows={1}
              value={input}
              onChange={onInput}
              onKeyDown={onKeyDown}
              disabled={engine.state === "error"}
              placeholder={
                engine.state === "error"
                  ? "Engine unavailable — see System"
                  : engine.state === "loading"
                    ? "Loading the model…"
                    : engine.state === "starting"
                      ? "Starting the engine…"
                      : "Ask anything…"
              }
            />
            {streaming ? (
              <button className="btn-stop" onClick={stop} title="Stop">
                ■
              </button>
            ) : (
              <div className="composer-actions">
                <button
                  type="button"
                  className={`btn-speak-toggle ${autoSpeak ? "active" : ""}`}
                  onClick={() => setAutoSpeak((v) => !v)}
                  title={
                    autoSpeak
                      ? "Spoken replies: ON (click to mute)"
                      : "Spoken replies: OFF (click to unmute)"
                  }
                  aria-label="Toggle spoken audio replies"
                >
                  {autoSpeak ? "🔊" : "🔇"}
                </button>
                <button
                  className="btn-send"
                  onClick={send}
                  disabled={!input.trim() || engine.state === "error"}
                  title="Send"
                >
                  ↑
                </button>
              </div>
            )}
          </div>
          <div className="footnote">
            {docCount > 0
              ? `${docCount} document${docCount === 1 ? "" : "s"} indexed · answers are cited`
              : "Runs entirely offline"}
            {hotkey && <> · press <kbd>{hotkey}</kbd> from anywhere</>}
          </div>
        </footer>
      </div>

      {showSystem && <SystemPanel onClose={() => setShowSystem(false)} />}
    </div>
  );
}
