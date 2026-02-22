//! Моки для тестирования kama-io без реальных устройств

use std::time::Duration;
use std::sync::atomic::{AtomicU32, Ordering};
use kama_io::{
    AudioConfig, AudioBackend, BackendType, IoResult, IoError,
};

/// Мок бэкенда для тестирования
#[derive(Debug)]
pub struct MockBackend {
    config: AudioConfig,
    xruns: AtomicU32,
    should_fail: bool,
}

impl MockBackend {
    pub fn new(config: AudioConfig) -> Self {
        Self {
            config,
            xruns: AtomicU32::new(0),
            should_fail: false,
        }
    }
    
    pub fn with_failure(mut self, fail: bool) -> Self {
        self.should_fail = fail;
        self
    }
}

impl AudioBackend for MockBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::Null
    }
    
    fn config(&self) -> &AudioConfig {
        &self.config
    }
    
    fn config_mut(&mut self) -> &mut AudioConfig {
        &mut self.config
    }
    
    fn init(&mut self) -> IoResult<()> {
        if self.should_fail {
            Err(IoError::Init("Mock failure".into()))
        } else {
            Ok(())
        }
    }
    
    fn start(&mut self) -> IoResult<()> {
        if self.should_fail {
            Err(IoError::Backend("Mock failure".into()))
        } else {
            Ok(())
        }
    }
    
    fn stop(&mut self) -> IoResult<()> {
        Ok(())
    }
    
    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize> {
        buffer.fill(0.0);
        Ok(buffer.len())
    }
    
    fn write(&mut self, buffer: &[f32]) -> IoResult<usize> {
        Ok(buffer.len())
    }
    
    fn xruns(&self) -> u32 {
        self.xruns.load(Ordering::Relaxed)
    }
    
    fn latency(&self) -> Duration {
        Duration::from_micros(0)
    }
    
    fn list_input_devices(&self) -> Vec<String> {
        vec!["Mock Input".to_string()]
    }
    
    fn list_output_devices(&self) -> Vec<String> {
        vec!["Mock Output".to_string()]
    }
}

/// Создаёт конфигурацию для тестов
pub fn test_config() -> AudioConfig {
    AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(256)
        .with_channels(2)
}