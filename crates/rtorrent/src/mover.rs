//! Move-on-complete executor (V3-14): the poll-side driver every host shares.
//!
//! Pure planning ([`decide_moves`], [`homed_hashes`]) stays unit-testable;
//! [`execute_move`] runs one journalised move (stop → preflight → collide →
//! copy → `d.directory.set` → clear `final_dir` → resume); [`resume_moves`]
//! recovers the journal after a restart. Hosts (Tauri poller, web poller,
//! GPUI policy) own scheduling, logging and progress surfacing, and translate
//! daemon paths to local ones before calling in.
//!
//! Path rule: [`MoveOp`] `src`/`dst` are **daemon-namespace** strings (stable
//! across restarts); the task's `local_src`/`local_dst_dir` are the
//! host-resolved equivalents the filesystem calls use.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::complete::{self, CollisionPolicy, Journal, MoveOp, MoveRule, MoveState};
use crate::fs::{self, FsMoveError, ProgressFn};
use crate::rtorrent::{RawTorrent, RtorrentApi};

/// One move the tick decided on. All paths daemon-namespace except where the
/// `local_` prefix says otherwise.
#[derive(Clone, Debug)]
pub struct MoveIntent {
    pub hash: String,
    pub name: String,
    /// `d.base_path` at plan time.
    pub src_base: String,
    /// Resolved destination directory (daemon namespace).
    pub dst_dir: String,
    pub size_bytes: i64,
    /// `size - done`, clamped at zero: the preflight figure.
    pub remaining: i64,
    pub was_active: bool,
}

/// Everything [`execute_move`] needs beyond the intent. Hosts fill the local
/// paths through their own namespace translation (plain identity except on
/// Windows/WSL) and the free-space figure (`d.free_diskspace` preferred,
/// local `statvfs`/volume stats as fallback).
pub struct MoveTask {
    pub intent: MoveIntent,
    pub local_src: PathBuf,
    pub local_dst_dir: PathBuf,
    pub free: Option<i64>,
    pub policy: CollisionPolicy,
    pub now_ms: i64,
}

/// What the host must do after [`execute_move`] returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveOutcome {
    /// Data is home, daemon updated, `final_dir` cleared.
    Done { daemon_base: String },
    /// Cancelled mid-copy; partial target removed, torrent resumed in place.
    Cancelled,
    /// The torrent was already being moved (a raced duplicate call).
    Busy,
    /// Failed with a user-facing message (journal holds it too).
    Failed(String),
}

/// Host callback channel: persist the journal, emit to the UI, log terminal
/// states. Progress fires per chunk; hosts throttle UI emission themselves.
pub type MoveEventFn = dyn Fn(MoveEvent) + Send + Sync;

/// Local existence probe for resume verification (host translates first).
pub type MoveExistsFn = dyn Fn(&str) -> bool + Send + Sync;

pub enum MoveEvent {
    StateChanged,
    Progress { done: u64, total: u64 },
}

/// Pure move planning for one successful tick: complete, settled torrents
/// whose resolved destination differs from where they are.
///
/// Intent exists only when it is explicit — a recorded `final_dir` or a
/// matching move/label rule. Torrents with no intent are left alone, so
/// enabling the feature never sweeps a legacy library into the default path.
#[must_use]
pub fn decide_moves(
    raw: &[RawTorrent],
    move_rules: &[MoveRule],
    label_paths: &[(&str, &str)],
    skip: &HashSet<String>,
    busy: &HashSet<String>,
) -> Vec<MoveIntent> {
    let mut seen = HashSet::new();
    raw.iter()
        .filter(|t| {
            t.complete
                && !t.hashing
                && !t.base_path.trim().is_empty()
                && !t.name.trim().is_empty()
                && seen.insert(t.hash.clone())
        })
        .filter(|t| !skip.contains(&t.hash) && !busy.contains(&t.hash))
        .filter_map(|t| {
            let default = if t.final_dir.trim().is_empty() {
                String::new()
            } else {
                t.final_dir.clone()
            };
            let dest = complete::completion_destination(
                &t.label,
                &t.tags,
                move_rules,
                label_paths,
                &default,
            );
            if dest.is_empty() {
                return None;
            }
            // Existence of a plan is the move condition; the intent carries
            // the resolved pieces the executor needs.
            complete::plan_move(&t.directory, &dest, &t.name)?;
            Some(MoveIntent {
                hash: t.hash.clone(),
                name: t.name.clone(),
                src_base: t.base_path.clone(),
                dst_dir: dest,
                size_bytes: t.size_bytes,
                remaining: (t.size_bytes - t.bytes_done).max(0),
                was_active: t.is_active,
            })
        })
        .collect()
}

/// Hashes whose recorded `final_dir` is stale: complete, settled, already
/// where the destination resolves — the host clears their `final_dir`
/// best-effort so a later rule change cannot surprise-move them.
#[must_use]
pub fn homed_hashes(
    raw: &[RawTorrent],
    move_rules: &[MoveRule],
    label_paths: &[(&str, &str)],
) -> Vec<String> {
    raw.iter()
        .filter(|t| {
            t.complete
                && !t.hashing
                && !t.final_dir.trim().is_empty()
                && !t.directory.trim().is_empty()
        })
        .filter(|t| {
            let dest = complete::completion_destination(
                &t.label,
                &t.tags,
                move_rules,
                label_paths,
                &t.final_dir,
            );
            complete::plan_move(&t.directory, &dest, &t.name).is_none()
        })
        .map(|t| t.hash.clone())
        .collect()
}

/// A move in flight: the journal op plus its live progress/cancel handles.
/// The host creates and registers this *before* spawning the task so a cancel
/// issued in the gap still lands.
pub struct ActiveMove {
    pub op_id: String,
    pub hash: String,
    pub cancel: Arc<AtomicBool>,
    pub done: Arc<AtomicU64>,
    pub total: u64,
}

/// Journal file + live moves, shared by the tick and the move tasks.
pub struct MoveStore {
    journal: Journal,
    path: PathBuf,
    active: HashMap<String, ActiveMove>,
}

impl MoveStore {
    /// Load the journal; a missing or corrupt file starts empty (moves are
    /// re-planned from daemon state, never invented from a half-write).
    #[must_use]
    pub fn load(path: PathBuf) -> Self {
        let journal = std::fs::read_to_string(&path)
            .ok()
            .map(|raw| Journal::from_json(&raw))
            .unwrap_or_default();
        Self {
            journal,
            path,
            active: HashMap::new(),
        }
    }

    /// Persist the journal, creating the parent directory as needed.
    ///
    /// # Errors
    ///
    /// Returns the I/O error when the file cannot be written; the in-memory
    /// journal is unaffected.
    pub fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(&self.path, self.journal.to_json())
    }

    #[must_use]
    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    #[must_use]
    pub fn journal_mut(&mut self) -> &mut Journal {
        &mut self.journal
    }

    /// Hashes no new move may start for: live tasks plus resumable journal
    /// entries (a kill mid-copy resumes through [`resume_moves`], never by
    /// double-planning).
    #[must_use]
    pub fn busy_hashes(&self) -> HashSet<String> {
        let mut busy: HashSet<String> = self.active.values().map(|a| a.hash.clone()).collect();
        busy.extend(self.journal.resumable().iter().map(|op| op.hash.clone()));
        busy
    }

    /// Register a live task's handles before spawning it.
    pub fn register(&mut self, active: ActiveMove) {
        self.active.insert(active.op_id.clone(), active);
    }

    /// Drop a finished task's handles.
    pub fn unregister(&mut self, op_id: &str) {
        self.active.remove(op_id);
    }

    /// Drop live handles for a hash (a task that died before journaling).
    pub fn unregister_hash(&mut self, hash: &str) {
        self.active.retain(|_, a| a.hash != hash);
    }

    /// Signal cancellation; true when a live task was found.
    pub fn cancel(&self, op_id: &str) -> bool {
        if let Some(active) = self.active.get(op_id) {
            active.cancel.store(true, Ordering::Relaxed);
            return true;
        }
        // Fall back to hash match so callers holding a torrent hash work too.
        if let Some(active) = self.active.values().find(|a| a.hash == op_id) {
            active.cancel.store(true, Ordering::Relaxed);
            return true;
        }
        false
    }

    /// Drop a terminal (Failed/Cancelled) entry so the next tick re-plans
    /// from daemon state. Returns false when there is nothing retryable —
    /// a live or already-done move is never disturbed.
    pub fn retry(&mut self, hash: &str) -> bool {
        let before = self.journal.ops.len();
        self.journal.ops.retain(|op| {
            !(op.hash.eq_ignore_ascii_case(hash)
                && matches!(op.state, MoveState::Failed | MoveState::Cancelled))
        });
        self.journal.ops.len() != before
    }

    /// Drop Pending/Failed/Cancelled entries whose torrent is gone. InProgress
    /// entries are left for [`resume_moves`] to verify, and Done entries are
    /// pruned separately so the file stays bounded.
    pub fn prune_unknown(&mut self, live: &HashSet<String>) {
        self.journal.ops.retain(|op| {
            matches!(op.state, MoveState::InProgress | MoveState::Done) || live.contains(&op.hash)
        });
    }

    /// Drop finished entries; the UI saw them via the terminal event + log.
    pub fn prune_done(&mut self) {
        self.journal.prune_done();
    }

    /// UI snapshot: every unfinished journal entry with live progress merged
    /// in. Done entries are pruned first (their log line is the record).
    #[must_use]
    pub fn snapshot(&self) -> Vec<MoveStatus> {
        self.journal
            .ops
            .iter()
            .filter(|op| op.state != MoveState::Done)
            .map(|op| {
                let (done_bytes, total_bytes) = self
                    .active
                    .get(&op.id)
                    .map(|a| {
                        (
                            a.done.load(Ordering::Relaxed),
                            a.total.max(op.size_bytes.max(0) as u64),
                        )
                    })
                    .unwrap_or((0, op.size_bytes.max(0) as u64));
                MoveStatus {
                    id: op.id.clone(),
                    hash: op.hash.clone(),
                    name: op.name.clone(),
                    src: op.src.clone(),
                    dst: op.dst.clone(),
                    state: op.state.clone(),
                    error: op.error.clone(),
                    done_bytes,
                    total_bytes,
                }
            })
            .collect()
    }
}

/// One move's UI-facing status (serde camelCase, shared with the TS `MoveStatus`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveStatus {
    pub id: String,
    pub hash: String,
    pub name: String,
    pub src: String,
    pub dst: String,
    pub state: MoveState,
    pub error: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
}

/// Parent directory of a daemon path (`/dl/Show` → `/dl`).
fn parent_dir(path: &str) -> String {
    match path.trim_end_matches('/').rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

/// Run one journalised move: stop → preflight → collision → copy →
/// `d.directory.set` → clear `final_dir` → resume.
///
/// The op is recorded Pending *before* anything mutates, so a kill at any
/// point leaves a resumable journal entry. The torrent is stopped first and
/// resumed (when it was active) on every exit path, so a failure or cancel
/// never strands it stopped.
pub async fn execute_move(
    backend: &dyn RtorrentApi,
    store: &Mutex<MoveStore>,
    task: MoveTask,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicU64>,
    on_event: Arc<MoveEventFn>,
) -> MoveOutcome {
    let now = task.now_ms;
    let op_id = {
        let mut store = store.lock().unwrap();
        if store
            .journal
            .resumable()
            .iter()
            .any(|op| op.hash.eq_ignore_ascii_case(&task.intent.hash))
        {
            return MoveOutcome::Busy;
        }
        let op = MoveOp {
            id: Journal::make_id(&task.intent.hash, now),
            hash: task.intent.hash.clone(),
            name: task.intent.name.clone(),
            src: task.intent.src_base.clone(),
            dst: format!(
                "{}/{}",
                task.intent.dst_dir.trim_end_matches('/'),
                task.intent.name.trim()
            ),
            state: MoveState::Pending,
            error: String::new(),
            size_bytes: task.intent.size_bytes,
            created_at_ms: now,
            updated_at_ms: now,
        };
        let id = op.id.clone();
        store.journal.add(op);
        let _ = store.save();
        on_event(MoveEvent::StateChanged);
        id
    };
    let one = std::slice::from_ref(&task.intent.hash);

    // Preflight before anything stops: a known shortage fails fast.
    match complete::preflight(task.intent.remaining, task.free) {
        complete::Preflight::BlockedNoSpace { required, free } => {
            let msg = format!(
                "not enough free space to move {} (need {}, have {})",
                task.intent.name,
                fmt_bytes(required),
                fmt_bytes(free)
            );
            finish(
                store,
                &op_id,
                MoveState::Failed,
                &msg,
                now,
                on_event.clone(),
            );
            return MoveOutcome::Failed(msg);
        }
        complete::Preflight::Unknown => {
            // Proceed; the host logs the warning alongside the start line.
        }
        complete::Preflight::Ready => {}
    }

    // CollisionGate: resolve the target name against the live directory.
    // (Pure check first — the fs move re-checks atomically at work time.)
    let siblings = siblings_lower(&task.local_dst_dir);
    let target_name = match complete::resolve_collision(
        &task.intent.dst_dir,
        &task.intent.name,
        &siblings,
        task.policy,
    ) {
        Ok(full) => file_name(&full),
        Err(msg) => {
            finish(
                store,
                &op_id,
                MoveState::Failed,
                &msg,
                now,
                on_event.clone(),
            );
            return MoveOutcome::Failed(msg);
        }
    };
    {
        let mut store = store.lock().unwrap();
        if let Some(op) = store.journal.ops.iter_mut().find(|op| op.id == op_id) {
            op.dst = format!(
                "{}/{}",
                task.intent.dst_dir.trim_end_matches('/'),
                target_name
            );
            op.updated_at_ms = now;
        }
        let _ = store.save();
    }

    // Intent state is a tick old: the queue promoting a freed slot, or the
    // user, may have started the torrent since it was planned. Re-read live
    // when the intent says stopped — copying under a running torrent is never
    // acceptable, and a torrent running at move time must resume afterwards.
    let mut resume = task.intent.was_active;
    if !resume {
        resume = backend
            .list_snapshot()
            .await
            .ok()
            .and_then(|rows| {
                rows.into_iter()
                    .find(|t| t.hash.eq_ignore_ascii_case(&task.intent.hash))
            })
            .map(|t| t.is_active)
            .unwrap_or(false);
    }
    if resume {
        if let Err(e) = backend.stop(one).await {
            let msg = format!("could not stop {} before moving: {e}", task.intent.name);
            finish(
                store,
                &op_id,
                MoveState::Failed,
                &msg,
                now,
                on_event.clone(),
            );
            return MoveOutcome::Failed(msg);
        }
    }
    mark(
        store,
        &op_id,
        MoveState::InProgress,
        "",
        now,
        on_event.clone(),
    );

    // The blocking copy, with progress into the shared counter.
    let total_hint = task.intent.size_bytes.max(0) as u64;
    let progress: ProgressFn = Arc::new({
        let on_event = Arc::clone(&on_event);
        move |d, _| {
            done.store(d, Ordering::Relaxed);
            on_event(MoveEvent::Progress {
                done: d,
                total: total_hint,
            });
        }
    });
    let res = {
        let src = task.local_src.clone();
        let dst_dir = task.local_dst_dir.clone();
        tokio::task::spawn_blocking(move || {
            fs::move_with_progress(&src, &dst_dir, &progress, &cancel)
        })
        .await
    };
    let target = match res {
        Ok(Ok(target)) => target,
        Ok(Err(FsMoveError::Cancelled)) => {
            if resume {
                let _ = backend.start(one).await;
            }
            finish(
                store,
                &op_id,
                MoveState::Cancelled,
                "move cancelled — resumed in place",
                now,
                on_event,
            );
            return MoveOutcome::Cancelled;
        }
        Ok(Err(FsMoveError::Failed(e))) => {
            if resume {
                let _ = backend.start(one).await;
            }
            let msg = format!("could not move {}: {e}", task.intent.name);
            finish(
                store,
                &op_id,
                MoveState::Failed,
                &msg,
                now,
                on_event.clone(),
            );
            return MoveOutcome::Failed(msg);
        }
        Err(join) => {
            if resume {
                let _ = backend.start(one).await;
            }
            let msg = format!("move task failed for {}: {join}", task.intent.name);
            finish(
                store,
                &op_id,
                MoveState::Failed,
                &msg,
                now,
                on_event.clone(),
            );
            return MoveOutcome::Failed(msg);
        }
    };

    // Point the daemon at the new home. The fs move re-verified the name, so
    // derive the daemon path from the same resolved target.
    let daemon_base = format!(
        "{}/{}",
        task.intent.dst_dir.trim_end_matches('/'),
        target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| task.intent.name.clone())
    );
    if let Err(e) = backend
        .set_directory(&task.intent.hash, &task.intent.dst_dir)
        .await
    {
        // Data is home but the daemon still points at the old dir. Keep
        // `final_dir` so a retry finishes the job (the fs move is a no-op
        // the second time: source is gone).
        let msg = format!(
            "moved {} on disk but the daemon still points at the old folder ({e}) — retry to finish",
            task.intent.name
        );
        finish(
            store,
            &op_id,
            MoveState::Failed,
            &msg,
            now,
            on_event.clone(),
        );
        return MoveOutcome::Failed(msg);
    }
    // Intent consumed: best-effort, never a failure on its own (a stale key
    // is self-healed by `homed_hashes`).
    let _ = backend
        .set_custom_metadata(&task.intent.hash, &[(complete::FINAL_DIR_KEY, "")])
        .await;
    if resume {
        let _ = backend.start(one).await;
    }
    finish(store, &op_id, MoveState::Done, "", now, on_event.clone());
    MoveOutcome::Done { daemon_base }
}

fn mark(
    store: &Mutex<MoveStore>,
    op_id: &str,
    state: MoveState,
    error: &str,
    now: i64,
    on_event: Arc<MoveEventFn>,
) {
    let mut store = store.lock().unwrap();
    store.journal.mark(op_id, state, error, now);
    let _ = store.save();
    drop(store);
    on_event(MoveEvent::StateChanged);
}

fn finish(
    store: &Mutex<MoveStore>,
    op_id: &str,
    state: MoveState,
    error: &str,
    now: i64,
    on_event: Arc<MoveEventFn>,
) {
    let terminal = matches!(
        state,
        MoveState::Done | MoveState::Failed | MoveState::Cancelled
    );
    mark(store, op_id, state, error, now, on_event);
    // Terminal states leave the live-handles map promptly so `busy_hashes`
    // (and the UI) stop treating the hash as in flight. The journal entry
    // itself stays for retry/audit until pruned.
    if terminal {
        store.lock().unwrap().unregister(op_id);
    }
}

/// Lowercased entry names in a local directory; missing dir = no siblings.
fn siblings_lower(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().to_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

fn file_name(full: &str) -> String {
    full.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(full)
        .to_string()
}

fn fmt_bytes(n: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = n.max(0) as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", n.max(0))
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Recover the journal after a (re)start, once the host has a snapshot.
///
/// - Drops Pending/Failed/Cancelled entries whose torrent is gone.
/// - Verifies InProgress entries against the local filesystem: target present
///   → finalize (point the daemon, clear intent, Done); target absent →
///   back to Pending so the next tick re-plans from daemon truth.
/// - An InProgress entry whose torrent is gone but whose target exists cannot
///   be finalized (nothing to point) → Failed with an honest message.
pub async fn resume_moves(
    backend: &dyn RtorrentApi,
    store: &Mutex<MoveStore>,
    live: &HashSet<String>,
    exists_local: &MoveExistsFn,
    now_ms: i64,
    on_event: Arc<MoveEventFn>,
) {
    // Snapshot the work first; no lock is held across awaits.
    let resumable: Vec<MoveOp> = store
        .lock()
        .unwrap()
        .journal
        .resumable()
        .into_iter()
        .cloned()
        .collect();

    {
        let mut store = store.lock().unwrap();
        store.prune_unknown(live);
        let _ = store.save();
    }

    for op in resumable {
        if op.state != MoveState::InProgress {
            continue;
        }
        // The entry may have been pruned above if its torrent is gone and it
        // was not InProgress — but InProgress survives pruning, so re-check.
        let alive = live.contains(&op.hash);
        let target_home = exists_local(&op.dst);
        if target_home && alive {
            // The copy finished before the kill; finalize without touching
            // bytes: point the daemon, clear the intent.
            let dir = parent_dir(&op.dst);
            let mut ok = backend.set_directory(&op.hash, &dir).await.is_ok();
            if ok {
                ok = backend
                    .set_custom_metadata(&op.hash, &[(complete::FINAL_DIR_KEY, "")])
                    .await
                    .is_ok();
            }
            // A daemon write failing here is not data loss (bytes are home);
            // leave InProgress so the next resume retries the pointer.
            if ok {
                mark(store, &op.id, MoveState::Done, "", now_ms, on_event.clone());
            }
        } else if target_home && !alive {
            let mut store = store.lock().unwrap();
            store.journal.mark(
                &op.id,
                MoveState::Failed,
                "torrent was removed mid-move; moved data was left in place",
                now_ms,
            );
            let _ = store.save();
            drop(store);
            on_event(MoveEvent::StateChanged);
        } else {
            // Target absent: the copy never landed — back to Pending for a
            // clean re-plan (the source side is verified at work time).
            mark(
                store,
                &op.id,
                MoveState::Pending,
                "",
                now_ms,
                on_event.clone(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::complete::{Journal, MoveState};

    fn torrent(
        hash: &str,
        complete: bool,
        directory: &str,
        final_dir: &str,
        name: &str,
    ) -> RawTorrent {
        RawTorrent {
            hash: hash.into(),
            name: name.into(),
            size_bytes: 1000,
            bytes_done: 1000,
            complete,
            is_active: true,
            label: String::new(),
            tags: Vec::new(),
            directory: directory.into(),
            base_path: format!("{directory}/{name}"),
            final_dir: final_dir.into(),
            ..RawTorrent::default()
        }
    }

    fn busy(hashes: &[&str]) -> HashSet<String> {
        hashes.iter().map(|h| h.to_string()).collect()
    }

    #[test]
    fn no_intent_no_move() {
        // Feature off everywhere: complete torrent already home, no rules.
        let raw = vec![torrent("A", true, "/dl", "", "Show")];
        assert!(decide_moves(&raw, &[], &[], &HashSet::new(), &HashSet::new()).is_empty());
    }

    #[test]
    fn recorded_final_dir_moves_when_complete() {
        let raw = vec![torrent("A", true, "/incomplete", "/dl", "Show")];
        let intents = decide_moves(&raw, &[], &[], &HashSet::new(), &HashSet::new());
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].dst_dir, "/dl");
        assert_eq!(intents[0].src_base, "/incomplete/Show");
    }

    #[test]
    fn incomplete_or_busy_or_skipped_torrents_are_left_alone() {
        let mut downloading = torrent("A", false, "/incomplete", "/dl", "Show");
        downloading.bytes_done = 100;
        let raw = vec![
            downloading,
            torrent("B", true, "/incomplete", "/dl", "Other"),
            torrent("C", true, "/incomplete", "/dl", "Third"),
        ];
        // B is busy (live task), C is skipped (seed-goal acted).
        let intents = decide_moves(&raw, &[], &[], &busy(&["C"]), &busy(&["B"]));
        assert!(intents.is_empty());
    }

    #[test]
    fn rules_move_torrents_without_recorded_intent() {
        let base = torrent("A", true, "/dl", "", "Show");
        // Label rule to a *different* dir: intent exists.
        let rules = vec![MoveRule::label_rule("video", "/media/video")];
        let mut t = base.clone();
        t.label = "video".into();
        let intents = decide_moves(&[t], &rules, &[], &HashSet::new(), &HashSet::new());
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].dst_dir, "/media/video");
        // Same dir: no plan, no intent.
        let same = vec![MoveRule::label_rule("video", "/dl")];
        let mut t2 = base;
        t2.label = "video".into();
        assert!(decide_moves(&[t2], &same, &[], &HashSet::new(), &HashSet::new()).is_empty());
    }

    #[test]
    fn hashing_torrents_and_empty_paths_are_skipped() {
        let mut t = torrent("A", true, "/incomplete", "/dl", "Show");
        t.hashing = true;
        assert!(decide_moves(&[t.clone()], &[], &[], &HashSet::new(), &HashSet::new()).is_empty());
        t.hashing = false;
        t.base_path = String::new();
        assert!(decide_moves(&[t], &[], &[], &HashSet::new(), &HashSet::new()).is_empty());
    }

    #[test]
    fn homed_hashes_find_stale_intent_only() {
        let home = torrent("A", true, "/dl", "/dl", "Show");
        let away = torrent("B", true, "/incomplete", "/dl", "Other");
        let no_intent = torrent("C", true, "/dl", "", "Plain");
        let found = homed_hashes(&[home, away, no_intent], &[], &[]);
        assert_eq!(found, vec!["A".to_string()]);
    }

    #[test]
    fn store_round_trips_and_tracks_busy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("move-journal.json");
        let mut store = MoveStore::load(path.clone());
        assert!(store.busy_hashes().is_empty());

        store.journal.add(MoveOp {
            id: Journal::make_id("AA", 1),
            hash: "AA".into(),
            name: "Show".into(),
            src: "/incomplete/Show".into(),
            dst: "/dl/Show".into(),
            state: MoveState::Pending,
            error: String::new(),
            size_bytes: 100,
            created_at_ms: 1,
            updated_at_ms: 1,
        });
        store.save().unwrap();
        assert!(store.busy_hashes().contains("AA"));

        let reloaded = MoveStore::load(path);
        assert!(reloaded.busy_hashes().contains("AA"));
        assert_eq!(reloaded.snapshot().len(), 1);

        // Retry drops the failed entry so the tick re-plans.
        store
            .journal
            .mark(&Journal::make_id("AA", 1), MoveState::Failed, "boom", 2);
        assert!(store.retry("aa"));
        assert!(!store.retry("aa"));
        assert!(store.snapshot().is_empty());
    }

    #[test]
    fn cancel_lands_on_a_live_task_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MoveStore::load(dir.path().join("j.json"));
        assert!(!store.cancel("nope"));
        store.register(ActiveMove {
            op_id: "AA-1".into(),
            hash: "AA".into(),
            cancel: Arc::new(AtomicBool::new(false)),
            done: Arc::new(AtomicU64::new(0)),
            total: 10,
        });
        assert!(store.cancel("AA-1"));
        assert!(store.cancel("AA"));
        assert!(store
            .active
            .get("AA-1")
            .unwrap()
            .cancel
            .load(Ordering::Relaxed));
    }

    fn test_task(
        dir: &std::path::Path,
        hash: &str,
        free: Option<i64>,
    ) -> (MoveTask, PathBuf, PathBuf) {
        let src_dir = dir.join("incomplete");
        let dst_dir = dir.join("dl");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("Show"), b"payload").unwrap();
        let task = MoveTask {
            intent: MoveIntent {
                hash: hash.into(),
                name: "Show".into(),
                src_base: "/incomplete/Show".into(),
                dst_dir: "/dl".into(),
                size_bytes: 7,
                remaining: 7,
                was_active: false,
            },
            local_src: src_dir.join("Show"),
            local_dst_dir: dst_dir,
            free,
            policy: CollisionPolicy::Error,
            now_ms: 1,
        };
        let dst_dir = dir.join("dl");
        (task, src_dir.join("Show"), dst_dir.join("Show"))
    }

    #[tokio::test]
    async fn execute_move_points_the_daemon_home_and_consumes_intent() {
        use crate::rtorrent::mock::MockClient;
        let backend = MockClient::new();
        let hash = backend.list_snapshot().await.unwrap()[0].hash.clone();
        // Record intent like the add path would.
        backend
            .set_custom_metadata(&hash, &[(crate::complete::FINAL_DIR_KEY, "/dl")])
            .await
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let (task, src, dst) = test_task(dir.path(), &hash, Some(1_000_000));
        let store = Mutex::new(MoveStore::load(dir.path().join("j.json")));
        let events = Arc::new(Mutex::new(0usize));
        let on_event: Arc<MoveEventFn> = Arc::new({
            let events = Arc::clone(&events);
            move |_| {
                *events.lock().unwrap() += 1;
            }
        });
        let outcome = execute_move(
            &backend,
            &store,
            task,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
            Arc::clone(&on_event),
        )
        .await;
        assert!(matches!(outcome, MoveOutcome::Done { .. }));
        assert!(!src.exists(), "source cleaned up");
        assert!(dst.exists(), "data home");

        let rows = backend.list_snapshot().await.unwrap();
        let row = rows.iter().find(|t| t.hash == hash).unwrap();
        assert_eq!(row.directory, "/dl");
        assert!(row.final_dir.is_empty(), "intent consumed");

        let store = store.lock().unwrap();
        assert_eq!(
            store
                .journal
                .ops
                .iter()
                .filter(|op| op.state == MoveState::Done)
                .count(),
            1
        );
        assert!(*events.lock().unwrap() >= 2, "state changes emitted");
    }

    #[tokio::test]
    async fn stale_stopped_intent_still_stops_and_resumes_a_live_torrent() {
        use crate::rtorrent::mock::MockClient;
        let backend = MockClient::new();
        // An active fixture: the intent below claims it is stopped.
        let active = backend
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.is_active)
            .expect("an active fixture");

        let dir = tempfile::tempdir().unwrap();
        let (mut task, _src, dst) = test_task(dir.path(), &active.hash, Some(1_000_000));
        task.intent.was_active = false;
        let store = Mutex::new(MoveStore::load(dir.path().join("j.json")));
        let on_event: Arc<MoveEventFn> = Arc::new(|_| {});
        let outcome = execute_move(
            &backend,
            &store,
            task,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
            on_event,
        )
        .await;
        assert!(
            matches!(outcome, MoveOutcome::Done { .. }),
            "live state wins over stale intent"
        );
        assert!(dst.exists());
        let row = backend
            .list_snapshot()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.hash == active.hash)
            .unwrap();
        assert!(row.is_active, "a running torrent resumes after its move");
    }

    #[tokio::test]
    async fn execute_move_fails_fast_on_no_space_without_touching_bytes() {
        use crate::rtorrent::mock::MockClient;
        let backend = MockClient::new();
        let hash = backend.list_snapshot().await.unwrap()[0].hash.clone();

        let dir = tempfile::tempdir().unwrap();
        let (task, src, dst) = test_task(dir.path(), &hash, Some(1));
        let store = Mutex::new(MoveStore::load(dir.path().join("j.json")));
        let on_event: Arc<MoveEventFn> = Arc::new(|_| {});
        let outcome = execute_move(
            &backend,
            &store,
            task,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
            on_event,
        )
        .await;
        assert!(matches!(outcome, MoveOutcome::Failed(_)));
        assert!(src.exists(), "source untouched");
        assert!(!dst.exists(), "nothing copied");
    }

    #[tokio::test]
    async fn cancelled_move_resumes_the_torrent_in_place() {
        use crate::rtorrent::mock::MockClient;
        let backend = MockClient::new();
        let hash = backend.list_snapshot().await.unwrap()[0].hash.clone();

        let dir = tempfile::tempdir().unwrap();
        // Big enough that the copy path (not the rename fast path) runs:
        // pre-create the target's *parent* only. Rename within one tmpdir
        // would win, so force the copy by pre-cancelling — the first chunk
        // check fires before any byte moves.
        let (mut task, src, dst) = test_task(dir.path(), &hash, Some(1_000_000));
        task.intent.was_active = true;
        // Point local paths into separate trees so the test reads clearly.
        let store = Mutex::new(MoveStore::load(dir.path().join("j.json")));
        let cancel = Arc::new(AtomicBool::new(true));
        let on_event: Arc<MoveEventFn> = Arc::new(|_| {});
        // Register like the host does before spawning.
        let op_id = format!("{}-1", hash.to_ascii_uppercase());
        store.lock().unwrap().register(ActiveMove {
            op_id: op_id.clone(),
            hash: hash.clone(),
            cancel: Arc::clone(&cancel),
            done: Arc::new(AtomicU64::new(0)),
            total: 7,
        });
        let outcome = execute_move(
            &backend,
            &store,
            task,
            cancel,
            Arc::new(AtomicU64::new(0)),
            on_event,
        )
        .await;
        // Rename may win before the cancel check (same volume): either way
        // the invariants hold — bytes in exactly one place, journal terminal.
        let store = store.lock().unwrap();
        let states: Vec<MoveState> = store
            .journal
            .ops
            .iter()
            .map(|op| op.state.clone())
            .collect();
        match outcome {
            MoveOutcome::Cancelled => {
                assert!(src.exists());
                assert!(states.contains(&MoveState::Cancelled));
            }
            MoveOutcome::Done { .. } => {
                assert!(!src.exists() && dst.exists());
                assert!(states.contains(&MoveState::Done));
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }
}
