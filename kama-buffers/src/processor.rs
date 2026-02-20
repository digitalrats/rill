//! Процессоры для обработки семплов в головках буфера

use crate::head::HeadState;
use crate::view::BufferView;

/// Обработчик семпла для головки
#[derive(Clone)]
pub enum SampleProcessor {
    None,
    Gain(f32),
    Pan(f32), // -1.0 (лево) до 1.0 (право)
    Lfo {
        frequency: f32,
        amplitude: f32,
        phase: f32,
    },
    Custom(HeadProcessor),
}

/// Процессор головки с пользовательской функцией
pub struct HeadProcessor {
    pub process_func: Box<dyn Fn(f32, &HeadState, &BufferView) -> f32 + Send + Sync>,
}

impl Clone for HeadProcessor {
    fn clone(&self) -> Self {
        // Для простоты возвращаем заглушку
        Self {
            process_func: Box::new(|sample, _state, _view| sample),
        }
    }
}

// Статические методы для обработки головок
impl SampleProcessor {
    pub(crate) fn process_sample_static(
        sample: f32, 
        state: &HeadState, 
        processor: &SampleProcessor, 
        sample_rate: f32, 
        _view: &BufferView
    ) -> f32 {
        let mut result = sample * state.volume;
        
        match processor {
            SampleProcessor::None => {}
            SampleProcessor::Gain(gain) => {
                result *= *gain;
            }
            SampleProcessor::Pan(_) => {
                // Панорамирование применяется при записи в outputs
            }
            SampleProcessor::Lfo { frequency, amplitude, phase } => {
                let time = state.current_position as f32 / sample_rate;
                let lfo = (2.0 * std::f32::consts::PI * *frequency * time + *phase).sin();
                result *= 1.0 + lfo * *amplitude;
            }
            SampleProcessor::Custom(processor) => {
                result = (processor.process_func)(result, state, _view);
            }
        }
        
        result
    }
    
    pub(crate) fn grain_window_static(position: usize, grain_size: usize) -> f32 {
        let x = position as f32 / grain_size as f32;
        0.5 * (1.0 - (2.0 * std::f32::consts::PI * x).cos())
    }
    
    pub(crate) fn pan_to_gains_static(pan: f32) -> (f32, f32) {
        let pan = pan.max(-1.0).min(1.0);
        let left_gain = if pan <= 0.0 { 1.0 } else { 1.0 - pan };
        let right_gain = if pan >= 0.0 { 1.0 } else { 1.0 + pan };
        (left_gain, right_gain)
    }
    
    pub(crate) fn write_to_outputs_static(
        sample: f32, 
        pan: f32, 
        index: usize, 
        outputs: &mut [&mut [f32]]
    ) {
        if outputs.is_empty() {
            return;
        }
        
        if outputs.len() >= 2 {
            let (first, rest) = outputs.split_at_mut(1);
            let left_output = &mut first[0];
            let right_output = &mut rest[0];
            
            if index < left_output.len() && index < right_output.len() {
                let (left_gain, right_gain) = Self::pan_to_gains_static(pan);
                left_output[index] += sample * left_gain;
                right_output[index] += sample * right_gain;
            }
        } else {
            let output = &mut outputs[0];
            if index < output.len() {
                output[index] += sample;
            }
        }
    }
}