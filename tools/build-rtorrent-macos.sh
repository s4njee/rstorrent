#!/usr/bin/env bash
#
# Build a self-contained rtorrent for bundling inside the macOS app.
#
# rstorrent ships its own rtorrent so the app does not depend on whatever the
# host happens to have installed (Homebrew currently provides 0.16.x). This
# builds rtorrent and libtorrent from source at a pinned tag — their versions
# lock to each other, so both use $VERSION — and produces a relocatable runtime
# whose non-system dylibs sit next to the executable and are referenced through
# `@executable_path`.
#
# The output lands in binaries/rtorrent-macos/, which crates/gpui/build.rs
# copies beside a cargo-built binary and tools/bundle-gpui-macos.sh copies into
# the .app's Resources. At runtime `daemon::start` prefers it and falls back to
# a system rtorrent.
#
# Version note: 0.15.7 predates libtorrent's `verify_libcurl_internal_wakeup`
# assertion, so it does not hit the libcurl/eventfd abort that affected 0.16.x
# on current Linux distros — the reason tools/wsl-setup-rtorrent.sh pins 0.16.18
# there. macOS uses curl's real socketpair rather than the eventfd shim anyway.
#
# Usage:
#   tools/build-rtorrent-macos.sh                 # v0.15.7, host arch
#   RTORRENT_VERSION=v0.16.18 tools/build-rtorrent-macos.sh
#
# The runtime is built for the host architecture, matching the app it bundles
# into.

set -euo pipefail

VERSION="${RTORRENT_VERSION:-v0.15.7}"
ARCH="$(uname -m)"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="$REPO_ROOT/binaries/rtorrent-macos"
BUILD_DIR="${BUILD_DIR:-$HOME/.cache/rstorrent-build}"
PREFIX="$BUILD_DIR/prefix-$ARCH"
JOBS="$(sysctl -n hw.ncpu)"
# Re-running must not have to rebuild libtorrent from scratch, so each project
# leaves a version stamp; set RTORRENT_FORCE_REBUILD=1 to ignore it.
FORCE_REBUILD="${RTORRENT_FORCE_REBUILD:-0}"

say() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }

if [ "$(uname -s)" != "Darwin" ]; then
  echo "This script builds the macOS runtime; run it on macOS." >&2
  exit 1
fi

case "$ARCH" in
  arm64|x86_64) ;;
  *) echo "Unsupported CPU architecture '$ARCH'" >&2; exit 1 ;;
esac
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"

# --- 1. Toolchain and libraries -------------------------------------------
say "Installing build dependencies"
brew install autoconf automake libtool pkg-config curl openssl@3 ncurses tinyxml2

CURL_PREFIX="$(brew --prefix curl)"
OPENSSL_PREFIX="$(brew --prefix openssl@3)"
NCURSES_PREFIX="$(brew --prefix ncurses)"
TINYXML2_PREFIX="$(brew --prefix tinyxml2)"

# Homebrew's curl/openssl/ncurses are keg-only, so their pkg-config files and
# headers have to be pointed at explicitly.
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:$CURL_PREFIX/lib/pkgconfig:$OPENSSL_PREFIX/lib/pkgconfig:$NCURSES_PREFIX/lib/pkgconfig:$TINYXML2_PREFIX/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export CPPFLAGS="-I$CURL_PREFIX/include -I$OPENSSL_PREFIX/include -I$NCURSES_PREFIX/include -I$TINYXML2_PREFIX/include ${CPPFLAGS:-}"
export LDFLAGS="-L$CURL_PREFIX/lib -L$OPENSSL_PREFIX/lib -L$NCURSES_PREFIX/lib -L$TINYXML2_PREFIX/lib ${LDFLAGS:-}"

# --- 2. Build libtorrent and rtorrent into a private prefix ----------------
# libtorrent is built static so only rtorrent and its system-near dependencies
# (curl, openssl, ncurses, zlib) need to travel in the bundle.
build_project() {
  local repo="$1"; shift
  local src="$BUILD_DIR/$repo"
  local configure_args=("$@")
  local stamp="$PREFIX/.built-$repo-$VERSION"

  if [ "$FORCE_REBUILD" != "1" ] && [ -f "$stamp" ]; then
    say "Using cached $repo $VERSION"
    return 0
  fi

  say "Building $repo $VERSION ($ARCH)"
  mkdir -p "$BUILD_DIR"
  if [ -d "$src/.git" ]; then
    git -C "$src" fetch --tags --quiet origin
  else
    git clone --quiet "https://github.com/rakshasa/$repo.git" "$src"
  fi
  git -C "$src" checkout --quiet "$VERSION"

  cd "$src"
  # A stale tree from a different tag would otherwise poison the build.
  git clean -xfdq
  # Neither repo ships a `configure` at these tags.
  autoreconf -ivf >/dev/null
  ./configure --prefix="$PREFIX" "${configure_args[@]}"
  make -j"$JOBS"
  make install
  mkdir -p "$PREFIX"
  touch "$stamp"
}

build_project libtorrent \
  --disable-shared --enable-static \
  --with-openssl="$OPENSSL_PREFIX"

# Without an XML-RPC backend rtorrent opens the SCGI port but answers every call
# with "XML-RPC not supported" — the entire interface this client speaks.
build_project rtorrent --with-xmlrpc-tinyxml2

# --- 3. Collect the runtime into the bundle directory ---------------------
say "Collecting runtime into binaries/rtorrent-macos"
# Clear only what this script generates. The directory also holds the committed
# README, so it must survive a re-run.
mkdir -p "$OUT_DIR"
rm -f "$OUT_DIR/rtorrent" "$OUT_DIR"/*.dylib
cp "$PREFIX/bin/rtorrent" "$OUT_DIR/rtorrent"
chmod u+w "$OUT_DIR/rtorrent"

# Dylibs that ship with macOS (or live in the SDK) must never be copied — they
# are guaranteed present and re-signing them would only break things.
is_system_lib() {
  # `@executable_path/...` is ours; Homebrew and the private prefix are not.
  case "$1" in
    /usr/lib/*|/System/*) return 0 ;;
    /opt/homebrew/*|/usr/local/*|"$PREFIX"/*) return 1 ;;
    *.dylib|*.so) return 1 ;;
    *) return 0 ;;
  esac
}

# Resolve a dependency path as written in an LC_LOAD_DYLIB entry to an absolute
# path on disk. `@rpath`/`@loader_path` entries are resolved against the RPATHs
# the binary actually carries.
resolve_dep() {
  local owner="$1" dep="$2" rpath dir
  dir="$(dirname "$owner")"
  case "$dep" in
    @rpath/*)
      local name="${dep#@rpath/}"
      # `@rpath` is resolved against the file's own LC_RPATHs, and `@loader_path`
      # is relative to that file's directory — which is why callers must pass
      # the *original* (pre-copy) path, not the staged one.
      while IFS= read -r rpath; do
        rpath="${rpath//@loader_path/$dir}"
        rpath="${rpath//@executable_path/$dir}"
        if [ -f "$rpath/$name" ]; then echo "$rpath/$name"; return; fi
      done < <(otool -l "$owner" | awk '/LC_RPATH/{getline; getline; print $2}')
      return 1
      ;;
    @loader_path/*) echo "$dir/${dep#@loader_path/}" ;;
    /*) echo "$dep" ;;
    *) return 1 ;;
  esac
}

# Transitively copy every non-system dylib the runtime needs; Homebrew dylibs
# pull in others (brotli → brotlicommon, curl → openssl → …), several by
# `@rpath`. The worklist holds the *original* paths so those references still
# resolve against the location they were built for; the staged copy is just
# `$OUT_DIR/$(basename)`. An indexed list with a cursor rather than a queue
# slice: bash 3.2 (macOS's default) has neither associative arrays nor safe
# empty-array expansion under `set -u`.
ORIG_LIST=("$PREFIX/bin/rtorrent")
cursor=0
while [ "$cursor" -lt "${#ORIG_LIST[@]}" ]; do
  owner="${ORIG_LIST[$cursor]}"
  cursor=$((cursor + 1))
  while IFS= read -r dep; do
    [ -n "$dep" ] || continue
    resolved="$(resolve_dep "$owner" "$dep" || true)"
    [ -n "$resolved" ] || continue
    is_system_lib "$resolved" && continue
    base="$(basename "$resolved")"
    dest="$OUT_DIR/$base"
    if [ ! -f "$dest" ]; then
      cp "$resolved" "$dest"
      chmod u+w "$dest"
      ORIG_LIST[${#ORIG_LIST[@]}]="$resolved"
    fi
  done < <(otool -L "$owner" | tail -n +2 | awk '{print $1}')
done

# Rewrite every load command to point at the sibling copy, then ad-hoc sign
# (install_name_tool invalidates any signature).
say "Rewriting install names for @executable_path"
rewrite() {
  local file="$1"
  while IFS= read -r dep; do
    [ -n "$dep" ] || continue
    base="$(basename "$dep")"
    if [ -f "$OUT_DIR/$base" ] && [ "$dep" != "@executable_path/$base" ]; then
      install_name_tool -change "$dep" "@executable_path/$base" "$file"
    fi
  done < <(otool -L "$file" | tail -n +2 | awk '{print $1}')

  if [ "$file" != "$OUT_DIR/rtorrent" ] && [[ "$file" == *.dylib ]]; then
    install_name_tool -id "@executable_path/$(basename "$file")" "$file"
  fi
}
rewrite "$OUT_DIR/rtorrent"
for lib in "$OUT_DIR"/*.dylib; do
  [ -e "$lib" ] && rewrite "$lib"
done
for f in "$OUT_DIR"/*.dylib; do
  [ -e "$f" ] && codesign --force --sign - --timestamp=none "$f" >/dev/null 2>&1 || true
done
codesign --force --sign - --timestamp=none "$OUT_DIR/rtorrent" >/dev/null 2>&1 || true

# --- 4. Verify it is actually relocatable ---------------------------------
say "Verifying the runtime"
for f in "$OUT_DIR/rtorrent" "$OUT_DIR"/*.dylib; do
  [ -e "$f" ] || continue
  if otool -L "$f" | tail -n +2 | awk '{print $1}' \
      | grep -vE '^(@executable_path/|/usr/lib/|/System/)' ; then
    echo "error: $(basename "$f") still links against a non-bundled library" >&2
    exit 1
  fi
done

# rtorrent has no `--version`; `-h` prints usage and exits, which is the only
# safe way to prove the binary loads every dylib outside the build prefix. dyld
# reports a missing library on stderr, so surface that rather than a bare fail.
if ! smoke="$("$OUT_DIR/rtorrent" -h 2>&1)"; then
  echo "error: the bundled rtorrent does not run:" >&2
  echo "$smoke" >&2
  exit 1
fi
echo "  rtorrent $VERSION ($ARCH) built and runnable"

cat <<DONE

Done.

  Runtime   $OUT_DIR/rtorrent
  Version   $VERSION ($ARCH)
  Bundle    tools/bundle-gpui-macos.sh copies it into the .app's Resources;
            daemon::start prefers it over a system install.

DONE
