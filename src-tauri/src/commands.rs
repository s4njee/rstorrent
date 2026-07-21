//! Tauri command surface — the frontend → Rust RPC boundary.
//!
//! Each `#[tauri::command]` here is thin: it validates/borrows state, delegates
//! to the rtorrent backend, logs notable outcomes, and (for mutations) triggers
//! an immediate re-poll so the UI updates promptly. Errors are mapped to plain
//! strings, which surface as rejected promises on the TypeScript side.
//!
//! Command names and argument shapes must match `src/ipc/commands.ts`.

use std::collections::HashSet;
use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::ipc::{
    AddOptions, AddSource, DaemonHealth, DetailTab, FeedItem, LogLevel, Settings, Statistics,
    TorrentMeta, Transport,
};
use crate::open_requests::OpenRequestState;
use crate::rtorrent::{client::RpcClient, LoadOptions, RtorrentApi, RtorrentError};
use crate::settings;
use crate::state::AppState;
use crate::throttles;
use crate::torrent_file;

/// Shorthand for the shared-state extractor.
type St<'a> = State<'a, Arc<AppState>>;

/// Map any displayable error into the string the IPC layer returns.
fn e(err: impl std::fmt::Display) -> String {
    err.to_string()
}

/// Convert the dialog options into backend load options.
fn load_opts(opts: &AddOptions) -> LoadOptions {
    LoadOptions {
        // The save path may have come from a native picker, so it needs to be
        // expressed in the daemon's namespace before it crosses the wire.
        directory: crate::localfs::to_daemon_path(&opts.save_path)
            .unwrap_or_else(|_| opts.save_path.clone()),
        label: opts.label.clone(),
        start: opts.start,
        top_of_queue: opts.top_of_queue,
        unselected_indexes: opts.unselected_indexes.clone(),
    }
}

#[tauri::command]
pub fn read_torrent_metadata(path: String) -> Result<TorrentMeta, String> {
    torrent_file::read_metadata(&path)
}

/// Drain file/deep-link requests retained while the frontend was loading.
/// Calling this also marks the frontend ready for live open-request events.
#[tauri::command]
pub fn take_open_requests(state: State<'_, OpenRequestState>) -> Vec<String> {
    state.take_initial()
}

#[tauri::command]
pub async fn add_torrent(
    app: AppHandle,
    state: St<'_>,
    source: AddSource,
    opts: AddOptions,
) -> Result<(), String> {
    let backend = state.backend();
    let load = load_opts(&opts);
    match source {
        AddSource::File { path } => {
            let bytes = std::fs::read(&path).map_err(e)?;
            backend.load_raw(bytes, load).await.map_err(e)?;
            state.log(
                &app,
                LogLevel::Info,
                format!("added torrent from {path}"),
                None,
            );
            // Deselected files → priority 0. rtorrent addresses files by the
            // torrent's info-hash, which we read back from the .torrent. This
            // runs right after load; on a busy daemon the download may not be
            // registered yet, so failures are logged rather than fatal (the
            // torrent is already added).
            if !opts.unselected_indexes.is_empty() {
                if let Ok(meta) = torrent_file::read_metadata(&path) {
                    for &idx in &opts.unselected_indexes {
                        if let Err(err) = backend.set_file_priority(&meta.info_hash, idx, 0).await {
                            state.log(
                                &app,
                                LogLevel::Warn,
                                format!("could not deselect file {idx}: {err}"),
                                Some(meta.info_hash.clone()),
                            );
                        }
                    }
                }
            }
        }
        AddSource::Magnet { uri } => {
            backend.load_magnet(&uri, load).await.map_err(e)?;
            state.log(&app, LogLevel::Info, "added magnet", None);
        }
    }
    state.repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn start(app: AppHandle, state: St<'_>, hashes: Vec<String>) -> Result<(), String> {
    state.backend().start(&hashes).await.map_err(e)?;
    state.log(
        &app,
        LogLevel::Info,
        format!("resumed {} torrent(s)", hashes.len()),
        None,
    );
    state.repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn stop(app: AppHandle, state: St<'_>, hashes: Vec<String>) -> Result<(), String> {
    state.backend().stop(&hashes).await.map_err(e)?;
    state.log(
        &app,
        LogLevel::Info,
        format!("paused {} torrent(s)", hashes.len()),
        None,
    );
    state.repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn recheck(app: AppHandle, state: St<'_>, hashes: Vec<String>) -> Result<(), String> {
    state.backend().recheck(&hashes).await.map_err(e)?;
    state.log(
        &app,
        LogLevel::Info,
        format!("rechecking {} torrent(s)", hashes.len()),
        None,
    );
    state.repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn force_reannounce(
    app: AppHandle,
    state: St<'_>,
    hashes: Vec<String>,
) -> Result<(), String> {
    if let Err(err) = state.backend().force_reannounce(&hashes).await {
        state.log(
            &app,
            LogLevel::Error,
            format!("force reannounce failed: {err}"),
            None,
        );
        return Err(e(err));
    }
    state.log(
        &app,
        LogLevel::Info,
        format!("forced reannounce for {} torrent(s)", hashes.len()),
        None,
    );
    state.repoll.notify_one();
    state.detail_repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn add_tracker(
    app: AppHandle,
    state: St<'_>,
    hash: String,
    url: String,
) -> Result<(), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("tracker URL cannot be empty".into());
    }
    if let Err(err) = state.backend().add_tracker(&hash, url).await {
        state.log(
            &app,
            LogLevel::Error,
            format!("add tracker failed: {err}"),
            Some(hash),
        );
        return Err(e(err));
    }
    state.log(
        &app,
        LogLevel::Info,
        format!("added tracker {url}"),
        Some(hash.clone()),
    );
    refresh_trackers(&state, &hash);
    Ok(())
}

#[tauri::command]
pub async fn remove_tracker(
    app: AppHandle,
    state: St<'_>,
    hash: String,
    tracker_index: usize,
) -> Result<(), String> {
    if let Err(err) = state.backend().remove_tracker(&hash, tracker_index).await {
        state.log(
            &app,
            LogLevel::Error,
            format!("remove tracker failed: {err}"),
            Some(hash),
        );
        return Err(e(err));
    }
    state.log(
        &app,
        LogLevel::Info,
        format!("removed or disabled tracker {tracker_index}"),
        Some(hash.clone()),
    );
    refresh_trackers(&state, &hash);
    Ok(())
}

#[tauri::command]
pub async fn set_tracker_enabled(
    app: AppHandle,
    state: St<'_>,
    hash: String,
    tracker_index: usize,
    enabled: bool,
) -> Result<(), String> {
    if let Err(err) = state
        .backend()
        .set_tracker_enabled(&hash, tracker_index, enabled)
        .await
    {
        state.log(
            &app,
            LogLevel::Error,
            format!(
                "{} tracker failed: {err}",
                if enabled { "enable" } else { "disable" }
            ),
            Some(hash),
        );
        return Err(e(err));
    }
    state.log(
        &app,
        LogLevel::Info,
        format!(
            "{} tracker {tracker_index}",
            if enabled { "enabled" } else { "disabled" }
        ),
        Some(hash.clone()),
    );
    refresh_trackers(&state, &hash);
    Ok(())
}

fn refresh_trackers(state: &AppState, hash: &str) {
    state.tracker_cache.lock().unwrap().remove(hash);
    state.repoll.notify_one();
    state.detail_repoll.notify_one();
}

#[tauri::command]
pub async fn remove(
    app: AppHandle,
    state: St<'_>,
    hashes: Vec<String>,
    delete_data: bool,
) -> Result<(), String> {
    let backend = state.backend();
    let local = settings::is_localhost(&state.settings().transport);

    // Read base paths *before* erasing, so we can trash the data afterward.
    let mut paths = Vec::new();
    if delete_data && local {
        for h in &hashes {
            if let Ok(p) = backend.base_path(h).await {
                if !p.is_empty() {
                    paths.push(p);
                }
            }
        }
    }

    backend.erase(&hashes).await.map_err(e)?;

    // Move data to the Trash (never a hard delete). Failures are logged per path.
    for p in paths {
        match crate::localfs::trash(&p) {
            Ok(_) => state.log(&app, LogLevel::Info, format!("moved to Trash: {p}"), None),
            Err(err) => state.log(
                &app,
                LogLevel::Warn,
                format!("could not trash {p}: {err}"),
                None,
            ),
        }
    }
    state.log(
        &app,
        LogLevel::Info,
        format!("removed {} torrent(s)", hashes.len()),
        None,
    );
    state.repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn set_label(
    app: AppHandle,
    state: St<'_>,
    hashes: Vec<String>,
    label: String,
) -> Result<(), String> {
    state
        .backend()
        .set_label(&hashes, &label)
        .await
        .map_err(e)?;
    state.log(&app, LogLevel::Info, format!("set label '{label}'"), None);
    state.repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn set_torrent_limits(
    app: AppHandle,
    state: St<'_>,
    hashes: Vec<String>,
    down_kb: i64,
    up_kb: i64,
) -> Result<(), String> {
    if hashes.is_empty() {
        return Ok(());
    }
    if down_kb < 0 || up_kb < 0 {
        let error = "rate limits must be zero or greater";
        state.log(&app, LogLevel::Error, error, None);
        return Err(error.into());
    }

    let backend = state.backend();
    if down_kb == 0 && up_kb == 0 {
        let clear_result = async {
            backend.assign_throttle(&hashes, None).await?;
            let assignment = backend.torrent_throttle_name(&hashes[0]).await?;
            if assignment.is_empty() {
                Ok(())
            } else {
                Err(RtorrentError::Unexpected(format!(
                    "torrent still uses throttle {assignment}"
                )))
            }
        }
        .await;
        if let Err(error) = clear_result {
            state.log(
                &app,
                LogLevel::Error,
                format!("clearing per-torrent rate limit failed: {error}"),
                None,
            );
            return Err(e(error));
        }
        state.log(
            &app,
            LogLevel::Info,
            format!("cleared rate limit for {} torrent(s)", hashes.len()),
            None,
        );
        state.repoll.notify_one();
        return Ok(());
    }

    let rows = match backend.list_snapshot().await {
        Ok(rows) => rows,
        Err(error) => {
            state.log(
                &app,
                LogLevel::Error,
                format!("setting per-torrent rate limit failed: {error}"),
                None,
            );
            return Err(e(error));
        }
    };
    let active_names: HashSet<String> = rows
        .iter()
        .map(|torrent| torrent.throttle_name.clone())
        .filter(|name| !name.is_empty())
        .collect();
    let (definition, changed) = match throttles::allocate(
        &state.settings().torrent_throttles,
        &active_names,
        down_kb,
        up_kb,
    ) {
        Ok(allocation) => allocation,
        Err(error) => {
            state.log(&app, LogLevel::Error, error, None);
            return Err(error.into());
        }
    };

    let result = async {
        backend
            .define_named_throttle(&definition.name, down_kb, up_kb)
            .await?;
        backend
            .assign_throttle(&hashes, Some(&definition.name))
            .await?;
        let assignment = backend.torrent_throttle_name(&hashes[0]).await?;
        if assignment == definition.name {
            Ok(())
        } else {
            Err(RtorrentError::Unexpected(format!(
                "torrent uses throttle '{assignment}' after assignment"
            )))
        }
    }
    .await;
    if let Err(error) = result {
        state.log(
            &app,
            LogLevel::Error,
            format!("setting per-torrent rate limit failed: {error}"),
            None,
        );
        return Err(e(error));
    }

    if changed {
        if let Err(error) = state.save_throttle_definition(definition) {
            state.log(
                &app,
                LogLevel::Error,
                format!("could not persist per-torrent rate limit: {error}"),
                None,
            );
            return Err(e(error));
        }
    }
    state.log(
        &app,
        LogLevel::Info,
        format!(
            "set rate limit for {} torrent(s): down {down_kb} KiB/s, up {up_kb} KiB/s",
            hashes.len()
        ),
        None,
    );
    state.repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn set_location(
    app: AppHandle,
    state: St<'_>,
    hash: String,
    path: String,
) -> Result<(), String> {
    // rtorrent requires the torrent be closed to move its directory; stop →
    // set → start restores the prior running state. Data is NOT moved (v1).
    let path = crate::localfs::to_daemon_path(&path)?;
    let backend = state.backend();
    let one = std::slice::from_ref(&hash);
    backend.stop(one).await.map_err(e)?;
    backend.set_directory(&hash, &path).await.map_err(e)?;
    backend.start(one).await.map_err(e)?;
    state.log(
        &app,
        LogLevel::Warn,
        format!("set location to {path} (files not moved)"),
        Some(hash),
    );
    state.repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn queue_move(
    app: AppHandle,
    state: St<'_>,
    hashes: Vec<String>,
    direction: String,
) -> Result<(), String> {
    // rtorrent has no true queue order; we nudge d.priority within 0..=3.
    let backend = state.backend();
    let rows = backend.list_snapshot().await.map_err(e)?;
    for h in &hashes {
        if let Some(t) = rows.iter().find(|t| &t.hash == h) {
            let next = if direction == "up" {
                (t.priority + 1).min(3)
            } else {
                (t.priority - 1).max(0)
            };
            backend.set_priority(h, next).await.map_err(e)?;
        }
    }
    state.log(
        &app,
        LogLevel::Info,
        format!("reordered {} torrent(s)", hashes.len()),
        None,
    );
    state.repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn copy_magnet(state: St<'_>, hash: String) -> Result<String, String> {
    // Build a minimal but valid magnet (xt + dn) from the current snapshot.
    let rows = state.backend().list_snapshot().await.map_err(e)?;
    let name = rows
        .iter()
        .find(|t| t.hash.eq_ignore_ascii_case(&hash))
        .map(|t| t.name.clone())
        .unwrap_or_default();
    let dn = urlencode(&name);
    Ok(format!("magnet:?xt=urn:btih:{hash}&dn={dn}"))
}

#[tauri::command]
pub async fn open_destination(state: St<'_>, hash: String) -> Result<(), String> {
    if !settings::is_localhost(&state.settings().transport) {
        return Err("open destination is only available for a local daemon".into());
    }
    let path = state.backend().base_path(&hash).await.map_err(e)?;
    if path.is_empty() {
        return Err("no path on disk yet".into());
    }
    crate::localfs::reveal(&path)
}

/// One of the Peers-tab actions (B16).
async fn run_peer_action(
    app: &AppHandle,
    state: &St<'_>,
    hash: &str,
    peer_id: &str,
    verb: &str,
    result: Result<(), RtorrentError>,
) -> Result<(), String> {
    if let Err(err) = result {
        state.log(
            app,
            LogLevel::Error,
            format!("{verb} peer failed: {err}"),
            Some(hash.to_string()),
        );
        return Err(e(err));
    }
    state.log(
        app,
        LogLevel::Info,
        format!("{verb} peer {peer_id}"),
        Some(hash.to_string()),
    );
    state.detail_repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn ban_peer(
    app: AppHandle,
    state: St<'_>,
    hash: String,
    peer_id: String,
) -> Result<(), String> {
    let result = state.backend().ban_peer(&hash, &peer_id).await;
    run_peer_action(&app, &state, &hash, &peer_id, "banned", result).await
}

#[tauri::command]
pub async fn snub_peer(
    app: AppHandle,
    state: St<'_>,
    hash: String,
    peer_id: String,
) -> Result<(), String> {
    let result = state.backend().snub_peer(&hash, &peer_id).await;
    run_peer_action(&app, &state, &hash, &peer_id, "snubbed", result).await
}

#[tauri::command]
pub async fn disconnect_peer(
    app: AppHandle,
    state: St<'_>,
    hash: String,
    peer_id: String,
) -> Result<(), String> {
    let result = state.backend().disconnect_peer(&hash, &peer_id).await;
    run_peer_action(&app, &state, &hash, &peer_id, "disconnected", result).await
}

#[tauri::command]
pub async fn set_file_priority(
    state: St<'_>,
    hash: String,
    file_index: usize,
    priority: i64,
) -> Result<(), String> {
    state
        .backend()
        .set_file_priority(&hash, file_index, priority)
        .await
        .map_err(e)
}

#[tauri::command]
pub fn get_settings(state: St<'_>) -> Settings {
    state.settings()
}

#[tauri::command]
pub async fn apply_settings(
    app: AppHandle,
    state: St<'_>,
    patch: serde_json::Value,
) -> Result<Settings, String> {
    let mut next = settings::apply_patch(&state.settings(), patch);
    // The save path is the daemon's, so a picker result has to be translated;
    // the watch folder is ours and stays a native path.
    next.default_save_path = crate::localfs::to_daemon_path(&next.default_save_path)?;
    let saved = state.update_settings(next.clone());
    // Push daemon-affecting changes to rtorrent (best-effort; some may need a
    // restart to take effect on older builds).
    let backend = state.backend();
    // Global rate limits are owned by the poller now (it reconciles them to the
    // turtle-effective value each tick, B14); nudge it to apply promptly.
    let _ = backend.set_port_range(&saved.port_range).await;
    let _ = backend.set_dht(saved.dht_enabled).await;
    // Network-pane prefs (v1.6): encryption/PEX, proxy, bind, global caps.
    crate::network_prefs::apply(backend.as_ref(), &saved).await;
    state.log(&app, LogLevel::Info, "settings updated", None);
    state.repoll.notify_one();
    Ok(saved)
}

/// Toggle turtle mode's manual switch (B14). The poller applies the resulting
/// effective limits on its next tick, which the nudge triggers immediately.
#[tauri::command]
pub fn set_turtle(state: St<'_>, enabled: bool) -> Settings {
    let mut next = state.settings();
    next.turtle_enabled = enabled;
    let saved = state.update_settings(next);
    state.repoll.notify_one();
    saved
}

/// What the 1 Gbps tuner would do, for the confirmation dialog.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TuningPreview {
    /// Where the managed block would be written; `None` for a remote daemon
    /// whose config file isn't reachable from here.
    pub rc_path: Option<String>,
    /// The exact block that would be written to `.rtorrent.rc`.
    pub block: String,
    /// True when the daemon is local, so the file can be edited (otherwise the
    /// tuner can only push the values live over XML-RPC).
    pub can_write_file: bool,
}

/// The outcome of applying the tuner.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TuningResult {
    pub rc_path: Option<String>,
    pub file_written: bool,
    pub file_error: Option<String>,
    /// How many directives the running daemon accepted, out of `live_total`.
    pub live_applied: usize,
    pub live_total: usize,
    pub live_error: Option<String>,
}

/// Preview the 1 Gbps tuning block and where it would be written (menu action).
#[tauri::command]
pub fn tuning_preview(state: St<'_>) -> TuningPreview {
    let local = settings::is_localhost(&state.settings().transport);
    TuningPreview {
        rc_path: local.then(crate::rtorrent_rc::display_path),
        block: crate::rtorrent_rc::render_block(),
        can_write_file: local,
    }
}

/// Apply the 1 Gbps tuning: write the managed block into a local daemon's
/// `.rtorrent.rc`, and push the same values to the running daemon over XML-RPC
/// so most take effect without a restart.
#[tauri::command]
pub async fn apply_tuning(app: AppHandle, state: St<'_>) -> Result<TuningResult, String> {
    let local = settings::is_localhost(&state.settings().transport);
    let live = crate::rtorrent_rc::live_calls();
    let live_total = live.len();

    // 1) Persist to .rtorrent.rc (local daemons only — a remote's file is not
    //    ours to touch). The write may shell out to WSL, so keep it off the
    //    async reactor.
    let mut rc_path = None;
    let mut file_written = false;
    let mut file_error = None;
    if local {
        match tokio::task::spawn_blocking(crate::rtorrent_rc::write_block).await {
            Ok(Ok(path)) => {
                rc_path = Some(path);
                file_written = true;
            }
            Ok(Err(err)) => file_error = Some(err),
            Err(err) => file_error = Some(err.to_string()),
        }
    }

    // 2) Apply live over XML-RPC (best-effort; partial acceptance is fine).
    let mut live_error = None;
    let live_applied = match state.backend().apply_config(&live).await {
        Ok(n) => n,
        Err(err) => {
            live_error = Some(err.to_string());
            0
        }
    };

    let wrote = match &rc_path {
        Some(p) => format!(", wrote {p}"),
        None => String::new(),
    };
    state.log(
        &app,
        LogLevel::Info,
        format!("applied 1 Gbps tuning: {live_applied}/{live_total} live{wrote}"),
        None,
    );
    // Limits changed on the daemon; refresh promptly.
    state.repoll.notify_one();

    Ok(TuningResult {
        rc_path,
        file_written,
        file_error,
        live_applied,
        live_total,
        live_error,
    })
}

#[tauri::command]
pub async fn test_connection(
    transport: Transport,
    password: Option<String>,
) -> Result<String, String> {
    // Probe the candidate transport directly, independent of the active backend.
    // A password typed into Preferences isn't saved yet, so prefer it; an empty
    // or absent one falls back to whatever the Keychain holds.
    let client = match password.filter(|p| !p.is_empty()) {
        Some(p) => RpcClient::with_password(transport, Some(p)),
        None => RpcClient::new(transport),
    };
    client.client_version().await.map_err(e)
}

/// Save a remote daemon's password to the Keychain (B9).
///
/// Passwords are deliberately not part of `Settings`: that file is plaintext on
/// disk. The frontend sends one here and never reads it back.
#[tauri::command]
pub fn set_http_password(url: String, username: String, password: String) -> Result<(), String> {
    if crate::secrets::set_password(&url, &username, &password) {
        Ok(())
    } else {
        Err("could not save the password to the Keychain".into())
    }
}

/// Is a password saved for this endpoint? Lets Preferences show a saved-state
/// hint without the secret ever entering the webview.
#[tauri::command]
pub fn has_http_password(url: String, username: String) -> bool {
    crate::secrets::has_password(&url, &username)
}

/// Forget a saved remote password.
#[tauri::command]
pub fn clear_http_password(url: String, username: String) -> Result<(), String> {
    if crate::secrets::clear_password(&url, &username) {
        Ok(())
    } else {
        Err("could not remove the password from the Keychain".into())
    }
}

/// Wake the poller immediately (used by the disconnected card's "Retry now").
#[tauri::command]
pub fn retry_connection(state: St<'_>) {
    state.repoll.notify_one();
}

#[tauri::command]
pub fn set_detail_watch(state: St<'_>, hash: Option<String>, tab: Option<DetailTab>) {
    {
        let mut w = state.detail_watch.lock().unwrap();
        *w = match (hash, tab) {
            (Some(h), Some(t)) => Some((h, t)),
            _ => None,
        };
    }
    state.detail_repoll.notify_one();
}

#[tauri::command]
pub fn get_log(state: St<'_>) -> Vec<crate::ipc::LogEntry> {
    state.log.snapshot()
}

#[tauri::command]
pub async fn get_statistics(state: St<'_>) -> Result<Statistics, String> {
    let raw = state.backend().statistics().await.map_err(e)?;
    // Fold this session's totals into the persisted since-install counters.
    let (all_time_down, all_time_up) =
        crate::stats::accumulate(&state.stats_path, raw.session_down, raw.session_up);
    let all_time_ratio = if all_time_down > 0 {
        Some(all_time_up as f64 / all_time_down as f64)
    } else {
        None
    };
    Ok(Statistics {
        session_down: raw.session_down,
        session_up: raw.session_up,
        all_time_down,
        all_time_up,
        all_time_ratio,
        session_waste: raw.session_waste,
        connected_peers: raw.connected_peers,
        cache_hit_pct: raw.cache_hit_pct,
        buffer_size: raw.buffer_size,
        cache_overload_pct: raw.cache_overload_pct,
        queued_io: raw.queued_io,
    })
}

/// Daemon self-report for the Statistics dialog's Daemon tab (D16).
#[tauri::command]
pub async fn daemon_health(state: St<'_>) -> Result<DaemonHealth, String> {
    state.backend().daemon_health().await.map_err(e)
}

/// Ask the daemon to write its session now (D13).
#[tauri::command]
pub async fn save_session(app: AppHandle, state: St<'_>) -> Result<(), String> {
    state.backend().save_session().await.map_err(e)?;
    state.log(&app, LogLevel::Info, "session saved", None);
    Ok(())
}

/// Ask the daemon to shut down cleanly (D13). The connection will then drop and
/// the poller reports disconnected until a daemon is running again.
#[tauri::command]
pub async fn shutdown_daemon(app: AppHandle, state: St<'_>) -> Result<(), String> {
    state.backend().shutdown().await.map_err(e)?;
    state.log(&app, LogLevel::Warn, "daemon shutdown requested", None);
    Ok(())
}

/// Fetch and parse an RSS/Atom feed for the RSS preview (B11).
#[tauri::command]
pub async fn rss_fetch(url: String) -> Result<Vec<FeedItem>, String> {
    crate::rss::fetch(&url).await
}

/// Manually add one feed item (the RSS preview's Download button) (B11).
#[tauri::command]
pub async fn rss_download(
    app: AppHandle,
    state: St<'_>,
    link: String,
    label: String,
    save_path: String,
) -> Result<(), String> {
    let settings = state.settings();
    let resolved = if save_path.is_empty() {
        settings::save_path_for_label(&settings, &label)
    } else {
        save_path
    };
    let directory = crate::localfs::to_daemon_path(&resolved).unwrap_or(resolved);
    let opts = LoadOptions {
        directory,
        label,
        start: true,
        top_of_queue: false,
        unselected_indexes: vec![],
    };
    state.backend().load_magnet(&link, opts).await.map_err(e)?;
    state.log(&app, LogLevel::Info, "added from RSS", None);
    state.repoll.notify_one();
    Ok(())
}

/// Percent-encode a string for use as a magnet `dn=` value.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
