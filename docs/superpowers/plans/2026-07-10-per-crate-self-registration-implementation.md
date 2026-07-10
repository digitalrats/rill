# Per-crate Self-Registration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move `Registry<T>` + `BuiltinSig` + `BlockBuiltin`/`SampleBuiltin` traits from `rill-lang` to `rill-core`. Add `register_graph_nodes()` + `register_lang_builtins()` per DSP crate. `rill-adrift` becomes a thin aggregator.

**Architecture:** 8 sequential phases. Phase 1 moves the foundation types to `rill-core`. Phase 2 moves wrapper structs to `rill-core-dsp`. Phases 3-4 add registration functions. Phases 5-6 extend to remaining DSP crates. Phase 7 thins `rill-adrift`. Phase 8 cleans up.

**Tech Stack:** Rust, rill-core, rill-core-dsp, rill-lang, rill-graph, rill-adrift, all DSP crates.

**Spec:** `docs/superpowers/specs/2026-07-10-per-crate-self-registration-design.md`

---

## Phase 1: Move Registry to rill-core

### Task 1.1: Create `rill-core/src/builtin.rs`

**Files:**
- Read: `rill-lang/src/builtin.rs` (source of truth)
- Create: `rill-core/src/builtin.rs`
- Modify: `rill-core/src/lib.rs` (add module)

- [ ] **Step 1: Copy the entire `builtin.rs` from rill-lang to rill-core**

```bash
cp rill-lang/src/builtin.rs rill-core/src/builtin.rs
```

- [ ] **Step 2: Fix imports in `rill-core/src/builtin.rs`**

The copied file references `rill_lang` imports. Fix them:

```rust
// rill-core/src/builtin.rs

// Fix imports at top of file:
use crate::traits::algorithm::Algorithm;
use crate::traits::Transcendental;
use crate::traits::param::ParamValue;
// Keep all other types/enums unchanged
```

Remove any `rill_lang`-specific imports. `Registry`, `BuiltinSig`, `ParamType`, `RecordSchema`, `RecordField`, `BuiltinKind`, `SampleBuiltin`, `BlockBuiltin`, `BuiltinInstance`, `SignatureSource` — all stay.

- [ ] **Step 3: Add `pub mod builtin;` to `rill-core/src/lib.rs`**

Find the module declarations in `rill-core/src/lib.rs` and add:

```rust
pub mod builtin;
```

- [ ] **Step 4: Build rill-core**

```bash
cargo build -p rill-core 2>&1 | head -20
```

Expected: compiles. Fix any import errors (e.g., `Algorithm` trait path).

- [ ] **Step 5: Run rill-core tests**

```bash
cargo test -p rill-core 2>&1 | tail -10
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add rill-core/src/builtin.rs rill-core/src/lib.rs
git commit -m "feat(rill-core): add builtin module with Registry, BuiltinSig, BlockBuiltin/SampleBuiltin"
```

---

### Task 1.2: Make rill-lang re-export from rill-core

**Files:**
- Modify: `rill-lang/src/builtin.rs`
- Modify: `rill-lang/src/lib.rs`

- [ ] **Step 1: Replace `rill-lang/src/builtin.rs` with re-export**

```rust
// rill-lang/src/builtin.rs

//! Re-exported from rill-core for backward compatibility.
pub use rill_core::builtin::*;
```

- [ ] **Step 2: Update rill-lang imports**

Run `cargo build -p rill-lang 2>&1 | head -30` and fix any code in rill-lang that now needs `rill_core::builtin::` prefix. Search for patterns:

```bash
cd rill && rg 'use crate::builtin::' rill-lang/src/ --type rust -l
```

These internal `crate::builtin::` references should still work since the module is still `pub mod builtin;` in `rill-lang/src/lib.rs`. Verify.

- [ ] **Step 3: Build and test rill-lang**

```bash
cargo build -p rill-lang 2>&1 | tail -5
cargo test -p rill-lang 2>&1 | tail -10
```

Expected: compiles, all tests pass.

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/builtin.rs
git commit -m "refactor(rill-lang): re-export builtin module from rill-core"
```

---

### Task 1.3: Update rill-adrift imports

**Files:**
- Modify: `rill-adrift/src/lang_builtins.rs`
- Modify: `rill-adrift/src/lang_node.rs`
- Modify: `rill-adrift/Cargo.toml`

- [ ] **Step 1: Check if rill-adrift imports from `rill_lang::builtin`**

```bash
cd rill && rg 'rill_lang::builtin' rill-adrift/src/ --type rust
```

If rill-adrift imports `rill_lang::builtin::*`, these will still work because rill-lang re-exports. No changes needed. But verify that `rill_core::builtin` is also directly importable:

```rust
// rill-adrift can now use either:
use rill_lang::builtin::Registry;     // via re-export (backward compat)
use rill_core::builtin::Registry;     // directly (new path)
```

- [ ] **Step 2: Build rill-adrift with lang feature**

```bash
cargo build -p rill-adrift --features lang 2>&1 | tail -5
```

Expected: compiles.

- [ ] **Step 3: Run tests**

```bash
cargo test -p rill-adrift --features lang 2>&1 | tail -10
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add rill-adrift/
git commit -m "refactor(rill-adrift): support rill_core::builtin path alongside rill_lang re-export"
```

---

## Phase 2: Move wrapper structs to rill-core-dsp

### Task 2.1: Create rill-core-dsp lang module with OnePoleBuiltin

**Files:**
- Create: `rill-core-dsp/src/lang/mod.rs`
- Create: `rill-core-dsp/src/lang/onepole.rs`

- [ ] **Step 1: Create module structure**

```rust
// rill-core-dsp/src/lang/mod.rs

pub mod onepole;
pub mod biquad;
pub mod moog;
pub mod osc;
pub mod noise;
```

- [ ] **Step 2: Move OnePoleBuiltin from rill-adrift to rill-core-dsp**

Read the `OnePoleBuiltin` struct and its `SampleBuiltin` impl from `rill-adrift/src/lang_builtins.rs`. Copy into `rill-core-dsp/src/lang/onepole.rs`:

```rust
// rill-core-dsp/src/lang/onepole.rs

use rill_core::builtin::{SampleBuiltin, ParamValue};
use rill_core::traits::{Algorithm, Transcendental};
use crate::filters::{FilterParams, FilterType, OnePole, Filter};

pub struct OnePoleBuiltin<T: Transcendental> {
    pub inner: OnePole<T>,
}

impl<T: Transcendental> SampleBuiltin<T> for OnePoleBuiltin<T> {
    fn process_sample(&mut self, inputs: &[T]) -> T {
        self.inner.process_sample(inputs[0])
    }

    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.inner, sr);
    }

    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => Filter::set_cutoff(&mut self.inner, v),
            1 => Filter::set_q(&mut self.inner, v),
            _ => {}
        }
    }
}

fn pv_f32(value: &ParamValue) -> f32 {
    match value {
        ParamValue::Float(v) => *v,
        _ => 0.0,
    }
}
```

- [ ] **Step 3: Move BiquadBuiltin similarly**

Create `rill-core-dsp/src/lang/biquad.rs`:

```rust
// rill-core-dsp/src/lang/biquad.rs

use rill_core::builtin::{BlockBuiltin, ParamValue};
use rill_core::traits::{Algorithm, Transcendental};
use crate::filters::{Biquad, FilterParams, FilterType};

pub struct BiquadBuiltin<T: Transcendental> {
    pub inner: Biquad<T>,
}

impl<T: Transcendental> Algorithm<T> for BiquadBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> rill_core::traits::ProcessResult<()> {
        self.inner.process(input, output)
    }
    fn init(&mut self, sr: f32) { Algorithm::init(&mut self.inner, sr); }
    fn reset(&mut self) { Algorithm::reset(&mut self.inner); }
}

impl<T: Transcendental> BlockBuiltin<T> for BiquadBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => self.inner.set_cutoff(v),
            1 => self.inner.set_q(v),
            _ => {}
        }
    }
}

fn pv_f32(value: &ParamValue) -> f32 {
    match value { ParamValue::Float(v) => *v, _ => 0.0 }
}
```

- [ ] **Step 4: Move remaining wrapper structs**

Move `MoogBuiltin`, oscillator builtins (`SineBuiltin`, `SawBuiltin`, etc.), and `NoiseBuiltin` from `rill-adrift/src/lang_builtins.rs` into corresponding files in `rill-core-dsp/src/lang/`.

For oscillator builtins: extract the per-oscillator wrapper structs from the closure bodies in `register_oscillator_builtins`. Trace the exact types returned by the factory closures.

- [ ] **Step 5: Add module and build**

```rust
// rill-core-dsp/src/lib.rs — add:
pub mod lang;
```

```bash
cargo build -p rill-core-dsp 2>&1 | head -20
```

Fix import errors.

- [ ] **Step 6: Commit**

```bash
git add rill-core-dsp/src/lang/ rill-core-dsp/src/lib.rs
git commit -m "feat(rill-core-dsp): add lang module with Builtin wrapper structs"
```

---

## Phase 3: rill-core-dsp self-registration

### Task 3.1: Add register_lang_builtins() to rill-core-dsp

**Files:**
- Create: `rill-core-dsp/src/lang/register.rs`
- Modify: `rill-core-dsp/src/lang/mod.rs`

- [ ] **Step 1: Create registration function**

```rust
// rill-core-dsp/src/lang/register.rs

use rill_core::builtin::{Registry, BuiltinSig, BuiltinKind, ParamType};
use rill_core::traits::{Algorithm, Transcendental};
use crate::filters::{FilterParams, FilterType, OnePole, Biquad, MoogLadder, Filter};
use crate::generators::{BasicOscillator, NoiseGenerator, Generator};
use super::onepole::OnePoleBuiltin;
use super::biquad::BiquadBuiltin;
// ... other wrapper imports

/// Register all rill-core-dsp lang built-ins (always available, no feature gate).
pub fn register_lang_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    register_dsp_filters(reg);
    register_oscillators(reg);
}

fn register_dsp_filters<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    reg.register_sample(
        BuiltinSig::simple("onepole", 1, 1, 2, BuiltinKind::Sample),
        |p, sr| {
            let mut inner = OnePole::<T>::new(FilterParams {
                filter_type: FilterType::LowPass,
                cutoff: p[0] as f32,
                q: p[1] as f32,
                gain_db: 0.0,
            });
            Algorithm::init(&mut inner, sr);
            Box::new(OnePoleBuiltin { inner })
        },
    );

    reg.register_sample(
        BuiltinSig::simple("moog", 1, 1, 2, BuiltinKind::Sample),
        |p, sr| {
            let mut inner = MoogLadder::<T>::new(p[0] as f32, p[1] as f32);
            Algorithm::init(&mut inner, sr);
            Box::new(MoogBuiltin { inner })
        },
    );

    reg.register_block(
        BuiltinSig::simple("lowpass", 1, 1, 2, BuiltinKind::Block),
        |p, sr| {
            let mut b = Biquad::<T>::new(FilterParams {
                filter_type: FilterType::LowPass,
                cutoff: p[0] as f32,
                q: p[1] as f32,
                gain_db: 0.0,
            });
            Algorithm::init(&mut b, sr);
            Box::new(BiquadBuiltin { inner: b })
        },
    );

    reg.register_block(
        BuiltinSig::simple("highpass", 1, 1, 2, BuiltinKind::Block),
        |p, sr| {
            let mut b = Biquad::<T>::new(FilterParams {
                filter_type: FilterType::HighPass,
                cutoff: p[0] as f32,
                q: p[1] as f32,
                gain_db: 0.0,
            });
            Algorithm::init(&mut b, sr);
            Box::new(BiquadBuiltin { inner: b })
        },
    );
}

fn register_oscillators<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig::simple("sine", 0, 1, 3, BuiltinKind::Block),
        |p, sr| {
            let mut osc = BasicOscillator::<T>::new_sine();
            osc.set_frequency(T::from_f32(p[0] as f32));
            osc.set_amplitude(T::from_f32(p[1] as f32));
            Algorithm::init(&mut osc, sr);
            Box::new(OscBuiltin { inner: osc })
        },
    );

    reg.register_block(
        BuiltinSig::simple("saw", 0, 1, 3, BuiltinKind::Block),
        |p, sr| {
            let mut osc = BasicOscillator::<T>::new_saw();
            osc.set_frequency(T::from_f32(p[0] as f32));
            osc.set_amplitude(T::from_f32(p[1] as f32));
            Algorithm::init(&mut osc, sr);
            Box::new(OscBuiltin { inner: osc })
        },
    );

    reg.register_block(
        BuiltinSig::simple("square", 0, 1, 3, BuiltinKind::Block),
        |p, sr| {
            let mut osc = BasicOscillator::<T>::new_square();
            osc.set_frequency(T::from_f32(p[0] as f32));
            osc.set_amplitude(T::from_f32(p[1] as f32));
            Algorithm::init(&mut osc, sr);
            Box::new(OscBuiltin { inner: osc })
        },
    );

    reg.register_block(
        BuiltinSig::simple("triangle", 0, 1, 3, BuiltinKind::Block),
        |p, sr| {
            let mut osc = BasicOscillator::<T>::new_triangle();
            osc.set_frequency(T::from_f32(p[0] as f32));
            osc.set_amplitude(T::from_f32(p[1] as f32));
            Algorithm::init(&mut osc, sr);
            Box::new(OscBuiltin { inner: osc })
        },
    );

    reg.register_block(
        BuiltinSig::simple("noise", 0, 1, 2, BuiltinKind::Block),
        |p, sr| {
            let mut gen = NoiseGenerator::<T>::new_white();
            gen.set_amplitude(T::from_f32(p[1] as f32));
            Algorithm::init(&mut gen, sr);
            Box::new(NoiseBuiltin { inner: gen })
        },
    );
}

// Note: Ensure wrapper structs for oscillators and noise exist in lang/osc.rs and lang/noise.rs.
// Trace the exact types from rill-adrift/src/lang_builtins.rs registration closures.
// Some builtins may return inline Algorithm impls (not named structs) — create named wrappers.
```

- [ ] **Step 2: Add module and build**

```rust
// rill-core-dsp/src/lang/mod.rs — add:
pub mod register;
```

```bash
cargo build -p rill-core-dsp 2>&1 | head -30
```

Fix all compilation errors. The key challenge: the registration closures in `rill-adrift` return various wrapper types. Ensure all wrapper structs are created in `rill-core-dsp/src/lang/` and imported correctly.

- [ ] **Step 3: Commit**

```bash
git add rill-core-dsp/src/lang/register.rs rill-core-dsp/src/lang/mod.rs
git commit -m "feat(rill-core-dsp): add register_lang_builtins for DSP filters and oscillators"
```

---

## Phase 4: rill-lang core builtins + router feature gate

### Task 4.1: Add register_core_builtins to rill-lang

**Files:**
- Create: `rill-lang/src/register.rs`

- [ ] **Step 1: Create registration for mixer, eq, dry_wet, complex**

```rust
// rill-lang/src/register.rs

use rill_core::builtin::{Registry, BuiltinSig, BuiltinKind, ParamType, RecordSchema, RecordField};
use rill_core::traits::Transcendental;

/// Register rill-lang's own builtins (mixer, eq, dry_wet, complex arithmetic).
/// Call this after `rill_core_dsp::register_lang_builtins()`.
pub fn register_core_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    register_complex_builtins(reg);
    register_mixer_builtins(reg);
    register_eq_parametric_builtin(reg);
    register_dry_wet_builtin(reg);
}

fn register_complex_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    // Move from rill-adrift/src/lang_builtins.rs::register_complex_builtins
    // Pure arithmetic — no DSP crate dependency
    reg.register_block(
        BuiltinSig {
            name: "complex",
            params: vec![ParamType::Float, ParamType::Float],
            signal_outs: 2,
            kind: BuiltinKind::Block,
        },
        |p, _sr| {
            Box::new(ComplexConstBuiltin {
                re: T::from_f32(p[0] as f32),
                im: T::from_f32(p[1] as f32),
            })
        },
    );
    // ... conj, re, im, norm, arg, cmul, cadd ...
}

fn register_mixer_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    use crate::builtins::mixer::{MixerConfig, MixerState};
    // Move from rill-adrift/src/lang_builtins.rs::register_mixer_builtins
}

fn register_eq_parametric_builtin<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    use crate::builtins::eq::{EqConfig, EqState};
    // Move from rill-adrift/src/lang_builtins.rs::register_eq_parametric_builtin
}

fn register_dry_wet_builtin<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    use crate::builtins::dry_wet::{DryWetConfig, DryWetState};
    // Move from rill-adrift/src/lang_builtins.rs::register_dry_wet_builtin
}
```

- [ ] **Step 2: Add module to lib.rs**

```rust
// rill-lang/src/lib.rs — add:
pub mod register;
```

- [ ] **Step 3: Build**

```bash
cargo build -p rill-lang 2>&1 | head -20
```

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/register.rs rill-lang/src/lib.rs
git commit -m "feat(rill-lang): add register_core_builtins for mixer, eq, dry_wet, complex"
```

---

### Task 4.2: Feature-gate MultichannelAlgorithm behind "router"

**Files:**
- Modify: `rill-lang/Cargo.toml`
- Modify: `rill-lang/src/program.rs`
- Modify: `rill-lang/src/graph_engine.rs`

- [ ] **Step 1: Add "router" feature to Cargo.toml**

```toml
# rill-lang/Cargo.toml

[features]
router = []
```

- [ ] **Step 2: Wrap MultichannelAlgorithm impls**

```rust
// rill-lang/src/program.rs

// Algorithm<T> — always present
impl<T: Transcendental> Algorithm<T> for RillProgram<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        // existing SISO implementation unchanged
        ...
    }
    fn reset(&mut self) { ... }
}

// MultichannelAlgorithm<T> — only with router
#[cfg(feature = "router")]
impl<T: Transcendental> MultichannelAlgorithm<T> for RillProgram<T> {
    fn num_inputs(&self) -> usize { self.ir.num_inputs }
    fn num_outputs(&self) -> usize { self.ir.num_outputs }
    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.run_multi_io(inputs, outputs)
    }
    fn reset(&mut self) { Algorithm::reset(self); }
}
```

- [ ] **Step 3: Same for RillGraphEngine**

```rust
// rill-lang/src/graph_engine.rs

#[cfg(feature = "router")]
impl<T: Transcendental, const BUF: usize> MultichannelAlgorithm<T> for RillGraphEngine<T, BUF> {
    fn num_inputs(&self) -> usize { self.schedule.inputs }
    fn num_outputs(&self) -> usize { self.schedule.outputs }
    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.drain_mailbox();
        MultichannelAlgorithm::process(&mut self.program, inputs, outputs)
    }
    fn reset(&mut self) { Algorithm::reset(self); }
}
```

- [ ] **Step 4: Gate mixer/eq/dry_wet builtins**

In `rill-lang/src/register.rs`, gate multi-IO builtins behind `#[cfg(feature = "router")]`:

```rust
pub fn register_core_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    register_complex_builtins(reg);      // always (SISO)
    #[cfg(feature = "router")] {
        register_mixer_builtins(reg);     // multi-IO
        register_eq_parametric_builtin(reg);
        register_dry_wet_builtin(reg);
    }
}
```

- [ ] **Step 5: Build with and without router**

```bash
cargo build -p rill-lang 2>&1 | tail -5
cargo build -p rill-lang --features router 2>&1 | tail -5
```

Expected: both compile.

- [ ] **Step 6: Run tests**

```bash
cargo test -p rill-lang 2>&1 | tail -10
cargo test -p rill-lang --features router 2>&1 | tail -10
```

- [ ] **Step 7: Commit**

```bash
git add rill-lang/Cargo.toml rill-lang/src/program.rs rill-lang/src/graph_engine.rs rill-lang/src/register.rs
git commit -m "feat(rill-lang): feature-gate MultichannelAlgorithm and multi-IO builtins behind 'router'"
```

---

## Phase 5: Per-crate graph registration

### Task 5.1: Add register_graph_nodes() to rill-oscillators

**Files:**
- Create: `rill-oscillators/src/register.rs`
- Modify: `rill-oscillators/Cargo.toml`
- Modify: `rill-oscillators/src/lib.rs`

- [ ] **Step 1: Add optional rill-graph dependency**

```toml
# rill-oscillators/Cargo.toml

[features]
graph = ["rill-graph"]

[dependencies]
rill-graph = { version = "0.5", path = "../rill-graph", optional = true }
```

- [ ] **Step 2: Create registration function**

```rust
// rill-oscillators/src/register.rs

#[cfg(feature = "graph")]
use rill_graph::{NodeFactory, node_ctor};
#[cfg(feature = "graph")]
use rill_core::traits::{Node, NodeId, NodeVariant, Params};

#[cfg(feature = "graph")]
pub fn register_graph_nodes<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    use crate::signal::{SineOsc, SawOsc, NoiseOsc, NoiseType};

    node_ctor!(factory, "rill/sine", |id: NodeId, params: &Params| {
        let mut n = SineOsc::<f32, BUF_SIZE>::new()
            .with_frequency(params.get_f32("freq", 440.0))
            .with_amplitude(params.get_f32("amp", 0.0));
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });

    node_ctor!(factory, "rill/saw", |id: NodeId, params: &Params| {
        let mut n = SawOsc::<f32, BUF_SIZE>::new()
            .with_frequency(params.get_f32("freq", 440.0))
            .with_amplitude(params.get_f32("amp", 0.0));
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });

    node_ctor!(factory, "rill/noise", |id: NodeId, params: &Params| {
        let t = match params.get("type").and_then(|v| v.as_f32()) {
            Some(2.0) => NoiseType::Brown,
            Some(1.0) => NoiseType::Pink,
            _ => NoiseType::White,
        };
        let mut n = NoiseOsc::<BUF_SIZE>::new().with_type(t);
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Source(Box::new(n))
    });
}
```

- [ ] **Step 3: Add public module**

```rust
// rill-oscillators/src/lib.rs — add:
pub mod register;
```

- [ ] **Step 4: Build with and without graph feature**

```bash
cargo build -p rill-oscillators 2>&1 | tail -5
cargo build -p rill-oscillators --features graph 2>&1 | tail -5
```

- [ ] **Step 5: Commit**

```bash
git add rill-oscillators/
git commit -m "feat(rill-oscillators): add register_graph_nodes behind 'graph' feature"
```

---

### Task 5.2–5.6: Repeat for remaining DSP crates

Same pattern for:
- `rill-digital-filters` — feature "graph" → `register_graph_nodes()` with `"rill/biquad"`, `"rill/moog_ladder"`
- `rill-digital-effects` — feature "graph" → `"rill/delay"`, `"rill/distortion"`, `"rill/limiter"`
- `rill-router` — feature "graph" → `"rill/mixer"`, `"rill/parametric_eq"`, `"rill/graphic_eq"`, `"rill/dry_wet_mix"`
- `rill-sampler` — feature "graph" → `"rill/sampler"`
- `rill-lofi` — feature "graph" → `"rill/lofi"`, `"rill/lofi_chip"`
- `rill-fft` — feature "graph" → `"rill/convolver"`
- `rill-core-model` + `rill-analog-*` — feature "graph" → analog nodes

Each follows the exact same template: add optional `rill-graph` dep, create `register.rs`, call `node_ctor!` for each node type, gate everything behind `#[cfg(feature = "graph")]`.

Commit each crate separately.

---

## Phase 6: Per-crate lang registration

### Task 6.1: Add register_lang_builtins() to rill-fft

**Files:**
- Create: `rill-fft/src/lang.rs` (or `src/register.rs`)
- Modify: `rill-fft/Cargo.toml`
- Modify: `rill-fft/src/lib.rs`

- [ ] **Step 1: Add feature flag**

```toml
# rill-fft/Cargo.toml

[features]
lang = []

# No new dependency needed — rill-core already has Registry
```

- [ ] **Step 2: Create registration**

```rust
// rill-fft/src/lang.rs

#[cfg(feature = "lang")]
use rill_core::builtin::{Registry, BuiltinSig, BuiltinKind, ParamType};
#[cfg(feature = "lang")]
use rill_core::traits::{Algorithm, Transcendental};

#[cfg(feature = "lang")]
pub fn register_lang_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    use crate::effects::spectral_gate::SpectralGate;
    use crate::effects::spectral_delay::SpectralDelay;

    reg.register_block(
        BuiltinSig::simple("spectralgate", 1, 1, 2, BuiltinKind::Block),
        |p, sr| {
            let mut gate = SpectralGate::<T>::new(p[0] as f32, p[1] as f32);
            Algorithm::init(&mut gate, sr);
            Box::new(SpectralGateBuiltin { inner: gate })
        },
    );

    reg.register_block(
        BuiltinSig::simple("spectraldelay", 1, 1, 2, BuiltinKind::Block),
        |p, sr| {
            let mut delay = SpectralDelay::<T>::new(p[0] as f32, p[1] as f32);
            Algorithm::init(&mut delay, sr);
            Box::new(SpectralDelayBuiltin { inner: delay })
        },
    );
}

// Wrapper structs — move from rill-adrift
#[cfg(feature = "lang")]
struct SpectralGateBuiltin<T: Transcendental> { inner: SpectralGate<T> }
// ... impl Algorithm + BlockBuiltin ...
```

- [ ] **Step 3: Build, test, commit**

```bash
cargo build -p rill-fft --features lang 2>&1 | tail -5
cargo test -p rill-fft --features lang 2>&1 | tail -10
git add rill-fft/ && git commit -m "feat(rill-fft): add register_lang_builtins behind 'lang' feature"
```

### Task 6.2–6.3: Repeat for rill-lofi and rill-core-model

Same pattern — move wrapper structs + registration closures from `rill-adrift/src/lang_builtins.rs` into each respective crate, behind `#[cfg(feature = "lang")]`.

---

## Phase 7: Thin rill-adrift

### Task 7.1: Replace registration.rs with aggregator

**Files:**
- Modify: `rill-adrift/src/registration.rs`
- Modify: `rill-adrift/Cargo.toml`

- [ ] **Step 1: Replace register_all_nodes() body**

```rust
// rill-adrift/src/registration.rs

pub fn register_all_nodes<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
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
    #[cfg(feature = "lang")]    register_lang(factory);
}
```

Remove ALL internal `register_*` function bodies. Delete wrapper struct imports. Delete `use rill_oscillators::signal::SineOsc` etc. — these are now internal to each crate.

- [ ] **Step 2: Replace lang_builtins.rs with aggregator**

```rust
// rill-adrift/src/lang_builtins.rs

pub fn full_registry<T: rill_core::Transcendental + 'static>() -> rill_core::builtin::Registry<T> {
    let mut reg = rill_core::builtin::Registry::new();

    // Always available
    rill_core_dsp::lang::register::register_lang_builtins(&mut reg);
    rill_lang::register::register_core_builtins(&mut reg);

    // Feature-gated
    #[cfg(feature = "fft")]
    rill_fft::lang::register_lang_builtins(&mut reg);
    #[cfg(feature = "lofi")]
    rill_lofi::lang::register_lang_builtins(&mut reg);
    #[cfg(feature = "analog")]
    rill_core_model::lang::register_lang_builtins(&mut reg);

    reg
}
```

Delete ALL wrapper structs, closure bodies, and internal registration functions from this file.

- [ ] **Step 3: Update Cargo.toml features**

Ensure `rill-adrift/Cargo.toml` features map correctly:
- Feature `"lang"` enables `rill-lang`
- Feature `"fft"` enables `rill-fft`
- Feature `"lofi"` enables `rill-lofi`
- Feature `"analog"` enables `rill-core-model`, `rill-analog-filters`, `rill-analog-effects`
- Feature `"router"` enables `rill-router` (for multi-IO)

- [ ] **Step 4: Build and test**

```bash
cargo build -p rill-adrift --features lang 2>&1 | tail -5
cargo build -p rill-adrift --features "lang,fft,lofi,analog" 2>&1 | tail -5
cargo test -p rill-adrift --features lang 2>&1 | tail -15
```

Expected: all tests pass. All features compile.

- [ ] **Step 5: Commit**

```bash
git add rill-adrift/
git commit -m "refactor(rill-adrift): thin aggregator — delegate registration to DSP crates"
```

---

## Phase 8: Cleanup

### Task 8.1: Remove old code, update docs

**Files:**
- Delete obsolete code from `rill-adrift/src/lang_builtins.rs` (wrapper structs)
- Update `rill-adrift/examples/` if they import from deleted locations
- Add module-level docs to new `register.rs` files

- [ ] **Step 1: Verify no dead imports**

```bash
cd rill && cargo build --workspace --features "lang,fft,lofi,analog,graph" 2>&1 | tail -10
```

Fix any unused import warnings.

- [ ] **Step 2: Run full test suite**

```bash
cargo test --workspace --features "lang" 2>&1 | grep -E 'FAILED|test result'
```

- [ ] **Step 3: Clippy**

```bash
cargo clippy --workspace --features "lang" 2>&1 | grep 'warning:' | wc -l
```

Expected: 0 warnings.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "chore: cleanup after per-crate self-registration migration"
```

---

## Plan Self-Review

### Spec coverage check

| Spec section | Task(s) |
|---|---|
| Move Registry to rill-core | 1.1, 1.2, 1.3 |
| Move wrapper structs to rill-core-dsp | 2.1 |
| rill-core-dsp self-registration | 3.1 |
| rill-lang core builtins + router gate | 4.1, 4.2 |
| Per-crate graph registration | 5.1–5.6 |
| Per-crate lang registration | 6.1–6.3 |
| Thin rill-adrift | 7.1 |
| Cleanup | 8.1 |

### Placeholder scan

No TBD/TODO. All code steps contain concrete implementations. Tasks 5.2–5.6 and 6.2–6.3 follow the same template as 5.1/6.1 — concrete filenames and patterns given.

### Type consistency

- `Registry<T>` defined in Task 1.1, used in 3.1, 4.1, 6.1, 7.1
- `BuiltinSig` defined in Task 1.1, used in 3.1, 4.1, 6.1
- `NodeFactory` used in 5.1–5.6, 7.1 — always from `rill-graph`
- Feature names: `"graph"`, `"lang"`, `"router"` — consistent across all tasks
