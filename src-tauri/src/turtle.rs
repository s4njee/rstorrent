//! Turtle mode (B14): alternative speed limits, engaged manually or on a daily
//! schedule.
//!
//! The poller calls [`is_active`] each tick with the current local wall-clock
//! (see `chrono::Local` at the call site), then [`effective_limits`] to pick the
//! rate limits to push. The scheduling maths is kept pure here so it can be
//! table-tested without a clock.

use crate::ipc::{Settings, TurtleSchedule};

/// Is the schedule window open at `weekday_sun0` (0 = Sunday) / `minute`
/// (minutes since local midnight)?
///
/// A window with `end <= start` wraps past midnight. The weekday filter is
/// applied to the *current* day, so an overnight window is matched whenever the
/// current day is selected.
pub fn schedule_active(sch: &TurtleSchedule, weekday_sun0: u8, minute: i64) -> bool {
    if !sch.enabled {
        return false;
    }
    if !sch.days.is_empty() && !sch.days.contains(&weekday_sun0) {
        return false;
    }
    // A zero-length window is treated as "off" rather than "always".
    if sch.start_min == sch.end_min {
        return false;
    }
    if sch.start_min < sch.end_min {
        minute >= sch.start_min && minute < sch.end_min
    } else {
        // Overnight: active from start to midnight, and midnight to end.
        minute >= sch.start_min || minute < sch.end_min
    }
}

/// Whether turtle mode is in effect: the manual toggle OR an active schedule.
pub fn is_active(s: &Settings, weekday_sun0: u8, minute: i64) -> bool {
    s.turtle_enabled || schedule_active(&s.turtle_schedule, weekday_sun0, minute)
}

/// The `(down_kb, up_kb)` global limits to apply given the turtle state.
pub fn effective_limits(s: &Settings, turtle_active: bool) -> (i64, i64) {
    if turtle_active {
        (s.turtle_down_kb, s.turtle_up_kb)
    } else {
        (s.down_limit_kb, s.up_limit_kb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sched(start: i64, end: i64, days: Vec<u8>) -> TurtleSchedule {
        TurtleSchedule {
            enabled: true,
            start_min: start,
            end_min: end,
            days,
        }
    }

    #[test]
    fn disabled_schedule_is_never_active() {
        let mut s = sched(0, 600, vec![]);
        s.enabled = false;
        assert!(!schedule_active(&s, 3, 300));
    }

    #[test]
    fn same_day_window() {
        // 02:00 (120) .. 08:00 (480), every day.
        let s = sched(120, 480, vec![]);
        assert!(!schedule_active(&s, 1, 119));
        assert!(schedule_active(&s, 1, 120));
        assert!(schedule_active(&s, 1, 479));
        assert!(!schedule_active(&s, 1, 480));
    }

    #[test]
    fn overnight_window_wraps_midnight() {
        // 23:00 (1380) .. 06:00 (360).
        let s = sched(1380, 360, vec![]);
        assert!(schedule_active(&s, 5, 1400)); // 23:20
        assert!(schedule_active(&s, 6, 60)); // 01:00
        assert!(!schedule_active(&s, 6, 400)); // 06:40
    }

    #[test]
    fn weekday_filter() {
        // Weekdays only (Mon..Fri = 1..5).
        let s = sched(120, 480, vec![1, 2, 3, 4, 5]);
        assert!(schedule_active(&s, 3, 300)); // Wednesday
        assert!(!schedule_active(&s, 0, 300)); // Sunday
        assert!(!schedule_active(&s, 6, 300)); // Saturday
    }

    #[test]
    fn zero_length_window_is_off() {
        assert!(!schedule_active(&sched(300, 300, vec![]), 1, 300));
    }

    #[test]
    fn manual_toggle_forces_active_regardless_of_schedule() {
        let s = Settings {
            turtle_enabled: true,
            ..Default::default()
        };
        assert!(is_active(&s, 1, 999));
    }

    #[test]
    fn effective_limits_switch_on_turtle() {
        let s = Settings {
            down_limit_kb: 5000,
            up_limit_kb: 1000,
            turtle_down_kb: 500,
            turtle_up_kb: 100,
            ..Default::default()
        };
        assert_eq!(effective_limits(&s, false), (5000, 1000));
        assert_eq!(effective_limits(&s, true), (500, 100));
    }
}
