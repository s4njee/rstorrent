//! Free/total bytes for the disk card, via `statvfs` on unix.
//!
//! The server's real deployment is Linux (next to the daemon), where this is a
//! cheap syscall. On non-unix dev hosts it returns `None`, so the disk card
//! hides — mock mode supplies its own fixture figures regardless.

/// `(free_bytes, total_bytes)` for the filesystem holding `path`, or `None`.
#[cfg(unix)]
pub fn disk_usage(path: &str) -> Option<(i64, i64)> {
    use std::ffi::CString;
    let c_path = CString::new(path).ok()?;
    // SAFETY: `stat` is zeroed then filled by `statvfs`; we only read it when the
    // call returns 0, and `c_path` outlives the call.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if rc != 0 {
        return None;
    }
    let frsize = stat.f_frsize as i64;
    let free = frsize.checked_mul(stat.f_bavail as i64)?;
    let total = frsize.checked_mul(stat.f_blocks as i64)?;
    (total > 0).then_some((free, total))
}

#[cfg(not(unix))]
pub fn disk_usage(_path: &str) -> Option<(i64, i64)> {
    None
}
