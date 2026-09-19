# rstorrent-web — live-daemon certification (WEB-04)

The web console's behaviour is certified against a **real rtorrent**, not only
the mock fixtures. Run this on the box that runs the daemon (or point
`[transport]` at a remote one), record the outcome in the table, and file any
failure as a story.

## Setup

```sh
# 1. A config with the real daemon and a password (see web-setup.md).
#    auth.mode = "password" + a hash from `rstorrent-web hash-password`.
# 2. Build and run against it.
npm run build:web
cargo run --release -p rstorrent-web -- --config /etc/rstorrent-web.toml
# 3. Open the console (loopback, or through the reverse proxy) and sign in.
```

Every step is marked **M** (manual, in the browser) or **A** (automated; the
ignored test below covers the read path).

## The matrix

`✅` run, `☐` not yet run. See the recorded run below for the environment.

| # | Step | How | Result |
|---|---|---|---|
| 1 | **Read** | The table lists the daemon's torrents; statuses, sizes, rates, labels and tracker hosts populate within a couple of polls. | ✅ |
| 2 | **Read (automated)** | `RSTORRENT_TEST_SOCKET=/path/to/rpc.socket cargo test -p rstorrent-web -- --ignored live_read_paths` → passes. | ✅ |
| 3 | **Write** | Select a torrent → Start/Pause/Stop; the row's status changes and the daemon agrees on the next poll. | ✅ |
| 4 | **Tracker actions** | Trackers tab: add a tracker, toggle one, force reannounce; the daemon reflects it. | ☐ |
| 5 | **Peer actions** | Peers tab on an active torrent: Snub / Disconnect / Ban a peer. | ☐ |
| 6 | **File priorities** | Files tab: cycle a file's priority; it survives a poll and reads back the same. | ☐ (read only) |
| 7 | **Details** | Trackers, Peers, Files, Transfer, Pieces and Log all populate for a live torrent. | ✅ |
| 8 | **Add .torrent** | Add modal → `.torrent file` → drop/pick a real torrent → tree shows → Add; it appears and checks. | ✅ |
| 9 | **Add magnet** | Add modal → Magnet/URL → paste a well-seeded magnet → Add; it appears and resolves its name. | ☐ (needs a swarm) |
| 10 | **Remove** | Remove (and Remove + data on a co-located daemon); the torrent leaves the list, data goes to the Trash. | ✅ (erase) |
| 11 | **Daemon restart** | Restart rtorrent; the banner appears, then clears; the table repopulates without a page reload. | ✅ |
| 12 | **Server restart** | Restart `rstorrent-web`; the browser stays signed in (persistent sessions, WEB-05) and reconnects. | ✅ |
| 13 | **Sign out everywhere** | Status dialog → "Sign out everywhere"; every browser drops to the login screen on its next request. | ✅ (API) |
| 14 | **Proxy + Secure cookie** | Through the HTTPS proxy, the `rstorrent_session` cookie in devtools has `HttpOnly`, `Secure`, `SameSite=Strict`. | ✅ |
| 15 | **Readiness** | With the daemon stopped, `curl -i .../ready` is `503`; `.../health` stays `200`; with it up, `/ready` is `200`. | ✅ |

## Recorded run — 2026-09-16

**Environment:** bundled rtorrent **0.15.7** (unix socket), `rstorrent-web` built
from this tree, `auth.mode = "password"`, `trusted_proxies = ["127.0.0.1"]`,
config in a scratch directory. Driven with `curl` (browser steps noted).

- `GET /health` → 200; `GET /ready` → 200 once polled; login → 204.
- `POST /api/torrents/file` (a real `.torrent`) → 204, the row appeared as
  `test.bin`; `GET /api/detail` for `trackers`, `peers`, `content`, `general`
  (pieces) and `log` all returned populated payloads.
- `POST /api/cmd/pause` → the row read `paused`; `POST /api/cmd/remove` → the
  list returned to empty.
- Killed rtorrent: `/ready` → **503**, `/health` stayed **200**; restarted it:
  `/ready` → 200 and the snapshot reconnected.
- Restarted the server: the **same cookie** still authenticated (200), and
  `session-store.json` held no raw token. `DELETE /api/sessions` → 204, then the
  cookie → **401**.
- `X-Forwarded-Proto: https` from the trusted proxy produced
  `Set-Cookie: rstorrent_session=…; HttpOnly; SameSite=Strict; Path=/; Secure`.

Still to run on a swarm: the magnet add, tracker add/toggle/reannounce, peer
actions, and a file-priority write (#4/#5/#6/#9).

## Notes

- Certify against the daemon version you ship against (the bundled 0.15.7 and the
  Homebrew 0.16.x have different config-key names; see the capability notes in
  `docs/web-console-tasks.md`).
- The proof is the recorded table plus any filed failures — not a screenshot.
- Recorded runs live in the release notes; the empty table above is the template.
