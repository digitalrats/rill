//! Basic filter example
//!
//! Run with: cargo run --example basic_filter

use kama_core_traits::AudioNode; // для init, reset, process
use kama_digital_filters::{BiquadFilter, FilterType};
use kama_dsp_common::filter::Filter; // для cutoff, q, gain_db

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Basic Filter Example ===\n");

    // Create a low-pass filter at 1000 Hz with Q=0.707
    let mut filter = BiquadFilter::new(FilterType::LowPass, 1000.0, 0.707, 0.0);
    filter.init(44100.0);

    println!("Filter type: LowPass");
    println!("Cutoff: {} Hz", filter.cutoff());
    println!("Q: {}", filter.q());

    // Generate a test signal (sine wave at 440 Hz and 2000 Hz)
    println!("\nProcessing test signal...");

    let num_samples = 4410; // 0.1 seconds
    let mut input_440 = Vec::with_capacity(num_samples);
    let mut input_2000 = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let t = i as f32 / 44100.0;
        input_440.push((2.0 * std::f32::consts::PI * 440.0 * t).sin());
        input_2000.push((2.0 * std::f32::consts::PI * 2000.0 * t).sin());
    }

    // Process 440 Hz (should pass through)
    let mut output_440 = vec![0.0; num_samples];
    let inputs = [input_440.as_slice()];
    let mut outputs = [output_440.as_mut_slice()];
    filter.process(&inputs, &mut outputs)?;

    let max_440: f32 = output_440
        .iter()
        .map(|&x: &f32| x.abs())
        .fold(0.0f32, |a, b| a.max(b));
    println!("440 Hz output max: {:.3}", max_440);

    // Reset filter
    filter.reset();

    // Process 2000 Hz (should be attenuated)
    let mut output_2000 = vec![0.0; num_samples];
    let inputs = [input_2000.as_slice()];
    let mut outputs = [output_2000.as_mut_slice()];
    filter.process(&inputs, &mut outputs)?;

    let max_2000: f32 = output_2000
        .iter()
        .map(|&x: &f32| x.abs())
        .fold(0.0f32, |a, b| a.max(b));
    println!("2000 Hz output max: {:.3}", max_2000);

    assert!(max_2000 < max_440, "High frequencies should be attenuated");

    println!("\n✅ Filter example completed");
    Ok(())
}
