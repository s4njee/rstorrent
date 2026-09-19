//! Rich scheduler (V3-18 / QUE-05): a weekly grid of windows with per-day
//! selection, limit-vs-pause modes, a temporary override, and a next-change
//! preview.
//!
//! This evolves turtle mode's single daily window (B14): one window becomes a
//! list, and each window chooses between capping the global rates (Limit) and
//! halting all traffic (Pause). Precedence, highest first:
//!
//! 1. a live temporary override,
//! 2. the first matching Pause window (list order),
//! 3. the first matching Limit window (list order),
//! 4. the manual turtle toggle (a Limit at the turtle rates),
//! 5. open (the global rates).
//!
//! Everything here reasons about wall-clock `(weekday, minute)` handed in by
//! the hosts, which read it from the platform local clock (`chrono::Local` in
//! both desktop pollers). That indirection is the whole of the DST story: a
//! window means "23:00 local time", and local time is what the clock says —
//! across a spring-forward the 02:xx minutes simply never occur, across a
//! fall-back they occur twice and the window covers both. No UTC arithmetic
//! appears anywhere in this module, so there is nothing to get wrong twice a
//! year. The browser cannot call this crate; the TS side mirrors nothing —
//! the schedule is evaluated host-side and only its outcomes cross IPC.
//!
//! Pause is enforced by pausing every active torrent on window *entry* and
//! releasing them on exit (see [`SchedMemory`]); torrents started mid-window
//! are left running, and a manual resume mid-window wins until the next
//! entry. The queue scheduler is told through its own `user_paused` set, so
//! the two never fight over the same hash.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// One grid window: selected weekdays (0 = Sunday, empty = every day),
/// a half-open `[start, end)` minute range that may wrap past midnight, and
/// what applies inside it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedWindow {
    #[serde(default)]
    pub days: Vec<u8>,
    pub start_min: i64,
    pub end_min: i64,
    /// Pause all traffic instead of capping it.
    #[serde(default)]
    pub pause: bool,
    pub down_kb: i64,
    pub up_kb: i64,
}

/// A temporary override: force a mode until a wall-clock instant (unix ms).
/// Expired (`until_ms <= now`) means absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TempOverride {
    pub pause: bool,
    pub down_kb: i64,
    pub up_kb: i64,
    pub until_ms: i64,
}

/// The weekly grid plus an optional override.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Schedule {
    #[serde(default)]
    pub windows: Vec<SchedWindow>,
    #[serde(default)]
    pub temp_override: Option<TempOverride>,
}

/// What the scheduler wants right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchedState {
    Open,
    Limited { down_kb: i64, up_kb: i64 },
    Paused,
}

impl SchedState {
    #[must_use]
    pub fn is_paused(&self) -> bool {
        matches!(self, SchedState::Paused)
    }

    /// The turtle-active flag the snapshot already carries: anything but
    /// open counts, so the status bar and tray keep working unchanged.
    #[must_use]
    pub fn turtle_active(&self) -> bool {
        !matches!(self, SchedState::Open)
    }
}

/// A legacy single turtle window, for hosts to import when the grid is
/// empty. The window carries the turtle rates (that is what the old schedule
/// meant: "apply the turtle rates during this window").
#[derive(Clone, Debug, Default)]
pub struct LegacyWindow {
    pub enabled: bool,
    pub start_min: i64,
    pub end_min: i64,
    pub days: Vec<u8>,
    pub down_kb: i64,
    pub up_kb: i64,
}

/// Import a legacy turtle window as a one-element grid. `None` when there is
/// nothing to import (disabled, or a zero-length window, which has always
/// meant "off").
#[must_use]
pub fn import_legacy(legacy: &LegacyWindow) -> Vec<SchedWindow> {
    if !legacy.enabled || legacy.start_min == legacy.end_min {
        return Vec::new();
    }
    vec![SchedWindow {
        days: legacy.days.clone(),
        start_min: legacy.start_min,
        end_min: legacy.end_min,
        pause: false,
        down_kb: legacy.down_kb,
        up_kb: legacy.up_kb,
    }]
}

/// Does one window cover `(weekday, minute)`? Day filter on the current day;
/// overnight ranges wrap past midnight. Zero-length means off, never always.
fn window_covers(window: &SchedWindow, weekday: u8, minute: i64) -> bool {
    if !window.days.is_empty() && !window.days.contains(&weekday) {
        return false;
    }
    if window.start_min == window.end_min {
        return false;
    }
    if window.start_min < window.end_min {
        minute >= window.start_min && minute < window.end_min
    } else {
        minute >= window.start_min || minute < window.end_min
    }
}

/// Evaluate the grid (already including any imported legacy window) plus an
/// optional manual toggle expressed as limit rates. Override first, then the
/// first matching Pause window, then the first matching Limit window.
#[must_use]
pub fn evaluate(
    windows: &[SchedWindow],
    over: Option<&TempOverride>,
    manual: Option<(i64, i64)>,
    weekday: u8,
    minute: i64,
    now_ms: i64,
) -> SchedState {
    if let Some(o) = over {
        if o.until_ms > now_ms {
            if o.pause {
                return SchedState::Paused;
            }
            return SchedState::Limited {
                down_kb: o.down_kb,
                up_kb: o.up_kb,
            };
        }
    }
    if windows
        .iter()
        .any(|w| w.pause && window_covers(w, weekday, minute))
    {
        return SchedState::Paused;
    }
    if let Some(w) = windows
        .iter()
        .find(|w| !w.pause && window_covers(w, weekday, minute))
    {
        return SchedState::Limited {
            down_kb: w.down_kb,
            up_kb: w.up_kb,
        };
    }
    if let Some((down_kb, up_kb)) = manual {
        return SchedState::Limited { down_kb, up_kb };
    }
    SchedState::Open
}

/// The kind of state a transition leads into (rates live on the windows, not
/// here — the preview names the change, the poller applies it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchedKind {
    Open,
    Limited,
    Paused,
}

impl From<&SchedState> for SchedKind {
    fn from(state: &SchedState) -> Self {
        match state {
            SchedState::Open => SchedKind::Open,
            SchedState::Limited { .. } => SchedKind::Limited,
            SchedState::Paused => SchedKind::Paused,
        }
    }
}

/// The next scheduled change: how far away, on which weekday, at what minute,
/// and what it leads into. Override expiry counts as a change (the state
/// past it is whatever the grid says).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    /// Minutes from now until the change.
    pub in_minutes: i64,
    /// Weekday of the change (0 = Sunday).
    pub weekday: u8,
    /// Minute-of-day of the change.
    pub minute: i64,
    pub to: SchedKind,
}

/// Preview the next change by walking window boundaries over the coming eight
/// days and re-evaluating past each one. Eight days guarantees a hit for any
/// weekly pattern (a window covering "now" still ends within seven days).
#[must_use]
pub fn next_change(
    windows: &[SchedWindow],
    over: Option<&TempOverride>,
    manual: Option<(i64, i64)>,
    weekday: u8,
    minute: i64,
    now_ms: i64,
) -> Option<Transition> {
    let current = SchedKind::from(&evaluate(windows, over, manual, weekday, minute, now_ms));
    // Candidate events: every window boundary on every covered weekday over
    // the coming eight days, plus the override expiry. Absolute minutes from
    // now, so overnight and week wraps need no special cases.
    let mut events: Vec<(i64, u8, i64)> = Vec::new();
    for offset in 0..8 {
        let day = (weekday + offset) % 7;
        for w in windows {
            if w.start_min == w.end_min {
                continue;
            }
            if !w.days.is_empty() && !w.days.contains(&day) {
                continue;
            }
            events.push((i64::from(offset) * 1440 + w.start_min, day, w.start_min));
            let end_day = if w.end_min <= w.start_min {
                (day + 1) % 7
            } else {
                day
            };
            let end_offset = if w.end_min <= w.start_min {
                i64::from(offset) * 1440 + 1440 + w.end_min
            } else {
                i64::from(offset) * 1440 + w.end_min
            };
            events.push((end_offset, end_day, w.end_min));
        }
    }
    if let Some(o) = over {
        if o.until_ms > now_ms {
            // Expiry is a wall-clock instant; express it in the same absolute
            // frame as the window events (minutes since day-0 midnight).
            let in_minutes = (o.until_ms - now_ms).div_euclid(60_000);
            let total = minute + in_minutes;
            events.push((
                total,
                (weekday + (total.div_euclid(1440) as u8)) % 7,
                total.rem_euclid(1440),
            ));
        }
    }
    events.sort();
    events.dedup();
    let now_abs = minute;
    for (at, day, min) in events {
        if at <= now_abs {
            continue;
        }
        // State just past the boundary (minute +1, carrying the day over).
        let (pday, pmin) = if min + 1 >= 1440 {
            ((day + 1) % 7, 0)
        } else {
            (day, min + 1)
        };
        let after = SchedKind::from(&evaluate(
            windows,
            over,
            manual,
            pday,
            pmin,
            now_ms + (at - now_abs) * 60_000,
        ));
        if after != current {
            return Some(Transition {
                in_minutes: at - now_abs,
                weekday: day,
                minute: min,
                to: after,
            });
        }
    }
    None
}

/// Host-side pause memory: which hashes this scheduler paused on entry, so
/// exit releases exactly those (and the queue, told through its own
/// `user_paused` set, never mistakes them for manual pauses).
#[derive(Clone, Debug, Default)]
pub struct SchedMemory {
    was_paused: bool,
    held: HashSet<String>,
}

impl SchedMemory {
    /// Enter/exit bookkeeping around the evaluated state. Returns the hashes
    /// to pause now and to release now; the caller applies them and mirrors
    /// both sets into the queue's `user_paused`.
    pub fn transition(
        &mut self,
        paused_now: bool,
        active: impl Iterator<Item = String>,
    ) -> TransitionActions {
        let mut pause = Vec::new();
        let mut release = Vec::new();
        if paused_now && !self.was_paused {
            // Entry: pause everything currently active.
            for hash in active {
                self.held.insert(hash.clone());
                pause.push(hash);
            }
        } else if !paused_now && self.was_paused {
            // Exit: release what entry paused (plus anything recorded since).
            release.extend(self.held.drain());
        }
        self.was_paused = paused_now;
        TransitionActions { pause, release }
    }

    /// Forget a hash the user resumed mid-window: it stays running until the
    /// next entry, and exit will not touch it either.
    pub fn user_resumed(&mut self, hash: &str) {
        self.held.remove(hash);
    }

    /// Reset on (re)connect alongside every other policy memory.
    pub fn reset(&mut self) {
        self.was_paused = false;
        self.held.clear();
    }

    #[must_use]
    pub fn held(&self) -> &HashSet<String> {
        &self.held
    }
}

/// Hashes to pause on window entry and to release on exit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransitionActions {
    pub pause: Vec<String>,
    pub release: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(days: Vec<u8>, start: i64, end: i64, pause: bool) -> SchedWindow {
        SchedWindow {
            days,
            start_min: start,
            end_min: end,
            pause,
            down_kb: 100,
            up_kb: 50,
        }
    }

    #[test]
    fn empty_grid_is_open_without_a_toggle() {
        assert_eq!(evaluate(&[], None, None, 3, 600, 0), SchedState::Open);
    }

    #[test]
    fn limit_window_applies_its_rates() {
        let grid = vec![window(vec![], 1380, 480, false)];
        assert_eq!(
            evaluate(&grid, None, None, 3, 1400, 0),
            SchedState::Limited {
                down_kb: 100,
                up_kb: 50
            }
        );
        assert_eq!(evaluate(&grid, None, None, 3, 600, 0), SchedState::Open);
    }

    #[test]
    fn day_filter_and_overnight_wrap() {
        // Weekdays only, 22:00–02:00. Wednesday 23:00 matches; Saturday does
        // not; Thursday 01:00 matches via the overnight spill.
        let grid = vec![window(vec![1, 2, 3, 4, 5], 1320, 120, false)];
        assert!(matches!(
            evaluate(&grid, None, None, 3, 1380, 0),
            SchedState::Limited { .. }
        ));
        assert_eq!(evaluate(&grid, None, None, 6, 1380, 0), SchedState::Open);
        assert!(matches!(
            evaluate(&grid, None, None, 4, 60, 0),
            SchedState::Limited { .. }
        ));
    }

    #[test]
    fn pause_beats_limit_and_override_beats_pause() {
        let grid = vec![
            window(vec![], 0, 1440, false),
            window(vec![], 600, 660, true),
        ];
        assert!(matches!(
            evaluate(&grid, None, None, 2, 630, 0),
            SchedState::Paused
        ));
        assert!(matches!(
            evaluate(&grid, None, None, 2, 700, 0),
            SchedState::Limited { .. }
        ));
        let over = TempOverride {
            pause: false,
            down_kb: 10,
            up_kb: 5,
            until_ms: 1_000_000,
        };
        assert_eq!(
            evaluate(&grid, Some(&over), None, 2, 630, 0),
            SchedState::Limited {
                down_kb: 10,
                up_kb: 5
            }
        );
        // Expired overrides vanish.
        assert!(matches!(
            evaluate(&grid, Some(&over), None, 2, 630, 1_000_000),
            SchedState::Paused
        ));
    }

    #[test]
    fn manual_toggle_is_a_limit_under_open_sky() {
        let grid = vec![window(vec![], 600, 660, true)];
        assert_eq!(
            evaluate(&grid, None, Some((200, 100)), 2, 700, 0),
            SchedState::Limited {
                down_kb: 200,
                up_kb: 100
            }
        );
        assert!(matches!(
            evaluate(&grid, None, Some((200, 100)), 2, 630, 0),
            SchedState::Paused
        ));
    }

    #[test]
    fn legacy_import_needs_an_enabled_nonempty_window() {
        let legacy = LegacyWindow {
            enabled: true,
            start_min: 1320,
            end_min: 360,
            days: vec![1, 2, 3],
            down_kb: 100,
            up_kb: 50,
        };
        let grid = import_legacy(&legacy);
        assert_eq!(grid.len(), 1);
        assert_eq!(grid[0].days, vec![1, 2, 3]);
        assert!(!grid[0].pause);
        let off = LegacyWindow {
            enabled: false,
            ..legacy.clone()
        };
        assert!(import_legacy(&off).is_empty());
        let empty = LegacyWindow {
            enabled: true,
            start_min: 600,
            end_min: 600,
            ..legacy
        };
        assert!(import_legacy(&empty).is_empty());
    }

    #[test]
    fn next_change_names_the_coming_boundary() {
        // One nightly limit window 22:00–06:00, every day. Wednesday 12:00 →
        // the change is 22:00 today, into Limited.
        let grid = vec![window(vec![], 1320, 360, false)];
        let change = next_change(&grid, None, None, 3, 720, 0).expect("a change");
        assert_eq!(change.weekday, 3);
        assert_eq!(change.minute, 1320);
        assert_eq!(change.in_minutes, 600);
        assert_eq!(change.to, SchedKind::Limited);
        // Wednesday 23:00 → the change is 06:00 Thursday, back to Open.
        let change = next_change(&grid, None, None, 3, 1380, 0).expect("a change");
        assert_eq!(change.weekday, 4);
        assert_eq!(change.minute, 360);
        assert_eq!(change.in_minutes, 420);
        assert_eq!(change.to, SchedKind::Open);
    }

    #[test]
    fn next_change_respects_days_and_pause() {
        // Sunday-only pause 09:00–10:00. Saturday noon → Sunday 09:00.
        let grid = vec![window(vec![0], 540, 600, true)];
        let change = next_change(&grid, None, None, 6, 720, 0).expect("a change");
        assert_eq!(change.weekday, 0);
        assert_eq!(change.minute, 540);
        assert_eq!(change.in_minutes, 1260);
        assert_eq!(change.to, SchedKind::Paused);
    }

    #[test]
    fn next_change_is_none_on_an_empty_grid() {
        assert_eq!(next_change(&[], None, None, 3, 720, 0), None);
        assert_eq!(next_change(&[], None, Some((1, 1)), 3, 720, 0), None);
    }

    #[test]
    fn override_expiry_counts_as_a_change() {
        let over = TempOverride {
            pause: true,
            down_kb: 0,
            up_kb: 0,
            until_ms: 30 * 60_000,
        };
        // Wednesday 12:00, override pauses for 30 more minutes.
        let change = next_change(&[], Some(&over), None, 3, 720, 0).expect("a change");
        assert_eq!(change.in_minutes, 30);
        assert_eq!(change.to, SchedKind::Open);
    }

    #[test]
    fn schedule_serialises_camel_case_like_the_ts_contract() {
        let schedule = Schedule {
            windows: vec![SchedWindow {
                days: vec![1],
                start_min: 1320,
                end_min: 360,
                pause: true,
                down_kb: 0,
                up_kb: 0,
            }],
            temp_override: Some(TempOverride {
                pause: false,
                down_kb: 10,
                up_kb: 5,
                until_ms: 99,
            }),
        };
        let value = serde_json::to_value(&schedule).unwrap();
        assert_eq!(value["windows"][0]["startMin"], 1320);
        assert_eq!(value["windows"][0]["pause"], true);
        assert_eq!(value["tempOverride"]["untilMs"], 99);
        // Old files without the key still load (the field defaults).
        let loaded: Schedule = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(loaded.windows.is_empty() && loaded.temp_override.is_none());
    }

    #[test]
    fn scheduler_memory_holds_entry_and_releases_exit() {
        let mut mem = SchedMemory::default();
        let entered = mem.transition(true, ["A".into(), "B".into()].into_iter());
        assert_eq!(entered.pause, vec!["A", "B"]);
        assert!(entered.release.is_empty());
        // Same state again: nothing new to do.
        let again = mem.transition(true, ["C".into()].into_iter());
        assert!(again.pause.is_empty() && again.release.is_empty());
        // A user resume mid-window drops out of the held set...
        mem.user_resumed("A");
        assert!(!mem.held().contains("A"));
        // ...and exit releases only what is left.
        let exited = mem.transition(false, ["B".into()].into_iter());
        assert!(exited.pause.is_empty());
        assert_eq!(exited.release, vec!["B"]);
    }
}
