//! RSS auto-download runner (V3-23) for the native shell.
//!
//! The same core engine the Tauri poller drives
//! ([`rtorrent_core::rss::plan`]), hosted as a slow background loop: wake
//! every minute, fetch feeds with due rules, add unseen matches, persist
//! tags/provenance and the seen-set. Outcomes come back as plain log lines
//! the model pushes into the Log pane, so the runner never touches UI state.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rtorrent_core::rss::{self, EngineState};

use crate::services::Services;
use rtorrent_core::types::{AddOptions, LogLevel};

/// How often the runner wakes to check rule intervals.
const TICK: Duration = Duration::from_secs(60);
/// Delay before the first poll, so it doesn't race startup/first connect.
const STARTUP_DELAY: Duration = Duration::from_secs(20);

/// One log line for the model to push.
pub struct RssLog {
    pub level: LogLevel,
    pub message: String,
    pub hash: Option<String>,
}

/// One preview row: the verdict plus the first failing clause.
#[derive(Clone, Debug, Default)]
pub struct TestRow {
    pub feed: String,
    pub title: String,
    pub matched: bool,
    pub reason: String,
}

/// The rule-test report the preferences preview renders.
#[derive(Clone, Debug, Default)]
pub struct TestReport {
    pub rows: Vec<TestRow>,
}

/// Background RSS state: seen-set, engine memory, and the journal path.
pub struct Runner {
    services: Arc<Services>,
    seen_path: PathBuf,
    seen: Vec<String>,
    engine: EngineState,
    started: bool,
}

impl Runner {
    pub fn new(services: Arc<Services>, settings_store: &crate::settings::SettingsStore) -> Self {
        let seen_path = settings_store
            .path()
            .parent()
            .map(|p| p.join("rss_seen.json"))
            .unwrap_or_else(|| PathBuf::from("rss_seen.json"));
        let seen = rss::load_seen(&seen_path);
        Self {
            services,
            seen_path,
            seen,
            engine: EngineState::default(),
            started: false,
        }
    }

    /// Current seen count, for the preferences row.
    #[must_use]
    pub fn seen_count(&self) -> usize {
        self.seen.len()
    }

    /// Drop every remembered id. The next tick re-matches live feed items —
    /// useful after fixing a broken rule, dangerous as a habit.
    pub fn clear_seen(&mut self) {
        self.seen.clear();
        rss::save_seen(&self.seen_path, &self.seen);
    }

    /// One pass: fetch feeds with due rules, add unseen matches. Returns log
    /// lines; the model pushes them and nudges a re-poll when anything was
    /// added.
    pub async fn tick(&mut self) -> Vec<RssLog> {
        if !self.started {
            self.started = true;
            tokio::time::sleep(STARTUP_DELAY).await;
        }
        let settings = self.services.settings();
        let now_ms = now_millis();
        let mut logs = Vec::new();

        // Feeds with at least one due, enabled rule. Due-ness is rechecked
        // inside the plan; this just avoids fetching for idle rules.
        let mut fetched: Vec<(rss::Feed, Vec<rss::FeedItem>)> = Vec::new();
        for feed in settings.rss_feeds.iter().filter(|f| f.enabled) {
            let wanted = settings.rss_rules.iter().any(|r| {
                r.enabled
                    && (r.feed_id.is_empty() || r.feed_id == feed.id)
                    && self.engine.due(r, settings.rss_poll_minutes, now_ms)
            });
            if !wanted {
                continue;
            }
            match rss::fetch(&feed.url).await {
                Ok(items) => fetched.push((
                    rss::Feed {
                        id: feed.id.clone(),
                        name: feed.name.clone(),
                        url: feed.url.clone(),
                        enabled: feed.enabled,
                    },
                    items,
                )),
                Err(err) => logs.push(RssLog {
                    level: LogLevel::Warn,
                    message: format!("rss: {} failed: {err}", feed.name),
                    hash: None,
                }),
            }
        }

        let seen_set: HashSet<String> = self.seen.iter().cloned().collect();
        let mut added = 0;
        for planned in rss::plan(
            &fetched,
            &settings.rss_rules,
            &seen_set,
            &mut self.engine,
            settings.rss_poll_minutes,
            now_ms,
        ) {
            let rule = planned.rule;
            let item = planned.item;
            let resolved = if rule.save_path.is_empty() {
                save_path_for_label(&settings, &rule.label)
            } else {
                rule.save_path.clone()
            };
            let options = AddOptions {
                save_path: resolved,
                label: rule.label.clone(),
                start: rule.start,
                top_of_queue: rule.top_of_queue,
                sequential: false,
                skip_hash_check: false,
                unselected_indexes: Vec::new(),
            };
            match self.services.add_magnet(item.link.clone(), options).await {
                Ok(()) => {
                    if let Some(hash) = rtorrent_core::rtorrent::magnet_hash(&item.link) {
                        if !rule.tags.is_empty() {
                            let _ = self
                                .services
                                .set_tags(vec![hash.clone()], rule.tags.clone())
                                .await;
                        }
                    }
                    self.seen.push(item.guid.clone());
                    added += 1;
                    logs.push(RssLog {
                        level: LogLevel::Info,
                        message: format!("rss: added \"{}\" (rule: {})", item.title, rule.name),
                        hash: None,
                    });
                }
                Err(err) => logs.push(RssLog {
                    level: LogLevel::Warn,
                    message: format!("rss: could not add \"{}\": {err}", item.title),
                    hash: None,
                }),
            }
        }
        if added > 0 {
            if self.seen.len() > rss::SEEN_CAP {
                self.seen.drain(0..self.seen.len() - rss::SEEN_CAP);
            }
            rss::save_seen(&self.seen_path, &self.seen);
            self.services.retry_now();
        }
        for (rule_id, pattern) in self.engine.take_warnings() {
            logs.push(RssLog {
                level: LogLevel::Warn,
                message: format!(
                    "rss: rule \"{rule_id}\" has an invalid regex '{pattern}' — it matches nothing"
                ),
                hash: None,
            });
        }
        logs
    }
}

/// Per-label default save path (C11), else the global default.
fn save_path_for_label(settings: &crate::settings::Settings, label: &str) -> String {
    settings
        .label_defaults
        .iter()
        .find(|d| d.label == label && !d.save_path.is_empty())
        .map(|d| d.save_path.clone())
        .unwrap_or_else(|| settings.default_save_path.clone())
}

/// Cadence for the model's slow RSS loop.
#[must_use]
pub const fn tick_interval() -> Duration {
    TICK
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
