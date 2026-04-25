//! Audio I/O backends for Rill
//! 
//! Этот крейт предоставляет унифицированный интерфейс для различных
//! аудио бэкендов с использованием кольцевых буферов из rill-buffers.

#![warn(missing_docs)]

mod backend;
mod config;
mod error;
mod engine;

// Публичные модули
pub mod backends;
pub mod processor;

// Реэкспорты из модулей
pub use backend::{AudioBackend, BackendType, DeviceInfo};
pub use config::AudioConfig;
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
};

#[cfg(feature = "graph")]
pub use processor::GraphProcessor;

#[cfg(feature = "examples")]
pub use processor::SineProcessor;

#[cfg(feature = "examples")]
pub use processor::GranularProcessor;

#[cfg(feature = "examples")]
pub use processor::CaptureProcessor;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AudioConfig;
    use crate::processor::{GainProcessor, PassThroughProcessor, SilenceProcessor, MonoMixerProcessor};

    #[test]
    fn test_config_default() {
        let config = AudioConfig::default();
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.buffer_size, 256);
        assert_eq!(config.input_channels, 2);
        assert_eq!(config.output_channels, 2);
    }

    #[test]
    fn test_config_with_methods() {
        let config = AudioConfig::new()
            .with_sample_rate(44100)
            .with_buffer_size(512)
            .with_channels(1);
        assert_eq!(config.sample_rate, 44100);
        assert_eq!(config.buffer_size, 512);
    }

    #[test]
    fn test_latency_calculation() {
        let config = AudioConfig::new()
            .with_sample_rate(48000)
            .with_buffer_size(256);
        let latency_sec = config.latency_seconds();
        let latency_ms = config.latency_ms();
        assert!((latency_sec - 256.0/48000.0).abs() < 1e-10);
        assert!((latency_ms - 256.0*1000.0/48000.0).abs() < 1e-10);
    }

    #[test]
    fn test_gain_processor() {
        let mut proc = GainProcessor::new(2.0);
        let input = vec![1.0, 2.0, 3.0];
        let mut output = vec![0.0; 3];
        proc.process(&input, &mut output);
        assert_eq!(output, vec![2.0, 4.0, 6.0]);
    }

    #[test]
    fn test_passthrough_processor() {
        let mut proc = PassThroughProcessor;
        let input = vec![1.0, -1.0, 0.5];
        let mut output = vec![0.0; 3];
        proc.process(&input, &mut output);
        assert_eq!(output, input);
    }

    #[test]
    fn test_silence_processor() {
        let mut proc = SilenceProcessor;
        let input = vec![1.0, 2.0, 3.0];
        let mut output = vec![1.0; 3]; // заполним чем-то
        proc.process(&input, &mut output);
        assert_eq!(output, vec![0.0; 3]);
    }

    #[test]
    fn test_mono_mixer_processor() {
        let mut proc = MonoMixerProcessor;
        // стерео вход: L,R,L,R...
        let input = vec![0.8, 0.2, 0.5, 0.5, 1.0, 0.0];
        let mut output = vec![0.0; 3];
        proc.process(&input, &mut output);
        // ожидаем (0.8+0.2)/2 = 0.5, (0.5+0.5)/2 = 0.5, (1.0+0.0)/2 = 0.5
        assert_eq!(output, vec![0.5, 0.5, 0.5]);
    }
}