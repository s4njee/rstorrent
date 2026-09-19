#!/usr/bin/env bash
#
# Assemble a macOS .app for the GPUI shell, with the bundled rtorrent inside.
#
# The Tauri shell gets its .app from `tauri build`, which copies
# `src-tauri/binaries/rtorrent/` into the bundle because `tauri.conf.json`
# declares it as a resource. The GPUI shell has no bundler, so this is that
# bundler: it builds the binary, lays out the .app by hand, and puts the same
# staged runtime in `Contents/Resources/binaries/rtorrent/` — which is exactly
# where `daemon::bundled_rtorrent` looks first.
#
# This is the whole build, in one command from a fresh clone: it stages the
# bundled rtorrent (from source, cached after the first time), builds the web
# console the binary embeds, builds the binary, and assembles the .app.
#
# Prerequisites: the Rust toolchain, Node (for the frontend) and Homebrew with
# curl/openssl@3/ncurses/tinyxml2 — the same set tools/build-rtorrent-macos.sh
# needs, since it builds the runtime from source on the first run.
#
# Usage:
#   tools/bundle-gpui-macos.sh              # release build, dist-gpui/rstorrent-gpui.app
#   GPUI_PROFILE=debug tools/bundle-gpui-macos.sh
#
# Set FORCE_WEB_BUILD=1 to rebuild the console the app embeds even when its
# output looks current.
#
# The result is ad-hoc signed, not notarized: Gatekeeper warns on first launch
# on another machine, which is the same deal the Tauri build ships with (see
# docs/release.md).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="${GPUI_PROFILE:-release}"
BINARY="rstorrent-gpui"
APP_NAME="rstorrent-gpui.app"
STAGED_RUNTIME="$REPO_ROOT/src-tauri/binaries/rtorrent"
OUT_DIR="$REPO_ROOT/dist-gpui"
APP="$OUT_DIR/$APP_NAME"
CONTENTS="$APP/Contents"

say() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

if [ "$(uname -s)" != "Darwin" ]; then
  die "this script builds the macOS bundle; run it on macOS."
fi

# --- 1. The runtime has to exist before we can bundle it -------------------
# A bundle without its daemon still launches, but it silently falls back to a
# system rtorrent, which is not what this script exists to do — so this is the
# one step worth failing over. Building it is slow once and free afterwards:
# the staging script stamps each project it builds and re-stages from the
# cache, so a second run costs a few seconds.
if [ ! -f "$STAGED_RUNTIME/rtorrent" ]; then
  say "Staging the bundled rtorrent (first run builds it from source)"
  "$REPO_ROOT/tools/build-rtorrent-macos.sh"
fi

# --- 2. The web console the binary embeds ---------------------------------
# The server half of the app serves the console from dist-web via rust-embed,
# which reads the directory at *compile* time — so it has to exist before cargo
# runs, and it has to be current or the app serves a stale console.
#
# Staleness matters for speed, not just tidiness: any file that changes under
# dist-web makes rust-embed re-run and cargo rebuild the whole binary, so
# rebuilding it unconditionally would cost a two-minute link on every bundle.
# Set FORCE_WEB_BUILD=1 when the mtimes lie.
# The console's entry is web.html (see vite.web.config.ts), and the assets it
# pulls in are content-hashed, so that one file is the sentinel.
console_stale=0
if [ ! -f "$REPO_ROOT/dist-web/web.html" ]; then
  console_stale=1
elif [ -n "$(find "$REPO_ROOT/src" "$REPO_ROOT/public" \
    "$REPO_ROOT/web.html" "$REPO_ROOT/vite.web.config.ts" "$REPO_ROOT/tsconfig.json" \
    "$REPO_ROOT/package.json" \
    -newer "$REPO_ROOT/dist-web/web.html" -print -quit 2>/dev/null)" ]; then
  console_stale=1
fi

if [ "${FORCE_WEB_BUILD:-0}" = "1" ]; then
  console_stale=1
fi

if [ "$console_stale" = "1" ]; then
  if [ ! -d "$REPO_ROOT/node_modules" ]; then
    command -v npm >/dev/null || die "npm is needed to build the web console"
    say "Installing frontend dependencies"
    (cd "$REPO_ROOT" && npm ci)
  fi
  say "Building the web console"
  (cd "$REPO_ROOT" && npm run build:web)
else
  say "Web console is up to date"
fi

# --- 3. Build the binary ---------------------------------------------------
say "Building $BINARY ($PROFILE)"
if [ "$PROFILE" = "release" ]; then
  cargo build --release -p "$BINARY"
else
  cargo build -p "$BINARY"
fi
BIN="$REPO_ROOT/target/$PROFILE/$BINARY"
[ -f "$BIN" ] || die "no binary at $BIN"

# --- 4. Lay out the bundle -------------------------------------------------
# The identifier matches the Tauri app's on purpose: both shells read and write
# the same ~/Library/Application Support/com.rstorrent.app/settings.json, and a
# user switching between them should not have to reconnect.
say "Assembling $APP_NAME"
rm -rf "$APP"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources"
cp "$BIN" "$CONTENTS/MacOS/$BINARY"

# The daemon and the dylibs it needs, exactly as staged: every library is
# referenced through @executable_path, which resolves to *the daemon's own*
# directory wherever it ends up.
mkdir -p "$CONTENTS/Resources/binaries/rtorrent"
cp -R "$STAGED_RUNTIME/." "$CONTENTS/Resources/binaries/rtorrent/"

ICON="$REPO_ROOT/crates/gpui/assets/icons/icon.icns"
[ -f "$ICON" ] || die "no app icon at $ICON"
cp "$ICON" "$CONTENTS/Resources/icon.icns"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO_ROOT/crates/gpui/Cargo.toml" | head -1)"
[ -n "$VERSION" ] || die "could not read the version from crates/gpui/Cargo.toml"

cat > "$CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key>
	<string>rstorrent</string>
	<key>CFBundleDisplayName</key>
	<string>rstorrent</string>
	<key>CFBundleIdentifier</key>
	<string>com.rstorrent.app</string>
	<key>CFBundleExecutable</key>
	<string>$BINARY</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$VERSION</string>
	<key>CFBundleVersion</key>
	<string>$VERSION</string>
	<key>CFBundleIconFile</key>
	<string>icon.icns</string>
	<key>LSMinimumSystemVersion</key>
	<string>11.0</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.utilities</string>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
PLIST

plutil -lint "$CONTENTS/Info.plist" >/dev/null

# --- 5. Sign it ------------------------------------------------------------
# Ad-hoc, inside out: every nested Mach-O first, then the bundle that seals
# them. `install_name_tool` isn't run here, so the runtime keeps the signatures
# the staging script gave it; re-signing is cheap insurance against a copy that
# dropped extended attributes.
say "Signing (ad-hoc)"
while IFS= read -r file; do
  codesign --force --sign - --timestamp=none "$file" >/dev/null 2>&1
done < <(find "$CONTENTS/Resources/binaries" -type f \( -name '*.dylib' -o -name 'rtorrent' \))
codesign --force --sign - --timestamp=none "$CONTENTS/MacOS/$BINARY"
codesign --force --sign - --timestamp=none "$APP"

# --- 6. Verify -------------------------------------------------------------
say "Verifying the bundle"
codesign --verify --deep --strict "$APP"

BUNDLED="$CONTENTS/Resources/binaries/rtorrent/rtorrent"
# dyld reports a missing library on stderr and the binary exits non-zero, so a
# silent success here proves the runtime still loads its dylibs from the bundle.
smoke="$("$BUNDLED" -h 2>&1)" || {
  echo "$smoke" >&2
  die "the bundled rtorrent does not run from the .app"
}
# `-h` opens with the version, and this build has no `--version`: that line is
# both the only way to ask and the proof of what got bundled.
daemon_version="$(printf '%s\n' "$smoke" | head -1)"

size="$(du -sh "$APP" | cut -f1)"
cat <<DONE

Done.

  Bundle    $APP  ($size)
  Binary    Contents/MacOS/$BINARY
  Daemon    Contents/Resources/binaries/rtorrent/
            $daemon_version

  Run it    open "$APP"
  Zip it    ditto -c -k --sequesterRsrc --keepParent "$APP" "$OUT_DIR/$APP_NAME.zip"

The daemon is preferred over a system install automatically; set
RSTORRENT_RTORRENT_BIN to point the app at something else for a test.

DONE
