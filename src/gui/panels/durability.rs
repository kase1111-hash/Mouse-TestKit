use eframe::egui;
use std::time::{Instant, Duration};

pub struct DurabilityPanel {
    is_running: bool,
    start_time: Option<Instant>,
    elapsed: Duration,
    click_counts: ClickCounts,
    target_clicks: u32,
    clicks_per_second: f64,
    last_click_time: Option<Instant>,
    session_history: Vec<SessionResult>,
}

#[derive(Default, Clone)]
struct ClickCounts {
    left: u32,
    right: u32,
    middle: u32,
    side1: u32,
    side2: u32,
}

struct SessionResult {
    duration: Duration,
    total_clicks: u32,
    cps: f64,
}

impl DurabilityPanel {
    pub fn new() -> Self {
        Self {
            is_running: false,
            start_time: None,
            elapsed: Duration::ZERO,
            click_counts: ClickCounts::default(),
            target_clicks: 1000,
            clicks_per_second: 0.0,
            last_click_time: None,
            session_history: Vec::new(),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        ui.heading("Button Durability Test");
        ui.add_space(5.0);
        ui.label("Tests button responsiveness and counts total clicks for durability testing.");
        ui.add_space(15.0);

        // Target setting
        ui.horizontal(|ui| {
            ui.label("Target clicks:");
            ui.add(egui::DragValue::new(&mut self.target_clicks)
                .range(100..=100000)
                .speed(100));
        });

        ui.add_space(15.0);

        // Controls
        ui.horizontal(|ui| {
            if self.is_running {
                if ui.button("Stop Session").clicked() {
                    self.stop_session();
                }
            } else {
                if ui.button("Start Session").clicked() {
                    self.start_session();
                }
            }

            if ui.button("Reset Counts").clicked() {
                self.reset_counts();
            }
        });

        ui.add_space(20.0);

        // Timer and progress
        if self.is_running {
            if let Some(start) = self.start_time {
                self.elapsed = start.elapsed();
            }
        }

        let total_clicks = self.click_counts.total();
        let progress = (total_clicks as f32 / self.target_clicks as f32).min(1.0);

        egui::Frame::dark_canvas(ui.style())
            .inner_margin(20.0)
            .rounding(8.0)
            .show(ui, |ui| {
                // Progress bar
                ui.horizontal(|ui| {
                    ui.label("Progress:");
                    let progress_bar = egui::ProgressBar::new(progress)
                        .text(format!("{} / {} clicks", total_clicks, self.target_clicks))
                        .animate(self.is_running);
                    ui.add_sized([300.0, 20.0], progress_bar);
                    ui.label(format!("{:.1}%", progress * 100.0));
                });

                ui.add_space(15.0);

                // Stats
                ui.horizontal(|ui| {
                    self.stat_box(ui, "Time", &format_duration(self.elapsed), egui::Color32::WHITE);
                    ui.add_space(30.0);
                    self.stat_box(ui, "Total", &format!("{}", total_clicks), egui::Color32::LIGHT_GREEN);
                    ui.add_space(30.0);
                    self.stat_box(ui, "CPS", &format!("{:.1}", self.clicks_per_second), egui::Color32::YELLOW);
                    ui.add_space(30.0);
                    self.stat_box(ui, "Remaining", &format!("{}", self.target_clicks.saturating_sub(total_clicks)), egui::Color32::LIGHT_BLUE);
                });
            });

        ui.add_space(20.0);

        // Click buttons
        ui.heading("Click Buttons");
        ui.label("Click each button to register clicks:");
        ui.add_space(10.0);

        let button_size = egui::vec2(120.0, 60.0);

        ui.horizontal(|ui| {
            if ui.add_sized(button_size, egui::Button::new(
                egui::RichText::new(format!("LEFT\n{}", self.click_counts.left))
                    .size(16.0)
            ).fill(egui::Color32::from_rgb(60, 100, 180))).clicked() {
                self.register_click(Button::Left);
            }

            if ui.add_sized(button_size, egui::Button::new(
                egui::RichText::new(format!("MIDDLE\n{}", self.click_counts.middle))
                    .size(16.0)
            ).fill(egui::Color32::from_rgb(100, 60, 180))).clicked() {
                self.register_click(Button::Middle);
            }

            if ui.add_sized(button_size, egui::Button::new(
                egui::RichText::new(format!("RIGHT\n{}", self.click_counts.right))
                    .size(16.0)
            ).fill(egui::Color32::from_rgb(180, 60, 100))).clicked() {
                self.register_click(Button::Right);
            }
        });

        ui.add_space(10.0);

        ui.horizontal(|ui| {
            if ui.add_sized(button_size, egui::Button::new(
                egui::RichText::new(format!("SIDE 1\n{}", self.click_counts.side1))
                    .size(16.0)
            ).fill(egui::Color32::from_rgb(60, 180, 100))).clicked() {
                self.register_click(Button::Side1);
            }

            if ui.add_sized(button_size, egui::Button::new(
                egui::RichText::new(format!("SIDE 2\n{}", self.click_counts.side2))
                    .size(16.0)
            ).fill(egui::Color32::from_rgb(180, 140, 60))).clicked() {
                self.register_click(Button::Side2);
            }
        });

        ui.add_space(20.0);

        // Breakdown
        ui.heading("Button Breakdown");
        egui::Frame::none()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .rounding(8.0)
            .show(ui, |ui| {
                egui::Grid::new("button_breakdown")
                    .num_columns(3)
                    .spacing([20.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Button").strong());
                        ui.label(egui::RichText::new("Clicks").strong());
                        ui.label(egui::RichText::new("Percentage").strong());
                        ui.end_row();

                        let total = total_clicks.max(1) as f64;

                        self.breakdown_row(ui, "Left", self.click_counts.left, total);
                        self.breakdown_row(ui, "Right", self.click_counts.right, total);
                        self.breakdown_row(ui, "Middle", self.click_counts.middle, total);
                        self.breakdown_row(ui, "Side 1", self.click_counts.side1, total);
                        self.breakdown_row(ui, "Side 2", self.click_counts.side2, total);
                    });
            });

        // Session history
        if !self.session_history.is_empty() {
            ui.add_space(20.0);
            ui.heading("Session History");

            egui::Frame::none()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(15.0)
                .rounding(8.0)
                .show(ui, |ui| {
                    for (i, session) in self.session_history.iter().enumerate().rev().take(5) {
                        ui.label(format!(
                            "Session {}: {} clicks in {} ({:.1} CPS)",
                            i + 1,
                            session.total_clicks,
                            format_duration(session.duration),
                            session.cps
                        ));
                    }
                });
        }

        // Target reached notification
        if total_clicks >= self.target_clicks && self.is_running {
            self.stop_session();
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Target reached!").size(20.0).color(egui::Color32::GREEN));
        }
    }

    fn stat_box(&self, ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32) {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(label).weak().size(12.0));
            ui.label(egui::RichText::new(value).color(color).size(24.0).strong());
        });
    }

    fn breakdown_row(&self, ui: &mut egui::Ui, name: &str, count: u32, total: f64) {
        ui.label(name);
        ui.label(format!("{}", count));
        ui.label(format!("{:.1}%", count as f64 / total * 100.0));
        ui.end_row();
    }

    fn start_session(&mut self) {
        self.is_running = true;
        self.start_time = Some(Instant::now());
        self.click_counts = ClickCounts::default();
        self.clicks_per_second = 0.0;
    }

    fn stop_session(&mut self) {
        self.is_running = false;

        let total = self.click_counts.total();
        let cps = if self.elapsed.as_secs_f64() > 0.0 {
            total as f64 / self.elapsed.as_secs_f64()
        } else {
            0.0
        };

        if total > 0 {
            self.session_history.push(SessionResult {
                duration: self.elapsed,
                total_clicks: total,
                cps,
            });
        }
    }

    fn reset_counts(&mut self) {
        self.click_counts = ClickCounts::default();
        self.elapsed = Duration::ZERO;
        self.start_time = None;
        self.is_running = false;
        self.clicks_per_second = 0.0;
    }

    fn register_click(&mut self, button: Button) {
        if !self.is_running {
            self.start_session();
        }

        match button {
            Button::Left => self.click_counts.left += 1,
            Button::Right => self.click_counts.right += 1,
            Button::Middle => self.click_counts.middle += 1,
            Button::Side1 => self.click_counts.side1 += 1,
            Button::Side2 => self.click_counts.side2 += 1,
        }

        // Update CPS
        let now = Instant::now();
        if let Some(last) = self.last_click_time {
            let interval = now.duration_since(last).as_secs_f64();
            if interval > 0.0 && interval < 2.0 {
                self.clicks_per_second = 1.0 / interval;
            }
        }
        self.last_click_time = Some(now);
    }
}

impl ClickCounts {
    fn total(&self) -> u32 {
        self.left + self.right + self.middle + self.side1 + self.side2
    }
}

enum Button {
    Left,
    Right,
    Middle,
    Side1,
    Side2,
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let mins = secs / 60;
    let hours = mins / 60;

    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, mins % 60, secs % 60)
    } else {
        format!("{}:{:02}", mins, secs % 60)
    }
}
