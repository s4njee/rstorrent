# rstorrent

A native macOS desktop client for the [`rtorrent`](https://github.com/rakshasa/rtorrent)
daemon, built with **Rust + Tauri 2** and a **React/TypeScript** frontend. It
implements the "Dark Ops" design in [`design/`](design/): a compact, monospace,
power-user torrent client in the mold of qBittorrent, front-ending rtorrent over
its XML-RPC-over-SCGI interface.

rstorrent is a *client* — it does not embed a BitTorrent engine.

## Status

Foundation complete (milestones M0–M1): live main window (toolbar, filter
sidebar, sortable torrent table, detail tabs, status bar) driven by a background
poller, running against either a live daemon or the built-in mock. See
[plan.md](plan.md) for the architecture and [tasks.md](tasks.md) for the epic/
story backlog and what's next.

## Quick start

```sh
npm install

# Run against the ten built-in fixture torrents — no daemon needed:
RSTORRENT_MOCK=1 npm run tauri dev

# Run against a real daemon (see docs/rtorrent-setup.md first):
npm run tauri dev
```

## Development

| Command | What it does |
|---|---|
| `npm run tauri dev` | Run the app (add `RSTORRENT_MOCK=1` for mock mode) |
| `npm test` | Frontend unit tests (Vitest) |
| `npm run typecheck` | `tsc --noEmit` |
| `npm run lint` | ESLint + Prettier check |
| `cargo test` (in `src-tauri/`) | Rust unit tests |
| `cargo clippy --all-targets -- -D warnings` | Rust lints |
| `npm run tauri build` | Package `.app` + `.dmg` |

## Layout

```
plan.md · tasks.md          # architecture + backlog
design/                     # the "Dark Ops" design reference (authoritative)
docs/rtorrent-setup.md      # connecting to a live rtorrent
src/                        # React frontend (ipc, store, components, theme)
src-tauri/src/              # Rust backend (rtorrent SCGI/XML-RPC, poller, commands)
```

## License

TBD.
