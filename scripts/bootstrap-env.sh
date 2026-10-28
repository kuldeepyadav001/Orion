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
# It also drops two things that are not large but are load-bearing:
#
#   * .git/config     - so the `origin` remote disappears and the repository
#                       looks like it has never been connected
#   * ~/.ssh file modes - the private key comes back world-readable, which
#                       ssh refuses to use
#
# Running this script restores all of it. It is idempotent: anything
# already present is left alone.
#
# WHERE THE HEAVY STUFF GOES
#
# Everything large lands under $ORION_BUILD_ROOT (default
# /usr/local/orion-build), on the container filesystem rather than under
# $HOME. The workspace snapshot carries /home/user and nothing else, and it
# has a budget of 10,000 files / 128 MB. A Rust toolchain plus its
# extracted crate registry is ~16,000 files; a sysroot is another 640.
# Keeping either under $HOME makes the snapshot truncate itself and
# silently drop unrelated work.
#
# USAGE
#     ./scripts/bootstrap-env.sh
#     source scripts/rust-env.sh
#     source scripts/sysroot-env.sh
#     cd src-tauri && cargo test --lib
#
# NOTE ON THE CARGO REGISTRY
#
# A partially-extracted crate registry fails with "can't find library
# `cairo_sys`, rename file to src/lib.rs". That is a truncated extraction,
# not a broken crate. Clearing registry/src forces cargo to re-extract from
# the cached .crate files.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ORION_BUILD_ROOT="${ORION_BUILD_ROOT:-/usr/local/orion-build}"
CARGO_HOME_DIR="${ORION_CARGO_HOME:-$ORION_BUILD_ROOT/cargo}"
SYSROOT_DIR="${ORION_SYSROOT:-$ORION_BUILD_ROOT/sysroot}"
GIT_REMOTE="${ORION_GIT_REMOTE:-git@github.com:kuldeepyadav001/Orion.git}"

echo "build root: $ORION_BUILD_ROOT"
echo

echo "=== 1/6 Git remote and SSH key ==="
cd "$REPO"
if [ -d .git ]; then
  if ! git remote get-url origin >/dev/null 2>&1; then
    git remote add origin "$GIT_REMOTE"
    echo "  restored remote origin -> $GIT_REMOTE"
  else
    echo "  origin already configured"
  fi
  # The snapshot strips the committer identity too, and a commit made
  # without one fails with "empty ident name ... not allowed".
  if [ -z "$(git config --local user.email 2>/dev/null)" ]; then
    git config --local user.name "${ORION_GIT_USER:-kuldeepyadav001}"
    git config --local user.email "${ORION_GIT_EMAIL:-kuldeepyadav@orion.ai}"
    echo "  restored committer identity"
  fi
fi
# The snapshot restores file modes loosely; ssh refuses a group- or
# world-readable private key, so the push fails with a misleading
# "repository does not exist" error.
if [ -f "$HOME/.ssh/id_ed25519" ]; then
  chmod 700 "$HOME/.ssh" 2>/dev/null || true
  chmod 600 "$HOME/.ssh/id_ed25519" 2>/dev/null || true
  chmod 644 "$HOME/.ssh/id_ed25519.pub" 2>/dev/null || true
  echo "  ssh key permissions: 0600"
fi

echo
echo "=== 2/6 Rust toolchain ==="
mkdir -p "$CARGO_HOME_DIR"
if [ -z "$(ls -d "$CARGO_HOME_DIR/.rustup"/toolchains/*/bin/cargo 2>/dev/null)" ]; then
  if [ ! -x "$CARGO_HOME_DIR/bin/rustup" ]; then
    # The rustup shim itself is a 21 MB binary. Keep it out of the
    # workspace too.
    echo "  fetching rustup ..."
    mkdir -p "$CARGO_HOME_DIR/bin"
    curl -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh 2>/dev/null \
      && CARGO_HOME="$CARGO_HOME_DIR" RUSTUP_HOME="$CARGO_HOME_DIR/.rustup" \
         sh /tmp/rustup-init.sh -y --no-modify-path --profile minimal \
             --default-toolchain stable >/dev/null 2>&1
  fi
  export CARGO_HOME="$CARGO_HOME_DIR"
  export RUSTUP_HOME="$CARGO_HOME_DIR/.rustup"
  "$CARGO_HOME_DIR/bin/rustup" set profile minimal 2>&1 | tail -2
  "$CARGO_HOME_DIR/bin/rustup" default stable 2>&1 | tail -3
else
  echo "  already present"
fi

echo
echo "=== 3/6 Cargo registry ==="
if [ -d "$CARGO_HOME_DIR/registry/src" ]; then
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
echo "=== 4/6 Build sysroot ==="
if [ -e "$SYSROOT_DIR/root/usr/lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so" ]; then
  echo "  already present"
else
  ORION_SYSROOT="$SYSROOT_DIR" bash "$REPO/scripts/setup-sysroot.sh" 2>&1 | tail -25
fi

echo
echo "=== 5/6 node modules ==="
cd "$REPO"
if [ -d node_modules ]; then
  echo "  already present"
else
  npm install --no-audit --no-fund 2>&1 | tail -4
fi

echo
echo "=== 6/6 Sidecar binaries ==="
if [ -d "$REPO/src-tauri/binaries" ]; then
  echo "  already present"
else
  echo "  absent - run scripts/fetch-sidecars.sh if the app needs to launch"
fi

echo
echo "=== BOOTSTRAP COMPLETE ==="
echo "  source scripts/rust-env.sh && source scripts/sysroot-env.sh"
echo "  cd src-tauri && cargo test --lib"
