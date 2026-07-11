//! Polling rate monitoring panel
//!
//! Measures and displays the mouse's polling rate in real-time.
//! Shows current, min, max, and average Hz with a live graph.
//!
//! When an InputBridge is available, uses raw input events for accurate
//! measurement. Falls back to egui pointer delta on unsupported platforms.

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use std::collections::VecDeque;
use std::time::Instant;

use crate::export::PollingRateExport;
use crate::input_bridge::{RawInputEvent, RawInputKind};

/// Panel for measuring mouse polling rate.
///
/// Calculates polling rate by counting mouse movement events per second.
/// Displays real-time statistics and a scrolling graph of polling rate history.
pub struct PollingPanel {
    is_running: bool,
    current_hz: u32,
    min_hz: u32,
    max_hz: u32,
    avg_hz: f64,
    samples: u32,
    history: VecDeque<f64>,
    /// Timestamps of recent mouse movements for calculating Hz
    event_times: VecDeque<Instant>,
    /// Last time we calculated and recorded a Hz sample
    last_hz_calc: Option<Instant>,
}

impl PollingPanel {
    pub fn new() -> Self {
        Self {
            is_running: false,
            current_hz: 0,
            min_hz: u32::MAX,
            max_hz: 0,
            avg_hz: 0.0,
            samples: 0,
            history: VecDeque::with_capacity(200),
            event_times: VecDeque::with_capacity(2000),
            last_hz_calc: None,
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
        ui.heading("Polling Rate Monitor");
        ui.add_space(5.0);
        ui.label("Measures your mouse's polling rate in real-time.");
        if !has_bridge {
            ui.label(
                egui::RichText::new(
                    "Note: Raw input unavailable — using framework input (reduced accuracy)",
                )
                .color(egui::Color32::YELLOW)
                .size(11.0),
            );
        }
        ui.add_space(15.0);

        // Control buttons
        ui.horizontal(|ui| {
            if self.is_running {
                if ui.button("Stop").clicked() {
                    self.is_running = false;
                }
            } else if ui.button("Start").clicked() {
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
                ui.horizontal(|ui| {
                    self.stat_box(
                        ui,
                        "Current",
                        &format!("{} Hz", self.current_hz),
                        egui::Color32::WHITE,
                    );
                    ui.add_space(30.0);
                    self.stat_box(
                        ui,
                        "Min",
                        &format!(
                            "{} Hz",
                            if self.min_hz == u32::MAX {
                                0
                            } else {
                                self.min_hz
                            }
                        ),
                        egui::Color32::LIGHT_BLUE,
                    );
                    ui.add_space(30.0);
                    self.stat_box(
                        ui,
                        "Max",
                        &format!("{} Hz", self.max_hz),
                        egui::Color32::LIGHT_GREEN,
                    );
                    ui.add_space(30.0);
                    self.stat_box(
                        ui,
                        "Avg",
                        &format!("{:.1} Hz", self.avg_hz),
                        egui::Color32::YELLOW,
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
                ui.label("2. Move your mouse around to generate events");
                ui.label("3. The graph shows real-time polling rate");
                ui.add_space(5.0);
                ui.label(
                    egui::RichText::new(
                        "Common polling rates: 125Hz, 250Hz, 500Hz, 1000Hz, 4000Hz, 8000Hz",
                    )
                    .weak(),
                );
            });

        // Capture input and calculate polling rate
        if self.is_running {
            if has_bridge {
                self.process_raw_events(raw_events);
            } else {
                self.process_egui_fallback(ctx);
            }
            self.calculate_hz();
        }
    }

    /// Process raw input events from the InputBridge (accurate)
    fn process_raw_events(&mut self, raw_events: &[RawInputEvent]) {
        let now = Instant::now();
        for event in raw_events {
            if let RawInputKind::Move { .. } = &event.kind {
                self.event_times.push_back(event.timestamp);
            }
        }
        // Prune events older than 1 second
        while let Some(front) = self.event_times.front() {
            if now.duration_since(*front).as_secs_f64() > 1.0 {
                self.event_times.pop_front();
            } else {
                break;
            }
        }
    }

    /// Fallback: read egui pointer delta (frame-rate limited)
    fn process_egui_fallback(&mut self, ctx: &egui::Context) {
        let delta = ctx.input(|i| i.pointer.delta());
        let now = Instant::now();
        if delta.x != 0.0 || delta.y != 0.0 {
            self.event_times.push_back(now);
            while let Some(front) = self.event_times.front() {
                if now.duration_since(*front).as_secs_f64() > 1.0 {
                    self.event_times.pop_front();
                } else {
                    break;
                }
            }
        }
    }

    /// Calculate Hz from event count in the last second (runs every 100ms)
    fn calculate_hz(&mut self) {
        let now = Instant::now();
        let should_calc = match self.last_hz_calc {
            None => true,
            Some(last) => now.duration_since(last).as_millis() >= 100,
        };

        if should_calc && self.event_times.len() >= 2 {
            let hz = self.event_times.len() as u32;
            if hz > 0 {
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
            self.last_hz_calc = Some(now);
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
        self.event_times.clear();
        self.last_hz_calc = None;
    }

    fn reset(&mut self) {
        self.is_running = false;
        self.current_hz = 0;
        self.min_hz = u32::MAX;
        self.max_hz = 0;
        self.avg_hz = 0.0;
        self.samples = 0;
        self.history.clear();
        self.event_times.clear();
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
