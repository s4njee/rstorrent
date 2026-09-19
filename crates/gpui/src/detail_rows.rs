//! Pure row vocabulary for the Peers and Trackers panes.
//!
//! What a row *says* and which colour family it takes, with no view attached — a
//! port of the helpers the Tauri detail panel keeps beside its JSX
//! (`splitAddress`, `trackerStatusWord` and `trackerTone`).

/// A peer's address split into host and port.
///
/// rtorrent reports `p.address` as `ip:port`, but not always — a bare IP is
/// common enough in fixtures and on older daemons that the port column shows
/// nothing rather than mangling the address. IPv6 is bracketed before the split,
/// so the last colon is the port only when there is one.
#[must_use]
pub fn split_address(address: &str) -> (String, Option<String>) {
    let value = address.trim();
    if value.is_empty() {
        return ("—".to_owned(), None);
    }

    if let Some(rest) = value.strip_prefix('[') {
        if let Some((host, tail)) = rest.split_once(']') {
            let port = tail.strip_prefix(':').unwrap_or_default();
            if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
                return (host.to_owned(), Some(port.to_owned()));
            }
            return (host.to_owned(), None);
        }
    }

    let Some(colon) = value.rfind(':') else {
        return (value.to_owned(), None);
    };
    let port = &value[colon + 1..];
    // A colon inside the host means IPv6 without brackets: not a port.
    if value[..colon].contains(':') || port.is_empty() || !port.chars().all(|c| c.is_ascii_digit())
    {
        return (value.to_owned(), None);
    }
    (value[..colon].to_owned(), Some(port.to_owned()))
}

/// How a tracker row reads: the accent while it works, amber when it is failing,
/// cyan while it is on without announcing, inert when it is off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Working.
    Active,
    Warn,
    /// Timed out or failed.
    Error,
    /// Enabled but quiet — a peer source rather than a tracker.
    Enabled,
    Idle,
}

/// The status word for a tracker row, in the daemon's register.
///
/// A disabled tracker is off rather than failing, so it outranks whatever status
/// rtorrent last left on the row.
#[must_use]
pub fn tracker_word(status: &str, enabled: bool) -> String {
    if !enabled {
        return "disabled".to_owned();
    }
    match status {
        "working" => "working".to_owned(),
        "updating" => "updating".to_owned(),
        "error" | "timeout" | "timed out" => "timed out".to_owned(),
        other => {
            if other.is_empty() {
                "—".to_owned()
            } else {
                other.to_owned()
            }
        }
    }
}

/// The colour family for a tracker row's dot and status.
#[must_use]
pub fn tracker_tone(status: &str, enabled: bool) -> Tone {
    if !enabled {
        return Tone::Idle;
    }
    match status {
        "error" | "timeout" | "timed out" => Tone::Error,
        "updating" | "warning" => Tone::Warn,
        _ => Tone::Active,
    }
}

/// The two peer sources that are not trackers, listed with them: on without
/// announcing, so they take the cyan rather than a tracker's accent.
pub const PEER_SOURCES: [&str; 2] = ["[DHT]", "[Peer exchange]"];

/// What a peer source row reads: off for a private torrent, which turns both off.
#[must_use]
pub fn peer_source_word(is_private: bool) -> &'static str {
    if is_private {
        "off — private"
    } else {
        "enabled"
    }
}

/// The legend for the peer flag letters, shown with the Flags column.
pub const PEER_FLAGS_LEGEND: &str =
    "E encrypted · I incoming · O obfuscated · P preferred · U unwanted";

#[cfg(test)]
mod tests {
    use super::*;

    fn split(address: &str) -> (String, Option<String>) {
        split_address(address)
    }

    #[test]
    fn an_address_splits_into_host_and_port() {
        assert_eq!(
            split("185.21.104.7:51413"),
            ("185.21.104.7".to_owned(), Some("51413".to_owned()))
        );
    }

    #[test]
    fn a_bare_address_keeps_its_whole_host_and_no_port() {
        assert_eq!(split("94.140.8.221"), ("94.140.8.221".to_owned(), None));
        // A trailing non-numeric field is part of the host, not a port.
        assert_eq!(split("host.example"), ("host.example".to_owned(), None));
    }

    #[test]
    fn ipv6_is_bracketed_or_left_whole() {
        assert_eq!(
            split("[2001:db8::1]:6881"),
            ("2001:db8::1".to_owned(), Some("6881".to_owned()))
        );
        assert_eq!(split("2001:db8::1"), ("2001:db8::1".to_owned(), None));
        assert_eq!(split("::1"), ("::1".to_owned(), None));
        // A bracketed host with nothing after it is still all host.
        assert_eq!(split("[2001:db8::1]"), ("2001:db8::1".to_owned(), None));
    }

    #[test]
    fn an_empty_address_reads_as_a_dash() {
        assert_eq!(split("   "), ("—".to_owned(), None));
    }

    #[test]
    fn a_disabled_tracker_reads_as_disabled_whatever_it_last_said() {
        assert_eq!(tracker_word("error", false), "disabled");
        assert_eq!(tracker_tone("error", false), Tone::Idle);
    }

    #[test]
    fn the_failing_spellings_fold_into_one_word() {
        for status in ["error", "timeout", "timed out"] {
            assert_eq!(tracker_word(status, true), "timed out", "{status}");
            assert_eq!(tracker_tone(status, true), Tone::Error, "{status}");
        }
    }

    #[test]
    fn a_working_tracker_takes_the_accent_and_an_updating_one_warns() {
        assert_eq!(tracker_word("working", true), "working");
        assert_eq!(tracker_tone("working", true), Tone::Active);
        assert_eq!(tracker_word("updating", true), "updating");
        assert_eq!(tracker_tone("updating", true), Tone::Warn);
        assert_eq!(tracker_tone("warning", true), Tone::Warn);
    }

    #[test]
    fn an_unknown_status_is_passed_through_rather_than_invented() {
        assert_eq!(tracker_word("queued", true), "queued");
        assert_eq!(tracker_word("", true), "—");
        assert_eq!(tracker_tone("queued", true), Tone::Active);
    }

    #[test]
    fn a_peer_source_is_enabled_unless_the_torrent_is_private() {
        assert_eq!(peer_source_word(false), "enabled");
        assert_eq!(peer_source_word(true), "off — private");
        assert_eq!(PEER_SOURCES.len(), 2);
    }
}
