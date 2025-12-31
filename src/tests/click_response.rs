/// Click Response Test
/// Measures click latency and response time

use std::time::{Duration, Instant};
use crossterm::event::{self, Event, KeyCode};
use crate::input::{self, MouseEvent, MouseButton};

const NUM_TRIALS: usize = 10;

pub fn run() {
    println!("\n=== Click Response Test ===");
    println!("This test measures your click reaction time.");
    println!("When you see 'CLICK NOW!', click as fast as possible.");
    println!("Press any key to start, or 'q' to quit...\n");

    crossterm::terminal::enable_raw_mode().ok();

    loop {
        if event::poll(Duration::from_millis(100)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                if key.code == KeyCode::Char('q') {
                    crossterm::terminal::disable_raw_mode().ok();
                    return;
                }
                break;
            }
        }
    }

    crossterm::terminal::disable_raw_mode().ok();

    let mut device = match input::select_mouse() {
        Some(d) => d,
        None => {
            println!("\nNo mouse selected. Returning to menu...");
            wait_for_enter();
            return;
        }
    };

    device.grab().ok();

    let mut results: Vec<ClickResult> = Vec::new();

    for trial in 0..NUM_TRIALS {
        let delay = 1000 + (trial * 200) % 2000;
        println!("\nTrial {}/{}  -  Get ready...", trial + 1, NUM_TRIALS);

        // Drain pending events (fetch once to clear buffer)
        let _ = device.fetch_events();

        std::thread::sleep(Duration::from_millis(delay as u64));

        let prompt_time = Instant::now();
        println!(">>> CLICK NOW! <<<");

        let mut clicked = false;
        let timeout = Duration::from_secs(5);

        while prompt_time.elapsed() < timeout {
            if let Ok(events) = device.fetch_events() {
                for ev in events {
                    if let Some(MouseEvent::ButtonPress(button)) = input::parse_event(&ev) {
                        let latency = prompt_time.elapsed().as_secs_f64() * 1000.0;
                        results.push(ClickResult { latency_ms: latency, button });
                        println!("Response time: {:.1} ms ({:?})", latency, button);
                        clicked = true;
                        break;
                    }
                }
            }
            if clicked { break; }
            std::thread::sleep(Duration::from_millis(1));
        }

        if !clicked { println!("Timeout - no click detected"); }
        std::thread::sleep(Duration::from_millis(500));
    }

    println!("\n=== Click Response Test Complete ===\n");

    if !results.is_empty() {
        let avg = results.iter().map(|r| r.latency_ms).sum::<f64>() / results.len() as f64;
        let mut sorted: Vec<f64> = results.iter().map(|r| r.latency_ms).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        println!("Results ({} clicks):", results.len());
        println!("  Average: {:.1} ms", avg);
        println!("  Minimum: {:.1} ms", sorted.first().unwrap_or(&0.0));
        println!("  Maximum: {:.1} ms", sorted.last().unwrap_or(&0.0));
        if sorted.len() >= 2 { println!("  Median:  {:.1} ms", sorted[sorted.len() / 2]); }

        println!("\nAll responses:");
        for (i, result) in results.iter().enumerate() {
            println!("  Trial {}: {:.1} ms", i + 1, result.latency_ms);
        }
    } else {
        println!("No clicks recorded.");
    }

    wait_for_enter();
}

fn wait_for_enter() {
    println!("\nPress Enter to return to menu...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
}

#[derive(Clone)]
pub struct ClickResult {
    pub latency_ms: f64,
    pub button: MouseButton,
}
