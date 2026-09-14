# Running Orion (M0) on your own machine

This is the guide for the **`main` branch**, which is M0: the foundation.
Build this first, confirm it works, and only then will M1 be merged in.

**There is no installer yet.** You build from source. That comes at M10.

---

## What M0 actually is

| Works | Notes |
|---|---|
| Chat with a local model | Streaming, token by token |
| Fully offline | Nothing leaves your machine once the model is downloaded |
| Chat history | SQLite, stored locally |
| Stop button | Cancels a running generation |

**Not here yet** — no tray icon, no `Win+O` hotkey, no file drop, no document
Q&A, no voice, no hardware detection. Those are M1–M3, on their own branches,
and they get merged in one at a time after you confirm each step.

So M0 is a plain chat window. That is the point: if the foundation is wrong,
everything stacked on top is wrong too.

---

## About Visual Studio — read this before installing anything

**Yes, you need it. No, you do not need 6.8 GB.**

The 6.8 GB figure is the *"Desktop development with C++"* workload, which
bundles CMake, MFC, ATL, profiling tools, test adapters and more. Rust needs
almost none of it. It needs exactly three things: the MSVC compiler, the
linker (`link.exe`), and the Windows SDK import libraries.

Rust cannot ship these itself — Microsoft does not permit redistribution —
which is why you have to install them separately.

### The smaller install (recommended, ~3–4 GB)

In the **Visual Studio Installer**, do *not* tick the big workload tile.
Instead:

1. Click the **"Individual components"** tab
2. Tick only:
   - **MSVC v143 – VS 2022 C++ x64/x86 build tools (Latest)**
   - **Windows 11 SDK** (or Windows 10 SDK if you are on Windows 10)
3. Install

That is the officially documented minimum from the rustup book.

**One honest warning:** there are reports that `rustup-init.exe` sometimes
still claims build tools are missing after the minimal install, and only stops
complaining once the full workload is added. If you hit that, the practical
answer is to tick the full workload — annoying, but it is 30 minutes rather
than a lost evening. You can remove components afterwards.

### Why not avoid it entirely?

Two real alternatives exist, and both have a catch:

- **GNU toolchain (MinGW)** — ~600 MB instead of several GB. But Orion uses
  `rusqlite` with the `bundled` feature, which compiles SQLite from C source,
  and Tauri on Windows is far better tested against MSVC. If the GNU build
  breaks, we would be debugging the toolchain instead of Orion.
- **WSL** — small, but you would get a *Linux* Orion running inside WSL, not a
  Windows app. That tells us nothing about how it behaves on Windows.

**Recommendation: take the MSVC route.** You have already downloaded the
installer; use the Individual components tab and it will be noticeably
smaller than 6.8 GB.

### What you already have

VS Code is a text editor and is **not** the same thing as Visual Studio Build
Tools — it does not provide a compiler, so it does not replace this step. Node
you already have, which covers the frontend. So the only genuinely new
installs are the build tools and Rust.

---

## Step 1 — Install the toolchains

### Windows

1. **Build tools** — see the section above. Install them **first**; rustup
   checks for them.
2. **Rust** — https://rustup.rs → run `rustup-init.exe`, accept defaults.
3. **Git** — https://git-scm.com/download/win
   During install, keep **"Git Bash"** enabled. The setup scripts are bash, and
   Git Bash is the easiest way to run them on Windows.
4. Node — you already have it.

Close and reopen your terminal afterwards so `PATH` updates.

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
```

### Verify

```bash
rustc --version    # 1.77 or newer
cargo --version
node --version     # v18 or newer
```

If `rustc` is not found, reopen your terminal.

---

## Step 2 — Get the code

```bash
git clone https://github.com/kuldeepyadav001/Orion.git
cd Orion
```

You are on `main` by default, which is what you want. Confirm:

```bash
git branch --show-current    # main
```

---

## Step 3 — Frontend packages

```bash
npm install
```

---

## Step 4 — Download llama-server

This is the inference engine; it is not committed to the repo.

**macOS / Linux / Windows Git Bash:**

```bash
chmod +x scripts/*.sh
./scripts/fetch-sidecars.sh
```

**Windows, manually** (if you skipped Git Bash):

1. Get `llama-b4585-bin-win-avx2-x64.zip` from
   https://github.com/ggml-org/llama.cpp/releases/tag/b4585
   (use `win-noavx-x64` if your CPU predates ~2013)
2. Unzip it
3. Copy `llama-server.exe` **and every `.dll` next to it** into
   `src-tauri\binaries\`
4. Rename the exe to `llama-server-x86_64-pc-windows-msvc.exe`

The triple suffix is mandatory — Tauri resolves sidecars by target triple. Run
`rustc -vV` and read the `host:` line to get yours exactly.

Check:

```bash
ls src-tauri/binaries/
```

Expect `llama-server-<triple>` plus several shared libraries.

---

## Step 5 — Download a model

```bash
./scripts/fetch-model.sh
```

~2 GB, Qwen2.5 3B Instruct Q4_K_M. Destination:

| OS | Path |
|---|---|
| Windows | `%APPDATA%\orion\models\` |
| macOS | `~/Library/Application Support/orion/models/` |
| Linux | `~/.local/share/orion/models/` |

Manual download if needed:
https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/blob/main/qwen2.5-3b-instruct-q4_k_m.gguf

---

## Step 6 — Run

```bash
npm run tauri dev
```

**First build: 10–25 minutes.** It compiles several hundred Rust crates and
will sit at `Compiling tauri v2...` looking frozen. It is not. Later runs take
seconds.

### What to check

1. A window opens.
2. Send a message — the reply should stream in, not appear all at once.
3. Send a long one and press **Stop** mid-reply.
4. Close the app, reopen it — is your history still there?
5. **Turn off your Wi-Fi and send another message.** It must still work. That
   is the entire premise of the product.

---

## When it breaks

Send me the **exact error text**, not a paraphrase. The first line of a Rust
error is usually enough.

### `linker 'link.exe' not found`

Build tools missing or installed after Rust. Install them, then:

```bash
rustup default stable-x86_64-pc-windows-msvc
```

### `resource path binaries/llama-server-... doesn't exist`

Step 4 skipped, or the filename lacks the target triple. Check `rustc -vV`.

### `Package webkit2gtk-4.1 was not found`

Linux only — re-run the `apt install` from Step 1. Fedora:
`sudo dnf install webkit2gtk4.1-devel gtk3-devel`.

### Window opens, but the engine never becomes ready

Test the sidecar directly:

```bash
./src-tauri/binaries/llama-server-<triple> --version
```

Should print `version: 4585 (...)`. If it complains about missing shared
libraries, the `.dll`/`.so` files were not copied next to it.

Then confirm the model file is in the folder from Step 5 and is ~2 GB, not a
few KB (a failed download leaves a stub).

### Very slow first reply

Normal. The first message loads ~2 GB into memory. On 8 GB RAM expect a slow
first token and roughly 3–8 tokens/second after that.

---

## What I want to know

1. **Does the window open at all**, and roughly how long from launch?
2. **Does text stream** token by token, or arrive in one block?
3. **Tokens per second**, roughly — and your RAM and CPU.
4. **Does it work with Wi-Fi off?**
5. Anything that crashes, hangs, or looks wrong.

Once M0 is confirmed good, tell me and I will merge M1 (hardware detection and
tiered model selection) into `main` for the next round.

---

## Honest status

M0 compiles clean and its tests pass, but **it has never been run on a machine
with a display** — my build environment has no desktop, so no window has ever
opened. You are the first person to actually launch it. Expect rough edges.
