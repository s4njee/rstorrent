# rstorrent — Backlog v3: a viable alternative to qBittorrent and ruTorrent

Last reviewed: 2026-09-16. Supersedes [backlog-v2.md](backlog-v2.md) as the
product backlog; [backlog.md](backlog.md) stays the shipped-method ledger. IDs
from v2 (`FND`, `WEB`, `REL`, `LIB`, `QUE`, `NET`, `AUT`, `OBS`, `PLT`, `UX`,
`PWR`, `LAB`) and the older `B`/`C`/`D` numbers are carried so nothing has to be
renumbered; new work uses `V3-` numbers.

## 0 · What "viable alternative" means here

Someone who runs qBittorrent on a desktop, or ruTorrent on a seedbox, should be
able to switch to rstorrent and not miss anything they use **weekly**. That is a
narrower bar than "every feature" and a higher bar than "every method exposed".
Concretely, a switcher expects:

1. **An install that just works** — signed build, updates, a first-run that finds
   or starts a daemon, a web UI they can leave running on a box.
2. **The daily loop** — add (file / magnet / RSS / watch), organise (labels,
   tags, categories, views), control (queue, limits, priorities, trackers), and
   finish (move-on-complete, seed goals, cleanup) without touching a config file.
3. **Honest answers when something is wrong** — why is this stalled, is my port
   open, did the move succeed, what did the tracker say.
4. **Scale** — a seedbox library of thousands of torrents stays responsive.

Everything below is ordered **most useful → most exotic**. Tier 0 is what a
switcher hits in the first hour; Tier 1 what they reach for every week; Tier 2
what a power user or seedbox operator reaches for monthly; Tier 3 is
differentiating work that has to prove itself. **Within a tier, items are in
priority order.**

### The three shells, and why ordering starts there

rstorrent currently has three UI surfaces over one Rust core (`crates/rtorrent`):

| Shell | State (2026-09-16) |
|---|---|
| **Tauri + React desktop** (`src-tauri/`, `src/`) | The feature-complete behavioural reference: every shipped B/C/D item lives here. Being restyled to the "rTorrent Console" design. |
| **GPUI native desktop** (`crates/gpui/`) | Port in progress ([GPUI.md](GPUI.md)): G0–G5 and G9 done. **Not ported:** Preferences, Statistics, tray, drag & drop, paste-to-add, watch folders, file/magnet association, and the poller *policy* (turtle mode, seed goals, max-active queue). |
| **Web UI** (`server/`, `src/web/`) | Console port in progress ([docs/web-console-tasks.md](docs/web-console-tasks.md)): WC0–WC2 done, WC3–WC10 open. Settings and Stats routes do not exist yet; Playwright, hardened deployment and persistent sessions are open. |

A feature that exists in one shell is not "shipped" for a switcher who installed
another. So every item below carries a **Shells** line (`T` Tauri, `G` GPUI,
`W` web) stating where it exists today, and the first Tier 0 item is the shell
decision itself. Until that decision lands, new feature work goes into the
shared core and the Tauri shell (the reference), and the GPUI port catches up
as part of its own epics.

### Daemon facts that constrain the list

Probed on the bundled rtorrent **0.15.7** (807 methods) on 2026-09-16; the
0.16.17 survey in `backlog.md` (870 methods) agrees on every point below.

- **No sequential download.** No `d.down.sequential*` in rakshasa rtorrent;
  the `jesec/rtorrent` fork has it. Sequential/first-last-piece is therefore a
  *capability-gated* feature, never a promise.
- **No per-file rename.** No `f.set_path` / `f.path.set`. `d.name` is
  read-only. `d.directory_base.set` and `d.directory.set` exist, so renaming the
  *root folder* is a client-side move plus a directory set; renaming a single
  file inside a multi-file torrent is impossible over RPC.
- **Blocklists are supported:** `ip_tables.insert_table` / `add_address`,
  `network.block.ipv4.set`.
- **DHT / UDP knobs exist:** `dht.mode.set`, `dht.port.set`, `dht.statistics`,
  `dht.add_node`, `trackers.use_udp.set`.
- **`d.free_diskspace`** exists per torrent — the free-space-on-destination
  question can be answered from the daemon, not only from local `statvfs`.
- **Choke groups:** ~54 `choke_group.*` methods are present (PWR-02 stays
  feasible, just deep).
- **No BitTorrent v2.** rtorrent parses v1 only; v2/hybrid torrents need a
  visible "unsupported by this daemon" explanation, not silent failure.

### Sizes and status

**S** ≤ 2 h · **M** ≤ half day · **L** ≈ a day · **XL** must be split into
stories before work starts. `[ ]` open · `[~]` in progress · `[x]` done ·
`[?]` discovery only.

---

## Tier 0 · Table stakes — what a switcher hits in the first hour

The gap here is not features; it is that the features are not *installable and
reachable* in one shell on one platform with one click. Nothing in Tier 1
matters to someone who cannot get past the first launch.

- [~] **V3-01 · Decide the desktop shell, then reach parity** (XL) —
  **Decision (2026-09-16): (a) — GPUI becomes the desktop; the Tauri shell is
  retired once G6–G8 land.** Until then the Tauri shell stays the behavioural
  reference but takes no new features; new feature work goes into the shared
  core and the GPUI shell. Shell-parity stories are split out in
  [GPUI.md §11](GPUI.md); the XL gate ("must be split into stories before work
  starts") is met by that section.
  **Landed so far:** the decision and the story split; **Preferences**
  (G6-S1–S5 — nine-pane dialog with validated Apply, daemon push, and the
  Web UI stub); the poller policy in GPUI
  (turtle B14 incl. the Daemon-menu toggle, seed goals B8/C14, max-active queue
  C9); free-space probing; and **Statistics** (G6-S6 — session/cache counters,
  daemon self-report, persisted since-install totals). **Remaining:** the
  tray (G7-S1), drag & drop / paste / watch
  folders (G7-S2–S4), associations (G7-S5), and the G8 hardening epic.
  **Shells:** T ✓ (frozen reference) · G partial. Whichever wins, the *shipped* desktop must have:
  Preferences (all nine panes: Behavior, Downloads, Connection, Speed,
  BitTorrent, Network, RSS, Web UI, Advanced), Statistics (History + Daemon
  tabs), the tray/menu-bar item (B17), drag & drop (C1), paste-to-add (C2),
  watch folders (C12), `.torrent`/`magnet:` association (B1), and the poller
  policy (turtle B14, seed goals B8/C14, max-active queue C9). For GPUI that is
  epics **G6, G7** and the "not yet ported" list in GPUI.md §4; for Tauri it is
  finishing the console restyle. **Accept:** the QA checklist
  ([docs/qa-checklist.md](docs/qa-checklist.md)) passes on the chosen shell with
  no "not ported yet" message reachable from a menu.

- [ ] **V3-02 · Signed, notarized macOS build with auto-update** (XL) —
  **Shells:** T (release CI exists, ad-hoc signed) · G (no release leg).
  Developer ID signing, hardened runtime, notarized DMG, signed update
  manifest, rollback guidance. qBittorrent's "download, open, done" is the bar;
  a Gatekeeper warning on first launch loses most switchers before the app
  opens. Carries **REL-03**, **B7**, **E14-S2**. Follow with **PLT-05** Homebrew
  cask (M, carries C24) so `brew install --cask rstorrent` works.

- [ ] **V3-03 · First-run connection wizard** (L) — **Shells:** none. On first
  launch: detect a bundled/system/WSL rtorrent or a saved profile, offer *Start
  the bundled daemon* (already implemented behind Daemon → Start), *Connect to
  a remote seedbox* (HTTP(S) XML-RPC with the Keychain), or *Use mock/demo*.
  Test the connection before closing. Explain in one sentence why SCGI stays
  off the network. Carries **UX-01**. Without this the disconnected card is the
  first thing every new user sees.

- [~] **V3-04 · Finish the web console** (XL) — **Shells:** W. This is the whole
  ruTorrent story: a browser UI on a seedbox. **WC0–WC9 are done**, including
  the unified add modal (WC6), and WC10's Playwright suite now guards the
  console in CI. Remaining: the visual-parity checklist (WC10-S2), the manual
  keyboard/screen-reader passes (WC10-S3), the desktop regression and doc
  refresh (WC10-S4) and the performance budgets (WC10-S5) — see the WC6/WC7/WC8/
  WC9/WC10 results in [docs/web-console-tasks.md](docs/web-console-tasks.md).
  Carries **WEB-01/02/07**, **WE2-S8**, **WE6-S3**.

- [~] **V3-05 · Web deployment you can leave running** (L+M+M) — **Shells:** W.
  Three v2 items that together make the web UI trustworthy on a public box:
  **WEB-05** persistent secure sessions (survive server restart, revoke-all,
  HttpOnly/Secure/SameSite=Strict cookies); **WEB-06** deployment hardening
  (trusted-proxy config, body/time limits, upload temp cleanup, `/ready` vs
  `/health`, graceful shutdown, refuse unsafe non-loopback configs);
  **WEB-04** live-daemon certification (carries WE3-S6). Plus a documented
  reverse-proxy recipe (nginx + Caddy) and a systemd unit in
  [docs/web-setup.md](docs/web-setup.md).
  **WEB-05/WEB-06 done and WEB-04 recorded (2026-09-16):** sessions are stored
  hashed and survive restart, `DELETE /api/sessions` is sign-out-everywhere, the
  cookie is `Secure` behind a trusted proxy; hardening covers trusted proxies,
  a 30 s request timeout, structured auth logs, SIGTERM drain, `/health` +
  `/ready`, and an actionable refusal of unsafe non-loopback binds. The recorded
  live run (bundled rtorrent 0.15.7) is
  [docs/web-live-cert.md](docs/web-live-cert.md); only the swarm-only steps
  (magnet, tracker/peer/file-priority writes) remain unrun, so this stays `[~]`.

- [ ] **V3-06 · End-to-end test suites** (L + L) — **Shells:** T/G/W. Playwright
  over the web console in mock mode (**WEB-03**, carries WE6-S1 / WC10-S1), and
  a headless path for the GPUI shell (GPUI.md §5 says it is *blocked* on the
  `Services` runtime seam — that seam is the story). Every later item is cheaper
  once "does the add dialog still work" is a CI job rather than a person.

- [ ] **V3-07 · Reliability at seedbox scale** (L + L + L + M) — **Shells:**
  core. **FND-03** poll scheduling and backpressure (cap RPC concurrency, coalesce
  refreshes, prioritise the visible detail); **FND-05** capability handshake
  (probe version + `system.listMethods` once per connection, cache, gate every
  control on it, and *say why* a control is unavailable — this is also what makes
  the sequential/rename/v2 items honest); **FND-06** bulk-action partial failure
  (per-hash outcomes, retry, keep failures selected); **FND-04** versioned
  migrations for settings, `d.custom` keys, RSS seen-state. FND-01
  virtualisation and FND-02 delta snapshots already shipped.

- [ ] **V3-08 · Windows/WSL2 production pass** (XL) — **Shells:** T (works,
  unsigned) · G (WSL bridge "not ported"). Installer, code signing, first-run
  WSL2/rtorrent detection, path-translation edge cases, firewall guidance,
  associations, Credential Manager, update channel, clean-machine QA. Carries
  **REL-04**. Linux is **PLT-01** (AppImage/Flatpak, tray, portals, secret
  storage, associations) — same tier, one step later, because GPUI's wgpu
  backend makes it a real target for the first time.

- [ ] **V3-09 · Release QA matrix and dependency hygiene** (M + M + M) — **REL-02**
  (clean-user, upgrade, reconnect, sleep/wake, offline, HiDPI, remote daemon
  cases, archived per release), **REL-05** backup-before-upgrade of settings and
  client metadata, **REL-06** advisory scanning + pinned release inputs. Also
  clear the inherited debt recorded in web-console-tasks.md §Baseline (the four
  red `TorrentTable` tests, the two lint errors, the `fmt` delta) so the gate is
  "all green" again.

---

## Tier 1 · Everyday features — what a switcher reaches for weekly

Each of these exists in qBittorrent (qB) or ruTorrent (ruT) and is used often
enough that its absence is noticed within a week. Ordered by how many users hit
the gap, then by how cheap it is.

### Organise

- [x] **V3-10 · Categories and tags** (XL) — qB: categories (with save-path
  inheritance) + many-to-many tags; ruT: single label. rstorrent has one
  ruT-compatible label (`d.custom1`) with per-label save paths and seed goals.
  Add client-managed **tags** (multi, coloured, bulk edit, sidebar filter,
  stored in `d.custom` so they follow the session) while keeping the label as the
  category. Define sync across profiles and the web UI. Carries **LIB-01**.
  **Shells:** T ✓ label only.
  **Done in both shipped shells (2026-09-16):** `tags` on the DTO
  read/written via `d.custom=tags` (normalised, de-duplicated, delta-aware),
  `set_tags`/`add_tags`/`remove_tags` commands, and a console + GPUI Tags
  sidebar group, filter facet, search, coloured chips and bulk editor.
  **Sync is the daemon's session**: tags live in `d.custom`, so every profile
  and the web UI see the same set; colours are derived per name (the same hash
  in both shells). **Deferred polish (not in the AC):** user-configurable tag
  colours and per-tag icons; tag-derived automation is **V3-33**.
  GPUI stories: [GPUI.md §12](GPUI.md).

- [ ] **V3-11 · Saved views 2.0** (L) — nested AND/OR conditions, negation,
  relative-time filters ("added < 7 days"), pinned views, and *editable*
  daemon-view membership (`view.set_visible`/`view.set_not_visible`). Show
  whether a view is daemon-backed or local. Carries **LIB-02**, folds in
  **PWR-01**. **Shells:** T ✓ smart filters, read-only native views.

- [x] **V3-12 · Library search that includes filenames** (L) — search name,
  hash, tracker, label/tag, save path, *and contained filenames* from a bounded
  local index built lazily per connection, so "which torrent has `S02E04`" is
  answerable without opening each one. Carries **LIB-03**. **Shells:** T/W
  name-only.
  **Done (2026-09-16):** `rtorrent_core::file_index::FileIndex` is a bounded,
  lazily filled map of torrent → lowercased file paths (5000 torrents,
  2000 paths each, oldest-evicted, cleared per connection). The web server
  indexes a 5-torrent batch per slow tick and serves `GET /api/search?q=`;
  GPUI indexes the same way in the model and matches locally through
  `visible_with_files`. Both shells now also match **hash** and **save path**
  locally. A filename query with a cold index simply matches nothing — it never
  blocks a search.

- [x] **V3-13 · Duplicate detection before add** (M) — same info-hash → focus
  the existing row and offer to merge trackers; same name+size → warn; same
  destination folder already in use → warn. Never add silently. qB does the
  first; ruT does none. Carries **LIB-04**. **Shells:** none.
  **Done (2026-09-16):** `rtorrent_core::duplicates::detect` (with a rule-for-rule
  TS mirror) returns the strongest finding by priority — exact hash, then
  name+size, then the destination folder. The web add modal and both GPUI add
  dialogs show the warning; an **exact** match disables Add and offers **Show
  it** (select the row, clear the filter) and **Merge trackers**. The mock now
  reads a real `.torrent`'s own name/size/hash so the check is exercised
  end-to-end.

### Finish

- [x] **V3-14 · Move-on-complete + incomplete-data folder** (L + L, needs the
  move journal) — qB's "keep incomplete torrents in" + "copy .torrent to";
  ruT's autotools. Destination by label/tag with a free-space preflight
  (`d.free_diskspace` + local `statvfs`), collision policy, cross-volume
  copy-verify-erase, progress, cancel, and *resume after a crash* — which means
  a minimal **operation journal** first (the part of **FND-07** that covers
  moves only; import/export journaling comes later with V3-22). Carries
  **LIB-07**, **LIB-08**, **C10**. Set-location-with-move (B12) is the base.
  **Shells:** T ✓ · G ✓ · W ✓.
  **Done (2026-09-16):** shared `rtorrent_core::mover` driver (pure
  `decide_moves` with leave-alone semantics — intent only from recorded
  `final_dir` or a matching rule; `execute_move`: stop → preflight → collide
  → chunked copy with progress/cancel → `d.directory.set` → clear intent →
  resume; `resume_moves` verifies InProgress targets on restart;
  `MoveStore` journal `move-journal.json` beside settings/config) with
  1 MiB-chunk cancellable copy in `fs.rs`. All three pollers run it
  (Tauri `moves.rs`, server `moves.rs` + `GET /api/moves` +
  `cancel_move`/`retry_move` cmds, GPUI `policy.rs` stage with status-bar
  notice + row-menu Cancel/Retry). Manual set-location clears `final_dir`
  everywhere so automation never fights the user. UI: status-bar move pill
  with % + ×, Moves dialog (progress/error/Cancel/Retry), Downloads prefs
  (incomplete dir, collision policy, rule editor). Co-located daemons only;
  remote gets one honest line, never per-tick failures. Exit gate ("survives
  a kill mid-copy") covered by journal resume tests + live-move e2e tests on
  the server and GPUI paths.
  Earlier slices (same day): `rtorrent_core::complete` (resolution, routing,
  collision, preflight, `Journal`, `d.free_diskspace`, TS mirror); settings +
  intent plumbing (`incomplete_dir` / `move_rules` / `collision_policy` on all
  surfaces, add-path routing with `d.custom=final_dir`, multicall column 33).

- [ ] **V3-15 · Rename torrent root folder, honestly** (L) — `d.directory_base.set`
  exists; `f.set_path` does not. So: rename the torrent's *root folder* (stop →
  move → set directory base → recheck → resume, through the journal) for local
  and co-located daemons; per-file rename is shown as "not supported by rtorrent
  over RPC" rather than a greyed mystery. Display-name override stays
  client-side (`d.custom`). Carries **LIB-06**, scoped down. **Shells:** none.

- [ ] **V3-16 · Content quick actions** (M) — Quick Look / open with default
  app, reveal in file manager, copy full and relative path, open containing
  folder, from the Files pane and the row menu. Gated on co-location. Carries
  **LIB-05**, **C19**, **PLT-03**. **Shells:** T reveal only.

### Control

- [x] **V3-17 · Complete queue policy** (XL) — qB has *max active downloads*,
  *max active uploads*, *max active torrents*, *slow-torrent exemption* and
  **force start**; rstorrent has max-active-downloads only. Add the other three
  limits and force-start (exempt from the client scheduler), and show plainly
  when the client scheduler overrides daemon state. Carries **QUE-01**. Then
  **QUE-02** (L): move top/bottom/up/down with a stable order — rtorrent has
  priority levels, not positions, so use `d.priority` + a client-side sequence
  in `d.custom` and never fake an exact position. **Shells:** T ✓ · G ✓ · W ✓.
  **Done (2026-09-16):** shared `rtorrent_core::queue` driver — per-class caps
  plus a total cap with global-rank trim, slow floor (never touched, never
  counted), `d.custom=force_start` exemption, and edge-triggered user intent
  (a manual resume is never re-paused, a manual pause never restarted; memory
  pruned per tick, reset on reconnect). Holds **pause** rather than stop, so
  held-back torrents stay loaded and read "Queued" in every shell (console
  mapping unchanged; GPUI status cell now words paused+open the same way).
  Order is `(priority, queue_pos, started_at)` with `d.custom=queue_pos`
  (multicall cols 34/35, mock round-trips); Top/Bottom pin band 3/0 with
  extreme sequence values, Up/Down swap whole pairs. All three pollers enforce
  it (server enforcement is new), skipping seed-goal decrees and in-flight
  moves; the mover re-reads live active state so a queue promotion racing a
  move never copies under a running torrent. UI: queue caps + slow floor in
  both Speed panes + server `[queue]`, Top/Bottom toolbar buttons, Force start
  check items in both row menus, `forceStart` on the DTO. Exit gate
  ("deterministic") via rank-ordered emission + table tests on every layer.

- [x] **V3-18 · Bandwidth rules per label/tag + rich scheduler** (L + XL) —
  named profiles (down/up caps, peer limits, upload slots) applied by label/tag
  with a visible precedence (torrent override → label rule → turtle → global),
  built on the throttle pools B4 already allocates. Then extend turtle mode's
  single daily window into a weekly grid with per-day exceptions, pause-vs-limit
  modes, DST correctness and a "next change at" preview — qB's scheduler and
  ruT's *schedule* plugin. Carries **QUE-04**, **QUE-05**. **Shells:** T ✓ · G ✓
  (scheduler enforcement desktop-only; the web rail shows rule precedence).
  **QUE-04 landed earlier (same day) — shared `rtorrent_core::bandwidth`
  engine, adoption markers, precedence display, editors in both Speed panes;
  see the paragraph below. Done QUE-05 (2026-09-16):**
  shared `rtorrent_core::schedule` — weekly grid windows with per-day
  selection and limit-vs-pause modes, temporary override with wall-clock
  expiry, pure evaluation (override → pause → limit → manual → open),
  analytic next-change preview over eight days, legacy turtle-window import
  without file rewrites, and entry/exit pause memory that releases exactly
  what entry paused (mid-window user resumes win until the next entry).
  Evaluation reads the platform local clock, so wall-clock windows are
  DST-correct by construction. Both pollers enforce it (pause via
  scheduler-owned holds mirrored into the queue's `user_paused`, so the two
  never fight; the mover re-reads live state so promotions racing a move
  never copy under a running torrent); both Speed panes edit the grid with
  override controls, next-change preview, and one-click legacy import.
  Exit gate: table-tested evaluation/transitions/memory in core, hold/release
  e2e through `run_tick`, a11y tour still green.
  **QUE-04 landed earlier (same day):** shared `rtorrent_core::bandwidth`
  engine — first-matching-tag then label rules owning deterministic `rule_<id>`
  throttles, adoption recorded in `d.custom=throttle_rule`, release on rule
  removal, manual edits (throttle or caps) opting out so torrent override wins
  precedence; defined-once-per-session throttles with reconnect replay on both
  desktops. Precedence display (override → rule → turtle → global) in both
  detail rails + the web rail (server reports TOML `[[bandwidth]]` via
  `/api/settings`; enforcement stays desktop-side, matching the v1 no-manage
  posture). Rules editors in both Speed panes. Exit gate: adopt/release/steady
  covered per layer, steady ticks cost zero daemon traffic.

- [ ] **V3-19 · Tracker editor 2.0** (L) — tier grouping and reordering,
  multi-line paste/import, export, dedupe and normalise URLs, batch edit across
  selection, copy announce URL, per-tracker failure history. Preserve the
  private-torrent guard (C7). Carries **NET-04**. **Shells:** T ✓ add/remove/
  enable/reannounce.

- [ ] **V3-20 · Sequential and first/last-piece, capability-gated** (M) — only
  where the handshake (V3-07) finds `d.down.sequential.set` (the jesec fork).
  On rakshasa builds the Add dialog's disabled "Sequential download" box gets a
  tooltip that says why, and the daemon-side alternative (per-file priority on
  the first file, already possible) is offered. Carries **QUE-03**. **Shells:**
  T/G checkbox present, always disabled.

- [ ] **V3-21 · Storage-aware admission and low-space guard** (L) — before
  starting a queued download, check free space on its destination (daemon
  `d.free_diskspace`, local `statvfs`, or the web server's volume stats from
  WC8); surface a *blocked: no space* state; global low-space notification with
  a threshold. Never count sparse/preallocated size as free. Carries **QUE-06**.
  **Shells:** T free-space in the status bar only.

### Migrate in

- [x] **V3-22 · Import from qBittorrent and Transmission; session export/import**
  (XL + L) — the single feature that turns "interested" into "switched".
  Discovery reads the other client's resume data into a read-only plan (hashes,
  `.torrent` sources, save paths, labels/categories/tags, priorities); the user
  maps paths, resolves duplicates, then torrents are added *stopped*, rechecked,
  and resumed. Export a portable manifest of the same shape for backup or moving
  to another daemon. Never embed credentials. Uses the operation journal.
  Carries **LIB-10**, **LIB-09**, **B21**, **C23**. **Shells:** T ✓ · G ✓ · W ✓
  (discovery is desktop/GPUI + co-located server; web uploads a manifest).
  **Done (2026-09-17):** shared `rtorrent_core::session` manifest
  (`rstorrent-session/1`: hashes, embedded-magnet-or-path sources with
  info-hash verification, trackers, labels/tags, paths, priorities, limits,
  provenance — never credentials) with pure validate/plan/remap, journaled
  `run_import` (add stopped → metadata → recheck → resume verified, crash-safe
  `import-journal.json`, shared preview DTOs); `rtorrent_core::foreign`
  read-only scanners with a hand-rolled bencode reader — qBittorrent
  `BT_backup` (`qBt-name/savePath/category/tags`, sibling `.torrent`
  embedded after hash verification, v2-only stems honestly skipped) and
  Transmission `resume/`+`torrents/` (kebab-case keys, priority mapping,
  queue position deliberately unread — Transmission does not persist it),
  both verified against upstream source and covered by fixture tests with
  real `.torrent` bytes. All three hosts run the same flow: Tauri commands
  (path- *or* text-based) + Session dialog (export, manifest/scan import,
  preview with selection + remap, polled journaled run) in the TopBar;
  server `export_session_text`/`validate_session`/`import_session`/
  `import_status`/`cancel_import`/`scan_foreign` cmd arms over the existing
  `/api/cmd` route; GPUI services + Session dialog + File-menu entry with a
  shared import state the dialog rejoins after reopen.

### Automate

- [x] **V3-23 · RSS rules 2.0** (XL) — qB's RSS downloader has regex, episode
  filters, smart-episode dedupe, per-rule category/save-path/start policy and
  a "matching articles" preview. rstorrent has must/exclude word lists. Add
  regex, season/episode ranges, min/max size, quality preference, target
  label/tag/path, per-rule interval, test-against-feed with a match explanation,
  exportable seen-set. Carries **AUT-02**. **Shells:** T ✓ · G ✓ (engine +
  prefs; preview test inline, seen copy/clear).
  **Done (2026-09-16):** shared `rtorrent_core::rss` — rule/feed/item types
  (wire-compatible with B11 files), tolerant RSS/Atom parsing with enclosure
  sizes, clause-by-clause explanations, SxxExx/NxNN episode parsing, span
  ranges, quality ranking with preference boost, smart-episode best-per-key
  picks, per-rule intervals, warn-once bad-regex log drain, and capped
  seen persistence reading both file shapes. Tauri engine rebuilt on the
  driver (tags/start/top at add, preview-test + export/import commands);
  GPUI gained its first RSS runner (slow loop, seen beside settings) plus an
  inline rule-tester and seen row in prefs. Both rule editors cover every
  filter; the React preview explains each miss. Exit gate: 18 core tests,
  e2e adoption through `run_tick`, a11y tour green.

- [ ] **V3-24 · Watch-folder rules 2.0** (L) — recursive, filename filters,
  stability delay so a half-copied `.torrent` is never consumed, post-processing
  (delete / rename `.loaded` / move), duplicate handling, an error inbox.
  Carries **AUT-03**. **Shells:** T ✓ basic (C12).

- [ ] **V3-25 · Completion integrations** (L) — allowlisted HTTP webhook and
  rich desktop notifications alongside the existing run-on-complete hook (C13),
  with secret redaction, retry cap, signing, delivery history; the direct-exec
  hook moves behind an expert toggle with a trust warning. Carries **AUT-04**.
  **Shells:** T ✓ exec hook only.

### Diagnose

- [ ] **V3-26 · Port and reachability diagnostics** (L) — listening
  address/port, incoming-peer evidence, tracker reachability, DHT status
  (`dht.statistics`), bind-interface state, likely causes. External port test
  opt-in with the contacted endpoint disclosed. qB shows a connection-status
  icon; ruT has none built in — both communities ask constantly. Carries
  **NET-02**. **Shells:** none.

- [ ] **V3-27 · Peer country and client details** (M) — country/flag from an
  optional local offline GeoIP database (never a network lookup), client name,
  transport, choke/interest state, per-peer totals. qB flags, ruT geoip plugin.
  Carries **NET-05**. **Shells:** T ✓ flags/actions without geo.

- [ ] **V3-28 · IP blocklist manager** (L) — import P2P-format files,
  normalise/merge, preview count/date, apply atomically via
  `ip_tables.insert_table` + `add_address` + `network.block.ipv4.set`, refresh
  from an explicit HTTPS URL with size/time limits. Carries **NET-01**, **D10**.
  **Shells:** none.

- [ ] **V3-29 · Log tab upgrade and notification center** (M + M) — severity
  filter, search, copy, pause-follow, timestamps, torrent scoping, bounded
  buffer (**OBS-04**, C25); and an in-app history of completion/error/
  connection/automation/low-space notifications with per-category controls,
  quiet hours and jump-to-torrent (**UX-07**). **Shells:** T basic log.

### Look and feel

- [ ] **V3-30 · Light and system themes on the desktop** (L) — the web console
  ships five themes with an accent picker (WC1); the Tauri shell inherits them
  via the console restyle; the GPUI shell has a single dark palette. Carries
  **UX-04**, **B22**. **Shells:** W ✓ · T via restyle · G ✗.

- [ ] **V3-31 · Density, zoom, and keyboard completeness** (M + M) —
  compact/comfortable rows (web has it), text zoom to 200%, narrow-window
  column priority (**UX-06**); keyboard reach for the row context menus and the
  manual screen-reader pass left over from REL-01 (**E13-S4**, WC10-S3).
  **Shells:** mixed.

---

## Tier 2 · Power-user and seedbox depth — reached for monthly

ruTorrent plugin parity and the things qB power users script around. Real
value, smaller audience, or needs a Tier 1 foundation first.

- [ ] **V3-32 · Auto-unpack on completion** (L) — ruT's *unpack* plugin is the
  most-installed seedbox plugin. Extract rar/zip/7z (system `unar`/`7z`, never a
  bundled extractor) into the torrent folder or a configured path, by
  label/tag rule, with a size cap, a dry-run listing, cleanup policy, and a
  visible result in the activity log. Local or web-host only; a remote daemon
  gets "runs where the files are". Uses the journal. **New.** **Shells:** none.

- [ ] **V3-33 · Rules engine with simulator** (XL + M) — event (added,
  metadata-ready, started, completed, ratio reached, error, idle, low disk) ×
  condition (tracker, private, size, label/tag, source, time, path) × allowlisted
  action (label, tag, move, limit, stop, remove, notify, webhook, unpack).
  Previewable, dry-run against the current library, execution log. Absorbs seed
  goals, per-label paths and completion hooks as *built-in rules* without
  changing their behaviour. Carries **AUT-01**, **AUT-05**. **Shells:** none.

- [ ] **V3-34 · "Why isn't this downloading?" inspector** (XL) — ranks
  evidence (tracker failures, peer count, availability, file priorities, queue
  policy, limits, disk space, bind interface, recent events) into actionable
  explanations with a safe suggested action each. Builds on the D19 error
  taxonomy and the C8 availability bar. Carries **OBS-01**. **Shells:** none.

- [ ] **V3-35 · Persistent history and activity log** (L + L) — bounded local
  transfer history across restarts (totals by torrent/label/day, export, clear)
  (**OBS-02**); a unified audit log of user actions, automation, daemon
  transitions and file operations, with a redacted support bundle (**OBS-03**,
  **OBS-05**). ruT *history* plugin; qB execution log. **Shells:** T session-only
  history graph (C17).

- [ ] **V3-36 · Multi-user web auth** (XL) — ruTorrent supports per-user setups;
  the web UI is single-password. Named users with roles (admin / operator /
  viewer), per-user sessions and revocation, an audit trail. Prerequisite for
  the collaborative-inbox and approval-link labs; also what makes a shared
  household seedbox safe. **New**, promoted from the LAB-08 prerequisite.
  **Shells:** W.

- [ ] **V3-37 · Multi-daemon dashboard** (XL) — connect several profiles at
  once, aggregate status, per-daemon filters, explicit action target, strict
  namespacing of hashes/tags/history/credentials by daemon identity. Carries
  **PWR-04**; cross-daemon handoff (**PWR-05**) follows. **Shells:** T profiles
  one-at-a-time.

- [ ] **V3-38 · Daemon configuration editor and diff** (XL) — the web console's
  WC7-S7 shows `.rtorrent.rc`; extend to a diff of desired vs last-applied vs
  live values with managed blocks (the 1 Gbps tuner already writes one),
  restart-required flags, and never touching unrelated lines. Carries
  **PWR-03**. **Shells:** T 1 Gbps block only · W read (WC7).

- [ ] **V3-39 · Start and supervise the local daemon** (L) — Daemon → Start
  exists in both desktop shells (bundled 0.15.7, tmux session, starter rc).
  Add stop/restart, log tailing, crash detection with restart offer, and
  discovery of an existing service so a hand-managed rc is never overwritten.
  Carries **PLT-02**, **C20**. **Shells:** T/G start only.

- [ ] **V3-40 · VPN binding guard and proxy test** (L + M) — resolve the bound
  interface, show its addresses, warn when it disappears, optionally stop
  active torrents until it returns (**NET-03**); test the tracker HTTP proxy
  and state plainly that UDP/DHT/peers bypass it (**NET-06**). **Shells:** T
  bind fields only (D9).

- [ ] **V3-41 · Raw XML-RPC console** (L) — hidden expert tool with method
  autocomplete from the handshake, typed args, history; read-only unless armed
  per-session; `execute.*` and `method.insert` always blocked. Carries
  **OBS-06**, **D15**. **Shells:** none.

- [ ] **V3-42 · Torrent v2/hybrid visibility** (M) — detect v2/hybrid in
  parsing and creation, show both hashes, and explain that rtorrent will load
  only the v1 half of a hybrid and cannot load a v2-only torrent. Carries
  **PWR-08**. **Shells:** none.

- [ ] **V3-43 · Command palette and customisable shortcuts** (L + M) —
  searchable actions/torrents/views/profiles/settings (**UX-02**); rebinding
  with conflict detection and platform conventions (**UX-03**). **Shells:**
  none.

- [ ] **V3-44 · Localization foundation** (XL) — string extraction, plural and
  number/date/size formatting, pseudo-locale, layout stress; no translations
  accepted before the foundation. Carries **UX-05**, **B20**. **Shells:** none.

- [ ] **V3-45 · Choke-group profiles** (XL) — guided presets over the 54
  `choke_group.*` methods with live validation and rollback; raw scheduling
  syntax never the default UX. Carries **PWR-02**, **D14**. **Shells:** none.

- [ ] **V3-46 · Headless CLI over the shared core** (XL) — `rstorrent-cli
  list/add/start/stop/tag/export` with JSON output, the same profiles,
  credential rules and capability gating. Carries **PWR-09**. **Shells:** n/a.

- [ ] **V3-47 · Session snapshots and tracker policy sets** (L + L) — named,
  encrypted-at-rest backups of client config + selected session metadata with
  diff and dry-run restore (**PWR-06**); reusable tracker tiers for new
  torrents, allow/deny rules, batch remediation (**PWR-07**). **Shells:** none.

- [ ] **V3-48 · Power and network transitions** (L) — suspend/resume,
  interface change, VPN reconnect, clock change, daemon address change: recover
  polling without duplicate work. Carries **PLT-04**. **Shells:** partial.

---

## Tier 3 · Exotic — differentiators that must prove themselves

Behind a Labs toggle or build flag; no new sensitive data without consent; each
has a graduate criterion and a removal plan. Prototype at most two at once.
Descriptions live in backlog-v2.md §8; only the ordering changed.

1. **LAB-07 · Privacy posture dashboard** (L) — cheapest, and turns V3-26/V3-40
   evidence into one honest page. No single "anonymous" score.
2. **LAB-01 · Swarm health map** (XL) — time-aware availability heatmap on top
   of C8. Graduate if it finds a dead swarm faster than tracker/peer counts.
3. **LAB-11 · Seed sustainability score** (L) — ranks seeding attention with a
   visible formula; never auto-deletes on the score.
4. **LAB-04 · Policy packs** (L) — shareable bundles of labels, limits, paths,
   RSS/watch rules; import shows a full diff and strips secrets.
5. **LAB-02 · Explainable torrent doctor** (XL) — stepwise repair plans from
   V3-34 evidence; any language model is optional and summarises redacted
   evidence only.
6. **LAB-05 · Content-aware library graph** (XL) — local-only duplicate and
   reseed detection from filenames, sizes and directory shape.
7. **LAB-06 · Storage placement advisor** (XL) — recommends destination volumes;
   automates only through the journal.
8. **LAB-08 · Collaborative inbox** (XL) and **LAB-12 · Approval links** (XL) —
   both need V3-36 first.
9. **LAB-03 · Adaptive bandwidth controller** (XL) — hard user caps always win;
   every automatic change visible and reversible.
10. **LAB-09 · Time-travel operations view** (XL) — needs V3-35 retention and
    redaction first.
11. **LAB-10 · Reproducible torrent recipe** (L) — validate demand first.
12. **PWR-10 · Extension API discovery** (M) — ship nothing without a threat
    model, versioning and revocation.

---

## Deliberate non-goals

Unchanged from v2, restated because two of them are visible gaps against qB:

- **Built-in torrent search / indexer plugins** (qB's search tab, ruT's
  *search* plugin) — rstorrent is a client, not an indexer. Document this on the
  README's comparison table so nobody discovers it after switching.
- **Embedding a BitTorrent engine** — the daemon is rtorrent; features rtorrent
  cannot do (sequential on rakshasa builds, per-file rename, v2 transfer) are
  explained, not emulated.
- **Arbitrary shell / `execute.*` / `method.insert` from the UI**, exposing raw
  SCGI to a network, or treating HTTP Basic Auth without TLS as safe.

---

## Suggested release sequence

Scope exits on quality gates, not dates. Numbers are illustrative.

| Release | Theme | Contents | Exit gate |
|---|---|---|---|
| **2.1** | One shell, installable | V3-01, V3-02, V3-03, V3-09 | Signed build, first-run wizard reaches a live daemon, QA checklist green on the chosen shell |
| **2.2** | Seedbox web | V3-04, V3-05, V3-06 (web half), V3-07 | Playwright green in CI, hardened deployment documented, 5k-torrent web budget met |
| **2.3** | Organise and finish | V3-10, V3-13, V3-14, V3-16, V3-17, V3-21 | Move-on-complete survives a kill mid-copy; queue policy deterministic |
| **2.4** | Switch to us | V3-22, V3-15, V3-19, V3-23, V3-24, V3-25 | Import a real qB library, recheck, and seed without hand edits |
| **2.5** | Diagnose | V3-26, V3-27, V3-28, V3-29, V3-20, V3-11, V3-12 | Port test + peer geo + blocklist live; "why stalled" answerable from the UI |
| **3.0** | Everywhere | V3-08 (Windows + Linux), V3-30, V3-31, V3-06 (GPUI half), V3-48 | Signed macOS/Windows and a Linux package, each clean-machine tested |
| **3.x** | Depth | Tier 2 in listed order, starting with V3-32, V3-33, V3-34, V3-35, V3-36 | Rules previewable and audited; multi-user web isolation tested |
| Labs | — | Tier 3, two at a time | Each has a measured graduate criterion or is removed |

---

## Dependency map

```text
V3-01 shell decision ─────────────┬─ every desktop item below lands in one shell
V3-07 capability handshake ───────┼─ V3-20 sequential · V3-42 v2 · V3-45 choke · V3-41 console
                                  └─ honest "unavailable because…" everywhere
operation journal (FND-07, move slice) ─┬─ V3-14 move-on-complete / incomplete dir
                                        ├─ V3-15 root-folder rename
                                        ├─ V3-22 import / export
                                        └─ V3-32 unpack · LAB-06 advisor
V3-04 + V3-05 web console ───── V3-36 multi-user ──┬─ LAB-08 inbox
                                                   └─ LAB-12 approval links
V3-35 activity log ────────────┬─ V3-33 rules engine + simulator
                               ├─ V3-34 inspector · LAB-02 doctor
                               └─ LAB-09 time-travel
V3-26 + V3-40 diagnostics ───── LAB-07 privacy posture
```

## Backlog hygiene

- `backlog.md` records shipped methods; tick the carried `B`/`C`/`D` item there
  when a V3 item lands, and link the implementation story.
- `tasks.md` (desktop/shared), [GPUI.md](GPUI.md) (the port) and
  [docs/web-console-tasks.md](docs/web-console-tasks.md) (web) own stories;
  this file owns priority.
- Every item with a **Shells** line is done only when it is done in the
  *shipped* shells, not the reference one.
- Any feature that fetches remotely, mutates the filesystem, handles
  credentials, adds users, runs hooks, or extracts archives needs a threat-model
  note before implementation.
- Re-probe the method list when the bundled or supported rtorrent version
  changes; the "daemon facts" section above is dated for that reason.
