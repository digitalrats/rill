# rill-lang: Full Track Unification (Stage B B3) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the block track entirely — `Rate::Signal` removed from the type system, signals become `Buffer`-typed value channels, and the whole computation path unifies onto the value track where everything is a closure. FFI algorithms become fragments with a single `ValueCallForeign` instruction over `Value::Buffer`.

**Architecture:** Two sub-tasks. B3a unifies the TYPE SYSTEM and LOWERING (signals = `ValueTy::Buffer`, `lower` routes signal expressions through the value track) while the block track still executes the buffer ops underneath. B3b unifies the RUNTIME (add `ValueCallForeign` + `Value::Buffer`; delete `Instr` block variants, `block_regs`, `run_block_mimo`, `build_schedule`, `exec_block_op`, `exec_foreign_block`, `schedule.rs`; merge `run_value_track` + `run_call` into a single `run`). See `docs/superpowers/specs/2026-10-10-rill-lang-full-track-unification-design.md`.

**Tech Stack:** Rust, crate `rill-lang`, branch `feature/rill-lang-arrays`. No new external dependencies.

---

## Conventions

- Verify after every task: `cargo test -p rill-lang`; before finishing `cargo clippy --all-features --workspace` + `cargo fmt`.
- Pre-commit hook runs `cargo fmt`.
- Commit style: conventional commits, single-quoted `-m`.
- The sub-plan (rank-N + Callable + no-remap) already landed: `Value::Callable { fragment, env, cell: Option }`, `run_call`, `exec_value_instr(prog, instr, base, drops)`. Build on that.
- The forked-rust dialect: no int→f32 coercion (write `&[2.0]`), core::/std:: preludes, `matches!`/`vec![]`.

---

## Task B3a: Type + lowering unification (signals = `Buffer` values)

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`Rate` collapsed; `ValueTy::Buffer`; `Channel`; `Scalar`/`elem` fate)
- Modify: `rill-lang/src/types/infer.rs` (8 `Rate::Signal` arms → `Rate::Value` + `Buffer`; FFI typing)
- Modify: `rill-lang/src/lower.rs` (`has_value_out` becomes the only branch; signal expressions lower through `lower_value` with `Buffer` values; FFI calls → a buffer op that still runs via the block track for now)
- Test: `rill-lang/tests/rate_unification.rs`

**Goal:** The type system has ONE rate (`Value`) and signal channels carry `vty = Buffer`. `lower` routes `main`'s body through the value track. The block track still executes the buffer ops underneath (runtime unchanged in B3a), so all signal tests stay green.

- [ ] **Step 1: Audit the exact sites**

Run:
```bash
grep -n "Rate::Signal" rill-lang/src/types/infer.rs rill-lang/src/types/ty.rs
grep -n "has_value_out\|fn lower_value\|fn lower\b" rill-lang/src/lower.rs | head
```
Document each `Rate::Signal` site's role. Expected: infer.rs:139, 324, 651, 1579, 3842, 4490, 5283 + ty.rs (enum, signal(), scalar(), resolve). In lower.rs: the `has_value_out` branch (the value/block split at ~5593).

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
        // vty is Buffer (or App("Buffer", []))
    }
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p rill-lang --test rate_unification`
Expected: FAIL — the output channel is `Rate::Signal`.

- [ ] **Step 4: Add `ValueTy::Buffer` + collapse `Rate`**

In `ty.rs`: add `ValueTy::Buffer` (a dedicated variant is cleaner than
`App("Buffer", vec![])` for the runtime seam). Collapse `Rate` — either remove
the enum and keep only `vty`, or keep `Rate` with a single `Value` value (pick
the less invasive; prefer removing the `Signal` variant and making every channel
`Value`). Update `Channel::signal` → a `Channel::value(Buffer)` path. Decide the
`Scalar`/`elem` fate: if `FixedBuffer f32` needs the scalar, keep `elem` tied to
`Buffer`'s parameter; if it becomes dead, delete it (but beware `check_all_numeric`
and `unify_scalar` — keep them if the signal element typing depends on them).

- [ ] **Step 5: Rewrite the `infer.rs` arms**

Each `Rate::Signal` arm → `Rate::Value` with `vty = Buffer`. The `instantiate`
block closure (infer.rs:139) and `resolve` (ty.rs:1153) must preserve `Buffer`
channels. FFI typing (`FixedBuffer a` in foreign sigs) → `Rate::Value` + `vty=Buffer`.
`check_all_numeric`/arity: a `Buffer` channel is a value channel whose `vty` is
`Buffer`.

- [ ] **Step 6: Route `lower` through the value track**

In `lower.rs`: `has_value_out` becomes trivially true (every output is `Value`).
`main.body()` ALWAYS lowers through `lower_value`. Signal constructs in
`lower_value`:
- A main input → a value-track buffer read (a `Value::Buffer` producer; for B3a
  this can be a placeholder that the block track still feeds, or a direct
  `Value::Buffer` const — decide what keeps tests green).
- FFI calls (`onepole`, `sine`, etc.) → lower as a buffer-op instruction that
  B3b will turn into `ValueCallForeign`. For B3a, keep it running via the block
  track: emit a bridge so `exec_foreign_block` still works (e.g. the value-track
  output register maps to a `block_regs` slot the foreign path reads).
- Math (`Un`/`Bin`/`Const`) → keep lowering to the block track for B3a (Task B4
  makes them `Buffer` methods).

The seam: `value in types, buffer in execution`. The value track produces
`Value::Buffer` values whose storage is still `block_regs` for now.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p rill-lang`, then `cargo test --workspace`
Expected: PASS (all existing signal + value tests + the new one). If signal tests
break, adjust the lower bridge so buffer ops still execute via the block track.

- [ ] **Step 8: Clippy + fmt + commit**

Run: `cargo clippy --all-features -p rill-lang`, `cargo fmt`.
Commit:

```bash
cargo fmt
git add rill-lang/src/types/ty.rs rill-lang/src/types/infer.rs rill-lang/src/lower.rs rill-lang/tests/rate_unification.rs
git commit -m 'refactor(rill-lang): B3a — signals are Buffer value channels, lower through value track
Rate::Signal removed from the type system; signal channels carry vty=Buffer.
main lowers through the value track; block track still executes buffer ops.'
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