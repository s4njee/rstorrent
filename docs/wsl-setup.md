# Running rstorrent on Windows with rtorrent in WSL

rtorrent is a Unix daemon and has no Windows build. On Windows, rstorrent runs
as a native app and talks to an rtorrent running inside WSL2. This document
covers the setup and the one concept that makes the arrangement work: **there
are two path namespaces, and the app translates between them.**

## Prerequisites

- Windows 10 21H2 / Windows 11 or newer, with WSL2 and a distro installed
  (`wsl --install -d Ubuntu`).
- systemd enabled in the distro. Check with `wsl -e sh -c 'ls -d /run/systemd/system'`;
  if it's missing, add this to `/etc/wsl.conf` and run `wsl.exe --shutdown`:

  ```ini
  [boot]
  systemd=true
  ```

- For building the app: Rust (`winget install Rustlang.Rustup`), the MSVC C++
  build tools, Node 18+, and the WebView2 runtime (preinstalled on Windows 11).

## 1. Install the daemon

From the repo root:

```powershell
wsl.exe -e bash tools/wsl-setup-rtorrent.sh
```

It prompts for your WSL sudo password. The script:

1. Installs build dependencies via apt.
2. Builds **libtorrent** and **rtorrent** from source at a matching tag
   (default `v0.16.18`). Ubuntu's packaged rtorrent is 0.9.8, which is missing
   much of the XML-RPC surface this client calls — hence building from source.
   Override with `RTORRENT_VERSION=… wsl.exe -e bash tools/…`, but do not go
   below 0.16.18 (see the crash note under Troubleshooting).
3. Writes `~/.rtorrent.rc` with SCGI listening on `127.0.0.1:5000` (any existing
   config is backed up first).
4. Installs and starts a `rtorrent.service` systemd unit.

Expect the build to take several minutes.

## 2. Point the app at it

Launch rstorrent, open **Preferences → Connection**, choose **TCP**, host
`127.0.0.1`, port `5000`, then **Test connection**. It should report the
rtorrent version.

This works because WSL2's `localhostForwarding` (on by default) bridges
`127.0.0.1` from Windows into the VM. The Unix socket transport is hidden on
Windows: `AF_UNIX` exists there, but it cannot reach a socket inside the VM's
filesystem.

> **Why loopback and not the VM's IP.** SCGI has no authentication whatsoever.
> Binding it to `0.0.0.0` so Windows can reach it over the WSL network adapter
> would also expose it to everything else on your LAN. Loopback plus
> localhostForwarding gives the same reachability with none of the exposure.

## Paths: the part worth understanding

The daemon reports paths in **its** namespace (`/home/you/Downloads/thing`).
Explorer, the folder pickers, and `std::fs` all speak Windows paths. rstorrent
translates at the boundary, in `src-tauri/src/wsl.rs`:

| Direction | Rule |
|---|---|
| Linux → Windows | `/mnt/c/x` → `C:\x`; anything else → `\\wsl.localhost\<distro>\…` |
| Windows → Linux | `C:\x` → `/mnt/c/x`; `\\wsl.localhost\<distro>\x` → `/x` |

Consequences worth knowing:

- **Picking a save folder with the native picker works.** It returns a Windows
  path, which is converted to `/mnt/…` before it reaches rtorrent. You can also
  type a Linux path directly into Preferences — anything starting with `/` is
  passed through untouched.
- **Storage location matters for speed.** Files under the distro's own home
  (`/home/you/…`) live on ext4 and run at full speed. Files under `/mnt/c/…`
  cross the 9p/drvfs bridge and are markedly slower — fine for a few torrents,
  bad for a large library. The setup script defaults to `~/Downloads` inside
  WSL for this reason.
- **A path with no counterpart is refused, not guessed.** Choosing a folder on
  a mapped network drive gives a clear error, because the VM genuinely cannot
  see it.
- **Free space** is read with `df` inside the VM rather than from the UNC share,
  which reports the host volume instead of the ext4 filesystem. The reading is
  cached for 30 seconds, since the status bar polls once a second.
- **Delete-data** moves files to the Recycle Bin when they're on a Windows drive.
  For files inside the VM — where there is no Recycle Bin — they go to the
  distro's freedesktop trash (`~/.local/share/Trash`), with a `.trashinfo` so a
  Linux file manager can restore them. It is never a hard delete either way.

## Managing the daemon

```powershell
wsl.exe -e systemctl status rtorrent      # is it up?
wsl.exe -e sudo systemctl restart rtorrent
wsl.exe -e journalctl -u rtorrent -n 50   # logs
```

rtorrent has no daemon mode — it is a curses application and exits without a
pty — so the service runs it inside `screen`. To use its own TUI, attach to
that session:

```powershell
wsl.exe -e screen -r rtorrent   # detach again with Ctrl-a d
```

Running it under systemd also keeps the WSL VM alive. Without a long-running
process, WSL shuts the VM down a few seconds after the last shell exits — and
the daemon with it.

## Troubleshooting

**"Test connection" fails.** Check the daemon is up
(`wsl.exe -e systemctl status rtorrent`) and that it is actually listening:

```powershell
wsl.exe -e ss -tlnp
```

You should see `127.0.0.1:5000`. If the service is up but the port isn't
reachable from Windows, `localhostForwarding` may be off — check for a
`%USERPROFILE%\.wslconfig` that disables it, or switch to mirrored networking:

```ini
[wsl2]
networkingMode=mirrored
```

**"XML-RPC not supported".** The daemon is up and the port is reachable, but
rtorrent was built without an XML-RPC backend — it accepts the SCGI connection
and then refuses every call, which is the whole interface this client speaks.
rtorrent needs `--with-xmlrpc-tinyxml2` (or `--with-xmlrpc-c`) at configure
time; the setup script passes it. To check an existing build:

```powershell
wsl.exe -e sh -c 'strings $(which rtorrent) | grep -qi tinyxml && echo built-in || echo MISSING'
```

tinyxml2 is vendored into the binary rather than linked, so `ldd` will not show
it — hence the `strings` check. If it reports MISSING, re-run the setup script.

**rtorrent dies the moment a torrent is added.** On libtorrent 0.16.17 and
earlier, built against a recent libcurl (Ubuntu 26.04's included), the first
tracker announce aborts the process:

```
torrent::internal_error: verify_libcurl_internal_wakeup(fd:24): fd is blocking, expected non-blocking
```

Fixed upstream in [libtorrent#813](https://github.com/rakshasa/libtorrent/pull/813),
released in 0.16.18. Rebuild both projects at that tag — the setup script now
defaults to it.

Note that `screen` exits 0 even when rtorrent crashes underneath it, so systemd
reports the unit as succeeding. `Restart=always` (rather than `on-failure`)
exists for exactly this reason. To see a crash, run rtorrent outside the
service and capture the pty:

```powershell
wsl.exe -e sh -c 'systemctl --user 2>/dev/null; sudo systemctl stop rtorrent; script -qec /usr/local/bin/rtorrent /tmp/rt.log'
```

**"Command ... does not exist" on startup.** 0.16.18 renamed some settings —
notably `network.port_range.set` became `network.listen.port.range.set`; the old
spelling now only resolves when rtorrent is started with `-D`. Check
`~/.rtorrent.rc` against the version you built.

**Free space shows nothing.** Only local daemons report it, and only when the
default save path is set to a path the VM can `df`.

**Toast notifications are attributed to PowerShell.** Windows addresses toasts
by AppUserModelID, which only exists once the NSIS installer has written a Start
Menu shortcut. In a `tauri dev` run there is no shortcut, so the app falls back
to the PowerShell AUMID. Installed builds are attributed correctly.
