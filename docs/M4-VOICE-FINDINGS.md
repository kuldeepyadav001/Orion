# M4 — voice: what the probe established

Everything below was **run**, not read. whisper.cpp v1.9.2 Linux x64 build,
executed in the dev sandbox against real audio, before any Orion code was
written. The last four milestones each shipped a crash that came from writing
integration code against an API I had only read about, so this milestone
starts by proving the contract.

## Decisions this changes

### 1. STT is a sidecar server, not a CLI

`whisper-server` exists in the prebuilt release and speaks HTTP. That means
STT follows **exactly the same pattern as `llama-server`**, which is already
proven in this codebase: spawn, poll `/health`, POST, register with the
sidecar registry so it dies with the app.

The alternative — invoking `whisper-cli` per utterance — would reload the
model on every phrase. Measured load time is ~75 ms for tiny.en, which is
tolerable, but the server keeps it warm and reuses the same lifecycle code.

### 2. VAD is built in. Silero is not a separate component

The plan listed Silero VAD as its own dependency. It is not needed:
whisper.cpp ships VAD natively via `--vad -vm <model>`, with a 865 KB Silero
model from `ggml-org/whisper-vad`.

Verified working: on an 11-second clip it found 5 speech segments and
**discarded 25% of the audio as silence** before transcription. That is a
direct latency saving, not just tidiness.

One component removed from the M4 dependency list.

### 3. There is no OpenAI-compatible route

```
POST /inference                  -> 200, {"text": "..."}
POST /v1/audio/transcriptions    -> 404 File Not Found
```

So the embedder's OpenAI-shaped client cannot be reused. `/inference` takes
multipart form data with a `file=` part.

### 4. `whisper-server` has NO authentication, and does not honour `--host`

This is the finding that matters most, and it is a real security problem.

`llama-server` takes `--api-key` and Orion generates a random token per
session. **`whisper-server` has no equivalent flag** — `--help` lists nothing
for api, key or auth, and a request with a junk bearer token is accepted with
HTTP 200.

Worse, despite `--host 127.0.0.1`:

```
LISTEN  127.0.0.1:8899
LISTEN  169.254.0.21:8899     <- also bound to the LAN interface
```

Confirmed reachable off loopback: a POST to the non-loopback address returned
a transcript. As shipped, that is **an unauthenticated speech-to-text service
exposed to the local network**. On a café or campus Wi-Fi, anyone could send
audio to it and read the result.

For a product whose entire claim is that your data never leaves the machine,
this cannot ship as-is. Mitigations, in order of preference:

1. Bind to a random ephemeral port (already the pattern for llama-server) so
   it is not discoverable by scanning a known port.
2. Add a firewall rule on install, or
3. Prefer `whisper-cli` per utterance, which opens no socket at all.

Option 3 costs ~75 ms of model load per phrase and removes the attack surface
entirely. Given the numbers below, that is a good trade and is what M4 should
use until the exposure is fixed upstream. **Recorded as a blocker for the
release checklist, not something to discover later.**

## Measured numbers

Sandbox CPU, 4 threads, `ggml-tiny.en.bin` (77 MB), Silero VAD, an 11-second
16 kHz mono WAV (the standard JFK sample).

| Metric | Value |
|---|---|
| `whisper-cli`, cold, including model load | 2.08 s |
| `whisper-server` HTTP round trip, warm | 1.77 s |
| Realtime factor | ~5-6x |
| Model load | 75 ms |
| Resident memory | ~77 MB weights + ~130 MB compute buffers |
| Audio discarded by VAD | 25% |

Transcription was exact:

> "And so my fellow Americans ask not what your country can do for you. Ask
> what you can do for your country."

### What this means for the latency budget

The plan warned of a 5-10 s CPU round trip. For **STT alone** that is
pessimistic: ~1.8 s for 11 seconds of speech, and a typical command is 2-3
seconds of audio, so under a second is realistic.

The remaining budget is dominated by the LLM, not by transcription:

```
speech -> text        ~0.5-1.8 s   measured
LLM first token       ~0.5-2 s     depends on context length
LLM full reply        seconds      ~10 tok/s on the target machine
text -> speech        ~0.1-0.5 s   Piper, per the plan; not yet measured
```

So the honest framing is: **voice input is fast; the wait is the model
thinking.** Streaming TTS from the first sentence matters more than STT
optimisation.

## Memory, on the machine this targets

The target laptop has 5.7 GiB usable and already runs:

- chat model, 2.4 GB
- embedder, ~130 MB

Adding tiny.en costs ~210 MB resident. That fits, but it is the third model in
memory on a machine that reports ~1 GB free. Two consequences:

- STT must be **lazy**, like the chat engine now is: load on first use, not at
  launch.
- tiny.en is the right default for T1. base.en (142 MB) is a tier-2 upgrade,
  not a default.

## Still unverified

The probe covered the parts I can execute here. These remain open:

- **Microphone capture.** No audio device in this sandbox. Recording, device
  selection, permissions and the 16 kHz mono conversion are all untested.
- **The wake word.** `oww-rs` (a Rust port of openWakeWord on tract-onnx) is
  the candidate, but nothing has been run.
- **Windows behaviour.** The Windows zip contains `whisper-server.exe` and the
  same DLL set, but the whole probe ran on Linux.
- **Real latency on the target machine.** The sandbox CPU is not a Ryzen 5
  7520U with 1 GB free.
