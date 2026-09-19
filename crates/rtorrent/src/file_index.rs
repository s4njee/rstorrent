//! A bounded, lazily built index of torrent file paths, for library search
//! (V3-12 / LIB-03).
//!
//! `f.multicall` is the only way to ask rtorrent for a torrent's files, so
//! indexing the whole library means one call per torrent. This keeps that cost
//! bounded and lazy: the host feeds [`FileIndex::needs`] a live hash list and
//! records only a few torrents per poll, and the index caps both how many
//! torrents and how many paths it holds. A query that lands on an unindexed
//! torrent simply does not match on filename yet — it never blocks a search.
//!
//! Pure and host-agnostic: the core stays UI-free, the web server and the GPUI
//! shell each own one instance per connection.

use std::collections::{HashMap, HashSet, VecDeque};

/// How many torrents the index holds before the oldest is evicted.
pub const DEFAULT_CAPACITY: usize = 5000;
/// A single torrent's path list is truncated to this many entries, so one
/// 100k-file torrent cannot crowd out the rest of the library.
pub const MAX_PATHS_PER_TORRENT: usize = 2000;

/// A bounded map of torrent hash → lowercased file paths.
#[derive(Debug)]
pub struct FileIndex {
    capacity: usize,
    /// Uppercase hash → lowercased paths.
    files: HashMap<String, Vec<String>>,
    /// Insertion order (oldest first), for eviction.
    order: VecDeque<String>,
}

impl Default for FileIndex {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl FileIndex {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            files: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// How many torrents are indexed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// True once a torrent's files have been recorded.
    #[must_use]
    pub fn is_indexed(&self, hash: &str) -> bool {
        self.files.contains_key(&normalise_hash(hash))
    }

    /// True when the indexed file list for `hash` contains `query` (already
    /// lowercased). An unindexed torrent is never a match.
    #[must_use]
    pub fn contains(&self, hash: &str, query_lower: &str) -> bool {
        if query_lower.is_empty() {
            return true;
        }
        self.files
            .get(&normalise_hash(hash))
            .is_some_and(|paths| paths.iter().any(|path| path.contains(query_lower)))
    }

    /// Record a torrent's file paths, replacing any previous entry. Paths are
    /// lowercased once so a query only lowercases itself; the list is truncated.
    pub fn record<I, S>(&mut self, hash: &str, paths: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let hash = normalise_hash(hash);
        let lowered: Vec<String> = paths
            .into_iter()
            .take(MAX_PATHS_PER_TORRENT)
            .map(|path| path.as_ref().to_lowercase())
            .collect();
        if self.files.insert(hash.clone(), lowered).is_none() {
            self.order.push_back(hash);
        }
        self.evict();
    }

    /// Drop everything not in `live`, so a removed torrent leaves no paths.
    pub fn retain_live(&mut self, live: &HashSet<String>) {
        self.files
            .retain(|hash, _| live.contains(hash) || live.contains(&hash.to_uppercase()));
        self.order.retain(|hash| self.files.contains_key(hash));
    }

    /// Forget everything — a new connection must not answer from the old daemon.
    pub fn clear(&mut self) {
        self.files.clear();
        self.order.clear();
    }

    /// Up to `batch` live torrents that are not indexed yet, in the order given.
    #[must_use]
    pub fn needs<'a>(&self, hashes: &'a [String], batch: usize) -> Vec<&'a str> {
        hashes
            .iter()
            .filter(|hash| !self.is_indexed(hash))
            .take(batch)
            .map(String::as_str)
            .collect()
    }

    fn evict(&mut self) {
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.files.remove(&oldest);
            }
        }
    }
}

fn normalise_hash(hash: &str) -> String {
    hash.to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(hashes: &[&str]) -> HashSet<String> {
        hashes.iter().map(|h| (*h).to_owned()).collect()
    }

    #[test]
    fn contains_is_case_insensitive_and_unknown_is_false() {
        let mut index = FileIndex::default();
        index.record("abc", ["Folder/S02E04.mkv", "readme.txt"]);
        assert!(index.contains("abc", "s02e04"));
        assert!(index.contains("ABC", "folder"));
        assert!(!index.contains("abc", "s02e05"));
        // Not indexed yet: no filename match, but it must not panic or block.
        assert!(!index.contains("other", "s02e04"));
        assert!(index.contains("abc", ""));
    }

    #[test]
    fn record_replaces_and_truncates() {
        let mut index = FileIndex::default();
        index.record("A", ["one"]);
        index.record("A", ["two"]);
        assert!(index.contains("A", "two"));
        assert!(!index.contains("A", "one"));

        let many: Vec<String> = (0..(MAX_PATHS_PER_TORRENT + 10))
            .map(|i| format!("f{i}"))
            .collect();
        let mut big = FileIndex::default();
        big.record("B", many);
        assert_eq!(
            big.len(),
            1,
            "only the torrent count is exposed; the paths are capped"
        );
        assert!(big.contains("B", "f0"));
        assert!(!big.contains("B", &format!("f{}", MAX_PATHS_PER_TORRENT + 5)));
    }

    #[test]
    fn capacity_evicts_the_oldest() {
        let mut index = FileIndex::new(2);
        index.record("A", ["a"]);
        index.record("B", ["b"]);
        index.record("C", ["c"]);
        assert!(!index.is_indexed("A"), "the oldest is evicted");
        assert!(index.is_indexed("B") && index.is_indexed("C"));
    }

    #[test]
    fn retain_live_drops_removed_torrents() {
        let mut index = FileIndex::default();
        index.record("A", ["a"]);
        index.record("B", ["b"]);
        index.retain_live(&live(&["A"]));
        assert!(index.is_indexed("A"));
        assert!(!index.is_indexed("B"));
    }

    #[test]
    fn clear_forgets_everything() {
        let mut index = FileIndex::default();
        index.record("A", ["a"]);
        index.clear();
        assert!(index.is_empty());
        assert!(!index.contains("A", "a"));
    }

    #[test]
    fn needs_skips_indexed_and_caps_the_batch() {
        let mut index = FileIndex::default();
        index.record("B", ["b"]);
        let hashes = vec!["A".to_owned(), "B".to_owned(), "C".to_owned()];
        assert_eq!(index.needs(&hashes, 5), vec!["A", "C"]);
        assert_eq!(index.needs(&hashes, 1), vec!["A"]);
    }
}
