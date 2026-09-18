//! Polling rate statistics
//!
//! Tracks min/max/avg polling rate from a stream of Hz samples, and estimates
//! the polling rate from inter-report intervals.

/// Standard USB/wireless mouse report rates, used to label a measurement.
pub const NOMINAL_RATES_HZ: [u32; 8] = [125, 250, 500, 1000, 2000, 4000, 8000, 16000];

/// Intervals longer than this are treated as the user pausing, not as a poll.
pub const MAX_POLL_INTERVAL_MS: f64 = 100.0;

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
    fn test_nominal_rate_snaps_to_standard_values() {
        assert_eq!(nominal_rate(986.0), 1000);
        assert_eq!(nominal_rate(131.0), 125);
        assert_eq!(nominal_rate(470.0), 500);
        assert_eq!(nominal_rate(7900.0), 8000);
        assert_eq!(nominal_rate(240.0), 250);
    }
}
