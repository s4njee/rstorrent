# rstorrent — Feature backlog (v3): expose the daemon

Third backlog. The v2 P1 slice has shipped; this one is themed around making
rstorrent **fully featured** — closing the gap to qBittorrent/ruTorrent and
exposing as much of rtorrent's actual capability surface as is sane in a GUI.

Grounded in reality, not docs: every method named below was confirmed present
by probing the live daemon (`system.listMethods` on rtorrent **0.16.17** —
870 methods; survey re-runnable with the same call). Old `B`/`C` numbers are
kept for carried items; new ideas use `D` numbers.

Priorities: **P1** (next releases) · **P2** (later) · **P3** (icebox).
Sizes: S ≤ 2 h, M ≤ half day, L ≈ day, XL = needs breaking down.

---

## 0 · Shipped so far

- **v1 core** — live table/sidebar/detail tabs over SCGI, add dialogs, file
  association + magnet scheme, notifications, dock badge, throttle pools,
  tracker management, columns, ratio groups/seed goals, pieces bar, packaging.
- **v1.4 slice** — C1 drag & drop · C2 paste-to-add · C3 selection bar ·
  C4 smart filters · C6 rate/ETA smoothing · C7 private-torrent affordances ·
  C21 CI (fmt/clippy/test on macOS, lint/types/vitest on Linux).
- **B9** — remote daemons over HTTP(S) XML-RPC, passwords in the Keychain.
- **v1.6 "network"** — D7 encryption/PEX · D8 tracker HTTP proxy · D9 bind/
  listen addresses · D11 global connection caps (all in a new Preferences →
  Network pane + Speed's Connection Limits) · B16 peer actions + C16 richer
  peer flags in the Peers tab. Network prefs are replayed on every (re)connect
  so they survive a daemon restart.
- **v1.7 "automation"** — C9 max-active-downloads queue · C11 per-label default
  save paths · C12 multiple watch folders · C13 client-side run-on-complete
  hook · C14 seed-goal action (stop/remove/remove-with-data) · B14 turtle mode
  (alt limits, manual toggle + daily schedule). The poller now owns global
  rate-limit application (turtle-aware, restart-surviving).
- **v2.0 "seedbox"** — B10 connection profiles · D12 native daemon views in the
  sidebar · D13 session controls (Daemon menu: save / shut down) · D16 daemon
  health tab in Statistics · B11 RSS feeds + auto-download rules (new RSS pane +
  background engine). **Deferred:** C22 (delta snapshots).

Still open from v1 close-out (tasks.md): `E13-S2` virtualization, `E13-S4`
accessibility, `E13-S5` QA-checklist run, `E14-S2` signing + clean-account QA.

---

## 1 · Carried over, still worth doing

- [ ] **B7 · Auto-update** (M) — needs E14-S2 signing first.
- [x] **B10 · Connection profiles** (M) — Preferences → Connection gains a
  profile dropdown: save the current connection under a name, then switch
  daemons from the list (Apply reconnects). Stored as `connection_profiles`;
  the active one is mirrored in `transport`. HTTP passwords stay per-endpoint in
  the Keychain. Shipped v2.0.
- [x] **B11 · RSS + auto-add rules** (XL) — the RSS Preferences pane (nav
  un-disabled): feeds, auto-download rules, poll interval, and a live per-feed
  preview with manual Download. A background engine (`rss.rs`) polls enabled
  feeds every `rss_poll_minutes`, matches enabled rules (space-separated
  must-contain-all / exclude-any, case-insensitive), and auto-adds via
  `load.normal` (handles both magnet links and `.torrent` URLs), deduped
  against a capped, persisted seen-set (`rss_seen.json`). RSS 2.0 + Atom parsing
  (quick-xml) prefers a torrent `<enclosure>` over a plain `<link>`. Shipped
  v2.0.
- [ ] **B12 · Move data on Set location** (L) — build with C10.
- [ ] **B13 · Torrent creation** (L).
- [x] **B14 · Scheduler / turtle limits** (M) — turtle mode: alternative
  down/up limits with a manual toggle (🐢 in the status bar) and an optional
  daily schedule (start/end time + weekdays, overnight-wrap aware). The poller
  computes the effective limits each tick from `chrono::Local` and pushes them
  on change; it now owns global-limit application, so limits also survive a
  daemon restart. Shipped v1.7. (Deep choke-group scheduling stays D14/icebox.)
- [x] **B16 · Peer actions** (S) — right-click a peer in the Peers tab to Snub
  (`p.snubbed.set`), Disconnect (`p.disconnect`), or Ban (`p.banned.set` +
  disconnect). Targets the peer by `HASH:p<p.id>`. Shipped with C16 (v1.6).
- [ ] **B17 · Menu-bar item + dock menu** (M).
- [ ] **B18 · Windows/Linux** (XL) · **B19 · Web UI** (L) · **B20 · l10n** (M)
  · **B21 · Import from other clients** (M) · **B22 · Light theme** (S) — icebox.
- [ ] **C5 · Label rename in sidebar** (S).
- [ ] **C8 · Availability overlay on pieces bar** (M) — `d.chunks_seen` confirmed.
- [x] **C9 · Max-active-downloads queue** (M) — Preferences → Speed → Queue.
  The poller keeps the highest-priority N incomplete torrents active and
  starts/stops the rest each tick (errored/hash-checking torrents excluded).
  0 = off. Shipped v1.7.
- [ ] **C10 · Move-on-complete** (L).
- [x] **C11 · Per-label defaults** (M) — per-label default save paths
  (Preferences → Downloads); the Add dialog pre-fills the save path when the
  typed label matches, and watch-folder adds resolve folder → label default →
  global. Shipped v1.7.
- [x] **C12 · Multiple watch folders** (M) — `watch_folders` list, each with an
  optional label and save path; the watcher spawns one per folder. The legacy
  single `watch_folder` is migrated in on load. Shipped v1.7.
- [x] **C13 · Run-on-complete hook** (S) — a client-side command run on
  completion with `%N`/`%F`/`%H` tokens, executed directly (no shell, so no
  injection; per-token substitution keeps spaced values one argument). Distinct
  from the daemon-side `execute.*` non-goal. Shipped v1.7.
- [x] **C14 · Auto-remove at seed goal** (S) — seed-goal action Stop / Remove /
  Remove-with-data (Preferences → BitTorrent), extending the poller's seed-goal
  handling; Remove-with-data trashes files for a local daemon. Shipped v1.7.
- [ ] **C15 · Per-file progress bars** (S) — fold into D6.
- [x] **C16 · Richer peer info** (S) — the Peers-tab Flags column now folds in
  `p.is_encrypted`/`p.is_incoming`/`p.is_obfuscated`/`p.is_preferred`/
  `p.is_unwanted` as E·I·O·P·U (legend in the column tooltip). Shipped with B16
  (v1.6).
- [ ] **C17 · Global transfer graph + history** (M).
- [x] **C18 · Announce countdown in Trackers** (S) — Next column shows
  "in 12m" from `t.activity_time_next`; Last column shows "4m ago" from
  `t.success_time_last`. A next-announce in the past (failing/overdue tracker)
  renders "—" rather than a misleading "115s ago". Landed with D18.
- [ ] **C19 · Quick Look / open from Content** (M) · **C20 · Start rtorrent
  from the app** (L) · **C22 · Delta snapshots** (M) · **C23 · Session
  export/import** (S) · **C24 · Homebrew cask** (S) · **C25 · Log tab
  upgrades** (S).

---

## 2 · D-items — per-torrent control (P1)

The biggest functional gaps against qBittorrent, all with confirmed methods.

- [ ] **D1 · Force recheck** (S) — `d.check_hash`. Context menu + ⌥⌘R.
  Progress is observable (`d.chunks_hashed`, `d.is_hash_checking`) → D19 shows
  it. The single most-missed action in the current menu.

- [ ] **D2 · Per-file priorities** (M) — `f.priority.set` (0 = skip,
  1 = normal, 2 = high) on the Content tab: click-to-cycle cell plus
  multi-select context menu (Skip / Normal / High). Skip is the headline —
  "don't download the sample folder" is table stakes. Absorbs C15 (progress
  bars in the same pass). After changing priorities call
  `d.update_priorities` (present) so rtorrent re-plans the download.

- [ ] **D3 · Per-torrent connection limits** (S) — `d.peers_max.set`,
  `d.peers_min.set`, `d.uploads_max.set` in the existing per-torrent limits
  dialog (RateLimitDialog grows a second section). Read side into General.

- [x] **D4 · Started / Finished columns** (S) — `d.timestamp.started` +
  `d.timestamp.finished` as optional sortable columns (both default-hidden,
  toggled from the column menu). **"Added" was dropped:** `d.load_date` is
  session-scoped — a probe showed it resets to *today* on every daemon
  restart, so it'd lie after any restart. The started/finished timestamps live
  in the resume file and survive. A durable added-date waits for D6 (sticky
  `d.custom` metadata).

- [ ] **D5 · Super seeding** (S) — initial-seed connection type via
  `d.connection_current.set = "initial_seed"` (readable via
  `d.connection_current`; `d.connection_seed` is the *default*-type getter and
  has no setter — verified against the method list).
  Checkbox in the per-torrent menu for complete torrents; badge in General.
  Niche but cheap, and rtorrent is one of the few clients that does it well.

- [ ] **D6 · Sticky per-torrent metadata** (M) — `d.custom.set`/`d.custom`
  (multi-key, distinct from custom1..5): record `added_by` (file / magnet /
  watch / RSS), original source path, and add-time. Survives restarts in the
  session, syncs to any other client reading the session. Feeds D4's Added
  column on daemons where `d.load_date` resets, and C23's export.

---

## 3 · D-items — network & protocol preferences (P1/P2)

A "Network" pane in Preferences, mapping rtorrent's global knobs. All
confirmed present; each is a labeled control with the daemon default shown.

- [x] **D7 · Encryption & PEX prefs** (M) — new Preferences → Network pane:
  encryption preset dropdown (`protocol.encryption.set` — disabled/allow/
  prefer/require flag lists) and a `protocol.pex.set` toggle. Encryption has no
  getter on 0.16.17, so the preset is persisted in settings and the UI says it
  shows the last-applied value. Shipped v1.6.

- [x] **D8 · Proxy support** (M) — Network pane: a proxy `host:port` field and
  an "apply to tracker HTTP requests" checkbox (`network.http.proxy_address.set`),
  with a warning that UDP trackers and peer connections bypass it. Shipped v1.6.

- [x] **D9 · Bind & listen controls** (M) — Network pane: `network.bind_address`
  and `network.local_address` fields alongside the existing port range, for the
  VPN-binding use case. Only pushed when set (rebinding drops connections), so
  clearing a bind takes effect on the next daemon restart. Shipped v1.6.

- [ ] **D10 · IP blocklist** (L) — `ip_tables.insert_table`/`add_address` +
  `network.block.ipv4.set`. Load a local P2P-format blocklist file, show
  loaded-range count. File parsing is ours; the daemon only takes ranges.

- [x] **D11 · Global connection caps** (S) — Preferences → Speed gains a
  "Connection Limits" group: max peers per torrent (`throttle.max_peers.normal`
  /`.seed`), global upload slots (`throttle.max_uploads.global`), global
  download slots (`throttle.max_downloads.global`); 0 = daemon default. Shipped
  v1.6.

---

## 4 · D-items — daemon integration (P2)

- [x] **D12 · Native views in the sidebar** (M) — the poller slow-polls
  `view.list` + per-view `d.multicall2` membership (excluding `main`/`default`),
  tags each torrent's DTO with its views, and the sidebar shows a **Views**
  group that filters like labels/trackers. Read-only (membership editing later);
  smart filters stay client-side. Shipped v2.0.

- [x] **D13 · Session controls** (S) — a **Daemon** menu: "Save Session"
  (`session.save`, runs immediately) and "Shut Down Daemon…"
  (`system.shutdown.normal`, behind a confirm dialog). Shipped v2.0.

- [ ] **D14 · Choke-group / scheduling primitives** (P3, XL) —
  `protocol.choke_heuristics.*`, `schedule2`. rtorrent's deepest knobs; a GUI
  that does them justice is a project. Icebox until someone actually asks.

- [x] **D15 · Raw XML-RPC console** (M) — a hidden-by-default power tool,
  reached only by ⌘/Ctrl+Shift+X (the native menu can't do the "hold ⌥"
  reveal, so there's no menu entry): method name + JSON-array args →
  pretty-printed result, with `system.listMethods` autocomplete. Every
  capability this file will never wrap stays reachable, and it doubles as our
  own probe UI. A new `xmlrpc` passthrough on `RtorrentApi` (mock answers the
  introspection calls) carries the call; policy lives in `xmlrpc_console.rs` —
  read-only unless "allow mutations" is armed for the session, and `execute.*`
  / `method.insert` / `method.erase` / `method.set_key` are refused always
  (also scanned inside `*.multicall` args so they can't be smuggled). Shipped.

- [x] **D16 · Daemon health panel** (M) — the Statistics dialog gains a
  **Daemon** tab: `system.client_version`/`api_version`, `session.path`,
  `pieces.memory.*` cache, open/max sockets, max open files, `network.http`
  max-open. (Uptime dropped — rtorrent exposes no reliable start time.) Shipped
  v2.0.

---

## 5 · D-items — observability (P1)

- [x] **D17 · Hash-check progress** (S) — while `checking`, the progress bar
  now tracks the verification sweep (`d.chunks_hashed` / `d.size_chunks`)
  rather than the byte-completion behind it. Derived in `to_dto` so no new UI —
  the existing progress column shows it. `chunks_hashed` equals completed
  chunks when idle, so the checking-status guard is load-bearing (tested both
  ways). D1 (Force recheck) was already shipped in the context menu.

- [x] **D18 · Tracker type + detail columns** (S) — Trackers tab gains a Type
  column (`t.type` → http/udp/dht) alongside the existing Seeds/Leeches scrape
  counts and the new Last/Next timing (C18). `t.last_announce` (previously a
  hardcoded-empty string in the DTO) is now the real `t.success_time_last`.
  Dropped `t.latest_new_peers`/`sum_peers` for now — Seeds/Leeches already
  cover swarm size and the row was getting wide.

- [ ] **D19 · Error taxonomy** (M) — classify `d.message` (tracker timeout vs
  unregistered vs storage error vs missing files) into distinct statuses and
  sidebar buckets, instead of one generic "trk error". The CachyOS episode
  showed the value; a missing-data error deserves different affordances
  (recheck/relocate) than a dead tracker (reannounce/remove tracker).

---

## Suggested release slices

- **v1.5 — "control"**: D1, D2, D17, D4, C5 (+ C18/D18 as the Trackers pass)
- **v1.6 — "network"**: D7, D8, D9, D11, B16+C16 as one Peers pass — ✅ shipped
- **v1.7 — "automation"**: C9, C11, C12, C13, C14, B14 — ✅ shipped
- **v2.0 — "seedbox"**: B10, D12, D13, D16, B11 ✅ shipped · C22 deferred
- Anytime: D3, D5, D6, D10, C17, C19, C23, C24, C25 (D15 ✅ shipped)
- Icebox: D14, B18–B22

## Non-goals (unchanged, deliberate)

- **Global tracker search** (B23) — client, not indexer.
- **`execute.*` / `method.insert` exposure** — arbitrary shell/config from a
  GUI is a security hole, not a feature; D15's console deliberately excludes
  them.
- **Embedding a BitTorrent engine** — rstorrent stays a *client*.
