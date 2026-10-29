#!/usr/bin/env node
//
// Measure what a local model actually costs: how long the sidecar takes to
// become ready, how long the first token takes, and how fast it generates.
//
// ## Why this exists
//
// Every latency claim Orion makes was, until now, a design contract. The
// M3 cold-start gate says the window must be visible in under a second; the
// answer to "and then what?" — how long until the user sees a token — was
// never measured, because no weights had ever been loaded on this
// measurement host.
//
// This script is the measurement. It starts `llama-server` the same way the
// application does (loopback, ephemeral port, bearer token, `--ctx-size`
// from the app's own default), then:
//
//   * times the cold start: process spawn -> `/health` answers 200, which
//     is the point at which the weights are mapped and the server can serve;
//   * times the first token of the first request;
//   * times steady-state time-to-first-token and generation rate.
//
// It reports the model, the quantisation and the host, because a tokens per
// second figure without them is meaningless.
//
// Usage:
//   scripts/bench-model.mjs <model.gguf> [--runs 3] [--ctx 4096] [--threads 2]
//                           [--prompt "..."] [--max-tokens 128] [--port 8090]

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import os from "node:os";
import { setTimeout as sleep } from "node:timers/promises";

const args = process.argv.slice(2);
function flag(name, fallback) {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
}

const MODEL = args[0];
const RUNS = Number(flag("runs", 3));
const CTX = Number(flag("ctx", 4096));
const THREADS = Number(flag("threads", 2));
const PORT = Number(flag("port", 8090));
const MAX_TOKENS = Number(flag("max-tokens", 128));
const PROMPT = flag(
  "prompt",
  "Explain in one paragraph why a personal AI assistant should run entirely on the user's own machine.",
);

if (!MODEL || !existsSync(MODEL)) {
  console.error("usage: scripts/bench-model.mjs <model.gguf> [--runs N] [--ctx N] [--threads N] [--port N]");
  process.exit(2);
}

const SIDECAR_DIR = process.env.ORION_BUILD_ROOT
  ? `${process.env.ORION_BUILD_ROOT}/sidecars`
  : "/usr/local/orion-build/sidecars";
const SERVER = `${SIDECAR_DIR}/llama-server-x86_64-unknown-linux-gnu`;
const TOKEN = "orion-bench-token";

function now() {
  return Number(process.hrtime.bigint() / 1000000n);
}

async function waitReady(proc, deadlineMs = 180000) {
  const t0 = now();
  while (now() - t0 < deadlineMs) {
    if (proc.exitCode !== null) return { ok: false, ms: now() - t0, reason: "server exited" };
    try {
      const r = await fetch(`http://127.0.0.1:${PORT}/health`, {
        headers: { Authorization: `Bearer ${TOKEN}` },
        signal: AbortSignal.timeout(2000),
      });
      if (r.status === 200) return { ok: true, ms: now() - t0 };
    } catch {
      /* not up yet */
    }
    await sleep(100);
  }
  return { ok: false, ms: now() - t0, reason: "timeout" };
}

/**
 * One streamed completion. Returns the time to the first content token,
 * the total time, the number of generated tokens and the reported usage.
 */
async function complete(prompt) {
  const t0 = now();
  const res = await fetch(`http://127.0.0.1:${PORT}/v1/chat/completions`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${TOKEN}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      model: "bench",
      messages: [{ role: "user", content: prompt }],
      max_tokens: MAX_TOKENS,
      temperature: 0,
      stream: true,
      stream_options: { include_usage: true },
    }),
  });

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let ttft = null;
  let text = "";
  let usage = null;

  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split("\n");
    buffer = lines.pop() ?? "";
    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed.startsWith("data:")) continue;
      const payload = trimmed.slice(5).trim();
      if (!payload || payload === "[DONE]") continue;
      let obj;
      try {
        obj = JSON.parse(payload);
      } catch {
        continue;
      }
      if (obj.usage) usage = obj.usage;
      const delta = obj.choices?.[0]?.delta?.content;
      if (delta) {
        if (ttft === null) ttft = now() - t0;
        text += delta;
      }
    }
  }

  const total = now() - t0;
  const completionTokens = usage?.completion_tokens ?? null;
  return {
    ttft,
    total,
    completionTokens,
    chars: text.length,
    tokensPerSecond:
      completionTokens && total > ttft ? completionTokens / ((total - ttft) / 1000) : null,
  };
}

function fmt(n, digits = 1) {
  return n === null || n === undefined ? "n/a" : Number(n).toFixed(digits);
}

console.log("Orion local-model benchmark");
console.log(`model:    ${MODEL}`);
console.log(`ctx:      ${CTX}   threads: ${THREADS}`);
console.log(`host:     ${os_info()}`);
console.log();

const coldLoads = [];
const coldFirstTokens = [];
const warmTtft = [];
const warmTps = [];

for (let run = 1; run <= RUNS; run += 1) {
  const t0 = now();
  const proc = spawn(
    SERVER,
    [
      "-m", MODEL,
      "--host", "127.0.0.1",
      "--port", String(PORT),
      "--api-key", TOKEN,
      "-c", String(CTX),
      "-t", String(THREADS),
      "--no-webui",
    ],
    { env: { ...process.env, LD_LIBRARY_PATH: SIDECAR_DIR }, stdio: ["ignore", "pipe", "pipe"] },
  );
  let stderr = "";
  proc.stderr.on("data", (d) => {
    stderr += d.toString();
  });

  const ready = await waitReady(proc);
  if (!ready.ok) {
    console.error(`run ${run}: server never became ready (${ready.reason})`);
    console.error(stderr.split("\n").slice(-25).join("\n"));
    proc.kill("SIGKILL");
    process.exit(1);
  }

  const first = await complete(PROMPT);
  coldLoads.push(ready.ms);
  coldFirstTokens.push(first.ttft);

  // Warm: the server is already up and the weights are mapped.
  for (let i = 0; i < 3; i += 1) {
    const r = await complete(PROMPT);
    warmTtft.push(r.ttft);
    if (r.tokensPerSecond) warmTps.push(r.tokensPerSecond);
  }

  console.log(
    `run ${run}: cold load ${ready.ms} ms | first token ${fmt(first.ttft)} ms | ` +
      `first completion ${fmt(first.total)} ms | ${fmt(first.completionTokens, 0)} tokens`,
  );

  proc.kill("SIGTERM");
  await sleep(400);
  if (proc.exitCode === null) proc.kill("SIGKILL");
  await sleep(300);
}

const median = (a) => {
  if (!a.length) return null;
  const s = [...a].sort((x, y) => x - y);
  return s[Math.floor((s.length - 1) / 2)];
};
const mean = (a) => (a.length ? a.reduce((x, y) => x + y, 0) / a.length : null);

console.log();
console.log("=== summary ===");
console.log(`cold start, weights mapped and server ready : ${fmt(median(coldLoads), 0)} ms (median of ${RUNS})`);
console.log(`first token of the first request            : ${fmt(median(coldFirstTokens), 0)} ms`);
console.log(`warm time to first token                    : ${fmt(median(warmTtft), 0)} ms (median)`);
console.log(`warm generation rate                        : ${fmt(mean(warmTps), 2)} tok/s (mean)`);
console.log(`mean warm time to first token               : ${fmt(mean(warmTtft), 0)} ms`);
console.log();
console.log(
  "Note: these figures are for the model named above. A larger model in the\n" +
    "same quantisation is slower per token; the host here has fewer cores and\n" +
    "less RAM than the product's target machine.",
);

function os_info() {
  return `${os.cpus().length} cores, ${(os.totalmem() / 1024 ** 3).toFixed(1)} GiB RAM, ${os.platform()}`;
}
