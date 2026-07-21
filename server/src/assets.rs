//! Serving the embedded single-page app.
//!
//! In a release build the `dist-web/` bundle is embedded in the binary, so a
//! deploy is one file. `--assets <dir>` overrides that with an on-disk directory
//! (used in development, or to ship assets separately). Unknown paths fall back
//! to `index.html` so client-side routing works.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

use crate::state::AppState;

// Path is relative to this crate's manifest dir (server/), i.e. repo-root/dist-web.
#[derive(Embed)]
#[folder = "../dist-web"]
struct Assets;

/// A router whose fallback serves the SPA.
pub fn router(state: Arc<AppState>) -> axum::Router {
    axum::Router::new().fallback(serve).with_state(state)
}

async fn serve(State(state): State<Arc<AppState>>, uri: Uri) -> Response {
    let raw = uri.path().trim_start_matches('/');
    match &state.config.assets_dir {
        Some(dir) => serve_from_dir(dir, raw),
        None => serve_embedded(raw),
    }
}

/// The SPA entry document. Vite (`vite.web.config.ts`) builds `web.html`, so the
/// bundle's HTML entry keeps that name; it is what `/` and any client route serve.
const INDEX: &str = "web.html";

fn serve_embedded(raw: &str) -> Response {
    let path = if raw.is_empty() { INDEX } else { raw };
    if let Some(file) = Assets::get(path) {
        return with_mime(path, file.data.into_owned());
    }
    // SPA fallback: unknown non-file path → the entry document.
    match Assets::get(INDEX) {
        Some(index) => with_mime(INDEX, index.data.into_owned()),
        None => not_built(),
    }
}

fn serve_from_dir(dir: &Path, raw: &str) -> Response {
    let rel = if raw.is_empty() { INDEX } else { raw };
    // Reject traversal; only serve within `dir`.
    if rel.split('/').any(|seg| seg == ".." || seg == ".") {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }
    let full: PathBuf = dir.join(rel);
    match std::fs::read(&full) {
        Ok(bytes) => with_mime(rel, bytes),
        Err(_) => match std::fs::read(dir.join(INDEX)) {
            Ok(index) => with_mime(INDEX, index),
            Err(_) => not_built(),
        },
    }
}

fn with_mime(path: &str, bytes: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    ([(header::CONTENT_TYPE, mime.as_ref().to_string())], bytes).into_response()
}

fn not_built() -> Response {
    (
        StatusCode::NOT_FOUND,
        "web UI not built — run `npm run build:web`, or pass --assets <dir>",
    )
        .into_response()
}
