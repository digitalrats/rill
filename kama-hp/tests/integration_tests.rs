// kama-hp/tests/integration_tests.rs
//! Интеграционные тесты для kama-hp с kama-buffers

use kama_hp::*;
use kama_buffers::BufferManager;  // <-- Убираем BufferManagerStats

// -----------------------------------------------------------------------------
// Тест 1: Конвертация PooledBuffer -> HighPrecisionBuffer
// -----------------------------------------------------------------------------
#[test]
fn test_pooled_buffer_to_hp_conversion() {
    println!("\n=== Test: PooledBuffer to HighPrecisionBuffer conversion ===");
    
    // Создаём BufferManager
    let manager = BufferManager::new();
    
    // Получаем PooledBuffer из пула
    let pooled = manager.acquire(256).unwrap();
    
    // Конвертируем в HighPrecisionBuffer
    let hp_buffer = HighPrecisionBuffer::from_pooled_buffer(
        &pooled,
        2, // стерео
        44100.0
    );
    
    println!("✅ Converted to HighPrecisionBuffer: {} channels, {} samples, {} Hz",
             hp_buffer.channels(), hp_buffer.size(), hp_buffer.sample_rate());
    
    // Проверяем, что размер правильный
    assert_eq!(hp_buffer.size(), 128); // 256 samples / 2 channels = 128 frames
    assert_eq!(hp_buffer.channels(), 2);
    assert_eq!(hp_buffer.sample_rate(), 44100.0);
}

// -----------------------------------------------------------------------------
// Тест 2: HighPrecisionBuffer -> PooledBuffer (для вывода)
// -----------------------------------------------------------------------------
#[test]
fn test_hp_to_pooled_buffer_conversion() {
    println!("\n=== Test: HighPrecisionBuffer to PooledBuffer conversion ===");
    
    // Создаём HighPrecisionBuffer с тестовыми данными
    let mut hp_buffer = HighPrecisionBuffer::new(512, 2, 48000.0);
    
    // Заполняем синусоидой
    for i in 0..512 {
        let t = i as f64 / 48000.0;
        let sample = (2.0 * std::f64::consts::PI * 440.0 * t).sin() * 0.5;
        hp_buffer.write(i, 0, sample); // левый канал
        hp_buffer.write(i, 1, sample); // правый канал
    }
    
    println!("✅ HighPrecisionBuffer filled with 440Hz sine wave");
    
    // Создаём BufferManager и получаем PooledBuffer
    let manager = BufferManager::new();
    let mut pooled = manager.acquire(hp_buffer.total_samples()).unwrap();
    
    // Копируем данные
    let copied = hp_buffer.copy_to_pooled_buffer(&mut pooled);
    
    println!("✅ Copied to PooledBuffer: {} samples", copied);
    
    assert_eq!(copied, hp_buffer.total_samples());
}

// -----------------------------------------------------------------------------
// Тест 3: Обработка через BufferManager с high-precision
// -----------------------------------------------------------------------------
#[test]
fn test_hp_processing_with_buffer_manager() {
    println!("\n=== Test: High-precision processing with BufferManager ===");
    
    let manager = BufferManager::new();
    
    // Создаём высокоточный фильтр
    let mut filter = HighPrecisionBiquad::new_lowpass(1000.0, 0.707, 48000.0);
    
    // Получаем буферы из менеджера
    let mut input_pooled = manager.acquire(512).unwrap();
    let mut output_pooled = manager.acquire(512).unwrap();
    
    // Заполняем входной буфер тестовым сигналом (шум + синус)
    use rand::Rng;
    let mut rng = rand::thread_rng();
    
    for i in 0..512 {
        let t = i as f32 / 48000.0;
        let sine = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.3;
        let noise = (rng.gen::<f32>() - 0.5) * 0.1;
        input_pooled.as_mut_slice()[i] = sine + noise;
    }
    
    println!("✅ Input buffer prepared");
    
    // Конвертируем в HighPrecisionBuffer
    let hp_input = HighPrecisionBuffer::from_pooled_buffer(&input_pooled, 1, 48000.0);
    
    // Обрабатываем
    let mut hp_output = HighPrecisionBuffer::new(512, 1, 48000.0);
    for i in 0..512 {
        hp_output.write(i, 0, filter.process(hp_input.read(i, 0)));
    }
    
    println!("✅ High-precision filter processed");
    
    // Конвертируем обратно в PooledBuffer
    hp_output.copy_to_pooled_buffer(&mut output_pooled);
    
    // Проверяем статистику
    let input_rms: f32 = (input_pooled.as_slice().iter().map(|&x| x * x).sum::<f32>() / 512.0).sqrt();
    let output_rms: f32 = (output_pooled.as_slice().iter().map(|&x| x * x).sum::<f32>() / 512.0).sqrt();
    
    println!("  Input RMS: {:.6}", input_rms);
    println!("  Output RMS: {:.6}", output_rms);
    
    // Фильтр нижних частот должен уменьшить RMS (отсечь высокие частоты)
    assert!(output_rms < input_rms, "Output RMS should be less than input RMS");
}

// -----------------------------------------------------------------------------
// Тест 4: Многоканальная обработка с high-precision
// -----------------------------------------------------------------------------
#[test]
fn test_multichannel_hp_processing() {
    println!("\n=== Test: Multichannel high-precision processing ===");
    
    let manager = BufferManager::new();
    let channels = 4;
    let frames = 256;
    let total_samples = frames * channels;
    
    // Получаем interleaved буфер из менеджера
    let mut interleaved = manager.acquire(total_samples).unwrap();
    
    // Заполняем тестовыми данными
    for ch in 0..channels {
        let freq = 440.0 * (ch + 1) as f32;
        for frame in 0..frames {
            let t = frame as f32 / 48000.0;
            let idx = frame * channels + ch;
            let val = (2.0 * std::f32::consts::PI * freq * t).sin() * 0.5;
            interleaved.as_mut_slice()[idx] = val;
        }
    }
    
    println!("✅ Multichannel buffer created ({} channels, {} frames)", channels, frames);
    println!("   Channel frequencies: 440Hz, 880Hz, 1320Hz, 1760Hz");
    
    // Конвертируем в HighPrecisionBuffer
    let hp_buffer = HighPrecisionBuffer::from_pooled_buffer(
        &interleaved,
        channels,
        48000.0
    );
    
    println!("✅ Converted to HighPrecisionBuffer");
    
    // Проверяем первые несколько фреймов каждого канала (для отладки)
    for ch in 0..channels {
        println!("\nChannel {} first 20 samples:", ch);
        for frame in 0..20 {
            let val = hp_buffer.read(frame, ch);
            println!("  frame {:2}: {:.6}", frame, val);
        }
    }
    
    // Константа для допуска при сравнении чисел с плавающей точкой
    let tolerance = 1e-3;  // 0.001
    
    // Проверяем каждый канал
    for ch in 0..channels {
        let mut has_positive = false;
        let mut has_negative = false;
        let mut max_val: f64 = f64::NEG_INFINITY;
        let mut min_val: f64 = f64::INFINITY;
        
        for frame in 0..frames {
            let val = hp_buffer.read(frame, ch);
            max_val = max_val.max(val);
            min_val = min_val.min(val);
            
            if val > tolerance {
                has_positive = true;
            }
            if val < -tolerance {
                has_negative = true;
            }
        }
        
        println!("\nChannel {} statistics:", ch);
        println!("  min: {:.6}, max: {:.6}", min_val, max_val);
        println!("  has_positive: {}, has_negative: {}", has_positive, has_negative);
        
        // Проверяем наличие положительных и отрицательных значений
        assert!(has_positive, "Channel {} should have positive values > {}", ch, tolerance);
        assert!(has_negative, "Channel {} should have negative values < -{}", ch, tolerance);
        
        // Проверяем, что амплитуда близка к 0.5 (с допуском)
        // Максимум должен быть близок к 0.5
        assert!((max_val - 0.5).abs() < 0.1, 
                "Channel {} max should be near 0.5, got {}", ch, max_val);
        
        // Минимум должен быть близок к -0.5
        assert!((min_val + 0.5).abs() < 0.1, 
                "Channel {} min should be near -0.5, got {}", ch, min_val);
        
        // Максимум должен быть положительным и близким к 0.5
        assert!(max_val > 0.4, "Channel {} max should be > 0.4, got {}", ch, max_val);
        
        // Минимум должен быть отрицательным и близким к -0.5
        assert!(min_val < -0.4, "Channel {} min should be < -0.4, got {}", ch, min_val);
    }
    
    // Конвертируем обратно и проверяем точность
    let back_to_f32 = hp_buffer.to_f32();
    assert_eq!(back_to_f32.len(), total_samples);
    
    println!("\nVerifying conversion accuracy:");
    let mut max_diff = 0.0f32;
    
    for i in 0..total_samples {
        let original = interleaved.as_slice()[i];
        let converted = back_to_f32[i];
        let diff = (original - converted).abs();
        max_diff = max_diff.max(diff);
        
        if i % 100 == 0 {  // Печатаем каждый 100-й сэмпл
            println!("  sample {}: original={:.6}, converted={:.6}, diff={:.6}", 
                     i, original, converted, diff);
        }
        
        assert!(diff < 1e-5,
                "Sample {} mismatch: original={:.10}, converted={:.10}, diff={:.10}", 
                i, original, converted, diff);
    }
    
    println!("  Maximum conversion error: {:.6}", max_diff);
    assert!(max_diff < 1e-5, "Maximum conversion error too large: {:.6}", max_diff);
}

// -----------------------------------------------------------------------------
// Тест 5: Производительность high-precision vs стандартная точность
// -----------------------------------------------------------------------------
#[test]
fn test_precision_comparison() {
    println!("\n=== Test: Precision comparison (f32 vs f64) ===");
    
    let sample_rate = 48000.0;
    let duration_secs = 0.1;  // <-- УМЕНЬШАЕМ для более быстрого теста
    let num_samples = (sample_rate * duration_secs) as usize;
    
    println!("Using {} samples for test", num_samples);
    
    // Создаём фильтры
    let mut hp_filter = HighPrecisionBiquad::new_lowpass(500.0, 0.707, sample_rate);
    
    // Генерируем тестовый сигнал с высокочастотной составляющей
    let mut input_f32 = vec![0.0; num_samples];
    let mut input_f64 = vec![0.0; num_samples];
    
    for i in 0..num_samples {
        let t = i as f64 / sample_rate;
        // Смесь низких и высоких частот
        let sample = (2.0 * std::f64::consts::PI * 100.0 * t).sin() * 0.5 +
                     (2.0 * std::f64::consts::PI * 2000.0 * t).sin() * 0.3;
        input_f64[i] = sample;
        input_f32[i] = sample as f32;
    }
    
    println!("✅ Test signal generated");
    
    // Обрабатываем в f64
    let start_f64 = std::time::Instant::now();
    let mut output_f64 = vec![0.0; num_samples];
    for i in 0..num_samples {
        output_f64[i] = hp_filter.process(input_f64[i]);
    }
    let duration_f64 = start_f64.elapsed();
    
    // Создаём новый фильтр для f32 теста
    let mut hp_filter_f32 = HighPrecisionBiquad::new_lowpass(500.0, 0.707, sample_rate);
    
    // Обрабатываем в f32 (через f64 фильтр, но с f32 входом)
    let start_f32 = std::time::Instant::now();
    let mut output_f32 = vec![0.0; num_samples];
    for i in 0..num_samples {
        output_f32[i] = hp_filter_f32.process(input_f32[i] as f64) as f32;
    }
    let duration_f32 = start_f32.elapsed();
    
    println!("  f64 processing time: {:?}", duration_f64);
    println!("  f32 processing time: {:?}", duration_f32);
    
    // Сравниваем накопленную ошибку
    let mut error_accum: f64 = 0.0;
    let mut max_error: f64 = 0.0;
    
    for i in 0..num_samples.min(1000) {  // Проверяем первые 1000 сэмплов
        let diff = (output_f64[i] - output_f32[i] as f64).abs();
        error_accum += diff;
        max_error = max_error.max(diff);
        
        if i < 10 {  // Печатаем первые 10 для отладки
            println!("  sample {}: f64={:.10}, f32={:.10}, diff={:.10}", 
                     i, output_f64[i], output_f32[i], diff);
        }
    }
    
    let avg_error = error_accum / num_samples.min(1000) as f64;
    
    println!("  Average error: {:.12}", avg_error);
    println!("  Max error: {:.12}", max_error);
    
    // Проверяем, что ошибка не слишком большая
    // Для этого теста допускаем небольшую ошибку из-за округления
    assert!(avg_error < 1e-4, "Average error too large: {:.12}", avg_error);
    assert!(max_error < 1e-3, "Max error too large: {:.12}", max_error);
}

// -----------------------------------------------------------------------------
// Тест 6: HighPrecisionBufferPool
// -----------------------------------------------------------------------------
#[test]
fn test_hp_buffer_pool() {
    println!("\n=== Test: HighPrecisionBufferPool ===");
    
    let manager = BufferManager::new();
    let mut pool = HighPrecisionBufferPool::new(manager, 2, 44100.0);
    
    // Получаем буфер из пула
    let buffer = pool.acquire(256).unwrap();
    assert_eq!(buffer.channels(), 2);
    assert_eq!(buffer.size(), 256);
    assert_eq!(buffer.sample_rate(), 44100.0);
    
    println!("✅ Buffer acquired from pool");
    
    // Возвращаем в пул
    pool.release(buffer);
    
    println!("✅ Buffer released to pool");
}