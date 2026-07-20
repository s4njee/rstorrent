# rstorrent

[![CI](https://github.com/s4njee/rstorrent/actions/workflows/ci.yml/badge.svg)](https://github.com/s4njee/rstorrent/actions/workflows/ci.yml)

A native macOS desktop client for the [`rtorrent`](https://github.com/rakshasa/rtorrent)
daemon, built with **Rust + Tauri 2** and a **React/TypeScript** frontend. It
implements the "Dark Ops" design in [`design/`](design/): a compact, monospace,
power-user torrent client in the mold of qBittorrent, front-ending rtorrent over
its XML-RPC interface.

rstorrent is a *client* — it does not embed a BitTorrent engine.

<!-- TODO capture (see docs/images/README.md):
![The rstorrent main window](docs/images/main-window.png)
-->

## Status

Milestones M0–M1 are complete: a live main window (toolbar, filter sidebar,
sortable torrent table, detail tabs, status bar) driven by a background poller,
running against either a live daemon or the built-in mock. Verified against
Homebrew's **rtorrent 0.16.17**.

See [plan.md](plan.md) for the architecture, [tasks.md](tasks.md) for the
execution tracker, and [backlog.md](backlog.md) for what's being considered next.

## Features

**Transports** — a local unix socket (the default; SCGI is unauthenticated, so
keeping it off the network is the safe posture), a TCP port, or XML-RPC over
HTTP(S) with basic auth for a remote seedbox. Remote passwords live in the macOS
Keychain, never in `settings.json`. Actions that only make sense for local files
— delete-data, reveal-in-Finder, free-space — are disabled for a remote daemon.

**Adding torrents** — `.torrent` file association and the `magnet:` URL scheme,
drag & drop onto the window, ⌘V to add from the clipboard, and a watch folder.

**The table** — sortable, resizable, customizable columns; multi-select with a
summary bar for bulk resume/pause/remove; a filter sidebar with status, label and
tracker groups, plus saved smart filters that AND several dimensions together.

<!-- TODO capture:
![Smart filters in the sidebar and the selection summary bar](docs/images/smart-filters.png)
-->

**Detail tabs** — General (with a pieces bar showing which chunks have landed),
Trackers (add/remove/enable/reannounce), Peers, Content, Speed, and Log.

<!-- TODO capture:
![The pieces bar on the General tab](docs/images/pieces-bar.png)
-->

**Automation** — per-torrent speed limits via named throttle pools, and ratio
groups / seed goals set globally or per label.

## Quick start

```sh
npm install

# Run against the ten built-in fixture torrents — no daemon needed:
RSTORRENT_MOCK=1 npm run tauri dev

# Run against a real daemon (see docs/rtorrent-setup.md first):
npm run tauri dev
```

Mock mode is the fastest way to see the UI: it serves ten fixture torrents in
assorted states, with no rtorrent and no network.

## Connecting

Install and configure a daemon per [docs/rtorrent-setup.md](docs/rtorrent-setup.md),
then open **Preferences → Connection**, match the transport to your
`.rtorrent.rc`, and hit **Test connection** — it reports the rtorrent version.

<!-- TODO capture:
![Preferences → Connection](docs/images/preferences-connection.png)
-->

## Development

| Command | What it does |
|---|---|
| `npm run tauri dev` | Run the app (add `RSTORRENT_MOCK=1` for mock mode) |
| `npm test` | Frontend unit tests (Vitest) |
| `npm run typecheck` | `tsc --noEmit` |
| `npm run lint` | ESLint + Prettier check |
| `cargo test` (in `src-tauri/`) | Rust unit tests |
| `cargo clippy --all-targets -- -D warnings` | Rust lints |
| `cargo fmt --check` (in `src-tauri/`) | Rust formatting |
| `npm run tauri build` | Package `.app` + `.dmg` |

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs all of the
above on push and PR — the frontend checks on Linux, the Rust ones on macOS.

Tests that touch a live daemon or the Keychain are marked `#[ignore]` and are run
deliberately — see [docs/rtorrent-setup.md](docs/rtorrent-setup.md).

## Layout

```
plan.md · tasks.md · backlog.md   # architecture, tracker, ideas
design/                           # the "Dark Ops" design reference (authoritative)
docs/rtorrent-setup.md            # connecting to a live rtorrent
docs/images/                      # README screenshots
src/                              # React frontend (ipc, store, components, theme)
src-tauri/src/                    # Rust backend (rtorrent transports, poller, commands)
tools/scgi-http-bridge.py         # dev-only HTTP→SCGI bridge, stands in for nginx
```

## License

TBD.
