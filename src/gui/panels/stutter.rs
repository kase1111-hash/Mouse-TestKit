use eframe::egui;
use egui_plot::{Plot, Line, PlotPoints, HLine};
use std::collections::VecDeque;
use std::time::Instant;

pub struct StutterPanel {
    is_running: bool,
    deltas: VecDeque<f64>,
    stutter_count: usize,
    avg_delta: f64,
    min_delta: f64,
    max_delta: f64,
    last_update: Instant,
    threshold: f64,
}

impl StutterPanel {
    pub fn new() -> Self {
        Self {
            is_running: false,
            deltas: VecDeque::with_capacity(100),
            stutter_count: 0,
            avg_delta: 0.0,
            min_delta: f64::MAX,
            max_delta: 0.0,
            last_update: Instant::now(),
            threshold: 4.0,
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
            ui.label("Threshold (ms):");
            ui.add(egui::Slider::new(&mut self.threshold, 1.0..=20.0).fixed_decimals(1));
        });

        ui.add_space(20.0);

        // Stats
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(20.0)
            .rounding(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    self.stat_box(ui, "Avg Interval", &format!("{:.2} ms", self.avg_delta));
                    ui.add_space(30.0);
                    self.stat_box(ui, "Min", &format!("{:.2} ms", if self.min_delta == f64::MAX { 0.0 } else { self.min_delta }));
                    ui.add_space(30.0);
                    self.stat_box(ui, "Max", &format!("{:.2} ms", self.max_delta));
                    ui.add_space(30.0);

                    let color = if self.stutter_count == 0 {
                        egui::Color32::GREEN
                    } else if self.stutter_count < 10 {
                        egui::Color32::YELLOW
                    } else {
                        egui::Color32::RED
                    };

                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Stutters").weak().size(12.0));
                        ui.label(egui::RichText::new(format!("{}", self.stutter_count)).color(color).size(24.0).strong());
                    });
                });
            });

        ui.add_space(20.0);

        // Graph
        ui.heading("Delta Time Graph");

        let avg = self.avg_delta;
        let threshold = self.threshold;

        let points: PlotPoints = self.deltas
            .iter()
            .enumerate()
            .map(|(i, &d)| [i as f64, d])
            .collect();

        let line = Line::new(points)
            .color(egui::Color32::from_rgb(100, 200, 255))
            .name("Delta Time");

        let avg_line = HLine::new(avg)
            .color(egui::Color32::from_rgb(255, 255, 100))
            .name("Average");

        let upper_threshold = HLine::new(avg + threshold)
            .color(egui::Color32::from_rgb(255, 100, 100))
            .style(egui_plot::LineStyle::dashed_loose())
            .name("Threshold");

        Plot::new("stutter_plot")
            .height(300.0)
            .show_axes(true)
            .show_grid(true)
            .allow_drag(false)
            .allow_zoom(false)
            .show(ui, |plot_ui| {
                plot_ui.line(line);
                plot_ui.hline(avg_line);
                plot_ui.hline(upper_threshold);
            });

        ui.add_space(10.0);

        // Legend
        ui.horizontal(|ui| {
            ui.colored_label(egui::Color32::from_rgb(100, 200, 255), "■ Delta Time");
            ui.add_space(10.0);
            ui.colored_label(egui::Color32::from_rgb(255, 255, 100), "■ Average");
            ui.add_space(10.0);
            ui.colored_label(egui::Color32::from_rgb(255, 100, 100), "■ Stutter Threshold");
        });

        ui.add_space(20.0);

        // Instructions
        egui::Frame::none()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .rounding(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Instructions").strong());
                ui.label("1. Click 'Start' and move your mouse in circles");
                ui.label("2. The graph shows time between mouse events");
                ui.label("3. Spikes above the threshold indicate stutters");
                ui.add_space(5.0);
                ui.label(egui::RichText::new("Consistent flat lines indicate smooth tracking.").weak());
            });

        // Simulate data
        if self.is_running {
            self.simulate_update();
        }
    }

    fn stat_box(&self, ui: &mut egui::Ui, label: &str, value: &str) {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(label).weak().size(12.0));
            ui.label(egui::RichText::new(value).size(20.0).strong());
        });
    }

    fn start(&mut self) {
        self.is_running = true;
        self.last_update = Instant::now();
    }

    fn reset(&mut self) {
        self.is_running = false;
        self.deltas.clear();
        self.stutter_count = 0;
        self.avg_delta = 0.0;
        self.min_delta = f64::MAX;
        self.max_delta = 0.0;
    }

    fn simulate_update(&mut self) {
        if self.last_update.elapsed().as_millis() >= 10 {
            // Simulate delta times with occasional stutters
            let base = 1.0;
            let noise = (rand_simple() % 50) as f64 / 100.0;
            let stutter = if rand_simple() % 100 < 5 { 8.0 } else { 0.0 };
            let delta = base + noise + stutter;

            self.deltas.push_back(delta);
            if self.deltas.len() > 100 {
                self.deltas.pop_front();
            }

            // Calculate stats
            if !self.deltas.is_empty() {
                self.avg_delta = self.deltas.iter().sum::<f64>() / self.deltas.len() as f64;
                self.min_delta = self.deltas.iter().cloned().fold(f64::MAX, f64::min);
                self.max_delta = self.deltas.iter().cloned().fold(0.0, f64::max);

                self.stutter_count = self.deltas.iter()
                    .filter(|d| (**d - self.avg_delta).abs() > self.threshold)
                    .count();
            }

            self.last_update = Instant::now();
        }
    }
}

fn rand_simple() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64 % 1000
}
