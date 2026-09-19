//! The `/api/*` HTTP surface. WE1 ships the read path: `/api/state` (the cached
//! snapshot, with ETag/304) and `/api/health`. Mutations, detail, and log tail
//! land in WE3.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Multipart, Path, Query, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use tower_http::timeout::TimeoutLayer;

use rtorrent_core::snapshot;
use rtorrent_core::types::{ConnPhase, DaemonHealth, DetailPayload, DetailTab, LogEntry, Snapshot};

use crate::config::AuthMode;
use crate::state::{etag_of, AppState};

/// How long a request will wait for the poller to fill a cold/stale cache.
const COLD_WAIT: Duration = Duration::from_secs(2);
/// Detail micro-cache TTL: rapid polls of the same (hash, tab) reuse this.
const DETAIL_TTL: Duration = Duration::from_millis(1000);
/// Requests slower than this are failed rather than left hanging (WEB-06).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The `/api` sub-router, behind the auth middleware.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/state", get(get_state))
        .route("/api/delta", get(get_delta))
        .route("/api/health", get(get_health))
        .route("/api/detail", get(get_detail))
        .route("/api/log", get(get_log))
        .route("/api/cmd/{name}", post(post_cmd))
        .route("/api/torrents/inspect", post(post_inspect))
        .route("/api/torrents/file", post(post_add_file))
        .route("/api/session", post(post_session).delete(delete_session))
        // Sign out everywhere: revoke every session, not just this browser.
        .route("/api/sessions", axum::routing::delete(delete_sessions))
        .route("/api/config", get(get_config).post(post_config))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/settings/password", post(post_password))
        .route("/api/settings/advanced", get(get_advanced))
        .route("/api/stats", get(get_stats))
        .route("/api/search", get(get_search))
        .route("/api/moves", get(get_moves))
        // Uploaded .torrent files are small; cap at 10 MiB.
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        // A request that cannot finish in this window is upstream trouble; the
        // client's 1 s poll will just try again.
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .with_state(state)
}

// --- Upload (WE4) ------------------------------------------------------------

// Uploads (WEB-06): the multipart fields are buffered in memory and dropped when
// the request ends, so there are no temp files to clean up. A failed parse
// returns before the body is consumed and axum drops the reader with it. The
// only cap that matters is the 10 MiB body limit on the router.

/// `POST /api/torrents/inspect` (multipart `file`) → parsed `TorrentMeta`, to
/// populate the Add dialog's file tree before the user confirms.
async fn post_inspect(mut mp: Multipart) -> Response {
    match file_field(&mut mp).await {
        Ok(bytes) => match rtorrent_core::torrent_file::read_metadata_bytes(&bytes) {
            Ok(meta) => Json(meta).into_response(),
            Err(e) => ApiError::bad(e).into_response(),
        },
        Err(e) => e.into_response(),
    }
}

/// `POST /api/torrents/file` (multipart `file` + `opts` JSON) → load the torrent.
async fn post_add_file(State(state): State<Arc<AppState>>, mut mp: Multipart) -> Response {
    if state.conn().phase != ConnPhase::Connected {
        return ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "rtorrent is not connected")
            .into_response();
    }
    let mut bytes: Option<Vec<u8>> = None;
    let mut opts = serde_json::Value::Null;
    loop {
        match mp.next_field().await {
            Ok(Some(field)) => match field.name() {
                Some("file") => match field.bytes().await {
                    Ok(b) => bytes = Some(b.to_vec()),
                    Err(e) => return ApiError::bad(e.to_string()).into_response(),
                },
                Some("opts") => {
                    if let Ok(text) = field.text().await {
                        opts = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
                    }
                }
                _ => {}
            },
            Ok(None) => break,
            Err(e) => return ApiError::bad(e.to_string()).into_response(),
        }
    }
    let Some(bytes) = bytes else {
        return ApiError::bad("missing `file` field").into_response();
    };
    let meta = match rtorrent_core::torrent_file::read_metadata_bytes(&bytes) {
        Ok(meta) => meta,
        Err(err) => return ApiError::bad(err).into_response(),
    };
    let mut load_opts = crate::cmd::load_options(Some(&opts));
    let (directory, final_dir) = rtorrent_core::complete::route_new_download(
        &state.config.incomplete_dir,
        &load_opts.directory,
    );
    load_opts.directory = directory;
    match state.backend.load_raw(bytes, load_opts).await {
        Ok(()) => {
            let added_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_default();
            let mut meta_values: Vec<(&str, &str)> = vec![
                ("added_by", "file"),
                ("source_path", "web-upload"),
                ("added_at", &added_at),
            ];
            if let Some(final_dir) = final_dir.as_deref() {
                meta_values.push((rtorrent_core::complete::FINAL_DIR_KEY, final_dir));
            }
            let _ = state
                .backend
                .set_custom_metadata(&meta.info_hash, &meta_values)
                .await;
            state.repoll.notify_waiters();
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => ApiError::from(e).into_response(),
    }
}

/// Read the `file` field's bytes from a multipart body.
async fn file_field(mp: &mut Multipart) -> Result<Vec<u8>, ApiError> {
    while let Some(field) = mp
        .next_field()
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?
    {
        if field.name() == Some("file") {
            return field
                .bytes()
                .await
                .map(|b| b.to_vec())
                .map_err(|e| ApiError::bad(e.to_string()));
        }
    }
    Err(ApiError::bad("missing `file` field"))
}

// --- Authentication (WE5) ----------------------------------------------------

#[derive(serde::Deserialize)]
struct LoginBody {
    password: String,
}

/// `POST /api/session` — verify the password, mint a session, set the cookie.
async fn post_session(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> Response {
    // No-auth mode (loopback dev): login is a no-op success.
    if state.config.auth_mode == AuthMode::None {
        return StatusCode::NO_CONTENT.into_response();
    }
    let client = client_context(&state.config, addr, &headers);
    if !state.rate.allow(client.ip) {
        tracing::warn!(
            event = "auth.login",
            outcome = "rate_limited",
            client = %client.ip,
            "login attempt rate-limited"
        );
        return ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too many attempts — wait a minute",
        )
        .into_response();
    }
    let ok = state
        .password_hash()
        .map(|h| crate::auth::verify_password(&h, &body.password))
        .unwrap_or(false);
    if !ok {
        // Generic message, no user enumeration surface (single user).
        tracing::warn!(
            event = "auth.login",
            outcome = "denied",
            client = %client.ip,
            "login denied"
        );
        return ApiError::new(StatusCode::UNAUTHORIZED, "invalid password").into_response();
    }
    let token = state.sessions.create();
    tracing::info!(
        event = "auth.login",
        outcome = "ok",
        client = %client.ip,
        secure = client.https || state.config.hardening.secure_cookies,
        "login accepted"
    );
    (
        [(
            header::SET_COOKIE,
            session_cookie(&state, &token, client.https),
        )],
        StatusCode::NO_CONTENT,
    )
        .into_response()
}

/// `DELETE /api/session` — revoke this session and clear the cookie.
async fn delete_session(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Some(token) = cookie_token(&headers) {
        state.sessions.revoke(&token);
        tracing::info!(
            event = "auth.logout",
            client = %addr.ip(),
            "session revoked"
        );
    }
    let client = client_context(&state.config, addr, &headers);
    (
        [(header::SET_COOKIE, cleared_cookie(&state, client.https))],
        StatusCode::NO_CONTENT,
    )
        .into_response()
}

/// `DELETE /api/sessions` — revoke **every** session ("sign out everywhere").
async fn delete_sessions(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    state.sessions.revoke_all();
    tracing::warn!(
        event = "auth.revoke_all",
        client = %addr.ip(),
        remaining = state.sessions.active(),
        "all sessions revoked"
    );
    let client = client_context(&state.config, addr, &headers);
    (
        [(header::SET_COOKIE, cleared_cookie(&state, client.https))],
        StatusCode::NO_CONTENT,
    )
        .into_response()
}

/// The client's IP and scheme, honouring `X-Forwarded-*` only from a trusted
/// proxy. The socket peer is otherwise the truth; a proxy that is not listed
/// cannot spoof the login rate-limit bucket or the cookie's `Secure` flag.
struct ClientContext {
    ip: IpAddr,
    https: bool,
}

fn client_context(
    config: &crate::config::Config,
    peer: SocketAddr,
    headers: &HeaderMap,
) -> ClientContext {
    let trusted = config.hardening.trusted_proxies.contains(&peer.ip());
    let forwarded_ip = || {
        headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .and_then(|value| value.trim().parse::<IpAddr>().ok())
    };
    let forwarded_https = || {
        headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("https"))
    };
    ClientContext {
        ip: if trusted {
            forwarded_ip().unwrap_or_else(|| peer.ip())
        } else {
            peer.ip()
        },
        https: trusted && forwarded_https(),
    }
}

/// The `Set-Cookie` for a login: `HttpOnly; SameSite=Strict; Path=/`, plus
/// `Secure` when the request arrived over HTTPS (a trusted proxy said so) or the
/// operator forced it.
fn session_cookie(state: &AppState, token: &str, https: bool) -> String {
    let secure = if state.config.hardening.secure_cookies || https {
        "; Secure"
    } else {
        ""
    };
    format!(
        "{}={token}; HttpOnly; SameSite=Strict; Path=/{secure}",
        crate::auth::COOKIE
    )
}

/// The `Set-Cookie` that clears the session.
fn cleared_cookie(state: &AppState, https: bool) -> String {
    let secure = if state.config.hardening.secure_cookies || https {
        "; Secure"
    } else {
        ""
    };
    format!(
        "{}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0{secure}",
        crate::auth::COOKIE
    )
}

/// Gate every `/api/*` route except `/api/session` on a valid session, and
/// require the `X-Rstorrent` header on mutations (CSRF defense-in-depth).
async fn require_auth(State(state): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    if state.config.auth_mode == AuthMode::None {
        return next.run(req).await;
    }
    if req.uri().path() == "/api/session" {
        return next.run(req).await;
    }
    let authed = cookie_token(req.headers())
        .map(|t| state.sessions.validate(&t))
        .unwrap_or(false);
    if !authed {
        return ApiError::new(StatusCode::UNAUTHORIZED, "not authenticated").into_response();
    }
    let is_mutation = req.method() == Method::POST
        || req.method() == Method::PUT
        || req.method() == Method::DELETE;
    if is_mutation && req.headers().get("X-Rstorrent").is_none() {
        tracing::warn!(
            event = "auth.csrf",
            path = req.uri().path(),
            "mutation missing X-Rstorrent header"
        );
        return ApiError::new(StatusCode::FORBIDDEN, "missing X-Rstorrent header").into_response();
    }
    next.run(req).await
}

/// Pull the session token out of the `Cookie` header.
fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    let prefix = format!("{}=", crate::auth::COOKIE);
    cookie
        .split(';')
        .map(str::trim)
        .find_map(|p| p.strip_prefix(&prefix))
        .map(String::from)
}

/// A JSON error mirroring how the frontend already surfaces Tauri rejections
/// (a plain message string), so the web adapter's error handling is identical.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    /// A 400 for a malformed command argument.
    pub fn bad(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
}

impl From<rtorrent_core::rtorrent::RtorrentError> for ApiError {
    fn from(e: rtorrent_core::rtorrent::RtorrentError) -> Self {
        // A daemon-side failure is upstream, not the client's fault.
        Self::new(StatusCode::BAD_GATEWAY, e.to_string())
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

/// `POST /api/cmd/{name}` — the mutation surface. Command names + JSON args
/// mirror the desktop `commands.ts` 1:1. 503 while disconnected; a success
/// triggers an immediate re-poll so the next `/api/state` reflects the change.
async fn post_cmd(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(args): Json<serde_json::Value>,
) -> Response {
    if state.conn().phase != ConnPhase::Connected {
        return ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "rtorrent is not connected")
            .into_response();
    }
    match crate::cmd::run(&state, &name, &args).await {
        Ok(value) => {
            state.repoll.notify_waiters();
            Json(value).into_response()
        }
        Err(e) => e.into_response(),
    }
}

/// `GET /api/state` → the cached [`Snapshot`], served with a strong ETag so an
/// unchanged 1s poll costs a 304.
async fn get_state(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    state.mark_request();
    ensure_fresh(&state).await;

    let cached = state.cache.read().unwrap().clone();
    let Some(cached) = cached else {
        // Still cold after the wait — hand back a connecting snapshot rather
        // than an error, so the UI shows its connecting state and keeps polling.
        let body = serde_json::to_vec(&connecting_snapshot(&state)).unwrap_or_default();
        return json_bytes(body, etag_of(b"connecting"), &headers);
    };

    json_bytes_cached(cached.body, cached.etag, &headers)
}

/// `GET /api/delta?since=<revision>` → incremental [`SnapshotDelta`] if the
/// client's revision is exactly one behind, otherwise `409` with the current
/// full snapshot so the client can heal (FND-02). `304` when already current.
async fn get_delta(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(q): axum::extract::Query<DeltaQuery>,
    headers: HeaderMap,
) -> Response {
    state.mark_request();
    ensure_fresh(&state).await;

    let cached = state.cache.read().unwrap().clone();
    let Some(cached) = cached else {
        let body = serde_json::to_vec(&connecting_snapshot(&state)).unwrap_or_default();
        return json_bytes(body, etag_of(b"connecting"), &headers);
    };
    let Some(since) = q.since else {
        return ApiError::bad("missing `since` query parameter").into_response();
    };

    // Client is already current — no delta needed.
    if since == cached.snapshot.revision {
        if let Some(inm) = headers.get(header::IF_NONE_MATCH) {
            // Honor ETag even for delta poll; treat matching snapshot ETag as 304.
            if inm.as_bytes() == cached.etag.as_bytes() {
                return Response::builder()
                    .status(StatusCode::NOT_MODIFIED)
                    .header(header::ETAG, &cached.etag)
                    .body(Body::empty())
                    .unwrap();
            }
        }
        // Also handle delta ETag if client sent it (rare).
        if let Some(delta) = state.delta_cache.read().unwrap().clone() {
            if let Some(inm) = headers.get(header::IF_NONE_MATCH) {
                if inm.as_bytes() == delta.etag.as_bytes() {
                    return Response::builder()
                        .status(StatusCode::NOT_MODIFIED)
                        .header(header::ETAG, &delta.etag)
                        .body(Body::empty())
                        .unwrap();
                }
            }
        }
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, &cached.etag)
            .body(Body::empty())
            .unwrap();
    }

    if let Some(delta) = state.delta_cache.read().unwrap().clone() {
        if delta.delta.base_revision == since && delta.delta.revision == cached.snapshot.revision {
            // Check If-None-Match for the delta body.
            if let Some(inm) = headers.get(header::IF_NONE_MATCH) {
                if inm.as_bytes() == delta.etag.as_bytes() {
                    return Response::builder()
                        .status(StatusCode::NOT_MODIFIED)
                        .header(header::ETAG, &delta.etag)
                        .body(Body::empty())
                        .unwrap();
                }
            }
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ETAG, &delta.etag)
                .body(Body::from(delta.body.to_vec()))
                .unwrap();
        }
    }

    // Missed revision or no delta (periodic full reconciliation) — tell the
    // client to re-sync with the current full snapshot (409).
    Response::builder()
        .status(StatusCode::CONFLICT)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ETAG, &cached.etag)
        .body(Body::from(cached.body.to_vec()))
        .unwrap()
}

/// If the cache is empty or stale (the loop was parked for idle), wait briefly
/// for the poll `mark_request` just kicked off to publish a fresh snapshot.
async fn ensure_fresh(state: &Arc<AppState>) {
    // Register interest *before* reading staleness, so a poll that completes in
    // the gap can't slip past the wait.
    let notified = state.cache_updated.notified();
    tokio::pin!(notified);
    notified.as_mut().enable();

    let stale = match &*state.cache.read().unwrap() {
        None => true,
        Some(c) => c.at.elapsed() > Duration::from_millis(state.config.poll_ms * 3),
    };
    if stale {
        let _ = tokio::time::timeout(COLD_WAIT, notified).await;
    }
}

/// `GET /api/detail?hash=&tab=` → the detail payload for one torrent's tab,
/// fetched on demand with a ~1s per-(hash,tab) micro-cache. No server-side watch
/// registration — the web adapter drives its own 2s loop.
async fn get_detail(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(q): axum::extract::Query<DetailQuery>,
) -> Response {
    if state.conn().phase != ConnPhase::Connected {
        return ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "rtorrent is not connected")
            .into_response();
    }
    let Some(tab) = parse_tab(&q.tab) else {
        return ApiError::bad("unknown tab").into_response();
    };
    let key = format!("{}:{}", q.hash, q.tab);

    // Serve from the micro-cache when fresh.
    if let Some((payload, at)) = state.detail_cache.lock().unwrap().get(&key) {
        if at.elapsed() < DETAIL_TTL {
            return Json(payload.clone()).into_response();
        }
    }

    let payload = match fetch_detail(&state, &q.hash, tab).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    state
        .detail_cache
        .lock()
        .unwrap()
        .insert(key, (payload.clone(), std::time::Instant::now()));
    Json(payload).into_response()
}

/// Fetch the data-bearing part of a tab. Speed/Log are frontend-derived, so they
/// return an empty payload.
async fn fetch_detail(
    state: &Arc<AppState>,
    hash: &str,
    tab: DetailTab,
) -> Result<DetailPayload, ApiError> {
    let mut payload = DetailPayload {
        hash: hash.to_string(),
        tab,
        trackers: None,
        peers: None,
        files: None,
        pieces: None,
    };
    match tab {
        DetailTab::General => payload.pieces = Some(state.backend.pieces(hash).await?),
        DetailTab::Trackers => payload.trackers = Some(state.backend.trackers(hash).await?),
        DetailTab::Peers => payload.peers = Some(state.backend.peers(hash).await?),
        DetailTab::Content => payload.files = Some(state.backend.files(hash).await?),
        DetailTab::Speed | DetailTab::Log => {}
    }
    Ok(payload)
}

fn parse_tab(s: &str) -> Option<DetailTab> {
    Some(match s {
        "general" => DetailTab::General,
        "trackers" => DetailTab::Trackers,
        "peers" => DetailTab::Peers,
        "content" => DetailTab::Content,
        "speed" => DetailTab::Speed,
        "log" => DetailTab::Log,
        _ => return None,
    })
}

/// `GET /api/log?after=<seq>` → log entries newer than `after`, plus the new
/// high-water sequence to pass back next time.
async fn get_log(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(q): axum::extract::Query<LogQuery>,
) -> Json<LogResponse> {
    let (entries, seq) = state.log_since(q.after.unwrap_or(0));
    Json(LogResponse { entries, seq })
}

#[derive(serde::Deserialize)]
struct DetailQuery {
    hash: String,
    tab: String,
}

#[derive(serde::Deserialize)]
struct DeltaQuery {
    since: Option<u64>,
}

#[derive(serde::Deserialize)]
struct LogQuery {
    after: Option<u64>,
}

#[derive(Serialize)]
struct LogResponse {
    entries: Vec<LogEntry>,
    seq: u64,
}

/// `GET /api/moves` → live move-on-complete statuses (V3-14), with byte
/// progress merged in for running moves. Browsers poll this while a move is
/// active; the desktop gets pushes over `moves://update` instead.
async fn get_moves(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<rtorrent_core::mover::MoveStatus>> {
    Json(state.moves.lock().unwrap().snapshot())
}

/// `GET /api/health` → server identity + best-effort daemon self-report.
async fn get_health(State(state): State<Arc<AppState>>) -> Json<Health> {
    let daemon = state.backend.daemon_health().await.ok();
    Json(Health {
        server: ServerInfo {
            version: env!("CARGO_PKG_VERSION"),
            display_name: state.config.display_name.clone(),
        },
        daemon,
    })
}

#[derive(Serialize)]
struct Health {
    server: ServerInfo,
    daemon: Option<DaemonHealth>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerInfo {
    version: &'static str,
    display_name: String,
}

/// An empty snapshot carrying the current connection state.
fn connecting_snapshot(state: &Arc<AppState>) -> Snapshot {
    Snapshot {
        revision: 0,
        torrents: vec![],
        globals: snapshot::empty_globals(),
        connection: state.conn(),
    }
}

/// Build a JSON response from freshly-serialized bytes, honoring `If-None-Match`.
fn json_bytes(body: Vec<u8>, etag: String, req_headers: &HeaderMap) -> Response {
    json_bytes_cached(body.into(), etag, req_headers)
}

/// Build a JSON response from cached bytes + ETag, honoring `If-None-Match`.
fn json_bytes_cached(body: Arc<[u8]>, etag: String, req_headers: &HeaderMap) -> Response {
    if let Some(inm) = req_headers.get(header::IF_NONE_MATCH) {
        if inm.as_bytes() == etag.as_bytes() {
            return Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, &etag)
                .body(Body::empty())
                .unwrap();
        }
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ETAG, &etag)
        .body(Body::from(body.to_vec()))
        .unwrap()
}

// --- Settings (WC7) ----------------------------------------------------------

/// `GET /api/config` → every daemon key with its current value (or unavailable).
async fn get_config(State(state): State<Arc<AppState>>) -> Response {
    if state.conn().phase != ConnPhase::Connected {
        return ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "rtorrent is not connected")
            .into_response();
    }
    let keys: Vec<&str> = crate::settings::DAEMON_KEYS
        .iter()
        .map(|key| key.key)
        .collect();
    match state.backend.config_get(&keys).await {
        Ok(values) => {
            let rows: Vec<crate::settings::ConfigRow> = crate::settings::DAEMON_KEYS
                .iter()
                .zip(values)
                .map(|(key, value)| key.row(value))
                .collect();
            Json(rows).into_response()
        }
        Err(e) => ApiError::from(e).into_response(),
    }
}

#[derive(Deserialize)]
struct ConfigUpdate {
    #[serde(default)]
    values: HashMap<String, String>,
}

#[derive(Serialize)]
struct ConfigFailure {
    id: String,
    error: String,
}

#[derive(Serialize)]
struct ConfigUpdateResult {
    applied: Vec<String>,
    failed: Vec<ConfigFailure>,
}

/// `POST /api/config` — write each changed key, reporting per-key outcomes so a
/// partial failure is never reported as success.
async fn post_config(
    State(state): State<Arc<AppState>>,
    Json(update): Json<ConfigUpdate>,
) -> Response {
    if state.conn().phase != ConnPhase::Connected {
        return ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "rtorrent is not connected")
            .into_response();
    }
    let mut applied = Vec::new();
    let mut failed = Vec::new();
    for (id, value) in &update.values {
        let Some(key) = crate::settings::daemon_key(id) else {
            failed.push(ConfigFailure {
                id: id.clone(),
                error: "unknown setting".to_owned(),
            });
            continue;
        };
        if let Err(error) = key.kind.validate(value) {
            failed.push(ConfigFailure {
                id: id.clone(),
                error,
            });
            continue;
        }
        match state.backend.config_set(key.key, value.trim()).await {
            Ok(()) => applied.push(id.clone()),
            Err(e) => failed.push(ConfigFailure {
                id: id.clone(),
                error: e.to_string(),
            }),
        }
    }
    if !applied.is_empty() {
        state.repoll.notify_waiters();
    }
    Json(ConfigUpdateResult { applied, failed }).into_response()
}

/// `GET /api/settings` → the server-owned interface preferences plus the server
/// facts the General section shows.
async fn get_settings(State(state): State<Arc<AppState>>) -> Response {
    Json(serde_json::json!({
        "ui": state.ui.load(),
        "server": {
            "displayName": state.config.display_name,
            "savePath": state.config.save_path,
            "listen": state.config.listen.to_string(),
            "pollMs": state.config.poll_ms,
            "authMode": state.config.auth_mode,
            "mock": state.config.mock,
            // Bandwidth rules (V3-18) for the precedence display; keys are
            // camelCase to match the TS contract (the struct stays snake
            // for TOML/settings-file convention).
            "bandwidthRules": state.config.bandwidth_rules.iter().map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "tag": r.tag,
                    "label": r.label,
                    "downKb": r.down_kb,
                    "upKb": r.up_kb,
                    "peersMax": r.peers_max,
                    "peersMin": r.peers_min,
                    "uploadsMax": r.uploads_max,
                })
            }).collect::<Vec<_>>(),
        }
    }))
    .into_response()
}

/// `PUT /api/settings` — merge and persist the interface preferences.
async fn put_settings(
    State(state): State<Arc<AppState>>,
    Json(update): Json<crate::settings::UiSettings>,
) -> Response {
    let mut current = state.ui.load();
    if update.theme.is_some() {
        current.theme = update.theme;
    }
    if update.accent.is_some() {
        current.accent = update.accent;
    }
    if update.density.is_some() {
        current.density = update.density;
    }
    if update.date_format.is_some() {
        current.date_format = update.date_format;
    }
    if update.rate_format.is_some() {
        current.rate_format = update.rate_format;
    }
    if update.columns.is_some() {
        current.columns = update.columns;
    }
    if let Err(error) = current.validate() {
        return ApiError::bad(error).into_response();
    }
    match state.ui.save(&current) {
        Ok(()) => Json(current).into_response(),
        Err(e) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("could not save settings: {e}"),
        )
        .into_response(),
    }
}

#[derive(Deserialize)]
struct PasswordChange {
    current: String,
    new: String,
}

/// `POST /api/settings/password` — verify the current password, rehash the new
/// one, and take effect without a restart. A wrong current password gets one
/// generic message, so nothing is leaked about the stored value.
async fn post_password(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PasswordChange>,
) -> Response {
    if state.config.auth_mode == AuthMode::None {
        return ApiError::bad("this server runs with authentication disabled").into_response();
    }
    let Some(hash) = state.password_hash() else {
        return ApiError::new(
            StatusCode::UNAUTHORIZED,
            "the current password is incorrect",
        )
        .into_response();
    };
    if !crate::auth::verify_password(&hash, &body.current) {
        return ApiError::new(
            StatusCode::UNAUTHORIZED,
            "the current password is incorrect",
        )
        .into_response();
    }
    if body.new.chars().count() < 8 {
        return ApiError::bad("the new password must be at least 8 characters").into_response();
    }
    let new_hash = match crate::auth::hash_password(&body.new) {
        Ok(hash) => hash,
        Err(e) => {
            return ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    };
    state.set_password_hash(new_hash.clone());
    if let Some(path) = &state.config.config_path {
        if let Err(e) = crate::settings::write_password_hash(path, &new_hash) {
            return ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "the password changed for this run, but could not be written to {}: {e}",
                    path.display()
                ),
            )
            .into_response();
        }
    }
    tracing::info!(event = "auth.password_change", "web password changed");
    StatusCode::NO_CONTENT.into_response()
}

/// `GET /api/settings/advanced` → the daemon's managed `.rtorrent.rc` block,
/// read-only. `block` is `None` when the file has none; the surface never
/// invents one.
async fn get_advanced() -> Response {
    let path = home_dir().join(".rtorrent.rc");
    let text = tokio::fs::read_to_string(&path).await.ok();
    let block = text.as_deref().and_then(crate::settings::managed_block);
    Json(serde_json::json!({
        "path": path.to_string_lossy(),
        "exists": text.is_some(),
        "block": block,
    }))
    .into_response()
}

fn home_dir() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/"))
}

// --- Stats (WC8) -------------------------------------------------------------

/// Counts by lifecycle state, from the cached snapshot.
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct StateCounts {
    total: i64,
    downloading: i64,
    seeding: i64,
    completed: i64,
    stopped: i64,
    checking: i64,
    errored: i64,
    stalled: i64,
}

/// One configured volume's usage; `free`/`total` are `None` when the path cannot
/// be read, which the surface shows as unavailable rather than zero.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VolumeStat {
    path: String,
    free: Option<i64>,
    total: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatsPayload {
    /// Up to 60 minutes of global-rate samples, oldest first.
    history: Vec<crate::history::StatsSample>,
    poll_ms: u64,
    session_down: i64,
    session_up: i64,
    session_ratio: Option<f64>,
    counts: StateCounts,
    uptime_seconds: i64,
    dht_nodes: i64,
    port_range: Option<String>,
    /// The external port check (WC11) owns this; `null` means "not checked".
    port_status: Option<bool>,
    volumes: Vec<VolumeStat>,
}

/// `GET /api/stats` — the Stats route's data: the rate history, session totals,
/// state counts, uptime and volumes. A cold server returns a short but valid
/// history rather than an error.
async fn get_stats(State(state): State<Arc<AppState>>) -> Response {
    state.mark_request();
    let stats = state.backend.statistics().await.ok();
    let (session_down, session_up) = stats
        .as_ref()
        .map_or((0, 0), |s| (s.session_down, s.session_up));
    let session_ratio = (session_down > 0).then(|| session_up as f64 / session_down as f64);

    let (counts, dht_nodes) = {
        let cache = state.cache.read().unwrap();
        match cache.as_ref() {
            Some(cached) => (
                count_states(&cached.snapshot.torrents),
                cached.snapshot.globals.dht_nodes,
            ),
            None => (StateCounts::default(), 0),
        }
    };

    let port_range = state
        .backend
        .config_get(&["network.port_range"])
        .await
        .ok()
        .and_then(|values| values.into_iter().next().flatten());

    Json(StatsPayload {
        history: state.rate_history(),
        poll_ms: state.config.poll_ms,
        session_down,
        session_up,
        session_ratio,
        counts,
        uptime_seconds: state.uptime_seconds(),
        dht_nodes,
        port_range,
        port_status: None,
        volumes: volume_stats(&state.config),
    })
    .into_response()
}

/// Count torrents by state from the snapshot's DTOs.
fn count_states(torrents: &[rtorrent_core::types::TorrentDto]) -> StateCounts {
    use rtorrent_core::types::Status;
    let mut counts = StateCounts {
        total: torrents.len() as i64,
        ..StateCounts::default()
    };
    for torrent in torrents {
        match torrent.status {
            Status::Downloading => counts.downloading += 1,
            Status::Seeding => counts.seeding += 1,
            Status::Completed => counts.completed += 1,
            Status::Paused => counts.stopped += 1,
            Status::Checking => counts.checking += 1,
            Status::Error => counts.errored += 1,
            Status::Stalled => counts.stalled += 1,
        }
    }
    counts
}

/// Probe each configured volume. In mock mode with none configured, serve one
/// healthy and one gone fixture so the surface is exercisable offline.
fn volume_stats(config: &crate::config::Config) -> Vec<VolumeStat> {
    const GIB: i64 = 1024 * 1024 * 1024;
    let paths: Vec<String> = if config.mock && config.volumes.is_empty() {
        vec!["/srv/downloads".to_owned(), "/mnt/archive".to_owned()]
    } else {
        config.volumes.clone()
    };
    paths
        .into_iter()
        .map(|path| {
            let usage = if config.mock {
                (path == "/srv/downloads").then_some((412 * GIB, 1114 * GIB))
            } else {
                crate::disk::disk_usage(&path)
            };
            match usage {
                Some((free, total)) => VolumeStat {
                    path,
                    free: Some(free),
                    total: Some(total),
                },
                None => VolumeStat {
                    path,
                    free: None,
                    total: None,
                },
            }
        })
        .collect()
}

// --- Library search (V3-12) --------------------------------------------------

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResults {
    hashes: Vec<String>,
    /// How many torrents are indexed so far, so the client can say so while the
    /// index is still filling.
    indexed: usize,
}

/// `GET /api/search?q=` — the hashes whose **contained filenames** match. The
/// client already matches the cheap fields (name, label, tags, tracker, path)
/// locally; this covers the part it cannot compute without one RPC per torrent.
async fn get_search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Response {
    let needle = query.q.trim().to_lowercase();
    let index = state.file_index.lock().unwrap();
    let indexed = index.len();
    if needle.is_empty() {
        return Json(SearchResults {
            hashes: Vec::new(),
            indexed,
        })
        .into_response();
    }
    let cache = state.cache.read().unwrap();
    let hashes = cache.as_ref().map_or_else(Vec::new, |cached| {
        cached
            .snapshot
            .torrents
            .iter()
            .filter(|torrent| index.contains(&torrent.hash, &needle))
            .map(|torrent| torrent.hash.clone())
            .collect()
    });
    Json(SearchResults { hashes, indexed }).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthMode, Config};
    use axum::body::to_bytes;
    use http_body_util::BodyExt;
    use rtorrent_core::rtorrent::mock::MockClient;
    use rtorrent_core::types::Transport;
    use tower::ServiceExt;

    fn mock_state_with(transport: Transport) -> Arc<AppState> {
        let config = Config {
            listen: "127.0.0.1:9080".parse().unwrap(),
            transport,
            daemon_password: None,
            auth_mode: AuthMode::None,
            password_hash: None,
            display_name: "sy".into(),
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
            poll_ms: 1000,
            assets_dir: None,
            mock: true,
            config_path: None,
            hardening: Default::default(),
        };
        Arc::new(AppState::new(config, Box::new(MockClient::new())))
    }

    fn mock_state() -> Arc<AppState> {
        mock_state_with(Transport::UnixSocket {
            path: String::new(),
        })
    }

    async fn post_json(
        app: Router,
        path: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status();
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    /// A hash that actually exists in the primed cache.
    fn a_hash(state: &Arc<AppState>) -> String {
        state
            .cache
            .read()
            .unwrap()
            .as_ref()
            .unwrap()
            .snapshot
            .torrents[0]
            .hash
            .clone()
    }

    #[tokio::test]
    async fn state_serves_the_fixtures_then_304s_on_matching_etag() {
        let state = mock_state();
        // Prime the cache the way the poller would.
        crate::poller::run_one_for_test(&state).await;

        let app = router(state.clone());

        // First request: 200 with an ETag and the mock fixture set.
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let etag = res
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let snap: Snapshot = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            snap.torrents.len(),
            rtorrent_core::rtorrent::mock::FIXTURE_COUNT,
            "the design's fixture rows"
        );

        // Second request with the ETag: 304, no body.
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/state")
                    .header(header::IF_NONE_MATCH, &etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty(), "304 carries no body");
    }

    #[tokio::test]
    async fn delta_serves_incremental_update_and_heals_on_miss() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        // Second poll to create a delta (tick 1, not a full reconciliation).
        // Manually bump revision and create a delta for testing without needing tick.
        let snap = state
            .cache
            .read()
            .unwrap()
            .as_ref()
            .unwrap()
            .snapshot
            .clone();
        let rev = snap.revision;
        let mut next = (*snap).clone();
        next.revision = rev + 1;
        // Simulate a small change: toggle a label.
        if let Some(t) = next.torrents.first_mut() {
            t.label = "delta-test".into();
        }
        let delta = rtorrent_core::delta::diff(&snap, &next);
        let dbody = serde_json::to_vec(&delta).unwrap();
        let detag = crate::state::etag_of(&dbody);
        *state.delta_cache.write().unwrap() = Some(crate::state::CachedDelta {
            etag: detag.clone(),
            body: dbody.into(),
            delta: std::sync::Arc::new(delta),
            at: std::time::Instant::now(),
        });
        // Also update the current cache to the next revision so delta base matches.
        let sbody = serde_json::to_vec(&next).unwrap();
        let setag = crate::state::etag_of(&sbody);
        *state.cache.write().unwrap() = Some(crate::state::Cached {
            etag: setag.clone(),
            body: sbody.into(),
            snapshot: std::sync::Arc::new(next),
            at: std::time::Instant::now(),
        });

        let app = router(state.clone());

        // Correct since → 200 delta
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/api/delta?since={rev}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let d: rtorrent_core::types::SnapshotDelta = serde_json::from_slice(&body).unwrap();
        assert_eq!(d.base_revision, rev);

        // Already current → 304
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/api/delta?since={}", rev + 1))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_MODIFIED);

        // Missed revision → 409 with full snapshot
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/delta?since=999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let s: Snapshot = serde_json::from_slice(&body).unwrap();
        assert_eq!(s.revision, rev + 1);
    }

    #[tokio::test]
    async fn health_reports_server_identity() {
        let state = mock_state();
        let app = router(state);
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["server"]["displayName"], "sy");
    }

    #[tokio::test]
    async fn cmd_rejects_while_disconnected() {
        // No poll yet → phase is "connecting", not "connected".
        let state = mock_state();
        let (status, _) = post_json(
            router(state),
            "/api/cmd/start",
            serde_json::json!({ "hashes": ["X"] }),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn cmd_start_ok_when_connected() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        let hash = a_hash(&state);
        let (status, body) = post_json(
            router(state),
            "/api/cmd/start",
            serde_json::json!({ "hashes": [hash] }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.is_null());
    }

    #[tokio::test]
    async fn copy_magnet_returns_a_uri() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        let hash = a_hash(&state);
        let (status, body) = post_json(
            router(state),
            "/api/cmd/copy_magnet",
            serde_json::json!({ "hash": hash }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.as_str().unwrap().starts_with("magnet:?xt=urn:btih:"));
    }

    #[tokio::test]
    async fn delete_data_forbidden_off_box() {
        // An HTTP transport is not co-located, so delete-data is refused.
        let state = mock_state_with(Transport::Http {
            url: "https://box.example/RPC2".into(),
            username: String::new(),
        });
        crate::poller::run_one_for_test(&state).await;
        let hash = a_hash(&state);
        let (status, _) = post_json(
            router(state),
            "/api/cmd/remove",
            serde_json::json!({ "hashes": [hash], "deleteData": true }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn set_location_move_data_forbidden_off_box() {
        let state = mock_state_with(Transport::Http {
            url: "https://box.example/RPC2".into(),
            username: String::new(),
        });
        crate::poller::run_one_for_test(&state).await;
        let hash = a_hash(&state);
        let (status, _) = post_json(
            router(state),
            "/api/cmd/set_location",
            serde_json::json!({ "hash": hash, "path": "/new/dir", "moveData": true }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn set_location_ok_when_colocated() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        let hash = a_hash(&state);
        let (status, _) = post_json(
            router(state.clone()),
            "/api/cmd/set_location",
            serde_json::json!({ "hash": hash, "path": "/srv/new_dl", "moveData": false }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn toggle_force_start_sets_and_clears_the_flag() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        let hash = a_hash(&state);
        let forced = |state: Arc<AppState>, hash: String| async move {
            state
                .backend
                .list_snapshot()
                .await
                .unwrap()
                .into_iter()
                .find(|t| t.hash == hash)
                .unwrap()
                .force_start
        };
        let (status, _) = post_json(
            router(state.clone()),
            "/api/cmd/toggle_force_start",
            serde_json::json!({ "hashes": [&hash] }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            forced(state.clone(), hash.clone()).await,
            "first toggle switches on"
        );
        let (status, _) = post_json(
            router(state.clone()),
            "/api/cmd/toggle_force_start",
            serde_json::json!({ "hashes": [&hash] }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            !forced(state.clone(), hash.clone()).await,
            "second toggle switches off"
        );
    }

    #[tokio::test]
    async fn queue_move_top_pins_priority_and_sequence() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        let hash = a_hash(&state);
        let (status, _) = post_json(
            router(state.clone()),
            "/api/cmd/queue_move",
            serde_json::json!({ "hashes": [hash], "direction": "top" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let row = state
            .backend
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.hash == hash)
            .unwrap();
        assert_eq!(row.priority, 3);
        assert!(row.queue_pos.is_some(), "top materialises a sequence value");
    }

    #[tokio::test]
    async fn queue_move_rejects_unknown_directions() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        let (status, _) = post_json(
            router(state),
            "/api/cmd/queue_move",
            serde_json::json!({ "hashes": ["AA"], "direction": "sideways" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn set_location_clears_recorded_final_dir() {
        // A manual move wins over automation (V3-14): the recorded intent must
        // go so a later completion does not drag the torrent back. Use an
        // incomplete torrent so the tick's stale-intent settling stays out.
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        let rows = state.backend.list_snapshot().await.unwrap();
        let hash = rows
            .iter()
            .find(|t| !t.complete)
            .expect("an incomplete fixture")
            .hash
            .clone();
        state
            .backend
            .set_custom_metadata(&hash, &[("final_dir", "/srv/home")])
            .await
            .unwrap();
        let (status, _) = post_json(
            router(state.clone()),
            "/api/cmd/set_location",
            serde_json::json!({ "hash": hash, "path": "/srv/manual", "moveData": false }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let rows = state.backend.list_snapshot().await.unwrap();
        let row = rows.iter().find(|t| t.hash == hash).unwrap();
        assert!(row.final_dir.is_empty(), "manual set wins over intent");
    }

    #[tokio::test]
    async fn moves_empty_initially_and_cancel_retry_report_false() {
        // Isolated state dir: the journal path derives from config_path, and
        // parallel tests must not share it.
        let dir = tempfile::tempdir().unwrap();
        let state = mock_state_in(dir.path());
        crate::poller::run_one_for_test(&state).await;
        let app = router(state);
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/moves")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let moves: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(moves.is_empty());

        let (status, body) = post_json(
            app.clone(),
            "/api/cmd/cancel_move",
            serde_json::json!({ "id": "NOPE" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::Value::Bool(false));

        let (status, body) = post_json(
            app,
            "/api/cmd/retry_move",
            serde_json::json!({ "hash": "NOPE" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::Value::Bool(false));
    }

    #[tokio::test]
    async fn session_export_validate_import_round_trip() {
        // Export → validate → import(nothing selected) → status, all against
        // the mock backend. The empty selection keeps the detached job to
        // planning only, so the test stays fast.
        let dir = tempfile::tempdir().unwrap();
        let state = mock_state_in(dir.path());
        crate::poller::run_one_for_test(&state).await;
        let app = router(state);

        let (status, body) = post_json(
            app.clone(),
            "/api/cmd/export_session_text",
            serde_json::json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let text = body.as_str().expect("manifest text").to_owned();
        let manifest: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(manifest["format"], "rstorrent-session/1");
        assert!(!manifest["torrents"].as_array().unwrap().is_empty());

        let (status, body) = post_json(
            app.clone(),
            "/api/cmd/validate_session",
            serde_json::json!({ "manifestText": text, "selected": [] }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let total = body["torrentCount"].as_u64().unwrap();
        assert!(total >= 1);
        assert!(
            body["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|i| i["action"] != "add"),
            "empty selection adds nothing (mock entries are already loaded or sourceless)"
        );

        let (status, _) = post_json(
            app.clone(),
            "/api/cmd/import_session",
            serde_json::json!({ "manifestText": text, "selected": [] }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // The detached job must finish promptly with everything skipped.
        let mut done = serde_json::Value::Null;
        for _ in 0..100 {
            let (status, body) =
                post_json(app.clone(), "/api/cmd/import_status", serde_json::json!({})).await;
            assert_eq!(status, StatusCode::OK);
            if body["running"] == false && body["done"] == true {
                done = body;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(done["done"], true);
        assert_eq!(done["skipped"], total);

        let (status, _) = post_json(app, "/api/cmd/cancel_import", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn session_scan_names_its_client_and_problems() {
        let dir = tempfile::tempdir().unwrap();
        let state = mock_state_in(dir.path());
        crate::poller::run_one_for_test(&state).await;
        let app = router(state);

        let (status, _) = post_json(
            app.clone(),
            "/api/cmd/scan_foreign",
            serde_json::json!({ "client": "utorrent", "dir": dir.path() }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // An empty folder scans cleanly with an explanatory problem, never an error.
        let (status, body) = post_json(
            app,
            "/api/cmd/scan_foreign",
            serde_json::json!({ "client": "qbittorrent", "dir": dir.path() }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["client"], "qbittorrent");
        assert_eq!(body["entryCount"], 0);
        assert!(!body["problems"].as_array().unwrap().is_empty());
        assert!(body["manifestText"]
            .as_str()
            .unwrap()
            .contains("rstorrent-session/1"));
    }

    #[tokio::test]
    async fn completed_torrent_with_final_dir_moves_on_the_next_tick() {
        let dir = tempfile::tempdir().unwrap();
        let incomplete = dir.path().join("incomplete");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&incomplete).unwrap();

        let config = Config {
            listen: "127.0.0.1:9080".parse().unwrap(),
            transport: Transport::UnixSocket {
                path: String::new(),
            },
            daemon_password: None,
            auth_mode: AuthMode::None,
            password_hash: None,
            display_name: "sy".into(),
            save_path: String::new(),
            volumes: Vec::new(),
            incomplete_dir: incomplete.to_string_lossy().into_owned(),
            move_rules: Vec::new(),
            collision_policy: Default::default(),
            bandwidth_rules: Vec::new(),
            max_active_downloads: 0,
            max_active_uploads: 0,
            max_active_torrents: 0,
            queue_slow_limit_kbs: 0,
            poll_ms: 1000,
            assets_dir: None,
            mock: true,
            // Isolated journal: parallel tests must not share the CWD file.
            config_path: Some(dir.path().join("rstorrent-web.toml")),
            hardening: Default::default(),
        };
        let state = Arc::new(AppState::new(config, Box::new(MockClient::new())));
        crate::poller::run_one_for_test(&state).await;

        // A complete fixture with real bytes in the incomplete dir and a
        // recorded intent home — exactly what the routed add path produces.
        let rows = state.backend.list_snapshot().await.unwrap();
        let picked = rows
            .iter()
            .find(|t| t.complete)
            .expect("a complete fixture")
            .clone();
        state
            .backend
            .set_directory(&picked.hash, &incomplete.to_string_lossy())
            .await
            .unwrap();
        std::fs::write(incomplete.join(&picked.name), b"moved bytes").unwrap();
        state
            .backend
            .set_custom_metadata(&picked.hash, &[("final_dir", &home.to_string_lossy())])
            .await
            .unwrap();

        crate::poller::run_one_for_test(&state).await;

        // The move runs detached; settle until the journal says Done.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let done = state.moves.lock().unwrap().journal().ops.iter().any(|op| {
                op.hash == picked.hash && op.state == rtorrent_core::complete::MoveState::Done
            });
            if done {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the move did not finish"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let rows = state.backend.list_snapshot().await.unwrap();
        let row = rows.iter().find(|t| t.hash == picked.hash).unwrap();
        assert_eq!(row.directory, home.to_string_lossy());
        assert!(row.final_dir.is_empty(), "intent consumed");
        assert_eq!(
            std::fs::read(home.join(&picked.name)).unwrap(),
            b"moved bytes"
        );
        assert!(!incomplete.join(&picked.name).exists(), "source cleaned up");

        // And the moves endpoint reports an empty board afterwards (Done is
        // pruned; the log line is the record).
        let res = router(state)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/moves")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let moves: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(moves.is_empty());
    }

    #[tokio::test]
    async fn create_torrent_forbidden_off_box() {
        let state = mock_state_with(Transport::Http {
            url: "https://box.example/RPC2".into(),
            username: String::new(),
        });
        crate::poller::run_one_for_test(&state).await;
        let (status, _) = post_json(
            router(state),
            "/api/cmd/create_torrent",
            serde_json::json!({
                "sourcePath": "/tmp/test.txt",
                "trackers": ["http://tracker.example.com/announce"],
                "isPrivate": false,
                "startSeeding": false,
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn create_torrent_ok_when_colocated() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;

        let temp_dir = tempfile::tempdir().unwrap();
        let sample_file = temp_dir.path().join("file.bin");
        std::fs::write(&sample_file, b"content for web create_torrent test").unwrap();
        let out_file = temp_dir.path().join("file.torrent");

        let (status, body) = post_json(
            router(state),
            "/api/cmd/create_torrent",
            serde_json::json!({
                "sourcePath": sample_file.to_string_lossy(),
                "outputPath": out_file.to_string_lossy(),
                "trackers": ["http://tracker.example.com/announce"],
                "isPrivate": false,
                "startSeeding": false,
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.get("infoHash").is_some());
        assert!(out_file.exists());
    }

    #[tokio::test]
    async fn unknown_command_is_404() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        let (status, _) = post_json(router(state), "/api/cmd/bogus", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn bad_args_are_400() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        // `start` needs a `hashes` array.
        let (status, _) = post_json(
            router(state),
            "/api/cmd/start",
            serde_json::json!({ "nope": true }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    async fn get_json(app: Router, path: &str) -> (StatusCode, serde_json::Value) {
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status();
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    #[tokio::test]
    async fn detail_serves_a_tab_payload() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        let hash = a_hash(&state);
        let (status, body) = get_json(
            router(state),
            &format!("/api/detail?hash={hash}&tab=general"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["hash"], hash);
        // General carries the pieces payload.
        assert!(body.get("pieces").is_some());
    }

    #[tokio::test]
    async fn detail_rejects_an_unknown_tab() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        let (status, _) = get_json(router(state), "/api/detail?hash=X&tab=bogus").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn log_returns_new_entries_then_nothing() {
        let state = mock_state();
        state.log(rtorrent_core::types::LogLevel::Info, "hello world", None);

        let (status, body) = get_json(router(state.clone()), "/api/log?after=0").await;
        assert_eq!(status, StatusCode::OK);
        let seq = body["seq"].as_u64().unwrap();
        assert!(seq >= 1);
        assert!(body["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["message"] == "hello world"));

        // Asking again after the high-water mark yields nothing new.
        let (_, body) = get_json(router(state), &format!("/api/log?after={seq}")).await;
        assert!(body["entries"].as_array().unwrap().is_empty());
    }

    // --- Auth (WE5) ---------------------------------------------------------

    fn password_state() -> Arc<AppState> {
        let hash = crate::auth::hash_password("s3cret").unwrap();
        let config = Config {
            listen: "127.0.0.1:9080".parse().unwrap(),
            transport: Transport::UnixSocket {
                path: String::new(),
            },
            daemon_password: None,
            auth_mode: AuthMode::Password,
            password_hash: Some(hash),
            display_name: "sy".into(),
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
            poll_ms: 1000,
            assets_dir: None,
            mock: true,
            config_path: None,
            hardening: Default::default(),
        };
        Arc::new(AppState::new(config, Box::new(MockClient::new())))
    }

    fn base(method: &str, path: &str) -> axum::http::request::Builder {
        axum::http::Request::builder()
            .method(method)
            .uri(path)
            .extension(axum::extract::ConnectInfo(
                "127.0.0.1:5555".parse::<std::net::SocketAddr>().unwrap(),
            ))
    }

    fn login_req(password: &str) -> axum::http::Request<Body> {
        base("POST", "/api/session")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({ "password": password }).to_string(),
            ))
            .unwrap()
    }

    /// Extract the `rstorrent_session=<token>` pair from a Set-Cookie header.
    fn session_cookie(res: &Response) -> String {
        let sc = res
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        sc.split(';').next().unwrap().to_string()
    }

    #[tokio::test]
    async fn login_flow_gates_state() {
        let state = password_state();
        crate::poller::run_one_for_test(&state).await;
        let app = router(state);

        // Unauthenticated read → 401.
        let res = app
            .clone()
            .oneshot(base("GET", "/api/state").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // Wrong password → 401.
        let res = app.clone().oneshot(login_req("nope")).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // Right password → 204 + Set-Cookie.
        let res = app.clone().oneshot(login_req("s3cret")).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let cookie = session_cookie(&res);

        // Authenticated read → 200.
        let res = app
            .oneshot(
                base("GET", "/api/state")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn mutation_requires_csrf_header() {
        let state = password_state();
        crate::poller::run_one_for_test(&state).await;
        let token = state.sessions.create();
        let cookie = format!("{}={token}", crate::auth::COOKIE);
        let app = router(state);
        let hash_hint = serde_json::json!({ "hashes": ["X"] }).to_string();

        // Authenticated POST without the header → 403.
        let res = app
            .clone()
            .oneshot(
                base("POST", "/api/cmd/start")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(hash_hint.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        // With the header it passes the gate (reaches the command layer).
        let res = app
            .oneshot(
                base("POST", "/api/cmd/start")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("X-Rstorrent", "1")
                    .body(Body::from(hash_hint))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn logout_revokes_the_session() {
        let state = password_state();
        crate::poller::run_one_for_test(&state).await;
        let token = state.sessions.create();
        let cookie = format!("{}={token}", crate::auth::COOKIE);
        let app = router(state);

        let res = app
            .clone()
            .oneshot(
                base("DELETE", "/api/session")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        // The revoked cookie no longer authenticates.
        let res = app
            .oneshot(
                base("GET", "/api/state")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn login_is_rate_limited() {
        let state = password_state();
        let app = router(state);
        // 5 attempts allowed, the 6th is throttled.
        for _ in 0..5 {
            let res = app.clone().oneshot(login_req("nope")).await.unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        }
        let res = app.oneshot(login_req("nope")).await.unwrap();
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn inspect_rejects_non_torrent_bytes() {
        let state = mock_state(); // AuthMode::None → no session needed
        crate::poller::run_one_for_test(&state).await;
        let boundary = "X-BOUNDARY";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"bad.torrent\"\r\n\r\nnot a torrent\r\n--{boundary}--\r\n"
        );
        let res = router(state)
            .oneshot(
                base("POST", "/api/torrents/inspect")
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    // --- Settings (WC7) -----------------------------------------------------

    /// A mock state whose UI-settings file lives in a scratch directory, so a
    /// test never writes into the repository.
    fn mock_state_in(dir: &std::path::Path) -> Arc<AppState> {
        let config = Config {
            listen: "127.0.0.1:9080".parse().unwrap(),
            transport: Transport::UnixSocket {
                path: String::new(),
            },
            daemon_password: None,
            auth_mode: AuthMode::None,
            password_hash: None,
            display_name: "sy".into(),
            save_path: "/srv/downloads".into(),
            volumes: Vec::new(),
            incomplete_dir: String::new(),
            move_rules: Vec::new(),
            collision_policy: Default::default(),
            bandwidth_rules: Vec::new(),
            max_active_downloads: 0,
            max_active_uploads: 0,
            max_active_torrents: 0,
            queue_slow_limit_kbs: 0,
            poll_ms: 1000,
            assets_dir: None,
            mock: true,
            config_path: Some(dir.join("rstorrent-web.toml")),
            hardening: Default::default(),
        };
        Arc::new(AppState::new(config, Box::new(MockClient::new())))
    }

    async fn put_json(
        app: Router,
        path: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .method("PUT")
                    .uri(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("X-Rstorrent", "1")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status();
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn config_lists_keys_with_values_and_unavailable_ones() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        let (status, json) = get_json(router(state), "/api/config").await;
        assert_eq!(status, StatusCode::OK);
        let rows = json.as_array().unwrap();
        assert_eq!(rows.len(), crate::settings::DAEMON_KEYS.len());
        assert!(rows
            .iter()
            .all(|row| row["section"].is_string() && row["key"].is_string()));

        let port = rows.iter().find(|row| row["id"] == "port_range").unwrap();
        assert_eq!(port["value"], "6881-6899");
        assert_eq!(port["available"], true);
        assert_eq!(port["kind"]["type"], "str");
    }

    #[tokio::test]
    async fn config_write_reports_per_key_outcomes() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        let (status, json) = post_json(
            router(state.clone()),
            "/api/config",
            serde_json::json!({ "values": {
                "port_range": "7000-7010",
                "max_open_files": "not a number",
                "no_such_setting": "1",
            }}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let applied = json["applied"].as_array().unwrap();
        assert!(applied.iter().any(|id| id == "port_range"), "{json}");
        let failed = json["failed"].as_array().unwrap();
        assert!(failed.iter().any(|f| f["id"] == "max_open_files"));
        assert!(failed.iter().any(|f| f["id"] == "no_such_setting"));

        // The accepted write is visible to the next read.
        let (_, cfg) = get_json(router(state), "/api/config").await;
        let port = cfg
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "port_range")
            .unwrap();
        assert_eq!(port["value"], "7000-7010");
    }

    #[tokio::test]
    async fn settings_persist_ui_preferences_and_reject_bad_ones() {
        let dir = std::env::temp_dir().join(format!("rstorrent-web-api-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let state = mock_state_in(&dir);

        let (status, json) = put_json(
            router(state.clone()),
            "/api/settings",
            serde_json::json!({ "theme": "light", "density": "comfortable" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["theme"], "light");

        // It round-trips from disk on the next read.
        let (status, settings) = get_json(router(state.clone()), "/api/settings").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(settings["ui"]["theme"], "light");
        assert_eq!(settings["ui"]["density"], "comfortable");
        assert_eq!(settings["server"]["displayName"], "sy");

        // A value the console cannot apply is rejected, naming no partial write.
        let (status, _) = put_json(
            router(state),
            "/api/settings",
            serde_json::json!({ "theme": "solarized" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn settings_report_bandwidth_rules_in_ts_shape() {
        let mut config = mock_state().config.clone();
        config.bandwidth_rules = vec![rtorrent_core::bandwidth::BandwidthRule::label_rule(
            "vid", "video", 1024, 256,
        )];
        let state = Arc::new(AppState::new(config, Box::new(MockClient::new())));
        let (_, settings) = get_json(router(state), "/api/settings").await;
        let rules = settings["server"]["bandwidthRules"]
            .as_array()
            .expect("bandwidthRules array");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["id"], "vid");
        assert_eq!(rules[0]["downKb"], 1024);
        assert_eq!(rules[0]["label"], "video");
    }

    #[tokio::test]
    async fn password_change_verifies_the_current_one_and_takes_effect() {
        let state = password_state();
        let token = state.sessions.create();
        let cookie = format!("{}={token}", crate::auth::COOKIE);
        let app = router(state.clone());
        let change = |current: &str, new: &str| {
            base("POST", "/api/settings/password")
                .header(header::COOKIE, &cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .header("X-Rstorrent", "1")
                .body(Body::from(
                    serde_json::json!({ "current": current, "new": new }).to_string(),
                ))
                .unwrap()
        };

        // Wrong current password: generic 401, hash unchanged.
        let res = app
            .clone()
            .oneshot(change("nope", "longenough"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert!(crate::auth::verify_password(
            &state.password_hash().unwrap(),
            "s3cret"
        ));

        // Correct: 204, and the new password verifies.
        let res = app.oneshot(change("s3cret", "longenough")).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let hash = state.password_hash().unwrap();
        assert!(crate::auth::verify_password(&hash, "longenough"));
        assert!(!crate::auth::verify_password(&hash, "s3cret"));
    }

    #[tokio::test]
    async fn password_change_rejects_a_short_new_password() {
        let state = password_state();
        let token = state.sessions.create();
        let cookie = format!("{}={token}", crate::auth::COOKIE);
        let res = router(state)
            .oneshot(
                base("POST", "/api/settings/password")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("X-Rstorrent", "1")
                    .body(Body::from(
                        serde_json::json!({ "current": "s3cret", "new": "short" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn stats_reports_history_counts_and_volumes() {
        let dir = std::env::temp_dir().join(format!("rstorrent-web-stats-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let state = mock_state_in(&dir);
        // Tick 0 is a slow tick, so one history sample is recorded.
        crate::poller::run_one_for_test(&state).await;

        let (status, json) = get_json(router(state), "/api/stats").await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert!(
            !json["history"].as_array().unwrap().is_empty(),
            "a primed server has at least one sample"
        );
        assert_eq!(
            json["counts"]["total"],
            rtorrent_core::rtorrent::mock::FIXTURE_COUNT as i64
        );
        assert!(json["sessionDown"].as_i64().unwrap() > 0);
        assert!(json["uptimeSeconds"].as_i64().unwrap() >= 0);

        // Mock fixtures: one healthy volume, one unavailable — never a fake zero.
        let volumes = json["volumes"].as_array().unwrap();
        assert_eq!(volumes.len(), 2);
        assert!(volumes.iter().any(|v| v["free"].is_number()));
        assert!(volumes.iter().any(|v| v["free"].is_null()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Deployment hardening (WEB-05/06) -----------------------------------

    /// A password-mode config whose loopback peer is a trusted proxy.
    fn hardened_config() -> Config {
        Config {
            listen: "127.0.0.1:9080".parse().unwrap(),
            transport: Transport::UnixSocket {
                path: String::new(),
            },
            daemon_password: None,
            auth_mode: AuthMode::Password,
            password_hash: Some(crate::auth::hash_password("s3cret").unwrap()),
            display_name: "sy".into(),
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
            poll_ms: 1000,
            assets_dir: None,
            mock: true,
            config_path: None,
            hardening: crate::config::Hardening {
                trusted_proxies: vec!["127.0.0.1".parse().unwrap()],
                secure_cookies: false,
            },
        }
    }

    #[test]
    fn forwarded_headers_are_only_trusted_from_a_configured_proxy() {
        let config = hardened_config();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "203.0.113.9".parse().unwrap());
        headers.insert("x-forwarded-proto", "https".parse().unwrap());

        // The trusted proxy's headers win: the real client and HTTPS are seen.
        let peer: SocketAddr = "127.0.0.1:5555".parse().unwrap();
        let ctx = client_context(&config, peer, &headers);
        assert_eq!(ctx.ip, "203.0.113.9".parse::<IpAddr>().unwrap());
        assert!(ctx.https);

        // A peer that is not a configured proxy cannot spoof either.
        let untrusted: SocketAddr = "198.51.100.1:5555".parse().unwrap();
        let ctx = client_context(&config, untrusted, &headers);
        assert_eq!(ctx.ip, untrusted.ip());
        assert!(!ctx.https);
    }

    #[tokio::test]
    async fn the_cookie_is_secure_only_over_https_or_when_forced() {
        let state = Arc::new(AppState::new(
            hardened_config(),
            Box::new(MockClient::new()),
        ));
        let login = |secure_header: bool| {
            let mut request = base("POST", "/api/session")
                .header(header::CONTENT_TYPE, "application/json")
                .header("X-Rstorrent", "1");
            if secure_header {
                request = request.header("x-forwarded-proto", "https");
            }
            request
                .body(Body::from(
                    serde_json::json!({ "password": "s3cret" }).to_string(),
                ))
                .unwrap()
        };

        // Behind the trusted proxy over HTTPS: the cookie is Secure.
        let res = router(state.clone()).oneshot(login(true)).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(cookie.contains("HttpOnly"), "{cookie}");
        assert!(cookie.contains("SameSite=Strict"), "{cookie}");
        assert!(cookie.contains("Path=/"), "{cookie}");
        assert!(cookie.contains("; Secure"), "{cookie}");

        // Plain HTTP through the proxy: no Secure flag (the browser would drop
        // it, and the operator has not forced it).
        let res = router(state).oneshot(login(false)).await.unwrap();
        let cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(!cookie.contains("; Secure"), "{cookie}");
    }

    #[tokio::test]
    async fn revoke_all_ends_every_session() {
        let state = password_state();
        let first = state.sessions.create();
        let second = state.sessions.create();
        assert_eq!(state.sessions.active(), 2);

        let res = router(state.clone())
            .oneshot(
                base("DELETE", "/api/sessions")
                    .header(header::COOKIE, format!("{}={first}", crate::auth::COOKIE))
                    .header("X-Rstorrent", "1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert_eq!(state.sessions.active(), 0);
        assert!(!state.sessions.validate(&second));
    }

    /// The automated sliver of the live-daemon certification (WEB-04): the read
    /// path against a real rtorrent. Ignored by default; run with
    /// `RSTORRENT_TEST_SOCKET=/path/to/rpc.socket cargo test -p rstorrent-web --
    /// --ignored live_read_paths`. The full manual matrix is
    /// `docs/web-live-cert.md`.
    #[tokio::test]
    #[ignore = "needs a live rtorrent: set RSTORRENT_TEST_SOCKET"]
    async fn live_read_paths() {
        let socket = std::env::var("RSTORRENT_TEST_SOCKET")
            .expect("set RSTORRENT_TEST_SOCKET to the daemon's rpc socket");
        let transport = Transport::UnixSocket {
            path: socket.clone(),
        };
        let config = Config {
            listen: "127.0.0.1:9080".parse().unwrap(),
            transport,
            daemon_password: None,
            auth_mode: AuthMode::None,
            password_hash: None,
            display_name: "live".into(),
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
            poll_ms: 1000,
            assets_dir: None,
            mock: false,
            config_path: None,
            hardening: Default::default(),
        };
        let state = Arc::new(AppState::new(
            config,
            Box::new(rtorrent_core::rtorrent::client::RpcClient::new(
                Transport::UnixSocket { path: socket },
            )),
        ));
        crate::poller::run_one_for_test(&state).await;

        let (status, state_json) = get_json(router(state.clone()), "/api/state").await;
        assert_eq!(status, StatusCode::OK, "{state_json}");
        assert_eq!(
            state_json["connection"]["phase"], "connected",
            "{state_json}"
        );
        assert!(state_json["torrents"].is_array(), "{state_json}");

        let (status, health) = get_json(router(state), "/api/health").await;
        assert_eq!(status, StatusCode::OK);
        assert!(health["daemon"]["clientVersion"].is_string(), "{health}");
    }

    #[tokio::test]
    async fn set_tags_writes_through_the_custom_namespace() {
        let state = mock_state();
        crate::poller::run_one_for_test(&state).await;
        let hash = a_hash(&state);

        let (status, _) = post_json(
            router(state.clone()),
            "/api/cmd/set_tags",
            serde_json::json!({
                "hashes": [hash.clone()],
                "tags": ["linux", " iso ", "linux"],
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // A fresh poll publishes the change; the test has no poller loop.
        crate::poller::run_one_for_test(&state).await;
        let (_, snap) = get_json(router(state), "/api/state").await;
        let row = snap["torrents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["hash"] == hash)
            .unwrap();
        assert_eq!(
            row["tags"],
            serde_json::json!(["linux", "iso"]),
            "normalised, de-duplicated, and visible in the snapshot"
        );
    }

    #[tokio::test]
    async fn search_matches_contained_filenames_from_the_index() {
        let state = mock_state();
        // Tick 0 is a slow tick, so the first batch of file lists is indexed.
        crate::poller::run_one_for_test(&state).await;

        let (status, json) = get_json(router(state.clone()), "/api/search?q=checksum").await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert!(
            !json["hashes"].as_array().unwrap().is_empty(),
            "the mock files contain CHECKSUM: {json}"
        );
        assert!(
            json["indexed"].as_i64().unwrap() >= 5,
            "one slow tick indexes a batch: {json}"
        );

        // A query no indexed file matches is empty, not an error.
        let (status, none) = get_json(router(state), "/api/search?q=zzz-no-such-file").await;
        assert_eq!(status, StatusCode::OK);
        assert!(none["hashes"].as_array().unwrap().is_empty());
    }
}
