//! Процессоры для AudioEngine
//!
//! Этот модуль содержит различные реализации трейта `AudioProcessor`
//! для обработки аудио в реальном времени.

mod basic;
mod graph;

#[cfg(feature = "examples")]
mod sine;

#[cfg(feature = "granular")]
mod granular;

// Реэкспорты из basic
pub use basic::{
    PassThroughProcessor,
    SilenceProcessor,
    GainProcessor,        // <-- Добавлено
    MonoMixerProcessor,   // <-- Добавлено
};

// Реэкспорт graph
pub use graph::GraphProcessor;

#[cfg(feature = "examples")]
pub use sine::SineProcessor;

#[cfg(feature = "granular")]
pub use granular::GranularProcessor;

/// Трейт для процессоров аудио (реэкспорт из engine для удобства)
pub use crate::engine::AudioProcessor;