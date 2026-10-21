//! Automated diagnostics panel
//!
//! Drives `mouse_testkit::analysis::diagnostics::AutoTest`: a guided,
//! timed sequence of phases that looks for faults on its own (phantom input
//! at rest, stutters and unstable polling, switch bounce, sticky or stuck
//! buttons, scroll encoder skips, lift-off jumps) and produces a report.
//!
//! The engine is fed from `App::ui` every frame regardless of which panel
//! is visible, so navigating away does not lose events.
//!
//! Without the raw-input bridge the engine runs with
//! `timing_reliable: false` and skips every check that needs report timing
//! or raw sensor counts; wheel input is then summed into whole notches here.

use eframe::egui;
use std::time::Instant;

use crate::input_bridge::{RawButton, RawInputEvent, RawInputKind};
use crate::theme::ThemeColors;
use mouse_testkit::analysis::diagnostics::{AutoTest, Config, Phase, Report, Severity, Verdict};
use mouse_testkit::types::{MouseButton, MouseEvent};

pub struct AutoTestPanel {
    engine: AutoTest,
    /// Wall-clock origin for converting timestamps to seconds.
    epoch: Instant,
    /// Whether the last run used precise raw-input timestamps.
    last_run_raw: bool,
    show_metrics: bool,
    /// Framework input: partial wheel movement not yet worth a notch.
    wheel: WheelAccumulator,
}

/// Turns window-system wheel deltas into whole notches.
///
/// Framework input reports wheel movement in lines, points or pages, and
/// high-resolution or smooth-scrolling wheels split one notch into many small
/// events. Counting every event as a notch would overcount, and a tiny
/// backwards wobble would look like an encoder skip, so partial movement is
/// summed until it reaches a whole line (one notch on a standard wheel).
///
/// macOS is the exception: it reports each notch of a standard wheel as one
/// line event whose size is accelerated (a slow notch is a fraction of a
/// line, a fast one several lines), so there each line event is one notch.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct WheelAccumulator {
    lines: f32,
    line_event_is_notch: bool,
}

impl WheelAccumulator {
    /// The accumulator for the window system this build runs on.
    pub(crate) fn for_platform() -> Self {
        Self::new(cfg!(target_os = "macos"))
    }

    pub(crate) fn new(line_event_is_notch: bool) -> Self {
        Self {
            lines: 0.0,
            line_event_is_notch,
        }
    }

    /// Add one wheel event and return the whole notches it completes
    /// (positive = up / away from the user). `points_per_line` converts
    /// point deltas; a page delta counts as one notch per page, which is what
    /// one notch scrolls with the "one screen at a time" setting.
    pub(crate) fn add(&mut self, unit: egui::MouseWheelUnit, dy: f32, points_per_line: f32) -> i32 {
        let lines = match unit {
            egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => dy,
            egui::MouseWheelUnit::Point => dy / points_per_line.max(1.0),
        };
        if !lines.is_finite() || lines == 0.0 {
            return 0;
        }
        if self.line_event_is_notch && unit == egui::MouseWheelUnit::Line {
            self.lines = 0.0;
            return lines.signum() as i32;
        }
        // Leftover from scrolling one way must not eat into the other way.
        if self.lines != 0.0 && self.lines.signum() != lines.signum() {
            self.lines = 0.0;
        }
        self.lines += lines;
        // Tolerate float error so four quarter-line events make a notch.
        let whole = (self.lines + 1e-3 * self.lines.signum()).trunc();
        self.lines -= whole;
        whole as i32
    }
}

fn to_lib_event(kind: &RawInputKind) -> MouseEvent {
    fn button(b: RawButton) -> MouseButton {
        match b {
            RawButton::Left => MouseButton::Left,
            RawButton::Right => MouseButton::Right,
            RawButton::Middle => MouseButton::Middle,
            RawButton::Side => MouseButton::Side,
            RawButton::Extra => MouseButton::Extra,
        }
    }
    match *kind {
        RawInputKind::Move { dx, dy } => MouseEvent::Move { dx, dy },
        RawInputKind::ButtonPress(b) => MouseEvent::ButtonPress(button(b)),
        RawInputKind::ButtonRelease(b) => MouseEvent::ButtonRelease(button(b)),
        RawInputKind::Scroll { delta } => MouseEvent::Scroll { delta },
    }
}

fn severity_color(s: Severity) -> egui::Color32 {
    match s {
        Severity::Info => ThemeColors::text_secondary(),
        Severity::Warning => egui::Color32::YELLOW,
        Severity::Critical => egui::Color32::from_rgb(255, 90, 90),
    }
}

fn verdict_color(v: Verdict) -> egui::Color32 {
    match v {
        Verdict::Healthy => ThemeColors::success(),
        Verdict::Attention => egui::Color32::YELLOW,
        Verdict::Faulty => egui::Color32::from_rgb(255, 90, 90),
        Verdict::Inconclusive => ThemeColors::text_secondary(),
    }
}

impl AutoTestPanel {
    pub fn new() -> Self {
        Self {
            engine: AutoTest::new(Config::default()),
            epoch: Instant::now(),
            last_run_raw: true,
            show_metrics: false,
            wheel: WheelAccumulator::for_platform(),
        }
    }

    pub fn is_running(&self) -> bool {
        self.engine.is_running()
    }

    pub fn report(&self) -> Option<&Report> {
        self.engine.report()
    }

    fn secs(&self, t: Instant) -> f64 {
        t.duration_since(self.epoch).as_secs_f64()
    }

    /// Feed this frame's input into the engine. Called every frame from the
    /// app, whether or not this panel is visible.
    pub fn process_input(
        &mut self,
        ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
    ) {
        if !self.engine.is_running() {
            return;
        }
        if has_bridge {
            for ev in raw_events {
                let t = self.secs(ev.timestamp);
                self.engine.feed(t, &to_lib_event(&ev.kind));
            }
        } else {
            // Framework fallback: frame-quantised timing, no per-report data.
            let now = self.secs(Instant::now());
            let (delta, events) = ctx.input(|i| (i.pointer.delta(), i.raw.events.clone()));
            let points_per_line = ctx.options(|o| o.input_options.line_scroll_speed);
            if delta.x != 0.0 || delta.y != 0.0 {
                self.engine.feed(
                    now,
                    &MouseEvent::Move {
                        dx: delta.x.round() as i32,
                        dy: delta.y.round() as i32,
                    },
                );
            }
            for e in &events {
                match e {
                    egui::Event::PointerButton {
                        button, pressed, ..
                    } => {
                        let b = match button {
                            egui::PointerButton::Primary => MouseButton::Left,
                            egui::PointerButton::Secondary => MouseButton::Right,
                            egui::PointerButton::Middle => MouseButton::Middle,
                            egui::PointerButton::Extra1 => MouseButton::Side,
                            egui::PointerButton::Extra2 => MouseButton::Extra,
                        };
                        let ev = if *pressed {
                            MouseEvent::ButtonPress(b)
                        } else {
                            MouseEvent::ButtonRelease(b)
                        };
                        self.engine.feed(now, &ev);
                    }
                    egui::Event::MouseWheel { unit, delta, .. } if delta.y != 0.0 => {
                        let notches = self.wheel.add(*unit, delta.y, points_per_line);
                        if notches != 0 {
                            self.engine
                                .feed(now, &MouseEvent::Scroll { delta: notches });
                        }
                    }
                    _ => {}
                }
            }
        }
        self.engine.tick(self.secs(Instant::now()));
    }

    fn start(&mut self, has_bridge: bool) {
        self.engine = AutoTest::new(Config {
            timing_reliable: has_bridge,
            ..Config::default()
        });
        self.last_run_raw = has_bridge;
        self.wheel = WheelAccumulator::for_platform();
        self.engine.start(self.secs(Instant::now()));
    }

    #[cfg(test)]
    pub(crate) fn start_for_test(&mut self, has_bridge: bool) {
        self.start(has_bridge);
    }

    /// Use the wheel conversion of every platform but macOS.
    #[cfg(test)]
    fn use_line_wheel_for_test(&mut self) {
        self.wheel = WheelAccumulator::new(false);
    }

    /// Time origin: tests build event timestamps as `epoch + seconds`.
    #[cfg(test)]
    pub(crate) fn epoch_for_test(&self) -> Instant {
        self.epoch
    }

    /// Advance the engine's clock to `secs` after the epoch without input.
    #[cfg(test)]
    pub(crate) fn tick_for_test(&mut self, secs: f64) {
        self.engine.tick(secs);
    }

    #[cfg(test)]
    pub(crate) fn set_show_metrics_for_test(&mut self, show: bool) {
        self.show_metrics = show;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, has_bridge: bool) {
        ui.heading("Auto Diagnostics");
        ui.add_space(5.0);
        ui.label("Runs a short guided check-up and looks for faults by itself: phantom input, stutters, switch bounce, sticky buttons, scroll skips and lift-off jumps.");
        if !has_bridge {
            ui.label(
                egui::RichText::new(
                    "Note: Raw input unavailable — only the rest, button and scroll-direction checks can run. Polling rate, stutter, switch bounce, scroll skips and lift-off need raw input. Keep the pointer inside this window: clicks and scrolls outside it are not seen.",
                )
                .color(egui::Color32::YELLOW)
                .size(11.0),
            );
        }
        ui.add_space(15.0);

        let running = self.engine.is_running();
        if running && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.engine.abort();
        }
        ui.horizontal(|ui| {
            if self.engine.is_running() {
                // Abort sits at the far right, away from where Start / Run
                // Again was (the pointer is often still there when the click
                // step begins), and ignores the mouse during the click step.
                ui.label(
                    egui::RichText::new("Running — follow the instruction below.")
                        .weak()
                        .size(11.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let clicking = self.engine.current_phase() == Some(Phase::Clicks);
                    let abort = ui
                        .add_enabled(!clicking, egui::Button::new("Abort (Esc)"))
                        .on_disabled_hover_text("Press Esc to stop during the click step");
                    if abort.clicked() {
                        self.engine.abort();
                    }
                });
            } else {
                if ui
                    .button(if self.engine.report().is_some() {
                        "Run Again"
                    } else {
                        "Start Diagnostics"
                    })
                    .clicked()
                {
                    self.start(has_bridge);
                }
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(
                        "Takes about one minute. Follow the instruction shown for each step.",
                    )
                    .weak()
                    .size(11.0),
                );
            }
        });

        ui.add_space(20.0);

        if self.engine.is_running() {
            self.ui_running(ui);
            ctx.request_repaint();
        } else if let Some(report) = self.engine.report().cloned() {
            self.ui_report(ui, &report);
        } else {
            self.ui_idle(ui, has_bridge);
        }
    }

    fn ui_idle(&self, ui: &mut egui::Ui, has_bridge: bool) {
        // The steps depend on the input source the run will use.
        let preview = AutoTest::new(Config {
            timing_reliable: has_bridge,
            ..Config::default()
        });
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("What happens").strong());
                ui.add_space(5.0);
                for (i, phase) in preview.phases().iter().enumerate() {
                    ui.label(format!("{}. {} — {}", i + 1, phase.title(), phase.instruction()));
                }
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "Each step ends on its own once enough data is collected or its time is up. Press Start Diagnostics, then take your hand off the mouse.",
                    )
                    .weak(),
                );
            });
    }

    fn ui_running(&self, ui: &mut egui::Ui) {
        let Some(status) = self.engine.status() else {
            return;
        };

        // Step list
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(15.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (i, phase) in self.engine.phases().iter().enumerate() {
                        let (marker, color) = if i < status.index {
                            ("✔", ThemeColors::success())
                        } else if i == status.index {
                            ("▶", ThemeColors::accent())
                        } else {
                            ("○", ThemeColors::text_muted())
                        };
                        ui.label(
                            egui::RichText::new(format!("{} {}", marker, phase.title()))
                                .color(color),
                        );
                        ui.add_space(12.0);
                    }
                });
            });

        ui.add_space(15.0);

        // Current instruction
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(40, 60, 40))
            .inner_margin(20.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "Step {} of {}: {}",
                        status.index + 1,
                        status.total,
                        status.phase.title()
                    ))
                    .strong()
                    .color(egui::Color32::GREEN),
                );
                ui.add_space(8.0);
                ui.label(egui::RichText::new(status.phase.instruction()).size(18.0));
                ui.add_space(12.0);
                ui.add(
                    egui::ProgressBar::new(status.progress as f32)
                        .show_percentage()
                        .desired_width(ui.available_width()),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(status.detail.clone());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{:.0} s left",
                                status.remaining_secs.ceil()
                            ))
                            .weak(),
                        );
                    });
                });
            });

        // A dedicated place to click, so the clicks of this step do not land
        // on buttons (Abort, the sidebar's export) and, without raw input,
        // stay inside the window where they can be seen.
        if status.phase == Phase::Clicks {
            ui.add_space(12.0);
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), 140.0),
                egui::Sense::click(),
            );
            ui.painter()
                .rect_filled(rect, 8.0, egui::Color32::from_rgb(50, 60, 80));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Point here and click",
                egui::FontId::proportional(22.0),
                egui::Color32::WHITE,
            );
        }

        // Findings so far
        let so_far = self.engine.findings_so_far();
        if !so_far.is_empty() {
            ui.add_space(15.0);
            ui.label(egui::RichText::new("Found so far").strong());
            for f in so_far {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(f.severity.label())
                            .size(11.0)
                            .strong()
                            .color(severity_color(f.severity)),
                    );
                    ui.label(&f.title);
                });
            }
        }
    }

    fn ui_report(&mut self, ui: &mut egui::Ui, report: &Report) {
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(20.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Verdict").weak().size(12.0));
                        ui.label(
                            egui::RichText::new(report.verdict.label())
                                .size(26.0)
                                .strong()
                                .color(verdict_color(report.verdict)),
                        );
                    });
                    ui.add_space(30.0);
                    for (label, sev) in [
                        ("Critical", Severity::Critical),
                        ("Warnings", Severity::Warning),
                        ("Notes", Severity::Info),
                    ] {
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(label).weak().size(12.0));
                            ui.label(
                                egui::RichText::new(format!("{}", report.count(sev)))
                                    .size(22.0)
                                    .color(severity_color(sev)),
                            );
                        });
                        ui.add_space(20.0);
                    }
                });
                ui.add_space(6.0);
                let mut note = format!(
                    "{} of {} steps completed in {:.0} s",
                    report.phases_completed.len(),
                    self.engine.phases().len(),
                    report.duration_secs
                );
                if report.aborted {
                    note.push_str(" (aborted early)");
                }
                if !self.last_run_raw {
                    note.push_str(" — framework input");
                }
                ui.label(egui::RichText::new(note).weak());
                if !report.timing_reliable {
                    ui.label(
                        egui::RichText::new(
                            "Limited check: without raw input, switch bounce, polling rate, stutter, scroll-wheel skips and lift-off jumps were not tested.",
                        )
                        .color(egui::Color32::YELLOW)
                        .size(12.0),
                    );
                }
            });

        ui.add_space(15.0);

        for sev in [Severity::Critical, Severity::Warning, Severity::Info] {
            let items: Vec<_> = report
                .findings
                .iter()
                .filter(|f| f.severity == sev)
                .collect();
            if items.is_empty() {
                continue;
            }
            let heading = match sev {
                Severity::Critical => "Faults",
                Severity::Warning => "Warnings",
                Severity::Info => "Notes",
            };
            ui.label(
                egui::RichText::new(heading)
                    .strong()
                    .color(severity_color(sev)),
            );
            ui.add_space(4.0);
            for f in items {
                egui::Frame::new()
                    .fill(ui.visuals().faint_bg_color)
                    .stroke(egui::Stroke::new(
                        1.0,
                        severity_color(sev).gamma_multiply(0.5),
                    ))
                    .inner_margin(12.0)
                    .corner_radius(8.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&f.title).strong());
                            ui.label(
                                egui::RichText::new(format!("[{}]", f.phase.title()))
                                    .weak()
                                    .size(11.0),
                            );
                        });
                        ui.label(
                            egui::RichText::new(&f.detail).color(ThemeColors::text_secondary()),
                        );
                    });
                ui.add_space(6.0);
            }
            ui.add_space(8.0);
        }

        ui.checkbox(&mut self.show_metrics, "Show raw measurements");
        if self.show_metrics {
            let m = &report.metrics;
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(12.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    egui::Grid::new("auto_test_metrics")
                        .num_columns(2)
                        .spacing([24.0, 4.0])
                        .show(ui, |ui| {
                            let rows: Vec<(&str, String)> = vec![
                                ("Rest: movement reports", m.rest_move_events.to_string()),
                                (
                                    "Rest: movement distance",
                                    format!("{:.1} counts", m.rest_distance_counts),
                                ),
                                ("Rest: clicks", m.rest_clicks.to_string()),
                                ("Rest: scroll events", m.rest_scroll_events.to_string()),
                                (
                                    "Rest: seconds with movement",
                                    m.rest_motion_seconds.to_string(),
                                ),
                                (
                                    "Movement: fast report intervals",
                                    m.move_intervals.to_string(),
                                ),
                                (
                                    "Movement: polling rate",
                                    match (m.polling_hz, m.polling_nominal_hz) {
                                        (Some(h), Some(n)) => {
                                            format!("{} Hz (nominal {} Hz)", h, n)
                                        }
                                        (Some(h), None) => format!("{} Hz", h),
                                        (None, _) => "n/a".to_string(),
                                    },
                                ),
                                (
                                    "Movement: stutters",
                                    format!("{} ({:.2}%)", m.stutter_count, m.stutter_rate_percent),
                                ),
                                (
                                    "Movement: unstable intervals",
                                    format!("{:.1}%", m.unstable_interval_percent),
                                ),
                                (
                                    "Movement: worst interval",
                                    format!("{:.1} ms", m.max_interval_ms),
                                ),
                                ("Movement: pauses > 1 s", m.movement_pauses.to_string()),
                                (
                                    "Clicks: left / right / other",
                                    format!(
                                        "{} / {} / {}",
                                        m.left_clicks, m.right_clicks, m.other_clicks
                                    ),
                                ),
                                ("Clicks: bounce events", m.bounce_events.to_string()),
                                ("Clicks: micro presses", m.micro_presses.to_string()),
                                ("Clicks: sticky releases", m.sticky_releases.to_string()),
                                ("Clicks: stuck buttons", m.stuck_buttons.to_string()),
                                (
                                    "Clicks: hold avg / max",
                                    format!("{:.0} / {:.0} ms", m.avg_hold_ms, m.max_hold_ms),
                                ),
                                (
                                    "Clicks: shortest press interval",
                                    m.min_press_interval_ms
                                        .map(|v| format!("{:.1} ms", v))
                                        .unwrap_or_else(|| "n/a".to_string()),
                                ),
                                (
                                    "Scroll: down / up",
                                    format!("{} / {}", m.scroll_down, m.scroll_up),
                                ),
                                ("Scroll: reversals", m.scroll_reversals.to_string()),
                                ("Lift: jumps", m.lift_jumps.to_string()),
                                (
                                    "Lift: largest jump",
                                    format!("{:.0} counts", m.lift_max_jump_counts),
                                ),
                            ];
                            for (k, v) in rows {
                                ui.label(egui::RichText::new(k).weak());
                                ui.label(v);
                                ui.end_row();
                            }
                        });
                });
        }

        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(
                "Use the JSON / CSV export in the sidebar to save this report together with any other test results.",
            )
            .weak()
            .size(11.0),
        );

        // Keep the phase list handy below the report
        ui.add_space(15.0);
        ui.collapsing("Steps in this check-up", |ui| {
            for (i, phase) in self.engine.phases().iter().enumerate() {
                let done = report.phases_completed.contains(phase);
                ui.label(format!(
                    "{} {}. {} — {}",
                    if done { "✔" } else { "○" },
                    i + 1,
                    phase.title(),
                    phase.instruction()
                ));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::MouseWheelUnit::{Line, Page, Point};

    fn run_frame(
        ctx: &egui::Context,
        panel: &mut AutoTestPanel,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 900.0),
            )),
            events,
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            panel.process_input(&ctx, &[], false);
            panel.ui(ui, &ctx, false);
        })
    }

    fn button_at(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn button(button: egui::PointerButton, pressed: bool) -> egui::Event {
        button_at(egui::pos2(600.0, 500.0), button, pressed)
    }

    /// Centre of the first text drawn as exactly `label`.
    fn text_center(out: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
        fn find(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(t) if t.galley.text() == label => {
                    Some(t.galley.rect.translate(t.pos.to_vec2()).center())
                }
                egui::Shape::Vec(v) => v.iter().find_map(|s| find(s, label)),
                _ => None,
            }
        }
        out.shapes.iter().find_map(|c| find(&c.shape, label))
    }

    /// Click with the pointer at `pos`: one frame down, one frame up.
    fn click_at(
        ctx: &egui::Context,
        panel: &mut AutoTestPanel,
        pos: egui::Pos2,
        b: egui::PointerButton,
    ) {
        run_frame(
            ctx,
            panel,
            vec![egui::Event::PointerMoved(pos), button_at(pos, b, true)],
        );
        run_frame(ctx, panel, vec![button_at(pos, b, false)]);
    }

    #[test]
    fn clicking_where_run_again_was_does_not_abort_the_click_step() {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::default());
        let mut panel = AutoTestPanel::new();
        // Start a run with the button, stop it with Esc to get "Run Again"
        let out = run_frame(&ctx, &mut panel, Vec::new());
        let start = text_center(&out, "Start Diagnostics").expect("start button");
        click_at(&ctx, &mut panel, start, egui::PointerButton::Primary);
        assert!(panel.is_running());
        let esc = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        run_frame(&ctx, &mut panel, vec![esc]);
        assert!(!panel.is_running());
        let out = run_frame(&ctx, &mut panel, Vec::new());
        let again = text_center(&out, "Run Again").expect("run again button");
        click_at(&ctx, &mut panel, again, egui::PointerButton::Primary);
        assert!(panel.is_running());

        // Without raw input the click step follows the rest step directly,
        // with the pointer still where Run Again was.
        panel.tick_for_test(7.5);
        assert_eq!(panel.engine.current_phase(), Some(Phase::Clicks));
        for b in [egui::PointerButton::Primary; 20]
            .into_iter()
            .chain([egui::PointerButton::Secondary; 5])
        {
            click_at(&ctx, &mut panel, again, b);
            assert!(
                panel.is_running(),
                "a click during the click step stopped the run"
            );
        }
        assert_eq!(panel.engine.current_phase(), Some(Phase::Scroll));
    }

    fn wheel(unit: egui::MouseWheelUnit, dy: f32) -> egui::Event {
        egui::Event::MouseWheel {
            unit,
            delta: egui::vec2(0.0, dy),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn framework_input_counts_whole_notches_and_skips_timing_checks() {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::default());
        let mut panel = AutoTestPanel::new();
        panel.start_for_test(false);
        panel.use_line_wheel_for_test();
        assert_eq!(
            panel.engine.phases(),
            &[Phase::Rest, Phase::Clicks, Phase::Scroll]
        );
        // Skip the rest step on the engine's clock
        panel.tick_for_test(7.5);
        assert_eq!(panel.engine.current_phase(), Some(Phase::Clicks));
        // Quick clicks: press and release arrive in the same frame
        for b in [egui::PointerButton::Primary; 20]
            .into_iter()
            .chain([egui::PointerButton::Secondary; 5])
        {
            run_frame(&ctx, &mut panel, vec![button(b, true), button(b, false)]);
        }
        assert_eq!(panel.engine.current_phase(), Some(Phase::Scroll));
        // Hi-res wheel: quarter-line steps, plus one tiny backwards wobble
        for _ in 0..40 {
            run_frame(&ctx, &mut panel, vec![wheel(Line, -0.25)]);
        }
        run_frame(&ctx, &mut panel, vec![wheel(Line, 0.1)]);
        for _ in 0..10 {
            run_frame(
                &ctx,
                &mut panel,
                vec![wheel(Point, 20.0), wheel(Point, 20.0)],
            );
        }
        assert!(!panel.is_running());
        let r = panel.report().expect("report");
        assert_eq!(r.metrics.left_clicks, 20);
        assert_eq!(r.metrics.right_clicks, 5);
        assert_eq!(r.metrics.scroll_down, 10);
        assert_eq!(r.metrics.scroll_up, 10);
        assert_eq!(r.metrics.scroll_reversals, 0);
        assert_eq!(r.metrics.micro_presses, 0);
        assert_eq!(r.metrics.bounce_events, 0);
        assert!(!r.timing_reliable);
        assert_eq!(r.verdict, Verdict::Healthy, "{:?}", r.findings);
        // The report view renders, including the limited-check note
        run_frame(&ctx, &mut panel, Vec::new());
    }

    #[test]
    fn whole_notches_count_once() {
        let mut w = WheelAccumulator::default();
        assert_eq!(w.add(Line, -1.0, 40.0), -1);
        assert_eq!(w.add(Line, 3.0, 40.0), 3);
        assert_eq!(w.add(Page, -1.0, 40.0), -1);
    }

    #[test]
    fn high_resolution_steps_add_up_to_one_notch() {
        let mut w = WheelAccumulator::default();
        // A hi-res wheel sending quarter notches
        let total: i32 = (0..8).map(|_| w.add(Line, 0.25, 40.0)).sum();
        assert_eq!(total, 2);
        // Smooth scrolling in points: 40 points per line
        let mut w = WheelAccumulator::default();
        let total: i32 = (0..10).map(|_| w.add(Point, -12.0, 40.0)).sum();
        assert_eq!(total, -3);
    }

    #[test]
    fn macos_line_events_are_one_notch_each_whatever_their_size() {
        let mut w = WheelAccumulator::new(true);
        // A slow notch is a fraction of a line, a fast one several lines
        assert_eq!(w.add(Line, -0.1, 40.0), -1);
        assert_eq!(w.add(Line, -4.5, 40.0), -1);
        assert_eq!(w.add(Line, 0.3, 40.0), 1);
        // Smooth-scrolling devices still add up point deltas
        assert_eq!(w.add(Point, 25.0, 40.0), 0);
        assert_eq!(w.add(Point, 25.0, 40.0), 1);
    }

    #[test]
    fn small_backwards_wobble_is_not_a_notch() {
        let mut w = WheelAccumulator::default();
        assert_eq!(w.add(Line, -0.75, 40.0), 0);
        assert_eq!(w.add(Line, 0.1, 40.0), 0);
        // The leftover from the other direction does not delay a real notch
        assert_eq!(w.add(Line, 0.95, 40.0), 1);
        assert_eq!(w.add(Line, f32::NAN, 40.0), 0);
    }
}
