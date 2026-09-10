# ORION — PROJECT STATE

**Last updated:** 2026-09-10
**Current state:** `READY_TO_BUILD` (pending repo creation)
**Current phase:** Architecture locked → M0 not started

---

## Locked decisions (G1–G7)

| Gate | Decision | Status |
|---|---|---|
| G1 | Project name `orion`, fresh repo. Serina archived as v1 reference. | **LOCKED** |
| G2 | Rust-only core (Tauri v2). No Python sidecar. | **LOCKED** |
| G3 | Windows + Linux first. macOS after v1. | **LOCKED** |
| G4 | Single full public release at M10. No partial release. | **LOCKED (risk accepted — see R-1)** |
| G5 | Global hotkey `Win+O` always on, permanent. Wake word = opt-in toggle in UI, default OFF. | **LOCKED** |
| G6 | Email: read + understand + proactively notify + draft reply + ask user before sending. Never auto-send. | **LOCKED** |
| G7 | Dev machine: Lenovo Slim, Ryzen 5 7250-series, **8 GB RAM** → **Tier T1**. | **LOCKED** |

---

## Hardware reality (G7) — this shapes everything

**Dev machine = 8 GB RAM. This is the binding constraint on the entire project.**

Consequences, stated plainly:

1. **You are your own worst-case user.** Everything you build will be validated on the
   hardware most likely to struggle. That is genuinely good for the product.
2. **You cannot test T2/T3/T4 locally.** Tier selection logic must be testable via a
   **forced-tier override / simulated-profile mode**, or it will never be verified.
   → This becomes a hard requirement in M1, not an afterthought.
3. **Rust compile + Tauri dev server + llama-server + browser on 8 GB is tight.**
   Mitigations, applied from day one:
   - `cargo` with `lto=false`, `debug=0` in the dev profile; use `sccache`
   - Keep `llama-server` **stopped** unless actively testing inference
   - Use the smallest model (Qwen3 4B Q4 / Phi-4-mini) for daily dev
   - Expect cold `cargo build` times in minutes; incremental builds are fine
4. **Voice on T1 will be 5–10 s round trip.** You will feel this every day. Use `tiny.en`
   for dev.
5. Suggested: add swap/pagefile headroom before starting M0.

---

## Active risks

| ID | Risk | Sev | Response |
|---|---|---|---|
| **R-1** | **Big-bang release at M10.** No external users until the very end → all feedback arrives after the code is set; motivation risk over a long solo build; a full-scope first release is also the largest possible security surface to expose at once. | **High** | **Accepted by owner.** Compensating controls: (a) tag every milestone as a *private* pre-release build, (b) run the adversarial suite from M5 onward continuously, (c) recruit 2–3 private testers around M6 without a public launch. |
| R-2 | Injection → RCE via actions | Critical | Domain separation; M5 boundary before any write tool |
| R-3 | Email as unauthenticated attack channel | Critical | Untrusted domain only; never auto-send |
| R-4 | 8 GB dev machine can't validate higher tiers | High | Forced-tier override in M1 (hard requirement) |
| R-5 | Rust learning curve (owner is new to Rust) | High | M0 is deliberately small; port UI first; thin vertical slices; heavy comments |
| R-6 | Scope over a 10-milestone solo build | **Critical** | Strict milestone exit criteria; no feature added mid-milestone |
| R-7 | Model licence terms for redistribution | Medium | Bundle only Apache/MIT (Qwen, Phi, Mistral). Verify Gemma/Llama before any bundling. |

---

## Next actions

1. **Owner:** create empty GitHub repo `orion` (public or private, either works).
2. **Owner:** confirm repo URL here.
3. **Agent:** scaffold M0 — Tauri v2 + React + SQLite + llama-server sidecar.
4. **Agent:** `README.md`, `LICENSE`, `.gitignore`, `ARCHITECTURE.md`, `DECISIONS.md`, CI.

---

## On deployment keys — read before sending anything

**Do not send me a deploy key, SSH key, PAT, or any other credential.**

- I have no secure secret storage. Anything pasted into chat is in plaintext in the
  conversation and in workspace files.
- I don't need one. I build in this workspace; **you** push.

**How we actually work:**
1. I generate the code here in `/home/user/orion`.
2. You either download the files, or I give you an exact `git` command sequence to run
   locally, or you clone-and-copy.
3. You commit and push under your own identity — which is correct anyway, since it's your
   project and your commit history.

If you later want CI publishing, GitHub Actions' built-in `GITHUB_TOKEN` covers it with no
secret handling from either of us.

---

## Milestones

| M | Milestone | Exit criteria |
|---|---|---|
| M0 | Shell: Tauri + llama-server + SQLite + streaming chat | Offline chat works on a fresh machine |
| M1 | Hardware profiler + model manager + **forced-tier override** | Correct tier chosen on 8/16/32 GB simulated profiles |
| M2 | Documents: multi-format, structure-aware chunking, hybrid BM25+vector, citations | 30-question eval set scored; baseline recorded |
| M3 | Everywhere: tray, `Win+O`, file drop, context menu | Cold start < 1 s |
| M4 | Voice: push-to-talk, STT, TTS, optional wake word | Measured round-trip documented per tier |
| M5 | **Capability broker + read-only tools, propose-only** | **Red team: injected PDF cannot fire a tool** |
| M6 | Private testing build; 2–3 testers | Feedback logged, no public launch |
| M7 | Email: read, triage, notify, draft | Injected email cannot trigger an action |
| M8 | Browser control, dedicated profile | Domain allowlist enforced |
| M9 | Desktop control + scoped writes with undo | Every change reversible via undo journal |
| M10 | Sandboxed T2/T3 execution + **public release** | Adversarial suite passes; signed installers; LICENSE |
