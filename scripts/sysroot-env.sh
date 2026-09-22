# Source this (do not execute) to point cargo at the local sysroot built by
# ./scripts/setup-sysroot.sh
#
#     source ./scripts/sysroot-env.sh
#     cd src-tauri && cargo test
#
# RUSTFLAGS carries -L as well as PKG_CONFIG_*: pkg-config supplies the
# headers and link *names*, but the linker still needs the directory holding
# the actual .so files. Without it the build type-checks and then fails at
# link time with "unable to find library -latk-1.0", which is a confusing
# place to end up after everything appeared to resolve.

_ORION_SYSROOT="${ORION_SYSROOT:-$HOME/sysroot}/root"

export PKG_CONFIG_PATH="$_ORION_SYSROOT/usr/lib/x86_64-linux-gnu/pkgconfig:$_ORION_SYSROOT/usr/share/pkgconfig"
export PKG_CONFIG_SYSROOT_DIR="$_ORION_SYSROOT"
export LD_LIBRARY_PATH="$_ORION_SYSROOT/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export RUSTFLAGS="-L $_ORION_SYSROOT/usr/lib/x86_64-linux-gnu${RUSTFLAGS:+ $RUSTFLAGS}"

echo "sysroot: $_ORION_SYSROOT"
