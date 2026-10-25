//! DPI accuracy panel
//!
//! The user moves the mouse a measured physical distance; the measured DPI is
//! the straight-line distance between where the movement started and where
//! it ended, in raw counts, divided by that distance. Using net displacement
//! (not the summed path) means wobble and an overshoot that is corrected
//! back to the mark do not inflate the result. Needs raw counts: the
//! window-system pointer is scaled by OS acceleration.

use eframe::egui;

use super::polling::raw_input_required_notice;
use crate::config::{DPI_DISTANCE_RANGE, DPI_TARGET_RANGE};
use crate::export::{DpiExport, DpiSampleExport};
use crate::input_bridge::{RawInputEvent, RawInputKind};
use mouse_testkit::analysis::dpi::{is_setup_error, SETUP_ERROR_FRACTION};

/// Selectable target DPI and distance (inches): the ranges saved settings
/// are clamped to on load, so a value set here survives a restart.
const MIN_TARGET_DPI: u32 = *DPI_TARGET_RANGE.start();
const MAX_TARGET_DPI: u32 = *DPI_TARGET_RANGE.end();
const CM_PER_INCH: f32 = 2.54;
const MAX_DISTANCE_INCHES: f32 = *DPI_DISTANCE_RANGE.end();
const MIN_DISTANCE_INCHES: f32 = *DPI_DISTANCE_RANGE.start();

/// Measured DPI from the net displacement in counts over a known distance.
fn measured_dpi(net_dx: i64, net_dy: i64, distance_inches: f32) -> f32 {
    let net = ((net_dx as f64).powi(2) + (net_dy as f64).powi(2)).sqrt();
    (net / distance_inches as f64) as f32
}

/// Accuracy score: 100% is exact, lower means deviation in either direction
/// (110% of target scores 90%).
fn accuracy_percent(measured: f32, target: u32) -> f32 {
    let raw = measured / target as f32 * 100.0;
    if raw > 100.0 { 200.0 - raw } else { raw }.max(0.0)
}

/// Rating label for one measurement.
fn dpi_rating(measured: f32, target: u32) -> &'static str {
    let accuracy = accuracy_percent(measured, target);
    if is_setup_error(measured as f64, target as f64) {
        "Check setup"
    } else if accuracy >= 98.0 {
        "Excellent"
    } else if accuracy >= 95.0 {
        "Good"
    } else if accuracy >= 90.0 {
        "Fair"
    } else {
        "Poor"
    }
}

pub struct DpiPanel {
    is_running: bool,
    target_dpi: u32,
    target_distance_inches: f32,
    /// Show and edit the distance in centimetres instead of inches.
    use_cm: bool,
    measured_dpi: f32,
    accuracy_percent: f32,
    /// Net displacement since the measurement started, in raw counts.
    current_pos: (i64, i64),
    samples: Vec<DpiSample>,
    last_saved_dpi: u32,
    last_saved_distance: f32,
    /// The last measurement was cancelled because raw input stopped.
    cancelled_for_input: bool,
    /// Reports the system dropped took some of the measurement's counts
    /// (see `RawInputEvent::lost_before`): it will not be recorded...
    spoiled: bool,
    /// ... and the last one was not, for that reason.
    discarded_for_gap: bool,
}

struct DpiSample {
    target_dpi: u32,
    measured_dpi: f32,
    accuracy: f32,
}

impl DpiPanel {
    pub fn new() -> Self {
        Self {
            is_running: false,
            target_dpi: 800,
            target_distance_inches: 2.0,
            use_cm: false,
            measured_dpi: 0.0,
            accuracy_percent: 0.0,
            current_pos: (0, 0),
            samples: Vec::new(),
            last_saved_dpi: 800,
            last_saved_distance: 2.0,
            cancelled_for_input: false,
            spoiled: false,
            discarded_for_gap: false,
        }
    }

    /// Set target DPI (from config)
    pub fn set_target_dpi(&mut self, value: u32) {
        self.target_dpi = value;
        self.last_saved_dpi = value;
    }

    /// Get target DPI
    pub fn get_target_dpi(&self) -> u32 {
        self.target_dpi
    }

    /// Set target distance in inches (from config)
    pub fn set_target_distance(&mut self, value: f32) {
        self.target_distance_inches = value;
        self.last_saved_distance = value;
    }

    /// Get target distance in inches
    pub fn get_target_distance(&self) -> f32 {
        self.target_distance_inches
    }

    /// Check if settings have changed since last save
    pub fn settings_changed(&mut self) -> bool {
        let changed = self.target_dpi != self.last_saved_dpi
            || (self.target_distance_inches - self.last_saved_distance).abs() > 0.01;
        if changed {
            self.last_saved_dpi = self.target_dpi;
            self.last_saved_distance = self.target_distance_inches;
        }
        changed
    }

    pub fn is_running(&self) -> bool {
        self.is_running
    }

    fn distance_label(&self) -> String {
        if self.use_cm {
            format!("{:.1} cm", self.target_distance_inches * CM_PER_INCH)
        } else {
            format!("{:.1} inches", self.target_distance_inches)
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
    ) {
        ui.heading("DPI Accuracy Test");
        ui.add_space(5.0);
        ui.label("Measures actual DPI against your mouse's configured DPI setting.");
        if !has_bridge {
            // Counts seen so far are only part of the movement, and counts
            // from whatever input comes next must not be added to them
            self.cancelled_for_input |= self.is_running;
            ui.add_space(5.0);
            raw_input_required_notice(ui, "DPI (counts per inch)");
            self.is_running = false;
        }
        if self.cancelled_for_input {
            ui.add_space(5.0);
            ui.label(
                egui::RichText::new(
                    "The last measurement was cancelled: raw input from the mouse stopped during it (it was unplugged, its receiver or link dropped out, or another input was chosen), so not all of its movement was counted. Press SPACE to measure again.",
                )
                .color(egui::Color32::LIGHT_RED),
            );
        }
        if self.discarded_for_gap {
            ui.add_space(5.0);
            ui.label(
                egui::RichText::new(
                    "The last measurement was not recorded: some of its movement was lost (the computer did not read the mouse in time and the system dropped reports). Press SPACE to measure again.",
                )
                .color(egui::Color32::LIGHT_RED),
            );
        }
        ui.add_space(15.0);

        // Configuration
        ui.horizontal(|ui| {
            ui.label("Target DPI:");
            ui.add(
                egui::DragValue::new(&mut self.target_dpi)
                    .range(MIN_TARGET_DPI..=MAX_TARGET_DPI)
                    .speed(50),
            );

            ui.add_space(20.0);

            ui.label("Distance:");
            if self.use_cm {
                let mut cm = self.target_distance_inches * CM_PER_INCH;
                let response = ui.add(
                    egui::DragValue::new(&mut cm)
                        .range(
                            MIN_DISTANCE_INCHES * CM_PER_INCH..=MAX_DISTANCE_INCHES * CM_PER_INCH,
                        )
                        .speed(0.1)
                        .fixed_decimals(1)
                        .suffix(" cm"),
                );
                if response.changed() {
                    self.target_distance_inches = cm / CM_PER_INCH;
                }
            } else {
                ui.add(
                    egui::DragValue::new(&mut self.target_distance_inches)
                        .range(MIN_DISTANCE_INCHES..=MAX_DISTANCE_INCHES)
                        .speed(0.1)
                        .fixed_decimals(1)
                        .suffix(" in"),
                );
            }
            ui.selectable_value(&mut self.use_cm, false, "in");
            ui.selectable_value(&mut self.use_cm, true, "cm");
        });

        ui.add_space(15.0);

        // Keyboard controls
        let space_pressed = ctx.input(|i| i.key_pressed(egui::Key::Space));
        let escape_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        if self.is_running {
            if space_pressed {
                self.finish_measurement();
            }
            if escape_pressed {
                self.is_running = false;
            }
        } else if space_pressed && has_bridge {
            self.start_measurement();
        }

        // Controls display
        ui.horizontal(|ui| {
            if self.is_running {
                ui.label(
                    egui::RichText::new("Press SPACE to finish | ESC to cancel")
                        .strong()
                        .color(egui::Color32::GREEN),
                );
            } else if has_bridge {
                ui.label(egui::RichText::new("Press SPACE to start measurement").strong());
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Clear Results").clicked() {
                    self.samples.clear();
                }
            });
        });

        ui.add_space(20.0);

        // Instructions
        if self.is_running {
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(40, 60, 40))
                .inner_margin(15.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Measurement in Progress")
                            .strong()
                            .color(egui::Color32::GREEN),
                    );
                    ui.add_space(5.0);
                    ui.label(format!(
                        "Move the mouse exactly {} from the start mark to the end mark.",
                        self.distance_label()
                    ));
                    ui.label("Use a ruler or mousepad markings. Wobble or overshooting and coming back to the mark is fine: only the start-to-end distance counts.");
                    ui.add_space(10.0);
                    let (x, y) = self.current_pos;
                    ui.label(format!(
                        "Start-to-end distance: {:.0} counts",
                        ((x as f64).powi(2) + (y as f64).powi(2)).sqrt()
                    ));
                    ui.label(format!(
                        "Expected counts: {:.0}",
                        self.target_dpi as f32 * self.target_distance_inches
                    ));
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("Press SPACE when the mouse is on the end mark")
                            .color(egui::Color32::YELLOW),
                    );
                    if self.spoiled {
                        ui.label(
                            egui::RichText::new(
                                "Some movement was lost (the computer fell behind reading the mouse), so this measurement will not be recorded. Finish it with SPACE, then measure again.",
                            )
                            .color(egui::Color32::LIGHT_RED),
                        );
                    }
                });
        } else {
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(15.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Instructions").strong());
                    ui.label("1. Set your mouse's DPI in its software/hardware");
                    ui.label("2. Enter the target DPI above");
                    ui.label("3. Mark a distance on your mousepad with a ruler (inches or cm); longer is more accurate, e.g. 10 cm or more");
                    ui.label("4. Put the mouse on the start mark and press SPACE");
                    ui.label("5. Move the mouse to the end mark");
                    ui.label("6. Press SPACE to finish");
                });
        }

        ui.add_space(20.0);

        // Results
        if !self.samples.is_empty() {
            ui.heading("Test Results");

            egui::Frame::dark_canvas(ui.style())
                .inner_margin(15.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    egui::Grid::new("dpi_results")
                        .num_columns(4)
                        .spacing([20.0, 8.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("Target DPI").strong());
                            ui.label(egui::RichText::new("Measured DPI").strong());
                            ui.label(egui::RichText::new("Accuracy").strong());
                            ui.label(egui::RichText::new("Rating").strong());
                            ui.end_row();

                            for sample in &self.samples {
                                ui.label(format!("{}", sample.target_dpi));
                                ui.label(format!("{:.0}", sample.measured_dpi));

                                let rating = dpi_rating(sample.measured_dpi, sample.target_dpi);
                                let accuracy_color = match rating {
                                    "Excellent" => egui::Color32::GREEN,
                                    "Good" => egui::Color32::LIGHT_GREEN,
                                    "Fair" | "Check setup" => egui::Color32::YELLOW,
                                    _ => egui::Color32::RED,
                                };
                                ui.label(
                                    egui::RichText::new(format!("{:.1}%", sample.accuracy))
                                        .color(accuracy_color),
                                );
                                ui.label(egui::RichText::new(rating).color(accuracy_color));
                                ui.end_row();
                            }
                        });
                });

            if self
                .samples
                .iter()
                .any(|s| dpi_rating(s.measured_dpi, s.target_dpi) == "Check setup")
            {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!(
                        "\"Check setup\": more than {:.0}% off target. That is almost always a different DPI stage, a wrong distance, or the movement not covering the marks - not the sensor.",
                        SETUP_ERROR_FRACTION * 100.0
                    ))
                    .color(egui::Color32::YELLOW),
                );
            }

            // Average
            if self.samples.len() > 1 {
                let avg_accuracy: f32 = self.samples.iter().map(|s| s.accuracy).sum::<f32>()
                    / self.samples.len() as f32;
                ui.add_space(10.0);
                ui.label(format!("Average Accuracy: {:.1}%", avg_accuracy));
            }
        }

        // Capture mouse input while running (raw counts only)
        if self.is_running && has_bridge {
            self.process_raw_events(raw_events);
        }
    }

    /// Track the net displacement in raw counts.
    fn process_raw_events(&mut self, raw_events: &[RawInputEvent]) {
        for ev in raw_events {
            self.spoiled |= ev.lost_before;
            if let RawInputKind::Move { dx, dy } = ev.kind {
                self.current_pos.0 += dx as i64;
                self.current_pos.1 += dy as i64;
            }
        }
    }

    fn start_measurement(&mut self) {
        self.is_running = true;
        self.cancelled_for_input = false;
        self.spoiled = false;
        self.discarded_for_gap = false;
        self.current_pos = (0, 0);
    }

    /// End the measurement on the end mark. One that lost counts is not
    /// recorded: it would read low (as in the CLI test).
    fn finish_measurement(&mut self) {
        self.is_running = false;
        if std::mem::take(&mut self.spoiled) {
            self.discarded_for_gap = true;
            return;
        }
        if self.current_pos == (0, 0) || self.target_distance_inches <= 0.0 {
            return;
        }

        self.measured_dpi = measured_dpi(
            self.current_pos.0,
            self.current_pos.1,
            self.target_distance_inches,
        );
        self.accuracy_percent = accuracy_percent(self.measured_dpi, self.target_dpi);

        self.samples.push(DpiSample {
            target_dpi: self.target_dpi,
            measured_dpi: self.measured_dpi,
            accuracy: self.accuracy_percent,
        });
    }

    pub fn export(&self) -> Option<DpiExport> {
        if self.samples.is_empty() {
            return None;
        }
        let avg_accuracy =
            self.samples.iter().map(|s| s.accuracy).sum::<f32>() / self.samples.len() as f32;
        Some(DpiExport {
            target_dpi: self.target_dpi,
            samples: self
                .samples
                .iter()
                .map(|s| DpiSampleExport {
                    target_dpi: s.target_dpi,
                    measured_dpi: s.measured_dpi,
                    accuracy_percent: s.accuracy,
                    rating: dpi_rating(s.measured_dpi, s.target_dpi).to_string(),
                })
                .collect(),
            avg_accuracy_percent: avg_accuracy,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn mv(dx: i32, dy: i32) -> RawInputEvent {
        RawInputEvent {
            kind: RawInputKind::Move { dx, dy },
            timestamp: Instant::now(),
            lost_before: false,
        }
    }

    fn measure(panel: &mut DpiPanel, events: &[RawInputEvent]) -> f32 {
        panel.start_measurement();
        panel.process_raw_events(events);
        panel.finish_measurement();
        panel.samples.last().map(|s| s.measured_dpi).unwrap_or(0.0)
    }

    #[test]
    fn wobble_and_overshoot_do_not_inflate_dpi() {
        // An exact 800 DPI mouse moved 2 inches (1600 counts) by a real hand:
        // a little vertical wobble, an 80-count overshoot and back to the mark.
        let mut p = DpiPanel::new();
        p.set_target_dpi(800);
        p.set_target_distance(2.0);
        let mut events = Vec::new();
        for i in 0..800 {
            let dy = match i % 8 {
                0 => 1,
                4 => -1,
                _ => 0,
            };
            events.push(mv(2, dy));
        }
        events.extend((0..40).map(|_| mv(2, 0)));
        events.extend((0..40).map(|_| mv(-2, 0)));
        let dpi = measure(&mut p, &events);
        assert!((dpi - 800.0).abs() < 1.0, "{}", dpi);
        assert_eq!(dpi_rating(dpi, 800), "Excellent");
    }

    #[test]
    fn a_sensor_that_overcounts_is_still_caught() {
        // 8% high: 1728 counts over 2 inches at a nominal 800 DPI
        let mut p = DpiPanel::new();
        p.set_target_dpi(800);
        p.set_target_distance(2.0);
        let events: Vec<_> = (0..864).map(|_| mv(2, 0)).collect();
        let dpi = measure(&mut p, &events);
        assert!((dpi - 864.0).abs() < 0.5);
        assert_eq!(dpi_rating(dpi, 800), "Fair");
    }

    #[test]
    fn export_keeps_the_check_setup_rating() {
        // A 1600 DPI stage measured against a target of 800: 0% accuracy
        // in the export must not read as a dead sensor
        let mut p = DpiPanel::new();
        p.set_target_dpi(800);
        p.set_target_distance(2.0);
        let events: Vec<_> = (0..1600).map(|_| mv(2, 0)).collect();
        measure(&mut p, &events);
        let e = p.export().unwrap();
        assert_eq!(e.samples[0].rating, "Check setup");
    }

    #[test]
    fn diagonal_movement_uses_straight_line_distance() {
        // 3-4-5 triangle: 1200 right, 1600 down = 2000 counts
        assert!((measured_dpi(1200, 1600, 2.5) - 800.0).abs() < 1e-3);
        assert!((measured_dpi(-1200, -1600, 2.5) - 800.0).abs() < 1e-3);
    }

    #[test]
    fn far_off_results_point_at_the_setup_not_the_sensor() {
        assert_eq!(dpi_rating(1600.0, 800), "Check setup");
        assert_eq!(dpi_rating(400.0, 800), "Check setup");
        assert_eq!(dpi_rating(700.0, 800), "Poor");
        assert_eq!(dpi_rating(26_000.0, 26_000), "Excellent");
        assert!((accuracy_percent(880.0, 800) - 90.0).abs() < 1e-3);
    }

    #[test]
    fn no_movement_records_nothing() {
        let mut p = DpiPanel::new();
        assert_eq!(measure(&mut p, &[mv(5, 0), mv(-5, 0)]), 0.0);
        assert!(p.export().is_none());
    }

    #[test]
    fn centimetre_distance_converts_to_inches() {
        let mut p = DpiPanel::new();
        p.use_cm = true;
        p.set_target_distance(10.0 / CM_PER_INCH);
        assert_eq!(p.distance_label(), "10.0 cm");
        p.set_target_dpi(1000);
        // 10 cm = 3.937 in, at 1000 DPI = 3937 counts
        let dpi = measure(&mut p, &[mv(3937, 0)]);
        assert!((dpi - 1000.0).abs() < 2.0, "{}", dpi);
    }

    #[test]
    fn a_measurement_that_lost_counts_is_not_recorded() {
        // 800 DPI over 2 in: 1600 counts, of which the system dropped about
        // a fifth while the reader fell behind
        let mut p = DpiPanel::new();
        let mut events: Vec<_> = (0..320).map(|_| mv(4, 0)).collect();
        events[160].lost_before = true;
        events.truncate(320 - 80);
        assert_eq!(measure(&mut p, &events), 0.0);
        assert!(p.export().is_none());
        assert!(p.discarded_for_gap);
        // Measured again without a gap: recorded
        let dpi = measure(&mut p, &[mv(1600, 0)]);
        assert!((dpi - 800.0).abs() < 1.0, "{}", dpi);
        assert!(!p.discarded_for_gap);
        assert_eq!(p.export().unwrap().samples.len(), 1);
    }

    #[test]
    fn fallback_does_not_measure() {
        let ctx = egui::Context::default();
        let mut p = DpiPanel::new();
        p.start_measurement();
        let events: Vec<_> = (0..100).map(|_| mv(4, 0)).collect();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let c = ui.ctx().clone();
            p.ui(ui, &c, &events, false);
        });
        assert!(!p.is_running());
        assert_eq!(p.current_pos, (0, 0));
    }

    #[test]
    fn losing_raw_input_cancels_the_measurement_without_mixing() {
        // Half the movement from the measured mouse, then it is unplugged
        // and another one is read: nothing of either is recorded, and the
        // panel says why
        let ctx = egui::Context::default();
        let frame = |p: &mut DpiPanel, events: &[RawInputEvent], raw: bool| {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                let c = ui.ctx().clone();
                p.ui(ui, &c, events, raw);
            });
        };
        let mut p = DpiPanel::new();
        p.start_measurement();
        let half: Vec<_> = (0..400).map(|_| mv(2, 0)).collect();
        frame(&mut p, &half, true);
        assert_eq!(p.current_pos, (800, 0));
        frame(&mut p, &[], false);
        assert!(!p.is_running() && p.cancelled_for_input);
        frame(&mut p, &half, true);
        assert_eq!(p.current_pos, (800, 0));
        assert!(p.export().is_none());
        // The next measurement starts clean
        p.start_measurement();
        assert!(!p.cancelled_for_input);
        assert_eq!(p.current_pos, (0, 0));
    }
}
