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
  MINGW*|MSYS*|CYGWIN*)
    # NOT "win-cpu-x64": that asset name does not exist for this release.
    # llama.cpp ships per-instruction-set Windows builds; avx2 is the safe
    # default for any x86-64 CPU made in the last decade. Use win-noavx-x64
    # on very old hardware.
    ASSET="llama-${VERSION}-bin-win-avx2-x64.zip"; EXT=".exe" ;;
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

# Shared libraries (ggml, llama, etc.) must sit alongside the binary.
#
# This used to end in `|| true`, which silently swallowed a failed copy. If the
# libraries are missing, llama-server dies instantly at spawn with exit code
# 0xC0000135 (STATUS_DLL_NOT_FOUND, reported by Rust as -1073741515) and prints
# nothing at all, which is a miserable thing to debug. Verify instead.
LIBCOUNT=0
while IFS= read -r lib; do
  cp "$lib" "$DEST/" && LIBCOUNT=$((LIBCOUNT + 1))
done < <(find "$TMP/x" -type f \( -name '*.so*' -o -name '*.dll' -o -name '*.dylib' \))

if [ "$LIBCOUNT" -eq 0 ]; then
  cat >&2 <<EOF

ERROR: no shared libraries were found in the archive.

llama-server cannot start without them: it exits immediately with
STATUS_DLL_NOT_FOUND and no error message. The release layout may have
changed. Inspect the archive and copy the libraries next to the binary by
hand:

    $DEST/

EOF
  exit 1
fi
echo "==> copied $LIBCOUNT shared librar$([ "$LIBCOUNT" -eq 1 ] && echo y || echo ies)"

# Tauri's sidecar mechanism copies ONLY the executable into the target
# directory, leaving its libraries behind in src-tauri/binaries. On Windows
# that guarantees STATUS_DLL_NOT_FOUND on the first `tauri dev` run, because
# Windows resolves DLLs relative to the executable.
#
# Found the hard way on a real machine: the app compiled, launched, and the
# engine died in 65 ms with no output.
for profile in debug release; do
  for base in "${CARGO_TARGET_DIR:-}" "$ROOT/src-tauri/target" "$ROOT/src-tauri/target_clean"; do
    [ -z "$base" ] && continue
    TARGET_DIR="$base/$profile"
    if [ -d "$TARGET_DIR" ]; then
      find "$TMP/x" -type f \( -name '*.so*' -o -name '*.dll' -o -name '*.dylib' \) \
        -exec cp {} "$TARGET_DIR/" \; 2>/dev/null || true
      echo "==> libraries mirrored into $TARGET_DIR"
    fi
  done
done

echo "==> installed: $DEST/llama-server-${TRIPLE}${EXT}"
ls -la "$DEST"

cat <<EOF

NOTE: if you build with a target directory that did not exist when this script
ran, re-run this script afterwards, or copy the libraries yourself:

    cp "$DEST"/*.dll "$ROOT/src-tauri/target/debug/"      # Windows
    cp "$DEST"/*.so* "$ROOT/src-tauri/target/debug/"      # Linux

EOF
