//! Menu-bar (system tray) icon and macOS dock menu (B17).
//!
//! Provides a persistent system tray icon with live speed metrics (title/tooltip),
//! a quick-action tray menu (show/hide, add torrent, create torrent, pause/resume all,
//! turtle mode, preferences, quit), left-click toggle, and macOS dock menu support.

use std::sync::Arc;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, Wry};

use crate::state::AppState;

const UNITS: &[&str] = &["B/s", "KB/s", "MB/s", "GB/s"];

/// Format transfer rate into human-readable tray string.
pub fn format_tray_speed(bytes_per_sec: i64) -> String {
    if bytes_per_sec <= 0 {
        return "0 B/s".to_string();
    }
    let mut value = bytes_per_sec as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 || value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Toggle visibility and focus of the main window.
pub fn toggle_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let is_visible = window.is_visible().unwrap_or(false);
        let is_minimized = window.is_minimized().unwrap_or(false);
        let is_focused = window.is_focused().unwrap_or(false);

        if is_visible && !is_minimized && is_focused {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }
}

/// Bring the main window to the front.
pub fn focus_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Build the tray menu.
fn build_tray_menu(app: &App) -> tauri::Result<Menu<Wry>> {
    let show_hide = MenuItemBuilder::with_id("tray:show-hide", "Show / Hide Window").build(app)?;
    let add_file = MenuItemBuilder::with_id("tray:add-file", "Add Torrent File…").build(app)?;
    let add_magnet = MenuItemBuilder::with_id("tray:add-magnet", "Add Magnet Link…").build(app)?;
    let create_torrent =
        MenuItemBuilder::with_id("tray:create-torrent", "Create Torrent…").build(app)?;

    let resume_all = MenuItemBuilder::with_id("tray:resume-all", "Resume All").build(app)?;
    let pause_all = MenuItemBuilder::with_id("tray:pause-all", "Pause All").build(app)?;
    let toggle_turtle =
        MenuItemBuilder::with_id("tray:toggle-turtle", "Toggle Turtle Mode (Limit Speed)")
            .build(app)?;

    let prefs = MenuItemBuilder::with_id("tray:prefs", "Preferences…").build(app)?;
    let quit = MenuItemBuilder::with_id("tray:quit", "Quit rtorrent").build(app)?;

    MenuBuilder::new(app)
        .item(&show_hide)
        .separator()
        .item(&add_file)
        .item(&add_magnet)
        .item(&create_torrent)
        .separator()
        .item(&resume_all)
        .item(&pause_all)
        .item(&toggle_turtle)
        .separator()
        .item(&prefs)
        .separator()
        .item(&quit)
        .build()
}

/// Initialize the system tray icon and dock menu.
pub fn setup(app: &App) -> tauri::Result<()> {
    let menu = build_tray_menu(app)?;

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .tooltip("rstorrent")
        .show_menu_on_left_click(false);

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main_window(tray.app_handle());
            }
        })
        .on_menu_event(|handle, event| {
            handle_tray_action(handle, event.id().0.as_str());
        })
        .build(app)?;

    Ok(())
}

/// Handle a click from the tray menu or macOS dock menu.
pub fn handle_tray_action(app: &AppHandle, action: &str) {
    match action {
        "tray:show-hide" => toggle_main_window(app),
        "tray:add-file" => {
            focus_main_window(app);
            let _ = app.emit("menu://action", "add-file");
        }
        "tray:add-magnet" => {
            focus_main_window(app);
            let _ = app.emit("menu://action", "add-magnet");
        }
        "tray:create-torrent" => {
            focus_main_window(app);
            let _ = app.emit("menu://action", "create-torrent");
        }
        "tray:prefs" => {
            focus_main_window(app);
            let _ = app.emit("menu://action", "prefs");
        }
        "tray:resume-all" => {
            let state = app.state::<Arc<AppState>>().inner().clone();
            tauri::async_runtime::spawn(async move {
                let backend = state.backend();
                if let Ok(torrents) = backend.list_snapshot().await {
                    let paused_hashes: Vec<String> = torrents
                        .into_iter()
                        .filter(|t| !t.is_active)
                        .map(|t| t.hash)
                        .collect();
                    if !paused_hashes.is_empty() {
                        let _ = backend.start(&paused_hashes).await;
                        state.repoll.notify_one();
                    }
                }
            });
        }
        "tray:pause-all" => {
            let state = app.state::<Arc<AppState>>().inner().clone();
            tauri::async_runtime::spawn(async move {
                let backend = state.backend();
                if let Ok(torrents) = backend.list_snapshot().await {
                    let active_hashes: Vec<String> = torrents
                        .into_iter()
                        .filter(|t| t.is_active)
                        .map(|t| t.hash)
                        .collect();
                    if !active_hashes.is_empty() {
                        let _ = backend.stop(&active_hashes).await;
                        state.repoll.notify_one();
                    }
                }
            });
        }
        "tray:toggle-turtle" => {
            let state = app.state::<Arc<AppState>>();
            let mut settings = state.settings();
            settings.turtle_enabled = !settings.turtle_enabled;
            let _ = state.update_settings(settings);
            state.repoll.notify_one();
        }
        "tray:quit" => {
            app.exit(0);
        }
        _ => {}
    }
}

/// Update the tray icon title and tooltip with live metrics.
pub fn update_tray(
    app: &AppHandle,
    down_rate: i64,
    up_rate: i64,
    active_count: usize,
    turtle: bool,
) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        let down_str = format_tray_speed(down_rate);
        let up_str = format_tray_speed(up_rate);
        let turtle_str = if turtle { " [🐢]" } else { "" };

        let title = format!("↓ {down_str}  ↑ {up_str}{turtle_str}");
        let tooltip = format!(
            "rstorrent{turtle_str}\n↓ {down_str}  ↑ {up_str}\n{active_count} active torrents"
        );

        let _ = tray.set_title(Some(title));
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_speed_zero_and_negative() {
        assert_eq!(format_tray_speed(0), "0 B/s");
        assert_eq!(format_tray_speed(-10), "0 B/s");
    }

    #[test]
    fn format_speed_units_and_precision() {
        assert_eq!(format_tray_speed(500), "500 B/s");
        assert_eq!(format_tray_speed(1536), "1.5 KB/s");
        assert_eq!(format_tray_speed(10485760), "10 MB/s");
        assert_eq!(format_tray_speed(1572864), "1.5 MB/s");
        assert_eq!(format_tray_speed(1073741824), "1.0 GB/s");
    }
}
