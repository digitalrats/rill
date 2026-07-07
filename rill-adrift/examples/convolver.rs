// rill-adrift/examples/convolver.rs
//! # Convolution Reverb Example
//!
//! Demonstrates frequency‑domain convolution using `PartitionedConvolver`
//! and the `ConvolverNode` graph node with a synthetic impulse response.
//!
//! ```bash
//! cargo run --example convolver --features fft
//! cargo run --example convolver --features fft -- --ir path/to/ir.wav
//! ```

use std::error::Error;

const BUF_SIZE: usize = 128;
const SAMPLE_RATE: f32 = 44100.0;

fn main() -> Result<(), Box<dyn Error>> {
    let ir_path = std::env::args().nth(2);

    // Load or generate impulse response
    let ir: Vec<f32> = match ir_path {
        Some(ref path) => {
            println!("Loading IR from: {path}");
            load_wav_ir(path)?
        }
        None => {
            let len = 4096;
            println!("No IR provided — generating synthetic {len}-sample exponential decay IR.");
            generate_synthetic_ir(len)
        }
    };
    println!(
        "IR length: {} samples ({:.1} ms at {} Hz)",
        ir.len(),
        ir.len() as f32 / SAMPLE_RATE * 1000.0,
        SAMPLE_RATE
    );

    // --- Demo 1: Stand‑alone PartitionedConvolver ---
    println!("\n=== PartitionedConvolver (stand‑alone) ===");
    demo_partitioned_conv(&ir);

    // --- Demo 2: Graph node ConvolverNode ---
    println!("\n=== ConvolverNode (graph node) ===");
    demo_graph_node(&ir);

    Ok(())
}

/// Stand‑alone `PartitionedConvolver` processing a unit impulse.
fn demo_partitioned_conv(ir: &[f32]) {
    use rill_adrift::fft::partitioned_conv::PartitionedConvolver;

    let mut conv = PartitionedConvolver::<f32, BUF_SIZE>::new(ir.len());
    conv.set_ir(ir);

    // Feed a unit impulse
    let mut input = [0.0f32; BUF_SIZE];
    input[0] = 1.0;
    let mut output = [0.0f32; BUF_SIZE];
    conv.process(&input, &mut output);

    println!("Unit impulse response (first 10 samples):");
    for (i, o) in output.iter().enumerate().take(10) {
        println!("  [{i:2}] = {o:+.6}");
    }
}

/// Graph‑node `ConvolverNode` with a synthetic signal.
fn demo_graph_node(ir: &[f32]) {
    use rill_adrift::fft::nodes::convolver_node::ConvolverNode;
    use rill_adrift::rill_core::traits::{Node, Processor};
    use rill_adrift::rill_core::RenderContext;

    let mut node = ConvolverNode::<f32, BUF_SIZE>::new(ir.len(), SAMPLE_RATE);
    Node::init(&mut node, SAMPLE_RATE);
    node.set_ir(ir);
    node.set_mix(1.0);

    // Generate a synthetic signal: 200 Hz + 800 Hz sine
    let mut signal = [0.0f32; BUF_SIZE];
    for (i, s) in signal.iter_mut().enumerate() {
        let t = i as f32 / SAMPLE_RATE;
        *s = (2.0 * std::f32::consts::PI * 200.0 * t).sin() * 0.5
            + (2.0 * std::f32::consts::PI * 800.0 * t).sin() * 0.3;
    }

    node.input_port_mut(0)
        .unwrap()
        .write()
        .copy_from_slice(&signal);
    let ctx = RenderContext::new(0, 0, SAMPLE_RATE);
    node.process(&ctx, &[], &[], &[], &[]).unwrap();

    let output = node.output_port(0).unwrap().read();
    println!("Convolved signal (first 8 samples):");
    for (i, (inp, out)) in signal.iter().zip(output.iter()).enumerate().take(8) {
        println!("  [{i:2}] in={inp:+7.4} → out={out:+9.6}");
    }

    // Parameter access
    use rill_adrift::rill_core::ParameterId;
    println!(
        "ir_gain = {:?}",
        node.get_parameter(&ParameterId::new("ir_gain").unwrap())
    );
    println!(
        "mix     = {:?}",
        node.get_parameter(&ParameterId::new("mix").unwrap())
    );
}

// --- Helpers ---

fn load_wav_ir(path: &str) -> Result<Vec<f32>, Box<dyn Error>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    match spec.sample_format {
        hound::SampleFormat::Float => Ok(reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?),
        hound::SampleFormat::Int => Ok(reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / 32768.0))
            .collect::<Result<Vec<_>, _>>()?),
    }
}

fn generate_synthetic_ir(len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE;
            let decay = (-t * 3.0).exp();
            let noise = (i as f32 * 0.7).sin() * 0.3 + (i as f32 * 1.3).sin() * 0.2;
            noise * decay
        })
        .collect()
}
