# rill-lang: Arrow Core, Free Variables (CAF), and Block Buffers — Design

> **Status:** Approved — 2026-09-24.
> **Date:** 2026-09-24
> **Branch:** `feature/rill-lang-vars`
> **Scope:** Formalize rill-lang as a category of signal arrows over block channels
> (`(Block<Scalar>..) → (Block<Scalar>..)`), add Haskell-style free variables
> (CAF semantics for closed top-level definitions), binding-level laziness, and
> migrate runtime block storage from raw `Vec` to `rill_core::buffer` fixed buffers.

## 1. Problem statement

The rill-lang DSL compiles a Faust-style block diagram into a signal processor.
Three architectural gaps motivated this change:

1. **Free variables are inlined away.** Top-level `name = expr` definitions are
   β-reduced (textually inlined) at every reference site. A *closed* stateful
   definition (`osc = sine 440 0.5 0`) referenced from two places compiles into
   two independent stateful instances. Haskell semantics require **one shared
   instance** (a CAF — Constant Applicative Form). This is needed so that
   user-defined functions can reference **global buffers** and shared state as
   free variables without threading them through parameters.

2. **The type model conflates channels with samples.** `Type { ins: Vec<Scalar>,
   outs: Vec<Scalar> }` uses `Vec<Scalar>` for the wire bundle. `Scalar` is a
   single sample; a channel is a *block* of samples. The program is a
   **per-tick block transform** executed by the backend on every hardware tick,
   yet the type system does not make the block nature of a wire explicit.

3. **The runtime is channel-0-hardcoded and uses raw `Vec`.** `Runtime::launch`
   drives one input channel and one output channel regardless of backend channel
   count. Backends are multichannel; the engine (`MultichannelAlgorithm`,
   `ProgramRunner`) is channel-aware, but the runtime glue is not. Runtime block
   storage (`block_regs`, `block_state`, delay rings) is `Vec<Vec<T>>`, grown at
   runtime, instead of the pre-allocated fixed buffers rill mandates.

## 2. Semantic model

### 2.1 Three type levels

| Level | Type | Meaning |
|---|---|---|
| sample | `Scalar` | one element inside a block (`Int` / `Float` / `Var`) |
| channel | `Block` | one signal channel: a fixed-size buffer of samples (`BUF_SIZE`), mutated in place each tick |
| arrow | `ArrowTy` | a block transform `(I₁:Block..Iₙ) → (O₁:Block..Oₘ)` |

### 2.2 A program is an arrow

A program `main : (I₁:Block<Scalar>…Iₙ) → (O₁:Block<Scalar>…Oₘ)` is a
**block-diagram arrow** in the sense of Hughes' *Arrows*. It is *not* a monad:
parallel composition (` , `) cannot be expressed as monadic bind.

Each hardware tick the backend feeds n input blocks through the program and
receives m output blocks. Stateful DSP (oscillators, filters, delay lines) lives
*inside* the arrow and persists between ticks.

### 2.3 Arrow combinators

| Surface | Arrow | Law |
|---|---|---|
| `_` | `identity` | `(X) → (X)` |
| `!` | `cut` | `(X) → ()` |
| literal `3.5` | `arr (const 3.5)` | `() → (T)` |
| `A : B` | `Seq` (comp) | `out(A) == in(B)`; associative; `_` is unit |
| `A , B` | `Par` (product) | `(in A + in B, out A + out B)`; interchange |
| `A <: B` | `Split` (fan-out) | `in(B) = k·out(A)` |
| `A :> B` | `Merge` (fan-in, sum) | `out(A) = k·in(B)` |
| `A ~ B` | `Loop` (feedback) | 1-block delayed loop; `in(B) ≤ out(A)`, `out(B) ≤ in(A)` |
| `A @ n` | `Delay n` | block-level ring buffer; `n` const |
| `+` `-` `*` `/` `%` | `Bin` | per-block elementwise |
| `sin cos …` | `Un` | per-block elementwise |

Fan-out/fan-in factors (`k`) depend on inferred arities and are resolved at
lowering time (arity-indexed arrows), as today.

## 3. Arrow core

### 3.1 Meta-level (Haskell-like host calculus)

Definitions `name p₁..pₙ = body`, juxtaposed application, `let`/`where`,
mutual recursion per binding group. `Scheme { lam_count, vars, ty }` separates
λ-parameters (meta-level) from signal channels (object-level). User-defined
function calls are eliminated by β-reduction, exactly as today.

### 3.2 Object-level (arrow expressions)

The existing `Expr` AST is **restructured** (no duplicate AST, no desugar pass)
into an explicit arrow core `ArrowExpr`:

```rust
pub enum ArrowExpr {
    Identity, Cut,
    ConstInt(i64), ConstFloat(f64), ConstImag(f64), Str(String),
    Var(String),                          // bound arrow (def / free var / CAF / λ-param)
    Seq(Box, Box), Par(Box, Box),
    Split(Box, Box), Merge(Box, Box),     // fan-out / fan-in (factor by type)
    Loop(Box, Box),                       // 1-block delayed feedback
    Delay(Box, Box),                      // @ n
    Bin(BinArith, Box, Box), Un(UnOp, Box),
    Call { name, args, resource, params, actor_params, record },
    Let { defs, body },                   // mutually-recursive binding group
    ActorParam { name, default },
    Param { name, default },
    Record(Vec<(String, ArrowExpr)>),
}
```

Restructuring is mechanical (rename/group constructors by category); the parser,
inference, and lowering keep their pipeline position. `infer` runs over
`ArrowExpr` assigning an `ArrowTy` to every term; `lower` becomes an arrow
interpreter mapping each constructor to linear-IR registers/instructions.

## 4. Type model changes (`types/ty.rs`, new `types/arrow.rs`)

```rust
/// A signal channel: a block of samples. `elem` is the per-sample type.
pub struct Block { pub elem: Scalar }

/// A block transform: n input channels → m output channels.
pub struct ArrowTy { pub ins: Vec<Block>, pub outs: Vec<Block> }
```

- `Type` is renamed/replaced by `ArrowTy` (mechanical rename across the crate).
- Arity = `len(ins)` / `len(outs)` = channel count.
- Block semantics documented on each combinator: `~` = 1-block delayed loop,
  `@` = block-level ring buffer, `,` = parallel channels.
- Arrow-law tests assert category identities on the type and on lowered IR:
  associativity of `:`; `_` as unit; interchange `(A,B):(C,D) == (A:C),(B:D)`;
  split/merge distributivity.

## 5. Buffers: `rill_core::buffer` instead of `Vec`

All runtime block storage uses `rill_core::buffer` fixed buffers. Channel counts
are backend-determined, so **all buffers are allocated before launch and mutated
in place**; nothing is allocated per tick.

| Site | Before | After |
|---|---|---|
| register store | `block_regs: Vec<Vec<T>>` | `Vec<FixedBuffer<T, BUF>>` |
| feedback state | `block_state` / `block_state_next: Vec<Vec<T>>` | `Vec<FixedBuffer<T, BUF>>` (double-buffered) |
| `@` delay | `DelayRing<T>` (hand-rolled `Vec` ring) | `DelayLine<T, MAX_DELAY>` (`MAX_DELAY` = max `@ n`) |
| runtime channels | `[0.0f32; BUF]`, channel 0 | `[FixedBuffer<f32, BUF>; MAX_CH]`, all channels |
| wire type | `Scalar` | `Block { elem: Scalar }` ↔ `FixedBuffer<T, BUF>` |

- `RillProgram<T>` and `ProgramEngine<T>` are generified over `const BUF: usize`
  (matches the intended `compile_graph::<T, BUF_SIZE>` direction).
- `Vec` remains only in fixed-size structural collections allocated once at
  construction (register/state lists) — RT-safe, no hot-path allocation.
- **`BufferPool` is NOT used** in the signal path: `acquire()` locks a
  `parking_lot::Mutex` (`rill-core/src/buffer/pool.rs:7`).

## 6. Multichannel runtime (`runtime.rs`)

- `Runtime::launch` reads the backend channel counts at launch
  (`IoCapture`/`IoPlayback`), verifies them against the program's
  `num_inputs()`/`num_outputs()`, and pre-allocates
  `[FixedBuffer<f32, BUF>; MAX_CH]` before `driver.run()`.
- The per-tick callback reads every input channel (`read_input(c, …)`), feeds
  the program, writes every output channel (`write_output(c, …)`).
- **Policy:** strict arity match at the program↔backend boundary; a mono
  effect applied to a multichannel backend is expressed by node replication per
  channel in the graph layer (existing `GraphBuilder` pattern) or an explicit
  multichannel program.

## 7. Free variables (CAF) and binding-level laziness

### 7.1 CAF set

In `types/infer.rs`, `infer_def_group` already computes each definition's final
`Scheme { lam_count, ty }`. A **top-level `Local`** with
`lam_count == 0 && ty.ins.is_empty()` (zero input channels) is **closed** and
becomes a CAF. The set of CAF names is stored on `TypedProgram`.

### 7.2 Reduce

In `reduce.rs`, a `Var` referencing a CAF is **not inlined** (both passes);
open blocks (input arity > 0) stay macro and are inlined as today. CAF bodies
are still β-reduced (e.g. `osc = my_sine 440` → `sine 440 0.5 0`).

### 7.3 Lower

In `lower.rs`, a `Var` referencing a closed CAF is **lifted once**: its body is
lowered with no input args and its output registers are cached in
`cafs: HashMap<String, Vec<usize>>`; subsequent references return the cached
registers. Sharing is ordinary DAG fan-out, which the scheduler already handles.

- **Recursion guard:** a CAF that (transitively) references itself during
  lifting is a compile error, not a stack overflow (today `a = a` overflows).
- **Builtin const-parameter edge case:** `cutoff = 1000.0;
  main = _ : lowpass cutoff 0.7`. When a builtin Float/Int/Record argument is a
  `Var` to a closed CAF, its body is const-folded via `const_f64`; if it does
  not fold, it is a compile error (as today for non-constants).

### 7.4 Laziness (binding-level)

- A CAF is lowered into the schedule **iff referenced** (dead-code elimination
  of unreferenced definitions by reference).
- The schedule is strict: everything referenced runs every tick.
- Stateful sub-arrows (oscillators, `write_head`) are never skipped — skipping
  would change observable behavior (effects).
- Feedback is only the 1-block delayed loop; instantaneous `loop` is out of
  scope (would require per-sample execution and break the block/SIMD model).

### 7.5 Global buffers

The existing `TapeLoop` + resource machinery is sufficient. A global buffer is a
top-level closed definition (`tape = TapeLoop 4096`) captured by functions as a
free variable; resource references already survive reduction and resolve at
lowering. No new buffer syntax is added.

## 8. Behavior change (documented)

A closed stateful top-level local referenced two or more times changes from N
independent copies to 1 shared instance:

```faust
osc = sine 440 0.5 0;
main = osc, osc;      // was 2 oscillators, now 1 shared
```

To get independent instances, define distinct names or parametrize with an
input/λ-argument. `where`/`let` bindings keep macro semantics (re-instantiated
per enclosing call — consistent with Haskell).

## 9. Out of scope

- Instantaneous `loop` / lazy streams (needs per-sample execution).
- `Signal<Complex>` as a single wire (complex is 2 wires today).
- JIT backend; whole-graph-as-one-program lowering.
- New buffer syntax beyond existing resources.

## 10. Testing

1. Arrow laws: associativity/unit/interchange/split-merge at type and IR level.
2. CAF: `osc, osc` lowers to 1 sine built-in; `voice amp = osc * amp;
   main = voice 0.3, voice 0.7` lowers to 1 sine + 2 muls.
3. Open blocks stay macro: `integ = + ~ _; main = integ, integ` → 2 state slots.
4. `cutoff = 1000.0` as a builtin param still compiles.
5. Recursion `a = a` → compile error.
6. Resources (`write_head`/`read_head`) regress-free.
7. Multichannel runtime: 2→2 program drives both channels.
8. Buffer migration: `FixedBuffer`-backed register store keeps existing behavior
   (`lang_chiptune_ir_structure`, hybrid/reference equivalence tests).

## 11. Documentation

Update `docs/src/guides/rill-lang.md`: a program is a block arrow
`(Block<Scalar>..) → (Block<Scalar>..)`; the three type levels
(Scalar / Block / ArrowTy); CAF semantics for closed top-level definitions;
binding-level laziness; per-tick execution model; the documented behavior
change in §8.

## 12. Files

| File | Change |
|---|---|
| `rill-lang/src/types/ty.rs` | `Type` → `ArrowTy`; add `Block` |
| `rill-lang/src/types/arrow.rs` | arrow-category documentation + law tests (new) |
| `rill-lang/src/types/infer.rs` | infer over `ArrowExpr`; CAF set on `TypedProgram` |
| `rill-lang/src/arrow.rs` | `ArrowExpr` (restructured `Expr`) |
| `rill-lang/src/reduce.rs` | leave CAF `Var`s; recursion guard |
| `rill-lang/src/lower.rs` | arrow interpreter; CAF lifting + const-fold edge case |
| `rill-lang/src/program.rs` | `RillProgram<T, const BUF>`; `FixedBuffer`/`DelayLine` |
| `rill-lang/src/program_engine.rs` | `ProgramEngine<T, const BUF>` |
| `rill-lang/src/runtime.rs` | multichannel launch, pre-allocated `[FixedBuffer; MAX_CH]` |
| `rill-lang/src/lib.rs` | `compile_graph::<T, BUF_SIZE>`; compile pipeline |
| `rill-lang/src/graph/*` | arity plumbing via `ArrowTy` |
| `rill-lang/tests/*` | new tests (§10) |
| `docs/src/guides/rill-lang.md` | language reference updates |

## 13. Implementation phases

| # | Phase | Scope |
|---|-------|-------|
| 1 | Type model | `Block`/`ArrowTy`, rename, arrow-law tests |
| 2 | Arrow core | restructure `Expr` → `ArrowExpr`, infer, lower-as-interpreter |
| 3 | Buffers | `const BUF` generification, `FixedBuffer`/`DelayLine` |
| 4 | Multichannel runtime | all channels, pre-allocated channel buffers |
| 5 | CAF / free vars / laziness | CAF set, reduce/lower changes, recursion guard, const-fold |
| 6 | Tests + docs + verify | tests, guide, `cargo test/clippy/fmt` |

Verification: `cargo test -p rill-lang`, then `cargo test --workspace`,
`cargo clippy --workspace`, `cargo fmt`. Zero warnings before merge
(`AGENTS.md` warnings policy).
