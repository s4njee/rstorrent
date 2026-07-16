//! Download-completion detection and native macOS notifications.
//!
//! Detection is deliberately session-scoped: the poller resets this tracker
//! whenever it disconnects, so a reconnect's first snapshot only establishes a
//! baseline. A notification is emitted only when the same connected session has
//! observed a torrent as incomplete and later observes it as complete.

use std::collections::HashMap;

use tauri::AppHandle;

use crate::ipc::Status;
use crate::rtorrent::{derive, RawTorrent};

/// The data needed after a completion transition has been detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub hash: String,
    pub name: String,
    pub size_bytes: i64,
}

/// Remembers the last observed completion flag for torrents in one session.
#[derive(Debug, Default)]
pub struct CompletionTracker {
    seen: HashMap<String, bool>,
}

impl CompletionTracker {
    /// Observe one successful poll and return eligible incomplete→complete
    /// transitions. Torrents complete when first seen are only baselined.
    pub fn observe(
        &mut self,
        torrents: &[RawTorrent],
        excluded_labels: &[String],
    ) -> Vec<Completion> {
        let mut next = HashMap::with_capacity(torrents.len());
        let mut completed = Vec::new();

        for torrent in torrents {
            let transitioned = self.seen.get(&torrent.hash) == Some(&false) && torrent.complete;
            let excluded = excluded_labels.iter().any(|label| label == &torrent.label);
            if transitioned && !excluded {
                completed.push(Completion {
                    hash: torrent.hash.clone(),
                    name: torrent.name.clone(),
                    size_bytes: torrent.size_bytes,
                });
            }
            next.insert(torrent.hash.clone(), torrent.complete);
        }

        // Dropping absent hashes ensures a removed/re-added complete torrent is
        // treated as first-seen instead of producing a stale transition.
        self.seen = next;
        completed
    }

    /// Start a fresh connected session. Its next snapshot is only a baseline.
    pub fn reset(&mut self) {
        self.seen.clear();
    }
}

/// Count rows that are currently and healthily transferring download data.
pub fn active_download_count(torrents: &[RawTorrent]) -> i64 {
    torrents
        .iter()
        .filter(|torrent| derive::status(torrent) == Status::Downloading)
        .count() as i64
}

/// Keep notification sizes consistent with the compact table formatter.
pub fn format_bytes(bytes: i64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];

    if bytes <= 0 {
        return "0 B".to_string();
    }

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 || value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Set or clear the app-wide Dock badge through Tauri's window API.
pub fn set_dock_badge(app: &AppHandle, count: i64) {
    use tauri::Manager;

    if let Some(window) = app.get_webview_window("main") {
        let badge = (count > 0).then_some(count);
        let _ = window.set_badge_count(badge);
    }
}

/// Post a completion notification and wait for a direct click off the poller
/// thread. On click, focus the window and tell React which row to reveal.
#[cfg(target_os = "macos")]
pub fn post_completion(app: AppHandle, completion: Completion) {
    let _ = std::thread::Builder::new()
        .name("completion-notification".to_string())
        .spawn(move || {
            use mac_notification_sys::{Notification, NotificationResponse};
            use tauri::{Emitter, Manager};

            let body = format!(
                "Download complete · {}",
                format_bytes(completion.size_bytes)
            );
            let mut notification = Notification::new();
            notification
                .title(&completion.name)
                .message(&body)
                .default_sound()
                .wait_for_click(true);

            if matches!(notification.send(), Ok(NotificationResponse::Click)) {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
                let _ = app.emit("torrent://notification-clicked", completion.hash);
            }
        });
}

/// Other desktop targets keep transition and badge behavior but do not post a
/// macOS notification.
#[cfg(not(target_os = "macos"))]
pub fn post_completion(_app: AppHandle, _completion: Completion) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn torrent(hash: &str, complete: bool, label: &str) -> RawTorrent {
        RawTorrent {
            hash: hash.to_string(),
            name: format!("torrent-{hash}"),
            size_bytes: five_point_eight_gib(),
            bytes_done: if complete { five_point_eight_gib() } else { 1 },
            complete,
            is_active: true,
            is_open: true,
            down_rate: if complete { 0 } else { 100 },
            label: label.to_string(),
            ..Default::default()
        }
    }

    const fn five_point_eight_gib() -> i64 {
        29 * 1024 * 1024 * 1024 / 5
    }

    #[test]
    fn only_reports_observed_incomplete_to_complete_transitions() {
        let mut tracker = CompletionTracker::default();

        assert!(tracker
            .observe(&[torrent("A", true, "linux")], &[])
            .is_empty());
        assert!(tracker
            .observe(
                &[torrent("A", true, "linux"), torrent("B", false, "video")],
                &[]
            )
            .is_empty());

        let events = tracker.observe(
            &[torrent("A", true, "linux"), torrent("B", true, "video")],
            &[],
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].hash, "B");

        assert!(tracker
            .observe(
                &[torrent("A", true, "linux"), torrent("B", true, "video")],
                &[]
            )
            .is_empty());
    }

    #[test]
    fn exclusion_is_applied_at_transition_time() {
        let mut tracker = CompletionTracker::default();
        tracker.observe(&[torrent("A", false, "private")], &[]);

        let excluded = vec!["private".to_string()];
        assert!(tracker
            .observe(&[torrent("A", true, "private")], &excluded)
            .is_empty());
    }

    #[test]
    fn reset_and_readded_torrents_establish_new_baselines() {
        let mut tracker = CompletionTracker::default();
        tracker.observe(&[torrent("A", false, "")], &[]);
        tracker.reset();
        assert!(tracker.observe(&[torrent("A", true, "")], &[]).is_empty());

        tracker.observe(&[], &[]);
        assert!(tracker.observe(&[torrent("A", true, "")], &[]).is_empty());
    }

    #[test]
    fn counts_only_derived_downloading_status() {
        let mut stalled = torrent("B", false, "");
        stalled.down_rate = 0;
        let complete = torrent("C", true, "");

        assert_eq!(
            active_download_count(&[torrent("A", false, ""), stalled, complete]),
            1
        );
    }

    #[test]
    fn formats_compact_binary_sizes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(631 * 1024 * 1024), "631 MiB");
        assert_eq!(format_bytes(five_point_eight_gib()), "5.8 GiB");
    }
}
