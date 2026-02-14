//! Audio I/O backends for Kama Audio
//! 
//! Этот крейт предоставляет унифицированный интерфейс для различных
//! аудио бэкендов с использованием кольцевых буферов из kama-buffers.

#![warn(missing_docs)]

mod backend;
mod config;
mod error;
mod engine;

// Модули
pub mod backends;
pub mod processor;

// Реэкспорты из модулей
pub use backend::{AudioBackend, BackendType};
pub use config::{AudioConfig, BackendOptions, 
                 CpalOptions, AlsaOptions, PipewireOptions, JackOptions};
pub use error::{IoError, IoResult};
pub use engine::{AudioEngine, AudioProcessor, EngineState};

// Реэкспорты бэкендов
pub use backends::NullBackend;

#[cfg(feature = "cpal")]
pub use backends::CpalBackend;

#[cfg(feature = "alsa")]
pub use backends::AlsaBackend;

#[cfg(feature = "pipewire")]
pub use backends::PipewireBackend;

#[cfg(feature = "jack")]
pub use backends::JackBackend;

// Реэкспорты процессоров
pub use processor::{
    PassThroughProcessor,
    SilenceProcessor,
    GainProcessor,
    MonoMixerProcessor,
    GraphProcessor,
    SineProcessor,
};

#[cfg(feature = "granular")]
pub use processor::GranularProcessor;

#[cfg(feature = "debug")]
pub use processor::CaptureProcessor;

pub use factory::BackendFactory;

/// Фабрика для создания бэкендов
pub mod factory {
    use super::*;
    use crate::backends;
    
    pub struct BackendFactory;
    
    impl BackendFactory {
        /// Создать бэкенд по умолчанию для текущей платформы
        /// 
        /// На Linux: ALSA (если доступен), иначе CPAL
        /// На других платформах: CPAL
        pub fn create_default(config: AudioConfig) -> IoResult<Box<dyn AudioBackend>> {
            #[cfg(all(target_os = "linux", feature = "alsa"))]
            {
                // Пробуем ALSA
                if let Ok(backend) = backends::AlsaBackend::new(config.clone()) {
                    return Ok(Box::new(backend));
                }
            }
            
            #[cfg(feature = "cpal")]
            {
                // Пробуем CPAL как запасной вариант
                if let Ok(backend) = backends::CpalBackend::new(config.clone()) {
                    return Ok(Box::new(backend));
                }
            }
            
            // Если ничего не работает, используем Null бэкенд
            Ok(Box::new(backends::NullBackend::new(config)))
        }
        
        /// Создать бэкенд указанного типа
        pub fn create(backend_type: BackendType, config: AudioConfig) -> IoResult<Box<dyn AudioBackend>> {
            match backend_type {
                #[cfg(feature = "alsa")]
                BackendType::Alsa => Ok(Box::new(backends::AlsaBackend::new(config)?)),
                
                #[cfg(feature = "cpal")]
                BackendType::Cpal => Ok(Box::new(backends::CpalBackend::new(config)?)),
                
                #[cfg(feature = "pipewire")]
                BackendType::PipeWire => Ok(Box::new(backends::PipewireBackend::new(config)?)),
                
                #[cfg(feature = "jack")]
                BackendType::Jack => Ok(Box::new(backends::JackBackend::new(config)?)),
                
                BackendType::Null => Ok(Box::new(backends::NullBackend::new(config))),
                
                _ => Err(IoError::Unsupported(format!("Backend {:?} not available", backend_type))),
            }
        }
        
        /// Получить список доступных бэкендов
        pub fn available_backends() -> Vec<BackendType> {
            let mut backends = Vec::new();
            
            #[cfg(feature = "alsa")]
            if cfg!(target_os = "linux") {
                backends.push(BackendType::Alsa);
            }
            
            #[cfg(feature = "cpal")]
            backends.push(BackendType::Cpal);
            
            #[cfg(feature = "pipewire")]
            if cfg!(target_os = "linux") {
                backends.push(BackendType::PipeWire);
            }
            
            #[cfg(feature = "jack")]
            backends.push(BackendType::Jack);
            
            backends.push(BackendType::Null);
            
            backends
        }
        
        /// Получить рекомендуемый бэкенд для текущей платформы
        pub fn recommended_backend() -> BackendType {
            #[cfg(target_os = "linux")]
            {
                BackendType::Alsa
            }
            
            #[cfg(not(target_os = "linux"))]
            {
                BackendType::Cpal
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::processor::{GainProcessor, MonoMixerProcessor};
    
    #[test]
    fn test_config_default() {
        let config = AudioConfig::default();
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.buffer_size, 256);
    }
    
    #[test]
    fn test_factory_available_backends() {
        let backends = factory::BackendFactory::available_backends();
        assert!(!backends.is_empty());
        
        // На Linux ALSA должен быть доступен, на других платформах - нет
        #[cfg(target_os = "linux")]
        {
            // Проверяем, что ALSA есть в списке (если фича включена)
            #[cfg(feature = "alsa")]
            assert!(backends.contains(&BackendType::Alsa));
            
            // Проверяем, что Null бэкенд всегда есть
            assert!(backends.contains(&BackendType::Null));
        }
        
        #[cfg(not(target_os = "linux"))]
        {
            // На не-Linux платформах ALSA не должен быть в списке
            assert!(!backends.contains(&BackendType::Alsa));
            assert!(backends.contains(&BackendType::Null));
        }
    }
    
    #[test]
    fn test_basic_processors() {
        let input = vec![0.5f32; 10];
        let mut output = vec![0.0f32; 10];
        
        let mut pass = PassThroughProcessor;
        pass.process(&input, &mut output);
        assert_eq!(input, output);
        
        let mut silence = SilenceProcessor;
        silence.process(&input, &mut output);
        assert_eq!(output, vec![0.0f32; 10]);
        
        let mut gain = GainProcessor::new(2.0);
        gain.process(&input, &mut output);
        assert_eq!(output, vec![1.0f32; 10]);
        
        // Тест моно микшера
        let stereo_input = vec![0.5, 0.3, 0.5, 0.3];
        let mut mono_output = vec![0.0; 2];
        let mut mixer = MonoMixerProcessor;
        mixer.process(&stereo_input, &mut mono_output);
        assert_eq!(mono_output, vec![0.4, 0.4]);
    }
}