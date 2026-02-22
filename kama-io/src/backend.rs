//! Трейт аудио бэкенда и связанные типы

use std::time::Duration;
use std::fmt::Debug;
use crate::config::AudioConfig;
use crate::error::IoResult;

/// Тип бэкенда
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde-config", derive(serde::Serialize, serde::Deserialize))]
pub enum BackendType {
    /// CPAL (кросс-платформенный)
    Cpal,
    /// ALSA (Linux)
    Alsa,
    /// PipeWire (Linux)
    PipeWire,
    /// JACK (Linux/macOS)
    Jack,
    /// Null (тестирование)
    Null,
}

impl BackendType {
    /// Получить имя бэкенда
    pub fn name(&self) -> &'static str {
        match self {
            BackendType::Cpal => "CPAL",
            BackendType::Alsa => "ALSA",
            BackendType::PipeWire => "PipeWire",
            BackendType::Jack => "JACK",
            BackendType::Null => "Null",
        }
    }
    
    /// Доступен ли бэкенд на текущей платформе
    pub fn is_available(&self) -> bool {
        match self {
            BackendType::Cpal => true,
            BackendType::Alsa => cfg!(target_os = "linux"),
            BackendType::PipeWire => cfg!(target_os = "linux"),
            BackendType::Jack => cfg!(any(target_os = "linux", target_os = "macos")),
            BackendType::Null => true,
        }
    }
}

/// Трейт аудио бэкенда
pub trait AudioBackend: Send + Sync + Debug {
    /// Получить тип бэкенда
    fn backend_type(&self) -> BackendType;
    
    /// Получить конфигурацию
    fn config(&self) -> &AudioConfig;
    
    /// Получить мутабельную конфигурацию
    fn config_mut(&mut self) -> &mut AudioConfig;
    
    /// Инициализировать бэкенд
    fn init(&mut self) -> IoResult<()>;
    
    /// Запустить обработку
    fn start(&mut self) -> IoResult<()>;
    
    /// Остановить обработку
    fn stop(&mut self) -> IoResult<()>;
    
    /// Прочитать данные из входного потока
    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize>;
    
    /// Записать данные в выходной поток
    fn write(&mut self, buffer: &[f32]) -> IoResult<usize>;
    
    /// Количество пропущенных семплов (xruns)
    fn xruns(&self) -> u32;
    
    /// Текущая задержка
    fn latency(&self) -> Duration;
    
    /// Получить список доступных входных устройств
    fn list_input_devices(&self) -> Vec<String>;
    
    /// Получить список доступных выходных устройств
    fn list_output_devices(&self) -> Vec<String>;
}

/// Информация об устройстве
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Имя устройства
    pub name: String,
    /// Тип бэкенда
    pub backend: BackendType,
    /// Является ли устройством по умолчанию
    pub is_default: bool,
    /// Максимальное количество входных каналов
    pub max_input_channels: u32,
    /// Максимальное количество выходных каналов
    pub max_output_channels: u32,
    /// Поддерживаемые частоты дискретизации
    pub supported_sample_rates: Vec<u32>,
    /// Поддерживаемые размеры буфера
    pub supported_buffer_sizes: Vec<u32>,
}