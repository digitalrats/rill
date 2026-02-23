//! # Конвертеры и oversampling
//!
//! Предоставляет инструменты для изменения частоты дискретизации и разрядности:
//!
//! - [`Oversampler`] — повышение/понижение частоты дискретизации
//! - [`PrecisionConverter`] — конвертация f64 ↔ f32 с dither'ом и noise shaping'ом

use rand::Rng;
use std::f64::consts::PI;

pub use crate::effects::DitherType;

/// Конвертер с oversampling'ом.
pub struct Oversampler {
    factor: usize,
    temp_buffer: Vec<f64>,
}

impl Oversampler {
    /// Создать новый oversampler.
    ///
    /// # Аргументы
    /// * `factor` — коэффициент oversampling'а
    pub fn new(factor: usize) -> Self {
        Self {
            factor,
            temp_buffer: Vec::new(),
        }
    }

    /// Повысить частоту дискретизации вставкой нулей.
    pub fn upsample(&mut self, input: &[f64], output: &mut [f64]) {
        let os_factor = self.factor;
        let output_len = input.len() * os_factor;

        if output.len() < output_len {
            return;
        }

        for (i, &sample) in input.iter().enumerate() {
            let base_idx = i * os_factor;
            output[base_idx] = sample;
            for j in 1..os_factor {
                output[base_idx + j] = 0.0;
            }
        }
    }

    /// Понизить частоту дискретизации простой децимацией.
    pub fn downsample(&mut self, input: &[f64], output: &mut [f64]) {
        let os_factor = self.factor;

        for (i, out) in output.iter_mut().enumerate() {
            let idx = i * os_factor;
            if idx < input.len() {
                *out = input[idx];
            }
        }
    }
}

/// Конвертер точности (f64 ↔ f32) с опциями dithering и noise shaping.
pub struct PrecisionConverter {
    dither_enabled: bool,
    dither_type: DitherType,
    bit_depth: u8,
    last_error: f64,
}

impl PrecisionConverter {
    /// Создать новый конвертер.
    ///
    /// # Аргументы
    /// * `dither_enabled` — включить dither
    /// * `dither_type` — тип dither'а
    /// * `bit_depth` — целевая разрядность (для dither'а)
    pub fn new(dither_enabled: bool, dither_type: DitherType, bit_depth: u8) -> Self {
        Self {
            dither_enabled,
            dither_type,
            bit_depth,
            last_error: 0.0,
        }
    }

    /// Конвертировать f64 → f32 с возможным dither'ом.
    pub fn f64_to_f32(&mut self, input: &[f64], output: &mut [f32]) {
        for i in 0..input.len().min(output.len()) {
            let mut sample = input[i];

            if self.dither_enabled {
                sample = self.apply_dither(sample);
            }

            // Простое noise shaping первого порядка
            sample += self.last_error * 0.5;

            let quantized = sample as f32;
            self.last_error = sample - quantized as f64;

            output[i] = quantized.clamp(-1.0, 1.0);
        }
    }

    /// Конвертировать f32 → f64 (простое приведение).
    pub fn f32_to_f64(&self, input: &[f32], output: &mut [f64]) {
        for i in 0..input.len().min(output.len()) {
            output[i] = input[i] as f64;
        }
    }

    fn apply_dither(&self, sample: f64) -> f64 {
        let quant_step = 2.0_f64.powi(-(self.bit_depth as i32) + 1);

        match self.dither_type {
            DitherType::None => sample,
            DitherType::Rectangular => {
                let dither = rand::thread_rng().gen::<f64>() * 2.0 - 1.0;
                sample + dither * quant_step
            }
            DitherType::Triangular => {
                let d1 = rand::thread_rng().gen::<f64>();
                let d2 = rand::thread_rng().gen::<f64>();
                let dither = (d1 + d2 - 1.0) * 2.0;
                sample + dither * quant_step
            }
            DitherType::Gaussian => {
                let u1 = rand::thread_rng().gen::<f64>().max(1e-10);
                let u2 = rand::thread_rng().gen::<f64>();
                let dither = (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos();
                sample + dither * quant_step * 0.5
            }
            DitherType::HighPass => sample, // упрощённо
        }
    }
}
