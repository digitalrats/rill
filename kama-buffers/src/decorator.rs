//! Декораторы для обработки буферов

/// Декоратор для панорамирования
pub struct PanningDecorator {
    pan: f32,
}

impl PanningDecorator {
    pub fn new(pan: f32) -> Self {
        Self { pan: pan.clamp(-1.0, 1.0) }
    }
    
    pub fn process(&self, left: &mut [f32], right: &mut [f32]) {
        let (left_gain, right_gain) = if self.pan <= 0.0 {
            (1.0, 1.0 + self.pan)
        } else {
            (1.0 - self.pan, 1.0)
        };
        
        for i in 0..left.len().min(right.len()) {
            left[i] *= left_gain;
            right[i] *= right_gain;
        }
    }
}

/// Декоратор с LFO модуляцией
pub struct LfoDecorator {
    frequency: f32,
    amplitude: f32,
    phase: f32,
    sample_rate: f32,
}

impl LfoDecorator {
    pub fn new(frequency: f32, amplitude: f32, sample_rate: f32) -> Self {
        Self {
            frequency,
            amplitude: amplitude.clamp(0.0, 1.0),
            phase: 0.0,
            sample_rate,
        }
    }
    
    pub fn process(&mut self, buffer: &mut [f32]) {
        let phase_increment = 2.0 * std::f32::consts::PI * self.frequency / self.sample_rate;
        
        for (i, sample) in buffer.iter_mut().enumerate() {
            let modulation = (self.phase + i as f32 * phase_increment).sin() * self.amplitude;
            *sample *= 1.0 + modulation;
        }
        
        self.phase += buffer.len() as f32 * phase_increment;
        while self.phase > 2.0 * std::f32::consts::PI {
            self.phase -= 2.0 * std::f32::consts::PI;
        }
    }
}