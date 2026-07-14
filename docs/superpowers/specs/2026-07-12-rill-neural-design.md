# rill-neural — Design

## Motivation

Integrate neural network inference as signal-graph nodes in rill without modifying `rill-core` or `rill-patchbay`. The crate models the brain's dual-layer architecture:

| Brain layer            | Rill component      | Mechanism                 |
|------------------------|---------------------|---------------------------|
| Electrical (spikes)    | `rill-graph`/`rill-lang` | `Algorithm` nodes, signal path    |
| Chemical (neuromodulators) | `rill-patchbay`      | `Automaton`/`Servo`, control path |

KVAE-Audio (Sber, MIT) is the first model targeted: an audio tokenizer compressing 1 s of 16 kHz audio into a 25-dimensional continuous latent, encoded as spike trains for downstream graph nodes.

## Constraints

- **Zero modifications** to `rill-core` or `rill-patchbay`.
- `Algorithm` trait for signal-path nodes (read chemical state via `Arc<ModulatorBank>`).
- `Automaton` trait for chemical-layer decision-making (write chemical state via `Arc<ModulatorBank>`).
- RT-safe: no allocations, no locks, no blocking I/O in `Algorithm::process()`.
- Inference runs on a dedicated background thread; signal thread reads results via lock-free `SpscQueue`.

## Crate structure

```
rill-neural/
  Cargo.toml
  src/
    lib.rs
    modulator.rs    — ModulatorBank: RT-safe chemical context
    synapse.rs      — Synapse: weighted connection + STDP + receptor tag
    tokenizer.rs    — KvaeTokenizer: Algorithm with async ONNX inference
    neuron.rs       — RillNeuron: multi-channel gated Algorithm
    encoder.rs      — SpikeEncoder: latent → spike train encoding
    prelude.rs
```

## Dependencies

- `rill-core` (workspace)
- `tract-onnx` — optional, feature `onnx`
- `candle-core` — optional, feature `candle` (future)

## Components

### 1. ModulatorBank (`modulator.rs`)

The chemical context. An array of `AtomicF32` values, indexed by `usize`. Semantics are application-defined (e.g. `0 = dopamine`, `1 = serotonin`).

```rust
pub struct ModulatorBank<const N: usize> {
    levels: [AtomicF32; N],
}

impl<const N: usize> ModulatorBank<N> {
    pub fn read(&self, index: usize) -> f32;
    pub fn write(&self, index: usize, value: f32);
}
```

**Usage:**
- Signal-path `Algorithm` nodes hold `Arc<ModulatorBank<N>>`, call `read()` in `process()` — atomic, RT-safe.
- Patchbay `Automaton` implementations hold `Arc<ModulatorBank<N>>`, call `write()` as a side effect of `step()`.

No enum, no new types. The application defines `const DOPAMINE: usize = 0;`.

### 2. Synapse (`synapse.rs`)

A weighted connection modulated by the chemical layer.

```rust
pub struct Synapse {
    pub weight: AtomicF32,
    pub receptor_tag: usize,               // modulator index
    pub plasticity: Option<StdpConfig>,
}

impl Synapse {
    /// Apply weight modulated by chemical context.
    pub fn fire<T: Scalar>(&self, spike: T, modulator: &ModulatorBank<16>) -> T;
    /// Apply STDP weight update.
    pub fn stdp_apply(&self, pre_t: Instant, post_t: Instant, lr: f32);
}

pub struct StdpConfig {
    pub tau_pre: f32,
    pub tau_post: f32,
    pub a_plus: f32,
    pub a_minus: f32,
}
```

`fire()` multiplies the spike by `weight * modulator.read(receptor_tag)` — silent when neuromodulator level is low, potentiated when high.

### 3. KvaeTokenizer (`tokenizer.rs`)

An `Algorithm` node running KVAE-Audio inference on a background thread.

**Architecture:**
```
Signal Thread (RT)              Background Thread
  DelayLine accumulation ──→ SpscQueue<Vec<f32>>
  SpscQueue::try_pop() ←── tract_model.run()
  SpikeEncoder → output
```

**State:**
- `DelayLine<T, 16000>` — accumulates 1 s at 16 kHz
- `SpscQueue<Vec<f32>>` — sends audio chunks to inference
- `SpscQueue<[f32; 25]>` — receives latent vectors
- `[T; 25]` — cached last latent
- `Box<dyn SpikeEncoder<T>>` — latent-to-spike encoding

**Deterministic latency:** 1 s (one chunk). The signal thread always reads the previous chunk's result. If inference hasn't finished, the previous latent is reused.

**Feature gates:** gated behind `onnx` feature (tract) or `candle` feature (future).

### 4. RillNeuron (`neuron.rs`)

A "neuron as microcontroller": multiple processing channels (rill-lang subgraphs), gated by neuromodulator levels.

```rust
pub struct RillNeuron<T: Transcendental, const CHANNELS: usize> {
    channels: [Box<dyn Algorithm<T>>; CHANNELS],
    gate_map: [usize; CHANNELS],         // modulator_index per channel
    modulator: Arc<ModulatorBank<16>>,
}
```

`process()`:
1. Read each channel's modulator level
2. Run all channels' `Algorithm::process()` into pre-allocated scratch buffers
3. Mix outputs weighted by `modulator_level / sum(modulator_levels)`, normalized to 1.0

Scratch buffers are allocated once at construction — no heap activity on the RT path.

The neuron's behavior changes smoothly as neuromodulator levels shift — chemical layer controls the blend ratio of competing processing paths.

### 5. SpikeEncoder (`encoder.rs`)

Encodes a 25-dimensional latent vector into a spike train output buffer.

```rust
pub trait SpikeEncoder<T: Scalar>: Send + Sync {
    fn encode(&self, latent: &[T; 25], output: &mut [T], sample_rate: f32);
}

pub struct RateEncoder {
    pub max_freq: f32,
}
```

Default: `RateEncoder` — each latent dimension drives a spike frequency proportional to its value. Application can supply custom encoders (temporal, population coding).

## Feature flags

```toml
[features]
default = []
onnx = ["tract-onnx"]
candle = ["candle-core"]
```

Without features, `rill-neural` provides `ModulatorBank`, `Synapse`, `RillNeuron`, and `SpikeEncoder` — zero third-party dependencies beyond `rill-core`. `KvaeTokenizer` requires at least one of `onnx`/`candle`.

## Integration pattern

The application wires everything together:

```rust
let bank = Arc::new(ModulatorBank::<16>::default());

// Signal path: KvaeTokenizer reads audio, encodes to spikes
let tokenizer = KvaeTokenizer::new(bank.clone(), RateEncoder::default());

// Chemical layer: Bayesian automaton writes modulator levels
let automaton = BayesianModulator::new(bank.clone());

// Neuron: gated subgraphs respond to modulator levels
let neuron = RillNeuron::new(bank.clone(), vec![
    (DOPAMINE,   boxed_dopamine_algorithm),
    (SEROTONIN,  boxed_serotonin_algorithm),
]);

// Servo connects automaton to graph (standard rill-patchbay pattern)
let servo = Servo::new(automaton, ControlStrategy::Absolute);
```

None of `rill-core` or `rill-patchbay` is aware of `ModulatorBank` — it's just shared state passed through `Arc` between application-owned components.

## Non-goals

- Training models (rill-neural is inference-only)
- SNN neuron primitives (LIF, Izhikevich) — these belong in application code or future crates
- Dynamic graph topology — structure is fixed at graph construction time; behavior changes via chemical gating, not rewiring
- GPU inference in the RT signal path — inference runs on background thread only

## Model export

KVAE-Audio must be exported to ONNX before use. The kave-audio repository provides PyTorch weights; `torch.onnx.export()` or `optimum-cli` converts them. This is a one-time setup step, not part of `rill-neural`.
