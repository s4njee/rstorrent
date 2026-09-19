//! Shared IPC/DTO contract — the wire types both hosts serialize.
//!
//! These structs are the single source of truth for the JSON exchanged with the
//! frontend, whether over the Tauri event/command boundary (desktop app) or over
//! HTTP (the `rstorrent-web` server). Both hosts serializing *these same structs*
//! is what keeps the two transports byte-for-byte identical by construction.
//!
//! Every struct uses `rename_all = "camelCase"` so the JSON field names match the
//! TypeScript definitions in `src/ipc/types.ts` exactly. Keep this file and that
//! one in lock-step: a change to one is a change to both.
//!
//! App-only types (Settings, Statistics, RSS/automation config, …) live in the
//! desktop crate's `ipc` module, which re-exports everything here.

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
    /// Per-torrent connection caps; zero means the daemon default.
    #[serde(default)]
    pub peers_max: i64,
    #[serde(default)]
    pub peers_min: i64,
    #[serde(default)]
    pub uploads_max: i64,
    /// Current connection type, e.g. `seed` or `initial_seed` (D5).
    #[serde(default)]
    pub connection_type: String,
    /// Sticky app metadata written to rtorrent's `d.custom` namespace (D6).
    #[serde(default)]
    pub added_by: String,
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub added_at: i64,
    /// Client-managed tags (V3-10), stored in `d.custom=tags` so they follow the
    /// daemon session.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Force-start (V3-17 / QUE-01): exempt from the client queue scheduler.
    /// Never faked — set only from the daemon's `d.custom=force_start`.
    #[serde(default)]
    pub force_start: bool,
    /// Bandwidth-rule marker (V3-18 / QUE-04): which rule manages this
    /// torrent's throttle. Empty = none (a set throttle is manual).
    #[serde(default)]
    pub throttle_rule: String,
    /// Native rtorrent views this torrent belongs to (D12); filled by the
    /// poller from `view.list`. Empty until the first view refresh.
    #[serde(default)]
    pub views: Vec<String>,
    /// Classified error bucket from `d.message` (D19); empty when not in error.
    /// One of: unregistered, tracker_timeout, tracker_error, missing_files,
    /// no_space, permission, disk_error, other.
    #[serde(default)]
    pub error_kind: String,
    /// rtorrent's `d.is_open`: loaded and may hold peer connections.
    #[serde(default)]
    pub is_open: bool,
    /// rtorrent's `d.is_active`: actually transferring.
    ///
    /// [`Status`] alone cannot tell a *stopped* torrent (`is_open == false`)
    /// from a *queued* one (loaded but idle), which the console's status column
    /// words differently, so the raw flags travel with the DTO.
    #[serde(default)]
    pub is_active: bool,
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
    /// Free bytes on the default save-path volume, or `None` when unknown/remote.
    pub free_space: Option<i64>,
    /// Total bytes on that volume, or `None` when unknown/remote. Paired with
    /// [`Self::free_space`] it lets the web disk card draw a used-fraction bar;
    /// the desktop status bar ignores it (WE0-S2).
    #[serde(default)]
    pub disk_size: Option<i64>,
    /// Whether turtle mode is currently in effect (manual toggle or an active
    /// schedule window) (B14).
    pub turtle_active: bool,
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

/// The full state pushed on every fast poll (`state://snapshot` / `GET /api/state`).
///
/// `revision` is a monotonic counter scoped to the poller process. Clients keep
/// the last revision they applied and can ask for a delta since that revision;
/// a gap triggers a full re-sync (FND-02).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(default)]
    pub revision: u64,
    pub torrents: Vec<TorrentDto>,
    pub globals: GlobalStats,
    pub connection: ConnState,
}

/// Incremental update between two [`Snapshot`]s. One contract for both Tauri
/// events (`state://delta`) and HTTP (`GET /api/delta?since=`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDelta {
    pub revision: u64,
    pub base_revision: u64,
    pub added: Vec<TorrentDto>,
    pub updated: Vec<TorrentDto>,
    pub removed: Vec<String>,
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
    /// rtorrent peer id (hex), used to target `p.*` actions (`HASH:p<id>`).
    /// Not shown; empty on the mock backend.
    pub id: String,
    pub address: String,
    pub client: String,
    pub progress: f64,
    pub down_rate: i64,
    pub up_rate: i64,
    /// Compact flag string (C16): E encrypted · I incoming · O obfuscated ·
    /// P preferred · U unwanted.
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
///
/// `availability` is rtorrent's `d.chunks_seen`, forwarded verbatim as a hex
/// string: one byte (two hex chars) per chunk holding the count of connected,
/// accounted peers that have that chunk — the swarm's per-piece coverage that
/// drives the availability bar. `None` when the daemon reports nothing (a
/// closed torrent, or an all-zero peerless swarm), which hides the bar.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PieceInfo {
    pub size_chunks: i64,
    pub completed_chunks: i64,
    pub chunk_size: i64,
    pub bitfield: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability: Option<String>,
}

/// Payload of the `state://detail` event / `GET /api/detail` response.
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

/// Internal options passed to the torrent creation engine.
#[derive(Debug, Clone)]
pub struct CreateTorrentOptions {
    pub source_path: std::path::PathBuf,
    pub piece_length: Option<i64>,
    pub trackers: Vec<String>,
    pub is_private: bool,
    pub comment: Option<String>,
    pub source: Option<String>,
    pub created_by: Option<String>,
}

/// Parameters for creating a new `.torrent` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTorrentParams {
    /// Path to single file or directory on the host.
    pub source_path: String,
    /// Destination `.torrent` file path on disk (optional).
    pub output_path: Option<String>,
    /// Piece size in bytes (e.g. 262144 for 256 KiB), or 0 / None for auto.
    pub piece_length: Option<i64>,
    /// Trackers list (grouped by tiers or single list).
    #[serde(default)]
    pub trackers: Vec<String>,
    /// Private torrent flag (BEP 27).
    #[serde(default)]
    pub is_private: bool,
    /// Optional comment string.
    pub comment: Option<String>,
    /// Optional private tracker source identifier.
    pub source: Option<String>,
    /// Automatically add and start seeding the newly created torrent in the daemon.
    #[serde(default)]
    pub start_seeding: bool,
}

/// Result from creating a torrent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTorrentResult {
    pub name: String,
    pub info_hash: String,
    pub total_size: i64,
    pub piece_length: i64,
    pub piece_count: i64,
    pub output_path: Option<String>,
    pub is_private: bool,
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
    /// The password is deliberately absent: on the desktop it lives in the OS
    /// keychain (see `secrets`), never in plaintext settings; the web server
    /// supplies it from config via `RpcClient::with_password`.
    Http {
        url: String,
        #[serde(default)]
        username: String,
    },
}

/// What the daemon reports about itself (D16), for the Statistics dialog's
/// Daemon tab. Numeric fields are 0 / strings empty when a build doesn't expose
/// the corresponding method.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonHealth {
    pub client_version: String,
    pub api_version: String,
    pub session_path: String,
    /// `pieces.memory.max` / `pieces.memory.current`, bytes.
    pub memory_max: i64,
    pub memory_current: i64,
    pub open_sockets: i64,
    pub max_open_sockets: i64,
    pub max_open_files: i64,
    pub http_max_open: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shared DTOs must serialize as the camelCase keys `src/ipc/types.ts`
    /// declares — both hosts serialize *these* structs, so a dropped
    /// `rename_all` would silently break the web JSON. This is the contract
    /// guard (WE6-S2) on the Rust side; `tsc` guards the TypeScript side.
    #[test]
    fn dtos_serialize_camelcase() {
        let globals = GlobalStats {
            down_rate: 1,
            up_rate: 2,
            down_rate_limit: 0,
            up_rate_limit: 0,
            dht_nodes: 3,
            free_space: Some(4),
            disk_size: Some(5),
            turtle_active: false,
        };
        let v = serde_json::to_value(&globals).unwrap();
        for key in [
            "downRate",
            "upRate",
            "downRateLimit",
            "upRateLimit",
            "dhtNodes",
            "freeSpace",
            "diskSize",
            "turtleActive",
        ] {
            assert!(v.get(key).is_some(), "GlobalStats missing `{key}`");
        }

        let torrent = TorrentDto {
            hash: "H".into(),
            name: "n".into(),
            size: 0,
            bytes_done: 0,
            percent: 0.0,
            status: Status::Seeding,
            status_msg: String::new(),
            seeds_connected: 0,
            peers_connected: 0,
            seeds_swarm: 0,
            peers_swarm: 0,
            down_rate: 0,
            up_rate: 0,
            eta_seconds: None,
            ratio: 0.0,
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
            tags: vec![],
            force_start: false,
            throttle_rule: String::new(),
            views: vec![],
            error_kind: String::new(),
            is_open: true,
            is_active: true,
        };
        let tv = serde_json::to_value(&torrent).unwrap();
        for key in [
            "bytesDone",
            "statusMsg",
            "errorKind",
            "seedsConnected",
            "peersConnected",
            "seedsSwarm",
            "peersSwarm",
            "downRate",
            "upRate",
            "etaSeconds",
            "trackerHost",
            "savePath",
            "isPrivate",
            "throttleName",
            "downRateLimit",
            "upRateLimit",
            "startedAt",
            "finishedAt",
            // The console words "stopped" and "queued" differently, and these
            // two flags are the only way to tell them apart.
            "isOpen",
            "isActive",
            "forceStart",
            "throttleRule",
        ] {
            assert!(tv.get(key).is_some(), "TorrentDto missing `{key}`");
        }
        // The status enum is lowercased.
        assert_eq!(tv.get("status").unwrap(), "seeding");
    }
}
