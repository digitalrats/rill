use crate::WdfElement;
use parking_lot::RwLock;
use rill_core::Transcendental;
use std::sync::Arc;

/// Analyze magnitude response of a WDF element chain
///
/// Returns a vector of (frequency, magnitude) pairs.
pub fn frequency_response_magnitude<T: Transcendental>(
    elements: &[Arc<RwLock<dyn WdfElement<T>>>],
    frequencies: &[T],
    _sample_rate: T,
) -> Vec<(T, T)> {
    let mut response = Vec::new();

    for &freq in frequencies {
        let two = T::from_f32(2.0);
        let omega = two * T::PI * freq;
        let mut mag = T::ONE;

        for element in elements {
            let r = element.read().port_resistance();
            let denom_sq = T::ONE + omega * omega * r * r;
            mag /= denom_sq.sqrt();
        }

        response.push((freq, mag));
    }

    response
}

/// Analyze total harmonic distortion (THD) of a WDF element
pub fn analyze_distortion<T: Transcendental>(
    element: &mut dyn WdfElement<T>,
    frequency: T,
    amplitude: T,
    sample_rate: T,
    num_cycles: usize,
) -> T {
    let num_samples = (sample_rate.to_f64() / frequency.to_f64() * num_cycles as f64) as usize;
    let mut output = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let two = T::from_f32(2.0);
        let t = T::from_f32(i as f32) / sample_rate;
        let sample = amplitude * (two * T::PI * frequency * t).sin();
        let b = element.process_incident(sample);
        output.push(b);
        element.update_state();
    }

    let peak_output = output.iter().cloned().fold(T::ZERO, T::max);
    let fundamental_amplitude = amplitude;

    ((peak_output - fundamental_amplitude) / fundamental_amplitude).abs()
}
