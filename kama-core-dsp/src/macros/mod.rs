//! # Макросы для создания DSP-алгоритмов
//!
//! Этот модуль предоставляет трейты для DSP алгоритмов.
//! Для создания узлов используйте макросы из kama-core.

mod algorithm;

pub use algorithm::{Algorithm, Filter, Effect, Generator};

/// Прелюдия для удобного импорта
pub mod prelude {
    pub use super::{Algorithm, Filter, Effect, Generator};
}