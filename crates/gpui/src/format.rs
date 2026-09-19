//! Human-readable formatting for sizes, rates, durations and ratios.
//!
//! A port of the pure `src/utils/format.ts` module: binary units (KiB/MiB/…)
//! throughout, matching rtorrent and the design reference. Kept independent of
//! GPUI so it can be unit-tested directly.

const BINARY_UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];

/// Choose a decimal count: one place under 10, none at/above.
fn decimals(value: f64) -> usize {
    usize::from(value < 10.0)
}

/// Format a byte count, e.g. 6227702349 → "5.8 GiB", 661651456 → "631 MiB".
/// `B` is always shown without decimals.
#[must_use]
pub fn bytes(bytes: i64) -> String {
    if bytes <= 0 {
        return "0 B".to_owned();
    }
    #[allow(clippy::cast_precision_loss)]
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < BINARY_UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let text = if unit == 0 {
        format!("{value:.0}")
    } else {
        format!("{value:.decimals$}", decimals = decimals(value))
    };
    format!("{} {}", text, BINARY_UNITS[unit])
}

/// Format a transfer rate, e.g. 8808038 → "8.4 MiB/s", 634880 → "620 KiB/s".
#[must_use]
pub fn rate(bytes_per_sec: i64) -> String {
    if bytes_per_sec <= 0 {
        return "0 B/s".to_owned();
    }
    format!("{}/s", bytes(bytes_per_sec))
}

/// The Down cell: rate when moving, "0 B/s" while actively (down)loading at
/// zero, and "—" otherwise (seeding/paused/error have no download).
#[must_use]
pub fn down_cell(rate_bps: i64, status: rtorrent_core::types::Status) -> String {
    use rtorrent_core::types::Status::{Downloading, Stalled};
    if rate_bps > 0 {
        return rate(rate_bps);
    }
    match status {
        Downloading | Stalled => "0 B/s".to_owned(),
        _ => "—".to_owned(),
    }
}

/// The Up cell: rate when uploading, else "—".
#[must_use]
pub fn up_cell(rate_bps: i64) -> String {
    if rate_bps > 0 {
        rate(rate_bps)
    } else {
        "—".to_owned()
    }
}

/// Compact duration, e.g. 252 → "4m12s", 820 → "13m40s", 45 → "45s".
#[must_use]
pub fn duration(total_seconds: i64) -> String {
    let seconds = total_seconds.max(0);
    if seconds < 60 {
        return format!("{seconds}s");
    }
    if seconds < 3600 {
        return format!("{}m{}s", seconds / 60, seconds % 60);
    }
    format!("{}h{}m", seconds / 3600, (seconds % 3600) / 60)
}

/// The ETA cell. A finite value formats as a duration; otherwise the status
/// decides: seeding/stalled show ∞ (indefinite), everything else shows —.
#[must_use]
pub fn eta(eta_seconds: Option<i64>, status: rtorrent_core::types::Status) -> String {
    use rtorrent_core::types::Status::{Seeding, Stalled};
    match eta_seconds {
        Some(seconds) => duration(seconds),
        None => match status {
            Seeding | Stalled => "∞".to_owned(),
            _ => "—".to_owned(),
        },
    }
}

/// Share ratio to two decimals, e.g. 0.19, 2.41.
#[must_use]
pub fn ratio(value: f64) -> String {
    format!("{value:.2}")
}

/// A Unix-seconds timestamp as a compact local date for the Started/Finished
/// columns, e.g. `12 Mar`, or `12 Mar 25` once the year differs from today's.
///
/// 0 (rtorrent's "unknown") renders as —. Absolute, not relative: it does not
/// drift between polls and needs no ticking clock, matching `formatDate`.
#[must_use]
pub fn date(unix_seconds: i64) -> String {
    use chrono::{Datelike, Local, TimeZone};

    if unix_seconds <= 0 {
        return "—".to_owned();
    }
    let Some(at) = Local.timestamp_opt(unix_seconds, 0).single() else {
        return "—".to_owned();
    };
    if at.year() == Local::now().year() {
        at.format("%-d %b").to_string()
    } else {
        at.format("%-d %b %y").to_string()
    }
}

/// A Unix-milliseconds timestamp as a wall clock, e.g. `14:04:07`.
///
/// For the log, where the time of day is the useful part and the date is not:
/// the entries are all from this session.
#[must_use]
pub fn clock(unix_millis: i64) -> String {
    use chrono::{Local, TimeZone};

    if unix_millis <= 0 {
        return String::new();
    }
    Local
        .timestamp_millis_opt(unix_millis)
        .single()
        .map_or_else(String::new, |at| at.format("%H:%M:%S").to_string())
}

/// Free-space status-bar string, or empty when unknown.
#[must_use]
pub fn free(free_bytes: Option<i64>) -> String {
    free_bytes.map_or_else(String::new, |free| format!("free: {}", bytes(free)))
}

/// Group digits in threes, e.g. 1428 → "1,428", 1204 → "1,204".
///
/// For the counts the design writes with separators (`1,204 nodes`) where the
/// comma is part of the figure rather than a locale's business — the app is
/// single-locale, and `toLocaleString` has no Rust counterpart without a crate.
#[must_use]
pub fn count(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if value < 0 {
        out.push('-');
    }
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// The current Unix time in seconds, for the relative formats below.
#[must_use]
pub fn now_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()),
    )
    .unwrap_or(i64::MAX)
}

/// Elapsed time since a past Unix time, e.g. "4m12s ago".
///
/// "—" for 0 (a tracker that has never announced) rather than a 56-year age.
#[must_use]
pub fn ago(unix_seconds: i64, now: i64) -> String {
    if unix_seconds <= 0 {
        return "—".to_owned();
    }
    format!("{} ago", duration((now - unix_seconds).max(0)))
}

/// Countdown to a future Unix time, e.g. "in 12m".
///
/// "—" for 0 (unset) or a time already past: rtorrent leaves the next announce
/// in the past for a tracker that is overdue or failing, and would otherwise
/// read as informative.
#[must_use]
pub fn countdown(unix_seconds: i64, now: i64) -> String {
    if unix_seconds <= 0 {
        return "—".to_owned();
    }
    let delta = unix_seconds - now;
    if delta <= 0 {
        return "—".to_owned();
    }
    format!("in {}", duration(delta))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtorrent_core::types::Status;

    #[test]
    fn byte_counts_use_binary_units_with_mockup_decimals() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(634_880), "620 KiB");
        assert_eq!(bytes(661_651_456), "631 MiB");
        assert_eq!(bytes(6_227_702_349), "5.8 GiB");
    }

    #[test]
    fn rates_append_per_second() {
        assert_eq!(rate(0), "0 B/s");
        assert_eq!(rate(8_808_038), "8.4 MiB/s");
    }

    #[test]
    fn down_cell_distinguishes_stalled_from_seeding() {
        assert_eq!(down_cell(100, Status::Downloading), rate(100));
        assert_eq!(down_cell(0, Status::Downloading), "0 B/s");
        assert_eq!(down_cell(0, Status::Stalled), "0 B/s");
        assert_eq!(down_cell(0, Status::Seeding), "—");
        assert_eq!(down_cell(0, Status::Paused), "—");
    }

    #[test]
    fn durations_compact() {
        assert_eq!(duration(45), "45s");
        assert_eq!(duration(252), "4m12s");
        assert_eq!(duration(820), "13m40s");
    }

    #[test]
    fn eta_cell_falls_back_by_status() {
        assert_eq!(eta(Some(252), Status::Downloading), "4m12s");
        assert_eq!(eta(None, Status::Seeding), "∞");
        assert_eq!(eta(None, Status::Stalled), "∞");
        assert_eq!(eta(None, Status::Paused), "—");
    }

    #[test]
    fn ratios_have_two_decimals() {
        assert_eq!(ratio(0.19), "0.19");
        assert_eq!(ratio(2.41), "2.41");
    }

    #[test]
    fn free_space_reports_empty_when_unknown() {
        assert_eq!(free(None), "");
        assert_eq!(free(Some(412 * 1_073_741_824)), "free: 412 GiB");
    }

    #[test]
    fn a_clock_reads_as_a_wall_time() {
        // 2026-09-15 12:00:00 UTC rendered wherever this runs: the hour depends
        // on the zone, so only the shape is asserted.
        let stamp = clock(1_789_473_600_000);
        assert_eq!(stamp.len(), 8, "{stamp}");
        assert_eq!(stamp.matches(':').count(), 2, "{stamp}");
        // Unknown and nonsense stamps render as nothing rather than 1970.
        assert_eq!(clock(0), "");
        assert_eq!(clock(-1), "");
    }

    #[test]
    fn counts_group_in_threes() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1_204), "1,204");
        assert_eq!(count(1_428), "1,428");
        assert_eq!(count(1_234_567), "1,234,567");
        assert_eq!(count(-4_200), "-4,200");
    }

    #[test]
    fn relative_times_read_as_ago_and_in() {
        let now = 1_700_000_000;
        assert_eq!(ago(0, now), "—");
        assert_eq!(ago(now - 252, now), "4m12s ago");
        // A clock skew must not produce a negative age.
        assert_eq!(ago(now + 100, now), "0s ago");

        assert_eq!(countdown(0, now), "—");
        assert_eq!(countdown(now + 720, now), "in 12m0s");
        // Overdue and failing trackers sit in the past: a dash, not "in -3m".
        assert_eq!(countdown(now - 180, now), "—");
    }

    #[test]
    fn dates_elide_the_current_year_and_hide_unknown_ones() {
        assert_eq!(date(0), "—");
        assert_eq!(date(-1), "—");
        // A fixed instant in mid-November, so the month is the same in every
        // time zone and the test does not depend on when it runs. The year
        // suffix is only asserted as "present or absent", since whether it
        // appears depends on the current year.
        let mid_november = date(1_700_000_000);
        assert!(mid_november.contains("Nov"), "{mid_november}");
        assert!(mid_november.starts_with("14") || mid_november.starts_with("15"));
    }
}
