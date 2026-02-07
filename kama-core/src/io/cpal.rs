use super::{AudioBackend, AudioConfig, AudioError};
use async_trait::async_trait;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize, Device, SampleRate, Stream, StreamConfig,
};

pub struct CpalBackend {
    device: Device,
    config: AudioConfig,
    input_stream: Option<Stream>,
    output_stream: Option<Stream>,
    xruns: std::sync::atomic::AtomicU32,
}

impl CpalBackend {
    pub async fn new(config: AudioConfig) -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| AudioError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No audio output device found"
            )))?;
        
        Ok(Self {
            device,
            config,
            input_stream: None,
            output_stream: None,
            xruns: std::sync::atomic::AtomicU32::new(0),
        })
    }
}

#[async_trait]
impl AudioBackend for CpalBackend {
    async fn start(&mut self) -> Result<(), AudioError> {
        let config = StreamConfig {
            channels: self.config.channels as u16,
            sample_rate: SampleRate(self.config.sample_rate),
            buffer_size: BufferSize::Fixed(self.config.buffer_size),
        };
        
        let err_fn = |err| {
            eprintln!("CPAL error: {}", err);
        };
        
        let channels = self.config.channels as usize;
        let buffer_size = self.config.buffer_size as usize;
        
        // Создаём output stream
        let output_stream = self.device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // Здесь будет вызываться audio callback
                // data нужно заполнить выходными данными
                for sample in data.iter_mut() {
                    *sample = 0.0;
                }
            },
            err_fn,
            None,
        )?;
        
        output_stream.play()?;
        self.output_stream = Some(output_stream);
        
        Ok(())
    }
    
    async fn stop(&mut self) -> Result<(), AudioError> {
        if let Some(stream) = self.output_stream.take() {
            stream.pause()?;
        }
        Ok(())
    }
    
    async fn read(&mut self, _buffer: &mut [f32]) -> Result<usize, AudioError> {
        // Реализация чтения для input streams
        Ok(0)
    }
    
    async fn write(&mut self, buffer: &[f32]) -> Result<usize, AudioError> {
        // Для CPAL запись происходит в callback
        Ok(buffer.len())
    }
    
    fn config(&self) -> &AudioConfig {
        &self.config
    }
    
    fn xruns(&self) -> u32 {
        self.xruns.load(std::sync::atomic::Ordering::Relaxed)
    }
    
    fn latency(&self) -> std::time::Duration {
        std::time::Duration::from_micros(
            (1_000_000.0 * self.config.buffer_size as f32 / self.config.sample_rate as f32) as u64
        )
    }
}