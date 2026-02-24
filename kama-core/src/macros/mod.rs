//! Макросы для создания узлов

#[macro_use]
mod simple;
#[macro_use]
mod source;
#[macro_use]
mod processor;
#[macro_use]
mod sink;

// Реэкспорт макросов
pub use source_node_f32;
pub use processor_node_f32;
pub use sink_node_f32;

// Вспомогательный макрос для подсчета control портов
#[doc(hidden)]
#[macro_export]
macro_rules! __count_control {
    () => { 0 };
    ($control:expr) => { $control };
}