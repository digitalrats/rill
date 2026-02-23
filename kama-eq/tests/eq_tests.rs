//! Integration tests for kama-eq
//!
//! Tests the parametric and graphic equalizers with real filters
//! and automation integration.

use float_cmp::approx_eq;
use kama_automation::automaton::{FunctionAutomaton, LfoAutomaton};
use kama_automation::{AutomationManager, ParameterMapping, Servo, TestSignalSender};
use kama_core_traits::time::{SystemClock, TimeProvider};
use kama_core_traits::Clock; // для advance
use kama_core_traits::{AudioNode, ParamValue};
use kama_digital_filters::{BiquadFactory, BiquadFilter, FilterType};
use kama_eq::{BandType, GraphicEq, ParametricEq};
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

// =============================================================================
// BASIC EQ TESTS
// =============================================================================

#[test]
fn test_parametric_eq_creation() {
    println!("\n=== Test: Parametric EQ Creation ===");

    let factory = BiquadFactory;
    let eq = ParametricEq::new(factory, 5, 44100.0);

    assert_eq!(eq.num_inputs(), 1);
    assert_eq!(eq.num_outputs(), 1);

    let metadata = eq.metadata();
    println!("EQ metadata: {}", metadata.name);
    assert!(metadata.parameters.len() > 5); // Should have many parameters
}

#[test]
fn test_graphic_eq_creation() {
    println!("\n=== Test: Graphic EQ Creation ===");

    let factory = BiquadFactory;
    let eq = GraphicEq::new_third_octave(factory, 44100.0);

    assert_eq!(eq.num_inputs(), 1);
    assert_eq!(eq.num_outputs(), 1);
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

    // Configure bands
    eq.set_band_type(0, BandType::Peak).unwrap();
    eq.set_band(0, 100.0, 1.0, 0.0).unwrap();

    eq.set_band_type(1, BandType::Peak).unwrap();
    eq.set_band(1, 1000.0, 5.0, 6.0).unwrap(); // +6dB boost

    eq.set_band_type(2, BandType::Peak).unwrap();
    eq.set_band(2, 5000.0, 1.0, 0.0).unwrap();

    eq.init(sample_rate);

    let test_freqs = [100.0, 500.0, 1000.0, 2000.0, 5000.0];
    let mut results = Vec::new();

    for &freq in &test_freqs {
        // Generate test signal at this frequency
        let signal = generate_sine(freq, sample_rate, 0.2);
        let mut output = vec![0.0; signal.len()];

        let inputs = [signal.as_slice()];
        let mut outputs = [output.as_mut_slice()];

        eq.process(&inputs, &mut outputs).unwrap();

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

    // Configure bands
    eq.set_band_type(0, BandType::Peak).unwrap();
    eq.set_band(0, 100.0, 1.0, 0.0).unwrap();

    eq.set_band_type(1, BandType::Peak).unwrap();
    eq.set_band(1, 1000.0, 2.0, -6.0).unwrap(); // -6dB cut at 1kHz

    eq.set_band_type(2, BandType::Peak).unwrap();
    eq.set_band(2, 5000.0, 1.0, 0.0).unwrap();

    eq.init(sample_rate);

    let test_freqs = [100.0, 500.0, 1000.0, 2000.0, 5000.0];
    let mut results = Vec::new();

    for &freq in &test_freqs {
        let signal = generate_sine(freq, sample_rate, 0.2);
        let mut output = vec![0.0; signal.len()];

        let inputs = [signal.as_slice()];
        let mut outputs = [output.as_mut_slice()];

        eq.process(&inputs, &mut outputs).unwrap();

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
            if (freq - 250.0).abs() < 10.0 {
                band_250 = Some(i);
                println!("Found 250 Hz at band {}", i);
            }
            if (freq - 2500.0).abs() < 100.0 {
                band_2500 = Some(i);
                println!("Found 2500 Hz at band {}", i);
            }
            if (freq - 5000.0).abs() < 200.0 {
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

    // ... rest of the test
}

// =============================================================================
// PARAMETER TESTS
// =============================================================================

#[test]
fn test_parametric_eq_parameters() {
    println!("\n=== Test: Parametric EQ Parameters ===");

    let factory = BiquadFactory;
    let mut eq = ParametricEq::new(factory, 3, 44100.0);

    // Test getting parameters
    assert!(eq.get_param("num_bands").is_some());
    assert!(eq.get_param("output_gain").is_some());
    assert!(eq.get_param("band_0_freq").is_some());
    assert!(eq.get_param("band_0_gain").is_some());
    assert!(eq.get_param("band_0_q").is_some());
    assert!(eq.get_param("band_0_enabled").is_some());

    // Test setting parameters
    eq.set_param("output_gain", ParamValue::Float(0.8)).unwrap();
    match eq.get_param("output_gain") {
        Some(ParamValue::Float(v)) => assert!((v - 0.8).abs() < 0.01),
        _ => panic!("output_gain should be float"),
    }

    eq.set_param("band_0_freq", ParamValue::Float(200.0))
        .unwrap();
    match eq.get_param("band_0_freq") {
        Some(ParamValue::Float(v)) => assert!((v - 200.0).abs() < 0.1),
        _ => panic!("band_0_freq should be float"),
    }

    eq.set_param("band_0_gain", ParamValue::Float(3.0)).unwrap();
    match eq.get_param("band_0_gain") {
        Some(ParamValue::Float(v)) => assert!((v - 3.0).abs() < 0.1),
        _ => panic!("band_0_gain should be float"),
    }

    eq.set_param("band_0_q", ParamValue::Float(2.5)).unwrap();
    match eq.get_param("band_0_q") {
        Some(ParamValue::Float(v)) => assert!((v - 2.5).abs() < 0.1),
        _ => panic!("band_0_q should be float"),
    }

    eq.set_param("band_0_enabled", ParamValue::Bool(false))
        .unwrap();
    match eq.get_param("band_0_enabled") {
        Some(ParamValue::Bool(v)) => assert!(!v),
        _ => panic!("band_0_enabled should be bool"),
    }

    println!("All parameter tests passed");
}

#[test]
fn test_graphic_eq_parameters() {
    println!("\n=== Test: Graphic EQ Parameters ===");

    let factory = BiquadFactory;
    let mut eq = GraphicEq::new_third_octave(factory, 44100.0);

    // Test getting parameters
    assert!(eq.get_param("num_bands").is_some());
    assert!(eq.get_param("output_gain").is_some());
    assert!(eq.get_param("band_10").is_some()); // 10th band gain

    // Test setting parameters
    eq.set_param("output_gain", ParamValue::Float(0.7)).unwrap();
    match eq.get_param("output_gain") {
        Some(ParamValue::Float(v)) => assert!((v - 0.7).abs() < 0.01),
        _ => panic!("output_gain should be float"),
    }

    eq.set_param("band_10", ParamValue::Float(4.0)).unwrap();
    match eq.get_param("band_10") {
        Some(ParamValue::Float(v)) => assert!((v - 4.0).abs() < 0.1),
        _ => panic!("band_10 should be float"),
    }

    println!("All graphic EQ parameter tests passed");
}

// =============================================================================
// AUTOMATION INTEGRATION TESTS
// =============================================================================

#[test]
fn test_eq_automation_integration() {
    println!("\n=== Test: EQ Automation Integration ===");

    // Setup time provider and automation manager
    let time_provider = Arc::new(SystemClock::new(44100.0, 120.0));
    let system_clock = SystemClock::new(44100.0, 120.0);
    let signal_sender = Arc::new(TestSignalSender::new());

    let mut manager = AutomationManager::new(time_provider.clone(), system_clock)
        .with_signal_sender(signal_sender.clone());

    // Create EQ
    let factory = BiquadFactory;
    let mut eq = ParametricEq::new(factory, 3, 44100.0);
    eq.init(44100.0);

    // Automate band 0 frequency with custom function
    let base_freq = 500.0;
    let freq_range = 200.0;

    let freq_automaton = FunctionAutomaton::new(
        "Freq LFO",
        move |time| {
            let lfo_val = (time * 0.5).sin(); // от -1 до 1
            base_freq + lfo_val * freq_range
        },
        "eq",
        "band_0_freq",
    );

    let context = kama_automation::AutomationContext::new(time_provider.clone());
    let mut servo = Servo::new(
        "freq_lfo".to_string(),
        Arc::new(freq_automaton),
        "eq".to_string(),
        "band_0_freq".to_string(),
        ParameterMapping::Linear,
        context,
    );

    // Убираем ограничения, чтобы значения могли быть > 1.0
    servo.set_range(f64::NEG_INFINITY, f64::INFINITY);
    manager.add_servo(servo);

    // Automate band 0 gain with custom function
    let gain_automaton = FunctionAutomaton::new(
        "Gain LFO",
        move |time| (time * 0.3).sin() * 3.0, // ±3dB variation
        "eq",
        "band_0_gain",
    );

    let context = kama_automation::AutomationContext::new(time_provider.clone());
    let mut servo = Servo::new(
        "gain_lfo".to_string(),
        Arc::new(gain_automaton),
        "eq".to_string(),
        "band_0_gain".to_string(),
        ParameterMapping::Linear,
        context,
    );

    servo.set_range(f64::NEG_INFINITY, f64::INFINITY);
    manager.add_servo(servo);

    // Automate output gain with custom function
    let envelope_automaton = FunctionAutomaton::new(
        "Output Envelope",
        move |time| {
            let attack = 0.5;
            let release = 0.5;
            if time < attack {
                time / attack * 0.8 + 0.2
            } else if time > 1.0 - release {
                (1.0 - time) / release * 0.8 + 0.2
            } else {
                1.0
            }
        },
        "eq",
        "output_gain",
    );

    let context = kama_automation::AutomationContext::new(time_provider.clone());
    let mut servo = Servo::new(
        "output_envelope".to_string(),
        Arc::new(envelope_automaton),
        "eq".to_string(),
        "output_gain".to_string(),
        ParameterMapping::Linear,
        context,
    );

    servo.set_range(f64::NEG_INFINITY, f64::INFINITY);
    manager.add_servo(servo);

    // Verify servos were added
    assert_eq!(manager.servos().len(), 3);

    // Run automation for a few steps and verify signals are sent
    let test_signal = generate_sweep(44100.0, 0.5);
    let mut output = vec![0.0; test_signal.len()];

    for i in 0..10 {
        time_provider.advance(4410); // 0.1 seconds
        manager.update(4410);

        // Process audio through EQ
        let inputs = [test_signal.as_slice()];
        let mut outputs = [output.as_mut_slice()];
        eq.process(&inputs, &mut outputs).unwrap();
    }

    // Check that signals were sent
    let freq_signals = signal_sender.get_signals_for_param("eq", "band_0_freq");
    let gain_signals = signal_sender.get_signals_for_param("eq", "band_0_gain");
    let output_signals = signal_sender.get_signals_for_param("eq", "output_gain");

    println!("Frequency signals: {:?}", freq_signals);
    println!("Gain signals: {:?}", gain_signals);
    println!("Output gain signals: {:?}", output_signals);

    assert!(!freq_signals.is_empty(), "No frequency automation signals");
    assert!(!gain_signals.is_empty(), "No gain automation signals");
    assert!(
        !output_signals.is_empty(),
        "No output gain automation signals"
    );

    // Verify that values are within expected ranges
    for &value in &freq_signals {
        assert!(
            value >= 300.0 && value <= 700.0,
            "Frequency out of range: {}",
            value
        );
    }

    for &value in &gain_signals {
        assert!(
            value >= -3.0 && value <= 3.0,
            "Gain out of range: {}",
            value
        );
    }

    println!("✅ Automation integration test passed");
}

// =============================================================================
// BOUNDARY TESTS
// =============================================================================

#[test]
fn test_eq_boundary_conditions() {
    println!("\n=== Test: EQ Boundary Conditions ===");

    let factory = BiquadFactory;
    let mut eq = ParametricEq::new(factory, 3, 44100.0);
    eq.init(44100.0);

    // Test with empty buffers
    let empty_input: &[&[f32]] = &[];
    let mut empty_output: &mut [&mut [f32]] = &mut [];
    assert!(eq.process(empty_input, &mut empty_output).is_ok());

    // Test with extreme frequency values
    eq.set_param("band_0_freq", ParamValue::Float(10.0))
        .unwrap(); // below min
    match eq.get_param("band_0_freq") {
        Some(ParamValue::Float(v)) => assert!((v - 20.0).abs() < 0.1), // should clamp to 20
        _ => panic!("band_0_freq should be float"),
    }

    eq.set_param("band_0_freq", ParamValue::Float(30000.0))
        .unwrap(); // above max
    match eq.get_param("band_0_freq") {
        Some(ParamValue::Float(v)) => assert!((v - 20000.0).abs() < 0.1), // should clamp to 20000
        _ => panic!("band_0_freq should be float"),
    }

    // Test with extreme gain values
    eq.set_param("band_0_gain", ParamValue::Float(30.0))
        .unwrap(); // above max
    match eq.get_param("band_0_gain") {
        Some(ParamValue::Float(v)) => assert!((v - 24.0).abs() < 0.1), // should clamp to 24
        _ => panic!("band_0_gain should be float"),
    }

    eq.set_param("band_0_gain", ParamValue::Float(-30.0))
        .unwrap(); // below min
    match eq.get_param("band_0_gain") {
        Some(ParamValue::Float(v)) => assert!((v + 24.0).abs() < 0.1), // should clamp to -24
        _ => panic!("band_0_gain should be float"),
    }

    // Test with extreme Q values
    eq.set_param("band_0_q", ParamValue::Float(0.01)).unwrap(); // below min
    match eq.get_param("band_0_q") {
        Some(ParamValue::Float(v)) => assert!((v - 0.1).abs() < 0.01), // should clamp to 0.1
        _ => panic!("band_0_q should be float"),
    }

    eq.set_param("band_0_q", ParamValue::Float(50.0)).unwrap(); // above max
    match eq.get_param("band_0_q") {
        Some(ParamValue::Float(v)) => assert!((v - 20.0).abs() < 0.1), // should clamp to 20
        _ => panic!("band_0_q should be float"),
    }

    println!("✅ Boundary tests passed");
}

#[test]
fn test_eq_reset() {
    println!("\n=== Test: EQ Reset ===");

    let factory = BiquadFactory;
    let mut eq = ParametricEq::new(factory, 3, 44100.0);
    eq.init(44100.0);

    // Set some parameters
    eq.set_param("band_0_freq", ParamValue::Float(200.0))
        .unwrap();
    eq.set_param("band_0_gain", ParamValue::Float(3.0)).unwrap();
    eq.set_param("output_gain", ParamValue::Float(0.5)).unwrap();

    // Process some audio
    let signal = generate_sine(440.0, 44100.0, 0.1);
    let mut output = vec![0.0; signal.len()];
    let inputs = [signal.as_slice()];
    let mut outputs = [output.as_mut_slice()];
    eq.process(&inputs, &mut outputs).unwrap();

    // Reset EQ
    eq.reset();

    // Parameters should remain the same (reset only resets filter states, not parameters)
    match eq.get_param("band_0_freq") {
        Some(ParamValue::Float(v)) => assert!((v - 200.0).abs() < 0.1),
        _ => panic!("band_0_freq changed after reset"),
    }

    match eq.get_param("band_0_gain") {
        Some(ParamValue::Float(v)) => assert!((v - 3.0).abs() < 0.1),
        _ => panic!("band_0_gain changed after reset"),
    }

    // Process audio again - should work without errors
    let inputs = [signal.as_slice()];
    let mut outputs = [output.as_mut_slice()];
    assert!(eq.process(&inputs, &mut outputs).is_ok());

    println!("✅ Reset test passed");
}
