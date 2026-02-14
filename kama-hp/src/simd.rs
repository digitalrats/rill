//! SIMD ускорение для high-precision операций (f64)

#![cfg(feature = "simd")]

use core::simd::{f64x2, f64x4, f64x8, SimdFloat};

/// Конфигурация SIMD для f64
#[derive(Debug, Clone, Copy)]
pub struct F64SimdConfig {
    pub has_avx512: bool,
    pub has_avx2: bool,
    pub has_sse2: bool,
    pub optimal_width: usize,
}

impl F64SimdConfig {
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        let (has_avx512, has_avx2, has_sse2) = unsafe {
            use std::arch::x86_64::*;
            (
                is_x86_feature_detected!("avx512f"),
                is_x86_feature_detected!("avx2"),
                is_x86_feature_detected!("sse2"),
            )
        };
        
        #[cfg(not(target_arch = "x86_64"))]
        let (has_avx512, has_avx2, has_sse2) = (false, false, true);
        
        let optimal_width = if has_avx512 {
            8
        } else if has_avx2 {
            4
        } else {
            2
        };
        
        Self {
            has_avx512,
            has_avx2,
            has_sse2,
            optimal_width,
        }
    }
}

/// SIMD-ускоренная конвертация f32 → f64
pub fn simd_convert_f32_to_f64(input: &[f32], output: &mut [f64]) {
    let config = F64SimdConfig::detect();
    
    match config.optimal_width {
        8 => convert_f32_to_f64_f64x8(input, output),
        4 => convert_f32_to_f64_f64x4(input, output),
        2 => convert_f32_to_f64_f64x2(input, output),
        _ => scalar_convert_f32_to_f64(input, output),
    }
}

fn convert_f32_to_f64_f64x8(input: &[f32], output: &mut [f64]) {
    let chunks = input.chunks_exact(8);
    let remainder = chunks.remainder();
    
    for (i, chunk) in chunks.enumerate() {
        for j in 0..8 {
            output[i * 8 + j] = chunk[j] as f64;
        }
    }
    
    let start = input.len() - remainder.len();
    for i in 0..remainder.len() {
        output[start + i] = input[start + i] as f64;
    }
}

fn convert_f32_to_f64_f64x4(input: &[f32], output: &mut [f64]) {
    let chunks = input.chunks_exact(4);
    let remainder = chunks.remainder();
    
    for (i, chunk) in chunks.enumerate() {use std::f64::consts::PI;

/// Высокоточный синусоидальный осциллятор
pub struct HighPrecisionSineOsc {
    frequency: f64,
    phase: f64,
    sample_rate: f64,
    amplitude: f64,
}

impl HighPrecisionSineOsc {
    pub fn new(frequency: f64, sample_rate: f64, amplitude: f64) -> Self {
        Self {
            frequency,
            phase: 0.0,
            sample_rate,
            amplitude: amplitude.max(0.0).min(1.0),
        }
    }
    
    pub fn set_frequency(&mut self, frequency: f64) {
        self.frequency = frequency.max(0.0).min(self.sample_rate / 2.0);
    }
    
    pub fn set_amplitude(&mut self, amplitude: f64) {
        self.amplitude = amplitude.max(0.0).min(1.0);
    }
    
    pub fn generate(&mut self, output: &mut [f64]) {
        let phase_increment = 2.0 * PI * self.frequency / self.sample_rate;
        
        for out in output.iter_mut() {
            *out = self.phase.sin() * self.amplitude;
            self.phase += phase_increment;
            if self.phase > 2.0 * PI {
                self.phase -= 2.0 * PI;
            }
        }
    }
    
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }
}


pub mod oscillators {
    use super::*;
    
    /// Высокоточный синусоидальный осциллятор
    pub struct HighPrecisionSineOscillator {
        frequency: f64,
        phase: f64,
        sample_rate: f64,
        amplitude: f64,
    }
    
    impl HighPrecisionSineOscillator {
        pub fn new(frequency: f64, sample_rate: f64, amplitude: f64) -> Self {
            Self {
                frequency,
                phase: 0.0,
                sample_rate,
                amplitude,
            }
        }
        
        pub fn set_frequency(&mut self, frequency: f64) {
            self.frequency = frequency.max(0.0).min(self.sample_rate / 2.0);
        }
        
        pub fn set_amplitude(&mut self, amplitude: f64) {
            self.amplitude = amplitude.max(0.0).min(1.0);
        }
        
        pub fn generate(&mut self, output: &mut [f64]) {
            let phase_increment = 2.0 * std::f64::consts::PI * self.frequency / self.sample_rate;
            
            for out in output.iter_mut() {
                *out = self.phase.sin() * self.amplitude;
                self.phase += phase_increment;
                
                // Нормализуем фазу для сохранения точности
                if self.phase > 2.0 * std::f64::consts::PI {
                    self.phase -= 2.0 * std::f64::consts::PI;
                }
            }
        }
    }
    
    impl HighPrecisionNode for HighPrecisionSineOscillator {
        fn process_hp(&mut self, inputs: &[&[f64]], outputs: &mut [&mut [f64]]) -> AudioResult<()> {
            if outputs.is_empty() {
                return Ok(());
            }
            
            let output = &mut outputs[0];
            self.generate(output);
            
            Ok(())
        }
    }
    
    /// Высокоточный FM осциллятор
    pub struct HighPrecisionFMOscillator {
        carrier_freq: f64,
        modulator_freq: f64,
        modulation_index: f64,
        carrier_phase: f64,
        modulator_phase: f64,
        sample_rate: f64,
        amplitude: f64,
    }
    
    impl HighPrecisionFMOscillator {
        pub fn new(
            carrier_freq: f64,
            modulator_freq: f64,
            modulation_index: f64,
            sample_rate: f64,
            amplitude: f64,
        ) -> Self {
            Self {
                carrier_freq,
                modulator_freq,
                modulation_index,
                carrier_phase: 0.0,
                modulator_phase: 0.0,
                sample_rate,
                amplitude,
            }
        }
        
        pub fn generate(&mut self, output: &mut [f64]) {
            let carrier_inc = 2.0 * std::f64::consts::PI * self.carrier_freq / self.sample_rate;
            let modulator_inc = 2.0 * std::f64::consts::PI * self.modulator_freq / self.sample_rate;
            
            for out in output.iter_mut() {
                // Модулирующая волна
                let modulation = self.modulator_phase.sin() * self.modulation_index;
                
                // Несущая волна с FM
                *out = (self.carrier_phase + modulation).sin() * self.amplitude;
                
                // Обновляем фазы
                self.carrier_phase += carrier_inc;
                self.modulator_phase += modulator_inc;
                
                // Нормализуем фазы
                if self.carrier_phase > 2.0 * std::f64::consts::PI {
                    self.carrier_phase -= 2.0 * std::f64::consts::PI;
                }
                if self.modulator_phase > 2.0 * std::f64::consts::PI {
                    self.modulator_phase -= 2.0 * std::f64::consts::PI;
                }
            }
        }
    }
}
        for j in 0..4 {
            output[i * 4 + j] = chunk[j] as f64;
        }
    }
    
    let start = input.len() - remainder.len();
    for i in 0..remainder.len() {
        output[start + i] = input[start + i] as f64;
    }
}

fn convert_f32_to_f64_f64x2(input: &[f32], output: &mut [f64]) {
    let chunks = input.chunks_exact(2);
    let remainder = chunks.remainder();
    
    for (i, chunk) in chunks.enumerate() {
        output[i * 2] = chunk[0] as f64;
        output[i * 2 + 1] = chunk[1] as f64;
    }
    
    let start = input.len() - remainder.len();
    for i in 0..remainder.len() {
        output[start + i] = input[start + i] as f64;
    }
}

fn scalar_convert_f32_to_f64(input: &[f32], output: &mut [f64]) {
    for i in 0..input.len().min(output.len()) {
        output[i] = input[i] as f64;
    }
}

/// SIMD-ускоренная конвертация f64 → f32
pub fn simd_convert_f64_to_f32(input: &[f64], output: &mut [f32]) {
    let config = F64SimdConfig::detect();
    
    match config.optimal_width {
        4 => convert_f64_to_f32_f64x4(input, output),
        2 => convert_f64_to_f32_f64x2(input, output),
        _ => scalar_convert_f64_to_f32(input, output),
    }
}

fn convert_f64_to_f32_f64x4(input: &[f64], output: &mut [f32]) {
    let chunks = input.chunks_exact(4);
    let remainder = chunks.remainder();
    
    for (i, chunk) in chunks.enumerate() {
        for j in 0..4 {
            output[i * 4 + j] = chunk[j] as f32;
        }
    }
    
    let start = input.len() - remainder.len();
    for i in 0..remainder.len() {
        output[start + i] = input[start + i] as f32;
    }
}

fn convert_f64_to_f32_f64x2(input: &[f64], output: &mut [f32]) {
    let chunks = input.chunks_exact(2);
    let remainder = chunks.remainder();
    
    for (i, chunk) in chunks.enumerate() {
        output[i * 2] = chunk[0] as f32;
        output[i * 2 + 1] = chunk[1] as f32;
    }
    
    let start = input.len() - remainder.len();
    for i in 0..remainder.len() {
        output[start + i] = input[start + i] as f32;
    }
}

fn scalar_convert_f64_to_f32(input: &[f64], output: &mut [f32]) {
    for i in 0..input.len().min(output.len()) {
        output[i] = input[i] as f32;
    }
}