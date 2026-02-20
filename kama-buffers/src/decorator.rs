/// Декоратор для панорамирования
pub struct PanningDecorator {
    pan: f32,
}

impl PanningDecorator {
    pub fn new(pan: f32) -> Self {
        Self { pan: pan.max(-1.0).min(1.0) }
    }
    
    pub fn process(&self, left: &mut [f32], right: &mut [f32]) {
        let (left_gain, right_gain) = self.pan_to_gains();
        
        for i in 0..left.len().min(right.len()) {
            left[i] *= left_gain;
            right[i] *= right_gain;
        }
    }
    
    fn pan_to_gains(&self) -> (f32, f32) {
        let pan = self.pan;
        let left_gain = if pan <= 0.0 { 1.0 } else { 1.0 - pan };
        let right_gain = if pan >= 0.0 { 1.0 } else { 1.0 + pan };
        (left_gain, right_gain)
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
            amplitude: amplitude.max(0.0).min(1.0),
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
        if self.phase > 2.0 * std::f32::consts::PI {
            self.phase -= 2.0 * std::f32::consts::PI;
        }
    }
}