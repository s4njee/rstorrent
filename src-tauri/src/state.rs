//! Shared application state.
//!
//! A single [`AppState`] is managed by Tauri (`app.manage(...)`) and shared by
//! every command and the background poller. It owns the current rtorrent backend
//! (swappable when the connection settings change), the settings, the app log,
//! and the small caches the poller maintains.
//!
//! Locking rule: the backend is stored behind an `RwLock<Arc<…>>`; always clone
//! the `Arc` out under a short lock and drop the guard *before* awaiting on it,
//! so we never hold a std lock across an `.await`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;

use crate::ipc::{ConnPhase, ConnState, DetailTab, LogLevel, Settings};
use crate::log::LogBuffer;
use crate::rtorrent::{make_backend, RtorrentApi};
use crate::settings;

pub struct AppState {
    /// Current backend (real SCGI client or mock), swappable at runtime.
    backend: RwLock<Arc<dyn RtorrentApi>>,
    /// Live settings; persisted to `settings_path` on change.
    settings: RwLock<Settings>,
    settings_path: PathBuf,
    /// Bounded app event log (Log tab).
    pub log: LogBuffer,
    /// hash → primary tracker host, filled by the slow poll.
    pub tracker_cache: std::sync::Mutex<HashMap<String, String>>,
    /// Which torrent+tab the detail poll should fetch, if any.
    pub detail_watch: std::sync::Mutex<Option<(String, DetailTab)>>,
    /// Latest connection state, mirrored into every snapshot.
    pub conn: RwLock<ConnState>,
    /// Notified to request an immediate extra poll (after a user action).
    pub repoll: Notify,
    /// Notified to refresh the active detail tab immediately.
    pub detail_repoll: Notify,
    /// Path to the persisted since-install transfer counters (see `stats.rs`).
    pub stats_path: PathBuf,
}

impl AppState {
    /// Build state from the settings file, constructing the initial backend.
    pub fn new(settings_path: PathBuf) -> Self {
        let settings = settings::load(&settings_path);
        let backend = make_backend(settings.transport.clone(), settings.mock);
        let conn = ConnState {
            phase: ConnPhase::Connecting,
            endpoint: settings::endpoint_label(&settings.transport),
            daemon_version: None,
            error: None,
            retry_in_seconds: None,
        };
        // Persist the all-time counters next to the settings file.
        let stats_path = settings_path
            .parent()
            .map(|p| p.join("stats.json"))
            .unwrap_or_else(|| PathBuf::from("stats.json"));
        Self {
            backend: RwLock::new(Arc::from(backend)),
            settings: RwLock::new(settings),
            settings_path,
            log: LogBuffer::new(),
            tracker_cache: std::sync::Mutex::new(HashMap::new()),
            detail_watch: std::sync::Mutex::new(None),
            conn: RwLock::new(conn),
            repoll: Notify::new(),
            detail_repoll: Notify::new(),
            stats_path,
        }
    }

    /// Clone the current backend `Arc` (drop the guard before awaiting on it).
    pub fn backend(&self) -> Arc<dyn RtorrentApi> {
        self.backend.read().unwrap().clone()
    }

    /// Snapshot of the current settings.
    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    /// Replace settings, persist them, and rebuild the backend if the transport
    /// or mock flag changed. Returns the stored settings.
    pub fn update_settings(&self, next: Settings) -> Settings {
        let rebuild = {
            let cur = self.settings.read().unwrap();
            cur.transport != next.transport || cur.mock != next.mock
        };
        *self.settings.write().unwrap() = next.clone();
        let _ = settings::save(&self.settings_path, &next);
        if rebuild {
            let backend = make_backend(next.transport.clone(), next.mock);
            *self.backend.write().unwrap() = Arc::from(backend);
            self.set_conn(ConnState {
                phase: ConnPhase::Connecting,
                endpoint: settings::endpoint_label(&next.transport),
                daemon_version: None,
                error: None,
                retry_in_seconds: None,
            });
            self.repoll.notify_one();
        }
        next
    }

    /// Current tracker host for a hash, if the slow poll has resolved it.
    pub fn tracker_host(&self, hash: &str) -> String {
        self.tracker_cache
            .lock()
            .unwrap()
            .get(hash)
            .cloned()
            .unwrap_or_default()
    }

    pub fn conn(&self) -> ConnState {
        self.conn.read().unwrap().clone()
    }

    pub fn set_conn(&self, next: ConnState) {
        *self.conn.write().unwrap() = next;
    }

    /// Append to the log and push it to the frontend.
    pub fn log(&self, app: &AppHandle, level: LogLevel, message: impl Into<String>, hash: Option<String>) {
        let entry = self.log.push(level, message, hash);
        let _ = app.emit("log://append", entry);
    }
}
