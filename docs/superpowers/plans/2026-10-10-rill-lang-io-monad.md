# rill-lang: Full IO Monad (Stage B IO) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** introduce the full IO monad. `main :: Buffer -> … -> IO ()`; the runtime LEADS the returned IO action each tick (Haskell RTS model). Old `main = <buffer-expr>` programs keep working via auto-desugar. See `docs/superpowers/specs/2026-10-10-rill-lang-io-monad-design.md`.

**Architecture:** Five tasks. IO-1 types (`IO a` + `instance Monad IO` + primitives). IO-2 desugar (combinators→functions, `_` as param name, old-main auto-desugar). IO-3 IR + lowering (`ValueReadInput`/`ValueWriteOutput`, main as root fragment). IO-4 runtime (lead main each tick). IO-5 tests (new IO tests, re-enable `#[ignore]`d signal-lambda + source-node tests, full suite green).

**Tech Stack:** Rust, crate `rill-lang`, branch `feature/rill-lang-arrays`. No new external dependencies.

---

## Conventions

- Verify after every task: `cargo test -p rill-lang`; before finishing `cargo clippy --all-features --workspace` + `cargo fmt`.
- Pre-commit hook runs `cargo fmt`.
- Commit style: conventional commits, single-quoted `-m`.
- B3-2 landed: signals on the value track (ValueBuffer* ops over Value::Buffer/RegionHandle), block track deleted. `runST` desugars to Apply+Lambda. `Monad` typeclass + do-notation work.
- The forked-rust dialect: no int→f32 coercion (write `&[2.0]`), core::/std:: preludes, `matches!`/`vec![]`.

---

## Task IO-1: Types — `IO a`, `instance Monad IO`, primitives

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (prelude: `data IO a` + `instance Monad IO`; `readInput`/`writeOutput` builtin sigs)
- Test: new `rill-lang/tests/io_types.rs` (IO types + Monad IO inference)

**Goal:** `IO a` is a first-class type with a `Monad` instance; `readInput : Int -> IO Buffer` and `writeOutput : Int -> Buffer -> IO ()` are builtin signatures.

- [ ] **Step 1: Add `IO a` + `instance Monad IO` to the prelude**

In `ty.rs` `CATEGORY_PRELUDE` (next to `data ST s a`):
```haskell
data IO a = { unIO: a };
instance Monad IO where {
    return x = IO { unIO: x };
    bind m f = IO { unIO: (f m.unIO).unIO };
};
```

- [ ] **Step 2: Add `readInput`/`writeOutput` builtin signatures**

In `with_builtins`, alongside `runST` (`env.builtin_sigs`):
```rust
env.builtin_sigs.insert("readInput".into(),
    TFunc(vec![TName("Int")], Box::new(TApp("IO".into(), vec![TName("Buffer")]))));
env.builtin_sigs.insert("writeOutput".into(),
    TFunc(vec![TName("Int"), TName("Buffer")], Box::new(TApp("IO".into(), vec![TName("Void")]))));
```
(Requires `Buffer`/`Void` as type names; see Step 3.)

- [ ] **Step 3: Ensure `Buffer`/`Void` resolve as type names**

`Buffer` is `ValueTy::Buffer`; `()` needs a `Void`/unit type. Either add
`ValueTy::Void` or reuse an existing unit (`data Void = Void` in prelude). Pick
the minimal one; `writeOutput`'s result is `IO ()`.

- [ ] **Step 4: Test**

`rill-lang/tests/io_types.rs`:
```rust
#[test]
fn io_is_a_monad() {
    let t = arrowty("main = do { writeOutput 0 (return 1.0); return () }");
    // `main` types as IO (); the do-block is a Monad IO computation.
}
```

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add rill-lang/src/types/ty.rs rill-lang/tests/io_types.rs
git commit -m 'feat(rill-lang): IO-1 — IO a + instance Monad IO, readInput/writeOutput signatures'
```

---

## Task IO-2: Desugar — combinators→functions, `_` as param, old-main auto-desugar

**Files:**
- Modify: `rill-lang/src/parser.rs` (`_` as param name; main form)
- Modify: `rill-lang/src/desugar.rs` (combinators→functions; old-main auto-desugar)
- Modify: `rill-lang/src/ast.rs` (if `Expr::Wire` removed/repurposed)

**Goal:** `main` is a function `Buffer -> … -> IO ()`; Faust combinators are
sugar over ordinary functions; old `main = <buffer-expr>` programs auto-desugar
to the IO form.

- [ ] **Step 1: `_` as an ordinary parameter name**

Parser: in `main`'s λ-parameter list, `_` becomes a fresh name (`i1`, `i2`, …).
Inside the body, `Expr::Wire` resolves to that parameter. (Keep `Expr::Wire`
for now; change its *meaning* to "the k-th main parameter".)

- [ ] **Step 2: Combinators → functions in desugar**

In `desugar.rs`, rewrite `Expr::Seq/Par/Split/Merge/Loop/Delay` into function
applications (`compose a b`, `par a b`, `split a b`, `merge a b`,
`feedback a b`, `delay n a`) against prelude functions over `Buffer` arrows.

- [ ] **Step 3: Old-main auto-desugar**

`main = <buffer-expr>` → `main i1 i2 = do { writeOutput 0 <out0>; …;
writeOutput N-1 <outN-1>; return () }` where `<outK>` are the buffer outputs of
`<buffer-expr>` and `i1..` the wires in order of first use.

- [ ] **Step 4: Test**

Old signal programs (`main = _ : onepole 200.0 0.7`, `main = osc , osc`) still
compile; new `main i1 = do { … }` form compiles.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add rill-lang/src/parser.rs rill-lang/src/desugar.rs rill-lang/src/ast.rs
git commit -m 'feat(rill-lang): IO-2 — main as Buffer -> … -> IO (); combinators desugar to functions; old-main auto-desugar'
```

---

## Task IO-3: IR + Lowering — `ValueReadInput`/`ValueWriteOutput`, main as root fragment

**Files:**
- Modify: `rill-lang/src/ir.rs` (`ValueReadInput`, `ValueWriteOutput`)
- Modify: `rill-lang/src/lower.rs` (main lowers as a root fragment; writeOutput/readInput lowering)

**Goal:** `main :: Buffer -> … -> IO ()` lowers to a root fragment; the program's
outputs are the `writeOutput` sites.

- [ ] **Step 1: Add `ValueReadInput`/`ValueWriteOutput`**

```rust
ValueInstr::ValueReadInput { dst: usize, index: usize },
ValueInstr::ValueWriteOutput { index: usize, src: usize },
```

- [ ] **Step 2: Lower `main` as a root fragment**

`main`'s λ-parameters bind to input buffer registers; the body (the IO action)
lowers through the value track. `writeOutput k buf` emits
`ValueWriteOutput { index: k, src: buf }`; `readInput k` emits
`ValueReadInput { dst, index: k }`.

- [ ] **Step 3: Outputs = writeOutput sites**

`num_outputs` = max writeOutput index + 1; the runtime drains each `src` buffer
into `outputs[index]`.

- [ ] **Step 4: Test**

`main i1 = do { writeOutput 0 (onepole 200.0 0.7 i1); return () }` lowers with a
`ValueWriteOutput` and `num_outputs == 1`.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add rill-lang/src/ir.rs rill-lang/src/lower.rs
git commit -m 'feat(rill-lang): IO-3 — ValueReadInput/ValueWriteOutput; main lowers as root fragment'
```

---

## Task IO-4: Runtime — lead main each tick

**Files:**
- Modify: `rill-lang/src/backend/interp.rs` (`ValueReadInput`/`ValueWriteOutput` arms; lead main)
- Modify: `rill-lang/src/program.rs` (drain via writeOutput sites)

**Goal:** `process()` materializes inputs, LEADS `main` (invokes the root
fragment with the input buffer registers), and drains outputs via `writeOutput`.

- [ ] **Step 1: `ValueReadInput` arm**

Read `buffers[input_buffers[index]]` → allocate `Value::Buffer` marker into `dst`.

- [ ] **Step 2: `ValueWriteOutput` arm**

Copy `src` buffer's slice into the external `outputs[index]` slice (store the
index → slice mapping on the program for the tick).

- [ ] **Step 3: Lead main**

`run_block_mimo` invokes `main`'s callable (root fragment) with the materialized
input buffer registers, then drains the writeOutput sites. This replaces the
current "value_output_regs" drain for IO programs (kept for old value programs).

- [ ] **Step 4: Test**

Run the full suite — old programs (auto-desugared) and new IO programs produce
correct output.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add rill-lang/src/backend/interp.rs rill-lang/src/program.rs
git commit -m 'feat(rill-lang): IO-4 — runtime leads main (IO action) each tick, drains via writeOutput'
```

---

## Task IO-5: Tests — re-enable #[ignore]d tests, verify green

**Files:**
- Modify: `rill-lang/tests/currying_wires.rs`, `rill-lang/tests/func_values.rs`, `rill-adrift/tests/engine_params.rs` (remove `#[ignore]`)
- Test: new IO tests

**Goal:** the full suite is green with IO-form `main`.

- [ ] **Step 1: Re-enable signal-lambda tests**

Remove `#[ignore]` from `signal_wire_argument_scales_block`,
`caf_applied_at_two_sites_with_different_wires`, `function_reference_calls_signal_definition`,
`func_value_can_be_chained`. Fix any remaining mismatch with the IO form.

- [ ] **Step 2: Re-enable rill-adrift source-node tests**

Remove `#[ignore]` from `source_node_produces_output`,
`source_node_with_param_routing`, `source_node_without_param_produces_default`.

- [ ] **Step 3: New IO tests**

- `main i1 = do { writeOutput 0 (onepole 200.0 0.7 i1); return () }` == old
  `main = _ : onepole 200.0 0.7`.
- `do` chaining two writeOutput calls.
- `readInput 0`.

- [ ] **Step 4: Full verification**

Run: `cargo test --workspace`, `cargo clippy --all-features --workspace`, `cargo fmt`.
Expected: green, 0 clippy warnings.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add -A
git commit -m 'feat(rill-lang): IO-5 — IO main form verified; signal-lambda + source-node tests re-enabled'
```

---

## Self-review checklist

- [ ] §2.1 `IO a` + `instance Monad IO` → IO-1.
- [ ] §2.2 `main :: Buffer -> … -> IO ()` → IO-2 (form), IO-3 (lowering), IO-4 (runtime).
- [ ] §2.3 primitives → IO-1 (types), IO-3 (IR/lowering), IO-4 (runtime).
- [ ] §2.4 combinators→functions + `_` as param → IO-2.
- [ ] §2.5 old-main auto-desugar → IO-2.
- [ ] §2.6 runtime leads IO → IO-4.
- [ ] §4 testing + re-enabled #[ignore] tests → IO-5.
- [ ] §6 IO-1..IO-5 order → Tasks IO-1..IO-5.