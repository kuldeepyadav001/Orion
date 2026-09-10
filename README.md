<div align="center">

# Orion

**A private, offline-first personal AI that lives on your machine.**

Chats. Reads your documents. Listens and speaks. Acts on your computer — inside a
permission boundary that holds even when the model is wrong.

No cloud. No API keys. No data leaving your device.

</div>

---

## Status

**Pre-alpha — M0 in progress.** Not usable yet. Not released. See
[`docs/PROJECT-STATE.md`](docs/PROJECT-STATE.md) for the live build state.

| Milestone | What it delivers | State |
|---|---|---|
| **M0** | App shell: Tauri + llama.cpp sidecar + SQLite + streaming chat | 🔨 in progress |
| M1 | Hardware profiler + tiered model manager | ⬜ |
| M2 | Documents: multi-format, hybrid search, citations | ⬜ |
| M3 | Everywhere: tray, `Win+O` hotkey, file drop | ⬜ |
| M4 | Voice: speech in, speech out, optional wake word | ⬜ |
| M5 | Capability broker + read-only tools (propose-only) | ⬜ |
| M6 | Private testing build | ⬜ |
| M7 | Email: read, triage, draft | ⬜ |
| M8 | Browser control | ⬜ |
| M9 | Desktop control + reversible writes | ⬜ |
| M10 | Sandboxed execution + **public release** | ⬜ |

---

## Why Orion

Most "local AI" tools are either a chat box with no reach into your life, or a cloud agent
that reaches everywhere and sends it all to someone else's server. Orion is built for the
gap between those.

- **Offline by construction.** The model runs on your CPU/GPU via `llama.cpp`. There is no
  network call to make.
- **Right-sized to your machine.** Orion measures your RAM, CPU and GPU and picks a model
  tier that will actually run well — from a 4B on an 8 GB laptop to a 32B on a workstation.
- **One file.** A single installer. Nothing to orchestrate, no Docker, no daemons.
- **Safe by architecture, not by prompt.** Content Orion *reads* can never issue commands.
  Only content you *authored* can. See [Security model](#security-model).

---

## Architecture

```
┌──────────────────── ORION.app (Tauri v2) ─────────────────────┐
│  React UI · tray · Win+O hotkey · voice HUD · file drop       │
│───────────────────────────────────────────────────────────────│
│  RUST CORE                                                    │
│    chat · RAG · model router · hardware profiler              │
│    ingestion (pdf / docx / xlsx / md / html / code)           │
│    ┌───────────────────────────────────────────┐              │
│    │ CAPABILITY BROKER   ← the security boundary│              │
│    │   tiers · allowlist · audit · undo         │              │
│    └───────────────────────────────────────────┘              │
│───────────────────────────────────────────────────────────────│
│  orion.db  (SQLite + sqlite-vec + FTS5)                       │
└────┬──────────────┬──────────────┬──────────────┬─────────────┘
     ▼              ▼              ▼              ▼
 llama-server   whisper.cpp     Piper       sandboxed tools
  (chat/embed)     (STT)         (TTS)
```

One app. One database file. Sidecars spawned on `127.0.0.1` with a per-session token.

| Layer | Choice | Why |
|---|---|---|
| Shell | Tauri v2 | 3–10 MB vs Electron's 120–200 MB; ~30 MB RAM idle |
| Core | Rust | The privileged layer should be compiled and memory-safe |
| UI | React + Vite | Fast, familiar |
| Inference | llama.cpp `llama-server` | Single binary, OpenAI-compatible, bundleable |
| Storage | SQLite + sqlite-vec + FTS5 | One file. Vector **and** keyword search together. |
| STT / TTS | whisper.cpp / Piper | No Python runtime anywhere |
| Browser | Playwright (accessibility tree) | Deterministic refs, not pixel guessing |

Full reasoning in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and
[`docs/DECISIONS.md`](docs/DECISIONS.md).

---

## Model tiers

Orion detects your hardware and recommends — you can always override.

| Tier | Machine | Model | Resident |
|---|---|---|---|
| T1 Minimal | ≤8 GB RAM, CPU only | Qwen3 4B Q4_K_M | ~3.4 GB |
| T2 Standard | 16 GB RAM | Qwen3 8B Q4_K_M | ~5.5 GB |
| T3 Performance | 32 GB / 8–12 GB VRAM | Qwen3 14B Q4_K_M | ~9 GB |
| T4 Workstation | ≥24 GB VRAM | Qwen3 32B | ~19 GB |
| T0 Remote | any | your own GPU box or an API | ~0 |

Budgeted against **free** RAM, not total — and Orion benchmarks real tokens/sec after first
load, offering to step down if your machine is struggling.

---

## Security model

Orion assumes the language model **will** be wrong, and eventually **will** be hijacked by
text hidden in a document or email. The design holds anyway.

**The core rule — two domains that never touch:**

```
  UNTRUSTED                          TRUSTED
  documents · email bodies           you typed it · you said it
  web pages · file contents                   │
        │                                     ▼
        ▼                            ┌─────────────────┐
   chat / answers                     │ CAPABILITY      │
   NO TOOL ACCESS  ──────✗──────►     │ BROKER          │
                                      └────────┬────────┘
                                               ▼
                                          tool executes
```

Also enforced:

- **Tiered capabilities** — read (auto) → reversible write (undo) → destructive (typed
  confirm) → shell/network (per-call approval + OS sandbox)
- **No shell string interpolation.** `argv` arrays only, always `--`-terminated.
- **Path containment** resolves symlinks *before* checking, not after.
- **Orion cannot modify its own permissions.** No write path to its config, allowlist, or
  keys — nor to `~/.ssh`, `.aws`, `.env`, git hooks or shell rc files.
- **Fail closed.** If the sandbox won't start, the tool refuses to run.
- **Append-only audit log** the agent cannot reach, plus a full undo journal.
- **Email is never sent automatically.** Orion drafts; you press send.

This design is a direct response to the 2026 wave of agentic RCE — Google Antigravity,
CrewAI (CVE-2026-2275 and friends), Cursor, Claude Cowork. Every one of those products had a
"safe mode"; in every case the payload reached the OS through a path that safe mode did not
consider dangerous. Sanitisation is not a boundary. Isolation is.

---

## Building from source

**Prerequisites:** [Rust](https://rustup.rs/) stable, Node.js 20+, and the
[Tauri v2 system dependencies](https://v2.tauri.app/start/prerequisites/) for your OS.

```bash
git clone git@github.com:kuldeepyadav001/Orion.git
cd Orion
npm install
npm run tauri dev
```

Targets Windows and Linux first; macOS support lands after v1.

---

## Project documentation

| Document | Purpose |
|---|---|
| [`docs/PROJECT-STATE.md`](docs/PROJECT-STATE.md) | Live state, risks, next actions |
| [`docs/ORION-PLAN.md`](docs/ORION-PLAN.md) | Full plan, scope, milestones |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | System design |
| [`docs/DECISIONS.md`](docs/DECISIONS.md) | Decision log with rationale |
| [`docs/SECURITY.md`](docs/SECURITY.md) | Threat model, reporting |

---

## License

[Apache License 2.0](LICENSE).

Model weights are **not** covered by this license — each model carries its own terms.
Orion bundles only permissively licensed models (Qwen, Phi, Mistral); others must be
supplied by the user.

---

## Acknowledgements

Orion is the successor to [Serina](https://github.com/kuldeepyadav001/serina), a
Docker-based microservice AI assistant. Serina proved the RAG pipeline; Orion rebuilds it as
something you can actually hand to another person.
