use eframe::egui;
use std::collections::VecDeque;
use std::time::Instant;

use super::polling::raw_input_required_notice;
use crate::export::{
    ClickButtonExport, ClickResponseExport, ClickStickyExport, LiftOffExport, StickyButtonExport,
};
use crate::input_bridge::{RawButton, RawInputEvent, RawInputKind};
use mouse_testkit::analysis::diagnostics::Config as DiagConfig;
use mouse_testkit::analysis::liftoff::{
    self, counts_to_mm, mm_to_counts, Burst, LiftDetector, LiftTally, LiftVerdict,
    JUMP_THRESHOLD_MM, JUMP_WINDOW_MS, MIN_LIFTS,
};

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

    // Lift Off (judged by `analysis::liftoff`, like the CLI test and Auto
    // Diagnostics)
    liftoff_running: bool,
    /// The mouse's DPI from the DPI Accuracy page, if set there.
    liftoff_dpi_setting: Option<f64>,
    /// DPI the current or last run judges with, and whether it was assumed.
    liftoff_dpi: (f64, bool),
    liftoff_detector: LiftDetector,
    liftoff_tally: LiftTally,
    /// SPACE presses: the lifts the user says they made. A clean lift
    /// produces no motion, so only these show that lifting was tested.
    liftoff_lifts: usize,
    /// Start of the current run; event times are seconds after it, and
    /// events from before it are ignored.
    liftoff_epoch: Instant,
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
            liftoff_dpi_setting: None,
            liftoff_dpi: liftoff::dpi_or_default(None),
            liftoff_detector: LiftDetector::new(liftoff::DEFAULT_DPI),
            liftoff_tally: LiftTally::default(),
            liftoff_lifts: 0,
            liftoff_epoch: Instant::now(),
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
        // A press still open after Stop never gets its release, so it says
        // nothing about the switch.
        let current_hold_ms = press_start
            .filter(|_| is_pressed && self.sticky_running)
            .map(|start| {
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

            // Only presses that start in the test area count. Raw input sees
            // every click, including the Stop button and the sidebar, whose
            // release this panel never gets once it stops or is left: the
            // press would stay open and later read as a long hold.
            let in_test_area = ctx
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|pos| rect.contains(pos));

            if has_bridge {
                // Raw input path: process all button events from the bridge
                for event in raw_events {
                    match &event.kind {
                        // Left click handling via raw input
                        RawInputKind::ButtonPress(RawButton::Left) if in_test_area => {
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
                        RawInputKind::ButtonPress(RawButton::Right) if in_test_area => {
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

    /// Set the mouse's DPI for the lift-off test: the value from the DPI
    /// Accuracy page, or `None` when it was not set there (800 is then
    /// assumed and the panel says so). Takes effect at the next Start.
    pub fn set_liftoff_dpi(&mut self, dpi: Option<f64>) {
        self.liftoff_dpi_setting = dpi;
    }

    fn start_liftoff(&mut self) {
        self.liftoff_dpi = liftoff::dpi_or_default(self.liftoff_dpi_setting);
        // The hand has just clicked Start: motion is judged only once the
        // mouse has been seen still.
        self.liftoff_detector = LiftDetector::new(self.liftoff_dpi.0).watching_from(0.0);
        self.liftoff_tally = LiftTally::default();
        self.liftoff_lifts = 0;
        self.liftoff_epoch = Instant::now();
        self.liftoff_running = true;
    }

    /// End the run; motion still under way is judged as it stands, as in
    /// the CLI test.
    fn stop_liftoff(&mut self) {
        let burst = self.liftoff_detector.flush();
        self.record_liftoff(burst);
        self.liftoff_running = false;
    }

    fn reset_liftoff(&mut self) {
        self.liftoff_running = false;
        self.liftoff_detector = LiftDetector::new(self.liftoff_dpi.0);
        self.liftoff_tally = LiftTally::default();
        self.liftoff_lifts = 0;
    }

    fn record_liftoff(&mut self, burst: Option<Burst>) {
        if let Some(burst) = burst {
            self.liftoff_tally.record(&burst, self.liftoff_dpi.0);
        }
    }

    /// Feed raw motion reports (from the bridge) and advance to `now`.
    fn feed_liftoff(&mut self, raw_events: &[RawInputEvent], now: Instant) {
        let epoch = self.liftoff_epoch;
        let secs = |t: Instant| t.duration_since(epoch).as_secs_f64();
        for event in raw_events {
            if let RawInputKind::Move { dx, dy } = event.kind {
                if event.timestamp < epoch {
                    continue;
                }
                let burst = self.liftoff_detector.feed(secs(event.timestamp), dx, dy);
                self.record_liftoff(burst);
            }
        }
        let burst = self.liftoff_detector.tick(secs(now.max(epoch)));
        self.record_liftoff(burst);
    }

    fn liftoff_verdict(&self) -> LiftVerdict {
        liftoff::verdict(
            self.liftoff_tally.jumps.len(),
            self.liftoff_lifts,
            MIN_LIFTS,
        )
    }

    pub fn ui_liftoff(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
    ) {
        // Input first, before any button is drawn: SPACE is consumed so it
        // cannot "click" a focused button such as Stop.
        if self.liftoff_running {
            if has_bridge {
                self.liftoff_lifts += take_space_presses(ctx);
                self.feed_liftoff(raw_events, Instant::now());
            } else {
                // The measured mouse went away: keep what was measured.
                self.stop_liftoff();
            }
        }

        let (dpi, dpi_assumed) = if self.liftoff_running || self.has_liftoff_data() {
            self.liftoff_dpi
        } else {
            liftoff::dpi_or_default(self.liftoff_dpi_setting)
        };

        ui.heading("Lift-Off Jump Test");
        ui.add_space(5.0);
        ui.label("Detects cursor jumps when the mouse is lifted or set down.");
        if !has_bridge {
            ui.add_space(5.0);
            raw_input_required_notice(
                ui,
                "A lift-off jump (sensor counts summed over the first 100 ms of motion)",
            );
        }
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(if dpi_assumed {
                format!(
                    "Distances assume {:.0} DPI. Set your mouse's DPI on the DPI Accuracy page if it differs: otherwise they are off by the same factor.",
                    dpi
                )
            } else {
                format!("Distances use {:.0} DPI (set on the DPI Accuracy page).", dpi)
            })
            .weak(),
        );
        ui.add_space(10.0);

        // Controls
        ui.horizontal(|ui| {
            if self.liftoff_running {
                if ui.button("Stop").clicked() {
                    self.stop_liftoff();
                }
            } else if ui
                .add_enabled(has_bridge, egui::Button::new("Start"))
                .clicked()
            {
                self.start_liftoff();
            }

            if ui.button("Reset").clicked() {
                self.reset_liftoff();
            }
        });

        ui.add_space(20.0);

        // Stats
        let tally = &self.liftoff_tally;
        let jumps = tally.jumps.len();
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(20.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label("Lifts (SPACE)");
                        ui.label(
                            egui::RichText::new(format!("{} / {}", self.liftoff_lifts, MIN_LIFTS))
                                .size(20.0),
                        );
                    });
                    ui.add_space(40.0);
                    ui.vertical(|ui| {
                        ui.label("Jumps Detected");
                        let color = match jumps {
                            0 => egui::Color32::WHITE,
                            1 => egui::Color32::YELLOW,
                            _ => egui::Color32::RED,
                        };
                        ui.label(
                            egui::RichText::new(format!("{}", jumps))
                                .size(24.0)
                                .color(color),
                        );
                    });
                    ui.add_space(40.0);
                    ui.vertical(|ui| {
                        ui.label("Largest lift/set-down");
                        ui.label(
                            egui::RichText::new(format!(
                                "{:.1} mm",
                                counts_to_mm(tally.largest_counts, dpi)
                            ))
                            .size(20.0),
                        );
                    });
                    ui.add_space(40.0);
                    ui.vertical(|ui| {
                        ui.label("Slides ignored");
                        ui.label(egui::RichText::new(format!("{}", tally.slides)).size(20.0));
                    });
                    ui.add_space(40.0);
                    ui.vertical(|ui| {
                        ui.label("Status");
                        let status = if !self.liftoff_running {
                            "Stopped"
                        } else if self.liftoff_detector.in_burst() {
                            "Moving"
                        } else {
                            "Still"
                        };
                        ui.label(egui::RichText::new(status).size(20.0));
                    });
                });
            });

        ui.add_space(10.0);

        // Verdict: the same rule as the CLI test
        if self.liftoff_running || self.has_liftoff_data() {
            let (title, color, message) = match self.liftoff_verdict() {
                LiftVerdict::Jumps => (
                    "Cursor jumps on lift",
                    egui::Color32::RED,
                    "The cursor jumps when the mouse is lifted or set down. Lower the lift-off distance (LOD) in the mouse software, or try a plainer surface.".to_string(),
                ),
                LiftVerdict::OneJump => (
                    "One jump",
                    egui::Color32::YELLOW,
                    "A single jump is often the hand sliding the mouse on set-down. Repeat the test: a second jump points to a high lift-off distance.".to_string(),
                ),
                LiftVerdict::TooFewLifts => (
                    "Not enough lifts yet",
                    egui::Color32::LIGHT_GRAY,
                    format!(
                        "No jumps so far, but only {} lift(s) recorded. Lift and set down the mouse at least {} times, pressing SPACE after each: a clean lift moves nothing, so the lifts must be counted.",
                        self.liftoff_lifts, MIN_LIFTS
                    ),
                ),
                LiftVerdict::Clean => (
                    "No jumps",
                    egui::Color32::GREEN,
                    format!(
                        "No jumps in {} lifts. Lift-off behaviour appears normal.",
                        self.liftoff_lifts
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
                    if dpi_assumed && jumps > 0 {
                        // At a higher DPI the same counts are a shorter
                        // distance: 2 mm at 800 DPI is 0.5 mm at 3200.
                        ui.add_space(5.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "These distances assume {:.0} DPI. If your mouse is set higher, the jumps are shorter than shown and may be ordinary set-down shifts: set its DPI on the DPI Accuracy page and repeat the test.",
                                dpi
                            ))
                            .color(egui::Color32::YELLOW),
                        );
                    }
                    if tally.slides > 0 {
                        ui.add_space(5.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "{} longer movement(s) were treated as sliding and not judged. If the mouse was in the air at those moments, the sensor is tracking while lifted (lift-off distance too high).",
                                tally.slides
                            ))
                            .weak(),
                        );
                    }
                });
            ui.add_space(10.0);
        }

        // Jump history
        if !tally.jumps.is_empty() {
            egui::Frame::dark_canvas(ui.style())
                .inner_margin(15.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Jump History").strong());
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "Average: {:.1} mm",
                            counts_to_mm(tally.avg_jump_counts(), dpi)
                        ));
                        ui.add_space(20.0);
                        ui.label(format!(
                            "Max: {:.1} mm",
                            counts_to_mm(tally.max_jump_counts(), dpi)
                        ));
                    });
                    ui.collapsing("All jumps", |ui| {
                        for (i, jump) in tally.jumps.iter().enumerate() {
                            ui.label(format!(
                                "#{}: {:.1} mm (dx {:+}, dy {:+} counts) within {:.0} ms",
                                i + 1,
                                counts_to_mm(jump.peak_counts, dpi),
                                jump.peak_dx,
                                jump.peak_dy,
                                jump.peak_ms
                            ));
                        }
                    });
                });

            ui.add_space(10.0);
        }

        // Instructions
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Instructions").strong());
                ui.label("1. Rest the mouse on the pad and click Start, then keep it still");
                ui.label("2. Lift it straight up about 1 cm, hold it there a moment, then set it straight back down - try not to slide it");
                ui.label("3. Press SPACE (with your other hand) after each lift and set-down, so the test knows how many lifts you made");
                ui.label(format!(
                    "4. Wait about a second and repeat - at least {} times - then click Stop",
                    MIN_LIFTS
                ));
                ui.add_space(5.0);
                ui.label(
                    egui::RichText::new(format!(
                        "A jump: motion that starts from stillness and carries the cursor more than {:.1} mm ({:.0} counts at {:.0} DPI) within {:.0} ms. Longer sliding movements are not judged.",
                        JUMP_THRESHOLD_MM,
                        mm_to_counts(JUMP_THRESHOLD_MM, dpi),
                        dpi,
                        JUMP_WINDOW_MS
                    ))
                    .weak(),
                );
            });
    }

    fn has_liftoff_data(&self) -> bool {
        self.liftoff_lifts > 0 || self.liftoff_tally.bursts() > 0
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
        if !self.has_liftoff_data() {
            return None;
        }
        let (dpi, dpi_assumed) = self.liftoff_dpi;
        let tally = &self.liftoff_tally;
        let counts: Vec<f64> = tally.jumps.iter().map(|j| j.peak_counts).collect();
        Some(LiftOffExport {
            jump_count: tally.jumps.len(),
            avg_distance_px: tally.avg_jump_counts(),
            max_distance_px: tally.max_jump_counts(),
            jump_distances_mm: counts.iter().map(|&c| counts_to_mm(c, dpi)).collect(),
            jump_distances: counts,
            max_distance_mm: counts_to_mm(tally.max_jump_counts(), dpi),
            lifts_recorded: self.liftoff_lifts,
            slides_ignored: tally.slides,
            dpi,
            dpi_assumed,
            jump_threshold_mm: JUMP_THRESHOLD_MM,
            verdict: self.liftoff_verdict().label().to_string(),
        })
    }
}

/// SPACE presses this frame (key repeat ignored). Every SPACE event is
/// consumed, so it cannot activate a focused button.
fn take_space_presses(ctx: &egui::Context) -> usize {
    ctx.input_mut(|i| {
        let mut presses = 0;
        i.events.retain(|e| match e {
            egui::Event::Key {
                key: egui::Key::Space,
                pressed,
                repeat,
                ..
            } => {
                presses += usize::from(*pressed && !*repeat);
                false
            }
            _ => true,
        });
        presses
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Pointer positions inside and outside (on the heading) the
    /// stickiness test area, in a 1280x900 window.
    const IN_TEST_AREA: egui::Pos2 = egui::pos2(400.0, 230.0);
    const ON_HEADING: egui::Pos2 = egui::pos2(60.0, 10.0);

    /// One frame of the stickiness panel with the pointer at `pointer`,
    /// fed `events` through the raw-input path.
    fn sticky_frame(panel: &mut ClickPanel, pointer: egui::Pos2, events: &[RawInputEvent]) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 900.0),
            )),
            events: vec![egui::Event::PointerMoved(pointer)],
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            panel.ui_sticky(ui, &ctx, events, true);
        });
    }

    /// Raw left press/release pairs with the given hold times (ms), 300 ms
    /// apart.
    fn left_clicks(holds_ms: &[u64]) -> Vec<RawInputEvent> {
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
        events
    }

    /// Run the stickiness panel over left clicks in the test area with the
    /// given hold times (ms), through the raw-input path.
    fn sticky_panel_with_holds(holds_ms: &[u64]) -> ClickPanel {
        let mut panel = ClickPanel::new();
        panel.sticky_running = true;
        sticky_frame(&mut panel, IN_TEST_AREA, &left_clicks(holds_ms));
        panel
    }

    #[test]
    fn clicks_outside_the_test_area_are_not_holds() {
        // Raw input sees every click. A press on the sidebar or the Stop
        // button whose release arrives after leaving the panel (5 s later)
        // must not become a 5 s hold or a button left held down.
        let mut panel = ClickPanel::new();
        panel.sticky_running = true;
        let events = left_clicks(&[5000]);
        sticky_frame(&mut panel, ON_HEADING, &events[..1]);
        assert!(!panel.sticky_is_pressed);
        sticky_frame(&mut panel, IN_TEST_AREA, &events[1..]);
        assert!(panel.sticky_holds.is_empty());
        assert_eq!(panel.sticky_count, 0);
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

    /// One frame of the lift-off panel with raw `events`. With `space`,
    /// SPACE is pressed, held long enough to auto-repeat, and released.
    fn liftoff_frame(
        panel: &mut ClickPanel,
        events: &[RawInputEvent],
        space: bool,
        has_bridge: bool,
    ) {
        let ctx = egui::Context::default();
        let key = |pressed, repeat| egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed,
            repeat,
            modifiers: egui::Modifiers::NONE,
        };
        let keys = if space {
            vec![key(true, false), key(true, true), key(false, false)]
        } else {
            Vec::new()
        };
        let input = egui::RawInput {
            events: keys,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            panel.ui_liftoff(ui, &ctx, events, has_bridge);
        });
    }

    /// `n` raw motion reports of `(dx, dy)`, `every_ms` apart, starting
    /// `at_ms` after `start`.
    fn reports(
        start: Instant,
        at_ms: u64,
        n: usize,
        every_ms: u64,
        dx: i32,
        dy: i32,
    ) -> Vec<RawInputEvent> {
        (0..n as u64)
            .map(|i| RawInputEvent {
                kind: RawInputKind::Move { dx, dy },
                timestamp: start + Duration::from_millis(at_ms + i * every_ms),
            })
            .collect()
    }

    #[test]
    fn liftoff_finds_jumps_spread_over_many_reports() {
        // Two set-down jumps of 200 counts (6.4 mm at 800 DPI), each sent
        // at 1000 Hz as 10 counts per report: no single report is large.
        let mut panel = ClickPanel::new();
        panel.start_liftoff();
        let t0 = panel.liftoff_epoch;
        let mut events = reports(t0, 500, 20, 1, 10, 0);
        events.extend(reports(t0, 1500, 20, 1, 0, -10));
        liftoff_frame(&mut panel, &events, true, true);
        panel.stop_liftoff();
        assert_eq!(panel.liftoff_tally.jumps.len(), 2);
        assert_eq!(panel.liftoff_verdict(), LiftVerdict::Jumps);
        let export = panel.export_liftoff().unwrap();
        assert_eq!(export.jump_count, 2);
        assert!((export.max_distance_mm - 6.35).abs() < 0.01);
        assert!(export.dpi_assumed);
        assert_eq!(export.verdict, "Cursor jumps on lift");
    }

    #[test]
    fn liftoff_normal_starts_at_125hz_and_high_dpi_are_not_jumps() {
        // 1600 DPI (set on the DPI page), 125 Hz: an ordinary movement after
        // a pause starts with a 60-count report. Five lifts were made and
        // set down cleanly in between.
        let mut panel = ClickPanel::new();
        panel.set_liftoff_dpi(Some(1600.0));
        panel.start_liftoff();
        let t0 = panel.liftoff_epoch;
        let mut events = Vec::new();
        for k in 0..3 {
            events.extend(reports(t0, 500 + k * 1500, 60, 8, 60, 5));
        }
        for k in 0..5 {
            // A set-down shifts the mouse 0.3 mm
            events.extend(reports(t0, 5000 + k * 1000, 4, 2, 5, 0));
        }
        for _ in 0..MIN_LIFTS {
            liftoff_frame(&mut panel, &[], true, true);
        }
        liftoff_frame(&mut panel, &events, false, true);
        panel.stop_liftoff();
        let tally = &panel.liftoff_tally;
        assert_eq!((tally.jumps.len(), tally.slides, tally.quiet), (0, 3, 5));
        assert_eq!(panel.liftoff_verdict(), LiftVerdict::Clean);
        let export = panel.export_liftoff().unwrap();
        assert_eq!((export.dpi, export.dpi_assumed), (1600.0, false));
        assert_eq!(export.lifts_recorded, MIN_LIFTS);
    }

    #[test]
    fn liftoff_without_lifts_is_not_called_clean() {
        // Started and stopped without lifting: nothing measured.
        let mut panel = ClickPanel::new();
        panel.start_liftoff();
        liftoff_frame(&mut panel, &[], false, true);
        panel.stop_liftoff();
        assert_eq!(panel.liftoff_verdict(), LiftVerdict::TooFewLifts);
        assert!(panel.export_liftoff().is_none());

        // Quiet set-downs but SPACE never pressed: still not a clean result.
        let mut panel = ClickPanel::new();
        panel.start_liftoff();
        let t0 = panel.liftoff_epoch;
        let mut events = reports(t0, 500, 4, 2, 5, 0);
        events.extend(reports(t0, 1500, 4, 2, -5, 0));
        liftoff_frame(&mut panel, &events, false, true);
        panel.stop_liftoff();
        assert_eq!(panel.liftoff_tally.quiet, 2);
        assert_eq!(panel.liftoff_verdict(), LiftVerdict::TooFewLifts);
        let export = panel.export_liftoff().unwrap();
        assert_eq!(export.verdict, "Inconclusive - too few lifts");
    }

    #[test]
    fn space_counts_lifts_without_key_repeat_or_stopping_the_test() {
        let mut panel = ClickPanel::new();
        panel.start_liftoff();
        for _ in 0..3 {
            liftoff_frame(&mut panel, &[], true, true);
        }
        assert_eq!(panel.liftoff_lifts, 3);
        assert!(panel.liftoff_running);
        // Not counted while stopped
        panel.stop_liftoff();
        liftoff_frame(&mut panel, &[], true, true);
        assert_eq!(panel.liftoff_lifts, 3);
    }

    #[test]
    fn liftoff_needs_raw_input() {
        // Without raw input the run stops instead of judging accelerated,
        // per-frame pointer motion; what was measured is kept.
        let mut panel = ClickPanel::new();
        panel.start_liftoff();
        liftoff_frame(&mut panel, &[], true, true);
        liftoff_frame(&mut panel, &[], true, false);
        assert!(!panel.liftoff_running);
        assert_eq!(panel.liftoff_lifts, 1);
    }

    #[test]
    fn thresholds_match_auto_diagnostics() {
        let cfg = DiagConfig::default();
        assert_eq!(cfg.sticky_threshold_ms, 250.0);
        assert_eq!(cfg.clicks_min_left, 5);
    }
}
