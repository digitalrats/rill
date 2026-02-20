//! Буферы для аудиообработки

#![warn(missing_docs)]

mod error;
mod pool;
mod ring;
mod head;
mod multi_head;
mod view;
mod decorator;
mod manager;

// Реэкспорты
pub use error::{BufferError, BufferResult};
pub use pool::{BufferPool, PoolStrategy};
pub use ring::RingBuffer;
pub use head::{BufferHead, HeadState, Direction, ReadMode};
pub use multi_head::MultiHeadBuffer;
pub use view::{BufferView, BufferViewMut};
pub use decorator::{PanningDecorator, LfoDecorator};
pub use manager::{
    BufferManager, BufferManagerStats, NodeBuffers,
    RegisteredBuffer,  // <-- Добавляем
};

// Реэкспортируем NodeId напрямую из kama_core_traits
pub use kama_core_traits::NodeId;

// Re-export из kama-core-traits для удобства
pub use kama_core_traits::AudioNode;

/// Тип аудиобуфера для удобства
pub type AudioBuffer = Vec<f32>;