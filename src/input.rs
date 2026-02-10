//! Input handling module for Linux
//!
//! Provides mouse device detection and raw event reading via the evdev subsystem.
//! This module handles:
//! - Enumerating available mouse devices in /dev/input
//! - Interactive device selection
//! - Parsing raw input events into structured MouseEvent types
//!
//! # Permissions
//!
//! Requires read access to /dev/input/event* devices. Users may need to be
//! added to the 'input' group: `sudo usermod -aG input $USER`

#[cfg(target_os = "linux")]
use evdev::{Device, InputEventKind, RelativeAxisType, Key};
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::PathBuf;

/// Represents a detected mouse device on Linux.
#[cfg(target_os = "linux")]
pub struct MouseDevice {
    /// The evdev Device handle
    pub device: Device,
    /// Human-readable device name
    pub name: String,
    /// Path to the device file (e.g., /dev/input/event5)
    pub path: PathBuf,
}

/// Find all mouse devices in the system
#[cfg(target_os = "linux")]
pub fn find_mouse_devices() -> Vec<MouseDevice> {
    let mut mice = Vec::new();
    let mut permission_denied_count = 0;

    match fs::read_dir("/dev/input") {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();

                // Only check event devices
                if !path.to_string_lossy().contains("event") {
                    continue;
                }

                match Device::open(&path) {
                    Ok(device) => {
                        // Check if device has mouse-like capabilities
                        let has_rel_x = device.supported_relative_axes()
                            .map(|axes| axes.contains(RelativeAxisType::REL_X))
                            .unwrap_or(false);

                        let has_left_btn = device.supported_keys()
                            .map(|keys| keys.contains(Key::BTN_LEFT))
                            .unwrap_or(false);

                        if has_rel_x && has_left_btn {
                            let name = device.name().unwrap_or("Unknown Mouse").to_string();
                            mice.push(MouseDevice {
                                device,
                                name,
                                path,
                            });
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::PermissionDenied {
                            permission_denied_count += 1;
                        }
                        // Other errors (device busy, etc.) are silently skipped
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error: Cannot read /dev/input directory: {}", e);
            eprintln!("This may indicate a permissions issue or missing input subsystem.");
        }
    }

    // Warn user if we encountered permission issues
    if permission_denied_count > 0 && mice.is_empty() {
        eprintln!();
        eprintln!("Warning: {} device(s) could not be accessed due to permissions.", permission_denied_count);
        eprintln!("To fix this, either:");
        eprintln!("  1. Run with elevated privileges (not recommended for regular use)");
        eprintln!("  2. Add your user to the 'input' group: sudo usermod -aG input $USER");
        eprintln!("     Then log out and back in for changes to take effect.");
        eprintln!();
    }

    mice
}

/// Select a mouse device interactively
#[cfg(target_os = "linux")]
pub fn select_mouse() -> Option<Device> {
    let mice = find_mouse_devices();

    if mice.is_empty() {
        println!();
        println!("╔═══════════════════════════════════════════════════════════════╗");
        println!("║                    No Mouse Devices Found                     ║");
        println!("╠═══════════════════════════════════════════════════════════════╣");
        println!("║ This usually means one of the following:                      ║");
        println!("║                                                               ║");
        println!("║ 1. Permission denied - Your user cannot access input devices ║");
        println!("║    Fix: sudo usermod -aG input $USER                          ║");
        println!("║    Then log out and back in.                                  ║");
        println!("║                                                               ║");
        println!("║ 2. No mouse connected - Check your USB connection             ║");
        println!("║                                                               ║");
        println!("║ 3. Unusual mouse driver - Some gaming mice need extra drivers ║");
        println!("╚═══════════════════════════════════════════════════════════════╝");
        println!();
        return None;
    }

    if mice.len() == 1 {
        println!("Found mouse: {}", mice[0].name);
        return Some(mice.into_iter().next()?.device);
    }

    println!("Found {} mouse devices:\n", mice.len());
    for (i, mouse) in mice.iter().enumerate() {
        println!("  {}. {} ({})", i + 1, mouse.name, mouse.path.display());
    }

    println!("\nSelect device (1-{}), or 0 to cancel: ", mice.len());

    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Error reading input: {}", e);
            return None;
        }
    }

    let choice: usize = match input.trim().parse() {
        Ok(n) => n,
        Err(_) => {
            println!("Invalid selection. Please enter a number.");
            return None;
        }
    };

    if choice == 0 {
        println!("Selection cancelled.");
        return None;
    }

    if choice > mice.len() {
        println!("Invalid selection: {} is not a valid option (1-{}).", choice, mice.len());
        return None;
    }

    Some(mice.into_iter().nth(choice - 1)?.device)
}

// Re-export canonical types from the shared library crate.
pub use mouse_testkit::types::{MouseEvent, MouseButton};

/// Parse an evdev event into a MouseEvent
#[cfg(target_os = "linux")]
pub fn parse_event(event: &evdev::InputEvent) -> Option<MouseEvent> {
    match event.kind() {
        InputEventKind::RelAxis(axis) => {
            let value = event.value();
            match axis {
                RelativeAxisType::REL_X => Some(MouseEvent::Move { dx: value, dy: 0 }),
                RelativeAxisType::REL_Y => Some(MouseEvent::Move { dx: 0, dy: value }),
                RelativeAxisType::REL_WHEEL => Some(MouseEvent::Scroll { delta: value }),
                _ => None,
            }
        }
        InputEventKind::Key(key) => {
            let button = MouseButton::from(key);
            if button == MouseButton::Unknown {
                return None;
            }

            if event.value() == 1 {
                Some(MouseEvent::ButtonPress(button))
            } else if event.value() == 0 {
                Some(MouseEvent::ButtonRelease(button))
            } else {
                None
            }
        }
        _ => None,
    }
}
