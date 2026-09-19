//! Session export/import (V3-22 / LIB-09) for the desktop shell.
//!
//! Export builds a portable manifest from live daemon state (snapshot +
//! per-torrent trackers + best-available sources); import validates it,
//! plans against loaded hashes, and runs a detached job (add stopped →
//! metadata → recheck → resume) with a crash-safe journal beside
//! `settings.json`. Progress and the final report travel through the app log
//! and the import status command the dialog polls.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::AppHandle;

use rtorrent_core::foreign::ForeignClient;
use rtorrent_core::session::{
    self, ExportReport, ImportJournal, Manifest, ManifestSource, RestoreAction, ValidationDto,
};

use crate::ipc::LogLevel;
use crate::state::AppState;

/// `import-journal.json` lives next to the settings file, like the move journal.
pub fn journal_path(settings_path: &std::path::Path) -> PathBuf {
    settings_path
        .parent()
        .map(|p| p.join("import-journal.json"))
        .unwrap_or_else(|| PathBuf::from("import-journal.json"))
}

/// Live import status for the dialog to poll.
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
/// when its file is readable, else the recorded magnet/URL/path as-is.
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
        Ok(bytes) => Some(ManifestSource::TorrentBase64(
            rtorrent_core::session::encode_base64(&bytes),
        )),
        Err(_) => Some(ManifestSource::TorrentPath(trimmed.to_owned())),
    }
}

/// Build the manifest and return its text: snapshot, per-torrent trackers,
/// best sources, global limits. The dialog saves it (native save dialog on
/// desktop, blob download on web) — the backend never picks a path.
pub async fn export_text(app: &AppHandle, state: &Arc<AppState>) -> Result<String, String> {
    Ok(build_manifest(app, state).await?.to_json())
}

/// Build the manifest file at `path`: snapshot, per-torrent trackers, best
/// sources, global limits. Returns torrent counts for the dialog.
pub async fn export_to(
    app: &AppHandle,
    state: &Arc<AppState>,
    path: &std::path::Path,
) -> Result<ExportReport, String> {
    let manifest = build_manifest(app, state).await?;
    let with_sources = manifest
        .torrents
        .iter()
        .filter(|t| t.source.is_some())
        .count();
    let count = manifest.torrents.len();
    let text = manifest.to_json();
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    std::fs::write(path, text).map_err(|e| e.to_string())?;
    state.log(
        app,
        LogLevel::Info,
        format!("exported session: {count} torrent(s), {with_sources} with sources"),
        None,
    );
    Ok(ExportReport {
        torrents: count,
        with_sources,
    })
}

/// Shared manifest builder behind both export commands.
async fn build_manifest(app: &AppHandle, state: &Arc<AppState>) -> Result<Manifest, String> {
    let backend = state.backend();
    let settings = state.settings();
    let rows = backend.list_snapshot().await.map_err(|e| e.to_string())?;
    let mut trackers = HashMap::new();
    let mut sources = HashMap::new();
    let total = rows.len();
    for (index, row) in rows.iter().enumerate() {
        if index % 25 == 0 {
            state.log(
                app,
                LogLevel::Info,
                format!("exporting session… {} of {total}", index + 1),
                None,
            );
        }
        match backend.trackers(&row.hash).await {
            Ok(list) => {
                trackers.insert(row.hash.clone(), list.into_iter().map(|t| t.url).collect());
            }
            Err(e) => {
                state.log(
                    app,
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
    Ok(session::export_manifest(
        &rows,
        &trackers,
        &sources,
        settings.down_limit_kb,
        settings.up_limit_kb,
        now_ms(),
        env!("CARGO_PKG_VERSION"),
    ))
}

/// Load a manifest from exactly one of a file path or pasted/uploaded text.
/// Both shells share this so desktop (native picker → path) and web
/// (`<input type=file>` → text) run the same validation and import.
fn load_manifest(path: Option<&str>, manifest_text: Option<&str>) -> Result<Manifest, String> {
    match (path, manifest_text) {
        (Some(path), None) => {
            let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            Manifest::parse(&text)
        }
        (None, Some(text)) => Manifest::parse(text),
        (Some(_), Some(_)) => Err("pass either a file or manifest text, not both".into()),
        (None, None) => Err("no manifest given — pick a file or paste manifest text".into()),
    }
}

/// Scan another client's resume data (V3-22 / LIB-10) into a portable
/// manifest. Read-only against the other client; the returned
/// `manifest_text` flows through the normal validate → import path.
pub fn scan_foreign(client: &str, dir: &str) -> Result<rtorrent_core::foreign::ScanReport, String> {
    let client: ForeignClient = client.parse()?;
    Ok(rtorrent_core::foreign::scan(
        client,
        std::path::Path::new(dir),
        now_ms(),
    ))
}

/// Validate + plan for the dialog preview, from a file path or pasted text.
pub async fn preview(
    state: &Arc<AppState>,
    path: Option<&str>,
    manifest_text: Option<&str>,
    selected: Option<HashSet<String>>,
    remaps: Vec<(String, String)>,
) -> Result<ValidationDto, String> {
    let manifest = load_manifest(path, manifest_text)?;
    let existing: HashSet<String> = state
        .backend()
        .list_snapshot()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|t| t.hash)
        .collect();
    Ok(session::preview(
        &manifest,
        &existing,
        selected.as_ref(),
        &remaps,
    ))
}

/// Run an import as a detached job with a crash-safe journal. The manifest
/// comes from a file path or pasted text (exactly one). Already-loaded and
/// invalid entries are never touched; `resume` continues an interrupted
/// journal for the same manifest instead of starting over.
pub fn start_import(
    app: &AppHandle,
    state: &Arc<AppState>,
    path: Option<&str>,
    manifest_text: Option<&str>,
    selected: Option<HashSet<String>>,
    remaps: Vec<(String, String)>,
    resume: bool,
) -> Result<(), String> {
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
    let manifest = load_manifest(path, manifest_text)?;
    let app = app.clone();
    let state = Arc::clone(state);
    tauri::async_runtime::spawn(async move {
        let outcome = run_import_job(
            &app,
            Arc::clone(&state),
            &manifest,
            selected,
            remaps,
            resume,
        )
        .await;
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
                    &app,
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
                state.log(&app, LogLevel::Error, format!("import failed: {e}"), None);
            }
        }
        state.repoll.notify_one();
    });
    Ok(())
}

async fn run_import_job(
    app: &AppHandle,
    state: Arc<AppState>,
    manifest: &Manifest,
    selected: Option<HashSet<String>>,
    remaps: Vec<(String, String)>,
    resume: bool,
) -> Result<session::ImportSummary, String> {
    let backend = state.backend();
    let journal_path = journal_path(&state.settings_path);
    let mut journal = session::load_journal(&journal_path);
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
    let app_log = app.clone();
    let log_state = Arc::clone(&state);
    let log: session::ImportLog = Arc::new(move |level, message, hash| {
        let level = match level {
            session::LogLevel::Info => LogLevel::Info,
            session::LogLevel::Warn => LogLevel::Warn,
            session::LogLevel::Error => LogLevel::Error,
        };
        log_state.log(&app_log, level, message, hash);
    });
    let cancel = Arc::new(AtomicBool::new(false));
    state.import_cancel.store(false, Ordering::Relaxed);
    // The driver polls a plain flag; mirror the shared one into it, and stop
    // mirroring once the run ends so the watcher task exits.
    let done = Arc::new(AtomicBool::new(false));
    let done_mirror = Arc::clone(&done);
    let cancel_mirror = Arc::clone(&cancel);
    let state_mirror = Arc::clone(&state);
    tauri::async_runtime::spawn(async move {
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
    let save_path = journal_path.clone();
    let save = move |journal: &ImportJournal| {
        let _ = session::save_journal(&save_path, journal);
    };
    let mut summary = session::run_import(
        backend.as_ref(),
        &items,
        &mut journal,
        &save,
        &log,
        &cancel,
        now_ms,
    )
    .await;
    done.store(true, Ordering::Relaxed);
    summary.skipped = skipped;
    // A clean finish retires the journal; anything unfinished stays for resume.
    if summary.failed.is_empty() && !cancel.load(Ordering::Relaxed) {
        let _ = std::fs::remove_file(&journal_path);
    }
    Ok(summary)
}
