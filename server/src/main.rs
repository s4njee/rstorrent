//! `rstorrent-web` — a self-hosted web UI server for the rtorrent daemon.
//!
//! It serves the same React UI as the desktop app (built to `dist-web/`) and
//! proxies the daemon's XML-RPC/SCGI interface as JSON over `/api/*`, reusing the
//! shared `rtorrent-core` client. The browser talks only to this server, never to
//! SCGI (which is unauthenticated and must stay off the network).

mod api;
mod assets;
mod auth;
mod cmd;
mod config;
mod disk;
mod poller;
mod state;

use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::http::{HeaderName, HeaderValue};
use clap::{Parser, Subcommand};
use tower_http::compression::CompressionLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use rtorrent_core::rtorrent::{client::RpcClient, mock::MockClient, RtorrentApi};
use rtorrent_core::types::Transport;

use crate::config::{CliOverrides, Config};
use crate::state::AppState;

#[derive(Parser)]
#[command(name = "rstorrent-web", version, about)]
struct Cli {
    /// Path to the config file (default: ./rstorrent-web.toml if present).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Override the listen address, e.g. 127.0.0.1:9080.
    #[arg(long, global = true)]
    listen: Option<String>,
    /// Serve the SPA from this directory instead of the embedded bundle.
    #[arg(long, global = true)]
    assets: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the server (default).
    Serve,
    /// Read a password from stdin and print its `[auth].password_hash` line.
    HashPassword,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rstorrent_web=info,tower_http=warn".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Some(Command::HashPassword) => hash_password_cmd(),
        Some(Command::Serve) | None => {
            let config = config::load(
                cli.config.as_deref(),
                CliOverrides {
                    listen: cli.listen,
                    assets_dir: cli.assets,
                },
            )
            .context("loading configuration")?;
            run_server(config)
        }
    }
}

/// `hash-password`: read one line from stdin, hash it, print the config line.
fn hash_password_cmd() -> Result<()> {
    eprint!("Enter the web-login password, then EOF: ");
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("reading password from stdin")?;
    let password = input.trim_end_matches(['\n', '\r']);
    if password.is_empty() {
        anyhow::bail!("no password given on stdin");
    }
    let hash = auth::hash_password(password)?;
    println!("# add this under [auth] in your rstorrent-web.toml:");
    println!("password_hash = \"{hash}\"");
    Ok(())
}

/// Build the backend, start the poller, and serve until shutdown.
fn run_server(config: Config) -> Result<()> {
    if !config.listen.ip().is_loopback() {
        tracing::warn!(
            "binding {} (non-loopback): terminate TLS at a reverse proxy — SCGI \
             and basic auth are plaintext",
            config.listen
        );
    }

    let runtime = tokio::runtime::Runtime::new().context("starting tokio runtime")?;
    runtime.block_on(async move {
        let backend = make_backend(&config);
        let state = Arc::new(AppState::new(config, backend));

        tokio::spawn(poller::run(state.clone()));

        let app = api::router(state.clone())
            .merge(assets::router(state.clone()))
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
            .layer(TraceLayer::new_for_http());

        let listener = tokio::net::TcpListener::bind(state.config.listen)
            .await
            .with_context(|| format!("binding {}", state.config.listen))?;
        tracing::info!("rstorrent-web listening on http://{}", state.config.listen);

        // `into_make_service_with_connect_info` exposes the client socket to the
        // auth handler for per-IP login rate limiting.
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")
    })
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

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}

/// A short human label for a transport, shown in the footer and connection state.
pub fn endpoint_label(transport: &Transport) -> String {
    match transport {
        Transport::UnixSocket { path } => format!("unix:{path}"),
        Transport::Tcp { host, port } => format!("tcp:{host}:{port}"),
        Transport::Http { url, .. } => url.clone(),
    }
}
