//! Watched-folder auto-add (E11-S5; multiple folders C12).
//!
//! For each configured watch folder, this scans it for `.torrent` files and
//! loads each into rtorrent, renaming successfully-loaded files to
//! `*.torrent.loaded` so they aren't re-added. A `notify` watcher triggers a
//! (debounced) rescan on any change; each folder is also scanned once at
//! startup.
//!
//! Each folder can carry its own label and save path; an empty save path falls
//! back to the label default (C11), then the global default. Robustness: each
//! file is parse-validated before loading (a half-written copy simply fails to
//! parse and is retried on the next event), and only the extension `.torrent` is
//! considered — already-loaded files are skipped.
//!
//! The folder list is read once at startup; changing it takes effect on next
//! launch.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use tauri::AppHandle;

use crate::ipc::{LogLevel, WatchFolder};
use crate::rtorrent::LoadOptions;
use crate::settings;
use crate::state::AppState;
use crate::torrent_file;

/// Start watching every configured folder.
pub fn spawn(app: AppHandle, state: Arc<AppState>) {
    for folder in state.settings().watch_folders {
        if folder.path.is_empty() {
            continue;
        }
        watch_one(app.clone(), state.clone(), folder);
    }
}

/// Watch a single folder: a `notify` thread pings a tokio task that rescans.
fn watch_one(app: AppHandle, state: Arc<AppState>, folder: WatchFolder) {
    let path = PathBuf::from(&folder.path);
    if !path.is_dir() {
        state.log(
            &app,
            LogLevel::Warn,
            format!("watch folder does not exist: {}", folder.path),
            None,
        );
        return;
    }
    state.log(
        &app,
        LogLevel::Info,
        format!("watching {}", folder.path),
        None,
    );

    // notify runs its callback on its own thread; forward "something changed"
    // pings into a tokio channel that the async processor drains.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(16);
    let _ = tx.try_send(()); // trigger the initial scan

    let watch_path = path.clone();
    std::thread::spawn(move || {
        let ping = tx.clone();
        let mut watcher =
            match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if res.is_ok() {
                    let _ = ping.try_send(());
                }
            }) {
                Ok(w) => w,
                Err(_) => return,
            };
        if watcher
            .watch(&watch_path, RecursiveMode::NonRecursive)
            .is_err()
        {
            return;
        }
        // Park this thread forever to keep `watcher` alive.
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    });

    tauri::async_runtime::spawn(async move {
        while rx.recv().await.is_some() {
            // Debounce: coalesce a burst of events (e.g. a multi-write copy).
            tokio::time::sleep(Duration::from_millis(400)).await;
            while rx.try_recv().is_ok() {}
            process_dir(&app, &state, &path, &folder).await;
        }
    });
}

/// Scan one folder and load any pending `.torrent` files with the folder's
/// label and (resolved) save path.
async fn process_dir(app: &AppHandle, state: &Arc<AppState>, dir: &Path, folder: &WatchFolder) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let settings = state.settings();
    let backend = state.backend();

    // Resolve the save path once per scan: the folder override, else the label
    // default (C11), else the global default. Translate to the daemon namespace
    // (a WSL daemon can't open a Windows path).
    let resolved = if folder.save_path.is_empty() {
        settings::save_path_for_label(&settings, &folder.label)
    } else {
        folder.save_path.clone()
    };
    let directory = crate::localfs::to_daemon_path(&resolved).unwrap_or(resolved);

    for entry in entries.flatten() {
        let path = entry.path();
        // Only files whose extension is exactly "torrent" (skips *.loaded).
        if path.extension().and_then(|e| e.to_str()) != Some("torrent") {
            continue;
        }
        let path_str = path.to_string_lossy().to_string();

        // Validate before loading; a partial write just fails and is retried.
        if torrent_file::read_metadata(&path_str).is_err() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };

        let opts = LoadOptions {
            directory: directory.clone(),
            label: folder.label.clone(),
            start: true,
            top_of_queue: false,
            unselected_indexes: vec![],
        };
        match backend.load_raw(bytes, opts).await {
            Ok(_) => {
                // Rename so it isn't picked up again.
                let loaded = PathBuf::from(format!("{path_str}.loaded"));
                let _ = std::fs::rename(&path, &loaded);
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                state.log(app, LogLevel::Info, format!("watch: added {name}"), None);
                state.repoll.notify_one();
            }
            Err(err) => {
                state.log(
                    app,
                    LogLevel::Warn,
                    format!("watch: failed to add {path_str}: {err}"),
                    None,
                );
            }
        }
    }
}
