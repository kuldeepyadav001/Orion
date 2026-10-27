#!/usr/bin/env bash
#
# Put the sandbox's Rust toolchain on PATH.
#
# The image ships no rustc, and rustup's proxy shims are not created in this
# filesystem, so the toolchain's own bin directory is added directly.
#
# The image already exports CARGO_HOME and RUSTUP_HOME, and the exported
# RUSTUP_HOME does not necessarily hold the toolchain — so every candidate
# root is probed and the first one with a real cargo binary wins.
#
# USAGE
#     source scripts/rust-env.sh
#     source scripts/sysroot-env.sh   # also needed for the GUI dev libs
#     cd src-tauri && cargo test --lib

export CARGO_HOME="${CARGO_HOME:-$HOME/cargo_home}"

for _root in "$HOME/cargo_home/.rustup" "$RUSTUP_HOME" "$HOME/.rustup" "$HOME/rustup_home"; do
  [ -n "$_root" ] || continue
  for _d in "$_root"/toolchains/*/bin; do
    if [ -x "$_d/cargo" ]; then
      export RUSTUP_HOME="$_root"
      export PATH="$_d:$PATH"
      break 2
    fi
  done
done

unset _root _d
