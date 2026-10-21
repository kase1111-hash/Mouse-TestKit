//! Scroll wheel test panel
//!
//! Counts wheel notches and runs the checks in
//! `mouse_testkit::analysis::scroll`: notches reported in the wrong direction
//! (encoder skipping) and a direction that never registers. Scrolling speed
//! is shown but not rated, since it describes the hand rather than the wheel.

use eframe::egui;
use std::collections::VecDeque;
use std::time::Instant;

use crate::export::ScrollExport;
use crate::input_bridge::{RawInputEvent, RawInputKind};
use mouse_testkit::analysis::scroll::{WheelCheck, WheelVerdict};

pub struct ScrollPanel {
    is_running: bool,
    /// Scroll events with their delta values
    scroll_events: VecDeque<ScrollEvent>,
    /// Total scroll up distance
    total_up: f32,
    /// Total scroll down distance
    total_down: f32,
    /// Total scroll steps (wheel reports)
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
    /// Skip and dead-direction checks over every wheel report
    wheel: WheelCheck,
    /// Time of the first wheel report, the origin for `wheel` timestamps
    epoch: Option<Instant>,
    /// Framework line scrolling not yet adding up to a whole notch
    line_remainder: f32,
    /// Framework input delivered pixel (smooth) scrolling, which has no notches
    saw_smooth_scroll: bool,
}

struct ScrollEvent {
    timestamp: Instant,
    direction_up: bool,
}

/// Whole notches from framework line deltas. High-resolution wheels send
/// fractions of a line; they are summed until they make a whole notch. A
/// change of direction starts the sum again, so a reversed notch is never
/// cancelled out by a leftover fraction from the other direction.
fn whole_notches(remainder: &mut f32, lines: f32) -> i32 {
    if *remainder != 0.0 && remainder.signum() != lines.signum() {
        *remainder = 0.0;
    }
    *remainder += lines;
    // The small bias keeps sums like 8 x 0.125 from landing just under 1.0
    let whole = (*remainder + remainder.signum() * 1e-3).trunc();
    *remainder -= whole;
    whole as i32
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
            wheel: WheelCheck::new(),
            epoch: None,
            line_remainder: 0.0,
            saw_smooth_scroll: false,
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
        ui.label(
            "Counts wheel notches and checks for skipping (notches reported in the wrong direction) and a direction that does not register.",
        );
        if !has_bridge {
            ui.label(
                egui::RichText::new(
                    "Note: Raw input unavailable — counting the scroll lines your system reports. System scroll acceleration can make these differ from physical notches.",
                )
                .color(egui::Color32::YELLOW)
                .size(11.0),
            );
        }
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
        // Keep the wheel from scrolling the page while testing. Otherwise a
        // few notches move the test area out from under the pointer, and
        // framework input (counted only while hovering it) loses notches.
        if self.is_running && response.hovered() {
            ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
        }

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
                        ui.label("Skips");
                        let color = if self.wheel.skips == 0 {
                            egui::Color32::WHITE
                        } else {
                            egui::Color32::YELLOW
                        };
                        ui.label(
                            egui::RichText::new(format!("{}", self.wheel.skips))
                                .size(18.0)
                                .color(color),
                        );
                    });
                    ui.add_space(30.0);
                    ui.vertical(|ui| {
                        ui.label("Direction Changes");
                        ui.label(
                            egui::RichText::new(format!("{}", self.direction_changes)).size(18.0),
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

        // Wheel checks
        if self.step_count > 0 || self.saw_smooth_scroll {
            ui.heading("Analysis");

            let (title, color, message) = self.verdict_text();

            egui::Frame::dark_canvas(ui.style())
                .inner_margin(15.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(title).size(18.0).strong().color(color));
                    ui.add_space(5.0);
                    ui.label(message);
                    if self.saw_smooth_scroll {
                        ui.add_space(5.0);
                        ui.label(
                            egui::RichText::new(
                                "Smooth (pixel) scrolling from a touchpad or touch-surface mouse was received and not counted: it has no notches to check.",
                            )
                            .weak(),
                        );
                    }
                });
        }

        ui.add_space(20.0);

        // Instructions
        let min_each = self.wheel.min_each();
        let skip_ms = mouse_testkit::analysis::diagnostics::Config::default().scroll_reversal_ms;
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Instructions").strong());
                ui.label("1. Click Start, then hover over the test area");
                ui.label(format!(
                    "2. Scroll steadily down at least {} notches, then up at least {}",
                    min_each, min_each
                ));
                ui.label(format!(
                    "3. A notch reported the opposite way within {:.0} ms of the previous one is a skip: a finger cannot turn the wheel back that fast",
                    skip_ms
                ));
                ui.label("4. Lost notches are not detected automatically: roll the wheel slowly by 10 clicks you can feel and check the counter rose by exactly 10");
                ui.add_space(5.0);
                ui.label(
                    egui::RichText::new(
                        "sps = steps per second. Speed describes your hand, not the wheel, and is not rated.",
                    )
                    .weak(),
                );
            });

        // Capture scroll input
        if self.is_running {
            if has_bridge {
                // Use raw input events from the input bridge (no hover check needed)
                for event in raw_events {
                    if let RawInputKind::Scroll { delta } = &event.kind {
                        self.record(event.timestamp, *delta);
                    }
                }
            } else if response.hovered() {
                // Fallback: egui scroll input (requires hover over test area).
                // Each wheel event is handled on its own so a reversed notch
                // is not cancelled out by a forward one in the same frame.
                let now = Instant::now();
                let wheel_events: Vec<(egui::MouseWheelUnit, f32)> = ctx.input(|i| {
                    i.raw
                        .events
                        .iter()
                        .filter_map(|e| match e {
                            egui::Event::MouseWheel { unit, delta, .. } => Some((*unit, delta.y)),
                            _ => None,
                        })
                        .collect()
                });
                for (unit, lines) in wheel_events {
                    if lines == 0.0 {
                        continue;
                    }
                    match unit {
                        egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => {
                            let notches = whole_notches(&mut self.line_remainder, lines);
                            if notches != 0 {
                                self.record(now, notches);
                            }
                        }
                        egui::MouseWheelUnit::Point => self.saw_smooth_scroll = true,
                    }
                }
            }
        }
    }

    /// Record one wheel report of `delta` notches (positive = up).
    fn record(&mut self, now: Instant, delta: i32) {
        if delta == 0 {
            return;
        }
        let direction_up = delta > 0;
        let abs_delta = delta.unsigned_abs() as f32;

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

        // Skip and dead-direction checks, on seconds since the first report
        let epoch = *self.epoch.get_or_insert(now);
        let t = match now.checked_duration_since(epoch) {
            Some(d) => d.as_secs_f64(),
            None => -epoch.duration_since(now).as_secs_f64(),
        };
        self.wheel.feed(t, delta);

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
            self.current_speed = recent.iter().copied().sum::<f64>() / recent.len() as f64;
        }
    }

    /// Title, colour and explanation for the current wheel verdict.
    fn verdict_text(&self) -> (String, egui::Color32, String) {
        let w = &self.wheel;
        let skip_ms = mouse_testkit::analysis::diagnostics::Config::default().scroll_reversal_ms;
        match w.verdict() {
            WheelVerdict::Skipping => (
                "Wheel is skipping".to_string(),
                egui::Color32::from_rgb(255, 90, 90),
                format!(
                    "{} notches were reported in the opposite direction within {:.0} ms of the previous notch, faster than a finger can turn the wheel back. The encoder is worn or dirty: the page will jump back while scrolling.",
                    w.skips, skip_ms
                ),
            ),
            WheelVerdict::OneDirectionOnly { missing_up } => {
                let (seen, missing, count) = if missing_up {
                    ("down", "up", w.down)
                } else {
                    ("up", "down", w.up)
                };
                (
                    format!("No {} notches registered", missing),
                    egui::Color32::YELLOW,
                    format!(
                        "{} notches {} and none {}. Scroll {} too: if nothing registers, that direction of the wheel encoder is dead.",
                        count, seen, missing, missing
                    ),
                )
            }
            WheelVerdict::PossibleSkip => (
                "Possible skip".to_string(),
                egui::Color32::YELLOW,
                format!(
                    "{} notch(es) reversed direction within {:.0} ms of the previous notch. This can be the hand flicking the wheel back; keep scrolling steadily one way, and if it repeats the encoder is failing.",
                    w.skips, skip_ms
                ),
            ),
            WheelVerdict::KeepScrolling => (
                "Keep scrolling".to_string(),
                egui::Color32::LIGHT_GRAY,
                format!(
                    "Scroll at least {} notches each way to check the wheel ({} down, {} up so far).",
                    w.min_each(),
                    w.down,
                    w.up
                ),
            ),
            WheelVerdict::NoFaultFound => (
                "No skipping detected".to_string(),
                egui::Color32::GREEN,
                format!(
                    "{} notches, both directions register and none were reversed.",
                    w.total()
                ),
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn start_for_test(&mut self) {
        self.is_running = true;
        self.reset();
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
        self.wheel = WheelCheck::new();
        self.epoch = None;
        self.line_remainder = 0.0;
        self.saw_smooth_scroll = false;
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

        // Share of wheel reports that went the way the wheel was being
        // turned (100 = no skips). Scrolling speed is deliberately not part
        // of it: speed variation measures the hand, not the wheel.
        let consistency =
            ((1.0 - self.wheel.skips as f64 / self.step_count as f64) * 100.0).round() as u32;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn run(panel: &mut ScrollPanel, notches: &[(u64, i32)]) {
        let ctx = egui::Context::default();
        let start = Instant::now();
        let events: Vec<RawInputEvent> = notches
            .iter()
            .map(|&(ms, delta)| RawInputEvent {
                kind: RawInputKind::Scroll { delta },
                timestamp: start + Duration::from_millis(ms),
            })
            .collect();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx().clone();
            panel.ui(ui, &ctx, &events, true);
        });
    }

    /// Stroke-style scrolling: bursts of 5 notches 30 ms apart, with a
    /// 200 ms pause to reposition the finger, 4 strokes down then 4 up.
    /// Every other stroke is slow (120 ms per notch), as the old
    /// instructions asked ("scroll at different speeds").
    fn stroke_scrolling() -> Vec<(u64, i32)> {
        let mut t = 0;
        let mut out = Vec::new();
        for dir in [-1, 1] {
            for stroke in 0..4 {
                for _ in 0..5 {
                    t += if stroke % 2 == 0 { 30 } else { 120 };
                    out.push((t, dir));
                }
                t += 200;
            }
            t += 500;
        }
        out
    }

    #[test]
    fn healthy_wheel_with_stroke_scrolling_is_not_rated_poor() {
        let mut panel = ScrollPanel::new();
        panel.start_for_test();
        run(&mut panel, &stroke_scrolling());
        let export = panel.export().unwrap();
        assert_eq!(export.total_steps, 40);
        assert_eq!(export.consistency_percent, 100);
        assert_eq!(panel.wheel.verdict(), WheelVerdict::NoFaultFound);
    }

    #[test]
    fn skipping_wheel_is_reported_from_raw_events() {
        let mut panel = ScrollPanel::new();
        panel.start_for_test();
        // Steady scrolling down, 25 ms per notch; every 5th report jumps up
        let mut notches: Vec<(u64, i32)> = (1..=40)
            .map(|i| (i * 25, if i % 5 == 0 { 1 } else { -1 }))
            .collect();
        notches.extend((1..=12).map(|i| (2000 + i * 25, 1)));
        run(&mut panel, &notches);
        assert_eq!(panel.wheel.skips, 8);
        assert_eq!(panel.wheel.verdict(), WheelVerdict::Skipping);
        assert!(panel.export().unwrap().consistency_percent < 100);
    }

    #[test]
    fn framework_input_counts_each_wheel_event_in_a_frame() {
        let ctx = egui::Context::default();
        let mut panel = ScrollPanel::new();
        panel.start_for_test();
        let screen = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 900.0),
        ));
        let wheel = |unit, y| egui::Event::MouseWheel {
            unit,
            delta: egui::vec2(0.0, y),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        };
        let frames = [
            vec![egui::Event::PointerMoved(egui::pos2(400.0, 200.0))],
            vec![
                wheel(egui::MouseWheelUnit::Line, -1.0),
                wheel(egui::MouseWheelUnit::Line, -1.0),
                // Encoder skip: one notch the wrong way within the frame
                wheel(egui::MouseWheelUnit::Line, 1.0),
                wheel(egui::MouseWheelUnit::Line, -1.0),
                wheel(egui::MouseWheelUnit::Point, -12.0),
            ],
        ];
        for events in frames {
            let input = egui::RawInput {
                screen_rect: screen,
                events,
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let ctx = ui.ctx().clone();
                panel.ui(ui, &ctx, &[], false);
            });
        }
        assert_eq!(panel.step_count, 4);
        assert_eq!(panel.total_down, 3.0);
        assert_eq!(panel.total_up, 1.0);
        assert_eq!(panel.wheel.skips, 1);
        assert!(panel.saw_smooth_scroll);
    }

    #[test]
    fn framework_input_keeps_every_notch_when_the_page_could_scroll() {
        // In the app the panel sits in a scrolling page taller than the
        // default window. The wheel must not scroll the test area away
        // from the pointer, or framework input stops counting notches and
        // the "count 10 detents" check would wrongly suggest lost notches.
        let ctx = egui::Context::default();
        let mut panel = ScrollPanel::new();
        panel.start_for_test();
        let mut frame = |panel: &mut ScrollPanel, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 600.0),
                )),
                events,
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                let ctx = ui.ctx().clone();
                egui::ScrollArea::vertical().show(ui, |ui| panel.ui(ui, &ctx, &[], false));
            });
        };
        frame(
            &mut panel,
            vec![egui::Event::PointerMoved(egui::pos2(300.0, 200.0))],
        );
        for dir in [-1.0, 1.0] {
            for _ in 0..10 {
                let notch = egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(0.0, dir),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                };
                frame(&mut panel, vec![notch]);
                // Let egui's smooth scrolling play out between notches
                for _ in 0..10 {
                    frame(&mut panel, Vec::new());
                }
            }
        }
        assert_eq!(panel.total_down, 10.0);
        assert_eq!(panel.total_up, 10.0);
    }

    #[test]
    fn framework_line_deltas_make_whole_notches() {
        let mut r = 0.0;
        // Classic wheel: one line per notch
        assert_eq!(whole_notches(&mut r, -1.0), -1);
        assert_eq!(whole_notches(&mut r, 1.0), 1);
        // High-resolution wheel: 1/8 notch per event
        let sum: i32 = (0..16).map(|_| whole_notches(&mut r, 0.125)).sum();
        assert_eq!(sum, 2);
        // A reversed notch is not swallowed by a leftover fraction
        let mut r = 0.0;
        assert_eq!(whole_notches(&mut r, 0.5), 0);
        assert_eq!(whole_notches(&mut r, -1.0), -1);
        // Multi-line events from fast scrolling
        assert_eq!(whole_notches(&mut r, -3.0), -3);
    }
}
