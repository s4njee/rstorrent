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

/// Build the queue config (V3-17 / QUE-01) from settings.
fn queue_config(settings: &crate::ipc::Settings) -> rtorrent_core::queue::QueueConfig {
    rtorrent_core::queue::QueueConfig {
        max_downloads: settings.max_active_downloads,
        max_uploads: settings.max_active_uploads,
        max_total: settings.max_active_torrents,
        slow_limit_kbs: settings.queue_slow_limit_kbs,
    }
}

/// Execute one tick's bandwidth plan (V3-18 / QUE-04): define-once rule
/// throttles, assign/release, caps, and the adoption marker. Steady state
/// issues no daemon calls at all.
async fn apply_bandwidth(
    app: &AppHandle,
    state: &Arc<AppState>,
    backend: &dyn crate::rtorrent::RtorrentApi,
    settings: &crate::ipc::Settings,
    raw: &[RawTorrent],
    bw: &mut rtorrent_core::bandwidth::BandwidthState,
) {
    use rtorrent_core::bandwidth::{plan_bandwidth, BandwidthAction, RULE_KEY};
    let names: std::collections::HashMap<&str, &str> = raw
        .iter()
        .map(|t| (t.hash.as_str(), t.name.as_str()))
        .collect();
    let name_of = |hash: &str| names.get(hash).copied().unwrap_or(hash).to_owned();
    for action in plan_bandwidth(raw, &settings.bandwidth_rules) {
        match action {
            BandwidthAction::Adopt { hash, rule } => {
                let throttle = rule.throttle_name();
                if bw.needs_define(&throttle) {
                    if let Err(error) = backend
                        .define_named_throttle(&throttle, rule.down_kb, rule.up_kb)
                        .await
                    {
                        if bw.should_warn(&rule.id) {
                            state.log(
                                app,
                                LogLevel::Error,
                                format!(
                                    "bandwidth rule {} ({}): could not define throttle: {error}",
                                    rule.id,
                                    rule.describe()
                                ),
                                Some(hash),
                            );
                        }
                        continue;
                    }
                }
                bw.clear_warned(&rule.id);
                let one = std::slice::from_ref(&hash);
                if backend.assign_throttle(one, Some(&throttle)).await.is_err() {
                    continue;
                }
                let _ = backend
                    .set_connection_limits(&hash, rule.peers_max, rule.peers_min, rule.uploads_max)
                    .await;
                let _ = backend
                    .set_custom_metadata(&hash, &[(RULE_KEY, &rule.id)])
                    .await;
                state.log(
                    &app,
                    LogLevel::Info,
                    format!(
                        "bandwidth rule {} ({}) now shaping {}",
                        rule.id,
                        rule.describe(),
                        name_of(&hash)
                    ),
                    Some(hash),
                );
            }
            BandwidthAction::Release { hash } => {
                let one = std::slice::from_ref(&hash);
                let _ = backend.assign_throttle(one, None).await;
                let _ = backend.set_connection_limits(&hash, 0, 0, 0).await;
                let _ = backend.set_custom_metadata(&hash, &[(RULE_KEY, "")]).await;
                state.log(
                    &app,
                    LogLevel::Info,
                    format!("bandwidth rule released {hash} back to global limits"),
                    Some(hash),
                );
            }
        }
    }
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

/// How often a full snapshot is sent for reconciliation (FND-02).
const FULL_EVERY: u64 = 30;

/// The main ~1s poll: list + globals + tracker resolution + snapshot emit.
async fn fast_loop(app: AppHandle, state: Arc<AppState>) {
    let mut failures: usize = 0;
    let mut completion_tracker = CompletionTracker::default();
    let mut goal_stops: HashMap<String, GoalStopRecord> = HashMap::new();
    let mut queue_mem = rtorrent_core::queue::QueueMemory::default();
    let mut sched_mem = rtorrent_core::schedule::SchedMemory::default();
    let mut bw_state = rtorrent_core::bandwidth::BandwidthState::default();
    // Last global rate limits pushed to the daemon (B14). Recomputed each tick
    // from turtle state; re-applied only on change. Cleared on disconnect so a
    // reconnect re-applies.
    let mut applied_limits: Option<(i64, i64)> = None;
    // Successful-poll counter, used to refresh native views on a slow cadence.
    let mut tick: u64 = 0;
    // FND-02 revisioned delta state.
    let mut revision: u64 = 0;
    let mut last_snapshot: Option<Snapshot> = None;
    // Move-journal recovery runs once, after the first successful tick.
    let mut moves_resumed = false;

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
                    // Bandwidth-rule throttles (V3-18) are definitions too:
                    // replay every configured rule's rates.
                    for (name, (down_kb, up_kb)) in
                        rtorrent_core::bandwidth::rule_throttles(&state.settings().bandwidth_rules)
                    {
                        if let Err(error) =
                            backend.define_named_throttle(&name, down_kb, up_kb).await
                        {
                            state.log(
                                &app,
                                LogLevel::Error,
                                format!("could not restore bandwidth rule {name}: {error}"),
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
                    queue_mem.reset();
                    bw_state.reset();
                    sched_mem.reset();
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
                // Hashes policy acted on this tick: the mover must not touch
                // them (in particular it must never restart a seed-goal stop).
                let acted: HashSet<String> = decisions.iter().map(|d| d.hash.clone()).collect();
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

                // Move-on-complete (V3-14): plan from this tick's rows, one
                // detached task per intent. Runs after seed goals so the
                // skip-set above stays truthful.
                if !moves_resumed {
                    moves_resumed = true;
                    let resume_app = app.clone();
                    let resume_state = state.clone();
                    let resume_raw = raw.clone();
                    tauri::async_runtime::spawn(async move {
                        crate::moves::resume_once(&resume_app, &resume_state, &resume_raw).await;
                    });
                }
                crate::moves::run_due_moves(&app, &state, &raw, &acted).await;

                // Complete queue policy (V3-17 / QUE-01): downloads, seeds
                // and total caps with slow-torrent exemption and force-start.
                // Holds pause (stay loaded → renders "Queued"), never stop.
                // Seed-goal decrees and in-flight moves are skipped: the queue
                // must never restart a torrent a move task just stopped.
                let mut queue_skip = acted.clone();
                queue_skip.extend(state.moves.lock().unwrap().busy_hashes());
                let queue = rtorrent_core::queue::decide_queues(
                    &raw,
                    queue_config(&settings),
                    &mut queue_mem,
                    &queue_skip,
                );
                if !queue.stop.is_empty() && backend.pause(&queue.stop).await.is_ok() {
                    state.log(
                        &app,
                        LogLevel::Info,
                        format!(
                            "queued {} torrent(s) over the active limit",
                            queue.stop.len()
                        ),
                        None,
                    );
                }
                if !queue.start.is_empty() && backend.start(&queue.start).await.is_ok() {
                    state.log(
                        &app,
                        LogLevel::Info,
                        format!("started {} queued torrent(s)", queue.start.len()),
                        None,
                    );
                }

                // Bandwidth rules (V3-18 / QUE-04): adopt/release per the
                // plan; steady torrents cost zero daemon traffic.
                apply_bandwidth(
                    &app,
                    &state,
                    backend.as_ref(),
                    &settings,
                    &raw,
                    &mut bw_state,
                )
                .await;

                // Turtle mode (B14) is now the scheduler grid (V3-18 /
                // QUE-05): weekly windows + override + the manual toggle.
                // Pause windows halt traffic via scheduler-owned pauses;
                // limit windows push global rates change-gated as before.
                let now = Local::now();
                let weekday = now.weekday().num_days_from_sunday() as u8;
                let minute = i64::from(now.hour() * 60 + now.minute());
                let now_ms = now.timestamp_millis();
                let legacy = rtorrent_core::schedule::LegacyWindow {
                    enabled: settings.turtle_schedule.enabled,
                    start_min: settings.turtle_schedule.start_min,
                    end_min: settings.turtle_schedule.end_min,
                    days: settings.turtle_schedule.days.clone(),
                    down_kb: settings.turtle_down_kb,
                    up_kb: settings.turtle_up_kb,
                };
                let windows: Vec<rtorrent_core::schedule::SchedWindow> =
                    if settings.schedule.windows.is_empty() {
                        rtorrent_core::schedule::import_legacy(&legacy)
                    } else {
                        settings.schedule.windows.clone()
                    };
                let manual = settings
                    .turtle_enabled
                    .then_some((settings.turtle_down_kb, settings.turtle_up_kb));
                let sched_state = rtorrent_core::schedule::evaluate(
                    &windows,
                    settings.schedule.temp_override.as_ref(),
                    manual,
                    weekday,
                    minute,
                    now_ms,
                );
                let turtle_active = sched_state.turtle_active();
                // Pause transitions own their torrents through the queue's
                // user-paused set, so neither the queue nor a reconnect
                // mistakes them for manual pauses.
                let actions = sched_mem.transition(
                    sched_state.is_paused(),
                    raw.iter()
                        .filter(|t| t.is_active)
                        .map(|t| t.hash.clone())
                        // Plus anything the queue just promoted: the snapshot
                        // predates this tick's starts.
                        .chain(queue.start.iter().cloned()),
                );
                if !actions.pause.is_empty() {
                    if backend.pause(&actions.pause).await.is_ok() {
                        for h in &actions.pause {
                            queue_mem.set_scheduler_held(h, true);
                        }
                        state.log(
                            &app,
                            LogLevel::Info,
                            format!(
                                "scheduler paused {} torrent(s) for the pause window",
                                actions.pause.len()
                            ),
                            None,
                        );
                    } else {
                        // Failed to pause: forget the entry so the next tick
                        // retries instead of believing the torrents are held.
                        sched_mem.reset();
                    }
                }
                if !actions.release.is_empty() {
                    for h in &actions.release {
                        queue_mem.set_scheduler_held(h, false);
                    }
                    // No starts here: the queue converges wanted torrents on
                    // its next tick, and anything else stays as the user or
                    // the seed goals left it.
                    state.log(
                        &app,
                        LogLevel::Info,
                        format!(
                            "scheduler released {} torrent(s) at the window end",
                            actions.release.len()
                        ),
                        None,
                    );
                }
                // Limit states push global rates exactly as turtle did; while
                // paused there is nothing to push and the last-applied memory
                // is left alone so resume re-pushes only on change.
                if !sched_state.is_paused() {
                    let limits = match &sched_state {
                        rtorrent_core::schedule::SchedState::Limited { down_kb, up_kb } => {
                            (*down_kb, *up_kb)
                        }
                        _ => (settings.down_limit_kb, settings.up_limit_kb),
                    };
                    if applied_limits != Some(limits)
                        && backend.set_throttles(limits.0, limits.1).await.is_ok()
                    {
                        applied_limits = Some(limits);
                    }
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
                crate::tray::update_tray(
                    &app,
                    globals.down_rate,
                    globals.up_rate,
                    notifications::active_download_count(&raw) as usize,
                    turtle_active,
                );
                revision = revision.wrapping_add(1);
                let mut snapshot =
                    build_snapshot(&state, raw, globals, turtle_active, revision).await;
                // Ensure revision is monotonic even if build_snapshot helper
                // overwrites it; keep the poller's counter authoritative.
                snapshot.revision = revision;
                state.set_snapshot(snapshot.clone());
                // FND-02: emit delta when we have a previous snapshot and this
                // is not the first tick after a (re)connect and not a
                // periodic full reconciliation.
                let should_emit_full =
                    last_snapshot.is_none() || !continuing_session || tick % FULL_EVERY == 0;
                if should_emit_full {
                    let _ = app.emit("state://snapshot", &snapshot);
                } else if let Some(prev) = last_snapshot.as_ref() {
                    let delta = rtorrent_core::delta::diff(prev, &snapshot);
                    let _ = app.emit("state://delta", &delta);
                } else {
                    let _ = app.emit("state://snapshot", &snapshot);
                }
                last_snapshot = Some(snapshot);
            }
            Err(e) => {
                failures += 1;
                completion_tracker.reset();
                goal_stops.clear();
                queue_mem.reset();
                sched_mem.reset();
                applied_limits = None;
                notifications::set_dock_badge(&app, 0);
                crate::tray::update_tray(&app, 0, 0, 0, false);
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
                revision = revision.wrapping_add(1);
                let snap = Snapshot {
                    revision,
                    torrents: vec![],
                    globals: empty_globals(),
                    connection: conn,
                };
                state.set_snapshot(snap.clone());
                last_snapshot = Some(snap.clone());
                let _ = app.emit("state://snapshot", &snap);
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
    revision: u64,
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
        revision,
        // Total-volume size is only surfaced by the web disk card; the desktop
        // status bar shows free space alone, so `disk_size` stays `None` here
        // (WE0-S2). Globals assembly itself is shared with the web server.
        globals: rtorrent_core::snapshot::to_globals(&g, free_space, None, turtle_active),
        connection: state.conn(),
        torrents,
    }
}

fn empty_globals() -> GlobalStats {
    rtorrent_core::snapshot::empty_globals()
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

    #[test]
    fn queue_config_maps_all_four_settings() {
        let mut settings = crate::ipc::Settings::default();
        settings.max_active_downloads = 3;
        settings.max_active_uploads = 2;
        settings.max_active_torrents = 4;
        settings.queue_slow_limit_kbs = 50;
        let cfg = queue_config(&settings);
        assert_eq!(cfg.max_downloads, 3);
        assert_eq!(cfg.max_uploads, 2);
        assert_eq!(cfg.max_total, 4);
        assert_eq!(cfg.slow_limit_kbs, 50);
    }
}
