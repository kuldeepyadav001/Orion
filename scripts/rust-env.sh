#!/usr/bin/env bash
#
# Put the sandbox's Rust toolchain on PATH.
#
# WHY THE TOOLCHAIN LIVES OUTSIDE $HOME
#
# The workspace snapshot carries /home/user and nothing else. A Rust
# toolchain is ~700 MB across ~8,000 files, and the extracted crate
# registry is another ~8,400 files. Together they are larger than the
# snapshot's own budget (10,000 files / 128 MB), so keeping them under
# $HOME does not merely waste space — it makes the snapshot truncate
# itself, silently dropping whatever else it was carrying. That is what
# caused work to disappear between sessions.
#
# $ORION_BUILD_ROOT therefore defaults to a path on the container
# filesystem, which persists across snapshot restores and is not counted
# against the workspace at all.
#
# USAGE
#     source scripts/rust-env.sh
#     source scripts/sysroot-env.sh   # also needed for the GUI dev libs
#     cd src-tauri && cargo test --lib

export ORION_BUILD_ROOT="${ORION_BUILD_ROOT:-/usr/local/orion-build}"
export CARGO_HOME="${CARGO_HOME:-$ORION_BUILD_ROOT/cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$CARGO_HOME/.rustup}"

# Resolve the installed toolchain directory without hard-coding a version.
for _root in "$CARGO_HOME/.rustup" "$RUSTUP_HOME" "$HOME/cargo_home/.rustup" \
             "$HOME/.rustup" "$HOME/rustup_home"; do
  [ -n "$_root" ] || continue
  for _d in "$_root"/toolchains/*/bin; do
    if [ -x "$_d/cargo" ]; then
      export RUSTUP_HOME="$_root"
      export PATH="$_d:$PATH"
      break 2
    fi
  done
done
export PATH

unset _root _d
