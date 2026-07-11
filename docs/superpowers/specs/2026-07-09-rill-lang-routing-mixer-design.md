# rill-lang — Unified Argument Model & Routing/Mixer Primitives

> **Status:** Design — awaiting user review, then implementation plan.
> **Date:** 2026-07-09
> **Scope:** Extends `rill-lang` with (a) unified function argument model (signals as first-class values), (b) mixer/routing/EQ/dry-wet primitives via built-in functions, (c) multi-IO compilation target via `MultichannelAlgorithm<T>` trait in `rill-core`.

## Motivation

`rill-lang` currently separates signals and parameters into two distinct channels:
signals flow through combinators (`:`, `,`, `<:`, `:>`, `~`, `@`), while parameters
are compile-time constants passed via `Apply` arguments. This duality prevents
multi-input built-in functions (mixers, dry/wet) from receiving signal arguments
directly — a mixer's N channel inputs must be wired through the combinator
context, which has fixed arity and no variadic combinator.

Unifying the argument model makes signals first-class objects, enabling:
- Mixer primitives: `mixer ch0 ch1 ch2 { buses: 2 }`
- EQ built-ins: `eq_parametric _ { bands: { b0: { freq: 1000, q: 1.0, gain_db: 3.0, type: peaking } } }`
- Dry/wet mix: `dry_wet dry_sig wet_sig { mix: 0.5 }`
- Variadic signal functions (sum, interleave, etc.)
- Foundation for future type classes

The mixer, EQ, and dry/wet primitives compile to hand-written RT code (no
runtime dependency on `rill-router`), outputting a single `MultichannelAlgorithm<T>`
with N inputs and M outputs.

## Confirmed decisions

| Dimension | Decision |
|---|---|
| **Multi-IO target** | New `MultichannelAlgorithm<T>` trait in `rill-core`, separate from `Algorithm<T>` (no breaking change) |
| **Argument model** | Unified: `Apply` args can be signals, scalars, strings, records, enums — discriminated by `ParamType` |
| **Parameter declarations** | Unchanged: formal parameters of `main`/`Anchor` remain the mechanism for named params. Mixer/EQ generate auto-params from record config fields |
| **Syntax style** | Bracket-free juxtaposition only. No `()` for function application. `{}` exclusively for record literals. `where` blocks use layout-based indentation only |
| **Combinators** | Retained. `_ : lowpass(1000.0, 0.7)` still works alongside `lowpass _ 1000.0 0.7` |
| **Mixer RT code** | Hand-written per-sample mixing logic (channel processing, pan law, bus summing, smoothing) — no dependency on `rill-router` |
| **EQ implementation** | Biquad cascade/parallel per band, hand-written in the mixer built-in |
| **Backward compat** | `Algorithm<T>` untouched. Existing rill-lang programs compile unchanged (combinator path still works for single-signal built-ins) |

## Architecture overview

```
rill-lang source
       │
       ▼
┌──────────────────────────────────────┐
│ Parser                               │
│ - juxtaposition-only Apply           │
│ - record literals { key: val }       │
│ - enum identifiers (pre, post, ...)  │
└──────────────┬───────────────────────┘
               ▼
┌──────────────────────────────────────┐
│ Type checker (unified infer_apply)   │
│ - ParamType::Signal → adds arity_in  │
│ - ParamType::Float/Int → const check │
│ - ParamType::Record → schema validate│
│ - ParamType::Enum → variant check    │
│ - ParamType::Variadic → loop args    │
└──────────────┬───────────────────────┘
               ▼
┌──────────────────────────────────────┐
│ Lowerer (unified Apply lowering)     │
│ - Signal args → lowered regs → srcs  │
│ - Scalar args → const_f64 → params   │
│ - Record args → auto-param generation│
│ - Emits CallBlock { srcs: Vec<Reg> } │
└──────────────┬───────────────────────┘
               ▼
┌──────────────────────────────────────┐
│ IR + Schedule → MultichannelAlgorithm│
│ - CallBlock with multi-input support │
│ - State slots for smoothing/feedback │
│ - num_inputs/num_outputs from arity  │
└──────────────────────────────────────┘
```

## Component design

### 1. `MultichannelAlgorithm<T>` trait (`rill-core`)

```rust
pub trait MultichannelAlgorithm<T: Transcendental>: Send {
    fn num_inputs(&self) -> usize;
    fn num_outputs(&self) -> usize;
    fn process(
        &mut self,
        inputs: &[&[T]],
        outputs: &mut [&mut [T]],
    ) -> ProcessResult<()>;
    fn reset(&mut self);
}
```

Live in `rill-core` alongside `Algorithm<T>`. No changes to `Algorithm<T>`.

Adapter for graph integration (`rill-graph`): wraps `MultichannelAlgorithm<T, BUF_SIZE>` as a `Router` node with `num_inputs()` input ports and `num_outputs()` output ports.

### 2. `ParamType` and new `BuiltinSig` (`rill-lang`)

```rust
pub enum ParamType {
    Signal,
    Float,
    Int,
    String,
    Bool,
    Record(RecordSchema),
    Enum(&'static [&'static str]),
    Variadic(Box<ParamType>),
}

pub struct RecordField {
    pub name: &'static str,
    pub ty: ParamType,
    pub default: Option<f64>,
}

pub struct RecordSchema {
    pub fields: Vec<RecordField>,
}

pub struct BuiltinSig {
    pub name: &'static str,
    pub params: Vec<ParamType>,
    pub signal_outs: usize,
    pub kind: BuiltinKind,
}
```

`signal_ins` = `params.iter().filter(|p| matches!(p, ParamType::Signal)).count()`
(for non-variadic). `signal_outs` may depend on `Variadic` arity or record
config — resolved at type-check time.

### 3. Parser changes (`rill-lang`)

**Removals:**
- Parenthesized application `f(x, y)` — the `LParen` path in `parse_prefix`
- Brace-delimited `where { defs }` — layout-only `where` blocks

**Additions:**
- **Record literals** `{ key: val, ... }` — parsed as `Expr::Record`. Distinguised from `where` blocks by `:` separator (vs `=`). Comma-separated fields.
- **Enum identifiers** — bare `Ident` tokens validated in context by `infer_apply`

**New AST nodes:**
```rust
pub enum Expr {
    // ... existing variants ...
    Record(Vec<(String, Expr)>, Span),
}
```

**Juxtaposition** — already parses `name atom1 atom2 atom3` as `Apply { name, args: [atom1, atom2, atom3] }`. With records as atom-start tokens (`{`), `mixer _ _ _ { buses: 2 }` parses as `Apply { name: "mixer", args: [Wire, Wire, Wire, Record] }`.

### 4. Type checker: unified `infer_apply` (`rill-lang`)

```rust
fn infer_apply(ctx, name, args, span) -> Result<Type> {
    if let Some(sig) = ctx.sigs.builtin_sig(name) {
        let (min_args, max_args) = sig.arg_count_bounds();
        validate_arg_count(args.len(), min_args, max_args)?;

        let mut signal_ins = 0;
        let mut pos = 0;
        for ptype in &sig.params {
            match ptype {
                ParamType::Signal => {
                    let ty = infer_expr(ctx, &args[pos])?;
                    ensure!(ty.arity_out() >= 1, "signal arg must have ≥1 output");
                    signal_ins += ty.arity_in();
                    pos += 1;
                }
                ParamType::Float | ParamType::Int => {
                    validate_constant_or_param_ref(ctx, &args[pos])?;
                    pos += 1;
                }
                ParamType::String => {
                    validate_string_literal(&args[pos])?;
                    pos += 1;
                }
                ParamType::Bool => {
                    validate_bool_literal(&args[pos])?;
                    pos += 1;
                }
                ParamType::Enum(variants) => {
                    validate_enum_value(&args[pos], variants)?;
                    pos += 1;
                }
                ParamType::Record(schema) => {
                    validate_record(&args[pos], schema)?;
                    pos += 1;
                }
                ParamType::Variadic(inner) => {
                    match &**inner {
                        ParamType::Signal => {
                            for arg in &args[pos..] {
                                let ty = infer_expr(ctx, arg)?;
                                ensure!(ty.arity_out() >= 1, "variadic signal arg must have ≥1 output");
                                signal_ins += ty.arity_in();
                            }
                        }
                        _ => {
                            for arg in &args[pos..] {
                                validate_constant_or_param_ref(ctx, arg)?;
                            }
                        }
                    }
                    pos = args.len();
                }
            }
        }
        let signal_outs = sig.resolve_signal_outs(&args);
        Ok(Type::uniform(signal_ins, signal_outs, Scalar::Float))
    } else {
        // user-defined function handling (unchanged)
    }
}
```

### 5. Lowerer: unified `Apply` lowering (`rill-lang`)

```rust
Expr::Apply { name, args: call_args, span } => {
    if let Some(sig) = self.sigs.builtin_sig(name) {
        let mut param_values = Vec::new();
        let mut param_bindings = Vec::new();
        let mut signal_srcs = Vec::new();
        let mut pos = 0;

        for ptype in &sig.params {
            match ptype {
                ParamType::Signal => {
                    let regs = self.lower(&call_args[pos], args)?;
                    signal_srcs.extend(regs);
                    pos += 1;
                }
                ParamType::Float | ParamType::Int => {
                    if let Expr::Ref(ref_name, _) = &call_args[pos] {
                        if let Some(&pidx) = self.param_names.get(ref_name) {
                            param_values.push(0.0); // placeholder
                            param_bindings.push((param_values.len() - 1, pidx));
                            pos += 1;
                            continue;
                        }
                    }
                    param_values.push(const_f64(&call_args[pos])?);
                    pos += 1;
                }
                ParamType::Record(schema) => {
                    extract_and_intern_params(&call_args[pos], schema, &mut self)?;
                    pos += 1;
                }
                ParamType::Enum(_) => {
                    param_values.push(extract_enum_tag(&call_args[pos])?);
                    pos += 1;
                }
                ParamType::Variadic(inner) => {
                    match &**inner {
                        ParamType::Signal => {
                            for arg in &call_args[pos..] {
                                let regs = self.lower(arg, args)?;
                                signal_srcs.extend(regs);
                            }
                        }
                        _ => {
                            for arg in &call_args[pos..] {
                                param_values.push(const_f64(arg)?);
                            }
                        }
                    }
                    pos = call_args.len();
                }
                _ => { pos += 1; }
            }
        }

        let signal_outs = sig.resolve_signal_outs(&call_args);
        let instance = self.builtins.len();
        self.builtins.push(BuiltinInstance {
            name: name.clone(),
            params: param_values,
            kind: sig.kind,
            signal_ins: signal_srcs.len(),
            signal_outs,
            param_bindings,
        });

        match sig.kind {
            BuiltinKind::Block => {
                let fst = self.fresh_reg();
                for _ in 1..signal_outs { self.fresh_reg(); }
                self.emit(Instr::CallBlock { dst: fst, srcs: signal_srcs, instance });
                Ok((0..signal_outs).map(|i| fst + i).collect())
            }
            BuiltinKind::Sample => {
                // Per-sample multi-input
            }
        }
    } else {
        // user-defined function handling (unchanged)
    }
}
```

### 6. IR changes (`rill-lang`)

```rust
pub enum Instr {
    // ... existing variants ...
    CallBlock {
        dst: Reg,
        srcs: Vec<Reg>,   // was: src: Reg (single input)
        instance: usize,
    },
}
```

`CallSample` already supports `srcs: SmallVec<[Reg; 4]>`.

### 7. `RillProgram` implements `MultichannelAlgorithm`

```rust
impl<T: Transcendental> MultichannelAlgorithm<T> for RillProgram<T> {
    fn num_inputs(&self) -> usize { self.ir.num_inputs }
    fn num_outputs(&self) -> usize { self.ir.num_outputs }
    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        // Hybrid executor: block steps over full buffers, sample steps per-sample
        // Multi-channel LoadInput/StoreOutput index accordingly
    }
    fn reset(&mut self) { /* zero state, delay lines */ }
}
```

`StateLayout` gains `num_outputs: usize`. `main` arity check relaxes from `(0|1) → 1` to `(0..N) → (1..M)` — validated when compiling to `MultichannelAlgorithm` target.

### 8. Graph integration (`rill-adrift` / `rill-graph`)

New node type `"rill/lang_multi"` wrapping `RillProgram<T>` as `MultichannelAlgorithm<T>`:

```rust
pub struct MultiLangNode<T: Transcendental, const BUF_SIZE: usize> {
    program: RillProgram<T>,
    input_ports: Vec<Port<T, BUF_SIZE>>,
    output_ports: Vec<Port<T, BUF_SIZE>>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for MultiLangNode<T, BUF_SIZE> { ... }
impl<T: Transcendental, const BUF_SIZE: usize> Router<T, BUF_SIZE> for MultiLangNode<T, BUF_SIZE> { ... }
```

`Router::route()` calls `program.process(input_slices, output_slices)` with port buffers.

## Syntax examples

### Mixer (3 channels, 2 buses, EQ, dry/wet)

```rill
main = mixer _ _ _ {
    channels: {
        kick:  { vol: 0.8, pan: 0.0 },
        snare: { vol: 0.6, pan: -0.3 },
        hat:   { vol: 0.4, pan: 0.5 },
    },
    buses: ["reverb", "delay"],
    sends: {
        kick_reverb: 0.3,
        snare_delay: 0.4,
    },
    master_vol: 1.0,
    eq: {
        bands: {
            b0: { freq: 1000, q: 1.0, gain_db: 3.0,  type: peaking },
            b1: { freq: 200,  q: 0.71, gain_db: -2.0, type: lowshelf },
        },
    },
    dry_wet: 0.5,
}
```

Generated params: `kick_vol`, `kick_pan`, `snare_vol`, `snare_pan`, `hat_vol`, `hat_pan`, `kick_reverb`, `snare_delay`, `master_vol`, `eq_b0_freq`, `eq_b0_q`, `eq_b0_gain_db`, `eq_b1_freq`, `eq_b1_q`, `eq_b1_gain_db`, `dry_wet`.
Arity: 3 inputs → 4 outputs (L, R, bus0, bus1).

### Mixer with named channels and DSP sources

```rill
main = mixer ch0 ch1 ch2 { buses: 2, master_vol: 1.0 }
  where
    ch0 = _ * 0.8
    ch1 = _ * 0.6
    ch2 = sine 440 0.3 0
```

### Standalone EQ

```rill
main = eq_parametric _ {
    bands: {
        b0: { freq: 1000, q: 1.0, gain_db: 3.0,  type: peaking },
        b1: { freq: 200,  q: 0.71, gain_db: -2.0, type: lowshelf },
    },
}
```

### Standalone Dry/Wet

```rill
main = dry_wet dry_signal wet_signal { mix: 0.5 }
  where
    dry_signal = _ * 0.5
    wet_signal = _ : delay 0.3 0.4
```

### Variadic sum

```rill
main = sum a b c d
  where
    a = sine 220 0.1 0
    b = sine 330 0.1 0
    c = sine 440 0.1 0
    d = sine 550 0.1 0
```

### Backward-compatible forms (both valid)

```rill
-- Combinator style (unchanged)
main = _ : lowpass 1000.0 0.7

-- Unified arg style (new)
main = lowpass _ 1000.0 0.7
```

## Mixer RT code structure

The `mixer` built-in compiles to per-sample mixing DSP with the following structure
(hand-written, no rill-router dependency):

```
for each sample in buffer:
    // clear bus accumulators
    for each bus: bus_sum[bus][sample] = 0.0

    // process each channel
    for each channel:
        input = inputs[ch][sample]

        // apply smoothing (EMA) to volume, pan
        current_vol += (target_vol - current_vol) * smoothing
        current_pan += (target_pan - current_pan) * smoothing

        // apply volume and pan (constant-power pan law)
        if muted: left = 0.0; right = 0.0
        else:
            left_gain, right_gain = pan_law(current_pan)
            left = input * current_vol * left_gain
            right = input * current_vol * right_gain

        // accumulate to master
        master_left[sample] += left
        master_right[sample] += right

        // process sends
        for each send:
            if pre_fader: bus_sum[bus][sample] += inputs[ch][sample] * send_level
            if post_fader: bus_sum[bus][sample] += input * current_vol * send_level

    // apply master volume with smoothing
    current_master += (master_vol - current_master) * smoothing
    master_left[sample] *= current_master
    master_right[sample] *= current_master

    // optional EQ on master (biquad cascade)
    if eq enabled:
        master_left[sample] = eq_process(master_left[sample])
        master_right[sample] = eq_process(master_right[sample])

    // optional dry/wet
    if dry_wet_enabled:
        master_left[sample] = mix * master_left[sample] + (1.0 - mix) * dry_input[sample]
```

## Interaction with combinators

Combinators (`:`, `,`, `<:`, `:>`, `~`, `@`) operate on **wire bundles** (arity counts).
Unified arguments supply **signal operands** to built-in function calls.
These two mechanisms are orthogonal but intersect in the same expression:

```rust
lowpass _ 1000 0.7 : _ * 0.5
//          ^         ^^^^^^^
//   signal arg создаёт    комбинатор Seq
//   вход для lowpass      с identity wire
```

### `Seq (:)` — sequential composition

Connects ALL outputs of the left expression to ALL inputs of the right.
For SISO built-ins this works as before:

```rill
some_expr : lowpass _ 1000 0.7     // out(some_expr)=1, in(lowpass)=1 ✓
```

For multi-IO built-ins, Seq requires exact arity match — rarely useful:

```rill
some_expr : mixer _ _ _ { buses: 2 }   // требует out(some_expr)=3
```

**Rule:** Seq is practical only when the left side produces exactly N wires matching
the built-in's N signal inputs. For mixers, per-channel wiring through combinators
is fragile — the recommended approach is signal arguments directly in the Apply call.

### `Par (,)` — parallel composition

Concatenates wire bundles. Works transparently for any arity:

```rill
(mixer a b { buses: 1 }, lowpass c 1000 0.7)
// (2→3) , (1→1) → (3→4)
```

### `Feedback (~)` — feedback with 1-sample delay

Requires the built-in to have at least one signal argument.
Without a signal arg, the built-in is a source `(0→M)` — nowhere to connect feedback:

```rill
+ ~ onepole _ 200 0.5     // _: signal arg создаёт вход для обратной связи ✓
+ ~ onepole 200 0.5       // ✗ onepole без signal arg — источник, in=0
```

### `Split (<:`) / `Merge (:>)` — fan-out / fan-in

Operate on wire bundles, transparent to any arity:

```rill
mixer a b c { buses: 2 } <: some_block    // дублирует 4 провода
(a, b, c) :> sum                           // группирует и суммирует
```

### `Delay (@)` — delay

Works transparently — delays all output wires:

```rill
mixer a b c { buses: 2 } @ 3   // задерживает все 4 выхода на 3 сэмпла
```

### Practical conclusion

For multi-IO built-ins (mixer, dry/wet), routing is done **inside** the built-in
(EQ in record config, sends in record config), **not** through combinators.
Combinators remain the primary mechanism for single-signal processing chains.

## Interaction with graph compilation (rill-lang graph mode)

The [graph-compilation spec](2026-07-07-rill-lang-graph-compilation-design.md) defines
`RillGraphEngine<T, BUF>` — a custom runtime that compiles an entire signal graph from
a rill-lang DSL program into one optimized execution unit with a linear `ScheduledGraph`.

### `RillGraphEngine` — `MultichannelAlgorithm<T>` instead of `Algorithm<T>`

Currently `RillGraphEngine` implements `Algorithm<T>` (SISO). With multi-IO mixers,
it implements `MultichannelAlgorithm<T>`:

```rust
impl<T, const BUF: usize> MultichannelAlgorithm<T> for RillGraphEngine<T, BUF> {
    fn num_inputs(&self) -> usize { self.schedule.inputs }
    fn num_outputs(&self) -> usize { self.schedule.outputs }
    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) {
        // 1. Scatter inputs[0..N] into input buffers
        // 2. Execute steps (InlineProgram, BufferCopy, ReadDelay/WriteDelay)
        // 3. Gather output buffers into outputs[0..M]
    }
}

// SISO adapter for backward compatibility:
impl<T, const BUF: usize> Algorithm<T> for RillGraphEngine<T, BUF> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) {
        assert!(self.num_inputs() <= 1 && self.num_outputs() == 1);
        // delegate to MultichannelAlgorithm::process
    }
}
```

`ScheduledGraph` already stores `inputs: usize` and `outputs: usize` — these
were previously restricted to 0 or 1. The restriction is lifted.

### Static vs dynamic parameters: inline eligibility

A `param` node can be **inlined** into its parent by the graph optimizer only when
ALL its parameters are **compile-time constants** (literals). If any parameter is
a runtime value (formal parameter of `main` / `param` definition), the node must
remain independent so `SetParameter` messages can reach it.

```rill
-- ALL values are literals → mixer CAN be inlined
param myMixer = mixer _ _ _ {
    channels: { kick: { vol: 0.8, pan: 0.0 } },
    master_vol: 1.0,
}

-- kick_vol is a runtime param → mixer CANNOT be inlined, needs keep param
param myMixer kick_vol = mixer _ _ _ {
    channels: { kick: { vol: kick_vol, pan: 0.0 } },
    master_vol: 1.0,
}
```

In the second case, `kick_vol` is adjustable via `SetParameter { anchor: "myMixer", param: "kick_vol", value: 0.5 }`.

### Beta-reduction already handles constant propagation

The existing `reduce.rs` module performs beta-reduction: user-defined function calls
are inlined via substitution, and compile-time constants are folded. This means:

- **Literals** (`0.8`, `440`, `peaking`) are already resolved to constants in the
  reduced AST — no `SetParameter` machinery needed.
- **Formal parameters** (`kick_vol`, `cutoff`) become `param()` slots with dynamic
  bindings — the optimizer detects these and keeps the node independent.
- The optimizer simply checks whether the lowered `Ir` contains any dynamic param
  references. If none → inline eligible.

This requires **no new mechanism** — beta-reduction + param detection in the
lowered IR is sufficient.

### Whole-graph JIT (future)

Even with `keep param` nodes that cannot be inlined, the JIT compiles the entire
`ScheduledGraph` into **one native function**. `InlineProgram` steps become basic
blocks; zero-copy buffer aliasing means no data movement between blocks. The result
is a single compiled function with optimal register allocation across all nodes.

Example: a 4-node graph (kick, snare, hat, mixer) where only the mixer has
dynamic params:

```
JIT function:
  block kick:     source → buffer[0]
  block snare:    source → buffer[1]
  block hat:      source → buffer[2]
  block mixer:    buffer[0..2] → buffer[3..6]  (zero-copy aliased = same memory)
  return buffer[3..6]
```

Two `InlineProgram` steps (the 3 sources are merged by parallel-merge pass,
the mixer is separate). No buffer copies. One JIT function.

## Implementation phases

| # | Phase | Scope |
|---|-------|-------|
| 1 | Foundation | `ParamType`, `BuiltinSig` redesign, record parser, `Expr::Record` in AST |
| 2 | Type checker | Unified `infer_apply`, mixed signal/scalar validation, `Variadic` support |
| 3 | Lowerer + IR | Multi-input `CallBlock`, signal arg lowering, record param extraction |
| 4 | Backend | `RillProgram` implements `MultichannelAlgorithm`, multi-IO `process` |
| 5 | Mixer built-in | Hand-written mixer RT code, smoothing, pan law, bus summing |
| 6 | EQ built-in | Biquad cascade, BandType support |
| 7 | Dry/Wet built-in | Stereo dry/wet mixing |
| 8 | Graph integration | `MultiLangNode` in `rill-adrift`, `Router` impl, `"rill/lang_multi"` factory |
| 9 | Cleanup | Remove parenthesized `Apply`, brace-delimited `where`, update all tests |
