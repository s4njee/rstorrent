//! Duplicate detection before add (V3-13 / LIB-04).
//!
//! Adding a torrent the daemon already has does nothing useful (rtorrent rejects
//! the hash), and adding one whose data folder is another torrent's folder
//! overwrites or fragments it. This is the pure check the add flows run before
//! they submit, so nothing is ever added *silently*.
//!
//! The browser cannot call this crate, so `src/utils/duplicates.ts` mirrors the
//! same rules; the tests on both sides pin the shared behaviour.

use crate::types::TorrentDto;

/// What is being added, as far as it is known before the daemon resolves it. A
/// magnet has a hash but no name/size yet; an inspected `.torrent` has all of
/// them.
#[derive(Clone, Copy, Debug, Default)]
pub struct AddCandidate<'a> {
    pub hash: Option<&'a str>,
    pub name: Option<&'a str>,
    pub size: Option<i64>,
    /// The chosen save directory (the torrent's own folder is `name` under it).
    pub destination: Option<&'a str>,
}

/// The strongest reason not to add silently, in the order they are checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Duplicate {
    /// The info-hash is already loaded: focus it and offer to merge trackers.
    Exact { hash: String, name: String },
    /// A different torrent with the same name and size: warn.
    SameNameAndSize {
        hash: String,
        name: String,
        size: i64,
    },
    /// Another torrent's data folder is exactly where this one would land: warn.
    DestinationInUse {
        hash: String,
        name: String,
        path: String,
    },
}

/// The strongest duplicate finding, or `None` when the add is clean.
#[must_use]
pub fn detect(candidate: &AddCandidate<'_>, library: &[TorrentDto]) -> Option<Duplicate> {
    if let Some(hash) = candidate.hash.filter(|hash| !hash.trim().is_empty()) {
        if let Some(existing) = library
            .iter()
            .find(|torrent| torrent.hash.eq_ignore_ascii_case(hash.trim()))
        {
            return Some(Duplicate::Exact {
                hash: existing.hash.clone(),
                name: existing.name.clone(),
            });
        }
    }

    if let (Some(name), Some(size)) = (candidate.name, candidate.size) {
        let name = name.trim();
        if !name.is_empty() && size > 0 {
            if let Some(existing) = library
                .iter()
                .find(|torrent| torrent.size == size && torrent.name.eq_ignore_ascii_case(name))
            {
                return Some(Duplicate::SameNameAndSize {
                    hash: existing.hash.clone(),
                    name: existing.name.clone(),
                    size,
                });
            }
        }
    }

    if let (Some(destination), Some(name)) = (candidate.destination, candidate.name) {
        let name = name.trim();
        if !destination.trim().is_empty() && !name.is_empty() {
            let target = normalise_path(&format!("{}/{}", destination.trim_end_matches('/'), name));
            if let Some(existing) = library
                .iter()
                .find(|torrent| normalise_path(&torrent.save_path) == target)
            {
                return Some(Duplicate::DestinationInUse {
                    hash: existing.hash.clone(),
                    name: existing.name.clone(),
                    path: existing.save_path.clone(),
                });
            }
        }
    }

    None
}

/// Trailing slashes and case do not distinguish a path; the filesystem on the
/// platforms this ships on does not either.
fn normalise_path(path: &str) -> String {
    path.trim().trim_end_matches('/').to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn torrent(hash: &str, name: &str, size: i64, save_path: &str) -> TorrentDto {
        TorrentDto {
            hash: hash.into(),
            name: name.into(),
            size,
            bytes_done: 0,
            percent: 0.0,
            status: crate::types::Status::Paused,
            status_msg: String::new(),
            error_kind: String::new(),
            seeds_connected: 0,
            peers_connected: 0,
            seeds_swarm: 0,
            peers_swarm: 0,
            down_rate: 0,
            up_rate: 0,
            eta_seconds: None,
            ratio: 0.0,
            label: String::new(),
            tags: Vec::new(),
            force_start: false,
            throttle_rule: String::new(),
            tracker_host: String::new(),
            save_path: save_path.into(),
            priority: 0,
            is_private: false,
            throttle_name: String::new(),
            down_rate_limit: None,
            up_rate_limit: None,
            started_at: 0,
            finished_at: 0,
            peers_max: 0,
            peers_min: 0,
            uploads_max: 0,
            connection_type: String::new(),
            added_by: String::new(),
            source_path: String::new(),
            added_at: 0,
            views: Vec::new(),
            is_open: false,
            is_active: false,
        }
    }

    fn library() -> Vec<TorrentDto> {
        vec![
            torrent("AAAA", "Show.S01", 1000, "/data/Show.S01"),
            torrent("BBBB", "Movie.mkv", 2000, "/data/Movie.mkv"),
        ]
    }

    #[test]
    fn an_exact_hash_wins_and_is_case_insensitive() {
        let candidate = AddCandidate {
            hash: Some("aaaa"),
            name: Some("Something else"),
            size: Some(9),
            destination: Some("/data"),
        };
        assert_eq!(
            detect(&candidate, &library()),
            Some(Duplicate::Exact {
                hash: "AAAA".into(),
                name: "Show.S01".into()
            })
        );
    }

    #[test]
    fn same_name_and_size_is_a_warning() {
        let candidate = AddCandidate {
            hash: Some("CCCC"),
            name: Some("show.s01"),
            size: Some(1000),
            destination: None,
        };
        assert_eq!(
            detect(&candidate, &library()),
            Some(Duplicate::SameNameAndSize {
                hash: "AAAA".into(),
                name: "Show.S01".into(),
                size: 1000
            })
        );
    }

    #[test]
    fn a_name_match_without_a_size_match_is_clean() {
        let candidate = AddCandidate {
            hash: Some("CCCC"),
            name: Some("Show.S01"),
            size: Some(7),
            destination: None,
        };
        assert_eq!(detect(&candidate, &library()), None);
    }

    #[test]
    fn a_destination_that_is_another_torrents_folder_warns() {
        let candidate = AddCandidate {
            hash: Some("CCCC"),
            name: Some("Show.S01"),
            size: Some(7),
            destination: Some("/data/"),
        };
        assert_eq!(
            detect(&candidate, &library()),
            Some(Duplicate::DestinationInUse {
                hash: "AAAA".into(),
                name: "Show.S01".into(),
                path: "/data/Show.S01".into()
            })
        );
        // A different folder under the same parent is fine.
        let clean = AddCandidate {
            destination: Some("/data/other"),
            ..candidate
        };
        assert_eq!(detect(&clean, &library()), None);
    }

    #[test]
    fn unknown_fields_never_match() {
        let empty = AddCandidate::default();
        assert_eq!(detect(&empty, &library()), None);
        let nameless = AddCandidate {
            destination: Some("/data"),
            ..AddCandidate::default()
        };
        assert_eq!(detect(&nameless, &library()), None);
        let zero_size = AddCandidate {
            hash: Some("CCCC"),
            name: Some("Show.S01"),
            size: Some(0),
            destination: None,
        };
        assert_eq!(detect(&zero_size, &library()), None);
    }
}
