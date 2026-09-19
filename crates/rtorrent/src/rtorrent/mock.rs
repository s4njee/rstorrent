//! [`MockClient`] — an in-memory rtorrent stand-in.
//!
//! Backs [`RtorrentApi`] with the ten torrents from the design reference
//! (`design/rTorrent Client 1c.dc.html`), so the entire UI runs and demos with
//! no daemon (`RSTORRENT_MOCK=1`). Downloading torrents advance their progress on
//! each `list_snapshot` based on real elapsed time, and mutating calls (stop,
//! start, erase, set_label…) actually change the fixture state, so the app feels
//! live. It's also the fixture source for the transport/derive tests.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

use super::{LoadOptions, RawGlobal, RawTorrent, Result, RtorrentApi};
use crate::types::{FileNode, PeerRow, TrackerRow};

const GIB: f64 = 1_073_741_824.0;
const MIB: f64 = 1_048_576.0;
const KIB: f64 = 1_024.0;

/// How many torrents [`fixtures`] serves: the design's fourteen sample rows plus
/// one `media`/stalled row the design's sidebar vocabulary needs (see
/// [`design_rows`]). Exported so a host's tests assert against the set rather
/// than a literal that silently drifts.
pub const FIXTURE_COUNT: usize = 15;

/// Mutable fixture state guarded by a mutex (locks are held only for the brief,
/// non-awaiting critical sections that read or mutate the torrent list).
struct State {
    torrents: Vec<RawTorrent>,
    trackers: HashMap<String, Vec<MockTracker>>,
    natural_rates: HashMap<String, (i64, i64)>,
    throttles: HashMap<String, (i64, i64)>,
    file_priorities: HashMap<(String, usize), i64>,
    last_tick: Instant,
}

#[derive(Clone)]
struct MockTracker {
    url: String,
    enabled: bool,
    seeds: i64,
    leeches: i64,
    /// Unix seconds of the last announce (0 = never), matching the real DTO.
    last_announce: i64,
}

impl MockTracker {
    fn row(&self, index: usize) -> TrackerRow {
        let kind = if self.url.starts_with("udp") {
            "udp"
        } else if self.url.starts_with("http") {
            "http"
        } else {
            "dht"
        };
        TrackerRow {
            index,
            url: self.url.clone(),
            enabled: self.enabled,
            status: if self.enabled {
                "working".into()
            } else {
                "disabled".into()
            },
            seeds: self.seeds,
            leeches: self.leeches,
            kind: kind.into(),
            // Next announce ~28 min out, as a real working tracker reports.
            next_announce: if self.enabled { unix_now() + 1680 } else { 0 },
            last_announce: self.last_announce,
        }
    }
}

pub struct MockClient {
    state: Mutex<State>,
    /// Global config values (variable name → value). Writes land here and reads
    /// see them, so a settings save in mock mode behaves like a real daemon.
    config: Mutex<HashMap<String, String>>,
}

impl Default for MockClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MockClient {
    pub fn new() -> Self {
        let torrents = fixtures();
        let trackers = torrents
            .iter()
            .map(|torrent| {
                (
                    torrent.hash.clone(),
                    vec![mock_tracker(tracker_host_for(&torrent.hash))],
                )
            })
            .collect();
        let natural_rates = torrents
            .iter()
            .map(|torrent| (torrent.hash.clone(), (torrent.down_rate, torrent.up_rate)))
            .collect();
        Self {
            state: Mutex::new(State {
                torrents,
                trackers,
                natural_rates,
                throttles: HashMap::new(),
                file_priorities: HashMap::new(),
                last_tick: Instant::now(),
            }),
            config: Mutex::new(HashMap::new()),
        }
    }

    /// Advance simulated download progress and seeding ratios by elapsed time.
    fn tick(state: &mut State) {
        let now = Instant::now();
        let dt = now.duration_since(state.last_tick).as_secs_f64();
        state.last_tick = now;
        let State {
            torrents,
            natural_rates,
            throttles,
            ..
        } = state;
        for t in torrents {
            let (down_rate, _) = effective_rates(t, natural_rates, throttles);
            if t.is_active && !t.complete && down_rate > 0 {
                t.bytes_done += (down_rate as f64 * dt) as i64;
                if t.bytes_done >= t.size_bytes {
                    // Finished: flip to a seeding state.
                    t.bytes_done = t.size_bytes;
                    t.complete = true;
                    t.down_rate = 0;
                    if let Some(rates) = natural_rates.get_mut(&t.hash) {
                        rates.0 = 0;
                    }
                    t.finished_at = unix_now();
                }
            } else if t.is_active && t.complete && t.up_rate > 0 && t.size_bytes > 0 {
                // Run seeding faster than wall time so a low goal is easy to
                // exercise during a short mock-mode development session.
                let uploaded = t.up_rate as f64 * dt * 20.0;
                t.ratio_permille += (uploaded * 1000.0 / t.size_bytes as f64) as i64;
            }
        }
    }

    fn with_hash<F: FnMut(&mut RawTorrent)>(&self, hashes: &[String], mut f: F) {
        let mut state = self.state.lock().unwrap();
        for t in &mut state.torrents {
            if hashes.iter().any(|h| h.eq_ignore_ascii_case(&t.hash)) {
                f(t);
            }
        }
    }
}

#[async_trait]
impl RtorrentApi for MockClient {
    async fn client_version(&self) -> Result<String> {
        Ok("0.9.8".to_string())
    }

    async fn list_snapshot(&self) -> Result<Vec<RawTorrent>> {
        let mut state = self.state.lock().unwrap();
        Self::tick(&mut state);
        Ok(state
            .torrents
            .iter()
            .cloned()
            .map(|mut torrent| {
                let rates = effective_rates(&torrent, &state.natural_rates, &state.throttles);
                torrent.down_rate = rates.0;
                torrent.up_rate = rates.1;
                torrent
            })
            .collect())
    }

    async fn global_stats(&self) -> Result<RawGlobal> {
        let state = self.state.lock().unwrap();
        let rates = state
            .torrents
            .iter()
            .map(|torrent| effective_rates(torrent, &state.natural_rates, &state.throttles));
        let (down_rate, up_rate) = rates.fold((0, 0), |sum, rate| (sum.0 + rate.0, sum.1 + rate.1));
        Ok(RawGlobal {
            down_rate,
            up_rate,
            down_rate_limit: 0,                // ∞
            up_rate_limit: (5.0 * MIB) as i64, // 5.0 MiB/s (matches design footer)
            dht_nodes: 387,
        })
    }

    async fn primary_tracker(&self, hash: &str) -> Result<String> {
        let state = self.state.lock().unwrap();
        let url = state
            .trackers
            .get(hash)
            .and_then(|trackers| {
                trackers
                    .iter()
                    .find(|tracker| tracker.enabled)
                    .or_else(|| trackers.first())
            })
            .map(|tracker| tracker.url.as_str())
            .unwrap_or("");
        Ok(tracker_host(url))
    }

    async fn trackers(&self, hash: &str) -> Result<Vec<TrackerRow>> {
        let state = self.state.lock().unwrap();
        Ok(state
            .trackers
            .get(hash)
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(index, tracker)| tracker.row(index))
            .collect())
    }

    async fn add_tracker(&self, hash: &str, url: &str) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state
            .trackers
            .entry(hash.to_string())
            .or_default()
            .push(mock_tracker(url));
        Ok(())
    }

    async fn remove_tracker(&self, hash: &str, index: usize) -> Result<()> {
        // Mock rtorrent identifies as 0.9.8, which has no d.tracker.remove;
        // mirror the real client's compatibility fallback by disabling it.
        self.set_tracker_enabled(hash, index, false).await
    }

    async fn set_tracker_enabled(&self, hash: &str, index: usize, enabled: bool) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(tracker) = state
            .trackers
            .get_mut(hash)
            .and_then(|trackers| trackers.get_mut(index))
        {
            tracker.enabled = enabled;
        }
        Ok(())
    }

    async fn force_reannounce(&self, hashes: &[String]) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        for hash in hashes {
            if let Some(trackers) = state.trackers.get_mut(hash) {
                for tracker in trackers.iter_mut().filter(|tracker| tracker.enabled) {
                    tracker.last_announce = unix_now();
                }
            }
        }
        Ok(())
    }

    async fn peers(&self, _hash: &str) -> Result<Vec<PeerRow>> {
        Ok(vec![
            PeerRow {
                id: "PEER0001".into(),
                address: "203.0.113.7".into(),
                client: "libtorrent 2.0.9".into(),
                progress: 84.0,
                down_rate: (1.2 * MIB) as i64,
                up_rate: (120.0 * KIB) as i64,
                flags: "EIP".into(),
            },
            PeerRow {
                id: "PEER0002".into(),
                address: "198.51.100.42".into(),
                client: "qBittorrent 4.6".into(),
                progress: 61.0,
                down_rate: (640.0 * KIB) as i64,
                up_rate: 0,
                flags: "EO".into(),
            },
        ])
    }

    async fn ban_peer(&self, _hash: &str, _peer_id: &str) -> Result<()> {
        Ok(())
    }

    async fn snub_peer(&self, _hash: &str, _peer_id: &str) -> Result<()> {
        Ok(())
    }

    async fn disconnect_peer(&self, _hash: &str, _peer_id: &str) -> Result<()> {
        Ok(())
    }

    async fn files(&self, hash: &str) -> Result<Vec<FileNode>> {
        let state = self.state.lock().unwrap();
        let priority = |index: usize, default: i64| {
            state
                .file_priorities
                .get(&(hash.to_ascii_uppercase(), index))
                .copied()
                .unwrap_or(default)
        };
        Ok(vec![
            FileNode {
                path: "Fedora-Workstation-Live.iso".into(),
                size: (2.29 * GIB) as i64,
                priority: priority(0, 1),
                progress: 67.0,
                is_dir: false,
            },
            FileNode {
                path: "CHECKSUM".into(),
                size: 1400,
                priority: priority(1, 1),
                progress: 100.0,
                is_dir: false,
            },
        ])
    }

    async fn pieces(&self, hash: &str) -> Result<crate::types::PieceInfo> {
        let state = self.state.lock().unwrap();
        let t = state
            .torrents
            .iter()
            .find(|t| t.hash.eq_ignore_ascii_case(hash));
        let (size_bytes, done_bytes) = match t {
            Some(t) => (t.size_bytes, t.bytes_done),
            None => (0, 0),
        };
        // Model 512 KiB chunks like a typical large torrent.
        let chunk_size = 512 * 1024;
        let size_chunks = ((size_bytes + chunk_size - 1) / chunk_size).max(1);
        let completed_chunks = (done_bytes / chunk_size).min(size_chunks);

        // Synthesize a plausible bitfield: mostly-contiguous completed pieces
        // with a scattered leading edge, so the mock bar looks like a real
        // download rather than a solid block.
        let mut bits = vec![false; size_chunks as usize];
        let solid = (completed_chunks as f64 * 0.85) as usize;
        for (i, b) in bits.iter_mut().enumerate() {
            if i < solid {
                *b = true;
            }
        }
        // Scatter the remaining completed chunks ahead of the solid region.
        let mut remaining = completed_chunks as usize - solid.min(completed_chunks as usize);
        let mut i = solid;
        while remaining > 0 && i < bits.len() {
            // Deterministic pseudo-scatter (every 3rd chunk).
            if i % 3 == 0 {
                bits[i] = true;
                remaining -= 1;
            }
            i += 1;
        }

        // Synthesize per-chunk peer availability (`d.chunks_seen`): a gentle
        // swarm "wave" of 2..6 peers with one scarce single-peer valley, so the
        // mock availability bar reads like a real swarm rather than a flat block.
        let seen: Vec<u8> = (0..size_chunks as usize)
            .map(|i| {
                let phase = i as f64 / size_chunks.max(1) as f64;
                if (0.42..0.52).contains(&phase) {
                    1 // a rare stretch: only a single peer has these chunks
                } else {
                    ((phase * std::f64::consts::PI * 3.0).sin() * 2.5 + 3.5).round() as u8
                }
            })
            .collect();

        Ok(crate::types::PieceInfo {
            size_chunks,
            completed_chunks,
            chunk_size,
            bitfield: bits_to_hex(&bits),
            availability: Some(bytes_to_hex(&seen)),
        })
    }

    async fn start(&self, hashes: &[String]) -> Result<()> {
        self.with_hash(hashes, |t| {
            t.is_active = true;
            t.is_open = true;
            t.message.clear();
        });
        Ok(())
    }

    async fn stop(&self, hashes: &[String]) -> Result<()> {
        self.with_hash(hashes, |t| {
            t.is_active = false;
            t.is_open = false;
            t.down_rate = 0;
            t.up_rate = 0;
        });
        Ok(())
    }

    async fn pause(&self, hashes: &[String]) -> Result<()> {
        // Paused, not stopped: still loaded, so it keeps its place in the swarm
        // and its peers — the same shape as a queued torrent.
        self.with_hash(hashes, |t| {
            t.is_active = false;
            t.down_rate = 0;
            t.up_rate = 0;
        });
        Ok(())
    }

    async fn recheck(&self, hashes: &[String]) -> Result<()> {
        self.with_hash(hashes, |t| t.hashing = true);
        Ok(())
    }

    async fn erase(&self, hashes: &[String]) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state
            .torrents
            .retain(|t| !hashes.iter().any(|h| h.eq_ignore_ascii_case(&t.hash)));
        state
            .trackers
            .retain(|hash, _| !hashes.iter().any(|h| h.eq_ignore_ascii_case(hash)));
        state
            .natural_rates
            .retain(|hash, _| !hashes.iter().any(|h| h.eq_ignore_ascii_case(hash)));
        Ok(())
    }

    async fn load_raw(&self, bytes: Vec<u8>, opts: LoadOptions) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        // A real `.torrent` keeps its own name, size and info-hash (so duplicate
        // detection works against the fixture); unreadable bytes fall back to a
        // synthetic name, which is what the transport tests hand it.
        let torrent = match crate::torrent_file::read_metadata_bytes(&bytes) {
            Ok(meta) => {
                let mut torrent = new_download(&meta.name, &opts);
                torrent.hash = meta.info_hash.to_uppercase();
                torrent.size_bytes = meta.size;
                torrent.is_private = meta.is_private;
                torrent
            }
            Err(_) => new_download("added-from-file.iso", &opts),
        };
        state.trackers.insert(
            torrent.hash.clone(),
            vec![mock_tracker(tracker_host_for(&torrent.hash))],
        );
        state
            .natural_rates
            .insert(torrent.hash.clone(), (torrent.down_rate, torrent.up_rate));
        state.torrents.push(torrent);
        Ok(())
    }

    async fn load_magnet(&self, uri: &str, opts: LoadOptions) -> Result<()> {
        // Pull a display name out of the magnet's `dn=` if present.
        let name = uri
            .split(['&', '?'])
            .find_map(|p| p.strip_prefix("dn="))
            .unwrap_or("magnet-download")
            .to_string();
        let mut state = self.state.lock().unwrap();
        let torrent = new_download(&name, &opts);
        state.trackers.insert(
            torrent.hash.clone(),
            vec![mock_tracker(tracker_host_for(&torrent.hash))],
        );
        state
            .natural_rates
            .insert(torrent.hash.clone(), (torrent.down_rate, torrent.up_rate));
        state.torrents.push(torrent);
        Ok(())
    }

    async fn set_label(&self, hashes: &[String], label: &str) -> Result<()> {
        self.with_hash(hashes, |t| t.label = label.to_string());
        Ok(())
    }

    async fn set_tags(&self, hashes: &[String], tags: &[String]) -> Result<()> {
        let normalised = crate::tags::normalise(tags);
        self.with_hash(hashes, |t| t.tags = normalised.clone());
        Ok(())
    }

    async fn set_directory(&self, hash: &str, path: &str) -> Result<()> {
        self.with_hash(&[hash.to_string()], |t| {
            t.directory = path.to_string();
            let base_name = std::path::Path::new(&t.base_path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| t.name.clone());
            t.base_path = format!("{}/{}", path.trim_end_matches('/'), base_name);
        });
        Ok(())
    }

    async fn free_diskspace(&self, _hash: &str) -> Result<Option<i64>> {
        // Fixtures live in no real filesystem; report a roomy 500 GiB so the
        // preflight path is exercisable in mock mode.
        Ok(Some(500 * 1024 * 1024 * 1024))
    }

    async fn set_priority(&self, hash: &str, priority: i64) -> Result<()> {
        self.with_hash(&[hash.to_string()], |t| t.priority = priority);
        Ok(())
    }

    async fn set_file_priority(&self, hash: &str, index: usize, priority: i64) -> Result<()> {
        self.state
            .lock()
            .unwrap()
            .file_priorities
            .insert((hash.to_ascii_uppercase(), index), priority.clamp(0, 2));
        Ok(())
    }

    async fn set_connection_limits(
        &self,
        hash: &str,
        peers_max: i64,
        peers_min: i64,
        uploads_max: i64,
    ) -> Result<()> {
        self.with_hash(&[hash.to_string()], |t| {
            t.peers_max = peers_max;
            t.peers_min = peers_min;
            t.uploads_max = uploads_max;
        });
        Ok(())
    }

    async fn set_super_seeding(&self, hash: &str, enabled: bool) -> Result<()> {
        self.with_hash(&[hash.to_string()], |t| {
            t.connection_current = if enabled { "initial_seed" } else { "seed" }.into();
        });
        Ok(())
    }

    async fn set_custom_metadata(&self, hash: &str, values: &[(&str, &str)]) -> Result<()> {
        self.with_hash(&[hash.to_string()], |t| {
            for (key, value) in values {
                match *key {
                    "added_by" => t.added_by = (*value).into(),
                    "source_path" => t.source_path = (*value).into(),
                    "added_at" => t.added_at = value.parse().unwrap_or(0),
                    crate::tags::CUSTOM_KEY => t.tags = crate::tags::parse(value),
                    crate::complete::FINAL_DIR_KEY => t.final_dir = (*value).into(),
                    crate::queue::FORCE_KEY => t.force_start = value.trim() == "1",
                    crate::queue::POS_KEY => t.queue_pos = crate::queue::parse_pos(value),
                    crate::bandwidth::RULE_KEY => t.throttle_rule = (*value).into(),
                    _ => {}
                }
            }
        });
        Ok(())
    }

    async fn base_path(&self, hash: &str) -> Result<String> {
        let state = self.state.lock().unwrap();
        Ok(state
            .torrents
            .iter()
            .find(|t| t.hash.eq_ignore_ascii_case(hash))
            .map(|t| t.base_path.clone())
            .unwrap_or_default())
    }

    async fn define_named_throttle(&self, name: &str, down_kb: i64, up_kb: i64) -> Result<()> {
        self.state
            .lock()
            .unwrap()
            .throttles
            .insert(name.to_string(), (down_kb, up_kb));
        Ok(())
    }

    async fn assign_throttle(&self, hashes: &[String], name: Option<&str>) -> Result<()> {
        let name = name.unwrap_or("").to_string();
        self.with_hash(hashes, |torrent| torrent.throttle_name.clone_from(&name));
        Ok(())
    }

    async fn torrent_throttle_name(&self, hash: &str) -> Result<String> {
        let state = self.state.lock().unwrap();
        Ok(state
            .torrents
            .iter()
            .find(|torrent| torrent.hash.eq_ignore_ascii_case(hash))
            .map(|torrent| torrent.throttle_name.clone())
            .unwrap_or_default())
    }

    async fn set_throttles(&self, _down_kb: i64, _up_kb: i64) -> Result<()> {
        Ok(())
    }

    async fn set_port_range(&self, _range: &str) -> Result<()> {
        Ok(())
    }

    async fn apply_config(&self, directives: &[(&str, i64)]) -> Result<usize> {
        // The fixture daemon accepts every directive.
        Ok(directives.len())
    }

    async fn apply_config_str(&self, directives: &[(&str, &str)]) -> Result<usize> {
        Ok(directives.len())
    }

    async fn config_get(&self, keys: &[&str]) -> Result<Vec<Option<String>>> {
        let overrides = self.config.lock().unwrap();
        let defaults = default_config();
        Ok(keys
            .iter()
            .map(|key| {
                overrides
                    .get(*key)
                    .cloned()
                    .or_else(|| defaults.get(*key).cloned())
            })
            .collect())
    }

    async fn config_set(&self, key: &str, value: &str) -> Result<()> {
        // A key the fixture daemon does not expose is a fault on a real build;
        // mirror that so the settings surface's unavailable path is exercisable.
        if !default_config().contains_key(key) {
            return Err(super::RtorrentError::Fault {
                code: 1,
                message: format!("unknown method: {key}.set"),
            });
        }
        self.config
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    async fn views(&self) -> Result<Vec<(String, Vec<String>)>> {
        // Synthesize a couple of daemon-style membership views from the fixtures.
        let state = self.state.lock().unwrap();
        let pick = |f: fn(&RawTorrent) -> bool| -> Vec<String> {
            state
                .torrents
                .iter()
                .filter(|t| f(t))
                .map(|t| t.hash.clone())
                .collect()
        };
        Ok(vec![
            ("seeding".to_string(), pick(|t| t.complete && t.is_active)),
            ("leeching".to_string(), pick(|t| !t.complete && t.is_active)),
        ])
    }

    async fn daemon_health(&self) -> Result<crate::types::DaemonHealth> {
        Ok(crate::types::DaemonHealth {
            client_version: "0.9.8".into(),
            api_version: "11".into(),
            session_path: "/home/you/.rtorrent/session".into(),
            memory_max: (4.0 * GIB) as i64,
            memory_current: (128.0 * MIB) as i64,
            open_sockets: 214,
            max_open_sockets: 3000,
            max_open_files: 1024,
            http_max_open: 128,
        })
    }

    async fn save_session(&self) -> Result<()> {
        Ok(())
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn set_dht(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    async fn statistics(&self) -> Result<super::RawStats> {
        let state = self.state.lock().unwrap();
        let connected_peers = state.torrents.iter().map(|t| t.peers_connected).sum();
        // Values chosen to match the design's Statistics screen (05).
        Ok(super::RawStats {
            session_down: (1.6 * GIB) as i64,
            session_up: (312.0 * MIB) as i64,
            connected_peers,
            session_waste: (184.0 * MIB) as i64,
            buffer_size: Some((128.0 * MIB) as i64),
            cache_hit_pct: Some(96.4),
            cache_overload_pct: Some(0.0),
            queued_io: Some(3),
        })
    }
}

fn effective_rates(
    torrent: &RawTorrent,
    natural_rates: &HashMap<String, (i64, i64)>,
    throttles: &HashMap<String, (i64, i64)>,
) -> (i64, i64) {
    if !torrent.is_active {
        return (0, 0);
    }
    let natural = natural_rates
        .get(&torrent.hash)
        .copied()
        .unwrap_or((torrent.down_rate, torrent.up_rate));
    let Some((down_kb, up_kb)) = throttles.get(&torrent.throttle_name).copied() else {
        return natural;
    };
    (cap_rate(natural.0, down_kb), cap_rate(natural.1, up_kb))
}

fn cap_rate(natural: i64, limit_kb: i64) -> i64 {
    if limit_kb == 0 {
        natural
    } else {
        natural.min(limit_kb.saturating_mul(1024))
    }
}

/// The host each fixture row announces to, matching the design's Tracker
/// column. Newly added torrents (any other hash) get a generic host.
///
/// Returned as a full announce URL; the poller derives the host from it, which
/// is what the table's Tracker column and the sidebar's Tracker group show.
fn tracker_host_for(hash: &str) -> &'static str {
    match hash {
        "D01" => "https://bittorrent.debian.org/announce",
        "D02" => "https://torrent.ubuntu.com/announce",
        "D03" => "https://tracker.archlinux.org/announce",
        "D04" | "D15" => "https://linuxtracker.org/announce",
        "D05" => "https://torrent.fedoraproject.org/announce",
        "D06" | "D08" => "https://academictorrents.com/announce",
        "D07" => "https://opensuse.org/announce",
        "D09" => "https://alpinelinux.org/announce",
        "D10" => "https://proxmox.com/announce",
        "D11" => "https://kiwix.org/announce",
        "D12" => "https://manjaro.org/announce",
        "D13" => "https://centos.org/announce",
        "D14" => "https://download.blender.org/announce",
        _ => "https://linuxtracker.org/announce",
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn tracker_host(url: &str) -> String {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    after_scheme
        .split(['/', ':'])
        .next()
        .unwrap_or(after_scheme)
        .to_string()
}

fn mock_tracker(url: &str) -> MockTracker {
    MockTracker {
        url: url.into(),
        enabled: true,
        seeds: 34,
        leeches: 12,
        last_announce: unix_now() - 120, // ~2 min ago
    }
}

/// Construct a freshly-added downloading torrent for load_* calls.
fn new_download(name: &str, opts: &LoadOptions) -> RawTorrent {
    RawTorrent {
        hash: format!("{:016X}", fxhash(name)),
        name: name.to_string(),
        size_bytes: (1.5 * GIB) as i64,
        bytes_done: 0,
        complete: false,
        is_active: opts.start,
        is_open: opts.start,
        down_rate: if opts.start { (3.0 * MIB) as i64 } else { 0 },
        up_rate: 0,
        ratio_permille: 0,
        label: opts.label.clone(),
        directory: opts.directory.clone(),
        base_path: format!("{}/{}", opts.directory, name),
        peers_complete: 20,
        peers_accounted: 8,
        peers_connected: 6,
        priority: if opts.top_of_queue { 3 } else { 2 },
        ..Default::default()
    }
}

/// Pack piece bits into rtorrent's `d.bitfield` hex layout: each byte holds 8
/// pieces, most-significant bit first, rendered as two uppercase hex chars.
fn bits_to_hex(bits: &[bool]) -> String {
    let mut out = String::with_capacity(bits.len().div_ceil(4));
    for byte_start in (0..bits.len()).step_by(8) {
        let mut byte: u8 = 0;
        for offset in 0..8 {
            if bits.get(byte_start + offset).copied().unwrap_or(false) {
                byte |= 0x80 >> offset;
            }
        }
        out.push_str(&format!("{byte:02X}"));
    }
    out
}

/// Uppercase hex encoding of a byte buffer — mirrors how rtorrent's
/// `d.chunks_seen` ships its per-chunk peer counts (one byte per chunk).
fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02X}"));
    }
    out
}

/// Tiny stable hash so added torrents get a deterministic pseudo info-hash.
fn fxhash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// One fixture row, written in the design's own vocabulary.
///
/// Positional construction was unreadable at this size — the design's sample
/// block is fourteen rows across fifteen fields — so each row is a named-field
/// literal that maps onto [`RawTorrent`]'s raw daemon fields in `to_raw`.
///
/// Sizes are the design's figures in **binary** units (3.74 GB → 3.74 GiB):
/// rstorrent formats binary, so this keeps the rendered column the same
/// magnitude as the design's.
struct Row {
    hash: &'static str,
    name: &'static str,
    size: f64,
    /// Percentage complete; exactly 100.0 marks a seeding row.
    done: f64,
    /// rtorrent's `d.is_open`: whether the torrent is loaded and may hold peer
    /// connections. A stopped row is closed; a *queued* row is open but idle,
    /// which is the distinction the design draws between Stopped and Queued.
    open: bool,
    /// rtorrent's `d.is_active`: whether it is actually transferring.
    active: bool,
    down: i64,
    up: i64,
    /// Share ratio as displayed (rtorrent stores per-mille).
    ratio: f64,
    label: &'static str,
    /// Seeds in the swarm; drives the DTO's `seeds_connected`.
    swarm_seeds: i64,
    /// Peers in the swarm.
    swarm_peers: i64,
    /// Peers this client is connected to.
    connected: i64,
    /// Non-empty means the daemon reported a tracker/storage problem.
    message: &'static str,
    /// Hours before now that the row was added, for the Added column.
    added_hours_ago: f64,
    /// `Some(percent)` while the row is re-verifying its data.
    checking: Option<f64>,
}

impl Row {
    fn to_raw(&self) -> RawTorrent {
        let size_bytes = self.size as i64;
        let complete = self.done >= 100.0;
        // A plausible chunk count (~1 MiB pieces); when idle, chunks_hashed
        // tracks completed chunks, matching a daemon that isn't mid-check.
        let size_chunks = (size_bytes / (1024 * 1024)).max(1);
        let chunks_hashed = match self.checking {
            Some(percent) => (size_chunks as f64 * percent / 100.0) as i64,
            None => (size_chunks as f64 * self.done / 100.0) as i64,
        };
        RawTorrent {
            hash: self.hash.to_string(),
            name: self.name.to_string(),
            size_bytes,
            bytes_done: (self.size * self.done / 100.0) as i64,
            complete,
            is_active: self.active,
            is_open: self.open,
            hashing: self.checking.is_some(),
            message: self.message.to_string(),
            down_rate: self.down,
            up_rate: self.up,
            ratio_permille: (self.ratio * 1000.0) as i64,
            label: self.label.to_string(),
            directory: "/mnt/data/downloads".to_string(),
            base_path: format!("/mnt/data/downloads/{}", self.name),
            peers_complete: self.swarm_seeds,
            peers_accounted: self.swarm_peers,
            peers_connected: self.connected,
            priority: 2,
            is_private: false,
            throttle_name: String::new(),
            finished_at: if complete { unix_now() - 3 * 3600 } else { 0 },
            chunks_hashed,
            size_chunks,
            started_at: unix_now() - if complete { 26 * 3600 } else { 5 * 3600 },
            added_at: unix_now() - (self.added_hours_ago * 3600.0) as i64,
            ..Default::default()
        }
    }
}

/// The design's sample rows, reproduced in order so the table matches frame 1a.
///
/// These are the fourteen torrents in the handoff's `renderVals()` data block,
/// with its exact statuses, labels, ratios and seeds/peers figures. Sizes are
/// that block's numbers in binary units (see [`Row`]).
///
/// Rate figures are the design's displayed rates; they drive the mock's
/// simulated progress, so a downloading row really does advance.
fn design_rows() -> Vec<Row> {
    vec![
        Row {
            hash: "D01",
            name: "debian-13.1.0-amd64-DVD-1.iso",
            size: 3.74 * GIB,
            done: 61.4,
            open: true,
            active: true,
            down: (12.4 * MIB) as i64,
            up: (1.1 * MIB) as i64,
            ratio: 0.18,
            label: "iso",
            swarm_seeds: 38,
            swarm_peers: 240,
            connected: 112,
            message: "",
            added_hours_ago: 0.4,
            checking: None,
        },
        Row {
            hash: "D02",
            name: "ubuntu-26.04-desktop-amd64.iso",
            size: 5.12 * GIB,
            done: 24.8,
            open: true,
            active: true,
            down: (18.9 * MIB) as i64,
            up: (740.0 * KIB) as i64,
            ratio: 0.06,
            label: "iso",
            swarm_seeds: 51,
            swarm_peers: 260,
            connected: 204,
            message: "",
            added_hours_ago: 0.9,
            checking: None,
        },
        Row {
            hash: "D03",
            name: "archlinux-2026.09.01-x86_64.iso",
            size: 1.28 * GIB,
            done: 100.0,
            open: true,
            active: true,
            down: 0,
            up: (6.2 * MIB) as i64,
            ratio: 8.41,
            label: "iso",
            swarm_seeds: 12,
            swarm_peers: 60,
            connected: 44,
            message: "",
            added_hours_ago: 16.0,
            checking: None,
        },
        Row {
            hash: "D04",
            name: "linux-6.14.9.tar.xz",
            size: 148.0 * MIB,
            done: 100.0,
            open: true,
            active: true,
            down: 0,
            up: (1.8 * MIB) as i64,
            ratio: 24.60,
            label: "kernel",
            swarm_seeds: 9,
            swarm_peers: 40,
            connected: 9,
            message: "",
            added_hours_ago: 18.0,
            checking: None,
        },
        Row {
            hash: "D05",
            name: "fedora-Workstation-Live-44-1.4.iso",
            size: 2.41 * GIB,
            done: 88.2,
            open: true,
            active: true,
            down: (6.1 * MIB) as i64,
            up: (420.0 * KIB) as i64,
            ratio: 0.31,
            label: "iso",
            swarm_seeds: 22,
            swarm_peers: 120,
            connected: 61,
            message: "",
            added_hours_ago: 0.2,
            checking: None,
        },
        Row {
            hash: "D06",
            name: "NASA-Apollo-17-scans-4K",
            size: 212.0 * GIB,
            done: 3.1,
            open: true,
            active: true,
            down: (1.2 * MIB) as i64,
            up: 0,
            ratio: 0.00,
            label: "archive",
            swarm_seeds: 4,
            swarm_peers: 24,
            connected: 7,
            message: "",
            added_hours_ago: 15.0,
            checking: None,
        },
        Row {
            hash: "D07",
            name: "openSUSE-Tumbleweed-DVD-x86_64.iso",
            size: 4.60 * GIB,
            done: 100.0,
            open: true,
            active: true,
            down: 0,
            up: (2.9 * MIB) as i64,
            ratio: 3.02,
            label: "iso",
            swarm_seeds: 18,
            swarm_peers: 55,
            connected: 18,
            message: "",
            added_hours_ago: 22.0,
            checking: None,
        },
        Row {
            hash: "D08",
            name: "gutenberg-mirror-2026-08.tar",
            size: 44.8 * GIB,
            done: 100.0,
            open: true,
            active: true,
            down: 0,
            up: (310.0 * KIB) as i64,
            ratio: 1.14,
            label: "archive",
            swarm_seeds: 3,
            swarm_peers: 12,
            connected: 3,
            message: "",
            added_hours_ago: 27.0,
            checking: None,
        },
        Row {
            hash: "D09",
            name: "alpine-standard-3.24.0-x86_64.iso",
            size: 224.0 * MIB,
            done: 100.0,
            open: false,
            active: false,
            down: 0,
            up: 0,
            ratio: 5.77,
            label: "iso",
            swarm_seeds: 0,
            swarm_peers: 0,
            connected: 0,
            message: "",
            added_hours_ago: 32.0,
            checking: None,
        },
        Row {
            hash: "D10",
            name: "proxmox-ve_9.1-1.iso",
            size: 1.42 * GIB,
            done: 47.0,
            // Queued, not stopped: open and holding peers, but not transferring.
            // This is the row that tells the two apart, and the design shows it
            // connected to the swarm's 26 peers.
            open: true,
            active: false,
            down: 0,
            up: 0,
            ratio: 0.00,
            label: "iso",
            swarm_seeds: 0,
            swarm_peers: 26,
            connected: 26,
            message: "",
            added_hours_ago: 1.5,
            checking: None,
        },
        Row {
            hash: "D11",
            name: "wikipedia-en-all-2026-07.zim",
            size: 108.0 * GIB,
            done: 100.0,
            open: true,
            active: true,
            down: 0,
            up: (4.4 * MIB) as i64,
            ratio: 2.08,
            label: "archive",
            swarm_seeds: 12,
            swarm_peers: 45,
            connected: 12,
            message: "",
            added_hours_ago: 46.0,
            checking: None,
        },
        Row {
            hash: "D12",
            name: "manjaro-kde-26.0-260815.iso",
            size: 3.90 * GIB,
            done: 12.6,
            open: false,
            active: false,
            down: 0,
            up: 0,
            ratio: 0.00,
            label: "iso",
            swarm_seeds: 0,
            swarm_peers: 0,
            connected: 0,
            message: "Tracker: [Failure reason \"unregistered torrent\"]",
            added_hours_ago: 3.0,
            checking: None,
        },
        Row {
            hash: "D13",
            name: "centos-stream-11-x86_64-dvd1.iso",
            size: 9.10 * GIB,
            done: 100.0,
            open: true,
            active: true,
            down: 0,
            up: (1.2 * MIB) as i64,
            ratio: 0.94,
            label: "iso",
            swarm_seeds: 7,
            swarm_peers: 30,
            connected: 7,
            message: "",
            added_hours_ago: 55.0,
            checking: None,
        },
        Row {
            hash: "D14",
            name: "blender-4.9-linux-x64.tar.xz",
            size: 382.0 * MIB,
            done: 100.0,
            open: false,
            active: false,
            down: 0,
            up: 0,
            ratio: 0.00,
            label: "apps",
            swarm_seeds: 0,
            swarm_peers: 0,
            connected: 0,
            message: "",
            added_hours_ago: 66.0,
            checking: Some(42.0),
        },
        // The design's table block has no `media` row, but its sidebar lists the
        // label, and rstorrent derives sidebar counts from the rows — so one row
        // carries it. It doubles as the only `Stalled` state (active, incomplete,
        // no rate), which the design's vocabulary also lacks but the UI renders.
        Row {
            hash: "D15",
            name: "Cinema.Pack.2026.2160p.mkv",
            size: 18.4 * GIB,
            done: 33.3,
            open: true,
            active: true,
            down: 0,
            up: 0,
            ratio: 0.00,
            label: "media",
            swarm_seeds: 0,
            swarm_peers: 4,
            connected: 0,
            message: "",
            added_hours_ago: 9.0,
            checking: None,
        },
    ]
}

/// The fixture set the mock daemon serves.
///
/// The tracker host comes from [`tracker_host_for`]; one row is private so the
/// C7 affordances (no Copy-magnet) have something to act on.
fn fixtures() -> Vec<RawTorrent> {
    let mut rows: Vec<RawTorrent> = design_rows().iter().map(Row::to_raw).collect();
    // `D12`'s "unregistered torrent" failure is a private-tracker scenario.
    if let Some(row) = rows.iter_mut().find(|row| row.hash == "D12") {
        row.is_private = true;
    }
    rows
}

/// The fixture daemon's config values, keyed by the variable name a settings
/// surface reads. A key absent here is one this "build" does not expose, so
/// `config_get` returns `None` and the UI must show it as unavailable.
fn default_config() -> HashMap<String, String> {
    HashMap::from([
        ("network.port_range".to_string(), "6881-6899".to_string()),
        ("network.max_open_files".to_string(), "1024".to_string()),
        ("network.max_open_sockets".to_string(), "3000".to_string()),
        ("network.http.max_open".to_string(), "128".to_string()),
        ("network.http.proxy_address".to_string(), String::new()),
        ("network.bind_address".to_string(), String::new()),
        ("network.local_address".to_string(), String::new()),
        (
            "session.path".to_string(),
            "/home/you/.rtorrent/session".to_string(),
        ),
        ("dht.mode".to_string(), "auto".to_string()),
        ("protocol.pex".to_string(), "1".to_string()),
        (
            "protocol.encryption".to_string(),
            "allow_incoming,try_outgoing".to_string(),
        ),
        ("throttle.global_down.max_rate".to_string(), "0".to_string()),
        ("throttle.global_up.max_rate".to_string(), "0".to_string()),
        ("throttle.max_uploads.global".to_string(), "0".to_string()),
        ("throttle.max_downloads.global".to_string(), "0".to_string()),
        ("throttle.max_peers.normal".to_string(), "0".to_string()),
        ("throttle.max_peers.seed".to_string(), "0".to_string()),
        (
            "pieces.memory.max".to_string(),
            ((4.0 * GIB) as i64).to_string(),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rtorrent::derive;
    use crate::types::Status;

    /// The exact bytes `e2e/fixtures/test.torrent` holds: one 1024-byte file,
    /// 20 bytes of non-UTF-8 pieces (lava_torrent requires a byte string).
    fn minimal_torrent() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(
            b"d8:announce28:http://tracker.test/announce4:infod6:lengthi1024e4:name8:test.bin12:piece lengthi16384e6:pieces20:",
        );
        bytes.extend_from_slice(&[0xff; 20]);
        bytes.extend_from_slice(b"ee");
        bytes
    }

    #[tokio::test]
    async fn load_raw_uses_the_torrents_own_metadata() {
        let c = MockClient::new();
        c.load_raw(
            minimal_torrent(),
            LoadOptions {
                directory: "/data".into(),
                label: String::new(),
                start: false,
                top_of_queue: false,
                unselected_indexes: vec![],
            },
        )
        .await
        .unwrap();
        let rows = c.list_snapshot().await.unwrap();
        let added = rows
            .iter()
            .find(|torrent| torrent.name == "test.bin")
            .expect("the added torrent keeps its own name");
        assert_eq!(added.size_bytes, 1024);
        assert_eq!(added.hash, "F03264193C402F29E54FBF5DF2F3A8E1F0EFFF7F");
        assert_eq!(added.base_path, "/data/test.bin");
    }

    #[tokio::test]
    async fn tags_round_trip_through_the_custom_namespace() {
        let c = MockClient::new();
        let hash = c.list_snapshot().await.unwrap()[0].hash.clone();

        c.set_tags(
            std::slice::from_ref(&hash),
            &[" Linux ".into(), "linux".into(), "iso".into()],
        )
        .await
        .unwrap();
        let row = c
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.hash == hash)
            .unwrap();
        assert_eq!(
            row.tags,
            vec!["Linux", "iso"],
            "normalised and de-duplicated"
        );

        // The keyed custom-metadata path parses the same wire form.
        c.set_custom_metadata(&hash, &[("tags", "a,b")])
            .await
            .unwrap();
        let row = c
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.hash == hash)
            .unwrap();
        assert_eq!(row.tags, vec!["a", "b"]);

        // An empty list clears them.
        c.set_tags(std::slice::from_ref(&hash), &[]).await.unwrap();
        let row = c
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.hash == hash)
            .unwrap();
        assert!(row.tags.is_empty());
    }

    #[tokio::test]
    async fn final_dir_round_trips_through_the_custom_namespace() {
        let c = MockClient::new();
        let hash = c.list_snapshot().await.unwrap()[0].hash.clone();

        c.set_custom_metadata(&hash, &[(crate::complete::FINAL_DIR_KEY, "/media/video")])
            .await
            .unwrap();
        let row = c
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.hash == hash)
            .unwrap();
        assert_eq!(row.final_dir, "/media/video");
    }

    #[tokio::test]
    async fn force_start_and_queue_pos_round_trip() {
        let c = MockClient::new();
        let hash = c.list_snapshot().await.unwrap()[0].hash.clone();

        let row = c
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.hash == hash)
            .unwrap();
        assert!(!row.force_start);
        assert_eq!(row.queue_pos, None);

        c.set_custom_metadata(
            &hash,
            &[
                (crate::queue::FORCE_KEY, "1"),
                (crate::queue::POS_KEY, "  -4 "),
            ],
        )
        .await
        .unwrap();
        let row = c
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.hash == hash)
            .unwrap();
        assert!(row.force_start);
        assert_eq!(row.queue_pos, Some(-4));

        // Clearing writes back to unset, not to zero.
        c.set_custom_metadata(
            &hash,
            &[(crate::queue::FORCE_KEY, ""), (crate::queue::POS_KEY, "")],
        )
        .await
        .unwrap();
        let row = c
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.hash == hash)
            .unwrap();
        assert!(!row.force_start);
        assert_eq!(row.queue_pos, None);
    }

    #[tokio::test]
    async fn throttle_rule_marker_round_trips() {
        let c = MockClient::new();
        let hash = c.list_snapshot().await.unwrap()[0].hash.clone();
        c.set_custom_metadata(&hash, &[(crate::bandwidth::RULE_KEY, "vid")])
            .await
            .unwrap();
        let row = c
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.hash == hash)
            .unwrap();
        assert_eq!(row.throttle_rule, "vid");
    }

    #[tokio::test]
    async fn config_read_fills_known_keys_and_reports_unknown_ones_absent() {
        let c = MockClient::new();
        let values = c
            .config_get(&["network.port_range", "dht.mode", "no.such.key"])
            .await
            .unwrap();
        assert_eq!(values[0].as_deref(), Some("6881-6899"));
        assert_eq!(values[1].as_deref(), Some("auto"));
        assert_eq!(
            values[2], None,
            "a key the build lacks reads as unavailable"
        );

        // A write is visible to the next read, as a real daemon's would be.
        c.config_set("dht.mode", "disable").await.unwrap();
        assert_eq!(
            c.config_get(&["dht.mode"]).await.unwrap()[0].as_deref(),
            Some("disable")
        );
        // An unknown key faults, which the settings surface reports per key.
        assert!(c.config_set("no.such.key", "1").await.is_err());
    }

    #[tokio::test]
    async fn fixtures_reproduce_the_design_table() {
        // Every row of the handoff's data block, with the status rtorrent's raw
        // flags derive to. The design's own sidebar counts are illustrative and
        // do not reconcile with its fourteen rows, so the assertion is on the
        // row-derived truth:
        //   4 downloading   debian, ubuntu, fedora, NASA
        //   6 seeding       arch, linux, openSUSE, gutenberg, wikipedia, centos
        //   1 stopped       alpine   -> Paused
        //   1 queued        proxmox  -> Paused (incomplete and not running)
        //   1 tracker error manjaro
        //   1 checking      blender
        //   1 stalled       the added media row
        let c = MockClient::new();
        let rows = c.list_snapshot().await.unwrap();
        assert_eq!(rows.len(), FIXTURE_COUNT);
        let mut counts = std::collections::HashMap::new();
        for r in &rows {
            *counts.entry(derive::status(r)).or_insert(0) += 1;
        }
        assert_eq!(counts.get(&Status::Downloading), Some(&4));
        assert_eq!(counts.get(&Status::Seeding), Some(&6));
        assert_eq!(counts.get(&Status::Paused), Some(&2));
        assert_eq!(counts.get(&Status::Stalled), Some(&1));
        assert_eq!(counts.get(&Status::Error), Some(&1));
        assert_eq!(counts.get(&Status::Checking), Some(&1));

        // Two notions of "complete", and they differ for exactly one row.
        // `derive::percent` is byte completion: eight rows, including blender,
        // which is fully downloaded.
        let byte_complete: Vec<&str> = rows
            .iter()
            .filter(|r| derive::percent(r) >= 100.0)
            .map(|r| r.hash.as_str())
            .collect();
        assert_eq!(
            byte_complete,
            ["D03", "D04", "D07", "D08", "D09", "D11", "D13", "D14"]
        );
        // What the table *shows* comes from `to_dto`, where a rehashing row
        // reports its chunk sweep — so blender drops out of the complete set.
        let shown_complete: Vec<&str> = rows
            .iter()
            .filter(|r| derive::to_dto(r, "", None).percent >= 100.0)
            .map(|r| r.hash.as_str())
            .collect();
        assert_eq!(
            shown_complete,
            ["D03", "D04", "D07", "D08", "D09", "D11", "D13"]
        );

        // The design's sidebar lists five labels; every one has a row so the
        // sidebar's groups and counts are exercisable in mock mode.
        let labels: std::collections::HashSet<&str> =
            rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            ["iso", "kernel", "archive", "apps", "media"]
                .into_iter()
                .collect()
        );
    }

    #[tokio::test]
    async fn the_designs_seeds_and_peers_figures_survive_derivation() {
        // The merged Seeds/Peers column reads connected seeds, then connected
        // peers — "38 / 112" for the debian row. Connected seeds is the overlap
        // of the swarm's seeds with what this client is actually connected to.
        let c = MockClient::new();
        let rows = c.list_snapshot().await.unwrap();
        let debian = rows.iter().find(|row| row.hash == "D01").unwrap();
        let dto = derive::to_dto(debian, "bittorrent.debian.org", None);
        assert_eq!(dto.seeds_connected, 38);
        assert_eq!(dto.peers_connected, 112);
        assert_eq!(dto.seeds_swarm, 38);
        assert_eq!(dto.peers_swarm, 240);
    }

    #[tokio::test]
    async fn stopped_and_queued_differ_in_their_raw_flags() {
        // Both derive to `Paused`, but the design words them differently. Nine
        // is stopped (closed); ten is queued — loaded and holding the swarm's
        // peers while it waits its turn. Only the raw flags carry that today,
        // which is why the fixtures pin them.
        let c = MockClient::new();
        let rows = c.list_snapshot().await.unwrap();
        let alpine = rows.iter().find(|r| r.hash == "D09").unwrap();
        let proxmox = rows.iter().find(|r| r.hash == "D10").unwrap();
        assert!(!alpine.is_open && !alpine.is_active, "stopped is closed");
        assert!(proxmox.is_open && !proxmox.is_active, "queued is open+idle");
        assert_eq!(derive::status(alpine), Status::Paused);
        assert_eq!(derive::status(proxmox), Status::Paused);

        let dto = derive::to_dto(proxmox, "proxmox.com", None);
        assert_eq!((dto.seeds_connected, dto.peers_connected), (0, 26));
    }

    #[tokio::test]
    async fn a_checking_row_reports_chunk_progress_not_bytes() {
        // Blender is complete but mid-rehash: the design shows "Checking 42%",
        // which must come from the chunk sweep rather than byte completion.
        let c = MockClient::new();
        let rows = c.list_snapshot().await.unwrap();
        let blender = rows.iter().find(|row| row.hash == "D14").unwrap();
        assert!(blender.hashing);
        let dto = derive::to_dto(blender, "", None);
        assert_eq!(dto.status, Status::Checking);
        assert!((dto.percent - 42.0).abs() < 0.5, "got {}", dto.percent);
    }

    #[tokio::test]
    async fn the_error_row_is_private_and_classified() {
        // The design's "Tracker error" row carries an unregistered-torrent
        // failure, which is the C7 private-tracker scenario.
        let c = MockClient::new();
        let rows = c.list_snapshot().await.unwrap();
        let manjaro = rows.iter().find(|row| row.hash == "D12").unwrap();
        assert!(manjaro.is_private);
        let dto = derive::to_dto(manjaro, "manjaro.org", None);
        assert_eq!(dto.status, Status::Error);
        assert_eq!(dto.error_kind, "unregistered");
    }

    #[tokio::test]
    async fn stop_then_start_toggles_active() {
        let c = MockClient::new();
        c.stop(&["D05".into()]).await.unwrap();
        let rows = c.list_snapshot().await.unwrap();
        let fedora = rows.iter().find(|r| r.hash == "D05").unwrap();
        assert!(!fedora.is_active);
        assert_eq!(derive::status(fedora), Status::Paused);
    }

    #[tokio::test]
    async fn active_seed_ratios_grow() {
        // Ratio accrues from the upload rate and size, so the row must be a
        // seeding one — the design's first row is a downloader.
        let c = MockClient::new();
        let before = c
            .state
            .lock()
            .unwrap()
            .torrents
            .iter()
            .find(|row| row.hash == "D03")
            .unwrap()
            .ratio_permille;
        c.state.lock().unwrap().last_tick = Instant::now() - std::time::Duration::from_secs(1);

        let rows = c.list_snapshot().await.unwrap();
        let arch = rows.iter().find(|row| row.hash == "D03").unwrap();
        assert!(arch.ratio_permille > before);
    }

    #[tokio::test]
    async fn erase_removes_torrent() {
        let c = MockClient::new();
        c.erase(&["D01".into()]).await.unwrap();
        assert_eq!(c.list_snapshot().await.unwrap().len(), FIXTURE_COUNT - 1);
    }

    #[tokio::test]
    async fn tracker_management_updates_mock_detail_rows() {
        let c = MockClient::new();
        let url = "udp://tracker.example.test:6969/announce";

        c.add_tracker("D05", url).await.unwrap();
        let rows = c.trackers("D05").await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].url, url);
        assert!(rows[1].enabled);

        c.set_tracker_enabled("D05", 1, false).await.unwrap();
        let rows = c.trackers("D05").await.unwrap();
        assert!(!rows[1].enabled);
        assert_eq!(rows[1].status, "disabled");

        c.set_tracker_enabled("D05", 1, true).await.unwrap();
        let before = unix_now();
        c.force_reannounce(&["D05".into()]).await.unwrap();
        // Reannounce stamps the last-announce time to ~now.
        assert!(c.trackers("D05").await.unwrap()[1].last_announce >= before);

        c.remove_tracker("D05", 1).await.unwrap();
        assert!(!c.trackers("D05").await.unwrap()[1].enabled);
    }

    #[tokio::test]
    async fn named_throttle_assignments_cap_simulated_rates() {
        let c = MockClient::new();
        c.define_named_throttle("rstorrent_1", 512, 100)
            .await
            .unwrap();
        c.assign_throttle(&["D05".into()], Some("rstorrent_1"))
            .await
            .unwrap();

        let before = {
            let mut state = c.state.lock().unwrap();
            state.last_tick = Instant::now() - std::time::Duration::from_secs(2);
            state
                .torrents
                .iter()
                .find(|row| row.hash == "D05")
                .unwrap()
                .bytes_done
        };

        let rows = c.list_snapshot().await.unwrap();
        let fedora = rows.iter().find(|row| row.hash == "D05").unwrap();
        assert_eq!(fedora.down_rate, 512 * 1024);
        assert_eq!(fedora.up_rate, 100 * 1024);
        let progressed = fedora.bytes_done - before;
        assert!(progressed >= 2 * 512 * 1024);
        assert!(progressed < 3 * 512 * 1024);
        assert_eq!(c.torrent_throttle_name("D05").await.unwrap(), "rstorrent_1");

        c.assign_throttle(&["D05".into()], None).await.unwrap();
        let rows = c.list_snapshot().await.unwrap();
        let fedora = rows.iter().find(|row| row.hash == "D05").unwrap();
        // Clearing the throttle restores the row's natural rate (the design's
        // 6.1 MB/s for fedora).
        assert_eq!(fedora.down_rate, (6.1 * MIB) as i64);
        assert_eq!(fedora.throttle_name, "");
    }

    #[test]
    fn zero_direction_is_unlimited_in_mock_throttle() {
        assert_eq!(cap_rate(900_000, 0), 900_000);
        assert_eq!(cap_rate(900_000, 512), 512 * 1024);
    }
}
