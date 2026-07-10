# Getting Started

Add `rill-adrift` to your `Cargo.toml`:

```toml
[dependencies]
rill-adrift = "0.5.0"
```

For individual crates (if you don't need the full ecosystem):

```toml
[dependencies]
rill-core-dsp = "0.5.0"
```

## Example: Signal graph with sine oscillator

This example builds a signal graph with a sine oscillator and runs it
through the pull model (Sink drives processing):

```rust,no_run
use rill_adrift::prelude::*;
use rill_adrift::rill_core::traits::*;
use rill_adrift::rill_core::time::ClockTick;
use rill_adrift::rill_graph::{GraphBuilder, NodeFactory};
use rill_adrift::rill_oscillators::SineOscNode;
use std::sync::Arc;

const BUF_SIZE: usize = 256;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let factory = Arc::new(NodeFactory::<f32, BUF_SIZE>::new());
    rill_adrift::registration::register_all_nodes(&mut Arc::get_mut(&mut factory.clone()).unwrap());
    let mut builder = GraphBuilder::<f32, BUF_SIZE>::new(factory);
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

> ⚠️ **Deprecated**: The `GraphBuilder::build()` → `ProcessingState` path shown here is the legacy API,
> kept for backward compatibility. For new development, use the `lang` feature:
> `GraphBuilder::build_ir(&registry)` → `RillGraphEngine`.
> 
> See the [rill-lang guide](rill-lang.md) for details.

> **Note:** For real I/O, use `Output` / `Input` from `rill-io` (feature-gated
> behind `io`). The `Output` node (Sink) writes to `IoPlayback`, `Input` (Source)
> reads from `IoCapture`. The orchestrator creates the backend, extracts
> `ProcessingState`, and registers the process callback.

## Using rill-lang instead of programmatic graphs

rill-lang provides a Faust-style functional DSL for signal processing.
Use it to define algorithms declaratively instead of wiring Rust node types:

```rust
use rill_lang::{compile, compile_with, compile_graph};
use rill_core::builtin::Registry;

// Simple compilation to an Algorithm<T>
let mut prog = compile::<f32>("main = _ * 0.5").unwrap();
let mut out = [0.0f32; 64];
prog.process(Some(&[1.0f32; 64]), &mut out).unwrap();

// Compile with a built-in registry (for DSP primitives)
let registry = rill_adrift::lang_builtins::full_registry::<f32>();
let mut prog2 = compile_with::<f32>(
    "main = sine(freq) * env",
    &registry,
    44100.0,
).unwrap();
prog2.set_param("freq", 440.0);
prog2.set_param("env", 0.5);

// Whole-graph compilation with runtime ?name parameters
let engine = compile_graph::<f32>(
    "main = sine(?freq) * ?gain",
    &registry,
    44100.0,
).unwrap();
engine.mailbox_set_param("freq", &rill_core::queues::ParamValue::F32(440.0)).unwrap();
```

## Per-crate registration without rill-adrift

If you depend on individual crates instead of the umbrella `rill-adrift`,
each crate provides its own `register_lang_builtins()` function to populate
a `rill_core::builtin::Registry`:

```rust
use rill_core::builtin::Registry;

let mut reg = Registry::<f32>::new();
rill_core_dsp::lang::register::register_lang_builtins(&mut reg);
rill_router::register::register_lang_builtins(&mut reg);
rill_digital_effects::register::register_lang_builtins(&mut reg);
// Feature-gated crates follow the same pattern:
// rill_fft::register::register_lang_builtins(&mut reg);
// rill_sampler::register::register_lang_builtins(&mut reg);

let mut prog = compile_with::<f32>("main = sine(440) * gain(0.5)", &reg, 44100.0).unwrap();
```

The `rill-adrift` crate provides `full_registry()` as a convenience that
aggregates all available per-crate registries in one call.

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
rill-adrift = { version = "0.5.0", features = ["io", "alsa"] }
```

Available backends: `portaudio` (default/minimal), `alsa` (Linux), `pipewire` (Linux), `jack` (Linux).

The `Input` node (push model) drives the graph from the source side.
`Output` (pull model) drives the graph from the sink side.
The orchestrator creates the backend, extracts `ProcessingState` from the graph,
and registers the process callback.

```rust,no_run
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
- **Control thread** (tokio green threads) — runs `Patchbay`
  with automatons (LFO, envelopes, sequencers). Communicates via
  lock‑free `MpscQueue` (via the graph actor mailbox, `ActorRef<CommandEnum>`).

## Next steps

- [Architecture Overview](../architecture/overview.md) — core concepts
- [Signal graph (rill-graph)](../architecture/graph.md) — graph processing details
- [The World of Automatons](world-of-automatons.md) — automation system
- [Real-Time Safety](real-time-safety.md) — RT constraints and rules
- [Crates reference](../reference/crates.md) — full crate list with features
