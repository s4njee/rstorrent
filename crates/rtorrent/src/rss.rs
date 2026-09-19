//! RSS/Atom auto-download rules 2.0 (V3-23 / AUT-02).
//!
//! The single home for feed parsing, rule matching and plan computation; the
//! Tauri engine and the GPUI runner are thin hosts around it (fetch → plan →
//! add → persist seen). Previously this lived in `src-tauri/src/rss.rs` with
//! word-list matching only; it now covers regex, season/episode ranges,
//! size bounds, quality preference with smart-episode dedupe, target
//! label/tags/path, start/top policy and per-rule intervals.
//!
//! Matching is pure over `(Rule, FeedItem)` and every clause explains
//! itself, so the preview UI shows *why* an item matched or not without
//! reimplementing anything. Feed fetching is async over plain HTTPS (the
//! crate already depends on reqwest for the daemon HTTP transport).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Ceiling on the persisted seen-id set, so it can't grow without bound.
pub const SEEN_CAP: usize = 2000;

/// An RSS/Atom feed polled for auto-add. Wire-identical to the historical
/// host shapes (camelCase JSON), so existing settings files load unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// An auto-download rule. Every V3-23 filter is optional — an empty rule
/// matches everything, exactly like the B11 word lists did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Feed id this rule applies to; empty = every feed.
    #[serde(default)]
    pub feed_id: String,
    /// Whitespace-separated tokens that must *all* appear in the title
    /// (case-insensitive). Empty matches everything.
    #[serde(default)]
    pub must_contain: String,
    /// Whitespace-separated tokens; if *any* appears, the item is skipped.
    #[serde(default)]
    pub must_not_contain: String,
    /// Optional regular expression the title must match (case-insensitive).
    /// Invalid patterns never match — the preview names the error instead
    /// of silently dropping everything.
    #[serde(default)]
    pub regex_match: String,
    /// Season selector: `2`, `1-3`, `1,4-6`. Empty = any season.
    #[serde(default)]
    pub season_range: String,
    /// Episode selector, same shape. Empty = any episode.
    #[serde(default)]
    pub episode_range: String,
    /// Size bounds in MiB; 0 = no bound. Items with unknown size fail a set
    /// bound rather than slipping through.
    #[serde(default)]
    pub min_size_mb: i64,
    #[serde(default)]
    pub max_size_mb: i64,
    /// Quality token preferred when several items share an episode
    /// (e.g. `1080p`); empty = fixed ranking only.
    #[serde(default)]
    pub prefer_quality: String,
    /// Keep only the best-scoring item per (series, season, episode) within
    /// one poll. Cross-poll upgrades still add (their guids are unseen) —
    /// see the module docs.
    #[serde(default)]
    pub smart_episode: bool,
    #[serde(default)]
    pub label: String,
    /// Comma-separated tags applied on add.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub save_path: String,
    /// Start the download immediately (false = add stopped).
    #[serde(default = "default_true")]
    pub start: bool,
    /// Pin to the top of the queue on add.
    #[serde(default)]
    pub top_of_queue: bool,
    /// Poll cadence for this rule in minutes; 0 = the global interval, and
    /// a non-positive global disables background polling entirely.
    #[serde(default)]
    pub poll_minutes: i64,
}

/// One parsed feed entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    pub title: String,
    /// The download URL: a magnet link or a `.torrent` URL (enclosure preferred).
    pub link: String,
    /// Stable identity for dedup (`guid`/`id`, or the link as a fallback).
    pub guid: String,
    pub pub_date: String,
    /// Enclosure length in bytes, when the feed states one.
    #[serde(default)]
    pub size: Option<i64>,
}

fn default_true() -> bool {
    true
}

// --- Fetch -----------------------------------------------------------------

/// Fetch and parse a feed. Errors are user-facing strings (shown in the RSS UI).
pub async fn fetch(url: &str) -> Result<Vec<FeedItem>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("rstorrent")
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let text = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("{url}: {e}"))?
        .error_for_status()
        .map_err(|e| format!("{url}: {e}"))?
        .text()
        .await
        .map_err(|e| format!("reading {url}: {e}"))?;
    Ok(parse_feed(&text))
}

// --- Episode parsing ---------------------------------------------------------

/// A detected episode reference: season may be absent (`Ep 12`-style is not
/// currently detected — only SxxExx / NxNN forms).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Episode {
    pub season: Option<u32>,
    pub episode: u32,
}

/// Find `S01E02`, `S01E02E03` (start), `1x02` in a title (case-insensitive).
#[must_use]
pub fn parse_episode(title: &str) -> Option<Episode> {
    let bytes = title.as_bytes();
    let lower: Vec<u8> = bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut i = 0;
    while i < lower.len() {
        // SxxExx — allow separators (`.`, ` `, `-`, `_`) around the parts.
        if lower[i] == b's' {
            if let Some((season, after_s)) = read_num(&lower, i + 1) {
                let j = skip_seps(&lower, after_s);
                if j < lower.len() && lower[j] == b'e' {
                    if let Some((episode, _)) = read_num(&lower, j + 1) {
                        return Some(Episode {
                            season: Some(season),
                            episode,
                        });
                    }
                }
            }
        }
        // NxNN.
        if lower[i].is_ascii_digit() {
            if let Some((season, after_s)) = read_num(&lower, i) {
                if after_s < lower.len() && (lower[after_s] == b'x') {
                    if let Some((episode, _)) = read_num(&lower, after_s + 1) {
                        // Guard against years/dimensions: seasons are small.
                        if season <= 60 {
                            return Some(Episode {
                                season: Some(season),
                                episode,
                            });
                        }
                    }
                }
            }
        }
        i += 1;
    }
    None
}

fn read_num(lower: &[u8], mut i: usize) -> Option<(u32, usize)> {
    let start = i;
    let mut value: u32 = 0;
    while i < lower.len() && lower[i].is_ascii_digit() {
        value = value
            .saturating_mul(10)
            .saturating_add((lower[i] - b'0') as u32);
        i += 1;
    }
    if i == start {
        return None;
    }
    Some((value, i))
}

fn skip_seps(lower: &[u8], mut i: usize) -> usize {
    while i < lower.len() && matches!(lower[i], b'.' | b' ' | b'-' | b'_') {
        i += 1;
    }
    i
}

/// Parse `2`, `1-3` or `1,4-6` into spans. Unparseable → `None` (a broken
/// filter matches nothing, loudly, instead of everything silently).
fn parse_spans(raw: &str) -> Option<Vec<(u32, u32)>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut spans = Vec::new();
    for part in trimmed.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return None;
        }
        if let Some((lo, hi)) = part.split_once('-') {
            let lo: u32 = lo.trim().parse().ok()?;
            let hi: u32 = hi.trim().parse().ok()?;
            if lo > hi {
                return None;
            }
            spans.push((lo, hi));
        } else {
            let value: u32 = part.parse().ok()?;
            spans.push((value, value));
        }
    }
    Some(spans)
}

fn in_spans(spans: &[(u32, u32)], value: u32) -> bool {
    spans.iter().any(|&(lo, hi)| value >= lo && value <= hi)
}

// --- Quality -----------------------------------------------------------------

/// Rank a release for smart-episode picks: resolution first, source second.
/// Unknown tokens score 0 — they lose to anything recognisable.
#[must_use]
pub fn quality_score(title: &str) -> u32 {
    let lower = title.to_lowercase();
    let resolution = if lower.contains("2160p") || lower.contains("4k") {
        400
    } else if lower.contains("1080p") {
        300
    } else if lower.contains("720p") {
        200
    } else if lower.contains("480p") {
        100
    } else {
        0
    };
    let source = if lower.contains("bluray") || lower.contains("bdremux") || lower.contains("remux")
    {
        30
    } else if lower.contains("web-dl")
        || lower.contains("webdl")
        || lower.contains("webrip")
        || lower.contains(" web ")
    {
        20
    } else if lower.contains("hdtv") {
        10
    } else if lower.contains("dvdrip") || lower.contains("dvd") {
        5
    } else {
        0
    };
    resolution + source
}

/// Grouping key for smart-episode dedupe: the title with episode markers,
/// resolution and source tokens stripped, separators collapsed.
#[must_use]
pub fn episode_key(title: &str) -> String {
    let mut lower = title.to_lowercase();
    // Strip SxxExx / NxNN markers.
    loop {
        let before = lower.clone();
        lower = regex_strip_episode(&lower);
        if lower == before {
            break;
        }
    }
    for token in [
        "2160p", "1080p", "720p", "480p", "360p", "4k", "bluray", "bdremux", "remux", "web-dl",
        "webdl", "webrip", "hdtv", "dvdrip", "dvd", "x264", "x265", "h264", "h265", "hevc", "aac",
        "dts", "proper", "repack",
    ] {
        lower = lower.replace(token, " ");
    }
    lower
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Remove one episode marker occurrence without pulling in the regex crate
/// for a single simple pattern (the crate is used for user patterns below).
fn regex_strip_episode(lower: &str) -> String {
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b's' {
            if let Some((_, after_s)) = read_num(bytes, i + 1) {
                let j = skip_seps(bytes, after_s);
                if j < bytes.len() && bytes[j] == b'e' {
                    if let Some((_, after_e)) = read_num(bytes, j + 1) {
                        let mut out = lower[..i].to_owned();
                        out.push(' ');
                        out.push_str(&lower[after_e..]);
                        return out;
                    }
                }
            }
        }
        if bytes[i].is_ascii_digit() {
            if let Some((season, after_s)) = read_num(bytes, i) {
                if after_s < bytes.len() && bytes[after_s] == b'x' && season <= 60 {
                    if let Some((_, after_e)) = read_num(bytes, after_s + 1) {
                        let mut out = lower[..i].to_owned();
                        out.push(' ');
                        out.push_str(&lower[after_e..]);
                        return out;
                    }
                }
            }
        }
        i += 1;
    }
    lower.to_owned()
}

// --- Matching ----------------------------------------------------------------

/// One clause of the verdict, for the "why did this (not) match" preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clause {
    pub label: String,
    pub passed: bool,
    pub detail: String,
}

/// Check a user pattern compiles (case-insensitive, as matching applies
/// it). Empty patterns are vacuously valid.
pub fn validate_regex(pattern: &str) -> Result<(), String> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    regex::Regex::new(&format!("(?i){trimmed}"))
        .map(|_| ())
        .map_err(|err| {
            format!(
                "invalid regex: {}",
                err.to_string().split('\n').next().unwrap_or("")
            )
        })
}

/// A rule's verdict on one item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Match {
        clauses: Vec<Clause>,
    },
    NoMatch {
        clauses: Vec<Clause>,
    },
    /// The rule itself is broken (invalid regex): never matches, and the
    /// preview names the error instead of silently dropping everything.
    RuleError(String),
}

/// Explain a rule against an item, clause by clause.
#[must_use]
pub fn explain(rule: &Rule, item: &FeedItem) -> Verdict {
    let mut clauses = Vec::new();
    let hay = item.title.to_lowercase();

    let missing: Vec<&str> = rule
        .must_contain
        .split_whitespace()
        .filter(|tok| !hay.contains(&tok.to_lowercase()))
        .collect();
    clauses.push(Clause {
        label: "required words".into(),
        passed: missing.is_empty(),
        detail: if missing.is_empty() {
            if rule.must_contain.trim().is_empty() {
                "no required words".into()
            } else {
                format!("all of '{}' present", rule.must_contain.trim())
            }
        } else {
            format!("missing {}", missing.join(", "))
        },
    });

    let hit = rule
        .must_not_contain
        .split_whitespace()
        .find(|tok| hay.contains(&tok.to_lowercase()))
        .map(str::to_string);
    clauses.push(Clause {
        label: "excluded words".into(),
        passed: hit.is_none(),
        detail: match hit {
            Some(tok) => format!("excluded by '{tok}'"),
            None => "nothing excluded".into(),
        },
    });

    if !rule.regex_match.trim().is_empty() {
        let pattern = format!("(?i){}", rule.regex_match.trim());
        match regex::Regex::new(&pattern) {
            Ok(re) => {
                let matched = re.is_match(&item.title);
                clauses.push(Clause {
                    label: "regex".into(),
                    passed: matched,
                    detail: if matched {
                        format!("'{}' matches", rule.regex_match.trim())
                    } else {
                        format!("'{}' does not match", rule.regex_match.trim())
                    },
                });
            }
            Err(err) => {
                return Verdict::RuleError(format!(
                    "invalid regex '{}': {}",
                    rule.regex_match.trim(),
                    err.to_string().split('\n').next().unwrap_or("")
                ));
            }
        }
    }

    // Season / episode constraints share one parse of the title.
    let season_filter = parse_spans(&rule.season_range);
    let episode_filter = parse_spans(&rule.episode_range);
    if rule.season_range.trim().is_empty() && rule.episode_range.trim().is_empty() {
        // No constraint — but a broken non-empty filter must fail loudly.
    } else if season_filter.is_none() && !rule.season_range.trim().is_empty()
        || episode_filter.is_none() && !rule.episode_range.trim().is_empty()
    {
        clauses.push(Clause {
            label: "episode filter".into(),
            passed: false,
            detail: "unparseable season/episode range (try 1-3 or 2,4)".into(),
        });
    } else {
        match parse_episode(&item.title) {
            None => clauses.push(Clause {
                label: "episode filter".into(),
                passed: false,
                detail: "no season/episode info in the title".into(),
            }),
            Some(ep) => {
                if let Some(spans) = season_filter {
                    let season = ep.season.unwrap_or(0);
                    clauses.push(Clause {
                        label: "season".into(),
                        passed: in_spans(&spans, season),
                        detail: format!("found S{season:02}"),
                    });
                }
                if let Some(spans) = episode_filter {
                    clauses.push(Clause {
                        label: "episode".into(),
                        passed: in_spans(&spans, ep.episode),
                        detail: format!("found E{:02}", ep.episode),
                    });
                }
            }
        }
    }

    const MIB: i64 = 1024 * 1024;
    if rule.min_size_mb > 0 || rule.max_size_mb > 0 {
        match item.size {
            None => clauses.push(Clause {
                label: "size".into(),
                passed: false,
                detail: "feed states no size".into(),
            }),
            Some(bytes) => {
                let mb = bytes as f64 / MIB as f64;
                let mut ok = true;
                let mut parts = Vec::new();
                if rule.min_size_mb > 0 {
                    let need = rule.min_size_mb as f64;
                    ok &= mb >= need;
                    parts.push(format!("≥ {need:.0} MiB"));
                }
                if rule.max_size_mb > 0 {
                    let cap = rule.max_size_mb as f64;
                    ok &= mb <= cap;
                    parts.push(format!("≤ {cap:.0} MiB"));
                }
                clauses.push(Clause {
                    label: "size".into(),
                    passed: ok,
                    detail: format!("{mb:.1} MiB ({})", parts.join(", ")),
                });
            }
        }
    }

    if clauses.iter().all(|c| c.passed) {
        Verdict::Match { clauses }
    } else {
        Verdict::NoMatch { clauses }
    }
}

/// The historical boolean check, kept for callers that only need it.
#[must_use]
pub fn rule_matches(rule: &Rule, item: &FeedItem) -> bool {
    matches!(explain(rule, item), Verdict::Match { .. })
}

// --- Planning ------------------------------------------------------------------

/// One vetted download for the host to add.
pub struct PlannedAdd<'a> {
    pub rule: &'a Rule,
    pub item: &'a FeedItem,
}

/// Per-rule poll memory, held by the host loop.
#[derive(Clone, Debug, Default)]
pub struct EngineState {
    /// Last poll per rule id (unix ms). Absent = never polled = due now.
    pub last_run: HashMap<String, i64>,
    /// `(rule id, regex)` pairs already warned about, so a broken pattern
    /// logs once per session rather than every tick.
    pub warned_bad_regex: HashSet<(String, String)>,
    /// Warnings not yet delivered to the host log.
    pending_warnings: Vec<(String, String)>,
}

impl EngineState {
    /// Forget everything (effectively: everything is due).
    pub fn reset(&mut self) {
        self.last_run.clear();
        self.warned_bad_regex.clear();
        self.pending_warnings.clear();
    }

    /// Record a bad-regex warning unless this rule already warned;
    /// returns true when the host should log it now.
    fn warn_once(&mut self, rule_id: &str, pattern: &str) -> bool {
        if self.warned_bad_regex.iter().any(|(id, _)| id == rule_id) {
            return false;
        }
        self.warned_bad_regex
            .insert((rule_id.to_owned(), pattern.to_owned()));
        self.pending_warnings
            .push((rule_id.to_owned(), pattern.to_owned()));
        true
    }

    /// Drain undelivered warnings for the host log.
    pub fn take_warnings(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.pending_warnings)
    }

    /// Whether a rule may poll now: its own interval else the global one; a
    /// non-positive effective interval never polls in the background.
    #[must_use]
    pub fn due(&self, rule: &Rule, global_interval_min: i64, now_ms: i64) -> bool {
        if !rule.enabled {
            return false;
        }
        let wait = interval_ms(rule, global_interval_min);
        if wait <= 0 {
            return false;
        }
        match self.last_run.get(&rule.id) {
            None => true,
            Some(&last) => now_ms - last >= wait,
        }
    }
}

/// Interval this rule polls on: its own, else the global. Non-positive means
/// "do not poll in the background".
fn interval_ms(rule: &Rule, global_interval_min: i64) -> i64 {
    let mins = if rule.poll_minutes > 0 {
        rule.poll_minutes
    } else {
        global_interval_min
    };
    mins.max(0).saturating_mul(60_000)
}

/// Decide one engine pass over already-fetched feeds: which unseen items
/// each due rule takes. Smart-episode rules keep the best item per
/// (series, season, episode); losers stay unseen for later polls.
pub fn plan<'a>(
    feeds: &'a [(Feed, Vec<FeedItem>)],
    rules: &'a [Rule],
    seen: &HashSet<String>,
    state: &mut EngineState,
    global_interval_min: i64,
    now_ms: i64,
) -> Vec<PlannedAdd<'a>> {
    let mut planned = Vec::new();
    for rule in rules.iter().filter(|r| r.enabled) {
        let wait = interval_ms(rule, global_interval_min);
        if wait <= 0 {
            continue;
        }
        let last = state.last_run.get(&rule.id).copied();
        // Absent = never polled = due now; a recorded run gates the wait.
        if matches!(last, Some(last) if now_ms - last < wait) {
            continue;
        }
        state.last_run.insert(rule.id.clone(), now_ms);

        // Candidates: in-scope feeds, unseen guids, matching verdicts.
        let mut candidates: Vec<&'a FeedItem> = Vec::new();
        for (feed, items) in feeds {
            if !feed.enabled || (!rule.feed_id.is_empty() && rule.feed_id != feed.id) {
                continue;
            }
            for item in items {
                if seen.contains(&item.guid) {
                    continue;
                }
                match explain(rule, item) {
                    Verdict::Match { .. } => candidates.push(item),
                    Verdict::NoMatch { .. } => {}
                    Verdict::RuleError(_) => {
                        // Warn once per rule; the item is skipped.
                        state.warn_once(&rule.id, &rule.regex_match);
                    }
                }
            }
        }
        if rule.smart_episode {
            // Best score per episode key wins; losers stay unseen for later
            // polls. Feed order is preserved by the second pass.
            planned.extend(candidates_deduped(
                rule,
                feeds,
                seen,
                &best_keys(rule, feeds, seen),
            ));
        } else {
            planned.extend(candidates.into_iter().map(|item| PlannedAdd { rule, item }));
        }
    }
    planned
}

/// Helper: the set of winner guids for a smart rule over this tick's data.
fn best_keys(
    rule: &Rule,
    feeds: &[(Feed, Vec<FeedItem>)],
    seen: &HashSet<String>,
) -> HashSet<String> {
    let prefer = rule.prefer_quality.trim().to_lowercase();
    let mut best: HashMap<String, (String, u32)> = HashMap::new();
    for (feed, items) in feeds {
        if !feed.enabled || (!rule.feed_id.is_empty() && rule.feed_id != feed.id) {
            continue;
        }
        for item in items {
            if seen.contains(&item.guid) {
                continue;
            }
            if !matches!(explain(rule, item), Verdict::Match { .. }) {
                continue;
            }
            let key = match parse_episode(&item.title) {
                Some(ep) => format!(
                    "{}|{}|{}",
                    episode_key(&item.title),
                    ep.season.unwrap_or(0),
                    ep.episode
                ),
                None => format!("nogroup|{}", item.guid),
            };
            let mut score = quality_score(&item.title);
            if !prefer.is_empty() && item.title.to_lowercase().contains(&prefer) {
                score = score.saturating_add(1000);
            }
            best.entry(key)
                .and_modify(|e| {
                    if score > e.1 {
                        *e = (item.guid.clone(), score);
                    }
                })
                .or_insert((item.guid.clone(), score));
        }
    }
    best.into_values().map(|(guid, _)| guid).collect()
}

/// Feed-order walk emitting only the winning guids.
fn candidates_deduped<'a>(
    rule: &'a Rule,
    feeds: &'a [(Feed, Vec<FeedItem>)],
    seen: &HashSet<String>,
    winners: &HashSet<String>,
) -> Vec<PlannedAdd<'a>> {
    let mut out = Vec::new();
    for (feed, items) in feeds {
        if !feed.enabled || (!rule.feed_id.is_empty() && rule.feed_id != feed.id) {
            continue;
        }
        for item in items {
            if winners.contains(&item.guid) && !seen.contains(&item.guid) {
                out.push(PlannedAdd { rule, item });
            }
        }
    }
    out
}

// --- Seen persistence ----------------------------------------------------------

/// Load guids. Accepts the historical bare array of strings and the newer
/// `{"seen": [...]}` envelope (which leaves room for metadata later).
#[must_use]
pub fn load_seen(path: &Path) -> Vec<String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return Vec::new(),
    };
    if let Ok(list) = serde_json::from_str::<Vec<String>>(&text) {
        return list;
    }
    serde_json::from_str::<SeenFile>(&text)
        .map(|file| file.seen)
        .unwrap_or_default()
}

#[derive(Serialize, Deserialize)]
struct SeenFile {
    #[serde(default)]
    seen: Vec<String>,
}

/// Persist guids, creating the parent directory as needed, capped so the
/// file cannot grow without bound (oldest first).
pub fn save_seen(path: &Path, seen: &[String]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut capped = seen.to_vec();
    if capped.len() > SEEN_CAP {
        capped.drain(0..capped.len() - SEEN_CAP);
    }
    if let Ok(text) = serde_json::to_string(&capped) {
        let _ = std::fs::write(path, text);
    }
}

// --- Feed parsing (ported from the Tauri engine) ---------------------------------

/// Accumulates one item as the parser walks its child elements.
#[derive(Default)]
struct Building {
    title: String,
    guid: String,
    pub_date: String,
    size: Option<i64>,
    /// A torrent enclosure / `rel="enclosure"` link — the preferred download URL.
    enclosure: Option<String>,
    /// A plain `<link>` (RSS text or Atom `href`) — the fallback download URL.
    plain: Option<String>,
}

impl Building {
    fn finish(self) -> Option<FeedItem> {
        let link = self.enclosure.or(self.plain).unwrap_or_default();
        if link.is_empty() {
            return None;
        }
        let guid = if self.guid.is_empty() {
            link.clone()
        } else {
            self.guid
        };
        Some(FeedItem {
            title: self.title,
            link,
            guid,
            pub_date: self.pub_date,
            size: self.size,
        })
    }
}

/// Which text field the parser is currently inside.
#[derive(Clone, Copy, PartialEq)]
enum Field {
    None,
    Title,
    Guid,
    PubDate,
    /// An RSS `<link>` whose URL is its text content.
    RssLink,
}

/// Parse an RSS 2.0 or Atom document into items. Unknown/garbage input yields
/// an empty list rather than an error.
pub fn parse_feed(xml: &str) -> Vec<FeedItem> {
    use quick_xml::events::Event;
    use quick_xml::Reader;
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut items = Vec::new();
    let mut cur: Option<Building> = None;
    let mut field = Field::None;

    loop {
        match reader.read_event_into(&mut buf) {
            // `<enclosure/>` and Atom `<link/>` are self-closing, so they arrive
            // as `Empty`, not `Start`; both open an element, so handle them alike.
            // An `Empty` element has no text, so it never leaves a capture field
            // set (open_element returns None for it).
            Ok(Event::Start(e)) => field = open_element(&mut cur, &e),
            Ok(Event::Empty(e)) => {
                open_element(&mut cur, &e);
                field = Field::None;
            }
            Ok(Event::Text(e)) => {
                if let Some(b) = cur.as_mut() {
                    let text = e.unescape().unwrap_or_default();
                    let text = text.trim();
                    if text.is_empty() {
                        continue;
                    }
                    match field {
                        Field::Title => push_str(&mut b.title, text),
                        Field::Guid => push_str(&mut b.guid, text),
                        Field::PubDate => push_str(&mut b.pub_date, text),
                        Field::RssLink => {
                            if b.plain.is_none() {
                                b.plain = Some(text.to_string());
                            }
                        }
                        Field::None => {}
                    }
                }
            }
            Ok(Event::CData(e)) => {
                // Titles are commonly wrapped in CDATA.
                if let Some(b) = cur.as_mut() {
                    let text = String::from_utf8_lossy(&e).trim().to_string();
                    if !text.is_empty() {
                        match field {
                            Field::Title => push_str(&mut b.title, &text),
                            Field::Guid => push_str(&mut b.guid, &text),
                            Field::RssLink if b.plain.is_none() => b.plain = Some(text),
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.name().as_ref());
                if name == "item" || name == "entry" {
                    if let Some(item) = cur.take().and_then(Building::finish) {
                        items.push(item);
                    }
                    field = Field::None;
                } else {
                    field = Field::None;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    items
}

/// Process an opening element (Start or Empty): begin an item, capture link /
/// enclosure attributes, and return which text field (if any) the element's
/// content should fill.
fn open_element(cur: &mut Option<Building>, e: &quick_xml::events::BytesStart) -> Field {
    match local_name(e.name().as_ref()).as_str() {
        "item" | "entry" => {
            *cur = Some(Building::default());
            Field::None
        }
        "title" => Field::Title,
        "guid" | "id" => Field::Guid,
        "pubdate" | "published" | "updated" => Field::PubDate,
        "enclosure" => {
            if let Some(b) = cur.as_mut() {
                let (url, ty, length) = enclosure_attrs(e);
                if let Some(url) = url {
                    if looks_like_torrent(&url, ty.as_deref()) {
                        b.enclosure = Some(url);
                        b.size = length;
                    }
                }
            }
            Field::None
        }
        "link" => {
            // Atom `<link href=… rel=…>` carries the URL in an attribute; RSS
            // `<link>` carries it as text (→ capture via Field::RssLink).
            if let Some(b) = cur.as_mut() {
                let (href, rel, length) = link_attrs(e);
                match href {
                    Some(href) => {
                        if rel.as_deref() == Some("enclosure") {
                            b.enclosure = Some(href);
                            if b.size.is_none() {
                                b.size = length;
                            }
                        } else if b.plain.is_none() {
                            b.plain = Some(href);
                        }
                        Field::None
                    }
                    None => Field::RssLink,
                }
            } else {
                Field::None
            }
        }
        _ => Field::None,
    }
}

fn push_str(dst: &mut String, text: &str) {
    if dst.is_empty() {
        dst.push_str(text);
    }
}

/// Lower-cased local name (namespace prefix stripped).
fn local_name(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    let local = s.rsplit(':').next().unwrap_or(&s);
    local.to_lowercase()
}

/// A download URL is torrent-ish when it's a magnet, ends in `.torrent`, or the
/// enclosure type says so.
fn looks_like_torrent(url: &str, ty: Option<&str>) -> bool {
    let u = url.to_lowercase();
    u.starts_with("magnet:")
        || u.split(['?', '#'])
            .next()
            .unwrap_or(&u)
            .ends_with(".torrent")
        || ty
            .map(|t| t.to_lowercase().contains("bittorrent"))
            .unwrap_or(false)
}

fn enclosure_attrs(
    e: &quick_xml::events::BytesStart,
) -> (Option<String>, Option<String>, Option<i64>) {
    let mut url = None;
    let mut ty = None;
    let mut length = None;
    for attr in e.attributes().flatten() {
        match local_name(attr.key.as_ref()).as_str() {
            "url" => url = Some(attr_value(&attr)),
            "type" => ty = Some(attr_value(&attr)),
            "length" => length = attr_value(&attr).parse::<i64>().ok(),
            _ => {}
        }
    }
    (url, ty, length)
}

fn link_attrs(e: &quick_xml::events::BytesStart) -> (Option<String>, Option<String>, Option<i64>) {
    let mut href = None;
    let mut rel = None;
    let mut length = None;
    for attr in e.attributes().flatten() {
        match local_name(attr.key.as_ref()).as_str() {
            "href" => href = Some(attr_value(&attr)),
            "rel" => rel = Some(attr_value(&attr).to_lowercase()),
            "length" => length = attr_value(&attr).parse::<i64>().ok(),
            _ => {}
        }
    }
    (href, rel, length)
}

fn attr_value(attr: &quick_xml::events::attributes::Attribute) -> String {
    String::from_utf8_lossy(&attr.value).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> Rule {
        Rule {
            id: "r".into(),
            name: "r".into(),
            enabled: true,
            feed_id: String::new(),
            must_contain: String::new(),
            must_not_contain: String::new(),
            regex_match: String::new(),
            season_range: String::new(),
            episode_range: String::new(),
            min_size_mb: 0,
            max_size_mb: 0,
            prefer_quality: String::new(),
            smart_episode: false,
            label: String::new(),
            tags: Vec::new(),
            save_path: String::new(),
            start: true,
            top_of_queue: false,
            poll_minutes: 0,
        }
    }

    fn item(title: &str) -> FeedItem {
        FeedItem {
            title: title.into(),
            link: "magnet:?xt=urn:btih:aa".into(),
            guid: title.into(),
            pub_date: String::new(),
            size: None,
        }
    }

    fn passed(verdict: &Verdict) -> bool {
        matches!(verdict, Verdict::Match { .. })
    }

    #[test]
    fn word_lists_keep_their_b11_meaning() {
        let mut r = rule();
        r.must_contain = "ubuntu amd64".into();
        assert!(passed(&explain(&r, &item("Ubuntu 24.04 AMD64 ISO"))));
        r.must_contain = "ubuntu arm64".into();
        assert!(!passed(&explain(&r, &item("Ubuntu 24.04 AMD64"))));
        r.must_contain = "ubuntu".into();
        r.must_not_contain = "beta rc".into();
        assert!(!passed(&explain(&r, &item("Ubuntu 24.10 Beta"))));
        assert!(passed(&explain(&r, &item("Ubuntu 24.04 LTS"))));
        assert!(passed(&explain(&rule(), &item("anything at all"))));
    }

    #[test]
    fn regex_matches_case_insensitively_and_errors_loudly() {
        let mut r = rule();
        r.regex_match = "^ubuntu.*lts$".into();
        assert!(passed(&explain(&r, &item("Ubuntu 24.04 LTS"))));
        assert!(!passed(&explain(&r, &item("Ubuntu 24.04 beta"))));
        r.regex_match = "([unclosed".into();
        assert!(matches!(
            explain(&r, &item("Ubuntu")),
            Verdict::RuleError(_)
        ));
        assert!(!rule_matches(&r, &item("Ubuntu"),));
    }

    #[test]
    fn episode_forms_parse() {
        assert_eq!(
            parse_episode("Show.S01E02.1080p"),
            Some(Episode {
                season: Some(1),
                episode: 2
            })
        );
        assert_eq!(
            parse_episode("Show - 2x04 - Title"),
            Some(Episode {
                season: Some(2),
                episode: 4
            })
        );
        assert_eq!(
            parse_episode("Show S12E128 Extra"),
            Some(Episode {
                season: Some(12),
                episode: 128
            })
        );
        assert_eq!(parse_episode("Ubuntu 24.04 LTS"), None);
        // A year is not a season... 24 <= 60 though. "24.04" has no x —
        // only NxNN with x counts, so this stays None.
        assert_eq!(parse_episode("Movie 2024 1080p"), None);
    }

    #[test]
    fn season_and_episode_ranges_filter() {
        let mut r = rule();
        r.season_range = "1-2".into();
        r.episode_range = "4-8".into();
        assert!(passed(&explain(&r, &item("Show S02E06"))));
        assert!(!passed(&explain(&r, &item("Show S03E06"))));
        assert!(!passed(&explain(&r, &item("Show S02E09"))));
        // A title with no episode info fails a set filter...
        assert!(!passed(&explain(&r, &item("Show Pilot"))));
        // ...with the reason spelled out.
        match explain(&r, &item("Show Pilot")) {
            Verdict::NoMatch { clauses } => {
                assert!(clauses
                    .iter()
                    .any(|c| c.detail.contains("no season/episode")));
            }
            other => panic!("unexpected {other:?}"),
        }
        // ...but an unset filter matches everything.
        assert!(passed(&explain(&rule(), &item("Show Pilot"))));
        // Broken ranges fail loudly, never silently-everything.
        let mut broken = rule();
        broken.season_range = "3-1".into();
        assert!(!passed(&explain(&broken, &item("Show S02E06"))));
    }

    #[test]
    fn size_bounds_need_a_stated_size() {
        let mut r = rule();
        r.min_size_mb = 100;
        r.max_size_mb = 2000;
        let mut small = item("Show S01E01");
        small.size = Some(50 * 1024 * 1024);
        assert!(!passed(&explain(&r, &small)));
        let mut good = item("Show S01E01");
        good.size = Some(1500 * 1024 * 1024);
        assert!(passed(&explain(&r, &good)));
        // Unknown size fails a set bound.
        assert!(!passed(&explain(&r, &item("Show S01E01"))));
    }

    #[test]
    fn quality_ranks_resolution_then_source() {
        assert!(
            quality_score("Show S01E01 2160p BluRay") > quality_score("Show S01E01 1080p BluRay")
        );
        assert!(
            quality_score("Show S01E01 1080p WEB-DL") > quality_score("Show S01E01 1080p HDTV")
        );
        assert!(quality_score("Show S01E01 720p") > quality_score("Show S01E01 CAM"));
        assert_eq!(quality_score("Ubuntu 24.04"), 0);
    }

    #[test]
    fn smart_episode_keeps_the_best_per_episode() {
        let mut r = rule();
        r.smart_episode = true;
        let feed = Feed {
            id: "f".into(),
            name: "f".into(),
            url: "https://example.test/feed".into(),
            enabled: true,
        };
        let items = vec![
            item("Show S01E01 720p HDTV"),
            item("Show S01E01 1080p BluRay"),
            item("Show S01E02 720p HDTV"),
        ];
        let feeds = vec![(feed, items)];
        let mut state = EngineState::default();
        let rules = [r];
        let planned = plan(&feeds, &rules, &HashSet::new(), &mut state, 15, 1_000_000);
        let titles: Vec<&str> = planned.iter().map(|p| p.item.title.as_str()).collect();
        assert_eq!(titles.len(), 2);
        assert!(titles.contains(&"Show S01E01 1080p BluRay"));
        assert!(titles.contains(&"Show S01E02 720p HDTV"));
    }

    #[test]
    fn prefer_quality_boosts_its_token() {
        let mut r = rule();
        r.smart_episode = true;
        r.prefer_quality = "720p".into();
        let feed = Feed {
            id: "f".into(),
            name: "f".into(),
            url: "https://example.test/feed".into(),
            enabled: true,
        };
        let items = vec![
            item("Show S01E01 1080p BluRay"),
            item("Show S01E01 720p HDTV"),
        ];
        let feeds = vec![(feed, items)];
        let mut state = EngineState::default();
        let rules = [r];
        let planned = plan(&feeds, &rules, &HashSet::new(), &mut state, 15, 1_000_000);
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].item.title, "Show S01E01 720p HDTV");
    }

    #[test]
    fn per_rule_intervals_gate_polling() {
        let mut r = rule();
        r.id = "hourly".into();
        r.poll_minutes = 60;
        let feed = Feed {
            id: "f".into(),
            name: "f".into(),
            url: "https://example.test/feed".into(),
            enabled: true,
        };
        let feeds = vec![(feed, vec![item("Ubuntu")])];
        let mut state = EngineState::default();
        // First pass runs (never polled), second is gated, third is due.
        assert_eq!(
            plan(&feeds, &[r.clone()], &HashSet::new(), &mut state, 15, 0).len(),
            1
        );
        assert!(plan(
            &feeds,
            &[r.clone()],
            &HashSet::new(),
            &mut state,
            15,
            30 * 60_000
        )
        .is_empty());
        assert_eq!(
            plan(
                &feeds,
                &[r.clone()],
                &HashSet::new(),
                &mut state,
                15,
                61 * 60_000
            )
            .len(),
            1
        );
        // A zero global with a zero rule never polls in the background.
        let mut never = rule();
        never.poll_minutes = 0;
        assert!(plan(
            &feeds,
            &[never],
            &HashSet::new(),
            &mut state,
            0,
            999_999_999
        )
        .is_empty());
    }

    #[test]
    fn seen_guids_are_skipped_and_bad_regex_warns_once() {
        let mut r = rule();
        r.regex_match = "([bad".into();
        let feed = Feed {
            id: "f".into(),
            name: "f".into(),
            url: "https://example.test/feed".into(),
            enabled: true,
        };
        let feeds = vec![(feed, vec![item("Ubuntu")])];
        let mut state = EngineState::default();
        assert!(plan(&feeds, &[r.clone()], &HashSet::new(), &mut state, 15, 0).is_empty());
        assert_eq!(state.warned_bad_regex.len(), 1);
        let warnings = state.take_warnings();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].0, "r");
        assert!(state.take_warnings().is_empty(), "drained once");
        // Disabled rules never run.
        let mut off = rule();
        off.id = "off".into();
        off.enabled = false;
        assert!(plan(&feeds, &[off], &HashSet::new(), &mut state, 15, 0).is_empty());
        // Seen guids never re-add.
        let mut ok = rule();
        ok.id = "ok".into();
        let seen: HashSet<String> = ["Ubuntu".to_string()].into_iter().collect();
        assert!(plan(&feeds, &[ok], &seen, &mut state, 15, 0).is_empty());
    }

    #[test]
    fn feed_scope_follows_feed_id() {
        let mut r = rule();
        r.feed_id = "other".into();
        let feed = Feed {
            id: "f".into(),
            name: "f".into(),
            url: "https://example.test/feed".into(),
            enabled: true,
        };
        let feeds = vec![(feed, vec![item("Ubuntu")])];
        let mut state = EngineState::default();
        assert!(plan(&feeds, &[r], &HashSet::new(), &mut state, 15, 0).is_empty());
    }

    #[test]
    fn enclosure_length_becomes_item_size() {
        let xml = r#"
        <rss><channel>
          <item>
            <title>Big Release 1080p</title>
            <enclosure url="https://x.example/f.torrent" length="1572864000" type="application/x-bittorrent"/>
            <guid>g1</guid>
          </item>
        </channel></rss>"#;
        let items = parse_feed(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].size, Some(1572864000));
    }

    #[test]
    fn seen_files_read_both_shapes_and_cap_on_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rss_seen.json");
        assert!(load_seen(&path).is_empty());
        let seen: Vec<String> = (0..5).map(|i| format!("g{i}")).collect();
        save_seen(&path, &seen);
        assert_eq!(load_seen(&path), seen);
        // The newer envelope reads too.
        std::fs::write(&path, r#"{"seen":["a","b"]}"#).unwrap();
        assert_eq!(load_seen(&path), vec!["a", "b"]);
    }

    #[test]
    fn rule_shape_matches_the_historical_wire_contract() {
        // The settings-file JSON must stay byte-compatible with B11 files:
        // same camelCase keys, new keys optional.
        let legacy = serde_json::json!({
            "id": "r",
            "name": "r",
            "enabled": true,
            "feedId": "",
            "mustContain": "ubuntu",
            "mustNotContain": "",
            "label": "iso",
            "savePath": "/dl"
        });
        let rule: Rule = serde_json::from_value(legacy).unwrap();
        assert_eq!(rule.must_contain, "ubuntu");
        assert!(rule.start, "adds start by default");
        assert!(!rule.top_of_queue);
        assert!(rule.tags.is_empty());
        let value = serde_json::to_value(&rule).unwrap();
        assert_eq!(value["mustContain"], "ubuntu");
        assert_eq!(value["start"], true);
    }

    #[test]
    fn parses_rss_with_enclosure_preferred_over_link() {
        let xml = r#"
        <rss><channel>
          <item>
            <title>Cool Release 1080p</title>
            <link>https://site.example/details/1</link>
            <enclosure url="https://site.example/t/1.torrent" type="application/x-bittorrent"/>
            <guid>abc-123</guid>
            <pubDate>Mon, 01 Jan 2026 00:00:00 GMT</pubDate>
          </item>
        </channel></rss>"#;
        let items = parse_feed(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Cool Release 1080p");
        assert_eq!(items[0].link, "https://site.example/t/1.torrent");
        assert_eq!(items[0].guid, "abc-123");
    }

    #[test]
    fn rss_link_is_used_when_no_enclosure_and_guid_falls_back_to_link() {
        let xml = r#"
        <rss><channel>
          <item>
            <title><![CDATA[Bracketed Title]]></title>
            <link>magnet:?xt=urn:btih:DEADBEEF&amp;dn=x</link>
          </item>
        </channel></rss>"#;
        let items = parse_feed(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Bracketed Title");
        assert!(items[0].link.starts_with("magnet:"));
        // No <guid> → identity falls back to the link.
        assert_eq!(items[0].guid, items[0].link);
    }

    #[test]
    fn parses_atom_link_href() {
        let xml = r#"
        <feed xmlns="http://www.w3.org/2005/Atom">
          <entry>
            <title>Atom Item</title>
            <id>tag:example,2026:1</id>
            <link rel="enclosure" href="https://x.example/a.torrent"/>
            <link rel="alternate" href="https://x.example/page"/>
          </entry>
        </feed>"#;
        let items = parse_feed(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Atom Item");
        assert_eq!(items[0].link, "https://x.example/a.torrent");
        assert_eq!(items[0].guid, "tag:example,2026:1");
    }

    #[test]
    fn garbage_input_is_empty_not_an_error() {
        assert!(parse_feed("not xml at all <<<").is_empty());
        assert!(parse_feed("").is_empty());
    }
}
