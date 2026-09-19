//! Session export/import (V3-22 / LIB-09/-10) for the web shell.
//!
//! Mirrors `src-tauri/src/session.rs`: export builds a portable manifest from
//! live daemon state, import validates, plans against loaded hashes, and runs
//! a detached journaled job (add stopped → metadata → recheck → resume).
//! Progress travels through the server log and the `import_status` command
//! the dialog polls. The web dialog passes manifest *text* (from an
//! `<input type=file>` or a desktop foreign-scan); the path-based desktop
//! export stays desktop-only.
//!
//! The server does not manage global rate caps, so exports record them as
//! 0/0 (unlimited) rather than inventing numbers.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rtorrent_core::foreign::ForeignClient;
use rtorrent_core::session::{
    self, ImportJournal, Manifest, ManifestSource, RestoreAction, ValidationDto,
};
use rtorrent_core::types::LogLevel;

use crate::state::AppState;

/// `import-journal.json` beside the config file, like the move journal.
pub fn journal_path(config_path: Option<&std::path::Path>) -> PathBuf {
    config_path
        .and_then(|p| p.parent())
        .map(|dir| dir.join("import-journal.json"))
        .unwrap_or_else(|| PathBuf::from("import-journal.json"))
}

/// Live import status for the dialog to poll. Same JSON shape as the desktop
/// `ImportStatus` so one dialog serves both shells.
#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportStatus {
    pub running: bool,
    pub added: usize,
    pub resumed: usize,
    pub skipped: usize,
    pub failed: Vec<String>,
    pub done: bool,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Best-effort re-addable source for one torrent: an embedded `.torrent`
/// when its file is readable on the server host, else the recorded
/// magnet/URL/path as-is.
fn source_for(source_path: &str) -> Option<ManifestSource> {
    let trimmed = source_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("magnet:")
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
    {
        return Some(ManifestSource::Magnet(trimmed.to_owned()));
    }
    match std::fs::read(trimmed) {
        Ok(bytes) => Some(ManifestSource::TorrentBase64(session::encode_base64(
            &bytes,
        ))),
        Err(_) => Some(ManifestSource::TorrentPath(trimmed.to_owned())),
    }
}

/// Build the manifest text: snapshot, per-torrent trackers, best sources.
pub async fn export_text(state: &Arc<AppState>) -> Result<String, String> {
    let backend = state.backend.as_ref();
    let rows = backend.list_snapshot().await.map_err(|e| e.to_string())?;
    let mut trackers = HashMap::new();
    let mut sources = HashMap::new();
    for row in &rows {
        match backend.trackers(&row.hash).await {
            Ok(list) => {
                trackers.insert(row.hash.clone(), list.into_iter().map(|t| t.url).collect());
            }
            Err(e) => {
                state.log(
                    LogLevel::Warn,
                    format!("export: no trackers for {} ({e})", row.name),
                    Some(row.hash.clone()),
                );
            }
        }
        if let Some(source) = source_for(&row.source_path) {
            sources.insert(row.hash.clone(), source);
        }
    }
    let manifest = session::export_manifest(
        &rows,
        &trackers,
        &sources,
        0,
        0,
        now_ms(),
        env!("CARGO_PKG_VERSION"),
    );
    state.log(
        LogLevel::Info,
        format!("exported session: {} torrent(s)", manifest.torrents.len()),
        None,
    );
    Ok(manifest.to_json())
}

/// Load a manifest from uploaded/pasted text (the web dialog has no paths).
fn load_manifest(manifest_text: Option<&str>) -> Result<Manifest, String> {
    match manifest_text {
        Some(text) => Manifest::parse(text),
        None => Err("no manifest given — upload a manifest file".into()),
    }
}

fn remaps(args: &serde_json::Value) -> Vec<(String, String)> {
    let from = args.get("remapFrom").and_then(|v| v.as_str()).unwrap_or("");
    let to = args.get("remapTo").and_then(|v| v.as_str()).unwrap_or("");
    if from.trim().is_empty() {
        Vec::new()
    } else {
        vec![(from.to_owned(), to.to_owned())]
    }
}

fn selected(args: &serde_json::Value) -> Option<HashSet<String>> {
    args.get("selected").and_then(|v| v.as_array()).map(|a| {
        a.iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect()
    })
}

/// Validate + plan for the dialog preview.
pub async fn preview(
    state: &Arc<AppState>,
    args: &serde_json::Value,
) -> Result<ValidationDto, String> {
    let text = args
        .get("manifestText")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let manifest = load_manifest(if text.is_empty() { None } else { Some(text) })?;
    let existing: HashSet<String> = state
        .backend
        .list_snapshot()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|t| t.hash)
        .collect();
    Ok(session::preview(
        &manifest,
        &existing,
        selected(args).as_ref(),
        &remaps(args),
    ))
}

/// Start an import as a detached, journaled job. Returns immediately; the
/// dialog polls `import_status`.
pub fn start_import(state: &Arc<AppState>, args: &serde_json::Value) -> Result<(), String> {
    {
        let mut status = state.import_status.lock().unwrap();
        if status.running {
            return Err("an import is already running".into());
        }
        *status = ImportStatus {
            running: true,
            ..Default::default()
        };
    }
    let text = args
        .get("manifestText")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let manifest = load_manifest(if text.is_empty() { None } else { Some(&text) })?;
    let selected = selected(args);
    let remaps = remaps(args);
    let resume = args
        .get("resume")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let state = Arc::clone(state);
    tokio::spawn(async move {
        let outcome = run_import_job(&state, &manifest, selected, remaps, resume).await;
        let mut status = state.import_status.lock().unwrap();
        status.running = false;
        status.done = true;
        match outcome {
            Ok(summary) => {
                let failed_count = summary.failed.len();
                status.added = summary.added;
                status.resumed = summary.resumed;
                status.skipped = summary.skipped;
                status.failed = summary.failed;
                state.log(
                    LogLevel::Info,
                    format!(
                        "import finished: {} added, {} resumed, {} skipped, {} failed",
                        status.added, status.resumed, status.skipped, failed_count
                    ),
                    None,
                );
            }
            Err(e) => {
                status.failed = vec![e.clone()];
                state.log(LogLevel::Error, format!("import failed: {e}"), None);
            }
        }
        state.repoll.notify_one();
    });
    Ok(())
}

async fn run_import_job(
    state: &Arc<AppState>,
    manifest: &Manifest,
    selected: Option<HashSet<String>>,
    remaps: Vec<(String, String)>,
    resume: bool,
) -> Result<session::ImportSummary, String> {
    let backend = state.backend.as_ref();
    let path = journal_path(state.config.config_path.as_deref());
    let mut journal = session::load_journal(&path);
    let manifest_id = ImportJournal::manifest_id(manifest);
    if !resume || journal.manifest_id != manifest_id {
        journal = ImportJournal {
            manifest_id: manifest_id.clone(),
            total: manifest.torrents.len(),
            started_ms: now_ms(),
            ..Default::default()
        };
    }
    let existing: HashSet<String> = backend
        .list_snapshot()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|t| t.hash)
        .collect();
    let planned = session::plan_restore(manifest, &existing, selected.as_ref(), &remaps);
    let mut skipped = 0usize;
    let mut items = Vec::new();
    for item in planned {
        match item.action {
            RestoreAction::Add => items.push(session::PlannedRestore {
                torrent: item.torrent,
                dst_dir: item.dst_dir,
            }),
            _ => skipped += 1,
        }
    }
    let log_state = Arc::clone(state);
    let log: session::ImportLog = Arc::new(move |level, message, hash| {
        let level = match level {
            session::LogLevel::Info => LogLevel::Info,
            session::LogLevel::Warn => LogLevel::Warn,
            session::LogLevel::Error => LogLevel::Error,
        };
        log_state.log(level, message, hash);
    });
    let cancel = Arc::new(AtomicBool::new(false));
    state.import_cancel.store(false, Ordering::Relaxed);
    let done = Arc::new(AtomicBool::new(false));
    let done_mirror = Arc::clone(&done);
    let cancel_mirror = Arc::clone(&cancel);
    let state_mirror = Arc::clone(state);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if done_mirror.load(Ordering::Relaxed)
                || state_mirror.import_cancel.load(Ordering::Relaxed)
            {
                if state_mirror.import_cancel.load(Ordering::Relaxed) {
                    cancel_mirror.store(true, Ordering::Relaxed);
                }
                break;
            }
        }
    });
    let save_path = path.clone();
    let save = move |journal: &ImportJournal| {
        let _ = session::save_journal(&save_path, journal);
    };
    let mut summary =
        session::run_import(backend, &items, &mut journal, &save, &log, &cancel, now_ms).await;
    done.store(true, Ordering::Relaxed);
    summary.skipped = skipped;
    if summary.failed.is_empty() && !cancel.load(Ordering::Relaxed) {
        let _ = std::fs::remove_file(&path);
    }
    Ok(summary)
}

/// Scan another client's resume data into a portable manifest (read-only).
/// `dir` is a path on the *server* host — useful when the server is
/// co-located with the migrated client.
pub fn scan_foreign(client: &str, dir: &str) -> Result<rtorrent_core::foreign::ScanReport, String> {
    let client: ForeignClient = client.parse()?;
    Ok(rtorrent_core::foreign::scan(
        client,
        std::path::Path::new(dir),
        now_ms(),
    ))
}
