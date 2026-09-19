//! Error taxonomy for `d.message` (D19).
//!
//! rtorrent surfaces every per-torrent failure through a single free-form
//! `d.message` string — tracker rejections, timeouts, and local storage errors
//! all land in the same field. The UI's old "trk error" bucket made a missing
//! data set look identical to a dead tracker, so the taxonomy below sorts a
//! message into a small set of actionable buckets, ordered by specificity so a
//! more precise match wins.

/// Structured error buckets derived from `d.message`.
///
/// The string keys are the values serialised as `TorrentDto.errorKind` and used
/// as sidebar filter keys. Keep them snake_case and stable — they are persisted
/// implicitly in URLs / localStorage filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// Private-tracker rejection: unregistered / unknown / not found / bad passkey.
    Unregistered,
    /// Tracker contact failed: timeout / unreachable / DNS / connection refused.
    TrackerTimeout,
    /// Generic tracker failure (e.g. `Tracker: [Failure reason "..."]`).
    TrackerError,
    /// Local data missing: chunk read error / no such file.
    MissingFiles,
    /// No space left on device.
    NoSpace,
    /// Permission / access denied / read-only.
    Permission,
    /// Other disk / storage failures.
    DiskError,
    /// Non-empty message that matched none of the above.
    Other,
}

impl ErrorKind {
    /// Stable wire key for this bucket (used as `errorKind` in the DTO).
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Unregistered => "unregistered",
            ErrorKind::TrackerTimeout => "tracker_timeout",
            ErrorKind::TrackerError => "tracker_error",
            ErrorKind::MissingFiles => "missing_files",
            ErrorKind::NoSpace => "no_space",
            ErrorKind::Permission => "permission",
            ErrorKind::DiskError => "disk_error",
            ErrorKind::Other => "other",
        }
    }

    /// Short lowercase label shown in the Status column for this bucket.
    pub fn label(self) -> &'static str {
        match self {
            ErrorKind::Unregistered => "unregistered",
            ErrorKind::TrackerTimeout => "timeout",
            ErrorKind::TrackerError => "trk error",
            ErrorKind::MissingFiles => "missing files",
            ErrorKind::NoSpace => "no space",
            ErrorKind::Permission => "permission",
            ErrorKind::DiskError => "disk error",
            ErrorKind::Other => "error",
        }
    }
}

/// Classify a non-empty `d.message` into a bucket, or `None` when healthy.
///
/// The checks are deliberately substring-based and case-insensitive — rtorrent's
/// messages vary by build, tracker software, and locale-adjacent phrasing, but
/// the keywords below have been stable for years. Priority is load-bearing:
/// unregistered and local-storage failures are tested before the broad tracker
/// timeout / tracker catch-alls so a precise bucket wins.
pub fn classify(msg: &str) -> Option<ErrorKind> {
    if msg.trim().is_empty() {
        return None;
    }
    let lower = msg.to_ascii_lowercase();

    // 1. Private-tracker unregistered / auth failures — must beat generic "tracker".
    if contains_any(
        &lower,
        &[
            "unregistered",
            "not registered",
            "torrent not found",
            "unknown torrent",
            "invalid torrent",
            "unrecognized torrent",
            "not authorized",
            "not authorised",
            "unauthorized",
            "unauthorised",
            "passkey",
            "bad passkey",
            "invalid passkey",
        ],
    ) {
        return Some(ErrorKind::Unregistered);
    }

    // 2. No space / disk full — must beat missing_files' generic "file" phrasing.
    if contains_any(
        &lower,
        &[
            "no space",
            "no space left",
            "disk full",
            "not enough disk space",
            "no disk space",
        ],
    ) {
        return Some(ErrorKind::NoSpace);
    }

    // 3. Permission / read-only — must beat missing_files ("could not open file").
    if contains_any(
        &lower,
        &[
            "permission denied",
            "access denied",
            "read-only file system",
            "read only file system",
            "read-only filesystem",
        ],
    ) {
        return Some(ErrorKind::Permission);
    }

    // 4. Local data missing — the CachyOS episode: moved/missing files after a
    //    completed torrent's data vanished. Distinct affordances (recheck / relocate).
    if contains_any(
        &lower,
        &[
            "no such file",
            "file not found",
            "file chunk read error",
            "chunk read error",
            "missing file",
            "bad file descriptor",
        ],
    ) {
        return Some(ErrorKind::MissingFiles);
    }
    // "could not open file" / "failed to open" are only storage-missing when they
    // don't already carry a more specific hint (permission/no-space above).
    if (lower.contains("could not open file")
        || lower.contains("couldn't open file")
        || lower.contains("failed to open")
        || lower.contains("open failed"))
        && !lower.contains("tracker")
    {
        return Some(ErrorKind::MissingFiles);
    }

    // 5. Tracker connectivity / timeout — transient, retryable via reannounce.
    if contains_any(
        &lower,
        &[
            "timed out",
            "timeout",
            "could not connect",
            "couldn't connect",
            "cannot connect",
            "connection timed out",
            "connection refused",
            "connection reset",
            "no connection",
            "unreachable",
            "host not found",
            "could not resolve host",
            "could not resolve",
            "name resolution",
            "network is unreachable",
            "network unreachable",
        ],
    ) {
        return Some(ErrorKind::TrackerTimeout);
    }

    // 6. Generic tracker failure — any message that mentions the tracker.
    if lower.contains("tracker") {
        return Some(ErrorKind::TrackerError);
    }

    // 7. Generic storage / disk phrasing.
    if contains_any(
        &lower,
        &[
            "storage error",
            "storage failed",
            "disk error",
            "disk i/o error",
            "io error",
            "file error",
        ],
    ) {
        return Some(ErrorKind::DiskError);
    }
    // Also catch residual storage-adjacent messages that mention file/directory
    // alongside an error verb — avoids misclassifying arbitrary file names.
    if (lower.contains("file") || lower.contains("directory"))
        && (lower.contains("error") || lower.contains("failed") || lower.contains("failure"))
    {
        return Some(ErrorKind::DiskError);
    }

    Some(ErrorKind::Other)
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_none() {
        assert_eq!(classify(""), None);
        assert_eq!(classify("   "), None);
    }

    #[test]
    fn unregistered_variants() {
        for msg in [
            r#"Tracker: [Failure reason "unregistered torrent"]"#,
            "torrent not found",
            "Unknown torrent",
            "bad passkey",
            "not authorized",
        ] {
            assert_eq!(classify(msg), Some(ErrorKind::Unregistered), "{msg}");
        }
    }

    #[test]
    fn missing_files() {
        assert_eq!(
            classify("Storage error: [File chunk read error: No such file or directory]"),
            Some(ErrorKind::MissingFiles)
        );
        assert_eq!(
            classify("Could not open file /tmp/x: No such file or directory"),
            Some(ErrorKind::MissingFiles)
        );
    }

    #[test]
    fn no_space() {
        assert_eq!(
            classify("Could not create file: No space left on device"),
            Some(ErrorKind::NoSpace)
        );
    }

    #[test]
    fn permission() {
        assert_eq!(
            classify("Could not open file: Permission denied"),
            Some(ErrorKind::Permission)
        );
    }

    #[test]
    fn timeout_before_generic_tracker() {
        assert_eq!(
            classify("Tracker: [Could not connect to tracker: Connection timed out]"),
            Some(ErrorKind::TrackerTimeout)
        );
        assert_eq!(
            classify("request timed out"),
            Some(ErrorKind::TrackerTimeout)
        );
    }

    #[test]
    fn generic_tracker() {
        assert_eq!(
            classify(r#"Tracker: [Failure reason "failure"]"#),
            Some(ErrorKind::TrackerError)
        );
    }

    #[test]
    fn generic_storage() {
        assert_eq!(
            classify("Storage error: [disk i/o error]"),
            Some(ErrorKind::DiskError)
        );
        assert_eq!(
            classify("File error: something failed"),
            Some(ErrorKind::DiskError)
        );
    }

    #[test]
    fn other_fallback() {
        assert_eq!(classify("something unexpected"), Some(ErrorKind::Other));
    }

    #[test]
    fn priority_missing_over_tracker_text() {
        // A message that mentions both tracker and file - missing should win.
        assert_eq!(
            classify("Tracker: file chunk read error: No such file or directory"),
            Some(ErrorKind::MissingFiles)
        );
    }
}
