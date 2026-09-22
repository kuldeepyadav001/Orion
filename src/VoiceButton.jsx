import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/**
 * Microphone control and status indicator.
 *
 * The indicator is the point, not decoration. Voice is the one feature where
 * the user cannot tell whether it is working: a microphone that is not
 * hearing you looks exactly like a microphone that is hearing you and
 * choosing not to reply. And an assistant that *might* be listening is worse
 * than one that clearly is not.
 *
 * So this shows four distinct things:
 *
 *   mic closed      grey, nothing moving      — it cannot hear you
 *   listening       violet, idle              — open, waiting
 *   hearing you     green, live level meter   — sound is arriving right now
 *   transcribing    amber, pulsing            — working on what you said
 *
 * The level meter matters most. A static "listening" label proves nothing;
 * a bar that moves when you speak proves the audio path works end to end.
 */
export default function VoiceButton({ onTranscript, speaking }) {
    const [status, setStatus] = useState({
        state: "off",
        mic_open: false,
        hearing: false,
        level: 0,
        speech_ms: 0,
        detail: "Microphone off",
        available: false,
    });
    const [error, setError] = useState(null);

    // Held in a ref so the polling effect never re-subscribes. An inline
    // callback here caused a listener to be registered on every render
    // elsewhere in this app, and one dropped file got indexed ~150 times.
    const onTranscriptRef = useRef(onTranscript);
    useEffect(() => {
        onTranscriptRef.current = onTranscript;
    }, [onTranscript]);

    /* ---------- events from the backend ---------- */

    useEffect(() => {
        let cancelled = false;
        let unlisteners = [];

        const subscribe = async () => {
            const handles = await Promise.all([
                listen("voice://transcript", (e) => {
                    const text = (e.payload ?? "").trim();
                    if (text) onTranscriptRef.current?.(text);
                }),
                listen("voice://error", (e) => setError(String(e.payload))),
            ]);
            if (cancelled) {
                handles.forEach((u) => u());
                return;
            }
            unlisteners = handles;
        };
        subscribe();

        return () => {
            cancelled = true;
            unlisteners.forEach((u) => u());
        };
    }, []);

    /* ---------- status polling ---------- */

    useEffect(() => {
        let alive = true;
        const tick = async () => {
            try {
                const st = await invoke("voice_status");
                if (alive) setStatus(st);
            } catch {
                /* backend not ready */
            }
        };
        tick();
        // Display only: the backend drives the listener on its own timer, so
        // this just reads status. Fast while open so the meter looks live;
        // slow while closed to avoid burning CPU on a machine already running
        // a 2.4 GB model.
        const id = setInterval(tick, status.mic_open ? 120 : 1500);
        return () => {
            alive = false;
            clearInterval(id);
        };
    }, [status.mic_open]);

    const toggle = useCallback(async () => {
        setError(null);
        try {
            if (status.mic_open) {
                setStatus(await invoke("voice_stop"));
            } else {
                setStatus(await invoke("voice_start"));
            }
        } catch (e) {
            setError(String(e));
        }
    }, [status.mic_open]);

    const talk = useCallback(async () => {
        setError(null);
        try {
            setStatus(await invoke("voice_trigger"));
        } catch (e) {
            setError(String(e));
        }
    }, []);

    /* ---------- presentation ---------- */

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
            : recording
                ? "Hearing you…"
                : hearing
                    ? "Hearing you…"
                    : status.mic_open
                        ? "Listening"
                        : "Voice off";

    // 0..1 level to bar heights. A floor of 8% keeps the bars visible when
    // idle so the control does not look dead.
    const bars = [0.55, 0.85, 1.0, 0.7, 0.4].map((k) =>
        Math.max(8, Math.min(100, status.level * 100 * k * 2.2)),
    );

    return (
        <div className="voice">
            <button
                className={`voice-btn ${tone}`}
                onClick={toggle}
                title={status.available ? status.detail : status.detail}
                aria-label={status.mic_open ? "Turn microphone off" : "Turn microphone on"}
                disabled={!status.available && !status.mic_open}
            >
                <span className="voice-icon">{status.mic_open ? "●" : "○"}</span>
            </button>

            <div className="voice-readout">
                <div className={`voice-label ${tone}`}>{label}</div>

                {/* The live meter. This is the proof that audio is arriving —
            a label alone could be lying. */}
                <div className="voice-meter" aria-hidden="true">
                    {bars.map((h, i) => (
                        <span
                            key={i}
                            className={`voice-bar ${tone}`}
                            style={{ height: `${status.mic_open ? h : 8}%` }}
                        />
                    ))}
                </div>
            </div>

            {status.mic_open && !transcribing && (
                <button className="voice-talk" onClick={talk}>
                    Talk
                </button>
            )}

            {(error || (!status.available && !status.mic_open)) && (
                <div className="voice-hint" title={error ?? status.detail}>
                    {error ?? status.detail}
                </div>
            )}
        </div>
    );
}
