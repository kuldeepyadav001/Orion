#!/usr/bin/env bash
#
# Download the embedding model that powers semantic document search.
#
# This is SEPARATE from the chat model and much smaller (~130 MB). Orion runs
# it in a second llama-server process in embedding mode, because one server
# serves one model and the chat model's vectors are not trained for retrieval.
#
# Without it Orion still works: document search falls back to keyword-only
# (BM25), which the M2 evaluation measured at 0.731 recall@4 against 0.769 for
# hybrid. Paraphrased questions suffer most.

set -euo pipefail

REPO="${ORION_EMBED_REPO:-CompendiumLabs/bge-small-en-v1.5-gguf}"
FILE="${ORION_EMBED_FILE:-bge-small-en-v1.5-q8_0.gguf}"

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
echo "==> downloading $FILE (~130 MB)"
echo "    from: $REPO"
echo "    to:   $DEST"

curl -fL --progress-bar "$URL" -o "$DEST/$FILE.part"
mv "$DEST/$FILE.part" "$DEST/$FILE"

echo "==> done: $DEST/$FILE"
echo "    Restart Orion; semantic search will come up a few seconds after the chat model."
