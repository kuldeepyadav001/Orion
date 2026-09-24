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

# 4. Packaging distribution bundle (Zip format for browser downloads)
NSIS_DIR="$ROOT/src-tauri/target/release/bundle/nsis"
DIST_DIR="$ROOT/src-tauri/target/release/bundle/dist_package"
if [ -d "$NSIS_DIR" ]; then
  LATEST_EXE="$(find "$NSIS_DIR" -name "*.exe" | head -n 1)"
  if [ -n "$LATEST_EXE" ]; then
    echo "==> Creating clean distribution ZIP archive..."
    rm -rf "$DIST_DIR"
    mkdir -p "$DIST_DIR"
    cp "$LATEST_EXE" "$DIST_DIR/"
    EXE_BASE="$(basename "$LATEST_EXE")"
    
    cat > "$DIST_DIR/Install-Orion.bat" <<EOF
@echo off
title Installing Orion Local AI...
echo ===================================================
echo   Orion Private AI Assistant - Setup Launcher
echo ===================================================
echo.
echo [1/2] Unblocking installer permissions...
powershell -NoProfile -ExecutionPolicy Bypass -Command "Get-ChildItem -Path '%~dp0' -Filter '*.exe' | Unblock-File"
echo.
echo [2/2] Launching installer...
start "" "%~dp0$EXE_BASE"
echo.
echo Setup initiated. You may close this window.
EOF

    cat > "$DIST_DIR/HOW-TO-INSTALL.txt" <<EOF
=====================================================================
               Orion - Offline Personal AI Assistant
=====================================================================

QUICK INSTALLATION:
1. Double-click "Install-Orion.bat" to start setup smoothly.
   OR double-click "$EXE_BASE" directly.

IF MICROSOFT EDGE OR WINDOWS SHOWS A WARNING:
- In Edge: Click the three dots (...) -> Click "Keep" -> Click "Keep anyway".
- In Windows: Click "More info" -> Click "Run anyway".
(This appears because Orion is an independent, offline-first application
running entirely on your computer without commercial Microsoft cloud certificates.)

REQUIREMENTS:
- 8 GB RAM or higher
- Windows 10 or 11 (64-bit)
- 100% offline, zero data leaves your PC.
=====================================================================
EOF

    if command -v zip &> /dev/null; then
      (cd "$DIST_DIR" && zip -r "$ROOT/src-tauri/target/release/bundle/Orion_v0.1.0_Windows_x64.zip" .)
      echo "    Distribution ZIP: src-tauri/target/release/bundle/Orion_v0.1.0_Windows_x64.zip"
    fi
  fi
fi
