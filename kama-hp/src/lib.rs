//! # High-precision audio processing (f64)
//! 
//! Этот крейт предоставляет компоненты для высокоточной обработки аудио с использованием `f64`.
//! Предназначен для приложений, где критична точность вычислений:
//! 
//! - Профессиональные синтезаторы
//! - Мастер-процессоры (лимитеры, компрессоры)
//! - Научные исследования
//! - Интеграция с [`kama-buffers`] для эффективного управления памятью
//! 
//! ## Основные компоненты
//! 
//! - **Буферы** — [`HighPrecisionBuffer`] для работы с f64 данными, интеграция с `kama-buffers`
//! - **Осцилляторы** — [`HighPrecisionSineOsc`], [`HighPrecisionFMOsc`] для генерации сигналов
//! - **Фильтры** — [`HighPrecisionBiquad`], [`HighPrecisionLadderFilter`] для обработки
//! - **Эффекты** — [`NoiseShaper`] для понижения разрядности с минимальными потерями
//! - **Анализ** — [`SpectrumAnalyzer`], [`PeakDetector`] для анализа сигналов
//! - **Конвертеры** — [`Oversampler`], [`PrecisionConverter`] для изменения частоты и разрядности
//! 
//! ## Пример использования
//! 
//! ```no_run
//! use kama_hp::{HighPrecisionBuffer, HighPrecisionBiquad};
//! 
//! // Создаём буфер и фильтр
//! let mut buffer = HighPrecisionBuffer::new(1024, 2, 48000.0);
//! let mut filter = HighPrecisionBiquad::new_lowpass(1000.0, 0.707, 48000.0);
//! 
//! // Обрабатываем сигнал
//! for i in 0..1024 {
//!     let sample = buffer.read(i, 0);
//!     let filtered = filter.process(sample);
//!     buffer.write(i, 0, filtered);
//! }
//! ```
//! 
//! [`HighPrecisionBuffer`]: crate::buffers::HighPrecisionBuffer
//! [`HighPrecisionSineOsc`]: crate::oscillators::HighPrecisionSineOsc
//! [`HighPrecisionFMOsc`]: crate::oscillators::HighPrecisionFMOsc
//! [`HighPrecisionBiquad`]: crate::filters::HighPrecisionBiquad
//! [`HighPrecisionLadderFilter`]: crate::filters::HighPrecisionLadderFilter
//! [`NoiseShaper`]: crate::effects::NoiseShaper
//! [`SpectrumAnalyzer`]: crate::analysis::SpectrumAnalyzer
//! [`PeakDetector`]: crate::analysis::PeakDetector
//! [`Oversampler`]: crate::converters::Oversampler
//! [`PrecisionConverter`]: crate::converters::PrecisionConverter

#![warn(missing_docs)]

pub mod buffers;
pub mod oscillators;
pub mod filters;
pub mod effects;
pub mod analysis;
pub mod converters;

// Re-export основных типов
pub use buffers::{HighPrecisionBuffer, HighPrecisionBufferPool};
pub use oscillators::{HighPrecisionSineOsc, HighPrecisionFMOsc};
pub use filters::{HighPrecisionBiquad, HighPrecisionLadderFilter, BiquadType};
pub use effects::NoiseShaper;
pub use converters::{Oversampler, PrecisionConverter, DitherType};

use kama_core_traits::AudioError;

/// Результат операций high-precision
pub type HpResult<T> = Result<T, HpError>;

/// Ошибки high-precision обработки
#[derive(Debug, thiserror::Error)]
pub enum HpError {
    /// Неверный параметр
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),
    
    /// Ошибка буфера
    #[error("Buffer error: {0}")]
    Buffer(String),
    
    /// Ошибка конвертации
    #[error("Conversion error: {0}")]
    Conversion(String),
    
    /// Ошибка аудио ядра
    #[error("Audio core error: {0}")]
    Core(#[from] AudioError),
}

/// Конфигурация high-precision движка
#[derive(Debug, Clone)]
pub struct HighPrecisionEngine {
    sample_rate: f64,
    buffer_size: usize,
    oversampling_factor: usize,
    dither_enabled: bool,
}

impl HighPrecisionEngine {
    /// Создаёт новый движок с заданной частотой дискретизации и размером буфера.
    pub fn new(sample_rate: f64, buffer_size: usize) -> Self {
        Self {
            sample_rate,
            buffer_size,
            oversampling_factor: 1,
            dither_enabled: true,
        }
    }
    
    /// Устанавливает коэффициент oversampling'а.
    pub fn with_oversampling(mut self, factor: usize) -> Self {
        self.oversampling_factor = factor;
        self
    }
    
    /// Включает/выключает dither при понижении разрядности.
    pub fn with_dither(mut self, enabled: bool) -> Self {
        self.dither_enabled = enabled;
        self
    }
    
    /// Создаёт высокоточный буфер с учётом настроек движка.
    pub fn create_buffer(&self, channels: usize) -> HighPrecisionBuffer {
        HighPrecisionBuffer::new(
            self.buffer_size * self.oversampling_factor,
            channels,
            self.sample_rate * self.oversampling_factor as f64,
        )
    }
}