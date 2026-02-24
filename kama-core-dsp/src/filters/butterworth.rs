//! # Фильтры Баттерворта (Butterworth Filters)

use crate::math::AudioNum;
use super::{Filter, FilterParams, FilterType};
use crate::algorithm::{Algorithm, ParameterizedAlgorithm, AlgorithmMetadata, AlgorithmCategory};
use std::f64::consts::PI as PI64;
use num_complex::Complex64;

// -----------------------------------------------------------------------------
// Вспомогательные функции
// -----------------------------------------------------------------------------

fn butterworth_analog_poles(n: usize) -> Vec<Complex64> {
    let mut poles = Vec::with_capacity(n);
    let n_f64 = n as f64;
    
    for k in 1..=n {
        let k_f64 = k as f64;
        let theta = PI64 * (2.0 * k_f64 - 1.0) / (2.0 * n_f64);
        
        let real = -theta.sin();
        let imag = theta.cos();
        
        poles.push(Complex64::new(real, imag));
    }
    
    poles
}

// -----------------------------------------------------------------------------
// Биквадратная секция
// -----------------------------------------------------------------------------

#[derive(Clone)]
struct BiquadSection<T: AudioNum> {
    coeffs: (T, T, T, T, T),
    state: (T, T, T, T),
}

impl<T: AudioNum> BiquadSection<T> {
    fn new() -> Self {
        Self {
            coeffs: (T::from_f32(1.0), T::ZERO, T::ZERO, T::ZERO, T::ZERO),
            state: (T::ZERO, T::ZERO, T::ZERO, T::ZERO),
        }
    }
    
    #[inline(always)]
    fn process(&mut self, input: T) -> T {
        let (b0, b1, b2, a1, a2) = self.coeffs;
        let (x1, x2, y1, y2) = self.state;
        
        let output = b0.mul(input)
            .add(b1.mul(x1))
            .add(b2.mul(x2))
            .sub(a1.mul(y1))
            .sub(a2.mul(y2));
        
        self.state = (input, x1, output, y1);
        
        output
    }
    
    fn set_coeffs(&mut self, b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) {
        self.coeffs = (
            T::from_f32(b0 as f32),
            T::from_f32(b1 as f32),
            T::from_f32(b2 as f32),
            T::from_f32(a1 as f32),
            T::from_f32(a2 as f32),
        );
    }
    
    fn reset(&mut self) {
        self.state = (T::ZERO, T::ZERO, T::ZERO, T::ZERO);
    }
}

// -----------------------------------------------------------------------------
// Фильтр Баттерворта
// -----------------------------------------------------------------------------

/// Фильтр Баттерворта (каскадная реализация)
pub struct Butterworth<T: AudioNum, const MAX_SECTIONS: usize> {
    /// Параметры фильтра (используем общий FilterParams)
    params: FilterParams,
    /// Порядок фильтра
    order: usize,
    /// Биквадратные секции
    sections: [BiquadSection<T>; MAX_SECTIONS],
    /// Количество активных секций
    num_sections: usize,
    /// Gain для нормализации
    gain: T,
    /// Частота дискретизации
    sample_rate: f32,
}

impl<T: AudioNum, const MAX_SECTIONS: usize> Butterworth<T, MAX_SECTIONS> {
    /// Создать новый фильтр Баттерворта
    pub fn new(params: FilterParams, order: usize) -> Self {
        let mut filter = Self {
            params,
            order,
            sections: [(); MAX_SECTIONS].map(|_| BiquadSection::new()),
            num_sections: 0,
            gain: T::from_f32(1.0),
            sample_rate: 44100.0,
        };
        filter.design();
        filter
    }
    
    /// Создать фильтр нижних частот
    pub fn lowpass(cutoff: f32, order: usize) -> Self {
        Self::new(FilterParams {
            filter_type: FilterType::LowPass,
            cutoff,
            q: 0.0,
            gain_db: 0.0,
        }, order)
    }
    
    /// Создать фильтр верхних частот
    pub fn highpass(cutoff: f32, order: usize) -> Self {
        Self::new(FilterParams {
            filter_type: FilterType::HighPass,
            cutoff,
            q: 0.0,
            gain_db: 0.0,
        }, order)
    }
    
    /// Спроектировать фильтр (рассчитать коэффициенты)
    pub fn design(&mut self) {
        let n = self.order;
        let cutoff = self.params.cutoff as f64;
        let sample_rate_f64 = self.sample_rate as f64;
        
        // Pre-warping частоты
        let warp_cutoff = 2.0 * (PI64 * cutoff / sample_rate_f64).tan();
        
        // Получаем полюса аналогового фильтра
        let analog_poles = butterworth_analog_poles(n);
        
        // Количество биквадратных секций
        self.num_sections = (n + 1) / 2;
        
        // Вычисляем gain
        self.gain = self.compute_gain(&analog_poles, warp_cutoff);
        
        // Группируем полюса в комплексно-сопряжённые пары
        for i in 0..self.num_sections {
            let idx1 = i * 2;
            let idx2 = i * 2 + 1;
            
            if idx2 < n {
                let p1 = analog_poles[idx1];
                let p2 = analog_poles[idx2];
                
                let sp1 = p1 * warp_cutoff;
                let sp2 = p2 * warp_cutoff;
                
                let zp1 = (Complex64::new(2.0, 0.0) + sp1) / (Complex64::new(2.0, 0.0) - sp1);
                let zp2 = (Complex64::new(2.0, 0.0) + sp2) / (Complex64::new(2.0, 0.0) - sp2);
                
                let a1 = -(zp1 + zp2).re;
                let a2 = (zp1 * zp2).re;
                
                let (b0, b1, b2) = self.numerator_coeffs(i);
                
                self.sections[i].set_coeffs(b0, b1, b2, a1, a2);
            } else {
                let p = analog_poles[idx1];
                
                let sp = p * warp_cutoff;
                let zp = (Complex64::new(2.0, 0.0) + sp) / (Complex64::new(2.0, 0.0) - sp);
                
                let a1 = -zp.re;
                let a2 = 0.0;
                
                let (b0, b1, b2) = self.numerator_coeffs(i);
                
                self.sections[i].set_coeffs(b0, b1, b2, a1, a2);
            }
        }
    }
    
    fn compute_gain(&self, analog_poles: &[Complex64], warp_cutoff: f64) -> T {
        let n = self.order;
        
        let mut analog_gain = 1.0;
        for pole in analog_poles {
            analog_gain *= (-pole).norm();
        }
        
        match self.params.filter_type {
            FilterType::LowPass => {
                let mut digital_response = Complex64::new(1.0, 0.0);
                for i in 0..self.num_sections {
                    let (b0, b1, b2, a1, a2) = self.sections[i].coeffs;
                    
                    let b = Complex64::new(
                        (b0.as_f32() + b1.as_f32() + b2.as_f32()) as f64,
                        0.0
                    );
                    let a = Complex64::new(
                        (1.0 + a1.as_f32() + a2.as_f32()) as f64,
                        0.0
                    );
                    
                    digital_response = digital_response * b / a;
                }
                
                T::from_f32((analog_gain / digital_response.norm()) as f32)
            }
            
            FilterType::HighPass => {
                let mut digital_response = Complex64::new(1.0, 0.0);
                for i in 0..self.num_sections {
                    let (b0, b1, b2, a1, a2) = self.sections[i].coeffs;
                    
                    let b = Complex64::new(
                        (b0.as_f32() - b1.as_f32() + b2.as_f32()) as f64,
                        0.0
                    );
                    let a = Complex64::new(
                        (1.0 - a1.as_f32() + a2.as_f32()) as f64,
                        0.0
                    );
                    
                    digital_response = digital_response * b / a;
                }
                
                T::from_f32((1.0 / digital_response.norm()) as f32)
            }
            
            _ => T::from_f32(1.0),
        }
    }
    
    fn numerator_coeffs(&self, _section_idx: usize) -> (f64, f64, f64) {
        match self.params.filter_type {
            FilterType::LowPass => (1.0, 2.0, 1.0),
            FilterType::HighPass => (1.0, -2.0, 1.0),
            FilterType::BandPass => (1.0, 0.0, -1.0),
            _ => (1.0, 0.0, 0.0),
        }
    }
}

impl<T: AudioNum, const MAX_SECTIONS: usize> Algorithm<T> for Butterworth<T, MAX_SECTIONS> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.design();
        self.reset();
    }
    
    fn reset(&mut self) {
        for section in &mut self.sections[..self.num_sections] {
            section.reset();
        }
    }
    
    fn process_sample(&mut self, input: T) -> T {
        let mut x = input.mul(self.gain);
        
        for section in &mut self.sections[..self.num_sections] {
            x = section.process(x);
        }
        
        x
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Butterworth Filter",
            category: AlgorithmCategory::Filter,
            description: format!("Butterworth filter (order {})", self.order),
            author: "Kama Audio",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
    
    fn as_any(&self) -> &dyn std::any::Any 
    where
        Self: 'static,
    {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any 
    where
        Self: 'static,
    {
        self
    }
}

impl<T: AudioNum, const MAX_SECTIONS: usize> ParameterizedAlgorithm<T> for Butterworth<T, MAX_SECTIONS> {
    type Params = FilterParams;
    
    fn params(&self) -> &Self::Params {
        &self.params
    }
    
    fn set_params(&mut self, params: Self::Params) {
        self.params = params;
        self.design();
    }
}