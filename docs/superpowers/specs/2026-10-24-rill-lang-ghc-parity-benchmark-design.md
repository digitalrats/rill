# rill-lang: GHC Parity Benchmark — Haskell 2010 reference harness for value-memory decisions

> **Status:** Design approved-in-conversation (2026-10-24); implementation planned
> separately via `superpowers:writing-plans`.
> **Date:** 2026-10-24
> **Scope:** a standalone, long-lived benchmark tool that runs one ST/array/list/map
> workload on **both** rill (production path) and **GHC 9.10 + Haskell 2010 / GHC
> Prelude**, verifies that both produce the same final result, and reports relative
> iteration time.
> **Not in this design:** the memory-manager implementation itself, CI wiring,
> allocation/pressure profiling as a first-class metric.

---

## 0. Executive summary

The memory-layers work decides how `runST` regions allocate, collapse, and hand off
values. Two questions must be answerable at every step of that work:

1. **Behavioral parity** — does rill produce the same observable result as a
   standard Haskell implementation of the same workload? (GHC / Haskell 2010 is the
   north star; low-level allocation profiles will differ by construction — GC vs
   RC + arena — and that difference is *not* what "parity" means here.)
2. **Performance impact of decisions** — what does rill's memory model cost,
   relative to GHC, on that workload; and how does that cost move after each
   accepted decision?

The tool: two programs, one per language, same algorithm, same final value; one
driver that compiles and runs both on the same machine and prints a report whose
**first number is per-iteration time** (rill vs GHC, absolute and ratio).

This replaces the earlier plan's ad-hoc `lang_bench` ST workload. The memory plan's
baseline/measurement tasks (Task 0.1 and Task 3.2 in
`docs/superpowers/plans/2026-10-24-rill-lang-memory-layers.md`) are re-pointed at
this harness.

---

## 1. Why this exists

- The memory manager is a **behavior-preserving** change. Its acceptance criterion
  is "RILL behaves like GHC on this workload, and the perf delta is understood".
- A one-off measurement cannot tell whether a decision helped or hurt. A standing
  harness re-measures after each decision, so "the decision's influence on
  performance" becomes a number rather than an opinion.
- rill's north star is **Haskell 2010 + GHC Prelude compatibility**. Any place
  where rill's idiomatic code must deviate from that (missing Prelude function,
  different spelling, different evaluation strategy) is a *known gap* — it must be
  recorded, not hidden inside benchmark code.

---

## 2. Goals, non-goals, anti-goals

### Goals

- **G1 — Behavioral parity check.** Both implementations run to completion and
  produce the **same final result**; the driver compares them and fails loudly on
  mismatch. This is the *only* parity criterion (user decision: final result only).
- **G2 — Relative performance.** Report per-iteration wall time for rill and GHC
  and their ratio, on the same machine, same run.
- **G3 — Re-runnability.** One command re-measures after any memory-manager change;
  results are comparable across runs (fixed workload, pinned iteration counts,
  same output shape).
- **G4 — Coverage of the three memory-relevant constructs** (user decision):
  - **arrays** — mutable `STArray` inside a `runST` region (`newArrayFlat` /
    `writeArrayFlat` / `readArrayFlat`),
  - **lists** — cons lists in the value arena (a different allocation mechanism
    from arrays),
  - **`map` over functions** — higher-order application, where rill may diverge
    from GHC.
- **G5 — Deviation register.** Every non-idiomatic spelling forced on the rill side
  is recorded in a checked-in register with the Haskell-2010/GHC form it deviates
  from. The register doubles as a parity-gap backlog.

### Non-goals

- Not a general language benchmark suite; not a DSP/signal-throughput benchmark.
- Not a comparison of low-level allocation profiles (no attempt to match GC vs
  RC+arena behavior, allocation counts, residency, or collection timing).
- No CI gating in this design (can be added later behind an explicit flag).
- Does not implement or modify the memory manager.

### Anti-goals (would make this a failure even if it runs)

- **A benchmark that "passes" by measuring something other than the intended
  workload** — e.g. GHC dead-code-eliminating the work, or rill returning a
  constant because the result was folded at compile time. Waste-source isolation is
  therefore a hard requirement (sink the result so it cannot be elided).
- **Comparing incomparable work** — different element counts, different iteration
  counts, different result types, or a Haskell program that maps to a different
  algorithm.
- **Parity claimed from a benchmark that cannot fail** — if the two results are not
  compared by the driver, the run proves nothing.
- **Silent rill deviations** — an unrecorded workaround in `rill.hs`-side code makes
  the "north star" claim false.

---

## 3. Decisions made (user)

| Decision | Value |
|---|---|
| First report number | **Per-iteration wall time** (absolute for each side + ratio) |
| Parity criterion | **Final program result only** (not memory profiles) |
| Reference implementation | **Real GHC** (9.10.3 available; stack 3.11.1, cabal 3.16.1), idiomatic **Haskell 2010 + GHC Prelude** |
| Workload coverage | **arrays + lists + `map` over functions** |
| rill path | **Production path only** (`compile` → `ProgramRunner::run`), never `test_eval` |
| Deviation policy | rill must be as idiomatic as Haskell 2010 permits; every deviation is registered |

### Decisions left to the builder (explicitly delegated)

- Exact element counts and iteration counts (must be large enough to dominate
  harness overhead, small enough to run in seconds).
- Metric plumbing: how GHC and rill are timed (e.g. repeated process runs vs
  in-process loop), warm-up policy, number of samples, how the median/ratio is
  computed.
- Output shape (human table; machine-readable JSON optional).
- Directory/name of the tool and its runner script.
- Whether GHC is invoked as `ghc -O2` binary or `runghc` (a compiled `-O2` binary is
  required for a fair comparison; `runghc` is not acceptable for measurement).

---

## 4. The workload (the contract both sides implement)

One tick = one `runST` region (or the Haskell equivalent) computing a scalar result
from three sub-workloads, composed so that **all three** participate in the result:

1. **Array sub-workload** — allocate an `STArray` of `N` elements, fill every
   element (value depending on index), overwrite at least one element, read a few
   elements (including one overwritten) and combine them.
2. **List sub-workload** — build a **finite** cons list of `M` elements (explicit
   cons chain / list literal — rill's value track has no `from`/`take` today),
   apply `map` with a function over it, and reduce the mapped list with
   `fold`/sum to a scalar.
3. **`map`-over-function component** — at least one `map` whose argument is a
   user-defined function (not a built-in), and one nested `map` (map of a map), so
   higher-order application and function-value allocation are exercised.

The final result is a scalar (or a small record of scalars) rendered identically on
both sides (rill: `show_float`/`show_int` to `stdout`; Haskell: `print` of the same
shape), and the driver compares the rendered form.

**Determinism and waste isolation (hard requirement):**

- The workload's inputs must be driven from a runtime value that the compiler cannot
  constant-fold. On the rill side this must be a **parameter read or a value derived
  from the program's input path**, not a literal; on the Haskell side an argument
  read at run time (`getArgs`, or a value threaded through `IO`). If the driver
  supplies `N`/`M` as arguments, both sides must consume them the same way.

### Runtime parameter channel (verified against the current code, 2026-10-24)

The rill production path already has an outside→program channel, and it is **not**
`test_eval`:

- **`?name` (actor/control parameter)** is readable in the value track of a
  `main : IO ()` program (verified: `main = do { write (Just (show_float ?g)) stdout;
  return () }` runs and observes the declared default; a `runner.world_mut()
  .set_param("g", ParamValue::Float(v))` before `run()` is observed).
- A `?n` value is instantiated as `Float`, but it is accepted where the flat
  array size/index expects an integer when the float is integral (verified:
  `newArrayFlat (Pair { first: 0, second: ?n }) 4 …` with `?n = 3.0` and
  `readArrayFlat a ?n` with `?n = 2.0` both run) — the documented integral-float
  acceptance in `forced_i64`.
- `param("name", …)` is the **signal-track** control source (arity `0 → 1`); it is
  *not* a value-track construct and a program using it in a value position is
  rejected at compile time (verified: "param first argument must be a string
  literal"). Do not use it in the benchmark.
- The `main : IO ()` gate is enforced at compile time (`main = ?g * 1.0` is
  rejected with "`main` must have type `IO ()`").

Consequence: the driver passes `N`/`M` on the rill side as `?n`/`?m` control
parameters (`set_param` before each run), and on the Haskell side as `getArgs`
values — same numbers, two idiomatic spellings. The fact that rill's `?n` is a
Float while Haskell's argument is an `Int` is recorded as a deviation
(`deviations.md`), not hidden.

### Where the iteration loop lives (verified, 2026-10-24)

Repetition happens **inside each program**, not by re-running the process from the
driver. This keeps fixed costs out of the measured region:

- rill: `complete`-mode recursion works inside `main : IO ()`, including a `runST`
  body (verified: `loop k acc = if k == 0.0 then acc else loop (k - 1.0) (acc + runST …)`
  runs to completion). Use a recursive loop with the workload in its body.
- Haskell: a recursive `go`/`loopM` in `main`, same shape.
- **Lazy infinite lists are not available in the rill value track today** (verified:
  `from 1.0` in `main : IO ()` fails with "unknown identifier `from`"), so list
  workloads must build *finite* lists explicitly (list literal / explicit
  cons-chain of length `M`), not via `from`/`take`.

**Consequence for argument parsing cost:** `getArgs`/`read` on the Haskell side is
performed **once, before the iteration loop**, so string→number conversion never
appears in the measured region. Its cost is O(µs) against O(10⁶+) iterations — below
the report's resolution. The same applies to rill's `?n` read (an instruction, not
a per-iteration parse).
- The result is **sunk**: rill writes it to the captured console (`write (Just
  (show_float result)) stdout`); Haskell prints it and/or forces it with `evaluate`
  before a timing loop. A program whose result is never observed is not admissible.
- Identical `N`, `M`, overwrite index, and map functions on both sides.
- The correctness check is a **real comparison**: the driver extracts both rendered
  results and asserts equality; a mismatch is a hard failure of the whole run.

### What is explicitly *not* required to match

- Number/order of allocations, arena occupancy, GC timing, peak residency, or any
  low-level memory profile. (User decision: final result only.)

---

## 5. Architecture

```
rill/bench/ghc-parity/            (not a Cargo workspace member)
├── README.md                     # how to run; what the report means
├── run.sh                        # one command: build both, run both, compare, report
├── rill-workload.rill            # production-path program (main : IO ())
├── ghc-workload.hs               # idiomatic Haskell 2010 + Prelude
├── deviations.md                 # register of rill deviations from Haskell 2010/GHC
└── (optional) driver/            # small Rust bin or shell+Criterion glue
```

- **Not a workspace member**: keeps `cargo test --workspace` free of GHC
  requirements. The rill side is driven by a small binary/example that depends on
  `rill-lang` (path dependency or `cargo run --manifest-path`).
- **`rill-workload.rill`** runs on `compile::<f32, 256>(src, ForeignRegistry::new())`
  + `ProgramRunner::run()`; repeated runs via the runner's re-seat semantics. The
  workload is a `main : IO ()` program (ST region bound at top level, result
  written to stdout), exactly as `tests/haskell2010_syntax.rs` and `tests/io_stdio.rs`
  demonstrate.
- **`ghc-workload.hs`** is plain Haskell 2010: `Data.Array.ST` (or `STUArray`) in
  `Control.Monad.ST`, lists built with `[1..n]`/`map`/`foldl'`, compiled with
  `ghc -O2`.
- **`run.sh`** performs: build rill driver → build `ghc -O2` binary → correctness
  run on both (compare rendered results, fail on mismatch) → timed runs (same
  machine, sequential) → print report.
- **Timing**: each side runs a fixed number of iterations around the workload body;
  harness overhead is measured and subtracted or made negligible by iteration
  count (builder's call). Warm-up runs precede measurement. Report median (or
  minimum) and the ratio.

### Report shape (first column = per-iteration time)

```
workload: arrays N=…, lists M=…, map(nested)=yes
parity:   PASS  (result = "…")

metric            rill            ghc            rill/ghc
iter time (µs)    1234.5          210.3          5.87x
```

Machine-readable JSON output is optional and left to the builder.

---

## 6. Deviation register (`deviations.md`)

A checked-in table, one row per rill deviation, each with:

| rill spelling | Haskell 2010 / GHC Prelude form | Why rill differs | Status |
|---|---|---|---|

Rules:

- Every non-idiomatic construct in `rill-workload.rill` has a row. The benchmark's
  claim ("rill is measured against idiomatic Haskell") stays true only while this
  table is complete.
- A deviation that is a *gap* (rill lacks a Prelude function or semantics) is a
  backlog item, not a benchmark workaround. The benchmark uses the closest
  rill-idiomatic spelling today and records the gap.
- When rill closes a gap, its row is deleted and the rill workload moves to the
  idiomatic spelling — the workload gets **more** idiomatic over time, never less.

Known candidates to check while building (from the current surface):

- `runST` bindings must be top-level (`ref = runST (…)`), not monadic `<-` inside
  `do` — `runST` yields a pure value (see `tests/io_stdio.rs` comment).
- Array ops are `newArrayFlat`/`writeArrayFlat`/`readArrayFlat` with explicit flat
  bounds, not `Data.Array.ST`'s `newArray`/`writeArray`/`readArray` with `Ix`.
- `show_float`/`show_int` render scalars where Haskell would use `show`/`print`.
- Lists are lazy cons chains in the arena; `fromList`/`toList` bridge to the
  memory-stream surface in some contexts (verify which is idiomatic for a pure
  list computation).
- **Runtime parameters: rill uses `?n` (Float, value-track actor parameter set via
  `world_mut().set_param`) where Haskell 2010 uses `getArgs` + `read` (Int).**
  A `?n` used as a flat array size/index relies on integral-Float acceptance.
- **No lazy infinite lists in the rill value track** (`from` is not in scope in
  `main : IO ()`), while Haskell 2010 naturally has `[1..m]`/`take`. Finite lists are
  built explicitly on the rill side.
- **`param(name, …)` is signal-track only** and must not appear in the rill workload.
- Nothing in the benchmark may use `test_eval` (test-only API).

---

## 7. Acceptance criteria

1. `run.sh` completes from a clean checkout with GHC/stack/cabal present, and exits
   non-zero if the two final results differ.
2. Both programs are the *same* algorithm with identical parameters; the parameters
   come from runtime input (no constant folding) and the result is sunk.
3. The report's first number is per-iteration time for rill and GHC plus the ratio.
4. `deviations.md` accounts for every non-idiomatic rill construct used.
5. Re-running after a memory-manager change produces a comparable report without
   editing the workload (parameters/iteration counts unchanged).
6. No `test_eval` on the rill side; no new dependencies in the rill workspace.
7. `README.md` states what is and is not being compared (no allocation-profile
   claims).

---

## 8. Risks and mitigations

| Risk | Mitigation |
|---|---|
| GHC optimizes the workload away | Runtime-parameterized sizes, sunk result, `-O2` with `-ddump-simpl` checked once during construction |
| rill folds the workload at compile time | Same: runtime-derived sizes/inputs; verify the result depends on the parameter value |
| Incomparable algorithms | One contract (§4); identical parameters; deviation register |
| Harness overhead dominates | Iteration counts large enough; warm-up; report harness-only baseline |
| rill's production path cannot express the workload idiomatically | Deviation register + closest rill spelling; escalate as a gap, never use `test_eval` |
| GHC version drift changes results | Pin the tested GHC version in README; report it in the run output |

---

## 9. Relationship to the memory-layers plan

- `docs/superpowers/plans/2026-10-24-rill-lang-memory-layers.md` Task 0.1 (ST/array
  workload in `lang_bench` + baseline) and Task 3.2 (benchmark after + gate) are
  **superseded**: their workload/measurement role moves to this harness.
- The memory-layers plan's Phase 0 becomes "construct this harness + record the
  baseline"; Task 3.2 becomes "re-run this harness + compare".
- `lang_bench.rs` is currently **stale** (does not compile: `RillProgram` and the
  one-argument `compile` are gone). This harness does not depend on it; whether to
  repair `lang_bench.rs` separately is out of scope here.
