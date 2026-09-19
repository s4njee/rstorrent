# Pointing rstorrent at a live rtorrent daemon

rstorrent is a *client* for an already-running `rtorrent` process — it does not
embed a BitTorrent engine. It talks to rtorrent over its XML-RPC-over-SCGI
interface. This document gets a local daemon running and connected.

> Prefer to explore the UI without a daemon? Launch with mock mode:
> `cargo run -p rstorrent-gpui -- --demo` — the app runs against ten built-in
> fixture torrents.

## 1. The daemon

**You usually don't have to install anything.** The macOS app bundles its own
rtorrent (rtorrent + libtorrent **0.15.7**) and prefers it over a system
install. Use **Daemon → Start Daemon**, or the **Start rtorrent** button on the
disconnected card; if you have no `~/.rtorrent.rc` yet, it writes a minimal one
for you (an existing file is never modified).

To run against a *system* rtorrent instead:

```sh
brew install rtorrent
```

`RSTORRENT_RTORRENT_BIN=/path/to/rtorrent` overrides which binary Start uses.

> Windows has no native rtorrent; the daemon runs in WSL — see
> [docs/wsl-setup.md](wsl-setup.md).

## 2. Minimal `~/.rtorrent.rc`

rstorrent defaults to a **unix socket** at `~/.rtorrent/rpc.socket` (no network
exposure — SCGI has no authentication, so this is the safe default). The
**Start rtorrent** button writes this for the bundled daemon; the notes here are
for a manual setup. Use absolute paths — `~` isn't expanded inside
`network.scgi.open_local`:

```ini
# ~/.rtorrent.rc  (replace /Users/you with your home path)

# Where downloads land (match this to rstorrent's default save path).
directory.default.set = /Users/you/Downloads

# Session directory so torrents persist across restarts.
session.path.set = /Users/you/.rtorrent/session

# --- SCGI: the socket rstorrent connects to ---
network.scgi.open_local = /Users/you/.rtorrent/rpc.socket

# Listen range. The bundled 0.15.7 (and anything before 0.16.20) uses this name:
network.port_range.set = 6881-6899
# 0.16.20 renamed it — use this line instead on those builds:
# network.listen.port.range.set = 6881-6899
```

The app sets the port range over XML-RPC once connected, trying both names, so
it is optional in the file.

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
- **Faults on actions** — some methods require a recent rtorrent. The bundled
  **0.15.7** and Homebrew's **0.16.17** are both known-good; distro packages as
  old as 0.9.8 are missing a good part of the XML-RPC surface.
- **rtorrent crashes when adding a magnet** — rtorrent 0.15.x/0.16.x can crash on
  a malformed magnet (e.g. an all-zero info-hash). Use real magnets; this is an
  upstream rtorrent input-validation bug, not rstorrent.
- **Which binary did Start use?** — the app log line from **Start rtorrent**
  says `bundled` or `system`; `RSTORRENT_RTORRENT_BIN` overrides the choice. In
  the GPUI shell, `rstorrent-gpui --where-rtorrent` prints it without a window,
  and `rstorrent-gpui --start-rtorrent` starts one from a terminal.
- **Start says the daemon exited immediately** — rtorrent rejected something in
  `~/.rtorrent.rc` and quit, having already opened the socket (which is why the
  app can briefly believe it started). rtorrent reports the offending line on its
  own terminal, which the app deliberately leaves to the daemon, so run it in a
  terminal to read it. The usual cause is a key name belonging to another
  rtorrent version — see the port-range note above.
- **Nothing in the Tracker column** — hostnames resolve on a slower poll; give it
  a few seconds after first load.

## Connecting to a remote daemon (seedbox) over HTTP(S)

rstorrent can talk XML-RPC over HTTP(S) instead of a local socket — the standard
nginx/ruTorrent seedbox arrangement. In **Preferences → Connection**, pick
**HTTP(S)** and enter the RPC URL (e.g. `https://seedbox.example.com/RPC2`) plus
the username and password.

**Prefer `https://`.** HTTP Basic auth is base64, not encryption: over plain
`http://` to a remote host, anything on the network path can read the password.
rstorrent warns in Preferences and in the log when that's the case.

**Where the password lives.** In your macOS Keychain, never in `settings.json`
(which is plaintext). The app stores it when you hit Apply and reads it back on
connect; it's never returned to the UI, which is why the field shows
"•••••••• (saved)" rather than the value. Leave it blank to keep the saved one,
or use **Forget** to remove it.

macOS may ask you to allow rstorrent to read that Keychain item — most often
after an unsigned/ad-hoc rebuild changes the app's signature. Until you answer,
the connection sits at "connecting…". Click Allow (or re-enter the password in
Preferences, which rewrites the item under the current build).

**Remote daemons are gated.** Delete-data, reveal-in-Finder and free-space stay
disabled for a non-local daemon: its files aren't on this machine.

### Server side

Typical nginx front end for rtorrent's SCGI socket:

```nginx
location /RPC2 {
    auth_basic "rtorrent";
    auth_basic_user_file /etc/nginx/rtorrent.htpasswd;
    include scgi_params;
    scgi_pass unix:/home/you/.rtorrent/rpc.socket;
}
```

To try this locally without a seedbox, `tools/scgi-http-bridge.py` does the same
job as the nginx block above:

```sh
python3 tools/scgi-http-bridge.py           # http://127.0.0.1:8099/RPC2, alice/hunter2
```

Then point Preferences → Connection at `http://127.0.0.1:8099/RPC2`. It's a dev
tool only — no TLS, single-threaded.
