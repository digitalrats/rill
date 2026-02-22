//! # Анализ аудиосигналов
//! 
//! Предоставляет инструменты для анализа аудиоданных:
//! 
//! - [`SpectrumAnalyzer`] — анализ спектра
//! - [`PeakDetector`] — детектор пиков (для VU-метров, лимитеров)

/// Анализатор спектра (FFT).
pub struct SpectrumAnalyzer {
    fft_size: usize,
    magnitudes: Vec<f64>,
}

impl SpectrumAnalyzer {
    /// Создать новый анализатор спектра.
    pub fn new(fft_size: usize) -> Self {
        Self {
            fft_size,
            magnitudes: vec![0.0; fft_size / 2],
        }
    }
    
    /// Выполнить анализ спектра.
    /// 
    /// # Аргументы
    /// * `signal` — входной сигнал
    /// 
    /// # Возвращает
    /// Вектор амплитуд для каждой частотной полосы
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

/// Детектор пиков.
/// 
/// Полезен для VU-метров, лимитеров, компрессоров.
pub struct PeakDetector {
    peak: f64,
    decay: f64,
}

impl PeakDetector {
    /// Создать новый детектор пиков.
    /// 
    /// # Аргументы
    /// * `decay` — скорость затухания пика (0.0-1.0)
    pub fn new(decay: f64) -> Self {
        Self { peak: 0.0, decay }
    }
    
    /// Обработать семпл, вернуть текущий пик.
    pub fn process(&mut self, sample: f64) -> f64 {
        self.peak = (self.peak * self.decay).max(sample.abs());
        self.peak
    }
    
    /// Сбросить состояние.
    pub fn reset(&mut self) {
        self.peak = 0.0;
    }
}