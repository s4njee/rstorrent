//! Move-on-complete execution for the web poller (V3-14).
//!
//! Same driver as the desktop poller ([`rtorrent_core::mover`]), adapted to
//! server idioms: log lines instead of toasts, no push events (browsers poll
//! `GET /api/moves`), `tokio::spawn` for move tasks. Moves only run when the
//! server is co-located with the daemon (unix-socket transport) — a remote
//! daemon's files are not ours to touch.

use rtorrent_core::complete::Journal;
use rtorrent_core::mover::{self, ActiveMove, MoveIntent, MoveOutcome, MoveTask};
use rtorrent_core::rtorrent::RawTorrent;
use rtorrent_core::types::Transport;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rtorrent_core::types::LogLevel;

use crate::state::AppState;

/// `move-journal.json` beside the config file, or the working directory when
/// the server runs without one (then moves still work, the journal just does
/// not survive a directory change).
pub fn journal_path(config_path: Option<&Path>) -> PathBuf {
    config_path
        .and_then(Path::parent)
        .map(|dir| dir.join("move-journal.json"))
        .unwrap_or_else(|| PathBuf::from("move-journal.json"))
}

/// The server reaches the daemon's files exactly when it talks over a unix
/// socket (same box). Mirrors the delete-data gate in `cmd.rs`.
pub fn is_colocated(state: &AppState) -> bool {
    matches!(state.config.transport, Transport::UnixSocket { .. })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Whether the remote-daemon warning below already fired this process.
static WARNED_REMOTE: AtomicBool = AtomicBool::new(false);

/// Plan and spawn every due move for one successful tick, then settle stale
/// intent. Pruning and stale-intent settling always run; new moves spawn only
/// while the feature is configured (so disabling it parks automation but
/// keeps self-healing).
pub async fn run_due_moves(state: &Arc<AppState>, raw: &[RawTorrent]) {
    if !is_colocated(state) {
        if (!state.config.incomplete_dir.trim().is_empty() || !state.config.move_rules.is_empty())
            && !WARNED_REMOTE.swap(true, Ordering::Relaxed)
        {
            state.log(
                LogLevel::Warn,
                "move-on-complete is configured but the daemon is remote — \
                 moves need the server co-located with the daemon",
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

    let homed = mover::homed_hashes(raw, &state.config.move_rules, &[]);
    if !homed.is_empty() {
        let backend = state.backend.as_ref();
        for hash in homed {
            let _ = backend
                .set_custom_metadata(&hash, &[(rtorrent_core::complete::FINAL_DIR_KEY, "")])
                .await;
        }
    }

    if state.config.incomplete_dir.trim().is_empty() && state.config.move_rules.is_empty() {
        return;
    }
    let busy = state.moves.lock().unwrap().busy_hashes();
    let intents = mover::decide_moves(raw, &state.config.move_rules, &[], &HashSet::new(), &busy);
    for intent in intents {
        // Register live handles synchronously so the next tick already treats
        // the hash as busy; the driver joins the same deterministic op id.
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
        spawn_move(state, intent, now, cancel, done);
    }
}

/// Spawn the detached task for one planned move.
fn spawn_move(
    state: &Arc<AppState>,
    intent: MoveIntent,
    now: i64,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicU64>,
) {
    let state = Arc::clone(state);
    tokio::spawn(async move {
        let local_src = PathBuf::from(&intent.src_base);
        let local_dst_dir = PathBuf::from(&intent.dst_dir);
        let free = state
            .backend
            .free_diskspace(&intent.hash)
            .await
            .ok()
            .flatten()
            .or_else(|| crate::disk::disk_usage(&intent.dst_dir).map(|(free, _)| free));

        // Handles were registered synchronously at decide time; the op id
        // below joins the same entry.
        let op_id = Journal::make_id(&intent.hash, now);

        // No push channel to browsers: the driver persists on every
        // transition, and `GET /api/moves` serves progress from the counter.
        let on_event: Arc<mover::MoveEventFn> = Arc::new(|_| {});

        let policy = state.config.collision_policy;
        let outcome = mover::execute_move(
            state.backend.as_ref(),
            &state.moves,
            MoveTask {
                intent: intent.clone(),
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

        match outcome {
            MoveOutcome::Done { daemon_base } => {
                state.log(
                    LogLevel::Info,
                    format!("moved {} to {}", intent.name, daemon_base),
                    Some(intent.hash),
                );
            }
            MoveOutcome::Cancelled => {
                state.log(
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
                state.log(LogLevel::Error, msg, Some(intent.hash));
            }
        }
        state.repoll.notify_one();
    });
}

/// Recover the journal once per process start, after the first successful
/// tick (so `live` is real daemon truth).
pub async fn resume_once(state: &Arc<AppState>, raw: &[RawTorrent]) {
    if state
        .moves_resumed
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    if state.moves.lock().unwrap().journal().resumable().is_empty() {
        return;
    }
    let live: HashSet<String> = raw.iter().map(|t| t.hash.clone()).collect();
    let on_event: Arc<mover::MoveEventFn> = Arc::new(|_| {});
    mover::resume_moves(
        state.backend.as_ref(),
        &state.moves,
        &live,
        &|daemon_path| Path::new(daemon_path).exists(),
        now_ms(),
        on_event,
    )
    .await;
    state.log(LogLevel::Info, "move journal recovered", None);
    state.repoll.notify_one();
}
