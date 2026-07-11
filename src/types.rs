//! Shared mouse event types
//!
//! Platform-agnostic event and button types used by both CLI and GUI binaries.
//! Platform-specific input modules re-export these types so existing imports
//! continue to work.

/// Parsed mouse event types.
///
/// These types are available on all platforms for use in shared code.
#[derive(Debug, Clone)]
pub enum MouseEvent {
    Move {
        dx: i32,
        dy: i32,
    },
    ButtonPress(MouseButton),
    ButtonRelease(MouseButton),
    Scroll {
        #[allow(dead_code)]
        delta: i32,
    },
}

/// Mouse button identifiers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    /// Side button (often "back" on gaming mice)
    Side,
    /// Extra button (often "forward" on gaming mice)
    Extra,
    Unknown,
}

#[cfg(target_os = "linux")]
impl From<evdev::KeyCode> for MouseButton {
    fn from(key: evdev::KeyCode) -> Self {
        match key {
            evdev::KeyCode::BTN_LEFT => MouseButton::Left,
            evdev::KeyCode::BTN_RIGHT => MouseButton::Right,
            evdev::KeyCode::BTN_MIDDLE => MouseButton::Middle,
            evdev::KeyCode::BTN_SIDE => MouseButton::Side,
            evdev::KeyCode::BTN_EXTRA => MouseButton::Extra,
            _ => MouseButton::Unknown,
        }
    }
}
