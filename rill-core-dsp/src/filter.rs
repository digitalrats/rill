//! Трейты для фильтров

use rill_core::AudioNum;
use crate::algorithm::{Algorithm, ParameterizedAlgorithm};

/// Тип фильтра
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterType {
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Peak,
    LowShelf,
    HighShelf,
    AllPass,
}

impl FilterType {
    pub const fn as_str(&self) -> &'static str {
        match self {
            FilterType::LowPass => "lowpass",
            FilterType::HighPass => "highpass",
            FilterType::BandPass => "bandpass",
            FilterType::Notch => "notch",
            FilterType::Peak => "peak",
            FilterType::LowShelf => "lowshelf",
            FilterType::HighShelf => "highshelf",
            FilterType::AllPass => "allpass",
        }
    }
}

/// Параметры фильтра
#[derive(Debug, Clone)]
pub struct FilterParams {
    /// Частота среза/центральная частота (Hz)
    pub cutoff: f32,
    /// Добротность (0.1 - 20.0)
    pub q: f32,
    /// Усиление в dB (для peak/shelving)
    pub gain_db: f32,
    /// Тип фильтра
    pub filter_type: FilterType,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            cutoff: 1000.0,
            q: 0.707,
            gain_db: 0.0,
            filter_type: FilterType::LowPass,
        }
    }
}

/// Трейт для фильтров
pub trait Filter<T: AudioNum>: ParameterizedAlgorithm<T, Params = FilterParams> {
    /// Установить частоту среза
    fn set_cutoff(&mut self, cutoff: f32) {
        let mut params = self.params().clone();
        params.cutoff = cutoff;
        self.set_params(params);
    }
    
    /// Получить частоту среза
    fn cutoff(&self) -> f32 {
        self.params().cutoff
    }
    
    /// Установить добротность
    fn set_q(&mut self, q: f32) {
        let mut params = self.params().clone();
        params.q = q;
        self.set_params(params);
    }
    
    /// Получить добротность
    fn q(&self) -> f32 {
        self.params().q
    }
    
    /// Установить усиление
    fn set_gain_db(&mut self, gain: f32) {
        let mut params = self.params().clone();
        params.gain_db = gain;
        self.set_params(params);
    }
    
    /// Получить усиление
    fn gain_db(&self) -> f32 {
        self.params().gain_db
    }
    
    /// Получить тип фильтра
    fn filter_type(&self) -> FilterType {
        self.params().filter_type
    }
    
    /// Рассчитать коэффициенты фильтра (если нужно)
    fn update_coefficients(&mut self) {
        // Переопределяется в конкретных фильтрах
    }
}

/// Фильтр с автоматической регулировкой (адаптивный)
pub trait AdaptiveFilter<T: AudioNum>: Filter<T> {
    /// Адаптироваться к сигналу
    fn adapt(&mut self, reference: &[T], error: &[T]);
    
    /// Скорость адаптации
    fn adaptation_rate(&self) -> f32;
    
    /// Установить скорость адаптации
    fn set_adaptation_rate(&mut self, rate: f32);
}