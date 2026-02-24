//! Однополюсный фильтр (One-pole)
//!
//! Самый быстрый фильтр, идеально подходит для:
//! - Сглаживания параметров
//! - Простых low-pass/high-pass фильтров
//! - Envelope followers

use crate::math::AudioNum;
use super::{Filter, FilterParams, FilterType};
use crate::algorithm::{Algorithm, ParameterizedAlgorithm, AlgorithmMetadata, AlgorithmCategory};
use core::f32::consts::PI;

/// Однополюсный фильтр
///
/// # Формула
/// ```text
/// y[n] = a * x[n] + (1 - a) * y[n-1]
/// ```
pub struct OnePole<T: AudioNum> {
    /// Параметры фильтра
    params: FilterParams,
    /// Коэффициент фильтра
    alpha: T,
    /// Предыдущий выход
    y1: T,
    /// Частота дискретизации
    sample_rate: f32,
}

impl<T: AudioNum> OnePole<T> {
    /// Создать новый однополюсный фильтр
    pub fn new(params: FilterParams) -> Self {
        let mut filter = Self {
            params,
            alpha: T::ZERO,
            y1: T::ZERO,
            sample_rate: 44100.0,
        };
        filter.update_alpha();
        filter
    }
    
    /// Обновить коэффициент alpha
    fn update_alpha(&mut self) {
        // α = 1 - exp(-2π * cutoff / sample_rate)
        let exp_arg = -2.0 * PI * self.params.cutoff / self.sample_rate;
        self.alpha = T::from_f32(1.0 - exp_arg.exp());
    }
}

impl<T: AudioNum> Algorithm<T> for OnePole<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_alpha();
        self.reset();
    }
    
    fn reset(&mut self) {
        self.y1 = T::ZERO;
    }
    
    fn process_sample(&mut self, input: T) -> T {
        // y[n] = α * x[n] + (1-α) * y[n-1]
        let output = match self.params.filter_type {
            FilterType::LowPass => {
                self.alpha.mul(input).add(T::from_f32(1.0).sub(self.alpha).mul(self.y1))
            }
            FilterType::HighPass => {
                // Для high-pass: y[n] = α * (y[n-1] + x[n] - x[n-1])
                // Упрощённая версия через low-pass: x - lowpass(x)
                let lp = self.alpha.mul(input).add(T::from_f32(1.0).sub(self.alpha).mul(self.y1));
                input.sub(lp)
            }
            _ => input, // Другие типы не поддерживаются
        };
        
        self.y1 = output;
        output
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "One-Pole Filter",
            category: AlgorithmCategory::Filter,
            description: "Fast one-pole filter for smoothing",
            author: "Kama Audio",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: AudioNum> ParameterizedAlgorithm<T> for OnePole<T> {
    type Params = FilterParams;
    
    fn params(&self) -> &Self::Params {
        &self.params
    }
    
    fn set_params(&mut self, params: Self::Params) {
        self.params = params;
        self.update_alpha();
    }
}