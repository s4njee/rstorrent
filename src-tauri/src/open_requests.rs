//! File-association and deep-link open request handoff.
//!
//! The two platforms deliver these differently. macOS raises
//! `RunEvent::Opened` on the running app; Windows launches a *second* process
//! with the file or magnet in `argv`, which the single-instance plugin forwards
//! to the live one. Both funnel into [`receive`].
//!
//! Either delivery may arrive before the webview has loaded. Requests are
//! therefore retained until the frontend subscribes and invokes
//! `take_open_requests`; later requests are emitted immediately.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

pub const OPEN_REQUEST_EVENT: &str = "app://open-requests";

#[derive(Default)]
struct Pending {
    frontend_ready: bool,
    urls: Vec<String>,
}

#[derive(Default)]
pub struct OpenRequestState(Mutex<Pending>);

impl OpenRequestState {
    /// Mark the frontend ready and return every request received during startup.
    pub fn take_initial(&self) -> Vec<String> {
        let mut pending = self.0.lock().unwrap_or_else(|err| err.into_inner());
        pending.frontend_ready = true;
        std::mem::take(&mut pending.urls)
    }

    /// Queue startup requests, or return warm requests for immediate emission.
    fn receive(&self, urls: Vec<String>) -> Option<Vec<String>> {
        if urls.is_empty() {
            return None;
        }

        let mut pending = self.0.lock().unwrap_or_else(|err| err.into_inner());
        if pending.frontend_ready {
            Some(urls)
        } else {
            pending.urls.extend(urls);
            None
        }
    }
}

/// Pick the openable arguments out of a command line.
///
/// Windows hands the app its document or URL as an ordinary argument, mixed in
/// with `argv[0]` and any switches, so anything that isn't a magnet link or a
/// `.torrent` is dropped rather than forwarded to the frontend as a bogus
/// request.
pub fn from_argv(argv: &[String]) -> Vec<String> {
    argv.iter()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .filter(|arg| {
            let lower = arg.to_ascii_lowercase();
            lower.starts_with("magnet:") || lower.ends_with(".torrent")
        })
        .cloned()
        .collect()
}

/// Accept open requests from either platform's delivery mechanism.
pub fn receive(app: &AppHandle, urls: Vec<String>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }

    let state = app.state::<OpenRequestState>();
    if let Some(urls) = state.receive(urls) {
        let _ = app.emit(OPEN_REQUEST_EVENT, urls);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queues_until_frontend_is_ready_then_forwards_warm_requests() {
        let state = OpenRequestState::default();

        assert_eq!(state.receive(vec!["file:///one.torrent".into()]), None);
        assert_eq!(
            state.receive(vec!["file:///two.torrent".into(), "magnet:?xt=x".into()]),
            None
        );
        assert_eq!(
            state.take_initial(),
            vec!["file:///one.torrent", "file:///two.torrent", "magnet:?xt=x"]
        );
        assert!(state.take_initial().is_empty());
        assert_eq!(
            state.receive(vec!["file:///warm.torrent".into()]),
            Some(vec!["file:///warm.torrent".into()])
        );
    }

    #[test]
    fn argv_keeps_only_openable_arguments() {
        let argv: Vec<String> = [
            r"C:\Program Files\rstorrent\rstorrent.exe",
            "--some-switch",
            r"C:\Users\you\Downloads\Debian.TORRENT",
            "magnet:?xt=urn:btih:abc",
            r"C:\Users\you\notes.txt",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        assert_eq!(
            from_argv(&argv),
            vec![
                r"C:\Users\you\Downloads\Debian.TORRENT",
                "magnet:?xt=urn:btih:abc"
            ]
        );
    }

    #[test]
    fn argv_without_documents_yields_nothing() {
        // A plain launch must not produce a phantom open request.
        assert!(from_argv(&[r"C:\rstorrent.exe".to_string()]).is_empty());
        assert!(from_argv(&[]).is_empty());
    }

    #[test]
    fn ignores_empty_batches() {
        let state = OpenRequestState::default();
        assert_eq!(state.receive(Vec::new()), None);
        assert!(state.take_initial().is_empty());
    }
}
