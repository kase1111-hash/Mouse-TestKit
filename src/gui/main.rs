#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod panels;
mod theme;

use eframe::egui;
use app::MouseTestKitApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([800.0, 600.0])
            .with_title("Mouse TRAP"),
        ..Default::default()
    };

    eframe::run_native(
        "Mouse TRAP",
        options,
        Box::new(|cc| Ok(Box::new(MouseTestKitApp::new(cc)))),
    )
}
