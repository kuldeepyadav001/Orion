# M3 — System Presence: evidence

Branch: `feat/m3-system-presence`, cut independently from `main` at `3784c8f`.
Status: **76 tests green, clippy `-D warnings` clean, frontend lints and builds.**
The Tauri layer is **not compile-verified** — see "The honest gap" below.

---

## What M3 adds

| Piece | File | Tested |
|---|---|---|
| Hotkey parsing, validation, fallbacks | `presence/hotkey.rs` | yes, 21 tests |
| File drop policy | `presence/filedrop.rs` | yes, 22 tests |
| Window state machine | `presence/window.rs` | yes, 17 tests |
| Cold-start budget | `presence/startup.rs` | yes, 16 tests |
| Tauri wiring | `presence/tauri_glue.rs` | **no — needs a desktop session** |
| Drop overlay UI | `src/DropZone.jsx` | lint + build only |

The split is deliberate. Everything decidable without a window handle is pure
and tested; `tauri_glue.rs` only translates those decisions into API calls. If
a future change adds an `if` to the glue, it belongs in a tested module
instead.

---

## The window state machine

`Win+O` has to do the right thing from four states, and the obvious
implementation is wrong in one of them:

| State | Action |
|---|---|
| Hidden | Show and focus |
| Visible and focused | Hide |
| **Visible, not focused** | **Focus — not hide** |
| Minimised | Restore and focus |

The third row is the one that ships broken in most tray apps. If the user is
in their browser and Orion is technically visible behind three windows,
"toggle visible" hides it — so the user presses the hotkey twice and sees a
flicker to get what one press should have done.

Explicit verbs (`Open Orion` in the tray, a second launch, a file drop) never
hide, whatever the state. A menu item labelled "Open" that sometimes closes
the window is indefensible.

Closing the window **hides to tray rather than quitting**, because quitting
would kill the global hotkey and break gate G5. Quit is in the tray menu, and
a test asserts it is reachable — otherwise the only way out is killing the
process.

---

## Cold start: the M3 gate

The gate is **under one second**. Phases carry individual budgets so a failure
names the culprit instead of leaving someone to guess:

| Phase | Budget |
|---|---|
| Runtime init | 250 ms |
| Database | 150 ms |
| Config | 100 ms |
| Tray and hotkey | 150 ms |
| Window show | 300 ms |
| **Sum** | **950 ms** (50 ms headroom under the gate) |

A test asserts the phase budgets sum to less than the gate. Without it, budgets
get raised one at a time until the gate is quietly abandoned.

### The decision that makes the gate achievable

**The model does not load during startup.** `llama-server` takes seconds to
memory-map a multi-gigabyte GGUF; blocking the window on that loses the gate by
an order of magnitude. The window appears, the engine warms behind it, and the
UI shows engine state so a user who types immediately sees "starting the
model…" rather than an input box that silently eats their message.

`DeferredWork` encodes this as data with a justification per item, and tests
assert `ModelLoad`, `EngineSpawn`, `IndexWarmup` and `UpdateCheck` are all on
it. A separate test asserts no startup phase is *named* after deferred work,
which is how this rule usually erodes.

Update checks are on the list for a second reason: network work on the startup
path would contradict the product's central offline-first claim.

**These budgets are unmeasured.** They are a design contract, not a
measurement — no Tauri app has been started. The first real number comes from
the M6 private build.

---

## File drop policy

Drop paths come from a file manager, a browser download, a chat client, or an
attacker who talked the user into dragging something. The renderer does **not**
decide what is acceptable: it passes paths to Rust, which applies policy and
returns a verdict. Letting the webview filter would mean an XSS or a
compromised npm dependency could feed arbitrary paths into ingestion.

| Rule | Behaviour |
|---|---|
| Symlinks | **Refused, never followed** |
| Unsupported type | Rejected, naming the extension |
| Empty file | Rejected |
| Over 64 MB | Rejected, stating the limit |
| Missing file | Rejected |
| Directories | Walked, max depth 8 |
| `node_modules`, `.git`, `target`, … | Skipped **silently** |
| Hidden files | Skipped **silently** |
| More than 200 files | Truncated, and the truncation is reported |

Two judgement calls worth naming:

**Symlinks are refused rather than resolved.** Following them lets a dropped
folder reach anywhere on disk — including places the user never intended to
share with an assistant that will quote the contents back.

**Noise is silent; failures are loud.** Reporting every `.DS_Store` buries the
one message that matters. But a file the user *meant* to add must never vanish
quietly — that is how someone ends up trusting an answer drawn from a document
Orion never read.

---

## Hotkey safety

A global hotkey is a system-wide capture. Binding it wrong makes the machine
feel broken in a way users will not attribute to us.

Refused: bare keys (capturing `O` means the user cannot type O anywhere),
Shift-only chords (`Shift+O` is just a capital O), and OS-reserved chords —
`Win+L`, `Win+D`, `Win+E`, `Win+R`, `Win+X`, `Win+I`, `Win+S`, `Win+A`,
`Win+P`, `Win+U`, `Win+V`, `Win+G`, `Alt+Tab`, `Alt+Esc`, `Ctrl+Alt+Tab`,
`Ctrl+Shift+Esc`.

Registration failure is treated as **normal, not exceptional** — another app
may already own `Win+O`. There is a fallback chain (`Win+O` → `Ctrl+Shift+O` →
`Ctrl+Alt+O` → `Ctrl+Shift+Space`), and if all fail the app still works from
the tray and logs an error. Silently registering nothing would leave the user
pressing a dead key they were promised in the UI.

---

## Bugs found by testing

1. **`Ctrl+Shift+E` was blocked; `Ctrl+Shift+Esc` was not.** The reserved-chord
   check matched the letter `E` instead of the Escape key — so it refused a
   perfectly good chord while missing the actual Task Manager shortcut. Caught
   by a test asserting both halves.
2. **`event.state()` would not have compiled.** `ShortcutEvent` is a re-export
   of `global_hotkey::GlobalHotKeyEvent`, where `state` is a public **field**,
   not a method. Found by reading the crate source, because this file cannot be
   compiled here. There is now a test pinning every accelerator token against
   the tokens `global-hotkey`'s parser actually accepts — a wrong token fails at
   *runtime registration*, not at compile time, so the hotkey would silently
   never fire.
3. **Branch independence was nearly broken.** Pulling M2's `ingest` module in
   for its `Format` enum would have dragged `zip`, `lopdf` and `quick-xml` into
   a branch about tray icons. `filedrop` now carries its own extension
   allowlist — which is the better design anyway: drop filtering is a *policy*
   question, parsing is a *parser* question, and they are free to diverge.
4. **Duplicated branch arms** in the window state machine (clippy).

---

## The honest gap

`tauri_glue.rs` **has never been compiled.** This sandbox has no
`libwebkit2gtk-4.1-dev` and no root to install it, so `cargo check` on the real
crate is impossible.

What was done instead:

- Parsed with `rustc` to rule out syntax errors (only unresolved-import errors
  remain, which is expected without dependencies).
- Every API verified by reading the vendored crate sources: `on_shortcut`
  signature, `ShortcutEvent`/`HotKeyState`, `TrayIconEvent::Click` fields,
  `MouseButtonState`, `Menu::with_items`, `show_menu_on_left_click`, and
  `tauri-plugin-single-instance::init`. Against tauri 2.11.5,
  tauri-plugin-global-shortcut 2.3.2, tauri-plugin-single-instance 2.4.4.
- One bug was found this way (#2 above). There may be others.

**Expect `tauri_glue.rs` to need fixes on first real build.** It is isolated
behind the `tauri-glue` feature so the tested logic below it compiles
independently. GitHub CI, which has the system libraries, is where this gets
its first real compile.

Also unverified: the tray icon rendering, whether `Win+O` actually registers on
Windows, real cold-start timings, and every drop interaction against a real
file manager.
