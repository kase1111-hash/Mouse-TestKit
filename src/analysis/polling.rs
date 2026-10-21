//! Polling rate statistics
//!
//! Tracks min/max/avg polling rate from a stream of Hz samples, and estimates
//! the polling rate from inter-report intervals.
//!
//! A mouse only sends a report when at least one count has accumulated, so
//! while the hand moves slowly (or turns around) it legitimately skips polls
//! and the gaps between reports say nothing about the polling rate. Only
//! intervals between two *saturated* reports moving the same way (see
//! [`is_saturated_pair`]) are known to be at the poll rate;
//! [`saturated_intervals_ms`] extracts them.

/// Standard USB/wireless mouse report rates, used to label a measurement.
pub const NOMINAL_RATES_HZ: [u32; 8] = [125, 250, 500, 1000, 2000, 4000, 8000, 16000];

/// Intervals longer than this are treated as the user pausing, not as a poll.
pub const MAX_POLL_INTERVAL_MS: f64 = 100.0;

/// Fast-movement intervals needed in a window of reports before a rate is
/// published from it ([`window_hz`]). Their median is not moved by a few late
/// reports, and even at 125 Hz this is only 160 ms of fast movement.
pub const MIN_FAST_INTERVALS: usize = 20;

/// The slowest rate a USB mouse runs at is 125 Hz; this leaves room for
/// timing error. Below it a mouse is on Bluetooth, where one report per
/// 7.5-15 ms connection interval (67-133 Hz) is normal, or is a USB mouse
/// set to a low rate. No USB rate is named for such a reading (see
/// [`usb_nominal_rate`]).
pub const USB_MIN_POLLING_HZ: u32 = 110;

/// Counts a report must carry on its larger axis to prove the mouse had a
/// report ready at every poll.
///
/// The sensor keeps the fractional remainder of its motion and reports once a
/// whole count has built up. Below one count per poll that is always exactly
/// one count on the larger axis, with skipped polls before it. Two or more
/// counts in one report can only appear when the hand moves at least about one
/// count per poll (or counts piled up during a real delay), so an interval
/// between two such reports should equal the poll interval.
pub const MIN_SATURATED_COUNTS: i32 = 2;

/// Whether a motion report shows the hand moving fast enough that the mouse
/// reports at every poll (see [`MIN_SATURATED_COUNTS`]).
pub fn is_saturated_report(dx: i32, dy: i32) -> bool {
    dx.saturating_abs().max(dy.saturating_abs()) >= MIN_SATURATED_COUNTS
}

/// Whether the interval between two consecutive motion reports must be at the
/// poll rate: both are saturated and they point the same way (less than 90
/// degrees apart).
///
/// The direction check matters at reversals: a hand turning around inside
/// one poll period moves out and back, the counts cancel and the mouse
/// rightly sends nothing for that poll even though the reports on either
/// side are large. At 125-500 Hz this happens on almost every reversal.
pub fn is_saturated_pair(prev: (i32, i32), next: (i32, i32)) -> bool {
    let dot = prev.0 as i64 * next.0 as i64 + prev.1 as i64 * next.1 as i64;
    is_saturated_report(prev.0, prev.1) && is_saturated_report(next.0, next.1) && dot > 0
}

/// Intervals (ms) between consecutive motion reports that form a saturated
/// pair (see [`is_saturated_pair`]).
///
/// `reports` are `(time_ms, dx, dy)` in arrival order; reports without motion
/// are skipped. Intervals that are non-positive or longer than
/// [`MAX_POLL_INTERVAL_MS`] (the hand stopped or was lifted) are dropped.
pub fn saturated_intervals_ms(reports: &[(f64, i32, i32)]) -> Vec<f64> {
    let mut out = Vec::new();
    let mut prev: Option<(f64, i32, i32)> = None;
    for &(t, dx, dy) in reports {
        if dx == 0 && dy == 0 {
            continue;
        }
        if let Some((prev_t, pdx, pdy)) = prev {
            let interval = t - prev_t;
            if is_saturated_pair((pdx, pdy), (dx, dy))
                && interval > 0.0
                && interval <= MAX_POLL_INTERVAL_MS
            {
                out.push(interval);
            }
        }
        prev = Some((t, dx, dy));
    }
    out
}

/// Running statistics for polling rate measurements.
pub struct PollingStats {
    pub current_hz: u32,
    pub min_hz: u32,
    pub max_hz: u32,
    pub avg_hz: f64,
    pub samples: u32,
}

impl PollingStats {
    pub fn new() -> Self {
        Self {
            current_hz: 0,
            min_hz: u32::MAX,
            max_hz: 0,
            avg_hz: 0.0,
            samples: 0,
        }
    }

    pub fn update(&mut self, hz: u32) {
        if hz == 0 {
            return;
        }
        self.current_hz = hz;
        self.min_hz = self.min_hz.min(hz);
        self.max_hz = self.max_hz.max(hz);
        self.samples += 1;
        self.avg_hz = (self.avg_hz * (self.samples - 1) as f64 + hz as f64) / self.samples as f64;
    }
}

impl Default for PollingStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Median of a slice (the slice is sorted in place). `None` for empty input.
pub fn median_in_place(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        Some((values[mid - 1] + values[mid]) / 2.0)
    } else {
        Some(values[mid])
    }
}

/// Estimate the polling rate in Hz from inter-report intervals (milliseconds).
///
/// Uses the median interval, which is robust against the mouse briefly
/// stopping (long gaps) and against an occasional missed report. Intervals
/// that are non-positive or longer than [`MAX_POLL_INTERVAL_MS`] are ignored.
/// Returns `None` when fewer than `min_samples` usable intervals remain.
pub fn estimate_hz(intervals_ms: &[f64], min_samples: usize) -> Option<u32> {
    let mut usable: Vec<f64> = intervals_ms
        .iter()
        .copied()
        .filter(|&i| i > 0.0 && i <= MAX_POLL_INTERVAL_MS)
        .collect();
    if usable.len() < min_samples.max(1) {
        return None;
    }
    let median = median_in_place(&mut usable)?;
    if median <= 0.0 {
        return None;
    }
    Some((1000.0 / median).round() as u32)
}

/// Polling rate (Hz) of a window of motion reports `(time_ms, dx, dy)`, from
/// its fast-movement intervals only (see [`saturated_intervals_ms`]), or
/// `None` when it holds fewer than [`MIN_FAST_INTERVALS`] of them: the hand
/// moved too slowly to show the rate.
pub fn window_hz(reports: &[(f64, i32, i32)]) -> Option<u32> {
    estimate_hz(&saturated_intervals_ms(reports), MIN_FAST_INTERVALS)
}

/// The standard USB rate nearest a measurement, or `None` below
/// [`USB_MIN_POLLING_HZ`]: there "nominal 125 Hz" would read as a USB mouse
/// running slow, while the rate is normal for Bluetooth.
pub fn usb_nominal_rate(hz: f64) -> Option<u32> {
    (hz >= USB_MIN_POLLING_HZ as f64).then(|| nominal_rate(hz))
}

/// Nearest standard report rate for a measured value (e.g. 986 Hz -> 1000).
pub fn nominal_rate(hz: f64) -> u32 {
    NOMINAL_RATES_HZ
        .iter()
        .copied()
        .min_by(|a, b| {
            let da = (*a as f64 - hz).abs() / *a as f64;
            let db = (*b as f64 - hz).abs() / *b as f64;
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polling_stats_new() {
        let stats = PollingStats::new();
        assert_eq!(stats.current_hz, 0);
        assert_eq!(stats.min_hz, u32::MAX);
        assert_eq!(stats.max_hz, 0);
        assert_eq!(stats.avg_hz, 0.0);
        assert_eq!(stats.samples, 0);
    }

    #[test]
    fn test_polling_stats_default() {
        let stats = PollingStats::default();
        assert_eq!(stats.current_hz, 0);
        assert_eq!(stats.min_hz, u32::MAX);
    }

    #[test]
    fn test_polling_stats_update_single() {
        let mut stats = PollingStats::new();
        stats.update(1000);

        assert_eq!(stats.current_hz, 1000);
        assert_eq!(stats.min_hz, 1000);
        assert_eq!(stats.max_hz, 1000);
        assert_eq!(stats.avg_hz, 1000.0);
        assert_eq!(stats.samples, 1);
    }

    #[test]
    fn test_polling_stats_update_multiple() {
        let mut stats = PollingStats::new();
        stats.update(500);
        stats.update(1000);
        stats.update(1500);

        assert_eq!(stats.current_hz, 1500);
        assert_eq!(stats.min_hz, 500);
        assert_eq!(stats.max_hz, 1500);
        assert_eq!(stats.samples, 3);
        assert!((stats.avg_hz - 1000.0).abs() < 0.001);
    }

    #[test]
    fn test_polling_stats_update_ignores_zero() {
        let mut stats = PollingStats::new();
        stats.update(1000);
        stats.update(0);

        assert_eq!(stats.current_hz, 1000);
        assert_eq!(stats.samples, 1);
        assert_eq!(stats.avg_hz, 1000.0);
    }

    #[test]
    fn test_polling_stats_min_max_tracking() {
        let mut stats = PollingStats::new();
        stats.update(800);
        stats.update(1200);
        stats.update(600);
        stats.update(1400);

        assert_eq!(stats.min_hz, 600);
        assert_eq!(stats.max_hz, 1400);
    }

    #[test]
    fn test_polling_stats_average_calculation() {
        let mut stats = PollingStats::new();
        stats.update(100);
        assert_eq!(stats.avg_hz, 100.0);
        stats.update(200);
        assert!((stats.avg_hz - 150.0).abs() < 0.001);
        stats.update(300);
        assert!((stats.avg_hz - 200.0).abs() < 0.001);
    }

    #[test]
    fn test_polling_stats_large_values() {
        let mut stats = PollingStats::new();
        stats.update(8000);
        assert_eq!(stats.current_hz, 8000);
        assert_eq!(stats.min_hz, 8000);
        assert_eq!(stats.max_hz, 8000);
    }

    #[test]
    fn test_polling_stats_consistent_samples() {
        let mut stats = PollingStats::new();
        for _ in 0..100 {
            stats.update(1000);
        }
        assert_eq!(stats.samples, 100);
        assert_eq!(stats.min_hz, 1000);
        assert_eq!(stats.max_hz, 1000);
        assert!((stats.avg_hz - 1000.0).abs() < 0.001);
    }

    #[test]
    fn test_median_odd_even_empty() {
        assert_eq!(median_in_place(&mut []), None);
        assert_eq!(median_in_place(&mut [3.0]), Some(3.0));
        assert_eq!(median_in_place(&mut [5.0, 1.0, 3.0]), Some(3.0));
        assert_eq!(median_in_place(&mut [4.0, 1.0, 3.0, 2.0]), Some(2.5));
    }

    #[test]
    fn test_estimate_hz_uniform_1000hz() {
        let intervals = vec![1.0; 50];
        assert_eq!(estimate_hz(&intervals, 10), Some(1000));
    }

    #[test]
    fn test_estimate_hz_ignores_pauses_and_garbage() {
        // 125 Hz mouse with two long pauses and a zero-length interval
        let mut intervals = vec![8.0; 30];
        intervals.push(500.0);
        intervals.push(1200.0);
        intervals.push(0.0);
        intervals.push(-1.0);
        assert_eq!(estimate_hz(&intervals, 10), Some(125));
    }

    #[test]
    fn test_estimate_hz_robust_to_single_missed_report() {
        let mut intervals = vec![2.0; 40];
        intervals.push(4.0); // one missed 500 Hz report
        assert_eq!(estimate_hz(&intervals, 10), Some(500));
    }

    #[test]
    fn test_estimate_hz_needs_min_samples() {
        assert_eq!(estimate_hz(&[1.0, 1.0, 1.0], 10), None);
        assert_eq!(estimate_hz(&[], 1), None);
        assert_eq!(estimate_hz(&[150.0, 200.0], 1), None);
    }

    #[test]
    fn test_saturated_report_threshold() {
        assert!(!is_saturated_report(1, 0));
        assert!(!is_saturated_report(-1, 1));
        assert!(is_saturated_report(0, -2));
        assert!(is_saturated_report(5, 1));
        assert!(is_saturated_report(i32::MIN, 0));
    }

    #[test]
    fn test_saturated_pair_needs_same_direction() {
        assert!(is_saturated_pair((5, 0), (4, 1)));
        assert!(is_saturated_pair((1, -5), (-1, -5))); // circle: X turns, Y carries on
        assert!(!is_saturated_pair((-5, 0), (4, 1))); // reversal inside one poll
        assert!(!is_saturated_pair((-3, -3), (3, -3))); // right-angle corner
        assert!(!is_saturated_pair((1, 0), (1, 0)));
        assert!(!is_saturated_pair((i32::MAX, 0), (1, 0)));
    }

    #[test]
    fn test_fast_reversal_at_125hz_is_not_a_poll_interval() {
        // Moving fast, the hand turns around within one 8 ms poll: the counts
        // cancel and that poll is skipped although both neighbours are large.
        let reports = vec![
            (0.0, -13, 0),
            (8.0, -9, 1),
            (16.0, -5, 0),
            (32.0, 4, 1),
            (40.0, 9, 0),
            (48.0, 13, 0),
        ];
        assert_eq!(saturated_intervals_ms(&reports), vec![8.0, 8.0, 8.0, 8.0]);
    }

    #[test]
    fn test_saturated_intervals_skip_slow_reversal_gaps() {
        // 1 kHz: fast pass, slow down (1-count reports with growing gaps),
        // turn around, speed up again. Only the fast intervals are kept.
        let mut reports = Vec::new();
        let mut t = 0.0;
        for _ in 0..20 {
            t += 1.0;
            reports.push((t, 3, 0));
        }
        for gap in [2.0, 4.0, 7.0, 12.0, 7.0, 4.0, 2.0] {
            t += gap;
            reports.push((t, -1, 0));
        }
        for _ in 0..20 {
            t += 1.0;
            reports.push((t, -3, 0));
        }
        let intervals = saturated_intervals_ms(&reports);
        assert_eq!(intervals.len(), 38);
        assert!(intervals.iter().all(|&i| (i - 1.0).abs() < 1e-9));
        assert_eq!(estimate_hz(&intervals, 10), Some(1000));
        // The naive estimate over every interval is dragged down
        let all: Vec<f64> = reports.windows(2).map(|w| w[1].0 - w[0].0).collect();
        let naive_mean = all.iter().sum::<f64>() / all.len() as f64;
        assert!(1000.0 / naive_mean < 800.0);
    }

    #[test]
    fn test_saturated_intervals_drop_pauses_and_zero_moves() {
        let reports = vec![
            (0.0, 3, 0),
            (1.0, 3, 0),
            (1.5, 0, 0), // no motion: ignored
            (2.0, 3, 0),
            (900.0, 3, 0), // hand stopped for most of a second
            (901.0, 3, 0),
            (901.0, 3, 0), // duplicate timestamp
        ];
        assert_eq!(saturated_intervals_ms(&reports), vec![1.0, 1.0, 1.0]);
        assert!(saturated_intervals_ms(&[]).is_empty());
    }

    #[test]
    fn test_usb_nominal_rate_is_not_named_for_bluetooth_rates() {
        assert_eq!(usb_nominal_rate(89.0), None);
        assert_eq!(usb_nominal_rate(109.9), None);
        assert_eq!(usb_nominal_rate(110.0), Some(125));
        assert_eq!(usb_nominal_rate(986.0), Some(1000));
    }

    #[test]
    fn test_window_hz_ignores_slow_movement() {
        // 1 kHz mouse moved slowly: a 1-count report every 3-8 ms
        let slow: Vec<(f64, i32, i32)> = (0..200)
            .map(|i| (i as f64 * (3.0 + (i % 6) as f64), 1, 0))
            .collect();
        assert_eq!(window_hz(&slow), None);
        // ... and briskly: a report every poll
        let fast: Vec<(f64, i32, i32)> = (0..200).map(|i| (i as f64, 3, 1)).collect();
        assert_eq!(window_hz(&fast), Some(1000));
    }

    #[test]
    fn test_nominal_rate_snaps_to_standard_values() {
        assert_eq!(nominal_rate(986.0), 1000);
        assert_eq!(nominal_rate(131.0), 125);
        assert_eq!(nominal_rate(470.0), 500);
        assert_eq!(nominal_rate(7900.0), 8000);
        assert_eq!(nominal_rate(240.0), 250);
    }
}
