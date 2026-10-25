//! Acceleration and angle-snapping panels
//!
//! Both tests reason about whole strokes (runs of movement separated by short
//! pauses) rather than individual reports. See
//! `mouse_testkit::analysis::{strokes, acceleration, angle_snap}` for the
//! analysis; this module only collects input and draws the results.
//!
//! With raw input the acceleration test sees the mouse's own counts, before
//! the OS applies pointer acceleration, so it covers sensor/firmware
//! acceleration only and must not be read as a verdict on the OS setting.
//! Without raw input it sees the on-screen pointer, which includes OS
//! acceleration.

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints, Points};
use std::collections::VecDeque;
use std::time::Instant;

use crate::export::{AccelerationExport, AngleSnapExport};
use crate::input_bridge::{RawInputEvent, RawInputKind};
use mouse_testkit::analysis::acceleration::{
    analyze_strokes, stroke_sample, AccelAnalysis, AccelVerdict, NotAPass, StrokeSample,
    MIN_STROKES_PER_GROUP, MIN_STROKE_COUNTS,
};
use mouse_testkit::analysis::angle_snap::{
    analyze_shapes, stroke_shape, AngleSnapAnalysis, StrokeShape, MIN_NEAR_AXIS_STROKES,
    NEAR_AXIS_DEG,
};
use mouse_testkit::analysis::strokes::{Stroke, StrokeSegmenter};

/// A pause this long (seconds) ends a stroke.
const STROKE_GAP_S: f64 = 0.25;
/// When the test is stopped with the Stop button, a stroke that ended this
/// recently (seconds) was the pointer moving to the button. A real last pass
/// ended earlier: a pause of [`STROKE_GAP_S`] split it from that trip, and
/// another such pause passed before the trip was recorded.
const STOP_TRIP_S: f64 = 0.6;
/// Points kept for the angle-snap path drawing.
const MAX_PATH_POINTS: usize = 4000;

/// How an acceleration run was stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopBy {
    /// From the keyboard: the mouse did not move to stop it.
    Key,
    /// With the Stop button, which the measured mouse moved to.
    Button,
    /// The measured input changed: the pass in progress was cut off.
    InputChanged,
}

pub struct AccelPanel {
    // Acceleration detection
    is_running: bool,
    /// Wall-clock origin for converting event timestamps to seconds.
    epoch: Instant,
    accel_segmenter: StrokeSegmenter,
    accel_strokes: Vec<StrokeSample>,
    /// End time (s) of the stroke behind the last entry of `accel_strokes`.
    accel_last_end: Option<f64>,
    /// Strokes left out because they turned back on themselves.
    accel_turned_back: usize,
    /// Reports were lost during a pass (see `RawInputEvent::lost_before`):
    /// the next stroke finished is the rest of it, not a pass of its own.
    accel_skip_stroke: bool,
    accel_result: Option<AccelAnalysis>,
    /// Whether the current or last acceleration run read raw counts (true)
    /// or the on-screen pointer, which includes OS acceleration.
    accel_raw: bool,

    // Angle snapping detection
    angle_running: bool,
    /// Whether the current angle-snapping run reads raw counts.
    angle_raw: bool,
    angle_segmenter: StrokeSegmenter,
    angle_shapes: Vec<StrokeShape>,
    /// Strokes too short to analyse (shown so the user knows why).
    angle_short_strokes: usize,
    angle_result: Option<AngleSnapAnalysis>,
    /// Cumulative path for visualisation (plot coordinates: y up).
    angle_points: VecDeque<(f64, f64)>,
    angle_accumulated_pos: (f64, f64),

    /// Pointer movement (points) not yet passed on as whole points in the
    /// framework-input fallback.
    pointer_remainder: (f32, f32),
}

/// Add a frame's pointer movement to `remainder` and take out the whole
/// points. Rounding each frame on its own would lose or add up to half a
/// point per frame, and a slow pass moves only a point or two per frame on a
/// high-refresh display: the counts of slow passes would be off by tens of
/// percent and look like acceleration.
fn take_whole_points(remainder: &mut (f32, f32), delta: egui::Vec2) -> (i32, i32) {
    remainder.0 += delta.x;
    remainder.1 += delta.y;
    let (dx, dy) = (remainder.0.round(), remainder.1.round());
    remainder.0 -= dx;
    remainder.1 -= dy;
    (dx as i32, dy as i32)
}

impl AccelPanel {
    pub fn new() -> Self {
        Self {
            is_running: false,
            epoch: Instant::now(),
            accel_segmenter: StrokeSegmenter::new(STROKE_GAP_S),
            accel_strokes: Vec::new(),
            accel_last_end: None,
            accel_turned_back: 0,
            accel_skip_stroke: false,
            accel_result: None,
            accel_raw: true,
            angle_running: false,
            angle_raw: true,
            angle_segmenter: StrokeSegmenter::new(STROKE_GAP_S),
            angle_shapes: Vec::new(),
            angle_short_strokes: 0,
            angle_result: None,
            angle_points: VecDeque::new(),
            angle_accumulated_pos: (0.0, 0.0),
            pointer_remainder: (0.0, 0.0),
        }
    }

    pub fn is_running(&self) -> bool {
        self.is_running || self.angle_running
    }

    /// The measured input changed (another device, or raw input stopped or
    /// started): runs in progress end, whichever page is shown, keeping the
    /// passes and lines measured so far but not the one cut off.
    pub fn input_changed(&mut self) {
        if self.is_running {
            self.stop_accel(self.secs(Instant::now()), StopBy::InputChanged);
        }
        if self.angle_running {
            self.angle_segmenter.reset();
            self.stop_angle();
        }
    }

    fn secs(&self, t: Instant) -> f64 {
        t.duration_since(self.epoch).as_secs_f64()
    }

    /// Collect (time, dx, dy) motion for this frame from raw input or the
    /// egui fallback, and where reports were lost: indices into the motion,
    /// each the first after a gap (its length when no motion followed).
    fn frame_motion(
        &mut self,
        ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
    ) -> (Vec<(f64, i32, i32)>, Vec<usize>) {
        if has_bridge {
            let mut motion = Vec::new();
            let mut gaps = Vec::new();
            for ev in raw_events {
                if ev.lost_before && gaps.last() != Some(&motion.len()) {
                    gaps.push(motion.len());
                }
                if let RawInputKind::Move { dx, dy } = ev.kind {
                    motion.push((self.secs(ev.timestamp), dx, dy));
                }
            }
            (motion, gaps)
        } else {
            let delta = ctx.input(|i| i.pointer.delta());
            let (dx, dy) = take_whole_points(&mut self.pointer_remainder, delta);
            if dx != 0 || dy != 0 {
                (vec![(self.secs(Instant::now()), dx, dy)], Vec::new())
            } else {
                (Vec::new(), Vec::new())
            }
        }
    }

    // ── Acceleration ───────────────────────────────────────────────────

    pub fn ui_accel(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
    ) {
        // Raw counts and OS-accelerated pointer movement cannot be compared
        // within one run: stop it, keeping the passes measured so far.
        if self.is_running && has_bridge != self.accel_raw {
            self.stop_accel(self.secs(Instant::now()), StopBy::InputChanged);
        }
        // Stopping from the keyboard does not move the mouse, so the pass in
        // progress is a real one. The key is consumed so it cannot also
        // press a focused button such as Start Test.
        if self.is_running {
            let stop_key = ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    | i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
            });
            if stop_key {
                self.stop_accel(self.secs(Instant::now()), StopBy::Key);
            }
        }
        // Labels describe the input the run used, not the current one
        let raw = if self.is_running || self.accel_result.is_some() {
            self.accel_raw
        } else {
            has_bridge
        };
        ui.heading("Acceleration Detection");
        ui.add_space(5.0);
        if raw {
            ui.label(
                "Detects acceleration in the mouse's sensor or firmware (more counts for the same distance when you move faster).",
            );
            ui.label(
                egui::RichText::new(
                    "Covers sensor/firmware acceleration only. OS pointer acceleration (Windows \"Enhance pointer precision\", macOS or libinput acceleration) is applied after the raw counts this test reads, so it cannot be seen here - check that setting separately.",
                )
                .weak()
                .size(11.0),
            );
        } else {
            ui.label(
                "Detects acceleration in the on-screen pointer (more movement for the same distance when you move faster).",
            );
            ui.label(
                egui::RichText::new(
                    "Note: Raw input unavailable - measuring the pointer after OS processing, so OS pointer acceleration is included. Keep the pointer inside this window: the screen edge stops it and spoils a pass.",
                )
                .color(egui::Color32::YELLOW)
                .size(11.0),
            );
        }
        ui.add_space(15.0);

        ui.horizontal(|ui| {
            if self.is_running {
                if ui.button("Stop Test").clicked() {
                    self.stop_accel(self.secs(Instant::now()), StopBy::Button);
                }
            } else if ui.button("Start Test").clicked() {
                self.start_accel(has_bridge);
            }
            if ui.button("Clear Data").clicked() {
                self.clear_accel();
            }
        });

        ui.add_space(15.0);

        if self.is_running {
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(40, 60, 40))
                .inner_margin(15.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Test in Progress")
                            .strong()
                            .color(egui::Color32::GREEN),
                    );
                    ui.add_space(5.0);
                    ui.label("Move the mouse across the SAME physical distance each time (for example the full width of your mousepad).");
                    ui.label("Pause briefly after every pass. Do about 5 passes slowly, then 5 passes quickly.");
                    ui.label("Press Enter or Esc to stop without moving the mouse.");
                    ui.add_space(10.0);
                    let in_progress = self
                        .accel_segmenter
                        .in_progress()
                        .map(|s| s.path_length())
                        .unwrap_or(0.0);
                    ui.label(format!(
                        "Passes recorded: {}   |   current pass: {:.0} counts",
                        self.accel_strokes.len(),
                        in_progress
                    ));
                    if self.accel_turned_back > 0 {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} movement(s) went there and back without a pause and were left out - pause after each pass.",
                                self.accel_turned_back
                            ))
                            .color(egui::Color32::YELLOW),
                        );
                    }
                });
        } else {
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(15.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("How This Test Works").strong());
                    ui.add_space(5.0);
                    ui.label("Without acceleration, moving the mouse a fixed distance always produces the same number of counts, however fast you move.");
                    ui.label("With acceleration, fast passes produce more counts than slow passes.");
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("During the test:").strong());
                    ui.label(format!(
                        "1. Choose a fixed distance (a pass must be at least {:.0} counts).",
                        MIN_STROKE_COUNTS
                    ));
                    ui.label("2. Move across it, pause, move back, pause - about 5 times slowly.");
                    ui.label("3. Repeat about 5 times quickly, then press Enter (or Stop Test).");
                    ui.label(format!(
                        "At least {} slow and {} fast passes are needed for a verdict.",
                        MIN_STROKES_PER_GROUP, MIN_STROKES_PER_GROUP
                    ));
                });
        }

        ui.add_space(20.0);

        ui.heading("Counts per Pass vs Speed");
        let median_counts = {
            let mut c: Vec<f64> = self.accel_strokes.iter().map(|s| s.counts).collect();
            mouse_testkit::analysis::polling::median_in_place(&mut c).unwrap_or(1.0)
        };
        let points: PlotPoints = self
            .accel_strokes
            .iter()
            .map(|s| [s.velocity, s.counts / median_counts.max(1.0)])
            .collect();
        let max_v = self
            .accel_strokes
            .iter()
            .map(|s| s.velocity)
            .fold(1000.0, f64::max);

        let scatter = Points::new("Passes", points)
            .color(egui::Color32::from_rgb(100, 200, 255))
            .radius(5.0);
        let ideal_line = Line::new(
            "No Acceleration (Ideal)",
            PlotPoints::from_explicit_callback(|_x| 1.0, 0.0..max_v * 1.1, 2),
        )
        .color(egui::Color32::GREEN)
        .width(2.0);

        Plot::new("accel_plot")
            .height(250.0)
            .x_axis_label("Pass speed (counts/s)")
            .y_axis_label("Counts relative to median pass")
            .include_y(0.0)
            .include_y(2.0)
            .show_axes(true)
            .show_grid(true)
            .allow_drag(false)
            .allow_zoom(false)
            .show(ui, |plot_ui| {
                plot_ui.line(ideal_line);
                plot_ui.points(scatter);
            });

        ui.add_space(20.0);

        if let Some(result) = &self.accel_result {
            ui.heading("Detection Result");
            egui::Frame::dark_canvas(ui.style())
                .inner_margin(20.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    // Fewer counts on fast passes is almost always the sensor
                    // losing tracking, not an acceleration setting to turn off.
                    let loses_counts = result.verdict == AccelVerdict::LosesCounts;
                    let (status, color) = match result.verdict {
                        AccelVerdict::Inconclusive => {
                            ("Inconclusive - vary your speed more", egui::Color32::YELLOW)
                        }
                        AccelVerdict::LosesCounts => {
                            ("Fast Passes LOSE Counts", egui::Color32::RED)
                        }
                        AccelVerdict::Acceleration if raw => {
                            ("Sensor/Firmware Acceleration DETECTED", egui::Color32::RED)
                        }
                        AccelVerdict::Acceleration => {
                            ("Pointer Acceleration DETECTED", egui::Color32::RED)
                        }
                        AccelVerdict::NoAcceleration if raw => {
                            ("No Sensor/Firmware Acceleration", egui::Color32::GREEN)
                        }
                        AccelVerdict::NoAcceleration => {
                            ("No Pointer Acceleration Detected", egui::Color32::GREEN)
                        }
                    };
                    ui.label(egui::RichText::new(status).size(24.0).strong().color(color));
                    ui.add_space(15.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label("Fast / slow counts");
                            ui.label(
                                egui::RichText::new(format!("{:.2}x", result.factor)).size(20.0),
                            );
                        });
                        ui.add_space(40.0);
                        ui.vertical(|ui| {
                            ui.label("Fast / slow speed");
                            ui.label(
                                egui::RichText::new(format!("{:.1}x", result.speed_ratio))
                                    .size(20.0),
                            );
                        });
                        ui.add_space(40.0);
                        ui.vertical(|ui| {
                            ui.label("Confidence");
                            ui.label(
                                egui::RichText::new(format!("{:.0}%", result.confidence_percent))
                                    .size(20.0),
                            );
                        });
                    });
                    ui.add_space(10.0);
                    ui.label(format!(
                        "Slow passes: {}, typically {:.0} counts at {:.0} counts/s   |   Fast passes: {}, typically {:.0} counts at {:.0} counts/s (medians)",
                        result.slow_count,
                        result.slow_median_counts,
                        result.slow_median_velocity,
                        result.fast_count,
                        result.fast_median_counts,
                        result.fast_median_velocity
                    ));
                    if !result.conclusive {
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new(
                                "The fast passes were not clearly faster than the slow ones. Make the slow passes slower and the fast passes faster.",
                            )
                            .color(egui::Color32::YELLOW),
                        );
                    } else if loses_counts {
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new(if raw {
                                "The sensor may be losing tracking at speed: try another mousepad or surface, or the mouse may be moving faster than it is rated for."
                            } else {
                                "The pointer moved less on fast passes: it may have hit the screen edge, or the sensor is losing tracking at speed."
                            })
                            .color(egui::Color32::YELLOW),
                        );
                    } else if result.has_acceleration {
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new(if raw {
                                "Recommendation: turn off acceleration, \"enhanced\" or smoothing modes in the mouse's own software or on-board settings"
                            } else {
                                "Recommendation: disable pointer acceleration / \"enhance pointer precision\" in your OS and mouse software"
                            })
                            .color(egui::Color32::YELLOW),
                        );
                    }
                    if result.conclusive && raw {
                        ui.add_space(5.0);
                        ui.label(
                            egui::RichText::new(
                                "This result does not tell you whether OS pointer acceleration is on: the OS applies it after the raw counts measured here.",
                            )
                            .weak()
                            .size(11.0),
                        );
                    }
                });
        } else if !self.is_running && !self.accel_strokes.is_empty() {
            ui.label(
                egui::RichText::new(format!(
                    "{} pass(es) recorded - not enough for a verdict. Need at least {} slow and {} fast passes of {:.0}+ counts.",
                    self.accel_strokes.len(),
                    MIN_STROKES_PER_GROUP,
                    MIN_STROKES_PER_GROUP,
                    MIN_STROKE_COUNTS
                ))
                .color(egui::Color32::YELLOW),
            );
        }

        if self.is_running {
            let (motion, gaps) = self.frame_motion(ctx, raw_events, has_bridge);
            let now = self.secs(Instant::now());
            self.feed_accel_with_gaps(&motion, &gaps, now);
        }
    }

    /// Feed one frame's motion `(t, dx, dy)` and advance time to `now` (s).
    #[cfg(test)]
    fn feed_accel(&mut self, motion: &[(f64, i32, i32)], now: f64) {
        self.feed_accel_with_gaps(motion, &[], now);
    }

    /// [`Self::feed_accel`] with reports lost before `motion[i]` for each
    /// `i` in `gaps`. The pass in progress lost some of its counts (reports
    /// only flow while the mouse moves): neither its part before the gap
    /// nor the rest after it is recorded (as in the CLI test).
    fn feed_accel_with_gaps(&mut self, motion: &[(f64, i32, i32)], gaps: &[usize], now: f64) {
        for (i, &(t, dx, dy)) in motion.iter().enumerate() {
            if gaps.contains(&i) {
                self.accel_input_gap();
            }
            if let Some(stroke) = self.accel_segmenter.feed(t, dx, dy) {
                self.record_accel_stroke(&stroke);
            }
        }
        if gaps.contains(&motion.len()) {
            self.accel_input_gap();
        }
        if let Some(stroke) = self.accel_segmenter.tick(now) {
            self.record_accel_stroke(&stroke);
        }
    }

    fn accel_input_gap(&mut self) {
        self.accel_segmenter.reset();
        self.accel_skip_stroke = true;
    }

    fn record_accel_stroke(&mut self, stroke: &Stroke) {
        if std::mem::take(&mut self.accel_skip_stroke) {
            return;
        }
        match stroke_sample(stroke) {
            Ok(sample) => {
                self.accel_strokes.push(sample);
                self.accel_last_end = Some(stroke.end);
            }
            Err(NotAPass::TurnsBack) => self.accel_turned_back += 1,
            Err(NotAPass::TooShort) => {}
        }
    }

    fn start_accel(&mut self, has_bridge: bool) {
        self.is_running = true;
        self.accel_raw = has_bridge;
        self.pointer_remainder = (0.0, 0.0);
        self.accel_segmenter.reset();
        self.accel_strokes.clear();
        self.accel_last_end = None;
        self.accel_turned_back = 0;
        self.accel_skip_stroke = false;
        self.accel_result = None;
    }

    /// Stop at time `now` (s). Reaching the Stop button moves the measured
    /// mouse, and that trip is no pass: it is the stroke in progress, or one
    /// that ended within [`STOP_TRIP_S`].
    fn stop_accel(&mut self, now: f64, by: StopBy) {
        self.is_running = false;
        match by {
            StopBy::Key => {
                if let Some(stroke) = self.accel_segmenter.flush() {
                    self.record_accel_stroke(&stroke);
                }
            }
            StopBy::Button => {
                self.accel_segmenter.reset();
                if self
                    .accel_last_end
                    .is_some_and(|end| now - end < STOP_TRIP_S)
                {
                    self.accel_strokes.pop();
                }
            }
            StopBy::InputChanged => self.accel_segmenter.reset(),
        }
        self.accel_result = analyze_strokes(&self.accel_strokes);
    }

    fn clear_accel(&mut self) {
        self.accel_segmenter.reset();
        self.accel_strokes.clear();
        self.accel_last_end = None;
        self.accel_turned_back = 0;
        self.accel_skip_stroke = false;
        self.accel_result = None;
    }

    // ── Angle snapping ─────────────────────────────────────────────────

    pub fn ui_angle(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
    ) {
        // Raw counts and OS-processed pointer movement cannot be mixed in one
        // run: stop it, keeping the lines measured so far.
        if self.angle_running && has_bridge != self.angle_raw {
            self.angle_segmenter.reset();
            self.stop_angle();
        }
        ui.heading("Angle Snapping Detection");
        ui.add_space(5.0);
        ui.label(
            "Detects firmware that straightens near-horizontal or near-vertical hand movements.",
        );
        if !has_bridge {
            ui.label(
                egui::RichText::new(
                    "Note: Raw input unavailable — using framework input (reduced accuracy)",
                )
                .color(egui::Color32::YELLOW)
                .size(11.0),
            );
        }
        ui.add_space(15.0);

        ui.horizontal(|ui| {
            if self.angle_running {
                if ui.button("Stop Test").clicked() {
                    self.stop_angle();
                }
            } else if ui.button("Start Test").clicked() {
                self.start_angle(has_bridge);
            }
            if ui.button("Clear Data").clicked() {
                self.clear_angle();
            }
        });

        ui.add_space(15.0);

        if self.angle_running {
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(40, 60, 40))
                .inner_margin(15.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Test in Progress")
                            .strong()
                            .color(egui::Color32::GREEN),
                    );
                    ui.add_space(5.0);
                    ui.label("Draw freehand lines that are ALMOST horizontal or vertical (tilted 5-10 degrees), one at a time.");
                    ui.label("Pause briefly between lines. Do not use a ruler - a natural hand wobble is what the test looks for.");
                    ui.add_space(10.0);
                    ui.label(format!(
                        "Lines recorded: {}   |   too short to judge: {}",
                        self.angle_shapes.len(),
                        self.angle_short_strokes
                    ));
                });
        } else {
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(15.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("What is Angle Snapping?").strong());
                    ui.add_space(5.0);
                    ui.label("Angle snapping (prediction) forces near-horizontal or near-vertical movements onto a perfectly straight axis line.");
                    ui.label("A hand cannot draw a perfectly straight line, so a slightly tilted freehand stroke that comes out exactly on-axis with zero wobble is the signature of snapping.");
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("During the test:").strong());
                    ui.label("Draw 5 or more slightly tilted freehand lines, pausing between them, then press Stop Test.");
                    ui.label(format!(
                        "Lines must be within {:.0} degrees of an axis and at least 100 counts long; {} such lines are needed for a verdict.",
                        NEAR_AXIS_DEG, MIN_NEAR_AXIS_STROKES
                    ));
                });
        }

        ui.add_space(20.0);

        ui.heading("Movement Path Visualization");
        let points: PlotPoints = self.angle_points.iter().map(|(x, y)| [*x, *y]).collect();
        let scatter = Points::new("Movement Path", points)
            .color(egui::Color32::from_rgb(100, 200, 255))
            .radius(2.0);
        Plot::new("angle_snap_plot")
            .height(300.0)
            .data_aspect(1.0)
            .show_axes(true)
            .show_grid(true)
            .allow_drag(false)
            .allow_zoom(false)
            .show(ui, |plot_ui| {
                plot_ui.points(scatter);
            });

        ui.add_space(20.0);

        if let Some(result) = &self.angle_result {
            ui.heading("Detection Result");
            egui::Frame::dark_canvas(ui.style())
                .inner_margin(20.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    let (status, color) = if !result.conclusive {
                        ("Inconclusive - draw more near-axis lines", egui::Color32::YELLOW)
                    } else if result.has_snapping {
                        ("Angle Snapping DETECTED", egui::Color32::RED)
                    } else {
                        ("No Angle Snapping Detected", egui::Color32::GREEN)
                    };
                    ui.label(egui::RichText::new(status).size(24.0).strong().color(color));
                    ui.add_space(15.0);
                    ui.label(format!(
                        "Lines analysed: {}   |   near an axis: {}   |   perfectly straight: {}",
                        result.total_strokes, result.near_axis_strokes, result.snapped_strokes
                    ));
                    ui.label(format!(
                        "Snap strength: {:.0}% of near-axis lines were snapped",
                        result.snap_strength * 100.0
                    ));
                    if !result.dominant_angles.is_empty() {
                        ui.add_space(10.0);
                        ui.label("Snapped axes:");
                        for angle in &result.dominant_angles {
                            let name = if *angle == 0.0 {
                                "horizontal"
                            } else {
                                "vertical"
                            };
                            ui.label(format!("  • {:.0}° ({})", angle, name));
                        }
                    }
                    if result.has_snapping {
                        ui.add_space(15.0);
                        ui.label(
                            egui::RichText::new(
                                "Recommendation: turn off angle snapping / prediction in the mouse software",
                            )
                            .color(egui::Color32::YELLOW),
                        );
                    }
                });
        }

        if self.angle_running {
            let (motion, gaps) = self.frame_motion(ctx, raw_events, has_bridge);
            for (i, (t, dx, dy)) in motion.into_iter().enumerate() {
                if gaps.contains(&i) {
                    // Dropped reports cut a piece out of the line in
                    // progress: it is not judged (as in the CLI test)
                    self.angle_segmenter.reset();
                }
                self.angle_accumulated_pos.0 += dx as f64;
                // Screen y grows downward; plot y grows upward
                self.angle_accumulated_pos.1 -= dy as f64;
                self.angle_points.push_back(self.angle_accumulated_pos);
                if self.angle_points.len() > MAX_PATH_POINTS {
                    self.angle_points.pop_front();
                }
                if let Some(stroke) = self.angle_segmenter.feed(t, dx, dy) {
                    self.record_angle_stroke(&stroke);
                }
            }
            let now = self.secs(Instant::now());
            if let Some(stroke) = self.angle_segmenter.tick(now) {
                self.record_angle_stroke(&stroke);
            }
        }
    }

    fn record_angle_stroke(&mut self, stroke: &Stroke) {
        match stroke_shape(&stroke.deltas) {
            Some(shape) => self.angle_shapes.push(shape),
            None => self.angle_short_strokes += 1,
        }
    }

    fn start_angle(&mut self, has_bridge: bool) {
        self.angle_running = true;
        self.angle_raw = has_bridge;
        self.pointer_remainder = (0.0, 0.0);
        self.angle_segmenter.reset();
        self.angle_shapes.clear();
        self.angle_short_strokes = 0;
        self.angle_result = None;
        self.angle_points.clear();
        self.angle_accumulated_pos = (0.0, 0.0);
    }

    fn stop_angle(&mut self) {
        self.angle_running = false;
        if let Some(stroke) = self.angle_segmenter.flush() {
            self.record_angle_stroke(&stroke);
        }
        self.angle_result = Some(analyze_shapes(&self.angle_shapes));
    }

    fn clear_angle(&mut self) {
        self.angle_segmenter.reset();
        self.angle_shapes.clear();
        self.angle_short_strokes = 0;
        self.angle_result = None;
        self.angle_points.clear();
        self.angle_accumulated_pos = (0.0, 0.0);
    }

    #[cfg(test)]
    pub(crate) fn start_accel_for_test(&mut self) {
        self.start_accel(true);
    }

    /// Stop as with Enter: the pass in progress counts.
    #[cfg(test)]
    pub(crate) fn stop_accel_for_test(&mut self) {
        self.stop_accel(self.secs(Instant::now()), StopBy::Key);
    }

    #[cfg(test)]
    pub(crate) fn start_angle_for_test(&mut self) {
        self.start_angle(true);
    }

    #[cfg(test)]
    pub(crate) fn stop_angle_for_test(&mut self) {
        self.stop_angle();
    }

    // ── Export ─────────────────────────────────────────────────────────

    pub fn export_accel(&self) -> Option<AccelerationExport> {
        self.accel_result.as_ref().map(|result| AccelerationExport {
            verdict: result.verdict.label().to_string(),
            input: if self.accel_raw {
                "raw sensor counts (sensor/firmware acceleration only)"
            } else {
                "on-screen pointer (includes OS pointer acceleration)"
            }
            .to_string(),
            has_acceleration: result.has_acceleration,
            conclusive: result.conclusive,
            accel_factor: result.factor,
            speed_ratio: result.speed_ratio,
            confidence_percent: result.confidence_percent,
            slow_sample_count: result.slow_count,
            fast_sample_count: result.fast_count,
        })
    }

    pub fn export_angle(&self) -> Option<AngleSnapExport> {
        self.angle_result.as_ref().map(|result| AngleSnapExport {
            has_snapping: result.has_snapping,
            conclusive: result.conclusive,
            snap_strength_percent: result.snap_strength * 100.0,
            dominant_angles: result.dominant_angles.clone(),
            stroke_count: result.total_strokes,
            near_axis_stroke_count: result.near_axis_strokes,
            snapped_stroke_count: result.snapped_strokes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed passes of `counts` along X at `per_ms` counts per millisecond,
    /// alternating direction, with a 0.4 s pause after each; returns the time
    /// after the last pause.
    fn passes(p: &mut AccelPanel, mut t: f64, n: usize, counts: i32, per_ms: i32) -> f64 {
        for i in 0..n {
            let dir = if i % 2 == 0 { 1 } else { -1 };
            let motion: Vec<(f64, i32, i32)> = (0..counts / per_ms)
                .map(|k| (t + 0.001 * k as f64, dir * per_ms, 0))
                .collect();
            t += 0.001 * motion.len() as f64;
            p.feed_accel(&motion, t);
            t += 0.4;
            p.feed_accel(&[], t);
        }
        t
    }

    #[test]
    fn the_trip_to_the_stop_button_is_not_a_pass() {
        // A linear mouse: 5 slow and 5 fast passes of 8000 counts, then the
        // pointer moved a short way to Stop Test, slowly, and clicked.
        for pause_before_click in [0.1, 0.4] {
            let mut p = AccelPanel::new();
            p.start_accel(true);
            let t = passes(&mut p, 0.0, 5, 8000, 4);
            let t = passes(&mut p, t, 5, 8000, 40);
            let trip: Vec<(f64, i32, i32)> =
                (0..300).map(|k| (t + 0.002 * k as f64, 2, 1)).collect();
            let end = t + 0.6;
            p.feed_accel(&trip, end);
            p.feed_accel(&[], end + pause_before_click);
            p.stop_accel(end + pause_before_click, StopBy::Button);
            let r = p.accel_result.as_ref().unwrap();
            assert_eq!(r.verdict, AccelVerdict::NoAcceleration, "{}", r.factor);
            assert_eq!(p.accel_strokes.len(), 10);
        }

        // Stopped with Enter, the pass in progress is a pass
        let mut p = AccelPanel::new();
        p.start_accel(true);
        let t = passes(&mut p, 0.0, 5, 8000, 4);
        let t = passes(&mut p, t, 4, 8000, 40);
        let last: Vec<(f64, i32, i32)> = (0..200).map(|k| (t + 0.001 * k as f64, 40, 0)).collect();
        p.feed_accel(&last, t + 0.2);
        p.stop_accel(t + 0.25, StopBy::Key);
        assert_eq!(p.accel_strokes.len(), 10);
    }

    #[test]
    fn a_pass_cut_by_dropped_reports_is_not_recorded() {
        // A fast pass of 8000 counts loses 40 ms of its reports (1600
        // counts) while the reader falls behind. Neither its part before the
        // gap nor the rest after it is a pass.
        for marked in [true, false] {
            let mut p = AccelPanel::new();
            p.start_accel(true);
            let t = passes(&mut p, 0.0, 5, 8000, 4);
            let t = passes(&mut p, t, 5, 8000, 40);
            let motion: Vec<(f64, i32, i32)> = (0..200)
                .filter(|k| !(80..120).contains(k))
                .map(|k| (t + 0.001 * k as f64, 40, 0))
                .collect();
            let gaps: &[usize] = if marked { &[80] } else { &[] };
            p.feed_accel_with_gaps(&motion, gaps, t + 0.2);
            p.feed_accel(&[], t + 0.6);
            p.stop_accel(t + 1.5, StopBy::Key);
            let expected = if marked { 10 } else { 11 };
            assert_eq!(p.accel_strokes.len(), expected, "marked: {}", marked);
        }
    }

    #[test]
    fn a_pass_cut_off_by_an_input_change_is_not_recorded() {
        // Another input is chosen halfway through a fast pass
        let mut p = AccelPanel::new();
        p.start_accel(true);
        let t = passes(&mut p, 0.0, 5, 8000, 4);
        let t = passes(&mut p, t, 4, 8000, 40);
        let half: Vec<(f64, i32, i32)> = (0..100).map(|k| (t + 0.001 * k as f64, 40, 0)).collect();
        p.feed_accel(&half, t + 0.1);
        p.input_changed();
        assert!(!p.is_running());
        assert_eq!(p.accel_strokes.len(), 9);
    }

    #[test]
    fn passes_run_together_are_left_out() {
        let mut p = AccelPanel::new();
        p.start_accel(true);
        let t = passes(&mut p, 0.0, 5, 8000, 4);
        let t = passes(&mut p, t, 4, 8000, 40);
        // Out and back without a pause: one stroke of 16000 counts
        let mut motion: Vec<(f64, i32, i32)> =
            (0..200).map(|k| (t + 0.001 * k as f64, 40, 0)).collect();
        motion.extend((200..400).map(|k| (t + 0.001 * k as f64, -40, 0)));
        p.feed_accel(&motion, t + 1.0);
        p.stop_accel(t + 1.5, StopBy::Key);
        assert_eq!(p.accel_turned_back, 1);
        assert_eq!(p.accel_strokes.len(), 9);
    }

    #[test]
    fn verdict_and_export_follow_the_input_the_run_used() {
        let ctx = egui::Context::default();
        // Fast passes lose a quarter of their counts on raw input
        let mut p = AccelPanel::new();
        p.start_accel(true);
        let t = passes(&mut p, 0.0, 5, 8000, 4);
        passes(&mut p, t, 5, 6000, 40);
        p.stop_accel(100.0, StopBy::Key);
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx().clone();
            // The mouse is released afterwards: the result stays a raw one
            p.ui_accel(ui, &ctx, &[], false);
        });
        let e = p.export_accel().unwrap();
        assert_eq!(e.verdict, "fast passes lose counts");
        assert!(!e.has_acceleration);
        assert!(e.input.starts_with("raw sensor counts"), "{}", e.input);

        // A run that loses raw input part way is stopped, not mixed with
        // OS-accelerated pointer movement
        let mut p = AccelPanel::new();
        p.start_accel(true);
        passes(&mut p, 0.0, 3, 8000, 4);
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx().clone();
            p.ui_accel(ui, &ctx, &[], false);
        });
        assert!(!p.is_running);
        assert!(p.accel_raw);
    }

    #[test]
    fn fallback_pointer_movement_is_not_lost_to_rounding() {
        // A slow pass on a 240 Hz display: 1.3 points per frame
        let mut remainder = (0.0, 0.0);
        let (mut x, mut y) = (0, 0);
        for _ in 0..100 {
            let (dx, dy) = take_whole_points(&mut remainder, egui::vec2(1.3, -0.4));
            x += dx;
            y += dy;
        }
        // (rounding every frame on its own would give 100 and 0)
        assert_eq!((x, y), (130, -40));
    }
}
