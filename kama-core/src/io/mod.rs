use crate::AudioError;
use async_trait::async_trait;

#[cfg(feature = "pipewire-backend")]
pub mod pipewire;

#[cfg(feature = "alsa-backend")]
pub mod alsa;

#[cfg(feature = "cpal")]
pub mod cpal;

/// Конфигурация аудиоустройства
#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub channels: u32,
    pub input_channels: u32,
    pub output_channels: u32,
    pub latency_ms: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            buffer_size: 128,
            channels: 2,
            input_channels: 2,
            output_channels: 2,
            latency_ms: 5,
        }
    }
}

/// Аудио backend
#[async_trait]
pub trait AudioBackend: Send + Sync {
    async fn start(&mut self) -> Result<(), AudioError>;
    async fn stop(&mut self) -> Result<(), AudioError>;
    async fn read(&mut self, buffer: &mut [f32]) -> Result<usize, AudioError>;
    async fn write(&mut self, buffer: &[f32]) -> Result<usize, AudioError>;
    fn config(&self) -> &AudioConfig;
    fn xruns(&self) -> u32;
    fn latency(&self) -> std::time::Duration;
}

/// Аудио процессор
pub trait AudioProcessor: Send + Sync {
    fn process(&mut self, input: &[f32], output: &mut [f32]);
    fn set_sample_rate(&mut self, sample_rate: f32);
}

/// Аудио движок
pub struct AudioEngine<B: AudioBackend, P: AudioProcessor> {
    backend: B,
    processor: P,
    running: bool,
}

impl<B: AudioBackend, P: AudioProcessor> AudioEngine<B, P> {
    pub fn new(backend: B, processor: P) -> Self {
        Self {
            backend,
            processor,
            running: false,
        }
    }
    
    pub async fn run(&mut self) -> Result<(), AudioError> {
        self.running = true;
        self.backend.start().await?;
        
        let buffer_size = self.backend.config().buffer_size as usize;
        let channels = self.backend.config().channels as usize;
        let total_size = buffer_size * channels;
        
        let mut input_buffer = vec![0.0f32; total_size];
        let mut output_buffer = vec![0.0f32; total_size];
        
        while self.running {
            let read = self.backend.read(&mut input_buffer).await?;
            
            if read > 0 {
                self.processor.process(&input_buffer[..read], &mut output_buffer[..read]);
                self.backend.write(&output_buffer[..read]).await?;
            }
        }
        
        self.backend.stop().await
    }
    
    pub fn stop(&mut self) {
        self.running = false;
    }
}
