# rstorrent — port the "rTorrent Console" design to the web UI

Replace rstorrent's **Dark Ops** UI with the **rTorrent Console** design, and build
the two surfaces rstorrent does not have yet — **Settings** and **Disk & global
stats** — including the server data they need.

The design originates in [`blackbird`](https://github.com/local/blackbird), a Go +
SolidJS self-hosted rTorrent console, whose `design_handoff_rtorrent_console/`
bundle is a high-fidelity spec written to be re-implemented in another codebase:

> The task is to **recreate these designs in the target codebase's existing
> environment** (React, Vue, Svelte, etc.) using its established patterns,
> component library, routing, and state management.

So this is a re-implementation in rstorrent's React + Zustand + CSS-module tree and
its axum `rstorrent-web` server — not a transplant of blackbird's SolidJS or Go.

Decisions taken with the owner (2026-09-15):

1. **Scope = the five design surfaces.** Restyle the console and build Settings +
   Stats. Blackbird's extra views (attention/why, flight recorder, history,
   preservation, RSS, IP filter, scheduler, port check, storage forecast, unpack)
   are explicitly deferred — see WC11.
2. **The desktop React shell follows the new design.** One design system, one
   component set. The design is not a token swap — IBM Plex Sans instead of
   monospace, 30px rows, a 196px sidebar, a 288px tabbed detail panel with a facts
   rail — so the shared components are restyled and the desktop chrome is rebuilt
   on the same tokens.
3. **Full theming is in scope**: palette layer + semantic layer, five built-in
   themes (dark, light, midnight, contrast, classic), a per-theme accent picker,
   dense/comfortable density, and custom theme import.
4. **Docs**: this file and [web-console-tasks.md](web-console-tasks.md) own the
   port. `design/README.md`, `design/plan.md`, `design/tasks.md`,
   `design/README-desktop.md` and `design/rTorrent Web UI.dc.html` are the
   superseded Dark Ops record; the still-open WE6-S1 (Playwright) is carried into
   the new tracker as WC10-S1.

---

## 1. What we are porting, and what already exists

**The design is the authority for look, layout, tokens and interactions.**
Copy the bundle into the repo as `design/web-console/` (WC0-S1):

| File | Role |
|---|---|
| `design_handoff_rtorrent_console/README.md` | **Authoritative spec** — 344 lines: colour tokens, typography, fixed heights, the five surfaces frame by frame, interactions, state model |
| `design_handoff_rtorrent_console/rTorrent Console.dc.html` | The prototype; all sample data in `renderVals()` — the fastest way to see every field the UI expects |

Blackbird's implementation is the authority for behaviour the handoff leaves open:
the theming system (`web/src/styles/{palette,tokens}.css`, `themes/*.css`,
`lib/themes.ts`, `lib/theme.ts`, `lib/custom-themes.ts`, `internal/themefile`), the
settings-to-rTorrent-key mapping, and the stats composition. Copy the *decisions*,
not the code.

**Already in rstorrent — reuse, do not rebuild:**

| Need | Where |
|---|---|
| DTO contract + camelCase guard | `crates/rtorrent/src/types.rs`, `src/ipc/types.ts` |
| Poll with revision / ETag / 304 / 409-heal | `server/src/api.rs`, `poller.rs`, `state.rs`; `src/ipc/web.ts` |
| Disk free/total (unix statvfs) | `server/src/disk.rs` |
| Detail payloads (files, peers, trackers, pieces) | `GET /api/detail`; `rtorrent_core::types::{FileNode,PeerRow,TrackerRow,PieceInfo}` |
| Commands, incl. add-by-upload | `POST /api/cmd/{name}`, `POST /api/torrents/{inspect,file}` |
| Session auth | `POST/DELETE /api/session`, `server/src/auth.rs` |
| Settings **write** primitives | `RtorrentApi::{set_throttles,set_port_range,set_dht,apply_config,apply_config_str,set_custom_metadata}` |
| Tabular formatters | `src/utils/format.ts` |
| Virtualiser for 5k rows | `src/hooks/useVirtualizer.ts` |
| IBM Plex TTFs (to self-host from) | `crates/gpui/assets/IBMPlexSans-{Regular,Medium,SemiBold}.ttf` |

**The gaps the port must close:**

| Gap | Epic |
|---|---|
| No palette / density / theming layer | WC1 |
| No router (the design has console, settings and stats routes) | WC7-S1 |
| **No config *read* path** — `RtorrentApi` has no generic getter, so no surface can show a live daemon key | WC7-S2 |
| No settings write path over HTTP (`apply_settings` is web-rejected; `webSettings.ts` returns a placeholder) | WC7-S3/S4 |
| No stats: session totals, 60-minute rate history, volumes, per-label space | WC8 |
| No notices/toasts and no lost-connection banner | WC2-S5, WC9-S2 |
| Status vocabulary differs (rtorrent vs the design's Downloading/Seeding/Stopped/Queued/Checking N%/Tracker error) | WC3-S3 ✅ |
| Filtering is one dimension at a time, where the design intersects status AND label AND tracker AND text | WC3-S7 ✅ — the filter is a facet set |
| Label colours and saved filters belong to the deployment, not the daemon | The palette exists (WC4-S3 ✅); both wait on the settings endpoint — label colours in WC7-S5, saved filters moving off `localStorage` in the same story |
| Label colours do not exist (rtorrent holds one label string per torrent) | WC4-S3 |
| Add is two dialogs (file, magnet), not one modal with a segmented control | WC6 |

---

## 2. Target architecture

```
src/
  theme/
    palette.css        --pal-* raw values (dark = the handoff palette)
    themes.css         the four theme overrides, in one import
    themes/*.css       palette overrides: light, midnight, contrast, classic
    tokens.css         semantic aliases + accent derivations + type/height/spacing/radius/shadow/motion
    theme.ts           the registry, applyAccent, storage, the color-mix fallback
    global.css         reset, @font-face, tabular-nums, scrollbars, focus ring
  lib/router.ts        pushState router for /, /settings, /stats
  web/
    WebApp.tsx         the console route
    SettingsRoute.tsx  WC7
    StatsRoute.tsx     WC8
    Notices.tsx, LostConnectionBanner.tsx
  components/          shared with the desktop shell, restyled (WC3–WC6)
server/src/            api.rs, state.rs, poller.rs, config.rs
crates/rtorrent/       RtorrentApi gains config_get
```

**One design system for both shells.** `src/theme/tokens.css` stops being "Dark Ops"
and becomes the console's semantic layer; `tokens.web.css` shrinks to the few
metrics that genuinely differ in a browser (app-bar height, footer height). The
palette, type scale and heights are shared, so `src/App.tsx` renders the same
table, sidebar, detail panel and dialogs, and only its chrome (native title bar,
native menu bar, tray actions) stays desktop-specific.

**Live data stays HTTP.** We keep `/api/state` + `/api/delta` with ETag/304 and
409-heal, at the design's ~2s cadence for the visible set, paused while the tab is
hidden. Blackbird's WebSocket hub is **not** ported: the poll loop already exists,
carries a revision, heals on a miss, and works through the reverse proxies the web
UI is documented behind. WC9-S1 tunes cadence only.

**Theming mechanism** (from blackbird, worth copying exactly):

- A **palette layer** holds raw values (`--pal-bg-app: #101214`), one file per
  theme selected by `data-theme` on `<html>`. Nothing else may hold a literal;
  stylelint enforces hex and `rgb()` only in the palette layer and `themes/`.
- A **semantic layer** aliases them (`--bg-app: var(--pal-bg-app)`) and derives the
  accent's tints with `color-mix()`: `--accent-tint` (22% toward transparent — the
  selected row), `--accent-tint-strong` (30% — active sidebar item, chips, graph
  fill), `--accent-text` (55% toward `--accent-ink`), `--focus-ring` (45%). A JS
  fallback (`applyAccent`) covers browsers without `color-mix`.
- **Progress is never the accent**: `--progress-active` blue below 100%,
  `--progress-complete` green at 100%, `--state-disabled` when stopped.
- A theme is *a palette value set*, optionally overriding shadows and radii only.
- Custom themes are a validated JSON file — `name`, `description`, `extends`,
  `dark`, `accents[]`, `palette{}`, `preview{}`, `accent`, `density` — stored
  server-side, applied as `data-theme="custom-<id>"`, delivered as generated CSS.

---

## 3. Epics

Order is strict; each epic is demoable on its own. Milestones: **WCM0** foundation ·
**WCM1** shell + table · **WCM2** sidebar + detail panel · **WCM3** add + settings ·
**WCM4** stats + polish · **WCM5** gates.

| # | Epic | Milestone | Stories | Carries |
|---|---|---|---|---|
| **WC0** | Foundations: design bundle, fonts, baseline, fixtures, dev loop | WCM0 | 5 | — |
| **WC1** | Token and theme system | WCM0–WCM1 | 7 | — |
| **WC2** | Shell and chrome | WCM1 | 7 | — |
| **WC3** | Torrent table | WCM1–WCM2 | 7 | ✅ |
| **WC4** | Sidebar | WCM2 | 6 | ✅ |
| **WC5** | Detail panel | WCM2–WCM3 | 8 | ✅ |
| **WC6** | Add torrent modal | WCM3 | 5 | — |
| **WC7** | Settings | WCM3–WCM4 | 7 | — |
| **WC8** | Disk and global stats | WCM4 | 6 | — |
| **WC9** | Live data, actions and states | WCM4 | 5 | — |
| **WC10** | Quality gates | WCM5 | 5 | WE6-S1 (Playwright) |
| **WC11** | Deferred — explicitly not ported | — | — | — |

Story detail, acceptance criteria and verification commands are in
[web-console-tasks.md](web-console-tasks.md).

**Load-bearing stories** (the ones that block or shape everything after them):

- **WC1-S1/S2** — the palette and semantic layers. Land them together, before any
  surface work, or the two designs coexist indefinitely.
- **WC3-S3** — the cell and status vocabulary. One tested mapping function decides
  how every row reads.
- **WC7-S2** — the config read path. Without it the Settings surface cannot show
  honest values, and its rows are key-driven.
- **WC8-S2** — server-side stats. The Stats surface has no data source today.

---

## 4. Files

**New**
```
design/web-console/{README.md, rTorrent Console.dc.html, support.js}
docs/web-console-plan.md, docs/web-console-tasks.md
src/theme/palette.css, src/theme/themes/{light,midnight,contrast,classic}.css
src/lib/{theme.ts, themes.ts, router.ts}
src/web/{SettingsRoute,StatsRoute,Notices,LostConnectionBanner}.tsx
src/components/stats/{StatCards,ThroughputChart,VolumesPanel,LabelUsage}.tsx
src/components/settings/{SettingsNav,SettingRow,ThemePicker,DensityToggle}.tsx
public/fonts/IBMPlexSans-{Regular,Medium,SemiBold}.woff2
scripts/check-contrast.mjs
```

**Changed**
```
src/theme/tokens.css                     rewritten as the semantic layer
src/theme/global.css                     reset, @font-face, tabular-nums, focus ring
src/theme/tokens.web.css                 shrinks to browser-only metrics
src/web/{WebApp,AppBar,ActionStrip,Footer,DiskCard,LoginScreen}.tsx
src/components/table/{TorrentTable,Row,columns.ts,ProgressBar}.*
src/components/sidebar/FilterSidebar.*
src/components/details/{DetailTabs,PieceBar,AvailabilityBar,SpeedChart}.*
src/components/menu/ContextMenu.*, src/components/dialogs/{ModalBase,AddTorrentDialog,AddMagnetDialog,PreferencesDialog,StatisticsDialog,...}.*
src/store/{ui,settings,selectors,torrents}.ts
src/ipc/{types,commands,web,webSettings}.ts
src/utils/format.ts                      the design's date and ETA formats
server/src/{api,state,poller,config}.rs
crates/rtorrent/src/rtorrent/mod.rs      config_get on the trait (+ client + mock)
docs/web-setup.md, docs/qa-checklist.md, README.md
```

**Superseded (kept as the record, marked in place)**
```
design/README.md, design/plan.md, design/tasks.md, design/README-desktop.md,
design/rTorrent Web UI.dc.html
```

---

## 5. Verification

| Layer | Command / check |
|---|---|
| Types | `npm run typecheck` |
| Lint (incl. the token rule) | `npm run lint` |
| Frontend units | `npm test` — the pre-port 108 plus new theme/table/settings tests |
| Server | `cargo test -p rstorrent-web`; `cargo test` for the workspace |
| Rust lints | `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` |
| Contrast | `node scripts/check-contrast.mjs` |
| E2E | `npm run e2e` (new in WC10-S1) |
| Visual | both themes at 1600px against `design/web-console/rTorrent Console.dc.html` |
| Live daemon | a real rtorrent: every Settings key reads and writes, add by magnet and by file, stats match the daemon |
| Desktop | `RSTORRENT_MOCK=1 npm run tauri dev` renders the new design with no regression |

Definition of done per story (the repo's convention): clippy and `tsc --noEmit`
clean, new pure logic unit-tested, no hard-coded hex outside the palette layer,
works in mock mode, and the desktop app stays green.

---

## 6. Risks and open questions

| Risk | Mitigation |
|---|---|
| **The restyle touches the shared desktop shell**, which has no visual test coverage | WC0-S3 baseline; every story keeps it green; the Playwright suite covers the shared console |
| **`config_get` changes the shared crate** used by three hosts | Additive trait method with a default "unsupported per key"; no exhaustive matches exist on the trait |
| **Older rtorrent builds expose fewer keys** | WC7-S2 reports per key; a row shows "not exposed by this build" rather than a wrong value |
| **The design's `Queued` and `Checking 42%` have no direct rtorrent state** | **Closed in WC3-S3.** One tested mapping owns the vocabulary; `checking` reads `percent` (hashed chunks), and `TorrentDto` gained `isOpen`/`isActive` so `Stopped` and `Queued` are distinguishable — `Status::Paused` alone could not tell them apart |
| **Volumes and label colours are app-owned**, not daemon knowledge | Documented as configuration in `docs/web-setup.md`; label colours default to the label palette |
| **Fonts add bundle weight** | Three weights, woff2, preloaded; measured in WC10-S5 |
| **Two designs coexist mid-port** | WC1 lands before WC2, so the flip is atomic per surface |
| **Scope creep toward blackbird's extras** | WC11 is an explicit no; a new idea needs its own plan |

**Open questions for the owner before WC7 starts:**

1. Should the Settings surface write straight to the daemon, or also keep a
   client-side desired state (the desktop holds `settings.json`; the design implies
   the daemon is the truth)?
2. Is `Queued` a state we model, or a label we drop?
3. Do we retire the desktop React shell once GPUI reaches parity (see
   [`../GPUI.md`](../GPUI.md))? If so, WC3–WC6 can stop carrying desktop-only
   concerns.

---

## 7. Where to start

1. **WC0-S1/S2** — copy the design bundle into `design/web-console/`, wire the
   self-hosted fonts, and mark the old design docs superseded.
2. **WC0-S3/S4** — record the regression baseline and expand the mock to the
   handoff's 14 sample torrents.
3. **WC1-S1/S2** — land the palette and semantic layers behind the existing
   components, so colours change while every running view keeps working. Then work
   epic by epic in order.
