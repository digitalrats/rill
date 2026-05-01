//! Basic oscillator example
use rill_core::traits::AudioNode;
use rill_core::traits::Source;
use rill_core::ClockTick;
use rill_oscillators::{NoiseOsc, NoiseType, SawOsc, SineOsc};

const BLOCK_SIZE: usize = 64;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Rill Oscillators Example ===\n");

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

    let clock = ClockTick::new(0, BLOCK_SIZE as u32, sample_rate);

    // Generate one block each
    sine.generate(&clock, &[], &[])?;
    saw.generate(&clock, &[], &[])?;
    noise.generate(&clock, &[], &[])?;

    // Read from output ports
    let sine_output = sine.output_port(0).unwrap().buffer.as_array();
    let saw_output = saw.output_port(0).unwrap().buffer.as_array();
    let noise_output = noise.output_port(0).unwrap().buffer.as_array();

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
