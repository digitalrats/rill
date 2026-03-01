//! Basic oscillator example
use kama_core::traits::Processor;
use kama_oscillators::{SineOsc, SawOsc, NoiseOsc, NoiseType};

const BLOCK_SIZE: usize = 64;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama Oscillators Example ===\n");

    let sample_rate = 44100.0;

    // Create oscillators
    let mut sine = SineOsc::<f32, BLOCK_SIZE>::new()
        .with_frequency(440.0)
        .with_amplitude(0.3);

    let mut saw = SawOsc::<f32, BLOCK_SIZE>::new()
        .with_frequency(220.0)
        .with_amplitude(0.2);

    let mut noise = NoiseOsc::<BLOCK_SIZE>::new()
        .with_type(NoiseType::Pink)
        .with_amplitude(0.1);

    // Initialize with sample rate
    sine.init(sample_rate);
    saw.init(sample_rate);
    noise.init(sample_rate);

    // Prepare buffers
    let mut sine_output = [0.0; BLOCK_SIZE];
    let mut saw_output = [0.0; BLOCK_SIZE];
    let mut noise_output = [0.0; BLOCK_SIZE];

    let mut sine_outputs = [&mut sine_output];
    let mut saw_outputs = [&mut saw_output];
    let mut noise_outputs = [&mut noise_output];

    // Process one block each
    sine.process(&[], &mut sine_outputs, &[])?;
    saw.process(&[], &mut saw_outputs, &[])?;
    noise.process(&[], &mut noise_outputs, &[])?;

    // Print first few samples
    println!("Sine first 5 samples: {:?}", &sine_output[..5]);
    println!("Saw first 5 samples: {:?}", &saw_output[..5]);
    println!("Noise first 5 samples: {:?}", &noise_output[..5]);

    // Verify they are not silent
    assert!(sine_output.iter().any(|&x| x != 0.0));
    assert!(saw_output.iter().any(|&x| x != 0.0));
    assert!(noise_output.iter().any(|&x| x != 0.0));

    println!("\n✅ Example completed successfully!");
    Ok(())
}