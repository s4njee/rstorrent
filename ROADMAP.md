# Roadmap: rstorrent on Windows

The goal is a Windows app a stranger can download, run, and torrent with,
without installing rtorrent, a Linux distro, or anything beyond WSL itself.

This file orders the work. [windows.md](windows.md) is the hands-on companion:
how the Windows build works, the code map, and the step-by-step bring-up
checklist. [GPUI.md](GPUI.md) covers the desktop shell as a whole.

## Where things stand (2026-09-19)

- **Design settled.** rtorrent can't be built natively for Windows, so the app
  ships a static Linux rtorrent 0.15.7 as an 8 MB WSL root filesystem. On first
  start it imports that as its own distro, `rstorrent`, launches rtorrent there
  and connects over `127.0.0.1:5000`. Paths are translated at the boundary
  (`C:\…` ↔ `/mnt/c/…`). windows.md §1 has the details.
- **Code written, never run on a Windows desktop.** The WSL bridge, daemon
  lifecycle, path translation, toasts, exe icon, packaging script, and release
  workflow all exist. All of it was written on a Mac.
- **First real Windows compile: passed.** CI run 35426607319 (`gpui-windows`)
  built and linked the app on `windows-latest`. That proves the shader compiler
  (`fxc`), the resource compiler (`rc`), and our icon/version resource work
  next to gpui's manifest. 184 tests passed and 3 failed (Phase 0).
- **Runtime proven on Linux, not under `wsl --import`.** The rootfs was
  smoke-tested in a chroot on leo: SCGI answers, and an HTTPS torrent loads.

Each phase below ends in something you can check. Don't start a phase's polish
until the previous phase's exit test passes.

---

## Phase 0: Green CI

*All of this can be done from the Mac. Nothing here needs a Windows desktop.*

| # | Item | Notes |
|---|---|---|
| 0.1 | Fix `create_torrent::suggested_output` | **A real bug**, caught by `the_output_is_suggested_from_the_source_name`. It joins a *daemon* path with `Path::join`, which produces `/torrents\debian.iso.torrent` on Windows. Daemon paths are always `/`-separated, so build them as strings. Audit other `Path::join`/`PathBuf` uses on daemon-namespace paths (`policy.rs`, `localfs::move_torrent_data`, `services.rs`). |
| 0.2 | Fix `prefs::tests::transport_kind_follows_the_live_document` | The test assumes the default transport is a Unix socket; on Windows it is TCP. Make the test set its transport explicitly. |
| 0.3 | Fix `policy::tests::completed_torrent_with_final_dir_moves_home` | "the move did not finish": on Windows `localfs::resolve` now sends temp-dir paths through WSL translation, so the fixture paths don't resolve. Give the test a resolver seam, or gate it to unix and add a Windows equivalent using `/mnt/c` fixtures. |
| 0.4 | Fix the Linux `crate + server` job | `server/src/assets.rs` embeds `../dist-web` at compile time, but the job runs clippy before `npm run build:web`. Move the web build first. (This job was already failing before the Windows work.) |
| 0.5 | Fix the `e2e` job | It was failing before the Tauri removal too; the frontend agent's local run passed 13/13. Read the Playwright report artifact and fix whatever is CI-specific. |
| 0.6 | Clear clippy warnings, then add `-D warnings` to both `gpui-*` jobs | The warnings are in `policy.rs`, `model.rs`, `web_host.rs`, `detail_pieces.rs` and `wsl.rs:205`. |

**Exit test:** CI is green on `main` for all five jobs.

## Phase 1: First run on the Windows machine

*Needs the Windows desktop. This phase is windows.md §4 steps 1–9; record
results in its §8 findings log.*

| # | Item | Notes |
|---|---|---|
| 1.1 | Toolchain | Rust MSVC, VS Build Tools + Windows SDK, Node (windows.md §2) |
| 1.2 | Build the runtime in WSL on the machine | `sudo tools/build-rtorrent-wsl.sh`, or copy it from the Mac |
| 1.3 | `cargo run -p rstorrent-gpui` | A window opens and renders on Direct3D 11; fonts and icon are correct; no console window in release builds |
| 1.4 | `--start-rtorrent` | First real `wsl --import`. Verify the distro, `/root/.rtorrent.rc`, and that the port answers within 30 s |
| 1.5 | Autostart from the GUI | With no daemon running, the app brings one up and the table connects |
| 1.6 | The core loop | Add a magnet → download → pause/resume → reveal in Explorer → remove with delete-to-Recycle-Bin → Set Location with a picked `C:\…` folder → free-space readout |
| 1.7 | Lifetime | Quit the app: the daemon survives. Reopen: no second daemon. Reboot: autostart brings it back with the session intact |

Expect to fix things here. The likeliest trouble spots:
- `wsl --status` as the "is WSL installed" probe
- UTF-16 decoding of `wsl.exe` output
- the 30 s start timeout on a cold VM
- a `--cd ~` quirk in the launch command

**Exit test:** a legal test torrent downloads to `Downloads` and seeds, driven
entirely from the GUI, on a machine where nobody installed rtorrent.

## Phase 2: Make it trustworthy

*Three unknowns decide whether the WSL design holds up under real use. Answer
them before polishing anything.*

| # | Question | How to answer | If the answer is bad |
|---|---|---|---|
| 2.1 | **Is `/mnt/c` fast and safe enough?** rtorrent writes through mmap, and drvfs/9p is slow and has had mmap quirks. | Download a multi-GB torrent to `/mnt/c/…`; measure throughput against a VM-local directory; force a recheck and confirm zero bad pieces. | Default downloads to a directory inside the VM (shown in Explorer as `\\wsl.localhost\rstorrent\…`), warn that unregistering the distro deletes it, and offer a "move to Windows folder on completion" rule (the move machinery already exists). |
| 2.2 | **Can peers connect in?** WSL's default NAT networking hides the listen port, so only outgoing connections work. | Check the listen port from outside with a port checker, and compare swarm speeds. | Detect and recommend mirrored networking (Windows 11 22H2+, `.wslconfig`), with a one-time elevated helper for the Hyper-V firewall rule. Fall back to documenting outgoing-only. Show the state in the status bar either way. |
| 2.3 | **Does the daemon survive?** WSL shuts idle distros down, and our `wsl.exe` child is what keeps it alive. | Leave it seeding overnight with the app closed; sleep and wake the machine; log out and back in. | Keep-alive options, cheapest first: a detached `wsl.exe` holder process; a Task Scheduler entry at logon; `vmIdleTimeout=-1` guidance. |

Also in this phase:
- **2.4 Existing daemon detection.** If the user already runs the Tauri-era
  rtorrent in their own distro on port 5000, prefer it. Don't import ours on top
  of it (windows.md §5 item 4).
- **2.5 Runtime upgrades.** Compare `/etc/rstorrent-runtime` in the imported
  distro with the shipped tarball. Re-import when stale, keeping
  `/root/.rtorrent.rc` and the session directory.
- **2.6 Failure messages.** Every way a start can fail (no WSL, virtualization
  off, import failed, port taken, daemon exited) should put a specific,
  actionable message on the disconnected card. Test each one deliberately.
- **2.7 Remaining path translation.** Watch-folder save paths and move-rule
  destinations edited in Preferences aren't translated yet. Run them through
  `localfs::to_daemon_path` on save.

**Exit test:** a week of real use on the Windows machine with no data loss, no
orphaned or duplicate daemons, and a written answer to 2.1–2.3 in windows.md.

## Phase 3: Make it installable

| # | Item | Notes |
|---|---|---|
| 3.1 | **Installer** (per-user; NSIS or WiX) | Installs the exe and `runtime\`; adds a Start-menu shortcut carrying the `com.rstorrent.app` AppUserModelID (this also fixes toast attribution). The uninstaller offers `wsl --unregister rstorrent` and warns that it deletes anything stored inside the VM. |
| 3.2 | **WSL onboarding** | When WSL is missing, the disconnected card explains it and offers a button that runs `wsl --install --no-distribution` elevated, then handles the "reboot needed" state on the next launch. |
| 3.3 | **`.torrent` / `magnet:` association** | Registry entries from the installer; argv routing into the add dialogs; single-instance forwarding (named mutex + pipe). Shares GPUI.md backlog item 5. |
| 3.4 | **Console output for CLI flags** | Call `AttachConsole(ATTACH_PARENT_PROCESS)` so `--where-rtorrent` and `--start-rtorrent` print from a release exe. |
| 3.5 | **Release pipeline proven** | Run `release.yml` via `workflow_dispatch`. This is the first time the `wsl-runtime` job runs on a GitHub runner; it was only tested on leo. Confirm the zip and installer contents, then cut a `v0.2.0` tag. |
| 3.6 | **Code signing** | Without it, SmartScreen blocks the installer for most users. Azure Trusted Signing is the cheap route. Decide whether this gates a public release. |

**Exit test:** on a clean Windows 11 machine with no WSL, go from download to a
seeding torrent in one sitting, using only the installer and the app's prompts.

## Phase 4: Parity and polish

The parity gaps at the top of the [GPUI.md](GPUI.md) backlog apply to Windows
too. Each has a Windows-specific side:
- **Tray** → the notification-area icon (rates, pause/resume all, quit).
- **Drag & drop** → `.torrent` files dropped from Explorer.
- **Paste-to-add** → Ctrl+V on the main window.
- **Watch folders** → a `/mnt/c/…` folder must be watched from the Windows
  side (`ReadDirectoryChangesW` via `notify`); inotify doesn't see drvfs changes.
- **Toast click-to-select** → move to gpui's `App::show_system_notification`.

Windows conventions to sweep in the same pass:
- Ctrl-based shortcuts, shown correctly in menus
- title-bar and menu placement
- high-DPI and multi-monitor behaviour
- dark-mode title bar
- `%APPDATA%\com.rstorrent.app\settings.json`, plus a first-run migration for
  anyone coming from the Tauri build's settings (same path, so verify it loads)

**Exit test:** `docs/qa-checklist.md` passes on Windows, with no "not ported
yet" message reachable.

---

## Out of scope for now

- **A native Windows rtorrent.** It would need months of porting libtorrent's
  event loop, plus a permanent fork. Cygwin would avoid WSL, but it means
  maintaining a `poll()` backend against every upstream release. Revisit only
  if requiring WSL proves to be a deal-breaker (windows.md has the research).
- **ARM64 Windows.** The runtime and the exe are x86-64 only.
- **Windows 10 without WSL2.** WSL1 has no `--import`-compatible networking
  story worth supporting.

## Standing risks

| Risk | Signal | Mitigation |
|---|---|---|
| WSL is unavailable (corporate lockdown, virtualization disabled, nested VMs) | Phase 1.4 or 3.2 fails on real machines | Keep "connect to a remote or existing daemon over TCP/HTTP" working on Windows as the fallback, and say so in the onboarding card |
| `/mnt/c` mmap problems corrupt data | Phase 2.1 recheck shows bad pieces | Switch the default to VM-local storage before any public release |
| gpui-pre's Windows backend regresses; the toolkit is young and pinned at `=0.3.2` | Rendering or input bugs appear in Phase 1.3 | Pin versions; report upstream; the web console served from the app (GPUI.md §10) is a usable stopgap UI |
| The WSL runtime build drifts from the macOS pin | XML-RPC commands behave differently per platform | Both scripts default to `v0.15.7`; bump them together, in one commit |
