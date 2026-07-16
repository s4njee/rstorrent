//! macOS file-association and deep-link open request handoff.
//!
//! LaunchServices may deliver `RunEvent::Opened` before the webview has loaded.
//! Requests are therefore retained until the frontend subscribes and invokes
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

/// Accept URLs from Tauri's macOS `RunEvent::Opened` callback.
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
    fn ignores_empty_batches() {
        let state = OpenRequestState::default();
        assert_eq!(state.receive(Vec::new()), None);
        assert!(state.take_initial().is_empty());
    }
}
