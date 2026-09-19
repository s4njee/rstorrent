//! rstorrent's native GPUI application.
//!
//! The GPUI shell is an alternative desktop front end to the existing Tauri
//! shell. It starts with the same visual vocabulary (title bar, toolbar,
//! filter sidebar, virtualised torrent table, status bar) and keeps
//! daemon/model work behind modules so those pieces can be developed and
//! tested separately.

// A GUI app: no console window beside it in a release build. Debug builds keep
// the console for logs. (`--where-rtorrent` and friends print nothing from a
// release exe until it attaches to the parent console — see windows.md.)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::borrow::Cow;
use std::sync::Arc;

use gpui_kit::component::{Theme, ThemeMode};
use rstorrent_gpui::{actions, services::Services, settings::SettingsStore, shell, theme};

#[cfg(target_os = "macos")]
#[path = "app_icon.rs"]
mod app_icon;

fn bundled_fonts() -> Vec<Cow<'static, [u8]>> {
    vec![
        Cow::Borrowed(include_bytes!("../assets/IBMPlexSans-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/IBMPlexSans-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../assets/IBMPlexSans-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/IBMPlexMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/IBMPlexMono-Medium.ttf")),
    ]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let settings_store = SettingsStore::app_default();

    // Report the daemon this build would launch, then exit. Nothing else can be
    // asked without a window, and which rtorrent gets started is the one thing
    // about a bundle that is otherwise invisible (see `daemon::describe`).
    if args.iter().any(|arg| arg == "--where-rtorrent") {
        println!("{}", rstorrent_gpui::daemon::describe());
        return;
    }

    // Start the daemon exactly as the Daemon menu does, and report what
    // happened. The shell has no window to drive here, so this is how the QA
    // checklist's "Start rtorrent launches the bundled 0.15.7" is run without
    // one (and how a failure is reproduced in a terminal).
    if args.iter().any(|arg| arg == "--start-rtorrent") {
        let transport = settings_store.load().transport;
        match rstorrent_gpui::daemon::start(&transport) {
            Ok(message) => println!("{message}"),
            Err(error) => {
                eprintln!("error: {error}");
                std::process::exit(1);
            }
        }
        return;
    }

    // Mock mode is for a session, not a standing preference: a Preferences
    // toggle left on (or written by the Tauri shell, which shares the file)
    // must not boot a release app into fixtures. Only `--demo` or
    // `RSTORRENT_MOCK` turn it on at launch.
    let demo = args.iter().any(|arg| arg == "--demo");
    let mut settings = settings_store.load();
    settings.mock = demo || std::env::var_os("RSTORRENT_MOCK").is_some();
    let services =
        Arc::new(Services::new(settings).expect("the native GPUI service runtime starts"));
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            // The component library starts light, which would draw its menus and
            // popovers dark-on-dark over the Dark Ops palette. Switch it, then
            // dress it in that palette.
            Theme::change(ThemeMode::Dark, None, cx);
            theme::apply_to_components(cx);
            actions::install(cx);
            // Inside a .app the Dock already has the bundle's icon, masked to
            // the system shape; overriding it shows the raw, uncropped ICNS.
            #[cfg(target_os = "macos")]
            if !app_icon::in_bundle() {
                app_icon::install();
            }

            cx.text_system()
                .add_fonts(bundled_fonts())
                .expect("bundled IBM Plex fonts load");

            shell::open_main_window(Arc::clone(&services), settings_store.clone(), cx);
            cx.activate(true);
        });
}
