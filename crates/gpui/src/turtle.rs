//! The local wall clock for the scheduler.
//!
//! Window evaluation itself lives in [`rtorrent_core::schedule`], which
//! reasons about plain `(weekday, minute)` pairs; this module is the single
//! place that reads the platform clock, so wall-clock semantics (and with
//! them DST behavior) have exactly one implementation to audit.

/// The current local weekday (Sunday = 0) and minute-of-day, which is what
/// the schedule reasons about. Read from the platform local clock, so a
/// window means "23:00 local time" across daylight-saving shifts by
/// construction — no UTC arithmetic appears anywhere on this path.
#[must_use]
pub fn local_day_minute() -> (u8, i64) {
    use chrono::{Datelike, Local, Timelike};
    let now = Local::now();
    (
        now.weekday().num_days_from_sunday() as u8,
        i64::from(now.hour() * 60 + now.minute()),
    )
}
