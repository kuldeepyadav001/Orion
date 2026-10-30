#!/usr/bin/env node
//
// Measure the voice loop: text -> speech -> text.
//
// WHY THIS EXISTS
//
// The M4 gap list said "text -> speech latency was never measured" and
// "the wake word: nothing has been run". This harness measures the part
// that can be measured without a microphone: it synthesises speech with
// Piper, feeds the WAV to whisper-cli, and reports the latency of both
// halves and of the round trip.
//
// It calls the same binaries with the same arguments the application
// uses -- `voice::tts::build_args` and `voice::transcribe::build_args`
// -- so the numbers describe the shipping code path rather than a
// convenient approximation of it.
//
// The synthesised audio is also a legitimate input to the speech
// recogniser, so the STT measurement is a real transcription of real
// speech, not a fixture.
//
// Usage:
//   node scripts/bench-voice.mjs
//   node scripts/bench-voice.mjs --runs 3 --threads 2
//   node scripts/bench-voice.mjs --text "Orion is ready."
//
// Environment:
//   ORION_PIPER_PATH            path to the piper binary
//   ORION_PIPER_MODEL_PATH      path to the piper .onnx model
//   ORION_WHISPER_MODEL_PATH    path to the ggml whisper model
//   ORION_MODELS_DIR            where the models live

import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const BIN_DIR = join(ROOT, "src-tauri", "binaries");

const DEFAULT_TEXT =
  "Orion is a private, offline-first personal assistant that runs entirely " +
  "on your own machine.";

function arg(name, fallback) {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : fallback;
}

function has(name) {
  return process.argv.includes(`--${name}`);
}

function dataDir() {
  if (process.env.ORION_MODELS_DIR) return process.env.ORION_MODELS_DIR;
  if (process.platform === "darwin") {
    return join(process.env.HOME, "Library/Application Support/orion/models");
  }
  if (process.platform === "win32") {
    return join(process.env.APPDATA || join(process.env.HOME, "AppData/Roaming"), "orion/models");
  }
  return join(process.env.XDG_DATA_HOME || join(process.env.HOME, ".local/share"), "orion/models");
}

function findBinary(envVar, name) {
  if (process.env[envVar] && existsSync(process.env[envVar])) {
    return process.env[envVar];
  }
  const local = join(BIN_DIR, name);
  if (existsSync(local)) return local;
  return null;
}

function findModel(envVar, dir, names) {
  if (process.env[envVar] && existsSync(process.env[envVar])) {
    return process.env[envVar];
  }
  for (const n of names) {
    const p = join(dir, n);
    if (existsSync(p)) return p;
  }
  if (existsSync(dir)) {
    for (const entry of readdirSync(dir)) {
      if (entry.includes("silero") && entry.endsWith(".bin")) return join(dir, entry);
    }
  }
  return null;
}

/// Run a child process, feed it stdin, and time it to completion.
function run(binary, args, stdinText, cwd, env) {
  return new Promise((resolvePromise, reject) => {
    const started = process.hrtime.bigint();
    const child = spawn(binary, args, {
      cwd: cwd || ROOT,
      stdio: ["pipe", "pipe", "pipe"],
      env: env || process.env,
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d.toString()));
    child.stderr.on("data", (d) => (stderr += d.toString()));
    child.on("error", reject);
    child.on("close", (code) => {
      const ms = Number(process.hrtime.bigint() - started) / 1e6;
      resolvePromise({ code, stdout, stderr, ms });
    });
    if (stdinText !== undefined) {
      child.stdin.write(stdinText);
      child.stdin.end();
    }
  });
}

function median(xs) {
  const s = [...xs].sort((a, b) => a - b);
  const mid = s.length >> 1;
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

function fmt(ms) {
  return `${ms.toFixed(0)} ms`;
}

async function main() {
  const runs = parseInt(arg("runs", "3"), 10);
  const threads = parseInt(arg("threads", "2"), 10);
  const text = arg("text", DEFAULT_TEXT);

  const models = dataDir();
  const piper = findBinary("ORION_PIPER_PATH", "piper");
  const piperModel = findModel("ORION_PIPER_MODEL_PATH", models, [
    "en_US-lessac-low.onnx",
    "en_US-lessac-medium.onnx",
  ]);
  const whisper = findBinary("ORION_WHISPER_PATH", "whisper-cli");
  const whisperModel = findModel("ORION_WHISPER_MODEL_PATH", models, [
    "ggml-tiny.en.bin",
    "ggml-base.en.bin",
  ]);
  const vadModel = findModel("ORION_VAD_MODEL_PATH", models, []);

  const missing = [];
  if (!piper) missing.push(`piper binary (run scripts/fetch-tts.sh)`);
  if (!piperModel) missing.push(`piper model (run scripts/fetch-tts.sh)`);
  if (!whisper) missing.push(`whisper-cli (run scripts/fetch-voice.sh)`);
  if (!whisperModel) missing.push(`whisper model (run scripts/fetch-voice.sh)`);
  if (missing.length) {
    console.error(`missing: ${missing.join(", ")}`);
    process.exit(2);
  }

  console.log(`piper        : ${piper}`);
  console.log(`piper model  : ${piperModel}`);
  console.log(`whisper      : ${whisper}`);
  console.log(`whisper model: ${whisperModel}`);
  console.log(`vad model    : ${vadModel || "(none)"}`);
  console.log(`threads      : ${threads}`);
  console.log(`runs         : ${runs}`);
  console.log();

  const work = mkdtempSync(join(tmpdir(), "orion-voice-bench-"));
  const wav = join(work, "speech.wav");

  // --- text -> speech ---------------------------------------------------
  const ttsArgs = ["--model", piperModel, "--output_file", wav];
  // Mirror the application: espeak-ng reads its phoneme tables from
  // ESPEAK_DATA_PATH when they are installed beside the binary.
  const ttsEnv = { ...process.env };
  const espeakData = join(dirname(piper), "espeak-ng-data");
  if (existsSync(espeakData)) ttsEnv.ESPEAK_DATA_PATH = espeakData;
  const ttsTimes = [];
  let wavBytes = 0;
  for (let i = 0; i < runs; i++) {
    const r = await run(piper, ttsArgs, text, ROOT, ttsEnv);
    if (r.code !== 0) {
      console.error(`piper failed (${r.code}): ${r.stderr.trim()}`);
      process.exit(1);
    }
    ttsTimes.push(r.ms);
    wavBytes = statSync(wav).size;
  }
  const ttsCold = ttsTimes[0];
  const ttsWarm = median(ttsTimes.slice(1));

  // --- speech -> text ---------------------------------------------------
  const wArgs = ["-m", whisperModel, "-f", wav, "-nt", "-l", "en", "-t", String(threads), "-np"];
  if (vadModel) wArgs.push("--vad", "-vm", vadModel);
  const sttTimes = [];
  let transcript = "";
  for (let i = 0; i < runs; i++) {
    const r = await run(whisper, wArgs, undefined);
    if (r.code !== 0) {
      console.error(`whisper-cli failed (${r.code}): ${r.stderr.trim()}`);
      process.exit(1);
    }
    sttTimes.push(r.ms);
    transcript = r.stdout.trim();
  }
  const sttCold = sttTimes[0];
  const sttWarm = median(sttTimes.slice(1));

  // 16-bit mono WAV: a 44-byte header, then the sample data. The byte
  // rate lives at offset 28, so the duration is (bytes - 44) / byte rate.
  const buf = readFileSync(wav);
  const byteRate = buf.readUInt32LE(28);
  const audioMs = byteRate > 0 ? (buf.length - 44) / byteRate * 1000 : 0;

  console.log("=== text -> speech (Piper) ===");
  console.log(`cold (first run, includes the model load): ${fmt(ttsCold)}`);
  console.log(`warm (median of ${runs - 1}):                     ${fmt(ttsWarm)}`);
  console.log(`audio produced: ${audioMs.toFixed(0)} ms (${(buf.length / 1024).toFixed(0)} KiB)`);
  console.log(`realtime factor (warm): ${(ttsWarm / audioMs).toFixed(2)}x`);
  console.log();

  console.log("=== speech -> text (whisper.cpp) ===");
  console.log(`cold (first run, includes the model load): ${fmt(sttCold)}`);
  console.log(`warm (median of ${runs - 1}):                     ${fmt(sttWarm)}`);
  console.log(`realtime factor (warm): ${(sttWarm / audioMs).toFixed(2)}x`);
  console.log(`transcript: ${JSON.stringify(transcript.slice(0, 300))}`);
  console.log();

  console.log("=== round trip ===");
  console.log(`cold: ${fmt(ttsCold + sttCold)}`);
  console.log(`warm: ${fmt(ttsWarm + sttWarm)}`);
  console.log();

  rmSync(work, { recursive: true, force: true });
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
