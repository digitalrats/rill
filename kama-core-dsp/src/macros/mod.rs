//! # Макросы для создания DSP-узлов
//!
//! Этот модуль предоставляет специализированные макросы для различных
//! категорий DSP-узлов: генераторы, фильтры, эффекты.

#[macro_use]
mod generator;
#[macro_use]
mod filter;
#[macro_use]
mod effect;

// Реэкспортируем макросы для использования в других крейтах
pub use crate::{
    generator,
    lfo,
    noise_generator,
    filter,
    butterworth,
    chebyshev,
    effect,
    dry_wet_effect,
    stereo_effect,
    delay_effect,
};

/// Прелюдия для удобного импорта всех макросов
///
/// # Пример
/// ```
/// use kama_core_dsp::macros::prelude::*;
///
/// // Здесь будут макросы, когда они будут реализованы
/// ```
pub mod prelude {
    pub use super::{
        generator,
        lfo,
        noise_generator,
        filter,
        butterworth,
        chebyshev,
        effect,
        dry_wet_effect,
        stereo_effect,
        delay_effect,
    };
}

// Re-export базовых типов для удобства использования в макросах
pub use crate::math::AudioNum;