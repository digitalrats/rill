//! Фильтр переменных состояния (State Variable Filter)
//!
//! Преимущества:
//! - Одновременный low-pass, high-pass и band-pass выходы
//! - Стабилен при высоких резонансах
//! - Идеален для аналоговой эмуляции

use crate::math::AudioNum;
use super::{Filter, FilterParams, FilterType};
use crate::algorithm::{Algorithm, ParameterizedAlgorithm, AlgorithmMetadata, AlgorithmCategory};
use core::f32::consts::PI;

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
    f: T,      // частота
    q: T,      // резонанс
    /// Состояние
    lp: T,     // low-pass выход
    hp: T,     // high-pass выход
    bp: T,     // band-pass выход
    /// Предыдущий вход (для задержки)
    x1: T,
    /// Частота дискретизации
    sample_rate: f32,
}

impl<T: AudioNum> StateVariableFilter<T> {
    /// Создать новый SVF
    pub fn new(params: FilterParams) -> Self {
        let mut filter = Self {
            params,
            f: T::ZERO,
            q: T::ZERO,
            lp: T::ZERO,
            hp: T::ZERO,
            bp: T::ZERO,
            x1: T::ZERO,
            sample_rate: 44100.0,
        };
        filter.update_coeffs();
        filter
    }
    
    /// Обновить коэффициенты
    fn update_coeffs(&mut self) {
        // f = 2 * sin(π * cutoff / sample_rate)
        self.f = T::from_f32(2.0 * (PI * self.params.cutoff / self.sample_rate).sin());
        self.q = T::from_f32(self.params.q);
    }
    
    /// Получить low-pass выход
    pub fn lowpass(&self) -> T {
        self.lp
    }
    
    /// Получить high-pass выход
    pub fn highpass(&self) -> T {
        self.hp
    }
    
    /// Получить band-pass выход
    pub fn bandpass(&self) -> T {
        self.bp
    }
}

impl<T: AudioNum> Algorithm<T> for StateVariableFilter<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_coeffs();
        self.reset();
    }
    
    fn reset(&mut self) {
        self.lp = T::ZERO;
        self.hp = T::ZERO;
        self.bp = T::ZERO;
        self.x1 = T::ZERO;
    }
    
    fn process_sample(&mut self, input: T) -> T {
        // SVF алгоритм (Chamberlin)
        // lp = lp + f * bp
        // hp = input - lp - q * bp
        // bp = f * hp + bp
        
        self.lp = self.lp.add(self.f.mul(self.bp));
        self.hp = input.sub(self.lp).sub(self.q.mul(self.bp));
        self.bp = self.f.mul(self.hp).add(self.bp);
        
        match self.params.filter_type {
            FilterType::LowPass => self.lp,
            FilterType::HighPass => self.hp,
            FilterType::BandPass => self.bp,
            _ => self.lp, // по умолчанию low-pass
        }
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "State Variable Filter",
            category: AlgorithmCategory::Filter,
            description: "SVF with simultaneous LP/HP/BP outputs",
            author: "Kama Audio",
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
