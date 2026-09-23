# ORION — MILESTONE 6: RELEASE V1 PACKAGING & ONBOARDING

**Specification Version:** 1.0  
**Status:** Implemented & Verified  
**Date:** 2026-09-23  

---

## 1. Executive Summary: The Release Gate

Milestone 6 marks the transition of Orion from an internal developer prototype to a **shippable, standalone desktop application**. 

In accordance with Section 8 of the Master Architecture Plan:
> *"Release at M6, not M10. Get real users on the safe half while the dangerous half matures."*

Milestone 6 integrates:
1. **Interactive First-Run Onboarding Wizard (`OnboardingWizard.jsx`):** Guides first-time users through local hardware detection, privacy confirmation, and persona selection.
2. **Workload Persona Specialization Engine (`src-tauri/src/persona.rs`):** Dynamically shapes the assistant's reasoning depth, tone, and syntax rules for four core professions without violating memory constraints.
3. **Strict 5.7 GB Usable RAM Invariant:** Constrains default models to $\le 3$B parameter quantized GGUF architectures (`Qwen2.5-3B` and `Qwen2.5-Coder-3B`), ensuring single-model sequential loading that never freezes or pages on 8 GB workstations.
4. **Desktop Packaging & Installers:** NSIS Windows installer (`.exe`) and Linux packages (`.deb`, `.AppImage`) configured in `tauri.conf.json` with desktop shortcuts, icons, and automated uninstallation.

---

## 2. Workload Personas (`src-tauri/src/persona.rs`)

A 3B parameter model operating on a local CPU has finite capacity. Personas resolve this by injecting targeted domain framing directly into the system prompt:

| Persona | Title | Icon | Focus & Output Behavior | Recommended Model |
| :--- | :--- | :---: | :--- | :--- |
| **`developer`** | Software Developer | 💻 | Clean, idiomatic, typed code with minimal boilerplate. Explains algorithmic complexity and edge cases. | `Qwen2.5-Coder-3B-Instruct-GGUF` |
| **`researcher`** | Researcher & Analyst | 🔬 | Analytical rigor, structured evidence, multi-document synthesis, and explicit citation attribution. | `Qwen2.5-3B-Instruct-GGUF` |
| **`creative`** | Creative Writer | 🎨 | Vivid sensory details, expressive dialogue, dynamic narrative pacing, and creative worldbuilding. | `Qwen2.5-3B-Instruct-GGUF` |
| **`general`** | General Assistant | ⚡ | Direct, concise, balanced everyday intelligence for fast conversational turnaround (Default). | `Qwen2.5-3B-Instruct-GGUF` |

* **Dynamic Prompt Enhancer:** `persona.enhance_prompt(base_prompt)` automatically attaches the persona's operational guidelines to both general chat and grounded RAG sessions.
* **Persistent Settings:** Active persona preference is saved to SQLite `settings` table (`key = "active_persona"`).
* **Live Switching:** Users can switch personas at any time from the sidebar badge or the System Panel with immediate effect.

---

## 3. First-Run Onboarding Flow (`src/OnboardingWizard.jsx`)

When Orion is launched on a fresh machine (where `onboarding_completed` is not set in SQLite):

1. **Step 1: Hardware Profiling & Privacy Guarantee**
   * Displays local RAM, CPU cores, and GPU status.
   * Explains that 100% of computation is offline with zero telemetry.
2. **Step 2: Persona Selection**
   * Displays 4 interactive cards allowing the user to select their primary professional workflow.
3. **Step 3: Quick Start Guide**
   * Highlights the global hotkey (`Ctrl+Shift+0`), voice talking mode, drag-and-drop document intake, and the M5 Capability Broker confirmation tickets.
4. **Completion:**
   * Invokes `complete_onboarding`, records `onboarding_completed = "true"`, and enters the main chat interface.

---

## 4. Packaging & Distribution Architecture

### Windows Installer (NSIS)
Configured in `src-tauri/tauri.conf.json`:
```json
"bundle": {
  "active": true,
  "targets": ["deb", "appimage", "nsis"],
  "windows": {
    "nsis": {
      "installMode": "currentUser",
      "installerIcon": "icons/icon.ico",
      "headerImage": "icons/128x128.png",
      "sidebarImage": "icons/128x128.png",
      "displayLanguageSelector": false
    }
  }
}
```

### Packaging Script (`scripts/build-release.sh`)
Builds production-ready packages:
```powershell
./scripts/build-release.sh
```
Outputs are generated in `src-tauri/target/release/bundle/`:
* **Windows:** `nsis/Orion_0.1.0_x64-setup.exe` and `msi/Orion_0.1.0_x64_en-US.msi`
* **Linux:** `deb/orion_0.1.0_amd64.deb` and `appimage/Orion_0.1.0_amd64.AppImage`

---

## 5. Dynamic Hardware-Aware Model Selection (`scripts/fetch-model.sh`)

The model download system dynamically interrogates the host machine's physical RAM across Linux (`/proc/meminfo`), macOS (`sysctl`), and Windows PowerShell (`TotalPhysicalMemory`) and automatically maps to the optimal tier:

| RAM Detected | Tier Assigned | Coder Role Model | Researcher Role Model | General/Creative Role Model |
| :--- | :---: | :--- | :--- | :--- |
| **$\le 10$ GB** | Tier 1 (3B) | `Qwen2.5-Coder-3B-Instruct` | `DeepSeek-R1-Distill-Qwen-1.5B` | `Qwen2.5-3B-Instruct` |
| **11 – 20 GB** | Tier 2 (7B) | `Qwen2.5-Coder-7B-Instruct` | `DeepSeek-R1-Distill-Qwen-7B` | `Qwen2.5-7B-Instruct` |
| **21 – 48 GB** | Tier 3 (14B) | `Qwen2.5-Coder-14B-Instruct` | `DeepSeek-R1-Distill-Qwen-14B` | `Qwen2.5-14B-Instruct` |
| **$> 48$ GB** | Tier 4 (32B) | `Qwen2.5-Coder-32B-Instruct` | `DeepSeek-R1-Distill-Qwen-32B` | `Qwen2.5-32B-Instruct` |

* **Single-Model Sequential Loading:** Prevents out-of-memory lockups on 8 GB systems (~5.7 GB usable after iGPU reservation).

---

## 6. ChatGPT-Style Chat Interactivity

1. **Interactive Markdown Code Blocks (`src/CodeBlock.jsx`):**
   * Language badge (e.g. `javascript`, `python`, `rust`, `bash`).
   * One-click "Copy code" button that transitions into "Copied!" with a checkmark for 2 seconds.
   * Monospace font formatting with clean horizontal scrolling.
2. **Dynamic Persona-Tailored Suggestion Cards:**
   * Automatically adapts to the active workload persona (Developer, Researcher, Creative, General).
   * 4 customized action prompts per persona on the empty state.
3. **Dynamic Follow-Up Suggestion Chips:**
   * Contextual follow-up chips shown above the composer in active chat threads.
4. **Message Action Bar:**
   * One-click "Copy response" for any assistant message.
   * "Speak / Stop" audio playback with live visual pulsing indicator.
   * "Retry / Regenerate" button to re-run the previous turn.
5. **Streaming Cursor:**
   * Smooth blinking vertical caret during token generation.

---

## 7. Master Passcode Security Lock (`src/LockScreen.jsx`)

Orion can be tightly locked to prevent unauthorized physical access on shared or test machines:
* **Salted SHA-256 Hashing:** Passcode is securely hashed with unique salt and stored in SQLite settings (`lock_hash`, `lock_enabled`).
* **Instant Lock:** "Lock Now" trigger locks the interface instantly when stepping away.
* **Full-Screen Shield:** Hides chat thread, library documents, and inputs behind a frosted glass security barrier until the master PIN is verified.
* **Backend Security Gate:** `send_message` IPC command rejects requests if the lock state is engaged.
* **Password Hint Support:** Optional security hint for forgotten codes.

---

## 8. Distribution & Testing Guide (e.g., Brother's PC)

### How to Build the Installer
On your build machine, run:
```powershell
npm run tauri build
# Or use the helper script:
./scripts/build-release.sh
```
The Windows installer `.exe` is generated at:
```
src-tauri/target/release/bundle/nsis/Orion_0.1.0_x64-setup.exe
```

### Transferring to Another PC
1. Copy `Orion_0.1.0_x64-setup.exe` to a USB flash drive, or send it via local network / cloud drive.
2. Run `Orion_0.1.0_x64-setup.exe` on your brother's computer. It installs into the user profile without needing Administrator rights (`installMode = "currentUser"`).
3. On first launch, Orion:
   * Detects your brother's RAM (if he has 16 GB, it recommends Tier 2; if 8 GB, Tier 1).
   * Launches the Onboarding Wizard to pick his primary persona.
   * Allows setting a Master Passcode if he shares his computer.

### Upgradability to M7–M10
* **Data Preservation:** User chats, library documents, vector embeddings, and master passcode settings reside in `%LOCALAPPDATA%\Orion\orion.db`.
* **Zero-Downtime Upgrades:** Future installers (v0.2.0 for M7, v0.3.0 for M8, up to M10) simply replace the binaries and run backwards-compatible SQLite migrations without deleting local data.

---

## 9. Master Roadmap Position

| Milestone | Capability | Status |
| :--- | :--- | :--- |
| **M0** | App Shell, Tauri v2, SQLite WAL, llama-server supervision | ✅ Verified |
| **M1** | Hardware Profiler & Tiered Model Manager (T0–T4) | ✅ Verified |
| **M2** | Document Intelligence & Hybrid RAG (BM25 + vectors) | ✅ Verified |
| **M3** | System Presence (tray, `Ctrl+Shift+0` hotkey, intake) | ✅ Verified |
| **M4** | Voice Loop (Whisper STT, Piper TTS / Web Speech, 8-min watchdog) | ✅ Verified |
| **M5** | Capability Broker & Security Boundary (Two-domain, Tiers, Audit, Undo) | ✅ Verified (458 tests passed) |
| **M6** | **Release v1 (Packaging, NSIS, Onboarding, Personas, Lock, GPT-Interactivity)** | ✅ **Implemented & Verified** |
| **M7** | Email Assistant (Read/Triage/Draft, IMAP, strictly no auto-send) | ⏳ Next Up |
| **M8** | Browser Automation (Playwright AX tree, dedicated profile) | ⏳ Queued |
| **M9** | Desktop Control & Scoped Writes (Accessibility tree automation) | ⏳ Queued |
| **M10** | Sandboxed Execution & GA Release (OS-level sandboxes) | ⏳ Queued |
