//! Анализ аудиосигналов (спектр, пики и т.д.)

/// Анализатор спектра (FFT)
pub struct SpectrumAnalyzer {
    fft_size: usize,
    magnitudes: Vec<f64>,
}

impl SpectrumAnalyzer {
    pub fn new(fft_size: usize) -> Self {
        Self {
            fft_size,
            magnitudes: vec![0.0; fft_size / 2],
        }
    }
    
    /// Простой анализ спектра (без FFT, для примера)
    pub fn analyze(&mut self, signal: &[f64]) -> Vec<f64> {
        let mut result = Vec::new();
        for i in 0..self.magnitudes.len() {
            let freq = i as f64 * 44100.0 / self.fft_size as f64;
            let mut correlation = 0.0;
            for (j, &sample) in signal.iter().enumerate() {
                if j >= self.fft_size {
                    break;
                }
                let t = j as f64 / 44100.0;
                correlation += sample * (2.0 * std::f64::consts::PI * freq * t).sin();
            }
            result.push(correlation.abs());
        }
        result
    }
}

/// Детектор пиков
pub struct PeakDetector {
    peak: f64,
    decay: f64,
}

impl PeakDetector {
    pub fn new(decay: f64) -> Self {
        Self { peak: 0.0, decay }
    }
    
    pub fn process(&mut self, sample: f64) -> f64 {
        self.peak = (self.peak * self.decay).max(sample.abs());
        self.peak
    }
    
    pub fn reset(&mut self) {
        self.peak = 0.0;
    }
}