import { useCallback, useEffect, useRef, useState } from "react";
import Markdown from "react-markdown";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import SystemPanel from "./SystemPanel";
import DocumentList from "./DocumentList";
import DropZone from "./DropZone.jsx";
import Logo from "./Logo";
import VoiceButton from "./VoiceButton";
import OnboardingWizard from "./OnboardingWizard";
import CodeBlock from "./CodeBlock";
import { LockScreen, LockSettingsModal } from "./LockScreen";
import { ChatHistoryList } from "./ChatSidebar";
import DocumentCard from "./DocumentCard";
import {
  extractDocBlock,
  parseMarkdownTable,
  parseMarkdownSections,
  compileDocumentFromSpec,
  buildExcelDocument,
  buildWordDocument,
  buildPdfDocument,
} from "./documentGenerator";

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


const FOLLOWUP_CHIPS = {
  developer: [
    "Show a step-by-step code example",
    "Add error handling & validation",
    "Analyze time and memory complexity",
  ],
  researcher: [
    "Cite specific evidence & excerpts",
    "Identify counterarguments & limitations",
    "Format findings into a comparison table",
  ],
  creative: [
    "Enhance descriptive imagery & atmosphere",
    "Introduce more interpersonal tension",
    "Make the tone more punchy and concise",
  ],
  general: [
    "Explain this more simply with an analogy",
    "Provide 3 actionable next steps",
    "Summarize in concise bullet points",
  ],
};

const markdownComponents = {
  code(props) {
    const { children, className } = props;
    if (className && className.includes("orion-doc")) {
      return null;
    }
    const match = /language-(\w+)/.exec(className || "");
    const isBlock = match || (typeof children === "string" && children.includes("\n"));
    if (isBlock) {
      return (
        <CodeBlock
          language={match ? match[1] : ""}
          code={String(children).replace(/\n$/, "")}
        />
      );
    }
    return (
      <code className="inline-code">
        {children}
      </code>
    );
  },
  pre(props) {
    if (!props.children) return null;
    return <div className="code-pre-wrap">{props.children}</div>;
  },
};

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
  const [showOnboarding, setShowOnboarding] = useState(false);
  const [persona, setPersona] = useState(null);
  const [lockStatus, setLockStatus] = useState({ enabled: false, locked: false, hint: null });
  const [showLockSettings, setShowLockSettings] = useState(false);
  const [copiedIndex, setCopiedIndex] = useState(null);
  const [modelInfo, setModelInfo] = useState(null);
  const [swapping, setSwapping] = useState(null);

  // Permanent Chat History states
  const [sessions, setSessions] = useState([]);
  const [activeSessionId, setActiveSessionId] = useState(null);
  const [activeSessionTitle, setActiveSessionTitle] = useState("New chat");
  const [sidebarTab, setSidebarTab] = useState("chats"); // "chats" | "library"
  const [isEditingTitle, setIsEditingTitle] = useState(false);
  const [editTitleValue, setEditTitleValue] = useState("");
  const [specialistPersona, setSpecialistPersona] = useState({ id: "developer", name: "Developer", icon: "💻" });

  const audioPlayerRef = useRef(null);
  const lastSourceRef = useRef("text");
  const lastUserQueryRef = useRef("");
  const autoSpeakRef = useRef(autoSpeak);
  useEffect(() => {
    autoSpeakRef.current = autoSpeak;
  }, [autoSpeak]);

  useEffect(() => {
    invoke("list_personas")
      .then((list) => {
        const nonGen = list?.find((p) => p.id !== "general");
        if (nonGen) setSpecialistPersona(nonGen);
      })
      .catch(() => {});
  }, []);

  const handleToggleMode = useCallback(async (targetPersonaId) => {
    try {
      await invoke("set_active_persona", { personaId: targetPersonaId });
      const updated = await invoke("get_active_persona");
      setPersona(updated);
    } catch (e) {
      console.error("Failed to switch mode:", e);
    }
  }, []);

  const processDocumentOutput = useCallback(async (rawReply, lastUserQuery) => {
    if (!rawReply) return;

    // 1. Explicit orion-doc block emitted by LLM
    const extracted = extractDocBlock(rawReply);
    if (extracted) {
      try {
        const doc = await compileDocumentFromSpec(extracted.type, extracted.spec);
        const savedDoc = await invoke("save_generated_document", {
          filename: doc.filename,
          dataBase64: doc.base64,
          fileType: doc.fileType,
        });

        const cleanedText = rawReply.replace(extracted.rawBlock, "").trim();

        setMessages((prev) => {
          const next = [...prev];
          const last = next[next.length - 1];
          if (last && last.role === "assistant") {
            last.content = cleanedText || `I've prepared your document: **${doc.filename}**.`;
            last.document = {
              ...savedDoc,
              base64: doc.base64,
            };
          }
          return next;
        });
        return;
      } catch (err) {
        console.error("Failed to compile orion-doc block:", err);
      }
    }

    // 2. Conversational Intent fallback: User asked for Excel / Word / PDF
    const query = (lastUserQuery || "").toLowerCase();
    const isExcelReq =
      query.includes("excel") ||
      query.includes("spreadsheet") ||
      query.includes(".xlsx") ||
      query.includes("sheet");
    const isWordReq =
      query.includes("word doc") ||
      query.includes(".docx") ||
      query.includes("word document") ||
      query.includes("proposal");
    const isPdfReq =
      query.includes("pdf") ||
      query.includes("pdf report") ||
      query.includes("export pdf");

    if (isExcelReq || isWordReq || isPdfReq) {
      try {
        if (isExcelReq) {
          const table = parseMarkdownTable(rawReply);
          if (table && table.rows.length > 0) {
            const doc = await buildExcelDocument({
              filename: "Orion_Spreadsheet.xlsx",
              title: "Orion Spreadsheet Export",
              columns: table.columns,
              rows: table.rows,
              showTotals: true,
            });
            const savedDoc = await invoke("save_generated_document", {
              filename: doc.filename,
              dataBase64: doc.base64,
              fileType: doc.fileType,
            });
            setMessages((prev) => {
              const next = [...prev];
              const last = next[next.length - 1];
              if (last && last.role === "assistant") {
                last.document = { ...savedDoc, base64: doc.base64 };
              }
              return next;
            });
          }
        } else if (isWordReq) {
          const sections = parseMarkdownSections(rawReply);
          if (sections.length > 0) {
            const doc = await buildWordDocument({
              filename: "Orion_Document.docx",
              title: sections[0]?.heading || "Orion Document",
              sections,
            });
            const savedDoc = await invoke("save_generated_document", {
              filename: doc.filename,
              dataBase64: doc.base64,
              fileType: doc.fileType,
            });
            setMessages((prev) => {
              const next = [...prev];
              const last = next[next.length - 1];
              if (last && last.role === "assistant") {
                last.document = { ...savedDoc, base64: doc.base64 };
              }
              return next;
            });
          }
        } else if (isPdfReq) {
          const sections = parseMarkdownSections(rawReply);
          const table = parseMarkdownTable(rawReply);
          if (table && sections.length > 0) {
            sections[0].table = {
              headers: table.columns.map((c) => c.header),
              rows: table.rows,
            };
          }
          if (sections.length > 0) {
            const doc = await buildPdfDocument({
              filename: "Orion_Report.pdf",
              title: sections[0]?.heading || "Orion Report",
              sections,
            });
            const savedDoc = await invoke("save_generated_document", {
              filename: doc.filename,
              dataBase64: doc.base64,
              fileType: doc.fileType,
            });
            setMessages((prev) => {
              const next = [...prev];
              const last = next[next.length - 1];
              if (last && last.role === "assistant") {
                last.document = { ...savedDoc, base64: doc.base64 };
              }
              return next;
            });
          }
        }
      } catch (e) {
        console.warn("Conversational document generation fallback error:", e);
      }
    }
  }, []);
  useEffect(() => {
    autoSpeakRef.current = autoSpeak;
  }, [autoSpeak]);

  const speakTextRef = useRef(null);

  const refreshSessions = useCallback(async () => {
    try {
      const list = await invoke("list_sessions");
      setSessions(list || []);
      const activeId = await invoke("get_active_session_id");
      if (activeId) {
        setActiveSessionId(activeId);
        const current = (list || []).find((s) => s.id === activeId);
        if (current) {
          setActiveSessionTitle(current.title || "New chat");
        }
      }
    } catch (err) {
      console.error("Failed to load sessions:", err);
    }
  }, []);

  const loadInitialChat = useCallback(async () => {
    try {
      const list = await invoke("list_sessions");
      setSessions(list || []);
      const activeId = await invoke("get_active_session_id");
      if (activeId) {
        setActiveSessionId(activeId);
        const current = (list || []).find((s) => s.id === activeId);
        if (current) {
          setActiveSessionTitle(current.title || "New chat");
        }
        // Storage-First: load only the most recent 50 messages from SQLite
        const storedMsgs = await invoke("get_session_messages", {
          sessionId: activeId,
          limit: 50,
        });
        if (storedMsgs && storedMsgs.length > 0) {
          setMessages(
            storedMsgs.map((m) => ({
              role: m.role,
              content: m.content,
              id: m.id,
              created_at: m.created_at,
            }))
          );
        }
      }
    } catch (err) {
      console.error("Failed to load initial chat history:", err);
    }
  }, []);

  const handleSelectSession = useCallback(
    async (sessionId) => {
      if (streaming || sessionId === activeSessionId) return;
      try {
        const storedMsgs = await invoke("switch_session", { sessionId });
        setActiveSessionId(sessionId);
        const s = sessions.find((x) => x.id === sessionId);
        if (s) setActiveSessionTitle(s.title || "New chat");
        setMessages(
          (storedMsgs || []).map((m) => ({
            role: m.role,
            content: m.content,
            id: m.id,
            created_at: m.created_at,
          }))
        );
        setCitations([]);
        setGrounded(null);
        setInput("");
        refreshSessions();
      } catch (err) {
        console.error("Failed to switch session:", err);
      }
    },
    [streaming, activeSessionId, sessions, refreshSessions]
  );

  const handleCreateSession = useCallback(async () => {
    if (streaming) return;
    try {
      const newSession = await invoke("create_new_session", { title: "New chat" });
      setActiveSessionId(newSession.id);
      setActiveSessionTitle(newSession.title);
      setMessages([]);
      setCitations([]);
      setGrounded(null);
      setInput("");
      refreshSessions();
    } catch (err) {
      console.error("Failed to create new session:", err);
    }
  }, [streaming, refreshSessions]);

  const handleRenameSession = useCallback(
    async (sessionId, newTitle) => {
      if (!newTitle || !newTitle.trim()) return;
      try {
        await invoke("rename_session", { sessionId, title: newTitle.trim() });
        setSessions((prev) =>
          prev.map((s) => (s.id === sessionId ? { ...s, title: newTitle.trim() } : s))
        );
        if (sessionId === activeSessionId) {
          setActiveSessionTitle(newTitle.trim());
        }
      } catch (err) {
        console.error("Failed to rename session:", err);
      }
    },
    [activeSessionId]
  );

  const handleDeleteSession = useCallback(async (sessionId) => {
    try {
      const res = await invoke("delete_session", { sessionId });
      setSessions(res.sessions || []);
      setActiveSessionId(res.active_session_id);
      const current = (res.sessions || []).find((s) => s.id === res.active_session_id);
      setActiveSessionTitle(current?.title || "New chat");
      setMessages(
        (res.messages || []).map((m) => ({
          role: m.role,
          content: m.content,
          id: m.id,
          created_at: m.created_at,
        }))
      );
    } catch (err) {
      console.error("Failed to delete session:", err);
    }
  }, []);

  const refreshModelInfo = useCallback(async () => {
    try {
      const info = await invoke("active_model_info");
      setModelInfo(info);
    } catch {
      /* ignore */
    }
  }, []);

  useEffect(() => {
    let unlistenSwapping;
    let unlistenSwapped;

    listen("engine://swapping", (e) => {
      setSwapping(e.payload);
    }).then((un) => {
      unlistenSwapping = un;
    });

    listen("engine://swapped", () => {
      setSwapping(null);
      refreshModelInfo();
    }).then((un) => {
      unlistenSwapped = un;
    });

    return () => {
      unlistenSwapping?.();
      unlistenSwapped?.();
    };
  }, [refreshModelInfo]);

  const refreshLockStatus = useCallback(async () => {
    try {
      const status = await invoke("lock_status");
      setLockStatus(status);
    } catch (e) {
      console.error("Failed to load lock status:", e);
    }
  }, []);

  const handleLockNow = async () => {
    try {
      await invoke("lock_app_now");
      refreshLockStatus();
    } catch (e) {
      console.error("Failed to lock app:", e);
    }
  };

  const copyMessage = useCallback(async (content, index) => {
    try {
      await navigator.clipboard.writeText(content);
      setCopiedIndex(index);
      setTimeout(() => setCopiedIndex(null), 2000);
    } catch (err) {
      console.error("Failed to copy message:", err);
    }
  }, []);

  // The label is computed in Rust so it matches the chord actually
  // registered, and uses the right glyphs for this platform.
  useEffect(() => {
    invoke("hotkey_label")
      .then(setHotkey)
      .catch(() => {});

    invoke("get_active_persona")
      .then(async (actPersona) => {
        setPersona(actPersona);
        const personaId = actPersona?.id || "general";
        try {
          const res = await invoke("check_resource_status", { persona: personaId });
          const onboardingCompleted = await invoke("get_onboarding_status");
          // If models are missing from disk OR onboarding hasn't been done, show setup wizard:
          if (!onboardingCompleted || !res.all_ready) {
            setShowOnboarding(true);
          }
        } catch {
          /* fallback */
        }
      })
      .catch(() => {});

    refreshLockStatus();
    refreshModelInfo();
    loadInitialChat();

    const handleOpenDownloader = () => setShowOnboarding(true);
    window.addEventListener("open-resource-downloader", handleOpenDownloader);
    return () => window.removeEventListener("open-resource-downloader", handleOpenDownloader);
  }, [refreshLockStatus, refreshModelInfo, loadInitialChat]);

  // Listen for automatic titling from first turn
  useEffect(() => {
    let unlistenTitled;
    listen("session://titled", (e) => {
      const { session_id, title } = e.payload || {};
      if (session_id) {
        setSessions((prev) =>
          prev.map((s) => (s.id === session_id ? { ...s, title } : s))
        );
        if (session_id === activeSessionId) {
          setActiveSessionTitle(title);
        }
      }
    }).then((un) => {
      unlistenTitled = un;
    });
    return () => unlistenTitled?.();
  }, [activeSessionId]);

  // Global Keyboard Shortcuts (Ctrl+N for new chat, Ctrl+B for sidebar toggle)
  useEffect(() => {
    const handleKeyDown = (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "n") {
        e.preventDefault();
        handleCreateSession();
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "b") {
        e.preventDefault();
        setSidebarOpen((v) => !v);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleCreateSession]);

  useEffect(() => {
    refreshModelInfo();
  }, [persona, refreshModelInfo]);

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

    const unlistenDone = listen("chat://done", async () => {
      const fullReply = pending.current;
      pending.current = "";
      setStreaming(false);
      refreshSessions();

      if (fullReply) {
        await processDocumentOutput(fullReply, lastUserQueryRef.current);
      }

      if ((lastSourceRef.current === "voice" || autoSpeakRef.current) && fullReply.trim()) {
        speakTextRef.current?.(cleanForSpeech(fullReply));
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
      lastUserQueryRef.current = text;
      setInput("");
      pending.current = "";
      setMessages((prev) => [
        ...prev,
        { role: "user", content: text },
        {
          role: "assistant",
          content: "",
          persona: persona,
          modelName: modelInfo?.file_name,
          isSpecialized: modelInfo?.is_specialized,
        },
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
    [input, streaming, stopSpeaking, persona, modelInfo],
  );

  const send = useCallback(() => sendMessage(input, false), [sendMessage, input]);

  const regenerateLast = useCallback(() => {
    if (streaming || messages.length === 0) return;
    let lastUserText = "";
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i].role === "user") {
        lastUserText = messages[i].content;
        break;
      }
    }
    if (!lastUserText) return;

    setMessages((prev) => {
      const next = [...prev];
      if (next[next.length - 1]?.role === "assistant") {
        next.pop();
      }
      return next;
    });
    sendMessage(lastUserText, false);
  }, [messages, streaming, sendMessage]);

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
          onClick={handleCreateSession}
          title={sidebarOpen ? "New chat (Ctrl+N)" : "New chat"}
        >
          <span className="btn-new-icon">+</span>
          <span className="btn-new-text">New chat</span>
          {sidebarOpen && <kbd className="btn-new-kbd">Ctrl+N</kbd>}
        </button>

        {sidebarOpen && (
          <div className="sidebar-tab-switcher">
            <button
              type="button"
              className={`sidebar-tab-btn ${sidebarTab === "chats" ? "active" : ""}`}
              onClick={() => setSidebarTab("chats")}
            >
              💬 Chats {sessions.length > 0 && <span className="tab-pill">{sessions.length}</span>}
            </button>
            <button
              type="button"
              className={`sidebar-tab-btn ${sidebarTab === "library" ? "active" : ""}`}
              onClick={() => setSidebarTab("library")}
            >
              📚 Library {docCount > 0 && <span className="tab-pill">{docCount}</span>}
            </button>
          </div>
        )}

        {sidebarOpen ? (
          sidebarTab === "chats" ? (
            <div className="side-section side-chats-section">
              <ChatHistoryList
                sessions={sessions}
                activeSessionId={activeSessionId}
                onSelectSession={handleSelectSession}
                onCreateSession={handleCreateSession}
                onDeleteSession={handleDeleteSession}
                onRenameSession={handleRenameSession}
              />
            </div>
          ) : (
            <div className="side-section">
              <div className="side-label">
                Library
                {docCount > 0 && <span className="count">{docCount}</span>}
              </div>
              <DocumentList onCountChange={(n) => setLibrary((l) => ({ ...l, documents: n }))} />
            </div>
          )
        ) : null}

        <div className="side-foot">
          {persona && (
            <button
              className="persona-pill"
              onClick={() => setShowSystem(true)}
              title="Active workload persona — click to change"
            >
              <span className="persona-pill-icon">{persona.icon}</span>
              <span className="persona-pill-text truncate">{persona.name}</span>
            </button>
          )}
          <div className="side-foot-row">
            <button className="side-link" onClick={() => setShowSystem(true)}>
              System
            </button>
            <button
              className="side-link"
              onClick={() => setShowLockSettings(true)}
              title={lockStatus?.enabled ? "Master Passcode Configured" : "Enable Master Lock"}
            >
              {lockStatus?.enabled ? "🔒 Lock" : "🛡 Lock"}
            </button>
            {lockStatus?.enabled && (
              <button
                className="btn-quick-lock"
                onClick={handleLockNow}
                title="Lock Orion immediately"
              >
                🔒
              </button>
            )}
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
        </div>
      </aside>

      <div className="main">
        <header className="chat-top-header">
          <div className="header-left-group">
            {!sidebarOpen && (
              <button
                type="button"
                className="btn-toggle-sidebar"
                onClick={() => setSidebarOpen(true)}
                title="Open conversations sidebar (Ctrl+B)"
                aria-label="Toggle sidebar"
              >
                💬 Chats
              </button>
            )}

            <div className="header-session-info">
              {isEditingTitle ? (
                <div className="header-rename-box">
                  <input
                    type="text"
                    className="header-rename-input"
                    value={editTitleValue}
                    onChange={(e) => setEditTitleValue(e.target.value)}
                    onBlur={() => {
                      handleRenameSession(activeSessionId, editTitleValue);
                      setIsEditingTitle(false);
                    }}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        handleRenameSession(activeSessionId, editTitleValue);
                        setIsEditingTitle(false);
                      } else if (e.key === "Escape") {
                        setIsEditingTitle(false);
                      }
                    }}
                    autoFocus
                    maxLength={60}
                  />
                  <button
                    type="button"
                    className="header-rename-save-btn"
                    onClick={() => {
                      handleRenameSession(activeSessionId, editTitleValue);
                      setIsEditingTitle(false);
                    }}
                  >
                    ✓
                  </button>
                </div>
              ) : (
                <div
                  className="header-title-display"
                  onClick={() => {
                    setEditTitleValue(activeSessionTitle);
                    setIsEditingTitle(true);
                  }}
                  title="Click to rename this conversation"
                >
                  <span className="header-chat-title">{activeSessionTitle || "New chat"}</span>
                  <button
                    type="button"
                    className="header-title-edit-icon"
                    title="Rename conversation"
                    onClick={(e) => {
                      e.stopPropagation();
                      setEditTitleValue(activeSessionTitle);
                      setIsEditingTitle(true);
                    }}
                  >
                    ✏️
                  </button>
                </div>
              )}
            </div>
          </div>

          {/* Clean 2-Mode Segmented Control */}
          <div className="mode-segmented-control" role="group" aria-label="Engine Mode">
            <button
              type="button"
              className={`mode-seg-btn ${persona?.id === "general" ? "active" : ""}`}
              onClick={() => handleToggleMode("general")}
              title="General Assistant model"
            >
              ⚡ General
            </button>
            <button
              type="button"
              className={`mode-seg-btn ${persona?.id !== "general" ? "active" : ""}`}
              onClick={() => handleToggleMode(specialistPersona?.id || "developer")}
              title={`Specialist: ${specialistPersona?.name || "Developer"}`}
            >
              {specialistPersona?.icon || "💻"} {specialistPersona?.name || "Developer"}
            </button>
          </div>

          <div className="header-actions">
            <button
              type="button"
              className="btn-quick-new-chat"
              onClick={handleCreateSession}
              title="New conversation (Ctrl+N)"
            >
              + New Chat
            </button>
            <button
              type="button"
              className="btn-header-settings"
              onClick={() => setShowSystem(true)}
              title="System & Hardware Settings"
              aria-label="Settings"
            >
              ⚙️
            </button>
          </div>
        </header>

        <main className="chat" ref={chatRef}>
          {messages.length === 0 ? (
            <div className="hero">
              <div className="hero-brand-mark">
                <Logo size={46} />
              </div>
              <h1>How can I help you?</h1>
              <p className="hero-sub">
                100% offline • Generates Excel, Word &amp; PDF • Single-resident RAM
              </p>

              <div className="suggestions">
                <button
                  type="button"
                  className="suggestion"
                  onClick={() => setInput("Create an annual budget forecast spreadsheet with revenue projections, operational costs, and profit calculations in Excel (.xlsx).")}
                >
                  <span className="sg-icon">📊</span>
                  <div className="sg-text">
                    <span className="sg-title">Budget Model in Excel</span>
                    <span className="sg-sub">Dynamic formulas, P&amp;L, auto-styles</span>
                  </div>
                </button>

                <button
                  type="button"
                  className="suggestion"
                  onClick={() => setInput("Draft a comprehensive project architecture proposal in a Word document (.docx) with executive summary and comparison tables.")}
                >
                  <span className="sg-icon">📄</span>
                  <div className="sg-text">
                    <span className="sg-title">Architecture Spec in Word</span>
                    <span className="sg-sub">Formatted headings, tables &amp; callouts</span>
                  </div>
                </button>

                <button
                  type="button"
                  className="suggestion"
                  onClick={() => setInput("Generate an executive compliance audit report in PDF (.pdf) evaluating zero-egress data residency.")}
                >
                  <span className="sg-icon">📑</span>
                  <div className="sg-text">
                    <span className="sg-title">Compliance Audit in PDF</span>
                    <span className="sg-sub">Vector styling, metrics &amp; sign-off</span>
                  </div>
                </button>

                <button
                  type="button"
                  className="suggestion"
                  onClick={() => setInput("Review and refactor this code snippet for optimal O(n) runtime performance and low memory allocations:")}
                >
                  <span className="sg-icon">💻</span>
                  <div className="sg-text">
                    <span className="sg-title">Refactor Code Performance</span>
                    <span className="sg-sub">Memory safety &amp; O(n) efficiency</span>
                  </div>
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
                      {m.role === "user" ? (
                        <span className="msg-user-title">You</span>
                      ) : (
                        <div className="msg-orion-meta">
                          <span className="orion-brand-name">Orion</span>
                          {m.isSpecialized && (
                            <span className="msg-specialist-indicator" title="Specialist Model Active">
                              {m.persona?.icon || "💻"} {m.persona?.name || "Specialist"}
                            </span>
                          )}
                        </div>
                      )}
                      {m.role === "assistant" && m.content && (
                        <div className="msg-actions">
                          <button
                            type="button"
                            className="btn-msg-action"
                            onClick={() => copyMessage(m.content, i)}
                            title="Copy response"
                            aria-label="Copy response"
                          >
                            {copiedIndex === i ? "✓ Copied" : "📋 Copy"}
                          </button>
                          <button
                            type="button"
                            className={`btn-msg-action btn-msg-speak ${isLastAssistant && speaking ? "speaking" : ""}`}
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
                            {isLastAssistant && speaking ? "⏹ Stop" : "🔊 Speak"}
                          </button>
                          {isLastAssistant && !streaming && (
                            <button
                              type="button"
                              className="btn-msg-action btn-msg-retry"
                              onClick={regenerateLast}
                              title="Regenerate this response"
                              aria-label="Regenerate response"
                            >
                              🔄 Retry
                            </button>
                          )}
                        </div>
                      )}
                    </div>
                    <div className="bubble">
                      {m.role === "assistant" ? (
                        <>
                          <Markdown components={markdownComponents}>{m.content}</Markdown>
                          {streaming && isLastAssistant && <span className="streaming-cursor" />}
                          {m.document && <DocumentCard doc={m.document} />}
                        </>
                      ) : (
                        <>
                          {m.content}
                          {m.document && <DocumentCard doc={m.document} />}
                        </>
                      )}
                    </div>
                  </div>
                );
              })}

              {swapping && (
                <div className="swapping-pill">
                  <span className="swap-spinner">🔄</span>
                  <span>Sequential Memory Handoff: Swapping to <strong>{swapping.to}</strong> model in RAM…</span>
                </div>
              )}

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

        {messages.length > 0 && !streaming && (
          <div className="followup-chips">
            {(FOLLOWUP_CHIPS[persona?.id] || FOLLOWUP_CHIPS.general).map((chip, idx) => (
              <button
                key={idx}
                type="button"
                className="followup-chip"
                onClick={() => sendMessage(chip, false)}
              >
                <span className="chip-spark">✦</span> {chip}
              </button>
            ))}
          </div>
        )}

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
                      : "Ask anything, or ask Orion to generate an Excel, Word, or PDF document…"
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

      {showSystem && (
        <SystemPanel
          onClose={() => setShowSystem(false)}
          onPersonaChanged={(newP) => setPersona(newP)}
        />
      )}

      {showOnboarding && (
        <OnboardingWizard
          onComplete={() => {
            setShowOnboarding(false);
            invoke("get_active_persona").then(setPersona).catch(() => {});
          }}
        />
      )}

      {lockStatus?.locked && (
        <LockScreen
          lockStatus={lockStatus}
          onUnlocked={refreshLockStatus}
        />
      )}

      {showLockSettings && (
        <LockSettingsModal
          lockStatus={lockStatus}
          onClose={() => setShowLockSettings(false)}
          onStatusChanged={refreshLockStatus}
        />
      )}
    </div>
  );
}
