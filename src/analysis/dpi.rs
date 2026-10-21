//! DPI accuracy rules shared by the GUI panel and the CLI test.

/// A measurement this far off the DPI the user entered (fraction) is almost
/// always the setup, not the sensor: the mouse on a different DPI stage, a
/// wrong distance, or a missed mark. Real sensors deviate by a few percent.
pub const SETUP_ERROR_FRACTION: f64 = 0.25;

/// Whether `measured` DPI is so far from `expected` that the setup should be
/// checked before the sensor is judged (see [`SETUP_ERROR_FRACTION`]).
pub fn is_setup_error(measured: f64, expected: f64) -> bool {
    expected > 0.0 && (measured / expected - 1.0).abs() > SETUP_ERROR_FRACTION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_different_dpi_stage_is_a_setup_error() {
        assert!(is_setup_error(1600.0, 800.0));
        assert!(is_setup_error(400.0, 800.0));
        assert!(!is_setup_error(1000.0, 800.0));
        assert!(!is_setup_error(820.0, 800.0));
    }
}
