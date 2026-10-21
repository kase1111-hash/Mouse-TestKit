//! Lift-off jump detection
//!
//! A sensor that keeps tracking while it goes out of focus makes the cursor
//! jump when the mouse is lifted or set back down. Any motion that starts
//! out of stillness is a *burst*. A jump is not one report - at 1000 Hz it
//! is spread over many - so the motion in the first [`JUMP_WINDOW_MS`] of
//! each burst is summed, and the furthest the cursor got from where the
//! burst started is compared with [`JUMP_THRESHOLD_MM`], converted to counts
//! with the mouse's DPI so the verdict does not depend on the DPI setting.
//! A burst that keeps moving the mouse by more than [`SLIDE_AFTER_MM`] after
//! that window is the hand sliding the mouse, not a lift, and is not judged;
//! a jump followed by a small re-grip still counts.
//!
//! A clean lift produces no motion at all, so "no jumps" only means
//! something once the user has actually lifted the mouse: [`verdict`] needs
//! a number of lifts before it calls the result clean.
//!
//! Used by the CLI Lift-Off Jump test, the GUI Lift-Off Jump panel and the
//! lift step of Auto Diagnostics, so all three judge a lift the same way.

use std::collections::VecDeque;

/// Window over which the mouse counts as still or moving: a burst starts
/// when motion within it reaches [`STILL_MM`] and ends once a whole window
/// passes with less.
pub const IDLE_THRESHOLD_MS: u64 = 100;
/// Lifting or setting down takes the sensor through its 1-2 mm focus range
/// in a few tens of ms, so a lift artefact is over within this window.
pub const JUMP_WINDOW_MS: f64 = 100.0;
/// A burst that moves the mouse further than this (path length) after
/// [`JUMP_WINDOW_MS`] is a deliberate slide. Judging by how much motion
/// follows, rather than by how long the burst lasts, keeps a set-down jump
/// that runs into a small re-grip of the hand from being dismissed.
pub const SLIDE_AFTER_MM: f64 = 2.0;
/// A lift or set-down that moves the cursor further than this is a jump.
/// Lifting straight up or setting down usually moves the mouse sideways by
/// well under a millimetre; 2 mm is ~63 counts at 800 DPI, a clearly visible
/// jump. A single careless set-down can exceed it, so one jump alone is only
/// reported as a possible problem.
pub const JUMP_THRESHOLD_MM: f64 = 2.0;
/// Less motion than this within [`IDLE_THRESHOLD_MS`] is the mouse at rest:
/// a resting hand at high DPI gives a stray count here and there, which must
/// neither start a burst nor keep one open and swallow a later jump.
pub const STILL_MM: f64 = 0.25;
/// Used when the mouse's DPI is not known (the usual factory default).
pub const DEFAULT_DPI: f64 = 800.0;
/// Two or more jumps are a pattern; a single one may be a hand slip.
pub const CONFIRMED_JUMPS: usize = 2;
/// Lifts the user must report before "no jumps" counts as a clean result.
pub const MIN_LIFTS: usize = 5;

const MM_PER_INCH: f64 = 25.4;

pub fn mm_to_counts(mm: f64, dpi: f64) -> f64 {
    mm / MM_PER_INCH * dpi
}

pub fn counts_to_mm(counts: f64, dpi: f64) -> f64 {
    counts / dpi * MM_PER_INCH
}

/// The DPI to judge with: `dpi` when it is a usable value, otherwise
/// [`DEFAULT_DPI`]. The flag is true when the default was assumed, so the
/// result can say that its distances depend on that guess.
pub fn dpi_or_default(dpi: Option<f64>) -> (f64, bool) {
    match dpi.filter(|d| d.is_finite() && *d > 0.0) {
        Some(d) => (d, false),
        None => (DEFAULT_DPI, true),
    }
}

/// Overall result of a lift-off test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiftVerdict {
    /// [`CONFIRMED_JUMPS`] or more jumps: the sensor tracks while lifted.
    Jumps,
    /// One jump: often the hand sliding the mouse on set-down; repeat.
    OneJump,
    /// No jumps, but too few lifts to call that a clean result.
    TooFewLifts,
    /// No jumps over enough lifts.
    Clean,
}

impl LiftVerdict {
    pub fn label(self) -> &'static str {
        match self {
            LiftVerdict::Jumps => "Cursor jumps on lift",
            LiftVerdict::OneJump => "One jump - repeat the test",
            LiftVerdict::TooFewLifts => "Inconclusive - too few lifts",
            LiftVerdict::Clean => "No jumps",
        }
    }
}

/// Verdict for `jumps` found over `lifts` lifts, where at least `min_lifts`
/// lifts are needed before "no jumps" is a clean result. Jumps are evidence
/// however few lifts were counted.
pub fn verdict(jumps: usize, lifts: usize, min_lifts: usize) -> LiftVerdict {
    if jumps >= CONFIRMED_JUMPS {
        LiftVerdict::Jumps
    } else if jumps == 1 {
        LiftVerdict::OneJump
    } else if lifts < min_lifts {
        LiftVerdict::TooFewLifts
    } else {
        LiftVerdict::Clean
    }
}

/// Motion that started after the mouse had been still.
#[derive(Debug, Clone, PartialEq)]
pub struct Burst {
    /// Net counts at the point furthest from the start, within the window.
    pub peak_dx: i64,
    pub peak_dy: i64,
    /// That furthest distance, in counts.
    pub peak_counts: f64,
    /// Time from the start of the burst to that furthest point (ms).
    pub peak_ms: f64,
    /// Path length moved after the jump window (counts).
    pub after_window_counts: f64,
    /// Time from when the motion began to its last report (ms).
    pub duration_ms: f64,
}

/// What a burst was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BurstKind {
    /// Short burst that moved the cursor further than the jump threshold.
    Jump,
    /// Short burst within the threshold: a clean lift or set-down.
    Quiet,
    /// Kept moving the mouse after the window: the hand slid it.
    Slide,
}

impl Burst {
    pub fn classify(&self, dpi: f64) -> BurstKind {
        if self.after_window_counts > mm_to_counts(SLIDE_AFTER_MM, dpi) {
            BurstKind::Slide
        } else if self.peak_counts > mm_to_counts(JUMP_THRESHOLD_MM, dpi) {
            BurstKind::Jump
        } else {
            BurstKind::Quiet
        }
    }
}

#[derive(Debug)]
struct OpenBurst {
    start: f64,
    last: f64,
    x: i64,
    y: i64,
    /// Furthest point: (dx, dy, distance, seconds after start).
    peak: (i64, i64, f64, f64),
    /// Path length after the jump window.
    after_window: f64,
}

impl OpenBurst {
    fn finish(self) -> Burst {
        Burst {
            peak_dx: self.peak.0,
            peak_dy: self.peak.1,
            peak_counts: self.peak.2,
            peak_ms: self.peak.3 * 1000.0,
            after_window_counts: self.after_window,
            duration_ms: (self.last - self.start) * 1000.0,
        }
    }
}

/// Splits motion into bursts that start from stillness. Times are seconds.
#[derive(Debug)]
pub struct LiftDetector {
    idle_s: f64,
    window_s: f64,
    still_counts: f64,
    /// Reports (time, dx, dy) within the last idle window.
    recent: VecDeque<(f64, i32, i32)>,
    current: Option<OpenBurst>,
    /// Until the mouse has been seen still for a whole idle window: the
    /// last time it was not. No burst opens meanwhile.
    unsettled_since: Option<f64>,
}

impl LiftDetector {
    /// A detector that takes the mouse to be still before the first report.
    pub fn new(dpi: f64) -> Self {
        Self {
            idle_s: IDLE_THRESHOLD_MS as f64 / 1000.0,
            window_s: JUMP_WINDOW_MS / 1000.0,
            still_counts: mm_to_counts(STILL_MM, dpi),
            recent: VecDeque::new(),
            current: None,
            unsettled_since: None,
        }
    }

    /// Watch from time `t` without knowing whether the mouse was still
    /// then: motion is only judged once the mouse has been seen still for a
    /// whole idle window, so a movement already under way at `t` is not
    /// taken for a burst that started from stillness.
    pub fn watching_from(mut self, t: f64) -> Self {
        self.unsettled_since = Some(t);
        self
    }

    /// Drop reports that are a whole idle window older than `t`.
    fn prune(&mut self, t: f64) {
        while self
            .recent
            .front()
            .is_some_and(|&(rt, _, _)| t - rt >= self.idle_s)
        {
            self.recent.pop_front();
        }
    }

    /// Path length of the reports in the last idle window.
    fn recent_path(&self) -> f64 {
        self.recent
            .iter()
            .map(|&(_, dx, dy)| (dx as f64).hypot(dy as f64))
            .sum()
    }

    /// The open burst ends once it has lasted at least one idle window and
    /// the last idle window was still.
    fn try_finish(&mut self, now: f64) -> Option<Burst> {
        let started = self.current.as_ref()?.start;
        self.prune(now);
        if now - started >= self.idle_s && self.recent_path() < self.still_counts {
            self.current.take().map(OpenBurst::finish)
        } else {
            None
        }
    }

    /// Feed one motion report. Returns the previous burst if it had ended.
    pub fn feed(&mut self, t: f64, dx: i32, dy: i32) -> Option<Burst> {
        if dx == 0 && dy == 0 {
            return None;
        }
        let finished = self.try_finish(t);
        self.prune(t);
        self.recent.push_back((t, dx, dy));
        if self
            .unsettled_since
            .is_some_and(|since| t - since >= self.idle_s)
        {
            self.unsettled_since = None;
        }

        match &mut self.current {
            None if self.unsettled_since.is_some() => {
                if self.recent_path() >= self.still_counts {
                    self.unsettled_since = Some(t);
                }
            }
            None => {
                // Motion out of stillness: the reports that built it up are
                // the start of the burst.
                if self.recent_path() >= self.still_counts {
                    let (x, y) = self
                        .recent
                        .iter()
                        .fold((0i64, 0i64), |(x, y), &(_, dx, dy)| {
                            (x + dx as i64, y + dy as i64)
                        });
                    self.current = Some(OpenBurst {
                        start: t,
                        last: t,
                        x,
                        y,
                        peak: (x, y, (x as f64).hypot(y as f64), 0.0),
                        after_window: 0.0,
                    });
                }
            }
            Some(b) => {
                if t - b.start <= self.window_s {
                    b.x += dx as i64;
                    b.y += dy as i64;
                    let d = (b.x as f64).hypot(b.y as f64);
                    if d > b.peak.2 {
                        b.peak = (b.x, b.y, d, t - b.start);
                    }
                } else {
                    b.after_window += (dx as f64).hypot(dy as f64);
                }
                b.last = t;
            }
        }
        finished
    }

    /// Advance time without a report; returns the burst if it has ended.
    pub fn tick(&mut self, now: f64) -> Option<Burst> {
        self.try_finish(now)
    }

    /// End the test: return whatever burst is still open.
    pub fn flush(&mut self) -> Option<Burst> {
        self.current.take().map(OpenBurst::finish)
    }

    pub fn in_burst(&self) -> bool {
        self.current.is_some()
    }
}

/// Running totals of a lift-off test, as every front-end reports them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiftTally {
    /// Bursts that were jumps, in order.
    pub jumps: Vec<Burst>,
    /// Lifts and set-downs that stayed within the threshold.
    pub quiet: usize,
    /// Sliding movements, not judged.
    pub slides: usize,
    /// Largest lift or set-down motion (jumps and quiet bursts), in counts.
    pub largest_counts: f64,
}

impl LiftTally {
    /// Classify a finished burst at `dpi` and add it to the totals.
    pub fn record(&mut self, burst: &Burst, dpi: f64) -> BurstKind {
        let kind = burst.classify(dpi);
        match kind {
            BurstKind::Jump => self.jumps.push(burst.clone()),
            BurstKind::Quiet => self.quiet += 1,
            BurstKind::Slide => self.slides += 1,
        }
        if kind != BurstKind::Slide {
            self.largest_counts = self.largest_counts.max(burst.peak_counts);
        }
        kind
    }

    /// Every burst seen: each is motion that started after a pause.
    pub fn bursts(&self) -> usize {
        self.jumps.len() + self.quiet + self.slides
    }

    /// Largest jump, in counts (0 without jumps).
    pub fn max_jump_counts(&self) -> f64 {
        self.jumps.iter().map(|j| j.peak_counts).fold(0.0, f64::max)
    }

    /// Average jump, in counts (0 without jumps).
    pub fn avg_jump_counts(&self) -> f64 {
        if self.jumps.is_empty() {
            0.0
        } else {
            self.jumps.iter().map(|j| j.peak_counts).sum::<f64>() / self.jumps.len() as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed reports `(t_ms, dx, dy)`, then let time run on; collect bursts.
    fn run(det: &mut LiftDetector, reports: &[(f64, i32, i32)]) -> Vec<Burst> {
        let mut out = Vec::new();
        let mut last = 0.0;
        for &(t_ms, dx, dy) in reports {
            out.extend(det.tick(t_ms / 1000.0));
            out.extend(det.feed(t_ms / 1000.0, dx, dy));
            last = t_ms;
        }
        out.extend(det.tick(last / 1000.0 + 1.0));
        out
    }

    fn kinds(bursts: &[Burst], dpi: f64) -> Vec<BurstKind> {
        bursts.iter().map(|b| b.classify(dpi)).collect()
    }

    #[test]
    fn test_jump_spread_over_many_reports_is_detected() {
        // 200 counts at 10 counts/ms (1000 Hz) after stillness. Each report
        // is only 10 counts, so the old per-report check never saw it.
        let dpi = 1600.0;
        let mut det = LiftDetector::new(dpi);
        let reports: Vec<(f64, i32, i32)> = (0..20).map(|i| (500.0 + i as f64, 10, 0)).collect();
        let bursts = run(&mut det, &reports);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Jump]);
        assert_eq!(bursts[0].peak_dx, 200);
        let mm = counts_to_mm(bursts[0].peak_counts, dpi);
        assert!((mm - 3.175).abs() < 0.01, "{} mm", mm);
    }

    #[test]
    fn test_jump_in_few_reports_at_125hz_is_detected() {
        // A 125 Hz mouse delivers a 4 mm set-down jump in three reports.
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi);
        let bursts = run(&mut det, &[(400.0, 60, 10), (408.0, 50, 5), (416.0, 16, 0)]);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Jump]);
        assert!(counts_to_mm(bursts[0].peak_counts, dpi) > 3.9);
    }

    #[test]
    fn test_normal_flick_at_high_dpi_is_not_a_jump() {
        // 1600 DPI, 125 Hz: the first report of an ordinary movement is
        // already 60 counts, which the old check called a jump.
        let dpi = 1600.0;
        let mut det = LiftDetector::new(dpi);
        let reports: Vec<(f64, i32, i32)> =
            (0..50).map(|i| (300.0 + i as f64 * 8.0, 60, 5)).collect();
        let bursts = run(&mut det, &reports);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Slide]);
    }

    #[test]
    fn test_small_set_down_wobble_is_quiet() {
        // Setting the mouse down shifts it a few counts: 0.5 mm at 800 DPI.
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi);
        let bursts = run(
            &mut det,
            &[
                (400.0, 3, 1),
                (401.0, 4, 0),
                (402.0, 3, -1),
                (404.0, 2, 0),
                (407.0, 3, 1),
            ],
        );
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Quiet]);
        assert!(counts_to_mm(bursts[0].peak_counts, dpi) < 1.0);
    }

    #[test]
    fn test_set_down_wobble_at_16000_dpi_is_quiet() {
        // At 16000 DPI the same 0.5 mm wobble is 315 counts - five times
        // the old fixed 50-count limit - and is still a clean set-down.
        let dpi = 16000.0;
        let mut det = LiftDetector::new(dpi);
        let reports: Vec<(f64, i32, i32)> = (0..15).map(|i| (400.0 + i as f64, 21, 0)).collect();
        let bursts = run(&mut det, &reports);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Quiet]);
    }

    #[test]
    fn test_threshold_follows_dpi() {
        // The same 60-count burst is a 3.8 mm jump at 400 DPI but only
        // 0.5 mm at 3200 DPI.
        let reports = [(200.0, 30, 0), (201.0, 30, 0)];
        let at_400 = run(&mut LiftDetector::new(400.0), &reports);
        assert_eq!(kinds(&at_400, 400.0), vec![BurstKind::Jump]);
        let at_3200 = run(&mut LiftDetector::new(3200.0), &reports);
        assert_eq!(kinds(&at_3200, 3200.0), vec![BurstKind::Quiet]);
    }

    #[test]
    fn test_back_and_forth_jump_uses_peak_distance() {
        // Out 100 counts and straight back: net zero, but the cursor jumped.
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi);
        let mut reports: Vec<(f64, i32, i32)> =
            (0..10).map(|i| (300.0 + i as f64, 0, -10)).collect();
        reports.extend((10..20).map(|i| (300.0 + i as f64, 0, 10)));
        let bursts = run(&mut det, &reports);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Jump]);
        assert_eq!((bursts[0].peak_dx, bursts[0].peak_dy), (0, -100));
    }

    #[test]
    fn test_hand_tremor_does_not_hide_a_jump() {
        // At 1600 DPI a resting hand produces a stray count every ~30 ms.
        // The mouse is still "still", so the jump is still found.
        let dpi = 1600.0;
        let mut det = LiftDetector::new(dpi);
        let mut reports: Vec<(f64, i32, i32)> = (0..30).map(|i| (i as f64 * 30.0, 1, 0)).collect();
        reports.extend((0..10).map(|i| (905.0 + i as f64, 0, 15)));
        reports.extend((0..10).map(|i| (1000.0 + i as f64 * 30.0, 0, 1)));
        let bursts = run(&mut det, &reports);
        // Tremor alone never opens a burst; the jump is the only one.
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Jump], "{:?}", bursts);
        assert!((150..=152).contains(&bursts[0].peak_dy), "{:?}", bursts);
    }

    #[test]
    fn test_motion_after_the_window_is_not_added() {
        // Set-down shift of 20 counts, then small corrections in the same
        // burst. Only the first 100 ms is judged: 50 counts (1.6 mm). All of
        // it would be 70 counts (2.2 mm) and a "jump".
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi);
        let mut reports: Vec<(f64, i32, i32)> =
            (0..4).map(|i| (300.0 + i as f64 * 2.0, 5, 0)).collect();
        reports.extend((0..5).map(|i| (360.0 + i as f64 * 20.0, 10, 0)));
        let bursts = run(&mut det, &reports);
        assert_eq!(bursts.len(), 1);
        assert_eq!(bursts[0].peak_dx, 50);
        assert_eq!(bursts[0].classify(dpi), BurstKind::Quiet);
    }

    #[test]
    fn test_tremor_alone_is_not_a_burst() {
        let mut det = LiftDetector::new(1600.0);
        let reports: Vec<(f64, i32, i32)> = (0..100)
            .map(|i| (i as f64 * 25.0, if i % 2 == 0 { 1 } else { -1 }, 0))
            .collect();
        assert!(run(&mut det, &reports).is_empty());
    }

    #[test]
    fn test_repeated_lifts_give_separate_bursts() {
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi);
        let mut reports = Vec::new();
        for lift in 0..3 {
            let t0 = 500.0 + lift as f64 * 1000.0;
            reports.extend((0..8).map(|i| (t0 + i as f64, 12, 4)));
        }
        let bursts = run(&mut det, &reports);
        assert_eq!(
            kinds(&bursts, dpi),
            vec![BurstKind::Jump, BurstKind::Jump, BurstKind::Jump]
        );
    }

    #[test]
    fn test_jump_followed_by_regrip_is_still_a_jump() {
        // 1600 DPI: a 4 mm set-down jump in 30 ms, then the hand re-grips
        // the mouse (about 1 mm over 150 ms) before it has been still for a
        // whole idle window. The burst lasts over 200 ms, which a
        // duration-based slide rule dismissed as "sliding".
        let dpi = 1600.0;
        let mut det = LiftDetector::new(dpi);
        let mut reports: Vec<(f64, i32, i32)> = (0..30).map(|i| (500.0 + i as f64, 8, 1)).collect();
        reports.extend((0..30).map(|i| (580.0 + i as f64 * 5.0, 2, 1)));
        let bursts = run(&mut det, &reports);
        assert_eq!(bursts.len(), 1, "{:?}", bursts);
        assert!(bursts[0].duration_ms > 200.0, "{:?}", bursts);
        assert_eq!(bursts[0].classify(dpi), BurstKind::Jump, "{:?}", bursts);
        assert!(bursts[0].peak_ms < JUMP_WINDOW_MS, "{:?}", bursts);
    }

    #[test]
    fn test_short_fast_slide_is_not_a_jump() {
        // 800 DPI: the hand pushes the mouse about 14 mm in 150 ms. It is
        // still moving fast after the first 100 ms (5 mm more), so it is a
        // slide even though it is over quickly; a duration-based rule called
        // it a 9 mm jump.
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi);
        let reports: Vec<(f64, i32, i32)> = (0..150)
            .map(|i| (300.0 + i as f64, if i < 50 { 1 } else { 4 }, 0))
            .collect();
        let bursts = run(&mut det, &reports);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Slide], "{:?}", bursts);
    }

    #[test]
    fn test_verdict_needs_lifts_before_calling_it_clean() {
        // Quitting at once, or never lifting, is not a clean result.
        assert_eq!(verdict(0, 0, MIN_LIFTS), LiftVerdict::TooFewLifts);
        assert_eq!(
            verdict(0, MIN_LIFTS - 1, MIN_LIFTS),
            LiftVerdict::TooFewLifts
        );
        assert_eq!(verdict(0, MIN_LIFTS, MIN_LIFTS), LiftVerdict::Clean);
        // Jumps are evidence whether or not lifts were counted.
        assert_eq!(verdict(1, 0, MIN_LIFTS), LiftVerdict::OneJump);
        assert_eq!(verdict(2, 0, MIN_LIFTS), LiftVerdict::Jumps);
        assert_eq!(verdict(3, 8, MIN_LIFTS), LiftVerdict::Jumps);
        // A front-end that cannot count lifts can still ask for one sign
        // that the step was done.
        assert_eq!(verdict(0, 0, 1), LiftVerdict::TooFewLifts);
        assert_eq!(verdict(0, 1, 1), LiftVerdict::Clean);
    }

    #[test]
    fn test_flush_returns_open_burst() {
        let mut det = LiftDetector::new(800.0);
        assert!(det.feed(0.5, 40, 0).is_none());
        assert!(det.in_burst());
        let b = det.flush().unwrap();
        assert_eq!(b.peak_dx, 40);
        assert!(!det.in_burst());
    }

    #[test]
    fn test_motion_under_way_when_watching_starts_is_not_a_burst() {
        // Watching starts while the hand is still moving the mouse (it was
        // moving before): that movement did not start from stillness.
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi).watching_from(10.0);
        let reports: Vec<(f64, i32, i32)> = (0..40).map(|i| (10_001.0 + i as f64, 3, 0)).collect();
        assert!(run(&mut det, &reports).is_empty());
        // Moving the whole time never gives a still window to start from.
        let mut det = LiftDetector::new(dpi).watching_from(10.0);
        let reports: Vec<(f64, i32, i32)> = (0..3000)
            .map(|i| (10_001.0 + i as f64 * 2.0, 3, 1))
            .collect();
        assert!(run(&mut det, &reports).is_empty());
    }

    #[test]
    fn test_jump_after_settling_is_found_when_watching_from() {
        // Motion at the start, a pause, then a set-down jump.
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi).watching_from(10.0);
        let mut reports: Vec<(f64, i32, i32)> =
            (0..50).map(|i| (10_001.0 + i as f64, 3, 0)).collect();
        reports.extend((0..10).map(|i| (10_400.0 + i as f64, 10, 0)));
        let bursts = run(&mut det, &reports);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Jump], "{:?}", bursts);
        assert_eq!(bursts[0].peak_dx, 100);
        // Motion only after a still first window counts straight away.
        let mut det = LiftDetector::new(dpi).watching_from(10.0);
        let reports: Vec<(f64, i32, i32)> = (0..10).map(|i| (10_150.0 + i as f64, 10, 0)).collect();
        assert_eq!(kinds(&run(&mut det, &reports), dpi), vec![BurstKind::Jump]);
    }

    #[test]
    fn test_tally_counts_each_kind() {
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi);
        // A jump, a quiet set-down and a slide, each after a pause.
        let mut reports: Vec<(f64, i32, i32)> = (0..8).map(|i| (500.0 + i as f64, 12, 4)).collect();
        reports.extend([(1500.0, 3, 1), (1502.0, 4, 0), (1504.0, 3, -1)]);
        reports.extend((0..60).map(|i| (2500.0 + i as f64 * 8.0, 40, 0)));
        let mut tally = LiftTally::default();
        let kinds: Vec<BurstKind> = run(&mut det, &reports)
            .iter()
            .map(|b| tally.record(b, dpi))
            .collect();
        assert_eq!(
            kinds,
            vec![BurstKind::Jump, BurstKind::Quiet, BurstKind::Slide]
        );
        assert_eq!((tally.jumps.len(), tally.quiet, tally.slides), (1, 1, 1));
        assert_eq!(tally.bursts(), 3);
        // The slide is far larger but is not a lift, so it is not "largest".
        assert!((tally.largest_counts - 101.19).abs() < 0.01, "{:?}", tally);
        assert_eq!(tally.max_jump_counts(), tally.avg_jump_counts());
    }

    #[test]
    fn test_dpi_or_default() {
        assert_eq!(dpi_or_default(Some(1600.0)), (1600.0, false));
        assert_eq!(dpi_or_default(None), (DEFAULT_DPI, true));
        assert_eq!(dpi_or_default(Some(0.0)), (DEFAULT_DPI, true));
        assert_eq!(dpi_or_default(Some(f64::NAN)), (DEFAULT_DPI, true));
    }

    #[test]
    fn test_unit_conversion() {
        assert!((mm_to_counts(25.4, 800.0) - 800.0).abs() < 1e-9);
        assert!((counts_to_mm(1600.0, 1600.0) - 25.4).abs() < 1e-9);
    }
}
