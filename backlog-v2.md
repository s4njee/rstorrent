# rstorrent — Product backlog v2

Roadmap for turning rstorrent into a dependable, full-featured desktop and web
client for rtorrent, while preserving the project's compact, power-user design.

This document reorganizes the open work in [backlog.md](backlog.md). That file
remains the historical record of shipped B/C/D items and confirmed rtorrent
methods; this file is the forward-looking product backlog. It intentionally
includes release engineering, safety, accessibility, performance, and recovery
work alongside visible features. A client is not “full featured” if it has every
button but is unreliable with 5,000 torrents or unsafe against a remote daemon.

Last reviewed: 2026-08-19.

---

## 1. Product definition

rstorrent is a **local-first control plane for rtorrent**, not a BitTorrent
engine. It should serve three related use cases through one shared core:

1. **Desktop client** — a polished macOS, Windows/WSL2, and eventually Linux
   application for daily transfer management.
2. **Seedbox client** — secure control of one or more remote rtorrent daemons.
3. **Self-hosted web client** — the same essential workflows from a browser,
   with server-side authentication and no direct exposure of SCGI.

“Full featured” means a user can add, inspect, prioritize, automate, diagnose,
move, back up, restore, and remove torrents without falling back to another GUI
or hand-editing rtorrent state. Advanced rtorrent primitives may remain behind
an expert mode, but ordinary workflows must be discoverable and safe.

### Product principles

- **Daemon truth wins.** The UI reflects rtorrent state and labels values that
  are client-derived, cached, or last-applied.
- **Safe by default.** SCGI stays local; secrets use platform credential stores;
  destructive file actions are scoped, confirmed, and recoverable where possible.
- **Capability-aware.** Local, remote, desktop, web, macOS, Windows, and Linux
  do not pretend to have identical abilities. Unsupported actions are explained.
- **Fast at seedbox scale.** The target is a responsive library at 5,000 torrents,
  not merely a pleasant ten-row demo.
- **Power without mystery.** Expert controls expose their effect, source, and
  persistence behavior. Diagnostics should answer “why is this stalled?”
- **One shared contract.** Desktop and web use the same Rust DTOs and behavioral
  semantics; platform-specific surfaces remain thin adapters.
- **No silent data loss.** Moving, deleting, importing, and restoring are
  transactional or resumable and produce an audit trail.

### Deliberate non-goals

- Embedding or forking a BitTorrent engine.
- A built-in public torrent index or global tracker search.
- General shell execution, `execute.*`, or unrestricted `method.insert` from UI.
- Exposing raw SCGI to a network or treating HTTP Basic Auth without TLS as safe.
- Claiming protocol features the connected rtorrent build does not support.
- DRM bypass, tracker circumvention, or features designed to conceal abuse.

---

## 2. How to use this backlog

### Priority

- **P0 — release blocker:** correctness, data safety, security, or a required
  capability for the next stable release.
- **P1 — core completeness:** expected in a mature general-purpose torrent client.
- **P2 — power user:** valuable depth after core workflows are dependable.
- **P3 — experimental:** differentiating work that must prove value before it
  becomes a supported product surface.

### Size

- **S:** up to two hours
- **M:** up to half a day
- **L:** about a day
- **XL:** must be split into implementation stories before work starts

### Status

- `[ ]` ready or needs refinement
- `[~]` already in progress elsewhere
- `[?]` discovery/prototype; not yet committed product scope

### Required story shape

Before implementation, every XL item and any risky L item must be decomposed in
`tasks.md` or `design/tasks.md` with:

- user-visible outcome and explicit non-goals;
- daemon methods/capabilities verified against supported rtorrent versions;
- behavior matrix for desktop/web and local/remote connections;
- failure, cancellation, restart, and rollback behavior;
- acceptance criteria and an automated or manual verification path;
- telemetry-free performance budget where relevant.

### Definition of done

Unless a story says otherwise, done means:

- Rust format, clippy, workspace tests, TypeScript typecheck, lint, and frontend
  tests pass; changed cross-platform code is covered by its CI target.
- New pure logic has table-driven tests and IPC changes have contract tests.
- Mutations trigger or await authoritative daemon state; the UI does not report
  success solely because a request was sent.
- Disconnected, permission-denied, unsupported, partial-failure, and retry states
  have useful messages.
- Keyboard navigation, focus, accessible names, reduced motion, and 200% zoom are
  considered for new UI.
- Mock mode and fixtures cover the new state without regressing desktop or web.
- User-facing behavior and configuration are documented.

---

## 3. Current baseline

The existing product already covers much more than a basic transfer list. The
historical backlog records these as shipped; they should be regression-tested,
not reimplemented:

- local SCGI/TCP and remote HTTP(S) transports, credential storage, saved
  connection profiles, and reconnecting polling;
- add by file, magnet/URL, association, drag-and-drop, paste, watch folders, RSS,
  and torrent creation;
- start/stop/remove, queue priority, recheck, labels, tracker management, peer
  actions, per-file priority, rate and connection limits, super seeding, and
  safe move-data-on-location-change;
- filters, smart filters, configurable columns, bulk selection, detail tabs,
  pieces and availability bars, transfer charts, error taxonomy, notifications,
  tray controls, and daemon health/session controls;
- scheduling, alternate limits, max-active-download queue, seed goals,
  per-label paths, completion hooks, and sticky provenance metadata;
- a shared Rust core and an in-progress authenticated self-hosted web UI.

The most consequential gaps are now **quality at scale, web completion,
recovery/interoperability, network diagnostics, platform distribution, and
automation depth** rather than basic start/stop functionality.

---

## 4. P0 — stabilize and ship the current product

### FND — foundations, correctness, and scale

- [x] **FND-01 · Large-library virtualization** (P0, L) — virtualize torrent
  rows and bound detail rendering without breaking sticky headers, multi-select,
  context menus, keyboard navigation, or variable-width columns. Preserve
  selection across filtering and polling. **Accept:** smooth scroll and action
  latency under 100 ms with a deterministic 5,000-torrent fixture.

  **Implementation report (2026-08-19):** Fixed-height row windowing keeps DOM
  bounded to ~30–50 rows at 5k (spacer `count * rowHeight` + translated inner;
  `rowHeight` resolved from CSS `--row-height` — 23 px desktop / 25 px web —
  with `ResizeObserver`/`requestAnimationFrame` coalescing and `overscan: 10`).
  Sticky header preserved via existing flex layout (header outside `.body`);
  variable-width columns share `--torrent-grid-template` between header and
  virtual rows. Selection stays hash-based (`Set<string>`) so filtering, sorting,
  and polling preserve it; scroll-to-hash via `CustomEvent("scrollToTorrent")`
  replaces `scrollIntoView` for virtual rows. Keyboard adds Arrow Up/Down +
  Home/End with Shift-range and auto-scroll. Deterministic 5k fixture
  (`xorshift` seeded PRNG) used for perf budget — `selectVisible` 5k sort
  <100 ms verified in tests — and reconcile identity preservation.

  New files:
  - `src/hooks/useVirtualizer.ts` — windowing hook + `resolveRowHeight`
  - `src/demo/largeFixture.ts` — deterministic 5k `Snapshot` generator
  - `src/demo/largeFixture.test.ts` — determinism + <100 ms + reconcile tests
  - `src/hooks/useVirtualizer.test.ts` — helper/range math tests
  - `src/components/table/TorrentTable.test.tsx` — virtualized mount tests

  Modified files:
  - `src/components/table/TorrentTable.tsx` — virtualized body, reset on filter, event-driven scroll
  - `src/components/table/TorrentTable.module.css` — `.virtualSpacer` / `.virtualInner`
  - `src/hooks/useKeyboard.ts` — arrow/Home/End navigation with virtual scroll
  - `src/App.tsx` — notification dispatch via `scrollToTorrent`
  - `src/test/setup.ts` — `IS_REACT_ACT_ENVIRONMENT` + `ResizeObserver` guard

- [x] **FND-02 · Delta snapshot protocol** (P0, XL) — replace full-list churn
  with revisioned add/update/remove deltas while retaining periodic full-state
  reconciliation. Detect a missed revision and self-heal without app restart.
  Use one contract for Tauri events and web HTTP responses. This carries C22.

  **Implementation report (2026-08-19):** Monotonic `revision` added to
  `Snapshot` (rust `crates/rtorrent/src/types.rs:140`, ts `src/ipc/types.ts:118`);
  new shared `SnapshotDelta {revision, baseRevision, added, updated, removed,
  globals, connection}` via `crates/rtorrent/src/delta.rs` (`diff`/`apply` with
  hash-map diff and deterministic ordering). Tauri poller
  (`src-tauri/src/poller.rs:271`) keeps `revision` + `last_snapshot`, emits
  `state://delta` when `tick % 30 != 0` and `continuing_session`, otherwise
  `state://snapshot` for periodic reconciliation; disconnected snapshot bumps
  revision and `AppState::snapshot` (`src-tauri/src/state.rs:64`) backs
  `get_snapshot` command for heal. Server poller (`server/src/poller.rs:113`)
  mirrors revision/delta cache (`CachedDelta`, `prev_snapshot`) and
  `FULL_EVERY=30`; `GET /api/delta?since=` (`server/src/api.rs:331`) returns
  `200` delta, `304` when current, `409` with full snapshot on miss/periodic
  full. One contract: both transports serialize `rtorrent_core::types`. Web
  adapter (`src/ipc/web.ts:151`) maintains `pollRevision`/`pollEtag`/`pollDeltaEtag`,
  polls `state` then `delta`, handles `409` heal. Store
  (`src/store/torrents.ts:37`) tracks `revision`, `applyDelta` returns `false`
  on `baseRevision` mismatch; `App.tsx:67`/`WebApp.tsx:48` listen to both
  `onSnapshot`/`onDelta` and heal via `getSnapshot()` on miss. `304`/`ETag`
  retained for both endpoints.

  New files:
  - `crates/rtorrent/src/delta.rs` — diff/apply + 4 unit tests
  - `src/store/delta.test.ts` — added/updated/removed + miss + heal (3 tests)
  - `server/src/api.rs: delta_serves_incremental_update_and_heals_on_miss` (1 test)

  Modified files:
  - `crates/rtorrent/src/types.rs` — `Snapshot.revision`, `SnapshotDelta`
  - `crates/rtorrent/src/lib.rs` — `pub mod delta`
  - `src/ipc/types.ts` — `Snapshot.revision?`, `SnapshotDelta`
  - `src/store/torrents.ts` — `revision`, `applyDelta` + EMA/removed handling
  - `src/ipc/events.ts` — `onDelta`
  - `src/ipc/commands.ts` — `getSnapshot`
  - `src/ipc/web.ts` — shared pollRevision/delta loop, `__resetPollForTests`
  - `src/ipc/web.test.ts` — revisioned SNAP + delta 304/409/heal tests
  - `src/App.tsx` / `src/web/WebApp.tsx` — dual listeners + heal
  - `src/demo/fixtures.ts` / `src/demo/largeFixture.ts` — `revision:1`
  - `src-tauri/src/state.rs` — `snapshot` cache, `set_snapshot`/`snapshot()`
  - `src-tauri/src/poller.rs` — revision, FULL_EVERY, delta emit, `build_snapshot(revision)`
  - `src-tauri/src/ipc.rs` — re-export `SnapshotDelta`
  - `src-tauri/src/commands.rs` / `src-tauri/src/lib.rs` — `get_snapshot`
  - `server/src/state.rs` — `CachedDelta`, `delta_cache`, `prev_snapshot`, `revision`
  - `server/src/poller.rs` — revisioned assemble/store, delta compute
  - `server/src/api.rs` — `GET /api/delta`, `connecting_snapshot` revision

  Verification: `cargo test --workspace` 70+36 ok (incl. `delta::diff` 4,
  `torrent::types` 1, `api::delta_serves…` 1), `npm test` 152 passed (incl.
  `store/delta` 3, `web` 14), `npm run typecheck` clean, `npm run build` ok.
  Full reconciliation every 30 revisions; missed revision heals without restart
  via 409 snapshot or `get_snapshot`.

- [ ] **FND-03 · Poll scheduling and backpressure** (P0, L) — prioritize active
  transfers and the visible detail, coalesce mutation-triggered refreshes, add
  jitter to slow work, and cap RPC concurrency. A slow daemon must not create an
  unbounded request queue or starve actions.

- [ ] **FND-04 · Durable metadata migration** (P0, M) — version client-owned
  settings, sticky `d.custom` keys, RSS seen state, and local history. Migrations
  must be idempotent, backed up before rewrite, and tested from every previously
  released schema.

- [ ] **FND-05 · Capability handshake** (P0, L) — probe daemon version and
  methods once per connection, cache a capability set, and gate controls from
  it. Show why an action is unavailable. Include rtorrent 0.16.17/0.16.18
  fixtures and a degraded older-daemon fixture.

- [ ] **FND-06 · Mutation idempotency and partial failure UX** (P0, L) — bulk
  actions return per-hash outcomes; retries do not duplicate add/remove/move
  operations; partial failures remain selected and can be retried or copied.

- [ ] **FND-07 · Crash-safe long operations** (P0, XL) — journal move/copy,
  import, export, and remove-with-data operations. On restart, offer resume,
  verify, or safe rollback. Never change rtorrent's path until copied data has
  passed size and content verification.

### WEB — finish the self-hosted web client

- [~] **WEB-01 · Add-flow parity** (P0, L) — finish browser file selection,
  torrent inspection, content selection, drag/drop, magnet clipboard prefill,
  and upload progress using the already-built server endpoints. Carry WE4-S2–S4.

- [ ] **WEB-02 · Browser interaction and visual QA** (P0, M) — complete the
  design-parity gate across supported viewport sizes, keyboard-only use, touch
  targets where appropriate, reconnects, and modal focus behavior. Carry WE2-S8.

- [ ] **WEB-03 · End-to-end test suite** (P0, L) — Playwright coverage for
  login/logout, list/filter/select, add, pause/resume, detail polling, file
  priority, tracker actions, removal confirmation, reconnect, and authorization
  failures. Run against deterministic mock mode in CI. Carry WE6-S1.

- [x] **WEB-04 · Live-daemon certification** (P0, M) — execute and record the
  live rtorrent checklist for read, write, add-file, magnet, tracker/peer/file
  details, removal, daemon restart, and server restart. Carry WE3-S6.
  **Done (2026-09-16, V3-05):** `docs/web-live-cert.md` is the checklist and the
  recorded run; against the bundled rtorrent **0.15.7** the read path, add-file,
  details, pause, remove, daemon restart, server restart, revoke-all, the
  `Secure` cookie and `/ready` vs `/health` all pass. The magnet and
  tracker/peer/file-priority writes remain to be run on a swarm. An ignored test
  (`live_read_paths`, `RSTORRENT_TEST_SOCKET=…`) covers the read path in CI-able
  form.

- [x] **WEB-05 · Persistent secure sessions** (P0, M) — authenticated sessions
  survive server restart using a rotatable server secret; revoke-all remains
  possible. Cookies stay HttpOnly, Secure behind HTTPS, SameSite=Strict, and
  scoped to the application path.
  **Done (2026-09-16, V3-05):** tokens are stored **hashed** (SHA-256) with
  expiry in `session-store.json` beside the config, so a restart keeps logins and
  a leak of the file is not a set of live logins; `Sessions::revoke_all`
  (`DELETE /api/sessions`) is the rotatable "sign out everywhere" control. The
  cookie is `HttpOnly; SameSite=Strict; Path=/`, plus `Secure` when a trusted
  proxy forwards `X-Forwarded-Proto: https` or `secure_cookies = true`.

- [x] **WEB-06 · Deployment hardening** (P0, L) — trusted-proxy configuration,
  request/body/time limits, upload temp-file cleanup, structured security logs,
  graceful shutdown, `/ready` vs `/health`, and documented backup/restore of
  configuration. Refuse unsafe non-loopback configurations with actionable errors.
  **Done (2026-09-16, V3-05):** `trusted_proxies` gates `X-Forwarded-For`
  (real client IP for login rate limiting) and `X-Forwarded-Proto` (Secure
  cookie); the api router keeps the 10 MiB body cap and gains a 30 s request
  timeout; uploads are in-memory with nothing to clean up; auth events log
  structured fields (`auth.login`/`auth.logout`/`auth.revoke_all`/`auth.csrf`);
  the CLI handles SIGTERM with a bounded drain; `/health` (liveness) and `/ready`
  (readiness) are unauthenticated; and a non-loopback bind is refused unless
  secure cookies are configured, with an error naming the fix. `docs/web-setup.md`
  documents the proxy headers, systemd hardening and backup/restore.

- [ ] **WEB-07 · Seedbox scale pass** (P0, L) — measure state response size,
  server CPU/RAM, DOM nodes, and interaction latency at 1k/5k torrents; set and
  enforce budgets. Carry WE6-S3 and depend on FND-01/FND-02.

### REL — release quality and distribution

- [x] **REL-01 · Accessibility baseline** (P0, L) — complete E13-S4: semantic
  tables/trees/dialogs, logical tab order, visible focus, screen-reader names,
  keyboard equivalents for pointer actions, non-color status cues, reduced
  motion, and contrast validation. **Accept:** an automated axe pass with no
  violations over everything a user can open.

  **Implementation report (2026-09-16):** The torrent table is a `role="grid"`:
  rows and cells carry their roles, sortable headers carry `aria-sort`, and
  `aria-rowcount`/`aria-rowindex` keep rows locatable while the list is
  filtered. The Files pane is a `role="tree"` (items with level, expanded and
  selected state), the detail tabs are a tablist whose panel is named by the
  selected tab, and the modal shell is `role="dialog"` with `aria-modal`, a
  focus trap and focus restoration.

  What the audit found that reading markup had missed: the sidebar's filter rows
  and the file-priority chip were click-only `div`s, so no keyboard path to them
  existed at all — both are now buttons (`aria-pressed` for the filters); the
  modal's close control had the same problem; and the magnet dialog's fields
  were labelled by unassociated `<span>`s, so a screen reader announced nothing
  (the add dialog's had the same shape, hidden behind a placeholder). Those
  fields are now real `<label>`s bound to their controls.

  Focus: the global `:focus-visible` ring stays, and the top bar's filter no
  longer swallows its own outline — it shows the ring on the wrapper. Reduced
  motion: nothing honoured `prefers-reduced-motion`, so it now zeroes
  `--t-progress` and any transition added later. Contrast was already enforced
  (165/165 pairs, 5 themes, 0 literals outside the palette). No positive
  `tabindex` exists anywhere, so tab order is DOM order.

  **Verify:** `src/web/a11y.test.tsx` — axe-core over the mounted console and all
  nine dialogs it can open, in `npm test` and therefore in CI;
  `npm run check:contrast`; 292 frontend tests, typecheck and lint green.

  **Left over:** the row context menus are still pointer-only (every verb they
  carry is reachable from the toolbar and menu bar), and the console's other two
  routes cannot be audited until the router lands (WC7-S1). The keyboard-only
  and screen-reader passes are manual and remain to be signed off alongside
  WC10-S3.

- [ ] **REL-02 · Release QA matrix** (P0, M) — complete E13-S5 with clean-user,
  upgrade, reconnect, sleep/wake, offline, high-DPI, multi-monitor, and remote
  daemon cases. Archive versions, OS/build details, and results with each release.

- [ ] **REL-03 · macOS signing, notarization, and auto-update** (P0, XL) —
  Developer ID signing, hardened runtime, notarized DMG, signed update manifest,
  rollback guidance, and staged update rollout. Completes E14-S2 and B7.

- [ ] **REL-04 · Windows/WSL2 production pass** (P0, XL) — installer, code
  signing, first-run WSL2/rtorrent detection, path translation edge cases,
  firewall guidance, protocol/file associations, credential storage, update
  channel, and clean-machine QA.

- [ ] **REL-05 · Backup before upgrade** (P0, M) — before schema-changing
  releases, snapshot app settings and client-owned metadata; explain where the
  backup lives and provide one-click restore. Do not copy arbitrary payload data.

- [ ] **REL-06 · Dependency and security maintenance** (P0, M) — automated
  Rust/npm advisory scanning, license allowlist, pinned release inputs, secret
  scanning, and a documented disclosure/update policy. Findings that can lead to
  remote code execution, credential disclosure, or data loss block release.

---

## 5. P1 — core feature completeness

### LIB — library organization and daily workflow

- [~] **LIB-01 · Categories and many-to-many tags** (P1, XL) — keep the current
  single label compatible with ruTorrent while adding client-managed tags,
  colors/icons, bulk editing, tag filters, and optional tag-derived automation.
  Define how tags sync across connection profiles and web clients.
  **V3-10 done in both shells (2026-09-16):** tags in `d.custom=tags`
  (normalised, delta-aware) with `set_tags`/`add_tags`/`remove_tags`, and a Tags
  sidebar group, filter facet, search, coloured chips and bulk editor in the web
  console **and** GPUI. **Sync** is the daemon session: `d.custom` is
  per-torrent daemon state, so profiles and the web UI agree without a client
  store. **Open (LIB-01's extras, not V3-10's AC):** user-configurable colours,
  per-tag icons, and optional tag-derived automation (**V3-33**).

- [ ] **LIB-02 · Saved views 2.0** (P1, L) — editable native-view membership,
  user-named smart views, nested AND/OR conditions, negative matches, relative
  time filters, and pinned views. Show whether a view is daemon-backed or local.

- [x] **LIB-03 · Full-text local library search** (P1, L) — search torrent name,
  hash, tracker, tag, save path, and contained filenames without fetching every
  file list during each query. Maintain a bounded local index per connection.
  **Done (V3-12, 2026-09-16):** `rtorrent_core::file_index::FileIndex` (bounded,
  lazy, per-connection) across core + web + GPUI; hash and save path are matched
  from the DTO, filenames from the index.

- [x] **LIB-04 · Duplicate detection before add** (P1, M) — detect existing
  info hash, likely duplicate content, and duplicate save destination. Offer
  focus-existing, merge trackers, recheck/reseed, or cancel; never silently add.
  **Done (V3-13, 2026-09-16):** core + web + GPUI; an exact match blocks the add
  and offers Show it / Merge trackers. Recheck/reseed on the existing torrent is
  the ordinary Recheck action, not part of the add flow.

- [ ] **LIB-05 · Content quick actions** (P1, L) — Quick Look/open, reveal,
  copy path, copy relative path, and open containing folder from Content. Gate
  local filesystem actions on co-location. Carries C19.

- [ ] **LIB-06 · Rename torrent and files safely** (P1, XL) — distinguish the
  display name from on-disk paths; support single-file/folder rename only where
  the daemon/filesystem can do it safely. Stop, validate containment, move,
  update state, recheck, and resume transactionally.

- [ ] **LIB-07 · Move-on-complete rules** (P1, L) — destination by tag/label,
  free-space preflight, collision policy, cross-volume verification, progress,
  cancellation, and recovery. Reuse the move journal from FND-07. Carries C10.

- [ ] **LIB-08 · Incomplete-data location** (P1, L) — configure a partial-data
  path and atomic completion transition, while clearly separating app-managed
  moves from rtorrent configuration. Handle multi-file torrents and restarts.

- [x] **LIB-09 · Session export/import** (P1, L) — landed as V3-22
  (backlog-v3): portable `rstorrent-session/1` manifest, dry-run validation,
  conflict report, selective restore, journaled import. Never credentials.
  Carries C23.

- [x] **LIB-10 · Import from other clients** (P1, XL) — landed as V3-22
  (backlog-v3): read-only qBittorrent + Transmission discovery into the same
  manifest shape, path mapping, duplicate resolution, stopped add with
  recheck before resume. Carries B21.

### QUE — queueing, priorities, and bandwidth

- [x] **QUE-01 · Complete queue policy** (P1, XL) — landed as V3-17
  (backlog-v3): per-class + total caps, slow exemption, force-start, queued
  display, all shells.

- [x] **QUE-02 · Stable manual queue order** (P1, L) — landed as V3-17
  (backlog-v3): top/up/down/bottom over `(priority, queue_pos)` swaps, bands
  never "#" slots. (Drag reorder stays out — same honesty rule.)

- [ ] **QUE-03 · First/last pieces and sequential mode** (P1, L) — torrent-level
  controls with explicit compatibility probing and warnings about swarm health.
  Persist the intended mode and show when the daemon cannot honor it.

- [x] **QUE-04 · Bandwidth rules by label/tag** (P1, L) — landed as V3-18
  (backlog-v3): rule engine with adoption markers, precedence display, editors
  in both shells.

- [x] **QUE-05 · Rich scheduler** (P1, XL) — landed as V3-18
  (backlog-v3): weekly grid, pause-vs-limit, DST-correct wall-clock evaluation,
  temporary override, next-change preview, legacy import.

- [ ] **QUE-06 · Storage-aware admission** (P1, L) — before starting queued
  downloads, reserve required free space by filesystem and surface blocked-by-
  space state. Never treat sparse/preallocated size as actually free.

### NET — network, trackers, and peers

- [ ] **NET-01 · IP blocklist manager** (P1, L) — import P2P-format files,
  normalize/merge ranges, preview source/count/date, apply atomically, and report
  rejected entries. Support refresh from an explicitly configured HTTPS URL with
  size/time limits. Carries D10.

- [ ] **NET-02 · Port and reachability diagnostics** (P1, L) — show listening
  address/port, incoming-peer evidence, tracker reachability, DHT status, bind
  interface state, and likely causes. External port tests are opt-in and disclose
  the endpoint contacted.

- [ ] **NET-03 · VPN binding guard** (P1, L) — resolve the configured interface,
  show current addresses, warn when it disappears, and optionally stop active
  torrents until it returns. This is a control-plane guard, not a claim that the
  app itself can prevent every packet leak.

- [ ] **NET-04 · Tracker editor 2.0** (P1, L) — tier reordering, multi-line
  import/export, dedupe/normalize URLs, batch edit, copy announce URL, failure
  history, and per-tier status. Preserve private-torrent warnings.

- [ ] **NET-05 · Peer diagnostics** (P1, M) — sortable country/ASN when a local
  offline database is installed, protocol/transport, choke/interest state,
  request rates, and per-peer totals where supported. Geolocation is optional,
  local, and never required for core behavior.

- [ ] **NET-06 · Proxy test and limitation report** (P1, M) — test configured
  tracker HTTP proxy without logging credentials; state clearly that UDP
  trackers, DHT, and peers bypass it. Prevent users from mistaking it for a VPN.

### AUT — automation without arbitrary shell exposure

- [ ] **AUT-01 · Rules engine** (P1, XL) — event/condition/action automation for
  added, metadata-ready, started, completed, ratio reached, error, idle, and low
  disk. Conditions cover tracker, private flag, size, label/tag, source, time,
  and path. Actions are allowlisted and previewable; rules support dry-run and
  an execution log.

- [x] **AUT-02 · RSS rules 2.0** (P1, XL) — landed as V3-23
  (backlog-v3): core matching engine + Tauri rebuild + first GPUI runner,
  explained previews, exportable seen-set.

- [ ] **AUT-03 · Watch-folder rules 2.0** (P1, L) — recursive option, filename
  filters, stability delay, post-processing states, duplicate handling, and an
  error inbox. Never consume a partially copied `.torrent` file.

- [ ] **AUT-04 · Completion integrations** (P1, L) — replace opaque command-only
  hooks with optional allowlisted HTTP webhook and desktop notification actions.
  Redact secrets, cap retries, sign webhooks, and show delivery history. Keep the
  existing direct-exec hook expert-only with an explicit trust warning.

- [ ] **AUT-05 · Automation simulator** (P1, M) — run a rule set against the
  current library or saved fixtures without performing actions; explain each
  condition and the winning precedence.

### OBS — diagnostics, history, and supportability

- [ ] **OBS-01 · “Why isn’t this downloading?” inspector** (P1, XL) — combine
  torrent state, tracker failures, peer count, piece availability, file
  priorities, queue policy, rate limits, disk space, bind interface, and recent
  events into ranked, actionable explanations. Every conclusion links to the
  underlying evidence and suggested safe action.

- [ ] **OBS-02 · Persistent transfer history** (P1, L) — optional bounded local
  history across restarts with configurable retention, totals by torrent/tag/
  day, compact storage, export, and clear-all. Off by default on shared web hosts.

- [ ] **OBS-03 · Activity and audit log** (P1, L) — unified chronological record
  of user actions, automation actions, daemon transitions, and file operations;
  filter by torrent/action/outcome and export a redacted support bundle.

- [ ] **OBS-04 · Log tab upgrade** (P1, M) — severity filters, search, copy,
  wrap, pause-follow, timestamps/timezone, torrent scoping, and a bounded buffer.
  Carries C25.

- [ ] **OBS-05 · Support bundle** (P1, M) — explicit preview before export;
  include versions, capability set, sanitized settings, recent redacted logs,
  and health metrics. Exclude passwords, auth cookies, full paths by default,
  torrent names, magnet links, and payload data unless separately opted in.

- [ ] **OBS-06 · Raw XML-RPC console** (P1, L) — hidden expert tool with method
  autocomplete, typed arguments, result formatting, history, and capability
  help. Read-only by default; a per-session mutation arm blocks `execute.*` and
  `method.insert` regardless. Carries D15.

---

## 6. P1 — platform and product parity

### PLT — desktop platforms and native integration

- [ ] **PLT-01 · Linux desktop release** (P1, XL) — AppImage or Flatpak first,
  tested on current Ubuntu and Fedora; system tray behavior, opener/dialogs,
  secret storage, magnet/file associations, local socket permissions, sandbox
  filesystem portals, packaging, and updates. Completes the Linux part of B18.

- [ ] **PLT-02 · Start/supervise local rtorrent** (P1, XL) — opt-in managed
  daemon profile: discover existing service, validate config, start/stop/restart,
  show logs, and never overwrite a hand-managed `.rtorrent.rc`. Platform-native
  service integration only after a safe discovery design. Carries C20.

- [ ] **PLT-03 · Native file integration** (P1, M) — complete file association,
  magnet handler, drag/drop, Finder/Explorer/file-manager reveal, Quick Look/open,
  and recent-items behavior on every supported desktop platform.

- [ ] **PLT-04 · Power and network transitions** (P1, L) — deterministic behavior
  across suspend/resume, interface change, VPN reconnect, metered connection,
  clock change, and daemon address changes; recover polling without duplicate work.

- [ ] **PLT-05 · Homebrew cask and package feeds** (P1, M) — reproducible cask,
  checksum/update automation, install/uninstall verification, and release docs.
  Carries C24; analogous package metadata belongs with Windows/Linux releases.

### UX — polish, themes, language, and onboarding

- [ ] **UX-01 · First-run connection wizard** (P1, L) — choose local/WSL2/
  remote, discover likely sockets/services, test connection, explain SCGI safety,
  and offer setup docs without blocking expert manual configuration.

- [ ] **UX-02 · Command palette** (P1, L) — searchable actions, torrents, views,
  connection profiles, and settings with keyboard-first execution and discoverable
  shortcuts. Capability and selection state must gate commands consistently.

- [ ] **UX-03 · Customizable shortcuts** (P1, M) — conflict detection, platform
  conventions, reset/export, and discoverable display in menus/tooltips.

- [ ] **UX-04 · Light and system themes** (P1, L) — a complete token set, system
  following, contrast validation, chart/piece-map parity, and no component-local
  color exceptions. Carries B22.

- [ ] **UX-05 · Localization foundation** (P1, XL) — extract strings, plural and
  number/date/size formatting, layout stress tests, translator notes, fallback
  behavior, and an initial pseudo-locale before accepting translations. Carries B20.

- [ ] **UX-06 · Responsive density and zoom** (P1, M) — compact/comfortable row
  density, text zoom to 200%, narrow-window column priority, and persisted layout
  profiles without clipping dialogs or detail tables.

- [ ] **UX-07 · Notification center** (P1, M) — in-app history for completion,
  error, connection, automation, and low-space notifications; per-category
  controls, quiet hours, and jump-to-torrent actions.

---

## 7. P2 — power-user depth

- [ ] **PWR-01 · Native view editor** (P2, L) — add/remove torrent membership,
  create/delete views where supported, and bulk operations scoped to a view.

- [ ] **PWR-02 · Choke-group profiles** (P2, XL) — guided presets and an advanced
  editor for `protocol.choke_heuristics.*` with live validation, rollback, and
  clear scope. Do not expose raw scheduling syntax as the default UX. Carries D14.

- [ ] **PWR-03 · Daemon configuration diff** (P2, XL) — compare desired settings,
  last-applied settings, live readable values, and `.rtorrent.rc` managed blocks.
  Preview changes and restart requirements; never rewrite unrelated config.

- [ ] **PWR-04 · Multi-daemon dashboard** (P2, XL) — simultaneous connection to
  several profiles, aggregate status, per-daemon filters, and explicit action
  target. Isolation is mandatory: hashes, tags, history, and credentials are
  namespaced by daemon identity.

- [ ] **PWR-05 · Cross-daemon handoff** (P2, XL) — export torrent metadata and
  state from one daemon, path-map to another, validate data availability, add
  stopped, recheck, then optionally remove the source. No payload transfer is
  implied unless a separately reviewed transport is configured.

- [ ] **PWR-06 · Session snapshots** (P2, L) — named, encrypted-at-rest backups
  of client configuration plus selected rtorrent session metadata, with diff,
  retention, dry-run restore, and portability warnings.

- [ ] **PWR-07 · Tracker policy sets** (P2, L) — reusable tracker tiers for new
  torrents, allow/deny rules, private-torrent safeguards, and batch remediation
  with a preview.

- [ ] **PWR-08 · Torrent v2/hybrid visibility** (P2, M) — detect and display
  v1/v2/hybrid metadata and hashes in parsing, creation, copy, and General. Actual
  transfer support remains whatever the connected rtorrent build supports and is
  capability-gated.

- [ ] **PWR-09 · Reproducible headless CLI** (P2, XL) — a small companion CLI
  using the shared Rust core for scripted list/add/start/stop/tag/export actions,
  structured JSON output, profiles, and the same credential/capability rules.
  It must not become an alternate uncontrolled RPC surface.

- [ ] **PWR-10 · Extension API discovery** (P2, M) — determine whether signed,
  permissioned extensions can add importers, notifications, and metadata panels
  without arbitrary code in the app process. Ship nothing until threat model,
  versioning, and revocation are credible.

---

## 8. P3 — experimental differentiators

Experimental work starts behind a build flag or explicit Labs toggle, stores no
new sensitive data without consent, and includes a success metric plus a removal
plan. A prototype graduates only after it works on real libraries and has a
maintainable non-AI fallback where relevant.

- [?] **LAB-01 · Swarm Health Map** (P3, XL) — evolve the availability bar into
  a time-aware heatmap: rare pieces, availability trend, peer churn, stalled
  regions, and estimated recoverability. The primary value is diagnosis, not a
  decorative animation. **Graduate if:** users can identify a bad/dead swarm
  materially faster than with tracker/peer counts alone.

- [?] **LAB-02 · Explainable torrent doctor** (P3, XL) — an offline rules-first
  assistant that turns OBS-01 evidence into a stepwise repair plan, can simulate
  safe actions, and never executes a destructive fix without confirmation. An
  optional language model may summarize already-redacted evidence, but diagnosis
  must remain inspectable and useful without it.

- [?] **LAB-03 · Adaptive bandwidth controller** (P3, XL) — learn from latency,
  active application/network state, schedules, and historical throughput to keep
  transfers fast without saturating interactive traffic. Local processing only;
  hard user caps always win; every automatic change is visible and reversible.

- [?] **LAB-04 · Policy packs** (P3, L) — shareable, human-readable bundles of
  labels/tags, queue limits, save-path templates, RSS/watch rules, seed goals,
  and notifications. Import shows a complete diff and strips secrets/absolute
  paths. Candidate packs: private tracker, archival seedbox, laptop, media lab.

- [?] **LAB-05 · Content-aware library graph** (P3, XL) — local-only similarity
  using filenames, sizes, directory shape, and hashes to find duplicates, partial
  duplicates, renamed releases, and payloads that can be reseeded. Never inspect
  file contents by default. **Graduate if:** it prevents meaningful duplicate
  storage or makes reseeding easier on large libraries.

- [?] **LAB-06 · Storage placement advisor** (P3, XL) — recommend destination
  volumes from free space, historical throughput, failure risk, label policy,
  and seeding needs; optionally generate a move plan. It advises first and may
  automate only through the crash-safe move journal.

- [?] **LAB-07 · Privacy posture dashboard** (P3, L) — an honest, evidence-based
  report of bind state, proxy scope, encryption preference, DHT/PEX, public IP
  test consent, remote-server TLS, credential storage, and exposed interfaces.
  Avoid a misleading single “anonymous” score.

- [?] **LAB-08 · Collaborative household/seedbox inbox** (P3, XL) — permissioned
  web requests to add a torrent, with approval, quotas, per-user attribution,
  expiry, and audit history. Requires real multi-user auth and isolation; it is
  not an extension of the current single-password mode.

- [?] **LAB-09 · Time-travel operations view** (P3, XL) — reconstruct why a
  torrent changed state from bounded event history and show the exact user,
  automation, daemon, network, or disk cause. Redaction and retention controls
  are prerequisites.

- [?] **LAB-10 · Reproducible torrent recipe** (P3, L) — export a portable,
  signed recipe describing creation inputs, tracker tiers, piece size, private
  flag, source tag, and resulting hashes, without bundling content. Useful for
  release engineering and archival workflows; validate demand before committing.

- [?] **LAB-11 · Seed sustainability score** (P3, L) — rank personal seeding
  attention using age, ratio, swarm availability, tracker health, last activity,
  and storage pressure. Always show the formula and inputs; never auto-delete on
  the score alone.

- [?] **LAB-12 · Remote operation approval links** (P3, XL) — short-lived,
  single-action approvals for trusted collaborators without exposing the full
  web client. Requires signed capabilities, strict scope, revocation, expiry,
  rate limits, and an audit trail. Threat-model before prototype.

---

## 9. Suggested release sequence

Release numbers are illustrative; scope exits on quality gates, not calendar dates.

### 2.1 — dependable core

FND-01, FND-03–FND-06, REL-01, REL-02, REL-05, OBS-04, and the remaining live
verification of recently shipped controls. Exit gate: clean upgrade plus 5,000-
torrent mock performance with no known data-loss or bulk-action correctness bugs.

### 2.2 — web and seedbox

WEB-01–WEB-07 plus FND-02. Exit gate: authenticated browser E2E suite, hardened
deployment, live-daemon certification, and documented backup/recovery.

### 2.3 — organize and automate

LIB-01–LIB-04, LIB-07, QUE-01/04/05, AUT-01–AUT-03, and OBS-01/03. Exit gate:
rules are previewable/auditable and scheduler precedence is deterministic.

### 2.4 — portable library

LIB-05/06/08–LIB-10, FND-07, PWR-06, and PLT-03. Exit gate: interrupted move,
import, export, and restore scenarios recover without silent data loss.

### 3.0 — supported everywhere

REL-03/04, PLT-01, PLT-04/05, UX-01/04–UX-06, and localization/a11y release
gates. Exit gate: signed and updatable macOS/Windows builds plus a supported
Linux package, each tested on a clean system.

### Labs trains

Prototype at most two LAB items at once. Good first candidates are LAB-01
(builds directly on existing availability data) and LAB-07 (turns current
network preferences into honest diagnostics). A lab that misses its success
criterion after two iterations is documented and removed rather than left as
permanent unfinished UI.

---

## 10. Dependency map

```text
capability handshake (FND-05)
  ├─ network diagnostics (NET-02/03/06)
  ├─ advanced protocol controls (QUE-03, PWR-02, PWR-08)
  └─ honest platform/web action gating

virtualization (FND-01) ─┐
delta snapshots (FND-02) ├─ web scale pass (WEB-07)
backpressure (FND-03) ───┘

crash-safe operation journal (FND-07)
  ├─ move-on-complete and incomplete paths (LIB-07/08)
  ├─ file rename (LIB-06)
  ├─ import/export and cross-daemon handoff (LIB-09/10, PWR-05)
  └─ storage advisor automation (LAB-06)

activity/audit evidence (OBS-03)
  ├─ automation engine and simulator (AUT-01/05)
  ├─ torrent doctor (OBS-01, LAB-02)
  └─ time-travel view (LAB-09)

single-user web hardening (WEB-05/06)
  └─ real multi-user design
       ├─ collaborative inbox (LAB-08)
       └─ approval links (LAB-12)
```

---

## 11. Backlog hygiene and decision gates

- `backlog.md` remains the shipped-method ledger; mark completion there when a
  carried item lands, and link to its implementation story.
- This file owns product priority and release grouping. Avoid adding method-level
  wishlist items unless they enable a stated user outcome.
- `tasks.md` owns desktop/shared implementation stories; `design/tasks.md` owns
  web-host implementation stories. Cross-cutting work should have one lead story
  and linked adapter stories, not duplicated acceptance criteria.
- Review P0/P1 monthly. Review P2/P3 at release boundaries. Remove ideas that no
  longer fit the product definition rather than keeping an eternal icebox.
- Any feature involving remote fetches, filesystem mutation, credentials,
  multi-user access, shell hooks, or extension code requires a threat-model note.
- Any feature that depends on an unverified rtorrent method begins with a probe
  against the supported-version matrix. Method presence alone is not enough;
  persistence, error behavior, and restart behavior must also be tested.
- Experimental features graduate only with documented demand, measurable benefit,
  acceptable maintenance cost, and a path that does not weaken the safe defaults.
