use eframe::egui;
use egui_plot::{Plot, Line, PlotPoints, Points};
use std::collections::VecDeque;
use std::time::Instant;

pub struct AccelPanel {
    // Acceleration detection
    is_running: bool,
    samples: Vec<AccelSample>,
    slow_movements: VecDeque<f64>,
    fast_movements: VecDeque<f64>,
    current_velocity: f64,
    last_move_time: Option<Instant>,
    detection_result: Option<AccelResult>,
    /// Accumulated distance for ratio calculation
    accumulated_distance: f64,
    /// Reference distance for slow movement
    reference_distance: Option<f64>,

    // Angle snapping detection
    angle_running: bool,
    angle_points: Vec<(f64, f64)>,
    angle_result: Option<AngleResult>,
    /// Accumulated position for angle visualization
    angle_accumulated_pos: (f64, f64),
}

struct AccelSample {
    velocity: f64,
    distance_ratio: f64,
}

struct AccelResult {
    has_acceleration: bool,
    accel_factor: f64,
    confidence: f32,
}

struct AngleResult {
    has_snapping: bool,
    snap_strength: f64,
    dominant_angles: Vec<f64>,
}

impl AccelPanel {
    pub fn new() -> Self {
        Self {
            is_running: false,
            samples: Vec::new(),
            slow_movements: VecDeque::with_capacity(100),
            fast_movements: VecDeque::with_capacity(100),
            current_velocity: 0.0,
            last_move_time: None,
            detection_result: None,
            accumulated_distance: 0.0,
            reference_distance: None,
            angle_running: false,
            angle_points: Vec::new(),
            angle_result: None,
            angle_accumulated_pos: (0.0, 0.0),
        }
    }

    pub fn ui_accel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.ui(ui, ctx);
    }

    pub fn ui_angle(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Angle Snapping Detection");
        ui.add_space(5.0);
        ui.label("Detects if your mouse firmware corrects diagonal movements to straight lines.");
        ui.add_space(15.0);

        // Controls
        ui.horizontal(|ui| {
            if self.angle_running {
                if ui.button("Stop Test").clicked() {
                    self.angle_running = false;
                    self.analyze_angle_results();
                }
            } else {
                if ui.button("Start Test").clicked() {
                    self.angle_running = true;
                    self.angle_points.clear();
                    self.angle_result = None;
                    self.angle_accumulated_pos = (0.0, 0.0);
                }
            }

            if ui.button("Clear Data").clicked() {
                self.angle_points.clear();
                self.angle_result = None;
            }
        });

        ui.add_space(15.0);

        // Instructions
        if self.angle_running {
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(40, 60, 40))
                .inner_margin(15.0)
                .rounding(8.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Test in Progress").strong().color(egui::Color32::GREEN));
                    ui.add_space(5.0);
                    ui.label("Draw diagonal lines across the test area.");
                    ui.label("Try various angles: 15°, 30°, 45°, 60°, etc.");
                    ui.add_space(10.0);
                    ui.label(format!("Points recorded: {}", self.angle_points.len()));
                });
        } else {
            egui::Frame::none()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(15.0)
                .rounding(8.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("What is Angle Snapping?").strong());
                    ui.add_space(5.0);
                    ui.label("Angle snapping (prediction) forces diagonal movements to snap to");
                    ui.label("straight horizontal or vertical lines. This can be problematic for:");
                    ui.label("• FPS gaming (affects aiming accuracy)");
                    ui.label("• Drawing/design work");
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("During the test:").strong());
                    ui.label("Draw diagonal lines at various angles. If snapping exists,");
                    ui.label("near-horizontal/vertical lines will appear unnaturally straight.");
                });
        }

        ui.add_space(20.0);

        // Drawing area
        ui.heading("Movement Path Visualization");

        let points: PlotPoints = self.angle_points
            .iter()
            .map(|(x, y)| [*x, *y])
            .collect();

        let scatter = Points::new(points)
            .color(egui::Color32::from_rgb(100, 200, 255))
            .radius(2.0)
            .name("Movement Path");

        Plot::new("angle_snap_plot")
            .height(300.0)
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
        if let Some(result) = &self.angle_result {
            ui.heading("Detection Result");

            egui::Frame::dark_canvas(ui.style())
                .inner_margin(20.0)
                .rounding(8.0)
                .show(ui, |ui| {
                    let (status, color) = if result.has_snapping {
                        ("Angle Snapping DETECTED", egui::Color32::RED)
                    } else {
                        ("No Angle Snapping Detected", egui::Color32::GREEN)
                    };

                    ui.label(egui::RichText::new(status).size(24.0).strong().color(color));
                    ui.add_space(15.0);

                    ui.label(format!("Snap Strength: {:.1}%", result.snap_strength * 100.0));

                    if !result.dominant_angles.is_empty() {
                        ui.add_space(10.0);
                        ui.label("Dominant Angles Detected:");
                        for angle in &result.dominant_angles {
                            ui.label(format!("  • {:.0}°", angle));
                        }
                    }

                    if result.has_snapping {
                        ui.add_space(15.0);
                        ui.label(egui::RichText::new("Recommendation: Check mouse software for angle snapping/prediction settings")
                            .color(egui::Color32::YELLOW));
                    }
                });
        }

        // Capture real mouse input while running
        if self.angle_running {
            ctx.request_repaint();

            let delta = ctx.input(|i| i.pointer.delta());
            if delta.x != 0.0 || delta.y != 0.0 {
                // Accumulate position from deltas
                self.angle_accumulated_pos.0 += delta.x as f64;
                self.angle_accumulated_pos.1 += delta.y as f64;
                self.angle_points.push((self.angle_accumulated_pos.0, self.angle_accumulated_pos.1));

                // Keep a reasonable number of points
                if self.angle_points.len() > 2000 {
                    self.angle_points.remove(0);
                }
            }
        }
    }

    fn analyze_angle_results(&mut self) {
        if self.angle_points.len() < 50 {
            return;
        }

        // Analyze for horizontal/vertical clustering
        let mut h_count = 0;
        let mut v_count = 0;
        let mut total_segments = 0;

        for i in 1..self.angle_points.len() {
            let dx = self.angle_points[i].0 - self.angle_points[i - 1].0;
            let dy = self.angle_points[i].1 - self.angle_points[i - 1].1;

            if dx.abs() > 0.1 || dy.abs() > 0.1 {
                total_segments += 1;
                let angle = dy.atan2(dx).to_degrees().abs();

                // Check for near-horizontal (0° or 180°) or near-vertical (90°)
                if angle < 5.0 || angle > 175.0 {
                    h_count += 1;
                } else if (angle - 90.0).abs() < 5.0 {
                    v_count += 1;
                }
            }
        }

        let snap_ratio = (h_count + v_count) as f64 / total_segments.max(1) as f64;
        let has_snapping = snap_ratio > 0.3; // More than 30% are axis-aligned

        let mut dominant_angles = Vec::new();
        if h_count > total_segments / 4 {
            dominant_angles.push(0.0);
        }
        if v_count > total_segments / 4 {
            dominant_angles.push(90.0);
        }

        self.angle_result = Some(AngleResult {
            has_snapping,
            snap_strength: snap_ratio,
            dominant_angles,
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Acceleration Detection");
        ui.add_space(5.0);
        ui.label("Detects mouse acceleration (pointer speed varies with movement speed).");
        ui.add_space(15.0);

        // Controls
        ui.horizontal(|ui| {
            if self.is_running {
                if ui.button("Stop Test").clicked() {
                    self.stop_test();
                }
            } else {
                if ui.button("Start Test").clicked() {
                    self.start_test();
                }
            }

            if ui.button("Clear Data").clicked() {
                self.clear_data();
            }
        });

        ui.add_space(15.0);

        // Instructions
        if self.is_running {
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(40, 60, 40))
                .inner_margin(15.0)
                .rounding(8.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Test in Progress").strong().color(egui::Color32::GREEN));
                    ui.add_space(5.0);
                    ui.label("Alternate between SLOW and FAST mouse movements");
                    ui.label("Move the same physical distance at different speeds");
                    ui.add_space(10.0);
                    ui.label(format!("Current velocity: {:.1} px/s", self.current_velocity));
                    ui.label(format!("Slow samples: {} | Fast samples: {}",
                        self.slow_movements.len(), self.fast_movements.len()));
                });
        } else {
            egui::Frame::none()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(15.0)
                .rounding(8.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("How This Test Works").strong());
                    ui.add_space(5.0);
                    ui.label("Mouse acceleration causes the cursor to move MORE when you move the mouse faster.");
                    ui.label("This test compares slow vs fast movements to detect this behavior.");
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("During the test:").strong());
                    ui.label("1. Move mouse SLOWLY across the same distance repeatedly");
                    ui.label("2. Then move mouse QUICKLY across the same distance");
                    ui.label("3. If acceleration exists, fast movements will register more counts");
                });
        }

        ui.add_space(20.0);

        // Velocity graph
        ui.heading("Velocity vs Distance Ratio");

        let points: PlotPoints = self.samples
            .iter()
            .map(|s| [s.velocity, s.distance_ratio])
            .collect();

        let scatter = egui_plot::Points::new(points)
            .color(egui::Color32::from_rgb(100, 200, 255))
            .radius(4.0)
            .name("Samples");

        // Ideal line (no acceleration = 1.0 ratio at all velocities)
        let ideal_line = Line::new(PlotPoints::from_explicit_callback(
            |_x| 1.0,
            0.0..2000.0,
            100
        ))
        .color(egui::Color32::GREEN)
        .name("No Acceleration (Ideal)")
        .width(2.0);

        Plot::new("accel_plot")
            .height(250.0)
            .x_axis_label("Movement Velocity (px/s)")
            .y_axis_label("Distance Ratio")
            .show_axes(true)
            .show_grid(true)
            .allow_drag(false)
            .allow_zoom(false)
            .show(ui, |plot_ui| {
                plot_ui.line(ideal_line);
                plot_ui.points(scatter);
            });

        ui.add_space(20.0);

        // Results
        if let Some(result) = &self.detection_result {
            ui.heading("Detection Result");

            egui::Frame::dark_canvas(ui.style())
                .inner_margin(20.0)
                .rounding(8.0)
                .show(ui, |ui| {
                    let (status, color) = if result.has_acceleration {
                        ("Acceleration DETECTED", egui::Color32::RED)
                    } else {
                        ("No Acceleration Detected", egui::Color32::GREEN)
                    };

                    ui.label(egui::RichText::new(status).size(24.0).strong().color(color));
                    ui.add_space(15.0);

                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label("Acceleration Factor");
                            ui.label(egui::RichText::new(format!("{:.2}x", result.accel_factor)).size(20.0));
                        });
                        ui.add_space(40.0);
                        ui.vertical(|ui| {
                            ui.label("Confidence");
                            ui.label(egui::RichText::new(format!("{:.0}%", result.confidence)).size(20.0));
                        });
                    });

                    if result.has_acceleration {
                        ui.add_space(15.0);
                        ui.label(egui::RichText::new("Recommendation: Disable mouse acceleration in your OS settings")
                            .color(egui::Color32::YELLOW));
                    }
                });
        }

        // Capture real mouse input
        if self.is_running {
            ctx.request_repaint();

            let delta = ctx.input(|i| i.pointer.delta());
            let now = Instant::now();

            if delta.x != 0.0 || delta.y != 0.0 {
                let distance = ((delta.x * delta.x + delta.y * delta.y) as f64).sqrt();

                // Calculate velocity (pixels per second)
                if let Some(last_time) = self.last_move_time {
                    let dt = now.duration_since(last_time).as_secs_f64();
                    if dt > 0.0 && dt < 0.5 {
                        let velocity = distance / dt;
                        self.current_velocity = velocity;

                        // Accumulate distance
                        self.accumulated_distance += distance;

                        // Categorize by velocity
                        if velocity < 400.0 {
                            // Slow movement
                            self.slow_movements.push_back(distance);
                            if self.slow_movements.len() > 100 {
                                self.slow_movements.pop_front();
                            }
                        } else {
                            // Fast movement
                            self.fast_movements.push_back(distance);
                            if self.fast_movements.len() > 100 {
                                self.fast_movements.pop_front();
                            }
                        }

                        // Calculate ratio based on velocity comparison
                        // With acceleration, faster movements should cover more distance per count
                        let slow_avg = if self.slow_movements.is_empty() {
                            1.0
                        } else {
                            self.slow_movements.iter().sum::<f64>() / self.slow_movements.len() as f64
                        };

                        let ratio = distance / slow_avg.max(1.0);

                        self.samples.push(AccelSample {
                            velocity,
                            distance_ratio: ratio,
                        });

                        if self.samples.len() > 500 {
                            self.samples.remove(0);
                        }
                    }
                }

                self.last_move_time = Some(now);
            }
        }
    }

    fn start_test(&mut self) {
        self.is_running = true;
        self.samples.clear();
        self.slow_movements.clear();
        self.fast_movements.clear();
        self.detection_result = None;
        self.last_move_time = None;
        self.accumulated_distance = 0.0;
        self.reference_distance = None;
    }

    fn stop_test(&mut self) {
        self.is_running = false;
        self.analyze_results();
    }

    fn clear_data(&mut self) {
        self.samples.clear();
        self.slow_movements.clear();
        self.fast_movements.clear();
        self.detection_result = None;
    }

    fn analyze_results(&mut self) {
        if self.slow_movements.len() < 5 || self.fast_movements.len() < 5 {
            return;
        }

        let slow_avg: f64 = self.slow_movements.iter().sum::<f64>() / self.slow_movements.len() as f64;
        let fast_avg: f64 = self.fast_movements.iter().sum::<f64>() / self.fast_movements.len() as f64;

        let accel_factor = fast_avg / slow_avg.max(0.001);
        let has_acceleration = accel_factor > 1.15 || accel_factor < 0.85;

        let sample_count = (self.slow_movements.len() + self.fast_movements.len()) as f32;
        let confidence = (sample_count / 50.0 * 100.0).min(100.0);

        self.detection_result = Some(AccelResult {
            has_acceleration,
            accel_factor,
            confidence,
        });
    }
}
