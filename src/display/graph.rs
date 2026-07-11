//! Simple ASCII graph renderer for terminal display

#![allow(dead_code)]

pub struct Graph {
    pub title: String,
    pub width: usize,
    pub height: usize,
    pub data: Vec<f64>,
}

impl Graph {
    pub fn new(title: &str, width: usize, height: usize) -> Self {
        Self {
            title: title.to_string(),
            width,
            height,
            data: Vec::new(),
        }
    }

    pub fn push(&mut self, value: f64) {
        self.data.push(value);
        if self.data.len() > self.width {
            self.data.remove(0);
        }
    }

    pub fn render(&self) -> String {
        if self.data.is_empty() {
            return format!("{}: No data\n", self.title);
        }

        let max = self.data.iter().cloned().fold(f64::MIN, f64::max);
        let min = self.data.iter().cloned().fold(f64::MAX, f64::min);
        let range = if (max - min).abs() < f64::EPSILON {
            1.0
        } else {
            max - min
        };

        let mut output = format!("┌─ {} ─┐\n", self.title);

        for row in (0..self.height).rev() {
            let threshold = min + (range * row as f64 / self.height as f64);
            output.push('│');

            for val in &self.data {
                if *val >= threshold {
                    output.push('█');
                } else {
                    output.push(' ');
                }
            }

            // Pad remaining width
            for _ in 0..(self.width.saturating_sub(self.data.len())) {
                output.push(' ');
            }

            output.push_str("│\n");
        }

        output.push_str(&format!("└{}┘\n", "─".repeat(self.width)));
        output.push_str(&format!("Min: {:.1}  Max: {:.1}\n", min, max));

        output
    }
}
