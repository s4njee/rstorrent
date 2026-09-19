# rtorrent WSL runtime (Windows build)

This directory holds the Linux rtorrent runtime that the Windows build of
rstorrent ships. Everything here except this README is build output and is
ignored by git.

| File | What it is |
| --- | --- |
| `rstorrent-rootfs.tar.gz` | Root filesystem for a dedicated WSL2 distro named `rstorrent` |
| `SHA256SUMS` | Checksum of the tarball (`sha256sum -c SHA256SUMS`) |
| `.build-stamp` | Records what the tarball was built from, so a re-run can skip the build |

## What is in the rootfs

A stock Alpine Linux userland (the same Alpine release the binary was built on)
plus:

- `/usr/local/bin/rtorrent`: rtorrent and libtorrent at the pinned tag (the
  same tag as `tools/build-rtorrent-macos.sh`), built with XML-RPC through
  rtorrent's vendored tinyxml2 and linked fully statically against musl.
  curl, OpenSSL, zlib and ncurses are all compiled into it.
- `ca-certificates`, for HTTPS trackers and feeds. The static OpenSSL and curl
  read Alpine's standard paths: `/etc/ssl/certs/ca-certificates.crt` and
  `/etc/ssl/certs/`.
- `ncurses-terminfo-base`, so the curses UI works if you start rtorrent by
  hand inside the distro to debug it.
- `/etc/wsl.conf`: systemd off, Windows drives automounted, Windows `PATH` not
  appended.
- `/etc/rstorrent-runtime`: the rtorrent version, Alpine release and build
  date, so the app can tell whether an imported distro is stale.
- `/root/.rtorrent/session/`: the session directory the app's starter config
  points at. rtorrent will not start if it is missing, and it does not create
  it.

The tarball is packed with numeric owners (everything is uid/gid 0 or an
Alpine system id) and sorted entries. It has no `/etc/resolv.conf`, because WSL
generates one when the distro boots.

## How the app uses it

On first run the Windows app imports the distro and then starts the daemon
directly, without a login shell:

```
wsl --import rstorrent <install dir> rstorrent-rootfs.tar.gz --version 2
wsl.exe -d rstorrent -e /usr/local/bin/rtorrent
```

rtorrent listens for SCGI on TCP port 5000 inside the distro, and WSL's
localhost forwarding makes it reachable from Windows at `127.0.0.1:5000`.

## Rebuilding

The build needs a Linux x86-64 host, used as root. It runs inside an Alpine
minirootfs chroot on that host, so nothing gets installed on the host itself.
A WSL2 distro works as a build host. You can drive the build over SSH from a
Mac, or run it directly on the Linux machine:

```
BUILD_HOST=root@buildbox tools/build-rtorrent-wsl.sh     # over SSH
sudo tools/build-rtorrent-wsl.sh                         # on Linux x86-64
RTORRENT_VERSION=v0.16.18 BUILD_HOST=... tools/build-rtorrent-wsl.sh
ALPINE_VERSION=3.22.6 BUILD_HOST=... tools/build-rtorrent-wsl.sh
FORCE=1 BUILD_HOST=... tools/build-rtorrent-wsl.sh       # ignore all caches
```

Everything on the build host lives under `/var/tmp/rstorrent-wsl-build`
(override it with `BUILD_DIR`):

- the minirootfs, checked against Alpine's published sha256
- the build chroot and its compiled prefix, with a stamp for each step
- the output

A re-run only re-assembles and re-tests the rootfs, which takes a few seconds.
A cold build takes about a minute on a 24-core machine. Each run finishes by
unpacking the tarball, starting rtorrent in daemon mode inside it and calling
`system.client_version` over SCGI.
