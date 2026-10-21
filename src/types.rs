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

/// A mouse event stamped with the time the device reported it.
///
/// On Linux the time is the kernel's event timestamp and on Windows the
/// moment the raw-input thread received the report, so it does not depend on
/// how often the caller drains the queue. Timing measurements (polling rate,
/// stutter, lift-off) must use this rather than `Instant::now()` at read time.
#[derive(Debug, Clone)]
pub struct TimedEvent {
    pub time: std::time::Instant,
    pub event: MouseEvent,
}

/// What one read of the mouse returned.
#[derive(Debug, Clone, Default)]
pub struct InputBatch {
    /// The events in the order the device reported them.
    pub events: Vec<TimedEvent>,
    /// Reports were lost just before `events`: the reader fell behind and
    /// the system dropped the reports it had queued (the mouse itself stayed
    /// connected). The time from the last event before this batch to the
    /// first one in it is not a real gap between reports, and whole clicks
    /// can be missing, so timing must not be judged across it: forget the
    /// previous report and any press still waiting for its release. A button
    /// whose release was lost is released at the start of `events`; one
    /// pressed during the gap is not reported, so its release can arrive
    /// without a press.
    pub lost_data: bool,
}

/// The mouse stopped delivering input for good: it was unplugged, its
/// receiver was removed, or reading it failed. Nothing more will come from
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceLost;

impl std::fmt::Display for DeviceLost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the mouse stopped responding (unplugged or disconnected)")
    }
}

impl std::error::Error for DeviceLost {}

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

#[cfg(target_os = "linux")]
impl MouseButton {
    /// The evdev key code of this button (`None` for `Unknown`).
    pub fn key_code(self) -> Option<evdev::KeyCode> {
        match self {
            MouseButton::Left => Some(evdev::KeyCode::BTN_LEFT),
            MouseButton::Right => Some(evdev::KeyCode::BTN_RIGHT),
            MouseButton::Middle => Some(evdev::KeyCode::BTN_MIDDLE),
            MouseButton::Side => Some(evdev::KeyCode::BTN_SIDE),
            MouseButton::Extra => Some(evdev::KeyCode::BTN_EXTRA),
            MouseButton::Unknown => None,
        }
    }
}
