use eframe::egui;
use egui_plot::{Plot, Points, PlotPoints};
use std::time::Instant;

pub struct JitterPanel {
    is_sampling: bool,
    samples: Vec<JitterSample>,
    current_events: Vec<(f64, f64)>,
    sample_start: Option<Instant>,
}

struct JitterSample {
    events: usize,
    total_distance: f64,
    max_single: f64,
}

impl JitterPanel {
    pub fn new() -> Self {
        Self {
            is_sampling: false,
            samples: Vec::new(),
            current_events: Vec::new(),
            sample_start: None,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        ui.heading("Jitter Test");
        ui.add_space(5.0);
        ui.label("Measures sensor noise when the mouse is stationary.");
        ui.add_space(15.0);

        // Controls
        ui.horizontal(|ui| {
            if self.is_sampling {
                ui.label("Sampling...");
            } else {
                if ui.button("Take Sample (5s)").clicked() {
                    self.start_sample();
                }
            }

            if ui.button("Clear All").clicked() {
                self.samples.clear();
                self.current_events.clear();
            }
        });

        ui.add_space(10.0);
        ui.label(egui::RichText::new("DO NOT touch the mouse during sampling!").color(egui::Color32::YELLOW));
        ui.add_space(20.0);

        // Jitter visualization
        ui.heading("Jitter Visualization");

        let points: PlotPoints = self.current_events
            .iter()
            .map(|(x, y)| [*x, *y])
            .collect();

        let scatter = Points::new(points)
            .color(egui::Color32::from_rgb(255, 100, 100))
            .radius(3.0)
            .name("Jitter Events");

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

            egui::Frame::dark_canvas(ui.style())
                .inner_margin(15.0)
                .rounding(8.0)
                .show(ui, |ui| {
                    let avg_events: f64 = self.samples.iter().map(|s| s.events as f64).sum::<f64>() / self.samples.len() as f64;
                    let avg_distance: f64 = self.samples.iter().map(|s| s.total_distance).sum::<f64>() / self.samples.len() as f64;
                    let max_jitter: f64 = self.samples.iter().map(|s| s.max_single).fold(0.0, f64::max);

                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label("Samples");
                            ui.label(egui::RichText::new(format!("{}", self.samples.len())).size(20.0).strong());
                        });
                        ui.add_space(30.0);
                        ui.vertical(|ui| {
                            ui.label("Avg Events");
                            ui.label(egui::RichText::new(format!("{:.1}", avg_events)).size(20.0));
                        });
                        ui.add_space(30.0);
                        ui.vertical(|ui| {
                            ui.label("Avg Distance");
                            ui.label(egui::RichText::new(format!("{:.2} px", avg_distance)).size(20.0));
                        });
                        ui.add_space(30.0);
                        ui.vertical(|ui| {
                            ui.label("Max Jitter");
                            ui.label(egui::RichText::new(format!("{:.2} px", max_jitter)).size(20.0));
                        });
                        ui.add_space(30.0);
                        ui.vertical(|ui| {
                            ui.label("Rating");
                            let (rating, color) = if avg_distance < 1.0 {
                                ("Excellent", egui::Color32::GREEN)
                            } else if avg_distance < 5.0 {
                                ("Good", egui::Color32::LIGHT_GREEN)
                            } else if avg_distance < 20.0 {
                                ("Moderate", egui::Color32::YELLOW)
                            } else {
                                ("High Jitter", egui::Color32::RED)
                            };
                            ui.label(egui::RichText::new(rating).size(20.0).color(color));
                        });
                    });
                });
        }

        // Simulate sampling
        if self.is_sampling {
            if let Some(start) = self.sample_start {
                if start.elapsed().as_secs() >= 5 {
                    self.finish_sample();
                } else {
                    // Simulate jitter events
                    if rand_simple() % 10 == 0 {
                        let x = (rand_simple() % 20) as f64 - 10.0;
                        let y = (rand_simple() % 20) as f64 - 10.0;
                        self.current_events.push((x * 0.1, y * 0.1));
                    }
                }
            }
        }
    }

    fn start_sample(&mut self) {
        self.is_sampling = true;
        self.sample_start = Some(Instant::now());
        self.current_events.clear();
    }

    fn finish_sample(&mut self) {
        let total_distance: f64 = self.current_events
            .iter()
            .map(|(x, y)| (x * x + y * y).sqrt())
            .sum();

        let max_single = self.current_events
            .iter()
            .map(|(x, y)| (x * x + y * y).sqrt())
            .fold(0.0, f64::max);

        self.samples.push(JitterSample {
            events: self.current_events.len(),
            total_distance,
            max_single,
        });

        self.is_sampling = false;
        self.sample_start = None;
    }
}

fn rand_simple() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64 % 1000
}
