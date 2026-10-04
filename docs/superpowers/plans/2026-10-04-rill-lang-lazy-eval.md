# rill-lang: Lazy Evaluation Core (Stage A) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make rill-lang's value track fully lazy (STG-style call-by-need thunks) behind a `strict`/`complete` mode switch, enabling recursion and infinite structures, with `strict` mode byte-identical to today.

**Architecture:** `Mode` gates lowering (strict = current eager `ValueInstr` stream; complete = thunk-emitting) and the recursion check. `Value::Thunk { fragment, env, cell }` + `Value::ThunkCell(ThunkState)` provide memoized suspension in the arena (cell rc=1, so forcing mutates in place). The interpreter's `force()` drives demand; re-entering a `Forcing` cell is a black hole ⇒ `Value::Bottom`. See `docs/superpowers/specs/2026-10-04-rill-lang-lazy-eval-design.md`.

**Tech Stack:** Rust, crate `rill-lang`. Branch `feature/rill-lang-arrays` (SP-4 already landed). No new external dependencies.

---

## Conventions

- Verify after every task: `cargo test -p rill-lang`; `cargo check --workspace`; before finishing `cargo clippy --all-features --workspace` + `cargo fmt`.
- There is a pre-commit hook running `cargo fmt` — run `cargo fmt` before every commit.
- Commit style: conventional commits, single-quoted `-m`.

---

## Task A1: `Mode` type + pragma parsing

**Files:**
- Create: `rill-lang/src/mode.rs`
- Modify: `rill-lang/src/ast.rs` (`Program` gains `mode: Mode`)
- Modify: `rill-lang/src/lexer.rs` (emit a pragma token, or skip pragmas in the lexer and let the parser scan leading comments)
- Modify: `rill-lang/src/parser.rs` (`parse_program` reads a leading `{-# MODE ... #-}`)
- Modify: `rill-lang/src/lib.rs` (declare `pub mod mode;`)
- Modify: `rill-lang/src/render.rs` (round-trip the pragma), `rill-lang/src/error.rs` (bad-pragma message)

- [ ] **Step 1: Write the failing tests**

Create `rill-lang/src/mode.rs` with a test module; add parsing tests to `parser.rs`'s `mod tests`:

```rust
// mode.rs
//! Evaluation mode: the strict/complete contract.

/// Evaluation contract selected per program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Current behavior: eager value track, recursion banned.
    Strict,
    /// Lazy (call-by-need) value track, Haskell-style recursion.
    Complete,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Strict
    }
}
```

In `parser.rs` tests:

```rust
#[test]
fn pragma_complete_parses() {
    let src = "{-# MODE complete #-}\nmain = _";
    let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
    assert_eq!(p.mode, crate::mode::Mode::Complete);
}

#[test]
fn pragma_strict_is_default() {
    let src = "main = _";
    let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
    assert_eq!(p.mode, crate::mode::Mode::Strict);
}

#[test]
fn pragma_overrides_previous() {
    let src = "{-# MODE complete #-}\n{-# MODE strict #-}\nmain = _";
    let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
    assert_eq!(p.mode, crate::mode::Mode::Strict);
}
```

In `error.rs` (or `mode.rs` tests), a bad pragma must error. Add to `parser.rs` tests:

```rust
#[test]
fn bad_pragma_errors() {
    let src = "{-# MODE nope #-}\nmain = _";
    assert!(parse(&tokenize(src).unwrap(), src.as_bytes()).is_err());
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang pragma_`
Expected: FAIL — `Mode`/`Program::mode` undefined.

- [ ] **Step 3: Add `Mode` and wire the pragma**

1. `rill-lang/src/mode.rs` as above (with `#[cfg(test)] mod tests` asserting `Mode::default() == Mode::Strict`).
2. `ast.rs`: `use crate::mode::Mode;` and add `pub mode: Mode,` to `struct Program`. Fix all `Program` construction sites (compiler will list them: `parser.rs`, `reduce.rs`, `serde_def.rs`, tests).
3. `lexer.rs`: add a `Pragma { name: String, value: String }` token kind emitted for `{-# ... #-}`; `tokenize` recognizes `{-#` ... `#-}` as one token (split on whitespace inside).
4. `parser.rs`: `parse_program` consumes any leading `Token::Pragma` and maps `name == "MODE"` to the `Mode` via `value` (`"strict"` | `"complete"`); any other value → `CompileError`. Multiple pragmas: last one wins (test `pragma_overrides_previous`). No pragma → `Mode::default()`.
5. `render.rs`: emit `{-# MODE strict|complete #-}\n` first when `mode != Strict` (round-trip).
6. `lib.rs`: `pub mod mode;`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p rill-lang pragma_`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add rill-lang/src/mode.rs rill-lang/src/ast.rs rill-lang/src/lexer.rs rill-lang/src/parser.rs rill-lang/src/render.rs rill-lang/src/lib.rs
git commit -m 'feat(rill-lang): Mode type + {-# MODE strict|complete #-} pragma parsing'
```

## Task A2: Thread `Mode` through the compile pipeline

**Files:**
- Modify: `rill-lang/src/types/infer.rs` (`infer_program`/`infer_program_with` take `Mode`; `TypedProgram` carries it)
- Modify: `rill-lang/src/lib.rs` (compile entry points + `compile_with_mode` variants)
- Modify: `rill-lang/src/types/mod.rs` (TypedProgram def), `rill-lang/src/reduce.rs` (accept mode), `rill-lang/src/lower.rs` (`lower_*` take mode)

- [ ] **Step 1: Write the failing test**

In `rill-lang/src/lib.rs` tests:

```rust
#[test]
fn complete_mode_is_carried_to_typed_program() {
    let tokens = lexer::tokenize("{-# MODE complete #-}\nmain = _").unwrap();
    let program = parser::parse(&tokens, "{-# MODE complete #-}\nmain = _".as_bytes()).unwrap();
    let typed = types::infer::infer_program(&program, crate::mode::Mode::Complete).unwrap();
    assert_eq!(typed.mode, crate::mode::Mode::Complete);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rill-lang complete_mode_is_carried`
Expected: FAIL — `infer_program` arity / no `typed.mode`.

- [ ] **Step 3: Thread the mode**

1. `TypedProgram` (in `types/mod.rs`) gains `pub mode: Mode`.
2. `infer_program(program)` → `infer_program(program, mode: Mode)`; `infer_program_with` likewise; both set `typed.mode = mode`. Update the two call sites in `lib.rs` and any tests.
3. `reduce::reduce_with_cafs(&typed.program, &typed.cafs)` — pass `typed.mode` if reduce needs it (recursion-aware CAF extraction, Task A7; for now accept-and-ignore via `reduce_with_cafs_mode`).
4. `lower::lower_with_cafs(&typed, sr, &typed.cafs)` → add `lower_with_cafs_mode(&typed, sr, &typed.cafs, typed.mode)`.
5. `lib.rs`: every `compile*` gains a mode:
   - `compile::<T>(src)` → strict (calls `compile_with_mode::<T>(src, Mode::Strict)`).
   - `compile_with_mode::<T>(src, mode)` — new public fn: parse, **override `program.mode = mode` only if the file has no pragma** (i.e., `program.mode == Mode::Strict` AND a pragma was present should win — simplest: the parser sets `program.mode` from the pragma; the API key is the default when no pragma exists. Implement by adding `has_pragma: bool` to `Program`, or: API key applies only when the parsed `program.mode == Mode::Strict` and the source contains no `{-# MODE` — recommended: track `mode_explicit` on `Program`).
   - `compile_with`, `compile_with_ffi`, `compile_graph`, `compile_program*` — add `_with_mode` variants (strict default unchanged).
6. `Program` gains `pub mode_explicit: bool` (set true when a pragma was parsed) so the API-key override logic is exact.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p rill-lang`
Expected: PASS (existing tests unaffected; new test passes).

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add rill-lang/src/types/infer.rs rill-lang/src/types/mod.rs rill-lang/src/reduce.rs rill-lang/src/lower.rs rill-lang/src/lib.rs rill-lang/src/ast.rs
git commit -m 'feat(rill-lang): thread Mode through compile pipeline (compile_with_mode; pragma overrides API)'
```

## Task A3: Gate recursion on `Mode::Strict`

**Files:**
- Modify: `rill-lang/src/types/infer.rs:1052` (`check_recursion` call)

- [ ] **Step 1: Write the failing tests**

Create `rill-lang/tests/lazy_mode.rs`:

```rust
use rill_lang::{compile, compile_with_mode};
use rill_lang::mode::Mode;

#[test]
fn recursion_rejected_in_strict() {
    assert!(compile::<f32>("a = a; main = a").is_err());
}

#[test]
fn recursion_accepted_in_complete() {
    // Compiles; running `main = a` where `a = a` yields bottom at runtime
    // (tested in Task A6) — here we only require it to COMPILE.
    let r = compile_with_mode::<f32>("a = a; main = a", Mode::Complete);
    assert!(r.is_ok(), "complete mode must accept recursion");
}
```

- [ ] **Step 2: Run to verify the second fails**

Run: `cargo test -p rill-lang --test lazy_mode`
Expected: `recursion_rejected_in_strict` PASS; `recursion_accepted_in_complete` FAIL (CompileError).

- [ ] **Step 3: Gate the check**

In `infer.rs:1052`:

```rust
if typed.mode == Mode::Strict {
    check_recursion(&program.defs)?;
}
```

(`typed.mode` is set from `program.mode` — see Task A2. If the pass runs before `typed` is built, use the `mode` parameter.)

Keep `env.check_acyclic()` (recursive *data types*) enforced in both modes for now — the `subtree_size` arena budget assumes acyclic value types (noted as a follow-up for when recursive data types are wanted).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p rill-lang --test lazy_mode` and `cargo test -p rill-lang`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add rill-lang/src/types/infer.rs rill-lang/tests/lazy_mode.rs
git commit -m 'feat(rill-lang): allow recursion in complete mode (check_recursion gated on Strict)'
```

## Task A4: Value model — `Thunk`, `ThunkCell`, `Bottom`

**Files:**
- Modify: `rill-lang/src/arena.rs`

- [ ] **Step 1: Write the failing tests**

In `arena.rs` `mod tests`, extend the `Node` payload with a thunk-like shape is overkill — add a direct test of the new variants via `Value`:

```rust
#[test]
fn thunk_visits_env_and_cell() {
    let mut a = Arena::with_capacity(16);
    let env = a.alloc(Value::Record(vec![])).unwrap();
    let cell = a.alloc(Value::ThunkCell(ThunkState::Unforced)).unwrap();
    let t = a.alloc(Value::Thunk { fragment: 0, env, cell }).unwrap();
    a.copy(cell).unwrap(); // extra ref on the cell
    a.drop_ref(t);
    // env freed (only the thunk owned it), cell survives via the extra ref.
    assert_eq!(a.rc(env), 0);
    assert_eq!(a.rc(cell), 1);
}

#[test]
fn thunk_cell_forced_keeps_result_alive() {
    let mut a = Arena::with_capacity(16);
    let res = a.alloc(Value::Int(7)).unwrap();
    let cell = a.alloc(Value::ThunkCell(ThunkState::Forced(res))).unwrap();
    a.copy(res).unwrap();
    a.drop_ref(cell);
    assert_eq!(a.rc(res), 1, "forced result survives via the copy");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang arena::tests::thunk`
Expected: FAIL — variants undefined.

- [ ] **Step 3: Implement the value model**

In `arena.rs`:

```rust
/// Call-by-need state of a thunk cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThunkState {
    /// Not yet forced.
    Unforced,
    /// Being forced right now (black hole: re-entry ⇒ bottom).
    Forcing,
    /// Forced to WHNF; the memoized result ref.
    Forced(ArenaRef),
    /// Divergent; the value is bottom.
    Bottom,
}

// in `enum Value`:
/// Suspended computation: body fragment + captured env + memo cell.
Thunk {
    /// Body fragment to run on forcing.
    fragment: u32,
    /// Captured environment record.
    env: ArenaRef,
    /// Memo/black-hole cell (ref to `Value::ThunkCell`).
    cell: ArenaRef,
},
/// The force-state cell behind a thunk (rc 1; mutated by forcing).
ThunkCell(ThunkState),
/// Divergent computation (bottom); propagates through strict ops.
Bottom,
```

- Add `ValueKind::Thunk`, `ValueKind::Bottom` (+ `kind()` arms).
- `impl ArenaValue for Value`:
  - `visit_refs`: `Thunk { env, cell, .. } => { f(*env); f(*cell); }`; `ThunkCell(Forced(r)) => f(*r)`; `ThunkCell(_) | Bottom => {}`.
  - `clone_pooled` / `detach_payload`: default (plain clone / None) for the new variants.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p rill-lang arena::tests::thunk` then full `cargo test -p rill-lang`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add rill-lang/src/arena.rs
git commit -m 'feat(rill-lang): Value::Thunk/ThunkCell/Bottom value model with RC-aware visits'
```

## Task A5: IR — `ValueLazy` / `ValueForce`

**Files:**
- Modify: `rill-lang/src/ir.rs` (new `ValueInstr` variants + `Ir` layout notes)

- [ ] **Step 1: Write the failing tests**

Add to `ir.rs` tests (or a unit test asserting the variants serialize/lower):

```rust
#[test]
fn lazy_and_force_instructions_exist() {
    let _ = ValueInstr::ValueLazy { dst: 0, env: 1, fragment: 2 };
    let _ = ValueInstr::ValueForce { dst: 0, src: 1 };
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rill-lang ir::tests::lazy_and_force`
Expected: FAIL — variants undefined.

- [ ] **Step 3: Add the variants**

In `ir.rs`, `enum ValueInstr`:

```rust
/// Create a lazy binding: allocate a `Value::Thunk` + `Value::ThunkCell(Unforced)`.
ValueLazy {
    /// Destination value register.
    dst: usize,
    /// Captured environment record register (arena ref).
    env: usize,
    /// Index into `Ir::fragments` (the suspended body).
    fragment: usize,
},
/// Force a value to WHNF, result into `dst`.
ValueForce {
    /// Destination value register.
    dst: usize,
    /// Source value register.
    src: usize,
},
```

Update every exhaustive `match` over `ValueInstr` the compiler reports (interpreter in Task A6 can `unimplemented!` temporarily; the lowering doesn't emit them yet).

- [ ] **Step 4: Run the tests**

Run: `cargo check -p rill-lang` (fix matches) then `cargo test -p rill-lang ir::tests::lazy_and_force`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add rill-lang/src/ir.rs
git commit -m 'feat(rill-lang): ValueLazy/ValueForce IR instructions'
```

## Task A6: Interpreter — `force()`, black hole, bottom propagation

**Files:**
- Modify: `rill-lang/src/backend/interp.rs`
- Modify: `rill-lang/src/program.rs` (expose nothing new; `value_error` reuse)

- [ ] **Step 1: Write the failing tests**

In `tests/lazy_mode.rs`:

```rust
#[test]
fn complete_infinite_list_take() {
    // `from n = n : from (n+1)` + `take k xs` — build minimal combinators in
    // the DSL and pull 5 values in ONE tick (records/values, not signals).
    let src = r#"
{-# MODE complete #-}
from n = n : from (n + 1)
take 0 xs = []
take k (x : xs) = x : take (k - 1) xs
main = take 5 (from 0)
"#;
    let mut prog = compile_with_mode::<f32>(src, Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    prog.process(Some(&[0.0]), &mut out).unwrap();
    let vals = prog.value_outputs();
    let v = vals[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::List { elems } => assert_eq!(elems.len(), 5),
        other => panic!("expected a List, got {other:?}"),
    }
}

#[test]
fn complete_self_recursion_is_bottom() {
    let src = "{-# MODE complete #-}\na = a\nmain = a";
    let mut prog = compile_with_mode::<f32>(src, Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = prog.process(Some(&[0.0]), &mut out);
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Bottom,
        "a = a must force to bottom, not hang"
    );
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --test lazy_mode complete_`
Expected: FAIL — the interpreter does not know `ValueLazy`/`ValueForce` yet and lowering emits none, so `from`/`take`/`a=a` will fail earlier (recursion compiles in complete; lowering of `:` and recursion is Task A7/A8 — so these tests may fail as CompileError until then; that is acceptable: they pin the END state).

- [ ] **Step 3: Implement `force()` and strict-op forcing**

In `interp.rs`:

```rust
/// Force `r` to WHNF and return the head ref. Idempotent via the memo cell.
pub(crate) fn force<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    r: ArenaRef,
) -> ArenaRef {
    loop {
        let v = match prog.arena.get(r) {
            Some(v) => v,
            None => return r, // dangling: caller's bug; treat as-is
        };
        let Value::Thunk { fragment, env, cell } = v else {
            return r; // already WHNF
        };
        let (fragment, env, cell) = (*fragment, *env, *cell);
        match prog.arena.get(cell) {
            Some(Value::ThunkCell(ThunkState::Forced(res))) => {
                r = *res;
                continue;
            }
            Some(Value::ThunkCell(ThunkState::Bottom)) => return r,
            Some(Value::ThunkCell(ThunkState::Unforced)) => {
                // Mark the cell Forcing (black hole) before dispatching.
                *prog.arena.get_mut(cell).expect("cell rc 1") =
                    Value::ThunkCell(ThunkState::Forcing);
                // Run the fragment; it writes its result into `r_holder`'s
                // slot. The fragment's calling convention binds `env` as its
                // captured cells and returns one result ref.
                let result = run_fragment_owned(prog, fragment, env);
                *prog.arena.get_mut(cell).expect("cell rc 1") =
                    Value::ThunkCell(ThunkState::Forced(result));
                r = result;
                continue;
            }
            Some(Value::ThunkCell(ThunkState::Forcing)) => {
                // Black hole re-entry: divergent.
                *prog.arena.get_mut(cell).expect("cell rc 1") =
                    Value::ThunkCell(ThunkState::Bottom);
                return r;
            }
            _ => return r,
        }
    }
}
```

Where `run_fragment_owned(prog, fragment, env)` is a new helper that invokes the fragment body exactly like `run_fragment` for a closure but with the env record given directly, returning the result ref (the fragment's `ValueReturn`-equivalent must write into a fresh slot; see Task A8 for the fragment ABI). The cell's rc is 1 by construction (`ValueLazy` allocates the cell once; only the thunk references it), so `get_mut` is sound.

- [ ] **Step 4: Make strict instructions force**

In the `ValueInstr` match inside `run_value_track`, wrap operand reads with `force` for: `ValueAdd/Sub/Mul/Div` (both operands), `ValueCompare`, `ValueLogic`, `ValueNot`, `ValueMatch` (scrutinee), `ValueProject`, `ValueUnwrap`, `ValueCallBuiltin` (collection args), and the tick-end output copy. `ValueCallFunc` leaves args unforced.

Add `ValueInstr::ValueForce { dst, src }` → `*reg(dst) = force(prog, reg(src))`. Add `ValueInstr::ValueLazy { dst, env, fragment }` → alloc a `Value::Thunk` + a fresh `Value::ThunkCell(Unforced)` cell, store the thunk ref into `reg(dst)`.

- [ ] **Step 5: Bottom propagation**

Any strict op whose forced operand is `Value::Bottom` writes `Value::Bottom` to its dst. `ValueMatch` on a `Bottom` scrutinee writes `Bottom` to all arm dsts and does not advance. `ValueProject`/`ValueUnwrap` on `Bottom` → `Bottom`.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p rill-lang --test lazy_mode complete_`
Expected: `complete_self_recursion_is_bottom` PASS once lowering emits a thunk for `a = a` (Task A8); `complete_infinite_list_take` needs `:` recursion lowering (Task A7/A8). If they fail on CompileError at this point, proceed to Task A7/A8 and re-run.

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add rill-lang/src/backend/interp.rs rill-lang/tests/lazy_mode.rs
git commit -m 'feat(rill-lang): demand-driven force() with black-hole bottom; strict ops force operands'
```

## Task A7: Recursion lowering + cyclic CAF extraction

**Files:**
- Modify: `rill-lang/src/reduce.rs` (`reduce_with_cafs` tolerates cycles in complete mode)
- Modify: `rill-lang/src/lower.rs` (recursive def → self-referential thunk)

- [ ] **Step 1: Write the failing test**

In `tests/lazy_mode.rs`:

```rust
#[test]
fn complete_fibs_lazily_defines_infinite_list() {
    let src = r#"
{-# MODE complete #-}
fibs = 0 : 1 : zipWith (+) fibs (tail fibs)
take 0 xs = []
take k (x : xs) = x : take (k - 1) xs
tail (x : xs) = xs
zipWith f (a : as) (b : bs) = f a b : zipWith f as bs
main = take 10 fibs
"#;
    let mut prog = compile_with_mode::<f32>(src, Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    prog.process(Some(&[0.0]), &mut out).unwrap();
    let vals = prog.value_outputs();
    let v = vals[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::List { elems } => assert_eq!(elems.len(), 10),
        other => panic!("expected a List, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rill-lang --test lazy_mode complete_fibs`
Expected: FAIL — cyclic defs not lowered.

- [ ] **Step 3: Cyclic CAF extraction**

In `reduce.rs`, the CAF pass (`reduce_with_cafs`) currently assumes acyclic definition order. In complete mode (`reduce_with_cafs_mode`), when a def references a later/earlier def forming a cycle, do **not** inline the callee; leave the reference as a `Ref(name)` that lowering resolves to the callee's own thunk (a cyclic env capture). Concretely: skip inlining for defs on a cycle; the recursive def's body fragment captures the def's own thunk ref via `ValueMakeClosure`-style env.

- [ ] **Step 4: Self-referential thunk lowering**

In `lower.rs`, for a top-level recursive def `name = body` in complete mode: allocate the def's thunk first (`ValueLazy` with a placeholder env), then build the body fragment whose env record contains the def's own thunk ref (so recursion resolves lazily). This requires a two-phase env allocation: allocate the thunk cell + thunk value, then construct the env record that references it. The reference resolution for `Ref(name)` inside the body points at the def's thunk ref.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p rill-lang --test lazy_mode complete_fibs complete_infinite_list_take complete_self_recursion_is_bottom`
Expected: PASS (all three), or iterate on Steps 3-4 until they do.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add rill-lang/src/reduce.rs rill-lang/src/lower.rs rill-lang/tests/lazy_mode.rs
git commit -m 'feat(rill-lang): cyclic CAF extraction + self-referential thunk lowering for complete mode'
```

## Task A8: Complete-mode lowering — thunk-emitting bindings + budget

**Files:**
- Modify: `rill-lang/src/lower.rs` (value bindings emit `ValueLazy` in complete mode; strict contexts emit `ValueForce`)
- Modify: `rill-lang/src/ir.rs` (`ValueLayout`/`subtree_size` budget)

- [ ] **Step 1: Write the failing test**

In `tests/lazy_mode.rs`:

```rust
#[test]
fn complete_repeat_and_cycle_are_lazy() {
    let src = r#"
{-# MODE complete #-}
repeat x = x : repeat x
cycle xs = xs ++ cycle xs   -- shorthand; implement ++ if missing
take 0 xs = []
take k (x : xs) = x : take (k - 1) xs
main = take 3 (repeat 42)
"#;
    let mut prog = compile_with_mode::<f32>(src, Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    prog.process(Some(&[0.0]), &mut out).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::List { elems } => assert_eq!(elems.len(), 3),
        other => panic!("expected a List, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rill-lang --test lazy_mode complete_repeat`
Expected: FAIL — `repeat x = x : repeat x` does not suspend (either eager `repeat` recurses to bottom, or lowering inlines it away).

- [ ] **Step 3: Mode-gated lowering**

In `lower.rs`, gate on `mode`:
- `complete`: value-binding sites (lambda arguments, `let`/`where` locals that are used lazily) emit `ValueLazy` (thunk) instead of eager computation; strict contexts (arithmetic/comparison/match/project/output) emit `ValueForce` where the operand is not guaranteed WHNF. This is the STG-style "thunks + strict points" lowering.
- `strict`: emit exactly today's instructions (no `ValueLazy`, no `ValueForce`) — the pipeline must be byte-identical.

**Practical rule (v1 of laziness):** in `complete` mode, function APPLICATION arguments are wrapped in thunks (`ValueLazy`), and the fragment body's strict ops force them. Record/Sum/List constructors keep lazy fields (fields are thunks). This is enough for `from`/`repeat`/`fibs`/`take`/`zipWith` to be productive.

- [ ] **Step 4: Budget**

In `lower.rs` `subtree_size`/`ValueLayout` construction: in complete mode, multiply the value-track `capacity` and `buffer_budget` by a conservative factor (e.g., 2×) to cover thunk cells + forced results. Exhaustion remains a latched `ProcessError` (RT-safe).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p rill-lang --test lazy_mode`
Expected: PASS (all complete-mode tests); `cargo test -p rill-lang` (strict regression) PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add rill-lang/src/lower.rs rill-lang/src/ir.rs rill-lang/tests/lazy_mode.rs
git commit -m 'feat(rill-lang): complete-mode thunk-emitting lowering + conservative value budget'
```

## Task A9: Semantics tests, stream hook, verification, docs

**Files:**
- Modify: `rill-lang/tests/lazy_mode.rs`
- Modify: `docs/src/guides/rill-lang.md`, `CHANGELOG.md`

- [ ] **Step 1: Write the remaining tests**

Add to `tests/lazy_mode.rs`:

```rust
#[test]
fn complete_iterate_and_take() { /* iterate f x = x : iterate f (f x); main = take 4 (iterate (+1) 0) → [0,1,2,3] */ }

#[test]
fn complete_mutual_recursion() { /* evens/odds interleave; main = take 5 evens → [0,2,4,6,8] */ }

#[test]
fn complete_bottom_propagates() { /* main = (1 + a) where a = a → Bottom, not a hang */ }

#[test]
fn complete_shared_thunk_forced_once() { /* x = expensive; main = (head x, head x) — memoization observable via correctness */ }

#[test]
fn complete_stream_cursor_across_ticks() {
    // Minimal: a value_state-held thunk advances one cons-cell per tick.
    // See notes: this is the Stage C hook; here assert a lazy list value can
    // live in value_state and be re-forced next tick to the SAME head
    // (call-by-need memoization across ticks).
}

#[test]
fn strict_mode_is_unchanged_regression() { /* a representative existing data program still runs */ }
```

- [ ] **Step 2: Run all tests**

Run: `cargo test --workspace`, `cargo clippy --all-features --workspace`, `cargo fmt`
Expected: all green; 0 warnings.

- [ ] **Step 3: Docs + CHANGELOG**

`docs/src/guides/rill-lang.md`: new section "Evaluation modes — `strict` / `complete`" (pragma, API, laziness, bottom, recursion). `CHANGELOG.md`: Stage A entry under `[Unreleased]`/`rill-lang`.

- [ ] **Step 4: Commit**

```bash
cargo fmt
git add rill-lang/tests/lazy_mode.rs docs/src/guides/rill-lang.md CHANGELOG.md
git commit -m 'docs(rill-lang): Stage A lazy evaluation — complete-mode tests, guide, CHANGELOG'
```

---

## Self-review checklist

- [ ] §2 mode switch (pragma + API, pragma overrides) → Tasks A1, A2.
- [ ] §3 value model (Thunk/ThunkCell rc-1 memo, visits) → Task A4.
- [ ] §4 IR (ValueLazy/ValueForce; strict ops force) → Task A5, A6.
- [ ] §5 interpreter (force loop, black hole, nested via run_fragment) → Task A6.
- [ ] §6 bottom (propagation, observability) → Task A6 (steps 5).
- [ ] §7 recursion (Haskell-style; gate on Strict; cyclic CAFs; no termination check) → Tasks A3, A7.
- [ ] §8 entry shape unchanged; streams hook → Task A9 (`stream_cursor` test documents the hook).
- [ ] §9 JIT-readiness (linear IR preserved; regalloc follow-up noted) → constraint honored by Tasks A5-A8.
- [ ] §10 backward compat (strict regression) + budget → Tasks A8 (step 4), A9 (step 2).