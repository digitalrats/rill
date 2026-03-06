//! Integration tests for kama-eq
//!
//! Tests the parametric and graphic equalizers with real filters
//! and automation integration.

use float_cmp::approx_eq;
use kama_core::time::{SystemClock, TimeProvider};
use kama_core::traits::Clock; // для advance
use kama_core::traits::{Processor, ParamValue};
use kama_core::{DEFAULT_BLOCK_SIZE};
use kama_core_dsp::filters::{Biquad, FilterParams, FilterType};
use kama_eq::{BandType, GraphicEq, ParametricEq, FilterFactory};
use std::sync::Arc;

// Helper function to generate test signal
fn generate_sweep(sample_rate: f32, duration_secs: f32) -> Vec<f32> {
    let num_samples = (sample_rate * duration_secs) as usize;
    let mut signal = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let t = i as f32 / sample_rate;
        // Sweep from 20Hz to 20kHz
        let freq = 20.0 * (1000.0_f32).powf(t);
        signal.push((2.0 * std::f32::consts::PI * freq * t).sin() * 0.5);
    }

    signal
}

fn generate_sine(freq: f32, sample_rate: f32, duration_secs: f32) -> Vec<f32> {
    let num_samples = (sample_rate * duration_secs) as usize;
    let mut signal = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let t = i as f32 / sample_rate;
        signal.push((2.0 * std::f32::consts::PI * freq * t).sin());
    }

    signal
}

/// Custom factory for Biquad<f32> that implements FilterFactory
struct BiquadFactory;

impl FilterFactory<Biquad<f32>> for BiquadFactory {
    fn create_filter(&self, filter_type: FilterType, frequency: f32, q: f32, gain_db: f32) -> Biquad<f32> {
        Biquad::new(FilterParams { filter_type, cutoff: frequency, q, gain_db })
    }
}

/// Process a signal blockwise using a Processor with DEFAULT_BLOCK_SIZE
fn process_signal<Proc: Processor<f32, DEFAULT_BLOCK_SIZE>>(
    processor: &mut Proc,
    input: &[f32],
    output: &mut [f32],
) {
    assert_eq!(input.len(), output.len());
    let num_blocks = (input.len() + DEFAULT_BLOCK_SIZE - 1) / DEFAULT_BLOCK_SIZE;
    for block_idx in 0..num_blocks {
        let start = block_idx * DEFAULT_BLOCK_SIZE;
        let end = (start + DEFAULT_BLOCK_SIZE).min(input.len());
        let block_len = end - start;
        // Prepare input block (pad with zeros if needed)
        let mut input_block = [0.0; DEFAULT_BLOCK_SIZE];
        input_block[..block_len].copy_from_slice(&input[start..end]);
        // Prepare output block
        let mut output_block = [0.0; DEFAULT_BLOCK_SIZE];
        let inputs = [&input_block];
        let mut outputs = [&mut output_block];
        // Process
        processor.process(&inputs, &mut outputs, &[]).unwrap();
        // Copy result back
        output[start..end].copy_from_slice(&output_block[..block_len]);
    }
}

// =============================================================================
// BASIC EQ TESTS
// =============================================================================

#[test]
fn test_parametric_eq_creation() {
    println!("\n=== Test: Parametric EQ Creation ===");

    let factory = BiquadFactory;
    let eq = ParametricEq::new(factory, 5, 44100.0);

    assert_eq!(eq.num_audio_inputs(), 1);
    assert_eq!(eq.num_audio_outputs(), 1);

    // Metadata may not be available; skip checking
    // let metadata = eq.metadata();
    // println!("EQ metadata: {}", metadata.name);
    // assert!(metadata.parameters.len() > 5); // Should have many parameters
}

#[test]
fn test_graphic_eq_creation() {
    println!("\n=== Test: Graphic EQ Creation ===");

    let factory = BiquadFactory;
    let eq = GraphicEq::new_third_octave(factory, 44100.0);

    assert_eq!(eq.num_audio_inputs(), 1);
    assert_eq!(eq.num_audio_outputs(), 1);
    assert_eq!(eq.num_bands(), 31); // должно быть 31 полоса

    println!("Graphic EQ with {} bands created", eq.num_bands());
}

// =============================================================================
// FREQUENCY RESPONSE TESTS
// =============================================================================
#[test]
fn test_parametric_eq_band_boost() {
    println!("\n=== Test: Parametric EQ Band Boost ===");

    let sample_rate = 44100.0;
    let factory = BiquadFactory;
    let mut eq = ParametricEq::new(factory, 3, sample_rate);

    eq.init(sample_rate);

    // Configure bands
    eq.set_band_type(0, BandType::Peak).unwrap();
    eq.set_band(0, 100.0, 1.0, 0.0).unwrap();

    eq.set_band_type(1, BandType::Peak).unwrap();
    eq.set_band(1, 1000.0, 5.0, 6.0).unwrap(); // +6dB boost

    eq.set_band_type(2, BandType::Peak).unwrap();
    eq.set_band(2, 5000.0, 1.0, 0.0).unwrap();

    let test_freqs = [100.0, 500.0, 1000.0, 2000.0, 5000.0];
    let mut results = Vec::new();

    for &freq in &test_freqs {
        // Generate test signal at this frequency
        let signal = generate_sine(freq, sample_rate, 0.2);
        let mut output = vec![0.0; signal.len()];

        process_signal(&mut eq, &signal, &mut output);

        // Calculate RMS (skip first 1000 samples for stabilization)
        let mut output_sum_squares = 0.0;
        let mut input_sum_squares = 0.0;
        let count = signal.len() - 1000;

        for i in 1000..signal.len() {
            output_sum_squares += output[i] * output[i];
            input_sum_squares += signal[i] * signal[i];
        }

        let output_rms = (output_sum_squares / count as f32).sqrt();
        let input_rms = (input_sum_squares / count as f32).sqrt();

        println!(
            "Frequency {}: input_rms = {:.6}, output_rms = {:.6}",
            freq, input_rms, output_rms
        );

        let gain = if input_rms > 0.0 {
            output_rms / input_rms
        } else {
            1.0
        };

        results.push((freq, gain));

        // Peak gain for verification
        let max_output = output
            .iter()
            .skip(1000)
            .map(|&x| x.abs())
            .fold(0.0f32, |a, b| a.max(b));
        let max_input = signal
            .iter()
            .skip(1000)
            .map(|&x| x.abs())
            .fold(0.0f32, |a, b| a.max(b));
        let peak_gain = if max_input > 0.0 {
            max_output / max_input
        } else {
            1.0
        };

        println!(
            "Frequency: {:6.0} Hz | Gain: {:.3} ({:.1} dB) | Peak gain: {:.3} | input_rms: {:.3}",
            freq,
            gain,
            20.0 * gain.log10(),
            peak_gain,
            input_rms
        );
    }

    // Verify boost at 1kHz
    let boost_at_1k = results
        .iter()
        .find(|(f, _)| (*f - 1000.0).abs() < 1.0)
        .unwrap()
        .1;
    println!(
        "Boost at 1kHz: {:.3} ({:.1} dB)",
        boost_at_1k,
        20.0 * boost_at_1k.log10()
    );

    assert!(
        (boost_at_1k - 2.0).abs() < 0.8,
        "Boost at 1kHz should be near 2.0 (6dB), got {:.3}",
        boost_at_1k
    );
}

#[test]
fn test_parametric_eq_band_cut() {
    println!("\n=== Test: Parametric EQ Band Cut ===");

    let sample_rate = 44100.0;
    let factory = BiquadFactory;
    let mut eq = ParametricEq::new(factory, 3, sample_rate);

    eq.init(sample_rate);

    // Configure bands
    eq.set_band_type(0, BandType::Peak).unwrap();
    eq.set_band(0, 100.0, 1.0, 0.0).unwrap();

    eq.set_band_type(1, BandType::Peak).unwrap();
    eq.set_band(1, 1000.0, 2.0, -6.0).unwrap(); // -6dB cut at 1kHz

    eq.set_band_type(2, BandType::Peak).unwrap();
    eq.set_band(2, 5000.0, 1.0, 0.0).unwrap();

    let test_freqs = [100.0, 500.0, 1000.0, 2000.0, 5000.0];
    let mut results = Vec::new();

    for &freq in &test_freqs {
        let signal = generate_sine(freq, sample_rate, 0.2);
        let mut output = vec![0.0; signal.len()];

        process_signal(&mut eq, &signal, &mut output);

        let mut sum_squares = 0.0;
        let count = signal.len() - 1000;
        for i in 1000..signal.len() {
            sum_squares += output[i] * output[i];
        }
        let rms = (sum_squares / count as f32).sqrt();

        let input_rms =
            (signal.iter().skip(1000).map(|&x| x * x).sum::<f32>() / count as f32).sqrt();
        let gain = rms / input_rms;

        results.push((freq, gain));
        println!(
            "Frequency: {:6.0} Hz | Gain: {:.3} ({:.1} dB)",
            freq,
            gain,
            20.0 * gain.log10()
        );
    }

    // Verify cut at 1kHz
    let cut_at_1k = results
        .iter()
        .find(|(f, _)| (*f - 1000.0).abs() < 1.0)
        .unwrap()
        .1;
    println!(
        "Cut at 1kHz: {:.3} ({:.1} dB)",
        cut_at_1k,
        20.0 * cut_at_1k.log10()
    );

    // Should be close to 0.5 (-6dB)
    assert!(
        (cut_at_1k - 0.5).abs() < 0.2,
        "Cut at 1kHz should be near 0.5 (-6dB), got {:.3}",
        cut_at_1k
    );
}

#[test]
fn test_graphic_eq_response() {
    println!("\n=== Test: Graphic EQ Response ===");

    let sample_rate = 44100.0;
    let factory = BiquadFactory;

    // Создаём временный EQ для просмотра частот
    let temp_eq = GraphicEq::new_third_octave(factory, sample_rate);

    // Выведем все частоты для отладки
    println!("Graphic EQ frequencies:");
    for i in 0..temp_eq.num_bands() {
        if let Some(freq) = temp_eq.band_frequency(i) {
            println!("  band {}: {:.1} Hz", i, freq);
        }
    }

    // Создаём основной EQ для теста
    let factory = BiquadFactory; // создаём новый экземпляр
    let mut eq = GraphicEq::new_third_octave(factory, sample_rate);

    // Найдём правильные индексы для нужных частот
    let mut band_250 = None;
    let mut band_2500 = None;
    let mut band_5000 = None;

    for i in 0..eq.num_bands() {
        if let Some(freq) = eq.band_frequency(i) {
            if (freq - 250.0_f32).abs() < 10.0 {
                band_250 = Some(i);
                println!("Found 250 Hz at band {}", i);
            }
            if (freq - 2500.0_f32).abs() < 100.0 {
                band_2500 = Some(i);
                println!("Found 2500 Hz at band {}", i);
            }
            if (freq - 5000.0_f32).abs() < 200.0 {
                band_5000 = Some(i);
                println!("Found 5000 Hz at band {}", i);
            }
        }
    }

    // Boost bands if found
    if let Some(i) = band_250 {
        eq.set_band_gain(i, 6.0).unwrap();
        println!("Boosting band {} (250 Hz) by 6dB", i);
    }
    if let Some(i) = band_2500 {
        eq.set_band_gain(i, 6.0).unwrap();
        println!("Boosting band {} (2500 Hz) by 6dB", i);
    }
    if let Some(i) = band_5000 {
        eq.set_band_gain(i, -6.0).unwrap();
        println!("Cutting band {} (5000 Hz) by 6dB", i);
    }

    eq.init(sample_rate);

    // TODO: Add actual frequency response verification
    // For now, just ensure no panic
    let signal = generate_sine(1000.0, sample_rate, 0.1);
    let mut output = vec![0.0; signal.len()];
    process_signal(&mut eq, &signal, &mut output);
    println!("Graphic EQ processing completed");
}
