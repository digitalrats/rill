use float_cmp::approx_eq;
use kama_core_traits::{AudioNode, ParamValue};
use kama_digital_effects::{Delay, Distortion, DistortionType, Limiter};

///--------------------------------------------------------------------------------------------------------------------
///  Delay tests
/// -------------------------------------------------------------------------------------------------------------------

#[test]
fn test_delay_basic() {
    let mut delay = Delay::new(0.1, 0.5, 0.5);
    delay.init(44100.0);

    let input = vec![1.0; 100];
    let mut output = vec![0.0; 100];

    let inputs = [input.as_slice()];
    let mut outputs = [output.as_mut_slice()];

    delay.process(&inputs, &mut outputs).unwrap();

    // First sample: dry only (no delayed signal yet)
    assert!(approx_eq!(f32, output[0], 0.5, epsilon = 0.001));

    // After delay time, should see wet signal
    let delay_samples = (0.1 * 44100.0) as usize;
    if delay_samples < 100 {
        assert!(output[delay_samples] > 0.5);
    }
}

#[test]
fn test_delay_parameters() {
    let mut delay = Delay::new(0.2, 0.3, 0.7);

    assert_eq!(delay.get_param("delay_time"), Some(ParamValue::Float(0.2)));
    assert_eq!(delay.get_param("feedback"), Some(ParamValue::Float(0.3)));
    assert_eq!(delay.get_param("mix"), Some(ParamValue::Float(0.7)));

    delay
        .set_param("delay_time", ParamValue::Float(0.5))
        .unwrap();
    delay.set_param("feedback", ParamValue::Float(0.8)).unwrap();
    delay.set_param("mix", ParamValue::Float(0.4)).unwrap();

    assert_eq!(delay.get_param("delay_time"), Some(ParamValue::Float(0.5)));
    assert_eq!(delay.get_param("feedback"), Some(ParamValue::Float(0.8)));
    assert_eq!(delay.get_param("mix"), Some(ParamValue::Float(0.4)));
}

///--------------------------------------------------------------------------------------------------------------------
///  Distortion tests
/// -------------------------------------------------------------------------------------------------------------------

#[test]
fn test_distortion_hard_clip() {
    let dist = Distortion::new(DistortionType::HardClip, 10.0, 1.0);

    assert_eq!(dist.process_sample(0.1), 1.0); // driven to 1.0, clipped to 1.0
    assert_eq!(dist.process_sample(-0.05), -0.5); // driven to -0.5, no clip
}

#[test]
fn test_distortion_soft_clip() {
    let dist = Distortion::new(DistortionType::SoftClip, 5.0, 1.0);

    let out = dist.process_sample(1.0);
    assert!(out < 1.0 && out > 0.9); // tanh(5) ~ 0.9999
}

#[test]
fn test_distortion_parameters() {
    let mut dist = Distortion::new(DistortionType::SoftClip, 2.0, 0.8);

    assert_eq!(dist.get_param("drive"), Some(ParamValue::Float(2.0)));
    assert_eq!(dist.get_param("output_gain"), Some(ParamValue::Float(0.8)));

    dist.set_param("drive", ParamValue::Float(5.0)).unwrap();
    dist.set_param("output_gain", ParamValue::Float(1.2))
        .unwrap();
    dist.set_param("type", ParamValue::Choice("hard_clip".to_string()))
        .unwrap();

    assert_eq!(dist.get_param("drive"), Some(ParamValue::Float(5.0)));
    assert_eq!(dist.get_param("output_gain"), Some(ParamValue::Float(1.2)));
}

#[test]
fn test_distortion_types() {
    let test_inputs = vec![0.1, 0.5, 1.0, -0.3, -0.8];

    for &dist_type in &[
        DistortionType::HardClip,
        DistortionType::SoftClip,
        DistortionType::Tube,
        DistortionType::Fuzz,
    ] {
        let dist = Distortion::new(dist_type, 2.0, 1.0);

        for &input in &test_inputs {
            let output = dist.process_sample(input);
            assert!(!output.is_nan(), "Output should not be NaN");
            assert!(
                output >= -2.0 && output <= 2.0,
                "Output out of range: {}",
                output
            );
        }
    }
}

///--------------------------------------------------------------------------------------------------------------------
///  Limiter tests
/// -------------------------------------------------------------------------------------------------------------------
#[test]
fn test_limiter_basic() {
    println!("\n=== Test: Limiter Basic ===");

    let mut limiter = Limiter::new(-6.0, 0.005, 0.1, 1.0);
    limiter.init(44100.0);

    let lookahead_samples = limiter.lookahead_samples();
    println!("Lookahead samples: {}, initializing...", lookahead_samples);

    // Заполняем буфер и ждем инициализации
    for i in 0..lookahead_samples {
        let out = limiter.process_sample(0.1);
        if i < 10 {
            println!("Init sample {}: output = {:.3}", i, out);
        }
    }

    // Теперь проверяем работу с низким сигналом
    let test_input = 0.2;
    let test_output = limiter.process_sample(test_input);
    println!(
        "Low signal - input: {}, output: {}",
        test_input, test_output
    );

    // Должен пропускать без изменений (gain ~1.0)
    assert!(
        (test_output - test_input).abs() < 0.1,
        "Low signal should pass through, got {}",
        test_output
    );

    // Проверяем ограничение высокого сигнала
    let high_input = 1.5;
    let mut outputs = Vec::new();

    println!("\nProcessing high-level signal ({}):", high_input);
    for i in 0..2000 {
        let out = limiter.process_sample(high_input);
        outputs.push(out);
        if i % 200 == 0 {
            println!(
                "Sample {}: output = {:.3}, current_gain = {:.3}",
                i,
                out,
                limiter.current_gain()
            );
        }
    }

    // Последние сэмплы должны быть стабильными и ограниченными
    let last_few = &outputs[1800..];
    let avg = last_few.iter().sum::<f32>() / last_few.len() as f32;
    let max_val = last_few.iter().fold(0.0f32, |a, &b| a.max(b));
    let min_val = last_few.iter().fold(0.0f32, |a, &b| a.min(b));

    println!("\nLast 200 samples statistics:");
    println!("  Average: {:.3}", avg);
    println!("  Max: {:.3}", max_val);
    println!("  Min: {:.3}", min_val);

    // Проверки
    let threshold_linear = 10.0_f32.powf(-6.0 / 20.0); // -6dB ≈ 0.5

    assert!(
        avg > 0.0,
        "Average output should be positive, got {:.3}",
        avg
    );
    assert!(max_val < high_input, "Max output should be less than input");
    assert!(max_val > 0.0, "Max output should be positive");
    assert!(
        max_val < threshold_linear * 1.2,
        "Max output should be near threshold, got {:.3}",
        max_val
    );

    println!("\n✅ Limiter basic test passed");
}

#[test]
fn test_limiter_envelope() {
    println!("\n=== Test: Limiter Envelope ===");

    let mut limiter = Limiter::new(-6.0, 0.01, 0.1, 1.0);
    limiter.set_lookahead(0.01);
    limiter.init(44100.0);

    let lookahead_samples = limiter.lookahead_samples();
    println!("Lookahead samples: {}, initializing...", lookahead_samples);

    // Инициализация
    for _ in 0..lookahead_samples {
        let _ = limiter.process_sample(0.1);
    }

    // Генерируем сигнал с пиком
    let total_samples = 4000;
    let peak_start = 1000;
    let peak_end = 1100;
    let mut outputs = Vec::with_capacity(total_samples);

    println!(
        "\nGenerating signal with peak at samples {}-{}",
        peak_start, peak_end
    );

    for i in 0..total_samples {
        let input = if i >= peak_start && i < peak_end {
            2.0
        } else {
            0.1
        };
        let output = limiter.process_sample(input);
        outputs.push(output);
    }

    // Находим максимум выходного сигнала
    let max_output = outputs.iter().fold(0.0f32, |a, &b| a.max(b));
    let max_idx = outputs
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
        .map(|(i, _)| i)
        .unwrap_or(0);

    println!("\nPeak output: {:.3} at sample {}", max_output, max_idx);

    // Проверяем, что пик ограничен согласно threshold
    let threshold_linear = 10.0_f32.powf(-6.0 / 20.0); // -6dB ≈ 0.5
    let expected_max = 2.0 * threshold_linear; // ≈ 1.0

    println!("Threshold linear: {:.3}", threshold_linear);
    println!("Expected max output: {:.3}", expected_max);

    // Пик должен быть примерно равен expected_max (с учетом smoothing)
    assert!(
        max_output > 0.8 && max_output < 1.2,
        "Peak should be around {:.3}, got {:.3}",
        expected_max,
        max_output
    );

    // Проверяем, что после пика сигнал восстанавливается
    let after_peak = peak_end + 500; // Даем время на восстановление
    if after_peak < outputs.len() {
        let recovery = outputs[after_peak];
        println!("Recovery at sample {}: {:.3}", after_peak, recovery);
        assert!(
            (recovery - 0.1).abs() < 0.15,
            "After peak, output should return near 0.1, got {:.3}",
            recovery
        );
    }

    println!("\n✅ Limiter envelope test passed");
}

#[test]
fn test_limiter_parameters() {
    println!("\n=== Test: Limiter Parameters ===");

    let mut limiter = Limiter::new(-3.0, 0.01, 0.2, 1.5);

    assert_eq!(
        limiter.get_param("threshold"),
        Some(ParamValue::Float(-3.0))
    );
    assert_eq!(limiter.get_param("attack"), Some(ParamValue::Float(0.01)));
    assert_eq!(limiter.get_param("release"), Some(ParamValue::Float(0.2)));
    assert_eq!(
        limiter.get_param("output_gain"),
        Some(ParamValue::Float(1.5))
    );

    limiter
        .set_param("threshold", ParamValue::Float(-10.0))
        .unwrap();
    limiter
        .set_param("attack", ParamValue::Float(0.02))
        .unwrap();
    limiter
        .set_param("release", ParamValue::Float(0.3))
        .unwrap();
    limiter
        .set_param("output_gain", ParamValue::Float(0.8))
        .unwrap();

    assert_eq!(
        limiter.get_param("threshold"),
        Some(ParamValue::Float(-10.0))
    );
    assert_eq!(limiter.get_param("attack"), Some(ParamValue::Float(0.02)));
    assert_eq!(limiter.get_param("release"), Some(ParamValue::Float(0.3)));
    assert_eq!(
        limiter.get_param("output_gain"),
        Some(ParamValue::Float(0.8))
    );

    println!("✅ Limiter parameter test passed");
}

#[test]
fn test_limiter_reset() {
    println!("\n=== Test: Limiter Reset ===");

    let mut limiter = Limiter::new(-6.0, 0.01, 0.1, 1.0);
    limiter.init(44100.0);

    // Инициализация
    let lookahead_samples = limiter.lookahead_samples();
    for _ in 0..lookahead_samples * 2 {
        limiter.process_sample(0.1);
    }

    // Обрабатываем высокий сигнал (должен изменить gain)
    println!("Processing high signal (first pass)...");
    for i in 0..200 {
        // Увеличили до 200 семплов
        let out = limiter.process_sample(1.5);
        if i == 0 || i == 100 || i == 199 {
            println!(
                "  Step {}: gain={:.3}, out={:.3}",
                i,
                limiter.current_gain(),
                out
            );
        }
    }

    let gain_before = limiter.current_gain();
    println!("Gain before reset: {:.3}", gain_before);

    // Проверяем, что gain уменьшился
    assert!(
        gain_before < 0.8,
        "Gain should be reduced (<0.8), got {:.3}",
        gain_before
    );

    // Сброс
    println!("Resetting...");
    limiter.reset();

    // После reset gain должен быть 1.0
    assert_eq!(limiter.current_gain(), 1.0, "Gain should reset to 1.0");

    // Снова прогреваем
    println!("Warming up after reset...");
    for i in 0..lookahead_samples * 2 {
        let out = limiter.process_sample(0.1);
        if i == 0 {
            println!("  First sample after reset: out={:.3}", out);
        }
    }

    // Проверяем, что лимитер снова работает
    println!("Testing limiting after reset (200 samples)...");
    let mut max_out = 0.0f32;
    for i in 0..200 {
        // Увеличили до 200 семплов
        let out = limiter.process_sample(1.5);
        max_out = max_out.max(out);
        if i == 0 || i == 100 || i == 199 {
            println!(
                "  Step {}: gain={:.3}, out={:.3}",
                i,
                limiter.current_gain(),
                out
            );
        }
    }

    let gain_after = limiter.current_gain();
    println!("Gain after reset and processing: {:.3}", gain_after);

    // Проверяем, что gain снова уменьшился (теперь <0.8)
    assert!(
        gain_after < 0.8,
        "Gain should be reduced again (<0.8), got {:.3}",
        gain_after
    );

    // Проверяем, что выход ограничен
    assert!(
        max_out < 1.0,
        "Output should be limited (<1.0), got {:.3}",
        max_out
    );

    println!("✅ Limiter reset test passed");
}
