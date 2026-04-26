//! PipeWire бэкенд для Linux (заглушка)

use std::time::Duration;
use std::fmt;

use crate::backend::{AudioBackend, BackendType};
use crate::config::AudioConfig;
use crate::error::{IoResult, IoError};

/// PipeWire бэкенд (заглушка)
pub struct PipewireBackend {
    config: AudioConfig,
    is_running: bool,
    xruns: u32,
}

impl fmt::Debug for PipewireBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipewireBackend")
            .field("config", &self.config)
            .field("is_running", &self.is_running)
            .field("xruns", &self.xruns)
            .finish()
    }
}

impl PipewireBackend {
    /// Создать новый PipeWire бэкенд
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        if !cfg!(target_os = "linux") {
            return Err(IoError::Unsupported("PipeWire is only available on Linux".into()));
        }
        
        Ok(Self {
            config,
            is_running: false,
            xruns: 0,
        })
    }
}

impl AudioBackend for PipewireBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::PipeWire
    }
    
    fn config(&self) -> &AudioConfig {
        &self.config
    }
    
    fn config_mut(&mut self) -> &mut AudioConfig {
        &mut self.config
    }
    
    fn init(&mut self) -> IoResult<()> {
        // Заглушка
        Ok(())
    }
    
    fn start(&mut self) -> IoResult<()> {
        self.is_running = true;
        Ok(())
    }
    
    fn stop(&mut self) -> IoResult<()> {
        self.is_running = false;
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
        self.xruns
    }
    
    fn latency(&self) -> Duration {
        Duration::from_micros(
            (1_000_000.0 * self.config.buffer_size as f64 / self.config.sample_rate as f64) as u64
        )
    }
    
    fn list_input_devices(&self) -> Vec<String> {
        vec!["default".to_string()]
    }
    
    fn list_output_devices(&self) -> Vec<String> {
        vec!["default".to_string()]
    }
}