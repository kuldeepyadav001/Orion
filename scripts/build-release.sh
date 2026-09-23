#!/usr/bin/env bash
#
# Build production-ready signed installers for Orion (Milestone 6).
#
# Outputs:
#   Windows: .msi and .exe (NSIS installer)
#   Linux:   .deb and .AppImage
#
# Usage: ./scripts/build-release.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "============================================="
echo "   Orion Release Build Pipeline (M6)        "
echo "============================================="

# 1. Frontend validation & build
echo "==> [1/4] Building production frontend assets..."
npm run build

# 2. Verify sidecar binaries
echo "==> [2/4] Verifying sidecar binaries..."
BINARIES_DIR="$ROOT/src-tauri/binaries"
mkdir -p "$BINARIES_DIR"

if [ ! -f "$BINARIES_DIR/llama-server" ] && [ ! -f "$BINARIES_DIR/llama-server.exe" ]; then
  echo "    Notice: llama-server not found in src-tauri/binaries."
  echo "    Running fetch-sidecars.sh to ensure local packaging assets exist..."
  ./scripts/fetch-sidecars.sh || true
fi

# 3. Compile release bundle
echo "==> [3/4] Compiling release binaries via Tauri..."
if command -v cargo-tauri &> /dev/null; then
  cargo tauri build
elif command -v npx &> /dev/null; then
  npx @tauri-apps/cli build
else
  echo "Error: neither cargo-tauri nor npx available." >&2
  exit 1
fi

echo "==> [4/4] Release build completed successfully!"
echo "    Check src-tauri/target/release/bundle/ for the installer packages."
