import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/**
 * JARVIS-class Voice Button & Continuous Conversational Audio Loop.
 *
 * Dual-Engine Architecture:
 * 1. Primary Engine: Embedded Whisper.cpp via Tauri IPC (`voice_start` / `voice_stop`).
 * 2. Guaranteed Fallback: Native Web Speech API (`SpeechRecognition` / `webkitSpeechRecognition`)
 *    with real-time Web Audio RMS level metering, ensuring zero-setup voice works
 *    out-of-the-box on day 1 even before binary models are fetched.
 *
 * Continuous JARVIS Loop:
 * - Hands-free auto-rearm: When Orion finishes speaking, listening resumes automatically.
 * - Echo prevention: Microphone pauses while Orion is speaking to eliminate speaker feedback.
 */
export default function VoiceButton({ onTranscript, speaking, onStopSpeaking }) {
  const [status, setStatus] = useState({
    state: "off",
    mic_open: false,
    hearing: false,
    level: 0,
    speech_ms: 0,
    detail: "Microphone off",
    available: false,
    engine: "auto", // "whisper" | "webspeech"
  });

  const [activeEngine, setActiveEngine] = useState("auto"); // "whisper" | "webspeech" | "none"
  const [error, setError] = useState(null);

  // References to preserve callbacks across renders
  const onTranscriptRef = useRef(onTranscript);
  useEffect(() => {
    onTranscriptRef.current = onTranscript;
  }, [onTranscript]);

  const speakingRef = useRef(speaking);
  useEffect(() => {
    speakingRef.current = speaking;
  }, [speaking]);

  const handsFreeRef = useRef(false);
  const recognitionRef = useRef(null);
  const audioContextRef = useRef(null);
  const mediaStreamRef = useRef(null);
  const meterIntervalRef = useRef(null);
  const restartTimeoutRef = useRef(null);

  /* ---------- Check capabilities ---------- */

  const isWebSpeechAvailable =
    typeof window !== "undefined" &&
    Boolean(window.SpeechRecognition || window.webkitSpeechRecognition);

  /* ---------- Tauri Whisper events ---------- */

  useEffect(() => {
    let cancelled = false;
    let unlisteners = [];

    const subscribe = async () => {
      try {
        const handles = await Promise.all([
          listen("voice://transcript", (e) => {
            const text = (e.payload ?? "").trim();
            if (text && !speakingRef.current) {
              onTranscriptRef.current?.(text);
            }
          }),
          listen("voice://error", (e) => {
            console.warn("Tauri voice error:", e.payload);
            setError(String(e.payload));
          }),
        ]);
        if (cancelled) {
          handles.forEach((u) => u());
          return;
        }
        unlisteners = handles;
      } catch {
        /* Not in Tauri runtime */
      }
    };
    subscribe();

    return () => {
      cancelled = true;
      unlisteners.forEach((u) => u());
    };
  }, []);

  /* ---------- Tauri Whisper status polling ---------- */

  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const st = await invoke("voice_status");
        if (alive) {
          setStatus((prev) => {
            // Keep Web Speech state if Web Speech is actively running
            if (activeEngine === "webspeech") return prev;
            return {
              ...st,
              engine: "whisper",
            };
          });
        }
      } catch {
        /* Backend not ready or outside Tauri */
      }
    };

    tick();
    const id = setInterval(tick, status.mic_open && activeEngine === "whisper" ? 120 : 1500);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, [status.mic_open, activeEngine]);

  /* ---------- Web Speech API Fallback Engine ---------- */

  const stopWebSpeech = useCallback(() => {
    if (meterIntervalRef.current) {
      clearInterval(meterIntervalRef.current);
      meterIntervalRef.current = null;
    }
    if (mediaStreamRef.current) {
      mediaStreamRef.current.getTracks().forEach((t) => t.stop());
      mediaStreamRef.current = null;
    }
    if (audioContextRef.current) {
      try {
        audioContextRef.current.close();
      } catch {
        /* ignore */
      }
      audioContextRef.current = null;
    }
    if (recognitionRef.current) {
      try {
        recognitionRef.current.onresult = null;
        recognitionRef.current.onerror = null;
        recognitionRef.current.onend = null;
        recognitionRef.current.abort();
      } catch {
        /* ignore */
      }
      recognitionRef.current = null;
    }
  }, []);

  const startWebSpeech = useCallback(() => {
    if (!isWebSpeechAvailable) {
      setError("Web Speech API is not supported in this environment.");
      return;
    }

    stopWebSpeech();
    setError(null);

    const SpeechRec = window.SpeechRecognition || window.webkitSpeechRecognition;
    const recognition = new SpeechRec();
    recognition.continuous = true;
    recognition.interimResults = true;
    recognition.lang = "en-US";

    recognition.onresult = (event) => {
      if (speakingRef.current) return; // Mute recognition while Orion is speaking

      let interim = "";
      let finalSpeech = "";

      for (let i = event.resultIndex; i < event.results.length; ++i) {
        if (event.results[i].isFinal) {
          finalSpeech += event.results[i][0].transcript;
        } else {
          interim += event.results[i][0].transcript;
        }
      }

      const hasSound = Boolean(interim.trim() || finalSpeech.trim());
      setStatus((prev) => ({
        ...prev,
        hearing: hasSound,
        detail: hasSound ? "Hearing you…" : "Listening — speak to chat",
      }));

      if (finalSpeech.trim()) {
        const clean = finalSpeech.trim();
        setStatus((prev) => ({
          ...prev,
          state: "transcribing",
          detail: "Transcribing…",
        }));
        onTranscriptRef.current?.(clean);
      }
    };

    recognition.onerror = (e) => {
      if (e.error !== "no-speech") {
        console.warn("Web Speech error:", e.error);
      }
    };

    recognition.onend = () => {
      // If hands-free is active and Orion is not speaking, automatically re-arm
      if (handsFreeRef.current && !speakingRef.current) {
        try {
          recognition.start();
        } catch {
          /* already started */
        }
      }
    };

    try {
      recognition.start();
      recognitionRef.current = recognition;
      setActiveEngine("webspeech");
      setStatus({
        state: "waiting",
        mic_open: true,
        hearing: false,
        level: 0.1,
        speech_ms: 0,
        detail: "Listening (Web Speech) — speak to chat",
        available: true,
        engine: "webspeech",
      });
    } catch (e) {
      setError(`Could not start speech recognition: ${e}`);
      return;
    }

    // Set up Web Audio Analyser for realistic live meter bars
    if (navigator.mediaDevices?.getUserMedia) {
      navigator.mediaDevices
        .getUserMedia({ audio: true })
        .then((stream) => {
          mediaStreamRef.current = stream;
          const AudioContextClass = window.AudioContext || window.webkitAudioContext;
          if (!AudioContextClass) return;

          const ctx = new AudioContextClass();
          audioContextRef.current = ctx;
          const analyser = ctx.createAnalyser();
          analyser.fftSize = 128;
          const source = ctx.createMediaStreamSource(stream);
          source.connect(analyser);

          const dataArray = new Uint8Array(analyser.frequencyBinCount);
          meterIntervalRef.current = setInterval(() => {
            if (speakingRef.current) {
              setStatus((prev) => ({ ...prev, level: 0.05, hearing: false }));
              return;
            }

            analyser.getByteFrequencyData(dataArray);
            let sum = 0;
            for (let i = 0; i < dataArray.length; i++) {
              sum += dataArray[i];
            }
            const avg = sum / dataArray.length;
            const norm = Math.min(1.0, avg / 100);

            setStatus((prev) => {
              if (!prev.mic_open) return prev;
              const hearing = norm > 0.12;
              return {
                ...prev,
                level: norm,
                hearing,
                detail: hearing ? "Hearing you…" : "Listening (Web Speech) — speak to chat",
              };
            });
          }, 100);
        })
        .catch(() => {
          /* mic stream failed; speech recognition will still work without audio meter */
        });
    }
  }, [isWebSpeechAvailable, stopWebSpeech]);

  /* ---------- Hands-free Echo Cancellation & Auto-Rearm ---------- */

  useEffect(() => {
    if (speaking) {
      // Pause or mute while Orion is speaking to prevent self-transcription loop
      if (activeEngine === "webspeech" && recognitionRef.current) {
        try {
          recognitionRef.current.abort();
        } catch {
          /* ignore */
        }
      }
    } else {
      // When Orion finishes speaking, re-arm listening if hands-free was active
      if (handsFreeRef.current) {
        if (restartTimeoutRef.current) clearTimeout(restartTimeoutRef.current);
        restartTimeoutRef.current = setTimeout(() => {
          if (!speakingRef.current && handsFreeRef.current) {
            if (activeEngine === "webspeech") {
              startWebSpeech();
            } else if (activeEngine === "whisper") {
              invoke("voice_start").catch(() => startWebSpeech());
            }
          }
        }, 400);
      }
    }

    return () => {
      if (restartTimeoutRef.current) clearTimeout(restartTimeoutRef.current);
    };
  }, [speaking, activeEngine, startWebSpeech]);

  /* ---------- Toggle Microphone (User Action) ---------- */

  const toggle = useCallback(async () => {
    if (speaking) {
      onStopSpeaking?.();
      return;
    }

    setError(null);

    // If microphone is currently open, turn it off completely
    if (status.mic_open) {
      handsFreeRef.current = false;
      if (activeEngine === "webspeech") {
        stopWebSpeech();
        setActiveEngine("none");
        setStatus({
          state: "off",
          mic_open: false,
          hearing: false,
          level: 0,
          speech_ms: 0,
          detail: "Microphone off",
          available: isWebSpeechAvailable || status.available,
          engine: "none",
        });
      } else {
        try {
          const st = await invoke("voice_stop");
          setStatus(st);
          setActiveEngine("none");
        } catch {
          stopWebSpeech();
        }
      }
      return;
    }

    // Turn microphone on: Try local Whisper first, then Web Speech fallback
    handsFreeRef.current = true;

    try {
      // Check if Whisper is available
      const check = await invoke("voice_status");
      if (check.available) {
        const st = await invoke("voice_start");
        setStatus(st);
        setActiveEngine("whisper");
        return;
      }
    } catch {
      /* Not available or failed */
    }

    // Fallback directly to Web Speech
    if (isWebSpeechAvailable) {
      startWebSpeech();
    } else {
      setError("Speech recognition is not installed. Run scripts/fetch-voice.sh");
    }
  }, [
    speaking,
    status.mic_open,
    status.available,
    activeEngine,
    isWebSpeechAvailable,
    onStopSpeaking,
    stopWebSpeech,
    startWebSpeech,
  ]);

  const talk = useCallback(async () => {
    setError(null);
    if (activeEngine === "whisper") {
      try {
        setStatus(await invoke("voice_trigger"));
      } catch (e) {
        setError(String(e));
      }
    }
  }, [activeEngine]);

  /* ---------- Cleanup on unmount ---------- */

  useEffect(() => {
    return () => {
      stopWebSpeech();
      if (restartTimeoutRef.current) clearTimeout(restartTimeoutRef.current);
    };
  }, [stopWebSpeech]);

  /* ---------- Visual Presentation ---------- */

  const transcribing = status.state === "transcribing";
  const recording = status.state === "recording";
  const hearing = status.hearing;

  let tone = "off";
  if (speaking) tone = "speaking";
  else if (transcribing) tone = "busy";
  else if (recording) tone = "recording";
  else if (hearing) tone = "hearing";
  else if (status.mic_open) tone = "open";

  const label = speaking
    ? "Speaking"
    : transcribing
    ? "Transcribing…"
    : recording || hearing
    ? "Hearing you…"
    : status.mic_open
    ? "Listening"
    : "Voice off";

  // Dynamic bar height calculation
  const bars = [0.55, 0.85, 1.0, 0.7, 0.4].map((k) =>
    Math.max(8, Math.min(100, status.level * 100 * k * 2.2)),
  );

  const canUseVoice = Boolean(status.available || isWebSpeechAvailable || status.mic_open);

  return (
    <div className="voice">
      <button
        className={`voice-btn ${tone}`}
        onClick={toggle}
        title={
          speaking
            ? "Stop speaking"
            : status.mic_open
            ? "Click to stop listening"
            : status.detail || "Turn microphone on (JARVIS Mode)"
        }
        aria-label={
          speaking
            ? "Stop speaking"
            : status.mic_open
            ? "Turn microphone off"
            : "Turn microphone on"
        }
        disabled={!speaking && !canUseVoice}
      >
        <span className="voice-icon">{speaking ? "🔊" : status.mic_open ? "●" : "🎤"}</span>
      </button>

      <div className="voice-readout">
        <div className={`voice-label ${tone}`}>{label}</div>

        {/* Live dynamic meter */}
        <div className="voice-meter" aria-hidden="true">
          {bars.map((h, i) => (
            <span
              key={i}
              className={`voice-bar ${tone}`}
              style={speaking ? undefined : { height: `${status.mic_open ? h : 8}%` }}
            />
          ))}
        </div>
      </div>

      {status.mic_open && !transcribing && activeEngine === "whisper" && (
        <button className="voice-talk" onClick={talk} title="Force transcribe immediately">
          Talk
        </button>
      )}

      {error && (
        <div className="voice-hint" title={error}>
          {error}
        </div>
      )}
    </div>
  );
}
