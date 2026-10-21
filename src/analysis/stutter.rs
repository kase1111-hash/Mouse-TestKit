//! Stutter analysis
//!
//! A stutter is a motion report that arrives late while the mouse should have
//! been reporting at every poll. A mouse only sends a report once a whole
//! count has accumulated, so slow movement, direction reversals and pauses
//! legitimately produce gaps of 2-20 ms (or seconds). Those gaps are not
//! stutters and must not be judged; [`StutterDetector`] only judges intervals
//! between reports that both prove the hand was moving fast enough, in a
//! steady direction, for a report at every poll (see
//! [`crate::analysis::polling::is_saturated_pair`]) and that are not the
//! small reports of a hand slowing to a brief stop (see
//! [`MIN_RELATIVE_REPORT_SIZE`]), compares them against the median of such
//! intervals, and counts each interval exactly once. The same rule finds
//! freezes: gaps longer than a poll between two such reports after which
//! the held-back movement arrives at once (see [`is_freeze`]), or after
//! which the reports go on at full speed with the movement lost, which no
//! hand can do (see [`is_dropout`]). Reports pinned at the largest value
//! the report format holds are counted apart (see [`ClipCheck`]): that is
//! movement lost at high speed, not late reports.
//!
//! [`analyze_stutter`] is the older deviation-from-mean classifier for a plain
//! list of intervals. It knows nothing about pauses or slow movement and is
//! only meaningful for intervals that were already filtered that way.

use std::collections::VecDeque;

use crate::analysis::polling::{
    batched_delivery, is_saturated_pair, is_saturated_report, median_in_place, Batching,
    BATCH_GAP_MS, MAX_POLL_INTERVAL_MS, MIN_SATURATED_COUNTS,
};

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

/// Both reports around a judged interval must be at least this fraction of
/// the median report size of fast movement.
///
/// A hand that slows to a brief stop and carries on in the same direction
/// leaves a gap between two same-direction reports, and both can hold two or
/// more counts: at 125-500 Hz a poll is long enough for an ordinary
/// slow-down to still move two counts in its last poll, and at very high DPI
/// two counts are a tiny distance. Those reports at the end and start of the
/// stop are much smaller than the reports of steady movement. During a real
/// stutter the hand keeps moving: the report before the gap is ordinary and
/// the one after it is ordinary (counts lost, or a catch-up burst) or larger
/// (it carries the counts of the missed polls).
pub const MIN_RELATIVE_REPORT_SIZE: f64 = 0.25;

/// Longest gap (ms) judged as a freeze (see [`is_freeze`]). A longer one is
/// left as a pause: the cursor stopping for over a second is plain to see
/// without a test, and Auto Diagnostics notes such pauses separately.
pub const MAX_FREEZE_MS: f64 = 1000.0;

/// The report after a freeze carries at least this many median reports'
/// worth of counts (see [`is_freeze`]).
pub const FREEZE_CARRY_REPORTS: f64 = 3.0;

/// Whether a gap between two reports of a saturated pair, longer than
/// [`MAX_POLL_INTERVAL_MS`] and at most [`MAX_FREEZE_MS`], is a freeze: the
/// reports stopped while the hand kept moving the same way, as when a
/// wireless link, a hub or a cable drops out.
///
/// `smaller` is the size (counts) of the smaller of the two reports, `after`
/// the size of the report after the gap, and `median_size` the median report
/// size of fast movement. The mouse keeps counting while its reports cannot
/// get through and sends the held-back movement once they can, so the
/// report after a freeze carries the movement of the missed polls (at least
/// [`FREEZE_CARRY_REPORTS`] median reports; the cursor freezes, then jumps).
/// A hand that stops and starts again sends nothing for the pause and then
/// an ordinary report, even when it stops within a poll or two, as a slow
/// hand often does at 125-250 Hz; a mouse lifted while moving does not count
/// at all while it is in the air. Neither is a freeze.
pub fn is_freeze(interval_ms: f64, smaller: f64, after: f64, median_size: f64) -> bool {
    interval_ms > MAX_POLL_INTERVAL_MS
        && interval_ms <= MAX_FREEZE_MS
        && smaller >= MIN_RELATIVE_REPORT_SIZE * median_size
        && after >= FREEZE_CARRY_REPORTS * median_size
}

/// A hand needs at least this long (ms) to bring the mouse from its brisk
/// cruising speed to rest, or from rest back up to it. Muscle force builds
/// up over tens of milliseconds, and even the sharpest flicks change their
/// acceleration no faster than about 6000 m/s^3, which takes 4 ms to reach
/// a slow 5 cm/s from rest (and as long to lose it).
pub const MIN_HAND_RAMP_MS: f64 = 4.0;

/// [`is_dropout`] is only judged when a hand's slow-down or speed-up spans
/// at least this many polls ([`MIN_HAND_RAMP_MS`] at 1 kHz and up). Then at
/// least one of the last two reports before a stop, and of the first two
/// after it, is clearly smaller than the reports of steady movement. At
/// 125-500 Hz a hand can stop within a poll or two, so the reports either
/// side of a stop can be full-size.
const RAMP_POLLS: f64 = 3.0;

/// Two consecutive reports holding at least this share of two median
/// reports of fast movement were sent at cruising speed (see
/// [`is_dropout`]). Taken over two reports, because the counts of one
/// report are rounded down and the remainder goes into the next: at a few
/// counts a poll single reports vary by a third.
pub const FULL_SPEED_SIZE: f64 = 0.75;

/// ... and neither report is smaller than this share of the median (a
/// large report followed by a small one is a hand slowing down).
const MIN_EDGE_SIZE: f64 = 0.5;

/// The four reports around a gap between the two reports of a saturated
/// pair: the two before the gap and the two after it, each pair also
/// saturated (see [`GapEdgeTracker`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GapEdges {
    /// The gap (ms): from the last report before it to the first after it.
    pub gap_ms: f64,
    /// Sizes (counts) of the two reports before the gap and of the two
    /// after it, in time order.
    pub sizes: [f64; 4],
    /// Intervals (ms) between the two reports before the gap and between
    /// the two after it.
    pub edge_ms: [f64; 2],
}

/// Whether a gap longer than [`MAX_POLL_INTERVAL_MS`] and at most
/// [`MAX_FREEZE_MS`] is a dropout that lost the movement: the reports
/// stopped while the mouse moved at full speed, and went on at full speed
/// (in any direction, see [`turned_gap`]).
///
/// `period_ms` is the poll period (the median interval of fast movement) and
/// `median_size` the median report size of fast movement. The two reports
/// before the gap and the two after it each move at least [`FULL_SPEED_SIZE`]
/// of the median per poll, each pair one poll apart. A hand cannot do that: it
/// slows down before it stops and speeds up again after it, over at least
/// [`MIN_HAND_RAMP_MS`], which shows in those reports once the poll period
/// is short enough ([`RAMP_POLLS`]). A link that drops reports without
/// sending their movement later, or a sensor that loses track of the
/// surface, does exactly that (see [`is_freeze`] for a mouse that holds the
/// movement back instead). A mouse lifted while moving fast and set down
/// still moving looks the same.
///
/// "One poll apart" is at least half a poll: reports that stopped reaching
/// the program while the computer was busy (Raw Input is stamped when the
/// program receives it) arrive back to back once it catches up, carrying
/// all their movement. That is a stall of the computer, not of the mouse.
pub fn is_dropout(edges: &GapEdges, period_ms: f64, median_size: f64) -> bool {
    let one_poll = |ms: f64| ms >= 0.5 * period_ms && ms < 2.0 * period_ms;
    period_ms > 0.0
        && period_ms * RAMP_POLLS <= MIN_HAND_RAMP_MS
        && edges.gap_ms > MAX_POLL_INTERVAL_MS
        && edges.gap_ms <= MAX_FREEZE_MS
        && edges
            .sizes
            .iter()
            .all(|&size| size >= MIN_EDGE_SIZE * median_size)
        && edges
            .sizes
            .chunks(2)
            .all(|pair| pair[0] + pair[1] >= 2.0 * FULL_SPEED_SIZE * median_size)
        && edges.edge_ms.iter().all(|&ms| one_poll(ms))
}

/// Whether two consecutive motion reports that do not point the same way
/// are still a gap to judge with [`is_dropout`]: longer than a poll, and
/// both large enough to show the mouse moving. What no hand can do is stop
/// and set off again without slowing down and speeding up, whichever way it
/// sets off; and a hand circling once a second keeps turning through a
/// dropout, by 90 degrees in a quarter of a second.
pub fn turned_gap(prev: (i32, i32), next: (i32, i32), interval_ms: f64) -> bool {
    interval_ms > MAX_POLL_INTERVAL_MS
        && is_saturated_report(prev.0, prev.1)
        && is_saturated_report(next.0, next.1)
}

/// Collects the reports around each gap longer than a poll, for
/// [`is_dropout`]. Feed it every saturated pair of consecutive motion
/// reports and every [`turned_gap`], numbered by the later report.
///
/// Reports are matched by number, so a pair that is not fed (not saturated,
/// or after a reset of the caller's previous report) breaks the chain
/// instead of joining reports that were not neighbours.
#[derive(Debug, Clone, Default)]
pub struct GapEdgeTracker {
    /// The last pair no longer than a poll: number of its later report, size
    /// of its earlier one and its interval (ms).
    last_pair: Option<(usize, f64, f64)>,
    /// A gap waiting for the pair after it: number of the report that ended
    /// the gap, and the edges known so far.
    open: Option<(usize, GapEdges)>,
}

impl GapEdgeTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that motion reports `index - 1` and `index`, `interval_ms`
    /// apart, holding `before` and `after` counts, form a saturated pair.
    /// Returns a gap whose four reports are now known, with the number of the
    /// report that ended it.
    pub fn pair(
        &mut self,
        index: usize,
        interval_ms: f64,
        before: f64,
        after: f64,
    ) -> Option<(usize, GapEdges)> {
        if interval_ms <= MAX_POLL_INTERVAL_MS {
            let done =
                self.open
                    .take()
                    .filter(|&(gap, _)| gap + 1 == index)
                    .map(|(gap, mut edges)| {
                        edges.sizes[3] = after;
                        edges.edge_ms[1] = interval_ms;
                        (gap, edges)
                    });
            self.last_pair = Some((index, before, interval_ms));
            done
        } else {
            self.open = self
                .last_pair
                .filter(|&(i, ..)| i + 1 == index && interval_ms <= MAX_FREEZE_MS)
                .map(|(_, first, edge_ms)| {
                    let edges = GapEdges {
                        gap_ms: interval_ms,
                        sizes: [first, before, after, 0.0],
                        edge_ms: [edge_ms, 0.0],
                    };
                    (index, edges)
                });
            self.last_pair = None;
            None
        }
    }

    /// Forget the gap ended by report `index` (it was already judged).
    pub fn cancel(&mut self, index: usize) {
        if self.open.is_some_and(|(gap, _)| gap == index) {
            self.open = None;
        }
    }
}

/// Largest |dx| or |dy| a mouse report can carry with 8-bit, 12-bit (used by
/// many wireless receivers) and 16-bit motion fields. A signed field also
/// reaches one more on the negative side (-128, -2048, -32768).
pub const REPORT_LIMITS: [i32; 3] = [127, 2047, 32767];

/// Clipping is reported once at least this many fast reports...
const MIN_CLIPPED_REPORTS: usize = 10;
/// ... and this share of them sit exactly at a limit...
const MIN_CLIPPED_SHARE: f64 = 0.02;
/// ... at least this many times as often as at each of the
/// [`CLIP_BAND`] values just below it, with none beyond it. A mouse whose
/// reports run into the limit piles every faster report onto that one value;
/// a mouse that merely moves at about that speed spreads its reports over
/// the neighbouring values too, and sometimes past the limit.
const CLIP_PILE_UP: f64 = 4.0;
const CLIP_BAND: i64 = 8;

/// Fast reports pinned at a report format's limit (see [`ClipCheck`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clipping {
    /// The limit (counts) the reports were pinned at.
    pub limit: i32,
    /// Fast reports with an axis at the limit...
    pub pinned: usize,
    /// ... out of this many fast reports (see
    /// [`crate::analysis::polling::MIN_SATURATED_COUNTS`]).
    pub fast_reports: usize,
}

impl Clipping {
    pub fn percent(&self) -> f64 {
        self.pinned as f64 / self.fast_reports.max(1) as f64 * 100.0
    }
}

/// Finds motion reports pinned at the largest value the mouse's report
/// format can carry ([`REPORT_LIMITS`]).
///
/// Moved faster than its reports can describe, a mouse sends the limit in
/// every report and loses the rest of the movement (or sends it later, so
/// the cursor lags behind): fast movements come up short. That is a fault
/// at high speed, not late reports. Feed it every motion report.
#[derive(Debug, Clone, Default)]
pub struct ClipCheck {
    fast_reports: usize,
    /// Largest and smallest dx or dy seen in any report.
    highest: i64,
    lowest: i64,
    /// Per limit: fast reports with an axis at the limit...
    at_limit: [usize; 3],
    /// ... and fast reports whose larger axis lies just below it.
    below_limit: [usize; 3],
}

impl ClipCheck {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, dx: i32, dy: i32) {
        let (dx, dy) = (i64::from(dx), i64::from(dy));
        self.highest = self.highest.max(dx).max(dy);
        self.lowest = self.lowest.min(dx).min(dy);
        let larger = dx.abs().max(dy.abs());
        if larger < i64::from(MIN_SATURATED_COUNTS) {
            return;
        }
        self.fast_reports += 1;
        for (k, &limit) in REPORT_LIMITS.iter().enumerate() {
            let limit = i64::from(limit);
            // A signed field holds -(limit + 1)..=limit; firmware that
            // clamps usually stops at -limit
            let pinned = |v: i64| v == limit || v == -limit || v == -limit - 1;
            if pinned(dx) || pinned(dy) {
                self.at_limit[k] += 1;
            } else if (limit - CLIP_BAND..limit).contains(&larger) {
                self.below_limit[k] += 1;
            }
        }
    }

    /// The format limit the fast reports were pinned at, if any.
    pub fn clipping(&self) -> Option<Clipping> {
        REPORT_LIMITS.iter().enumerate().find_map(|(k, &limit)| {
            let pinned = self.at_limit[k];
            let per_value_below = self.below_limit[k] as f64 / CLIP_BAND as f64;
            let fits = self.highest <= i64::from(limit) && self.lowest >= -i64::from(limit) - 1;
            (fits
                && pinned >= MIN_CLIPPED_REPORTS
                && pinned as f64 >= MIN_CLIPPED_SHARE * self.fast_reports as f64
                && pinned as f64 >= CLIP_PILE_UP * per_value_below)
                .then_some(Clipping {
                    limit,
                    pinned,
                    fast_reports: self.fast_reports,
                })
        })
    }
}

/// This many freezes in a session are a fault whatever the stutter rate: a
/// moment of radio interference can explain one, not a repeat.
pub const CONFIRMED_FREEZES: usize = 2;

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
/// motion in a steady direction) and neither is much smaller than the median
/// report of such movement (see [`MIN_RELATIVE_REPORT_SIZE`]). A judged
/// interval longer than [`stutter_threshold_ms`]`(median, multiplier)` is a
/// stutter. Only intervals up to [`MAX_POLL_INTERVAL_MS`] shape the median;
/// a longer interval is judged only when it is a freeze (see [`is_freeze`]
/// and [`is_dropout`]): otherwise it is the hand stopping or the mouse being
/// lifted. Intervals over [`MAX_FREEZE_MS`] are not judged. When reports
/// arrive several at a time (see
/// [`crate::analysis::polling::batched_delivery`]) the median is the
/// interval between deliveries and the gaps inside a delivery are not
/// judged.
#[derive(Debug, Clone)]
pub struct StutterDetector {
    multiplier: f64,
    /// Previous motion report: (time in seconds, dx, dy).
    last: Option<(f64, i32, i32)>,
    reports: usize,
    /// Most recent judged-candidate intervals (ms) for the baseline median.
    recent: VecDeque<f64>,
    median_ms: Option<f64>,
    /// Sizes (counts) of the most recent reports that ended a candidate
    /// interval, for the median report size.
    recent_sizes: VecDeque<f64>,
    median_size: Option<f64>,
    since_refresh: usize,
    /// Intervals seen before the baseline existed: (report index, ms, size
    /// of the smaller of its two reports, size of the later one).
    pending: Vec<(usize, f64, f64, f64)>,
    judged: usize,
    judged_sum_ms: f64,
    judged_min_ms: f64,
    judged_max_ms: f64,
    stutter_count: usize,
    minor: usize,
    moderate: usize,
    severe: usize,
    freezes: usize,
    /// Reports around gaps longer than a poll, for [`is_dropout`].
    gap_edges: GapEdgeTracker,
    /// Gaps whose four reports were known before the baseline existed:
    /// (number of the report that ended the gap, edges).
    pending_gaps: Vec<(usize, GapEdges)>,
    /// Freezes after which the movement was lost (see [`is_dropout`]).
    dropouts: usize,
    /// Reports arriving several at a time, as of the last baseline.
    batching: Option<Batching>,
    clip: ClipCheck,
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
            recent_sizes: VecDeque::with_capacity(BASELINE_WINDOW),
            median_size: None,
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
            freezes: 0,
            gap_edges: GapEdgeTracker::new(),
            pending_gaps: Vec::new(),
            dropouts: 0,
            batching: None,
            clip: ClipCheck::new(),
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
    /// Returns the interval (ms) since the previous report when it is judged,
    /// or held back to be judged once the baseline exists (a gap longer than
    /// a poll only once it is judged a freeze); `None` otherwise. A dropout
    /// (see [`is_dropout`]) is only known one report after the gap: that
    /// report returns the gap instead of its own interval.
    pub fn feed(&mut self, t: f64, dx: i32, dy: i32) -> Option<f64> {
        if dx == 0 && dy == 0 {
            return None;
        }
        self.clip.feed(dx, dy);
        let index = self.reports;
        self.reports += 1;
        let (prev_t, pdx, pdy) = self.last.replace((t, dx, dy))?;
        let interval = (t - prev_t) * 1000.0;
        let saturated = is_saturated_pair((pdx, pdy), (dx, dy));
        if !(saturated || turned_gap((pdx, pdy), (dx, dy), interval))
            || interval <= 0.0
            || interval > MAX_FREEZE_MS
        {
            return None;
        }
        let size = (dx as f64).hypot(dy as f64);
        let prev_size = (pdx as f64).hypot(pdy as f64);
        let smaller = prev_size.min(size);
        let gap_done = self.gap_edges.pair(index, interval, prev_size, size);
        if !saturated {
            // Only a dropout candidate, judged once the next report is in
            return None;
        }

        // A longer gap is a pause or a freeze, judged by the size of its
        // reports; it says nothing about the poll period.
        if interval <= MAX_POLL_INTERVAL_MS {
            self.recent.push_back(interval);
            if self.recent.len() > BASELINE_WINDOW {
                self.recent.pop_front();
            }
            self.recent_sizes.push_back(size);
            if self.recent_sizes.len() > BASELINE_WINDOW {
                self.recent_sizes.pop_front();
            }
            self.since_refresh += 1;
        }

        if self.median_ms.is_none() {
            self.pending.push((index, interval, smaller, size));
            self.pending_gaps.extend(gap_done);
            if self.pending.len() >= BASELINE_MIN_INTERVALS {
                self.refresh_baseline();
                let mut freezes = Vec::new();
                for (i, ms, smaller, after) in std::mem::take(&mut self.pending) {
                    if self.judge(i, ms, smaller, after) && ms > MAX_POLL_INTERVAL_MS {
                        freezes.push(i);
                    }
                }
                for (i, edges) in std::mem::take(&mut self.pending_gaps) {
                    if !freezes.contains(&i) {
                        self.judge_dropout(i, &edges);
                    }
                }
            }
            (interval <= MAX_POLL_INTERVAL_MS).then_some(interval)
        } else {
            if self.since_refresh >= BASELINE_REFRESH {
                self.refresh_baseline();
            }
            let dropout = gap_done
                .filter(|(i, edges)| self.judge_dropout(*i, edges))
                .map(|(_, edges)| edges.gap_ms);
            let judged = self.judge(index, interval, smaller, size);
            dropout.or(judged.then_some(interval))
        }
    }

    fn refresh_baseline(&mut self) {
        let mut values: Vec<f64> = self.recent.iter().copied().collect();
        self.batching = batched_delivery(&values);
        self.median_ms = match self.batching {
            Some(b) => Some(b.event_period_ms),
            None => median_in_place(&mut values),
        };
        let mut sizes: Vec<f64> = self.recent_sizes.iter().copied().collect();
        self.median_size = median_in_place(&mut sizes);
        self.since_refresh = 0;
    }

    /// Judge the gap that report `index` ended, once the reports around it
    /// are known (see [`is_dropout`]). Returns whether it was a dropout.
    fn judge_dropout(&mut self, index: usize, edges: &GapEdges) -> bool {
        let (Some(median), Some(median_size)) = (self.median_ms, self.median_size) else {
            return false;
        };
        if !is_dropout(edges, median, median_size) {
            return false;
        }
        self.freezes += 1;
        self.dropouts += 1;
        self.record(index, edges.gap_ms, median);
        true
    }

    /// Judge one interval whose smaller report holds `smaller` counts and
    /// whose later report holds `after`. Returns whether it was judged (it
    /// is skipped when that report is one of the small reports around a
    /// brief stop, or when a gap longer than a poll is no freeze).
    fn judge(&mut self, index: usize, interval: f64, smaller: f64, after: f64) -> bool {
        let (Some(median), Some(median_size)) = (self.median_ms, self.median_size) else {
            return false;
        };
        if smaller < MIN_RELATIVE_REPORT_SIZE * median_size {
            return false;
        }
        // Reports delivered together say nothing about the link's timing
        if self.batching.is_some() && interval <= BATCH_GAP_MS {
            return false;
        }
        if interval > MAX_POLL_INTERVAL_MS {
            if !is_freeze(interval, smaller, after, median_size) {
                return false;
            }
            self.freezes += 1;
            // Counted once: not again as a dropout
            self.gap_edges.cancel(index);
        }
        self.record(index, interval, median)
    }

    /// Count one judged interval, and the stutter it is if it is late.
    fn record(&mut self, index: usize, interval: f64, median: f64) -> bool {
        self.judged += 1;
        self.judged_sum_ms += interval;
        self.judged_min_ms = self.judged_min_ms.min(interval);
        self.judged_max_ms = self.judged_max_ms.max(interval);
        if interval <= stutter_threshold_ms(median, self.multiplier) {
            return true;
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
        true
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

    /// Polling rate (Hz) implied by the median interval of fast movement
    /// (deliveries per second when reports arrive batched).
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

    /// Freezes: stutters longer than [`MAX_POLL_INTERVAL_MS`] (see
    /// [`is_freeze`] and [`is_dropout`]).
    pub fn freezes(&self) -> usize {
        self.freezes
    }

    /// Freezes after which the movement was lost rather than caught up
    /// (see [`is_dropout`]); included in [`Self::freezes`].
    pub fn dropouts(&self) -> usize {
        self.dropouts
    }

    /// Reports arriving several at a time, one delivery apart (see
    /// [`crate::analysis::polling::batched_delivery`]); the median interval
    /// and polling rate are then per delivery.
    pub fn batching(&self) -> Option<Batching> {
        self.batching
    }

    /// Fast reports pinned at the report format's limit (see [`ClipCheck`]).
    pub fn clipping(&self) -> Option<Clipping> {
        self.clip.clipping()
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

    /// The session's rating. Repeated freezes are visible however long the
    /// session, so they make it at least Fair.
    pub fn rating(&self) -> StutterRating {
        let rating = rate_stutters(self.judged_secs(), self.stutter_count);
        if self.freezes >= CONFIRMED_FREEZES
            && matches!(
                rating,
                StutterRating::Inconclusive | StutterRating::Excellent | StutterRating::Good
            )
        {
            StutterRating::Fair
        } else {
            rating
        }
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

    /// Smooth slow-down / speed-up time around each stop (s).
    const STOP_GO_RAMP: f64 = 0.03;
    const STOP_GO_CRUISE: f64 = 0.3;

    /// Length of the i-th stop: 30-100 ms, varied so the stops fall at
    /// different points of the poll cycle.
    fn stop_go_rest(i: usize) -> f64 {
        0.03 + 0.0037 * i as f64
    }

    /// Steady movement along X at `v` counts/s where the hand smoothly slows
    /// to a brief stop and carries on the same way, 20 times.
    fn stop_and_go(v: f64) -> Vec<Seg> {
        let ramp = STOP_GO_RAMP;
        let mut segs = Vec::new();
        for i in 0..20 {
            segs.push(seg(STOP_GO_CRUISE, move |t| (v * t, 0.0)));
            segs.push(seg(ramp, move |w| {
                let wave = ramp / TAU * (TAU / 2.0 * w / ramp).sin();
                (v * (w / 2.0 + wave), 0.0)
            }));
            segs.push(pause(stop_go_rest(i)));
            segs.push(seg(ramp, move |w| {
                let wave = ramp / TAU * (TAU / 2.0 * w / ramp).sin();
                (v * (w / 2.0 - wave), 0.0)
            }));
        }
        segs
    }

    #[test]
    fn brief_stops_in_the_same_direction_are_not_stutters() {
        // Start of each steady stretch, to place dropouts inside them
        let starts: Vec<f64> = (0..20)
            .scan(0.0, |t, i| {
                let start = *t;
                *t += STOP_GO_CRUISE + 2.0 * STOP_GO_RAMP + stop_go_rest(i);
                Some(start)
            })
            .collect();
        // A hand at about 0.5 m/s with the mouse at 1600, 6400 and 26000 DPI
        for (hz, v) in [(125.0, 30_000.0), (250.0, 120_000.0), (500.0, 500_000.0)] {
            let reports = simulate(hz, stop_and_go(v), no_drops, 30e-6);
            // The scenario really has same-direction pairs of 2+ count
            // reports around a stop, longer than the threshold: what was
            // reported as stutters before reports were compared by size.
            let threshold = stutter_threshold_ms(1000.0 / hz, 2.0);
            let stop_gaps = reports
                .windows(2)
                .filter(|w| {
                    is_saturated_pair((w[0].1, w[0].2), (w[1].1, w[1].2))
                        && (w[1].0 - w[0].0) * 1000.0 > threshold
                })
                .count();
            assert!(stop_gaps >= 5, "{} Hz: {}", hz, stop_gaps);

            let d = run(&reports, 2.0);
            assert_eq!(d.stutter_count(), 0, "{} Hz", hz);
            assert!(d.judged_secs() >= MIN_JUDGED_SECS, "{}", d.judged_secs());
            assert_eq!(d.rating(), StutterRating::Excellent);

            // A 20 ms dropout in the middle of every steady stretch is
            // still caught, once each.
            let drop = |t: f64| starts.iter().any(|&s| (s + 0.15..s + 0.17).contains(&t));
            let d = run(&simulate(hz, stop_and_go(v), drop, 30e-6), 2.0);
            assert_eq!(d.stutter_count(), 20, "{} Hz", hz);
        }
    }

    /// Like [`stop_and_go`], with stops of `rest(i)` seconds.
    fn stop_and_go_with(v: f64, rest: impl Fn(usize) -> f64) -> Vec<Seg> {
        stop_and_go_ramp(v, STOP_GO_RAMP, rest)
    }

    /// Like [`stop_and_go_with`], slowing down and speeding up over `ramp`
    /// seconds.
    fn stop_and_go_ramp(v: f64, ramp: f64, rest: impl Fn(usize) -> f64) -> Vec<Seg> {
        let mut segs = Vec::new();
        for i in 0..20 {
            segs.push(seg(STOP_GO_CRUISE, move |t| (v * t, 0.0)));
            segs.push(seg(ramp, move |w| {
                let wave = ramp / TAU * (TAU / 2.0 * w / ramp).sin();
                (v * (w / 2.0 + wave), 0.0)
            }));
            segs.push(pause(rest(i)));
            segs.push(seg(ramp, move |w| {
                let wave = ramp / TAU * (TAU / 2.0 * w / ramp).sin();
                (v * (w / 2.0 - wave), 0.0)
            }));
        }
        segs
    }

    #[test]
    fn longer_stops_in_the_same_direction_are_not_freezes() {
        // Stops of 0.12-0.88 s, each slowing down first and starting from
        // rest. A hand at about 0.5 m/s with the mouse at 26000 DPI: the
        // reports either side of each stop still carry 2+ counts.
        let rest = |i: usize| 0.12 + 0.04 * i as f64;
        for hz in [125.0, 250.0, 500.0] {
            let v = 500_000.0;
            let reports = simulate(hz, stop_and_go_with(v, rest), no_drops, 30e-6);
            // The stops really leave same-direction pairs of 2+ count reports
            // further apart than a poll
            let long_gaps = reports
                .windows(2)
                .filter(|w| {
                    is_saturated_pair((w[0].1, w[0].2), (w[1].1, w[1].2))
                        && (w[1].0 - w[0].0) * 1000.0 > MAX_POLL_INTERVAL_MS
                })
                .count();
            assert!(long_gaps >= 5, "{} Hz: {}", hz, long_gaps);
            let d = run(&reports, 2.0);
            assert_eq!(d.freezes(), 0, "{} Hz", hz);
            assert_eq!(d.stutter_count(), 0, "{} Hz", hz);
            assert_eq!(d.rating(), StutterRating::Excellent, "{} Hz", hz);
        }
    }

    #[test]
    fn freezes_while_moving_are_detected() {
        // A wireless link that drops out for 150-400 ms every 2 s while the
        // hand keeps circling; the counts carry over to the next report.
        let drop = |t: f64| {
            let k = (t / 2.0).floor();
            let start = k * 2.0 + 1.0;
            t > 0.5 && (start..start + 0.15 + 0.05 * (k % 6.0)).contains(&t)
        };
        for (hz, radius) in [(125.0, 400.0), (1000.0, 600.0)] {
            let reports = simulate(hz, vec![circles(12.0, radius, 1.0)], drop, 30e-6);
            let d = run(&reports, 2.0);
            assert_eq!(d.freezes(), 6, "{} Hz", hz);
            assert_eq!(d.stutter_count(), 6, "{} Hz", hz);
            assert!(d.worst_gap_ms() > 300.0, "{}", d.worst_gap_ms());
            assert_eq!(d.rating(), StutterRating::Fair, "{} Hz", hz);
        }
        // One freeze (a moment of interference) is not a fault
        let one = |t: f64| (5.0..5.3).contains(&t);
        let d = run(
            &simulate(1000.0, vec![circles(12.0, 600.0, 1.0)], one, 30e-6),
            2.0,
        );
        assert_eq!(d.freezes(), 1);
        assert_eq!(d.rating(), StutterRating::Good);
    }

    /// Like [`simulate`], but polls for which `lost(t)` is true are lost
    /// with their movement: the mouse discards what it could not send.
    fn simulate_losing(
        hz: f64,
        segs: Vec<Seg>,
        lost: impl Fn(f64) -> bool,
        noise_s: f64,
    ) -> Vec<(f64, i32, i32)> {
        let (total, pos) = path(segs);
        let mut rng = Lcg(42);
        let (mut rx, mut ry) = (0i64, 0i64);
        let mut out = Vec::new();
        for k in 1..(total * hz) as usize {
            let t = k as f64 / hz;
            let (x, y) = pos(t);
            let dx = (x - rx as f64).trunc() as i64;
            let dy = (y - ry as f64).trunc() as i64;
            rx += dx;
            ry += dy;
            if !lost(t) && (dx != 0 || dy != 0) {
                out.push((t + noise_s * rng.unit(), dx as i32, dy as i32));
            }
        }
        out
    }

    /// A link that drops out for 150-400 ms every 2 s.
    fn every_two_seconds(t: f64) -> bool {
        let k = (t / 2.0).floor();
        let start = k * 2.0 + 1.0;
        t > 0.5 && (start..start + 0.15 + 0.05 * (k % 6.0)).contains(&t)
    }

    #[test]
    fn dropouts_that_lose_the_movement_are_freezes() {
        // Brisk circles while the link (or the sensor's tracking) drops out
        // and the mouse discards the movement: nothing is caught up, but
        // the reports stop and resume at full speed
        // About 4 counts a poll
        for (hz, radius) in [(1000.0, 600.0), (2000.0, 1200.0), (8000.0, 4800.0)] {
            let reports = simulate_losing(
                hz,
                vec![circles(12.0, radius, 1.0)],
                every_two_seconds,
                20e-6,
            );
            let d = run(&reports, DEFAULT_STUTTER_MULTIPLIER);
            assert_eq!(d.freezes(), 6, "{} Hz", hz);
            assert_eq!(d.dropouts(), 6, "{} Hz", hz);
            assert_eq!(d.rating(), StutterRating::Fair, "{} Hz", hz);
        }
        // One is not a fault
        let one = |t: f64| (5.0..5.3).contains(&t);
        let d = run(
            &simulate_losing(1000.0, vec![circles(12.0, 600.0, 1.0)], one, 30e-6),
            2.0,
        );
        assert_eq!((d.freezes(), d.dropouts()), (1, 1));
        assert_eq!(d.rating(), StutterRating::Good);
    }

    #[test]
    fn stops_that_slow_down_and_speed_up_are_not_dropouts() {
        // 0.12-0.88 s stops from about 0.3-0.5 m/s at 400-26000 DPI, slowing
        // down and speeding up over 4-30 ms, at 1-8 kHz: the reports either
        // side of each stop shrink, as a hand's always do
        let rest = |i: usize| 0.12 + 0.04 * i as f64;
        let mut candidates = 0;
        for hz in [1000.0, 2000.0, 4000.0, 8000.0] {
            // 3 and 20 counts a poll, and 0.5 m/s at 26000 DPI
            for v in [3.0 * hz, 20.0 * hz, 500_000.0] {
                for ramp in [0.004, 0.008, 0.03] {
                    let reports = simulate(hz, stop_and_go_ramp(v, ramp, rest), no_drops, 20e-6);
                    // Stops with reports of 2+ counts either side
                    candidates += reports
                        .windows(2)
                        .filter(|w| {
                            turned_gap(
                                (w[0].1, w[0].2),
                                (w[1].1, w[1].2),
                                (w[1].0 - w[0].0) * 1000.0,
                            )
                        })
                        .count();
                    let d = run(&reports, DEFAULT_STUTTER_MULTIPLIER);
                    assert_eq!(d.freezes(), 0, "{} Hz, {} counts/s, {} s", hz, v, ramp);
                    assert_eq!(d.stutter_count(), 0, "{} Hz {} {}", hz, v, ramp);
                }
            }
        }
        // (mostly at 26000 DPI; with fewer counts the slowing hand sends
        // 1-count reports, which end the judging at once)
        assert!(candidates >= 50, "{}", candidates);
        // At 125-500 Hz a hand can stop within a poll or two, so a stop and a
        // dropout that loses the movement look alike: neither is judged
        for hz in [125.0, 250.0, 500.0] {
            let reports = simulate(hz, stop_and_go_ramp(1500.0, 0.008, rest), no_drops, 30e-6);
            assert_eq!(run(&reports, 2.0).freezes(), 0, "{} Hz", hz);
            let lost = simulate_losing(
                hz,
                vec![circles(12.0, 400.0, 1.0)],
                every_two_seconds,
                30e-6,
            );
            assert_eq!(run(&lost, 2.0).freezes(), 0, "{} Hz", hz);
        }
    }

    #[test]
    fn dropouts_before_the_baseline_exists_are_still_judged() {
        // The link drops the movement 30 ms into the session, before 50
        // fast intervals exist, and again later
        let lost = |t: f64| (0.03..0.23).contains(&t) || (5.0..5.25).contains(&t);
        let reports = simulate_losing(1000.0, vec![circles(8.0, 600.0, 1.0)], lost, 20e-6);
        let d = run(&reports, DEFAULT_STUTTER_MULTIPLIER);
        assert_eq!((d.freezes(), d.dropouts()), (2, 2));
        // A dropout is reported by the report after its gap, once
        let mut d = StutterDetector::new(DEFAULT_STUTTER_MULTIPLIER);
        let gaps: Vec<f64> = reports
            .iter()
            .filter_map(|&(t, dx, dy)| d.feed(t, dx, dy))
            .filter(|&ms| ms > MAX_POLL_INTERVAL_MS)
            .collect();
        assert_eq!(gaps.len(), 1, "{:?}", gaps);
        assert!((gaps[0] - 250.0).abs() < 2.0, "{:?}", gaps);
    }

    #[test]
    fn a_held_back_freeze_is_not_counted_again_as_a_dropout() {
        // Fast circles at 1 kHz whose catch-up report is followed by full
        // reports: both rules match, it is one freeze
        let drop = |t: f64| (3.0..3.2).contains(&t) || (7.0..7.3).contains(&t);
        let reports = simulate(1000.0, vec![circles(10.0, 600.0, 1.0)], drop, 20e-6);
        let d = run(&reports, DEFAULT_STUTTER_MULTIPLIER);
        assert_eq!((d.freezes(), d.dropouts()), (2, 0));
        assert_eq!(d.stutter_count(), 2);
    }

    #[test]
    fn a_busy_computer_handing_over_held_up_reports_is_not_a_dropout() {
        // Brisk circles at 1 and 8 kHz. Every 2 s the program does not get
        // the reports for 150-400 ms (Raw Input is stamped on receipt), then
        // takes them all at once, 5 us apart: nothing was lost
        for (hz, radius) in [(1000.0, 600.0), (8000.0, 4800.0)] {
            let reports = simulate(hz, vec![circles(12.0, radius, 1.0)], no_drops, 20e-6);
            let mut held = 0.0;
            let mut last: f64 = 0.0;
            let delivered: Vec<(f64, i32, i32)> = reports
                .iter()
                .map(|&(t, dx, dy)| {
                    if every_two_seconds(t) {
                        let k = (t / 2.0).floor();
                        held = k * 2.0 + 1.0 + 0.15 + 0.05 * (k % 6.0);
                    }
                    last = if t < held { held } else { t }.max(last + 5e-6);
                    (last, dx, dy)
                })
                .collect();
            let d = run(&delivered, DEFAULT_STUTTER_MULTIPLIER);
            assert_eq!((d.freezes(), d.dropouts()), (0, 0), "{} Hz", hz);
            assert_eq!(d.rating(), StutterRating::Excellent, "{} Hz", hz);
        }
    }

    /// Reports of a Bluetooth mouse sampling every `sample_ms` and served
    /// every `event_ms`: each connection event delivers the reports made
    /// since the last one, 0.4 ms apart. Events where `skip(k)` is true are
    /// missed and their reports go out with the next.
    fn bluetooth(
        segs: Vec<Seg>,
        sample_ms: f64,
        event_ms: f64,
        skip: impl Fn(usize) -> bool,
    ) -> Vec<(f64, i32, i32)> {
        let made = simulate(1000.0 / sample_ms, segs, no_drops, 0.0);
        let mut out = Vec::new();
        let mut next = 0;
        let mut k = 1;
        while next < made.len() {
            let event = k as f64 * event_ms / 1000.0;
            if !skip(k) {
                let mut at = event;
                while next < made.len() && made[next].0 <= event {
                    out.push((at, made[next].1, made[next].2));
                    at += 0.0004;
                    next += 1;
                }
            }
            k += 1;
        }
        out
    }

    #[test]
    fn bluetooth_batches_are_not_stutters() {
        // 250 Hz sampling over 7.5-15 ms connection intervals: two to four
        // reports per event, judged per delivery
        for event_ms in [7.5, 11.25, 15.0] {
            let reports = bluetooth(everyday_session(1.0), 4.0, event_ms, |_| false);
            let d = run(&reports, DEFAULT_STUTTER_MULTIPLIER);
            assert!(d.batching().is_some(), "{} ms", event_ms);
            assert_eq!(d.stutter_count(), 0, "{} ms", event_ms);
            assert_eq!(d.rating(), StutterRating::Excellent, "{} ms", event_ms);
            let hz = d.polling_hz().unwrap();
            assert!(
                (hz - 1000.0 / event_ms).abs() < 2.0,
                "{} ms: {} Hz",
                event_ms,
                hz
            );
        }
        // Missing three connection events in a row, every half second, is
        // still a stutter on Bluetooth
        let reports = bluetooth(vec![circles(8.0, 600.0, 1.0)], 4.0, 11.25, |k| k % 45 < 3);
        let d = run(&reports, DEFAULT_STUTTER_MULTIPLIER);
        assert!(d.batching().is_some());
        assert!(d.stutter_count() >= 12, "{}", d.stutter_count());
    }

    /// `reports` with every axis clamped to +-`limit`, as firmware with
    /// that report format does when the mouse moves faster.
    fn clamped(reports: &[(f64, i32, i32)], limit: i32) -> Vec<(f64, i32, i32)> {
        reports
            .iter()
            .map(|&(t, dx, dy)| (t, dx.clamp(-limit, limit), dy.clamp(-limit, limit)))
            .collect()
    }

    fn clip_check(reports: &[(f64, i32, i32)]) -> Option<Clipping> {
        let mut c = ClipCheck::new();
        for &(_, dx, dy) in reports {
            c.feed(dx, dy);
        }
        c.clipping()
    }

    #[test]
    fn reports_pinned_at_the_format_limit_are_clipping() {
        // Brisk circles at 125 Hz and 1600 DPI peak at about 200 counts a
        // poll: an 8-bit mouse sends 127 instead
        let fast = simulate(125.0, vec![circles(8.0, 4000.0, 1.0)], no_drops, 30e-6);
        let c = clip_check(&clamped(&fast, 127)).expect("clipped");
        assert_eq!(c.limit, 127);
        assert!(c.percent() > 20.0, "{:?}", c);
        // ... the same movement from a 16-bit mouse is not
        assert_eq!(clip_check(&fast), None);
        // ... and the stutter rating does not count it
        let d = run(&clamped(&fast, 127), DEFAULT_STUTTER_MULTIPLIER);
        assert_eq!(d.clipping().map(|c| c.limit), Some(127));
        assert_eq!(d.stutter_count(), 0);
        // 12-bit fields at 26000 DPI
        let faster = simulate(125.0, vec![circles(8.0, 60_000.0, 1.0)], no_drops, 30e-6);
        assert_eq!(
            clip_check(&clamped(&faster, 2047)).map(|c| c.limit),
            Some(2047)
        );
        assert_eq!(clip_check(&faster), None);
    }

    #[test]
    fn moving_at_about_the_limit_is_not_clipping() {
        // Circles whose fastest reports are 120-127 counts on an axis, on a
        // mouse with 16-bit reports; and everyday movement
        for radius in [2420.0, 2480.0, 2526.0] {
            let reports = simulate(125.0, vec![circles(12.0, radius, 1.0)], no_drops, 30e-6);
            let largest = reports
                .iter()
                .map(|r| r.1.abs().max(r.2.abs()))
                .max()
                .unwrap();
            assert!((120..=127).contains(&largest), "{}: {}", radius, largest);
            assert_eq!(clip_check(&reports), None, "{}", radius);
        }
        for hz in [125.0, 1000.0, 8000.0] {
            let reports = simulate(hz, everyday_session(4.0), no_drops, 20e-6);
            assert_eq!(clip_check(&reports), None, "{} Hz", hz);
        }
    }

    #[test]
    fn abrupt_stops_of_a_slow_hand_are_not_freezes() {
        // A slow hand at 125-250 Hz stops within a poll or two (friction
        // stops a slowly pushed mouse in milliseconds) and starts again the
        // same way: the reports either side of each 0.12-0.88 s stop are
        // still a good part of the median report, but nothing is held back.
        let rest = |i: usize| 0.12 + 0.04 * i as f64;
        for hz in [125.0, 250.0] {
            for v in [300.0, 650.0, 1500.0] {
                for ramp in [0.008, 0.012, 0.016] {
                    let reports = simulate(hz, stop_and_go_ramp(v, ramp, rest), no_drops, 30e-6);
                    let d = run(&reports, 2.0);
                    assert_eq!(d.freezes(), 0, "{} Hz, {} counts/s, {} s", hz, v, ramp);
                    assert_eq!(d.stutter_count(), 0, "{} Hz, {} counts/s", hz, v);
                }
            }
        }
    }

    #[test]
    fn lifting_the_mouse_while_moving_is_no_catch_up_freeze() {
        // Circling, the mouse is lifted for 150-400 ms every 2 s and set
        // down still moving: the sensor counts nothing in the air, so
        // nothing is held back. At 1 kHz the reports stop and resume at full
        // speed, which a link dropping out also does and a hand stopping
        // never does: it is counted as a dropout (the finding names a lift
        // as a cause). At 125-500 Hz a stop can look the same, and it is
        // not judged.
        for (hz, dropouts) in [(125.0, 0), (250.0, 0), (500.0, 0), (1000.0, 6)] {
            let (_, pos) = path(vec![circles(12.0, 600.0, 1.0)]);
            let mut reports = Vec::new();
            let (mut rx, mut ry) = (0i64, 0i64);
            for k in 1..(12.0 * hz) as usize {
                let t = k as f64 / hz;
                let (x, y) = pos(t);
                let dx = (x - rx as f64).trunc() as i64;
                let dy = (y - ry as f64).trunc() as i64;
                rx += dx;
                ry += dy;
                if !every_two_seconds(t) && (dx != 0 || dy != 0) {
                    reports.push((t, dx as i32, dy as i32));
                }
            }
            let d = run(&reports, 2.0);
            assert_eq!(d.freezes(), dropouts, "{} Hz", hz);
            assert_eq!(d.dropouts(), dropouts, "{} Hz", hz);
        }
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
