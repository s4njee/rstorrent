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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};

use chrono::{Datelike, Local, Timelike};

use crate::ipc::{
    ConnPhase, ConnState, DetailPayload, DetailTab, GlobalStats, LabelSeedGoal, LogLevel, SeedGoal,
    SeedGoalAction, Snapshot, TorrentDto,
};
use crate::notifications::{self, CompletionTracker};
use crate::rtorrent::{derive, RawGlobal, RawTorrent};
use crate::settings;
use crate::state::AppState;

/// Backoff schedule (seconds) applied after consecutive fast-poll failures.
const BACKOFF: [u64; 4] = [1, 2, 5, 10];
/// Max new tracker hosts resolved per fast poll, to avoid a burst on first load.
const TRACKERS_PER_TICK: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GoalStopRecord {
    ratio_permille: i64,
    finished_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SeedGoalDecision {
    hash: String,
    message: String,
    record: GoalStopRecord,
}

/// Pure seed-goal policy: select completed, active torrents whose applicable
/// ratio or elapsed-time rule has been reached.
///
/// A successful goal-stop is remembered at its ratio and completion timestamp.
/// If the user manually starts that torrent again, the same completion is not
/// stopped immediately. It becomes eligible again only after its ratio grows,
/// or after rtorrent reports a new completion timestamp (a fresh completion).
fn seed_goal_decisions(
    torrents: &[RawTorrent],
    global: &SeedGoal,
    overrides: &[LabelSeedGoal],
    already_stopped: &HashMap<String, GoalStopRecord>,
    now: i64,
) -> Vec<SeedGoalDecision> {
    let mut seen = HashSet::new();
    torrents
        .iter()
        .filter(|torrent| {
            torrent.complete && torrent.is_active && seen.insert(torrent.hash.clone())
        })
        .filter_map(|torrent| {
            let goal = overrides
                .iter()
                .find(|goal| goal.label == torrent.label)
                .map(|goal| SeedGoal {
                    stop_ratio: goal.stop_ratio,
                    seed_hours: goal.seed_hours,
                })
                .unwrap_or_else(|| global.clone());

            if goal.stop_ratio <= 0.0 && goal.seed_hours <= 0.0 {
                return None;
            }

            if already_stopped.get(&torrent.hash).is_some_and(|record| {
                record.finished_at == torrent.finished_at
                    && torrent.ratio_permille <= record.ratio_permille
            }) {
                return None;
            }

            let ratio = torrent.ratio_permille as f64 / 1000.0;
            let ratio_met = goal.stop_ratio > 0.0 && ratio >= goal.stop_ratio;
            let seeded_seconds = (torrent.finished_at > 0 && now >= torrent.finished_at)
                .then_some(now - torrent.finished_at);
            let time_met = goal.seed_hours > 0.0
                && seeded_seconds.is_some_and(|seconds| seconds as f64 >= goal.seed_hours * 3600.0);

            let message = if ratio_met {
                format!(
                    "seed goal reached: ratio {ratio:.1} ≥ {:.1}",
                    goal.stop_ratio
                )
            } else if time_met {
                format!(
                    "seed goal reached: seeded {:.1} h ≥ {:.1} h",
                    seeded_seconds.unwrap_or_default() as f64 / 3600.0,
                    goal.seed_hours
                )
            } else {
                return None;
            };

            Some(SeedGoalDecision {
                hash: torrent.hash.clone(),
                message,
                record: GoalStopRecord {
                    ratio_permille: torrent.ratio_permille,
                    finished_at: torrent.finished_at,
                },
            })
        })
        .collect()
}

/// Carry out the configured seed-goal action for the reached goals (C14):
/// stop and remember (so a manual restart isn't re-stopped), or remove the
/// torrent — optionally trashing its data for a local daemon.
async fn apply_seed_goal(
    app: &AppHandle,
    state: &Arc<AppState>,
    backend: &dyn crate::rtorrent::RtorrentApi,
    settings: &crate::ipc::Settings,
    raw: &[RawTorrent],
    decisions: Vec<SeedGoalDecision>,
    goal_stops: &mut HashMap<String, GoalStopRecord>,
) {
    let hashes: Vec<String> = decisions.iter().map(|d| d.hash.clone()).collect();
    match settings.seed_goal_action {
        SeedGoalAction::Stop => match backend.stop(&hashes).await {
            Ok(()) => {
                for d in decisions {
                    goal_stops.insert(d.hash.clone(), d.record);
                    state.log(
                        app,
                        LogLevel::Info,
                        format!("{} — stopped", d.message),
                        Some(d.hash),
                    );
                }
            }
            Err(error) => state.log(
                app,
                LogLevel::Error,
                format!("could not stop torrent at seed goal: {error}"),
                None,
            ),
        },
        SeedGoalAction::Remove | SeedGoalAction::RemoveData => {
            let with_data = settings.seed_goal_action == SeedGoalAction::RemoveData;
            let local = settings::is_localhost(&settings.transport);
            // Read base paths before erasing, so we can trash the data after.
            let paths: Vec<String> = if with_data && local {
                hashes
                    .iter()
                    .filter_map(|h| raw.iter().find(|t| &t.hash == h))
                    .map(|t| t.base_path.clone())
                    .filter(|p| !p.is_empty())
                    .collect()
            } else {
                Vec::new()
            };
            match backend.erase(&hashes).await {
                Ok(()) => {
                    for p in &paths {
                        match crate::localfs::trash(p) {
                            Ok(_) => {
                                state.log(app, LogLevel::Info, format!("moved to Trash: {p}"), None)
                            }
                            Err(err) => state.log(
                                app,
                                LogLevel::Warn,
                                format!("could not trash {p}: {err}"),
                                None,
                            ),
                        }
                    }
                    let verb = if with_data {
                        "removed with data"
                    } else {
                        "removed"
                    };
                    for d in decisions {
                        state.log(
                            app,
                            LogLevel::Info,
                            format!("{} — {verb}", d.message),
                            Some(d.hash),
                        );
                    }
                }
                Err(error) => state.log(
                    app,
                    LogLevel::Error,
                    format!("could not remove torrent at seed goal: {error}"),
                    None,
                ),
            }
        }
    }
}

/// Start/stop decisions to honor the max-active-downloads limit (C9).
#[derive(Debug, Default, PartialEq, Eq)]
struct QueueActions {
    start: Vec<String>,
    stop: Vec<String>,
}

/// `started_at` with 0 (unknown) sorted last, so torrents with a real start
/// time keep their slots ahead of ones we can't order.
fn started_key(t: &RawTorrent) -> i64 {
    if t.started_at == 0 {
        i64::MAX
    } else {
        t.started_at
    }
}

/// Decide which downloads to start/stop to honor the max-active limit (C9).
///
/// Considers only incomplete torrents that are neither hash-checking nor in an
/// error state. Keeps the highest-priority `max_active` of them active (ties
/// broken by earliest start), stops the rest, and starts stopped ones to fill
/// free slots. `max_active <= 0` disables queue management.
fn queue_decisions(torrents: &[RawTorrent], max_active: i64) -> QueueActions {
    let mut actions = QueueActions::default();
    if max_active <= 0 {
        return actions;
    }
    let mut downloads: Vec<&RawTorrent> = torrents
        .iter()
        .filter(|t| !t.complete && !t.hashing && t.message.is_empty())
        .collect();
    downloads.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| started_key(a).cmp(&started_key(b)))
    });
    for (i, t) in downloads.iter().enumerate() {
        let want_active = (i as i64) < max_active;
        if want_active && !t.is_active {
            actions.start.push(t.hash.clone());
        } else if !want_active && t.is_active {
            actions.stop.push(t.hash.clone());
        }
    }
    actions
}

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
    let mut goal_stops: HashMap<String, GoalStopRecord> = HashMap::new();
    // Last global rate limits pushed to the daemon (B14). Recomputed each tick
    // from turtle state; re-applied only on change. Cleared on disconnect so a
    // reconnect re-applies.
    let mut applied_limits: Option<(i64, i64)> = None;
    // Successful-poll counter, used to refresh native views on a slow cadence.
    let mut tick: u64 = 0;

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
                    // The assignment is persisted by rtorrent, but named
                    // throttle definitions are not. Replay our small pool on
                    // every initial connection and reconnect.
                    for definition in &state.settings().torrent_throttles {
                        if let Err(error) = backend
                            .define_named_throttle(
                                &definition.name,
                                definition.down_kb,
                                definition.up_kb,
                            )
                            .await
                        {
                            state.log(
                                &app,
                                LogLevel::Error,
                                format!(
                                    "could not restore rate limit {}: {error}",
                                    definition.name
                                ),
                                None,
                            );
                        }
                    }
                    // Push the app-owned network prefs (encryption/PEX, proxy,
                    // bind, global caps). rtorrent forgets runtime config on a
                    // restart and several have no getter, so replay them here.
                    crate::network_prefs::apply(backend.as_ref(), &state.settings()).await;
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
                    // Basic auth is base64, not encryption. Say so plainly when
                    // credentials are actually crossing a network in the clear —
                    // Preferences warns up front, but settings can also arrive by
                    // other routes (a hand-edited file, an older build).
                    if let crate::ipc::Transport::Http { url, username } = &s.transport {
                        if crate::rtorrent::http::is_insecure_credentialed(url, username) {
                            state.log(
                                &app,
                                LogLevel::Warn,
                                "sending credentials over plain http — anything on the \
                                 network path can read them; prefer https",
                                None,
                            );
                        }
                    }
                }
                failures = 0;

                if !continuing_session {
                    completion_tracker.reset();
                    goal_stops.clear();
                }

                let settings = state.settings();
                let completed = completion_tracker
                    .observe(&raw, &settings.completion_notification_excluded_labels);
                notifications::set_dock_badge(&app, notifications::active_download_count(&raw));
                for completion in completed {
                    // Run-on-complete hook (C13): fire the user's command, then
                    // still post the notification.
                    if !settings.run_on_complete.is_empty() {
                        if let Some(program) = crate::hooks::run_on_complete(
                            &settings.run_on_complete,
                            &completion.name,
                            &completion.base_path,
                            &completion.hash,
                        ) {
                            state.log(
                                &app,
                                LogLevel::Info,
                                format!("run-on-complete: launched {program}"),
                                Some(completion.hash.clone()),
                            );
                        }
                    }
                    notifications::post_completion(app.clone(), completion);
                }

                let decisions = seed_goal_decisions(
                    &raw,
                    &settings.global_seed_goal,
                    &settings.label_seed_goals,
                    &goal_stops,
                    unix_now(),
                );
                if !decisions.is_empty() {
                    apply_seed_goal(
                        &app,
                        &state,
                        backend.as_ref(),
                        &settings,
                        &raw,
                        decisions,
                        &mut goal_stops,
                    )
                    .await;
                }

                // Max-active-downloads queue (C9): keep the top N incomplete
                // torrents downloading, stop the rest, promote as slots free.
                let queue = queue_decisions(&raw, settings.max_active_downloads);
                if !queue.stop.is_empty() && backend.stop(&queue.stop).await.is_ok() {
                    state.log(
                        &app,
                        LogLevel::Info,
                        format!(
                            "queued {} download(s) over the active limit",
                            queue.stop.len()
                        ),
                        None,
                    );
                }
                if !queue.start.is_empty() && backend.start(&queue.start).await.is_ok() {
                    state.log(
                        &app,
                        LogLevel::Info,
                        format!("started {} queued download(s)", queue.start.len()),
                        None,
                    );
                }

                // Turtle mode (B14): compute the effective global limits for the
                // current wall clock and push them only when they change.
                let now = Local::now();
                let turtle_active = crate::turtle::is_active(
                    &settings,
                    now.weekday().num_days_from_sunday() as u8,
                    i64::from(now.hour() * 60 + now.minute()),
                );
                let limits = crate::turtle::effective_limits(&settings, turtle_active);
                if applied_limits != Some(limits)
                    && backend.set_throttles(limits.0, limits.1).await.is_ok()
                {
                    applied_limits = Some(limits);
                }

                resolve_trackers(&app, &state, &raw).await;
                // Refresh native views (D12) every ~5 successful polls — cheap
                // enough to keep the sidebar current, rare enough to be light.
                if tick % 5 == 0 {
                    if let Ok(views) = backend.views().await {
                        *state.views.lock().unwrap() = views;
                    }
                }
                tick += 1;
                let snapshot = build_snapshot(&state, raw, globals, turtle_active).await;
                let _ = app.emit("state://snapshot", &snapshot);
            }
            Err(e) => {
                failures += 1;
                completion_tracker.reset();
                goal_stops.clear();
                applied_limits = None;
                notifications::set_dock_badge(&app, 0);
                let delay = BACKOFF[(failures - 1).min(BACKOFF.len() - 1)];
                let s = state.settings();
                // Only log the first failure of a streak to avoid log spam.
                if failures == 1 {
                    state.log(
                        &app,
                        LogLevel::Error,
                        format!("rtorrent unreachable: {e}"),
                        None,
                    );
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

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

/// Sleep for `ms`, waking early if an immediate re-poll is requested.
async fn wait(ms: u64, state: &AppState) {
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_millis(ms)) => {}
        _ = state.repoll.notified() => {}
    }
}

/// Resolve tracker hosts for hashes we haven't seen yet (bounded per tick).
async fn resolve_trackers(
    _app: &AppHandle,
    state: &Arc<AppState>,
    raw: &[crate::rtorrent::RawTorrent],
) {
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
///
/// Async only because the free-space probe shells out to WSL on Windows and so
/// has to be pushed onto the blocking pool.
async fn build_snapshot(
    state: &AppState,
    raw: Vec<crate::rtorrent::RawTorrent>,
    g: RawGlobal,
    turtle_active: bool,
) -> Snapshot {
    let settings = state.settings();
    let mut torrents: Vec<TorrentDto> = raw
        .iter()
        .map(|t| {
            let limits = settings
                .torrent_throttles
                .iter()
                .find(|definition| definition.name == t.throttle_name)
                .map(|definition| (definition.down_kb, definition.up_kb));
            derive::to_dto(t, &state.tracker_host(&t.hash), limits)
        })
        .collect();

    // Tag each torrent with the native views it belongs to (D12), inverting the
    // cached name→hashes map the slow poll maintains.
    {
        let views = state.views.lock().unwrap();
        if !views.is_empty() {
            let mut by_hash: HashMap<&str, Vec<String>> = HashMap::new();
            for (name, hashes) in views.iter() {
                for h in hashes {
                    by_hash.entry(h.as_str()).or_default().push(name.clone());
                }
            }
            for dto in torrents.iter_mut() {
                if let Some(v) = by_hash.get(dto.hash.as_str()) {
                    dto.views = v.clone();
                }
            }
        }
    }

    // Free space is only meaningful for a local daemon, and on Windows costs a
    // `wsl.exe df` — so it is TTL-cached inside `localfs` and read off the
    // runtime threads. `None` means "unknown" and hides the readout.
    let free_space = if settings.mock {
        Some(412 * 1_073_741_824_i64)
    } else if crate::settings::is_localhost(&settings.transport)
        && !settings.default_save_path.is_empty()
    {
        let path = settings.default_save_path.clone();
        tokio::task::spawn_blocking(move || crate::localfs::free_space(&path))
            .await
            .unwrap_or(None)
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
            turtle_active,
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
        turtle_active: false,
    }
}

/// The ~2s detail poll for the watched torrent/tab.
async fn detail_loop(app: AppHandle, state: Arc<AppState>) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            _ = state.detail_repoll.notified() => {}
        }

        let watch = state.detail_watch.lock().unwrap().clone();
        let Some((hash, tab)) = watch else { continue };

        let backend = state.backend();
        // Only the data-bearing tabs need a fetch; general/speed/log are derived
        // on the frontend from the snapshot / log stream.
        let payload = match tab {
            DetailTab::Trackers => backend
                .trackers(&hash)
                .await
                .ok()
                .map(|rows| DetailPayload {
                    hash: hash.clone(),
                    tab,
                    trackers: Some(rows),
                    peers: None,
                    files: None,
                    pieces: None,
                }),
            DetailTab::Peers => backend.peers(&hash).await.ok().map(|rows| DetailPayload {
                hash: hash.clone(),
                tab,
                trackers: None,
                peers: Some(rows),
                files: None,
                pieces: None,
            }),
            DetailTab::Content => backend.files(&hash).await.ok().map(|rows| DetailPayload {
                hash: hash.clone(),
                tab,
                trackers: None,
                peers: None,
                files: Some(rows),
                pieces: None,
            }),
            // General carries the pieces bar, so it now needs a fetch too.
            DetailTab::General => backend.pieces(&hash).await.ok().map(|p| DetailPayload {
                hash: hash.clone(),
                tab,
                trackers: None,
                peers: None,
                files: None,
                pieces: Some(p),
            }),
            DetailTab::Speed | DetailTab::Log => None,
        };

        if let Some(p) = payload {
            let _ = app.emit("state://detail", &p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;

    fn torrent(
        ratio_permille: i64,
        finished_at: i64,
        label: &str,
        complete: bool,
        active: bool,
    ) -> RawTorrent {
        RawTorrent {
            hash: "HASH".into(),
            ratio_permille,
            finished_at,
            label: label.into(),
            complete,
            is_active: active,
            ..RawTorrent::default()
        }
    }

    #[test]
    fn seed_goal_policy_is_table_driven() {
        struct Case {
            name: &'static str,
            torrent: RawTorrent,
            global: SeedGoal,
            overrides: Vec<LabelSeedGoal>,
            stopped: HashMap<String, GoalStopRecord>,
            should_stop: bool,
        }

        let ratio_goal = SeedGoal {
            stop_ratio: 2.0,
            seed_hours: 0.0,
        };
        let time_goal = SeedGoal {
            stop_ratio: 0.0,
            seed_hours: 2.0,
        };
        let stopped_at_two = HashMap::from([(
            "HASH".into(),
            GoalStopRecord {
                ratio_permille: 2_000,
                finished_at: NOW - 10_800,
            },
        )]);
        let cases = vec![
            Case {
                name: "ratio met",
                torrent: torrent(2_000, NOW - 60, "", true, true),
                global: ratio_goal.clone(),
                overrides: vec![],
                stopped: HashMap::new(),
                should_stop: true,
            },
            Case {
                name: "time met",
                torrent: torrent(100, NOW - 10_800, "", true, true),
                global: time_goal.clone(),
                overrides: vec![],
                stopped: HashMap::new(),
                should_stop: true,
            },
            Case {
                name: "both configured use OR semantics",
                torrent: torrent(500, NOW - 10_800, "", true, true),
                global: SeedGoal {
                    stop_ratio: 2.0,
                    seed_hours: 2.0,
                },
                overrides: vec![],
                stopped: HashMap::new(),
                should_stop: true,
            },
            Case {
                name: "label override beats met global goal",
                torrent: torrent(2_500, NOW - 60, "video", true, true),
                global: ratio_goal.clone(),
                overrides: vec![LabelSeedGoal {
                    label: "video".into(),
                    stop_ratio: 5.0,
                    seed_hours: 0.0,
                }],
                stopped: HashMap::new(),
                should_stop: false,
            },
            Case {
                name: "explicit label no-limit beats global",
                torrent: torrent(5_000, NOW - 10_800, "archive", true, true),
                global: ratio_goal.clone(),
                overrides: vec![LabelSeedGoal {
                    label: "archive".into(),
                    stop_ratio: 0.0,
                    seed_hours: 0.0,
                }],
                stopped: HashMap::new(),
                should_stop: false,
            },
            Case {
                name: "global no-limit",
                torrent: torrent(9_000, NOW - 86_400, "", true, true),
                global: SeedGoal::default(),
                overrides: vec![],
                stopped: HashMap::new(),
                should_stop: false,
            },
            Case {
                name: "missing finished timestamp skips time rule",
                torrent: torrent(100, 0, "", true, true),
                global: time_goal,
                overrides: vec![],
                stopped: HashMap::new(),
                should_stop: false,
            },
            Case {
                name: "incomplete torrent",
                torrent: torrent(3_000, NOW - 10_800, "", false, true),
                global: ratio_goal.clone(),
                overrides: vec![],
                stopped: HashMap::new(),
                should_stop: false,
            },
            Case {
                name: "already inactive torrent",
                torrent: torrent(3_000, NOW - 10_800, "", true, false),
                global: ratio_goal.clone(),
                overrides: vec![],
                stopped: HashMap::new(),
                should_stop: false,
            },
            Case {
                name: "manually restarted goal-stop at same ratio",
                torrent: torrent(2_000, NOW - 10_800, "", true, true),
                global: ratio_goal.clone(),
                overrides: vec![],
                stopped: stopped_at_two,
                should_stop: false,
            },
            Case {
                name: "restarted goal-stop after ratio advances",
                torrent: torrent(2_001, NOW - 10_800, "", true, true),
                global: ratio_goal,
                overrides: vec![],
                stopped: HashMap::from([(
                    "HASH".into(),
                    GoalStopRecord {
                        ratio_permille: 2_000,
                        finished_at: NOW - 10_800,
                    },
                )]),
                should_stop: true,
            },
        ];

        for case in cases {
            let decisions = seed_goal_decisions(
                &[case.torrent],
                &case.global,
                &case.overrides,
                &case.stopped,
                NOW,
            );
            assert_eq!(!decisions.is_empty(), case.should_stop, "{}", case.name);
        }

        let duplicate = torrent(2_000, NOW - 60, "", true, true);
        let decisions = seed_goal_decisions(
            &[duplicate.clone(), duplicate],
            &SeedGoal {
                stop_ratio: 2.0,
                seed_hours: 0.0,
            },
            &[],
            &HashMap::new(),
            NOW,
        );
        assert_eq!(
            decisions.len(),
            1,
            "a hash is stopped at most once per poll"
        );
    }

    fn dl(hash: &str, priority: i64, active: bool, started_at: i64) -> RawTorrent {
        RawTorrent {
            hash: hash.into(),
            complete: false,
            is_active: active,
            priority,
            started_at,
            ..RawTorrent::default()
        }
    }

    #[test]
    fn queue_keeps_top_priority_active_and_promotes_to_fill() {
        // Three incomplete downloads, cap of 2. B (pri 3) and A (pri 2) should be
        // the two active; C (pri 1) is queued.
        let torrents = vec![
            dl("A", 2, true, 100),
            dl("B", 3, false, 90), // higher priority but stopped → promote
            dl("C", 1, true, 80),  // lowest priority but active → stop
        ];
        let actions = queue_decisions(&torrents, 2);
        assert_eq!(actions.start, vec!["B".to_string()]);
        assert_eq!(actions.stop, vec!["C".to_string()]);
    }

    #[test]
    fn queue_zero_is_disabled() {
        let torrents = vec![dl("A", 2, false, 1), dl("B", 2, true, 2)];
        assert_eq!(queue_decisions(&torrents, 0), QueueActions::default());
    }

    #[test]
    fn queue_ignores_complete_hashing_and_errored() {
        let mut complete = dl("DONE", 3, true, 1);
        complete.complete = true;
        let mut hashing = dl("CHK", 3, true, 1);
        hashing.hashing = true;
        let mut errored = dl("ERR", 3, true, 1);
        errored.message = "tracker down".into();
        let active = dl("A", 1, true, 1);
        // Cap 0-of-these-managed: only "A" is a managed download; cap 1 keeps it.
        let actions = queue_decisions(&[complete, hashing, errored, active], 1);
        assert!(actions.start.is_empty() && actions.stop.is_empty());
    }
}
