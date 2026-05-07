//! # Макросы для создания узлов и работы с ядром
//!
//! Этот модуль предоставляет макросы для упрощения создания
//! различных типов узлов в Rill.

#[macro_use]
mod params;
#[macro_use]
mod ports;

#[macro_use]
mod source;
#[macro_use]
mod processor;
#[macro_use]
mod sink;

mod tests;

// Реэкспорт макросов с верхнего уровня
pub use crate::{node, processor_node, sink_node, source_node, with_parameters};

/// Прелюдия для удобного импорта всех макросов
pub mod prelude {
    pub use crate::{node, processor_node, sink_node, source_node, with_parameters};
}
