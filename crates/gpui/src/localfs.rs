//! Filesystem side-effects on paths that came from the daemon.
//!
//! A port of `src-tauri/src/localfs.rs`, narrowed to what the ported surfaces
//! need. rtorrent reports paths in its own namespace; on macOS and Linux that
//! namespace is this process's too, so these are thin wrappers. On Windows the
//! daemon runs inside WSL and every one of these crosses the boundary through
//! [`crate::wsl`]: `/mnt/c/…` is `C:\…`, anything else is the distro's
//! `\\wsl.localhost\…` share.
//!
//! Everything here assumes the caller has already checked
//! [`crate::settings::is_localhost`]; a remote daemon's files are not ours to
//! touch.

use std::path::{Path, PathBuf};

/// Resolve a daemon path to something the local OS can act on.
///
/// The `Err` is user-facing: it explains why a path cannot be reached rather
/// than failing silently.
///
/// # Errors
///
/// Returns a message on Windows when the path cannot be mapped (relative, or
/// VM-local with no WSL distro to name the share after).
pub fn resolve(daemon_path: &str) -> Result<PathBuf, String> {
    #[cfg(not(target_os = "windows"))]
    {
        Ok(PathBuf::from(daemon_path))
    }
    #[cfg(target_os = "windows")]
    {
        crate::wsl::to_windows(daemon_path)
            .ok_or_else(|| format!("{daemon_path} is inside WSL, which could not be reached"))
    }
}

/// Normalize a save directory chosen on *this* machine into the namespace the
/// daemon uses. On Windows this is where a `C:\…` path becomes `/mnt/c/…`.
///
/// # Errors
///
/// Returns a message on Windows for a path the VM cannot see (a network
/// share, a relative path).
pub fn to_daemon_path(picked: &str) -> Result<String, String> {
    #[cfg(not(target_os = "windows"))]
    {
        Ok(picked.to_owned())
    }
    #[cfg(target_os = "windows")]
    {
        // Already a daemon path (typed, or a stored setting), or empty (no
        // directory chosen): nothing to translate.
        if picked.is_empty() || picked.starts_with('/') {
            return Ok(picked.to_owned());
        }
        crate::wsl::to_wsl(Path::new(picked))
            .ok_or_else(|| format!("{picked} is not reachable from WSL, where rtorrent runs"))
    }
}

/// Select the item in the platform file manager, not just open its folder.
///
/// # Errors
///
/// Returns the launcher's error when the file manager cannot be started.
pub fn reveal(daemon_path: &str) -> Result<(), String> {
    let local = resolve(daemon_path)?;

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args([std::ffi::OsStr::new("-R"), local.as_os_str()])
            .status()
            .map_err(|error| error.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        // `explorer.exe /select,<path>` needs the path glued to the switch, and
        // it exits non-zero even when it succeeds — so only a spawn failure is
        // reported.
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(local.as_os_str());
        std::process::Command::new("explorer.exe")
            .arg(arg)
            .spawn()
            .map_err(|error| format!("could not launch Explorer: {error}"))?;
        Ok(())
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let dir = local.parent().unwrap_or(&local);
        std::process::Command::new("xdg-open")
            .arg(dir)
            .status()
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

/// Move a path to the platform trash. Never a hard delete — see `remove`.
///
/// # Errors
///
/// Returns the trash backend's error, or a message when the path cannot be
/// resolved.
pub fn trash(daemon_path: &str) -> Result<(), String> {
    // On Windows only drvfs paths (`/mnt/c/…`) are in the Recycle Bin's reach;
    // a path inside the VM goes to the distro's own freedesktop trash.
    #[cfg(target_os = "windows")]
    if crate::wsl::drvfs_to_windows(daemon_path).is_none() {
        return crate::wsl::trash(daemon_path);
    }
    let local = resolve(daemon_path)?;
    trash::delete(&local).map_err(|error| error.to_string())
}

/// True when `daemon_path` exists on this machine.
#[must_use]
pub fn exists(daemon_path: &str) -> bool {
    resolve(daemon_path).is_ok_and(|path| path.exists())
}

/// Bytes free on the volume holding `daemon_path`.
///
/// `None` when the path cannot be resolved (Windows without WSL) or
/// the volume cannot be queried; callers render that as "unknown" rather than a
/// number.
#[must_use]
pub fn free_space(daemon_path: &str) -> Option<i64> {
    #[cfg(target_os = "windows")]
    {
        // Asked of WSL itself: stat-ing the Windows side of the 9p share
        // would answer the host volume, not the distro's disk.
        crate::wsl::free_space(daemon_path)
    }
    #[cfg(not(target_os = "windows"))]
    {
        statvfs_free(&resolve(daemon_path).ok()?).and_then(|free| {
            u64::try_from(free)
                .ok()
                .and_then(|free| i64::try_from(free).ok())
        })
    }
}

#[cfg(not(target_os = "windows"))]
fn statvfs_free(path: &std::path::Path) -> Option<u64> {
    use std::mem::MaybeUninit;
    // Plain `statvfs`: one symbol, one struct shape on macOS and Linux, which
    // are the targets this shell ships on. The struct is only ever read after
    // a successful call, and only the two block-count fields the answer needs.
    let mut raw = MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `raw` is a valid zeroed `statvfs`; `path` is NUL-terminated via
    // the `CString` below, which outlives the call.
    let cpath = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
    let ok = unsafe { libc::statvfs(cpath.as_ptr(), raw.as_mut_ptr()) } == 0;
    if !ok {
        return None;
    }
    // SAFETY: the call succeeded, so every field is initialised.
    let stat = unsafe { raw.assume_init() };
    // Field widths differ by platform (`c_ulong` on macOS, `__fsword_t`/`u64` on
    // Linux), so go through `u64` explicitly on both sides.
    let block = u64::try_from(stat.f_bsize).unwrap_or(0);
    let avail = u64::try_from(stat.f_bavail).unwrap_or(0);
    block.checked_mul(avail).or(Some(0))
}

/// Move a torrent's on-disk data from `from_base_path` into `to_dir`.
///
/// Returns the new base path in the daemon's namespace. An empty source (a
/// torrent that has not written anything yet) is treated as already moved, so
/// relocation works for a torrent with no data.
///
/// # Errors
///
/// Returns the resolver's message, or [`rtorrent_core::fs::move_torrent_data`]'s
/// refusal when the destination already holds the data.
pub fn move_torrent_data(from_base_path: &str, to_dir: &str) -> Result<String, String> {
    if from_base_path.is_empty() {
        return Ok(to_dir.to_owned());
    }
    let source = resolve(from_base_path)?;
    let destination = resolve(to_dir)?;
    rtorrent_core::fs::move_torrent_data(&source, &destination)?;

    let name = Path::new(from_base_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.is_empty() {
        Ok(to_dir.to_owned())
    } else {
        Ok(format!("{}/{}", to_dir.trim_end_matches('/'), name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn unix_paths_pass_through_unchanged() {
        assert_eq!(resolve("/data/tv").unwrap(), PathBuf::from("/data/tv"));
        assert_eq!(to_daemon_path("/data/tv").unwrap(), "/data/tv");
    }

    #[test]
    fn an_empty_base_path_needs_no_move() {
        assert_eq!(move_torrent_data("", "/new").unwrap(), "/new");
    }

    #[test]
    fn missing_paths_are_not_present() {
        assert!(!exists("/definitely/not/here-9f3a1c"));
    }

    #[test]
    fn a_missing_volume_reports_no_free_space() {
        assert_eq!(free_space("/definitely/not/here-9f3a1c"), None);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn a_real_volume_reports_a_positive_number() {
        let free = free_space(std::env::temp_dir().to_str().unwrap_or("/tmp"));
        assert!(
            free.is_some_and(|bytes| bytes > 0),
            "temp dir should have space: {free:?}"
        );
    }
}
