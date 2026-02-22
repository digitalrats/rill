//! Пример с Null бэкендом

use kama_io::{
    AudioConfig, AudioEngine,
    backends::NullBackend,
    processor::SilenceProcessor,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama IO Null Backend Demo ===\n");
    
    let config = AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(256)
        .with_channels(2);
    
    let backend = NullBackend::new(config.clone());
    println!("Using backend: {}", backend.backend_type().name());
    
    let processor = SilenceProcessor;
    let mut engine = AudioEngine::new(backend, processor);
    
    println!("Starting null backend...");
    engine.start()?;
    
    println!("Running for 1 second...");
    std::thread::sleep(std::time::Duration::from_secs(1));
    
    println!("Stopping...");
    engine.stop()?;
    
    println!("\nDone! Xruns: {}", engine.xruns());
    
    Ok(())
}