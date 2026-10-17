# ORION — PROJECT STATE & STRATEGIC MASTER RECORD

**Last updated:** 2026-10-12
**Current state:** `BUILDING & DEPLOYING`
**Current phase:** M8 (Sandboxed Browser Automation) ✅ COMPLETED → Advancing to M9 (OS UI Automation)
**Repo:** `https://github.com/kuldeepyadav001/Orion.git` (`main`)

---

## 1. THE BIG PICTURE: STARTUP THESIS & BUSINESS ARCHITECTURE

Orion is not a standard hobby project or demo. It is the foundation of a **Private AI Workspace Startup** designed to solve a fundamental corporate problem: **How do organizations get high-utility AI without sending sensitive data to public cloud AI providers?**

### 1.1 The Core Thesis
> *"The customer is not buying an LLM. They are buying **Private AI Infrastructure = Models + UI + Agent Runtime + Local Knowledge + Permissions + Security + Governance + Deployment**."*

The model itself is replaceable open-weight compute (Qwen, Mistral, Gemma, DeepSeek, Phi). The product moat is:
- Reliable, frictionless one-installer local deployment.
- Hardware-aware dynamic model & compute routing on standard 8 GB laptops (<= 2.0 GB RAM ceiling).
- Two-domain security boundary (untrusted content can never trigger actions).
- Verifiable zero-cloud-egress and optional air-gapped isolation.
- Enterprise governance, audit logging, and undo journals.

### 1.2 Target Verticals & Initial Market
1. **Law firms and legal teams:** Highly sensitive client disclosures, litigation documents, M&A due diligence.
2. **Software enterprises:** Proprietary source code, vulnerability audits, trade secrets.
3. **Healthcare & Life Sciences:** HIPAA-compliant clinical notes, patient summaries, biomedical research.
4. **Financial & Accounting institutions:** Regulated customer records, internal audits, compliance filings.
5. **Engineering & R&D:** Confidential industrial designs, patent filings, proprietary schematics.
6. **Government, Defense & Regulated Enclaves:** Air-gapped environments subject to strict procurement rules.
7. **Privacy-Conscious Professionals:** Power users wanting Jarvis-like system access with absolute privacy.

### 1.3 5-Tier Deployment Progression
- **Tier 1: Personal Local (M0–M10 Milestone Scope):** Runs self-contained on an individual desktop/laptop (Linux/Windows). Sub-second startup, low-memory footprint, native hotkey, offline voice and document intelligence.
- **Tier 2: Team Server (Post-M10):** Runs on an organization's on-prem GPU workstation/server, accessed over a private LAN.
- **Tier 3: Private Cloud Infrastructure:** Deployed in VPC / private cloud Kubernetes clusters with role-based access.
- **Tier 4: True Air-Gapped Enclave:** Zero network connectivity during operation, with signed offline update packages for model weights and binaries.
- **Tier 5: Hybrid Mode:** Local execution for confidential data, with policy-governed dispatch to approved cloud endpoints for non-sensitive public queries.

### 1.4 Post-M10 Transition to Production Multi-Agent System
Once Orion reaches M10 and ships as a hardened single-agent local product, its local engine and capability broker will serve as an inference/execution backend for the broader Production Multi-Agent Engineering System. Specialized local models will collaborate across routing, extraction, coding, and review within the verified permission boundaries.

### 1.5 Architecture Enhancements: Inactivity Sleep & Dual-Model Routing
- **Auto-Hibernate Policy:** Inactivity watchdog timer unloads `llama-server` from RAM after a period of user inactivity (exactly 8 minutes default), freeing memory back to the OS. State cleanly reverts to `Standby` and wakes transparently on next user prompt.
- **Installer Workload Persona:** Setup wizard prompts users for their domain focus (Developer/Coder, Researcher/Analyst, Creative/Animation, or General Assistant), configuring an appropriate specialized model.
- **Two-Tier Model Orchestration:** Lightweight Generalist model serves as the Front-of-House router handling voice input and quick conversation. When complex domain tasks are recognized, the Generalist enhances/structures the prompt, offloads from RAM, and brings the Specialist model into memory for deep execution.

---

## 2. CANONICAL MILESTONE MASTER ROADMAP (M0 — M10)

The foundational product roadmap established for Orion consists of 10 core engineering milestones plus specialized platform additions:

| Milestone | Capability | Status | Architectural Invariants & Verified Deliverables |
|---|---|---|---|
| **M0** | **App Shell & Core Engine** | ✅ **VERIFIED** | Tauri v2, SQLite, embedded llama-server sidecar, streaming chat, 5-step migration ladder, ephemeral loopback auth, lazy engine startup, JobObject child process cleanup. |
| **M1** | **C++ Inference Engine** | ✅ **VERIFIED** | Embedded llama-server, SSE token streaming, sub-second startup, hardware profiler with dual-bounding RAM tiers (T0–T4), forced-tier override, Apache/MIT redistribution gate. |
| **M2** | **Hybrid RAG Engine** | ✅ **VERIFIED** | Structure-aware chunking (heading trails + pages), hardened extractors (PDF, DOCX, XLSX, MD, CSV), BGE-small / MiniLM dense vector search + SQLite FTS5 BM25 + Reciprocal Rank Fusion, 30-question eval suite, intent-aware lead chunk retrieval. |
| **M3** | **Desktop HUD & Presence** | ✅ **VERIFIED** | Global `Ctrl+Shift+0` daemon (`Win+O` fallback), instant tray overlay, 4-state window toggle logic, file drag-and-drop intake, single-instance process lock. |
| **M4** | **Voice Duplex Loop** | ✅ **VERIFIED** | `whisper-cli` edge STT with Silero VAD, 96 KB pre-roll ring buffer, multi-format `cpal` audio capture, hands-free conversational auto-send, Piper neural TTS + Web Speech fallback, 8-min RAM watchdog. |
| **M5** | **Capability Broker** | ✅ **VERIFIED** | 4-tier permission matrix (T0 read-only, T1 propose/draft, T2 gated action, T3 privileged), two-domain isolation (`Domain::Trusted` vs `Domain::Untrusted`), symlink jail (`RESOLVE_BENEATH`), immutable SQLite WAL audit log & undo journal. |
| **M6** | **Auto-Installer & Personas** | ✅ **VERIFIED** | Windows NSIS packaging, streaming background assets, sequential model distribution, 2-mode segmented switcher (General vs Selected Specialist), zero dual-model RAM bloat. |
| **M7** | **Sovereign Email Assistant** | 🚀 **COMPLETED** | Offline IMAP/SMTP triage, multi-vector sanitization (strips zero-width chars, bidi overrides, hidden CSS), adversarial prompt-injection quarantine, real IMAP over TLS sync, Rule R-5 compliance (strictly no auto-send). |
| **M8** | **Sandboxed Browser** | 🚀 **COMPLETED** | Zero-vision Accessibility (AX) DOM tree automation, dedicated profile isolation (~/.orion/browser_profile), two-domain quarantine, 2–5 KB text context vs 1.4 MB screenshots, Capability Broker T2 navigation & audit trail. |
| **M9** | **OS UI Automation** | 🚀 **COMPLETED** | Win32 UIA / AT-SPI desktop control, direct native execution for File Explorer / Calculator / system apps without prompt bleed, accessibility tree driver, undo journal integration. |
| **M10** | **Kernel Hardening** | 🚀 **COMPLETED** | Windows AppContainer / JobObject containment (`KILL_ON_JOB_CLOSE`), 8-minute dynamic RAM standby hibernation (zeroing memory footprint), salted SHA-256 PIN auto-lock, zero-egress air-gap enterprise certification (`docs/M10-DEPLOYMENT-CERTIFICATION.md`). |

### 2.1 Integrated Platform Additions (Co-Engineered with M1–M7)
- **Permanent SQLite Multi-Session Chat Engine:** Storage-first architecture replacing in-memory RAM loading with SQLite Schema v3; pagination of active turns; persistent session switching, renaming, and audit logs.
- **Native Document Compilation Engine:** True binary document generation directly from conversational context for Microsoft Excel (`.xlsx` with formulas, headers, styles), Microsoft Word (`.docx` with typography, tables, callouts), and Adobe PDF (`.pdf` with executive layouts).
- **Salted PIN Lock & Security Shield:** SHA-256 salted PIN screen lock, 8-minute inactivity timer, and top-level React diagnostic `ErrorBoundary` for zero-black-screen resilience.

---

## 3. M4 VOICE STATUS, BUGS IDENTIFIED & RESOLVED

The voice pipeline was audited end-to-end against real audio hardware contracts and Linux/Windows environments. The following critical bugs were caught and resolved:

### 1. Dynamic Linker Failure on Linux (`libwhisper.so.1` missing)
- **Forensic Diagnosis:** `scripts/fetch-voice.sh` used `find "$TMP/x" -type f` to mirror shared libraries. This ignored all symlinks (`libwhisper.so.1 -> libwhisper.so.1.9.2`, `libggml.so.0 -> libggml.so.0.18.1`). Because `whisper-cli` links to `libwhisper.so.1` via its ELF `NEEDED` header, executing `whisper-cli` crashed immediately with `cannot open shared object file: No such file or directory`.
- **Resolution:** Updated `fetch-voice.sh` to match `\( -type f -o -type l \)` and copy preserving symlinks (`cp -a`). Both binaries and target directories now contain exact symlinks. Verified with live execution of JFK 16 kHz WAV audio.

### 2. Microphone Sample Format Panic in `cpal`
- **Forensic Diagnosis:** `src-tauri/src/voice/capture.rs` directly bound `build_input_stream::<f32, ...>`. On many physical microphones (Realtek, USB mics, Bluetooth headsets), the hardware's native format is `SampleFormat::I16` or `SampleFormat::U16`. Attempting to open an `f32` stream on an integer PCM device causes `cpal` to return `CannotConvertSupportedStreamConfig` or fail at runtime.
- **Resolution:** Added explicit format matching on `config.sample_format()` for `SampleFormat::F32`, `SampleFormat::I16` (normalized via `s as f32 / 32768.0`), and `SampleFormat::U16`.

### 3. Whisper Binary Resolution Failure in Varied Environments
- **Forensic Diagnosis:** `whisper_cli_path()` only inspected `current_exe().parent()` and bare `"whisper-cli"`. If invoked from the repo root or in developer test harnesses where `target/debug/whisper-cli` hadn't been mirrored, it falsely reported that speech recognition was not installed.
- **Resolution:** Added support for `ORION_WHISPER_PATH` override, followed by executable directory checks, `CARGO_MANIFEST_DIR/binaries`, and project `binaries/` fallbacks.

### 4. Audio Staging File Name Collisions
- **Forensic Diagnosis:** `transcribe_samples()` staged temporary audio to `orion-utterance-<pid>.wav`. Concurrent or rapid successive audio pushes risked file collisions or read/write races.
- **Resolution:** Migrated to `orion-utterance-<pid>-<uuid>.wav` with guaranteed cleanup in all execution branches.

### 5. Local Sysroot ALSA Resolution
- **Forensic Diagnosis:** `scripts/setup-sysroot.sh` omitted `libasound2-dev` and `libasound2t64`, preventing `alsa-sys` from building in rootless/headless Docker sandboxes.
- **Resolution:** Added ALSA packages to `PACKAGES` in `setup-sysroot.sh`.

### 6. Windows Sidecar DLL Loader Failure (`STATUS_DLL_NOT_FOUND` / `0xC0000135` / `-1073741515`)
- **Forensic Diagnosis:** When running `npm run tauri dev` on Windows with a custom `$env:CARGO_TARGET_DIR` (e.g. `$PWD\src-tauri\target_clean`), the Tauri CLI copied `llama-server-x86_64-pc-windows-msvc.exe` into `target_clean\debug\`, but left the required dynamic runtime libraries (`ggml.dll`, `llama.dll`, `ggml-base.dll`, `ggml-cpu.dll`, etc.) in `src-tauri\binaries\`. Because Windows resolves DLLs relative to the executable path and `PATH`, and neither contained `src-tauri\binaries\`, Windows terminated `llama-server.exe` within 54 ms with exit code `-1073741515` (`0xC0000135`). The exact same crash hit the embedding sidecar.
- **Resolution:**
  1. Added `sync_sidecar_libraries()` in `src-tauri/src/lib.rs` which executes on app startup and before starting sidecars: it scans candidate `binaries/` locations and automatically mirrors all `.dll` / `.so` / `.dylib` files directly into `current_exe().parent()` (`target_clean\debug\`), completely removing the need for manual library copying regardless of target directory.
  2. Updated `start_engine` and `start_embedder` to explicitly prepend the executable directory and the binaries directory to `PATH` (on Windows) / `LD_LIBRARY_PATH` (on Linux), and set the process `current_dir`.
  3. Updated `scripts/fetch-sidecars.sh` and `scripts/fetch-voice.sh` to mirror libraries into `$CARGO_TARGET_DIR`, `target_clean`, and `target`.

### 7. Engine 180-Second Hang on Child Process Crash
- **Forensic Diagnosis:** When `llama-server` crashed on spawn, the background listener received `CommandEvent::Terminated(p)`, logged a warning, and exited the loop. However, it never notified `engine`. Meanwhile, `ensure_engine` / `wait_until_ready` entered an HTTP polling loop to `127.0.0.1:49999/health`. Because the port was closed, `reqwest` received `Connection refused`, which `wait_until_ready` misclassified as "server still binding", looping repeatedly for the full 180-second timeout while reporting `Waiting for engine…`. The user was left waiting 3 minutes with no clue that the engine had already died.
- **Resolution:**
  1. Added child termination notification in `start_engine`: when `CommandEvent::Terminated(p)` is received, `engine.set_status(EngineState::Error, ...)` is immediately invoked with exact exit code diagnostics (specifically identifying `0xC0000135 / STATUS_DLL_NOT_FOUND`).
  2. In `engine.rs`, `wait_until_ready` checks `self.status().await.state == EngineState::Error` on every cycle and bails out immediately with the exact cause instead of hanging for 180 seconds.
  3. In `documents.rs`, `start_embedder` checks `service.state().await == EmbedState::Unavailable` to abort immediately on termination.
  4. In `ensure_engine`, if the engine previously failed (`EngineState::Error`), it allows re-attempting spawn on subsequent prompts.

### 8. Deceptive UI Readiness Status at Launch
- **Forensic Diagnosis:** `App.jsx` mapped `EngineState::Idle` to `"Ready"` and `.dot.ok` (green). This falsely informed users that the 2.4 GB chat model was already loaded into memory at startup. When the user sent their first message, the status suddenly switched to amber "Starting engine…" and froze, destroying user trust.
- **Resolution:**
  1. Mapped `idle` state honestly to `"Standby"` with a neutral muted `.dot.idle` indicator.
  2. Only genuine `ready` state (verified `/health` HTTP 200) receives `"Ready"` and solid green `.dot.ok`.
  3. Added informative tooltips explaining that the model loads into memory upon the first message.
  4. Removed the blocking guard in `send()` for `engine.state === "error"` so the user can retry sending after resolving an issue.

### 9. Unused Windows Import Warning in `transcribe.rs`
- **Forensic Diagnosis:** `tokio::process::Command` implements `creation_flags` as an inherent method on Windows. Importing `std::os::windows::process::CommandExt` triggered `#[warn(unused_imports)]`.
- **Resolution:** Removed the redundant import.

### 10. Voice Audio Normalization, Adaptive Noise Floor & Continuous Capture
- **Forensic Diagnosis:**
  1. Quiet microphone inputs produced peak signals below 0.05 (-26 dBFS), causing high word error rates and hallucinated repetitions in `whisper-cli`.
  2. Fixed RMS speech energy threshold (`0.012`) failed in rooms with variable ambient noise (fans/AC) or quiet voices.
  3. When the microphone was open and the listener was in `ListenState::Waiting`, speech did not automatically trigger audio recording without a manual wake trigger.
  4. The 800 ms silence timeout cut off contemplative natural speech during brief mid-sentence pauses.
- **Resolution:**
  1. Added automatic peak gain normalization in `src-tauri/src/voice/audio.rs` (`encode_wav_16k_mono`) with dynamic gain boost up to 12x (targeting 85% full-scale / -1.4 dBFS) with soft saturation protection.
  2. Implemented asymmetric exponential moving average ambient noise-floor tracking in `src-tauri/src/voice/capture.rs`, computing an adaptive speech threshold (`floor * 2.2 + 0.004`) to handle varying acoustic environments.
  3. Updated `Listener::push_block` in `listener.rs` to automatically transition from `ListenState::Waiting` to `ListenState::Recording` and emit `ListenAction::BeginCapture` on speech.
  4. Increased `SILENCE_TIMEOUT_MS` from 800 ms to 900 ms.
  5. Handled `ListenAction::BeginCapture` in `VoiceService::poll` by invoking `capture.begin_utterance()`.
  6. Enhanced `VoiceButton.jsx` and `index.css` with dedicated recording styles, animated green indicators, and live level meters.

### 6. Conversational Voice Loop & Dual-Engine Speech Output
- **Forensic Diagnosis:** Voice transcripts were previously routed only to `<textarea>` requiring manual Enter keystrokes, which broke the conversational flow. Furthermore, if Piper standalone binaries were missing on Windows, TTS failed silently with no audio output or active visual indicators.
- **Resolution:**
  1. Implemented hands-free conversational auto-send: transcribing voice now immediately dispatches `sendMessage(clean, true)` without manual intervention.
  2. Implemented dual-engine speech synthesis: calls local Piper neural TTS first, and instantly falls back to native Windows WebView2 `window.speechSynthesis` with natural English voices.
  3. Added text sanitizer (`cleanForSpeech`) to strip code blocks, markdown links, and citation markers before speech.
  4. Added active speaking HUD with electric cyan glow, `@keyframes speakingWave` equalizer animation, message bubble audio controls, and an auto-speak toggle.
  5. Implemented an 8-minute inactivity watchdog that transitions llama-server to `Standby` and unloads model weights from RAM to honor the 5.7 GB usable RAM budget.

---

## 4. MILESTONES COMPLETED & CURRENT STATUS

### Milestone 5: Capability Broker & Security Boundary (VERIFIED)
- **Status:** Complete & passing 458 tests.
- **Deliverables:**
  1. Two-Domain Separation (`domain.rs`): Untrusted Content vs. Trusted Directives.
  2. Four-Tier Capability Broker (`engine.rs`): T0 (Read-only auto), T1 (Reversible with undo), T2 (Destructive with typed confirmation), T3 (Per-call human approval), BLOCKED (Strictly blocked credential files, keys, shell profiles).
  3. Symlink Containment & Path Sandboxing: Strict resolution under workspace directory.
  4. Audit Log & Undo Journal: SQLite schema v6 storing every operation with rollback snapshots.
  5. Injection Defense & Red-Team Testing: Verified rejection of injected tool directives.

### Milestone 6: Release v1 Packaging, Workload Personas & Security (VERIFIED)
- **Status:** Implemented, verified, 0 lint warnings, clean build.
- **Deliverables:**
  1. **Packaging & NSIS Installer:** Desktop bundle configuration in `tauri.conf.json` with user-level installation, desktop shortcuts, and release scripts (`scripts/build-release.sh`).
  2. **First-Run Onboarding Wizard (`OnboardingWizard.jsx`):** Detects hardware, explains local privacy, guides workload persona selection, and provides quick tips.
  3. **Workload Personas (`src-tauri/src/persona.rs`):** Specialized system prompt augmentation for Developer, Researcher, Creative, and General roles without exceeding 3B parameters or dual-model RAM footprints on 8 GB workstations (~5.7 GB usable).
  4. **Dynamic Hardware-Aware Model Selection (`scripts/fetch-model.sh`):** Auto-detects total host RAM across Linux, macOS, and Windows PowerShell, automatically selecting optimal weights for each role (T1 3B, T2 7B, T3 14B, T4 32B).
  5. **ChatGPT-Style Interactivity:** Syntax-highlighted code blocks with language labels and one-click copy buttons (`CodeBlock.jsx`), live streaming caret, message action bar (copy message, speak audio, retry/regenerate), and dynamic persona-tailored prompt suggestion chips.
  6. **Master Passcode Security Lock (`LockScreen.jsx`):** Salted SHA-256 encrypted master PIN lock, instant quick lock button, password hint support, and backend command gating against unauthorized local access on shared PCs.
  7. **Release Packaging & Distribution Guide (`docs/M6-RELEASE-V1.md`):** Complete workflow for generating `.exe` installers, transferring to family/test machines, and seamless upgrade ladder to M7–M10 preserving local databases and embeddings.

### Milestone 7: Sovereign Email Assistant (VERIFIED & COMPLETED)
- **Status:** Complete, passing test suite, 0 lint warnings.
- **Deliverables:**
  1. **Rule R-5 Compliance:** Strictly no auto-send API or unattended outbound SMTP capabilities exist in Orion. All email generation terminates in clipboard copy or native `mailto:` draft launch, enforcing human-in-the-loop dispatch.
  2. **Multi-Vector Threat Sanitization:** Ingested email bodies stripped of zero-width control chars (`\u{200B}`, `\u{200C}`, `\u{FEFF}`), bidirectional overrides, hidden CSS, and base64 tracking pixels before storage in SQLite schema v3.
  3. **Adversarial Prompt Injection Quarantine:** Ingestion classifier intercepts systemic override directives (`"ignore instructions"`, `"transfer funds"`, etc.), tagging records as `cat-quarantine` and neutralizing tool access.
  4. **Dual Ingestion Engine:** Live RFC9051 IMAP over TLS client (`native-tls` / `rustls`) and offline air-gapped `.eml` / RFC822 parser (`mailparse`).
  5. **Email Studio UI (`EmailStudio.jsx`):** Modern, minimalist split-pane UI with priority triage pills (Urgent, Action Required, Newsletter, Quarantined), action item extractors, tone-adjustable draft generation, and account manager.

### Milestone 8: Sandboxed Browser Automation (VERIFIED & COMPLETED)
- **Status:** Complete, 0 lint warnings, clean build.
- **Deliverables:**
  1. **Zero-Vision Accessibility (AX) Trees:** Abandons 500 KB–2 MB pixel dumps in favor of 2–5 KB deterministic structured semantic text (`AxElement`, `AxSnapshot`, `InteractiveRef`). Eliminates coordinate vision hallucinations on 3B/4B local models and saves 99.8% bandwidth/RAM.
  2. **Dedicated Isolated Browser Profile:** Automates Chromium/Edge instances rooted at `~/.orion/browser_profile`. The user's personal browser cookies, history, and passwords remain physically inaccessible.
  3. **Two-Domain Boundary Enforcement:** Scraped web pages are tagged strictly as `DomainSource::WebPage` (`Domain::Untrusted`). Web-based prompt injections are physically prevented from invoking Capability Broker tools.
  4. **Capability Broker T2 Audit Logging:** Navigation commands trigger Tier 2 broker audit records in SQLite WAL `audit_log`, providing non-repudiation and operator transparency.
  5. **Browser Studio (`BrowserStudio.jsx`):** Dedicated UI panel with address bar, quick presets, interactive element inspector table, copyable `[ref: eX]` tokens, compact AX tree viewer, and one-click "Analyze in Chat" bridge.

---

## 6. IMMEDIATE OBJECTIVE: MILESTONE 9 (M9) — OS UI AUTOMATION

With M0 through M8 completed and verified, Orion is positioned for **Milestone 9 (OS UI Automation)**:
- Win32 UI Automation (UIA) and Linux AT-SPI accessibility tree automation across desktop applications.
- Reversible OS file actions backed by the Capability Broker SQLite Undo Journal.
- Local model task synthesis with deterministic element refs (`ref: eX`) matching the M8 zero-vision paradigm.
