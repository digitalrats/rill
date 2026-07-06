# rill-lang — RT-safe Named Parameters + Smoothing

> **Status:** Design approved — awaiting implementation plan.
> **Date:** 2026-07-06
> **Branch:** `feature/rill-lang`.
> **Depends on:** the crate MVP, hybrid block processing, and DSP built-ins designs.

## Motivation

Today a programmable node (`LangNode`) can only change behavior by replacing its
whole `source` and **recompiling** (allocating, on the I/O thread). rill-lang
built-in params and literals are compile-time constants, so there is no way to
tweak, say, a filter cutoff at runtime. This increment adds **first-class named
parameters** with **RT-safe control-rate updates** and **smoothing**, and wires
them into the graph's automation stack (servos / LFO / MIDI) for free.

## Threading foundation

Per the engine model, a node's `set_parameter` is applied by the graph actor's
`drain()` **inside the I/O callback, immediately before `process()`**. So a
parameter write and the subsequent read happen on the **same thread, in order** —
a plain `Vec<f64>` slot is race-free, and parameter changes take effect at
**block boundaries (control rate)**.

## 1. `param()` — named mutable slots

A new primitive: `param("cutoff", 1000.0)` or `param("cutoff", 1000.0, 20.0, 20000.0)`
(name, default, optional min, max), arity **0→1**.

This needs **string literals** in the language: add `Tok::Str(String)` (lexer),
`Expr::Str(String, Span)` (AST), and a parser rule. `param` is a reserved form
handled in inference/lowering (not via the built-in registry): `args[0]` must be
a string literal (the name), the rest constant floats.

- **IR:** `ReadParam { dst, idx }` + a side table `ir.params: Vec<ParamDef { name, default, min, max }>`.
- **`RillProgram`:** `params: Vec<f64>` (current values, initialised to defaults),
  `param_index(name) -> Option<usize>`, `set_param(idx, v)` (clamped to `[min,max]`),
  `param(idx) -> f64`, `params_meta() -> &[ParamDef]`.
- **Execution:** `ReadParam` is **combinational** — constant within a block, so a
  **block op** fills the register buffer once per block with `params[idx]`. It is
  **not stateful** (does not force a sample region). Reference oracle reads
  `params[idx]` per sample identically.
- Usable anywhere a signal/constant is expected: `_ * param("gain", 0.5)`,
  `_ + param("dc")`, and inside DSL-authored filters (`+ ~ (_ * param("fb", 0.9))`).

## 2. Dynamic built-in params

Allow `lowpass(param("cutoff"), 0.7)`. A built-in argument may be **either a
constant or a single `param(...)`** (composed expressions on built-in args are a
non-goal). Lowering records, per built-in instance, a list of
`(arg_position → param_idx)` **bindings** on `ir.builtins[instance]`. Each block,
before running the built-in, the executor pushes the current param values via a
`set_param` hook.

- Add `fn set_param(&mut self, index: usize, value: T) {}` to `SampleBuiltin`.
- Introduce **`BlockBuiltin<T>: rill_core::Algorithm<T>`** with the same
  `set_param`; block built-ins become `Box<dyn BlockBuiltin<T>>`. (Still
  `rill-core`-only in rill-lang core.)
- Bindings map indices to coefficients — e.g. Biquad `set_param(0)=cutoff`,
  `set_param(1)=q` → recompute; OnePole/Moog likewise. Non-dynamic args keep their
  constant (from the built-in's construction).
- The executor pushes the (few) bound params every block before processing —
  simple and correct at control rate; the built-in recomputes coefficients.

## 3. `smooth(x, ms)` — zipper-free changes

`smooth(x, ms)` (arity 1→1, `ms` a constant) lowers to a **native one-pole
recurrence in IR**: `y[n] = y[n-1] + a·(x[n] − y[n-1])`, realised with a
`ReadState`/`WriteState` pair and the coefficient `a` computed from `ms` and the
sample rate at **lower time**. It reuses the existing feedback machinery (runs in
a sample region), needs no runtime smoother object, and is JIT-friendly.

`smooth` is the reason `lower` gains a `sample_rate` argument: `lower_with(tp,
sigs, sample_rate)`. Baking the sample rate means an SR change requires a
recompile (documented; the program is cheap to recompile, and `compile_with`
already takes the sample rate). Typical use: `_ * smooth(param("gain", 0.5), 10.0)`.

## 4. Graph integration (`LangNode`)

- `LangNode::metadata()` advertises `program.params_meta()` as
  `Vec<ParamMetadata>` (`ParamMetadata::new(name, ParamType::Float,
  ParamValue::Float(default)).with_range(min, max, step)`).
- `set_parameter(name, ParamValue::Float(v))` → `program.set_param(idx, v)` (the
  RT-safe slot write); `"source"` still recompiles (kept). `get_parameter(name)`
  reads the slot back.
- Because automation targets `NodeId + param_name → ParameterId → SetParameter`,
  **servos, LFOs, envelopes, and MIDI mappings drive rill-lang params with no
  extra work**.

## Correctness

- Reference-oracle equivalence for programs using `param` and `smooth`.
- Param `set`/`get` round-trip and range clamping.
- Automation smoke: changing a param between `process()` calls changes the output.
- Dynamic built-in param: a `param("cutoff")` sweep into `lowpass`/`moog` alters
  the filtered output.
- `smooth` step response: an abrupt param jump ramps over ~`ms`.

## Non-goals (this increment)

- Composed expressions as built-in args (`lowpass(param("c") * 2.0, ...)`).
- True audio-rate (per-sample) modulation of imported built-ins — this is
  control-rate/block.
- Parameter persistence in `RillLangDef` (source stays the canonical form; current
  param values are runtime state).

## Foundation note

Named params + `set_param` hooks are also what the JIT and whole-graph steps need:
params become stable memory slots the generated code reads, and a graph's
node parameters map onto one program's `params_meta()`.
