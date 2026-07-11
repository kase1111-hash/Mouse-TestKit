//! Mouse TRAP CLI Application
//!
//! A terminal-based mouse testing utility that provides comprehensive diagnostics
//! for mouse hardware and performance. This CLI version requires Linux or Windows
//! for raw input device access.
//!
//! # Features
//!
//! - **Stutter Detection**: Identifies movement irregularities and timing issues
//! - **Polling Rate Monitor**: Measures actual polling rate in Hz
//! - **Click Testing**: Response time, stickiness, and double-click detection
//! - **DPI Accuracy**: Verifies mouse DPI settings against actual movement
//! - **Sensor Analysis**: Jitter, acceleration, and angle snapping detection
//!
//! # Usage
//!
//! Run the binary and select tests from the interactive menu.
//! Most tests require moving or clicking the mouse to gather data.

mod display;
#[cfg(target_os = "linux")]
mod input;
#[cfg(target_os = "windows")]
mod input_windows;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod terminal;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod tests;
mod usb;

#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::io::{self, Write};

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn main() {
    println!("╔════════════════════════════════════╗");
    println!("║        Mouse TRAP v0.1.0           ║");
    println!("║  Test Response And Positioning     ║");
    println!("╚════════════════════════════════════╝");
    println!();

    loop {
        print_menu();

        let choice = get_input("Select option: ");

        match choice.trim() {
            "1" => tests::stutter::run(),
            "2" => tests::polling::run(),
            "3" => usb::conflicts::scan(),
            "4" => tests::click_response::run(),
            "5" => tests::click_sticky::run(),
            "6" => tests::liftoff::run(),
            "7" => tests::dpi::run(),
            "8" => tests::angle_snap::run(),
            "9" => tests::acceleration::run(),
            "10" => tests::double_click::run(),
            "11" => tests::jitter::run(),
            "12" => tests::standard::run_all(),
            "0" => {
                println!("Exiting Mouse TRAP. Goodbye!");
                break;
            }
            _ => println!("Invalid option. Please try again."),
        }
        println!();
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn main() {
    println!("╔════════════════════════════════════╗");
    println!("║        Mouse TRAP v0.1.0           ║");
    println!("║  Test Response And Positioning     ║");
    println!("╚════════════════════════════════════╝");
    println!();
    println!("The CLI version requires Linux or Windows for raw input access.");
    println!("Please use mouse-testkit-gui instead for the graphical interface.");
    println!();
    println!("Run: cargo run --bin mouse-testkit-gui");
}

/// Displays the main menu with all available test options.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn print_menu() {
    println!("┌─────────────────────────────────────┐");
    println!("│              Main Menu              │");
    println!("├─────────────────────────────────────┤");
    println!("│  1. Stutter Detection Test          │");
    println!("│  2. Polling Rate Monitor            │");
    println!("│  3. USB Conflict Scanner            │");
    println!("│  4. Click Response Test             │");
    println!("│  5. Click Stickiness Test           │");
    println!("│  6. Lift-Off Jump Test              │");
    println!("├─────────────────────────────────────┤");
    println!("│  7. DPI Accuracy Test               │");
    println!("│  8. Angle Snapping Detection        │");
    println!("│  9. Acceleration Detection          │");
    println!("│ 10. Double-Click Test               │");
    println!("│ 11. Jitter Test                     │");
    println!("├─────────────────────────────────────┤");
    println!("│ 12. Run All Standard Tests          │");
    println!("│  0. Exit                            │");
    println!("└─────────────────────────────────────┘");
}

/// Prompts the user for input and returns the entered string.
///
/// Handles input errors gracefully by returning an empty string.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn get_input(prompt: &str) -> String {
    print!("{}", prompt);
    if let Err(e) = io::stdout().flush() {
        eprintln!("Warning: Failed to flush output: {}", e);
    }

    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(_) => input,
        Err(e) => {
            eprintln!("Error reading input: {}", e);
            eprintln!("Please check that stdin is available and try again.");
            String::new()
        }
    }
}
