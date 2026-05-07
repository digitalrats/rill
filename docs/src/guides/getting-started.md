# Getting Started

Add `rill-adrift` to your `Cargo.toml`:

```toml
[dependencies]
rill-adrift = "0.5.0-beta.2"
```

For individual crates (if you don't need the full ecosystem):

```toml
[dependencies]
rill-core-dsp = "0.5.0-beta.2"
```

## Example: Audio graph with sine oscillator

This example builds a signal graph with a sine oscillator and runs it
through the pull model (Sink drives processing):

```rust
use rill_adrift::prelude::*;
use rill_adrift::rill_core::traits::*;
use rill_adrift::rill_graph::GraphBuilder;
use rill_adrift::rill_oscillators::SineOscNode;

const BUF_SIZE: usize = 256;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Build the graph
    let mut builder = GraphBuilder::<f32, BUF_SIZE>::new();
    let osc = builder.add_source(
        Box::new(SineOscNode::<f32, BUF_SIZE>::new().with_frequency(440.0))
    );
    let sink = builder.add_sink(Box::new(MySink::new()));
    builder.connect_signal(osc, 0, sink, 0);
    let _graph = builder.build()?;

    // The graph is ready. AudioOutput from rill-io drives it:
    // output.set_active(source_idx);
    // output.start(ptr, drain_fn, sample_rate);

    Ok(())
}

// Minimal sink that prints RMS every block
struct MySink<const BUF_SIZE: usize> { .. }
impl Node<f32, BUF_SIZE> for MySink<BUF_SIZE> { .. }
impl Sink<f32, BUF_SIZE> for MySink<BUF_SIZE> { .. }
```

> **Note:** `AudioOutput` / `AudioInput` are in `rill-io` (feature-gated
> behind `io`). For testing without audio hardware, use the `NullBackend`
> or a custom `Sink` implementation.

## Using individual DSP algorithms

For algorithm-level processing without the graph infrastructure:

```rust
use rill_core_dsp::generators::basic::SineOsc;
use rill_core_dsp::delay::Delay;
use rill_core_dsp::algorithm::Algorithm;

let sample_rate = 44100.0;
let mut osc = SineOsc::<f32>::new(440.0, sample_rate);
osc.set_amplitude(0.5);

let mut delay = Delay::<f32>::new(0.3, sample_rate);
delay.set_feedback(0.4);

let mut input = vec![0.0f32; 64];
let mut output = vec![0.0f32; 64];
osc.process_block(&[], &mut input)?;
delay.process_block(&input, &mut output)?;
```

## Audio I/O

Enable the `io` feature on `rill-adrift` (default):

```toml
rill-adrift = { version = "0.5.0-beta.2", features = ["io", "alsa"] }
```

Available backends: `cpal` (default), `alsa`, `pipewire`, `jack`.

The `AudioInput` node (push model) owns the backend and calls
`Source::generate()` on each audio tick. `AudioOutput` (pull model)
drives the graph from the sink side.

```rust
use rill_io::AudioOutput;

let mut output = AudioOutput::<f32, 256>::new();
output.set_backend(ptr);
output.set_active(source_idx);
output.start(nodes_ptr, drain_fn, 44100.0);
```

## Two-Thread Architecture

- **Audio thread** (hard or soft RT) — runs the process callback:
  drain `MpscQueue`, `generate()`, `propagate()`, `consume()`.
  No heap allocs, no locks, no syscalls.
- **Control thread** (tokio green threads) — runs `Manager`
  with automatons (LFO, envelopes, sequencers). Communicates via
  lock-free `MpscQueue<ParameterCommand>`.

## Next steps

- [Architecture Overview](../architecture/overview.md) — core concepts
- [Signal graph (rill-graph)](../architecture/graph.md) — graph processing details
- [The World of Automatons](world-of-automatons.md) — automation system
- [Real-Time Safety](real-time-safety.md) — RT constraints and rules
- [Crates reference](../reference/crates.md) — full crate list with features
