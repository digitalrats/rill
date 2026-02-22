//! Пример использования ALSA бэкенда
//!
//! Запуск: cargo run --example alsa_demo --features "alsa,examples"

use kama_io::{
    AudioConfig, AudioEngine,
    backends::AlsaBackend,
    processor::SineProcessor,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama IO ALSA Demo ===\n");
    
    // Создаём конфигурацию
    let config = AudioConfig::default()
        .with_sample_rate(48000)
        .with_buffer_size(256)
        .with_channels(2);
    
    println!("Audio config: {} Hz, {} samples, {} channels",
             config.sample_rate, config.buffer_size, config.output_channels);
    
    // Создаём ALSA бэкенд
    let backend = AlsaBackend::new(config.clone())?;
    println!("\nUsing backend: {}", backend.backend_type().name());
    
    // Создаём процессор (генератор синуса)
    let processor = SineProcessor::new(440.0, config.sample_rate as f32);
    
    // Создаём движок
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