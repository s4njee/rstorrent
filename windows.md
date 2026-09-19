# Windows bring-up for the GPUI shell

Handoff notes for whoever picks this up on the Windows machine, whether a
person, Claude or another agent. Read this first, then [GPUI.md](GPUI.md) for
the shell as a whole.

**Goal:** `rstorrent-gpui.exe` runs natively on Windows 10/11 with its own
bundled rtorrent. The Tauri shell it replaces has been removed (its last
version is in commit `44e6680` if you need to compare behaviour).

**State at handoff (2026-09-19):** all Windows code is written and
type-checks for `x86_64-pc-windows-msvc` from macOS. **None of it has run on
Windows yet.** A native Windows build is only possible on Windows, because
gpui compiles its Direct3D shaders with `fxc.exe` at build time. §4 is the
verification checklist, in order.

---

## 1. How it works on Windows

```
rstorrent-gpui.exe (native, Direct3D 11)
   │  SCGI over TCP 127.0.0.1:5000  (WSL forwards the VM's loopback)
   ▼
WSL2 distro "rstorrent"  (Alpine + static musl rtorrent 0.15.7, ~20 MB)
   └─ /usr/local/bin/rtorrent, daemon mode, config /root/.rtorrent.rc
```

- **rtorrent can't be built natively.** libtorrent's only event loops are
  epoll and kqueue, and there's no maintained Windows or Cygwin port. So the app
  ships a statically linked *Linux* rtorrent as a WSL root filesystem, and on
  first start imports it as a **dedicated distro named `rstorrent`**. The
  user's own distros are never touched. This is the Docker Desktop and Rancher
  Desktop pattern.
- **The only user prerequisite is WSL itself**
  (`wsl --install --no-distribution`, which needs admin and usually a reboot).
  No apt, no systemd, no user distro.
- **Version.** It's the same rtorrent tag as the macOS bundle (0.15.7), so both
  platforms speak the same XML-RPC command set. Use 0.15.7 key spellings, e.g.
  `network.port_range.set`.
- **Downloads** default to the Windows Downloads folder as WSL sees it
  (`/mnt/c/Users/<you>/Downloads`), so they survive `wsl --unregister rstorrent`
  and open in Explorer without the slow 9p share.
- **Paths.** Every path crosses the boundary through `crates/gpui/src/wsl.rs`:
  `/mnt/c/…` ↔ `C:\…`, and anything else ↔ `\\wsl.localhost\rstorrent\…`.

### Code map

| File | Role |
|---|---|
| `crates/gpui/src/wsl.rs` | WSL bridge, ported from the Tauri shell's `wsl.rs`: path translation, `wsl.exe` wrappers (`available`, `registered_distros`, `import`, `run_script`, home-file read/write, `df`, trash). It targets the `rstorrent` distro when registered, otherwise the default distro. The pure parts compile and are unit-tested on every OS. |
| `crates/gpui/src/daemon_wsl.rs` | Windows daemon lifecycle: find the rootfs → `wsl --import` → write the starter `~/.rtorrent.rc` → launch `wsl.exe -d rstorrent --cd ~ -e /usr/local/bin/rtorrent` as a child → wait up to 30 s for the port. It also has `should_autostart` and `describe`. |
| `crates/gpui/src/daemon.rs` | Cross-platform entry points. The Windows `start`, `should_autostart` and `describe` delegate to `daemon_wsl`. `is_reachable` is shared. |
| `crates/gpui/src/model.rs` (`autostart_daemon`) | At launch, starts the bundled daemon in the background if nothing answers. The same code path runs on macOS. |
| `crates/gpui/src/localfs.rs` | The Windows branches of `resolve`, `to_daemon_path`, `trash` (Recycle Bin for `/mnt/x/…`, the distro's freedesktop trash otherwise) and `free_space` (`df` in WSL). |
| `crates/gpui/src/services.rs` (`load_options`, `add_raw`, `add_magnet`) | Save and incomplete dirs are translated into the daemon's namespace at the one place every add passes through. |
| `crates/gpui/src/settings.rs` | The Windows defaults: transport `Tcp 127.0.0.1:5000`, save path `/mnt/c/Users/<you>/Downloads`. |
| `crates/gpui/src/notifications.rs` | Windows toast via `tauri-winrt-notification` on a background thread. It tries the `com.rstorrent.app` AUMID first and falls back to PowerShell's. |
| `crates/gpui/build.rs` | On a Windows target, embeds `assets/icons/icon.ico` (ID 1, which gpui loads as the window icon) and VERSIONINFO. The macOS runtime staging runs only for macOS targets. |
| `crates/gpui/src/main.rs` | `windows_subsystem = "windows"` in release builds, so no console window. |
| `tools/build-rtorrent-wsl.sh` | Builds the runtime: static musl rtorrent in an Alpine chroot, then packs a clean Alpine rootfs and smoke-tests SCGI. Needs Linux x86-64 as root. |
| `tools/bundle-gpui-windows.ps1` | Packaging: builds the web console if stale and the release exe, then lays out `dist-gpui\windows\rstorrent\{rstorrent-gpui.exe, runtime\rstorrent-rootfs.tar.gz}` and zips it. |
| `binaries/rtorrent-wsl/` | Runtime output: tarball, `SHA256SUMS`, README. Git-ignored except the README. |
| `.github/workflows/ci.yml` | New `gpui-macos` and `gpui-windows` jobs (clippy and tests). They haven't run yet. |
| `.github/workflows/release.yml` | On a `v*` tag: a `wsl-runtime` ubuntu job builds the rootfs, then the `macos` and `windows` jobs package the app and attach both zips to the GitHub Release (see §6). |

The runtime lookup order is in `daemon_wsl::bundled_rootfs`, first hit wins:
1. the `RSTORRENT_WSL_ROOTFS` env var
2. `<exe dir>\runtime\rstorrent-rootfs.tar.gz`
3. debug builds only: `<checkout>\binaries\rtorrent-wsl\rstorrent-rootfs.tar.gz`, so a bare `cargo run` finds it

---

## 2. Prerequisites on the Windows machine

- **Rust** with the MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc`).
- **Visual Studio 2022 Build Tools**, with "Desktop development with C++" and a
  **Windows 10/11 SDK**. gpui's build needs `rc.exe` and `fxc.exe`. They're found
  through the SDK registry key, or set `GPUI_FXC_PATH` to point at `fxc.exe`.
- **Node 20+** and npm. The server half embeds the web console from `dist-web/`
  at compile time, so `npm ci && npm run build:web` must run before
  `cargo build`, or the crate won't compile.
- **Git.** `.gitattributes` forces LF, so the bash scripts survive a Windows
  checkout.
- **WSL** installed (`wsl --status` exits 0). This machine already has an Arch
  distro (reachable from the Mac as `root@leo.local`); the app will add a
  separate `rstorrent` distro next to it.

---

## 3. Getting the code and the runtime onto Windows

1. **Code.** Everything is on `main` (`git clone git@github.com:s4njee/rstorrent.git`).
   Confirm `crates/gpui/src/wsl.rs` exists.
2. **Runtime tarball.** It's git-ignored, so either:
   - build it on this machine inside WSL, from the checkout (which lives under `/mnt/c/...`):
     ```bash
     sudo tools/build-rtorrent-wsl.sh
     ```
     This takes about 60 s cold and seconds when cached; the build cache is in `/var/tmp/rstorrent-wsl-build`.
   - or build it from the Mac with `BUILD_HOST=root@leo.local tools/build-rtorrent-wsl.sh`,
     then copy `binaries/rtorrent-wsl/` over.

   Verify it with `sha256sum -c SHA256SUMS` in `binaries/rtorrent-wsl/`. At
   handoff the tarball was 8.0 MiB, sha256 `1f36cfbb…6098bfa7`.

---

## 4. Verification checklist, in order

Run from a Developer PowerShell. Record results in §8.

1. **Web console:** `npm ci; npm run build:web` produces `dist-web\web.html`.
2. **Tests:** `cargo test -p rstorrent-gpui -p rtorrent-core`. All should pass,
   including `wsl::tests` and `daemon_wsl::tests`. Any Windows-only failure is
   new information; fix it. The macOS-only tests are cfg'd out.
3. **Lint:** `cargo clippy -p rstorrent-gpui --all-targets`. The only
   pre-existing warnings are in `policy.rs`, `model.rs` and `web_host.rs`.
4. **Debug build and run:** `cargo run -p rstorrent-gpui`. Check:
   - it links, with no duplicate-resource error between gpui's manifest and our icon/VERSIONINFO `.rc`;
   - `fxc`/`rc` are found;
   - a window opens and renders (Direct3D 11);
   - the icon shows in the title bar and taskbar.
5. **Runtime resolution:** `cargo run -p rstorrent-gpui -- --where-rtorrent`
   should report the repo tarball ("imported as WSL distro 'rstorrent' on first
   start"). Only a debug build prints here; see §5 on release builds and the
   console.
6. **Headless start:** `cargo run -p rstorrent-gpui -- --start-rtorrent`. This is
   the first real WSL exercise. Expect:
   - `wsl --import rstorrent %LOCALAPPDATA%\rstorrent\wsl <tarball> --version 2`
     succeeds (check `wsl -l -v`: `rstorrent`, version 2);
   - `/root/.rtorrent.rc` gets written with SCGI `127.0.0.1:5000`, `system.daemon.set = true`,
     and `/mnt/c/Users/<you>/Downloads` (check with `wsl -d rstorrent cat /root/.rtorrent.rc`);
   - the command prints `rtorrent started (bundled, WSL distro 'rstorrent', port 5000)`
     within 30 s;
   - on failure, read `%LOCALAPPDATA%\rstorrent\rtorrent.err`.
7. **GUI end to end.** Open the app with no daemon running
   (`wsl -t rstorrent` first). Autostart should bring the daemon up and the
   table should connect. Then:
   - add a magnet (use a legal test torrent, e.g. an Ubuntu ISO);
   - let it download to the Windows Downloads folder;
   - pause and resume;
   - Reveal → Explorer selects the file;
   - Remove + delete data → the file goes to the Recycle Bin;
   - check the free-space readout;
   - use Set Location with a picked `C:\…` folder (it must arrive as `/mnt/c/…`);
   - in the add dialog, Browse… for a save folder.
8. **Completion toast:** a finished download shows a toast. If nothing shows and
   there's no error, the unregistered `com.rstorrent.app` AUMID was silently
   accepted. Register it (a Start-menu shortcut from the installer, or
   `HKCU\Software\Classes\AppUserModelId\com.rstorrent.app`), or use gpui's own
   `App::show_system_notification`, which registers the AUMID and routes
   clicks back.
9. **Lifetime:** quit the app. The daemon should keep running (its `wsl.exe`
   child outlives the app on purpose, like the macOS tmux session). Reopening
   should connect without starting a second rtorrent.
10. **Packaging:** after `powershell -File tools\bundle-gpui-windows.ps1`,
    run it again from pwsh 7. It should produce
    `dist-gpui\windows\rstorrent-0.1.0-windows-x64.zip` with a top-level
    `rstorrent\` folder. Unzip it somewhere else and repeat steps 6–7 from the
    unzipped exe; this time the runtime comes from `runtime\`.
11. **No-WSL path, if you can test it** (e.g. a VM without WSL): Start should
    show the `wsl --install --no-distribution` message, and nothing should crash.
12. **CI:** push and check that the `gpui-windows` job in `ci.yml` passes.

---

## 5. Known gaps: work for the Windows side

Roughly by priority. Everything above is ported; these are not.

1. **Make these behaviours robust:**
   - `wsl --status` as the "WSL installed" probe (`wsl::available`): confirm it
     exits non-zero on a machine without WSL, and zero with WSL but no distro.
   - `wsl -l -q` decoding (UTF-16LE; unit-tested with a synthetic sample only).
2. **Performance on `/mnt/c`.** rtorrent writes through mmap, and drvfs/9p
   under WSL2 is slow and has had mmap quirks. If downloads to `/mnt/c` are
   slow or corrupt, make the default a VM-local directory (e.g.
   `/root/Downloads`, reachable from Explorer at `\\wsl.localhost\rstorrent\…`),
   and warn that `wsl --unregister` deletes it.
3. **Incoming peer connections.** Under WSL's default NAT networking the
   listen port can't be reached from the internet, so only outgoing connections
   work. Options:
   - mirrored networking (Windows 11 22H2+; needs a Hyper-V firewall rule
     created by an admin);
   - `netsh interface portproxy` (TCP only, needs admin, breaks when the VM IP
     changes).

   Decide, then document it in the UI.
4. **An existing daemon in the default distro.** If the user already runs the
   Tauri-era rtorrent (`tools/wsl-setup-rtorrent.sh`: systemd, TCP 5000) and it
   isn't running at launch, autostart imports and starts ours on the same port.
   Detect it (e.g. an `rtorrent.service` in the default distro) and prefer it,
   or ask.
5. **Console output from the release exe.** `windows_subsystem = "windows"`
   means `--where-rtorrent` and `--start-rtorrent` print nothing from a release
   build. Call `AttachConsole(ATTACH_PARENT_PROCESS)` when those flags are
   present.
6. **Opening `.torrent` and `magnet:` links**, which Tauri had and GPUI lacks:
   argv handling, single-instance forwarding (named mutex plus a pipe), and
   registering the file type and URL scheme. That needs an installer.
7. **Installer.** NSIS, WiX or MSIX, per-user install. It needs:
   - the Start-menu shortcut with the AUMID (fixes toasts, item 8 in §4);
   - the file and scheme associations;
   - the runtime in `runtime\`;
   - an uninstaller that offers `wsl --unregister rstorrent`, **warning that
     this deletes anything stored inside the VM**.
8. **Toast click-to-select.** Switch to `App::show_system_notification`, which
   means `post_completion` needs `&App`.
9. **Stale runtime upgrades.** When the app ships a newer tarball than the
   imported distro, detect it (the rootfs has `/etc/rstorrent-runtime` with
   the version and build date) and re-import. Keep `/root/.rtorrent.rc` and the
   session.
10. **Remaining path translation.** Watch-folder save paths and move-rule
    destinations picked in Preferences are stored as typed. Translate them with
    `localfs::to_daemon_path` on save, as `load_options` does for adds.
11. **Code signing.** Unsigned exes trigger SmartScreen. That's out of scope
    until there's a certificate.

---

## 6. CI and release notes

- `ci.yml` → `gpui-windows`: `npm ci && npm run build:web`, then clippy and
  tests for `rstorrent-gpui` and `rtorrent-core`. There's no `-D warnings`,
  because of pre-existing warnings.
- `release.yml` → the `wsl-runtime` job (ubuntu-latest) runs
  `sudo tools/build-rtorrent-wsl.sh` and uploads the rootfs; the `windows` job
  downloads it into `binaries/rtorrent-wsl/` and runs the packaging script.
  **Neither has run yet.** The chroot build was only tested on leo, so the first
  `workflow_dispatch` run is the test for the ubuntu runner.

---

## 7. After the Tauri removal

- Done: the Tauri shell (`src-tauri/`) is deleted. The macOS runtime now stages
  in `binaries/rtorrent-macos/`, and GPUI owns its icons in
  `crates/gpui/assets/icons/`.
- Not ported from Tauri, and needed on Windows too: the tray; drag & drop;
  paste-to-add; the watch-folder runner; `.torrent`/`magnet:` association
  (see §5 and GPUI.md §4).
- `tools/wsl-setup-rtorrent.sh` and `docs/wsl-setup.md` remain as the "use your
  own distro" route. Retire them if the bundled runtime makes them pointless.

---

## 8. Findings log (Windows side, fill in as you go)

| Date | Step | Result / notes |
|---|---|---|
| | | |
