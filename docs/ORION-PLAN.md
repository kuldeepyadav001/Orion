# ORION — Master Plan v0.1

**A private, offline-first personal AI that lives on your machine, talks, sees your documents, and acts on your behalf.**

**Date:** 2026-09-10 · **State:** `DESIGNING` · **Predecessor:** Serina (archived reference)
**Status:** plan + decision gates. Nothing built yet.

---

## 0. What Orion is

One installer. Works offline. Reachable anywhere on your system by hotkey or voice.
Answers from your documents. Acts on your computer when you tell it to — inside a
permission boundary that holds even if the model is wrong or hijacked.

**Why a new project, not Serina v2:** the stack changes completely (Docker microservices →
single native app). Nothing but the React components and the RAG *concepts* survive. A clean
repo is correct. Serina stays public and archived as the v1 learning artifact — it's a
legitimate portfolio piece, don't delete it.

---

## 1. Scope — everything you've asked for, organised

| # | Capability | Tier |
|---|---|---|
| C1 | Natural chat, streaming, persistent history | Core |
| C2 | Document intelligence — PDF, DOCX, XLSX, CSV, MD, TXT, HTML, code | Core |
| C3 | Hardware-aware model selection at install | Core |
| C4 | Reachable everywhere — tray, global hotkey, file drop, context menu | Core |
| C5 | Voice — speak to it, it speaks back, wake word | Core |
| C6 | Read & respond to email | Action |
| C7 | Open apps, control the desktop | Action |
| C8 | Browse, scroll, click, fill forms | Action |
| C9 | File management, shell | Action |
| C10 | Single-file release, offline, cross-platform | Release |

---

## 2. The one architectural idea that makes this work

**Accessibility trees, not screenshots.**

For browser control, desktop control, and document structure — read the *semantic* tree the
OS/browser already maintains for screen readers, never pixels.

Evidence (2026): Playwright's entire AI ecosystem abandoned screenshots for the accessibility
tree. Numbers: **2–5 KB of structured text vs 500 KB–2 MB per screenshot**, 10–100× faster,
vision tokens cost 3–5× text tokens, and element references are *deterministic* — the agent
says "click ref abc123" instead of guessing x,y coordinates.

Why this decides the whole project: **a 4B local model cannot reliably do vision-based
coordinate clicking.** It can absolutely pick a labelled element out of a text list. This one
choice is what makes Jarvis-mode feasible on 8 GB CPU-only hardware instead of fantasy.

Same principle everywhere:
- Browser → Playwright accessibility snapshot
- Windows desktop → UI Automation · macOS → AX API · Linux → AT-SPI
- Documents → structural extraction (headings, tables), not flat text dumps

Vision is the **fallback only** for canvas/WebGL/legacy apps where the tree is empty.

---

## 3. Hardware-aware model selection (your idea — it's a good one)

This is genuinely one of the best ideas in your message. Done properly it's a real
differentiator, because most local AI tools either dump a model list on the user or ship one
model that's wrong for half of them.

### 3.1 Detect, don't ask (but let them override)

At install and on every launch, probe: total RAM, free RAM, CPU cores + AVX2/NEON, GPU vendor
+ VRAM, Vulkan/Metal/CUDA availability, free disk.

Then **recommend** a tier, show the reasoning, and let the user change it. Never silently
decide, never make them guess their own specs from a dropdown.

### 3.2 Tier table

| Tier | Detected | Chat model | Resident | Expect |
|---|---|---|---|---|
| **T1 Minimal** | ≤8 GB RAM, CPU | Qwen3 4B Q4_K_M (or Phi-4-mini) | ~3.4 GB | Chat + RAG. Voice OK. Agent = propose-only. |
| **T2 Standard** | 16 GB RAM | Qwen3 8B Q4_K_M | ~5.5 GB | Comfortable. Tool calls become usable. |
| **T3 Performance** | 32 GB / 8–12 GB VRAM | Qwen3 14B Q4_K_M | ~9 GB | Strong reasoning, reliable agent. |
| **T4 Workstation** | ≥24 GB VRAM | Qwen3 32B / Gemma 3 27B | ~19 GB | Near-frontier, fully local. |
| **T0 Remote** | any | your GPU box / API | ~0 | Weak laptop, heavy model. |

Companions, all tiers: embeddings ~120 MB · Whisper STT 75–470 MB · Piper TTS 15–80 MB.

### 3.3 Rules that keep it honest

- Budget against **free** RAM, not total. An 8 GB machine with Chrome open has ~4 GB.
- Leave ≥2 GB headroom for OS. Never recommend a model that will swap.
- **Benchmark after first load** — measure real tok/s. If <3 tok/s, offer to step down.
- **Licence matters for redistribution:** Qwen (Apache 2.0), Phi (MIT), Mistral (Apache 2.0)
  are safe to bundle. **Gemma and Llama have custom terms — verify before shipping.**
- Model registry is a **signed JSON manifest**, so new models ship without an app update.
- Runtime downgrade: if a T3 machine is under memory pressure, offer the T2 model.

---

## 4. Voice (C5)

Fully local pipeline, all offline:

```
mic → VAD (Silero) → wake word → Whisper STT → Orion → Piper TTS → speakers
```

| Stage | Choice | Size | Why |
|---|---|---|---|
| STT | **whisper.cpp** | tiny 75 MB → small 466 MB by tier | C binary, no Python, CPU/Vulkan/Metal, streaming |
| TTS | **Piper** | 15–80 MB | ~50× real-time on CPU, good quality |
| VAD | Silero | ~2 MB | Cheap, avoids constant transcription |
| Wake word | openWakeWord | ~5 MB | "Hey Orion", fully offline |

Not faster-whisper: it drags in Python + CTranslate2, and is CUDA-only for GPU. whisper.cpp
matches the same stack decision as llama.cpp — one toolchain, no runtime.

**Latency reality — say this out loud:** CPU-only round trip is **5–10 s**. GPU is 2–4 s.
On T1 hardware, voice will feel like a walkie-talkie, not like talking to a person. Mitigate
by streaming TTS from the first sentence rather than waiting for the full reply, and using
tiny.en on T1. Set expectations in the UI; don't let users think it's broken.

**Security note:** voice is *trusted* input (you spoke it) — it may reach the action domain.
Audio from a file or video is **untrusted** and must not.

---

## 5. Actions — C6/C7/C8/C9

### 5.1 The rule that governs all of it

```
   UNTRUSTED DOMAIN                    TRUSTED DOMAIN
   documents · email bodies            you typed it · you said it
   web pages · file contents                    │
            │                                   ▼
            ▼                          ┌──────────────────┐
      chat / answers                   │ CAPABILITY BROKER│
      NO TOOL ACCESS  ─────✗─────►     │  tiers · audit   │
                                       │  undo · sandbox  │
                                       └────────┬─────────┘
                                                ▼
                                            tool runs
```

Content Orion *reads* can never issue instructions. Only content you *authored* can.
Crossings happen as structured values you confirm — never free text into the instruction stream.

This is not paranoia. In 2026 this exact chain produced RCE in Google Antigravity, CrewAI
(4 CVEs), Cursor, and Claude Cowork. Every one of them had a "safe mode" that the payload
walked around.

### 5.2 Email (C6) — the sharpest edge in the whole project

Email is **attacker-controlled text that arrives unsolicited**. If Orion can read mail and
also act, anyone who knows your address can attempt to drive your computer.

- Read via IMAP (app password / OAuth in OS keychain — never in a config file)
- Email bodies enter the **untrusted domain only**. Summaries, triage, search: fine.
- **Drafting a reply is allowed. Sending is never automatic.** You review and press send.
- Strip HTML, hidden text, white-on-white, zero-width chars before the model sees it
- Never auto-follow links or fetch remote images from mail
- Attachments quarantined; ingest only on explicit request

### 5.3 Browser (C8)

Playwright with accessibility snapshots. Tools: `navigate`, `snapshot`, `click`, `type`,
`scroll`, `fill_form`, `extract`.

- Runs in a **dedicated Orion browser profile** — never your logged-in personal one.
  Bank tabs and session cookies stay out of reach.
- Domain allowlist, off by default.
- Page content = untrusted. A page cannot cause a new tool call.
- No downloads without confirm; no credential fields ever auto-filled.

### 5.4 Desktop (C7) & Files (C9)

- Launch apps: allowlisted, `argv` arrays only, **never shell string interpolation**
  (that was the exact Antigravity flag-injection RCE — always prepend `--`).
- Window/UI control via platform accessibility APIs, T1+ tier.
- Files: allowlisted roots, symlink-resolved containment (`RESOLVE_BENEATH` semantics —
  naive `startswith()` loses to `../` and symlinks). Deletes go to trash, never `unlink`.
- **Permanently blocked, not "discouraged":** Orion's own config/allowlist/keys, `~/.ssh`,
  `.aws`, `.env`, git hooks, shell rc files, startup folders, system dirs.
  The agent must have **no write path to its own permissions** — that's the
  Configuration-Based Sandbox Escape that survived uninstall in Antigravity.

### 5.5 Capability tiers

| Tier | Examples | Gate |
|---|---|---|
| T0 read | list dir, read allowlisted file, snapshot page, read mail | auto |
| T1 reversible | create/edit in workspace, draft reply, open app | notify + undo |
| T2 destructive | delete, mass rename, overwrite, install | typed confirm |
| T3 shell/net | run command, browser writes, send email | per-call approval + sandbox |
| BLOCKED | self-config, credentials, system paths | impossible |

**Fail closed.** If the sandbox can't start, the tool refuses — CrewAI's worst CVE was a
*silent fallback* to an unsafe sandbox when Docker was missing.

**Everything is dry-run first** for multi-step plans: show the plan, then execute.
Append-only audit log the agent cannot touch. Global kill switch.

---

## 6. Stack

| Layer | Choice | Why |
|---|---|---|
| Shell | **Tauri v2** | 3–10 MB vs Electron 120–200 MB; 25–80 MB RAM idle; capability system; sidecars; tray + global hotkey native |
| Core | **Rust** | Privileged layer should be compiled + memory-safe; no CPython to bundle |
| UI | React (ported from Serina) | Your work carries over |
| LLM | **llama.cpp `llama-server`** sidecar | Single binary, OpenAI-compatible, bundleable. Ollama can't be shipped in one file. |
| Storage | **SQLite + sqlite-vec + FTS5** | One file. Replaces Postgres+Redis+Qdrant. Hybrid BM25+vector nearly free. |
| STT/TTS | whisper.cpp + Piper | Same no-runtime philosophy |
| Browser | Playwright | Accessibility tree |
| Tools | MCP-shaped internally | Standard tool schema; **do not** load third-party MCP servers by default (43% of v2.0 impls had command injection) |

Six containers → one app + sidecars + one `.db` file.

```
┌──────────────────── ORION.app (Tauri v2) ─────────────────────┐
│ React UI · tray · hotkey · voice HUD · file drop              │
│───────────────────────────────────────────────────────────────│
│ RUST CORE                                                     │
│   chat · RAG · model router · hardware profiler               │
│   ingestion (pdf/docx/xlsx/md/html/code)                      │
│   ┌───────────────────────────────────────────┐               │
│   │ CAPABILITY BROKER  ← security boundary    │               │
│   └───────────────────────────────────────────┘               │
│───────────────────────────────────────────────────────────────│
│ orion.db (SQLite + sqlite-vec + FTS5)                         │
└────┬──────────────┬──────────────┬──────────────┬─────────────┘
     ▼              ▼              ▼              ▼
 llama-server   whisper.cpp     Piper       sandboxed tools
                                            (seccomp/AppContainer)
```

---

## 7. Distribution (C10)

**Still a pick-two.** Model weights are 2.5–5 GB; that's physics.

| Asset | Size | Offline day one |
|---|---|---|
| `Orion-Setup.exe` (slim) | ~60 MB | no — one download |
| `Orion-Offline-T1.exe` (fat) | ~3.2 GB | **yes** |
| `Orion-Offline-T2.exe` | ~6 GB | yes |

Both are "one file." User picks. Slim is the default; fat exists for air-gapped machines and
USB-stick handoff. Ship Linux + Windows first, macOS after (signing/notarisation is its own
project).

**Public release conditions:** LICENSE (Serina still has none — legally unusable), signed
binaries, published hashes, security policy, **actions off by default with an explicit
opt-in warning screen.**

---

## 8. Build order

| M | Milestone | Proves | Evidence |
|---|---|---|---|
| **M0** | Tauri + llama-server + SQLite + streaming chat | the shell works | fresh machine, offline chat |
| **M1** | Hardware profiler + model manager | your tiering idea | 8/16/32 GB VMs each pick correctly |
| **M2** | Documents: multi-format, structure-aware chunking, hybrid search, citations | the product | **30-question eval set, scored** |
| **M3** | Everywhere: tray, hotkey, file drop, context menu | reach | cold-start <1 s |
| **M4** | Voice in/out + wake word | C5 | measured round-trip per tier |
| **M5** | **Capability broker + read-only tools, propose-only** | safety *before* power | red-team: injected PDF cannot fire a tool |
| **M6** | **Release v1** — Linux + Windows, signed, actions opt-in | shippable | install on a machine that never had it |
| **M7** | Email read/triage/draft | C6 | injected email cannot act |
| **M8** | Browser control (own profile) | C8 | allowlist enforced |
| **M9** | Desktop control + scoped writes with undo | C7/C9 | undo restores every change |
| **M10** | T2/T3 sandboxed execution | full Jarvis | adversarial suite passes |

**M5 before M7–M10 is non-negotiable.** The boundary gets built before anything can write.

**Release at M6, not M10.** Get real users on the safe half while the dangerous half matures.

---

## 9. Honest risks

| Risk | Sev | Mitigation |
|---|---|---|
| Injection → RCE via actions | **Critical** | Domain separation, OS sandbox, M5 first |
| Email as attack vector | **Critical** | Untrusted-only, never auto-send |
| Scope kills the project | **Critical** | 10 milestones, ship at M6 |
| 4B too weak for tool calls | High | Propose-only on T1; tiering; remote option |
| Voice latency disappoints on T1 | Medium | Stream TTS early, set expectations |
| Rust learning curve | High | Port UI first, thin vertical slices |
| Desktop accessibility APIs differ per OS | High | Trait + per-OS impl; Windows/Linux first |
| Model licences block redistribution | Medium | Qwen/Phi/Mistral only in bundles |
| Cross-platform packaging | High | 2 OSes first |

**The top risk is scope, not difficulty.** Ten milestones, voice, three OSes, and an agent
that drives your computer — that is a year of serious evenings. It's achievable in that
order. It is not achievable all at once.

---

## 10. Decisions needed (G1–G7)

1. **G1 Name/repo** — confirm `orion`, fresh repo, Serina archived with a pointer?
2. **G2 Core language** — Rust-only (recommended) or Python sidecar for faster start?
3. **G3 OS priority** — Linux+Windows first (recommended), or macOS in v1?
4. **G4 Release point** — ship at M6 without actions (recommended), or hold for M10?
5. **G5 Voice depth** — push-to-talk hotkey only for v1, or wake word from the start?
   *(Recommend hotkey first: always-on mic is a privacy + battery + false-trigger problem.)*
6. **G6 Email** — read/triage/draft only (recommended), or auto-send with rules?
7. **G7 Your hardware** — what are you developing on? It sets the default tier and what I can
   realistically ask you to test.

---

## 11. Straight answers

- **New project: yes.** Correct call. Orion is a different system, not a Serina release.
- **Hardware-aware tiering: yes, and it's your best idea here.** Detect and recommend rather
  than asking users to self-report specs.
- **Voice: yes, straightforward** — whisper.cpp + Piper, but be honest that T1 CPU round-trip
  is 5–10 s.
- **Open apps / scroll / click / browse: yes** — via accessibility trees, which is also the
  only way a 4B model can do it reliably.
- **Read and respond to email: read yes, auto-respond no.** Draft-and-review. An inbox is an
  unauthenticated input channel from the entire internet; wiring it to an agent that can act
  is the single most dangerous thing on your list.
- **Everything at once: no.** M5 (the boundary) before M7–M10 (the power), and ship at M6.

Awaiting G1–G7.
