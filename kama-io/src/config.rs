use std::time::Duration;

/// Конфигурация аудиоустройства
#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// Частота дискретизации (Гц)
    pub sample_rate: u32,
    
    /// Размер буфера (в семплах)
    pub buffer_size: u32,
    
    /// Количество каналов (общее)
    pub channels: u32,
    
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
    
    /// Дополнительные параметры бэкенда
    pub backend_options: BackendOptions,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            buffer_size: 256,
            channels: 2,
            input_channels: 2,
            output_channels: 2,
            target_latency_ms: 10,
            input_device: None,
            output_device: None,
            backend_options: BackendOptions::default(),
        }
    }
}

impl AudioConfig {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn with_sample_rate(mut self, sample_rate: u32) -> Self {
        self.sample_rate = sample_rate;
        self
    }
    
    pub fn with_buffer_size(mut self, buffer_size: u32) -> Self {
        self.buffer_size = buffer_size;
        self
    }
    
    pub fn with_channels(mut self, channels: u32) -> Self {
        self.channels = channels;
        self.input_channels = channels;
        self.output_channels = channels;
        self
    }
    
    pub fn with_input_device(mut self, device: impl Into<String>) -> Self {
        self.input_device = Some(device.into());
        self
    }
    
    pub fn with_output_device(mut self, device: impl Into<String>) -> Self {
        self.output_device = Some(device.into());
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

/// Опции конкретного бэкенда
#[derive(Debug, Clone, Default)]
pub struct BackendOptions {
    /// Для CPAL
    pub cpal: CpalOptions,
    
    /// Для ALSA
    pub alsa: AlsaOptions,
    
    /// Для PipeWire
    pub pipewire: PipewireOptions,
    
    /// Для JACK
    pub jack: JackOptions,
}

/// Опции CPAL
#[derive(Debug, Clone)]
pub struct CpalOptions {
    /// Использовать хост по умолчанию
    pub use_default_host: bool,
    
    /// Предпочитаемый хост (если не default)
    pub preferred_host: Option<String>,
}

impl Default for CpalOptions {
    fn default() -> Self {
        Self {
            use_default_host: true,
            preferred_host: None,
        }
    }
}

/// Опции ALSA
#[derive(Debug, Clone)]
pub struct AlsaOptions {
    /// Имя устройства PCM (например, "hw:0,0")
    pub pcm_device: Option<String>,
    
    /// Количество периодов
    pub periods: u32,
    
    /// Использовать mmap
    pub use_mmap: bool,
}

impl Default for AlsaOptions {
    fn default() -> Self {
        Self {
            pcm_device: None,
            periods: 4,
            use_mmap: true,
        }
    }
}

/// Опции PipeWire
#[derive(Debug, Clone)]
pub struct PipewireOptions {
    /// Имя приложения
    pub app_name: String,
    
    /// Автоматически подключаться к узлам
    pub auto_connect: bool,
}

impl Default for PipewireOptions {
    fn default() -> Self {
        Self {
            app_name: "Kama Audio".to_string(),
            auto_connect: true,
        }
    }
}

/// Опции JACK
#[derive(Debug, Clone)]
pub struct JackOptions {
    /// Имя клиента
    pub client_name: String,
    
    /// Автоматически подключать порты
    pub auto_connect: bool,
}

impl Default for JackOptions {
    fn default() -> Self {
        Self {
            client_name: "kama_audio".to_string(),
            auto_connect: true,
        }
    }
}