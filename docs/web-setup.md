# rstorrent-web — self-hosting the browser UI

`rstorrent-web` serves the same UI the desktop app draws from any browser. It
runs **next to the rtorrent daemon** (a seedbox or home server), proxies
rtorrent's XML-RPC/SCGI interface as JSON under `/api/*`, and serves the
single-page app. The browser talks only to `rstorrent-web` — never to SCGI,
which is unauthenticated and must stay off the network.

```
browser ──HTTPS──▶ reverse proxy ──HTTP──▶ rstorrent-web ──SCGI/unix──▶ rtorrentd
                    (TLS, you)              (:9080, loopback)            (same box)
```

## Build

```sh
npm ci
npm run build:web                       # → dist-web/, embedded into the binary
cargo build --release -p rstorrent-web  # → target/release/rstorrent-web
```

The web bundle is embedded, so the release binary is self-contained. For
development, `--assets <dir>` serves the SPA from disk instead.

## Configure

Generate a password hash and drop it into a config file:

```sh
printf 'my-web-password' | rstorrent-web hash-password
# prints: password_hash = "$argon2id$v=19$..."
```

`rstorrent-web.toml` (looked up in the working directory, or pass `--config`):

```toml
listen = "127.0.0.1:9080"    # keep this on loopback; terminate TLS at a proxy
poll_ms = 1000

# Deployment hardening (WEB-06). A non-loopback bind is refused unless the
# session cookie can be marked Secure: either list the proxy so its
# X-Forwarded-Proto is honoured, or force the flag.
trusted_proxies = ["127.0.0.1"]   # whose X-Forwarded-For/-Proto are trusted
# secure_cookies = true           # force Secure when the proxy is not listed

[transport]                  # how to reach rtorrent — the crate's transports
kind = "unix"                # unix | tcp | http
path = "/home/user/.rtorrent/rpc.socket"
# kind = "tcp"  → host, port
# kind = "http" → url, username, password  (an nginx/ruTorrent-fronted daemon)

[auth]
mode = "password"            # password | none  (none is refused off loopback)
password_hash = "$argon2id$v=19$..."

[ui]
display_name = "SY"          # avatar initials

[paths]
save_path = "/data/torrents" # statvfs target for the disk card + Add default
# Move on complete (V3-14; co-located daemons only):
# incomplete_dir = "/data/torrents/.incomplete"  # new downloads land here…
# collision_policy = "error"   # …or "auto-rename" for (1), (2), … on clash
# [[paths.move_rules]]         # tag/label → destination, first match wins
# tag = "archive"
# destination = "/media/archive"

[queue]                        # complete queue policy (V3-17; 0 = off)
# max_active_downloads = 3     # at most N downloading
# max_active_uploads = 2       # at most N seeding
# max_active_torrents = 4      # at most N active in total
# queue_slow_limit_kbs = 50    # slower torrents are never held back

[[bandwidth]]                  # precedence display only (V3-18 / QUE-04);
                               # enforcement stays desktop-side for now
# id = "video"
# label = "video"
# down_kb = 1024
# up_kb = 256
# peers_max = 50
```

Overrides for these: `RSTORRENT_WEB_INCOMPLETE_DIR` (env) joins the list below.

Every field is overridable by env (`RSTORRENT_WEB_LISTEN`,
`RSTORRENT_WEB_DISPLAY_NAME`, `RSTORRENT_WEB_SAVE_PATH`,
`RSTORRENT_WEB_INCOMPLETE_DIR`, `RSTORRENT_WEB_POLL_MS`)
and by flags (`--listen`, `--assets`), with **flags > env > file**. Set
`RSTORRENT_MOCK=1` to run the ten fixture torrents with no daemon.

The rtorrent side is configured exactly as for the desktop app — see
[rtorrent-setup.md](rtorrent-setup.md) for a minimal `.rtorrent.rc` with a
`scgi_local` socket.

## Run

```sh
rstorrent-web --config /etc/rstorrent-web.toml
# then browse to the proxied HTTPS URL (see below)
```

### systemd

```ini
# /etc/systemd/system/rstorrent-web.service
[Unit]
Description=rstorrent web UI
After=network.target

[Service]
User=rtorrent
ExecStart=/usr/local/bin/rstorrent-web --config /etc/rstorrent-web.toml
# systemd stops with SIGTERM; the server drains in-flight requests (bounded).
KillSignal=SIGTERM
TimeoutStopSec=15
Restart=on-failure
RestartSec=2

# Hardening: the process needs its config directory (config + session store +
# UI settings) and the daemon socket, and nothing else.
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=/var/lib/rstorrent-web
# Point the config at a writable state dir so sessions survive a restart:
#   listen/… in /etc/rstorrent-web.toml, session-store.json written beside it,
#   or mount /var/lib/rstorrent-web and keep the config there.

[Install]
WantedBy=multi-user.target
```

**Health and readiness.** `GET /health` (liveness) answers `200 ok` while the
process is serving; `GET /ready` (readiness) answers `200 ready` only while the
rtorrent daemon is reachable, and `503 not ready` otherwise. Both are
unauthenticated and leak nothing. Use `/ready` for a load-balancer or a
healthcheck that should pull the box out of rotation when rtorrent is down, and
`/health` for a restart-on-hang check.

### Backup and restore

Back up three files, all beside the config (or in the working directory when
there is no `--config`):

| File | Holds | Sensitivity |
|---|---|---|
| `rstorrent-web.toml` | transport, auth, hardening, paths | **contains `password_hash`** — treat as a secret |
| `session-store.json` | hashed session tokens + expiry | logins; deleting it revokes everyone |
| `ui-settings.json` | theme/accent/density | not sensitive |

To restore, stop the service, copy the files back, and start it. Sessions
survive a restart as long as `session-store.json` and its directory are
writable; deleting the file is a hard "sign out everywhere". Rotating the web
password rewrites `password_hash` in the config file in place.

### Library export / import (V3-22)

The TopBar's archive button exports the library as a portable
`rstorrent-session/1` manifest (hashes, re-addable sources, trackers,
labels/tags, paths, priorities, limits, client metadata — never
credentials) and imports one back: stopped add, recheck, resume only verified
data, through a crash-safe `import-journal.json` beside the config. The web
dialog uploads a manifest file; all six operations ride the existing
`POST /api/cmd/{name}` route, so no extra routes or auth surface:

| Command | Args | Returns |
|---|---|---|
| `export_session_text` | — | manifest JSON text (downloaded as `session-manifest.json`) |
| `validate_session` | `manifestText`, `selected?`, `remapFrom?`, `remapTo?` | dry-run counts, errors/warnings, per-item plan |
| `import_session` | same as validate + `resume?` | immediately; poll `import_status` |
| `import_status` | — | `{ running, added, resumed, skipped, failed, done }` |
| `cancel_import` | — | stops after the current torrent's step |
| `scan_foreign` | `client` (`qbittorrent`/`transmission`), `dir` | read-only discovery; `dir` is a path **on the server host**, so it only helps when the server is co-located with the migrated client |

The server records global rate caps as 0/0 in exports — it does not manage
them, and inventing numbers would be worse than omitting them.

### Docker

```dockerfile
# Multi-stage: build the SPA, build the binary, ship a distroless image.
FROM node:22-slim AS web
WORKDIR /app
COPY package*.json ./
RUN npm ci
COPY . .
RUN npm run build:web

FROM rust:1-slim AS server
WORKDIR /app
COPY --from=web /app /app
RUN cargo build --release -p rstorrent-web

FROM gcr.io/distroless/cc-debian12
COPY --from=server /app/target/release/rstorrent-web /rstorrent-web
EXPOSE 9080
ENTRYPOINT ["/rstorrent-web"]
```

```sh
docker run --rm -p 9080:9080 \
  -v /home/user/.rtorrent:/home/user/.rtorrent \
  -v /etc/rstorrent-web.toml:/rstorrent-web.toml \
  rstorrent-web --listen 0.0.0.0:9080
```

Binding a non-loopback address logs a warning: **put TLS in front**. `auth.mode
= "none"` is refused entirely on a non-loopback bind, and the server **refuses to
start** a non-loopback bind unless the session cookie can be marked `Secure` —
set `trusted_proxies = ["<proxy ip>"]` (the docker bridge gateway, e.g.
`172.17.0.1`) or `secure_cookies = true` in the config. Sessions need a writable
directory beside the config: mount one (e.g. `-v /var/lib/rstorrent-web:/data`
with the config at `/data/rstorrent-web.toml`) or they live only in memory.

## TLS at a reverse proxy

`rstorrent-web` speaks plain HTTP; terminate TLS at nginx or Caddy. The session
cookie is `HttpOnly; SameSite=Strict; Path=/`, and is marked `Secure` when the
request arrives with `X-Forwarded-Proto: https` from a `trusted_proxies` address
(or always, with `secure_cookies = true`).

**Caddy**

```
torrents.example.com {
    reverse_proxy 127.0.0.1:9080
}
```

**nginx**

```nginx
server {
    listen 443 ssl;
    server_name torrents.example.com;
    ssl_certificate     /etc/letsencrypt/live/torrents.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/torrents.example.com/privkey.pem;
    add_header Strict-Transport-Security "max-age=63072000" always;

    location / {
        proxy_pass http://127.0.0.1:9080;
        proxy_set_header Host $host;
        # Both forwarded headers are needed: X-Forwarded-Proto makes the session
        # cookie Secure, and X-Forwarded-For gives the real client IP for login
        # rate limiting. The server honours them only from a `trusted_proxies`
        # address — list 127.0.0.1 (or the proxy's address).
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        client_max_body_size 12m;   # .torrent uploads (server caps at 10 MiB)
    }
}
```

Caddy sends `X-Forwarded-Proto` and `X-Forwarded-For` on a `reverse_proxy` by
default, so with Caddy list the proxy in `trusted_proxies` and no `proxy_set_header`
lines are needed. If you terminate TLS somewhere the server cannot see, set
`secure_cookies = true` instead.

## Security notes

- Never expose SCGI (`scgi_port`/`scgi_local`) to the network. `rstorrent-web`
  is the auth boundary; keep `listen` on loopback behind the proxy.
- One password, one shared session — this is a single-user tool (v1). Sessions
  are stored **hashed** and survive a server restart; "sign out everywhere" in
  the status dialog revokes them all (and rotating the password is another way
  to force every browser to log in again).
- Mutations require an `X-Rstorrent` header (CSRF defense-in-depth), which the
  app's `fetch` sends and cross-site forms can't. Responses carry
  `X-Content-Type-Options: nosniff` and `X-Frame-Options: DENY`.
- **Delete-with-data** is offered only when the server is co-located with the
  daemon (a unix-socket transport); a remote/HTTP transport gates it off, exactly
  like the desktop's localhost posture.

## The desktop app can serve it too

The macOS app serves this same UI from inside itself — **Daemon → Toggle Web UI**
— on the app's own daemon connection, loopback only and with no login, putting the
link on the clipboard. Nothing to configure: no `[transport]`, no `[auth]`, no
second process. Use the server below when you want it on a box *without* the
desktop app, or with a password, or behind a proxy.

## Local QA against the mock

The server authenticates every request, including `/api/health`, so it needs to be
told what to do about a login before it will start usefully. One file covers both
that and where the daemon is; drop it in the repo root (`rstorrent-web.toml`, read
from the working directory) or pass it with `--config`:

```sh
cat > rstorrent-web.toml <<'TOML'
# Your daemon's socket. Without a [transport] the server assumes
# ~/.rtorrent/rpc.socket, which is where a local daemon usually lives.
[transport]
kind = "unix"
path = "/Users/you/.rtorrent/rpc.socket"

[auth]
mode = "none"        # loopback only; a non-loopback bind refuses this
TOML
```

Then, two commands in two terminals:

```sh
RSTORRENT_MOCK=1 cargo run -p rstorrent-web      # fixtures on :9080, no daemon
npm run dev:web                                  # Vite on :1421, /api → :9080
```

Then open <http://localhost:1421>. Without `npm run dev:web`, the server serves the
embedded bundle at <http://localhost:9080> instead.

**Mock mode still needs an auth decision.** With `auth.mode = "password"` and no
`[auth].password_hash`, the server refuses to start rather than accepting a
password nobody can satisfy — mock runs included, because a server that refuses
every request looks identical to a broken UI. So the dev loop uses
`auth.mode = "none"`, which is refused on a non-loopback bind and therefore cannot
leak onto a network. To exercise the real sign-in path instead, put a hash in the
file — `rstorrent-web hash-password` reads a password on stdin and prints the
`[auth].password_hash` line — or add one to the same `[auth]` table.

**Against your own daemon**, drop `RSTORRENT_MOCK=1` and the same file serves the
real thing:

```sh
cargo run -p rstorrent-web        # :9080, talking to the socket above
```

### What the fixtures cover

`RSTORRENT_MOCK=1` serves fifteen torrents with no daemon: the fourteen sample
rows from the console design's data block (`design/web-console/README.md`), plus
one `media` row that also carries the stalled state. Between them they cover

- **statuses** — four downloading, six seeding, one stopped, one queued, one
  tracker error, one checking (rehashing at 42%), one stalled;
- **labels** — `iso`, `archive`, `kernel`, `apps`, `media`;
- **figures** — a 100% row, a stopped-and-incomplete row, a row with no peers, a
  private torrent, and the design's seeds/peers pairs (`38 / 112`, `12 / 44`,
  `0 / 26`).

Downloading rows advance as the mock runs, so progress, ETA and rates move.

### The design reference

The console's look is specified in [`design/web-console/`](../design/web-console/):
`README.md` is the authority (tokens, fixed heights, the five surfaces) and
`rTorrent Console.dc.html` is the prototype. Open the HTML in a browser next to
the running app to compare — its markup holds all five frames side by side, and
every sample value lives in its `renderVals()` block.

The superseded Dark Ops web spec is [`design/README.md`](../design/README.md),
kept as the record. The port is planned in
[`docs/web-console-plan.md`](web-console-plan.md).
