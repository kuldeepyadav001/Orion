#!/usr/bin/env bash
#
# Build a local sysroot so the Tauri crate can be compiled without root.
#
# WHY THIS EXISTS
#
# Tauri needs libwebkit2gtk-4.1-dev and its dependency closure. In a sandbox
# with no root there is no `apt-get install`, which left `tauri_glue.rs`
# uncompiled through all of M3 — the code was written, reviewed against
# vendored crate sources, and shipped without a compiler ever seeing it. That
# is exactly how the `event.state()` bug survived review.
#
# `dpkg-deb -x` unpacks a .deb into any directory without root, and pkg-config
# understands PKG_CONFIG_SYSROOT_DIR. That is enough to type-check, lint and
# run the full test suite.
#
# USAGE
#     ./scripts/setup-sysroot.sh          # download + unpack (~10 min cold)
#     source ./scripts/sysroot-env.sh     # export the vars into your shell
#     cd src-tauri && cargo test
#
# LIMITS
# This gives you compile, clippy and unit tests. It does NOT give you a
# running app: there is no display server, so `cargo tauri dev` still cannot
# run here, and nothing below has been executed against a real desktop.

set -euo pipefail

SYSROOT="${ORION_SYSROOT:-$HOME/.sysroot}"
DEBS="$SYSROOT/debs"
ROOT="$SYSROOT/root"
MIRROR_MAIN="http://deb.debian.org/debian/pool/main"
MIRROR_SEC="http://deb.debian.org/debian-security/pool/updates/main"

mkdir -p "$DEBS" "$ROOT"

# pool-path:package-name
# Split into dev (headers + .pc) and runtime (.so) because linking needs both
# and the -dev packages alone are not enough — a lesson from -latk-1.0 not
# being found after everything appeared to resolve.
PACKAGES=(
  # webkit + javascriptcore live in the security pool
  "SEC:w/webkit2gtk:libwebkit2gtk-4.1-dev"
  "SEC:w/webkit2gtk:libwebkit2gtk-4.1-0"
  "SEC:w/webkit2gtk:libjavascriptcoregtk-4.1-dev"
  "SEC:w/webkit2gtk:libjavascriptcoregtk-4.1-0"

  # gtk stack, dev
  "MAIN:g/gtk+3.0:libgtk-3-dev"
  "MAIN:g/gtk+3.0:libgtk-3-0t64"
  "MAIN:libs/libsoup3:libsoup-3.0-dev"
  "MAIN:libs/libsoup3:libsoup-3.0-0"
  "MAIN:g/glib2.0:libglib2.0-dev"
  "MAIN:g/glib2.0:libglib2.0-0t64"
  "MAIN:a/atk1.0:libatk1.0-dev"
  "MAIN:a/atk1.0:libatk1.0-0"
  "MAIN:a/at-spi2-core:libatspi2.0-dev"
  "MAIN:a/at-spi2-core:libatspi2.0-0t64"
  "MAIN:a/at-spi2-atk:libatk-bridge2.0-dev"
  "MAIN:a/at-spi2-core:libatk-bridge2.0-0t64"
  "MAIN:c/cairo:libcairo2-dev"
  "MAIN:c/cairo:libcairo2"
  "MAIN:c/cairo:libcairo-gobject2"
  "MAIN:p/pango1.0:libpango1.0-dev"
  "MAIN:p/pango1.0:libpango-1.0-0"
  "MAIN:p/pango1.0:libpangocairo-1.0-0"
  "MAIN:g/gdk-pixbuf:libgdk-pixbuf-2.0-dev"
  "MAIN:g/gdk-pixbuf:libgdk-pixbuf-2.0-0"
  "MAIN:h/harfbuzz:libharfbuzz-dev"
  "MAIN:h/harfbuzz:libharfbuzz0b"

  # gdk-pixbuf now pulls glycin, which pulls seccomp and lcms2
  "MAIN:g/glycin:libglycin-2-dev"
  "MAIN:g/glycin:libglycin-2-0"
  "MAIN:libs/libseccomp:libseccomp-dev"
  "MAIN:l/lcms2:liblcms2-dev"
  "MAIN:s/shared-mime-info:shared-mime-info"

  # X11 and friends
  "MAIN:libx/libx11:libx11-dev"
  "MAIN:libx/libxext:libxext-dev"
  "MAIN:libx/libxi:libxi-dev"
  "MAIN:libx/libxfixes:libxfixes-dev"
  "MAIN:libx/libxrandr:libxrandr-dev"
  "MAIN:libx/libxcursor:libxcursor-dev"
  "MAIN:libx/libxinerama:libxinerama-dev"
  "MAIN:libx/libxrender:libxrender-dev"
  "MAIN:libx/libxcb:libxcb1-dev"
  "MAIN:libx/libxau:libxau-dev"
  "MAIN:libx/libxdmcp:libxdmcp-dev"
  "MAIN:libx/libxcomposite:libxcomposite-dev"
  "MAIN:libx/libxdamage:libxdamage-dev"
  "MAIN:libx/libxkbcommon:libxkbcommon-dev"
  "MAIN:libx/libxres:libxres-dev"
  "MAIN:libx/libxtst:libxtst-dev"
  "MAIN:w/wayland:libwayland-dev"

  # misc transitive
  "MAIN:libe/libepoxy:libepoxy-dev"
  "MAIN:p/pixman:libpixman-1-dev"
  "MAIN:libx/libxml2:libxml2-dev"
  "MAIN:s/sqlite3:libsqlite3-dev"
  "MAIN:n/nghttp2:libnghttp2-dev"
  "MAIN:libp/libpsl:libpsl-dev"
  "MAIN:z/zlib:zlib1g-dev"
  "MAIN:libf/libffi:libffi-dev"
  "MAIN:u/util-linux:libmount-dev"
  "MAIN:u/util-linux:libblkid-dev"
  "MAIN:libs/libselinux:libselinux1-dev"
  "MAIN:p/pcre2:libpcre2-dev"
  "MAIN:g/graphene:libgraphene-1.0-dev"
  "MAIN:g/gobject-introspection:libgirepository1.0-dev"
  "MAIN:f/fontconfig:libfontconfig-dev"
  "MAIN:f/freetype:libfreetype-dev"
  "MAIN:libp/libpng1.6:libpng-dev"
  "MAIN:libc/libcloudproviders:libcloudproviders-dev"
  "MAIN:libg/libglvnd:libgl-dev"
  "MAIN:libg/libglvnd:libegl-dev"
  "MAIN:libg/libglvnd:libglvnd-dev"
  "MAIN:m/mesa:libgl1-mesa-dev"
  "MAIN:d/dbus:libdbus-1-dev"
  "MAIN:s/systemd:libsystemd-dev"
)

echo "Resolving and downloading into $DEBS ..."
for entry in "${PACKAGES[@]}"; do
  IFS=':' read -r which pool pkg <<<"$entry"
  base="$MIRROR_MAIN"
  [ "$which" = "SEC" ] && base="$MIRROR_SEC"

  (
    listing=$(curl -s "$base/$pool/" || true)
    file=$(printf '%s' "$listing" | grep -o "${pkg}_[^\"]*_amd64\.deb" | grep -v dbgsym | sort -V | tail -1)
    if [ -z "$file" ]; then
      file=$(printf '%s' "$listing" | grep -o "${pkg}_[^\"]*_all\.deb" | sort -V | tail -1)
    fi
    if [ -z "$file" ]; then
      echo "  skip (not found): $pkg"
      exit 0
    fi
    if [ -f "$DEBS/$file" ]; then
      echo "  cached: $file"
      exit 0
    fi
    if curl -sSL --fail -o "$DEBS/$file" "$base/$pool/$file"; then
      echo "  got: $file"
    else
      rm -f "$DEBS/$file"
      echo "  FAILED: $pkg"
    fi
  ) &
done
wait

echo "Unpacking ..."
for d in "$DEBS"/*.deb; do
  [ -f "$d" ] && dpkg-deb -x "$d" "$ROOT" 2>/dev/null || true
done

export PKG_CONFIG_PATH="$ROOT/usr/lib/x86_64-linux-gnu/pkgconfig:$ROOT/usr/share/pkgconfig"
export PKG_CONFIG_SYSROOT_DIR="$ROOT"

echo
echo "Verifying:"
ok=0
for p in glib-2.0 gtk+-3.0 gdk-3.0 libsoup-3.0 javascriptcoregtk-4.1 webkit2gtk-4.1; do
  if pkg-config --exists "$p"; then
    echo "  OK   $p"
  else
    echo "  MISS $p -> $(pkg-config --cflags "$p" 2>&1 | grep -oP "Package '\K[^']+" | head -1)"
    ok=1
  fi
done

if [ "$ok" -ne 0 ]; then
  echo
  echo "Some packages did not resolve. Debian moves package names between"
  echo "releases (libfoo-0 -> libfoo-0t64 etc.); check the missing name in the"
  echo "pool listing and add it to PACKAGES above."
  exit 1
fi

echo
echo "Sysroot ready. Now run:  source ./scripts/sysroot-env.sh"
