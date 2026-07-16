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
use std::time::Instant;

use async_trait::async_trait;

use super::{LoadOptions, RawGlobal, RawTorrent, Result, RtorrentApi};
use crate::ipc::{FileNode, PeerRow, TrackerRow};

const GIB: f64 = 1_073_741_824.0;
const MIB: f64 = 1_048_576.0;
const KIB: f64 = 1_024.0;

/// Mutable fixture state guarded by a mutex (locks are held only for the brief,
/// non-awaiting critical sections that read or mutate the torrent list).
struct State {
    torrents: Vec<RawTorrent>,
    trackers: HashMap<String, Vec<MockTracker>>,
    natural_rates: HashMap<String, (i64, i64)>,
    throttles: HashMap<String, (i64, i64)>,
    last_tick: Instant,
}

#[derive(Clone)]
struct MockTracker {
    url: String,
    enabled: bool,
    seeds: i64,
    leeches: i64,
    last_announce: String,
}

impl MockTracker {
    fn row(&self, index: usize) -> TrackerRow {
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
            last_announce: self.last_announce.clone(),
        }
    }
}

pub struct MockClient {
    state: Mutex<State>,
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
                    vec![mock_tracker(tracker_url(&torrent.hash))],
                )
            })
            .collect();
        let natural_rates = torrents
            .iter()
            .map(|torrent| {
                (
                    torrent.hash.clone(),
                    (torrent.down_rate, torrent.up_rate),
                )
            })
            .collect();
        Self {
            state: Mutex::new(State {
                torrents,
                trackers,
                natural_rates,
                throttles: HashMap::new(),
                last_tick: Instant::now(),
            }),
        }
    }

    /// Advance simulated progress by the real time elapsed since the last call.
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
                }
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
                let rates = effective_rates(
                    &torrent,
                    &state.natural_rates,
                    &state.throttles,
                );
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
            .map(|torrent| {
                effective_rates(torrent, &state.natural_rates, &state.throttles)
            });
        let (down_rate, up_rate) = rates.fold((0, 0), |sum, rate| {
            (sum.0 + rate.0, sum.1 + rate.1)
        });
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
                    tracker.last_announce = "just now".into();
                }
            }
        }
        Ok(())
    }

    async fn peers(&self, _hash: &str) -> Result<Vec<PeerRow>> {
        Ok(vec![
            PeerRow {
                address: "203.0.113.7".into(),
                client: "libtorrent 2.0.9".into(),
                progress: 84.0,
                down_rate: (1.2 * MIB) as i64,
                up_rate: (120.0 * KIB) as i64,
                flags: "EI".into(),
            },
            PeerRow {
                address: "198.51.100.42".into(),
                client: "qBittorrent 4.6".into(),
                progress: 61.0,
                down_rate: (640.0 * KIB) as i64,
                up_rate: 0,
                flags: "E".into(),
            },
        ])
    }

    async fn files(&self, _hash: &str) -> Result<Vec<FileNode>> {
        Ok(vec![
            FileNode {
                path: "Fedora-Workstation-Live.iso".into(),
                size: (2.29 * GIB) as i64,
                priority: 1,
                progress: 67.0,
                is_dir: false,
            },
            FileNode {
                path: "CHECKSUM".into(),
                size: 1400,
                priority: 1,
                progress: 100.0,
                is_dir: false,
            },
        ])
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

    async fn load_raw(&self, _bytes: Vec<u8>, opts: LoadOptions) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let torrent = new_download("added-from-file.iso", &opts);
        state.trackers.insert(
            torrent.hash.clone(),
            vec![mock_tracker(tracker_url(&torrent.hash))],
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
            vec![mock_tracker(tracker_url(&torrent.hash))],
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

    async fn set_directory(&self, hash: &str, path: &str) -> Result<()> {
        self.with_hash(&[hash.to_string()], |t| t.directory = path.to_string());
        Ok(())
    }

    async fn set_priority(&self, hash: &str, priority: i64) -> Result<()> {
        self.with_hash(&[hash.to_string()], |t| t.priority = priority);
        Ok(())
    }

    async fn set_file_priority(&self, _hash: &str, _index: usize, _priority: i64) -> Result<()> {
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
    (
        cap_rate(natural.0, down_kb),
        cap_rate(natural.1, up_kb),
    )
}

fn cap_rate(natural: i64, limit_kb: i64) -> i64 {
    if limit_kb == 0 {
        natural
    } else {
        natural.min(limit_kb.saturating_mul(1024))
    }
}

fn tracker_url(hash: &str) -> &'static str {
    match hash {
        "A1" => "https://torrent.ubuntu.com/announce",
        "B2" => "https://bttracker.debian.org/announce",
        "F6" | "G7" | "J10" => "https://tracker.blender.org/announce",
        "I9" => "https://downloads.raspberrypi.org/announce",
        _ => "https://linuxtracker.org/announce",
    }
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
        last_announce: "2m ago".into(),
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

/// Tiny stable hash so added torrents get a deterministic pseudo info-hash.
fn fxhash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Helper to build one fixture row concisely.
#[allow(clippy::too_many_arguments)]
fn t(
    hash: &str,
    name: &str,
    size: f64,
    done_pct: f64,
    active: bool,
    open: bool,
    down: i64,
    up: i64,
    ratio_permille: i64,
    label: &str,
    swarm_seeds: i64,
    swarm_peers: i64,
    conn: i64,
    message: &str,
) -> RawTorrent {
    let size_bytes = size as i64;
    RawTorrent {
        hash: hash.to_string(),
        name: name.to_string(),
        size_bytes,
        bytes_done: (size * done_pct / 100.0) as i64,
        complete: done_pct >= 100.0,
        is_active: active,
        is_open: open,
        hashing: false,
        message: message.to_string(),
        down_rate: down,
        up_rate: up,
        ratio_permille,
        label: label.to_string(),
        directory: "/srv/downloads".to_string(),
        base_path: format!("/srv/downloads/{name}"),
        peers_complete: swarm_seeds,
        peers_accounted: swarm_peers,
        peers_connected: conn,
        priority: 2,
        is_private: false,
        throttle_name: String::new(),
    }
}

/// The ten torrents shown in the design reference, with matching states.
fn fixtures() -> Vec<RawTorrent> {
    vec![
        t("A1", "ubuntu-24.04.2-desktop-amd64.iso", 5.8 * GIB, 100.0, true, true, 0, (1.2 * MIB) as i64, 2410, "linux-iso", 142, 87, 34, ""),
        t("B2", "debian-12.9.0-amd64-netinst.iso", 631.0 * MIB, 100.0, true, true, 0, (214.0 * KIB) as i64, 3870, "linux-iso", 98, 12, 10, ""),
        t("C3", "Fedora-Workstation-Live-x86_64-41-1.4.iso", 2.3 * GIB, 67.4, true, true, (8.4 * MIB) as i64, (620.0 * KIB) as i64, 190, "linux-iso", 34, 12, 30, ""),
        t("D4", "archlinux-2026.07.01-x86_64.iso", 1.1 * GIB, 23.1, true, true, (1.1 * MIB) as i64, (88.0 * KIB) as i64, 40, "linux-iso", 18, 6, 12, ""),
        t("E5", "linuxmint-22.1-cinnamon-64bit.iso", 2.8 * GIB, 45.2, false, false, 0, 0, 110, "linux-iso", 63, 28, 0, ""),
        t("F6", "Big.Buck.Bunny.2008.2160p.mkv", 7.9 * GIB, 100.0, true, true, 0, (980.0 * KIB) as i64, 5020, "video", 211, 140, 60, ""),
        t("G7", "Sintel.2010.2160p.mkv", 5.1 * GIB, 91.8, true, true, (2.9 * MIB) as i64, (410.0 * KIB) as i64, 440, "video", 26, 9, 20, ""),
        t("H8", "openSUSE-Tumbleweed-DVD-x86_64.iso", 4.4 * GIB, 12.0, true, true, 0, 0, 10, "linux-iso", 0, 2, 2, ""),
        t("I9", "raspios-bookworm-arm64-full.img.xz", 2.7 * GIB, 100.0, false, false, 0, 0, 1080, "sbc", 57, 16, 0, ""),
        t("J10", "Cosmos.Laundromat.2015.4K.mkv", 3.2 * GIB, 66.7, true, true, 0, 0, 310, "video", 0, 0, 0, "Tracker: [Failure reason \"unregistered torrent\"]"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::Status;
    use crate::rtorrent::derive;

    #[tokio::test]
    async fn fixtures_match_design_row_states() {
        // The fixtures reproduce the design's per-row data block exactly (its
        // Name/Done/Status columns). Note: the design's sidebar counts
        // (seeding 4, completed 5) are illustrative and do NOT reconcile with
        // its own 10 rows — see plan.md §5.4. We assert the authoritative,
        // row-derived truth instead: 3 downloading / 3 seeding / 2 paused /
        // 1 stalled / 1 error, and 4 rows at 100%.
        let c = MockClient::new();
        let rows = c.list_snapshot().await.unwrap();
        assert_eq!(rows.len(), 10);
        let mut counts = std::collections::HashMap::new();
        for r in &rows {
            *counts.entry(derive::status(r)).or_insert(0) += 1;
        }
        assert_eq!(counts.get(&Status::Downloading), Some(&3)); // Fedora, arch, Sintel
        assert_eq!(counts.get(&Status::Seeding), Some(&3)); // ubuntu, debian, BBB
        assert_eq!(counts.get(&Status::Paused), Some(&2)); // mint, raspios
        assert_eq!(counts.get(&Status::Stalled), Some(&1)); // openSUSE
        assert_eq!(counts.get(&Status::Error), Some(&1)); // Cosmos
        let complete = rows.iter().filter(|r| derive::percent(r) >= 100.0).count();
        assert_eq!(complete, 4); // ubuntu, debian, BBB, raspios
    }

    #[tokio::test]
    async fn stop_then_start_toggles_active() {
        let c = MockClient::new();
        c.stop(&["C3".into()]).await.unwrap();
        let rows = c.list_snapshot().await.unwrap();
        let fedora = rows.iter().find(|r| r.hash == "C3").unwrap();
        assert!(!fedora.is_active);
        assert_eq!(derive::status(fedora), Status::Paused);
    }

    #[tokio::test]
    async fn erase_removes_torrent() {
        let c = MockClient::new();
        c.erase(&["A1".into()]).await.unwrap();
        assert_eq!(c.list_snapshot().await.unwrap().len(), 9);
    }

    #[tokio::test]
    async fn tracker_management_updates_mock_detail_rows() {
        let c = MockClient::new();
        let url = "udp://tracker.example.test:6969/announce";

        c.add_tracker("C3", url).await.unwrap();
        let rows = c.trackers("C3").await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].url, url);
        assert!(rows[1].enabled);

        c.set_tracker_enabled("C3", 1, false).await.unwrap();
        let rows = c.trackers("C3").await.unwrap();
        assert!(!rows[1].enabled);
        assert_eq!(rows[1].status, "disabled");

        c.set_tracker_enabled("C3", 1, true).await.unwrap();
        c.force_reannounce(&["C3".into()]).await.unwrap();
        assert_eq!(c.trackers("C3").await.unwrap()[1].last_announce, "just now");

        c.remove_tracker("C3", 1).await.unwrap();
        assert!(!c.trackers("C3").await.unwrap()[1].enabled);
    }

    #[tokio::test]
    async fn named_throttle_assignments_cap_simulated_rates() {
        let c = MockClient::new();
        c.define_named_throttle("rstorrent_1", 512, 100)
            .await
            .unwrap();
        c.assign_throttle(&["C3".into()], Some("rstorrent_1"))
            .await
            .unwrap();

        let before = {
            let mut state = c.state.lock().unwrap();
            state.last_tick = Instant::now() - std::time::Duration::from_secs(2);
            state
                .torrents
                .iter()
                .find(|row| row.hash == "C3")
                .unwrap()
                .bytes_done
        };

        let rows = c.list_snapshot().await.unwrap();
        let fedora = rows.iter().find(|row| row.hash == "C3").unwrap();
        assert_eq!(fedora.down_rate, 512 * 1024);
        assert_eq!(fedora.up_rate, 100 * 1024);
        let progressed = fedora.bytes_done - before;
        assert!(progressed >= 2 * 512 * 1024);
        assert!(progressed < 3 * 512 * 1024);
        assert_eq!(c.torrent_throttle_name("C3").await.unwrap(), "rstorrent_1");

        c.assign_throttle(&["C3".into()], None).await.unwrap();
        let rows = c.list_snapshot().await.unwrap();
        let fedora = rows.iter().find(|row| row.hash == "C3").unwrap();
        assert_eq!(fedora.down_rate, (8.4 * MIB) as i64);
        assert_eq!(fedora.throttle_name, "");
    }

    #[test]
    fn zero_direction_is_unlimited_in_mock_throttle() {
        assert_eq!(cap_rate(900_000, 0), 900_000);
        assert_eq!(cap_rate(900_000, 512), 512 * 1024);
    }
}
