# rstorrent — Feature backlog

Forward-looking feature backlog: what to build **after** the v1 scope in
[tasks.md](tasks.md). Where an item already has a story ID there (E13/E15), it's
referenced, not duplicated — tasks.md stays the execution tracker for anything
promoted out of this file.

Priorities: **P1** (next release, v1.x) · **P2** (v2) · **P3** (icebox — revisit
when there's pull). Sizes as in tasks.md: S ≤ 2 h, M ≤ half day, L ≈ day,
XL = needs breaking down.

---

## 0 · v1 close-out (already tracked — listed for visibility)

Finish these in tasks.md before starting anything below:

- `E13-S2` performance pass / 1,000-row virtualization
- `E13-S4` accessibility & keyboard completeness
- `E13-S5` run the QA checklist
- `E14-S2` clean-account release QA + Developer-ID signing docs

---

## 1 · P1 — v1.x: daily-driver gaps

Things a real user hits in the first week of daily use.

- [ ] **B1 · `.torrent` file association + `magnet:` URL scheme** (M) — *tracked as E15-S1*
  Opening either from Finder/browser routes into the add dialogs (or instant-add
  per prefs). This is the single biggest friction point vs. every other client.

- [ ] **B2 · Completion notifications** (S) — *tracked as E15-S5*
  macOS notification on download-complete; per-label opt-out in Behavior prefs.
  Clicking the notification selects the torrent (and reveals in Finder if complete).

- [ ] **B3 · Dock badge & dock menu** (S)
  Badge with active-download count; dock menu with global pause/resume and
  current ↓/↑ rates. Cheap, very visible.

- [x] **B4 · Per-torrent speed limits** (M)
  `d.throttle_name.set` against named throttles (rtorrent has no true per-torrent
  cap, so: manage a small pool of named throttle groups). Context menu ▸ Limit
  download/upload… Surfaced in the General tab.

- [ ] **B5 · Tracker management** (M)
  Trackers tab: add tracker (`d.tracker.insert`), remove, enable/disable,
  **force reannounce** (also on the context menu). Today the tab is read-only.

- [ ] **B6 · Resizable / customizable columns** (M) — *tracked as E15-S4*
  Drag header edges, show/hide via header context menu, persisted with the rest
  of UI state (E13-S3 already persists sort).

- [ ] **B7 · Auto-update** (M)
  Tauri updater plugin + GitHub Releases feed. Requires Developer-ID signing
  (E14-S2) first. Menu: "Check for Updates…".

- [ ] **B8 · Ratio groups / seed-goal automation** (L)
  Per-label or per-torrent stop conditions: stop (or remove) at ratio X or after
  seeding N hours, mapped onto rtorrent's ratio scheduler (`group.seeding.*`).
  Prefs ▸ BitTorrent section grows a "Seeding limits" block.

---

## 2 · P2 — v2: bigger bets

- [ ] **B9 · Remote daemons: HTTP(S) XML-RPC + basic auth** (L) — *tracked as E15-S2*
  For nginx/ruTorrent-fronted seedboxes. Keeps delete-data/reveal-in-Finder
  disabled for non-local daemons. Unlocks the seedbox audience.

- [ ] **B10 · Multiple connection profiles** (M)
  Named daemon profiles (home, seedbox…) with a fast switcher in the title bar
  / app menu. Depends on B9 to be interesting. Per-profile UI state.

- [ ] **B11 · RSS feeds + auto-add rules** (XL) — *tracked as E15-S6*
  Feed polling, per-feed rules (match/exclude regex, label, save path, start
  state), history de-dupe. Fills the disabled RSS prefs nav. Split into
  plumbing / rules engine / UI stories when promoted.

- [ ] **B12 · Move data on Set location** (L) — *tracked as E15-S3*
  Same-volume rename, else copy + verify + erase, with progress toast. Removes
  the "files are not moved" caveat.

- [ ] **B13 · Torrent creation** (L)
  File ▸ New Torrent…: pick files/folder, piece size (auto), trackers, private
  flag, source tag → build .torrent (pure Rust), optionally add-and-seed
  immediately.

- [ ] **B14 · Scheduler / alternative speed limits** (M)
  "Turtle mode" toggle in the status bar + time-of-day schedule in Prefs ▸ Speed
  (weekday/weekend grid), driving the global throttles.

- [ ] **B15 · Pieces map in detail panel** (M)
  Canvas strip of piece availability/completion (from `d.bitfield`) on the
  General or Content tab — classic power-user affordance, fits the Dark Ops
  aesthetic.

- [ ] **B16 · Peer actions** (S)
  Peers tab context menu: ban peer (`p.banned.set`), snub/unsnub, add peer
  manually (`d.add_peer`).

- [ ] **B17 · Menu-bar (status item) mini-widget** (M)
  Optional NSStatusItem: global rates at a glance, click for a compact popover
  list of active transfers, global pause. App can close to menu bar.

---

## 3 · P3 — icebox

- [ ] **B18 · Windows/Linux ports** (XL)
  Nothing in the stack precludes it (Tauri is cross-platform); needs a pass on
  window chrome (no overlay title bar), menus, Trash semantics, and packaging.

- [ ] **B19 · Web UI prefs section** (L)
  Serve the frontend over HTTP for browser access (the second disabled prefs
  nav item). Overlaps heavily with B9; decide later if it's worth it vs. just
  recommending ruTorrent for that use case.

- [ ] **B20 · Localization** (M)
  Externalize strings; the all-monospace design keeps layout risk low. Wait for
  demand.

- [ ] **B21 · Import from other clients** (M)
  One-shot migration: scan qBittorrent/Transmission session dirs, re-add
  torrents with save paths + labels preserved (fast-resume via skip-hash-check).

- [ ] **B22 · Light theme** (S)
  Second token set in `tokens.css` behind `prefers-color-scheme` / a prefs
  toggle. Design work is the real cost, not code.

- [ ] **B23 · Global search across trackers** (—)
  Deliberately **out**: rstorrent is a client, not an indexer. Recorded here so
  it's a decision, not an omission.

---

## Suggested release slices

- **v1.1** — B1, B2, B3 (native-citizen polish: associations, notifications, dock)
- **v1.2** — B5, B6, B4 (power-user table & tracker control)
- **v1.3** — B7, B8 (auto-update + seeding automation)
- **v2.0** — B9, B10, B11 (remote daemons + RSS: the seedbox release)
