//! The `/api/*` HTTP surface. WE1 ships the read path: `/api/state` (the cached
//! snapshot, with ETag/304) and `/api/health`. Mutations, detail, and log tail
//! land in WE3.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Multipart, Path, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Serialize;

use rtorrent_core::snapshot;
use rtorrent_core::types::{ConnPhase, DaemonHealth, DetailPayload, DetailTab, LogEntry, Snapshot};

use crate::config::AuthMode;
use crate::state::{etag_of, AppState};

/// How long a request will wait for the poller to fill a cold/stale cache.
const COLD_WAIT: Duration = Duration::from_secs(2);
/// Detail micro-cache TTL: rapid polls of the same (hash, tab) reuse this.
const DETAIL_TTL: Duration = Duration::from_millis(1000);

/// The `/api` sub-router, behind the auth middleware.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/state", get(get_state))
        .route("/api/health", get(get_health))
        .route("/api/detail", get(get_detail))
        .route("/api/log", get(get_log))
        .route("/api/cmd/{name}", post(post_cmd))
        .route("/api/torrents/inspect", post(post_inspect))
        .route("/api/torrents/file", post(post_add_file))
        .route("/api/session", post(post_session).delete(delete_session))
        // Uploaded .torrent files are small; cap at 10 MiB.
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .with_state(state)
}

// --- Upload (WE4) ------------------------------------------------------------

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
    match state
        .backend
        .load_raw(bytes, crate::cmd::load_options(Some(&opts)))
        .await
    {
        Ok(()) => {
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
    Json(body): Json<LoginBody>,
) -> Response {
    // No-auth mode (loopback dev): login is a no-op success.
    if state.config.auth_mode == AuthMode::None {
        return StatusCode::NO_CONTENT.into_response();
    }
    if !state.rate.allow(addr.ip()) {
        return ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too many attempts — wait a minute",
        )
        .into_response();
    }
    let ok = state
        .config
        .password_hash
        .as_deref()
        .map(|h| crate::auth::verify_password(h, &body.password))
        .unwrap_or(false);
    if !ok {
        // Generic message, no user enumeration surface (single user).
        return ApiError::new(StatusCode::UNAUTHORIZED, "invalid password").into_response();
    }
    let token = state.sessions.create();
    let cookie = format!(
        "{}={token}; HttpOnly; SameSite=Strict; Path=/",
        crate::auth::COOKIE
    );
    ([(header::SET_COOKIE, cookie)], StatusCode::NO_CONTENT).into_response()
}

/// `DELETE /api/session` — revoke the session and clear the cookie.
async fn delete_session(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(token) = cookie_token(&headers) {
        state.sessions.revoke(&token);
    }
    let cleared = format!(
        "{}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        crate::auth::COOKIE
    );
    ([(header::SET_COOKIE, cleared)], StatusCode::NO_CONTENT).into_response()
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
    if req.method() == Method::POST && req.headers().get("X-Rstorrent").is_none() {
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
struct LogQuery {
    after: Option<u64>,
}

#[derive(Serialize)]
struct LogResponse {
    entries: Vec<LogEntry>,
    seq: u64,
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
            poll_ms: 1000,
            assets_dir: None,
            mock: true,
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

        // First request: 200 with an ETag and the ten fixtures.
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
        assert_eq!(snap.torrents.len(), 10, "the ten design fixtures");

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
            poll_ms: 1000,
            assets_dir: None,
            mock: true,
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
}
