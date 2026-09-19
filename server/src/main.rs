//! The `rstorrent-web` command line: load a config, then serve.
//!
//! Everything serveable lives in the library beside this (`rstorrent_web`), so
//! the desktop app can offer the same web UI from inside its own process. This
//! file is only the CLI around it: argument parsing, logging, and Ctrl-C.

use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use rstorrent_web::config::{CliOverrides, Config};

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
            let config = rstorrent_web::config::load(
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
    let hash = rstorrent_web::auth::hash_password(password)?;
    println!("# add this under [auth] in your rstorrent-web.toml:");
    println!("password_hash = \"{hash}\"");
    Ok(())
}

/// Serve until Ctrl-C or SIGTERM: this process owns the runtime, the library
/// owns the server. systemd stops with SIGTERM, so both are handled.
fn run_server(config: Config) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new().context("starting tokio runtime")?;
    runtime.block_on(async move {
        let server = rstorrent_web::serve(config).await?;
        shutdown_signal().await;
        tracing::info!("shutting down");
        // Bound the drain so a stuck request cannot hang a restart for ever.
        match tokio::time::timeout(std::time::Duration::from_secs(10), server.stop()).await {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!("graceful shutdown timed out; exiting");
                Ok(())
            }
        }
    })
}

/// Resolve on Ctrl-C or SIGTERM (systemd's stop).
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut sig) = signal(SignalKind::terminate()) {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
