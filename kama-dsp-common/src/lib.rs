//! Общие утилиты для DSP-крейтов
//!
//! Предоставляет:
//! - `DspContext` — контекст выполнения с доступом ко времени, параметрам, буферам
//! - Конструкторы функциональных узлов (`stateless_fn_node`, `stateful_fn_node`)
//! - Макросы для упрощения создания эффектов (`effect!`, `filter!`)

#![warn(missing_docs)]

mod context;
mod fn_node;
mod dummy;
mod macros;

pub use context::DspContext;
pub use fn_node::{
    stateless_fn_node,
    stateful_fn_node,
    block_fn_node,
};

// Реэкспорты для удобства
pub use kama_core_traits::{
    AudioNode,
    AudioError,
    NodeCategory,
    NodeMetadata,
    NodeTypeId,
    param::{ParamValue, ParamType, ParamMetadata},
};

#[cfg(feature = "simd")]
pub mod simd;

/// Реэкспорт буферного реестра
pub use kama_buffers::BufferRegistry;

// Экспортируем макросы
pub use crate::macros::*;