#!/usr/bin/env bash
#
# Restore the build environment in one step.
#
# WHY THIS EXISTS
#
# The workspace snapshot carries the repository but not the toolchain it
# needs to build:
#
#   * no rustc/cargo  - the image ships none, and rustup's proxy shims are
#                       not created on this filesystem
#   * no sysroot      - the GUI development libraries (webkit2gtk, gtk,
#                       alsa) that the Tauri crate links against
#   * no node_modules - the React dependency tree
#
# Running this script restores all three. It is idempotent: anything
# already present is left alone.
#
# USAGE
#     ./scripts/bootstrap-env.sh
#     source scripts/rust-env.sh
#     source scripts/sysroot-env.sh
#     cd src-tauri && cargo test --lib
#
# NOTE ON THE CARGO REGISTRY
#
# The snapshot can restore a partially-extracted crate registry, which
# fails with "can't find library `cairo_sys`, rename file to src/lib.rs".
# That is a truncated extraction, not a broken crate. Clearing
# registry/src forces cargo to re-extract from the cached .crate files.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_HOME_DIR="${ORION_CARGO_HOME:-$HOME/cargo_home}"
SYSROOT_DIR="${ORION_SYSROOT:-$HOME/sysroot}"

echo "=== 1/4 Rust toolchain ==="
mkdir -p "$CARGO_HOME_DIR"
if [ -z "$(ls -d "$CARGO_HOME_DIR/.rustup"/toolchains/*/bin/cargo 2>/dev/null)" ]; then
  chmod +x "$CARGO_HOME_DIR/bin/rustup" 2>/dev/null || true
  export CARGO_HOME="$CARGO_HOME_DIR"
  export RUSTUP_HOME="$CARGO_HOME_DIR/.rustup"
  "$CARGO_HOME_DIR/bin/rustup" set profile minimal 2>&1 | tail -2
  "$CARGO_HOME_DIR/bin/rustup" default stable 2>&1 | tail -3
else
  echo "  already present"
fi

echo
echo "=== 2/4 Cargo registry ==="
if [ -d "$CARGO_HOME_DIR/registry/src" ]; then
  # A truncated extraction fails to parse the manifest. Re-extract from the
  # cached .crate archives; cargo re-downloads anything missing.
  bad=0
  for manifest in "$CARGO_HOME_DIR"/registry/src/*/*/Cargo.toml; do
    [ -f "$manifest" ] || continue
    dir="$(dirname "$manifest")"
    [ -d "$dir/src" ] || { echo "  truncated: $(basename "$dir")"; bad=1; }
  done
  if [ "$bad" -eq 1 ]; then
    rm -rf "$CARGO_HOME_DIR/registry/src"
    echo "  cleared truncated extraction"
  else
    echo "  ok"
  fi
else
  echo "  not unpacked yet"
fi

echo
echo "=== 3/4 Build sysroot ==="
if [ -e "$SYSROOT_DIR/root/usr/lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so" ]; then
  echo "  already present"
else
  bash "$REPO/scripts/setup-sysroot.sh" 2>&1 | tail -25
fi

echo
echo "=== 4/4 node modules ==="
cd "$REPO"
if [ -d node_modules ]; then
  echo "  already present"
else
  npm install --no-audit --no-fund 2>&1 | tail -4
fi

echo
echo "=== BOOTSTRAP COMPLETE ==="
echo "  source scripts/rust-env.sh && source scripts/sysroot-env.sh"
echo "  cd src-tauri && cargo test --lib"
