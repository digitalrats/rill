# Rill Architecture (version 0.5.0-beta.7)

## General Concept

Rill is a **modular ecosystem** built around a minimal core with traits. Each crate has a clear responsibility and can be used independently. After the major refactoring of 0.5.0-beta.7, all crates use a unified `rill-core`.

```
┌─────────────────────────────────────────────────────────────┐
│                         Products                              │
│  ┌──────────┐                                                │
│  │  drift   │  (effects server for live coding)             │
│  └──────────┘                                                │
├─────────────────────────────────────────────────────────────┤
│                      Infrastructure                            │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐             │
│   │rill-osc │  │rill-graph  │  │rill-patchbay│  │rill- │   │
│   │(OSC server)    │ │(signal graph)│ │(automation) │ │sampler│   │
│   └────────────┘  └────────────┘  └────────────┘  └──────┘   │
├─────────────────────────────────────────────────────────────┤
│                      Audio Processing                        │
│  ┌─────────────────────────────────────────────────────┐    │
│  │    rill-core-dsp (algorithms + vector operations)  │    │
│  │   Algorithm trait, generators, filters, delays       │    │
│  └─────────────────────────────────────────────────────┘    │
│  ┌──────────┐ ┌───────────────┐ ┌───────────────┐ ┌──────┐ │
│  │rill-osc  │ │rill-digital-  │ │rill-digital-  │ │rill- │ │
│  │(oscillator│ │filters        │ │effects        │ │router│ │
│  │ nodes)   │ │(filter nodes) │ │(effect nodes) │ │router│ │
│  │ active   │ │ active        │ │ active        │ │active│ │
│  └──────────┘ └───────────────┘ └───────────────┘ └──────┘ │
│  ┌──────────────────────────────────────────────────────┐   │
│  │              Analog Modeling                          │   │
│  │  ┌──────────────┐ ┌───────────────┐ ┌──────────────┐ │   │
│  │  │rill-core-model │ │rill-analog-   │ │rill-analog-  │ │   │
│  │  │(WDF core)    │ │filters        │ │effects       │ │   │
│  │  │ active       │ │(WdfMoogLadder)│ │(op-amp, tape)│ │   │
│  │  └──────────────┘ └───────────────┘ └──────────────┘ │   │
│  └──────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────┤
│                      Input/Output                            │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐      │
│  │  ALSA    │ │  CPAL    │ │ PipeWire │ │   JACK   │      │
│  │(rill-io) │ │(rill-io) │ │(rill-io) │ │(rill-io) │      │
│  │ active   │ │ active   │ │ active   │ │ active   │      │
│  │          │ │          │ │          │ │          │      │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘      │
├─────────────────────────────────────────────────────────────┤
│                         Core                                  │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                   rill-core                          │    │
│  │  ┌─────────────┐  ┌─────────────┐                  │    │
│  │  │   traits    │  │   queues    │                  │    │
│  │  │(Node,etc.)  │  │(MpscQueue) │                  │    │
│  │  └─────────────┘  └─────────────┘                  │    │
│  │  rill-core-actor (Actor, ActorRef, ActorSystem)   │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

## Unified core: rill-core

### Structure

```
rill-core/
├── src/
│   ├── lib.rs                 # Root module, re-exports
│   ├── prelude.rs             # Prelude for convenient imports
│   ├── config.rs              # Configuration
│   ├── error.rs               # Error system
│   ├── event.rs               # Events and signals
│   ├── graph.rs               # Basic graph types
│   ├── utils.rs               # Utilities
│   ├── traits/
│   │   ├── mod.rs             # Node traits (Node, Source, Processor, Sink)
│   │   ├── node.rs            # Nodes and identifiers
│   │   ├── port.rs            # Ports
│   │   ├── param.rs           # Parameters
│   │   ├── processable.rs     # Processing interface
│   │   └── error.rs           # Trait errors
│   ├── math/
│   │   ├── mod.rs             # Numeric type abstractions
│   │   ├── num.rs             # AudioNum trait
│   │   ├── conversions.rs     # Conversions
│   │   └── functions.rs       # Functions
│   ├── buffer/
│   │   ├── mod.rs             # Buffers (PipeBuffer, FanOutBuffer, etc.)
│   │   ├── buffer_trait.rs    # Buffer trait + FixedBuffer
│   │   ├── pipe.rs            # Direct connections (PipeBuffer)
│   │   ├── fan.rs             # Fan-out and summing (FanOutBuffer/FanInBuffer)
│   │   ├── delay.rs           # Delay line (DelayLine)
│   │   ├── ring.rs            # Ring buffer (RingBuffer)
│   │   ├── tape.rs            # Tape loop (TapeLoop — delay/feedback)
│   │   ├── registry.rs        # ResourceRegistry (shared graph resources)
│   │   ├── storage.rs         # AtomicCell
│   │   └── pool.rs            # Buffer pool
│   ├── queues/
│   │   ├── mod.rs             # Command and telemetry queues
│   │   ├── rt_queue.rs        # Real-time queue
│   │   ├── spsc.rs            # Single-producer single-consumer
│   │   ├── mpsc.rs            # Multi-producer single-consumer
│   │   ├── ring.rs            # Ring queue
│   │   ├── command.rs         # Commands
│   │   ├── telemetry.rs       # Telemetry
│   │   ├── signal.rs          # Signals
│   │   ├── observer.rs        # Observers
│   │   ├── atomic.rs          # Atomic operations
│   │   └── error.rs           # Queue errors
│   ├── time/
│   │   ├── mod.rs             # Time and clock signals
│   │   ├── clock.rs           # Clock and ClockSource traits
│   │   ├── source.rs          # Time source implementations
│   │   ├── tick.rs            # ClockTick (per-block timing)
│   │   ├── render.rs          # RenderContext + TransportState
│   │   └── error.rs           # Time errors
│   ├── macros/
│   │   ├── mod.rs             # Macros
│   │   ├── source.rs          # source_node!
│   │   ├── processor.rs       # processor_node!
│   │   ├── sink.rs            # sink_node!
│   │   ├── params.rs          # Parameters
│   │   ├── ports.rs           # Ports
│   │   └── tests.rs           # Macro tests
│   └── executor/
│       └── mod.rs             # Graph executor
```

### Key core components

#### buffer (buffers)

Provides buffer types for transferring signal data between nodes: `PipeBuffer` (single-threaded 1:1 channel), `FanOutBuffer` (fan-out), `FanInBuffer` (summing), `DelayLine` (delay line), `RingBuffer` (ring buffer), `TapeLoop` (delay/feedback tape), and `FixedBuffer` / `ResourceRegistry` (fixed blocks + shared graph resources). Buffers are fixed-size (`const N` block size) and support usage statistics.

```rust
use rill_core::buffer::PipeBuffer;

// Block size is a const generic (here 4 samples per block).
let mut pipe = PipeBuffer::<f32, 4>::new();
pipe.write(&[1.0, 2.0, 3.0, 4.0]);
if let Some(block) = pipe.read() {
    // consume one [f32; 4] block
    let _ = block;
}
```

#### macros

Contains macros for convenient node creation: `processor!`, `sink!`, `source!`. Simplifies writing custom processors, sources, and sinks without boilerplate code.

```rust
use rill_core::macros::{processor, sink, source};

processor!(Gain, |sample, _| sample * 0.5);
sink!(Logger, |sample, _| println!("{}", sample));
source!(Silence, || 0.0);
```

#### math

Defines the `AudioNum` trait for audio-specific numeric operations (dB conversion, phase wrapping), as well as conversion functions and utilities.

```rust
use rill_core::math::AudioNum;

let db = (-6.0).db_to_linear(); // ≈ 0.501
let phase = 3.0.wrap_phase();   // in range [0, 2π)
```

#### queues

Implements non-blocking command and telemetry queues for communication between the signal graph and the outside world. Contains `CommandQueue`, `TelemetryQueue`, `SignalOrigin`, `MicroControlObserver` and other components for real-time parameter control.

```rust
use rill_core::queues::{CommandQueue, CommandEnum, SetParameter};

let mut queue = CommandQueue::new();
queue.send(CommandEnum::SetParameter(SetParameter {
    node_id: 1,
    param_id: "cutoff".to_string(),
    value: 1000.0,
}));
```

#### time

Time, tempo, and transport abstractions: `RenderContext` (unified per-block context — sample clock, `TransportState` with BPM/playing flag/time signature, `speed_ratio` for hardware clock correction), `SystemClock` (atomic BPM + position), `ClockTick` (per-block timing carried from the backend, including `io_quantum` and `sample_pos`), `ClockSource` trait. `RenderContext` is built by the I/O backend once per block, passed through the entire DAG via `process_block(&ctx)`. Transport state flows from JACK/PipeWire transport or MIDI clock sync into `SystemClock`, which feeds BPM into the context.

File: `rill-core/src/time/render.rs`.

```rust
use rill_core::time::{Clock, SystemClock};

let clock = SystemClock::new(44100.0);
let pos = clock.position_samples();
clock.advance(64);
```

#### error

Crate-level error types `SignalError` and `SignalResult`. Separate from `traits/error.rs` (which contains trait errors) and used across all public core APIs.

```rust
use rill_core::{SignalError, SignalResult};

fn safe_process() -> SignalResult<()> {
    Ok(())
}
```

#### prelude

Convenient re-export of the most commonly used types from all core modules. It is recommended to import `use rill_core::prelude::*;` in user code.

```rust
use rill_core::prelude::*;
// Now available: Node, AudioNum, PipeBuffer, CommandQueue, Clock, etc.
```

### `rill-core-actor` ( actor model)

Domain-agnostic actor model infrastructure for lock-free message passing. Provides `Actor<M>` (handler + mailbox, drained in place), `ActorRef<M>` (thread-safe send-only handle), and `ActorSystem` (named registry with `spawn` / `spawn_detached` / `spawn_detached_tokio`, `route`, `broadcast`, dead letters). Generic over any `M: Send + 'static` with no coupling to audio or signal processing.

The mailbox (`Arc<Mailbox<M>>` wrapping a lock-free `MpscQueue<M>`) is the hard RT boundary — `send()` is lock-free and safe from any thread, while `drain()` runs on the consumer's thread and inherits its RT constraints.

```rust
use rill_core_actor::ActorSystem;

let system = ActorSystem::new();
let mut actor = system.spawn("echo", |msg: String| println!("got: {msg}"));
actor.actor_ref().send("hello".into());
actor.drain(); // processes "hello"
```

## Infrastructure crates


### `rill-graph` (0.5.0-beta.7)
Audio graph with topological sort.

```rust
let mut graph = Graph::new(44100.0);
let osc_id = graph.add_node(Box::new(SineOsc::new(440.0)));
let filter_id = graph.add_node(Box::new(BiquadFilter::lowpass(1000.0, 0.707)));

graph.connect(PortId::output(osc_id, 0), PortId::input(filter_id, 0), 1.0)?;

// Automatic topological sort
for &node_id in graph.processing_order() {
    // nodes in correct order
}
```

#### Audio graph architecture

The Rill graph is built on a rigorous mathematical foundation — **category theory**, which ensures type safety, compositionality, and predictable behavior.

**Key concepts:**

- **Objects** — fixed-size sample blocks (`[T; BUF_SIZE]`), control values (`Control`), and clock signals (`Clock`).
- **Arrows (morphisms)** — processors that transform blocks (sources `Source`, processors `Processor`, sinks `Sink`).
- **Composition** — sequential node connections form a processing chain.
- **Product** — parallel processing of multiple signals (e.g., multi-channel mixer).

**Port types:** each port is characterized by signal type (`Audio`, `Control`, `Clock`, `Feedback`, `Param`), direction (input/output), and index.

**Topological sort:** the graph automatically determines node processing order, excluding cyclic dependencies (except for intentional feedback loops).

**Real-time:** all graph operations are guaranteed to execute in bounded time, which is critical for audio applications.

**Block processing:** data is transferred in fixed-size blocks, improving performance through cache locality and enabling SIMD optimizations.

### `rill-patchbay` (0.5.0-beta.7, ✅ active)
Graph parameter automation — unification of `rill-automation` and `rill-control` crates. A central framework of automatons (LFO, envelopes, random walks, sequencers), sensors (acoustic, physical), and servos connected via non-blocking command and telemetry queues. See the "World of Automatons" section for details.

```rust
use rill_patchbay::prelude::*;
use rill_core::queues::MpscQueue;
use std::sync::Arc;

// Create command queue and Engine
let cmd_queue = Arc::new(MpscQueue::new(1024));
let mut control = Engine::new(cmd_queue);

// Add LFO servo
control.add_lfo(
    "vibrato", 5.0, 0.5, 0.0, LfoWaveform::Sine,
    osc_node_id, "frequency", 400.0, 480.0,
);

// Add ADSR servo
control.add_envelope(
    "amp", 0.01, 0.1, 0.7, 0.2,
    vca_node_id, "gain", 0.0, 1.0,
);

    // External event mapping (MIDI, OSC)
control.add_mapping_str(
    "midi:7:1",
    filter_node_id, "cutoff",
    20.0, 20000.0, Transform::Logarithmic,
);

// Update automatons in a loop
control.update(1.0 / 60.0);
```

Or via `Manager` with a separate update thread:

```rust
let mut manager = Manager::new(
    Config::default(),
    Arc::new(MpscQueue::new(1024)),
);

manager.add_lfo_servo(
    "vibrato", 5.0, 0.5, 0.0, LfoWaveform::Sine,
    osc_node_id, "frequency",
    ParameterMapping::Linear, 400.0, 480.0,
)?;
manager.start()?;  // Automatons begin their own life
```




## DSP infrastructure

### `rill-core-dsp` (0.5.0-beta.7)
Unified DSP infrastructure with vector operations, algorithms, and macros. Includes:

- **Vector abstractions** (`ScalarVector1`, `ScalarVector2`, `ScalarVector4`) — generic numeric types for portable SIMD operations
- **Generators** (`generators/`) — oscillators (sine, saw, triangle, square, pulse), noise, LFO, FM synthesis, envelopes
- **Filters** (`filters/`) — biquad, one-pole, SVF, Butterworth, Chebyshev, comb filters
- **Delay algorithms** (`delay`) — Delay, MultiTapDelay, DiffusionDelay, ModulatedDelay
- **Macros** (`macros/`) — `simple_algorithm!`, `parameterized_algorithm!`, `filter_algorithm!`, `effect_algorithm!`, `generator_algorithm!` for rapid algorithm creation
- **`Algorithm` trait** — unified interface for all DSP components with block processing (`process_block`)

All components use `AudioNum` abstractions from `rill-core/math` and vector operations, ensuring portability and performance.

```rust
use rill_core::math::AudioNum;
use rill_core_dsp::generators::basic::SineOsc;
use rill_core_dsp::filters::{BiquadFilter, FilterParams, FilterType};
use rill_core_dsp::algorithm::Algorithm;

let sample_rate = 44100.0;

// Create sine oscillator
let mut osc = SineOsc::<f32>::new(440.0, sample_rate);
osc.set_amplitude(0.5);

// Create biquad low-pass filter
let mut filter = BiquadFilter::<f32>::new(FilterParams {
    filter_type: FilterType::LowPass,
    cutoff: 1000.0,
    q: 0.707,
    gain_db: 0.0,
});

// Process data block
let mut input = vec![0.0f32; 64];
let mut output = vec![0.0f32; 64];
osc.process_block(&[], &mut input);
filter.process_block(&input, &mut output);
```

### `rill-oscillators` (0.5.0-beta.7, ✅ active)
Graph nodes for oscillators (sine, saw, triangle, square, pulse), noise, LFO, and envelopes. Implements `Source`/`Processor` traits from `rill-core`, using DSP algorithms from `rill-core-dsp::generators` and `ScalarVectorN<T>` vector abstractions.

### `rill-digital-filters` (0.5.0-beta.7, ✅ active)
Graph nodes for digital filters: biquad, one-pole, SVF, Butterworth, Chebyshev, comb. Implements the `Processor` trait from `rill-core` based on DSP algorithms from `rill-core-dsp::filters`.

### `rill-digital-effects` (0.5.0-beta.7, ✅ active)
Graph nodes for digital effects: Delay, Distortion, Limiter. Implements the `Processor` trait from `rill-core`, using delay algorithms from `rill-core-dsp::delay`. Optional `modulation` feature enables `rill-oscillators` for LFO modulation.

### `rill-router` (0.5.0-beta.7)
Router combining equalizer (`rill-eq`) and mixer (`rill-mixer`) functionality with matrix routing capabilities. Includes `eq` (graphic and parametric equalizers) and `mixer` (mixer with channels, sends, master) modules. A `matrix` module is planned for flexible signal routing.

```rust
use rill_router::eq::{GraphicEq, ParametricEq};
use rill_router::mixer::{MixerNode, ChannelConfig};

let mut eq = GraphicEq::new(44100.0);
eq.set_band_gain(0, 3.0)?;

let mut mixer = MixerNode::new(4, 2);
mixer.set_channel_pan(0, -0.5)?;
mixer.set_channel_volume(1, 0.8)?;
```

## Specialized crates

### `rill-lofi` (0.5.0-beta.7, ✅ active)
Lo-Fi emulation of classic systems (NES, AY-3-8910, Akai S900). Implements graph nodes (`Node`) based on `rill-core`, using internal DSP algorithms to emulate bit depth, sample rate, and characteristic noise of retro systems.

```rust
// NES emulator
let mut nes = NesEmulator::new(44100.0);

// Akai S900 (12-bit)
let akai_config = LofiConfig::for_system(ClassicSystem::AkaiS900);
let mut akai = LofiProcessor::new(akai_config);
```

### `rill-telemetry` (0.5.0-beta.7, ✅ active)
Probes and data collectors for monitoring audio flow and control. Provides mechanisms for collecting performance statistics, tracking real-time safety violations, and providing feedback for external systems.

### `rill-core-model` (0.5.0-beta.7, ✅ active)
WDF core + physical modeling — elements (Resistor, Capacitor, Inductor, Diode, OpAmp), adapters (SeriesAdapter, ParallelAdapter), analysis functions (frequency response, distortion), WDF filters (MoogLadder, DiodeClipper), tape models (RecordHead, PlaybackHead), and resonant physical models (StringModel — 1D waveguide, PlateModel — 2D FDTD mesh, ModalModel — parallel filter bank, HelmholtzCavity + CavityArray). Generic over `rill_core::Transcendental` — supports `f32` and `f64`.

```rust
use rill_core_model::{Resistor, Capacitor, WdfElement, WaveVariables};
use rill_core_model::wdf::{MoogLadder, RcPole};
use rill_core::traits::Algorithm;

let mut cap: Capacitor<f64> = Capacitor::new(0.1e-6, 44100.0);
let a = cap.port_resistance();

let pole = RcPole::new(0.0.into());
let mut ladder: MoogLadder<f64> = MoogLadder::new(pole, 1000.0.into(), 0.0.into(), 44100.0.into());
ladder.set_cutoff(5000.0.into());
ladder.set_resonance(0.7.into());
let y = ladder.process_sample(0.5.into());
```

### `rill-analog-filters` (0.5.0-beta.7, ✅ active)
WDF-based analog filters. Includes `WdfMoogLadderProcessor` — a Node wrapper around `rill_core_model::wdf::MoogLadder<f64>`. Provides graph nodes for the processor.

```rust
use rill_analog_filters::WdfMoogLadderProcessor;

let mut processor = WdfMoogLadderProcessor::<f32, 64>::new(44100.0);
processor.set_parameter(&ParameterId::new("cutoff").unwrap(), ParamValue::Float(5000.0));
```

### `rill-analog-effects` (0.5.0-beta.7, ✅ active)
Analog circuit models: operational amplifiers (OperationalAmplifier with slew-rate, bandwidth, rail-clamping), cassette decks (CassetteDeckModel with tape saturation emulation, wow and flutter, noise), preamps. Depends on `rill-core` and `rill-core-model`.

```rust
use rill_analog_effects::OperationalAmplifier;

let mut opamp = OperationalAmplifier::new(44100.0);
opamp.set_slew_rate(0.5);
let output = opamp.process(0.3);
```

### `rill-io` (0.5.0-beta.7, active)
Audio input/output. Pure I/O backends — no engine, no processors.

Single trait:

- **`IoDriver`** — drives the graph: `set_process_callback(FnMut(&ClockTick))`, `run`, `stop`
- **`IoCapture`** — reads input: `read_input(channel, &mut [f32])`, `num_input_channels()`
- **`IoPlayback`** — writes output: `write_output(channel, &[f32])`, `num_output_channels()`
- **`IoControl`** — optional: `write_data(&[u8])` for chip register writes

A single backend struct (e.g. `PipewireBackend`) implements `IoDriver` and
optionally `IoCapture` / `IoPlayback`.  The driver owns the timing loop.

**Buffer sizing.** Callback-driven backends request a DMA buffer of
`buffer_size × AudioConfig::buffer_blocks` (default 16 × 256 = 4096 frames) and
chunk it back into `block_size` pieces in the callback, driving `process_block`
once per piece and emitting one `ClockTick` per rill block. A single 256-frame
period is unstable through PipeWire (crackling / xruns), so PipeWire negotiates
the bounded size via a `SPA_PARAM_Buffers` object on connect (instead of its
~12288-frame default) and PortAudio passes it as `frames_per_buffer`. The buffer
duration is also the async-control look-ahead (`ClockTick.io_quantum` ≈ 93 ms at
16 blocks), so `buffer_blocks` trades control latency against stability
(poll-driven ALSA and JACK ignore it).

Two graph nodes:

- **`Input`** (Source) — holds `Arc<dyn IoCapture>`, calls `read_input()` directly
  in `generate()`.  No longer depends on `ClockTick` for I/O.
- **`Output`** (Sink) — holds `Arc<dyn IoPlayback>`, calls `write_output()` directly
  in `consume()`.  Same pattern.

Backends are created by the orchestrator **before** graph construction.
`ProcessingState::wire_backends(capture, playback)` injects the backends
into Source / Sink nodes via `Source::set_capture()` / `Sink::set_playback()`.
The driver is wired separately via `ProcessingState::run_with_driver()`.

The graph is `!Send + !Sync` — it stays on the I/O callback thread.

### Graph processing

The graph has no external engine. `ProcessingState::process_block(&tick)`:

1. Re-initialises nodes if the backend's hardware rate differs (chip clocks,
   filter coefficients etc.)
2. Drains the actor mailbox (applies queued `SetParameter` commands — writes
   with `sample_pos` are deferred and applied during the block that contains
   their target position; writes without it apply immediately for live UI/MIDI)
3. Calls `source.process_block(&ctx, &tick)` — Source fills output ports
4. `Port::propagate()` — recursive DAG traversal, zero-copy for 1:1 connections
5. `send_clock_tick(&tick)` — forwards the tick to modules (gated by
   `tick.is_final`; chunking backends leave `is_final = true` on every
   `block_size` chunk, so modules receive one tick per block — sample-accurate
   downstream placement uses `SetParameter.sample_pos` + `ClockTick.io_quantum`)

No external loop. Two-thread architecture:
I/O callback thread (hard or soft RT) + control thread (tokio, patchbay).

## Key architectural principles

1. **Unified core** — `rill-core` unifies all base traits and the signal system
2. **Minimal dependencies** — each crate depends only on what it actually uses
3. **Modularity** — each crate has a clear responsibility
4. **Composition** — complex nodes are built from simple ones
5. **Performance** — zero-cost abstractions, real-time safety
6. **Testability** — all components are tested in isolation

## Crate dependencies (version 0.5.0-beta.7)

Dependency diagram between crates (solid arrows — mandatory dependencies, dashed — optional):

```mermaid
graph TD
    CORE[rill-core] --> CORE_DSP[rill-core-dsp]
    CORE --> CORE_ACTOR[rill-core-actor]
    CORE --> GRAPH[rill-graph]
    CORE_DSP --> OSC[rill-oscillators]
    CORE_DSP --> FILTERS[rill-digital-filters]
    CORE_DSP --> EFFECTS[rill-digital-effects]
    CORE_DSP --> ROUTER[rill-router]
    CORE --> PATCHBAY[rill-patchbay]
    CORE --> IO[rill-io]
    CORE --> LOFI[rill-lofi]
    CORE --> TELEMETRY[rill-telemetry]
    CORE --> ANALOG_FILTERS[rill-analog-filters]
    CORE --> ANALOG_EFFECTS[rill-analog-effects]
    CORE --> CORE_WDF[rill-core-model]
    CORE_WDF --> ANALOG_FILTERS
    CORE_WDF --> ANALOG_EFFECTS
    
    style CORE fill:#90ee90
    style CORE_DSP fill:#90ee90
    style CORE_ACTOR fill:#90ee90
    style GRAPH fill:#90ee90
    style OSC fill:#90ee90
    style FILTERS fill:#90ee90
    style EFFECTS fill:#90ee90
    style ROUTER fill:#90ee90
    style PATCHBAY fill:#90ee90
    style IO fill:#90ee90
    style LOFI fill:#90ee90
    style TELEMETRY fill:#90ee90
    style CORE_WDF fill:#90ee90
    style ANALOG_FILTERS fill:#90ee90
    style ANALOG_EFFECTS fill:#90ee90
    
    %% Planned
    SERVER[rill-osc<br/>(OSC server)]
    
    CORE -.-> SERVER
    
    style SERVER fill:#cccccc
```

## World of Automatons

**Rill Patchbay** is not just a control system. It is a **world** where **automatons** live — mysterious beings that sense the environment and influence it. They communicate in the language of signals, hear sound through sensors, and affect the Graph through servos.

### 🧠 World architecture

```
┌─────────────────────────────────────────────────────┐
│             WORLD OF AUTOMATONS                      │
│  (your Rill application)                       │
│                                                       │
│  ┌─────────────────────────────────────────────────┐ │
│  │                    PATCHBAY                       │ │
│  │  ┌─────────────────────────────────────────┐    │ │
│  │  │          AUTOMATONS (mind)             │    │ │
│  │  │  ┌──────────┐  ┌──────────┐  ┌──────────┐ │ │
│  │  │  │   LFO    │  │   ENV    │  │  RANDOM  │ │ │
│  │  │  └────┬─────┘  └────┬─────┘  └────┬─────┘ │ │
│  │  │       │             │             │       │ │
│  │  └───────┼─────────────┼─────────────┼───────┘ │ │
│  │          │             │             │         │ │
│  │          ▼             ▼             ▼         │ │
│  │  ┌─────────────────────────────────────────┐   │ │
│  │  │           SENSORS (senses)              │   │ │
│  │  │  • Hear sound (acoustic)           │   │ │
│  │  │  • Feel touch (physical) │   │ │
│  │  │  • See MIDI/CV                         │   │ │
│  │  └─────────────────────────────────────────┘   │ │
│  │                   │                              │ │
│  │                   │ Signals                      │ │
│  │                   ▼                              │ │
│  │  ┌─────────────────────────────────────────┐   │ │
│  │  │           SERVO (hands)                   │   │ │
│  │  │    Apply signals to Graph       │   │ │
│  │  └─────────────────────────────────────────┘   │ │
│  └──────────────────────┬──────────────────────────┘ │
│                         │ Non-blocking queues        │
│                         ▼ (Command/Telemetry)        │
│  ┌─────────────────────────────────────────────────┐ │
│  │                 AUDIOGRAPH                        │ │
│  │          (internal device schematic)            │ │
│  │                                                   │ │
│  │  Oscillators → Filters → Effects → Mixer        │ │
│  └─────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────┘
```

### 🦾 Automatons — mind (Automaton)

Automatons are intelligent beings that make decisions and generate signals. They can be simple (LFO, envelope) or complex (logic circuits, mathematical transformers).

| Automaton | Description | Code example |
|---------|----------|---------------------|
| **LFO** | Pulses at a given frequency | `LfoAutomaton::new("vibrato", 5.0, 0.5, 0.0, LfoWaveform::Sine)` |
| **Envelope** | Reacts to events (triggers) | `EnvelopeAutomaton::adsr("amp", 0.01, 0.1, 0.7, 0.2)` |
| **Random Walk** | Wanders randomly | `RandomAutomaton::walk("chaos", 10.0)` |
| **Sequencer** | Plays a sequence of steps | `SequencerAutomaton::new("seq", steps)` |
| **Function** | Arbitrary time function | `FunctionAutomaton::new("math", \|t\| (t * 0.5).sin())` |
| **Cellular** | Cellular automaton (Game of Life, Rule 30) | `CellularAutomaton::game_of_life("life", 16, 16)` |

### 👁️ Sensors — senses

For automatons to perceive the world, they need sensory organs. Sensors
convert external stimuli into signals understandable by automatons.

#### Acoustic sensors (hear sound)

```rust
// Hears pitch
let pitch = AcousticSensor::new("pitch", 
    Box::new(PitchDetector::new(44100.0)))
    .listening_to("osc1_out");  // Listens to oscillator output

// Hears loudness
let envelope = AcousticSensor::new("envelope",
    Box::new(EnvelopeFollower::new(44100.0)
        .with_attack(0.01)
        .with_release(0.1)))
    .listening_to("vca_out");

// Hears rhythm (zero crossings)
let rhythm = AcousticSensor::new("rhythm",
    Box::new(ZeroCrossing::new(44100.0)))
    .listening_to("kick_out");
```

#### Physical sensors (feel touch)

```rust
// Front panel knob
let cutoff = PhysicalSensor::knob("filter_cutoff")
    .with_range(20.0, 20000.0)
    .with_curve(KnobCurve::Logarithmic);

// Button
let button = PhysicalSensor::button("arpeggio_on");

// Switch
let mode = PhysicalSensor::switch("filter_mode")
    .with_positions(vec!["LPF", "BPF", "HPF"]);
```

#### MIDI/CV/OSC sensors (see the outside world)

External protocol sensors decode hardware input into `ControlEvent`s and
dispatch them to servos for parameter mapping.

**MIDI sensors** (feature `midi`):

```rust
use rill_patchbay::{spawn_midi_sensor, MidiHub};
use rill_core_actor::{ActorSystem, ActorRef};
use rill_core::queues::CommandEnum;

// Actor-model: spawns polling thread, sends events to servo
let sensor_ref = spawn_midi_sensor(
    "keyboard",
    Box::new(MidirBackend::new("rill-midi")?),
    &system,
    servo_ref,
);
```

**OSC sensors** (feature `osc`):

```rust
use rill_patchbay::{spawn_osc_sensor, OscSensor};
use std::net::SocketAddr;

// Actor-model: binds UDP socket, decodes OSC → ControlEvent::Osc
let sensor_ref = spawn_osc_sensor(
    "touchosc",
    SocketAddr::from(([0, 0, 0, 0], 9000)),
    &system,
    servo_ref,
);

// Legacy Sensor trait path
let osc = OscSensor::new("touchosc", "0.0.0.0:9000".parse().unwrap());
osc.attach(events_ref);
osc.start();
```

### 🎯 Servo — hands

Servos are the **actuators** of automatons. Obeying the laws of nature (non-blocking queues), they transmit signals from the world of automatons to the Graph, changing sound parameters.

```rust
// Servo controlling filter cutoff
let filter_servo = Servo::new(
    "filter_servo",
    lfo_automaton,          // Which automaton provides the signal
    filter_node_id,         // Node ID in Graph
    "cutoff",               // Parameter name
    ParameterMapping::Linear,
    20.0, 20000.0           // Value range
);
```

### ⚡ Laws of nature (non-blocking queues)

The world of automatons and the world of sound exist in parallel. They are connected by **non-blocking queues**:

- **Command Queue** — servos send commands to the Graph
- **Telemetry Queue** — sensors receive data from the Graph

This allows automatons to "think" at their own pace without interfering with the audio stream.

### 🏭 Automaton Space (Patchbay)

**Patchbay** is the place where all your automatons live, where their senses and hands are located.

```rust
use rill_patchbay::prelude::*;
use rill_core::queues::MpscQueue;
use std::sync::Arc;

// Create command queue and Engine
let cmd_queue = Arc::new(MpscQueue::new(1024));
let mut control = Engine::new(cmd_queue);

// Add LFO servo (mind + hands)
control.add_lfo(
    "vibrato", 5.0, 0.5, 0.0,
    LfoWaveform::Sine,
    osc_node_id, "frequency",
    400.0, 480.0,
);

// Add ADSR servo
control.add_envelope(
    "amp", 0.01, 0.1, 0.7, 0.2,
    vca_node_id, "gain",
    0.0, 1.0,
);

// Update automatons in a loop
loop {
    control.update(1.0 / 60.0);
    std::thread::sleep(std::time::Duration::from_millis(16));
}
```

Or via `Manager` with a separate update thread:

```rust
let mut manager = Manager::new(
    Config::default(),
    Arc::new(MpscQueue::new(1024)),
);

manager.add_lfo_servo(
    "vibrato", 5.0, 0.5, 0.0, LfoWaveform::Sine,
    osc_node_id, "frequency",
    ParameterMapping::Linear, 400.0, 480.0,
)?;

manager.start()?;  // Automatons begin their own life
```

## Plans for future versions

- 🔌 **rill-core-dsp** — new DSP algorithms, SIMD optimization (activated via `simd` feature)
- 🧩 **Analog modeling** — expanding WDF element library and physical models
- 🧪 **Cross-crate integration tests** — end-to-end tests spanning multiple crates
- 📦 **rill-sampler** — WAV loading, time-series playback, streaming from disk

### 🧪 Testing

Rill uses a comprehensive testing system. To run all tests, execute:

```bash
# All tests
cargo test --workspace

# Test a specific crate
cargo test -p rill-patchbay

cargo test -p rill-digital-effects
```

### 📚 Documentation

- [README.md](README.md) — project overview and quick start
- [Project Architecture](architecture.md) — detailed description of all components
- [Development Plan](plans/two_thread_architecture.md) — two-thread architecture
- [Examples](examples/) — usage examples for each crate

### 📄 License

The project is distributed under the **Apache 2.0** license. This means you can:

- ✅ Use in commercial products
- ✅ Modify and distribute
- ✅ Use patent rights
- ❗ Attribute authors when making changes
- ❗ Retain copyright notice

Full license text: [LICENSE.md](../LICENSE.md)

## Summary

- **Stable core** — unified `rill-core` crate with clear API boundaries
- **DSP infrastructure** — `rill-core-dsp` provides the `Algorithm` trait and implementations (generators, filters, delay) with vector operations; specialized crates provide graph nodes
- **Vector abstractions** — `ScalarVectorN<T>` for portable SIMD across x86 and ARM
- **Clean modularity** — each crate has a single responsibility, composable independently
- **Real-time safe** — zero-allocation hot path, lock-free SPSC queues, no syscalls
- **Well-tested** — 487 unit tests across the workspace
- **Extensible** — add custom algorithms via macros or the `Algorithm` trait, register custom graph nodes via `NodeFactory`