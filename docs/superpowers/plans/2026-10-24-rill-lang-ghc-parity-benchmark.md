# GHC Parity Benchmark Harness — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a standalone harness that runs one arrays+lists+`map` workload on both rill (production path) and GHC 9.10 (idiomatic Haskell 2010), proves the final results match, and reports per-iteration wall time for each side plus their ratio.

**Architecture:** A non-workspace directory `bench/ghc-parity/` holds two programs implementing one contract, a small Rust driver crate that compiles/executes the rill program in-process and spawns the compiled GHC binary, and a `run.sh` that builds both and prints the report. The rill side uses only the production path (`compile` → `ProgramRunner::run`), never `test_eval`. Inputs arrive at runtime (`?n`/`?k` on rill, `getArgs` on GHC) so neither compiler can fold the workload away; the result is written to stdout and compared.

**Tech Stack:** Rust (`rill-lang` path dependency), Haskell 2010 + GHC 9.10.3 (`-O2`), bash. No new dependencies in the rill workspace.

**Spec:** `docs/superpowers/specs/2026-10-24-rill-lang-ghc-parity-benchmark-design.md`

## Global Constraints

- **Production path only.** rill must be driven by `compile::<f32, 256>(src, ForeignRegistry::new())` + `ProgramRunner::run()`. `test_eval` is forbidden in the harness.
- **Runtime inputs.** Array size, iteration count, and the value-shift parameter arrive at runtime: rill via `?n`/`?k`/`?d` set with `world_mut().set_param(name, ParamValue::Float(v))`; GHC via `getArgs` + `read`.
- **Waste isolation.** The final result is derived from the runtime parameters (so `-O2` cannot constant-fold it), is written to stdout, and the driver parses and compares it.
- **One contract.** Both programs use the same array element count, the same fixed list length (64), the same iteration count, the same shift value, and the same left-fold association order.
- **Do not add a workspace member.** `bench/ghc-parity/driver/Cargo.toml` carries its own empty `[workspace]` table so `cargo test --workspace` never builds it.
- **No new external crates.** The driver uses only `std` (+ `rill-lang`, `rill-core`).
- **English only** in code, comments, and docs; commit messages single-quoted.
- **Timing excludes compilation.** Both sides are built once; only execution runs are timed.

## Review Focus

1. **A workload both compilers fold away** — the run must visibly depend on runtime parameters; verify by running with two different parameter values and asserting the outputs differ.
2. **An unfair comparison** — different element counts, iteration counts, association orders, or one side including compile/startup cost the other does not.
3. **A parity check that cannot fail** — the driver must compare parsed results and exit non-zero on mismatch; verify by temporarily perturbing one program's output.
4. **rill harness quietly using `test_eval`** — a grep for `test_eval` in `bench/ghc-parity/` must return nothing.
5. **Fragile DSL assumptions** — the rill workload must be proven to compile and run in a spike before the harness is built; any construct the language rejects is recorded in `deviations.md` and replaced by the closest valid spelling, never by a test-only API.

---

## File Structure

| File | Responsibility |
|---|---|
| `bench/ghc-parity/README.md` | What is compared, how to run, what the numbers mean |
| `bench/ghc-parity/run.sh` | One command: build both, run correctness check, print report |
| `bench/ghc-parity/workload.rill` | rill workload (`main : IO ()`, production path) |
| `bench/ghc-parity/workload.hs` | idiomatic Haskell 2010 workload |
| `bench/ghc-parity/deviations.md` | every rill deviation from Haskell 2010/GHC Prelude |
| `bench/ghc-parity/driver/Cargo.toml`, `driver/src/main.rs` | builds + times both sides, compares results, prints the report |

---

### Task 1: Scaffold, README, and the deviation register

**Files:**
- Create: `bench/ghc-parity/README.md`
- Create: `bench/ghc-parity/deviations.md`
- Create: `bench/ghc-parity/.gitignore` (ignore `target/`, `workload`, `*.hi`, `*.o`)
- Create: `bench/ghc-parity/driver/Cargo.toml`
- Test: `cargo metadata --manifest-path bench/ghc-parity/driver/Cargo.toml` succeeds and the crate is not in the workspace

**Interfaces:**
- Consumes: nothing.
- Produces: the directory skeleton; a driver crate manifest that depends on `rill-lang` by path and declares its own empty `[workspace]`.

- [ ] **Step 1: Create the driver manifest**

`bench/ghc-parity/driver/Cargo.toml`:
```toml
[package]
name = "ghc-parity-driver"
version = "0.1.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
rill-lang = { path = "../../../rill-lang" }
rill-core = { path = "../../../rill-core" }
```

- [ ] **Step 2: Prove it is outside the workspace and resolves**

Run: `cargo metadata --manifest-path bench/ghc-parity/driver/Cargo.toml --format-version 1 > /dev/null && echo OK`
Expected: `OK`.
Run: `cargo metadata --format-version 1 | grep -c ghc-parity-driver`
Expected: `0`.

- [ ] **Step 3: Write README with the comparison contract**

Must state: first metric is per-iteration wall time (absolute per side + ratio); parity means the **final program result only** (allocation profiles intentionally differ: GC vs RC+arena); how to run (`bash bench/ghc-parity/run.sh [ARRAY_ELEMS] [ITERS] [RUNS]`); defaults (`ARRAY_ELEMS=64`, `LIST_LEN=64`, `ITERS=1000`, `RUNS=5`); the GHC version tested (`9.10.3`); and that compilation is excluded from timing.

- [ ] **Step 4: Seed the deviation register**

Table with columns `rill spelling | Haskell 2010 / GHC Prelude form | Why rill differs | Status`, rows:
1. `?n` (Float, `set_param`) | `getArgs` + `read` as `Int` | rill has no argument surface; `?name` is the control channel | open
2. `main : IO ()` with `?name` | `main = do { args <- getArgs; ... }` | no launch arguments | open
3. fixed list literal (64 elements) | `[1..listLen]` | `from`/`take` not in scope in the value track | open
4. `newArrayFlat`/`writeArrayFlat`/`readArrayFlat` with flat bounds | `newArray`/`writeArray`/`readArray` with `Ix` | no `Ix`-classed array ops on the working path | open
5. `show_float` | `show` | scalar renderer differs; driver compares parsed values | open
6. `runST` bound at top level | `runST` inline in `do` | `runST` yields a pure value, not an `m a` | open

- [ ] **Step 5: Commit**

```bash
git add bench/ghc-parity
git commit -m 'bench: scaffold GHC parity harness and deviation register'
```

---

### Task 2: Prove the rill workload shape (spike)

**Files:**
- Create: `bench/ghc-parity/workload.rill`
- Test: `bench/ghc-parity/driver/src/main.rs` (temporary minimal runner) or a scratch test under `rill-lang/tests/` (deleted after)

**Interfaces:**
- Consumes: Task 1's directory.
- Produces: a rill program `main : IO ()` that compiles, runs, prints one float, and depends on `?n`, `?k`, `?d`.

- [ ] **Step 1: Write the workload skeleton and prove it compiles**

`workload.rill` must, per iteration (a recursion counter `k`):
- allocate an `STArray` of `?n` elements via `newArrayFlat (Pair { first: 0, second: ?n }) ?n 0.0`;
- fill every element with `?d + index` (recursive index loop; see Step 2);
- overwrite one element, read back three elements, sum them;
- build the fixed 64-element list literal, apply `map` with a user function that adds `?d`, then a **nested** `map`, then `fold` to a sum;
- accumulate `array_sum + list_sum` across iterations;
- write the final accumulator with `show_float` to `stdout`.

Verify by compiling with a scratch harness:
```
cargo test -p rill-lang --features dsp --test zz_workload_spike
```
Expected: compiles and prints one float.

- [ ] **Step 2: Resolve the loop form empirically (record result in deviations.md)**

Try, in order, and keep the first that compiles and runs:
(a) a top-level recursive function taking the array handle inside the region;
(b) a top-level recursive function returning the accumulated scalar with the `runST` in its body;
(c) a `do`-block helper.
If recursion over the array handle is not expressible, fall back to (b) and record the limitation as a row in `deviations.md` (`status: open`). **Do not** switch to `test_eval` and **do not** use signal combinators (`_`, `<:`, `:>` are parse errors).

- [ ] **Step 3: Prove the output depends on the parameters**

Run the program twice with different `?d` values (e.g. 1.0 and 2.0) using `world_mut().set_param`. Expected: two different printed floats. This is the anti-folding evidence for Review Focus 1.

- [ ] **Step 4: Commit**

```bash
git add bench/ghc-parity/workload.rill bench/ghc-parity/deviations.md
git commit -m 'bench: rill workload for GHC parity harness'
```

---

### Task 3: The GHC workload

**Files:**
- Create: `bench/ghc-parity/workload.hs`
- Test: `ghc -O2 -o bench/ghc-parity/workload bench/ghc-parity/workload.hs` then run with arguments

**Interfaces:**
- Consumes: Task 2's contract (element counts, iteration count, shift, association order).
- Produces: an executable that prints one `Double`, using `getArgs`.

- [ ] **Step 1: Implement the same contract in Haskell 2010**

- `main`: `getArgs` → `[n, iters, d]`; `read` each **once, before the loop**.
- Array: `runSTUArray` / `STUArray` of size `n`, filled with `d + fromIntegral i`, one element overwritten, three read back and summed.
- List: fixed 64-element list, `map` (adding `d`), nested `map`, `foldl'` sum — same association order as the rill `fold`.
- Loop: recursive `go` over `iters`, accumulating the per-iteration scalar.
- Print with `show` (driver compares parsed values, so rendering need not match).

- [ ] **Step 2: Build with `-O2` and verify against two parameter values**

Run: `ghc -O2 -fforce-recomp -o bench/ghc-parity/workload bench/ghc-parity/workload.hs`
Run: `bench/ghc-parity/workload 64 1000 1.0` and `... 2.0`
Expected: two different numbers (anti-folding evidence).

- [ ] **Step 3: Confirm the optimiser keeps the work**

Run: `ghc -O2 -ddump-simpl -dsuppress-all -fforce-recomp bench/ghc-parity/workload.hs -o /dev/null 2>&1 | grep -c . > /dev/null` and inspect manually that `main` calls the worker rather than printing a literal.
Record the observation in the report of this task (one sentence: "worker survives, no literal result in Core").

- [ ] **Step 4: Commit**

```bash
git add bench/ghc-parity/workload.hs
git commit -m 'bench: Haskell 2010 workload for GHC parity harness'
```

---

### Task 4: Driver — build, run, compare, time

**Files:**
- Create: `bench/ghc-parity/driver/src/main.rs`
- Modify: `bench/ghc-parity/run.sh`
- Test: `bash bench/ghc-parity/run.sh`

**Interfaces:**
- Consumes: `workload.rill` (Task 2), the compiled Haskell binary (Task 3).
- Produces: stdout report with header `metric  rill  ghc  rill/ghc`; exit 1 on parity mismatch.

- [ ] **Step 1: Implement the driver**

`main` accepts `<array_elems> <iters> <runs> <ghc_binary>`; then:
1. Read `workload.rill` (path relative to the driver manifest, or passed as a fifth argument).
2. `compile::<f32, 256>(&src, ForeignRegistry::new())`, then for each run: `world_mut().set_param("n", ParamValue::Float(n))`, `"k"`, `"d"`, `runner.run()`, capture `runner.stdout()` (the console is cumulative — take the **last** line, or clear by construction: use a fresh runner per run).
3. Parse rill's last printed line as `f64`.
4. Spawn the GHC binary with the same numeric arguments `runs` times; parse each stdout line as `f64`.
5. **Parity:** compare the rill and GHC values with relative tolerance `1e-9`; on mismatch print both and exit 1.
6. **Timing:** warm-up one run per side (discarded), then time `runs` runs per side with `std::time::Instant`; compute `median_ms / iters` → per-iteration microseconds; print absolute values and the ratio.

- [ ] **Step 2: Write `run.sh`**

```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
ARRAY_ELEMS=${1:-64}; ITERS=${2:-1000}; RUNS=${3:-5}
ghc -O2 -fforce-recomp -o workload workload.hs
cargo run --release --manifest-path driver/Cargo.toml -- "$ARRAY_ELEMS" "$ITERS" "$RUNS" "$PWD/workload" "$PWD/workload.rill"
```

- [ ] **Step 3: Run it end to end**

Run: `bash bench/ghc-parity/run.sh`
Expected: compiles both, prints `parity: PASS`, then the table with `iter time (µs)` for rill, ghc, and the ratio.

- [ ] **Step 4: Prove the parity check can fail**

Temporarily change the rill workload to add `1.0` to the result; run; expect exit 1 and a printed mismatch. Revert.

- [ ] **Step 5: Prove the comparison is parameter-sensitive**

Run `bash bench/ghc-parity/run.sh 32 100 3` and `bash bench/ghc-parity/run.sh 64 1000 3`; expect different timings but parity PASS both times.

- [ ] **Step 6: Commit**

```bash
git add bench/ghc-parity/driver bench/ghc-parity/run.sh
git commit -m 'bench: driver runs, compares and times both workloads'
```

---

### Task 5: Report, README completion, and baseline record

**Files:**
- Modify: `bench/ghc-parity/README.md`
- Modify: `bench/ghc-parity/driver/src/main.rs` (report formatting only)
- Test: `bash bench/ghc-parity/run.sh`

**Interfaces:**
- Consumes: Task 4's driver.
- Produces: the documented report shape and a recorded baseline.

- [ ] **Step 1: Finalise the report format**

```
workload: arrays n=<n>, lists m=64, nested map=yes, iters=<iters>
ghc: 9.10.3   runs=<runs> (median)
parity: PASS  (result = <value>)

metric            rill            ghc            rill/ghc
iter time (µs)    <x>             <y>            <x/y>x
```

- [ ] **Step 2: Record the baseline**

Run `bash bench/ghc-parity/run.sh` and append to `README.md` a "Baseline (2026-10-24)" block with the machine description (`16 cores`, GHC 9.10.3), the parameters used, and the table. Note: this baseline measures **pre-memory-layers** behaviour and is the reference for after/before comparisons.

- [ ] **Step 3: Document the workflow in README**

"After a memory-manager change, run `bash bench/ghc-parity/run.sh` and compare against the Baseline block; parity must stay PASS; change in `iter time (µs)` is the effect of the decision."

- [ ] **Step 4: Verify no forbidden API and no workspace pollution**

Run: `grep -rn 'test_eval' bench/ghc-parity/` → no matches.
Run: `cargo metadata --format-version 1 | grep -c ghc-parity` → `0`.
Run: `cargo test --workspace --no-run 2>&1 | tail -2` → unchanged workspace build.

- [ ] **Step 5: Commit**

```bash
git add bench/ghc-parity
git commit -m 'bench: document GHC parity report and record baseline'
```

---

### Task 6: Hand-off note in the memory-layers plan

**Files:**
- Modify: `docs/superpowers/plans/2026-10-24-rill-lang-memory-layers.md` (Tasks 0.1 and 3.2 only)
- Test: n/a (documentation change); `scripts/task-brief` still extracts every task heading

**Interfaces:**
- Consumes: Task 5's harness.
- Produces: the memory-layers plan points its baseline/after measurement at `bench/ghc-parity/run.sh` instead of `lang_bench`.

- [ ] **Step 1: Replace Task 0.1's body**

Rewrite Task 0.1 to: "run `bash bench/ghc-parity/run.sh` on the branch base and record the baseline block in the implementation report; do not add a workload to `lang_bench`". Keep the task heading.

- [ ] **Step 2: Replace Task 3.2's body**

Rewrite Task 3.2 to: "re-run `bash bench/ghc-parity/run.sh` and compare against the recorded baseline; parity must stay PASS; the `iter time (µs)` delta is the gate". Keep the task heading.

- [ ] **Step 3: Verify extraction still works**

Run: `bash <superpowers>/skills/subagent-driven-development/scripts/task-brief docs/superpowers/plans/2026-10-24-rill-lang-memory-layers.md 0.1`
Expected: writes a brief containing the new Task 0.1 text.

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/plans/2026-10-24-rill-lang-memory-layers.md
git commit -m 'docs: point memory-layers baseline at the GHC parity harness'
```
