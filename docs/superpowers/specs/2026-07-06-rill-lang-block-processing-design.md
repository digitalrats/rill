# rill-lang — Hybrid Block Processing

> **Status:** Design approved — awaiting implementation plan.
> **Date:** 2026-07-06
> **Branch:** `feature/rill-lang` (follow-on increment; the crate MVP is already implemented on this branch).
> **Depends on:** `docs/superpowers/specs/2026-07-06-rill-lang-design.md` (the crate MVP).

## Motivation

The MVP interpreter evaluates the linear IR **sample by sample**. That is only
strictly required for **feedback** (`~`), whose recurrence needs sample *n−1* to
compute sample *n*. Everything else — arithmetic, math builtins, fan-out/fan-in,
and feedforward delay — has no intra-block dependency and can be processed a
**whole buffer at a time**, which is dramatically faster (amortized dispatch,
auto-vectorization).

This increment adds **hybrid block/sample processing** (the chosen "Option B" —
full region partitioning): feedforward regions run block-wise via the existing
vector eDSL, and only true recurrences run per-sample. It is the single most
impactful performance step short of the JIT, and — critically — it introduces
the region schedule that the two planned follow-ons build on.

### Reuses the existing eDSL

`rill_core::math::vector` already provides SIMD whole-buffer primitives that map
1:1 onto the IR's `BinArith` / `UnOp`:

| IR op | Vector-eDSL call |
|---|---|
| `Bin{Add/Sub/Mul/Div}` | `add_slices` / `sub_slices` / `mul_slices` / `div_slices` |
| `Bin{Min/Max}` | `min_slice` / `max_slice` |
| `Un{Sin/Cos/Tan/Exp/Ln/Sqrt/Abs/Tanh}` | `sin_slice` / … / `abs_slice` (`tanh` via `map`) |

All operate on `[T]` in chunks of 4 through `ScalarVector4<T>`.

## Scheduling algorithm (compile-time, over `Ir`)

1. **Contract read/write pairs.** For each feedback slot, `ReadState(s)` +
   `WriteState(s)` collapse to one graph node; for each delay line,
   `ReadDelay(l)` + `WriteDelay(l)` collapse to one node. Each pair shares state
   and therefore must co-execute in the same region.
2. **Build the data-dependency graph** over instructions using the SSA registers
   (edge: consumer → producer), then add a **loop-closing edge** for every
   contracted state/delay node (the cross-sample recurrence).
3. **Tarjan SCC** → **condensation DAG** → **topological order**.
4. **Classify each SCC (in topo order) as a `Step`:**
   - a single combinational instruction (`Const`, `LoadInput`, `Un`, `Bin`,
     `Move`) → **`Step::Block`** (whole-buffer op);
   - anything containing a recurrence or a contracted state/delay node →
     **`Step::Sample`** (per-sample loop over its instructions, original order).

### Why this is correct

- Contracting read/write pairs guarantees a slot's/line's two ends land in the
  same region.
- Adding loop-closing edges makes **feedback-through-delay** (`+ ~ (_ @ 3)`) a
  single SCC → one sample region, while a **standalone** `_ @ 3` (no feedback)
  is an isolated sample region and its surrounding math stays block.
- The condensation is a DAG, so a topological order always exists; executing
  steps in that order guarantees each step's inputs are already computed.

### Worked classifications

| Program | Schedule |
|---|---|
| `_ * 0.5` | all `Block` |
| `abs(_) : _ * 2` | all `Block` |
| `+ ~ _` | one `Sample` region |
| `+ ~ (_ * 0.5)` | one `Sample` region |
| `(_ * 0.5) : (+ ~ _)` | `Block` (gain) → `Sample` (integrator) |
| `_ @ 3` | isolated `Sample` region (delay) |

## Execution model

A single register store `regs: Vec<Vec<T>>` — **each register holds a whole
buffer** (length = the call's block length `n`), computed in `T` (not `f64`) so
the vector eDSL delivers real SIMD. The store is lazily grown to `n` and reused
(zero allocation after warm-up), honoring the RT rules.

- **`Step::Block(instr)`** operates on the `[..n]` slice:
  - `Const` → fill; `LoadInput` → copy the input slice; `Un`/`Bin` → the matching
    `*_slice`/`*_slices` call; `Move` → copy.
- **`Step::Sample(instrs)`** loops `i in 0..n`, evaluating its instructions
  scalar-wise: it reads and writes `regs[reg][i]` (external inputs were produced
  by earlier block steps; region outputs are consumed by later steps), and
  threads `state` slots and delay rings across `i`.

Both modes share the same `regs` store — whole-slice vs `[i]`-indexed — so they
compose with no glue. A fully-feedback program degenerates to one sample region
(≈ today's behavior); a fully-combinational one is all block ops.

### Numerics

The block path computes in `T` (e.g. `f32`), replacing the MVP's `f64` scalar
intermediates. Existing tests are either `f32`-exact (gain, integrator, delay)
or use `f32` tolerances (leaky integrator), so they continue to hold.

## Correctness strategy

Keep the MVP's per-sample interpreter as a **reference oracle** (`pub(crate)`),
and add equivalence tests: for a suite of programs (feedforward, feedback,
delay, and mixed) over random inputs, assert `hybrid ≈ reference` within an
`f32` tolerance, alongside the existing exact-value tests. This pins the hybrid
to the already-trusted behavior.

## Scope / non-goals (this increment)

- **In:** the scheduler, the block executor (via vector eDSL), the sample-region
  executor, integration into `RillProgram`/`compile()`, equivalence + perf tests.
- **Deferred:** a fused **block delay op** (feedforward delays run in an isolated
  sample region for now); **vectorizing long-distance recurrences** (a feedback
  whose delay ≥ block size could be chunked); the Cranelift `jit` backend.

## Foundation for the next two steps

This schedule is deliberately the substrate for the roadmap:

1. **Whole-graph-as-one-program.** Modeling an entire rill signal graph (many
   nodes) as a single rill-lang program lowered to one IR + schedule lets the
   scheduler fuse the whole chain into a minimal set of block loops and a few
   sample regions — realizing the spec's "keep the block in L1, flatten the
   chain" thesis across node boundaries, not just within one node.
2. **JIT compilation.** The linear IR plus the block/sample region schedule is
   the shared lowering target for a future Cranelift backend: block regions
   become vectorized loops and sample regions become scalar loops with carried
   state — a direct, mechanical translation of the same structure the
   interpreter runs.
