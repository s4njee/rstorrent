//! `rstorrent-web` as a library: the axum app, its state and poller, and
//! [`serve`], which a host can run on its own Tokio runtime.
//!
//! The server began as a binary and is still one — `main.rs` is a thin CLI over
//! this — but the desktop app offers the same web UI from inside itself, so the
//! serveable half lives here rather than in `main`.

pub mod api;
pub mod assets;
pub mod auth;
pub mod cmd;
pub mod config;
pub mod disk;
pub mod history;
pub mod moves;
pub mod poller;
pub mod session;
pub mod settings;
pub mod state;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use axum::http::{HeaderName, HeaderValue};
use tokio::sync::oneshot;
use tower_http::compression::CompressionLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use rtorrent_core::rtorrent::{client::RpcClient, mock::MockClient, RtorrentApi};
use rtorrent_core::types::Transport;

use crate::config::Config;
use crate::state::AppState;

/// A running server: where it is listening, and how to stop it.
///
/// Dropping it stops the server too — the shutdown channel closes, which the
/// serving task reads as "graceful shutdown" — so a host cannot leak one by
/// forgetting to call [`Server::stop`].
pub struct Server {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Result<()>>,
}

impl Server {
    /// The address actually bound, which is how a caller that asked for port 0
    /// learns which port it got.
    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The address as a browser URL.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Stop accepting, let in-flight requests finish, and wait for the task.
    ///
    /// # Errors
    ///
    /// Returns the serving error, or a message when the task itself panicked.
    pub async fn stop(mut self) -> Result<()> {
        if let Some(shutdown) = self.shutdown.take() {
            // The receiver may already be gone if the task is finishing anyway.
            let _ = shutdown.send(());
        }
        match self.task.await {
            Ok(result) => result,
            Err(error) => Err(anyhow!("the server task failed: {error}")),
        }
    }
}

/// Bind `config.listen`, start the poller, and serve until the returned
/// [`Server`] is stopped or dropped.
///
/// Runs on the caller's Tokio runtime rather than making one of its own, so a
/// host that already has one (the desktop app does) can hold this alongside its
/// other work. The listener is bound before this returns, so a port already in
/// use is an error the caller sees rather than a background failure.
///
/// # Errors
///
/// Returns the bind error, or any error the serving task ends with.
pub async fn serve(config: Config) -> Result<Server> {
    if !config.listen.ip().is_loopback() {
        tracing::warn!(
            "binding {} (non-loopback): terminate TLS at a reverse proxy — SCGI \
             and basic auth are plaintext",
            config.listen
        );
    }

    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("binding {}", config.listen))?;
    let addr = listener.local_addr().context("reading the bound address")?;

    let backend = make_backend(&config);
    let state = Arc::new(AppState::new(config, backend));
    tokio::spawn(poller::run(state.clone()));

    let (shutdown, stopped) = oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = stopped.await;
        })
        .await
        .context("server error")
    });

    tracing::info!("rstorrent-web listening on http://{addr}");
    Ok(Server {
        addr,
        shutdown: Some(shutdown),
        task,
    })
}

/// The routes, with the hardening headers every response carries.
fn router(state: Arc<AppState>) -> axum::Router {
    api::router(state.clone())
        .merge(obs_router(state.clone()))
        .merge(assets::router(state))
        // Hardening headers on every response (WE5-S4).
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
}

/// Unauthenticated probes (WEB-06): `/health` is liveness (the process is
/// serving) and `/ready` is readiness (the rtorrent daemon is reachable). An
/// orchestrator can restart on liveness and keep traffic off on readiness;
/// neither leaks configuration.
fn obs_router(state: Arc<AppState>) -> axum::Router {
    use axum::http::StatusCode;
    use axum::routing::get;

    let ready = state.clone();
    axum::Router::new()
        .route("/health", get(|| async { (StatusCode::OK, "ok") }))
        .route(
            "/ready",
            get(move || {
                let state = ready.clone();
                async move {
                    if state.conn().phase == rtorrent_core::types::ConnPhase::Connected {
                        (StatusCode::OK, "ready")
                    } else {
                        (StatusCode::SERVICE_UNAVAILABLE, "not ready")
                    }
                }
            }),
        )
}

/// Construct the daemon backend from config: the fixture mock, or a real client
/// (with the config-supplied basic-auth password for an HTTP transport).
fn make_backend(config: &Config) -> Box<dyn RtorrentApi> {
    if config.mock {
        return Box::new(MockClient::new());
    }
    match (&config.transport, &config.daemon_password) {
        (Transport::Http { .. }, Some(_)) => Box::new(RpcClient::with_password(
            config.transport.clone(),
            config.daemon_password.clone(),
        )),
        _ => Box::new(RpcClient::new(config.transport.clone())),
    }
}

/// A short human label for a transport, shown in the footer and connection state.
#[must_use]
pub fn endpoint_label(transport: &Transport) -> String {
    match transport {
        Transport::UnixSocket { path } => format!("unix:{path}"),
        Transport::Tcp { host, port } => format!("tcp:{host}:{port}"),
        Transport::Http { url, .. } => url.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuthMode;

    fn loopback_config(mock: bool) -> Config {
        Config {
            listen: "127.0.0.1:0".parse().expect("a loopback address"),
            transport: Transport::UnixSocket {
                path: "/tmp/rstorrent-test.sock".into(),
            },
            daemon_password: None,
            auth_mode: AuthMode::None,
            password_hash: None,
            display_name: "rt".into(),
            save_path: String::new(),
            volumes: Vec::new(),
            incomplete_dir: String::new(),
            move_rules: Vec::new(),
            collision_policy: Default::default(),
            bandwidth_rules: Vec::new(),
            max_active_downloads: 0,
            max_active_uploads: 0,
            max_active_torrents: 0,
            queue_slow_limit_kbs: 0,
            poll_ms: 1_000,
            assets_dir: None,
            mock,
            config_path: None,
            hardening: Default::default(),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn serving_binds_reports_its_address_and_stops() {
        let server = serve(loopback_config(true)).await.expect("the mock serves");
        let addr = server.addr();
        assert_ne!(addr.port(), 0, "the OS assigned a real port");
        assert_eq!(server.url(), format!("http://{addr}"));

        // A real request over the socket, through the whole stack.
        let body = get(&format!("http://{addr}/api/health")).await;
        assert!(body.starts_with("HTTP/1.1 200"), "{body}");
        assert!(body.contains("\"displayName\""), "{body}");

        let port = addr.port();
        server.stop().await.expect("stops cleanly");

        // The port is released, so a second server can take it.
        let mut again = loopback_config(true);
        again.listen = format!("127.0.0.1:{port}").parse().expect("an address");
        let second = serve(again).await.expect("the port was released");
        second.stop().await.expect("stops cleanly");
    }

    #[tokio::test]
    async fn ready_reflects_the_daemon_connection() {
        use crate::state::AppState;
        use axum::body::Body;
        use axum::http::StatusCode;
        use rtorrent_core::rtorrent::mock::MockClient;
        use std::sync::Arc;
        use tower::ServiceExt;

        let state = Arc::new(AppState::new(
            loopback_config(true),
            Box::new(MockClient::new()),
        ));
        let request = |path: &str| {
            axum::http::Request::builder()
                .uri(path)
                .body(Body::empty())
                .unwrap()
        };
        let app = obs_router(state.clone());
        // Nothing polled yet: liveness is up, readiness is not.
        assert_eq!(
            app.clone()
                .oneshot(request("/health"))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            app.clone()
                .oneshot(request("/ready"))
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );

        crate::poller::run_one_for_test(&state).await;
        assert_eq!(
            app.oneshot(request("/ready")).await.unwrap().status(),
            StatusCode::OK
        );
    }

    // A tiny HTTP GET, so the test needs no client crate: the request is one
    // line and the response is JSON we can look inside.
    async fn get(url: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let without_scheme = url.trim_start_matches("http://");
        let (authority, path) = without_scheme.split_once('/').expect("a path");
        let mut stream = tokio::net::TcpStream::connect(authority)
            .await
            .expect("connects");
        let request = format!("GET /{path} HTTP/1.1\r\nHost: {authority}\r\nX-Rstorrent: 1\r\nConnection: close\r\n\r\n");
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
