# Running Orion on your own machine

Read this first, because it will save you an hour of confusion:

**There is no installer yet, and no single downloadable file.** That is M10.
What exists today is source code that you build yourself, and it gives you a
chat window talking to a local model. Most of the headline features are not in
this build — see [What you will and will not get](#what-you-will-and-will-not-get).

**Build the `feat/m3-system-presence` branch.** Not `main`, which is the older
M0 skeleton. The branches are deliberately independent and have *not* been
merged, so no single branch has everything. M3 has the most complete app.

---

## What you will and will not get

Building `feat/m3-system-presence` gives you:

| Works | Notes |
|---|---|
| Chat with a local model | Streaming, fully offline once the model is downloaded |
| Chat history | Stored in SQLite on your machine |
| Tray icon | Left click toggles the window |
| Global hotkey | `Win+O` (`Ctrl+Shift+O` etc. as fallbacks) |
| File drop | Files are **assessed and reported**, but not yet indexed |
| Single instance | Launching twice focuses the existing window |

**Not in this build:**

- **Document Q&A.** The PDF/DOCX/XLSX pipeline is finished and tested, but it
  lives on `feat/m2-documents-rag` and has *not* been merged into M3. Dropping
  a PDF will tell you it was accepted and then do nothing with it. Merging M2
  and M3 is a real integration job — they both rewrote `lib.rs` — and it is
  not done.
- **Hardware-aware model picking** — that is M1, also unmerged.
- **Voice** (M4), **OS control** (M5), **installer / single file** (M10).

So: this is worth running to see whether the core feels right, and to catch
things I cannot catch without a desktop. It is not a preview of the finished
product.

---

## Before you start

You need about **6 GB of free disk** (3 GB of Rust build artefacts, 2 GB
model, 1 GB toolchains) and a working internet connection for the build. The
app itself runs offline afterwards.

**Nothing here has ever been run on a real desktop.** It compiles, it passes
220 tests, clippy is clean — but no window has ever opened, the tray icon has
never rendered, and `Win+O` has never been pressed, because the machine I
build on has no display. You are the first person to actually run it. Expect
problems, and see [When it breaks](#when-it-breaks).

---

## Step 1 — Install the toolchains

### Windows

Install in this order. The Visual Studio build tools are the step people skip,
and nothing works without them.

1. **Visual Studio Build Tools** — https://visualstudio.microsoft.com/visual-cpp-build-tools/
   Run the installer and tick **"Desktop development with C++"**. This is a
   several-GB download. Rust cannot link anything on Windows without it.
2. **Rust** — https://rustup.rs → download and run `rustup-init.exe`, accept
   the defaults.
3. **Node.js LTS** — https://nodejs.org → the LTS installer.
4. **Git** — https://git-scm.com/download/win

WebView2 is already present on Windows 10 and 11, so there is nothing to do
for it.

Then close and reopen your terminal so the `PATH` changes take effect.

### macOS

```bash
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
brew install node git
```

### Linux (Debian/Ubuntu)

```bash
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev patchelf \
  build-essential curl wget file libssl-dev git

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Node via nodesource, if your distro's is old
curl -fsSL https://deb.nodesource.com/setup_lts.x | sudo -E bash -
sudo apt install -y nodejs
```

`libayatana-appindicator3-dev` is what draws the tray icon on Linux. Without
it the app still runs but the tray is missing.

### Check it worked

```bash
rustc --version    # expect 1.77 or newer
node --version     # expect v18 or newer
git --version
```

---

## Step 2 — Get the code

```bash
git clone https://github.com/kuldeepyadav001/Orion.git
cd Orion
git checkout feat/m3-system-presence
```

Confirm you are on the right branch:

```bash
git branch --show-current      # must print feat/m3-system-presence
git log --oneline -1           # b91457f or later
```

---

## Step 3 — Install the frontend packages

```bash
npm install
```

---

## Step 4 — Download llama-server

This is the inference engine. It is not committed to the repo.

**macOS / Linux:**

```bash
chmod +x scripts/*.sh
./scripts/fetch-sidecars.sh
```

**Windows (PowerShell):**

The script is bash, so either use Git Bash:

```bash
./scripts/fetch-sidecars.sh
```

or do it by hand:

1. Download `llama-b4585-bin-win-avx2-x64.zip` from
   https://github.com/ggml-org/llama.cpp/releases/tag/b4585
   (use `win-noavx-x64` instead if your CPU is pre-2013)
2. Unzip it
3. Copy `llama-server.exe` **and every `.dll` beside it** into
   `src-tauri\binaries\`
4. Rename the exe to `llama-server-x86_64-pc-windows-msvc.exe`

The long name is not optional — Tauri resolves sidecars by target triple. Get
your exact triple with `rustc -vV` and read the `host:` line.

Verify:

```bash
ls src-tauri/binaries/
```

You should see `llama-server-<your-triple>` plus several shared libraries.

---

## Step 5 — Download a model

```bash
./scripts/fetch-model.sh
```

About 2 GB — Qwen2.5 3B Instruct, Q4_K_M. It goes to:

| OS | Location |
|---|---|
| Linux | `~/.local/share/orion/models/` |
| macOS | `~/Library/Application Support/orion/models/` |
| Windows | `%APPDATA%\orion\models\` |

On Windows without Git Bash, download it manually from
https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/blob/main/qwen2.5-3b-instruct-q4_k_m.gguf
and put it in that folder.

---

## Step 6 — Run it

```bash
npm run tauri dev
```

**The first build takes 10–25 minutes.** It is compiling several hundred Rust
crates. Subsequent runs take seconds. It will look frozen at
`Compiling tauri v2...` — it is not.

A window should open. The model takes a few more seconds to load after the
window appears; that is deliberate, so the window is not held hostage to a
2 GB memory map.

### Things to try

1. Send a message — it should stream back a token at a time.
2. Close the window with the X. **It should hide to the tray, not quit.**
3. Press `Win+O` (Linux/Windows) or `Cmd+O` (macOS) from another app.
4. Click the tray icon.
5. Right-click the tray icon → the menu, and **Quit Orion** to actually exit.
6. Drag a PDF onto the window — expect "Adding 1 file" and then nothing
   further. That is the known M2/M3 split, not a bug.
7. Drag a `.png` on — it should be refused by name and type.

---

## Step 7 — Build a real installer (optional)

```bash
npm run tauri build
```

Output lands in `src-tauri/target/release/bundle/`:

- Windows → `.msi` and `.exe` in `msi/` and `nsis/`
- macOS → `.dmg` and `.app`
- Linux → `.deb` and `.AppImage`

This is **not** the single-file offline installer from the plan. It does not
bundle the model, and it has never been tested. It is the stock Tauri bundler.

---

## When it breaks

Please send me the **exact** error text rather than a description — the first
line of a Rust error is usually enough to identify it.

### `error: linker 'cc' not found` / `link.exe not found`

Build tools missing. Windows: install the VS C++ workload from Step 1. Linux:
`sudo apt install build-essential`.

### `failed to run custom build command for 'orion'` → `resource path binaries/llama-server-... doesn't exist`

Step 4 was skipped, or the filename lacks the target triple. Run `rustc -vV`,
take the `host:` value, and make sure the file is named exactly
`llama-server-<that triple>` (plus `.exe` on Windows).

### `Package webkit2gtk-4.1 was not found`

Linux only, missing dev packages. Re-run the `apt install` line in Step 1. On
Fedora: `sudo dnf install webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel`.

### The window opens but says the engine failed to start

`llama-server` is present but not running. Test it directly:

```bash
./src-tauri/binaries/llama-server-<triple> --version
```

On Linux, if it complains about shared libraries, the `.so` files did not get
copied next to the binary — re-run `fetch-sidecars.sh`.

### No tray icon on Linux

Install `libayatana-appindicator3-dev` and rebuild. Some desktops (notably
GNOME without an extension) hide tray icons entirely; the hotkey still works.

### `Win+O` does nothing

Something else owns that chord. The app falls back to `Ctrl+Shift+O`, then
`Ctrl+Alt+O`, then `Ctrl+Shift+Space`. Check the terminal output — it logs
which one it registered. On Linux, Wayland restricts global hotkeys and they
may not work at all outside X11; that is a platform limit, not a bug I can fix
from here.

### It builds but the first run is enormously slow

Expected. First launch memory-maps a 2 GB model. Later launches are faster
because the OS caches it.

---

## What I most want to know

In rough priority order:

1. **Does the window actually open**, and how long does it take from launch to
   visible? The design budget is under one second, and that number has never
   been measured on real hardware — it is arithmetic, not a benchmark.
2. **Does the tray icon render**, and does left-click toggle correctly?
3. **Does `Win+O` work from inside another application** — a browser, a game,
   a full-screen editor?
4. **Does closing to tray feel right or surprising?**
5. Anything that crashes, hangs, or looks broken.

Timings and screenshots are more useful than "it worked".
