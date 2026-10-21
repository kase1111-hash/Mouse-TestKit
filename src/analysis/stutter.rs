//! Stutter analysis
//!
//! A stutter is a motion report that arrives late while the mouse should have
//! been reporting at every poll. A mouse only sends a report once a whole
//! count has accumulated, so slow movement, direction reversals and pauses
//! legitimately produce gaps of 2-20 ms (or seconds). Those gaps are not
//! stutters and must not be judged; [`StutterDetector`] only judges intervals
//! between reports that both prove the hand was moving fast enough, in a
//! steady direction, for a report at every poll (see
//! [`crate::analysis::polling::is_saturated_pair`]),
//! compares them against the median of such intervals, and counts each
//! interval exactly once.
//!
//! [`analyze_stutter`] is the older deviation-from-mean classifier for a plain
//! list of intervals. It knows nothing about pauses or slow movement and is
//! only meaningful for intervals that were already filtered that way.

use std::collections::VecDeque;

use crate::analysis::polling::{is_saturated_pair, median_in_place, MAX_POLL_INTERVAL_MS};

/// Default stutter threshold as a multiple of the median report interval.
/// A single missed poll doubles the interval; 2.5x needs at least two in a
/// row (the same multiplier the Auto Diagnostics movement check uses).
pub const DEFAULT_STUTTER_MULTIPLIER: f64 = 2.5;

/// An interval is never a stutter unless it is at least this long (ms).
///
/// At 2-8 kHz the median interval is 0.125-0.5 ms, so a pure multiple of it
/// would flag ordinary OS interrupt and thread-scheduling delays of a few
/// hundred microseconds. 2.5 ms is still only two missed reports in a row at
/// 1 kHz (about twenty at 8 kHz), so a real dropout is not missed.
pub const MIN_STUTTER_GAP_MS: f64 = 2.5;

/// Judged intervals collected before the median baseline is trusted.
/// Intervals seen before that are held back and judged once it exists.
pub const BASELINE_MIN_INTERVALS: usize = 50;

/// Seconds of judged fast movement needed before a rating is given. At 8 kHz
/// a few hundred intervals are only tens of milliseconds, far too short to
/// say anything about occasional stutters, so the minimum is in time.
pub const MIN_JUDGED_SECS: f64 = 3.0;

/// This many stutters are a clear fault even before [`MIN_JUDGED_SECS`] of
/// movement is collected: only fast, steady movement is judged, and there a
/// healthy mouse on a healthy PC reports at every poll.
const EARLY_VERDICT_STUTTERS: usize = 5;

/// The baseline median is taken over the most recent this-many intervals...
const BASELINE_WINDOW: usize = 1000;
/// ...and recomputed after this many new ones.
const BASELINE_REFRESH: usize = 100;
/// Detected events kept for display; totals are counted separately.
const MAX_KEPT_EVENTS: usize = 1000;

/// Lateness (ms beyond the median interval) from which a stutter is
/// Moderate: about one frame at 240 Hz.
const MODERATE_LATE_MS: f64 = 4.0;
/// Lateness from which a stutter is Severe: about one frame at 120 Hz, half
/// a frame at 60 Hz, which shows up as a visible hitch.
const SEVERE_LATE_MS: f64 = 8.0;

/// Stutters per second of judged movement above which the rating is Fair,
/// and Poor. An isolated late report every few seconds is common on a
/// healthy PC (background load, wireless retransmits) and is not visible.
const FAIR_STUTTERS_PER_SEC: f64 = 0.5;
const POOR_STUTTERS_PER_SEC: f64 = 2.0;

/// The interval (ms) above which a judged interval is a stutter.
pub fn stutter_threshold_ms(median_ms: f64, multiplier: f64) -> f64 {
    (median_ms * multiplier).max(MIN_STUTTER_GAP_MS)
}

fn classify_lateness(late_ms: f64) -> StutterSeverity {
    if late_ms >= SEVERE_LATE_MS {
        StutterSeverity::Severe
    } else if late_ms >= MODERATE_LATE_MS {
        StutterSeverity::Moderate
    } else {
        StutterSeverity::Minor
    }
}

/// Overall verdict for a stutter session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StutterRating {
    /// Not enough fast, continuous movement was recorded to judge.
    Inconclusive,
    Excellent,
    Good,
    Fair,
    Poor,
}

impl StutterRating {
    pub fn label(self) -> &'static str {
        match self {
            StutterRating::Inconclusive => "Inconclusive",
            StutterRating::Excellent => "Excellent",
            StutterRating::Good => "Good",
            StutterRating::Fair => "Fair",
            StutterRating::Poor => "Poor",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            StutterRating::Inconclusive => {
                "Not enough fast movement yet. Keep moving the mouse quickly and continuously (circles work best)."
            }
            StutterRating::Excellent => "No late reports during fast movement",
            StutterRating::Good => {
                "Isolated late reports only - usually the PC or wireless link, not visible in use"
            }
            StutterRating::Fair => "Repeated late reports - may be visible as occasional hitches",
            StutterRating::Poor => {
                "Frequent late reports - check the USB port/hub, wireless interference, drivers and system load"
            }
        }
    }
}

/// Rate a session from the seconds of fast movement that were judged and the
/// stutters found in them.
pub fn rate_stutters(judged_secs: f64, stutters: usize) -> StutterRating {
    if judged_secs < MIN_JUDGED_SECS && stutters < EARLY_VERDICT_STUTTERS {
        return StutterRating::Inconclusive;
    }
    // A short run is rated as if it were MIN_JUDGED_SECS long so a burst of
    // early stutters is not exaggerated into a huge per-second rate.
    let per_sec = stutters as f64 / judged_secs.max(MIN_JUDGED_SECS);
    if stutters == 0 {
        StutterRating::Excellent
    } else if per_sec < FAIR_STUTTERS_PER_SEC {
        StutterRating::Good
    } else if per_sec < POOR_STUTTERS_PER_SEC {
        StutterRating::Fair
    } else {
        StutterRating::Poor
    }
}

/// Streaming stutter detector fed with one motion report at a time.
///
/// Each interval between two consecutive motion reports is considered once:
/// it is *judged* only when the two reports form a saturated pair (fast
/// motion in a steady direction) and it is no longer than [`MAX_POLL_INTERVAL_MS`] (longer gaps are the hand stopping or
/// lifting). A judged interval longer than
/// [`stutter_threshold_ms`]`(median, multiplier)` is a stutter.
#[derive(Debug, Clone)]
pub struct StutterDetector {
    multiplier: f64,
    /// Previous motion report: (time in seconds, dx, dy).
    last: Option<(f64, i32, i32)>,
    reports: usize,
    /// Most recent judged-candidate intervals (ms) for the baseline median.
    recent: VecDeque<f64>,
    median_ms: Option<f64>,
    since_refresh: usize,
    /// Intervals seen before the baseline existed: (report index, ms).
    pending: Vec<(usize, f64)>,
    judged: usize,
    judged_sum_ms: f64,
    judged_min_ms: f64,
    judged_max_ms: f64,
    stutter_count: usize,
    minor: usize,
    moderate: usize,
    severe: usize,
    worst_gap_ms: f64,
    events: VecDeque<StutterEvent>,
}

impl StutterDetector {
    pub fn new(multiplier: f64) -> Self {
        Self {
            multiplier,
            last: None,
            reports: 0,
            recent: VecDeque::with_capacity(BASELINE_WINDOW),
            median_ms: None,
            since_refresh: 0,
            pending: Vec::new(),
            judged: 0,
            judged_sum_ms: 0.0,
            judged_min_ms: f64::MAX,
            judged_max_ms: 0.0,
            stutter_count: 0,
            minor: 0,
            moderate: 0,
            severe: 0,
            worst_gap_ms: 0.0,
            events: VecDeque::new(),
        }
    }

    /// Change the threshold multiplier. Intervals already judged keep their verdict.
    pub fn set_multiplier(&mut self, multiplier: f64) {
        self.multiplier = multiplier;
    }

    /// Feed one motion report: time in seconds (any fixed origin) and its
    /// counts. Reports must arrive in time order.
    ///
    /// Returns the interval (ms) since the previous report when it is going
    /// to be judged (now, or once the baseline exists), `None` otherwise.
    pub fn feed(&mut self, t: f64, dx: i32, dy: i32) -> Option<f64> {
        if dx == 0 && dy == 0 {
            return None;
        }
        let index = self.reports;
        self.reports += 1;
        let (prev_t, pdx, pdy) = self.last.replace((t, dx, dy))?;
        let interval = (t - prev_t) * 1000.0;
        if !is_saturated_pair((pdx, pdy), (dx, dy))
            || interval <= 0.0
            || interval > MAX_POLL_INTERVAL_MS
        {
            return None;
        }

        self.recent.push_back(interval);
        if self.recent.len() > BASELINE_WINDOW {
            self.recent.pop_front();
        }
        self.since_refresh += 1;

        if self.median_ms.is_none() {
            self.pending.push((index, interval));
            if self.pending.len() >= BASELINE_MIN_INTERVALS {
                self.refresh_baseline();
                for (i, ms) in std::mem::take(&mut self.pending) {
                    self.judge(i, ms);
                }
            }
        } else {
            if self.since_refresh >= BASELINE_REFRESH {
                self.refresh_baseline();
            }
            self.judge(index, interval);
        }
        Some(interval)
    }

    fn refresh_baseline(&mut self) {
        let mut values: Vec<f64> = self.recent.iter().copied().collect();
        self.median_ms = median_in_place(&mut values);
        self.since_refresh = 0;
    }

    fn judge(&mut self, index: usize, interval: f64) {
        let Some(median) = self.median_ms else {
            return;
        };
        self.judged += 1;
        self.judged_sum_ms += interval;
        self.judged_min_ms = self.judged_min_ms.min(interval);
        self.judged_max_ms = self.judged_max_ms.max(interval);
        if interval <= stutter_threshold_ms(median, self.multiplier) {
            return;
        }
        let late = interval - median;
        let severity = classify_lateness(late);
        match severity {
            StutterSeverity::Minor => self.minor += 1,
            StutterSeverity::Moderate => self.moderate += 1,
            StutterSeverity::Severe => self.severe += 1,
        }
        self.stutter_count += 1;
        self.worst_gap_ms = self.worst_gap_ms.max(interval);
        self.events.push_back(StutterEvent {
            index,
            delta_ms: interval,
            deviation_ms: late,
            severity,
        });
        if self.events.len() > MAX_KEPT_EVENTS {
            self.events.pop_front();
        }
    }

    /// Motion reports fed so far.
    pub fn reports(&self) -> usize {
        self.reports
    }

    /// Intervals compared against the baseline so far.
    pub fn judged(&self) -> usize {
        self.judged
    }

    /// Seconds of fast movement covered by the judged intervals.
    pub fn judged_secs(&self) -> f64 {
        self.judged_sum_ms / 1000.0
    }

    /// Saturated intervals collected but not judged yet (no baseline yet).
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Median interval (ms) of fast movement, once known.
    pub fn median_interval_ms(&self) -> Option<f64> {
        self.median_ms
    }

    /// Polling rate (Hz) implied by the median interval of fast movement.
    pub fn polling_hz(&self) -> Option<f64> {
        self.median_ms.filter(|&m| m > 0.0).map(|m| 1000.0 / m)
    }

    /// Current stutter threshold (ms), once the baseline is known.
    pub fn threshold_ms(&self) -> Option<f64> {
        self.median_ms
            .map(|m| stutter_threshold_ms(m, self.multiplier))
    }

    /// Mean / shortest / longest judged interval (ms); `None` before any was judged.
    pub fn judged_mean_ms(&self) -> Option<f64> {
        (self.judged > 0).then(|| self.judged_sum_ms / self.judged as f64)
    }

    pub fn judged_min_ms(&self) -> Option<f64> {
        (self.judged > 0).then_some(self.judged_min_ms)
    }

    pub fn judged_max_ms(&self) -> Option<f64> {
        (self.judged > 0).then_some(self.judged_max_ms)
    }

    /// Total stutters detected.
    pub fn stutter_count(&self) -> usize {
        self.stutter_count
    }

    /// Stutters as a percentage of judged intervals.
    pub fn stutter_rate_percent(&self) -> f64 {
        if self.judged == 0 {
            0.0
        } else {
            self.stutter_count as f64 / self.judged as f64 * 100.0
        }
    }

    /// (minor, moderate, severe) stutter counts.
    pub fn severity_counts(&self) -> (usize, usize, usize) {
        (self.minor, self.moderate, self.severe)
    }

    /// Longest stuttering interval (ms), 0 when there was none.
    pub fn worst_gap_ms(&self) -> f64 {
        self.worst_gap_ms
    }

    /// The most recent detected stutters (at most a fixed number are kept).
    pub fn recent_stutters(&self) -> impl Iterator<Item = &StutterEvent> {
        self.events.iter()
    }

    pub fn rating(&self) -> StutterRating {
        rate_stutters(self.judged_secs(), self.stutter_count)
    }
}

impl Default for StutterDetector {
    fn default() -> Self {
        Self::new(DEFAULT_STUTTER_MULTIPLIER)
    }
}

/// Analyze a slice of delta times (ms) and return detected stutter events.
///
/// A stutter is any delta that deviates from the mean by more than 2 ms.
/// Severity is classified as Minor (>2ms), Moderate (>4ms), or Severe (>8ms).
///
/// The mean is dragged up by any pause or slow-movement gap in the input, so
/// raw report intervals must not be passed here; use [`StutterDetector`].
pub fn analyze_stutter(deltas: &[f64]) -> Vec<StutterEvent> {
    if deltas.len() < 2 {
        return Vec::new();
    }
    let avg: f64 = deltas.iter().sum::<f64>() / deltas.len() as f64;
    let mut events = Vec::new();

    for (i, delta) in deltas.iter().enumerate() {
        let deviation = (*delta - avg).abs();
        let severity = if deviation > 8.0 {
            Some(StutterSeverity::Severe)
        } else if deviation > 4.0 {
            Some(StutterSeverity::Moderate)
        } else if deviation > 2.0 {
            Some(StutterSeverity::Minor)
        } else {
            None
        };

        if let Some(sev) = severity {
            events.push(StutterEvent {
                index: i,
                delta_ms: *delta,
                deviation_ms: deviation,
                severity: sev,
            });
        }
    }
    events
}

/// A single detected stutter event.
///
/// For [`StutterDetector`], `index` is the motion report that ended the
/// interval, `delta_ms` the interval and `deviation_ms` how much later than
/// the median interval it arrived.
#[derive(Clone, Debug, PartialEq)]
pub struct StutterEvent {
    #[allow(dead_code)]
    pub index: usize,
    #[allow(dead_code)]
    pub delta_ms: f64,
    #[allow(dead_code)]
    pub deviation_ms: f64,
    pub severity: StutterSeverity,
}

/// Severity classification for a stutter event.
#[derive(Clone, Debug, PartialEq)]
pub enum StutterSeverity {
    Minor,
    Moderate,
    Severe,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_stutter_empty_input() {
        let result = analyze_stutter(&[]);
        assert!(result.is_empty(), "Empty input should return empty result");
    }

    #[test]
    fn test_analyze_stutter_single_element() {
        let result = analyze_stutter(&[5.0]);
        assert!(
            result.is_empty(),
            "Single element should return empty result"
        );
    }

    #[test]
    fn test_analyze_stutter_uniform_deltas() {
        // Uniform deltas with no deviation should produce no stutter events
        let deltas = vec![2.0, 2.0, 2.0, 2.0, 2.0];
        let result = analyze_stutter(&deltas);
        assert!(
            result.is_empty(),
            "Uniform deltas should produce no stutter events"
        );
    }

    #[test]
    fn test_analyze_stutter_minor_deviation() {
        // Need deviation > 2.0 but <= 4.0 for Minor
        // deltas = [5.0, 5.0, 5.0, 5.0, 8.0], avg = 5.6
        // deviation of 8.0 from 5.6 = 2.4 (minor)
        let deltas = vec![5.0, 5.0, 5.0, 5.0, 8.0];
        let result = analyze_stutter(&deltas);
        let minor_count = result
            .iter()
            .filter(|e| e.severity == StutterSeverity::Minor)
            .count();
        assert!(
            minor_count > 0,
            "Should detect minor stutter for small deviations"
        );
    }

    #[test]
    fn test_analyze_stutter_moderate_deviation() {
        // Create a delta with 4-8ms deviation from average
        let deltas = vec![5.0, 5.0, 5.0, 5.0, 11.0]; // avg=6.2, deviation of 11-6.2=4.8
        let result = analyze_stutter(&deltas);
        let moderate_count = result
            .iter()
            .filter(|e| e.severity == StutterSeverity::Moderate)
            .count();
        assert!(
            moderate_count > 0,
            "Should detect moderate stutter for 4-8ms deviations"
        );
    }

    #[test]
    fn test_analyze_stutter_severe_deviation() {
        // Create a delta with >8ms deviation from average
        let deltas = vec![2.0, 2.0, 2.0, 2.0, 15.0]; // avg=4.6, deviation of 15-4.6=10.4
        let result = analyze_stutter(&deltas);
        let severe_count = result
            .iter()
            .filter(|e| e.severity == StutterSeverity::Severe)
            .count();
        assert!(
            severe_count > 0,
            "Should detect severe stutter for >8ms deviations"
        );
    }

    #[test]
    fn test_analyze_stutter_event_fields() {
        let deltas = vec![2.0, 2.0, 2.0, 20.0]; // Clear outlier
        let result = analyze_stutter(&deltas);

        assert!(
            !result.is_empty(),
            "Should detect at least one stutter event"
        );
        let event = &result[result.len() - 1]; // The outlier should be last
        assert_eq!(
            event.index, 3,
            "Event index should match the outlier position"
        );
        assert!(
            (event.delta_ms - 20.0).abs() < 0.001,
            "Delta should match input value"
        );
        assert!(event.deviation_ms > 8.0, "Deviation should be significant");
    }

    #[test]
    fn test_stutter_severity_thresholds() {
        // Test threshold boundaries
        // Minor: deviation > 2.0 and <= 4.0
        // Moderate: deviation > 4.0 and <= 8.0
        // Severe: deviation > 8.0

        // For minor: avg = (10*4 + 13) / 5 = 10.6, deviation of 13 = 2.4 (minor)
        let deltas_minor = vec![10.0, 10.0, 10.0, 10.0, 13.0];
        // For moderate: avg = (10*4 + 16) / 5 = 11.2, deviation of 16 = 4.8 (moderate)
        let deltas_moderate = vec![10.0, 10.0, 10.0, 10.0, 16.0];
        // For severe: avg = (10*4 + 22) / 5 = 12.4, deviation of 22 = 9.6 (severe)
        let deltas_severe = vec![10.0, 10.0, 10.0, 10.0, 22.0];

        let result_minor = analyze_stutter(&deltas_minor);
        let result_moderate = analyze_stutter(&deltas_moderate);
        let result_severe = analyze_stutter(&deltas_severe);

        assert!(
            result_minor
                .iter()
                .any(|e| e.severity == StutterSeverity::Minor),
            "Should detect minor stutter"
        );
        assert!(
            result_moderate
                .iter()
                .any(|e| e.severity == StutterSeverity::Moderate),
            "Should detect moderate stutter"
        );
        assert!(
            result_severe
                .iter()
                .any(|e| e.severity == StutterSeverity::Severe),
            "Should detect severe stutter"
        );
    }
}

#[cfg(test)]
mod detector_tests {
    use super::*;
    use std::f64::consts::TAU;

    type Path = Box<dyn Fn(f64) -> (f64, f64)>;

    /// One stretch of hand movement: duration (s) and position (counts) over
    /// time since the stretch started.
    struct Seg {
        dur: f64,
        pos: Path,
    }

    fn seg(dur: f64, pos: impl Fn(f64) -> (f64, f64) + 'static) -> Seg {
        Seg {
            dur,
            pos: Box::new(pos),
        }
    }

    /// Back and forth along X with a slight hand drift on Y.
    fn back_and_forth(dur: f64, amplitude: f64, freq: f64) -> Seg {
        seg(dur, move |t| {
            (
                amplitude * (TAU * freq * t).sin(),
                0.05 * amplitude * (TAU * 0.37 * t).sin(),
            )
        })
    }

    fn circles(dur: f64, radius: f64, freq: f64) -> Seg {
        seg(dur, move |t| {
            (
                radius * (TAU * freq * t).cos(),
                radius * (TAU * freq * t).sin(),
            )
        })
    }

    /// Slow wandering with reversals: under one count per millisecond.
    fn slow_wander(dur: f64) -> Seg {
        seg(dur, |t| {
            (150.0 * (TAU * 0.5 * t).sin(), 60.0 * (TAU * 0.3 * t).sin())
        })
    }

    fn pause(dur: f64) -> Seg {
        seg(dur, |_| (0.0, 0.0))
    }

    /// Chain segments so each starts where the previous one ended.
    fn path(segs: Vec<Seg>) -> (f64, impl Fn(f64) -> (f64, f64)) {
        let mut plan = Vec::new();
        let (mut t0, mut base) = (0.0, (0.0, 0.0));
        for s in segs {
            let p0 = (s.pos)(0.0);
            let p1 = (s.pos)(s.dur);
            plan.push((t0, s.dur, base, p0, s.pos));
            base = (base.0 + p1.0 - p0.0, base.1 + p1.1 - p0.1);
            t0 += s.dur;
        }
        let total = t0;
        let f = move |t: f64| {
            let (start, dur, base, p0, pos) = plan
                .iter()
                .find(|(start, dur, ..)| t < start + dur)
                .unwrap_or_else(|| plan.last().unwrap());
            let p = pos((t - start).min(*dur));
            (base.0 + p.0 - p0.0, base.1 + p.1 - p0.1)
        };
        (total, f)
    }

    /// Small deterministic pseudo-random generator for timestamp noise.
    struct Lcg(u64);
    impl Lcg {
        fn unit(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
        }
    }

    /// Simulated mouse polled at `hz`. The sensor keeps the fractional part of
    /// the motion and reports only when a whole count has built up, like real
    /// hardware. `dropped(t)` marks polls the host misses (counts carry over
    /// to the next report); `noise_s` is +- host timestamp noise.
    fn simulate(
        hz: f64,
        segs: Vec<Seg>,
        dropped: impl Fn(f64) -> bool,
        noise_s: f64,
    ) -> Vec<(f64, i32, i32)> {
        let (total, pos) = path(segs);
        let mut rng = Lcg(42);
        let (mut rx, mut ry) = (0i64, 0i64);
        let mut out = Vec::new();
        let polls = (total * hz) as usize;
        for k in 1..polls {
            let t = k as f64 / hz;
            if dropped(t) {
                continue;
            }
            let (x, y) = pos(t);
            let dx = (x - rx as f64).trunc() as i64;
            let dy = (y - ry as f64).trunc() as i64;
            if dx != 0 || dy != 0 {
                rx += dx;
                ry += dy;
                out.push((t + noise_s * rng.unit(), dx as i32, dy as i32));
            }
        }
        out
    }

    fn run(reports: &[(f64, i32, i32)], multiplier: f64) -> StutterDetector {
        let mut d = StutterDetector::new(multiplier);
        for &(t, dx, dy) in reports {
            d.feed(t, dx, dy);
        }
        d
    }

    /// What an imperfect person does when told to "move in circles or back
    /// and forth": fast passes, a pause, slow wandering, reversals.
    fn everyday_session(scale: f64) -> Vec<Seg> {
        vec![
            back_and_forth(3.0, 800.0 * scale, 1.5),
            pause(2.0),
            slow_wander(3.0),
            circles(4.0, 600.0 * scale, 1.0),
            pause(1.5),
            back_and_forth(2.0, 500.0 * scale, 2.5),
        ]
    }

    fn no_drops(_: f64) -> bool {
        false
    }

    #[test]
    fn healthy_1khz_everyday_session_has_no_stutters() {
        // 800 DPI-like amplitudes, +-50 us host timestamp noise
        let reports = simulate(1000.0, everyday_session(1.0), no_drops, 50e-6);
        for multiplier in [1.5, 2.0, DEFAULT_STUTTER_MULTIPLIER] {
            let d = run(&reports, multiplier);
            assert_eq!(d.stutter_count(), 0, "multiplier {}", multiplier);
            assert!(d.judged_secs() >= MIN_JUDGED_SECS, "{}", d.judged_secs());
            assert_eq!(d.rating(), StutterRating::Excellent);
            let hz = d.polling_hz().unwrap();
            assert!((hz - 1000.0).abs() < 20.0, "{}", hz);
        }
        // The slow wandering and reversals really did produce long gaps
        let longest = reports
            .windows(2)
            .map(|w| (w[1].0 - w[0].0) * 1000.0)
            .filter(|&ms| ms <= MAX_POLL_INTERVAL_MS)
            .fold(0.0, f64::max);
        assert!(longest > 10.0, "{}", longest);
    }

    #[test]
    fn healthy_8khz_everyday_session_has_no_stutters() {
        // 3200 DPI-like amplitudes; +-20 us noise and every 50th report
        // handled 0.3 ms late by the host with the next one catching up.
        let mut reports = simulate(8000.0, everyday_session(4.0), no_drops, 20e-6);
        for (i, r) in reports.iter_mut().enumerate() {
            if i % 50 == 0 {
                r.0 += 0.0003;
            }
        }
        let d = run(&reports, 2.0);
        assert_eq!(d.stutter_count(), 0);
        assert!(d.judged_secs() >= MIN_JUDGED_SECS, "{}", d.judged_secs());
        assert_eq!(d.rating(), StutterRating::Excellent);
        let hz = d.polling_hz().unwrap();
        assert!((hz - 8000.0).abs() < 200.0, "{}", hz);
    }

    #[test]
    fn healthy_125hz_with_noisy_timestamps_has_no_stutters() {
        let reports = simulate(125.0, everyday_session(1.0), no_drops, 0.5e-3);
        let d = run(&reports, 2.0);
        assert_eq!(d.stutter_count(), 0);
        assert!((d.polling_hz().unwrap() - 125.0).abs() < 3.0);
    }

    #[test]
    fn slow_movement_only_is_inconclusive_not_a_fault() {
        let reports = simulate(1000.0, vec![slow_wander(10.0)], no_drops, 50e-6);
        assert!(reports.len() > 500);
        let d = run(&reports, 2.0);
        assert_eq!(d.stutter_count(), 0);
        assert_eq!(d.judged(), 0);
        assert_eq!(d.rating(), StutterRating::Inconclusive);
    }

    #[test]
    fn dropouts_during_fast_movement_are_detected_at_1khz() {
        // 5 ms of missed polls every 250 ms while moving in circles: 4 visible
        // hitches per second.
        let drop = |t: f64| t > 0.1 && (t % 0.25) < 0.005;
        let reports = simulate(1000.0, vec![circles(6.0, 600.0, 1.0)], drop, 50e-6);
        let d = run(&reports, 2.0);
        assert_eq!(d.stutter_count(), 23);
        assert_eq!(d.rating(), StutterRating::Poor);
        let (minor, moderate, severe) = d.severity_counts();
        assert_eq!((minor, moderate, severe), (0, 23, 0));
        // 5-6 missed polls (float rounding of the drop window) plus noise
        assert!(
            (6.0..7.2).contains(&d.worst_gap_ms()),
            "{}",
            d.worst_gap_ms()
        );
    }

    #[test]
    fn dropouts_during_fast_movement_are_detected_at_8khz() {
        // 3 ms dropouts every 500 ms on an 8 kHz mouse at 3200 DPI, brisk
        // circles (about 4 counts per report)
        let drop = |t: f64| t > 0.1 && (t % 0.5) < 0.003;
        let reports = simulate(8000.0, vec![circles(5.0, 2400.0, 2.0)], drop, 20e-6);
        let d = run(&reports, 2.0);
        assert_eq!(d.stutter_count(), 9);
        assert_eq!(d.severity_counts(), (9, 0, 0));
        assert_eq!(d.rating(), StutterRating::Fair);
    }

    #[test]
    fn severe_dropouts_are_classified_severe() {
        let drop = |t: f64| t > 0.1 && (t % 1.0) < 0.012;
        let reports = simulate(1000.0, vec![circles(4.0, 600.0, 1.0)], drop, 0.0);
        let d = run(&reports, 2.0);
        assert_eq!(d.stutter_count(), 3);
        assert_eq!(d.severity_counts(), (0, 0, 3));
    }

    #[test]
    fn each_interval_is_judged_once_however_it_is_fed() {
        let drop = |t: f64| t > 0.1 && (t % 0.25) < 0.005;
        let reports = simulate(1000.0, vec![circles(3.0, 600.0, 1.0)], drop, 0.0);
        let all = run(&reports, 2.0);
        // Feed in GUI-frame-sized chunks and query the totals in between
        let mut chunked = StutterDetector::new(2.0);
        for frame in reports.chunks(16) {
            for &(t, dx, dy) in frame {
                chunked.feed(t, dx, dy);
            }
            let _ = (chunked.stutter_count(), chunked.rating());
        }
        assert_eq!(all.stutter_count(), 11);
        assert_eq!(chunked.stutter_count(), all.stutter_count());
        assert_eq!(chunked.judged(), all.judged());
        assert_eq!(all.recent_stutters().count(), 11);
    }

    #[test]
    fn idle_gaps_do_not_poison_the_baseline() {
        let reports = simulate(
            1000.0,
            vec![
                circles(1.0, 600.0, 1.0),
                pause(2.0),
                circles(1.0, 600.0, 1.0),
                pause(5.0),
                circles(1.0, 600.0, 1.0),
            ],
            no_drops,
            0.0,
        );
        let d = run(&reports, 2.0);
        assert_eq!(d.stutter_count(), 0);
        assert!((d.median_interval_ms().unwrap() - 1.0).abs() < 1e-6);
        assert!(d.judged_max_ms().unwrap() < 1.5);
    }

    #[test]
    fn stutters_before_the_baseline_exists_are_still_judged() {
        // Dropout 20 ms into the session, long before 50 fast intervals exist
        let drop = |t: f64| (0.02..0.026).contains(&t);
        let reports = simulate(1000.0, vec![circles(1.0, 600.0, 1.0)], drop, 0.0);
        let mut d = StutterDetector::new(2.0);
        for &(t, dx, dy) in reports.iter().take(30) {
            d.feed(t, dx, dy);
        }
        assert_eq!(d.judged(), 0);
        assert!(d.pending() > 0);
        assert!(d.threshold_ms().is_none());
        for &(t, dx, dy) in reports.iter().skip(30) {
            d.feed(t, dx, dy);
        }
        assert_eq!(d.stutter_count(), 1);
        assert_eq!(d.pending(), 0);
    }

    #[test]
    fn threshold_has_an_absolute_floor_at_high_rates() {
        assert_eq!(stutter_threshold_ms(0.125, 2.0), MIN_STUTTER_GAP_MS);
        assert_eq!(stutter_threshold_ms(1.0, 2.0), MIN_STUTTER_GAP_MS);
        assert_eq!(stutter_threshold_ms(1.0, 3.0), 3.0);
        assert_eq!(stutter_threshold_ms(8.0, 2.0), 16.0);
    }

    #[test]
    fn rating_needs_enough_fast_movement() {
        assert_eq!(rate_stutters(0.0, 0), StutterRating::Inconclusive);
        assert_eq!(rate_stutters(2.9, 4), StutterRating::Inconclusive);
        assert_eq!(rate_stutters(1.0, 5), StutterRating::Fair);
        assert_eq!(rate_stutters(2.0, 7), StutterRating::Poor);
        assert_eq!(rate_stutters(10.0, 0), StutterRating::Excellent);
        assert_eq!(rate_stutters(10.0, 1), StutterRating::Good);
        assert_eq!(rate_stutters(10.0, 10), StutterRating::Fair);
        assert_eq!(rate_stutters(10.0, 40), StutterRating::Poor);
    }

    #[test]
    fn zero_motion_and_out_of_order_reports_are_ignored() {
        let mut d = StutterDetector::new(2.0);
        assert_eq!(d.feed(0.0, 0, 0), None);
        assert_eq!(d.feed(0.001, 3, 0), None);
        assert_eq!(d.feed(0.0005, 3, 0), None);
        assert_eq!(d.reports(), 2);
        assert_eq!(d.judged_mean_ms(), None);
    }
}
