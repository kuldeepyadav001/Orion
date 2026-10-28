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

SYSROOT="${ORION_SYSROOT:-${ORION_BUILD_ROOT:-/usr/local/orion-build}/sysroot}"
DEBS="$SYSROOT/debs"
ROOT="$SYSROOT/root"
MIRROR_MAIN="http://deb.debian.org/debian/pool/main"
MIRROR_SEC="http://deb.debian.org/debian-security/pool/updates/main"

mkdir -p "$DEBS" "$ROOT"

# The roots of the closure. Only the -dev packages are named explicitly;
# everything they pull in — headers, runtime .so files, their own
# dependencies — is resolved from apt's index below.
#
# The runtime packages matter as much as the headers. A -dev package ships
# the `libfoo.so` symlink but not `libfoo.so.0`; unpack the -dev package
# alone and the symlink dangles, and the linker fails with
# "unable to find library -lfoo" even though pkg-config reported OK.
ROOT_PACKAGES=(
  libwebkit2gtk-4.1-dev
  libjavascriptcoregtk-4.1-dev
  libgtk-3-dev
  libsoup-3.0-dev
  libglib2.0-dev
  libatk1.0-dev
  libatspi2.0-dev
  libatk-bridge2.0-dev
  libcairo2-dev
  libpango1.0-dev
  libgdk-pixbuf-2.0-dev
  libharfbuzz-dev
  libasound2-dev
  libepoxy-dev
  libpixman-1-dev
  libxml2-dev
  libsqlite3-dev
  libnghttp2-dev
  libpsl-dev
  zlib1g-dev
  libffi-dev
  libmount-dev
  libblkid-dev
  libselinux1-dev
  libpcre2-dev
  libgraphene-1.0-dev
  libgirepository1.0-dev
  libfontconfig-dev
  libfreetype-dev
  libpng-dev
  libcloudproviders-dev
  libgl-dev
  libegl-dev
  libglvnd-dev
  libgl1-mesa-dev
  libdbus-1-dev
  libsystemd-dev
  libx11-dev
  libxext-dev
  libxi-dev
  libxfixes-dev
  libxrandr-dev
  libxcursor-dev
  libxinerama-dev
  libxrender-dev
  libxcb1-dev
  libxau-dev
  libxdmcp-dev
  libxcomposite-dev
  libxdamage-dev
  libxkbcommon-dev
  libxres-dev
  libxtst-dev
  libwayland-dev
)

# Pool-path form, retained for the fallback path when apt-get is absent.
PACKAGES=(
  "SEC:w/webkit2gtk:libwebkit2gtk-4.1-dev"
  "SEC:w/webkit2gtk:libjavascriptcoregtk-4.1-dev"
  "MAIN:g/gtk+3.0:libgtk-3-dev"
  "MAIN:libs/libsoup3:libsoup-3.0-dev"
  "MAIN:g/glib2.0:libglib2.0-dev"
  "MAIN:a/atk1.0:libatk1.0-dev"
  "MAIN:a/at-spi2-core:libatspi2.0-dev"
  "MAIN:a/at-spi2-atk:libatk-bridge2.0-dev"
  "MAIN:c/cairo:libcairo2-dev"
  "MAIN:p/pango1.0:libpango1.0-dev"
  "MAIN:g/gdk-pixbuf:libgdk-pixbuf-2.0-dev"
  "MAIN:h/harfbuzz:libharfbuzz-dev"
  "MAIN:a/alsa-lib:libasound2-dev"
  "MAIN:l/libepoxy:libepoxy-dev"
  "MAIN:p/pixman:libpixman-1-dev"
  "MAIN:libx/libxml2:libxml2-dev"
  "MAIN:s/sqlite3:libsqlite3-dev"
  "MAIN:n/nghttp2:libnghttp2-dev"
  "MAIN:p/libpsl:libpsl-dev"
  "MAIN:z/zlib:zlib1g-dev"
  "MAIN:f/libffi:libffi-dev"
  "MAIN:u/util-linux:libmount-dev"
  "MAIN:u/util-linux:libblkid-dev"
  "MAIN:libs/libselinux:libselinux1-dev"
  "MAIN:p/pcre2:libpcre2-dev"
  "MAIN:g/graphene:libgraphene-1.0-dev"
  "MAIN:g/gobject-introspection:libgirepository1.0-dev"
  "MAIN:f/fontconfig:libfontconfig-dev"
  "MAIN:f/freetype:libfreetype-dev"
  "MAIN:libx/libpng1.6:libpng-dev"
  "MAIN:c/libcloudproviders:libcloudproviders-dev"
  "MAIN:libg/libglvnd:libgl-dev"
  "MAIN:libg/libglvnd:libegl-dev"
  "MAIN:libg/libglvnd:libglvnd-dev"
  "MAIN:m/mesa:libgl1-mesa-dev"
  "MAIN:d/dbus:libdbus-1-dev"
  "MAIN:s/systemd:libsystemd-dev"
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
)

echo "Resolving and downloading into $DEBS ..."
#
# WHY THE DEPENDENCY CLOSURE INSTEAD OF A FLAT LIST
#
# `apt-get download <pkg>` fetches exactly one .deb. It does not pull in
# dependencies. A flat list of -dev packages therefore yields a sysroot
# whose headers are present but whose runtime shared objects are not, and
# every `libfoo.so` symlink dangles. pkg-config reports success — the
# symlink exists — and the failure only appears at link time as
# "unable to find library -lfoo", which is a confusing place to discover
# that the problem was the download step.
#
# `apt-cache depends --recurse` expands the roots into their full closure
# against one suite, so every package comes from the same Debian release
# and every runtime library arrives with its header.
#
# This also fixes the Debian 13 renames (libfoo-0 -> libfoo-0t64) for
# free: apt knows the new names, a pool scrape does not.
if command -v apt-get >/dev/null 2>&1 && command -v apt-cache >/dev/null 2>&1; then
  CLOSURE=$(apt-cache depends --recurse \
      --no-recommends --no-suggests --no-conflicts --no-breaks \
      --no-replaces --no-enhances \
      "${ROOT_PACKAGES[@]}" 2>/dev/null \
    | grep -E '^[a-zA-Z0-9+.-]+$' | sort -u)

  echo "  closure: $(printf '%s\n' "$CLOSURE" | wc -l) packages"

  # Resolve every package to its pool path and published SHA-256 in a
  # single index query, then fetch the .deb files in parallel.
  #
  # `apt-get download <pkg>` is far too slow for a closure this size: it
  # starts a resolver per invocation, so 700 packages take ~15 minutes of
  # pure process startup. `apt-cache show` answers for the whole list in
  # one pass, and curl fetches the files concurrently.
  #
  # The SHA-256 is not optional. A silently truncated .deb unpacks into a
  # directory that looks correct and then fails at link time.
  echo "  resolving pool paths ..."
  apt-cache show --no-all-versions $CLOSURE 2>/dev/null > "$DEBS/.index"

  python3 - "$DEBS" <<'PYEOF' > "$DEBS/.fetchlist"
import io, re, sys

debs = sys.argv[1]
blocks = io.open(debs + "/.index", encoding="utf-8", errors="replace").read().split("\n\n")
seen = set()
for b in blocks:
    fn = re.search(r"^Filename: (.+)$", b, re.M)
    sh = re.search(r"^SHA256: ([0-9a-f]{64})$", b, re.M)
    if not (fn and sh):
        continue
    name = fn.group(1).rsplit("/", 1)[-1]
    if name in seen:
        continue
    seen.add(name)
    print(f"{fn.group(1)}\t{sh.group(1)}\t{name}")
PYEOF

  cat > "$DEBS/.fetch.sh" <<'FETCHEOF'
#!/bin/bash
# Fetch one .deb, preferring the exact version the index names.
#
#   $1 = "pool/path\tsha256\tfilename"
#   $2 = the debs directory
#
# The index and the pool drift apart: a point release can be superseded
# and removed from the pool while the index still names it. When that
# happens the exact URL 404s, so fall back to the newest file the pool
# still holds for the same package. A substituted file is recorded, and
# the substitution is reported by the caller — it is a patch release of
# the same upstream version, not a different library.
IFS=$'\t' read -r path sha name <<< "$1"
out="$2/$name"
[ -f "$out" ] && exit 0

verify() {
  [ "$(sha256sum "$1" | cut -d' ' -f1)" = "$sha" ]
}

for base in "http://deb.debian.org/debian" \
            "http://security.debian.org/debian-security" \
            "http://deb.debian.org/debian-security"; do
  if curl -sSL --fail --retry 2 -o "$out.tmp" "$base/$path" 2>/dev/null && verify "$out.tmp"; then
    mv "$out.tmp" "$out"
    exit 0
  fi
  rm -f "$out.tmp"
done

# Fallback: the pool has a newer point release of the same package.
prefix="${name%%_*}"
dir="$(dirname "$path")"
for base in "http://deb.debian.org/debian" \
            "http://security.debian.org/debian-security"; do
  cand=$(curl -s "$base/$dir/" 2>/dev/null \
    | grep -oE "${prefix}_[^\"]*_(amd64|all)\.deb" | sort -V | tail -1)
  [ -n "$cand" ] || continue
  if curl -sSL --fail --retry 2 -o "$out.tmp" "$base/$dir/$cand" 2>/dev/null; then
    if dpkg-deb -I "$out.tmp" >/dev/null 2>&1; then
      mv "$out.tmp" "$out"
      echo "$name -> $cand" >> "$2/.substituted"
      exit 0
    fi
  fi
  rm -f "$out.tmp"
done

echo "$name" >> "$2/.failed"
exit 0
FETCHEOF
  chmod +x "$DEBS/.fetch.sh"
  rm -f "$DEBS/.failed" "$DEBS/.substituted"

  echo "  downloading ..."
  # `set -e` is active, so the pipeline's status is explicitly discarded:
  # a single unresolvable package must not abort the whole build.
  xargs -P 16 -d '\n' -I{} "$DEBS/.fetch.sh" "{}" "$DEBS" < "$DEBS/.fetchlist" || true

  rm -f "$DEBS/.index" "$DEBS/.fetchlist" "$DEBS/.fetch.sh"
  if [ -s "$DEBS/.substituted" ]; then
    echo "  NOTE: $(wc -l < "$DEBS/.substituted") package(s) substituted with a newer point release:"
    sed 's/^/    /' "$DEBS/.substituted"
  fi
  rm -f "$DEBS/.substituted"
  if [ -s "$DEBS/.failed" ]; then
    echo "  WARNING: $(wc -l < "$DEBS/.failed") package(s) could not be fetched:"
    sed 's/^/    /' "$DEBS/.failed"
  fi
  rm -f "$DEBS/.failed"
  echo "  downloaded: $(ls "$DEBS"/*.deb 2>/dev/null | wc -l) .deb files"
else
  echo "  apt-get unavailable — falling back to pool scraping."
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
fi

echo "Unpacking ..."
for d in "$DEBS"/*.deb; do
  [ -f "$d" ] && dpkg-deb -x "$d" "$ROOT" 2>/dev/null || true
done

export PKG_CONFIG_PATH="$ROOT/usr/lib/x86_64-linux-gnu/pkgconfig:$ROOT/usr/share/pkgconfig"
export PKG_CONFIG_SYSROOT_DIR="$ROOT"

echo
echo "Verifying:"
ok=0
for p in glib-2.0 gobject-2.0 gio-2.0 gtk+-3.0 gdk-3.0 libsoup-3.0 \
         javascriptcoregtk-4.1 webkit2gtk-4.1 alsa; do
  if pkg-config --exists "$p"; then
    echo "  OK   $p ($(pkg-config --modversion "$p"))"
  else
    echo "  MISS $p"
    ok=1
  fi
done

# A dangling .so symlink means the -dev package arrived without its
# runtime partner. pkg-config will still say OK, so check explicitly.
# Only shared objects matter here: some packages (aspell, ispell) ship
# dangling dictionary symlinks that have nothing to do with linking.
echo
echo "Checking for dangling shared objects ..."
dangling=$(find "$ROOT/usr/lib" "$ROOT/lib" -type l -name '*.so*' \
             ! -exec test -e {} \; -print 2>/dev/null | wc -l)
if [ "$dangling" -ne 0 ]; then
  echo "  $dangling dangling .so symlink(s):"
  find "$ROOT/usr/lib" "$ROOT/lib" -type l -name '*.so*' \
       ! -exec test -e {} \; -print 2>/dev/null | head -20 | sed 's/^/    /'
  ok=1
else
  echo "  OK   no dangling .so symlinks"
fi

# The link-time libraries the Tauri crate actually asks for.
echo
echo "Checking link libraries ..."
for so in libgtk-3.so libgdk-3.so libwebkit2gtk-4.1.so libjavascriptcoregtk-4.1.so \
          libsoup-3.0.so libatk-1.0.so libglib-2.0.so libgobject-2.0.so \
          libasound.so libdbus-1.so; do
  if [ -e "$ROOT/usr/lib/x86_64-linux-gnu/$so" ]; then
    echo "  OK   $so"
  else
    echo "  MISS $so"
    ok=1
  fi
done

if [ "$ok" -ne 0 ]; then
  echo
  echo "Some packages did not resolve. Debian moves package names between"
  echo "releases (libfoo-0 -> libfoo-0t64 etc.); check the missing name in"
  echo "the pool listing and add it to ROOT_PACKAGES above."
  exit 1
fi

echo
echo "Sysroot ready. Now run:  source ./scripts/sysroot-env.sh"
