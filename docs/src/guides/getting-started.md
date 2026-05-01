# Getting Started

Add `rill-adrift` to your `Cargo.toml`:

```toml
[dependencies]
rill-adrift = "0.3"
```

For individual crates (if you don't need the full ecosystem):

```toml
[dependencies]
rill-core-dsp = "0.3"
```

## Minimal DSP Pipeline

This example creates a sine wave oscillator through a delay effect, using block processing from `rill-core-dsp`:

```rust
use rill_core_dsp::generators::basic::SineOsc;
use rill_core_dsp::delay::Delay;
use rill_core_dsp::algorithm::Algorithm;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sample_rate = 44100.0;
    let block_size = 64;

    let mut osc = SineOsc::<f32>::new(440.0, sample_rate);
    osc.set_amplitude(0.5);

    let mut delay = Delay::<f32>::new(0.3, sample_rate);
    delay.set_feedback(0.4);
    delay.set_mix(0.7);

    let mut input_block = vec![0.0f32; block_size];
    let mut output_block = vec![0.0f32; block_size];

    let total_samples = sample_rate as usize;
    let mut processed = Vec::with_capacity(total_samples);

    for _ in 0..(total_samples / block_size) {
        osc.process_block(&[], &mut input_block);
        delay.process_block(&input_block, &mut output_block);
        processed.extend_from_slice(&output_block);
    }

    println!("Processed {} samples", processed.len());
    Ok(())
}
```

## Using the Audio Graph

For more complex signal routing, use `rill-graph` with `GraphBuilder`:

```rust
use rill_graph::GraphBuilder;
use rill_oscillators::audio::SineOsc;

let mut builder = GraphBuilder::<f32, 64>::new();
let osc_id = builder.add_source(Box::new(
    SineOsc::new(440.0, 44100.0)
));
```

## Audio I/O

Enable the `io` feature on `rill-adrift` (default) or add `rill-io` directly:

```toml
rill-adrift = { version = "0.3", features = ["io"] }
```

Backends are feature-gated:

```toml
rill-adrift = { version = "0.3", features = ["io", "alsa"] }
```

Available backends: `alsa`, `cpal` (default), `pipewire`, `jack`.
