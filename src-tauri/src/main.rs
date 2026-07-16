// Binary entry point for the rstorrent desktop app.
//
// All real setup lives in the library crate (`lib.rs`) so it can be shared with
// mobile entry points and integration tests; `main` just forwards to it.

// Prevents an extra console window on Windows in release builds. Harmless on macOS.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    rstorrent_lib::run()
}
