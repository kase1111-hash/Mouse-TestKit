//! Main application module for Mouse TRAP GUI
//!
//! Contains the primary application struct and UI rendering logic.
//! Manages navigation between test panels, theming, and result export.

use eframe::egui;

use crate::config::Config;
use crate::input_bridge::InputBridge;
use crate::panels::{
    PollingPanel, StutterPanel, ClickPanel, JitterPanel,
    DpiPanel, AccelPanel, DoubleClickPanel, ScrollPanel
};
use crate::theme::{self, ThemeColors};
use crate::export::{TestResultsExport, ExportInfo};

/// Represents the currently active test or view in the application.
#[derive(PartialEq, Clone, Copy)]
pub enum ActiveTest {
    Dashboard,
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
    /// Current theme (true = dark mode)
    dark_mode: bool,
    /// Whether to show the About dialog
    show_about: bool,
    /// Status message from last export operation
    export_status: Option<String>,
    /// User configuration
    config: Config,
    /// Whether config needs to be saved to disk
    config_dirty: bool,
    /// Background raw input thread (None on unsupported platforms)
    input_bridge: Option<InputBridge>,
}

impl MouseTestKitApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Set up custom fonts and style
        theme::setup_custom_style(&cc.egui_ctx);

        // Load saved configuration
        let config = Config::load();

        // Create panels with config values
        let mut stutter_panel = StutterPanel::new();
        stutter_panel.set_threshold_multiplier(config.stutter_threshold_multiplier);

        let mut dpi_panel = DpiPanel::new();
        dpi_panel.set_target_dpi(config.dpi_target);
        dpi_panel.set_target_distance(config.dpi_distance_inches);

        let mut double_click_panel = DoubleClickPanel::new();
        double_click_panel.set_threshold_ms(config.double_click_threshold_ms);

        // Start background raw input thread
        let input_bridge = InputBridge::start();
        if input_bridge.is_none() {
            eprintln!("Note: Raw input not available. GUI tests will use framework input.");
        }

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
            dark_mode: config.dark_mode,
            show_about: false,
            export_status: None,
            config,
            config_dirty: false,
            input_bridge,
        }
    }

    /// Collect current settings and update config
    fn update_config(&mut self) {
        self.config.dark_mode = self.dark_mode;
        self.config.stutter_threshold_multiplier = self.stutter_panel.get_threshold_multiplier();
        self.config.dpi_target = self.dpi_panel.get_target_dpi();
        self.config.dpi_distance_inches = self.dpi_panel.get_target_distance();
        self.config.double_click_threshold_ms = self.double_click_panel.get_threshold_ms();
    }

    /// Save config to disk if dirty
    fn save_config_if_needed(&mut self) {
        if self.config_dirty {
            self.update_config();
            if let Err(e) = self.config.save() {
                eprintln!("Warning: Failed to save config: {}", e);
            }
            self.config_dirty = false;
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
                        Ok(_) => self.export_status = Some(format!("Exported to {}", path.display())),
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

    fn render_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .default_width(200.0)
            .frame(egui::Frame::none()
                .fill(ThemeColors::sidebar_bg())
                .stroke(egui::Stroke::new(1.0, ThemeColors::border())))
            .show(ctx, |ui| {
                ui.add_space(20.0);

                // Logo/Title with mouse head icon
                ui.vertical_centered(|ui| {
                    // Draw mouse head icon (1 big circle + 2 ear circles)
                    let icon_size = 40.0;
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(icon_size, icon_size), egui::Sense::hover());
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
                        ThemeColors::accent()
                    );

                    // Right ear
                    painter.circle_filled(
                        egui::pos2(center.x + ear_offset_x, center.y + ear_offset_y),
                        ear_radius,
                        ThemeColors::accent()
                    );
                });

                ui.add_space(24.0);

                // Separator with gradient look
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    let rect = ui.available_rect_before_wrap();
                    let painter = ui.painter();
                    painter.line_segment(
                        [egui::pos2(rect.left(), rect.top()), egui::pos2(rect.right() - 20.0, rect.top())],
                        egui::Stroke::new(1.0, ThemeColors::border())
                    );
                    ui.add_space(ui.available_width());
                });

                ui.add_space(16.0);

                // Navigation section
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new("NAVIGATION")
                        .size(10.0)
                        .strong()
                        .color(ThemeColors::text_muted()));
                });
                ui.add_space(8.0);

                self.nav_button(ui, "Dashboard", ActiveTest::Dashboard, true);

                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new("CORE TESTS")
                        .size(10.0)
                        .strong()
                        .color(ThemeColors::text_muted()));
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
                    ui.label(egui::RichText::new("ADVANCED TESTS")
                        .size(10.0)
                        .strong()
                        .color(ThemeColors::text_muted()));
                });
                ui.add_space(8.0);

                self.nav_button(ui, "DPI Accuracy", ActiveTest::Dpi, false);
                self.nav_button(ui, "Angle Snapping", ActiveTest::AngleSnap, false);
                self.nav_button(ui, "Acceleration", ActiveTest::Acceleration, false);
                self.nav_button(ui, "Double-Click", ActiveTest::DoubleClick, false);
                self.nav_button(ui, "Jitter Test", ActiveTest::Jitter, false);

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(16.0);

                    // Bottom buttons row 1: Theme and About
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);

                        let theme_btn = egui::Button::new(
                            egui::RichText::new(if self.dark_mode { "☀" } else { "🌙" })
                                .size(14.0)
                        )
                        .fill(ThemeColors::bg_glass())
                        .stroke(egui::Stroke::new(1.0, ThemeColors::border()))
                        .rounding(8.0)
                        .min_size(egui::vec2(36.0, 32.0));

                        if ui.add(theme_btn).clicked() {
                            self.dark_mode = !self.dark_mode;
                            self.config_dirty = true;
                        }

                        let about_btn = egui::Button::new(
                            egui::RichText::new("About")
                                .size(12.0)
                                .color(ThemeColors::text_secondary())
                        )
                        .fill(ThemeColors::bg_glass())
                        .stroke(egui::Stroke::new(1.0, ThemeColors::border()))
                        .rounding(8.0)
                        .min_size(egui::vec2(60.0, 32.0));

                        if ui.add(about_btn).clicked() {
                            self.show_about = true;
                        }
                    });

                    ui.add_space(8.0);

                    // Export buttons row
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);

                        let json_btn = egui::Button::new(
                            egui::RichText::new("JSON")
                                .size(11.0)
                                .color(ThemeColors::accent())
                        )
                        .fill(ThemeColors::accent_dim())
                        .stroke(egui::Stroke::new(1.0, ThemeColors::accent()))
                        .rounding(6.0)
                        .min_size(egui::vec2(50.0, 28.0));

                        if ui.add(json_btn).on_hover_text("Export all results to JSON").clicked() {
                            self.export_json();
                        }

                        let csv_btn = egui::Button::new(
                            egui::RichText::new("CSV")
                                .size(11.0)
                                .color(ThemeColors::accent())
                        )
                        .fill(ThemeColors::accent_dim())
                        .stroke(egui::Stroke::new(1.0, ThemeColors::accent()))
                        .rounding(6.0)
                        .min_size(egui::vec2(50.0, 28.0));

                        if ui.add(csv_btn).on_hover_text("Export all results to CSV").clicked() {
                            self.export_csv();
                        }
                    });

                    // Export status message
                    if let Some(ref status) = self.export_status {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add_space(16.0);
                            ui.label(egui::RichText::new(status)
                                .size(10.0)
                                .color(if status.starts_with("Error") {
                                    egui::Color32::RED
                                } else {
                                    ThemeColors::success()
                                }));
                        });
                    }

                    ui.add_space(8.0);

                    // Separator
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        let rect = ui.available_rect_before_wrap();
                        let painter = ui.painter();
                        painter.line_segment(
                            [egui::pos2(rect.left(), rect.top()), egui::pos2(rect.right() - 20.0, rect.top())],
                            egui::Stroke::new(1.0, ThemeColors::border())
                        );
                        ui.add_space(ui.available_width());
                    });

                    ui.add_space(4.0);

                    // Export section header
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.label(egui::RichText::new("EXPORT RESULTS")
                            .size(10.0)
                            .strong()
                            .color(ThemeColors::text_muted()));
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
                    ThemeColors::accent()
                )
            } else {
                (
                    egui::Color32::TRANSPARENT,
                    egui::Stroke::NONE,
                    ThemeColors::text_secondary()
                )
            };

            let button = egui::Button::new(
                egui::RichText::new(label)
                    .size(13.0)
                    .color(text_color)
            )
            .fill(bg_fill)
            .stroke(stroke)
            .rounding(8.0)
            .min_size(egui::vec2(172.0, 32.0));

            let response = ui.add(button);

            if response.hovered() && !selected {
                // Draw hover effect
                let rect = response.rect;
                ui.painter().rect_filled(
                    rect,
                    8.0,
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 5)
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
                        egui::vec2(3.0, rect.height() - 12.0)
                    ),
                    2.0,
                    ThemeColors::accent()
                );
            }
        });

        ui.add_space(2.0);
    }

    fn render_dashboard(&mut self, ui: &mut egui::Ui) {
        ui.label(theme::heading_style("Welcome to Mouse TRAP"));
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Test Response And Positioning — precision diagnostics for your mouse.")
            .color(ThemeColors::text_secondary()));

        ui.add_space(28.0);

        // Quick start section
        ui.label(theme::subheading_style("QUICK START"));
        ui.add_space(12.0);

        egui::Grid::new("quick_start_grid")
            .num_columns(3)
            .spacing([16.0, 16.0])
            .show(ui, |ui| {
                self.test_card(ui, "Polling Rate", "Measure your mouse's polling rate in Hz", "⚡", ActiveTest::PollingRate);
                self.test_card(ui, "Stutter Test", "Detect movement irregularities", "📊", ActiveTest::Stutter);
                self.test_card(ui, "Click Response", "Test reaction time and latency", "🖱", ActiveTest::ClickResponse);
                ui.end_row();

                self.test_card(ui, "Jitter Test", "Measure sensor noise at rest", "〰", ActiveTest::Jitter);
                self.test_card(ui, "DPI Accuracy", "Verify your DPI settings", "🎯", ActiveTest::Dpi);
                self.test_card(ui, "Double-Click", "Detect switch issues", "👆", ActiveTest::DoubleClick);
                ui.end_row();
            });

        ui.add_space(32.0);

        // System info
        ui.label(theme::subheading_style("SYSTEM INFO"));
        ui.add_space(12.0);

        theme::glass_frame(ui).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(theme::metric_label_style("PLATFORM"));
                    ui.label(egui::RichText::new(std::env::consts::OS.to_uppercase())
                        .size(16.0)
                        .strong()
                        .color(ThemeColors::text_primary()));
                });
                ui.add_space(40.0);
                ui.vertical(|ui| {
                    ui.label(theme::metric_label_style("ARCHITECTURE"));
                    ui.label(egui::RichText::new(std::env::consts::ARCH.to_uppercase())
                        .size(16.0)
                        .strong()
                        .color(ThemeColors::text_primary()));
                });
                ui.add_space(40.0);
                ui.vertical(|ui| {
                    ui.label(theme::metric_label_style("STATUS"));
                    ui.label(egui::RichText::new("READY")
                        .size(16.0)
                        .strong()
                        .color(ThemeColors::success()));
                });
            });
        });
    }

    fn test_card(&mut self, ui: &mut egui::Ui, title: &str, description: &str, icon: &str, test: ActiveTest) {
        theme::card_frame(ui).show(ui, |ui| {
            ui.set_min_size(egui::vec2(200.0, 110.0));
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icon).size(20.0));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(title)
                        .strong()
                        .size(15.0)
                        .color(ThemeColors::text_primary()));
                });
                ui.add_space(8.0);
                ui.label(egui::RichText::new(description)
                    .size(12.0)
                    .color(ThemeColors::text_muted()));
                ui.add_space(12.0);

                let btn = egui::Button::new(
                    egui::RichText::new("Start Test")
                        .size(12.0)
                        .color(ThemeColors::accent())
                )
                .fill(ThemeColors::accent_dim())
                .stroke(egui::Stroke::new(1.0, ThemeColors::accent()))
                .rounding(6.0)
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
            .frame(egui::Frame::none()
                .fill(ThemeColors::bg_panel())
                .stroke(egui::Stroke::new(1.0, ThemeColors::border()))
                .rounding(12.0)
                .inner_margin(24.0)
                .shadow(egui::epaint::Shadow {
                    offset: egui::vec2(0.0, 8.0),
                    blur: 24.0,
                    spread: 0.0,
                    color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 100),
                }))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    // Draw mouse head icon in About dialog
                    let icon_size = 50.0;
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(icon_size, icon_size), egui::Sense::hover());
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
                        ThemeColors::accent()
                    );

                    // Right ear
                    painter.circle_filled(
                        egui::pos2(center.x + ear_offset_x, center.y + ear_offset_y),
                        ear_radius,
                        ThemeColors::accent()
                    );

                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("Mouse TRAP")
                        .size(24.0)
                        .strong()
                        .color(ThemeColors::accent()));
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new("Test Response And Positioning")
                        .size(12.0)
                        .color(ThemeColors::text_secondary()));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Version 0.1.0")
                        .size(12.0)
                        .color(ThemeColors::text_muted()));
                    ui.add_space(16.0);

                    ui.label(egui::RichText::new("Test Response And Positioning")
                        .color(ThemeColors::text_secondary()));

                    ui.add_space(16.0);

                    theme::glass_frame(ui).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new("Features")
                                .strong()
                                .color(ThemeColors::text_primary()));
                            ui.add_space(8.0);
                            for feature in [
                                "Polling rate monitoring",
                                "Stutter detection with graphs",
                                "Click latency testing",
                                "DPI accuracy verification",
                                "Sensor jitter analysis",
                            ] {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("•")
                                        .color(ThemeColors::accent()));
                                    ui.label(egui::RichText::new(feature)
                                        .color(ThemeColors::text_secondary()));
                                });
                            }
                        });
                    });

                    ui.add_space(20.0);

                    let close_btn = egui::Button::new(
                        egui::RichText::new("Close")
                            .color(ThemeColors::text_primary())
                    )
                    .fill(ThemeColors::accent())
                    .rounding(8.0)
                    .min_size(egui::vec2(100.0, 32.0));

                    if ui.add(close_btn).clicked() {
                        self.show_about = false;
                    }

                    // Hidden message
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new("MERRY CHRISTMAS // TO: XANDER FROM: DAD")
                        .size(8.0)
                        .color(egui::Color32::from_rgba_unmultiplied(80, 80, 80, 60)));
                });
            });
    }
}

impl eframe::App for MouseTestKitApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll raw input events from the background thread (once per frame).
        // Use a local variable so the borrow checker doesn't conflict with &mut self in the closure.
        let frame_events = match &self.input_bridge {
            Some(bridge) => bridge.poll(),
            None => Vec::new(),
        };
        let has_bridge = self.input_bridge.is_some();

        // Check if any panel settings changed and mark config dirty
        if self.stutter_panel.settings_changed() ||
           self.dpi_panel.settings_changed() ||
           self.double_click_panel.settings_changed() {
            self.config_dirty = true;
        }

        // Save config if dirty (debounced - only saves once changes stop)
        self.save_config_if_needed();

        // Apply theme - always reapply custom style after setting visuals
        if self.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }
        theme::setup_custom_style(ctx);

        // Render sidebar
        self.render_sidebar(ctx);

        let raw_events = &frame_events;

        // Render main content with styled central panel
        egui::CentralPanel::default()
            .frame(egui::Frame::none()
                .fill(ThemeColors::bg_dark())
                .inner_margin(24.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    match self.active_test {
                        ActiveTest::Dashboard => self.render_dashboard(ui),
                        ActiveTest::PollingRate => self.polling_panel.ui(ui, ctx, raw_events, has_bridge),
                        ActiveTest::Stutter => self.stutter_panel.ui(ui, ctx, raw_events, has_bridge),
                        ActiveTest::ClickResponse => self.click_panel.ui_response(ui, ctx, raw_events, has_bridge),
                        ActiveTest::ClickSticky => self.click_panel.ui_sticky(ui, ctx, raw_events, has_bridge),
                        ActiveTest::LiftOff => self.click_panel.ui_liftoff(ui, ctx, raw_events, has_bridge),
                        ActiveTest::ScrollWheel => self.scroll_panel.ui(ui, ctx, raw_events, has_bridge),
                        ActiveTest::Dpi => self.dpi_panel.ui(ui, ctx, raw_events, has_bridge),
                        ActiveTest::AngleSnap => self.accel_panel.ui_angle(ui, ctx, raw_events, has_bridge),
                        ActiveTest::Acceleration => self.accel_panel.ui_accel(ui, ctx, raw_events, has_bridge),
                        ActiveTest::DoubleClick => self.double_click_panel.ui(ui, ctx),
                        ActiveTest::Jitter => self.jitter_panel.ui(ui, ctx, raw_events, has_bridge),
                    }
                });
            });

        // About window
        if self.show_about {
            self.render_about_window(ctx);
        }

        // Only request continuous repaints when a test is actively running
        let any_test_running = self.polling_panel.is_running()
            || self.stutter_panel.is_running()
            || self.click_panel.is_running()
            || self.jitter_panel.is_running()
            || self.dpi_panel.is_running()
            || self.accel_panel.is_running()
            || self.scroll_panel.is_running();
        if any_test_running {
            ctx.request_repaint();
        }
    }
}
