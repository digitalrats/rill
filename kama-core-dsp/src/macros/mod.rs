//! # Макросы для создания DSP-узлов
//!
//! Этот модуль предоставляет специализированные макросы для различных
//! категорий DSP-узлов: генераторы, фильтры, эффекты.
//!
//! ## Доступные макросы
//!
//! - `generator!` - для создания генераторов сигналов
//! - `lfo!` - для создания низкочастотных генераторов
//! - `noise_generator!` - для создания шумовых генераторов
//! - `filter!` - для создания фильтров
//! - `butterworth!` - для создания фильтров Баттерворта
//! - `chebyshev!` - для создания фильтров Чебышева
//! - `effect!` - для создания эффектов
//! - `dry_wet_effect!` - для создания эффектов с dry/wet
//! - `stereo_effect!` - для создания стерео-эффектов
//! - `delay_effect!` - для создания эффектов задержки

// Макросы экспортируются в корень крейта через #[macro_export],
// поэтому мы реэкспортируем их оттуда, а не из модулей
pub use crate::{
    // Генераторы
    generator,
    lfo,
    noise_generator,
    
    // Фильтры
    filter,
    butterworth,
    chebyshev,
    
    // Эффекты
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
/// generator! {
///     // ... используем макросы без дополнительных импортов
/// }
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

// Вспомогательные макросы (не экспортируются наружу)
#[doc(hidden)]
#[macro_export]
macro_rules! __count_control {
    () => { 0 };
    ($control:expr) => { $control };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __extract_doc {
    ($(#[$meta:meta])*) => {
        {
            let mut doc = String::new();
            $(
                if let Some(doc_str) = $crate::__meta_to_doc!($meta) {
                    doc = doc_str;
                }
            )*
            doc
        }
    };
    () => { String::new() };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __meta_to_doc {
    (doc = $s:expr) => { Some($s.to_string()) };
    (doc $s:expr) => { Some($s.to_string()) };
    ($other:tt) => { None };
}