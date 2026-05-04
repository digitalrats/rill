//! Конфигурация аудиоустройства

use crate::backend::BackendType;
use crate::midi::MidiEvent;
use std::sync::mpsc::Sender;

/// Конфигурация аудиоустройства
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-config", derive(serde::Serialize, serde::Deserialize))]
pub struct AudioConfig {
    /// Частота дискретизации (Гц)
    pub sample_rate: u32,

    /// Размер буфера (в семплах)
    pub buffer_size: u32,

    /// Количество входных каналов
    pub input_channels: u32,

    /// Количество выходных каналов
    pub output_channels: u32,

    /// Желаемая задержка (мс)
    pub target_latency_ms: u32,

    /// Имя входного устройства (если None - используется дефолтное)
    pub input_device: Option<String>,

    /// Имя выходного устройства (если None - используется дефолтное)
    pub output_device: Option<String>,

    /// Тип бэкенда
    pub backend_type: BackendType,

    /// Включить MIDI-вход через PipeWire
    #[cfg_attr(feature = "serde-config", serde(default))]
    pub midi_input: bool,

    /// Канал для отправки MIDI-событий из бэкенда в приложение.
    /// Если `None`, MIDI-события не обрабатываются.
    #[cfg_attr(feature = "serde-config", serde(skip))]
    pub midi_event_tx: Option<Sender<MidiEvent>>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            buffer_size: 256,
            input_channels: 2,
            output_channels: 2,
            target_latency_ms: 10,
            input_device: None,
            output_device: None,
            backend_type: BackendType::Cpal,
            midi_input: false,
            midi_event_tx: None,
        }
    }
}

impl AudioConfig {
    /// Создать новую конфигурацию
    pub fn new() -> Self {
        Self::default()
    }

    /// Установить частоту дискретизации
    pub fn with_sample_rate(mut self, sample_rate: u32) -> Self {
        self.sample_rate = sample_rate;
        self
    }

    /// Установить размер буфера
    pub fn with_buffer_size(mut self, buffer_size: u32) -> Self {
        self.buffer_size = buffer_size;
        self
    }

    /// Установить количество каналов (одинаково для входа и выхода)
    pub fn with_channels(mut self, channels: u32) -> Self {
        self.input_channels = channels;
        self.output_channels = channels;
        self
    }

    /// Установить входное устройство
    pub fn with_input_device(mut self, device: impl Into<String>) -> Self {
        self.input_device = Some(device.into());
        self
    }

    /// Установить выходное устройство
    pub fn with_output_device(mut self, device: impl Into<String>) -> Self {
        self.output_device = Some(device.into());
        self
    }

    /// Установить тип бэкенда
    pub fn with_backend(mut self, backend: BackendType) -> Self {
        self.backend_type = backend;
        self
    }

    /// Включить MIDI-вход
    pub fn with_midi_input(mut self, enabled: bool) -> Self {
        self.midi_input = enabled;
        self
    }

    /// Установить канал для MIDI-событий
    pub fn with_midi_tx(mut self, tx: Sender<MidiEvent>) -> Self {
        self.midi_event_tx = Some(tx);
        self
    }

    /// Рассчитать реальную задержку в секундах
    pub fn latency_seconds(&self) -> f64 {
        self.buffer_size as f64 / self.sample_rate as f64
    }

    /// Рассчитать реальную задержку в миллисекундах
    pub fn latency_ms(&self) -> f64 {
        self.latency_seconds() * 1000.0
    }
}
