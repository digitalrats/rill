use std::sync::Arc;
use parking_lot::RwLock;
use crate::WdfElement;

/// Analyze magnitude response of a WDF element chain
///
/// Returns a vector of (frequency, magnitude) pairs.
pub fn frequency_response_magnitude(
    elements: &[Arc<RwLock<dyn WdfElement>>],
    frequencies: &[f64],
    _sample_rate: f64,
) -> Vec<(f64, f64)> {
    let mut response = Vec::new();

    for &freq in frequencies {
        let omega = 2.0 * std::f64::consts::PI * freq;
        let mut mag = 1.0;

        for element in elements {
            let r = element.read().port_resistance();
            let denom_sq = 1.0 + omega * omega * r * r;
            mag /= denom_sq.sqrt();
        }

        response.push((freq, mag));
    }

    response
}

/// Analyze total harmonic distortion (THD) of a WDF element
pub fn analyze_distortion(
    element: &mut dyn WdfElement,
    frequency: f64,
    amplitude: f64,
    sample_rate: f64,
    num_cycles: usize,
) -> f64 {
    let num_samples = (sample_rate / frequency * num_cycles as f64) as usize;
    let mut output = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let t = i as f64 / sample_rate;
        let sample = amplitude * (2.0 * std::f64::consts::PI * frequency * t).sin();
        let b = element.process_incident(sample);
        output.push(b);
        element.update_state();
    }

    let peak_output = output.iter().cloned().fold(0.0_f64, f64::max);
    let fundamental_amplitude = amplitude;

    ((peak_output - fundamental_amplitude) / fundamental_amplitude).abs()
}
