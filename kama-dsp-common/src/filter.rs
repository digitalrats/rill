//! # Общие трейты и типы для фильтров
//!
//! Предоставляет единый интерфейс для всех типов фильтров в экосистеме Kama Audio.
//!
//! ## Основные компоненты
//!
//! - [`FilterType`] — перечисление всех поддерживаемых типов фильтров
//! - [`Filter`] — общий трейт для всех фильтров
//! - [`FilterFactory`] — фабрика для создания фильтров

//! Common filter traits and types for DSP filters

use kama_core::traits::AudioNode;

/// Type of filter
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterType {
    /// Фильтр нижних частот.
    LowPass,
    /// Фильтр верхних частот.
    HighPass,
    /// Полосовой фильтр.
    BandPass,
    /// Режекторный фильтр (полосно-заграждающий).
    Notch,
    /// Пиковый фильтр (эквалайзер).
    Peak,
    /// Полочный фильтр низких частот.
    LowShelf,
    /// Полочный фильтр высоких частот.
    HighShelf,
    /// Всепропускающий фильтр (фазовращатель).
    AllPass,
}

impl FilterType {
    /// Get filter type from string
    /// Получить тип фильтра из строки.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            /// Фильтр нижних частот.
            "lowpass" | "low_pass" => Some(FilterType::LowPass),
            /// Фильтр верхних частот.
            "highpass" | "high_pass" => Some(FilterType::HighPass),
            /// Полосовой фильтр.
            "bandpass" | "band_pass" => Some(FilterType::BandPass),
            /// Режекторный фильтр (полосно-заграждающий).
            "notch" => Some(FilterType::Notch),
            /// Пиковый фильтр (эквалайзер).
            "peak" => Some(FilterType::Peak),
            /// Полочный фильтр низких частот.
            "lowshelf" | "low_shelf" => Some(FilterType::LowShelf),
            /// Полочный фильтр высоких частот.
            "highshelf" | "high_shelf" => Some(FilterType::HighShelf),
            /// Всепропускающий фильтр (фазовращатель).
            "allpass" | "all_pass" => Some(FilterType::AllPass),
            _ => None,
        }
    }

    /// Get string representation
    /// Получить строковое представление типа фильтра.
    pub fn as_str(&self) -> &'static str {
        match self {
            /// Фильтр нижних частот.
            FilterType::LowPass => "lowpass",
            /// Фильтр верхних частот.
            FilterType::HighPass => "highpass",
            /// Полосовой фильтр.
            FilterType::BandPass => "bandpass",
            /// Режекторный фильтр (полосно-заграждающий).
            FilterType::Notch => "notch",
            /// Пиковый фильтр (эквалайзер).
            FilterType::Peak => "peak",
            /// Полочный фильтр низких частот.
            FilterType::LowShelf => "lowshelf",
            /// Полочный фильтр высоких частот.
            FilterType::HighShelf => "highshelf",
            /// Всепропускающий фильтр (фазовращатель).
            FilterType::AllPass => "allpass",
        }
    }
}

/// Common trait for all filters
/// Общий трейт для всех фильтров.
///
/// Расширяет [`AudioNode`] методами, специфичными для фильтров.
pub trait Filter: AudioNode {
    /// Set cutoff frequency in Hz
    /// Установить частоту среза в Hz.
    fn set_cutoff(&mut self, freq: f32);

    /// Get current cutoff frequency
    /// Получить текущую частоту среза.
    fn cutoff(&self) -> f32;

    /// Set Q factor (resonance)
    /// Установить добротность (Q-фактор).
    fn set_q(&mut self, q: f32);

    /// Get current Q factor
    /// Получить текущую добротность.
    fn q(&self) -> f32;

    /// Set gain in dB (for peak/shelving filters)
    /// Установить усиление в dB (для пиковых и полочных фильтров).
    fn set_gain_db(&mut self, gain: f32);

    /// Get current gain in dB
    /// Получить текущее усиление в dB.
    fn gain_db(&self) -> f32;

    /// Get filter type
    /// Получить тип фильтра.
    fn filter_type(&self) -> FilterType;

    /// Reset filter state
    /// Сбросить внутреннее состояние фильтра.
    fn reset_filter(&mut self);
}

/// Factory for creating filters
/// Общий трейт для всех фильтров.
///
/// Расширяет [`AudioNode`] методами, специфичными для фильтров.
/// Фабрика для создания фильтров.
///
/// Позволяет создавать фильтры одного типа с разными параметрами.
pub trait FilterFactory<F: Filter> {
    /// Create a new filter
    /// Создать новый фильтр с заданными параметрами.
    fn create_filter(&self, filter_type: FilterType, cutoff: f32, q: f32, gain_db: f32) -> F;

    /// Get factory name (for metadata)
    /// Получить имя фабрики (для метаданных).
    fn factory_name(&self) -> &str;
}
