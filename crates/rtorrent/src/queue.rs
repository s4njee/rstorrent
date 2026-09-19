//! Complete queue policy (V3-17 / QUE-01 + QUE-02).
//!
//! qB gates three independent limits — max active downloads, max active
//! uploads (seeds), max active torrents — with a slow-torrent exemption and a
//! per-torrent force-start. rtorrent has none of these, so the client
//! scheduler enforces them by pausing (never closing: a paused torrent stays
//! loaded, which is exactly what the console already words "Queued") and
//! starting, one detached decision per successful tick.
//!
//! Two honesty rules shape this module:
//! * The scheduler never fights the user. A manual resume of a held-back
//!   torrent exempts it from stopping; a manual pause of a wanted torrent
//!   exempts it from starting. Both exemptions are edge-triggered, pruned to
//!   live hashes every tick, and forgotten on reconnect.
//! * Positions are never faked. rtorrent has priority levels, not slots, so
//!   manual order is `d.priority` plus a client-side sequence in
//!   `d.custom=queue_pos` — and the UI shows bands, never "#" slots.
//!
//! The browser cannot call this crate, so the TS side mirrors only the tiny
//! pure pieces it needs (nothing yet); hosts share this driver verbatim.

use std::collections::{HashMap, HashSet};

use crate::rtorrent::RawTorrent;

/// `d.custom` flag marking a torrent force-started (exempt from the queue).
pub const FORCE_KEY: &str = "force_start";
/// `d.custom` key holding the client-side queue sequence (QUE-02).
pub const POS_KEY: &str = "queue_pos";

/// Parse a stored queue position; missing, empty or corrupt sorts as 0, so
/// legacy torrents keep their long-standing relative order.
#[must_use]
pub fn parse_pos(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<i64>().ok()
}

/// Format a queue position for `d.custom`.
#[must_use]
pub fn format_pos(pos: i64) -> String {
    pos.to_string()
}

/// The three caps plus the slow-torrent exemption, straight from settings.
/// Every cap `<= 0` disables that limit; `slow_limit_kbs <= 0` disables the
/// exemption.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueueConfig {
    pub max_downloads: i64,
    pub max_uploads: i64,
    pub max_total: i64,
    /// Torrents slower than this (down + up, KiB/s) are left alone entirely:
    /// never stopped, never counted. Mirrors qB's "do not count slow
    /// torrents".
    pub slow_limit_kbs: i64,
}

impl QueueConfig {
    #[must_use]
    pub fn configured(self) -> bool {
        self.max_downloads > 0 || self.max_uploads > 0 || self.max_total > 0
    }
}

/// Edge-triggered scheduler memory, one per host poll loop.
///
/// `prev_active` spots manual resumes/pauses; `user_exempt` holds hashes the
/// user resumed past the scheduler (never stop them); `user_paused` holds
/// hashes the user paused against the scheduler's wishes (never start them).
/// All three are pruned to live hashes every tick and cleared on reconnect,
/// so a new session is judged fresh.
#[derive(Clone, Debug, Default)]
pub struct QueueMemory {
    prev_active: HashMap<String, bool>,
    user_exempt: HashSet<String>,
    user_paused: HashSet<String>,
}

impl QueueMemory {
    /// Forget everything: a (re)connect judges the new session fresh.
    pub fn reset(&mut self) {
        self.prev_active.clear();
        self.user_exempt.clear();
        self.user_paused.clear();
    }

    /// Record scheduler ownership (V3-18 / QUE-05): a scheduler-paused hash
    /// reads as user-paused to the queue decider, so the two never fight.
    /// Clearing hands the hash back to normal queue management.
    pub fn set_scheduler_held(&mut self, hash: &str, held: bool) {
        if held {
            self.user_paused.insert(hash.to_owned());
        } else {
            self.user_paused.remove(hash);
        }
    }
}

/// One tick's verdict. Holds use `pause` (stay loaded → renders "Queued"),
/// never `stop`, so a held-back torrent resumes without a reannounce.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueueActions {
    pub start: Vec<String>,
    pub stop: Vec<String>,
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

/// Scheduler order within one class: priority first, then the client
/// sequence, then start time. Unpositioned torrents sort as 0.
fn order_key(t: &RawTorrent) -> (i64, i64, i64) {
    (t.priority, t.queue_pos.unwrap_or(0), started_key(t))
}

/// Better rank first.
fn better(a: &(i64, i64, i64), b: &(i64, i64, i64)) -> bool {
    if a.0 != b.0 {
        return a.0 > b.0;
    }
    if a.1 != b.1 {
        return a.1 < b.1;
    }
    a.2 < b.2
}

/// Healthy enough for the scheduler to touch: settled, and no daemon-reported
/// problem. Mirrors the C9 filter.
fn healthy(t: &RawTorrent) -> bool {
    !t.hashing && t.message.is_empty()
}

/// Torrents slower than the exemption floor (down + up, bytes/s).
fn is_slow(t: &RawTorrent, slow_limit_kbs: i64) -> bool {
    slow_limit_kbs > 0 && t.down_rate + t.up_rate < slow_limit_kbs.saturating_mul(1024)
}

/// Decide one tick: which hashes to start, which to hold back (pause).
///
/// `skip` holds hashes sibling policy already acted on this tick (seed-goal
/// stops/removes), so the queue never resurrects them. Every cap `<= 0` in
/// `cfg` disables that limit; a fully-disabled config is a no-op.
pub fn decide_queues(
    raw: &[RawTorrent],
    cfg: QueueConfig,
    mem: &mut QueueMemory,
    skip: &HashSet<String>,
) -> QueueActions {
    let mut actions = QueueActions::default();
    if !cfg.configured() {
        return actions;
    }
    let live: HashSet<&str> = raw.iter().map(|t| t.hash.as_str()).collect();
    mem.prev_active.retain(|h, _| live.contains(h.as_str()));
    mem.user_exempt.retain(|h| live.contains(h.as_str()));
    mem.user_paused.retain(|h| live.contains(h.as_str()));

    // Class split first: downloads (!complete) and uploads (complete) draw
    // from separate caps. Forced and slow torrents are outside the system —
    // never touched, never counted.
    let mut downloads: Vec<&RawTorrent> = Vec::new();
    let mut uploads: Vec<&RawTorrent> = Vec::new();
    for t in raw {
        if !healthy(t) || t.force_start || is_slow(t, cfg.slow_limit_kbs) {
            continue;
        }
        if t.complete {
            uploads.push(t);
        } else {
            downloads.push(t);
        }
    }
    let rank = |t: &&RawTorrent| order_key(t);
    downloads.sort_by(|a, b| {
        let (ka, kb) = (rank(a), rank(b));
        if better(&ka, &kb) {
            std::cmp::Ordering::Less
        } else if better(&kb, &ka) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    uploads.sort_by(|a, b| {
        let (ka, kb) = (rank(a), rank(b));
        if better(&ka, &kb) {
            std::cmp::Ordering::Less
        } else if better(&kb, &ka) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });

    // Per-class windows; a disabled cap wants everything.
    let mut want: HashSet<String> = HashSet::new();
    let window = |list: &[&RawTorrent], cap: i64| -> usize {
        if cap <= 0 {
            list.len()
        } else {
            (cap as usize).min(list.len())
        }
    };
    want.extend(
        downloads[..window(&downloads, cfg.max_downloads)]
            .iter()
            .map(|t| t.hash.clone()),
    );
    want.extend(
        uploads[..window(&uploads, cfg.max_uploads)]
            .iter()
            .map(|t| t.hash.clone()),
    );

    // The total cap trims the combined want-set by global rank, downloads
    // breaking ties (finish what is being fetched first).
    if cfg.max_total > 0 && want.len() > cfg.max_total as usize {
        /// Global rank for the total-cap trim.
        struct Ranked<'a> {
            torrent: &'a RawTorrent,
            priority: i64,
            pos: i64,
            started: i64,
            complete: bool,
        }
        let mut ranked: Vec<Ranked<'_>> = raw
            .iter()
            .filter(|t| want.contains(&t.hash))
            .map(|t| {
                let k = order_key(t);
                Ranked {
                    torrent: t,
                    priority: k.0,
                    pos: k.1,
                    started: k.2,
                    complete: t.complete,
                }
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.pos.cmp(&b.pos))
                .then_with(|| a.started.cmp(&b.started))
                .then_with(|| a.complete.cmp(&b.complete))
        });
        want = ranked
            .into_iter()
            .take(cfg.max_total as usize)
            .map(|r| r.torrent.hash.clone())
            .collect();
    }

    // Edge-triggered user intent, disambiguated by the want-set geometry: our
    // own starts only ever target wanted hashes, so a 0→1 edge off-window is
    // always a manual resume, and a 1→0 edge on-window is always a manual
    // pause.
    for t in raw {
        let prev = mem.prev_active.get(&t.hash).copied();
        let wanted = want.contains(&t.hash);
        if t.is_active && prev == Some(false) {
            if wanted {
                mem.user_paused.remove(&t.hash);
            } else {
                mem.user_exempt.insert(t.hash.clone());
            }
        } else if !t.is_active && prev == Some(true) && wanted {
            mem.user_paused.insert(t.hash.clone());
        }
        if wanted {
            mem.user_exempt.remove(&t.hash);
        }
    }
    for t in raw {
        mem.prev_active.insert(t.hash.clone(), t.is_active);
    }

    // The verdict, emitted in rank order (promotions head-first, holds
    // worst-first). Exempt hashes are left exactly as the user left them;
    // everything else converges on the want-set. Holds pause (stay loaded);
    // promotions start.
    for t in downloads.iter().chain(uploads.iter()) {
        if skip.contains(&t.hash) || !healthy(t) || t.force_start || is_slow(t, cfg.slow_limit_kbs)
        {
            continue;
        }
        if want.contains(&t.hash) && !t.is_active && !mem.user_paused.contains(&t.hash) {
            actions.start.push(t.hash.clone());
        }
    }
    for t in uploads.iter().rev().chain(downloads.iter().rev()) {
        if skip.contains(&t.hash) || !healthy(t) || t.force_start || is_slow(t, cfg.slow_limit_kbs)
        {
            continue;
        }
        if !want.contains(&t.hash) && t.is_active && !mem.user_exempt.contains(&t.hash) {
            actions.stop.push(t.hash.clone());
        }
    }
    actions
}

/// Manual reorder direction (QUE-02).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveDir {
    Top,
    Up,
    Down,
    Bottom,
}

/// One daemon write the caller must apply: a priority band plus a sequence
/// value. Swapping whole `(priority, pos)` pairs moves a torrent to exactly
/// the neighbour's rank — the closest rtorrent gets to a list reorder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reorder {
    pub hash: String,
    pub priority: i64,
    pub pos: i64,
}

/// Plan a manual reorder over the full ordered list. Selection is processed
/// head-first in the direction of travel so multi-selects glide as a block;
/// Top/Bottom pin `(3, min-1)` / `(0, max+1)` per item to preserve relative
/// order. Unpositioned torrents materialise at 0 only when they take part in
/// a swap.
pub fn plan_reorder(raw: &[RawTorrent], hashes: &[String], dir: MoveDir) -> Vec<Reorder> {
    if hashes.is_empty() {
        return Vec::new();
    }
    // Full order, best rank first. Stable over hash for full ties.
    let mut ordered: Vec<String> = raw.iter().map(|t| t.hash.clone()).collect();
    ordered.sort_by(|a, b| {
        let ta = raw.iter().find(|t| &t.hash == a);
        let tb = raw.iter().find(|t| &t.hash == b);
        match (ta, tb) {
            (Some(ta), Some(tb)) => {
                let (ka, kb) = (order_key(ta), order_key(tb));
                if better(&ka, &kb) {
                    std::cmp::Ordering::Less
                } else if better(&kb, &ka) {
                    std::cmp::Ordering::Greater
                } else {
                    a.cmp(b)
                }
            }
            _ => std::cmp::Ordering::Equal,
        }
    });
    let key_of = |hash: &str, assigned: &HashMap<String, (i64, i64)>| -> Option<(i64, i64)> {
        if let Some(k) = assigned.get(hash) {
            return Some(*k);
        }
        raw.iter().find(|t| t.hash == hash).map(|t| {
            let k = order_key(t);
            (k.0, k.1)
        })
    };

    let selected: HashSet<&str> = hashes.iter().map(String::as_str).collect();
    let mut assigned: HashMap<String, (i64, i64)> = HashMap::new();
    let mut out: Vec<Reorder> = Vec::new();

    match dir {
        MoveDir::Up => {
            for hash in ordered.clone() {
                if !selected.contains(hash.as_str()) {
                    continue;
                }
                let pos = ordered.iter().position(|h| h == &hash).unwrap_or(0);
                if pos == 0 {
                    continue;
                }
                let pred = ordered[pos - 1].clone();
                let hk = key_of(&hash, &assigned).unwrap_or((1, 0));
                let pk = key_of(&pred, &assigned).unwrap_or((1, 0));
                emit_reorder(&mut assigned, &mut out, &hash, pk);
                emit_reorder(&mut assigned, &mut out, &pred, hk);
                ordered.swap(pos, pos - 1);
            }
        }
        MoveDir::Down => {
            for hash in ordered.clone().into_iter().rev() {
                if !selected.contains(hash.as_str()) {
                    continue;
                }
                let pos = ordered.iter().position(|h| h == &hash).unwrap_or(0);
                if pos + 1 >= ordered.len() {
                    continue;
                }
                let succ = ordered[pos + 1].clone();
                let hk = key_of(&hash, &assigned).unwrap_or((1, 0));
                let sk = key_of(&succ, &assigned).unwrap_or((1, 0));
                emit_reorder(&mut assigned, &mut out, &hash, sk);
                emit_reorder(&mut assigned, &mut out, &succ, hk);
                ordered.swap(pos, pos + 1);
            }
        }
        MoveDir::Top => {
            let mut floor = raw.iter().map(|t| order_key(t).1).min().unwrap_or(0);
            for hash in ordered.clone() {
                if !selected.contains(hash.as_str()) {
                    continue;
                }
                floor -= 1;
                emit_reorder(&mut assigned, &mut out, &hash, (3, floor));
            }
        }
        MoveDir::Bottom => {
            let mut ceil = raw.iter().map(|t| order_key(t).1).max().unwrap_or(0);
            for hash in ordered.clone().into_iter().rev() {
                if !selected.contains(hash.as_str()) {
                    continue;
                }
                ceil += 1;
                emit_reorder(&mut assigned, &mut out, &hash, (0, ceil));
            }
        }
    }
    out
}

/// Force-start toggle value for a selection: `"1"` when any targeted torrent
/// lacks the flag (mixed selections switch on), `""` when all have it.
#[must_use]
pub fn force_toggle_value(rows: &[RawTorrent], hashes: &[String]) -> &'static str {
    let any_off = hashes.iter().any(|h| {
        rows.iter()
            .find(|t| &t.hash == h)
            .map_or(true, |t| !t.force_start)
    });
    if any_off {
        "1"
    } else {
        ""
    }
}

/// Record one (priority, pos) assignment, merging repeats for the same hash
/// so a multi-select gliding as a block emits one write per torrent.
fn emit_reorder(
    assigned: &mut HashMap<String, (i64, i64)>,
    out: &mut Vec<Reorder>,
    hash: &str,
    key: (i64, i64),
) {
    if let Some(prev) = assigned.get_mut(hash) {
        *prev = key;
        if let Some(r) = out.iter_mut().find(|r| r.hash == hash) {
            r.priority = key.0;
            r.pos = key.1;
        }
    } else {
        assigned.insert(hash.to_owned(), key);
        out.push(Reorder {
            hash: hash.to_owned(),
            priority: key.0,
            pos: key.1,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn torrent(
        hash: &str,
        complete: bool,
        active: bool,
        priority: i64,
        started: i64,
    ) -> RawTorrent {
        RawTorrent {
            hash: hash.into(),
            name: hash.into(),
            size_bytes: 1000,
            bytes_done: if complete { 1000 } else { 100 },
            complete,
            is_active: active,
            is_open: true,
            priority,
            started_at: started,
            down_rate: if !complete && active { 10 } else { 0 },
            up_rate: if complete && active { 10 } else { 0 },
            ..RawTorrent::default()
        }
    }

    fn cfg(downloads: i64, uploads: i64, total: i64) -> QueueConfig {
        QueueConfig {
            max_downloads: downloads,
            max_uploads: uploads,
            max_total: total,
            slow_limit_kbs: 0,
        }
    }

    fn empty() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn disabled_config_is_a_no_op() {
        let raw = vec![torrent("A", false, true, 1, 1)];
        let mut mem = QueueMemory::default();
        assert_eq!(
            decide_queues(&raw, QueueConfig::default(), &mut mem, &empty()),
            QueueActions::default()
        );
    }

    #[test]
    fn downloads_cap_pauses_the_tail_and_promotes_the_head() {
        // B(pri 3, stopped) beats A(pri 2, active) beats C(pri 1, active).
        let raw = vec![
            torrent("A", false, true, 2, 100),
            torrent("B", false, false, 3, 90),
            torrent("C", false, true, 1, 80),
        ];
        let mut mem = QueueMemory::default();
        let actions = decide_queues(&raw, cfg(2, 0, 0), &mut mem, &empty());
        assert_eq!(actions.start, vec!["B".to_string()]);
        assert_eq!(actions.stop, vec!["C".to_string()]);
    }

    #[test]
    fn uploads_cap_holds_finished_seeds() {
        let raw = vec![
            torrent("S1", true, true, 2, 100),
            torrent("S2", true, true, 1, 90),
            torrent("S3", true, false, 3, 80),
        ];
        let mut mem = QueueMemory::default();
        let actions = decide_queues(&raw, cfg(0, 2, 0), &mut mem, &empty());
        assert_eq!(actions.start, vec!["S3".to_string()]);
        assert_eq!(actions.stop, vec!["S2".to_string()]);
    }

    #[test]
    fn total_cap_trims_across_classes_by_global_rank() {
        // Two downloads + two seeds, total cap 2: the two pri-3 win.
        let raw = vec![
            torrent("D1", false, true, 3, 100),
            torrent("D2", false, true, 1, 90),
            torrent("S1", true, true, 3, 80),
            torrent("S2", true, true, 1, 70),
        ];
        let mut mem = QueueMemory::default();
        let actions = decide_queues(&raw, cfg(0, 0, 2), &mut mem, &empty());
        assert!(actions.start.is_empty());
        assert_eq!(actions.stop.len(), 2);
        assert!(actions.stop.contains(&"D2".to_string()));
        assert!(actions.stop.contains(&"S2".to_string()));
    }

    #[test]
    fn force_started_and_slow_torrents_are_untouched_and_uncounted() {
        let mut forced = torrent("F", false, true, 0, 100);
        forced.force_start = true;
        let mut slow = torrent("S", false, true, 3, 90);
        slow.down_rate = 1;
        slow.up_rate = 1;
        let plain = torrent("P", false, true, 2, 80);
        let raw = vec![forced, slow, plain];
        let mut mem = QueueMemory::default();
        let slow_cfg = QueueConfig {
            slow_limit_kbs: 10,
            ..cfg(1, 0, 0)
        };
        // Cap 1: the plain torrent is the only managed one and fits.
        let actions = decide_queues(&raw, slow_cfg, &mut mem, &empty());
        assert!(actions.start.is_empty() && actions.stop.is_empty());
    }

    #[test]
    fn manual_resume_exempts_from_stopping() {
        // Cap 1: A(pri 5, stopped) is wanted → start; B(pri 0, active) is
        // over the cap → hold back.
        let mut mem = QueueMemory::default();
        let t1 = vec![
            torrent("A", false, false, 5, 100),
            torrent("B", false, true, 0, 90),
        ];
        let a = decide_queues(&t1, cfg(1, 0, 0), &mut mem, &empty());
        assert_eq!(a.start, vec!["A".to_string()]);
        assert_eq!(a.stop, vec!["B".to_string()]);
        // The daemon applies the start but the user resumes B first, so the
        // next snapshot shows both active. Feed the applied state first so
        // the edges read correctly...
        let t_mid = vec![
            torrent("A", false, true, 5, 100),
            torrent("B", false, false, 0, 90),
        ];
        let _ = decide_queues(&t_mid, cfg(1, 0, 0), &mut mem, &empty());
        // ...then the user resumes B: 0→1 edge off-window.
        let t2 = vec![
            torrent("A", false, true, 5, 100),
            torrent("B", false, true, 0, 90),
        ];
        let a2 = decide_queues(&t2, cfg(1, 0, 0), &mut mem, &empty());
        assert!(
            !a2.stop.contains(&"B".to_string()),
            "a manual resume is never re-paused"
        );
    }

    #[test]
    fn manual_pause_is_never_restarted() {
        let mut mem = QueueMemory::default();
        let running = vec![torrent("A", false, true, 5, 100)];
        let _ = decide_queues(&running, cfg(2, 0, 0), &mut mem, &empty());
        // User pauses A although it is wanted.
        let paused = vec![torrent("A", false, false, 5, 100)];
        let a = decide_queues(&paused, cfg(2, 0, 0), &mut mem, &empty());
        assert!(a.start.is_empty(), "a manual pause sticks");
        // And stays sticky across ticks.
        let a2 = decide_queues(&paused, cfg(2, 0, 0), &mut mem, &empty());
        assert!(a2.start.is_empty());
    }

    #[test]
    fn skip_set_protects_sibling_policy_decrees() {
        let raw = vec![torrent("A", false, false, 5, 100)];
        let mut mem = QueueMemory::default();
        let skip: HashSet<String> = ["A".to_string()].into_iter().collect();
        let a = decide_queues(&raw, cfg(2, 0, 0), &mut mem, &skip);
        assert!(a.start.is_empty());
    }

    #[test]
    fn reorder_up_swaps_with_the_predecessor() {
        let mut c = torrent("C", false, true, 1, 80);
        c.queue_pos = Some(5);
        let raw = vec![
            torrent("A", false, true, 2, 100),
            torrent("B", false, true, 1, 90),
            c,
        ];
        // Order: A(2,0), B(1,0), C(1,5). B's predecessor is A: they swap
        // whole (priority, pos) pairs, so B takes A's rank exactly.
        let plan = plan_reorder(&raw, &["B".to_string()], MoveDir::Up);
        assert_eq!(plan.len(), 2);
        let b = plan.iter().find(|r| r.hash == "B").unwrap();
        let a = plan.iter().find(|r| r.hash == "A").unwrap();
        assert_eq!((b.priority, b.pos), (2, 0));
        assert_eq!((a.priority, a.pos), (1, 0));
    }

    #[test]
    fn reorder_top_pins_and_bottom_grounds() {
        let raw = vec![
            torrent("A", false, true, 2, 100),
            torrent("B", false, true, 1, 90),
        ];
        let top = plan_reorder(&raw, &["B".to_string()], MoveDir::Top);
        assert_eq!(top.len(), 1);
        assert_eq!((top[0].priority, top[0].pos), (3, -1));
        let bottom = plan_reorder(&raw, &["A".to_string()], MoveDir::Bottom);
        assert_eq!(bottom.len(), 1);
        assert_eq!((bottom[0].priority, bottom[0].pos), (0, 1));
    }

    #[test]
    fn reorder_down_at_the_floor_is_a_no_op() {
        let raw = vec![torrent("A", false, true, 2, 100)];
        assert!(plan_reorder(&raw, &["A".to_string()], MoveDir::Down).is_empty());
        assert!(plan_reorder(&[], &["A".to_string()], MoveDir::Up).is_empty());
    }

    #[test]
    fn force_toggle_switches_mixed_selections_on() {
        let mut on = torrent("A", false, true, 1, 1);
        on.force_start = true;
        let off = torrent("B", false, true, 1, 2);
        let raw = vec![on, off];
        assert_eq!(
            force_toggle_value(&raw, &["A".to_string(), "B".to_string()]),
            "1"
        );
        assert_eq!(force_toggle_value(&raw, &["A".to_string()]), "");
        assert_eq!(force_toggle_value(&raw, &["ZZZ".to_string()]), "1");
    }

    #[test]
    fn parse_pos_treats_blanks_and_garbage_as_unset() {
        assert_eq!(parse_pos(""), None);
        assert_eq!(parse_pos("  "), None);
        assert_eq!(parse_pos("abc"), None);
        assert_eq!(parse_pos("  -12 "), Some(-12));
        assert_eq!(parse_pos("7"), Some(7));
        assert_eq!(format_pos(-3), "-3");
    }
}
