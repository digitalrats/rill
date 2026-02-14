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
pub mod processor;  // <-- НОВЫЙ МОДУЛЬ

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
};

#[cfg(feature = "examples")]
pub use processor::SineProcessor;

#[cfg(feature = "granular")]
pub use processor::GranularProcessor;

pub use factory::BackendFactory;

/// Фабрика для создания бэкендов
pub mod factory {
    use super::*;
    use crate::backends;
    
    pub struct BackendFactory;
    
    impl BackendFactory {
        #[cfg(feature = "cpal")]
        pub fn create_default(config: AudioConfig) -> IoResult<impl AudioBackend> {
            Ok(backends::CpalBackend::new(config)?)
        }
        
        pub fn create(backend_type: BackendType, config: AudioConfig) -> IoResult<Box<dyn AudioBackend>> {
            match backend_type {
                #[cfg(feature = "cpal")]
                BackendType::Cpal => Ok(Box::new(backends::CpalBackend::new(config)?)),
                
                #[cfg(feature = "alsa")]
                BackendType::Alsa => Ok(Box::new(backends::AlsaBackend::new(config)?)),
                
                #[cfg(feature = "pipewire")]
                BackendType::PipeWire => Ok(Box::new(backends::PipewireBackend::new(config)?)),
                
                #[cfg(feature = "jack")]
                BackendType::Jack => Ok(Box::new(backends::JackBackend::new(config)?)),
                
                BackendType::Null => Ok(Box::new(backends::NullBackend::new(config))),
                
                _ => Err(IoError::Unsupported(format!("Backend {:?} not available", backend_type))),
            }
        }
        
        pub fn available_backends() -> Vec<BackendType> {
            let mut backends = vec![BackendType::Null];
            
            #[cfg(feature = "cpal")]
            backends.push(BackendType::Cpal);
            
            #[cfg(feature = "alsa")]
            if cfg!(target_os = "linux") {
                backends.push(BackendType::Alsa);
            }
            
            #[cfg(feature = "pipewire")]
            if cfg!(target_os = "linux") {
                backends.push(BackendType::PipeWire);
            }
            
            #[cfg(feature = "jack")]
            backends.push(BackendType::Jack);
            
            backends
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
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
        assert!(backends.contains(&BackendType::Null));
    }
    
    #[test]
    fn test_basic_processors() {
        let mut input = vec![0.5f32; 10];
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
    }
}#[cfg(test)]
mod tests {
    use super::*;
    use crate::processor::{GainProcessor, MonoMixerProcessor};  // <-- Добавляем импорты
    
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
        assert!(backends.contains(&BackendType::Null));
    }
    
    #[test]
    fn test_basic_processors() {
        let mut input = vec![0.5f32; 10];
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
        let mut stereo_input = vec![0.5, 0.3, 0.5, 0.3]; // L,R,L,R
        let mut mono_output = vec![0.0; 2];
        let mut mixer = MonoMixerProcessor;
        mixer.process(&stereo_input, &mut mono_output);
        assert_eq!(mono_output, vec![0.4, 0.4]); // (0.5+0.3)/2 = 0.4
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processor::{GainProcessor, MonoMixerProcessor};  // <-- Добавляем импорты
    
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
        assert!(backends.contains(&BackendType::Null));
    }
    
    #[test]
    fn test_basic_processors() {
        let mut input = vec![0.5f32; 10];
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
        let mut stereo_input = vec![0.5, 0.3, 0.5, 0.3]; // L,R,L,R
        let mut mono_output = vec![0.0; 2];
        let mut mixer = MonoMixerProcessor;
        mixer.process(&stereo_input, &mut mono_output);
        assert_eq!(mono_output, vec![0.4, 0.4]); // (0.5+0.3)/2 = 0.4
    }
}