//! Acceleration and angle-snapping panels
//!
//! Both tests reason about whole strokes (runs of movement separated by short
//! pauses) rather than individual reports. See
//! `mouse_testkit::analysis::{strokes, acceleration, angle_snap}` for the
//! analysis; this module only collects input and draws the results.

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints, Points};
use std::collections::VecDeque;
use std::time::Instant;

use crate::export::{AccelerationExport, AngleSnapExport};
use crate::input_bridge::{RawInputEvent, RawInputKind};
use mouse_testkit::analysis::acceleration::{
    analyze_strokes, AccelAnalysis, StrokeSample, MIN_STROKES_PER_GROUP, MIN_STROKE_COUNTS,
};
use mouse_testkit::analysis::angle_snap::{
    analyze_shapes, stroke_shape, AngleSnapAnalysis, StrokeShape, MIN_NEAR_AXIS_STROKES,
    NEAR_AXIS_DEG,
};
use mouse_testkit::analysis::strokes::{Stroke, StrokeSegmenter};

/// A pause this long (seconds) ends a stroke.
const STROKE_GAP_S: f64 = 0.25;
/// Points kept for the angle-snap path drawing.
const MAX_PATH_POINTS: usize = 4000;

pub struct AccelPanel {
    // Acceleration detection
    is_running: bool,
    /// Wall-clock origin for converting event timestamps to seconds.
    epoch: Instant,
    accel_segmenter: StrokeSegmenter,
    accel_strokes: Vec<StrokeSample>,
    accel_result: Option<AccelAnalysis>,

    // Angle snapping detection
    angle_running: bool,
    angle_segmenter: StrokeSegmenter,
    angle_shapes: Vec<StrokeShape>,
    /// Strokes too short to analyse (shown so the user knows why).
    angle_short_strokes: usize,
    angle_result: Option<AngleSnapAnalysis>,
    /// Cumulative path for visualisation (plot coordinates: y up).
    angle_points: VecDeque<(f64, f64)>,
    angle_accumulated_pos: (f64, f64),
}

impl AccelPanel {
    pub fn new() -> Self {
        Self {
            is_running: false,
            epoch: Instant::now(),
            accel_segmenter: StrokeSegmenter::new(STROKE_GAP_S),
            accel_strokes: Vec::new(),
            accel_result: None,
            angle_running: false,
            angle_segmenter: StrokeSegmenter::new(STROKE_GAP_S),
            angle_shapes: Vec::new(),
            angle_short_strokes: 0,
            angle_result: None,
            angle_points: VecDeque::new(),
            angle_accumulated_pos: (0.0, 0.0),
        }
    }

    pub fn is_running(&self) -> bool {
        self.is_running || self.angle_running
    }

    fn secs(&self, t: Instant) -> f64 {
        t.duration_since(self.epoch).as_secs_f64()
    }

    /// Collect (time, dx, dy) motion for this frame from raw input or the
    /// egui fallback.
    fn frame_motion(
        &self,
        ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
    ) -> Vec<(f64, i32, i32)> {
        if has_bridge {
            raw_events
                .iter()
                .filter_map(|ev| match ev.kind {
                    RawInputKind::Move { dx, dy } => Some((self.secs(ev.timestamp), dx, dy)),
                    _ => None,
                })
                .collect()
        } else {
            let delta = ctx.input(|i| i.pointer.delta());
            if delta.x != 0.0 || delta.y != 0.0 {
                vec![(
                    self.secs(Instant::now()),
                    delta.x.round() as i32,
                    delta.y.round() as i32,
                )]
            } else {
                Vec::new()
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
        ui.heading("Acceleration Detection");
        ui.add_space(5.0);
        ui.label(
            "Detects mouse acceleration (more counts for the same distance when you move faster).",
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
            if self.is_running {
                if ui.button("Stop Test").clicked() {
                    self.stop_accel();
                }
            } else if ui.button("Start Test").clicked() {
                self.start_accel();
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
                    ui.label("3. Repeat about 5 times quickly, then press Stop Test.");
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
                    let (status, color) = if !result.conclusive {
                        ("Inconclusive - vary your speed more", egui::Color32::YELLOW)
                    } else if result.has_acceleration {
                        ("Acceleration DETECTED", egui::Color32::RED)
                    } else {
                        ("No Acceleration Detected", egui::Color32::GREEN)
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
                        "Slow passes: {} at {:.0} counts/s averaging {:.0} counts   |   Fast passes: {} at {:.0} counts/s averaging {:.0} counts",
                        result.slow_count,
                        result.slow_avg_velocity,
                        result.slow_avg_counts,
                        result.fast_count,
                        result.fast_avg_velocity,
                        result.fast_avg_counts
                    ));
                    if !result.conclusive {
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new(
                                "The fast passes were not clearly faster than the slow ones. Make the slow passes slower and the fast passes faster.",
                            )
                            .color(egui::Color32::YELLOW),
                        );
                    } else if result.has_acceleration {
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new(
                                "Recommendation: disable pointer acceleration / \"enhance pointer precision\" in your OS and mouse software",
                            )
                            .color(egui::Color32::YELLOW),
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
            let motion = self.frame_motion(ctx, raw_events, has_bridge);
            for (t, dx, dy) in motion {
                if let Some(stroke) = self.accel_segmenter.feed(t, dx, dy) {
                    self.record_accel_stroke(&stroke);
                }
            }
            let now = self.secs(Instant::now());
            if let Some(stroke) = self.accel_segmenter.tick(now) {
                self.record_accel_stroke(&stroke);
            }
        }
    }

    fn record_accel_stroke(&mut self, stroke: &Stroke) {
        let counts = stroke.path_length();
        if counts < MIN_STROKE_COUNTS {
            return;
        }
        self.accel_strokes.push(StrokeSample {
            counts,
            velocity: stroke.velocity(),
        });
    }

    fn start_accel(&mut self) {
        self.is_running = true;
        self.accel_segmenter.reset();
        self.accel_strokes.clear();
        self.accel_result = None;
    }

    fn stop_accel(&mut self) {
        self.is_running = false;
        if let Some(stroke) = self.accel_segmenter.flush() {
            self.record_accel_stroke(&stroke);
        }
        self.accel_result = analyze_strokes(&self.accel_strokes);
    }

    fn clear_accel(&mut self) {
        self.accel_segmenter.reset();
        self.accel_strokes.clear();
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
                self.start_angle();
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
            let motion = self.frame_motion(ctx, raw_events, has_bridge);
            for (t, dx, dy) in motion {
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

    fn start_angle(&mut self) {
        self.angle_running = true;
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
        self.start_accel();
    }

    #[cfg(test)]
    pub(crate) fn stop_accel_for_test(&mut self) {
        self.stop_accel();
    }

    #[cfg(test)]
    pub(crate) fn start_angle_for_test(&mut self) {
        self.start_angle();
    }

    #[cfg(test)]
    pub(crate) fn stop_angle_for_test(&mut self) {
        self.stop_angle();
    }

    // ── Export ─────────────────────────────────────────────────────────

    pub fn export_accel(&self) -> Option<AccelerationExport> {
        self.accel_result.as_ref().map(|result| AccelerationExport {
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
