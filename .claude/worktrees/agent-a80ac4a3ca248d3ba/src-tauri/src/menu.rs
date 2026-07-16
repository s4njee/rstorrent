//! Native macOS application menu.
//!
//! Builds the menubar (App / Torrent / Edit) with the conventional accelerators
//! and forwards clicks on the custom items to the frontend via a `menu://action`
//! event, which `App.tsx` maps to the corresponding dialog. The Edit menu uses
//! predefined items so system clipboard shortcuts work inside text inputs.

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
        .build()?;

    // Edit menu: predefined clipboard actions (so ⌘C/⌘V work in dialog inputs).
    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let menu = MenuBuilder::new(app)
        .items(&[&app_menu, &torrent_menu, &edit_menu])
        .build()?;
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
