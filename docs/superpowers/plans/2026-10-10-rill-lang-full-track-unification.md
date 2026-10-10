# rill-lang: Full Track Unification (Stage B B3) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the block track entirely — `Rate::Signal` removed from the type system, signals become `Buffer`-typed value channels, and the whole computation path unifies onto the value track where everything is a closure. FFI algorithms become fragments with a single `ValueCallForeign` instruction over `Value::Buffer`.

**Architecture:** Four sub-tasks. B3a1 collapses `Rate` and makes signal channels `Buffer` value channels. B3a2 removes `Scalar`/`elem` and retargets the signal combinators to `Buffer` types. B3a3 routes `lower` through the value track. B3b unifies the RUNTIME (add `ValueCallForeign` + `Value::Buffer`; delete the block track; merge `run_value_track` + `run_call` into a single `run`). B3a keeps the block track executing buffer ops underneath. See `docs/superpowers/specs/2026-10-10-rill-lang-full-track-unification-design.md`.

**Tech Stack:** Rust, crate `rill-lang`, branch `feature/rill-lang-arrays`. No new external dependencies.

---

## Conventions

- Verify after every task: `cargo test -p rill-lang`; before finishing `cargo clippy --all-features --workspace` + `cargo fmt`.
- Pre-commit hook runs `cargo fmt`.
- Commit style: conventional commits, single-quoted `-m`.
- The sub-plan (rank-N + Callable + no-remap) already landed: `Value::Callable { fragment, env, cell: Option }`, `run_call`, `exec_value_instr(prog, instr, base, drops)`. Build on that.
- The forked-rust dialect: no int→f32 coercion (write `&[2.0]`), core::/std:: preludes, `matches!`/`vec![]`.

---

## Task B3a1: Rate collapse + `ValueTy::Buffer` (types only)

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`Rate` collapsed; `ValueTy::Buffer`; `Channel`)
- Modify: `rill-lang/src/types/infer.rs` (`Rate::Signal` arms → `Rate::Value` + `Buffer`; FFI typing)
- Test: `rill-lang/tests/rate_unification.rs`

**Goal:** The type system has ONE rate (`Value`); signal channels carry `vty = Buffer`.
`Scalar`/`elem` stays (removed in B3a2). All existing tests stay green.

- [ ] **Step 1: Audit the sites**

Run:
```bash
grep -n "Rate::Signal" rill-lang/src/types/infer.rs rill-lang/src/types/ty.rs
```
Expected: infer.rs:139, 324, 651, 1579, 3842, 4490, 5283; ty.rs (enum, signal(), scalar(), resolve).

- [ ] **Step 2: Write the failing test**

Create `rill-lang/tests/rate_unification.rs`:
```rust
#[test]
fn signal_channel_is_buffer_value() {
    let src = "main = _ : onepole 200.0 0.7";
    let p = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&p, src.as_bytes()).unwrap();
    let tp = rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Strict).unwrap();
    for c in &tp.process_ty.outs {
        assert_eq!(c.rate, rill_lang::types::ty::Rate::Value,
            "signal output must be a Buffer value channel");
    }
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p rill-lang --test rate_unification`
Expected: FAIL — output channel is `Rate::Signal`.

- [ ] **Step 4: Add `ValueTy::Buffer` + collapse `Rate`**

In `ty.rs`: add `ValueTy::Buffer` (dedicated variant). Collapse `Rate` — remove the
`Signal` variant, make every channel `Value`. Update `Channel::signal` → a
`Channel::value(Buffer)` path; keep `elem`/`Scalar` (B3a2 removes them). Update
the 7 `infer.rs` arms (139, 324, 651, 1579, 3842, 4490, 5283) and the `ty.rs`
resolve (1153) so every channel is `Value` with `vty = Buffer` for signal channels.
FFI typing (`FixedBuffer a`) → `Rate::Value` + `vty=Buffer`. The signal combinators
still use `elem`/`Scalar` for now (that's fine — they're unchanged until B3a2).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p rill-lang`, then `cargo test --workspace`
Expected: PASS — but note: the `rate_unification.rs` assertion on `vty=Buffer`
may need the FFI typing to actually produce `Buffer`; verify the signal test suite
still passes (combinators unchanged).

- [ ] **Step 6: Clippy + fmt + commit**

Run: `cargo clippy --all-features -p rill-lang`, `cargo fmt`.
Commit:
```bash
cargo fmt
git add rill-lang/src/types/ty.rs rill-lang/src/types/infer.rs rill-lang/tests/rate_unification.rs
git commit -m 'refactor(rill-lang): B3a1 — one Rate, signal channels are Buffer value channels'
```

---

## Task B3a2: Remove `Scalar`/`elem`; signal combinators on `Buffer`

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`Scalar` enum removed; `Channel` = `{ vty }`; `Block` alias)
- Modify: `rill-lang/src/types/infer.rs` (signal combinators seq/par/split/merge/loop/delay on `Buffer`; `unify_scalar`/`check_all_numeric` removed; `free_vars`/`instantiate` scalar path removed)
- Test: existing suite

**Goal:** `Scalar`/`elem` are gone — they were the signal element type with no
structures left after `Rate::Signal` removal. The signal combinators unify
`Buffer` types through `vty`.

- [ ] **Step 1: Audit `Scalar`/`elem` uses**

Run:
```bash
grep -rn "Scalar\b\|\.elem\b" rill-lang/src/ | grep -v "//"
```
Expected ~74 `Scalar` uses + ~16 `.elem` uses, concentrated in the combinators
(seq/par/split/merge/loop/delay), `free_vars`, `instantiate`, `check_all_numeric`,
`unify_scalar`.

- [ ] **Step 2: Rewrite the combinators on `Buffer`**

In `infer.rs`, the combinator functions (`seq`, `split`, `merge`, `feedback`,
`delay` — around 4622-4761) currently do `unify_scalar(&a.outs[k].elem, &b.ins[r].elem)`.
Replace with `Buffer`-type unification:
- `seq` (`:`): both sides are `Buffer` channels; the result is `Buffer`. Unify the
  channel `vty` (both `Buffer`) and drop `unify_scalar`/arity mismatch on `elem`.
- `split`/`merge`/`feedback`/`delay`: same — unify `vty` as `Buffer`; remove the
  `elem`/`Scalar` bookkeeping.
- `check_all_numeric` (infer.rs:4770, already a stub): delete or make it a no-op on
  `Buffer`.

- [ ] **Step 3: Remove `Scalar` and the `elem` field**

In `ty.rs`: delete the `Scalar` enum; `Channel` becomes `{ vty: ValueTy }` (drop
`rate` too if B3a1 left it, or keep a single `Value`); `Block` alias removed or
repointed. Update every `Scalar`/`elem` consumer: `free_vars` (only collects
`Scalar::Var` — now value-tyvars only), `instantiate` (the scalar `rw` closure
goes), `Subst.map` (scalar substitution map removed, keep `value_map`),
`unify_scalar` deleted, `Channel::scalar()`/`new()`/`signal()` deleted.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p rill-lang`, then `cargo test --workspace`
Expected: PASS. This is the risky step — the combinators are re-targeted to
`Buffer`; verify `:` on signals (`main = _ : onepole 200.0 0.7`) still types.

- [ ] **Step 5: Clippy + fmt + commit**

Run: `cargo clippy --all-features -p rill-lang`, `cargo fmt`.
Commit:
```bash
cargo fmt
git add rill-lang/src/types/ty.rs rill-lang/src/types/infer.rs
git commit -m 'refactor(rill-lang): B3a2 — remove Scalar/elem; signal combinators unify Buffer types'
```

---

## Task B3a3: Route `lower` through the value track

**Files:**
- Modify: `rill-lang/src/lower.rs` (`has_value_out` becomes the only branch; signal expressions lower through `lower_value` with `Buffer` values; FFI calls → a buffer op that still runs via the block track)
- Test: existing suite + `rate_unification.rs`

**Goal:** `main.body()` ALWAYS lowers through `lower_value`. The block track still
executes the buffer ops underneath (runtime unchanged until B3b), so all signal
tests stay green.

- [ ] **Step 1: Audit `lower`'s value/block split**

Run:
```bash
grep -n "has_value_out\|fn lower_value\|fn lower\b" rill-lang/src/lower.rs | head
```
Expected: the `has_value_out` branch at ~5593.

- [ ] **Step 2: Make `lower_value` the only path**

`has_value_out` becomes trivially true (every output is `Value`). `main.body()`
ALWAYS lowers through `lower_value`. Signal constructs in `lower_value`:
- A main input → a value-track buffer read (a `Value::Buffer` producer; for B3a
  this can be a placeholder the block track still feeds, or a direct `Value::Buffer`
  const — decide what keeps tests green).
- FFI calls (`onepole`, `sine`) → lower as a buffer-op instruction B3b will turn
  into `ValueCallForeign`; for B3a emit a bridge so `exec_foreign_block` still
  works (value-track output register maps to a `block_regs` slot the foreign path
  reads).
- Math (`Un`/`Bin`/`Const`) → keep on the block track (Task B4 makes them `Buffer`
  methods).

The seam: `value in types, buffer in execution`. The value track produces
`Value::Buffer` values whose storage is still `block_regs` for now.

- [ ] **Step 3: Run the tests**

Run: `cargo test -p rill-lang`, then `cargo test --workspace`
Expected: PASS (all existing signal + value tests + rate_unification).

- [ ] **Step 4: Verify examples compile**

Run:
```bash
cargo build -p rill-adrift --features "io,lofi,lang" --example lang_chiptune
cargo build -p rill-adrift --features "io,lang,sampler" --example record_mic
```
Expected: both compile.

- [ ] **Step 5: Clippy + fmt + commit**

Run: `cargo clippy --all-features -p rill-lang`, `cargo fmt`.
Commit:
```bash
cargo fmt
git add rill-lang/src/lower.rs
git commit -m 'refactor(rill-lang): B3a3 — lower signal expressions through the value track'
```

---

## Task B3b: Runtime unification — delete the block track

**Files:**
- Modify: `rill-lang/src/arena.rs` (`Value::Buffer`)
- Modify: `rill-lang/src/ir.rs` (`ValueInstr::ValueCallForeign`; remove `Instr` block variants)
- Modify: `rill-lang/src/backend/interp.rs` (merge `run_value_track` + `run_call` into a single `run`; `ValueCallForeign` arm; delete `run_block_mimo`/`exec_block_op`/`exec_foreign_block`)
- Modify: `rill-lang/src/program.rs` (remove `block_regs`; `process()` through value track)
- Modify: `rill-lang/src/lower.rs` (buffer ops → `ValueCallForeign`/`Value::Buffer`)
- Delete: `rill-lang/src/schedule.rs`, `rill-lang/src/regalloc.rs` (if only block IR)
- Test: `rill-lang/tests/rate_unification.rs`, existing suite

**Goal:** The block track is GONE. FFI algorithms are fragments with a single
`ValueCallForeign` instruction over `Value::Buffer`. One `run` runner.

- [ ] **Step 1: Add `Value::Buffer`**

In `arena.rs`:
```rust
/// A first-class signal buffer. Inline `FixedBuffer` backing for B3;
/// `RegionHandle` (large slab) layers on in Task B6.
Value::Buffer(FixedBuffer<T, BUF>),   // generic T? No — arena is not generic.
```
Note: `Value` is not generic over `T`. So `Value::Buffer` must hold the buffer
as a **table index** into `RillProgram`'s buffer store (the old `block_regs`
becomes a per-program `Vec<FixedBuffer<T,BUF>>` that `Value::Buffer(usize)`
indexes), OR `Value::Buffer` holds the data boxed. **Choose the table-index
approach**: `Value::Buffer(usize)` indexes `prog.buffer_regs` (renamed
`block_regs`). Update `visit_refs`/`clone_pooled`/`drop_ref`/`kind` for a leaf
value (owns no arena children).

- [ ] **Step 2: Add `ValueInstr::ValueCallForeign`**

In `ir.rs`:
```rust
/// Call a foreign DSP algorithm over `Value::Buffer` operands.
ValueInstr::ValueCallForeign { dst: usize, srcs: Vec<usize>, instance: usize },
```
Wire it into `is_alloc_producing`/budget, `remap`-equivalent (base threading in
`exec_value_instr` — the arm reads `dst + base`, `srcs[i] + base`), and the
dispatch.

- [ ] **Step 3: Implement the `ValueCallForeign` arm**

In `exec_value_instr`:
1. Read `srcs` registers — each must be `Value::Buffer(idx)` (input channels).
2. Read `prog.buffer_regs[idx]` (`FixedBuffer`), call
   `Algorithm::process`/`MultichannelAlgorithm::process` with the input buffers.
3. Write the output buffer(s) into `dst` (+ consecutive regs) as fresh
   `Value::Buffer(out_idx)` (allocate output slots in `buffer_regs`).
4. Carry over the current aliasing rules (scratch snapshot for multichannel;
   `dst != srcs[0]` for single-channel).

- [ ] **Step 4: Merge `run_value_track` + `run_call` into a single `run`**

Both already share the block-loop + branch-terms + `exec_value_instr(prog, instr, base, drops)` + drops-drain shape; the only differences are which block list to run (`ir.value_blocks`/`value_entry` vs `frag.value_blocks`/`frag.entry`), the base (0 vs fragment offset), and env-binding/args-source. Merge them into ONE runner:

```rust
/// Run one fragment's value blocks. The program is a root fragment (base=0,
/// empty env); a function/FFI call is a nested fragment (base=offset, env +
/// args). Everything is a closure.
fn run<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    blocks: &[ValueBlock],        // the fragment's blocks (or ir.value_blocks for the root)
    entry: usize,                  // the fragment's entry (or ir.value_entry)
    base: usize,                   // register offset (0 for the root)
    env_ref: ArenaRef,             // env record (Void for the root)
    args: ArgsSource<'_>,          // Registers(&[]) for the root
    dst: Option<usize>,            // result register (None: force/memo path)
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef>
```

`run_value_track` becomes `run(prog, &prog.ir.value_blocks, prog.ir.value_entry, 0, void_env, ArgsSource::Registers(&[]), None, drops)`. The program's `Ir` can keep `value_blocks`/`value_entry` (the root fragment) or gain a `root_fragment` — pick whichever avoids re-plumbing. Update `process()` and `force()` to call `run`. Rename `run_call` → `run` everywhere. `swap_block_state`/`block_state` migrate to `value_state` (feedback via `value_state` reads/writes). The name `run` (or `run_thunk`) signals that every computation is a thunk/fragment execution.

- [ ] **Step 5: Delete the block track**

- `ir.rs`: remove `Instr` block variants (`Const`, `LoadInput`, `ReadBlockState`,
  `ReadDelay`, `Un`, `Bin`, `Move`, `WriteBlockState`, `WriteDelay`, `CallBlock`,
  `ReadParam`, `ReadActorParam`, `ReadMainCell`, `ProbePoint`). Keep the value
  variants. If the `Instr`/`ValueInstr` enums were merged in an earlier stage,
  remove the block arms.
- `backend/interp.rs`: delete `run_block_mimo`, `exec_block_op`,
  `exec_foreign_block`.
- `schedule.rs`: delete the file (and its `mod`/use).
- `regalloc.rs`: if it only handled block IR, delete it.
- `program.rs`: rename/keep `block_regs` as the `Value::Buffer` backing store;
  remove the block-track fields (`block_state`/`block_state_next`/`delays` → the
  value-track equivalents if any).

- [ ] **Step 6: Lower buffer ops to the value track**

In `lower.rs`: replace the B3a bridge with direct value-track lowering:
- FFI calls → a one-instruction fragment `[ValueCallForeign]`.
- Math (`Un`/`Bin`/`Const`) → `Buffer` method placeholders (Task B4 makes them
  real; for B3b lower to a `ValueCallForeign`-style buffer op or leave a clear
  TODO that B4 fills). If math must work for B3b, wire it to the existing
  `buffer_math` scalar loops over `Value::Buffer` storage.
- Inputs/outputs → value-track buffer reads/writes; `process()` materializes
  `inputs` as `Value::Buffer` and drains `outputs` from the value-output regs.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p rill-lang`, then `cargo test --workspace`
Expected: PASS. Fix regressions — the seam is buffer ops over `Value::Buffer`.

- [ ] **Step 8: Verify examples compile**

Run:
```bash
cargo build -p rill-adrift --features "io,lofi,lang" --example lang_chiptune
cargo build -p rill-adrift --features "io,lang,sampler" --example record_mic
```
Expected: both compile.

- [ ] **Step 9: Clippy + fmt + commit**

Run: `cargo clippy --all-features -p rill-lang`, `cargo fmt`.
Commit:

```bash
cargo fmt
git add rill-lang/src/arena.rs rill-lang/src/ir.rs rill-lang/src/backend/interp.rs rill-lang/src/program.rs rill-lang/src/lower.rs rill-lang/tests/rate_unification.rs
git rm rill-lang/src/schedule.rs rill-lang/src/regalloc.rs
git commit -m 'refactor(rill-lang): B3b — block track eliminated, everything is a closure
Value::Buffer + ValueCallForeign. run_value_track + run_call merged into one
run (everything is a closure). Deleted
Instr block variants, block_regs as a track, run_block_mimo, build_schedule,
exec_block_op, exec_foreign_block, schedule.rs. FFI = fragment with one
ValueCallForeign over Value::Buffer.'
```

---

## Self-review checklist

- [ ] §2.1 everything is a closure → B3b.
- [ ] §2.2 `Value::Buffer` (table-index, inline backing) → B3b.
- [ ] §2.3 `ValueCallForeign` → B3b.
- [ ] §2.4 lowering signals to the value track → B3a (types/lower), B3b (buffer ops).
- [ ] §2.5 runtime unification (merge runners, delete block track) → B3b.
- [ ] §2.6 state → `value_state` → B3b.
- [ ] §3 backward compat + RT safety → both tasks verify the full suite.
- [ ] §4 testing → `rate_unification.rs` + existing suite.
- [ ] §6 B3a/B3b split → Tasks B3a, B3b.