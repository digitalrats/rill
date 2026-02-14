use kama_io::{
    AudioConfig, AudioEngine, AudioBackend,  // <-- ДОБАВЛЯЕМ AudioBackend
    processor::SineProcessor,
};

#[cfg(feature = "cpal")]
use kama_io::CpalBackend;

#[cfg(feature = "alsa")]
use kama_io::AlsaBackend;

// Определяем тип бэкенда по умолчанию для текущей платформы
#[cfg(all(target_os = "linux", feature = "alsa"))]
type DefaultBackend = AlsaBackend;

#[cfg(all(target_os = "linux", not(feature = "alsa"), feature = "cpal"))]
type DefaultBackend = CpalBackend;

#[cfg(all(not(target_os = "linux"), feature = "cpal"))]
type DefaultBackend = CpalBackend;

// Если ничего не подходит, используем заглушку
#[cfg(not(any(
    all(target_os = "linux", feature = "alsa"),
    all(target_os = "linux", not(feature = "alsa"), feature = "cpal"),
    all(not(target_os = "linux"), feature = "cpal")
)))]
type DefaultBackend = kama_io::NullBackend;

// Функция для создания бэкенда с конкретным типом
fn create_default_backend(config: AudioConfig) -> Result<DefaultBackend, Box<dyn std::error::Error>> {
    #[cfg(all(target_os = "linux", feature = "alsa"))]
    {
        Ok(DefaultBackend::new(config)?)
    }
    
    #[cfg(all(target_os = "linux", not(feature = "alsa"), feature = "cpal"))]
    {
        Ok(DefaultBackend::new(config)?)
    }
    
    #[cfg(all(not(target_os = "linux"), feature = "cpal"))]
    {
        Ok(DefaultBackend::new(config)?)
    }
    
    #[cfg(not(any(
        all(target_os = "linux", feature = "alsa"),
        all(target_os = "linux", not(feature = "alsa"), feature = "cpal"),
        all(not(target_os = "linux"), feature = "cpal")
    )))]
    {
        Ok(DefaultBackend::new(config))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama IO Simple Playback Demo ===\n");
    
    let config = AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(256)
        .with_channels(2);
    
    println!("Audio config: {} Hz, {} samples, {} channels",
             config.sample_rate, config.buffer_size, config.channels);
    
    // Создаем бэкенд с конкретным типом
    let backend = create_default_backend(config.clone())?;
    println!("\nUsing backend: {}", backend.name());  // <-- теперь метод name доступен
    
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