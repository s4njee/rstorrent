//! The server's polling loop.
//!
//! One fast loop (`config.poll_ms`, ~1s) fetches the torrent list + globals,
//! resolves any unknown tracker hosts and the disk figures on a slow cadence,
//! assembles a [`Snapshot`] with the shared `rtorrent_core::snapshot` helpers,
//! serializes it once, and stores it in the cache with an ETag. On failure it
//! records a disconnected [`ConnState`] and backs off.
//!
//! **Idle-stop:** when no `/api/state` request has arrived for [`IDLE_AFTER`],
//! the loop parks on `activity`/`repoll` instead of polling, so a seedbox with
//! no browser open costs the daemon nothing. The next request (or a mutation)
//! wakes it for an immediate refresh.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use rtorrent_core::rtorrent::{RawGlobal, RawTorrent, RtorrentError};
use rtorrent_core::snapshot;
use rtorrent_core::types::{ConnPhase, ConnState, LogLevel, Snapshot};

use crate::state::{etag_of, AppState, Cached};

/// Backoff schedule (seconds) after consecutive failures.
const BACKOFF: [u64; 4] = [1, 2, 5, 10];
/// Park the loop after this long without a state request. Shortened under test
/// so the idle-stop behavior can be exercised without a 10s wait.
#[cfg(not(test))]
const IDLE_AFTER: Duration = Duration::from_secs(10);
#[cfg(test)]
const IDLE_AFTER: Duration = Duration::from_millis(150);
/// New tracker hosts resolved per tick (avoids a burst on first load).
const TRACKERS_PER_TICK: usize = 5;
/// Refresh disk figures / tracker sweep every N successful ticks.
const SLOW_EVERY: u64 = 30;
/// Full snapshot every N ticks for reconciliation (FND-02).
const FULL_EVERY: u64 = 30;

/// Run the fast loop forever.
pub async fn run(state: Arc<AppState>) {
    let mut failures: u32 = 0;
    let mut tick: u64 = 0;

    loop {
        // Idle-stop: park until a request or mutation wakes us.
        if state.last_request.lock().unwrap().elapsed() > IDLE_AFTER {
            tokio::select! {
                _ = state.activity.notified() => {}
                _ = state.repoll.notified() => {}
            }
        }

        match poll_once(&state, tick).await {
            Ok(()) => {
                failures = 0;
                tick = tick.wrapping_add(1);
                // Wait the interval, waking early on a mutation.
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(state.config.poll_ms)) => {}
                    _ = state.repoll.notified() => {}
                }
            }
            Err(err) => {
                failures += 1;
                let delay = BACKOFF[(failures as usize - 1).min(BACKOFF.len() - 1)];
                if failures == 1 {
                    state.log(
                        LogLevel::Error,
                        format!("rtorrent unreachable: {err}"),
                        None,
                    );
                }
                set_disconnected(&state, &err, delay);
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(delay)) => {}
                    _ = state.repoll.notified() => {}
                }
            }
        }
    }
}

/// One poll: fetch, assemble, cache. `Err` trips the disconnected path.
async fn poll_once(state: &Arc<AppState>, tick: u64) -> Result<(), RtorrentError> {
    let was_connected = state.conn().phase == ConnPhase::Connected;

    let raw = state.backend.list_snapshot().await?;
    let globals = state.backend.global_stats().await?;
    state.poll_count.fetch_add(1, Ordering::Relaxed);

    if !was_connected {
        let version = state.backend.client_version().await.ok();
        state.set_conn(ConnState {
            phase: ConnPhase::Connected,
            endpoint: crate::endpoint_label(&state.config.transport),
            daemon_version: version,
            error: None,
            retry_in_seconds: None,
        });
        // A new connection indexes from scratch: the old daemon's file lists
        // must not answer a search against this one (V3-12).
        state.file_index.lock().unwrap().clear();
        state.log(LogLevel::Info, "connected to rtorrent", None);
    }

    // Drop paths for torrents that have gone before indexing more.
    {
        let live: std::collections::HashSet<String> =
            raw.iter().map(|torrent| torrent.hash.clone()).collect();
        state.file_index.lock().unwrap().retain_live(&live);
    }

    // Slow work: resolve unknown tracker hosts, index a bounded batch of file
    // lists, and refresh disk figures — on the first tick and every SLOW_EVERY.
    if tick.is_multiple_of(SLOW_EVERY) {
        resolve_trackers(state, &raw).await;
        index_files(state, &raw).await;
        // One rate sample for the Stats history, on the same slow cadence.
        state.record_rate_sample(now_millis(), globals.down_rate, globals.up_rate);
    }

    // Move-on-complete (V3-14): recover the journal once, then plan from
    // this tick's rows. Detached tasks do the moving; the tick never blocks.
    crate::moves::resume_once(state, &raw).await;
    crate::moves::run_due_moves(state, &raw).await;

    // Complete queue policy (V3-17 / QUE-01): new on this host, same driver
    // as the desktops. Holds pause (stay loaded → renders "Queued"), never
    // stop; in-flight moves are skipped so the queue never restarts a
    // torrent a move task just stopped.
    {
        let cfg = rtorrent_core::queue::QueueConfig {
            max_downloads: state.config.max_active_downloads,
            max_uploads: state.config.max_active_uploads,
            max_total: state.config.max_active_torrents,
            slow_limit_kbs: state.config.queue_slow_limit_kbs,
        };
        // Short lock: the guard dies with this scope, before any await.
        let actions = {
            let skip = state.moves.lock().unwrap().busy_hashes();
            let mut mem = state.queue_mem.lock().unwrap();
            rtorrent_core::queue::decide_queues(&raw, cfg, &mut mem, &skip)
        };
        if !actions.stop.is_empty() && state.backend.pause(&actions.stop).await.is_ok() {
            state.log(
                LogLevel::Info,
                format!(
                    "queued {} torrent(s) over the active limit",
                    actions.stop.len()
                ),
                None,
            );
        }
        if !actions.start.is_empty() && state.backend.start(&actions.start).await.is_ok() {
            state.log(
                LogLevel::Info,
                format!("started {} queued torrent(s)", actions.start.len()),
                None,
            );
        }
    }

    let rev = state.revision.fetch_add(1, Ordering::Relaxed) + 1;
    let mut snapshot = assemble(state, &raw, &globals, rev);
    // Ensure revision is authoritative even if assemble overwrites.
    snapshot.revision = rev;
    store(state, &snapshot, tick, was_connected);
    Ok(())
}

/// Turn raw torrents + globals into a [`Snapshot`] using the shared assembly.
fn assemble(
    state: &Arc<AppState>,
    raw: &[RawTorrent],
    globals: &RawGlobal,
    revision: u64,
) -> Snapshot {
    let torrents = snapshot::to_dtos(
        raw,
        |hash| state.tracker_host(hash),
        // The web server does not manage named throttles (v1), so every torrent
        // rides the global limits.
        |_name| None,
    );

    let (free_space, disk_size) = disk_figures(state);
    let globals = snapshot::to_globals(globals, free_space, disk_size, globals_turtle(globals));

    Snapshot {
        revision,
        torrents,
        globals,
        connection: state.conn(),
    }
}

/// The web server has no turtle schedule; reflect the daemon's own state, which
/// `global_stats` doesn't carry, so this is always false for now.
fn globals_turtle(_g: &RawGlobal) -> bool {
    false
}

/// Free/total bytes for the disk card. Mock mode returns the fixture figures so
/// the card renders offline; otherwise `statvfs` on the configured save path.
fn disk_figures(state: &Arc<AppState>) -> (Option<i64>, Option<i64>) {
    if state.config.mock {
        const GIB: i64 = 1024 * 1024 * 1024;
        return (Some(412 * GIB), Some(1114 * GIB));
    }
    if state.config.save_path.is_empty() {
        return (None, None);
    }
    match crate::disk::disk_usage(&state.config.save_path) {
        Some((free, total)) => (Some(free), Some(total)),
        None => (None, None),
    }
}

/// Serialize the snapshot and publish it to the cache, waking any waiters.
/// Also maintains the delta cache for `GET /api/delta` (FND-02).
fn store(state: &Arc<AppState>, snapshot: &Snapshot, tick: u64, was_connected: bool) {
    let body = match serde_json::to_vec(snapshot) {
        Ok(b) => b,
        Err(e) => {
            state.log(
                LogLevel::Error,
                format!("snapshot serialize failed: {e}"),
                None,
            );
            return;
        }
    };
    let etag = etag_of(&body);
    let at = std::time::Instant::now();
    let arc = std::sync::Arc::new(snapshot.clone());

    // Delta cache: keep previous snapshot to diff; clear on full reconciliation.
    let prev = state.prev_snapshot.read().unwrap().clone();
    let should_full = prev.is_none() || !was_connected || tick.is_multiple_of(FULL_EVERY);
    if should_full {
        *state.delta_cache.write().unwrap() = None;
    } else if let Some(prev_snap) = prev {
        let delta = rtorrent_core::delta::diff(&prev_snap, snapshot);
        if let Ok(dbody) = serde_json::to_vec(&delta) {
            let detag = etag_of(&dbody);
            *state.delta_cache.write().unwrap() = Some(crate::state::CachedDelta {
                etag: detag,
                body: dbody.into(),
                delta: std::sync::Arc::new(delta),
                at,
            });
        }
    }

    *state.prev_snapshot.write().unwrap() = Some(arc.clone());
    *state.cache.write().unwrap() = Some(Cached {
        etag,
        body: body.into(),
        snapshot: arc,
        at,
    });
    state.cache_updated.notify_waiters();
}

/// Record a disconnected connection state and publish an empty snapshot so the
/// UI can render the disconnected card.
fn set_disconnected(state: &Arc<AppState>, err: &RtorrentError, delay: u64) {
    let conn = ConnState {
        phase: ConnPhase::Disconnected,
        endpoint: crate::endpoint_label(&state.config.transport),
        daemon_version: None,
        error: Some(err.to_string()),
        retry_in_seconds: Some(delay as i64),
    };
    state.set_conn(conn.clone());
    let rev = state.revision.fetch_add(1, Ordering::Relaxed) + 1;
    let snapshot = Snapshot {
        revision: rev,
        torrents: vec![],
        globals: snapshot::empty_globals(),
        connection: conn,
    };
    // Disconnected is always a full snapshot; clear delta.
    *state.delta_cache.write().unwrap() = None;
    let body = serde_json::to_vec(&snapshot).unwrap_or_default();
    let etag = etag_of(&body);
    let arc = std::sync::Arc::new(snapshot);
    *state.prev_snapshot.write().unwrap() = Some(arc.clone());
    *state.cache.write().unwrap() = Some(Cached {
        etag,
        body: body.into(),
        snapshot: arc,
        at: std::time::Instant::now(),
    });
    state.cache_updated.notify_waiters();
}

/// Index a bounded batch of torrents' file lists (V3-12). One `f.multicall` per
/// torrent, a few per slow tick, so a 5k library fills in over minutes without a
/// burst — and a search never waits on it.
async fn index_files(state: &Arc<AppState>, raw: &[RawTorrent]) {
    /// Torrents indexed per slow tick.
    const FILE_INDEX_BATCH: usize = 5;
    let hashes: Vec<String> = raw.iter().map(|torrent| torrent.hash.clone()).collect();
    let needed: Vec<String> = {
        let index = state.file_index.lock().unwrap();
        index
            .needs(&hashes, FILE_INDEX_BATCH)
            .into_iter()
            .map(str::to_owned)
            .collect()
    };
    for hash in needed {
        if let Ok(files) = state.backend.files(&hash).await {
            let paths: Vec<String> = files.into_iter().map(|file| file.path).collect();
            state.file_index.lock().unwrap().record(&hash, paths);
        }
    }
}

/// Resolve tracker hosts for hashes we haven't seen yet (bounded per tick).
async fn resolve_trackers(state: &Arc<AppState>, raw: &[RawTorrent]) {
    let unknown: Vec<String> = {
        let cache = state.tracker_cache.lock().unwrap();
        raw.iter()
            .map(|t| t.hash.clone())
            .filter(|h| !cache.contains_key(h))
            .take(TRACKERS_PER_TICK)
            .collect()
    };
    for hash in unknown {
        if let Ok(host) = state.backend.primary_tracker(&hash).await {
            state.tracker_cache.lock().unwrap().insert(hash, host);
        }
    }
}

/// Prime the cache with a single poll — a helper for handler tests.
#[cfg(test)]
pub async fn run_one_for_test(state: &Arc<AppState>) {
    let _ = poll_once(state, 0).await;
}

/// Unix milliseconds for a history sample.
fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthMode, Config};
    use rtorrent_core::rtorrent::mock::MockClient;
    use rtorrent_core::types::Transport;
    use std::collections::HashSet;
    use std::sync::atomic::Ordering;

    fn mock_state() -> Arc<AppState> {
        let config = Config {
            listen: "127.0.0.1:9080".parse().unwrap(),
            transport: Transport::UnixSocket {
                path: String::new(),
            },
            daemon_password: None,
            auth_mode: AuthMode::None,
            password_hash: None,
            display_name: "sy".into(),
            save_path: String::new(),
            volumes: Vec::new(),
            incomplete_dir: String::new(),
            move_rules: Vec::new(),
            collision_policy: Default::default(),
            bandwidth_rules: Vec::new(),
            max_active_downloads: 0,
            max_active_uploads: 0,
            max_active_torrents: 0,
            queue_slow_limit_kbs: 0,
            poll_ms: 250,
            assets_dir: None,
            mock: true,
            config_path: None,
            hardening: Default::default(),
        };
        Arc::new(AppState::new(config, Box::new(MockClient::new())))
    }

    #[tokio::test]
    async fn poll_once_populates_the_cache_and_connects() {
        let state = mock_state();
        assert!(state.cache.read().unwrap().is_none());
        poll_once(&state, 0).await.unwrap();
        assert!(state.cache.read().unwrap().is_some());
        assert_eq!(state.conn().phase, ConnPhase::Connected);
        assert_eq!(state.poll_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn idle_parks_the_loop_and_a_request_wakes_it() {
        let state = mock_state();
        tokio::spawn(run(state.clone()));

        // Poll cadence is 250ms and the idle threshold 150ms: after the initial
        // burst the loop has no requests and parks. Let it settle, then confirm
        // the poll count stops advancing.
        tokio::time::sleep(Duration::from_millis(600)).await;
        let parked = state.poll_count.load(Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(500)).await;
        let still_parked = state.poll_count.load(Ordering::Relaxed);
        assert_eq!(
            parked, still_parked,
            "an idle loop must not keep polling the daemon"
        );

        // A state request wakes it for a fresh poll.
        state.mark_request();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            state.poll_count.load(Ordering::Relaxed) > still_parked,
            "a request must wake the parked loop"
        );
    }

    fn queued_state(max_downloads: i64) -> Arc<AppState> {
        let mut config = mock_state().config.clone();
        config.max_active_downloads = max_downloads;
        Arc::new(AppState::new(config, Box::new(MockClient::new())))
    }

    #[tokio::test]
    async fn queue_cap_holds_back_extra_downloads_but_keeps_them_loaded() {
        let state = queued_state(1);
        let was_active: HashSet<String> = state
            .backend
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .filter(|t| !t.complete && t.is_active)
            .map(|t| t.hash)
            .collect();
        assert!(was_active.len() > 1, "fixtures must overfill the cap");
        poll_once(&state, 0).await.unwrap();
        poll_once(&state, 1).await.unwrap();
        let rows = state.backend.list_snapshot().await.unwrap();
        let active: Vec<_> = rows.iter().filter(|t| !t.complete && t.is_active).collect();
        assert_eq!(active.len(), 1, "exactly one download keeps its slot");
        // Torrents the scheduler held back pause loaded (never closed): the
        // console words that state "Queued".
        for t in rows.iter().filter(|t| !t.complete && !t.is_active) {
            if was_active.contains(&t.hash) {
                assert!(t.is_open, "{} was held back but closed", t.name);
            }
        }
    }

    #[tokio::test]
    async fn queue_disabled_leaves_everything_alone() {
        let state = mock_state();
        let before: Vec<(String, bool)> = state
            .backend
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .map(|t| (t.hash, t.is_active))
            .collect();
        poll_once(&state, 0).await.unwrap();
        let after: Vec<(String, bool)> = state
            .backend
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .map(|t| (t.hash, t.is_active))
            .collect();
        assert_eq!(before, after, "no caps configured, no scheduler action");
    }
}
