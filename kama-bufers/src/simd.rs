//! SIMD оптимизации для буферных операций

use crate::{AudioBuffer, SharedAudioBuffer};
use kama_core::simd::*;

impl AudioBuffer for SharedAudioBuffer {
    // ... существующие методы ...
    
    /// SIMD оптимизированное чтение batch
    fn read_batch_simd(&self, start_pos: usize, count: usize, channel: usize) -> Vec<f32> {
        let config = SimdConfig::detect();
        let lanes = config.f32_width;
        
        let mut result = Vec::with_capacity(count);
        unsafe { result.set_len(count) };
        
        let data = self.data.read();
        let size = self.size;
        let channels = self.channels;
        
        // SIMD обработка
        let num_vectors = count / lanes;
        
        for i in 0..num_vectors {
            let base_idx = (start_pos + i * lanes) % size;
            
            // Читаем SIMD вектор
            let mut simd_data = [0.0f32; 16]; // Максимум для f32x16
            for j in 0..lanes {
                let pos = (base_idx + j) % size;
                let idx = pos * channels + channel;
                simd_data[j] = *data.get(idx).unwrap_or(&0.0);
            }
            
            // Копируем в результат
            for j in 0..lanes {
                result[i * lanes + j] = simd_data[j];
            }
        }
        
        // Остаток
        let start = num_vectors * lanes;
        for i in 0..(count - start) {
            let pos = (start_pos + start + i) % size;
            let idx = pos * channels + channel;
            result[start + i] = *data.get(idx).unwrap_or(&0.0);
        }
        
        result
    }
    
    /// SIMD оптимизированная запись batch
    fn write_batch_simd(&mut self, start_pos: usize, samples: &[f32], channel: usize) {
        let config = SimdConfig::detect();
        let lanes = config.f32_width;
        
        let mut data = self.data.write();
        let size = self.size;
        let channels = self.channels;
        
        let num_vectors = samples.len() / lanes;
        
        for i in 0..num_vectors {
            let base_idx = (start_pos + i * lanes) % size;
            let sample_start = i * lanes;
            
            for j in 0..lanes {
                let pos = (base_idx + j) % size;
                let idx = pos * channels + channel;
                if idx < data.len() {
                    data[idx] = samples[sample_start + j];
                }
            }
        }
        
        // Остаток
        let start = num_vectors * lanes;
        for i in 0..(samples.len() - start) {
            let pos = (start_pos + start + i) % size;
            let idx = pos * channels + channel;
            if idx < data.len() {
                data[idx] = samples[start + i];
            }
        }
    }
}

/// SIMD оптимизированный биткрашер
pub struct SimdBitcrusher {
    bit_depth: u8,
    sample_rate_reduction: f32,
    last_sample: f32,
    sample_counter: usize,
    simd_type: SimdType,
}

impl SimdBitcrusher {
    pub fn new(bit_depth: u8, sample_rate_reduction: f32) -> Self {
        let config = SimdConfig::detect();
        Self {
            bit_depth,
            sample_rate_reduction: sample_rate_reduction.clamp(0.0, 1.0),
            last_sample: 0.0,
            sample_counter: 0,
            simd_type: config.f32_simd_type(),
        }
    }
    
    pub fn process_simd(&mut self, input: &[f32], output: &mut [f32]) {
        match self.simd_type {
            SimdType::F32x4 => self.process_f32x4(input, output),
            SimdType::F32x8 => self.process_f32x8(input, output),
            SimdType::F32x16 => self.process_f32x16(input, output),
            _ => self.process_scalar(input, output),
        }
    }
    
    fn process_f32x4(&mut self, input: &[f32], output: &mut [f32]) {
        if self.bit_depth >= 32 {
            output.copy_from_slice(input);
            return;
        }
        
        let steps = (1u32 << self.bit_depth) as f32;
        let steps_vec = f32x4::splat(steps);
        let inv_steps_vec = f32x4::splat(1.0 / steps);
        
        let chunks = input.chunks_exact(4);
        let remainder = chunks.remainder();
        
        for (i, chunk) in chunks.enumerate() {
            let input_vec = f32x4::from_slice(chunk);
            
            // Квантование: round(x * steps) / steps
            let scaled = input_vec * steps_vec;
            let rounded = scaled.round();
            let quantized = rounded * inv_steps_vec;
            
            quantized.copy_to_slice(&mut output[i*4..(i+1)*4]);
        }
        
        // Остаток + sample rate reduction
        let start = input.len() - remainder.len();
        for i in 0..remainder.len() {
            let idx = start + i;
            output[idx] = self.process_sample(input[idx]);
        }
    }
    
    // Аналогично для других SIMD ширины...
    
    fn process_scalar(&mut self, input: &[f32], output: &mut [f32]) {
        for i in 0..input.len().min(output.len()) {
            output[i] = self.process_sample(input[i]);
        }
    }
    
    fn process_sample(&mut self, input: f32) -> f32 {
        // Sample rate reduction
        let should_update = self.sample_counter as f32 >= 1.0 / self.sample_rate_reduction;
        
        if should_update {
            self.sample_counter = 0;
            self.last_sample = self.quantize(input);
        } else {
            self.sample_counter += 1;
        }
        
        self.last_sample
    }
    
    fn quantize(&self, sample: f32) -> f32 {
        if self.bit_depth >= 32 {
            return sample;
        }
        
        let steps = (1u32 << self.bit_depth) as f32;
        let scaled = (sample * steps).round();
        scaled / steps
    }
}