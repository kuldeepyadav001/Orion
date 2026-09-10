# Serina v2 — Research & Feasibility Report

**Date:** 2026-09-10
**State:** `RESEARCHING` → decision gate pending
**Author:** Single Project Master Engineer
**Status of this document:** research findings + recommendation. **No code changed. No decision committed.**

---

## 0. What you asked for

1. Personal AI reachable from anywhere on your system (not just a browser tab)
2. Share reports, docs, all file types — advanced version of the B/M/A feature list
3. **Jarvis mode** — it acts on your computer under your instruction
4. **One downloadable file**, anyone installs it, works **without internet**
5. **Light** — small footprint, minimal boilerplate, still does everything

---

## 1. The three-way conflict (read this first)

Your requirements pull against each other. This is not a reason to give up; it is a reason to
choose deliberately instead of discovering it in month four.

### Conflict A — "Single file, offline" vs "capable model"

| Component | Realistic size |
|---|---|
| Tauri app shell (UI + Rust core) | 5–10 MB |
| llama.cpp binaries (CPU + Vulkan) | ~25–80 MB |
| Embedding model (bge-small / nomic Q8) | 80–130 MB |
| **Chat model — the problem** | **2.5 GB (Qwen3 4B Q4_K_M)** |

A truly offline single file = **~3 GB installer**. That is not "light."
A 15 MB installer that downloads the model on first run = **not offline on day one**.

**You cannot have all three. Pick two.**

Recommendation: **hybrid**. Ship a ~40 MB installer. First launch offers:
- "Download recommended model (2.5 GB)" — needs internet **once**, then offline forever
- "I already have a GGUF / Ollama" — point at it, never touch the internet
- Separately publish a **fat offline bundle** (~3 GB `.exe`/`.dmg`/`.AppImage`) for
  air-gapped users and for people you hand a USB stick to

Both are "one file." The user picks which one they download. This is what LM Studio,
Jan, and GPT4All all converged on — it is the proven answer.

### Conflict B — "Runs on any machine" vs "8 GB RAM"

Evidence from research: on **8 GB RAM CPU-only**, the practical ceiling is a **3–4B model at
Q4**. Qwen3 4B Q4_K_M (~3.4 GB resident) is the consensus 2026 pick; Phi-4-mini and
Gemma 3 4B are the fallbacks.

That matters because **agentic tool-calling is much harder than chat**. A 4B model will
mis-call tools. Not occasionally — regularly. Any Jarvis design must assume the model is
unreliable and make the *permission layer*, not the model, the safety boundary.

### Conflict C — "Jarvis mode" vs "RAG on untrusted documents"

**This is the serious one, and it is the reason I pushed back last time.**

You want, in one application:
- (a) ingest arbitrary PDFs/docs from the outside world
- (b) execute shell commands, move files, and drive the browser on your machine

Combining (a) and (b) in one trust domain builds a **complete remote-code-execution chain**:

```
attacker text hidden in a PDF you were emailed
    → retrieved into context by RAG
    → model reads it as instruction, not data
    → tool call fires
    → arbitrary code runs as YOU
```

This is not theoretical. 2026 has been a bloodbath for exactly this pattern:

- **Google Antigravity** — prompt injection → RCE via unsanitised `find_by_name` param;
  bypassed the product's own "Secure Mode" because the tool wasn't classified as a shell
  command [Pillar Security, Apr 2026]
- **CrewAI VU#221883** — 4 chained CVEs, RAG injection → `ctypes` → `libc.system()` → host RCE
- **Cursor CVE-2026-26268** — injection writes malicious git hooks → out-of-sandbox RCE
- **Claude Cowork CVE-2026-46331** — sandbox escape, read/write anywhere on host
- **MCP STDIO class** — command injection across LangFlow, GPT Researcher, LiteLLM
- Non-malicious failure too: agents deleting 15,000 photos, wiping drives, deleting home dirs

NVIDIA's AI Red Team and the CSA both reach the same conclusion:
**sanitisation is not a defence. Only execution isolation is.**

Notice the pattern in every single one of those CVEs: the vendor *had* a safety mode, and the
payload reached the OS through a path the safety mode didn't consider a "dangerous" path.
Your report's proposed safety design (allowlists + confirm dialog + kill switch) is exactly
the design that failed in all of those products.

**So: no, I will not tell you Jarvis mode is fine as sketched. But I am not saying don't build
it.** I'm saying build it with a real boundary. Section 4 has the design.

---

## 2. Stack recommendation (with evidence)

### 2.1 Shell — **Tauri v2**, not Electron

| Metric | Electron | Tauri v2 |
|---|---|---|
| Installer | 120–200 MB | 3–10 MB |
| RAM idle | 150–400 MB | 25–80 MB |
| Startup | 2–5 s | <0.3 s |

On an 8 GB machine every 100 MB matters, because the model wants 3.4 GB.
Tauri also gives a **capability/permission system** and first-class **sidecar** support
(bundling `llama-server` as a child binary) — both directly needed here.
Cost: you write Rust for the core. Given Jarvis mode, that's a feature — the privileged
layer *should* be memory-safe and compiled, not a Python process you can monkey-patch.

**Your React UI ports over almost unchanged.** That work is not wasted.

### 2.2 Inference — **llama.cpp `llama-server` as a bundled sidecar**

Drop Ollama. Ollama is a great dev tool and a bad dependency to ship: it's a separate ~1 GB
install, its own daemon, its own model store, and you cannot legally/practically bundle it
into "one file."

`llama-server` is a single binary, exposes an **OpenAI-compatible API**, and Tauri spawns it
as a sidecar on localhost with a random port + bearer token. Same abstraction as your current
`clients/` layer — so **M5 (cloud providers) and M1 (remote GPU) collapse into one adapter**:
everything, local or remote, speaks OpenAI-compatible.

Also: this fixes your B-8 finding for free — `/v1/chat/completions` applies the model's real
chat template instead of your hand-rolled `"User:\nAssistant:"` string.

### 2.3 Storage — **SQLite + sqlite-vec**, drop Postgres + Redis + Qdrant

This is the single biggest simplification available to you.

| Today | v2 |
|---|---|
| Postgres 17 container | SQLite file |
| Redis container | in-process cache (or nothing) |
| Qdrant container | `sqlite-vec` extension |
| Docker Compose, 6 services | one `serina.db` file |

Evidence: Qdrant holds ~400 MB RAM constantly even idle; LanceDB ~4 MB idle / ~150 MB
searching; sqlite-vec is an extension inside a file you already have. For a single user
with <50k chunks, sqlite-vec is exact brute-force search and is *fast enough* — and it gives
you **FTS5 full-text search in the same file**, which means **hybrid search (BM25 + vector)
becomes nearly free**. That directly fixes the retrieval-quality gap I flagged last time.

Backup becomes "copy one file." Uninstall becomes "delete one folder."
Redis's job (session cache) does not exist when there's no network hop.

If you ever exceed ~100k chunks, LanceDB is the documented migration target. Note it in
DECISIONS.md and move on.

Keep Postgres **only** if you genuinely intend multi-user. For a personal AI you do not.

### 2.4 Backend language — the real decision

Two viable shapes:

**Option 1 — Rust-only core.** Everything in Tauri's Rust process. Smallest, fastest,
no Python runtime to bundle (Python + deps adds 50–150 MB and a packaging nightmare).
Cost: PDF/DOCX/XLSX extraction in Rust is weaker than Python's ecosystem, and you'd be
rewriting your existing backend.

**Option 2 — Rust shell + Python sidecar.** Keep your FastAPI code nearly as-is, bundle it
with PyInstaller as a sidecar. Fastest path from where you are. Cost: size, slower startup,
and PyInstaller cross-platform packaging is genuinely painful.

**My recommendation: Option 1, phased.** Start Rust-only with PDF (`pdfium`/`lopdf`) and
plain text. Add a *optional* Python sidecar later only if a format demands it. Reason: your
hard constraint is "light," and a bundled CPython violates it more than any other choice.

This is a **high-impact, hard-to-reverse decision** and per your medium-autonomy setting I am
**not** making it without you. See the gate in §6.

### 2.5 Document handling — advanced, but boringly

Not one giant "extractor". A trait/interface with per-format impls:
PDF, DOCX, XLSX/CSV, MD/TXT, HTML, images (OCR later), code files.
Chunking must become **structure-aware** (headings, paragraphs, table rows) —
your current fixed 500-char slicer is the main thing hurting answer quality.

---

## 3. Reachable from anywhere on your system

"Not just a browser tab" is a solved UX problem:

- **Global hotkey** (e.g. `Ctrl+Space`) → floating command palette. Tauri does this natively.
- **System tray** always resident.
- **Share/context menu integration** — right-click a file → "Ask Serina". OS-level, per-platform.
- **Drag-and-drop onto the tray icon**.
- **Optional local HTTP API** bound to `127.0.0.1` with a token, so scripts can talk to it.
- **Clipboard actions** — select text anywhere, hotkey, ask.

None of these need a server. All of them need Tauri, not a Docker stack.

---

## 4. Jarvis mode — how to build it without building the CVE

The lesson from every 2026 agentic RCE is the same: **the LLM is never the security boundary.**
Design so that a fully-compromised model still cannot hurt you.

### 4.1 Non-negotiable: separate the trust domains

```
┌──────────────────────────────────────────────────────┐
│  DOCUMENT DOMAIN (untrusted input)                   │
│  PDFs, docs, web pages → RAG → chat                  │
│  Tools available: NONE                               │
└──────────────────────────────────────────────────────┘
                    ✗  no path
┌──────────────────────────────────────────────────────┐
│  ACTION DOMAIN (trusted input only)                   │
│  Direct typed user instruction → agent → tools       │
│  Retrieved document text NEVER enters here           │
└──────────────────────────────────────────────────────┘
```

If document content can reach a context that has tool access, you have rebuilt CrewAI's
CVE chain. Everything else is secondary to this one rule.

Where they must meet (e.g. "read this invoice and rename the file"), the crossing is
**one structured value, extracted and shown to you for confirmation** — never free text
flowing into the agent's instruction stream.

### 4.2 Capability tiers, enforced in Rust, not in the prompt

| Tier | Examples | Enforcement |
|---|---|---|
| **T0 read** | list dir, read file in allowlisted root, get clipboard | auto |
| **T1 reversible write** | create file, move to trash, write in workspace | notify + undo log |
| **T2 destructive** | delete, overwrite, mass rename, install | **typed confirm, per action** |
| **T3 shell / network** | run command, browser automation | **explicit per-call approval, sandboxed** |
| **BLOCKED** | anything touching Serina's own config, keys, allowlist, `~/.ssh`, `.aws`, `.env`, git hooks | **impossible, not "discouraged"** |

That last row is the one every vendor got wrong. The agent must have **no write path to its
own permission config** — CSA calls this Configuration-Based Sandbox Escape and it survived
uninstall/reinstall in Antigravity.

### 4.3 Concrete rules derived from the CVEs

- **No shell string interpolation, ever.** `execve`-style argv arrays only. The Antigravity
  RCE was flag injection (`-X`) through a parameter that was never meant to be a flag.
  Always prepend `--`. Validate every param against a strict schema.
- **Path resolution with `openat2`/`RESOLVE_BENEATH`** semantics — resolve symlinks *then*
  check containment. Naive `startswith()` checks lose to `../` and symlinks.
- **Deny-by-default allowlist of tools**, not a blocklist of commands. `check_safe()` denylists
  are trivially bypassed by obfuscation (MS-Agent CVE-2026-2256, CVSS 9.8).
- **Fail closed.** CrewAI's worst CVE was a *silent fallback* to an unsafe sandbox when Docker
  was unavailable. If the sandbox can't start, the tool must refuse to run.
- **Append-only audit log** the agent cannot write to, with full undo journal.
- **Dry-run first** for anything multi-file: show the plan, then execute.
- **Egress control** — the agent has no network tool by default.

### 4.4 Phasing — this is how it stays sane

- **J0** — read-only tools + *proposals only*. Agent says "run `git status`"; you click run.
  Zero autonomous execution. Ship this first; it's ~80% of the daily value at ~5% of the risk.
- **J1** — T1 writes inside one user-chosen workspace folder, with undo.
- **J2** — T2/T3 behind per-call approval and OS sandbox (bubblewrap/seccomp Linux, AppContainer
  Windows, sandbox-exec macOS).
- **J3** — browser automation, in a **separate profile**, never your logged-in one.

**Straight answer:** J0 is genuinely achievable and safe. J2–J3 are a serious multi-month
security engineering project on their own. Don't let the roadmap pretend otherwise.

### 4.5 A hard warning about the release requirement

You want to **release this so anyone can download and run it**. That changes your liability
posture completely.

A personal script that occasionally deletes the wrong file is your problem. A signed installer
distributed publicly, containing an LLM agent with shell access, is a **supply-chain and
safety exposure** — and the "it's local so it's private" framing makes users trust it *more*,
which makes the failure worse.

If you ship a public release:
- Jarvis mode **off by default**, opt-in, with an unambiguous warning screen
- No auto-execute defaults, ever
- Clear LICENSE + disclaimer (your repo currently has **no LICENSE at all** — legally nobody
  may use it)
- Signed binaries, reproducible builds, published hashes
- A documented way to report vulnerabilities

I'd suggest: **release the assistant + RAG publicly; keep Jarvis mode as an opt-in build flag
until it has had real adversarial testing.**

---

## 5. Proposed target architecture

```
┌─────────────────────── SERINA.app (Tauri v2) ────────────────────────┐
│                                                                       │
│   React UI  ·  tray  ·  global hotkey  ·  file drop  ·  palette      │
│  ───────────────────────────────────────────────────────────────────  │
│   RUST CORE                                                           │
│     chat orchestrator   ·   RAG pipeline   ·   model router           │
│     ingestion (pdf/docx/xlsx/md/txt/html)                             │
│     ┌──────────────────────────────────────────────────┐              │
│     │ CAPABILITY BROKER  (the security boundary)       │              │
│     │  allowlist · tiers · audit · undo · sandbox spawn│              │
│     └──────────────────────────────────────────────────┘              │
│  ───────────────────────────────────────────────────────────────────  │
│   serina.db  (SQLite + sqlite-vec + FTS5)                             │
│     chats · messages · documents · vectors · audit · settings         │
└───────────────┬───────────────────────────────┬───────────────────────┘
                │ sidecar (127.0.0.1 + token)   │ sandboxed child procs
        ┌───────▼────────┐             ┌────────▼─────────┐
        │  llama-server  │             │  tool executors  │
        │  (GGUF, local) │             │  (seccomp/AppC)  │
        └────────────────┘             └──────────────────┘
                │ optional, opt-in
        ┌───────▼──────────────────────┐
        │ remote OpenAI-compatible     │
        │ endpoint (your GPU / API)    │
        └──────────────────────────────┘
```

Six containers → one app + one sidecar + one file.
That is the "light" you asked for, and it's lighter than what you have now.

---

## 6. Decision gates — I need your call before building

**G1. Single-file strategy.** Slim installer + first-run model download, fat offline bundle,
or both? *(Recommend: both — slim as default, fat as an alternate release asset.)*

**G2. Core language.** Rust-only core, or keep Python as a bundled sidecar?
*(Recommend: Rust-only. Irreversible-ish, so it's your call.)*

**G3. Jarvis scope for v2.0.** Stop at J0 (propose-only), or commit to J2 (real execution
with sandboxing)? *(Recommend: J0 for v2.0, J1 for v2.1, J2 only after adversarial testing.)*

**G4. Repo strategy.** Evolve `serina` in place, or `serina-v2` as a fresh repo with the old
one archived? *(Recommend: new repo. The stack change is total; git history would be noise.)*

**G5. Public release timing.** Release once RAG is solid, or hold everything until Jarvis
ships? *(Recommend: release early without Jarvis. Get users, get bug reports, ship Jarvis
into a codebase that's already been stress-tested.)*

---

## 7. Honest risk register

| Risk | Severity | Note |
|---|---|---|
| Prompt injection → RCE via Jarvis | **Critical** | Only mitigated by domain separation + OS sandbox |
| 4B model too weak for reliable tool calls | High | Mitigate: propose-only mode; remote model option |
| Rust rewrite stalls the project | High | Mitigate: J0 scope, port UI first, vertical slices |
| Cross-platform packaging pain (3 OSes) | High | Mitigate: Linux + Windows first, macOS after |
| 3 GB download kills adoption | Medium | Mitigate: slim default |
| Model licence terms for redistribution | Medium | **Must verify per model.** Qwen/Phi/Mistral are Apache/MIT; Gemma and Llama have custom terms |
| Scope explosion (B1–A6 all at once) | **High** | This is the most likely way the project dies |

That last one is the real one. Your feature list is ~20 features across 3 phases plus an OS
rewrite plus a distribution story. Nobody ships that in one pass.

---

## 8. What I'd build, in order

**M0 — Skeleton (proves the whole idea)**
Tauri shell + llama-server sidecar + SQLite/sqlite-vec + streaming chat. No RAG.
*Evidence: fresh machine, install, chat offline.*

**M1 — Documents, done properly**
Multi-format ingest, structure-aware chunking, hybrid BM25+vector, citations.
*Evidence: 30-question eval set with a score to beat.*

**M2 — Everywhere on your system**
Tray, global hotkey, file drop, context menu, local API.

**M3 — Capability broker + J0**
Read-only tools, propose-only, audit log. Security boundary built *before* the first write tool.

**M4 — Release v1**
Installer for Linux + Windows, signed, LICENSE, docs. **Jarvis off / propose-only.**

**M5 — J1/J2**
Scoped writes with undo, then sandboxed execution — only after adversarial testing.

---

## 9. Bottom line

- The **stack shift is right**: Tauri + llama.cpp + SQLite/sqlite-vec makes it dramatically
  lighter, genuinely offline, and actually shippable as one file. Your current 6-container
  Docker stack can never be a consumer download.
- **"One file + offline + light" is a pick-two.** Slim installer with an offline bundle
  alternative is the honest resolution.
- **Jarvis mode: yes, but not as sketched.** The design must assume the model is
  compromised. Document domain and action domain must not touch. J0 first.
- **The biggest threat to this project is scope, not difficulty.** Everything on your list is
  buildable. All of it at once is not.

Nothing has been built. Awaiting G1–G5.
