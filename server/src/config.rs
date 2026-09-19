//! Server configuration: a TOML file, overlaid by `RSTORRENT_WEB_*` env vars,
//! overlaid by CLI flags (flags > env > file > built-in defaults).
//!
//! The resolution is split into a pure [`resolve`] step (file + env + cli →
//! [`Config`]) so precedence is unit-testable without touching the filesystem or
//! the process environment.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use rtorrent_core::complete::{CollisionPolicy, MoveRule};
use rtorrent_core::types::Transport;
use serde::{Deserialize, Serialize};

/// The default listen address — loopback, so a fresh install is not exposed.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:9080";

/// How the server authenticates browser sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    /// Require the configured password (the only safe mode off loopback).
    #[default]
    Password,
    /// No login at all — a loopback-only development escape hatch. Refused on a
    /// non-loopback bind (see [`Config::validate`]).
    None,
}

/// Fully-resolved, validated configuration the server runs against.
#[derive(Debug, Clone)]
pub struct Config {
    pub listen: SocketAddr,
    pub transport: Transport,
    /// Basic-auth password for an HTTP-transport daemon (distinct from the web
    /// login password). Supplied to `RpcClient::with_password`.
    pub daemon_password: Option<String>,
    pub auth_mode: AuthMode,
    /// argon2id PHC string for the web login password; `None` in `mode = none`.
    pub password_hash: Option<String>,
    /// Avatar initials / display name shown in the app bar.
    pub display_name: String,
    /// Volume probed for the disk card and used as the Add dialog default.
    pub save_path: String,
    /// Extra volumes the Stats route reports use for (WC8-S5). A path that does
    /// not exist is reported unavailable, never as empty.
    pub volumes: Vec<String>,
    /// "Keep incomplete torrents in" (V3-14 / LIB-08). Empty = disabled: new
    /// downloads go straight to their chosen path.
    pub incomplete_dir: String,
    /// Destination rules for completed data, by tag or label (V3-14 / LIB-07).
    pub move_rules: Vec<MoveRule>,
    /// What to do when a move destination is already taken (V3-14).
    pub collision_policy: CollisionPolicy,
    /// Queue caps (V3-17 / QUE-01). All zero = unmanaged.
    pub max_active_downloads: i64,
    pub max_active_uploads: i64,
    pub max_active_torrents: i64,
    /// Slow-torrent exemption floor, KiB/s (V3-17 / QUE-01). 0 = disabled.
    pub queue_slow_limit_kbs: i64,
    /// Named bandwidth profiles for the precedence display (V3-18 / QUE-04).
    pub bandwidth_rules: Vec<rtorrent_core::bandwidth::BandwidthRule>,
    /// Fast-poll cadence in milliseconds.
    pub poll_ms: u64,
    /// Optional on-disk asset directory (overrides the embedded SPA).
    pub assets_dir: Option<PathBuf>,
    /// Serve the fixture torrents with no daemon (`RSTORRENT_MOCK=1`).
    pub mock: bool,
    /// The config file this run was loaded from, when there was one. The settings
    /// surface writes a password change back here; `None` means in-memory only.
    pub config_path: Option<PathBuf>,
    /// Deployment-hardening knobs (WEB-06).
    pub hardening: Hardening,
}

/// Deployment-hardening knobs (WEB-06).
#[derive(Debug, Clone, Default)]
pub struct Hardening {
    /// Proxy addresses whose `X-Forwarded-For`/`X-Forwarded-Proto` headers are
    /// trusted. Empty means the headers are ignored entirely.
    pub trusted_proxies: Vec<IpAddr>,
    /// Force `Secure` on the session cookie. Set this when TLS terminates at a
    /// proxy you have not listed in `trusted_proxies` but you know is HTTPS.
    pub secure_cookies: bool,
}

impl Config {
    /// True when the listen address is loopback (127.0.0.0/8 or ::1).
    pub fn is_loopback(&self) -> bool {
        self.listen.ip().is_loopback()
    }

    /// Where the hashed session tokens live: `session-store.json` beside the
    /// config, or `None` when no config file was loaded (an in-memory store, so
    /// a short-lived `--listen`-only run does not scribble in the working
    /// directory). Sessions survive a restart only when this is `Some`.
    #[must_use]
    pub fn session_store_path(&self) -> Option<PathBuf> {
        self.config_path
            .as_deref()
            .and_then(Path::parent)
            .map(|dir| dir.join("session-store.json"))
    }

    /// Reject configurations that would expose an unauthenticated daemon, or
    /// send a session cookie that a browser cannot mark `Secure`.
    pub fn validate(&self) -> Result<()> {
        if self.auth_mode == AuthMode::None && !self.is_loopback() {
            return Err(anyhow!(
                "auth.mode = \"none\" is only allowed on a loopback bind; \
                 {} is reachable from the network — set a password_hash",
                self.listen
            ));
        }
        // Mock mode is not an exemption: a server whose every request is refused
        // is no more useful for fixtures than for a daemon, and the refusal used
        // to be visible only in the browser's network tab.
        if self.auth_mode == AuthMode::Password && self.password_hash.is_none() {
            return Err(anyhow!(
                "auth.mode = \"password\" needs an [auth].password_hash — run \
                 `rstorrent-web hash-password` to generate one, or set \
                 auth.mode = \"none\" for a loopback development server"
            ));
        }
        // A non-loopback bind is plain HTTP (TLS terminates in front), so the
        // session cookie can only be marked `Secure` if we can see the forwarded
        // scheme (a trusted proxy) or are told to force it. Without either, the
        // cookie would be sent over HTTP on a downgrade.
        if !self.is_loopback()
            && !self.hardening.secure_cookies
            && self.hardening.trusted_proxies.is_empty()
        {
            return Err(anyhow!(
                "a non-loopback bind ({}) needs secure session cookies: list the \
                 proxy in `trusted_proxies = [\"127.0.0.1\"]` (so its \
                 X-Forwarded-Proto: https is honoured) or set `secure_cookies = true`",
                self.listen
            ));
        }
        Ok(())
    }
}

// --- The on-disk file shape (every field optional) ---------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub listen: Option<String>,
    pub poll_ms: Option<u64>,
    #[serde(default)]
    pub transport: Option<TransportConfig>,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub paths: PathsConfig,
    /// Queue caps (V3-17 / QUE-01).
    #[serde(default)]
    pub queue: QueueFileConfig,
    /// Named bandwidth profiles for the precedence display (V3-18 / QUE-04).
    /// Enforcement stays desktop-side; the server only reports them.
    #[serde(default)]
    pub bandwidth: Vec<BandwidthFileRule>,
    /// Proxy addresses whose forwarded headers are trusted (WEB-06).
    #[serde(default)]
    pub trusted_proxies: Option<Vec<String>>,
    /// Force `Secure` on the session cookie (WEB-06).
    #[serde(default)]
    pub secure_cookies: Option<bool>,
}

/// One bandwidth rule in TOML shape: `{ id, tag?, label?, down_kb, up_kb,
/// peers_max?, peers_min?, uploads_max? }`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BandwidthFileRule {
    pub id: String,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub down_kb: i64,
    #[serde(default)]
    pub up_kb: i64,
    #[serde(default)]
    pub peers_max: i64,
    #[serde(default)]
    pub peers_min: i64,
    #[serde(default)]
    pub uploads_max: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    Unix,
    Tcp,
    Http,
}

/// `[transport]` — friendly `kind = "unix" | "tcp" | "http"` mapped onto the
/// shared [`Transport`] enum. A dedicated shape (rather than deserializing
/// `Transport` directly) keeps the TOML ergonomic and robust.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportConfig {
    pub kind: TransportKind,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub username: String,
    /// Daemon basic-auth password for an HTTP transport.
    #[serde(default)]
    pub password: String,
}

impl TransportConfig {
    fn resolve(self) -> Result<(Transport, Option<String>)> {
        let transport = match self.kind {
            TransportKind::Unix => {
                if self.path.is_empty() {
                    return Err(anyhow!("[transport] kind = \"unix\" needs a `path`"));
                }
                Transport::UnixSocket { path: self.path }
            }
            TransportKind::Tcp => {
                if self.host.is_empty() || self.port == 0 {
                    return Err(anyhow!(
                        "[transport] kind = \"tcp\" needs `host` and `port`"
                    ));
                }
                Transport::Tcp {
                    host: self.host,
                    port: self.port,
                }
            }
            TransportKind::Http => {
                if self.url.is_empty() {
                    return Err(anyhow!("[transport] kind = \"http\" needs a `url`"));
                }
                Transport::Http {
                    url: self.url,
                    username: self.username,
                }
            }
        };
        let password = (!self.password.is_empty()).then_some(self.password);
        Ok((transport, password))
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    pub password_hash: Option<String>,
    #[serde(default)]
    pub mode: AuthMode,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiConfig {
    pub display_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathsConfig {
    pub save_path: Option<String>,
    /// Extra volumes the Stats route reports use.
    #[serde(default)]
    pub volumes: Option<Vec<String>>,
    /// "Keep incomplete torrents in" (V3-14 / LIB-08).
    #[serde(default)]
    pub incomplete_dir: Option<String>,
    /// Destination rules for completed data, by tag or label (V3-14 / LIB-07).
    #[serde(default)]
    pub move_rules: Option<Vec<MoveRule>>,
    /// What to do when a move destination is already taken (V3-14).
    #[serde(default)]
    pub collision_policy: Option<CollisionPolicy>,
}

/// Queue caps (V3-17 / QUE-01): `[queue]` section, all zero = unmanaged.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueFileConfig {
    /// Keep at most this many torrents downloading.
    #[serde(default)]
    pub max_active_downloads: Option<i64>,
    /// Keep at most this many finished torrents seeding.
    #[serde(default)]
    pub max_active_uploads: Option<i64>,
    /// Keep at most this many torrents active in total.
    #[serde(default)]
    pub max_active_torrents: Option<i64>,
    /// Torrents slower than this (down + up, KiB/s) are never held back.
    #[serde(default)]
    pub queue_slow_limit_kbs: Option<i64>,
}

// --- Env + CLI overlays ------------------------------------------------------

/// Values pulled from `RSTORRENT_WEB_*` env vars (and `RSTORRENT_MOCK`).
#[derive(Debug, Default)]
pub struct EnvOverrides {
    pub listen: Option<String>,
    pub display_name: Option<String>,
    pub save_path: Option<String>,
    pub incomplete_dir: Option<String>,
    pub poll_ms: Option<u64>,
    pub mock: bool,
}

impl EnvOverrides {
    /// Read overrides from the process environment.
    pub fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Self {
            listen: var("RSTORRENT_WEB_LISTEN"),
            display_name: var("RSTORRENT_WEB_DISPLAY_NAME"),
            save_path: var("RSTORRENT_WEB_SAVE_PATH"),
            incomplete_dir: var("RSTORRENT_WEB_INCOMPLETE_DIR"),
            poll_ms: var("RSTORRENT_WEB_POLL_MS").and_then(|v| v.parse().ok()),
            mock: std::env::var("RSTORRENT_MOCK").is_ok(),
        }
    }
}

/// Values supplied on the command line (highest precedence).
#[derive(Debug, Default)]
pub struct CliOverrides {
    pub listen: Option<String>,
    pub assets_dir: Option<PathBuf>,
}

/// Pure resolution: overlay file < env < cli and validate. No I/O.
pub fn resolve(file: FileConfig, env: EnvOverrides, cli: CliOverrides) -> Result<Config> {
    let listen_str = cli
        .listen
        .or(env.listen)
        .or(file.listen)
        .unwrap_or_else(|| DEFAULT_LISTEN.to_string());
    let listen: SocketAddr = listen_str
        .parse()
        .with_context(|| format!("invalid listen address: {listen_str}"))?;

    let (transport, daemon_password) = match file.transport {
        Some(t) => t.resolve()?,
        // No `[transport]`: the local socket, which is where a daemon on this
        // machine lives. A run pointed elsewhere says so in the file.
        None => (
            Transport::UnixSocket {
                path: default_socket_path(),
            },
            None,
        ),
    };

    let config = Config {
        listen,
        transport,
        daemon_password,
        auth_mode: file.auth.mode,
        password_hash: file.auth.password_hash,
        display_name: env
            .display_name
            .or(file.ui.display_name)
            .unwrap_or_else(|| "rt".to_string()),
        save_path: env.save_path.or(file.paths.save_path).unwrap_or_default(),
        volumes: file.paths.volumes.unwrap_or_default(),
        incomplete_dir: env
            .incomplete_dir
            .or(file.paths.incomplete_dir)
            .unwrap_or_default(),
        move_rules: file.paths.move_rules.unwrap_or_default(),
        collision_policy: file.paths.collision_policy.unwrap_or_default(),
        max_active_downloads: file.queue.max_active_downloads.unwrap_or_default(),
        max_active_uploads: file.queue.max_active_uploads.unwrap_or_default(),
        max_active_torrents: file.queue.max_active_torrents.unwrap_or_default(),
        queue_slow_limit_kbs: file.queue.queue_slow_limit_kbs.unwrap_or_default(),
        bandwidth_rules: file
            .bandwidth
            .into_iter()
            .map(|r| rtorrent_core::bandwidth::BandwidthRule {
                id: r.id,
                tag: r.tag,
                label: r.label,
                down_kb: r.down_kb,
                up_kb: r.up_kb,
                peers_max: r.peers_max,
                peers_min: r.peers_min,
                uploads_max: r.uploads_max,
            })
            .collect(),
        poll_ms: cli_poll(env.poll_ms, file.poll_ms),
        assets_dir: cli.assets_dir,
        mock: env.mock,
        config_path: None,
        hardening: Hardening {
            trusted_proxies: parse_trusted_proxies(file.trusted_proxies.unwrap_or_default())?,
            secure_cookies: file.secure_cookies.unwrap_or(false),
        },
    };
    config.validate()?;
    Ok(config)
}

/// Parse the `trusted_proxies` list, naming the offending entry on a bad address.
fn parse_trusted_proxies(entries: Vec<String>) -> Result<Vec<IpAddr>> {
    entries
        .into_iter()
        .map(|entry| {
            let trimmed = entry.trim();
            trimmed.parse::<IpAddr>().map_err(|_| {
                anyhow!(
                    "trusted_proxies entry '{trimmed}' is not an IP address; \
                     list the proxy's address, e.g. \"127.0.0.1\""
                )
            })
        })
        .collect()
}

fn cli_poll(env: Option<u64>, file: Option<u64>) -> u64 {
    // Floor at 250ms so a typo can't hammer the daemon.
    env.or(file).unwrap_or(1000).max(250)
}

/// The local daemon's socket, when nothing says otherwise.
///
/// The same path both desktop shells default to. It used to be the container
/// image's `/home/rtorrent/...`, which made a config-less run on someone's own
/// machine point at a socket that cannot exist there and report the daemon
/// unreachable with no hint as to why. A container that needs the image's path
/// says so in its `[transport]`, as the docs' recipe already does.
fn default_socket_path() -> String {
    home_dir()
        .join(".rtorrent")
        .join("rpc.socket")
        .to_string_lossy()
        .into_owned()
}

/// The user's home directory, resolved as the desktop shells resolve it.
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" }))
}

/// Load the config file (if present) and overlay env + CLI. `path` is the
/// `--config` value; when `None`, `rstorrent-web.toml` in the CWD is used if it
/// exists, otherwise defaults + env + CLI stand alone.
pub fn load(path: Option<&Path>, cli: CliOverrides) -> Result<Config> {
    let explicit = path.is_some();
    let file_path = path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("rstorrent-web.toml"));

    let file = if file_path.exists() {
        let text = std::fs::read_to_string(&file_path)
            .with_context(|| format!("reading {}", file_path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", file_path.display()))?
    } else if explicit {
        return Err(anyhow!("config file not found: {}", file_path.display()));
    } else {
        FileConfig::default()
    };

    resolve(file, EnvOverrides::from_env(), cli).map(|mut config| {
        // Remember where the file was, so a settings change can write back to it.
        config.config_path = file_path.exists().then_some(file_path);
        config
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_with_listen(listen: &str) -> FileConfig {
        FileConfig {
            listen: Some(listen.to_string()),
            auth: AuthConfig {
                mode: AuthMode::None,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn precedence_is_cli_over_env_over_file() {
        // File says :9001, env says :9002, cli says :9003 → cli wins.
        let cfg = resolve(
            file_with_listen("127.0.0.1:9001"),
            EnvOverrides {
                listen: Some("127.0.0.1:9002".into()),
                ..Default::default()
            },
            CliOverrides {
                listen: Some("127.0.0.1:9003".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(cfg.listen.port(), 9003);

        // Drop the cli override → env wins.
        let cfg = resolve(
            file_with_listen("127.0.0.1:9001"),
            EnvOverrides {
                listen: Some("127.0.0.1:9002".into()),
                ..Default::default()
            },
            CliOverrides::default(),
        )
        .unwrap();
        assert_eq!(cfg.listen.port(), 9002);

        // Drop env too → file wins.
        let cfg = resolve(
            file_with_listen("127.0.0.1:9001"),
            EnvOverrides::default(),
            CliOverrides::default(),
        )
        .unwrap();
        assert_eq!(cfg.listen.port(), 9001);
    }

    #[test]
    fn defaults_when_nothing_supplied() {
        let cfg = resolve(
            file_with_listen("127.0.0.1:9001"),
            EnvOverrides::default(),
            CliOverrides::default(),
        )
        .unwrap();
        assert_eq!(cfg.poll_ms, 1000);
        assert_eq!(cfg.display_name, "rt");
    }

    #[test]
    fn poll_ms_is_floored() {
        let mut file = file_with_listen("127.0.0.1:9001");
        file.poll_ms = Some(10);
        let cfg = resolve(file, EnvOverrides::default(), CliOverrides::default()).unwrap();
        assert_eq!(cfg.poll_ms, 250);
    }

    #[test]
    fn auth_none_rejected_off_loopback() {
        let err = resolve(
            file_with_listen("0.0.0.0:9080"),
            EnvOverrides::default(),
            CliOverrides::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("loopback"), "{err}");
    }

    #[test]
    fn password_mode_needs_a_hash() {
        let file = FileConfig {
            listen: Some("0.0.0.0:9080".into()),
            auth: AuthConfig {
                mode: AuthMode::Password,
                password_hash: None,
            },
            ..Default::default()
        };
        let err = resolve(file, EnvOverrides::default(), CliOverrides::default()).unwrap_err();
        assert!(err.to_string().contains("password_hash"), "{err}");
    }

    #[test]
    fn mock_mode_is_not_exempt_from_the_password_check() {
        // A mock server still authenticates: without a hash every request is
        // refused, which reads as a broken UI rather than a missing setting.
        let file = FileConfig {
            listen: Some("127.0.0.1:9080".into()),
            auth: AuthConfig {
                mode: AuthMode::Password,
                password_hash: None,
            },
            ..Default::default()
        };
        let err = resolve(
            file,
            EnvOverrides {
                mock: true,
                ..Default::default()
            },
            CliOverrides::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("password_hash"), "{err}");
    }

    #[test]
    fn a_mock_run_with_auth_off_still_resolves() {
        let file = FileConfig {
            listen: Some("127.0.0.1:9080".into()),
            auth: AuthConfig {
                mode: AuthMode::None,
                password_hash: None,
            },
            ..Default::default()
        };
        let cfg = resolve(
            file,
            EnvOverrides {
                mock: true,
                ..Default::default()
            },
            CliOverrides::default(),
        )
        .expect("the documented mock loop resolves");
        assert!(cfg.mock);
        assert_eq!(cfg.auth_mode, AuthMode::None);
    }

    #[test]
    fn the_fallback_transport_is_the_local_daemons_socket() {
        // Not the container image's `/home/rtorrent/...`: a config-less run is a
        // run on someone's machine, and pointing it at a path that cannot exist
        // there reports a healthy daemon as unreachable.
        let file = FileConfig {
            listen: Some("127.0.0.1:9080".into()),
            auth: AuthConfig {
                mode: AuthMode::None,
                password_hash: None,
            },
            ..Default::default()
        };
        let cfg = resolve(file, EnvOverrides::default(), CliOverrides::default())
            .expect("a loopback config resolves");
        let expected = home_dir().join(".rtorrent").join("rpc.socket");
        match cfg.transport {
            Transport::UnixSocket { path } => {
                assert_eq!(path, expected.to_string_lossy());
                assert!(!path.starts_with("/home/rtorrent"), "{path}");
            }
            other => panic!("expected the local socket, got {other:?}"),
        }
    }

    #[test]
    fn transport_kinds_parse_from_toml() {
        let toml = r#"
            listen = "127.0.0.1:9080"
            [transport]
            kind = "tcp"
            host = "127.0.0.1"
            port = 5000
            [auth]
            mode = "none"
        "#;
        let file: FileConfig = toml::from_str(toml).unwrap();
        let cfg = resolve(file, EnvOverrides::default(), CliOverrides::default()).unwrap();
        assert!(matches!(cfg.transport, Transport::Tcp { port: 5000, .. }));
    }

    #[test]
    fn http_transport_password_is_extracted() {
        let toml = r#"
            listen = "127.0.0.1:9080"
            [transport]
            kind = "http"
            url = "https://box.example/RPC2"
            username = "alice"
            password = "s3cret"
            [auth]
            mode = "none"
        "#;
        let file: FileConfig = toml::from_str(toml).unwrap();
        let cfg = resolve(file, EnvOverrides::default(), CliOverrides::default()).unwrap();
        assert_eq!(cfg.daemon_password.as_deref(), Some("s3cret"));
        assert!(matches!(cfg.transport, Transport::Http { .. }));
    }

    /// A password-mode file, so the cookie/session checks (not the auth checks)
    /// are what a test is exercising.
    fn password_file(listen: &str) -> FileConfig {
        FileConfig {
            listen: Some(listen.to_string()),
            auth: AuthConfig {
                mode: AuthMode::Password,
                password_hash: Some("$argon2id$placeholder".into()),
            },
            ..Default::default()
        }
    }

    #[test]
    fn a_non_loopback_bind_needs_secure_session_cookies() {
        let err = resolve(
            password_file("0.0.0.0:9080"),
            EnvOverrides::default(),
            CliOverrides::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("secure session cookies"), "{err}");

        // A trusted proxy (so X-Forwarded-Proto can be honoured) is enough…
        let mut with_proxy = password_file("0.0.0.0:9080");
        with_proxy.trusted_proxies = Some(vec!["127.0.0.1".into()]);
        let cfg = resolve(with_proxy, EnvOverrides::default(), CliOverrides::default())
            .expect("a trusted proxy makes an HTTPS scheme visible");
        assert_eq!(cfg.hardening.trusted_proxies.len(), 1);

        // …and forcing the flag is enough without one.
        let mut forced = password_file("0.0.0.0:9080");
        forced.secure_cookies = Some(true);
        resolve(forced, EnvOverrides::default(), CliOverrides::default())
            .expect("forcing Secure resolves");
    }

    #[test]
    fn a_bad_trusted_proxy_entry_names_the_value() {
        let mut file = file_with_listen("127.0.0.1:9080");
        file.trusted_proxies = Some(vec!["not-an-ip".into()]);
        let err = resolve(file, EnvOverrides::default(), CliOverrides::default()).unwrap_err();
        assert!(err.to_string().contains("not-an-ip"), "{err}");
    }

    #[test]
    fn trusted_proxies_parse_from_toml() {
        let toml = r#"
            listen = "127.0.0.1:9080"
            trusted_proxies = ["127.0.0.1", "::1"]
            secure_cookies = true
            [auth]
            mode = "none"
        "#;
        let file: FileConfig = toml::from_str(toml).unwrap();
        let cfg = resolve(file, EnvOverrides::default(), CliOverrides::default()).unwrap();
        assert_eq!(cfg.hardening.trusted_proxies.len(), 2);
        assert!(cfg.hardening.secure_cookies);
    }

    #[test]
    fn queue_caps_parse_from_toml_and_default_to_zero() {
        let toml = r#"
            listen = "127.0.0.1:9080"
            [queue]
            max_active_downloads = 3
            max_active_uploads = 2
            max_active_torrents = 4
            queue_slow_limit_kbs = 50
            [auth]
            mode = "none"
        "#;
        let file: FileConfig = toml::from_str(toml).unwrap();
        let cfg = resolve(file, EnvOverrides::default(), CliOverrides::default()).unwrap();
        assert_eq!(cfg.max_active_downloads, 3);
        assert_eq!(cfg.max_active_uploads, 2);
        assert_eq!(cfg.max_active_torrents, 4);
        assert_eq!(cfg.queue_slow_limit_kbs, 50);

        let bare = r#"
            listen = "127.0.0.1:9080"
            [auth]
            mode = "none"
        "#;
        let file: FileConfig = toml::from_str(bare).unwrap();
        let cfg = resolve(file, EnvOverrides::default(), CliOverrides::default()).unwrap();
        assert_eq!(cfg.max_active_downloads, 0);
        assert_eq!(cfg.max_active_uploads, 0);
        assert_eq!(cfg.max_active_torrents, 0);
        assert_eq!(cfg.queue_slow_limit_kbs, 0);
        assert!(cfg.bandwidth_rules.is_empty());
    }

    #[test]
    fn bandwidth_rules_parse_from_toml() {
        let toml = r#"
            listen = "127.0.0.1:9080"
            [[bandwidth]]
            id = "vid"
            label = "video"
            down_kb = 1024
            up_kb = 256
            peers_max = 50
            [[bandwidth]]
            id = "archive"
            tag = "Archive"
            down_kb = 512
            up_kb = 128
            [auth]
            mode = "none"
        "#;
        let file: FileConfig = toml::from_str(toml).unwrap();
        let cfg = resolve(file, EnvOverrides::default(), CliOverrides::default()).unwrap();
        assert_eq!(cfg.bandwidth_rules.len(), 2);
        assert_eq!(cfg.bandwidth_rules[0].id, "vid");
        assert_eq!(cfg.bandwidth_rules[0].peers_max, 50);
        assert_eq!(cfg.bandwidth_rules[0].peers_min, 0);
        assert_eq!(cfg.bandwidth_rules[1].tag.as_deref(), Some("Archive"));
        assert_eq!(cfg.bandwidth_rules[1].throttle_name(), "rule_archive");
    }

    #[test]
    fn incomplete_dir_and_move_rules_parse_from_toml() {
        let toml = r#"
            listen = "127.0.0.1:9080"
            [paths]
            incomplete_dir = "/dl/.incomplete"
            collision_policy = "auto-rename"
            [[paths.move_rules]]
            tag = "Archive"
            destination = "/media/archive"
            [[paths.move_rules]]
            label = "video"
            destination = "/media/video"
            [auth]
            mode = "none"
        "#;
        let file: FileConfig = toml::from_str(toml).unwrap();
        let cfg = resolve(file, EnvOverrides::default(), CliOverrides::default()).unwrap();
        assert_eq!(cfg.incomplete_dir, "/dl/.incomplete");
        assert_eq!(cfg.collision_policy, CollisionPolicy::AutoRename);
        assert_eq!(cfg.move_rules.len(), 2);
        // The mixed-case tag still matches (matching is case-insensitive).
        assert_eq!(
            rtorrent_core::complete::resolve_destination(
                "other",
                &["archive".to_string()],
                &cfg.move_rules,
                "/dl"
            ),
            "/media/archive"
        );
    }

    #[test]
    fn the_incomplete_dir_env_override_wins_over_the_file() {
        let toml = r#"
            listen = "127.0.0.1:9080"
            [paths]
            incomplete_dir = "/dl/.incomplete"
            [auth]
            mode = "none"
        "#;
        let file: FileConfig = toml::from_str(toml).unwrap();
        let cfg = resolve(
            file,
            EnvOverrides {
                incomplete_dir: Some("/env/incomplete".into()),
                ..Default::default()
            },
            CliOverrides::default(),
        )
        .unwrap();
        assert_eq!(cfg.incomplete_dir, "/env/incomplete");
    }
}
