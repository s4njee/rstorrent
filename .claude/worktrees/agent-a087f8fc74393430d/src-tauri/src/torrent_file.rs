//! Parsing of `.torrent` files for the Add-torrent dialog.
//!
//! Uses `lava_torrent` to decode the bencoded metainfo into a [`TorrentMeta`]
//! (name, total size, info-hash, private flag, file list, trackers) that the
//! frontend renders as a checkable file tree. This is metadata only — actually
//! loading the torrent goes through the rtorrent `load.raw*` path.

use lava_torrent::torrent::v1::Torrent;

use crate::ipc::{FileNode, TorrentMeta};

/// Read and parse a `.torrent` file at `path`.
pub fn read_metadata(path: &str) -> Result<TorrentMeta, String> {
    let torrent = Torrent::read_from_file(path).map_err(|e| format!("not a valid .torrent: {e}"))?;

    // Build the flat file list. Multi-file torrents expose `files`; single-file
    // torrents don't, so we synthesize a single node from the top-level name.
    let files: Vec<FileNode> = match &torrent.files {
        Some(list) => list
            .iter()
            .map(|f| FileNode {
                // Prefix the torrent name so the tree shows the containing folder.
                path: format!("{}/{}", torrent.name, f.path.to_string_lossy()),
                size: f.length,
                priority: 1,
                progress: 0.0,
                is_dir: false,
            })
            .collect(),
        None => vec![FileNode {
            path: torrent.name.clone(),
            size: torrent.length,
            priority: 1,
            progress: 0.0,
            is_dir: false,
        }],
    };

    // Flatten announce + announce-list into a de-duplicated tracker list.
    let mut trackers: Vec<String> = Vec::new();
    if let Some(a) = &torrent.announce {
        trackers.push(a.clone());
    }
    if let Some(tiers) = &torrent.announce_list {
        for tier in tiers {
            for url in tier {
                if !trackers.contains(url) {
                    trackers.push(url.clone());
                }
            }
        }
    }

    Ok(TorrentMeta {
        name: torrent.name.clone(),
        size: torrent.length,
        info_hash: torrent.info_hash().to_uppercase(),
        is_private: torrent.is_private(),
        files,
        trackers,
    })
}
