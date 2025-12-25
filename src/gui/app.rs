use eframe::egui;

use crate::panels::{
    PollingPanel, StutterPanel, ClickPanel, JitterPanel,
    DpiPanel, AccelPanel, DoubleClickPanel
};
use crate::theme::{self, ThemeColors};

#[derive(PartialEq, Clone, Copy)]
pub enum ActiveTest {
    Dashboard,
    PollingRate,
    Stutter,
    ClickResponse,
    ClickSticky,
    LiftOff,
    Dpi,
    AngleSnap,
    Acceleration,
    DoubleClick,
    Jitter,
}

pub struct MouseTestKitApp {
    active_test: ActiveTest,
    polling_panel: PollingPanel,
    stutter_panel: StutterPanel,
    click_panel: ClickPanel,
    jitter_panel: JitterPanel,
    dpi_panel: DpiPanel,
    accel_panel: AccelPanel,
    double_click_panel: DoubleClickPanel,
    dark_mode: bool,
    show_about: bool,
}

impl MouseTestKitApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Set up custom fonts and style
        theme::setup_custom_style(&cc.egui_ctx);

        Self {
            active_test: ActiveTest::Dashboard,
            polling_panel: PollingPanel::new(),
            stutter_panel: StutterPanel::new(),
            click_panel: ClickPanel::new(),
            jitter_panel: JitterPanel::new(),
            dpi_panel: DpiPanel::new(),
            accel_panel: AccelPanel::new(),
            double_click_panel: DoubleClickPanel::new(),
            dark_mode: true,
            show_about: false,
        }
    }

    fn render_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .default_width(240.0)
            .frame(egui::Frame::none()
                .fill(ThemeColors::sidebar_bg())
                .stroke(egui::Stroke::new(1.0, ThemeColors::border())))
            .show(ctx, |ui| {
                ui.add_space(20.0);

                // Logo/Title with glassy effect
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Mouse-TestKit")
                        .size(22.0)
                        .strong()
                        .color(ThemeColors::text_primary()));
                    ui.label(egui::RichText::new("v0.1.0")
                        .size(12.0)
                        .color(ThemeColors::text_muted()));
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

                    // Bottom buttons
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

                    ui.add_space(12.0);

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

                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.label(egui::RichText::new("Run with sudo for\nfull device access")
                            .size(11.0)
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
            .min_size(egui::vec2(208.0, 34.0));

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
        ui.label(theme::heading_style("Welcome to Mouse-TestKit"));
        ui.add_space(8.0);
        ui.label(egui::RichText::new("A comprehensive mouse testing utility for analyzing performance, reliability, and accuracy.")
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
        egui::Window::new("About Mouse-TestKit")
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
                    ui.label(egui::RichText::new("Mouse-TestKit")
                        .size(24.0)
                        .strong()
                        .color(ThemeColors::accent()));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Version 0.1.0")
                        .size(12.0)
                        .color(ThemeColors::text_muted()));
                    ui.add_space(16.0);

                    ui.label(egui::RichText::new("A comprehensive mouse testing utility")
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
        // Apply theme - always reapply custom style after setting visuals
        if self.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }
        theme::setup_custom_style(ctx);

        // Render sidebar
        self.render_sidebar(ctx);

        // Render main content with styled central panel
        egui::CentralPanel::default()
            .frame(egui::Frame::none()
                .fill(ThemeColors::bg_dark())
                .inner_margin(24.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    match self.active_test {
                        ActiveTest::Dashboard => self.render_dashboard(ui),
                        ActiveTest::PollingRate => self.polling_panel.ui(ui, ctx),
                        ActiveTest::Stutter => self.stutter_panel.ui(ui, ctx),
                        ActiveTest::ClickResponse => self.click_panel.ui_response(ui, ctx),
                        ActiveTest::ClickSticky => self.click_panel.ui_sticky(ui, ctx),
                        ActiveTest::LiftOff => self.click_panel.ui_liftoff(ui, ctx),
                        ActiveTest::Dpi => self.dpi_panel.ui(ui, ctx),
                        ActiveTest::AngleSnap => self.accel_panel.ui_angle(ui, ctx),
                        ActiveTest::Acceleration => self.accel_panel.ui_accel(ui, ctx),
                        ActiveTest::DoubleClick => self.double_click_panel.ui(ui, ctx),
                        ActiveTest::Jitter => self.jitter_panel.ui(ui, ctx),
                    }
                });
            });

        // About window
        if self.show_about {
            self.render_about_window(ctx);
        }

        // Request repaint for real-time updates
        ctx.request_repaint();
    }
}
