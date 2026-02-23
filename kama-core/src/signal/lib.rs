//! # Kama Core
//!
//! Ядро экосистемы Kama Audio. Содержит:
//!
//! - **traits**: базовые трейты (`AudioNode`, `ParamValue`, `Clock`, ...)
//! - **signal**: сигнальная система для коммуникации между компонентами

#![warn(missing_docs)]

pub mod traits;
pub mod signal;

/// Прелюдия для удобного импорта основных типов
pub mod prelude {
    pub use crate::traits::prelude::*;
    pub use crate::signal::prelude::*;
}