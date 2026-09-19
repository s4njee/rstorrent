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
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, State};

use crate::ipc::{
    AddOptions, AddSource, CreateTorrentParams, CreateTorrentResult, DaemonHealth, DetailTab,
    FeedItem, LogLevel, Settings, Statistics, TorrentMeta, Transport,
};
use crate::open_requests::OpenRequestState;
use crate::rtorrent::{client::RpcClient, magnet_hash, LoadOptions, RtorrentApi, RtorrentError};
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

/// Convert the dialog options into backend load options, loading into
/// `directory` (already routed through the incomplete dir and expressed in
/// the daemon's namespace by the caller).
fn load_opts(opts: &AddOptions, directory: String) -> LoadOptions {
    LoadOptions {
        directory,
        label: opts.label.clone(),
        start: opts.start,
        top_of_queue: opts.top_of_queue,
        unselected_indexes: opts.unselected_indexes.clone(),
    }
}

/// Record a routed download's final directory (`d.custom=final_dir`, V3-14)
/// so the completion move knows where home is. Best-effort like the other
/// add metadata: callers log a warning and continue on failure.
pub(crate) async fn persist_final_dir(
    backend: &dyn RtorrentApi,
    hash: &str,
    final_dir: Option<&str>,
) -> Result<(), String> {
    let Some(final_dir) = final_dir.filter(|d| !d.trim().is_empty()) else {
        return Ok(());
    };
    backend
        .set_custom_metadata(hash, &[(rtorrent_core::complete::FINAL_DIR_KEY, final_dir)])
        .await
        .map_err(e)
}

/// Persist provenance in rtorrent's session-backed custom namespace. Metadata
/// failure must not make a successful add look like a failed add, so callers
/// log it as a warning and continue.
pub(crate) async fn persist_add_metadata(
    backend: &dyn RtorrentApi,
    hash: &str,
    added_by: &str,
    source_path: &str,
) -> Result<(), String> {
    let added_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(e)?
        .as_secs()
        .to_string();
    backend
        .set_custom_metadata(
            hash,
            &[
                ("added_by", added_by),
                ("source_path", source_path),
                ("added_at", &added_at),
            ],
        )
        .await
        .map_err(e)
}

#[tauri::command]
pub fn read_torrent_metadata(path: String) -> Result<TorrentMeta, String> {
    torrent_file::read_metadata(&path)
}

#[tauri::command]
pub async fn create_torrent(
    app: AppHandle,
    state: St<'_>,
    params: CreateTorrentParams,
) -> Result<CreateTorrentResult, String> {
    let resolved_src = crate::localfs::resolve(&params.source_path)?;
    if !resolved_src.exists() {
        return Err(format!(
            "source path does not exist: {}",
            params.source_path
        ));
    }

    let opts = rtorrent_core::types::CreateTorrentOptions {
        source_path: resolved_src.clone(),
        piece_length: params.piece_length,
        trackers: params.trackers,
        is_private: params.is_private,
        comment: params.comment,
        source: params.source,
        created_by: Some("rstorrent".into()),
    };

    let (torrent, bytes) = rtorrent_core::torrent_file::create_torrent(opts)?;

    let output_path = if let Some(ref out) = params.output_path {
        if !out.trim().is_empty() {
            let resolved_out = crate::localfs::resolve(out)?;
            if let Some(parent) = resolved_out.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            std::fs::write(&resolved_out, &bytes)
                .map_err(|e| format!("could not write .torrent file: {e}"))?;
            Some(out.clone())
        } else {
            None
        }
    } else {
        None
    };

    let name = torrent.name.clone();
    let info_hash = torrent.info_hash().to_uppercase();
    let total_size = torrent.length;
    let piece_length = torrent.piece_length;
    let piece_count = torrent.pieces.len() as i64;
    let is_private = torrent.is_private();

    if params.start_seeding {
        let parent_dir = resolved_src
            .parent()
            .unwrap_or(&resolved_src)
            .to_string_lossy()
            .into_owned();
        let daemon_dir = crate::localfs::to_daemon_path(&parent_dir)?;

        let load_opts = rtorrent_core::rtorrent::LoadOptions {
            start: true,
            directory: daemon_dir,
            label: String::new(),
            top_of_queue: false,
            unselected_indexes: Vec::new(),
        };

        state
            .backend()
            .load_raw(bytes, load_opts)
            .await
            .map_err(e)?;
        if let Err(err) =
            persist_add_metadata(&*state.backend(), &info_hash, "file", &params.source_path).await
        {
            state.log(
                &app,
                LogLevel::Warn,
                format!("could not persist add metadata: {err}"),
                Some(info_hash.clone()),
            );
        }
        state.log(
            &app,
            LogLevel::Info,
            format!("created torrent '{name}' and started seeding"),
            Some(info_hash.clone()),
        );
        state.repoll.notify_one();
    } else {
        state.log(
            &app,
            LogLevel::Info,
            format!("created torrent '{name}' ({info_hash})"),
            Some(info_hash.clone()),
        );
    }

    Ok(CreateTorrentResult {
        name,
        info_hash,
        total_size,
        piece_length,
        piece_count,
        output_path,
        is_private,
    })
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
    // Route through the incomplete dir when configured (V3-14); the final
    // directory is recorded so the completion move knows where home is.
    let (directory, final_dir) = settings::route_new_download(&state.settings(), &opts.save_path);
    let load = load_opts(&opts, directory);
    match source {
        AddSource::File { path } => {
            let meta = torrent_file::read_metadata(&path).map_err(e)?;
            let bytes = std::fs::read(&path).map_err(e)?;
            backend.load_raw(bytes, load).await.map_err(e)?;
            if let Err(err) =
                persist_final_dir(&*backend, &meta.info_hash, final_dir.as_deref()).await
            {
                state.log(
                    &app,
                    LogLevel::Warn,
                    format!("could not persist final directory: {err}"),
                    Some(meta.info_hash.clone()),
                );
            }
            if let Err(err) = persist_add_metadata(&*backend, &meta.info_hash, "file", &path).await
            {
                state.log(
                    &app,
                    LogLevel::Warn,
                    format!("could not persist add metadata: {err}"),
                    Some(meta.info_hash.clone()),
                );
            }
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
            if let Some(hash) = magnet_hash(&uri) {
                if let Err(err) = persist_final_dir(&*backend, &hash, final_dir.as_deref()).await {
                    state.log(
                        &app,
                        LogLevel::Warn,
                        format!("could not persist final directory: {err}"),
                        Some(hash.clone()),
                    );
                }
                if let Err(err) = persist_add_metadata(&*backend, &hash, "magnet", &uri).await {
                    state.log(
                        &app,
                        LogLevel::Warn,
                        format!("could not persist add metadata: {err}"),
                        Some(hash),
                    );
                }
            }
            state.log(&app, LogLevel::Info, "added magnet", None);
        }
    }
    state.repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn set_connection_limits(
    app: AppHandle,
    state: St<'_>,
    hash: String,
    peers_max: i64,
    peers_min: i64,
    uploads_max: i64,
) -> Result<(), String> {
    if peers_max < 0 || peers_min < 0 || uploads_max < 0 {
        return Err("connection limits must be zero or greater".into());
    }
    state
        .backend()
        .set_connection_limits(&hash, peers_max, peers_min, uploads_max)
        .await
        .map_err(e)?;
    // Hand-set caps opt out of bandwidth rules too (V3-18 precedence).
    let _ = state
        .backend()
        .set_custom_metadata(&hash, &[(rtorrent_core::bandwidth::RULE_KEY, "")])
        .await;
    state.log(
        &app,
        LogLevel::Info,
        "updated per-torrent connection limits",
        Some(hash),
    );
    state.repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn set_super_seeding(
    app: AppHandle,
    state: St<'_>,
    hash: String,
    enabled: bool,
) -> Result<(), String> {
    let rows = state.backend().list_snapshot().await.map_err(e)?;
    let Some(torrent) = rows
        .into_iter()
        .find(|t| t.hash.eq_ignore_ascii_case(&hash))
    else {
        return Err("torrent not found".into());
    };
    if !torrent.complete {
        return Err("super-seeding is only available for complete torrents".into());
    }
    state
        .backend()
        .set_super_seeding(&hash, enabled)
        .await
        .map_err(e)?;
    state.log(
        &app,
        LogLevel::Info,
        if enabled {
            "enabled super-seeding"
        } else {
            "disabled super-seeding"
        },
        Some(hash),
    );
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
pub async fn pause(app: AppHandle, state: St<'_>, hashes: Vec<String>) -> Result<(), String> {
    state.backend().pause(&hashes).await.map_err(e)?;
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
        // Hand-cleared limits opt out of bandwidth rules too (V3-18).
        for h in &hashes {
            let _ = backend
                .set_custom_metadata(h, &[(rtorrent_core::bandwidth::RULE_KEY, "")])
                .await;
        }
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
    // A hand-set limit is a torrent override (V3-18 precedence): drop any
    // bandwidth-rule marker so the rule engine keeps its hands off.
    for h in &hashes {
        let _ = backend
            .set_custom_metadata(h, &[(rtorrent_core::bandwidth::RULE_KEY, "")])
            .await;
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
    move_data: Option<bool>,
) -> Result<(), String> {
    let path = crate::localfs::to_daemon_path(&path)?;
    let backend = state.backend();
    let local = settings::is_localhost(&state.settings().transport);
    let should_move = move_data.unwrap_or(true) && local;

    // Check torrent status to preserve active/stopped state and obtain base path.
    let snapshot = backend.list_snapshot().await.map_err(e)?;
    let torrent = snapshot.iter().find(|t| t.hash.eq_ignore_ascii_case(&hash));
    let was_active = torrent.map(|t| t.is_active).unwrap_or(false);
    let old_base_path = torrent.map(|t| t.base_path.clone()).unwrap_or_default();

    let one = std::slice::from_ref(&hash);
    if was_active {
        backend.stop(one).await.map_err(e)?;
    }

    if should_move && !old_base_path.is_empty() {
        if let Err(err) = crate::localfs::move_torrent_data(&old_base_path, &path) {
            if was_active {
                let _ = backend.start(one).await;
            }
            return Err(e(err));
        }
    }

    backend.set_directory(&hash, &path).await.map_err(e)?;

    // A manual move wins over automation: drop any recorded final_dir so a
    // later completion does not drag the torrent back (V3-14).
    let _ = backend
        .set_custom_metadata(&hash, &[(rtorrent_core::complete::FINAL_DIR_KEY, "")])
        .await;

    if was_active {
        backend.start(one).await.map_err(e)?;
    }

    let (level, msg) = if should_move {
        (
            LogLevel::Info,
            format!("set location to {path} (moved data)"),
        )
    } else {
        (
            LogLevel::Info,
            format!("set location to {path} (files not moved)"),
        )
    };
    state.log(&app, level, msg, Some(hash));
    state.repoll.notify_one();
    Ok(())
}

/// Live move-on-complete statuses (V3-14) for the status pill / moves dialog.
#[tauri::command]
pub fn get_moves(state: St<'_>) -> Vec<rtorrent_core::mover::MoveStatus> {
    state.moves.lock().unwrap().snapshot()
}

/// Cancel a running move by op id (or torrent hash). The torrent resumes in
/// place; the journal keeps a terminal Cancelled entry until retried.
#[tauri::command]
pub fn cancel_move(state: St<'_>, id: String) -> bool {
    state.moves.lock().unwrap().cancel(&id)
}

/// Drop a Failed/Cancelled entry so the next tick re-plans from daemon
/// truth. Returns false when there is nothing retryable for the hash.
#[tauri::command]
pub fn retry_move(state: St<'_>, hash: String) -> bool {
    let retried = {
        let mut moves = state.moves.lock().unwrap();
        let retried = moves.retry(&hash);
        if retried {
            let _ = moves.save();
        }
        retried
    };
    if retried {
        state.repoll.notify_one();
    }
    retried
}

#[tauri::command]
pub async fn queue_move(
    app: AppHandle,
    state: St<'_>,
    hashes: Vec<String>,
    direction: String,
) -> Result<(), String> {
    // rtorrent has no true queue order: reorder swaps whole (priority,
    // queue_pos) pairs with the neighbour, so a torrent takes exactly the
    // rank above/below it (QUE-02). Top/bottom pin the daemon's max/min band
    // with an extreme sequence value.
    let dir = match direction.as_str() {
        "top" => rtorrent_core::queue::MoveDir::Top,
        "up" => rtorrent_core::queue::MoveDir::Up,
        "down" => rtorrent_core::queue::MoveDir::Down,
        "bottom" => rtorrent_core::queue::MoveDir::Bottom,
        _ => return Err("direction must be top, up, down or bottom".into()),
    };
    let backend = state.backend();
    let rows = backend.list_snapshot().await.map_err(e)?;
    let plan = rtorrent_core::queue::plan_reorder(&rows, &hashes, dir);
    for reorder in &plan {
        backend
            .set_priority(&reorder.hash, reorder.priority)
            .await
            .map_err(e)?;
        backend
            .set_custom_metadata(
                &reorder.hash,
                &[(
                    rtorrent_core::queue::POS_KEY,
                    &rtorrent_core::queue::format_pos(reorder.pos),
                )],
            )
            .await
            .map_err(e)?;
    }
    state.log(
        &app,
        LogLevel::Info,
        format!("reordered {} torrent(s)", plan.len()),
        None,
    );
    state.repoll.notify_one();
    Ok(())
}

/// Toggle force-start (V3-17 / QUE-01) on the selection: mixed selections
/// switch on, uniformly forced selections switch off. Forced torrents are
/// exempt from the client queue scheduler.
#[tauri::command]
pub async fn toggle_force_start(
    app: AppHandle,
    state: St<'_>,
    hashes: Vec<String>,
) -> Result<(), String> {
    let backend = state.backend();
    let rows = backend.list_snapshot().await.map_err(e)?;
    let value = rtorrent_core::queue::force_toggle_value(&rows, &hashes);
    for h in &hashes {
        backend
            .set_custom_metadata(h, &[(rtorrent_core::queue::FORCE_KEY, value)])
            .await
            .map_err(e)?;
    }
    state.log(
        &app,
        LogLevel::Info,
        if value == "1" {
            format!("force-started {} torrent(s)", hashes.len())
        } else {
            format!("force-start cleared on {} torrent(s)", hashes.len())
        },
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
    if !next.incomplete_dir.trim().is_empty() {
        next.incomplete_dir = crate::localfs::to_daemon_path(&next.incomplete_dir)?;
    }
    let old_rules = state.settings().bandwidth_rules.clone();
    let saved = state.update_settings(next.clone());
    // Reprofiled bandwidth rules (V3-18) must re-adopt at the new rates: the
    // steady tick would otherwise keep the stale definition. Clearing the
    // markers drops affected torrents back to clean, and the next tick
    // adopts them fresh.
    let changed = rtorrent_core::bandwidth::changed_rule_ids(&old_rules, &saved.bandwidth_rules);
    if !changed.is_empty() {
        let backend = state.backend();
        let app = app.clone();
        let state = state.inner().clone();
        tauri::async_runtime::spawn(async move {
            if let Ok(rows) = backend.list_snapshot().await {
                for t in rows
                    .iter()
                    .filter(|t| changed.iter().any(|id| id == &t.throttle_rule))
                {
                    let _ = backend
                        .set_custom_metadata(&t.hash, &[(rtorrent_core::bandwidth::RULE_KEY, "")])
                        .await;
                }
            }
            state.log(&app, LogLevel::Info, "bandwidth rules updated", None);
            state.repoll.notify_one();
        });
    }
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
pub fn get_snapshot(state: St<'_>) -> Option<crate::ipc::Snapshot> {
    state.snapshot()
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

/// Start a local rtorrent daemon (C20). Only meaningful when the transport is
/// local — for a remote HTTP daemon the user manages it out-of-band.
#[tauri::command]
pub async fn start_daemon(app: AppHandle, state: St<'_>) -> Result<String, String> {
    if !settings::is_localhost(&state.settings().transport) {
        return Err("start is only available for a local daemon".into());
    }
    let transport = state.settings().transport.clone();
    // `AppHandle` is `Send + 'static`, and `start` needs it to find the runtime
    // this build ships (Resources/binaries/rtorrent).
    let app_for_start = app.clone();
    let started =
        tokio::task::spawn_blocking(move || crate::daemon_start::start(&app_for_start, transport))
            .await
            .map_err(|e| e.to_string())?;
    let msg = started?;
    state.log(&app, LogLevel::Info, msg.clone(), None);
    state.repoll.notify_one();
    Ok(msg)
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

/// Test one rule against a live feed (V3-23): every item with its
/// clause-by-clause verdict, so the preview explains each match and miss.
#[tauri::command]
pub async fn rss_test(
    rule: rtorrent_core::rss::Rule,
    url: String,
) -> Result<Vec<crate::rss::RssTestRow>, String> {
    crate::rss::test_rule(&rule, &url).await
}

/// Export the seen-set as JSON for backup or another machine (V3-23).
#[tauri::command]
pub fn rss_export_seen(state: St<'_>) -> String {
    crate::rss::export_seen(state.inner())
}

/// Import guids into the seen-set; returns how many were new (V3-23).
#[tauri::command]
pub fn rss_import_seen(state: St<'_>, json: String) -> usize {
    let added = crate::rss::import_seen(state.inner(), &json);
    state.repoll.notify_one();
    added
}

/// Build the manifest and return its text (V3-22 / LIB-09): hashes,
/// re-addable sources, trackers, labels/tags, paths, priorities, limits and
/// client metadata. Never credentials. The dialog saves the text (native
/// save dialog on desktop, blob download on web).
#[tauri::command]
pub async fn export_session_text(app: AppHandle, state: St<'_>) -> Result<String, String> {
    crate::session::export_text(&app, &state).await
}

/// Export the session manifest to a file (V3-22 / LIB-09): hashes,
/// re-addable sources, trackers, labels/tags, paths, priorities, limits and
/// client metadata. Never credentials.
#[tauri::command]
pub async fn export_session(
    app: AppHandle,
    state: St<'_>,
    path: String,
) -> Result<rtorrent_core::session::ExportReport, String> {
    crate::session::export_to(&app, &state, std::path::Path::new(&path)).await
}

/// Dry-run validation + restore preview for a manifest (V3-22). Takes either
/// a file `path` (desktop native picker) or `manifest_text` (web upload /
/// paste / foreign-scan result) — exactly one.
#[tauri::command]
pub async fn validate_session(
    state: St<'_>,
    path: Option<String>,
    selected: Option<Vec<String>>,
    remap_from: Option<String>,
    remap_to: Option<String>,
    manifest_text: Option<String>,
) -> Result<rtorrent_core::session::ValidationDto, String> {
    let remaps = match (remap_from, remap_to) {
        (Some(from), Some(to)) if !from.trim().is_empty() => {
            vec![(from, to)]
        }
        _ => Vec::new(),
    };
    let selected = selected.map(|hashes| hashes.into_iter().collect());
    crate::session::preview(
        &state,
        path.as_deref(),
        manifest_text.as_deref(),
        selected,
        remaps,
    )
    .await
}

/// Import a manifest as a detached, journalised job (V3-22): torrents are
/// added stopped, rechecked and resumed. Takes either a file `path` or
/// `manifest_text`. Returns immediately; poll `import_status` for progress.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri maps one param per invoke field.
pub fn import_session(
    app: AppHandle,
    state: St<'_>,
    path: Option<String>,
    selected: Option<Vec<String>>,
    remap_from: Option<String>,
    remap_to: Option<String>,
    resume: Option<bool>,
    manifest_text: Option<String>,
) -> Result<(), String> {
    let remaps = match (remap_from, remap_to) {
        (Some(from), Some(to)) if !from.trim().is_empty() => {
            vec![(from, to)]
        }
        _ => Vec::new(),
    };
    let selected = selected.map(|hashes| hashes.into_iter().collect());
    crate::session::start_import(
        &app,
        &state,
        path.as_deref(),
        manifest_text.as_deref(),
        selected,
        remaps,
        resume.unwrap_or(false),
    )
}

/// Scan qBittorrent (`BT_backup`) or Transmission (config dir / `resume/`)
/// resume data into a portable manifest (V3-22 / LIB-10). Read-only; the
/// returned `manifest_text` flows through the normal validate → import path.
#[tauri::command]
pub fn scan_foreign(
    client: String,
    dir: String,
) -> Result<rtorrent_core::foreign::ScanReport, String> {
    crate::session::scan_foreign(&client, &dir)
}

/// Live import status for the dialog to poll.
#[tauri::command]
pub fn import_status(state: St<'_>) -> crate::session::ImportStatus {
    state.import_status.lock().unwrap().clone()
}

/// Cancel a running import after the current torrent finishes its step.
#[tauri::command]
pub fn cancel_import(state: St<'_>) {
    state
        .import_cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);
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
    let (directory, final_dir) = settings::route_new_download(&settings, &resolved);
    let opts = LoadOptions {
        directory,
        label,
        start: true,
        top_of_queue: false,
        unselected_indexes: vec![],
    };
    let backend = state.backend();
    backend.load_magnet(&link, opts).await.map_err(e)?;
    if let Some(hash) = magnet_hash(&link) {
        if let Err(err) = persist_final_dir(&*backend, &hash, final_dir.as_deref()).await {
            state.log(
                &app,
                LogLevel::Warn,
                format!("could not persist final directory: {err}"),
                Some(hash.clone()),
            );
        }
        if let Err(err) = persist_add_metadata(&*backend, &hash, "rss", &link).await {
            state.log(
                &app,
                LogLevel::Warn,
                format!("could not persist RSS add metadata: {err}"),
                Some(hash),
            );
        }
    }
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
