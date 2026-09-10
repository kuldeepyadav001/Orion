#!/usr/bin/env bash
#
# Download a T1-tier GGUF chat model into Orion's data directory.
# T1 = the 8 GB / CPU-only tier. Qwen3 4B Q4_K_M, ~2.5 GB on disk, ~3.4 GB resident.
#
# Apache-2.0 licensed, so it is safe to redistribute in an offline bundle later.

set -euo pipefail

REPO="${ORION_MODEL_REPO:-Qwen/Qwen2.5-3B-Instruct-GGUF}"
FILE="${ORION_MODEL_FILE:-qwen2.5-3b-instruct-q4_k_m.gguf}"

case "$(uname -s)" in
  Linux)  DATA="${XDG_DATA_HOME:-$HOME/.local/share}/orion" ;;
  Darwin) DATA="$HOME/Library/Application Support/orion" ;;
  *)      DATA="${APPDATA:-$HOME}/orion" ;;
esac

DEST="$DATA/models"
mkdir -p "$DEST"

if [ -f "$DEST/$FILE" ]; then
  echo "==> already present: $DEST/$FILE"
  exit 0
fi

URL="https://huggingface.co/${REPO}/resolve/main/${FILE}?download=true"
echo "==> downloading $FILE (this is a couple of GB)"
echo "    from: $REPO"
echo "    to:   $DEST"

curl -fL --progress-bar "$URL" -o "$DEST/$FILE.part"
mv "$DEST/$FILE.part" "$DEST/$FILE"

echo "==> done: $DEST/$FILE"
echo "    Orion will pick this up automatically on next launch."
