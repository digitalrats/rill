//! Простое воспроизведение с автоматическим выбором бэкенда

use kama_io::{
    backends::{CpalBackend, NullBackend},
    processor::SineProcessor,
    AudioConfig, AudioEngine, BackendType,
};

#[cfg(feature = "alsa")]
use kama_io::backends::AlsaBackend;

fn create_backend(
    config: AudioConfig,
) -> Result<Box<dyn kama_io::AudioBackend>, Box<dyn std::error::Error>> {
    // Пробуем ALSA на Linux
    #[cfg(all(target_os = "linux", feature = "alsa"))]
    {
        let backend = AlsaBackend::new(config.clone())?;
        return Ok(Box::new(backend));
    }

    // Пробуем CPAL как запасной вариант
    #[cfg(feature = "cpal")]
    {
        let backend = CpalBackend::new(config.clone())?;
        return Ok(Box::new(backend));
    }

    // Если ничего не работает, используем Null
    Ok(Box::new(NullBackend::new(config)))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama IO Simple Playback ===\n");

    let config = AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(256)
        .with_channels(2);

    println!(
        "Audio config: {} Hz, {} samples, {} channels",
        config.sample_rate, config.buffer_size, config.output_channels
    );

    // Создаём бэкенд
    let backend = create_backend(config.clone())?;
    println!("\nUsing backend: {}", backend.backend_type().name());

    let processor = SineProcessor::new(440.0, config.sample_rate as f32);
    let mut engine = AudioEngine::new(backend, processor);

    println!("\nStarting audio engine...");
    engine.start()?;

    println!("Playing 440Hz sine wave for 3 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(3));

    println!("Stopping...");
    engine.stop()?;

    println!("\nDone! Xruns: {}", engine.xruns());

    Ok(())
}
