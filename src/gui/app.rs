use eframe::egui;

use crate::panels::{
    PollingPanel, StutterPanel, ClickPanel, JitterPanel,
    DpiPanel, AccelPanel, DoubleClickPanel, DurabilityPanel
};
use crate::theme;

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
    Durability,
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
    durability_panel: DurabilityPanel,
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
            durability_panel: DurabilityPanel::new(),
            dark_mode: true,
            show_about: false,
        }
    }

    fn render_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.add_space(10.0);

                // Logo/Title
                ui.vertical_centered(|ui| {
                    ui.heading("Mouse-TestKit");
                    ui.label("v0.1.0");
                });

                ui.add_space(20.0);
                ui.separator();
                ui.add_space(10.0);

                // Navigation
                ui.label(egui::RichText::new("TESTS").small().strong());
                ui.add_space(5.0);

                self.nav_button(ui, "Dashboard", ActiveTest::Dashboard);

                ui.add_space(10.0);
                ui.label(egui::RichText::new("Core Tests").small().weak());

                self.nav_button(ui, "Polling Rate", ActiveTest::PollingRate);
                self.nav_button(ui, "Stutter Detection", ActiveTest::Stutter);
                self.nav_button(ui, "Click Response", ActiveTest::ClickResponse);
                self.nav_button(ui, "Click Stickiness", ActiveTest::ClickSticky);
                self.nav_button(ui, "Lift-Off Jump", ActiveTest::LiftOff);

                ui.add_space(10.0);
                ui.label(egui::RichText::new("Advanced Tests").small().weak());

                self.nav_button(ui, "DPI Accuracy", ActiveTest::Dpi);
                self.nav_button(ui, "Angle Snapping", ActiveTest::AngleSnap);
                self.nav_button(ui, "Acceleration", ActiveTest::Acceleration);
                self.nav_button(ui, "Double-Click", ActiveTest::DoubleClick);
                self.nav_button(ui, "Jitter Test", ActiveTest::Jitter);
                self.nav_button(ui, "Durability", ActiveTest::Durability);

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        if ui.button(if self.dark_mode { "Light" } else { "Dark" }).clicked() {
                            self.dark_mode = !self.dark_mode;
                        }
                        if ui.button("About").clicked() {
                            self.show_about = true;
                        }
                    });

                    ui.add_space(5.0);
                    ui.separator();
                    ui.add_space(5.0);
                    ui.label(egui::RichText::new("Run with sudo for\ndevice access").small().weak());
                });
            });
    }

    fn nav_button(&mut self, ui: &mut egui::Ui, label: &str, test: ActiveTest) {
        let selected = self.active_test == test;
        let button = egui::Button::new(label)
            .fill(if selected {
                ui.visuals().selection.bg_fill
            } else {
                egui::Color32::TRANSPARENT
            })
            .min_size(egui::vec2(200.0, 30.0));

        if ui.add(button).clicked() {
            self.active_test = test;
        }
    }

    fn render_dashboard(&mut self, ui: &mut egui::Ui) {
        ui.heading("Welcome to Mouse-TestKit");
        ui.add_space(10.0);
        ui.label("A comprehensive mouse testing utility for analyzing performance, reliability, and accuracy.");

        ui.add_space(20.0);
        ui.separator();
        ui.add_space(20.0);

        // Quick start grid
        ui.heading("Quick Start");
        ui.add_space(10.0);

        egui::Grid::new("quick_start_grid")
            .num_columns(3)
            .spacing([20.0, 20.0])
            .show(ui, |ui| {
                self.test_card(ui, "Polling Rate", "Measure your mouse's polling rate in Hz", ActiveTest::PollingRate);
                self.test_card(ui, "Stutter Test", "Detect movement irregularities", ActiveTest::Stutter);
                self.test_card(ui, "Click Response", "Test reaction time and latency", ActiveTest::ClickResponse);
                ui.end_row();

                self.test_card(ui, "Jitter Test", "Measure sensor noise at rest", ActiveTest::Jitter);
                self.test_card(ui, "DPI Accuracy", "Verify your DPI settings", ActiveTest::Dpi);
                self.test_card(ui, "Double-Click", "Detect switch issues", ActiveTest::DoubleClick);
                ui.end_row();
            });

        ui.add_space(30.0);

        // System info
        ui.heading("System Info");
        ui.add_space(10.0);

        egui::Frame::dark_canvas(ui.style())
            .inner_margin(15.0)
            .rounding(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Platform:");
                    ui.label(egui::RichText::new(std::env::consts::OS).strong());
                    ui.add_space(30.0);
                    ui.label("Architecture:");
                    ui.label(egui::RichText::new(std::env::consts::ARCH).strong());
                });
            });
    }

    fn test_card(&mut self, ui: &mut egui::Ui, title: &str, description: &str, test: ActiveTest) {
        egui::Frame::none()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(15.0)
            .rounding(8.0)
            .show(ui, |ui| {
                ui.set_min_size(egui::vec2(220.0, 100.0));
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(title).strong().size(16.0));
                    ui.add_space(5.0);
                    ui.label(egui::RichText::new(description).weak().size(12.0));
                    ui.add_space(10.0);
                    if ui.button("Start Test").clicked() {
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
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Mouse-TestKit");
                    ui.label("Version 0.1.0");
                    ui.add_space(10.0);
                    ui.label("A comprehensive mouse testing utility");
                    ui.add_space(10.0);
                    ui.label("Features:");
                    ui.label("- Polling rate monitoring");
                    ui.label("- Stutter detection with graphs");
                    ui.label("- Click latency testing");
                    ui.label("- DPI accuracy verification");
                    ui.label("- Sensor jitter analysis");
                    ui.label("- And more!");
                    ui.add_space(20.0);
                    if ui.button("Close").clicked() {
                        self.show_about = false;
                    }
                });
            });
    }
}

impl eframe::App for MouseTestKitApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply theme
        if self.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // Render sidebar
        self.render_sidebar(ctx);

        // Render main content
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(10.0);

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
                    ActiveTest::Durability => self.durability_panel.ui(ui, ctx),
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
