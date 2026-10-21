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
use evdev::{Device, EventSummary, KeyCode, RelativeAxisCode, SynchronizationCode};
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::time::{Instant, SystemTime};

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
                        // Tests poll the keyboard and the mouse in one loop; a
                        // blocking read would stall the loop (and the 'q' key,
                        // timed samples, etc.) until the mouse produces an event.
                        if let Err(e) = device.set_nonblocking(true) {
                            eprintln!(
                                "Warning: could not set {} non-blocking: {}",
                                path.display(),
                                e
                            );
                        }
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
                            mice.push(MouseDevice { device, name, path });
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
        eprintln!(
            "Warning: {} device(s) could not be accessed due to permissions.",
            permission_denied_count
        );
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
        println!(
            "Invalid selection: {} is not a valid option (1-{}).",
            choice,
            mice.len()
        );
        return None;
    }

    Some(mice.into_iter().nth(choice - 1)?.device)
}

// Re-export canonical types from the shared library crate.
pub use mouse_testkit::types::{MouseButton, MouseEvent, TimedEvent};

/// Fetch pending events with one `Move` per hardware report.
///
/// evdev splits a report into separate REL_X and REL_Y events followed by
/// SYN_REPORT; they are merged here so a diagonal report is one move, not two.
/// Every event carries its kernel timestamp (converted to `Instant`) instead
/// of the time it was read, so batches drained together keep their real
/// spacing. Returns an empty list when nothing is pending.
#[cfg(target_os = "linux")]
pub fn fetch_timed_events(device: &mut Device) -> Vec<TimedEvent> {
    let events: Vec<evdev::InputEvent> = match device.fetch_events() {
        Ok(events) => events.collect(),
        Err(_) => return Vec::new(),
    };

    // Map kernel (CLOCK_REALTIME) timestamps onto the monotonic clock using
    // one reference pair for the whole batch, so intervals within it are exact.
    let ref_system = SystemTime::now();
    let ref_instant = Instant::now();
    group_reports(&events, |ts| {
        ref_system
            .duration_since(ts)
            .ok()
            .and_then(|age| ref_instant.checked_sub(age))
            .unwrap_or(ref_instant)
    })
}

/// Merge the axis events of each SYN_REPORT-terminated report into one move
/// and stamp every event via `to_instant(kernel timestamp)`.
#[cfg(target_os = "linux")]
fn group_reports(
    events: &[evdev::InputEvent],
    to_instant: impl Fn(SystemTime) -> Instant,
) -> Vec<TimedEvent> {
    let mut out = Vec::new();
    let (mut dx, mut dy) = (0, 0);
    let mut move_time = None;
    let flush = |out: &mut Vec<TimedEvent>, dx: &mut i32, dy: &mut i32, time: Option<Instant>| {
        if let Some(time) = time {
            if *dx != 0 || *dy != 0 {
                out.push(TimedEvent {
                    time,
                    event: MouseEvent::Move { dx: *dx, dy: *dy },
                });
            }
        }
        (*dx, *dy) = (0, 0);
    };
    for ev in events {
        if let EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) =
            ev.destructure()
        {
            flush(&mut out, &mut dx, &mut dy, move_time.take());
            continue;
        }
        match parse_event(ev) {
            Some(MouseEvent::Move { dx: x, dy: y }) => {
                dx += x;
                dy += y;
                move_time.get_or_insert_with(|| to_instant(ev.timestamp()));
            }
            Some(event) => out.push(TimedEvent {
                time: to_instant(ev.timestamp()),
                event,
            }),
            None => {}
        }
    }
    flush(&mut out, &mut dx, &mut dy, move_time);
    out
}

/// Parse an evdev event into a MouseEvent
#[cfg(target_os = "linux")]
pub fn parse_event(event: &evdev::InputEvent) -> Option<MouseEvent> {
    match event.destructure() {
        EventSummary::RelativeAxis(_, axis, value) => match axis {
            RelativeAxisCode::REL_X => Some(MouseEvent::Move { dx: value, dy: 0 }),
            RelativeAxisCode::REL_Y => Some(MouseEvent::Move { dx: 0, dy: value }),
            RelativeAxisCode::REL_WHEEL => Some(MouseEvent::Scroll { delta: value }),
            _ => None,
        },
        EventSummary::Key(_, key, value) => {
            let button = MouseButton::from(key);
            if button == MouseButton::Unknown {
                return None;
            }

            if value == 1 {
                Some(MouseEvent::ButtonPress(button))
            } else if value == 0 {
                Some(MouseEvent::ButtonRelease(button))
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use evdev::{EventType, InputEvent};

    fn rel(axis: RelativeAxisCode, value: i32) -> InputEvent {
        InputEvent::new(EventType::RELATIVE.0, axis.0, value)
    }

    fn syn() -> InputEvent {
        InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_REPORT.0,
            0,
        )
    }

    fn moves(events: &[TimedEvent]) -> Vec<(i32, i32)> {
        events
            .iter()
            .filter_map(|e| match e.event {
                MouseEvent::Move { dx, dy } => Some((dx, dy)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn x_and_y_of_one_report_become_one_move() {
        let now = Instant::now();
        let events = [
            rel(RelativeAxisCode::REL_X, 3),
            rel(RelativeAxisCode::REL_Y, -2),
            syn(),
            rel(RelativeAxisCode::REL_Y, 5),
            syn(),
        ];
        let out = group_reports(&events, |_| now);
        assert_eq!(moves(&out), vec![(3, -2), (0, 5)]);
    }

    #[test]
    fn buttons_and_wheel_pass_through_and_trailing_motion_is_kept() {
        let now = Instant::now();
        let events = [
            InputEvent::new(EventType::KEY.0, KeyCode::BTN_LEFT.0, 1),
            syn(),
            rel(RelativeAxisCode::REL_WHEEL, -1),
            syn(),
            rel(RelativeAxisCode::REL_X, 7),
        ];
        let out = group_reports(&events, |_| now);
        assert!(matches!(
            out[0].event,
            MouseEvent::ButtonPress(MouseButton::Left)
        ));
        assert!(matches!(out[1].event, MouseEvent::Scroll { delta: -1 }));
        assert_eq!(moves(&out), vec![(7, 0)]);
    }
}
