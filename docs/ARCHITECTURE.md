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

| Module | Milestone | Responsibility |
|---|---|---|
| `main.rs` | | Thin entry point |
| `lib.rs` | | Tauri setup, commands, sidecar lifecycle |
| `engine.rs` | M1 | llama-server process + OpenAI-compatible streaming client |
| `db.rs` | | SQLite schema, migration ladder, message persistence |
| `error.rs` | | Central error type, safe serialisation to the webview |
| `hashing.rs` | | Content hashing for deduplication |
| `profiler.rs` | M1 | Hardware detection and tier recommendation |
| `models.rs` | M1 | Model catalogue, download and checksum verification |
| `downloader.rs` | M1 | Resumable downloads with checksum verification |
| `documents.rs` | M2 | Ingestion, embedding sidecar, search commands |
| `ingest/` | M2 | Format extractors (`extract.rs`, `binary.rs`) and the structure-aware chunker (`chunker.rs`) |
| `rag/` | M2 | Embedding client (`embed.rs`), vector store (`store.rs`), hybrid ranking (`search.rs`), grounded context (`context.rs`), evaluation harness (`eval.rs`, `stats.rs`) |
| `presence/` | M3 | Startup trace, window, hotkey, file drop, tray glue |
| `residency.rs` | | Model residency, persona routing, sequential swap |
| `persona.rs` | | Persona registry and prompt assembly |
| `voice/` | M4 | Audio capture, Whisper STT, Piper TTS, wake word |
| `broker/` | M5 | Capability tiers, allowlist, audit log, undo journal, path rules |
| `search.rs` | | Web search for live answers |
| `email/` | M7 | IMAP sync and SMTP send |
| `browser/` | M8 | Isolated browser profile and accessibility-tree driver |
| `automation/` | M9 | OS UI automation |

Every module listed above exists in the tree. The remaining work is
measurement and polish, not scaffolding — see the gap ledger in
[`PROJECT-STATE.md`](PROJECT-STATE.md) §11.

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

**Build profiles tuned for a 2 GB build box.** Dev: `opt-level=0`, `debug=0`,
`lto=false`, `codegen-units=16`, `incremental=true`. Release: `opt-level=3`,
`lto="thin"`, `codegen-units=16`, `strip=true`.

Both settings are load-bearing rather than cosmetic. A dev profile of
`debug=1` with `codegen-units=256` needed more memory than the build host
has and was killed while compiling `gtk`; `debug=0` with 16 units builds the
same code in under a minute. On the release side, `opt-level="s"` with fat
LTO and `codegen-units=1` had rustc SIGKILLed during the link; `opt-level=3`
with thin LTO and 16 units links in ~15 minutes and produces a 21 MB binary.
Only release numbers may be quoted as product performance — the debug build
measures roughly 13x slower on the same retrieval path.

See the architecture-decisions section of
[`PROJECT-STATE.md`](PROJECT-STATE.md) for the reasoning behind each.
