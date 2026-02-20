//! Буферы для аудиообработки
//!
//! Предоставляет:
//! - Кольцевые буферы (`RingBuffer`)
//! - Многоголовые буферы для гранулярного синтеза (`MultiHeadBuffer`)
//! - Пулы буферов для предотвращения аллокаций (`BufferPool`)
//! - Менеджер буферов для графа (`BufferManager`)

#![warn(missing_docs)]

mod error;
mod ring;
mod pool;
mod head;
mod view;
mod processor;
mod decorator;
mod multi_head;
mod manager;

#[cfg(feature = "simd")]
pub mod simd;

// Реэкспорты
pub use error::{BufferError, BufferResult};
pub use ring::RingBuffer;
pub use pool::{BufferPool, BufferPoolError, PoolStrategy};  // Добавляем реэкспорт
pub use head::{BufferHead, HeadState, Direction, ReadMode};
pub use view::BufferView;
pub use decorator::{PanningDecorator, LfoDecorator};
pub use multi_head::MultiHeadBuffer;
pub use manager::{BufferManager, NodeBuffers, BufferManagerStats};

// Re-export из kama-core-traits для удобства
pub use kama_core_traits::AudioNode;

/// Тип аудиобуфера для совместимости
pub type AudioBuffer = Vec<f32>;