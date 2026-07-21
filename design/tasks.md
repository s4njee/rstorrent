# rstorrent Web UI — Epics & Stories

Execution backlog for the plan in [plan.md](plan.md). Design authority: [`design/README.md`](README.md) + `design/rTorrent Web UI.dc.html` (the desktop `rTorrent Client 1c.dc.html` covers the shared dialog designs).

## Conventions

- **Order:** epics are numbered in dependency order and map to milestones W0–W6 (plan.md §10). Don't start an epic before its `Deps` are satisfied unless the story says otherwise.
- **Story format:** `[ ]` checkbox · ID · title · what/why · **AC** (acceptance criteria) · **Verify** (a concrete check to run).
- **Definition of done (every story):** workspace compiles with zero warnings from `cargo clippy --all-targets -- -D warnings` and `tsc --noEmit`; new pure logic has unit tests; UI uses tokens only (`tokens.css` + `tokens.web.css` — no hard-coded hex in components); works in **mock mode** (`RSTORRENT_MOCK=1`); and **the desktop app stays green** — `cargo test`, `npm test`, and `npm run tauri dev` (mock) are unaffected by every story in this file.
- **Run commands** (established in WE0/WE1): `RSTORRENT_MOCK=1 cargo run -p rstorrent-web` (mock server) · `npm run dev:web` (Vite + `/api` proxy) · `npm run build:web` · `cargo test` (workspace) · `npm test` · `npm run e2e` (Playwright, from WE6-S1).
- Sizes: S (≤2 h), M (≤half day), L (day). Split anything growing past L.

## Progress

**WE0 complete.** The workspace extraction landed and the desktop app is
unchanged and green. Baseline before the refactor (Rust **90 passed / 11
ignored**, frontend typecheck + **99 vitest**) is preserved exactly:

- `crates/rtorrent` (`rtorrent-core`) now owns the whole daemon client layer —
  `rtorrent/{mod,scgi,http,xmlrpc,transport,client,derive,mock}`, plus `secrets`
  (keychain), `torrent_file` (lava_torrent parsing), and a new `types` module
  holding the shared DTO contract. It has **zero Tauri dependencies**.
- `src-tauri` depends on the crate and re-exports it at the historical paths
  (`crate::rtorrent::…`, `crate::secrets::…`, `crate::torrent_file::…`,
  `crate::ipc::…`), so the ~32 call sites and all component code are untouched.
- Root `Cargo.toml` workspace (`crates/rtorrent`, `src-tauri`); `[profile.release]`
  moved to the root; single root `Cargo.lock`; root `/target` git-ignored;
  eslint/prettier updated to ignore the now-root `target/`.
- Additive `GlobalStats.diskSize` (Rust + `src/ipc/types.ts` + fixtures); desktop
  poller leaves it `null`, the web disk card will populate it.
- Files moved with `git mv` (history preserved as renames).

Verification (this machine): `cargo test` **90/11** (39 crate + 51 desktop),
`cargo clippy --workspace --all-targets -D warnings` clean, `cargo fmt --check
--all` clean, full workspace `cargo build` links the desktop binary, frontend
`typecheck`/`vitest`(99)/`lint` all clean.

> Note: S1–S3 landed as **one coupled refactor** — the `RtorrentApi` trait
> signatures reference the DTOs, so the module and the shared types had to move
> together. The `server` workspace member and the snapshot-assembly pure-fn
> extraction are deferred to WE1, where the server first needs them (keeping WE0
> a pure, low-risk move).
>
> Environment note: a Windows Application Control policy (os error 4551) blocks
> freshly-built Tauri plugin build-scripts in a *new* target dir. Reusing the
> pre-existing `src-tauri/target` (via `CARGO_TARGET_DIR`) sidesteps it; CI's
> clean macOS runners are unaffected. This policy also blocks the *desktop* test
> binary after any src-tauri change, so desktop tests are verified by compile +
> clippy here and by CI's macOS runner; the crate/server test binaries run fine.

**WE1 complete.** `rstorrent-web` serves the mock over HTTP and the browser
backend polls it — the read-path demo works end to end.

- **`server/` crate** (`rstorrent-web`): axum + tokio + clap. `config.rs`
  (TOML + `RSTORRENT_WEB_*` env + flags, precedence-tested), `auth.rs` (argon2id
  `hash-password`), `poller.rs` (1s fast loop via the shared `rtorrent_core::
  snapshot` assembly, 30s tracker/disk slow work, **idle-stop** parking after
  10s), `state.rs` (`SnapshotCache` + ETag), `api.rs` (`/api/state` with
  ETag/304 + cold-cache wait, `/api/health`), `disk.rs` (unix statvfs),
  `assets.rs` (rust-embed of `dist-web/`, `--assets` override, SPA fallback).
- **Frontend backend registry**: `ipc/backend.ts` (the `Backend` seam +
  `capabilities`), `ipc/tauri.ts` (desktop/demo), `ipc/web.ts` (fetch/poll read
  path: ETag reuse, `visibilitychange` pause, 401 hook). `commands.ts`/`events.ts`
  now delegate to the registered backend — ~40 call sites unchanged. `main.tsx`
  and `demo/main.tsx` register the Tauri backend.
- **Web entry**: `web.html` → `src/web/main.tsx` mounts a placeholder shell
  (`Placeholder.tsx`, live ticking table) over the web backend; `vite.web.config.ts`
  (dev on :1421 proxying `/api` → :9080), `dev:web`/`build:web` scripts.

Verification: server `cargo test` **14**, clippy clean; web-adapter vitest **9**
(cadence, ETag, 304, hidden-tab pause, unlisten, 401); frontend typecheck + lint
clean, **108** vitest total. End-to-end smoke: `RSTORRENT_MOCK=1 rstorrent-web`
serves the built SPA + assets at `/`, `/api/state` returns the ten fixtures with
working ETag→304, `/api/health` reports daemon + server identity.

> Deferred within WE1: the server member and shared assembly fn (WE0-S2) landed
> here as planned. `dist-web/` is git-kept (`.gitkeep`) so rust-embed always
> compiles; the built bundle is git-ignored.

**WE2 implemented (S1–S7); S8 visual-parity is the pending manual gate.** The web
shell composes the real chrome around the shared components.

- **`tokens.web.css`** (row-height 25px, row-hover, web column template, card
  radius) loaded after `tokens.css` by the web entry only — desktop stays 23px.
- **Shell components** (`src/web/`): `AppBar` (logo/wordmark, Add/Magnet, 240px
  search with `/` focus, live speeds, connection dot, settings + avatar stubs),
  `ActionStrip` (resume/pause/remove · queue · `n of m selected`), `Footer`
  (version · endpoint · dht · counts · totals), `DiskCard` (used-fraction bar,
  hidden when free/size null). `WebApp` wires the live-data channels and composes
  `FilterSidebar` (now with an optional `footer` slot for the disk card),
  `TorrentTable`, `DetailTabs`, `ContextMenu`, `DialogHost` — the shared
  components, unchanged. The desktop `DetailTabs` already renders the prototype's
  4×2 General grid, so it's reused rather than forked (WE2-S6).
- **Status-error text** (`webStatusLabel`): `trk error` / `disk error` / `error`
  from `statusMsg` — pure + unit-tested; wired into the web table in WE3.

Verification: typecheck + lint clean, **113** vitest (adds `statusText` ×3 and a
`WebApp` render smoke proving the composed shell renders with no mount-time Tauri
crash), `npm run build:web` bundles all 94 modules (the shared table/sidebar/
detail/dialogs now compile into the web app), and the server serves the built
SPA + assets. **Not yet done (S8):** pixel parity vs the prototype and live
in-browser interaction — the manual gate, since this environment is headless
(matches the project's mock-mode manual-QA approach).

> Note: `renderToString` reads zustand's *initial* snapshot by design, so the
> render smoke can't inject a populated state; the connected table render is
> covered by the bundle build + the desktop's use of the same components.

**WE3 implemented (S1–S5); S6 live-daemon verification is the pending manual
gate.** Full read/write over HTTP against the mock.

- **`POST /api/cmd/{name}`** (`cmd.rs`): every desktop command name 1:1 —
  start/stop/recheck/force_reannounce/set_label/set_location/set_file_priority/
  add|remove|set_tracker_enabled/ban|snub|disconnect_peer, plus compound
  `queue_move` (priority steps from the cached snapshot), `remove` (delete-data
  trashes co-located files, 403 off-box), `add_torrent` (magnet), and
  `copy_magnet` (returns the URI). 503 while disconnected; success triggers an
  immediate re-poll.
- **`GET /api/detail?hash=&tab=`** with a ~1s per-(hash,tab) micro-cache;
  **`GET /api/log?after=<seq>`** over a sequenced ring buffer.
- **Adapter** (`web.ts`): mutations → `POST /api/cmd/{name}` (mechanical table,
  `X-Rstorrent` header, error-message surfacing, immediate snapshot refetch);
  desktop-only commands rejected client-side; `onDetail`/`onLog` 2s polling
  loops; `getLog` hydration.
- **Capability-gated UI**: `actions.copyMagnet` uses `navigator.clipboard` on
  web / Tauri clipboard on desktop; `ContextMenu` swaps **Open destination →
  Copy path** and the native folder picker → a prompt when `!localFs` /
  `!nativeDialogs`. `WebApp` wires the browser-safe keyboard shortcuts (S5).

Verification: server `cargo test` **25** (mutation happy-paths, 503/403/404/400
gating, detail tab payloads, log sequencing) + clippy clean; adapter vitest **13**
(POST mapping, error surfacing, desktop-only rejection, detail + log loops);
frontend typecheck/lint clean, **117** vitest, build:web green. **Not yet done
(S6):** the live-daemon checklist against rtorrent 0.16.x — needs a real daemon,
unavailable in this environment.

**WE5 complete.** Single-password auth over sessions, verified end-to-end.

- **Sessions** (`auth.rs`): argon2id verify, 128-bit tokens, sliding 30-day
  expiry (in-memory store; a restart re-prompts), per-IP login rate limiter
  (5/min). **Endpoints** (`api.rs`): `POST /api/session` (204 + `HttpOnly;
  SameSite=Strict; Path=/` cookie, generic error, 429 when throttled),
  `DELETE /api/session`. **Middleware** gates every `/api/*` except
  `/api/session` on a valid session (bypassed only in `auth.mode = none`, which
  startup refuses off-loopback), and requires the `X-Rstorrent` header on POSTs
  (CSRF). **Hardening headers** (`nosniff`, `X-Frame-Options: DENY`) on every
  response.
- **Adapter**: `webLogin`/`webLogout`; the existing 401 hook flips the app to
  the login screen. **UI**: `LoginScreen` (centered card, tokens), `StatusDialog`
  (settings-icon target: daemon/endpoint/server info + Sign out), the app-bar
  avatar/settings wired, and a `WebRoot` auth gate that probes `/api/health`.

Verification: server `cargo test` **31** (login gates state, wrong password 401,
CSRF 403, logout revokes, rate-limit 429, session/limiter units) + clippy clean;
frontend typecheck/lint clean, **117** vitest, build:web green. **End-to-end over
HTTP** (`rstorrent-web hash-password` → password-mode config → real binary):
unauth `/api/state` 401 → login 204 + cookie → authed 200 → mutation-without-CSRF
403 → logout 204 → 401. Deferred to a design pass: the login screen is built from
tokens (the handoff has none); session persistence across restarts is v2.

**WE4 (S1 done) + WE6 (S2/S4/S5 done); the browser/live-daemon-gated stories
remain.**

- **WE4-S1** — upload endpoints: `POST /api/torrents/inspect` (multipart →
  `TorrentMeta` via a new `read_metadata_bytes` in the crate) and `POST
  /api/torrents/file` (bytes + opts → `load.raw`), 10 MiB cap; adapter helpers
  `webInspectTorrent`/`webUploadTorrent`. Server test covers the multipart
  wiring + parse-error path.
- **WE6-S2** — contract test: a crate test asserts the shared DTOs serialize as
  the camelCase `src/ipc/types.ts` expects (guards against a dropped
  `rename_all`); `tsc` guards the TS side.
- **WE6-S4** — [docs/web-setup.md](../docs/web-setup.md): config reference,
  systemd unit, multi-stage Dockerfile, nginx + Caddy TLS, security notes.
- **WE6-S5** — root README (Web UI section + workspace Layout), backlog `B19 →
  in progress`, and CI: the Linux job now builds/clippy/tests `rtorrent-core`
  **and** `rstorrent-web` and builds the web SPA.

Verification: crate `cargo test` **42**, server **32**, clippy + `fmt --all`
clean across the workspace; frontend typecheck/lint clean, **117** vitest,
build:web green.

**Still open (browser or live-daemon gated, not actionable headlessly):**
WE2-S8 (pixel parity), WE3-S6 (live-daemon checklist), WE4-S2/S3 (the Add-dialog
file-input rewiring + drag-drop — server side + adapter helpers are ready),
WE4-S4 (magnet dialog — magnet add already works via the adapter; only the
clipboard-prefill polish remains), WE6-S1 (Playwright suite), WE6-S3 (perf pass
at 5k torrents). These need a real browser and/or a live rtorrent daemon.

---

## WE0 — Workspace extraction & shared crate  *(W0)*

Deps: none. This epic is a pure refactor: **no behavior change** anywhere; the desktop test suites are the regression gate.

- [x] **WE0-S1 · Cargo workspace + move the rtorrent module** (L)
  Add a root `Cargo.toml` workspace (members: `crates/rtorrent`, `src-tauri`; `server` joins in WE1). Move `src-tauri/src/rtorrent/{mod,scgi,http,xmlrpc,transport,client,derive,mock}.rs` to `crates/rtorrent` (verified free of `tauri::` imports); `src-tauri` depends on the crate and re-exports so its internal `use` paths barely change. Use `git mv` so history follows.
  **AC:** desktop `cargo test` + clippy green from the workspace root; the crate has no tauri/tauri-plugin dependencies; `npm run tauri dev` (mock) launches and behaves identically.
  **Verify:** `cargo test && cargo clippy --all-targets -- -D warnings` at the root; mock-mode smoke run.

- [x] **WE0-S2 · DTOs + snapshot assembly into the crate** (M)
  Move the serde structs the poller emits (`Snapshot`, `TorrentDto`, `GlobalStats`, `ConnState`, detail payloads, `LogEntry`) from `src-tauri` into the crate, plus the pure snapshot-assembly function (multicall → DTOs) and the detail fetchers (trackers/peers/files) out of `poller.rs` — the Tauri poller keeps only cadence + event emission. Additive field: `GlobalStats.diskSize: Option<u64>` (+ `diskSize: number | null` in `src/ipc/types.ts`), so the web disk card can render used-fraction (plan §5.3).
  **AC:** the JSON the Tauri poller emits is field-for-field identical to before, plus `diskSize`; both hosts will serialize the *same structs* (plan §5.1).
  **Verify:** `cargo test`; a serde test asserting the `Snapshot` field set matches a fixture list mirrored from `types.ts`.

- [x] **WE0-S3 · .torrent parsing into the crate** (S)
  Move `torrent_file.rs` (lava_torrent metadata parsing) into the crate; the desktop `read_torrent_metadata` command delegates. The server needs it for uploaded files (WE4-S1).
  **AC:** desktop Add dialog unaffected; info-hash test still passes.
  **Verify:** `cargo test` (torrent-file cases).

- [x] **WE0-S4 · CI: workspace matrix** (S)
  CI runs workspace `fmt`/`clippy`/`test` — the shared crate (and later `server`) on **Linux**, the Tauri crate stays on macOS. Frontend jobs unchanged.
  **AC:** a PR touching only `crates/rtorrent` gets Linux-speed feedback; nothing regresses on the existing jobs.
  **Verify:** CI green on the WE0 PR.

- [x] **WE0-S5 · Design-doc hygiene** (S)
  Restore the desktop handoff (overwritten by the web handoff) from git history as `design/README-desktop.md`; point root `plan.md` §2 at it.
  **AC:** both plans reference live files; the web `design/README.md` is untouched.
  **Verify:** follow every design link from both plan files.

---

## WE1 — Server skeleton & read path  *(W1)*

Deps: WE0 (S5 and S6 are frontend-only and parallel-safe from day one).

- [x] **WE1-S1 · `rstorrent-web` crate scaffold: CLI + config** (M)
  `server/` axum + tokio + tracing. Subcommands: `serve` (default) and `hash-password` (argon2id, prints the TOML line). Config per plan §5.5: TOML file + `RSTORRENT_WEB_*` env + flags; sections `listen`, `[transport]` (unix | tcp | http — the crate's existing transports), `[auth]` (`password_hash`, `mode: password|none`), `[ui].display_name`, `[paths].save_path`, poll intervals. Startup **warning on a non-loopback bind** with `auth.mode = none` or no TLS-forwarding config.
  **AC:** `hash-password` output round-trips through verify; malformed config produces an actionable one-line error; precedence flags > env > file is tested.
  **Verify:** `cargo test -p rstorrent-web config`.

- [x] **WE1-S2 · Server poller + SnapshotCache** (L)
  Fast 1 s loop via the shared assembly fn → `RwLock<(Snapshot, ETag)>`; slow ~30 s loop (tracker hosts cached by hash; `statvfs` on `paths.save_path` → `freeSpace`/`diskSize`, `null` off-box). **Idle-stop:** pause both loops after 10 s without a `/api/state` request; on wake, synchronous refresh with a 2 s cap before responding. **After any mutation** (WE3-S1) trigger an immediate fast poll. `ConnState` phases with retry countdown on failure. `RSTORRENT_MOCK=1` swaps in the crate's `MockClient`.
  **AC:** mock cache ticks ~1 s; daemon-down yields `disconnected` + retry seconds; idle provably stops daemon traffic (trace log); ETag changes iff the snapshot changed.
  **Verify:** `cargo test -p rstorrent-web poller` + an integration test against the in-process mock SCGI server.

- [x] **WE1-S3 · `GET /api/state` + `GET /api/health`** (M)
  `/api/state` serves the cache with a strong ETag; `If-None-Match` → 304; gzip via tower-http. `/api/health` → `{server: {version, displayName}, daemon: DaemonHealth | null}`.
  **AC:** two identical polls → 200 then 304; payload is byte-identical JSON to the Tauri event shape (same structs).
  **Verify:** `tower::ServiceExt` handler tests.

- [x] **WE1-S4 · `web.html` entry, build, embed** (M)
  `web.html` + `src/web/main.tsx` (registers the web backend, mounts `<App/>` with a placeholder web shell); `vite.web.config.ts` → `dist-web/`; npm scripts `dev:web` (port 1421, proxy `/api` → `127.0.0.1:9080`) and `build:web`; `server/src/assets.rs` embeds `dist-web/` via rust-embed with an `--assets <dir>` override and an SPA fallback route.
  **AC:** `cargo run -p rstorrent-web` alone serves the built app at `/`; the dev-proxy loop hot-reloads.
  **Verify:** browser smoke via both paths; `npm run build:web` in CI.

- [x] **WE1-S5 · Backend registry (frontend refactor)** (M) — *parallel-safe from day one*
  `src/ipc/backend.ts`: a `Backend` interface derived from the union of `commands.ts` + `events.ts` signatures, plus a `capabilities` object (localFs, revealInFileManager, nativeDialogs, keychain, …). Move the `invoke`/`listen` bodies to `src/ipc/tauri.ts`; `commands.ts`/`events.ts` keep their exported signatures and delegate to the registered backend (no churn at ~40 call sites). Entries register before importing `App` (`src/main.tsx` → tauri; `src/demo/main.tsx` may keep `mockIPC` for now).
  **AC:** desktop app and demo behave identically; no component outside `src/ipc/` + entry files imports `@tauri-apps/*` except the audited list in plan §6.2 (those migrate in WE3/WE4).
  **Verify:** `npm test` + `tsc --noEmit`; `npm run tauri dev` and `demo.html` smoke.

- [x] **WE1-S6 · Web adapter: read path** (M)
  `src/ipc/web.ts`: `onSnapshot` = 1 s `fetch('/api/state')` with ETag reuse, **`visibilitychange` pause** + immediate refetch on focus; `onDetail`/`onLog` stubs (WE3); commands reject with a capability error for now; 401 handling lands in WE5-S2.
  **AC:** mock server + `npm run dev:web` → live ticking table in the placeholder shell — **the W1 demo**.
  **Verify:** Vitest with stubbed `fetch` (cadence, hidden-tab pause, 304 path); browser smoke.

---

## WE2 — Web shell & visual parity  *(W2)*

Deps: WE1-S4/S5/S6. Authority: the prototype + plan §2's pinned details; compare side-by-side at 1280×800.

- [x] **WE2-S1 · `tokens.web.css` overrides** (S)
  Loaded only by the web entry, after `tokens.css`: `--row-height: 25px`, the web column-template default (`minmax(220px,1fr) 70 100 90 52 52 80 80 66 50 78 minmax(90px,120px)`), disk-card radius 6 px / bar radius 3 px. Declare min viewport 1000×640.
  **AC:** desktop stays 23 px; no new hex outside theme files.
  **Verify:** grep for hex in `src/web/`; both apps side-by-side.

- [x] **WE2-S2 · AppBar** (L)
  Per prototype: logo mark (22 px, `bg/selected`, cyan border, "r") + wordmark `rtorrent / web` · separator · **Add** (primary) + **Magnet** (secondary) wired to the existing dialogs · spacer · 240 px search (`/ search torrents` placeholder, `/` shortcut focuses, filters live) · live ↓/↑ (10.5 px, cyan-bright/green-soft) · separator · connection dot ("connected" green / "disconnected" red per handoff) · settings icon (26 px, opens the Status modal — stub until WE5-S3) · avatar (24 px, lowercase initials from `/api/health`).
  **AC:** pixel-matches the prototype's app bar; disconnected state renders red and mutating buttons disable.
  **Verify:** side-by-side vs prototype; search + `/` behave in mock.

- [x] **WE2-S3 · ActionStrip** (M)
  Resume ▶ / Pause ⏸ / Remove (trash) · separator · queue up/down (tooltip "priority" — plan §2 deviation) · right-aligned `n of m selected`. Buttons disable on empty selection and while disconnected; Remove opens the existing confirm dialog.
  **AC:** matches prototype layout/metrics; actions dispatch through the existing action layer.
  **Verify:** mock: select rows, run each button.

- [x] **WE2-S4 · Sidebar footer slot + DiskCard** (M)
  `FilterSidebar` gains an optional `footer` slot (desktop passes none). DiskCard per prototype: uppercase caption row `DISK / N GiB free` + 5 px cyan bar at `1 − free/total` from `freeSpace`/`diskSize`; hidden when either is `null`.
  **AC:** mock renders the 412 GiB / 63 % look; remote transport hides the card.
  **Verify:** mock + a `diskSize: null` fixture case.

- [x] **WE2-S5 · Footer** (S)
  26 px, dim 10.5 px: `rtorrent {version} · {TRANSPORT} @ {endpoint}` · `dht: N nodes` · spacer · `N torrents` · ↓/↑ colored totals — all from `Snapshot`.
  **AC:** matches prototype; shows the live endpoint string, not a hard-coded one.
  **Verify:** side-by-side in mock.

- [x] **WE2-S6 · Detail panel web chrome + web GeneralTab** (M)
  Re-skin the `DetailTabs` strip (tabs on `bg/chrome`, active cyan-bright + 2 px underline, right-aligned dim filename of the selected torrent). Web `GeneralTab` = the prototype's minimal 4×2 grid (`active · down · up · ratio / eta · conns · dl-limit · ul-limit`; limits compact `∞`/`5.0M`; **no pieces bar**). Other five tabs mount the desktop components unchanged.
  **AC:** general matches the prototype; switching tabs still drives the detail watch.
  **Verify:** mock: select Fedora row, compare panel side-by-side.

- [x] **WE2-S7 · Status-column error text** (S)
  Error rows display a short lowercase error text (`trk error` / `disk error` / `error`) derived from `statusMsg`, per the prototype. Implement as an optional formatter on the shared status cell; desktop rendering unchanged.
  **AC:** the mock error fixture shows `trk error` on web and today's text on desktop.
  **Verify:** unit test on the formatter; both apps in mock.

- [ ] **WE2-S8 · Visual parity pass** (S)
  Walk the prototype + plan §2 metrics with devtools (heights 46/25/26, sidebar 186, paddings, every color eyedropper-checked against tokens); fix drift. **The W2 demo.**
  **AC:** no visible diff at 1280×800 beyond live data.
  **Verify:** annotated side-by-side screenshots attached to the PR.

---

## WE3 — Control & detail (live daemon)  *(W3)*

Deps: WE1 (server stories), WE2 (UI stories).

- [x] **WE3-S1 · `POST /api/cmd/{name}` mutation surface** (L)
  Mirror the command names 1:1 (plan §5.3): `start`, `stop`, `recheck`, `force_reannounce`, `remove`, `set_label`, `set_location`, `queue_move`, `set_file_priority`, `add_tracker`, `remove_tracker`, `set_tracker_enabled`, `ban_peer`, `snub_peer`, `disconnect_peer`, `copy_magnet` (returns the URI string). `AppError` JSON mapping; **503 while disconnected**; success triggers an immediate fast poll (WE1-S2). `remove{deleteData:true}` uses the `trash` crate and is rejected unless co-location is attested (unix transport, or explicit config) — same posture as the desktop's remote gating.
  **AC:** every listed name round-trips against the mock; delete-data gating and 503 paths covered.
  **Verify:** `tower::ServiceExt` tests per command family.

- [x] **WE3-S2 · Adapter command table + capability-gated UI** (M)
  `web.ts` maps commands → `POST /api/cmd/{name}` (mechanical table); `actions.ts` copies via `navigator.clipboard` (fallback: select-text modal); `ContextMenu` swaps **open destination → Copy path** behind `capabilities.revealInFileManager` (plan §2 deviation); `set_detail_watch` becomes adapter-internal (drives WE3-S3's loop, no server call).
  **AC:** resume/pause/remove/label/copy-magnet/copy-path all work from the web context menu in mock; desktop menu unchanged.
  **Verify:** Vitest on the table + browser smoke.

- [x] **WE3-S3 · `GET /api/detail` + adapter detail loop** (M)
  Server fetches on demand with a ~1 s per-`(hash, tab)` micro-cache — no watch registration. Adapter polls at 2 s for the current selection/tab only. Trackers/peers/content/speed tabs go live (speed's ring buffer already feeds off snapshots).
  **AC:** switching rows/tabs updates within 2 s; only the active tab is polled.
  **Verify:** handler tests (cache TTL) + mock browser check of all tabs.

- [x] **WE3-S4 · `GET /api/log`** (S)
  Server ring buffer (connection changes, action results, RPC errors) with `?after=<seq>` → `{entries, seq}`. Adapter: hydrate on open, 2 s tail while the Log tab is active, feed `onLog` by diffing.
  **AC:** an action's result line appears in the Log tab within 2 s.
  **Verify:** handler test + mock smoke.

- [x] **WE3-S5 · Keyboard pass** (S)
  `/` focuses search (Ctrl/⌘F alias), Space pause/resume, Delete → remove confirm, arrows + shift/ctrl/cmd selection, Esc closes modals/menus; drop desktop-only chords that fight the browser (⌘O etc.).
  **AC:** every binding works in Chromium + Firefox; none hijack native browser shortcuts beyond `/`.
  **Verify:** manual matrix in mock.

- [ ] **WE3-S6 · Live-daemon verification** (M)
  Against rtorrent 0.16.x (macOS brew or WSL, per existing docs): polling, every WE3-S1 action, all detail tabs, `set_location`'s stop→set→conditional-start, tracker/peer ops, disconnect/reconnect (kill and restart the daemon under the UI). Record as a QA checklist section in `docs/web-setup.md`. **The W3 demo.**
  **AC:** checklist green; any deviation filed as a story.
  **Verify:** run the checklist.

---

## WE4 — Add flows  *(W4)*

Deps: WE3-S1, WE2.

- [x] **WE4-S1 · Upload endpoints** (M)
  `POST /api/torrents/inspect` (multipart bytes → `TorrentMeta` via the crate parser; ~10 MiB cap) and `POST /api/torrents/file` (bytes + `AddOptions` → `load.raw`/`load.raw_start` with save-path/label commands; unwanted files → priority 0 after load). Magnets go through `/api/cmd/add_torrent`.
  **AC:** oversize/malformed uploads → clean 4xx with `AppError`; add lands in mock and live.
  **Verify:** multipart handler tests + a live add.

- [ ] **WE4-S2 · AddTorrentDialog web path** (M)
  Replace the native picker under `capabilities.nativeDialogs`: `<input type="file" accept=".torrent">`, hold the `File`, `inspect` → the existing tri-state file tree, upload on confirm. Save-path/label/start/priority options unchanged.
  **AC:** the dialog is visually identical to desktop (design authority: desktop 1c); full flow works in mock.
  **Verify:** browser smoke; desktop dialog untouched.

- [ ] **WE4-S3 · Drag & drop + paste-to-add** (M)
  DOM `dragover`/`drop` on the window: `.torrent` files → the WE4-S2 flow (pre-filled dialog); dropped/pasted magnet text → the magnet dialog. `usePasteToAdd` verified in-browser.
  **AC:** dropping a file and pasting a magnet each open the right pre-filled dialog.
  **Verify:** manual in Chromium + Firefox.

- [ ] **WE4-S4 · AddMagnetDialog** (S)
  Clipboard prefill via `navigator.clipboard.readText`, silently skipped when permission is denied; submit via `/api/cmd/add_torrent`. **The W4 demo.**
  **AC:** works with clipboard permission granted and denied.
  **Verify:** browser smoke both states.

---

## WE5 — Auth  *(W5)*

Deps: WE1. Parallel with WE2–WE4.

- [x] **WE5-S1 · Sessions + login/logout endpoints** (L)
  `POST /api/session` `{password}` → argon2id verify (constant-time), 128-bit random token, server-side store persisted to a state file (survives restart), 30-day sliding expiry; cookie `HttpOnly; SameSite=Strict; Path=/` (+`Secure` behind a TLS-forwarding header). `DELETE /api/session`. Per-IP rate limit 5/min (respecting `trusted_proxies` for the client IP). Generic error text.
  **AC:** login/logout/expiry/rate-limit paths tested; tokens absent from logs.
  **Verify:** `cargo test -p rstorrent-web auth`.

- [x] **WE5-S2 · Auth middleware + CSRF + adapter 401 handling** (M)
  All `/api/*` except `session` require a session (`auth.mode = none` bypass refuses to serve on non-loopback binds). Mutations additionally require the `X-Rstorrent: 1` header; the adapter always sends it and maps any 401 → login state (in-flight polls stop).
  **AC:** unauthenticated `/api/state` → 401; a mutation without the header → 403; expired-session UX lands on the login screen without a JS error.
  **Verify:** middleware tests + manual expiry check.

- [x] **WE5-S3 · LoginScreen + Status modal + sign-out** (M)
  LoginScreen per plan §6.3 (centered ~320 px card, logo + wordmark, password input, primary cyan button, `accent/red` error). Avatar menu → **Sign out**. Settings icon → read-only **Status** modal on `ModalBase`: daemon version/endpoint/health, server version (from `/api/health`).
  **AC:** wrong password shows the error inline; sign-out returns to login; Status renders live data.
  **Verify:** browser smoke; screenshot for the design record.

- [x] **WE5-S4 · Security pass** (S)
  Recheck: bind warnings (WE1-S1), cookie flags, `X-Content-Type-Options: nosniff` + `X-Frame-Options: DENY`, no secrets in logs/errors, docs' reverse-proxy guidance consistent with the middleware. **The W5 gate.**
  **AC:** a written checklist in the PR, all items ticked.
  **Verify:** curl-level spot checks against a running server.

---

## WE6 — Ship  *(W6)*

Deps: all previous epics.

- [ ] **WE6-S1 · Playwright suite + CI job** (L)
  Against `RSTORRENT_MOCK=1 rstorrent-web` (password auth with a seeded test hash): login → table shows the ten fixtures → filter/search/sort → select → each detail tab → context-menu action → add-magnet flow → simulated disconnect state → sign-out. `npm run e2e`; Linux CI job. Chrome-level screenshot comparison vs the prototype (plan §9 caveat).
  **AC:** suite green and deflakes (retries ≤1); runs < 5 min in CI.
  **Verify:** CI run; deliberate UI break turns it red.

- [x] **WE6-S2 · Contract round-trip test** (S)
  Shared fixtures serialized by the Rust structs must satisfy `src/ipc/types.ts` (vitest structural check of a committed JSON fixture regenerated by a cargo test). Drift fails one side's CI.
  **AC:** removing a DTO field breaks the check.
  **Verify:** mutate a field locally, watch it fail, revert.

- [ ] **WE6-S3 · Perf pass at seedbox scale** (M)
  5 000 mock torrents: serialized `/api/state` size (gzip on), p95 latency, browser frame time while polling. Budget: <150 ms p95 server-side, no dropped frames while scrolling. File follow-up stories (virtualization/delta protocol per plan §11) only if budgets miss.
  **AC:** numbers recorded in the PR; budgets met or follow-ups filed.
  **Verify:** the measurement script committed under `tools/`.

- [x] **WE6-S4 · Packaging + deployment docs** (M)
  Multi-stage Dockerfile (npm build → cargo build → distroless), sample systemd unit, `docs/web-setup.md`: config reference, nginx + caddy TLS snippets, co-location guidance, rtorrent config pointers (reusing `docs/rtorrent-setup.md`), the WE3-S6 QA checklist.
  **AC:** fresh `docker run` against a real daemon serves a working, authenticated UI.
  **Verify:** follow the doc from scratch on a clean machine/VM.

- [x] **WE6-S5 · Repo bookkeeping** (S)
  Root README: web UI section + updated Layout (`Cargo.toml` workspace, `crates/`, `server/`, `web.html`); backlog **B19 → shipped**; web-UI screenshots in `docs/images/` (regenerated from mock). **The W6 gate = plan §9 definition of done.**
  **AC:** README accurate; links resolve; screenshots current.
  **Verify:** read-through + link check.
