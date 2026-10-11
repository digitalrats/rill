//! Utility functions for EQ

/// Calculate logarithmic frequency spacing
pub fn log_spaced_frequencies(start_hz: f32, end_hz: f32, num_bands: usize) -> Vec<f32> {
    if num_bands == 0 {
        return Vec::new();
    }

    let start_log = start_hz.ln();
    let end_log = end_hz.ln();
    let step = (end_log - start_log) / (num_bands - 1) as f32;

    (0..num_bands)
        .map(|i| (start_log + i as f32 * step).exp())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_spaced_frequencies() {
        let freqs = log_spaced_frequencies(20.0, 20000.0, 5);
        assert_eq!(freqs.len(), 5);
        assert!((freqs[0] - 20.0).abs() < 0.1);
        assert!((freqs[4] - 20000.0).abs() < 0.1);

        // Should be increasing
        for i in 1..freqs.len() {
            assert!(freqs[i] > freqs[i - 1]);
        }
    }
}
