//! Stutter detection panel
//!
//! Detects late motion reports (stutters) while the mouse moves fast. A mouse
//! only reports once a whole count has accumulated, so slow movement,
//! direction reversals and pauses legitimately leave gaps; only intervals
//! during fast, steady movement are judged, against their median. The
//! decision logic is `mouse_testkit::analysis::stutter::StutterDetector`;
//! this panel only collects raw input and draws the result.

use eframe::egui;
use egui_plot::{HLine, Line, Plot, PlotPoints};
use std::collections::VecDeque;
use std::time::Instant;

use super::polling::{input_lost_notice, raw_input_required_notice};
use crate::export::StutterExport;
use crate::input_bridge::{RawInputEvent, RawInputKind};
use mouse_testkit::analysis::stutter::{
    StutterDetector, StutterRating, DEFAULT_STUTTER_MULTIPLIER, MIN_JUDGED_SECS, MIN_STUTTER_GAP_MS,
};

/// Judged intervals kept for the graph and the export.
const GRAPH_SAMPLES: usize = 200;

/// Seconds from `epoch` to `t`, negative when `t` is earlier.
fn secs_since(epoch: Instant, t: Instant) -> f64 {
    match t.checked_duration_since(epoch) {
        Some(d) => d.as_secs_f64(),
        None => -epoch.duration_since(t).as_secs_f64(),
    }
}

/// Panel for detecting mouse movement stutters.
///
/// Feeds raw motion reports to a [`StutterDetector`] and shows the judged
/// intervals, the median-based threshold and a rating.
pub struct StutterPanel {
    is_running: bool,
    detector: StutterDetector,
    /// Judged (fast-movement) intervals in ms, most recent last.
    deltas: VecDeque<f64>,
    /// Origin for converting event timestamps to seconds.
    epoch: Instant,
    /// Motion reports and judged intervals in the latest frame, for the
    /// "move faster" hint.
    frame_reports: usize,
    frame_judged: usize,
    threshold_multiplier: f64,
    last_saved_threshold: f64,
    /// The run was ended because raw input stopped (see
    /// [`super::polling::input_lost_notice`]).
    stopped_for_input: bool,
}

impl StutterPanel {
    pub fn new() -> Self {
        Self {
            is_running: false,
            detector: StutterDetector::new(DEFAULT_STUTTER_MULTIPLIER),
            deltas: VecDeque::with_capacity(GRAPH_SAMPLES),
            epoch: Instant::now(),
            frame_reports: 0,
            frame_judged: 0,
            threshold_multiplier: DEFAULT_STUTTER_MULTIPLIER,
            last_saved_threshold: DEFAULT_STUTTER_MULTIPLIER,
            stopped_for_input: false,
        }
    }

    /// Set the stutter threshold multiplier (from config)
    pub fn set_threshold_multiplier(&mut self, value: f64) {
        self.threshold_multiplier = value;
        self.last_saved_threshold = value;
        self.detector.set_multiplier(value);
    }

    /// Get the current threshold multiplier
    pub fn get_threshold_multiplier(&self) -> f64 {
        self.threshold_multiplier
    }

    /// Check if settings have changed since last save
    pub fn settings_changed(&mut self) -> bool {
        let changed = (self.threshold_multiplier - self.last_saved_threshold).abs() > 0.01;
        if changed {
            self.last_saved_threshold = self.threshold_multiplier;
        }
        changed
    }

    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// The measured input changed: a run in progress ends, whichever page
    /// is shown (see [`input_lost_notice`]). Another mouse's reports would
    /// be judged against this one's baseline.
    pub fn input_changed(&mut self) {
        self.stopped_for_input |= self.is_running;
        self.is_running = false;
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
    ) {
        ui.heading("Stutter Detection");
        ui.add_space(5.0);
        ui.label("Detects reports that arrive late while the mouse is moving fast.");
        if !has_bridge {
            // Only raw reports are judged: a run ends when they stop, and is
            // not continued on another source (see `input_lost_notice`)
            self.stopped_for_input |= self.is_running;
            ui.add_space(5.0);
            raw_input_required_notice(ui, "Report timing (stutters)");
            self.is_running = false;
        }
        if self.stopped_for_input {
            ui.add_space(5.0);
            input_lost_notice(ui);
        }
        ui.add_space(15.0);

        // Controls
        ui.horizontal(|ui| {
            if self.is_running {
                if ui.button("Stop").clicked() {
                    self.is_running = false;
                }
            } else if ui
                .add_enabled(has_bridge, egui::Button::new("Start"))
                .clicked()
            {
                self.start();
            }

            if ui.button("Reset").clicked() {
                self.reset();
            }

            ui.add_space(20.0);
            ui.label("Sensitivity:");
            ui.add(
                egui::Slider::new(&mut self.threshold_multiplier, 1.5..=4.0)
                    .fixed_decimals(1)
                    .suffix("x"),
            );
            ui.label(
                egui::RichText::new(format!(
                    "x median interval, at least {:.1} ms (lower = more sensitive)",
                    MIN_STUTTER_GAP_MS
                ))
                .weak()
                .size(11.0),
            );
        });
        self.detector.set_multiplier(self.threshold_multiplier);

        // Capture input and measure timing (raw input only)
        if self.is_running && has_bridge {
            self.process_raw_events(raw_events);
        }

        ui.add_space(20.0);

        let median = self.detector.median_interval_ms();
        let threshold = self.detector.threshold_ms();

        // Main stats
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(20.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Polling Rate (fast movement)")
                                .weak()
                                .size(12.0),
                        );
                        ui.label(
                            egui::RichText::new(match self.detector.polling_hz() {
                                Some(hz) => format!("{:.0} Hz", hz),
                                None => "-".to_string(),
                            })
                            .size(24.0)
                            .strong(),
                        );
                    });
                    ui.add_space(30.0);

                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Median Interval").weak().size(12.0));
                        ui.label(
                            egui::RichText::new(match median {
                                Some(m) => format!("{:.3} ms", m),
                                None => "-".to_string(),
                            })
                            .size(20.0)
                            .strong(),
                        );
                    });
                    ui.add_space(30.0);

                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Threshold").weak().size(12.0));
                        ui.label(
                            egui::RichText::new(match threshold {
                                Some(t) => format!("{:.2} ms", t),
                                None => "-".to_string(),
                            })
                            .size(16.0),
                        );
                    });
                    ui.add_space(30.0);

                    let count = self.detector.stutter_count();
                    let color = if count == 0 {
                        egui::Color32::GREEN
                    } else if count < 10 {
                        egui::Color32::YELLOW
                    } else {
                        egui::Color32::RED
                    };
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Total Stutters").weak().size(12.0));
                        ui.label(
                            egui::RichText::new(format!("{}", count))
                                .color(color)
                                .size(24.0)
                                .strong(),
                        );
                    });
                    ui.add_space(30.0);

                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Fast Movement Judged")
                                .weak()
                                .size(12.0),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "{:.1} s ({} intervals)",
                                self.detector.judged_secs(),
                                self.detector.judged()
                            ))
                            .size(16.0),
                        );
                    });
                });
            });

        if self.is_running && self.frame_reports > 0 && self.frame_judged == 0 {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "Moving too slowly to judge - move faster. Slow movement and direction changes are ignored because the mouse legitimately sends fewer reports then.",
                )
                .color(egui::Color32::YELLOW),
            );
        }

        ui.add_space(20.0);

        // Graph
        ui.heading("Interval Graph (fast movement only)");
        ui.label(
            egui::RichText::new(
                "Time between consecutive reports while moving fast. Spikes above the red line are stutters.",
            )
            .weak()
            .size(12.0),
        );

        let points: PlotPoints = self
            .deltas
            .iter()
            .enumerate()
            .map(|(i, &d)| [i as f64, d])
            .collect();
        let line = Line::new("Interval", points).color(egui::Color32::from_rgb(100, 200, 255));

        Plot::new("stutter_plot")
            .height(280.0)
            .show_axes(true)
            .show_grid(true)
            .allow_drag(false)
            .allow_zoom(false)
            .y_axis_label("ms")
            .include_y(0.0)
            .show(ui, |plot_ui| {
                plot_ui.line(line);
                if let Some(m) = median {
                    plot_ui.hline(
                        HLine::new("Median", m).color(egui::Color32::from_rgb(100, 255, 100)),
                    );
                }
                if let Some(t) = threshold {
                    plot_ui.hline(
                        HLine::new("Stutter Threshold", t)
                            .color(egui::Color32::from_rgb(255, 100, 100))
                            .style(egui_plot::LineStyle::dashed_loose()),
                    );
                }
            });

        ui.add_space(8.0);

        // Legend
        ui.horizontal(|ui| {
            ui.colored_label(egui::Color32::from_rgb(100, 200, 255), "■ Interval");
            ui.add_space(15.0);
            ui.colored_label(egui::Color32::from_rgb(100, 255, 100), "■ Median");
            ui.add_space(15.0);
            ui.colored_label(
                egui::Color32::from_rgb(255, 100, 100),
                "■ Stutter Threshold",
            );
        });

        ui.add_space(20.0);

        // Rating
        if self.detector.reports() > 0 {
            let rating = self.detector.rating();
            let color = match rating {
                StutterRating::Inconclusive => egui::Color32::YELLOW,
                StutterRating::Excellent => egui::Color32::GREEN,
                StutterRating::Good => egui::Color32::LIGHT_GREEN,
                StutterRating::Fair => egui::Color32::YELLOW,
                StutterRating::Poor => egui::Color32::RED,
            };

            egui::Frame::dark_canvas(ui.style())
                .inner_margin(15.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Rating:");
                        ui.label(
                            egui::RichText::new(rating.label())
                                .size(18.0)
                                .strong()
                                .color(color),
                        );
                        ui.add_space(20.0);
                        ui.label(egui::RichText::new(rating.message()).color(egui::Color32::GRAY));
                    });
                    ui.add_space(5.0);
                    if rating == StutterRating::Inconclusive {
                        ui.label(format!(
                            "Fast movement judged so far: {:.1} s of the {:.0} s needed.",
                            self.detector.judged_secs(),
                            MIN_JUDGED_SECS
                        ));
                    }
                    let (minor, moderate, severe) = self.detector.severity_counts();
                    ui.label(format!(
                        "Stutter rate: {:.2}% | Worst gap: {:.1} ms | Minor (<4 ms late): {} | Moderate (4-8 ms): {} | Severe (8+ ms): {} | Freezes (0.1-1 s): {}",
                        self.detector.stutter_rate_percent(),
                        self.detector.worst_gap_ms(),
                        minor,
                        moderate,
                        severe,
                        self.detector.freezes()
                    ));
                    for (note, color) in self.notes() {
                        ui.add_space(5.0);
                        ui.label(egui::RichText::new(note).color(color));
                    }
                });
        }

        ui.add_space(20.0);

        // Instructions
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Instructions").strong());
                ui.label("1. Click 'Start' and move the mouse quickly and continuously in circles, without lifting it");
                ui.label(format!(
                    "2. Keep going until at least {:.0} s of fast movement has been judged",
                    MIN_JUDGED_SECS
                ));
                ui.label("3. Spikes in the graph above the red line indicate stutters");
                ui.add_space(5.0);
                ui.label(
                    egui::RichText::new(
                        "Only fast, steady movement is judged; slow passes, reversals and pauses are ignored. At 4000-8000 Hz move briskly or use 1600+ DPI so each report carries enough counts. Keep moving: at 125-250 Hz a hand that stops for a moment and sets off again the same way can look like a late report.",
                    )
                    .weak(),
                );
            });
    }

    /// What the counts above do not say on their own: freezes that lost the
    /// movement, reports delivered in batches, and clipped reports.
    fn notes(&self) -> Vec<(String, egui::Color32)> {
        let d = &self.detector;
        let mut notes = Vec::new();
        if d.dropouts() > 0 {
            notes.push((
                format!(
                    "{} of the freezes lost the movement: the reports stopped and went on at full speed, which no hand does. The connection dropped out or the sensor lost track of the surface (a mouse lifted while moving looks the same).",
                    d.dropouts()
                ),
                egui::Color32::YELLOW,
            ));
        }
        if let Some(b) = d.batching() {
            notes.push((
                format!(
                    "Reports arrive in batches, {:.1} per delivery on average, one delivery every {:.1} ms, as a Bluetooth mouse sends the reports it has ready once per connection event. The cursor moves once per delivery, so the rate, median and stutters are per delivery.",
                    b.reports_per_event, b.event_period_ms
                ),
                egui::Color32::LIGHT_GRAY,
            ));
        }
        if let Some(c) = d.clipping() {
            notes.push((
                format!(
                    "Clipped at high speed: {} of {} fast reports ({:.0}%) held exactly {} counts on one axis, the most the mouse's reports can carry. Faster movement is lost or sent late, so fast movements come up short. Lower the DPI or raise the polling rate. This is not counted as stutter.",
                    c.pinned,
                    c.fast_reports,
                    c.percent(),
                    c.limit
                ),
                egui::Color32::YELLOW,
            ));
        }
        notes
    }

    /// Feed raw motion reports to the detector. Nothing is timed across
    /// reports the system dropped (see `RawInputEvent::lost_before`).
    fn process_raw_events(&mut self, raw_events: &[RawInputEvent]) {
        self.frame_reports = 0;
        self.frame_judged = 0;
        for event in raw_events {
            if event.lost_before {
                self.detector.break_continuity();
            }
            if let RawInputKind::Move { dx, dy } = event.kind {
                if dx == 0 && dy == 0 {
                    continue;
                }
                self.frame_reports += 1;
                let t = secs_since(self.epoch, event.timestamp);
                if let Some(interval) = self.detector.feed(t, dx, dy) {
                    self.frame_judged += 1;
                    self.deltas.push_back(interval);
                    if self.deltas.len() > GRAPH_SAMPLES {
                        self.deltas.pop_front();
                    }
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn start_for_test(&mut self) {
        self.start();
    }

    fn start(&mut self) {
        self.is_running = true;
        self.clear();
    }

    fn reset(&mut self) {
        self.is_running = false;
        self.clear();
    }

    fn clear(&mut self) {
        self.detector = StutterDetector::new(self.threshold_multiplier);
        self.stopped_for_input = false;
        self.deltas.clear();
        self.epoch = Instant::now();
        self.frame_reports = 0;
        self.frame_judged = 0;
    }

    pub fn export(&self) -> Option<StutterExport> {
        let judged = self.detector.judged();
        if judged == 0 {
            return None;
        }
        Some(StutterExport {
            rating: self.detector.rating().label().to_string(),
            freezes: self.detector.freezes(),
            freezes_movement_lost: self.detector.dropouts(),
            batched_delivery: self.detector.batching().is_some(),
            clipped_at_counts: self.detector.clipping().map(|c| c.limit),
            total_stutter_count: self.detector.stutter_count(),
            total_samples: judged,
            avg_delta_ms: self.detector.judged_mean_ms().unwrap_or(0.0),
            min_delta_ms: self.detector.judged_min_ms().unwrap_or(0.0),
            max_delta_ms: self.detector.judged_max_ms().unwrap_or(0.0),
            polling_rate_hz: self.detector.polling_hz().unwrap_or(0.0),
            threshold_multiplier: self.threshold_multiplier,
            stutter_rate_percent: self.detector.stutter_rate_percent(),
            deltas: self.deltas.iter().cloned().collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn mv(t: Instant, dx: i32, dy: i32) -> RawInputEvent {
        RawInputEvent {
            kind: RawInputKind::Move { dx, dy },
            timestamp: t,
            lost_before: false,
        }
    }

    /// A 1 kHz mouse moved back and forth by an ordinary hand: fast passes,
    /// slowing into each reversal (1-count reports with growing gaps), a slow
    /// drag now and then, and a pause.
    fn back_and_forth(passes: usize, stall_every: Option<usize>) -> Vec<RawInputEvent> {
        let mut t = Instant::now();
        let mut out = Vec::new();
        let mut dir = 1;
        let mut fast_reports = 0;
        for pass in 0..passes {
            for _ in 0..300 {
                fast_reports += 1;
                let late = stall_every.is_some_and(|n| fast_reports % n == 0);
                t += Duration::from_millis(if late { 7 } else { 1 });
                out.push(mv(t, 3 * dir, if late { 0 } else { 1 }));
            }
            for gap in [2, 3, 5, 8, 12] {
                t += Duration::from_millis(gap);
                out.push(mv(t, dir, 0));
            }
            dir = -dir;
            for gap in [12, 8, 5, 3, 2] {
                t += Duration::from_millis(gap);
                out.push(mv(t, dir, 0));
            }
            if pass % 4 == 3 {
                t += Duration::from_millis(1500);
            }
        }
        out
    }

    #[test]
    fn healthy_reversals_and_slow_passes_are_not_stutters() {
        let mut p = StutterPanel::new();
        p.start();
        let events = back_and_forth(20, None);
        for frame in events.chunks(40) {
            p.process_raw_events(frame);
        }
        let e = p.export().unwrap();
        assert_eq!(e.total_stutter_count, 0);
        assert!(
            (e.polling_rate_hz - 1000.0).abs() < 1.0,
            "{}",
            e.polling_rate_hz
        );
        assert_eq!(p.detector.rating(), StutterRating::Excellent);
    }

    #[test]
    fn a_short_run_exports_as_inconclusive() {
        // 1.5 s of fast movement with three late reports: on screen this is
        // Inconclusive, and the export must say so next to the counts
        let mut p = StutterPanel::new();
        p.start();
        let mut t = Instant::now();
        let events: Vec<_> = (0..1500u32)
            .map(|i| {
                t += Duration::from_millis(if i % 500 == 499 { 8 } else { 1 });
                mv(t, 3, 1)
            })
            .collect();
        p.process_raw_events(&events);
        let e = p.export().unwrap();
        assert_eq!(e.total_stutter_count, 3);
        assert_eq!(e.rating, "Inconclusive");
    }

    #[test]
    fn eight_khz_host_jitter_is_not_a_stutter() {
        // Every 50th report is handled 0.3 ms late and the next catches up
        let mut p = StutterPanel::new();
        p.start();
        let mut t = Instant::now();
        let events: Vec<_> = (0..40_000u32)
            .map(|i| {
                let us = match i % 50 {
                    0 => 425,
                    1 => 25,
                    _ => 125,
                };
                t += Duration::from_micros(us);
                mv(t, 3, 1)
            })
            .collect();
        p.process_raw_events(&events);
        let e = p.export().unwrap();
        assert_eq!(e.total_stutter_count, 0);
        assert!((e.polling_rate_hz - 8000.0).abs() < 10.0);
    }

    #[test]
    fn late_reports_during_fast_movement_are_detected_once() {
        let mut p = StutterPanel::new();
        p.start();
        // One 7 ms gap every 100 fast reports
        let events = back_and_forth(20, Some(100));
        for frame in events.chunks(40) {
            p.process_raw_events(frame);
        }
        let e = p.export().unwrap();
        assert_eq!(e.total_stutter_count, 60);
        assert!(matches!(
            p.detector.rating(),
            StutterRating::Fair | StutterRating::Poor
        ));
        // Rendering frames afterwards does not count anything again
        let ctx = egui::Context::default();
        for _ in 0..3 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                let c = ui.ctx().clone();
                p.ui(ui, &c, &[], true);
            });
        }
        assert_eq!(p.export().unwrap().total_stutter_count, 60);
    }

    /// An 8 kHz mouse moved briskly for 4 s while the reader thread falls
    /// behind every 0.25 s and the kernel drops 5 ms of reports; the first
    /// report after each hole is marked when `marked`.
    fn eight_khz_with_dropped_reports(marked: bool) -> Vec<RawInputEvent> {
        let t0 = Instant::now();
        let mut events: Vec<RawInputEvent> = Vec::new();
        let mut after_hole = false;
        for k in 1..=32_000u64 {
            if k > 2000 && k % 2000 < 40 {
                after_hole = true;
                continue;
            }
            let mut e = mv(t0 + Duration::from_micros(125 * k), 3, 1);
            e.lost_before = marked && after_hole;
            after_hole = false;
            events.push(e);
        }
        events
    }

    #[test]
    fn reports_the_system_dropped_are_not_stutters() {
        let mut p = StutterPanel::new();
        p.start();
        for frame in eight_khz_with_dropped_reports(true).chunks(128) {
            p.process_raw_events(frame);
        }
        assert_eq!(p.detector.stutter_count(), 0);
        assert_eq!(p.detector.rating(), StutterRating::Excellent);
        // Unmarked, each hole reads as a stutter of the mouse
        let mut p = StutterPanel::new();
        p.start();
        for frame in eight_khz_with_dropped_reports(false).chunks(128) {
            p.process_raw_events(frame);
        }
        assert!(
            p.detector.stutter_count() >= 10,
            "{}",
            p.detector.stutter_count()
        );
    }

    #[test]
    fn fallback_does_not_measure() {
        let ctx = egui::Context::default();
        let mut p = StutterPanel::new();
        p.start();
        let events = back_and_forth(4, Some(50));
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let c = ui.ctx().clone();
            p.ui(ui, &c, &events, false);
        });
        assert!(!p.is_running());
        assert!(p.export().is_none());
    }

    #[test]
    fn losing_raw_input_ends_the_run_without_mixing() {
        // Measured on one mouse, which is then unplugged; reports read
        // afterwards (another mouse) must not join the run
        let ctx = egui::Context::default();
        let frame = |p: &mut StutterPanel, events: &[RawInputEvent], raw: bool| {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                let c = ui.ctx().clone();
                p.ui(ui, &c, events, raw);
            });
        };
        let mut p = StutterPanel::new();
        p.start();
        let events = back_and_forth(8, None);
        frame(&mut p, &events, true);
        let reports = p.detector.reports();
        assert!(reports > 2000);
        frame(&mut p, &[], false);
        assert!(!p.is_running() && p.stopped_for_input);
        frame(&mut p, &back_and_forth(4, Some(10)), true);
        assert_eq!(p.detector.reports(), reports);
        assert_eq!(p.export().unwrap().total_stutter_count, 0);
        p.start_for_test();
        assert!(!p.stopped_for_input);
    }

    #[test]
    fn default_sensitivity_matches_the_cli_and_auto_diagnostics() {
        let p = StutterPanel::new();
        assert_eq!(p.get_threshold_multiplier(), DEFAULT_STUTTER_MULTIPLIER);
        assert_eq!(
            p.get_threshold_multiplier(),
            mouse_testkit::analysis::diagnostics::Config::default().stutter_multiplier
        );
    }

    #[test]
    fn secs_since_handles_earlier_timestamps() {
        let epoch = Instant::now();
        assert!(secs_since(epoch, epoch + Duration::from_millis(5)) > 0.0049);
        if let Some(earlier) = epoch.checked_sub(Duration::from_millis(5)) {
            assert!(secs_since(epoch, earlier) < -0.0049);
        }
    }
}
