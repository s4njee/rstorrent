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
    /// Native rtorrent views this torrent belongs to (D12); filled by the
    /// poller from `view.list`. Empty until the first view refresh.
    #[serde(default)]
    pub views: Vec<String>,
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

/// A saved, named daemon connection (B10). The active connection is whichever
/// profile's transport currently sits in [`Settings::transport`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
    pub name: String,
    pub transport: Transport,
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

    // --- Network pane (v1.6) ---
    /// Protocol-encryption preset (D7). Write-only on 0.16.17 — see [`EncryptionMode`].
    #[serde(default = "default_encryption")]
    pub encryption: EncryptionMode,
    /// Peer exchange (`protocol.pex.set`) (D7).
    #[serde(default = "default_pex_enabled")]
    pub pex_enabled: bool,
    /// HTTP proxy `host:port` for tracker announces (D8); empty = none.
    #[serde(default)]
    pub proxy_address: String,
    /// Whether to route tracker HTTP requests through [`Self::proxy_address`] (D8).
    #[serde(default)]
    pub proxy_tracker_http: bool,
    /// `network.bind_address` — bind outgoing/listen to this address, e.g. a VPN
    /// interface (D9); empty = don't manage (the daemon default, all interfaces).
    #[serde(default)]
    pub bind_address: String,
    /// `network.local_address` — the address reported to trackers/peers (D9);
    /// empty = don't manage.
    #[serde(default)]
    pub local_address: String,
    /// Global peer cap per torrent (`throttle.max_peers.normal/seed`) (D11);
    /// 0 = leave the daemon default.
    #[serde(default)]
    pub max_peers: i64,
    /// Global simultaneous upload slots (`throttle.max_uploads.global`) (D11);
    /// 0 = unlimited.
    #[serde(default)]
    pub max_uploads_global: i64,
    /// Global simultaneous download slots (`throttle.max_downloads.global`) (D11);
    /// 0 = unlimited.
    #[serde(default)]
    pub max_downloads_global: i64,

    // --- Automation (v1.7) ---
    /// Keep at most this many torrents downloading; the app queues the rest
    /// (C9). 0 = unlimited (no queue management).
    #[serde(default)]
    pub max_active_downloads: i64,
    /// Per-label default save paths (C11).
    #[serde(default)]
    pub label_defaults: Vec<LabelDefault>,
    /// Watched folders for auto-add (C12). Supersedes [`Self::watch_folder`],
    /// which is migrated into this list on load.
    #[serde(default)]
    pub watch_folders: Vec<WatchFolder>,
    /// Command run on this machine when a torrent completes (C13); empty =
    /// disabled. Tokens `%N` (name), `%F` (save path), `%H` (hash) are
    /// substituted. Run directly (no shell) — point it at a script for pipes.
    #[serde(default)]
    pub run_on_complete: String,
    /// What to do when a torrent reaches its seed goal (C14).
    #[serde(default)]
    pub seed_goal_action: SeedGoalAction,
    /// Turtle (alternative) download limit, KiB/s; 0 = unlimited (B14).
    #[serde(default)]
    pub turtle_down_kb: i64,
    /// Turtle upload limit, KiB/s; 0 = unlimited (B14).
    #[serde(default)]
    pub turtle_up_kb: i64,
    /// Manual turtle-mode toggle (B14). The effective state is this OR an active
    /// schedule window.
    #[serde(default)]
    pub turtle_enabled: bool,
    /// Optional daily schedule that auto-engages turtle mode (B14).
    #[serde(default)]
    pub turtle_schedule: TurtleSchedule,

    /// Saved daemon connections (B10). The active one is mirrored in `transport`.
    #[serde(default)]
    pub connection_profiles: Vec<ConnectionProfile>,

    // --- RSS (v2.0 / B11) ---
    /// RSS/Atom feeds to poll.
    #[serde(default)]
    pub rss_feeds: Vec<RssFeed>,
    /// Auto-download rules matched against feed items.
    #[serde(default)]
    pub rss_rules: Vec<RssRule>,
    /// How often to poll feeds, minutes; 0 disables background polling.
    #[serde(default = "default_rss_poll_minutes")]
    pub rss_poll_minutes: i64,

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

/// Outgoing/incoming BitTorrent protocol encryption preset (D7).
///
/// rtorrent 0.16.17 has no getter for `protocol.encryption`, so the app can't
/// read the daemon's current mode — it persists the last preset it applied and
/// shows that (the UI says as much).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EncryptionMode {
    /// No encryption (`none`).
    Disabled,
    /// Accept encrypted peers, plaintext outgoing (`allow_incoming,try_outgoing`).
    #[default]
    Allow,
    /// Try encrypted outgoing, retry with encryption on failure.
    Prefer,
    /// Require encryption both ways.
    Require,
}

impl EncryptionMode {
    /// The `protocol.encryption.set` flag list for this preset.
    pub fn flags(self) -> &'static str {
        match self {
            EncryptionMode::Disabled => "none",
            EncryptionMode::Allow => "allow_incoming,try_outgoing",
            EncryptionMode::Prefer => "allow_incoming,try_outgoing,enable_retry",
            EncryptionMode::Require => "allow_incoming,require,require_RC4,enable_retry",
        }
    }
}

/// What to do with a torrent when its seed goal is reached (C14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SeedGoalAction {
    /// Stop seeding but keep the torrent (the original v1 behavior).
    #[default]
    Stop,
    /// Remove the torrent from rtorrent, leaving the data on disk.
    Remove,
    /// Remove the torrent and move its data to the Trash (local daemons only).
    RemoveData,
}

/// A per-label default (C11): overrides the global save path for torrents added
/// with this label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelDefault {
    pub label: String,
    pub save_path: String,
}

/// One watched folder (C12). `label`/`save_path` are optional per-folder
/// overrides; empty means "fall back to the label default, then the global
/// default save path".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchFolder {
    pub path: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub save_path: String,
}

/// Daily window that auto-engages turtle mode (B14).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurtleSchedule {
    pub enabled: bool,
    /// Window start, minutes since local midnight `[0, 1440)`.
    pub start_min: i64,
    /// Window end, minutes since local midnight. If `end <= start` the window
    /// wraps past midnight (e.g. 23:00→06:00).
    pub end_min: i64,
    /// Active weekdays, `0 = Sunday .. 6 = Saturday`. Empty = every day.
    pub days: Vec<u8>,
}

/// An RSS/Atom feed polled for auto-add (B11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RssFeed {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// An auto-download rule: items whose title matches are added (B11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RssRule {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Feed id this rule applies to; empty = every feed.
    #[serde(default)]
    pub feed_id: String,
    /// Whitespace-separated tokens that must *all* appear in the title
    /// (case-insensitive). Empty matches everything.
    #[serde(default)]
    pub must_contain: String,
    /// Whitespace-separated tokens; if *any* appears, the item is skipped.
    #[serde(default)]
    pub must_not_contain: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub save_path: String,
}

/// One parsed feed entry (B11), shown in the RSS preview and matched by rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    pub title: String,
    /// The download URL: a magnet link or a `.torrent` URL (enclosure preferred).
    pub link: String,
    /// Stable identity for dedup (`guid`/`id`, or the link as a fallback).
    pub guid: String,
    pub pub_date: String,
}

fn default_true() -> bool {
    true
}

fn default_rss_poll_minutes() -> i64 {
    15
}

fn default_port_range() -> String {
    "6881-6899".to_string()
}

fn default_pex_enabled() -> bool {
    true
}

fn default_encryption() -> EncryptionMode {
    EncryptionMode::Allow
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
