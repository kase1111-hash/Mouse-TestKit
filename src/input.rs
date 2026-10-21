//! Input handling module for Linux
//!
//! Provides mouse device detection and raw event reading via the evdev subsystem.
//! This module handles:
//! - Enumerating available mouse devices in /dev/input
//! - Interactive device selection
//! - Reading the selected mouse ([`read_events`]): one event per hardware
//!   report with the kernel's timestamp, gaps where the kernel dropped
//!   reports, and a mouse that was unplugged (via
//!   `mouse_testkit::evdev_input`, shared with the GUI)
//!
//! # Permissions
//!
//! Requires read access to /dev/input/event* devices. Users may need to be
//! added to the 'input' group: `sudo usermod -aG input $USER`

#[cfg(target_os = "linux")]
use evdev::raw_stream::RawDevice;
#[cfg(target_os = "linux")]
use evdev::{KeyCode, RelativeAxisCode};
#[cfg(target_os = "linux")]
use mouse_testkit::evdev_input::{self, EvdevMouse};
#[cfg(target_os = "linux")]
use std::collections::VecDeque;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::time::Duration;

/// A mouse-capable device found in /dev/input.
#[cfg(target_os = "linux")]
pub struct MouseDeviceInfo {
    /// The opened device node
    pub device: RawDevice,
    /// Human-readable device name
    pub name: String,
    /// Path to the device file (e.g., /dev/input/event5)
    pub path: PathBuf,
}

/// A mouse opened for a test. Read it with [`read_events`].
#[cfg(target_os = "linux")]
pub struct MouseDevice {
    mouse: EvdevMouse,
    /// Batches read but not yet returned: the events after a gap are
    /// returned on their own, so `lost_data` marks exactly where it is.
    queued: VecDeque<InputBatch>,
}

#[cfg(target_os = "linux")]
impl MouseDevice {
    /// Take the device for this program alone (other programs, including
    /// the desktop, stop receiving its input until it is dropped).
    pub fn grab(&mut self) -> std::io::Result<()> {
        self.mouse.device_mut().grab()
    }
}

/// Find all mouse devices in the system
#[cfg(target_os = "linux")]
pub fn find_mouse_devices() -> Vec<MouseDeviceInfo> {
    let mut mice = Vec::new();
    let mut permission_denied_count = 0;
    // Of those, nodes that report X motion (a mouse the user cannot open)
    let mut denied_mice = 0;

    match fs::read_dir("/dev/input") {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();

                // Only check event devices
                if !path.to_string_lossy().contains("event") {
                    continue;
                }

                match RawDevice::open(&path) {
                    Ok(device) => {
                        // Check if device has mouse-like capabilities
                        let has_rel_x = device
                            .supported_relative_axes()
                            .map(|axes| axes.contains(RelativeAxisCode::REL_X))
                            .unwrap_or(false);

                        let has_left_btn = device
                            .supported_keys()
                            .map(|keys| keys.contains(KeyCode::BTN_LEFT))
                            .unwrap_or(false);

                        if has_rel_x && has_left_btn {
                            let name = device.name().unwrap_or("Unknown Mouse").to_string();
                            mice.push(MouseDeviceInfo { device, name, path });
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::PermissionDenied {
                            permission_denied_count += 1;
                            let node = entry.file_name();
                            if evdev_input::node_reports_rel_x(&node.to_string_lossy()) {
                                denied_mice += 1;
                            }
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

    // Warn user if we encountered permission issues. A mouse that cannot be
    // opened is worth a warning even when another device can be: the one
    // listed may not be the mouse being tested.
    if permission_denied_count > 0 && (mice.is_empty() || denied_mice > 0) {
        eprintln!();
        if denied_mice > 0 {
            eprintln!(
                "Warning: {} mouse device(s) could not be opened (permission denied).",
                denied_mice
            );
            if !mice.is_empty() {
                eprintln!("If the mouse you want to test is not listed below, it is one of them.");
            }
        } else {
            eprintln!(
                "Warning: {} device(s) could not be accessed due to permissions.",
                permission_denied_count
            );
        }
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
pub fn select_mouse() -> Option<MouseDevice> {
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
        return open(mice.into_iter().next()?);
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
        println!(
            "Invalid selection: {} is not a valid option (1-{}).",
            choice,
            mice.len()
        );
        return None;
    }

    open(mice.into_iter().nth(choice - 1)?)
}

/// Prepare a found mouse for reading.
#[cfg(target_os = "linux")]
fn open(info: MouseDeviceInfo) -> Option<MouseDevice> {
    match EvdevMouse::new(info.device) {
        Ok(mouse) => Some(MouseDevice {
            mouse,
            queued: VecDeque::new(),
        }),
        Err(e) => {
            eprintln!(
                "Error: could not prepare {} for reading: {}",
                info.path.display(),
                e
            );
            None
        }
    }
}

// Re-export canonical types from the shared library crate.
pub use mouse_testkit::types::{DeviceLost, InputBatch, MouseButton, MouseEvent, TimedEvent};

/// Read the mouse: wait up to `wait` for input (not at all when some is
/// already pending), then return everything pending, one `Move` per
/// hardware report, each stamped with its kernel timestamp.
///
/// A batch with `lost_data` starts after reports the kernel dropped because
/// the reader fell behind (see [`InputBatch`]); events after such a gap
/// always come in a batch of their own. Returns [`DeviceLost`] once the
/// mouse is unplugged or can no longer be read, after the input read
/// before that was returned.
#[cfg(target_os = "linux")]
pub fn read_events(device: &mut MouseDevice, wait: Duration) -> Result<InputBatch, DeviceLost> {
    if device.queued.is_empty() {
        if !wait.is_zero() {
            device.mouse.wait(Some(wait));
        }
        // Kernel timestamps are kept however old: input queued while a
        // prompt waited must be recognised as older than the test.
        device.queued.extend(device.mouse.read(None)?);
    }
    Ok(device.queued.pop_front().unwrap_or_default())
}

/// Pending events, without waiting. Prefer [`read_events`]: here lost
/// reports and a disconnected mouse only show as missing events.
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub fn fetch_timed_events(device: &mut MouseDevice) -> Vec<TimedEvent> {
    read_events(device, Duration::ZERO)
        .map(|batch| batch.events)
        .unwrap_or_default()
}
