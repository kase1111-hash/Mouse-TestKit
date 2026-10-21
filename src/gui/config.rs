//! Configuration persistence for Mouse TRAP
//!
//! Saves and loads user preferences to a config file

use mouse_testkit::analysis::diagnostics::Config as DiagConfig;
use serde::{Deserialize, Serialize};
use std::fs;
use std::ops::RangeInclusive;
use std::path::PathBuf;

// Ranges the settings widgets allow. Loaded values are clamped to these so a
// hand-edited or stale config file cannot set a value the UI would never
// offer, such as a 300 ms double-click threshold that would flag every
// normal double click as switch bounce.

/// Stutter panel sensitivity slider.
pub const STUTTER_MULTIPLIER_RANGE: RangeInclusive<f64> = 1.5..=4.0;
/// DPI panel target DPI field.
pub const DPI_TARGET_RANGE: RangeInclusive<u32> = 100..=16000;
/// DPI panel movement distance field (inches).
pub const DPI_DISTANCE_RANGE: RangeInclusive<f32> = 0.5..=12.0;
/// Double-click panel threshold field (ms).
pub const DOUBLE_CLICK_THRESHOLD_RANGE: RangeInclusive<f64> = 10.0..=100.0;

/// User configuration that persists between sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Stutter detection sensitivity (lower = more sensitive)
    pub stutter_threshold_multiplier: f64,

    /// DPI test target value
    pub dpi_target: u32,

    /// DPI test movement distance in inches
    pub dpi_distance_inches: f32,

    /// Double-click detection threshold in milliseconds
    pub double_click_threshold_ms: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stutter_threshold_multiplier: 2.0,
            dpi_target: 800,
            dpi_distance_inches: 2.0,
            // Same press-to-press bounce threshold as Auto Diagnostics
            double_click_threshold_ms: DiagConfig::default().bounce_threshold_ms,
        }
    }
}

impl Config {
    /// Parse a config file, clamping every value to the range its widget
    /// allows.
    pub fn parse(contents: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str::<Self>(contents).map(Self::sanitized)
    }

    /// Clamp every value to the range its widget allows. A value that is not
    /// a finite number falls back to the default.
    pub fn sanitized(self) -> Self {
        let defaults = Self::default();
        let clamp = |v: f64, r: &RangeInclusive<f64>, d: f64| {
            if v.is_finite() {
                v.clamp(*r.start(), *r.end())
            } else {
                d
            }
        };
        Self {
            stutter_threshold_multiplier: clamp(
                self.stutter_threshold_multiplier,
                &STUTTER_MULTIPLIER_RANGE,
                defaults.stutter_threshold_multiplier,
            ),
            dpi_target: self
                .dpi_target
                .clamp(*DPI_TARGET_RANGE.start(), *DPI_TARGET_RANGE.end()),
            dpi_distance_inches: if self.dpi_distance_inches.is_finite() {
                self.dpi_distance_inches
                    .clamp(*DPI_DISTANCE_RANGE.start(), *DPI_DISTANCE_RANGE.end())
            } else {
                defaults.dpi_distance_inches
            },
            double_click_threshold_ms: clamp(
                self.double_click_threshold_ms,
                &DOUBLE_CLICK_THRESHOLD_RANGE,
                defaults.double_click_threshold_ms,
            ),
        }
    }

    /// Load config from disk, or return default if not found
    pub fn load() -> Self {
        let path = Self::config_path();

        match fs::read_to_string(&path) {
            Ok(contents) => match Self::parse(&contents) {
                Ok(config) => config,
                Err(e) => {
                    eprintln!("Warning: Failed to parse config file: {}", e);
                    eprintln!("Using default settings.");
                    Self::default()
                }
            },
            Err(_) => {
                // Config file doesn't exist yet, use defaults
                Self::default()
            }
        }
    }

    /// Save config to disk
    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        fs::write(&path, json).map_err(|e| format!("Failed to write config file: {}", e))?;

        Ok(())
    }

    /// Get the config file path based on platform
    fn config_path() -> PathBuf {
        let config_dir = Self::config_dir();
        config_dir.join("config.json")
    }

    /// Get the config directory based on platform
    fn config_dir() -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            // Use %APPDATA%/mouse-trap on Windows
            if let Ok(appdata) = std::env::var("APPDATA") {
                return PathBuf::from(appdata).join("mouse-trap");
            }
        }

        #[cfg(target_os = "macos")]
        {
            // Use ~/Library/Application Support/mouse-trap on macOS
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home)
                    .join("Library")
                    .join("Application Support")
                    .join("mouse-trap");
            }
        }

        #[cfg(target_os = "linux")]
        {
            // Use XDG config dir or ~/.config on Linux
            if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
                return PathBuf::from(xdg).join("mouse-trap");
            }
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home).join(".config").join("mouse-trap");
            }
        }

        // Fallback: current directory
        PathBuf::from(".").join("mouse-trap-config")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.stutter_threshold_multiplier, 2.0);
        assert_eq!(config.dpi_target, 800);
        assert_eq!(config.dpi_distance_inches, 2.0);
        assert_eq!(config.double_click_threshold_ms, 50.0);
    }

    #[test]
    fn test_config_serialization() {
        let config = Config {
            stutter_threshold_multiplier: 3.0,
            dpi_target: 1600,
            dpi_distance_inches: 4.0,
            double_click_threshold_ms: 75.0,
        };

        let json = serde_json::to_string(&config).unwrap();
        let loaded: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.stutter_threshold_multiplier, 3.0);
        assert_eq!(loaded.dpi_target, 1600);
        assert_eq!(loaded.dpi_distance_inches, 4.0);
        assert_eq!(loaded.double_click_threshold_ms, 75.0);
    }

    #[test]
    fn test_loaded_values_are_clamped_to_widget_ranges() {
        // A 300 ms threshold would call every normal double click bounce
        let loaded = Config::parse(
            r#"{"stutter_threshold_multiplier": 0.5, "dpi_target": 0,
                "dpi_distance_inches": 40.0, "double_click_threshold_ms": 300.0}"#,
        )
        .unwrap();
        assert_eq!(loaded.stutter_threshold_multiplier, 1.5);
        assert_eq!(loaded.dpi_target, 100);
        assert_eq!(loaded.dpi_distance_inches, 12.0);
        assert_eq!(loaded.double_click_threshold_ms, 100.0);

        let loaded = Config::parse(
            r#"{"stutter_threshold_multiplier": 9.0, "dpi_target": 99999,
                "dpi_distance_inches": 0.0, "double_click_threshold_ms": -5.0}"#,
        )
        .unwrap();
        assert_eq!(loaded.stutter_threshold_multiplier, 4.0);
        assert_eq!(loaded.dpi_target, 16000);
        assert_eq!(loaded.dpi_distance_inches, 0.5);
        assert_eq!(loaded.double_click_threshold_ms, 10.0);
    }

    #[test]
    fn test_in_range_values_load_unchanged() {
        let json = serde_json::to_string(&Config::default()).unwrap();
        let loaded = Config::parse(&json).unwrap();
        let d = Config::default();
        assert_eq!(
            loaded.stutter_threshold_multiplier,
            d.stutter_threshold_multiplier
        );
        assert_eq!(loaded.dpi_target, d.dpi_target);
        assert_eq!(loaded.dpi_distance_inches, d.dpi_distance_inches);
        assert_eq!(
            loaded.double_click_threshold_ms,
            d.double_click_threshold_ms
        );
    }

    #[test]
    fn test_non_finite_values_fall_back_to_defaults() {
        let c = Config {
            stutter_threshold_multiplier: f64::NAN,
            dpi_target: 800,
            dpi_distance_inches: f32::INFINITY,
            double_click_threshold_ms: f64::NAN,
        }
        .sanitized();
        assert_eq!(c.stutter_threshold_multiplier, 2.0);
        assert_eq!(c.dpi_distance_inches, 2.0);
        assert_eq!(c.double_click_threshold_ms, 50.0);
    }
}
