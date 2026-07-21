//! Mutation dispatch for `POST /api/cmd/{name}`.
//!
//! Command names and their JSON argument shapes mirror the desktop
//! `src/ipc/commands.ts` 1:1, so the web adapter is an almost-mechanical table
//! and the same server plumbing serves both. Each arm pulls typed fields out of
//! the JSON body and calls the shared `rtorrent-core` backend.

use std::sync::Arc;

use serde_json::Value;

use axum::http::StatusCode;
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
        "recheck" => b.recheck(&hashes(args)?).await?,
        "force_reannounce" => b.force_reannounce(&hashes(args)?).await?,
        "set_label" => b.set_label(&hashes(args)?, &string(args, "label")?).await?,
        "set_location" => {
            b.set_directory(&hash(args)?, &string(args, "path")?)
                .await?
        }
        "set_file_priority" => {
            b.set_file_priority(
                &hash(args)?,
                int(args, "fileIndex")? as usize,
                int(args, "priority")?,
            )
            .await?
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
        "remove" => remove(state, args).await?,
        "add_torrent" => add_torrent(state, args).await?,
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

/// Nudge queue order via `d.priority` steps (rtorrent has no true queue order —
/// the honest "priority" mapping the desktop documents). Reads the current
/// priority from the cached snapshot.
async fn queue_move(state: &Arc<AppState>, args: &Value) -> Result<(), ApiError> {
    let hs = hashes(args)?;
    let delta = match string(args, "direction")?.as_str() {
        "up" => 1,
        "down" => -1,
        _ => return Err(ApiError::bad("direction must be \"up\" or \"down\"")),
    };
    for h in &hs {
        let current = state.torrent(h).map(|t| t.priority).unwrap_or(1);
        let next = (current + delta).clamp(0, 3);
        if next != current {
            state.backend.set_priority(h, next).await?;
        }
    }
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

/// Add a magnet. File adds go through the multipart upload endpoint (WE4), not
/// here, since the bytes don't belong in a JSON command body.
async fn add_torrent(state: &Arc<AppState>, args: &Value) -> Result<(), ApiError> {
    let source = args
        .get("source")
        .ok_or_else(|| ApiError::bad("`source` required"))?;
    let opts = load_options(args.get("opts"));
    match source.get("kind").and_then(Value::as_str) {
        Some("magnet") => {
            let uri = source
                .get("uri")
                .and_then(Value::as_str)
                .ok_or_else(|| ApiError::bad("magnet `uri` required"))?;
            state.backend.load_magnet(uri, opts).await?;
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

fn hash(args: &Value) -> Result<String, ApiError> {
    string(args, "hash")
}

fn string(args: &Value, key: &str) -> Result<String, ApiError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| ApiError::bad(format!("`{key}` (string) required")))
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
