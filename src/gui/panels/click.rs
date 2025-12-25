use eframe::egui;
use std::time::Instant;

/// Jump threshold in pixels - movements larger than this after idle are considered jumps
const JUMP_THRESHOLD_PX: f64 = 15.0;
/// Time in milliseconds without movement to consider mouse "idle" (potential lift)
const IDLE_THRESHOLD_MS: u64 = 80;

pub struct ClickPanel {
    // Click Response - real click testing
    response_running: bool,
    /// Total clicks recorded
    response_click_count: usize,
    /// Hold durations in ms
    response_hold_times: Vec<f64>,
    /// When the current click started (button pressed)
    response_press_start: Option<Instant>,
    /// Is button currently held down
    response_is_pressed: bool,
    /// Timestamps of recent clicks for CPS calculation
    response_click_times: std::collections::VecDeque<Instant>,
    /// Current CPS
    response_cps: f64,

    // Click Sticky
    sticky_running: bool,
    sticky_holds: Vec<f64>,
    sticky_count: usize,
    /// When click started for hold measurement
    sticky_press_start: Option<Instant>,
    /// Is button currently held
    sticky_is_pressed: bool,

    // Lift Off
    liftoff_running: bool,
    liftoff_jumps: usize,
    liftoff_position: (i64, i64),
    /// Time of last mouse movement
    liftoff_last_move: Instant,
    /// Whether mouse is currently considered idle (lifted)
    liftoff_is_idle: bool,
    /// Jump events with their distances
    liftoff_jump_events: Vec<f64>,
}

impl ClickPanel {
    pub fn new() -> Self {
        Self {
            response_running: false,
            response_click_count: 0,
            response_hold_times: Vec::new(),
            response_press_start: None,
            response_is_pressed: false,
            response_click_times: std::collections::VecDeque::with_capacity(100),
            response_cps: 0.0,

            sticky_running: false,
            sticky_holds: Vec::new(),
            sticky_count: 0,
            sticky_press_start: None,
            sticky_is_pressed: false,

            liftoff_running: false,
            liftoff_jumps: 0,
            liftoff_position: (0, 0),
            liftoff_last_move: Instant::now(),
            liftoff_is_idle: false,
            liftoff_jump_events: Vec::new(),
        }
    }

    pub fn ui_response(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Click Test");
        ui.add_space(5.0);
        ui.label("Tests mouse button response - measures click registration and hold duration.");
        ui.add_space(15.0);

        // Controls
        ui.horizontal(|ui| {
            if self.response_running {
                if ui.button("Stop").clicked() {
                    self.response_running = false;
                }
            } else {
                if ui.button("Start").clicked() {
                    self.response_running = true;
                    self.response_click_count = 0;
                    self.response_hold_times.clear();
                    self.response_click_times.clear();
                    self.response_cps = 0.0;
                    self.response_press_start = None;
                    self.response_is_pressed = false;
                }
            }

            if ui.button("Reset").clicked() {
                self.response_running = false;
                self.response_click_count = 0;
                self.response_hold_times.clear();
                self.response_click_times.clear();
                self.response_cps = 0.0;
                self.response_press_start = None;
                self.response_is_pressed = false;
            }
        });

        ui.add_space(20.0);

        // Click area - changes color when pressed
        let (rect, _response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 150.0),
            egui::Sense::hover(),
        );

        let color = if self.response_is_pressed {
            egui::Color32::from_rgb(50, 200, 50) // Green when pressed
        } else if self.response_running {
            egui::Color32::from_rgb(60, 60, 80) // Dark when waiting
        } else {
            egui::Color32::from_rgb(50, 50, 50)
        };

        ui.painter().rect_filled(rect, 8.0, color);

        let text = if !self.response_running {
            "Click Start, then click here to test"
        } else if self.response_is_pressed {
            "PRESSED"
        } else {
            "Click here!"
        };

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(28.0),
            egui::Color32::WHITE,
        );

        ui.add_space(20.0);

        // Stats display
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(20.0)
            .rounding(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label("Clicks");
                        ui.label(egui::RichText::new(format!("{}", self.response_click_count)).size(24.0).strong());
                    });
                    ui.add_space(40.0);
                    ui.vertical(|ui| {
                        ui.label("CPS");
                        ui.label(egui::RichText::new(format!("{:.1}", self.response_cps)).size(24.0).color(egui::Color32::YELLOW));
                    });
                    ui.add_space(40.0);
                    ui.vertical(|ui| {
                        ui.label("Avg Hold");
                        let avg_hold = if self.response_hold_times.is_empty() {
                            0.0
                        } else {
                            self.response_hold_times.iter().sum::<f64>() / self.response_hold_times.len() as f64
                        };
                        ui.label(egui::RichText::new(format!("{:.1} ms", avg_hold)).size(24.0));
                    });
                    ui.add_space(40.0);
                    ui.vertical(|ui| {
                        ui.label("Min Hold");
                        let min_hold = self.response_hold_times.iter().cloned().fold(f64::MAX, f64::min);
                        let min_str = if min_hold == f64::MAX { "- ms".to_string() } else { format!("{:.1} ms", min_hold) };
                        ui.label(egui::RichText::new(min_str).size(24.0).color(egui::Color32::LIGHT_GREEN));
                    });
                    ui.add_space(40.0);
                    ui.vertical(|ui| {
                        ui.label("Max Hold");
                        let max_hold = self.response_hold_times.iter().cloned().fold(0.0, f64::max);
                        ui.label(egui::RichText::new(format!("{:.1} ms", max_hold)).size(24.0).color(egui::Color32::LIGHT_RED));
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
                ui.label("1. Click Start to begin testing");
                ui.label("2. Click in the box above - it turns green when pressed");
                ui.label("3. Hold duration = time between press and release");
                ui.label("4. CPS = clicks per second (measured over last second)");
            });

        // Capture real mouse button input
        if self.response_running {
            // Request continuous repaints while running
            ctx.request_repaint();

            let now = Instant::now();
            let pointer = ctx.input(|i| i.pointer.clone());
            let is_primary_down = pointer.primary_down();

            // Only track clicks that start within the test area
            let in_test_area = pointer.hover_pos().map(|pos| rect.contains(pos)).unwrap_or(false);

            // Detect press (transition from not pressed to pressed) - only in test area
            if in_test_area && is_primary_down && !self.response_is_pressed {
                self.response_is_pressed = true;
                self.response_press_start = Some(now);
            }

            // Detect release (transition from pressed to not pressed) - can happen anywhere
            if !is_primary_down && self.response_is_pressed {
                self.response_is_pressed = false;
                self.response_click_count += 1;
                self.response_click_times.push_back(now);

                // Calculate hold duration
                if let Some(start) = self.response_press_start {
                    let hold_ms = now.duration_since(start).as_secs_f64() * 1000.0;
                    self.response_hold_times.push(hold_ms);
                    // Keep only last 100 hold times
                    if self.response_hold_times.len() > 100 {
                        self.response_hold_times.remove(0);
                    }
                }
                self.response_press_start = None;

                // Keep only clicks from last second for CPS
                while let Some(front) = self.response_click_times.front() {
                    if now.duration_since(*front).as_secs_f64() > 1.0 {
                        self.response_click_times.pop_front();
                    } else {
                        break;
                    }
                }
            }

            // Update CPS (clicks in the last second)
            let one_sec_ago = now - std::time::Duration::from_secs(1);
            self.response_cps = self.response_click_times.iter()
                .filter(|t| **t > one_sec_ago)
                .count() as f64;
        }
    }

    pub fn ui_sticky(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Click Stickiness Test");
        ui.add_space(5.0);
        ui.label("Tests for stuck or delayed click releases. Long holds (>100ms) may indicate sticky switches.");
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
                    self.sticky_press_start = None;
                    self.sticky_is_pressed = false;
                }
            }

            if ui.button("Reset").clicked() {
                self.sticky_running = false;
                self.sticky_holds.clear();
                self.sticky_count = 0;
                self.sticky_press_start = None;
                self.sticky_is_pressed = false;
            }
        });

        ui.add_space(20.0);

        // Test area
        let (rect, _response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 150.0),
            egui::Sense::hover(),
        );

        let color = if self.sticky_is_pressed {
            egui::Color32::from_rgb(50, 200, 50)
        } else if self.sticky_running {
            egui::Color32::from_rgb(60, 60, 80)
        } else {
            ui.visuals().faint_bg_color
        };

        ui.painter().rect_filled(rect, 8.0, color);

        let text = if !self.sticky_running {
            "Click Start to begin"
        } else if self.sticky_is_pressed {
            "HELD"
        } else {
            "Click rapidly to test switches"
        };

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(20.0),
            egui::Color32::WHITE,
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
                        ui.label("Sticky (>100ms)");
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
                    ui.add_space(30.0);
                    ui.vertical(|ui| {
                        ui.label("Max Hold");
                        let max = self.sticky_holds.iter().cloned().fold(0.0, f64::max);
                        let color = if max > 100.0 { egui::Color32::RED } else { egui::Color32::GREEN };
                        ui.label(egui::RichText::new(format!("{:.1} ms", max)).size(20.0).color(color));
                    });
                });
            });

        // Capture real mouse button input
        if self.sticky_running {
            // Request continuous repaints while running
            ctx.request_repaint();

            let now = Instant::now();
            let pointer = ctx.input(|i| i.pointer.clone());
            let is_primary_down = pointer.primary_down();

            // Only track clicks that start within the test area
            let in_test_area = pointer.hover_pos().map(|pos| rect.contains(pos)).unwrap_or(false);

            // Detect press - only when pointer is in test area
            if in_test_area && is_primary_down && !self.sticky_is_pressed {
                self.sticky_is_pressed = true;
                self.sticky_press_start = Some(now);
            }

            // Detect release - can happen anywhere as long as we were tracking a press
            if !is_primary_down && self.sticky_is_pressed {
                self.sticky_is_pressed = false;

                if let Some(start) = self.sticky_press_start {
                    let hold_ms = now.duration_since(start).as_secs_f64() * 1000.0;
                    self.sticky_holds.push(hold_ms);

                    // Count as sticky if held > 100ms
                    if hold_ms > 100.0 {
                        self.sticky_count += 1;
                    }
                }
                self.sticky_press_start = None;
            }
        }
    }

    pub fn ui_liftoff(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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
                    self.liftoff_position = (0, 0);
                    self.liftoff_jump_events.clear();
                    self.liftoff_is_idle = false;
                    self.liftoff_last_move = Instant::now();
                }
            }

            if ui.button("Reset").clicked() {
                self.liftoff_running = false;
                self.liftoff_jumps = 0;
                self.liftoff_position = (0, 0);
                self.liftoff_jump_events.clear();
                self.liftoff_is_idle = false;
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
                        let status = if !self.liftoff_running {
                            "Stopped"
                        } else if self.liftoff_is_idle {
                            "IDLE (mouse lifted?)"
                        } else {
                            "Moving..."
                        };
                        let color = if self.liftoff_is_idle { egui::Color32::YELLOW } else { egui::Color32::WHITE };
                        ui.label(egui::RichText::new(status).size(20.0).color(color));
                    });
                });
            });

        ui.add_space(20.0);

        // Jump history
        if !self.liftoff_jump_events.is_empty() {
            egui::Frame::dark_canvas(ui.style())
                .inner_margin(15.0)
                .rounding(8.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Jump History").strong());
                    let avg: f64 = self.liftoff_jump_events.iter().sum::<f64>() / self.liftoff_jump_events.len() as f64;
                    let max = self.liftoff_jump_events.iter().cloned().fold(0.0, f64::max);
                    ui.horizontal(|ui| {
                        ui.label(format!("Average: {:.1} px", avg));
                        ui.add_space(20.0);
                        ui.label(format!("Max: {:.1} px", max));
                    });
                    ui.collapsing("All jumps", |ui| {
                        for (i, dist) in self.liftoff_jump_events.iter().enumerate() {
                            ui.label(format!("#{}: {:.1} px", i + 1, dist));
                        }
                    });
                });

            ui.add_space(20.0);
        }

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
                ui.add_space(5.0);
                ui.label(egui::RichText::new(format!("Jump threshold: {} px | Idle threshold: {} ms", JUMP_THRESHOLD_PX, IDLE_THRESHOLD_MS)).weak());
            });

        // Capture real mouse input and detect jumps
        if self.liftoff_running {
            // Request continuous repaints for idle detection timing
            ctx.request_repaint();

            let delta = ctx.input(|i| i.pointer.delta());
            let now = Instant::now();

            if delta.x != 0.0 || delta.y != 0.0 {
                // Mouse is moving
                let distance = ((delta.x as f64).powi(2) + (delta.y as f64).powi(2)).sqrt();

                // Check for jump: large movement after being idle
                if self.liftoff_is_idle && distance > JUMP_THRESHOLD_PX {
                    self.liftoff_jumps += 1;
                    self.liftoff_jump_events.push(distance);
                }

                // Update position
                self.liftoff_position.0 += delta.x as i64;
                self.liftoff_position.1 += delta.y as i64;

                // Reset idle state
                self.liftoff_is_idle = false;
                self.liftoff_last_move = now;
            } else {
                // No movement - check if we've been idle long enough
                let time_since_move = now.duration_since(self.liftoff_last_move).as_millis() as u64;
                if time_since_move > IDLE_THRESHOLD_MS {
                    self.liftoff_is_idle = true;
                }
            }
        }
    }
}
