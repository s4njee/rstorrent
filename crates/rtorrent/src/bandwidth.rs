//! Bandwidth rules by label/tag (V3-18 / QUE-04).
//!
//! Named rate profiles applied by label/tag through rtorrent's named
//! throttles: each rule owns a deterministic `rule_<id>` throttle the hosts
//! define on first use (and re-define on reconnect — rtorrent forgets
//! definitions on restart, like the B4 pool). Peer caps ride alongside via
//! `set_connection_limits`; adoption is recorded in `d.custom=throttle_rule`
//! so manual edits and rule management never mistake each other:
//!
//! * rule matches + marker is another/none → adopt (assign, caps, mark);
//! * rule matches + marker is this rule → steady (no daemon traffic);
//! * no rule + marker set → release (unassign, reset caps, clear marker);
//! * throttle set with no marker, or custom caps with no marker → manual,
//!   the engine keeps its hands off (torrent override wins precedence).
//!
//! Precedence, everywhere it is shown: torrent override → tag/label rule →
//! turtle → global.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::rtorrent::RawTorrent;

/// `d.custom` key recording which rule manages a torrent (empty = none).
pub const RULE_KEY: &str = "throttle_rule";

/// One bandwidth rule: a tag match or a label match plus the profile to
/// apply. Rates are KiB/s (0 = unlimited); caps are daemon units (0 = the
/// daemon default, i.e. explicitly reset on adopt).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BandwidthRule {
    /// Stable id, also the throttle-name suffix (`rule_<sanitised id>`).
    pub id: String,
    /// Lowercased tag this rule fires on. `None` when this is a label rule.
    #[serde(default)]
    pub tag: Option<String>,
    /// Label this rule fires on. `None` when this is a tag rule.
    #[serde(default)]
    pub label: Option<String>,
    pub down_kb: i64,
    pub up_kb: i64,
    #[serde(default)]
    pub peers_max: i64,
    #[serde(default)]
    pub peers_min: i64,
    #[serde(default)]
    pub uploads_max: i64,
}

impl BandwidthRule {
    /// A tag rule; the tag is lowercased so matching is case-insensitive.
    #[must_use]
    pub fn tag_rule(id: &str, tag: &str, down_kb: i64, up_kb: i64) -> Self {
        Self {
            id: id.trim().to_owned(),
            tag: Some(tag.trim().to_lowercase()),
            label: None,
            down_kb,
            up_kb,
            peers_max: 0,
            peers_min: 0,
            uploads_max: 0,
        }
    }

    /// A label rule; matching is case-insensitive like the sidebar.
    #[must_use]
    pub fn label_rule(id: &str, label: &str, down_kb: i64, up_kb: i64) -> Self {
        Self {
            id: id.trim().to_owned(),
            tag: None,
            label: Some(label.trim().to_lowercase()),
            down_kb,
            up_kb,
            peers_max: 0,
            peers_min: 0,
            uploads_max: 0,
        }
    }

    /// The daemon-side throttle name for this rule.
    #[must_use]
    pub fn throttle_name(&self) -> String {
        let mut sanitised: String = self
            .id
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if sanitised.is_empty() {
            sanitised.push_str("rule");
        }
        sanitised.truncate(32);
        format!("rule_{sanitised}")
    }

    /// Human match descriptor for the precedence display (`tag "x"`).
    #[must_use]
    pub fn describe(&self) -> String {
        if let Some(tag) = self.tag.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            return format!("tag \"{tag}\"");
        }
        if let Some(label) = self
            .label
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty())
        {
            return format!("label \"{label}\"");
        }
        "untargeted".to_owned()
    }
}

/// First matching tag rule (in rule order), then the label rule — the same
/// precedence convention as the move rules.
#[must_use]
pub fn rule_for<'a>(
    label: &str,
    tags: &[String],
    rules: &'a [BandwidthRule],
) -> Option<&'a BandwidthRule> {
    let lowered: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    for rule in rules {
        if rule.id.trim().is_empty() {
            continue;
        }
        if let Some(tag) = rule.tag.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            if lowered.iter().any(|t| t == &tag.to_lowercase()) {
                return Some(rule);
            }
        }
    }
    let key = label.trim().to_lowercase();
    if !key.is_empty() {
        for rule in rules {
            if rule.id.trim().is_empty() || rule.tag.is_some() {
                continue;
            }
            if let Some(rule_label) = rule.label.as_deref().map(str::trim) {
                if rule_label.to_lowercase() == key {
                    return Some(rule);
                }
            }
        }
    }
    None
}

/// Where a torrent's limits come from, for the precedence display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LimitSource {
    /// A hand-assigned throttle or hand-set caps: the engine keeps off.
    Manual,
    /// Managed by the rule with this match descriptor.
    Rule(String),
    /// The turtle window is in effect.
    Turtle,
    /// The global limits.
    Global,
}

/// The precedence word the limit rows show.
#[must_use]
pub fn source_word(source: &LimitSource) -> String {
    match source {
        LimitSource::Manual => "torrent override".to_owned(),
        LimitSource::Rule(desc) => format!("rule {desc}"),
        LimitSource::Turtle => "turtle".to_owned(),
        LimitSource::Global => "global".to_owned(),
    }
}

/// Resolve the display source: an explicit throttle with no rule marker is a
/// manual override, and so is a resolved per-torrent limit with neither
/// throttle nor marker (it can only be torrent-owned data); a matching rule
/// owns the display otherwise.
#[must_use]
pub fn limit_source(
    throttle: &str,
    marker: &str,
    has_own_limit: bool,
    rule: Option<&BandwidthRule>,
    turtle_active: bool,
) -> LimitSource {
    if marker.trim().is_empty() && (has_own_limit || !throttle.trim().is_empty()) {
        return LimitSource::Manual;
    }
    if let Some(rule) = rule {
        return LimitSource::Rule(rule.describe());
    }
    if turtle_active {
        return LimitSource::Turtle;
    }
    LimitSource::Global
}

/// One engine verdict for a torrent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BandwidthAction {
    /// Ensure the rule's throttle is defined, assign it, push its caps, and
    /// record the marker.
    Adopt { hash: String, rule: BandwidthRule },
    /// A rule used to manage this torrent and no longer matches: unassign,
    /// reset caps to the daemon default, clear the marker.
    Release { hash: String },
}

/// Per-session engine memory: which throttle names are already defined (so a
/// steady tick costs zero daemon traffic) and which rules already warned.
#[derive(Clone, Debug, Default)]
pub struct BandwidthState {
    defined: HashSet<String>,
    warned: HashSet<String>,
}

impl BandwidthState {
    /// Forget a session: definitions die with the daemon, warnings with it.
    pub fn reset(&mut self) {
        self.defined.clear();
        self.warned.clear();
    }

    /// True when the throttle still needs defining this session.
    pub fn needs_define(&mut self, throttle: &str) -> bool {
        self.defined.insert(throttle.to_owned())
    }

    /// True the first time a rule fails per session (log it); silenced after.
    pub fn should_warn(&mut self, rule_id: &str) -> bool {
        self.warned.insert(rule_id.to_owned())
    }

    /// A later success re-arms the warning.
    pub fn clear_warned(&mut self, rule_id: &str) {
        self.warned.remove(rule_id);
    }
}

///torrent caps at the daemon default (untouched by hand).
fn caps_default(t: &RawTorrent) -> bool {
    t.peers_max == 0 && t.peers_min == 0 && t.uploads_max == 0
}

/// Plan one tick: adopt where a rule newly matches, release where a recorded
/// rule no longer does, silence everywhere else. Pure — hosts execute.
#[must_use]
pub fn plan_bandwidth(raw: &[RawTorrent], rules: &[BandwidthRule]) -> Vec<BandwidthAction> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for t in raw {
        if !seen.insert(t.hash.clone()) {
            continue;
        }
        let rule = rule_for(&t.label, &t.tags, rules);
        let marker = t.throttle_rule.trim();
        match rule {
            Some(rule) if marker != rule.id => {
                // Adopt only clean torrents: anything hand-touched (a
                // throttle with no marker, or custom caps with no marker)
                // is a manual override the engine must not swallow.
                if marker.is_empty() && t.throttle_name.trim().is_empty() && caps_default(t) {
                    out.push(BandwidthAction::Adopt {
                        hash: t.hash.clone(),
                        rule: rule.clone(),
                    });
                } else if !marker.is_empty() {
                    // Retagged (or reconfigured) under a live marker: switch.
                    out.push(BandwidthAction::Adopt {
                        hash: t.hash.clone(),
                        rule: rule.clone(),
                    });
                }
            }
            None if !marker.is_empty() => {
                out.push(BandwidthAction::Release {
                    hash: t.hash.clone(),
                });
            }
            _ => {}
        }
    }
    out
}

/// Snapshot helper for the replay stage: the distinct throttle names the
/// configured rules need defined.
#[must_use]
pub fn rule_throttles(rules: &[BandwidthRule]) -> HashMap<String, (i64, i64)> {
    let mut map = HashMap::new();
    for rule in rules {
        if rule.id.trim().is_empty() {
            continue;
        }
        map.insert(rule.throttle_name(), (rule.down_kb, rule.up_kb));
    }
    map
}

/// Ids whose profile changed between two rule sets (same id, different
/// rates/caps/match). Hosts clear these markers on save so the next tick
/// re-adopts at the new rates; removed ids need nothing (release covers
/// them) and added ids adopt on sight.
#[must_use]
pub fn changed_rule_ids(old: &[BandwidthRule], new: &[BandwidthRule]) -> Vec<String> {
    new.iter()
        .filter(|n| {
            !n.id.trim().is_empty()
                && old.iter().find(|o| o.id == n.id).is_some_and(|o| {
                    o.down_kb != n.down_kb
                        || o.up_kb != n.up_kb
                        || o.peers_max != n.peers_max
                        || o.peers_min != n.peers_min
                        || o.uploads_max != n.uploads_max
                        || o.tag != n.tag
                        || o.label != n.label
                })
        })
        .map(|n| n.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rules() -> Vec<BandwidthRule> {
        vec![
            BandwidthRule::tag_rule("archive", "Archive", 512, 128),
            BandwidthRule::label_rule("vid", "video", 1024, 256),
        ]
    }

    fn torrent(hash: &str, label: &str, tags: &[&str]) -> RawTorrent {
        RawTorrent {
            hash: hash.into(),
            name: hash.into(),
            label: label.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..RawTorrent::default()
        }
    }

    #[test]
    fn first_matching_tag_wins_then_label() {
        let rules = sample_rules();
        assert_eq!(
            rule_for("video", &["archive".into()], &rules).map(|r| r.id.as_str()),
            Some("archive")
        );
        assert_eq!(
            rule_for("VIDEO", &[], &rules).map(|r| r.id.as_str()),
            Some("vid")
        );
        assert_eq!(rule_for("other", &[], &rules), None);
        // Rules without ids never match.
        let mut bad = rules;
        bad[0].id.clear();
        assert_eq!(
            rule_for("x", &["archive".into()], &bad).map(|r| r.id.as_str()),
            None
        );
    }

    #[test]
    fn throttle_names_are_stable_and_safe() {
        let rule = BandwidthRule::tag_rule("My Rule!", "x", 1, 1);
        assert_eq!(rule.throttle_name(), "rule_my_rule_");
        assert_eq!(rule.describe(), "tag \"x\"");
        let label = BandwidthRule::label_rule("v", "Video", 1, 1);
        assert_eq!(label.describe(), "label \"video\"");
    }

    #[test]
    fn precedence_puts_manual_first() {
        let rules = sample_rules();
        let rule = rule_for("video", &[], &rules);
        assert_eq!(
            limit_source("rstorrent_1", "", false, rule, false),
            LimitSource::Manual
        );
        assert_eq!(limit_source("", "", true, None, false), LimitSource::Manual);
        assert_eq!(
            limit_source("rule_vid", "vid", true, rule, true),
            LimitSource::Rule("label \"video\"".into())
        );
        assert_eq!(source_word(&LimitSource::Manual), "torrent override");
        assert_eq!(
            source_word(&LimitSource::Rule("tag \"x\"".into())),
            "rule tag \"x\""
        );
        assert_eq!(source_word(&LimitSource::Turtle), "turtle");
        assert_eq!(source_word(&LimitSource::Global), "global");
        assert_eq!(limit_source("", "", false, None, true), LimitSource::Turtle);
        assert_eq!(
            limit_source("", "", false, None, false),
            LimitSource::Global
        );
    }

    #[test]
    fn clean_torrents_adopt_manual_ones_are_left_alone() {
        let rules = sample_rules();
        let clean = torrent("A", "video", &[]);
        let mut manual = torrent("B", "video", &[]);
        manual.throttle_name = "rstorrent_1".into();
        let mut capped = torrent("C", "video", &[]);
        capped.peers_max = 10;
        let mut marked = torrent("D", "video", &[]);
        marked.throttle_rule = "vid".into();
        marked.throttle_name = "rule_vid".into();
        let raw = vec![clean, manual, capped, marked];
        let actions = plan_bandwidth(&raw, &rules);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            BandwidthAction::Adopt { hash, rule } => {
                assert_eq!(hash, "A");
                assert_eq!(rule.id, "vid");
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn removed_rules_release_their_torrents() {
        // Label matches nothing now, but the stale marker says a rule used
        // to manage it.
        let mut orphaned = torrent("A", "other", &[]);
        orphaned.throttle_rule = "gone".into();
        orphaned.throttle_name = "rule_gone".into();
        let actions = plan_bandwidth(&[orphaned], &sample_rules());
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], BandwidthAction::Release { .. }));
    }

    #[test]
    fn retagging_switches_rules() {
        let mut t = torrent("A", "other", &["archive"]);
        t.throttle_rule = "vid".into();
        t.throttle_name = "rule_vid".into();
        let actions = plan_bandwidth(&[t], &sample_rules());
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            BandwidthAction::Adopt { hash, rule } => {
                assert_eq!(hash, "A");
                assert_eq!(rule.id, "archive");
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn duplicates_plan_once() {
        let t = torrent("A", "video", &[]);
        let actions = plan_bandwidth(&[t.clone(), t], &sample_rules());
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn rule_throttles_collect_distinct_definitions() {
        let map = rule_throttles(&sample_rules());
        assert_eq!(map.get("rule_archive"), Some(&(512, 128)));
        assert_eq!(map.get("rule_vid"), Some(&(1024, 256)));
        assert_eq!(rule_throttles(&[]).len(), 0);
    }

    #[test]
    fn warn_once_then_rearm() {
        let mut state = BandwidthState::default();
        assert!(state.should_warn("r1"));
        assert!(!state.should_warn("r1"));
        state.clear_warned("r1");
        assert!(state.should_warn("r1"));
        assert!(state.needs_define("rule_x"));
        assert!(!state.needs_define("rule_x"));
        state.reset();
        assert!(state.needs_define("rule_x"));
    }

    #[test]
    fn changed_rule_ids_spots_reprofiles_only() {
        let old = sample_rules();
        // Identical: nothing.
        assert!(changed_rule_ids(&old, &old.clone()).is_empty());
        // Retargeted match on the same id: changed.
        let mut retargeted = old.clone();
        retargeted[0].tag = Some("other".into());
        assert_eq!(changed_rule_ids(&old, &retargeted), vec!["archive"]);
        // New and removed ids are not "changed": adoption and release
        // already cover them.
        let mut added = old.clone();
        added.push(BandwidthRule::tag_rule("new", "n", 1, 1));
        assert!(changed_rule_ids(&old, &added).is_empty());
        assert!(changed_rule_ids(&added, &old).is_empty());
    }
}
