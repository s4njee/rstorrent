//! IPC contract — Rust mirror of `src/ipc/types.ts`.
//!
//! These structs are (de)serialized across the Tauri boundary. Every one uses
//! `rename_all = "camelCase"` so the JSON field names match the TypeScript
//! definitions exactly. Keep this file and `src/ipc/types.ts` in lock-step: a
//! change to one is a change to both.

use serde::{Deserialize, Serialize};

/// Lifecycle status of a torrent, derived from rtorrent's raw flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Downloading,
    Seeding,
    Completed,
    Paused,
    Stalled,
    Checking,
    Error,
}

/// One torrent row. Sizes are bytes, rates are bytes/second; the frontend does
/// all human formatting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentDto {
    pub hash: String,
    pub name: String,
    pub size: i64,
    pub bytes_done: i64,
    pub percent: f64,
    pub status: Status,
    pub status_msg: String,
    pub seeds_connected: i64,
    pub peers_connected: i64,
    pub seeds_swarm: i64,
    pub peers_swarm: i64,
    pub down_rate: i64,
    pub up_rate: i64,
    /// Seconds remaining; `None` renders as ∞ or — depending on status.
    pub eta_seconds: Option<i64>,
    pub ratio: f64,
    pub label: String,
    pub tracker_host: String,
    pub save_path: String,
    pub priority: i64,
    pub is_private: bool,
    /// App-owned named throttle assigned to this torrent, empty when it uses
    /// the global limits.
    pub throttle_name: String,
    /// Named-throttle limits in bytes/s. `None` means use the corresponding
    /// global limit; `Some(0)` is an unlimited direction within a named group.
    pub down_rate_limit: Option<i64>,
    pub up_rate_limit: Option<i64>,
    /// Unix seconds first started / finished; 0 when unknown. Drive the Started
    /// and Finished columns (D4). Durable across daemon restarts (unlike
    /// `d.load_date`), so no separate "added" field until D6's sticky metadata.
    pub started_at: i64,
    pub finished_at: i64,
}

/// Global counters for the status bar and General tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalStats {
    pub down_rate: i64,
    pub up_rate: i64,
    pub down_rate_limit: i64,
    pub up_rate_limit: i64,
    pub dht_nodes: i64,
    pub free_space: Option<i64>,
}

/// Connection lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnPhase {
    Connecting,
    Connected,
    Disconnected,
}

/// Connection state to the rtorrent daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnState {
    pub phase: ConnPhase,
    pub endpoint: String,
    pub daemon_version: Option<String>,
    pub error: Option<String>,
    pub retry_in_seconds: Option<i64>,
}

/// The full state pushed on every fast poll (`state://snapshot`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub torrents: Vec<TorrentDto>,
    pub globals: GlobalStats,
    pub connection: ConnState,
}

/// Which detail tab is being watched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DetailTab {
    General,
    Trackers,
    Peers,
    Content,
    Speed,
    Log,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerRow {
    /// Zero-based position in rtorrent's tracker list (`HASH:tINDEX`).
    pub index: usize,
    pub url: String,
    pub enabled: bool,
    pub status: String,
    pub seeds: i64,
    pub leeches: i64,
    /// Tracker protocol: "http" / "udp" / "dht" (empty if unknown).
    pub kind: String,
    /// Unix seconds of the next scheduled announce; 0 when unset. May be in the
    /// past for a tracker that's overdue/failing — the UI shows "—" then.
    pub next_announce: i64,
    /// Unix seconds of the last successful announce; 0 = never.
    pub last_announce: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerRow {
    pub address: String,
    pub client: String,
    pub progress: f64,
    pub down_rate: i64,
    pub up_rate: i64,
    pub flags: String,
}

/// A file or folder in a torrent (Content tab & Add-dialog file tree).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNode {
    pub path: String,
    pub size: i64,
    pub priority: i64,
    pub progress: f64,
    pub is_dir: bool,
}

/// Per-piece download state for the General tab's pieces bar.
///
/// `bitfield` is rtorrent's `d.bitfield` hex string: each byte (two hex chars)
/// covers 8 pieces, most-significant bit first. We forward it verbatim rather
/// than expanding to a bool array — a 100k-piece torrent would otherwise mean a
/// 100k-element JSON array every poll; the frontend downsamples it to the bar's
/// pixel width. An empty string means "nothing yet" (treated as all-zero).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PieceInfo {
    pub size_chunks: i64,
    pub completed_chunks: i64,
    pub chunk_size: i64,
    pub bitfield: String,
}

/// Payload of the `state://detail` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailPayload {
    pub hash: String,
    pub tab: DetailTab,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trackers: Option<Vec<TrackerRow>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peers: Option<Vec<PeerRow>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileNode>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pieces: Option<PieceInfo>,
}

/// Log severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// One entry in the app event log.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub time: i64,
    pub level: LogLevel,
    pub message: String,
    pub hash: Option<String>,
}

/// Metadata parsed from a .torrent file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentMeta {
    pub name: String,
    pub size: i64,
    pub info_hash: String,
    pub is_private: bool,
    pub files: Vec<FileNode>,
    pub trackers: Vec<String>,
}

/// Options for an add request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddOptions {
    pub save_path: String,
    pub label: String,
    pub start: bool,
    pub top_of_queue: bool,
    pub sequential: bool,
    pub skip_hash_check: bool,
    pub unselected_indexes: Vec<usize>,
}

/// Source of an add request; internally tagged by `kind` to match the TS union.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AddSource {
    File { path: String },
    Magnet { uri: String },
}

/// Connection transport; internally tagged by `kind` to match the TS union.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Transport {
    UnixSocket {
        path: String,
    },
    Tcp {
        host: String,
        port: u16,
    },
    /// XML-RPC over HTTP(S), e.g. an nginx/ruTorrent-fronted seedbox (B9).
    ///
    /// The password is deliberately absent: it lives in the macOS Keychain
    /// (see `secrets.rs`), because settings.json is plaintext on disk.
    Http {
        url: String,
        #[serde(default)]
        username: String,
    },
}

/// Ratio/time limits applied to a completed torrent. Zero disables a rule.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedGoal {
    pub stop_ratio: f64,
    pub seed_hours: f64,
}

/// A label-specific replacement for the global seed goal.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelSeedGoal {
    pub label: String,
    pub stop_ratio: f64,
    pub seed_hours: f64,
}

/// App settings shared with the Preferences UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub transport: Transport,
    pub poll_ms: u64,
    pub stall_window_s: u64,
    pub default_save_path: String,
    pub show_add_dialog: bool,
    pub confirm_on_remove: bool,
    pub down_limit_kb: i64,
    pub up_limit_kb: i64,
    /// Listen port range, e.g. "6881-6899" (BitTorrent prefs).
    #[serde(default = "default_port_range")]
    pub port_range: String,
    /// Whether the DHT is enabled.
    #[serde(default)]
    pub dht_enabled: bool,
    /// Directory auto-scanned for `.torrent` files to add (empty = disabled).
    #[serde(default)]
    pub watch_folder: String,
    /// Labels whose completed torrents should not produce a notification.
    #[serde(default)]
    pub completion_notification_excluded_labels: Vec<String>,
    /// Definitions for the small app-owned named-throttle pool. rtorrent does
    /// not persist definitions across daemon restarts, so the poller replays
    /// these whenever it connects.
    #[serde(default)]
    pub torrent_throttles: Vec<NamedThrottle>,
    /// Default seeding limits; both zero means no limit.
    #[serde(default)]
    pub global_seed_goal: SeedGoal,
    /// Per-label replacements for the global goal, including explicit no-limit rows.
    #[serde(default)]
    pub label_seed_goals: Vec<LabelSeedGoal>,
    pub mock: bool,
}

/// One app-owned rtorrent named-throttle definition. Rates are KiB/s and zero
/// means unlimited for that direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedThrottle {
    pub name: String,
    pub down_kb: i64,
    pub up_kb: i64,
}

fn default_port_range() -> String {
    "6881-6899".to_string()
}

/// Aggregate figures for the Statistics dialog.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Statistics {
    pub session_down: i64,
    pub session_up: i64,
    pub all_time_down: i64,
    pub all_time_up: i64,
    pub all_time_ratio: Option<f64>,
    pub session_waste: i64,
    pub connected_peers: i64,
    pub cache_hit_pct: Option<f64>,
    pub buffer_size: Option<i64>,
    pub cache_overload_pct: Option<f64>,
    pub queued_io: Option<i64>,
}
