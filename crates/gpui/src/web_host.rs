//! The web UI, served from inside the app.
//!
//! The server is a library (`rstorrent_web`), so this does not spawn a second
//! binary: it runs the same axum app on a loopback port, in the app's own Tokio
//! runtime and against the daemon the app is already connected to — so the
//! browser sees the same torrents, with no config file and nothing extra to
//! sign. Switching it off drops the server, which shuts down gracefully.

use std::net::SocketAddr;

use rstorrent_web::config::{AuthMode, Config};
use rstorrent_web::Server;
use rtorrent_core::types::Transport;

use crate::settings::Settings;

/// The port to try first: the one the docs, the bookmarks and the dev loop use.
pub const PREFERRED_PORT: u16 = 9080;

/// The web UI, while the app is serving it.
#[derive(Default)]
pub struct WebHost {
    server: Option<Server>,
}

impl WebHost {
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.server.is_some()
    }

    /// Where the browser should go, while it is running.
    #[must_use]
    pub fn url(&self) -> Option<String> {
        self.server.as_ref().map(Server::url)
    }

    /// Hand the running server to a caller that will stop it.
    ///
    /// A `Server` stops itself when it is dropped, so taking it out of here and
    /// awaiting its stop is the whole of "turning the web UI off".
    pub fn take(&mut self) -> Option<Server> {
        self.server.take()
    }

    /// Keep a server the caller has just started.
    pub fn adopt(&mut self, server: Server) {
        self.server = Some(server);
    }
}

/// The configuration a desktop-hosted server runs with.
///
/// Loopback only, and no login: the listener is on this machine, and the
/// daemon's own SCGI socket is unauthenticated too, so a password here would
/// protect nothing a local process could not already reach. Everything else
/// follows the app: the same daemon, the same save path, and the same
/// move-on-complete settings (V3-14) so web-UI adds route like native adds.
#[must_use]
pub fn config_for(
    transport: Transport,
    mock: bool,
    listen: SocketAddr,
    settings: &Settings,
) -> Config {
    Config {
        listen,
        transport,
        daemon_password: None,
        auth_mode: AuthMode::None,
        password_hash: None,
        display_name: "bb".to_owned(),
        save_path: settings.default_save_path.clone(),
        volumes: Vec::new(),
        incomplete_dir: settings.incomplete_dir.clone(),
        move_rules: settings.move_rules.clone(),
        collision_policy: settings.collision_policy,
        bandwidth_rules: settings.bandwidth_rules.clone(),
        max_active_downloads: settings.max_active_downloads,
        max_active_uploads: settings.max_active_uploads,
        max_active_torrents: settings.max_active_torrents,
        queue_slow_limit_kbs: settings.queue_slow_limit_kbs,
        poll_ms: settings.poll_ms,
        // The SPA is embedded in this binary, as it is in the server's.
        assets_dir: None,
        mock,
        config_path: None,
        hardening: Default::default(),
    }
}

/// Start serving on the preferred port, or any free one if it is taken.
///
/// The preferred port first so the URL is worth remembering; a dev server or
/// another instance holding it should not stop the app from offering the UI.
///
/// # Errors
///
/// Returns a user-facing message when neither attempt can bind.
pub async fn start(settings: Settings) -> Result<(Server, String), String> {
    let preferred = SocketAddr::from(([127, 0, 0, 1], PREFERRED_PORT));
    let fallback = SocketAddr::from(([127, 0, 0, 1], 0));

    let mut last = String::new();
    for listen in [preferred, fallback] {
        let config = config_for(settings.transport.clone(), settings.mock, listen, &settings);
        match rstorrent_web::serve(config).await {
            Ok(server) => {
                let url = server.url();
                return Ok((server, url));
            }
            Err(error) => last = error.to_string(),
        }
    }
    Err(format!("could not start the web UI: {last}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transport() -> Transport {
        Transport::UnixSocket {
            path: "/tmp/rstorrent-web-host-test.sock".into(),
        }
    }

    #[test]
    fn the_hosted_config_is_loopback_with_no_login() {
        let mut settings = Settings::default();
        settings.mock = true;
        settings.default_save_path = "/downloads".into();
        settings.poll_ms = 500;
        settings.incomplete_dir = "/downloads/.incomplete".into();
        let config = config_for(
            transport(),
            true,
            SocketAddr::from(([127, 0, 0, 1], PREFERRED_PORT)),
            &settings,
        );
        assert!(config.listen.ip().is_loopback());
        assert_eq!(config.auth_mode, AuthMode::None);
        assert!(config.password_hash.is_none());
        assert!(config.mock, "the app's mock flag is carried over");
        assert_eq!(config.save_path, "/downloads");
        assert_eq!(config.poll_ms, 500);
        assert_eq!(config.incomplete_dir, "/downloads/.incomplete");
        // The SPA comes from the binary, not a directory.
        assert!(config.assets_dir.is_none());
        // And the config the app builds passes the server's own validation.
        config.validate().expect("the hosted config is valid");
    }

    #[test]
    fn a_fresh_host_is_not_running() {
        let host = WebHost::default();
        assert!(!host.is_running());
        assert_eq!(host.url(), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn starting_and_stopping_hands_the_port_back() {
        let mut host = WebHost::default();
        let mut settings = Settings::default();
        settings.mock = true;
        settings.default_save_path = "/downloads".into();
        let (server, url) = start(settings).await.expect("the mock server starts");
        host.adopt(server);

        assert!(host.is_running());
        assert_eq!(host.url().as_deref(), Some(url.as_str()));
        assert!(url.starts_with("http://127.0.0.1:"), "{url}");

        // The whole point of the feature: the SPA this app embeds comes back
        // over the socket, so the browser gets a real page.
        let page = get(&url, "/").await;
        assert!(page.starts_with("HTTP/1.1 200"), "{page}");
        assert!(page.contains("<title>Blackbird"), "{page}");

        let stopped = host.take().expect("a running server").stop().await;
        stopped.expect("stops cleanly");
        assert!(!host.is_running());
        assert_eq!(host.url(), None);
    }

    /// One HTTP GET, so the test needs no client crate.
    async fn get(base: &str, path: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let authority = base.trim_start_matches("http://");
        let mut stream = tokio::net::TcpStream::connect(authority)
            .await
            .expect("connects");
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {authority}\r\nX-Rstorrent: 1\r\nConnection: close\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .await
            .expect("writes the request");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("reads the response");
        response
    }
}
