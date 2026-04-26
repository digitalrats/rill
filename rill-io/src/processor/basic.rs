//! Базовые процессоры для тестирования и отладки

use crate::engine::AudioProcessor;

/// Процессор, пропускающий входной сигнал без изменений
pub struct PassThroughProcessor;

impl AudioProcessor for PassThroughProcessor {
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        output.copy_from_slice(input);
    }
    
    fn reset(&mut self) {}
    
    fn set_sample_rate(&mut self, _sample_rate: f32) {}
}

/// Процессор, генерирующий тишину (все нули)
pub struct SilenceProcessor;

impl AudioProcessor for SilenceProcessor {
    fn process(&mut self, _input: &[f32], output: &mut [f32]) {
        output.fill(0.0);
    }
    
    fn reset(&mut self) {}
    
    fn set_sample_rate(&mut self, _sample_rate: f32) {}
}

/// Процессор, усиливающий сигнал с заданным коэффициентом
pub struct GainProcessor {
    gain: f32,
}

impl GainProcessor {
    /// Создать новый процессор с коэффициентом усиления
    pub fn new(gain: f32) -> Self {
        Self { gain: gain.max(0.0) }
    }
    
    /// Установить коэффициент усиления
    pub fn set_gain(&mut self, gain: f32) {
        self.gain = gain.max(0.0);
    }
}

impl AudioProcessor for GainProcessor {
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        for (i, sample) in input.iter().enumerate() {
            if i < output.len() {
                output[i] = sample * self.gain;
            }
        }
    }
    
    fn reset(&mut self) {}
    
    fn set_sample_rate(&mut self, _sample_rate: f32) {}
}

/// Процессор, смешивающий два канала в моно
pub struct MonoMixerProcessor;

impl AudioProcessor for MonoMixerProcessor {
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let stereo_samples = input.len() / 2;
        for i in 0..stereo_samples.min(output.len()) {
            output[i] = (input[i*2] + input[i*2 + 1]) * 0.5;
        }
    }
    
    fn reset(&mut self) {}
    
    fn set_sample_rate(&mut self, _sample_rate: f32) {}
}

/// Процессор, записывающий входной сигнал в кольцевой буфер (для отладки)
#[cfg(feature = "examples")]
pub struct CaptureProcessor {
    buffer: Vec<f32>,
    position: usize,
}

#[cfg(feature = "examples")]
impl CaptureProcessor {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity],
            position: 0,
        }
    }
    
    pub fn get_buffer(&self) -> &[f32] {
        &self.buffer
    }
}

#[cfg(feature = "examples")]
impl AudioProcessor for CaptureProcessor {
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        // Просто копируем вход в выход
        output.copy_from_slice(input);
        
        // И записываем в буфер
        for sample in input {
            self.buffer[self.position] = *sample;
            self.position = (self.position + 1) % self.buffer.len();
        }
    }
    
    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.position = 0;
    }
    
    fn set_sample_rate(&mut self, _sample_rate: f32) {}
}