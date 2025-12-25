use eframe::egui;
use egui_plot::{Plot, Line, PlotPoints};
use std::collections::VecDeque;
use std::time::Instant;

pub struct PollingPanel {
    is_running: bool,
    current_hz: u32,
    min_hz: u32,
    max_hz: u32,
    avg_hz: f64,
    samples: u32,
    history: VecDeque<f64>,
    timestamps: VecDeque<Instant>,
    last_update: Instant,
}

impl PollingPanel {
    pub fn new() -> Self {
        Self {
            is_running: false,
            current_hz: 0,
            min_hz: u32::MAX,
            max_hz: 0,
            avg_hz: 0.0,
            samples: 0,
            history: VecDeque::with_capacity(200),
            timestamps: VecDeque::with_capacity(1000),
            last_update: Instant::now(),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Polling Rate Monitor");
        ui.add_space(5.0);
        ui.label("Measures your mouse's polling rate in real-time.");
        ui.add_space(15.0);

        // Control buttons
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
        });

        ui.add_space(20.0);

        // Stats display
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(20.0)
            .rounding(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    self.stat_box(ui, "Current", &format!("{} Hz", self.current_hz), egui::Color32::WHITE);
                    ui.add_space(30.0);
                    self.stat_box(ui, "Min", &format!("{} Hz", if self.min_hz == u32::MAX { 0 } else { self.min_hz }), egui::Color32::LIGHT_BLUE);
                    ui.add_space(30.0);
                    self.stat_box(ui, "Max", &format!("{} Hz", self.max_hz), egui::Color32::LIGHT_GREEN);
                    ui.add_space(30.0);
                    self.stat_box(ui, "Avg", &format!("{:.1} Hz", self.avg_hz), egui::Color32::YELLOW);
                    ui.add_space(30.0);
                    self.stat_box(ui, "Samples", &format!("{}", self.samples), egui::Color32::GRAY);
                });
            });

        ui.add_space(20.0);

        // Graph
        ui.heading("Polling Rate History");
        let points: PlotPoints = self.history
            .iter()
            .enumerate()
            .map(|(i, &hz)| [i as f64, hz])
            .collect();

        let line = Line::new(points)
            .color(egui::Color32::from_rgb(100, 200, 255))
            .name("Polling Rate");

        Plot::new("polling_plot")
            .height(250.0)
            .show_axes(true)
            .show_grid(true)
            .allow_drag(false)
            .allow_zoom(false)
            .show(ui, |plot_ui| {
                plot_ui.line(line);
            });

        ui.add_space(20.0);

        // Instructions
        egui::Frame::none()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .rounding(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Instructions").strong());
                ui.label("1. Click 'Start' to begin monitoring");
                ui.label("2. Move your mouse around to generate events");
                ui.label("3. The graph shows real-time polling rate");
                ui.add_space(5.0);
                ui.label(egui::RichText::new("Common polling rates: 125Hz, 250Hz, 500Hz, 1000Hz, 4000Hz, 8000Hz").weak());
            });

        // Simulate data for demo (in real app, this would come from evdev)
        if self.is_running {
            self.simulate_update();
        }
    }

    fn stat_box(&self, ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32) {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(label).weak().size(12.0));
            ui.label(egui::RichText::new(value).color(color).size(24.0).strong());
        });
    }

    fn start(&mut self) {
        self.is_running = true;
        self.timestamps.clear();
        self.last_update = Instant::now();
    }

    fn reset(&mut self) {
        self.is_running = false;
        self.current_hz = 0;
        self.min_hz = u32::MAX;
        self.max_hz = 0;
        self.avg_hz = 0.0;
        self.samples = 0;
        self.history.clear();
        self.timestamps.clear();
    }

    fn simulate_update(&mut self) {
        // Simulate polling rate data for demo
        // In the real implementation, this would read from evdev
        if self.last_update.elapsed().as_millis() >= 100 {
            // Simulate ~1000Hz with some variation
            let hz = 950 + (rand_simple() % 100) as u32;

            self.current_hz = hz;
            self.min_hz = self.min_hz.min(hz);
            self.max_hz = self.max_hz.max(hz);
            self.samples += 1;
            self.avg_hz = (self.avg_hz * (self.samples - 1) as f64 + hz as f64) / self.samples as f64;

            self.history.push_back(hz as f64);
            if self.history.len() > 200 {
                self.history.pop_front();
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
