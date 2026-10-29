#!/usr/bin/env bash
#
# Run the retrieval evaluation against a LIVE embedding model.
#
# The unit tests in `src-tauri/src/rag/eval.rs` use a pseudo-embedder: a
# hashed bag-of-words projection that matches vocabulary but never meaning.
# It proves the fusion plumbing behaves; it cannot support a quality claim.
#
# This script closes that gap. It starts `llama-server` in embedding mode
# with the real model Orion ships (bge-small-en-v1.5-q8_0), runs
# `cargo run --bin orion_eval` against it, and stops the server again.
#
# The output includes Wilson confidence intervals and a *paired* comparison
# against BM25 — the difference between "hybrid is directionally better" and
# "hybrid is better, and here is the evidence".
#
# Usage:
#   scripts/run-eval.sh                 # hybrid vs BM25 vs dense, k = 4
#   scripts/run-eval.sh --k 6
#   scripts/run-eval.sh --no-headings   # ablation: heading trail removed
#   scripts/run-eval.sh --verbose       # print the per-question table
#
# Environment:
#   ORION_EVAL_PORT   port for the embedding server (default 8082)
#   ORION_EVAL_TOKEN  bearer token          (default orion-eval-token)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_ROOT="${ORION_BUILD_ROOT:-/usr/local/orion-build}"
SIDECAR_DIR="$BUILD_ROOT/sidecars"
LLAMA_SERVER="$SIDECAR_DIR/llama-server-x86_64-unknown-linux-gnu"

# --- locate the embedding model ------------------------------------------
MODEL="${ORION_EMBED_MODEL:-${XDG_DATA_HOME:-$HOME/.local/share}/orion/models/bge-small-en-v1.5-q8_0.gguf}"

# --- locate the server binary ---------------------------------------------
if [ ! -x "$LLAMA_SERVER" ]; then
  for candidate in "$ROOT/src-tauri/binaries/llama-server-x86_64-unknown-linux-gnu" \
                   "$ROOT/src-tauri/binaries/llama-server" \
                   "$BUILD_ROOT/sidecars/llama-server"; do
    if [ -x "$candidate" ]; then
      LLAMA_SERVER="$candidate"
      break
    fi
  done
fi

if [ ! -x "$LLAMA_SERVER" ]; then
  echo "==> llama-server not found; fetching it"
  "$ROOT/scripts/fetch-sidecars.sh"
fi

if [ ! -f "$MODEL" ]; then
  echo "==> embedding model not found at $MODEL; fetching it"
  "$ROOT/scripts/fetch-embed-model.sh"
fi

PORT="${ORION_EVAL_PORT:-8082}"
TOKEN="${ORION_EVAL_TOKEN:-orion-eval-token}"
LOG="$(mktemp -t orion-embed-server.XXXXXX.log)"
SERVER_PID=""

cleanup() {
  if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
    echo "==> stopping embedding server (pid $SERVER_PID)"
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  if [ "${ORION_KEEP_EMBED_LOG:-0}" != "1" ]; then
    rm -f "$LOG"
  else
    echo "==> server log kept at $LOG"
  fi
}
trap cleanup EXIT INT TERM

echo "==> starting embedding server on 127.0.0.1:$PORT"
LD_LIBRARY_PATH="$SIDECAR_DIR:${LD_LIBRARY_PATH:-}" \
  "$LLAMA_SERVER" \
    -m "$MODEL" \
    --embedding \
    --pooling cls \
    --host 127.0.0.1 \
    --port "$PORT" \
    --api-key "$TOKEN" \
    -c 512 \
    --no-webui \
    > "$LOG" 2>&1 &
SERVER_PID=$!

# Wait for /health. llama-server answers 503 with "Loading model" until the
# weights are mapped, so poll rather than sleep a fixed amount.
READY=0
for _ in $(seq 1 120); do
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    echo "!! embedding server died during startup; log follows:" >&2
    cat "$LOG" >&2
    exit 1
  fi
  code="$(curl -s -o /dev/null -w '%{http_code}' -m 3 \
            "http://127.0.0.1:$PORT/health" || true)"
  if [ "$code" = "200" ]; then
    READY=1
    break
  fi
  sleep 1
done

if [ "$READY" != "1" ]; then
  echo "!! embedding server did not become ready in 120s; log follows:" >&2
  cat "$LOG" >&2
  exit 1
fi

echo "==> embedding server ready"

# --- build and run the evaluator -------------------------------------------
cd "$ROOT/src-tauri"
# shellcheck source=/dev/null
source "$ROOT/scripts/rust-env.sh"
# shellcheck source=/dev/null
source "$ROOT/scripts/sysroot-env.sh"

ORION_EVAL_URL="http://127.0.0.1:$PORT" \
ORION_EVAL_TOKEN="$TOKEN" \
  cargo run --quiet --bin orion_eval -- "$@"
