//! Фильтр переменных состояния (State Variable Filter)
//!
//! Преимущества:
//! - Одновременный low-pass, high-pass и band-pass выходы
//! - Стабилен при высоких резонансах
//! - Идеален для аналоговой эмуляции

use super::{Filter, FilterParams, FilterType};
use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata, ParameterizedAlgorithm};
use crate::vector::{ScalarVector1, Vector};
use core::f32::consts::PI;
use rill_core::AudioNum;

/// Фильтр переменных состояния
///
/// Даёт три выхода одновременно:
/// - lowpass: низкие частоты
/// - highpass: высокие частоты
/// - bandpass: полосовой
pub struct StateVariableFilter<T: AudioNum> {
    /// Параметры фильтра
    params: FilterParams,
    /// Коэффициенты
    f: ScalarVector1<T>, // частота
    q: ScalarVector1<T>, // резонанс
    /// Состояние
    lp: ScalarVector1<T>, // low-pass выход
    hp: ScalarVector1<T>, // high-pass выход
    bp: ScalarVector1<T>, // band-pass выход
    /// Предыдущий вход (для задержки)
    x1: ScalarVector1<T>,
    /// Частота дискретизации
    sample_rate: f32,
}

impl<T: AudioNum> StateVariableFilter<T> {
    /// Создать новый SVF
    pub fn new(params: FilterParams) -> Self {
        let mut filter = Self {
            params,
            f: ScalarVector1::splat(T::ZERO),
            q: ScalarVector1::splat(T::ZERO),
            lp: ScalarVector1::splat(T::ZERO),
            hp: ScalarVector1::splat(T::ZERO),
            bp: ScalarVector1::splat(T::ZERO),
            x1: ScalarVector1::splat(T::ZERO),
            sample_rate: 44100.0,
        };
        filter.update_coeffs();
        filter
    }

    /// Обновить коэффициенты
    fn update_coeffs(&mut self) {
        // f = 2 * sin(π * cutoff / sample_rate)
        self.f = ScalarVector1::splat(T::from_f32(
            2.0 * (PI * self.params.cutoff / self.sample_rate).sin(),
        ));
        self.q = ScalarVector1::splat(T::from_f32(self.params.q));
    }

    /// Получить low-pass выход
    pub fn lowpass(&self) -> T {
        self.lp.extract(0)
    }

    /// Получить high-pass выход
    pub fn highpass(&self) -> T {
        self.hp.extract(0)
    }

    /// Получить band-pass выход
    pub fn bandpass(&self) -> T {
        self.bp.extract(0)
    }
}

impl<T: AudioNum> Algorithm<T> for StateVariableFilter<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_coeffs();
        self.reset();
    }

    fn reset(&mut self) {
        self.lp = ScalarVector1::splat(T::ZERO);
        self.hp = ScalarVector1::splat(T::ZERO);
        self.bp = ScalarVector1::splat(T::ZERO);
        self.x1 = ScalarVector1::splat(T::ZERO);
    }

    fn process_block(&mut self, input: &[T], output: &mut [T]) {
        let len = input.len().min(output.len());

        for i in 0..len {
            let input_vec = ScalarVector1::splat(input[i]);
            self.lp = self.lp + self.f * self.bp;
            self.hp = input_vec - self.lp - self.q * self.bp;
            self.bp = self.f * self.hp + self.bp;

            output[i] = match self.params.filter_type {
                FilterType::LowPass => self.lp.extract(0),
                FilterType::HighPass => self.hp.extract(0),
                FilterType::BandPass => self.bp.extract(0),
                _ => self.lp.extract(0), // по умолчанию low-pass
            };
        }
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "State Variable Filter",
            category: AlgorithmCategory::Filter,
            description: "SVF with simultaneous LP/HP/BP outputs",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: AudioNum> ParameterizedAlgorithm<T> for StateVariableFilter<T> {
    type Params = FilterParams;

    fn params(&self) -> &Self::Params {
        &self.params
    }

    fn set_params(&mut self, params: Self::Params) {
        self.params = params;
        self.update_coeffs();
    }
}
