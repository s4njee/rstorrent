//! App settings: defaults, JSON persistence, and small helpers.
//!
//! Settings are stored as a single JSON file in the app config directory so the
//! Rust poller can read them directly (the frontend edits them through the
//! `get_settings`/`apply_settings` commands). We keep our own file rather than
//! going through the store plugin so the backend has no async dependency on the
//! WebView being alive.

use std::path::{Path, PathBuf};

use crate::ipc::{Settings, Transport};

impl Default for Settings {
    fn default() -> Self {
        let home = home_dir();
        Settings {
            // Unix socket is the safest default (no network exposure).
            transport: Transport::UnixSocket {
                path: home
                    .join(".rtorrent")
                    .join("rpc.socket")
                    .to_string_lossy()
                    .into_owned(),
            },
            poll_ms: 1000,
            stall_window_s: 30,
            default_save_path: home.join("Downloads").to_string_lossy().into_owned(),
            show_add_dialog: true,
            confirm_on_remove: true,
            down_limit_kb: 0,
            up_limit_kb: 0,
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
    }
}

/// Human-readable endpoint string for the connection UI.
pub fn endpoint_label(transport: &Transport) -> String {
    match transport {
        Transport::UnixSocket { path } => format!("unix:{path}"),
        Transport::Tcp { host, port } => format!("tcp:{host}:{port}"),
    }
}

/// Best-effort home directory (`$HOME`, else `/`).
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
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
    fn localhost_detection() {
        assert!(is_localhost(&Transport::UnixSocket { path: "/x".into() }));
        assert!(is_localhost(&Transport::Tcp { host: "127.0.0.1".into(), port: 5000 }));
        assert!(!is_localhost(&Transport::Tcp { host: "10.0.0.5".into(), port: 5000 }));
    }
}
