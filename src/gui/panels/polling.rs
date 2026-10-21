//! Polling rate monitoring panel
//!
//! Measures and displays the mouse's polling rate in real-time.
//! Shows current, min, max, and average Hz with a live graph.
//!
//! Needs raw input from the InputBridge. The window-system pointer (the
//! fallback when raw input is unavailable) is delivered once per display
//! frame, so it measures the screen refresh rate, not the mouse; the panel
//! says so instead of showing a number.

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use std::collections::VecDeque;
use std::time::Instant;

use crate::export::PollingRateExport;
use crate::input_bridge::{RawInputEvent, RawInputKind};
use mouse_testkit::analysis::polling::{estimate_hz, nominal_rate, saturated_intervals_ms};

/// Fast-movement intervals needed inside the one-second window before a Hz
/// value is published. Their median is not moved by a few late reports, and
/// even at 125 Hz this is only 160 ms of fast movement.
const MIN_FAST_INTERVALS: usize = 20;

/// Tell the user plainly that a measurement needs raw input.
///
/// Without raw input the panels only see the window system's pointer: one
/// update per display frame, already scaled by OS pointer acceleration, and
/// only while the pointer is over this window.
pub(super) fn raw_input_required_notice(ui: &mut egui::Ui, what: &str) {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(70, 50, 20))
        .inner_margin(12.0)
        .corner_radius(8.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Raw input unavailable - not measurable here")
                    .strong()
                    .color(egui::Color32::YELLOW),
            );
            ui.label(format!(
                "Without raw input the app only sees the window system's pointer: one update per display frame (e.g. 60-240 Hz), already scaled by OS pointer acceleration. {} cannot be measured from that, so no reading is shown.",
                what
            ));
            ui.label(
                egui::RichText::new(
                    "On Linux, add your user to the 'input' group (sudo usermod -aG input $USER) and log in again. Raw input is not available on macOS yet.",
                )
                .weak()
                .size(11.0),
            );
        });
}

/// Panel for measuring mouse polling rate.
///
/// Calculates the polling rate from the median interval between consecutive
/// reports in the last second, using only intervals where the mouse was
/// moving fast enough to report at every poll (see
/// `analysis::polling::saturated_intervals_ms`). Slow movement and reversals
/// legitimately skip polls; windows without enough fast movement publish
/// nothing, so they cannot drag Min/Avg/Nominal down. Displays real-time
/// statistics and a scrolling graph of polling rate history.
pub struct PollingPanel {
    is_running: bool,
    current_hz: u32,
    /// Whether the most recent window had enough fast movement to measure.
    live: bool,
    min_hz: u32,
    max_hz: u32,
    avg_hz: f64,
    samples: u32,
    history: VecDeque<f64>,
    /// Recent motion reports (time, dx, dy) for calculating Hz
    reports: VecDeque<(Instant, i32, i32)>,
    /// Last time we calculated and recorded a Hz sample
    last_hz_calc: Option<Instant>,
}

impl PollingPanel {
    pub fn new() -> Self {
        Self {
            is_running: false,
            current_hz: 0,
            live: false,
            min_hz: u32::MAX,
            max_hz: 0,
            avg_hz: 0.0,
            samples: 0,
            history: VecDeque::with_capacity(200),
            reports: VecDeque::with_capacity(2000),
            last_hz_calc: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.is_running
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _ctx: &egui::Context,
        raw_events: &[RawInputEvent],
        has_bridge: bool,
    ) {
        ui.heading("Polling Rate Monitor");
        ui.add_space(5.0);
        ui.label("Measures your mouse's polling rate in real-time.");
        if !has_bridge {
            ui.add_space(5.0);
            raw_input_required_notice(ui, "The mouse's polling rate");
            self.is_running = false;
        }
        ui.add_space(15.0);

        // Control buttons
        ui.horizontal(|ui| {
            if self.is_running {
                if ui.button("Stop").clicked() {
                    self.is_running = false;
                }
            } else if ui
                .add_enabled(has_bridge, egui::Button::new("Start"))
                .clicked()
            {
                self.start();
            }

            if ui.button("Reset").clicked() {
                self.reset();
            }
        });

        ui.add_space(20.0);

        // Stats display
        egui::Frame::dark_canvas(ui.style())
            .inner_margin(20.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                let measured = self.samples > 0;
                let hz_or_dash = |hz: String| if measured { hz } else { "-".to_string() };
                ui.horizontal(|ui| {
                    self.stat_box(
                        ui,
                        "Current",
                        &if self.live {
                            format!("{} Hz", self.current_hz)
                        } else {
                            "-".to_string()
                        },
                        egui::Color32::WHITE,
                    );
                    ui.add_space(30.0);
                    self.stat_box(
                        ui,
                        "Min",
                        &hz_or_dash(format!("{} Hz", self.min_hz)),
                        egui::Color32::LIGHT_BLUE,
                    );
                    ui.add_space(30.0);
                    self.stat_box(
                        ui,
                        "Max",
                        &hz_or_dash(format!("{} Hz", self.max_hz)),
                        egui::Color32::LIGHT_GREEN,
                    );
                    ui.add_space(30.0);
                    self.stat_box(
                        ui,
                        "Avg",
                        &hz_or_dash(format!("{:.1} Hz", self.avg_hz)),
                        egui::Color32::YELLOW,
                    );
                    ui.add_space(30.0);
                    self.stat_box(
                        ui,
                        "Nominal",
                        &if self.samples > 0 {
                            format!("{} Hz", nominal_rate(self.avg_hz))
                        } else {
                            "-".to_string()
                        },
                        egui::Color32::LIGHT_GRAY,
                    );
                    ui.add_space(30.0);
                    self.stat_box(
                        ui,
                        "Samples",
                        &format!("{}", self.samples),
                        egui::Color32::GRAY,
                    );
                });
            });

        if self.is_running && !self.live {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "Move the mouse faster. A reading is only taken while it moves fast enough to send a report at every poll; slow movement and direction changes legitimately send fewer.",
                )
                .color(egui::Color32::YELLOW),
            );
        }

        ui.add_space(20.0);

        // Graph
        ui.heading("Polling Rate History");
        let points: PlotPoints = self
            .history
            .iter()
            .enumerate()
            .map(|(i, &hz)| [i as f64, hz])
            .collect();

        let line = Line::new("Polling Rate", points).color(egui::Color32::from_rgb(100, 200, 255));

        Plot::new("polling_plot")
            .height(250.0)
            .show_axes(true)
            .show_grid(true)
            .allow_drag(false)
            .allow_zoom(false)
            .show(ui, |plot_ui| {
                plot_ui.line(line);
            });

        ui.add_space(20.0);

        // Instructions
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Instructions").strong());
                ui.label("1. Click 'Start' to begin monitoring");
                ui.label("2. Move the mouse quickly and continuously, e.g. in circles");
                ui.label("3. The graph shows the polling rate of each fast-movement window");
                ui.add_space(5.0);
                ui.label(
                    egui::RichText::new(
                        "Only fast movement counts: at 4000-8000 Hz a report holds very few counts, so move briskly or use 1600+ DPI. Slow-movement windows are skipped and do not lower Min/Avg/Nominal.",
                    )
                    .weak(),
                );
                ui.label(
                    egui::RichText::new(
                        "Common polling rates: 125Hz, 250Hz, 500Hz, 1000Hz, 4000Hz, 8000Hz",
                    )
                    .weak(),
                );
            });

        // Capture input and calculate polling rate (raw input only)
        if self.is_running && has_bridge {
            let now = Instant::now();
            self.process_raw_events(raw_events, now);
            self.calculate_hz(now);
        }
    }

    /// Keep the motion reports of the last second.
    fn process_raw_events(&mut self, raw_events: &[RawInputEvent], now: Instant) {
        for event in raw_events {
            if let RawInputKind::Move { dx, dy } = event.kind {
                self.reports.push_back((event.timestamp, dx, dy));
            }
        }
        while let Some(&(front, ..)) = self.reports.front() {
            if now.saturating_duration_since(front).as_secs_f64() > 1.0 {
                self.reports.pop_front();
            } else {
                break;
            }
        }
    }

    /// Calculate Hz from the fast-movement intervals of the last second
    /// (runs every 100ms). Uses the median so that a partially filled window
    /// or a single missed report does not distort the reading. A window
    /// without enough fast movement publishes nothing.
    fn calculate_hz(&mut self, now: Instant) {
        let should_calc = match self.last_hz_calc {
            None => true,
            Some(last) => now.saturating_duration_since(last).as_millis() >= 100,
        };
        if !should_calc {
            return;
        }
        self.last_hz_calc = Some(now);

        let Some(&(origin, ..)) = self.reports.front() else {
            self.live = false;
            return;
        };
        let reports: Vec<(f64, i32, i32)> = self
            .reports
            .iter()
            .map(|&(t, dx, dy)| {
                (
                    t.saturating_duration_since(origin).as_secs_f64() * 1000.0,
                    dx,
                    dy,
                )
            })
            .collect();
        let fast = saturated_intervals_ms(&reports);
        match estimate_hz(&fast, MIN_FAST_INTERVALS) {
            Some(hz) => {
                self.live = true;
                self.current_hz = hz;
                self.min_hz = self.min_hz.min(hz);
                self.max_hz = self.max_hz.max(hz);
                self.samples += 1;
                self.avg_hz =
                    (self.avg_hz * (self.samples - 1) as f64 + hz as f64) / self.samples as f64;
                self.history.push_back(hz as f64);
                if self.history.len() > 200 {
                    self.history.pop_front();
                }
            }
            None => self.live = false,
        }
    }

    fn stat_box(&self, ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32) {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(label).weak().size(12.0));
            ui.label(egui::RichText::new(value).color(color).size(24.0).strong());
        });
    }

    fn start(&mut self) {
        self.is_running = true;
        self.live = false;
        self.reports.clear();
        self.last_hz_calc = None;
    }

    #[cfg(test)]
    pub(crate) fn start_for_test(&mut self) {
        self.start();
    }

    fn reset(&mut self) {
        self.is_running = false;
        self.current_hz = 0;
        self.live = false;
        self.min_hz = u32::MAX;
        self.max_hz = 0;
        self.avg_hz = 0.0;
        self.samples = 0;
        self.history.clear();
        self.reports.clear();
        self.last_hz_calc = None;
    }

    pub fn export(&self) -> Option<PollingRateExport> {
        if self.samples == 0 {
            return None;
        }
        Some(PollingRateExport {
            current_hz: self.current_hz,
            min_hz: if self.min_hz == u32::MAX {
                0
            } else {
                self.min_hz
            },
            max_hz: self.max_hz,
            avg_hz: self.avg_hz,
            samples: self.samples,
            history: self.history.iter().cloned().collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn mv(t: Instant, dx: i32, dy: i32) -> RawInputEvent {
        RawInputEvent {
            kind: RawInputKind::Move { dx, dy },
            timestamp: t,
        }
    }

    /// Feed `events` frame by frame (every 16 ms of event time) and run the
    /// 100 ms calculation as the GUI would.
    fn drive(panel: &mut PollingPanel, events: &[RawInputEvent]) {
        for frame in events
            .chunk_by(|a, b| b.timestamp.duration_since(a.timestamp) < Duration::from_millis(16))
        {
            let now = frame.last().unwrap().timestamp;
            panel.process_raw_events(frame, now);
            panel.calculate_hz(now);
        }
    }

    #[test]
    fn slow_movement_is_not_published_as_a_low_rate() {
        // A 1 kHz mouse moved slowly: one count every ~3 ms for a whole second
        let mut p = PollingPanel::new();
        p.start();
        let t0 = Instant::now();
        let events: Vec<_> = (0..330)
            .map(|i| mv(t0 + Duration::from_millis(3 * i), 1, 0))
            .collect();
        drive(&mut p, &events);
        assert!(p.export().is_none(), "slow motion produced a Hz reading");
        assert!(!p.live);
    }

    #[test]
    fn slow_passes_and_reversals_do_not_pollute_min_avg_nominal() {
        // 1 kHz: alternate 400 ms fast passes with slow turnarounds (1-count
        // reports 2-12 ms apart) for 6 seconds.
        let mut p = PollingPanel::new();
        p.start();
        let mut t = Instant::now();
        let mut events = Vec::new();
        let mut dir = 1;
        for _ in 0..12 {
            for _ in 0..400 {
                t += Duration::from_millis(1);
                events.push(mv(t, 4 * dir, 1));
            }
            for gap in [2, 3, 5, 8, 12, 12, 12, 8, 5, 3, 2] {
                t += Duration::from_millis(gap);
                events.push(mv(t, dir, 0));
            }
            dir = -dir;
            // a slow drag in between passes
            for _ in 0..30 {
                t += Duration::from_millis(4);
                events.push(mv(t, dir, 0));
            }
        }
        drive(&mut p, &events);
        let e = p.export().expect("fast passes produce readings");
        assert!(e.min_hz >= 990, "min polluted: {}", e.min_hz);
        assert!(e.max_hz <= 1010, "max: {}", e.max_hz);
        assert!((e.avg_hz - 1000.0).abs() < 5.0, "avg {}", e.avg_hz);
        assert_eq!(nominal_rate(e.avg_hz), 1000);
    }

    #[test]
    fn eight_khz_fast_movement_reads_8000() {
        let mut p = PollingPanel::new();
        p.start();
        let t0 = Instant::now();
        let events: Vec<_> = (0..8000u32)
            .map(|i| mv(t0 + Duration::from_micros(125 * i as u64), 3, -2))
            .collect();
        drive(&mut p, &events);
        let e = p.export().unwrap();
        assert!((7900..=8100).contains(&e.min_hz), "{}", e.min_hz);
        assert_eq!(nominal_rate(e.avg_hz), 8000);
    }

    #[test]
    fn fallback_never_measures() {
        let ctx = egui::Context::default();
        let mut p = PollingPanel::new();
        p.start();
        let t0 = Instant::now();
        let events: Vec<_> = (0..300)
            .map(|i| mv(t0 + Duration::from_millis(i), 3, 0))
            .collect();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let c = ui.ctx().clone();
            p.ui(ui, &c, &events, false);
        });
        assert!(!p.is_running());
        assert!(p.export().is_none());
    }
}
