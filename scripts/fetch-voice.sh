#!/usr/bin/env bash
#
# Download whisper.cpp and the speech models for voice input.
#
# Separate from fetch-sidecars.sh because voice is optional: Orion works
# without it, and this pulls ~80 MB of models on top of the binaries.
#
# Usage:  ./scripts/fetch-voice.sh [version]

set -euo pipefail

VERSION="${1:-v1.9.2}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/src-tauri/binaries"
mkdir -p "$DEST"

case "$(uname -s)" in
  Linux)  ASSET="whisper-bin-ubuntu-x64.tar.gz"; EXT="" ;;
  Darwin) ASSET="whisper-v${VERSION#v}-xcframework.zip"; EXT="" ;;
  MINGW*|MSYS*|CYGWIN*) ASSET="whisper-bin-x64.zip"; EXT=".exe" ;;
  *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac

URL="https://github.com/ggml-org/whisper.cpp/releases/download/${VERSION}/${ASSET}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "==> downloading $ASSET"
if ! curl -fSL "$URL" -o "$TMP/w.archive"; then
  cat >&2 <<EOF

Download failed. Release asset names change between whisper.cpp versions.
Open https://github.com/ggml-org/whisper.cpp/releases, find the current
x64 asset for your platform, and pass the version:

    ./scripts/fetch-voice.sh v1.9.2

EOF
  exit 1
fi

mkdir -p "$TMP/x"
case "$ASSET" in
  *.zip)    unzip -qo "$TMP/w.archive" -d "$TMP/x" ;;
  *.tar.gz) tar xzf "$TMP/w.archive" -C "$TMP/x" ;;
esac

# whisper-cli, NOT whisper-server. The server has no authentication of any
# kind and does not honour --host: started on 127.0.0.1 it also binds the LAN
# interface, so it exposes an unauthenticated transcription service to the
# local network. See docs/M4-VOICE-FINDINGS.md.
BIN="$(find "$TMP/x" -type f -name "whisper-cli${EXT}" | head -1)"
if [ -z "$BIN" ]; then
  echo "whisper-cli${EXT} not found inside the archive" >&2
  exit 1
fi
cp "$BIN" "$DEST/whisper-cli${EXT}"
chmod +x "$DEST/whisper-cli${EXT}"

# Shared libraries must sit next to the binary, or it exits immediately with
# STATUS_DLL_NOT_FOUND and prints nothing at all.
#
# IMPORTANT: we must copy symlinks (e.g. libwhisper.so.1 -> libwhisper.so.1.9.2)
# preserving their link structure (cp -a). Skipping symlinks (-type f alone)
# causes dynamic linker failures at runtime: "cannot open shared object file".
LIBS=0
while IFS= read -r lib; do
  cp -a "$lib" "$DEST/" && LIBS=$((LIBS + 1))
done < <(find "$TMP/x" \( -type f -o -type l \) \( -name '*.so*' -o -name '*.dll' -o -name '*.dylib' \))
echo "==> copied $LIBS shared library files/links"

for profile in debug release; do
  for base in "${CARGO_TARGET_DIR:-}" "$ROOT/src-tauri/target" "$ROOT/src-tauri/target_clean"; do
    [ -z "$base" ] && continue
    T="$base/$profile"
    if [ -d "$T" ]; then
      cp "$DEST/whisper-cli${EXT}" "$T/" 2>/dev/null || true
      while IFS= read -r lib; do
        cp -a "$lib" "$T/" 2>/dev/null || true
      done < <(find "$TMP/x" \( -type f -o -type l \) \( -name '*.so*' -o -name '*.dll' -o -name '*.dylib' \))
      echo "==> mirrored into $T"
    fi
  done
done

# Models go in the data directory alongside the chat model.
case "$(uname -s)" in
  Linux)  DATA="${XDG_DATA_HOME:-$HOME/.local/share}/orion" ;;
  Darwin) DATA="$HOME/Library/Application Support/orion" ;;
  *)      DATA="${APPDATA:-$HOME}/orion" ;;
esac
MODELS="$DATA/models"
mkdir -p "$MODELS"

# tiny.en: 77 MB, measured at ~5-6x realtime on CPU with exact transcription
# of the standard test clip. base.en is a tier-2 upgrade, not a default —
# nearly double the memory for a modest gain speech commands rarely need.
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

fetch_model "ggml-tiny.en.bin" \
  "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin"

# Silero VAD, 865 KB. Built into whisper.cpp via --vad; on the test clip it
# discarded 25% of the audio as silence before transcription.
fetch_model "ggml-silero-v5.1.2.bin" \
  "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin"

echo
echo "==> voice ready"
echo "    binary : $DEST/whisper-cli${EXT}"
echo "    models : $MODELS"
echo "    Restart Orion and press the microphone button."
