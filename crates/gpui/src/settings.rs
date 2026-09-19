//! Durable, non-secret settings for the GPUI shell.
//!
//! The Tauri backend stores settings as one JSON file in the app config
//! directory (`com.rstorrent.app/settings.json`), edited through the
//! `get_settings`/`apply_settings` commands. The native shell reads and writes
//! the same file with the same shape, so a user can switch shells without
//! losing their daemon connection, limits, or automation.
//!
//! Unknown fields are ignored and missing fields take serde defaults, so the
//! file survives upgrades. Corrupt files fall back to defaults rather than
//! bricking the app, matching `src-tauri/src/settings.rs`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub use rtorrent_core::types::Transport;

/// The settings document. Field-for-field the Tauri `Settings` shape, with
/// the same serde defaults, so the same file loads in both shells.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub transport: Transport,
    pub poll_ms: u64,
    pub stall_window_s: u64,
    pub default_save_path: String,
    pub show_add_dialog: bool,
    pub confirm_on_remove: bool,
    pub down_limit_kb: i64,
    pub up_limit_kb: i64,
    pub port_range: String,
    pub dht_enabled: bool,
    pub watch_folder: String,
    pub completion_notification_excluded_labels: Vec<String>,
    pub torrent_throttles: Vec<NamedThrottle>,
    pub global_seed_goal: SeedGoal,
    pub label_seed_goals: Vec<LabelSeedGoal>,
    pub encryption: EncryptionMode,
    pub pex_enabled: bool,
    pub proxy_address: String,
    pub proxy_tracker_http: bool,
    pub bind_address: String,
    pub local_address: String,
    pub max_peers: i64,
    pub max_uploads_global: i64,
    pub max_downloads_global: i64,
    pub max_active_downloads: i64,
    /// Keep at most this many finished torrents seeding (V3-17 / QUE-01).
    /// 0 = unlimited.
    pub max_active_uploads: i64,
    /// Keep at most this many torrents active in total (V3-17 / QUE-01).
    /// 0 = unlimited.
    pub max_active_torrents: i64,
    /// Torrents slower than this (down + up, KiB/s) are left alone entirely
    /// (V3-17 / QUE-01). 0 = disabled.
    pub queue_slow_limit_kbs: i64,
    pub label_defaults: Vec<LabelDefault>,
    pub watch_folders: Vec<WatchFolder>,
    /// "Keep incomplete torrents in" (V3-14 / LIB-08). Empty = disabled.
    pub incomplete_dir: String,
    /// Destination rules for completed data, by tag or label (V3-14 / LIB-07).
    pub move_rules: Vec<rtorrent_core::complete::MoveRule>,
    /// Named bandwidth profiles applied by tag/label (V3-18 / QUE-04).
    pub bandwidth_rules: Vec<rtorrent_core::bandwidth::BandwidthRule>,
    /// What to do when a move destination is already taken (V3-14).
    pub collision_policy: rtorrent_core::complete::CollisionPolicy,
    pub run_on_complete: String,
    pub seed_goal_action: SeedGoalAction,
    pub turtle_down_kb: i64,
    pub turtle_up_kb: i64,
    pub turtle_enabled: bool,
    pub turtle_schedule: TurtleSchedule,
    /// Weekly scheduler grid (V3-18 / QUE-05); see the Tauri shape.
    pub schedule: rtorrent_core::schedule::Schedule,
    pub connection_profiles: Vec<ConnectionProfile>,
    pub rss_feeds: Vec<RssFeed>,
    pub rss_rules: Vec<RssRule>,
    pub rss_poll_minutes: i64,
    pub mock: bool,
    // --- GPUI-only view state (never written by the Tauri shell) ---
    /// Active sidebar filter, e.g. `status:downloading` or `label:video`.
    pub filter: String,
    pub search: String,
    pub sort_column: String,
    pub sort_descending: bool,
    pub sidebar_width: u32,
    /// Table column widths and visibility; empty means the defaults.
    pub columns: Vec<crate::columns::ColumnPref>,
    /// The detail panel's active pane, e.g. `files` or `trackers`.
    pub active_tab: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            transport: default_transport(),
            poll_ms: 1000,
            stall_window_s: 30,
            default_save_path: default_save_path(),
            show_add_dialog: true,
            confirm_on_remove: true,
            down_limit_kb: 0,
            up_limit_kb: 0,
            port_range: "6881-6899".to_owned(),
            dht_enabled: false,
            watch_folder: String::new(),
            completion_notification_excluded_labels: Vec::new(),
            torrent_throttles: Vec::new(),
            global_seed_goal: SeedGoal::default(),
            label_seed_goals: Vec::new(),
            encryption: EncryptionMode::Allow,
            pex_enabled: true,
            proxy_address: String::new(),
            proxy_tracker_http: false,
            bind_address: String::new(),
            local_address: String::new(),
            max_peers: 0,
            max_uploads_global: 0,
            max_downloads_global: 0,
            max_active_downloads: 0,
            max_active_uploads: 0,
            max_active_torrents: 0,
            queue_slow_limit_kbs: 0,
            label_defaults: Vec::new(),
            watch_folders: Vec::new(),
            incomplete_dir: String::new(),
            move_rules: Vec::new(),
            bandwidth_rules: Vec::new(),
            collision_policy: Default::default(),
            run_on_complete: String::new(),
            seed_goal_action: SeedGoalAction::Stop,
            turtle_down_kb: 0,
            turtle_up_kb: 0,
            turtle_enabled: false,
            turtle_schedule: TurtleSchedule::default(),
            schedule: Default::default(),
            connection_profiles: Vec::new(),
            rss_feeds: Vec::new(),
            rss_rules: Vec::new(),
            rss_poll_minutes: 15,
            mock: std::env::var("RSTORRENT_MOCK").is_ok(),
            filter: String::new(),
            search: String::new(),
            sort_column: "name".to_owned(),
            sort_descending: false,
            sidebar_width: 206,
            columns: Vec::new(),
            active_tab: "files".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedThrottle {
    pub name: String,
    pub down_kb: i64,
    pub up_kb: i64,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedGoal {
    pub stop_ratio: f64,
    pub seed_hours: f64,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelSeedGoal {
    pub label: String,
    pub stop_ratio: f64,
    pub seed_hours: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EncryptionMode {
    Disabled,
    #[default]
    Allow,
    Prefer,
    Require,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SeedGoalAction {
    #[default]
    Stop,
    Remove,
    RemoveData,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelDefault {
    pub label: String,
    pub save_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchFolder {
    pub path: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub save_path: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurtleSchedule {
    pub enabled: bool,
    pub start_min: i64,
    pub end_min: i64,
    pub days: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
    pub name: String,
    pub transport: Transport,
}

/// RSS shapes live in the shared core so every shell plans against the same
/// document (V3-23); these aliases keep the existing paths working.
pub use rtorrent_core::rss::Feed as RssFeed;
pub use rtorrent_core::rss::Rule as RssRule;

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" }))
}

#[cfg(not(target_os = "windows"))]
fn default_transport() -> Transport {
    Transport::UnixSocket {
        path: home_dir()
            .join(".rtorrent")
            .join("rpc.socket")
            .to_string_lossy()
            .into_owned(),
    }
}

#[cfg(target_os = "windows")]
fn default_transport() -> Transport {
    Transport::Tcp {
        host: "127.0.0.1".to_owned(),
        port: 5000,
    }
}

/// The default save directory, in the daemon's namespace.
#[cfg(not(target_os = "windows"))]
#[must_use]
pub fn default_save_path() -> String {
    home_dir().join("Downloads").to_string_lossy().into_owned()
}

/// The default save directory, in the daemon's namespace: the Windows
/// Downloads folder as WSL sees it (`/mnt/c/Users/you/Downloads`). A Windows
/// folder rather than one inside the VM, so downloads survive the distro being
/// unregistered and Explorer opens them without the slow 9p share.
#[cfg(target_os = "windows")]
#[must_use]
pub fn default_save_path() -> String {
    let windows = home_dir().join("Downloads");
    crate::wsl::to_wsl(&windows).unwrap_or_else(|| windows.to_string_lossy().into_owned())
}

/// True when the transport points at the local machine.
///
/// Gates delete-data, reveal-in-Finder and free space, which only make sense
/// when the daemon's files are on this host. Mirrors
/// `src-tauri/src/settings.rs::is_localhost`.
#[must_use]
pub fn is_localhost(transport: &Transport) -> bool {
    match transport {
        Transport::UnixSocket { .. } => true,
        Transport::Tcp { host, .. } => matches!(host.as_str(), "127.0.0.1" | "::1" | "localhost"),
        // A remote daemon's files aren't on this machine.
        Transport::Http { url, .. } => rtorrent_core::rtorrent::http::host_is_local(url),
    }
}

/// Human-readable endpoint string for the connection UI.
#[must_use]
pub fn endpoint_label(transport: &Transport) -> String {
    match transport {
        Transport::UnixSocket { path } => format!("unix:{path}"),
        Transport::Tcp { host, port } => format!("tcp:{host}:{port}"),
        Transport::Http { url, .. } => strip_userinfo(url),
    }
}

fn strip_userinfo(url: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (Some(s), r),
        None => (None, url),
    };
    let path_start = rest.find('/').unwrap_or(rest.len());
    let cleaned = match rest[..path_start].rfind('@') {
        Some(at) => format!("{}{}", &rest[at + 1..path_start], &rest[path_start..]),
        None => rest.to_owned(),
    };
    match scheme {
        Some(s) => format!("{s}://{cleaned}"),
        None => cleaned,
    }
}

/// A JSON settings file at a caller-selected path. Saves are atomic: the
/// complete temporary file is written and synced before it replaces the final
/// path, so a crash cannot leave a half-written document behind.
#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The standard path: the same `com.rstorrent.app/settings.json` the
    /// Tauri shell uses, so both shells share one file.
    #[must_use]
    pub fn app_default() -> Self {
        Self::new(default_path())
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where the persisted all-time transfer counters live: `stats.json` beside
    /// the settings file, the same name and place the Tauri shell uses.
    #[must_use]
    pub fn stats_path(&self) -> PathBuf {
        self.path
            .parent()
            .map(|parent| parent.join("stats.json"))
            .unwrap_or_else(|| PathBuf::from("stats.json"))
    }

    /// Where the session-import journal lives (V3-22): `import-journal.json`
    /// beside the settings file, the same name and place the Tauri shell
    /// uses, so either shell can resume the other's interrupted import.
    #[must_use]
    pub fn import_journal_path(&self) -> PathBuf {
        self.path
            .parent()
            .map(|parent| parent.join("import-journal.json"))
            .unwrap_or_else(|| PathBuf::from("import-journal.json"))
    }

    /// Load settings, falling back to defaults when the file is missing or
    /// unreadable. A corrupt file never bricks the app.
    #[must_use]
    pub fn load(&self) -> Settings {
        let loaded: Settings = fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        migrate(loaded)
    }

    /// Persist settings, creating the parent directory as needed.
    ///
    /// # Errors
    ///
    /// Returns the I/O or encode error when the file cannot be written.
    pub fn save(&self, settings: &Settings) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(settings)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let temporary = temporary_path(&self.path);
        let result = (|| {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

/// One-time migration: fold a legacy single `watch_folder` into the
/// `watch_folders` list, so the rest of the app reasons about the list only.
fn migrate(mut settings: Settings) -> Settings {
    if settings.watch_folders.is_empty() && !settings.watch_folder.is_empty() {
        settings.watch_folders.push(WatchFolder {
            path: std::mem::take(&mut settings.watch_folder),
            label: String::new(),
            save_path: String::new(),
        });
    }
    settings
}

fn default_path() -> PathBuf {
    if cfg!(target_os = "macos") {
        home_dir().join("Library/Application Support/com.rstorrent.app/settings.json")
    } else if cfg!(target_os = "windows") {
        home_dir().join("AppData/Roaming/com.rstorrent.app/settings.json")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".config"))
            .join("com.rstorrent.app/settings.json")
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(".tmp-{}-{stamp}", std::process::id()));
    PathBuf::from(temporary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_counters_live_beside_the_settings_file() {
        let store = SettingsStore::new("/tmp/rstorrent-settings/settings.json");
        assert_eq!(
            store.stats_path(),
            std::path::PathBuf::from("/tmp/rstorrent-settings/stats.json")
        );
    }

    #[test]
    fn missing_file_loads_defaults() {
        let path = std::env::temp_dir().join(format!(
            "rstorrent-gpui-missing-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let loaded = SettingsStore::new(&path).load();
        assert_eq!(loaded.poll_ms, 1000);
        assert_eq!(loaded.port_range, "6881-6899");
    }

    #[test]
    fn old_settings_without_new_keys_still_load() {
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("torrentThrottles");
        object.remove("filter");
        object.remove("maxActiveUploads");
        object.remove("maxActiveTorrents");
        object.remove("queueSlowLimitKbs");
        object.remove("bandwidthRules");
        let loaded: Settings = serde_json::from_value(value).unwrap();
        assert!(loaded.torrent_throttles.is_empty());
        assert!(loaded.filter.is_empty());
        assert_eq!(loaded.max_active_uploads, 0);
        assert_eq!(loaded.max_active_torrents, 0);
        assert_eq!(loaded.queue_slow_limit_kbs, 0);
        assert!(loaded.bandwidth_rules.is_empty());
    }

    #[test]
    fn old_rss_rules_without_v3_filters_still_load() {
        let rule: crate::settings::RssRule = serde_json::from_value(serde_json::json!({
            "id": "r",
            "name": "r",
            "enabled": true,
            "feedId": "",
            "mustContain": "ubuntu",
            "mustNotContain": "",
            "label": "iso",
            "savePath": "/dl"
        }))
        .unwrap();
        assert_eq!(rule.must_contain, "ubuntu");
        assert!(rule.start, "adds start by default");
    }

    #[test]
    fn tauri_settings_file_loads_verbatim() {
        let dir = std::env::temp_dir().join(format!(
            "rstorrent-gpui-compat-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let fixture = serde_json::json!({
            "transport": {"kind": "tcp", "host": "127.0.0.1", "port": 5000},
            "pollMs": 2000,
            "mock": true,
        });
        fs::write(&path, serde_json::to_string(&fixture).unwrap()).unwrap();
        let loaded = SettingsStore::new(&path).load();
        assert_eq!(loaded.poll_ms, 2000);
        assert!(loaded.mock);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_trip_preserves_connection_profiles() {
        let dir = std::env::temp_dir().join(format!(
            "rstorrent-gpui-roundtrip-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let store = SettingsStore::new(dir.join("settings.json"));
        let settings = Settings {
            connection_profiles: vec![ConnectionProfile {
                name: "seedbox".into(),
                transport: Transport::Tcp {
                    host: "10.0.0.5".into(),
                    port: 5000,
                },
            }],
            filter: "status:downloading".into(),
            ..Settings::default()
        };
        store.save(&settings).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.connection_profiles.len(), 1);
        assert_eq!(loaded.filter, "status:downloading");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn endpoint_label_never_shows_credentials() {
        assert_eq!(
            endpoint_label(&Transport::Http {
                url: "https://user:hunter2@box.example/RPC2".into(),
                username: String::new(),
            }),
            "https://box.example/RPC2"
        );
    }

    #[test]
    fn legacy_watch_folder_migrates_into_the_list() {
        let settings = Settings {
            watch_folder: "/watch".into(),
            ..Settings::default()
        };
        let migrated = migrate(settings);
        assert!(migrated.watch_folder.is_empty());
        assert_eq!(migrated.watch_folders.len(), 1);
        assert_eq!(migrated.watch_folders[0].path, "/watch");
    }

    #[test]
    fn only_a_local_daemon_counts_as_local() {
        assert!(is_localhost(&Transport::UnixSocket {
            path: "/tmp/rpc.socket".into()
        }));
        assert!(is_localhost(&Transport::Tcp {
            host: "127.0.0.1".into(),
            port: 5000
        }));
        assert!(is_localhost(&Transport::Tcp {
            host: "localhost".into(),
            port: 5000
        }));
        assert!(!is_localhost(&Transport::Tcp {
            host: "10.0.0.5".into(),
            port: 5000
        }));
        assert!(!is_localhost(&Transport::Http {
            url: "https://seedbox.example/RPC2".into(),
            username: String::new(),
        }));
    }

    #[test]
    fn column_prefs_survive_a_round_trip_through_the_file() {
        let dir = std::env::temp_dir().join(format!(
            "rstorrent-gpui-columns-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let store = SettingsStore::new(dir.join("settings.json"));
        let mut state = crate::columns::ColumnState::default();
        state.resize(crate::columns::ColumnId::Tracker, 180.);
        state.set_visible(crate::columns::ColumnId::Started, true);
        let settings = Settings {
            columns: state.prefs(),
            ..Settings::default()
        };
        store.save(&settings).unwrap();

        let loaded = store.load();
        let restored = crate::columns::ColumnState::from_prefs(&loaded.columns);
        assert_eq!(restored, state);
        let _ = fs::remove_dir_all(&dir);
    }
}
