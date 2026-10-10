# rill-lang: Full Track Unification (Stage B B3) — Design

> **Stage:** B, task B3 of the main plan.
> **Parent spec:** `2026-10-10-rill-lang-value-signal-unification-st-design.md` (§2.6).
> **Scope:** eliminate the block track ENTIRELY — not just `Rate::Signal` in the
> type system, but the whole computation path. Everything is a closure
> (Haskell-style). FFI algorithms become fragments with a single
> `ValueCallForeign` instruction.

---

## 0. Executive summary

rill-lang has two parallel execution paths: the **block track** (`Instr` +
`block_regs` + `run_block_mimo` + `build_schedule` + `exec_block_op` +
`exec_foreign_block`) and the **value track** (`ValueInstr` + arena +
`run_value_track`). B3 unifies them into a **single linear lambda model**:
there are only lambda functions (applicability determined by the type system),
all data is equal, and there is exactly ONE computation model. `Rate`, `Channel`,
and `Scalar` are removed from the type system entirely; `ArrowTy` becomes
`{ ins: Vec<ValueTy>, outs: Vec<ValueTy> }`. Signals are just `ValueTy::Buffer`
(an ordinary value type like `List`/`Map`), executed as `Value::Buffer` on the
value track. The block track is deleted. Every computation is a closure:

| Concept | Before (block track) | After (linear lambda model) |
|---|---|---|
| Type system | `Rate`/`Channel`/`Scalar` split | `ArrowTy = Vec<ValueTy>`; all data equal |
| Program | `Instr` list, `build_schedule` topological order | root `FragmentIr` (base=0) |
| Function | `Value::Closure` + `run_fragment` | `Value::Callable` + `run_call` |
| FFI algorithm | `Instr::CallBlock` + `exec_foreign_block` over `block_regs` | fragment with one `ValueCallForeign`, `Algorithm::process` over `Value::Buffer` |
| Math | `Instr::Un`/`Bin`/`Const` over `block_regs` | `Buffer` methods (scalar loops) |
| State | `block_state`/`delays` (double-buffer) | `value_state` (the ST effect) |
| Inputs | `Instr::LoadInput` → `block_regs` | value-track buffer reads |

After B3: `Rate`/`Channel`/`Scalar`, the `Instr` block variants, `block_regs`,
`run_block_mimo`, `build_schedule`, `exec_block_op`, `exec_foreign_block`, and
`schedule.rs` are **deleted**. `run_value_track` and `run_call` merge into a
single `run` (a program is a root fragment). SIMD and the `Algorithm` factories
stay intact.

---

## 1. Current state (audited)

### 1.1 The two tracks

**Block track** (`ir.rs` `Instr`, `program.rs` `block_regs: Vec<FixedBuffer<T,BUF>>`,
`backend/interp.rs`):
- ~14 block `Instr` variants: `Const`, `LoadInput`, `ReadBlockState`,
  `ReadDelay`, `Un`, `Bin`, `Move`, `WriteBlockState`, `WriteDelay`,
  `CallBlock`, `ReadParam`, `ReadActorParam`, `ReadMainCell`, `ProbePoint`.
- `build_schedule` (schedule.rs, 227 lines) builds a topological order (Tarjan
  SCC); `CallBlock` → `Step::ForeignBlock`, rest → `Step::Block`.
- `run_block_mimo` (96 lines): per-tick, executes `exec_block_op` (90 lines,
  math/state/sources) and `exec_foreign_block` (98 lines, `Algorithm::process`
  over `block_regs`).
- `process()` (program.rs:724, 799) calls `run_block_mimo`.

**Value track** (`ir.rs` `ValueInstr`, `arena.rs`, `backend/interp.rs`):
- `Value::Callable { fragment, env, cell: Option }` (unified Closure/Thunk, T1).
- `run_value_track` (program path, base=0) + `run_call` (fragment, base=offset)
  share `exec_value_instr(prog, instr, base, drops)` (T2).
- `exec_value_instr` is a ~659-line match over `ValueInstr`.

### 1.2 FFI

`foreign fn name : FixedBuffer a -> ... -> FixedBuffer a` declares the
signature (language owns it); Rust `Algorithm`/`MultichannelAlgorithm`
implementations register factories (`register_block`). Lowering emits
`Instr::CallBlock { dst, srcs, instance }`; `exec_foreign_block` reads
`block_regs[srcs]`, calls `Algorithm::process`, writes `block_regs[dst]`.
`BuiltinInst::Block` = `BlockBuiltin<T>` (= `Algorithm<T>` + `set_param`);
`BuiltinInst::MultichannelBlock` = `MultichannelAlgorithm<T>`.

### 1.3 Inputs/outputs

`process(inputs: &[&[T]], outputs: &mut [&mut [T]])` (MultichannelAlgorithm)
and `process(input, output)` (Algorithm) call `run_block_mimo`. `main`'s signal
arity = input count; `LoadInput` copies each input slice into `block_regs[dst]`;
outputs copy from `block_regs[reg]` back to the external slices. `main` λ-params
are both named params (`set_param`) and signal inputs.

---

## 2. Design

### 2.1 Linear lambda model — everything is a closure

rill-lang is a **linear lambda model**: there are only lambda functions, whose
applicability is determined by the type system, and all data is equal. The type
system has no `Rate`/`Channel`/`Scalar` — `ArrowTy = { ins: Vec<ValueTy>, outs:
Vec<ValueTy> }`. A signal is an ordinary `ValueTy::Buffer` value; combinators
operate on `ValueTy` vectors.

After B3 there is ONE value track. A program is a root fragment; every callable
is a `Value::Callable`; every dispatch goes through `run_call` →
`exec_value_instr`. `run_value_track` and `run_call` merge because a program is
just a fragment with `base=0` and an empty env:

```
program   = FragmentIr (entry block, base=0)
function  = Value::Callable { fragment, env, cell }  →  run_call
FFI       = FragmentIr whose single instruction is ValueCallForeign { builtin }
          →  run_call → exec_value_instr(ValueCallForeign) → Algorithm::process(Buffer)
math      = Buffer methods (scalar loops)
state     = value_state (ST effect)
```

### 2.2 `Value::Buffer`

A signal buffer becomes a first-class arena value:

```rust
/// A first-class signal buffer. `FixedBuffer` (inline) and `RegionHandle`
/// (large slab, SP-4) are the two concrete backing kinds.
Value::Buffer(BufferKind)
```

Two options for the backing:
- **Inline:** `Value::Buffer(FixedBuffer<T, BUF>)` — small buffers, stack/arena
  inline (Tier 2 memory, SP-4). Simple; the buffer is an ordinary owned arena
  value.
- **Slab handle:** `Value::Buffer(usize)` — an index into a program-level
  `RegionHandle` table (Tier 1, big objects). For large buffers.

Stage B uses **inline `Value::Buffer(FixedBuffer<T, BUF>)`** first (matches the
current `block_regs` capacity model, no new memory subsystem); the
`RegionHandle` path (Task B6) layers on later. The `Buffer` typeclass (Task B4)
abstracts over the backing.

`visit_refs`/`clone_pooled`/`drop_ref` handle `Value::Buffer` as a leaf (owns no
arena children) or a table-index (no children).

### 2.3 `ValueCallForeign` instruction

In `ir.rs`, add a value-track instruction:

```rust
/// Call a foreign DSP algorithm (`Algorithm::process`) over `Value::Buffer`
/// operands. `instance` indexes `Ir::builtins` (the `BuiltinInstance`).
ValueInstr::ValueCallForeign { dst: usize, srcs: Vec<usize>, instance: usize },
```

In `exec_value_instr`, the arm:
1. Read `srcs` registers — each must be a `Value::Buffer` (input channels).
2. Call `Algorithm::process`/`MultichannelAlgorithm::process` with the input
   buffers and write the output buffer(s) into `dst` (+ consecutive regs).
3. In-place: the output buffer is a fresh `Value::Buffer` (or the first input
   reused per the `dst != srcs[0]` aliasing rule — carry over the current
   scratch-snapshot for multichannel).

The foreign call is wrapped in its own tiny `FragmentIr` (one instruction), so
it dispatches through the SAME `run_call` path as any function.

### 2.4 Lowering signals to the value track

`lower.rs` `lower_with_cafs_mode`: the `has_value_out` branch becomes the ONLY
branch. `main.body()` always lowers through `lower_value` (value track). Signal
constructs map as follows:

| Signal construct (old `Instr`) | New lowering |
|---|---|
| `LoadInput` | a value-track input read → `Value::Buffer` (one per main input) |
| `Un`/`Bin`/`Const` math | `Buffer` method calls (Task B4; for B3, lower to a `ValueCallForeign`-like buffer op or a placeholder that the math instance fills) |
| `CallBlock` (FFI) | a one-instruction fragment with `ValueCallForeign` |
| `ReadBlockState`/`WriteBlockState` | `value_state` reads/writes (already exist) |
| `ReadDelay`/`WriteDelay` | delay in `value_state` (or a `Buffer` ring) |
| `ReadParam`/`ReadActorParam`/`ReadMainCell` | value-track param reads (already exist) |
| outputs | value-track output registers → copy to external slices |

The 1251-line `lower` (block path) and its 132 `Instr::` emits are replaced by
value-track lowering. This is the biggest chunk of B3.

### 2.5 Runtime unification

`program.rs`:
- Remove `block_regs`.
- `process()` calls the value-track entry (merged `run_value_track`/`run_call`)
  with `inputs` materialized as `Value::Buffer` and `outputs` drained from the
  value-output registers.

`backend/interp.rs`:
- Merge `run_value_track` + `run_call` into a single **`run`** (program = root
  fragment, base=0, empty env). One runner; everything is a closure.
- Add the `ValueCallForeign` arm to `exec_value_instr`.
- Delete `run_block_mimo`, `exec_block_op`, `exec_foreign_block`.

`schedule.rs`: delete. `ir.rs`: remove `Instr` block variants, keep `ValueInstr`
+ `ValueCallForeign`. `regalloc.rs`: only handled block IR — delete or neuter.

### 2.6 State

`block_state`/`block_state_next` (double-buffer) and `delays` move to
`value_state` (the value-track 1-tick delay store). Feedback (`~`) and delay
(`@`) become `value_state` reads/writes — the ST effect.

---

## 3. Backward compatibility, RT safety

- **Behavior:** existing signal programs (`lang_chiptune`, `record_mic`, signal
  test suite) keep working. `main : inputs -> outputs` stays.
- **RT safety:** one value track; `Value::Buffer` math adds no allocation beyond
  the existing arena/region budget. The `ValueCallForeign` path reuses the
  current `Algorithm::process` (no new allocation).
- **SIMD:** preserved — math uses scalar loops (LLVM auto-vectorizes) or
  `rill_core::math::vector`; manual SIMD avoided.
- **Budget:** the arena capacity heuristic must now cover buffers (each
  `Value::Buffer` is one slot). The 2× complete-mode multiplier already exists.

---

## 4. Testing

- `tests/rate_unification.rs`: signal programs type with `ArrowTy = Vec<ValueTy>`
  (no `Rate`/`Channel`); a signal output is `ValueTy::Buffer`.
- The full existing suite (signal + value) passes unchanged after each sub-task.
- `lang_chiptune`/`record_mic` compile (and run on hardware).
- New: a signal program (e.g. `main = _ : onepole 200.0 0.7`) produces the same
  output as before via the value track.

---

## 5. File map

| Area | Files |
|---|---|
| Types | `types/ty.rs` (remove `Rate`/`Channel`/`Scalar`; `ArrowTy = Vec<ValueTy>`; `ValueTy::Buffer`), `types/infer.rs`, `types/unify.rs`, `types/arrow.rs`, `types/ffi.rs` |
| IR | `ir.rs` (`ValueCallForeign`, remove `Instr` block variants) |
| Arena | `arena.rs` (`Value::Buffer`) |
| Lowering | `lower.rs` (value-track-only; signal→Buffer) |
| Interp | `backend/interp.rs` (merge runners, `ValueCallForeign`, delete block track) |
| Program | `program.rs` (remove `block_regs`), `schedule.rs` (delete), `regalloc.rs` (delete) |
| Tests | `tests/rate_unification.rs`, existing suite |

---

## 6. Implementation order (safe, incremental)

The track unification proceeds in four steps. The two-track model stays working
until the last step; nothing is deleted early. **The new types are tested with
their own tests, not by keeping the old signal tests green** — the old signal
tests target the `Rate`/`Channel`/`Scalar` type system that is being removed, so
they are rewritten (or replaced) for the linear lambda model rather than
dragged through the refactor.

- **B3-1 — Buffer value type + operations, parallel to the signal track.**
  Add `ValueTy::Buffer` and the `Buffer` typeclass operations (from
  `SIGNAL_PRELUDE`, backed by rill-core buffers). At the same time, transition
  the TYPE SYSTEM to the linear lambda model: remove `Rate`/`Channel`/`Scalar`,
  `ArrowTy = { ins: Vec<ValueTy>, outs: Vec<ValueTy> }`. The signal track still
  executes underneath (the block `Instr`/`block_regs` remain). New tests cover
  `Buffer` typing/operations; the old signal tests are updated to the new model.
- **B3-2 — Switch program entry/exit to `Value::Buffer` (value track).**
  `main`'s inputs/outputs become `Value::Buffer` values; signal programs run
  through the value track. The block track is now dead code (still present,
  not referenced).
- **B3-3 — Verify.** Full suite + `lang_chiptune`/`record_mic` still produce the
  same output (compile + run).
- **B3-4 — Delete the dead signal track.** Remove the block `Instr` variants,
  `block_regs`, `run_block_mimo`, `build_schedule`, `exec_block_op`,
  `exec_foreign_block`, `schedule.rs`; merge `run_value_track` + `run_call` into
  a single `run`. Full suite green.

Each step lands independently; the new-model tests are the source of truth for
B3-1/B3-2, and B3-3/B3-4 confirm the end-to-end output is unchanged.

---

## 7. Non-goals (deferred)

- `(arg..) -> IO` form (Stage C).
- `RegionHandle` big-buffer values (Task B6).
- `Buffer` typeclass math methods (Task B4).
- Stream I/O, zero-copy at the boundary (Stage C).
