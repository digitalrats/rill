//! Null бэкенд для тестирования

use std::time::Duration;

use crate::backend::AudioBackend;
use crate::config::AudioConfig;
use crate::error::IoResult;

/// Null бэкенд - не производит реального аудио ввода-вывода
pub struct NullBackend {
    config: AudioConfig,
    is_running: bool,
    xruns: u32,
}

impl NullBackend {
    /// Создать новый Null бэкенд
    pub fn new(config: AudioConfig) -> Self {
        Self {
            config,
            is_running: false,
            xruns: 0,
        }
    }
}

impl AudioBackend for NullBackend {
    fn name(&self) -> &'static str {
        "Null"
    }
    
    fn config(&self) -> &AudioConfig {
        &self.config
    }
    
    fn config_mut(&mut self) -> &mut AudioConfig {
        &mut self.config
    }
    
    fn init(&mut self) -> IoResult<()> {
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
    
    fn is_available(&self) -> bool {
        true
    }
    
    fn list_input_devices(&self) -> Vec<String> {
        vec!["Null Input".to_string()]
    }
    
    fn list_output_devices(&self) -> Vec<String> {
        vec!["Null Output".to_string()]
    }
}