# rill-lang: WorldState as the single execution state — external world, IR/world separation

Status: design (builder check complete — awaiting user approval)
Date: 2026-10-18
Branch: `feature/rill-lang-arrays`
Related:
- `docs/superpowers/specs/2026-10-17-rill-lang-io-state-world-streams-design.md` (ancestor — where `WorldState`/`realWorld`, the `World` token, and stream/stdio realization were born)
- `docs/superpowers/specs/2026-10-18-rill-lang-single-run-design.md` (ancestor — interpreter unification: one `run_blocks`/`force`, `run_entry`, `prog.main`)
- `docs/superpowers/plans/2026-10-18-rill-lang-single-run-main-io.md` (in-flight plan; this spec's plan supersedes its remaining tasks 4–14 and re-expresses its runtime surface)
- `docs/superpowers/specs/2026-10-04-rill-core-memory-memoryregion-design.md` (page/chunk memory substrate: `ChunkedVec`, grow-safe arena, `BufferPool`)

## Intent

`main` is (after the in-flight single-run plan) a pure `IO ()` value with no
launch arguments: there is no way to hand a program initial state, and no way
to read its state back out. `test_eval` therefore cannot receive initial state,
cannot expose post-run data, and `set_param` between runs has no surface —
which is exactly why Class-2 value tests (`set_param`-persistence, multi-run
arena survival, param modulation) are currently parked behind `#[ignore]`.

Today `WorldState` (`program.rs:140`) is only streams + stdio. Everything else
— the value arena, `value_regs`/`value_regs_top`, `block_state`/`block_state_next`,
`delays`, the `buffers` table, `builtins` (the runtime `BuiltinInst` instances),
`params`/`params_dirty`/`params_meta`, `frag_cells`, `drops_scratch`, `main`,
`value_error`, `regions`, `st_cells` — lives on `RillProgram`, is sized from the
IR at `new_with` time, and is fused with compilation: compile a program and you
have already built its entire runtime state, which is hit once and thrown away.

The fix, per the user's decision: **split the object model into a compiled
program (`Ir`, pure code) and a living `WorldState` (all execution state)**,
modeled on GHC's `runMainIO` — the initial IO is performed against
`State# RealWorld` — with the difference that rill's world is **externally
creatable and externally observable**: it is configured by the caller, survives
compilation and execution, is passed into `test_eval` as `(ir, world)`, and can
be inspected (arena, buffers, streams, params, result) after a run. This closes
the `main : IO ()` state problem (no arguments ⇒ state must come from outside),
unblocks `set_param` between runs in the same world, and is the natural
foundation for rill-analyzer introspection of the live world.

## Goals

1. **`WorldState` is the single execution state.** It owns everything a run
   touches: the FFI factory registry, the memory management system (value arena,
   buffer table, feedback/delay state), streams/stdio, params (values), value
   registers, call scratch, and ST state — i.e. **all fields currently on
   `RillProgram`** move into it.
2. **Compilation and execution are separate objects and separate phases.** A
   compile step produces `Ir` only (parse → infer → reduce → lower; no runtime
   state). An execution step runs an `Ir` against a world. `test_eval` takes a
   **compiled `Ir` and a `RealWorld`**, not a program.
3. **The world is created externally, with caller-supplied config** (page size,
   buffer budget, `BUF`, memory policy, FFI registry, initial streams/input).
   The world's *config* is never derived from one program's IR.
4. **The world survives compilation and execution** and is **inspectable after**
   a run: arena, buffers, streams/stdio, params, result — initial state can be
   injected before a run, live data read back after it.
5. **Re-execution surface for `set_param`.** A world is re-runnable: set a
   parameter, run again in the same world. This is the Class-2 re-run surface
   that no longer existed after the single-run redesign.
6. **`RillProgram` is removed.** The executable surface is `compile` →
   `ProgramRunner`, where the runner **creates a fresh world and owns it** (the
   "simpler than ST-magic" lifetime option the user chose); `run()` performs the
   initial IO in that world. Exactly one execution function serves both the
   runner path and `test_eval`.
7. **`main : IO ()` stays a static compile-time gate** (user directive, pulled
   forward from the in-flight plan's Task 14): a compile entry literally named
   `main` whose type is not `IO ()` is a compile error. The eval surface never
   selects `main` as an entry, so value expressions bypass the gate.

## Non-goals

- **No ST-magic world lifetime.** GHC can conjure `RealWorld` out of thin air
  (`runRW#`); rill does not replicate the *magic* — the world is an ordinary
  owned object with an ordinary Rust lifetime (the runner owns it, or the caller
  owns it). The GHC model is borrowed for *shape* (initial IO against the world),
  not for *lifetime magic*.
- **No new allocator research.** Alexandrescu-style page/slab allocation for the
  run-time store is *already implemented* in `rill_core::memory` (`ChunkedVec`
  paged slot store; `Arena::alloc` appends a page under `growable`; `BufferPool`
  has the same door). The memory work here is un-gating an existing path, not
  writing one.
- **No channel-collapse.** The residual signal-program channel fields
  (`Ir::num_inputs`/`num_outputs`/`output_channels`/`has_value_out`/`outs`) stay
  put; collapsing them is a separate, deferred step (already recorded in the
  in-flight plan's Task 3 ruling). Architecturally there are no signal programs:
  buffer results are `Value::Buffer(idx)` arena values.
- **No trampoline/tail-call work.** The in-flight plan already defers that.

## Anti-goals

- The world must **not** derive its whole shape from a single program's IR "so
  that one test compiles": the world is a session object with caller-supplied
  configuration. (Exception, below: *structural layout requirements* an IR
  needs — register counts, state-slot counts, tape/buffer tables — are exact
  IR data, not guessed capacity, and are seated per IR.)
- `test_eval` must not smuggle compile-time state into the run (the current
  implementation recompiles `"{defs}\n__rill_eval = ({expr});"` and assembles a
  full `RillProgram` per call). Under this spec compilation is explicit and the
  world is passed in.
- No `unsafe` (rill-lang carries `#![deny(unsafe_code)]`; world lifetime is
  ordinary ownership, so none is needed).

## How it was decided (user decisions, 2026-10-18)

1. **Two objects.** The compiled program (`Ir`/code) and the living world
   (state) are separate objects; `test_eval` accepts **compiled IR + RealWorld**.
2. **All `RillProgram` fields move into `WorldState`** — arena, buffers,
   feedback/delay, builtins (runtime instances), params, registers, call
   scratch, ST state, streams/stdio — so the world can outlive compilation and
   execution and can be passed into `test_eval`.
3. **World config comes from the caller**, not from a program's IR.
4. **The FFI registry lives in the world.** Foreign built-ins are resolved and
   instantiated at execution/seat time against the world's registry, not once in
   `new_with`.
5. **Page-based allocation; capacity is not precomputed.** Start with one
   fixed-size page and let the store grow page-by-page (Alexandrescu: page
   allocation is the right compromise between dynamic allocation and speed).
   This un-gates the existing `ChunkedVec`/`growable` door.
6. **`RillProgram` is removed.** `ProgramRunner` creates a fresh world and owns
   it ("можно и проще" — the ST-magic option was considered and declined).
7. **`main : IO ()` gate stays at compile time.** Entry named `main` must infer
   as `IO ()`; value-`main` (`main = 1.0 + 2.0`) is a compile error on the
   compile front-end. The eval surface never uses `main` as an entry.
8. **Process (assistant's call, user confirmed the design as a whole):** this is
   a separate spec + plan **on top of** the in-flight single-run plan; the
   remaining tasks of that plan are re-expressed against this architecture
   (most notably its `test_eval`/`RillProgram`/runner surface).
9. **The params map is `rill_core::Params` — no new type.** The world's
   named-value map is literally the existing `Params` (`param.rs:433`):
   `sample_rate: f32` field + `parameters: HashMap<String, ParamValue>`, with
   `get`/`insert`/`remove`/`contains`/`get_f32`, and its companion
   `ParameterWrite` trait (`write_parameter(name, value)` /
   `read_parameter(name)`). `Params` is the **shared param bag of the whole
   ecosystem**, not a rill-lang invention: the identical type feeds
   `GraphBuilder::add_node(type, &Params)` (`graph/builder.rs:91`), graph
   serialization (`HashMap<String, ParamValue>`), and
   `AutomatonFactory::construct(id, &Params, &ServoTarget)` and `spawn_async`
   (rill-patchbay `automaton/factory.rs`); automatons step on `ParamValue`s
   (`step(current: &ParamValue, …) -> ParamValue`) and servos emit
   `CommandEnum::SetParameter { anchor, parameter: ParameterId, value }`.
   As the user notes, `Params` was designed to work in tandem with the actor
   model — the same model the `?name` DSL syntax came from. The world owns a
   `Params`; `?name` reads and `set_param` writes it **by name**;
   `param_index` (index plumbing) is gone. `?sample_rate` is the
   `Params::sample_rate` field, not a separate world record and not a compile
   parameter — a signal-aware world carries it, a world running no signal
   program never touches it.

## The shape of the change

### Two objects

```
compile(defs…) ──▶ Ir (pure code; no runtime state)      ──▶ seat+exec ──▶ WorldState
                                                                 ▲            │
                                            runner creates / caller passes    │ inspect:
                                                                              │ arena, buffers,
                                                                              │ streams, params
```

- `Ir` keeps exactly what it has today: code + *structural layout required by
  the code* — `value_blocks`, `value_entry`, `result_reg`, `num_value_regs`,
  `max_call_regs`, `value_funcs`, `fragments`, `state: StateLayout`,
  `value_state: ValueLayout`, `num_buffers`, `input_buffers`, `tapes`
  (capacities), `builtins: Vec<BuiltinInstance>` (call-site descriptors),
  `params: Vec<ParamDef>` (name/default/range), `main_io_unit`.
- `WorldState<T, const BUF>` owns the *runtime* state: value arena
  (`Arena<Value>`), `value_regs`/`value_regs_top`, `block_state`/
  `block_state_next` (`FixedBuffer<T, BUF>`), `delays`, `buffers`
  (`Vec<RegionHandle<T>>`), `block_len`, `builtins: Vec<BuiltinInst<T>>` (the
  instantiated runtime objects), `params: rill_core::Params` (`sample_rate` +
  `HashMap<String, ParamValue>`, values only) alongside `params_dirty`,
  `value_state`, `cell_stack`, `frag_cells`/`frag_cells_base`,
  `drops_scratch`, `main` (the captured result ref), `value_error`, `regions`,
  `st_cells`, streams/ stdio — **and the `ffi: ForeignRegistry<T>`** it
  resolves built-ins from.

  `BUF` stays a const generic on the world because `block_state` uses
  `FixedBuffer<T, BUF>` (RT-safe fixed-size feedback); the *rest* of the world
  config (page size, buffer budget, memory policy, FFI registry, initial stdio
  input) is runtime data supplied by the caller.

### Seat: the code-meets-memory boundary

`Ir` and `WorldState` meet through a **seat** operation — the world assembles
itself *for* a given IR against its external config:

```
fn seat(&mut self, ir: &Ir) -> Result<(), CompileError>
```

What seat does is exactly today's `RillProgram::build` (`program.rs:627`), with
three differences driven by the user's decisions:

1. **The arena is not sized from the IR.** Instead of
   `Arena::with_capacity(ir.value_state.capacity)` the world's arena starts at
   **one fixed-size page** (page size from world config) and grows by appending
   pages on demand (the `ChunkedVec` door). `BufferPool` is constructed with the
   world's buffer budget and the world's growable policy (the `growable-arena`
   cargo feature becomes the world's memory-policy flag, not a build-time gate).
2. **Built-ins resolve against the world's registry.** `build_builtin`
   (`program.rs:334`) is called with `world.ffi` instead of a per-assembly
   `foreign: Option<…>`; the legacy `Registry` path disappears with the in-flight
   plan's Task 14 deletion. An unresolvable built-in name is a `CompileError` at
   seat time (there is no registry at compile time anymore).
3. **Params seed the world's `Params` by name — external pre-seed survives.**
   `Ir::params` holds the *declarations* (`ParamDef`: name, default, min/max;
   conflicting redeclaration is a compile-time error at `intern_param`,
   `lower.rs:7104`). Seat walks them: a name already present in
   `world.params.parameters` is **left untouched** (the caller pre-seeded it —
   that is initial-state injection); a name absent from the map is inserted
   with the declaration's default. `?name` reads and `set_param` writes
   `world.params` by name at run time; `?sample_rate` reads
   `world.params.sample_rate`.

The *structural* tables are NOT guesses and are NOT world-config: `value_regs`
is sized `ir.num_value_regs + ir.max_call_regs`, `block_state` is sized
`ir.state.block_state_slots`, the buffer table is carved per `ir.num_buffers`,
tapes per `ir.tapes`, delays per `ir.state.delay_lens`. These are exact IR
requirements — seating them is not "computing capacity", it is reading the
program's layout. (User decision 5 is about the *arena's dynamic store*: that
one starts at a page and grows.)

A world may be **re-seated** for a different IR (a long-lived world running
several compiled programs): structural tables resize to the next IR's layout;
the arena grows monotonically (pages are never returned — a session arena).
Whether resizing-only or grow-only matters for program-equivalence tests is a
builder decision (see *Left to the builder*).

### Execution: one function for both paths

- `exec_entry(ir, &mut world)` — today's `run_entry` (`interp.rs:65`) moved onto
  the world: push builtin params, set `block_len`, release the previous result
  ref, walk `ir.value_blocks` at base 0 (`run_blocks`), swap block state, force
  `value_regs[result_reg]` into `world.main`, drain drops, latch
  `world.value_error`. `ValueReadParam` (`interp.rs:1562`) no longer reads
  `prog.params[idx]` — it resolves **by name** through `world.params`
  (`read_parameter`/`get`), so a `set_param` written between two runs in the
  same world is what the next run reads.
- `run_main_io(ir, &mut world)` — today's `run_main_io` (`interp.rs:255`) moved
  onto the world: `exec_entry`, then the `unIO` projection, `WORLD_TOKEN`
  application, pair-force and the `main : IO ()` payload contract. The runner
  and `test_eval` both land here (runner: entry `"main"`; eval: entry
  `__rill_eval`, no `main`-gate).

Both take `&Ir` (not ownership), so one compiled program can be executed many
times in one world, and many compiled programs can be executed in one world —
the two reuse directions `set_param`-between-runs and multi-program-session
need.

### The public surface

- `WorldState` accessors (moved from `RillProgram`): `result()`,
  `arena()`, `copy_buffer(r, dst)`, `set_param(name, value)` /
  `param(name)` / `params_meta()` (on the world so initial params can be set
  before a run and read after), `inject_stdin`, `init`/`reset` (session
  teardown semantics). Addressing is **by name** — `set_param` writes
  `world.params` (`insert` / `ParameterWrite::write_parameter`), `param` reads
  it (`get` / `read_parameter`). `param_index` plumbing is removed.
  Whether metadata (`params_meta`) lives on the world or stays on `Ir` is a
  builder call (see below).
- **Eval surface (compilation explicit):**
  - `compile_eval(defs, expr, mode) -> Result<Ir, CompileError>` —
    builds `"{defs}\n__rill_eval = ({expr});"`, compiles through the existing
    entry-name pipeline with the internal marker; no `main` gate. No
    `sample_rate` parameter — frequency, when a program reads `?sample_rate`,
    comes from the world's `Params` at run time.
  - `test_eval(ir, &mut world) -> Result<(), CompileError>` — seats `ir` into
    `world`, runs `exec_entry`; a run `ProcessError` maps to
    `CompileError::Unsupported`. The run result and all live state are read from
    `world` afterwards.
- **Runner surface:** `compile::<T, BUF>(src, ffi) -> Result<ProgramRunner, CompileError>`
  (Complete mode default, `main : IO ()` gate on the `"main"` entry) creates a
  fresh `WorldState` from default config and seats the IR into it;
  `ProgramRunner { ir: Ir, world: WorldState }`; `run()` = `run_main_io(ir, world)`;
  console accessors (`stdout`/`stderr`) read `world.stdout`/`world.stderr`;
  `set_stdin` delegates to `world.inject_stdin`. The caller who needs a specific
  sample rate pre-seeds it on `world.params.sample_rate` (or supplies a
  `WorldConfig`) before `run()` — the compiler never sees `sr`.
  `RillProgram` is gone.
- The `IO ()` gate: after lowering, in `compile`'s front-end, `if !ir.main_io_unit → Err`
  ("`main` must have type `IO ()`"). Pinned by the compile-gate tests.

### What happens to the current in-flight work

- The in-flight plan's Tasks 1–2 (committed `43a5fcd`) stay as-is: `run_blocks`,
  `run_entry`, `prog.main`, `result`/`copy_buffer`, `test_eval` entry surface.
- Task 3's uncommitted edits (main is a pure `IO ()`; String launch-args
  machinery deleted; `def_schemes`/entry-name threading; rill-adrift `set_args`
  dropped) stay — they are prerequisites this spec builds on, and the pending
  Task-3 gate/migration directives remain valid; only the *target shape* of the
  migrated `test_eval` calls changes to `(ir, world)` once this spec lands.
- The remaining tasks of the in-flight plan (4–14) are **re-expressed** by this
  spec's plan: `test_eval` signature, `RillProgram` removal, runner ownership,
  the compile-gate, and the Registry deletion all happen against the world
  architecture. The `compile` FFi-only direction (Task 5/14) is unchanged and
  becomes world-owned.
- rill-adrift: `modular`/examples migrate to the new `compile` +
  `ProgramRunner` shape; `full_registry` deletion (Task 14) continues to apply.

## Consequences to keep an eye on

- **Result refs are world-owned.** `world.main` pins its subtree (rc ≥ 1) across
  runs; `exec_entry` releases the previous result before the next run allocates —
  the build-time capacity bound comment ("at most one run's live set") becomes a
  world-session invariant. With a page-growable arena, repeated runs across a
  session may grow pages instead of failing — the Class-2 "repeated runs must
  not exhaust the fixed arena" worry (in-flight Review Focus 4) softens into a
  memory-policy question.
- **Built-in existence is checked at seat time.** Tests that assert
  compile errors for unknown built-in names currently do so at assembly; under
  this spec the same error surfaces from `seat` (still before any effect runs).
- **Params are addressed by name at run time.** `ValueReadParam`
  (`interp.rs:1562`) currently reads `prog.params[idx]` — a positional index
  into a `Vec<ParamValue>`. Under this spec the world's `Params` is the single
  store and reads/writes go through name-keyed `get`/`insert`
  (`Params`/`ParameterWrite`). The instruction may keep an IR-local index
  into `Ir::params` (the `ParamDef` declaration table, which carries the name)
  or carry the name directly — a builder decision for the plan; either way the
  runtime store is the world's name-keyed map, so `set_param("cutoff", 0.3)`
  between two runs is exactly what the second run reads.
- **`smooth` computes its coefficient at run time.** The only lowering consumer
  of the sample rate is `smooth` (`lower.rs:1944/6598`): instead of the compiler
  baking `a = 1 − exp(−1/(τ·sr))` into the IR, the operator reads `?sample_rate`
  at run time and computes the coefficient per run — the compiler never needs
  `sr` again. Precedent: `lofi ?sr=44100` (`parser.rs:2395`).
- **The world's `Params` is the same type the actor model writes through.**
  `Params`/`ParameterWrite` (and `SetParameter` in `rill_core::queues`, which
  rill-patchbay servos emit) already speak `ParamValue`s; a world backed by a
  graph engine shares the same store. That is the seam rill-analyzer and
  patchbay-facing control can later use to inspect or drive a live world.
- **A world parameter is ecosystem-addressable by name, and the name
  conventions on that boundary are `ParameterId`-validated.** The graph world
  (`SetParameter.parameter: ParameterId`, `write_parameter(name, …)` names,
  `ServoTarget.target_param: String`) validates names: ≤ 64 chars,
  `[a-zA-Z][a-zA-Z0-9_.]*` (`rill-core/src/traits/param.rs:21`). The world's
  `Params` itself accepts whatever the DSL produces (`?`-names are not
  `ParameterId`-constrained today); when a world parameter routes into graph
  construction or a `SetParameter`, the `ParameterId` rules apply at that
  boundary — a future concern, noted so the world API does not paint itself
  into a corner (e.g. by baking in index addressing).
- **The `World` token stays opaque.** `IO a = { unIO: World -> Pair World a }`
  and the `WORLD_TOKEN` value are unchanged; only the *state behind* the token
  grows from streams to the whole world. The runner/`test_eval` never see the
  token — they pass the world and read it back.
- **`Rc<RefCell<WorldState>>` vs plain ownership.** Today `RillProgram.world` is
  `Rc<RefCell<WorldState>>` because effects mutate the shared realization. Once
  the world *is* the program's whole state and execution receives `&mut world`,
  the indirection may be removable — a builder decision (see below). The
  single-threaded, `Send`-free model stays either way.

## Left to the builder

- **Exact method names/visibility** for the world accessors (`result`/`arena`/
  `copy_buffer`/`params*`/`inject_stdin`/`init`/`reset`), and whether
  `params_meta` lives on the world (for `params_meta()` inspection of a live
  world) or stays on `Ir` (lookup by name needs `ir.params`). Sensible default:
  expose on the world, sourced from the seat.
- **How `ValueReadParam` names its target.** Keep the positional `idx` into
  `Ir::params` (the name lives in `ParamDef`, so the seat binds name→value once)
  or hoist the name into the instruction. Sensible default: keep `idx` — the
  IR stays as-is and the world store remains name-keyed regardless.
- **Ownership shape of the world** — plain `WorldState` field vs the current
  `Rc<RefCell<…>>` realization handle. Start from plain ownership; the plan
  should verify effect sites (`write`/`read`/`makeInputStream`/…) with
  borrow-checker feedback.
- **Seat granularity and multi-IR world reuse.** Whether a world resizes
  structural tables per re-seat (a true session running several programs) or is
  effectively per-IR in this iteration. Sensible default: allow re-seat with
  resize of structural tables; arena pages grow monotonically.
- **Where the world's default config lives** for the runner path (a
  `Default`/`WorldConfig::default()` the runner uses when the caller passes
  none).
- **`test_eval` mode plumbing**: `compile_eval` carries `Mode`
  (`test_eval_with_mode` parity), or the pragma in `defs` alone is honored.
  Sensible default: explicit `mode` param like today.

---