//! Move-on-complete execution for the desktop poller (V3-14).
//!
//! Each successful tick plans moves from daemon truth
//! ([`rtorrent_core::mover::decide_moves`]) and spawns one detached task per
//! intent. The shared [`rtorrent_core::mover::MoveStore`] (journal file beside
//! `settings.json`) keeps tasks, retries and crash-resume coherent; this
//! module adds scheduling, path translation, logging and the `moves://update`
//! event the status pill listens to.
//!
//! Moves only run against a local daemon: the files must be on this machine.
//! A remote daemon with the feature configured gets one honest log line per
//! session, not a per-tick failure.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};

use rtorrent_core::complete::Journal;
use rtorrent_core::mover::{self, ActiveMove, MoveEvent, MoveIntent, MoveOutcome, MoveTask};
use rtorrent_core::rtorrent::RawTorrent;

use crate::ipc::LogLevel;
use crate::state::AppState;

/// `move-journal.json` lives next to the settings file, like `stats.json`.
pub fn journal_path(settings_path: &std::path::Path) -> PathBuf {
    settings_path
        .parent()
        .map(|p| p.join("move-journal.json"))
        .unwrap_or_else(|| PathBuf::from("move-journal.json"))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Label paths for destination resolution: the C11 per-label defaults.
fn label_paths(settings: &crate::ipc::Settings) -> Vec<(&str, &str)> {
    settings
        .label_defaults
        .iter()
        .map(|d| (d.label.as_str(), d.save_path.as_str()))
        .collect()
}

/// Whether the remote-daemon warning below already fired this process.
static WARNED_REMOTE: AtomicBool = AtomicBool::new(false);

/// Plan and spawn every due move for one successful tick, then settle stale
/// intent. `skip` holds hashes policy already acted on this tick (seed-goal
/// stops/removes) so the mover never resurrects them.
pub async fn run_due_moves(
    app: &AppHandle,
    state: &Arc<AppState>,
    raw: &[RawTorrent],
    skip: &HashSet<String>,
) {
    let settings = state.settings();
    if !crate::settings::is_localhost(&settings.transport) {
        if (!settings.incomplete_dir.trim().is_empty() || !settings.move_rules.is_empty())
            && !WARNED_REMOTE.swap(true, Ordering::Relaxed)
        {
            state.log(
                app,
                LogLevel::Warn,
                "move-on-complete is configured but the daemon is remote — \
                 moves need the daemon's files on this machine",
                None,
            );
        }
        return;
    }
    let live: HashSet<String> = raw.iter().map(|t| t.hash.clone()).collect();
    {
        let mut moves = state.moves.lock().unwrap();
        let before = moves.journal().ops.len();
        moves.prune_unknown(&live);
        moves.prune_done();
        if moves.journal().ops.len() != before {
            let _ = moves.save();
        }
    }

    // Stale intent self-heals: complete torrents already home lose their
    // `final_dir` so a later rule change cannot surprise-move them.
    let homed = mover::homed_hashes(raw, &settings.move_rules, &label_paths(&settings));
    if !homed.is_empty() {
        let backend = state.backend();
        let app = app.clone();
        let state = Arc::clone(state);
        tauri::async_runtime::spawn(async move {
            for hash in homed {
                let _ = backend
                    .set_custom_metadata(&hash, &[(rtorrent_core::complete::FINAL_DIR_KEY, "")])
                    .await;
            }
            state.repoll.notify_one();
            let _ = app;
        });
    }

    let busy = state.moves.lock().unwrap().busy_hashes();
    // New moves spawn only while the feature is configured; pruning and
    // stale-intent settling above run regardless so disabling the feature
    // parks automation but keeps self-healing.
    if settings.incomplete_dir.trim().is_empty() && settings.move_rules.is_empty() {
        return;
    }
    let intents = mover::decide_moves(
        raw,
        &settings.move_rules,
        &label_paths(&settings),
        skip,
        &busy,
    );
    for intent in intents {
        // Register live handles *synchronously* so the next tick (a second
        // away) already treats the hash as busy — the task itself starts a
        // moment later. The op id is deterministic off (hash, now), so the
        // driver joins the same entry.
        let now = now_ms();
        let cancel = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicU64::new(0));
        state.moves.lock().unwrap().register(ActiveMove {
            op_id: Journal::make_id(&intent.hash, now),
            hash: intent.hash.clone(),
            cancel: Arc::clone(&cancel),
            done: Arc::clone(&done),
            total: intent.size_bytes.max(0) as u64,
        });
        spawn_move(app, state, intent, now, cancel, done);
    }
}

/// Spawn the detached task for one planned move.
fn spawn_move(
    app: &AppHandle,
    state: &Arc<AppState>,
    intent: MoveIntent,
    now: i64,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicU64>,
) {
    let app = app.clone();
    let state = Arc::clone(state);
    tauri::async_runtime::spawn(async move {
        let backend = state.backend();
        let settings = state.settings();

        // Daemon → local translation first; an untranslatable path fails the
        // op visibly instead of panicking the task.
        let local_src = match crate::localfs::resolve(&intent.src_base) {
            Ok(p) => p,
            Err(e) => {
                record_failure(&state, &app, &intent, &e, now);
                return;
            }
        };
        let local_dst_dir = match crate::localfs::resolve(&intent.dst_dir) {
            Ok(p) => p,
            Err(e) => {
                record_failure(&state, &app, &intent, &e, now);
                return;
            }
        };
        // Preflight figure: the daemon's own number preferred, local statvfs
        // as fallback (matching the status-bar readout's posture).
        let free = backend
            .free_diskspace(&intent.hash)
            .await
            .ok()
            .flatten()
            .or_else(|| crate::localfs::free_space(&intent.dst_dir));

        // Handles were registered synchronously at decide time (see
        // `run_due_moves`); the driver joins the same deterministic op id.
        let op_id = Journal::make_id(&intent.hash, now);

        let last_emit = Arc::new(Mutex::new(HashMap::<String, Instant>::new()));
        let emit_app = app.clone();
        let emit_op_id = op_id.clone();
        let on_event: Arc<mover::MoveEventFn> = Arc::new(move |event| match event {
            MoveEvent::StateChanged => {
                let _ = emit_app.emit("moves://update", ());
            }
            MoveEvent::Progress { .. } => {
                // Progress fires per chunk; the pill polls the snapshot, so
                // the event only needs to beat the poll interval, not match it.
                let mut last = last_emit.lock().unwrap();
                let now = Instant::now();
                if last.get(&emit_op_id).map_or(true, |t| {
                    now.duration_since(*t) > Duration::from_millis(750)
                }) {
                    last.insert(emit_op_id.clone(), now);
                    let _ = emit_app.emit("moves://update", ());
                }
            }
        });

        let outcome = mover::execute_move(
            backend.as_ref(),
            &state.moves,
            MoveTask {
                intent: intent.clone(),
                local_src,
                local_dst_dir,
                free,
                policy: settings.collision_policy,
                now_ms: now,
            },
            cancel,
            done,
            on_event,
        )
        .await;

        let _ = app.emit("moves://update", ());
        match outcome {
            MoveOutcome::Done { daemon_base } => {
                state.log(
                    &app,
                    LogLevel::Info,
                    format!("moved {} to {}", intent.name, daemon_base),
                    Some(intent.hash),
                );
            }
            MoveOutcome::Cancelled => {
                state.log(
                    &app,
                    LogLevel::Info,
                    format!("move of {} cancelled — resumed in place", intent.name),
                    Some(intent.hash),
                );
            }
            MoveOutcome::Busy => {
                // A duplicate call lost the race; drop our pre-registered
                // handles — the live task owns the log.
                state.moves.lock().unwrap().unregister(&op_id);
            }
            MoveOutcome::Failed(msg) => {
                state.log(&app, LogLevel::Error, msg, Some(intent.hash));
            }
        }
        state.repoll.notify_one();
    });
}

/// Record a planning-time failure (untranslatable path) as a Failed journal
/// entry so the UI shows it instead of swallowing it.
fn record_failure(
    state: &Arc<AppState>,
    app: &AppHandle,
    intent: &MoveIntent,
    error: &str,
    now: i64,
) {
    let msg = format!("could not move {}: {error}", intent.name);
    {
        let mut moves = state.moves.lock().unwrap();
        let op = rtorrent_core::complete::MoveOp {
            id: Journal::make_id(&intent.hash, now),
            hash: intent.hash.clone(),
            name: intent.name.clone(),
            src: intent.src_base.clone(),
            dst: intent.dst_dir.clone(),
            state: rtorrent_core::complete::MoveState::Failed,
            error: msg.clone(),
            size_bytes: intent.size_bytes,
            created_at_ms: now,
            updated_at_ms: now,
        };
        moves.journal_mut().add(op);
        // No task will run: drop the pre-registered live handles.
        moves.unregister_hash(&intent.hash);
        let _ = moves.save();
    }
    state.log(app, LogLevel::Error, msg, Some(intent.hash.clone()));
    let _ = app.emit("moves://update", ());
}

/// Recover the journal once per process start, after the first successful
/// tick (so the backend is connected and `live` is real daemon truth).
pub async fn resume_once(app: &AppHandle, state: &Arc<AppState>, raw: &[RawTorrent]) {
    let live: HashSet<String> = raw.iter().map(|t| t.hash.clone()).collect();
    if state.moves.lock().unwrap().journal().resumable().is_empty() {
        return;
    }
    let backend = state.backend();
    let emit_app = app.clone();
    let on_event: Arc<mover::MoveEventFn> = Arc::new(move |_| {
        let _ = emit_app.emit("moves://update", ());
    });
    mover::resume_moves(
        backend.as_ref(),
        &state.moves,
        &live,
        &|daemon_path| {
            crate::localfs::resolve(daemon_path)
                .map(|p| p.exists())
                .unwrap_or(false)
        },
        now_ms(),
        on_event,
    )
    .await;
    state.log(app, LogLevel::Info, "move journal recovered", None);
    state.repoll.notify_one();
}
