# rill-lang: Lazy Evaluation Core (STG-style) — Design (Stage A)

> **Status:** Implemented (Stage A). Audited 2026-10-19: `strict`/`complete`
> modes (`mode.rs`), the unified `Value::Callable` closure/thunk with
> `Value::ThunkCell(ThunkState)` black-hole, `Value::Bottom`, and `force` in the
> interpreter are all in the code.
> **Date:** 2026-10-04 (audited 2026-10-19)
> **Branch:** `feature/rill-lang-arrays`
> **Scope:** Stage A of the rill-lang evaluation-model rework: a **fully lazy
> call-by-need** value track (STG-style thunks in the arena) behind a
> `strict`/`complete` mode switch, enabling recursion and infinite structures.
> The signal (block) track is untouched; program entry stays the current
> `arrow : inputs -> outputs` shape the backend drives per tick.

---

## 0. Executive summary

rill-lang evaluates on two tracks: the **block track** (`Rate::Signal`,
whole-buffer instructions over `FixedBuffer` registers) and the **value track**
(`Rate::Value`, arena values + `ValueInstr` once per tick). The value track is
currently eager: a linear `ValueInstr` walker materializes values in order,
recursion is rejected at compile time, and only finite data (records,
collections, closures) exists.

Stage A makes the value track **fully lazy (call-by-need)** in the style of
GHC's STG machine: expressions become **thunks** (suspended fragments with a
memo cell) forced to WHNF on demand; recursion becomes expressible; divergent
computations become the value `⊥` (bottom) via black-hole detection. A mode
switch selects the evaluation contract:

| Mode | Recursion | Laziness | Entry shape |
|---|---|---|---|
| `strict` (default) | banned (current) | eager (current) | `arrow : in -> out` |
| `complete` | allowed (Haskell-style) | call-by-need | `arrow : in -> out` |

**The design keeps the current program representation.** The backend still
calls a per-tick arrow `inputs -> outputs`. The long-term direction is a
separate stage: the program becomes `(arg..) -> IO` with the backend as a
stream *inside* the program; Stage A's lazy core is the foundation for that
move but does not change the entry shape.

**JIT readiness (design constraint).** The IR stays linear; each fragment is a
future native function (entry point), forcing is a call, memoization is a
write. This is the STG model — *not* a graph-reduction engine (dynamic redex
selection does not map to native code and would put lazy machinery on the hot
DSP path). See §9.

---

## 1. Context and motivation

### 1.1 The current value track

- `ValueInstr` (linear, eager) over per-tick `value_regs`; values are arena
  `Value`s (Int/Float/Bool/String/Record/Sum/Newtype/Closure/List/Map/Set/Void)
  with RC + COW (`Arena`, `rill_core::memory` since SP-4).
- `ValueCallFunc` dispatches a `FragmentIr` via `run_fragment` (nested call
  scratch `value_regs_top`); `ValueMakeClosure` builds `Value::Closure {
  env_record, fragment }` with captured environment.
- `value_state` persists values across ticks (feedback/delay of values).
- Recursion is rejected at compile time (`a = a` is a graceful `CompileError`).

### 1.2 Deferred plans this stage serves

- SP-3 (signal-buffer design) deferred the "big objects" subsystem, cons-list
  refactor, and streams; SP-4 §7 designed `RegionHandle` as the future memory
  of `Array`/`Vector` big objects.
- **Long-term (after this stage):** program shape evolves from the arrow
  `inputs -> outputs` (backend calls each step) to `(arg..) -> IO`, where the
  backend is a stream inside the program. Stage A's lazy value track is the
  prerequisite: infinite structures, streams, and recursion only make sense
  under laziness.

### 1.3 Why STG, not graph reduction

Graph reduction's benefits (sharing, laziness) are achievable with **memoized
thunks over a linear IR** — exactly GHC's STG machine (closures with compiled
entry points, update frames). A dynamic graph-reduction engine would not map
to native code, would put lazy machinery on the strict DSP hot path, and is a
rewrite for no semantic gain. The linear IR + thunks model is both
incremental and JIT-friendly (§9).

---

## 2. Mode switch

### 2.1 Syntax and API

- **File pragma** (lexer/parser): `{-# MODE strict #-}` / `{-# MODE complete #-}`
  at the top of the source, before any definitions. Parsed into the AST as a
  program-level attribute.
- **API key** (default `strict`):
  - `compile::<T>(src)` — strict (unchanged).
  - `compile_with_mode::<T>(src, Mode)` — explicit.
  - `compile_with`/`compile_with_ffi`/`compile_graph` gain `_with_mode`
    variants (or a `Mode` parameter). The file pragma overrides the API key.
- `enum Mode { Strict, Complete }` — thread through parser → typed → reduce →
  lower. In `strict` mode the pipeline is byte-identical in behavior to today.

### 2.2 What the mode gates

| Concern | `strict` | `complete` |
|---|---|---|
| Cyclic definitions | rejected (current) | allowed (Haskell-style: all top-level defs recursive, mutual recursion, `let`/`where` recursive) |
| Value lowering | eager (current IR) | thunk-emitting (`ValueLazy`) + strict ops force |
| `force()` behavior | values already WHNF → trivial | demand-driven |

The same interpreter executes both modes: `strict` lowering never creates
thunks, so forcing is a cheap no-op.

---

## 3. Value model: Thunk + ThunkCell

```rust
// rill-lang/src/arena.rs (Value enum additions)

/// Suspended computation: a body fragment + captured env + memo cell.
Value::Thunk {
    /// Body fragment to run on forcing.
    fragment: u32,
    /// Captured environment record (arena ref).
    env: ArenaRef,
    /// Memo/black-hole cell (arena ref to `Value::ThunkCell`).
    cell: ArenaRef,
},

/// The force-state cell behind a thunk. rc == 1 (only its thunk references
/// it), so forcing mutates it in place — shared thunk copies all see the memo.
Value::ThunkCell(ThunkState),
```

```rust
/// Call-by-need state of a thunk cell.
pub enum ThunkState {
    /// Not yet forced.
    Unforced,
    /// Being forced right now (black hole: re-entry ⇒ ⊥).
    Forcing,
    /// Forced to WHNF; the memoized result ref.
    Forced(ArenaRef),
    /// Divergent; the value is bottom.
    Bottom,
}
```

- `visit_refs` for `Value::Thunk` visits `env` and `cell`; for
  `Value::ThunkCell(Forced(r))` visits `r` — so a forced result stays alive as
  long as any thunk referencing the cell lives. `detach_payload` returns
  `None` for both.
- The cell is a **single arena slot with rc 1**: created at `ValueLazy`,
  referenced only by the thunk value that owns it. Because the thunk value may
  be RC-shared (many references to the same suspended computation), the memo
  must live in the cell, not inline in the thunk value (inline state could not
  be mutated without exclusive access). This is the STG update-frame analog.
- COW interplay: `Value::Thunk` is a leaf-ish container (env + cell refs); COW
  recounting works through `visit_refs`.

---

## 4. IR additions (`ir.rs`)

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

/// Force a value to WHNF, result into `dst` (used by strict contexts).
ValueForce {
    /// Destination value register.
    dst: usize,
    /// Source value register.
    src: usize,
},
```

**Strict contexts force implicitly** (the interpreter forces operands before
reading payloads): `ValueAdd/Sub/Mul/Div`, `ValueCompare`, `ValueLogic`,
`ValueNot`, `ValueMatch` (scrutinee), `ValueProject`/`ValueUnwrap` (record /
newtype), `ValueCallBuiltin` (collection args), and the value-output copy at
tick end. `ValueCallFunc` passes arguments **unforced** (lazy application); the
callee fragment forces what its strict instructions need.

`ValueMakeClosure` stays for first-class functions. In `complete` mode,
function application of a user function to an argument evaluates the argument
lazily (the arg register holds a thunk). In `strict` mode, `ValueLazy` is never
emitted (eager bindings) and `ValueForce` is a no-op — the current instruction
stream is produced unchanged.

---

## 5. Interpreter: `force()` and black holes

`run_value_track` gains a helper:

```rust
/// Force `r` to WHNF and return the head ref. Idempotent via the memo cell.
fn force(&mut self, r: ArenaRef) -> ArenaRef {
    // loop:
    //   match arena.get(r) {
    //     Value::Thunk { fragment, env, cell } => match cell.state {
    //       Unforced => { cell = Forcing;
    //                     run_fragment(fragment, env, /* write result slot */);
    //                     cell = Forced(result); r = result; }
    //       Forcing  => { cell = Bottom; return Bottom; }   // black hole ⇒ ⊥
    //       Forced(res) => r = res,
    //       Bottom    => return Bottom,
    //     },
    //     _ => return r,                                    // already WHNF
    //   }
}
```

- **No explicit stack needed**: the cell's `Forcing` state *is* the re-entry
  marker (GHC blackhole). Nested forcing of distinct thunks recurses through
  `run_fragment`, which already supports nested calls via the pre-allocated
  call scratch (`value_regs_top`).
- `force` allocates the forced result into the arena and writes it into the
  cell (`get_mut` valid because the cell has rc 1). Repeated forcing returns
  the memoized ref — shared thunks compute once.
- Strict instructions call `force` on their operands first. Since `strict`
  mode emits no thunks, `force` returns immediately in that mode.
- **Divergence guard**: re-entering a `Forcing` cell converts it to `Bottom`
  and returns bottom. A `bottom` value propagates through strict ops (§6).

---

## 6. Bottom (⊥) as a value

- `Value::Bottom` is a member of the value model.
- **Propagation**: arithmetic / comparison / logic / match / project on a
  `Bottom` operand yields `Bottom` (the operand forces to bottom and the op
  returns bottom), like `undefined` in Haskell. A `Bottom` scrutinee in
  `ValueMatch` yields bottom for the matched arm.
- **Observability**: a tick output may be `Bottom`; it is inspectable through
  `prog.arena()` / `prog.value_outputs()`. Programs can match on bottom
  explicitly if desired (bottom is just a constructor).
- `visit_refs`/`detach_payload` treat `Bottom` as a leaf.

---

## 7. Recursion (complete mode)

- **Haskell-style**: all top-level definitions are recursive by default;
  mutual recursion is allowed; `let`/`where` bindings are recursive.
- **Inference**: the cycle-rejection in `types/infer.rs` (and any
  recursion-check in `reduce`) is gated on `Mode::Strict`; in `complete` mode
  cyclic reference graphs are accepted.
- **Lowering**: recursive definitions become self-referential thunks. The CAF
  extraction (`reduce_with_cafs`) must tolerate cycles: a recursive def
  lowers to `ValueLazy` whose body fragment references the def's own thunk
  (captured in the env record, resolved lazily on forcing). Productivity is
  guarded at runtime by the black-hole mechanism (§5) — a non-productive
  recursion becomes `⊥`, not a hang.
- **Safety**: `a = a` in complete mode ⇒ forcing `a` enters its own `Forcing`
  cell ⇒ `⊥`. The RT path never spins; divergence is a value.
- **Termination is not checked** (Rice): `complete` mode accepts all recursion;
  non-termination surfaces as bottom. This is the Turing-completeness contract.

---

## 8. Streams and the long-term program shape

- **Stage A hook**: infinite structures are ordinary lazy values —
  `from n = n : from (n+1)`, `repeat x`, `cycle xs`, `iterate f x`, `fibs`,
  `take n`, `zipWith` — all expressible once recursion + thunks exist. The
  per-tick **advance** of a stream cursor (force WHNF → `Cons head tail`,
  store `tail` in `value_state`) is a Stage C concern; Stage A makes the
  values themselves representable.
- **Entry shape**: Stage A keeps `main : inputs -> outputs` and the backend's
  per-tick call. **Forward-compat**: the lazy value track is the substrate for
  the future `(arg..) -> IO` form, where the backend becomes a stream inside
  the program (SP-3 §6). Nothing in Stage A precludes that: thunks + streams
  are the vocabulary it needs.
- The `List`/`Array`/`Stream` type work (cons-list refactor, RegionHandle
  arrays, lazy streams) is downstream of Stage A and remains a later stage.

---

## 9. JIT readiness (constraint, not implementation)

The STG choice is deliberate for a future Cranelift backend:

- Each `FragmentIr` compiles to a **native function** with a fixed calling
  convention; `Value::Thunk`/`Value::Closure` carry a fragment id → function
  pointer.
- **Forcing = a native call** to the thunk body; memoization = a single arena
  write after the call returns; black-hole = a state check at the call
  boundary.
- **Strict DSP path compiles clean**: `strict` mode emits no thunks, so the
  whole-buffer block track and eager value arithmetic compile to tight SIMD
  loops with zero lazy overhead.
- Arena/RC/COW operations are runtime-library calls (later: intrinsics).
- Consequence for Stage A: **the IR must stay linear and stable** — no
  graph-reduction dynamicism, no IR-churn that would break a future native
  lowering. Re-activating `regalloc.rs` (currently disabled) is an early
  follow-up that benefits both modes.

---

## 10. Backward compatibility, RT safety, budget

- **Behavior**: `strict` mode is the current pipeline; all existing tests must
  pass unchanged.
- **RT safety**: forcing allocates arena values — bounded by the pre-allocated
  arena capacity. In `complete` mode the build-time capacity heuristic
  (`lower.rs` `subtree_size`) must budget for thunks + forced results; because
  `complete` programs are not the DSP hot path, a conservative budget is
  acceptable (exhaustion remains a latched `ProcessError`, as today).
- **No new allocation on the block track**: the signal path is untouched.
- **Testing**:
  - `strict` mode: full existing rill-lang regression suite.
  - `complete` mode: `from n`/`iterate`/`repeat`/`cycle`/`fibs`/`zipWith`/
    `take` behavior; recursion (`fact`, mutual `evens`/`odds`);
    `a = a` ⇒ bottom; shared thunk memoized once; nested forcing; bottom
    propagation; stream cursor across ticks (minimal).
  - Mode pragma parsing; pragma overrides API key.

---

## 11. File map (Stage A)

| Area | Files |
|---|---|
| Mode type + pragma | `rill-lang/src/{lexer,parser,ast}.rs` (`Mode` on `Program`), `rill-lang/src/lib.rs` (compile entry points), `rill-lang/src/error.rs` (bad-pragma diagnostics) |
| Value model | `rill-lang/src/arena.rs` (`Value::Thunk`, `Value::ThunkCell`, `ThunkState`, `Value::Bottom`; `ArenaValue` visits) |
| IR | `rill-lang/src/ir.rs` (`ValueLazy`, `ValueForce`; fragment env shape) |
| Lowering | `rill-lang/src/lower.rs` (mode-gated: eager vs thunk-emitting; recursion lowering; `subtree_size` budget) |
| Inference / reduce | `rill-lang/src/types/infer.rs`, `rill-lang/src/reduce.rs` (recursion gate; cyclic CAF extraction) |
| Interpreter | `rill-lang/src/backend/interp.rs` (`force`, black hole, strict-op forcing, bottom propagation, output forcing) |
| Program | `rill-lang/src/program.rs` (mode on `RillProgram`, force plumbing) |
| Tests | `rill-lang/tests/*` (complete-mode suite: infinite structures, recursion, bottom; pragma) |
| Docs | `docs/src/guides/rill-lang.md` (mode, laziness, bottom), `CHANGELOG.md` |

---

## Verification

- `cargo test -p rill-lang` (strict regression) after every task; new
  `complete`-mode tests; `cargo test --workspace`, `cargo clippy
  --all-features --workspace`, `cargo fmt` before finishing.
- Zero new external dependencies. No `unsafe` beyond the existing
  `rill_core::memory` policy (this stage is pure safe Rust in rill-lang).
- Branch: `feature/rill-lang-arrays`; conventional commits, single-quoted `-m`.
- Follow-ups (not in this stage): `regalloc.rs` re-activation; cons-list /
  `Array` / `Stream` types; signal/value unification; `(arg..) -> IO` program
  shape; JIT (Cranelift).