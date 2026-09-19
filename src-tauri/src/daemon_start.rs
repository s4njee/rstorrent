//! Start a local rtorrent daemon on demand (C20).
//!
//! The app is a *client* for an rtorrent daemon, but the macOS build also ships
//! its own rtorrent (`tools/build-rtorrent-macos.sh` stages it in
//! `src-tauri/binaries/rtorrent/`, which `tauri.conf.json` copies into the
//! app's Resources). When that bundled runtime is present we prefer it, so a
//! fresh install needs no `brew install rtorrent`; otherwise we fall back to
//! whatever is on `PATH`.
//!
//! Starting mirrors `start-rtorrent.sh` — create the session dir, clear stale
//! lock/socket files, and launch rtorrent inside a detached tmux session
//! (falling back to a direct spawn when tmux is absent). On Windows the daemon
//! lives inside the WSL VM, so the same work is done through `wsl.exe`.

use crate::ipc::Transport;
use tauri::AppHandle;
#[cfg(not(target_os = "windows"))]
use tauri::Manager;

/// The rtorrent/libtorrent tag staged by `tools/build-rtorrent-macos.sh`.
///
/// Keep this in step with the script's default `RTORRENT_VERSION`; it only
/// labels the generated config and the log line, never gates behavior.
#[cfg(not(target_os = "windows"))]
pub const BUNDLED_RTORRENT_VERSION: &str = "0.15.7";

pub fn start(app: &AppHandle, transport: Transport) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let _ = app;
        start_windows(&transport)
    }
    #[cfg(not(target_os = "windows"))]
    {
        start_unix(app, &transport)
    }
}

#[cfg(not(target_os = "windows"))]
fn start_unix(app: &AppHandle, transport: &Transport) -> Result<String, String> {
    let home = crate::settings::home_dir();
    let session_dir = home.join(".rtorrent/session");
    let socket_path = match transport {
        Transport::UnixSocket { path } => path.clone(),
        _ => home
            .join(".rtorrent/rpc.socket")
            .to_string_lossy()
            .into_owned(),
    };
    let lock_path = session_dir.join("rtorrent.lock");

    std::fs::create_dir_all(&session_dir)
        .map_err(|e| format!("could not create {}: {e}", session_dir.display()))?;

    if is_tmux_session_running() {
        return Ok("rtorrent is already running (tmux session 'rtorrent')".into());
    }
    if is_process_running() {
        return Ok("rtorrent is already running".into());
    }

    if lock_path.exists() {
        let _ = std::fs::remove_file(&lock_path);
    }
    let socket = std::path::Path::new(&socket_path);
    if socket.exists() {
        let _ = std::fs::remove_file(socket);
    }

    let (bin, bundled) = rtorrent_bin(app)?;
    let bin = bin.to_string_lossy().into_owned();

    // A bundled daemon should come up reachable: rtorrent won't open SCGI from
    // its built-in defaults, so a machine that has never had rtorrent gets a
    // minimal config. An existing `~/.rtorrent.rc` is never touched.
    if bundled {
        ensure_config(transport)?;
    }

    let tmux_available = std::process::Command::new("tmux")
        .arg("-V")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if tmux_available {
        let shell_cmd = format!(
            "ulimit -n 4096 2>/dev/null || true; TERM=xterm-256color '{}'",
            bin.replace('\'', "'\\''")
        );
        let status = std::process::Command::new("tmux")
            .args(["new-session", "-d", "-s", "rtorrent", &shell_cmd])
            .status()
            .map_err(|e| format!("could not start tmux: {e}"))?;
        if !status.success() {
            return Err(format!("tmux failed to start rtorrent (exit {status})"));
        }
    } else {
        std::process::Command::new(&bin)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("could not start rtorrent ({bin}): {e}"))?;
    }

    let origin = if bundled { "bundled" } else { "system" };
    if matches!(transport, Transport::UnixSocket { .. }) {
        for _ in 0..15 {
            if socket.exists() {
                return Ok(format!("rtorrent started ({origin}, socket {socket_path})"));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        return Ok(format!(
            "rtorrent launched ({origin}) — waiting for it to create the socket…"
        ));
    }
    Ok(format!("rtorrent started ({origin})"))
}

#[cfg(not(target_os = "windows"))]
fn is_tmux_session_running() -> bool {
    std::process::Command::new("tmux")
        .args(["has-session", "-t", "rtorrent"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
fn is_process_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", "rtorrent"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Resolve the rtorrent to launch, preferring the one this build ships.
///
/// Returns the path and whether it came from the bundle (which decides if we
/// may write a starter config).
#[cfg(not(target_os = "windows"))]
fn rtorrent_bin(app: &AppHandle) -> Result<(std::path::PathBuf, bool), String> {
    if let Some(path) = bundled_rtorrent(app) {
        return Ok((path, true));
    }
    find_rtorrent_bin().map(|path| (path, false))
}

/// The runtime `tools/build-rtorrent-macos.sh` staged into the app, if any.
///
/// A release bundle keeps it under `Contents/Resources/binaries/rtorrent/`
/// together with the dylibs it needs; `tauri dev` reads the same layout from
/// the crate directory. `RSTORRENT_RTORRENT_BIN` overrides both for testing.
#[cfg(not(target_os = "windows"))]
fn bundled_rtorrent(app: &AppHandle) -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("RSTORRENT_RTORRENT_BIN") {
        let path = std::path::PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }

    if let Ok(dir) = app.path().resource_dir() {
        let candidate = dir.join("binaries").join("rtorrent").join("rtorrent");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // `tauri dev` doesn't stage resources, so reach into the source tree. Only
    // compiled in for debug builds — a release .app must not bake in the
    // builder's paths.
    #[cfg(debug_assertions)]
    {
        let candidate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join("rtorrent")
            .join("rtorrent");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

/// The starter `.rtorrent.rc` for `transport`, rooted at `home`, or `None` when
/// there is nothing useful to write (a remote/HTTP transport has no local SCGI
/// endpoint to open).
#[cfg(not(target_os = "windows"))]
fn render_config(transport: &Transport, home: &std::path::Path) -> Option<String> {
    // The bundled rtorrent is pinned at 0.15.7, which spells the listen range
    // `network.port_range.set` (0.16.20 renamed it). Leave the range out
    // entirely — the app sets it over XML-RPC once connected, where it can pick
    // the name the running daemon actually knows.
    let scgi = match transport {
        Transport::UnixSocket { path } => format!("network.scgi.open_local = {path}"),
        Transport::Tcp { host, port } => format!("network.scgi.open_port = {host}:{port}"),
        Transport::Http { .. } => return None,
    };

    Some(format!(
        "# Written by rstorrent for its bundled rtorrent {version}.\n\
         # Delete this file to manage the daemon yourself.\n\
         \n\
         directory.default.set = {downloads}\n\
         session.path.set      = {session}\n\
         {scgi}\n",
        version = BUNDLED_RTORRENT_VERSION,
        downloads = home.join("Downloads").display(),
        session = home.join(".rtorrent/session").display(),
    ))
}

/// Write a minimal `~/.rtorrent.rc` when the user has never had one, so the
/// bundled daemon opens the SCGI endpoint the app connects to. Existing files
/// are left exactly as they are.
#[cfg(not(target_os = "windows"))]
fn ensure_config(transport: &Transport) -> Result<(), String> {
    let path = crate::settings::home_dir().join(".rtorrent.rc");
    if path.exists() {
        return Ok(());
    }

    let Some(body) = render_config(transport, &crate::settings::home_dir()) else {
        return Ok(());
    };

    let _ = std::fs::create_dir_all(crate::settings::home_dir().join(".rtorrent/session"));
    let _ = std::fs::create_dir_all(crate::settings::home_dir().join("Downloads"));

    std::fs::write(&path, body).map_err(|e| format!("could not write {}: {e}", path.display()))
}

#[cfg(not(target_os = "windows"))]
fn find_rtorrent_bin() -> Result<std::path::PathBuf, String> {
    if let Ok(out) = std::process::Command::new("which").arg("rtorrent").output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() && std::path::Path::new(&p).exists() {
                return Ok(p.into());
            }
        }
    }
    for cand in [
        "/opt/homebrew/bin/rtorrent",
        "/usr/local/bin/rtorrent",
        "/usr/bin/rtorrent",
        "/opt/local/bin/rtorrent",
    ] {
        if std::path::Path::new(cand).exists() {
            return Ok(cand.into());
        }
    }
    Err(
        "rtorrent executable not found — the app ships one on macOS, or install \
         it with 'brew install rtorrent'"
            .into(),
    )
}

#[cfg(target_os = "windows")]
fn start_windows(transport: &Transport) -> Result<String, String> {
    let _ = transport;
    if crate::wsl::distro().is_none() {
        return Err("WSL is not available — install a WSL distribution first".into());
    }

    let tmux_running = std::process::Command::new("wsl.exe")
        .args(["-e", "sh", "-c", "tmux has-session -t rtorrent 2>/dev/null"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if tmux_running {
        return Ok("rtorrent is already running (tmux session 'rtorrent' inside WSL)".into());
    }
    let pgrep_running = std::process::Command::new("wsl.exe")
        .args(["-e", "pgrep", "-x", "rtorrent"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if pgrep_running {
        return Ok("rtorrent is already running inside WSL".into());
    }

    // Clean stale files and start inside WSL.
    let script = r#"
set -e
mkdir -p ~/.rtorrent/session
rm -f ~/.rtorrent/session/rtorrent.lock ~/.rtorrent/rpc.socket
ulimit -n 4096 2>/dev/null || true
if command -v tmux >/dev/null 2>&1; then
  TERM=xterm-256color tmux new-session -d -s rtorrent rtorrent
else
  nohup rtorrent >/dev/null 2>&1 &
fi
"#;
    let status = std::process::Command::new("wsl.exe")
        .args(["-e", "sh", "-c", script])
        .status()
        .map_err(|e| format!("could not run wsl.exe: {e}"))?;
    if !status.success() {
        return Err(format!("WSL failed to start rtorrent (exit {status})"));
    }

    // Briefly wait for the socket inside WSL.
    for _ in 0..15 {
        let exists = std::process::Command::new("wsl.exe")
            .args(["-e", "sh", "-c", "test -S ~/.rtorrent/rpc.socket"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if exists {
            return Ok("rtorrent started inside WSL".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Ok("rtorrent launched inside WSL — waiting for socket…".into())
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn unix_socket_config_opens_the_socket() {
        let transport = Transport::UnixSocket {
            path: "/Users/you/.rtorrent/rpc.socket".into(),
        };
        let body = render_config(&transport, Path::new("/Users/you")).expect("config");
        assert!(body.contains("network.scgi.open_local = /Users/you/.rtorrent/rpc.socket"));
        assert!(body.contains("session.path.set      = /Users/you/.rtorrent/session"));
        assert!(body.contains("directory.default.set = /Users/you/Downloads"));
        // The listen range is deliberately absent: the pre-0.16.20 name is only
        // correct for the bundled 0.15.7 and the renamed one for newer daemons,
        // so the app sets it over XML-RPC where it can tell which is which.
        assert!(!body.contains("port.range"));
    }

    #[test]
    fn tcp_config_opens_the_port() {
        let transport = Transport::Tcp {
            host: "127.0.0.1".into(),
            port: 5000,
        };
        let body = render_config(&transport, Path::new("/home/you")).expect("config");
        assert!(body.contains("network.scgi.open_port = 127.0.0.1:5000"));
    }

    #[test]
    fn http_transport_has_no_local_config() {
        let transport = Transport::Http {
            url: "https://box.example/RPC2".into(),
            username: "alice".into(),
        };
        assert!(render_config(&transport, Path::new("/home/you")).is_none());
    }
}
