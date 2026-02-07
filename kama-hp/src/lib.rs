//! High-precision audio processing ecosystem
//! 
//! Для приложений, требующих максимальной точности:
//! - Профессиональные синтезаторы
//! - Мастер-процессоры
//! - Научные исследования
//! - Машинное обучение для audio

pub mod buffers;
pub mod oscillators;
pub mod filters;
pub mod effects;
pub mod analysis;
pub mod converters;
pub mod simd;

// Re-export основных типов
pub use buffers::HighPrecisionBuffer;
pub use oscillators::{HighPrecisionSineOsc, HighPrecisionFMOsc};
pub use filters::{HighPrecisionBiquad, HighPrecisionLadderFilter};
pub use converters::{Oversampler, PrecisionConverter};

// --- Конфигурация системы high-precision ---

pub struct HighPrecisionEngine {
    sample_rate: f64,
    buffer_size: usize,
    precision_mode: PrecisionMode,
    dither_enabled: bool,
    oversampling_factor: usize,
}

pub enum PrecisionMode {
    Native,      // f64 везде
    Hybrid,      // f64 обработка → f32 I/O
    Adaptive,    // Автоматический выбор
}

impl HighPrecisionEngine {
    pub fn new(sample_rate: f64, buffer_size: usize) -> Self {
        Self {
            sample_rate,
            buffer_size,
            precision_mode: PrecisionMode::Hybrid,
            dither_enabled: true,
            oversampling_factor: 1,
        }
    }
    
    pub fn with_oversampling(mut self, factor: usize) -> Self {
        self.oversampling_factor = factor;
        self
    }
    
    pub fn create_buffer(&self, channels: usize) -> HighPrecisionBuffer {
        HighPrecisionBuffer::new(
            self.buffer_size * self.oversampling_factor,
            channels,
            self.sample_rate * self.oversampling_factor as f64,
        )
    }
}

// --- Бенчмарки производительности ---

#[cfg(feature = "benchmarks")]
pub mod benchmarks {
    use criterion::{Criterion, criterion_group, criterion_main};
    use super::*;
    
    pub fn bench_f32_vs_f64_sine(c: &mut Criterion) {
        let mut group = c.benchmark_group("sine_oscillator");
        
        // f32 версия
        group.bench_function("f32", |b| {
            let mut osc = kama_core::dsp::SineOscillator::new(440.0, 44100.0, 0.5);
            let mut buffer = vec![0.0f32; 1024];
            
            b.iter(|| {
                osc.generate(&mut buffer);
                criterion::black_box(&buffer);
            });
        });
        
        // f64 версия
        group.bench_function("f64", |b| {
            let mut osc = oscillators::HighPrecisionSineOsc::new(440.0, 44100.0, 0.5);
            let mut buffer = vec![0.0f64; 1024];
            
            b.iter(|| {
                osc.generate(&mut buffer);
                criterion::black_box(&buffer);
            });
        });
        
        group.finish();
    }
    
    pub fn bench_filter_precision(c: &mut Criterion) {
        let mut group = c.benchmark_group("biquad_filter");
        
        let input: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut output_f32 = vec![0.0f32; 1024];
        let mut output_f64 = vec![0.0f64; 1024];
        
        // f32 фильтр
        group.bench_function("f32_biquad", |b| {
            let mut filter = kama_core::dsp::BiquadFilter::new_lowpass(1000.0, 0.707, 44100.0);
            
            b.iter(|| {
                filter.process_buffer(&input, &mut output_f32);
                criterion::black_box(&output_f32);
            });
        });
        
        // f64 фильтр
        group.bench_function("f64_biquad", |b| {
            let input_f64: Vec<f64> = input.iter().map(|&x| x as f64).collect();
            let mut filter = filters::HighPrecisionBiquad::new_lowpass(1000.0, 0.707, 44100.0);
            
            b.iter(|| {
                filter.process_buffer(&input_f64, &mut output_f64);
                criterion::black_box(&output_f64);
            });
        });
        
        group.finish();
    }
    
    criterion_group!(
        benches,
        bench_f32_vs_f64_sine,
        bench_filter_precision,
    );
    criterion_main!(benches);
}