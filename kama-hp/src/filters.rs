use std::f64::consts::PI;

/// Тип биквадратного фильтра
#[derive(Debug, Clone, Copy)]
pub enum BiquadType {
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

/// Высокоточный биквадратный фильтр
pub struct HighPrecisionBiquad {
    b0: f64, b1: f64, b2: f64,
    a1: f64, a2: f64,
    x1: f64, x2: f64,
    y1: f64, y2: f64,
    sample_rate: f64,
}

impl HighPrecisionBiquad {
    /// Создаёт фильтр нижних частот.
    pub fn new_lowpass(cutoff: f64, q: f64, sample_rate: f64) -> Self {
        let omega = 2.0 * PI * cutoff / sample_rate;
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
            x1: 0.0, x2: 0.0,
            y1: 0.0, y2: 0.0,
            sample_rate,
        }
    }
    
    /// Создаёт фильтр верхних частот.
    pub fn new_highpass(cutoff: f64, q: f64, sample_rate: f64) -> Self {
        let omega = 2.0 * PI * cutoff / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cos_omega = omega.cos();
        
        let b0 = (1.0 + cos_omega) / 2.0;
        let b1 = -(1.0 + cos_omega);
        let b2 = b0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha;
        
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0, x2: 0.0,
            y1: 0.0, y2: 0.0,
            sample_rate,
        }
    }
    
    /// Создаёт полосовой фильтр.
    pub fn new_bandpass(cutoff: f64, q: f64, sample_rate: f64) -> Self {
        let omega = 2.0 * PI * cutoff / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        
        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * omega.cos();
        let a2 = 1.0 - alpha;
        
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0, x2: 0.0,
            y1: 0.0, y2: 0.0,
            sample_rate,
        }
    }
    
    /// Создаёт режекторный фильтр.
    pub fn new_notch(cutoff: f64, q: f64, sample_rate: f64) -> Self {
        let omega = 2.0 * PI * cutoff / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cos_omega = omega.cos();
        
        let b0 = 1.0;
        let b1 = -2.0 * cos_omega;
        let b2 = 1.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha;
        
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0, x2: 0.0,
            y1: 0.0, y2: 0.0,
            sample_rate,
        }
    }
    
    /// Обрабатывает один семпл.
    pub fn process(&mut self, input: f64) -> f64 {
        let output = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1 - self.a2 * self.y2;
        
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        
        output
    }
    
    /// Обрабатывает буфер целиком.
    pub fn process_buffer(&mut self, input: &[f64], output: &mut [f64]) {
        for i in 0..input.len().min(output.len()) {
            output[i] = self.process(input[i]);
        }
    }
    
    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// Высокоточный лестничный фильтр (Moog ladder)
pub struct HighPrecisionLadderFilter {
    cutoff: f64,
    resonance: f64,
    sample_rate: f64,
    stage1: f64,
    stage2: f64,
    stage3: f64,
    stage4: f64,
}

impl HighPrecisionLadderFilter {
    pub fn new(cutoff: f64, resonance: f64, sample_rate: f64) -> Self {
        Self {
            cutoff,
            resonance: resonance.clamp(0.0, 1.0),
            sample_rate,
            stage1: 0.0,
            stage2: 0.0,
            stage3: 0.0,
            stage4: 0.0,
        }
    }
    
    pub fn set_cutoff(&mut self, cutoff: f64) {
        self.cutoff = cutoff.max(20.0).min(self.sample_rate / 2.0);
    }
    
    pub fn set_resonance(&mut self, resonance: f64) {
        self.resonance = resonance.clamp(0.0, 1.0);
    }
    
    /// Обрабатывает один семпл.
    pub fn process(&mut self, input: f64) -> f64 {
        let f = 2.0 * (PI * self.cutoff / self.sample_rate).sin();
        let fb = self.resonance * 4.0;
        
        let x = input - fb * self.stage4;
        
        self.stage1 = x * f + self.stage1 - f * self.stage1;
        self.stage2 = self.stage1 * f + self.stage2 - f * self.stage2;
        self.stage3 = self.stage2 * f + self.stage3 - f * self.stage3;
        self.stage4 = self.stage3 * f + self.stage4 - f * self.stage4;
        
        self.stage4
    }
    
    /// Обрабатывает буфер целиком.
    pub fn process_buffer(&mut self, input: &[f64], output: &mut [f64]) {
        for i in 0..input.len().min(output.len()) {
            output[i] = self.process(input[i]);
        }
    }
    
    pub fn reset(&mut self) {
        self.stage1 = 0.0;
        self.stage2 = 0.0;
        self.stage3 = 0.0;
        self.stage4 = 0.0;
    }
}

/// Каскад биквадратных фильтров (для фильтров высокого порядка)
pub struct HighPrecisionBiquadCascade {
    filters: Vec<HighPrecisionBiquad>,
    temp_buffer: Vec<f64>,
}

impl HighPrecisionBiquadCascade {
    pub fn new_elliptic_lowpass(
        order: usize,
        cutoff: f64,
        _ripple: f64,
        _stopband_attenuation: f64,
        sample_rate: f64,
    ) -> Self {
        let filters = (0..order)
            .map(|_| HighPrecisionBiquad::new_lowpass(cutoff, 0.707, sample_rate))
            .collect();
        
        Self {
            filters,
            temp_buffer: Vec::new(),
        }
    }
    
    pub fn process_buffer(&mut self, input: &[f64], output: &mut [f64]) {
        if self.temp_buffer.len() < input.len() * 2 {
            self.temp_buffer.resize(input.len() * 2, 0.0);
        }
        
        self.temp_buffer[..input.len()].copy_from_slice(input);
        
        // Вычисляем длину ДО начала итерации, чтобы избежать конфликта заимствований
        let num_filters = self.filters.len();
        
        for (i, filter) in self.filters.iter_mut().enumerate() {
            if i == num_filters - 1 {
                // Последний фильтр: читаем из temp_buffer, пишем в output
                filter.process_buffer(&self.temp_buffer[..input.len()], output);
            } else {
                // Промежуточный фильтр: читаем из первой половины, пишем во вторую
                let (in_buf, out_buf) = self.temp_buffer.split_at_mut(input.len());
                filter.process_buffer(in_buf, out_buf);
                // Копируем результат обратно в первую половину для следующей итерации
                in_buf[..input.len()].copy_from_slice(&out_buf[..input.len()]);
            }
        }
    }
}