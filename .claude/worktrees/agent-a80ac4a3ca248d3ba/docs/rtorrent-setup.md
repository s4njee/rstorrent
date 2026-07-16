# Pointing rstorrent at a live rtorrent daemon

rstorrent is a *client* for an already-running `rtorrent` process — it does not
embed a BitTorrent engine. It talks to rtorrent over its XML-RPC-over-SCGI
interface. This document gets a local daemon running and connected.

> Prefer to explore the UI without a daemon? Launch with mock mode:
> `RSTORRENT_MOCK=1 npm run tauri dev` — the app runs against ten built-in
> fixture torrents.

## 1. Install rtorrent

```sh
brew install rtorrent
```

## 2. Minimal `~/.rtorrent.rc`

rstorrent defaults to a **unix socket** at `~/.rtorrent/rpc.socket` (no network
exposure — SCGI has no authentication, so this is the safe default).

The config below is **verified against Homebrew's rtorrent 0.16.17** (use
absolute paths — `~` isn't expanded inside `network.scgi.open_local`):

```ini
# ~/.rtorrent.rc  (replace /Users/you with your home path)

# Where downloads land (match this to rstorrent's default save path).
directory.default.set = /Users/you/Downloads

# Session directory so torrents persist across restarts.
session.path.set = /Users/you/.rtorrent/session

# --- SCGI: the socket rstorrent connects to ---
network.scgi.open_local = /Users/you/.rtorrent/rpc.socket

network.port_range.set = 6881-6899
```

Create the session dir and start the daemon. rtorrent is an ncurses TUI, so run
it inside `tmux` (or `screen`) to keep it alive in the background:

```sh
mkdir -p ~/.rtorrent/session
tmux new-session -d -s rtorrent 'rtorrent'   # detached; reattach with: tmux attach -t rtorrent
```

Confirm the socket came up: `ls -l ~/.rtorrent/rpc.socket`.

### Prefer a TCP port instead?

```ini
network.scgi.open_port = 127.0.0.1:5000
```

Then in rstorrent → Preferences → Connection, choose **TCP** with host
`127.0.0.1`, port `5000`. Keep it bound to localhost; a non-localhost SCGI port
is unauthenticated and rstorrent will warn about it.

## 3. Connect from rstorrent

1. Open **Preferences → Connection**.
2. Confirm the transport matches your `.rtorrent.rc` (socket path or host:port).
3. Click **Test connection** — it should report the rtorrent version.

The main window then goes live: the torrent list, speeds, and status bar update
on the poll interval (default 1s).

## Troubleshooting

- **"cannot reach rtorrent"** — the daemon isn't running, or the socket path /
  port doesn't match. Verify `ls -l ~/.rtorrent/session/rpc.socket` exists.
- **Faults on actions** — some methods require a recent rtorrent. Homebrew's
  `rtorrent` (verified on 0.16.17) is known-good.
- **rtorrent crashes when adding a magnet** — rtorrent 0.16.x can crash on a
  malformed magnet (e.g. an all-zero info-hash). Use real magnets; this is an
  upstream rtorrent input-validation bug, not rstorrent.
- **Nothing in the Tracker column** — hostnames resolve on a slower poll; give it
  a few seconds after first load.
