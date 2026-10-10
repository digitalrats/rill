# rill-lang: Full Track Unification (Stage B B3) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the block track entirely — `Rate::Signal` removed from the type system, signals become `Buffer`-typed value channels, and the whole computation path unifies onto the value track where everything is a closure. FFI algorithms become fragments with a single `ValueCallForeign` instruction over `Value::Buffer`.

**Architecture:** Four steps. B3-1 adds `ValueTy::Buffer` and transitions the TYPE SYSTEM to a linear lambda model (`ArrowTy = Vec<ValueTy>`, `Rate`/`Channel`/`Scalar` removed), parallel to the still-working signal track. B3-2 switches program entry/exit to `Value::Buffer` (value track), making the block track dead code. B3-3 verifies end-to-end. B3-4 deletes the dead signal track and merges `run_value_track` + `run_call` into a single `run`. New-model tests are the source of truth; old signal tests are rewritten for the new model. See `docs/superpowers/specs/2026-10-10-rill-lang-full-track-unification-design.md`.

**Tech Stack:** Rust, crate `rill-lang`, branch `feature/rill-lang-arrays`. No new external dependencies.

---

## Conventions

- Verify after every task: `cargo test -p rill-lang`; before finishing `cargo clippy --all-features --workspace` + `cargo fmt`.
- Pre-commit hook runs `cargo fmt`.
- Commit style: conventional commits, single-quoted `-m`.
- The sub-plan (rank-N + Callable + no-remap) already landed: `Value::Callable { fragment, env, cell: Option }`, `run_call`, `exec_value_instr(prog, instr, base, drops)`. Build on that.
- The forked-rust dialect: no int→f32 coercion (write `&[2.0]`), core::/std:: preludes, `matches!`/`vec![]`.

---

## Task B3-1: Buffer value type + linear-lambda type system (parallel to signal track)

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (remove `Rate`/`Channel`/`Scalar`; `ArrowTy = { ins: Vec<ValueTy>, outs: Vec<ValueTy> }`; `ValueTy::Buffer`)
- Modify: `rill-lang/src/types/infer.rs` (all rate/Channel/Scalar sites; combinators on `ValueTy` vectors)
- Modify: `rill-lang/src/types/unify.rs` (remove `unify_scalar`/scalar substitution), `types/arrow.rs`, `types/ffi.rs`
- Modify: `rill-lang/src/lower.rs` (type-system call sites; signal expressions still lower to the block track for now)
- Test: `rill-lang/tests/linear_lambda.rs` (new-model tests)

**Goal:** The type system is a linear lambda model: `ArrowTy = { ins: Vec<ValueTy>, outs: Vec<ValueTy> }`, no `Rate`/`Channel`/`Scalar`. Signals are `ValueTy::Buffer` (an ordinary value type). The block track still executes underneath (no runtime change). **New-model tests are the source of truth; old signal tests are rewritten for the new model, not kept green through the refactor.**

- [ ] **Step 1: Add `ValueTy::Buffer` and rewrite `ArrowTy`**

In `ty.rs`: add `ValueTy::Buffer`. Replace `ArrowTy { ins: Vec<Block>, outs: Vec<Block> }` with `ArrowTy { ins: Vec<ValueTy>, outs: Vec<ValueTy> }`. Delete `Rate`, `Channel`, `Scalar`. Update `ArrowTy` helpers (`uniform`, `value_channel`, arity).

- [ ] **Step 2: Write the new-model test**

Create `rill-lang/tests/linear_lambda.rs`:
```rust
#[test]
fn arrowty_is_plain_value_types() {
    // No Rate/Channel/Scalar: ArrowTy carries ValueTy vectors, signals are Buffer.
    let src = "main = _ : onepole 200.0 0.7";
    let p = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&p, src.as_bytes()).unwrap();
    let tp = rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Strict).unwrap();
    for o in &tp.process_ty.outs {
        assert_eq!(o, &rill_lang::types::ty::ValueTy::Buffer);
    }
}
```

- [ ] **Step 3: Rewrite `infer.rs` for `Vec<ValueTy>`**

Every `.rate`/`.elem`/`.vty` on channels becomes a plain `ValueTy` element access. The combinators (seq/par/split/merge/feedback/delay) operate on `ValueTy` vectors, unifying `ValueTy` (Buffer for signals). `free_vars`/`instantiate` drop the scalar path (already value-only). FFI typing (`FixedBuffer a`) → `ValueTy::Buffer`.

- [ ] **Step 4: Update `unify.rs`/`arrow.rs`/`ffi.rs`**

Remove `unify_scalar` and the scalar substitution map (`Subst.map`); keep `value_map`. Update `arrow.rs` and `ffi.rs` to the `Vec<ValueTy>` shape.

- [ ] **Step 5: Update `lower.rs` type call sites**

Update the type-system call sites (subtree_size, arity, FFI). Signal expressions still lower to the block track (runtime unchanged) — only the typing is new.

- [ ] **Step 6: Update the old signal tests**

Rewrite the signal tests (ffi.rs, graph_*, arrow.rs, categories.rs, lower::tests) that reference `Rate`/`Channel`/`Scalar`/`.rate` to the new model. Old-model assertions are removed/replaced.

- [ ] **Step 7: Run the new-model tests**

Run: `cargo test -p rill-lang --test linear_lambda`
Expected: PASS. Old signal tests may still fail (they are being rewritten) — the new-model tests are the source of truth for this step.

- [ ] **Step 8: Commit**

```bash
cargo fmt
git add rill-lang/src/types/ rill-lang/src/lower.rs rill-lang/tests/linear_lambda.rs
git commit -m 'refactor(rill-lang): B3-1 — linear lambda type system (ArrowTy = Vec<ValueTy>), Buffer value type
Remove Rate/Channel/Scalar. Signals are ValueTy::Buffer. Block track still
executes underneath. Old signal tests rewritten for the new model.'
```

---

## Task B3-2: Switch program entry/exit to `Value::Buffer` (value track)

**Files:**
- Modify: `rill-lang/src/arena.rs` (`Value::Buffer(usize)`)
- Modify: `rill-lang/src/program.rs` (program-level buffer table `Vec<RegionHandle<T>>` + `BufferAllocator`)
- Modify: `rill-lang/src/lower.rs` (signal expressions lower to the value track; FFI calls → a buffer op; inputs/outputs → `Value::Buffer` reads/writes)
- Modify: `rill-lang/src/backend/interp.rs` (value-track executes `Value::Buffer` ops in-place via `with_data_mut`; block track becomes dead)
- Test: `rill-lang/tests/linear_lambda.rs`, updated signal tests

**Goal:** `main`'s inputs/outputs are `Value::Buffer`; signal programs run through the value track. The block track is dead code (present, unreferenced).

**Memory model (user directive):** Buffer is an ORDINARY value type backed by the separate Tier-1 big-object memory (SP-4): `Value::Buffer(usize)` indexes a program-level `Vec<RegionHandle<T>>` carved from a `BufferAllocator` at build (program-lifetime static, RC). Buffer mutation is IN-PLACE and scoped by an ST region (`runST (fn s -> …)`); the phantom `s` prevents a mutated buffer from leaking. See `docs/superpowers/specs/2026-10-10-rill-lang-full-track-unification-design.md` §2.2.

- [ ] **Step 1: Add `Value::Buffer`**

In `arena.rs`: `Value::Buffer(usize)` (index into the program-level buffer table). Leaf value — `visit_refs`/`detach_payload`/`clone_pooled` treat it as owning no arena children. Add `ValueKind::Buffer`.

- [ ] **Step 2: Program buffer table**

In `program.rs`: `RillProgram` owns a `BufferAllocator<T>` and `buffers: Vec<RegionHandle<T>>`, sized at build from `Ir::num_buffers` (each handle length `BUF`). Remove `block_regs` from construction.

- [ ] **Step 3: Lower signal expressions to the value track**

In `lower.rs`: signal expressions produce `Value::Buffer` on the value track. FFI calls → a buffer-op instruction (B3-4 makes it `ValueCallForeign`). Inputs/outputs → value-track buffer reads/writes. The 1251-line block `lower` path is replaced by value-track lowering.

- [ ] **Step 4: Run through the value track**

In `interp.rs`: the value track executes `Value::Buffer` ops (math via `buffer_math` in-place on `with_data_mut` slices, FFI via `Algorithm::process` over `Value::Buffer`). `process()` materializes inputs as `Value::Buffer` and drains outputs from value-output regs. The block track is not deleted, just unreferenced.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p rill-lang`, then `cargo test --workspace`
Expected: PASS — signal programs run via the value track; block track is dead.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add rill-lang/src/arena.rs rill-lang/src/lower.rs rill-lang/src/backend/interp.rs rill-lang/src/program.rs
git commit -m 'refactor(rill-lang): B3-2 — program entry/exit switched to Value::Buffer (value track)
Signal programs run through the value track; block track is now dead code.'
```

---

## Task B3-3: Verify end-to-end

**Files:**
- Verify only (no code change unless a bug surfaces)
- Test: existing suite + examples

**Goal:** the unified pipeline produces the same output as before.

- [ ] **Step 1: Run the full suite**

Run: `cargo test --workspace`, `cargo clippy --all-features --workspace`, `cargo fmt`
Expected: green.

- [ ] **Step 2: Verify examples compile**

Run:
```bash
cargo build -p rill-adrift --features "io,lofi,lang" --example lang_chiptune
cargo build -p rill-adrift --features "io,lang,sampler" --example record_mic
```
Expected: both compile.

- [ ] **Step 3: Commit (if any verification fix landed)**

```bash
cargo fmt
git add -A
git commit -m 'fix(rill-lang): B3-3 — end-to-end verification of the unified pipeline'
```

---

## Task B3-4: Delete the dead signal track; merge the runners

**Files:**
- Modify: `rill-lang/src/ir.rs` (`ValueCallForeign`; remove `Instr` block variants)
- Modify: `rill-lang/src/backend/interp.rs` (merge `run_value_track` + `run_call` into a single `run`; `ValueCallForeign` arm; delete `run_block_mimo`/`exec_block_op`/`exec_foreign_block`)
- Modify: `rill-lang/src/program.rs` (remove `block_regs`), `rill-lang/src/schedule.rs` (delete), `rill-lang/src/regalloc.rs` (delete)
- Test: existing suite

**Goal:** the block track is gone. One `run` runner; everything is a closure.

- [ ] **Step 1: Add `ValueCallForeign`**

In `ir.rs`: `ValueInstr::ValueCallForeign { dst, srcs, instance }`. Wire budget/remap (base threading in `exec_value_instr`). In `exec_value_instr`: read `srcs` (each `Value::Buffer`), call `Algorithm::process`/`MultichannelAlgorithm::process`, write output buffers.

- [ ] **Step 2: Merge `run_value_track` + `run_call` into `run`**

```rust
fn run(prog, blocks: &[ValueBlock], entry: usize, base: usize,
       env_ref, args: ArgsSource, dst: Option<usize>, drops) -> Option<ArenaRef>
```
`run_value_track` becomes `run(prog, &ir.value_blocks, ir.value_entry, 0, void_env, ArgsSource::Registers(&[]), None, drops)`. Rename `run_call` → `run`.

- [ ] **Step 3: Delete the block track**

- `ir.rs`: remove `Instr` block variants (Const/LoadInput/ReadBlockState/ReadDelay/Un/Bin/Move/WriteBlockState/WriteDelay/CallBlock/ReadParam/ReadActorParam/ReadMainCell/ProbePoint).
- `interp.rs`: delete `run_block_mimo`, `exec_block_op`, `exec_foreign_block`.
- `schedule.rs`: delete the file. `regalloc.rs`: delete (block-IR only).
- `program.rs`: remove `block_regs` and block-state fields.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p rill-lang`, then `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Clippy + fmt + commit**

Run: `cargo clippy --all-features -p rill-lang`, `cargo fmt`.
Commit:
```bash
cargo fmt
git add rill-lang/src/ir.rs rill-lang/src/backend/interp.rs rill-lang/src/program.rs
git rm rill-lang/src/schedule.rs rill-lang/src/regalloc.rs
git commit -m 'refactor(rill-lang): B3-4 — block track deleted, everything is a closure
ValueCallForeign + Value::Buffer. run_value_track + run_call merged into one
run. Deleted Instr block variants, block_regs, run_block_mimo, build_schedule,
exec_*, schedule.rs, regalloc.rs.'
```

---

## Self-review checklist

- [ ] §2.1 linear lambda model (everything is a closure) → B3-1 (type system), B3-4 (runners).
- [ ] §2.2 `Value::Buffer` → B3-2.
- [ ] §2.3 `ValueCallForeign` → B3-4.
- [ ] §2.4 lowering signals to the value track → B3-2.
- [ ] §2.5 runtime unification (merge runners, delete block track) → B3-4.
- [ ] §2.6 state → `value_state` → B3-4.
- [ ] §3 backward compat + RT safety → B3-3 verifies the full suite + examples.
- [ ] §4 testing → `linear_lambda.rs` (new-model) + rewritten signal tests.
- [ ] §6 B3-1..B3-4 order → Tasks B3-1..B3-4.