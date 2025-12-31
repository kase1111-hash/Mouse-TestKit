/// Configuration persistence for Mouse TRAP
///
/// Saves and loads user preferences to a config file

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// User configuration that persists between sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Application theme
    pub dark_mode: bool,

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
            dark_mode: true,
            stutter_threshold_multiplier: 2.0,
            dpi_target: 800,
            dpi_distance_inches: 2.0,
            double_click_threshold_ms: 50.0,
        }
    }
}

impl Config {
    /// Load config from disk, or return default if not found
    pub fn load() -> Self {
        let path = Self::config_path();

        match fs::read_to_string(&path) {
            Ok(contents) => {
                match serde_json::from_str(&contents) {
                    Ok(config) => config,
                    Err(e) => {
                        eprintln!("Warning: Failed to parse config file: {}", e);
                        eprintln!("Using default settings.");
                        Self::default()
                    }
                }
            }
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

        fs::write(&path, json)
            .map_err(|e| format!("Failed to write config file: {}", e))?;

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
        assert!(config.dark_mode);
        assert_eq!(config.stutter_threshold_multiplier, 2.0);
        assert_eq!(config.dpi_target, 800);
        assert_eq!(config.dpi_distance_inches, 2.0);
        assert_eq!(config.double_click_threshold_ms, 50.0);
    }

    #[test]
    fn test_config_serialization() {
        let config = Config {
            dark_mode: false,
            stutter_threshold_multiplier: 3.0,
            dpi_target: 1600,
            dpi_distance_inches: 4.0,
            double_click_threshold_ms: 75.0,
        };

        let json = serde_json::to_string(&config).unwrap();
        let loaded: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.dark_mode, false);
        assert_eq!(loaded.stutter_threshold_multiplier, 3.0);
        assert_eq!(loaded.dpi_target, 1600);
        assert_eq!(loaded.dpi_distance_inches, 4.0);
        assert_eq!(loaded.double_click_threshold_ms, 75.0);
    }
}
