//! JACK бэкенд (заглушка)

use std::fmt;
use std::time::Duration;

use crate::backend::{AudioBackend, BackendType};
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};

/// JACK бэкенд (заглушка)
pub struct JackBackend {
    config: AudioConfig,
    is_running: bool,
    xruns: u32,
}

impl fmt::Debug for JackBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JackBackend")
            .field("config", &self.config)
            .field("is_running", &self.is_running)
            .field("xruns", &self.xruns)
            .finish()
    }
}

impl JackBackend {
    /// Создать новый JACK бэкенд
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            return Err(IoError::Unsupported(
                "JACK is only available on Linux and macOS".into(),
            ));
        }

        Ok(Self {
            config,
            is_running: false,
            xruns: 0,
        })
    }
}

impl AudioBackend for JackBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::Jack
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
            (1_000_000.0 * self.config.buffer_size as f64 / self.config.sample_rate as f64) as u64,
        )
    }

    fn list_input_devices(&self) -> Vec<String> {
        vec!["default".to_string()]
    }

    fn list_output_devices(&self) -> Vec<String> {
        vec!["default".to_string()]
    }
}
