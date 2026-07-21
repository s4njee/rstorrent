//! Server configuration: a TOML file, overlaid by `RSTORRENT_WEB_*` env vars,
//! overlaid by CLI flags (flags > env > file > built-in defaults).
//!
//! The resolution is split into a pure [`resolve`] step (file + env + cli →
//! [`Config`]) so precedence is unit-testable without touching the filesystem or
//! the process environment.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use rtorrent_core::types::Transport;
use serde::Deserialize;

/// The default listen address — loopback, so a fresh install is not exposed.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:9080";

/// How the server authenticates browser sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
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
    /// Fast-poll cadence in milliseconds.
    pub poll_ms: u64,
    /// Optional on-disk asset directory (overrides the embedded SPA).
    pub assets_dir: Option<PathBuf>,
    /// Serve the ten fixture torrents with no daemon (`RSTORRENT_MOCK=1`).
    pub mock: bool,
}

impl Config {
    /// True when the listen address is loopback (127.0.0.0/8 or ::1).
    pub fn is_loopback(&self) -> bool {
        self.listen.ip().is_loopback()
    }

    /// Reject configurations that would expose an unauthenticated daemon.
    pub fn validate(&self) -> Result<()> {
        if self.auth_mode == AuthMode::None && !self.is_loopback() {
            return Err(anyhow!(
                "auth.mode = \"none\" is only allowed on a loopback bind; \
                 {} is reachable from the network — set a password_hash",
                self.listen
            ));
        }
        if self.auth_mode == AuthMode::Password && self.password_hash.is_none() && !self.mock {
            return Err(anyhow!(
                "auth.mode = \"password\" needs an [auth].password_hash — run \
                 `rstorrent-web hash-password` to generate one"
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
}

// --- Env + CLI overlays ------------------------------------------------------

/// Values pulled from `RSTORRENT_WEB_*` env vars (and `RSTORRENT_MOCK`).
#[derive(Debug, Default)]
pub struct EnvOverrides {
    pub listen: Option<String>,
    pub display_name: Option<String>,
    pub save_path: Option<String>,
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
        // A mock run needs no real transport; default to the conventional local
        // socket so a non-mock run without [transport] still starts and simply
        // reports "disconnected" until pointed at a real daemon.
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
        poll_ms: cli_poll(env.poll_ms, file.poll_ms),
        assets_dir: cli.assets_dir,
        mock: env.mock,
    };
    config.validate()?;
    Ok(config)
}

fn cli_poll(env: Option<u64>, file: Option<u64>) -> u64 {
    // Floor at 250ms so a typo can't hammer the daemon.
    env.or(file).unwrap_or(1000).max(250)
}

fn default_socket_path() -> String {
    "/home/rtorrent/.rtorrent/rpc.socket".to_string()
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

    resolve(file, EnvOverrides::from_env(), cli)
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
}
