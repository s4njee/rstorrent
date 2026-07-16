//! Persisted "since install" transfer counters.
//!
//! rtorrent's `throttle.global_*.total` counts only the current daemon session
//! and resets to 0 on restart. To show all-time totals we accumulate the deltas
//! into a small JSON file: each read adds `(current − last_seen)` to the running
//! total, treating `current < last_seen` as a session reset (add `current`).

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
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(path: &Path, c: &Counters) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(c) {
        let _ = std::fs::write(path, text);
    }
}

/// Fold the current session totals into the persisted all-time totals and
/// return the updated `(all_time_down, all_time_up)`.
pub fn accumulate(path: &Path, session_down: i64, session_up: i64) -> (i64, i64) {
    let mut c = load(path);
    // A drop below the last-seen value means the daemon restarted (counter reset).
    let delta = |current: i64, last: i64| if current >= last { current - last } else { current };
    c.all_time_down += delta(session_down, c.last_session_down);
    c.all_time_up += delta(session_up, c.last_session_up);
    c.last_session_down = session_down;
    c.last_session_up = session_up;
    save(path, &c);
    (c.all_time_down, c.all_time_up)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_across_sessions_and_resets() {
        let dir = std::env::temp_dir().join(format!("rstorrent-stats-{}", std::process::id()));
        let path = dir.join("stats.json");
        let _ = std::fs::remove_file(&path);

        // First read of a session: full current counts as the delta.
        assert_eq!(accumulate(&path, 1000, 500), (1000, 500));
        // Same session grows: only the delta is added.
        assert_eq!(accumulate(&path, 1500, 700), (1500, 700));
        // Daemon restart (counter dropped): the smaller value is added on top.
        assert_eq!(accumulate(&path, 200, 100), (1700, 800));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
