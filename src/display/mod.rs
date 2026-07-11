#![allow(dead_code)]

pub mod graph;

/// Prints a simple horizontal bar
pub fn print_bar(label: &str, value: f64, max: f64, width: usize) {
    let filled = ((value / max) * width as f64) as usize;
    let empty = width.saturating_sub(filled);

    println!(
        "{}: [{}{}] {:.1}",
        label,
        "█".repeat(filled),
        "░".repeat(empty),
        value
    );
}

/// Clears the terminal screen
pub fn clear_screen() {
    print!("\x1B[2J\x1B[1;1H");
}
