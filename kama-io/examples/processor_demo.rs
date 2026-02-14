// Демонстрация различных процессоров

use kama_io::{
    AudioConfig, AudioEngine, AudioBackend,
    processor::{
        PassThroughProcessor,
        GainProcessor,
        MonoMixerProcessor,
        SineProcessor,
    },
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
    println!("=== Kama IO Processor Demo ===\n");
    
    let config = AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(256)
        .with_channels(2);
    
    println!("Audio config: {} Hz, {} samples, {} channels",
             config.sample_rate, config.buffer_size, config.channels);
    
    // Демо 1: Sine Processor
    println!("\n1. SineWave Processor (440Hz)");
    let backend1 = create_default_backend(config.clone())?;
    println!("   Using backend: {}", backend1.name());
    let sine = SineProcessor::new(440.0, config.sample_rate as f32);
    let mut engine1 = AudioEngine::new(backend1, sine);
    engine1.start()?;
    println!("   Playing for 2 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(2));
    engine1.stop()?;
    
    // Демо 2: PassThrough Processor
    println!("\n2. PassThrough Processor (echo)");
    let backend2 = create_default_backend(config.clone())?;
    println!("   Using backend: {}", backend2.name());
    let passthrough = PassThroughProcessor;
    let mut engine2 = AudioEngine::new(backend2, passthrough);
    engine2.start()?;
    println!("   Playing for 2 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(2));
    engine2.stop()?;
    
    // Демо 3: Gain Processor
    println!("\n3. Gain Processor (2x)");
    let backend3 = create_default_backend(config.clone())?;
    println!("   Using backend: {}", backend3.name());
    let gain = GainProcessor::new(2.0);
    let mut engine3 = AudioEngine::new(backend3, gain);
    engine3.start()?;
    println!("   Playing for 2 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(2));
    engine3.stop()?;
    
    // Демо 4: Mono Mixer
    println!("\n4. Mono Mixer (stereo to mono)");
    let backend4 = create_default_backend(config.clone())?;
    println!("   Using backend: {}", backend4.name());
    let mixer = MonoMixerProcessor;
    let mut engine4 = AudioEngine::new(backend4, mixer);
    engine4.start()?;
    println!("   Playing for 2 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(2));
    engine4.stop()?;
    
    println!("\nDemo completed successfully!");
    
    Ok(())
}