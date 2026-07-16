//! Pure derivation of presentation state from rtorrent's raw fields.
//!
//! Everything here is a deterministic function of its inputs (no I/O, no clock),
//! which is what makes it exhaustively unit-testable — see the tests at the
//! bottom, which cover every row of the design fixture. The rules are documented
//! in plan.md §5.4.
//!
//! Note on stall detection: the pure rule treats a zero instantaneous rate as
//! "stalled". The design fixtures reproduce exactly under that rule; the
//! time-window smoothing described in the plan is a refinement the poller can
//! layer on top without changing this function's contract.

use super::RawTorrent;
use crate::ipc::{Status, TorrentDto};

/// Classify a torrent's status from its raw flags.
pub fn status(t: &RawTorrent) -> Status {
    if !t.message.is_empty() {
        // A tracker/storage message means something is wrong.
        return Status::Error;
    }
    if t.hashing {
        return Status::Checking;
    }
    if !t.is_open || !t.is_active {
        // Stopped or paused (complete-and-stopped also lands here).
        return Status::Paused;
    }
    if t.complete {
        return Status::Seeding;
    }
    if t.down_rate > 0 {
        Status::Downloading
    } else {
        Status::Stalled
    }
}

/// Percentage complete in 0..=100.
pub fn percent(t: &RawTorrent) -> f64 {
    if t.size_bytes <= 0 {
        return 0.0;
    }
    (t.bytes_done as f64 / t.size_bytes as f64 * 100.0).clamp(0.0, 100.0)
}

/// Estimated seconds to completion. `None` means the UI shows ∞ or — depending
/// on status (there's no finite ETA for seeding/paused/complete/stalled).
pub fn eta_seconds(t: &RawTorrent, status: Status) -> Option<i64> {
    if status == Status::Downloading && t.down_rate > 0 {
        let remaining = (t.size_bytes - t.bytes_done).max(0);
        Some(remaining / t.down_rate)
    } else {
        None
    }
}

/// Display ratio (rtorrent stores it per-mille).
pub fn ratio(t: &RawTorrent) -> f64 {
    t.ratio_permille as f64 / 1000.0
}

/// Assemble the full DTO the frontend consumes. `tracker_host` comes from the
/// slow poll's per-hash cache and may be empty until resolved.
pub fn to_dto(
    t: &RawTorrent,
    tracker_host: &str,
    named_limits: Option<(i64, i64)>,
) -> TorrentDto {
    let st = status(t);
    TorrentDto {
        hash: t.hash.clone(),
        name: t.name.clone(),
        size: t.size_bytes,
        bytes_done: t.bytes_done,
        percent: percent(t),
        status: st,
        status_msg: t.message.clone(),
        seeds_connected: t.peers_complete.min(t.peers_connected),
        peers_connected: t.peers_connected,
        seeds_swarm: t.peers_complete,
        peers_swarm: t.peers_accounted,
        down_rate: t.down_rate,
        up_rate: t.up_rate,
        eta_seconds: eta_seconds(t, st),
        ratio: ratio(t),
        label: t.label.clone(),
        tracker_host: tracker_host.to_string(),
        save_path: if t.base_path.is_empty() {
            t.directory.clone()
        } else {
            t.base_path.clone()
        },
        priority: t.priority,
        is_private: t.is_private,
        throttle_name: t.throttle_name.clone(),
        down_rate_limit: named_limits.map(|limits| limits.0.saturating_mul(1024)),
        up_rate_limit: named_limits.map(|limits| limits.1.saturating_mul(1024)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a raw torrent with sensible "active download" defaults; tests then
    /// tweak just the fields that matter, mirroring the design fixture rows.
    fn raw() -> RawTorrent {
        RawTorrent {
            hash: "ABCD".into(),
            name: "x".into(),
            size_bytes: 1000,
            bytes_done: 500,
            complete: false,
            is_active: true,
            is_open: true,
            hashing: false,
            message: String::new(),
            down_rate: 100,
            up_rate: 0,
            ratio_permille: 190,
            ..Default::default()
        }
    }

    #[test]
    fn seeding_when_complete_and_active() {
        // e.g. ubuntu-24.04 — done 100, active.
        let mut t = raw();
        t.complete = true;
        t.bytes_done = t.size_bytes;
        t.down_rate = 0;
        assert_eq!(status(&t), Status::Seeding);
        assert_eq!(eta_seconds(&t, Status::Seeding), None);
    }

    #[test]
    fn downloading_when_incomplete_active_with_rate() {
        // e.g. Fedora — 67.4%, 8.4 MiB/s.
        let t = raw();
        assert_eq!(status(&t), Status::Downloading);
        // 500 bytes remaining at 100 B/s = 5s.
        assert_eq!(eta_seconds(&t, Status::Downloading), Some(5));
    }

    #[test]
    fn paused_when_not_active() {
        // e.g. linuxmint / raspios — paused.
        let mut t = raw();
        t.is_active = false;
        assert_eq!(status(&t), Status::Paused);
        assert_eq!(eta_seconds(&t, Status::Paused), None);
    }

    #[test]
    fn stalled_when_incomplete_active_zero_rate() {
        // e.g. openSUSE — 12%, 0 B/s.
        let mut t = raw();
        t.down_rate = 0;
        assert_eq!(status(&t), Status::Stalled);
    }

    #[test]
    fn error_when_message_present() {
        // e.g. Cosmos.Laundromat — tracker error.
        let mut t = raw();
        t.message = "Tracker: [Failure]".into();
        assert_eq!(status(&t), Status::Error);
    }

    #[test]
    fn checking_takes_priority_over_download() {
        let mut t = raw();
        t.hashing = true;
        assert_eq!(status(&t), Status::Checking);
    }

    #[test]
    fn ratio_is_permille() {
        let t = raw();
        assert!((ratio(&t) - 0.19).abs() < 1e-9);
    }

    #[test]
    fn percent_clamps_and_computes() {
        let mut t = raw();
        assert!((percent(&t) - 50.0).abs() < 1e-9);
        t.bytes_done = t.size_bytes;
        assert!((percent(&t) - 100.0).abs() < 1e-9);
        t.size_bytes = 0;
        assert_eq!(percent(&t), 0.0);
    }

    #[test]
    fn dto_includes_named_throttle_limits_in_bytes() {
        let mut t = raw();
        t.throttle_name = "rstorrent_1".into();
        let dto = to_dto(&t, "tracker.example", Some((512, 0)));
        assert_eq!(dto.throttle_name, "rstorrent_1");
        assert_eq!(dto.down_rate_limit, Some(512 * 1024));
        assert_eq!(dto.up_rate_limit, Some(0));
    }
}
