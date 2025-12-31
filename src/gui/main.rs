//! Mouse TRAP GUI Application
//!
//! A graphical mouse testing utility built with egui/eframe. Provides a modern,
//! responsive interface for comprehensive mouse diagnostics.
//!
//! # Features
//!
//! - Real-time polling rate monitoring with live graphs
//! - Stutter detection and timing analysis
//! - Click response testing (latency, stickiness, double-click)
//! - DPI accuracy verification
//! - Sensor analysis (jitter, acceleration, angle snapping)
//! - Export results to JSON or CSV
//! - Persistent configuration across sessions
//!
//! # Architecture
//!
//! The GUI is organized into panels, each handling a specific test type.
//! The main app manages navigation, theming, and configuration persistence.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod export;
mod panels;
mod theme;

use eframe::egui;
use app::MouseTestKitApp;

/// Generate mouse head icon as RGBA pixels (32x32)
fn create_mouse_icon() -> egui::IconData {
    let size = 32;
    let mut pixels = vec![0u8; size * size * 4];

    let center_x = size as f32 / 2.0;
    let center_y = size as f32 / 2.0 + 2.0; // Shift down slightly for ears
    let head_radius = 10.0f32;
    let ear_radius = 5.0f32;
    let ear_offset_x = 9.0f32;
    let ear_offset_y = 8.0f32;

    // Icon color (accent blue)
    let color = [64u8, 156, 255, 255];

    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            // Check if pixel is inside head circle
            let dx_head = px - center_x;
            let dy_head = py - center_y;
            let in_head = (dx_head * dx_head + dy_head * dy_head) <= head_radius * head_radius;

            // Check left ear
            let ear_left_x = center_x - ear_offset_x;
            let ear_left_y = center_y - ear_offset_y;
            let dx_left = px - ear_left_x;
            let dy_left = py - ear_left_y;
            let in_left_ear = (dx_left * dx_left + dy_left * dy_left) <= ear_radius * ear_radius;

            // Check right ear
            let ear_right_x = center_x + ear_offset_x;
            let ear_right_y = center_y - ear_offset_y;
            let dx_right = px - ear_right_x;
            let dy_right = py - ear_right_y;
            let in_right_ear = (dx_right * dx_right + dy_right * dy_right) <= ear_radius * ear_radius;

            if in_head || in_left_ear || in_right_ear {
                let idx = (y * size + x) * 4;
                pixels[idx] = color[0];
                pixels[idx + 1] = color[1];
                pixels[idx + 2] = color[2];
                pixels[idx + 3] = color[3];
            }
        }
    }

    egui::IconData {
        rgba: pixels,
        width: size as u32,
        height: size as u32,
    }
}

fn main() -> eframe::Result<()> {
    let icon = create_mouse_icon();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 640.0])
            .with_min_inner_size([640.0, 480.0])
            .with_title("Mouse TRAP")
            .with_icon(std::sync::Arc::new(icon)),
        ..Default::default()
    };

    eframe::run_native(
        "Mouse TRAP",
        options,
        Box::new(|cc| Ok(Box::new(MouseTestKitApp::new(cc)))),
    )
}
