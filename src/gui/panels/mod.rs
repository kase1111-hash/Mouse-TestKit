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

mod accel;
mod click;
mod double_click;
mod dpi;
mod jitter;
mod polling;
mod scroll;
mod stutter;

pub use accel::AccelPanel;
pub use click::ClickPanel;
pub use double_click::DoubleClickPanel;
pub use dpi::DpiPanel;
pub use jitter::JitterPanel;
pub use polling::PollingPanel;
pub use scroll::ScrollPanel;
pub use stutter::StutterPanel;
