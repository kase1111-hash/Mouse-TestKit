//! Automated diagnostics panel
//!
//! Drives `mouse_testkit::analysis::diagnostics::AutoTest`: a guided,
//! timed sequence of phases that looks for faults on its own (phantom input
//! at rest, stutters and unstable polling, switch bounce, sticky or stuck
//! buttons, scroll encoder skips, lift-off jumps) and produces a report.
//!
//! The engine is fed from `App::ui` every frame regardless of which panel
//! is visible, so navigating away does not lose events.

use eframe::egui;
use std::time::Instant;

use crate::input_bridge::{RawButton, RawInputEvent, RawInputKind};
use crate::theme::ThemeColors;
use mouse_testkit::analysis::diagnostics::{AutoTest, Config, Report, Severity, Verdict};
use mouse_testkit::types::{MouseButton, MouseEvent};

pub struct AutoTestPanel {
    engine: AutoTest,
    /// Wall-clock origin for converting timestamps to seconds.
    epoch: Instant,
    /// Whether the last run used precise raw-input timestamps.
    last_run_raw: bool,
    show_metrics: bool,
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
                    egui::Event::MouseWheel { delta, .. } if delta.y != 0.0 => {
                        self.engine.feed(
                            now,
                            &MouseEvent::Scroll {
                                delta: delta.y.signum() as i32,
                            },
                        );
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
        self.engine.start(self.secs(Instant::now()));
    }

    #[cfg(test)]
    pub(crate) fn start_for_test(&mut self, has_bridge: bool) {
        self.start(has_bridge);
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
                    "Note: Raw input unavailable — timing checks (polling rate, stutter) will be skipped",
                )
                .color(egui::Color32::YELLOW)
                .size(11.0),
            );
        }
        ui.add_space(15.0);

        ui.horizontal(|ui| {
            if self.engine.is_running() {
                if ui.button("Abort").clicked() {
                    self.engine.abort();
                }
            } else if ui
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
        });

        ui.add_space(20.0);

        if self.engine.is_running() {
            self.ui_running(ui);
            ctx.request_repaint();
        } else if let Some(report) = self.engine.report().cloned() {
            self.ui_report(ui, &report);
        } else {
            self.ui_idle(ui);
        }
    }

    fn ui_idle(&self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("What happens").strong());
                ui.add_space(5.0);
                for (i, phase) in self.engine.phases().iter().enumerate() {
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
                    note.push_str(" — framework input, timing checks skipped");
                }
                ui.label(egui::RichText::new(note).weak());
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
                                ("Movement: report intervals", m.move_intervals.to_string()),
                                (
                                    "Movement: polling rate",
                                    m.polling_hz
                                        .map(|h| {
                                            format!(
                                                "{} Hz (nominal {} Hz)",
                                                h,
                                                m.polling_nominal_hz.unwrap_or(h)
                                            )
                                        })
                                        .unwrap_or_else(|| "n/a".to_string()),
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
