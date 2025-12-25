/// Input handling module
/// Provides mouse device detection and event reading via evdev

use evdev::{Device, InputEventKind, RelativeAxisType, Key};
use std::fs;
use std::path::PathBuf;

pub struct MouseDevice {
    pub device: Device,
    pub name: String,
    pub path: PathBuf,
}

/// Find all mouse devices in the system
pub fn find_mouse_devices() -> Vec<MouseDevice> {
    let mut mice = Vec::new();

    if let Ok(entries) = fs::read_dir("/dev/input") {
        for entry in entries.flatten() {
            let path = entry.path();

            // Only check event devices
            if !path.to_string_lossy().contains("event") {
                continue;
            }

            if let Ok(device) = Device::open(&path) {
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
        }
    }

    mice
}

/// Select a mouse device interactively
pub fn select_mouse() -> Option<Device> {
    let mice = find_mouse_devices();

    if mice.is_empty() {
        println!("No mouse devices found!");
        println!("Make sure you have read permissions on /dev/input/event* devices.");
        println!("Try running with sudo or add your user to the 'input' group.");
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
    std::io::stdin().read_line(&mut input).ok()?;

    let choice: usize = input.trim().parse().ok()?;
    if choice == 0 || choice > mice.len() {
        return None;
    }

    Some(mice.into_iter().nth(choice - 1)?.device)
}

#[derive(Debug, Clone)]
pub enum MouseEvent {
    Move { dx: i32, dy: i32 },
    ButtonPress(MouseButton),
    ButtonRelease(MouseButton),
    Scroll { delta: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Side,
    Extra,
    Unknown,
}

impl From<Key> for MouseButton {
    fn from(key: Key) -> Self {
        match key {
            Key::BTN_LEFT => MouseButton::Left,
            Key::BTN_RIGHT => MouseButton::Right,
            Key::BTN_MIDDLE => MouseButton::Middle,
            Key::BTN_SIDE => MouseButton::Side,
            Key::BTN_EXTRA => MouseButton::Extra,
            _ => MouseButton::Unknown,
        }
    }
}

/// Parse an evdev event into a MouseEvent
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
