# Getting Started

Add `rill-adrift` to your `Cargo.toml`:

```toml
[dependencies]
rill-adrift = "0.6.0-M2"
```

For individual crates (if you don't need the full ecosystem):

```toml
[dependencies]
rill-core-dsp = "0.6.0-M2"
```

## Example: Signal graph with sine oscillator

This example builds a signal graph with a sine oscillator using the
`GraphBuilder` API and the built-in registry:

```rust,no_run
use rill_graph::GraphBuilder;
use rill_core::builtin::Registry;

const BUF_SIZE: usize = 256;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut reg = Registry::<f32>::new();
    rill_core_dsp::lang::register::register_lang_builtins(&mut reg);
    rill_lang::register::register_core_builtins(&mut reg);

    let mut builder = GraphBuilder::<f32, BUF_SIZE>::new();
    let osc = builder.add_node("rill/sinosc", &[("freq", 440.0)].into());
    let out = builder.add_node("rill/output", &[].into());
    builder.connect_signal(osc, 0, out, 0);

    // build_ir() produces a GraphIr, then internally calls
    // rill_lang::graph_compiler::compile() to produce a CompiledGraphEngine:
    let engine = builder.build_ir(&reg, 44100.0)?;
    let mut buf = [0.0f32; BUF_SIZE];
    engine.process(Some(&[0.0f32; BUF_SIZE]), &mut buf)?;

    Ok(())
}
```

> **Note:** For real I/O, use `Output` / `Input` from `rill-io` (feature-gated
> behind `io`). The orchestration layer creates the backend and drives the
> `CompiledGraphEngine` via `process()`.

## Using rill-lang instead of programmatic graphs

rill-lang provides a Faust-style functional DSL. A program is either an IO
program — `main : (String…) -> IO ()` executed to completion by `ProgramRunner`
— or a pure-value program evaluated one tick at a time:

```rust,no_run
use rill_lang::{compile, compile_runner, Registry};
use rill_core::traits::ParamValue;

// An IO program: streams and console, no buffers handed in from outside.
let stdout = rill_lang::run_on_stack(16 << 20, || {
    let mut runner = compile_runner::<f32, 256>(
        r#"main = do { write (Just "hi") stdout; return () };"#,
        &Registry::new(),
        44_100.0,
    )
    .unwrap();
    runner.run().unwrap();
    runner.stdout().to_vec()
});
assert_eq!(stdout, ["hi"]);

// A pure-value program: one value tick, read from value_outputs().
let mut prog = compile::<f32>("main = 1.0 + 2.0;").unwrap();
prog.run_value().unwrap();
```

Named parameter slots are still available by index
(`param_index` / `set_param`):

```rust,no_run
use rill_lang::compile_with;
use rill_core::traits::ParamValue;

let registry = rill_adrift::lang_builtins::full_registry::<f32>();
let mut prog = compile_with::<f32>(r#"main = _ * param("gain", 0.5);"#, &registry, 44_100.0).unwrap();
if let Some(idx) = prog.param_index("gain") {
    prog.set_param(idx, ParamValue::Float(0.5));
}
```

> **Not in this branch:** `compile_graph()`, `CompiledGraphEngine`, the
> `rill/lang` graph node, and the `MultichannelAlgorithm` transform dispatch are
> removed with the old signal-transform executor. Programs own their I/O as
> streams; a graph host that drives a program per block is follow-on work.

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
use rill_core::traits::Algorithm;

let sample_rate = 44100.0;
let mut osc = SineOsc::<f32>::new(440.0, sample_rate);
osc.set_amplitude(0.5);

let mut delay = Delay::<f32>::new(0.3, sample_rate);
delay.set_feedback(0.4);

# let mut input = [0.0f32; 64];
# let mut output = [0.0f32; 64];
Algorithm::process(&mut osc, None, &mut input)?;
Algorithm::process(&mut delay, Some(&input), &mut output)?;
```

## Signal I/O

Enable the `io` feature on `rill-adrift` (default):

```toml
rill-adrift = { version = "0.6.0-M2", features = ["io", "alsa"] }
```

Available backends: `portaudio` (default/minimal), `alsa` (Linux), `pipewire` (Linux), `jack` (Linux).

The `Input` node (push model) drives the graph from the source side.
`Output` (pull model) drives the graph from the sink side.
The orchestrator creates the backend and drives the `CompiledGraphEngine`
from the I/O callback.

## Two-Thread Architecture

- **Signal thread** (hard or soft RT) — runs `CompiledGraphEngine::process()`.
  No heap allocs, no locks, no syscalls.
- **Control thread** (tokio green threads) — runs `Patchbay`
  with automatons (LFO, envelopes, sequencers). Communicates via
  the actor mailbox (`ActorRef<CommandEnum>`) returned by `engine.handle()`.

## Next steps

- [Architecture Overview](../architecture/overview.md) — core concepts
- [Signal graph (rill-graph)](../architecture/graph.md) — graph processing details
- [The World of Automatons](world-of-automatons.md) — automation system
- [Real-Time Safety](real-time-safety.md) — RT constraints and rules
- [Crates reference](../reference/crates.md) — full crate list with features
