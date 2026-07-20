//! Filesystem side-effects on paths that came from the daemon.
//!
//! rtorrent reports paths in its own namespace. On macOS that namespace is the
//! app's namespace too, so these are thin wrappers. On Windows the daemon runs
//! inside WSL, so each of these has to cross the boundary — see [`crate::wsl`].
//!
//! Everything here assumes the caller has already checked
//! [`crate::settings::is_localhost`]; a remote daemon's files are not ours to
//! touch.

use std::path::PathBuf;

/// Resolve a daemon path to something the local OS can act on.
///
/// The `Err` is user-facing: it explains why a path can't be reached rather
/// than failing silently, which matters on Windows where a perfectly valid
/// Linux path may have no Windows equivalent.
pub fn resolve(daemon_path: &str) -> Result<PathBuf, String> {
    #[cfg(not(target_os = "windows"))]
    {
        Ok(PathBuf::from(daemon_path))
    }
    #[cfg(target_os = "windows")]
    {
        crate::wsl::to_windows(daemon_path).ok_or_else(|| {
            format!("{daemon_path} is inside WSL, but the distribution could not be reached")
        })
    }
}

/// Normalize a save directory chosen on *this* machine into the namespace the
/// daemon uses.
///
/// The native folder picker hands back a Windows path, but the daemon lives in
/// WSL and cannot open one — so a picked `C:\Users\you\Downloads` has to become
/// `/mnt/c/Users/you/Downloads`. A path that is already daemon-shaped (typed
/// into Preferences, or the default) passes through untouched, so this is safe
/// to apply to every directory heading for rtorrent.
pub fn to_daemon_path(picked: &str) -> Result<String, String> {
    #[cfg(not(target_os = "windows"))]
    {
        Ok(picked.to_string())
    }
    #[cfg(target_os = "windows")]
    {
        // Already a POSIX absolute path: the user typed a daemon path directly.
        if picked.starts_with('/') || picked.is_empty() {
            return Ok(picked.to_string());
        }
        crate::wsl::to_wsl(std::path::Path::new(picked)).ok_or_else(|| {
            format!("{picked} is not reachable from WSL — pick a local drive or a \\\\wsl.localhost path")
        })
    }
}

/// Select the item in the platform file manager (not just open its folder).
pub fn reveal(daemon_path: &str) -> Result<(), String> {
    let local = resolve(daemon_path)?;

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args([std::ffi::OsStr::new("-R"), local.as_os_str()])
            .status()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        // `explorer.exe /select,<path>` needs the path glued to the switch, and
        // it exits non-zero even when it succeeds — so the status is ignored and
        // only a spawn failure is reported.
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(local.as_os_str());
        std::process::Command::new("explorer.exe")
            .arg(arg)
            .spawn()
            .map_err(|e| format!("could not launch Explorer: {e}"))?;
        Ok(())
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let dir = local.parent().unwrap_or(&local);
        std::process::Command::new("xdg-open")
            .arg(dir)
            .status()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Move a path to the platform trash. Never a hard delete — see `remove`.
pub fn trash(daemon_path: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // Files inside the VM live on a 9p share, which has no Recycle Bin, so
        // the `trash` crate can't help there. Move them to the distro's own
        // freedesktop trash instead: still recoverable, still not an `rm`.
        if !is_drvfs(daemon_path) {
            return crate::wsl::trash(daemon_path);
        }
    }
    let local = resolve(daemon_path)?;
    trash::delete(&local).map_err(|e| e.to_string())
}

/// True when a daemon path is really a Windows path seen through `/mnt/`, and
/// so is handled by the ordinary Windows Recycle Bin.
#[cfg(target_os = "windows")]
fn is_drvfs(daemon_path: &str) -> bool {
    daemon_path.starts_with("/mnt/")
}

/// Bytes available on the filesystem holding `daemon_path`.
pub fn free_space(daemon_path: &str) -> Option<i64> {
    #[cfg(target_os = "windows")]
    {
        crate::wsl::free_space(daemon_path)
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Not implemented for the unix builds yet; the poller treats `None` as
        // "unknown" and hides the free-space readout.
        let _ = daemon_path;
        None
    }
}
