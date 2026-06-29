# Getting Started

Add `rill-adrift` to your `Cargo.toml`:

```toml
[dependencies]
rill-adrift = "0.5.0-beta.6"
```

For individual crates (if you don't need the full ecosystem):

```toml
[dependencies]
rill-core-dsp = "0.5.0-beta.6"
```

## Example: Signal graph with sine oscillator

This example builds a signal graph with a sine oscillator and runs it
through the pull model (Sink drives processing):

```rust
use rill_adrift::prelude::*;
use rill_adrift::rill_core::traits::*;
use rill_adrift::rill_core::time::ClockTick;
use rill_adrift::rill_graph::GraphBuilder;
use rill_adrift::rill_oscillators::SineOscNode;

const BUF_SIZE: usize = 256;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = GraphBuilder::<f32, BUF_SIZE>::new();
    let osc = builder.add_source(Box::new(
        SineOscNode::<f32, BUF_SIZE>::new().with_frequency(440.0)
    ));
    let sink = builder.add_sink(Box::new(MySink::new()));
    builder.connect_signal(osc, 0, sink, 0)?;
    let graph = builder.build()?;

    let mut state = graph.into_processing_state();
    let tick = ClockTick::new_block(0, BUF_SIZE as u32, 44100.0);
    state.process_block(&tick)?;

    Ok(())
}
```

> **Note:** For real I/O, use `Output` / `Input` from `rill-io` (feature-gated
> behind `io`). The `Output` node (Sink) writes to `IoPlayback`, `Input` (Source)
> reads from `IoCapture`. The orchestrator creates the backend, extracts
> `ProcessingState`, and registers the process callback.

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

## Signal I/O

Enable the `io` feature on `rill-adrift` (default):

```toml
rill-adrift = { version = "0.5.0-beta.6", features = ["io", "alsa"] }
```

Available backends: `portaudio` (default), `alsa` (Linux), `pipewire` (Linux), `jack` (Linux).

The `Input` node (push model) drives the graph from the source side.
`Output` (pull model) drives the graph from the sink side.
The orchestrator creates the backend, extracts `ProcessingState` from the graph,
and registers the process callback.

```rust
use rill_io::{BackendFactory, BackendParams};
use std::sync::{Arc, atomic::AtomicBool};

let factory = BackendFactory::new();
let output = factory.create_output("portaudio", &BackendParams::default())?;
let mut state = graph.into_processing_state();
state.wire_backends(None, Some(output.playback));
state.run_with_driver(output.driver, Arc::new(AtomicBool::new(true)))?;
```

## Two-Thread Architecture

- **Signal thread** (hard or soft RT) — runs the process callback:
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
