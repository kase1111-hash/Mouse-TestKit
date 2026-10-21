//! Terminal handling utilities with proper error reporting
//!
//! Provides safe wrappers around terminal operations that give users
//! meaningful feedback when something goes wrong.

use mouse_testkit::types::{MouseEvent, TimedEvent};
use std::io::{self, Write};
use std::time::{Duration, Instant};

// Windows-specific imports
#[cfg(target_os = "windows")]
use crate::input_windows::MouseDevice;

/// Enable raw mode with error reporting
/// Returns true if successful, false otherwise
pub fn enable_raw_mode() -> bool {
    match crossterm::terminal::enable_raw_mode() {
        Ok(_) => true,
        Err(e) => {
            eprintln!("Warning: Could not enable raw terminal mode: {}", e);
            eprintln!("Some features like 'q' to quit may not work properly.");
            eprintln!("This can happen when running in certain terminal emulators or via SSH.");
            false
        }
    }
}

/// Disable raw mode with error reporting
/// Returns true if successful, false otherwise
pub fn disable_raw_mode() -> bool {
    match crossterm::terminal::disable_raw_mode() {
        Ok(_) => true,
        Err(e) => {
            eprintln!("Warning: Could not restore terminal mode: {}", e);
            eprintln!("Your terminal may behave unexpectedly. Try running 'reset' command.");
            false
        }
    }
}

/// Grab a device exclusively with error reporting
/// Returns true if successful, false otherwise
#[cfg(target_os = "linux")]
pub fn grab_device(device: &mut evdev::Device) -> bool {
    match device.grab() {
        Ok(_) => true,
        Err(e) => {
            eprintln!("Warning: Could not grab device exclusively: {}", e);
            eprintln!("Other applications may interfere with mouse input.");
            eprintln!("Close other mouse-related software for best results.");
            false
        }
    }
}

/// Grab a device on Windows (no-op, Raw Input handles this differently)
/// Returns true always since Windows Raw Input doesn't require exclusive access
#[cfg(target_os = "windows")]
pub fn grab_device(_device: &mut MouseDevice) -> bool {
    // Windows Raw Input API doesn't require exclusive device grabbing
    // The input is received via WM_INPUT messages without blocking other apps
    true
}

/// Flush stdout with error handling
pub fn flush_stdout() {
    if let Err(e) = io::stdout().flush() {
        eprintln!("Warning: Could not flush output: {}", e);
    }
}

/// Wait for user to press Enter with error handling
pub fn wait_for_enter() {
    print!("\nPress Enter to continue...");
    flush_stdout();

    let mut input = String::new();
    if let Err(e) = io::stdin().read_line(&mut input) {
        eprintln!("Warning: Could not read input: {}", e);
    }
}

/// Discard the input queued for a device so far. A device node buffers its
/// events from the moment it is opened, so clicks and movement made while a
/// prompt (such as the device list) waited would otherwise reach the test
/// with stale timestamps.
pub fn discard_queued(mut fetch: impl FnMut() -> Vec<TimedEvent>) {
    while !fetch().is_empty() {}
}

/// Seconds from `start` to an event at `t`, or `None` for an event from
/// before `start`: input queued earlier must be left out, not counted as
/// happening at the start (two stale clicks would be 0 ms apart).
pub fn secs_after(start: Instant, t: Instant) -> Option<f64> {
    t.checked_duration_since(start).map(|d| d.as_secs_f64())
}

/// `text` with every line ending as "\r\n", for printing in raw mode: there
/// the terminal does not turn "\n" into a return to the start of the line,
/// so lines printed with a bare "\n" each start where the last one ended.
pub fn raw_lines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// How long [`confirm_device_moves`] waits for movement.
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(10);

/// Ask the user to nudge the mouse and wait for movement from the device:
/// proof that the device read is the mouse being tested and that it is
/// connected. Input queued before the prompt does not count. Returns false
/// when nothing arrived within [`CONFIRM_TIMEOUT`].
///
/// Tests in which a healthy mouse sends nothing (sensor at rest, clean
/// lifts) need this before "nothing seen" can mean a clean result: a silent
/// device node (the wrong one of several, one held by another program, or a
/// mouse that was unplugged) sends nothing too.
pub fn confirm_device_moves(prompt: &str, mut fetch: impl FnMut() -> Vec<TimedEvent>) -> bool {
    discard_queued(&mut fetch);
    print!("{} ", prompt);
    flush_stdout();
    let deadline = Instant::now() + CONFIRM_TIMEOUT;
    while Instant::now() < deadline {
        let moved = fetch()
            .iter()
            .any(|e| matches!(e.event, MouseEvent::Move { dx, dy } if dx != 0 || dy != 0));
        if moved {
            println!("OK");
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    println!("no movement.");
    false
}

/// RAII guard that enables raw terminal mode on creation and disables it on drop.
/// Guarantees terminal state cleanup even if a test panics.
///
/// On Windows it also takes the console's mouse input for the program while
/// it lives. Raw Input does not keep the tested mouse's clicks from the
/// window under the pointer, which is usually this console: otherwise a
/// right-click pastes the clipboard into the test's key input (a 'q' in it
/// ends the test) and a left click starts a QuickEdit selection that pauses
/// the program's output. With mouse input enabled those clicks arrive as
/// mouse events, which the tests ignore.
pub struct TerminalGuard {
    raw_mode_enabled: bool,
    #[cfg(target_os = "windows")]
    mouse_captured: bool,
}

impl TerminalGuard {
    pub fn new() -> Self {
        let raw_mode_enabled = enable_raw_mode();
        #[cfg(target_os = "windows")]
        let mouse_captured =
            crossterm::execute!(io::stdout(), crossterm::event::EnableMouseCapture).is_ok();
        Self {
            raw_mode_enabled,
            #[cfg(target_os = "windows")]
            mouse_captured,
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // In reverse order: mouse capture restores the raw-mode console mode
        #[cfg(target_os = "windows")]
        if self.mouse_captured {
            crossterm::execute!(io::stdout(), crossterm::event::DisableMouseCapture).ok();
        }
        if self.raw_mode_enabled {
            disable_raw_mode();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_lines_return_to_the_start_of_each_line() {
        assert_eq!(raw_lines("a\nb\r\nc\n"), "a\r\nb\r\nc\r\n");
    }

    #[test]
    fn events_from_before_the_start_are_left_out() {
        let start = Instant::now();
        assert_eq!(
            secs_after(start, start + Duration::from_millis(250)),
            Some(0.25)
        );
        if let Some(earlier) = start.checked_sub(Duration::from_millis(5)) {
            assert_eq!(secs_after(start, earlier), None);
        }
    }

    #[test]
    fn discard_queued_drains_every_batch() {
        let mut batches = vec![
            vec![],
            vec![TimedEvent {
                time: Instant::now(),
                event: MouseEvent::Move { dx: 1, dy: 0 },
            }],
            vec![TimedEvent {
                time: Instant::now(),
                event: MouseEvent::Move { dx: 1, dy: 0 },
            }],
        ];
        // Stops at the first empty fetch: nothing more is queued
        discard_queued(|| batches.pop().unwrap_or_default());
        assert!(batches.is_empty());
    }
}
