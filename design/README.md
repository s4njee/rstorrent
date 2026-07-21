# Handoff: rtorrent Web UI — "Dark Ops"

## Overview
A **browser-based web UI** for the `rtorrent` daemon (the ruTorrent/Flood category), sharing
the "dark ops" visual language of the companion desktop design. Single-page app, full
viewport, responsive column: app bar → (filter sidebar + torrent table + detail panel) →
status footer. Density is compact power-user (25px rows, 11.5px base, all monospace).

## About the Design Files
The file in this bundle is a **design reference created in HTML** — a static prototype
showing the intended look and behavior, **not production code to copy directly**. The task
is to **recreate this design in the target codebase's existing environment** (React, Vue,
Svelte, htmx…) using its established components, state, and styling patterns. If no
environment exists yet, choose a stack suited to a long-lived polling SPA. The backend is
the `rtorrent` XML-RPC/SCGI interface, typically reverse-proxied behind the same origin.

## Fidelity
**High-fidelity.** Colors, typography, spacing, and layout are final and specified below.
The prototype is static — implement real behavior per "Interactions & Behavior".

---

## Design Tokens

### Color
| Token | Hex | Use |
|---|---|---|
| `bg/page` | `#0f1114` | Page background, inputs |
| `bg/surface` | `#14161a` | Sidebar, table body, detail-tab body |
| `bg/chrome` | `#191c21` | App bar, table header, detail panel chrome, footer |
| `bg/row-alt` | `#171a1f` | Odd table rows (zebra); sidebar disk card |
| `bg/selected` | `#1d2b33` | Selected row, active nav, primary-button fill |
| `bg/raised` | `#22262d` | Progress track, secondary-button fill |
| `bg/row-hover` | `#1a2026` | Table row hover |
| `border/black` | `#000000` | App bar bottom edge, footer top edge |
| `border/strong` | `#2a2f38` | Input/button borders, vertical separators |
| `border/mid` | `#23272e` | Panel dividers, sidebar right edge |
| `border/row` | `#1a1d22` | Table row dividers |
| `text/primary` | `#d6dae2` | Torrent names, wordmark |
| `text/body` | `#c8cdd6` | Default text |
| `text/muted` | `#8b93a2` | Secondary values, inactive nav, icons |
| `text/dim` | `#565e6b` | Column headers, counts, captions, footer |
| `accent/cyan` | `#58c4dd` | Primary accent: borders, active edge, DL bar, disk gauge |
| `accent/cyan-bright` | `#8fdcee` | Active text, download speed, primary-button label |
| `accent/green` | `#57d597` | Seeding bar, "connected" dot |
| `accent/green-soft` | `#7fd8a4` | Upload speed, connected label, completed dot |
| `accent/amber` | `#d5a04c` | Stalled |
| `accent/red` | `#e05d5d` | Error |
| paused gray | `#4a515c` | Paused bar/dot |

### Typography
- **Family:** `ui-monospace, SFMono-Regular, Menlo, monospace` for everything.
- **Base:** 11.5px. Table numerics 10.5px. Column headers/captions 10px (or 9.5px in
  sidebar) weight 600–700, uppercase, `letter-spacing:.05em–.08em`.
- Wordmark: `rtorrent / web` — 12.5px/700 in `text/primary`, "/ web" in `text/dim` 400.

### Layout & metrics
- **App bar:** 46px tall, `bg/chrome`, 16px side padding, 1px black bottom border.
- **Sidebar:** 186px fixed, `bg/surface`, scrolls independently.
- **Table rows:** 25px; header sticky (`position:sticky;top:0`, `bg/chrome`, z-index above rows).
- **Detail panel:** pinned below the table (flex column: table flexes, panel is `flex:none`).
- **Footer:** 26px, `bg/chrome`.
- Radii: buttons/inputs 5px, nav items 4px, progress bar 1px, disk card 6px.
- Separators: 1px `border/strong`, 16–20px tall verticals.

### Iconography
Inline SVG line glyphs, 1.3–1.8px stroke, drawn in `text/muted` (primary Add icon in
`cyan-bright`). Recreate with the codebase's icon set at ~11–14px.

---

## Screen Anatomy (single view)

### App bar (top, 46px)
Left→right: logo mark (22px square, `bg/selected` fill, cyan border, "r") + wordmark ·
separator · **Add** button (primary: cyan border, `bg/selected`, cyan-bright text) +
**Magnet** button (secondary: `border/strong`, `bg/raised`) · spacer · search input
(240px, placeholder `/ search torrents`) · live speeds `↓` cyan-bright / `↑` green-soft
(10.5px) · separator · connection status (7px green dot + "connected") · settings icon
button (26px) · avatar circle (24px, initials).

### Filter sidebar (186px)
Three uppercase-dim group headers:
- **Status** — all (active) / downloading / seeding / completed / paused / stalled / error.
  Each row: 7px colored dot (cyan, cyan, green, green-soft, gray, amber, red) + name +
  right-aligned dim count. Active row = `bg/selected` fill, 2px cyan left edge, cyan-bright
  text, 4px radius.
- **Labels** — `#` prefix glyph + name + count (linux-iso 6, video 3, sbc 1).
- **Trackers** — elided hostnames + counts.
At bottom: **Disk card** (`bg/row-alt`, `border/mid`, 6px radius): "DISK / 412 GiB free"
caption row + 5px cyan usage bar (63%).

### Action strip (above table)
Icon buttons: Resume ▶, Pause ⏸, Remove (trash) · separator · queue up/down arrows.
Right-aligned dim caption `1 of 10 selected`.

### Torrent table
12 columns, CSS grid `minmax(220px,1fr) 70 100 90 52 52 80 80 66 50 78 minmax(90px,120px)`:
Name ▾ · Size · Done · Status · S · P · Down · Up · ETA · Ratio · Label · Tracker.
- Sticky header row, 10px uppercase dim; sort indicator `▾` on active column.
- Rows 25px, zebra `bg/surface`/`bg/row-alt`, hover `bg/row-hover`, selected `bg/selected`.
- **Done** = 8px bar (track `bg/raised`; fill by status: DL cyan, seeding green, paused
  gray, stalled amber, error red) + right-aligned dim percent (9.5px).
- **Status** = lowercase colored text (same status palette). **Down** cyan-bright, **Up**
  green-soft, other numerics `text/muted` right-aligned.
- Table area scrolls; header stays pinned.

### Detail panel (bottom, pinned)
Tab strip on `bg/chrome`: general (active, cyan-bright + 2px cyan underline) · trackers ·
peers · content · speed · log (dim); right-aligned dim filename of the selected torrent.
Body (`bg/surface`): 4-column grid of `label: value` pairs — active, down, up, ratio, eta,
conns, dl-limit, ul-limit.

### Footer (26px)
Dim 10.5px: `rtorrent 0.9.8 · SCGI @ 127.0.0.1:5000` · `dht: 387 nodes` · spacer ·
`10 torrents` · `↓ 9.5 MiB/s` (cyan-bright) · `↑ 2.6 MiB/s` (green-soft).

---

## Interactions & Behavior
- **Add / Magnet** open modal dialogs (reuse the desktop handoff's add-torrent and
  add-magnet dialog specs if available; same visual language — `bg/chrome` header/footer,
  primary cyan Add button).
- **Search** filters the table live; `/` keyboard shortcut focuses it.
- **Sidebar filters** are exclusive within a group; clicking filters the table and moves the
  active treatment; counts update live.
- **Row selection:** click selects; ctrl/cmd + shift multi-select; selection count in the
  action strip; selection drives the detail panel and action strip buttons.
- **Column sort:** click header toggles asc/desc, `▾/▴` indicator.
- **Right-click row** → context menu (resume, pause, force recheck, set label ▸, set
  location, copy magnet, open destination, remove — destructive styling for remove).
- **Detail tabs** switch panel content for the selected torrent.
- **Connection status:** green dot = SCGI reachable; turn dot/label red (`#e05d5d`,
  "disconnected") and disable mutating actions when polling fails.
- **Live updates:** poll every 1–2s; update speeds, bars, ETA, counts, footer totals, disk
  gauge. Consider a visibilitychange pause when the tab is hidden.

## State Management
- `torrents[]`: id, name, size, bytesDone, percent, status enum
  (`downloading|seeding|completed|paused|stalled|error`), seeds, peers, downRate, upRate,
  eta, ratio, label, tracker, savePath, queuePosition.
- `selection: Set<id>` · `filter: {type:'status'|'label'|'tracker', value}` ·
  `searchQuery` · `sort: {column, dir}` · `activeDetailTab` · `connection: 'ok'|'down'`.
- Derived: filtered+sorted rows, sidebar counts, aggregate speeds, disk usage.
- Data source: rtorrent XML-RPC over SCGI (`d.multicall2` list polling; `load.start`,
  `d.start`, `d.stop`, `d.erase`, `d.directory.set`, `d.priority` for actions), proxied
  server-side; the browser talks JSON to a thin API layer.

## Assets
No image assets; icons are inline SVG line glyphs (swap for the codebase icon set). System
monospace stack only — no web fonts.

## Files
- `rTorrent Web UI.dc.html` — **the design.** Full-viewport web UI, primary reference.
- Companion desktop package `design_handoff_rtorrent_client/` (separate) documents the
  add/magnet/preferences/statistics/context-menu/remove dialogs; the web app reuses those
  dialog designs re-skinned onto this page's modal layer.
