//! Native GPUI application library for rstorrent.
//!
//! The binary is deliberately thin. Keeping the port's models, services and
//! views in a library makes their public contracts testable and prevents the
//! compiler from treating not-yet-wired parity surfaces as dead binary code.

pub mod actions;
pub mod add_dialogs;
pub mod add_source;
pub mod app_log;
pub mod columns;
pub mod create_torrent;
pub mod daemon;
pub mod daemon_wsl;
pub mod detail_facts;
pub mod detail_files;
pub mod detail_panel;
pub mod detail_panes;
pub mod detail_pieces;
pub mod detail_rows;
pub mod dialogs;
pub mod format;
pub mod icons;
pub mod localfs;
pub mod model;
pub mod network_prefs;
pub mod notifications;
pub mod policy;
pub mod prefs;
pub mod rate_history;
pub mod rss;
pub mod services;
pub mod session_dialog;
pub mod settings;
pub mod shell;
pub mod sidebar;
pub mod stats;
pub mod table_state;
pub mod theme;
pub mod throttles;
pub mod torrent_table;
pub mod turtle;
pub mod web_host;
pub mod wsl;
