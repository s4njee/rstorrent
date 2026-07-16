//! rstorrent library crate: app setup and wiring.
//!
//! `run()` builds the Tauri app — registers plugins, constructs the shared
//! [`AppState`], starts the background poller, and exposes the command surface.
//! Module map:
//!   * [`ipc`]         — serde types shared with the frontend (mirrors `src/ipc/types.ts`).
//!   * [`rtorrent`]    — all daemon communication (SCGI/XML-RPC) + the mock backend.
//!   * [`state`]       — the shared `AppState` (backend, settings, log, caches).
//!   * [`settings`]    — settings model + JSON persistence.
//!   * [`log`]         — bounded app event log.
//!   * [`poller`]      — background polling loops that push snapshots to the UI.
//!   * [`commands`]    — `#[tauri::command]` handlers.
//!   * [`torrent_file`]— `.torrent` metadata parsing for the Add dialog.

pub mod ipc;

mod commands;
mod log;
mod menu;
mod notifications;
mod open_requests;
mod poller;
mod rtorrent;
mod settings;
mod state;
mod stats;
mod torrent_file;
mod throttles;
mod watcher;

use std::sync::Arc;

use tauri::Manager;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        // Persist and restore the window's size/position across launches.
        .plugin(tauri_plugin_window_state::Builder::default().build());

    // LaunchServices handles both registered URL schemes and document types on
    // macOS. Other platforms would additionally need single-instance handling.
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_plugin_deep_link::init());

    let app = builder
        .setup(|app| {
            app.manage(open_requests::OpenRequestState::default());

            // Settings live in the app's config dir; fall back to a temp path if
            // that can't be resolved (keeps the app usable regardless).
            let settings_path = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::env::temp_dir())
                .join("settings.json");

            let app_state = Arc::new(AppState::new(settings_path));
            app.manage(app_state.clone());

            // Install the native menubar (forwards to the frontend via events).
            menu::setup(app)?;

            // Kick off the polling loops that keep the UI live.
            poller::spawn(app.handle().clone(), app_state.clone());
            // Start the watched-folder auto-add (no-op if unconfigured).
            watcher::spawn(app.handle().clone(), app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::take_open_requests,
            commands::read_torrent_metadata,
            commands::add_torrent,
            commands::start,
            commands::stop,
            commands::recheck,
            commands::force_reannounce,
            commands::add_tracker,
            commands::remove_tracker,
            commands::set_tracker_enabled,
            commands::remove,
            commands::set_label,
            commands::set_torrent_limits,
            commands::set_location,
            commands::queue_move,
            commands::copy_magnet,
            commands::open_destination,
            commands::set_file_priority,
            commands::get_settings,
            commands::apply_settings,
            commands::test_connection,
            commands::retry_connection,
            commands::set_detail_watch,
            commands::get_log,
            commands::get_statistics,
        ])
        .build(tauri::generate_context!())
        .expect("error while building rstorrent");

    app.run(|app, event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Opened { urls } = event {
            open_requests::receive(app, urls.into_iter().map(|url| url.to_string()).collect());
        }
    });
}
