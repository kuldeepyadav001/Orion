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

echo "==> TTS setup completed"
