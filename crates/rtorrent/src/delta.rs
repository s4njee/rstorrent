//! Delta computation between two [`Snapshot`]s (FND-02).
//!
//! The pollers (desktop and web) keep the last snapshot they published and
//! derive a [`SnapshotDelta`] by hashing on `TorrentDto.hash`. A torrent is
//! "added" when its hash is new, "removed" when it disappears, and "updated"
//! when it exists in both but any field differs (shallow compare of the
//! primitives the table renders). Globals/connection are always carried in the
//! delta so the client can replace them without diffing them separately.

use std::collections::{HashMap, HashSet};

use crate::types::{Snapshot, SnapshotDelta, TorrentDto};

fn same_torrent(a: &TorrentDto, b: &TorrentDto) -> bool {
    a.bytes_done == b.bytes_done
        && a.percent == b.percent
        && a.status == b.status
        && a.status_msg == b.status_msg
        && a.error_kind == b.error_kind
        && a.down_rate == b.down_rate
        && a.up_rate == b.up_rate
        && a.seeds_connected == b.seeds_connected
        && a.peers_connected == b.peers_connected
        && a.seeds_swarm == b.seeds_swarm
        && a.peers_swarm == b.peers_swarm
        && a.eta_seconds == b.eta_seconds
        && (a.ratio - b.ratio).abs() < f64::EPSILON
        && a.label == b.label
        && a.tags == b.tags
        && a.force_start == b.force_start
        && a.throttle_rule == b.throttle_rule
        && a.tracker_host == b.tracker_host
        && a.priority == b.priority
        && a.name == b.name
        && a.throttle_name == b.throttle_name
        && a.down_rate_limit == b.down_rate_limit
        && a.up_rate_limit == b.up_rate_limit
        && a.started_at == b.started_at
        && a.finished_at == b.finished_at
        && a.views == b.views
        && a.save_path == b.save_path
        && a.is_private == b.is_private
}

/// Compute a delta from `prev` to `next`.
///
/// `next.revision` must already be set (>= `prev.revision + 1`);
/// `base_revision` is copied from `prev.revision`.
pub fn diff(prev: &Snapshot, next: &Snapshot) -> SnapshotDelta {
    let prev_map: HashMap<&str, &TorrentDto> =
        prev.torrents.iter().map(|t| (t.hash.as_str(), t)).collect();
    let next_map: HashMap<&str, &TorrentDto> =
        next.torrents.iter().map(|t| (t.hash.as_str(), t)).collect();

    let prev_hashes: HashSet<&str> = prev_map.keys().copied().collect();
    let next_hashes: HashSet<&str> = next_map.keys().copied().collect();

    let mut added = Vec::new();
    let mut updated = Vec::new();
    let mut removed = Vec::new();

    for hash in next_hashes.difference(&prev_hashes) {
        if let Some(t) = next_map.get(*hash) {
            added.push((*t).clone());
        }
    }
    for hash in prev_hashes.difference(&next_hashes) {
        removed.push((*hash).to_string());
    }
    for hash in prev_hashes.intersection(&next_hashes) {
        let a = prev_map[hash];
        let b = next_map[hash];
        if !same_torrent(a, b) {
            updated.push(b.clone());
        }
    }

    // Deterministic order for stable ETags / tests.
    added.sort_by(|a, b| a.hash.cmp(&b.hash));
    updated.sort_by(|a, b| a.hash.cmp(&b.hash));
    removed.sort();

    SnapshotDelta {
        revision: next.revision,
        base_revision: prev.revision,
        added,
        updated,
        removed,
        globals: next.globals.clone(),
        connection: next.connection.clone(),
    }
}

/// Apply a delta to a snapshot, producing the next snapshot. Returns `None` if
/// `delta.base_revision != snapshot.revision` (caller must re-sync via full).
pub fn apply(snapshot: &Snapshot, delta: &SnapshotDelta) -> Option<Snapshot> {
    if delta.base_revision != snapshot.revision {
        return None;
    }
    let removed: HashSet<&str> = delta.removed.iter().map(|s| s.as_str()).collect();
    let updated_map: HashMap<&str, &TorrentDto> =
        delta.updated.iter().map(|t| (t.hash.as_str(), t)).collect();

    let mut torrents: Vec<TorrentDto> =
        Vec::with_capacity(snapshot.torrents.len() - removed.len() + delta.added.len());
    for t in &snapshot.torrents {
        if removed.contains(t.hash.as_str()) {
            continue;
        }
        if let Some(u) = updated_map.get(t.hash.as_str()) {
            torrents.push((*u).clone());
        } else {
            torrents.push(t.clone());
        }
    }
    for t in &delta.added {
        torrents.push(t.clone());
    }
    // Keep deterministic hash order for tests / UI stable sort base.
    torrents.sort_by(|a, b| a.hash.cmp(&b.hash));

    Some(Snapshot {
        revision: delta.revision,
        torrents,
        globals: delta.globals.clone(),
        connection: delta.connection.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ConnPhase, ConnState, GlobalStats, Status};

    fn conn() -> ConnState {
        ConnState {
            phase: ConnPhase::Connected,
            endpoint: "test".into(),
            daemon_version: Some("0.9.8".into()),
            error: None,
            retry_in_seconds: None,
        }
    }
    fn globals() -> GlobalStats {
        GlobalStats {
            down_rate: 0,
            up_rate: 0,
            down_rate_limit: 0,
            up_rate_limit: 0,
            dht_nodes: 0,
            free_space: None,
            disk_size: None,
            turtle_active: false,
        }
    }
    fn dto(hash: &str, name: &str) -> TorrentDto {
        TorrentDto {
            hash: hash.into(),
            name: name.into(),
            size: 100,
            bytes_done: 100,
            percent: 100.0,
            status: Status::Seeding,
            status_msg: String::new(),
            seeds_connected: 0,
            peers_connected: 0,
            seeds_swarm: 0,
            peers_swarm: 0,
            down_rate: 0,
            up_rate: 0,
            eta_seconds: None,
            ratio: 1.0,
            label: String::new(),
            tracker_host: String::new(),
            save_path: String::new(),
            priority: 0,
            is_private: false,
            throttle_name: String::new(),
            down_rate_limit: None,
            up_rate_limit: None,
            started_at: 0,
            finished_at: 0,
            peers_max: 0,
            peers_min: 0,
            uploads_max: 0,
            connection_type: String::new(),
            added_by: String::new(),
            source_path: String::new(),
            added_at: 0,
            tags: Vec::new(),
            force_start: false,
            throttle_rule: String::new(),
            views: vec![],
            error_kind: String::new(),
            is_open: true,
            is_active: true,
        }
    }

    #[test]
    fn diff_detects_added_updated_removed() {
        let prev = Snapshot {
            revision: 1,
            torrents: vec![dto("A", "a"), dto("B", "b")],
            globals: globals(),
            connection: conn(),
        };
        let next = Snapshot {
            revision: 2,
            torrents: vec![dto("B", "b2"), dto("C", "c")],
            globals: globals(),
            connection: conn(),
        };
        let d = diff(&prev, &next);
        assert_eq!(d.base_revision, 1);
        assert_eq!(d.revision, 2);
        assert_eq!(d.added.len(), 1);
        assert_eq!(d.added[0].hash, "C");
        assert_eq!(d.updated.len(), 1);
        assert_eq!(d.updated[0].hash, "B");
        assert_eq!(d.removed, vec!["A"]);
    }

    #[test]
    fn diff_treats_a_tag_only_change_as_an_update() {
        let prev = Snapshot {
            revision: 1,
            torrents: vec![dto("A", "a")],
            globals: globals(),
            connection: conn(),
        };
        let mut tagged = dto("A", "a");
        tagged.tags = vec!["linux".into()];
        let next = Snapshot {
            revision: 2,
            torrents: vec![tagged],
            globals: globals(),
            connection: conn(),
        };
        let d = diff(&prev, &next);
        assert_eq!(d.updated.len(), 1, "a tag change must reach the client");
        assert_eq!(d.updated[0].tags, vec!["linux"]);
    }

    #[test]
    fn apply_reconstructs_next() {
        let prev = Snapshot {
            revision: 5,
            torrents: vec![dto("A", "a"), dto("B", "b")],
            globals: globals(),
            connection: conn(),
        };
        let next = Snapshot {
            revision: 6,
            torrents: vec![dto("B", "b2"), dto("C", "c")],
            globals: globals(),
            connection: conn(),
        };
        let d = diff(&prev, &next);
        let applied = apply(&prev, &d).unwrap();
        assert_eq!(applied.revision, 6);
        assert_eq!(applied.torrents.len(), 2);
        // sorted by hash
        assert_eq!(applied.torrents[0].hash, "B");
        assert_eq!(applied.torrents[1].hash, "C");
        assert_eq!(applied.torrents[0].name, "b2");
    }

    #[test]
    fn apply_rejects_wrong_base() {
        let prev = Snapshot {
            revision: 5,
            torrents: vec![],
            globals: globals(),
            connection: conn(),
        };
        let delta = SnapshotDelta {
            revision: 6,
            base_revision: 4,
            added: vec![],
            updated: vec![],
            removed: vec![],
            globals: globals(),
            connection: conn(),
        };
        assert!(apply(&prev, &delta).is_none());
    }

    #[test]
    fn diff_no_changes_yields_empty_delta() {
        let s = Snapshot {
            revision: 1,
            torrents: vec![dto("A", "a")],
            globals: globals(),
            connection: conn(),
        };
        let next = Snapshot {
            revision: 2,
            torrents: vec![dto("A", "a")],
            globals: globals(),
            connection: conn(),
        };
        let d = diff(&s, &next);
        assert!(d.added.is_empty() && d.updated.is_empty() && d.removed.is_empty());
    }
}
