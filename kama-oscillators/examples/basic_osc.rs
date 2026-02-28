//! Basic oscillator example with new graph architecture

use kama_core::traits::processor::Processor;
use kama_core::traits::{NodeId, PortId};
use kama_graph::prelude::*;
use kama_oscillators::prelude::*;

const BLOCK_SIZE: usize = 64;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama Oscillators Example ===\n");

    // Create graph
    let sample_rate = 44100.0;
    let mut graph = AudioGraph::<BLOCK_SIZE>::new(sample_rate);

    // Create oscillators
    let sine = SineOsc::<BLOCK_SIZE>::new()
        .with_frequency(440.0)
        .with_amplitude(0.3);
    
    let saw = SawOsc::<BLOCK_SIZE>::new()
        .with_frequency(440.0)
        .with_amplitude(0.3)
        .with_bandlimited(true);
    
    let lfo = LFO::<BLOCK_SIZE>::new()
        .with_frequency(5.0)
        .with_waveform(LfoWaveform::Triangle)
        .with_range(0.0, 1.0);

    // Add to graph
    let sine_id = graph.add_processor(Box::new(sine));
    let saw_id = graph.add_processor(Box::new(saw));
    let lfo_id = graph.add_processor(Box::new(lfo));

    println!("Added processors:");
    println!("  Sine: {:?}", sine_id);
    println!("  Saw: {:?}", saw_id);
    println!("  LFO: {:?}", lfo_id);

    // Configure as producer (for testing)
    let sine_out = PortId::audio_out(sine_id, 0);
    graph.configure_as_producer(vec![sine_out])?;
    graph.start()?;

    println!("\nGenerating 1 second of audio...");

    // Produce 43 blocks (approx 1 second at 44.1kHz with BLOCK_SIZE=64)
    for i in 0..43 {
        let output = graph.produce_next(sine_out)?;
        
        // Print first few samples of first block
        if i == 0 {
            println!("First block samples: {:?}", &output[..5]);
        }
    }

    println!("\nStatistics: {:?}", graph.stats());
    println!("\n✅ Example completed successfully!");

    Ok(())
}