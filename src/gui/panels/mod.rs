//! Test panel modules for Mouse TRAP GUI
//!
//! Each panel provides a self-contained UI and logic for a specific mouse test.
//! Panels handle user interaction, data collection, analysis, and result export.
//!
//! # Panel Types
//!
//! - [`PollingPanel`] - Real-time polling rate measurement
//! - [`StutterPanel`] - Movement stutter and timing irregularity detection
//! - [`ClickPanel`] - Click response, stickiness, and lift-off tests
//! - [`JitterPanel`] - Sensor jitter analysis when mouse is stationary
//! - [`DpiPanel`] - DPI accuracy verification
//! - [`AccelPanel`] - Acceleration and angle snapping detection
//! - [`DoubleClickPanel`] - Switch health and double-click detection
//! - [`ScrollPanel`] - Scroll wheel consistency testing

mod polling;
mod stutter;
mod click;
mod jitter;
mod dpi;
mod accel;
mod double_click;
mod scroll;

pub use polling::PollingPanel;
pub use stutter::StutterPanel;
pub use click::ClickPanel;
pub use jitter::JitterPanel;
pub use dpi::DpiPanel;
pub use accel::AccelPanel;
pub use double_click::DoubleClickPanel;
pub use scroll::ScrollPanel;
