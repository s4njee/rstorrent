//! Native application menu.
//!
//! Forwards clicks on the custom items to the frontend via a `menu://action`
//! event, which `App.tsx` maps to the corresponding dialog. The Edit menu uses
//! predefined items so system clipboard shortcuts work inside text inputs.
//!
//! The layout is per-platform, because the conventions genuinely differ: macOS
//! puts About/Preferences/Quit under an app-named first menu and owns the
//! menubar globally, while Windows expects File/Edit/Help inside the window and
//! has no equivalent of Hide Others / Show All.

use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{App, Emitter};

/// Construct and install the app menu, and wire its event handler.
pub fn setup(app: &App) -> tauri::Result<()> {
    // Custom items carry a `menu:<action>` id we translate into a dialog open.
    let prefs = MenuItemBuilder::with_id("menu:prefs", "Preferences…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let add_file = MenuItemBuilder::with_id("menu:add-file", "Add Torrent File…")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    let add_magnet = MenuItemBuilder::with_id("menu:add-magnet", "Add Magnet Link…")
        .accelerator("CmdOrCtrl+Shift+O")
        .build(app)?;
    let stats = MenuItemBuilder::with_id("menu:stats", "Statistics").build(app)?;
    let tune = MenuItemBuilder::with_id("menu:tune-network", "Tune for 1 Gbps…").build(app)?;

    // Daemon lifecycle (D13): write the session, or shut the daemon down.
    let save_session = MenuItemBuilder::with_id("menu:save-session", "Save Session").build(app)?;
    let shutdown =
        MenuItemBuilder::with_id("menu:shutdown-daemon", "Shut Down Daemon…").build(app)?;
    let daemon_menu = SubmenuBuilder::new(app, "Daemon")
        .item(&save_session)
        .separator()
        .item(&shutdown)
        .build()?;

    // Edit menu: predefined clipboard actions, so the system shortcuts keep
    // working inside dialog text inputs on both platforms.
    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    #[cfg(target_os = "macos")]
    let menu = {
        // App menu: About + Preferences + standard hide/quit.
        let app_menu = SubmenuBuilder::new(app, "rstorrent")
            .about(Some(Default::default()))
            .separator()
            .item(&prefs)
            .separator()
            .hide()
            .hide_others()
            .show_all()
            .separator()
            .quit()
            .build()?;

        // Torrent menu: the add flows + statistics.
        let torrent_menu = SubmenuBuilder::new(app, "Torrent")
            .item(&add_file)
            .item(&add_magnet)
            .separator()
            .item(&stats)
            .item(&tune)
            .build()?;

        MenuBuilder::new(app)
            .items(&[&app_menu, &torrent_menu, &daemon_menu, &edit_menu])
            .build()?
    };

    #[cfg(not(target_os = "macos"))]
    let menu = {
        // File carries the add flows, Preferences and Exit — the Windows
        // convention, where there is no app-named menu to hold them.
        let file_menu = SubmenuBuilder::new(app, "File")
            .item(&add_file)
            .item(&add_magnet)
            .separator()
            .item(&stats)
            .item(&tune)
            .separator()
            .item(&prefs)
            .separator()
            .quit()
            .build()?;

        let help_menu = SubmenuBuilder::new(app, "Help")
            .about(Some(Default::default()))
            .build()?;

        MenuBuilder::new(app)
            .items(&[&file_menu, &daemon_menu, &edit_menu, &help_menu])
            .build()?
    };

    app.set_menu(menu)?;

    // Forward custom item clicks to the frontend.
    app.on_menu_event(move |handle, event| {
        let id = event.id().0.as_str();
        if let Some(action) = id.strip_prefix("menu:") {
            let _ = handle.emit("menu://action", action.to_string());
        }
    });

    Ok(())
}
