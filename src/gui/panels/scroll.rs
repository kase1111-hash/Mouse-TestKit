use eframe::egui;
use std::collections::VecDeque;
use std::time::Instant;

use crate::export::ScrollExport;
use crate::input_bridge::{RawInputEvent, RawInputKind};

pub struct ScrollPanel {
    is_running: bool,
    /// Scroll events with their delta values
    scroll_events: VecDeque<ScrollEvent>,
    /// Total scroll up distance
    total_up: f32,
    /// Total scroll down distance
    total_down: f32,
    /// Total scroll steps (notches)
    step_count: usize,
    /// Direction changes detected
    direction_changes: usize,
    /// Last scroll direction (true = up, false = down)
    last_direction: Option<bool>,
    /// Scroll speed samples for averaging
    speed_samples: VecDeque<f64>,
    /// Last scroll time for speed calculation
    last_scroll_time: Option<Instant>,
    /// Current scroll speed (steps per second)
    current_speed: f64,
}

struct ScrollEvent {
    timestamp: Instant,
    direction_up: bool,
}

impl ScrollPanel {
    pub fn new() -> Self {
        Self {
            is_running: false,
            scroll_events: VecDeque::with_capacity(500),
            total_up: 0.0,
            total_down: 0.0,
            step_count: 0,
            direction_changes: 0,
            last_direction: None,
            speed_samples: VecDeque::with_capacity(100),
            last_scroll_time: None,
            current_speed: 0.0,
        }
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
        ui.heading("Scroll Wheel Test");
        ui.add_space(5.0);
        ui.label("Tests scroll wheel functionality, consistency, and detects missed steps.");
        ui.add_space(15.0);

        // Controls
        ui.horizontal(|ui| {
            if self.is_running {
                if ui.button("Stop").clicked() {
                    self.is_running = false;
                }
            } else if ui.button("Start").clicked() {
                self.is_running = true;
                self.reset();
            }

            if ui.button("Reset").clicked() {
                self.is_running = false;
                self.reset();
            }
        });

        ui.add_space(20.0);

        // Scroll test area
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 200.0),
            egui::Sense::hover(),
        );

        // Visual scroll indicator
        let scroll_indicator = if let Some(event) = self.scroll_events.back() {
            let age = event.timestamp.elapsed().as_secs_f32();
            if age < 0.3 {
                let alpha = ((1.0 - age / 0.3) * 255.0) as u8;
                if event.direction_up {
                    Some((
                        egui::Color32::from_rgba_unmultiplied(50, 200, 100, alpha),
                        "UP",
                    ))
                } else {
                    Some((
                        egui::Color32::from_rgba_unmultiplied(200, 100, 50, alpha),
                        "DOWN",
                    ))
                }
            } else {
                None
            }
        } else {
            None
        };

        let base_color = if self.is_running {
            egui::Color32::from_rgb(50, 55, 70)
        } else {
            egui::Color32::from_rgb(40, 40, 50)
        };

        ui.painter().rect_filled(rect, 8.0, base_color);

        // Draw scroll direction indicator
        if let Some((color, direction)) = scroll_indicator {
            let arrow_rect = if direction == "UP" {
                egui::Rect::from_center_size(
                    egui::pos2(rect.center().x, rect.center().y - 30.0),
                    egui::vec2(60.0, 40.0),
                )
            } else {
                egui::Rect::from_center_size(
                    egui::pos2(rect.center().x, rect.center().y + 30.0),
                    egui::vec2(60.0, 40.0),
                )
            };

            ui.painter().rect_filled(arrow_rect, 4.0, color);
            ui.painter().text(
                arrow_rect.center(),
                egui::Align2::CENTER_CENTER,
                direction,
                egui::FontId::proportional(16.0),
                egui::Color32::WHITE,
            );
        }

        let text = if !self.is_running {
            "Click Start and scroll here to test"
        } else {
            "Scroll your mouse wheel here"
        };

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(20.0),
            egui::Color32::from_rgb(180, 180, 190),
        );

        ui.add_space(20.0);

        // Stats display
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(20.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label("Total Steps");
                        ui.label(
                            egui::RichText::new(format!("{}", self.step_count))
                                .size(24.0)
                                .strong(),
                        );
                    });
                    ui.add_space(30.0);
                    ui.vertical(|ui| {
                        ui.label("Scroll Up");
                        ui.label(
                            egui::RichText::new(format!("{:.0}", self.total_up.abs()))
                                .size(24.0)
                                .color(egui::Color32::LIGHT_GREEN),
                        );
                    });
                    ui.add_space(30.0);
                    ui.vertical(|ui| {
                        ui.label("Scroll Down");
                        ui.label(
                            egui::RichText::new(format!("{:.0}", self.total_down.abs()))
                                .size(24.0)
                                .color(egui::Color32::from_rgb(255, 150, 100)),
                        );
                    });
                    ui.add_space(30.0);
                    ui.vertical(|ui| {
                        ui.label("Speed (sps)");
                        ui.label(
                            egui::RichText::new(format!("{:.1}", self.current_speed))
                                .size(24.0)
                                .color(egui::Color32::YELLOW),
                        );
                    });
                });
            });

        ui.add_space(15.0);

        // Secondary stats
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(15.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label("Direction Changes");
                        let color = if self.direction_changes > 20 {
                            egui::Color32::YELLOW
                        } else {
                            egui::Color32::WHITE
                        };
                        ui.label(
                            egui::RichText::new(format!("{}", self.direction_changes))
                                .size(18.0)
                                .color(color),
                        );
                    });
                    ui.add_space(30.0);
                    ui.vertical(|ui| {
                        ui.label("Avg Step Size");
                        let avg = if self.step_count > 0 {
                            (self.total_up.abs() + self.total_down.abs()) / self.step_count as f32
                        } else {
                            0.0
                        };
                        ui.label(egui::RichText::new(format!("{:.1}", avg)).size(18.0));
                    });
                    ui.add_space(30.0);
                    ui.vertical(|ui| {
                        ui.label("Balance");
                        let total = self.total_up.abs() + self.total_down.abs();
                        let balance = if total > 0.0 {
                            (self.total_up.abs() / total) * 100.0
                        } else {
                            50.0
                        };
                        ui.label(egui::RichText::new(format!("{:.0}% up", balance)).size(18.0));
                    });
                });
            });

        ui.add_space(20.0);

        // Analysis (after enough samples)
        if self.step_count >= 20 {
            ui.heading("Analysis");

            let avg_speed = if self.speed_samples.is_empty() {
                0.0
            } else {
                self.speed_samples.iter().sum::<f64>() / self.speed_samples.len() as f64
            };

            let speed_variance = if self.speed_samples.len() >= 2 {
                let mean = avg_speed;
                self.speed_samples
                    .iter()
                    .map(|x| (x - mean).powi(2))
                    .sum::<f64>()
                    / self.speed_samples.len() as f64
            } else {
                0.0
            };
            let speed_std = speed_variance.sqrt();

            let consistency = if avg_speed > 0.0 {
                ((1.0 - (speed_std / avg_speed).min(1.0)) * 100.0) as u32
            } else {
                0
            };

            let (rating, color, message) = if consistency >= 80 {
                (
                    "Excellent",
                    egui::Color32::GREEN,
                    "Scroll wheel is very consistent",
                )
            } else if consistency >= 60 {
                (
                    "Good",
                    egui::Color32::LIGHT_GREEN,
                    "Scroll wheel is working well",
                )
            } else if consistency >= 40 {
                (
                    "Fair",
                    egui::Color32::YELLOW,
                    "Some scroll inconsistency detected",
                )
            } else {
                ("Poor", egui::Color32::RED, "Scroll wheel may have issues")
            };

            egui::Frame::dark_canvas(ui.style())
                .inner_margin(15.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Rating:");
                        ui.label(egui::RichText::new(rating).size(18.0).strong().color(color));
                    });
                    ui.add_space(5.0);
                    ui.label(message);
                    ui.add_space(5.0);
                    ui.label(format!("Consistency Score: {}%", consistency));
                    ui.label(format!("Average Speed: {:.1} steps/sec", avg_speed));
                });
        }

        ui.add_space(20.0);

        // Instructions
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Instructions").strong());
                ui.label("1. Click Start to begin testing");
                ui.label("2. Hover over the test area and scroll your mouse wheel");
                ui.label("3. Test both directions for a complete analysis");
                ui.label("4. Scroll at different speeds to test consistency");
                ui.add_space(5.0);
                ui.label(egui::RichText::new("sps = steps per second").weak());
            });

        // Capture scroll input
        if self.is_running {
            if has_bridge {
                // Use raw input events from the input bridge (no hover check needed)
                for event in raw_events {
                    if let RawInputKind::Scroll { delta } = &event.kind {
                        let now = event.timestamp;
                        let direction_up = *delta > 0;
                        let abs_delta = (*delta).unsigned_abs() as f32;

                        // Record event
                        self.scroll_events.push_back(ScrollEvent {
                            timestamp: now,
                            direction_up,
                        });

                        // Keep only last 500 events
                        while self.scroll_events.len() > 500 {
                            self.scroll_events.pop_front();
                        }

                        // Update totals
                        if direction_up {
                            self.total_up += abs_delta;
                        } else {
                            self.total_down += abs_delta;
                        }

                        self.step_count += 1;

                        // Check for direction change
                        if let Some(last_dir) = self.last_direction {
                            if last_dir != direction_up {
                                self.direction_changes += 1;
                            }
                        }
                        self.last_direction = Some(direction_up);

                        // Calculate scroll speed
                        if let Some(last_time) = self.last_scroll_time {
                            let delta_secs = now.duration_since(last_time).as_secs_f64();
                            if delta_secs > 0.0 && delta_secs < 1.0 {
                                let speed = 1.0 / delta_secs;
                                self.speed_samples.push_back(speed);
                                if self.speed_samples.len() > 100 {
                                    self.speed_samples.pop_front();
                                }
                            }
                        }
                        self.last_scroll_time = Some(now);

                        // Update current speed (rolling average of last 10 samples)
                        let recent: Vec<_> = self.speed_samples.iter().rev().take(10).collect();
                        if !recent.is_empty() {
                            self.current_speed =
                                recent.iter().copied().sum::<f64>() / recent.len() as f64;
                        }
                    }
                }
            } else {
                // Fallback: use egui scroll input (requires hover over test area)
                let scroll_delta = ctx.input(|i| {
                    i.raw
                        .events
                        .iter()
                        .fold(egui::Vec2::ZERO, |acc, e| match e {
                            egui::Event::MouseWheel { delta, .. } => acc + *delta,
                            _ => acc,
                        })
                });
                let in_test_area = response.hovered();

                if in_test_area && scroll_delta.y != 0.0 {
                    let now = Instant::now();
                    let direction_up = scroll_delta.y > 0.0;

                    // Record event
                    self.scroll_events.push_back(ScrollEvent {
                        timestamp: now,
                        direction_up,
                    });

                    // Keep only last 500 events
                    while self.scroll_events.len() > 500 {
                        self.scroll_events.pop_front();
                    }

                    // Update totals
                    if direction_up {
                        self.total_up += scroll_delta.y.abs();
                    } else {
                        self.total_down += scroll_delta.y.abs();
                    }

                    self.step_count += 1;

                    // Check for direction change
                    if let Some(last_dir) = self.last_direction {
                        if last_dir != direction_up {
                            self.direction_changes += 1;
                        }
                    }
                    self.last_direction = Some(direction_up);

                    // Calculate scroll speed
                    if let Some(last_time) = self.last_scroll_time {
                        let delta_secs = now.duration_since(last_time).as_secs_f64();
                        if delta_secs > 0.0 && delta_secs < 1.0 {
                            let speed = 1.0 / delta_secs;
                            self.speed_samples.push_back(speed);
                            if self.speed_samples.len() > 100 {
                                self.speed_samples.pop_front();
                            }
                        }
                    }
                    self.last_scroll_time = Some(now);

                    // Update current speed (rolling average of last 10 samples)
                    let recent: Vec<_> = self.speed_samples.iter().rev().take(10).collect();
                    if !recent.is_empty() {
                        self.current_speed =
                            recent.iter().copied().sum::<f64>() / recent.len() as f64;
                    }
                }
            }
        }
    }

    fn reset(&mut self) {
        self.scroll_events.clear();
        self.total_up = 0.0;
        self.total_down = 0.0;
        self.step_count = 0;
        self.direction_changes = 0;
        self.last_direction = None;
        self.speed_samples.clear();
        self.last_scroll_time = None;
        self.current_speed = 0.0;
    }

    pub fn export(&self) -> Option<ScrollExport> {
        if self.step_count == 0 {
            return None;
        }
        let avg_speed = if self.speed_samples.is_empty() {
            0.0
        } else {
            self.speed_samples.iter().sum::<f64>() / self.speed_samples.len() as f64
        };

        let speed_variance = if self.speed_samples.len() >= 2 {
            let mean = avg_speed;
            self.speed_samples
                .iter()
                .map(|x| (x - mean).powi(2))
                .sum::<f64>()
                / self.speed_samples.len() as f64
        } else {
            0.0
        };
        let speed_std = speed_variance.sqrt();

        let consistency = if avg_speed > 0.0 {
            ((1.0 - (speed_std / avg_speed).min(1.0)) * 100.0) as u32
        } else {
            0
        };

        Some(ScrollExport {
            total_steps: self.step_count,
            scroll_up: self.total_up,
            scroll_down: self.total_down,
            direction_changes: self.direction_changes,
            avg_speed_sps: avg_speed,
            consistency_percent: consistency,
        })
    }
}
