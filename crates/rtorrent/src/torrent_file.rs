use std::path::Path;

use lava_torrent::bencode::BencodeElem;
use lava_torrent::torrent::v1::{Torrent, TorrentBuilder};

use crate::types::{CreateTorrentOptions, FileNode, TorrentMeta};

/// Read and parse a `.torrent` file at `path` (desktop file picker).
pub fn read_metadata(path: &str) -> Result<TorrentMeta, String> {
    let torrent =
        Torrent::read_from_file(path).map_err(|e| format!("not a valid .torrent: {e}"))?;
    Ok(from_torrent(torrent))
}

/// Parse `.torrent` bytes (the web upload path).
pub fn read_metadata_bytes(bytes: &[u8]) -> Result<TorrentMeta, String> {
    let torrent =
        Torrent::read_from_bytes(bytes).map_err(|e| format!("not a valid .torrent: {e}"))?;
    Ok(from_torrent(torrent))
}

/// Compute standard optimal power-of-two piece size based on total content bytes.
///
/// Keeps total piece count roughly in the 1000..2000 range for efficient swarm distribution.
pub fn optimal_piece_length(total_size: u64) -> i64 {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;

    if total_size <= 16 * MIB {
        (32 * KIB) as i64
    } else if total_size <= 64 * MIB {
        (64 * KIB) as i64
    } else if total_size <= 150 * MIB {
        (128 * KIB) as i64
    } else if total_size <= 350 * MIB {
        (256 * KIB) as i64
    } else if total_size <= 700 * MIB {
        (512 * KIB) as i64
    } else if total_size <= 2048 * MIB {
        MIB as i64
    } else if total_size <= 4096 * MIB {
        (2 * MIB) as i64
    } else if total_size <= 8192 * MIB {
        (4 * MIB) as i64
    } else if total_size <= 16384 * MIB {
        (8 * MIB) as i64
    } else {
        (16 * MIB) as i64
    }
}

/// Calculate the total size in bytes of a single file or recursive directory.
pub fn calculate_path_size(path: &Path) -> Result<u64, String> {
    if path.is_file() {
        path.metadata()
            .map(|m| m.len())
            .map_err(|e| format!("failed to read metadata: {e}"))
    } else if path.is_dir() {
        let mut total = 0u64;
        for entry in
            std::fs::read_dir(path).map_err(|e| format!("failed to read directory: {e}"))?
        {
            let entry = entry.map_err(|e| format!("failed to read entry: {e}"))?;
            let entry_path = entry.path();
            if entry_path.is_file() {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            } else if entry_path.is_dir() {
                total += calculate_path_size(&entry_path)?;
            }
        }
        Ok(total)
    } else {
        Err(format!("invalid path: {}", path.display()))
    }
}

/// Create a new `.torrent` file from the specified options.
///
/// Returns the built [`Torrent`] and its bencoded bytes.
pub fn create_torrent(opts: CreateTorrentOptions) -> Result<(Torrent, Vec<u8>), String> {
    if !opts.source_path.exists() {
        return Err(format!(
            "source path does not exist: {}",
            opts.source_path.display()
        ));
    }

    let piece_length = match opts.piece_length {
        Some(len) if len >= 16384 && (len & (len - 1)) == 0 => len,
        _ => {
            let total_size = calculate_path_size(&opts.source_path)?;
            optimal_piece_length(total_size)
        }
    };

    let mut builder = TorrentBuilder::new(&opts.source_path, piece_length);

    // Group trackers into tiers
    let mut tiers: Vec<Vec<String>> = Vec::new();
    let mut current_tier: Vec<String> = Vec::new();
    for t in opts.trackers {
        let trimmed = t.trim();
        if trimmed.is_empty() {
            if !current_tier.is_empty() {
                tiers.push(std::mem::take(&mut current_tier));
            }
        } else if trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
            || trimmed.starts_with("udp://")
        {
            current_tier.push(trimmed.to_string());
        }
    }
    if !current_tier.is_empty() {
        tiers.push(current_tier);
    }

    if !tiers.is_empty() {
        if let Some(first) = tiers.first().and_then(|tier| tier.first()) {
            builder = builder.set_announce(Some(first.clone()));
        }
        builder = builder.set_announce_list(tiers);
    }

    builder = builder.set_privacy(opts.is_private);

    if let Some(comment) = opts.comment {
        if !comment.trim().is_empty() {
            builder = builder
                .add_extra_field("comment".into(), BencodeElem::String(comment.trim().into()));
        }
    }

    let created_by = opts.created_by.unwrap_or_else(|| "rstorrent".into());
    builder = builder.add_extra_field("created by".into(), BencodeElem::String(created_by));

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if now > 0 {
        builder = builder.add_extra_field("creation date".into(), BencodeElem::Integer(now));
    }

    if let Some(source) = opts.source {
        if !source.trim().is_empty() {
            builder = builder
                .add_extra_info_field("source".into(), BencodeElem::String(source.trim().into()));
        }
    }

    let torrent = builder
        .build()
        .map_err(|e| format!("failed to build torrent: {e}"))?;
    let bytes = torrent
        .clone()
        .encode()
        .map_err(|e| format!("failed to encode torrent: {e}"))?;

    Ok((torrent, bytes))
}

/// Shape a parsed torrent into the [`TorrentMeta`] the frontend renders.
fn from_torrent(torrent: Torrent) -> TorrentMeta {
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

    TorrentMeta {
        name: torrent.name.clone(),
        size: torrent.length,
        info_hash: torrent.info_hash().to_uppercase(),
        is_private: torrent.is_private(),
        files,
        trackers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn optimal_piece_lengths_cover_ranges() {
        assert_eq!(optimal_piece_length(1024), 32 * 1024);
        assert_eq!(optimal_piece_length(50 * 1024 * 1024), 64 * 1024);
        assert_eq!(optimal_piece_length(200 * 1024 * 1024), 256 * 1024);
        assert_eq!(optimal_piece_length(1024 * 1024 * 1024), 1024 * 1024);
        assert_eq!(
            optimal_piece_length(5 * 1024 * 1024 * 1024),
            4 * 1024 * 1024
        );
        assert_eq!(
            optimal_piece_length(20 * 1024 * 1024 * 1024),
            16 * 1024 * 1024
        );
    }

    #[test]
    fn create_single_file_torrent_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("sample.txt");
        let mut f = File::create(&file_path).unwrap();
        f.write_all(b"Hello world torrent creation content")
            .unwrap();

        let (torrent, bytes) = create_torrent(CreateTorrentOptions {
            source_path: file_path.clone(),
            piece_length: Some(32768),
            trackers: vec!["udp://tracker.example.com:80/announce".into()],
            is_private: true,
            comment: Some("Test comment".into()),
            source: Some("MYTRACKER".into()),
            created_by: Some("rstorrent-test".into()),
        })
        .unwrap();

        assert_eq!(torrent.name, "sample.txt");
        assert_eq!(torrent.length, 36);
        assert!(torrent.is_private());

        // Verify roundtrip parsing from bytes
        let meta = read_metadata_bytes(&bytes).unwrap();
        assert_eq!(meta.name, "sample.txt");
        assert_eq!(meta.size, 36);
        assert!(meta.is_private);
        assert_eq!(meta.trackers, vec!["udp://tracker.example.com:80/announce"]);
        assert_eq!(meta.files.len(), 1);
        assert_eq!(meta.files[0].path, "sample.txt");
    }

    #[test]
    fn create_directory_torrent_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("MyFolder");
        let sub = folder.join("sub");
        std::fs::create_dir_all(&sub).unwrap();

        File::create(folder.join("file1.bin"))
            .unwrap()
            .write_all(&[1; 100])
            .unwrap();
        File::create(sub.join("file2.bin"))
            .unwrap()
            .write_all(&[2; 200])
            .unwrap();

        let (torrent, bytes) = create_torrent(CreateTorrentOptions {
            source_path: folder.clone(),
            piece_length: None, // Auto
            trackers: vec![
                "http://tracker1.com/announce".into(),
                "".into(), // tier separator
                "http://tracker2.com/announce".into(),
            ],
            is_private: false,
            comment: None,
            source: None,
            created_by: None,
        })
        .unwrap();

        assert_eq!(torrent.name, "MyFolder");
        assert_eq!(torrent.length, 300);
        assert!(!torrent.is_private());

        let meta = read_metadata_bytes(&bytes).unwrap();
        assert_eq!(meta.name, "MyFolder");
        assert_eq!(meta.size, 300);
        assert_eq!(meta.files.len(), 2);
    }
}
