//! Starting the bundled rtorrent on Windows, inside WSL.
//!
//! rtorrent has no Windows build (libtorrent's only event loops are epoll and
//! kqueue), so the Windows app ships a statically linked *Linux* rtorrent as a
//! WSL root filesystem — `tools/build-rtorrent-wsl.sh` builds it, pinned to
//! the same tag as the macOS runtime, and `tools/bundle-gpui-windows.ps1` puts
//! it at `runtime\rstorrent-rootfs.tar.gz` beside the exe. Starting then means:
//!
//!   1. WSL itself must be installed (`wsl --install --no-distribution`, which
//!      needs an administrator and usually a reboot — we can only explain).
//!   2. On first start the tarball is imported as a dedicated distro,
//!      [`wsl::DISTRO_NAME`], under `%LOCALAPPDATA%\rstorrent\wsl`. The user's
//!      own distros are never touched.
//!   3. A starter `~/.rtorrent.rc` is written inside it when there is none:
//!      daemon mode, SCGI on the configured `127.0.0.1:<port>` (WSL forwards
//!      the VM's loopback to Windows), downloads in the Windows Downloads
//!      folder via `/mnt/c` so they survive `wsl --unregister`.
//!   4. rtorrent is launched as a child `wsl.exe -d rstorrent -e rtorrent`
//!      process. Daemon mode keeps it in the foreground, and a running
//!      `wsl.exe` is what keeps WSL from shutting the idle distro down.
//!   5. We wait for the SCGI port to answer before reporting success.
//!
//! The pure parts (config text, rootfs lookup) compile everywhere so their
//! tests run on any host; the process work is Windows-only.

use std::path::{Path, PathBuf};

#[cfg(windows)]
use rtorrent_core::types::Transport;

#[cfg(windows)]
use crate::wsl;

/// Where the rootfs sits beside the installed exe.
pub const ROOTFS_BESIDE_EXE: &str = "runtime/rstorrent-rootfs.tar.gz";

/// Where `tools/build-rtorrent-wsl.sh` leaves it in a checkout, for `cargo run`.
pub const ROOTFS_IN_REPO: &str = "binaries/rtorrent-wsl/rstorrent-rootfs.tar.gz";

/// The rtorrent binary inside the bundled distro.
pub const RTORRENT_IN_DISTRO: &str = "/usr/local/bin/rtorrent";

/// The bundled rootfs, if this build has one.
///
/// `RSTORRENT_WSL_ROOTFS` overrides everything (a developer testing another
/// build); then beside the exe (an installed app); then, for a debug build
/// only, the checkout's staging directory (a bare `cargo run`).
#[must_use]
pub fn bundled_rootfs() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("RSTORRENT_WSL_ROOTFS") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    rootfs_in(&exe_dir).or_else(|| {
        cfg!(debug_assertions)
            .then(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join(ROOTFS_IN_REPO)
            })
            .filter(|path| path.is_file())
    })
}

/// The rootfs beside a given exe directory, split out for tests.
#[must_use]
pub fn rootfs_in(exe_dir: &Path) -> Option<PathBuf> {
    Some(exe_dir.join(ROOTFS_BESIDE_EXE)).filter(|path| path.is_file())
}

/// The starter `~/.rtorrent.rc` for the bundled distro.
///
/// `downloads` is already in the daemon's namespace (`/mnt/c/Users/…`). The
/// keys are the 0.15.7 spellings, like the macOS starter config; the listen
/// port range is left to the app, which sets it over XML-RPC.
#[must_use]
pub fn render_config(port: u16, downloads: &str) -> String {
    format!(
        "# Written by rstorrent for its bundled rtorrent {version} (WSL).\n\
         # Delete this file to have the app write a fresh one.\n\
         \n\
         system.daemon.set     = true\n\
         directory.default.set = {downloads}\n\
         session.path.set      = /root/.rtorrent/session\n\
         network.scgi.open_port = 127.0.0.1:{port}\n",
        version = crate::daemon::BUNDLED_RTORRENT_VERSION,
    )
}

/// Whether opening the app should start the bundled daemon: a local TCP
/// endpoint that nothing answers on, WSL installed, and either the distro
/// already imported or a rootfs to import. Blocking; call off the UI thread.
#[cfg(windows)]
#[must_use]
pub fn should_autostart(transport: &Transport) -> bool {
    matches!(transport, Transport::Tcp { .. })
        && crate::settings::is_localhost(transport)
        && !crate::daemon::is_reachable(transport)
        && wsl::available()
        && (wsl::has_own_distro() || bundled_rootfs().is_some())
}

/// Start the bundled rtorrent in its WSL distro. See the module docs.
///
/// # Errors
///
/// Returns a user-facing message for each way this can fail: a transport we
/// cannot start for, no WSL, no runtime, a failed import, or a daemon that
/// exited or never opened its port.
#[cfg(windows)]
pub fn start(transport: &Transport) -> Result<String, String> {
    use std::time::{Duration, Instant};

    let Transport::Tcp { port, .. } = transport else {
        return Err(
            "on Windows the app starts rtorrent for a TCP connection (127.0.0.1:5000); \
             change the connection in Preferences"
                .to_owned(),
        );
    };
    if !crate::settings::is_localhost(transport) {
        return Err("start is only available for a local daemon".to_owned());
    }
    if crate::daemon::is_reachable(transport) {
        return Ok("rtorrent is already running".to_owned());
    }
    if !wsl::available() {
        return Err(
            "rtorrent runs inside WSL, which is not installed. In an administrator \
             terminal run `wsl --install --no-distribution`, restart Windows, then \
             start rtorrent again"
                .to_owned(),
        );
    }

    if !wsl::has_own_distro() {
        let rootfs = bundled_rootfs().ok_or_else(|| {
            format!("this build has no rtorrent runtime ({ROOTFS_BESIDE_EXE} is missing)")
        })?;
        wsl::import(&rootfs, &install_dir())?;
    }

    ensure_config(*port)?;

    // Clear a lock a killed daemon left behind — but only when none is
    // running, since a live daemon's lock is what stops a second one.
    let running = wsl::run_script(
        r#"mkdir -p "$HOME/.rtorrent/session"
if pgrep -x rtorrent >/dev/null; then echo running; else rm -f "$HOME/.rtorrent/session/rtorrent.lock"; fi"#,
        &[],
    )?;
    if running.trim() != "running" {
        launch()?;
    }

    // A cold VM boot plus rtorrent loading a large session can take a while;
    // the port answering is the only proof of life worth reporting.
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if crate::daemon::is_reachable(transport) {
            return Ok(format!(
                "rtorrent started (bundled, WSL distro '{}', port {port})",
                wsl::DISTRO_NAME
            ));
        }
        if let Some(message) = exited() {
            return Err(message);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(format!(
        "rtorrent was launched in WSL but port {port} never answered; see {}",
        log_path().display()
    ))
}

/// A one-line description of what [`start`] would use, for `--where-rtorrent`.
#[cfg(windows)]
#[must_use]
pub fn describe() -> String {
    if !wsl::available() {
        return "rtorrent: none — WSL is not installed".to_owned();
    }
    if wsl::has_own_distro() {
        return format!(
            "rtorrent: {RTORRENT_IN_DISTRO} in WSL distro '{}' (bundled, imported)",
            wsl::DISTRO_NAME
        );
    }
    match bundled_rootfs() {
        Some(rootfs) => format!(
            "rtorrent: {} (bundled, imported as WSL distro '{}' on first start)",
            rootfs.display(),
            wsl::DISTRO_NAME
        ),
        None => "rtorrent: none — no bundled runtime and no 'rstorrent' WSL distro".to_owned(),
    }
}

/// `%LOCALAPPDATA%\rstorrent`, where the distro's disk and our log live.
#[cfg(windows)]
fn app_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("rstorrent")
}

#[cfg(windows)]
fn install_dir() -> PathBuf {
    app_data_dir().join("wsl")
}

#[cfg(windows)]
fn log_path() -> PathBuf {
    app_data_dir().join("rtorrent.err")
}

/// Write the starter config when the distro has none. An existing file is
/// the user's and is left exactly as it is.
#[cfg(windows)]
fn ensure_config(port: u16) -> Result<(), String> {
    let current = wsl::read_home_file(".rtorrent.rc")
        .ok_or_else(|| "could not reach the rtorrent WSL distro".to_owned())?;
    if !current.trim().is_empty() {
        return Ok(());
    }
    let downloads = crate::settings::default_save_path();
    wsl::run_script(r#"mkdir -p "$1""#, &[&downloads])?;
    wsl::write_home_file(".rtorrent.rc", &render_config(port, &downloads))
}

/// The daemon's `wsl.exe`, kept so its exit (and stderr) can be checked while
/// waiting for the port. Dropping a `Child` does not kill it, and the process
/// outlives the app on purpose — like the macOS tmux session.
#[cfg(windows)]
static CHILD: std::sync::Mutex<Option<std::process::Child>> = std::sync::Mutex::new(None);

#[cfg(windows)]
fn launch() -> Result<(), String> {
    use std::process::Stdio;

    let log = log_path();
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let stderr = std::fs::File::create(&log).map_or_else(|_| Stdio::null(), Stdio::from);
    let child = wsl::distro_command()
        .args(["--cd", "~", "-e", RTORRENT_IN_DISTRO])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .map_err(|error| format!("could not start rtorrent in WSL: {error}"))?;
    if let Ok(mut slot) = CHILD.lock() {
        *slot = Some(child);
    }
    Ok(())
}

/// The failure message if the launched daemon has already exited.
#[cfg(windows)]
fn exited() -> Option<String> {
    let mut slot = CHILD.lock().ok()?;
    let status = slot.as_mut()?.try_wait().ok()??;
    *slot = None;
    let detail = std::fs::read_to_string(log_path()).ok().and_then(|text| {
        text.lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .map(str::to_owned)
    });
    Some(match detail {
        Some(line) => format!("rtorrent exited in WSL ({status}): {line}"),
        None => format!("rtorrent exited in WSL ({status})"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_starter_config_runs_headless_on_the_forwarded_port() {
        let rc = render_config(5000, "/mnt/c/Users/you/Downloads");
        assert!(rc.contains("system.daemon.set     = true"));
        assert!(rc.contains("network.scgi.open_port = 127.0.0.1:5000"));
        assert!(rc.contains("directory.default.set = /mnt/c/Users/you/Downloads"));
        // Never a unix socket: Windows cannot reach one inside the VM.
        assert!(!rc.contains("open_local"));
    }

    #[test]
    fn the_rootfs_is_found_beside_the_exe() {
        let dir = std::env::temp_dir().join(format!("rstorrent-rootfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(rootfs_in(&dir), None);
        let rootfs = dir.join(ROOTFS_BESIDE_EXE);
        std::fs::create_dir_all(rootfs.parent().unwrap()).unwrap();
        std::fs::write(&rootfs, b"tar").unwrap();
        assert_eq!(rootfs_in(&dir), Some(rootfs));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
