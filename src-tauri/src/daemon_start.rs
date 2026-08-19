//! Start a local rtorrent daemon on demand (C20).
//!
//! The app is a *client* for an already-running rtorrent, but when the daemon
//! is local we can attempt to start it. This mirrors `start-rtorrent.sh` —
//! create the session dir, clear stale lock/socket files, and launch rtorrent
//! inside a detached tmux session (falling back to a direct spawn when tmux is
//! absent). On Windows the daemon lives inside the WSL VM, so the same work
//! is done through `wsl.exe`.

use crate::ipc::Transport;

pub fn start(transport: Transport) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        start_windows(&transport)
    }
    #[cfg(not(target_os = "windows"))]
    {
        start_unix(&transport)
    }
}

#[cfg(not(target_os = "windows"))]
fn start_unix(transport: &Transport) -> Result<String, String> {
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

    let bin = find_rtorrent_bin()?;

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

    if matches!(transport, Transport::UnixSocket { .. }) {
        for _ in 0..15 {
            if socket.exists() {
                return Ok(format!("rtorrent started (socket {socket_path})"));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        return Ok("rtorrent launched — waiting for it to create the socket…".into());
    }
    Ok("rtorrent started".into())
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

#[cfg(not(target_os = "windows"))]
fn find_rtorrent_bin() -> Result<String, String> {
    if let Ok(out) = std::process::Command::new("which").arg("rtorrent").output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() && std::path::Path::new(&p).exists() {
                return Ok(p);
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
    Err("rtorrent executable not found — install it with 'brew install rtorrent'".into())
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
