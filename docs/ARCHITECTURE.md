# Architecture

## System

```
┌──────────────────── ORION.app (Tauri v2) ─────────────────────┐
│  WEBVIEW (untrusted)                                          │
│    React UI · tray · Win+O hotkey · voice HUD · file drop     │
│    receives: rendered tokens, status                          │
│    never receives: ports, tokens, filesystem paths            │
│═══════════════════ IPC: explicit commands ════════════════════│
│  RUST CORE (trusted)                                          │
│    chat orchestrator · RAG · model router                     │
│    hardware profiler · ingestion pipeline                     │
│    ┌───────────────────────────────────────────┐              │
│    │ CAPABILITY BROKER   ← security boundary   │   (M5)       │
│    │   tiers · allowlist · audit · undo        │              │
│    └───────────────────────────────────────────┘              │
│───────────────────────────────────────────────────────────────│
│  orion.db  (SQLite + sqlite-vec + FTS5)                       │
└────┬──────────────┬──────────────┬──────────────┬─────────────┘
     ▼              ▼              ▼              ▼
 llama-server   whisper.cpp     Piper       sandboxed tools
  (chat/embed)     (STT)         (TTS)          (M9/M10)
  127.0.0.1:<ephemeral> + bearer token
```

## Module map (`src-tauri/src/`)

| Module | Responsibility |
|---|---|
| `main.rs` | Thin entry point |
| `lib.rs` | Tauri setup, commands, sidecar lifecycle |
| `engine.rs` | llama-server process + OpenAI-compatible streaming client |
| `db.rs` | SQLite schema, migration ladder, message persistence |
| `error.rs` | Central error type, safe serialisation to the webview |

Planned: `profiler.rs` (M1), `models.rs` (M1), `ingest/` (M2), `rag.rs` (M2), `voice.rs` (M4),
`broker/` (M5), `tools/` (M7–M10).

## Data flow — a chat turn

```
user types
   → invoke("send_message")
   → persist user turn to SQLite      ← before generation, so nothing is lost
   → load last 20 turns (newest, chronological)
   → prepend system prompt
   → POST /v1/chat/completions (stream) to llama-server
   → for each SSE delta: emit "chat://token"
   → on completion: persist assistant turn, emit "chat://done"
```

Cancellation sets an atomic flag the stream loop checks per chunk.

## Key invariants

1. **The webview is untrusted.** Every capability is an explicit command.
2. **The sidecar is unreachable off-machine.** Loopback, ephemeral port, per-session token.
3. **Untrusted content never reaches a tool-capable context.** (Enforced from M5.)
4. **Schema changes go through the migration ladder.** Never `CREATE TABLE IF NOT EXISTS`.
5. **One database file.** Backup = copy; uninstall = delete a folder.

## Notable choices

**Events, not HTTP, for streaming.** The renderer never learns the sidecar's port or token,
so even a markdown-rendering XSS cannot reach the model.

**OpenAI-compatible endpoint.** Lets llama.cpp apply the model's real chat template, and
makes local, remote-GPU and cloud providers the same adapter.

**UTF-8 safe streaming.** SSE frames are accumulated in a buffer and only complete lines are
consumed — decoding each network chunk independently corrupts multi-byte characters split
across boundaries.

**Dev profile tuned for 8 GB.** `opt-level=0`, `lto=false`, `codegen-units=256`,
`incremental=true`. Release uses `opt-level="s"` + LTO + strip for a small binary.

See [`DECISIONS.md`](DECISIONS.md) for the reasoning behind each.
