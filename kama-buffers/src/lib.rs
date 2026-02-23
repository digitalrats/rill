//! # Буферы для аудиообработки
//!
//! Этот крейт предоставляет различные типы буферов и утилиты для работы с ними:
//!
//! - **Кольцевые буферы** ([`RingBuffer`]) — для задержек и циклического чтения/записи
//! - **Многоголовые буферы** ([`MultiHeadBuffer`]) — для гранулярного синтеза и сложного воспроизведения
//! - **Пул буферов** ([`BufferManager`]) — для эффективного переиспользования памяти
//! - **Головки воспроизведения** ([`BufferHead`]) — независимые позиции чтения
//! - **Представления** ([`BufferView`], [`BufferViewMut`]) — безопасный доступ к данным
//!
//! ## Основные компоненты
//!
//! - [`RingBuffer`] — кольцевой буфер с фиксированным размером
//! - [`MultiHeadBuffer`] — буфер с множеством головок для сложного воспроизведения
//! - [`BufferManager`] — централизованное управление буферами и пулинг
//! - [`BufferHead`] — головка воспроизведения с поддержкой разных режимов
//! - [`PooledBuffer`] — умный указатель на буфер из пула
//!
//! [`RingBuffer`]: crate::ring::RingBuffer
//! [`MultiHeadBuffer`]: crate::multi_head::MultiHeadBuffer
//! [`BufferManager`]: crate::manager::BufferManager
//! [`BufferHead`]: crate::head::BufferHead
//! [`PooledBuffer`]: crate::manager::PooledBuffer
//! [`BufferView`]: crate::view::BufferView
//! [`BufferViewMut`]: crate::view::BufferViewMut

//! Буферы для аудиообработки

#![warn(missing_docs)]

mod decorator;
mod error;
mod head;
mod manager;
mod multi_head;
mod pool;
mod ring;
mod view;

// Реэкспорты
pub use decorator::{LfoDecorator, PanningDecorator};
pub use error::{BufferError, BufferResult};
pub use head::{BufferHead, Direction, HeadState, ReadMode};
pub use manager::{BufferManager, BufferManagerStats, NodeBuffers, PooledBuffer, RegisteredBuffer};
pub use multi_head::MultiHeadBuffer;
pub use pool::{BufferPool, PoolStrategy};
pub use ring::RingBuffer;
pub use view::{BufferIterator, BufferView, BufferViewMut}; // добавили BufferIterator

// Реэкспортируем NodeId напрямую из kama_core_traits
pub use kama_core_traits::NodeId;

// Re-export из kama-core-traits для удобства
pub use kama_core_traits::AudioNode;

/// Тип аудиобуфера для удобства
pub type AudioBuffer = Vec<f32>;
