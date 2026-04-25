//! Процессоры для AudioEngine
//!
//! Этот модуль содержит различные реализации трейта `AudioProcessor`
//! для обработки аудио в реальном времени.

mod basic;

#[cfg(feature = "graph")]
mod graph;

#[cfg(feature = "examples")]
mod sine;

#[cfg(feature = "examples")]
mod granular;

// Реэкспорты из basic
pub use basic::{
    PassThroughProcessor,
    SilenceProcessor,
    GainProcessor,
    MonoMixerProcessor,
};

#[cfg(feature = "graph")]
pub use graph::GraphProcessor;

#[cfg(feature = "examples")]
pub use sine::SineProcessor;

#[cfg(feature = "examples")]
pub use granular::GranularProcessor;

#[cfg(feature = "examples")]
pub use basic::CaptureProcessor;

/// Трейт для процессоров аудио (реэкспорт из engine для удобства)
pub use crate::engine::AudioProcessor;