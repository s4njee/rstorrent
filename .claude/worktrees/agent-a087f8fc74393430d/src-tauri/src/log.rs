//! App event log — a bounded ring buffer feeding the Log detail tab.
//!
//! Records connection changes, action results, RPC faults, and per-torrent
//! error-message transitions. The buffer is capped so a long-running session
//! can't grow unbounded; the newest `CAP` entries are kept. Callers push through
//! [`AppState`](crate::state::AppState), which also emits a `log://append` event
//! so the frontend can append live.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ipc::{LogEntry, LogLevel};

/// Maximum retained log entries.
const CAP: usize = 1000;

/// Thread-safe bounded log.
#[derive(Default)]
pub struct LogBuffer {
    entries: Mutex<VecDeque<LogEntry>>,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an entry (evicting the oldest past `CAP`) and return a clone for
    /// the caller to emit to the frontend.
    pub fn push(&self, level: LogLevel, message: impl Into<String>, hash: Option<String>) -> LogEntry {
        let entry = LogEntry {
            time: now_millis(),
            level,
            message: message.into(),
            hash,
        };
        let mut q = self.entries.lock().unwrap();
        if q.len() >= CAP {
            q.pop_front();
        }
        q.push_back(entry.clone());
        entry
    }

    /// Current contents oldest→newest (used to hydrate the Log tab on open).
    pub fn snapshot(&self) -> Vec<LogEntry> {
        self.entries.lock().unwrap().iter().cloned().collect()
    }
}

/// Milliseconds since the Unix epoch.
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_at_limit() {
        let log = LogBuffer::new();
        for i in 0..(CAP + 50) {
            log.push(LogLevel::Info, format!("m{i}"), None);
        }
        let snap = log.snapshot();
        assert_eq!(snap.len(), CAP);
        // Oldest entries evicted; newest retained.
        assert_eq!(snap.last().unwrap().message, format!("m{}", CAP + 49));
    }
}
