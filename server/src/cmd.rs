//! Mutation dispatch for `POST /api/cmd/{name}`.
//!
//! Command names and their JSON argument shapes mirror the desktop
//! `src/ipc/commands.ts` 1:1, so the web adapter is an almost-mechanical table
//! and the same server plumbing serves both. Each arm pulls typed fields out of
//! the JSON body and calls the shared `rtorrent-core` backend.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use axum::http::StatusCode;
use rtorrent_core::rtorrent::magnet_hash;
use rtorrent_core::types::Transport;

use crate::api::ApiError;
use crate::state::AppState;
use rtorrent_core::types::LogLevel;

/// Run a command by name; `Ok(Value::Null)` for actions, a string for
/// `copy_magnet`.
pub async fn run(state: &Arc<AppState>, name: &str, args: &Value) -> Result<Value, ApiError> {
    let b = state.backend.as_ref();
    match name {
        "start" => b.start(&hashes(args)?).await?,
        "stop" => b.stop(&hashes(args)?).await?,
        "pause" => b.pause(&hashes(args)?).await?,
        "recheck" => b.recheck(&hashes(args)?).await?,
        "force_reannounce" => b.force_reannounce(&hashes(args)?).await?,
        "set_label" => b.set_label(&hashes(args)?, &string(args, "label")?).await?,
        "set_tags" => {
            b.set_tags(&hashes(args)?, &string_array(args, "tags")?)
                .await?
        }
        "add_tags" => {
            let tags = string_array(args, "tags")?;
            for hash in hashes(args)? {
                let mut current = state.torrent(&hash).map(|t| t.tags).unwrap_or_default();
                current.extend(tags.iter().cloned());
                b.set_tags(&[hash], &current).await?;
            }
        }
        "remove_tags" => {
            let removed: Vec<String> = string_array(args, "tags")?
                .into_iter()
                .map(|tag| tag.to_lowercase())
                .collect();
            for hash in hashes(args)? {
                let current = state.torrent(&hash).map(|t| t.tags).unwrap_or_default();
                let kept: Vec<String> = current
                    .into_iter()
                    .filter(|tag| !removed.contains(&tag.to_lowercase()))
                    .collect();
                b.set_tags(&[hash], &kept).await?;
            }
        }
        "set_location" => set_location(state, args).await?,
        "cancel_move" => {
            let id = string(args, "id")?;
            let cancelled = state.moves.lock().unwrap().cancel(&id);
            return Ok(Value::Bool(cancelled));
        }
        "retry_move" => {
            let hash = hash(args)?;
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
            return Ok(Value::Bool(retried));
        }
        "export_session_text" => {
            // Portable manifest text (V3-22 / LIB-09); the dialog downloads it.
            let text = crate::session::export_text(state)
                .await
                .map_err(ApiError::bad)?;
            return Ok(Value::String(text));
        }
        "validate_session" => {
            // Dry-run validation + restore preview from uploaded text (V3-22).
            let dto = crate::session::preview(state, args)
                .await
                .map_err(ApiError::bad)?;
            return serde_json::to_value(dto)
                .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
        }
        "import_session" => {
            // Detached journaled import; the dialog polls `import_status`.
            crate::session::start_import(state, args).map_err(ApiError::bad)?;
            state.repoll.notify_one();
            return Ok(Value::Null);
        }
        "import_status" => {
            let status = state.import_status.lock().unwrap().clone();
            return serde_json::to_value(status)
                .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
        }
        "cancel_import" => {
            state
                .import_cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
            return Ok(Value::Null);
        }
        "scan_foreign" => {
            // Read-only discovery of another client's resume data (V3-22 /
            // LIB-10); `dir` is a path on the server host.
            let client = args.get("client").and_then(|v| v.as_str()).unwrap_or("");
            let dir = args.get("dir").and_then(|v| v.as_str()).unwrap_or("");
            let report = crate::session::scan_foreign(client, dir).map_err(ApiError::bad)?;
            return serde_json::to_value(report)
                .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
        }
        "set_file_priority" => {
            b.set_file_priority(
                &hash(args)?,
                int(args, "fileIndex")? as usize,
                int(args, "priority")?,
            )
            .await?
        }
        "set_connection_limits" => {
            let peers_max = int(args, "peersMax")?;
            let peers_min = int(args, "peersMin")?;
            let uploads_max = int(args, "uploadsMax")?;
            if peers_max < 0 || peers_min < 0 || uploads_max < 0 {
                return Err(ApiError::bad("connection limits must be zero or greater"));
            }
            b.set_connection_limits(&hash(args)?, peers_max, peers_min, uploads_max)
                .await?
        }
        "set_super_seeding" => {
            let hash = hash(args)?;
            let enabled = boolean(args, "enabled")?;
            let torrent = state
                .torrent(&hash)
                .ok_or_else(|| ApiError::bad("torrent not found"))?;
            if torrent.percent < 100.0 {
                return Err(ApiError::bad(
                    "super-seeding is only available for complete torrents",
                ));
            }
            b.set_super_seeding(&hash, enabled).await?
        }
        "add_tracker" => b.add_tracker(&hash(args)?, &string(args, "url")?).await?,
        "remove_tracker" => {
            b.remove_tracker(&hash(args)?, int(args, "trackerIndex")? as usize)
                .await?
        }
        "set_tracker_enabled" => {
            b.set_tracker_enabled(
                &hash(args)?,
                int(args, "trackerIndex")? as usize,
                boolean(args, "enabled")?,
            )
            .await?
        }
        "ban_peer" => b.ban_peer(&hash(args)?, &string(args, "peerId")?).await?,
        "snub_peer" => b.snub_peer(&hash(args)?, &string(args, "peerId")?).await?,
        "disconnect_peer" => {
            b.disconnect_peer(&hash(args)?, &string(args, "peerId")?)
                .await?
        }
        "queue_move" => queue_move(state, args).await?,
        "toggle_force_start" => toggle_force_start(state, args).await?,
        "remove" => remove(state, args).await?,
        "add_torrent" => add_torrent(state, args).await?,
        "create_torrent" => return create_torrent(state, args).await,
        "copy_magnet" => return copy_magnet(state, args),
        other => {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                format!("unknown command: {other}"),
            ))
        }
    }
    Ok(Value::Null)
}

// --- Compound commands -------------------------------------------------------

/// Reorder via (priority, queue_pos) swaps (QUE-02): a torrent takes exactly
/// the neighbour's rank, or pins the max/min band. Reads live rows from the
/// daemon (the cache may lag a previous reorder).
async fn queue_move(state: &Arc<AppState>, args: &Value) -> Result<(), ApiError> {
    let hs = hashes(args)?;
    let dir = match string(args, "direction")?.as_str() {
        "top" => rtorrent_core::queue::MoveDir::Top,
        "up" => rtorrent_core::queue::MoveDir::Up,
        "down" => rtorrent_core::queue::MoveDir::Down,
        "bottom" => rtorrent_core::queue::MoveDir::Bottom,
        _ => return Err(ApiError::bad("direction must be top, up, down or bottom")),
    };
    let rows = state.backend.list_snapshot().await?;
    let mut moved = 0;
    for reorder in rtorrent_core::queue::plan_reorder(&rows, &hs, dir) {
        state
            .backend
            .set_priority(&reorder.hash, reorder.priority)
            .await?;
        state
            .backend
            .set_custom_metadata(
                &reorder.hash,
                &[(
                    rtorrent_core::queue::POS_KEY,
                    &rtorrent_core::queue::format_pos(reorder.pos),
                )],
            )
            .await?;
        moved += 1;
    }
    state.log(
        rtorrent_core::types::LogLevel::Info,
        format!("reordered {moved} torrent(s)"),
        None,
    );
    Ok(())
}

/// Toggle force-start (QUE-01): mixed selections switch on, uniformly forced
/// selections switch off. Forced torrents are exempt from queue scheduling.
async fn toggle_force_start(state: &Arc<AppState>, args: &Value) -> Result<(), ApiError> {
    let hs = hashes(args)?;
    let rows = state.backend.list_snapshot().await?;
    let value = rtorrent_core::queue::force_toggle_value(&rows, &hs);
    for h in &hs {
        state
            .backend
            .set_custom_metadata(h, &[(rtorrent_core::queue::FORCE_KEY, value)])
            .await?;
    }
    state.log(
        rtorrent_core::types::LogLevel::Info,
        if value == "1" {
            format!("force-started {} torrent(s)", hs.len())
        } else {
            format!("force-start cleared on {} torrent(s)", hs.len())
        },
        None,
    );
    Ok(())
}

/// Erase torrents; when `deleteData` and the server is co-located with the
/// daemon, move their data to the trash first (never `rm`).
async fn remove(state: &Arc<AppState>, args: &Value) -> Result<(), ApiError> {
    let hs = hashes(args)?;
    let delete_data = args
        .get("deleteData")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if delete_data && !is_colocated(state) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "delete-data is only available when the server is co-located with the daemon",
        ));
    }

    // Read base paths *before* erasing, so we can trash the data after.
    let mut paths = Vec::new();
    if delete_data {
        for h in &hs {
            if let Ok(p) = state.backend.base_path(h).await {
                if !p.is_empty() {
                    paths.push(p);
                }
            }
        }
    }

    state.backend.erase(&hs).await?;

    for p in paths {
        match trash::delete(&p) {
            Ok(()) => state.log(LogLevel::Info, format!("moved to trash: {p}"), None),
            Err(e) => state.log(LogLevel::Warn, format!("could not trash {p}: {e}"), None),
        }
    }
    Ok(())
}

/// Set download directory for a torrent; when `moveData` and the server is
/// co-located with the daemon, moves existing data on disk to the new directory.
async fn set_location(state: &Arc<AppState>, args: &Value) -> Result<(), ApiError> {
    let h = hash(args)?;
    let p = string(args, "path")?;
    let move_data = args
        .get("moveData")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let colocated = is_colocated(state);
    if move_data && !colocated {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "moving data is only available when the server is co-located with the daemon",
        ));
    }

    let b = state.backend.as_ref();
    let snapshot = b.list_snapshot().await?;
    let torrent = snapshot.iter().find(|t| t.hash.eq_ignore_ascii_case(&h));
    let was_active = torrent.map(|t| t.is_active).unwrap_or(false);
    let old_base_path = torrent.map(|t| t.base_path.clone()).unwrap_or_default();

    let one = std::slice::from_ref(&h);
    if was_active {
        b.stop(one).await?;
    }

    if move_data && colocated && !old_base_path.is_empty() {
        let src_path = std::path::Path::new(&old_base_path);
        let dst_dir = std::path::Path::new(&p);
        if let Err(err) = rtorrent_core::fs::move_torrent_data(src_path, dst_dir) {
            if was_active {
                let _ = b.start(one).await;
            }
            return Err(ApiError::bad(format!("could not move files: {err}")));
        }
    }

    b.set_directory(&h, &p).await?;

    // A manual move wins over automation: drop any recorded final_dir so a
    // later completion does not drag the torrent back (V3-14).
    let _ = b
        .set_custom_metadata(&h, &[(rtorrent_core::complete::FINAL_DIR_KEY, "")])
        .await;

    if was_active {
        b.start(one).await?;
    }

    let msg = if move_data && colocated {
        format!("set location to {p} (moved data)")
    } else {
        format!("set location to {p} (files not moved)")
    };
    state.log(LogLevel::Info, msg, Some(h));
    Ok(())
}

/// Add a magnet. File adds go through the multipart upload endpoint (WE4), not
/// here, since the bytes don't belong in a JSON command body.
async fn add_torrent(state: &Arc<AppState>, args: &Value) -> Result<(), ApiError> {
    let source = args
        .get("source")
        .ok_or_else(|| ApiError::bad("`source` required"))?;
    let mut opts = load_options(args.get("opts"));
    // Route through the incomplete dir when configured (V3-14); the final
    // directory is recorded so the completion move knows where home is.
    let (directory, final_dir) =
        rtorrent_core::complete::route_new_download(&state.config.incomplete_dir, &opts.directory);
    opts.directory = directory;
    match source.get("kind").and_then(Value::as_str) {
        Some("magnet") => {
            let uri = source
                .get("uri")
                .and_then(Value::as_str)
                .ok_or_else(|| ApiError::bad("magnet `uri` required"))?;
            state.backend.load_magnet(uri, opts).await?;
            if let Some(hash) = magnet_hash(uri) {
                let added_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs().to_string())
                    .unwrap_or_default();
                let mut meta: Vec<(&str, &str)> = vec![
                    ("added_by", "magnet"),
                    ("source_path", uri),
                    ("added_at", &added_at),
                ];
                if let Some(final_dir) = final_dir.as_deref() {
                    meta.push((rtorrent_core::complete::FINAL_DIR_KEY, final_dir));
                }
                let _ = state.backend.set_custom_metadata(&hash, &meta).await;
            }
            Ok(())
        }
        Some("file") => Err(ApiError::bad(
            "file adds use POST /api/torrents/file, not /api/cmd/add_torrent",
        )),
        _ => Err(ApiError::bad("unknown add source kind")),
    }
}

/// Map the JSON `AddOptions` onto the crate's `LoadOptions`. Shared with the
/// upload endpoint in `api.rs`.
pub(crate) fn load_options(opts: Option<&Value>) -> rtorrent_core::rtorrent::LoadOptions {
    let get = |k: &str| opts.and_then(|o| o.get(k));
    rtorrent_core::rtorrent::LoadOptions {
        directory: get("savePath")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        label: get("label")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        start: get("start").and_then(Value::as_bool).unwrap_or(true),
        top_of_queue: get("topOfQueue").and_then(Value::as_bool).unwrap_or(false),
        unselected_indexes: get("unselectedIndexes")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_u64().map(|n| n as usize))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Build a magnet URI from the cached torrent name (the browser writes the
/// clipboard itself).
fn copy_magnet(state: &Arc<AppState>, args: &Value) -> Result<Value, ApiError> {
    let h = hash(args)?;
    let name = state.torrent(&h).map(|t| t.name).unwrap_or_default();
    let mut uri = format!("magnet:?xt=urn:btih:{h}");
    if !name.is_empty() {
        uri.push_str("&dn=");
        uri.push_str(&percent_encode(&name));
    }
    Ok(Value::String(uri))
}

/// The server is co-located with the daemon when it reaches it over a unix
/// socket (same box). Remote transports (tcp/http) gate off delete-data, matching
/// the desktop's localhost posture.
fn is_colocated(state: &Arc<AppState>) -> bool {
    matches!(state.config.transport, Transport::UnixSocket { .. })
}

/// Minimal percent-encoding for a magnet `dn` value.
fn percent_encode(s: &str) -> String {
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

// --- Argument extractors -----------------------------------------------------

fn hashes(args: &Value) -> Result<Vec<String>, ApiError> {
    args.get("hashes")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .ok_or_else(|| ApiError::bad("`hashes` (string array) required"))
}
/// Create a new .torrent from a path on the daemon host.
async fn create_torrent(state: &Arc<AppState>, args: &Value) -> Result<Value, ApiError> {
    if !is_colocated(state) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "creating torrents from host paths is only available when the server is co-located with the daemon",
        ));
    }

    let params: rtorrent_core::types::CreateTorrentParams = serde_json::from_value(args.clone())
        .map_err(|e| ApiError::bad(format!("invalid createTorrent parameters: {e}")))?;

    let src_path = std::path::PathBuf::from(&params.source_path);
    if !src_path.exists() {
        return Err(ApiError::bad(format!(
            "source path does not exist: {}",
            params.source_path
        )));
    }

    let opts = rtorrent_core::types::CreateTorrentOptions {
        source_path: src_path.clone(),
        piece_length: params.piece_length,
        trackers: params.trackers,
        is_private: params.is_private,
        comment: params.comment,
        source: params.source,
        created_by: Some("rstorrent".into()),
    };

    let (torrent, bytes) = rtorrent_core::torrent_file::create_torrent(opts)
        .map_err(|e| ApiError::bad(format!("failed to create torrent: {e}")))?;

    let output_path = if let Some(ref out) = params.output_path {
        if !out.trim().is_empty() {
            let out_path = std::path::Path::new(out);
            if let Some(parent) = out_path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            std::fs::write(out_path, &bytes)
                .map_err(|e| ApiError::bad(format!("could not write .torrent file: {e}")))?;
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
        let parent_dir = src_path
            .parent()
            .unwrap_or(&src_path)
            .to_string_lossy()
            .into_owned();

        let load_opts = rtorrent_core::rtorrent::LoadOptions {
            start: true,
            directory: parent_dir,
            label: String::new(),
            top_of_queue: false,
            unselected_indexes: Vec::new(),
        };

        state.backend.load_raw(bytes, load_opts).await?;
        state.log(
            LogLevel::Info,
            format!("created torrent '{name}' and started seeding"),
            Some(info_hash.clone()),
        );
    } else {
        state.log(
            LogLevel::Info,
            format!("created torrent '{name}' ({info_hash})"),
            Some(info_hash.clone()),
        );
    }

    let result = rtorrent_core::types::CreateTorrentResult {
        name,
        info_hash,
        total_size,
        piece_length,
        piece_count,
        output_path,
        is_private,
    };

    serde_json::to_value(result).map_err(|e| ApiError::bad(format!("serialization error: {e}")))
}

fn hash(args: &Value) -> Result<String, ApiError> {
    string(args, "hash")
}

fn string(args: &Value, key: &str) -> Result<String, ApiError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| ApiError::bad(format!("`{key}` (string) required")))
}

/// An array of strings (V3-10 tags). Normalisation happens in the core, so this
/// only checks the shape.
fn string_array(args: &Value, key: &str) -> Result<Vec<String>, ApiError> {
    let items = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::bad(format!("`{key}` (array of strings) required")))?;
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(String::from)
                .ok_or_else(|| ApiError::bad(format!("`{key}` entries must be strings")))
        })
        .collect()
}

fn int(args: &Value, key: &str) -> Result<i64, ApiError> {
    args.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| ApiError::bad(format!("`{key}` (integer) required")))
}

fn boolean(args: &Value, key: &str) -> Result<bool, ApiError> {
    args.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| ApiError::bad(format!("`{key}` (boolean) required")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encodes_spaces_and_specials() {
        assert_eq!(percent_encode("ubuntu 24.iso"), "ubuntu%2024.iso");
        assert_eq!(percent_encode("a/b&c"), "a%2Fb%26c");
        assert_eq!(percent_encode("plain-Name_1.0~"), "plain-Name_1.0~");
    }

    #[test]
    fn extractors_reject_missing_fields() {
        let empty = serde_json::json!({});
        assert!(hashes(&empty).is_err());
        assert!(string(&empty, "label").is_err());
        assert!(int(&empty, "priority").is_err());
        assert!(boolean(&empty, "enabled").is_err());

        let good = serde_json::json!({"hashes": ["A", "B"], "label": "x"});
        assert_eq!(hashes(&good).unwrap(), vec!["A", "B"]);
        assert_eq!(string(&good, "label").unwrap(), "x");
    }
}
