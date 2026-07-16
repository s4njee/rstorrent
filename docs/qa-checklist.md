# rstorrent manual QA checklist

Run this against a build in **both** modes:

- **Mock:** `RSTORRENT_MOCK=1 npm run tauri dev` (or a build with `mock: true`).
- **Live:** a real rtorrent per [rtorrent-setup.md](rtorrent-setup.md).

Mark each item ✅ / ❌ / n/a. File failures as new stories in `tasks.md`.

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
- [ ] Set location… opens a folder picker and warns files aren't moved.
- [ ] Copy magnet link puts a working magnet on the clipboard; Open destination reveals the data in Finder (localhost).

## Dialogs

- [ ] **Add torrent** (⌘O / menu): file picker → name/size/file-count, save path + Browse, label, options; tri-state file tree with select all/none and a live "selected" size; deselected files load at priority 0 (verify in Content tab).
- [ ] **Add magnet** (⌘⇧O / menu): validates magnet/URL; prefills from clipboard; adds on Add.
- [ ] **Remove** (⌫ / menu): shows name(s) + total size; delete-data checkbox disabled off-localhost; erase-only leaves data, delete moves files to Trash (recoverable).
- [ ] **Preferences** (⌘,): each section edits and Apply persists; Connection Test-connection reports the version; changing transport reconnects; Speed limits reflect in the status bar; port range / DHT apply; watched-folder path saves.
- [ ] **Statistics**: opens from the status-bar DHT segment; values populate; unavailable ones show —; all-time totals grow across sessions.
- [ ] All dialogs: Esc = Cancel, Enter = primary, ✕ = Cancel, backdrop blocks the window, focus trapped.

## Detail tabs

- [ ] General shows the label/value grid for the selected torrent.
- [ ] Trackers / Peers populate for an active torrent (live).
- [ ] Content lists files; clicking a priority cell cycles off/normal/high and sticks (live).
- [ ] Speed chart accumulates and scrolls; empty hint before enough samples.
- [ ] Log shows actions/errors; selected torrent's entries highlighted.

## Watched folder (live)

- [ ] With a watch folder set, dropping a `.torrent` into it adds the torrent and renames the file to `*.torrent.loaded`.

## Resilience

- [ ] With no daemon, the disconnected card shows the endpoint + retry countdown; starting rtorrent recovers automatically without an app restart.
- [ ] Native menu items (Preferences, Add File, Add Magnet, Statistics) open the right dialogs; Edit menu clipboard works in inputs.
