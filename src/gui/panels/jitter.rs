//! Jitter test panel
//!
//! Records motion while the mouse lies untouched. The mouse under test is
//! also the one that clicks "Take Sample", so recording starts only after a
//! settle period: the click and the hand leaving the mouse are not sensor
//! noise. Amounts are raw counts at the mouse's current DPI.

use eframe::egui;
use egui_plot::{Plot, PlotPoints, Points};
use std::time::{Duration, Instant};

use crate::export::{JitterExport, JitterSampleExport};
use crate::input_bridge::{RawInputEvent, RawInputKind};

/// Seconds between clicking "Take Sample" and the start of recording: time to
/// release the button and take the hand off the mouse.
const SETTLE_SECS: u64 = 2;
/// Recording length in seconds.
const SAMPLE_SECS: u64 = 5;

/// Rating for the average total movement (counts) per untouched sample. A
/// healthy sensor that nobody touches normally reports nothing at all.
fn jitter_rating(avg_distance_counts: f64) -> &'static str {
    if avg_distance_counts < 1.0 {
        "Excellent"
    } else if avg_distance_counts < 5.0 {
        "Good"
    } else if avg_distance_counts < 20.0 {
        "Moderate"
    } else {
        "High Jitter"
    }
}

/// Shown instead of a rating when only window-system pointer data exists.
const NOT_RATED: &str = "Not rated (no raw input)";

pub struct JitterPanel {
    is_sampling: bool,
    samples: Vec<JitterSample>,
    /// Raw delta movements for jitter calculation
    current_deltas: Vec<(f64, f64)>,
    /// Accumulated positions for visualization
    current_positions: Vec<(f64, f64)>,
    /// When "Take Sample" was clicked; recording starts SETTLE_SECS later.
    sample_start: Option<Instant>,
    /// Accumulated position for visualization (sum of deltas)
    accumulated_pos: (f64, f64),
    /// A button changed state while recording: the hand was on the mouse.
    touched: bool,
    /// The last sample was thrown away because the mouse was touched.
    last_discarded: bool,
    /// Whether the current sample reads raw counts (false: pointer points).
    current_raw: bool,
}

struct JitterSample {
    events: usize,
    total_distance: f64,
    max_single: f64,
    /// Raw counts (true) or window-system pointer points (false).
    raw: bool,
}

impl JitterPanel {
    pub fn new() -> Self {
        Self {
            is_sampling: false,
            samples: Vec::new(),
            current_deltas: Vec::new(),
            current_positions: Vec::new(),
            sample_start: None,
            accumulated_pos: (0.0, 0.0),
            touched: false,
            last_discarded: false,
            current_raw: true,
        }
    }

    pub fn is_running(&self) -> bool {
        self.is_sampling
    }

    /// Draw the page. Input is recorded by [`Self::process_input`].
    pub fn ui(&mut self, ui: &mut egui::Ui, has_bridge: bool) {
        ui.heading("Jitter Test");
        ui.add_space(5.0);
        ui.label("Measures sensor noise when the mouse is stationary.");
        if !has_bridge {
            ui.add_space(5.0);
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(70, 50, 20))
                .inner_margin(12.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Raw input unavailable - jitter cannot be measured")
                            .strong()
                            .color(egui::Color32::YELLOW),
                    );
                    ui.label("Without raw input only the window-system pointer is visible: movement in screen points after OS acceleration, and only while the pointer is over this window. Sensor noise below one point is invisible, so no rating is given. Any movement shown still means the pointer moved while nobody touched the mouse.");
                });
        }
        ui.add_space(15.0);

        // Controls
        ui.horizontal(|ui| {
            if self.is_sampling {
                ui.label(self.progress_text(Instant::now()));
            } else if ui
                .button(format!("Take Sample ({}s)", SAMPLE_SECS))
                .clicked()
            {
                self.start_sample(has_bridge);
            }

            if ui.button("Clear All").clicked() {
                self.samples.clear();
                self.current_deltas.clear();
                self.current_positions.clear();
                self.last_discarded = false;
            }
        });

        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(format!(
                "After clicking, let go of the mouse: recording starts {} s later. DO NOT touch the mouse while recording!",
                SETTLE_SECS
            ))
            .color(egui::Color32::YELLOW),
        );
        if self.last_discarded {
            ui.label(
                egui::RichText::new(
                    "Last sample discarded: a mouse button was pressed or released while recording, so the hand was on the mouse. Take another sample.",
                )
                .color(egui::Color32::LIGHT_RED),
            );
        }
        ui.add_space(20.0);

        // Jitter visualization
        ui.heading("Jitter Visualization");

        let points: PlotPoints = self
            .current_positions
            .iter()
            .map(|(x, y)| [*x, *y])
            .collect();

        let scatter = Points::new("Jitter Events", points)
            .color(egui::Color32::from_rgb(255, 100, 100))
            .radius(3.0);

        Plot::new("jitter_plot")
            .height(250.0)
            .data_aspect(1.0)
            .show_axes(true)
            .show_grid(true)
            .allow_drag(false)
            .allow_zoom(false)
            .show(ui, |plot_ui| {
                plot_ui.points(scatter);
            });

        ui.add_space(20.0);

        // Results
        if !self.samples.is_empty() {
            ui.heading("Sample Results");

            let raw = self.samples.iter().all(|s| s.raw);
            let unit = if raw { "counts" } else { "points" };
            egui::Frame::dark_canvas(ui.style())
                .inner_margin(15.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    let avg_events: f64 = self.samples.iter().map(|s| s.events as f64).sum::<f64>()
                        / self.samples.len() as f64;
                    let avg_distance: f64 =
                        self.samples.iter().map(|s| s.total_distance).sum::<f64>()
                            / self.samples.len() as f64;
                    let max_jitter: f64 = self
                        .samples
                        .iter()
                        .map(|s| s.max_single)
                        .fold(0.0, f64::max);

                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label("Samples");
                            ui.label(
                                egui::RichText::new(format!("{}", self.samples.len()))
                                    .size(20.0)
                                    .strong(),
                            );
                        });
                        ui.add_space(30.0);
                        ui.vertical(|ui| {
                            ui.label("Avg Events");
                            ui.label(egui::RichText::new(format!("{:.1}", avg_events)).size(20.0));
                        });
                        ui.add_space(30.0);
                        ui.vertical(|ui| {
                            ui.label("Avg Distance");
                            ui.label(
                                egui::RichText::new(format!("{:.2} {}", avg_distance, unit))
                                    .size(20.0),
                            );
                        });
                        ui.add_space(30.0);
                        ui.vertical(|ui| {
                            ui.label("Max Jitter");
                            ui.label(
                                egui::RichText::new(format!("{:.2} {}", max_jitter, unit))
                                    .size(20.0),
                            );
                        });
                        ui.add_space(30.0);
                        ui.vertical(|ui| {
                            ui.label("Rating");
                            if raw {
                                let rating = jitter_rating(avg_distance);
                                let color = match rating {
                                    "Excellent" => egui::Color32::GREEN,
                                    "Good" => egui::Color32::LIGHT_GREEN,
                                    "Moderate" => egui::Color32::YELLOW,
                                    _ => egui::Color32::RED,
                                };
                                ui.label(egui::RichText::new(rating).size(20.0).color(color));
                            } else {
                                ui.label(
                                    egui::RichText::new(NOT_RATED)
                                        .size(14.0)
                                        .color(egui::Color32::GRAY),
                                );
                            }
                        });
                    });
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(if raw {
                            "Counts at the mouse's current DPI, per 5 s sample. A healthy sensor that nobody touches normally reports nothing."
                        } else {
                            "Screen points after OS acceleration; not comparable to sensor counts."
                        })
                        .weak()
                        .size(11.0),
                    );
                });
        }
    }

    /// Record this frame's input for a sample in progress. The app calls
    /// this every frame, whichever page is shown: input drained while
    /// another page was displayed would otherwise never be seen, and a
    /// sample finished on return would claim an untouched mouse.
    pub fn process_input(
        &mut self,
        ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
    ) {
        self.process_frame(ctx, raw_events, has_bridge, Instant::now());
    }

    fn process_frame(
        &mut self,
        ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
        now: Instant,
    ) {
        if !self.is_sampling {
            return;
        }
        if has_bridge {
            self.record_raw(raw_events);
        } else if self
            .recording_window()
            .is_some_and(|(from, until)| now >= from && now < until)
        {
            // Fallback: window-system pointer delta (points, OS-accelerated)
            let (delta, clicked) = ctx.input(|i| {
                (
                    i.pointer.delta(),
                    i.pointer.any_pressed() || i.pointer.any_released(),
                )
            });
            if clicked {
                self.touched = true;
            }
            if delta.x != 0.0 || delta.y != 0.0 {
                self.record_delta(delta.x as f64, delta.y as f64);
            }
        }
        self.tick(now);
    }

    /// (start, end) of the recording window of the current sample.
    fn recording_window(&self) -> Option<(Instant, Instant)> {
        self.sample_start.map(|start| {
            let from = start + Duration::from_secs(SETTLE_SECS);
            (from, from + Duration::from_secs(SAMPLE_SECS))
        })
    }

    fn progress_text(&self, now: Instant) -> String {
        match self.recording_window() {
            Some((from, _)) if now < from => format!(
                "Let go of the mouse... recording starts in {:.0} s",
                (from - now).as_secs_f64().ceil()
            ),
            Some((_, until)) => format!(
                "Recording... {:.0} s left - do not touch",
                until.saturating_duration_since(now).as_secs_f64().ceil()
            ),
            None => String::new(),
        }
    }

    /// Record raw events whose timestamps fall inside the recording window.
    fn record_raw(&mut self, raw_events: &[RawInputEvent]) {
        let Some((from, until)) = self.recording_window() else {
            return;
        };
        for event in raw_events {
            if event.timestamp < from || event.timestamp >= until {
                continue;
            }
            match event.kind {
                RawInputKind::Move { dx, dy } if dx != 0 || dy != 0 => {
                    self.record_delta(dx as f64, dy as f64);
                }
                RawInputKind::ButtonPress(_) | RawInputKind::ButtonRelease(_) => {
                    self.touched = true;
                }
                _ => {}
            }
        }
    }

    fn record_delta(&mut self, dx: f64, dy: f64) {
        self.current_deltas.push((dx, dy));
        // Negate Y because raw counts and screen coordinates have Y
        // increasing downward, but plot coordinates have Y increasing upward
        self.accumulated_pos.0 += dx;
        self.accumulated_pos.1 -= dy;
        self.current_positions
            .push((self.accumulated_pos.0, self.accumulated_pos.1));
    }

    /// Finish the sample once its recording window has passed.
    fn tick(&mut self, now: Instant) {
        if self
            .recording_window()
            .is_some_and(|(_, until)| now >= until)
        {
            self.finish_sample();
        }
    }

    fn start_sample(&mut self, raw: bool) {
        self.start_sample_at(Instant::now(), raw);
    }

    fn start_sample_at(&mut self, at: Instant, raw: bool) {
        self.is_sampling = true;
        self.sample_start = Some(at);
        self.current_deltas.clear();
        self.current_positions.clear();
        self.accumulated_pos = (0.0, 0.0);
        self.touched = false;
        self.last_discarded = false;
        self.current_raw = raw;
    }

    fn finish_sample(&mut self) {
        self.is_sampling = false;
        self.sample_start = None;
        if self.touched {
            self.last_discarded = true;
            return;
        }

        // Calculate jitter metrics from raw deltas
        let total_distance: f64 = self
            .current_deltas
            .iter()
            .map(|(dx, dy)| (dx * dx + dy * dy).sqrt())
            .sum();

        let max_single = self
            .current_deltas
            .iter()
            .map(|(dx, dy)| (dx * dx + dy * dy).sqrt())
            .fold(0.0, f64::max);

        self.samples.push(JitterSample {
            events: self.current_deltas.len(),
            total_distance,
            max_single,
            raw: self.current_raw,
        });
    }

    pub fn export(&self) -> Option<JitterExport> {
        if self.samples.is_empty() {
            return None;
        }
        let avg_events =
            self.samples.iter().map(|s| s.events as f64).sum::<f64>() / self.samples.len() as f64;
        let avg_distance =
            self.samples.iter().map(|s| s.total_distance).sum::<f64>() / self.samples.len() as f64;
        let max_jitter = self
            .samples
            .iter()
            .map(|s| s.max_single)
            .fold(0.0, f64::max);

        let raw = self.samples.iter().all(|s| s.raw);
        let rating = if raw {
            jitter_rating(avg_distance)
        } else {
            NOT_RATED
        };

        Some(JitterExport {
            sample_count: self.samples.len(),
            avg_events,
            distance_unit: if raw { "counts" } else { "points" }.to_string(),
            avg_distance_px: avg_distance,
            max_jitter_px: max_jitter,
            rating: rating.to_string(),
            samples: self
                .samples
                .iter()
                .map(|s| JitterSampleExport {
                    events: s.events,
                    total_distance: s.total_distance,
                    max_single: s.max_single,
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_bridge::RawButton;

    fn at(start: Instant, ms: u64, kind: RawInputKind) -> RawInputEvent {
        RawInputEvent {
            kind,
            timestamp: start + Duration::from_millis(ms),
        }
    }

    fn mv(dx: i32, dy: i32) -> RawInputKind {
        RawInputKind::Move { dx, dy }
    }

    /// The click on "Take Sample" and the hand leaving a 1600 DPI mouse.
    fn click_and_let_go(start: Instant) -> Vec<RawInputEvent> {
        let mut ev = vec![at(start, 0, RawInputKind::ButtonRelease(RawButton::Left))];
        for (i, (dx, dy)) in [(3, -2), (5, -1), (4, 0), (2, 1), (1, 0), (1, 1)]
            .into_iter()
            .enumerate()
        {
            ev.push(at(start, 150 + 40 * i as u64, mv(dx, dy)));
        }
        ev
    }

    fn run_sample(p: &mut JitterPanel, events: &[RawInputEvent], start: Instant) {
        p.start_sample_at(start, true);
        p.record_raw(events);
        p.tick(start + Duration::from_secs(SETTLE_SECS + SAMPLE_SECS));
    }

    #[test]
    fn click_and_hand_release_are_not_jitter() {
        let mut p = JitterPanel::new();
        let start = Instant::now();
        run_sample(&mut p, &click_and_let_go(start), start);
        let e = p.export().unwrap();
        assert_eq!(e.avg_events, 0.0);
        assert_eq!(e.rating, "Excellent");
        assert_eq!(e.distance_unit, "counts");
        assert!(!p.is_running());
    }

    #[test]
    fn phantom_motion_while_untouched_is_detected() {
        // A noisy sensor: a count or two every few hundred ms, ~24 counts in all
        let mut p = JitterPanel::new();
        let start = Instant::now();
        let mut events = click_and_let_go(start);
        for i in 0..20u64 {
            let (dx, dy) = if i % 2 == 0 { (1, 1) } else { (-1, 0) };
            events.push(at(start, 2_100 + 240 * i, mv(dx, dy)));
        }
        run_sample(&mut p, &events, start);
        let e = p.export().unwrap();
        assert_eq!(e.avg_events, 20.0);
        assert!((e.avg_distance_px - 24.14).abs() < 0.01);
        assert_eq!(e.rating, "High Jitter");
    }

    #[test]
    fn touching_the_mouse_while_recording_discards_the_sample() {
        let mut p = JitterPanel::new();
        let start = Instant::now();
        let mut events = click_and_let_go(start);
        events.push(at(
            start,
            4_000,
            RawInputKind::ButtonPress(RawButton::Right),
        ));
        events.push(at(start, 4_010, mv(12, 3)));
        run_sample(&mut p, &events, start);
        assert!(p.export().is_none());
        assert!(p.last_discarded);
    }

    #[test]
    fn input_while_another_page_is_shown_still_counts() {
        // Take Sample, then the user opens another page during the settle
        // time and comes back 20 s later: the panel's ui() never ran, but
        // the app fed every frame. The mouse moved and clicked meanwhile.
        let ctx = egui::Context::default();
        let mut p = JitterPanel::new();
        let start = Instant::now();
        p.start_sample_at(start, true);
        let away = [
            at(start, 3_000, mv(40, 12)),
            at(start, 3_500, RawInputKind::ButtonPress(RawButton::Left)),
            at(start, 3_600, RawInputKind::ButtonRelease(RawButton::Left)),
        ];
        p.process_frame(&ctx, &away, true, start + Duration::from_millis(3_700));
        p.process_frame(&ctx, &[], true, start + Duration::from_secs(20));
        assert!(!p.is_running());
        assert!(p.export().is_none(), "touched sample was kept");
        assert!(p.last_discarded);

        // Untouched but moving on its own while away: recorded, not Excellent
        p.start_sample_at(start, true);
        let drift: Vec<_> = (0..20u64)
            .map(|i| at(start, 2_100 + 240 * i, mv(1, 1)))
            .collect();
        p.process_frame(&ctx, &drift, true, start + Duration::from_millis(7_000));
        p.process_frame(&ctx, &[], true, start + Duration::from_secs(20));
        assert_ne!(p.export().unwrap().rating, "Excellent");
    }

    #[test]
    fn events_after_the_window_are_ignored() {
        let mut p = JitterPanel::new();
        let start = Instant::now();
        let end_ms = (SETTLE_SECS + SAMPLE_SECS) * 1000;
        let events = vec![at(start, end_ms + 5, mv(9, 9))];
        run_sample(&mut p, &events, start);
        assert_eq!(p.export().unwrap().avg_distance_px, 0.0);
    }

    #[test]
    fn rating_thresholds_and_fallback() {
        assert_eq!(jitter_rating(0.0), "Excellent");
        assert_eq!(jitter_rating(3.0), "Good");
        assert_eq!(jitter_rating(10.0), "Moderate");
        assert_eq!(jitter_rating(25.0), "High Jitter");

        let mut p = JitterPanel::new();
        let start = Instant::now();
        p.start_sample_at(start, false);
        p.record_delta(0.5, 0.0);
        p.tick(start + Duration::from_secs(SETTLE_SECS + SAMPLE_SECS));
        let e = p.export().unwrap();
        assert_eq!(e.rating, NOT_RATED);
        assert_eq!(e.distance_unit, "points");
    }
}
