# Per-crate Self-Registration — Design

> **Status:** Design — awaiting user review, then implementation plan.
> **Date:** 2026-07-10
> **Scope:** Move `Registry<T>` + `BuiltinSig` + `BlockBuiltin`/`SampleBuiltin` traits from `rill-lang` to `rill-core`. Move registration functions from `rill-adrift` into DSP crates. Add `register_graph_nodes()` + `register_lang_builtins()` per crate. rill-adrift becomes thin aggregator. Minimal SISO fallback when `rill-router` is absent.

## Motivation

Currently all registration — both `NodeFactory` (graph nodes) and `Registry` (lang built-ins) — is centralized in `rill-adrift`. To register a new oscillator, a user must:

1. Add their crate to `rill-adrift/Cargo.toml`
2. Add `register_my_crate()` to `registration.rs`  
3. Import internal types from the crate
4. Write wrapper boilerplate for each node/built-in

This makes `rill-adrift` a mandatory dependency and a bottleneck for extensibility. Users who want only oscillators + filters shouldn't need to pull in `rill-adrift`.

The goal: each DSP crate is **self-sufficient for registration**. A user imports the crate, calls its `register_*()` function, and gets everything wired — no `rill-adrift` required. `rill-adrift` remains as an optional convenience aggregator.

## Confirmed decisions

| Dimension | Decision |
|---|---|
| **`Registry<T>` location** | Move from `rill-lang` to `rill-core` — DSP crates only need `rill-core` |
| **`rill-core-dsp`** | Exception: always provides `register_lang_builtins()` — no feature flag |
| **Other DSP crates** | Registration behind feature `"graph"` (NodeFactory) and/or `"lang"` (Registry) |
| **`rill-lang` backward compat** | `pub use rill_core::builtin::*` — existing code unchanged |
| **`NodeFactory` location** | Stays in `rill-graph` — DSP crates optionally depend on `rill-graph` |
| **`MultichannelAlgorithm`** | Feature-gated behind `"router"` — without `rill-router`, system is purely SISO |
| **`rill-adrift` role** | Thin aggregator: calls each crate's `register_*()` function. No internal type knowledge |

## Architecture

### Layer 0: Foundation (`rill-core`)

```rust
// rill-core/src/builtin.rs  ← MOVED from rill-lang
// rill-core/src/traits/builtin/ — or co-located

pub trait SampleBuiltin<T: Transcendental>: Send + Sync { ... }
pub trait BlockBuiltin<T: Transcendental>: Algorithm<T> + Send + Sync { ... }
pub struct BuiltinSig { name, params: Vec<ParamType>, signal_outs, kind }
pub enum ParamType { Signal, Float, Int, String, Bool, Record(...), Enum(...), Variadic(...) }
pub struct Registry<T: Transcendental> { ... }
```

`rill-lang` re-exports via `pub use rill_core::builtin::*`. No API break.

### Layer 1: Self-registering DSP crates

```
rill-core-dsp  ─── всегда, без feature-флагов
├── OnePoleBuiltin, MoogBuiltin, BiquadBuiltin    ← from rill-adrift
├── SineBuiltin, SawBuiltin, NoiseBuiltin          ← from rill-adrift
├── register_lang_builtins(&mut Registry<T>)        ← фильтры + генераторы
└── [deps] rill-core (exists)

rill-oscillators  ─── feature "graph"
├── register_graph_nodes(&mut NodeFactory<f32, BUF>)
│   └── "rill/sine", "rill/saw", "rill/noise"
└── [deps] rill-core, rill-core-dsp, rill-graph (optional)

rill-digital-filters  ─── feature "graph"
├── register_graph_nodes(&mut NodeFactory<f32, BUF>)
│   └── "rill/biquad", "rill/moog_ladder"
└── [deps] rill-core, rill-core-dsp, rill-graph (optional)

rill-router  ─── feature "graph"
├── register_graph_nodes(&mut NodeFactory<f32, BUF>)
│   └── "rill/mixer", "rill/parametric_eq", ...
└── [deps] rill-core, rill-core-dsp, rill-graph (optional)

rill-fft  ─── feature "graph" | feature "lang"
├── register_graph_nodes(&mut NodeFactory)      [graph]
├── register_lang_builtins(&mut Registry<T>)    [lang]
└── SpectralGateBuiltin, SpectralDelayBuiltin    ← from rill-adrift

rill-lofi  ─── feature "graph" | feature "lang"
rill-core-model / analog-*  ─── feature "graph" | feature "lang"

rill-lang  ─── (компилятор, НЕ крейт регистрации)
├── pub use rill_core::builtin::*                 ← re-export
├── register_core_builtins(&mut Registry<T>)       ← mixer, eq, dry_wet, complex
├── builtins/{mixer,eq,dry_wet}                   ← stay here (no DSP deps)
├── MultichannelAlgorithm impls                   ← behind feature "router"
└── [deps] rill-core, rill-core-actor, опц. rill-router
```

### Layer 2: Thin aggregator (`rill-adrift`)

```rust
// rill-adrift/src/registration.rs

pub fn register_all_nodes<const BUF: usize>(factory: &mut NodeFactory<f32, BUF>) {
    rill_oscillators::register_graph_nodes(factory);
    rill_digital_filters::register_graph_nodes(factory);
    rill_digital_effects::register_graph_nodes(factory);
    rill_router::register_graph_nodes(factory);
    rill_patchbay::register_graph_nodes(factory);
    #[cfg(feature = "sampler")] rill_sampler::register_graph_nodes(factory);
    #[cfg(feature = "lofi")]    rill_lofi::register_graph_nodes(factory);
    #[cfg(feature = "fft")]     rill_fft::register_graph_nodes(factory);
    #[cfg(feature = "analog")] {
        rill_core_model::register_graph_nodes(factory);
        rill_analog_filters::register_graph_nodes(factory);
        rill_analog_effects::register_graph_nodes(factory);
    }
}

// rill-adrift/src/lang_builtins.rs → DELETED (or reduced to aggregator)

pub fn full_registry<T: Transcendental>() -> Registry<T> {
    let mut reg = Registry::new();
    rill_core_dsp::register_lang_builtins(&mut reg);   // always
    rill_lang::register_core_builtins(&mut reg);        // mixer, eq, dry_wet, complex
    #[cfg(feature = "fft")]     rill_fft::register_lang_builtins(&mut reg);
    #[cfg(feature = "lofi")]    rill_lofi::register_lang_builtins(&mut reg);
    #[cfg(feature = "analog")]  rill_core_model::register_lang_builtins(&mut reg);
    reg
}
```

### Layer 3: MultichannelAlgorithm — feature-gated behind `"router"`

**Fact**: Without `rill-router`, the signal graph has only SISO nodes (oscillators,
filters, effects). No `MixerNode`, no `MultiLangNode`, no multi-IO consumer exists.
A `RillProgram` will always have `num_inputs ∈ {0,1}`, `num_outputs = 1`.

**Decision**: `MultichannelAlgorithm<T>` impl on `RillProgram` and `RillGraphEngine`
is behind the `"router"` feature flag. With router off, the system is purely SISO.

```rust
// rill-lang/src/program.rs

// Algorithm<T> — always present, always SISO
impl<T: Transcendental> Algorithm<T> for RillProgram<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        // Direct SISO path — no multi-IO branching
        ...
    }
}

// MultichannelAlgorithm<T> — only when router is available
#[cfg(feature = "router")]
impl<T: Transcendental> MultichannelAlgorithm<T> for RillProgram<T> {
    fn num_inputs(&self) -> usize { self.ir.num_inputs }
    fn num_outputs(&self) -> usize { self.ir.num_outputs }
    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.run_multi_io(inputs, outputs)
    }
}
```

```rust
// rill-lang/src/graph_engine.rs — same pattern

impl<T, const BUF: usize> Algorithm<T> for RillGraphEngine<T, BUF> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.drain_mailbox();
        self.program.process(input, output)
    }
}

#[cfg(feature = "router")]
impl<T, const BUF: usize> MultichannelAlgorithm<T> for RillGraphEngine<T, BUF> {
    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.drain_mailbox();
        MultichannelAlgorithm::process(&mut self.program, inputs, outputs)
    }
}
```

**What the `"router"` feature enables:**

| With router | Without router |
|---|---|
| `rill-router` crate linked | `rill-router` absent |
| `MixerNode`, `EQ`, `DryWet` graph nodes | No multi-IO graph nodes |
| `MultichannelAlgorithm` impl on `RillProgram` + `RillGraphEngine` | Only `Algorithm<T>` |
| `MultiLangNode` available | Only `LangNode` (SISO) |
| Mixer/eq/dry_wet lang builtins | Only SISO builtins (filters, oscillators) |
| Multi-IO programs compilable | Programs always SISO |

**API stability**: `MultichannelAlgorithm<T>` is only useful when multi-IO nodes exist.
Code that bounds on `MultichannelAlgorithm<T>` is inherently code that works with
mixers/routers — and those require `rill-router`. The feature flag naturally aligns
with the domain. No false compile errors.

## Dependency changes

### Before (current)

```
rill-adrift
├── rill-core-dsp      (always)
├── rill-oscillators   (always)
├── rill-fft           (feature "fft")
├── rill-lofi          (feature "lofi")
├── rill-lang          (feature "lang")
└── [all other DSL crates]

rill-core-dsp
└── rill-core

rill-fft
├── rill-core
├── rill-core-dsp
└── (no rill-lang, no rill-adrift)
```

### After

```
rill-core
├── traits::builtin::{Registry, BuiltinSig, BlockBuiltin, SampleBuiltin}
└── (no new deps)

rill-lang
├── pub use rill_core::builtin::*     (re-export)
└── (no internal Registry definition)

rill-core-dsp
├── rill-core         (Registry, BuiltinSig now in rill-core — already a dep)
├── OnePoleBuiltin, BiquadBuiltin, OscBuiltin  ← moved from rill-adrift
└── register_lang_builtins()

rill-fft
├── rill-core, rill-core-dsp  (already)
├── [graph] rill-graph         (optional — for register_graph_nodes)
├── [lang]  —                 (rill-core is enough — for register_lang_builtins)
└── register_graph_nodes(), register_lang_builtins()

rill-adrift
├── rill-core-dsp      (always — calls register_lang_builtins)
├── rill-oscillators   (always — calls register_graph_nodes)
├── ...                 (other crates — calls their register_*)
└── full_registry()     (thin aggregator, no internal type knowledge)
```

## Registration function signature

### Graph nodes

```rust
// In each DSP crate, behind #[cfg(feature = "graph")]:

pub fn register_graph_nodes<const BUF_SIZE: usize>(
    factory: &mut rill_graph::NodeFactory<f32, BUF_SIZE>,
) {
    use rill_graph::node_ctor;
    use rill_core::traits::{Node, NodeId, NodeVariant, Params};

    node_ctor!(factory, "rill/sine", |id, params| {
        let mut n = SineOsc::<f32, BUF_SIZE>::new()
            .with_frequency(params.get_f32("freq", 440.0))
            .with_amplitude(params.get_f32("amp", 0.0));
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });
    // ...
}
```

### Lang built-ins

```rust
// In each DSP crate, behind #[cfg(feature = "lang")]  (except rill-core-dsp — always):

pub fn register_lang_builtins<T: rill_core::Transcendental + 'static>(
    reg: &mut rill_core::builtin::Registry<T>,
) {
    use rill_core::builtin::{BuiltinSig, BuiltinKind, ParamType};
    use rill_core::traits::Algorithm;

    reg.register_sample(
        BuiltinSig::simple("onepole", 1, 1, 2, BuiltinKind::Sample),
        |p, sr| {
            let mut inner = OnePole::<T>::new(FilterParams { ... });
            Algorithm::init(&mut inner, sr);
            Box::new(OnePoleBuiltin { inner })
        },
    );
    // ...
}
```

## Wrapper structs — where they go

| Wrapper | Current location | New location |
|---------|-----------------|-------------|
| `OnePoleBuiltin<T>` | `rill-adrift/src/lang_builtins.rs` | `rill-core-dsp/src/lang/onepole.rs` |
| `MoogBuiltin<T>` | `rill-adrift/src/lang_builtins.rs` | `rill-core-dsp/src/lang/moog.rs` |
| `BiquadBuiltin<T>` | `rill-adrift/src/lang_builtins.rs` | `rill-core-dsp/src/lang/biquad.rs` |
| `SineBuiltin<T>` | `rill-adrift/src/lang_builtins.rs` | `rill-core-dsp/src/lang/osc.rs` |
| `NoiseBuiltin<T>` | `rill-adrift/src/lang_builtins.rs` | `rill-core-dsp/src/lang/noise.rs` |
| `ComplexBuiltin<T>` | `rill-adrift/src/lang_builtins.rs` | `rill-lang/src/builtins/complex.rs` |
| `SpectralGateBuiltin<T>` | `rill-adrift/src/lang_builtins.rs` | `rill-fft/src/lang/` |
| `SpectralDelayBuiltin<T>` | `rill-adrift/src/lang_builtins.rs` | `rill-fft/src/lang/` |
| `LofiBuiltin` | `rill-adrift/src/lang_builtins.rs` | `rill-lofi/src/lang/` |
| `AnalogMoogBuiltin<T>` | `rill-adrift/src/lang_builtins.rs` | `rill-core-model/src/lang/` |
| `MixerState<T>` | `rill-lang/src/builtins/mixer.rs` | stays in `rill-lang` |
| `EqState<T>` | `rill-lang/src/builtins/eq.rs` | stays in `rill-lang` |
| `DryWetState` | `rill-lang/src/builtins/dry_wet.rs` | stays in `rill-lang` |

## User-facing API

### Minimal — no rill-adrift

```toml
[dependencies]
rill-core = "0.5"
rill-core-dsp = "0.5"
rill-oscillators = { version = "0.5", features = ["graph"] }
rill-digital-filters = { version = "0.5", features = ["graph"] }
rill-lang = "0.5"
rill-graph = "0.5"
```

```rust
use rill_graph::NodeFactory;
use rill_lang::builtin::Registry;

let mut factory = NodeFactory::new();
rill_oscillators::register_graph_nodes(&mut factory);
rill_digital_filters::register_graph_nodes(&mut factory);

let mut reg = Registry::new();
rill_core_dsp::register_lang_builtins(&mut reg);
rill_lang::register_core_builtins(&mut reg);
```

### Full — with rill-adrift convenience

```toml
[dependencies]
rill-adrift = { version = "0.5", features = ["lofi", "fft", "lang"] }
```

```rust
use rill_adrift::{full_registry, register_all_nodes};

let mut factory = NodeFactory::new();
register_all_nodes(&mut factory);

let reg = full_registry::<f32>();
```

## rill-lang backward compatibility

All existing code that uses `rill_lang::builtin::Registry`, `rill_lang::builtin::BuiltinSig`, etc. continues to work — `rill-lang` re-exports from `rill-core::builtin`:

```rust
// rill-lang/src/builtin.rs
pub use rill_core::builtin::*;
```

```rust
// rill-lang/src/lib.rs
pub mod builtin;  // now re-exports, not defines
```

Tests in `rill-adrift/tests/` that import `rill_adrift::lang_builtins::full_registry` continue to work — `rill-adrift` still provides `full_registry()` as a convenience aggregator.

## Implementation phases

| # | Phase | Scope |
|---|-------|-------|
| 1 | **Move Registry to rill-core** | `Registry<T>`, `BuiltinSig`, `ParamType`, `BlockBuiltin`, `SampleBuiltin` → `rill-core/src/builtin.rs`. `rill-lang` re-exports |
| 2 | **Move wrapper structs to rill-core-dsp** | `OnePoleBuiltin`, `BiquadBuiltin`, `MoogBuiltin`, oscillator builtins → `rill-core-dsp/src/lang/` |
| 3 | **rill-core-dsp self-registration** | `register_lang_builtins()` in `rill-core-dsp` — always available |
| 4 | **rill-lang core builtins** | `register_core_builtins()` for mixer, eq, dry_wet, complex. Gate multi-IO builtins + `MultichannelAlgorithm` impl behind `"router"` feature |
| 5 | **Per-crate graph registration** | `register_graph_nodes()` in rill-oscillators, rill-digital-filters, rill-router, rill-digital-effects (feature "graph") |
| 6 | **Per-crate lang registration** | `register_lang_builtins()` in rill-fft, rill-lofi, rill-core-model (feature "lang") |
| 7 | **Thin rill-adrift** | Replace `registration.rs` and `lang_builtins.rs` with aggregator calls |
| 8 | **Cleanup** | Remove old code from rill-adrift, update docs, add crate-level examples |
