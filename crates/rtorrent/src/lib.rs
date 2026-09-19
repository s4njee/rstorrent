//! `rtorrent-core` — the host-agnostic rtorrent client layer.
//!
//! Everything needed to talk to an rtorrent daemon and shape its state into the
//! wire contract, with **no dependency on Tauri or any UI shell**. Both the
//! desktop app (`src-tauri`) and the `rstorrent-web` server depend on this crate,
//! so there is exactly one implementation of the protocol, the status derivation,
//! and the DTOs.
//!
//! Module map:
//!   * [`types`] — the shared serde DTO contract (mirrors `src/ipc/types.ts`).
//!   * [`rtorrent`] — the [`rtorrent::RtorrentApi`] trait, the SCGI/HTTP
//!     transports, the XML-RPC dialect, status derivation, and the fixture
//!     [`rtorrent::mock::MockClient`].
//!   * [`secrets`] — OS-keychain storage for remote-daemon credentials.
//!   * [`torrent_file`] — `.torrent` metadata parsing for the Add dialog / upload.

pub mod bandwidth;
pub mod complete;
pub mod delta;
pub mod duplicates;
pub mod file_index;
pub mod foreign;
pub mod fs;
pub mod mover;
pub mod queue;
pub mod rss;
pub mod rtorrent;
pub mod schedule;
pub mod secrets;
pub mod session;
pub mod snapshot;
pub mod tags;
pub mod torrent_file;
pub mod types;
