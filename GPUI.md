# rstorrent — GPUI port plan

Port the desktop GUI from **Tauri 2 + React/TypeScript** to **GPUI**, keeping
`crates/rtorrent` (the host-agnostic client layer) exactly as it is. The web
client (`server/` + the browser half of `src/`) keeps the React UI; only the
desktop shell changes.

Companion documents: [plan.md](plan.md) is the architecture of the product,
[tasks.md](tasks.md) the Tauri-era execution tracker, [backlog.md](backlog.md)
and [backlog-v2.md](backlog-v2.md) the feature roadmaps. This file owns the port.

---

## Backlog

### Top priority: parity gaps left by the Tauri removal

The Tauri shell was removed on 2026-09-19 (`3323c6d`) before these were
ported, so for now the app simply lacks them. They come first, in the order they
matter (§4). Each is a G7 story (§11). The Tauri implementation is the
behavioural reference, and it's still in history at `44e6680`, the last commit
before the removal. Read it with `git show 44e6680:<path>`.

| # | Gap | Size | Tauri reference (`git show 44e6680:…`) | Notes |
|---|---|---|---|---|
| 1 | **Tray / menu-bar item**: rates; Show/Hide; Add; Resume/Pause All; turtle toggle; Preferences; Quit | L | `src-tauri/src/tray.rs`, tray setup in `src-tauri/src/lib.rs` | G7-S1. On Windows this is the notification-area icon. |
| 2 | **Drag & drop `.torrent` onto the window** → add dialog | M | `src/hooks/useDragDrop.ts`, `src/addQueue.ts` | G7-S2. Drops and pastes fed one add queue. |
| 3 | **Paste-to-add**: ⌘V/Ctrl+V on the main window with a magnet or torrent URL on the clipboard opens the add flow | S | `src/hooks/usePasteToAdd.ts`, `src/externalOpen.ts` (`parsePastedText`) | G7-S3. Only the add dialog's clipboard prefill exists today (`add_dialogs.rs`). Pastes into text fields and non-link text must be ignored. |
| 4 | **Watch-folder runner**: `notify`-based auto-add with a stability delay, `.loaded` rename and an error inbox | L | `src-tauri/src/watcher.rs` | G7-S4. Preferences already edits `watch_folders`; nothing acts on them. On Windows, a `/mnt/c/…` folder must be watched from the Windows side. |
| 5 | **`.torrent` / `magnet:` association**: claim the file type and URL scheme, and route argv and deep links (plus a second launch, via single-instance forwarding) into the add dialogs | M | `src-tauri/src/open_requests.rs`, `src-tauri/src/lib.rs` (single-instance + deep-link plugins), `src-tauri/tauri.conf.json` (`fileAssociations`, `deep-link` schemes) | G7-S5. On macOS: `CFBundleDocumentTypes` + `CFBundleURLTypes` in `tools/bundle-gpui-macos.sh`'s Info.plist, and an open-URLs handler. On Windows: registry entries from an installer (windows.md §5). |

Once these land, the §4 "not yet ported" list is empty and V3-01's accept
criterion (§11) is reachable.

---

## 1. Why, and what it changes

| Area | Tauri 2 + React today | GPUI |
|---|---|---|
| Frame cost | Webview layout + JS; a hand-written virtualiser keeps 5k rows bounded | Native GPU UI; `uniform_list` over `Arc<[TorrentDto]>`, no IPC or projection copy |
| IPC | 40+ commands and 7 event streams, JSON over the Tauri boundary | None: views call `rtorrent-core` through an in-process Tokio-backed facade |
| Processes | WebKit/WebView2 helper + main process | One native process |
| Startup | Vite bundle, webview bootstrap | Native window, bundled font registration |
| Platform reach | WebKitGTK is the weak Linux target; no Windows rtorrent (WSL bridge) | Metal on macOS, Direct3D 11 on Windows; Windows runs a bundled Linux rtorrent in its own WSL distro (see [windows.md](windows.md)) |

Costs paid: a second UI stack during the migration, a younger toolkit, and
every view re-implemented rather than reused. The design handoff (~14 surfaces)
is the work; the client layer and the DTO contract are shared and unchanged.

**Decision (2026-09-15):** `gpui-kit` 0.6.1 with the `gpui-pre` snapshot it was
resolved against (`=0.3.2`), both pinned exactly — the same pin the Ferrite and
Excalibur shells build and ship on. `gpui-kit` re-exports GPUI, so
`gpui_kit::prelude` *is* GPUI's prelude; the `gpui` dependency is named
explicitly only because `#[derive(Action)]` generates `gpui::` paths.

---

## 2. Where the port lives

```
crates/gpui/                    rstorrent-gpui: the native shell
  src/main.rs                   open the window; fonts, palette, icon, services
  src/theme.rs                  the design tokens (mirrors src/theme/tokens.css)
  src/format.rs                 bytes/rates/eta/ratio formatting (mirrors utils/format.ts)
  src/settings.rs               the same settings.json the Tauri shell reads and writes
  src/services.rs               Tokio-backed facade over RtorrentApi + host policy
  src/model.rs                  the polled torrent list, selection, sort, filter
  src/table_state.rs            pure filter/search/sort/selection/counts
  src/columns.rs                pure column definitions, widths, visibility
  src/throttles.rs              pure allocation for the per-torrent rate-limit pool
  src/icons.rs                  the design's line icons as embedded SVG
  src/actions.rs                actions, key bindings, native menus
  src/localfs.rs                reveal, trash, daemon-path translation
  src/shell.rs                  title bar, toolbar, selection bar, status bar, overlays
  src/sidebar.rs                the filter sidebar
  src/torrent_table.rs          the virtualised torrent table
  src/dialogs.rs                modal frame plus the confirm/limit/label/location dialogs
  src/app_icon.rs               the Dock icon for direct launches
```

**The seams that matter.** `rtorrent-core` is unchanged and UI-free: the port
depends on its `RtorrentApi` trait, its DTOs, and its pure derivation, and never
reaches around it. `services.rs` owns the one Tokio runtime and turns each
daemon call into a `Send` future a GPUI task can await, so views hold no runtime
handle. `model.rs` owns the poll loop and is the only writer of the torrent
list. Pure rules — formatting, filtering, sorting, selection, columns — live in
modules with no GPUI dependency and are unit-tested directly, exactly as
`src/store/selectors.ts` and `src/utils/format.ts` are in the Tauri tree.

**The Tauri shell has been removed (2026-09-19).** For the surfaces not yet
ported, its behaviour is still in git history — `git show 44e6680:src-tauri/…`
is the last commit that has it. Where this document and the code disagree, the
code is current.

---

## 3. Epics

Port order is strict; each epic is demoable on its own.

| # | Deliverable | Source of truth |
|---|---|---|
| **G0** | Foundation: crate, palette, fonts, window, settings file, icon | `tokens.css`, `tauri.conf.json` |
| **G1** | Main window: title bar, toolbar, filter sidebar, virtualised table with configurable columns, status bar, disconnected card | `App.tsx`, `shell/*`, `sidebar/*`, `table/*` |
| **G2** | Reading: the poll loop, connection/backoff, tray-free status, free space | `poller.rs` |
| **G3** | Control: selection, the action set, context menu, column menu, keyboard, remove/label/limit/location dialogs | `actions.ts`, `menu/ContextMenu.tsx`, `useKeyboard.ts` |
| **G4** | The bottom detail panel: Files, Peers, Trackers, Transfer, Pieces, Log | `details/*` |
| **G5** | Add flows: `.torrent` with the file tree, magnet, drag & drop, paste, watch folders | `dialogs/Add*`, `watcher.rs` |
| **G6** | Preferences and Statistics, wired to the daemon | `dialogs/Preferences*`, `StatisticsDialog.tsx` |
| **G7** | Native menus, tray, file association, deep links | `menu.rs`, `tray.rs` |
| **G8** | Resilience, accessibility, packaging, the QA checklist | `docs/qa-checklist.md` |
| **G9** | The web UI, served from the app (Daemon → Toggle Web UI) | `server/`, `web_host.rs` |

---

## 4. Status

| Epic | State |
|---|---|
| **G0** foundation | Done. Crate builds; palette, fonts, settings file, app icon and menu bar in place. |
| **G1** main window | Done, with the full 14-column set, per-column resize, a visibility menu, an empty state and a selection bar. |
| **G2** reading | Done: 1 s poll with backoff, disconnected card, globals in the status bar. |
| **G3** control | Done: the whole action set, native menus, key bindings, the row context menu, and the Remove / Set label / Rate limit / Set location dialogs. |
| **G4** detail panel | Done: the six panes under the table, on their own poll (§8). The pane set is the Tauri drawer's current one, not this file's older "General/Content/Speed" wording. |
| **G5** add flows | Done: add a `.torrent` (picker, metadata, contents tree), add a magnet/URL, Create torrent, and remove with ⇧Del (§9). Not done: drag & drop onto the window, paste-to-add and the watch folder. |
| **G6–G7** | G6 done (Preferences S1–S5 + Statistics S6); G7 not started. |
| **G8** packaging | The macOS bundle and its bundled rtorrent are done (§7); the accessibility pass, the QA checklist sweep and the resilience work are not. |
| **G9** web UI in-app | Done: the app serves the browser UI itself, one menu item toggling it (§10). |

**Not yet ported, in the order they matter:** the tray; drag &
drop onto the window, paste-to-add and the watch folder runner;
`.torrent`/`magnet:` association.

Ported since this paragraph was first written: the **Statistics dialog**
(**Statistics** in the app menu — session and cache counters plus the daemon's
self-report, with persisted since-install totals in `stats.rs`); the poller
*policy* — turtle mode (maths in `turtle.rs`, enforcement in `policy.rs`, and the
Daemon menu toggle flips and persists `turtle_enabled`), seed goals and the
max-active-download queue; and free-space probing for the status bar
(`localfs::free_space` via `statvfs`).

**Known rough edges:**
- A column resize only tracks while the pointer stays inside the 24 px header
  strip; the alternative is a window-level mouse listener, which GPUI allows only
  from the paint phase.
- The app does not watch `settings.json` for changes made by the Tauri shell while
  it runs; the next launch picks them up.
- `.torrent` file association and `magnet:` links are unclaimed, so an add from
  outside the app has nowhere to land yet.

## 5. Testing

- **Pure logic has unit tests.** `format`, `columns`, `table_state`, `throttles`,
  `settings`, `stats`, `daemon`, the detail panel's six companions (§8) and
  `web_host` (§10) are testable without GPUI. `cargo test -p rstorrent-gpui`
  currently runs 186 of them, the add, create and remove flows among them (§9).
- **A live smoke run is the check for the views.** `cargo run -p rtorrent-gpui --
  --demo` opens the window over the fixture torrents; that is how the render paths
  are exercised today.
- **Headless UI tests are blocked, not merely absent.** GPUI's test harness
  (`gpui-kit/test-support`) refuses work scheduled from another thread — it fails
  the test with "Your test is not deterministic" — and `Services` deliberately
  owns a Tokio runtime whose worker threads complete its futures. Making the shell
  testable would need both a `.test_support()` opt-in on the elements under query
  and a services seam that resolves without a worker thread. It is the largest
  remaining gap: the detail panel is ~1600 lines of view code that only a person
  looking at the window has checked.

## 6. Conventions

- **No raw colours or sizes in views.** Colours come from `theme::Palette`,
  geometry from `theme::geometry`, text through `theme::text`/`theme::mono`.
  The token values mirror `tokens.css` and must not drift from it.
- **Pure logic is separated and tested.** If a rule can be written without GPUI,
  it lives in `table_state`, `columns`, `format` or `settings` and has a test.
- **Views never block.** Daemon work goes through `Services`; results are
  applied inside `cx.spawn`, and every mutation asks for an immediate re-poll.
- **The daemon is the truth.** No optimistic row state: an action re-polls and
  the UI shows what the daemon reports.
- **Icons are embedded SVG**, tinted by the caller (`svg().data(bytes)`), never
  an icon font — the design's 12×12 / 1.6 px stroke set is the reference.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean
  before an epic is done.

---

## 7. The bundled rtorrent

The macOS app ships its own rtorrent so a fresh install needs no `brew install
rtorrent`. Nothing in cargo builds it — the staging script does, and two steps
then carry it into the app.

**To build the app, run one command:**

```sh
tools/bundle-gpui-macos.sh
```

It stages the runtime (from source on the first run, from the cache after),
builds the web console the binary embeds, builds the release binary, and
assembles `dist-gpui/rstorrent-gpui.app` with the runtime inside it, ad-hoc
signed and verified. A warm re-run is a couple of seconds — the console is only
rebuilt when its sources are newer than its output, which matters because any
change under `dist-web/` makes rust-embed re-run and cargo relink the binary.

Two things to know if you build by hand instead. `cargo build` alone needs
`dist-web/` to exist, because `server/src/assets.rs` embeds the console from it
with `#[folder = "../dist-web"]` — `npm run build:web` produces it, and without
it the crate does not compile from a fresh clone. And a bare `cargo build` gets
the runtime beside the binary through `build.rs` rather than inside a bundle:

| Step | What it does |
|---|---|
| `tools/build-rtorrent-macos.sh` | Builds rtorrent + libtorrent from source at the pinned tag (`v0.15.7` by default) and stages them in `binaries/rtorrent-macos/` — the daemon plus its non-system dylibs, every one referenced through `@executable_path`, so the directory is relocatable |
| `crates/gpui/build.rs` | Copies that directory to `<target>/<profile>/binaries/rtorrent/` at build time, which is what a bare `cargo run`/`cargo build` looks beside |
| `tools/bundle-gpui-macos.sh` | The one command above: runtime, console, binary, bundle, signature, verification |

**Resolution order** (`daemon::bundled_rtorrent`, first hit wins): the
`RSTORRENT_RTORRENT_BIN` override, then `Contents/Resources/binaries/rtorrent/`
(a bundle), then `binaries/rtorrent/` beside the executable (a cargo build).
Nothing found there means a system rtorrent from `PATH`, then the usual package
manager locations — and if there is none either, the start fails with a message
saying so rather than doing nothing.

**Starting** (`daemon::start`, **Daemon → Start Daemon** or the disconnected
card's *Start rtorrent*) is a port of the Tauri `daemon_start`: it refuses a
non-local transport, clears stale lock and socket files, writes a starter
`~/.rtorrent.rc` when the bundled runtime is used and the user has none, and
launches inside a detached tmux session (falling back to a direct spawn). The
launch blocks, so it runs on the background executor and the poll loop is nudged
afterwards; the outcome lands in the status bar. On Windows the same entry
points go to `daemon_wsl`, which imports the bundled runtime as a dedicated WSL
distro and launches rtorrent there — see [windows.md](windows.md).

**It will not claim a daemon that died.** rtorrent opens its socket *before* it
finishes reading `~/.rtorrent.rc`, so a key it does not recognise kills it and
still leaves the socket behind — for which "the socket exists" reads as success
and the poller contradicts it a second later. So the start settles briefly and
then checks the process (or its tmux session) before reporting anything; a
failure says the daemon exited, and quotes its stderr when it wrote any. Its
*stdout* is left alone on purpose: that is the interface rtorrent draws, and
whoever attaches to the session should find it there.

**Which binary would it use?** Bundling is invisible when it works, so
`rstorrent-gpui --where-rtorrent` prints the resolution and exits, and
`rstorrent-gpui --start-rtorrent` runs the start itself from a terminal (exit
non-zero on failure) — the QA checklist's Start and "which binary did it use"
checks, neither of which needs a window, and the GPUI shell has no headless UI
tests (§5).

The result is ad-hoc signed, not notarized: Gatekeeper warns on first launch on
another machine (see [docs/release.md](docs/release.md)). The Release workflow
packages this `.app` and the Windows zip on a version tag.

---

## 8. The bottom detail panel

Six panes under the table — **Files · Peers · Trackers · Transfer · Pieces ·
Log** — in the shell's own tokens rather than the design handoff's
(`src/components/details/` is the behaviour being ported, not its styling). The
pane set is the Tauri drawer's current one; this file's older G4 wording
("General/Content/Speed") named the pre-console tabs.

`detail_panel.rs` is the view: the tab strip, the focused torrent's name, the
facts rail the Files and Transfer panes share, the error banner, and the six
panes. Everything a pane *decides* rather than draws lives beside it in a pure
module with tests:

| Module | What it owns |
|---|---|
| `detail_panes` | the pane set, which daemon tab each reads, which take the rail, the persisted name and its older spellings, and which torrent the panel describes |
| `detail_files` | paths → a folder tree, with size-weighted progress and mixed-priority folders |
| `detail_facts` | the rail's eleven figures, our additions, and the uploaded-bytes inversion |
| `detail_rows` | peer addresses and the tracker status vocabulary |
| `detail_pieces` | bitfield and availability decoding, and the bucketing to stripe columns |
| `rate_history` | per-torrent rate samples and the chart's geometry |
| `app_log` | the bounded ring behind the Log pane |

**Its own poll.** The panel reads a payload the model keeps in its own field, on
a 2 s cadence that wakes early when something asks for a refresh (an action's
re-poll, a settings change, a new subject), so a large payload never touches the
table's tick. It runs on **GPUI's** timer: an earlier version slept with
`tokio::time::sleep` inside a GPUI task, which panics for want of a reactor — the
smoke run caught it, and the same mistake is why `services::wait` is gone.

**Stripe columns, not a canvas.** The piece and availability stripes are divs —
a fixed column count, each mixed between the trough and the full colour by its
share of the slice (`theme::blend`) — which keeps the downsampling in a tested
module and the row a couple of hundred elements. The Transfer chart *is* a
canvas (`canvas` + `PathBuilder`), the crate's only custom painting so far.

**Not ported from the Tauri pane:** the peers' and trackers' rows keep their
context menus, but a row's flags legend and the tracker's last-announce column
are tooltips or extra width the shell has nowhere to put yet.

---

## 9. Add, create and remove

**Add magnet / URL** (⌘⇧O, the toolbar's magnet button) takes a magnet with a
btih hash or an `http(s)` URL. What counts as loadable, and a magnet's `dn=` name,
live in `add_source` with tests; the field adds on Enter, the header says nothing
when the line is empty and the reason when it is wrong, and a magnet on the
clipboard is offered when the dialog opens — but only when it would be accepted.

**Add torrent** (⌘O, the toolbar's plus) asks the platform for a `.torrent` with
`cx.prompt_for_paths` — GPUI's own `NSOpenPanel`, so no dependency was added —
and reads it off the UI thread. The dialog then shows the name, size and file
count, the destination and label, start / skip-hash / top-of-queue, and the
contents tree with folder-level tri-state boxes (a port of the Tauri
`folderState`/`selectedSize` into `detail_files`). Everything unticked is passed
as `unselected_indexes`, which rtorrent loads at priority 0 rather than omitting.

Neither dialog talks to the daemon. Each emits an `AddEvent` and the shell applies
it through the model — `add_raw` for bytes, `add_magnet` for a URI — so an add is
logged, a failure is noticed, and the list re-polls like every other action;
`services.rs` has a test that the whole form reaches the backend (destination,
label, start state and queue position all survive into the daemon call).

**Create torrent** (⌘N, the toolbar's create button) hashes a file or folder *on
this machine* into a `.torrent`: source with File…/Folder… pickers, where to save
it, announce URLs (added one at a time to a list, since the core takes a flat list
and tiers are not expressible), the piece-size ladder, a source tag, a comment,
and Private / start-seeding. It is the one dialog that does its own file I/O —
`torrent_file::create_torrent` is a local hash, not a daemon call — so it runs on
the service runtime's blocking pool, and starting a large torrent does not stall
the UI. Seeding afterwards goes through the same `add_raw` an add uses, pointed at
the source's parent directory, which is where rtorrent expects a torrent's data.

Two deliberate differences from the Tauri dialog: the result stays on screen until
you dismiss it rather than closing after a second, because the info-hash is the
value you need *next* (to upload to a tracker); and the trackers are a list rather
than a textarea, because GPUI's input has no multi-line mode wired up. The shell
also records the creation in the app log, so the info-hash is still there
afterwards.

**Remove** confirms the selection and names it — one torrent by name, several by
count — with an "also delete the files" box, disabled when the daemon is not on
this machine. ⇧Del (or the row menu's *Remove + data*) opens the same dialog with
that box already ticked, which is the one thing the Tauri flow had that this did
not.

---

## 10. The web UI, from the app

**Daemon → Toggle Web UI** serves the browser UI from inside the running app, and
toggles it off again. One process: `server/` is a library now (`rstorrent_web`,
with `main.rs` a thin CLI over it), so there is no second binary to ship, sign or
supervise — the app calls `rstorrent_web::serve` on the runtime it already has and
holds the returned `Server`. Stopping it drops the server, whose shutdown channel
closing is itself the graceful-stop signal.

`web_host.rs` is that glue:

- **The app's own daemon.** The config is built from the settings the app is
  connected with — transport, mock, save path, poll cadence — so the browser shows
  the same torrents without a config file, and switching the daemon moves both.
- **Loopback, no login.** The listener is `127.0.0.1` only; a password would
  protect nothing a local process cannot already reach, since the daemon's own
  SCGI socket is unauthenticated on the same machine. `auth.mode = "none"` is
  refused on a non-loopback bind by the server's own validation.
- **Port 9080 first, then any free one.** The preferred port keeps the URL worth
  remembering; a dev server already holding it must not stop the app offering the
  UI, so the second attempt binds port 0 and the status line reports what the OS
  gave it.
- **The link lands on the clipboard**, because that is the fastest way into a
  browser. The status bar and the Log pane both say where it is.

One consequence worth knowing: the SPA served this way is the bundle compiled
into the binary, exactly as the CLI serves it, so a frontend change needs
`npm run build:web` and a rebuild before it appears — or `npm run dev:web` for
live editing against the same daemon.

---

## 11. V3-01 shell-parity stories (the G6–G8 split)

**Decision (2026-09-16):** per backlog-v3 V3-01, GPUI is the desktop and the
Tauri shell is retired once these land — the Tauri shell stays the behavioural
reference but takes no new features. (Superseded 2026-09-19: the Tauri shell was
removed before G7 landed; see §4 for what is still unported.) This section is the XL split the backlog
requires before work starts; each story is sized S/M/L.

### G6 · Preferences and Statistics

| # | Story | Size | Source of truth |
|---|---|---|---|
| G6-S1 | Preferences shell: nine-pane nav (Behavior, Downloads, Connection, Speed, BitTorrent, Network, RSS, Web UI, Advanced), draft + Apply/Cancel over `update_settings`, per-field error | M | `PreferencesDialog.tsx` |
| G6-S2 | Connection + Speed panes: transport picker, test-connection, poll/stall, global limits, queue cap | M | E11-S3/S4 |
| G6-S3 | Downloads + BitTorrent panes: save path, watch-folder list, run-on-complete, port range, DHT, seed goals + label overrides | L | E11-S2/S4/S5 |
| G6-S4 | Behavior + Network + RSS panes: confirms, notification exclusions, encryption/PEX/proxy/bind, feeds + rules | L | `PreferencesDialog.tsx` |
| G6-S5 | Advanced + Web UI panes: mock toggle, log verbosity; Web UI stays a documented stub | S | E11-S4 |
| G6-S6 | Statistics dialog: session + cache counters and the daemon's self-report over `services.statistics()`/`daemon_health()`, with persisted since-install totals (`stats.rs`) | M | `StatisticsDialog.tsx`, E12 |

**G6-S6 is done.** **G6-S1–S5 are done** (2026-09-16): `prefs.rs` ships the
nine-pane dialog — draft + Apply/Cancel over `update_settings`, per-field
validation errors, transport picker with test-connection, poll/stall, limits,
queue, turtle schedule, save path, incomplete dir, collision policy, move
rules, label defaults, watch folders, run-on-complete, port range, DHT, seed
goals + label overrides, confirms, notification exclusions, encryption/PEX/
proxy/bind, feeds + rules, mock toggle, and the Web UI stub. Apply pushes
daemon-affecting keys without waiting for a reconnect
(`Services::push_daemon_config`); view state rides through untouched. The
other G7–G8 stories are open. The dialog ships the
`Statistics` and `Daemon` tabs and the menu path; the Tauri `History` tab (a
global rate chart) is deferred — GPUI's `rate_history` samples per torrent, so a
global series is its own small story rather than a port.

### G7 · Native integration

| # | Story | Size | Source of truth |
|---|---|---|---|
| G7-S1 | Tray / menu-bar item with rates, Show/Hide, Add, Resume/Pause All, turtle toggle, Preferences, Quit | L | B17, `tray.rs` |
| G7-S2 | Drag & drop `.torrent` onto the window → add dialog | M | C1 |
| G7-S3 | Paste-to-add: a magnet on the clipboard offered in the add dialog (already partly there in `add_dialogs`) | S | C2 |
| G7-S4 | Watch-folder runner: `notify`-based auto-add with stability delay, `.loaded` rename, error inbox | L | C12, E11-S5 |
| G7-S5 | `.torrent` / `magnet:` association: claim types, argv/deep-link routing into the add dialogs | M | B1, E15-S1 |

### G8 · Hardening

| # | Story | Size | Source of truth |
|---|---|---|---|
| G8-S1 | Resilience: stop/restart daemon, log tail, crash detection, existing-service discovery | L | V3-39, PLT-02 |
| G8-S2 | Accessibility pass: focus rings, keyboard-reachable rows/menus, labelled dialogs, VoiceOver pass | M | E13-S4 |
| G8-S3 | QA checklist sweep on the GPUI shell (mock + live), findings filed as stories | M | `docs/qa-checklist.md` |
| G8-S4 | GPUI release leg in CI + packaging docs update | M | V3-02, `docs/release.md` |

**Accept (from V3-01):** the QA checklist passes on the GPUI shell with no
"not ported yet" message reachable from a menu. One such message remains today
(Preferences); the turtle and Statistics ones were wired in this pass.

---

## 12. V3-10 tags — GPUI stories

Tags are the many-to-many companion to the single label (V3-10). The **shared
core and the web console landed first** (see the V3-10 entry in
`backlog-v3.md` and the web results in `docs/web-console-tasks.md`): `tags:
Vec<String>` on `RawTorrent`/`TorrentDto`, read from and written to
`d.custom=tags`, with `set_tags`/`add_tags`/`remove_tags` commands and a web
Tags sidebar group, chips, facet and bulk editor. GPUI is the *shipping desktop*
(V3-01), so V3-10 is not done until these land there.

| # | Story | Size | Notes |
|---|---|---|---|
| GT-S1 | Thread tags through GPUI: the DTO already carries them (`rtorrent-core`), so `model.rs` needs `tags()` and a `set_tags`/`add_tags`/`remove_tags` service passthrough, and `Filter::Tag(String)` + `parse_filter("tag:…")` in `table_state`/`model` | M | `model.rs`, `services.rs`, `table_state.rs` |
| GT-S2 | Sidebar Tags group with a colour square per tag and counts, after Labels; clicking sets the tag filter; the persisted filter string round-trips | M | `sidebar.rs`, `table_state.rs::sidebar_counts` |
| GT-S3 | Coloured tag chips in the Label column (or a hidden-by-default Tags column), coloured by the same name-hash palette as the web (`tagColour` in `utils/tags.ts` → a Rust mirror) | M | `torrent_table.rs`, `theme.rs` |
| GT-S4 | "Edit tags…" dialog (add/remove on the selection) and a menu item, mirroring the web `SetTagsDialog` and the existing `LabelDialog` | L | `dialogs.rs`, `actions.rs`, `shell.rs` |
| GT-S5 | Search includes tags (`matches_search`), and the QA checklist covers a tag add/filter/remove pass | S | `table_state.rs`, `docs/qa-checklist.md` |

**Not blocking GPUI:** user-configurable tag colours are a follow-up on *both*
shells (both currently derive a stable colour from the name); the web shows a
tag's colour without a settings field, and GPUI should match that first.

### GT results (2026-09-16)

GT-S1…GT-S5 landed, so the tags feature is now in both shipped shells:

- **GT-S1** — `Filter::Tag(String)` with a case-insensitive membership match,
  `parse_filter`/the persisted filter string round-trip `tag:…`, search includes
  tags, and `TorrentsModel::{tags, selected_tags, set_tags, edit_tags}` run
  through a new `Services::set_tags`. `edit_tags` groups the selection by the
  list each torrent should end up with (`table_state::tag_edits`), so a bulk
  add/remove is a few daemon calls rather than one per torrent.
- **GT-S2** — a **TAGS** sidebar group after LABELS, each row with a colour
  square whose hue is `theme::tag_colour` (the same name hash the web uses, so a
  tag is the same colour in both shells) and a global count.
- **GT-S3** — the Label column renders the label plus a small chip per tag,
  tinted with `theme::blend(app, colour, 0.22)`.
- **GT-S4** — an **Edit tags** dialog (`TagDialog`): the selection's tag union as
  chips (click to mark for removal), a field for new tags, and Cancel / Clear all
  / Apply, reachable from the app menu and the row context menu.
- **GT-S5** — covered by the tests above; the QA checklist's Tags line is below.

---

## 13. V3-12 library search — GPUI

The filename index (V3-12) is shared core work: `rtorrent_core::file_index` is
used by GPUI as well as the server. In the GPUI shell the model owns one
`Arc<Mutex<FileIndex>>`, fills a 5-torrent batch every 10 ticks, clears it on
reconnect, and drops removed torrents each tick; the table searches through
`table_state::visible_with_files`, whose `matches_search` now also covers
`hash` and `save_path`. No new UI: the existing filter box does the work.

---

## 14. V3-13 duplicates — GPUI

Duplicate detection (V3-13) is shared-core (`rtorrent_core::duplicates`). Both
GPUI add dialogs take the model entity, run `detect` against its live rows on
each render, and show the same amber warning as the console. An **exact** match
disables Add and offers **Show it** (`AddEvent::Reveal` →
`TorrentsModel::focus_torrent`, which selects the row and clears the
filter/search) and **Merge trackers** (`AddEvent::MergeTrackers` →
`TorrentsModel::merge_trackers`, one `add_tracker` per URL).
