//! # Макросы для создания DSP алгоритмов
//!
//! Этот модуль предоставляет макросы для удобного создания DSP алгоритмов,
//! реализующих трейты из `crate::algorithm` и использующих `AudioNum` из `kama_core`.
//!
//! ## Доступные макросы
//!
//! - [`simple_algorithm!`] - для простых алгоритмов без параметров
//! - [`parameterized_algorithm!`] - для алгоритмов с параметрами
//! - [`filter_algorithm!`] - для фильтров (с коэффициентами)
//! - [`effect_algorithm!`] - для эффектов (с dry/wet)
//! - [`generator_algorithm!`] - для генераторов
//!
//! ## Пример
//!
//! ```
//! use kama_core_dsp::simple_algorithm;
//! use kama_core::math::AudioNum;
//!
//! simple_algorithm! {
//!     /// Простой усилитель
//!     #[derive(Debug, Clone, Copy)]
//!     pub struct Gain<T: AudioNum> {
//!         params: {
//!             /// Коэффициент усиления
//!             gain: T = T::from_f32(1.0),
//!         },
//!         state: {
//!             /// Последнее значение (для статистики)
//!             last_output: T = T::ZERO,
//!         },
//!         process: |this, input| {
//!             let output = input * this.gain;
//!             this.last_output = output;
//!             output
//!         }
//!     }
//! }
//! ```

#[macro_use]
mod algorithm;