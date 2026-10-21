//! Automated diagnostics engine
//!
//! Runs a short, guided sequence of phases and looks for faults on its own:
//!
//! 1. **Rest** - the mouse must not be touched. Any movement, click or scroll
//!    is a phantom event (sensor jitter, drift, a failing switch).
//! 2. **Movement** - continuous motion. Measures the report rate and looks
//!    for stutters (late reports), unstable rates and freezes (dropouts).
//!    Only intervals between reports sent while the mouse moves fast enough,
//!    in a steady direction, to have counts on every poll are timed, with the
//!    rules of the Stutter test (`analysis::stutter`): a slowly moving,
//!    turning or stopping mouse skips polls that carry no motion, which is
//!    normal and says nothing about the link.
//! 3. **Clicks** - normal clicking. Looks for switch bounce (accidental
//!    double clicks), extremely short presses, sticky releases and buttons
//!    that never release.
//! 4. **Scroll** - scrolling both ways. Looks for encoder skipping (a notch
//!    reported in the wrong direction) and a wheel that only works one way.
//! 5. **Lift** - lifting and replacing the mouse. Looks for cursor jumps on
//!    lift and set-down with the rules of the Lift-Off Jump test
//!    (`analysis::liftoff`), using `Config::dpi` to measure them in mm.
//!
//! The engine is pure: it is driven by `feed(time, event)` and `tick(time)`
//! with timestamps in seconds, so it can be exercised with synthetic input
//! streams in unit tests and reused by both the GUI and the CLI.
//!
//! A **Healthy** verdict needs every step to have collected enough data. A
//! button or wheel direction that registered nothing makes the run
//! Inconclusive, because a dead part and a skipped instruction look the same.
//! Without raw input (`Config::timing_reliable == false`) only the checks
//! that need neither precise report timing nor raw sensor counts are run.

use serde::Serialize;

use crate::analysis::liftoff::{
    self, counts_to_mm, mm_to_counts, LiftDetector, LiftTally, LiftVerdict, JUMP_THRESHOLD_MM,
    JUMP_WINDOW_MS,
};
use crate::analysis::polling::{
    estimate_hz, is_saturated_pair, median_in_place, usb_nominal_rate, MAX_POLL_INTERVAL_MS,
    USB_MIN_POLLING_HZ,
};
use crate::analysis::scroll::{WheelCheck, SKIP_FAULT_COUNT};
use crate::analysis::stutter::{
    is_freeze, CONFIRMED_FREEZES, MAX_FREEZE_MS, MIN_RELATIVE_REPORT_SIZE,
};
use crate::types::{MouseButton, MouseEvent};

/// The guided phases, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Phase {
    Rest,
    Movement,
    Clicks,
    Scroll,
    Lift,
}

impl Phase {
    pub fn title(self) -> &'static str {
        match self {
            Phase::Rest => "Sensor at rest",
            Phase::Movement => "Continuous movement",
            Phase::Clicks => "Button clicks",
            Phase::Scroll => "Scroll wheel",
            Phase::Lift => "Lift-off",
        }
    }

    pub fn instruction(self) -> &'static str {
        match self {
            Phase::Rest => "Take your hand off the mouse and do not touch it or the desk.",
            Phase::Movement => {
                "Move the mouse continuously in brisk, wide circles. Keep it moving until this step ends."
            }
            Phase::Clicks => {
                "Click the left button at a normal pace (about 20 times), then the right button a few times. Do not hold the buttons."
            }
            Phase::Scroll => "Scroll the wheel down about 10 notches, then up about 10 notches.",
            Phase::Lift => {
                "Lift the mouse straight up about 1 cm and set it straight back down without sliding it. Keep it still for a second, then move it in a small circle and stop. Repeat 3 times."
            }
        }
    }
}

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "INFO",
            Severity::Warning => "WARNING",
            Severity::Critical => "CRITICAL",
        }
    }
}

/// One thing the diagnostics noticed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub phase: Phase,
    pub title: String,
    pub detail: String,
}

/// Overall outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Verdict {
    /// No warnings or critical findings.
    Healthy,
    /// At least one warning.
    Attention,
    /// At least one critical finding.
    Faulty,
    /// Not enough data was collected to judge the core phases.
    Inconclusive,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Healthy => "Healthy",
            Verdict::Attention => "Needs attention",
            Verdict::Faulty => "Fault detected",
            Verdict::Inconclusive => "Inconclusive",
        }
    }
}

/// Raw numbers behind the findings.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Metrics {
    // Rest
    pub rest_move_events: usize,
    pub rest_distance_counts: f64,
    pub rest_clicks: usize,
    pub rest_scroll_events: usize,
    /// Separate one-second windows of the rest phase that saw movement.
    pub rest_motion_seconds: usize,
    // Movement
    /// Report intervals used for the timing checks: those between reports
    /// sent while the mouse moved fast enough to report on every poll.
    pub move_intervals: usize,
    pub polling_hz: Option<u32>,
    /// The standard USB rate (125, 250, 500, 1000 ... Hz) nearest
    /// `polling_hz`. None below 110 Hz, where no USB rate fits: that is the
    /// Bluetooth range or a USB mouse running slow.
    pub polling_nominal_hz: Option<u32>,
    pub stutter_count: usize,
    pub stutter_rate_percent: f64,
    pub unstable_interval_percent: f64,
    pub max_interval_ms: f64,
    /// Gaps of 0.1-1 s while the mouse kept moving the same way (see
    /// `analysis::stutter::is_freeze`).
    pub movement_freezes: usize,
    pub movement_pauses: usize,
    // Clicks
    pub left_clicks: usize,
    pub right_clicks: usize,
    pub other_clicks: usize,
    pub bounce_events: usize,
    pub micro_presses: usize,
    pub sticky_releases: usize,
    pub stuck_buttons: usize,
    pub avg_hold_ms: f64,
    pub max_hold_ms: f64,
    pub min_press_interval_ms: Option<f64>,
    // Scroll
    pub scroll_up: usize,
    pub scroll_down: usize,
    pub scroll_reversals: usize,
    // Lift
    pub lift_jumps: usize,
    pub lift_max_jump_counts: f64,
    /// Largest jump converted with `lift_dpi`.
    pub lift_max_jump_mm: f64,
    /// DPI the lift distances were measured with...
    pub lift_dpi: f64,
    /// ... and whether it was assumed because `Config::dpi` was not given.
    pub lift_dpi_assumed: bool,
}

/// Final report.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub verdict: Verdict,
    pub findings: Vec<Finding>,
    pub metrics: Metrics,
    pub phases_completed: Vec<Phase>,
    /// Steps that ran but had too little data to judge (or found a part
    /// that registered nothing).
    pub untested: Vec<Phase>,
    pub aborted: bool,
    /// False when timestamps came from a frame-rate-limited source, in which
    /// case the timing-based checks were skipped.
    pub timing_reliable: bool,
    pub duration_secs: f64,
}

impl Report {
    pub fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == severity)
            .count()
    }

    fn step_ran(&self, phase: Phase) -> bool {
        self.phases_completed.contains(&phase)
    }

    fn step_judged(&self, phase: Phase) -> bool {
        self.step_ran(phase) && !self.untested.contains(&phase)
    }

    // The headline numbers, `None` where the check was not run or had too
    // little data: a 0 there would read as a measured pass.

    /// Stutters as a percentage of the timed movement intervals.
    pub fn stutter_rate_percent(&self) -> Option<f64> {
        self.step_judged(Phase::Movement)
            .then_some(self.metrics.stutter_rate_percent)
    }

    /// Switch bounces, judged only on device timestamps.
    pub fn bounce_events(&self) -> Option<usize> {
        let clicks =
            self.metrics.left_clicks + self.metrics.right_clicks + self.metrics.other_clicks;
        (self.timing_reliable && self.step_ran(Phase::Clicks) && clicks > 0)
            .then_some(self.metrics.bounce_events)
    }

    /// Scroll notches reversed within the skip window, judged only on device
    /// timestamps.
    pub fn scroll_reversals(&self) -> Option<usize> {
        let notches = self.metrics.scroll_up + self.metrics.scroll_down;
        (self.timing_reliable && self.step_ran(Phase::Scroll) && notches > 0)
            .then_some(self.metrics.scroll_reversals)
    }

    /// Cursor jumps on lift, when the lift step had something to judge.
    pub fn lift_jumps(&self) -> Option<usize> {
        self.step_judged(Phase::Lift)
            .then_some(self.metrics.lift_jumps)
    }

    /// Unit of `Metrics::rest_distance_counts`: raw sensor counts, or
    /// pointer units without raw input.
    pub fn rest_unit(&self) -> &'static str {
        if self.timing_reliable {
            "counts"
        } else {
            "pointer units"
        }
    }
}

/// Tunable thresholds and durations.
#[derive(Debug, Clone)]
pub struct Config {
    /// Whether event timestamps are precise enough for timing checks. When
    /// false (frame-rate-limited window-system input with accelerated pointer
    /// deltas) the movement and lift-off steps are left out, and switch
    /// bounce, very short presses and scroll skips are not judged.
    pub timing_reliable: bool,
    /// Seconds ignored at the start of the rest phase so the click that
    /// started the run and the hand leaving the mouse do not count.
    pub rest_settle_secs: f64,
    pub rest_secs: f64,
    pub movement_secs: f64,
    /// The movement phase ends early once this many fast-movement intervals
    /// (between reports of a saturated pair, see
    /// `polling::is_saturated_pair`) are recorded...
    pub movement_target_intervals: usize,
    /// ... and once this many seconds of fast movement are recorded, so a
    /// 2-8 kHz mouse is not judged on a fraction of a second.
    pub movement_target_secs: f64,
    /// Below this many fast-movement intervals the movement phase is
    /// inconclusive...
    pub movement_min_intervals: usize,
    /// ... and below this many seconds of fast movement.
    pub movement_min_secs: f64,
    pub clicks_secs: f64,
    pub clicks_target_left: usize,
    pub clicks_target_right: usize,
    pub clicks_min_left: usize,
    pub scroll_secs: f64,
    pub scroll_target_each: usize,
    pub lift_secs: f64,
    /// Two presses of the same button closer than this are switch bounce.
    pub bounce_threshold_ms: f64,
    /// A press that follows the previous release of the same button within
    /// this gap is switch bounce (the contact re-closing as it opens).
    pub bounce_gap_ms: f64,
    /// A press shorter than this cannot be a human click.
    pub micro_press_ms: f64,
    /// A press held longer than this, when told to click normally, is sticky.
    pub sticky_threshold_ms: f64,
    /// A button still down for this long at the end of the phase is stuck.
    pub stuck_threshold_ms: f64,
    /// An interval longer than this multiple of the poll period (the median
    /// fast-movement interval) is a stutter...
    pub stutter_multiplier: f64,
    /// ... and never shorter than this (ms). At 2-8 kHz the host's own
    /// timestamping (USB interrupt moderation, batched delivery) moves reports
    /// by a few hundred microseconds, and a gap this short is well under one
    /// display frame (4 ms at 240 Hz), so it is not a visible hitch.
    pub min_stutter_gap_ms: f64,
    /// Intervals this far from the poll period (fraction) count as unstable...
    pub unstable_fraction: f64,
    /// ... but deviations under this many ms are host timestamp jitter and
    /// never count as unstable (only matters at 2 kHz and above).
    pub timing_jitter_ms: f64,
    /// Rest movement only counts as jitter or drift when it appears in at
    /// least this many separate one-second windows. Bumping the mouse or desk
    /// is one burst well under a second long; a drifting or noisy sensor
    /// keeps reporting. The hand leaving late is not counted: movement that
    /// runs from the start of the step and then stops, or everything up to
    /// the largest burst when only noise follows it for a second or more.
    /// Severity is judged on the movement outside the largest burst.
    pub rest_spread_windows: usize,
    /// A direction change within this many ms of the previous notch is a skip.
    pub scroll_reversal_ms: f64,
    /// Whether to run the lift-off phase at all. Lift-off jumps are judged
    /// with the rules in `analysis::liftoff`.
    pub include_lift: bool,
    /// The mouse's DPI, used to turn lift-off motion into mm. `None` when
    /// unknown: `liftoff::DEFAULT_DPI` (800) is assumed, the findings say
    /// so, and jumps found then make the step inconclusive rather than a
    /// warning, since at a higher DPI they may be ordinary set-down shifts.
    pub dpi: Option<f64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            timing_reliable: true,
            // Pressing Start and taking the hand away takes about 1-2 s.
            rest_settle_secs: 2.0,
            rest_secs: 5.0,
            movement_secs: 12.0,
            movement_target_intervals: 3000,
            movement_target_secs: 3.0,
            movement_min_intervals: 200,
            movement_min_secs: 1.0,
            clicks_secs: 20.0,
            clicks_target_left: 20,
            clicks_target_right: 5,
            clicks_min_left: 5,
            scroll_secs: 12.0,
            scroll_target_each: 10,
            lift_secs: 10.0,
            bounce_threshold_ms: 50.0,
            bounce_gap_ms: 25.0,
            micro_press_ms: 3.0,
            sticky_threshold_ms: 250.0,
            stuck_threshold_ms: 1000.0,
            stutter_multiplier: 2.5,
            min_stutter_gap_ms: 2.0,
            unstable_fraction: 0.5,
            timing_jitter_ms: 0.5,
            rest_spread_windows: 3,
            scroll_reversal_ms: 60.0,
            include_lift: true,
            dpi: None,
        }
    }
}

/// Live status of the current phase, for progress display.
#[derive(Debug, Clone, PartialEq)]
pub struct PhaseStatus {
    pub phase: Phase,
    pub index: usize,
    pub total: usize,
    pub elapsed_secs: f64,
    pub remaining_secs: f64,
    /// 0..1, the larger of time progress and data-collection progress.
    pub progress: f64,
    /// Short live summary such as "812 reports, ~1000 Hz".
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Idle,
    Running { index: usize, phase_start: f64 },
    Finished,
}

const BUTTONS: usize = 6;

fn button_index(b: MouseButton) -> usize {
    match b {
        MouseButton::Left => 0,
        MouseButton::Right => 1,
        MouseButton::Middle => 2,
        MouseButton::Side => 3,
        MouseButton::Extra => 4,
        MouseButton::Unknown => 5,
    }
}

fn button_name(i: usize) -> &'static str {
    ["left", "right", "middle", "side", "extra", "unknown"][i.min(5)]
}

/// Fewer reports than a 60 Hz screen shows frames: the pointer moves in
/// visible steps whatever the connection. A healthy Bluetooth link (15 ms at
/// most, 67 Hz) stays above it.
const CHOPPY_POLLING_HZ: u32 = 60;

/// Severity, title and detail for a measured report rate below what a USB
/// mouse uses, or None when the rate is normal for any mouse.
fn low_polling_finding(hz: u32) -> Option<(Severity, &'static str, String)> {
    if hz < CHOPPY_POLLING_HZ {
        Some((
            Severity::Warning,
            "Very low polling rate",
            format!(
                "{} Hz is fewer reports than a 60 Hz screen shows frames, so the pointer moves in visible steps. A Bluetooth mouse normally reports at 67-133 Hz: this slow, it is likely in a power-saving mode (charge it) or the computer set up a slow Bluetooth connection. A USB mouse should report 125 Hz or more: check the polling rate in its software and try another USB port.",
                hz
            ),
        ))
    } else if hz < USB_MIN_POLLING_HZ {
        Some((
            Severity::Info,
            "Low polling rate (normal for Bluetooth)",
            format!(
                "{} Hz is normal for a Bluetooth mouse, which reports once per connection interval (about 67-133 Hz). A USB mouse, wired or with a receiver, should report 125 Hz or more: if this is one, check the polling rate in its software and try another USB port.",
                hz
            ),
        ))
    } else {
        None
    }
}

/// Switch bounces in one run at which the switch is failing. Neither a
/// finger nor a healthy switch can re-press within the bounce gaps
/// (`Config::bounce_gap_ms`, `Config::bounce_threshold_ms`), but one or two
/// can be a fluke, so they ask for a re-test, as scroll skips do
/// (`scroll::SKIP_FAULT_COUNT`). Every test that counts bounces (Auto
/// Diagnostics, the GUI and CLI Double-Click tests) uses this rule.
pub const CONFIRMED_BOUNCES: usize = 3;

/// What a number of switch bounces in one run means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BounceVerdict {
    None,
    /// One or two: re-run, a repeat means the switch is starting to fail.
    Possible,
    /// [`CONFIRMED_BOUNCES`] or more: a failing switch.
    Failing,
}

pub fn bounce_verdict(bounces: usize) -> BounceVerdict {
    match bounces {
        0 => BounceVerdict::None,
        n if n < CONFIRMED_BOUNCES => BounceVerdict::Possible,
        _ => BounceVerdict::Failing,
    }
}

/// Rest movement (counts) below which it is ordinary sensor noise.
const REST_NOISE_COUNTS: f64 = 3.0;

/// Movement seen in one second of the rest phase.
#[derive(Debug, Default, Clone, Copy)]
struct RestSecond {
    reports: usize,
    distance: f64,
}

#[derive(Debug, Default)]
struct ButtonState {
    pressed_at: Option<f64>,
    last_press: Option<f64>,
    last_release: Option<f64>,
    presses: usize,
    releases: usize,
    bounces: usize,
    micro_presses: usize,
    sticky: usize,
}

/// The diagnostics state machine.
#[derive(Debug)]
pub struct AutoTest {
    cfg: Config,
    phases: Vec<Phase>,
    state: State,
    now: f64,
    started_at: f64,
    findings: Vec<Finding>,
    metrics: Metrics,
    phases_completed: Vec<Phase>,
    report: Option<Report>,
    /// Phases whose data was too thin to judge (or that found a part which
    /// registered nothing); any entry rules out a Healthy verdict.
    untested: Vec<Phase>,
    // Rest
    rest_distance: f64,
    rest_seconds: Vec<RestSecond>,
    // Movement
    /// Time and counts of the previous motion report.
    last_move: Option<(f64, i32, i32)>,
    /// Intervals (up to a poll) between consecutive reports that form a
    /// saturated pair...
    fast_intervals_ms: Vec<f64>,
    /// ... with the size (counts) of the smaller of the two reports and of
    /// the later one, to leave out the small reports around a brief stop.
    fast_sizes: Vec<(f64, f64)>,
    /// Longer gaps between the reports of a saturated pair, up to
    /// `MAX_FREEZE_MS`, with the smaller report's size: a brief stop or a
    /// freeze, told apart by that size once the median is known.
    long_gaps: Vec<(f64, f64)>,
    /// Running sum of `fast_intervals_ms`, checked on every report.
    fast_total_ms: f64,
    /// All report intervals, for the "move faster" hint.
    all_intervals: usize,
    movement_pauses: usize,
    // Clicks
    buttons: Vec<ButtonState>,
    holds_ms: Vec<f64>,
    min_press_interval_ms: Option<f64>,
    // Scroll
    /// The Scroll Wheel test's checker, so both count skips alike.
    wheel: WheelCheck,
    // Lift
    /// Created when the lift step first sees input or time pass.
    lift: Option<LiftDetector>,
    lift_tally: LiftTally,
}

impl AutoTest {
    pub fn new(cfg: Config) -> Self {
        let mut phases = vec![Phase::Rest];
        if cfg.timing_reliable {
            phases.push(Phase::Movement);
        }
        phases.push(Phase::Clicks);
        phases.push(Phase::Scroll);
        // Lift-off sums raw sensor counts over the first 100 ms of motion and
        // converts them to mm with the DPI; per-frame, OS-accelerated pointer
        // deltas cannot be judged that way.
        if cfg.include_lift && cfg.timing_reliable {
            phases.push(Phase::Lift);
        }
        let wheel = WheelCheck::with_thresholds(cfg.scroll_reversal_ms, cfg.scroll_target_each);
        Self {
            cfg,
            phases,
            state: State::Idle,
            now: 0.0,
            started_at: 0.0,
            findings: Vec::new(),
            metrics: Metrics::default(),
            phases_completed: Vec::new(),
            report: None,
            untested: Vec::new(),
            rest_distance: 0.0,
            rest_seconds: Vec::new(),
            last_move: None,
            fast_intervals_ms: Vec::new(),
            fast_sizes: Vec::new(),
            long_gaps: Vec::new(),
            fast_total_ms: 0.0,
            all_intervals: 0,
            movement_pauses: 0,
            buttons: (0..BUTTONS).map(|_| ButtonState::default()).collect(),
            holds_ms: Vec::new(),
            min_press_interval_ms: None,
            wheel,
            lift: None,
            lift_tally: LiftTally::default(),
        }
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// The phases this run will go through.
    pub fn phases(&self) -> &[Phase] {
        &self.phases
    }

    /// Begin the run at time `now` (seconds).
    pub fn start(&mut self, now: f64) {
        *self = AutoTest::new(self.cfg.clone());
        self.now = now;
        self.started_at = now;
        self.state = State::Running {
            index: 0,
            phase_start: now,
        };
    }

    /// Stop early. A report is still produced for the phases that finished.
    pub fn abort(&mut self) {
        if let State::Running { index, .. } = self.state {
            let _ = index;
            self.finish(true);
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self.state, State::Running { .. })
    }

    pub fn is_finished(&self) -> bool {
        matches!(self.state, State::Finished)
    }

    pub fn current_phase(&self) -> Option<Phase> {
        match self.state {
            State::Running { index, .. } => self.phases.get(index).copied(),
            _ => None,
        }
    }

    pub fn report(&self) -> Option<&Report> {
        self.report.as_ref()
    }

    pub fn findings_so_far(&self) -> &[Finding] {
        &self.findings
    }

    /// Feed one input event that happened at time `t` (seconds).
    pub fn feed(&mut self, t: f64, event: &MouseEvent) {
        self.now = self.now.max(t);
        let (index, phase_start) = match self.state {
            State::Running { index, phase_start } => (index, phase_start),
            _ => return,
        };
        let phase = self.phases[index];
        let elapsed = t - phase_start;
        match phase {
            Phase::Rest => self.feed_rest(elapsed, event),
            Phase::Movement => self.feed_movement(t, event),
            Phase::Clicks => self.feed_clicks(t, event),
            Phase::Scroll => self.feed_scroll(t, event),
            Phase::Lift => self.feed_lift(phase_start, t, event),
        }
        self.advance_if_done(index, phase_start);
    }

    /// Advance time without an event. Call regularly (each frame / loop).
    pub fn tick(&mut self, now: f64) {
        self.now = self.now.max(now);
        if let State::Running { index, phase_start } = self.state {
            if self.phases[index] == Phase::Lift {
                self.tick_lift(phase_start, self.now);
            }
            self.advance_if_done(index, phase_start);
        }
    }

    /// Progress of the current phase, or `None` when not running.
    pub fn status(&self) -> Option<PhaseStatus> {
        let (index, phase_start) = match self.state {
            State::Running { index, phase_start } => (index, phase_start),
            _ => return None,
        };
        let phase = self.phases[index];
        let elapsed = (self.now - phase_start).max(0.0);
        let limit = self.phase_time_limit(phase);
        let time_progress = (elapsed / limit).min(1.0);
        let (data_progress, detail) = self.phase_data_progress(phase);
        Some(PhaseStatus {
            phase,
            index,
            total: self.phases.len(),
            elapsed_secs: elapsed,
            remaining_secs: (limit - elapsed).max(0.0),
            progress: time_progress.max(data_progress).min(1.0),
            detail,
        })
    }

    // ── phase bookkeeping ──────────────────────────────────────────────

    fn phase_time_limit(&self, phase: Phase) -> f64 {
        match phase {
            Phase::Rest => self.cfg.rest_settle_secs + self.cfg.rest_secs,
            Phase::Movement => self.cfg.movement_secs,
            Phase::Clicks => self.cfg.clicks_secs,
            Phase::Scroll => self.cfg.scroll_secs,
            Phase::Lift => self.cfg.lift_secs,
        }
    }

    fn phase_data_progress(&self, phase: Phase) -> (f64, String) {
        match phase {
            Phase::Rest => (
                0.0,
                if self.metrics.rest_move_events == 0 && self.metrics.rest_clicks == 0 {
                    "No input so far - good".to_string()
                } else {
                    format!(
                        "{} movement report(s), {} click(s) while at rest",
                        self.metrics.rest_move_events, self.metrics.rest_clicks
                    )
                },
            ),
            Phase::Movement => {
                let n = self.fast_intervals_ms.len();
                let progress = (n as f64 / self.cfg.movement_target_intervals as f64)
                    .min(self.fast_secs() / self.cfg.movement_target_secs);
                let hz = estimate_hz(&self.fast_intervals_ms, 20)
                    .map(|h| format!("~{} Hz", h))
                    .unwrap_or_else(|| "measuring...".to_string());
                // Most reports too slow to time: tell the user while they can
                // still do something about it.
                let hint = if self.all_intervals >= 50 && n * 2 < self.all_intervals {
                    " - move faster"
                } else {
                    ""
                };
                (progress, format!("{} fast reports, {}{}", n, hz, hint))
            }
            Phase::Clicks => {
                let l = self.buttons[0].releases;
                let r = self.buttons[1].releases;
                let p = (l as f64 / self.cfg.clicks_target_left as f64)
                    .min(r as f64 / self.cfg.clicks_target_right as f64);
                (
                    p,
                    format!(
                        "Left {}/{}, right {}/{}",
                        l, self.cfg.clicks_target_left, r, self.cfg.clicks_target_right
                    ),
                )
            }
            Phase::Scroll => {
                let u = self.metrics.scroll_up;
                let d = self.metrics.scroll_down;
                let p = (u as f64 / self.cfg.scroll_target_each as f64)
                    .min(d as f64 / self.cfg.scroll_target_each as f64);
                (
                    p,
                    format!(
                        "Down {}/{}, up {}/{}",
                        d, self.cfg.scroll_target_each, u, self.cfg.scroll_target_each
                    ),
                )
            }
            Phase::Lift => (
                0.0,
                format!("{} jump(s) detected", self.lift_tally.jumps.len()),
            ),
        }
    }

    fn phase_criteria_met(&self, phase: Phase) -> bool {
        match phase {
            Phase::Rest | Phase::Lift => false,
            Phase::Movement => {
                self.fast_intervals_ms.len() >= self.cfg.movement_target_intervals
                    && self.fast_secs() >= self.cfg.movement_target_secs
            }
            Phase::Clicks => {
                self.buttons[0].releases >= self.cfg.clicks_target_left
                    && self.buttons[1].releases >= self.cfg.clicks_target_right
            }
            Phase::Scroll => {
                self.metrics.scroll_up >= self.cfg.scroll_target_each
                    && self.metrics.scroll_down >= self.cfg.scroll_target_each
            }
        }
    }

    fn advance_if_done(&mut self, index: usize, phase_start: f64) {
        let phase = self.phases[index];
        let elapsed = self.now - phase_start;
        let timed_out = elapsed >= self.phase_time_limit(phase);
        if !timed_out && !self.phase_criteria_met(phase) {
            return;
        }
        self.finalize_phase(phase);
        self.phases_completed.push(phase);
        let next = index + 1;
        if next >= self.phases.len() {
            self.finish(false);
        } else {
            self.state = State::Running {
                index: next,
                phase_start: self.now,
            };
        }
    }

    fn finish(&mut self, aborted: bool) {
        let has_critical = self
            .findings
            .iter()
            .any(|f| f.severity == Severity::Critical);
        let has_warning = self
            .findings
            .iter()
            .any(|f| f.severity == Severity::Warning);
        // Healthy means every step ran and had something to judge; a skipped
        // step or a part that registered nothing leaves the run Inconclusive.
        let all_done = self
            .phases
            .iter()
            .all(|p| self.phases_completed.contains(p));
        let enough_data = all_done && self.untested.is_empty();
        let verdict = if has_critical {
            Verdict::Faulty
        } else if has_warning {
            Verdict::Attention
        } else if !enough_data {
            Verdict::Inconclusive
        } else {
            Verdict::Healthy
        };
        if !self.cfg.timing_reliable {
            self.findings.push(Finding {
                severity: Severity::Info,
                phase: Phase::Movement,
                title: "Timing checks skipped".to_string(),
                detail: "Raw input is not available, so only checks that need neither precise report timing nor raw sensor counts were run (phantom input at rest, buttons and wheel directions registering, stuck or sticky buttons). Polling rate, stutters, switch bounce, very short presses, scroll-wheel skips and lift-off jumps were NOT tested.".to_string(),
            });
        }
        let mut findings = self.findings.clone();
        findings.sort_by_key(|f| std::cmp::Reverse(f.severity));
        self.report = Some(Report {
            verdict,
            findings,
            metrics: self.metrics.clone(),
            phases_completed: self.phases_completed.clone(),
            untested: {
                let mut untested = self.untested.clone();
                untested.dedup();
                untested
            },
            aborted,
            timing_reliable: self.cfg.timing_reliable,
            duration_secs: self.now - self.started_at,
        });
        self.state = State::Finished;
    }

    /// Seconds of fast movement timed so far.
    fn fast_secs(&self) -> f64 {
        self.fast_total_ms / 1000.0
    }

    /// Unit of rest-phase movement: raw sensor counts, or pointer points when
    /// only the window system's (accelerated) pointer deltas are available.
    fn move_unit(&self) -> &'static str {
        if self.cfg.timing_reliable {
            "counts"
        } else {
            "pointer units"
        }
    }

    fn push(&mut self, severity: Severity, phase: Phase, title: &str, detail: String) {
        self.findings.push(Finding {
            severity,
            phase,
            title: title.to_string(),
            detail,
        });
    }

    // ── Rest ───────────────────────────────────────────────────────────

    fn feed_rest(&mut self, elapsed: f64, event: &MouseEvent) {
        if elapsed < self.cfg.rest_settle_secs {
            return;
        }
        match event {
            MouseEvent::Move { dx, dy } => {
                if *dx != 0 || *dy != 0 {
                    let distance = ((*dx as f64).powi(2) + (*dy as f64).powi(2)).sqrt();
                    self.metrics.rest_move_events += 1;
                    self.rest_distance += distance;
                    let second = ((elapsed - self.cfg.rest_settle_secs).floor() as usize)
                        .min(self.cfg.rest_secs.ceil() as usize);
                    if self.rest_seconds.len() <= second {
                        self.rest_seconds.resize(second + 1, RestSecond::default());
                    }
                    self.rest_seconds[second].reports += 1;
                    self.rest_seconds[second].distance += distance;
                }
            }
            MouseEvent::ButtonPress(_) => self.metrics.rest_clicks += 1,
            MouseEvent::ButtonRelease(_) => {}
            MouseEvent::Scroll { .. } => self.metrics.rest_scroll_events += 1,
        }
    }

    fn finalize_rest(&mut self) {
        self.metrics.rest_distance_counts = self.rest_distance;
        let moved = |s: &&RestSecond| s.reports > 0;
        self.metrics.rest_motion_seconds = self.rest_seconds.iter().filter(moved).count();
        let n = self.metrics.rest_move_events;
        let d = self.rest_distance;
        let unit = self.move_unit();
        // Movement that runs from the start of the step and then stops for
        // good is the hand still leaving the mouse, however long that took.
        // Only movement after it can show the sensor moving on its own.
        let windows_in_step = self.cfg.rest_secs.ceil() as usize;
        let lead = self
            .rest_seconds
            .iter()
            .take_while(|s| s.reports > 0)
            .count();
        // A hand resting on the mouse moves it only now and then, so it can
        // also leave after a still second: the largest burst is the hand
        // lifting off when only noise follows it for at least a whole
        // second, and everything before it was the hand too. A burst in the
        // last second is not excused: it may be the next step started early.
        let lift_off = self
            .rest_seconds
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.distance.total_cmp(&b.1.distance))
            .map(|(i, _)| i)
            .filter(|&i| {
                let after: f64 = self.rest_seconds[i + 1..].iter().map(|s| s.distance).sum();
                i + 1 < windows_in_step && after < REST_NOISE_COUNTS
            })
            .map_or(0, |i| i + 1);
        let skip = if lead < windows_in_step { lead } else { 0 }.max(lift_off);
        let later = &self.rest_seconds[skip.min(self.rest_seconds.len())..];
        let windows = later.iter().filter(moved).count();
        let later_reports: usize = later.iter().map(|s| s.reports).sum();
        let later_distance: f64 = later.iter().map(|s| s.distance).sum();
        // One burst (the mouse or desk nudged) cannot make a fault by itself,
        // so severity is judged on the movement outside the largest one.
        let burst = later.iter().map(|s| s.distance).fold(0.0, f64::max);
        let spread = later_distance - burst;
        if n == 0 {
            self.push(
                Severity::Info,
                Phase::Rest,
                "Sensor perfectly still at rest",
                "No movement was reported while the mouse was untouched.".to_string(),
            );
        } else if d < REST_NOISE_COUNTS {
            self.push(
                Severity::Info,
                Phase::Rest,
                "Slight sensor noise at rest",
                format!(
                    "{} movement report(s) totalling {:.1} {} in {:.0} s. This is within normal sensor noise.",
                    n, d, unit, self.cfg.rest_secs
                ),
            );
        } else if windows == 0 {
            self.push(
                Severity::Info,
                Phase::Rest,
                "Movement at the start of the rest step",
                format!(
                    "{} movement report(s) totalling {:.1} {} in the first {} second(s) after the {:.0} s allowed for letting go, then none until the step ended: the hand leaving the mouse late, not the sensor. A drifting sensor keeps moving. If nobody touched the mouse then, re-run to check.",
                    n, d, unit, skip, self.cfg.rest_settle_secs
                ),
            );
        } else if windows < self.cfg.rest_spread_windows || spread < REST_NOISE_COUNTS {
            self.push(
                Severity::Info,
                Phase::Rest,
                "Brief movement at rest",
                format!(
                    "{} movement report(s) totalling {:.1} {}, in short bursts rather than the steady movement of a drifting or noisy sensor, as when the hand leaves late or the mouse or desk is nudged. Re-run without touching the mouse or desk if unsure.",
                    n, d, unit
                ),
            );
        } else if spread < 20.0 {
            self.push(
                Severity::Warning,
                Phase::Rest,
                "Sensor jitter at rest",
                format!(
                    "{} movement report(s) totalling {:.1} {} in {} separate seconds while untouched. Try a different surface or clean the sensor lens; if it persists the sensor is noisy.",
                    later_reports, later_distance, unit, windows
                ),
            );
        } else {
            self.push(
                Severity::Critical,
                Phase::Rest,
                "Sensor moves on its own",
                format!(
                    "{} movement report(s) totalling {:.1} {} in {} separate seconds while untouched. The sensor is drifting or the surface is unsuitable (glass, glossy, or patterned). If your hand was on the mouse, re-run without touching it.",
                    later_reports, later_distance, unit, windows
                ),
            );
        }
        if self.metrics.rest_clicks > 0 {
            self.push(
                Severity::Critical,
                Phase::Rest,
                "Phantom clicks",
                format!(
                    "{} button press(es) were reported while the mouse was untouched. A switch is firing on its own.",
                    self.metrics.rest_clicks
                ),
            );
        }
        if self.metrics.rest_scroll_events > 0 {
            self.push(
                Severity::Warning,
                Phase::Rest,
                "Phantom scroll events",
                format!(
                    "{} scroll notch(es) were reported while the mouse was untouched. The wheel encoder may be dirty or failing.",
                    self.metrics.rest_scroll_events
                ),
            );
        }
    }

    // ── Movement ───────────────────────────────────────────────────────

    fn feed_movement(&mut self, t: f64, event: &MouseEvent) {
        if let MouseEvent::Move { dx, dy } = *event {
            if dx == 0 && dy == 0 {
                return;
            }
            if let Some((last_t, ldx, ldy)) = self.last_move {
                let interval_ms = (t - last_t) * 1000.0;
                if interval_ms > MAX_FREEZE_MS {
                    self.movement_pauses += 1;
                } else if interval_ms > 0.0 {
                    self.all_intervals += 1;
                    // Only fast movement in a steady direction is certain to
                    // report on every poll (a reversal inside one poll
                    // cancels its counts), as in the Stutter test.
                    if is_saturated_pair((ldx, ldy), (dx, dy)) {
                        let size = (dx as f64).hypot(dy as f64);
                        let smaller = (ldx as f64).hypot(ldy as f64).min(size);
                        // Longer gaps are a stop or a freeze, not a poll (as
                        // in `estimate_hz`), and must not count as timed
                        // movement.
                        if interval_ms <= MAX_POLL_INTERVAL_MS {
                            self.fast_intervals_ms.push(interval_ms);
                            self.fast_sizes.push((smaller, size));
                            self.fast_total_ms += interval_ms;
                        } else {
                            self.long_gaps.push((interval_ms, smaller));
                        }
                    }
                }
            }
            self.last_move = Some((t, dx, dy));
        }
    }

    /// Fast-movement intervals to judge, and freezes: those whose smaller
    /// report is not one of the small reports of a hand slowing to a stop
    /// (see `stutter::MIN_RELATIVE_REPORT_SIZE`).
    fn judged_movement(&self) -> (Vec<f64>, Vec<f64>) {
        let mut sizes: Vec<f64> = self.fast_sizes.iter().map(|&(_, size)| size).collect();
        let Some(median_size) = median_in_place(&mut sizes) else {
            return (Vec::new(), Vec::new());
        };
        let intervals = self
            .fast_intervals_ms
            .iter()
            .zip(&self.fast_sizes)
            .filter(|(_, &(smaller, _))| smaller >= MIN_RELATIVE_REPORT_SIZE * median_size)
            .map(|(&ms, _)| ms)
            .collect();
        let freezes = self
            .long_gaps
            .iter()
            .filter(|&&(ms, smaller)| is_freeze(ms, smaller, median_size))
            .map(|&(ms, _)| ms)
            .collect();
        (intervals, freezes)
    }

    fn finalize_movement(&mut self) {
        let (usable, freezes) = self.judged_movement();
        let usable_secs = usable.iter().sum::<f64>() / 1000.0;
        self.metrics.move_intervals = usable.len();
        self.metrics.movement_pauses = self.movement_pauses;
        self.metrics.movement_freezes = freezes.len();
        // A freeze is judged whether or not enough movement was timed: the
        // link dropping out may be why there was too little.
        self.push_movement_freezes(&freezes);

        if usable.len() < self.cfg.movement_min_intervals
            || usable_secs < self.cfg.movement_min_secs
        {
            self.untested.push(Phase::Movement);
            self.push(
                Severity::Info,
                Phase::Movement,
                "Not enough fast movement recorded",
                format!(
                    "Only {} of {} report intervals ({:.1} s) were captured while the mouse moved fast enough to report on every poll (need {} over {:.0} s). A slowly moving mouse skips polls that carry no motion, so its gaps say nothing about polling. Polling rate and stutter checks are inconclusive; move in brisk, wide circles for the whole step.",
                    usable.len(),
                    self.all_intervals,
                    usable_secs,
                    self.cfg.movement_min_intervals,
                    self.cfg.movement_min_secs
                ),
            );
            if self.movement_pauses > 0 {
                self.push_movement_pauses();
            }
            return;
        }

        // Saturated reports come once per poll, so their typical interval is
        // the poll period itself.
        let mut sorted = usable.clone();
        let period = median_in_place(&mut sorted).unwrap_or(0.0);
        let hz = estimate_hz(&usable, self.cfg.movement_min_intervals);
        let nominal = hz.and_then(|h| usb_nominal_rate(h as f64));
        self.metrics.polling_hz = hz;
        self.metrics.polling_nominal_hz = nominal;
        self.metrics.max_interval_ms = usable.iter().copied().fold(0.0, f64::max);

        let stutter_gap = (period * self.cfg.stutter_multiplier).max(self.cfg.min_stutter_gap_ms);
        let unstable_dev = (period * self.cfg.unstable_fraction).max(self.cfg.timing_jitter_ms);
        let late: Vec<f64> = usable
            .iter()
            .copied()
            .filter(|&i| i > stutter_gap)
            .collect();
        let stutters = late.len();
        let unstable = usable
            .iter()
            .filter(|&&i| (i - period).abs() > unstable_dev)
            .count();
        self.metrics.stutter_count = stutters;
        self.metrics.stutter_rate_percent = stutters as f64 / usable.len() as f64 * 100.0;
        self.metrics.unstable_interval_percent = unstable as f64 / usable.len() as f64 * 100.0;
        // Severity is judged on the share of movement time spent waiting for
        // late reports, which means the same at 125 Hz and at 8 kHz (a count
        // of late reports would need 64x more hitches at 8 kHz to register).
        let lost_percent =
            late.iter().map(|i| i - period).sum::<f64>() / (usable_secs * 1000.0) * 100.0;

        if let Some(hz) = hz {
            let looks_like = nominal
                .map(|n| format!(" (looks like a {} Hz mouse)", n))
                .unwrap_or_default();
            self.push(
                Severity::Info,
                Phase::Movement,
                "Polling rate measured",
                format!(
                    "Typical report interval {:.3} ms = {} Hz{}, from {} reports sent while moving fast.",
                    period,
                    hz,
                    looks_like,
                    usable.len()
                ),
            );
            if let Some((severity, title, detail)) = low_polling_finding(hz) {
                self.push(severity, Phase::Movement, title, detail);
            }
        }

        if lost_percent > 5.0 {
            self.push(
                Severity::Critical,
                Phase::Movement,
                "Frequent stutters",
                format!(
                    "{} of {} reports arrived more than {:.1} ms after the previous one (normal {:.2} ms, worst gap {:.1} ms), costing {:.1}% of the movement time. Expect visible hitching. Check the USB port/hub, wireless interference, and CPU load.",
                    stutters,
                    usable.len(),
                    stutter_gap,
                    period,
                    self.metrics.max_interval_ms,
                    lost_percent
                ),
            );
        } else if lost_percent > 1.0 {
            self.push(
                Severity::Warning,
                Phase::Movement,
                "Occasional stutters",
                format!(
                    "{} of {} reports arrived late (worst gap {:.1} ms), costing {:.1}% of the movement time. Usually a busy USB hub, wireless interference, or background load.",
                    stutters,
                    usable.len(),
                    self.metrics.max_interval_ms,
                    lost_percent
                ),
            );
        }
        if self.metrics.unstable_interval_percent > 10.0 && lost_percent <= 5.0 {
            self.push(
                Severity::Warning,
                Phase::Movement,
                "Unstable polling interval",
                format!(
                    "{:.1}% of report intervals while moving fast were more than {:.2} ms away from the typical {:.3} ms. The report rate is not steady.",
                    self.metrics.unstable_interval_percent, unstable_dev, period
                ),
            );
        }
        if self.movement_pauses > 0 {
            self.push_movement_pauses();
        }
    }

    fn push_movement_freezes(&mut self, freezes: &[f64]) {
        let longest = freezes.iter().copied().fold(0.0, f64::max);
        if freezes.len() >= CONFIRMED_FREEZES {
            self.push(
                Severity::Warning,
                Phase::Movement,
                "Movement froze",
                format!(
                    "{} times the reports stopped for {:.0}-{:.0} ms while the mouse kept moving the same way (the reports either side were full-size, so the hand had not slowed down). The cursor freezes like this when the connection drops out: move a wireless receiver closer and away from USB 3 ports and cables, or try another port or cable.",
                    freezes.len(),
                    freezes.iter().copied().fold(f64::MAX, f64::min),
                    longest
                ),
            );
        } else if freezes.len() == 1 {
            self.push(
                Severity::Info,
                Phase::Movement,
                "One freeze during movement",
                format!(
                    "Once, the reports stopped for {:.0} ms while the mouse kept moving the same way. Lifting the mouse while moving and setting it down still moving looks the same; if you did not, the connection dropped out. Re-run to check.",
                    longest
                ),
            );
        }
    }

    fn push_movement_pauses(&mut self) {
        self.push(
            Severity::Info,
            Phase::Movement,
            "Pauses during movement",
            format!(
                "{} gap(s) longer than {:.0} s were seen while you were asked to keep moving. If you did not stop, the connection is dropping out.",
                self.movement_pauses,
                MAX_FREEZE_MS / 1000.0
            ),
        );
    }

    // ── Clicks ─────────────────────────────────────────────────────────

    fn feed_clicks(&mut self, t: f64, event: &MouseEvent) {
        match event {
            MouseEvent::ButtonPress(b) => {
                let i = button_index(*b);
                let (cfg_bounce, cfg_gap) = (self.cfg.bounce_threshold_ms, self.cfg.bounce_gap_ms);
                // Bounce lives on a 1-50 ms scale; frame-quantised timestamps
                // put a quick click's press and release in the same frame.
                let timed = self.cfg.timing_reliable;
                let st = &mut self.buttons[i];
                st.presses += 1;
                let mut bounced = false;
                if let (Some(last), true) = (st.last_press, timed) {
                    let interval_ms = (t - last) * 1000.0;
                    if interval_ms < cfg_bounce {
                        bounced = true;
                    }
                    self.min_press_interval_ms = Some(
                        self.min_press_interval_ms
                            .map_or(interval_ms, |m: f64| m.min(interval_ms)),
                    );
                }
                if let (Some(last_release), true) = (st.last_release, timed) {
                    if (t - last_release) * 1000.0 < cfg_gap {
                        bounced = true;
                    }
                }
                if bounced {
                    st.bounces += 1;
                }
                st.last_press = Some(t);
                st.pressed_at = Some(t);
            }
            MouseEvent::ButtonRelease(b) => {
                let i = button_index(*b);
                let (micro, sticky) = (self.cfg.micro_press_ms, self.cfg.sticky_threshold_ms);
                let timed = self.cfg.timing_reliable;
                let st = &mut self.buttons[i];
                if let Some(pressed_at) = st.pressed_at.take() {
                    let hold_ms = (t - pressed_at) * 1000.0;
                    st.releases += 1;
                    st.last_release = Some(t);
                    if timed && hold_ms < micro {
                        st.micro_presses += 1;
                    }
                    if hold_ms > sticky {
                        st.sticky += 1;
                    }
                    self.holds_ms.push(hold_ms);
                }
                // A release with no matching press (e.g. the click that started
                // the run) is ignored.
            }
            _ => {}
        }
    }

    fn finalize_clicks(&mut self) {
        let end = self.now;
        let mut stuck = 0usize;
        let mut stuck_names: Vec<&'static str> = Vec::new();
        for (i, st) in self.buttons.iter().enumerate() {
            if let Some(pressed_at) = st.pressed_at {
                if (end - pressed_at) * 1000.0 > self.cfg.stuck_threshold_ms {
                    stuck += 1;
                    stuck_names.push(button_name(i));
                }
            }
        }
        let left = self.buttons[0].releases;
        let right = self.buttons[1].releases;
        let other: usize = self.buttons[2..].iter().map(|b| b.releases).sum();
        let bounces: usize = self.buttons.iter().map(|b| b.bounces).sum();
        let micro: usize = self.buttons.iter().map(|b| b.micro_presses).sum();
        let sticky: usize = self.buttons.iter().map(|b| b.sticky).sum();
        let total_presses: usize = self.buttons.iter().map(|b| b.presses).sum();

        self.metrics.left_clicks = left;
        self.metrics.right_clicks = right;
        self.metrics.other_clicks = other;
        self.metrics.bounce_events = bounces;
        self.metrics.micro_presses = micro;
        self.metrics.sticky_releases = sticky;
        self.metrics.stuck_buttons = stuck;
        self.metrics.min_press_interval_ms = self.min_press_interval_ms;
        if !self.holds_ms.is_empty() {
            self.metrics.avg_hold_ms =
                self.holds_ms.iter().sum::<f64>() / self.holds_ms.len() as f64;
            self.metrics.max_hold_ms = self.holds_ms.iter().copied().fold(0.0, f64::max);
        }

        // A button that never came back up is a fault however few clicks
        // happened: once it sticks, further clicks cannot register.
        if stuck > 0 {
            self.push(
                Severity::Critical,
                Phase::Clicks,
                "Button stuck down",
                format!(
                    "The {} button was still reported as pressed at the end of the step. The switch is not releasing.",
                    stuck_names.join(" and ")
                ),
            );
        }

        let left_stuck = stuck_names.contains(&"left");
        if left >= self.cfg.clicks_min_left {
            self.push(
                Severity::Info,
                Phase::Clicks,
                "Clicks recorded",
                format!(
                    "Left {}, right {}, other {}. Average hold {:.0} ms, longest {:.0} ms.",
                    left, right, other, self.metrics.avg_hold_ms, self.metrics.max_hold_ms
                ),
            );
        } else {
            self.untested.push(Phase::Clicks);
            // A stuck left button explains the low count by itself.
            if !left_stuck {
                let detail = if left == 0 {
                    "No left-button clicks were registered. If you clicked the left button, it may be dead; if you did not, the switch checks were not run.".to_string()
                } else {
                    format!(
                        "Only {} left click(s) were registered (need at least {}). If you clicked more often than that, the button is missing clicks; otherwise the switch checks are inconclusive.",
                        left, self.cfg.clicks_min_left
                    )
                };
                let title = if left == 0 {
                    "No left clicks registered"
                } else {
                    "Not enough clicks recorded"
                };
                self.push(Severity::Info, Phase::Clicks, title, detail);
            }
        }

        // Bounce evidence counts even in a short run. Neither a finger nor a
        // healthy switch can re-press within these gaps, so severity follows
        // the number of bounces, not their share (see `bounce_verdict`).
        let bounce_rate = if total_presses > 0 {
            bounces as f64 / total_presses as f64 * 100.0
        } else {
            0.0
        };
        match bounce_verdict(bounces) {
            BounceVerdict::Failing => self.push(
                Severity::Critical,
                Phase::Clicks,
                "Switch bounce (accidental double clicks)",
                format!(
                    "{} press(es) came within {:.0} ms of the previous release, or within {:.0} ms of the previous press, of the same button ({:.1}% of presses). This is the classic sign of a worn microswitch.",
                    bounces, self.cfg.bounce_gap_ms, self.cfg.bounce_threshold_ms, bounce_rate
                ),
            ),
            BounceVerdict::Possible => self.push(
                Severity::Warning,
                Phase::Clicks,
                "Possible switch bounce",
                format!(
                    "{} press(es) came within {:.0} ms of the previous release, or within {:.0} ms of the previous press, of the same button. Re-run the test; if it happens again the switch is starting to fail.",
                    bounces, self.cfg.bounce_gap_ms, self.cfg.bounce_threshold_ms
                ),
            ),
            BounceVerdict::None => {}
        }
        if micro > 0 {
            self.push(
                Severity::Warning,
                Phase::Clicks,
                "Extremely short presses",
                format!(
                    "{} press(es) lasted under {:.0} ms, which a finger cannot do. This is contact bounce inside the switch.",
                    micro, self.cfg.micro_press_ms
                ),
            );
        }
        if sticky >= 2 {
            self.push(
                Severity::Warning,
                Phase::Clicks,
                "Slow or sticky releases",
                format!(
                    "{} click(s) were held longer than {:.0} ms although you were asked to click normally (longest {:.0} ms). If you were not holding the button, the switch is releasing late.",
                    sticky, self.cfg.sticky_threshold_ms, self.metrics.max_hold_ms
                ),
            );
        } else if sticky == 1 {
            self.push(
                Severity::Info,
                Phase::Clicks,
                "One long click",
                format!(
                    "One click was held for {:.0} ms. A single long hold is usually just the hand, not the switch.",
                    self.metrics.max_hold_ms
                ),
            );
        }
        if right == 0 && !stuck_names.contains(&"right") {
            self.untested.push(Phase::Clicks);
            self.push(
                Severity::Info,
                Phase::Clicks,
                "No right clicks registered",
                "No right-button clicks were registered. If you clicked the right button, it may be dead; if you did not, it was not tested."
                    .to_string(),
            );
        }
    }

    // ── Scroll ─────────────────────────────────────────────────────────

    fn feed_scroll(&mut self, t: f64, event: &MouseEvent) {
        if let MouseEvent::Scroll { delta } = *event {
            self.wheel.feed(t, delta);
            self.metrics.scroll_up = self.wheel.up;
            self.metrics.scroll_down = self.wheel.down;
            // A skip is a wrong-way notch within tens of ms, which frame-
            // quantised timestamps cannot resolve.
            if self.cfg.timing_reliable {
                self.metrics.scroll_reversals = self.wheel.skips;
            }
        }
    }

    fn finalize_scroll(&mut self) {
        let up = self.metrics.scroll_up;
        let down = self.metrics.scroll_down;
        let total = up + down;
        let reversals = self.metrics.scroll_reversals;
        if total < 5 {
            self.untested.push(Phase::Scroll);
            self.push(
                Severity::Info,
                Phase::Scroll,
                "Not enough scrolling recorded",
                if total == 0 {
                    "No scroll notches were registered. If you turned the wheel, it may be dead; if you did not, the wheel was not tested.".to_string()
                } else {
                    format!(
                        "Only {} notch(es) were registered (need 5). If you scrolled more than that, the wheel is missing notches; otherwise the wheel checks are inconclusive.",
                        total
                    )
                },
            );
            return;
        }
        self.push(
            Severity::Info,
            Phase::Scroll,
            "Scroll wheel recorded",
            format!("{} notches down, {} notches up.", down, up),
        );
        if up == 0 || down == 0 {
            // Without raw input up and down are the window system's, which
            // natural scrolling (the macOS default) swaps: naming the missing
            // direction could point at the one that works.
            let detail = if self.cfg.timing_reliable {
                let (seen, missing) = if up == 0 {
                    ("down", "up")
                } else {
                    ("up", "down")
                };
                format!(
                    "{} notches {} and none {}. If you also scrolled {}, that direction of the wheel may be dead; if you did not, it was not tested.",
                    total, seen, missing, missing
                )
            } else {
                format!(
                    "{} notches registered one way and none the other. If you also turned the wheel the other way, that direction may be dead; if you did not, it was not tested. Up and down are not named because the system's scroll-direction setting may have swapped them.",
                    total
                )
            };
            self.untested.push(Phase::Scroll);
            self.push(
                Severity::Info,
                Phase::Scroll,
                "Wheel only registered one direction",
                detail,
            );
        }
        // Like switch bounce, judged on the count. Three are needed because a
        // healthy wheel let go just past a detent can spring back one notch.
        let rate = reversals as f64 / total as f64 * 100.0;
        if reversals >= SKIP_FAULT_COUNT {
            self.push(
                Severity::Critical,
                Phase::Scroll,
                "Scroll wheel skipping",
                format!(
                    "{} notch(es) were reported in the opposite direction within {:.0} ms of the previous notch ({:.1}%). The encoder is worn or dirty and the page will jump back while scrolling.",
                    reversals, self.cfg.scroll_reversal_ms, rate
                ),
            );
        } else if reversals > 0 {
            self.push(
                Severity::Warning,
                Phase::Scroll,
                "Possible scroll wheel skip",
                format!(
                    "{} notch(es) reversed direction within {:.0} ms. Re-run the test; a repeat indicates a failing encoder.",
                    reversals, self.cfg.scroll_reversal_ms
                ),
            );
        }
    }

    // ── Lift ───────────────────────────────────────────────────────────

    /// DPI for lift-off distances, and whether it was assumed.
    fn lift_dpi(&self) -> (f64, bool) {
        liftoff::dpi_or_default(self.cfg.dpi)
    }

    /// The step's detector. It starts watching when the step does, without
    /// assuming the mouse was still then (the user may still be moving it).
    fn lift_detector(&mut self, phase_start: f64) -> &mut LiftDetector {
        let (dpi, _) = self.lift_dpi();
        self.lift
            .get_or_insert_with(|| LiftDetector::new(dpi).watching_from(phase_start))
    }

    fn record_lift_burst(&mut self, burst: Option<liftoff::Burst>) {
        if let Some(burst) = burst {
            let (dpi, _) = self.lift_dpi();
            self.lift_tally.record(&burst, dpi);
        }
    }

    fn feed_lift(&mut self, phase_start: f64, t: f64, event: &MouseEvent) {
        if let MouseEvent::Move { dx, dy } = *event {
            let burst = self.lift_detector(phase_start).feed(t, dx, dy);
            self.record_lift_burst(burst);
        }
    }

    fn tick_lift(&mut self, phase_start: f64, now: f64) {
        let burst = self.lift_detector(phase_start).tick(now);
        self.record_lift_burst(burst);
    }

    fn finalize_lift(&mut self) {
        let now = self.now;
        let burst = self.lift.as_mut().and_then(|d| d.tick(now));
        self.record_lift_burst(burst);
        // Motion still under way when the step ends may be the start of a
        // slide, so it is not judged; it still shows the user paused and
        // then moved the mouse.
        let unfinished = self.lift.as_ref().is_some_and(LiftDetector::in_burst);
        let resumes = self.lift_tally.bursts() + usize::from(unfinished);
        let n = self.lift_tally.jumps.len();
        let max_counts = self.lift_tally.max_jump_counts();
        let (dpi, assumed) = self.lift_dpi();
        let max_mm = counts_to_mm(max_counts, dpi);
        self.metrics.lift_jumps = n;
        self.metrics.lift_max_jump_counts = max_counts;
        self.metrics.lift_max_jump_mm = max_mm;
        self.metrics.lift_dpi = dpi;
        self.metrics.lift_dpi_assumed = assumed;
        let threshold = format!(
            "{:.1} mm within {:.0} ms ({:.0} counts at {:.0} DPI{})",
            JUMP_THRESHOLD_MM,
            JUMP_WINDOW_MS,
            mm_to_counts(JUMP_THRESHOLD_MM, dpi),
            dpi,
            if assumed { ", assumed" } else { "" }
        );
        let dpi_note = if assumed {
            format!(
                " The mouse's DPI was not given, so {:.0} DPI is assumed; if it is set differently, the distances are off by the same factor.",
                liftoff::DEFAULT_DPI
            )
        } else {
            String::new()
        };
        // Without a way to count lifts, one movement after a pause is the
        // least sign that the step was done at all.
        match liftoff::verdict(n, resumes, 1) {
            LiftVerdict::TooFewLifts => {
                // Without a pause followed by movement there was no lift or
                // set-down to check, so "no jumps" would claim something
                // never measured.
                self.untested.push(Phase::Lift);
                self.push(
                    Severity::Info,
                    Phase::Lift,
                    "Lift-off not tested",
                    "No movement after a pause was recorded, so cursor jumps on lift or set-down could not be checked. Lift the mouse, set it down, keep it still for a moment and then move it during this step.".to_string(),
                );
            }
            LiftVerdict::Clean => {
                // Sliding the mouse is no lift; say when nothing that looked
                // like a lift or set-down was seen at all.
                let seen = match self.lift_tally.quiet {
                    0 => "no small lift or set-down movement was seen, so either every lift was perfectly clean or the mouse was not lifted (the Lift-Off Jump test counts lifts for a definite result)".to_string(),
                    quiet => format!("{} lift or set-down movement(s) checked", quiet),
                };
                self.push(
                    Severity::Info,
                    Phase::Lift,
                    "No cursor jumps on lift",
                    format!(
                        "No movement after a pause carried the cursor more than {} and then stopped; {}. Sliding movements are not judged.{}",
                        threshold, seen, dpi_note
                    ),
                );
            }
            LiftVerdict::OneJump => self.push(
                Severity::Info,
                Phase::Lift,
                "One cursor jump after a pause",
                format!(
                    "One movement after a pause carried the cursor {:.1} mm and then stopped; the jump limit is {}. One jump is often the hand shifting the mouse on set-down; several indicate a high lift-off distance.{}",
                    max_mm, threshold, dpi_note
                ),
            ),
            // Without the DPI the counts are no distance: the 63 counts that
            // are 2 mm at the assumed 800 DPI are 0.5 mm at 3200 DPI, which
            // an ordinary set-down can shift the mouse. Such jumps leave the
            // step inconclusive instead of raising a warning.
            LiftVerdict::Jumps if assumed => {
                self.untested.push(Phase::Lift);
                self.push(
                    Severity::Info,
                    Phase::Lift,
                    "Possible cursor jumps on lift",
                    format!(
                        "{} movements after a pause carried the cursor more than {} and then stopped (largest {:.1} mm). The mouse's DPI was not given: if it is set to {:.0} DPI or lower, the sensor keeps tracking while lifted (lower the lift-off distance or use a plainer surface); at a higher DPI the same counts are a shorter distance and may be ordinary set-down shifts. Run the Lift-Off Jump test with the mouse's DPI set to tell them apart.",
                        n, threshold, max_mm, dpi
                    ),
                );
            }
            LiftVerdict::Jumps => self.push(
                Severity::Warning,
                Phase::Lift,
                "Cursor jumps on lift",
                format!(
                    "{} movements after a pause carried the cursor more than {} and then stopped (largest {:.1} mm). The sensor keeps tracking while lifted; lower the lift-off distance or use a plainer surface.{}",
                    n, threshold, max_mm, dpi_note
                ),
            ),
        }
    }

    fn finalize_phase(&mut self, phase: Phase) {
        match phase {
            Phase::Rest => self.finalize_rest(),
            Phase::Movement => self.finalize_movement(),
            Phase::Clicks => self.finalize_clicks(),
            Phase::Scroll => self.finalize_scroll(),
            Phase::Lift => self.finalize_lift(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::default()
    }

    /// Runs a well-behaved 1000 Hz mouse through every phase.
    struct Sim {
        t: f64,
        at: AutoTest,
    }

    impl Sim {
        fn new(cfg: Config) -> Self {
            let mut at = AutoTest::new(cfg);
            at.start(0.0);
            Sim { t: 0.0, at }
        }

        fn idle(&mut self, secs: f64) {
            let end = self.t + secs;
            while self.t < end {
                self.t += 0.05;
                self.at.tick(self.t);
            }
        }

        fn phase(&self) -> Option<Phase> {
            self.at.current_phase()
        }

        fn move_for(&mut self, secs: f64, interval_ms: f64) {
            let end = self.t + secs;
            while self.t < end {
                self.t += interval_ms / 1000.0;
                self.at.feed(self.t, &MouseEvent::Move { dx: 3, dy: -2 });
            }
        }

        fn click(&mut self, button: MouseButton, hold_ms: f64, gap_ms: f64) {
            self.at.feed(self.t, &MouseEvent::ButtonPress(button));
            self.t += hold_ms / 1000.0;
            self.at.feed(self.t, &MouseEvent::ButtonRelease(button));
            self.t += gap_ms / 1000.0;
            self.at.tick(self.t);
        }

        fn scroll(&mut self, delta: i32, gap_ms: f64) {
            self.at.feed(self.t, &MouseEvent::Scroll { delta });
            self.t += gap_ms / 1000.0;
            self.at.tick(self.t);
        }

        fn good_rest(&mut self) {
            assert_eq!(self.phase(), Some(Phase::Rest));
            let cfg = self.at.config();
            self.idle(cfg.rest_settle_secs + cfg.rest_secs + 0.1);
        }

        fn good_movement(&mut self) {
            assert_eq!(self.phase(), Some(Phase::Movement));
            self.move_for(4.0, 1.0);
            self.at.tick(self.t);
        }

        fn good_clicks(&mut self) {
            assert_eq!(self.phase(), Some(Phase::Clicks));
            for _ in 0..20 {
                self.click(MouseButton::Left, 70.0, 200.0);
            }
            for _ in 0..5 {
                self.click(MouseButton::Right, 80.0, 250.0);
            }
        }

        fn good_scroll(&mut self) {
            assert_eq!(self.phase(), Some(Phase::Scroll));
            for _ in 0..10 {
                self.scroll(-1, 90.0);
            }
            for _ in 0..10 {
                self.scroll(1, 90.0);
            }
        }

        /// Lifts and sets the mouse down three times; it shifts a fraction
        /// of a millimetre on landing. Each time it is then held still and
        /// moved in a small circle, as the instruction asks.
        fn good_lift(&mut self) {
            assert_eq!(self.phase(), Some(Phase::Lift));
            for _ in 0..3 {
                self.idle(0.8);
                self.set_down(&[(3, 1), (4, 0), (3, -1), (2, 0)], 2.0);
                self.idle(1.0);
                self.move_for(0.6, 1.0);
            }
            self.idle(10.5);
        }

        /// Reports `moves`, `interval_ms` apart, starting now.
        fn set_down(&mut self, moves: &[(i32, i32)], interval_ms: f64) {
            for &(dx, dy) in moves {
                self.t += interval_ms / 1000.0;
                self.at.feed(self.t, &MouseEvent::Move { dx, dy });
            }
        }
    }

    /// Feeds `secs` of circular motion the way a real mouse reports it: the
    /// sensor moves `speed` counts/s along a circle, the host polls at
    /// `poll_hz`, and a report is sent only on polls where a whole count
    /// accrued. `drop_every`/`drop_len` make the link lose `drop_len`
    /// consecutive polls every `drop_every` polls (the counts carry over).
    fn circle_stream(
        sim: &mut Sim,
        secs: f64,
        poll_hz: f64,
        speed: f64,
        drop_every: usize,
        drop_len: usize,
    ) {
        let period_s = 1.5;
        let omega = 2.0 * std::f64::consts::PI / period_s;
        let radius = speed / omega;
        let t0 = sim.t;
        let polls = (secs * poll_hz) as usize;
        let pos = |t: f64| {
            let a = omega * (t - t0);
            (
                (radius * a.cos()).floor() as i64,
                (radius * a.sin()).floor() as i64,
            )
        };
        let (mut rx, mut ry) = pos(t0);
        for k in 1..=polls {
            let t = t0 + k as f64 / poll_hz;
            if drop_every > 0 && k % drop_every < drop_len {
                continue;
            }
            let (x, y) = pos(t);
            if x != rx || y != ry {
                sim.t = t;
                sim.at.feed(
                    t,
                    &MouseEvent::Move {
                        dx: (x - rx) as i32,
                        dy: (y - ry) as i32,
                    },
                );
                rx = x;
                ry = y;
            }
        }
        sim.t = t0 + secs;
        sim.at.tick(sim.t);
    }

    /// Feeds `secs` of hand movement along `pos(t)` (counts, `t` in seconds
    /// from now) the way a real mouse reports it: polled at `poll_hz`, a
    /// report only once a whole count has built up, the fraction kept.
    /// Polls where `lost(t)` is true are not delivered (counts carry over).
    fn path_stream(
        sim: &mut Sim,
        secs: f64,
        poll_hz: f64,
        pos: impl Fn(f64) -> (f64, f64),
        lost: impl Fn(f64) -> bool,
    ) {
        let t0 = sim.t;
        let (mut rx, mut ry) = (0i64, 0i64);
        for k in 1..=(secs * poll_hz) as usize {
            let t = k as f64 / poll_hz;
            if lost(t) {
                continue;
            }
            let (x, y) = pos(t);
            let dx = (x - rx as f64).trunc() as i64;
            let dy = (y - ry as f64).trunc() as i64;
            if dx != 0 || dy != 0 {
                rx += dx;
                ry += dy;
                sim.t = t0 + t;
                sim.at.feed(
                    sim.t,
                    &MouseEvent::Move {
                        dx: dx as i32,
                        dy: dy as i32,
                    },
                );
            }
        }
        sim.t = t0 + secs;
        sim.at.tick(sim.t);
    }

    /// Back and forth along X: smooth (minimum-jerk) strokes of `stroke`
    /// counts lasting `stroke_s`, with a dwell of `dwell(i)` seconds at the
    /// end of the i-th stroke, and a slight drift on Y.
    fn back_and_forth_with_dwells(
        stroke: f64,
        stroke_s: f64,
        dwell: impl Fn(usize) -> f64,
    ) -> impl Fn(f64) -> (f64, f64) {
        let mut plan = Vec::new();
        let mut t = 0.0;
        for i in 0..200 {
            plan.push((t, i));
            t += stroke_s + dwell(i);
        }
        move |time: f64| {
            let &(start, i) = plan.iter().rev().find(|(s, _)| *s <= time).unwrap();
            let s = ((time - start) / stroke_s).min(1.0);
            let done = stroke * (10.0 * s.powi(3) - 15.0 * s.powi(4) + 6.0 * s.powi(5));
            // Strokes end between counts, as a real hand's do
            let x = 0.4 + if i % 2 == 0 { done } else { stroke - done };
            (x, 0.02 * stroke * (time * 0.7).sin())
        }
    }

    #[test]
    fn test_back_and_forth_with_dwells_is_not_a_stutter() {
        // A hand moving side to side at about 1.5 Hz (80 mm strokes, 0.3 s
        // each) instead of circling, resting 30-60 ms at each end, with
        // the mouse at 1000, 1600 and 3200 DPI.
        for (hz, stroke) in [(125.0, 3150.3), (250.0, 5040.3), (500.0, 10_080.3)] {
            let mut sim = Sim::new(cfg());
            sim.good_rest();
            let dwell = |i: usize| 0.03 + 0.003 * (i % 11) as f64;
            let pos = back_and_forth_with_dwells(stroke, 0.3, dwell);
            path_stream(&mut sim, 12.5, hz, pos, |_| false);
            assert_eq!(sim.phase(), Some(Phase::Clicks), "{} Hz", hz);
            let f = movement_findings(&sim);
            assert!(
                f.iter().all(|(s, _)| *s == Severity::Info),
                "{} Hz flagged: {:?}",
                hz,
                f
            );
            assert_eq!(sim.at.metrics.stutter_count, 0, "{} Hz", hz);
            assert_eq!(sim.at.metrics.polling_nominal_hz, Some(hz as u32));
        }
    }

    /// Circles of `radius` counts, one per second of movement, where once a
    /// second the hand slows to a stop over 30 ms, rests `pause` seconds and
    /// carries on the same way.
    fn circles_with_stops(radius: f64, pause: f64) -> impl Fn(f64) -> (f64, f64) {
        use std::f64::consts::{PI, TAU};
        let ramp = 0.03;
        let cycle = 1.0 + pause + ramp;
        let wave = move |w: f64| ramp / TAU * (PI * w / ramp).sin();
        // Seconds of full-speed movement covered by time `t`
        let travelled = move |t: f64| {
            let (n, f) = ((t / cycle).floor(), t % cycle);
            let part = if f < 1.0 - ramp {
                f
            } else if f < 1.0 {
                let w = f - (1.0 - ramp);
                1.0 - ramp + w / 2.0 + wave(w)
            } else if f < 1.0 + pause {
                1.0 - ramp / 2.0
            } else {
                let w = f - 1.0 - pause;
                1.0 - ramp / 2.0 + w / 2.0 - wave(w)
            };
            n * 1.0 + part
        };
        move |t: f64| {
            let a = TAU * travelled(t);
            (radius * a.cos(), radius * a.sin())
        }
    }

    #[test]
    fn test_circles_with_hesitations_are_not_a_stutter() {
        // Brisk circles at 125 Hz with the hand stopping for 40 ms once a
        // second and carrying on the same way.
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        path_stream(
            &mut sim,
            12.5,
            125.0,
            circles_with_stops(600.0, 0.04),
            |_| false,
        );
        let f = movement_findings(&sim);
        assert!(f.iter().all(|(s, _)| *s == Severity::Info), "{:?}", f);
        assert_eq!(sim.at.metrics.polling_nominal_hz, Some(125));
    }

    #[test]
    fn test_longer_stops_are_not_freezes() {
        // The hand stops for 0.3 s once a second: at 125 Hz and 26000 DPI
        // the reports either side still carry 2+ counts.
        for (hz, radius) in [(125.0, 30_000.0), (1000.0, 600.0)] {
            let mut sim = Sim::new(cfg());
            sim.good_rest();
            path_stream(&mut sim, 12.5, hz, circles_with_stops(radius, 0.3), |_| {
                false
            });
            let f = movement_findings(&sim);
            assert!(f.iter().all(|(s, _)| *s == Severity::Info), "{:?}", f);
            assert_eq!(sim.at.metrics.movement_freezes, 0, "{} Hz", hz);
        }
    }

    #[test]
    fn test_freezes_while_moving_are_flagged() {
        // Brisk circles while the link drops out for 150-400 ms every 2 s
        // (a wireless receiver next to a USB 3 port), at 1 kHz and 125 Hz.
        let lost = |t: f64| {
            let n = (t / 2.0).floor();
            let start = n * 2.0 + 1.0;
            (start..start + 0.15 + 0.05 * (n % 6.0)).contains(&t)
        };
        for (hz, radius) in [(1000.0, 600.0), (125.0, 400.0)] {
            let mut sim = Sim::new(cfg());
            sim.good_rest();
            let circle = move |t: f64| {
                let a = std::f64::consts::TAU * t;
                (radius * a.cos(), radius * a.sin())
            };
            path_stream(&mut sim, 12.5, hz, circle, lost);
            let f = movement_findings(&sim);
            assert!(
                f.contains(&(Severity::Warning, "Movement froze".to_string())),
                "{} Hz: {:?}",
                hz,
                f
            );
            // At 1 kHz the step ends once 3 s of fast movement are timed
            let freezes = sim.at.metrics.movement_freezes;
            assert!((2..=6).contains(&freezes), "{} Hz: {}", hz, freezes);
        }

        // One freeze (a lift while moving looks the same) is only noted
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        let circle = |t: f64| {
            let a = std::f64::consts::TAU * t;
            (600.0 * a.cos(), 600.0 * a.sin())
        };
        path_stream(&mut sim, 12.5, 1000.0, circle, |t| (2.0..2.3).contains(&t));
        let f = movement_findings(&sim);
        assert!(f.iter().all(|(s, _)| *s == Severity::Info), "{:?}", f);
        assert!(f.iter().any(|(_, t)| t == "One freeze during movement"));
    }

    fn movement_findings(sim: &Sim) -> Vec<(Severity, String)> {
        sim.at
            .findings_so_far()
            .iter()
            .filter(|f| f.phase == Phase::Movement)
            .map(|f| (f.severity, f.title.clone()))
            .collect()
    }

    fn titles(report: &Report) -> Vec<&str> {
        report.findings.iter().map(|f| f.title.as_str()).collect()
    }

    #[test]
    fn test_phase_order_and_titles() {
        let at = AutoTest::new(cfg());
        assert_eq!(
            at.phases(),
            &[
                Phase::Rest,
                Phase::Movement,
                Phase::Clicks,
                Phase::Scroll,
                Phase::Lift
            ]
        );
        for p in at.phases() {
            assert!(!p.title().is_empty());
            assert!(!p.instruction().is_empty());
        }
        assert!(!at.is_running());
        assert!(at.status().is_none());
    }

    #[test]
    fn test_unreliable_timing_skips_movement_phase() {
        let at = AutoTest::new(Config {
            timing_reliable: false,
            ..cfg()
        });
        assert!(!at.phases().contains(&Phase::Movement));
        // Lift-off needs raw sensor counts, not accelerated pointer deltas
        assert!(!at.phases().contains(&Phase::Lift));
    }

    #[test]
    fn test_healthy_mouse_full_run() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        sim.good_clicks();
        sim.good_scroll();
        sim.good_lift();
        assert!(sim.at.is_finished());
        let r = sim.at.report().expect("report");
        assert_eq!(r.verdict, Verdict::Healthy, "findings: {:?}", titles(r));
        assert_eq!(r.count(Severity::Critical), 0);
        assert_eq!(r.count(Severity::Warning), 0);
        assert!(!r.aborted);
        assert_eq!(r.phases_completed.len(), 5);
        assert_eq!(r.metrics.polling_hz, Some(1000));
        assert_eq!(r.metrics.polling_nominal_hz, Some(1000));
        assert_eq!(r.metrics.left_clicks, 20);
        assert_eq!(r.metrics.right_clicks, 5);
        assert_eq!(r.metrics.scroll_up, 10);
        assert_eq!(r.metrics.scroll_down, 10);
        assert_eq!(r.metrics.scroll_reversals, 0);
        assert_eq!(r.metrics.lift_jumps, 0);
        assert_eq!(r.metrics.rest_move_events, 0);
        assert!(r.duration_secs > 20.0);
        // Findings are sorted most severe first; all Info here
        assert!(r.findings.iter().all(|f| f.severity == Severity::Info));
    }

    #[test]
    fn test_rest_phase_ignores_settle_window_but_flags_jitter_after() {
        let mut sim = Sim::new(cfg());
        // Within the 2 s settle window: ignored
        sim.t = 0.5;
        sim.at.feed(sim.t, &MouseEvent::Move { dx: 30, dy: 30 });
        sim.at
            .feed(sim.t, &MouseEvent::ButtonRelease(MouseButton::Left));
        sim.t = 1.9;
        sim.at.feed(sim.t, &MouseEvent::Move { dx: -40, dy: 5 });
        // After settle: single-count jitter throughout the rest window
        sim.t = 2.2;
        for _ in 0..10 {
            sim.at.feed(sim.t, &MouseEvent::Move { dx: 1, dy: 0 });
            sim.t += 0.45;
        }
        sim.idle(3.0);
        assert_eq!(sim.phase(), Some(Phase::Movement));
        let f = sim.at.findings_so_far();
        assert!(f
            .iter()
            .any(|f| f.title == "Sensor jitter at rest" && f.severity == Severity::Warning));
        assert_eq!(sim.at.metrics.rest_move_events, 10);
        assert_eq!(sim.at.metrics.rest_motion_seconds, 5);
    }

    #[test]
    fn test_rest_phantom_click_and_drift_are_critical() {
        let mut sim = Sim::new(cfg());
        sim.t = 2.5;
        sim.at
            .feed(sim.t, &MouseEvent::ButtonPress(MouseButton::Left));
        sim.at
            .feed(sim.t + 0.05, &MouseEvent::ButtonRelease(MouseButton::Left));
        for i in 0..40 {
            sim.at
                .feed(sim.t + 0.1 * i as f64, &MouseEvent::Move { dx: 1, dy: 1 });
        }
        sim.at.feed(sim.t + 4.0, &MouseEvent::Scroll { delta: 1 });
        sim.idle(6.5);
        let f = sim.at.findings_so_far();
        assert!(f
            .iter()
            .any(|f| f.title == "Phantom clicks" && f.severity == Severity::Critical));
        assert!(f
            .iter()
            .any(|f| f.title == "Sensor moves on its own" && f.severity == Severity::Critical));
        assert!(f
            .iter()
            .any(|f| f.title == "Phantom scroll events" && f.severity == Severity::Warning));
    }

    #[test]
    fn test_movement_stutters_detected() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        // 1000 Hz moving fast (several counts per poll) with a 10 ms stall
        // every 20 reports (5% stutters)
        let mut n = 0;
        while sim.at.current_phase() == Some(Phase::Movement) && n < 20_000 {
            n += 1;
            sim.t += if n % 20 == 0 { 0.010 } else { 0.001 };
            sim.at.feed(sim.t, &MouseEvent::Move { dx: 4, dy: 3 });
        }
        let f = sim.at.findings_so_far();
        assert!(
            f.iter()
                .any(|f| f.title == "Frequent stutters" || f.title == "Occasional stutters"),
            "{:?}",
            f.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert!(sim.at.metrics.stutter_count > 0);
        assert_eq!(sim.at.metrics.polling_hz, Some(1000));
    }

    #[test]
    fn test_movement_low_rate_and_pauses() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        // 40 Hz mouse with a 2 s dropout in the middle
        sim.move_for(3.0, 25.0);
        sim.t += 2.0;
        sim.move_for(3.0, 25.0);
        sim.idle(12.0);
        assert_eq!(sim.at.metrics.polling_hz, Some(40));
        assert!(movement_findings(&sim)
            .contains(&(Severity::Warning, "Very low polling rate".to_string())));
        let f = sim.at.findings_so_far();
        assert!(f.iter().any(|f| f.title == "Pauses during movement"));
        assert_eq!(sim.at.metrics.movement_pauses, 1);
    }

    #[test]
    fn test_bluetooth_polling_rate_is_noted_not_flagged() {
        // A healthy Bluetooth mouse reports once per 11.25 ms connection
        // interval (89 Hz), moved briskly for the whole step
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        while sim.phase() == Some(Phase::Movement) {
            sim.move_for(0.5, 11.25);
            sim.at.tick(sim.t);
        }
        assert_eq!(sim.at.metrics.polling_hz, Some(89));
        let movement = movement_findings(&sim);
        assert!(
            movement.iter().all(|(s, _)| *s == Severity::Info),
            "{:?}",
            movement
        );
        assert!(movement
            .iter()
            .any(|(_, t)| t == "Low polling rate (normal for Bluetooth)"));
        // No USB rate fits 89 Hz, so none is named: "looks like a 125 Hz
        // mouse" would read as a USB mouse running slow
        assert_eq!(sim.at.metrics.polling_nominal_hz, None);
        let measured = sim
            .at
            .findings_so_far()
            .iter()
            .find(|f| f.title == "Polling rate measured")
            .map(|f| f.detail.clone())
            .unwrap();
        assert!(measured.contains("= 89 Hz,"), "{}", measured);
        assert!(!measured.contains("looks like"), "{}", measured);
        sim.good_clicks();
        sim.good_scroll();
        sim.good_lift();
        let r = sim.at.report().unwrap();
        assert_eq!(r.verdict, Verdict::Healthy, "findings: {:?}", titles(r));
    }

    #[test]
    fn test_low_polling_rate_thresholds() {
        let sev = |hz| low_polling_finding(hz).map(|(s, _, _)| s);
        // Bluetooth connection intervals of 7.5-15 ms: 133-67 Hz
        assert_eq!(sev(133), None);
        assert_eq!(sev(89), Some(Severity::Info));
        assert_eq!(sev(67), Some(Severity::Info));
        // Fewer reports than a 60 Hz screen shows frames
        assert_eq!(sev(59), Some(Severity::Warning));
        assert_eq!(sev(30), Some(Severity::Warning));
        // USB mice: 125 Hz and up, with room for timing error
        assert_eq!(sev(125), None);
        assert_eq!(sev(110), None);
        assert_eq!(sev(109), Some(Severity::Info));
        let (_, _, detail) = low_polling_finding(100).unwrap();
        assert!(detail.contains("Bluetooth") && detail.contains("125 Hz"));
    }

    #[test]
    fn test_movement_insufficient_data_is_inconclusive() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.move_for(0.05, 1.0); // ~50 reports only
        sim.idle(12.5);
        assert_eq!(sim.phase(), Some(Phase::Clicks));
        assert!(sim
            .at
            .findings_so_far()
            .iter()
            .any(|f| f.title == "Not enough fast movement recorded"));
        sim.good_clicks();
        sim.good_scroll();
        sim.good_lift();
        assert_eq!(sim.at.report().unwrap().verdict, Verdict::Inconclusive);
    }

    #[test]
    fn test_switch_bounce_is_critical() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        assert_eq!(sim.phase(), Some(Phase::Clicks));
        for i in 0..20 {
            sim.click(MouseButton::Left, 60.0, 200.0);
            if i % 4 == 0 {
                // bounce: another press 8 ms after the previous release
                sim.t -= 0.192;
                sim.click(MouseButton::Left, 2.0, 200.0);
            }
        }
        for _ in 0..5 {
            sim.click(MouseButton::Right, 80.0, 200.0);
        }
        let f = sim.at.findings_so_far();
        assert!(f
            .iter()
            .any(|f| f.title.starts_with("Switch bounce") && f.severity == Severity::Critical));
        assert!(f
            .iter()
            .any(|f| f.title == "Extremely short presses" && f.severity == Severity::Warning));
        assert!(sim.at.metrics.bounce_events >= 5);
        assert!(sim.at.metrics.micro_presses >= 5);
        assert!(sim.at.metrics.min_press_interval_ms.unwrap() < 80.0);
    }

    #[test]
    fn test_press_edge_bounce_counts_by_press_interval() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        // Each physical click registers as a 1 ms blip then the real 60 ms press
        for _ in 0..10 {
            sim.click(MouseButton::Left, 1.0, 30.0);
            sim.click(MouseButton::Left, 60.0, 200.0);
        }
        assert_eq!(sim.at.buttons[0].bounces, 10);
        assert_eq!(sim.at.buttons[0].micro_presses, 10);
    }

    #[test]
    fn test_fast_but_human_clicking_is_not_bounce() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        // 8 clicks per second: 60 ms hold, 65 ms gap -> 125 ms press-to-press
        for _ in 0..20 {
            sim.click(MouseButton::Left, 60.0, 65.0);
        }
        for _ in 0..5 {
            sim.click(MouseButton::Right, 60.0, 65.0);
        }
        assert_eq!(sim.at.metrics.bounce_events, 0);
        assert_eq!(sim.at.metrics.micro_presses, 0);
    }

    #[test]
    fn test_sticky_and_stuck_buttons() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        for i in 0..19 {
            let hold = if i < 3 { 400.0 } else { 70.0 };
            sim.click(MouseButton::Left, hold, 150.0);
        }
        // Final press never releases
        sim.at
            .feed(sim.t, &MouseEvent::ButtonPress(MouseButton::Left));
        sim.idle(21.0);
        assert_eq!(sim.phase(), Some(Phase::Scroll));
        let f = sim.at.findings_so_far();
        assert!(f
            .iter()
            .any(|f| f.title == "Slow or sticky releases" && f.severity == Severity::Warning));
        assert!(f
            .iter()
            .any(|f| f.title == "Button stuck down" && f.severity == Severity::Critical));
        assert!(f.iter().any(|f| f.title == "No right clicks registered"));
        assert_eq!(sim.at.metrics.sticky_releases, 3);
        assert_eq!(sim.at.metrics.stuck_buttons, 1);
    }

    #[test]
    fn test_orphan_release_is_ignored_and_few_clicks_inconclusive() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        sim.at
            .feed(sim.t, &MouseEvent::ButtonRelease(MouseButton::Left));
        sim.click(MouseButton::Left, 60.0, 100.0);
        sim.idle(21.0);
        assert_eq!(sim.at.metrics.left_clicks, 1);
        assert!(sim
            .at
            .findings_so_far()
            .iter()
            .any(|f| f.title == "Not enough clicks recorded"));
    }

    #[test]
    fn test_scroll_reversals_and_one_direction() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        sim.good_clicks();
        assert_eq!(sim.phase(), Some(Phase::Scroll));
        // Only downward scrolling, with encoder skips every 3rd notch
        for i in 0..15 {
            sim.scroll(-1, 80.0);
            if i % 3 == 0 {
                sim.scroll(1, 10.0); // wrong-direction notch 10 ms later
                sim.scroll(-1, 80.0);
            }
        }
        sim.idle(13.0);
        assert_eq!(sim.phase(), Some(Phase::Lift));
        let f = sim.at.findings_so_far();
        assert!(f
            .iter()
            .any(|f| f.title == "Scroll wheel skipping" && f.severity == Severity::Critical));
        // "up" notches did occur (the skips), so no one-direction warning here
        assert!(sim.at.metrics.scroll_reversals >= 5);
    }

    #[test]
    fn test_one_wrong_way_notch_is_one_skip() {
        // Scrolling down a notch every 40 ms; the encoder bounces and sends
        // one wrong-way notch 2 ms after a real one. The next real notch,
        // 38 ms later, is the hand carrying on, not a second skip.
        let glitches = |n: usize| {
            let mut sim = Sim::new(cfg());
            sim.good_rest();
            sim.good_movement();
            sim.good_clicks();
            assert_eq!(sim.phase(), Some(Phase::Scroll));
            for i in 0..12 {
                sim.scroll(-1, if i % 5 == 2 && i / 5 < n { 2.0 } else { 40.0 });
                if i % 5 == 2 && i / 5 < n {
                    sim.scroll(1, 38.0);
                }
            }
            sim.idle(0.5);
            for _ in 0..10 {
                sim.scroll(1, 90.0);
            }
            sim.idle(13.0);
            sim
        };
        let sim = glitches(1);
        assert_eq!(sim.at.metrics.scroll_reversals, 1);
        let f = sim.at.findings_so_far();
        assert!(f
            .iter()
            .any(|f| f.title == "Possible scroll wheel skip" && f.detail.starts_with("1 notch")));
        // Two glitches are two skips: a re-test, as in the Scroll Wheel test
        let sim = glitches(2);
        assert_eq!(sim.at.metrics.scroll_reversals, 2);
        assert!(!sim
            .at
            .findings_so_far()
            .iter()
            .any(|f| f.severity == Severity::Critical));
    }

    #[test]
    fn test_scroll_dead_direction() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        sim.good_clicks();
        for _ in 0..12 {
            sim.scroll(-1, 80.0);
        }
        sim.idle(13.0);
        let f = sim.at.findings_so_far();
        let dead = f
            .iter()
            .find(|f| f.title == "Wheel only registered one direction")
            .unwrap();
        assert!(dead.detail.starts_with("12 notches down and none up."));
        assert_eq!(sim.at.metrics.scroll_reversals, 0);
    }

    #[test]
    fn test_scroll_dead_direction_without_raw_input_is_not_named() {
        // Framework input: natural scrolling (the macOS default) swaps up
        // and down, so a dead physical "down" arrives as up notches only
        let mut sim = Sim::new(Config {
            timing_reliable: false,
            ..cfg()
        });
        sim.good_rest();
        sim.good_clicks();
        for _ in 0..12 {
            sim.scroll(1, 80.0);
        }
        sim.idle(13.0);
        let f = sim.at.findings_so_far();
        let dead = f
            .iter()
            .find(|f| f.title == "Wheel only registered one direction")
            .unwrap();
        assert!(dead
            .detail
            .starts_with("12 notches registered one way and none the other."));
        assert!(!dead.detail.contains("none down"), "{}", dead.detail);
    }

    #[test]
    fn test_scroll_multi_notch_deltas_count_each_notch() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        sim.good_clicks();
        sim.scroll(-3, 100.0);
        sim.scroll(4, 100.0);
        assert_eq!(sim.at.metrics.scroll_down, 3);
        assert_eq!(sim.at.metrics.scroll_up, 4);
    }

    #[test]
    fn test_lift_jumps_flagged() {
        let mut sim = Sim::new(Config {
            dpi: Some(800.0),
            ..cfg()
        });
        sim.good_rest();
        sim.good_movement();
        sim.good_clicks();
        sim.good_scroll();
        assert_eq!(sim.phase(), Some(Phase::Lift));
        for _ in 0..3 {
            // The cursor jumps 100 counts (3.2 mm at 800 DPI) on set-down,
            // then the mouse is held still and moved in a circle.
            sim.idle(0.5);
            sim.at.feed(sim.t, &MouseEvent::Move { dx: 80, dy: -60 });
            sim.idle(1.0);
            sim.move_for(0.3, 1.0);
        }
        sim.idle(10.5);
        let r = sim.at.report().unwrap();
        assert_eq!(r.metrics.lift_jumps, 3, "{:?}", titles(r));
        assert!((r.metrics.lift_max_jump_counts - 100.0).abs() < 1e-9);
        assert!((r.metrics.lift_max_jump_mm - 3.175).abs() < 1e-9);
        assert!(!r.metrics.lift_dpi_assumed);
        assert!(r
            .findings
            .iter()
            .any(|f| f.title == "Cursor jumps on lift" && f.severity == Severity::Warning));
        assert_eq!(r.verdict, Verdict::Attention);
    }

    #[test]
    fn test_lift_jump_spread_over_many_reports_is_flagged() {
        // A set-down jump of 200 counts (6.4 mm at 800 DPI) delivered at
        // 1000 Hz as 10 counts per report. No single report is large.
        let mut sim = sim_at_lift(Config {
            dpi: Some(800.0),
            ..cfg()
        });
        for _ in 0..3 {
            sim.idle(0.6);
            sim.set_down(&[(10, 0); 20], 1.0);
            sim.idle(1.0);
            sim.move_for(0.5, 1.0);
        }
        sim.idle(10.5);
        let r = sim.at.report().unwrap();
        assert_eq!(r.metrics.lift_jumps, 3, "{:?}", titles(r));
        assert!(r
            .findings
            .iter()
            .any(|f| f.title == "Cursor jumps on lift" && f.severity == Severity::Warning));
    }

    #[test]
    fn test_lift_normal_starts_at_125hz_are_not_jumps() {
        // A 125 Hz mouse: the first report of an ordinary movement after a
        // pause already carries 60 counts. Starting to move is not a jump.
        let mut sim = sim_at_lift(cfg());
        for _ in 0..3 {
            sim.idle(0.6);
            sim.set_down(&[(60, 5); 60], 8.0);
        }
        sim.idle(10.5);
        let r = sim.at.report().unwrap();
        assert_eq!(r.metrics.lift_jumps, 0, "{:?}", titles(r));
        assert_eq!(r.count(Severity::Warning), 0, "{:?}", titles(r));
        // Only sliding was seen: the finding does not claim lifts were clean
        assert!(lift_finding(r)
            .detail
            .contains("or the mouse was not lifted"));
    }

    /// A run that reaches the lift step with healthy earlier steps.
    fn sim_at_lift(cfg: Config) -> Sim {
        let mut sim = Sim::new(cfg);
        sim.good_rest();
        sim.good_movement();
        sim.good_clicks();
        sim.good_scroll();
        assert_eq!(sim.phase(), Some(Phase::Lift));
        sim
    }

    fn lift_finding(r: &Report) -> &Finding {
        r.findings
            .iter()
            .find(|f| f.phase == Phase::Lift)
            .expect("a lift finding")
    }

    #[test]
    fn test_lift_threshold_uses_configured_dpi() {
        // Three 100-count set-down shifts: 3.2 mm at 800 DPI (jumps), but
        // only 1.6 mm at 1600 DPI (a normal set-down).
        let lifts = |sim: &mut Sim| {
            for _ in 0..3 {
                sim.idle(0.5);
                sim.set_down(&[(20, 0); 5], 1.0);
                sim.idle(1.0);
                sim.move_for(0.3, 1.0);
            }
            sim.idle(10.5);
        };
        let mut sim = sim_at_lift(Config {
            dpi: Some(800.0),
            ..cfg()
        });
        lifts(&mut sim);
        let r = sim.at.report().unwrap();
        assert_eq!(r.metrics.lift_jumps, 3);
        assert_eq!(lift_finding(r).title, "Cursor jumps on lift");
        assert!(!lift_finding(r).detail.contains("assumed"));

        let mut sim = sim_at_lift(Config {
            dpi: Some(1600.0),
            ..cfg()
        });
        lifts(&mut sim);
        let r = sim.at.report().unwrap();
        assert_eq!(r.metrics.lift_jumps, 0);
        assert_eq!(r.verdict, Verdict::Healthy, "{:?}", titles(r));
        assert_eq!(lift_finding(r).title, "No cursor jumps on lift");
        assert!(lift_finding(r)
            .detail
            .contains("3 lift or set-down movement(s) checked"));
        assert!(!lift_finding(r).detail.contains("assumed"));
        assert!(!r.metrics.lift_dpi_assumed);
        assert_eq!(r.metrics.lift_dpi, 1600.0);
    }

    #[test]
    fn test_lift_jumps_with_dpi_unknown_are_inconclusive_not_a_warning() {
        // A healthy 3200 DPI mouse at 1000 Hz shifts 0.6 mm (75 counts) on
        // each set-down. Taken for 800 DPI that is 2.4 mm, a "jump"; without
        // the DPI the step cannot tell, so it must not raise a warning.
        let lifts = |sim: &mut Sim| {
            for _ in 0..3 {
                sim.idle(0.8);
                sim.set_down(&[(3, 0); 25], 1.0);
                sim.idle(1.0);
                sim.move_for(0.6, 1.0);
            }
            sim.idle(10.5);
        };
        let mut sim = sim_at_lift(cfg());
        lifts(&mut sim);
        let r = sim.at.report().unwrap();
        assert_eq!(r.metrics.lift_jumps, 3, "{:?}", titles(r));
        assert_eq!(r.count(Severity::Warning), 0, "{:?}", titles(r));
        let f = lift_finding(r);
        assert_eq!(
            (f.severity, f.title.as_str()),
            (Severity::Info, "Possible cursor jumps on lift")
        );
        assert!(f.detail.contains("DPI was not given"), "{}", f.detail);
        assert_eq!(r.verdict, Verdict::Inconclusive);

        // With its DPI given, the same motion is a set of clean set-downs.
        let mut sim = sim_at_lift(Config {
            dpi: Some(3200.0),
            ..cfg()
        });
        lifts(&mut sim);
        let r = sim.at.report().unwrap();
        assert_eq!(r.metrics.lift_jumps, 0, "{:?}", titles(r));
        assert_eq!(lift_finding(r).title, "No cursor jumps on lift");
        assert_eq!(r.verdict, Verdict::Healthy, "{:?}", titles(r));
    }

    #[test]
    fn test_one_lift_jump_is_only_noted() {
        let mut sim = sim_at_lift(cfg());
        sim.idle(0.5);
        sim.set_down(&[(10, 0); 20], 1.0);
        sim.idle(1.0);
        sim.move_for(0.5, 1.0);
        sim.idle(10.5);
        let r = sim.at.report().unwrap();
        assert_eq!(r.metrics.lift_jumps, 1);
        let f = lift_finding(r);
        assert_eq!(
            (f.severity, f.title.as_str()),
            (Severity::Info, "One cursor jump after a pause")
        );
        assert_eq!(r.verdict, Verdict::Healthy, "{:?}", titles(r));
    }

    #[test]
    fn test_moving_through_the_whole_lift_step_is_not_tested() {
        // Never paused: there was no lift or set-down to check.
        let mut sim = sim_at_lift(cfg());
        sim.move_for(10.5, 1.0);
        sim.idle(0.5);
        let r = sim.at.report().unwrap();
        assert_eq!(lift_finding(r).title, "Lift-off not tested");
        assert_eq!(r.verdict, Verdict::Inconclusive);
    }

    #[test]
    fn test_motion_cut_off_by_the_step_end_is_not_judged() {
        // A 3 mm movement starts 30 ms before the step ends: it may be the
        // start of a slide, so it is not called a jump, but it does show
        // the user paused and then moved.
        let mut sim = sim_at_lift(cfg());
        let remaining = sim.at.status().unwrap().remaining_secs;
        sim.t += remaining - 0.03;
        sim.at.tick(sim.t);
        sim.set_down(&[(20, 0); 5], 1.0);
        sim.idle(0.5);
        let r = sim.at.report().unwrap();
        assert_eq!(r.metrics.lift_jumps, 0);
        assert_eq!(lift_finding(r).title, "No cursor jumps on lift");
    }

    #[test]
    fn test_abort_produces_partial_report() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        sim.at.abort();
        assert!(sim.at.is_finished());
        let r = sim.at.report().unwrap();
        assert!(r.aborted);
        assert_eq!(r.phases_completed, vec![Phase::Rest, Phase::Movement]);
        assert_eq!(r.verdict, Verdict::Inconclusive);
        // Aborting twice or feeding after finish is harmless
        sim.at.abort();
        sim.at.feed(sim.t + 1.0, &MouseEvent::Move { dx: 1, dy: 0 });
        assert!(sim.at.is_finished());
    }

    #[test]
    fn test_status_progress_and_detail() {
        let mut sim = Sim::new(cfg());
        let s = sim.at.status().unwrap();
        assert_eq!(s.phase, Phase::Rest);
        assert_eq!(s.index, 0);
        assert_eq!(s.total, 5);
        assert!(s.progress < 0.01);
        // Rest lasts 2 s settle + 5 s
        sim.idle(3.5);
        let s = sim.at.status().unwrap();
        assert!(s.progress > 0.4 && s.progress < 0.6);
        assert!(s.remaining_secs > 3.0 && s.remaining_secs < 4.0);
        sim.idle(3.7);
        sim.move_for(1.6, 1.0);
        let s = sim.at.status().unwrap();
        assert_eq!(s.phase, Phase::Movement);
        assert!(s.detail.contains("Hz"));
        assert!(!s.detail.contains("move faster"));
        // data progress (1600/3000 reports, 1.6/3 s) beats time progress (1.6/12)
        assert!(s.progress > 0.5, "{}", s.progress);
    }

    #[test]
    fn test_restart_clears_previous_run() {
        let mut sim = Sim::new(cfg());
        sim.t = 2.0;
        sim.at
            .feed(sim.t, &MouseEvent::ButtonPress(MouseButton::Left));
        sim.at.start(100.0);
        assert_eq!(sim.at.metrics.rest_clicks, 0);
        assert!(sim.at.findings_so_far().is_empty());
        assert_eq!(sim.at.current_phase(), Some(Phase::Rest));
    }

    #[test]
    fn test_report_serializes_to_json() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.at.abort();
        let json = serde_json::to_string(sim.at.report().unwrap()).unwrap();
        assert!(json.contains("\"verdict\":\"Inconclusive\""));
        assert!(json.contains("\"phases_completed\":[\"Rest\"]"));
    }

    // ── regression tests for audit findings ────────────────────────────

    #[test]
    fn test_slow_movement_on_healthy_1khz_mouse_is_not_flagged() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        // 300 counts/s: under one count per poll, so reports come every 2-5 ms
        circle_stream(&mut sim, 2.0, 1000.0, 300.0, 0, 0);
        let status = sim.at.status().unwrap();
        assert!(status.detail.contains("move faster"), "{}", status.detail);
        circle_stream(&mut sim, 10.5, 1000.0, 300.0, 0, 0);
        assert_eq!(sim.phase(), Some(Phase::Clicks));
        assert!(sim
            .at
            .findings_so_far()
            .iter()
            .any(|f| f.title == "Not enough fast movement recorded"));
        let f = movement_findings(&sim);
        assert!(
            f.iter().all(|(s, _)| *s == Severity::Info),
            "healthy mouse flagged: {:?}",
            f
        );
        // Either measured correctly or honestly not measured
        assert!(
            matches!(sim.at.metrics.polling_hz, None | Some(990..=1010)),
            "{:?}",
            sim.at.metrics.polling_hz
        );
    }

    #[test]
    fn test_slow_movement_on_healthy_8khz_mouse_is_not_flagged() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        // 4000 counts/s at 8 kHz is half a count per poll
        circle_stream(&mut sim, 12.5, 8000.0, 4000.0, 0, 0);
        assert_eq!(sim.phase(), Some(Phase::Clicks));
        let f = movement_findings(&sim);
        assert!(
            f.iter().all(|(s, _)| *s == Severity::Info),
            "healthy mouse flagged: {:?}",
            f
        );
        assert!(
            matches!(sim.at.metrics.polling_hz, None | Some(7900..=8100)),
            "{:?}",
            sim.at.metrics.polling_hz
        );
    }

    #[test]
    fn test_normal_speed_circles_measure_the_true_rate() {
        for (hz, speed) in [(1000.0, 2500.0), (8000.0, 24000.0), (125.0, 1500.0)] {
            let mut sim = Sim::new(cfg());
            sim.good_rest();
            circle_stream(&mut sim, 12.5, hz, speed, 0, 0);
            assert_eq!(sim.phase(), Some(Phase::Clicks), "{} Hz", hz);
            let f = movement_findings(&sim);
            assert!(
                f.iter().all(|(s, _)| *s == Severity::Info),
                "{} Hz flagged: {:?}",
                hz,
                f
            );
            assert_eq!(
                sim.at.metrics.polling_nominal_hz,
                Some(hz as u32),
                "{:?}",
                f
            );
        }
    }

    #[test]
    fn test_host_timestamp_jitter_is_not_flagged() {
        // Fast movement, every poll carries counts, but the host stamps
        // reports late by up to `jitter` and sometimes hands two over at once.
        for (hz, jitter_ms) in [(1000.0, 0.3), (8000.0, 0.06)] {
            let mut sim = Sim::new(cfg());
            sim.good_rest();
            let t0 = sim.t;
            let mut seed: u32 = 12345;
            for k in 1..=(12.5 * hz) as usize {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let noise = (seed >> 16) as f64 / 65_536.0 * jitter_ms / 1000.0;
                // Every 40th report waits for the next one (batched delivery)
                let late = if k % 40 == 0 { 1.0 / hz } else { 0.0 };
                let t = t0 + k as f64 / hz + noise + late;
                sim.t = sim.t.max(t);
                sim.at.feed(sim.t, &MouseEvent::Move { dx: 5, dy: -3 });
            }
            sim.idle(12.5);
            assert_eq!(sim.phase(), Some(Phase::Scroll), "{} Hz", hz);
            let f = movement_findings(&sim);
            assert!(
                f.iter().all(|(s, _)| *s == Severity::Info),
                "{} Hz flagged: {:?}",
                hz,
                f
            );
            assert_eq!(sim.at.metrics.polling_nominal_hz, Some(hz as u32));
        }
    }

    #[test]
    fn test_dropouts_during_fast_movement_are_flagged() {
        // 1 kHz, fast circles, 6 polls lost every 50 polls
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        circle_stream(&mut sim, 12.5, 1000.0, 5000.0, 50, 6);
        let f = movement_findings(&sim);
        assert!(
            f.iter()
                .any(|(s, t)| *s >= Severity::Warning && t.contains("stutter")),
            "{:?}",
            f
        );
        assert_eq!(sim.at.metrics.polling_hz, Some(1000));

        // 8 kHz, fast circles, 4 ms lost every 50 ms
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        circle_stream(&mut sim, 12.5, 8000.0, 30000.0, 400, 32);
        let f = movement_findings(&sim);
        assert!(
            f.iter()
                .any(|(s, t)| *s >= Severity::Warning && t.contains("stutter")),
            "{:?}",
            f
        );
    }

    #[test]
    fn test_single_bump_during_rest_is_not_a_fault() {
        let mut sim = Sim::new(cfg());
        // The hand leaves the mouse a little late and nudges it once
        sim.idle(2.2);
        for _ in 0..8 {
            sim.at.feed(sim.t, &MouseEvent::Move { dx: 9, dy: -4 });
            sim.t += 0.008;
        }
        sim.idle(6.0);
        assert_eq!(sim.phase(), Some(Phase::Movement));
        let f = sim.at.findings_so_far();
        assert!(
            f.iter().all(|f| f.severity == Severity::Info),
            "{:?}",
            f.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
    }

    fn rest_findings(sim: &Sim) -> Vec<(Severity, String)> {
        sim.at
            .findings_so_far()
            .iter()
            .filter(|f| f.phase == Phase::Rest)
            .map(|f| (f.severity, f.title.clone()))
            .collect()
    }

    #[test]
    fn test_hand_leaving_late_is_not_a_fault() {
        let mut sim = Sim::new(cfg());
        // The hand stays on the mouse until 5 s after Start (3 s past the
        // settle time): small tremor movements, then the lift-off bump.
        sim.t = 2.0;
        while sim.t < 5.0 {
            sim.at.feed(sim.t, &MouseEvent::Move { dx: 1, dy: -1 });
            sim.t += 0.15;
        }
        for _ in 0..6 {
            sim.at.feed(sim.t, &MouseEvent::Move { dx: 12, dy: 7 });
            sim.t += 0.008;
        }
        sim.idle(3.0);
        assert_eq!(sim.phase(), Some(Phase::Movement));
        let f = rest_findings(&sim);
        assert!(f.iter().all(|(s, _)| *s == Severity::Info), "{:?}", f);
        assert!(f
            .iter()
            .any(|(_, t)| t == "Movement at the start of the rest step"));
        assert_eq!(sim.at.metrics.rest_motion_seconds, 4);
    }

    #[test]
    fn test_hand_leaving_late_after_a_still_second_is_not_a_fault() {
        // The hand rests on the mouse after Start: nothing in the first
        // second after the settle time, a few single-count tremors, then the
        // hand lifts off at 5.1 s and nothing more.
        for tremor in [1, 3] {
            let mut sim = Sim::new(cfg());
            for t in [3.2, 3.8, 4.3, 4.7] {
                sim.at.feed(t, &MouseEvent::Move { dx: tremor, dy: 0 });
            }
            sim.t = 5.1;
            for _ in 0..6 {
                sim.at.feed(sim.t, &MouseEvent::Move { dx: 12, dy: 7 });
                sim.t += 0.008;
            }
            sim.idle(2.5);
            assert_eq!(sim.phase(), Some(Phase::Movement));
            let f = rest_findings(&sim);
            assert_eq!(
                f,
                vec![(
                    Severity::Info,
                    "Movement at the start of the rest step".to_string()
                )],
                "tremor {}",
                tremor
            );
        }
    }

    #[test]
    fn test_drift_before_an_early_start_is_still_a_fault() {
        // The sensor drifts all through the step, then the user starts the
        // movement step early: the burst at the end excuses nothing.
        let mut sim = Sim::new(cfg());
        for i in 0..45 {
            sim.at
                .feed(2.05 + 0.1 * i as f64, &MouseEvent::Move { dx: 1, dy: 0 });
        }
        sim.t = 6.8;
        for _ in 0..20 {
            sim.at.feed(sim.t, &MouseEvent::Move { dx: 15, dy: 4 });
            sim.t += 0.008;
        }
        sim.idle(0.5);
        assert_eq!(sim.phase(), Some(Phase::Movement));
        let f = rest_findings(&sim);
        assert!(
            f.contains(&(Severity::Critical, "Sensor moves on its own".to_string())),
            "{:?}",
            f
        );

        // Drift that stops after the hand leaves late is still drift
        let mut sim = Sim::new(cfg());
        sim.t = 3.0;
        for _ in 0..6 {
            sim.at.feed(sim.t, &MouseEvent::Move { dx: 12, dy: 7 });
            sim.t += 0.008;
        }
        for i in 0..30 {
            sim.at
                .feed(4.05 + 0.1 * i as f64, &MouseEvent::Move { dx: 1, dy: 1 });
        }
        sim.t = 7.0;
        sim.idle(0.5);
        let f = rest_findings(&sim);
        assert!(f.iter().any(|(s, _)| *s >= Severity::Warning), "{:?}", f);
    }

    #[test]
    fn test_nudge_with_slight_sensor_noise_is_not_a_fault() {
        let mut sim = Sim::new(cfg());
        // Single counts of noise in three seconds and one nudge of the desk
        for (t, dx) in [(2.5, 1), (4.5, 30), (4.51, 25), (5.5, 1), (6.5, 1)] {
            sim.at.feed(t, &MouseEvent::Move { dx, dy: 0 });
        }
        sim.t = 6.5;
        sim.idle(1.0);
        assert_eq!(sim.phase(), Some(Phase::Movement));
        let f = rest_findings(&sim);
        assert!(f.iter().all(|(s, _)| *s == Severity::Info), "{:?}", f);
        assert!(f.iter().any(|(_, t)| t == "Brief movement at rest"));
    }

    #[test]
    fn test_stuck_button_reported_even_with_few_clicks() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        for _ in 0..2 {
            sim.click(MouseButton::Left, 70.0, 200.0);
        }
        // The third press never comes back up; further clicks do nothing
        sim.at
            .feed(sim.t, &MouseEvent::ButtonPress(MouseButton::Left));
        sim.idle(21.0);
        sim.good_scroll();
        sim.good_lift();
        let r = sim.at.report().unwrap();
        assert!(
            r.findings
                .iter()
                .any(|f| f.title == "Button stuck down" && f.severity == Severity::Critical),
            "{:?}",
            titles(r)
        );
        assert_eq!(r.verdict, Verdict::Faulty);
    }

    #[test]
    fn test_dead_right_button_or_wheel_is_never_healthy() {
        // No right clicks at all
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        for _ in 0..20 {
            sim.click(MouseButton::Left, 70.0, 200.0);
        }
        sim.idle(21.0);
        sim.good_scroll();
        sim.good_lift();
        let r = sim.at.report().unwrap();
        assert_eq!(r.verdict, Verdict::Inconclusive, "{:?}", titles(r));

        // No scrolling at all
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        sim.good_clicks();
        sim.idle(12.5);
        sim.good_lift();
        let r = sim.at.report().unwrap();
        assert_eq!(r.verdict, Verdict::Inconclusive, "{:?}", titles(r));
    }

    #[test]
    fn test_skipped_lift_or_aborted_run_is_not_healthy() {
        // Lift step without a single pause-then-move
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        sim.good_clicks();
        sim.good_scroll();
        sim.idle(10.5);
        let r = sim.at.report().unwrap();
        assert!(titles(r).contains(&"Lift-off not tested"));
        assert_eq!(r.verdict, Verdict::Inconclusive);

        // Stopped during the scroll step: scroll and lift never ran
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        sim.good_clicks();
        sim.at.abort();
        let r = sim.at.report().unwrap();
        assert!(r.aborted);
        assert_eq!(r.verdict, Verdict::Inconclusive);
    }

    #[test]
    fn test_one_scroll_direction_is_untested_not_a_fault() {
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        sim.good_clicks();
        for _ in 0..12 {
            sim.scroll(-1, 80.0);
        }
        sim.idle(13.0);
        sim.good_lift();
        let r = sim.at.report().unwrap();
        assert_eq!(r.count(Severity::Warning), 0, "{:?}", titles(r));
        assert_eq!(r.verdict, Verdict::Inconclusive, "{:?}", titles(r));
    }

    #[test]
    fn test_bounce_severity_depends_on_count_not_sample_size() {
        // One isolated bounce in a short run: a warning, not a fault
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        for i in 0..8 {
            sim.click(MouseButton::Left, 70.0, 200.0);
            if i == 3 {
                sim.t -= 0.192;
                sim.click(MouseButton::Left, 30.0, 200.0);
            }
        }
        sim.idle(21.0);
        let f = sim.at.findings_so_far();
        assert_eq!(sim.at.metrics.bounce_events, 1);
        assert!(
            !f.iter().any(|f| f.severity == Severity::Critical),
            "{:?}",
            f.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert!(f
            .iter()
            .any(|f| f.title == "Possible switch bounce" && f.severity == Severity::Warning));

        // In a long run two bounces still only ask for a re-test, and
        // three are a failing switch, as in the Double-Click test
        for (bounced_at, fault) in [(vec![10, 30], false), (vec![5, 20, 35], true)] {
            let mut sim = Sim::new(cfg());
            sim.good_rest();
            sim.good_movement();
            for i in 0..40 {
                sim.click(MouseButton::Left, 70.0, 150.0);
                if bounced_at.contains(&i) {
                    sim.t -= 0.142;
                    sim.click(MouseButton::Left, 30.0, 150.0);
                }
            }
            for _ in 0..5 {
                sim.click(MouseButton::Right, 80.0, 200.0);
            }
            let f = sim.at.findings_so_far();
            assert_eq!(sim.at.metrics.bounce_events, bounced_at.len());
            let severity = if fault {
                Severity::Critical
            } else {
                Severity::Warning
            };
            let title = if fault {
                "Switch bounce (accidental double clicks)"
            } else {
                "Possible switch bounce"
            };
            assert!(
                f.iter().any(|f| f.title == title && f.severity == severity),
                "{:?}",
                f.iter().map(|f| &f.title).collect::<Vec<_>>()
            );
        }
        assert_eq!(bounce_verdict(0), BounceVerdict::None);
        assert_eq!(bounce_verdict(2), BounceVerdict::Possible);
        assert_eq!(bounce_verdict(CONFIRMED_BOUNCES), BounceVerdict::Failing);
    }

    #[test]
    fn test_unmeasured_checks_have_no_numbers() {
        // Framework input: the timing checks were not run
        let mut sim = Sim::new(Config {
            timing_reliable: false,
            ..cfg()
        });
        sim.good_rest();
        sim.good_clicks();
        sim.good_scroll();
        let r = sim.at.report().unwrap();
        assert_eq!(r.verdict, Verdict::Healthy);
        assert_eq!(r.stutter_rate_percent(), None);
        assert_eq!(r.bounce_events(), None);
        assert_eq!(r.scroll_reversals(), None);
        assert_eq!(r.lift_jumps(), None);
        assert_eq!(r.rest_unit(), "pointer units");

        // Raw input, but too little fast movement and no lift
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.move_for(0.05, 1.0);
        sim.idle(12.5);
        sim.good_clicks();
        sim.good_scroll();
        sim.idle(10.5);
        let r = sim.at.report().unwrap();
        assert_eq!(r.stutter_rate_percent(), None);
        assert_eq!(r.lift_jumps(), None);
        assert_eq!(r.bounce_events(), Some(0));
        assert_eq!(r.scroll_reversals(), Some(0));
        assert_eq!(r.rest_unit(), "counts");

        // A full healthy run has every number
        let mut sim = Sim::new(cfg());
        sim.good_rest();
        sim.good_movement();
        sim.good_clicks();
        sim.good_scroll();
        sim.good_lift();
        let r = sim.at.report().unwrap();
        assert_eq!(r.stutter_rate_percent(), Some(0.0));
        assert_eq!(r.lift_jumps(), Some(0));
    }

    #[test]
    fn test_framework_input_skips_timing_and_count_checks() {
        let mut sim = Sim::new(Config {
            timing_reliable: false,
            ..cfg()
        });
        assert_eq!(
            sim.at.phases(),
            &[Phase::Rest, Phase::Clicks, Phase::Scroll]
        );
        sim.good_rest();
        assert_eq!(sim.phase(), Some(Phase::Clicks));
        // Frame-quantised: press and release of a quick click share a frame
        for _ in 0..20 {
            sim.click(MouseButton::Left, 0.0, 200.0);
        }
        for _ in 0..5 {
            sim.click(MouseButton::Right, 0.0, 16.0);
        }
        assert_eq!(sim.phase(), Some(Phase::Scroll));
        // Smooth scrolling turned into whole notches; two land in one frame
        for _ in 0..10 {
            sim.scroll(-1, 0.0);
            sim.scroll(-1, 16.0);
        }
        sim.scroll(1, 16.0);
        for _ in 0..10 {
            sim.scroll(1, 90.0);
        }
        assert!(sim.at.is_finished());
        let r = sim.at.report().unwrap();
        assert!(
            r.findings.iter().all(|f| f.severity == Severity::Info),
            "{:?}",
            titles(r)
        );
        assert_eq!(r.verdict, Verdict::Healthy, "{:?}", titles(r));
        assert!(!r.timing_reliable);
    }
}
