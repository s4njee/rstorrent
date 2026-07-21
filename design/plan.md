# rstorrent Web UI — Implementation Plan

A **browser-based web UI** for the `rtorrent` daemon (the ruTorrent/Flood category), implementing the **"Dark Ops" web** high-fidelity design specified in [`design/README.md`](README.md). It is a sibling of the shipped desktop app: same visual language, same rtorrent plumbing, delivered as a single self-hosted server binary you point a browser at.

This document explains *what we're building and how*. Treat `design/README.md` as the authoritative visual spec; the desktop [`plan.md`](../plan.md) remains the architecture doc for the Tauri app. The companion [tasks.md](tasks.md) breaks this plan into epics and stories (ids `WE#-S#`) for execution.

---

## 1. Goal & scope

**Goal:** the existing rstorrent UI, reachable from any browser — a qBittorrent-density, monospace, dark-ops SPA served by a small self-hosted server that proxies rtorrent's XML-RPC/SCGI interface as JSON. Typical deployment: the server runs *next to* the daemon on a seedbox or home server; the browser talks only to the server, never to SCGI.

**v1 scope (exactly the web handoff):**

- Single view: app bar (logo/wordmark, Add + Magnet, search, live speeds, connection dot, settings, avatar) → filter sidebar (status/labels/trackers + disk card) → action strip → 12-column torrent table → pinned detail panel (general/trackers/peers/content/speed/log) → status footer.
- Dialogs: Add torrent (browser file upload + drag & drop, with the file tree), Add magnet, Remove confirmation, row context menu — the desktop dialog designs re-skinned onto the web modal layer.
- Live updates: the server polls rtorrent at ~1 s; the browser polls the server at 1–2 s (paused when the tab is hidden); speeds, bars, ETA, counts, disk gauge all refresh.
- Actions: add (upload/magnet), resume, pause, force recheck, remove (± data when co-located), set label, set location, copy magnet, queue move, per-file priority, tracker add/remove/enable/reannounce, peer ban/snub/disconnect.
- Auth: single-user password login, session cookie, login screen in the same design language.
- Mock mode (`RSTORRENT_MOCK=1`) so the whole web UI runs with no daemon — and, for the first time, real browser e2e tests.

**Non-goals for v1:**

- Multi-user accounts, roles, or per-user state. One password, one shared view.
- A mobile/responsive layout. The design is desktop-density; declare ~1000 px minimum width and revisit in v2.
- Web-managed preferences, RSS, watch folders, turtle scheduling, connection profiles, or the 1 Gbps tuner. Those remain desktop/daemon-side features; the web settings affordance is minimal (see §7.5).
- Built-in TLS termination (document a reverse proxy; optional rustls listener is v2).
- WebSocket push (v1 is HTTP polling with ETags; push is a v2 upgrade behind the same adapter).

This realizes backlog item **B19 · Web UI (L)**.

---

## 2. Reference design

| File | Role |
|---|---|
| [`design/README.md`](README.md) | **Authoritative spec**: tokens, typography, metrics, screen anatomy, interactions, state model. |
| `design/rTorrent Web UI.dc.html` | **The hi-fi prototype** — the full single view, with the ten fixture rows in its inline data block. It confirms the README's metrics (46/186/25/26 px, the exact column template) and pins details the text leaves open (list below). Like the desktop `.dc.html` files it loads the design tool's `support.js` (not vendored in the repo), so the `sc-for` row templates don't render standalone — inspect the static chrome in a browser and read row/fixture data from the script block. |
| `design/rTorrent Client 1c.dc.html` | Desktop hi-fi design. Reference for the **shared dialog designs** (add/magnet/remove/context menu) the web app re-skins, and for table/sidebar/detail styling that carries over. |

> **Repo hygiene note (done, WE0-S5):** the web handoff replaced the desktop handoff at `design/README.md`, so the desktop spec has been restored from git history as `design/README-desktop.md` and root `plan.md` §2 now points at it. Both plans have a live authority; the web `design/README.md` is untouched.

Key fidelity requirements, where the web design **differs from the shipped desktop UI** (everything else — tokens, monospace stack, zebra rows, flat status text, 8 px status-colored bars — carries over unchanged):

- **Row height 25 px** (desktop is 23 px) and table numerics 10.5 px.
- **Column template** `minmax(220px,1fr) 70 100 90 52 52 80 80 66 50 78 minmax(90px,120px)` for Name · Size · Done · Status · S · P · Down · Up · ETA · Ratio · Label · Tracker.
- **App bar (46 px)** replaces the title bar + toolbar: no traffic lights, no native menus; Add/Magnet as labeled buttons; a 240 px search input (`/` focuses it); live ↓/↑ speeds; connection dot; settings icon; avatar circle with initials.
- **Wordmark** `rtorrent / web` — 12.5 px/700, "/ web" in `text/dim`.
- **Sidebar 186 px** with the **disk card** pinned at the bottom (caption row + 5 px cyan usage bar).
- **Action strip** above the table: Resume / Pause / Remove · separator · queue up/down · right-aligned `n of m selected`.
- **Footer (26 px)**: `rtorrent {version} · {transport} @ {endpoint}` · `dht: N nodes` · spacer · `N torrents` · ↓/↑ totals.
- Radii: buttons/inputs 5 px, nav 4 px, progress 1 px, disk card 6 px.

Details pinned by the prototype (beyond the README text):

- Minimum viewport ~**1000 × 640** (the prototype sets `min-height: 640px`; the column template needs ~986 px before the Name/Tracker columns flex).
- **Error rows show a short lowercase error text** in the Status column (`trk error`), not the bare word "error" — derive it from `statusMsg` (tracker error → `trk error`, storage → `disk error`, fallback `error`).
- Seeding rows show ETA `∞`; paused rows show `—` across S/P/Down/Up/ETA.
- The detail strip's right-aligned dim filename tracks the selected row (which drops its zebra for `bg/selected`).
- Avatar initials render lowercase (`op` in the prototype); the disk usage bar is 5 px tall with a 3 px radius.
- The **general detail tab is a minimal 4×2 `label: value` grid** — `active · down · up · ratio` / `eta · conns · dl-limit · ul-limit`, limits rendered compact (`∞`, `5.0M`) — with **no pieces bar** (see §6.3).

### Deliberate deviations (design ↔ web reality)

| Handoff says | We ship | Why |
|---|---|---|
| Context menu has **open destination** | Replaced by **Copy path** (copies `savePath` to the clipboard) | A browser cannot reveal files on the server's filesystem; "open on the server" is meaningless. |
| Status enum has 6 values | `checking` appears as a 7th status text/bar state (cyan, like the desktop) during rechecks | rtorrent reports hashing; hiding it would show stale "paused". No sidebar row for it — `all` covers it. |
| Queue up/down arrows | Kept, mapped to `d.priority` steps with the honest "priority" tooltip | Same rtorrent limitation the desktop documents (root plan §10). |
| Settings icon (unspecified behavior) | v1 opens a read-only **Status** modal (daemon version/endpoint/health, server version, sign-out) | There are no meaningful browser-side settings yet; server config is server-side. Full prefs = v2. |
| Avatar circle, initials | Initials from server config `ui.display_name`; clicking opens a one-item menu: **Sign out** | Auth needs a logout affordance; the avatar is the natural home. |

---

## 3. Strategy: one frontend, two shells, three backends

**Decision: reuse the existing React frontend, not a fresh SPA.** The handoff itself instructs this ("recreate this design in the target codebase's existing environment … using its established components, state, and styling patterns"), and the codebase is unusually well positioned:

- Every Tauri call is funneled through **two files** — `src/ipc/commands.ts` (typed `invoke` wrappers) and `src/ipc/events.ts` (typed `listen` wrappers). Swap those internals and the entire component tree, stores, selectors, formatters, dialogs, and context menu run against any backend.
- The **browser demo already proves it**: `demo.html` + `src/demo/main.tsx` mount the real `<App/>` in a plain browser over `mockIPC`. The web app is the same trick with a real HTTP backend and a different shell.
- The Rust side already contains a complete, battle-tested rtorrent client (`src-tauri/src/rtorrent/`: SCGI framing, the `<i8>` XML-RPC dialect, HTTP(S) transport, typed client, status derivation, mock) — and it has **zero Tauri dependencies**, so it extracts cleanly into a shared crate.

The architecture is therefore:

- **One frontend** — the existing components/stores/utils, with a `Backend` interface extracted from the current IPC surface.
- **Two shells** — the desktop shell (TitleBar/Toolbar/StatusBar, unchanged) and a new web shell (AppBar/ActionStrip/Footer/DiskCard) composing the *same* sidebar, table, detail tabs, and dialogs.
- **Three backends** — `tauri` (current), `web` (fetch + polling against the new server), `demo` (existing fixtures, now trivially reusable for web screenshots and e2e).

Rejected alternative: a standalone web app (fresh Vite project or a fork). It duplicates ~90 % of the UI, forks the design system, and drifts immediately. The desktop app's own roadmap (column customization, smart filters) would have to be re-implemented twice.

---

## 4. Tech stack (additions only — frontend stack is unchanged)

| Layer | Choice | Rationale |
|---|---|---|
| Web server | **Rust + axum** (tokio, tower-http) | Same language as the existing client code; reuses the extracted crate directly; single static binary. |
| Shared crate | `crates/rtorrent` (extracted, see §5.1) | One implementation of SCGI/XML-RPC/derivation/mock for both hosts. |
| Password hashing | `argon2` | Standard choice; hash lives in the server config, never plaintext. |
| Static assets | `rust-embed` (embed `dist-web/` into the binary) | One-file deploys; `--assets <dir>` override for development. |
| Config | `toml` + env vars + flags | Seedbox-friendly; `rstorrent-web --config rstorrent-web.toml`. |
| Frontend transport | `fetch` + `setInterval`, ETag/If-None-Match | Matches the handoff's polling model; no new npm dependencies. |
| E2E tests | **Playwright** against the server in mock mode | Now possible (the desktop app couldn't e2e on macOS — root plan §8). |

No other runtime dependencies without a story-level justification.

---

## 5. Architecture

### 5.1 Process model & workspace extraction

```
┌────────── browser ──────────┐        ┌───────────── rstorrent-web (Rust) ─────────────┐
│  React SPA (web shell)      │        │  axum: static assets + /api/* + auth sessions  │
│  src/ipc backend = "web"    │ HTTPS* │  ┌──────────────┐   ┌───────────────────────┐  │
│   onSnapshot ← poll 1–2s    │◀──────▶│  │ SnapshotCache │◀──│ Poller (tokio task,   │  │
│   commands  → POST /api/cmd │  JSON  │  │ + ETag        │   │ 1s fast / 30s slow,   │  │
│   visibilitychange pause    │        │  └──────────────┘   │ idle-stop)            │  │
└─────────────────────────────┘        │                     └──────────┬────────────┘  │
                                       │            crates/rtorrent    │               │
   * TLS via reverse proxy             │  (scgi · xmlrpc · client · derive · mock)     │
                                       └───────────────────────────────┼───────────────┘
                                                                       │ SCGI (unix/tcp) or XML-RPC HTTP(S)
                                                                ┌──────▼──────┐
                                                                │  rtorrentd  │
                                                                └─────────────┘
```

**Workspace extraction (W0, the enabling refactor):** add a root `Cargo.toml` workspace with members `crates/rtorrent`, `server`, `src-tauri`.

Moves into `crates/rtorrent` (verified free of `tauri::` imports):

- `src-tauri/src/rtorrent/` — `scgi.rs`, `http.rs`, `xmlrpc.rs`, `transport.rs`, `client.rs`, `derive.rs`, `mock.rs`, `mod.rs`.
- The serde DTOs the poller emits (`Snapshot`, `TorrentDto`, `GlobalStats`, `ConnState`, detail payloads, `LogEntry`) — currently in `src-tauri/src/ipc.rs`. Both hosts serializing the *same structs* makes the JSON contract identical to the Tauri event payloads by construction.
- The pure **snapshot-assembly** function (multicall → `Vec<TorrentDto>` + globals) refactored out of `src-tauri/src/poller.rs`; each host keeps its own cadence/fan-out loop around it. Same for the slow-poll tracker-host resolution and the detail fetchers (trackers/peers/files).
- `torrent_file.rs` (lava_torrent parsing) — the server needs it for uploaded `.torrent` files.

`src-tauri` then depends on the crate; its behavior is unchanged. This is a pure move-and-reexport refactor gated on `cargo test` + `cargo clippy` staying green for the desktop app.

### 5.2 Repository layout (new/changed only)

```
rstorrent/
  Cargo.toml                    # NEW: [workspace] members = crates/rtorrent, server, src-tauri
  crates/rtorrent/              # NEW: shared client crate (see §5.1)
  server/                       # NEW: rstorrent-web binary
    src/
      main.rs                   # CLI (serve / hash-password), config load, tracing
      config.rs                 # TOML + env + flags; transport, auth, ui, paths
      poller.rs                 # 1s fast / 30s slow loops, SnapshotCache, idle-stop
      api.rs                    # /api/* routes; thin, delegates to crate
      auth.rs                   # login, session store, cookie, rate limit, middleware
      assets.rs                 # rust-embed of dist-web/ (+ --assets override)
  web.html                      # NEW: web SPA entry (like demo.html)
  vite.web.config.ts            # NEW: build web.html → dist-web/; dev proxy /api → server
  src/
    ipc/
      backend.ts                # NEW: Backend interface + active-backend registry
      tauri.ts                  # current invoke/listen bodies move here
      web.ts                    # NEW: fetch/poll implementation (§6)
      commands.ts · events.ts   # unchanged signatures, delegate to the active backend
    web/                        # NEW: web-only shell + entry
      main.tsx                  # installs the web backend, mounts <App shell="web"/>
      AppBar.tsx · ActionStrip.tsx · Footer.tsx · DiskCard.tsx · LoginScreen.tsx · StatusDialog.tsx
      tokens.web.css            # web token overrides (25px rows, column template, radii)
```

### 5.3 HTTP API contract

Same-origin JSON. Reads are GETs; mutations POST to `/api/cmd/{name}` where `{name}` **mirrors the existing command names 1:1** (`start`, `stop`, `recheck`, `force_reannounce`, `remove`, `set_label`, `set_location`, `queue_move`, `set_file_priority`, `add_tracker`, `remove_tracker`, `set_tracker_enabled`, `ban_peer`, `snub_peer`, `disconnect_peer`, `add_torrent` for magnets) with the same JSON argument objects. This keeps `src/ipc/web.ts` an almost mechanical table and reuses the server-side command plumbing already shaped by the Tauri surface.

| Endpoint | Purpose |
|---|---|
| `POST /api/session` `{password}` → 204 + cookie | Login. Argon2 verify, rate-limited (5/min/IP), constant-time. |
| `DELETE /api/session` | Logout (avatar menu). |
| `GET /api/state` → `Snapshot` | Served from the poller's cache. Strong ETag (hash of the serialized snapshot); `If-None-Match` → 304 so idle 1 s polling is ~free. |
| `GET /api/detail?hash=&tab=` → `DetailPayload` | Fetched on demand with a ~1 s per-(hash,tab) micro-cache. **No server-side watch registration** — the web adapter drives its own 2 s loop, so `set_detail_watch` becomes an adapter-internal no-op. |
| `GET /api/log?after=<seq>` → `{entries, seq}` | Ring-buffer tail; hydrates the Log tab and feeds `onLog` by diffing. |
| `POST /api/cmd/{name}` `{...args}` → result or `AppError` | Mutations + `copy_magnet` (returns the string; the *browser* writes the clipboard). |
| `POST /api/torrents/file` (multipart: bytes + `AddOptions`) → 204 | Upload add. Replaces the path-based `read_torrent_metadata`/`add_torrent{kind:"file"}` pair; the server parses metadata from the bytes. A `GET`-less `POST /api/torrents/inspect` (multipart → `TorrentMeta`) populates the Add dialog's file tree before confirming. |
| `GET /api/health` → `{server, daemon: DaemonHealth}` | Status modal + monitoring. |

Contract types are the serde structs from `crates/rtorrent` — the same shapes `src/ipc/types.ts` already declares. One additive change: `GlobalStats` gains `diskSize: number | null` next to `freeSpace` so the disk card can render the used-fraction bar (desktop ignores it until it wants a gauge too).

### 5.4 Server poller

- **Fast loop (1 s):** the same multicall the desktop issues (root plan §5.3) via the shared assembly function → `SnapshotCache` (RwLock’d `(Snapshot, ETag)`).
- **Slow loop (~30 s):** tracker hosts per torrent (cached by hash), disk stats via `statvfs` on the configured save path (co-located deployments; `freeSpace/diskSize = null` hides the card otherwise, exactly like the desktop's remote gating).
- **Idle-stop:** if no authenticated `GET /api/state` for 10 s, pause both loops; the next request triggers an immediate synchronous refresh (2 s timeout) before responding. A seedbox with no browser open costs the daemon nothing.
- **After any mutation:** trigger an immediate fast poll (mirrors the desktop's "act → re-poll, no optimistic state" rule).
- **Disconnected:** `ConnState` carries phase/error/retry countdown as today; mutations return 503 and the UI disables mutating affordances per the handoff.

### 5.5 Config (`rstorrent-web.toml`, env `RSTORRENT_WEB_*`, flags override)

```toml
listen = "127.0.0.1:9080"        # bind non-loopback → startup warning unless TLS/proxy attested
[transport]                       # same shapes as the desktop Transport type
kind = "unix"                     # unix | tcp | http
path = "/home/user/.rtorrent/rpc.socket"
[auth]
password_hash = "$argon2id$..."   # `rstorrent-web hash-password` prints this
mode = "password"                 # password | none (loopback-only dev escape hatch)
[ui]
display_name = "SY"               # avatar initials
[paths]
save_path = "/data/torrents"      # statvfs target + Add dialog default
```

`RSTORRENT_MOCK=1` swaps in the crate's `MockClient` (the ten design fixtures with ticking rates) — the same flag and fixtures as the desktop app, now also the Playwright substrate.

---

## 6. Frontend work

### 6.1 Backend abstraction (`src/ipc/backend.ts`)

Derive a `Backend` interface from the union of today's `commands.ts` + `events.ts` signatures. `commands.ts`/`events.ts` keep their exported functions (no churn in ~40 call sites) but delegate to the registered backend. Entry points register their backend before importing `App`: `src/main.tsx` → tauri, `src/web/main.tsx` → web, `src/demo/main.tsx` → mock (which can then drop `mockIPC` for the same interface — nice-to-have, not gating).

The web backend:

- `onSnapshot(cb)` → 1 s `fetch('/api/state')` loop with ETag; **`visibilitychange` pauses it** (handoff requirement) and refetches immediately on focus; any successful mutation also refetches immediately.
- `onDetail(cb)` → its own 2 s loop driven by the last `setDetailWatch(hash, tab)` args.
- `onLog(cb)` → 2 s `after=seq` loop while the Log tab is open; `getLog()` hydrates.
- Commands → `POST /api/cmd/{name}`; a 401 anywhere flips the app to the login screen; errors surface as `AppError` exactly like Tauri rejections so existing error handling just works.
- Desktop-only surface stubbed honestly: `takeOpenRequests`/`onOpenRequests`/`onMenuAction`/`onNotificationClick` → empty; keychain trio, `tuningPreview/applyTuning`, `rssFetch/rssDownload`, `setTurtle`, `testConnection` → never called because the web shell doesn't render those affordances (guard with a `capabilities` object on the backend so shared components like the context menu can ask, rather than sniffing platform).

### 6.2 Tauri-leak audit (the complete list, verified by grep)

Nine files import `@tauri-apps/*` today. Beyond `src/ipc/*` and `src/demo/main.tsx` (handled above), each needs a capability-gated web path:

| File | Tauri use | Web replacement |
|---|---|---|
| `src/actions.ts` | clipboard-manager (copy magnet) | `navigator.clipboard.writeText` |
| `src/components/menu/ContextMenu.tsx` | opener (open destination) | **Copy path** item (§2 deviation) |
| `src/components/dialogs/AddTorrentDialog.tsx` | dialog plugin (native file picker), path-based flow | `<input type="file" accept=".torrent">`; hold a `File`, `POST /api/torrents/inspect` for the tree, upload on confirm. The dialog's option surface (save path, label, start, priorities) is unchanged. |
| `src/components/dialogs/AddMagnetDialog.tsx` | clipboard read for prefill | `navigator.clipboard.readText` (graceful when permission-denied) |
| `src/components/dialogs/PreferencesDialog.tsx` | various | Not rendered in the web shell (v2). |
| `src/hooks/useDragDrop.ts` | Tauri drag-drop events (paths) | DOM `dragover/drop` with `DataTransfer.files` → same upload flow; magnet text drops too. |

`src/platform.ts` and `src/hooks/usePasteToAdd.ts`/`useKeyboard.ts` are DOM-based but assume desktop chords — see §6.4.

### 6.3 Web shell & tokens

New components under `src/web/`, composing the existing `sidebar/`, `table/`, `dialogs/`, `menu/`, and most `details/` components unchanged:

- **AppBar** per the handoff anatomy (logo mark, wordmark, Add/Magnet, search, speeds, connection dot with red "disconnected" state, settings icon → Status modal, avatar → sign-out).
- **ActionStrip** (resume/pause/remove · queue arrows · `n of m selected`) — a re-layout of existing toolbar/SelectionBar logic.
- **DiskCard** appended to the sidebar (the sidebar gains an optional `footer` slot).
- **Footer** from `Snapshot.connection` + `globals` + torrent count.
- **Detail panel web chrome + GeneralTab**: reuse the `DetailTabs` strip logic re-skinned (tab row on `bg/chrome`, right-aligned dim filename of the selected torrent), but swap in a web `GeneralTab` matching the prototype's minimal 4×2 grid (§2). The other five tabs (trackers/peers/content/speed/log) reuse the desktop components unchanged — the handoff defers their design to the desktop package.
- **LoginScreen**: centered ~320 px card on `bg/page` — logo mark + wordmark, password input, primary cyan button, `accent/red` error line. (Not in the handoff; built strictly from its tokens. Flag for a design pass.)
- **`tokens.web.css`**: loaded after `tokens.css` by the web entry only — `--row-height: 25px`, the web column template default, disk-card radius. Single-source tokens, per-shell overrides; **no hex in components** stays the law.
- Table: reuse the existing customizable-column system with **web defaults exactly matching the handoff's 12-column template**; customization remains available (harmless superset).

### 6.4 Keyboard & interactions

- **`/` focuses search** (handoff); keep ⌘/Ctrl F as an alias. Space pause/resume, Delete → remove confirm, arrows + shift/ctrl/cmd selection, Esc closes modals — all existing handlers, re-keyed where browser chords conflict (⌘O stays off the web).
- Search/filter/sort/multi-select semantics are already implemented and match the handoff (exclusive per group, live counts, sortable headers with ▾/▴).
- Paste-to-add (`usePasteToAdd`) works in browsers for magnet text as-is.

---

## 7. Security

- **The server is the auth boundary.** SCGI is unauthenticated by design — it must never be exposed; the docs say so loudly, and `listen` defaults to loopback with a startup warning on non-loopback binds when `auth.mode = none` or no TLS-forwarding header is configured.
- **Sessions:** random 128-bit token, server-side in-memory store persisted to a state file (survives restarts), 30-day sliding expiry. Cookie: `HttpOnly; SameSite=Strict; Path=/; Secure` when behind HTTPS.
- **CSRF:** `SameSite=Strict` plus a required `X-Rstorrent: 1` header on all mutations (fetch adds it; forms can't).
- **Login hardening:** argon2id verify, per-IP token bucket, generic error text, no user enumeration surface (single user).
- **Destructive gating:** delete-data uses the `trash` crate and is offered only when the server attests co-location with the daemon (unix-socket transport, or explicit config) — the same posture the desktop takes for remote daemons.
- **Reverse proxy docs** (nginx/caddy snippets in `docs/web-setup.md`): TLS, HSTS, `X-Forwarded-For` trust only when `trusted_proxies` is set (rate-limit correctness).

---

## 8. Deployment & packaging

- `cargo build --release -p rstorrent-web` → one binary with the SPA embedded. `rstorrent-web hash-password`, `rstorrent-web --config …`.
- **Dockerfile** (multi-stage: npm build → cargo build → distroless) and a sample `systemd` unit; both in `docs/web-setup.md` alongside the reverse-proxy and rtorrent-config snippets (reusing `docs/rtorrent-setup.md` content).
- Dev loop: `npm run dev:web` (Vite on 1421, proxying `/api` → `127.0.0.1:9080`) against `cargo run -p rstorrent-web` — or fully offline with `RSTORRENT_MOCK=1`.
- CI additions: server `cargo test`/`clippy`/`fmt` on **Linux** (the target platform — cheaper than the macOS runners the Tauri crate needs), `vite.web.config.ts` build, and a Playwright job against the mock-mode server.

---

## 9. Testing & verification

| Layer | How |
|---|---|
| Shared crate | Existing Rust unit tests move with the code (xmlrpc `<i8>`/faults, SCGI framing, derivation) — must stay green for **both** hosts. |
| Contract | JSON round-trip test: serde fixtures ↔ `src/ipc/types.ts` via the shared design fixtures, so Tauri events and HTTP bodies can't drift apart. |
| Server | axum handler tests via `tower::ServiceExt` (auth flows, ETag/304, gating, error mapping); integration against the in-process mock SCGI server (exists). |
| Web adapter | Vitest with a stubbed `fetch`: polling cadence, visibility pause, 401 → login, ETag handling. |
| Stores/selectors/format | Existing Vitest suites — untouched and still binding. |
| **E2E (new capability)** | Playwright vs `RSTORRENT_MOCK=1 rstorrent-web`: login, table renders the ten fixtures, filter/search/sort, select → detail tabs, context menu, add-magnet flow, disconnect banner. Chrome-level screenshot comparison against the prototype (its row templates need the design tool's `support.js` to render, so row-level QA is driven by the fixture data block + tokens). |
| Live daemon | Manual checklist against rtorrent 0.16.x (macOS brew + WSL, per existing docs), incl. a remote HTTP-transport config. |

Definition of done: all v1 stories complete, Playwright suite green in CI, `cargo test`/`clippy`/`fmt` + `npm test`/`typecheck`/`lint` clean across the workspace, the desktop app builds and passes unchanged, and a fresh `docker run` serves a working UI against a real daemon.

---

## 10. Milestones

| # | Epic | Deliverable (demoable) | Contents |
|---|---|---|---|
| **W0** | WE0 | Workspace extraction lands; desktop app unchanged and green | Root workspace, `crates/rtorrent`, DTO/assembly moves, `src-tauri` on the crate |
| **W1** | WE1 | `rstorrent-web` serves the mock: browser shows a live-updating table | axum skeleton, config, poller + cache + ETag, `/api/state`, `web.html` entry, backend registry + web adapter (read path), embedded assets |
| **W2** | WE2 | Visual parity with the handoff | AppBar, ActionStrip, Footer, DiskCard, `tokens.web.css` (25 px rows, column template), search + `/`, connection states |
| **W3** | WE3 | Control: full read/write against a live daemon | `/api/cmd/*`, action strip + context menu + keyboard, remove confirm, detail tabs via `/api/detail`, log tail, slow-poll trackers + disk stats |
| **W4** | WE4 | Add flows | Upload + inspect (file tree), drag & drop, magnet dialog, paste-to-add, error surfacing |
| **W5** | WE5 | Auth | Login screen, sessions, sign-out, rate limiting, CSRF header, gating, security pass |
| **W6** | WE6 | Ship | Docker + systemd + reverse-proxy docs, Playwright suite in CI, perf pass (N-hundred-torrent snapshot), README/status updates |

Strict order W0→W1; W2–W4 parallelize after W1; W5 anytime after W1; W6 last.

---

## 11. Risks & open questions

| Risk | Mitigation |
|---|---|
| ~~Desktop design README was overwritten by the web handoff~~ (resolved, WE0-S5) | Desktop spec restored as `design/README-desktop.md`; root `plan.md` §2 repointed. |
| Extraction regressions in the desktop app | W0 is a pure move; desktop `cargo test` + manual smoke in mock mode gate the merge. |
| Shell divergence — web/desktop drift as features land | Shared components stay shared; shells own layout only. Token overrides live in one file per shell. New feature stories must state which shells they touch. |
| Two token values differ by design (23 vs 25 px, grid template) | Explicit, documented overrides in `tokens.web.css` — not forks of `tokens.css`. |
| Multiple browsers mutating concurrently | Last-writer-wins; the shared 1 s poller converges all clients quickly. No locking in v1. |
| Snapshot payload size at seedbox scale (1–5 k torrents) | ETag/304 covers the idle case; measure serialized size at 5 k fixtures in W6; gzip via tower-http; delta/push protocol is the v2 lever, behind the adapter. |
| Browser clipboard/file APIs need secure context | Fine over HTTPS or localhost; degrade gracefully (magnet prefill optional; copy falls back to a select-text modal). |
| `d.directory.set`, queue-order, missing-RPC stats | Same realities and same mitigations as the desktop (root plan §10); server reuses that logic via the crate. |

**Open questions (parked, with leanings):** WebSocket/SSE push (v2; adapter-shaped so it's a drop-in) · responsive/mobile layout (v2) · serving the web UI *from the desktop app* as an embedded remote (attractive later — the server crate makes it cheap) · multi-daemon profiles in one web instance (v2) · whether the Status modal grows into web Preferences (decide after v1 usage).

---

## 12. How to use these docs

1. `design/README.md` wins on any visual question; then `rTorrent Web UI.dc.html`; then this plan; the desktop 1c HTML covers the shared dialog designs.
2. Work through [tasks.md](tasks.md) epic by epic in milestone order (§10); every story lists acceptance criteria and a **Verify** step — run it before marking the story done.
3. Root `plan.md` remains authoritative for the desktop app and the rtorrent RPC details it documents (§5 there is not duplicated here — the crate carries that knowledge).
4. Update the repo README's Layout section when `crates/`, `server/`, and `web.html` land.
