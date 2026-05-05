use std::time::Duration;

use crate::backend::{AudioBackend, BackendType};
use crate::config::AudioConfig;
use crate::error::IoResult;
use rill_core::io::IoBackend;

#[derive(Debug)]
pub struct NullBackend {
    config: AudioConfig,
    is_running: bool,
    xruns: u32,
}

impl NullBackend {
    pub fn new(config: AudioConfig) -> Self {
        Self {
            config,
            is_running: false,
            xruns: 0,
        }
    }
}

impl AudioBackend for NullBackend {
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
            (1_000_000.0 * self.config.buffer_size as f64 / self.config.sample_rate as f64) as u64,
        )
    }
    fn list_input_devices(&self) -> Vec<String> {
        vec!["Null Input".into()]
    }
    fn list_output_devices(&self) -> Vec<String> {
        vec!["Null Output".into()]
    }
}

impl IoBackend<f32> for NullBackend {
    fn set_process_callback(&self, _cb: Box<dyn Fn()>) {}
    fn read(&self, channels: &mut [&mut [f32]]) -> usize {
        let n = channels.first().map(|c| c.len()).unwrap_or(0);
        for ch in channels.iter_mut() {
            ch[..n].fill(0.0);
        }
        n
    }
    fn write(&self, channels: &[&[f32]]) -> usize {
        channels.first().map(|c| c.len()).unwrap_or(0)
    }
    fn start(&self) -> Result<(), String> {
        Ok(())
    }
    fn stop(&self) -> Result<(), String> {
        Ok(())
    }
}
