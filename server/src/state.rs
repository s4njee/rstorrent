//! Shared server state: the daemon backend, the snapshot cache, connection
//! state, and the coordination primitives the poller and handlers use.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use tokio::sync::Notify;

use rtorrent_core::rtorrent::RtorrentApi;
use rtorrent_core::types::{
    ConnPhase, ConnState, DetailPayload, LogEntry, LogLevel, Snapshot, TorrentDto,
};

use crate::config::Config;

/// Most log lines kept in the ring buffer (fed to the Log tab).
const LOG_CAPACITY: usize = 500;

/// A serialized, cache-ready snapshot: the JSON body + ETag, plus the parsed
/// snapshot so mutation handlers can look up a torrent (name, priority).
#[derive(Clone)]
pub struct Cached {
    pub etag: String,
    pub body: Arc<[u8]>,
    pub snapshot: Arc<Snapshot>,
    pub at: Instant,
}

/// Cached delta between the last two snapshots (FND-02).
#[derive(Clone)]
pub struct CachedDelta {
    pub etag: String,
    pub body: Arc<[u8]>,
    pub delta: Arc<rtorrent_core::types::SnapshotDelta>,
    pub at: Instant,
}

pub struct AppState {
    pub config: Config,
    pub backend: Box<dyn RtorrentApi>,
    /// Latest serialized snapshot; `None` until the first successful poll.
    pub cache: RwLock<Option<Cached>>,
    /// Latest delta (since previous snapshot); `None` until second poll.
    pub delta_cache: RwLock<Option<CachedDelta>>,
    /// Previous snapshot for delta computation.
    pub prev_snapshot: RwLock<Option<Arc<Snapshot>>>,
    /// Monotonic revision counter (FND-02).
    pub revision: AtomicU64,
    pub conn: Mutex<ConnState>,
    /// hash → primary tracker host, filled by the slow poll.
    pub tracker_cache: Mutex<HashMap<String, String>>,
    pub log: Mutex<VecDeque<LogEntry>>,
    /// Total log entries ever appended; an entry's absolute sequence number. The
    /// Log tab asks for entries `after` a sequence, so it only pulls new lines.
    pub log_total: AtomicU64,
    /// Per-`(hash, tab)` detail micro-cache, so rapid detail polls don't hammer
    /// the daemon. Keyed `"HASH:tab"`.
    pub detail_cache: Mutex<HashMap<String, (DetailPayload, Instant)>>,
    /// When the last authenticated `/api/state` was served (idle-stop clock).
    pub last_request: Mutex<Instant>,
    /// A request arrived — wake the poller if it parked itself for idle.
    pub activity: Notify,
    /// A mutation happened — poll immediately rather than waiting the interval.
    pub repoll: Notify,
    /// The poller wrote a fresh snapshot — wake handlers waiting on a cold cache.
    pub cache_updated: Notify,
    /// Count of daemon polls performed; lets tests prove idle-stop halts traffic.
    pub poll_count: AtomicU64,
    /// Web-login sessions (WE5).
    pub sessions: crate::auth::Sessions,
    /// Per-IP login rate limiter (WE5).
    pub rate: crate::auth::RateLimiter,
    /// The server-owned interface preferences (theme, density, …).
    pub ui: crate::settings::UiSettingsStore,
    /// The effective login-password hash. `Config` holds the startup value; this
    /// is the live one, so a password change takes effect without a restart.
    pub password_hash: RwLock<Option<String>>,
    /// Bounded global-rate history for the Stats route (WC8-S2).
    pub history: Mutex<crate::history::RateHistory>,
    /// Lazily built file-path index for library search (V3-12).
    pub file_index: Mutex<rtorrent_core::file_index::FileIndex>,
    /// Move-on-complete journal + live tasks (V3-14).
    pub moves: Mutex<rtorrent_core::mover::MoveStore>,
    /// Session-import status + cancel flag (V3-22 / LIB-09).
    pub import_status: Mutex<crate::session::ImportStatus>,
    pub import_cancel: std::sync::atomic::AtomicBool,
    /// Queue edge memory (V3-17): manual resumes/pauses the scheduler must
    /// not fight. The daemon is the source of truth; this only remembers
    /// transitions between ticks.
    pub queue_mem: Mutex<rtorrent_core::queue::QueueMemory>,
    /// The move journal was recovered once after (re)start (V3-14).
    pub moves_resumed: std::sync::atomic::AtomicBool,
    /// When this server started, for the Stats route's uptime.
    pub started: Instant,
}

impl AppState {
    pub fn new(config: Config, backend: Box<dyn RtorrentApi>) -> Self {
        let endpoint = crate::endpoint_label(&config.transport);
        let ui = crate::settings::UiSettingsStore::beside(config.config_path.as_deref());
        let password_hash = RwLock::new(config.password_hash.clone());
        let sessions = match config.session_store_path() {
            Some(path) => crate::auth::Sessions::with_store(path),
            None => crate::auth::Sessions::default(),
        };
        let moves = Mutex::new(rtorrent_core::mover::MoveStore::load(
            crate::moves::journal_path(config.config_path.as_deref()),
        ));
        Self {
            config,
            backend,
            cache: RwLock::new(None),
            delta_cache: RwLock::new(None),
            prev_snapshot: RwLock::new(None),
            revision: AtomicU64::new(0),
            conn: Mutex::new(ConnState {
                phase: ConnPhase::Connecting,
                endpoint,
                daemon_version: None,
                error: None,
                retry_in_seconds: None,
            }),
            tracker_cache: Mutex::new(HashMap::new()),
            log: Mutex::new(VecDeque::new()),
            log_total: AtomicU64::new(0),
            detail_cache: Mutex::new(HashMap::new()),
            last_request: Mutex::new(Instant::now()),
            activity: Notify::new(),
            repoll: Notify::new(),
            cache_updated: Notify::new(),
            poll_count: AtomicU64::new(0),
            sessions,
            rate: crate::auth::RateLimiter::default(),
            ui,
            password_hash,
            history: Mutex::new(crate::history::RateHistory::default()),
            file_index: Mutex::new(rtorrent_core::file_index::FileIndex::default()),
            queue_mem: Mutex::new(rtorrent_core::queue::QueueMemory::default()),
            moves,
            moves_resumed: std::sync::atomic::AtomicBool::new(false),
            import_status: Mutex::new(crate::session::ImportStatus::default()),
            import_cancel: std::sync::atomic::AtomicBool::new(false),
            started: Instant::now(),
        }
    }

    /// Record one global-rate sample for the Stats history.
    pub fn record_rate_sample(&self, at_ms: i64, down: i64, up: i64) {
        self.history.lock().unwrap().record(at_ms, down, up);
    }

    /// A snapshot of the rate history, oldest first.
    pub fn rate_history(&self) -> Vec<crate::history::StatsSample> {
        self.history.lock().unwrap().to_vec()
    }

    /// Seconds since this server started.
    pub fn uptime_seconds(&self) -> i64 {
        self.started.elapsed().as_secs() as i64
    }

    /// The live login-password hash (the changed one, not the startup value).
    pub fn password_hash(&self) -> Option<String> {
        self.password_hash.read().unwrap().clone()
    }

    /// Replace the live login-password hash after a verified change.
    pub fn set_password_hash(&self, hash: String) {
        *self.password_hash.write().unwrap() = Some(hash);
    }

    pub fn conn(&self) -> ConnState {
        self.conn.lock().unwrap().clone()
    }

    pub fn set_conn(&self, conn: ConnState) {
        *self.conn.lock().unwrap() = conn;
    }

    /// Resolve a hash to its cached primary tracker host (empty if unknown).
    pub fn tracker_host(&self, hash: &str) -> String {
        self.tracker_cache
            .lock()
            .unwrap()
            .get(hash)
            .cloned()
            .unwrap_or_default()
    }

    /// The most recently cached torrent row for a hash, if any.
    pub fn torrent(&self, hash: &str) -> Option<TorrentDto> {
        let cache = self.cache.read().unwrap();
        let cached = cache.as_ref()?;
        cached
            .snapshot
            .torrents
            .iter()
            .find(|t| t.hash == hash)
            .cloned()
    }

    /// Note that a client just asked for state, for the idle-stop clock, and
    /// wake the poller if it was parked.
    pub fn mark_request(&self) {
        *self.last_request.lock().unwrap() = Instant::now();
        self.activity.notify_waiters();
    }

    /// Append a log line (bounded ring buffer) and bump the sequence counter.
    pub fn log(&self, level: LogLevel, message: impl Into<String>, hash: Option<String>) {
        let mut log = self.log.lock().unwrap();
        if log.len() >= LOG_CAPACITY {
            log.pop_front();
        }
        log.push_back(LogEntry {
            time: unix_now(),
            level,
            message: message.into(),
            hash,
        });
        self.log_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Log entries with an absolute sequence `>= after`, and the new sequence
    /// high-water mark (the total appended). A client passes back the returned
    /// `seq` as `after` next time, so it only ever receives new lines.
    pub fn log_since(&self, after: u64) -> (Vec<LogEntry>, u64) {
        let log = self.log.lock().unwrap();
        let total = self.log_total.load(Ordering::Relaxed);
        // The buffer holds the last `len` entries, i.e. sequences
        // `[total - len, total)`. Skip those already seen.
        let first_seq = total - log.len() as u64;
        let skip = after.saturating_sub(first_seq) as usize;
        let entries = log.iter().skip(skip).cloned().collect();
        (entries, total)
    }
}

/// Seconds since the Unix epoch (server wall clock).
pub fn unix_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// A strong ETag derived from the response body — changes iff the body changes.
pub fn etag_of(bytes: &[u8]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("\"{:016x}\"", hasher.finish())
}
