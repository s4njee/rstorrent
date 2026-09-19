//! Move-on-complete + incomplete-data folder (V3-14 / LIB-07 / LIB-08 / C10).
//!
//! The shared, shell-independent slice: destination resolution, free-space
//! preflight, collision-safe target naming, and the minimal crash-safe
//! operation journal (the move-only part of FND-07). Filesystem moves
//! themselves stay in [`crate::fs::move_torrent_data`]; the poller/host owns
//! the stop → move → `d.directory.set` → recheck → resume sequence and drives
//! it from [`Journal`] entries so a kill mid-copy resumes instead of losing
//! data.
//!
//! Precedence (documented, so every shell agrees): the first tag rule whose
//! tag the torrent carries wins, then the label rule, then the default save
//! path. Tags win because they are explicit and ordered; the label is the
//! single ruTorrent-compatible category.
//!
//! The browser cannot call this crate, so `src/utils/complete.ts` mirrors the
//! pure rules; the tests on both sides pin the shared behaviour.

use serde::{Deserialize, Serialize};

/// The `d.custom` key holding a torrent's intended final directory (V3-14).
/// Recorded at add time when the torrent is routed through the incomplete
/// dir, so the completion move knows where "home" is even if rules change
/// before the download finishes. Consumed (cleared) by the move.
pub const FINAL_DIR_KEY: &str = "final_dir";

/// One destination rule: either a tag match or a label match.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveRule {
    /// Tag this rule fires on (`tag_rule` lowercases it; matching lowercases
    /// anyway, so hand-written config needs no normalisation). `None` when
    /// this is a label rule.
    #[serde(default)]
    pub tag: Option<String>,
    /// Label this rule fires on (same case-insensitive deal). `None` when
    /// this is a tag rule.
    #[serde(default)]
    pub label: Option<String>,
    /// Destination directory for completed data.
    pub destination: String,
}

impl MoveRule {
    /// A tag rule; the tag is lowercased so matching is case-insensitive.
    #[must_use]
    pub fn tag_rule(tag: &str, destination: &str) -> Self {
        Self {
            tag: Some(tag.trim().to_lowercase()),
            label: None,
            destination: destination.trim().to_string(),
        }
    }

    /// A label rule; matching is case-insensitive like the sidebar.
    #[must_use]
    pub fn label_rule(label: &str, destination: &str) -> Self {
        Self {
            tag: None,
            label: Some(label.trim().to_lowercase()),
            destination: destination.trim().to_string(),
        }
    }
}

/// Where a torrent's data should live right now.
#[must_use]
pub fn resolve_destination(
    label: &str,
    tags: &[String],
    rules: &[MoveRule],
    default_dir: &str,
) -> String {
    let lowered_tags: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    for rule in rules {
        // Lowercased here (not only in the constructors) so hand-written
        // config files need no normalisation to match case-insensitively.
        if let Some(tag) = rule.tag.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            let tag = tag.to_lowercase();
            if lowered_tags.iter().any(|t| t == &tag) && !rule.destination.is_empty() {
                return rule.destination.clone();
            }
        }
    }
    let label_key = label.trim().to_lowercase();
    if !label_key.is_empty() {
        for rule in rules {
            if rule.tag.is_none() {
                if let Some(rule_label) = rule.label.as_deref().map(str::trim) {
                    if rule_label.to_lowercase() == label_key && !rule.destination.is_empty() {
                        return rule.destination.clone();
                    }
                }
            }
        }
    }
    default_dir.to_string()
}

/// Where a *completing* torrent's data belongs: the explicit move rules
/// first, then the per-label save paths (C11) as label rules, then the
/// default — which the caller sets to the torrent's recorded `final_dir`
/// when one was stored at add time, else the global save path.
///
/// Chaining the C11 paths keeps one truth for "where label L lives": a label
/// default configured before this feature still guides completions, and an
/// explicit move rule supersedes it (documented, so every shell agrees).
#[must_use]
pub fn completion_destination(
    label: &str,
    tags: &[String],
    move_rules: &[MoveRule],
    label_paths: &[(&str, &str)],
    default_dir: &str,
) -> String {
    let mut combined: Vec<MoveRule> = move_rules.to_vec();
    for (label, path) in label_paths {
        if !label.trim().is_empty() && !path.trim().is_empty() {
            combined.push(MoveRule::label_rule(label, path));
        }
    }
    resolve_destination(label, tags, &combined, default_dir)
}

/// Route a new download: which directory to load it with, and the final
/// directory to record in `d.custom=final_dir` (if any).
///
/// With no incomplete dir configured this is the identity — the torrent loads
/// straight into its chosen path and nothing is ever recorded. Otherwise the
/// torrent loads into the incomplete dir and the chosen path is returned for
/// recording, so the completion move can take it home.
#[must_use]
pub fn route_new_download(incomplete_dir: &str, chosen_dir: &str) -> (String, Option<String>) {
    let incomplete = incomplete_dir.trim();
    let chosen = chosen_dir.trim();
    if incomplete.is_empty() || chosen.is_empty() {
        return (chosen.to_string(), None);
    }
    if incomplete.eq_ignore_ascii_case(chosen.trim_end_matches('/'))
        || incomplete
            .trim_end_matches('/')
            .eq_ignore_ascii_case(chosen)
    {
        return (chosen.to_string(), None);
    }
    (incomplete.to_string(), Some(chosen.to_string()))
}

/// A planned move: `src_dir/name` → `dst_dir/name`. `None` when there is
/// nothing to do (empty destination, or already there, case-insensitively).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MovePlan {
    pub src: String,
    pub dst: String,
}

#[must_use]
pub fn plan_move(current_dir: &str, destination: &str, name: &str) -> Option<MovePlan> {
    let current = current_dir.trim().trim_end_matches('/');
    let dest = destination.trim().trim_end_matches('/');
    let name = name.trim();
    if dest.is_empty() || name.is_empty() || current.eq_ignore_ascii_case(dest) {
        return None;
    }
    if current.is_empty() {
        return None;
    }
    Some(MovePlan {
        src: format!("{current}/{name}"),
        dst: format!("{dest}/{name}"),
    })
}

/// What to do when the destination name is already taken. Never overwrite
/// silently: the default is [`CollisionPolicy::Error`].
///
/// Serialised kebab-case (`"auto-rename"`) to match the TS mirror.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollisionPolicy {
    /// Refuse the move and keep the data where it is.
    #[default]
    Error,
    /// Append ` (1)`, ` (2)`, … until the name is free.
    AutoRename,
}

/// Pick a free target name under `dst_dir`. `siblings` are the lowercased
/// names already known to be there (the caller adds an on-disk check before
/// the real move; this pure step keeps the naming rule testable).
pub fn resolve_collision(
    dst_dir: &str,
    name: &str,
    siblings_lower: &[String],
    policy: CollisionPolicy,
) -> Result<String, String> {
    let dir = dst_dir.trim().trim_end_matches('/');
    let name = name.trim();
    if !siblings_lower.iter().any(|s| s == &name.to_lowercase()) {
        return Ok(format!("{dir}/{name}"));
    }
    match policy {
        CollisionPolicy::Error => Err(format!("destination already exists: {dir}/{name}")),
        CollisionPolicy::AutoRename => {
            let (stem, ext) = split_ext(name);
            for n in 1..1000 {
                let candidate = if ext.is_empty() {
                    format!("{stem} ({n})")
                } else {
                    format!("{stem} ({n}).{ext}")
                };
                if !siblings_lower
                    .iter()
                    .any(|s| s == &candidate.to_lowercase())
                {
                    return Ok(format!("{dir}/{candidate}"));
                }
            }
            Err(format!("destination already exists: {dir}/{name}"))
        }
    }
}

fn split_ext(name: &str) -> (&str, &str) {
    // Only a trailing dot + 2–5 ASCII letters counts as a file extension
    // ("Movie.mkv" → "Movie (1).mkv"). Dotted folder names like "Show.S01"
    // keep the suffix at the end ("Show.S01 (1)").
    match name.rfind('.') {
        Some(idx) if idx > 0 => {
            let ext = &name[idx + 1..];
            if (2..=5).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphabetic()) {
                (&name[..idx], ext)
            } else {
                (name, "")
            }
        }
        _ => (name, ""),
    }
}

/// Free-space preflight before a move (or before starting a queued download).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Preflight {
    /// Enough free space (or nothing left to copy).
    Ready,
    /// The daemon/host cannot report free space — the caller shows a warning
    /// and lets the user proceed rather than blocking.
    Unknown,
    /// Not enough free space for the remaining bytes.
    BlockedNoSpace { required: i64, free: i64 },
}

/// `remaining` is `size - done` clamped at zero by the caller; `free` is
/// `d.free_diskspace` preferred, local `statvfs` / volume stats as fallback.
#[must_use]
pub fn preflight(remaining: i64, free: Option<i64>) -> Preflight {
    if remaining <= 0 {
        return Preflight::Ready;
    }
    match free {
        None => Preflight::Unknown,
        Some(free) if free >= remaining => Preflight::Ready,
        Some(free) => Preflight::BlockedNoSpace {
            required: remaining,
            free,
        },
    }
}

/// Crash-safe move journal entry (the move-only slice of FND-07).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveState {
    /// Recorded but not started.
    Pending,
    /// Copy/rename in flight when the host died — needs verify on startup.
    InProgress,
    Done,
    Failed,
    /// The user cancelled; the torrent was resumed in place. Terminal unless
    /// retried (retry drops the entry so the next tick re-plans).
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveOp {
    /// Stable id (`{HASH}-{millis}`), so a restart can correlate entries.
    pub id: String,
    pub hash: String,
    pub name: String,
    /// Daemon-namespace source base path (`d.base_path` at plan time).
    pub src: String,
    /// Daemon-namespace destination base path (dir + collision-resolved name).
    pub dst: String,
    pub state: MoveState,
    /// User-facing failure, if any.
    #[serde(default)]
    pub error: String,
    /// Torrent size at plan time: the progress denominator and the resume
    /// sanity check. Older journals lack it (default 0 = unknown).
    #[serde(default)]
    pub size_bytes: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// The journal itself: a small JSON document (`move-journal.json`) beside the
/// other client state. Bounded — finished entries are pruned by the host.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    #[serde(default)]
    pub ops: Vec<MoveOp>,
}

impl Journal {
    #[must_use]
    pub fn make_id(hash: &str, now_ms: i64) -> String {
        format!("{}-{now_ms}", hash.to_ascii_uppercase())
    }

    /// Record a planned move. Idempotent per hash: an existing pending or
    /// in-progress entry for the hash is returned untouched.
    pub fn add(&mut self, op: MoveOp) -> &MoveOp {
        if let Some(index) = self
            .ops
            .iter()
            .position(|o| o.hash.eq_ignore_ascii_case(&op.hash) && resumable(&o.state))
        {
            return &self.ops[index];
        }
        self.ops.push(op);
        // SAFETY: just pushed.
        self.ops.last().unwrap()
    }

    /// Entries that need work on startup: never-finished moves resume with a
    /// verify of `dst` before anything is erased; failures are retryable.
    #[must_use]
    pub fn resumable(&self) -> Vec<&MoveOp> {
        self.ops.iter().filter(|o| resumable(&o.state)).collect()
    }

    pub fn mark(&mut self, id: &str, state: MoveState, error: &str, now_ms: i64) {
        if let Some(op) = self.ops.iter_mut().find(|o| o.id == id) {
            op.state = state;
            op.error = error.to_string();
            op.updated_at_ms = now_ms;
        }
    }

    /// Drop finished entries; keep the journal bounded.
    pub fn prune_done(&mut self) {
        self.ops.retain(|o| o.state != MoveState::Done);
    }

    /// Serialize for disk.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| r#"{"ops":[]}"#.into())
    }

    /// Parse from disk; a corrupt journal is an empty one (moves are
    /// re-planned from daemon state, never invented from a half-write).
    #[must_use]
    pub fn from_json(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_default()
    }
}

fn resumable(state: &MoveState) -> bool {
    matches!(
        state,
        MoveState::Pending | MoveState::InProgress | MoveState::Failed
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Vec<MoveRule> {
        vec![
            MoveRule::tag_rule("Archive", "/media/archive"),
            MoveRule::label_rule("video", "/media/video"),
        ]
    }

    #[test]
    fn first_matching_tag_wins_then_label_then_default() {
        let rules = rules();
        assert_eq!(
            resolve_destination("video", &["archive".into()], &rules, "/dl"),
            "/media/archive"
        );
        assert_eq!(
            resolve_destination("VIDEO", &[], &rules, "/dl"),
            "/media/video"
        );
        assert_eq!(
            resolve_destination("other", &["other".into()], &rules, "/dl"),
            "/dl"
        );
        // Empty destinations never win.
        let empty = vec![MoveRule::label_rule("video", "")];
        assert_eq!(resolve_destination("video", &[], &empty, "/dl"), "/dl");
        // Hand-written (non-normalised) rules still match case-insensitively.
        let raw = vec![
            MoveRule {
                tag: Some("Archive".into()),
                label: None,
                destination: "/media/archive".into(),
            },
            MoveRule {
                tag: None,
                label: Some("Video".into()),
                destination: "/media/video".into(),
            },
        ];
        assert_eq!(
            resolve_destination("video", &["ARCHIVE".into()], &raw, "/dl"),
            "/media/archive"
        );
        assert_eq!(
            resolve_destination("VIDEO", &[], &raw, "/dl"),
            "/media/video"
        );
    }

    #[test]
    fn routing_a_new_download_records_the_way_home() {
        // Feature off: identity, nothing recorded.
        assert_eq!(route_new_download("", "/dl"), ("/dl".to_string(), None));
        assert_eq!(route_new_download("  ", "/dl"), ("/dl".to_string(), None));
        // Feature on: load into incomplete, remember the chosen path.
        assert_eq!(
            route_new_download("/dl/.incomplete", "/media/video"),
            (
                "/dl/.incomplete".to_string(),
                Some("/media/video".to_string())
            )
        );
        // Already home: no intent to record.
        assert_eq!(
            route_new_download("/dl/.incomplete", "/dl/.incomplete"),
            ("/dl/.incomplete".to_string(), None)
        );
        // Nothing chosen: nothing to record.
        assert_eq!(
            route_new_download("/dl/.incomplete", ""),
            (String::new(), None)
        );
    }

    #[test]
    fn completion_chains_rules_then_label_paths_then_recorded_home() {
        let rules = rules();
        let labels = [("video", "/media/label-video")];
        // An explicit rule beats the C11 label path.
        assert_eq!(
            completion_destination("video", &[], &rules, &labels, "/dl"),
            "/media/video"
        );
        // No rule: the C11 path still guides the completion…
        assert_eq!(
            completion_destination("video", &[], &[], &labels, "/dl"),
            "/media/label-video"
        );
        // …and the recorded final dir is the last resort before the default.
        assert_eq!(
            completion_destination("other", &[], &[], &[], "/media/manual"),
            "/media/manual"
        );
        assert_eq!(completion_destination("other", &[], &[], &[], "/dl"), "/dl");
    }

    #[test]
    fn no_plan_when_already_home_or_missing_inputs() {
        assert_eq!(plan_move("/dl", "/dl", "Show.S01"), None);
        assert_eq!(plan_move("/DL", "/dl", "Show.S01"), None);
        assert_eq!(plan_move("/tmp", "", "Show.S01"), None);
        assert_eq!(plan_move("", "/dl", "Show.S01"), None);
        assert_eq!(
            plan_move("/tmp/.incomplete", "/dl", "Show.S01"),
            Some(MovePlan {
                src: "/tmp/.incomplete/Show.S01".into(),
                dst: "/dl/Show.S01".into(),
            })
        );
    }

    #[test]
    fn collision_errors_by_default_and_renames_on_request() {
        let taken = vec!["show.s01".to_string()];
        assert_eq!(
            resolve_collision("/dl", "Show.S01", &taken, CollisionPolicy::Error),
            Err("destination already exists: /dl/Show.S01".into())
        );
        assert_eq!(
            resolve_collision("/dl", "Show.S01", &taken, CollisionPolicy::AutoRename),
            Ok("/dl/Show.S01 (1)".into())
        );
        assert_eq!(
            resolve_collision("/dl", "Movie.mkv", &taken, CollisionPolicy::Error),
            Ok("/dl/Movie.mkv".into())
        );
        let taken2 = vec!["show.s01".to_string(), "show.s01 (1)".to_string()];
        assert_eq!(
            resolve_collision("/dl", "Show.S01", &taken2, CollisionPolicy::AutoRename),
            Ok("/dl/Show.S01 (2)".into())
        );
    }

    #[test]
    fn collision_policy_serialises_kebab_case_like_the_ts_mirror() {
        assert_eq!(
            serde_json::to_string(&CollisionPolicy::AutoRename).unwrap(),
            r#""auto-rename""#
        );
        assert_eq!(
            serde_json::from_str::<CollisionPolicy>(r#""auto-rename""#).unwrap(),
            CollisionPolicy::AutoRename
        );
        assert_eq!(
            serde_json::from_str::<CollisionPolicy>(r#""error""#).unwrap(),
            CollisionPolicy::Error
        );
    }

    #[test]
    fn preflight_blocks_only_on_known_shortage() {
        assert_eq!(preflight(0, Some(0)), Preflight::Ready);
        assert_eq!(preflight(100, None), Preflight::Unknown);
        assert_eq!(preflight(100, Some(100)), Preflight::Ready);
        assert_eq!(
            preflight(101, Some(100)),
            Preflight::BlockedNoSpace {
                required: 101,
                free: 100
            }
        );
    }

    #[test]
    fn journal_round_trips_and_resumes_unfinished_moves() {
        let mut journal = Journal::default();
        journal.add(MoveOp {
            id: Journal::make_id("AAAA", 1),
            hash: "AAAA".into(),
            name: "Show".into(),
            src: "/incomplete/Show".into(),
            dst: "/dl/Show".into(),
            state: MoveState::Pending,
            error: String::new(),
            size_bytes: 0,
            created_at_ms: 1,
            updated_at_ms: 1,
        });
        // Idempotent per hash while resumable.
        journal.add(MoveOp {
            id: Journal::make_id("AAAA", 2),
            hash: "aaaa".into(),
            name: "Show".into(),
            src: "/x".into(),
            dst: "/y".into(),
            state: MoveState::Pending,
            error: String::new(),
            size_bytes: 0,
            created_at_ms: 2,
            updated_at_ms: 2,
        });
        assert_eq!(journal.ops.len(), 1);
        assert_eq!(journal.resumable().len(), 1);

        let raw = journal.to_json();
        let loaded = Journal::from_json(&raw);
        assert_eq!(loaded, journal);
        // Corrupt input degrades to empty, never a phantom move.
        assert_eq!(Journal::from_json("{nope"), Journal::default());

        let id = journal.ops[0].id.clone();
        journal.mark(&id, MoveState::Done, "", 3);
        assert!(journal.resumable().is_empty());
        journal.prune_done();
        assert!(journal.ops.is_empty());
    }
}
