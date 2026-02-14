use kama_core::dsp::SineOscillator;
use kama_core::AudioNode;
use kama_io::{
    AudioConfig, AudioEngine, AudioProcessor,
    BackendFactory, BackendType,
    AudioBackend,  // <-- ВАЖНО: импортируем трейт AudioBackend
    CpalBackend, NullBackend
};
use std::sync::Arc;
use parking_lot::RwLock;

struct SineProcessor {
    oscillator: SineOscillator,
    sample_rate: Arc<RwLock<f32>>,
    position: usize,
}

impl SineProcessor {
    fn new(sample_rate: f32) -> Self {
        Self {
            oscillator: SineOscillator::new(440.0),
            sample_rate: Arc::new(RwLock::new(sample_rate)),
            position: 0,
        }
    }
}

impl AudioProcessor for SineProcessor {
    fn process(&mut self, _input: &[f32], output: &mut [f32]) {
        let sample_rate = *self.sample_rate.read();
        let mut temp = vec![0.0f32; output.len() / 2];
        
        // Генерируем синус через AudioNode трейт
        let mut temp_slice = [temp.as_mut_slice()];
        self.oscillator.process(&[], &mut temp_slice).unwrap();
        
        // Копируем на левый и правый каналы
        for i in 0..temp.len() {
            output[i * 2] = temp[i] * 0.5;
            output[i * 2 + 1] = temp[i] * 0.5;
        }
        
        self.position += temp.len();
    }
    
    fn reset(&mut self) {
        self.position = 0;
        self.oscillator.reset();
    }
    
    fn set_sample_rate(&mut self, sample_rate: f32) {
        *self.sample_rate.write() = sample_rate;
        self.oscillator.init(sample_rate);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama IO Simple Playback Demo ===\n");
    
    let config = AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(256)
        .with_channels(2);
    
    println!("Available backends:");
    for backend in BackendFactory::available_backends() {
        println!("  - {}", backend.name());
    }
    
    // Создаем конкретный тип бэкенда
    #[cfg(feature = "cpal")]
    let backend = CpalBackend::new(config.clone())?;
    
    #[cfg(not(feature = "cpal"))]
    let backend = NullBackend::new(config.clone());
    
    println!("\nUsing backend: {}", backend.name());  // <-- теперь метод name доступен
    
    let processor = SineProcessor::new(config.sample_rate as f32);
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