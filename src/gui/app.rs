//! Main application module for Mouse TRAP GUI
//!
//! Contains the primary application struct and UI rendering logic.
//! Manages navigation between test panels, theming, and result export.

use eframe::egui;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::export::{ExportInfo, TestResultsExport};
use crate::input_bridge::{DeviceChoice, InputBridge};
use crate::panels::{
    AccelPanel, AutoTestPanel, ClickPanel, DoubleClickPanel, DpiPanel, JitterPanel, PollingPanel,
    ScrollPanel, StutterPanel,
};
use crate::theme::{self, ThemeColors};

/// How long after the last settings change the config is written to disk.
const CONFIG_SAVE_DEBOUNCE: Duration = Duration::from_millis(750);
/// Repaint interval while idle, so the raw-input channel keeps being drained
/// even when the pointer is outside the window.
const IDLE_REPAINT_INTERVAL: Duration = Duration::from_millis(250);

/// Represents the currently active test or view in the application.
#[derive(PartialEq, Clone, Copy)]
pub enum ActiveTest {
    Dashboard,
    AutoTest,
    PollingRate,
    Stutter,
    ClickResponse,
    ClickSticky,
    LiftOff,
    ScrollWheel,
    Dpi,
    AngleSnap,
    Acceleration,
    DoubleClick,
    Jitter,
}

/// Main application struct for Mouse TRAP.
///
/// Manages all test panels, navigation state, theming, and configuration.
/// Implements `eframe::App` to integrate with the egui framework.
pub struct MouseTestKitApp {
    /// Currently displayed test panel
    active_test: ActiveTest,
    /// Polling rate measurement panel
    polling_panel: PollingPanel,
    /// Stutter detection panel
    stutter_panel: StutterPanel,
    /// Click testing panel (response, sticky, liftoff)
    click_panel: ClickPanel,
    /// Sensor jitter analysis panel
    jitter_panel: JitterPanel,
    /// DPI accuracy verification panel
    dpi_panel: DpiPanel,
    /// Acceleration and angle snapping panel
    accel_panel: AccelPanel,
    /// Double-click detection panel
    double_click_panel: DoubleClickPanel,
    /// Scroll wheel testing panel
    scroll_panel: ScrollPanel,
    /// Automated diagnostics panel
    auto_test_panel: AutoTestPanel,
    /// Whether to show the About dialog
    show_about: bool,
    /// Status message from last export operation
    export_status: Option<String>,
    /// User configuration
    config: Config,
    /// When settings last changed and still need saving (debounced)
    config_dirty_since: Option<Instant>,
    /// Background raw input thread (None on unsupported platforms)
    input_bridge: Option<InputBridge>,
}

impl MouseTestKitApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Always dark, styled once (the style persists across frames)
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        theme::setup_custom_style(&cc.egui_ctx);

        // Start background raw input thread
        let input_bridge = InputBridge::start();
        if input_bridge.is_none() {
            eprintln!("Note: Raw input not available. GUI tests will use framework input.");
        }

        Self::with_parts(Config::load(), input_bridge)
    }

    /// Build the app from a loaded configuration and an (optional) raw input
    /// bridge.
    fn with_parts(config: Config, input_bridge: Option<InputBridge>) -> Self {
        // Create panels with config values
        let mut stutter_panel = StutterPanel::new();
        stutter_panel.set_threshold_multiplier(config.stutter_threshold_multiplier);

        let mut dpi_panel = DpiPanel::new();
        dpi_panel.set_target_dpi(config.dpi_target);
        dpi_panel.set_target_distance(config.dpi_distance_inches);

        let mut double_click_panel = DoubleClickPanel::new();
        double_click_panel.set_threshold_ms(config.double_click_threshold_ms);

        Self {
            active_test: ActiveTest::Dashboard,
            polling_panel: PollingPanel::new(),
            stutter_panel,
            click_panel: ClickPanel::new(),
            jitter_panel: JitterPanel::new(),
            dpi_panel,
            accel_panel: AccelPanel::new(),
            double_click_panel,
            scroll_panel: ScrollPanel::new(),
            auto_test_panel: AutoTestPanel::new(),
            show_about: false,
            export_status: None,
            config,
            config_dirty_since: None,
            input_bridge,
        }
    }

    fn any_test_running(&self) -> bool {
        self.polling_panel.is_running()
            || self.stutter_panel.is_running()
            || self.click_panel.is_running()
            || self.jitter_panel.is_running()
            || self.dpi_panel.is_running()
            || self.accel_panel.is_running()
            || self.double_click_panel.is_running()
            || self.scroll_panel.is_running()
            || self.auto_test_panel.is_running()
    }

    /// Collect current settings and update config
    fn update_config(&mut self) {
        self.config.stutter_threshold_multiplier = self.stutter_panel.get_threshold_multiplier();
        self.config.dpi_target = self.dpi_panel.get_target_dpi();
        self.config.dpi_distance_inches = self.dpi_panel.get_target_distance();
        self.config.double_click_threshold_ms = self.double_click_panel.get_threshold_ms();
    }

    /// Save config to disk once settings have stopped changing for
    /// [`CONFIG_SAVE_DEBOUNCE`] (or immediately when `force` is set).
    fn save_config_if_needed(&mut self, force: bool) {
        let due = match self.config_dirty_since {
            Some(since) => force || since.elapsed() >= CONFIG_SAVE_DEBOUNCE,
            None => false,
        };
        if due {
            self.update_config();
            if let Err(e) = self.config.save() {
                eprintln!("Warning: Failed to save config: {}", e);
            }
            self.config_dirty_since = None;
        }
    }

    fn collect_results(&self) -> TestResultsExport {
        TestResultsExport {
            export_info: ExportInfo::new(),
            polling_rate: self.polling_panel.export(),
            stutter: self.stutter_panel.export(),
            click_response: self.click_panel.export_response(),
            click_sticky: self.click_panel.export_sticky(),
            liftoff: self.click_panel.export_liftoff(),
            jitter: self.jitter_panel.export(),
            double_click: self.double_click_panel.export(),
            dpi: self.dpi_panel.export(),
            acceleration: self.accel_panel.export_accel(),
            angle_snap: self.accel_panel.export_angle(),
            scroll: self.scroll_panel.export(),
            auto_diagnostics: self.auto_test_panel.report().cloned(),
        }
    }

    fn export_json(&mut self) {
        let results = self.collect_results();
        match results.to_json() {
            Ok(json) => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Export Results as JSON")
                    .set_file_name("mouse-trap-results.json")
                    .add_filter("JSON", &["json"])
                    .save_file()
                {
                    match std::fs::write(&path, json) {
                        Ok(_) => {
                            self.export_status = Some(format!("Exported to {}", path.display()))
                        }
                        Err(e) => self.export_status = Some(format!("Error: {}", e)),
                    }
                }
            }
            Err(e) => self.export_status = Some(format!("Error: {}", e)),
        }
    }

    fn export_csv(&mut self) {
        let results = self.collect_results();
        let csv = results.to_csv();
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Export Results as CSV")
            .set_file_name("mouse-trap-results.csv")
            .add_filter("CSV", &["csv"])
            .save_file()
        {
            match std::fs::write(&path, csv) {
                Ok(_) => self.export_status = Some(format!("Exported to {}", path.display())),
                Err(e) => self.export_status = Some(format!("Error: {}", e)),
            }
        }
    }

    fn render_sidebar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("sidebar")
            .resizable(false)
            .default_size(200.0)
            .frame(
                egui::Frame::new()
                    .fill(ThemeColors::sidebar_bg())
                    .stroke(egui::Stroke::new(1.0, ThemeColors::border())),
            )
            .show(ui, |ui| {
                ui.add_space(20.0);

                // Logo/Title with mouse head icon
                ui.vertical_centered(|ui| {
                    // Draw mouse head icon (1 big circle + 2 ear circles)
                    let icon_size = 40.0;
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(icon_size, icon_size),
                        egui::Sense::hover(),
                    );
                    let painter = ui.painter();
                    let center = rect.center();

                    // Main head circle
                    let head_radius = 14.0;
                    painter.circle_filled(center, head_radius, ThemeColors::accent());

                    // Left ear
                    let ear_radius = 7.0;
                    let ear_offset_x = 12.0;
                    let ear_offset_y = -10.0;
                    painter.circle_filled(
                        egui::pos2(center.x - ear_offset_x, center.y + ear_offset_y),
                        ear_radius,
                        ThemeColors::accent(),
                    );

                    // Right ear
                    painter.circle_filled(
                        egui::pos2(center.x + ear_offset_x, center.y + ear_offset_y),
                        ear_radius,
                        ThemeColors::accent(),
                    );
                });

                ui.add_space(24.0);

                // Separator with gradient look
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    let rect = ui.available_rect_before_wrap();
                    let painter = ui.painter();
                    painter.line_segment(
                        [
                            egui::pos2(rect.left(), rect.top()),
                            egui::pos2(rect.right() - 20.0, rect.top()),
                        ],
                        egui::Stroke::new(1.0, ThemeColors::border()),
                    );
                    ui.add_space(ui.available_width());
                });

                ui.add_space(16.0);

                // Navigation section
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new("NAVIGATION")
                            .size(10.0)
                            .strong()
                            .color(ThemeColors::text_muted()),
                    );
                });
                ui.add_space(8.0);

                self.nav_button(ui, "Dashboard", ActiveTest::Dashboard, true);
                self.nav_button(ui, "Auto Diagnostics", ActiveTest::AutoTest, false);

                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new("CORE TESTS")
                            .size(10.0)
                            .strong()
                            .color(ThemeColors::text_muted()),
                    );
                });
                ui.add_space(8.0);

                self.nav_button(ui, "Polling Rate", ActiveTest::PollingRate, false);
                self.nav_button(ui, "Stutter Detection", ActiveTest::Stutter, false);
                self.nav_button(ui, "Click Response", ActiveTest::ClickResponse, false);
                self.nav_button(ui, "Click Stickiness", ActiveTest::ClickSticky, false);
                self.nav_button(ui, "Lift-Off Jump", ActiveTest::LiftOff, false);
                self.nav_button(ui, "Scroll Wheel", ActiveTest::ScrollWheel, false);

                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new("ADVANCED TESTS")
                            .size(10.0)
                            .strong()
                            .color(ThemeColors::text_muted()),
                    );
                });
                ui.add_space(8.0);

                self.nav_button(ui, "DPI Accuracy", ActiveTest::Dpi, false);
                self.nav_button(ui, "Angle Snapping", ActiveTest::AngleSnap, false);
                self.nav_button(ui, "Acceleration", ActiveTest::Acceleration, false);
                self.nav_button(ui, "Double-Click", ActiveTest::DoubleClick, false);
                self.nav_button(ui, "Jitter Test", ActiveTest::Jitter, false);

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(16.0);

                    // Bottom buttons row
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);

                        let about_btn = egui::Button::new(
                            egui::RichText::new("About")
                                .size(12.0)
                                .color(ThemeColors::text_secondary()),
                        )
                        .fill(ThemeColors::bg_glass())
                        .stroke(egui::Stroke::new(1.0, ThemeColors::border()))
                        .corner_radius(8.0)
                        .min_size(egui::vec2(60.0, 32.0));

                        if ui.add(about_btn).clicked() {
                            self.show_about = true;
                        }
                    });

                    ui.add_space(8.0);

                    // Export buttons row. Disabled during Auto Diagnostics: the
                    // file dialog blocks the window while the step timer runs.
                    let export_enabled = !self.auto_test_panel.is_running();
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);

                        let json_btn = egui::Button::new(
                            egui::RichText::new("JSON")
                                .size(11.0)
                                .color(ThemeColors::accent()),
                        )
                        .fill(ThemeColors::accent_dim())
                        .stroke(egui::Stroke::new(1.0, ThemeColors::accent()))
                        .corner_radius(6.0)
                        .min_size(egui::vec2(50.0, 28.0));

                        if ui
                            .add_enabled(export_enabled, json_btn)
                            .on_hover_text("Export all results to JSON")
                            .on_disabled_hover_text("Finish or abort Auto Diagnostics first")
                            .clicked()
                        {
                            self.export_json();
                        }

                        let csv_btn = egui::Button::new(
                            egui::RichText::new("CSV")
                                .size(11.0)
                                .color(ThemeColors::accent()),
                        )
                        .fill(ThemeColors::accent_dim())
                        .stroke(egui::Stroke::new(1.0, ThemeColors::accent()))
                        .corner_radius(6.0)
                        .min_size(egui::vec2(50.0, 28.0));

                        if ui
                            .add_enabled(export_enabled, csv_btn)
                            .on_hover_text("Export all results to CSV")
                            .on_disabled_hover_text("Finish or abort Auto Diagnostics first")
                            .clicked()
                        {
                            self.export_csv();
                        }
                    });

                    // Export status message
                    if let Some(ref status) = self.export_status {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add_space(16.0);
                            ui.label(egui::RichText::new(status).size(10.0).color(
                                if status.starts_with("Error") {
                                    egui::Color32::RED
                                } else {
                                    ThemeColors::success()
                                },
                            ));
                        });
                    }

                    ui.add_space(8.0);

                    // Separator
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        let rect = ui.available_rect_before_wrap();
                        let painter = ui.painter();
                        painter.line_segment(
                            [
                                egui::pos2(rect.left(), rect.top()),
                                egui::pos2(rect.right() - 20.0, rect.top()),
                            ],
                            egui::Stroke::new(1.0, ThemeColors::border()),
                        );
                        ui.add_space(ui.available_width());
                    });

                    ui.add_space(4.0);

                    // Export section header
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.label(
                            egui::RichText::new("EXPORT RESULTS")
                                .size(10.0)
                                .strong()
                                .color(ThemeColors::text_muted()),
                        );
                    });

                    ui.add_space(8.0);
                });
            });
    }

    fn nav_button(&mut self, ui: &mut egui::Ui, label: &str, test: ActiveTest, _is_home: bool) {
        let selected = self.active_test == test;

        ui.horizontal(|ui| {
            ui.add_space(12.0);

            let (bg_fill, stroke, text_color) = if selected {
                (
                    ThemeColors::sidebar_selected(),
                    egui::Stroke::new(1.0, ThemeColors::accent()),
                    ThemeColors::accent(),
                )
            } else {
                (
                    egui::Color32::TRANSPARENT,
                    egui::Stroke::NONE,
                    ThemeColors::text_secondary(),
                )
            };

            let button = egui::Button::new(egui::RichText::new(label).size(13.0).color(text_color))
                .fill(bg_fill)
                .stroke(stroke)
                .corner_radius(8.0)
                .min_size(egui::vec2(172.0, 32.0));

            let response = ui.add(button);

            if response.hovered() && !selected {
                // Draw hover effect
                let rect = response.rect;
                ui.painter().rect_filled(
                    rect,
                    8.0,
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 5),
                );
            }

            if response.clicked() {
                self.active_test = test;
            }

            // Accent indicator for selected item
            if selected {
                let rect = response.rect;
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(rect.left(), rect.top() + 6.0),
                        egui::vec2(3.0, rect.height() - 12.0),
                    ),
                    2.0,
                    ThemeColors::accent(),
                );
            }
        });

        ui.add_space(2.0);
    }

    fn render_dashboard(&mut self, ui: &mut egui::Ui) {
        ui.label(theme::heading_style("Welcome to Mouse TRAP"));
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(
                "Test Response And Positioning — precision diagnostics for your mouse.",
            )
            .color(ThemeColors::text_secondary()),
        );

        ui.add_space(28.0);

        // Auto diagnostics call-to-action
        ui.label(theme::subheading_style("CHECK-UP"));
        ui.add_space(12.0);
        theme::card_frame(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("🔍").size(28.0));
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("Auto Diagnostics")
                            .strong()
                            .size(15.0)
                            .color(ThemeColors::text_primary()),
                    );
                    ui.label(
                        egui::RichText::new(
                            "One-minute guided check-up that looks for faults by itself: phantom input, stutters, switch bounce, sticky buttons, scroll skips and lift-off jumps.",
                        )
                        .size(12.0)
                        .color(ThemeColors::text_muted()),
                    );
                    ui.add_space(8.0);
                    let btn = egui::Button::new(
                        egui::RichText::new("Run Check-up")
                            .size(12.0)
                            .color(ThemeColors::text_primary()),
                    )
                    .fill(ThemeColors::accent())
                    .corner_radius(6.0)
                    .min_size(egui::vec2(110.0, 28.0));
                    if ui.add(btn).clicked() {
                        self.active_test = ActiveTest::AutoTest;
                    }
                });
            });
        });

        ui.add_space(28.0);

        // Quick start section
        ui.label(theme::subheading_style("QUICK START"));
        ui.add_space(12.0);

        egui::Grid::new("quick_start_grid")
            .num_columns(3)
            .spacing([16.0, 16.0])
            .show(ui, |ui| {
                self.test_card(
                    ui,
                    "Polling Rate",
                    "Measure your mouse's polling rate in Hz",
                    "⚡",
                    ActiveTest::PollingRate,
                );
                self.test_card(
                    ui,
                    "Stutter Test",
                    "Detect movement irregularities",
                    "📊",
                    ActiveTest::Stutter,
                );
                self.test_card(
                    ui,
                    "Click Response",
                    "Measure hold time and clicks per second",
                    "🖱",
                    ActiveTest::ClickResponse,
                );
                ui.end_row();

                self.test_card(
                    ui,
                    "Jitter Test",
                    "Measure sensor noise at rest",
                    "〰",
                    ActiveTest::Jitter,
                );
                self.test_card(
                    ui,
                    "DPI Accuracy",
                    "Verify your DPI settings",
                    "🎯",
                    ActiveTest::Dpi,
                );
                self.test_card(
                    ui,
                    "Double-Click",
                    "Detect switch issues",
                    "👆",
                    ActiveTest::DoubleClick,
                );
                ui.end_row();
            });

        ui.add_space(32.0);

        // System info
        ui.label(theme::subheading_style("SYSTEM INFO"));
        ui.add_space(12.0);

        let status = input_source_status(self.input_bridge.as_ref());

        theme::glass_frame(ui).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(theme::metric_label_style("PLATFORM"));
                    ui.label(
                        egui::RichText::new(std::env::consts::OS.to_uppercase())
                            .size(16.0)
                            .strong()
                            .color(ThemeColors::text_primary()),
                    );
                });
                ui.add_space(40.0);
                ui.vertical(|ui| {
                    ui.label(theme::metric_label_style("ARCHITECTURE"));
                    ui.label(
                        egui::RichText::new(std::env::consts::ARCH.to_uppercase())
                            .size(16.0)
                            .strong()
                            .color(ThemeColors::text_primary()),
                    );
                });
                ui.add_space(40.0);
                ui.vertical(|ui| {
                    ui.label(theme::metric_label_style("INPUT SOURCE"));
                    ui.label(
                        egui::RichText::new(status.headline)
                            .size(16.0)
                            .strong()
                            .color(if status.raw {
                                ThemeColors::success()
                            } else {
                                egui::Color32::YELLOW
                            }),
                    );
                });
            });
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(&status.detail)
                    .size(11.0)
                    .color(ThemeColors::text_muted()),
            );
            if let Some(warning) = &status.warning {
                ui.label(
                    egui::RichText::new(format!(
                        "{warning} If you are testing a different mouse, select it below."
                    ))
                    .size(11.0)
                    .color(egui::Color32::YELLOW),
                );
            }
            if let Some(bridge) = self.input_bridge.as_mut() {
                ui.add_space(8.0);
                device_picker(ui, bridge);
            }
        });
    }

    fn test_card(
        &mut self,
        ui: &mut egui::Ui,
        title: &str,
        description: &str,
        icon: &str,
        test: ActiveTest,
    ) {
        theme::card_frame(ui).show(ui, |ui| {
            ui.set_min_size(egui::vec2(200.0, 110.0));
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icon).size(20.0));
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(title)
                            .strong()
                            .size(15.0)
                            .color(ThemeColors::text_primary()),
                    );
                });
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(description)
                        .size(12.0)
                        .color(ThemeColors::text_muted()),
                );
                ui.add_space(12.0);

                let btn = egui::Button::new(
                    egui::RichText::new("Start Test")
                        .size(12.0)
                        .color(ThemeColors::accent()),
                )
                .fill(ThemeColors::accent_dim())
                .stroke(egui::Stroke::new(1.0, ThemeColors::accent()))
                .corner_radius(6.0)
                .min_size(egui::vec2(90.0, 28.0));

                if ui.add(btn).clicked() {
                    self.active_test = test;
                }
            });
        });
    }

    fn render_about_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("About Mouse TRAP")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::new()
                    .fill(ThemeColors::bg_panel())
                    .stroke(egui::Stroke::new(1.0, ThemeColors::border()))
                    .corner_radius(12)
                    .inner_margin(24)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 100),
                    }),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    // Draw mouse head icon in About dialog
                    let icon_size = 50.0;
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(icon_size, icon_size),
                        egui::Sense::hover(),
                    );
                    let painter = ui.painter();
                    let center = rect.center();

                    // Main head circle
                    let head_radius = 18.0;
                    painter.circle_filled(center, head_radius, ThemeColors::accent());

                    // Left ear
                    let ear_radius = 9.0;
                    let ear_offset_x = 15.0;
                    let ear_offset_y = -12.0;
                    painter.circle_filled(
                        egui::pos2(center.x - ear_offset_x, center.y + ear_offset_y),
                        ear_radius,
                        ThemeColors::accent(),
                    );

                    // Right ear
                    painter.circle_filled(
                        egui::pos2(center.x + ear_offset_x, center.y + ear_offset_y),
                        ear_radius,
                        ThemeColors::accent(),
                    );

                    ui.add_space(8.0);

                    ui.label(
                        egui::RichText::new("Mouse TRAP")
                            .size(24.0)
                            .strong()
                            .color(ThemeColors::accent()),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("Test Response And Positioning")
                            .size(12.0)
                            .color(ThemeColors::text_secondary()),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("Version 0.1.0")
                            .size(12.0)
                            .color(ThemeColors::text_muted()),
                    );
                    ui.add_space(16.0);

                    ui.label(
                        egui::RichText::new("Test Response And Positioning")
                            .color(ThemeColors::text_secondary()),
                    );

                    ui.add_space(16.0);

                    theme::glass_frame(ui).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("Features")
                                    .strong()
                                    .color(ThemeColors::text_primary()),
                            );
                            ui.add_space(8.0);
                            for feature in [
                                "Polling rate monitoring",
                                "Stutter detection with graphs",
                                "Click hold time and rate",
                                "DPI accuracy verification",
                                "Sensor jitter analysis",
                            ] {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("•").color(ThemeColors::accent()));
                                    ui.label(
                                        egui::RichText::new(feature)
                                            .color(ThemeColors::text_secondary()),
                                    );
                                });
                            }
                        });
                    });

                    ui.add_space(20.0);

                    let close_btn = egui::Button::new(
                        egui::RichText::new("Close").color(ThemeColors::text_primary()),
                    )
                    .fill(ThemeColors::accent())
                    .corner_radius(8)
                    .min_size(egui::vec2(100.0, 32.0));

                    if ui.add(close_btn).clicked() {
                        self.show_about = false;
                    }

                    // Hidden message
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new("MERRY CHRISTMAS // TO: XANDER FROM: DAD")
                            .size(8.0)
                            .color(egui::Color32::from_rgba_unmultiplied(80, 80, 80, 60)),
                    );
                });
            });
    }
}

/// What the Dashboard says about where test input comes from.
struct InputSourceStatus {
    headline: &'static str,
    /// Raw input from a device is in use.
    raw: bool,
    detail: String,
    /// The measured device is not the one moving the pointer.
    warning: Option<String>,
}

const FRAMEWORK_HEADLINE: &str = "FRAMEWORK (REDUCED ACCURACY)";
const FRAMEWORK_EXPLAINED: &str =
    "tests use framework input (frame-rate limited, after OS acceleration)";

fn input_source_status(bridge: Option<&InputBridge>) -> InputSourceStatus {
    let Some(bridge) = bridge else {
        let detail = if cfg!(target_os = "linux") {
            "No readable mouse device was found. Add your user to the 'input' group (sudo usermod -aG input $USER) and log in again for precise timing."
        } else if cfg!(target_os = "windows") {
            "Raw Input registration failed; timing tests will use frame-rate-limited input."
        } else {
            "Raw input is not implemented on this platform; timing tests use frame-rate-limited input."
        };
        return InputSourceStatus {
            headline: FRAMEWORK_HEADLINE,
            raw: false,
            detail: detail.to_string(),
            warning: None,
        };
    };
    if let Some(device) = bridge.active_device() {
        let detail = if bridge.choice() == DeviceChoice::Auto {
            format!(
                "Measuring {}: the mouse you last moved or clicked (kept while a test runs).",
                device.label()
            )
        } else {
            format!("Measuring {}, as selected below.", device.label())
        };
        return InputSourceStatus {
            headline: "RAW INPUT",
            raw: true,
            detail,
            warning: pointer_mismatch_warning(bridge),
        };
    }
    let detail = if bridge.choice() == DeviceChoice::Pointer {
        format!("Pointer input selected: {FRAMEWORK_EXPLAINED}.")
    } else if bridge.devices().is_empty() {
        format!(
            "No mouse can be read directly, so {FRAMEWORK_EXPLAINED}. A mouse plugged in later is picked up automatically."
        )
    } else if bridge.pointer_mismatch() {
        format!(
            "The pointer is moving but no readable mouse is: the device in use (touchpad, tablet, remote session) cannot be read directly, so {FRAMEWORK_EXPLAINED}."
        )
    } else {
        format!(
            "Raw input is ready on {} device(s). Move or click the mouse you want to test to select it; until then {FRAMEWORK_EXPLAINED}.",
            bridge.devices().len()
        )
    };
    InputSourceStatus {
        headline: FRAMEWORK_HEADLINE,
        raw: false,
        detail,
        warning: None,
    }
}

/// Warning shown while the pointer moves but the measured device does not.
fn pointer_mismatch_warning(bridge: &InputBridge) -> Option<String> {
    let device = bridge.active_device()?;
    bridge.pointer_mismatch().then(|| {
        format!(
            "The pointer is moving but the measured device, {}, is not reporting it.",
            device.label()
        )
    })
}

/// Combo box choosing which device the tests measure.
fn device_picker(ui: &mut egui::Ui, bridge: &mut InputBridge) {
    const AUTO: &str = "Auto (the mouse you move or click)";
    const POINTER: &str = "Pointer (framework input)";
    let mut choice = bridge.choice();
    let selected = match choice {
        DeviceChoice::Auto => AUTO.to_string(),
        DeviceChoice::Device(id) => bridge
            .devices()
            .iter()
            .find(|d| d.id == id)
            .map_or_else(|| AUTO.to_string(), |d| d.label()),
        DeviceChoice::Pointer => POINTER.to_string(),
    };
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Measure:")
                .size(12.0)
                .color(ThemeColors::text_secondary()),
        );
        egui::ComboBox::from_id_salt("input_device_choice")
            .selected_text(selected)
            .width(320.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut choice, DeviceChoice::Auto, AUTO);
                for device in bridge.devices() {
                    ui.selectable_value(
                        &mut choice,
                        DeviceChoice::Device(device.id),
                        device.label(),
                    );
                }
                ui.selectable_value(&mut choice, DeviceChoice::Pointer, POINTER);
            });
    });
    if choice != bridge.choice() {
        bridge.set_choice(choice);
    }
}

impl eframe::App for MouseTestKitApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        // Poll raw input events from the background thread (once per frame).
        // Use a local variable so the borrow checker doesn't conflict with &mut self in the closure.
        let tests_running = self.any_test_running();
        let pointer_moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
        let frame_events = match &mut self.input_bridge {
            Some(bridge) => {
                // Auto device selection must not switch devices mid-test
                bridge.set_selection_frozen(tests_running);
                let events = bridge.poll();
                bridge.cross_check_pointer(Instant::now(), pointer_moved, &events);
                events
            }
            None => Vec::new(),
        };
        // Raw input is used only while a device is actually being measured;
        // until then (and when pointer input is chosen) panels use framework
        // input and say so.
        let has_bridge = self
            .input_bridge
            .as_ref()
            .is_some_and(InputBridge::is_delivering);
        let mismatch_warning = self
            .input_bridge
            .as_ref()
            .and_then(pointer_mismatch_warning);

        // The auto diagnostics engine must see every frame's events even
        // while another panel is displayed.
        self.auto_test_panel
            .process_input(ctx, &frame_events, has_bridge);

        // Check if any panel settings changed and mark config dirty
        if self.stutter_panel.settings_changed()
            || self.dpi_panel.settings_changed()
            || self.double_click_panel.settings_changed()
        {
            self.config_dirty_since = Some(Instant::now());
        }

        // Save config once changes have settled
        self.save_config_if_needed(false);

        // Render sidebar
        self.render_sidebar(ui);

        let raw_events = &frame_events;

        // Render main content with styled central panel
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(ThemeColors::bg_dark())
                    .inner_margin(24),
            )
            .show(ui, |ui| {
                // Test pages say when the measured device is not the one
                // moving the pointer (the Dashboard has its own notice)
                if let Some(warning) = &mismatch_warning {
                    if self.active_test != ActiveTest::Dashboard {
                        ui.label(
                            egui::RichText::new(format!(
                                "{warning} Choose the mouse you are testing on the Dashboard."
                            ))
                            .size(11.0)
                            .color(egui::Color32::YELLOW),
                        );
                        ui.add_space(8.0);
                    }
                }
                egui::ScrollArea::vertical().show(ui, |ui| match self.active_test {
                    ActiveTest::Dashboard => self.render_dashboard(ui),
                    ActiveTest::AutoTest => self.auto_test_panel.ui(ui, ctx, has_bridge),
                    ActiveTest::PollingRate => {
                        self.polling_panel.ui(ui, ctx, raw_events, has_bridge)
                    }
                    ActiveTest::Stutter => self.stutter_panel.ui(ui, ctx, raw_events, has_bridge),
                    ActiveTest::ClickResponse => self
                        .click_panel
                        .ui_response(ui, ctx, raw_events, has_bridge),
                    ActiveTest::ClickSticky => {
                        self.click_panel.ui_sticky(ui, ctx, raw_events, has_bridge)
                    }
                    ActiveTest::LiftOff => {
                        self.click_panel.ui_liftoff(ui, ctx, raw_events, has_bridge)
                    }
                    ActiveTest::ScrollWheel => {
                        self.scroll_panel.ui(ui, ctx, raw_events, has_bridge)
                    }
                    ActiveTest::Dpi => self.dpi_panel.ui(ui, ctx, raw_events, has_bridge),
                    ActiveTest::AngleSnap => {
                        self.accel_panel.ui_angle(ui, ctx, raw_events, has_bridge)
                    }
                    ActiveTest::Acceleration => {
                        self.accel_panel.ui_accel(ui, ctx, raw_events, has_bridge)
                    }
                    ActiveTest::DoubleClick => {
                        self.double_click_panel.ui(ui, ctx, raw_events, has_bridge)
                    }
                    ActiveTest::Jitter => self.jitter_panel.ui(ui, ctx, raw_events, has_bridge),
                });
            });

        // About window
        if self.show_about {
            self.render_about_window(ctx);
        }

        // Only request continuous repaints when a test is actively running.
        // While idle, repaint occasionally so the raw-input channel is drained
        // (and devices are picked up) even when the pointer is outside the
        // window.
        if self.any_test_running() {
            ctx.request_repaint();
        } else if self.input_bridge.is_some() {
            ctx.request_repaint_after(IDLE_REPAINT_INTERVAL);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_config_if_needed(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_bridge::{BridgeMessage, InputDevice, RawButton, RawInputEvent, RawInputKind};

    fn device(id: u64, name: &str, location: &str) -> BridgeMessage {
        BridgeMessage::Added(InputDevice {
            id,
            name: name.to_string(),
            location: location.to_string(),
        })
    }

    fn press(id: u64) -> BridgeMessage {
        BridgeMessage::Event(
            id,
            RawInputEvent {
                kind: RawInputKind::ButtonPress(RawButton::Left),
                timestamp: Instant::now(),
            },
        )
    }

    fn render_dashboard(app: &mut MouseTestKitApp) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 900.0),
            )),
            ..Default::default()
        };
        for _ in 0..2 {
            let _ = ctx.run_ui(input.clone(), |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| app.render_dashboard(ui));
            });
        }
    }

    #[test]
    fn dashboard_claims_raw_input_only_while_a_device_delivers() {
        let (mut bridge, sender) = InputBridge::for_test();
        // A laptop: TrackPoint node plus an external mouse, neither used yet
        sender
            .send(device(1, "TPPS/2 TrackPoint", "event10"))
            .unwrap();
        sender.send(device(2, "Gaming Mouse", "event2")).unwrap();
        bridge.poll();
        let status = input_source_status(Some(&bridge));
        assert!(!status.raw);
        assert_eq!(status.headline, FRAMEWORK_HEADLINE);
        assert!(status.detail.contains("Move or click"), "{}", status.detail);

        // The user clicks with the external mouse
        sender.send(press(2)).unwrap();
        assert_eq!(bridge.poll().len(), 1);
        let status = input_source_status(Some(&bridge));
        assert!(status.raw);
        assert!(status.detail.contains("Gaming Mouse (event2)"));
        assert!(status.warning.is_none());

        let mut app = MouseTestKitApp::with_parts(Config::default(), Some(bridge));
        render_dashboard(&mut app);

        // Pointer input chosen in the picker
        let bridge = app.input_bridge.as_mut().unwrap();
        bridge.set_choice(DeviceChoice::Pointer);
        let status = input_source_status(Some(bridge));
        assert!(!status.raw);
        assert!(status.detail.starts_with("Pointer input selected"));
        render_dashboard(&mut app);
    }

    #[test]
    fn unplugging_the_measured_mouse_falls_back_to_framework_input() {
        let (mut bridge, sender) = InputBridge::for_test();
        sender.send(device(5, "USB Mouse", "event5")).unwrap();
        sender.send(press(5)).unwrap();
        bridge.poll();
        assert!(input_source_status(Some(&bridge)).raw);

        sender.send(BridgeMessage::Removed(5)).unwrap();
        bridge.poll();
        let status = input_source_status(Some(&bridge));
        assert!(!status.raw);
        assert!(
            status.detail.contains("plugged in later"),
            "{}",
            status.detail
        );
    }

    #[test]
    fn dashboard_renders_without_raw_input() {
        let mut app = MouseTestKitApp::with_parts(Config::default(), None);
        render_dashboard(&mut app);
        assert!(!input_source_status(None).raw);
    }
}
