//! App settings: defaults, JSON persistence, and small helpers.
//!
//! Settings are stored as a single JSON file in the app config directory so the
//! Rust poller can read them directly (the frontend edits them through the
//! `get_settings`/`apply_settings` commands). We keep our own file rather than
//! going through the store plugin so the backend has no async dependency on the
//! WebView being alive.

use std::path::{Path, PathBuf};

use crate::ipc::{SeedGoal, Settings, Transport};

impl Default for Settings {
    fn default() -> Self {
        Settings {
            transport: default_transport(),
            poll_ms: 1000,
            stall_window_s: 30,
            default_save_path: default_save_path(),
            show_add_dialog: true,
            confirm_on_remove: true,
            down_limit_kb: 0,
            up_limit_kb: 0,
            port_range: "6881-6899".to_string(),
            dht_enabled: false,
            watch_folder: String::new(),
            completion_notification_excluded_labels: Vec::new(),
            torrent_throttles: Vec::new(),
            global_seed_goal: SeedGoal::default(),
            label_seed_goals: Vec::new(),
            // Honour the env var so `RSTORRENT_MOCK=1` flips the default on.
            mock: std::env::var("RSTORRENT_MOCK").is_ok(),
        }
    }
}

/// Load settings from `path`, falling back to defaults if it's missing or
/// unreadable (a corrupt file should never brick the app).
pub fn load(path: &Path) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

/// Persist settings to `path`, creating the parent directory as needed.
pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(settings)?;
    std::fs::write(path, text)
}

/// Merge a partial JSON patch into `current`, returning the updated settings.
/// Unknown/missing keys keep their current value.
pub fn apply_patch(current: &Settings, patch: serde_json::Value) -> Settings {
    // Round-trip through JSON so only the provided fields override.
    let mut base = serde_json::to_value(current).unwrap_or(serde_json::Value::Null);
    if let (Some(base_obj), Some(patch_obj)) = (base.as_object_mut(), patch.as_object()) {
        for (k, v) in patch_obj {
            base_obj.insert(k.clone(), v.clone());
        }
    }
    serde_json::from_value(base).unwrap_or_else(|_| current.clone())
}

/// True when the transport points at the local machine (gates delete-data /
/// reveal-in-Finder / free-space, which only make sense on localhost).
pub fn is_localhost(transport: &Transport) -> bool {
    match transport {
        Transport::UnixSocket { .. } => true,
        Transport::Tcp { host, .. } => {
            matches!(host.as_str(), "127.0.0.1" | "::1" | "localhost")
        }
        // A remote daemon's files aren't on this machine, so delete-data,
        // reveal-in-Finder and free-space must stay disabled for it.
        Transport::Http { url, .. } => crate::rtorrent::http::host_is_local(url),
    }
}

/// Human-readable endpoint string for the connection UI.
pub fn endpoint_label(transport: &Transport) -> String {
    match transport {
        Transport::UnixSocket { path } => format!("unix:{path}"),
        Transport::Tcp { host, port } => format!("tcp:{host}:{port}"),
        Transport::Http { url, .. } => strip_userinfo(url),
    }
}

/// Remove any `user:pass@` from a URL before it is displayed or logged.
///
/// Nothing stops a user pasting `https://user:hunter2@box/RPC2` into the URL
/// field, and this string reaches the disconnected card and the app log — so
/// strip credentials rather than broadcast them.
fn strip_userinfo(url: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (Some(s), r),
        None => (None, url),
    };
    // Userinfo ends at the last '@' before the first '/' of the path.
    let path_start = rest.find('/').unwrap_or(rest.len());
    let cleaned = match rest[..path_start].rfind('@') {
        Some(at) => format!("{}{}", &rest[at + 1..path_start], &rest[path_start..]),
        None => rest.to_string(),
    };
    match scheme {
        Some(s) => format!("{s}://{cleaned}"),
        None => cleaned,
    }
}

/// Best-effort home directory (`$HOME`, or `%USERPROFILE%` on Windows).
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" }))
}

/// Default transport for a fresh install.
///
/// Unix socket is the safest default where it works — SCGI is unauthenticated,
/// so keeping it off the network entirely is the right posture. Windows can't
/// reach a socket inside the WSL VM, so it falls back to loopback TCP, which
/// WSL's `localhostForwarding` bridges into the VM without exposing the port
/// to the LAN.
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
        host: "127.0.0.1".to_string(),
        port: 5000,
    }
}

#[cfg(not(target_os = "windows"))]
fn default_save_path() -> String {
    home_dir().join("Downloads").to_string_lossy().into_owned()
}

/// On Windows the daemon writes files from *inside* WSL, so this has to be a
/// Linux path. Prefer the distro's own home (ext4 — full speed); if WSL can't
/// be probed, fall back to the Windows Downloads folder as seen through
/// `/mnt/`, which is slower but always valid and always where the user looks.
#[cfg(target_os = "windows")]
fn default_save_path() -> String {
    if let Some(distro) = crate::wsl::distro() {
        return format!("{}/Downloads", distro.home.trim_end_matches('/'));
    }
    crate::wsl::to_wsl(&home_dir().join("Downloads")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_overrides_only_given_fields() {
        let s = Settings::default();
        let patched = apply_patch(&s, serde_json::json!({ "pollMs": 2000 }));
        assert_eq!(patched.poll_ms, 2000);
        // Untouched field preserved.
        assert_eq!(patched.stall_window_s, s.stall_window_s);
    }

    #[test]
    fn old_settings_without_torrent_throttles_still_load() {
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value.as_object_mut().unwrap().remove("torrentThrottles");
        let loaded: Settings = serde_json::from_value(value).unwrap();
        assert!(loaded.torrent_throttles.is_empty());
    }

    #[test]
    fn localhost_detection() {
        assert!(is_localhost(&Transport::UnixSocket { path: "/x".into() }));
        assert!(is_localhost(&Transport::Tcp {
            host: "127.0.0.1".into(),
            port: 5000
        }));
        assert!(!is_localhost(&Transport::Tcp {
            host: "10.0.0.5".into(),
            port: 5000
        }));
        // A remote HTTP daemon is not local; a bridge on this machine is.
        assert!(!is_localhost(&Transport::Http {
            url: "https://seedbox.example.com/RPC2".into(),
            username: "alice".into(),
        }));
        assert!(is_localhost(&Transport::Http {
            url: "http://127.0.0.1:8080/RPC2".into(),
            username: String::new(),
        }));
    }

    #[test]
    fn endpoint_label_never_shows_credentials() {
        // This string reaches the disconnected card and the app log.
        assert_eq!(
            endpoint_label(&Transport::Http {
                url: "https://user:hunter2@box.example/RPC2".into(),
                username: String::new(),
            }),
            "https://box.example/RPC2"
        );
        assert_eq!(
            endpoint_label(&Transport::Http {
                url: "https://box.example/RPC2".into(),
                username: String::new(),
            }),
            "https://box.example/RPC2"
        );
    }
}

#[cfg(test)]
mod diag {
    use super::*;

    /// Diagnostic: what does the running app actually load from settings.json?
    #[test]
    #[ignore]
    fn live_dump_loaded_settings() {
        // Same location Tauri's `app_config_dir()` resolves to on each platform.
        let path = if cfg!(target_os = "windows") {
            home_dir().join("AppData/Roaming/com.rstorrent.app/settings.json")
        } else {
            home_dir().join("Library/Application Support/com.rstorrent.app/settings.json")
        };
        println!("path exists: {}", path.exists());
        let raw = std::fs::read_to_string(&path).unwrap_or_default();
        // Does it parse at all, or does load() silently fall back to defaults?
        match serde_json::from_str::<Settings>(&raw) {
            Ok(s) => println!("PARSED transport = {:?}", s.transport),
            Err(e) => println!("PARSE ERROR = {e}"),
        }
        let loaded = load(&path);
        println!("load() transport = {:?}", loaded.transport);
        println!("load() mock = {}", loaded.mock);
    }
}
