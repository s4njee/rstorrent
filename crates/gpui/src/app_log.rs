//! The app's own log: what the app did, for the Log pane.
//!
//! A port of `src-tauri/src/log.rs`. rtorrent keeps its own log; this is the
//! other half — the actions this app took, the failures it saw and the
//! reconnects it made, which is what you need when a torrent did not do what you
//! asked. Held in memory and bounded: the log is for the session in front of
//! you, not an audit trail.

use std::collections::VecDeque;

use rtorrent_core::types::{LogEntry, LogLevel};

/// How many entries are kept. The oldest fall off the front.
pub const CAP: usize = 1000;

/// A bounded ring of the app's own entries.
#[derive(Default)]
pub struct AppLog {
    entries: VecDeque<LogEntry>,
}

impl AppLog {
    /// Append an entry, stamped now, and drop the oldest past [`CAP`].
    pub fn push(&mut self, level: LogLevel, message: impl Into<String>, hash: Option<String>) {
        self.entries.push_back(LogEntry {
            time: now_millis(),
            level,
            message: message.into(),
            hash,
        });
        while self.entries.len() > CAP {
            self.entries.pop_front();
        }
    }

    /// The entries, oldest first.
    pub fn entries(&self) -> impl DoubleEndedIterator<Item = &LogEntry> {
        self.entries.iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many entries mention a torrent — the Log pane's header.
    #[must_use]
    pub fn count_for(&self, hash: &str) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.hash.as_deref() == Some(hash))
            .count()
    }
}

/// Milliseconds since the Unix epoch, which is what `LogEntry::time` carries.
fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis()),
    )
    .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(entry: &LogEntry) -> &str {
        entry.message.as_str()
    }

    #[test]
    fn entries_keep_their_order_and_their_stamp() {
        let mut log = AppLog::default();
        log.push(LogLevel::Info, "first", None);
        log.push(LogLevel::Warn, "second", Some("H".to_owned()));

        let entries: Vec<&LogEntry> = log.entries().collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(message(entries[0]), "first");
        assert_eq!(entries[1].level, LogLevel::Warn);
        assert_eq!(entries[1].hash.as_deref(), Some("H"));
        // Stamped as it was written, so the pane can show a time.
        assert!(entries[0].time > 0);
    }

    #[test]
    fn the_ring_is_bounded_and_drops_the_oldest() {
        let mut log = AppLog::default();
        for index in 0..(CAP + 10) {
            log.push(LogLevel::Info, format!("entry {index}"), None);
        }
        assert_eq!(log.len(), CAP);
        let newest = log.entries().next_back().expect("an entry");
        assert_eq!(message(newest), format!("entry {}", CAP + 9));
        let oldest = log.entries().next().expect("an entry");
        assert_eq!(message(oldest), "entry 10");
    }

    #[test]
    fn a_fresh_log_is_empty() {
        let log = AppLog::default();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
        assert_eq!(log.count_for("H"), 0);
    }

    #[test]
    fn entries_are_counted_per_torrent() {
        let mut log = AppLog::default();
        log.push(LogLevel::Info, "one", Some("A".to_owned()));
        log.push(LogLevel::Error, "two", Some("A".to_owned()));
        log.push(LogLevel::Info, "three", Some("B".to_owned()));
        log.push(LogLevel::Info, "app-wide", None);

        assert_eq!(log.count_for("A"), 2);
        assert_eq!(log.count_for("B"), 1);
        assert_eq!(log.count_for("C"), 0);
    }
}
