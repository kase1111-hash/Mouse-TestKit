//! Mouse TRAP shared library
//!
//! Contains types and analysis functions shared between the CLI and GUI binaries.

pub mod analysis;
#[cfg(target_os = "linux")]
pub mod evdev_input;
pub mod types;
