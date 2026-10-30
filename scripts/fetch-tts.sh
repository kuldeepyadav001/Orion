#!/usr/bin/env bash
#
# Download Piper TTS and the lightweight speech synthesis model.
#
# Usage: ./scripts/fetch-tts.sh [version]

set -euo pipefail

VERSION="${1:-2023.11.14-2}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/src-tauri/binaries"
mkdir -p "$DEST"

case "$(uname -s)" in
  Linux)  ASSET="piper_linux_x86_64.tar.gz"; EXT="" ;;
  Darwin) ASSET="piper_macos_x64.tar.gz"; EXT="" ;;
  MINGW*|MSYS*|CYGWIN*) ASSET="piper_windows_amd64.zip"; EXT=".exe" ;;
  *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac

URL="https://github.com/rhasspy/piper/releases/download/${VERSION}/${ASSET}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "==> downloading Piper $ASSET"
if curl -fSL "$URL" -o "$TMP/piper.archive"; then
  mkdir -p "$TMP/x"
  case "$ASSET" in
    *.zip)    unzip -qo "$TMP/piper.archive" -d "$TMP/x" ;;
    *.tar.gz) tar xzf "$TMP/piper.archive" -C "$TMP/x" ;;
  esac

  BIN="$(find "$TMP/x" -type f -name "piper${EXT}" | head -1)"
  if [ -n "$BIN" ]; then
    cp "$BIN" "$DEST/piper${EXT}"
    chmod +x "$DEST/piper${EXT}"

    # Copy companion libraries if any
    BINDIR="$(dirname "$BIN")"
    find "$BINDIR" \( -name '*.so*' -o -name '*.dll' -o -name '*.dylib' \) -exec cp -a {} "$DEST/" \; 2>/dev/null || true
    echo "==> installed Piper binary: $DEST/piper${EXT}"
  fi
else
  echo "==> download of release archive failed or rate-limited; checking local system piper"
fi

# Target data directory for models
case "$(uname -s)" in
  Linux)  DATA="${XDG_DATA_HOME:-$HOME/.local/share}/orion" ;;
  Darwin) DATA="$HOME/Library/Application Support/orion" ;;
  *)      DATA="${APPDATA:-$HOME}/orion" ;;
esac
MODELS="$DATA/models"
mkdir -p "$MODELS"

fetch_model() {
  local name="$1" url="$2"
  if [ -f "$MODELS/$name" ]; then
    echo "==> already present: $name"
    return
  fi
  echo "==> downloading $name"
  curl -fL --progress-bar "$url" -o "$MODELS/$name.part"
  mv "$MODELS/$name.part" "$MODELS/$name"
}

# en_US-lessac-low: ~15 MB, very fast on 8 GB / low RAM CPUs
HF_BASE="https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/lessac/low"
fetch_model "en_US-lessac-low.onnx" "$HF_BASE/en_US-lessac-low.onnx" || true
fetch_model "en_US-lessac-low.onnx.json" "$HF_BASE/en_US-lessac-low.onnx.json" || true

# espeak-ng phoneme tables.
#
# The Piper release ships libespeak-ng but NOT the data it reads, and
# espeak-ng looks for the data in a compiled-in system path
# (/usr/share/espeak-ng-data on Linux). Without it Piper exits with
# "Error processing file '/usr/share/espeak-ng-data/phontab'" and Orion
# produces no speech at all. The tables are installed beside the binary
# and the child process is pointed at them with ESPEAK_DATA_PATH.
ESPEAK_DIR="$DEST/espeak-ng-data"
if [ -d "$ESPEAK_DIR" ] && [ -f "$ESPEAK_DIR/phontab" ]; then
  echo "==> espeak-ng data already present: $ESPEAK_DIR"
else
  echo "==> fetching espeak-ng phoneme data"
  TMP2="$(mktemp -d)"
  if command -v apt-get >/dev/null 2>&1 && (cd "$TMP2" && apt-get download espeak-ng-data >/dev/null 2>&1); then
    DEB="$(find "$TMP2" -maxdepth 1 -name 'espeak-ng-data_*.deb' | head -1)"
    [ -n "$DEB" ] && dpkg-deb -x "$DEB" "$TMP2/x" && rm -f "$DEB"
  fi
  if [ ! -d "$TMP2/x" ]; then
    # No apt (Windows, macOS): take the tables from the upstream tarball.
    curl -fSL "https://github.com/espeak-ng/espeak-ng/releases/download/1.52/espeak-ng-1.52.tar.gz" \
      -o "$TMP2/espeak.tar.gz" || true
    tar xzf "$TMP2/espeak.tar.gz" -C "$TMP2" 2>/dev/null || true
    if [ -d "$TMP2/espeak-ng-1.52/espeak-ng-data" ]; then
      mkdir -p "$ESPEAK_DIR"
      cp -a "$TMP2/espeak-ng-1.52/espeak-ng-data/." "$ESPEAK_DIR/"
    fi
  else
    SRC="$(find "$TMP2/x" -type d -name 'espeak-ng-data' | head -1)"
    if [ -n "$SRC" ]; then
      mkdir -p "$ESPEAK_DIR"
      cp -a "$SRC/." "$ESPEAK_DIR/"
    fi
  fi
  rm -rf "$TMP2"
  if [ -f "$ESPEAK_DIR/phontab" ]; then
    echo "==> installed espeak-ng data: $ESPEAK_DIR"
  else
    echo "!! espeak-ng data could not be installed; Piper will fail to" >&2
    echo "!! synthesise until espeak-ng is installed on the system." >&2
  fi
fi

echo "==> TTS setup completed"
