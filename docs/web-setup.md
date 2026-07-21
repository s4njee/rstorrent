# rstorrent-web — self-hosting the browser UI

`rstorrent-web` serves the same "Dark Ops" UI as the desktop app from any
browser. It runs **next to the rtorrent daemon** (a seedbox or home server),
proxies rtorrent's XML-RPC/SCGI interface as JSON under `/api/*`, and serves the
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
```

Every field is overridable by env (`RSTORRENT_WEB_LISTEN`,
`RSTORRENT_WEB_DISPLAY_NAME`, `RSTORRENT_WEB_SAVE_PATH`, `RSTORRENT_WEB_POLL_MS`)
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
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

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
= "none"` is refused entirely on a non-loopback bind.

## TLS at a reverse proxy

`rstorrent-web` speaks plain HTTP; terminate TLS at nginx or Caddy. The session
cookie is `HttpOnly; SameSite=Strict`; add `Secure` by serving over HTTPS.

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
        proxy_set_header X-Forwarded-Proto https;
        client_max_body_size 12m;   # .torrent uploads (server caps at 10 MiB)
    }
}
```

## Security notes

- Never expose SCGI (`scgi_port`/`scgi_local`) to the network. `rstorrent-web`
  is the auth boundary; keep `listen` on loopback behind the proxy.
- One password, one shared session — this is a single-user tool (v1).
- Mutations require an `X-Rstorrent` header (CSRF defense-in-depth), which the
  app's `fetch` sends and cross-site forms can't. Responses carry
  `X-Content-Type-Options: nosniff` and `X-Frame-Options: DENY`.
- **Delete-with-data** is offered only when the server is co-located with the
  daemon (a unix-socket transport); a remote/HTTP transport gates it off, exactly
  like the desktop's localhost posture.

## Local QA against the mock

```sh
RSTORRENT_MOCK=1 cargo run -p rstorrent-web     # serves fixtures at :9080
npm run dev:web                                  # Vite on :1421, proxying /api → :9080
```
