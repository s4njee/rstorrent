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

use crate::ipc::{
    ConnPhase, ConnState, DetailPayload, DetailTab, GlobalStats, LabelSeedGoal, LogLevel, SeedGoal,
    Snapshot, TorrentDto,
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
                    "seed goal reached: ratio {ratio:.1} ≥ {:.1} — stopped",
                    goal.stop_ratio
                )
            } else if time_met {
                format!(
                    "seed goal reached: seeded {:.1} h ≥ {:.1} h — stopped",
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
                    let hashes: Vec<String> = decisions
                        .iter()
                        .map(|decision| decision.hash.clone())
                        .collect();
                    match backend.stop(&hashes).await {
                        Ok(()) => {
                            for decision in decisions {
                                goal_stops.insert(decision.hash.clone(), decision.record);
                                state.log(
                                    &app,
                                    LogLevel::Info,
                                    decision.message,
                                    Some(decision.hash),
                                );
                            }
                        }
                        Err(error) => state.log(
                            &app,
                            LogLevel::Error,
                            format!("could not stop torrent at seed goal: {error}"),
                            None,
                        ),
                    }
                }

                resolve_trackers(&app, &state, &raw).await;
                let snapshot = build_snapshot(&state, raw, globals);
                let _ = app.emit("state://snapshot", &snapshot);
            }
            Err(e) => {
                failures += 1;
                completion_tracker.reset();
                goal_stops.clear();
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
fn build_snapshot(
    state: &AppState,
    raw: Vec<crate::rtorrent::RawTorrent>,
    g: RawGlobal,
) -> Snapshot {
    let settings = state.settings();
    let torrents: Vec<TorrentDto> = raw
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

    // Free space is only meaningful for a local daemon; a real statvfs is a
    // follow-up, so we surface the mock's fixed value and otherwise None.
    let free_space = if settings.mock {
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
}
