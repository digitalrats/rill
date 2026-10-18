# rill-lang: Real IO + Full ST + io-streams-style Streams (memory + stdio) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `IO` and `ST` real (State + RealWorld token-threading), add full Haskell ST with `STRef`, add io-streams-style streams over a generic element (memory + stdio), run `main : (String…) -> IO ()` to completion in a new `ProgramRunner`, and remove the old signal-transform execution system.

**Architecture:** `IO a`/`ST s a` become record wrappers over a token-threading function (`World -> Pair World a` / `STState -> Pair STState a`); `World`/`STState`/`STRef`/`InputStream`/`OutputStream` are new **opaque primitive types** — nameable, never constructible in source, only the runtime creates their values. Effects are performed by builtin effect-closures (`Value::BuiltinCallable`) that the interpreter applies against a runner-owned `WorldState` (stream table + console) hanging off `RillProgram`. The `ProgramRunner` compiles in `Complete` (lazy) mode, evaluates `main`'s value, applies `.unIO` to the `World` token, and drives effects to completion. Task 2 is the pivot: re-expressing `IO`'s shape breaks the old `readInput`/`writeOutput`/buffer-λ `main`, so removal and suite migration land there.

**Tech Stack:** Rust, `rill-lang` crate (`--features dsp`), no new dependencies. Workspace at `/home/mikek/Projects/digitalrats/rill/.worktrees/io-streams-core`.

**Spec:** `docs/superpowers/specs/2026-10-17-rill-lang-io-state-world-streams-design.md` (approved; this plan argues from it).

## Global Constraints

- **No backward compatibility (directive).** Buffer-λ `main`, `readInput`/`writeOutput`, and the signal-transform `ProgramRunner`/`ProgramEngine` framing are removed. Existing programs and tests migrate to `main : (String…) -> IO ()` with stream I/O. No sugar layer, no transition period.
- **ProgramRunner is an executor of a Turing-complete program**; it takes no input/output buffers or backend handles from outside. Streams (memory, stdio) are the program's I/O, created inside the program.
- **Prelude placement:** `CATEGORY_PRELUDE` keeps only the typeclass declarations (Functor/Applicative/Monad/Monoid/Arrow) and the Kleisli construction. `STD_PRELUDE` holds the concrete data types and instances moved from `CATEGORY_PRELUDE` (ST + STRef + IO, the Maybe/List/Float/String instances), plus the stream types/ops and stdio. `SIGNAL_PRELUDE` is untouched.
- **Runner evaluation mode:** the runner compiles and runs `main` in **Complete mode** (`{-# MODE complete #-}` applied when the source has no pragma). Recursion (stream loops, the reference program) is banned by Strict's `check_recursion`.
- **Opaque primitive types:** `World`/`STState`/`STRef`/`InputStream`/`OutputStream` are nameable but not constructible, projectable, or matchable in source; only runtime builtins create their values.
- **No new third-party dependencies.** All mechanisms come from the existing value track, the `Monad` machinery, the rank-2 `runST` special case, and `STD_PRELUDE`.
- `rill-lang` denies `unsafe_code`, warns on `missing_docs`. Docs in English, `max_width=100`.
- **Verification target:** `cargo test -p rill-lang --features dsp` green after every task. **Graph compilation is not required** — the `graph` feature is out of scope and may stop compiling; `rill-adrift` drops its `graph` dependency on `rill-lang`. `cargo clippy -p rill-lang --features dsp` zero warnings, `cargo fmt`.
- **Bounded memory:** programs are assumed bounded for this plan; the fixed arena budget is kept, its limit documented, tests sized within it. Overflow reports a clean capacity error, not a crash.
- **Commits:** conventional, single quotes (`feat(rill-lang): …`). Do not touch `src/arrow.rs` or `src/graph/reconstruct.rs`.
- From Task 2 through Task 4, `rill-adrift` does **not** compile (its surface is repaired in Task 5). Only `-p rill-lang --features dsp` is verified in Tasks 2–4.

## Review Focus

1. **An opaque type is constructible/projectable/matchable in source** (`World {}`, `x.unIO`-style projection, `case w of …`) — must be a **compile-time** error, never a runtime crash. → Task 1.
2. **An `STRef` (or ST result mentioning `s`) escapes its `runST` region** (returned inside a list, pair, etc.), and **two `runST` regions share cells** — escape must be a compile-time error and regions must be isolated at runtime. → Task 1.
3. **IO effect ordering under laziness/sharing** — two `write`s in a `do`-block, and the same action bound under `let` and used twice, must be observed in program order; the `World` token must prevent reordering even though each effect is lazy. → Task 2.
4. **Stream edge cases** — `read` at EOF returns `Nothing` (not an error); `toList` on an empty stream; `connect` between unequal lengths; `unread` then `read` round-trips. → Task 3.
5. **A program with no I/O (`main = return ()`) runs to completion cleanly; a bounded-memory overflow reports a clean capacity error, not a crash.** → Task 2.

---

### Task 1: Opaque primitive types + prelude relocation + real ST with STRef

**Files:**
- Modify: `rill-lang/src/ast.rs` — add `Def::Abstract { name: String, tyvars: Vec<String>, span: Span }`.
- Modify: `rill-lang/src/parser.rs` — `parse_data_def` accepts `data Name tyvars;` (no `=`) → `Def::Abstract`.
- Modify: `rill-lang/src/render.rs` — render `Def::Abstract` as `data Name tyvars;`.
- Modify: `rill-lang/src/types/ty.rs` — add `DataInfo::Opaque`; relocate prelude text (below); register `STRef` builtin sigs; add `env.derive_superclass_instances()` after the STD registration (after line 631).
- Modify: `rill-lang/src/types/infer.rs` — opaque-type enforcement; forall instantiation for ordinary builtin sigs; `type_expr_to_vty`/`vty_of_name` resolves the opaque names with arity.
- Modify: `rill-lang/src/lower.rs` — `runST` from body-erasure to a runtime `RunST` op; STRef ops as effect closures.
- Modify: `rill-lang/src/ir.rs` — new `ValueBuiltinOp` variants (or a new `ValueEffectOp` enum): `RunST`, `NewStRef`, `ReadStRef`, `WriteStRef`, `ModifyStRef`.
- Modify: `rill-lang/src/arena.rs` — add `Value::Opaque(u32)` and `Value::BuiltinCallable { op: ValueEffectOp, env: ArenaRef }`.
- Modify: `rill-lang/src/program.rs` — add the ST region/cell-table state on `RillProgram`.
- Modify: `rill-lang/src/backend/interp.rs` — dispatch `RunST` + STRef ops; `apply_callable` helper.
- Test: `rill-lang/tests/st_real.rs` (new), `rill-lang/src/types/infer.rs` (opaque + forall tests), `rill-lang/src/lower.rs` (runST IR tests).

**Interfaces:**
- Consumes: `Mode`, the `builtin_sigs` map (`TypeEnv::builtin_sigs: HashMap<String, ast::TypeExpr>`), the rank-2 `runST` special case (`infer.rs:3350-3424`), `ValueInstr::ValueCallBuiltin`/`exec_value_call_builtin` (`interp.rs:1747`).
- Produces:
  - `Def::Abstract { name, tyvars, span }` — abstract type declaration.
  - `DataInfo::Opaque` — a nameable type with no constructors and no fields.
  - `Value::Opaque(u32)` — opaque handle value (World, STState, STRef).
  - `Value::BuiltinCallable { op: ValueEffectOp, env: ArenaRef }` — effect closure; `interp` dispatches on `op` and performs the effect against program state.
  - `fn apply_callable(prog, callee: ArenaRef, arg: ArenaRef) -> Result<ArenaRef, ProcessError>` — applies a `Value::Callable`/`Value::BuiltinCallable` to one argument (used by `RunST` and later the runner).
  - `ValueEffectOp` with `RunST`, `NewStRef`, `ReadStRef`, `WriteStRef`, `ModifyStRef`.
  - New `STD_PRELUDE` text (below) and reduced `CATEGORY_PRELUDE` text (below).
  - Builtin sigs (below) for the STRef family, registered via `TForall`.

- [ ] **Step 1: Write the failing tests for opaque types and real ST**

Add to `rill-lang/src/types/infer.rs` tests:
```rust
#[test]
fn opaque_type_is_nameable_but_not_constructible() {
    // `data World;` — a signature may mention World, but `World {}` is an error.
    // ty_of("main = World {};") -> err; ty_of("f: World; f = undefined; main = ();") -> ok
}
#[test]
fn opaque_type_has_no_projection_or_patterns() {
    // ty_of("data W; main = case (undefined: W) of { _ -> 1.0 };") -> err (no ctors)
    // projection on an opaque value -> err
}
```
Add to `rill-lang/tests/st_real.rs`:
```rust
#[test]
fn stref_read_write_modify_roundtrip() {
    // "{-# MODE complete #-}\nmain = runST (\\s -> do { r <- newSTRef 0.0;
    //   writeSTRef r 42.0; modifySTRef r (\\x -> x + 1.0); readSTRef r });"
    // evaluate to a Float value of 43.0 (via the value-eval path).
}
#[test]
fn two_runst_regions_are_isolated() {
    // region A writes a cell, region B reads its own fresh cell -> no sharing.
    // main = Pair { first: runST (\\s -> do { r <- newSTRef 1.0; readSTRef r }),
    //               second: runST (\\s -> do { r <- newSTRef 2.0; readSTRef r }) };
    // -> Pair { first: 1.0, second: 2.0 }
}
#[test]
fn stref_escaping_region_is_compile_error() {
    // main = runST (\\s -> do { r <- newSTRef 0.0; return r }); -> Type error
}
```

- [ ] **Step 2: Run to verify the tests fail**

Run: `cargo test -p rill-lang --features dsp --lib types::infer` and `cargo test -p rill-lang --features dsp --test st_real`.
Expected: FAIL (no `Def::Abstract`, no `DataInfo::Opaque`, no `Value::Opaque`, runST still erases).

- [ ] **Step 3: Add `Def::Abstract` + parse + render + `DataInfo::Opaque`**

AST: `Def::Abstract { name, tyvars, span }`; `name()`, `tyvars()` accessors matching the other `Def` variants. Parser: in the `data` production, if there is no `= … ;` body, emit `Def::Abstract`. Renderer: `data Name tyvars;`. In `register_prelude`/`with_builtins` and user-data registration (`infer.rs:1176-1178`), an `Abstract` def registers in `data_arities` (if parameterized) and as `DataInfo::Opaque`.

- [ ] **Step 4: Enforce opaque types (no construction / projection / pattern-match)**

In inference, when a record-literal/ctor-application targets a type whose `DataInfo` is `Opaque`, or an expression projects a field of an opaque-typed value, or a `case`/`match` scrutinee is opaque, return `CompileError::Type` ("opaque type `World` has no constructors/fields"). Lowering never emits construction for opaque types (inference rejects first). These checks mirror the existing record/ctor/match handling.

- [ ] **Step 5: Add `Value::Opaque(u32)` and `Value::BuiltinCallable` to the arena; add the region state**

`Value::Opaque(u32)` — an opaque handle (World token = `0`; STState = a region id; STRef = a cell handle). `Value::BuiltinCallable { op, env }` — an effect closure holding its captured args in `env` (an arena ref to a record/pair of args) and a tag naming the effect. Add to `RillProgram` a region table (e.g. `regions: Vec<RegionState>` where `RegionState` holds a cell table `Vec<Option<ArenaRef>>`) and a monotone region-id counter.

- [ ] **Step 6: Rewrite `runST` and add the STRef ops in lowering + interpreter**

Lowering: `runST (\s -> body)` — keep the phantom-`s` handling (lower the lambda body to the ST value, an `{ unST: … }` record) and emit the ST value through the `RunST` builtin op. The `RunST` handler (`apply_callable` helper): allocate a fresh `RegionState` + `Value::Opaque(region_id)`, project the ST value's `.unST` closure, apply it to the region token, discard the final state, return the `a`. The STRef ops lower to `Value::BuiltinCallable { op, env }`; their handlers look up the region by the `STState` token id and read/write/modify the cell (new `NewStRef` allocates a cell and returns an `STRef` handle `Value::Opaque(cell_handle)`).

- [ ] **Step 7: Add forall instantiation for ordinary builtin sigs; register the STRef sigs**

Extend the ordinary builtin-sig path (`infer.rs:3432`) so a stored sig that is a top-level `TForall(vars, inner)` instantiates each bound var and each remaining free type variable to a fresh `TypeVarId`, then types `inner` as today. The `runST` rank-2 special case is unchanged. Register (matching the `runST` registration style at `ty.rs:641-656`):
```
newSTRef    :: a -> ST s (STRef s a)
readSTRef   :: STRef s a -> ST s a
writeSTRef  :: STRef s a -> a -> ST s ()
modifySTRef :: STRef s a -> (a -> a) -> ST s ()
```

- [ ] **Step 8: Relocate the prelude text and re-express `ST`**

`CATEGORY_PRELUDE` (`ty.rs:256-313`) is reduced to:
```
typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
typeclass Applicative f where { pure: a -> f a; ap: f (a -> b) -> f a -> f b; }
typeclass Monad m where { return: a -> m a; bind: m a -> (a -> m b) -> m b; }
typeclass Monoid m where { mempty: m; mappend: m -> m -> m; }
data Kleisli m a b = { unKleisli: a -> m b };
typeclass Arrow a where { arr: (b -> c) -> a b c; first: a b c -> a (Pair b d) -> a (Pair c d); compose: a b c -> a c d -> a b d; second: a b c -> a (Pair d b) -> a (Pair d c) = k (compose (compose (arr (\ p -> Pair { first: p.second, second: p.first })) (first k)) (arr (\ p -> Pair { first: p.second, second: p.first }))); both: a b c -> a d e -> a (Pair b d) -> a (Pair c e) = f g (compose (first f) (second g)); fan: a b c -> a b d -> a b (Pair c d) = f g (compose (arr (\ x -> Pair { first: x, second: x })) (both f g)); }
instance (Monad m) => Arrow (Kleisli m) where { arr f = Kleisli (\ x -> return (f x)); first k = Kleisli (\ p -> bind (k.unKleisli p.first) (\ z -> return (Pair { first: z, second: p.second }))); compose k1 k2 = Kleisli (\ x -> bind (k1.unKleisli x) (\ y -> k2.unKleisli y)); }
main = ();
```
(Verbatim from the current `CATEGORY_PRELUDE` minus the moved instances.)

`STD_PRELUDE` (`ty.rs:359-370`) becomes (this task's slice; IO stays phantom until Task 2):
```
data World;                                   -- primitive opaque token (GHC State# RealWorld analog)
data STState;                                 -- primitive opaque token (GHC State# s analog)
data STRef s a;                               -- opaque cell handle (only newSTRef creates one)
data ST s a = { unST: STState -> Pair STState a };   -- state-threading monad (moved + re-expressed)
data IO a = { unIO: a };                      -- moved from CATEGORY_PRELUDE (re-expressed in Task 2)
instance Monoid Float where { mempty = 0.0; mappend a b = a + b; }
instance Monoid String where { mempty = ""; mappend a b = concat_string a b; }
instance Monoid List where { mempty = list; mappend a b = append_list a b; }
instance Functor List where { fmap g xs = map g xs; }
instance Functor Maybe where { fmap g m = case m of { Nothing -> Nothing; Just x -> Just (g x); }; }
instance Monad Maybe where { return x = Just x; bind m f = case m of { Nothing -> Nothing; Just x -> f x; }; }
instance Monad List where { return x = cons x (list); bind xs f = concat_map f xs; }
instance Monad (ST s) where {
    return x = ST { unST: \s -> Pair { first: s, second: x } };
    bind m f = ST { unST: \s -> let p = m.unST s in (f p.second).unST p.first };
};
instance Monad IO where { return x = IO { unIO: x }; bind m f = IO { unIO: (f m.unIO).unIO }; }
main = ();
```
Add `env.derive_superclass_instances()` after the STD registration so the moved `Monad` instances derive their `Applicative`/`Functor` superclasses (as they did while in `CATEGORY_PRELUDE`).

**RULING (plan-defect fix, pre-flight scan):** the current tree's `STD_PRELUDE` also holds the OLD design's stream declarations — `data InputStream a = { unInputStream: a };`, `data OutputStream a = { unOutputStream: a };`, `data Stream a = { };`, `data BackendSpec = { name: String };`. Task 1 **KEEPS** those declarations (they keep `tests/stream_contract.rs` and the old stream sigs `runStream`/`asInput`/`asOutput`/`mkStream` type-checking through Task 1). Task 1 **ADDS** the new decls/instances above to `STD_PRELUDE` and **REMOVES** the moved `ST`/`IO`/instances from `CATEGORY_PRELUDE`; it does NOT delete `Stream`/`BackendSpec` or replace the record `InputStream`/`OutputStream` with opaque forms — that is Task 2's removal/swap (Task 2 Step 3/7).

- [ ] **Step 9: Run the full suite**

Run: `cargo test -p rill-lang --features dsp`.
Expected: all green — the existing type-level `st_monad.rs`/`rank2_poly.rs` still pass (escape check retained), and the old signal suite is untouched (IO still phantom; `readInput`/`writeOutput` still compile).
Run: `cargo clippy -p rill-lang --features dsp --tests`, `cargo fmt`.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m 'feat(rill-lang): opaque primitive types, prelude relocation, real ST with STRef'
```

---

### Task 2: Real IO + the new ProgramRunner + memory-stream core + remove the old IO surface + migrate the suite

This is the pivot. Re-expressing `IO` changes its shape and breaks `readInput`/`writeOutput`/buffer-λ `main`, so removal and the suite migration land in the same task. The value-track interpreter and the pure-value compile path (`compile` → value-outputs) are retained; only the signal *IO surface* is removed here. `ProgramEngine`, the `Algorithm`/`MultichannelAlgorithm` impls, `compile_graph`, and `runtime.rs` stay until Task 5 (they still serve the pure-value test path and rill-adrift).

**Files:**
- Modify: `rill-lang/src/types/ty.rs` — re-express `IO` in `STD_PRELUDE`; add `data InputStream a;`/`data OutputStream a;` opaque decls; remove the old stream sigs (`read`/`write`/`channel`/`runStream`/`asInput`/`asOutput`/`mkStream`, `ty.rs:693-830`) and `readInput`/`writeOutput` (`ty.rs:657-683`); add `fromList`/`toList`/`write`/`stdout`/`show_float` sigs; drop `Stream`/`BackendSpec` from `STD_PRELUDE`.
- Modify: `rill-lang/src/types/infer.rs` — `main` λ-params change from `Buffer` to `String` (`:1441-1447`, `:1458-1462`); remove the `main`-as-Buffer special-case.
- Modify: `rill-lang/src/lower.rs` — remove `readInput`/`writeOutput` lowering (`:1140-1197`, `:3147-3157`); remove buffer-λ `main` + `is_io_program` special-casing (`:6160-6242`); add `fromList`/`toList`/`write`/`stdout`/`show_float` lowering to `Value::BuiltinCallable` IO records.
- Modify: `rill-lang/src/ir.rs` — add `ValueEffectOp::StreamFromList`, `StreamToList`, `StreamWrite` (Task 3 adds the remaining memory ops).
- Modify: `rill-lang/src/program.rs` — add `world: Rc<RefCell<WorldState>>` to `RillProgram`; define `WorldState { streams: Vec<StreamHandle>, stdout: Vec<String>, stderr: Vec<String> }`.
- Modify: `rill-lang/src/arena.rs` — `Value::BuiltinCallable` already added (Task 1); stream handles are `Value::Opaque(u32)` into the stream table.
- Modify: `rill-lang/src/backend/interp.rs` — dispatch `StreamFromList`/`StreamToList` against `prog.world`.
- Rewrite: `rill-lang/src/program_runner.rs` — the new executor (`set_args`/`run`/`stdout`/`stderr`).
- Modify: `rill-lang/src/lib.rs` — new compile entry for the runner; keep `compile` (pure-value path).
- Delete: `rill-lang/src/stream_backend.rs`, `rill-lang/tests/stream_contract.rs`.
- Migrate: the old signal test files (inventory below).
- Test: `rill-lang/tests/io_runner.rs` (new), `rill-lang/tests/io_streams_memory.rs` (new, fromList/toList slice).

**Interfaces:**
- Consumes: `Def::Abstract`, `Value::Opaque`, `Value::BuiltinCallable`, `apply_callable`, `ValueEffectOp` (Task 1).
- Produces:
  - `STD_PRELUDE` `IO` re-expression: `data IO a = { unIO: World -> Pair World a };` + `Monad IO` threading (verbatim in Step 3).
  - `WorldState` on `RillProgram`; `ValueEffectOp::StreamFromList`/`StreamToList`.
  - The new `ProgramRunner`:
    ```rust
    impl<T: Transcendental, const BUF: usize> ProgramRunner<T, BUF> {
        pub fn new(program: RillProgram<T, BUF>) -> Self;
        pub fn set_args(&mut self, args: &[String]);       // bind main's String λ-params
        pub fn run(&mut self) -> Result<(), Error>;        // run main's IO to completion vs realWorld
        pub fn stdout(&self) -> &[String];                 // captured stdout (test access)
        pub fn stderr(&self) -> &[String];                 // captured stderr (test access)
    }
    ```
  - `pub fn compile_runner<T: Transcendental, const BUF>(src: &str, registry: &Registry<T>, sample_rate: f32, mode: Mode) -> Result<ProgramRunner<T, BUF>, CompileError>` (or `ProgramRunner::new(compile::<f32>(src)?)` — either entry; the runner forces `Complete` when the source has no pragma).
  - Builtin sigs: `fromList :: List a -> IO (InputStream a)`, `toList :: InputStream a -> IO (List a)`, `write :: Maybe a -> OutputStream a -> IO ()`, `stdout :: OutputStream String`, `show_float :: Float -> String` — registered via the forall mechanism from Task 1 (`stdout`/`show_float` are monomorphic; `fromList`/`toList`/`write` need forall instantiation). `stdin`/`stderr`/`show_int`/`show_bool` land in Task 4.
  - `main` λ-params typed `String`; `set_args` binds them.

- [ ] **Step 1: Write the failing tests**

`rill-lang/tests/io_runner.rs`:
```rust
#[test]
fn no_io_main_runs_to_completion() {
    // "{-# MODE complete #-}\nmain = return ();" compiles to a runner; run() -> Ok.
}
#[test]
fn memory_stream_roundtrip() {
    // main = do { xs <- fromList [1.0, 2.0, 3.0]; ys <- toList xs;
    //             write (Just (concat_map show_float ys)) stdout; return () }
    // run(); runner.stdout() == ["1.02.03.0"]   (concat_map + show_float render the drained list)
}
#[test]
fn io_effects_are_ordered() {
    // (a) two writes in a do-block must appear in source order in runner.stdout();
    // (b) the same IO action bound under `let` and used twice must fire twice, in program order:
    //     let a = write (Just "x") stdout in do { a; a }  -> stdout() == ["x", "x"].
}
#[test]
fn main_string_args_bind() {
    // main a = do { write (Just a) stdout; return () }; set_args(&["hi"]); stdout() == ["hi"]
}
#[test]
fn bounded_memory_overflow_is_a_clean_error() {
    // a Complete-mode program that accumulates beyond the fixed arena budget
    // (e.g. a recursive helper building an unbounded list before writing it)
    // -> run() returns Err(capacity), not a panic; assert !result.is_ok() and no unwind.
}
```
Note (decided): Task 2 adds `write`/`stdout`/`show_float` (plain stream ops over a `String` element + a scalar renderer — no machinery beyond Task 3), so `fromList`→`toList` is observable through the runner immediately. `stdin`/`stderr`/`show_int`/`show_bool` and the reference program stay in Task 4.

- [ ] **Step 2: Run to verify the tests fail**

Run: `cargo test -p rill-lang --features dsp --test io_runner`.
Expected: FAIL (`fromList`/`toList`/`write`/`stdout` unknown; `ProgramRunner::run` missing).

- [ ] **Step 3: Re-express `IO` in `STD_PRELUDE` and add the stream decls/sigs**

`STD_PRELUDE` gains (replacing the phantom `IO` from Task 1):
```
data InputStream  a;                          -- opaque stream handle (only runtime effects create one)
data OutputStream a;                          -- opaque stream handle (only runtime effects create one)
data IO a = { unIO: World -> Pair World a };
instance Monad IO where {
    return x = IO { unIO: \w -> Pair { first: w, second: x } };
    bind m f = IO { unIO: \w -> let p = m.unIO w in (f p.second).unIO p.first };
};
```
Remove `data Stream a = { };`/`data BackendSpec = { name: String };` from `STD_PRELUDE`. In `ty.rs`, remove the old stream sigs and `readInput`/`writeOutput`; add `fromList`/`toList`, `write`, `stdout`, and `show_float` via the forall mechanism (`write :: Maybe a -> OutputStream a -> IO ()` is polymorphic; `stdout`/`show_float` are monomorphic `String`/`Float -> String`).

- [ ] **Step 4: Change `main` λ-params to `String`; remove the buffer-λ `main` and `is_io_program` special-casing; remove `readInput`/`writeOutput`**

In `infer.rs`, the `main` param special-case (`:1441-1447`, `:1458-1462`) types `main`'s λ-params as `String` (not `Buffer`). In `lower.rs`, delete the `readInput`/`writeOutput` arms (`:1140-1197`, `:3147-3157`) and the buffer-λ `main`/`is_io_program` block (`:6160-6242`). `main` is now an ordinary value definition.

- [ ] **Step 5: Lower `fromList`/`toList`/`write`/`stdout`/`show_float`; add `WorldState`; dispatch the stream effects**

`fromList xs` lowers to `IO (InputStream a)` = a record whose single field is `Value::BuiltinCallable { op: StreamFromList, env: xs }`; `toList s` likewise with `StreamToList`; `write mb out` with `StreamWrite`; `stdout` resolves to a `StreamWrite`-target handle for `world.stdout`. `show_float` is a plain value builtin returning a `String` (like `concat_string`). `RillProgram.world: Rc<RefCell<WorldState>>` (default empty). The interpreter, when applying a `BuiltinCallable` (in `apply_callable`/`run_call`), performs the op: `StreamFromList` allocates a stream in `world.streams` and returns `Pair(new_world, handle)`; `StreamToList` drains it; `StreamWrite` appends to the target (for stdio targets, `world.stdout`). If `world` has no stream table (a bare compile), a clean error results.

- [ ] **Step 6: Write the new `ProgramRunner` (executor) + compile entry**

`run()`: force `Complete` mode on the source (if no pragma); evaluate `main`'s value (run the value track); project `.unIO`; apply it to `Value::Opaque(0)` (the World token); drive to completion; capture `stdout`/`stderr` from `world`. A program whose `main` is not an `IO ()` value returns a clean error. `set_args(&[String])` binds `main`'s String λ-params before running (fewer args than params → empty-string defaults, per spec §4.5). The runner takes no buffers/backends from outside.

- [ ] **Step 7: Remove the old stream surface; delete `stream_backend.rs` + `stream_contract.rs`; remove the old stream tests**

Remove the old stream sigs (`read`/`write`/`channel`/`runStream`/`asInput`/`asOutput`/`mkStream`) and `Stream`/`BackendSpec`; delete `src/stream_backend.rs` (it is referenced only from the `lib.rs` module declaration) and `tests/stream_contract.rs`; drop the `stream_backend` module from `lib.rs`. Note: with the `readInput`/`writeOutput` lowering removed, the IR/interp instructions `ValueReadInput`/`ValueWriteOutputList` (kept until Task 5) may become unconstructed and trip the dead-code lint — gate them with a scoped `#[allow(dead_code)]` (never crate-level) or `#[cfg]` them out now, and remove them in Task 5.

- [ ] **Step 8: Migrate the old signal test suite**

Apply the §4.6 migration rule to the `-p rill-lang` suite. For each old test that used `readInput`/`writeOutput`/buffer-λ `main`:
- **Pure computation preserved** → `main = <pure value over literal data>`, asserted via the retained `MultichannelAlgorithm::process` + `value_outputs` path (kept until Task 5). Examples: `tests/ffi.rs`, `tests/ffi_extended.rs`, `tests/ffi_partial_application.rs`, `tests/integration.rs`, `tests/atan2_lang_op.rs`, `tests/smooth.rs`, `tests/haskell2010_syntax.rs`, `tests/typeclass.rs` (one hybrid), `tests/collections_list.rs` (one), `tests/caf_free_variables.rs`, in-src `infer.rs`/`lower.rs` tests that only exercise the computation.
- **Genuine I/O** → `main : IO ()` via `fromList`/`toList` (and `write`/`stdout` if added in Step 1). Example: `tests/io_types.rs`.
- **Feature removed by the signal-transform engine** (per-block processing, buffer-λ main as control params, multichannel signal channels) → rewrite against the retained `?name`/`SetParameter` builtin-param mechanism or remove: `tests/main_cells.rs`, `tests/multichannel_runtime.rs`, `tests/graph_compile.rs` (graph is not required), `tests/program_json.rs` (update its `writeOutput` program string to the new form).
- `rill-lang/src/program_runner.rs` old test and `rill-lang/src/lib.rs` tests that reference `compile_graph`/`writeOutput` → migrate or remove.
Gate: `cargo test -p rill-lang --features dsp` green.

- [ ] **Step 9: Run the full suite; verify zero old-surface terms**

Run: `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp --tests`, `cargo fmt`.
Run: `grep -rn "readInput\|writeOutput\|Stream\|mkStream\|BackendSpec" rill-lang/src rill-lang/tests` — expected: no old-surface hits (the `graph` feature and `src/graph/**` are out of scope).

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m 'feat(rill-lang): real IO, ProgramRunner executor, memory streams; remove old IO surface and migrate suite'
```

---

### Task 3: Full memory-stream surface + the new stream tests

**Files:**
- Modify: `rill-lang/src/types/ty.rs` — add sigs for the remaining memory ops (`read :: InputStream a -> IO (Maybe a)`, `atEnd :: InputStream a -> IO Bool`, `unread :: a -> InputStream a -> IO ()`, `connect :: InputStream a -> OutputStream a -> IO ()`, `makeInputStream :: IO (Maybe a) -> IO (InputStream a)`, `makeOutputStream :: (Maybe a -> IO ()) -> IO (OutputStream a)`; `write` was added in Task 2).
- Modify: `rill-lang/src/ir.rs` — `ValueEffectOp` variants for `StreamRead`, `StreamAtEnd`, `StreamUnread`, `StreamConnect`, `StreamMakeInput`, `StreamMakeOutput` (`StreamWrite` came with Task 2).
- Modify: `rill-lang/src/lower.rs` — lower each op to an IO effect closure.
- Modify: `rill-lang/src/backend/interp.rs` — perform each op against `world.streams`.
- Modify: `rill-lang/src/program.rs` — `WorldState` stream table grows (memory sources with a position, sinks, connect buffers).
- Test: `rill-lang/tests/io_streams_memory.rs` (extend), `rill-lang/tests/io_runner.rs`.

**Interfaces:**
- Consumes: forall instantiation (Task 1), `Value::BuiltinCallable`, `WorldState` (Task 2).
- Produces: the io-streams operation surface over the generic element `a`, faithful to Haskell io-streams: `read` returns `Nothing` at EOF; `write Nothing` closes; `atEnd` reports EOF without consuming; `unread` pushes back; `connect` pipes input→output; `makeInputStream`/`makeOutputStream` wrap program-provided closures.

- [ ] **Step 1: Write the failing tests**

Extend `rill-lang/tests/io_streams_memory.rs`:
```rust
#[test]
fn read_pulls_elements_and_nothing_at_eof() { /* fromList [1.0,2.0]; read -> Just 1.0, Just 2.0, Nothing */ }
#[test]
fn to_list_on_empty_stream_is_empty()      { /* fromList (list); toList -> the empty List; render via concat_map show_float -> "" */ }
#[test]
fn write_nothing_closes_the_stream()       { /* write (Just 1.0); write Nothing; further write is inert */ }
#[test]
fn at_end_reports_eof_without_consuming()  { /* atEnd on a drained stream -> True */ }
#[test]
fn unread_then_read_roundtrips()           { /* unread 5.0 then read -> Just 5.0 */ }
#[test]
fn connect_pipes_input_to_output()         { /* fromList [..] >>= \in -> makeOutputStream (\mb -> record) >>= \out -> connect in out */ }
#[test]
fn make_stream_wraps_program_closures()    { /* makeInputStream (return (Just 7.0)) / makeOutputStream (\mb -> ...) */ }
#[test]
fn connect_unequal_lengths_ends_cleanly()  { /* longer input than output drain: ends at EOF, no error */ }
```
Assert results through the runner's captures or a `toList` drain.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp --test io_streams_memory`.
Expected: FAIL (ops unknown).

- [ ] **Step 3: Implement the ops**

Register the sigs via the forall mechanism. Lower each to `Value::BuiltinCallable` and perform against `world.streams`. `makeInputStream`/`makeOutputStream` capture a program closure value (a `Value::Callable`) in the effect closure's `env`; the handler applies it (via `apply_callable`) to get/accept the `Maybe a` element.

- [ ] **Step 4: Run the stream tests and the full suite**

Run: `cargo test -p rill-lang --features dsp --test io_streams_memory`, then `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp --tests`, `cargo fmt`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m 'feat(rill-lang): io-streams memory surface — read/write/atEnd/unread/connect/make*'
```

---

### Task 4: stdio streams + scalar rendering + the reference program

**Files:**
- Modify: `rill-lang/src/types/ty.rs` — add sigs `stdin :: InputStream String`, `stderr :: OutputStream String` (`stdout` was added in Task 2); add scalar renderers `show_int :: Int -> String`, `show_bool :: Bool -> String` (`show_float` was added in Task 2).
- Modify: `rill-lang/src/ir.rs` — `ValueEffectOp` variants (or a single `Stdin`/`Stdout`/`Stderr` handle resolution) + renderer ops (or route through `concat_string`-style value builtins).
- Modify: `rill-lang/src/lower.rs` — lower the stdio ops and renderers.
- Modify: `rill-lang/src/backend/interp.rs` — stdio ops read/write the runner's console (`world.stdout`/`world.stderr`); the console is the runner's realized `realWorld` (real stdio by default, injectable capture console for tests).
- Modify: `rill-lang/src/program_runner.rs` — console injection (`stdout()`/`stderr()` accessors already exist from Task 2).
- Test: `rill-lang/tests/io_stdio.rs` (new); the reference program end-to-end.

**Interfaces:**
- Consumes: stream ops (Task 3), `WorldState.stdout/stderr` (Task 2).
- Produces: `stdin :: InputStream String` and `stderr :: OutputStream String` (line-oriented `String` text; rill has no `ByteString`; `stdout` came with Task 2), `show_int`/`show_bool` scalar renderers (`show_float` came with Task 2); console injection on the runner for tests.

- [ ] **Step 1: Write the failing tests**

`rill-lang/tests/io_stdio.rs`:
```rust
#[test]
fn stdout_capture_roundtrip()   { /* main = write (Just "hi") stdout; run(); runner.stdout() == ["hi"] */ }
#[test]
fn stderr_is_separate()         { /* write to stderr -> runner.stderr() == [...] , stdout() empty */ }
#[test]
fn stdin_feeds_lines()          { /* injected stdin lines; read stdin -> the injected Strings */ }
#[test]
fn reference_program_runs()     { /* the spec §7 reference program; assert captured stdout contains "sum-ish" */ }
#[test]
fn show_renders_scalars()       { /* show_float 1.5 -> "1.5"; show_int 3 -> "3"; show_bool True -> "True" */ }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp --test io_stdio`.
Expected: FAIL (ops/renderers unknown).

- [ ] **Step 3: Implement**

Register the stdio sigs (monomorphic `String`; no forall needed) and the scalar renderers (value builtins returning `String`, like `concat_string`). Lower the stdio ops to `Value::BuiltinCallable`; handlers resolve `stdin`/`stderr` against `world` (real stdio by default; the runner injects a capture console; `stdout` resolution is already in place from Task 2). Implement `write Nothing` = EOF/close for **program-owned** output streams (`makeOutputStream` sinks); on the console handles `stdout`/`stderr` it is **inert** — those are runtime-owned and not program-closeable (spec §4.4).

- [ ] **Step 4: Wire the reference program and console injection**

The spec §7 reference program uses `++`/`show`; the exact surface is `concat_string` + `show_float` (no `++`, no `Show` typeclass; list rendering is out of scope — render the scalar STRef result). Add the console-injection hook on `ProgramRunner` for tests. Run the reference program through the runner and assert captured stdout.

- [ ] **Step 5: Run the full suite**

Run: `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp --tests`, `cargo fmt`.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m 'feat(rill-lang): stdio streams, scalar rendering, reference program'
```

---

### Task 5: Remove the remaining old orchestration + rill-adrift migration + final verification

**Files:**
- Modify: `rill-lang/src/program_runner.rs` — delete `apply`/`engine`/`handle`/`num_inputs`/`num_outputs` (old signal surface).
- Delete/rewrite: `rill-lang/src/program_engine.rs` (ProgramEngine removed; `graph` is not required and its `graph::compile`/`CompiledStream` may be dropped or gated out).
- Modify: `rill-lang/src/program.rs` — remove the signal-only `RillProgram` state (`block_state`, `delays`, `buffers`, `input_buffers`, `pending_outputs`, signal `num_inputs`/`num_outputs` plumbing) and the `Algorithm`/`MultichannelAlgorithm`/`BlockBuiltin` impls; add a public `run_value(&mut self) -> Result<(), ProcessError>` for the retained pure-value test path.
- Modify: `rill-lang/src/backend/interp.rs` — drop `run_block_mimo`'s signal input/output materialization (or keep a value-only `run_value_track` entry); remove `ValueReadInput`/`ValueReadInputs`/`ValueWriteOutput`/`ValueWriteOutputList` instructions.
- Modify: `rill-lang/src/lib.rs` — remove `compile_graph`/`compile_graph_mode`/`compile_program(_mode)`/`compile_program_with_resources(_mode)`; keep `compile(_with*_mode)` (pure-value path) and the new runner entry.
- Modify: `rill-lang/src/runtime.rs` — remove the signal `launch`/`launch_stream` IoDriver wiring.
- Migrate: every pure-value test from `MultichannelAlgorithm::process(&mut prog, &[], …)` + `value_outputs()` to `prog.run_value().unwrap()` + `value_outputs()` (one call-site change per test file).
- Modify: `rill-adrift/Cargo.toml` — drop `graph` from the `rill-lang` feature list (`features = ["dsp"]`).
- Modify: `rill-adrift/src/modular/mod.rs` — replace `ProgramEngine`-based `build_engine` + `ProgramRunner::new(engine, parent_ref)` + `Runtime::launch` orchestration with the new executor (`compile_runner` → `set_args` → `run`); drop the `graph::compile` path.
- Migrate: `rill-adrift/tests/lang_builtins.rs`, `rill-adrift/tests/engine_params.rs`, `rill-adrift/examples/*` (`lang_chiptune`, `record_mic`, `player`, `advanced_player`, `complex_dsl`, `dsl_spectral`) to the new program form / runner. Remove examples using dead Fausto syntax (`complex_dsl`, `dsl_spectral`).
- Test: full verification below.

**Interfaces:**
- Consumes: the new `ProgramRunner`, `run_value` (retained value path), stream ops (Tasks 2-4).
- Produces: a `rill-lang` crate with no signal-transform execution surface; the `-p rill-lang` suite green via the retained value path + the runner; `rill-adrift` building against the new entries.

- [ ] **Step 1: Remove the old orchestration from `rill-lang`**

Delete `ProgramEngine`, the `Algorithm`/`MultichannelAlgorithm` impls, `ProgramRunner::apply`, the signal `RillProgram` state, `compile_graph`/`compile_program*`, and the `runtime.rs` IoDriver launcher. Add `RillProgram::run_value(&mut self) -> Result<(), ProcessError>` (run the value track once; reads `value_outputs()`).

- [ ] **Step 2: Migrate the pure-value test run path**

In every retained pure-value test file, replace `MultichannelAlgorithm::process(&mut prog, &[], …)` (and any direct `process`/`run_block_mimo` call) with `prog.run_value().unwrap()`. Run: `cargo test -p rill-lang --features dsp` — expected: green.

- [ ] **Step 3: Migrate `rill-adrift`**

Drop `graph` from the `rill-lang` dependency features; replace the `modular/mod.rs` orchestration and the tests/examples with the new runner entry points; remove dead-syntax examples. Run: `cargo test -p rill-adrift` — expected: builds and passes (the `graph`-gated `src/graph/**` in `rill-lang` is out of scope and may not compile under `--features graph`).

- [ ] **Step 4: Final verification**

Run: `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp --tests`, `cargo fmt`, `cargo test -p rill-adrift`.
Run: `grep -rn "readInput\|writeOutput\|ProgramEngine\|compile_graph\|Stream \|BackendSpec\|mkStream" rill-lang/src rill-lang/tests rill-adrift/src rill-adrift/tests rill-adrift/examples` — expected: zero hits outside `src/graph/**`.
Confirm the spec §8 checklist: `main = return ()` runs cleanly; STRef isolation + escape; memory + stdio stream round-trips; bounded-memory overflow is a clean capacity error; zero old-surface terms.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m 'refactor(rill-lang): remove signal-transform orchestration; migrate rill-adrift to the IO runner'
```
