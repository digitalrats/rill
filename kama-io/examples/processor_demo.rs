//! Демонстрация различных процессоров

use kama_io::{
    AudioConfig, AudioEngine,
    AudioBackend, CpalBackend, NullBackend,
    processor::{
        PassThroughProcessor,
        GainProcessor,
        MonoMixerProcessor,
        SineProcessor,
    },
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama IO Processor Demo ===\n");
    
    let config = AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(256)
        .with_channels(2);
    
    #[cfg(feature = "cpal")]
    let backend = CpalBackend::new(config.clone())?;
    
    #[cfg(not(feature = "cpal"))]
    let backend = NullBackend::new(config.clone());
    
    println!("1. SineWave Processor (440Hz)");
    let sine = SineProcessor::new(440.0, config.sample_rate as f32);
    let mut engine1 = AudioEngine::new(backend, sine);
    engine1.start()?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    engine1.stop()?;
    
    println!("\n2. PassThrough Processor (echo)");
    #[cfg(feature = "cpal")]
    let backend2 = CpalBackend::new(config.clone())?;
    #[cfg(not(feature = "cpal"))]
    let backend2 = NullBackend::new(config.clone());
    
    let passthrough = PassThroughProcessor;
    let mut engine2 = AudioEngine::new(backend2, passthrough);
    engine2.start()?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    engine2.stop()?;
    
    println!("\n3. Gain Processor (2x)");
    #[cfg(feature = "cpal")]
    let backend3 = CpalBackend::new(config.clone())?;
    #[cfg(not(feature = "cpal"))]
    let backend3 = NullBackend::new(config.clone());
    
    let gain = GainProcessor::new(2.0);
    let mut engine3 = AudioEngine::new(backend3, gain);
    engine3.start()?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    engine3.stop()?;
    
    println!("\n4. Mono Mixer (stereo to mono)");
    #[cfg(feature = "cpal")]
    let backend4 = CpalBackend::new(config.clone())?;
    #[cfg(not(feature = "cpal"))]
    let backend4 = NullBackend::new(config.clone());
    
    let mixer = MonoMixerProcessor;
    let mut engine4 = AudioEngine::new(backend4, mixer);
    engine4.start()?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    engine4.stop()?;
    
    println!("\nDemo completed!");
    
    Ok(())
}