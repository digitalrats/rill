//! Однополюсный фильтр (One-pole)
//!
//! Самый быстрый фильтр, идеально подходит для:
//! - Сглаживания параметров
//! - Простых low-pass/high-pass фильтров
//! - Envelope followers

use super::{Filter, FilterParams, FilterType};
use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata, ParameterizedAlgorithm};
use crate::vector::{ScalarVector1, Vector};
use core::f32::consts::PI;
use rill_core::AudioNum;

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
    alpha: ScalarVector1<T>,
    /// Предыдущий выход
    y1: ScalarVector1<T>,
    /// Частота дискретизации
    sample_rate: f32,
}

impl<T: AudioNum> OnePole<T> {
    /// Создать новый однополюсный фильтр
    pub fn new(params: FilterParams) -> Self {
        let mut filter = Self {
            params,
            alpha: ScalarVector1::splat(T::ZERO),
            y1: ScalarVector1::splat(T::ZERO),
            sample_rate: 44100.0,
        };
        filter.update_alpha();
        filter
    }

    /// Обновить коэффициент alpha
    fn update_alpha(&mut self) {
        // α = 1 - exp(-2π * cutoff / sample_rate)
        let exp_arg = -2.0 * PI * self.params.cutoff / self.sample_rate;
        self.alpha = ScalarVector1::splat(T::from_f32(1.0 - exp_arg.exp()));
    }

    /// Обработать один семпл
    pub fn process_sample(&mut self, input: T) -> T {
        let one = ScalarVector1::splat(T::from_f32(1.0));
        let inp = ScalarVector1::splat(input);
        let out = match self.params.filter_type {
            FilterType::LowPass => self.alpha * inp + (one - self.alpha) * self.y1,
            FilterType::HighPass => {
                // Для high-pass: y[n] = α * (y[n-1] + x[n] - x[n-1])
                // Упрощённая версия через low-pass: x - lowpass(x)
                let lp = self.alpha * inp + (one - self.alpha) * self.y1;
                inp - lp
            }
            _ => inp, // Другие типы не поддерживаются
        };
        self.y1 = out;
        out.extract(0)
    }
}

impl<T: AudioNum> Algorithm<T> for OnePole<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_alpha();
        self.reset();
    }

    fn reset(&mut self) {
        self.y1 = ScalarVector1::splat(T::ZERO);
    }

    fn process_block(&mut self, input: &[T], output: &mut [T]) {
        let len = input.len().min(output.len());
        let one = ScalarVector1::splat(T::from_f32(1.0));

        for i in 0..len {
            let inp = input[i];
            let out = match self.params.filter_type {
                FilterType::LowPass => self.alpha * inp + (one - self.alpha) * self.y1,
                FilterType::HighPass => {
                    // Для high-pass: y[n] = α * (y[n-1] + x[n] - x[n-1])
                    // Упрощённая версия через low-pass: x - lowpass(x)
                    let lp = self.alpha * inp + (one - self.alpha) * self.y1;
                    ScalarVector1::splat(inp) - lp
                }
                _ => ScalarVector1::splat(inp), // Другие типы не поддерживаются
            };

            self.y1 = out;
            output[i] = out.extract(0);
        }
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "One-Pole Filter",
            category: AlgorithmCategory::Filter,
            description: "Fast one-pole filter for smoothing",
            author: "Rill",
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
