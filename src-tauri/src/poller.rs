//! Background polling: the engine that keeps the UI live.
//!
//! Two tokio tasks are spawned at startup:
//!   * The **fast loop** (`poll_ms`, ~1s) fetches the torrent list + globals,
//!     resolves any not-yet-known tracker hosts (the "slow poll", cached per
//!     hash), assembles a [`Snapshot`], and emits `state://snapshot`. On failure
//!     it reports a disconnected state and backs off (1→2→5→10s).
//!   * The **detail loop** (~2s) fetches only the selected torrent's active tab
//!     data and emits `state://detail`, and only while a tab is being watched.
//!
//! A user action calls `state.repoll.notify_one()` to trigger an immediate extra
//! fast poll so the UI reflects the change without waiting a full interval.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::ipc::{
    ConnPhase, ConnState, DetailPayload, DetailTab, GlobalStats, LogLevel, Snapshot, TorrentDto,
};
use crate::notifications::{self, CompletionTracker};
use crate::rtorrent::{derive, RawGlobal};
use crate::settings;
use crate::state::AppState;

/// Backoff schedule (seconds) applied after consecutive fast-poll failures.
const BACKOFF: [u64; 4] = [1, 2, 5, 10];
/// Max new tracker hosts resolved per fast poll, to avoid a burst on first load.
const TRACKERS_PER_TICK: usize = 5;

/// Spawn the fast and detail polling loops.
///
/// We use Tauri's async runtime (`tauri::async_runtime::spawn`) rather than
/// `tokio::spawn`: the `setup` hook that calls this does not itself run inside a
/// Tokio runtime, so a bare `tokio::spawn` would panic with "no reactor
/// running". Tauri's runtime is Tokio-backed with I/O + timers enabled, so the
/// SCGI sockets and `tokio::time`/`Notify` primitives inside the loops work.
pub fn spawn(app: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(fast_loop(app.clone(), state.clone()));
    tauri::async_runtime::spawn(detail_loop(app, state));
}

/// The main ~1s poll: list + globals + tracker resolution + snapshot emit.
async fn fast_loop(app: AppHandle, state: Arc<AppState>) {
    let mut failures: usize = 0;
    let mut completion_tracker = CompletionTracker::default();

    loop {
        let backend = state.backend();
        let poll_ms = state.settings().poll_ms.max(250);

        // Fetch the list and globals; either error trips the disconnected path.
        let result = async {
            let torrents = backend.list_snapshot().await?;
            let globals = backend.global_stats().await?;
            Ok::<_, crate::rtorrent::RtorrentError>((torrents, globals))
        }
        .await;

        match result {
            Ok((raw, globals)) => {
                let continuing_session =
                    failures == 0 && state.conn().phase == ConnPhase::Connected;
                if failures > 0 || state.conn().phase != ConnPhase::Connected {
                    // (Re)connected: learn the version and log the transition.
                    let version = backend.client_version().await.ok();
                    let s = state.settings();
                    state.set_conn(ConnState {
                        phase: ConnPhase::Connected,
                        endpoint: settings::endpoint_label(&s.transport),
                        daemon_version: version,
                        error: None,
                        retry_in_seconds: None,
                    });
                    state.log(&app, LogLevel::Info, "connected to rtorrent", None);
                }
                failures = 0;

                if !continuing_session {
                    completion_tracker.reset();
                }

                let settings = state.settings();
                let completed = completion_tracker
                    .observe(&raw, &settings.completion_notification_excluded_labels);
                notifications::set_dock_badge(&app, notifications::active_download_count(&raw));
                for completion in completed {
                    notifications::post_completion(app.clone(), completion);
                }

                resolve_trackers(&app, &state, &raw).await;
                let snapshot = build_snapshot(&state, raw, globals);
                let _ = app.emit("state://snapshot", &snapshot);
            }
            Err(e) => {
                failures += 1;
                completion_tracker.reset();
                notifications::set_dock_badge(&app, 0);
                let delay = BACKOFF[(failures - 1).min(BACKOFF.len() - 1)];
                let s = state.settings();
                // Only log the first failure of a streak to avoid log spam.
                if failures == 1 {
                    state.log(&app, LogLevel::Error, format!("rtorrent unreachable: {e}"), None);
                }
                let conn = ConnState {
                    phase: ConnPhase::Disconnected,
                    endpoint: settings::endpoint_label(&s.transport),
                    daemon_version: None,
                    error: Some(e.to_string()),
                    retry_in_seconds: Some(delay as i64),
                };
                state.set_conn(conn.clone());
                // Emit an empty snapshot so the UI can render the disconnected card.
                let _ = app.emit(
                    "state://snapshot",
                    &Snapshot {
                        torrents: vec![],
                        globals: empty_globals(),
                        connection: conn,
                    },
                );
                wait(delay * 1000, &state).await;
                continue;
            }
        }

        wait(poll_ms, &state).await;
    }
}

/// Sleep for `ms`, waking early if an immediate re-poll is requested.
async fn wait(ms: u64, state: &AppState) {
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_millis(ms)) => {}
        _ = state.repoll.notified() => {}
    }
}

/// Resolve tracker hosts for hashes we haven't seen yet (bounded per tick).
async fn resolve_trackers(_app: &AppHandle, state: &Arc<AppState>, raw: &[crate::rtorrent::RawTorrent]) {
    let unknown: Vec<String> = {
        let cache = state.tracker_cache.lock().unwrap();
        raw.iter()
            .map(|t| t.hash.clone())
            .filter(|h| !cache.contains_key(h))
            .take(TRACKERS_PER_TICK)
            .collect()
    };
    if unknown.is_empty() {
        return;
    }
    let backend = state.backend();
    for hash in unknown {
        if let Ok(host) = backend.primary_tracker(&hash).await {
            state.tracker_cache.lock().unwrap().insert(hash, host);
        }
    }
}

/// Turn raw torrents + globals into the DTO snapshot for the frontend.
fn build_snapshot(state: &AppState, raw: Vec<crate::rtorrent::RawTorrent>, g: RawGlobal) -> Snapshot {
    let torrents: Vec<TorrentDto> = raw
        .iter()
        .map(|t| derive::to_dto(t, &state.tracker_host(&t.hash)))
        .collect();

    let s = state.settings();
    // Free space is only meaningful for a local daemon; a real statvfs is a
    // follow-up, so we surface the mock's fixed value and otherwise None.
    let free_space = if s.mock {
        Some(412 * 1_073_741_824_i64)
    } else {
        None
    };

    Snapshot {
        globals: GlobalStats {
            down_rate: g.down_rate,
            up_rate: g.up_rate,
            down_rate_limit: g.down_rate_limit,
            up_rate_limit: g.up_rate_limit,
            dht_nodes: g.dht_nodes,
            free_space,
        },
        connection: state.conn(),
        torrents,
    }
}

fn empty_globals() -> GlobalStats {
    GlobalStats {
        down_rate: 0,
        up_rate: 0,
        down_rate_limit: 0,
        up_rate_limit: 0,
        dht_nodes: 0,
        free_space: None,
    }
}

/// The ~2s detail poll for the watched torrent/tab.
async fn detail_loop(app: AppHandle, state: Arc<AppState>) {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;

        let watch = state.detail_watch.lock().unwrap().clone();
        let Some((hash, tab)) = watch else { continue };

        let backend = state.backend();
        // Only the data-bearing tabs need a fetch; general/speed/log are derived
        // on the frontend from the snapshot / log stream.
        let payload = match tab {
            DetailTab::Trackers => backend.trackers(&hash).await.ok().map(|rows| DetailPayload {
                hash: hash.clone(),
                tab,
                trackers: Some(rows),
                peers: None,
                files: None,
            }),
            DetailTab::Peers => backend.peers(&hash).await.ok().map(|rows| DetailPayload {
                hash: hash.clone(),
                tab,
                trackers: None,
                peers: Some(rows),
                files: None,
            }),
            DetailTab::Content => backend.files(&hash).await.ok().map(|rows| DetailPayload {
                hash: hash.clone(),
                tab,
                trackers: None,
                peers: None,
                files: Some(rows),
            }),
            DetailTab::General | DetailTab::Speed | DetailTab::Log => None,
        };

        if let Some(p) = payload {
            let _ = app.emit("state://detail", &p);
        }
    }
}
