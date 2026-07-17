# rstorrent — Feature backlog (v2)

Fresh idea backlog, superseding the first one now that its P1 slice has shipped.
Old `B` numbers are kept for items carried over; new ideas use `C` numbers so
references in commits/PRs stay unambiguous. As before: this file is for ideas,
[tasks.md](tasks.md) is the execution tracker for anything promoted.

Priorities: **P1** (next releases, v1.x) · **P2** (v2) · **P3** (icebox).
Sizes: S ≤ 2 h, M ≤ half day, L ≈ day, XL = needs breaking down.

---

## 0 · Shipped from the previous backlog

- [x] **B1** `.torrent` file association + `magnet:` URL scheme
- [x] **B2** completion notifications · **B3** dock badge (dock *menu* still open, see B17)
- [x] **B4** per-torrent speed limits (named throttle pool)
- [x] **B5** tracker management (add/remove/enable/reannounce)
- [x] **B6** resizable / customizable columns
- [x] **B8** ratio groups / seed-goal automation (global + per-label)
- [x] **B15** pieces map — shipped as the General-tab pieces bar (`514c09e`)

Still open from v1 close-out in tasks.md: `E13-S2` virtualization, `E13-S4`
accessibility, `E13-S5` QA-checklist run, `E14-S2` signing + clean-account QA.

---

## 1 · Carried over (unshipped B-items)

- [ ] **B7 · Auto-update** (M) — Tauri updater + GitHub Releases; needs
  Developer-ID signing (E14-S2) first. Menu: "Check for Updates…".
- [ ] **B9 · Remote daemons: HTTP(S) XML-RPC + basic auth** (L) — seedbox
  audience; keeps delete-data/reveal disabled off-localhost.
- [ ] **B10 · Multiple connection profiles** (M) — depends on B9.
- [ ] **B11 · RSS feeds + auto-add rules** (XL) — fills the disabled prefs nav.
- [ ] **B12 · Move data on Set location** (L) — same-volume rename, else
  copy+verify+erase with progress.
- [ ] **B13 · Torrent creation** (L) — File ▸ New Torrent…, build + seed.
- [ ] **B14 · Scheduler / alternative "turtle" limits** (M).
- [ ] **B16 · Peer actions** (S) — ban / snub / add peer.
- [ ] **B17 · Menu-bar status item + dock menu** (M) — needs native code beyond
  Tauri's API; the dock-menu half of B3 folds in here.
- [ ] **B18 · Windows/Linux ports** (XL) · **B19 · Web UI** (L) ·
  **B20 · Localization** (M) · **B21 · Import from other clients** (M) ·
  **B22 · Light theme** (S) — icebox, unchanged.
- **B23 · Global tracker search** — still deliberately **out** (client, not indexer).

---

## 2 · New ideas — P1: daily-driver friction

Things you notice in week two of real use.

- [x] **C1 · Drag & drop onto the window** (M)
  Drop `.torrent` files anywhere on the main window → add dialog (or instant-add
  per prefs), with a drop overlay while a torrent-carrying drag hovers. Uses
  Tauri's native drag-drop for real file paths; dragged *text* (a magnet from a
  browser) is handled best-effort via DOM events, since the native handler owns
  file drags — paste (C2) is the reliable magnet route.

- [x] **C2 · Paste to add** (S)
  ⌘V on the main window with a magnet or `.torrent` URL in the clipboard adds it
  (respecting the show-dialog pref). Hooks the DOM `paste` event, not a keydown:
  the native Edit menu owns the ⌘V accelerator. Pastes into a text field, and
  clipboard text that isn't a torrent, are left alone.

- [ ] **C3 · Selection summary bar** (S)
  With ≥2 rows selected, a slim strip above the detail tabs: `4 selected ·
  18.2 GiB · ↓ 3.1 MiB/s` plus resume/pause/remove buttons. Multi-select
  currently gives no aggregate feedback at all ("multiple torrents selected").

- [ ] **C4 · Smart filters (saved searches)** (M)
  Persisted queries combining status + label + tracker + text (e.g. "stalled
  linux-isos"), shown as a fourth sidebar group. The sidebar today filters on
  exactly one dimension at a time; power users keep re-typing searches.

- [ ] **C5 · Label management in the sidebar** (S)
  Right-click a label → rename (rewrites `d.custom1` across matching torrents)
  or remove label. Labels can currently be set per-torrent but never renamed
  without touching every torrent by hand.

- [ ] **C6 · Rate & ETA smoothing** (S)
  EMA over the last ~5 poll samples for the Down/Up columns and ETA so numbers
  stop flickering every second. Pure frontend change in the snapshot
  reconciler; the raw values stay available to the Speed tab.

- [ ] **C7 · Private-torrent affordances** (S)
  `isPrivate` is already in the DTO but rendered nowhere. Show a small badge in
  the General tab and a dimmed marker in the table; disable "Copy magnet link"
  for private torrents with a tooltip (a bare-hash magnet is useless/leaky for
  private trackers).

- [ ] **C8 · Availability overlay on the pieces bar** (M)
  Second lane under the completion bar showing swarm availability per piece,
  plus an `availability: 3.4×` figure in the caption. `d.chunks_seen` is
  confirmed present on rtorrent 0.16.17 (probed alongside `d.bitfield`); the
  canvas/bucketing machinery from the pieces bar is reusable as-is.

---

## 3 · New ideas — P1/P2: automation

- [ ] **C9 · Max-active-downloads scheduler** (M)
  "Download at most N torrents at once": app-enforced queue in the poller —
  excess downloading torrents are held stopped and auto-started as slots free
  up, honoring priority order. rtorrent has no real queue (our up/down buttons
  only nudge `d.priority`); this makes the queue actually mean something.

- [ ] **C10 · Move-on-complete** (L)
  Download into an incomplete dir, move to the final (per-label) destination on
  completion: stop → move (rename same-volume, else copy+verify) → `d.directory.set`
  → start. Shares its data-move machinery with B12 — build them together. Also
  un-disables the "keep incomplete torrents in" pref that currently ships as a
  tooltip apology.

- [ ] **C11 · Per-label defaults** (M)
  Per-label save path and start-state applied by the add dialogs, watch folder,
  and deep-link adds. B8 already established the per-label settings shape
  (`label_seed_goals`) — extend the same pattern.

- [ ] **C12 · Multiple watch folders** (M)
  The watch folder is a single string today. Allow several, each with its own
  label/save path (which C11 makes meaningful), plus a per-folder "delete
  .torrent after load" option instead of the `.loaded` rename.

- [ ] **C13 · Run-on-complete hook** (S)
  Optional user-configured shell command on completion, with `RST_NAME`,
  `RST_PATH`, `RST_LABEL`, `RST_HASH` env vars. Off by default, configured only
  in Preferences (never from torrent data), output captured to the app log.

- [ ] **C14 · Auto-remove at seed goal** (S)
  Extension to B8: when a seed goal is met, optionally remove the torrent from
  the session (data stays; optional trash of the source `.torrent`). Turns the
  seed-goal engine into full lifecycle automation.

---

## 4 · New ideas — P2: information depth

- [ ] **C15 · Per-file progress bars in Content** (S)
  Replace the bare percentage column with the table's mini progress bar; the
  per-file `f.completed_chunks` data is already fetched.

- [ ] **C16 · Richer peer info** (S)
  Add snubbed/interested/choked flags (`p.is_snubbed` etc.), make Peers columns
  sortable, and show a per-peer progress bar. Pairs with B16 (peer actions) —
  do both in one pass on the Peers tab.

- [ ] **C17 · Global transfer graph + history** (M)
  Clicking the status-bar rates opens a popover with a session-wide speed chart
  (the per-torrent SpeedChart generalizes directly). Persist daily up/down
  totals alongside the existing since-install counters and grow the Statistics
  dialog a small history chart.

- [ ] **C18 · Announce countdown in Trackers** (S)
  "next announce in 12m" per tracker row. Needs a probe for which timing
  fields 0.16.17 exposes (`t.activity_time_next` / `t.success_time_last` were
  not part of the B5 probe) — S if present, drop if not.

---

## 5 · New ideas — P2: macOS-native

- [ ] **C19 · Quick Look & open from Content** (M)
  Space = Quick Look the selected file, double-click = open with default app;
  enabled only for 100%-complete files on a localhost daemon. The most
  mac-native thing a torrent client can do.

- [ ] **C20 · Start rtorrent from the app** (L)
  Opt-in daemon management: when the disconnected card detects a local
  `rtorrent` binary (brew paths), offer "Start rtorrent" — spawn it detached
  with the session dir, supervise, and offer stop-on-quit. **This reverses a
  stated v1 non-goal** (no process management) — worth it because tmux is the
  single biggest onboarding hurdle in docs/rtorrent-setup.md. Scope it to
  launch/stop only: no config editing, no bundled binary.

---

## 6 · New ideas — engineering & distribution

- [ ] **C21 · GitHub Actions CI** (M)
  No CI exists. Lint + typecheck + vitest + `cargo clippy/test` on PR; release
  workflow producing the `.dmg` artifact (unsigned until B7/E14-S2 land).
  Should come first — everything else in this file benefits.

- [ ] **C22 · Delta snapshots over IPC** (M)
  The poller currently emits the full torrent list every second; the frontend
  reconciles by identity. Send `{changed: [...], removed: [...]}` deltas keyed
  by hash instead. Only matters at hundreds+ of torrents — implement together
  with `E13-S2` virtualization as one "scale pass," measured before/after.

- [ ] **C23 · Session export / import** (S)
  Export all torrents (source `.torrent`/magnet, save path, label, state) to a
  JSON bundle; import re-adds with skip-hash-check. Complements B21
  (import from other clients) and doubles as a backup story.

- [ ] **C24 · Homebrew cask** (S)
  `brew install --cask rstorrent` once B7/signing exist. Cheap distribution;
  the audience already has brew (rtorrent came from it).

- [ ] **C25 · Log tab upgrades** (S)
  Level filter (info/warn/error), copy-to-clipboard, and export-to-file. The
  ring buffer and Log tab exist; this is UI only.

---

## Suggested release slices

- **v1.4 — "feels native"**: ~~C1~~, ~~C2~~ (shipped), C3, C5, C6, C7 (+ C21 CI underneath)
- **v1.5 — "automation"**: C9, C11, C12, C14, B7
- **v1.6 — "depth"**: C8, C15, C16, C17 (+ E13-S2 with C22)
- **v2.0 — "seedbox"**: B9, B10, B11, C10/B12
- Anytime, independently: C13, C18, C19, C23, C24, C25, C20
