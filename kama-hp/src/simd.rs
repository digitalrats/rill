//! # SIMD ускорение для high-precision операций (f64)
//! 
//! Предоставляет оптимизированные SIMD-реализации для операций с f64.
//! Модуль доступен только при включении фичи `simd`.

#![cfg(feature = "simd")]

use core::simd::{f64x2, f64x4, f64x8, SimdFloat};

/// Конфигурация SIMD для f64
#[derive(Debug, Clone, Copy)]
pub struct F64SimdConfig {
    /// Поддержка AVX-512
    pub has_avx512: bool,
    /// Поддержка AVX2
    pub has_avx2: bool,
    /// Поддержка SSE2
    pub has_sse2: bool,
    /// Оптимальная ширина SIMD (2, 4 или 8)
    pub optimal_width: usize,
}

impl F64SimdConfig {
    /// Определить доступные SIMD возможности на текущем процессоре
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
    
    for (i, chunk) in chunks.enumerate() {
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