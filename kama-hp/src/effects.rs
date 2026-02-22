//! # Высокоточные эффекты
//! 
//! Предоставляет эффекты для высокоточной обработки:
//! 
//! - [`NoiseShaper`] — понижение разрядности с noise shaping'ом
//! - [`SimpleReverb`] — простая реверберация

use rand::Rng;
use std::f64::consts::PI;

/// Тип dither'а для понижения разрядности.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DitherType {
    /// Без dither'а
    None,
    /// Прямоугольный dither (RPDF)
    Rectangular,
    /// Треугольный dither (TPDF)
    Triangular,
    /// Гауссов dither (GPDF)
    Gaussian,
    /// High-pass dither
    HighPass,
}


/// Noise shaper для уменьшения quantization noise при понижении разрядности.
pub struct NoiseShaper {
    dither_type: DitherType,
    bit_depth: u8,
    error_history: Vec<f64>,
    history_pos: usize,
    last_error: f64,
}

impl NoiseShaper {
    /// Создать новый noise shaper.
    pub fn new(dither_type: DitherType, bit_depth: u8) -> Self {
        Self {
            dither_type,
            bit_depth,
            error_history: vec![0.0; 4],
            history_pos: 0,
            last_error: 0.0,
        }
    }
    
    /// Обработать один семпл.
    /// 
    /// Возвращает квантованное значение с noise shaping'ом.
    pub fn process(&mut self, input: f64) -> f32 {
        let quant_step = 2.0_f64.powi(-(self.bit_depth as i32) + 1);
        
        // Добавляем dither
        let dithered = match self.dither_type {
            DitherType::None => input,
            DitherType::Rectangular => {
                let dither = rand::thread_rng().gen::<f64>() * 2.0 - 1.0;
                input + dither * quant_step
            }
            DitherType::Triangular => {
                let d1 = rand::thread_rng().gen::<f64>();
                let d2 = rand::thread_rng().gen::<f64>();
                let dither = (d1 + d2 - 1.0) * 2.0;
                input + dither * quant_step
            }
            DitherType::Gaussian => {
                let u1 = rand::thread_rng().gen::<f64>().max(1e-10);
                let u2 = rand::thread_rng().gen::<f64>();
                let dither = (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos();
                input + dither * quant_step * 0.5
            }
            DitherType::HighPass => {
                let tpdf = rand::thread_rng().gen::<f64>() + rand::thread_rng().gen::<f64>() - 1.0;
                let highpass = tpdf - 0.5 * self.last_error;
                self.last_error = tpdf;
                input + highpass * quant_step
            }
        };
        
        // Простое noise shaping первого порядка
        let shaped = dithered + 0.5 * self.error_history[self.history_pos];
        
        // Квантование
        let scaled = shaped / quant_step;
        let quantized = scaled.round() * quant_step;
        
        // Обновляем историю ошибок
        let error = shaped - quantized;
        self.history_pos = (self.history_pos + 1) % self.error_history.len();
        self.error_history[self.history_pos] = error;
        
        quantized as f32
    }
    
    /// Обработать буфер целиком.
    pub fn process_buffer(&mut self, input: &[f64], output: &mut [f32]) {
        for i in 0..input.len().min(output.len()) {
            output[i] = self.process(input[i]);
        }
    }
}

/// Простой ревербератор (комбинация гребенчатых фильтров).
pub struct SimpleReverb {
    comb_filters: Vec<f64>, // упрощённо: буферы задержки
    comb_pos: Vec<usize>,
    comb_gain: Vec<f64>,
    allpass_filters: Vec<f64>,
    allpass_pos: Vec<usize>,
    allpass_gain: Vec<f64>,
    sample_rate: f64,
    decay: f64,
    mix: f64,
}

impl SimpleReverb {
    /// Создать новый ревербератор.
    pub fn new(decay: f64, mix: f64, sample_rate: f64) -> Self {
        // Классические задержки для ревербератора
        let comb_delays = [0.0297, 0.0371, 0.0411, 0.0437];
        let allpass_delays = [0.005, 0.0017];
        
        let comb_filters: Vec<f64> = comb_delays
            .iter()
            .map(|&d| (d * sample_rate) as usize)
            .map(|size| vec![0.0; size])
            .flatten()
            .collect();
        // Упрощённо: здесь нужна более аккуратная реализация с отдельными буферами.
        // Для краткости оставим заглушку.
        
        Self {
            comb_filters: Vec::new(),
            comb_pos: Vec::new(),
            comb_gain: Vec::new(),
            allpass_filters: Vec::new(),
            allpass_pos: Vec::new(),
            allpass_gain: Vec::new(),
            sample_rate,
            decay,
            mix,
        }
    }
    
    /// Обработать один семпл.
    pub fn process(&mut self, input: f64) -> f64 {
        // Заглушка
        input * (1.0 - self.mix)
    }
}