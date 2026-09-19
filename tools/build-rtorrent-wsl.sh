#!/usr/bin/env bash
#
# Build the Linux rtorrent runtime the Windows app ships, as a WSL distro.
#
# On Windows the app does not assume the user has a Linux rtorrent anywhere.
# On first run it imports a dedicated WSL2 distro from the root filesystem this
# script produces:
#
#   wsl --import rstorrent <dir> rstorrent-rootfs.tar.gz --version 2
#   wsl.exe -d rstorrent -e /usr/local/bin/rtorrent
#
# and reaches the daemon over SCGI on TCP 127.0.0.1:5000 through WSL's
# localhost forwarding. The rootfs is a stock Alpine userland plus one fully
# static rtorrent, so nothing inside the distro needs a package manager, a
# network install or a compiler at run time.
#
# Version note: the pinned tag MUST match tools/build-rtorrent-macos.sh. Both
# platforms talk to rtorrent through the same XML-RPC command set, and command
# names move between releases (0.16.20 renamed network.port_range.set, among
# others), so one app version has to talk to one rtorrent version.
#
# Why static and why musl: a static binary has no loader or shared-library
# story to get wrong inside a distro we do not otherwise maintain, and Alpine
# ships `-static` archives for almost every dependency (curl and its TLS/HTTP
# stack, OpenSSL, zlib, ncurses). The one missing piece, tinyxml2, is vendored
# in rtorrent's own tree (src/rpc/tinyxml2), so it needs no package at all.
#
# Where it builds: on a Linux x86-64 host, as root, inside an Alpine
# minirootfs chroot — so the host needs nothing but bash, curl, tar, unshare,
# chroot and python3 (for the smoke test), and gets no packages installed. From
# a Mac (or any machine) point BUILD_HOST at such a host over SSH; the script
# copies itself there, runs the build, and fetches the tarball back. On a Linux
# x86-64 machine as root it can build locally instead. A WSL2 distro is a
# perfectly good build host.
#
# Usage:
#   BUILD_HOST=root@buildbox tools/build-rtorrent-wsl.sh     # build over SSH
#   sudo tools/build-rtorrent-wsl.sh                         # on Linux x86-64
#   RTORRENT_VERSION=v0.16.18 BUILD_HOST=... tools/build-rtorrent-wsl.sh
#   FORCE=1 BUILD_HOST=... tools/build-rtorrent-wsl.sh       # ignore all caches
#
# Everything on the build host lives under /var/tmp/rstorrent-wsl-build
# (override with BUILD_DIR): the downloaded minirootfs, the build chroot with
# its compiled prefix (stamped per step, so a re-run only re-assembles), and
# the assembled output. `rm -rf` it to reclaim the space.
#
# Output (binaries/rtorrent-wsl/):
#   rstorrent-rootfs.tar.gz   the WSL root filesystem
#   SHA256SUMS                its checksum, for the app to verify before import
#   README.md                 what this is and how to rebuild it (committed)

set -euo pipefail

VERSION="${RTORRENT_VERSION:-v0.15.7}"
# A full point release, because the minirootfs tarball is per point release and
# its checksum is what makes the download trustworthy. The build chroot and the
# shipped rootfs come from the same one, so the CA bundle and terminfo paths
# compiled into the static curl/OpenSSL/ncurses match what is on disk.
ALPINE_VERSION="${ALPINE_VERSION:-3.22.6}"
FORCE="${FORCE:-0}"
BUILD_HOST="${BUILD_HOST:-}"
WORK="${BUILD_DIR:-/var/tmp/rstorrent-wsl-build}"

say() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# `shasum` ships with macOS, `sha256sum` with Linux; SHA256SUMS uses the same
# two-space format either way, so `sha256sum -c` works on it on any host.
sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$@"; else shasum -a 256 "$@"; fi
}

# ===========================================================================
# Build-host half: everything from here to the end of `on_build_host` runs on
# the Linux x86-64 machine, as root.
# ===========================================================================

# The packages the build chroot needs. The `-static` ones are the .a archives;
# the `-dev` ones carry the headers and the pkg-config files that
# `pkg-config --static` walks for the full, transitive link line. libcurl's is
# the long one: Alpine builds it against nghttp2/nghttp3/ngtcp2 (HTTP/2 and
# HTTP/3), brotli, zstd, libidn2 and libpsl, and a static link needs every one
# of those archives present.
BUILD_PACKAGES="build-base autoconf automake libtool pkgconf git linux-headers
  curl-dev curl-static openssl-dev openssl-libs-static zlib-dev zlib-static
  ncurses-dev ncurses-static nghttp2-dev nghttp2-static nghttp3-dev
  nghttp3-static ngtcp2-dev ngtcp2-static brotli-dev brotli-static zstd-dev
  zstd-static libidn2-dev libidn2-static libunistring-dev libunistring-static
  libpsl-dev libpsl-static"

# What runs inside the build chroot, one step per invocation. Kept as a script
# of its own (written into the chroot on every run) so the compile happens with
# Alpine's shell, toolchain and pkg-config, never the host's.
write_chroot_steps() {
  cat > "$1" <<'STEPS'
#!/bin/sh
# Written by tools/build-rtorrent-wsl.sh; runs inside the Alpine build chroot.
# Usage: steps.sh <deps|fetch|libtorrent|rtorrent> <version> <jobs> [packages]
set -eu
step="$1" version="$2" jobs="$3"
prefix="/opt/rtorrent-$version"

# `--static` makes pkg-config emit Libs.private too, which is what turns
# `-lcurl` into `-lcurl -lnghttp2 -lssl -lcrypto -lz ...`. `-static` on the
# compiler driver keeps configure's own link tests honest: a check that would
# only pass against a .so must fail here, not at the final link.
export PKG_CONFIG="pkg-config --static"
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig"
export CFLAGS="-O2 -pipe" CXXFLAGS="-O2 -pipe" LDFLAGS="-static"

case "$step" in
  deps)
    # The package list is meant to word-split.
    apk add --no-cache --quiet $4
    ;;
  fetch)
    mkdir -p /src
    for repo in libtorrent rtorrent; do
      rm -rf "/src/$repo-$version"
      git clone --quiet --depth 1 --branch "$version" \
        "https://github.com/rakshasa/$repo.git" "/src/$repo-$version"
    done
    ;;
  libtorrent)
    # A plain static library. musl has no <execinfo.h>, and the probe for it
    # is better switched off than left to guess.
    cd "/src/libtorrent-$version"
    git clean -xfdq
    autoreconf -ivf >/dev/null 2>&1
    ./configure --prefix="$prefix" \
      --disable-shared --enable-static \
      --disable-debug --disable-execinfo
    make -j"$jobs"
    make install
    ;;
  rtorrent)
    # Without an XML-RPC backend rtorrent opens the SCGI port but answers
    # every call with "XML-RPC not supported" — the entire interface the app
    # speaks. tinyxml2 is compiled from rtorrent's vendored copy, as on macOS.
    #
    # `-all-static` is libtool's spelling of "link the executable fully
    # static"; plain `-static` only means "prefer libtool's own static
    # archives" there, and would still leave a dynamically linked binary. It
    # goes to make, not configure, because configure hands LDFLAGS straight to
    # gcc, which rejects it.
    cd "/src/rtorrent-$version"
    git clean -xfdq
    autoreconf -ivf >/dev/null 2>&1
    ./configure --prefix="$prefix" \
      --disable-shared --enable-static \
      --disable-debug --disable-execinfo \
      --with-xmlrpc-tinyxml2
    make -j"$jobs" LDFLAGS="-all-static"
    make install
    strip "$prefix/bin/rtorrent"
    ;;
  *)
    echo "unknown step: $step" >&2
    exit 2
    ;;
esac
STEPS
}

# Refuse to `rm -rf` a tree with anything mounted inside it. Every mount this
# script makes lives in a private namespace and cannot outlive it, so this is a
# guard against someone else's bind mount, not our own.
assert_unmounted() {
  if awk -v root="$1/" 'index($2, root) == 1 { found = 1 } END { exit !found }' /proc/mounts; then
    die "$1 has something mounted inside it; refusing to delete it"
  fi
}

# Run a command inside a chroot with /proc, /dev and /sys available and the
# host's DNS. The mounts are made in a private mount namespace (`unshare -m`),
# so they are invisible to the rest of the host and vanish with the command
# even if it is killed; the trap is belt and braces for the normal path.
in_chroot() {
  local root="$1"; shift
  # WSL (and systemd-resolved) make /etc/resolv.conf a symlink; copy its target.
  rm -f "$root/etc/resolv.conf"
  cp -L /etc/resolv.conf "$root/etc/resolv.conf"
  unshare --mount --propagation private -- /bin/bash -euc '
    root="$1"; shift
    trap "umount -l \"\$root/sys\" \"\$root/dev\" \"\$root/proc\" 2>/dev/null || true" EXIT
    mount -t proc proc "$root/proc"
    mount --bind /dev "$root/dev"
    mount -t sysfs -o ro sysfs "$root/sys"
    chroot "$root" /usr/bin/env -i \
      PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
      HOME=/root LANG=C.UTF-8 TERM=dumb "$@"
  ' in_chroot "$root" "$@"
}

# Download the minirootfs once and check it against the published checksum on
# every run; a partial or tampered file is deleted rather than trusted.
fetch_minirootfs() {
  local branch="v${ALPINE_VERSION%.*}"
  local base="https://dl-cdn.alpinelinux.org/alpine/$branch/releases/x86_64"
  local name="alpine-minirootfs-$ALPINE_VERSION-x86_64.tar.gz"
  MINIROOTFS="$WORK/dl/$name"
  mkdir -p "$WORK/dl"
  if [ "$FORCE" = "1" ] || [ ! -f "$MINIROOTFS" ]; then
    say "Downloading Alpine $ALPINE_VERSION minirootfs"
    curl -fsSL -o "$MINIROOTFS.tmp" "$base/$name"
    mv "$MINIROOTFS.tmp" "$MINIROOTFS"
  fi
  curl -fsSL -o "$MINIROOTFS.sha256" "$base/$name.sha256"
  if ! (cd "$WORK/dl" && sha256sum -c --quiet "$name.sha256"); then
    rm -f "$MINIROOTFS"
    die "the minirootfs does not match its published sha256 (deleted; re-run to retry)"
  fi
  echo "  $name: sha256 OK"
}

# Unpack the minirootfs into a fresh directory, keeping its numeric owners.
unpack_minirootfs() {
  local dest="$1"
  assert_unmounted "$dest"
  rm -rf "$dest"
  mkdir -p "$dest"
  tar -xzpf "$MINIROOTFS" --numeric-owner -C "$dest"
}

# Run one build step unless its stamp says it is done. A stamp holds the hash
# of the step script and the package list, so editing either re-runs the step;
# running a step clears every stamp after it, since those depend on it.
STEP_ORDER="deps fetch libtorrent rtorrent"
run_step() {
  local step="$1" stamp="$BUILD_ROOT/.stamp-$1-$VERSION" later=0 s
  if [ -f "$stamp" ] && [ "$(cat "$stamp")" = "$STEPS_HASH" ]; then
    echo "  $step: cached"
    return 0
  fi
  for s in $STEP_ORDER; do
    [ "$later" = 1 ] && rm -f "$BUILD_ROOT/.stamp-$s-$VERSION"
    [ "$s" = "$step" ] && later=1
  done
  say "Build step: $step ($VERSION)"
  in_chroot "$BUILD_ROOT" /bin/sh /build/steps.sh "$step" "$VERSION" "$JOBS" "$BUILD_PACKAGES"
  printf '%s\n' "$STEPS_HASH" > "$stamp"
}

# Smoke-test the tarball itself, not the directory it was made from: unpack it,
# start rtorrent in daemon mode inside it and call it over SCGI from the host.
# A chroot shares the host's network namespace, so 127.0.0.1 is the same
# loopback on both sides.
smoke_test() {
  local root="$WORK/smoke" port
  say "Smoke test: daemon mode + XML-RPC over SCGI"
  assert_unmounted "$root"
  rm -rf "$root"
  mkdir -p "$root"
  tar -xzpf "$OUT_TARBALL" --numeric-owner -C "$root"
  file "$root/usr/local/bin/rtorrent"

  # 5000 is the port the app uses; if something on the build host already
  # holds it (quite possibly an rtorrent), pick a free one rather than talk to
  # the wrong daemon.
  port=5000
  if [ -n "$(ss -ltnH "sport = :$port" 2>/dev/null)" ]; then
    port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
    echo "  port 5000 is in use on the build host; testing on $port instead"
  fi
  mkdir -p "$root/tmp/s" "$root/tmp/d"
  cat > "$root/tmp/smoke.rc" <<RC
system.daemon.set = true
network.scgi.open_port = 127.0.0.1:$port
session.path.set = /tmp/s
directory.default.set = /tmp/d
RC

  # A minimal SCGI client: netstring-framed CGI headers, then the XML body.
  cat > "$WORK/scgi-call.py" <<'PY'
import socket, sys, time
port, method, expect = int(sys.argv[1]), sys.argv[2], sys.argv[3]
body = ("<?xml version='1.0'?><methodCall><methodName>%s</methodName>"
        "<params></params></methodCall>" % method).encode()
headers = b"CONTENT_LENGTH\0" + str(len(body)).encode() + b"\0SCGI\x001\0"
frame = str(len(headers)).encode() + b":" + headers + b"," + body
deadline = time.time() + 30
while True:
    try:
        s = socket.create_connection(("127.0.0.1", port), timeout=5)
        break
    except OSError:
        if time.time() > deadline:
            sys.exit("rtorrent never opened 127.0.0.1:%d" % port)
        time.sleep(0.5)
s.sendall(frame)
reply = b""
while chunk := s.recv(65536):
    reply += chunk
xml = reply.decode(errors="replace").split("\r\n\r\n", 1)[-1]
print("  %s -> %s" % (method, xml))
sys.exit(0 if "<string>%s</string>" % expect in xml else "unexpected reply")
PY

  unshare --mount --propagation private -- /bin/bash -euc '
    root="$1" port="$2" expect="$3" work="$4"
    mount -t proc proc "$root/proc"
    mount --bind /dev "$root/dev"
    chroot "$root" /usr/bin/env -i HOME=/root PATH=/usr/local/bin:/usr/bin:/bin TERM=dumb \
      /usr/local/bin/rtorrent -n -o import=/tmp/smoke.rc > "$root/tmp/rtorrent.log" 2>&1 &
    pid=$!
    # Each part tolerates failure: under -e a failing `wait` (rtorrent dies of
    # the SIGTERM, status 143) would otherwise end the trap before the umount.
    trap "kill \$pid 2>/dev/null || true; wait \$pid 2>/dev/null || true; umount -l \"\$root/dev\" \"\$root/proc\" 2>/dev/null || true" EXIT
    if ! python3 "$work/scgi-call.py" "$port" system.client_version "$expect"; then
      echo "  rtorrent output:"; cat "$root/tmp/rtorrent.log"
      exit 1
    fi
    kill -0 "$pid" || { echo "  rtorrent exited after answering"; exit 1; }
  ' smoke "$root" "$port" "${VERSION#v}" "$WORK" || die "smoke test failed"
  echo "  daemon answered on 127.0.0.1:$port and was stopped"
  assert_unmounted "$root"
  rm -rf "$root"
}

on_build_host() {
  local started=$SECONDS
  [ "$(uname -s)" = "Linux" ] && [ "$(uname -m)" = "x86_64" ] \
    || die "the build host must be Linux x86-64 (this is $(uname -s) $(uname -m))"
  [ "$(id -u)" -eq 0 ] || die "building needs root on the build host (chroot and mounts)"
  local tool
  for tool in curl tar gzip unshare chroot sha256sum python3 file ss; do
    command -v "$tool" >/dev/null 2>&1 || die "the build host is missing '$tool'"
  done

  JOBS="$(nproc)"
  BUILD_ROOT="$WORK/build-root-$ALPINE_VERSION"
  OUT_TARBALL="$WORK/out/rstorrent-rootfs.tar.gz"
  mkdir -p "$WORK/out"

  # --- 1. The Alpine minirootfs ---------------------------------------------
  fetch_minirootfs

  # --- 2. The build chroot ----------------------------------------------------
  # Created once per Alpine release and reused, compiled prefix and all.
  if [ "$FORCE" = "1" ] || [ ! -x "$BUILD_ROOT/bin/busybox" ]; then
    say "Creating the build chroot ($BUILD_ROOT)"
    unpack_minirootfs "$BUILD_ROOT"
  fi
  mkdir -p "$BUILD_ROOT/build"
  write_chroot_steps "$BUILD_ROOT/build/steps.sh"
  STEPS_HASH="$( (cat "$BUILD_ROOT/build/steps.sh"; echo "$BUILD_PACKAGES") | sha256sum | cut -d' ' -f1)"

  # --- 3. Static libtorrent + rtorrent ---------------------------------------
  say "Building static rtorrent $VERSION on Alpine $ALPINE_VERSION ($JOBS jobs)"
  local step
  for step in $STEP_ORDER; do
    run_step "$step"
  done
  local binary="$BUILD_ROOT/opt/rtorrent-$VERSION/bin/rtorrent"
  # Fail here, not on the user's first run, if anything slipped through as a
  # shared dependency.
  file "$binary" | grep -q 'statically linked' || die "rtorrent is not statically linked"

  # --- 4. The shipped rootfs, from a fresh minirootfs -------------------------
  # Never the build chroot: that one carries a compiler and every -dev package.
  say "Assembling the WSL root filesystem"
  local rootfs="$WORK/rootfs"
  unpack_minirootfs "$rootfs"
  # ca-certificates: tracker announces and RSS/HTTP fetches go through the
  # static curl + OpenSSL, which read Alpine's standard locations: the bundle
  # /etc/ssl/certs/ca-certificates.crt (also /etc/ssl/cert.pem) and the hashed
  # directory /etc/ssl/certs.
  # ncurses-terminfo-base (~100 KB): the daemon never needs it, but it makes
  # the curses UI work when someone starts rtorrent by hand inside the distro
  # to debug it, which is otherwise the first thing to break.
  in_chroot "$rootfs" /sbin/apk add --no-cache --quiet ca-certificates ncurses-terminfo-base
  # WSL generates resolv.conf at boot; ours was only there for apk.
  rm -f "$rootfs/etc/resolv.conf"
  rm -rf "$rootfs/var/cache/apk/"*

  install -D -m 0755 "$binary" "$rootfs/usr/local/bin/rtorrent"

  # systemd=false: nothing here needs an init system; the app runs rtorrent
  #   directly with `wsl.exe -e`, and WSL's own init stays PID 1.
  # automount: the user's download folders live on Windows drives (/mnt/c/...).
  # appendWindowsPath=false: keeps Windows' PATH (with its spaces and
  #   parentheses) out of the environment rtorrent's execute.* commands inherit.
  cat > "$rootfs/etc/wsl.conf" <<'CONF'
# Written by tools/build-rtorrent-wsl.sh for the rstorrent runtime distro.
[boot]
systemd=false

[automount]
enabled=true

[interop]
appendWindowsPath=false
CONF
  printf 'rtorrent=%s\nalpine=%s\nbuilt=%s\n' \
    "$VERSION" "$(cat "$rootfs/etc/alpine-release")" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    > "$rootfs/etc/rstorrent-runtime"
  # The app's starter config (crates/gpui/src/daemon_wsl.rs) puts the session
  # in /root/.rtorrent/session. rtorrent refuses to start ("Could not lock
  # session directory") rather than create it, so the directory ships here.
  mkdir -p "$rootfs/root/.rtorrent/session"

  # Numeric owners, so the tarball does not depend on the build host's
  # /etc/passwd (everything in a minirootfs is uid/gid 0 or an Alpine system
  # id, which is what it means inside the distro), and sorted entries, so it
  # does not depend on directory order either.
  say "Packing $OUT_TARBALL"
  tar --numeric-owner --sort=name -C "$rootfs" -cpf - . | gzip -n -9 > "$OUT_TARBALL.tmp"
  mv "$OUT_TARBALL.tmp" "$OUT_TARBALL"
  (cd "$WORK/out" && sha256sum "$(basename "$OUT_TARBALL")" > SHA256SUMS)
  assert_unmounted "$rootfs"
  rm -rf "$rootfs"

  # --- 5. Verify the tarball ---------------------------------------------------
  smoke_test

  echo "  $(cat "$WORK/out/SHA256SUMS")"
  echo "  size $(du -h "$OUT_TARBALL" | cut -f1); $((SECONDS - started))s on the build host"
}

# ===========================================================================
# Driver half: decides where to build, and brings the result home.
# ===========================================================================

if [ "${1:-}" = "--on-build-host" ]; then
  on_build_host
  exit 0
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="$REPO_ROOT/binaries/rtorrent-wsl"
TARBALL="$OUT_DIR/rstorrent-rootfs.tar.gz"
STAMP="$OUT_DIR/.build-stamp"
started=$SECONDS

if [ -n "$BUILD_HOST" ]; then
  mode="remote"
elif [ "$(uname -s)" = "Linux" ] && [ "$(uname -m)" = "x86_64" ] && [ "$(id -u)" -eq 0 ]; then
  mode="local"
else
  die "set BUILD_HOST to a Linux x86-64 machine reachable over SSH as root, e.g.
  BUILD_HOST=root@buildbox $0
(or run this script as root on Linux x86-64 to build locally)"
fi

# Anything that changes what ends up in the tarball belongs in the stamp. The
# script's own hash is included so an edit to the build steps re-runs them.
SCRIPT_HASH="$(sha256 "$0" | awk '{print $1}')"
STAMP_CONTENT="rtorrent=$VERSION alpine=$ALPINE_VERSION script=$SCRIPT_HASH"

if [ "$FORCE" != "1" ] && [ -f "$TARBALL" ] && [ -f "$STAMP" ] \
    && [ "$(cat "$STAMP")" = "$STAMP_CONTENT" ]; then
  say "Using cached rootfs ($VERSION, Alpine $ALPINE_VERSION); FORCE=1 rebuilds"
  echo "  $TARBALL"
  exit 0
fi

mkdir -p "$OUT_DIR"
if [ "$mode" = "remote" ]; then
  ssh_opts=(-o BatchMode=yes)
  say "Building on $BUILD_HOST (work dir $WORK)"
  ssh "${ssh_opts[@]}" "$BUILD_HOST" "mkdir -p $(printf '%q' "$WORK")" </dev/null
  scp -q "${ssh_opts[@]}" "$0" "$BUILD_HOST:$WORK/build-rtorrent-wsl.sh"
  # The settings travel as environment on the remote command line; `%q` keeps
  # each one a single shell word whatever it contains.
  ssh "${ssh_opts[@]}" "$BUILD_HOST" \
    "RTORRENT_VERSION=$(printf '%q' "$VERSION")" \
    "ALPINE_VERSION=$(printf '%q' "$ALPINE_VERSION")" \
    "FORCE=$(printf '%q' "$FORCE")" \
    "BUILD_DIR=$(printf '%q' "$WORK")" \
    "bash $(printf '%q' "$WORK/build-rtorrent-wsl.sh") --on-build-host" </dev/null
  say "Fetching the rootfs from $BUILD_HOST"
  scp -q "${ssh_opts[@]}" "$BUILD_HOST:$WORK/out/rstorrent-rootfs.tar.gz" "$TARBALL.tmp"
  scp -q "${ssh_opts[@]}" "$BUILD_HOST:$WORK/out/SHA256SUMS" "$OUT_DIR/SHA256SUMS"
else
  bash "$0" --on-build-host
  cp "$WORK/out/rstorrent-rootfs.tar.gz" "$TARBALL.tmp"
  cp "$WORK/out/SHA256SUMS" "$OUT_DIR/SHA256SUMS"
fi
mv "$TARBALL.tmp" "$TARBALL"

# The checksum was computed on the build host; checking it here proves the copy.
(cd "$OUT_DIR" && sha256 -c SHA256SUMS >/dev/null) \
  || die "the fetched rootfs does not match the build host's SHA256SUMS"
printf '%s\n' "$STAMP_CONTENT" > "$STAMP"

cat <<DONE

Done in $((SECONDS - started))s.

  Rootfs    $TARBALL ($(du -h "$TARBALL" | cut -f1))
  Checksum  $OUT_DIR/SHA256SUMS
  Version   rtorrent $VERSION on Alpine $ALPINE_VERSION, static x86-64 musl
  Import    wsl --import rstorrent <dir> rstorrent-rootfs.tar.gz --version 2

DONE
