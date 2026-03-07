//! # Конфигурация
//!
//! Типы для конфигурации компонентов системы.

/// Режим работы
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Режим реального времени (максимальная производительность)
    Realtime,
    /// Режим низкой задержки (для live-coding)
    LowLatency,
    /// Экономичный режим (меньше CPU)
    Eco,
    /// Отладочный режим (проверки, логи)
    Debug,
}

/// Приоритет потока
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadPriority {
    /// Низкий (background)
    Low,
    /// Нормальный
    Normal,
    /// Высокий
    High,
    /// Максимальный (для аудиопотока)
    Realtime,
    /// Пользовательский
    Custom(i32),
}

/// Конфигурация аудио
#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// Частота дискретизации
    pub sample_rate: u32,
    /// Размер буфера
    pub buffer_size: usize,
    /// Количество каналов
    pub channels: u16,
    /// Режим работы
    pub mode: Mode,
    /// Приоритет потока
    pub thread_priority: ThreadPriority,
    /// Имя устройства (опционально)
    pub device_name: Option<String>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            buffer_size: 256,
            channels: 2,
            mode: Mode::Realtime,
            thread_priority: ThreadPriority::Realtime,
            device_name: None,
        }
    }
}

impl AudioConfig {
    /// Создать новую конфигурацию
    pub fn new(sample_rate: u32, buffer_size: usize) -> Self {
        Self {
            sample_rate,
            buffer_size,
            ..Default::default()
        }
    }
    
    /// Установить количество каналов
    pub fn with_channels(mut self, channels: u16) -> Self {
        self.channels = channels;
        self
    }
    
    /// Установить режим
    pub fn with_mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }
    
    /// Установить приоритет
    pub fn with_priority(mut self, priority: ThreadPriority) -> Self {
        self.thread_priority = priority;
        self
    }
    
    /// Установить имя устройства
    pub fn with_device(mut self, name: impl Into<String>) -> Self {
        self.device_name = Some(name.into());
        self
    }
    
    /// Получить задержку в секундах
    pub fn latency_seconds(&self) -> f64 {
        self.buffer_size as f64 / self.sample_rate as f64
    }
    
    /// Получить задержку в миллисекундах
    pub fn latency_ms(&self) -> f64 {
        self.latency_seconds() * 1000.0
    }
}

/// Конфигурация очереди
#[derive(Debug, Clone)]
pub struct QueueConfig {
    /// Размер очереди
    pub size: usize,
    /// Режим переполнения
    pub overflow_policy: queue::OverflowPolicy,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            size: 1024,
            overflow_policy: queue::OverflowPolicy::OverwriteOldest,
        }
    }
}