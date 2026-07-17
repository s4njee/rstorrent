# README screenshots

Four images, referenced from the top-level [README](../../README.md). Capture
with **⌘⇧4 then Space**, then click the window — that grabs the window alone,
with its shadow and no desktop behind it.

| File | What to have on screen |
|---|---|
| `main-window.png` | Main view: a few torrents in mixed states, one selected, sidebar visible |
| `pieces-bar.png` | A partially-downloaded torrent, General tab, pieces bar visible |
| `smart-filters.png` | Sidebar showing a saved smart filter, plus the selection bar (select 2+ rows) |
| `preferences-connection.png` | Preferences → Connection |

**Shoot these in mock mode** (`RSTORRENT_MOCK=1 npm run tauri dev`) unless you
have a reason not to. The ten fixtures cover more states than a real session
usually does, and nothing real ends up in a committed image.

Two things that do leak if you shoot a live session: torrent names, and the
endpoint in the Connection pane. The password field is safe — it renders as
"•••••••• (saved)" and never holds the secret — but a real URL and username in
`preferences-connection.png` are as public as this repo is. Use a stand-in like
`https://seedbox.example.com/RPC2`.

These are committed to git, so treat a replacement as permanent: the old image
stays in history even after it's overwritten.
