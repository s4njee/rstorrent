//! The detail panel's facts rail, as data.
//!
//! Pure on purpose: the rail is a list of key/value rows whose arithmetic is
//! worth testing without a GPUI window. A port of `src/utils/facts.ts`.
//!
//! The handoff's eleven keys come first, in its order. Everything after them is
//! ours — per-torrent limits, peer caps and provenance are real rtorrent state
//! the design's prototype had no field for, and dropping them would lose
//! function. They are separated visually by a divider, not a different treatment.

use rtorrent_core::types::{GlobalStats, PieceInfo, TorrentDto};

use crate::format;

/// One rail row: a key, its value, and how the value is coloured.
pub struct Fact {
    pub key: &'static str,
    pub value: String,
    pub tone: Tone,
}

/// How a value reads: the two rates carry their direction's hue, the way the
/// table's cells do; everything else is ordinary text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Plain,
    /// Download: the accent.
    Rate,
    /// Upload: the cyan.
    Up,
}

fn fact(key: &'static str, value: impl Into<String>) -> Fact {
    Fact {
        key,
        value: value.into(),
        tone: Tone::Plain,
    }
}

/// The handoff's facts, in its order.
#[must_use]
pub fn primary(t: &TorrentDto, pieces: Option<&PieceInfo>) -> Vec<Fact> {
    vec![
        fact("Hash", t.hash.clone()),
        fact("Downloaded", format::bytes(t.bytes_done)),
        fact("Uploaded", format::bytes(uploaded_bytes(t))),
        fact("Ratio", format::ratio(t.ratio)),
        fact("Pieces", pieces_text(pieces)),
        fact("Peers", format::count(t.peers_connected)),
        Fact {
            key: "Down rate",
            value: rate_or_dash(t.down_rate),
            tone: Tone::Rate,
        },
        Fact {
            key: "Up rate",
            value: rate_or_dash(t.up_rate),
            tone: Tone::Up,
        },
        fact("Path", path_text(&t.save_path)),
        fact("Added", added_text(t)),
        // Always shown, unlike the old grid which only mentioned it when true:
        // the rail answers "is this private?" in both directions.
        fact("Private", if t.is_private { "yes" } else { "no" }),
    ]
}

/// Our additions to the rail: the per-torrent state the design had no field for.
#[must_use]
pub fn extra(
    t: &TorrentDto,
    globals: &GlobalStats,
    rules: &[rtorrent_core::bandwidth::BandwidthRule],
) -> Vec<Fact> {
    let rule = rtorrent_core::bandwidth::rule_for(&t.label, &t.tags, rules);
    let mut facts = vec![
        fact("Size", format::bytes(t.size)),
        fact("ETA", format::eta(t.eta_seconds, t.status)),
        fact(
            "Connections",
            format::count(t.peers_connected + t.seeds_connected),
        ),
        fact(
            "Down limit",
            limit_text(
                t.down_rate_limit,
                &t.throttle_name,
                &t.throttle_rule,
                rule,
                globals.turtle_active,
                globals.down_rate_limit,
            ),
        ),
        fact(
            "Up limit",
            limit_text(
                t.up_rate_limit,
                &t.throttle_name,
                &t.throttle_rule,
                rule,
                globals.turtle_active,
                globals.up_rate_limit,
            ),
        ),
        fact("Peer cap", cap_text(t.peers_max)),
        fact("Peer floor", cap_text(t.peers_min)),
        fact("Upload slots", cap_text(t.uploads_max)),
    ];
    if !t.source_path.is_empty() {
        facts.push(fact("Source", t.source_path.clone()));
    }
    if t.connection_type == "initial_seed" {
        facts.push(fact("Mode", "super-seeding"));
    }
    if t.is_private {
        facts.push(fact("DHT / PEX", "off — private torrent"));
    }
    facts
}

/// A rate, or a dash when nothing is moving.
fn rate_or_dash(rate: i64) -> String {
    if rate > 0 {
        format::rate(rate)
    } else {
        "—".to_owned()
    }
}

/// A path, or a dash when the daemon has not reported one.
fn path_text(path: &str) -> String {
    if path.is_empty() {
        "—".to_owned()
    } else {
        path.to_owned()
    }
}

/// Total uploaded bytes.
///
/// rtorrent exposes `d.up.total`, but the DTO carries only `d.ratio`, which *is*
/// uploaded ÷ downloaded — so this inverts exactly the figure the daemon reports
/// rather than inventing one. A ratio of 0 means "nothing uploaded yet".
#[must_use]
pub fn uploaded_bytes(t: &TorrentDto) -> i64 {
    if !t.ratio.is_finite() || t.ratio <= 0.0 {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation)]
    let uploaded = (t.bytes_done as f64 * t.ratio).round();
    if uploaded <= 0.0 || uploaded > i64::MAX as f64 {
        0
    } else {
        uploaded as i64
    }
}

/// `1,428 × 2 MiB`, or a dash until the detail poll delivers the piece map.
#[must_use]
pub fn pieces_text(pieces: Option<&PieceInfo>) -> String {
    let Some(pieces) = pieces else {
        return "—".to_owned();
    };
    if pieces.size_chunks <= 0 {
        return "—".to_owned();
    }
    let chunk = if pieces.chunk_size > 0 {
        format::bytes(pieces.chunk_size)
    } else {
        "?".to_owned()
    };
    format!("{} × {chunk}", format::count(pieces.size_chunks))
}

/// Who added it and when, either of which may be unknown.
#[must_use]
pub fn added_text(t: &TorrentDto) -> String {
    if t.added_by.is_empty() && t.added_at == 0 {
        return "—".to_owned();
    }
    let who = if t.added_by.is_empty() {
        "unknown"
    } else {
        t.added_by.as_str()
    };
    if t.added_at == 0 {
        who.to_owned()
    } else {
        format!("{who} · {}", format::date(t.added_at))
    }
}

/// A per-torrent limit, the session's when the torrent has none, or unlimited.
///
/// The source is part of the value: "is this cap the torrent's or the session's?"
/// is the question the row exists to answer. `Some(0)` is rtorrent's "unlimited
/// within this named group" — the torrent's own setting, so it keeps its name,
/// while `None` means the torrent defers to the session.
fn limit_text(
    own: Option<i64>,
    throttle: &str,
    marker: &str,
    rule: Option<&rtorrent_core::bandwidth::BandwidthRule>,
    turtle: bool,
    session: i64,
) -> String {
    // Precedence, in the backlog's words: torrent override → rule → turtle
    // → global. Decided by the shared driver, not re-derived here.
    let source = rtorrent_core::bandwidth::source_word(&rtorrent_core::bandwidth::limit_source(
        throttle,
        marker,
        own.is_some(),
        rule,
        turtle,
    ));
    match own {
        Some(limit) if limit > 0 => format!("{} · {source}", format::rate(limit)),
        Some(_) => format!("∞ · {source}"),
        None if session > 0 => format!("{} · {source}", format::rate(session)),
        None => format!("∞ · {source}"),
    }
}

/// A cap, or the daemon's default when the torrent does not set one.
fn cap_text(value: i64) -> String {
    if value > 0 {
        value.to_string()
    } else {
        "default".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtorrent_core::types::Status;

    fn torrent() -> TorrentDto {
        TorrentDto {
            hash: "8F4A2C1D9E7B3F60".into(),
            name: "debian-13.1.0-amd64-DVD-1".into(),
            size: 0,
            bytes_done: 0,
            percent: 0.0,
            status: Status::Downloading,
            status_msg: String::new(),
            error_kind: String::new(),
            seeds_connected: 0,
            peers_connected: 0,
            seeds_swarm: 0,
            peers_swarm: 0,
            down_rate: 0,
            up_rate: 0,
            eta_seconds: None,
            ratio: 0.0,
            label: String::new(),
            tracker_host: String::new(),
            save_path: String::new(),
            priority: 0,
            is_private: false,
            throttle_name: String::new(),
            down_rate_limit: None,
            up_rate_limit: None,
            started_at: 0,
            finished_at: 0,
            peers_max: 0,
            peers_min: 0,
            uploads_max: 0,
            connection_type: String::new(),
            added_by: String::new(),
            source_path: String::new(),
            added_at: 0,
            tags: Vec::new(),
            force_start: false,
            throttle_rule: String::new(),
            views: Vec::new(),
            is_open: true,
            is_active: true,
        }
    }

    fn globals() -> GlobalStats {
        rtorrent_core::snapshot::empty_globals()
    }

    fn value(facts: &[Fact], key: &str) -> String {
        facts
            .iter()
            .find(|fact| fact.key == key)
            .map_or_else(|| panic!("no {key} fact"), |fact| fact.value.clone())
    }

    #[test]
    fn the_handoffs_eleven_facts_come_first_in_its_order() {
        let keys: Vec<&str> = primary(&torrent(), None)
            .iter()
            .map(|fact| fact.key)
            .collect();
        assert_eq!(
            keys,
            [
                "Hash",
                "Downloaded",
                "Uploaded",
                "Ratio",
                "Pieces",
                "Peers",
                "Down rate",
                "Up rate",
                "Path",
                "Added",
                "Private",
            ]
        );
    }

    #[test]
    fn uploaded_is_the_ratio_inverted() {
        let mut t = torrent();
        t.bytes_done = 2_469_000_000;
        t.ratio = 0.18;
        assert_eq!(uploaded_bytes(&t), 444_420_000);
        // Nothing uploaded, and a ratio the daemon could not compute.
        t.ratio = 0.0;
        assert_eq!(uploaded_bytes(&t), 0);
        t.ratio = f64::NAN;
        assert_eq!(uploaded_bytes(&t), 0);
        t.ratio = -1.0;
        assert_eq!(uploaded_bytes(&t), 0);
    }

    #[test]
    fn each_rate_carries_its_direction_and_a_dash_when_idle() {
        let mut t = torrent();
        t.down_rate = 8_808_038;
        let facts = primary(&t, None);
        let down = facts.iter().find(|f| f.key == "Down rate").unwrap();
        assert_eq!(down.value, "8.4 MiB/s");
        assert_eq!(down.tone, Tone::Rate);
        let up = facts.iter().find(|f| f.key == "Up rate").unwrap();
        assert_eq!(up.value, "—");
        assert_eq!(up.tone, Tone::Up);
    }

    #[test]
    fn private_is_answered_in_both_directions() {
        assert_eq!(value(&primary(&torrent(), None), "Private"), "no");
        let mut t = torrent();
        t.is_private = true;
        assert_eq!(value(&primary(&t, None), "Private"), "yes");
    }

    #[test]
    fn the_path_dashes_until_the_daemon_reports_one() {
        assert_eq!(value(&primary(&torrent(), None), "Path"), "—");
    }

    #[test]
    fn the_piece_map_reads_as_a_count_at_a_chunk_size() {
        let mut t = torrent();
        t.peers_connected = 112;
        assert_eq!(value(&primary(&t, None), "Peers"), "112");

        let pieces = PieceInfo {
            size_chunks: 1_428,
            completed_chunks: 900,
            chunk_size: 2_097_152,
            bitfield: String::new(),
            availability: None,
        };
        assert_eq!(
            value(&primary(&t, Some(&pieces)), "Pieces"),
            "1,428 × 2.0 MiB"
        );
    }

    #[test]
    fn added_names_the_author_and_the_date() {
        assert_eq!(added_text(&torrent()), "—");
        let mut t = torrent();
        t.added_by = "alice".into();
        assert_eq!(added_text(&t), "alice");
        t.added_at = 1_700_000_000;
        assert!(added_text(&t).starts_with("alice · "), "{}", added_text(&t));
        // A date with no author still shows the date.
        let mut t = torrent();
        t.added_at = 1_700_000_000;
        assert!(added_text(&t).starts_with("unknown · "));
    }

    #[test]
    fn our_facts_follow_the_handoffs_and_keep_the_state_it_had_no_field_for() {
        let keys: Vec<&str> = extra(&torrent(), &globals(), &[])
            .iter()
            .map(|fact| fact.key)
            .collect();
        assert_eq!(
            keys,
            [
                "Size",
                "ETA",
                "Connections",
                "Down limit",
                "Up limit",
                "Peer cap",
                "Peer floor",
                "Upload slots",
            ]
        );
    }

    #[test]
    fn a_limit_says_whether_it_is_the_torrents_or_the_sessions() {
        let mut globals = globals();
        globals.down_rate_limit = 1_048_576;
        assert_eq!(
            value(&extra(&torrent(), &globals, &[]), "Down limit"),
            "1.0 MiB/s · global"
        );
        // A session with no limit is unlimited, and 0 is rtorrent's "off".
        assert_eq!(
            value(&extra(&torrent(), &globals, &[]), "Up limit"),
            "∞ · global"
        );

        let mut t = torrent();
        t.down_rate_limit = Some(2048);
        t.throttle_name = "lan".into();
        assert_eq!(
            value(&extra(&t, &globals, &[]), "Down limit"),
            "2.0 KiB/s · torrent override"
        );

        // `Some(0)` is unlimited *within that named group* — the torrent's own
        // setting, so it keeps the name rather than falling back to the global.
        t.up_rate_limit = Some(0);
        assert_eq!(
            value(&extra(&t, &globals, &[]), "Up limit"),
            "∞ · torrent override"
        );

        // A torrent with a limit but no named group still owns it.
        let mut solo = torrent();
        solo.down_rate_limit = Some(1024);
        assert_eq!(
            value(&extra(&solo, &globals, &[]), "Down limit"),
            "1.0 KiB/s · torrent override"
        );
    }

    #[test]
    fn an_unset_cap_reads_as_the_daemons_default() {
        let facts = extra(&torrent(), &globals(), &[]);
        assert_eq!(value(&facts, "Peer cap"), "default");
        assert_eq!(value(&facts, "Peer floor"), "default");
        assert_eq!(value(&facts, "Upload slots"), "default");
    }

    #[test]
    fn connections_count_seeds_and_peers_together() {
        let mut t = torrent();
        t.seeds_connected = 38;
        t.peers_connected = 112;
        assert_eq!(value(&extra(&t, &globals(), &[]), "Connections"), "150");
    }

    #[test]
    fn provenance_appears_only_when_there_is_some() {
        let plain: Vec<&str> = extra(&torrent(), &globals(), &[])
            .iter()
            .map(|fact| fact.key)
            .collect();
        assert!(!plain.contains(&"Source"));
        assert!(!plain.contains(&"Mode"));
        assert!(!plain.contains(&"DHT / PEX"));

        let mut t = torrent();
        t.source_path = "/inbox/debian.torrent".into();
        t.connection_type = "initial_seed".into();
        t.is_private = true;
        let facts = extra(&t, &globals(), &[]);
        assert_eq!(value(&facts, "Source"), "/inbox/debian.torrent");
        assert_eq!(value(&facts, "Mode"), "super-seeding");
        assert_eq!(value(&facts, "DHT / PEX"), "off — private torrent");
    }
}
