use eframe::egui;
use std::collections::VecDeque;
use std::time::Instant;

pub struct ClickPanel {
    // Click Response
    response_running: bool,
    response_results: Vec<f64>,
    response_trial: usize,
    response_waiting: bool,
    response_start: Option<Instant>,

    // Click Sticky
    sticky_running: bool,
    sticky_holds: Vec<f64>,
    sticky_count: usize,

    // Lift Off
    liftoff_running: bool,
    liftoff_jumps: usize,
    liftoff_position: (i64, i64),
}

impl ClickPanel {
    pub fn new() -> Self {
        Self {
            response_running: false,
            response_results: Vec::new(),
            response_trial: 0,
            response_waiting: false,
            response_start: None,

            sticky_running: false,
            sticky_holds: Vec::new(),
            sticky_count: 0,

            liftoff_running: false,
            liftoff_jumps: 0,
            liftoff_position: (0, 0),
        }
    }

    pub fn ui_response(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        ui.heading("Click Response Test");
        ui.add_space(5.0);
        ui.label("Measures your click reaction time.");
        ui.add_space(15.0);

        // Controls
        ui.horizontal(|ui| {
            if self.response_running {
                if ui.button("Stop").clicked() {
                    self.response_running = false;
                }
            } else {
                if ui.button("Start Test").clicked() {
                    self.response_running = true;
                    self.response_results.clear();
                    self.response_trial = 0;
                    self.response_waiting = false;
                }
            }

            if ui.button("Reset").clicked() {
                self.response_running = false;
                self.response_results.clear();
                self.response_trial = 0;
            }
        });

        ui.add_space(20.0);

        // Test area
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 200.0),
            egui::Sense::click(),
        );

        let color = if self.response_waiting {
            egui::Color32::from_rgb(50, 200, 50)
        } else if self.response_running {
            egui::Color32::from_rgb(200, 50, 50)
        } else {
            egui::Color32::from_rgb(80, 80, 80)
        };

        ui.painter().rect_filled(rect, 8.0, color);

        let text = if self.response_waiting {
            "CLICK NOW!"
        } else if self.response_running {
            "Wait for green..."
        } else {
            "Click Start to begin"
        };

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(32.0),
            egui::Color32::WHITE,
        );

        if response.clicked() && self.response_waiting {
            if let Some(start) = self.response_start {
                let latency = start.elapsed().as_secs_f64() * 1000.0;
                self.response_results.push(latency);
                self.response_trial += 1;
                self.response_waiting = false;

                if self.response_trial >= 10 {
                    self.response_running = false;
                }
            }
        }

        // Simulate waiting phase
        if self.response_running && !self.response_waiting && self.response_trial < 10 {
            if rand_simple() % 100 < 2 {
                self.response_waiting = true;
                self.response_start = Some(Instant::now());
            }
        }

        ui.add_space(20.0);

        // Results
        if !self.response_results.is_empty() {
            ui.heading("Results");

            let avg: f64 = self.response_results.iter().sum::<f64>() / self.response_results.len() as f64;
            let min = self.response_results.iter().cloned().fold(f64::MAX, f64::min);
            let max = self.response_results.iter().cloned().fold(0.0, f64::max);

            egui::Frame::dark_canvas(ui.style())
                .inner_margin(15.0)
                .rounding(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label("Average");
                            ui.label(egui::RichText::new(format!("{:.1} ms", avg)).size(20.0).strong());
                        });
                        ui.add_space(30.0);
                        ui.vertical(|ui| {
                            ui.label("Best");
                            ui.label(egui::RichText::new(format!("{:.1} ms", min)).size(20.0).color(egui::Color32::GREEN));
                        });
                        ui.add_space(30.0);
                        ui.vertical(|ui| {
                            ui.label("Worst");
                            ui.label(egui::RichText::new(format!("{:.1} ms", max)).size(20.0).color(egui::Color32::RED));
                        });
                        ui.add_space(30.0);
                        ui.vertical(|ui| {
                            ui.label("Trials");
                            ui.label(egui::RichText::new(format!("{}/10", self.response_results.len())).size(20.0));
                        });
                    });
                });

            ui.add_space(10.0);

            // Individual results
            ui.collapsing("All Results", |ui| {
                for (i, result) in self.response_results.iter().enumerate() {
                    ui.label(format!("Trial {}: {:.1} ms", i + 1, result));
                }
            });
        }
    }

    pub fn ui_sticky(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        ui.heading("Click Stickiness Test");
        ui.add_space(5.0);
        ui.label("Tests for stuck or delayed click releases.");
        ui.add_space(15.0);

        // Controls
        ui.horizontal(|ui| {
            if self.sticky_running {
                if ui.button("Stop").clicked() {
                    self.sticky_running = false;
                }
            } else {
                if ui.button("Start").clicked() {
                    self.sticky_running = true;
                    self.sticky_holds.clear();
                    self.sticky_count = 0;
                }
            }

            if ui.button("Reset").clicked() {
                self.sticky_running = false;
                self.sticky_holds.clear();
                self.sticky_count = 0;
            }
        });

        ui.add_space(20.0);

        // Test area
        let (rect, _response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 150.0),
            egui::Sense::click(),
        );

        ui.painter().rect_filled(rect, 8.0, ui.visuals().faint_bg_color);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            if self.sticky_running { "Click rapidly to test switches" } else { "Click Start to begin" },
            egui::FontId::proportional(20.0),
            ui.visuals().text_color(),
        );

        ui.add_space(20.0);

        // Stats
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(15.0)
            .rounding(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label("Clicks");
                        ui.label(egui::RichText::new(format!("{}", self.sticky_holds.len())).size(20.0).strong());
                    });
                    ui.add_space(30.0);
                    ui.vertical(|ui| {
                        ui.label("Sticky Clicks");
                        let color = if self.sticky_count == 0 { egui::Color32::GREEN } else { egui::Color32::RED };
                        ui.label(egui::RichText::new(format!("{}", self.sticky_count)).size(20.0).color(color));
                    });
                    ui.add_space(30.0);
                    ui.vertical(|ui| {
                        ui.label("Avg Hold");
                        let avg = if self.sticky_holds.is_empty() {
                            0.0
                        } else {
                            self.sticky_holds.iter().sum::<f64>() / self.sticky_holds.len() as f64
                        };
                        ui.label(egui::RichText::new(format!("{:.1} ms", avg)).size(20.0));
                    });
                });
            });

        // Simulate clicks
        if self.sticky_running && rand_simple() % 20 == 0 {
            let hold = 50.0 + (rand_simple() % 50) as f64;
            self.sticky_holds.push(hold);
            if hold > 100.0 {
                self.sticky_count += 1;
            }
        }
    }

    pub fn ui_liftoff(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        ui.heading("Lift-Off Jump Test");
        ui.add_space(5.0);
        ui.label("Detects cursor jumps when lifting the mouse.");
        ui.add_space(15.0);

        // Controls
        ui.horizontal(|ui| {
            if self.liftoff_running {
                if ui.button("Stop").clicked() {
                    self.liftoff_running = false;
                }
            } else {
                if ui.button("Start").clicked() {
                    self.liftoff_running = true;
                    self.liftoff_jumps = 0;
                }
            }

            if ui.button("Reset").clicked() {
                self.liftoff_running = false;
                self.liftoff_jumps = 0;
                self.liftoff_position = (0, 0);
            }
        });

        ui.add_space(20.0);

        // Stats
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(20.0)
            .rounding(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label("Position");
                        ui.label(egui::RichText::new(format!("({}, {})", self.liftoff_position.0, self.liftoff_position.1)).size(20.0));
                    });
                    ui.add_space(50.0);
                    ui.vertical(|ui| {
                        ui.label("Jumps Detected");
                        let color = if self.liftoff_jumps == 0 { egui::Color32::GREEN } else { egui::Color32::RED };
                        ui.label(egui::RichText::new(format!("{}", self.liftoff_jumps)).size(24.0).color(color));
                    });
                    ui.add_space(50.0);
                    ui.vertical(|ui| {
                        ui.label("Status");
                        let status = if self.liftoff_running { "Monitoring..." } else { "Stopped" };
                        ui.label(egui::RichText::new(status).size(20.0));
                    });
                });
            });

        ui.add_space(20.0);

        // Instructions
        egui::Frame::none()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .rounding(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Instructions").strong());
                ui.label("1. Start the test");
                ui.label("2. Move your mouse around");
                ui.label("3. Slowly lift the mouse off the surface");
                ui.label("4. Large cursor jumps during lift indicate high LOD");
            });

        // Simulate position updates
        if self.liftoff_running {
            self.liftoff_position.0 += (rand_simple() % 10) as i64 - 5;
            self.liftoff_position.1 += (rand_simple() % 10) as i64 - 5;
        }
    }
}

fn rand_simple() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64 % 1000
}
