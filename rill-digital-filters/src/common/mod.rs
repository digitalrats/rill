//! Common imports and utilities for tests

pub use rill_digital_filters::{BiquadFilter, FilterType};
pub use rill_core::traits::Node;  // для init, reset, process
pub use rill_core_dsp::filters::Filter;  // для cutoff, q, gain_db
pub use float_cmp::approx_eq;

// Helper function to generate sine wave
pub fn generate_sine(freq: f32, sample_rate: f32, duration_secs: f32) -> Vec<f32> {
    let num_samples = (sample_rate * duration_secs) as usize;
    let mut signal = Vec::with_capacity(num_samples);
    
    for i in 0..num_samples {
        let t = i as f32 / sample_rate;
        signal.push((2.0 * std::f32::consts::PI * freq * t).sin());
    }
    
    signal
}