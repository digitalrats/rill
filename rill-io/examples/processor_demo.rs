//! Демонстрация различных процессоров

use rill_io::{
    backends::{CpalBackend, NullBackend},
    processor::{GainProcessor, MonoMixerProcessor, PassThroughProcessor, SineProcessor},
    AudioBackend, AudioConfig, AudioEngine, BackendType,
};

#[cfg(feature = "alsa")]
use rill_io::backends::AlsaBackend;

fn create_backend(
    config: AudioConfig,
) -> Result<Box<dyn rill_io::AudioBackend>, Box<dyn std::error::Error>> {
    #[cfg(all(target_os = "linux", feature = "alsa"))]
    {
        let backend = AlsaBackend::new(config.clone())?;
        return Ok(Box::new(backend));
    }

    #[cfg(feature = "cpal")]
    {
        let backend = CpalBackend::new(config.clone())?;
        return Ok(Box::new(backend));
    }

    Ok(Box::new(NullBackend::new(config)))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Rill IO Processor Demo ===\n");

    let config = AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(256)
        .with_channels(2);

    println!(
        "Audio config: {} Hz, {} samples, {} channels",
        config.sample_rate, config.buffer_size, config.output_channels
    );

    // Демо 1: Sine Processor
    println!("\n1. SineWave Processor (440Hz)");
    let backend1 = create_backend(config.clone())?;
    println!("   Using backend: {}", backend1.backend_type().name());
    let sine = SineProcessor::new(440.0, config.sample_rate as f32);
    let mut engine1 = AudioEngine::new(backend1, sine);
    engine1.start()?;
    println!("   Playing for 2 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(2));
    engine1.stop()?;

    // Демо 2: PassThrough Processor
    println!("\n2. PassThrough Processor (echo)");
    let backend2 = create_backend(config.clone())?;
    println!("   Using backend: {}", backend2.backend_type().name());
    let passthrough = PassThroughProcessor;
    let mut engine2 = AudioEngine::new(backend2, passthrough);
    engine2.start()?;
    println!("   Playing for 2 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(2));
    engine2.stop()?;

    // Демо 3: Gain Processor
    println!("\n3. Gain Processor (2x)");
    let backend3 = create_backend(config.clone())?;
    println!("   Using backend: {}", backend3.backend_type().name());
    let gain = GainProcessor::new(2.0);
    let mut engine3 = AudioEngine::new(backend3, gain);
    engine3.start()?;
    println!("   Playing for 2 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(2));
    engine3.stop()?;

    // Демо 4: Mono Mixer
    println!("\n4. Mono Mixer (stereo to mono)");
    let backend4 = create_backend(config.clone())?;
    println!("   Using backend: {}", backend4.backend_type().name());
    let mixer = MonoMixerProcessor;
    let mut engine4 = AudioEngine::new(backend4, mixer);
    engine4.start()?;
    println!("   Playing for 2 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(2));
    engine4.stop()?;

    println!("\nDemo completed successfully!");

    Ok(())
}
