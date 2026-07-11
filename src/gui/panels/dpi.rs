use eframe::egui;

use crate::export::{DpiExport, DpiSampleExport};
use crate::input_bridge::{RawInputEvent, RawInputKind};

pub struct DpiPanel {
    is_running: bool,
    target_dpi: u32,
    target_distance_inches: f32,
    accumulated_counts: f32,
    measured_dpi: f32,
    accuracy_percent: f32,
    start_pos: Option<(i32, i32)>,
    current_pos: (i32, i32),
    samples: Vec<DpiSample>,
    last_saved_dpi: u32,
    last_saved_distance: f32,
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
            accumulated_counts: 0.0,
            measured_dpi: 0.0,
            accuracy_percent: 0.0,
            start_pos: None,
            current_pos: (0, 0),
            samples: Vec::new(),
            last_saved_dpi: 800,
            last_saved_distance: 2.0,
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

    /// Set target distance (from config)
    pub fn set_target_distance(&mut self, value: f32) {
        self.target_distance_inches = value;
        self.last_saved_distance = value;
    }

    /// Get target distance
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
        ui.add_space(15.0);

        // Configuration
        ui.horizontal(|ui| {
            ui.label("Target DPI:");
            ui.add(
                egui::DragValue::new(&mut self.target_dpi)
                    .range(100..=16000)
                    .speed(50),
            );

            ui.add_space(20.0);

            ui.label("Distance (inches):");
            ui.add(
                egui::DragValue::new(&mut self.target_distance_inches)
                    .range(0.5..=12.0)
                    .speed(0.1)
                    .fixed_decimals(1),
            );
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
                self.start_pos = None;
            }
        } else if space_pressed {
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
            } else {
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
                        "Move mouse exactly {} inches in a straight line.",
                        self.target_distance_inches
                    ));
                    ui.label("Use a ruler or mousepad markings for accuracy.");
                    ui.add_space(10.0);
                    ui.label(format!(
                        "Accumulated counts: {:.1}",
                        self.accumulated_counts
                    ));
                    ui.label(format!(
                        "Expected counts: {:.0}",
                        self.target_dpi as f32 * self.target_distance_inches
                    ));
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("Press SPACE when done moving")
                            .color(egui::Color32::YELLOW),
                    );
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
                    ui.label("3. Measure a distance on your mousepad (use a ruler)");
                    ui.label("4. Press SPACE to start");
                    ui.label("5. Move the mouse that exact distance in a straight line");
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

                                let accuracy_color = if sample.accuracy >= 98.0 {
                                    egui::Color32::GREEN
                                } else if sample.accuracy >= 95.0 {
                                    egui::Color32::LIGHT_GREEN
                                } else if sample.accuracy >= 90.0 {
                                    egui::Color32::YELLOW
                                } else {
                                    egui::Color32::RED
                                };
                                ui.label(
                                    egui::RichText::new(format!("{:.1}%", sample.accuracy))
                                        .color(accuracy_color),
                                );

                                let rating = if sample.accuracy >= 98.0 {
                                    "Excellent"
                                } else if sample.accuracy >= 95.0 {
                                    "Good"
                                } else if sample.accuracy >= 90.0 {
                                    "Fair"
                                } else {
                                    "Poor"
                                };
                                ui.label(egui::RichText::new(rating).color(accuracy_color));
                                ui.end_row();
                            }
                        });
                });

            // Average
            if self.samples.len() > 1 {
                let avg_accuracy: f32 = self.samples.iter().map(|s| s.accuracy).sum::<f32>()
                    / self.samples.len() as f32;
                ui.add_space(10.0);
                ui.label(format!("Average Accuracy: {:.1}%", avg_accuracy));
            }
        }

        // Capture mouse input while running
        if self.is_running {
            if has_bridge {
                // Use raw input events — integer counts are exactly what DPI is
                // defined against, so Euclidean distance from raw counts is the
                // most accurate measurement.
                for ev in raw_events {
                    if let RawInputKind::Move { dx, dy } = ev.kind {
                        let distance =
                            (((dx as f64) * (dx as f64)) + ((dy as f64) * (dy as f64))).sqrt();
                        self.accumulated_counts += distance as f32;

                        self.current_pos.0 += dx;
                        self.current_pos.1 += dy;
                    }
                }
            } else {
                // Fallback: use egui's pointer delta when no raw input bridge
                let delta = ctx.input(|i| i.pointer.delta());
                if delta.x != 0.0 || delta.y != 0.0 {
                    let distance = ((delta.x * delta.x + delta.y * delta.y) as f64).sqrt();
                    self.accumulated_counts += distance as f32;

                    self.current_pos.0 += delta.x as i32;
                    self.current_pos.1 += delta.y as i32;
                }
            }
        }
    }

    fn start_measurement(&mut self) {
        self.is_running = true;
        self.accumulated_counts = 0.0;
        self.current_pos = (0, 0);
        self.start_pos = Some((0, 0));
    }

    fn finish_measurement(&mut self) {
        if self.accumulated_counts == 0.0 {
            self.is_running = false;
            self.start_pos = None;
            return;
        }

        self.measured_dpi = self.accumulated_counts / self.target_distance_inches;

        // Calculate accuracy as percentage of target (can be over or under)
        let raw_accuracy = self.measured_dpi / self.target_dpi as f32 * 100.0;

        // Convert to accuracy score: 100% means perfect, lower means deviation in either direction
        self.accuracy_percent = if raw_accuracy > 100.0 {
            200.0 - raw_accuracy // e.g., 110% raw -> 90% accuracy
        } else {
            raw_accuracy
        }
        .max(0.0);

        self.samples.push(DpiSample {
            target_dpi: self.target_dpi,
            measured_dpi: self.measured_dpi,
            accuracy: self.accuracy_percent,
        });

        self.is_running = false;
        self.start_pos = None;
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
                })
                .collect(),
            avg_accuracy_percent: avg_accuracy,
        })
    }
}
