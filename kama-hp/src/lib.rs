//! High-precision audio processing ecosystem (f64)
//! 
//! Для приложений, требующих максимальной точности:
//! - Профессиональные синтезаторы
//! - Мастер-процессоры
//! - Научные исследования
//! - Интеграция с kama-buffers для эффективного управления памятью

#![warn(missing_docs)]

pub mod buffers;
pub mod oscillators;
pub mod filters;
pub mod effects;
pub mod analysis;
pub mod converters;

// Re-export основных типов
pub use buffers::{HighPrecisionBuffer, HighPrecisionBufferPool};  // <-- ДОБАВЛЯЕМ
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
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),
    
    #[error("Buffer error: {0}")]
    Buffer(String),
    
    #[error("Conversion error: {0}")]
    Conversion(String),
    
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