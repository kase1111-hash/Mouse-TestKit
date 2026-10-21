use eframe::egui;
use std::collections::VecDeque;
use std::time::Instant;

use crate::export::{
    ClickButtonExport, ClickResponseExport, ClickStickyExport, LiftOffExport, StickyButtonExport,
};
use crate::input_bridge::{RawButton, RawInputEvent, RawInputKind};
use mouse_testkit::analysis::diagnostics::Config as DiagConfig;

/// Jump threshold in pixels - movements larger than this after idle are considered jumps
const JUMP_THRESHOLD_PX: f64 = 15.0;
/// Time in milliseconds without movement to consider mouse "idle" (potential lift)
const IDLE_THRESHOLD_MS: u64 = 80;

/// Smallest value in the iterator, or 0.0 when it is empty (so an export
/// never contains `f64::MAX`).
fn min_or_zero(values: impl Iterator<Item = f64>) -> f64 {
    values
        .fold(None, |acc: Option<f64>, v| {
            Some(acc.map_or(v, |a| a.min(v)))
        })
        .unwrap_or(0.0)
}

/// What the stickiness test says about one button. Uses the Auto
/// Diagnostics thresholds so the GUI, CLI and Auto Diagnostics agree.
#[derive(Debug, Clone, Copy, PartialEq)]
enum StickyVerdict {
    /// The button has been down longer than any click lasts.
    HeldDown { held_ms: f64 },
    /// Fewer clicks than needed to judge.
    NeedMoreClicks { need: usize },
    /// Two or more long holds: the switch may be releasing late.
    SlowReleases { count: usize, longest_ms: f64 },
    /// A single long hold: usually the hand, not the switch.
    OneLongHold { held_ms: f64 },
    /// No click held past the threshold.
    NoSlowReleases,
}

/// Judge one button from its completed hold times and, if it is down right
/// now, how long it has been held. A deliberate click is held about
/// 80-150 ms, so only holds past `sticky_threshold_ms` (250 ms) count, and
/// one long hold is not enough: anyone can linger on a click once.
fn sticky_verdict(
    holds_ms: &[f64],
    current_hold_ms: Option<f64>,
    cfg: &DiagConfig,
) -> StickyVerdict {
    if let Some(held_ms) = current_hold_ms.filter(|&h| h > cfg.stuck_threshold_ms) {
        return StickyVerdict::HeldDown { held_ms };
    }
    if holds_ms.len() < cfg.clicks_min_left {
        return StickyVerdict::NeedMoreClicks {
            need: cfg.clicks_min_left,
        };
    }
    let long: Vec<f64> = holds_ms
        .iter()
        .copied()
        .filter(|&h| h > cfg.sticky_threshold_ms)
        .collect();
    let longest_ms = long.iter().copied().fold(0.0, f64::max);
    match long.len() {
        0 => StickyVerdict::NoSlowReleases,
        1 => StickyVerdict::OneLongHold {
            held_ms: longest_ms,
        },
        count => StickyVerdict::SlowReleases { count, longest_ms },
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum TestButton {
    Left,
    Right,
}

pub struct ClickPanel {
    // Click Response - real click testing
    response_running: bool,
    response_button: TestButton,
    /// Total clicks recorded
    response_click_count: usize,
    response_right_click_count: usize,
    /// Hold durations in ms
    response_hold_times: VecDeque<f64>,
    response_right_hold_times: VecDeque<f64>,
    /// When the current click started (button pressed)
    response_press_start: Option<Instant>,
    response_right_press_start: Option<Instant>,
    /// Is button currently held down
    response_is_pressed: bool,
    response_right_is_pressed: bool,
    /// Timestamps of recent clicks for CPS calculation
    response_click_times: VecDeque<Instant>,
    response_right_click_times: VecDeque<Instant>,
    /// Current CPS
    response_cps: f64,
    response_right_cps: f64,

    // Click Sticky
    sticky_running: bool,
    sticky_button: TestButton,
    sticky_holds: Vec<f64>,
    sticky_right_holds: Vec<f64>,
    sticky_count: usize,
    sticky_right_count: usize,
    /// When click started for hold measurement
    sticky_press_start: Option<Instant>,
    sticky_right_press_start: Option<Instant>,
    /// Is button currently held
    sticky_is_pressed: bool,
    sticky_right_is_pressed: bool,

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
            response_button: TestButton::Left,
            response_click_count: 0,
            response_right_click_count: 0,
            response_hold_times: VecDeque::new(),
            response_right_hold_times: VecDeque::new(),
            response_press_start: None,
            response_right_press_start: None,
            response_is_pressed: false,
            response_right_is_pressed: false,
            response_click_times: VecDeque::with_capacity(100),
            response_right_click_times: VecDeque::with_capacity(100),
            response_cps: 0.0,
            response_right_cps: 0.0,

            sticky_running: false,
            sticky_button: TestButton::Left,
            sticky_holds: Vec::new(),
            sticky_right_holds: Vec::new(),
            sticky_count: 0,
            sticky_right_count: 0,
            sticky_press_start: None,
            sticky_right_press_start: None,
            sticky_is_pressed: false,
            sticky_right_is_pressed: false,

            liftoff_running: false,
            liftoff_jumps: 0,
            liftoff_position: (0, 0),
            liftoff_last_move: Instant::now(),
            liftoff_is_idle: false,
            liftoff_jump_events: Vec::new(),
        }
    }

    pub fn is_running(&self) -> bool {
        self.response_running || self.sticky_running || self.liftoff_running
    }

    pub fn ui_response(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
    ) {
        ui.heading("Click Test");
        ui.add_space(5.0);
        ui.label("Counts clicks and measures how long each press is held.");
        ui.label(
            egui::RichText::new(
                "Click latency (the delay inside the mouse) is not measured: that needs external hardware.",
            )
            .weak(),
        );
        ui.add_space(15.0);

        // Button selector
        ui.horizontal(|ui| {
            ui.label("Test Button:");
            ui.selectable_value(&mut self.response_button, TestButton::Left, "Left Click");
            ui.selectable_value(&mut self.response_button, TestButton::Right, "Right Click");
        });

        ui.add_space(10.0);

        // Controls
        ui.horizontal(|ui| {
            if self.response_running {
                if ui.button("Stop").clicked() {
                    self.response_running = false;
                }
            } else if ui.button("Start").clicked() {
                self.response_running = true;
                self.response_click_count = 0;
                self.response_right_click_count = 0;
                self.response_hold_times.clear();
                self.response_right_hold_times.clear();
                self.response_click_times.clear();
                self.response_right_click_times.clear();
                self.response_cps = 0.0;
                self.response_right_cps = 0.0;
                self.response_press_start = None;
                self.response_right_press_start = None;
                self.response_is_pressed = false;
                self.response_right_is_pressed = false;
            }

            if ui.button("Reset").clicked() {
                self.response_running = false;
                self.response_click_count = 0;
                self.response_right_click_count = 0;
                self.response_hold_times.clear();
                self.response_right_hold_times.clear();
                self.response_click_times.clear();
                self.response_right_click_times.clear();
                self.response_cps = 0.0;
                self.response_right_cps = 0.0;
                self.response_press_start = None;
                self.response_right_press_start = None;
                self.response_is_pressed = false;
                self.response_right_is_pressed = false;
            }
        });

        ui.add_space(20.0);

        // Click area - changes color when pressed
        let (rect, _response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 150.0),
            egui::Sense::hover(),
        );

        let is_pressed = match self.response_button {
            TestButton::Left => self.response_is_pressed,
            TestButton::Right => self.response_right_is_pressed,
        };

        let color = if is_pressed {
            match self.response_button {
                TestButton::Left => egui::Color32::from_rgb(50, 200, 50), // Green for left
                TestButton::Right => egui::Color32::from_rgb(50, 150, 200), // Blue for right
            }
        } else if self.response_running {
            egui::Color32::from_rgb(60, 60, 80) // Dark when waiting
        } else {
            egui::Color32::from_rgb(50, 50, 50)
        };

        ui.painter().rect_filled(rect, 8.0, color);

        let button_name = match self.response_button {
            TestButton::Left => "Left",
            TestButton::Right => "Right",
        };

        let text = if !self.response_running {
            format!(
                "Click Start, then {} click here",
                button_name.to_lowercase()
            )
        } else if is_pressed {
            "PRESSED".to_string()
        } else {
            format!("{} click here!", button_name)
        };

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(28.0),
            egui::Color32::WHITE,
        );

        ui.add_space(20.0);

        // Stats display - show stats for selected button
        let (click_count, cps, hold_times_slice): (usize, f64, Vec<f64>) =
            match self.response_button {
                TestButton::Left => (
                    self.response_click_count,
                    self.response_cps,
                    self.response_hold_times.iter().cloned().collect(),
                ),
                TestButton::Right => (
                    self.response_right_click_count,
                    self.response_right_cps,
                    self.response_right_hold_times.iter().cloned().collect(),
                ),
            };

        egui::Frame::dark_canvas(ui.style())
            .inner_margin(20.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(format!("{} Clicks", button_name));
                        ui.label(
                            egui::RichText::new(format!("{}", click_count))
                                .size(24.0)
                                .strong(),
                        );
                    });
                    ui.add_space(40.0);
                    ui.vertical(|ui| {
                        ui.label("CPS");
                        ui.label(
                            egui::RichText::new(format!("{:.1}", cps))
                                .size(24.0)
                                .color(egui::Color32::YELLOW),
                        );
                    });
                    ui.add_space(40.0);
                    ui.vertical(|ui| {
                        ui.label("Avg Hold");
                        let avg_hold = if hold_times_slice.is_empty() {
                            0.0
                        } else {
                            hold_times_slice.iter().sum::<f64>() / hold_times_slice.len() as f64
                        };
                        ui.label(egui::RichText::new(format!("{:.1} ms", avg_hold)).size(24.0));
                    });
                    ui.add_space(40.0);
                    ui.vertical(|ui| {
                        ui.label("Min Hold");
                        let min_hold = hold_times_slice.iter().cloned().fold(f64::MAX, f64::min);
                        let min_str = if min_hold == f64::MAX {
                            "- ms".to_string()
                        } else {
                            format!("{:.1} ms", min_hold)
                        };
                        ui.label(
                            egui::RichText::new(min_str)
                                .size(24.0)
                                .color(egui::Color32::LIGHT_GREEN),
                        );
                    });
                    ui.add_space(40.0);
                    ui.vertical(|ui| {
                        ui.label("Max Hold");
                        let max_hold = hold_times_slice.iter().cloned().fold(0.0, f64::max);
                        ui.label(
                            egui::RichText::new(format!("{:.1} ms", max_hold))
                                .size(24.0)
                                .color(egui::Color32::LIGHT_RED),
                        );
                    });
                });
            });

        ui.add_space(20.0);

        // Instructions
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Instructions").strong());
                ui.label("1. Select Left Click or Right Click to test");
                ui.label("2. Click Start to begin testing");
                ui.label("3. Click in the box above - it changes color when pressed");
                ui.label("4. Hold duration = time between press and release");
                ui.label("5. CPS = clicks per second (measured over last second)");
            });

        // Capture real mouse button input
        if self.response_running {
            let now = Instant::now();

            if has_bridge {
                // Raw input path: process all button events from the bridge
                for event in raw_events {
                    match &event.kind {
                        // Left click handling via raw input
                        RawInputKind::ButtonPress(RawButton::Left) => {
                            if !self.response_is_pressed {
                                self.response_is_pressed = true;
                                self.response_press_start = Some(event.timestamp);
                            }
                        }
                        RawInputKind::ButtonRelease(RawButton::Left) => {
                            if self.response_is_pressed {
                                self.response_is_pressed = false;
                                self.response_click_count += 1;
                                self.response_click_times.push_back(event.timestamp);

                                if let Some(start) = self.response_press_start {
                                    let hold_ms =
                                        event.timestamp.duration_since(start).as_secs_f64()
                                            * 1000.0;
                                    self.response_hold_times.push_back(hold_ms);
                                    if self.response_hold_times.len() > 100 {
                                        self.response_hold_times.pop_front();
                                    }
                                }
                                self.response_press_start = None;
                            }
                        }
                        // Right click handling via raw input
                        RawInputKind::ButtonPress(RawButton::Right) => {
                            if !self.response_right_is_pressed {
                                self.response_right_is_pressed = true;
                                self.response_right_press_start = Some(event.timestamp);
                            }
                        }
                        RawInputKind::ButtonRelease(RawButton::Right)
                            if self.response_right_is_pressed =>
                        {
                            self.response_right_is_pressed = false;
                            self.response_right_click_count += 1;
                            self.response_right_click_times.push_back(event.timestamp);

                            if let Some(start) = self.response_right_press_start {
                                let hold_ms =
                                    event.timestamp.duration_since(start).as_secs_f64() * 1000.0;
                                self.response_right_hold_times.push_back(hold_ms);
                                if self.response_right_hold_times.len() > 100 {
                                    self.response_right_hold_times.pop_front();
                                }
                            }
                            self.response_right_press_start = None;
                        }
                        _ => {}
                    }
                }
            } else {
                // Fallback: egui pointer input
                let pointer = ctx.input(|i| i.pointer.clone());
                let is_primary_down = pointer.primary_down();
                let is_secondary_down = pointer.secondary_down();

                // Only track clicks that start within the test area
                let in_test_area = pointer
                    .hover_pos()
                    .map(|pos| rect.contains(pos))
                    .unwrap_or(false);

                // Left click handling
                if in_test_area && is_primary_down && !self.response_is_pressed {
                    self.response_is_pressed = true;
                    self.response_press_start = Some(now);
                }

                if !is_primary_down && self.response_is_pressed {
                    self.response_is_pressed = false;
                    self.response_click_count += 1;
                    self.response_click_times.push_back(now);

                    if let Some(start) = self.response_press_start {
                        let hold_ms = now.duration_since(start).as_secs_f64() * 1000.0;
                        self.response_hold_times.push_back(hold_ms);
                        if self.response_hold_times.len() > 100 {
                            self.response_hold_times.pop_front();
                        }
                    }
                    self.response_press_start = None;
                }

                // Right click handling
                if in_test_area && is_secondary_down && !self.response_right_is_pressed {
                    self.response_right_is_pressed = true;
                    self.response_right_press_start = Some(now);
                }

                if !is_secondary_down && self.response_right_is_pressed {
                    self.response_right_is_pressed = false;
                    self.response_right_click_count += 1;
                    self.response_right_click_times.push_back(now);

                    if let Some(start) = self.response_right_press_start {
                        let hold_ms = now.duration_since(start).as_secs_f64() * 1000.0;
                        self.response_right_hold_times.push_back(hold_ms);
                        if self.response_right_hold_times.len() > 100 {
                            self.response_right_hold_times.pop_front();
                        }
                    }
                    self.response_right_press_start = None;
                }
            }

            // Prune old click times and update CPS for both buttons
            while let Some(front) = self.response_click_times.front() {
                if now.duration_since(*front).as_secs_f64() > 1.0 {
                    self.response_click_times.pop_front();
                } else {
                    break;
                }
            }
            while let Some(front) = self.response_right_click_times.front() {
                if now.duration_since(*front).as_secs_f64() > 1.0 {
                    self.response_right_click_times.pop_front();
                } else {
                    break;
                }
            }

            let one_sec_ago = now - std::time::Duration::from_secs(1);
            self.response_cps = self
                .response_click_times
                .iter()
                .filter(|t| **t > one_sec_ago)
                .count() as f64;
            self.response_right_cps = self
                .response_right_click_times
                .iter()
                .filter(|t| **t > one_sec_ago)
                .count() as f64;
        }
    }

    pub fn ui_sticky(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
    ) {
        let cfg = DiagConfig::default();
        let sticky_ms = cfg.sticky_threshold_ms;

        ui.heading("Click Stickiness Test");
        ui.add_space(5.0);
        ui.label(format!(
            "Tests for slow or stuck click releases. Clicks held longer than {:.0} ms are counted; it takes two or more to suggest a sticky switch.",
            sticky_ms
        ));
        ui.add_space(15.0);

        // Button selector
        ui.horizontal(|ui| {
            ui.label("Test Button:");
            ui.selectable_value(&mut self.sticky_button, TestButton::Left, "Left Click");
            ui.selectable_value(&mut self.sticky_button, TestButton::Right, "Right Click");
        });

        ui.add_space(10.0);

        // Controls
        ui.horizontal(|ui| {
            if self.sticky_running {
                if ui.button("Stop").clicked() {
                    self.sticky_running = false;
                }
            } else if ui.button("Start").clicked() {
                self.sticky_running = true;
                self.sticky_holds.clear();
                self.sticky_right_holds.clear();
                self.sticky_count = 0;
                self.sticky_right_count = 0;
                self.sticky_press_start = None;
                self.sticky_right_press_start = None;
                self.sticky_is_pressed = false;
                self.sticky_right_is_pressed = false;
            }

            if ui.button("Reset").clicked() {
                self.sticky_running = false;
                self.sticky_holds.clear();
                self.sticky_right_holds.clear();
                self.sticky_count = 0;
                self.sticky_right_count = 0;
                self.sticky_press_start = None;
                self.sticky_right_press_start = None;
                self.sticky_is_pressed = false;
                self.sticky_right_is_pressed = false;
            }
        });

        ui.add_space(20.0);

        // Test area
        let (rect, _response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 150.0),
            egui::Sense::hover(),
        );

        let is_pressed = match self.sticky_button {
            TestButton::Left => self.sticky_is_pressed,
            TestButton::Right => self.sticky_right_is_pressed,
        };

        let color = if is_pressed {
            match self.sticky_button {
                TestButton::Left => egui::Color32::from_rgb(50, 200, 50),
                TestButton::Right => egui::Color32::from_rgb(50, 150, 200),
            }
        } else if self.sticky_running {
            egui::Color32::from_rgb(60, 60, 80)
        } else {
            ui.visuals().faint_bg_color
        };

        ui.painter().rect_filled(rect, 8.0, color);

        let button_name = match self.sticky_button {
            TestButton::Left => "Left",
            TestButton::Right => "Right",
        };

        let text = if !self.sticky_running {
            "Click Start to begin".to_string()
        } else if is_pressed {
            "HELD".to_string()
        } else {
            format!("{} click rapidly to test", button_name)
        };

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(20.0),
            egui::Color32::WHITE,
        );

        ui.add_space(20.0);

        // Stats - show for selected button
        let (holds, sticky_count, press_start) = match self.sticky_button {
            TestButton::Left => (
                &self.sticky_holds,
                self.sticky_count,
                self.sticky_press_start,
            ),
            TestButton::Right => (
                &self.sticky_right_holds,
                self.sticky_right_count,
                self.sticky_right_press_start,
            ),
        };
        let current_hold_ms = press_start.filter(|_| is_pressed).map(|start| {
            Instant::now()
                .saturating_duration_since(start)
                .as_secs_f64()
                * 1000.0
        });
        let verdict = sticky_verdict(holds, current_hold_ms, &cfg);
        let slow = matches!(verdict, StickyVerdict::SlowReleases { .. });
        let warn_color = egui::Color32::ORANGE;

        egui::Frame::dark_canvas(ui.style())
            .inner_margin(15.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(format!("{} Clicks", button_name));
                        ui.label(
                            egui::RichText::new(format!("{}", holds.len()))
                                .size(20.0)
                                .strong(),
                        );
                    });
                    ui.add_space(30.0);
                    ui.vertical(|ui| {
                        ui.label(format!("Long holds (>{:.0} ms)", sticky_ms));
                        let color = match sticky_count {
                            0 => egui::Color32::GREEN,
                            1 => egui::Color32::WHITE,
                            _ => warn_color,
                        };
                        ui.label(
                            egui::RichText::new(format!("{}", sticky_count))
                                .size(20.0)
                                .color(color),
                        );
                    });
                    ui.add_space(30.0);
                    ui.vertical(|ui| {
                        ui.label("Avg Hold");
                        let avg = if holds.is_empty() {
                            0.0
                        } else {
                            holds.iter().sum::<f64>() / holds.len() as f64
                        };
                        ui.label(egui::RichText::new(format!("{:.1} ms", avg)).size(20.0));
                    });
                    ui.add_space(30.0);
                    ui.vertical(|ui| {
                        ui.label("Max Hold");
                        let max = holds.iter().cloned().fold(0.0, f64::max);
                        let color = if slow {
                            warn_color
                        } else {
                            egui::Color32::WHITE
                        };
                        ui.label(
                            egui::RichText::new(format!("{:.1} ms", max))
                                .size(20.0)
                                .color(color),
                        );
                    });
                });
            });

        ui.add_space(10.0);

        // Verdict for the selected button
        let (title, color, message) = match verdict {
            StickyVerdict::HeldDown { held_ms } => (
                "Button held down",
                egui::Color32::YELLOW,
                format!(
                    "The {} button has been down for {:.1} s. If you are not pressing it, the switch is stuck.",
                    button_name.to_lowercase(),
                    held_ms / 1000.0
                ),
            ),
            StickyVerdict::NeedMoreClicks { need } => (
                "Keep clicking",
                egui::Color32::LIGHT_GRAY,
                format!(
                    "Click quickly at least {} times to check the releases ({} so far).",
                    need,
                    holds.len()
                ),
            ),
            StickyVerdict::SlowReleases { count, longest_ms } => (
                "Slow or sticky releases",
                warn_color,
                format!(
                    "{} of {} clicks were held longer than {:.0} ms (longest {:.0} ms). If you were clicking quickly and not holding the button, the switch is releasing late.",
                    count,
                    holds.len(),
                    sticky_ms,
                    longest_ms
                ),
            ),
            StickyVerdict::OneLongHold { held_ms } => (
                "One long click",
                egui::Color32::WHITE,
                format!(
                    "One click was held for {:.0} ms. A single long hold is usually the hand, not the switch.",
                    held_ms
                ),
            ),
            StickyVerdict::NoSlowReleases => (
                "No slow releases",
                egui::Color32::GREEN,
                format!(
                    "None of {} clicks was held longer than {:.0} ms.",
                    holds.len(),
                    sticky_ms
                ),
            ),
        };
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(15.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new(title).size(18.0).strong().color(color));
                ui.add_space(5.0);
                ui.label(message);
            });

        // Capture real mouse button input
        if self.sticky_running {
            let now = Instant::now();

            if has_bridge {
                // Raw input path: process all button events from the bridge
                for event in raw_events {
                    match &event.kind {
                        // Left click handling via raw input
                        RawInputKind::ButtonPress(RawButton::Left) => {
                            if !self.sticky_is_pressed {
                                self.sticky_is_pressed = true;
                                self.sticky_press_start = Some(event.timestamp);
                            }
                        }
                        RawInputKind::ButtonRelease(RawButton::Left) => {
                            if self.sticky_is_pressed {
                                self.sticky_is_pressed = false;

                                if let Some(start) = self.sticky_press_start {
                                    let hold_ms =
                                        event.timestamp.duration_since(start).as_secs_f64()
                                            * 1000.0;
                                    self.sticky_holds.push(hold_ms);

                                    if hold_ms > sticky_ms {
                                        self.sticky_count += 1;
                                    }
                                }
                                self.sticky_press_start = None;
                            }
                        }
                        // Right click handling via raw input
                        RawInputKind::ButtonPress(RawButton::Right) => {
                            if !self.sticky_right_is_pressed {
                                self.sticky_right_is_pressed = true;
                                self.sticky_right_press_start = Some(event.timestamp);
                            }
                        }
                        RawInputKind::ButtonRelease(RawButton::Right)
                            if self.sticky_right_is_pressed =>
                        {
                            self.sticky_right_is_pressed = false;

                            if let Some(start) = self.sticky_right_press_start {
                                let hold_ms =
                                    event.timestamp.duration_since(start).as_secs_f64() * 1000.0;
                                self.sticky_right_holds.push(hold_ms);

                                if hold_ms > sticky_ms {
                                    self.sticky_right_count += 1;
                                }
                            }
                            self.sticky_right_press_start = None;
                        }
                        _ => {}
                    }
                }
            } else {
                // Fallback: egui pointer input
                let pointer = ctx.input(|i| i.pointer.clone());
                let is_primary_down = pointer.primary_down();
                let is_secondary_down = pointer.secondary_down();

                // Only track clicks that start within the test area
                let in_test_area = pointer
                    .hover_pos()
                    .map(|pos| rect.contains(pos))
                    .unwrap_or(false);

                // Left click handling
                if in_test_area && is_primary_down && !self.sticky_is_pressed {
                    self.sticky_is_pressed = true;
                    self.sticky_press_start = Some(now);
                }

                if !is_primary_down && self.sticky_is_pressed {
                    self.sticky_is_pressed = false;

                    if let Some(start) = self.sticky_press_start {
                        let hold_ms = now.duration_since(start).as_secs_f64() * 1000.0;
                        self.sticky_holds.push(hold_ms);

                        if hold_ms > sticky_ms {
                            self.sticky_count += 1;
                        }
                    }
                    self.sticky_press_start = None;
                }

                // Right click handling
                if in_test_area && is_secondary_down && !self.sticky_right_is_pressed {
                    self.sticky_right_is_pressed = true;
                    self.sticky_right_press_start = Some(now);
                }

                if !is_secondary_down && self.sticky_right_is_pressed {
                    self.sticky_right_is_pressed = false;

                    if let Some(start) = self.sticky_right_press_start {
                        let hold_ms = now.duration_since(start).as_secs_f64() * 1000.0;
                        self.sticky_right_holds.push(hold_ms);

                        if hold_ms > sticky_ms {
                            self.sticky_right_count += 1;
                        }
                    }
                    self.sticky_right_press_start = None;
                }
            }
        }
    }

    pub fn ui_liftoff(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
    ) {
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
            } else if ui.button("Start").clicked() {
                self.liftoff_running = true;
                self.liftoff_jumps = 0;
                self.liftoff_position = (0, 0);
                self.liftoff_jump_events.clear();
                self.liftoff_is_idle = false;
                self.liftoff_last_move = Instant::now();
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
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label("Position");
                        ui.label(
                            egui::RichText::new(format!(
                                "({}, {})",
                                self.liftoff_position.0, self.liftoff_position.1
                            ))
                            .size(20.0),
                        );
                    });
                    ui.add_space(50.0);
                    ui.vertical(|ui| {
                        ui.label("Jumps Detected");
                        let color = if self.liftoff_jumps == 0 {
                            egui::Color32::GREEN
                        } else {
                            egui::Color32::RED
                        };
                        ui.label(
                            egui::RichText::new(format!("{}", self.liftoff_jumps))
                                .size(24.0)
                                .color(color),
                        );
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
                        let color = if self.liftoff_is_idle {
                            egui::Color32::YELLOW
                        } else {
                            egui::Color32::WHITE
                        };
                        ui.label(egui::RichText::new(status).size(20.0).color(color));
                    });
                });
            });

        ui.add_space(20.0);

        // Jump history
        if !self.liftoff_jump_events.is_empty() {
            egui::Frame::dark_canvas(ui.style())
                .inner_margin(15.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Jump History").strong());
                    let avg: f64 = self.liftoff_jump_events.iter().sum::<f64>()
                        / self.liftoff_jump_events.len() as f64;
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
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Instructions").strong());
                ui.label("1. Start the test");
                ui.label("2. Move your mouse around");
                ui.label("3. Slowly lift the mouse off the surface");
                ui.label("4. Large cursor jumps during lift indicate high LOD");
                ui.add_space(5.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Jump threshold: {} px | Idle threshold: {} ms",
                        JUMP_THRESHOLD_PX, IDLE_THRESHOLD_MS
                    ))
                    .weak(),
                );
            });

        // Capture real mouse input and detect jumps
        if self.liftoff_running {
            let now = Instant::now();

            if has_bridge {
                // Raw input path: use Move events with their timestamps for idle detection
                for event in raw_events {
                    if let RawInputKind::Move { dx, dy } = &event.kind {
                        let distance = ((*dx as f64).powi(2) + (*dy as f64).powi(2)).sqrt();

                        // Check time gap since last movement for idle detection
                        let gap_ms = event
                            .timestamp
                            .duration_since(self.liftoff_last_move)
                            .as_millis() as u64;
                        let was_idle = gap_ms > IDLE_THRESHOLD_MS;

                        // Check for jump: large movement after being idle
                        if was_idle && distance > JUMP_THRESHOLD_PX {
                            self.liftoff_jumps += 1;
                            self.liftoff_jump_events.push(distance);
                        }

                        // Update position
                        self.liftoff_position.0 += *dx as i64;
                        self.liftoff_position.1 += *dy as i64;

                        // Reset idle state
                        self.liftoff_is_idle = false;
                        self.liftoff_last_move = event.timestamp;
                    }
                }

                // Check if currently idle (no move events recently)
                let time_since_move = now.duration_since(self.liftoff_last_move).as_millis() as u64;
                if time_since_move > IDLE_THRESHOLD_MS {
                    self.liftoff_is_idle = true;
                }
            } else {
                // Fallback: egui pointer delta
                let delta = ctx.input(|i| i.pointer.delta());

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
                    let time_since_move =
                        now.duration_since(self.liftoff_last_move).as_millis() as u64;
                    if time_since_move > IDLE_THRESHOLD_MS {
                        self.liftoff_is_idle = true;
                    }
                }
            }
        }
    }

    pub fn export_response(&self) -> Option<ClickResponseExport> {
        if self.response_click_count == 0 && self.response_right_click_count == 0 {
            return None;
        }
        Some(ClickResponseExport {
            left: ClickButtonExport {
                click_count: self.response_click_count,
                cps: self.response_cps,
                avg_hold_ms: if self.response_hold_times.is_empty() {
                    0.0
                } else {
                    self.response_hold_times.iter().sum::<f64>()
                        / self.response_hold_times.len() as f64
                },
                min_hold_ms: min_or_zero(self.response_hold_times.iter().cloned()),
                max_hold_ms: self.response_hold_times.iter().cloned().fold(0.0, f64::max),
                hold_times: self.response_hold_times.iter().cloned().collect(),
            },
            right: ClickButtonExport {
                click_count: self.response_right_click_count,
                cps: self.response_right_cps,
                avg_hold_ms: if self.response_right_hold_times.is_empty() {
                    0.0
                } else {
                    self.response_right_hold_times.iter().sum::<f64>()
                        / self.response_right_hold_times.len() as f64
                },
                min_hold_ms: min_or_zero(self.response_right_hold_times.iter().cloned()),
                max_hold_ms: self
                    .response_right_hold_times
                    .iter()
                    .cloned()
                    .fold(0.0, f64::max),
                hold_times: self.response_right_hold_times.iter().cloned().collect(),
            },
        })
    }

    pub fn export_sticky(&self) -> Option<ClickStickyExport> {
        if self.sticky_holds.is_empty() && self.sticky_right_holds.is_empty() {
            return None;
        }
        Some(ClickStickyExport {
            left: StickyButtonExport {
                click_count: self.sticky_holds.len(),
                sticky_count: self.sticky_count,
                avg_hold_ms: if self.sticky_holds.is_empty() {
                    0.0
                } else {
                    self.sticky_holds.iter().sum::<f64>() / self.sticky_holds.len() as f64
                },
                max_hold_ms: self.sticky_holds.iter().cloned().fold(0.0, f64::max),
                hold_times: self.sticky_holds.clone(),
            },
            right: StickyButtonExport {
                click_count: self.sticky_right_holds.len(),
                sticky_count: self.sticky_right_count,
                avg_hold_ms: if self.sticky_right_holds.is_empty() {
                    0.0
                } else {
                    self.sticky_right_holds.iter().sum::<f64>()
                        / self.sticky_right_holds.len() as f64
                },
                max_hold_ms: self.sticky_right_holds.iter().cloned().fold(0.0, f64::max),
                hold_times: self.sticky_right_holds.clone(),
            },
        })
    }

    pub fn export_liftoff(&self) -> Option<LiftOffExport> {
        if self.liftoff_jump_events.is_empty() && self.liftoff_jumps == 0 {
            return None;
        }
        let avg = if self.liftoff_jump_events.is_empty() {
            0.0
        } else {
            self.liftoff_jump_events.iter().sum::<f64>() / self.liftoff_jump_events.len() as f64
        };
        let max = self.liftoff_jump_events.iter().cloned().fold(0.0, f64::max);
        Some(LiftOffExport {
            jump_count: self.liftoff_jumps,
            avg_distance_px: avg,
            max_distance_px: max,
            jump_distances: self.liftoff_jump_events.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Run the stickiness panel over left clicks with the given hold times
    /// (ms), 300 ms apart, through the raw-input path.
    fn sticky_panel_with_holds(holds_ms: &[u64]) -> ClickPanel {
        let ctx = egui::Context::default();
        let mut panel = ClickPanel::new();
        panel.sticky_running = true;
        let mut t = Instant::now();
        let mut events = Vec::new();
        for &hold in holds_ms {
            events.push(RawInputEvent {
                kind: RawInputKind::ButtonPress(RawButton::Left),
                timestamp: t,
            });
            events.push(RawInputEvent {
                kind: RawInputKind::ButtonRelease(RawButton::Left),
                timestamp: t + Duration::from_millis(hold),
            });
            t += Duration::from_millis(hold + 300);
        }
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx().clone();
            panel.ui_sticky(ui, &ctx, &events, true);
        });
        panel
    }

    #[test]
    fn deliberate_clicks_are_not_sticky() {
        // A person clicking deliberately holds ~80-150 ms
        let panel = sticky_panel_with_holds(&[90, 110, 150, 120, 100, 140]);
        let export = panel.export_sticky().unwrap();
        assert_eq!(export.left.click_count, 6);
        assert_eq!(export.left.sticky_count, 0);
        assert_eq!(
            sticky_verdict(&panel.sticky_holds, None, &DiagConfig::default()),
            StickyVerdict::NoSlowReleases
        );
    }

    #[test]
    fn switch_releasing_late_is_flagged() {
        // Normal finger holds, but the switch keeps 3 of them closed 250+ ms longer
        let panel = sticky_panel_with_holds(&[90, 380, 110, 420, 100, 350, 120]);
        let export = panel.export_sticky().unwrap();
        assert_eq!(export.left.sticky_count, 3);
        assert!(matches!(
            sticky_verdict(&panel.sticky_holds, None, &DiagConfig::default()),
            StickyVerdict::SlowReleases { count: 3, .. }
        ));
    }

    #[test]
    fn one_long_click_is_not_called_sticky() {
        let cfg = DiagConfig::default();
        let holds = [100.0, 120.0, 400.0, 90.0, 110.0, 130.0];
        assert_eq!(
            sticky_verdict(&holds, None, &cfg),
            StickyVerdict::OneLongHold { held_ms: 400.0 }
        );
        // Exactly at the threshold is still a normal click
        let holds = [250.0, 250.0, 100.0, 100.0, 100.0];
        assert_eq!(
            sticky_verdict(&holds, None, &cfg),
            StickyVerdict::NoSlowReleases
        );
    }

    #[test]
    fn too_few_clicks_are_inconclusive() {
        let cfg = DiagConfig::default();
        assert_eq!(
            sticky_verdict(&[400.0, 500.0], None, &cfg),
            StickyVerdict::NeedMoreClicks { need: 5 }
        );
        assert_eq!(
            sticky_verdict(&[], None, &cfg),
            StickyVerdict::NeedMoreClicks { need: 5 }
        );
    }

    #[test]
    fn button_down_past_the_stuck_threshold_is_reported() {
        let cfg = DiagConfig::default();
        let holds = [100.0; 6];
        assert_eq!(
            sticky_verdict(&holds, Some(1500.0), &cfg),
            StickyVerdict::HeldDown { held_ms: 1500.0 }
        );
        // A press in progress that is not yet long says nothing
        assert_eq!(
            sticky_verdict(&holds, Some(400.0), &cfg),
            StickyVerdict::NoSlowReleases
        );
    }

    #[test]
    fn thresholds_match_auto_diagnostics() {
        let cfg = DiagConfig::default();
        assert_eq!(cfg.sticky_threshold_ms, 250.0);
        assert_eq!(cfg.clicks_min_left, 5);
    }
}
