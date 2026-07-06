# rill-lang — DSP/Model Built-in Functions (FFI registry)

> **Status:** Design approved — awaiting implementation plan.
> **Date:** 2026-07-06
> **Branch:** `feature/rill-lang` (follow-on to the crate MVP + hybrid block processing).
> **Depends on:** `2026-07-06-rill-lang-design.md`, `2026-07-06-rill-lang-block-processing-design.md`.

## Motivation

rill-lang's built-ins are currently pure point functions (arithmetic, `sin`/`cos`/…).
This increment lets programs call the ecosystem's **stateful DSP blocks** — filters,
oscillators, WDF models — from `rill-core-dsp` and `rill-core-model`, via an
extensible **foreign-function registry**. It is the "foreign DSP block references"
follow-on, delivered as the full FFI architecture (sample-level **and**
block-level built-ins in one pass).

## What the crates offer (two granularities)

- **Sample-level** — a scalar `process_sample(T) -> T`: `OnePole`, `MoogLadder`
  (`rill-core-dsp`); WDF elements / `wdf_cascade!` / `tape` (`rill-core-model`).
- **Block-level** — buffer processors via `Algorithm<T>`: `Biquad`, `SVF`, `Comb`,
  generators, effects (`rill-core-dsp`); WDF processors (`rill-analog-filters`).

These map onto rill-lang's two execution modes: sample-level built-ins thread
through the **per-sample sample-region** executor (feedback-legal), block-level
built-ins run as an **opaque block step** (not feedback-legal).

## Calling convention

Built-ins take **constant params as args**; **signals flow via combinators**
(Faust-style). A built-in reference has arity `(signal_ins → signal_outs)`:

```faust
process = _ : lowpass(1000.0, 0.7);          -- 1→1 biquad, params cutoff,Q
process = _ : moog(800.0, 0.6) : _ * 0.5;    -- filter in a chain
process = (_ , _) : mix2(0.5);               -- a 2→1 built-in
```

Args must be **constant expressions** (folded to `f64`); the incoming signal
comes from the left via `:`. Parser and AST are unchanged — the existing
`Apply { name, args }` / `Ref(name)` nodes are resolved against the registry
during inference and lowering.

## Registry (rill-lang core — `rill-core`-only, no new deps)

```rust
pub trait SampleBuiltin<T: Transcendental>: Send {
    /// signal_ins inputs → 1 output, per sample.
    fn process_sample(&mut self, inputs: &[T]) -> T;
    fn init(&mut self, _sample_rate: f32) {}
    fn reset(&mut self);
}
// Block built-ins are `rill_core::Algorithm<T>` (1→1 buffer processors).

pub enum BuiltinKind { Sample, Block }

pub struct BuiltinSig {
    pub name: &'static str,
    pub signal_ins: usize,
    pub signal_outs: usize, // MVP: 1
    pub num_params: usize,
    pub kind: BuiltinKind,
}

pub struct Registry<T: Transcendental> { /* name → (sig, factory) */ }
// factories: Fn(&[f64] /*params*/, f32 /*sample_rate*/) -> Box<dyn SampleBuiltin<T>>
//        or: Fn(&[f64], f32) -> Box<dyn Algorithm<T>>
impl<T> Registry<T> {
    pub fn new() -> Self;
    pub fn register_sample(&mut self, sig: BuiltinSig, factory: …);
    pub fn register_block(&mut self, sig: BuiltinSig, factory: …);
    pub fn get(&self, name: &str) -> Option<&Entry<T>>;
}
```

Filters require a **sample rate** (some, like WDF, at construction), so factories
receive it and `compile_with` carries it.

## Pipeline

```
compile_with::<T>(src, &registry, sample_rate)
  → parse
  → infer(program, &registry)     # arity/params from registry signatures
  → lower(typed, &registry)       # emits CallSample/CallBlock + ir.builtins table
  → RillProgram::new(ir, &registry, sample_rate)   # builds boxed instances
```

`compile::<T>(src)` stays unchanged (empty registry, default 44.1 kHz), so all
existing tests hold.

### IR additions

```rust
Instr::CallSample { dst: Reg, srcs: Vec<Reg>, instance: usize }, // stateful, per-sample
Instr::CallBlock  { dst: Reg, src: Reg,       instance: usize }, // opaque, whole-buffer (1→1)
// side table (not generic over T; built into runtime instances by RillProgram::new):
Ir.builtins: Vec<BuiltinInstance { name: String, params: Vec<f64>, kind: BuiltinKind }>
```

`instance` indexes `ir.builtins`; `RillProgram::new` constructs
`sample_builtins: Vec<Box<dyn SampleBuiltin<T>>>` and
`block_builtins: Vec<Box<dyn Algorithm<T>>>` from those entries via the registry.

## Scheduler & executor

- **`CallSample`** is stateful → `is_stateful == true` → lands in a **sample
  region** (feedback-legal, composes with `~`). Executed per sample by calling
  `process_sample(&inputs)` (inputs gathered from `srcs[..][i]` into a small
  stack buffer — no allocation).
- **`CallBlock`** → a new **`Step::ForeignBlock(idx)`** that runs
  `Algorithm::process` over the `[..n]` buffer (`mem::take` on the destination
  register for the borrow, no allocation).
- **Feedback-legality:** `build_schedule` classifies; `compile_with` then
  validates that no `Step::Sample` region contains a `CallBlock` — if it does, a
  block built-in sits inside a `~` loop, which is a `CompileError` (block
  built-ins are opaque, buffer-boundary — they cannot be per-sample-recurred).

## `RillProgram` changes

Adds `sample_builtins` + `block_builtins`, built in `new(ir, &registry, sample_rate)`
(each factory called with its folded params + sample rate; `init(sample_rate)`
applied). `Algorithm::reset` also resets every built-in; a forwarding
`init(sample_rate)` re-initialises them (called when the node enters a graph).

## Bindings in `rill-adrift`

- `register_dsp_builtins(&mut Registry<T>)` — always available (adrift depends on
  `rill-core-dsp`): `onepole(cutoff, q)` and `moog(cutoff, resonance)` as
  **sample** built-ins; `lowpass(cutoff, q)` / `highpass(cutoff, q)` as **block**
  built-ins (Biquad).
- `register_model_builtins(&mut Registry<T>)` — `#[cfg(feature = "analog")]`
  (pulls `rill-core-model` / `rill-analog-filters`): `analog_moog(cutoff,
  resonance)` as a **block** built-in (`WdfMoogLadderProcessor`).
- `full_registry<T>(sample_rate)` convenience assembling the above.

This keeps rill-lang core decoupled — the same pattern as the `rill/lang` node.

## Correctness strategy

- **Sample built-ins:** the existing reference-oracle equivalence covers them
  (both hybrid and reference call `process_sample` per sample identically).
- **Block built-ins:** validated by comparing `_ : lowpass(f, q)` in rill-lang
  against running that `Biquad` `Algorithm` **directly** on the same input
  (confirms param folding, arity, and scheduling order). Also assert that a
  block built-in inside `~` is rejected with a `CompileError`.

## Non-goals (this increment)

- Signal-rate parameter **modulation** (params are compile-time constants).
- **n→m** block built-ins (block is 1→1) and multi-output built-ins.
- Wiring a populated registry into the `rill/lang` graph node with the graph
  sample rate — a small follow-on (the registry + `compile_with` are the
  foundation).

## Foundation note

The registry + `CallSample`/`CallBlock`/`ForeignBlock` structure is exactly what
the future JIT and whole-graph-as-one-program steps consume: foreign blocks
become call sites in generated code, and a graph's nodes can be expressed as
registered built-ins composed by one rill-lang program.
