#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod panels;
mod theme;

use eframe::egui;
use app::MouseTestKitApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 640.0])
            .with_min_inner_size([640.0, 480.0])
            .with_title("Mouse TRAP"),
        ..Default::default()
    };

    eframe::run_native(
        "Mouse TRAP",
        options,
        Box::new(|cc| Ok(Box::new(MouseTestKitApp::new(cc)))),
    )
}
