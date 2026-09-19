//! Per-torrent rate samples, and the geometry that turns them into a chart.
//!
//! rtorrent reports a rate, never a history, so the Transfer pane's chart is
//! drawn from samples this app takes as it polls. A port of
//! `src/store/rateHistory.ts` and the point maths in `details/SpeedChart.tsx`.
//!
//! Pure: the samples are recorded and the points computed here, and the pane
//! only paints them.

use std::collections::{HashMap, HashSet};

use rtorrent_core::types::TorrentDto;

/// How many samples a torrent keeps: ten minutes at the list's cadence.
pub const MAX_POINTS: usize = 600;

/// One sample: the two rates at an instant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RatePoint {
    pub down: i64,
    pub up: i64,
}

/// The samples for the torrents that are still in the list.
#[derive(Default)]
pub struct RateHistory {
    series: HashMap<String, Vec<RatePoint>>,
}

impl RateHistory {
    /// Record one sample per torrent, and forget the hashes that have gone — a
    /// removal must not leave samples behind to grow without bound.
    pub fn record(&mut self, torrents: &[TorrentDto]) {
        let live: HashSet<&str> = torrents
            .iter()
            .map(|torrent| torrent.hash.as_str())
            .collect();
        self.series.retain(|hash, _| live.contains(hash.as_str()));

        for torrent in torrents {
            let samples = self.series.entry(torrent.hash.clone()).or_default();
            samples.push(RatePoint {
                down: torrent.down_rate.max(0),
                up: torrent.up_rate.max(0),
            });
            let excess = samples.len().saturating_sub(MAX_POINTS);
            if excess > 0 {
                samples.drain(..excess);
            }
        }
    }

    /// A torrent's samples, oldest first. Empty for one never seen.
    #[must_use]
    pub fn get(&self, hash: &str) -> &[RatePoint] {
        self.series.get(hash).map_or(&[], Vec::as_slice)
    }

    /// How many torrents are being sampled — the History view's business.
    #[must_use]
    pub fn len(&self) -> usize {
        self.series.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.series.is_empty()
    }
}

/// The peak of either series, floored at 1 so an idle torrent still has a scale
/// to draw against.
#[must_use]
pub fn peak(samples: &[RatePoint]) -> i64 {
    samples
        .iter()
        .map(|sample| sample.down.max(sample.up))
        .max()
        .unwrap_or(0)
        .max(1)
}

/// One series' points: x spread evenly across `width`, y measured down from
/// `top` as a fraction of `peak`.
///
/// Fewer than two samples has no line to draw — the pane says it is still
/// collecting rather than plotting a dot.
#[must_use]
pub fn series_points(
    samples: &[RatePoint],
    up: bool,
    width: f32,
    top: f32,
    height: f32,
    peak: i64,
) -> Vec<(f32, f32)> {
    if samples.len() < 2 {
        return Vec::new();
    }
    #[allow(clippy::cast_precision_loss)]
    let step = width / (samples.len() - 1) as f32;
    #[allow(clippy::cast_precision_loss)]
    let scale = peak.max(1) as f32;

    samples
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            #[allow(clippy::cast_precision_loss)]
            let value = if up { sample.up } else { sample.down } as f32;
            let ratio = (value / scale).clamp(0.0, 1.0);
            #[allow(clippy::cast_precision_loss)]
            let x = index as f32 * step;
            (x, top + height - ratio * height)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtorrent_core::types::Status;

    fn torrent(hash: &str, down: i64, up: i64) -> TorrentDto {
        TorrentDto {
            hash: hash.to_owned(),
            name: hash.to_owned(),
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
            down_rate: down,
            up_rate: up,
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

    #[test]
    fn samples_accumulate_in_order() {
        let mut history = RateHistory::default();
        history.record(&[torrent("A", 10, 1)]);
        history.record(&[torrent("A", 20, 2)]);
        assert_eq!(
            history.get("A"),
            [RatePoint { down: 10, up: 1 }, RatePoint { down: 20, up: 2 }]
        );
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn a_negative_rate_the_daemon_might_report_reads_as_idle() {
        let mut history = RateHistory::default();
        history.record(&[torrent("A", -5, -1)]);
        assert_eq!(history.get("A"), [RatePoint { down: 0, up: 0 }]);
    }

    #[test]
    fn a_departed_torrent_stops_being_sampled() {
        let mut history = RateHistory::default();
        history.record(&[torrent("A", 1, 1), torrent("B", 2, 2)]);
        history.record(&[torrent("B", 3, 3)]);
        assert!(history.get("A").is_empty());
        assert_eq!(history.get("B").len(), 2);
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn the_buffer_is_bounded() {
        let mut history = RateHistory::default();
        for _ in 0..(MAX_POINTS + 5) {
            history.record(&[torrent("A", 1, 1)]);
        }
        assert_eq!(history.get("A").len(), MAX_POINTS);
        // The oldest went, not the newest.
        assert_eq!(history.get("A").last(), Some(&RatePoint { down: 1, up: 1 }));
    }

    #[test]
    fn an_unknown_torrent_has_no_samples() {
        let history = RateHistory::default();
        assert!(history.get("nope").is_empty());
        assert!(history.is_empty());
    }

    #[test]
    fn the_peak_covers_both_series_and_never_reaches_zero() {
        let samples = [RatePoint { down: 5, up: 9 }, RatePoint { down: 40, up: 2 }];
        assert_eq!(peak(&samples), 40);
        // An idle torrent still needs a scale.
        assert_eq!(peak(&[RatePoint { down: 0, up: 0 }]), 1);
        assert_eq!(peak(&[]), 1);
    }

    #[test]
    fn a_line_needs_two_points() {
        assert!(series_points(&[], false, 100.0, 0.0, 50.0, 1).is_empty());
        assert!(
            series_points(&[RatePoint { down: 1, up: 1 }], false, 100.0, 0.0, 50.0, 1).is_empty()
        );
    }

    #[test]
    fn points_spread_across_the_width_and_hang_from_the_top() {
        let samples = [RatePoint { down: 0, up: 0 }, RatePoint { down: 8, up: 4 }];
        let points = series_points(&samples, false, 100.0, 10.0, 50.0, 8);
        assert_eq!(points.len(), 2);
        // The first sample is idle: on the baseline.
        assert_eq!(points[0], (0.0, 60.0));
        // The last is the peak: at the top of the plot.
        assert_eq!(points[1], (100.0, 10.0));

        // The upload series shares the peak, so it reads lower than download —
        // a trickle must not draw as tall as a flood.
        let upload = series_points(&samples, true, 100.0, 10.0, 50.0, 8);
        assert_eq!(upload[1], (100.0, 35.0));
    }

    #[test]
    fn a_rate_above_the_peak_is_clamped_to_the_plot() {
        let samples = [RatePoint { down: 99, up: 0 }, RatePoint { down: 99, up: 0 }];
        let points = series_points(&samples, false, 100.0, 0.0, 50.0, 10);
        assert_eq!(points[0].1, 0.0);
    }
}
