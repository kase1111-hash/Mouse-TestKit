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
//!
//! A Bluetooth mouse can deliver two or more reports at once, one connection
//! event apart (see [`batched_delivery`]); its rate and steadiness are then
//! judged per delivery, not per report.

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
/// set to a low rate. No USB rate is named for such a reading, nor for one
/// well above a USB rate, such as 133 Hz (see [`usb_nominal_rate`]).
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
/// moved too slowly to show the rate. Batched delivery is measured per
/// delivery (see [`rate_hz`]).
pub fn window_hz(reports: &[(f64, i32, i32)]) -> Option<u32> {
    rate_hz(&saturated_intervals_ms(reports), MIN_FAST_INTERVALS)
}

/// Reports that reach the computer this close together (ms) arrived in one
/// delivery. A Bluetooth mouse is served once per connection event; when it
/// has several reports ready it sends them all in that event, back to back,
/// and they arrive well within a millisecond or so of each other.
pub const BATCH_GAP_MS: f64 = 1.5;

/// A whole delivery (first to last report) spans at most this long (ms).
/// Two to four back-to-back Bluetooth packets take about 0.3-0.5 ms each.
/// A 1 kHz USB mouse cannot deliver several reports this close together,
/// since they come one poll (1 ms) apart.
const MAX_BATCH_SPAN_MS: f64 = 2.0;

/// Deliveries at least this far apart (ms) can be Bluetooth connection
/// events: the shortest connection interval Bluetooth LE allows is 7.5 ms
/// (classic Bluetooth mice use similar 10-15 ms intervals).
pub const MIN_EVENT_PERIOD_MS: f64 = 5.0;

/// Delivery is batched when at least this share of fast-movement intervals
/// fall inside a delivery (two reports in every event are half of them; a
/// mouse that makes reports a little faster than its connection events
/// sends two in one event of every few, a tenth or so)...
const MIN_BATCHED_SHARE: f64 = 0.05;

/// ... and at most half the deliveries of several reports come this many
/// delivery periods or more after the one before. A connection event comes
/// on schedule and carries the reports made since the last one. Reports
/// held up on the way (a USB mouse's report reaching the program almost a
/// poll late, with the next one right behind it) arrive together after a
/// gap: a catch-up, the instability batching must not excuse.
const CATCH_UP_PERIODS: f64 = 1.5;

/// ... and a delivery that comes on schedule carries at most this many
/// reports on average. A Bluetooth mouse samples at about 125-250 Hz, so
/// even a 15 ms connection event holds no more than about four reports
/// (one after missed events also carries theirs, so it is left out). A USB
/// mouse whose reports stall now and then sends dozens at its poll rate
/// between the stalls.
const MAX_REPORTS_PER_EVENT: f64 = 4.0;

/// Fewest fast-movement intervals batching is judged on.
const MIN_BATCH_INTERVALS: usize = 20;

/// Reports delivered several at a time, one delivery event apart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Batching {
    /// Median time (ms) from one delivery to the next.
    pub event_period_ms: f64,
    /// Average reports per delivery that came on schedule (one per
    /// connection event).
    pub reports_per_event: f64,
}

impl Batching {
    /// Deliveries per second.
    pub fn event_hz(&self) -> f64 {
        1000.0 / self.event_period_ms
    }
}

/// Intervals (ms) between deliveries: every interval longer than
/// [`BATCH_GAP_MS`] ends a delivery, and the near-zero intervals inside the
/// delivery before it are added to it, so each value runs from the first
/// report of one delivery to the first report of the next. `intervals_ms`
/// must be in arrival order.
pub fn event_intervals_ms(intervals_ms: &[f64]) -> Vec<f64> {
    let mut out = Vec::new();
    let mut within = 0.0;
    for &i in intervals_ms {
        within += i;
        if i > BATCH_GAP_MS {
            out.push(within);
            within = 0.0;
        }
    }
    out
}

/// Deliveries after the first (see [`event_intervals_ms`]): time (ms) from
/// the first report of the delivery before, reports in it, and time (ms)
/// from its first report to its last.
fn deliveries(intervals_ms: &[f64]) -> Vec<(f64, usize, f64)> {
    let mut out: Vec<(f64, usize, f64)> = Vec::new();
    let mut first_span = 0.0;
    for &i in intervals_ms {
        if i > BATCH_GAP_MS {
            let span = out.last().map_or(first_span, |d| d.2);
            out.push((span + i, 1, 0.0));
        } else if let Some(d) = out.last_mut() {
            d.1 += 1;
            d.2 += i;
        } else {
            first_span += i;
        }
    }
    out
}

/// Whether fast-movement intervals (ms, in arrival order) show reports
/// delivered several at a time, as a Bluetooth mouse does when it has more
/// than one report ready per connection event: near-zero intervals
/// ([`BATCH_GAP_MS`]) among intervals of the event period (at least
/// [`MIN_EVENT_PERIOD_MS`]), few reports per delivery, each delivery
/// arriving within [`MAX_BATCH_SPAN_MS`], and the deliveries of several
/// reports coming on schedule, not after a gap ([`CATCH_UP_PERIODS`]).
///
/// Judged per report, such a mouse looks unsteady (up to half its intervals
/// near zero, the rest 7.5-15 ms) and its median interval can be
/// meaningless; per delivery it is as steady as the link. A USB mouse, even
/// at 4-8 kHz, sends one report per poll, never several within a fraction of
/// a poll and then nothing for 5 ms; reports of a 125 Hz mouse that reach
/// the program together came after a gap. Its instability is not excused as
/// batching.
pub fn batched_delivery(intervals_ms: &[f64]) -> Option<Batching> {
    if intervals_ms.len() < MIN_BATCH_INTERVALS {
        return None;
    }
    let within = intervals_ms.iter().filter(|&&i| i <= BATCH_GAP_MS).count();
    let between = intervals_ms.len() - within;
    if between == 0 || (within as f64) < MIN_BATCHED_SHARE * intervals_ms.len() as f64 {
        return None;
    }
    let deliveries = deliveries(intervals_ms);
    let mut periods: Vec<f64> = deliveries.iter().map(|d| d.0).collect();
    let event_period_ms = median_in_place(&mut periods)?;
    // A delivery after missed connection events also carries their reports
    let on_schedule: Vec<usize> = deliveries
        .iter()
        .filter(|d| d.0 < CATCH_UP_PERIODS * event_period_ms)
        .map(|d| d.1)
        .collect();
    let reports_per_event = on_schedule.iter().sum::<usize>() as f64 / on_schedule.len() as f64;
    if reports_per_event > MAX_REPORTS_PER_EVENT {
        return None;
    }
    let batches: Vec<(f64, f64)> = deliveries
        .iter()
        .filter(|d| d.1 >= 2)
        .map(|d| (d.0, d.2))
        .collect();
    let mut spans: Vec<f64> = batches.iter().map(|b| b.1).collect();
    if median_in_place(&mut spans)? > MAX_BATCH_SPAN_MS {
        return None;
    }
    let catch_ups = batches
        .iter()
        .filter(|b| b.0 >= CATCH_UP_PERIODS * event_period_ms)
        .count();
    (event_period_ms >= MIN_EVENT_PERIOD_MS && 2 * catch_ups <= batches.len()).then_some(Batching {
        event_period_ms,
        reports_per_event,
    })
}

/// Polling rate (Hz) from fast-movement intervals (ms, in arrival order), as
/// [`estimate_hz`]; with batched delivery (see [`batched_delivery`]) the
/// rate of deliveries, which is how often the cursor can move.
pub fn rate_hz(intervals_ms: &[f64], min_samples: usize) -> Option<u32> {
    if intervals_ms.len() < min_samples.max(1) {
        return None;
    }
    match batched_delivery(intervals_ms) {
        Some(b) => Some(b.event_hz().round() as u32),
        None => estimate_hz(intervals_ms, min_samples),
    }
}

/// How far above a standard rate (as a share of it) a measurement may be
/// and still be named that rate. The host polls a USB mouse on its own
/// frame clock (1 ms, or 125 us at high speed), so a missed or late report
/// can lengthen an interval but none is shorter than the poll; the median
/// keeps timestamp noise to a percent or two. A 7.5 ms Bluetooth
/// connection interval reads 133 Hz, 6.7% above 125.
const MAX_ABOVE_NOMINAL: f64 = 0.04;

/// The standard USB rate nearest a measurement, or `None` below
/// [`USB_MIN_POLLING_HZ`] (there "nominal 125 Hz" would read as a USB mouse
/// running slow, while the rate is normal for Bluetooth) and when it is
/// well above that rate ([`MAX_ABOVE_NOMINAL`]), which no USB mouse is.
pub fn usb_nominal_rate(hz: f64) -> Option<u32> {
    let nominal = nominal_rate(hz);
    (hz >= USB_MIN_POLLING_HZ as f64 && hz <= nominal as f64 * (1.0 + MAX_ABOVE_NOMINAL))
        .then_some(nominal)
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
        // A USB rate is never measured well above itself: 133 Hz is a
        // Bluetooth mouse on a 7.5 ms connection interval
        assert_eq!(usb_nominal_rate(133.0), None);
        assert_eq!(usb_nominal_rate(150.0), None);
        assert_eq!(usb_nominal_rate(128.0), Some(125));
        assert_eq!(usb_nominal_rate(1030.0), Some(1000));
        assert_eq!(usb_nominal_rate(8150.0), Some(8000));
        assert_eq!(usb_nominal_rate(7700.0), Some(8000));
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

    /// Small deterministic generator in [0, 1).
    struct Lcg(u64);
    impl Lcg {
        fn unit(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    /// Arrival times (ms) of a Bluetooth mouse sampling at `sample_hz`,
    /// served every `event_ms`: each connection event delivers the reports
    /// made since the last one, 0.3-0.6 ms apart. `skip(k)` drops the k-th
    /// event (its reports go out in the next one).
    fn bluetooth_arrivals(
        secs: f64,
        sample_hz: f64,
        event_ms: f64,
        skip: impl Fn(usize) -> bool,
    ) -> Vec<f64> {
        let mut rng = Lcg(7);
        let mut out = Vec::new();
        let mut sent = 0usize;
        for k in 1..(secs * 1000.0 / event_ms) as usize {
            let t = k as f64 * event_ms;
            let ready = (t * sample_hz / 1000.0).floor() as usize;
            if skip(k) {
                continue;
            }
            let mut at = t + 0.2 * rng.unit();
            for _ in sent..ready {
                out.push(at);
                at += 0.3 + 0.3 * rng.unit();
            }
            sent = ready;
        }
        out
    }

    fn intervals(times: &[f64]) -> Vec<f64> {
        times.windows(2).map(|w| w[1] - w[0]).collect()
    }

    #[test]
    fn test_bluetooth_batches_are_judged_per_delivery() {
        // 250 Hz sampling over 7.5, 11.25 and 15 ms connection intervals:
        // two to four reports per event
        for (event_ms, per_event) in [(7.5, 1.875), (11.25, 2.8125), (15.0, 3.75)] {
            let i = intervals(&bluetooth_arrivals(3.0, 250.0, event_ms, |_| false));
            let b = batched_delivery(&i).expect("batched");
            assert!((b.event_period_ms - event_ms).abs() < 0.3, "{:?}", b);
            assert!((b.reports_per_event - per_event).abs() < 0.2, "{:?}", b);
            // The median report interval is a gap inside a delivery or one
            // shortened by it, not the rate the cursor moves at
            let naive = estimate_hz(&i, 10).unwrap();
            let per_delivery = rate_hz(&i, 10).unwrap();
            assert!((per_delivery as f64 - 1000.0 / event_ms).abs() < 3.0);
            assert!(
                naive.abs_diff(per_delivery) > 5,
                "{} {}",
                naive,
                per_delivery
            );
            let window: Vec<(f64, i32, i32)> = bluetooth_arrivals(1.0, 250.0, event_ms, |_| false)
                .into_iter()
                .map(|t| (t, 6, 2))
                .collect();
            assert_eq!(window_hz(&window), Some(per_delivery));
        }
        // Sampling a little faster than the connection events: two reports
        // in one event of every four to eight. Per report, a tenth or more
        // of the intervals are near zero.
        for (sample_hz, event_ms) in [(100.0, 11.25), (150.0, 7.5), (80.0, 15.0)] {
            let i = intervals(&bluetooth_arrivals(3.0, sample_hz, event_ms, |_| false));
            let b = batched_delivery(&i).expect("batched");
            assert!((b.event_period_ms - event_ms).abs() < 0.3, "{:?}", b);
            assert!(b.reports_per_event < 1.3, "{:?}", b);
        }
        // 133 Hz sampling over a 7.5 ms interval: one report per event,
        // nothing to fold
        let i = intervals(&bluetooth_arrivals(3.0, 133.0, 7.5, |_| false));
        assert_eq!(batched_delivery(&i), None);
        assert_eq!(rate_hz(&i, 10), Some(133));
        // Events skipped by interference show up between deliveries
        let i = intervals(&bluetooth_arrivals(3.0, 250.0, 11.25, |k| k % 10 == 0));
        let events = event_intervals_ms(&i);
        assert!(events.iter().filter(|&&e| e > 20.0).count() >= 20);
        assert!(batched_delivery(&i).is_some());
        // Four reports per 15 ms event, with 5-10% of the events missed:
        // their reports arrive with the next event, which then holds seven
        // or eight. Per connection event it is still under four.
        for every in [20, 12, 10] {
            let i = intervals(&bluetooth_arrivals(10.0, 250.0, 15.0, |k| k % every == 0));
            let b = batched_delivery(&i).expect("batched");
            assert!((b.event_period_ms - 15.0).abs() < 0.3, "{} {:?}", every, b);
            assert!(
                (b.reports_per_event - 3.75).abs() < 0.2,
                "{} {:?}",
                every,
                b
            );
            assert_eq!(rate_hz(&i, 10), Some(67), "{}", every);
        }
    }

    #[test]
    fn test_usb_mice_are_never_batched() {
        let mut rng = Lcg(3);
        for hz in [125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0] {
            let period = 1000.0 / hz;
            // Healthy, with host timestamp noise of up to 0.3 ms
            let noisy: Vec<f64> = (0..4000)
                .map(|k| k as f64 * period + 0.3 * rng.unit())
                .collect();
            assert_eq!(batched_delivery(&intervals(&noisy)), None, "{} Hz", hz);
            // Unsteady: every 5th report 0.3-0.9 polls late, the next on time
            let unsteady: Vec<f64> = (0..4000)
                .map(|k| {
                    let late = if k % 5 == 0 {
                        period * (0.3 + 0.6 * rng.unit())
                    } else {
                        0.0
                    };
                    k as f64 * period + late
                })
                .collect();
            let i = intervals(&unsteady);
            assert_eq!(batched_delivery(&i), None, "{} Hz", hz);
            assert_eq!(rate_hz(&i, 10), estimate_hz(&i, 10));
            // Stalling for 5-12 ms about every 30 reports
            let mut t = 0.0;
            let stalls: Vec<f64> = (0..4000)
                .map(|_| {
                    t += if rng.unit() < 1.0 / 30.0 {
                        5.0 + 7.0 * rng.unit()
                    } else {
                        period
                    };
                    t
                })
                .collect();
            assert_eq!(batched_delivery(&intervals(&stalls)), None, "{} Hz", hz);
        }
        // A 1 kHz mouse whose every third report reaches the program late,
        // together with the next (a busy Windows message loop): its
        // "deliveries" are 3 ms apart, far too often for Bluetooth
        let paired: Vec<f64> = (0..3000)
            .map(|k| k as f64 + if k % 3 == 1 { 0.95 } else { 0.0 })
            .collect();
        assert_eq!(batched_delivery(&intervals(&paired)), None);
        // A 125 Hz mouse, 5-45% of whose reports reach the program almost a
        // poll late, right before the next: those pairs come after a gap
        for share in [0.05, 0.1, 0.2, 0.3, 0.45] {
            let late: Vec<f64> = (0..3000)
                .map(|k| {
                    let late = if rng.unit() < share {
                        6.6 + 1.3 * rng.unit()
                    } else {
                        0.0
                    };
                    k as f64 * 8.0 + late
                })
                .collect();
            assert_eq!(batched_delivery(&intervals(&late)), None, "{}", share);
        }
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
