//! Audio I/O backends for Kama Audio
//! 
//! Этот крейт предоставляет унифицированный интерфейс для различных
//! аудио бэкендов с использованием кольцевых буферов из kama-buffers.

#![warn(missing_docs)]

mod backend;
mod config;
mod error;
mod engine;
mod null;
mod graph_processor;  // <-- НОВЫЙ МОДУЛЬ

#[cfg(feature = "cpal")]
mod cpal;
#[cfg(feature = "alsa")]
mod alsa;
#[cfg(feature = "pipewire")]
mod pipewire;
#[cfg(feature = "jack")]
mod jack;

// Реэкспорты
pub use backend::{AudioBackend, BackendType};
pub use config::{AudioConfig, BackendOptions, 
                 CpalOptions, AlsaOptions, PipewireOptions, JackOptions};
pub use error::{IoError, IoResult};
pub use engine::{AudioEngine, AudioProcessor, EngineState};
pub use graph_processor::GraphProcessor;  // <-- НОВЫЙ РЕЭКСПОРТ

#[cfg(feature = "cpal")]
pub use cpal::CpalBackend;

#[cfg(feature = "alsa")]
pub use alsa::AlsaBackend;

#[cfg(feature = "pipewire")]
pub use pipewire::PipewireBackend;

#[cfg(feature = "jack")]
pub use jack::JackBackend;

pub use null::NullBackend;

pub use factory::BackendFactory;
pub use processor::{PassThroughProcessor, SilenceProcessor};

/// Фабрика для создания бэкендов
pub mod factory {
    use super::*;
    
    pub struct BackendFactory;
    
    impl BackendFactory {
        #[cfg(feature = "cpal")]
        pub fn create_default(config: AudioConfig) -> IoResult<impl AudioBackend> {
            Ok(CpalBackend::new(config)?)
        }
        
        pub fn create(backend_type: BackendType, config: AudioConfig) -> IoResult<Box<dyn AudioBackend>> {
            match backend_type {
                #[cfg(feature = "cpal")]
                BackendType::Cpal => Ok(Box::new(CpalBackend::new(config)?)),
                
                #[cfg(feature = "alsa")]
                BackendType::Alsa => Ok(Box::new(AlsaBackend::new(config)?)),
                
                #[cfg(feature = "pipewire")]
                BackendType::PipeWire => Ok(Box::new(PipewireBackend::new(config)?)),
                
                #[cfg(feature = "jack")]
                BackendType::Jack => Ok(Box::new(JackBackend::new(config)?)),
                
                BackendType::Null => Ok(Box::new(NullBackend::new(config))),
                
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

/// Простые процессоры для тестирования
pub mod processor {
    use super::AudioProcessor;
    
    pub struct PassThroughProcessor;
    
    impl AudioProcessor for PassThroughProcessor {
        fn process(&mut self, input: &[f32], output: &mut [f32]) {
            output.copy_from_slice(input);
        }
        
        fn reset(&mut self) {}
        
        fn set_sample_rate(&mut self, _sample_rate: f32) {}
    }
    
    pub struct SilenceProcessor;
    
    impl AudioProcessor for SilenceProcessor {
        fn process(&mut self, _input: &[f32], output: &mut [f32]) {
            output.fill(0.0);
        }
        
        fn reset(&mut self) {}
        
        fn set_sample_rate(&mut self, _sample_rate: f32) {}
    }
}