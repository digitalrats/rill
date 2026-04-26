//! Integration tests for BiquadFilter

use float_cmp::approx_eq;
use rill_core::time::ClockTick;
use rill_core::traits::{ActionContext, Algorithm};
use rill_digital_filters::{BiquadFilter, FilterType};
use rill_core_dsp::filters::{Filter, FilterParams};


/// Test parameter changes
#[test]
fn test_biquad_parameters() {
    println!("\n=== Test: Parameter Changes ===");

    let mut filter = BiquadFilter::new(FilterParams { filter_type: FilterType::LowPass, cutoff: 1000.0, q: 0.707, gain_db: 0.0 });
    filter.init(44100.0);

    // Test cutoff change
    filter.set_cutoff(2000.0);
    assert_eq!(filter.cutoff(), 2000.0);

    // Test Q change
    filter.set_q(2.0);
    assert_eq!(filter.q(), 2.0);

    // Test gain change
    filter.set_gain_db(3.0);
    assert_eq!(filter.gain_db(), 3.0);

    // Test type change (via new)
    let new_filter = BiquadFilter::new(FilterParams { filter_type: FilterType::HighPass, cutoff: 500.0, q: 1.0, gain_db: 0.0 });
    assert_eq!(new_filter.filter_type(), FilterType::HighPass);

    println!("All parameter changes verified");
}

/// Test stereo processing
#[test]
fn test_biquad_stereo() {
    println!("\n=== Test: Stereo Processing ===");

    let sample_rate = 44100.0;
    let mut left = BiquadFilter::new(FilterParams { filter_type: FilterType::LowPass, cutoff: 1000.0, q: 0.707, gain_db: 0.0 });
    let mut right = BiquadFilter::new(FilterParams { filter_type: FilterType::LowPass, cutoff: 1000.0, q: 0.707, gain_db: 0.0 });

    left.init(sample_rate);
    right.init(sample_rate);

    let num_samples = 1000;
    let mut left_input = Vec::with_capacity(num_samples);
    let mut right_input = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let t = i as f32 / sample_rate;
        left_input.push((2.0 * std::f32::consts::PI * 440.0 * t).sin());
        right_input.push((2.0 * std::f32::consts::PI * 880.0 * t).sin());
    }

    let mut left_output = vec![0.0; num_samples];
    let mut right_output = vec![0.0; num_samples];

    let tick = ClockTick::default();
    let ctx = ActionContext::new(&tick);

    // Process left channel
    left.process(Some(&left_input), &mut left_output, &ctx).unwrap();
    // Process right channel
    right.process(Some(&right_input), &mut right_output, &ctx).unwrap();

    // Verify both channels processed correctly
    assert!(left_output.iter().any(|&x| x != 0.0));
    assert!(right_output.iter().any(|&x| x != 0.0));

    println!("Stereo processing completed successfully");
}
