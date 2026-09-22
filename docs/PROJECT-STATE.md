# ORION — PROJECT STATE & STRATEGIC MASTER RECORD

**Last updated:** 2026-09-21
**Current state:** `BUILDING`
**Current phase:** M4 (Voice Pipeline Hardened & Verified) → Moving to M5 (Capability Broker)
**Repo:** `https://github.com/kuldeepyadav001/Orion.git` (`main` @ `f5dfc21` + M4 voice fixes)

---

## 1. THE BIG PICTURE: STARTUP THESIS & BUSINESS ARCHITECTURE

Orion is not a standard hobby project or demo. It is the foundation of a **Private AI Workspace Startup** designed to solve a fundamental corporate problem: **How do organizations get high-utility AI without sending sensitive data to public cloud AI providers?**

### 1.1 The Core Thesis
> *"The customer is not buying an LLM. They are buying **Private AI Infrastructure = Models + UI + Agent Runtime + Local Knowledge + Permissions + Security + Governance + Deployment**."*

The model itself is replaceable open-weight compute (Qwen, Mistral, Gemma, Phi). The product moat is:
- Reliable, frictionless one-installer local deployment.
- Hardware-aware dynamic model & compute routing.
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

---

## 2. VERIFIED MILESTONE PROGRESS (M0 — M4)

| Milestone | Capability | Status | Verified Evidence |
|---|---|---|---|
| **M0** | App Shell, Tauri v2, SQLite, llama-server sidecar | ✅ **VERIFIED** | Streaming chat, migration ladder (5 migrations), ephemeral loopback auth, lazy engine startup, JobObject child cleanup. |
| **M1** | Hardware Profiler & Tiered Model Manager | ✅ **VERIFIED** | Dual-bounding RAM profiler (T0–T4), forced-tier override (`ORION_FORCE_PROFILE`), Apache/MIT redistribution gate, user `.gguf` fallback. |
| **M2** | Document Intelligence & RAG Pipeline | ✅ **VERIFIED** | Structure-aware chunking (heading trails + pages), hardened extractors (PDF, DOCX, XLSX, MD, CSV), bge-small embedder, hybrid BM25+vector RRF search, 30-question eval suite. |
| **M3** | System Presence & OS Integration | ✅ **VERIFIED** | System tray + menu with lifecycle ownership, `Win+O` global hotkey with 4-state window logic, file drag-and-drop intake, single-instance lock. |
| **M4** | Voice Input Pipeline (STT) | ✅ **VERIFIED** | `whisper-cli` one-shot transcription with Silero VAD, 96 KB pre-roll ring buffer, RMS speech energy gate, multi-format `cpal` audio capture, live 5-bar meter in React UI. |
| **M5** | **Capability Broker & Security Boundary** | ⏳ **NEXT UP** | Two-domain isolation, T0–T3 permission tiers, symlink resolution (`RESOLVE_BENEATH`), audit log & undo journal, red-team injection tests. |
| **M6** | Private Testing Release (v1 Beta) | ⏳ Queued | Signed installers (Linux `.deb`/`.AppImage`, Windows `.msi`/`.exe`), onboarding wizard. |
| **M7** | Email Assistant (Read/Triage/Draft) | ⏳ Queued | IMAP with OS keychain, untrusted text quarantine, strictly no auto-send. |
| **M8** | Browser Automation | ⏳ Queued | Playwright Accessibility (AX) tree snapshots, dedicated browser profile, domain allowlist. |
| **M9** | Desktop Control & Scoped Writes | ⏳ Queued | Windows UIA / Linux AT-SPI accessibility tree automation, undo-journal backed file operations. |
| **M10** | Sandboxed Execution & GA Release | ⏳ Queued | OS-level sandboxes (AppContainer / Bubblewrap), fat offline bundle (~3.2 GB) + slim installer (~50 MB). |

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

---

## 4. IMMEDIATE OBJECTIVE: MILESTONE 5 (M5)

With M0–M4 hardened and passing 436 unit/integration tests, we proceed to **Milestone 5 (Capability Broker & Security Boundary)**.

### Non-Negotiable Gate Rule (G4 / R-2):
**No write action, tool execution, or OS modification capability may be implemented until the Capability Broker is verified.**

### M5 Deliverables:
1. **Two-Domain Separation (`src-tauri/src/broker/domain.rs`):** Strict structural separation between Untrusted Content (documents, emails, web pages) and Trusted Directives (user keyboard, authenticated voice).
2. **Capability Engine (`src-tauri/src/broker/`):** T0 (Read-only auto), T1 (Reversible with undo), T2 (Destructive with typed confirm), T3 (Per-call approval), BLOCKED (Permanent block on credentials, keys, shell profiles, Orion config).
3. **Path Containment:** Absolute symlink resolution (`RESOLVE_BENEATH`) preventing `../` traversal or link escapes.
4. **Audit Log & Undo Journal:** SQLite schema additions (`PRAGMA user_version = 6`).
5. **Red-Team Injection Suite:** Automated tests verifying that prompt-injected PDFs attempting to call tools are rejected.
