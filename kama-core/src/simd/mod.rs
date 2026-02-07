//! SIMD ускорение для аудиообработки
//! Использует std::simd для переносимых SIMD операций

#![cfg(feature = "simd")]

// В Rust 1.75+ std::simd стабилен
use std::simd::{f32x4, f32x8, f32x16, f64x2, f64x4, f64x8, Simd, SimdFloat, SimdInt};

/// SIMD конфигурация
#[derive(Debug, Clone, Copy)]
pub struct SimdConfig {
    pub f32_width: usize,
    pub f64_width: usize,
}

impl SimdConfig {
    /// Простое определение - используем максимальную доступную ширину
    pub fn detect() -> Self {
        // На данный момент используем консервативные значения
        // В будущем можно добавить определение CPU возможностей
        Self {
            f32_width: 4,  // Минимальная гарантированная ширина
            f64_width: 2,
        }
    }
    
    pub fn is_simd_available(&self) -> bool {
        true  // std::simd всегда доступен в Rust 1.75+
    }
}

/// Простая SIMD операция gain
pub fn simd_gain(input: &[f32], output: &mut [f32], gain: f32) {
    // Используем f32x4 как базовую ширину
    let gain_vec = f32x4::splat(gain);
    
    // Обрабатываем по 4 семпла за раз
    let chunks = input.chunks_exact(4);
    let remainder = chunks.remainder();
    
    for (i, chunk) in chunks.enumerate() {
        let input_vec = f32x4::from_slice(chunk);
        let output_vec = input_vec * gain_vec;
        output_vec.copy_to_slice(&mut output[i*4..(i+1)*4]);
    }
    
    // Обработка остатка
    let start = input.len() - remainder.len();
    for i in 0..remainder.len() {
        output[start + i] = input[start + i] * gain;
    }
}

/// SIMD saturating add (сложение с насыщением)
pub fn simd_saturating_add(a: &[f32], b: &[f32], output: &mut [f32]) {
    let min_vec = f32x4::splat(-1.0);
    let max_vec = f32x4::splat(1.0);
    
    let chunks = a.chunks_exact(4).zip(b.chunks_exact(4));
    let remainder = a.len() % 4;
    
    for ((i, a_chunk), b_chunk) in chunks.enumerate() {
        let a_vec = f32x4::from_slice(a_chunk);
        let b_vec = f32x4::from_slice(b_chunk);
        let sum = a_vec + b_vec;
        let clamped = sum.clamp(min_vec, max_vec);
        clamped.copy_to_slice(&mut output[i*4..(i+1)*4]);
    }
    
    // Остаток
    let start = a.len() - remainder;
    for i in 0..remainder {
        let sum = a[start + i] + b[start + i];
        output[start + i] = sum.max(-1.0).min(1.0);
    }
}

/// SIMD fast tanh approximation
pub fn simd_fast_tanh(input: &[f32], output: &mut [f32]) {
    let one_vec = f32x4::splat(1.0);
    
    let chunks = input.chunks_exact(4);
    let remainder = chunks.remainder();
    
    for (i, chunk) in chunks.enumerate() {
        let input_vec = f32x4::from_slice(chunk);
        let abs_vec = input_vec.abs();
        let denom = one_vec + abs_vec;
        let result = input_vec / denom;
        result.copy_to_slice(&mut output[i*4..(i+1)*4]);
    }
    
    // Остаток
    for i in 0..remainder.len() {
        let idx = input.len() - remainder.len() + i;
        let x = input[idx];
        output[idx] = x / (1.0 + x.abs());
    }
}

/// SIMD биквадратный фильтр
pub struct SimdBiquadFilter {
    b0: f32, b1: f32, b2: f32,
    a1: f32, a2: f32,
    z1: f32, z2: f32,
}

impl SimdBiquadFilter {
    pub fn new_lowpass(cutoff: f32, q: f32, sample_rate: f32) -> Self {
        let omega = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        
        let b0 = (1.0 - omega.cos()) / 2.0;
        let b1 = 1.0 - omega.cos();
        let b2 = b0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * omega.cos();
        let a2 = 1.0 - alpha;
        
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }
    
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        // Скалярная версия для простоты
        for i in 0..input.len().min(output.len()) {
            let x = input[i];
            let y = x * self.b0 + self.z1;
            self.z1 = x * self.b1 + self.z2 - y * self.a1;
            self.z2 = x * self.b2 - y * self.a2;
            output[i] = y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simd_gain() {
        let input: Vec<f32> = (0..10).map(|i| i as f32 * 0.1).collect();
        let mut output = vec![0.0f32; 10];
        let gain = 2.0;
        
        simd_gain(&input, &mut output, gain);
        
        // Проверяем корректность
        for i in 0..10 {
            let expected = input[i] * gain;
            assert!((output[i] - expected).abs() < 1e-6,
                   "Sample {}: {} != {}", i, output[i], expected);
        }
    }
    
    #[test]
    fn test_simd_saturating_add() {
        let a: Vec<f32> = vec![0.8, 0.6, 0.4, 0.2, 0.0, -0.2, -0.4, -0.6];
        let b: Vec<f32> = vec![0.3, 0.5, 0.7, 0.9, 1.1, 1.3, -0.8, -0.9];
        let mut output = vec![0.0f32; 8];
        
        simd_saturating_add(&a, &b, &mut output);
        
        // Проверяем насыщение
        for i in 0..8 {
            let sum = a[i] + b[i];
            let expected = sum.max(-1.0).min(1.0);
            assert!((output[i] - expected).abs() < 1e-6,
                   "Sample {}: {} != {}", i, output[i], expected);
        }
    }
}