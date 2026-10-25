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
//! Distance alone cannot tell a jump from a quick nudge of the hand, so the
//! movement behind each jump-sized burst is also judged by its shape
//! ([`Shape`]). The hand moves the mouse with limited force: a movement it
//! makes speeds up from rest and slows down to rest, in one direction. A
//! sensor artefact needs no force: it starts and stops at full speed, or
//! turns back on itself. From the report times and counts the least
//! acceleration each end of the movement needed is worked out; a movement
//! that needed more than [`HAND_ACCEL`] at both ends (or to turn) is a
//! jump no hand can make, and a straight one that speeds up and slows down
//! gradually is the hand. Anything in between - typically the mouse landing
//! while it still moved sideways, or a 125 Hz mouse whose reports are too
//! coarse to show the shape - is only a *possible* jump.
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
/// Most acceleration (m/s^2, about 3 g) the hand gives the mouse while
/// lifting and setting it down. A deliberate movement from rest to rest
/// takes at least about 40 ms, and even a 5 mm nudge that quick peaks at
/// 18 m/s^2; a hasty set-down stops in a few m/s^2. A movement that needed
/// more than this both to start from rest and to come to rest (or to turn
/// back on itself) was not made by the hand; one that needed no more than
/// this anywhere, and is smooth, was.
pub const HAND_ACCEL: f64 = 30.0;
/// Fewest reports a movement needs to show that it sped up and slowed down
/// gradually. At 125 Hz that takes 40 ms of reports, so a nudge much
/// quicker than 50 ms is too short there to show its shape.
pub const HAND_MIN_REPORTS: usize = 5;
/// Shortest reported movement (ms) that can be the hand. A deliberate
/// movement lasts at least about 40 ms; the mouse reports the part where it
/// moved at least a count per report, which is at least 25 ms of that.
pub const HAND_MIN_MS: f64 = 25.0;

const MM_PER_INCH: f64 = 25.4;
/// A pause between reports this long (s) ends a movement: longer than any
/// common report interval (8 ms at 125 Hz, up to 15 ms over Bluetooth), so
/// the mouse moved less than a count meanwhile.
const STOP_GAP_S: f64 = 0.020;
/// What a mouse may hold back (counts): its unreported remainder is under
/// one count on each axis.
const REMAINDER_COUNTS: f64 = std::f64::consts::SQRT_2;
/// Reports less than this apart (s) were bunched by the host or a radio
/// link: the fastest USB polling (8000 Hz) is 125 us apart.
const MIN_REPORT_INTERVAL_S: f64 = 0.000_1;
/// Steady report times: every interval is one to [`STEADY_MAX_POLLS`]
/// polls, within this share (or [`TIMESTAMP_JITTER_S`]). A USB mouse
/// reports on its polls (and skips a poll when it has less than a count to
/// send); bunched delivery puts reports far closer together than the gaps
/// between bunches, under half a poll. Steady times are trusted to this
/// share of a poll.
const STEADY_SPREAD: f64 = 0.25;
/// How far (s) the host's timestamps of two reports can wander apart, with
/// a margin for estimating the poll from them: USB interrupt moderation (40
/// us by default on Linux) and the interrupt handler's own delay stamp a
/// report up to about 60 us late, half a poll at 8000 Hz. Times are never
/// trusted more closely than this.
const TIMESTAMP_JITTER_S: f64 = 0.000_075;
/// Most polls a steady mouse skips between two reports of one movement.
const STEADY_MAX_POLLS: f64 = 4.0;
/// A gradual end: over the first and last fifth of a movement, its speed
/// stays below this share of its top speed (a hand nudge stays under 0.5;
/// a jump at full speed is 1).
const RAMP_MAX: f64 = 0.6;
/// A straight movement strays sideways, or back, by at most this share of
/// its length (plus a count).
const STRAY_MAX: f64 = 0.2;
/// Reports kept per burst for its shape: the jump window at 8000 Hz, with
/// room for the stillness before it and the movement running on after it.
const MAX_SHAPE_REPORTS: usize = 4096;

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
    /// [`CONFIRMED_JUMPS`] or more jump-sized movements, but fewer than
    /// that whose shape rules out the hand (see [`LiftTally::verdict`]).
    PossibleJumps,
}

impl LiftVerdict {
    pub fn label(self) -> &'static str {
        match self {
            LiftVerdict::Jumps => "Cursor jumps on lift",
            LiftVerdict::OneJump => "One jump - repeat the test",
            LiftVerdict::TooFewLifts => "Inconclusive - too few lifts",
            LiftVerdict::Clean => "No jumps",
            LiftVerdict::PossibleJumps => "Possible jumps - repeat the test",
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
    /// Time of the last report, on the clock the detector was fed (s).
    pub end_s: f64,
    /// Shape of the movement that carried the cursor to that furthest point.
    pub shape: Shape,
}

/// What a burst was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BurstKind {
    /// Moved the cursor further than the jump threshold, faster than a hand
    /// can start and stop the mouse (or turning back on itself).
    Jump,
    /// Short burst within the threshold: a clean lift or set-down.
    Quiet,
    /// Kept moving the mouse after the window: the hand slid it.
    Slide,
    /// Moved the cursor further than the jump threshold, but its shape
    /// cannot tell a jump from the hand (for example the mouse landing
    /// while it still moved sideways).
    PossibleJump,
    /// Moved the cursor further than the jump threshold, but sped up and
    /// slowed down gradually in one direction: the hand nudged the mouse.
    Nudge,
}

impl Burst {
    pub fn classify(&self, dpi: f64) -> BurstKind {
        if self.after_window_counts > mm_to_counts(SLIDE_AFTER_MM, dpi) {
            BurstKind::Slide
        } else if self.peak_counts <= mm_to_counts(JUMP_THRESHOLD_MM, dpi) {
            BurstKind::Quiet
        } else if self.shape.beyond_hand(dpi) {
            BurstKind::Jump
        } else if self.shape.hand_like(dpi) {
            BurstKind::Nudge
        } else {
            BurstKind::PossibleJump
        }
    }
}

/// How the movement behind a burst's jump started, moved and stopped: of
/// the movements in the burst (reports with no pause of [`STOP_GAP_S`])
/// that began by its furthest point, the one that carried the cursor
/// furthest.
///
/// The accelerations are lower bounds in counts/s^2: the least any motion
/// that produced these reports could have needed, allowing for the counts
/// a mouse holds back and for the report times being off by a quarter of
/// a poll (at least [`TIMESTAMP_JITTER_S`]). The start is bounded only from the speed
/// reached after the distance covered, not from when the first report
/// came: a mouse waking from power saving may put several ms of motion into
/// its first report.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Shape {
    /// Reports in the movement.
    pub reports: usize,
    /// How long it was reported for (ms): from one report interval before
    /// its first report to its last.
    pub span_ms: f64,
    /// To start from rest.
    pub start_accel: f64,
    /// To come to rest.
    pub stop_accel: f64,
    /// To turn back on its own path.
    pub turn_accel: f64,
    /// The reports came at steady intervals, so their times can be trusted
    /// to a fraction of an interval. Bunched delivery (Bluetooth, a stalled
    /// host) puts several reports at nearly one time and is not steady. A
    /// stray count at either end, a pause away from the rest, is left out.
    pub steady_timing: bool,
    /// Straight, and slow at both ends compared with its top speed, over
    /// at least [`HAND_MIN_REPORTS`] reports.
    pub smooth: bool,
    /// For any stretch of it: from the speed the path covered before allows
    /// (from rest), to the speed the path still to come allows (to rest). At
    /// least `start_accel` and `stop_accel`; more for a sensor spike among a
    /// resting hand's stray counts. Short stretches need steady times, so
    /// without them it is no more than those two.
    pub stretch_accel: f64,
}

impl Shape {
    /// `counts_per_s2` in m/s^2 at `dpi`.
    fn to_m_s2(counts_per_s2: f64, dpi: f64) -> f64 {
        counts_to_mm(counts_per_s2, dpi) / 1000.0
    }

    /// No hand did this: it started and stopped (or turned back) harder
    /// than [`HAND_ACCEL`]. A hand-made movement has at least one gentle
    /// end even when the other is not real motion - the sensor picking up
    /// a mouse that lands while moving sideways, or losing it as the mouse
    /// leaves the pad - so both ends must be too hard. Needs steady report
    /// times and at least three reports, so one interval of each end shows.
    pub fn beyond_hand(&self, dpi: f64) -> bool {
        let hard = |a: f64| Self::to_m_s2(a, dpi) > HAND_ACCEL;
        self.steady_timing
            && self.reports >= 3
            && ((hard(self.start_accel) && hard(self.stop_accel)) || hard(self.turn_accel))
    }

    /// The hand moved the mouse: a smooth, straight movement, reported for
    /// at least [`HAND_MIN_MS`], that needed no more than [`HAND_ACCEL`] at
    /// either end, anywhere along it or to turn.
    pub fn hand_like(&self, dpi: f64) -> bool {
        let gentle = |a: f64| Self::to_m_s2(a, dpi) <= HAND_ACCEL;
        self.smooth
            && self.span_ms >= HAND_MIN_MS
            && gentle(self.start_accel)
            && gentle(self.stop_accel)
            && gentle(self.stretch_accel)
            && gentle(self.turn_accel)
    }
}

/// The least constant acceleration that covers `path` within `time` when
/// the speed at one end is at most what that acceleration builds up over
/// `prior` (the path covered before, or still to come after). From
/// `path <= sqrt(2 a prior) t + a t^2 / 2`, solved for `a`.
fn least_accel(path: f64, time: f64, prior: f64) -> f64 {
    if path <= 0.0 || time <= 0.0 {
        return 0.0;
    }
    let prior = prior.max(0.0);
    (((2.0 * (prior + path)).sqrt() - (2.0 * prior).sqrt()) / time).powi(2)
}

/// The shape of the movement in `reports` (time in s, counts) that carried
/// the cursor furthest, among those that began by the report at `peak`
/// (the furthest point of the jump window): a re-grip after a jump can
/// carry on in the same direction and hold the furthest point itself.
/// Without `stopped`, the last movement may still have been under way (the
/// test ended first), so how it stopped is not known.
fn analyse(reports: &[(f64, i32, i32)], peak: usize, stopped: bool) -> Shape {
    // (first, last) report of each movement that began by the peak.
    let mut movements = Vec::new();
    let mut first = 0;
    for i in 0..reports.len() {
        if i + 1 == reports.len() || reports[i + 1].0 - reports[i].0 >= STOP_GAP_S {
            if first <= peak {
                movements.push((first, i));
            }
            first = i + 1;
        }
    }
    let reach = |&(a, b): &(usize, usize)| {
        let (x, y) = reports[a..=b]
            .iter()
            .fold((0.0, 0.0), |(x, y), &(_, dx, dy)| {
                (x + dx as f64, y + dy as f64)
            });
        f64::hypot(x, y)
    };
    let Some((a, b)) = movements
        .into_iter()
        .max_by(|m, n| reach(m).total_cmp(&reach(n)))
    else {
        return Shape::default();
    };
    let mut shape = shape_of(&reports[a..=b]);
    if b + 1 == reports.len() && !stopped {
        shape.stop_accel = 0.0;
        shape.stretch_accel = shape.start_accel;
        shape.smooth = false;
    }
    shape
}

/// The poll period (s) of report `intervals`: the typical one among those
/// within [`TIMESTAMP_JITTER_S`] of the shortest, so a report stamped late
/// (which shortens the next interval) does not stand for the poll.
/// Infinite when there are none.
fn poll_of(intervals: &[f64]) -> f64 {
    let shortest = intervals.iter().copied().fold(f64::INFINITY, f64::min);
    let mut close: Vec<f64> = intervals
        .iter()
        .copied()
        .filter(|&i| i <= shortest + 2.0 * TIMESTAMP_JITTER_S)
        .collect();
    close.sort_by(f64::total_cmp);
    close.get(close.len() / 2).copied().unwrap_or(f64::INFINITY)
}

/// The shape of one movement: reports with no pause of [`STOP_GAP_S`].
fn shape_of(seg: &[(f64, i32, i32)]) -> Shape {
    let n = seg.len();
    let t: Vec<f64> = seg.iter().map(|r| r.0).collect();
    // Position after each report, from where the movement began.
    let mut pos = Vec::with_capacity(n);
    let (mut x, mut y) = (0.0, 0.0);
    for &(_, dx, dy) in seg {
        x += dx as f64;
        y += dy as f64;
        pos.push((x, y));
    }
    let dist = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0).hypot(b.1 - a.1);
    let mut shape = Shape {
        reports: n,
        ..Shape::default()
    };
    if n < 2 {
        return shape;
    }

    let intervals: Vec<f64> = t.windows(2).map(|w| w[1] - w[0]).collect();
    let shortest = intervals.iter().copied().fold(f64::INFINITY, f64::min);
    shape.span_ms = (t[n - 1] - t[0] + shortest) * 1000.0;
    let lost = 2.0 * REMAINDER_COUNTS;
    // Whole polls of `unit`, one to [`STEADY_MAX_POLLS`], at a steady time.
    let on_grid = |i: f64, unit: f64| {
        let polls = (i / unit).round();
        (1.0..=STEADY_MAX_POLLS).contains(&polls)
            && (i - polls * unit).abs() <= (STEADY_SPREAD * i).max(TIMESTAMP_JITTER_S)
    };
    let all_poll = poll_of(&intervals);
    // A pause: more polls without a report than a moving mouse skips, so
    // the mouse moved less than a count meanwhile. With bunched times the
    // shortest intervals are no poll, and the typical one stands in for it.
    let poll = if all_poll >= MIN_REPORT_INTERVAL_S {
        all_poll
    } else {
        let mut sorted = intervals.clone();
        sorted.sort_by(f64::total_cmp);
        sorted[sorted.len() / 2]
    };
    let paused = |i: f64| i > (STEADY_MAX_POLLS + 0.5) * poll;
    // A count or two at either end, a pause away from a movement of larger
    // reports, is a resting hand (or the sensor about to find or lose the
    // pad), not part of the movement's timing.
    let size = |i: usize| (seg[i].1 as f64).hypot(seg[i].2 as f64);
    let largest = (0..n).map(size).fold(0.0, f64::max);
    let lone = |i: usize| size(i) <= lost && largest > lost;
    let (mut from, mut to) = (0, n - 1);
    while to - from >= 3 && lone(from) && paused(intervals[from]) {
        from += 1;
    }
    while to - from >= 3 && lone(to) && paused(intervals[to - 1]) {
        to -= 1;
    }
    let timed = &intervals[from..to];
    let unit = poll_of(timed);
    shape.steady_timing =
        to - from >= 2 && unit >= MIN_REPORT_INTERVAL_S && timed.iter().all(|&i| on_grid(i, unit));
    let slack = (STEADY_SPREAD * unit).max(TIMESTAMP_JITTER_S);

    // Start: from rest, the speed after a path s is at most sqrt(2 a s), so
    // the path before the first report bounds the speed it had then.
    let before = (seg[0].1 as f64).hypot(seg[0].2 as f64) + REMAINDER_COUNTS;
    for k in 1..n {
        let path = dist(pos[0], pos[k]) - lost;
        shape.start_accel = shape
            .start_accel
            .max(least_accel(path, t[k] - t[0] + slack, before));
    }
    // Stop: after the last report the mouse moved less than a count, which
    // bounds the speed it still had then.
    for j in 1..n {
        let path = dist(pos[j - 1], pos[n - 1]) - lost;
        shape.stop_accel =
            shape
                .stop_accel
                .max(least_accel(path, t[n - 1] - t[j - 1] + slack, lost));
    }
    // Without the stray counts at its ends, the movement starts from rest
    // after the path they covered and comes to rest within the path they
    // cover after it, at the steady times of the reports between them.
    if shape.steady_timing && (from, to) != (0, n - 1) {
        let before = (0..=from).map(size).sum::<f64>() + REMAINDER_COUNTS;
        let after = (to + 1..n).map(size).sum::<f64>() + lost;
        for k in from + 1..=to {
            let path = dist(pos[from], pos[k]) - lost;
            shape.start_accel =
                shape
                    .start_accel
                    .max(least_accel(path, t[k] - t[from] + slack, before));
            let path = dist(pos[k - 1], pos[to]) - lost;
            shape.stop_accel =
                shape
                    .stop_accel
                    .max(least_accel(path, t[to] - t[k - 1] + slack, after));
        }
    }
    // The same holds for any stretch of the movement: the speed the mouse
    // had at a report is bounded by the path it covered since the movement
    // began, and by the path still to come. A sensor spike among a resting
    // hand's stray counts needs far more than the hand gives, though over
    // the whole movement it can pass for one that sped up and slowed down.
    // Short stretches need steady times, apart from pauses next to a stray
    // count: a pause between two larger reports is more likely a radio link
    // delivering reports in bunches than a mouse that stopped.
    shape.stretch_accel = shape.start_accel.max(shape.stop_accel);
    let trusted = all_poll >= MIN_REPORT_INTERVAL_S
        && intervals
            .iter()
            .enumerate()
            .all(|(b, &i)| on_grid(i, all_poll) || (paused(i) && (lone(b) || lone(b + 1))));
    if trusted {
        // Path covered by the end of each report.
        let walked: Vec<f64> = (0..n)
            .scan(0.0, |path, i| {
                *path += size(i);
                Some(*path)
            })
            .collect();
        for r in 0..n {
            let before = walked[r] + REMAINDER_COUNTS;
            let after = walked[n - 1] - walked[r] + lost;
            let mut m = 1;
            while m < n {
                if r + m < n {
                    let path = dist(pos[r], pos[r + m]) - lost;
                    shape.stretch_accel =
                        shape
                            .stretch_accel
                            .max(least_accel(path, t[r + m] - t[r] + slack, before));
                }
                if m <= r {
                    let path = dist(pos[r - m], pos[r]) - lost;
                    shape.stretch_accel =
                        shape
                            .stretch_accel
                            .max(least_accel(path, t[r] - t[r - m] + slack, after));
                }
                m *= 2;
            }
        }
    }
    // Turning back: the mean velocities of two neighbouring stretches can
    // differ by at most a * (their combined time) / 2. Only stretches more
    // than 90 degrees apart are used, where shrinking either velocity (for
    // the slack in the times) can only shrink the difference.
    for m in [1usize, 2, 4, 8] {
        for b in m..n.saturating_sub(m) {
            let (d1, d2) = (
                (pos[b].0 - pos[b - m].0, pos[b].1 - pos[b - m].1),
                (pos[b + m].0 - pos[b].0, pos[b + m].1 - pos[b].1),
            );
            if d1.0 * d2.0 + d1.1 * d2.1 >= 0.0 {
                continue;
            }
            let (t1, t2) = (t[b] - t[b - m] + slack, t[b + m] - t[b] + slack);
            if t1 <= 0.0 || t2 <= 0.0 {
                continue;
            }
            let change =
                (d2.0 / t2 - d1.0 / t1).hypot(d2.1 / t2 - d1.1 / t1) - lost / t1 - lost / t2;
            if change > 0.0 {
                shape.turn_accel = shape.turn_accel.max(2.0 * change / (t1 + t2));
            }
        }
    }

    // Straight: no point strays far sideways from, or back along, the line
    // from where the movement began to where it ended.
    let end = pos[n - 1];
    let length = end.0.hypot(end.1);
    // Without steady times the bounds above cannot tell stray counts around
    // a spike from a gradual start or stop, so the shape is judged without
    // them.
    let (from, to) = if trusted { (0, n - 1) } else { (from, to) };
    if to - from + 1 < HAND_MIN_REPORTS || length <= 0.0 {
        return shape;
    }
    let (ux, uy) = (end.0 / length, end.1 / length);
    let along: Vec<f64> = pos.iter().map(|p| p.0 * ux + p.1 * uy).collect();
    let stray = STRAY_MAX * length + 1.0;
    let sideways = pos.iter().all(|p| (p.1 * ux - p.0 * uy).abs() <= stray);
    let mut furthest = 0.0f64;
    let back = along.iter().all(|&a| {
        furthest = furthest.max(a);
        furthest - a <= stray
    });
    // Gradual: the speed over the first and the last stretch of about a
    // fifth of the movement stays well under its top speed over any such
    // stretch. Stretches are whole report intervals and end half an
    // interval after a report, so jitter in the times cannot move a report
    // across an end; the first report covers one interval before it.
    let poll = all_poll.max(MIN_REPORT_INTERVAL_S);
    let span = t[to] - t[from] + poll;
    let w = poll * (span / 5.0 / poll).floor().max(1.0);
    let at = |time: f64| {
        let i = t.partition_point(|&r| r <= time);
        if i == 0 {
            0.0
        } else {
            along[i - 1]
        }
    };
    let speed = |until: f64| (at(until) - at(until - w)) / w;
    let first_end = t[from] - poll / 2.0 + w;
    let top = t[from..=to]
        .iter()
        .map(|&r| r + poll / 2.0)
        .filter(|&end| end >= first_end)
        .chain([first_end])
        .map(speed)
        .fold(0.0, f64::max);
    let ramped = top > 0.0
        && speed(first_end) <= RAMP_MAX * top
        && speed(t[to] + poll / 2.0) <= RAMP_MAX * top;
    shape.smooth = sideways && back && ramped;
    shape
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
    /// Reports from the stillness before the burst on, for its shape.
    reports: Vec<(f64, i32, i32)>,
    /// Index in `reports` of the report that reached the furthest point.
    peak_report: usize,
}

impl OpenBurst {
    /// The burst as it stands; `stopped` once the mouse has been still
    /// for a whole idle window after it.
    fn finish(self, stopped: bool) -> Burst {
        Burst {
            peak_dx: self.peak.0,
            peak_dy: self.peak.1,
            peak_counts: self.peak.2,
            peak_ms: self.peak.3 * 1000.0,
            after_window_counts: self.after_window,
            duration_ms: (self.last - self.start) * 1000.0,
            end_s: self.last,
            shape: analyse(&self.reports, self.peak_report, stopped),
        }
    }
}

/// What a [`LiftDetector`] is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiftState {
    /// Started watching while the mouse may have been moving: nothing is
    /// judged until it has been still for a whole idle window.
    Settling,
    /// Still: the next motion starts a burst.
    Still,
    /// A burst is under way.
    Moving,
}

impl LiftState {
    /// Short, for a one-line status display.
    pub fn label(self) -> &'static str {
        match self {
            LiftState::Settling => "waiting for stillness",
            LiftState::Still => "still",
            LiftState::Moving => "moving",
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

    /// The mouse has been still for a whole idle window by `t`.
    fn settle(&mut self, t: f64) {
        if self
            .unsettled_since
            .is_some_and(|since| t - since >= self.idle_s)
        {
            self.unsettled_since = None;
        }
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
            self.current.take().map(|b| b.finish(true))
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
        self.settle(t);

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
                    let reports: Vec<(f64, i32, i32)> = self.recent.iter().copied().collect();
                    self.current = Some(OpenBurst {
                        start: t,
                        last: t,
                        x,
                        y,
                        peak: (x, y, (x as f64).hypot(y as f64), 0.0),
                        after_window: 0.0,
                        peak_report: reports.len() - 1,
                        reports,
                    });
                }
            }
            Some(b) => {
                if b.reports.len() < MAX_SHAPE_REPORTS {
                    b.reports.push((t, dx, dy));
                }
                if t - b.start <= self.window_s {
                    b.x += dx as i64;
                    b.y += dy as i64;
                    let d = (b.x as f64).hypot(b.y as f64);
                    if d > b.peak.2 {
                        b.peak = (b.x, b.y, d, t - b.start);
                        b.peak_report = b.reports.len() - 1;
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
        self.settle(now);
        self.try_finish(now)
    }

    /// End the test: return whatever burst is still open. It may still have
    /// been under way, so it is judged without its stop.
    pub fn flush(&mut self) -> Option<Burst> {
        self.current.take().map(|b| b.finish(false))
    }

    pub fn in_burst(&self) -> bool {
        self.current.is_some()
    }

    /// Settling, still or moving, as of the last report or tick.
    pub fn state(&self) -> LiftState {
        if self.current.is_some() {
            LiftState::Moving
        } else if self.unsettled_since.is_some() {
            LiftState::Settling
        } else {
            LiftState::Still
        }
    }
}

/// Running totals of a lift-off test, as every front-end reports them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiftTally {
    /// Bursts that were jumps no hand could make, in order.
    pub jumps: Vec<Burst>,
    /// Lifts and set-downs that stayed within the threshold.
    pub quiet: usize,
    /// Sliding movements, not judged.
    pub slides: usize,
    /// Largest lift or set-down motion (jumps, possible jumps and quiet
    /// bursts), in counts.
    pub largest_counts: f64,
    /// Jump-sized bursts whose shape could also be the hand, in order.
    pub possible: Vec<Burst>,
    /// Jump-sized bursts the hand made (smooth nudges), not judged.
    pub nudges: usize,
}

impl LiftTally {
    /// Classify a finished burst at `dpi` and add it to the totals.
    pub fn record(&mut self, burst: &Burst, dpi: f64) -> BurstKind {
        let kind = burst.classify(dpi);
        match kind {
            BurstKind::Jump => self.jumps.push(burst.clone()),
            BurstKind::PossibleJump => self.possible.push(burst.clone()),
            BurstKind::Quiet => self.quiet += 1,
            BurstKind::Slide => self.slides += 1,
            BurstKind::Nudge => self.nudges += 1,
        }
        if !matches!(kind, BurstKind::Slide | BurstKind::Nudge) {
            self.largest_counts = self.largest_counts.max(burst.peak_counts);
        }
        kind
    }

    /// Every burst seen: each is motion that started after a pause.
    pub fn bursts(&self) -> usize {
        self.jumps.len() + self.possible.len() + self.quiet + self.slides + self.nudges
    }

    /// Jumps and possible jumps: every burst that moved the cursor further
    /// than the threshold and was not the hand, in the order recorded
    /// within each group (jumps first).
    pub fn jump_sized(&self) -> impl Iterator<Item = &Burst> {
        self.jumps.iter().chain(&self.possible)
    }

    /// Number of jumps and possible jumps.
    pub fn jump_sized_count(&self) -> usize {
        self.jumps.len() + self.possible.len()
    }

    /// Largest jump or possible jump, in counts (0 without any).
    pub fn max_jump_counts(&self) -> f64 {
        self.jump_sized().map(|j| j.peak_counts).fold(0.0, f64::max)
    }

    /// Average jump or possible jump, in counts (0 without any).
    pub fn avg_jump_counts(&self) -> f64 {
        match self.jump_sized_count() {
            0 => 0.0,
            n => self.jump_sized().map(|j| j.peak_counts).sum::<f64>() / n as f64,
        }
    }

    /// Verdict over `lifts` lifts, at least `min_lifts` of them needed
    /// before "no jumps" is a clean result. Only jumps no hand could make
    /// confirm the fault: [`CONFIRMED_JUMPS`] jump-sized bursts that could
    /// also be the hand leave it at "possible", and a single one of either
    /// kind is [`LiftVerdict::OneJump`].
    pub fn verdict(&self, lifts: usize, min_lifts: usize) -> LiftVerdict {
        if self.jumps.len() >= CONFIRMED_JUMPS {
            LiftVerdict::Jumps
        } else if self.jump_sized_count() >= CONFIRMED_JUMPS {
            LiftVerdict::PossibleJumps
        } else {
            verdict(self.jump_sized_count(), lifts, min_lifts)
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
        // 200 counts at 20 counts/ms (1000 Hz) after stillness. Each report
        // is only 20 counts, so the old per-report check never saw it. It
        // runs at 0.3 m/s from the first report to the last and then stops
        // dead: no hand starts and stops the mouse like that.
        let dpi = 1600.0;
        let mut det = LiftDetector::new(dpi);
        let reports: Vec<(f64, i32, i32)> = (0..10).map(|i| (500.0 + i as f64, 20, 0)).collect();
        let bursts = run(&mut det, &reports);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Jump], "{:?}", bursts);
        assert_eq!(bursts[0].peak_dx, 200);
        let mm = counts_to_mm(bursts[0].peak_counts, dpi);
        assert!((mm - 3.175).abs() < 0.01, "{} mm", mm);
        // Half as fast, it may be the hand sliding the mouse as it lifted it
        // (a waking sensor can hide the start of the slide in its first
        // report): still counted, as a possible jump.
        let reports: Vec<(f64, i32, i32)> = (0..20).map(|i| (500.0 + i as f64, 10, 0)).collect();
        let bursts = run(&mut LiftDetector::new(dpi), &reports);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::PossibleJump]);
    }

    #[test]
    fn test_jump_in_few_reports_at_125hz_is_a_possible_jump() {
        // A 125 Hz mouse delivers a 4 mm set-down jump in three reports.
        // 8 ms apart they cannot show whether it started and stopped faster
        // than the mouse landing while it slid would, so it is counted, but
        // only as possible.
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi);
        let bursts = run(&mut det, &[(400.0, 60, 10), (408.0, 50, 5), (416.0, 16, 0)]);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::PossibleJump]);
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
        let reports = [(200.0, 20, 0), (201.0, 20, 0), (202.0, 20, 0)];
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
        // 1600 DPI: a 4 mm set-down jump in 16 ms, then the hand re-grips
        // the mouse (about 1 mm over 150 ms, the same way) before it has
        // been still for a whole idle window. The burst lasts over 200 ms,
        // which a duration-based slide rule dismissed as "sliding"; the
        // re-grip holds the furthest point, but the jump is judged by its
        // own, much larger, movement.
        let dpi = 1600.0;
        let mut det = LiftDetector::new(dpi);
        let mut reports: Vec<(f64, i32, i32)> =
            (0..16).map(|i| (500.0 + i as f64, 16, 2)).collect();
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
        // The test ended while a jump-sized movement was under way: whether
        // it would have stopped dead or slowed down is unknown, so it is no
        // more than a possible jump.
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi);
        for i in 0..10 {
            det.feed(0.5 + i as f64 / 1000.0, 10, 0);
        }
        let b = det.flush().unwrap();
        assert_eq!(b.shape.stop_accel, 0.0);
        assert_eq!(b.classify(dpi), BurstKind::PossibleJump);
        // Once it has been still for an idle window, the same is a jump.
        let mut det = LiftDetector::new(dpi);
        for i in 0..10 {
            det.feed(0.5 + i as f64 / 1000.0, 10, 0);
        }
        assert_eq!(det.tick(0.7).unwrap().classify(dpi), BurstKind::Jump);
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
    fn test_detector_says_when_it_waits_for_stillness() {
        // Watching starts while the hand still moves the mouse: until it has
        // been still for an idle window nothing is judged, and the state
        // says so instead of "still".
        let mut det = LiftDetector::new(800.0).watching_from(0.0);
        assert_eq!(det.state(), LiftState::Settling);
        for i in 0..20 {
            det.feed(0.01 + i as f64 * 0.002, 4, 0);
        }
        assert_eq!(det.state(), LiftState::Settling);
        assert!(det.tick(0.1).is_none());
        assert_eq!(det.state(), LiftState::Settling);
        // A whole idle window without motion, even with no report since.
        assert!(det.tick(0.05 + IDLE_THRESHOLD_MS as f64 / 1000.0).is_none());
        assert_eq!(det.state(), LiftState::Still);
        assert_eq!(det.state().label(), "still");
        det.feed(0.5, 30, 0);
        assert_eq!(det.state(), LiftState::Moving);
        assert_eq!(LiftState::Settling.label(), "waiting for stillness");
        // Without `watching_from` the mouse is taken to be still at once.
        assert_eq!(LiftDetector::new(800.0).state(), LiftState::Still);
    }

    #[test]
    fn test_tally_counts_each_kind() {
        let dpi = 800.0;
        let mut det = LiftDetector::new(dpi);
        // A jump, a quiet set-down, a slide, a possible jump (125 Hz) and a
        // smooth hand nudge, each after a pause.
        let mut reports: Vec<(f64, i32, i32)> = (0..8).map(|i| (500.0 + i as f64, 12, 4)).collect();
        reports.extend([(1500.0, 3, 1), (1502.0, 4, 0), (1504.0, 3, -1)]);
        reports.extend((0..60).map(|i| (2500.0 + i as f64 * 8.0, 40, 0)));
        reports.extend([(4000.0, 60, 10), (4008.0, 50, 5), (4016.0, 16, 0)]);
        reports.extend(nudge(5000.0, 1000.0, dpi, 3.0, 60.0, 0.0));
        let mut tally = LiftTally::default();
        let kinds: Vec<BurstKind> = run(&mut det, &reports)
            .iter()
            .map(|b| tally.record(b, dpi))
            .collect();
        assert_eq!(
            kinds,
            vec![
                BurstKind::Jump,
                BurstKind::Quiet,
                BurstKind::Slide,
                BurstKind::PossibleJump,
                BurstKind::Nudge
            ]
        );
        assert_eq!(
            (
                tally.jumps.len(),
                tally.quiet,
                tally.slides,
                tally.possible.len(),
                tally.nudges
            ),
            (1, 1, 1, 1, 1)
        );
        assert_eq!(tally.bursts(), 5);
        assert_eq!(tally.jump_sized_count(), 2);
        // The slide and the nudge are not lifts, so neither is "largest";
        // the possible jump (126 counts) is.
        assert!((tally.largest_counts - 126.89).abs() < 0.01, "{:?}", tally);
        assert_eq!(tally.max_jump_counts(), tally.largest_counts);
        assert!((tally.avg_jump_counts() - (101.19 + 126.89) / 2.0).abs() < 0.01);
    }

    #[test]
    fn test_dpi_or_default() {
        assert_eq!(dpi_or_default(Some(1600.0)), (1600.0, false));
        assert_eq!(dpi_or_default(None), (DEFAULT_DPI, true));
        assert_eq!(dpi_or_default(Some(0.0)), (DEFAULT_DPI, true));
        assert_eq!(dpi_or_default(Some(f64::NAN)), (DEFAULT_DPI, true));
    }

    /// Reports of a mouse polled at `rate_hz` (first poll `phase` of an
    /// interval after `t0_ms`) whose sensor sits at `pos(ms)` (x, y in mm)
    /// from `t0_ms` on, for `dur_ms`. Like a real mouse it reports only
    /// whole counts and carries the remainder.
    fn sensor(
        t0_ms: f64,
        rate_hz: f64,
        phase: f64,
        dpi: f64,
        dur_ms: f64,
        pos: impl Fn(f64) -> (f64, f64),
    ) -> Vec<(f64, i32, i32)> {
        let poll_ms = 1000.0 / rate_hz;
        let (mut rx, mut ry) = (0.0, 0.0);
        let mut last = pos(0.0);
        let mut out = Vec::new();
        let mut t = phase * poll_ms;
        while t <= dur_ms + 50.0 {
            let p = pos(t.min(dur_ms));
            rx += mm_to_counts(p.0 - last.0, dpi);
            ry += mm_to_counts(p.1 - last.1, dpi);
            last = p;
            let (dx, dy) = (rx.trunc() as i32, ry.trunc() as i32);
            if dx != 0 || dy != 0 {
                rx -= dx as f64;
                ry -= dy as f64;
                out.push((t0_ms + t, dx, dy));
            }
            t += poll_ms;
        }
        out
    }

    /// A hand nudge: `mm` in `ms` along a slant, from rest to rest with the
    /// minimum-jerk profile of a practised hand movement.
    fn nudge(
        t0_ms: f64,
        rate_hz: f64,
        dpi: f64,
        mm: f64,
        ms: f64,
        phase: f64,
    ) -> Vec<(f64, i32, i32)> {
        sensor(t0_ms, rate_hz, phase, dpi, ms, |t| {
            let tau = (t / ms).clamp(0.0, 1.0);
            let d = mm * (10.0 * tau.powi(3) - 15.0 * tau.powi(4) + 6.0 * tau.powi(5));
            (d * 0.95, d * 0.31)
        })
    }

    #[test]
    fn test_hand_nudges_are_not_jumps() {
        // 1-5 mm nudged in 40-200 ms, at every common polling rate and DPI:
        // the hand speeds the mouse up and slows it down, so none is a jump
        // or a possible jump (the short ones are quiet, the long ones
        // slides) - except that at 125 Hz a 40 ms nudge can come in four
        // reports, too few to show its shape: never a jump, but possible.
        for rate in [125.0, 250.0, 500.0, 1000.0, 8000.0] {
            for dpi in [400.0, 800.0, 1600.0, 3200.0, 16000.0] {
                for (mm, ms) in [
                    (1.0, 40.0),
                    (2.5, 40.0),
                    (5.0, 40.0),
                    (2.5, 60.0),
                    (3.0, 100.0),
                    (5.0, 200.0),
                ] {
                    for phase in [0.0, 0.25, 0.5, 0.75] {
                        let reports = nudge(500.0, rate, dpi, mm, ms, phase);
                        let bursts = run(&mut LiftDetector::new(dpi), &reports);
                        let k = kinds(&bursts, dpi);
                        assert_eq!(
                            k.len(),
                            1,
                            "{rate} Hz {dpi} DPI {mm} mm/{ms} ms: {:?}",
                            bursts
                        );
                        let hand =
                            matches!(k[0], BurstKind::Nudge | BurstKind::Quiet | BurstKind::Slide);
                        let too_few = bursts[0].shape.reports < HAND_MIN_REPORTS;
                        assert!(
                            hand || (rate == 125.0
                                && ms == 40.0
                                && too_few
                                && k[0] == BurstKind::PossibleJump),
                            "{rate} Hz {dpi} DPI {mm} mm/{ms} ms phase {phase}: {:?} {:?}",
                            k,
                            bursts[0].shape
                        );
                    }
                }
                // The 3 mm nudge in 60 ms was a "jump" before shapes were
                // judged.
                let bursts = run(
                    &mut LiftDetector::new(dpi),
                    &nudge(500.0, rate, dpi, 3.0, 60.0, 0.0),
                );
                assert_eq!(
                    kinds(&bursts, dpi),
                    vec![BurstKind::Nudge],
                    "{rate} Hz {dpi} DPI"
                );
            }
        }
    }

    #[test]
    fn test_sensor_spikes_are_jumps() {
        // A sensor going out of focus reports 3-6 mm in 6-10 ms and stops
        // dead, at 500 Hz and up (three reports or more): no hand starts and
        // stops the mouse that fast. Straight or turning back on itself.
        for rate in [500.0, 1000.0, 2000.0, 8000.0] {
            for dpi in [400.0, 800.0, 1600.0, 3200.0, 16000.0] {
                for (mm, ms) in [(3.0, 6.0), (4.0, 10.0), (6.0, 8.0)] {
                    let straight = sensor(500.0, rate, 0.3, dpi, ms, |t| {
                        let d = mm * (t / ms).clamp(0.0, 1.0);
                        (d * 0.8, -d * 0.6)
                    });
                    let bursts = run(&mut LiftDetector::new(dpi), &straight);
                    assert_eq!(
                        kinds(&bursts, dpi),
                        vec![BurstKind::Jump],
                        "{rate} Hz {dpi} DPI {mm} mm/{ms} ms: {:?}",
                        bursts
                    );
                }
                // Out 4 mm and most of the way back within 8 ms.
                let back = sensor(500.0, rate, 0.3, dpi, 8.0, |t| {
                    let d = if t <= 4.0 { t } else { 4.0 - 0.75 * (t - 4.0) };
                    (d, 0.0)
                });
                let bursts = run(&mut LiftDetector::new(dpi), &back);
                assert_eq!(
                    kinds(&bursts, dpi),
                    vec![BurstKind::Jump],
                    "{rate} Hz {dpi} DPI"
                );
            }
        }
    }

    /// `reports` with host timestamp noise: each report stamped up to
    /// `late_us` late (`spread`: false) or up to `late_us` either way.
    fn jittered(
        reports: &[(f64, i32, i32)],
        late_us: f64,
        spread: bool,
        seed: u64,
    ) -> Vec<(f64, i32, i32)> {
        let mut state = seed;
        let mut unit = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        reports
            .iter()
            .map(|&(t, dx, dy)| {
                let u = unit();
                let late = if spread { 2.0 * u - 1.0 } else { u };
                (t + late * late_us / 1000.0, dx, dy)
            })
            .collect()
    }

    #[test]
    fn test_spikes_stay_jumps_with_host_timestamp_noise() {
        // Host timestamps at 4-8 kHz wander by tens of microseconds (USB
        // interrupt moderation, the interrupt handler's delay): up to 20 us
        // either way, or up to 60 us late. A sensor spike is still a jump.
        for rate in [4000.0, 8000.0] {
            for dpi in [800.0, 1600.0, 3200.0] {
                for (mm, ms) in [(3.0, 6.0), (4.0, 10.0), (6.0, 8.0)] {
                    let spike = sensor(500.0, rate, 0.3, dpi, ms, |t| {
                        let d = mm * (t / ms).clamp(0.0, 1.0);
                        (d * 0.8, -d * 0.6)
                    });
                    for (seed, (late_us, spread)) in
                        [(20.0, true), (60.0, false)].into_iter().enumerate()
                    {
                        let reports = jittered(&spike, late_us, spread, seed as u64 + 1);
                        let bursts = run(&mut LiftDetector::new(dpi), &reports);
                        assert_eq!(
                            kinds(&bursts, dpi),
                            vec![BurstKind::Jump],
                            "{rate} Hz {dpi} DPI {mm} mm/{ms} ms {late_us} us: {:?}",
                            bursts[0].shape
                        );
                    }
                }
                // A hand nudge with the same noise is still the hand
                for (mm, ms) in [(2.5, 40.0), (3.0, 60.0), (3.0, 100.0)] {
                    let nudge = nudge(500.0, rate, dpi, mm, ms, 0.25);
                    for (late_us, spread) in [(20.0, true), (60.0, false)] {
                        let reports = jittered(&nudge, late_us, spread, 9);
                        let bursts = run(&mut LiftDetector::new(dpi), &reports);
                        let k = kinds(&bursts, dpi);
                        assert!(
                            matches!(
                                k[..],
                                [BurstKind::Nudge | BurstKind::Quiet | BurstKind::Slide]
                            ),
                            "{rate} Hz {dpi} DPI {mm} mm/{ms} ms {late_us} us: {:?} {:?}",
                            k,
                            bursts[0].shape
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_turning_back_at_speed_is_a_jump() {
        // Speeds up and slows down as gently as a hand, but turns straight
        // back at 0.28 m/s halfway: the direction flips within a report,
        // which no hand does.
        let dpi = 1600.0;
        let f = |tau: f64| 10.0 * tau.powi(3) - 15.0 * tau.powi(4) + 6.0 * tau.powi(5);
        let reports = sensor(500.0, 1000.0, 0.3, dpi, 40.0, |t| {
            let x = if t <= 20.0 {
                6.0 * f(t / 40.0)
            } else {
                6.0 - 6.0 * f(t / 40.0)
            };
            (x, 0.0)
        });
        let bursts = run(&mut LiftDetector::new(dpi), &reports);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Jump], "{:?}", bursts);
        let shape = bursts[0].shape;
        let m_s2 = |a: f64| counts_to_mm(a, dpi) / 1000.0;
        assert!(m_s2(shape.start_accel) <= HAND_ACCEL, "{:?}", shape);
        assert!(m_s2(shape.turn_accel) > HAND_ACCEL, "{:?}", shape);
    }

    #[test]
    fn test_ambiguous_jumps_stay_possible() {
        let dpi = 1600.0;
        // One or two reports cannot show how fast the motion came or went,
        // at any polling rate: a jump, but only a possible one.
        for reports in [vec![(500.0, 120, 40)], vec![(500.0, 80, 0), (501.0, 80, 0)]] {
            let bursts = run(&mut LiftDetector::new(dpi), &reports);
            assert_eq!(kinds(&bursts, dpi), vec![BurstKind::PossibleJump]);
        }
        // A sensor spike at 125 Hz: 8 ms apart, three reports look like a
        // quick hand movement.
        let spike = sensor(500.0, 125.0, 0.3, dpi, 5.0, |t| {
            (4.0 * (t / 5.0).min(1.0), 0.0)
        });
        let bursts = run(&mut LiftDetector::new(dpi), &spike);
        assert_eq!(
            kinds(&bursts, dpi),
            vec![BurstKind::PossibleJump],
            "{:?}",
            bursts
        );
        // The mouse lands while still sliding at 0.3 m/s and friction stops
        // it in 30 ms: the motion starts at full speed (the sensor picked it
        // up) but the hand stops it gently. It could be the hand, or a
        // sensor tracking while still in the air. (At 125 Hz a partial first
        // report can make the start look gradual too, and it passes for the
        // hand, which it is.)
        for rate in [500.0, 1000.0, 8000.0] {
            let landing = sensor(500.0, rate, 0.3, dpi, 30.0, |t| {
                let t = t.min(30.0) / 1000.0;
                ((0.3 * t - 5.0 * t * t) * 1000.0, 0.0)
            });
            let bursts = run(&mut LiftDetector::new(dpi), &landing);
            assert_eq!(
                kinds(&bursts, dpi),
                vec![BurstKind::PossibleJump],
                "{rate} Hz: {:?}",
                bursts
            );
        }
        // The hand slides the mouse as it lifts it: a gentle start, cut off
        // at full speed when the sensor loses the pad.
        let lifting = sensor(500.0, 1000.0, 0.3, dpi, 50.0, |t| {
            let tau = (t.min(30.0) / 50.0).clamp(0.0, 1.0);
            (
                6.0 * (10.0 * tau.powi(3) - 15.0 * tau.powi(4) + 6.0 * tau.powi(5)),
                0.0,
            )
        });
        let bursts = run(&mut LiftDetector::new(dpi), &lifting);
        assert_eq!(
            kinds(&bursts, dpi),
            vec![BurstKind::PossibleJump],
            "{:?}",
            bursts
        );
    }

    #[test]
    fn test_spike_among_stray_counts_is_a_jump() {
        // 1000 Hz, 3200 DPI: a 3 mm sensor spike in 6 ms, with a resting
        // hand's stray counts 15 and 8 ms before it and 5 and 13 ms after.
        // Taken as one movement it sped up and slowed down gently enough
        // for a hand nudge, and was not judged.
        let dpi = 3200.0;
        let mut reports = vec![(485.0, 1, 0), (492.0, 0, 1)];
        reports.extend((0..6).map(|i| (500.0 + i as f64, 63, 0)));
        reports.extend([(510.0, 1, 0), (518.0, 0, -1)]);
        let bursts = run(&mut LiftDetector::new(dpi), &reports);
        assert_eq!(kinds(&bursts, dpi), vec![BurstKind::Jump], "{:?}", bursts);
        // One stray count 5-19 ms before or after a spike (1000 Hz, 1600
        // DPI, 3 mm in 8 ms) made it no more than a possible jump.
        let dpi = 1600.0;
        let spike: Vec<(f64, i32, i32)> = (0..8).map(|i| (500.0 + i as f64, 24, 0)).collect();
        for gap in [5.0, 8.0, 12.0, 16.0, 19.0] {
            let mut before = vec![(500.0 - gap, 0, 1)];
            before.extend(&spike);
            let mut after = spike.clone();
            after.push((507.0 + gap, 0, -1));
            for reports in [before, after] {
                let bursts = run(&mut LiftDetector::new(dpi), &reports);
                assert_eq!(
                    kinds(&bursts, dpi),
                    vec![BurstKind::Jump],
                    "{gap} ms: {:?}",
                    bursts
                );
            }
        }
        // A hand nudge with stray counts around it is still the hand.
        for dpi in [800.0, 3200.0] {
            let mut reports = nudge(500.0, 1000.0, dpi, 3.0, 60.0, 0.0);
            let (first, last) = (reports[0].0, reports[reports.len() - 1].0);
            reports.insert(0, (first - 9.0, 0, 1));
            reports.push((last + 12.0, 1, 0));
            let bursts = run(&mut LiftDetector::new(dpi), &reports);
            assert_eq!(
                kinds(&bursts, dpi),
                vec![BurstKind::Nudge],
                "{dpi} DPI: {:?}",
                bursts
            );
        }
    }

    #[test]
    fn test_bunched_report_times_never_prove_a_jump() {
        // A hand nudge whose reports a Bluetooth link or a stalled host
        // delivered in bunches: the times cannot be trusted, so even though
        // the bunches look abrupt it is not called a jump.
        let dpi = 800.0;
        let reports: Vec<(f64, i32, i32)> = nudge(500.0, 1000.0, dpi, 4.0, 40.0, 0.0)
            .into_iter()
            .map(|(t, dx, dy)| ((t / 15.0).ceil() * 15.0 + (t % 15.0) * 0.01, dx, dy))
            .collect();
        let bursts = run(&mut LiftDetector::new(dpi), &reports);
        assert!(!bursts[0].shape.steady_timing, "{:?}", bursts[0].shape);
        assert_ne!(bursts[0].classify(dpi), BurstKind::Jump);
        // An 8000 Hz spike whose reports a stalled host passed on two at a
        // time, 20 us apart: under half a poll is no timestamp noise
        let spike = sensor(500.0, 8000.0, 0.3, 1600.0, 6.0, |t| {
            (3.0 * (t / 6.0).clamp(0.0, 1.0), 0.0)
        });
        let paired: Vec<(f64, i32, i32)> = spike
            .iter()
            .enumerate()
            .map(|(i, &(t, dx, dy))| (if i % 2 == 0 { t + 0.105 } else { t }, dx, dy))
            .collect();
        let bursts = run(&mut LiftDetector::new(1600.0), &paired);
        assert!(!bursts[0].shape.steady_timing, "{:?}", bursts[0].shape);
        assert_ne!(bursts[0].classify(1600.0), BurstKind::Jump);
    }

    #[test]
    fn test_only_jumps_no_hand_could_make_confirm_the_fault() {
        let burst = |kind: BurstKind| {
            let dpi = 800.0;
            let reports = match kind {
                BurstKind::Jump => (0..8).map(|i| (500.0 + i as f64, 12, 4)).collect(),
                _ => vec![(500.0, 60, 10), (508.0, 50, 5), (516.0, 16, 0)],
            };
            let b = run(&mut LiftDetector::new(dpi), &reports).remove(0);
            assert_eq!(b.classify(dpi), kind);
            b
        };
        let (jump, possible) = (burst(BurstKind::Jump), burst(BurstKind::PossibleJump));
        let tally = |j: usize, p: usize| {
            let mut t = LiftTally::default();
            for _ in 0..j {
                t.record(&jump, 800.0);
            }
            for _ in 0..p {
                t.record(&possible, 800.0);
            }
            t
        };
        assert_eq!(tally(2, 0).verdict(5, MIN_LIFTS), LiftVerdict::Jumps);
        assert_eq!(tally(2, 4).verdict(5, MIN_LIFTS), LiftVerdict::Jumps);
        // Careless set-downs that slid the mouse are no proof of a fault.
        assert_eq!(
            tally(0, 2).verdict(5, MIN_LIFTS),
            LiftVerdict::PossibleJumps
        );
        assert_eq!(
            tally(1, 1).verdict(5, MIN_LIFTS),
            LiftVerdict::PossibleJumps
        );
        assert_eq!(tally(0, 1).verdict(5, MIN_LIFTS), LiftVerdict::OneJump);
        assert_eq!(tally(1, 0).verdict(0, MIN_LIFTS), LiftVerdict::OneJump);
        assert_eq!(tally(0, 0).verdict(4, MIN_LIFTS), LiftVerdict::TooFewLifts);
        assert_eq!(tally(0, 0).verdict(5, MIN_LIFTS), LiftVerdict::Clean);
        // Hand nudges are not jumps, and a clean result can include them.
        let mut t = tally(0, 0);
        let hand = run(
            &mut LiftDetector::new(800.0),
            &nudge(500.0, 1000.0, 800.0, 3.0, 60.0, 0.0),
        );
        assert_eq!(t.record(&hand[0], 800.0), BurstKind::Nudge);
        assert_eq!(t.verdict(5, MIN_LIFTS), LiftVerdict::Clean);
        assert_eq!(
            LiftVerdict::PossibleJumps.label(),
            "Possible jumps - repeat the test"
        );
    }

    #[test]
    fn test_least_accel() {
        // From rest, 1 m in 1 s needs 2 m/s^2.
        assert!((least_accel(1.0, 1.0, 0.0) - 2.0).abs() < 1e-12);
        // Already moving (1 m covered at 1 m/s^2 gives sqrt(2) m/s): less.
        assert!(least_accel(1.0, 1.0, 1.0) < 2.0);
        assert_eq!(least_accel(0.0, 1.0, 0.0), 0.0);
        assert_eq!(least_accel(-1.0, 1.0, 0.0), 0.0);
        assert_eq!(least_accel(1.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn test_unit_conversion() {
        assert!((mm_to_counts(25.4, 800.0) - 800.0).abs() < 1e-9);
        assert!((counts_to_mm(1600.0, 1600.0) - 25.4).abs() < 1e-9);
    }
}
