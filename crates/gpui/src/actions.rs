//! Commands as GPUI actions, key bindings and native menus.
//!
//! The Tauri shell split these across `src-tauri/src/menu.rs` (native menus),
//! `src/hooks/useKeyboard.ts` (in-window shortcuts) and `src/actions.ts` (the
//! shared implementations). Here they are one table: an action type per
//! command, a binding per shortcut, and a menu item that dispatches the same
//! action — so the menu, the toolbar, the context menu and the keyboard cannot
//! drift apart.
//!
//! Views handle the actions they own with `on_action`; anything unhandled
//! reaches the shell.

use gpui_kit::{KeyBinding, Menu, MenuItem, SystemMenuType};

gpui_kit::actions!(
    rstorrent,
    [
        // The application menu, which GPUI leaves to the app (unlike services).
        About,
        Hide,
        HideOthers,
        ShowAll,
        Quit,
        // Selection verbs, shared by the toolbar, context menu and keyboard.
        StartSelection,
        StopSelection,
        ToggleSelection,
        RecheckSelection,
        ForceReannounce,
        RemoveSelection,
        QueueUp,
        QueueDown,
        QueueTop,
        QueueBottom,
        ToggleForceStart,
        SuperSeeding,
        CopyMagnet,
        OpenDestination,
        SetLocation,
        /// Cancel the single selection's live move-on-complete move (V3-14).
        CancelMove,
        /// Retry the single selection's failed/cancelled move (V3-14).
        RetryMove,
        SelectAll,
        ClearSelection,
        OpenLabelDialog,
        OpenTagDialog,
        OpenRateLimitDialog,
        // Add flows.
        AddFile,
        AddMagnet,
        CreateTorrent,
        /// Remove, asking for the downloaded files too (⇧Del).
        RemoveSelectionWithData,
        // Window-level surfaces.
        FocusFilter,
        OpenPreferences,
        OpenStatistics,
        OpenSession,
        ToggleTurtle, // Daemon lifecycle. Start launches a local rtorrent, preferring the
        // runtime this build ships (see `daemon`); the web UI is that daemon
        // served to a browser from inside this app (see `web_host`).
        StartDaemon,
        ToggleWebUi,
        SaveSession,
        ShutdownDaemon,
        // The detail panel's row menus. Each acts on the row the menu was
        // opened over, which the panel records before opening it: a context
        // menu carries no target of its own.
        PeerSnub,
        PeerDisconnect,
        PeerBan,
        TrackerToggle,
        TrackerRemove,
        // Overlay unwind, bound to Escape.
        DismissOverlay
    ]
);

/// Assign a label to the selection.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = rstorrent, no_json)]
pub struct SetLabel(pub String);

/// Apply a per-torrent rate limit, in KiB/s; zero in both directions clears it.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = rstorrent, no_json)]
pub struct SetRateLimit {
    pub down_kb: i64,
    pub up_kb: i64,
}

/// Set the priority of the file rows the panel has selected: 0 skip, 1 normal,
/// 2 high.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = rstorrent, no_json)]
pub struct SetFilePriority(pub i64);

/// Set the create dialog's piece length in bytes; 0 lets the creator choose.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = rstorrent, no_json)]
pub struct SetPieceLength(pub i64);

/// The key context `gpui-base`'s `Input` binds in. Shortcuts that would type
/// into a field stay out of it.
const TEXT_INPUT: &str = "Input";

/// Bindings for every shortcut, including the ones the native menu also shows.
///
/// Single-key shortcuts (`space`, `backspace`, `escape`) are scoped with `!Input`
/// so they cannot fire while a field has focus; ⌘-combos stay global, because
/// none of them collide with a text field's bindings.
#[must_use]
pub fn key_bindings() -> Vec<KeyBinding> {
    let not_typing = Some(format!("!{TEXT_INPUT}"));
    vec![
        // Application.
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        // Add flows, matching the native menu's accelerators.
        KeyBinding::new("cmd-o", AddFile, None),
        KeyBinding::new("cmd-shift-o", AddMagnet, None),
        KeyBinding::new("cmd-n", CreateTorrent, not_typing.as_deref()),
        // Window surfaces.
        KeyBinding::new("cmd-f", FocusFilter, None),
        KeyBinding::new("cmd-,", OpenPreferences, None),
        KeyBinding::new("cmd-a", SelectAll, not_typing.as_deref()),
        KeyBinding::new("alt-cmd-r", RecheckSelection, None),
        // Selection verbs.
        KeyBinding::new("space", ToggleSelection, not_typing.as_deref()),
        KeyBinding::new("backspace", RemoveSelection, not_typing.as_deref()),
        KeyBinding::new("delete", RemoveSelection, not_typing.as_deref()),
        // ⇧ opens the same confirmation with the data box already ticked.
        KeyBinding::new(
            "shift-backspace",
            RemoveSelectionWithData,
            not_typing.as_deref(),
        ),
        KeyBinding::new(
            "shift-delete",
            RemoveSelectionWithData,
            not_typing.as_deref(),
        ),
        // Global, not guarded against a text field: a dialog must dismiss on
        // Escape even while one of its fields has focus.
        KeyBinding::new("escape", DismissOverlay, None),
    ]
}

fn app_menu() -> Menu {
    Menu::new("rstorrent").items([
        MenuItem::action("About rstorrent", About),
        MenuItem::separator(),
        MenuItem::action("Preferences…", OpenPreferences),
        MenuItem::separator(),
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        MenuItem::separator(),
        MenuItem::action("Hide rstorrent", Hide),
        MenuItem::action("Hide Others", HideOthers),
        MenuItem::action("Show All", ShowAll),
        MenuItem::separator(),
        MenuItem::action("Quit rstorrent", Quit),
    ])
}

/// The add flows. macOS keeps them in the Torrent menu, as the Tauri shell's
/// macOS menu bar does; everywhere else they live in File.
fn add_items() -> Vec<MenuItem> {
    vec![
        MenuItem::action("Add Torrent File…", AddFile),
        MenuItem::action("Add Magnet Link…", AddMagnet),
        MenuItem::action("Create Torrent…", CreateTorrent),
    ]
}

fn file_menu() -> Menu {
    let mut items = add_items();
    items.push(MenuItem::separator());
    items.push(MenuItem::action("Session export / import…", OpenSession));
    items.push(MenuItem::action("Preferences…", OpenPreferences));
    Menu::new("File").items(items)
}

fn torrent_menu() -> Menu {
    let mut items = if cfg!(target_os = "macos") {
        add_items()
    } else {
        Vec::new()
    };
    if !items.is_empty() {
        items.push(MenuItem::separator());
    }
    items.extend([
        MenuItem::action("Resume", StartSelection),
        MenuItem::action("Pause", StopSelection),
        MenuItem::separator(),
        MenuItem::action("Force Recheck", RecheckSelection),
        MenuItem::action("Force Reannounce", ForceReannounce),
        MenuItem::separator(),
        MenuItem::action("Set Label…", OpenLabelDialog),
        MenuItem::action("Edit Tags…", OpenTagDialog),
        MenuItem::action("Limit Rates…", OpenRateLimitDialog),
        MenuItem::action("Set Location…", SetLocation),
        MenuItem::separator(),
        MenuItem::action("Copy Magnet Link", CopyMagnet),
        MenuItem::action("Open Destination", OpenDestination),
        MenuItem::separator(),
        MenuItem::action("Remove…", RemoveSelection),
    ]);
    Menu::new("Torrent").items(items)
}

fn daemon_menu() -> Menu {
    Menu::new("Daemon").items([
        // Starting a local daemon first, split from the lifecycle verbs below,
        // as the Tauri shell's menu orders them.
        MenuItem::action("Start Daemon", StartDaemon),
        MenuItem::separator(),
        // The same daemon, to a browser. One item that toggles, because this
        // menu is built once and its labels cannot follow the state.
        MenuItem::action("Toggle Web UI", ToggleWebUi),
        MenuItem::separator(),
        MenuItem::action("Toggle Turtle Mode", ToggleTurtle),
        MenuItem::action("Statistics", OpenStatistics),
        MenuItem::separator(),
        MenuItem::action("Save Session", SaveSession),
        MenuItem::action("Shut Down Daemon…", ShutdownDaemon),
    ])
}

/// The application menus. macOS gets the app menu first, as the platform
/// expects; every other platform gets File instead.
#[must_use]
pub fn menus() -> Vec<Menu> {
    let mut menus = Vec::new();
    if cfg!(target_os = "macos") {
        menus.push(app_menu());
    } else {
        menus.push(file_menu());
    }
    menus.push(torrent_menu());
    menus.push(daemon_menu());
    menus
}

/// Bind the shortcuts, install the application-level actions, and publish the
/// menus. Call once at startup, after `gpui_kit::init`.
pub fn install(cx: &mut gpui_kit::App) {
    cx.bind_keys(key_bindings());
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &About, _| {
        // A proper About sheet is not ported yet; saying so beats doing nothing.
        eprintln!("rstorrent: no About sheet yet");
    });
    cx.set_menus(menus());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shortcut_parses() {
        // `KeyBinding::new` panics on a keystroke or context GPUI cannot parse,
        // so building them all is the check.
        assert!(!key_bindings().is_empty());
    }

    #[test]
    fn menus_cover_the_ported_commands() {
        let menus = menus();
        let names: Vec<String> = menus
            .iter()
            .flat_map(|menu| menu.items.iter())
            .filter_map(|item| match item {
                MenuItem::Action { name, .. } => Some(name.to_string()),
                _ => None,
            })
            .collect();
        for expected in [
            "Add Torrent File…",
            "Add Magnet Link…",
            "Create Torrent…",
            "Preferences…",
            "Resume",
            "Pause",
            "Remove…",
            "Statistics",
            "Start Daemon",
        ] {
            assert!(names.iter().any(|name| name == expected), "{expected}");
        }
    }

    #[test]
    fn macos_puts_the_app_menu_first() {
        let menus = menus();
        if cfg!(target_os = "macos") {
            assert_eq!(menus[0].name.as_ref(), "rstorrent");
        } else {
            assert_eq!(menus[0].name.as_ref(), "File");
        }
    }
}
