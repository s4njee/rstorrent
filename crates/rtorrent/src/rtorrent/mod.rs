//! rtorrent client layer.
//!
//! This module owns *all* communication with the rtorrent daemon. Nothing above
//! it (commands, poller) knows about XML-RPC or SCGI — they see the
//! [`RtorrentApi`] trait and the plain `Raw*` data structs defined here.
//!
//! Two implementations back the trait:
//!   * [`client::RpcClient`] — a real daemon, over SCGI (unix socket / TCP) or
//!     XML-RPC over HTTP(S) for a remote one; see [`transport`].
//!   * [`mock::MockClient`]   — the design-fixture torrents, for offline dev/tests.
//!
//! The split lets the whole UI run with `RSTORRENT_MOCK=1` and lets the transport
//! code be unit-tested against an in-process fixture server.

pub mod client;
pub mod derive;
pub mod http;
pub mod mock;
pub mod scgi;
pub mod transport;
pub mod xmlrpc;

use async_trait::async_trait;

use crate::types::Transport;

/// Errors surfaced by the rtorrent layer. Mapped to strings at the IPC edge.
#[derive(Debug, thiserror::Error)]
pub enum RtorrentError {
    /// Could not reach the daemon (socket refused / not found / DNS).
    #[error("cannot reach rtorrent: {0}")]
    Unreachable(String),
    /// Connected, but the request timed out.
    #[error("rtorrent request timed out")]
    Timeout,
    /// Transport- or framing-level problem (bad SCGI/CGI response).
    #[error("protocol error: {0}")]
    Protocol(String),
    /// The XML body could not be parsed.
    #[error("failed to parse response: {0}")]
    Parse(String),
    /// rtorrent returned an XML-RPC `<fault>`.
    #[error("rtorrent fault {code}: {message}")]
    Fault { code: i64, message: String },
    /// A method returned data in an unexpected shape.
    #[error("unexpected response: {0}")]
    Unexpected(String),
}

/// Convenience result alias for the rtorrent layer.
pub type Result<T> = std::result::Result<T, RtorrentError>;

/// Raw per-torrent fields as fetched by `d.multicall2`. This is a faithful,
/// *underived* view of the daemon state; [`derive`] turns it into a
/// presentation-ready [`crate::types::TorrentDto`].
#[derive(Debug, Clone, Default)]
pub struct RawTorrent {
    pub hash: String,
    pub name: String,
    pub size_bytes: i64,
    pub bytes_done: i64,
    /// `d.complete`: 1 when all wanted chunks are present.
    pub complete: bool,
    /// `d.is_active`: torrent is started and participating.
    pub is_active: bool,
    /// `d.is_open`: torrent has open file handles (0 when stopped).
    pub is_open: bool,
    /// `d.hashing`: non-zero while a hash check is running.
    pub hashing: bool,
    /// `d.message`: tracker/storage error text, empty when healthy.
    pub message: String,
    pub down_rate: i64,
    pub up_rate: i64,
    /// `d.ratio`: per-mille (divide by 1000 for the display ratio).
    pub ratio_permille: i64,
    /// `d.custom1`: ruTorrent label convention.
    pub label: String,
    pub directory: String,
    pub base_path: String,
    /// `d.peers_complete`: seeds in the swarm.
    pub peers_complete: i64,
    /// `d.peers_accounted`: leechers we know about.
    pub peers_accounted: i64,
    pub peers_connected: i64,
    pub priority: i64,
    pub is_private: bool,
    /// `d.throttle_name`: empty means the torrent uses the global throttle.
    pub throttle_name: String,
    /// `d.timestamp.finished`: Unix seconds, or zero when unavailable.
    pub finished_at: i64,
    /// `d.chunks_hashed`: chunks verified so far. Counts up during a hash check;
    /// equals the completed-chunk count when idle, so only meaningful while
    /// [`Self::hashing`] (see the `Checking` progress in [`derive`]).
    pub chunks_hashed: i64,
    /// `d.size_chunks`: total chunks — the denominator for `chunks_hashed`.
    pub size_chunks: i64,
    /// `d.timestamp.started`: Unix seconds first started. Unlike `d.load_date`
    /// (which resets when the daemon reloads its session) this is persisted in
    /// the resume file, so it's the durable "since" for the Started column (D4).
    pub started_at: i64,
}

/// Raw global counters fetched alongside the torrent list each poll.
#[derive(Debug, Clone, Default)]
pub struct RawGlobal {
    pub down_rate: i64,
    pub up_rate: i64,
    pub down_rate_limit: i64,
    pub up_rate_limit: i64,
    pub dht_nodes: i64,
}

/// Raw figures for the Statistics dialog. Session totals come from rtorrent's
/// `throttle.global_*.total` (reset each daemon session); the command layer
/// derives the persisted "since install" totals from these. `None` fields are
/// ones this rtorrent build doesn't expose.
#[derive(Debug, Clone, Default)]
pub struct RawStats {
    pub session_down: i64,
    pub session_up: i64,
    pub connected_peers: i64,
    pub session_waste: i64,
    pub buffer_size: Option<i64>,
    pub cache_hit_pct: Option<f64>,
    pub cache_overload_pct: Option<f64>,
    pub queued_io: Option<i64>,
}

/// Options for a load/add request, translated into rtorrent commands.
#[derive(Debug, Clone)]
pub struct LoadOptions {
    pub directory: String,
    pub label: String,
    pub start: bool,
    pub top_of_queue: bool,
    /// File indexes to set to priority 0 (skip) after load. Wired end-to-end in
    /// E8-S4 (needs the resolved info-hash to target `f.priority.set`); carried
    /// through the API now so the contract is stable.
    #[allow(dead_code)]
    pub unselected_indexes: Vec<usize>,
}

/// The daemon-agnostic surface the rest of the app programs against.
///
/// Implementations must be `Send + Sync` so the poller can hold one behind an
/// `Arc` and call it from tokio worker threads.
#[async_trait]
pub trait RtorrentApi: Send + Sync {
    /// rtorrent version string, e.g. `"0.9.8"` (used by title bar / connection test).
    async fn client_version(&self) -> Result<String>;

    /// One `d.multicall2` over the main view → every torrent's raw fields.
    async fn list_snapshot(&self) -> Result<Vec<RawTorrent>>;

    /// Global rates, limits, and DHT node count.
    async fn global_stats(&self) -> Result<RawGlobal>;

    /// Primary tracker URL for a torrent (first announce URL), for the slow poll.
    async fn primary_tracker(&self, hash: &str) -> Result<String>;

    /// Tracker rows for the Trackers detail tab.
    async fn trackers(&self, hash: &str) -> Result<Vec<crate::types::TrackerRow>>;

    /// Append an announce URL to a torrent's tracker list.
    async fn add_tracker(&self, hash: &str, url: &str) -> Result<()>;

    /// Remove a tracker when supported, otherwise disable it.
    async fn remove_tracker(&self, hash: &str, index: usize) -> Result<()>;

    /// Enable or disable one tracker by its zero-based list index.
    async fn set_tracker_enabled(&self, hash: &str, index: usize, enabled: bool) -> Result<()>;

    /// Ask the selected torrents to announce to their trackers now.
    async fn force_reannounce(&self, hashes: &[String]) -> Result<()>;

    /// Peer rows for the Peers detail tab.
    async fn peers(&self, hash: &str) -> Result<Vec<crate::types::PeerRow>>;

    /// Ban a peer (`p.banned.set = 1`) and drop the connection (B16). A banned
    /// peer is refused if it tries to reconnect this session.
    async fn ban_peer(&self, hash: &str, peer_id: &str) -> Result<()>;

    /// Snub a peer (`p.snubbed.set = 1`) — stop uploading to it, without
    /// disconnecting (B16).
    async fn snub_peer(&self, hash: &str, peer_id: &str) -> Result<()>;

    /// Disconnect a peer now (`p.disconnect`), without banning it (B16).
    async fn disconnect_peer(&self, hash: &str, peer_id: &str) -> Result<()>;

    /// File rows for the Content detail tab (and re-used by the Add dialog).
    async fn files(&self, hash: &str) -> Result<Vec<crate::types::FileNode>>;

    /// Piece/chunk state for the General tab's pieces bar.
    async fn pieces(&self, hash: &str) -> Result<crate::types::PieceInfo>;

    async fn start(&self, hashes: &[String]) -> Result<()>;
    async fn stop(&self, hashes: &[String]) -> Result<()>;
    async fn recheck(&self, hashes: &[String]) -> Result<()>;
    async fn erase(&self, hashes: &[String]) -> Result<()>;

    /// Load raw `.torrent` bytes with the given options; returns the info-hash.
    async fn load_raw(&self, bytes: Vec<u8>, opts: LoadOptions) -> Result<()>;
    /// Load a magnet URI / torrent URL with the given options.
    async fn load_magnet(&self, uri: &str, opts: LoadOptions) -> Result<()>;

    async fn set_label(&self, hashes: &[String], label: &str) -> Result<()>;
    async fn set_directory(&self, hash: &str, path: &str) -> Result<()>;
    async fn set_priority(&self, hash: &str, priority: i64) -> Result<()>;
    async fn set_file_priority(&self, hash: &str, index: usize, priority: i64) -> Result<()>;

    /// Define or update both directions of a named throttle (rates in KiB/s,
    /// zero = unlimited). rtorrent requires rate arguments to be strings.
    async fn define_named_throttle(&self, name: &str, down_kb: i64, up_kb: i64) -> Result<()>;

    /// Assign a named throttle to all hashes. `None` clears the assignment and
    /// returns those torrents to the global throttle.
    async fn assign_throttle(&self, hashes: &[String], name: Option<&str>) -> Result<()>;

    /// Read one torrent's current named-throttle assignment.
    async fn torrent_throttle_name(&self, hash: &str) -> Result<String>;

    /// Read a torrent's on-disk base path (needed before "delete data").
    async fn base_path(&self, hash: &str) -> Result<String>;

    /// Set the global up/down throttle in KiB/s (0 = unlimited).
    async fn set_throttles(&self, down_kb: i64, up_kb: i64) -> Result<()>;

    /// Set the incoming listen port range, e.g. "6881-6899".
    async fn set_port_range(&self, range: &str) -> Result<()>;

    /// Apply a batch of global `<method> = <value>` config directives to the
    /// running daemon, best-effort. Used by the 1 Gbps tuner to push the values
    /// it also writes to `.rtorrent.rc` so they take effect without a restart.
    /// Returns how many of the directives the daemon accepted.
    async fn apply_config(&self, directives: &[(&str, i64)]) -> Result<usize>;

    /// Like [`Self::apply_config`], but for directives whose argument is a string
    /// (encryption flags, proxy/bind addresses). Best-effort; returns the count
    /// the daemon accepted.
    async fn apply_config_str(&self, directives: &[(&str, &str)]) -> Result<usize>;

    /// Enable or disable the DHT.
    async fn set_dht(&self, enabled: bool) -> Result<()>;

    /// Aggregate figures for the Statistics dialog.
    async fn statistics(&self) -> Result<RawStats>;

    /// Native rtorrent views and their member hashes (D12), excluding the
    /// everything-views `main`/`default`. Read-only surface for the sidebar.
    async fn views(&self) -> Result<Vec<(String, Vec<String>)>>;

    /// What the daemon reports about itself, for the health panel (D16).
    async fn daemon_health(&self) -> Result<crate::types::DaemonHealth>;

    /// Ask the daemon to write its session now (`session.save`) (D13).
    async fn save_session(&self) -> Result<()>;

    /// Ask the daemon to shut down cleanly (`system.shutdown.normal`) (D13).
    async fn shutdown(&self) -> Result<()>;
}

/// Build the appropriate backend for the given settings. When `mock` is set (or
/// the `RSTORRENT_MOCK` env var is present) the fixture client is returned so the
/// app runs with no daemon.
pub fn make_backend(transport: Transport, mock: bool) -> Box<dyn RtorrentApi> {
    if mock || std::env::var("RSTORRENT_MOCK").is_ok() {
        Box::new(mock::MockClient::new())
    } else {
        Box::new(client::RpcClient::new(transport))
    }
}
