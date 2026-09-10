# Security Model

Orion is designed on one assumption: **the language model will be wrong, and will eventually
be hijacked by text hidden in a document, a web page, or an email.** The system must hold
anyway.

This is not hypothetical. Through 2026 this exact failure mode produced remote code execution
in Google Antigravity, CrewAI (CVE-2026-2275 and three siblings), Cursor (CVE-2026-26268),
and Claude Cowork (CVE-2026-46331). Every one of those products shipped a "safe mode." In
every case the payload reached the OS through a path the safe mode did not classify as
dangerous.

**The lesson: sanitisation is not a boundary. Isolation is.**

---

## 1. The core rule — two domains that never touch

```
  UNTRUSTED DOMAIN                       TRUSTED DOMAIN
  documents · email bodies               you typed it · you said it
  web pages · file contents                        │
        │                                          ▼
        ▼                                ┌───────────────────┐
   chat / answers                        │ CAPABILITY BROKER │
   NO TOOL ACCESS  ─────────✗──────────► │  tiers · audit    │
                                         │  undo · sandbox   │
                                         └─────────┬─────────┘
                                                   ▼
                                              tool executes
```

Content Orion **reads** can never issue instructions. Only content the user **authored** can.

Where the domains must meet — "read this invoice and rename the file" — the crossing is a
single **structured value**, extracted and displayed for confirmation. Never free text
flowing into the agent's instruction stream.

---

## 2. Capability tiers

Enforced in Rust, in the broker. Never in a prompt.

| Tier | Examples | Gate |
|---|---|---|
| **T0 read** | list directory, read allowlisted file, page snapshot, read mail | automatic |
| **T1 reversible** | create/edit in workspace, draft a reply, open an app | notify + undo journal |
| **T2 destructive** | delete, mass rename, overwrite, install | typed confirmation |
| **T3 shell / network** | run a command, browser writes, send email | per-call approval + OS sandbox |
| **BLOCKED** | Orion's own config, allowlist, keys; `~/.ssh`, `.aws`, `.env`; git hooks; shell rc; startup folders | impossible |

That last row is what most vendors got wrong. **Orion has no write path to its own
permissions.** The CSA calls this Configuration-Based Sandbox Escape; in Antigravity it
survived uninstall and reinstall.

---

## 3. Implementation rules

- **No shell string interpolation, ever.** `argv` arrays only, always `--`-terminated. The
  Antigravity RCE was flag injection (`-X`) through a parameter never meant to carry flags.
- **Path containment resolves symlinks first, then checks.** Naive `starts_with()` loses to
  `../` and symlinks.
- **Deny by default.** An allowlist of permitted tools, not a blocklist of dangerous commands.
  Denylists fall to trivial obfuscation (MS-Agent CVE-2026-2256, CVSS 9.8).
- **Fail closed.** If the sandbox cannot start, the tool refuses. CrewAI's worst CVE was a
  *silent fallback* to an unsafe sandbox when Docker was unavailable.
- **Dry-run first** for any multi-step plan: show it, then execute.
- **Append-only audit log** the agent cannot write to, plus a full undo journal.
- **No network tool by default.**
- **Third-party MCP servers are not loaded by default** — a March 2026 audit found command
  injection in 43% of MCP v2.0 implementations.

---

## 4. Current posture (M0)

Implemented today:

- `llama-server` binds `127.0.0.1` on an **ephemeral port** with a **random 48-char bearer
  token** generated per session. Nothing is reachable off-machine.
- The **webview never receives** the port, the token, or filesystem paths. All privileged
  work happens behind explicit Tauri commands.
- **Narrow capability manifest** — no filesystem, HTTP, or arbitrary shell permission is
  granted to the frontend.
- **Strict CSP**, no remote script/style origins.
- CI blocks committed credentials and model weights.

Not yet built (by design — the broker lands in M5, before any tool can write):

- Capability broker, audit log, undo journal, OS sandboxing, document/action separation
  enforcement.

**No action or tool capability ships before M5.**

---

## 5. Threats specific to planned features

**Email (M7).** An inbox is an unauthenticated input channel from the entire internet.
Bodies enter the untrusted domain only. HTML, hidden text, white-on-white and zero-width
characters are stripped before the model sees anything. Links are never auto-followed;
remote images are never fetched. Attachments are quarantined. **Orion drafts replies; the
user sends them.** There is no auto-send path.

**Browser (M8).** Runs in a dedicated Orion profile — never the user's logged-in one, so
session cookies and banking tabs are out of reach. Domain allowlist, off by default. Page
content cannot trigger a new tool call. No credential field is ever auto-filled.

**Voice (M4).** Speech from the user's microphone is trusted input. Audio decoded from a
file or video is untrusted and cannot reach the action domain.

**Model weights.** Only permissively licensed models (Apache-2.0 / MIT) are bundled.
Downloads are checksum-verified.

---

## 6. Reporting a vulnerability

Orion is pre-alpha and unreleased; there are no users to protect yet. Once released, please
report privately via GitHub Security Advisories on
[the repository](https://github.com/kuldeepyadav001/Orion) rather than opening a public issue.

---

## 7. What Orion does not protect against

Stated plainly, because overclaiming is its own security problem:

- A **compromised host**. If malware is already running as your user, Orion offers nothing.
- **Malicious model weights** you supply yourself.
- **Your own instructions.** If you tell Orion to delete something and confirm it, it will.
- **Physical access** to an unlocked machine.
