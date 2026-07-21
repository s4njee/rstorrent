//! Host-agnostic snapshot assembly.
//!
//! The shared core of what each host's poller does per tick: turn the raw
//! torrent list + globals into the presentation [`Snapshot`] DTOs. The inputs
//! that differ per host — how tracker hosts are cached, which per-torrent
//! throttle limits apply, how free/total disk is probed — are passed in as
//! lookups/values, so this stays pure and both the desktop poller and the
//! `rstorrent-web` server call it. The *automation* around a tick (seed goals,
//! the active-downloads queue, turtle mode, notifications) is deliberately not
//! here — that is host policy, not assembly.

use crate::rtorrent::{derive, RawGlobal, RawTorrent};
use crate::types::{GlobalStats, TorrentDto};

/// Map raw torrents to presentation DTOs.
///
/// `tracker_host` resolves a hash to its primary tracker host (empty if not yet
/// known); `throttle_limits` resolves a torrent's named-throttle to its
/// `(down_kb, up_kb)` limits, or `None` when it rides the global throttle.
pub fn to_dtos(
    raw: &[RawTorrent],
    tracker_host: impl Fn(&str) -> String,
    throttle_limits: impl Fn(&str) -> Option<(i64, i64)>,
) -> Vec<TorrentDto> {
    raw.iter()
        .map(|t| derive::to_dto(t, &tracker_host(&t.hash), throttle_limits(&t.throttle_name)))
        .collect()
}

/// Assemble the global-stats DTO from raw globals plus host-provided disk
/// figures and turtle state.
pub fn to_globals(
    g: &RawGlobal,
    free_space: Option<i64>,
    disk_size: Option<i64>,
    turtle_active: bool,
) -> GlobalStats {
    GlobalStats {
        down_rate: g.down_rate,
        up_rate: g.up_rate,
        down_rate_limit: g.down_rate_limit,
        up_rate_limit: g.up_rate_limit,
        dht_nodes: g.dht_nodes,
        free_space,
        disk_size,
        turtle_active,
    }
}

/// The zeroed globals a host emits while disconnected.
pub fn empty_globals() -> GlobalStats {
    to_globals(&RawGlobal::default(), None, None, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(hash: &str, throttle: &str) -> RawTorrent {
        RawTorrent {
            hash: hash.into(),
            throttle_name: throttle.into(),
            ..RawTorrent::default()
        }
    }

    #[test]
    fn to_dtos_applies_the_host_lookups() {
        let raw = vec![raw("A", ""), raw("B", "slow")];
        let dtos = to_dtos(
            &raw,
            |h| format!("tracker.{h}.example"),
            |name| (name == "slow").then_some((100, 50)),
        );
        assert_eq!(dtos[0].tracker_host, "tracker.A.example");
        assert_eq!(dtos[0].down_rate_limit, None);
        assert_eq!(dtos[1].tracker_host, "tracker.B.example");
        // 100 KiB/s -> bytes/s in the DTO.
        assert_eq!(dtos[1].down_rate_limit, Some(100 * 1024));
        assert_eq!(dtos[1].up_rate_limit, Some(50 * 1024));
    }

    #[test]
    fn empty_globals_is_zeroed_and_unturtled() {
        let g = empty_globals();
        assert_eq!(g.down_rate, 0);
        assert_eq!(g.free_space, None);
        assert_eq!(g.disk_size, None);
        assert!(!g.turtle_active);
    }
}
