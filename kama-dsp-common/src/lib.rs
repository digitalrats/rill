//! Общие утилиты для DSP-крейтов
//!
//! Предоставляет:
//! - `DspContext` — контекст выполнения DSP-узлов
//! - Конструкторы функциональных узлов (`stateless_fn_node`, `stateful_fn_node`)
//! - Макросы для упрощения создания эффектов

#![warn(missing_docs)]

mod context;
mod fn_node;
mod dummy;
mod macros;  // Добавляем модуль macros

pub use context::DspContext;
pub use fn_node::{stateless_fn_node, stateful_fn_node, block_fn_node};

// Реэкспорты для удобства
pub use kama_core_traits::{
    AudioNode, AudioError, NodeCategory, NodeMetadata, NodeTypeId,
    param::{ParamValue, ParamType, ParamMetadata},
};

// Реэкспортируем макросы
pub use crate::macros::*;