use eframe::egui;
use egui_plot::{Plot, Line, PlotPoints, HLine};
use std::collections::VecDeque;
use std::time::Instant;

use crate::export::StutterExport;

pub struct StutterPanel {
    is_running: bool,
    deltas: VecDeque<f64>,
    total_stutter_count: usize,
    window_stutter_count: usize,
    total_samples: usize,
    avg_delta: f64,
    min_delta: f64,
    max_delta: f64,
    last_move_time: Option<Instant>,
    threshold_multiplier: f64,
}

impl StutterPanel {
    pub fn new() -> Self {
        Self {
            is_running: false,
            deltas: VecDeque::with_capacity(200),
            total_stutter_count: 0,
            window_stutter_count: 0,
            total_samples: 0,
            avg_delta: 0.0,
            min_delta: f64::MAX,
            max_delta: 0.0,
            last_move_time: None,
            threshold_multiplier: 2.0,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Stutter Detection");
        ui.add_space(5.0);
        ui.label("Detects movement irregularities and timing stutters.");
        ui.add_space(15.0);

        // Controls
        ui.horizontal(|ui| {
            if self.is_running {
                if ui.button("Stop").clicked() {
                    self.is_running = false;
                }
            } else {
                if ui.button("Start").clicked() {
                    self.start();
                }
            }

            if ui.button("Reset").clicked() {
                self.reset();
            }

            ui.add_space(20.0);
            ui.label("Sensitivity:");
            ui.add(egui::Slider::new(&mut self.threshold_multiplier, 1.5..=4.0)
                .fixed_decimals(1)
                .suffix("x"));
            ui.label(egui::RichText::new("(lower = more sensitive)").weak().size(11.0));
        });

        ui.add_space(20.0);

        // Calculate derived values
        let polling_rate = if self.avg_delta > 0.0 { 1000.0 / self.avg_delta } else { 0.0 };
        let stutter_threshold = self.avg_delta * self.threshold_multiplier;

        // Main stats
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(20.0)
            .rounding(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Polling rate (most important)
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Polling Rate").weak().size(12.0));
                        ui.label(egui::RichText::new(format!("{:.0} Hz", polling_rate))
                            .size(24.0)
                            .strong()
                            .color(if polling_rate > 900.0 {
                                egui::Color32::GREEN
                            } else if polling_rate > 400.0 {
                                egui::Color32::LIGHT_GREEN
                            } else if polling_rate > 100.0 {
                                egui::Color32::YELLOW
                            } else {
                                egui::Color32::WHITE
                            }));
                    });
                    ui.add_space(30.0);

                    // Average interval
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Avg Interval").weak().size(12.0));
                        ui.label(egui::RichText::new(format!("{:.2} ms", self.avg_delta)).size(20.0).strong());
                    });
                    ui.add_space(30.0);

                    // Min/Max
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Range").weak().size(12.0));
                        let min_val = if self.min_delta == f64::MAX { 0.0 } else { self.min_delta };
                        ui.label(egui::RichText::new(format!("{:.1} - {:.1} ms", min_val, self.max_delta)).size(16.0));
                    });
                    ui.add_space(30.0);

                    // Total stutters
                    let color = if self.total_stutter_count == 0 {
                        egui::Color32::GREEN
                    } else if self.total_stutter_count < 10 {
                        egui::Color32::YELLOW
                    } else {
                        egui::Color32::RED
                    };

                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Total Stutters").weak().size(12.0));
                        ui.label(egui::RichText::new(format!("{}", self.total_stutter_count)).color(color).size(24.0).strong());
                    });
                    ui.add_space(30.0);

                    // Samples
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Samples").weak().size(12.0));
                        ui.label(egui::RichText::new(format!("{}", self.total_samples)).size(20.0));
                    });
                });
            });

        ui.add_space(20.0);

        // Graph
        ui.heading("Delta Time Graph");
        ui.label(egui::RichText::new("Shows time between mouse events. Spikes indicate stutters.").weak().size(12.0));

        let avg = self.avg_delta;

        let points: PlotPoints = self.deltas
            .iter()
            .enumerate()
            .map(|(i, &d)| [i as f64, d])
            .collect();

        let line = Line::new(points)
            .color(egui::Color32::from_rgb(100, 200, 255))
            .name("Delta Time");

        let avg_line = HLine::new(avg)
            .color(egui::Color32::from_rgb(100, 255, 100))
            .name("Average");

        let upper_threshold = HLine::new(stutter_threshold)
            .color(egui::Color32::from_rgb(255, 100, 100))
            .style(egui_plot::LineStyle::dashed_loose())
            .name("Stutter Threshold");

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
                plot_ui.hline(avg_line);
                plot_ui.hline(upper_threshold);
            });

        ui.add_space(8.0);

        // Legend
        ui.horizontal(|ui| {
            ui.colored_label(egui::Color32::from_rgb(100, 200, 255), "■ Delta Time");
            ui.add_space(15.0);
            ui.colored_label(egui::Color32::from_rgb(100, 255, 100), "■ Average");
            ui.add_space(15.0);
            ui.colored_label(egui::Color32::from_rgb(255, 100, 100), "■ Stutter Threshold");
        });

        ui.add_space(20.0);

        // Rating
        if self.total_samples >= 50 {
            let stutter_rate = self.total_stutter_count as f64 / self.total_samples as f64 * 100.0;
            let jitter = self.max_delta - self.min_delta.min(self.max_delta);

            let (rating, color, message) = if self.total_stutter_count == 0 && jitter < 5.0 {
                ("Excellent", egui::Color32::GREEN, "No stutters detected, very consistent timing")
            } else if stutter_rate < 1.0 && jitter < 10.0 {
                ("Good", egui::Color32::LIGHT_GREEN, "Minimal stuttering, good consistency")
            } else if stutter_rate < 5.0 {
                ("Fair", egui::Color32::YELLOW, "Some stuttering detected")
            } else {
                ("Poor", egui::Color32::RED, "Significant stuttering - check USB connection, drivers, or system load")
            };

            egui::Frame::dark_canvas(ui.style())
                .inner_margin(15.0)
                .rounding(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Rating:");
                        ui.label(egui::RichText::new(rating).size(18.0).strong().color(color));
                        ui.add_space(20.0);
                        ui.label(egui::RichText::new(message).color(egui::Color32::GRAY));
                    });
                    ui.add_space(5.0);
                    ui.label(format!("Stutter rate: {:.1}% | Jitter: {:.1} ms", stutter_rate, jitter));
                });
        }

        ui.add_space(20.0);

        // Instructions
        egui::Frame::none()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .rounding(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Instructions").strong());
                ui.label("1. Click 'Start' and move your mouse in circles or back and forth");
                ui.label("2. Keep moving continuously - the test measures timing between movements");
                ui.label("3. Spikes in the graph above the red line indicate stutters");
                ui.add_space(5.0);
                ui.label(egui::RichText::new("A flat, consistent line near the average indicates smooth tracking.").weak());
            });

        // Capture real mouse input and measure timing
        if self.is_running {
            // IMPORTANT: Request continuous repaints for accurate timing measurement
            ctx.request_repaint();

            let delta = ctx.input(|i| i.pointer.delta());

            // Only record timing when mouse is actually moving
            if delta.x != 0.0 || delta.y != 0.0 {
                let now = Instant::now();

                // If we have a previous movement, calculate the delta time
                if let Some(last_time) = self.last_move_time {
                    let time_since_last = now.duration_since(last_time).as_secs_f64() * 1000.0;

                    // Only record reasonable deltas (ignore gaps > 100ms when mouse was stopped)
                    if time_since_last < 100.0 && time_since_last > 0.1 {
                        self.deltas.push_back(time_since_last);
                        if self.deltas.len() > 200 {
                            self.deltas.pop_front();
                        }

                        self.total_samples += 1;

                        // Calculate stats
                        self.avg_delta = self.deltas.iter().sum::<f64>() / self.deltas.len() as f64;
                        self.min_delta = self.deltas.iter().cloned().fold(f64::MAX, f64::min);
                        self.max_delta = self.deltas.iter().cloned().fold(0.0, f64::max);

                        // Check if this is a stutter
                        let threshold = self.avg_delta * self.threshold_multiplier;
                        if time_since_last > threshold {
                            self.total_stutter_count += 1;
                        }

                        // Count stutters in current window
                        self.window_stutter_count = self.deltas.iter()
                            .filter(|d| **d > threshold)
                            .count();
                    }
                }

                // Always update last move time when mouse moves
                self.last_move_time = Some(now);
            }
        }
    }

    fn start(&mut self) {
        self.is_running = true;
        self.last_move_time = None;
        self.deltas.clear();
        self.total_stutter_count = 0;
        self.window_stutter_count = 0;
        self.total_samples = 0;
        self.avg_delta = 0.0;
        self.min_delta = f64::MAX;
        self.max_delta = 0.0;
    }

    fn reset(&mut self) {
        self.is_running = false;
        self.last_move_time = None;
        self.deltas.clear();
        self.total_stutter_count = 0;
        self.window_stutter_count = 0;
        self.total_samples = 0;
        self.avg_delta = 0.0;
        self.min_delta = f64::MAX;
        self.max_delta = 0.0;
    }

    pub fn export(&self) -> Option<StutterExport> {
        if self.total_samples == 0 {
            return None;
        }
        let polling_rate = if self.avg_delta > 0.0 { 1000.0 / self.avg_delta } else { 0.0 };
        let stutter_rate = self.total_stutter_count as f64 / self.total_samples as f64 * 100.0;
        Some(StutterExport {
            total_stutter_count: self.total_stutter_count,
            total_samples: self.total_samples,
            avg_delta_ms: self.avg_delta,
            min_delta_ms: if self.min_delta == f64::MAX { 0.0 } else { self.min_delta },
            max_delta_ms: self.max_delta,
            polling_rate_hz: polling_rate,
            threshold_multiplier: self.threshold_multiplier,
            stutter_rate_percent: stutter_rate,
            deltas: self.deltas.iter().cloned().collect(),
        })
    }
}
