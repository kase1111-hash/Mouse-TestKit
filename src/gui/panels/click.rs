use eframe::egui;
use std::collections::VecDeque;
use std::time::Instant;

use super::polling::{input_lost_notice, raw_input_required_notice};
use crate::export::{
    ClickButtonExport, ClickResponseExport, ClickStickyExport, LiftOffExport, StickyButtonExport,
};
use crate::input_bridge::{RawButton, RawInputEvent, RawInputKind};
use mouse_testkit::analysis::diagnostics::Config as DiagConfig;
use mouse_testkit::analysis::liftoff::{
    self, counts_to_mm, mm_to_counts, Burst, LiftDetector, LiftState, LiftTally, LiftVerdict,
    JUMP_THRESHOLD_MM, JUMP_WINDOW_MS, MIN_LIFTS,
};

/// When the lift-off test is stopped by clicking Stop, motion that ended
/// this recently (s) was the hand taking the measured mouse to the button,
/// not a lift (the acceleration test uses the same window). Enter or Esc
/// stop the test without moving the mouse.
const LIFTOFF_STOP_TRIP_S: f64 = 0.6;

/// How a lift-off run was stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiftoffStop {
    /// From the keyboard: the mouse did not move to stop it.
    Key,
    /// With the Stop button, which the measured mouse moved to.
    Button,
    /// The measured input changed (the mouse went away, or another input
    /// was chosen): motion still under way was cut off.
    InputChanged,
}

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

impl StickyVerdict {
    /// The conclusion in one line, for exports.
    fn summary(self) -> String {
        match self {
            StickyVerdict::HeldDown { .. } => "Button held down".to_string(),
            StickyVerdict::NeedMoreClicks { need } => {
                format!("Inconclusive: fewer than {} clicks", need)
            }
            StickyVerdict::SlowReleases { .. } => "Slow or sticky releases".to_string(),
            StickyVerdict::OneLongHold { .. } => {
                "One long click, usually the hand, not the switch".to_string()
            }
            StickyVerdict::NoSlowReleases => "No slow releases".to_string(),
        }
    }
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

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum TestButton {
    Left,
    Right,
}

/// One left or right button press or release.
#[derive(Debug, Clone, Copy)]
struct ButtonEdge {
    button: TestButton,
    pressed: bool,
    time: Instant,
    /// Whether the pointer was over the test area: only presses there count.
    in_area: bool,
}

/// A button press or release, or a gap in the input.
#[derive(Debug, Clone, Copy)]
enum ButtonInput {
    Edge(ButtonEdge),
    /// Reports were lost here (see `RawInputEvent::lost_before`): a release
    /// can be among them, or restored after the gap with the time of the
    /// drop, so no press is timed across it.
    Gap,
}

/// This frame's left and right button presses and releases. Raw input
/// carries each report's own time, and marks where reports were lost.
/// Without it, egui's button events are used, all of them even when a quick
/// click's press and release fall in one frame, stamped with the frame time
/// `now`. Raw input sees clicks anywhere (the sidebar, the panel's own
/// buttons), so presses are marked with whether the pointer was over `area`.
fn button_edges(
    ctx: &egui::Context,
    raw_events: &[RawInputEvent],
    has_bridge: bool,
    area: egui::Rect,
    now: Instant,
) -> Vec<ButtonInput> {
    if has_bridge {
        let in_area = ctx
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|pos| area.contains(pos));
        let mut out = Vec::new();
        for e in raw_events {
            if e.lost_before {
                out.push(ButtonInput::Gap);
            }
            let (button, pressed) = match e.kind {
                RawInputKind::ButtonPress(RawButton::Left) => (TestButton::Left, true),
                RawInputKind::ButtonRelease(RawButton::Left) => (TestButton::Left, false),
                RawInputKind::ButtonPress(RawButton::Right) => (TestButton::Right, true),
                RawInputKind::ButtonRelease(RawButton::Right) => (TestButton::Right, false),
                _ => continue,
            };
            out.push(ButtonInput::Edge(ButtonEdge {
                button,
                pressed,
                time: e.timestamp,
                in_area,
            }));
        }
        out
    } else {
        ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::PointerButton {
                        pos,
                        button,
                        pressed,
                        ..
                    } => {
                        let button = match button {
                            egui::PointerButton::Primary => TestButton::Left,
                            egui::PointerButton::Secondary => TestButton::Right,
                            _ => return None,
                        };
                        Some(ButtonInput::Edge(ButtonEdge {
                            button,
                            pressed: *pressed,
                            time: now,
                            in_area: area.contains(*pos),
                        }))
                    }
                    _ => None,
                })
                .collect()
        })
    }
}

/// Apply a press or release to one button's open press. A press counts only
/// in the test area and while no press is open; a release closes the open
/// press wherever it happens. Returns the hold (ms) a release completes.
fn track_hold(pressed_at: &mut Option<Instant>, edge: &ButtonEdge) -> Option<f64> {
    if edge.pressed {
        if pressed_at.is_none() && edge.in_area {
            *pressed_at = Some(edge.time);
        }
        None
    } else {
        pressed_at
            .take()
            .map(|start| edge.time.saturating_duration_since(start).as_secs_f64() * 1000.0)
    }
}

/// Shown on the click panels without raw input.
const FRAME_TIMED_NOTE: &str = "Raw input unavailable: presses and releases are only known to the display frame (about 16 ms), so hold times are rounded to frames and the shortest hold is not shown.";

pub struct ClickPanel {
    // Click Response - real click testing
    response_running: bool,
    /// Whether the current or last Click Test run read raw input.
    response_raw: bool,
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
    /// Whether the current or last stickiness run read raw input.
    sticky_raw: bool,
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
    /// Bursts that ended less than [`LIFTOFF_STOP_TRIP_S`] ago, kept out of
    /// `liftoff_tally` until then: if Stop is clicked first, they were the
    /// trip to the button.
    liftoff_pending: Vec<Burst>,
    /// The run was ended because the measured input changed.
    liftoff_stopped_for_input: bool,
}

impl ClickPanel {
    /// End the Click Test run. A press still open never gets its release
    /// here, so it is dropped.
    fn stop_response(&mut self) {
        self.response_running = false;
        self.response_press_start = None;
        self.response_right_press_start = None;
        self.response_is_pressed = false;
        self.response_right_is_pressed = false;
    }

    pub fn new() -> Self {
        Self {
            response_running: false,
            response_raw: true,
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
            sticky_raw: true,
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
            liftoff_pending: Vec::new(),
            liftoff_stopped_for_input: false,
        }
    }

    pub fn is_running(&self) -> bool {
        self.response_running || self.sticky_running || self.liftoff_running
    }

    /// The measured input changed (another device, or raw input stopped or
    /// started): every run in progress ends, whichever page is shown,
    /// keeping what was measured so far. A press still open or a lift-off
    /// motion still under way is not judged.
    pub fn input_changed(&mut self) {
        if self.response_running {
            self.stop_response();
        }
        if self.sticky_running {
            self.stop_sticky();
        }
        if self.liftoff_running {
            self.stop_liftoff_at(Instant::now(), LiftoffStop::InputChanged);
        }
    }

    /// End the stickiness run. A press still open never gets its release
    /// here, so it is dropped.
    fn stop_sticky(&mut self) {
        self.sticky_running = false;
        self.sticky_press_start = None;
        self.sticky_right_press_start = None;
        self.sticky_is_pressed = false;
        self.sticky_right_is_pressed = false;
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

        // Device-timed and frame-timed holds cannot be mixed in one run, and
        // the export names one timing for it: a change of input source ends
        // the run, keeping the clicks measured so far.
        if self.response_running && has_bridge != self.response_raw {
            self.stop_response();
        }

        // Controls
        ui.horizontal(|ui| {
            if self.response_running {
                if ui.button("Stop").clicked() {
                    self.stop_response();
                }
            } else if ui.button("Start").clicked() {
                self.response_running = true;
                self.response_raw = has_bridge;
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

        // Hold times follow the input the run used
        let raw = if self.response_running || click_count > 0 {
            self.response_raw
        } else {
            has_bridge
        };
        if !raw {
            ui.label(egui::RichText::new(FRAME_TIMED_NOTE).color(egui::Color32::YELLOW));
            ui.add_space(8.0);
        }

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
                        // A press shorter than a frame reads as 0 ms or a
                        // frame without raw input: not a measurement.
                        let min_str = if min_hold == f64::MAX || !raw {
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

            for input in button_edges(ctx, raw_events, has_bridge, rect, now) {
                let ButtonInput::Edge(edge) = input else {
                    self.response_press_start = None;
                    self.response_right_press_start = None;
                    continue;
                };
                let (pressed_at, count, times, holds) = match edge.button {
                    TestButton::Left => (
                        &mut self.response_press_start,
                        &mut self.response_click_count,
                        &mut self.response_click_times,
                        &mut self.response_hold_times,
                    ),
                    TestButton::Right => (
                        &mut self.response_right_press_start,
                        &mut self.response_right_click_count,
                        &mut self.response_right_click_times,
                        &mut self.response_right_hold_times,
                    ),
                };
                if let Some(hold_ms) = track_hold(pressed_at, &edge) {
                    *count += 1;
                    times.push_back(edge.time);
                    holds.push_back(hold_ms);
                    if holds.len() > 100 {
                        holds.pop_front();
                    }
                }
            }
            self.response_is_pressed = self.response_press_start.is_some();
            self.response_right_is_pressed = self.response_right_press_start.is_some();

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

        // Device-timed and frame-timed holds are not judged together: a
        // change of input source ends the run, keeping the clicks so far.
        if self.sticky_running && has_bridge != self.sticky_raw {
            self.stop_sticky();
        }

        // Controls
        ui.horizontal(|ui| {
            if self.sticky_running {
                if ui.button("Stop").clicked() {
                    self.sticky_running = false;
                }
            } else if ui.button("Start").clicked() {
                self.sticky_running = true;
                self.sticky_raw = has_bridge;
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
            // Only presses that start in the test area count. Raw input sees
            // every click, including the Stop button and the sidebar, whose
            // release this panel never gets once it stops or is left: the
            // press would stay open and later read as a long hold. Without
            // raw input every button event counts, even a press and release
            // within one frame.
            for input in button_edges(ctx, raw_events, has_bridge, rect, Instant::now()) {
                let ButtonInput::Edge(edge) = input else {
                    self.sticky_press_start = None;
                    self.sticky_right_press_start = None;
                    continue;
                };
                let (pressed_at, holds, slow) = match edge.button {
                    TestButton::Left => (
                        &mut self.sticky_press_start,
                        &mut self.sticky_holds,
                        &mut self.sticky_count,
                    ),
                    TestButton::Right => (
                        &mut self.sticky_right_press_start,
                        &mut self.sticky_right_holds,
                        &mut self.sticky_right_count,
                    ),
                };
                if let Some(hold_ms) = track_hold(pressed_at, &edge) {
                    holds.push(hold_ms);
                    if hold_ms > sticky_ms {
                        *slow += 1;
                    }
                }
            }
            self.sticky_is_pressed = self.sticky_press_start.is_some();
            self.sticky_right_is_pressed = self.sticky_right_press_start.is_some();
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
        self.liftoff_pending.clear();
        self.liftoff_lifts = 0;
        self.liftoff_epoch = Instant::now();
        self.liftoff_stopped_for_input = false;
        self.liftoff_running = true;
    }

    /// Seconds from the start of the run to `t`.
    fn liftoff_secs(&self, t: Instant) -> f64 {
        t.saturating_duration_since(self.liftoff_epoch)
            .as_secs_f64()
    }

    /// End the run from the keyboard; motion still under way is judged as
    /// it stands, as in the CLI test.
    fn stop_liftoff(&mut self) {
        self.stop_liftoff_at(Instant::now(), LiftoffStop::Key);
    }

    /// End the run at `now`. Clicking Stop moved the measured mouse: the
    /// motion under way, and any that ended within [`LIFTOFF_STOP_TRIP_S`],
    /// was that trip and is not judged. Motion cut off by the input
    /// changing is not judged either (as in the CLI test), and the page
    /// says the run was cut short.
    fn stop_liftoff_at(&mut self, now: Instant, by: LiftoffStop) {
        let open = self.liftoff_detector.flush();
        let pending = std::mem::take(&mut self.liftoff_pending);
        let dpi = self.liftoff_dpi.0;
        match by {
            LiftoffStop::Key => {
                for burst in pending.iter().chain(&open) {
                    self.liftoff_tally.record(burst, dpi);
                }
            }
            LiftoffStop::InputChanged => {
                for burst in &pending {
                    self.liftoff_tally.record(burst, dpi);
                }
                self.liftoff_stopped_for_input = true;
            }
            LiftoffStop::Button => {
                let trip_from = self.liftoff_secs(now) - LIFTOFF_STOP_TRIP_S;
                for burst in pending.iter().filter(|b| b.end_s < trip_from) {
                    self.liftoff_tally.record(burst, dpi);
                }
            }
        }
        self.liftoff_running = false;
    }

    fn reset_liftoff(&mut self) {
        self.liftoff_running = false;
        self.liftoff_stopped_for_input = false;
        self.liftoff_detector = LiftDetector::new(self.liftoff_dpi.0);
        self.liftoff_tally = LiftTally::default();
        self.liftoff_pending.clear();
        self.liftoff_lifts = 0;
    }

    /// Hold a finished burst back until it is [`LIFTOFF_STOP_TRIP_S`] old.
    fn record_liftoff(&mut self, burst: Option<Burst>) {
        self.liftoff_pending.extend(burst);
    }

    /// Feed raw motion reports (from the bridge) and advance to `now`. After
    /// reports the system dropped, part of a movement may be gone: it is
    /// not judged, and the detector starts over once the mouse is seen
    /// still again (as in the CLI test).
    fn feed_liftoff(&mut self, raw_events: &[RawInputEvent], now: Instant) {
        let epoch = self.liftoff_epoch;
        let secs = |t: Instant| t.duration_since(epoch).as_secs_f64();
        for event in raw_events {
            if event.timestamp < epoch {
                continue;
            }
            if event.lost_before {
                self.liftoff_detector =
                    LiftDetector::new(self.liftoff_dpi.0).watching_from(secs(event.timestamp));
            }
            if let RawInputKind::Move { dx, dy } = event.kind {
                let burst = self.liftoff_detector.feed(secs(event.timestamp), dx, dy);
                self.record_liftoff(burst);
            }
        }
        let now_s = secs(now.max(epoch));
        let burst = self.liftoff_detector.tick(now_s);
        self.record_liftoff(burst);
        // Bursts old enough that a Stop click now cannot be their trip.
        let pending = std::mem::take(&mut self.liftoff_pending);
        let (settled, recent): (Vec<Burst>, Vec<Burst>) = pending
            .into_iter()
            .partition(|b| b.end_s < now_s - LIFTOFF_STOP_TRIP_S);
        for burst in &settled {
            self.liftoff_tally.record(burst, self.liftoff_dpi.0);
        }
        self.liftoff_pending = recent;
    }

    /// The totals so far, with the bursts still held back counted in.
    fn liftoff_view(&self) -> LiftTally {
        let mut tally = self.liftoff_tally.clone();
        for burst in &self.liftoff_pending {
            tally.record(burst, self.liftoff_dpi.0);
        }
        tally
    }

    fn liftoff_verdict(&self) -> LiftVerdict {
        self.liftoff_view().verdict(self.liftoff_lifts, MIN_LIFTS)
    }

    /// What the status cell shows.
    fn liftoff_status(&self) -> &'static str {
        if !self.liftoff_running {
            return "Stopped";
        }
        match self.liftoff_detector.state() {
            LiftState::Settling => "Waiting for the mouse to be still",
            LiftState::Still => "Still",
            LiftState::Moving => "Moving",
        }
    }

    pub fn ui_liftoff(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
    ) {
        // Input first, before any button is drawn: SPACE is consumed so it
        // cannot "click" a focused button such as Stop. Enter or Esc stop the
        // test without moving the mouse, so the last lift still counts.
        if self.liftoff_running {
            if has_bridge {
                let stop_key = ctx.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                        | i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                });
                self.liftoff_lifts += take_space_presses(ctx);
                self.feed_liftoff(raw_events, Instant::now());
                if stop_key {
                    self.stop_liftoff();
                }
            } else {
                // The measured mouse went away: keep what was measured.
                self.stop_liftoff_at(Instant::now(), LiftoffStop::InputChanged);
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
        if self.liftoff_stopped_for_input {
            ui.add_space(5.0);
            input_lost_notice(ui);
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
                    self.stop_liftoff_at(Instant::now(), LiftoffStop::Button);
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
        let tally = &self.liftoff_view();
        let jumps = tally.jumps.len();
        let possible = tally.possible.len();
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
                        let color = match (jumps, jumps + possible) {
                            (_, 0) => egui::Color32::WHITE,
                            (0 | 1, _) => egui::Color32::YELLOW,
                            _ => egui::Color32::RED,
                        };
                        ui.label(
                            egui::RichText::new(format!("{}", jumps))
                                .size(24.0)
                                .color(color),
                        );
                        if possible > 0 {
                            ui.label(
                                egui::RichText::new(format!("+{} possible", possible))
                                    .color(egui::Color32::YELLOW),
                            );
                        }
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
                        if tally.nudges > 0 {
                            ui.label(
                                egui::RichText::new(format!("+{} hand nudges", tally.nudges))
                                    .weak(),
                            );
                        }
                    });
                    ui.add_space(40.0);
                    ui.vertical(|ui| {
                        ui.label("Status");
                        // The long "waiting" text in a smaller size
                        let settling = self.liftoff_running
                            && self.liftoff_detector.state() == LiftState::Settling;
                        let size = if settling { 14.0 } else { 20.0 };
                        ui.label(egui::RichText::new(self.liftoff_status()).size(size));
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
                    "The cursor jumps when the mouse is lifted or set down, too abruptly for the hand to have moved it. Lower the lift-off distance (LOD) in the mouse software, or try a plainer surface.".to_string(),
                ),
                LiftVerdict::PossibleJumps => (
                    "Possible jumps",
                    egui::Color32::YELLOW,
                    format!(
                        "{} movements carried the cursor more than {:.1} mm, but {} of them started or stopped gently enough to be the hand - for example the mouse landing while it still slid sideways, or a mouse reporting too rarely to show how it moved. Set the mouse straight down without sliding it and repeat the test: if they come back, the lift-off distance (LOD) is likely too high.",
                        jumps + possible,
                        JUMP_THRESHOLD_MM,
                        possible
                    ),
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
                    if dpi_assumed && jumps + possible > 0 {
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
                    if tally.nudges > 0 {
                        ui.add_space(5.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "{} movement(s) sped up and slowed down smoothly, as the hand moves the mouse, and were not judged.",
                                tally.nudges
                            ))
                            .weak(),
                        );
                    }
                });
            ui.add_space(10.0);
        }

        // Jump history
        if tally.jump_sized_count() > 0 {
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
                        let all = tally
                            .jumps
                            .iter()
                            .map(|j| (j, "too abrupt for a hand"))
                            .chain(
                                tally
                                    .possible
                                    .iter()
                                    .map(|j| (j, "possible: could be the hand")),
                            );
                        for (i, (jump, kind)) in all.enumerate() {
                            ui.label(format!(
                                "#{}: {:.1} mm (dx {:+}, dy {:+} counts) within {:.0} ms - {}",
                                i + 1,
                                counts_to_mm(jump.peak_counts, dpi),
                                jump.peak_dx,
                                jump.peak_dy,
                                jump.peak_ms,
                                kind
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
                    "4. Wait about a second and repeat - at least {} times - then press Enter (or click Stop)",
                    MIN_LIFTS
                ));
                ui.label(
                    egui::RichText::new(format!(
                        "Enter or Esc stop the test without moving the mouse. Clicking Stop does move it, so motion in the last {:.1} s before the click is taken as the trip to the button and not judged.",
                        LIFTOFF_STOP_TRIP_S
                    ))
                    .weak(),
                );
                ui.add_space(5.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Jump-sized: motion that starts from stillness and carries the cursor more than {:.1} mm ({:.0} counts at {:.0} DPI) within {:.0} ms. Longer sliding movements are not judged, nor are movements that speed up and slow down smoothly as the hand moves the mouse. One that starts and stops too abruptly for a hand is a jump; one that could also be the hand (for example the mouse landing while it still slid sideways) is a possible jump.",
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
        self.liftoff_lifts > 0 || self.liftoff_tally.bursts() + self.liftoff_pending.len() > 0
    }

    pub fn export_response(&self) -> Option<ClickResponseExport> {
        if self.response_click_count == 0 && self.response_right_click_count == 0 {
            return None;
        }
        Some(ClickResponseExport {
            hold_timing: if self.response_raw {
                "device"
            } else {
                "display frame"
            }
            .to_string(),
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
        // A press still open is left out, as on screen after Stop.
        let cfg = DiagConfig::default();
        let verdict = |holds: &[f64]| sticky_verdict(holds, None, &cfg).summary();
        Some(ClickStickyExport {
            long_hold_threshold_ms: cfg.sticky_threshold_ms,
            left: StickyButtonExport {
                click_count: self.sticky_holds.len(),
                sticky_count: self.sticky_count,
                long_hold_count: self.sticky_count,
                verdict: verdict(&self.sticky_holds),
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
                long_hold_count: self.sticky_right_count,
                verdict: verdict(&self.sticky_right_holds),
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
        let tally = &self.liftoff_view();
        // Jumps and possible jumps; the verdict says whether any could have
        // been the hand.
        let counts: Vec<f64> = tally.jump_sized().map(|j| j.peak_counts).collect();
        Some(LiftOffExport {
            jump_count: tally.jump_sized_count(),
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
                lost_before: false,
            });
            events.push(RawInputEvent {
                kind: RawInputKind::ButtonRelease(RawButton::Left),
                timestamp: t + Duration::from_millis(hold),
                lost_before: false,
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

    /// One frame of the Click Test panel with the pointer at `pointer`,
    /// `raw` events through the raw-input path when `has_bridge`, and
    /// `pointer_events` as the window system's input.
    fn response_frame(
        panel: &mut ClickPanel,
        pointer: egui::Pos2,
        raw: &[RawInputEvent],
        pointer_events: Vec<egui::Event>,
        has_bridge: bool,
    ) {
        let ctx = egui::Context::default();
        let mut events = vec![egui::Event::PointerMoved(pointer)];
        events.extend(pointer_events);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 900.0),
            )),
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            panel.ui_response(ui, &ctx, raw, has_bridge);
        });
    }

    #[test]
    fn click_test_counts_only_presses_in_its_box() {
        // A press on the heading (the sidebar, Stop) whose release comes
        // with a later click in the box must not become a long hold
        let mut panel = ClickPanel::new();
        panel.response_running = true;
        let events = left_clicks(&[30_000]);
        response_frame(&mut panel, ON_HEADING, &events[..1], vec![], true);
        assert!(!panel.response_is_pressed);
        response_frame(&mut panel, IN_TEST_AREA, &events[1..], vec![], true);
        assert_eq!(panel.response_click_count, 0);
        assert!(panel.response_hold_times.is_empty());
        // Clicks in the box count
        response_frame(
            &mut panel,
            IN_TEST_AREA,
            &left_clicks(&[90, 80]),
            vec![],
            true,
        );
        assert_eq!(panel.response_click_count, 2);
        assert_eq!(panel.response_hold_times.len(), 2);
    }

    #[test]
    fn click_test_stops_when_the_input_source_changes() {
        // A raw run whose mouse is unplugged: its frame-timed clicks must
        // not join holds the export calls device-timed
        let mut panel = ClickPanel::new();
        panel.response_running = true;
        panel.response_raw = true;
        response_frame(&mut panel, IN_TEST_AREA, &left_clicks(&[90]), vec![], true);
        assert_eq!(panel.response_click_count, 1);
        let button = |pressed| egui::Event::PointerButton {
            pos: IN_TEST_AREA,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        response_frame(
            &mut panel,
            IN_TEST_AREA,
            &[],
            vec![button(true), button(false)],
            false,
        );
        assert!(!panel.response_running);
        assert_eq!(panel.response_click_count, 1);
        assert_eq!(panel.export_response().unwrap().hold_timing, "device");
    }

    #[test]
    fn framework_clicks_within_one_frame_are_counted() {
        // A fast click: press and release reach egui in the same frame
        let mut panel = ClickPanel::new();
        panel.response_running = true;
        panel.response_raw = false;
        let button = |pressed| egui::Event::PointerButton {
            pos: IN_TEST_AREA,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        response_frame(
            &mut panel,
            IN_TEST_AREA,
            &[],
            vec![button(true), button(false)],
            false,
        );
        assert_eq!(panel.response_click_count, 1);
        assert!(!panel.response_is_pressed);
        assert_eq!(
            panel.export_response().unwrap().hold_timing,
            "display frame"
        );

        // The same in the stickiness panel
        let mut panel = ClickPanel::new();
        panel.sticky_running = true;
        panel.sticky_raw = false;
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 900.0),
            )),
            events: vec![
                egui::Event::PointerMoved(IN_TEST_AREA),
                button(true),
                button(false),
            ],
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            panel.ui_sticky(ui, &ctx, &[], false);
        });
        assert_eq!(panel.sticky_holds.len(), 1);
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
        assert_eq!(export.left.verdict, "No slow releases");
        assert_eq!(export.right.verdict, "Inconclusive: fewer than 5 clicks");
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
        assert_eq!(export.left.long_hold_count, 3);
        assert_eq!(export.left.verdict, "Slow or sticky releases");
        assert_eq!(export.long_hold_threshold_ms, 250.0);
        assert!(matches!(
            sticky_verdict(&panel.sticky_holds, None, &DiagConfig::default()),
            StickyVerdict::SlowReleases { count: 3, .. }
        ));
    }

    #[test]
    fn releases_restored_after_dropped_reports_are_not_slow_releases() {
        // The kernel dropped two releases while the reader fell behind; they
        // were restored after the gap with the time of the drop, 400 ms
        // after their press
        let mut events = left_clicks(&[400, 100, 400, 100, 100, 100]);
        for release in [1, 5] {
            events[release].lost_before = true;
        }
        let mut panel = ClickPanel::new();
        panel.sticky_running = true;
        sticky_frame(&mut panel, IN_TEST_AREA, &events);
        assert_eq!(panel.sticky_count, 0);
        assert_eq!(panel.sticky_holds, [100.0, 100.0, 100.0, 100.0]);
        // Unmarked, they read as a switch releasing late
        let panel = sticky_panel_with_holds(&[400, 100, 400, 100, 100, 100]);
        assert_eq!(panel.sticky_count, 2);
    }

    #[test]
    fn one_long_click_is_not_called_sticky() {
        let cfg = DiagConfig::default();
        let holds = [100.0, 120.0, 400.0, 90.0, 110.0, 130.0];
        assert_eq!(
            sticky_verdict(&holds, None, &cfg),
            StickyVerdict::OneLongHold { held_ms: 400.0 }
        );
        // The export says the same, not "1 sticky click"
        let export = sticky_panel_with_holds(&[100, 120, 400, 90, 110, 130])
            .export_sticky()
            .unwrap();
        assert_eq!(export.left.long_hold_count, 1);
        assert_eq!(
            export.left.verdict,
            "One long click, usually the hand, not the switch"
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
                lost_before: false,
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
        // Frames go on until the second set-down has been still a while.
        panel.feed_liftoff(&[], t0 + Duration::from_millis(3000));
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
    fn liftoff_does_not_judge_motion_cut_by_dropped_reports() {
        // A set-down jump of 200 counts at 1000 Hz, part of which the system
        // dropped while the reader fell behind: what is left is not judged
        for marked in [true, false] {
            let mut panel = ClickPanel::new();
            panel.start_liftoff();
            let t0 = panel.liftoff_epoch;
            let mut events = reports(t0, 500, 20, 1, 10, 0);
            events[10].lost_before = marked;
            liftoff_frame(&mut panel, &events, true, true);
            panel.feed_liftoff(&[], t0 + Duration::from_millis(3000));
            panel.stop_liftoff();
            let jumps = panel.liftoff_view().jump_sized_count();
            assert_eq!(jumps, usize::from(!marked), "marked: {}", marked);
        }
    }

    #[test]
    fn liftoff_does_not_judge_motion_cut_off_by_an_input_change() {
        // A burst still under way (7 ms of a sideways slide) when the
        // receiver drops out: the CLI does not judge it either
        for input_lost in [true, false] {
            let mut panel = ClickPanel::new();
            panel.start_liftoff();
            let t0 = panel.liftoff_epoch;
            panel.feed_liftoff(
                &reports(t0, 500, 7, 1, 30, 0),
                t0 + Duration::from_millis(507),
            );
            if input_lost {
                panel.input_changed();
            } else {
                panel.stop_liftoff();
            }
            assert!(!panel.liftoff_running);
            let judged = panel.liftoff_view().jump_sized_count();
            assert_eq!(judged, usize::from(!input_lost), "lost: {}", input_lost);
            // ... and the page says the run was cut short until the next Start
            assert_eq!(panel.liftoff_stopped_for_input, input_lost);
        }
        let mut panel = ClickPanel::new();
        panel.start_liftoff();
        liftoff_frame(&mut panel, &[], false, false);
        assert!(!panel.liftoff_running && panel.liftoff_stopped_for_input);
        panel.start_liftoff();
        assert!(!panel.liftoff_stopped_for_input);
    }

    #[test]
    fn stickiness_ends_when_the_input_source_changes() {
        // Raw clicks, then the mouse goes away: frame-timed holds are not
        // judged with device-timed ones
        let mut panel = ClickPanel::new();
        panel.sticky_running = true;
        panel.sticky_raw = true;
        sticky_frame(&mut panel, IN_TEST_AREA, &left_clicks(&[100, 100]));
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx().clone();
            panel.ui_sticky(ui, &ctx, &[], false);
        });
        assert!(!panel.sticky_running);
        assert_eq!(panel.sticky_holds.len(), 2);
        // Another mouse chosen while the page is not shown
        panel.sticky_running = true;
        panel.input_changed();
        assert!(!panel.sticky_running);
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

    /// One frame of the lift-off panel with `keys` pressed and released.
    fn liftoff_frame_keys(panel: &mut ClickPanel, keys: &[egui::Key]) {
        let ctx = egui::Context::default();
        let events = keys
            .iter()
            .flat_map(|&key| {
                [true, false].map(|pressed| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                })
            })
            .collect();
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            panel.ui_liftoff(ui, &ctx, &[], true);
        });
    }

    #[test]
    fn liftoff_stop_button_does_not_count_the_trip_to_it() {
        // A set-down jump, then the hand takes the mouse to the Stop button:
        // a quick 3 mm move that ends 0.3 s before the click (and that a
        // stray count after it has already closed), and a last correction
        // still under way at the click. Neither is a lift.
        let mut panel = ClickPanel::new();
        panel.start_liftoff();
        let t0 = panel.liftoff_epoch;
        let mut events = reports(t0, 500, 10, 1, 10, 0);
        events.extend(reports(t0, 1690, 10, 1, -10, 0));
        events.extend(reports(t0, 1900, 1, 1, 1, 0));
        events.extend(reports(t0, 1990, 6, 1, 0, 12));
        liftoff_frame(&mut panel, &events, false, true);
        assert_eq!(panel.liftoff_view().jumps.len(), 2);
        panel.stop_liftoff_at(t0 + Duration::from_millis(2000), LiftoffStop::Button);
        assert_eq!(
            panel.liftoff_tally.jumps.len(),
            1,
            "{:?}",
            panel.liftoff_tally
        );
        assert_eq!(panel.liftoff_tally.jumps[0].peak_dx, 100);
        assert!(panel.liftoff_pending.is_empty());

        // Stopped from the keyboard the mouse did not move: all of it counts,
        // the motion under way included (judged as it stands, which without
        // its stop makes it a possible jump at most).
        let mut panel = ClickPanel::new();
        panel.start_liftoff();
        let t0 = panel.liftoff_epoch;
        let mut shifted = events.clone();
        for e in &mut shifted {
            e.timestamp =
                t0 + e.timestamp.duration_since(events[0].timestamp) + Duration::from_millis(500);
        }
        liftoff_frame(&mut panel, &shifted, false, true);
        panel.stop_liftoff_at(t0 + Duration::from_millis(2000), LiftoffStop::Key);
        let tally = &panel.liftoff_tally;
        assert_eq!(
            (tally.jumps.len(), tally.possible.len()),
            (2, 1),
            "{:?}",
            tally
        );
    }

    #[test]
    fn liftoff_enter_or_esc_stops_without_the_mouse() {
        for key in [egui::Key::Enter, egui::Key::Escape] {
            let mut panel = ClickPanel::new();
            panel.start_liftoff();
            let t0 = panel.liftoff_epoch;
            // A jump still under way when the key is pressed is kept.
            panel.feed_liftoff(
                &reports(t0, 500, 10, 1, 10, 0),
                t0 + Duration::from_millis(505),
            );
            assert!(panel.liftoff_detector.in_burst());
            liftoff_frame_keys(&mut panel, &[key]);
            assert!(!panel.liftoff_running, "{:?}", key);
            assert_eq!(panel.liftoff_tally.jump_sized_count(), 1);
        }
        // Not a stop key while stopped (Enter could start a test again).
        let mut panel = ClickPanel::new();
        liftoff_frame_keys(&mut panel, &[egui::Key::Enter]);
        assert!(!panel.liftoff_running);
    }

    #[test]
    fn liftoff_status_says_when_it_waits_for_stillness() {
        let mut panel = ClickPanel::new();
        assert_eq!(panel.liftoff_status(), "Stopped");
        panel.start_liftoff();
        let t0 = panel.liftoff_epoch;
        // Still moving from the click on Start: nothing is judged yet, and
        // the panel says so rather than "Still".
        panel.feed_liftoff(
            &reports(t0, 10, 30, 2, 4, 0),
            t0 + Duration::from_millis(70),
        );
        assert_eq!(panel.liftoff_status(), "Waiting for the mouse to be still");
        assert!(panel.liftoff_view().bursts() == 0);
        // A whole idle window without motion.
        panel.feed_liftoff(&[], t0 + Duration::from_millis(200));
        assert_eq!(panel.liftoff_status(), "Still");
        panel.feed_liftoff(
            &reports(t0, 300, 3, 1, 20, 0),
            t0 + Duration::from_millis(303),
        );
        assert_eq!(panel.liftoff_status(), "Moving");
        // It renders with every status.
        liftoff_frame(&mut panel, &[], false, true);
    }

    #[test]
    fn liftoff_hand_nudges_and_possible_jumps_are_told_apart() {
        // 800 DPI, 1000 Hz. Two smooth 3 mm hand nudges are not judged;
        // two set-downs that land sliding (full speed at once, then slowing)
        // could be the hand: possible jumps, not "cursor jumps on lift".
        let mut panel = ClickPanel::new();
        panel.start_liftoff();
        let t0 = panel.liftoff_epoch;
        let f = |tau: f64| 10.0 * tau.powi(3) - 15.0 * tau.powi(4) + 6.0 * tau.powi(5);
        let mut events = Vec::new();
        for k in 0..2u64 {
            let (mut carried, mut last) = (0.0, 0.0);
            for ms in 1..=60u64 {
                let x = 3.0 * f(ms as f64 / 60.0);
                carried += mm_to_counts(x - last, 800.0);
                last = x;
                if carried >= 1.0 {
                    let dx = carried.trunc();
                    carried -= dx;
                    events.extend(reports(t0, 500 + k * 1000 + ms, 1, 1, dx as i32, 0));
                }
            }
        }
        for k in 0..2u64 {
            for (i, dx) in [12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 2, 1, 1, 1]
                .into_iter()
                .enumerate()
            {
                events.extend(reports(t0, 3000 + k * 1000 + i as u64, 1, 1, dx, 0));
            }
        }
        for _ in 0..MIN_LIFTS {
            liftoff_frame(&mut panel, &[], true, true);
        }
        liftoff_frame(&mut panel, &events, false, true);
        panel.stop_liftoff();
        let tally = &panel.liftoff_tally;
        assert_eq!(
            (tally.nudges, tally.possible.len(), tally.jumps.len()),
            (2, 2, 0),
            "{:?}",
            tally
        );
        assert_eq!(panel.liftoff_verdict(), LiftVerdict::PossibleJumps);
        let export = panel.export_liftoff().unwrap();
        assert_eq!(export.jump_count, 2);
        assert_eq!(export.verdict, "Possible jumps - repeat the test");
        liftoff_frame(&mut panel, &[], false, true);
    }

    #[test]
    fn thresholds_match_auto_diagnostics() {
        let cfg = DiagConfig::default();
        assert_eq!(cfg.sticky_threshold_ms, 250.0);
        assert_eq!(cfg.clicks_min_left, 5);
    }
}
