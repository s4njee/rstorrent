//! RSS/Atom feeds and auto-download rules (B11, rules 2.0 in V3-23).
//!
//! Thin host over [`rtorrent_core::rss`]: this module owns the background
//! loop, the seen-file, and the daemon writes (load, tags, provenance),
//! while matching, smart-episode picks and interval gating live in the core
//! engine all shells share.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;

use crate::ipc::{FeedItem, LogLevel, RssRule, Settings};
use crate::rtorrent::{magnet_hash, LoadOptions};
use crate::settings;
use crate::state::AppState;

use rtorrent_core::rss;

/// Delay before the first poll, so it doesn't race startup/first connect.
const STARTUP_DELAY: Duration = Duration::from_secs(20);
/// How often to re-check settings while RSS polling is disabled.
const IDLE_RECHECK_MINS: u64 = 5;

/// Fetch and parse a feed. Errors are user-facing strings (shown in the RSS UI).
pub async fn fetch(url: &str) -> Result<Vec<FeedItem>, String> {
    rss::fetch(url).await
}

/// Test one rule against a live feed for the preview (V3-23): every item
/// with its verdict, so the UI can explain each match and each miss.
pub async fn test_rule(rule: &RssRule, url: &str) -> Result<Vec<RssTestRow>, String> {
    // Cap the report: previews show the head of the feed, never all of it.
    const PREVIEW_CAP: usize = 50;
    let items = fetch(url).await?;
    Ok(items
        .into_iter()
        .take(PREVIEW_CAP)
        .map(|item| {
            let (matched, clauses) = match rss::explain(rule, &item) {
                rss::Verdict::Match { clauses } => (true, clauses),
                rss::Verdict::NoMatch { clauses } => (false, clauses),
                rss::Verdict::RuleError(err) => (
                    false,
                    vec![rss::Clause {
                        label: "rule".into(),
                        passed: false,
                        detail: err,
                    }],
                ),
            };
            RssTestRow {
                title: item.title.clone(),
                link: item.link.clone(),
                guid: item.guid.clone(),
                size: item.size,
                matched,
                clauses: clauses
                    .into_iter()
                    .map(|c| RssTestClause {
                        label: c.label,
                        passed: c.passed,
                        detail: c.detail,
                    })
                    .collect(),
            }
        })
        .collect())
}

/// One preview row: the item plus its clause-by-clause verdict.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RssTestRow {
    pub title: String,
    pub link: String,
    pub guid: String,
    pub size: Option<i64>,
    pub matched: bool,
    pub clauses: Vec<RssTestClause>,
}

/// One explained clause for the preview.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RssTestClause {
    pub label: String,
    pub passed: bool,
    pub detail: String,
}

/// Load the persisted set of already-added item ids (guids).
pub fn load_seen(path: &Path) -> Vec<String> {
    rss::load_seen(path)
}

/// Persist the seen-id set, creating the parent directory as needed.
pub fn save_seen(path: &Path, seen: &[String]) {
    rss::save_seen(path, seen)
}

/// Where the seen-id set is persisted (next to the stats/settings files).
fn seen_path(state: &AppState) -> PathBuf {
    state
        .stats_path
        .parent()
        .map(|p| p.join("rss_seen.json"))
        .unwrap_or_else(|| PathBuf::from("rss_seen.json"))
}

/// Export the seen-set as JSON (V3-23): the frontend downloads it as a file.
pub fn export_seen(state: &AppState) -> String {
    let seen = load_seen(&seen_path(state));
    serde_json::to_string_pretty(&seen).unwrap_or_else(|_| "[]".into())
}

/// Import guids into the seen-set, returning how many were new. Never
/// re-adds: imported ids suppress future matches exactly like added ones.
pub fn import_seen(state: &AppState, json: &str) -> usize {
    let incoming: Vec<String> = serde_json::from_str(json).unwrap_or_default();
    if incoming.is_empty() {
        return 0;
    }
    let path = seen_path(state);
    let mut seen = load_seen(&path);
    let mut added = 0;
    for guid in incoming {
        let guid = guid.trim().to_owned();
        if guid.is_empty() || seen.iter().any(|g| g == &guid) {
            continue;
        }
        seen.push(guid);
        added += 1;
    }
    save_seen(&path, &seen);
    added
}

/// Start the background RSS poller. Wakes on the shortest configured cadence
/// (global or any per-rule interval); rules due nothing cost nothing. Idles
/// cheaply while RSS is disabled.
pub fn spawn(app: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        let path = seen_path(&state);
        let mut seen = load_seen(&path);
        let mut engine = rss::EngineState::default();
        loop {
            let settings = state.settings();
            let wait_mins = wake_minutes(&settings);
            if polling_enabled(&settings) {
                run_once(&app, &state, &settings, &mut seen, &mut engine, &path).await;
            }
            tokio::time::sleep(Duration::from_secs(wait_mins.max(1) * 60)).await;
        }
    });
}

/// Minutes until the next wake: the shortest positive cadence anywhere, else
/// the idle recheck.
fn wake_minutes(settings: &Settings) -> u64 {
    let mut best: Option<i64> = None;
    if settings.rss_poll_minutes > 0 {
        best = Some(settings.rss_poll_minutes);
    }
    for rule in settings.rss_rules.iter().filter(|r| r.enabled) {
        if rule.poll_minutes > 0 {
            best = Some(best.map_or(rule.poll_minutes, |b| b.min(rule.poll_minutes)));
        }
    }
    best.map(|m| m as u64).unwrap_or(IDLE_RECHECK_MINS)
}

fn polling_enabled(settings: &Settings) -> bool {
    // Rules with no own interval ride the global one; either cadence being
    // positive keeps the loop fetching.
    settings.rss_poll_minutes > 0
        || settings
            .rss_rules
            .iter()
            .any(|r| r.enabled && r.poll_minutes > 0)
}

/// One polling pass: fetch due feeds, add unseen matches through the core
/// plan, persist provenance, tags and the seen-set.
async fn run_once(
    app: &AppHandle,
    state: &Arc<AppState>,
    settings: &Settings,
    seen: &mut Vec<String>,
    engine: &mut rss::EngineState,
    path: &Path,
) {
    let backend = state.backend();
    let now_ms = now_millis();
    let seen_set: HashSet<String> = seen.iter().cloned().collect();

    // Fetch every enabled feed that has an enabled rule; the plan handles
    // per-rule intervals and scope, so one fetch serves all its rules.
    let mut fetched: Vec<(rtorrent_core::rss::Feed, Vec<rtorrent_core::rss::FeedItem>)> =
        Vec::new();
    for feed in settings.rss_feeds.iter().filter(|f| f.enabled) {
        let wanted = settings
            .rss_rules
            .iter()
            .any(|r| r.enabled && (r.feed_id.is_empty() || r.feed_id == feed.id));
        if !wanted {
            continue;
        }
        match fetch(&feed.url).await {
            Ok(items) => fetched.push((
                rtorrent_core::rss::Feed {
                    id: feed.id.clone(),
                    name: feed.name.clone(),
                    url: feed.url.clone(),
                    enabled: feed.enabled,
                },
                items,
            )),
            Err(err) => {
                state.log(
                    app,
                    LogLevel::Warn,
                    format!("rss: {} failed: {err}", feed.name),
                    None,
                );
            }
        }
    }

    let mut added = 0usize;
    let mut changed = false;
    for planned in rss::plan(
        &fetched,
        &settings.rss_rules,
        &seen_set,
        engine,
        settings.rss_poll_minutes,
        now_ms,
    ) {
        let rule = planned.rule;
        let item = planned.item;
        // Save path: rule override → label default → global default, then
        // through the incomplete dir when configured (V3-14), and translate
        // into the daemon namespace (a WSL daemon needs a Linux path).
        let resolved = if rule.save_path.is_empty() {
            settings::save_path_for_label(settings, &rule.label)
        } else {
            rule.save_path.clone()
        };
        let (directory, final_dir) = settings::route_new_download(settings, &resolved);
        let before: HashSet<String> = backend
            .list_snapshot()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|torrent| torrent.hash)
            .collect();
        let opts = LoadOptions {
            directory,
            label: rule.label.clone(),
            start: rule.start,
            top_of_queue: rule.top_of_queue,
            unselected_indexes: vec![],
        };
        match backend.load_magnet(&item.link, opts).await {
            Ok(_) => {
                let hash = if let Some(hash) = magnet_hash(&item.link) {
                    Some(hash)
                } else {
                    backend.list_snapshot().await.ok().and_then(|rows| {
                        rows.into_iter()
                            .find(|torrent| !before.contains(&torrent.hash))
                            .map(|torrent| torrent.hash)
                    })
                };
                if let Some(hash) = hash.as_deref() {
                    if let Err(err) =
                        crate::commands::persist_final_dir(&*backend, hash, final_dir.as_deref())
                            .await
                    {
                        state.log(
                            app,
                            LogLevel::Warn,
                            format!("rss: could not persist final directory: {err}"),
                            Some(hash.to_owned()),
                        );
                    }
                    if !rule.tags.is_empty() {
                        if let Err(err) = backend.set_tags(&[hash.to_owned()], &rule.tags).await {
                            state.log(
                                app,
                                LogLevel::Warn,
                                format!("rss: could not set tags: {err}"),
                                Some(hash.to_owned()),
                            );
                        }
                    }
                    if let Err(err) =
                        crate::commands::persist_add_metadata(&*backend, hash, "rss", &item.link)
                            .await
                    {
                        state.log(
                            app,
                            LogLevel::Warn,
                            format!("rss: could not persist add metadata: {err}"),
                            Some(hash.to_owned()),
                        );
                    }
                }
                seen.push(item.guid.clone());
                changed = true;
                added += 1;
                state.log(
                    app,
                    LogLevel::Info,
                    format!("rss: added \"{}\" (rule: {})", item.title, rule.name),
                    None,
                );
            }
            Err(err) => state.log(
                app,
                LogLevel::Warn,
                format!("rss: could not add \"{}\": {err}", item.title),
                None,
            ),
        }
    }

    for (rule_id, pattern) in engine.take_warnings() {
        let name = settings
            .rss_rules
            .iter()
            .find(|r| r.id == rule_id)
            .map(|r| r.name.clone())
            .unwrap_or(rule_id);
        state.log(
            app,
            LogLevel::Warn,
            format!("rss: rule \"{name}\" has an invalid regex '{pattern}' — it matches nothing"),
            None,
        );
    }

    if changed {
        // Keep the in-memory set bounded too, not just the file.
        if seen.len() > rss::SEEN_CAP {
            seen.drain(0..seen.len() - rss::SEEN_CAP);
        }
        save_seen(path, seen);
        if added > 0 {
            state.repoll.notify_one();
        }
    }
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
