# rstorrent manual QA checklist

Run this against a build in **both** modes:

- **Desktop (GPUI, the shipping shell):** `RSTORRENT_MOCK=1 cargo run -p rstorrent-gpui`
  (mock) or a live `cargo run -p rstorrent-gpui`; the bundled build is
  `dist-gpui/rstorrent-gpui.app`.
- **Web console:** `RSTORRENT_MOCK=1 cargo run -p rstorrent-web` with
  `npm run dev:web` (see [web-setup.md](web-setup.md)).
- **Tauri reference (frozen):** `RSTORRENT_MOCK=1 npm run tauri dev`, or a build
  with `mock: true`.

Mark each item ✅ / ❌ / n/a. File failures as new stories in `tasks.md` (or the
GPUI/web trackers).

## Main window

- [ ] Title bar shows `rtorrent <version> · <n> torrents`; window drags by the strip; traffic lights work.
- [ ] Table matches the design: 12 columns, 23px zebra rows, status as lowercase colored text, 8px progress bars colored by status, Down cyan / Up green.
- [ ] Mock shows the 10 fixture torrents in the design's states; the Fedora row's progress advances over time.
- [ ] Status bar shows DHT nodes, ↓/↑ rates, free space (live/localhost).

## Sidebar & search

- [ ] Status/Labels/Trackers groups show correct global counts.
- [ ] Clicking a filter narrows the table + highlights the row; re-clicking clears to All.
- [ ] Toolbar filter box narrows by name/label/tracker; combines with the sidebar filter; ⌘F focuses it.

## Selection, sorting, keyboard

- [ ] Click / ⌘-click / ⇧-click select as expected; ⌘A selects all visible; Esc clears.
- [ ] Header click sorts (numeric columns sort numerically); indicator arrow shows; persists across relaunch.
- [ ] Space pauses/resumes the selection; ⌫ opens Remove.
- [ ] Sort/filter/active-tab survive relaunch.

## Toolbar & actions (live)

- [ ] Resume/Pause/Recheck act on the selection; buttons disabled with no selection.
- [ ] Move up/down changes rtorrent priority.

## Context menu (live)

- [ ] Right-click selects the row and opens the menu at the cursor; closes on click-away / Esc.
- [ ] Resume, Pause, Force recheck work.
- [ ] Set label ▸ lists existing labels + none + a new-label input; the label appears in the sidebar next poll.
- [ ] Set location… opens the SetLocationDialog with path input and move-data checkbox; moves files on disk when selected on local daemons.
- [ ] Copy magnet link puts a working magnet on the clipboard; Open destination reveals the data in Finder (localhost).

## System tray & Menu-bar item (live)

- [ ] Tray icon shows in menu bar / system tray with live speed rates in title and tooltip.
- [ ] Left-click toggles and focuses the main window.
- [ ] Tray menu items (Show/Hide, Add File/Magnet, Create Torrent, Resume/Pause All, Turtle Mode toggle, Preferences, Quit) function correctly.

## Dialogs

- [ ] **Add torrent** (⌘O / menu): file picker → name/size/file-count, save path + Browse, label, options; tri-state file tree with select all/none and a live "selected" size; deselected files load at priority 0 (verify in Content tab).
- [ ] **Add magnet** (⌘⇧O / menu): validates magnet/URL; prefills from clipboard; adds on Add.
- [ ] **Create torrent** (⌘N / toolbar / menu): source file/folder picker, piece size (auto/manual), tracker list with tier support, private flag, comment, optional output save path, and instant seeding.
- [ ] **Remove** (⌫ / menu): shows name(s) + total size; delete-data checkbox disabled off-localhost; erase-only leaves data, delete moves files to Trash (recoverable).
- [ ] **Preferences** (⌘,): each section edits and Apply persists; Connection Test-connection reports the version; changing transport reconnects; Speed limits reflect in the status bar; port range / DHT apply; watched-folder path saves.
- [ ] **Statistics**: opens (app menu; Tauri also from the status-bar DHT segment); values populate; unavailable ones show —; all-time totals grow across sessions.
- [ ] All dialogs: Esc = Cancel, Enter = primary, ✕ = Cancel, backdrop blocks the window, focus trapped.
- [ ] **Duplicates (V3-13):** add a `.torrent` twice; the second time the Add dialog warns "Already in your library as …", Add is disabled, and **Show it** selects the existing row / **Merge trackers** adds the candidate's trackers.

## Detail tabs

- [ ] General shows the label/value grid for the selected torrent.
- [ ] Trackers / Peers populate for an active torrent (live).
- [ ] Content lists files; clicking a priority cell cycles off/normal/high and sticks (live).
- [ ] Speed chart accumulates and scrolls; empty hint before enough samples.
- [ ] Log shows actions/errors; selected torrent's entries highlighted.

## Watched folder (live)

- [ ] With a watch folder set, dropping a `.torrent` into it adds the torrent and renames the file to `*.torrent.loaded`.
- [ ] **Tags (V3-10):** select a torrent, **Edit tags…**, add `linux` and remove one; the row shows the chips, the sidebar TAGS group counts it, clicking the tag filters the table, and the search box matches a tag. Tags survive a daemon restart (they live in `d.custom`).
- [ ] **Filename search (V3-12):** type a filename fragment that appears only inside a torrent's file list (not its name) in the filter box; rows appear once the server's index has reached them. Also check a hash fragment and a save-path fragment match locally.

## Bundled daemon (macOS)

- [ ] On a machine with no rtorrent on `PATH`, **Start rtorrent** launches the bundled 0.15.7 (log says `bundled`), writes a starter `~/.rtorrent.rc` if none exists, and the app connects.
- [ ] An existing `~/.rtorrent.rc` is left untouched; `RSTORRENT_RTORRENT_BIN` overrides which binary is used.
- [ ] Port range in Preferences applies to the bundled daemon (the pre-0.16.20 `network.port_range.set` name).

## Resilience

- [ ] With no daemon, the disconnected card shows the endpoint + retry countdown; starting rtorrent recovers automatically without an app restart.
- [ ] Native menu items (Preferences, Add File, Add Magnet, Statistics) open the right dialogs; Edit menu clipboard works in inputs.
