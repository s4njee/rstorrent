//! The server-side rate history behind the Stats route (WC8-S2).
//!
//! The daemon reports a rate, never a history, so the poller samples one on its
//! slow cadence and this keeps a bounded window. Two bounds apply: by age (the
//! design's 60-minute window) and by count (a cap that holds even if the cadence
//! is misconfigured), so a long-running seedbox cannot grow the buffer without
//! limit.

use std::collections::VecDeque;

use serde::Serialize;

/// How much history to keep: the design's throughput window.
pub const WINDOW_MS: i64 = 60 * 60 * 1000;
/// Hard cap on samples (60 min ÷ 15 s, with headroom).
pub const MAX_SAMPLES: usize = 400;

/// One sample of the global rates at an instant (Unix milliseconds).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsSample {
    pub at: i64,
    /// Bytes per second; never negative.
    pub down: i64,
    pub up: i64,
}

/// A bounded, time-trimmed ring of rate samples.
#[derive(Debug, Default)]
pub struct RateHistory {
    samples: VecDeque<StatsSample>,
}

impl RateHistory {
    /// Record a sample. A timestamp that does not advance past the last one is
    /// ignored — a clock step back must not reorder or duplicate the window.
    pub fn record(&mut self, at_ms: i64, down: i64, up: i64) {
        if let Some(last) = self.samples.back() {
            if at_ms <= last.at {
                return;
            }
        }
        self.samples.push_back(StatsSample {
            at: at_ms,
            down: down.max(0),
            up: up.max(0),
        });
        self.trim(at_ms);
        while self.samples.len() > MAX_SAMPLES {
            self.samples.pop_front();
        }
    }

    fn trim(&mut self, now_ms: i64) {
        while let Some(front) = self.samples.front() {
            if now_ms.saturating_sub(front.at) > WINDOW_MS {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    /// A snapshot of the window, oldest first.
    #[must_use]
    pub fn to_vec(&self) -> Vec<StatsSample> {
        self.samples.iter().copied().collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_older_than_the_window_are_dropped() {
        let mut history = RateHistory::default();
        history.record(0, 10, 1);
        history.record(WINDOW_MS, 20, 2);
        // The first is exactly one window old; it goes.
        history.record(WINDOW_MS + 1, 30, 3);
        let samples = history.to_vec();
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].down, 20);
        assert_eq!(samples[1].down, 30);
    }

    #[test]
    fn the_buffer_is_capped() {
        let mut history = RateHistory::default();
        for i in 0..(MAX_SAMPLES + 50) {
            history.record(i as i64, 1, 1);
        }
        assert_eq!(history.len(), MAX_SAMPLES);
    }

    #[test]
    fn non_advancing_timestamps_are_ignored() {
        let mut history = RateHistory::default();
        history.record(1000, 5, 5);
        history.record(1000, 9, 9); // same instant
        history.record(500, 7, 7); // clock stepped back
        let samples = history.to_vec();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].down, 5);
    }

    #[test]
    fn negative_rates_clamp_to_zero() {
        let mut history = RateHistory::default();
        history.record(1, -5, -1);
        assert_eq!(history.to_vec()[0].down, 0);
    }

    #[test]
    fn a_cold_history_is_empty_not_an_error() {
        let history = RateHistory::default();
        assert!(history.is_empty());
        assert!(history.to_vec().is_empty());
    }
}
