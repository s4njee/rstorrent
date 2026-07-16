# rstorrent — Implementation Plan

A native macOS desktop client for the `rtorrent` daemon, built with **Rust + Tauri 2**, implementing the **"Dark Ops" (1c)** high-fidelity design in `design/`.

This document explains *what we're building and how*. The companion [tasks.md](tasks.md) breaks the work into epics and stories for execution. Read this document first; treat `design/README.md` as the authoritative visual spec.

---

## 1. Goal & scope

**Goal:** a qBittorrent-style GUI front-end for an already-running `rtorrent` daemon, controlled over its XML-RPC-over-SCGI interface. The app does **not** embed a BitTorrent engine — it is a pure client of rtorrent's RPC API.

**v1 scope (everything in the design handoff):**

- Main window: toolbar, filter sidebar (status/labels/trackers), 12-column torrent table, detail tab strip (general/trackers/peers/content/speed/log), status bar.
- Dialogs: Add torrent (from `.torrent` file, with file tree), Add magnet/URL, Preferences, Statistics, Remove confirmation, row right-click context menu.
- Live updates: 1–2 s polling of rtorrent; speeds, progress, ETA, counts all refresh.
- Actions: add (file/magnet), resume, pause, force recheck, remove (± data), set label, set location, copy magnet link, open destination in Finder, queue move up/down.
- Mock mode so the whole UI can be developed and demoed without a live rtorrent.

**Non-goals for v1** (visible in design nav but explicitly deferred):

- RSS and Web UI preference sections (render nav items disabled/dimmed with a "v2" tooltip).
- Moving data on "Set location" (v1 sets the path with a warning; file-move is stretch).
- Windows/Linux support (macOS only; nothing should *preclude* porting, but don't spend effort on it).
- rtorrent process management (installing/launching/supervising the daemon).

---

## 2. Reference design

| File | Role |
|---|---|
| `design/README.md` | **Authoritative spec**: design tokens, typography, per-screen layout, interactions, state model. |
| `design/rTorrent Client 1c.dc.html` | Chosen hi-fi design, all 7 windows/modals in one canvas. Open in a browser to inspect. |
| `design/rTorrent Client.dc.html` | Earlier 3-flavor exploration. Context only — do not implement 1a/1b. |

Key fidelity requirements (from the README, repeated here because they define the look):

- **Everything is monospace**: `ui-monospace, SFMono-Regular, Menlo, monospace` — labels, body, numerals. No web fonts.
- Base 11.5 px type; table cells 10.5 px; column headers 10 px uppercase with letter-spacing.
- Table row height **23 px**, zebra `#14161a` / `#171a1f`, selected `#1d2b33`.
- Full color-token table lives in `design/README.md` → implement as CSS custom properties, never hard-code hex in components.
- Status is **plain colored lowercase text**, not pills. Progress bar is an 8 px flat bar colored by status.
- The design's fake traffic lights are replaced by **real macOS traffic lights** via Tauri's overlay title bar (see §7).
- Emoji glyphs in menus/prefs (🏷 📁 🔗 ⚙ …) are placeholders — replace with hand-drawn inline SVG line icons (1.6 px stroke) matching the toolbar set.

The 10 sample torrents in the design (ubuntu/debian/fedora ISOs etc., with their exact sizes, states, speeds) double as the **mock-mode fixture** — see §5.7.

---

## 3. Tech stack

| Layer | Choice | Rationale |
|---|---|---|
| Shell | **Tauri 2.x** (macOS) | Requested. Small binaries, Rust backend, overlay title bar support. |
| Backend | **Rust** (stable), tokio | Async SCGI socket I/O, polling loop, file ops. |
| XML | `quick-xml` (hand-rolled minimal XML-RPC) | rtorrent emits the non-standard `<i8>` 64-bit int tag (xmlrpc-c extension); generic XML-RPC crates mishandle it and none speak SCGI. A ~300-line encoder/parser we own is more reliable. |
| .torrent parsing | `lava_torrent` | Bencode decode + info-hash + file list for the Add dialog, in Rust. |
| Trash | `trash` crate | "Remove + delete data" moves files to macOS Trash — never `rm -rf`. |
| Frontend | **React 18 + TypeScript + Vite** | Mainstream, best-documented Tauri pairing; safe for an implementing agent. |
| State | **Zustand** | Frequent (1 s) snapshot updates with cheap selectors; minimal boilerplate. |
| Styling | Plain **CSS Modules** + `src/theme/tokens.css` custom properties | Pixel-precise custom design; Tailwind/UI-kits add nothing here. |
| Tauri plugins | `dialog`, `opener`, `clipboard-manager`, `store` | File pickers, reveal-in-Finder, copy magnet, persisted settings. |
| Charts (Speed tab) | Hand-rolled SVG | One small area chart; no chart library. |
| Tests | `cargo test` (+ in-process mock rtorrent server), Vitest for store logic | See §8. |

No other runtime dependencies without a story-level justification.

---

## 4. Architecture

### 4.1 Process model

```
┌─────────────────────────── Tauri app ───────────────────────────┐
│  WebView (React/TS)                Rust core                    │
│  ┌─────────────────┐   events    ┌──────────────────────────┐   │
│  │ Zustand stores  │◀────────────│ Poller (tokio task)      │   │
│  │ components      │             │  fast 1s / slow 30–60s   │   │
│  │                 │──invoke────▶│ Commands (actions)       │   │
│  └─────────────────┘             │        │                 │   │
│                                  │  RtorrentApi (trait)     │   │
│                                  │   ├─ ScgiClient          │   │
│                                  │   └─ MockClient          │   │
│                                  └────────┼─────────────────┘   │
└───────────────────────────────────────────┼─────────────────────┘
                                            │ SCGI (unix socket or TCP)
                                     ┌──────▼──────┐
                                     │  rtorrentd  │
                                     └─────────────┘
```

- **All rtorrent I/O lives in Rust.** The WebView never talks to the network.
- The **poller** pushes state to the UI via Tauri **events**; user actions go UI → Rust via **commands**, which execute the RPC and then trigger an immediate re-poll so the UI reflects reality (no optimistic state beyond disabling buttons in flight).
- `RtorrentApi` is a trait so `MockClient` can stand in for the daemon (dev + tests).

### 4.2 Repository layout

```
rstorrent/
  plan.md · tasks.md
  design/                        # handoff (already present)
  src-tauri/
    tauri.conf.json              # overlay title bar, window size, identifier
    src/
      main.rs / lib.rs           # setup, state, plugin registration
      commands.rs                # #[tauri::command] surface (thin; delegates)
      poller.rs                  # polling loops, snapshot assembly, event emit
      settings.rs                # app settings model + tauri-plugin-store IO
      torrent_file.rs            # .torrent parsing for Add dialog (lava_torrent)
      log.rs                     # app event ring buffer → Log tab
      rtorrent/
        mod.rs                   # RtorrentApi trait + Value type
        scgi.rs                  # SCGI framing over unix socket / TCP
        xmlrpc.rs                # encode methodCall / parse methodResponse (incl. <i8>, faults)
        client.rs                # typed wrappers: multicall, actions, globals
        derive.rs                # PURE status/ETA derivation fns (unit-tested)
        mock.rs                  # MockClient with design-fixture torrents
  src/
    main.tsx / App.tsx
    theme/tokens.css             # design tokens as CSS custom properties
    ipc/{types.ts,commands.ts,events.ts}   # ONE place defining the IPC contract
    store/{torrents.ts,ui.ts,settings.ts}
    utils/format.ts              # bytes, rates, ETA, ratio formatting
    components/
      shell/    (TitleBar, Toolbar, StatusBar, Layout)
      table/    (TorrentTable, Row, ProgressBar, headers/sorting)
      sidebar/  (FilterSidebar)
      details/  (DetailTabs, GeneralTab, TrackersTab, PeersTab, ContentTab, SpeedTab, LogTab)
      dialogs/  (ModalBase, AddTorrent, AddMagnet, RemoveConfirm, Preferences, Statistics)
      menu/     (ContextMenu)
      icons/    (inline SVG icon set)
```

### 4.3 IPC contract (single source of truth: `src/ipc/types.ts` mirrored by Rust structs)

**Events (Rust → UI):**

| Event | Payload | Cadence |
|---|---|---|
| `state://snapshot` | `{ torrents: TorrentDto[], globals: GlobalStats, connection: ConnState }` | every fast poll (~1 s) |
| `state://detail` | `{ hash, tab, data }` (trackers/peers/files rows) | ~2 s while a detail tab is open |
| `log://append` | `LogEntry` | as they happen |

**Commands (UI → Rust), all `Result<T, AppError>`:**

`read_torrent_metadata(path)` · `add_torrent(source: File{path}|Magnet{uri}, opts: {savePath, label, start, topOfQueue, sequential?, skipHash?, unselectedIndexes})` · `start(hashes)` · `stop(hashes)` · `recheck(hashes)` · `remove(hashes, deleteData)` · `set_label(hashes, label)` · `set_location(hash, path)` · `queue_move(hashes, up|down)` · `copy_magnet(hash) -> string` · `open_destination(hash)` · `get_settings()` / `apply_settings(patch)` · `test_connection(conn)` · `set_detail_watch(hash|null, tab|null)` · `get_statistics()`

`TorrentDto` fields follow the design README's state section: `hash, name, size, bytesDone, percent, status, statusMsg, seedsConnected, peersConnected, downRate, upRate, etaSeconds, ratio, label, trackerHost, savePath, priority, isPrivate`.

---

## 5. rtorrent integration

### 5.1 Transport: SCGI

rtorrent exposes XML-RPC over SCGI via either `scgi_local` (unix socket, **preferred default**) or `scgi_port` (TCP, typically `127.0.0.1:5000`).

- SCGI request = netstring: `"<len>:" + headers + "," + body`, headers NUL-separated with `CONTENT_LENGTH` **first**, plus `SCGI\x001\x00`.
- Response = CGI-style headers (`Status`, `Content-Type`), blank line, XML body.
- One request per connection (rtorrent closes it). Serialize requests through a small connection helper; timeouts (2 s connect / 5 s read) and typed errors (`Unreachable`, `Timeout`, `Fault{code,msg}`, `Parse`).

**Security note:** SCGI has no auth. Default to the unix socket. If the user configures TCP to a non-localhost host, show a warning in Preferences → Connection. XML-RPC over HTTP(S) with basic auth (nginx/ruTorrent-style setups) is a stretch epic.

### 5.2 XML-RPC dialect

Support exactly what rtorrent uses: `string`, `int`/`i4`, **`i8`** (64-bit, non-standard), `array`, `struct` (faults, `system.multicall` results), `base64` (for `load.raw*`). Batch independent action calls through `system.multicall`.

### 5.3 List poll (fast, ~1 s)

One `d.multicall2('', 'main', …)` fetching per torrent:

```
d.hash= d.name= d.size_bytes= d.bytes_done= d.complete= d.is_active= d.is_open=
d.hashing= d.message= d.down.rate= d.up.rate= d.ratio= d.custom1= d.directory=
d.base_path= d.peers_complete= d.peers_accounted= d.peers_connected= d.priority=
d.is_private= d.state_changed= d.timestamp.finished=
```

Plus global calls in the same poll cycle: `throttle.global_down.rate/.max_rate/.total`, `throttle.global_up.rate/.max_rate/.total`, `dht.statistics` (node count). Free disk space: `statvfs` on the default save path in Rust (local daemon assumption; hide the segment if the path is missing).

### 5.4 Status derivation (pure function in `derive.rs`, exhaustively unit-tested)

```
error       d.message non-empty (tracker/storage error)
checking    d.hashing > 0
paused      d.is_open == 0 || d.is_active == 0
seeding     complete == 1 && active
downloading complete == 0 && active && downRate > 0 (or recently > 0)
stalled     complete == 0 && active && downRate == 0 for ≥ stallWindow (default 30 s)
```

Sidebar counts use overlapping predicates (matching the design where 3+4+5+2+1+1 > 10): `all` = everything; `completed` = `d.complete == 1` (superset of seeding); others = status above. ETA = `(size − bytesDone) / downRate`, `∞` when rate is 0 and incomplete, `—` when paused/complete. Ratio = `d.ratio / 1000`.

### 5.5 Slow poll (~30–60 s) and detail poll (~2 s, only while a tab is open)

- **Slow:** per-torrent primary tracker domain (`t.multicall(hash,'','t.url=')` → first URL's host, cached by hash) → feeds the Tracker column and sidebar Trackers group. Optionally scrape totals later.
- **Detail (selected torrent + active tab only):**
  - *trackers:* `t.multicall`: url, is_usable, success/failed counters, scrape complete/incomplete, last announce.
  - *peers:* `p.multicall`: address, client_version, completed_percent, down/up rate, flags (encrypted, incoming, snubbed).
  - *content:* `f.multicall`: path, size, completed_chunks/size_chunks, priority. Rendered as the same folder tree as the Add dialog; `f.priority.set` (0 off / 1 normal / 2 high) from the UI.
  - *speed:* no RPC — frontend keeps a ring buffer of the selected torrent's rates from snapshots (~10 min) and renders an SVG area chart (down cyan `#58c4dd`, up green `#57d597`).
  - *general:* from snapshot + `d.down.total`, `d.up.total`; limits shown are the global throttle max rates.
  - *log:* app event log (connection changes, action results, RPC errors, per-torrent `d.message` transitions) from the Rust ring buffer; entries tagged with a hash are highlighted when that torrent is selected.

### 5.6 Action mapping

| UI action | RPC |
|---|---|
| Add .torrent (start) | `load.raw_start('', <base64 bytes>, "d.directory.set=…", "d.custom1.set=…", …)` (use `load.raw` variant when "start" unchecked; unwanted files → `f.priority.set=0` after load; `d.check_hash` respects "skip hash check" by *not* forcing) |
| Add magnet | `load.start('', "magnet:…", commands…)` / `load.normal` when not starting |
| Resume / Pause | `d.start` / `d.stop` (batched via `system.multicall`) |
| Force recheck | `d.check_hash` |
| Remove | `d.erase`; if "delete data": read `d.base_path` first, erase, then move path to Trash (`trash` crate). Disable the checkbox when the daemon isn't on localhost. |
| Set label | `d.custom1.set` (ruTorrent convention) |
| Set location | `d.stop` (if active) → `d.directory.set` → restart if it was active; UI warns "files are not moved" |
| Queue up/down | `d.priority.set` step within 0–3 (rtorrent has no true queue order — documented approximation; toolbar tooltip says "priority") |
| Copy magnet | Build `magnet:?xt=urn:btih:{hash}&dn={name}&tr=…` from cached tracker URLs |
| Open destination | Reveal `d.base_path` in Finder via opener plugin (localhost only) |
| Speed limits (prefs) | `throttle.global_down.max_rate.set_kb` / `…up…` |
| Port / DHT (prefs) | `network.port_range.set`, `dht.mode.set` |

### 5.7 Mock mode

`RSTORRENT_MOCK=1` (env or settings flag) swaps in `MockClient`: the 10 design-fixture torrents with ticking simulated progress/rates, so every screen is developable and demoable offline. The mock implements the same `RtorrentApi` trait and honors actions (pause actually pauses the fake torrent). CI tests run against an in-process SCGI mock server (same fixtures) to exercise the real transport code.

---

## 6. Frontend architecture

### 6.1 Stores (Zustand)

- `torrents.ts` — snapshot from `state://snapshot`, reconciled by hash (reuse object identity when unchanged to keep row re-renders cheap); derived selectors: filtered+sorted visible list, sidebar counts, selection-aware aggregates.
- `ui.ts` — `selection: Set<hash>`, `anchorHash` (shift ranges), `activeFilter {type:'status'|'label'|'tracker', value} | null`, `searchText`, `sortColumn/sortDir`, `activeDetailTab`, `dialog: null|'add-file'|'add-magnet'|'prefs'|'stats'|'remove'` + dialog form models, `contextMenu {x,y}|null`.
- `settings.ts` — mirror of Rust-persisted app settings.

Persist UI prefs (sort, filter, tab, window size) to `localStorage`; app settings (connection, paths, poll intervals) live Rust-side via `tauri-plugin-store` so the poller can read them without the WebView.

### 6.2 Filtering/sorting/search semantics

- Sidebar filter AND search text AND (nothing else) → visible rows. Search matches name, label, tracker (case-insensitive substring).
- Sort: click header toggles asc/desc; numeric columns compare raw numbers (bytes, rates, seconds), not formatted strings. Default sort: queue/insertion order as returned by rtorrent.
- Counts in the sidebar reflect *unfiltered* totals (per design).

### 6.3 Formatting (`utils/format.ts`, unit-tested)

Binary units (`KiB/MiB/GiB`, 1 decimal ≥ 10 shows 0 decimals per design samples like `5.8 GiB`, `631 MiB`), rates `X.X MiB/s`, ETA compact `4m12s` / `13m40s` / `∞` / `—`, ratio 2 decimals. Match the design's sample strings exactly — they are the fixtures.

### 6.4 Component notes

- **Table** is CSS grid with the exact template from the design: `minmax(200px,1fr) 70 92 84 52 52 76 76 62 46 72 110`. Header row is not part of the scroll body. Virtualize rows (TanStack Virtual) only if profiling shows >200 rows hurting; write the row as a memoized component from day one.
- **ContextMenu** and **ModalBase** are our own tiny primitives (positioning, Esc/Enter, focus trap, backdrop dim) — no library.
- **Icons**: one `icons/` module of inline SVGs; toolbar glyphs copied from the design HTML verbatim, menu/prefs emoji replaced by drawn equivalents (1.6 px stroke, `#8b93a2`).

---

## 7. macOS specifics

- **Window chrome:** `tauri.conf.json` → `"titleBarStyle": "Overlay"`, `"hiddenTitle": true`. Native traffic lights float over our `bg/panel` header strip (34 px, `data-tauri-drag-region`), which renders the centered title text `rtorrent {version} · {n} torrents`. Do **not** draw fake traffic lights.
- **App menu:** minimal native menu (About, Preferences… ⌘, / Quit ⌘Q, Edit menu for copy/paste in inputs). Built with Tauri's menu API.
- **Keyboard:** ⌘O add file, ⌘⇧O magnet (avoid ⌘U conflict? fine, configurable later), ⌘F focus search, Space pause/resume selection, ⌫ remove (opens confirm), ⌘A select all, arrows + shift/cmd for selection.
- **Reveal/Trash:** opener plugin `reveal_item_in_dir`; `trash` crate for deletions.
- **Packaging:** `tauri build` → `.app` + `.dmg`. App icon (dark-ops styled down-arrow glyph) via `tauri icon`. Code-signing/notarization documented but optional (ad-hoc signed dev builds fine).
- **Stretch:** `.torrent` file association + `magnet:` URL scheme via deep-link plugin.

---

## 8. Testing & verification

| Layer | How |
|---|---|
| XML-RPC encode/parse | Rust unit tests incl. `<i8>`, faults, `system.multicall`, real captured rtorrent responses as fixtures |
| SCGI framing | Unit tests + integration test against in-process mock SCGI server |
| Status/ETA derivation, formatters | Pure-function unit tests (Rust `derive.rs`, TS `format.ts`) — table-driven, covering every design sample row |
| Store logic (filter/sort/selection) | Vitest |
| End-to-end | `tauri-driver` does **not** support macOS → per-milestone manual QA checklist in tasks.md, run in mock mode; visual comparison against `design/rTorrent Client 1c.dc.html` opened side-by-side |
| Live daemon | `brew install rtorrent`, minimal `~/.rtorrent.rc` with `scgi_local` (snippet ships in repo `docs/rtorrent-setup.md` and in the app's disconnected screen) |

Definition of done for the project: all v1 stories complete, manual QA checklist green in both mock and live-daemon modes, `cargo test` + `npm test` + `cargo clippy -- -D warnings` + `tsc --noEmit` clean, `.dmg` builds and runs on a clean macOS user account.

---

## 9. Milestones

| # | Deliverable (demoable) | Epics |
|---|---|---|
| **M0** | App builds & launches: dark window, overlay title bar, tokens.css, CI-able scripts | E0 |
| **M1** | Read-only main window: live table + sidebar counts + status bar from mock **and** live rtorrent | E1, E2, E3, E4, E5 |
| **M2** | Control: selection, toolbar + context-menu actions, remove confirmation, keyboard | E6, E7 |
| **M3** | Add flows: .torrent dialog with file tree, magnet dialog, error surfacing | E8, E9 |
| **M4** | Detail tabs all functional | E10 |
| **M5** | Preferences + Statistics wired to daemon/app settings | E11, E12 |
| **M6** | Resilience (disconnected/reconnect), polish, perf, packaged .dmg | E13, E14 |

Ship order is strict M0→M6; within a milestone, stories parallelize per the dependency notes in tasks.md.

---

## 10. Risks & open questions

| Risk | Mitigation |
|---|---|
| rtorrent has **no real queue order** | Map queue buttons to `d.priority` steps; label honestly in tooltip. Revisit with view-based ordering in v2. |
| Some stats in the design (all-time totals, session waste, cache hit %) have **no direct RPC** | All-time totals: app-persisted counters accumulated from `throttle.global_*.total` deltas ("since install"). Cache stats: best-effort from `pieces.memory.*` / `pieces.stats_*`; show `—` for anything unavailable. Exact command availability must be verified against rtorrent 0.9.8 in E12-S1. |
| "Keep incomplete in…" pref isn't a native rtorrent feature | Render the control disabled with tooltip "requires rtorrent watch/move config" (v2: app-side move-on-complete). |
| Remote (non-localhost) daemons break delete-data / reveal / free-space | Feature-gate those affordances on localhost detection; hide/disable otherwise. |
| `d.directory.set` semantics (torrent must be closed) | Wrap in stop→set→conditional-start sequence server-side; surface failures in Log. |
| 1 s polling with many torrents could churn the WebView | Snapshot reconciliation by hash + memoized rows; virtualization story gated on profiling. |
| `protocol.encryption` / port changes may need daemon restart | Mark such prefs with "takes effect after rtorrent restart" caption when set fails live. |

Open questions parked for v2: multiple daemon profiles, HTTP(S)+auth transport, RSS, per-torrent throttles (rtorrent named throttles), column customization/resize.

---

## 11. How to use these docs

1. Work through [tasks.md](tasks.md) epic by epic in milestone order (§9).
2. Every story lists acceptance criteria and a **Verify** step — run it before marking the story done.
3. When a visual question arises, `design/README.md` wins, then the 1c HTML, then this plan.
4. Update tasks.md checkboxes as you go; append discovered work as new stories rather than silently expanding existing ones.
