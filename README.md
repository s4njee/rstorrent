# rstorrent

[![CI](https://github.com/s4njee/rstorrent/actions/workflows/ci.yml/badge.svg)](https://github.com/s4njee/rstorrent/actions/workflows/ci.yml)

A native desktop client for the [`rtorrent`](https://github.com/rakshasa/rtorrent)
daemon on **macOS and Windows**, built with **Rust** and a native **GPUI**
desktop shell and a **React/TypeScript** web console. It implements the "Dark
Ops" design in [`design/`](design/): a compact, monospace, power-user torrent
client in the mold of qBittorrent, front-ending rtorrent over its XML-RPC
interface.

rtorrent has no Windows build, so the Windows app drives a daemon running in
WSL2 and translates paths across the boundary — see
[docs/wsl-setup.md](docs/wsl-setup.md). The macOS app **ships its own rtorrent**
as a separate executable, so there is nothing to install before using it.

rstorrent is a *client* — it does not embed a BitTorrent engine in-process; it
speaks to rtorrent (bundled on macOS, system, or remote) over XML-RPC.

The desktop shell is **GPUI** (the Tauri + React shell is being retired — see
[GPUI.md](GPUI.md); the Tauri shell stays frozen as the behavioural reference
until the GPUI parity epics G6–G8 land). The web console keeps the React UI.

![The rstorrent main window](docs/images/main-window.png)

## Build the app

One command builds the app and everything inside it — the bundled rtorrent
(built from source on the first run, reused after), the web console the binary
embeds, and the macOS `.app` with the runtime in it:

```sh
tools/bundle-gpui-macos.sh
```

That leaves `dist-gpui/rstorrent-gpui.app`, ready to run or to hand to someone:

```sh
open dist-gpui/rstorrent-gpui.app

ditto -c -k --sequesterRsrc --keepParent \
  dist-gpui/rstorrent-gpui.app dist-gpui/rstorrent-gpui.app.zip
```

The first run is the slow one: it builds libtorrent and rtorrent 0.15.7 from
source into `~/.cache/rstorrent-build` and reuses them from then on. It needs
the Rust toolchain, Node, and Homebrew with `curl openssl@3 ncurses tinyxml2`.
The result is ad-hoc signed rather than notarized, so Gatekeeper warns on first
launch on another machine — see [docs/release.md](docs/release.md).

For the Tauri shell instead, it is the same two steps that script performs:
[`tools/build-rtorrent-macos.sh`](tools/build-rtorrent-macos.sh) to stage the
runtime, then `npm run tauri build`.

## Status

The core client and several feature slices have shipped: a live main window
(toolbar, filter sidebar, sortable torrent table, detail tabs, status bar)
driven by a background poller, plus a network-preferences pane, per-torrent and
automation controls, connection profiles, native daemon views, and RSS
auto-add. Verified against Homebrew's **rtorrent 0.16.17** on macOS, and
**rtorrent 0.16.18** built in WSL on Windows. The macOS `.app` bundles its own
**rtorrent 0.15.7** — built from source by
[`tools/build-rtorrent-macos.sh`](tools/build-rtorrent-macos.sh) and staged in
`src-tauri/binaries/rtorrent/` — and prefers it over any system install. See
[backlog.md](backlog.md) for the shipped-so-far list and what's next.

See [plan.md](plan.md) for the architecture, [tasks.md](tasks.md) for the
execution tracker, and [backlog.md](backlog.md) for what's being considered next.

## Features

**Transports** — a local unix socket (the macOS default; SCGI is
unauthenticated, so keeping it off the network is the safe posture), a TCP port
(the Windows default, bridged into WSL over loopback), or XML-RPC over HTTP(S)
with basic auth for a remote seedbox. Remote passwords live in the macOS
Keychain or Windows Credential Manager, never in `settings.json`. Actions that
only make sense for local files — delete-data, reveal-in-file-manager,
free-space — are disabled for a remote daemon.

**Adding torrents** — `.torrent` file association and the `magnet:` URL scheme,
drag & drop onto the window, ⌘V to add from the clipboard, and a watch folder.

**The table** — sortable, resizable, customizable columns; multi-select with a
summary bar for bulk resume/pause/remove; a filter sidebar with status, label and
tracker groups, plus saved smart filters that AND several dimensions together.

![Smart filters in the sidebar and the selection summary bar](docs/images/smart-filters.png)

**Detail tabs** — General (with a pieces bar showing which chunks have landed),
Trackers (type / announce timing / add / remove / enable / reannounce), Peers
(with per-peer ban / snub / disconnect), Content (per-file priorities), Speed,
and Log.

![The pieces bar on the General tab](docs/images/pieces-bar.png)

**Network** — a Preferences pane for protocol encryption/PEX, an HTTP tracker
proxy, and bind/listen addresses (bind to a VPN interface so traffic dies with
the tunnel), plus global peer and connection-slot caps. A one-click **Tune for
1 Gbps** menu action writes a managed block to `.rtorrent.rc` and applies it
live.

**Automation** — per-torrent speed limits via named throttle pools; ratio
groups / seed goals (stop, or auto-remove) set globally or per label; a
max-active-downloads queue; multiple watch folders and per-label default save
paths; a run-on-complete command hook; and turtle mode (alternative limits on a
manual toggle or a daily schedule).

**Seedbox** — saved connection profiles to switch daemons, the daemon's own
native views surfaced in the sidebar, a Daemon menu (save session / shut down),
a daemon-health tab in Statistics, and RSS feeds with auto-download rules.

## Quick start

```sh
npm install

# Run against the ten built-in fixture torrents — no daemon needed:
RSTORRENT_MOCK=1 npm run tauri dev      # PowerShell: $env:RSTORRENT_MOCK=1

# Run against a real daemon:
#   macOS   — build the bundled one with tools/build-rtorrent-macos.sh,
#             or use a system rtorrent; see docs/rtorrent-setup.md
#   Windows — see docs/wsl-setup.md
npm run tauri dev
```

Mock mode is the fastest way to see the UI: it serves ten fixture torrents in
assorted states, with no rtorrent and no network.

## Connecting

On macOS the app ships its own rtorrent: hit **Start rtorrent** on the
disconnected card (or **Daemon → Start Daemon**) and it launches the bundled
0.15.7 daemon, writing a starter `~/.rtorrent.rc` if you have none. To use a
system or remote daemon instead, follow
[docs/rtorrent-setup.md](docs/rtorrent-setup.md), then open
**Preferences → Connection**, match the transport to your `.rtorrent.rc`, and
hit **Test connection** — it reports the rtorrent version.

![Preferences → Connection](docs/images/preferences-connection.png)

## Web UI

The same UI runs in a browser, served by a small self-hosted server
(`rstorrent-web`) that sits next to the daemon and proxies its XML-RPC/SCGI
interface as JSON. The React frontend is shared with the desktop app — the only
difference is the host backend (HTTP polling instead of Tauri IPC) and the shell
chrome (an app bar instead of the native title bar). Single-password login,
session cookie, delete-with-data gated to co-located daemons.

```sh
# Develop against the fixtures with no daemon:
RSTORRENT_MOCK=1 cargo run -p rstorrent-web     # server + embedded UI at :9080
npm run dev:web                                  # or Vite on :1421, proxying /api → :9080

# Ship it:
npm run build:web && cargo build --release -p rstorrent-web
```

See [docs/web-setup.md](docs/web-setup.md) for configuration, Docker, systemd,
and reverse-proxy (TLS) setup.

## Development

| Command | What it does |
|---|---|
| `npm run tauri dev` | Run the desktop app (add `RSTORRENT_MOCK=1` for mock mode) |
| `RSTORRENT_MOCK=1 cargo run -p rstorrent-web` | Run the web server against fixtures |
| `npm run build:web` | Build the browser SPA the server embeds |
| `npm test` | Frontend unit tests (Vitest) |
| `npm run check:contrast` | Palette contrast floors + colour-literal guard (see `docs/web-console-plan.md`) |
| `npm run typecheck` | `tsc --noEmit` |
| `npm run lint` | ESLint + Prettier check |
| `cargo test` (in `src-tauri/`) | Rust unit tests |
| `cargo clippy --all-targets -- -D warnings` | Rust lints |
| `cargo fmt --check` (in `src-tauri/`) | Rust formatting |
| `npm run tauri build` | Package `.app`/`.dmg` (macOS) or `.msi`/NSIS `.exe` (Windows) |

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs all of the
above on push and PR — the frontend checks on Linux, the Rust ones on macOS.

Tests that touch a live daemon or the Keychain are marked `#[ignore]` and are run
deliberately — see [docs/rtorrent-setup.md](docs/rtorrent-setup.md).

## Layout

This is a Cargo workspace: the rtorrent client layer is a Tauri-free crate that
both the desktop app and the web server share.

```
plan.md · tasks.md · backlog.md   # architecture, tracker, ideas
Cargo.toml                        # workspace: crates/rtorrent · server · src-tauri
design/                           # the "Dark Ops" design reference (authoritative)
docs/rtorrent-setup.md            # connecting to a live rtorrent (macOS)
docs/wsl-setup.md                 # connecting to rtorrent in WSL (Windows)
docs/web-setup.md                 # self-hosting the web UI (server + reverse proxy)
docs/images/                      # README screenshots (regenerated from the demo)
demo.html · src/demo/             # browser demo: real UI over mocked IPC + fixtures
crates/rtorrent/                  # rtorrent-core: SCGI/HTTP transports, XML-RPC, DTOs, mock
src/                              # React frontend (ipc backends, store, components, theme)
src/web/ · web.html               # the browser web UI shell (WE) over an HTTP backend
server/src/                       # rstorrent-web: axum server proxying the daemon as JSON
src-tauri/src/                    # Tauri desktop shell over the shared crate (poller, commands)
src-tauri/src/wsl.rs              # Windows-only: path translation across the WSL boundary
src-tauri/binaries/rtorrent/      # the bundled macOS runtime (built, gitignored)
tools/scgi-http-bridge.py         # dev-only HTTP→SCGI bridge, stands in for nginx
tools/build-rtorrent-macos.sh     # builds the bundled rtorrent + libtorrent for the .app
tools/wsl-setup-rtorrent.sh       # builds rtorrent inside WSL and starts it under systemd
```

## License

TBD.
