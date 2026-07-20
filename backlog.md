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

Still open from v1 close-out (tasks.md): `E13-S2` virtualization, `E13-S4`
accessibility, `E13-S5` QA-checklist run, `E14-S2` signing + clean-account QA.

---

## 1 · Carried over, still worth doing

- [ ] **B7 · Auto-update** (M) — needs E14-S2 signing first.
- [ ] **B10 · Connection profiles** (M) — unblocked by B9; pairs with D16.
- [ ] **B11 · RSS + auto-add rules** (XL) — fills the disabled prefs nav.
- [ ] **B12 · Move data on Set location** (L) — build with C10.
- [ ] **B13 · Torrent creation** (L).
- [ ] **B14 · Scheduler / turtle limits** (M) — see also D14.
- [ ] **B16 · Peer actions** (S) — now grounded: `p.banned.set`,
  `p.snubbed.set`, `p.disconnect` all present. Do together with C16.
- [ ] **B17 · Menu-bar item + dock menu** (M).
- [ ] **B18 · Windows/Linux** (XL) · **B19 · Web UI** (L) · **B20 · l10n** (M)
  · **B21 · Import from other clients** (M) · **B22 · Light theme** (S) — icebox.
- [ ] **C5 · Label rename in sidebar** (S).
- [ ] **C8 · Availability overlay on pieces bar** (M) — `d.chunks_seen` confirmed.
- [ ] **C9 · Max-active-downloads queue** (M) · **C10 · Move-on-complete** (L)
  · **C11 · Per-label defaults** (M) · **C12 · Multiple watch folders** (M)
  · **C13 · Run-on-complete hook** (S) · **C14 · Auto-remove at seed goal** (S).
- [ ] **C15 · Per-file progress bars** (S) — fold into D6.
- [ ] **C16 · Richer peer info** (S) — `p.is_encrypted`, `p.is_incoming`,
  `p.is_obfuscated`, `p.is_preferred`, `p.is_unwanted` all present.
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

- [ ] **D7 · Encryption & PEX prefs** (M) — `protocol.encryption.set`
  (require/prefer/allow-plaintext presets over the raw flag list),
  `protocol.pex.set` toggle. Write-mostly: encryption has no getter on
  0.16.17, so persist the last-applied preset in settings and say so in the UI.

- [ ] **D8 · Proxy support** (M) — `network.proxy.global.set` /
  `network.http.proxy_address.set`. One proxy URL field + "apply to tracker
  HTTP requests" checkbox. Warn that UDP trackers bypass an HTTP proxy.

- [ ] **D9 · Bind & listen controls** (M) — `network.bind_address.set`
  (with `.ipv4`/`.ipv6` variants), `network.local_address.set`, alongside the
  existing port range. The VPN-binding use case: bind to the tunnel interface
  address so traffic dies with the VPN instead of leaking.

- [ ] **D10 · IP blocklist** (L) — `ip_tables.insert_table`/`add_address` +
  `network.block.ipv4.set`. Load a local P2P-format blocklist file, show
  loaded-range count. File parsing is ours; the daemon only takes ranges.

- [ ] **D11 · Global connection caps** (S) — `throttle.max_peers.normal.set`,
  `throttle.max_uploads.global.set`, `throttle.max_downloads.global.set` etc.
  in Preferences → Transfers, next to the existing global rate limits.

---

## 4 · D-items — daemon integration (P2)

- [ ] **D12 · Native views in the sidebar** (M) — `view.list`, `d.views`,
  `d.views.push_back_unique`/`remove`. Surface the daemon's own views
  (`started`, `stopped`, `complete`, custom ones from .rtorrent.rc) as a
  sidebar group. Read-only first; view *membership* editing later. Smart
  filters stay client-side — these are the daemon-side complement visible to
  every client, not just ours.

- [ ] **D13 · Session controls** (S) — File menu: "Save session now"
  (`session.save`), "Shut down daemon…" (`system.shutdown.normal`, with
  confirm). The pair rstorrent conspicuously lacks for being a *client*:
  clean daemon lifecycle without reaching for tmux. Complements C20
  (start) — together they close the loop.

- [ ] **D14 · Choke-group / scheduling primitives** (P3, XL) —
  `protocol.choke_heuristics.*`, `schedule2`. rtorrent's deepest knobs; a GUI
  that does them justice is a project. Icebox until someone actually asks.

- [ ] **D15 · Raw XML-RPC console** (M) — a hidden-by-default power tool
  (Help ▸ hold ⌥): method name + args → pretty-printed result, with
  `system.listMethods` autocomplete. Every capability this file will never
  wrap stays reachable, and it doubles as our own probe UI. Read-only unless
  "allow mutations" is armed per-session.

- [ ] **D16 · Daemon health panel** (M) — surface what the daemon says about
  itself: `system.client_version`/`api_version`, `session.path`, uptime
  (`system.time` − start), `pieces.memory.*` cache stats, open sockets
  (`network.open_sockets`), `network.http.*` settings in effect. Lives in the
  Statistics dialog as a second tab. Pairs with B10 profiles.

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
- **v1.6 — "network"**: D7, D8, D9, D11, B16+C16 as one Peers pass
- **v1.7 — "automation"**: C9, C11, C12, C13, C14, B14
- **v2.0 — "seedbox"**: B10, D12, D13, D16, C22, B11
- Anytime: D3, D5, D6, D10, D15, C17, C19, C23, C24, C25
- Icebox: D14, B18–B22

## Non-goals (unchanged, deliberate)

- **Global tracker search** (B23) — client, not indexer.
- **`execute.*` / `method.insert` exposure** — arbitrary shell/config from a
  GUI is a security hole, not a feature; D15's console deliberately excludes
  them.
- **Embedding a BitTorrent engine** — rstorrent stays a *client*.
