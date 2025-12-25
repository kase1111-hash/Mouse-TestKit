use eframe::egui;
use std::time::Instant;

pub struct DoubleClickPanel {
    clicks: Vec<Instant>,
    intervals: Vec<f64>,
    avg_interval: f64,
    min_interval: f64,
    max_interval: f64,
    double_click_count: u32,
    accidental_double_clicks: u32,
    threshold_ms: f64,
}

impl DoubleClickPanel {
    pub fn new() -> Self {
        Self {
            clicks: Vec::new(),
            intervals: Vec::new(),
            avg_interval: 0.0,
            min_interval: f64::MAX,
            max_interval: 0.0,
            double_click_count: 0,
            accidental_double_clicks: 0,
            threshold_ms: 50.0,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        ui.heading("Double-Click Test");
        ui.add_space(5.0);
        ui.label("Tests click timing consistency and detects accidental double-clicks.");
        ui.add_space(15.0);

        // Threshold setting
        ui.horizontal(|ui| {
            ui.label("Accidental double-click threshold:");
            ui.add(egui::DragValue::new(&mut self.threshold_ms)
                .range(10.0..=100.0)
                .speed(1.0)
                .suffix(" ms"));
            ui.label(egui::RichText::new("(clicks faster than this are flagged)").weak());
        });

        ui.add_space(20.0);

        // Big click button
        let button_size = egui::vec2(300.0, 150.0);
        ui.vertical_centered(|ui| {
            let button = egui::Button::new(
                egui::RichText::new("CLICK HERE")
                    .size(32.0)
                    .color(egui::Color32::WHITE)
            )
            .fill(egui::Color32::from_rgb(60, 100, 180))
            .min_size(button_size)
            .rounding(12.0);

            if ui.add(button).clicked() {
                self.register_click();
            }
        });

        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            ui.label("Click the button repeatedly to test");
        });

        ui.add_space(20.0);

        // Stats
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(20.0)
            .rounding(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    self.stat_box(ui, "Total Clicks", &format!("{}", self.clicks.len()), egui::Color32::WHITE);
                    ui.add_space(30.0);
                    self.stat_box(ui, "Double-Clicks", &format!("{}", self.double_click_count), egui::Color32::LIGHT_GREEN);
                    ui.add_space(30.0);
                    self.stat_box(ui, "Accidental", &format!("{}", self.accidental_double_clicks),
                        if self.accidental_double_clicks > 0 { egui::Color32::RED } else { egui::Color32::GREEN });
                    ui.add_space(30.0);
                    self.stat_box(ui, "Avg Interval", &format!("{:.1} ms", self.avg_interval), egui::Color32::YELLOW);
                });
            });

        ui.add_space(20.0);

        // Interval details
        if !self.intervals.is_empty() {
            ui.heading("Click Intervals");

            egui::Frame::none()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(15.0)
                .rounding(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("Min: {:.1} ms", if self.min_interval == f64::MAX { 0.0 } else { self.min_interval }));
                        ui.add_space(20.0);
                        ui.label(format!("Max: {:.1} ms", self.max_interval));
                        ui.add_space(20.0);
                        ui.label(format!("Range: {:.1} ms", self.max_interval - if self.min_interval == f64::MAX { 0.0 } else { self.min_interval }));
                    });

                    ui.add_space(15.0);

                    // Recent intervals
                    ui.label(egui::RichText::new("Recent Intervals:").strong());
                    ui.horizontal_wrapped(|ui| {
                        for (i, interval) in self.intervals.iter().rev().take(20).enumerate() {
                            let color = if *interval < self.threshold_ms {
                                egui::Color32::RED
                            } else if *interval < 100.0 {
                                egui::Color32::YELLOW
                            } else {
                                egui::Color32::GREEN
                            };
                            ui.label(egui::RichText::new(format!("{:.0}", interval)).color(color));
                            if i < 19 {
                                ui.label("|");
                            }
                        }
                    });
                });
        }

        ui.add_space(20.0);

        // Analysis
        if self.clicks.len() >= 10 {
            ui.heading("Analysis");

            let consistency = self.calculate_consistency();
            let (rating, color, message) = if self.accidental_double_clicks > 0 {
                ("Warning", egui::Color32::RED,
                 format!("{} accidental double-clicks detected! This may indicate switch issues.", self.accidental_double_clicks))
            } else if consistency > 80.0 {
                ("Excellent", egui::Color32::GREEN, "Very consistent clicking rhythm".to_string())
            } else if consistency > 60.0 {
                ("Good", egui::Color32::LIGHT_GREEN, "Reasonably consistent clicking".to_string())
            } else {
                ("Variable", egui::Color32::YELLOW, "High variation in click timing".to_string())
            };

            egui::Frame::dark_canvas(ui.style())
                .inner_margin(15.0)
                .rounding(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Rating:");
                        ui.label(egui::RichText::new(rating).size(18.0).strong().color(color));
                    });
                    ui.add_space(5.0);
                    ui.label(message);
                    ui.add_space(5.0);
                    ui.label(format!("Consistency Score: {:.0}%", consistency));
                });
        }

        ui.add_space(20.0);

        // Clear button
        if ui.button("Clear Results").clicked() {
            self.clear();
        }
    }

    fn stat_box(&self, ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32) {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(label).weak().size(12.0));
            ui.label(egui::RichText::new(value).color(color).size(24.0).strong());
        });
    }

    fn register_click(&mut self) {
        let now = Instant::now();

        if let Some(last) = self.clicks.last() {
            let interval = last.elapsed().as_secs_f64() * 1000.0;
            self.intervals.push(interval);

            // Update stats
            self.min_interval = self.min_interval.min(interval);
            self.max_interval = self.max_interval.max(interval);
            self.avg_interval = self.intervals.iter().sum::<f64>() / self.intervals.len() as f64;

            // Check for double-click (between threshold and 500ms)
            if interval >= self.threshold_ms && interval <= 500.0 {
                self.double_click_count += 1;
            } else if interval < self.threshold_ms {
                self.accidental_double_clicks += 1;
            }
        }

        self.clicks.push(now);
    }

    fn calculate_consistency(&self) -> f64 {
        if self.intervals.len() < 2 {
            return 0.0;
        }

        let mean = self.avg_interval;
        let variance: f64 = self.intervals.iter()
            .map(|x| (x - mean).powi(2))
            .sum::<f64>() / self.intervals.len() as f64;
        let std_dev = variance.sqrt();

        // Convert to consistency score (lower std_dev = higher consistency)
        let coefficient_of_variation = std_dev / mean.max(1.0);
        (1.0 - coefficient_of_variation.min(1.0)) * 100.0
    }

    fn clear(&mut self) {
        self.clicks.clear();
        self.intervals.clear();
        self.avg_interval = 0.0;
        self.min_interval = f64::MAX;
        self.max_interval = 0.0;
        self.double_click_count = 0;
        self.accidental_double_clicks = 0;
    }
}
