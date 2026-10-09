# rill-lang: Value/Signal Unification via ST — Design (Stage B)

> **Stage:** B of the rill-lang evaluation-model rework.
> **Prerequisite:** Stage A (lazy value track, `strict`/`complete` modes) — see
> `2026-10-04-rill-lang-lazy-eval-design.md`.
> **Scope:** Unify the signal and value tracks through an **ST-style effect
> monad**: `Buffer` becomes a first-class mutable value, Faust operators become
> methods of the `Buffer`/`Arrow` typeclasses, and `:` becomes a single overload
> (cons for `List`, seq for `Buffer`). The `(arg..) -> IO` program form and
> stream I/O are **deferred to Stage C**, but this design is shaped to receive
> them.

---

## 0. Executive summary

rill-lang today has **two parallel tracks**: the signal/block track
(`Instr`/`block_regs`/`run_block_mimo`, SIMD whole-buffer) and the value track
(`ValueInstr`/arena/`run_value_track`, lazy). They are separated by `Rate`
(`Signal` vs `Value`) and cannot mix. Stage B unifies them by making a **signal
buffer a first-class, mutable value** and modelling signal processing as an
**ST-style effect monad**:

- `Buffer` is a typeclass; `FixedBuffer a` and `RegionHandle` are instances.
- Buffer **math is in-place mutation** (ST-style), implemented with scalar loops
  that LLVM auto-vectorizes (manual SIMD does not pay off — AGENTS.md).
- A signal block is a **`Kleisli ST Buffer Buffer` arrow**; Faust operators
  (`:`, `,`, `<:`, `:>`, `~`, `@`) desugar to `Arrow` methods.
- **`:` becomes a single overload**: cons for `List`, seq for `Buffer`,
  dispatched through the typeclass machinery (`class_of_method` →
  `resolve_method`).
- `ST` carries a **phantom type `s`**; `runST (fn s -> body)` opens an explicit
  region with a fresh `s` and a check-escaping guarantee (one-point rank-2, not
  full rank-2 HM).

The block track is **not deleted** — it is the existing runtime underneath. We
re-frame its semantics (math = `Buffer` methods, state = ST effect, opaque DSP =
`Buffer` arrows) while keeping SIMD and the `Algorithm` factories intact.

---

## 1. Context and current state (audited)

### 1.1 The signal/block track is the only runtime

An audit of the codebase (this branch) found that the graph-engine system
described in `docs/src/architecture/graph.md` (`GraphIr`, `GraphBuilder::build_ir`,
`graph_compiler::compile`, `graph_optimize::optimize`, `CompiledGraphEngine`,
`NodeClosure`, `Port<T, BUF_SIZE>`, pointer-routed buffers) **does not exist in
the source**. Only the frontend survives:

- `rill-graph::GraphBuilder` → `to_graph_spec()` → `GraphSpec` → `reconstruct()`
  → rill-lang AST → `compile_program` → the same block track.

The block track is the **sole execution engine**:

| File | Role |
|---|---|
| `ir.rs` `enum Instr` | Linear SSA block IR (~14 block variants + value variants) |
| `schedule.rs` `build_schedule` | Topological order (Tarjan SCC); `CallBlock` → `ForeignBlock`, rest → `Block` |
| `backend/interp.rs` `run_block_mimo` | Per-tick execution: `exec_block_op` / `exec_foreign_block` |
| `program.rs` `block_regs: Vec<FixedBuffer<T,BUF>>` | One physical `[T; SIZE]` buffer per register |

The 12 block `Instr` variants actually emitted by `lower.rs`, grouped:

| Category | `Instr` | Nature |
|---|---|---|
| **Opaque DSP** | `CallBlock` | `Algorithm::process(&mut block_regs[dst], &[in])` — e.g. `sine`, `biquad`, `ay38910`, `lofi` |
| **Buffer math** | `Un`, `Bin`, `Const` | Element-wise SIMD over `block_regs` (`apply_un_slice`/`apply_bin_slice` → `rill_core::math::vector`) |
| **State/delay** | `ReadBlockState`, `WriteBlockState`, `ReadDelay`, `WriteDelay` | 1-tick feedback (`~`), delay lines (`@`), double-buffer swap |
| **Sources** | `LoadInput`, `ReadParam`, `ReadActorParam`, `ReadMainCell` | Program inputs, control params, main λ-params |

### 1.2 Buffers, aliasing, and I/O

- **`block_regs` is a stack of `FixedBuffer<T, BUF>`** (`[T; SIZE]`, inline). Each
  `fresh_reg()` is a distinct buffer; `dst` never aliases `a`/`b`. `regalloc.rs`
  exists but is dead (not invoked).
- **`CallBlock` mutates `block_regs[dst]`** — exactly "apply `Algorithm` to a
  buffer". `exec_foreign_block` guards `dst != srcs[0]` and snapshots multichannel
  inputs into stack scratch.
- **I/O boundary is a copy**: `LoadInput` copies from the external slice (DMA) into
  `block_regs[dst]`; outputs copy back out. There is no zero-copy DMA inside
  `RillProgram` today.
- **I/O model already exists** (`rill-core/src/io.rs`): `IoCapture::read_input`,
  `IoPlayback::write_output`, `IoBackend { active }`, `NullBackend`. These are the
  seeds of `InputStream`/`OutputStream` for the future `(arg..) -> IO` form.

### 1.3 `:` (colon) today

`:` is `InfixOp::Seq` → `Expr::Seq`. In value position `infer_seq` calls
`reject_value_channels` → a compile error. So `x : xs` (cons-list) is impossible
until `:` becomes overloaded. Faust sequencing uses `:` as `Seq` on the signal
track.

### 1.4 Readymade infrastructure

- **`Buffer` typeclass exists** as a marker: `typeclass Buffer b where { }`,
  `instance Buffer (FixedBuffer a)`, `instance Buffer (Tape f32)` (`ty.rs`).
  Comment: "the home of the Faust combinators and buffer math in later stages."
- **`RegionHandle`/`BufferAllocator`** (`rill-core/src/memory/region.rs`, SP-4):
  reference-counted views (offset + length) over a chunked slab, single-writer /
  many-readers, Miri-checked. Designed as the future memory of `Array`/`Vector`
  big objects.
- **Monadic infrastructure**: `Monad`/`bind`/`return`, `Kleisli m a b`,
  `Arrow` typeclass, `do { x <- e; ... }` syntax — all working for `Maybe`/`List`
  (`tests/categories.rs`). Typeclass dispatch (`class_of_method` →
  `resolve_method`) works for `Arrow (Kleisli m)`.
- **SIMD**: `rill_core::math::vector` provides `add_slices`, `sin_slice`, etc. and
  `SliceMut` (`+=`, `-=`, `*=`, `/=`, scalar). Per AGENTS.md, **LLVM
  auto-vectorizes simple scalar loops**; manual `ScalarVector4` attempts did not
  pay off (3 failures). Scalar loops are preferred.

### 1.5 Reference programs

- `rill-adrift/examples/lang_chiptune.rs`: `main regs = ay38910 1750000.0 regs : lofi ...` —
  `Algorithm` + `:` seq, params via `SetParameter`, null input backend.
- `rill-adrift/examples/record_mic.rs`: `main = _` (wire), live input backend.
- `rill-adrift/examples/play_wav.rs`: non-DSL manual chain.

---

## 2. Design decisions

### 2.1 Approach

**Approach A — `Kleisli ST Buffer Buffer`.** A signal block is an arrow
`Kleisli ST Buffer Buffer`. Faust operators desugar to `Arrow` methods over this
arrow. `Buffer` is a first-class mutable value. ST is the effect monad that
localizes buffer mutation (a tick is a `runST` region). This is the chosen
approach.

Rationale: it uses the existing `Arrow`/`Kleisli`/`Monad`/`do` machinery, makes
signal blocks first-class composable values, and matches the long-term
`(arg..) -> IO` goal (an ST foundation is the substrate for IO).

### 2.2 `:` single overload

`: ` becomes one operator, dispatched by the type of its operands through the
typeclass machinery:

- `List` operands → **cons** (`x : xs`, element : list).
- `Buffer` operands → **seq** (Faust sequence).

Dispatch uses the existing `class_of_method` → `resolve_method` path. Two methods
share the `:` spelling; resolution picks by operand type. (Exact class structure
is refined in the plan; the constraint is that `:` is *one* syntax with
type-directed dispatch.)

### 2.3 Math is in-place mutation

Buffer math **mutates its first operand** (ST-style `a += b`), it does not
allocate a fresh result buffer. This matches the ST single-writer model and
avoids per-op allocation:

```rill
typeclass Buffer b where {
    add:  b -> b -> ST ();
    sub:  b -> b -> ST ();
    mul:  b -> b -> ST ();
    div:  b -> b -> ST ();
    rem:  b -> b -> ST ();
    neg:  b -> ST ();
    sin:  b -> ST ();
    cos:  b -> ST ();
    tan:  b -> ST ();
    sqrt: b -> ST ();
    exp:  b -> ST ();
    ln:   b -> ST ();
    tanh: b -> ST ();
    abs:  b -> ST ();
    min:  b -> b -> ST ();
    max:  b -> b -> ST ();
    fill: Scalar -> b -> ST ();   -- constant fill (was `Const`)
}
```

Implementation uses **scalar loops** (LLVM auto-vectorizes), not manual SIMD.
The existing `rill_core::math::vector` helpers remain available; we prefer simple
`SliceMut`-style loops in the instances.

### 2.4 `ST` with a phantom type `s`

`ST` is an effect monad modelled on Haskell, with a phantom type parameter `s`:

```rill
data ST s a = { unST: a };   -- s is phantom: appears in types, not in data
```

`runST` opens an explicit region:

```rill
runST (fn s -> body) : a
```

- `fn s -> ...` is an ordinary lambda, but `s` is bound as a **type-constructor
  variable** (`TyConVar`), not a value variable.
- The body is inferred with `s` as a fresh `TyConVar`.
- **Check-escaping**: after inference, verify that `s` does not appear free in the
  result type. This is a **one-point rank-2** (enough for ST isolation), not full
  rank-2 HM.

This reuses the existing data-type-with-type-parameters mechanism (`Kleisli m a b`).
Full rank-2 polymorphism may be needed later for IO; this design only needs the
one-point form.

### 2.5 Faust operators as `Arrow` methods

The signal combinators map onto `Arrow`/`Kleisli` methods:

| Faust op | `Expr` | `Arrow`/mapping |
|---|---|---|
| `a : b` | `Seq` | seq for `Buffer` (the `:` overload's Buffer instance); `compose` for arrows |
| `a , b` | `Par` | `both` / parallel |
| `a <: b` | `Split` | `fan` / split |
| `a :> b` | `Merge` | merge |
| `a ~ b` | `Loop` | feedback (1-tick) |
| `a @ n` | `Delay` | delay line |

These become `Buffer`/`Arrow` methods; the desugar pass rewrites `Expr::Seq`/`Par`/
`Split`/`Merge`/`Loop`/`Delay` in signal position into the corresponding method
calls (or keeps the AST and dispatches at lower).

### 2.6 Scope boundary with IO

Stage B does **not** introduce the `(arg..) -> IO` program form. The existing
`main : inputs -> outputs` binding and the block track remain the entry/exit.
However:

- `Buffer` as a value is the substrate for stream I/O later.
- The ST effect model is the foundation for IO.
- `IoCapture`/`IoPlayback` are unchanged; Stage C will turn them into
  `InputStream.read` / `OutputStream.write` inside `(arg..) -> IO`.

---

## 3. Architecture

```
┌────────────────────────────────────────────────────────────┐
│ Language                                                   │
│  Buffer = typeclass (FixedBuffer, RegionHandle instances)  │
│  ST s a  = effect monad with phantom s                     │
│  Signal  = Kleisli ST Buffer Buffer (Arrow)                │
│  Faust ops : , <: :> ~ @  → Arrow/Buffer methods           │
│  ':' overload: List→cons, Buffer→seq                       │
├────────────────────────────────────────────────────────────┤
│ Compiler                                                   │
│  desugar: Faust ops → method calls; ':' → dispatch         │
│  infer: Buffer/Arrow/ST typing, runST check-escaping       │
│  lower: arrows/methods → ops over Buffer (SIMD via rill-core)│
├────────────────────────────────────────────────────────────┤
│ Runtime                                                    │
│  Arena<Value> + RegionHandle (big objects)                 │
│  one tick = runST region                                   │
│  Buffer math = scalar loops (LLVM auto-vectorized)         │
│  Algorithm (CallBlock) unchanged                           │
└────────────────────────────────────────────────────────────┘
```

### 3.1 Components

1. **`Buffer` typeclass** (`types/ty.rs` prelude): methods + instances.
2. **`ST` + `runST`** (`parser.rs`, `ast.rs`, `types/infer.rs`, `reduce.rs`):
   `data ST s a`, `runST (fn s -> ...)`, phantom `s`, check-escaping.
3. **`:` overload** (`parser.rs` → `desugar.rs`, `infer.rs`, `lower.rs`):
   one spelling, type-directed dispatch (List→cons, Buffer→seq).
4. **Faust ops → methods** (`desugar.rs`, `infer.rs`, `lower.rs`): `Seq`/`Par`/
   `Split`/`Merge`/`Loop`/`Delay` in signal position map to Arrow/Buffer methods.
5. **Buffer math execution** (`backend/interp.rs`, new `Buffer` instances):
   scalar loops over the buffer; SIMD via `rill_core::math::vector` where it pays.
6. **RegionHandle as a `Buffer` value** (`arena.rs`, `memory/region.rs`): make
   `RegionHandle` a `Value` (or a value-carrying handle) so buffers are
   first-class.

### 3.2 Data flow

A signal expression now composes `Buffer` arrows inside a `runST` region. The
boundary stays `main : inputs -> outputs`; internally each stage is a `Buffer`
arrow. `CallBlock` (opaque DSP) and math both operate on `Buffer` values.

---

## 4. Error handling

- **Buffer math on exhausted/borrowed buffers** latches a `ProcessError` (as
  today's value-track exhaustion — Stage A I1 fix).
- **runST check-escaping failure** is a compile error (s escaped into result).
- **`:` dispatch ambiguity** is a compile error ("ambiguous `:` for type ...").

---

## 5. Backward compatibility, RT safety, budget

- **Behavior**: existing signal programs (`lang_chiptune`, `record_mic`, the
  signal test suite) keep working. `main : inputs -> outputs` stays.
- **RT safety**: the block track and `Algorithm` factories are unchanged. Buffer
  math adds no allocation beyond the existing arena/region budget.
- **SIMD**: preserved — math uses scalar loops that LLVM auto-vectorizes, or the
  existing `rill_core::math::vector` helpers; manual SIMD is avoided.
- **Regalloc**: `regalloc.rs` stays dead (out of scope); fresh-buffer-per-result
  is acceptable until a dedicated buffer-reuse pass is designed.

---

## 6. Testing

- **New**: `Buffer` math (in-place, correct values, no aliasing issues),
  `ST`/`runST` check-escaping, `:` overload (cons vs seq by operand type),
  Faust-operator-as-Arrow (seq/par/split/merge/loop/delay), `Buffer` as a
  first-class value (stored in `List`, passed to functions).
- **Regression**: all existing `rill-lang` signal and value tests, the category
  tests (`Arrow (Kleisli m)`), and `lang_chiptune`/`record_mic` compile + run.

---

## 7. Verification

- `cargo test --workspace`, `cargo clippy --all-features --workspace`,
  `cargo fmt` — all green, 0 warnings.
- Signal test suite (ffi, graph, categories, arrow) passes unchanged.
- `lang_chiptune` and `record_mic` examples compile and run.

---

## 8. File map (Stage B)

| Area | Files |
|---|---|
| `Buffer` typeclass | `types/ty.rs` (`SIGNAL_PRELUDE`), `types/infer.rs` |
| `ST`/`runST` | `parser.rs`, `ast.rs`, `reduce.rs`, `types/infer.rs`, `lower.rs` |
| `:` overload | `parser.rs`, `desugar.rs`, `types/infer.rs`, `lower.rs` |
| Faust ops → methods | `desugar.rs`, `types/infer.rs`, `lower.rs` |
| Buffer math exec | `backend/interp.rs`, new `Buffer` instances, `memory/region.rs` |
| RegionHandle as value | `arena.rs`, `memory/region.rs` |
| Tests | `tests/buffer_st.rs`, `tests/*` regression |

---

## 9. Non-goals (deferred to Stage C)

- `(arg..) -> IO` program form.
- `InputStream.read` / `OutputStream.write` reactive/passive stream I/O.
- Full rank-2 HM (only the one-point `runST` form is added).
- Buffer-reuse / regalloc; buffer zero-copy at the I/O boundary.
- `Array`/`Vector` collection types built on `RegionHandle` memory.
