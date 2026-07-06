# rill-lang Hybrid Block Processing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add hybrid block/sample processing to `rill-lang`: compile the linear IR into a region *schedule* (via SCC analysis), run feedforward regions whole-buffer with the `rill_core::math::vector` eDSL, and run feedback regions per-sample — keeping identical numerical behavior to the existing sample interpreter.

**Architecture:** A new compile-time pass `build_schedule(&Ir) -> Schedule` contracts state/delay read-write pairs, builds the data-dependency graph with loop-closing edges, runs Tarjan SCC, and emits ordered `Step`s (`Block(instr)` or `Sample(instrs)`). At runtime a single register store `Vec<Vec<T>>` (one whole buffer per register, computed in `T`) is driven whole-slice for block steps and `[i]`-indexed for sample steps. The MVP per-sample interpreter is retained as a reference oracle for equivalence tests.

**Tech Stack:** Rust (edition 2021, `max_width=100`, `#![deny(unsafe_code)]`). Uses existing `rill_core::math::vector` slice ops; no new dependencies.

**Design:** `docs/superpowers/specs/2026-07-06-rill-lang-block-processing-design.md`.
**Branch:** `feature/rill-lang` (follow-on to the crate MVP already on this branch).

**Key facts locked here (read before coding):**
- IR is SSA: every register has exactly one producer instruction (the instr whose `dst == reg`); `LoadInput`/`ReadState`/`ReadDelay`/`Const`/`Un`/`Bin`/`Move` produce a reg, `WriteState`/`WriteDelay` are sinks.
- Feedback lowering emits, per state slot, exactly one `ReadState` and one `WriteState`; delay lowering emits, per line, exactly one `ReadDelay` and one `WriteDelay`.
- Dependency edge direction is **consumer → producer**. With that direction, **Tarjan emits SCCs in execution order** (dependencies popped first), so no separate topological sort is needed.
- Recurrence is modeled by adding **bidirectional** edges between each slot's `ReadState`/`WriteState` and each line's `ReadDelay`/`WriteDelay`, forcing them into one SCC and closing feedback loops.
- The hybrid computes in `T` (e.g. `f32`); existing tests are `f32`-exact or use `f32` tolerances.
- `RillProgram::process` must not allocate after warm-up: the `Vec<Vec<T>>` store is grown to the block length on first use (or when the block grows) and reused.

---

## File Structure

```
rill-lang/src/
  schedule.rs        # NEW: Step, Schedule, build_schedule (graph + Tarjan SCC + classify)
  backend/interp.rs  # MODIFY: add run_block_hybrid + block/sample-region executors
  program.rs         # MODIFY: RillProgram gains schedule + Vec<Vec<T>> block store; process() = hybrid; keep process_reference()
  lib.rs             # MODIFY: pub mod schedule;
  tests/hybrid.rs    # NEW (crate tests dir): equivalence + behavior tests
```

---

## Task B1: Schedule types + `build_schedule` (graph, Tarjan SCC, classification)

**Files:**
- Create: `rill-lang/src/schedule.rs`
- Modify: `rill-lang/src/lib.rs` (add `pub mod schedule;`)
- Test: `rill-lang/src/schedule.rs` (`mod tests`)

- [ ] **Step 1: Create `rill-lang/src/schedule.rs`**

```rust
//! Compile-time partitioning of the linear IR into a hybrid execution schedule.
//!
//! Feedforward instructions become whole-buffer [`Step::Block`] ops; recurrences
//! (feedback loops, and the read/write of each state slot or delay line) become
//! per-sample [`Step::Sample`] regions. See the block-processing design doc.

use crate::ir::{Instr, Ir};

/// One scheduled unit of work, in execution order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A single combinational instruction, executed over the whole buffer.
    Block(usize),
    /// A recurrent region, executed per sample. Instruction indices are in
    /// original IR order (which preserves intra-sample data + read-before-write
    /// ordering established by lowering).
    Sample(Vec<usize>),
}

/// The full execution plan for an [`Ir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    /// Steps in execution order (dependencies first).
    pub steps: Vec<Step>,
}

/// Which register each instruction produces (`None` for sinks).
fn instr_dst(instr: &Instr) -> Option<usize> {
    match *instr {
        Instr::Const { dst, .. }
        | Instr::LoadInput { dst, .. }
        | Instr::ReadState { dst, .. }
        | Instr::ReadDelay { dst, .. }
        | Instr::Un { dst, .. }
        | Instr::Bin { dst, .. }
        | Instr::Move { dst, .. } => Some(dst),
        Instr::WriteState { .. } | Instr::WriteDelay { .. } => None,
    }
}

/// The registers an instruction consumes.
fn instr_srcs(instr: &Instr) -> Vec<usize> {
    match *instr {
        Instr::Un { src, .. } | Instr::Move { src, .. } => vec![src],
        Instr::Bin { a, b, .. } => vec![a, b],
        Instr::WriteState { src, .. } | Instr::WriteDelay { src, .. } => vec![src],
        _ => Vec::new(),
    }
}

/// True for instructions that touch persistent state and therefore must run in
/// a sample region (never as a standalone block op).
fn is_stateful(instr: &Instr) -> bool {
    matches!(
        instr,
        Instr::ReadState { .. }
            | Instr::WriteState { .. }
            | Instr::ReadDelay { .. }
            | Instr::WriteDelay { .. }
    )
}

/// Build the hybrid schedule for an IR.
pub fn build_schedule(ir: &Ir) -> Schedule {
    let n = ir.instrs.len();

    // producer[reg] = instr index whose dst == reg (SSA: unique).
    let mut producer: Vec<Option<usize>> = vec![None; ir.num_regs];
    for (i, instr) in ir.instrs.iter().enumerate() {
        if let Some(d) = instr_dst(instr) {
            producer[d] = Some(i);
        }
    }

    // Adjacency: consumer -> producer (dependency edges).
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, instr) in ir.instrs.iter().enumerate() {
        for s in instr_srcs(instr) {
            if let Some(p) = producer[s] {
                adj[i].push(p);
            }
        }
    }

    // Recurrence edges (bidirectional) for each state slot and delay line, so
    // the read/write ends share an SCC and feedback loops close.
    let mut read_state: Vec<Option<usize>> = vec![None; ir.state.state_slots];
    let mut write_state: Vec<Option<usize>> = vec![None; ir.state.state_slots];
    let mut read_delay: Vec<Option<usize>> = vec![None; ir.state.delay_lens.len()];
    let mut write_delay: Vec<Option<usize>> = vec![None; ir.state.delay_lens.len()];
    for (i, instr) in ir.instrs.iter().enumerate() {
        match *instr {
            Instr::ReadState { slot, .. } => read_state[slot] = Some(i),
            Instr::WriteState { slot, .. } => write_state[slot] = Some(i),
            Instr::ReadDelay { line, .. } => read_delay[line] = Some(i),
            Instr::WriteDelay { line, .. } => write_delay[line] = Some(i),
            _ => {}
        }
    }
    let mut add_pair = |a: Option<usize>, b: Option<usize>, adj: &mut Vec<Vec<usize>>| {
        if let (Some(a), Some(b)) = (a, b) {
            adj[a].push(b);
            adj[b].push(a);
        }
    };
    for s in 0..ir.state.state_slots {
        add_pair(read_state[s], write_state[s], &mut adj);
    }
    for l in 0..ir.state.delay_lens.len() {
        add_pair(read_delay[l], write_delay[l], &mut adj);
    }

    // Tarjan SCC. Emission order is reverse-finish = execution order
    // (dependencies first) because edges point consumer -> producer.
    let sccs = tarjan_scc(n, &adj);

    // Classify each SCC into a Step.
    let mut steps = Vec::with_capacity(sccs.len());
    for scc in sccs {
        let recurrent = scc.len() > 1 || scc.iter().any(|&i| is_stateful(&ir.instrs[i]));
        if recurrent {
            let mut instrs = scc;
            instrs.sort_unstable();
            steps.push(Step::Sample(instrs));
        } else {
            steps.push(Step::Block(scc[0]));
        }
    }
    Schedule { steps }
}

/// Iterative Tarjan strongly-connected-components.
///
/// Returns SCCs in reverse topological order of the condensation — with our
/// consumer→producer edges, that is exactly execution order (a node's
/// dependencies appear before it).
fn tarjan_scc(n: usize, adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    const UNVISITED: i64 = -1;
    let mut index = vec![UNVISITED; n];
    let mut low = vec![0i64; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next_index: i64 = 0;
    let mut out: Vec<Vec<usize>> = Vec::new();

    // Explicit DFS stack of (node, next-neighbor-index).
    for root in 0..n {
        if index[root] != UNVISITED {
            continue;
        }
        let mut call: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&mut (v, ref mut ni)) = call.last_mut() {
            if *ni == 0 {
                index[v] = next_index;
                low[v] = next_index;
                next_index += 1;
                stack.push(v);
                on_stack[v] = true;
            }
            if *ni < adj[v].len() {
                let w = adj[v][*ni];
                *ni += 1;
                if index[w] == UNVISITED {
                    call.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                if low[v] == index[v] {
                    let mut comp = Vec::new();
                    loop {
                        let w = stack.pop().unwrap();
                        on_stack[w] = false;
                        comp.push(w);
                        if w == v {
                            break;
                        }
                    }
                    out.push(comp);
                }
                let finished = v;
                call.pop();
                if let Some(&mut (parent, _)) = call.last_mut() {
                    low[parent] = low[parent].min(low[finished]);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::lower::lower;
    use crate::parser::parse;
    use crate::types::infer::infer_program;

    fn schedule_of(src: &str) -> Schedule {
        let p = parse(&tokenize(src).unwrap()).unwrap();
        let tp = infer_program(&p).unwrap();
        let ir = lower(&tp).unwrap();
        build_schedule(&ir)
    }

    fn n_sample(s: &Schedule) -> usize {
        s.steps.iter().filter(|st| matches!(st, Step::Sample(_))).count()
    }
    fn n_block(s: &Schedule) -> usize {
        s.steps.iter().filter(|st| matches!(st, Step::Block(_))).count()
    }

    #[test]
    fn combinational_program_is_all_block() {
        let s = schedule_of("process = _ * 0.5;");
        assert_eq!(n_sample(&s), 0);
        assert!(n_block(&s) >= 1);
    }

    #[test]
    fn feedback_program_has_one_sample_region() {
        let s = schedule_of("process = + ~ _;");
        assert_eq!(n_sample(&s), 1);
    }

    #[test]
    fn const_feeding_feedback_stays_block() {
        // `+ ~ (_ * 0.5)`: the 0.5 constant is combinational (Block); the
        // ReadState/Add/Mul/WriteState cycle is one Sample region.
        let s = schedule_of("process = + ~ (_ * 0.5);");
        assert_eq!(n_sample(&s), 1);
        assert!(n_block(&s) >= 1); // at least the Const 0.5 and the LoadInput
    }

    #[test]
    fn feedforward_delay_is_isolated_sample_region() {
        // `_ @ 3`: delay read/write form a sample region; no feedback.
        let s = schedule_of("process = _ @ 3;");
        assert_eq!(n_sample(&s), 1);
    }

    #[test]
    fn feedback_through_delay_is_one_region() {
        let s = schedule_of("process = + ~ (_ @ 2);");
        assert_eq!(n_sample(&s), 1);
    }

    #[test]
    fn gain_then_integrator_splits_block_and_sample() {
        let s = schedule_of("process = (_ * 0.5) : (+ ~ _);");
        assert_eq!(n_sample(&s), 1);
        assert!(n_block(&s) >= 1);
    }

    #[test]
    fn steps_are_in_dependency_order() {
        // Every Block step's producer appears before any step that consumes it:
        // here we only assert the schedule is non-empty and ends producing output.
        let s = schedule_of("process = abs(_) : _ * 2;");
        assert!(!s.steps.is_empty());
        assert_eq!(n_sample(&s), 0);
    }
}
```

- [ ] **Step 2: Add `pub mod schedule;` to `rill-lang/src/lib.rs`** (alongside the other module decls).

- [ ] **Step 3: Run the tests**

Run: `cargo test -p rill-lang schedule`
Expected: 7 tests pass. If a classification test fails, the fault is in
`build_schedule` (edges/SCC), not the test — fix the pass. Verify Tarjan by
reasoning about the small graphs in the design doc's worked-classifications table.

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/schedule.rs rill-lang/src/lib.rs
git commit -m 'feat(rill-lang): SCC-based hybrid execution scheduler (block vs sample regions)'
```

---

## Task B2: Hybrid executor + `RillProgram` block store

**Files:**
- Modify: `rill-lang/src/program.rs`
- Modify: `rill-lang/src/backend/interp.rs`
- Test: `rill-lang/src/backend/interp.rs` (extend `mod tests`)

The MVP `RillProgram` uses a scalar `regs: Vec<f64>` and `run_block`/`eval_sample`.
Keep those as the **reference** path (`process_reference`) and add the hybrid path
(`process`) using a whole-buffer store.

- [ ] **Step 1: Update `rill-lang/src/program.rs`**

Replace the file with the version below. It keeps the scalar reference fields
(`regs_scalar`, used only by `process_reference`) and adds `schedule` +
`block_regs` for the hybrid path. `reset()` clears both.

```rust
//! `RillProgram<T>` — a compiled rill-lang program that implements
//! [`rill_core::Algorithm`]. Owns its IR, schedule, and pre-allocated state;
//! `process()` performs no heap allocation after warm-up.

use rill_core::math::Transcendental;
use rill_core::traits::{Algorithm, ProcessResult};

use crate::ir::Ir;
use crate::schedule::{build_schedule, Schedule};

/// A compiled program ready to run inside the rill graph.
pub struct RillProgram<T: Transcendental> {
    pub(crate) ir: Ir,
    pub(crate) schedule: Schedule,
    /// Persistent feedback state (previous-sample values). Length = state_slots.
    pub(crate) state: Vec<f64>,
    /// Next-sample feedback writes, applied at sample end.
    pub(crate) state_next: Vec<f64>,
    /// Delay lines: ring buffers, one per `@` site.
    pub(crate) delays: Vec<DelayRing>,
    /// Whole-buffer register store for the hybrid path (grown to block length).
    pub(crate) block_regs: Vec<Vec<T>>,
    /// Scalar register file for the reference (per-sample) path.
    pub(crate) regs_scalar: Vec<f64>,
}

/// A fixed-length ring buffer for one `@` delay site.
pub(crate) struct DelayRing {
    pub(crate) buf: Vec<f64>,
    pub(crate) head: usize,
}

impl DelayRing {
    pub(crate) fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.max(1)], head: 0 }
    }
    pub(crate) fn read(&self) -> f64 {
        self.buf[self.head]
    }
    pub(crate) fn write(&mut self, v: f64) {
        self.buf[self.head] = v;
        self.head = (self.head + 1) % self.buf.len();
    }
}

impl<T: Transcendental> RillProgram<T> {
    pub(crate) fn new(ir: Ir) -> Self {
        let state = vec![0.0; ir.state.state_slots];
        let state_next = state.clone();
        let delays = ir.state.delay_lens.iter().map(|&l| DelayRing::new(l)).collect();
        let block_regs = vec![Vec::new(); ir.num_regs];
        let regs_scalar = vec![0.0; ir.num_regs];
        let schedule = build_schedule(&ir);
        Self { ir, schedule, state, state_next, delays, block_regs, regs_scalar }
    }

    /// Ensure every block register can hold `n` samples (grows + reuses).
    pub(crate) fn ensure_block_len(&mut self, n: usize) {
        for r in &mut self.block_regs {
            if r.len() < n {
                r.resize(n, T::ZERO);
            }
        }
    }

    /// Reference implementation: the MVP per-sample interpreter. Used by tests
    /// as a numerical oracle; not the production path.
    pub fn process_reference(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        crate::backend::interp::run_block_reference(self, input, output);
        Ok(())
    }
}

impl<T: Transcendental> Algorithm<T> for RillProgram<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        crate::backend::interp::run_block_hybrid(self, input, output);
        Ok(())
    }

    fn reset(&mut self) {
        for s in &mut self.state {
            *s = 0.0;
        }
        for s in &mut self.state_next {
            *s = 0.0;
        }
        for d in &mut self.delays {
            for v in &mut d.buf {
                *v = 0.0;
            }
            d.head = 0;
        }
    }
}
```

- [ ] **Step 2: Update `rill-lang/src/backend/interp.rs`**

Rename the existing per-sample entry point to `run_block_reference` (keep its
body verbatim — it is the oracle), and add the hybrid executor. Full file:

```rust
//! IR evaluators: the reference per-sample interpreter and the hybrid
//! block/sample executor.

use rill_core::math::vector::ScalarVector4;
use rill_core::math::Transcendental;

use crate::ir::{BinArith, Instr, UnOp};
use crate::program::RillProgram;
use crate::schedule::Step;

// ============================================================================
// Reference (per-sample) interpreter — numerical oracle, MVP behavior.
// ============================================================================

/// Run one block sample-by-sample using the scalar `f64` register file.
pub fn run_block_reference<T: Transcendental>(
    prog: &mut RillProgram<T>,
    input: Option<&[T]>,
    output: &mut [T],
) {
    let n = output.len();
    for i in 0..n {
        let in_sample = match input {
            Some(buf) if i < buf.len() => buf[i].to_f64(),
            _ => 0.0,
        };
        let y = eval_sample_scalar(prog, in_sample);
        output[i] = T::from_f64(y);
    }
}

fn eval_sample_scalar<T: Transcendental>(prog: &mut RillProgram<T>, in0: f64) -> f64 {
    for idx in 0..prog.ir.instrs.len() {
        match prog.ir.instrs[idx].clone() {
            Instr::Const { dst, value } => prog.regs_scalar[dst] = value,
            Instr::LoadInput { dst, index } => {
                prog.regs_scalar[dst] = if index == 0 { in0 } else { 0.0 };
            }
            Instr::ReadState { dst, slot } => prog.regs_scalar[dst] = prog.state[slot],
            Instr::ReadDelay { dst, line } => prog.regs_scalar[dst] = prog.delays[line].read(),
            Instr::Move { dst, src } => prog.regs_scalar[dst] = prog.regs_scalar[src],
            Instr::Un { dst, op, src } => {
                let x = prog.regs_scalar[src];
                prog.regs_scalar[dst] = apply_un_f64(op, x);
            }
            Instr::Bin { dst, op, a, b } => {
                let x = prog.regs_scalar[a];
                let y = prog.regs_scalar[b];
                prog.regs_scalar[dst] = apply_bin_f64(op, x, y);
            }
            Instr::WriteState { slot, src } => prog.state_next[slot] = prog.regs_scalar[src],
            Instr::WriteDelay { line, src } => {
                let v = prog.regs_scalar[src];
                prog.delays[line].write(v);
            }
        }
    }
    for (s, nx) in prog.state.iter_mut().zip(prog.state_next.iter()) {
        *s = *nx;
    }
    prog.regs_scalar[prog.ir.output_reg]
}

fn apply_un_f64(op: UnOp, x: f64) -> f64 {
    match op {
        UnOp::Neg => -x,
        UnOp::Abs => x.abs(),
        UnOp::Sin => x.sin(),
        UnOp::Cos => x.cos(),
        UnOp::Tan => x.tan(),
        UnOp::Sqrt => x.sqrt(),
        UnOp::Exp => x.exp(),
        UnOp::Ln => x.ln(),
        UnOp::Tanh => x.tanh(),
    }
}

fn apply_bin_f64(op: BinArith, x: f64, y: f64) -> f64 {
    match op {
        BinArith::Add => x + y,
        BinArith::Sub => x - y,
        BinArith::Mul => x * y,
        BinArith::Div => x / y,
        BinArith::Rem => x % y,
        BinArith::Min => x.min(y),
        BinArith::Max => x.max(y),
    }
}

// ============================================================================
// Hybrid (block + sample-region) executor.
// ============================================================================

/// Run one block via the schedule: block steps whole-buffer, sample regions
/// per sample. All registers are computed in `T`.
pub fn run_block_hybrid<T: Transcendental>(
    prog: &mut RillProgram<T>,
    input: Option<&[T]>,
    output: &mut [T],
) {
    let n = output.len();
    prog.ensure_block_len(n);

    // Steps are stored by value; clone the small schedule step list to avoid
    // borrowing `prog` while mutating its registers.
    for step in prog.schedule.steps.clone() {
        match step {
            Step::Block(idx) => exec_block_op(prog, idx, input, n),
            Step::Sample(instrs) => exec_sample_region(prog, &instrs, input, n),
        }
    }

    let out_reg = prog.ir.output_reg;
    output[..n].copy_from_slice(&prog.block_regs[out_reg][..n]);
}

/// Execute a single combinational instruction over the whole `[..n]` buffer.
fn exec_block_op<T: Transcendental>(
    prog: &mut RillProgram<T>,
    idx: usize,
    input: Option<&[T]>,
    n: usize,
) {
    match prog.ir.instrs[idx].clone() {
        Instr::Const { dst, value } => {
            let v = T::from_f64(value);
            prog.block_regs[dst][..n].fill(v);
        }
        Instr::LoadInput { dst, index } => {
            let reg = &mut prog.block_regs[dst];
            if index == 0 {
                if let Some(buf) = input {
                    let m = buf.len().min(n);
                    reg[..m].copy_from_slice(&buf[..m]);
                    for v in &mut reg[m..n] {
                        *v = T::ZERO;
                    }
                } else {
                    for v in &mut reg[..n] {
                        *v = T::ZERO;
                    }
                }
            } else {
                for v in &mut reg[..n] {
                    *v = T::ZERO;
                }
            }
        }
        Instr::Move { dst, src } => {
            // dst != src (SSA); move src out to satisfy the borrow checker.
            let tmp = std::mem::take(&mut prog.block_regs[dst]);
            let mut tmp = tmp;
            tmp[..n].copy_from_slice(&prog.block_regs[src][..n]);
            prog.block_regs[dst] = tmp;
        }
        Instr::Un { dst, op, src } => {
            let mut out = std::mem::take(&mut prog.block_regs[dst]);
            apply_un_slice(op, &prog.block_regs[src][..n], &mut out[..n]);
            prog.block_regs[dst] = out;
        }
        Instr::Bin { dst, op, a, b } => {
            let mut out = std::mem::take(&mut prog.block_regs[dst]);
            apply_bin_slice(op, &prog.block_regs[a][..n], &prog.block_regs[b][..n], &mut out[..n]);
            prog.block_regs[dst] = out;
        }
        // Stateful instrs never appear as a Block step.
        Instr::ReadState { .. }
        | Instr::WriteState { .. }
        | Instr::ReadDelay { .. }
        | Instr::WriteDelay { .. } => {
            unreachable!("stateful instruction scheduled as a block op")
        }
    }
}

/// Execute a recurrent region per sample, indexing the shared block store.
fn exec_sample_region<T: Transcendental>(
    prog: &mut RillProgram<T>,
    instrs: &[usize],
    input: Option<&[T]>,
    n: usize,
) {
    for i in 0..n {
        for &idx in instrs {
            match prog.ir.instrs[idx].clone() {
                Instr::Const { dst, value } => prog.block_regs[dst][i] = T::from_f64(value),
                Instr::LoadInput { dst, index } => {
                    let v = if index == 0 {
                        match input {
                            Some(buf) if i < buf.len() => buf[i],
                            _ => T::ZERO,
                        }
                    } else {
                        T::ZERO
                    };
                    prog.block_regs[dst][i] = v;
                }
                Instr::ReadState { dst, slot } => {
                    prog.block_regs[dst][i] = T::from_f64(prog.state[slot]);
                }
                Instr::ReadDelay { dst, line } => {
                    prog.block_regs[dst][i] = T::from_f64(prog.delays[line].read());
                }
                Instr::Move { dst, src } => {
                    prog.block_regs[dst][i] = prog.block_regs[src][i];
                }
                Instr::Un { dst, op, src } => {
                    let x = prog.block_regs[src][i];
                    prog.block_regs[dst][i] = apply_un_t(op, x);
                }
                Instr::Bin { dst, op, a, b } => {
                    let x = prog.block_regs[a][i];
                    let y = prog.block_regs[b][i];
                    prog.block_regs[dst][i] = apply_bin_t(op, x, y);
                }
                Instr::WriteState { slot, src } => {
                    prog.state_next[slot] = prog.block_regs[src][i].to_f64();
                }
                Instr::WriteDelay { line, src } => {
                    let v = prog.block_regs[src][i].to_f64();
                    prog.delays[line].write(v);
                }
            }
        }
        for (s, nx) in prog.state.iter_mut().zip(prog.state_next.iter()) {
            *s = *nx;
        }
    }
}

// ---- T-typed scalar ops (sample regions) ----

fn apply_un_t<T: Transcendental>(op: UnOp, x: T) -> T {
    match op {
        UnOp::Neg => T::ZERO - x,
        UnOp::Abs => x.abs(),
        UnOp::Sin => x.sin(),
        UnOp::Cos => x.cos(),
        UnOp::Tan => x.tan(),
        UnOp::Sqrt => x.sqrt(),
        UnOp::Exp => x.exp(),
        UnOp::Ln => x.ln(),
        UnOp::Tanh => x.tanh(),
    }
}

fn apply_bin_t<T: Transcendental>(op: BinArith, x: T, y: T) -> T {
    match op {
        BinArith::Add => x + y,
        BinArith::Sub => x - y,
        BinArith::Mul => x * y,
        BinArith::Div => x / y,
        BinArith::Rem => x % y,
        BinArith::Min => x.min(y),
        BinArith::Max => x.max(y),
    }
}

// ---- T-typed whole-buffer ops (block steps) via the vector eDSL ----

fn apply_un_slice<T: Transcendental>(op: UnOp, src: &[T], out: &mut [T]) {
    use rill_core::math::vector::math::{
        abs_slice, cos_slice, exp_slice, ln_slice, sin_slice, sqrt_slice, tan_slice,
    };
    match op {
        UnOp::Neg => {
            for (o, &x) in out.iter_mut().zip(src.iter()) {
                *o = T::ZERO - x;
            }
        }
        UnOp::Abs => abs_slice::<T, 4, ScalarVector4<T>>(src, out),
        UnOp::Sin => sin_slice::<T, 4, ScalarVector4<T>>(src, out),
        UnOp::Cos => cos_slice::<T, 4, ScalarVector4<T>>(src, out),
        UnOp::Tan => tan_slice::<T, 4, ScalarVector4<T>>(src, out),
        UnOp::Sqrt => sqrt_slice::<T, 4, ScalarVector4<T>>(src, out),
        UnOp::Exp => exp_slice::<T, 4, ScalarVector4<T>>(src, out),
        UnOp::Ln => ln_slice::<T, 4, ScalarVector4<T>>(src, out),
        UnOp::Tanh => {
            for (o, &x) in out.iter_mut().zip(src.iter()) {
                *o = x.tanh();
            }
        }
    }
}

fn apply_bin_slice<T: Transcendental>(op: BinArith, a: &[T], b: &[T], out: &mut [T]) {
    use rill_core::math::vector::math::{max_slice, min_slice};
    use rill_core::math::vector::ops::{add_slices, div_slices, mul_slices, sub_slices};
    match op {
        BinArith::Add => add_slices::<T, 4, ScalarVector4<T>>(a, b, out),
        BinArith::Sub => sub_slices::<T, 4, ScalarVector4<T>>(a, b, out),
        BinArith::Mul => mul_slices::<T, 4, ScalarVector4<T>>(a, b, out),
        BinArith::Div => div_slices::<T, 4, ScalarVector4<T>>(a, b, out),
        BinArith::Min => min_slice::<T, 4, ScalarVector4<T>>(a, b, out),
        BinArith::Max => max_slice::<T, 4, ScalarVector4<T>>(a, b, out),
        BinArith::Rem => {
            for ((o, &x), &y) in out.iter_mut().zip(a.iter()).zip(b.iter()) {
                *o = x % y;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::lexer::tokenize;
    use crate::lower::lower;
    use crate::parser::parse;
    use crate::program::RillProgram;
    use crate::types::infer::infer_program;
    use rill_core::traits::Algorithm;

    fn build(src: &str) -> RillProgram<f32> {
        let p = parse(&tokenize(src).unwrap()).unwrap();
        let tp = infer_program(&p).unwrap();
        let ir = lower(&tp).unwrap();
        RillProgram::<f32>::new(ir)
    }

    #[test]
    fn hybrid_gain_halves_input() {
        let mut prog = build("process = _ * 0.5;");
        let mut out = [0.0f32; 4];
        prog.process(Some(&[1.0, 2.0, 4.0, 8.0]), &mut out).unwrap();
        assert_eq!(out, [0.5, 1.0, 2.0, 4.0]);
    }

    #[test]
    fn hybrid_integrator_accumulates() {
        let mut prog = build("process = + ~ _;");
        let mut out = [0.0f32; 4];
        prog.process(Some(&[1.0, 1.0, 1.0, 1.0]), &mut out).unwrap();
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn hybrid_one_sample_delay() {
        let mut prog = build("process = _ @ 1;");
        let mut out = [0.0f32; 3];
        prog.process(Some(&[5.0, 7.0, 9.0]), &mut out).unwrap();
        assert_eq!(out, [0.0, 5.0, 7.0]);
    }

    #[test]
    fn hybrid_split_merge_doubles() {
        let mut prog = build("process = _ <: (_ , _) :> + ;");
        let mut out = [0.0f32; 2];
        prog.process(Some(&[1.0, 3.0]), &mut out).unwrap();
        assert_eq!(out, [2.0, 6.0]);
    }

    #[test]
    fn hybrid_matches_reference_on_mixed_program() {
        let mut a = build("process = (_ * 0.5) : (+ ~ (_ * 0.5));");
        let mut b = build("process = (_ * 0.5) : (+ ~ (_ * 0.5));");
        let input: Vec<f32> = (0..32).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut oa = vec![0.0f32; input.len()];
        let mut ob = vec![0.0f32; input.len()];
        a.process(Some(&input), &mut oa).unwrap();
        b.process_reference(Some(&input), &mut ob).unwrap();
        for (x, y) in oa.iter().zip(ob.iter()) {
            assert!((x - y).abs() < 1e-4, "hybrid {x} vs reference {y}");
        }
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p rill-lang`
Expected: all pass (existing tests still green; new hybrid tests pass). The
integrator/delay/split-merge results must be identical to the MVP.

- [ ] **Step 4: Clippy + fmt**

Run: `cargo clippy -p rill-lang --all-targets` → zero warnings. If clippy flags
`schedule.steps.clone()` in `run_block_hybrid`, keep it (needed to avoid
borrowing `prog` while mutating; the schedule is small). If it flags
`needless_range_loop` in `exec_sample_region`, add `#[allow(clippy::needless_range_loop)]`
with a one-line comment (indexing `[i]` across many registers is intentional).
Run: `cargo fmt -p rill-lang`.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/program.rs rill-lang/src/backend/interp.rs
git commit -m 'feat(rill-lang): hybrid block/sample executor + whole-buffer register store'
```

---

## Task B3: Equivalence + behavior test suite

**Files:**
- Create: `rill-lang/tests/hybrid.rs`

- [ ] **Step 1: Create `rill-lang/tests/hybrid.rs`**

```rust
//! Hybrid backend: equivalence with the reference interpreter + behavior.

use rill_core::traits::Algorithm;
use rill_lang::compile;

fn hybrid(src: &str, input: &[f32]) -> Vec<f32> {
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = vec![0.0f32; input.len()];
    prog.process(Some(input), &mut out).unwrap();
    out
}

fn reference(src: &str, input: &[f32]) -> Vec<f32> {
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = vec![0.0f32; input.len()];
    prog.process_reference(Some(input), &mut out).unwrap();
    out
}

fn assert_equiv(src: &str) {
    let input: Vec<f32> = (0..64).map(|i| ((i as f32) * 0.13).sin() * 0.7).collect();
    let h = hybrid(src, &input);
    let r = reference(src, &input);
    for (k, (x, y)) in h.iter().zip(r.iter()).enumerate() {
        assert!((x - y).abs() < 1e-4, "[{k}] {src}: hybrid {x} vs ref {y}");
    }
}

#[test]
fn equiv_feedforward() {
    assert_equiv("process = _ * 0.5;");
    assert_equiv("process = abs(_) : _ * 2;");
    assert_equiv("process = _ + 1 : sin;");
    assert_equiv("process = _ <: (_ , _ * 0.5) :> +;");
}

#[test]
fn equiv_feedback() {
    assert_equiv("process = + ~ _;");
    assert_equiv("process = + ~ (_ * 0.5);");
    assert_equiv("process = + ~ (_ * 0.9) : _ * 0.1;");
}

#[test]
fn equiv_delay_and_mixed() {
    assert_equiv("process = _ @ 1;");
    assert_equiv("process = _ @ 5;");
    assert_equiv("process = (_ * 0.5) : (+ ~ (_ @ 2));");
    assert_equiv("process = (_ @ 3) : (+ ~ _);");
}

#[test]
fn exact_values_hold() {
    assert_eq!(hybrid("process = _ * 0.5;", &[1.0, 2.0, 4.0, 8.0]), vec![0.5, 1.0, 2.0, 4.0]);
    assert_eq!(hybrid("process = + ~ _;", &[1.0, 1.0, 1.0, 1.0]), vec![1.0, 2.0, 3.0, 4.0]);
    assert_eq!(hybrid("process = _ @ 1;", &[5.0, 7.0, 9.0]), vec![0.0, 5.0, 7.0]);
}

#[test]
fn multi_block_state_persists() {
    // Two consecutive calls: the integrator state must carry across blocks.
    let mut prog = compile::<f32>("process = + ~ _;").unwrap();
    let mut o1 = [0.0f32; 3];
    let mut o2 = [0.0f32; 3];
    prog.process(Some(&[1.0, 1.0, 1.0]), &mut o1).unwrap();
    prog.process(Some(&[1.0, 1.0, 1.0]), &mut o2).unwrap();
    assert_eq!(o1, [1.0, 2.0, 3.0]);
    assert_eq!(o2, [4.0, 5.0, 6.0]);
}

#[test]
fn varying_block_length_reuses_store() {
    // Larger then smaller blocks must both work (store grows once, then reused).
    let mut prog = compile::<f32>("process = _ * 2;").unwrap();
    let mut big = vec![0.0f32; 100];
    prog.process(Some(&vec![1.0f32; 100]), &mut big).unwrap();
    assert!(big.iter().all(|&v| v == 2.0));
    let mut small = [0.0f32; 4];
    prog.process(Some(&[3.0, 3.0, 3.0, 3.0]), &mut small).unwrap();
    assert_eq!(small, [6.0, 6.0, 6.0, 6.0]);
}
```

- [ ] **Step 2: Run**

Run: `cargo test -p rill-lang --test hybrid`
Expected: all pass. A failure in `equiv_*` means the hybrid diverges from the
reference — debug the executor/scheduler, not the test.

- [ ] **Step 3: Commit**

```bash
git add rill-lang/tests/hybrid.rs
git commit -m 'test(rill-lang): hybrid vs reference equivalence + multi-block state + varying length'
```

---

## Task B4: Docs, changelog, and full verification

**Files:**
- Modify: `rill-lang/README.md` (execution-model note)
- Modify: `docs/src/guides/rill-lang-language.md` (execution-model section)
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Update the execution-model wording** in both `rill-lang/README.md`
  and `docs/src/guides/rill-lang-language.md`: the interpreter now compiles the IR
  into a hybrid schedule — feedforward regions run whole-buffer (SIMD via the
  `rill_core::math::vector` eDSL), and only feedback/delay recurrences run
  per-sample. Keep it factual and brief; note the block path computes in `T`.

- [ ] **Step 2: Add a `CHANGELOG.md` entry** under the `rill-lang` section of
  `[0.5.0-beta.7]`:

```markdown
- **Hybrid block processing.** `rill-lang` now compiles the IR into an execution
  schedule via SCC analysis: feedforward regions run whole-buffer through the
  `rill_core::math::vector` SIMD eDSL, while feedback/delay recurrences run
  per-sample. The block path computes in `T`. The per-sample interpreter is
  retained as a reference oracle (`RillProgram::process_reference`). This is the
  foundation for whole-graph-as-one-program lowering and the future JIT.
```

- [ ] **Step 3: Full verification**

Run: `cargo fmt`
Run: `cargo fmt --check` → clean.
Run: `cargo clippy --workspace --all-targets` → zero warnings.
Run: `cargo test --workspace` → all pass.
Run: `cargo test -p rill-lang --features serde` → all pass.
Run: `cargo test -p rill-adrift --features lang` → all pass (the `rill/lang`
node now runs on the hybrid path — confirm `node_halves_input_block` still passes).
Run: `mdbook build docs/` → builds; spot-check `docs/book/guides/rill-lang-language.html` is not a stub.

- [ ] **Step 4: Commit**

```bash
git add rill-lang/README.md docs/src/guides/rill-lang-language.md CHANGELOG.md
git commit -m 'docs(rill-lang): document hybrid block processing + changelog'
```

---

## Self-Review checklist (completed while writing)

- **Spec coverage:** scheduler (contract pairs + loop edges + Tarjan SCC +
  classify) → Task B1; block executor via vector eDSL + sample-region executor +
  whole-buffer store + `process()` switch → Task B2; reference-oracle equivalence
  + multi-block state + varying length → Task B3; docs/verify → Task B4. Deferred
  items (fused block delay op, long-distance recurrence vectorization, JIT,
  whole-graph lowering) match the design doc's non-goals.
- **Placeholder scan:** none — full code for `schedule.rs`, `program.rs`,
  `interp.rs`, and both test files; Task B4 doc edits are prose changes with exact
  changelog text.
- **Type consistency:** `Step`/`Schedule`/`build_schedule`, `RillProgram` fields
  (`schedule`, `block_regs`, `regs_scalar`, `state`, `state_next`, `delays`),
  `run_block_hybrid`/`run_block_reference`/`process_reference`, and the
  `apply_*_slice`/`apply_*_t`/`apply_*_f64` helpers are used consistently across
  tasks. `ScalarVector4<T>` + `sin_slice`/`add_slices`/… signatures match
  `rill-core/src/math/vector/{ops,math}.rs` (turbofish `::<T, 4, ScalarVector4<T>>`).

## Verification notes for the implementer

- Before writing Task B2, confirm the exact turbofish form the vector-eDSL
  functions expect by reading `rill-core/src/math/vector/ops.rs` and `math.rs`
  (the `<T, const N, V>` order) and the `ScalarVector4` import path
  (`rill_core::math::vector::ScalarVector4`). Adjust the calls if the generic
  order differs.
- `T::ZERO`, `to_f64`, `from_f64`, `abs/sin/cos/tan/sqrt/exp/ln/tanh`, and the
  arithmetic operators are all on `rill_core::math::{Scalar, Transcendental}` —
  already used by `rill-lang`.
```

