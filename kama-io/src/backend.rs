use std::time::Duration;

use crate::config::AudioConfig;
use crate::error::IoResult;

/// Трейт аудио бэкенда (синхронная версия)
pub trait AudioBackend: Send + Sync {
    /// Получить имя бэкенда
    fn name(&self) -> &'static str;
    
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
    
    /// Доступен ли бэкенд на этой платформе
    fn is_available(&self) -> bool;
    
    /// Получить список доступных входных устройств
    fn list_input_devices(&self) -> Vec<String>;
    
    /// Получить список доступных выходных устройств
    fn list_output_devices(&self) -> Vec<String>;
}

/// Тип бэкенда
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    Cpal,
    Alsa,
    PipeWire,
    Jack,
    Null,
}

impl BackendType {
    pub fn name(&self) -> &'static str {
        match self {
            BackendType::Cpal => "CPAL",
            BackendType::Alsa => "ALSA",
            BackendType::PipeWire => "PipeWire",
            BackendType::Jack => "JACK",
            BackendType::Null => "Null",
        }
    }
}

/// Информация об устройстве
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub backend: BackendType,
    pub is_default: bool,
    pub max_input_channels: u32,
    pub max_output_channels: u32,
    pub supported_sample_rates: Vec<u32>,
    pub supported_buffer_sizes: Vec<u32>,
}