//! Persisted "since install" transfer counters.
//!
//! rtorrent's `throttle.global_*.total` counts only the current daemon session
//! and resets to 0 on restart. To show all-time totals the shell accumulates
//! the deltas into a small JSON file next to `settings.json`: each read adds
//! `(current − last_seen)` to the running total, treating `current < last_seen`
//! as a session reset (add `current`).
//!
//! A port of `src-tauri/src/stats.rs`, sharing its file (`stats.json`) and its
//! shape, so the two shells show the same all-time figures and neither
//! double-counts the other's reads.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct Counters {
    all_time_down: i64,
    all_time_up: i64,
    /// Last session totals we observed, to compute the next delta.
    last_session_down: i64,
    last_session_up: i64,
}

fn load(path: &Path) -> Counters {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(path: &Path, counters: &Counters) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(counters) {
        let _ = std::fs::write(path, text);
    }
}

/// The delta to add for one counter: its growth since `last`, or the whole
/// value when it dropped (a daemon restart reset the session counter).
#[must_use]
pub fn delta(current: i64, last: i64) -> i64 {
    if current >= last {
        current - last
    } else {
        current
    }
}

/// Fold the current session totals into the persisted all-time totals and
/// return the updated `(all_time_down, all_time_up)`.
pub fn accumulate(path: &Path, session_down: i64, session_up: i64) -> (i64, i64) {
    let mut counters = load(path);
    counters.all_time_down += delta(session_down, counters.last_session_down);
    counters.all_time_up += delta(session_up, counters.last_session_up);
    counters.last_session_down = session_down;
    counters.last_session_up = session_up;
    save(path, &counters);
    (counters.all_time_down, counters.all_time_up)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A unique path per test run, so parallel tests cannot share counters.
    fn scratch(name: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir()
            .join(format!(
                "rstorrent-gpui-stats-{}-{stamp}",
                std::process::id()
            ))
            .join(name)
    }

    #[test]
    fn first_read_counts_the_whole_session() {
        assert_eq!(delta(1000, 0), 1000);
    }

    #[test]
    fn a_growing_session_only_adds_the_delta() {
        let path = scratch("stats.json");
        assert_eq!(accumulate(&path, 1000, 500), (1000, 500));
        assert_eq!(accumulate(&path, 1500, 700), (1500, 700));
        // A daemon restart (counter dropped) adds the smaller value on top.
        assert_eq!(accumulate(&path, 200, 100), (1700, 800));
        // And continues from there.
        assert_eq!(accumulate(&path, 250, 100), (1750, 800));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_corrupt_file_falls_back_to_zero_rather_than_failing() {
        let path = scratch("corrupt.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(accumulate(&path, 42, 7), (42, 7));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
