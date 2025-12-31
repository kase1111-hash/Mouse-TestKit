/// Terminal handling utilities with proper error reporting
///
/// Provides safe wrappers around terminal operations that give users
/// meaningful feedback when something goes wrong.

use std::io::{self, Write};

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

/// Cleanup handler for tests - restores terminal state
pub struct TerminalGuard {
    raw_mode_enabled: bool,
}

impl TerminalGuard {
    pub fn new() -> Self {
        let raw_mode_enabled = enable_raw_mode();
        Self { raw_mode_enabled }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.raw_mode_enabled {
            disable_raw_mode();
        }
    }
}
