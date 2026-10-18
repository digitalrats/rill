# rill-lang: one `run` — unify all execution into a single function (M8-pre)

Status: design (review — builder check passed, calls settled)
Date: 2026-10-18
Branch: `feature/rill-lang-arrays`
Related: `docs/superpowers/specs/2026-10-17-rill-lang-arrays-design.md`,
`docs/superpowers/plans/2026-10-18-rill-lang-arrays.md`.

## Intent

Before the array-hole work touches the interpreter, unify the run forms so that
**every execution physically runs through one function**. This is the load-bearing
precondition for two downstream goals — a generalized trampoline (any call
returns to a single loop; frames live in an explicit structure; the native stack
does not grow with call depth) and tail-call optimization (a tail call becomes a
frame rebind + jump inside the same loop). Both require that the 0-ary/1-ary
lambda dichotomy and the top-level track are *one mechanism*, distinguished by
frame data, not by separate code paths.

This design continues and completes the line started by
`667977b refactor(rill-lang): unify Closure/Thunk into Value::Callable + single
run_call` (2026-10-10): that commit already merged `Closure`/`Thunk` into one
`Value::Callable` and made `run_call` the single fragment dispatcher. What
remains is folding the *top-level track* into the same mechanism and making the
eager/lazy split a property of the frame, not of the entry path.

## Decisions (confirmed with the user, 2026-10-18)

1. **One function `run`** — every execution (top-level track, eager call, lazy
   force, effect application) physically goes through a single execution
   function. The 0-ary (thunk, `cell: Some`) vs 1-ary (eager closure,
   `cell: None`) dichotomy is **data of the frame** — argument count resolved
   against the fragment's `value_ins` plus the `cell: Some/None` greediness
   marker — not separate code.
2. **Correctness/structure first, optimizations later.** The unified path may
   build an env record per eager application (an allocation today's
   `ValueCallFunc` avoids by reading args from registers). Allocation-shaving is
   a later compile-time optimization, out of scope.
3. **Program = fragment.** The top-level value track becomes the body of the
   program's `main` fragment; running the program is running that fragment at
   base 0. One mechanism for everything.
4. **Reuse the greediness marker as-is.** `Value::Callable { cell: None }` =
   eager/greedy (already-WHNF, nothing to memoize); `cell: Some(cell)` = lazy
   thunk (memoized, call-by-need, black-hole FSM). No new field in the value
   structure; no compile-time optimization of call sites for now.
5. **Scope:** pure-value interpreter paths. No change to the ST runtime
   (`StCell`, `runST` regions), no change to `Value::Bottom` semantics, no
   change to fault messages, no change to the parser/AST/prelude. `signal_ins`
   etc. are vestigial (signal mode no longer exists — only values); they are
   left alone unless the unification touches their exact lines.

## Background: the six run forms and where the duplication lives

| Form | Location | Role |
|---|---|---|
| `run_value` | `program.rs:979` | public entry, delegates to `run_value_tick` |
| `run_value_tick` | `interp.rs:54` | one whole-buffer tick: push builtin params → release old outputs → `run_value_track` → `swap_block_state` → force+copy outputs → clear regs |
| `run_value_track` | `interp.rs:139` | walks top-level `value_blocks` from `value_entry`, base 0 |
| `run_main_io` | `interp.rs:213` | runs `main : IO ()` to completion (tick + project `unIO` + apply + force) |
| `run_io_action` | `interp.rs:4552` | reentrant helper: force `unIO`, apply to a World token, return payload (used by stream effects) |
| `run_call` | `interp.rs:4762` | walks a fragment's blocks with a frame: captures + args + watermark + result copy + frame unwind |

The key observation (verified in code): **`run_value_track` and `run_call`
contain literally the same ~40-line block-walk loop** — `for i in &b.instrs {
exec_value_instr(prog, i, base, drops) }` then the terminator dispatch
(`Fallthrough`/`Branch`/`BranchCtor`/`Halt`) — differing only in the block
source (`prog.ir.value_blocks` mem::taken vs `frag.value_blocks` borrowed from
an `Arc`) and the register base (`0` vs the call frame's `base`).

Entry points into the fragment executor today:
- `ValueCallEager` → `run_call(ArgsSource::Registers)` (strict-mode direct call);
- `ValueCallLazy` → builds a thunk; later `force()` → `run_call(ArgsSource::EnvTail)`;
- `apply_callable` → `run_call(ArgsSource::Registers)` (7 call sites: IO, effects, streams, runST);
- `force()` → `run_call(ArgsSource::EnvTail)` for thunk bodies;
- `run_value_track` → its own copy of the loop (top-level).

Native recursion: `exec_value_instr` → (nested `ValueCallEager`) → `run_call` →
`exec_value_instr` → … — the call depth grows the native stack. `max_call_regs`
(`call_depth_allowance × max_fragment_regs`) is currently a *limit* (a latch at
exhaustion), not a managed structure.

## Design

### 1. A single block-walker

Extract the duplicated loop into one function that walks any block list at any
base:

```
run_blocks(prog, blocks: &[ValueBlock], entry: usize, base: usize, drops: &mut Vec<ArenaRef>)
```

- Executes `b.instrs` via `exec_value_instr(prog, i, base, drops)` and the
  terminator dispatch, with the same `debug_assert!` fall-off-the-end guard.
- Both `run_value_track` and `run_call`'s body-walk become calls to it. (The
  `mem::take`/restore of `prog.ir.value_blocks` stays at the top-level caller —
  it is an ownership discipline of the top-level path, not of the walker.)
- This is the single point where a future trampoline/TCO loop lives: the
  walker is the "step" and the dispatcher decides whether to return to the loop
  or rebind the frame.

### 2. Program = fragment (`main`)

- The top-level track becomes the body of the program's `main` fragment:
  `Ir.fragments` gains (or `Ir.value_blocks`/`value_entry` is aliased as) the
  main fragment, executed with `base = 0`, `ArgsSource`-empty env (no args,
  nothing captured), and the top-level output registers.
- `run_value_tick`'s track phase becomes "run the main fragment" through the
  same `run_call`-family entry as any other fragment. The per-tick
  orchestration (param push, output release, block-state swap, output copy,
  reg clear) stays in `run_value_tick` — that is tick policy, not execution.
- Consequence: `run_value_track` disappears as a separate function; the
  top-level path is a call with an empty frame at base 0. The `debug_assert!`
  and drops discipline of the top-level path are preserved by the main-fragment
  caller.
- **Frame-unwind reconciliation (call made, from builder check):** `run_call`
  currently unwinds its frame by dropping every register in `base..top`; if the
  main fragment used the ordinary path, that unwind would drop the top-level
  output registers *before* `run_value_tick` can force+copy them. Resolution:
  the unified entry gains a **no-unwind mode** (a frame flag meaning "the
  caller owns register cleanup"). The main-fragment caller (`run_value_tick`)
  runs the main fragment in no-unwind mode, then does the existing output
  force+copy and the existing `clear_value_regs` as today. The top-level
  `value_error` latch check moves to the main-fragment caller, preserving its
  `Result` contract and its ordering relative to output forcing (including the
  "latched during output forcing" case).

### 3. One entry: `force` as the single execution point

- `force()` already loops (thunk-chase trampoline): it forces a `cell: Some`
  callable through the FSM, memoizes, and continues `r = result`. It already
  treats `cell: None` as "already WHNF" and returns as-is.
- The eager path (`ValueCallEager`, `apply_callable`) is folded into the same
  function: applying a callable = building the applied frame (captures + args,
  `cell: None` for eager / `cell: Some` for lazy) and forcing it. The
  `ArgsSource` distinction (`Registers` vs `EnvTail`) becomes a property of how
  the frame is materialized, not of which function runs the body.
- `BuiltinCallable` (effects) stays a value variant, but its application also
  funnels through the same entry (the unified function dispatches on
  callable-kind: `Callable` fragment vs `BuiltinCallable` effect vs `Bottom`).
  Effects remain strict — applying one still runs it eagerly through
  `exec_effect`; the point is a single call boundary, not deferring effects.
- Result: `run_call`'s body-walk is the *only* place a fragment's blocks run,
  and every way to reach it (top-level, eager, lazy, effect, IO) goes through
  the same function. The 0-ary/1-ary dichotomy is gone as a code-path
  distinction: both are "a callable with a frame; run its body".
- **Return-value ownership contract (call made, from builder check):** the
  unified entry returns a *borrowed* head ref (`force`-style — the memo cell or
  the caller's operand owns it), and fresh ownership is applied at the
  boundaries that need it: `dst` writes (`copy_owned`), the 7 `apply_callable`
  sites' owned-return contract, `ThunkState::Forced` memo storage, and the
  `force_owned` wrapper becoming the standard adapter rather than a special
  case. Rationale: the entry *is* "force to WHNF", and a borrowed return is
  what the future trampoline loop wants (frames own the heads, no per-step
  copies). This makes `force` the canonical entry and `force_owned` the
  standard way to take ownership.
- **`ValueCallLazy` eager special case is preserved (call made, from builder
  check):** `ValueCallLazy` deliberately does *not* suspend when the callee is
  a thunk or a `BuiltinCallable` — it forces and applies eagerly, which is what
  makes an `ST` action reached through a `let` run at all (interp.rs:1314-1321
  documents this as load-bearing). The unified lazy arm keeps that callee-kind
  check: build a thunk only for an eager `Value::Callable` callee; force+apply
  eagerly for thunk / `BuiltinCallable` / `Bottom` callees. This invariant is
  asserted by the ST tests, which stay green.

### 4. Frame content, not code, distinguishes eager/lazy

- The greediness marker `cell: Some/None` is reused as-is (Decision 4): for a
  forced value, `cell: Some` means memoize through the FSM; `cell: None` means
  run-and-return without memo (a greedy application, or an already-WHNF
  closure/value).
- Arity: the frame materializes `captures + value_ins args` (as `EnvTail`
  already does); a 0-ary lambda is a frame with `value_ins == 0`, a 1-ary with
  `value_ins == 1`. The walker itself does not need the arity — `run_call`'s
  env split already knows it from `frag.sig.value_ins`.

### 5. What stays

- `run_value_tick` — tick orchestration (param push, output release, block
  state swap, output copy, reg clear) around "run the main fragment".
- `run_main_io` — IO entry: tick + project `unIO` + apply + force + the
  `IO ()` payload contract. Its apply step goes through the unified entry.
- `run_io_action` — the reentrant stream helper: it already uses
  `force`/`apply_callable`; it now calls the unified entry. Kept as a distinct
  *orchestration* (it receives a token and returns a payload), not a distinct
  *execution*.
- RC/COW (`drop_value_children`/`recount_children`), the arena, the FSM, fault
  latching, `StCell`/runST — untouched.

## Why this enables trampoline / TCO

- **Trampoline:** with one `run_blocks` + one `force`-style entry, a call
  becomes "materialize the next frame and continue the loop" instead of
  "recurse into `run_call`". The existing flat `value_regs` array + watermark is
  already a frame store; the unification is what lets the loop, not the native
  stack, own call depth.
- **TCO:** a tail call (the fragment's last operation) is a frame rebind +
  jump within the same walker, no push. This is a later transform; the unified
  walker is its prerequisite.

## Testing

- Full existing suite green: `cargo test -p rill-lang --features dsp`;
  clippy `--lib --tests`; fmt `--check`. The refactor is behavior-preserving.
- Dedicated tests:
  - A deep non-tail recursion now runs to the same result as before the
    refactor (behavior parity, not yet stack-safety — stack-safety is the
    trampoline step, out of scope here).
  - Program-as-fragment: a program with top-level bindings, outputs, and a
    `main : IO ()` all run identically to pre-refactor.
  - 0-ary vs 1-ary parity: a nullary `where` binding, a thunk in complete mode,
    an eager closure, and an applied effect all reach the same execution
    function (structural assertion via a test-only probe if feasible).
- No parser/AST/prelude/IR-format change: the IR may gain a `main_fragment`
  view, but existing IR shapes (top-level blocks) keep lowering unchanged.

## Calls I made (builder-check settlement)

The builder check asked three questions; all were *minor* (a sensible builder
could settle them without changing what gets built), so they became my calls
rather than going back to the user. They are marked inline in the design above
and summarized here for review:

1. **Return-value ownership contract** (Q1): the unified entry returns a
   **borrowed** head ref (`force`-style); fresh ownership is applied at the
   boundaries that need it (`dst` writes, the 7 `apply_callable` sites'
   owned-return, `ThunkState::Forced` memo, `force_owned` as the standard
   adapter). See §3.
2. **Main-fragment frame-unwind** (Q2): the unified entry gains a **no-unwind
   mode** so the main fragment runs through the frame machinery without the
   caller losing its output registers; `run_value_tick` keeps output
   force+copy and `clear_value_regs`, and the `value_error` latch check moves
   to the main-fragment caller. See §2.
3. **`ValueCallLazy` eager special case** (Q3): preserved as a callee-kind
   check inside the unified lazy arm (force+apply eagerly for thunk /
   `BuiltinCallable` / `Bottom`; build a thunk only for an eager callable). See
   §3.

## What is left to the builder

- Exact function names and module placement (`run_blocks`, the unified entry's
  name — `force` vs a new `run` — and how `ArgsSource` is materialized).
- Whether `Ir.fragments` gains a real `main` fragment entry or the top-level
  blocks are wrapped by reference (the "program = fragment" decision is fixed;
  the wiring is the builder's).
- How `apply_callable`'s 7 call sites are rewritten against the unified entry
  (each is a one-line change; the callable-kind dispatch moves into the unified
  function).
- Whether effects (`BuiltinCallable`) share the unified entry's dispatch or
  keep a thin fast path inside it — the *boundary* is one function; the internal
  arm structure is the builder's call.
- Preserving the exact drop discipline (mark/drain) of the top-level path when
  it becomes the main fragment.
