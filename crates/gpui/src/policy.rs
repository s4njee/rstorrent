//! Poller policy: seed goals, the max-active queue, and completion hooks.
//!
//! A port of the policy half of `src-tauri/src/poller.rs` plus
//! `src-tauri/src/hooks.rs`. Everything decision-shaped is pure here and
//! table-tested; the only impure parts are the two appliers that drive the
//! backend (`apply_seed_goal`, `apply_queue`) and the detached-thread process
//! spawn in [`run_on_complete`].
//!
//! Policy runs inside `TorrentsModel::apply_tick`, after a successful poll and
//! before the rows are published, so the rows the model holds already reflect
//! what policy just decided.

use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rtorrent_core::complete::Journal;
use rtorrent_core::mover::{self, ActiveMove, MoveIntent, MoveStatus, MoveTask};
use rtorrent_core::rtorrent::{LoadOptions, RawTorrent};

use crate::settings::{LabelSeedGoal, SeedGoal, SeedGoalAction, Settings};

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

/// Poll-side queue memory lives in the shared driver now
/// ([`rtorrent_core::queue::QueueMemory`]); see `run_tick` stage 3.

/// Substitute the hook tokens in one template token.
fn substitute(token: &str, name: &str, path: &str, hash: &str) -> String {
    token
        .replace("%N", name)
        .replace("%F", path)
        .replace("%H", hash)
}

/// Split `template` into `(program, args)` with the tokens substituted, or
/// `None` if the template is blank.
fn build(template: &str, name: &str, path: &str, hash: &str) -> Option<(String, Vec<String>)> {
    let mut tokens = template.split_whitespace();
    let program = substitute(tokens.next()?, name, path, hash);
    let args = tokens.map(|t| substitute(t, name, path, hash)).collect();
    Some((program, args))
}

/// Run the completion hook for one torrent, if `template` is non-empty.
///
/// Spawns on a detached thread that waits on the child, so the poll loop never
/// blocks and no zombie is left behind. Returns the resolved program name (for
/// logging) when a hook was launched.
fn run_on_complete(template: &str, name: &str, path: &str, hash: &str) -> Option<String> {
    let (program, args) = build(template, name, path, hash)?;
    let launched = program.clone();
    // `status()` waits for and reaps the child; do it off-thread.
    let _ = std::thread::Builder::new()
        .name("completion-hook".to_string())
        .spawn(move || {
            let _ = Command::new(&program).args(&args).status();
        });
    Some(launched)
}

/// Current Unix time in milliseconds; 0 only if the clock is unreadable.
pub(crate) fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Current Unix time in seconds; 0 only if the clock is unreadable.
fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// A completion the model has not seen finished before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub hash: String,
    pub name: String,
    pub size_bytes: i64,
    pub base_path: String,
}

/// Runs the model's poll-side automation after a successful tick.
///
/// This is the one entry point the model calls. Each stage reads the same
/// `settings` snapshot and the same raw rows the poll just fetched, applies
/// what it decided, and reports back through `PolicyLog` — plain data the
/// caller turns into log lines — so the policy itself never touches GPUI or
/// the app log.
///
/// Stages, in the order the Tauri poller ran them: reconnect replay (throttle
/// pool + network prefs, once per session), completions (hook), seed goals,
/// the max-active queue, move-on-complete (V3-14, detached tasks), then
/// turtle limits. Each returns quickly on empty/defaulted settings, so a
/// default config costs one extra list scan.
pub async fn run_tick(
    services: &Arc<crate::services::Services>,
    settings: &Settings,
    raw: &[RawTorrent],
    state: &mut PolicyState,
    moves: &Arc<Mutex<mover::MoveStore>>,
) -> PolicyLog {
    let mut log = PolicyLog::default();

    // 0. Reconnect replay: the assignment is persisted by rtorrent, but named
    // throttle definitions are not — replay the app's pool, then the network
    // prefs rtorrent also forgets and several of which have no getter.
    if state.needs_replay {
        state.needs_replay = false;
        let backend = services.backend();
        for definition in &settings.torrent_throttles {
            let result = backend
                .define_named_throttle(&definition.name, definition.down_kb, definition.up_kb)
                .await;
            if let Err(error) = result {
                log.lines.push(PolicyLine::SeedFailed {
                    message: format!("could not restore rate limit {}: {error}", definition.name),
                });
            }
        }
        // Bandwidth-rule throttles (V3-18) are definitions too; mark them
        // defined so the steady tick skips re-defining.
        for (name, (down_kb, up_kb)) in
            rtorrent_core::bandwidth::rule_throttles(&settings.bandwidth_rules)
        {
            if let Err(error) = backend.define_named_throttle(&name, down_kb, up_kb).await {
                log.lines.push(PolicyLine::BandwidthFailed {
                    message: format!("could not restore bandwidth rule {name}: {error}"),
                });
            }
            state.bw_state.needs_define(&name);
        }
        crate::network_prefs::apply(&*services.backend(), settings).await;
    }

    // 1. Completions: hook + notification eligibility, tracked across ticks.
    let completed: Vec<Completion> = {
        let mut next = HashMap::with_capacity(raw.len());
        let mut out = Vec::new();
        for torrent in raw {
            let transitioned =
                state.seen_complete.get(&torrent.hash) == Some(&false) && torrent.complete;
            let excluded = settings
                .completion_notification_excluded_labels
                .iter()
                .any(|label| label == &torrent.label);
            if transitioned && !excluded {
                out.push(Completion {
                    hash: torrent.hash.clone(),
                    name: torrent.name.clone(),
                    size_bytes: torrent.size_bytes,
                    base_path: torrent.base_path.clone(),
                });
            }
            next.insert(torrent.hash.clone(), torrent.complete);
        }
        // Dropping absent hashes means a removed/re-added complete torrent is
        // treated as first-seen instead of producing a stale transition.
        state.seen_complete = next;
        out
    };
    if !settings.run_on_complete.is_empty() {
        for completion in &completed {
            if let Some(program) = run_on_complete(
                &settings.run_on_complete,
                &completion.name,
                &completion.base_path,
                &completion.hash,
            ) {
                log.lines.push(PolicyLine::Hooked {
                    program,
                    hash: completion.hash.clone(),
                });
            }
        }
    }
    log.completions = completed;

    // 2. Seed goals.
    let decisions = seed_goal_decisions(
        raw,
        &settings.global_seed_goal,
        &settings.label_seed_goals,
        &state.goal_stops,
        unix_now(),
    );
    // Hashes policy acted on this tick: the mover must not touch them (in
    // particular it must never restart a seed-goal stop).
    let acted: HashSet<String> = decisions.iter().map(|d| d.hash.clone()).collect();
    if !decisions.is_empty() {
        let backend = services.backend();
        let hashes: Vec<String> = decisions.iter().map(|d| d.hash.clone()).collect();
        match settings.seed_goal_action {
            SeedGoalAction::Stop => match backend.stop(&hashes).await {
                Ok(()) => {
                    for d in decisions {
                        state.goal_stops.insert(d.hash.clone(), d.record);
                        log.lines.push(PolicyLine::SeedStopped {
                            message: d.message,
                            hash: d.hash,
                        });
                    }
                }
                Err(error) => log.lines.push(PolicyLine::SeedFailed {
                    message: format!("could not stop torrent at seed goal: {error}"),
                }),
            },
            SeedGoalAction::Remove | SeedGoalAction::RemoveData => {
                let with_data = settings.seed_goal_action == SeedGoalAction::RemoveData;
                let local = crate::settings::is_localhost(&settings.transport);
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
                // Read base paths before erasing, so data can be trashed after.
                match backend.erase(&hashes).await {
                    Ok(()) => {
                        for path in paths {
                            log.lines.push(PolicyLine::Trashed { path });
                        }
                        let verb = if with_data {
                            "removed with data"
                        } else {
                            "removed"
                        };
                        for d in decisions {
                            log.lines.push(PolicyLine::SeedRemoved {
                                message: format!("{} — {verb}", d.message),
                                hash: d.hash,
                            });
                        }
                    }
                    Err(error) => log.lines.push(PolicyLine::SeedFailed {
                        message: format!("could not remove torrent at seed goal: {error}"),
                    }),
                }
            }
        }
    }

    // 3. The complete queue policy (V3-17 / QUE-01): downloads, seeds and
    // total caps with slow-torrent exemption and force-start. Holds pause
    // (stay loaded → renders "Queued"), never stop. Seed-goal decrees win,
    // and so do in-flight moves: hashes policy just acted on, or a move task
    // stopped, are skipped.
    let mut queue_skip = acted.clone();
    queue_skip.extend(moves.lock().unwrap().busy_hashes());
    let queue = rtorrent_core::queue::decide_queues(
        raw,
        rtorrent_core::queue::QueueConfig {
            max_downloads: settings.max_active_downloads,
            max_uploads: settings.max_active_uploads,
            max_total: settings.max_active_torrents,
            slow_limit_kbs: settings.queue_slow_limit_kbs,
        },
        &mut state.queue_mem,
        &queue_skip,
    );
    if !queue.stop.is_empty() {
        let backend = services.backend();
        if backend.pause(&queue.stop).await.is_ok() {
            log.lines.push(PolicyLine::QueueStopped {
                count: queue.stop.len(),
            });
        }
    }
    if !queue.start.is_empty() {
        let backend = services.backend();
        if backend.start(&queue.start).await.is_ok() {
            log.lines.push(PolicyLine::QueueStarted {
                count: queue.start.len(),
            });
        }
    }

    // 3b. Bandwidth rules (V3-18 / QUE-04): adopt/release per the plan;
    // steady torrents cost zero daemon traffic.
    {
        use rtorrent_core::bandwidth::{plan_bandwidth, BandwidthAction, RULE_KEY};
        let names: HashMap<&str, &str> = raw
            .iter()
            .map(|t| (t.hash.as_str(), t.name.as_str()))
            .collect();
        let backend = services.backend();
        for action in plan_bandwidth(raw, &settings.bandwidth_rules) {
            match action {
                BandwidthAction::Adopt { hash, rule } => {
                    let throttle = rule.throttle_name();
                    if state.bw_state.needs_define(&throttle) {
                        if let Err(error) = backend
                            .define_named_throttle(&throttle, rule.down_kb, rule.up_kb)
                            .await
                        {
                            if state.bw_state.should_warn(&rule.id) {
                                log.lines.push(PolicyLine::BandwidthFailed {
                                    message: format!(
                                        "bandwidth rule {} ({}): could not define throttle: {error}",
                                        rule.id,
                                        rule.describe()
                                    ),
                                });
                            }
                            continue;
                        }
                    }
                    state.bw_state.clear_warned(&rule.id);
                    let one = std::slice::from_ref(&hash);
                    if backend.assign_throttle(one, Some(&throttle)).await.is_err() {
                        continue;
                    }
                    let _ = backend
                        .set_connection_limits(
                            &hash,
                            rule.peers_max,
                            rule.peers_min,
                            rule.uploads_max,
                        )
                        .await;
                    let _ = backend
                        .set_custom_metadata(&hash, &[(RULE_KEY, &rule.id)])
                        .await;
                    log.lines.push(PolicyLine::BandwidthAdopted {
                        message: format!(
                            "bandwidth rule {} ({}) now shaping {}",
                            rule.id,
                            rule.describe(),
                            names.get(hash.as_str()).copied().unwrap_or(&hash)
                        ),
                        hash,
                    });
                }
                BandwidthAction::Release { hash } => {
                    let one = std::slice::from_ref(&hash);
                    let _ = backend.assign_throttle(one, None).await;
                    let _ = backend.set_connection_limits(&hash, 0, 0, 0).await;
                    let _ = backend.set_custom_metadata(&hash, &[(RULE_KEY, "")]).await;
                    log.lines.push(PolicyLine::BandwidthReleased { hash });
                }
            }
        }
    }

    // 4. Scheduler (V3-18 / QUE-05): the weekly grid subsumes turtle's
    // single window. Pause windows halt traffic via scheduler-owned pauses
    // (mirrored into the queue's user-paused set); limit windows push global
    // rates change-gated as turtle did.
    let sched_state = schedule_state(settings);
    let turtle_active = sched_state.turtle_active();
    let sched_actions = state.sched_mem.transition(
        sched_state.is_paused(),
        raw.iter()
            .filter(|t| t.is_active)
            .map(|t| t.hash.clone())
            .chain(queue.start.iter().cloned()),
    );
    if !sched_actions.pause.is_empty() {
        let backend = services.backend();
        if backend.pause(&sched_actions.pause).await.is_ok() {
            for h in &sched_actions.pause {
                state.queue_mem.set_scheduler_held(h, true);
            }
            log.lines.push(PolicyLine::SchedPaused {
                count: sched_actions.pause.len(),
            });
        } else {
            state.sched_mem.reset();
        }
    }
    if !sched_actions.release.is_empty() {
        for h in &sched_actions.release {
            state.queue_mem.set_scheduler_held(h, false);
        }
        log.lines.push(PolicyLine::SchedResumed {
            count: sched_actions.release.len(),
        });
    }
    if !sched_state.is_paused() {
        let limits = match &sched_state {
            rtorrent_core::schedule::SchedState::Limited { down_kb, up_kb } => (*down_kb, *up_kb),
            _ => (settings.down_limit_kb, settings.up_limit_kb),
        };
        if state.applied_limits != Some(limits) {
            let backend = services.backend();
            if backend.set_throttles(limits.0, limits.1).await.is_ok() {
                state.applied_limits = Some(limits);
            }
        }
    }
    log.turtle_active = turtle_active;

    // 5. Move-on-complete (V3-14): plan from this tick's rows; each intent
    // runs detached (a cross-volume copy outlives the tick) and reports
    // through the next snapshots, which the model turns into log lines and
    // the status notice. Pruning and stale-intent settling always run; new
    // moves spawn only while the feature is configured.
    if crate::settings::is_localhost(&settings.transport) {
        let live: HashSet<String> = raw.iter().map(|t| t.hash.clone()).collect();
        {
            let mut guard = moves.lock().unwrap();
            let before = guard.journal().ops.len();
            guard.prune_unknown(&live);
            guard.prune_done();
            if guard.journal().ops.len() != before {
                let _ = guard.save();
            }
        }
        let label_paths = label_paths(settings);
        let homed = mover::homed_hashes(raw, &settings.move_rules, &label_paths);
        if !homed.is_empty() {
            let backend = services.backend();
            for hash in homed {
                let _ = backend
                    .set_custom_metadata(&hash, &[(rtorrent_core::complete::FINAL_DIR_KEY, "")])
                    .await;
            }
        }
        let configured =
            !settings.incomplete_dir.trim().is_empty() || !settings.move_rules.is_empty();
        if configured {
            let busy = moves.lock().unwrap().busy_hashes();
            let intents =
                mover::decide_moves(raw, &settings.move_rules, &label_paths, &acted, &busy);
            for intent in intents {
                // Register live handles synchronously so the next tick already
                // treats the hash as busy; the driver joins the same
                // deterministic op id.
                let now = unix_millis();
                let cancel = Arc::new(AtomicBool::new(false));
                let done = Arc::new(AtomicU64::new(0));
                moves.lock().unwrap().register(ActiveMove {
                    op_id: Journal::make_id(&intent.hash, now),
                    hash: intent.hash.clone(),
                    cancel: Arc::clone(&cancel),
                    done: Arc::clone(&done),
                    total: intent.size_bytes.max(0) as u64,
                });
                spawn_move(services, moves, settings, intent, now, cancel, done);
            }
        }
    }
    log.moves = moves.lock().unwrap().snapshot();

    log
}

/// Label paths for destination resolution: the C11 per-label defaults.
fn label_paths(settings: &Settings) -> Vec<(&str, &str)> {
    settings
        .label_defaults
        .iter()
        .map(|d| (d.label.as_str(), d.save_path.as_str()))
        .collect()
}

/// Spawn the detached task for one planned move. Terminal outcomes reach the
/// log through the snapshots the model diffs each tick (see
/// `TorrentsModel::apply_move_statuses`); the task itself only journals.
fn spawn_move(
    services: &Arc<crate::services::Services>,
    moves: &Arc<Mutex<mover::MoveStore>>,
    settings: &Settings,
    intent: MoveIntent,
    now: i64,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicU64>,
) {
    let services = Arc::clone(services);
    let moves = Arc::clone(moves);
    let policy = settings.collision_policy;
    // Handles were registered synchronously at decide time; the op id below
    // joins the same entry.
    let op_id = Journal::make_id(&intent.hash, now);
    services.runtime().spawn(async move {
        let backend = services.backend();
        let local_src = match crate::localfs::resolve(&intent.src_base) {
            Ok(p) => p,
            Err(e) => {
                record_failure(&moves, &intent, &e, now);
                return;
            }
        };
        let local_dst_dir = match crate::localfs::resolve(&intent.dst_dir) {
            Ok(p) => p,
            Err(e) => {
                record_failure(&moves, &intent, &e, now);
                return;
            }
        };
        let free = backend
            .free_diskspace(&intent.hash)
            .await
            .ok()
            .flatten()
            .or_else(|| crate::localfs::free_space(&intent.dst_dir));

        // No push channel in this shell: the driver persists on every
        // transition and the model's next snapshot picks up progress.
        let on_event: Arc<mover::MoveEventFn> = Arc::new(|_| {});
        let outcome = mover::execute_move(
            backend.as_ref(),
            &moves,
            MoveTask {
                intent,
                local_src,
                local_dst_dir,
                free,
                policy,
                now_ms: now,
            },
            cancel,
            done,
            on_event,
        )
        .await;
        // A raced duplicate owns nothing (drop our pre-registered handles);
        // every other outcome is journalled and surfaced through the
        // snapshot diff.
        if matches!(outcome, mover::MoveOutcome::Busy) {
            moves.lock().unwrap().unregister(&op_id);
        }
    });
}

/// Record a planning-time failure (untranslatable path) as a Failed journal
/// entry so the UI shows it instead of swallowing it.
fn record_failure(
    moves: &Arc<Mutex<mover::MoveStore>>,
    intent: &MoveIntent,
    error: &str,
    now: i64,
) {
    let mut moves = moves.lock().unwrap();
    let op = rtorrent_core::complete::MoveOp {
        id: Journal::make_id(&intent.hash, now),
        hash: intent.hash.clone(),
        name: intent.name.clone(),
        src: intent.src_base.clone(),
        dst: intent.dst_dir.clone(),
        state: rtorrent_core::complete::MoveState::Failed,
        error: format!("could not move {}: {error}", intent.name),
        size_bytes: intent.size_bytes,
        created_at_ms: now,
        updated_at_ms: now,
    };
    moves.journal_mut().add(op);
    // No task will run: drop the pre-registered live handles.
    moves.unregister_hash(&intent.hash);
    let _ = moves.save();
}

/// Poll-side policy memory: goal-stop records, completion flags, and the last
/// turtle limits pushed. Reset on (re)connect so a new session is judged fresh.
#[derive(Debug, Default)]
pub struct PolicyState {
    goal_stops: HashMap<String, GoalStopRecord>,
    seen_complete: HashMap<String, bool>,
    applied_limits: Option<(i64, i64)>,
    /// Queue edge memory (V3-17): manual resumes/pauses the scheduler must
    /// not fight. Reset with everything else on (re)connect.
    queue_mem: rtorrent_core::queue::QueueMemory,
    /// Bandwidth-rule session memory (V3-18): defined throttles + warned
    /// rules. Reset on (re)connect — definitions die with the daemon.
    bw_state: rtorrent_core::bandwidth::BandwidthState,
    /// Scheduler pause memory (V3-18 / QUE-05): hashes this scheduler paused
    /// on window entry. Reset on (re)connect.
    sched_mem: rtorrent_core::schedule::SchedMemory,
    /// Set by `on_reconnect`/`disconnect`, cleared by the replay stage.
    /// `Default` (false) is what the model relies on when it takes the state
    /// out for a tick and puts it back.
    needs_replay: bool,
}

impl PolicyState {
    /// Forget everything a session taught: fresh connection, fresh judgement.
    pub fn reset(&mut self) {
        self.goal_stops.clear();
        self.seen_complete.clear();
        self.queue_mem.reset();
        self.bw_state.reset();
        self.sched_mem.reset();
        // Not `applied_limits`: a reconnect re-pushes the limits the daemon
        // forgot across its restart — handled below in `disconnect`.
    }

    /// A (re)connect: clear the goal memory and completion flags, and forget
    /// which limits were pushed (so network prefs and throttle pool replay).
    /// `down_limit_kb`/`up_limit_kb` are *not* seeded: the next tick computes
    /// the turtle-effective pair and pushes it, which is the single writer.
    pub fn on_reconnect(&mut self, _settings: &Settings) {
        self.goal_stops.clear();
        self.seen_complete.clear();
        self.queue_mem.reset();
        self.bw_state.reset();
        self.sched_mem.reset();
        self.applied_limits = None;
        self.needs_replay = true;
    }

    /// A disconnect clears the pushed-limits memory so the reconnect replays
    /// the network prefs and throttle pool, which rtorrent forgets when it
    /// restarts (it forgets named throttle *definitions* too — the model's
    /// reconnect path covers those).
    pub fn disconnect(&mut self) {
        self.goal_stops.clear();
        self.seen_complete.clear();
        self.queue_mem.reset();
        self.bw_state.reset();
        self.sched_mem.reset();
        self.applied_limits = None;
        self.needs_replay = true;
    }
}

/// What one policy pass did, as data. The model turns these into log lines and
/// notifications.
#[derive(Debug, Default)]
pub struct PolicyLog {
    pub completions: Vec<Completion>,
    pub turtle_active: bool,
    pub lines: Vec<PolicyLine>,
    /// Live move-on-complete statuses (V3-14); the model diffs these against
    /// the last tick to log terminal transitions and drive the notice.
    pub moves: Vec<MoveStatus>,
}

/// One loggable policy outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyLine {
    Hooked { program: String, hash: String },
    SeedStopped { message: String, hash: String },
    SeedRemoved { message: String, hash: String },
    SeedFailed { message: String },
    QueueStopped { count: usize },
    QueueStarted { count: usize },
    SchedPaused { count: usize },
    SchedResumed { count: usize },
    BandwidthAdopted { message: String, hash: String },
    BandwidthReleased { hash: String },
    BandwidthFailed { message: String },
    Trashed { path: String },
}

/// The scheduler's current verdict for these settings, read off the local
/// wall clock. Both the model (for the `turtle_active` snapshot flag) and
/// `run_tick` (for enforcement) go through here, so display and behaviour
/// can never disagree about whether a window is open.
#[must_use]
pub fn schedule_state(settings: &Settings) -> rtorrent_core::schedule::SchedState {
    let (weekday, minute) = crate::turtle::local_day_minute();
    let now_ms = unix_millis();
    let legacy = rtorrent_core::schedule::LegacyWindow {
        enabled: settings.turtle_schedule.enabled,
        start_min: settings.turtle_schedule.start_min,
        end_min: settings.turtle_schedule.end_min,
        days: settings.turtle_schedule.days.clone(),
        down_kb: settings.turtle_down_kb,
        up_kb: settings.turtle_up_kb,
    };
    let windows: Vec<rtorrent_core::schedule::SchedWindow> = if settings.schedule.windows.is_empty()
    {
        rtorrent_core::schedule::import_legacy(&legacy)
    } else {
        settings.schedule.windows.clone()
    };
    let manual = settings
        .turtle_enabled
        .then_some((settings.turtle_down_kb, settings.turtle_up_kb));
    rtorrent_core::schedule::evaluate(
        &windows,
        settings.schedule.temp_override.as_ref(),
        manual,
        weekday,
        minute,
        now_ms,
    )
}

/// The LoadOptions a watch-folder or RSS add hands the daemon.
#[must_use]
pub fn load_options(directory: String, label: String) -> LoadOptions {
    LoadOptions {
        directory,
        label,
        start: true,
        top_of_queue: false,
        unselected_indexes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000;

    fn raw(
        hash: &str,
        complete: bool,
        is_active: bool,
        ratio_permille: i64,
        finished_at: i64,
        label: &str,
        priority: i64,
        started_at: i64,
    ) -> RawTorrent {
        RawTorrent {
            hash: hash.into(),
            complete,
            is_active,
            ratio_permille,
            finished_at,
            label: label.into(),
            priority,
            started_at,
            ..RawTorrent::default()
        }
    }

    fn downloading(hash: &str, priority: i64, started_at: i64, active: bool) -> RawTorrent {
        RawTorrent {
            hash: hash.into(),
            complete: false,
            is_active: active,
            hashing: false,
            message: String::new(),
            priority,
            started_at,
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
                torrent: raw("HASH", true, true, 2_000, NOW - 60, "", 0, 0),
                global: ratio_goal.clone(),
                overrides: vec![],
                stopped: HashMap::new(),
                should_stop: true,
            },
            Case {
                name: "time met",
                torrent: raw("HASH", true, true, 100, NOW - 10_800, "", 0, 0),
                global: time_goal.clone(),
                overrides: vec![],
                stopped: HashMap::new(),
                should_stop: true,
            },
            Case {
                name: "both configured use OR semantics",
                torrent: raw("HASH", true, true, 500, NOW - 10_800, "", 0, 0),
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
                torrent: raw("HASH", true, true, 2_500, NOW - 60, "video", 0, 0),
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
                torrent: raw("HASH", true, true, 5_000, NOW - 10_800, "archive", 0, 0),
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
                torrent: raw("HASH", false, true, 9_000, NOW - 86_400, "", 0, 0),
                global: ratio_goal.clone(),
                overrides: vec![],
                stopped: HashMap::new(),
                should_stop: false,
            },
            Case {
                name: "missing finished timestamp skips time rule",
                torrent: raw("HASH", true, true, 100, 0, "", 0, 0),
                global: time_goal,
                overrides: vec![],
                stopped: HashMap::new(),
                should_stop: false,
            },
            Case {
                name: "incomplete torrent",
                torrent: raw("HASH", false, true, 3_000, NOW - 10_800, "", 0, 0),
                global: ratio_goal.clone(),
                overrides: vec![],
                stopped: HashMap::new(),
                should_stop: false,
            },
            Case {
                name: "already inactive torrent",
                torrent: raw("HASH", true, false, 3_000, NOW - 10_800, "", 0, 0),
                global: ratio_goal.clone(),
                overrides: vec![],
                stopped: HashMap::new(),
                should_stop: false,
            },
            Case {
                name: "manually restarted goal-stop at same ratio",
                torrent: raw("HASH", true, true, 2_000, NOW - 10_800, "", 0, 0),
                global: ratio_goal.clone(),
                overrides: vec![],
                stopped: stopped_at_two,
                should_stop: false,
            },
            Case {
                name: "restarted goal-stop after ratio advances",
                torrent: raw("HASH", true, true, 2_001, NOW - 10_800, "", 0, 0),
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

        let duplicate = raw("HASH", true, true, 2_000, NOW - 60, "", 0, 0);
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
    fn queue_stage_holds_with_pause_and_promotes() {
        // The driver lives in rtorrent-core now; this pins the stage wiring:
        // same rows through the shared decider with a downloads cap.
        use rtorrent_core::queue::{decide_queues, QueueConfig, QueueMemory};
        let rows = vec![
            downloading("low", 0, 100, true),   // active but below the line
            downloading("mid", 1, 200, true),   // active, inside
            downloading("high", 2, 300, false), // stopped, inside
        ];
        let cfg = QueueConfig {
            max_downloads: 2,
            max_uploads: 0,
            max_total: 0,
            slow_limit_kbs: 0,
        };
        let mut mem = QueueMemory::default();
        let actions = decide_queues(&rows, cfg, &mut mem, &HashSet::new());
        // Sorted: high(2), mid(1), low(0). Window of 2: high, mid.
        assert_eq!(actions.start, vec!["high".to_string()]);
        assert_eq!(actions.stop, vec!["low".to_string()]);
    }

    #[test]
    fn queue_stage_ignores_unmanaged_rows() {
        use rtorrent_core::queue::{decide_queues, QueueConfig, QueueMemory};
        let mut checking = downloading("checking", 3, 50, false);
        checking.hashing = true;
        let mut errored = downloading("errored", 3, 60, true);
        errored.message = "tracker timeout".into();
        let mut done = downloading("done", 3, 70, true);
        done.complete = true;
        let active = downloading("active", 0, 80, true);
        let cfg = QueueConfig {
            max_downloads: 1,
            max_uploads: 0,
            max_total: 0,
            slow_limit_kbs: 0,
        };
        let mut mem = QueueMemory::default();
        let actions = decide_queues(
            &[checking, errored, done, active],
            cfg,
            &mut mem,
            &HashSet::new(),
        );
        assert!(actions.start.is_empty());
        assert!(actions.stop.is_empty(), "{actions:?}");
    }

    #[test]
    fn queue_stage_disabled_at_zero_or_below() {
        use rtorrent_core::queue::{decide_queues, QueueConfig, QueueMemory};
        let cfg = QueueConfig {
            max_downloads: 0,
            max_uploads: -3,
            max_total: 0,
            slow_limit_kbs: 0,
        };
        let mut mem = QueueMemory::default();
        let actions = decide_queues(
            &[downloading("a", 0, 1, true)],
            cfg,
            &mut mem,
            &HashSet::new(),
        );
        assert!(actions.start.is_empty() && actions.stop.is_empty());
    }

    #[test]
    fn queue_stage_tiebreaks_on_earliest_start_with_unknown_last() {
        use rtorrent_core::queue::{decide_queues, QueueConfig, QueueMemory};
        let a = downloading("a", 1, 100, false);
        let b = downloading("b", 1, 50, false);
        let unknown = downloading("u", 1, 0, false);
        let cfg = QueueConfig {
            max_downloads: 2,
            max_uploads: 0,
            max_total: 0,
            slow_limit_kbs: 0,
        };
        let mut mem = QueueMemory::default();
        let actions = decide_queues(&[a, b, unknown], cfg, &mut mem, &HashSet::new());
        // Order: b(50), a(100), u(MAX). Window of 2 starts b and a.
        assert_eq!(actions.start, vec!["b".to_string(), "a".to_string()]);
        assert!(actions.stop.is_empty());
    }

    #[test]
    fn hook_template_builds_nothing_from_blank() {
        assert!(build("", "n", "p", "h").is_none());
        assert!(build("  ", "n", "p", "h").is_none());
    }

    #[test]
    fn hook_tokens_are_substituted_per_argument() {
        let (program, args) = build("notify %N %F %H", "My File", "/srv/My File", "ABC").unwrap();
        assert_eq!(program, "notify");
        assert_eq!(args, vec!["My File", "/srv/My File", "ABC"]);
    }

    #[test]
    fn hook_tokens_embedded_in_a_token_are_replaced() {
        let (program, args) = build("/bin/log --msg=done:%H", "n", "p", "DEADBEEF").unwrap();
        assert_eq!(program, "/bin/log");
        assert_eq!(args, vec!["--msg=done:DEADBEEF"]);
    }

    #[test]
    fn policy_state_reset_forgets_but_replays_limits() {
        let mut state = PolicyState::default();
        state.goal_stops.insert(
            "H".into(),
            GoalStopRecord {
                ratio_permille: 1,
                finished_at: 2,
            },
        );
        state.seen_complete.insert("H".into(), true);
        state.applied_limits = Some((1, 2));
        state.reset();
        assert!(state.goal_stops.is_empty());
        assert!(state.seen_complete.is_empty());
        // Kept: a reconnect must re-push what the daemon forgot.
        assert_eq!(state.applied_limits, Some((1, 2)));
        state.disconnect();
        assert_eq!(state.applied_limits, None);
    }

    /// A complete fixture with recorded intent moves home on the tick: the
    /// daemon is repointed, the intent consumed, and the journal says Done.
    /// The fs side moves real bytes (a temp incomplete dir), so this covers
    /// the copy path, not just the no-op.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn completed_torrent_with_final_dir_moves_home() {
        use rtorrent_core::complete::{MoveState, FINAL_DIR_KEY};
        use rtorrent_core::mover::MoveStore;

        let dir = tempfile::tempdir().unwrap();
        let incomplete = dir.path().join("incomplete");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&incomplete).unwrap();

        let services = Arc::new(crate::services::Services::mock().expect("runtime"));
        let mut next = services.settings();
        next.incomplete_dir = incomplete.to_string_lossy().into_owned();
        services.update_settings(next);

        let backend = services.backend();
        let rows = backend.list_snapshot().await.unwrap();
        let picked = rows
            .iter()
            .find(|t| t.complete)
            .expect("a complete fixture")
            .clone();
        backend
            .set_directory(&picked.hash, &incomplete.to_string_lossy())
            .await
            .unwrap();
        std::fs::write(incomplete.join(&picked.name), b"moved bytes").unwrap();
        backend
            .set_custom_metadata(&picked.hash, &[(FINAL_DIR_KEY, &home.to_string_lossy())])
            .await
            .unwrap();

        let settings = services.settings();
        let raw = backend.list_snapshot().await.unwrap();
        let moves = Arc::new(Mutex::new(MoveStore::load(dir.path().join("j.json"))));
        let mut state = PolicyState::default();
        let log = run_tick(&services, &settings, &raw, &mut state, &moves).await;
        // Nothing to hook or stop on a fresh session with default goals.
        assert!(log.completions.is_empty());

        // The move runs detached; settle until the journal says Done.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let done = moves
                .lock()
                .unwrap()
                .journal()
                .ops
                .iter()
                .any(|op| op.hash == picked.hash && op.state == MoveState::Done);
            if done {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the move did not finish"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        let rows = backend.list_snapshot().await.unwrap();
        let row = rows.iter().find(|t| t.hash == picked.hash).unwrap();
        assert_eq!(row.directory, home.to_string_lossy());
        assert!(row.final_dir.is_empty(), "intent consumed");
        assert_eq!(
            std::fs::read(home.join(&picked.name)).unwrap(),
            b"moved bytes"
        );
        assert!(!incomplete.join(&picked.name).exists(), "source cleaned up");
    }

    /// A pause window covering the whole week halts every active torrent on
    /// entry, stays quiet while held, and releases on exit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pause_window_holds_and_releases() {
        use rtorrent_core::schedule::SchedWindow;

        let dir = tempfile::tempdir().unwrap();
        let services = Arc::new(crate::services::Services::mock().expect("runtime"));
        let mut next = services.settings();
        next.schedule.windows = vec![SchedWindow {
            days: Vec::new(),
            start_min: 0,
            end_min: 1440,
            pause: true,
            down_kb: 0,
            up_kb: 0,
        }];
        services.update_settings(next);

        let moves = Arc::new(Mutex::new(rtorrent_core::mover::MoveStore::load(
            dir.path().join("j.json"),
        )));
        let mut state = PolicyState::default();
        async fn tick_once(
            services: &Arc<crate::services::Services>,
            state: &mut PolicyState,
            moves: &Arc<Mutex<rtorrent_core::mover::MoveStore>>,
        ) -> PolicyLog {
            let settings = services.settings();
            let backend = services.backend();
            let raw = backend.list_snapshot().await.unwrap();
            run_tick(services, &settings, &raw, state, moves).await
        }
        let log = tick_once(&services, &mut state, &moves).await;
        assert!(
            log.lines
                .iter()
                .any(|line| matches!(line, PolicyLine::SchedPaused { .. })),
            "entry pauses: {log:?}"
        );
        let paused: Vec<_> = services
            .backend()
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .filter(|t| t.is_active)
            .collect();
        assert!(paused.is_empty(), "nothing transfers inside the window");

        // Second tick: steady, no repeat lines.
        let log = tick_once(&services, &mut state, &moves).await;
        assert!(
            log.lines.iter().all(|line| !matches!(
                line,
                PolicyLine::SchedPaused { .. } | PolicyLine::SchedResumed { .. }
            )),
            "a held window is silent"
        );

        // Clearing the grid releases: the exit is logged.
        let mut next = services.settings();
        next.schedule.windows.clear();
        services.update_settings(next);
        let log = tick_once(&services, &mut state, &moves).await;
        assert!(
            log.lines
                .iter()
                .any(|line| matches!(line, PolicyLine::SchedResumed { .. })),
            "exit releases: {log:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bandwidth_rule_adopts_a_matching_torrent() {
        use rtorrent_core::bandwidth::BandwidthRule;

        let dir = tempfile::tempdir().unwrap();
        let services = Arc::new(crate::services::Services::mock().expect("runtime"));
        let mut next = services.settings();
        next.bandwidth_rules = vec![BandwidthRule::label_rule("iso-cap", "iso", 512, 128)];
        services.update_settings(next);

        let backend = services.backend();
        let rows = backend.list_snapshot().await.unwrap();
        let picked = rows
            .iter()
            .find(|t| t.label == "iso" && t.throttle_name.is_empty())
            .expect("a clean iso torrent")
            .clone();
        assert!(picked.throttle_rule.is_empty());

        let settings = services.settings();
        let raw = backend.list_snapshot().await.unwrap();
        let moves = Arc::new(Mutex::new(rtorrent_core::mover::MoveStore::load(
            dir.path().join("j.json"),
        )));
        let mut state = PolicyState::default();
        let log = run_tick(&services, &settings, &raw, &mut state, &moves).await;

        assert!(
            log.lines.iter().any(|line| matches!(
                line,
                PolicyLine::BandwidthAdopted { hash, .. } if hash == &picked.hash
            )),
            "adoption is logged: {log:?}"
        );
        let row = backend
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.hash == picked.hash)
            .unwrap();
        assert_eq!(row.throttle_name, "rule_iso-cap");
        assert_eq!(row.throttle_rule, "iso-cap");

        // Second tick: steady, no more daemon writes and no new log lines.
        let raw = backend.list_snapshot().await.unwrap();
        let log = run_tick(&services, &settings, &raw, &mut state, &moves).await;
        assert!(
            log.lines.iter().all(|line| !matches!(
                line,
                PolicyLine::BandwidthAdopted { .. } | PolicyLine::BandwidthReleased { .. }
            )),
            "steady state is silent"
        );
    }
}
