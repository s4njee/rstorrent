//! Tauri command surface — the frontend → Rust RPC boundary.
//!
//! Each `#[tauri::command]` here is thin: it validates/borrows state, delegates
//! to the rtorrent backend, logs notable outcomes, and (for mutations) triggers
//! an immediate re-poll so the UI updates promptly. Errors are mapped to plain
//! strings, which surface as rejected promises on the TypeScript side.
//!
//! Command names and argument shapes must match `src/ipc/commands.ts`.

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::ipc::{
    AddOptions, AddSource, DetailTab, LogLevel, Settings, Statistics, TorrentMeta, Transport,
};
use crate::rtorrent::{client::ScgiClient, LoadOptions, RtorrentApi};
use crate::settings;
use crate::state::AppState;
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
        directory: opts.save_path.clone(),
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
            state.log(&app, LogLevel::Info, format!("added torrent from {path}"), None);
            // Deselected files → priority 0. rtorrent addresses files by the
            // torrent's info-hash, which we read back from the .torrent. This
            // runs right after load; on a busy daemon the download may not be
            // registered yet, so failures are logged rather than fatal (the
            // torrent is already added).
            if !opts.unselected_indexes.is_empty() {
                if let Ok(meta) = torrent_file::read_metadata(&path) {
                    for &idx in &opts.unselected_indexes {
                        if let Err(err) =
                            backend.set_file_priority(&meta.info_hash, idx, 0).await
                        {
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
    state.log(&app, LogLevel::Info, format!("resumed {} torrent(s)", hashes.len()), None);
    state.repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn stop(app: AppHandle, state: St<'_>, hashes: Vec<String>) -> Result<(), String> {
    state.backend().stop(&hashes).await.map_err(e)?;
    state.log(&app, LogLevel::Info, format!("paused {} torrent(s)", hashes.len()), None);
    state.repoll.notify_one();
    Ok(())
}

#[tauri::command]
pub async fn recheck(app: AppHandle, state: St<'_>, hashes: Vec<String>) -> Result<(), String> {
    state.backend().recheck(&hashes).await.map_err(e)?;
    state.log(&app, LogLevel::Info, format!("rechecking {} torrent(s)", hashes.len()), None);
    state.repoll.notify_one();
    Ok(())
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
        match trash::delete(&p) {
            Ok(_) => state.log(&app, LogLevel::Info, format!("moved to Trash: {p}"), None),
            Err(err) => state.log(&app, LogLevel::Warn, format!("could not trash {p}: {err}"), None),
        }
    }
    state.log(&app, LogLevel::Info, format!("removed {} torrent(s)", hashes.len()), None);
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
    state.backend().set_label(&hashes, &label).await.map_err(e)?;
    state.log(&app, LogLevel::Info, format!("set label '{label}'"), None);
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
    let backend = state.backend();
    let one = std::slice::from_ref(&hash);
    backend.stop(one).await.map_err(e)?;
    backend.set_directory(&hash, &path).await.map_err(e)?;
    backend.start(one).await.map_err(e)?;
    state.log(&app, LogLevel::Warn, format!("set location to {path} (files not moved)"), Some(hash));
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
    state.log(&app, LogLevel::Info, format!("reordered {} torrent(s)", hashes.len()), None);
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
    // Reveal the item in Finder (macOS).
    std::process::Command::new("open")
        .args(["-R", &path])
        .status()
        .map_err(e)?;
    Ok(())
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
    let next = settings::apply_patch(&state.settings(), patch);
    let saved = state.update_settings(next.clone());
    // Push daemon-affecting changes to rtorrent (best-effort; some may need a
    // restart to take effect on older builds).
    let backend = state.backend();
    let _ = backend
        .set_throttles(saved.down_limit_kb, saved.up_limit_kb)
        .await;
    let _ = backend.set_port_range(&saved.port_range).await;
    let _ = backend.set_dht(saved.dht_enabled).await;
    state.log(&app, LogLevel::Info, "settings updated", None);
    Ok(saved)
}

#[tauri::command]
pub async fn test_connection(transport: Transport) -> Result<String, String> {
    // Probe the candidate transport directly, independent of the active backend.
    ScgiClient::new(transport)
        .client_version()
        .await
        .map_err(e)
}

/// Wake the poller immediately (used by the disconnected card's "Retry now").
#[tauri::command]
pub fn retry_connection(state: St<'_>) {
    state.repoll.notify_one();
}

#[tauri::command]
pub fn set_detail_watch(state: St<'_>, hash: Option<String>, tab: Option<DetailTab>) {
    let mut w = state.detail_watch.lock().unwrap();
    *w = match (hash, tab) {
        (Some(h), Some(t)) => Some((h, t)),
        _ => None,
    };
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
