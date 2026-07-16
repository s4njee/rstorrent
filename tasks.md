# rstorrent — Epics & Stories

Execution backlog for the plan in [plan.md](plan.md). Design authority: `design/README.md` + `design/rTorrent Client 1c.dc.html`.

## Conventions

- **Order:** epics are numbered in dependency order and map to milestones M0–M6 (plan.md §9). Don't start an epic before its `Deps` are satisfied unless the story says otherwise.
- **Story format:** `[ ]` checkbox · ID · title · what/why · **AC** (acceptance criteria) · **Verify** (a concrete check to run).
- **Definition of done (every story):** code compiles with zero warnings from `cargo clippy -- -D warnings` and `tsc --noEmit`; new pure logic has unit tests; UI matches the 1c design tokens (no hard-coded colors — use `tokens.css` variables); works in **mock mode** (`RSTORRENT_MOCK=1`).
- **Run commands** (established in E0): `npm run tauri dev` (app), `RSTORRENT_MOCK=1 npm run tauri dev` (mock mode), `cargo test --manifest-path src-tauri/Cargo.toml`, `npm test`.
- Sizes: S (≤2 h), M (≤half day), L (day). Split anything growing past L.

## Progress (as of feature build)

Milestones **M0–M5 complete.** `RSTORRENT_MOCK=1 npm run tauri dev` launches a
full, interactive client:

- Main window: **native macOS menubar**, overlay title bar, toolbar, filter
  sidebar, sortable/selectable torrent table, status bar, right-click context
  menu, keyboard shortcuts.
- **All six detail tabs**: General, Trackers, Peers, Content (click a file's
  priority to cycle off/normal/high), Speed (live SVG rate chart), Log.
- **Dialogs:** Add-torrent (tri-state file tree), Add-magnet, Remove, Statistics,
  and **Preferences** (⌘,) — Behavior / Downloads (incl. watched-folder) /
  Connection (Test-connection) / Speed / BitTorrent (port range + DHT) / Advanced.
- **Watched folder** auto-adds `.torrent` files; **since-install** transfer
  counters persist across restarts.

Verification: Rust **29 tests** + `clippy -D warnings` clean + full binary builds
and launches cleanly; frontend **22 tests** + `tsc --noEmit` + ESLint + `vite build`
clean.

**Live-daemon verified** against Homebrew **rtorrent 0.16.17** (ignored integration
tests in `src-tauri/src/rtorrent/client.rs`, run with `RSTORRENT_TEST_SOCKET`):
version, globals, `d.multicall2` list, `load_raw` add, start/stop, erase, `statistics`,
and `set_port_range` (set + read-back) all round-trip; our `.torrent` parser's
info-hash matches rtorrent's; and the **watched folder** was verified end-to-end
(dropped file auto-loaded into the live daemon and renamed `*.loaded`). The GUI app
connects to the live daemon and polls cleanly. (Note: rtorrent 0.16 crashes on a
malformed all-zero-hash magnet — an upstream bug, documented in
`docs/rtorrent-setup.md`; use real magnets.)

**Packaged:** `npm run tauri build` produces a branded `rstorrent.app` (5.4 MB)
and `rstorrent_0.1.0_aarch64.dmg`; the release `.app` launches cleanly in mock
mode. Build/signing steps are in `docs/release.md`.

Still open (M6): `E13` polish — perf/virtualization (`E13-S2`), fuller
disconnected/first-run card (`E13-S1`), accessibility (`E13-S4`), UI-state
persistence beyond the current sort/filter/tab (`E13-S3`), and *running* the
`docs/qa-checklist.md` on a clean account (`E13-S5`); and `E14-S2` (clean-account
release QA + Developer-ID signing, when a cert exists). `E12`'s cache-overload
stat has no rtorrent source (stays "—"). Still to verify live against an *actively
downloading* torrent: per-file priorities, Speed chart, peers/trackers population.

---

## E0 — Project scaffold & tooling  *(M0)*

Deps: none.

- [x] **E0-S1 · Scaffold Tauri 2 + React-TS app** (M)
  `git init`; scaffold with `npm create tauri-app@latest` (react-ts / Vite template) into the repo root; identifier `com.rstorrent.app`; window 1180×760 min 900×600; app name `rstorrent`. Pin Node ≥ 20, Rust stable. Add `.gitignore` (node_modules, target, dist).
  **AC:** `npm run tauri dev` opens a window on macOS; repo has one commit with scaffold.
  **Verify:** fresh clone → `npm ci && npm run tauri dev` works.

- [x] **E0-S2 · Design tokens & global styles** (M)
  Create `src/theme/tokens.css` defining every token from `design/README.md` §Design Tokens as CSS custom properties (`--bg-app: #14161a` … `--danger-border`), plus font stack, base sizes (11.5/10.5/10 px), and a minimal reset (margin 0, `user-select:none` on chrome, `-webkit-font-smoothing`). Global background `--bg-app`.
  **AC:** every hex from the README table exists exactly once, in this file; App renders dark with monospace text.
  **Verify:** grep `#14161a` appears only in tokens.css.

- [x] **E0-S3 · Overlay title bar & window chrome** (S)
  `tauri.conf.json`: `titleBarStyle: "Overlay"`, `hiddenTitle: true`. Render a 34 px `bg/panel` header strip with `data-tauri-drag-region` and centered `text/dim` title placeholder `rtorrent — connecting…`. Native traffic lights must sit on the strip.
  **AC:** window drags by the strip; traffic lights functional; no fake traffic lights drawn.
  **Verify:** run app, drag/minimize/close via lights.

- [x] **E0-S4 · Lint/format/test harness & scripts** (S)
  `rustfmt` + clippy (deny warnings) config; ESLint + Prettier; Vitest configured; `cargo test` wired. npm scripts: `dev`, `tauri dev`, `test`, `lint`, `typecheck`. Optional: GitHub Actions running lint+tests.
  **AC:** all scripts exit 0 on the scaffold.
  **Verify:** run each script.

- [x] **E0-S5 · IPC contract module & shared types** (M)
  `src/ipc/types.ts` with `TorrentDto`, `GlobalStats`, `ConnState`, `Status` enum, command arg/result types (plan.md §4.3); matching Rust structs (serde, camelCase rename) in `src-tauri/src/` with a doc comment pointing at the TS file. Stub `commands.ts`/`events.ts` helpers typed over `invoke`/`listen`.
  **AC:** field names/casing identical across TS and Rust; compiles both sides.
  **Verify:** `tsc --noEmit` + `cargo check`.

---

## E1 — rtorrent client core (Rust)  *(M1)*

Deps: E0-S1. Stories S1–S3 are pure Rust, parallel-safe with all frontend work.

- [x] **E1-S1 · XML-RPC encoder/parser** (L)
  `rtorrent/xmlrpc.rs` with a `Value` enum (`Int(i64)`, `Str`, `Bytes`, `Array`, `Struct`) using `quick-xml`. Encode `methodCall` (params incl. base64); parse `methodResponse` handling `int`/`i4`/**`i8`**, `string` (and bare text nodes), `array`, `struct`, `base64`, and `fault` → typed `Fault{code,string}` error.
  **AC:** round-trips all value kinds; parses a captured real `d.multicall2` response fixture; fault becomes `Err`.
  **Verify:** `cargo test xmlrpc` (include an `<i8>` case).

- [x] **E1-S2 · SCGI transport** (M)
  `rtorrent/scgi.rs`: async request/response over unix socket **and** TCP (tokio), netstring framing with `CONTENT_LENGTH` first + `SCGI 1`, CGI response header parse, connect/read timeouts, typed errors (`Unreachable`, `Timeout`, `Protocol`). One request per connection.
  **AC:** works against the mock server (E1-S4); malformed responses error cleanly, never panic.
  **Verify:** `cargo test scgi`.

- [x] **E1-S3 · Typed client + RtorrentApi trait** (M)
  `rtorrent/mod.rs` trait covering: `list_snapshot()`, `global_stats()`, `trackers(hash)`, `peers(hash)`, `files(hash)`, `start/stop/recheck/erase(hashes)`, `load_raw(bytes, opts)`, `load_magnet(uri, opts)`, `set_custom1`, `set_directory`, `set_priority`, `set_file_priority`, `throttle_get/set`, `dht_stats`, `client_version()`. `client.rs` implements it over S1+S2, batching mutations via `system.multicall`.
  **AC:** every plan.md §5.6 action has a trait method; hex info-hashes normalized uppercase.
  **Verify:** `cargo test client` against mock server.

- [x] **E1-S4 · Mock rtorrent (client + SCGI test server)** (L)
  `rtorrent/mock.rs`: `MockClient` implementing the trait with the **10 design-fixture torrents** (exact names/sizes/states/speeds from `design/rTorrent Client 1c.dc.html`'s data block), ticking simulated progress each poll and honoring actions (stop actually pauses, erase removes, labels stick). Plus an in-process SCGI+XML-RPC test server serving the same fixtures for transport tests. Activated by `RSTORRENT_MOCK=1` or settings flag.
  **AC:** app in mock mode shows all 10 torrents matching the design screenshot states (3 downloading, 4 seeding, 2 paused, 1 stalled, 1 error, 5 complete); Fedora row progresses over time.
  **Verify:** `RSTORRENT_MOCK=1 npm run tauri dev` once E2/E4 land; `cargo test mock` now.

- [x] **E1-S5 · Status/ETA derivation** (M)
  `rtorrent/derive.rs`: pure fns mapping raw fields → `Status` (rules in plan.md §5.4 incl. stall window), ETA seconds, ratio. Table-driven tests covering **every row of the design fixture** (expected: seeding/downloading/paused/stalled/error) plus checking + edge cases (0-rate complete, message set).
  **AC:** all fixture rows derive to the design's shown status.
  **Verify:** `cargo test derive`.

- [x] **E1-S6 · Connection settings + docs** (S)
  Settings model: `{ transport: UnixSocket{path} | Tcp{host,port}, pollMs, stallWindowS }`, defaults `~/.rtorrent/rpc.socket`, 1000 ms. Write `docs/rtorrent-setup.md`: brew install, minimal `.rtorrent.rc` with `scgi_local`, how to point the app at it.
  **AC:** settings persist via `tauri-plugin-store`; doc snippet verified against a real local rtorrent.
  **Verify:** follow the doc from scratch; `test_connection` command returns the daemon version.

---

## E2 — Sync engine & IPC  *(M1)*

Deps: E1-S3/S4/S5, E0-S5.

- [x] **E2-S1 · Poller task + snapshot event** (L)
  `poller.rs`: tokio loop (interval from settings) calling `list_snapshot` + `global_stats`, assembling `{torrents, globals, connection}` and emitting `state://snapshot`. Reconnect with backoff (1→2→5→10 s cap) on failure, emitting `ConnState` transitions. Actions trigger an immediate extra poll.
  **AC:** kill/restart rtorrent → app shows disconnected then recovers without restart; no unbounded task spawn.
  **Verify:** run live, `pkill rtorrent`, restart, watch events (add temporary logging).

- [x] **E2-S2 · Slow poll: tracker-domain cache** (M)
  Every 60 s (and on first sight of a hash): `t.multicall` per new torrent → first announce URL's host; cache by hash; include as `trackerHost` in snapshots. Batch politely (≤5 concurrent).
  **AC:** Tracker column + sidebar Trackers group populate within one slow-poll cycle; no per-fast-poll tracker calls.
  **Verify:** live daemon with 2+ trackers; watch RPC call log.

- [x] **E2-S3 · Detail watch channel** (M)
  `set_detail_watch(hash, tab)` command starts/steers a 2 s loop fetching only the active tab's data (`trackers`/`peers`/`files`), emitting `state://detail`. Stops when hash/tab is null or window hidden.
  **AC:** RPC log shows detail calls only while a detail tab is open for a selected torrent.
  **Verify:** toggle tabs with RPC logging on.

- [x] **E2-S4 · Torrents store + reconciliation (frontend)** (M)
  `store/torrents.ts` subscribes to `state://snapshot`; reconcile by hash preserving object identity for unchanged rows; selectors: visible list (filter+search+sort), sidebar counts, global stats. `store/ui.ts` skeleton (selection, filter, sort, tab, dialog).
  **AC:** Vitest: reconciliation keeps references stable for unchanged rows; counts match fixture expectations (all 10 / downloading 3 / completed 5 …).
  **Verify:** `npm test`.

- [x] **E2-S5 · App log ring buffer** (S)
  `log.rs`: bounded (1000) ring buffer; entries for connection changes, action results, RPC faults, `d.message` transitions (tagged with hash); `log://append` event + `get_log` command for hydration.
  **AC:** pausing a torrent produces a log entry; buffer never exceeds cap.
  **Verify:** `cargo test log` + observe in Log tab later (E10-S6).

---

## E3 — App shell  *(M1)*

Deps: E0. Pure UI — parallel-safe with E1/E2 using placeholder data; wire to stores as they land.

- [x] **E3-S1 · Layout frame** (S)
  App grid: title bar / toolbar / body (sidebar + table) / detail panel / status bar, per design §1. Body height flexes; detail panel fixed-height (tab strip + content) initially.
  **AC:** regions match the 1c main-window proportions at 1180×760; no scrollbars on the frame itself.
  **Verify:** side-by-side with design HTML.

- [x] **E3-S2 · Title bar live text** (S)
  Center text `rtorrent {version} · {n} torrents` from connection state + snapshot (`text/dim`, 11 px, weight 600); `rtorrent — disconnected` when down.
  **AC:** updates live; correct in mock mode (`rtorrent 0.9.8 · 10 torrents`).
  **Verify:** mock mode matches design string.

- [x] **E3-S3 · Icon set** (M)
  `components/icons/`: inline-SVG components for every glyph — toolbar (add `+` cyan, magnet, remove `−`, play, pause, up, down), menu icons (recheck ↻, label, folder, link, open, remove ✕), prefs nav (8), close ✕, submenu ▸. 1.6 px stroke, currentColor, 12–13 px viewBox per design; **no emoji**.
  **AC:** visual match to design toolbar; all icons render crisp at 1x/2x.
  **Verify:** icon gallery route or Storybook-less test page in dev.

- [x] **E3-S4 · Toolbar UI** (M)
  Buttons per design order with separators: add-file, add-magnet, remove | resume, pause | move-up, move-down; right-aligned filter input (190 px, `bg/field`, placeholder `/ filter`). Hover state `bg/track`; disabled at 40% opacity (all disabled until selection/actions exist). Tooltips (title attr) incl. "priority" note on queue buttons.
  **AC:** pixel-matches design toolbar; buttons fire store callbacks (no-ops for now).
  **Verify:** side-by-side compare.

- [x] **E3-S5 · Status bar** (S)
  `dht: N nodes` left · `↓ rate` cyan-bright / `↑ rate` green-soft / `free: X GiB` right, from globals; hide free-space segment when unavailable. 10.5 px `text/dim` on `bg/panel`.
  **AC:** live values in mock mode; formats match design (`↓ 9.5 MiB/s`).
  **Verify:** mock mode.

- [x] **E3-S6 · Modal primitive** (M)
  `ModalBase`: backdrop dim over the whole window, centers a `win`-styled panel (border `#000`, radius 9, shadow), header with title + ✕, footer slot; Esc = cancel, Enter = primary, focus trap, click-✕ = cancel. Used by every dialog epic.
  **AC:** keyboard behavior per design §Interactions; background inert while open.
  **Verify:** demo modal in dev; tab-cycle stays inside.

- [x] **E3-S7 · Native app menu + shortcuts registry** (M)
  macOS menu: App (About, Preferences… ⌘,, Quit ⌘Q), Edit (standard clipboard for inputs), a small central keyboard-shortcut handler (⌘O add-file, ⌘⇧O magnet, ⌘F focus search, Space toggle, ⌫ remove, ⌘A select-all) dispatching to ui store actions.
  **AC:** menu items + shortcuts fire the same actions as buttons; no shortcut fires while typing in an input (except ⌘-combos).
  **Verify:** manual pass over each shortcut.

---

## E4 — Torrent table  *(M1)*

Deps: E2-S4, E3-S1.

- [x] **E4-S1 · Grid table + columns + zebra** (L)
  12-column CSS grid exactly per design (`minmax(200px,1fr) 70 92 84 52 52 76 76 62 46 72 110`): Name, Size, Done, Status, S, P, Down, Up, ETA, Ratio, Label, Tracker. Header 10 px uppercase `text/dim`; rows 23 px zebra with `border/row` dividers; Name `text/primary` ellipsized; numerics right-aligned 10.5 px `text/muted`; Down cyan-bright, Up green-soft; Status lowercase colored text; scroll container is the table body only.
  **AC:** mock mode is visually indistinguishable from design screen 01 (states, colors, alignment).
  **Verify:** side-by-side compare, all 10 fixture rows.

- [x] **E4-S2 · Progress bar cell** (S)
  8 px bar, track `bg/track`, radius 1, fill % = percent, fill color by status (dl cyan, seeding green, paused `#4a515c`, stalled amber, error red).
  **AC:** fixture rows match design fills/colors; animates smoothly as mock progresses.
  **Verify:** watch Fedora row tick in mock mode.

- [x] **E4-S3 · Formatters** (M)
  `utils/format.ts`: bytes, rate, ETA, ratio per plan.md §6.3, table-driven Vitest against **every design sample string** (`5.8 GiB`, `631 MiB`, `8.4 MiB/s`, `4m12s`, `13m40s`, `∞`, `—`, `2.41`).
  **AC:** all design samples reproduce exactly.
  **Verify:** `npm test format`.

- [x] **E4-S4 · Sorting** (M)
  Header click cycles asc/desc (indicator arrow, `text/muted`); numeric columns sort on raw values; stable sort; default = daemon order; persisted to localStorage.
  **AC:** sorting by Size orders `631 MiB` < `1.1 GiB`; survives relaunch.
  **Verify:** Vitest on sort comparators + manual.

- [x] **E4-S5 · Selection model** (M)
  Click select (row `bg/selected`), ⌘-click toggle, ⇧-click range from anchor, ⌘A all (visible), Esc clears, arrows move selection (⇧-arrows extend). Selection stored as hash set; survives snapshot updates; auto-prunes removed hashes.
  **AC:** matches design interaction spec; selection drives detail panel + toolbar enablement.
  **Verify:** Vitest for range logic + manual pass.

- [x] **E4-S6 · Empty & filtered-empty states** (S)
  No torrents: centered dim hint "no torrents — ⌘O to add a .torrent, ⌘⇧O for magnet". Filter/search with no matches: "no torrents match" + inline "clear filter" link.
  **AC:** both states reachable and styled in tokens.
  **Verify:** mock mode with search gibberish; empty mock flag.

---

## E5 — Filter sidebar & search  *(M1)*

Deps: E2-S4, E3-S1.

- [x] **E5-S1 · Sidebar groups & counts** (M)
  150 px fixed sidebar: uppercase group headers (Status / Labels / Trackers); Status rows all·downloading·seeding·completed·paused·stalled·error with right-aligned dim counts; Labels from distinct `d.custom1` values; Trackers from distinct `trackerHost` (ellipsized). Counts live from store selectors (overlapping predicates per plan.md §5.4).
  **AC:** mock mode shows exactly the design counts (all 10, downloading 3, seeding 4, completed 5, paused 2, stalled 1, error 1; linux-iso 6, video 3, sbc 1).
  **Verify:** compare with design.

- [x] **E5-S2 · Active filter behavior** (S)
  Click row → filters table + moves highlight (`bg/selected` fill, 2 px cyan left border, cyan-bright text); click active row again → back to `all`. One active filter at a time; persisted.
  **AC:** clicking `seeding` shows 4 rows; highlight matches design; counts stay global.
  **Verify:** manual in mock mode.

- [x] **E5-S3 · Search filter wiring** (S)
  Toolbar input filters by name/label/tracker substring (case-insensitive), debounced 150 ms, combined AND with sidebar filter; Esc in field clears; ⌘F focuses.
  **AC:** typing `fedora` shows 1 row; combined with `downloading` filter still correct.
  **Verify:** Vitest on the combined selector + manual.

---

## E6 — Actions, toolbar wiring & context menu  *(M2)*

Deps: E1-S3, E4-S5.

- [x] **E6-S1 · Action command layer** (M)
  Rust commands: `start`, `stop`, `recheck`, `set_label`, `set_location`, `queue_move`, `copy_magnet`, `open_destination` (per plan.md §5.6, batched via `system.multicall`, each followed by an immediate poll). Frontend `ipc/commands.ts` wrappers with per-action in-flight state (disable initiating buttons) and log entries on failure.
  **AC:** each action works against live rtorrent AND mock; failures surface as log entries, not silent.
  **Verify:** live daemon: pause/resume/recheck a real torrent; `cargo test` mock actions.

- [x] **E6-S2 · Toolbar wiring & enablement** (S)
  Resume/pause/remove/up/down enabled only with selection; add buttons always on; actions apply to all selected hashes.
  **AC:** disabled states correct; multi-select pause pauses all selected.
  **Verify:** manual, 3-row selection in mock.

- [x] **E6-S3 · Context menu** (L)
  Right-click row (selects it if not in selection) → menu at cursor per design screen 06: Resume, Pause | Force recheck, Set label ▸ (submenu: existing labels + `New…` inline-prompt + `None`), Set location…, | Copy magnet link, Open destination | Remove (danger styling). `bg/panel`, hover `bg/selected`, separators per spec; closes on click-away/Esc; flips near window edges.
  **AC:** pixel-matches design; every item dispatches E6-S1 actions; label submenu creates/uses labels (visible in sidebar next poll).
  **Verify:** side-by-side + live label set.

- [x] **E6-S4 · Set location flow** (S)
  Menu item → native folder picker (dialog plugin) → `set_location` (stop→set→restart per plan); toast/log warning "files are not moved".
  **AC:** live rtorrent shows new `d.directory`; active torrent restarts; warning appears.
  **Verify:** live daemon check via `d.directory`.

- [x] **E6-S5 · Copy magnet & open destination** (S)
  Copy magnet builds URI from hash/name/cached trackers → clipboard plugin + log "copied". Open destination reveals `base_path` in Finder (localhost only; hidden otherwise).
  **AC:** pasted magnet re-adds the torrent on a second client; Finder opens with the item selected.
  **Verify:** manual live test.

---

## E7 — Remove confirmation  *(M2)*

Deps: E3-S6, E6-S1.

- [x] **E7-S1 · Remove dialog** (M)
  Per design screen 07: warning roundel (danger palette), "Remove **{name}** from the transfer list?" (multi-select: "{n} torrents"), checkbox "Also delete downloaded files ({total size})" in danger style, Cancel + destructive Remove. Enter = Remove, Esc = Cancel. Checkbox disabled + tooltip when daemon isn't localhost.
  **AC:** matches design; size sums selection; unchecked by default.
  **Verify:** side-by-side compare.

- [x] **E7-S2 · Remove execution (trash, never rm)** (M)
  `remove(hashes, deleteData)`: read `d.base_path` per hash → `d.erase` → move paths to macOS **Trash** (`trash` crate); partial failures logged per-torrent; selection pruned.
  **AC:** files land in Trash (recoverable); erase-only leaves data; a locked file failure logs but other torrents still process.
  **Verify:** live daemon with a throwaway torrent; check Trash.

---

## E8 — Add torrent (.torrent file) dialog  *(M3)*

Deps: E3-S6, E1-S3.

- [x] **E8-S1 · .torrent metadata command** (M)
  `read_torrent_metadata(path)` in Rust via `lava_torrent`: name, total size, info-hash, file list (paths+sizes), private flag, trackers. Errors typed (not a torrent / unreadable).
  **AC:** parses single-file and multi-file torrents; returns tree-buildable path list.
  **Verify:** `cargo test` with two fixture .torrent files committed under `src-tauri/tests/fixtures/`.

- [x] **E8-S2 · Add-torrent dialog UI** (L)
  Per design screen 02 (620 px): Torrent row (name + `{size} · {n} files` meta), Save to (input + Browse… via folder picker, default from settings), Label dropdown (existing labels + New…), Rename checkbox (v1 disabled + tooltip), options grid (Start ✓ default, Sequential — disabled/tooltip "not supported by rtorrent", Skip hash check, Add to top of queue), Contents tree (S3), Cancel/Add footer.
  Entry points: toolbar, ⌘O, app menu, and **drag-drop of a .torrent onto the window**.
  **AC:** pixel-matches screen 02; opens pre-populated from metadata.
  **Verify:** side-by-side + drop a file.

- [x] **E8-S3 · File tree with tri-state checkboxes** (L)
  Folder tree from path list; folder checkbox reflects children (checked/unchecked/indeterminate); `select all · none` header links; per-row right-aligned sizes; toggling updates a "selected: X GiB of Y" footer line; children indented per design.
  **AC:** tri-state logic correct (Vitest); deselected files map to indexes for `f.priority.set=0`.
  **Verify:** `npm test tree` + visual.

- [x] **E8-S4 · Add execution** (M)
  On Add: `load.raw`(+`_start` if Start checked) with base64 bytes + inline `d.directory.set`/`d.custom1.set`; after load, zero-priority deselected files; top-of-queue → `d.priority.set=3`; dialog closes; torrent appears next poll; failures keep dialog open with inline error (`accent/red`).
  **AC:** live daemon receives torrent in right dir/label with unselected files skipped; bad daemon state shows inline error.
  **Verify:** live add of a fixture torrent; check `d.directory`, `f.priority` via RPC.

---

## E9 — Add magnet dialog  *(M3)*

Deps: E3-S6, E1-S3.

- [x] **E9-S1 · Magnet dialog UI + parse** (M)
  Per design screen 03 (460 px): textarea (magnet URI or torrent URL, cyan-bright 10.5 px), Save to, Label, Start ✓ / top-of-queue checkboxes, Cancel/Add. Client-side parse of `magnet:` (btih, dn, tr) for validation + display; plain `.torrent` **URLs** accepted and passed through (`load.start` handles URLs). Invalid input disables Add + inline hint. Entry: toolbar, ⌘⇧O, paste-detection (if clipboard holds a magnet when dialog opens, prefill).
  **AC:** matches design; malformed URI blocked with message; valid magnet prefills name from `dn`.
  **Verify:** side-by-side + paste the design's sample magnet.

- [x] **E9-S2 · Magnet add execution** (S)
  `load.start`/`load.normal` with directory/label commands; appears as metadata-resolving torrent next poll (name may be hash until resolved — table must tolerate).
  **AC:** live daemon accepts magnet; row shows and later resolves name.
  **Verify:** live daemon with a well-seeded magnet.

---

## E10 — Detail tabs  *(M4)*

Deps: E2-S3, E4-S5. One story per tab; parallel-safe.

- [x] **E10-S1 · Tab strip + panel shell** (S)
  lowercase tabs general·trackers·peers·content·speed·log; active = cyan-bright + 2 px cyan underline; panel on `bg/app`; empty-selection state ("select a torrent"); active tab persisted; drives `set_detail_watch`.
  **AC:** matches design; watch starts/stops with selection/tab (check RPC log).
  **Verify:** visual + RPC log.

- [x] **E10-S2 · General tab** (S)
  4-column `label: value` grid per design: active, down (d.down.total), up (d.up.total), ratio, eta, conns (`peers_connected`), dl-limit / ul-limit (global throttles, `∞` when 0). Multi-select: aggregates where sensible, `—` otherwise.
  **AC:** values live-update; matches design layout/strings.
  **Verify:** mock-mode compare.

- [x] **E10-S3 · Trackers tab** (M)
  Table: url, status (working/updating/error from usable+counters), seeds/leeches (scrape), last announce (relative). 23 px rows, same table tokens.
  **AC:** live torrent shows real tracker rows updating ~2 s.
  **Verify:** live daemon.

- [x] **E10-S4 · Peers tab** (M)
  Table: address, client, done %, down/up rate, flags (E encrypted, I incoming, S snubbed). Sorted by down rate desc.
  **AC:** live active torrent lists peers with moving rates.
  **Verify:** live daemon on an active download.

- [x] **E10-S5 · Content tab** (L)
  Reuse E8-S3 tree (read-only structure + per-file progress % and priority control off/normal/high via click-cycle or mini-menu) → `set_file_priority`; folder rows aggregate.
  **AC:** priority change survives re-poll (verify via `f.priority`); progress per file accurate.
  **Verify:** live daemon; skip a file mid-download.

- [x] **E10-S6 · Speed tab** (M)
  SVG area chart of selected torrent down/up rates from a frontend ring buffer (~10 min of snapshots), down cyan / up green-soft, dim gridlines + current-rate legend. No chart lib.
  **AC:** chart scrolls as data arrives; empty state before enough samples.
  **Verify:** watch a mock download for 2 min.

- [x] **E10-S7 · Log tab** (S)
  Render app log (E2-S5 + `get_log` hydration): timestamp dim, message body-color, errors `accent/red`; entries tagged with selected hash highlighted; autoscroll with pause-on-hover.
  **AC:** actions/errors appear immediately; buffer capped.
  **Verify:** pause a torrent, see entry.

---

## E11 — Preferences  *(M5)*

Deps: E3-S6, E1-S6, E6-S1.

- [x] **E11-S1 · Prefs window shell + nav** (M)
  Per design screen 04 (860×~500): left nav (170 px) Behavior · Downloads (default) · Connection · Speed · BitTorrent · RSS · Web UI · Advanced, active = `bg/selected` + cyan edge; **RSS + Web UI rendered disabled/dim with "v2" tooltip**; Cancel/Apply footer (Apply = collect dirty fields → `apply_settings`, stays open on partial failure with per-field error).
  **AC:** matches design; nav switches panels; Apply/Cancel semantics work.
  **Verify:** side-by-side.

- [x] **E11-S2 · Downloads section** (M)
  Default save path (input+Browse, app setting used by add dialogs), "Keep incomplete in" **disabled** + tooltip (plan.md §10), "Do not start automatically", "Show add-torrent dialog" (skip-dialog = instant add with defaults), content layout dropdown (Original only, disabled), Watched folder: enable + path (S5).
  **AC:** toggles affect add-flow behavior (verify skip-dialog path).
  **Verify:** manual add with dialog disabled.

- [x] **E11-S3 · Connection section** (M)
  Transport picker (unix socket path / TCP host+port), poll interval, stall window, **Test connection** button showing daemon version or typed error; non-localhost TCP shows the unauthenticated-SCGI warning.
  **AC:** switching transports live-reconnects the poller; test button accurate.
  **Verify:** point at bad port → clean error; back to good → recovers.

- [x] **E11-S4 · Speed / BitTorrent / Behavior / Advanced sections** (M)
  Speed: global down/up limits (KiB/s, 0 = ∞) → `throttle.global_*.max_rate.set_kb`, reflected in status bar. BitTorrent: port range → `network.port_range.set`, DHT on/off → `dht.mode.set` (caption "may require rtorrent restart" on failure). Behavior: confirm-on-remove toggle. Advanced: log verbosity, mock-mode toggle.
  **AC:** setting a 5 MiB/s up-limit shows in rtorrent (`throttle.global_up.max_rate`) and caps real traffic.
  **Verify:** live daemon check.

- [x] **E11-S5 · Watched folder** (M)
  Rust `notify`-based watcher on the configured dir: new `*.torrent` → auto-add with defaults (respecting do-not-start), rename to `*.torrent.loaded` on success, log entry either way; robust to duplicates/partial writes (debounce + parse-validate).
  **AC:** dropping a file into the watch dir adds it within 2 s; bad file logged, not crash-looped.
  **Verify:** manual drop of good + garbage file.

---

## E12 — Statistics dialog  *(M5)*

Deps: E3-S6, E2-S1.

- [x] **E12-S1 · Stats plumbing & verification spike** (M)
  Verify against live rtorrent 0.9.8 which of these exist and map: `throttle.global_*.total`, `pieces.memory.current/.max`, `pieces.stats_preloaded/.stats_not_preloaded`, `pieces.sync.queue_size`, per-torrent `d.skip.total`. Implement `get_statistics` returning: session down/up, **since-install** down/up/ratio (app-persisted counters accumulated from throttle totals across daemon restarts — store last-seen totals to handle counter resets), session waste (Σ `d.skip.total`), connected peers (Σ `d.peers_connected`), cache block (hit % from preloaded stats, buffer size, overload, queued I/O) with `None` for unavailable.
  **AC:** document actual mappings in code comments; counters survive app+daemon restarts without double-count.
  **Verify:** `cargo test stats` (persistence logic) + live daemon smoke.

- [x] **E12-S2 · Statistics dialog UI** (S)
  Per design screen 05 (400 px): User Statistics + Cache Statistics groups, `key … value` rows with row dividers, ratio in green when ≥ 1, `—` for unavailable; label all-time rows "(since install)" tooltip.
  **AC:** matches design; opens from app menu.
  **Verify:** side-by-side.

---

## E13 — Resilience & polish  *(M6)*

Deps: all M1–M5 epics.

- [x] **E13-S1 · Disconnected / first-run experience** (M)
  When `ConnState` ≠ connected: table area shows a centered card — "can't reach rtorrent at {endpoint}", retry countdown, buttons Open Preferences / Retry now, and a collapsible `.rtorrent.rc` snippet (from docs). Toolbar actions disabled; title bar shows disconnected.
  **AC:** fresh install with no daemon lands here (not a blank/broken UI); recovers automatically when daemon appears.
  **Verify:** launch without rtorrent, then start it.

- [ ] **E13-S2 · Performance pass & virtualization decision** (M)
  Profile with mock generating 1,000 torrents (add mock flag): if frame drops during 1 s snapshots, add TanStack Virtual to the table body (row markup unchanged). Also: drop poll to 5 s when window unfocused/hidden, resume fast on focus.
  **AC:** 1,000 rows scroll at 60 fps; CPU near-idle when backgrounded.
  **Verify:** Instruments/Activity Monitor before/after.

- [x] **E13-S3 · UI-state persistence & window memory** (S)
  Persist and restore: window size/position, sort, active filter, detail tab, last save-path per label (nice-to-have: skip if >S).
  **AC:** relaunch restores everything.
  **Verify:** manual relaunch.

- [ ] **E13-S4 · Accessibility & keyboard completeness** (M)
  Focus rings (cyan outline) on all interactive elements; table rows reachable by keyboard; dialogs labelled (`aria-modal`, labelledby); context-menu key (or ⌃-return) opens menu on selection; VoiceOver announces row name+status.
  **AC:** full app drivable without mouse; VoiceOver pass on main window + one dialog.
  **Verify:** keyboard-only session; VoiceOver spot-check.

- [ ] **E13-S5 · Manual QA checklist run** (M)
  Write `docs/qa-checklist.md` covering every design interaction (§Interactions in design README) × {mock, live}; run it; file/fix findings as new stories.
  **AC:** checklist committed with all items checked for the release candidate.
  **Verify:** the checklist itself.

---

## E14 — Packaging  *(M6)*

Deps: E13.

- [x] **E14-S1 · App icon & bundle config** (S)
  Dark-ops app icon (rounded-square `#14161a`, cyan down-arrow glyph) via `tauri icon`; bundle metadata (name, version 0.1.0, copyright, category public.app-category.utilities); dmg target.
  **AC:** `npm run tauri build` yields a branded `.app` + `.dmg`.
  **Verify:** build + open dmg.

- [ ] **E14-S2 · Release build QA + signing docs** (S)
  Run the QA checklist against the release build on a clean macOS account (mock + live). Write `docs/release.md`: build steps, ad-hoc signing status, exact steps for Developer-ID signing + notarization when a cert exists (not required for v1).
  **AC:** release build passes checklist; docs complete.
  **Verify:** clean-account run.

---

## E15 — Stretch (post-v1, unordered)

- [x] **E15-S1 · `.torrent` file association + `magnet:` URL scheme** — deep-link plugin + fileAssociations; opening either routes into the add dialogs (or instant-add per prefs).
- [ ] **E15-S2 · HTTP(S) XML-RPC transport with basic auth** — for nginx/ruTorrent-fronted remote daemons; unlocks remote use (keeps delete-data/reveal disabled).
- [ ] **E15-S3 · Move data on Set location** — local daemon same-volume rename, else copy+verify+erase; progress toast.
- [ ] **E15-S4 · Resizable/customizable columns** — drag header edges, show/hide via header context menu, persisted.
- [ ] **E15-S5 · Completion notifications** — macOS notification on download-complete (per-label opt-out in Behavior).
- [ ] **E15-S6 · RSS section** — feed polling + auto-add rules (fills the disabled prefs nav).

---

## Dependency quick-map

```
E0 ─┬─ E1 ─┬─ E2 ─┬─ E4 ─┬─ E6 ─ E7
    │      │      ├─ E5  │
    ├─ E3 ─┴──────┤      ├─ E8, E9
    │             ├─ E10 │
    │             └──────┴─ E11, E12 ─ E13 ─ E14
```

Parallelization hint (per global instructions, 2–4 agents): after E0, one agent on E1→E2 (Rust core) while another does E3→E4/E5 (UI against mock types); dialogs E8/E9 and tabs E10 parallelize cleanly in M3/M4.
