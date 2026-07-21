# Handoff: rtorrent Desktop Client — "Dark Ops" (1c)

## Overview
A desktop BitTorrent client GUI in the mold of qBittorrent, front-ending the `rtorrent`
daemon. This package covers the complete main window plus every secondary window and modal:
add-torrent (from file), add-magnet, preferences, statistics, the row right-click context
menu, and the remove-confirmation dialog. Density is **compact power-user** (23px table
rows, 11.5px base type, monospace numerals).

## About the Design Files
The files in this bundle are **design references created in HTML** — a prototype showing the
intended look and behavior, **not production code to copy directly**. The task is to
**recreate these designs in the target codebase's existing environment** (Electron/React,
Tauri, Qt/QML, GTK, SwiftUI, etc.) using its established components, state, and styling
patterns. If no environment exists yet, choose the framework best suited to a cross-platform
desktop torrent client and implement the designs there. `rtorrent` itself is controlled over
its XML-RPC / SCGI interface — the UI is a client of that API.

## Fidelity
**High-fidelity (hifi).** Final colors, typography, spacing, and layout are specified below
and should be reproduced closely. Interactions are described but the prototype is static —
implement real behavior per the "Interactions & Behavior" and "State Management" sections.

---

## Design Tokens

### Color
| Token | Hex | Use |
|---|---|---|
| `bg/app` | `#14161a` | Main app background, table body |
| `bg/panel` | `#191c21` | Title bar, toolbar, sidebar, footer, tab bar, dialog headers/footers |
| `bg/field` | `#0f1114` | Text inputs, textareas, dropdowns |
| `bg/row-alt` | `#171a1f` | Odd table rows (zebra) |
| `bg/selected` | `#1d2b33` | Selected row, active nav, checked checkbox fill |
| `bg/track` | `#22262d` | Progress-bar track; secondary button fill |
| `border/black` | `#000000` | Window outer border, dialog header/footer dividers |
| `border/strong` | `#2a2f38` | Input borders, toolbar separators |
| `border/mid` | `#23272e` | Panel/section dividers |
| `border/row` | `#1a1d22` | Table row dividers |
| `text/primary` | `#d6dae2` | Torrent names, dialog body headings |
| `text/body` | `#c8cdd6` | Default body text, labels |
| `text/muted` | `#8b93a2` | Secondary values, inactive nav |
| `text/dim` | `#565e6b` | Column headers, captions, tertiary values, inactive tabs |
| `accent/cyan` | `#58c4dd` | Primary accent: add icon, active borders, checkbox stroke, DL bar |
| `accent/cyan-bright` | `#8fdcee` | Active tab/nav text, download speed, primary-button text |
| `accent/green` | `#57d597` | Seeding bar |
| `accent/green-soft` | `#7fd8a4` | Upload speed, good ratio |
| `accent/amber` | `#d5a04c` | Stalled status/bar |
| `accent/red` | `#e05d5d` | Error status/bar, destructive action |
| `danger/bg` | `#3a1f1f` | Destructive button fill, warning icon bg |
| `danger/border` | `#6b2f2f` | Destructive button border |
| traffic lights | `#f45c53` / `#f5b435` / `#3fc24d` | Window close/min/max |

### Typography
- **Family:** `ui-monospace, SFMono-Regular, Menlo, monospace` throughout (this is the
  defining trait of the "dark ops" look — everything, including labels and body text, is
  monospaced).
- **Base:** 11.5px. **Table cells / numerals:** 10.5px. **Column headers & captions:** 10px,
  weight 600–700, `text-transform:uppercase`, `letter-spacing:.05em–.08em`.
- **Window titles:** 11px weight 600. **Dialog body heading:** 11.5px, `#d6dae2`.
- Numeric columns use tabular figures; monospace gives this for free.

### Spacing / radius / shadow
- Row height (table): **23px**. Toolbar buttons: **26×24px**. Input padding: `5px 9px`.
- Window radius **9px**; buttons/inputs **4–5px**; progress bar **1px**; status pills n/a
  (this theme uses plain colored text for status, not pills).
- Window shadow: `0 12px 40px rgba(0,0,0,.55)`.
- Toolbar/section separators: 1px vertical `#2a2f38`, 16px tall, `margin:0 5px`.

### Iconography
Icons are simple inline SVG line glyphs (1.6px stroke) drawn in `text/muted` `#8b93a2`,
except the primary **Add** (`+`) which is `accent/cyan`. In menus/prefs some emoji glyphs
are used as placeholders (🏷 📁 🔗) — **replace with the codebase's real icon set.**

---

## Screens / Views

### 1. Main Window
**Purpose:** monitor and control all transfers.
**Layout (top→bottom):** title bar → toolbar → body (`flex`, 430px tall) → detail tab strip
→ status bar. Body = 150px fixed filter sidebar (`border-right #23272e`) + flexible torrent
table.

- **Title bar** (`bg/panel`, 8×12px pad): macOS traffic lights left, centered title
  `rtorrent 0.9.8 · 10 torrents` in `text/dim`.
- **Toolbar** (`bg/panel`, `border-bottom #0b0d10`, 5×8px pad, `gap:2px`): icon buttons —
  Add file (cyan +), Add magnet, Remove | Resume ▶, Pause ⏸ | Move-up, Move-down. Right-
  aligned filter input (190px, `bg/field`, placeholder `/ filter`).
- **Sidebar:** three uppercase-dim group headers — **Status** (all / downloading / seeding /
  completed / paused / stalled / error, with counts), **Labels** (linux-iso / video / sbc),
  **Trackers** (elided hostnames). Active item = `bg/selected` fill + 2px `accent/cyan`
  left border + `cyan-bright` text. Counts in `text/dim` right-aligned.
- **Torrent table:** 12 columns via CSS grid
  `minmax(200px,1fr) 70 92 84 52 52 76 76 62 46 72 110`:
  Name · Size · Done · Status · S · P · Down · Up · ETA · Ratio · Label · Tracker.
  Header row: 10px uppercase `text/dim`. Data rows 23px, zebra (`#14161a`/`#171a1f`),
  selected row `bg/selected`, divider `#1a1d22`.
  - **Name** `text/primary`, ellipsized. **Done** = 8px progress bar, track `#22262d`,
    fill colored by status (DL cyan, seeding green, paused `#4a515c`, stalled amber,
    error red). **Status** = lowercase colored text (no pill). **Down** cyan-bright,
    **Up** green-soft. Other numerics `text/muted`, right-aligned, 10.5px.
- **Detail tabs** (`bg/panel`): general (active, `cyan-bright` + 2px cyan underline),
  trackers / peers / content / speed / log (`text/dim`). Active tab body = 4-col grid of
  `label: value` pairs (active, down, up, ratio, eta, conns, dl-limit, ul-limit) on
  `bg/app`.
- **Status bar** (`bg/panel`, `text/dim`, 10.5px): `dht: 387 nodes` … right side
  `↓ 9.5 MiB/s` (cyan-bright) · `↑ 2.6 MiB/s` (green-soft) · `free: 412 GiB`.

### 2. Add Torrent (from .torrent file) — 620px
Header `Add torrent` + ✕. Body rows: **Torrent** (filename + `2.3 GiB · 587 files` meta),
**Save to** (input + Browse…), **Label** (dropdown) + "Rename torrent" checkbox. Divider,
then a 2×2 checkbox grid: **Start torrent** (checked), Sequential download, Skip hash check,
Add to top of queue. **Contents** box: header (`Contents` / `select all · none`) + a file
tree with tri-state checkboxes, folder/file rows (indented 34px for children) and right-
aligned sizes. Footer (`bg/panel`): **Cancel** (secondary) + **Add** (primary: cyan border,
`bg/selected` fill, `cyan-bright` text).

### 3. Add Magnet / URL — 460px
Header `Add magnet link` + ✕. Body: multiline **Magnet URI or torrent URL** textarea (64px,
magnet text shown in `cyan-bright` 10.5px), **Save to** input, **Label** dropdown, and a row
of two checkboxes (Start torrent checked, Add to top of queue). Same Cancel/Add footer.

### 4. Preferences — 860px, 420px body
Title bar with traffic lights, centered `Preferences`. **Left nav** (170px, `bg/panel`):
Behavior ⚙ · Downloads ⬇ (active) · Connection ⇄ · Speed ⏱ · BitTorrent ⦿ · RSS ⤳ ·
Web UI ⧉ · Advanced ⚑. Active = `bg/selected` + 2px cyan left border + cyan-bright text.
**Right panel** shows the Downloads section: three sub-groups with uppercase-dim headers —
*Saving Management* (Default save path + Keep incomplete in… each with input+Browse),
*When Adding a Torrent* (Do-not-start checkbox, Show-dialog checkbox checked, content-layout
dropdown), *Watched Folder* (auto-load checkbox + path). Footer: Cancel + **Apply** (primary).

### 5. Statistics — 400px
Header `Statistics` + ✕. Two labeled groups of `key … value` rows (each row
`justify-content:space-between`, 1px `#1a1d22` divider): **User Statistics** (all-time
download, all-time upload, all-time share ratio [green], session waste, connected peers) and
**Cache Statistics** (read cache hits, total buffer size, write cache overload, queued I/O).

### 6. Right-click Context Menu — 216px
Floating `bg/panel` menu, 5×0 pad. Items (5×14px, `gap:9px`, 14px icon column in
`text/muted`, label in `text/body`, optional right `▸` submenu arrow in `text/dim`): Resume,
Pause | Force recheck, Set label ▸, Set location… | Copy magnet link, Open destination |
**Remove** (red icon + `#e0a5a5` label). Separators = 1px `#2a2f38`, `margin:5px 8px`. Hover
state: row bg → `bg/selected`.

### 7. Remove Confirmation — 400px
Header `Remove torrent` + ✕. Body: round warning badge (`danger/bg` fill, `danger/border`
ring, red `!`) + message "Remove **{name}** from the transfer list?" and a checkbox "Also
delete downloaded files ({size})" styled in the danger palette. Footer: Cancel + **Remove**
(destructive: `danger/border`, `danger/bg`, red text).

---

## Interactions & Behavior
- **Toolbar:** Add-file opens screen 2; Add-magnet opens screen 3; Remove (with selection)
  opens screen 7; Resume/Pause act on the selection; Move-up/down reorder the rtorrent queue
  (`d.priority` / queue position). Preferences opens screen 4.
- **Row selection:** single click selects (row → `bg/selected`); ctrl/cmd + shift multi-
  select; selection drives the detail tabs, toolbar actions, and context menu.
- **Right-click** a row → screen 6 at cursor. "Set label ▸" opens a submenu of existing
  labels + "New…". "Remove" → screen 7.
- **Filter sidebar:** clicking a Status/Label/Tracker filters the table to matching torrents
  and moves the active highlight; counts update live.
- **Detail tabs:** switch the panel content for the selected torrent (general/trackers/peers/
  content/speed/log).
- **Dialogs:** modal, dim + block the window behind; Esc = Cancel, Enter = primary; ✕ =
  Cancel. Add-torrent file tree checkboxes are tri-state (folder reflects children); toggling
  files updates the total selected size.
- **Live updates:** speeds, progress bars, ETA, peer/seed counts, and status-bar totals
  refresh on a poll interval (rtorrent XML-RPC, ~1–2s). Progress bar fill width = percent
  done; color follows status.

## State Management
- `torrents[]`: id, name, size, bytesDone, percent, status enum
  (`downloading|seeding|completed|paused|stalled|error`), seeds/peers (connected + swarm),
  downRate, upRate, eta, ratio, label, tracker, savePath, queuePosition.
- `selection: Set<id>`, `activeFilter: {type:'status'|'label'|'tracker', value}`,
  `activeDetailTab`, `sortColumn/sortDir`.
- Dialog state: `dialog: null | 'add-file' | 'add-magnet' | 'prefs' | 'stats' | 'remove'`
  plus the in-progress form model (save path, label, option flags, file-tree selection).
- Derived: sidebar counts, filtered+sorted row list, aggregate down/up totals for the status
  bar.
- Data source: poll rtorrent over XML-RPC/SCGI (`d.multicall2` for the list; `d.start`,
  `d.stop`, `d.erase`, `d.priority`, `d.directory.set`, `load.start`, etc. for actions).

## Assets
No external image assets. Toolbar/status icons are inline SVG line glyphs (recreate as the
codebase's icon components). A few menu/prefs glyphs use emoji placeholders (🏷 📁 🔗 and the
nav symbols) — **swap for the real icon set.** Fonts: system monospace stack only, no web
fonts to bundle.

## Files
- `rTorrent Client 1c.dc.html` — **the chosen design (1c).** All seven windows/modals in one
  canvas. Primary reference for implementation.
- `rTorrent Client.dc.html` — earlier exploration with three visual flavors (1a classic gray,
  1b modern light, 1c dark). Included for context on rejected directions only.
