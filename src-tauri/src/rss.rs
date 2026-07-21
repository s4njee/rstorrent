//! RSS/Atom feeds and auto-add rules (B11).
//!
//! This module is the plumbing: fetch a feed over HTTP, parse RSS 2.0 or Atom
//! into [`FeedItem`]s, decide whether a [`RssRule`] matches an item's title, and
//! persist the set of item ids already added so nothing is added twice. The
//! background engine ([`spawn`]) ties these together.
//!
//! Parsing is deliberately tolerant — feeds in the wild are messy — and picks
//! the best download URL it can: a torrent `<enclosure>` beats a plain `<link>`,
//! and a magnet link is taken as-is.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use quick_xml::events::Event;
use quick_xml::Reader;
use tauri::AppHandle;

use crate::ipc::{FeedItem, LogLevel, RssRule, Settings};
use crate::rtorrent::LoadOptions;
use crate::settings;
use crate::state::AppState;

/// Ceiling on the persisted seen-id set, so it can't grow without bound.
const SEEN_CAP: usize = 2000;
/// Delay before the first poll, so it doesn't race startup/first connect.
const STARTUP_DELAY: Duration = Duration::from_secs(20);
/// How often to re-check settings while RSS polling is disabled.
const IDLE_RECHECK_MINS: u64 = 5;

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

/// Does an auto-download rule match this item title?
///
/// `must_contain` is whitespace-split into tokens that must *all* appear
/// (case-insensitive); an empty field matches everything. `must_not_contain` is
/// tokens of which *none* may appear.
pub fn rule_matches(rule: &RssRule, title: &str) -> bool {
    let hay = title.to_lowercase();
    let all_present = rule
        .must_contain
        .split_whitespace()
        .all(|tok| hay.contains(&tok.to_lowercase()));
    let none_present = !rule
        .must_not_contain
        .split_whitespace()
        .any(|tok| hay.contains(&tok.to_lowercase()));
    all_present && none_present
}

/// Load the persisted set of already-added item ids (guids).
pub fn load_seen(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist the seen-id set, creating the parent directory as needed.
pub fn save_seen(path: &Path, seen: &[String]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(seen) {
        let _ = std::fs::write(path, text);
    }
}

/// Where the seen-id set is persisted (next to the stats/settings files).
fn seen_path(state: &AppState) -> PathBuf {
    state
        .stats_path
        .parent()
        .map(|p| p.join("rss_seen.json"))
        .unwrap_or_else(|| PathBuf::from("rss_seen.json"))
}

/// Start the background RSS poller. It polls enabled feeds every
/// `rss_poll_minutes`, auto-adds items that match an enabled rule (deduped
/// against the persisted seen-set), and idles cheaply while RSS is disabled.
pub fn spawn(app: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        let path = seen_path(&state);
        let mut seen = load_seen(&path);
        loop {
            let settings = state.settings();
            let interval = settings.rss_poll_minutes;
            if interval > 0 && !settings.rss_feeds.is_empty() {
                run_once(&app, &state, &settings, &mut seen, &path).await;
            }
            let mins = if interval > 0 {
                (interval as u64).max(1)
            } else {
                IDLE_RECHECK_MINS
            };
            tokio::time::sleep(Duration::from_secs(mins * 60)).await;
        }
    });
}

/// One polling pass: fetch each enabled feed, add rule matches not seen before.
async fn run_once(
    app: &AppHandle,
    state: &Arc<AppState>,
    settings: &Settings,
    seen: &mut Vec<String>,
    path: &Path,
) {
    let backend = state.backend();
    let mut added = 0usize;
    let mut changed = false;

    for feed in settings.rss_feeds.iter().filter(|f| f.enabled) {
        let rules: Vec<&RssRule> = settings
            .rss_rules
            .iter()
            .filter(|r| r.enabled && (r.feed_id.is_empty() || r.feed_id == feed.id))
            .collect();
        if rules.is_empty() {
            continue;
        }
        let items = match fetch(&feed.url).await {
            Ok(items) => items,
            Err(err) => {
                state.log(
                    app,
                    LogLevel::Warn,
                    format!("rss: {} failed: {err}", feed.name),
                    None,
                );
                continue;
            }
        };
        for item in &items {
            if seen.iter().any(|g| g == &item.guid) {
                continue;
            }
            let Some(rule) = rules.iter().find(|r| rule_matches(r, &item.title)) else {
                continue;
            };
            // Save path: rule override → label default → global default, then
            // translate into the daemon namespace (a WSL daemon needs a Linux path).
            let resolved = if rule.save_path.is_empty() {
                settings::save_path_for_label(settings, &rule.label)
            } else {
                rule.save_path.clone()
            };
            let directory = crate::localfs::to_daemon_path(&resolved).unwrap_or(resolved);
            let opts = LoadOptions {
                directory,
                label: rule.label.clone(),
                start: true,
                top_of_queue: false,
                unselected_indexes: vec![],
            };
            match backend.load_magnet(&item.link, opts).await {
                Ok(_) => {
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
    }

    if changed {
        // Keep only the most recent ids so the file can't grow forever.
        if seen.len() > SEEN_CAP {
            seen.drain(0..seen.len() - SEEN_CAP);
        }
        save_seen(path, seen);
        if added > 0 {
            state.repoll.notify_one();
        }
    }
}

/// Accumulates one item as the parser walks its child elements.
#[derive(Default)]
struct Building {
    title: String,
    guid: String,
    pub_date: String,
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

/// Parse an RSS 2.0 or Atom document into items. Unknown/garbage input yields an
/// empty list rather than an error.
pub fn parse_feed(xml: &str) -> Vec<FeedItem> {
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
                let (url, ty) = enclosure_attrs(e);
                if let Some(url) = url {
                    if looks_like_torrent(&url, ty.as_deref()) {
                        b.enclosure = Some(url);
                    }
                }
            }
            Field::None
        }
        "link" => {
            // Atom `<link href=… rel=…>` carries the URL in an attribute; RSS
            // `<link>` carries it as text (→ capture via Field::RssLink).
            if let Some(b) = cur.as_mut() {
                let (href, rel) = link_attrs(e);
                match href {
                    Some(href) => {
                        if rel.as_deref() == Some("enclosure") {
                            b.enclosure = Some(href);
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

fn enclosure_attrs(e: &quick_xml::events::BytesStart) -> (Option<String>, Option<String>) {
    let mut url = None;
    let mut ty = None;
    for attr in e.attributes().flatten() {
        match local_name(attr.key.as_ref()).as_str() {
            "url" => url = Some(attr_value(&attr)),
            "type" => ty = Some(attr_value(&attr)),
            _ => {}
        }
    }
    (url, ty)
}

fn link_attrs(e: &quick_xml::events::BytesStart) -> (Option<String>, Option<String>) {
    let mut href = None;
    let mut rel = None;
    for attr in e.attributes().flatten() {
        match local_name(attr.key.as_ref()).as_str() {
            "href" => href = Some(attr_value(&attr)),
            "rel" => rel = Some(attr_value(&attr).to_lowercase()),
            _ => {}
        }
    }
    (href, rel)
}

fn attr_value(attr: &quick_xml::events::attributes::Attribute) -> String {
    String::from_utf8_lossy(&attr.value).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(must: &str, must_not: &str) -> RssRule {
        RssRule {
            id: "r".into(),
            name: "r".into(),
            enabled: true,
            feed_id: String::new(),
            must_contain: must.into(),
            must_not_contain: must_not.into(),
            label: String::new(),
            save_path: String::new(),
        }
    }

    #[test]
    fn rule_tokens_are_and_for_contain_and_none_for_exclude() {
        assert!(rule_matches(
            &rule("ubuntu amd64", ""),
            "Ubuntu 24.04 AMD64 ISO"
        ));
        // A missing token fails the AND.
        assert!(!rule_matches(
            &rule("ubuntu arm64", ""),
            "Ubuntu 24.04 AMD64"
        ));
        // Exclusion: any token present rejects.
        assert!(!rule_matches(
            &rule("ubuntu", "beta rc"),
            "Ubuntu 24.10 Beta"
        ));
        assert!(rule_matches(&rule("ubuntu", "beta rc"), "Ubuntu 24.04 LTS"));
        // Empty must_contain matches anything.
        assert!(rule_matches(&rule("", ""), "anything at all"));
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
