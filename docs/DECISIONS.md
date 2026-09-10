# Decision Log

Significant, hard-to-reverse choices with their reasoning. Newest first.
Format: context → alternatives → decision → consequences → reversal cost.

---

## ADR-008 — Persist the user turn before generation

**Context.** Serina wrote both the user and assistant messages only after the stream
completed. Closing the tab mid-generation lost the user's own question permanently.

**Decision.** Write the user message to SQLite immediately, before calling the model. The
assistant reply is written on completion.

**Consequences.** An interrupted generation leaves a user turn with no reply — correct and
recoverable. Regression covered by tests.

**Reversal:** trivial.

---

## ADR-007 — Tokens stream over Tauri events, not HTTP to the webview

**Context.** The webview needs streaming tokens; the naive route is letting it call
llama-server directly.

**Alternatives.** (a) expose the sidecar port to the webview, (b) proxy HTTP through Rust,
(c) Rust owns the connection and emits events.

**Decision.** (c). Commands `send_message` / `cancel_generation`, events `chat://token`,
`chat://done`, `chat://error`.

**Consequences.** The renderer never learns the port or bearer token, so a compromised
frontend (XSS via rendered markdown, say) cannot reach the model or the sidecar. Cancellation
is a first-class command rather than an aborted fetch.

**Reversal:** moderate.

---

## ADR-006 — Sidecar on loopback, ephemeral port, random bearer token

**Context.** Serina published `ollama:11434`, `qdrant:6333`, `redis:6379` and `api:8000` to
the host with no authentication. On any shared network that exposed the user's entire
document store and an open inference server.

**Decision.** `llama-server` binds `127.0.0.1` on an OS-assigned free port with a 48-char
random `--api-key` generated per session.

**Consequences.** Nothing is reachable off-machine. Port collisions are impossible.

**Reversal:** trivial.

---

## ADR-005 — Real migration ladder, not `CREATE TABLE IF NOT EXISTS`

**Context.** Serina called SQLModel's `create_all()` on startup and called it
"auto-migrations". That creates *missing tables* and silently ignores every change to an
existing one — adding a column later would simply never reach an installed database.

**Decision.** `PRAGMA user_version` plus an ordered, idempotent migration ladder in `db.rs`.

**Consequences.** Schema changes are explicit and testable. Users upgrading Orion get their
database migrated rather than quietly broken.

**Reversal:** N/A — this is the correct baseline.

---

## ADR-004 — SQLite + sqlite-vec instead of Postgres + Redis + Qdrant

**Context.** Serina ran six containers for one user. Qdrant alone holds ~400 MB RAM at idle;
the target machine has 8 GB total.

**Alternatives.** Keep the stack; LanceDB; sqlite-vec.

**Decision.** One SQLite file with `sqlite-vec` for vectors and FTS5 for keywords.

**Consequences.** Backup is copying one file. Uninstall is deleting one folder. Hybrid
BM25 + vector search becomes nearly free, which directly addresses the retrieval-quality gap
in Serina. Redis's role (session cache) disappears entirely once there is no network hop.
Trade-off: sqlite-vec is exact brute-force, fine below ~100k chunks.

**Reversal:** moderate — LanceDB is the documented migration target if we exceed that.

---

## ADR-003 — llama.cpp `llama-server` sidecar instead of Ollama

**Context.** The product must ship as one installer that works offline.

**Alternatives.** Require Ollama; bundle Ollama; bundle llama-server.

**Decision.** Bundle `llama-server` as a Tauri sidecar.

**Consequences.** Ollama is a ~1 GB separate install with its own daemon and model store —
it cannot be shipped inside one file. `llama-server` is a single binary exposing an
OpenAI-compatible API, which also collapses "remote GPU" and "cloud provider" support into
the same adapter. Using `/v1/chat/completions` applies the model's real chat template
instead of a hand-built `"User:\nAssistant:"` string, which measurably improves output.

**Reversal:** low — the client is one module.

---

## ADR-002 — Rust-only core, no Python sidecar

**Context.** Serina's backend is Python/FastAPI. Reusing it means bundling CPython.

**Alternatives.** PyInstaller sidecar (fast to start, +50–150 MB and painful cross-platform
packaging); Rust-only (smaller, slower to write, owner is new to Rust).

**Decision.** Rust-only.

**Consequences.** The hard constraint is "light", and a bundled CPython violates it more than
any other single choice. The privileged layer — which will eventually execute shell commands
— should be compiled and memory-safe. Cost: a real learning curve, and PDF/DOCX extraction
in Rust is weaker than Python's ecosystem. Accepted; revisit only if a format demands it.

**Reversal:** expensive. Deliberately gated with the owner.

---

## ADR-001 — Tauri v2 instead of Electron

**Context.** Needs to be a native app: tray, global hotkey, file drop, OS integration.

**Decision.** Tauri v2.

**Consequences.** 3–10 MB installer vs 120–200 MB; ~30 MB RAM idle vs 150–400 MB; sub-second
start. On an 8 GB machine where the model wants 3.4 GB, every 100 MB is real. Tauri's
capability system and sidecar support are both directly required. Trade-off: OS webview means
minor CSS inconsistency across platforms.

**Reversal:** expensive.

---

## ADR-000 — New project rather than Serina v2

**Context.** The stack changes completely: Docker microservices → single native app.

**Decision.** New repo `Orion`. Serina stays public and archived as the v1 artifact.

**Consequences.** Only the React components and RAG *concepts* carry over. Clean history.
Serina remains a legitimate portfolio piece showing the progression.
