//! # Макросы для создания узлов и работы с ядром
//!
//! Этот модуль предоставляет макросы для упрощения создания
//! различных типов узлов в Kama Audio.

// Макросы экспортируются на верхний уровень crate
#[macro_use]
mod source;
#[macro_use]
mod processor;
#[macro_use]
mod sink;
#[macro_use]
mod params;
#[macro_use]
mod ports;

mod tests;

// Реэкспорт макросов с верхнего уровня
pub use crate::{
    source_node,
    processor_node,
    sink_node,
    audio_node,
    with_parameters,
    with_ports,
};

/// Прелюдия для удобного импорта всех макросов
pub mod prelude {
    pub use crate::{
        source_node,
        processor_node,
        sink_node,
        audio_node,
        with_parameters,
        with_ports,
    };
}