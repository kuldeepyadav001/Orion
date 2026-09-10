#!/usr/bin/env bash
#
# Fetch the llama.cpp `llama-server` binary and place it where Tauri expects
# a sidecar: src-tauri/binaries/llama-server-<target-triple><ext>
#
# Tauri resolves sidecars by target triple, so the suffix is mandatory.
# These binaries are NOT committed — see .gitignore.
#
# Usage:  ./scripts/fetch-sidecars.sh [version]

set -euo pipefail

VERSION="${1:-b4585}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/src-tauri/binaries"
mkdir -p "$DEST"

TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
echo "==> target triple: $TRIPLE"

case "$(uname -s)" in
  Linux)  ASSET="llama-${VERSION}-bin-ubuntu-x64.zip"; EXT="" ;;
  Darwin)
    if [ "$(uname -m)" = "arm64" ]; then
      ASSET="llama-${VERSION}-bin-macos-arm64.zip"
    else
      ASSET="llama-${VERSION}-bin-macos-x64.zip"
    fi
    EXT="" ;;
  MINGW*|MSYS*|CYGWIN*) ASSET="llama-${VERSION}-bin-win-cpu-x64.zip"; EXT=".exe" ;;
  *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac

URL="https://github.com/ggml-org/llama.cpp/releases/download/${VERSION}/${ASSET}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "==> downloading $URL"
if ! curl -fSL "$URL" -o "$TMP/llama.zip"; then
  cat >&2 <<EOF

Download failed. Release asset names change between llama.cpp versions.
Open https://github.com/ggml-org/llama.cpp/releases, find the current asset
for your platform, and either pass the version:

    ./scripts/fetch-sidecars.sh b1234

or place llama-server manually at:

    $DEST/llama-server-${TRIPLE}${EXT}

EOF
  exit 1
fi

unzip -qo "$TMP/llama.zip" -d "$TMP/x"

BIN="$(find "$TMP/x" -type f -name "llama-server${EXT}" | head -1)"
if [ -z "$BIN" ]; then
  echo "llama-server${EXT} not found inside the archive" >&2
  exit 1
fi

cp "$BIN" "$DEST/llama-server-${TRIPLE}${EXT}"
chmod +x "$DEST/llama-server-${TRIPLE}${EXT}"

# Shared libraries (ggml, etc.) must sit alongside the binary.
find "$TMP/x" -type f \( -name '*.so*' -o -name '*.dll' -o -name '*.dylib' \) \
  -exec cp {} "$DEST/" \; 2>/dev/null || true

echo "==> installed: $DEST/llama-server-${TRIPLE}${EXT}"
ls -la "$DEST"
