//! # Векторные операции для DSP
//!
//! Этот модуль реэкспортирует векторную инфраструктуру из `rill-core`.
//! Смотрите [rill_core::vector] для документации.

/// Prelude для удобного импорта
pub mod prelude {
    pub use rill_core::vector::prelude::*;
}

pub use rill_core::vector::*;
