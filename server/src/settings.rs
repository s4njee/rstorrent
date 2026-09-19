//! The settings surface's data.
//!
//! Two halves:
//!
//! - [`DAEMON_KEYS`] — the rtorrent variables the web console reads (`config_get`)
//!   and writes (`config_set`), with the section, control kind, unit and hint the
//!   Settings route renders. One table, so a key is described in exactly one place.
//! - [`UiSettings`] — the browser preferences the *server* owns (theme, accent,
//!   density, date/rate format, column layout), persisted to a JSON file beside
//!   the config so a seedbox's console looks the same from any browser.
//!
//! Validation lives here and is pure, so the HTTP layer and the tests agree on
//! what a legal value is.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A Settings-route section, in the design's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    General,
    Connection,
    Bandwidth,
    Queue,
    Directories,
    Labels,
    Interface,
    Advanced,
}

/// What kind of control a key takes, and therefore how it validates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Int {
        unit: &'static str,
        min: i64,
        max: i64,
    },
    Str {
        max_len: usize,
    },
    Choice(&'static [&'static str]),
    Bool,
    ReadOnly,
}

/// The JSON shape of [`Kind`], so the client can pick a control.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum KindDesc {
    Int {
        unit: &'static str,
        min: i64,
        max: i64,
    },
    Str {
        max_len: usize,
    },
    Choice {
        options: &'static [&'static str],
    },
    Bool,
    ReadOnly,
}

impl Kind {
    fn describe(self) -> KindDesc {
        match self {
            Kind::Int { unit, min, max } => KindDesc::Int { unit, min, max },
            Kind::Str { max_len } => KindDesc::Str { max_len },
            Kind::Choice(options) => KindDesc::Choice { options },
            Kind::Bool => KindDesc::Bool,
            Kind::ReadOnly => KindDesc::ReadOnly,
        }
    }

    /// Whether a value is acceptable for this kind. Empty string means "clear"
    /// and is legal for string kinds (the daemon takes an empty target to reset).
    ///
    /// # Errors
    ///
    /// Returns a message naming the failing rule, for inline display.
    pub fn validate(self, value: &str) -> Result<(), String> {
        let value = value.trim();
        match self {
            Kind::Int { min, max, .. } => {
                let number: i64 = value
                    .parse()
                    .map_err(|_| format!("'{value}' is not a whole number"))?;
                if number < min || number > max {
                    return Err(format!("must be between {min} and {max}"));
                }
                Ok(())
            }
            Kind::Str { max_len } => {
                if value.len() > max_len {
                    return Err(format!("must be at most {max_len} characters"));
                }
                Ok(())
            }
            Kind::Choice(options) => {
                if options.contains(&value) {
                    Ok(())
                } else {
                    Err(format!("must be one of {}", options.join(", ")))
                }
            }
            Kind::Bool => match value {
                "0" | "1" | "false" | "true" => Ok(()),
                _ => Err("must be 0 or 1".to_owned()),
            },
            Kind::ReadOnly => Err("this setting is read-only".to_owned()),
        }
    }
}

/// One daemon config key the Settings route shows.
pub struct DaemonKey {
    /// Stable id the client posts back; never the rtorrent name, so a renamed
    /// variable does not break a saved form.
    pub id: &'static str,
    pub label: &'static str,
    /// The rtorrent variable name: read by calling it, written via `<key>.set`.
    pub key: &'static str,
    pub section: Section,
    pub kind: Kind,
    pub hint: &'static str,
}

const CHOICE_DHT: &[&str] = &["auto", "disable"];

/// The one table of daemon keys. Order is presentation order within a section.
pub const DAEMON_KEYS: &[DaemonKey] = &[
    // Connection
    DaemonKey {
        id: "port_range",
        label: "Listen port range",
        key: "network.port_range",
        section: Section::Connection,
        kind: Kind::Str { max_len: 32 },
        hint: "e.g. 6881-6899",
    },
    DaemonKey {
        id: "max_open_files",
        label: "Max open files",
        key: "network.max_open_files",
        section: Section::Connection,
        kind: Kind::Int {
            unit: "",
            min: 0,
            max: 1_000_000,
        },
        hint: "0 = the daemon's default",
    },
    DaemonKey {
        id: "max_open_sockets",
        label: "Max open sockets",
        key: "network.max_open_sockets",
        section: Section::Connection,
        kind: Kind::Int {
            unit: "",
            min: 0,
            max: 1_000_000,
        },
        hint: "",
    },
    DaemonKey {
        id: "http_max_open",
        label: "HTTP max open",
        key: "network.http.max_open",
        section: Section::Connection,
        kind: Kind::Int {
            unit: "",
            min: 0,
            max: 1_000_000,
        },
        hint: "",
    },
    DaemonKey {
        id: "bind_address",
        label: "Bind address",
        key: "network.bind_address",
        section: Section::Connection,
        kind: Kind::Str { max_len: 128 },
        hint: "(all interfaces) — takes effect after a daemon restart",
    },
    DaemonKey {
        id: "proxy_address",
        label: "Tracker HTTP proxy",
        key: "network.http.proxy_address",
        section: Section::Connection,
        kind: Kind::Str { max_len: 128 },
        hint: "host:port; only HTTP announces are proxied",
    },
    DaemonKey {
        id: "pex",
        label: "Peer exchange (PEX)",
        key: "protocol.pex",
        section: Section::Connection,
        kind: Kind::Bool,
        hint: "",
    },
    DaemonKey {
        id: "encryption",
        label: "Protocol encryption",
        key: "protocol.encryption",
        section: Section::Connection,
        kind: Kind::Str { max_len: 128 },
        hint: "rtorrent flag list, e.g. allow_incoming,try_outgoing",
    },
    DaemonKey {
        id: "dht_mode",
        label: "DHT",
        key: "dht.mode",
        section: Section::Connection,
        kind: Kind::Choice(CHOICE_DHT),
        hint: "",
    },
    // Bandwidth
    DaemonKey {
        id: "down_rate",
        label: "Global download limit",
        key: "throttle.global_down.max_rate",
        section: Section::Bandwidth,
        kind: Kind::Int {
            unit: "B/s",
            min: 0,
            max: i64::MAX,
        },
        hint: "0 = unlimited",
    },
    DaemonKey {
        id: "up_rate",
        label: "Global upload limit",
        key: "throttle.global_up.max_rate",
        section: Section::Bandwidth,
        kind: Kind::Int {
            unit: "B/s",
            min: 0,
            max: i64::MAX,
        },
        hint: "0 = unlimited",
    },
    DaemonKey {
        id: "max_peers_normal",
        label: "Max peers (leeching)",
        key: "throttle.max_peers.normal",
        section: Section::Bandwidth,
        kind: Kind::Int {
            unit: "",
            min: 0,
            max: 100_000,
        },
        hint: "0 = the daemon's default",
    },
    DaemonKey {
        id: "max_peers_seed",
        label: "Max peers (seeding)",
        key: "throttle.max_peers.seed",
        section: Section::Bandwidth,
        kind: Kind::Int {
            unit: "",
            min: 0,
            max: 100_000,
        },
        hint: "",
    },
    DaemonKey {
        id: "memory_max",
        label: "Piece cache size",
        key: "pieces.memory.max",
        section: Section::Bandwidth,
        kind: Kind::Int {
            unit: "B",
            min: 0,
            max: i64::MAX,
        },
        hint: "0 = the daemon's default",
    },
    // Queue
    DaemonKey {
        id: "max_uploads_global",
        label: "Global upload slots",
        key: "throttle.max_uploads.global",
        section: Section::Queue,
        kind: Kind::Int {
            unit: "",
            min: 0,
            max: 100_000,
        },
        hint: "",
    },
    DaemonKey {
        id: "max_downloads_global",
        label: "Global download slots",
        key: "throttle.max_downloads.global",
        section: Section::Queue,
        kind: Kind::Int {
            unit: "",
            min: 0,
            max: 100_000,
        },
        hint: "",
    },
    // Directories
    DaemonKey {
        id: "session_path",
        label: "Session path",
        key: "session.path",
        section: Section::Directories,
        kind: Kind::ReadOnly,
        hint: "set in .rtorrent.rc",
    },
];

/// Look a key up by its stable id.
#[must_use]
pub fn daemon_key(id: &str) -> Option<&'static DaemonKey> {
    DAEMON_KEYS.iter().find(|key| key.id == id)
}

/// The `uint8`-style JSON for one key plus its current value, as `/api/config`
/// returns it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigRow {
    pub id: &'static str,
    pub label: &'static str,
    pub key: &'static str,
    pub section: Section,
    pub kind: KindDesc,
    pub hint: &'static str,
    /// The daemon's current value, or `None` when the build does not expose it.
    pub value: Option<String>,
    pub available: bool,
}

impl DaemonKey {
    #[must_use]
    pub fn row(&self, value: Option<String>) -> ConfigRow {
        ConfigRow {
            id: self.id,
            label: self.label,
            key: self.key,
            section: self.section,
            kind: self.kind.describe(),
            hint: self.hint,
            available: value.is_some(),
            value,
        }
    }
}

// --- Interface preferences (server-owned) ------------------------------------

/// Themes the console ships, mirroring `src/theme/themes/*`.
pub const THEMES: &[&str] = &["system", "dark", "light", "midnight", "contrast", "classic"];
/// Row densities, mirroring `tokens.css`'s `data-density`.
pub const DENSITIES: &[&str] = &["dense", "comfortable"];

/// The browser preferences the server persists and injects into the SPA.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UiSettings {
    pub theme: Option<String>,
    /// A `#rrggbb` accent, or empty for the theme's default.
    pub accent: Option<String>,
    pub density: Option<String>,
    pub date_format: Option<String>,
    pub rate_format: Option<String>,
    /// Opaque column visibility/order, validated only as JSON.
    pub columns: Option<serde_json::Value>,
}

impl UiSettings {
    /// Reject values the console cannot apply, naming the failing field.
    ///
    /// # Errors
    ///
    /// Returns a message for the first invalid field.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(theme) = &self.theme {
            if !THEMES.contains(&theme.as_str()) {
                return Err(format!("theme must be one of {}", THEMES.join(", ")));
            }
        }
        if let Some(density) = &self.density {
            if !DENSITIES.contains(&density.as_str()) {
                return Err(format!("density must be one of {}", DENSITIES.join(", ")));
            }
        }
        if let Some(accent) = &self.accent {
            if !accent.is_empty() && !is_hex_colour(accent) {
                return Err("accent must be #rrggbb or empty".to_owned());
            }
        }
        Ok(())
    }
}

/// `#rgb` / `#rrggbb`, the only accent form the token layer accepts.
fn is_hex_colour(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

/// The UI-settings JSON file: `ui-settings.json` beside the server config.
#[derive(Clone, Debug)]
pub struct UiSettingsStore {
    path: PathBuf,
}

impl UiSettingsStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The store beside a config file (or in the CWD when there is no config).
    #[must_use]
    pub fn beside(config_path: Option<&Path>) -> Self {
        let path = config_path.and_then(Path::parent).map_or_else(
            || PathBuf::from("ui-settings.json"),
            |dir| dir.join("ui-settings.json"),
        );
        Self::new(path)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load, falling back to defaults for a missing or corrupt file.
    #[must_use]
    pub fn load(&self) -> UiSettings {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Persist atomically enough for a settings file (write then rename).
    ///
    /// # Errors
    ///
    /// Returns the I/O or encode error when it cannot be written.
    pub fn save(&self, settings: &UiSettings) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let text = serde_json::to_string_pretty(settings)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, &self.path)
    }
}

// --- The managed `.rtorrent.rc` block ----------------------------------------

/// The 1 Gbps tuner's managed block markers (kept identical to
/// `src-tauri/src/rtorrent_rc.rs`).
pub const BLOCK_START: &str = "# >>> rstorrent: 1 Gbps tuning >>>";
pub const BLOCK_END: &str = "# <<< rstorrent: 1 Gbps tuning <<<";

/// The managed block from an `.rtorrent.rc`, markers included, or `None` when
/// the file carries none (so the surface never invents one).
#[must_use]
pub fn managed_block(text: &str) -> Option<String> {
    let start = text.find(BLOCK_START)?;
    let from_start = &text[start..];
    let end = from_start.find(BLOCK_END)? + BLOCK_END.len();
    Some(from_start[..end].to_owned())
}

/// Set `password_hash` in an `[auth]` section, preserving every other line.
///
/// Returns the rewritten document. Set `hash` to an empty string to remove the
/// line (used when switching to `mode = "none"`). A file without an `[auth]`
/// section gains one; a section without the key gains the key, right after the
/// header.
#[must_use]
pub fn set_password_hash_in_toml(text: &str, hash: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let auth = lines.iter().position(|line| line.trim() == "[auth]");
    let assignment = format!("password_hash = \"{hash}\"");

    if hash.is_empty() {
        if let Some(start) = auth {
            if let Some(index) = find_key(&lines, start) {
                lines.remove(index);
            }
        }
        return finish(lines, text);
    }

    match auth {
        Some(start) => match find_key(&lines, start) {
            Some(index) => lines[index] = assignment,
            None => lines.insert(start + 1, assignment),
        },
        None => {
            lines.push(String::new());
            lines.push("[auth]".to_owned());
            lines.push(assignment);
        }
    }
    finish(lines, text)
}

/// The index of a `password_hash = …` line at or after a `[auth]` header (and
/// before the next section).
fn find_key(lines: &[String], auth: usize) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(auth + 1)
        .take_while(|(_, line)| !line.trim_start().starts_with('['))
        .find(|(_, line)| line.trim_start().starts_with("password_hash"))
        .map(|(index, _)| index)
}

/// Rejoin preserving the original trailing-newline habit.
fn finish(lines: Vec<String>, original: &str) -> String {
    let mut out = lines.join("\n");
    if original.ends_with('\n') || original.is_empty() {
        out.push('\n');
    }
    out
}

/// Rewrite the password hash in the config file (used after a verified change).
///
/// # Errors
///
/// Returns the I/O error when the file cannot be read or written.
pub fn write_password_hash(path: &Path, hash: &str) -> std::io::Result<()> {
    let text = std::fs::read_to_string(path)?;
    let updated = set_password_hash_in_toml(&text, hash);
    std::fs::write(path, updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_kinds_bound_their_values() {
        let kind = Kind::Int {
            unit: "",
            min: 0,
            max: 100,
        };
        assert!(kind.validate("0").is_ok());
        assert!(kind.validate("100").is_ok());
        assert!(kind.validate("101").is_err());
        assert!(kind.validate("-1").is_err());
        assert!(kind.validate("fast").is_err());
    }

    #[test]
    fn choice_and_bool_kinds_are_strict() {
        let choice = Kind::Choice(CHOICE_DHT);
        assert!(choice.validate("auto").is_ok());
        assert!(choice.validate("maybe").is_err());
        let bool_kind = Kind::Bool;
        assert!(bool_kind.validate("1").is_ok());
        assert!(bool_kind.validate("0").is_ok());
        assert!(bool_kind.validate("2").is_err());
    }

    #[test]
    fn read_only_keys_refuse_every_write() {
        assert!(Kind::ReadOnly.validate("").is_err());
    }

    #[test]
    fn ids_are_unique_and_looked_up_by_id() {
        let mut ids: Vec<&str> = DAEMON_KEYS.iter().map(|k| k.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "ids must be unique");
        assert_eq!(daemon_key("port_range").unwrap().key, "network.port_range");
        assert!(daemon_key("nope").is_none());
    }

    #[test]
    fn ui_settings_reject_unknown_themes_and_bad_accents() {
        let ok = UiSettings {
            theme: Some("midnight".into()),
            accent: Some("#f59e0b".into()),
            density: Some("comfortable".into()),
            ..Default::default()
        };
        assert!(ok.validate().is_ok());

        let bad_theme = UiSettings {
            theme: Some("solarized".into()),
            ..Default::default()
        };
        assert!(bad_theme.validate().is_err());

        let bad_accent = UiSettings {
            accent: Some("amber".into()),
            ..Default::default()
        };
        assert!(bad_accent.validate().is_err());

        // An empty accent is the "theme default" signal, not an error.
        let cleared = UiSettings {
            accent: Some(String::new()),
            ..Default::default()
        };
        assert!(cleared.validate().is_ok());
    }

    #[test]
    fn ui_settings_round_trip_through_the_store() {
        let dir = std::env::temp_dir().join(format!("rstorrent-web-ui-{}", std::process::id()));
        let store = UiSettingsStore::new(dir.join("ui-settings.json"));
        let settings = UiSettings {
            theme: Some("light".into()),
            density: Some("dense".into()),
            ..Default::default()
        };
        store.save(&settings).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.theme.as_deref(), Some("light"));
        assert_eq!(loaded.density.as_deref(), Some("dense"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn managed_block_is_extracted_or_reported_absent() {
        let text = format!(
            "a = 1\n{BLOCK_START}\nthrottle.global_down.max_rate = 0\n{BLOCK_END}\nb = 2\n"
        );
        let block = managed_block(&text).unwrap();
        assert!(block.starts_with(BLOCK_START));
        assert!(block.ends_with(BLOCK_END));
        assert!(!block.contains("a = 1"), "only the block is returned");
        assert!(managed_block("a = 1\n").is_none());
    }

    #[test]
    fn password_hash_line_is_replaced_in_place() {
        let text =
            "[auth]\nmode = \"password\"\npassword_hash = \"old\"\n\n[ui]\ndisplay_name = \"rt\"\n";
        let updated = set_password_hash_in_toml(text, "new");
        assert!(updated.contains("password_hash = \"new\""));
        assert!(!updated.contains("\"old\""));
        assert!(updated.contains("mode = \"password\""));
        assert!(updated.contains("[ui]"));
        assert!(updated.contains("display_name = \"rt\""));
    }

    #[test]
    fn password_hash_is_inserted_into_an_existing_auth_section() {
        let text = "[auth]\nmode = \"password\"\n";
        let updated = set_password_hash_in_toml(text, "s3cret");
        assert_eq!(
            updated,
            "[auth]\npassword_hash = \"s3cret\"\nmode = \"password\"\n"
        );
    }

    #[test]
    fn password_hash_section_is_appended_when_absent() {
        let text = "listen = \"127.0.0.1:9080\"\n";
        let updated = set_password_hash_in_toml(text, "s3cret");
        assert!(updated.contains("[auth]"));
        assert!(updated.contains("password_hash = \"s3cret\""));
        assert!(updated.contains("listen ="));
    }

    #[test]
    fn an_empty_hash_removes_the_line() {
        let text = "[auth]\nmode = \"none\"\npassword_hash = \"old\"\n";
        let updated = set_password_hash_in_toml(text, "");
        assert!(!updated.contains("password_hash"));
        assert!(updated.contains("mode = \"none\""));
    }
}
