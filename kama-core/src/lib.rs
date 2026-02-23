//! # Kama Core
//!
//! Ядро экосистемы Kama Audio. Содержит:
//!
//! - **traits**: базовые трейты (`AudioNode`, `ParamValue`, `Clock`, ...)
//! - **signal**: сигнальная система (будет добавлена позже)

#![warn(missing_docs)]
#![warn(rustdoc::missing_doc_code_examples)]

pub mod traits;
pub mod signal;

/// Прелюдия для удобного импорта основных типов
pub mod prelude {
    pub use crate::traits::prelude::*;
}