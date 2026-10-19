//! IR evaluator: the block-only executor.

use rill_core::buffer::FixedBuffer;
use rill_core::math::vector::ScalarVector4;
use rill_core::math::Transcendental;
use rill_core::traits::MultichannelAlgorithm;
use rill_core::traits::ProcessError;

use crate::arena::{Arena, ArenaRef, ThunkState, Value};
use crate::ir::{
    BinArith, CmpOp, FragmentIr, Ir, LogicOp, UnOp, ValueBlock, ValueBuiltinOp, ValueInstr,
    ValueTerm,
};
use crate::program::{
    StCell, StreamHandle, WorldState, MAX_BUILTIN_CHANNELS, STREAM_HANDLE_TAG, WORLD_TOKEN,
};

pub(crate) fn param_to_f64(pv: &rill_core::traits::ParamValue) -> f64 {
    match pv {
        rill_core::traits::ParamValue::Float(v) => *v as f64,
        rill_core::traits::ParamValue::Int(v) => *v as f64,
        _ => 0.0,
    }
}

/// The runtime value of the parameter declared at `idx` in
/// [`Ir::params`](crate::ir::Ir::params), resolved **by name**: the IR keeps a
/// positional declaration table (name, default, range), but the value store is
/// the world's name-addressed [`rill_core::Params`]. `"sample_rate"` reads the
/// rate field; every other name reads the named map.
pub(crate) fn param_at<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &WorldState<T, BUF>,
    idx: usize,
) -> f64 {
    match ir.params.get(idx) {
        Some(def) => param_to_f64(&world.param_value(&def.name)),
        None => 0.0,
    }
}

pub(crate) fn push_builtin_params<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
) {
    let n = ir.builtins.len();
    for instance in 0..n {
        let blen = ir.builtins[instance].param_bindings.len();
        for k in 0..blen {
            let (arg_pos, param_idx) = ir.builtins[instance].param_bindings[k];
            if !world.params_dirty[param_idx] {
                continue;
            }
            // `param_bindings` stays `(arg_pos, param_idx)` (an IR shape — the
            // positional slot of the built-in argument); the VALUE is read by
            // the declared name from the world's bag.
            let v = world.param_value(&ir.params[param_idx].name);
            world.params_dirty[param_idx] = false;
            match &mut world.builtins[instance] {
                crate::program::BuiltinInst::Block(b) => b.set_param(arg_pos, &v),
                crate::program::BuiltinInst::MultichannelBlock(b) => b.set_param(arg_pos, &v),
            }
        }
    }
}

/// Run a program: execute its top-level value track at base 0 once to
/// completion and capture the result register into [`world.main`].
///
/// This is the single execution mode of a rill-lang program. It evaluates ALL
/// of the program's value-level semantics (whole-buffer `Value::Buffer` ops,
/// value ops, effectful `IO` actions built as values) by walking the top-level
/// track — the program's own `value_blocks` at base 0, exactly like a fragment
/// body but WITHOUT a call frame. There is NO env record, NO `run_call`, NO
/// fragment wrapper: the entry is a plain top-level value-track binding, and
/// architecturally the language has no signal programs (0/1-argument lambdas
/// only) — the run is one block-walk that ends at the entry's terminal. There
/// is NO per-block signal tick loop: the walk runs once to its terminal and
/// the result register is captured into the world's `main`.
/// Returns a `ProcessError` when the run latched one (a collection capacity
/// overflow — a user error, not a build bug).
///
/// The run length is [`BUF`]: the whole-buffer value operations fill a full
/// block, and the value state reads (`smooth`, delay lines) read `block_len`,
/// so it must equal the length the block was written with.
pub(crate) fn exec_entry<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
) -> Result<(), ProcessError> {
    push_builtin_params(ir, world);
    world.block_len = BUF;

    // Release the previous run's captured `world.main` BEFORE this run
    // allocates. The captured terminal pins its whole subtree across runs (the
    // root is kept at rc >= 1), and a closure result additionally pins its
    // captured env record — so the old tree must be freed before this run's
    // allocations can reuse its slots (the build-time capacity bound assumes
    // at most one run's live set at a time). The new terminal is captured at
    // run end.
    if let Some(r) = world.main.take() {
        world.arena.drop_ref(r);
    }

    // Value-track phase (this run): walk the program's own top-level blocks at
    // base 0 — no env record, no call frame, no per-call unwind. The entry is
    // a 0-argument constant-like binding (architecturally lambdas have only
    // 0/1 args), so there is no frame to reserve for it: the top-level
    // registers occupy `0..num_value_regs` and the watermark is pinned there so
    // nested fragment calls (function dispatch inside the track) reserve their
    // scratch ABOVE the top-level registers, unwinding back to this ceiling.
    // The IR is borrowed immutably while the world is borrowed mutably, so the
    // block-walk reads `ir.value_blocks` in place. A latched value error (a
    // collection capacity overflow) is propagated only AFTER the run-end
    // cleanup below — the registers, block-state swap and terminal release all
    // still run, so a repeatedly erroring program cannot leak the fixed arena
    // or desync its feedback state.
    let mut drops = std::mem::take(&mut world.drops_scratch);
    let drops_mark = drops.len();
    world.value_regs_top = ir.num_value_regs;
    let entry = ir.value_entry;
    run_blocks(ir, world, &ir.value_blocks, entry, 0, &mut drops);
    let value_res = match world.value_error.take() {
        Some(err) => Err(err),
        None => Ok(()),
    };

    // Apply the block-level feedback shadow copy (double-buffer swap).
    world.swap_block_state();

    // Capture the run's terminal into `world.main`: `value_regs[result_reg]`
    // holds the entry body's value (owned by the register slot). Copy it out
    // (a fresh owner via `copy`), force it to WHNF (a lazy terminal register
    // holding a thunk materializes its head here) and copy the head (rc++) so
    // it survives the register clear below. The previous terminal was already
    // released at the start of this run (see above), so nothing leaks across
    // runs.
    let result: Option<ArenaRef> = match world.value_regs.get(ir.result_reg).copied().flatten() {
        Some(sr) => copy_owned(ir, world, sr),
        None => None,
    };
    if let Some(r) = result {
        let head = force(ir, world, r, &mut drops);
        if let Ok(c) = world.arena.copy(head) {
            world.main = Some(c);
        }
        world.arena.drop_ref(r);
    }
    for r in drops.drain(drops_mark..) {
        world.arena.drop_ref(r);
    }
    world.drops_scratch = drops;

    // A resource exhaustion can be latched DURING terminal forcing (a lazy
    // terminal register recurses past `max_call_regs` / the arena pool while
    // its head is materialized here, after the fragment already returned).
    // Surface it as the run's error instead of a silent `Bottom`.
    let latched: Option<Result<(), ProcessError>> = match &value_res {
        Ok(()) => world.value_error.take().map(Err),
        Err(_) => None,
    };

    // Release this run's per-run value registers now that the terminal has
    // been read (it was copied above). The value-state is NOT cleared: it is
    // the run-to-run delay store, and `ValueStateWrite` already drops the
    // previous ref when overwriting a slot.
    world.clear_value_regs(ir.num_value_regs);
    match latched {
        Some(Err(err)) => Err(err),
        _ => value_res,
    }
}

/// Test-visible top-level-track driver: run the program's own value blocks at
/// base 0 WITHOUT `exec_entry`'s run-end behavior (no register clear, no
/// `world.main` capture).
///
/// The interpreter's internal tests drive the program's own registers directly
/// (seeding `world.value_regs`, asserting on `arena.get(0)`-style absolute
/// slots), so they need the raw top-level block walk previously provided by
/// the deleted `run_value_track`. The IR is borrowed immutably (the world
/// mutably), so the walk reads `ir.value_blocks` in place; drain drops and
/// surface the latched value error. Only the tests use this — the production
/// path is [`exec_entry`].
#[cfg(test)]
pub(crate) fn run_main_blocks_for_tests<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
) -> Result<(), ProcessError> {
    let mut drops = std::mem::take(&mut world.drops_scratch);
    let mark = drops.len();
    let entry = ir.value_entry;
    run_blocks(ir, world, &ir.value_blocks, entry, 0, &mut drops);
    for r in drops.drain(mark..) {
        world.arena.drop_ref(r);
    }
    world.drops_scratch = drops;
    if let Some(err) = world.value_error.take() {
        return Err(err);
    }
    Ok(())
}

/// Execute the value track: walk the program's value-track blocks from
/// the entry block, following each block's terminator, allocating and freeing
/// values in the program's fixed arena.
///
/// Every value register, value-state slot, and cell holds ONE ownership of the
/// arena ref it stores (the RC is already counted for that ref). A store that
/// places an already-owned ref into a second location `copy`s it first
/// (RC++); a store that MOVES a ref clears the source (`None`). Drops are
/// deferred to the end of the track so a drop cannot free a slot mid-track
/// that a later instruction reuses, and the release order is deterministic.
/// Walk a run of value blocks: execute each block's instructions against the
/// register base, then dispatch the block terminator. Owns no drops discipline
/// and no register cleanup — callers drain drops above their own mark and
/// manage register state. This is the single block-walker shared by the
/// top-level value track and every fragment call.
fn run_blocks<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    blocks: &[ValueBlock],
    entry: usize,
    base: usize,
    drops: &mut Vec<ArenaRef>,
) {
    let mut cur = entry;
    loop {
        debug_assert!(
            cur < blocks.len(),
            "run_blocks fall-off-the-end: block {cur} does not exist"
        );
        let Some(b) = blocks.get(cur) else { break };
        for i in &b.instrs {
            exec_value_instr(ir, world, i, base, drops);
        }
        cur = match b.term {
            ValueTerm::Fallthrough(n) => n,
            ValueTerm::Branch { cond, then, els } => {
                if value_reg_is_true(ir, world, cond + base) {
                    then
                } else {
                    els
                }
            }
            ValueTerm::BranchCtor {
                slot,
                ctor,
                then,
                els,
            } => {
                if value_reg_ctor(ir, world, slot + base) == Some(ctor) {
                    then
                } else {
                    els
                }
            }
            ValueTerm::Halt => break,
        };
    }
}

/// Run a program's `main : IO ()` to completion against the realized
/// [`WorldState`](crate::program::WorldState).
///
/// Evaluates `main`'s value through the value track (this is where the `IO`
/// action — a `{ unIO: World -> Pair World () }` record — is built), projects
/// `unIO`, applies it to the realized `World` token, and forces the resulting
/// pair so every effect the action requests actually runs. The final token is
/// discarded; the effects' observable state stays in the program's world
/// (`stdout`/`stderr` and the stream table).
///
/// A program whose `main` is not an executable `IO` value, or whose effects
/// exhaust the fixed arena, yields a clean [`ProcessError`].
pub(crate) fn run_main_io<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
) -> Result<(), ProcessError> {
    // Run one value-track pass with no signal outputs: `main`'s value (the IO
    // action) lands in `world.main`.
    exec_entry(ir, world)?;

    let mut drops = std::mem::take(&mut world.drops_scratch);
    let mark = drops.len();
    let mut applied = false;
    if let Some(action) = world.main {
        let action = force(ir, world, action, &mut drops);
        // `IO a = { unIO: World -> Pair World a }`: field 0 is the computation.
        let un_io = match world.arena.get(action) {
            Some(Value::Record(fields)) => fields.first().copied(),
            _ => None,
        };
        if let Some(un_io) = un_io {
            if let Some(token_ref) = alloc_owned(ir, world, Value::Opaque(WORLD_TOKEN)) {
                let pair = apply_callable(ir, world, un_io, token_ref, &mut drops);
                // The action only borrowed the token.
                world.arena.drop_ref(token_ref);
                if let Some(pair) = pair.and_then(|p| force_owned(ir, world, p, &mut drops)) {
                    // Force the action's result while its parts are still
                    // alive: leaving the run is a strict boundary, so the
                    // final effect must be materialized before the pair (and
                    // the closure environment it pins) is released.
                    let payload = match world.arena.get(pair) {
                        Some(Value::Record(fields)) => fields.get(1).copied(),
                        _ => None,
                    };
                    applied = payload.is_some();
                    if let Some(payload) = payload {
                        let payload = force(ir, world, payload, &mut drops);
                        // `main : IO ()`: the contract is on the action's
                        // RESULT, not merely on it being an action. A program
                        // whose `main` yields a non-`()` value (e.g.
                        // `main = return 1.0`) is malformed even though it is
                        // an `IO` value, so report it cleanly instead of
                        // silently discarding the payload.
                        //
                        // The value track's closure-cell encoding erases `()`
                        // to `Float(0.0)` (a cell's un-set state is `Void`, so
                        // a stored unit reads back as 0.0), so the erased form
                        // is accepted ONLY when `main` is statically `IO ()`
                        // (`ir.main_io_unit`): `main = do { …; return 0.0 }`
                        // is typed `IO Float` and is still rejected.
                        let payload_is_unit = match world.arena.get(payload) {
                            Some(Value::Void) => true,
                            Some(Value::Float(f)) => ir.main_io_unit && *f == 0.0,
                            _ => false,
                        };
                        if !payload_is_unit {
                            latch_fault(ir, world,
                                "`main` must have type `IO ()`: the final action's result is not `()`",
                            );
                        }
                    }
                    world.arena.drop_ref(pair);
                }
            }
        }
    }
    for r in drops.drain(mark..) {
        world.arena.drop_ref(r);
    }
    world.drops_scratch = drops;

    if let Some(err) = world.value_error.take() {
        return Err(err);
    }
    if applied {
        Ok(())
    } else {
        Err(ProcessError::processing(
            "`main` is not an executable `IO ()` action",
        ))
    }
}

/// Latch a value-track resource exhaustion as a user-facing `ProcessError`
/// (the plan's contract: exhaustion is a latched error, RT-safe — one `Option`
/// write, no panic, no silent `None`). The first latch wins; later exhaustions
/// are no-ops. The modern runner takes the latched error at run end.
fn latch_exhaustion<T: Transcendental, const BUF: usize>(
    _ir: &Ir,
    world: &mut WorldState<T, BUF>,
    what: &'static str,
) {
    if world.value_error.is_none() {
        world.value_error = Some(ProcessError::processing(format!(
            "value-track resource exhausted: {what}"
        )));
    }
}

/// Latch a value-track FAULT as a user-facing `ProcessError`: an internal
/// invariant violation (a primitive effect with an unbound operand), as opposed
/// to [`latch_exhaustion`]'s resource limits. First latch wins.
fn latch_fault<T: Transcendental, const BUF: usize>(
    _ir: &Ir,
    world: &mut WorldState<T, BUF>,
    what: &str,
) {
    if world.value_error.is_none() {
        world.value_error = Some(ProcessError::processing(format!(
            "value-track fault: {what}"
        )));
    }
}

/// Whether a value register holds `Bool(true)`. Non-Bool/`None` is `false`
/// (defensive; static typing guarantees a Bool here).
fn value_reg_is_true<T: Transcendental, const BUF: usize>(
    _ir: &Ir,
    world: &WorldState<T, BUF>,
    reg: usize,
) -> bool {
    matches!(
        world
            .value_regs
            .get(reg)
            .copied()
            .flatten()
            .and_then(|r| world.arena.get(r)),
        Some(Value::Bool(true))
    )
}

/// The constructor tag of a value register's sum value, if any.
fn value_reg_ctor<T: Transcendental, const BUF: usize>(
    _ir: &Ir,
    world: &WorldState<T, BUF>,
    reg: usize,
) -> Option<u32> {
    match world
        .value_regs
        .get(reg)
        .copied()
        .flatten()
        .and_then(|r| world.arena.get(r))
    {
        Some(Value::Sum(c, _)) => Some(*c),
        _ => None,
    }
}

/// Allocate a slot holding `v`, taking over the ownership of `v`'s child refs.
///
/// The caller must have counted those children (via `copy` / `read_field_refs`)
/// so the slot owns them independently. Returns `None` on capacity exhaustion,
/// releasing the not-yet-owned children so the fixed capacity stays balanced.
///
/// Deliberately does NOT fall back to `Some(0)`: slot 0 is a valid ref, so
/// conflating "arena full" with a real slot would silently corrupt data.
/// Capacity is computed conservatively at build time, so exhaustion means a
/// lowering bug — `debug_assert!` flags it in debug builds and the `None`
/// register is a detectable no-op in release. No child refs are cloned on the
/// happy path: `Arena::alloc` returns the value on failure so the children are
/// released without a separate bookkeeping `Vec`.
fn alloc_owned<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    v: Value,
) -> Option<ArenaRef> {
    match world.arena.alloc(v) {
        Ok(r) => Some(r),
        Err((_, v)) => {
            drop_value_children(ir, world, &v);
            latch_exhaustion(ir, world, "value arena capacity");
            None
        }
    }
}

/// Resolve a value register's `Value::Buffer` handle index, falling back to the
/// register-index convention when the register is empty or holds a non-Buffer
/// value.
///
/// The fallback is the ABSOLUTE register index `reg + base` whenever the buffer
/// table covers it (the normal program-level case: 1-to-1 signal lowering and
/// `Ir::num_buffers == Ir::next_value_reg`), so a fragment-relative caller can
/// no longer silently address a different program register's buffer.
///
/// `Ir::num_buffers` counts PROGRAM-level registers only: a fragment's scratch
/// registers are numbered from 0 in the fragment IR and offset by `base` at run
/// time, so `base + reg` can exceed the table. Signal-lowering buffer ops inside
/// a fragment therefore keep addressing their fragment-relative index (the
/// established convention); the guard keeps that from becoming an out-of-bounds
/// table access.
fn buffer_index<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &WorldState<T, BUF>,
    reg: usize,
    base: usize,
) -> usize {
    match world.value_regs.get(reg + base).copied().flatten() {
        Some(r) => match world.arena.get(r) {
            Some(Value::Buffer(idx)) => *idx,
            _ => buffer_index_fallback(ir, world, reg, base),
        },
        None => buffer_index_fallback(ir, world, reg, base),
    }
}

/// The register-index fallback for [`buffer_index`]: absolute `reg + base` when
/// the buffer table is large enough, else the fragment-relative `reg`.
fn buffer_index_fallback<T: Transcendental, const BUF: usize>(
    _ir: &Ir,
    world: &WorldState<T, BUF>,
    reg: usize,
    base: usize,
) -> usize {
    let abs = reg + base;
    if abs < world.buffers.len() {
        abs
    } else {
        reg
    }
}

/// Recount (RC++) every child of a value for a fresh slot that will own them
/// independently of the original owner.
fn recount_children<T: Transcendental, const BUF: usize>(
    _ir: &Ir,
    world: &mut WorldState<T, BUF>,
    v: &Value,
) {
    match v {
        Value::Record(fields) | Value::Sum(_, fields) => {
            for f in fields {
                _ = world.arena.copy(*f);
            }
        }
        Value::Newtype(inner) => {
            _ = world.arena.copy(*inner);
        }
        Value::Callable { env, cell, .. } => {
            _ = world.arena.copy(*env);
            if let Some(c) = cell {
                _ = world.arena.copy(*c);
            }
        }
        Value::BuiltinCallable { env, .. } => {
            _ = world.arena.copy(*env);
        }
        Value::List { elems } => {
            for e in elems {
                _ = world.arena.copy(*e);
            }
        }
        Value::Map { pairs, .. } => {
            for (k, v) in pairs {
                _ = world.arena.copy(*k);
                _ = world.arena.copy(*v);
            }
        }
        Value::Set { elems } => {
            for e in elems {
                _ = world.arena.copy(*e);
            }
        }
        Value::Array { bounds, elems } => {
            _ = world.arena.copy(*bounds);
            for e in elems {
                _ = world.arena.copy(*e);
            }
        }
        // A lazy callable owns its memo cell; a forced cell owns its result.
        // Mirroring `visit_refs` keeps the recount accounting balanced when a
        // callable/cell value is copied into a fresh slot.
        Value::ThunkCell(ThunkState::Forced(r)) => {
            _ = world.arena.copy(*r);
        }
        // A cons cell owns its head and (lazy) tail.
        Value::Cons(h, t) => {
            _ = world.arena.copy(*h);
            _ = world.arena.copy(*t);
        }
        // An unforced/forcing/bottom cell and bottom itself own nothing.
        Value::ThunkCell(_) | Value::Bottom | Value::Nil => {}
        // Leaf values own no children.
        _ => {}
    }
}

/// Copy a SLOT's value into a fresh slot: pooled payload copy + child recount.
///
/// Unlike [`alloc_copy`], which takes a `&Value` (used for values not owned by
/// the arena), this takes the slot ref so no `&Value` borrow of the arena is
/// held while the pool is mutated — the borrow-checker-safe RT path.
fn alloc_copy_slot<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    r: ArenaRef,
) -> Option<ArenaRef> {
    let cloned = match world.arena.clone_pooled(r) {
        Ok(v) => v,
        Err(_) => return None,
    };
    recount_children(ir, world, &cloned);
    alloc_owned(ir, world, cloned)
}

/// Allocate a fresh slot holding an independent copy of `v`.
///
/// The new slot counts its own refs on `v`'s children (RC++ per child), so the
/// caller may keep `v` — its register retains its own ownership. Returns `None`
/// on exhaustion, undoing the recounts so nothing leaks. Container payloads are
/// copied into pooled buffers (no per-tick heap allocation).
fn alloc_copy<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    v: &Value,
) -> Option<ArenaRef> {
    let cloned = pool_clone_value(ir, world, v);
    recount_children(ir, world, &cloned);
    match world.arena.alloc(cloned) {
        Ok(r) => Some(r),
        Err((_, cloned)) => {
            // No slot was created; undo the recounts above so the original
            // value keeps exclusive ownership of its children. Never `Some(0)`.
            drop_value_children(ir, world, &cloned);
            latch_exhaustion(ir, world, "value arena capacity");
            None
        }
    }
}

/// Share `r` into a new owner (RC++). `None` on arena error — never `Some(0)`.
fn copy_owned<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    r: ArenaRef,
) -> Option<ArenaRef> {
    match world.arena.copy(r) {
        Ok(c) => Some(c),
        Err(_) => {
            latch_exhaustion(ir, world, "arena ref copy");
            None
        }
    }
}

/// Clone a value for COW/copy, serving Record/Sum/List/Set payloads from the
/// buffer pool so the copy allocates nothing on the heap (default mode). Map
/// payloads and Strings fall back to `Value::clone` (v1 limitation). Falls back
/// to `clone` on pool exhaustion (a build-time budget bug — the `debug_assert!`
/// in `alloc_owned`/`alloc_copy` flags it).
fn pool_clone_value<T: Transcendental, const BUF: usize>(
    _ir: &Ir,
    world: &mut WorldState<T, BUF>,
    v: &Value,
) -> Value {
    match v {
        Value::Record(fields) | Value::Sum(_, fields) => {
            let mut out = match world.arena.take_buf(fields.len()) {
                Ok(b) => b,
                Err(_) => return v.clone(),
            };
            out.extend_from_slice(fields);
            match v {
                Value::Record(_) => Value::Record(out),
                Value::Sum(c, _) => Value::Sum(*c, out),
                _ => unreachable!(),
            }
        }
        Value::List { elems } => {
            let mut out = match world.arena.take_buf(elems.len()) {
                Ok(b) => b,
                Err(_) => return v.clone(),
            };
            out.extend_from_slice(elems);
            Value::List { elems: out }
        }
        Value::Set { elems } => {
            let mut out = match world.arena.take_buf(elems.len()) {
                Ok(b) => b,
                Err(_) => return v.clone(),
            };
            out.extend_from_slice(elems);
            Value::Set { elems: out }
        }
        Value::Array { bounds, elems } => {
            let mut out = match world.arena.take_buf(elems.len()) {
                Ok(b) => b,
                Err(_) => return v.clone(),
            };
            out.extend_from_slice(elems);
            Value::Array {
                bounds: *bounds,
                elems: out,
            }
        }
        _ => v.clone(),
    }
}

/// Collect a copied ref for every register in `regs`.
///
/// Each source register keeps its own ownership, so every collected ref is a
/// fresh `copy` (RC++) — the constructed record/sum owns its fields
/// independently of the source registers (aliasing). Returns `None`, dropping
/// anything already copied, when any source is unbound or the arena cannot
/// count another ref. The result buffer is pooled (no per-tick heap
/// allocation).
fn read_field_refs<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    regs: &[usize],
    base: usize,
) -> Option<Vec<ArenaRef>> {
    let mut refs: Vec<ArenaRef> = match world.arena.take_buf(regs.len()) {
        Ok(b) => b,
        Err(_) => {
            latch_exhaustion(ir, world, "value buffer pool");
            return None;
        }
    };
    for r in regs {
        match world.value_regs[*r + base] {
            Some(src) => match world.arena.copy(src) {
                Ok(c) => refs.push(c),
                Err(_) => {
                    for x in &refs {
                        world.arena.drop_ref(*x);
                    }
                    return None;
                }
            },
            None => {
                for x in &refs {
                    world.arena.drop_ref(*x);
                }
                return None;
            }
        }
    }
    Some(refs)
}

/// Release the child refs of a raw value: the children of a slot that is about
/// to be overwritten or was never created. Mirrors `Arena::drop_ref` recursion.
fn drop_value_children<T: Transcendental, const BUF: usize>(
    _ir: &Ir,
    world: &mut WorldState<T, BUF>,
    v: &Value,
) {
    match v {
        Value::Record(fields) | Value::Sum(_, fields) => {
            for f in fields {
                world.arena.drop_ref(*f);
            }
        }
        Value::Newtype(inner) => world.arena.drop_ref(*inner),
        Value::Callable { env, cell, .. } => {
            world.arena.drop_ref(*env);
            if let Some(c) = cell {
                world.arena.drop_ref(*c);
            }
        }
        Value::BuiltinCallable { env, .. } => world.arena.drop_ref(*env),
        Value::List { elems, .. } => {
            for e in elems {
                world.arena.drop_ref(*e);
            }
        }
        Value::Map { pairs, .. } => {
            for (k, v) in pairs {
                world.arena.drop_ref(*k);
                world.arena.drop_ref(*v);
            }
        }
        Value::Set { elems, .. } => {
            for e in elems {
                world.arena.drop_ref(*e);
            }
        }
        Value::Array { bounds, elems } => {
            world.arena.drop_ref(*bounds);
            for e in elems {
                world.arena.drop_ref(*e);
            }
        }
        // A lazy callable releases its memo cell; a forced cell releases its
        // result. Mirroring `visit_refs` keeps the raw-value drop (an alloc
        // failure) balanced with the arena's own recursive drop.
        Value::ThunkCell(ThunkState::Forced(r)) => world.arena.drop_ref(*r),
        // A cons cell releases its head and (lazy) tail.
        Value::Cons(h, t) => {
            world.arena.drop_ref(*h);
            world.arena.drop_ref(*t);
        }
        // An unforced/forcing/bottom cell and bottom itself own nothing.
        Value::ThunkCell(_) | Value::Bottom | Value::Nil => {}
        // Leaf values own no children.
        _ => {}
    }
}

/// Structural total order over acyclic arena values (the derived `Eq`/`Ord`).
/// Returns `< 0`, `== 0`, or `> 0`.
///
/// Leaves compare by value; a `Sum` by constructor index then payload; a
/// `Record` by field order; a `Newtype` by its inner value; `List`/`Set`
/// lexicographically over elements; `Array` by bounds then elements
/// lexicographically; `Map` lexicographically over the sorted (key, value)
/// pairs. Mixed kinds fall back to [`kind_rank`], so any two
/// acyclic values are comparable — this total cross-kind order is what makes
/// arbitrary-typed `Map` keys work.
///
/// `Closure` values are unordered — reaching one is a lowering bug (the type
/// checker rejects `Ord` over functions).
fn value_cmp(arena: &Arena<Value>, a: ArenaRef, b: ArenaRef) -> i8 {
    match (arena.get(a), arena.get(b)) {
        (Some(va), Some(vb)) => value_cmp_ref(arena, va, vb),
        _ => 0,
    }
}

fn value_cmp_ref(arena: &Arena<Value>, a: &Value, b: &Value) -> i8 {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y)) => x.cmp(y) as i8,
        // `total_cmp` is a genuine total order: NaN sorts greatest and NaN ≡
        // NaN, so NaN keys in maps and sets stay well-defined and the ordering
        // is transitive. A `partial_cmp`-with-`Equal`-fallback would be an
        // intransitive equivalence (NaN ≈ 3.0 but 3.0 < 5.0), breaking the
        // sorted Map/Set binary search.
        (Float(x), Float(y)) => x.total_cmp(y) as i8,
        (Bool(x), Bool(y)) => x.cmp(y) as i8,
        (String(x), String(y)) => x.cmp(y) as i8,
        (Newtype(x), Newtype(y)) => value_cmp(arena, *x, *y),
        (Sum(i, px), Sum(j, py)) => {
            let c = i.cmp(j) as i8;
            if c != 0 {
                return c;
            }
            cmp_ref_slices(arena, px, py)
        }
        (Record(fx), Record(fy)) => cmp_ref_slices(arena, fx, fy),
        (List { elems: ex, .. }, List { elems: ey, .. }) => cmp_ref_slices(arena, ex, ey),
        (Set { elems: ex, .. }, Set { elems: ey, .. }) => cmp_ref_slices(arena, ex, ey),
        (
            Array {
                bounds: bx,
                elems: ex,
            },
            Array {
                bounds: by,
                elems: ey,
            },
        ) => {
            let c = value_cmp(arena, *bx, *by);
            if c != 0 {
                return c;
            }
            cmp_ref_slices(arena, ex, ey)
        }
        (Map { pairs: px, .. }, Map { pairs: py, .. }) => {
            for (i, (kx, vx)) in px.iter().enumerate() {
                let Some((ky, vy)) = py.get(i) else {
                    return 1;
                };
                let c = value_cmp(arena, *kx, *ky);
                if c != 0 {
                    return c;
                }
                let c = value_cmp(arena, *vx, *vy);
                if c != 0 {
                    return c;
                }
            }
            px.len().cmp(&py.len()) as i8
        }
        (Callable { .. }, Callable { .. }) => 0, // unreachable for Ord-typed keys
        _ => kind_rank(a).cmp(&kind_rank(b)) as i8,
    }
}

fn cmp_ref_slices(arena: &Arena<Value>, xs: &[ArenaRef], ys: &[ArenaRef]) -> i8 {
    for (i, x) in xs.iter().enumerate() {
        let Some(y) = ys.get(i) else {
            return 1;
        };
        let c = value_cmp(arena, *x, *y);
        if c != 0 {
            return c;
        }
    }
    xs.len().cmp(&ys.len()) as i8
}

fn kind_rank(v: &Value) -> i8 {
    use Value::*;
    match v {
        Bool(_) => 0,
        Int(_) => 1,
        Float(_) => 2,
        String(_) => 3,
        Record(_) => 4,
        Sum(..) => 5,
        Newtype(_) => 6,
        List { .. } => 7,
        Map { .. } => 8,
        Set { .. } => 9,
        Callable { .. } => 10,
        Void => 11,
        ThunkCell(_) => 12,
        Bottom => 13,
        Cons(..) => 14,
        Nil => 15,
        Buffer(_) => 16,
        Opaque(_) => 17,
        // Array sits at the end rather than in the collection cluster: a fresh
        // variant, so appending preserves every existing rank and keeps all
        // previously-built values' cross-kind order stable.
        Array { .. } => 18,
        BuiltinCallable { .. } => 10,
    }
}

/// The arithmetic operator of a [`ValueInstr::ValueAdd`]-family instruction.
#[derive(Debug, Clone, Copy)]
enum ValueArith {
    Add,
    Sub,
    Mul,
    Div,
}

/// A forced value-arithmetic operand, keeping the Int/Float KIND so integer
/// arithmetic can stay on the Int track.
///
/// `Bottom` propagates through strict arithmetic; an unbound or non-numeric
/// slot reads as the float `0.0` (the pre-existing "unbound reads as zero"
/// rule).
enum ArithmeticOperand {
    Int(i64),
    Float(f64),
    Bottom,
}

/// Force a slot's value and classify it as an arithmetic operand. The operand
/// is read exactly once (forcing may replace the slot's ref), so callers must
/// not re-read the same register.
fn arithmetic_operand<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    reg: usize,
    drops: &mut Vec<ArenaRef>,
) -> ArithmeticOperand {
    let Some(r) = world.value_regs.get(reg).copied().flatten() else {
        return ArithmeticOperand::Float(0.0);
    };
    let head = force(ir, world, r, drops);
    match world.arena.get(head) {
        Some(Value::Bottom) => ArithmeticOperand::Bottom,
        Some(Value::Int(i)) => ArithmeticOperand::Int(*i),
        Some(Value::Float(f)) => ArithmeticOperand::Float(*f),
        _ => ArithmeticOperand::Float(0.0),
    }
}

/// Read a scalar value as `f64`: a Float reads directly, an Int widens.
/// Non-numeric values (records, sums, closures, `Void`) read as `None`.
fn value_to_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// Execute one value-track arithmetic instruction and store the result in
/// `dst`.
///
/// The result keeps the Int track: when BOTH operands are `Value::Int` and the
/// operation is `+`, `-`, or `*`, the result is a `Value::Int` (wrapping, so a
/// pathological overflow cannot panic a debug build). `/` always yields a
/// `Float` — it is float division in this language, and truncating it would
/// silently change `3 / 2`. Everything else (a `Float` operand, an unbound or
/// non-numeric slot) computes in `f64` and yields a `Float`, exactly as before.
/// The static type tracks this rule in `infer_arith` / `lower`'s `Expr::Arith`
/// arm — a typed `Int` slot therefore holds a real `Value::Int`, which the flat
/// array primitives require of an offset.
///
/// The operands are read BEFORE any `dst` occupant is released, so `dst` may
/// alias an operand (`dst == a` or `dst == b`) without losing its value.
/// Lowering emits SSA registers (dst is always fresh), but the aliasing-safe
/// order keeps hand-written IR correct. The old occupant, when present, is
/// taken out of the register and queued, so the per-tick register clear cannot
/// drop it a second time.
fn exec_value_arith<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    op: ValueArith,
    dst: usize,
    a: usize,
    b: usize,
    drops: &mut Vec<ArenaRef>,
) {
    let av = arithmetic_operand(ir, world, a, drops);
    let bv = arithmetic_operand(ir, world, b, drops);
    // Release the previous `dst` occupant AFTER the operands are read/forced,
    // so `dst` may alias an operand without losing its value.
    if let Some(r) = world.value_regs.get_mut(dst).and_then(|r| r.take()) {
        drops.push(r);
    }
    // Bottom propagates through strict arithmetic.
    if matches!(av, ArithmeticOperand::Bottom) || matches!(bv, ArithmeticOperand::Bottom) {
        world.value_regs[dst] = alloc_owned(ir, world, Value::Bottom);
        return;
    }
    // Int-preserving `+`/`-`/`*` (see the doc comment above).
    if let (ArithmeticOperand::Int(x), ArithmeticOperand::Int(y)) = (&av, &bv) {
        let int_result = match op {
            ValueArith::Add => Some(x.wrapping_add(*y)),
            ValueArith::Sub => Some(x.wrapping_sub(*y)),
            ValueArith::Mul => Some(x.wrapping_mul(*y)),
            ValueArith::Div => None,
        };
        if let Some(v) = int_result {
            world.value_regs[dst] = alloc_owned(ir, world, Value::Int(v));
            return;
        }
    }
    let av = match av {
        ArithmeticOperand::Int(i) => i as f64,
        ArithmeticOperand::Float(f) => f,
        ArithmeticOperand::Bottom => unreachable!("bottom handled above"),
    };
    let bv = match bv {
        ArithmeticOperand::Int(i) => i as f64,
        ArithmeticOperand::Float(f) => f,
        ArithmeticOperand::Bottom => unreachable!("bottom handled above"),
    };
    let result = match op {
        ValueArith::Add => av + bv,
        ValueArith::Sub => av - bv,
        ValueArith::Mul => av * bv,
        ValueArith::Div => av / bv,
    };
    world.value_regs[dst] = alloc_owned(ir, world, Value::Float(result));
}

/// Allocate a fresh `Value::Bottom` slot. `None` on arena exhaustion (a
/// build-time budget bug — `debug_assert!` flags it; the caller falls back to
/// a value it already holds).
fn try_alloc_bottom<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
) -> Option<ArenaRef> {
    match world.arena.alloc(Value::Bottom) {
        Ok(b) => Some(b),
        Err(_) => {
            latch_exhaustion(ir, world, "value arena capacity");
            None
        }
    }
}

/// Write `Value::Bottom` into `dst`, releasing the previous occupant.
fn write_bottom<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    dst: usize,
    drops: &mut Vec<ArenaRef>,
) {
    if let Some(old) = world.value_regs.get_mut(dst).and_then(|r| r.take()) {
        drops.push(old);
    }
    world.value_regs[dst] = try_alloc_bottom(ir, world);
}

/// Force a value register's operand to WHNF and return the head ref.
///
/// The returned ref is a BORROWED view — the memo cell (or the caller's own
/// operand) owns it. A caller that stores the ref must `copy` it first; a
/// caller that only reads it must not drop it. `None` when the register is
/// unbound.
fn forced_operand<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    reg: usize,
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    let r = world.value_regs.get(reg).copied().flatten()?;
    Some(force(ir, world, r, drops))
}

fn exec_value_instr<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    instr: &ValueInstr,
    base: usize,
    drops: &mut Vec<ArenaRef>,
) {
    match instr {
        ValueInstr::ValuePushScope => {
            world.cell_stack.push(Vec::new());
        }
        ValueInstr::ValuePopScope => {
            // Frames are empty in v1 (cells are owned by their register); the
            // drop collection below is the future-proof hook for scoped cells.
            if let Some(frame) = world.cell_stack.pop() {
                for (_, cell) in frame {
                    drops.push(cell);
                }
            }
        }
        ValueInstr::ValueBindCell { dst } => {
            // A cell is a fresh slot holding the cell's value directly; the
            // register owns the cell ref.
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Void);
        }
        ValueInstr::ValueReadCell { dst, cell } => {
            // Inside a fragment (`frag_cells_base < frag_cells.len()`), `cell`
            // is a CAPTURE index into the current call's env frame — the
            // lambda's free variables, resolved at `frag_cells_base + cell`.
            // Otherwise `cell` is a value register holding a cell ref
            // (main-track reads). `cell` is never a fragment-local register, so
            // it is NOT offset by `base`.
            let capture = if world.frag_cells_base < world.frag_cells.len() {
                Some(world.frag_cells[world.frag_cells_base + *cell])
            } else {
                None
            };
            match capture.or_else(|| world.value_regs.get(*cell).copied().flatten()) {
                Some(cr) => {
                    // Copy the value OUT of the cell into a fresh slot: a
                    // read result is a new owner, it does not share. An
                    // uninitialised (Void) cell reads as 0.0. Payloads are
                    // pooled (no per-tick heap allocation).
                    world.value_regs[*dst + base] =
                        if matches!(world.arena.get(cr), Some(&Value::Void)) {
                            alloc_owned(ir, world, Value::Float(0.0))
                        } else {
                            alloc_copy_slot(ir, world, cr)
                        };
                }
                None => world.value_regs[*dst + base] = None,
            }
        }
        ValueInstr::ValueWriteCell { cell, src } => {
            // Write src's value INTO the existing cell slot, PRESERVING the
            // cell's identity: any other reference to the cell (an env-record
            // field capturing a self-cell for a recursive closure) must see
            // the new value. The old implementation allocated a fresh slot and
            // updated only the register — captured references went stale and
            // read the freed slot as Void. Both `cell` and `src` are
            // fragment-local registers — offset both.
            if let (Some(cr), Some(sr)) = (
                world.value_regs[*cell + base],
                world.value_regs[*src + base],
            ) {
                // A safe copy: recount (RC++) the children so the cell owns
                // them independently of src.
                let new_value: Option<Value> = match world.arena.get(sr).cloned() {
                    Some(v) => {
                        recount_children(ir, world, &v);
                        Some(v.clone())
                    }
                    None => None,
                };
                if let Some(nv) = new_value {
                    // Release the old cell value's children, then write the
                    // new value into the same slot.
                    if let Some(old) = world.arena.get(cr).cloned() {
                        drop_value_children(ir, world, &old);
                    }
                    let _ = world.arena.mutate_slot(cr, |slot| {
                        *slot = nv;
                    });
                }
            }
        }
        ValueInstr::ValueConstInt { dst, value } => {
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Int(*value));
        }
        ValueInstr::ValueConstFloat { dst, value } => {
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Float(*value));
        }
        ValueInstr::ValueAdd { dst, a, b } => exec_value_arith(
            ir,
            world,
            ValueArith::Add,
            *dst + base,
            *a + base,
            *b + base,
            drops,
        ),
        ValueInstr::ValueSub { dst, a, b } => exec_value_arith(
            ir,
            world,
            ValueArith::Sub,
            *dst + base,
            *a + base,
            *b + base,
            drops,
        ),
        ValueInstr::ValueMul { dst, a, b } => exec_value_arith(
            ir,
            world,
            ValueArith::Mul,
            *dst + base,
            *a + base,
            *b + base,
            drops,
        ),
        ValueInstr::ValueDiv { dst, a, b } => exec_value_arith(
            ir,
            world,
            ValueArith::Div,
            *dst + base,
            *a + base,
            *b + base,
            drops,
        ),
        ValueInstr::ValueConstructRecord { dst, fields } => {
            world.value_regs[*dst + base] =
                if let Some(refs) = read_field_refs(ir, world, fields, base) {
                    alloc_owned(ir, world, Value::Record(refs))
                } else {
                    None
                };
        }
        ValueInstr::ValueConstructSum { dst, ctor, payload } => {
            world.value_regs[*dst + base] =
                if let Some(refs) = read_field_refs(ir, world, payload, base) {
                    alloc_owned(ir, world, Value::Sum(*ctor, refs))
                } else {
                    None
                };
        }
        ValueInstr::ValueProject { dst, slot, field } => {
            match forced_operand(ir, world, *slot + base, drops) {
                Some(rec) => {
                    let fref: Option<ArenaRef> = match world.arena.get(rec) {
                        Some(Value::Bottom) => {
                            // Projecting bottom is bottom.
                            write_bottom(ir, world, *dst + base, drops);
                            return;
                        }
                        Some(Value::Record(fields)) => fields.get(*field).cloned(),
                        _ => None,
                    };
                    // A project result is a new owner: share the field ref.
                    match fref {
                        Some(fr) => world.value_regs[*dst + base] = copy_owned(ir, world, fr),
                        None => world.value_regs[*dst + base] = None,
                    }
                }
                None => world.value_regs[*dst + base] = None,
            }
        }
        ValueInstr::ValueUpdateField { slot, field, src } => {
            match (
                world.value_regs[*slot + base],
                world.value_regs[*src + base],
            ) {
                (Some(rec), Some(sr)) => {
                    // COW: if the record is shared, mutate gives us a private
                    // copy; the slot register takes the (possibly new) ref.
                    let new_rec = match world.arena.mutate(rec) {
                        Ok(r) => r,
                        Err(_) => {
                            world.value_regs[*slot + base] = None;
                            return;
                        }
                    };
                    // Snapshot the old field ref and count the source ref
                    // before touching the record, so the field write below is
                    // a clean replacement.
                    let old_field: Option<ArenaRef> = match world.arena.get(new_rec) {
                        Some(Value::Record(fields)) => fields.get(*field).cloned(),
                        _ => None,
                    };
                    let new_field = copy_owned(ir, world, sr);
                    let wrote = match new_field {
                        Some(nf) => match world.arena.get_mut(new_rec) {
                            Some(Value::Record(fields)) => {
                                fields[*field] = nf;
                                true
                            }
                            _ => {
                                // The slot held no record after all: release
                                // the counted ref and fail the update.
                                world.arena.drop_ref(nf);
                                false
                            }
                        },
                        None => false,
                    };
                    // Release the previous field ref now that the record no
                    // longer points at it, and surface the (possibly
                    // COW-copied) record ref to the slot register.
                    if wrote {
                        if let Some(of) = old_field {
                            world.arena.drop_ref(of);
                        }
                        world.value_regs[*slot + base] = Some(new_rec);
                    } else {
                        world.value_regs[*slot + base] = None;
                    }
                }
                _ => world.value_regs[*slot + base] = None,
            }
        }
        ValueInstr::ValueNewtype { dst, src } => {
            match world.value_regs[*src + base] {
                // The newtype owns its inner value; the src register keeps its
                // own ref — copy first so both hold a counted ref.
                Some(sr) => {
                    let copied = copy_owned(ir, world, sr);
                    match copied {
                        Some(c) => {
                            world.value_regs[*dst + base] =
                                alloc_owned(ir, world, Value::Newtype(c))
                        }
                        None => world.value_regs[*dst + base] = None,
                    }
                }
                None => world.value_regs[*dst + base] = None,
            }
        }
        ValueInstr::ValueUnwrap { dst, src } => {
            match forced_operand(ir, world, *src + base, drops) {
                Some(sr) => {
                    let inner: Option<ArenaRef> = match world.arena.get(sr) {
                        Some(Value::Bottom) => {
                            // Unwrapping bottom is bottom.
                            write_bottom(ir, world, *dst + base, drops);
                            return;
                        }
                        Some(Value::Newtype(inner)) => Some(*inner),
                        _ => None,
                    };
                    // The unwrapped result is a new owner: share the inner ref.
                    match inner {
                        Some(i) => world.value_regs[*dst + base] = copy_owned(ir, world, i),
                        None => world.value_regs[*dst + base] = None,
                    }
                }
                None => world.value_regs[*dst + base] = None,
            }
        }
        ValueInstr::ValueCallFunc {
            dst,
            closure_slot,
            args,
        } => {
            // Runtime dispatch: read the callable from `closure_slot`, run the
            // referenced fragment with a temporary register frame, and copy the
            // result into `dst`. An unbound slot or a non-eager callable value
            // (a lazy thunk) yields a detectable `None`.
            let slot = world
                .value_regs
                .get(*closure_slot + base)
                .copied()
                .flatten();
            // A primitive effect closure (`Value::BuiltinCallable`) is applied
            // to its STState token and run eagerly — effects never suspend.
            if let Some(callee) = slot {
                if matches!(world.arena.get(callee), Some(Value::BuiltinCallable { .. })) {
                    let arg = args
                        .first()
                        .and_then(|a| world.value_regs.get(*a + base).copied().flatten());
                    world.value_regs[*dst + base] = match arg {
                        Some(a) => apply_callable(ir, world, callee, a, drops),
                        None => None,
                    };
                    return;
                }
            }
            match slot.and_then(|s| world.arena.get(s)) {
                Some(Value::Callable {
                    fragment: fragment_id,
                    env: env_ref,
                    cell: None,
                }) => {
                    // Share the fragment via `Arc` so the world can be borrowed
                    // mutably inside `run_call` without cloning the body
                    // (an atomic refcount bump, no heap allocation on the RT
                    // path). The `Arc::clone` ends the immutable IR borrow.
                    match ir.fragments.get(*fragment_id as usize).cloned() {
                        Some(frag) => {
                            run_call(
                                ir,
                                world,
                                &frag,
                                *env_ref,
                                ArgsSource::Registers { regs: args, base },
                                Some(*dst + base),
                                drops,
                            );
                        }
                        None => world.value_regs[*dst + base] = None,
                    }
                }
                _ => world.value_regs[*dst + base] = None,
            }
        }
        ValueInstr::ValueCallLazy {
            dst,
            closure_slot,
            args,
        } => {
            // Lazily apply a callable: build a thunk over its fragment and an
            // env record that concatenates the callable's captured env fields
            // with the (unforced) argument refs. The application does not run —
            // `run_call` splits the env into captures + args by the fragment's
            // `value_ins` when the thunk is forced.
            let Some(cr) = world
                .value_regs
                .get(*closure_slot + base)
                .copied()
                .flatten()
            else {
                world.value_regs[*dst + base] = None;
                return;
            };
            // Only an EAGER function closure is suspended into a thunk. A
            // primitive effect closure must run (effects are strict), and a
            // thunk callee is forced before application — both go through
            // `apply_callable`.
            //
            // Trade-off: forcing a THUNK callee here applies it eagerly, i.e. it
            // removes call deferral for that one shape. That is what makes an
            // `ST` action reached through a `let` run at all (the `Monad (ST s)`
            // instance body is inlined at lowering, so its callee can be a
            // thunk), and it only ever upgrades a call that previously produced
            // nothing — an infinite structure cannot be applied as a function,
            // so no terminating program loses an answer that used to be
            // returned.
            if !matches!(
                world.arena.get(cr),
                Some(Value::Callable { cell: None, .. })
            ) {
                let arg = args
                    .first()
                    .and_then(|a| world.value_regs.get(*a + base).copied().flatten());
                world.value_regs[*dst + base] = match arg {
                    Some(a) => apply_callable(ir, world, cr, a, drops),
                    None => None,
                };
                return;
            }
            let (env_ref, fid) = match world.arena.get(cr) {
                Some(Value::Callable {
                    fragment,
                    env,
                    cell: None,
                }) => (*env, *fragment),
                _ => {
                    world.value_regs[*dst + base] = None;
                    return;
                }
            };
            let cell = match world.arena.alloc(Value::ThunkCell(ThunkState::Unforced)) {
                Ok(c) => c,
                Err(_) => {
                    world.value_regs[*dst + base] = None;
                    return;
                }
            };
            // Combined env: the closure's captured fields (recounted so both
            // the closure's env and this record own them) followed by the arg
            // refs (copied from the caller's registers).
            let mut fields: Vec<ArenaRef> = match world.arena.get(env_ref) {
                Some(Value::Record(fs)) => {
                    let fs = fs.clone();
                    for f in &fs {
                        let _ = world.arena.copy(*f);
                    }
                    fs
                }
                _ => Vec::new(),
            };
            for &a in args {
                if let Some(r) = world.value_regs.get(a + base).copied().flatten() {
                    match world.arena.copy(r) {
                        Ok(c) => fields.push(c),
                        Err(_) => {
                            for f in &fields {
                                world.arena.drop_ref(*f);
                            }
                            world.arena.drop_ref(cell);
                            world.value_regs[*dst + base] = None;
                            return;
                        }
                    }
                }
            }
            let env_owned = match alloc_owned(ir, world, Value::Record(fields)) {
                Some(e) => e,
                None => {
                    world.arena.drop_ref(cell);
                    world.value_regs[*dst + base] = None;
                    return;
                }
            };
            world.value_regs[*dst + base] = alloc_owned(
                ir,
                world,
                Value::Callable {
                    fragment: fid,
                    env: env_owned,
                    cell: Some(cell),
                },
            );
        }
        ValueInstr::ValueMakeClosure { dst, env, fragment } => {
            // A callable is a leaf holding the env record ref, the fragment id,
            // and no memo cell (eager). The env register is read by value; the
            // callable COUNTS the env as a child (like a Record counts its
            // fields), so the creating register keeps its own ownership and
            // `drop_ref` releases the env when the callable is freed. An
            // unbound env (v1 named references) falls back to a fresh Void slot
            // the callable owns outright.
            let env_ref = match world.value_regs.get(*env + base).copied().flatten() {
                Some(r) => match world.arena.copy(r) {
                    Ok(c) => c,
                    Err(_) => {
                        world.value_regs[*dst + base] = None;
                        return;
                    }
                },
                None => match world.arena.alloc(Value::Void) {
                    Ok(v) => v,
                    Err((err, _)) => {
                        world.value_regs[*dst + base] = None;
                        let _ = err;
                        return;
                    }
                },
            };
            world.value_regs[*dst + base] = alloc_owned(
                ir,
                world,
                Value::Callable {
                    fragment: *fragment as u32,
                    env: env_ref,
                    cell: None,
                },
            );
        }
        ValueInstr::ValueCopy { dst, src } => match world.value_regs[*src + base] {
            Some(sr) => {
                let copied = copy_owned(ir, world, sr);
                world.value_regs[*dst + base] = copied;
            }
            None => world.value_regs[*dst + base] = None,
        },
        ValueInstr::ValueMove { dst, src } => {
            // A move overwrites `dst`, so release any previous occupant (dst
            // regs are SSA in lowering, but re-assignment must not leak) — and
            // a self-move (`dst == src`) is a no-op, never a clear.
            if *dst != *src {
                if let Some(old) = world.value_regs[*dst + base].take() {
                    drops.push(old);
                }
                world.value_regs[*dst + base] = world.value_regs[*src + base].take();
            }
        }
        ValueInstr::ValueDrop { src } => {
            if let Some(r) = world.value_regs[*src + base].take() {
                drops.push(r);
            }
        }
        ValueInstr::ValueStateRead { dst, slot } => {
            match world.value_state.get(*slot) {
                // The read result is a new owner of the stored value.
                Some(Some(stored)) => {
                    let sr = *stored;
                    world.value_regs[*dst + base] = copy_owned(ir, world, sr);
                }
                // An empty state slot reads as 0.0.
                _ => world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Float(0.0)),
            }
        }
        ValueInstr::ValueStateWrite { slot, src } => {
            if let Some(sr) = world.value_regs[*src + base] {
                // The state slot owns its value: copy the src ref, then
                // release the previous tick's value and store the fresh one.
                let stored = copy_owned(ir, world, sr);
                if let Some(old) = world.value_state[*slot].take() {
                    world.arena.drop_ref(old);
                }
                world.value_state[*slot] = stored;
            }
        }
        ValueInstr::ValueMatch { dst, slot, ctor } => {
            // v1 static dispatch: the scrutinee's constructor is statically
            // known (a literal sum), so exactly one arm matches. The scrutinee
            // is forced to WHNF first — a bottom scrutinee writes bottom into
            // every dst reg (no arm is selected, control does not advance on
            // its payload); a non-matching arm (or an unbound scrutinee)
            // writes None into every dst reg — a detectable no-op. Each dst
            // reg owns its payload ref via a copy (rc++); the scrutinee keeps
            // its own ownership of the sum.
            let scrutinee = forced_operand(ir, world, *slot + base, drops);
            if let Some(sr) = scrutinee {
                if matches!(world.arena.get(sr), Some(Value::Bottom)) {
                    for d in dst {
                        write_bottom(ir, world, *d + base, drops);
                    }
                    return;
                }
            }
            let payload: Option<Vec<ArenaRef>> = match scrutinee {
                Some(sr) => match world.arena.get(sr) {
                    Some(Value::Sum(c, fields)) if *c == *ctor => Some(fields.clone()),
                    _ => None,
                },
                None => None,
            };
            match payload {
                Some(fields) => {
                    for (i, d) in dst.iter().enumerate() {
                        let r = fields
                            .get(i)
                            .copied()
                            .and_then(|fr| world.arena.copy(fr).ok());
                        if let Some(reg) = world.value_regs.get_mut(*d + base) {
                            // Release any previous occupant (dst regs are SSA in
                            // lowering, but re-assignment must not leak).
                            if let Some(old) = reg.take() {
                                drops.push(old);
                            }
                            *reg = r;
                        }
                    }
                }
                None => {
                    for d in dst {
                        if let Some(reg) = world.value_regs.get_mut(*d + base) {
                            if let Some(old) = reg.take() {
                                drops.push(old);
                            }
                            *reg = None;
                        }
                    }
                }
            }
        }
        ValueInstr::ValueBool { dst, value } => {
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Bool(*value));
        }
        ValueInstr::ValueConstVoid { dst } => {
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Void);
        }
        ValueInstr::ValueConstOpaque { dst, handle } => {
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Opaque(*handle));
        }
        ValueInstr::ValueReadParam { dst, idx } => {
            let v = param_at(ir, world, *idx);
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Float(v));
        }
        ValueInstr::ValueReadSampleRate { dst } => {
            let v = world.params.sample_rate as f64;
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Float(v));
        }
        ValueInstr::ValueConstString { dst, value } => {
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::String(value.clone()));
        }
        ValueInstr::ValueListLit { dst, elems } => {
            world.value_regs[*dst + base] =
                if let Some(refs) = read_field_refs(ir, world, elems, base) {
                    alloc_owned(ir, world, Value::List { elems: refs })
                } else {
                    None
                };
        }
        ValueInstr::ValueMapLit { dst, keys, vals } => {
            world.value_regs[*dst + base] = if let (Some(ks), Some(vs)) = (
                read_field_refs(ir, world, keys, base),
                read_field_refs(ir, world, vals, base),
            ) {
                // Map entries are kept SORTED by the derived key order (the
                // insert/lookup/member binary search relies on it), so a map
                // literal written out of order is normalised here.
                let mut pairs: Vec<(ArenaRef, ArenaRef)> = ks.into_iter().zip(vs).collect();
                pairs.sort_by(|a, b| value_cmp(&world.arena, a.0, b.0).cmp(&0));
                alloc_owned(ir, world, Value::Map { pairs })
            } else {
                None
            };
        }
        ValueInstr::ValueCompare { dst, op, a, b } => {
            let (ar, br) = (
                forced_operand(ir, world, *a + base, drops),
                forced_operand(ir, world, *b + base, drops),
            );
            let res = match (ar, br) {
                (Some(x), Some(y)) => match (world.arena.get(x), world.arena.get(y)) {
                    (Some(Value::Bottom), _) | (_, Some(Value::Bottom)) => Some(Value::Bottom),
                    _ => {
                        let c = value_cmp(&world.arena, x, y);
                        let b = match op {
                            CmpOp::Eq => c == 0,
                            CmpOp::Ne => c != 0,
                            CmpOp::Lt => c < 0,
                            CmpOp::Gt => c > 0,
                            CmpOp::Le => c <= 0,
                            CmpOp::Ge => c >= 0,
                        };
                        Some(Value::Bool(b))
                    }
                },
                _ => None,
            };
            world.value_regs[*dst + base] = res.and_then(|v| alloc_owned(ir, world, v));
        }
        ValueInstr::ValueLogic { dst, op, a, b } => {
            let (ar, br) = (
                forced_operand(ir, world, *a + base, drops),
                forced_operand(ir, world, *b + base, drops),
            );
            let res = match (ar, br) {
                (Some(x), Some(y)) => match (world.arena.get(x), world.arena.get(y)) {
                    (Some(Value::Bottom), _) | (_, Some(Value::Bottom)) => Some(Value::Bottom),
                    (Some(Value::Bool(p)), Some(Value::Bool(q))) => Some(Value::Bool(match op {
                        LogicOp::And => *p && *q,
                        LogicOp::Or => *p || *q,
                    })),
                    _ => None,
                },
                _ => None,
            };
            world.value_regs[*dst + base] = res.and_then(|v| alloc_owned(ir, world, v));
        }
        ValueInstr::ValueNot { dst, src } => {
            let b = match forced_operand(ir, world, *src + base, drops) {
                Some(r) => match world.arena.get(r) {
                    Some(Value::Bottom) => Some(Value::Bottom),
                    Some(Value::Bool(p)) => Some(Value::Bool(!*p)),
                    _ => None,
                },
                _ => None,
            };
            world.value_regs[*dst + base] = b.and_then(|v| alloc_owned(ir, world, v));
        }
        ValueInstr::ValueSetError => {
            world.value_error = Some(ProcessError::processing(
                "match is non-exhaustive at runtime (no arm matched)",
            ));
        }
        ValueInstr::ValueArrayHoleFault => {
            // The synthetic hole fragment's sole instruction: forcing an
            // unlisted position of an M8 array latches the clean fault (the
            // message is the "undefined array element" contract, NOT the
            // match-specific `ValueSetError` text). The forced thunk gets no
            // register result — `run_call` returns None, `force()` memoizes
            // bottom into the shared cell, and the fault surfaces at tick end.
            latch_fault(ir, world, "array: undefined array element");
        }
        ValueInstr::ValueCallBuiltin { dst, op, args } => {
            exec_value_call_builtin(ir, world, *op, args, *dst + base, base, drops);
        }
        ValueInstr::ValueEffectOp { dst, op, args } => match op {
            crate::ir::ValueEffectOp::RunST | crate::ir::ValueEffectOp::RunSTArray => {
                // The argument register holds the `ST s a` value; run its region.
                // `runSTArray` additionally FREEZES the region's `STArray`
                // result into an immutable `Array` before the region unwinds.
                let freeze = matches!(op, crate::ir::ValueEffectOp::RunSTArray);
                let st_value = args
                    .first()
                    .and_then(|a| world.value_regs.get(*a + base).copied().flatten());
                world.value_regs[*dst + base] = match st_value {
                    Some(st) => exec_runst(ir, world, st, drops, freeze),
                    None => None,
                };
            }
            _ => {
                // Build the effect closure: an argument record owning copies of
                // the operand refs (the operand registers keep their own).
                let mut fields: Vec<crate::arena::ArenaRef> = Vec::with_capacity(args.len());
                for a in args {
                    let Some(r) = world.value_regs.get(*a + base).copied().flatten() else {
                        // `exec_effect` reads the captured operands
                        // POSITIONALLY, so skipping an unbound one would
                        // silently re-index the rest — a missing FIRST operand
                        // would turn the stored value into the cell handle.
                        // Typed code cannot reach this; fail loudly instead.
                        for f in &fields {
                            world.arena.drop_ref(*f);
                        }
                        world.value_regs[*dst + base] = None;
                        latch_fault(ir, world, "primitive effect with an unbound operand");
                        return;
                    };
                    match copy_owned(ir, world, r) {
                        Some(c) => fields.push(c),
                        None => {
                            for f in &fields {
                                world.arena.drop_ref(*f);
                            }
                            world.value_regs[*dst + base] = None;
                            return;
                        }
                    }
                }
                let env = alloc_owned(ir, world, Value::Record(fields));
                world.value_regs[*dst + base] = match env {
                    Some(env) => alloc_owned(ir, world, Value::BuiltinCallable { op: *op, env }),
                    None => None,
                };
            }
        },
        ValueInstr::ValueLazy { dst, env, fragment } => {
            // Create a lazy binding: a fresh memo cell plus a callable value
            // referencing it and the captured env (a thunk). The env register
            // is read by value; the callable COUNTS the env as a child (like a
            // Record), so the creating register keeps its own ownership. An
            // unbound env (v1 named references) falls back to a fresh Void slot
            // the callable owns outright. The cell is created with rc 1 and is
            // referenced only by this callable value — copies of it SHARE the
            // slot (`ValueCopy`/`copy_owned`), never duplicating the cell, so
            // forcing can mutate it in place.
            let cell = match world.arena.alloc(Value::ThunkCell(ThunkState::Unforced)) {
                Ok(c) => c,
                Err(_) => {
                    world.value_regs[*dst + base] = None;
                    return;
                }
            };
            let env_ref = match world.value_regs.get(*env + base).copied().flatten() {
                Some(r) => match world.arena.copy(r) {
                    Ok(c) => c,
                    Err(_) => {
                        world.arena.drop_ref(cell);
                        world.value_regs[*dst + base] = None;
                        return;
                    }
                },
                None => match world.arena.alloc(Value::Void) {
                    Ok(v) => v,
                    Err((err, _)) => {
                        world.arena.drop_ref(cell);
                        world.value_regs[*dst + base] = None;
                        let _ = err;
                        return;
                    }
                },
            };
            world.value_regs[*dst + base] = alloc_owned(
                ir,
                world,
                Value::Callable {
                    fragment: *fragment as u32,
                    env: env_ref,
                    cell: Some(cell),
                },
            );
        }
        ValueInstr::ValueForce { dst, src } => {
            // Force the source to WHNF and share the head ref with `dst`. The
            // head is borrowed from the source's memo cell (or is the source
            // itself when already WHNF), so `dst` claims its own copy.
            match world.value_regs.get(*src + base).copied().flatten() {
                Some(sr) => {
                    let head = force(ir, world, sr, drops);
                    world.value_regs[*dst + base] = copy_owned(ir, world, head);
                }
                None => world.value_regs[*dst + base] = None,
            }
        }
        // ---- Whole-buffer value ops (B3-2) ----
        // Each op reads its operand value registers' `Value::Buffer` indices,
        // operates on the program's `RegionHandle` buffer table IN-PLACE (via
        // `with_data`/`with_data_mut`), and stores a fresh `Value::Buffer(dst)`
        // marker into the destination value register. The buffer index equals
        // the value register index (1:1 signal lowering).
        ValueInstr::ValueBufferConst { dst, value } => {
            let n = world.block_len;
            world.buffers[*dst]
                .with_data_mut(|b| crate::buffer_math::fill_into(&mut b[..n], T::from_f64(*value)));
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Buffer(*dst));
        }
        ValueInstr::ValueBufferLoadInput { dst, index } => {
            // Allocate the `Value::Buffer` marker for the k-th program input
            // channel. Program inputs are not driven on the retained value
            // path (only the removed signal-transform surface materialized
            // them), so the marker is never read; the buffer index is the
            // destination register (the 1:1 signal lowering).
            let _ = index;
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Buffer(*dst));
        }
        ValueInstr::ValueBufferStateRead { dst, slot } => {
            let n = world.block_len;
            let di = buffer_index(ir, world, *dst, base);
            world.buffers[di]
                .with_data_mut(|b| b[..n].copy_from_slice(&world.block_state[*slot][..n]));
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Buffer(di));
        }
        ValueInstr::ValueBufferDelayRead { dst, line } => {
            let n = world.block_len;
            let di = buffer_index(ir, world, *dst, base);
            world.buffers[di].with_data_mut(|b| world.delays[*line].read_block(&mut b[..n]));
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Buffer(di));
        }
        ValueInstr::ValueBufferUn { dst, op, src } => {
            let n = world.block_len;
            let si = buffer_index(ir, world, *src, base);
            let di = buffer_index(ir, world, *dst, base);
            let mut scratch = [T::ZERO; BUF];
            world.buffers[si].with_data(|s| scratch[..n].copy_from_slice(&s[..n]));
            world.buffers[di].with_data_mut(|b| {
                apply_un_slice(*op, &scratch[..n], &mut b[..n]);
            });
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Buffer(di));
        }
        ValueInstr::ValueBufferBin { dst, op, a, b } => {
            let n = world.block_len;
            let ai = buffer_index(ir, world, *a, base);
            let bi = buffer_index(ir, world, *b, base);
            let di = buffer_index(ir, world, *dst, base);
            let mut sa = [T::ZERO; BUF];
            let mut sb = [T::ZERO; BUF];
            world.buffers[ai].with_data(|s| sa[..n].copy_from_slice(&s[..n]));
            world.buffers[bi].with_data(|s| sb[..n].copy_from_slice(&s[..n]));
            world.buffers[di].with_data_mut(|out| {
                apply_bin_slice(*op, &sa[..n], &sb[..n], &mut out[..n]);
            });
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Buffer(di));
        }
        ValueInstr::ValueBufferMove { dst, src } => {
            let n = world.block_len;
            let si = buffer_index(ir, world, *src, base);
            let di = buffer_index(ir, world, *dst, base);
            let mut scratch = [T::ZERO; BUF];
            world.buffers[si].with_data(|s| scratch[..n].copy_from_slice(&s[..n]));
            world.buffers[di].with_data_mut(|b| b[..n].copy_from_slice(&scratch[..n]));
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Buffer(di));
        }
        ValueInstr::ValueBufferFill { dst, src } => {
            // Broadcast a scalar Float value register into a buffer constant.
            let n = world.block_len;
            let v = match world.value_regs.get(*src + base).copied().flatten() {
                Some(r) => match world.arena.get(r) {
                    Some(Value::Float(f)) => *f,
                    Some(Value::Int(i)) => *i as f64,
                    _ => 0.0,
                },
                _ => 0.0,
            };
            let di = buffer_index(ir, world, *dst, base);
            world.buffers[di].with_data_mut(|b| b[..n].fill(T::from_f64(v)));
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Buffer(di));
        }
        ValueInstr::ValueBufferStateWrite { slot, src } => {
            let n = world.block_len;
            let si = buffer_index(ir, world, *src, base);
            world.buffers[si]
                .with_data(|s| world.block_state_next[*slot][..n].copy_from_slice(&s[..n]));
        }
        ValueInstr::ValueBufferDelayWrite { line, src } => {
            let n = world.block_len;
            let si = buffer_index(ir, world, *src, base);
            world.buffers[si].with_data(|s| world.delays[*line].write_block(&s[..n]));
        }
        ValueInstr::ValueCallForeign {
            dst,
            srcs,
            instance,
        } => {
            exec_value_foreign(ir, world, *dst, srcs, *instance, base);
        }
        ValueInstr::ValueBufferParamRead { dst, idx } => {
            let n = world.block_len;
            let v = T::from_f64(param_at(ir, world, *idx));
            let di = buffer_index(ir, world, *dst, base);
            world.buffers[di].with_data_mut(|b| b[..n].fill(v));
            world.value_regs[*dst + base] = alloc_owned(ir, world, Value::Buffer(di));
        }
    }
}

/// The sorted position at which `key` belongs among the sorted `keys`, via
/// binary search over the total `value_cmp` order — the first index whose key
/// is not less than `key` (a lower bound). Map/Set entries are kept sorted by
/// this derived order, so `insert` splices here, and `lookup`/`member` test
/// `value_cmp(key, keys[pos]) == 0` to distinguish hit from insert point.
fn sorted_insert_pos<T: Transcendental, const BUF: usize>(
    _ir: &Ir,
    world: &WorldState<T, BUF>,
    key: ArenaRef,
    keys: &[ArenaRef],
) -> usize {
    let mut lo = 0;
    let mut hi = keys.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if value_cmp(&world.arena, key, keys[mid]) > 0 {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Copy a slot's List/Set element refs into a freshly pooled buffer.
///
/// Returns an OWNED copy of the refs (no borrow of `world.arena` survives), so
/// the caller can iterate them while mutating the arena (calling closures,
/// recounting) without a borrow conflict and without a per-tick heap
/// allocation. `None` when the register/slot is not the right container kind.
fn pooled_elems_of<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    reg: usize,
) -> Option<Vec<ArenaRef>> {
    let r = world.value_regs[reg]?;
    let len = match world.arena.get(r) {
        Some(Value::List { elems }) | Some(Value::Set { elems }) => elems.len(),
        _ => return None,
    };
    let mut out = match world.arena.take_buf(len) {
        Ok(b) => b,
        Err(_) => {
            latch_exhaustion(ir, world, "value buffer pool");
            return None;
        }
    };
    if let Some(elems) = match world.arena.get(r) {
        Some(Value::List { elems }) | Some(Value::Set { elems }) => Some(elems),
        _ => None,
    } {
        out.extend_from_slice(elems);
    }
    Some(out)
}

/// Copy a SLOT's List element refs into a freshly pooled buffer.
///
/// Like [`pooled_elems_of`] but takes an arena ref (a closure call result)
/// instead of a value-register index.
fn pooled_elems_of_ref<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    r: ArenaRef,
) -> Option<Vec<ArenaRef>> {
    let len = match world.arena.get(r) {
        Some(Value::List { elems }) => elems.len(),
        _ => return None,
    };
    let mut out = match world.arena.take_buf(len) {
        Ok(b) => b,
        Err(_) => {
            latch_exhaustion(ir, world, "value buffer pool");
            return None;
        }
    };
    if let Some(elems) = match world.arena.get(r) {
        Some(Value::List { elems }) => Some(elems),
        _ => None,
    } {
        out.extend_from_slice(elems);
    }
    Some(out)
}

/// Copy an immutable ARRAY's element refs into a freshly pooled buffer.
///
/// The destination value register holds an `Array i e`; the refs are copied
/// WITHOUT recounting (the source array keeps its own claims) — the caller
/// `copy`s the ones its result claims, the same discipline as
/// [`pooled_elems_of`]. Returns `None` (latching exhaustion, or for a
/// non-array register) when there is nothing usable.
fn pooled_array_elems_of<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    reg: usize,
) -> Option<Vec<ArenaRef>> {
    let r = world.value_regs[reg]?;
    let len = match world.arena.get(r) {
        Some(Value::Array { elems, .. }) => elems.len(),
        _ => return None,
    };
    let mut out = match world.arena.take_buf(len) {
        Ok(b) => b,
        Err(_) => {
            latch_exhaustion(ir, world, "value buffer pool");
            return None;
        }
    };
    if let Some(elems) = match world.arena.get(r) {
        Some(Value::Array { elems, .. }) => Some(elems),
        _ => None,
    } {
        out.extend_from_slice(elems);
    }
    debug_assert_eq!(out.len(), len, "pooled array elems length mismatch");
    Some(out)
}

/// Copy an array's element refs into a fresh pooled buffer, RECOUNTED (RC++)
/// for a new array that owns them independently of the source.
///
/// Every ref in the returned buffer therefore carries exactly one claim owned
/// by the caller (released with `drop_ref` on unwind, or transferred into a new
/// `Value::Array`).
fn copy_array_elems<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    reg: usize,
) -> Option<Vec<ArenaRef>> {
    let out = pooled_array_elems_of(ir, world, reg)?;
    for e in &out {
        // `copy` shares the same ref with a fresh claim (RC++).
        _ = world.arena.copy(*e);
    }
    Some(out)
}

/// Read an index-typed `Pair i e` association: the pair's index ref and value
/// ref.
///
/// The pair is a `Pair` record (`first` = index, `second` = value) — the shape
/// the type checker enforces. A malformed pair is reported as a distinct fault
/// reason rather than a panic, so the caller can name the actual cause. The
/// index itself is resolved against the bounds shape by the caller.
fn indexed_assoc<T: Transcendental, const BUF: usize>(
    world: &WorldState<T, BUF>,
    pair: ArenaRef,
) -> Result<(ArenaRef, ArenaRef), &'static str> {
    let fields = match world.arena.get(pair) {
        Some(Value::Record(fields)) if fields.len() == 2 => fields,
        _ => return Err("association is not a Pair"),
    };
    Ok((fields[0], fields[1]))
}

/// The run-time shape of an array's bounds — the interpreter-side mirror of the
/// prelude `Ix` instances for the two supported index types (`Int`,
/// `Pair Int Int`). Everything else is a clean "unsupported index type" fault
/// (Q1): construction/update offsets are resolved HERE against the bounds
/// shape instead of being computed in the language, so the supported surface is
/// exactly the two prelude shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArrayShape {
    /// One-dimensional: `bounds = Pair lo hi` over `Int` indices.
    Dim1 { lo: i64, hi: i64 },
    /// Row-major 2-D: `bounds = Pair (Pair lo1 lo2) (Pair hi1 hi2)`.
    Dim2 {
        lo1: i64,
        lo2: i64,
        hi1: i64,
        hi2: i64,
    },
}

/// Discriminate the bounds VALUE of an array (or construction bounds pair)
/// into its shape. Each field is forced to WHNF before matching, so lazy
/// bounds values in Complete mode read correctly. Any shape other than the two
/// prelude index types is `Err` (a clean "unsupported index type" fault).
fn array_shape<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    bounds: ArenaRef,
    drops: &mut Vec<ArenaRef>,
) -> Result<ArrayShape, &'static str> {
    // Copy the two bound field refs out of the borrow before any `&mut world`
    // call below.
    let (b0, b1) = match world.arena.get(bounds) {
        Some(Value::Record(fields)) if fields.len() == 2 => (fields[0], fields[1]),
        _ => return Err("unsupported index type"),
    };
    let f0 = force(ir, world, b0, drops);
    let f1 = force(ir, world, b1, drops);
    // Dim1: both sides are plain `Int`s. Dim2: both sides are 2-field records
    // of `Int`s (nested order: `lo = (lo1, lo2)`, `hi = (hi1, hi2)`).
    match (world.arena.get(f0), world.arena.get(f1)) {
        (Some(Value::Int(_)), Some(Value::Int(_))) => {
            let (Some(lo), Some(hi)) = (
                forced_i64(ir, world, f0, drops),
                forced_i64(ir, world, f1, drops),
            ) else {
                return Err("unsupported index type");
            };
            Ok(ArrayShape::Dim1 { lo, hi })
        }
        (Some(Value::Record(lf)), Some(Value::Record(hf))) if lf.len() == 2 && hf.len() == 2 => {
            // Copy the four nested field refs out of the borrow, then resolve.
            let (lf0, lf1, hf0, hf1) = (lf[0], lf[1], hf[0], hf[1]);
            let l1 = forced_i64(ir, world, lf0, drops);
            let l2 = forced_i64(ir, world, lf1, drops);
            let h1 = forced_i64(ir, world, hf0, drops);
            let h2 = forced_i64(ir, world, hf1, drops);
            let (Some(lo1), Some(lo2), Some(hi1), Some(hi2)) = (l1, l2, h1, h2) else {
                return Err("unsupported index type");
            };
            Ok(ArrayShape::Dim2 { lo1, lo2, hi1, hi2 })
        }
        _ => Err("unsupported index type"),
    }
}

impl ArrayShape {
    /// The flat element count of the shape. The arithmetic wraps i64 exactly
    /// like the language's unchecked `Ix` methods: an INVERTED bounds pair
    /// yields a negative size, meaning a VALID EMPTY array (Q3); a huge
    /// wrapping size is bounded by the arena capacity at construction.
    fn size(&self) -> i64 {
        match self {
            ArrayShape::Dim1 { lo, hi } => hi.wrapping_sub(*lo).wrapping_add(1),
            ArrayShape::Dim2 { lo1, lo2, hi1, hi2 } => hi1
                .wrapping_sub(*lo1)
                .wrapping_add(1)
                .wrapping_mul(hi2.wrapping_sub(*lo2).wrapping_add(1)),
        }
    }

    /// The flat offset of `index` under this shape, or `None` when the index is
    /// OUT OF the bounded rectangle — checked BEFORE any arithmetic (Q3: an
    /// out-of-rectangle cell with an accidentally in-range flat offset must
    /// fault). Reproduces the prelude instances' `inRange`/`index` bodies
    /// exactly. Called by the index-typed construction/update arms
    /// (`arrayFromAssocs`, `arrayUpdate`, `arrayAccum`) for every association.
    fn offset<T: Transcendental, const BUF: usize>(
        &self,
        ir: &Ir,
        world: &mut WorldState<T, BUF>,
        index: ArenaRef,
        drops: &mut Vec<ArenaRef>,
    ) -> Option<i64> {
        let i = forced_i64(ir, world, index, drops);
        match (self, i) {
            (ArrayShape::Dim1 { .. }, None) => None,
            (ArrayShape::Dim1 { lo, hi }, Some(i)) => {
                if *lo <= i && i <= *hi {
                    Some(i.wrapping_sub(*lo))
                } else {
                    None
                }
            }
            // A 2-D index is a `Pair i1 i2` record. Non-record / non-Int
            // fields are simply out of the supported surface (=> `None`, so the
            // caller reports an out-of-range / unsupported fault).
            (ArrayShape::Dim2 { lo1, lo2, hi1, hi2 }, _) => {
                let (x0, x1) = match world.arena.get(index) {
                    Some(Value::Record(f)) if f.len() == 2 => (f[0], f[1]),
                    _ => return None,
                };
                let i1 = forced_i64(ir, world, x0, drops);
                let i2 = forced_i64(ir, world, x1, drops);
                let (Some(i1), Some(i2)) = (i1, i2) else {
                    return None;
                };
                if *lo1 <= i1 && i1 <= *hi1 && *lo2 <= i2 && i2 <= *hi2 {
                    Some(
                        i1.wrapping_sub(*lo1)
                            .wrapping_mul(hi2.wrapping_sub(*lo2).wrapping_add(1))
                            .wrapping_add(i2.wrapping_sub(*lo2)),
                    )
                } else {
                    None
                }
            }
        }
    }

    /// The cause string for an association index that `offset` rejected: a
    /// structurally-wrong index (a non-`Int`, or a non-`Pair`-of-`Int`s for the
    /// 2-D shape) is reported with a shape-specific cause, and an index inside
    /// the right structure but outside the bounded rectangle is plain
    /// "index out of range".
    fn index_rejection_cause<T: Transcendental, const BUF: usize>(
        &self,
        ir: &Ir,
        world: &mut WorldState<T, BUF>,
        index: ArenaRef,
        drops: &mut Vec<ArenaRef>,
    ) -> &'static str {
        match self {
            ArrayShape::Dim1 { .. } => {
                if forced_i64(ir, world, index, drops).is_some() {
                    "index out of range"
                } else {
                    "association index is not an Int"
                }
            }
            ArrayShape::Dim2 { .. } => {
                let fields = match world.arena.get(index) {
                    Some(Value::Record(f)) if f.len() == 2 => [f[0], f[1]],
                    _ => return "association index is not a Pair of Ints",
                };
                if forced_i64(ir, world, fields[0], drops).is_some()
                    && forced_i64(ir, world, fields[1], drops).is_some()
                {
                    "index out of range"
                } else {
                    "association index is not a Pair of Ints"
                }
            }
        }
    }
}

/// Whether `offset` is a valid flat index into an array of `len` elements.
fn flat_index_in_range(offset: i64, len: usize) -> Option<usize> {
    usize::try_from(offset).ok().filter(|i| *i < len)
}

/// Allocate a `Pair Int Int` bounds record holding `(lo, hi)`.
///
/// The array records its bounds as an ordinary value (design §Value-level
/// design), so the buffer bridge builds the `Int`-indexed pair `(0, len - 1)`
/// here. Returns `None` (after releasing the partial allocations) on arena or
/// pool exhaustion.
fn alloc_int_bounds<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    lo: i64,
    hi: i64,
) -> Option<ArenaRef> {
    let lo_ref = alloc_owned(ir, world, Value::Int(lo));
    let hi_ref = alloc_owned(ir, world, Value::Int(hi));
    match (lo_ref, hi_ref) {
        (Some(l), Some(h)) => {
            let mut fields = match world.arena.take_buf(2) {
                Ok(b) => b,
                Err(_) => {
                    latch_exhaustion(ir, world, "value buffer pool");
                    world.arena.drop_ref(l);
                    world.arena.drop_ref(h);
                    return None;
                }
            };
            fields.push(l);
            fields.push(h);
            alloc_owned(ir, world, Value::Record(fields))
        }
        (l, h) => {
            if let Some(l) = l {
                world.arena.drop_ref(l);
            }
            if let Some(h) = h {
                world.arena.drop_ref(h);
            }
            None
        }
    }
}

/// Dispatch a collection operation (`ValueCallBuiltin`).
///
/// Container reads copy (RC++) any child refs the result claims so both the
/// source and the new container own them. Arena exhaustion is reported as a
/// build-time bug (`debug_assert!`) and a detectable no-op — open collections
/// grow up to the pre-allocated pool, so a correctly-budgeted program never
/// exhausts it.
fn exec_value_call_builtin<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    op: ValueBuiltinOp,
    args: &[usize],
    dst: usize,
    base: usize,
    drops: &mut Vec<ArenaRef>,
) {
    use ValueBuiltinOp::*;
    // LazyCons is the lazy list constructor (`:` in complete mode): it must
    // NOT force its tail (that is the laziness), so it bypasses the blanket
    // demand loop below. Nil has no arguments to demand.
    match op {
        LazyCons => {
            let Some(xr) = world.value_regs[base + args[0]].and_then(|r| world.arena.copy(r).ok())
            else {
                world.value_regs[dst] = None;
                return;
            };
            let Some(tr) = world.value_regs[base + args[1]].and_then(|r| world.arena.copy(r).ok())
            else {
                world.arena.drop_ref(xr);
                world.value_regs[dst] = None;
                return;
            };
            world.value_regs[dst] = alloc_owned(ir, world, Value::Cons(xr, tr));
            return;
        }
        Nil => {
            world.value_regs[dst] = alloc_owned(ir, world, Value::Nil);
            return;
        }
        _ => {}
    }
    // Demand: force every collection argument to WHNF before dispatch, and
    // propagate bottom. Each register is normalized to a fresh claim on its
    // WHNF head (the memo cell keeps its own claim on a forced result), so the
    // op bodies below read the heads unchanged. A bottom argument releases the
    // original claim (the bottom slot survives via its cell) and writes bottom
    // to the dst. In strict mode no thunks exist, so this is a no-op modulo RC.
    for &a in args {
        let Some(r) = world.value_regs.get_mut(base + a).and_then(|r| r.take()) else {
            continue;
        };
        let head = force(ir, world, r, drops);
        if matches!(world.arena.get(head), Some(Value::Bottom)) {
            world.arena.drop_ref(r);
            write_bottom(ir, world, dst, drops);
            return;
        }
        world.value_regs[base + a] = match world.arena.copy(head) {
            Ok(c) => Some(c),
            Err(_) => {
                world.arena.drop_ref(r);
                world.value_regs[dst] = None;
                return;
            }
        };
        world.arena.drop_ref(r);
    }
    match op {
        Cons => {
            // cons x xs: build a new List whose head is x followed by the
            // source elems (`x : xs`). The result recounts (RC++) the source
            // elems and the new element so both the source container and the
            // result own them (the source register is dropped at tick end).
            // Payloads are pooled — no per-tick heap allocation.
            let Some(mut src) = pooled_elems_of(ir, world, base + args[1]) else {
                world.value_regs[dst] = None;
                return;
            };
            let Some(xr) = world.value_regs[base + args[0]].and_then(|r| world.arena.copy(r).ok())
            else {
                world.value_regs[dst] = None;
                return;
            };
            for e in &src {
                _ = world.arena.copy(*e);
            }
            let mut out = match world.arena.take_buf(src.len() + 1) {
                Ok(b) => b,
                Err(_) => {
                    latch_exhaustion(ir, world, "value buffer pool");
                    world.value_regs[dst] = None;
                    return;
                }
            };
            out.push(xr);
            out.append(&mut src);
            // `src`'s refs moved into `out`; return the emptied pooled buffer.
            world.arena.put_buf(src);
            world.value_regs[dst] = alloc_owned(ir, world, Value::List { elems: out });
        }
        // Handled before the demand loop (they must not force their tail / have
        // no arguments); unreachable here.
        LazyCons | Nil => unreachable!("LazyCons/Nil handled before the demand loop"),
        Length => {
            // The IR doc says Length covers list/set/map: each container
            // reports its entry count. An immutable array reports its FLAT
            // element count — the least-surprise reading of "direct primitive
            // reads" (`arrayElems` returns exactly that many elements), and the
            // only count an array has that does not require `Ix` math. Falling
            // through to the `_ => 0` arm would silently report an empty
            // non-empty array.
            let n = world.value_regs[base + args[0]]
                .and_then(|r| world.arena.get(r))
                .map(|v| match v {
                    Value::List { elems } => elems.len() as i64,
                    Value::Map { pairs, .. } => pairs.len() as i64,
                    Value::Set { elems } => elems.len() as i64,
                    Value::Array { elems, .. } => elems.len() as i64,
                    _ => 0,
                })
                .unwrap_or(0);
            world.value_regs[dst] = alloc_owned(ir, world, Value::Int(n));
        }
        Head => {
            let first = {
                let Some(r) = world.value_regs[base + args[0]] else {
                    world.value_regs[dst] = None;
                    return;
                };
                match world.arena.get(r) {
                    Some(Value::List { elems }) => elems.first().copied(),
                    Some(Value::Cons(h, _)) => Some(*h),
                    _ => None,
                }
            };
            world.value_regs[dst] = match first {
                // `Just e`: the Sum owns a counted ref on the element (the list
                // keeps its own ownership). `Nothing`: an empty sum payload.
                Some(e) => match copy_owned(ir, world, e) {
                    Some(ce) => {
                        let mut payload = match world.arena.take_buf(1) {
                            Ok(b) => b,
                            Err(_) => {
                                latch_exhaustion(ir, world, "value buffer pool");
                                return;
                            }
                        };
                        payload.push(ce);
                        alloc_owned(ir, world, Value::Sum(0, payload))
                    }
                    None => None,
                },
                None => alloc_owned(ir, world, Value::Sum(1, vec![])),
            };
        }
        Map => {
            // map f xs: dispatch f per element via a one-arg closure call.
            let Some(src) = pooled_elems_of(ir, world, base + args[1]) else {
                world.value_regs[dst] = None;
                return;
            };
            let mut out = match world.arena.take_buf(src.len()) {
                Ok(b) => b,
                Err(_) => {
                    latch_exhaustion(ir, world, "value buffer pool");
                    world.value_regs[dst] = None;
                    return;
                }
            };
            for e in &src {
                if let Some(o) = call_closure_single(ir, world, base + args[0], *e, dst, drops) {
                    out.push(o);
                }
            }
            world.arena.put_buf(src);
            world.value_regs[dst] = alloc_owned(ir, world, Value::List { elems: out });
        }
        Fold => {
            // fold f z xs: seed the accumulator with a copy of z, then for each
            // element call f with (acc, elem); the result becomes the new acc.
            let Some(src) = pooled_elems_of(ir, world, base + args[2]) else {
                world.value_regs[dst] = None;
                return;
            };
            let Some(accr) = world.value_regs[base + args[1]] else {
                world.value_regs[dst] = None;
                return;
            };
            let mut cur = match copy_owned(ir, world, accr) {
                Some(c) => c,
                None => {
                    world.value_regs[dst] = None;
                    return;
                }
            };
            let mut failed = false;
            for e in &src {
                let pair = [cur, *e];
                let next = call_closure_args(ir, world, base + args[0], &pair, dst, drops);
                match next {
                    Some(n) => {
                        // Replace the previous accumulator copy with the
                        // fresh result: exactly one owner lives.
                        world.arena.drop_ref(cur);
                        cur = n;
                    }
                    None => {
                        failed = true;
                        break;
                    }
                }
            }
            world.arena.put_buf(src);
            if failed {
                world.arena.drop_ref(cur);
                world.value_regs[dst] = None;
            } else {
                world.value_regs[dst] = Some(cur);
            }
        }
        ListEmpty => {
            // list: an empty List. Open collections have no capacity — the
            // list grows via cons/append up to the pre-allocated arena pool.
            world.value_regs[dst] = alloc_owned(ir, world, Value::List { elems: Vec::new() });
        }
        Tail => {
            // tail xs: the source without its head. For a lazy Cons the tail is
            // returned UNFORCED (a thunk, another cons, or Nil) — this is the
            // laziness that makes infinite sequences productive. For the
            // array-backed List the result claims elems[1..]: recount (RC++)
            // every element the source owns, allocate the tail list over the
            // kept slice, then release the extra count on the removed head —
            // the source keeps its single claim, and the head is never freed
            // mid-tick. An empty array list is its own tail; Nil's tail is Nil.
            match world.value_regs[base + args[0]].and_then(|r| world.arena.get(r)) {
                Some(Value::Cons(_, t)) => {
                    world.value_regs[dst] = copy_owned(ir, world, *t);
                }
                Some(Value::Nil) => {
                    world.value_regs[dst] = alloc_owned(ir, world, Value::Nil);
                }
                _ => {
                    let Some(mut kept) = pooled_elems_of(ir, world, base + args[0]) else {
                        world.value_regs[dst] = None;
                        return;
                    };
                    if kept.is_empty() {
                        world.value_regs[dst] =
                            alloc_owned(ir, world, Value::List { elems: Vec::new() });
                        return;
                    }
                    for e in &kept {
                        _ = world.arena.copy(*e);
                    }
                    let head = kept[0];
                    kept.remove(0);
                    world.value_regs[dst] = alloc_owned(ir, world, Value::List { elems: kept });
                    world.arena.drop_ref(head);
                }
            }
        }
        Filter => {
            // filter p xs: keep the elements for which the unary predicate
            // returns Bool(true). A kept element is recounted (RC++) into the
            // result list so both the source and the result own it; the
            // predicate's Bool result is released after each call.
            let Some(src) = pooled_elems_of(ir, world, base + args[1]) else {
                world.value_regs[dst] = None;
                return;
            };
            let mut kept = match world.arena.take_buf(src.len()) {
                Ok(b) => b,
                Err(_) => {
                    latch_exhaustion(ir, world, "value buffer pool");
                    world.value_regs[dst] = None;
                    return;
                }
            };
            for e in &src {
                let pred = call_closure_single(ir, world, base + args[0], *e, dst, drops);
                let is_keep = match pred {
                    Some(pr) => match world.arena.get(pr) {
                        Some(Value::Bool(b)) => *b,
                        _ => false,
                    },
                    None => false,
                };
                if is_keep {
                    if let Ok(c) = world.arena.copy(*e) {
                        kept.push(c);
                    }
                }
                if let Some(pr) = pred {
                    world.arena.drop_ref(pr);
                }
            }
            world.arena.put_buf(src);
            world.value_regs[dst] = alloc_owned(ir, world, Value::List { elems: kept });
        }
        InsertMap => {
            // insert k v m: COW-insert (k, v) into the sorted map, or
            // COW-replace the value when an equal key already exists. Open
            // collections have no capacity bound. Map pair payloads stay
            // heap-backed in v1 (deferred pooling); the source is read via
            // short borrows, not a full value clone.
            let (Some(kr), Some(vr), Some(sr)) = (
                world.value_regs[base + args[0]],
                world.value_regs[base + args[1]],
                world.value_regs[base + args[2]],
            ) else {
                world.value_regs[dst] = None;
                return;
            };
            let (pos, dup) = {
                let Some(Value::Map { pairs, .. }) = world.arena.get(sr) else {
                    world.value_regs[dst] = None;
                    return;
                };
                let keys: Vec<ArenaRef> = pairs.iter().map(|(pk, _)| *pk).collect();
                let pos = sorted_insert_pos(ir, world, kr, &keys);
                let dup = pos < pairs.len() && value_cmp(&world.arena, kr, pairs[pos].0) == 0;
                (pos, dup)
            };
            let Some(Value::Map { pairs, .. }) = world.arena.get(sr).cloned() else {
                world.value_regs[dst] = None;
                return;
            };
            if dup {
                // Replace-on-duplicate: recount every pair ref, swap in the
                // new value (counted once for the map), and release the
                // replaced value's extra count — the source keeps its own
                // claim, so the replaced value is never freed mid-tick.
                let mut new_pairs = pairs;
                for (pk, pv) in &new_pairs {
                    _ = world.arena.copy(*pk);
                    _ = world.arena.copy(*pv);
                }
                let old_val = new_pairs[pos].1;
                new_pairs[pos].1 = vr;
                _ = world.arena.copy(vr);
                world.arena.drop_ref(old_val);
                world.value_regs[dst] = alloc_owned(ir, world, Value::Map { pairs: new_pairs });
            } else {
                // Insert at the sorted position: splice (k, v) into the cloned
                // pair list, then recount every key and value the new map
                // claims (the source and the result each own them).
                let mut new_pairs: Vec<(ArenaRef, ArenaRef)> = Vec::with_capacity(pairs.len() + 1);
                for (i, (pk, pv)) in pairs.iter().enumerate() {
                    if i == pos {
                        new_pairs.push((kr, vr));
                    }
                    new_pairs.push((*pk, *pv));
                }
                if new_pairs.len() == pairs.len() {
                    new_pairs.push((kr, vr));
                }
                for (pk, pv) in &new_pairs {
                    _ = world.arena.copy(*pk);
                    _ = world.arena.copy(*pv);
                }
                world.value_regs[dst] = alloc_owned(ir, world, Value::Map { pairs: new_pairs });
            }
        }
        Lookup => {
            // lookup k m: binary search the sorted map for the key; `Just v`
            // when found, `Nothing` otherwise (the same Sum encoding as
            // `head`). The found value ref is counted (RC++) so the Sum owns
            // it independently of the map.
            let (Some(kr), Some(sr)) = (
                world.value_regs[base + args[0]],
                world.value_regs[base + args[1]],
            ) else {
                world.value_regs[dst] = None;
                return;
            };
            let found = {
                let Some(Value::Map { pairs, .. }) = world.arena.get(sr) else {
                    world.value_regs[dst] = None;
                    return;
                };
                let keys: Vec<ArenaRef> = pairs.iter().map(|(pk, _)| *pk).collect();
                let pos = sorted_insert_pos(ir, world, kr, &keys);
                if pos < pairs.len() && value_cmp(&world.arena, kr, pairs[pos].0) == 0 {
                    Some(pairs[pos].1)
                } else {
                    None
                }
            };
            match found {
                Some(fv) => match copy_owned(ir, world, fv) {
                    Some(cv) => {
                        let mut payload = match world.arena.take_buf(1) {
                            Ok(b) => b,
                            Err(_) => {
                                latch_exhaustion(ir, world, "value buffer pool");
                                return;
                            }
                        };
                        payload.push(cv);
                        world.value_regs[dst] = alloc_owned(ir, world, Value::Sum(0, payload));
                    }
                    None => world.value_regs[dst] = None,
                },
                None => world.value_regs[dst] = alloc_owned(ir, world, Value::Sum(1, vec![])),
            }
        }
        Member => {
            // member k c: a shared op — the container's variant tells map
            // membership (among the sorted keys) from set membership (among
            // the sorted elements). A non-container reads as `false`.
            let (Some(kr), Some(sr)) = (
                world.value_regs[base + args[0]],
                world.value_regs[base + args[1]],
            ) else {
                world.value_regs[dst] = None;
                return;
            };
            let found = match world.arena.get(sr) {
                Some(Value::Map { pairs, .. }) => {
                    let keys: Vec<ArenaRef> = pairs.iter().map(|(pk, _)| *pk).collect();
                    let pos = sorted_insert_pos(ir, world, kr, &keys);
                    pos < pairs.len() && value_cmp(&world.arena, kr, pairs[pos].0) == 0
                }
                Some(Value::Set { elems }) => {
                    let pos = sorted_insert_pos(ir, world, kr, elems);
                    pos < elems.len() && value_cmp(&world.arena, kr, elems[pos]) == 0
                }
                _ => false,
            };
            world.value_regs[dst] = alloc_owned(ir, world, Value::Bool(found));
        }
        InsertSet => {
            // insert k s: COW-insert the element into the sorted set; a
            // duplicate leaves the set unchanged (a fresh COW copy). Open
            // collections have no capacity bound.
            let (Some(kr), Some(sr)) = (
                world.value_regs[base + args[0]],
                world.value_regs[base + args[1]],
            ) else {
                world.value_regs[dst] = None;
                return;
            };
            let (len, pos, dup) = {
                let Some(elems) = (match world.arena.get(sr) {
                    Some(Value::Set { elems }) => Some(elems),
                    _ => None,
                }) else {
                    world.value_regs[dst] = None;
                    return;
                };
                let pos = sorted_insert_pos(ir, world, kr, elems);
                let dup = pos < elems.len() && value_cmp(&world.arena, kr, elems[pos]) == 0;
                (elems.len(), pos, dup)
            };
            let mut out = match world.arena.take_buf(len + usize::from(!dup)) {
                Ok(b) => b,
                Err(_) => {
                    latch_exhaustion(ir, world, "value buffer pool");
                    world.value_regs[dst] = None;
                    return;
                }
            };
            if let Some(elems) = match world.arena.get(sr) {
                Some(Value::Set { elems }) => Some(elems),
                _ => None,
            } {
                out.extend_from_slice(elems);
            }
            if dup {
                for e in &out {
                    _ = world.arena.copy(*e);
                }
            } else {
                out.insert(pos, kr);
                for e in &out {
                    _ = world.arena.copy(*e);
                }
            }
            world.value_regs[dst] = alloc_owned(ir, world, Value::Set { elems: out });
        }
        MapEmpty => {
            // empty_map: an empty Map. Open collections have no capacity — the
            // map grows via insert up to the pre-allocated arena pool.
            world.value_regs[dst] = alloc_owned(ir, world, Value::Map { pairs: Vec::new() });
        }
        SetEmpty => {
            // empty_set: an empty Set. Open collections have no capacity.
            world.value_regs[dst] = alloc_owned(ir, world, Value::Set { elems: Vec::new() });
        }
        ConcatMap => {
            // bind xs f: for each element, call f -> a List, splicing all the
            // resulting lists into one (`concat_map`). Element and result
            // payloads are pooled.
            let Some(src) = pooled_elems_of(ir, world, base + args[1]) else {
                world.value_regs[dst] = None;
                return;
            };
            let mut out = match world.arena.take_buf(src.len()) {
                Ok(b) => b,
                Err(_) => {
                    latch_exhaustion(ir, world, "value buffer pool");
                    world.value_regs[dst] = None;
                    return;
                }
            };
            for e in &src {
                if let Some(lr) = call_closure_single(ir, world, base + args[0], *e, dst, drops) {
                    if let Some(sub) = pooled_elems_of_ref(ir, world, lr) {
                        for s in &sub {
                            if let Ok(c) = world.arena.copy(*s) {
                                out.push(c);
                            }
                        }
                        world.arena.put_buf(sub);
                    }
                    world.arena.drop_ref(lr);
                }
            }
            world.arena.put_buf(src);
            world.value_regs[dst] = alloc_owned(ir, world, Value::List { elems: out });
        }
        AppendList => {
            // append_list xs ys: the concatenation of two lists (Monoid
            // mappend). Every element is recounted for the result.
            let Some(mut left) = pooled_elems_of(ir, world, base + args[0]) else {
                world.value_regs[dst] = None;
                return;
            };
            let Some(mut right) = pooled_elems_of(ir, world, base + args[1]) else {
                world.value_regs[dst] = None;
                return;
            };
            for e in &left {
                _ = world.arena.copy(*e);
            }
            for e in &right {
                _ = world.arena.copy(*e);
            }
            let mut out = match world.arena.take_buf(left.len() + right.len()) {
                Ok(b) => b,
                Err(_) => {
                    latch_exhaustion(ir, world, "value buffer pool");
                    world.value_regs[dst] = None;
                    return;
                }
            };
            out.append(&mut left);
            out.append(&mut right);
            world.arena.put_buf(left);
            world.arena.put_buf(right);
            world.value_regs[dst] = alloc_owned(ir, world, Value::List { elems: out });
        }
        ConcatString => {
            let a = world
                .value_regs
                .get(base + args[0])
                .copied()
                .flatten()
                .and_then(|r| world.arena.get(r))
                .and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                });
            let b = world
                .value_regs
                .get(base + args[1])
                .copied()
                .flatten()
                .and_then(|r| world.arena.get(r))
                .and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                });
            world.value_regs[dst] = match (a, b) {
                (Some(x), Some(y)) => alloc_owned(ir, world, Value::String(format!("{x}{y}"))),
                _ => None,
            };
        }
        ShowFloat => {
            let v = world
                .value_regs
                .get(base + args[0])
                .copied()
                .flatten()
                .and_then(|r| world.arena.get(r))
                .and_then(value_to_f64);
            world.value_regs[dst] =
                v.and_then(|x| alloc_owned(ir, world, Value::String(format!("{x:?}"))));
        }
        ShowInt => {
            // `show_int 3` renders `3` (decimal, no fractional part). A Float
            // operand is truncated defensively; typing makes it unreachable.
            let v = world
                .value_regs
                .get(base + args[0])
                .copied()
                .flatten()
                .and_then(|r| world.arena.get(r))
                .and_then(|v| match v {
                    Value::Int(i) => Some(*i),
                    Value::Float(f) => Some(*f as i64),
                    _ => None,
                });
            world.value_regs[dst] =
                v.and_then(|i| alloc_owned(ir, world, Value::String(i.to_string())));
        }
        ShowBool => {
            // Booleans are the `true`/`false` keywords in source, but they
            // render with the Haskell constructor spelling (per the spec's
            // `show`), matching `show_float`'s `{:?}` conventions.
            let v = world
                .value_regs
                .get(base + args[0])
                .copied()
                .flatten()
                .and_then(|r| world.arena.get(r))
                .and_then(|v| match v {
                    Value::Bool(b) => Some(*b),
                    _ => None,
                });
            world.value_regs[dst] = v.and_then(|b| {
                alloc_owned(
                    ir,
                    world,
                    Value::String(if b { "True" } else { "False" }.to_string()),
                )
            });
        }
        // ---- Immutable array storage (flat, type-agnostic) ----
        ArrayFromList => {
            // arrayFromList bounds xs: store the bounds value verbatim (an
            // ordinary `Pair i i`) together with the flat elements. Both the
            // bounds ref and every element ref are recounted, so the new array
            // owns them independently of the source registers.
            //
            // The flat storage needs the ARRAY-BACKED list form. A cons-built
            // list (`1.0 : 2.0 : []`, which Complete mode lowers to a lazy cons
            // chain) has no flat element Vec, so it is a clean fault — leaving
            // the destination silently unbound would be a wrong result on the
            // canonical Haskell list syntax.
            let Some(elems) = pooled_elems_of(ir, world, base + args[1]) else {
                // (Pool exhaustion, if that was the cause, already latched;
                // first latch wins.)
                latch_fault(ir, world, "arrayFromList: expected a list of elements");
                world.value_regs[dst] = None;
                return;
            };
            let Some(bounds) = world.value_regs[base + args[0]] else {
                world.arena.put_buf(elems);
                latch_fault(ir, world, "arrayFromList: bounds operand is unbound");
                world.value_regs[dst] = None;
                return;
            };
            let Some(bounds) = copy_owned(ir, world, bounds) else {
                world.arena.put_buf(elems);
                world.value_regs[dst] = None;
                return;
            };
            for e in &elems {
                _ = world.arena.copy(*e);
            }
            world.value_regs[dst] = alloc_owned(ir, world, Value::Array { bounds, elems });
        }
        ArraySeed => {
            // arraySeed bounds init: build an array whose EVERY position is the
            // given seed value. `size` claims on the same init ref are pooled
            // (no per-position arena allocation; the array owns one ref per
            // slot, released on drop). Inverted bounds give a VALID EMPTY
            // array (Q3): `size <= 0` stores zero elements.
            let Some(bounds) = world.value_regs[base + args[0]] else {
                latch_fault(ir, world, "arraySeed: bounds operand is unbound");
                world.value_regs[dst] = None;
                return;
            };
            let shape = match array_shape(ir, world, bounds, drops) {
                Ok(shape) => shape,
                Err(why) => {
                    latch_fault(ir, world, &format!("arraySeed: {why}"));
                    world.value_regs[dst] = None;
                    return;
                }
            };
            let size = shape.size();
            let Some(init) = world.value_regs[base + args[1]] else {
                latch_fault(ir, world, "arraySeed: seed operand is unbound");
                world.value_regs[dst] = None;
                return;
            };
            let mut elems = Vec::new();
            if size > 0 {
                let n = usize::try_from(size).unwrap_or(usize::MAX);
                let mut buf = match world.arena.take_buf(n) {
                    Ok(b) => b,
                    Err(_) => {
                        latch_exhaustion(ir, world, "value buffer pool");
                        world.value_regs[dst] = None;
                        return;
                    }
                };
                let mut ok = true;
                for _ in 0..n {
                    match world.arena.copy(init) {
                        Ok(r) => buf.push(r),
                        Err(_) => {
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok {
                    for r in &buf {
                        world.arena.drop_ref(*r);
                    }
                    world.arena.put_buf(buf);
                    latch_exhaustion(ir, world, "value arena capacity");
                    world.value_regs[dst] = None;
                    return;
                }
                elems = buf;
            }
            let Some(bounds) = copy_owned(ir, world, bounds) else {
                for r in &elems {
                    world.arena.drop_ref(*r);
                }
                world.arena.put_buf(elems);
                world.value_regs[dst] = None;
                return;
            };
            world.value_regs[dst] = alloc_owned(ir, world, Value::Array { bounds, elems });
        }
        ArrayFromAssocs => {
            // arrayFromAssocs bounds assocs: build an immutable array whose
            // every position starts as ONE shared lazy hole (a callable over
            // the synthetic hole fragment), then overlay the index-typed
            // associations (last wins). Forcing an unlisted position runs the
            // hole fragment, which latches the clean "undefined array element"
            // fault (GHC's `undefined` for absent array positions). Large holes
            // are cheap: one callable claimed by every slot, no per-slot arena
            // allocation beyond the ref claims.
            let Some(bounds) = world.value_regs[base + args[0]] else {
                latch_fault(ir, world, "arrayFromAssocs: bounds operand is unbound");
                world.value_regs[dst] = None;
                return;
            };
            let shape = match array_shape(ir, world, bounds, drops) {
                Ok(shape) => shape,
                Err(why) => {
                    latch_fault(ir, world, &format!("arrayFromAssocs: {why}"));
                    world.value_regs[dst] = None;
                    return;
                }
            };
            let size = shape.size();
            let mut elems = Vec::new();
            if size > 0 {
                let n = usize::try_from(size).unwrap_or(usize::MAX);
                match world.arena.take_buf(n) {
                    Ok(buf) => elems = buf,
                    Err(_) => {
                        latch_exhaustion(ir, world, "value buffer pool");
                        world.value_regs[dst] = None;
                        return;
                    }
                }
                // Build the ONE shared hole: a callable over the synthetic hole
                // fragment with an empty env (the body takes no captures) and a
                // memo cell (a forced hole memoizes bottom into the SHARED
                // cell, so every position observes the same divergence). The
                // construction shape mirrors `ValueCallLazy`.
                let Some(frag) = ir.array_hole_frag else {
                    world.arena.put_buf(elems);
                    latch_fault(ir, world, "arrayFromAssocs: no hole fragment");
                    world.value_regs[dst] = None;
                    return;
                };
                let Some(cell) = alloc_owned(ir, world, Value::ThunkCell(ThunkState::Unforced))
                else {
                    world.arena.put_buf(elems);
                    world.value_regs[dst] = None;
                    return;
                };
                let Some(env) = alloc_owned(ir, world, Value::Record(vec![])) else {
                    world.arena.drop_ref(cell);
                    world.arena.put_buf(elems);
                    world.value_regs[dst] = None;
                    return;
                };
                let Some(hole) = alloc_owned(
                    ir,
                    world,
                    Value::Callable {
                        fragment: frag,
                        env,
                        cell: Some(cell),
                    },
                ) else {
                    world.arena.drop_ref(env);
                    world.arena.drop_ref(cell);
                    world.arena.put_buf(elems);
                    world.value_regs[dst] = None;
                    return;
                };
                // `size` claims on the same ref: one claim per slot, released
                // when the array (or a later association) drops it. The pooled
                // buffer starts EMPTY (`take_buf` returns len 0), so push one
                // claim per slot (mirrors the `arraySeed` fill).
                let mut ok = true;
                for _ in 0..n {
                    match world.arena.copy(hole) {
                        Ok(c) => elems.push(c),
                        Err(_) => {
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok {
                    for r in &elems {
                        world.arena.drop_ref(*r);
                    }
                    world.arena.drop_ref(hole);
                    world.arena.put_buf(elems);
                    latch_exhaustion(ir, world, "value arena capacity");
                    world.value_regs[dst] = None;
                    return;
                }
                world.arena.drop_ref(hole);
            }
            // Overlay the associations: resolve each index against the bounds
            // shape, replace the hole claim at that position. In-order
            // application gives last-wins (mirrors `arrayUpdate`'s duplicate-
            // offset rule, but index-typed).
            let Some(assocs) = pooled_elems_of(ir, world, base + args[1]) else {
                for r in &elems {
                    world.arena.drop_ref(*r);
                }
                world.arena.put_buf(elems);
                latch_fault(
                    ir,
                    world,
                    "arrayFromAssocs: expected a list of associations",
                );
                world.value_regs[dst] = None;
                return;
            };
            let mut fault: Option<String> = None;
            let mut aborted = false;
            for pair in &assocs {
                let fields = match world.arena.get(*pair) {
                    Some(Value::Record(fields)) if fields.len() == 2 => [fields[0], fields[1]],
                    _ => {
                        fault = Some(String::from("arrayFromAssocs: association is not a Pair"));
                        aborted = true;
                        break;
                    }
                };
                let Some(pos) = shape.offset(ir, world, fields[0], drops) else {
                    fault = Some(String::from("arrayFromAssocs: index out of range"));
                    aborted = true;
                    break;
                };
                let Some(fresh) = copy_owned(ir, world, fields[1]) else {
                    aborted = true;
                    break;
                };
                let old = elems[pos as usize];
                elems[pos as usize] = fresh;
                world.arena.drop_ref(old);
            }
            world.arena.put_buf(assocs);
            if aborted {
                for r in &elems {
                    world.arena.drop_ref(*r);
                }
                world.arena.put_buf(elems);
                if let Some(why) = fault {
                    latch_fault(ir, world, &why);
                }
                world.value_regs[dst] = None;
                return;
            }
            let Some(bounds) = copy_owned(ir, world, bounds) else {
                for r in &elems {
                    world.arena.drop_ref(*r);
                }
                world.arena.put_buf(elems);
                world.value_regs[dst] = None;
                return;
            };
            world.value_regs[dst] = alloc_owned(ir, world, Value::Array { bounds, elems });
        }
        ArrayBounds => {
            // arrayBounds arr: a direct primitive read of the stored bounds
            // value. The result claims its own ref.
            let bounds = match world.value_regs[base + args[0]].and_then(|r| world.arena.get(r)) {
                Some(Value::Array { bounds, .. }) => Some(*bounds),
                _ => None,
            };
            world.value_regs[dst] = match bounds.and_then(|b| copy_owned(ir, world, b)) {
                Some(b) => Some(b),
                None => {
                    // (Arena exhaustion, if that was the cause, already
                    // latched; first latch wins.)
                    latch_fault(ir, world, "arrayBounds: operand is not an array");
                    None
                }
            };
        }
        ArrayElems => {
            // arrayElems arr: a direct primitive read of the flat elements as
            // a List. Every element is recounted for the new list.
            let Some(elems) = copy_array_elems(ir, world, base + args[0]) else {
                latch_fault(ir, world, "arrayElems: operand is not an array");
                world.value_regs[dst] = None;
                return;
            };
            world.value_regs[dst] = alloc_owned(ir, world, Value::List { elems });
        }
        ArrayFromListChecked => {
            // arrayFromListChecked bounds rangeSize xs: the lenient constructor
            // behind the language-level `listArray`, GHC-faithful. A list
            // SHORTER than `rangeSize` pads the tail with the ONE shared lazy
            // hole (the same fault-on-force object as `array`'s unlisted
            // positions); a LONGER list is silently truncated. The element
            // buffer holds BORROWED refs (no RC change) and the claim loop
            // below claims every slot exactly once, so padding pushes the RAW
            // hole ref and the builder's own claim (`rc 1` from `alloc_owned`)
            // is released only AFTER that loop — never before, when a freed
            // hole slot would make the loop's `copy` a dangling ref.
            let Some(mut elems) = pooled_elems_of(ir, world, base + args[2]) else {
                latch_fault(ir, world, "listArray: expected a list of elements");
                world.value_regs[dst] = None;
                return;
            };
            let size_reg = match world.value_regs[base + args[1]] {
                Some(r) => r,
                None => {
                    world.arena.put_buf(elems);
                    latch_fault(ir, world, "listArray: the rangeSize operand is unbound");
                    world.value_regs[dst] = None;
                    return;
                }
            };
            let expected = match forced_i64(ir, world, size_reg, drops) {
                // A negative computed rangeSize means an empty domain (prelude
                // `Ix` returns 0 for inverted bounds; user instances could
                // return less — clamp defensively, mirroring GHC's
                // `safeRangeSize`).
                Some(n) => n.max(0),
                None => {
                    world.arena.put_buf(elems);
                    latch_fault(ir, world, "listArray: rangeSize is not an integral number");
                    world.value_regs[dst] = None;
                    return;
                }
            };
            let mut pad_hole: Option<ArenaRef> = None;
            if elems.len() as i64 > expected {
                // Extras are silently dropped. They were BORROWED refs (owned by
                // the source list), so a plain truncate suffices — no per-ref
                // drop, no RC bookkeeping.
                elems.truncate(expected as usize);
            } else if (elems.len() as i64) < expected {
                // Ensure the buffer can hold the padded length WITHOUT
                // reallocating on the eval path: re-take a pooled buffer sized
                // for `expected`, copy the borrowed refs across (no RC), return
                // the old buffer.
                if elems.capacity() < expected as usize {
                    match world.arena.take_buf(expected as usize) {
                        Ok(mut bigger) => {
                            bigger.extend_from_slice(&elems);
                            world.arena.put_buf(elems);
                            elems = bigger;
                        }
                        Err(_) => {
                            world.arena.put_buf(elems);
                            latch_exhaustion(ir, world, "value buffer pool");
                            world.value_regs[dst] = None;
                            return;
                        }
                    }
                }
                // Build the ONE shared hole exactly like `ArrayFromAssocs`: a
                // callable over the synthetic hole fragment with an empty env
                // and a memo cell (see that arm for the construction shape).
                let Some(frag) = ir.array_hole_frag else {
                    world.arena.put_buf(elems);
                    latch_fault(ir, world, "listArray: no hole fragment");
                    world.value_regs[dst] = None;
                    return;
                };
                let Some(cell) = alloc_owned(ir, world, Value::ThunkCell(ThunkState::Unforced))
                else {
                    world.arena.put_buf(elems);
                    world.value_regs[dst] = None;
                    return;
                };
                let Some(env) = alloc_owned(ir, world, Value::Record(vec![])) else {
                    world.arena.drop_ref(cell);
                    world.arena.put_buf(elems);
                    world.value_regs[dst] = None;
                    return;
                };
                let Some(hole) = alloc_owned(
                    ir,
                    world,
                    Value::Callable {
                        fragment: frag,
                        env,
                        cell: Some(cell),
                    },
                ) else {
                    world.arena.drop_ref(env);
                    world.arena.drop_ref(cell);
                    world.arena.put_buf(elems);
                    world.value_regs[dst] = None;
                    return;
                };
                // Push the RAW hole ref (NO `copy`): the claim loop below gives
                // each padded slot exactly one claim; the builder's own claim
                // keeps the target alive until the loop has run.
                while elems.len() < expected as usize {
                    elems.push(hole);
                }
                pad_hole = Some(hole);
            }
            let Some(bounds) = world.value_regs[base + args[0]] else {
                world.arena.put_buf(elems);
                latch_fault(ir, world, "listArray: bounds operand is unbound");
                world.value_regs[dst] = None;
                return;
            };
            let Some(bounds) = copy_owned(ir, world, bounds) else {
                world.arena.put_buf(elems);
                world.value_regs[dst] = None;
                return;
            };
            for e in &elems {
                _ = world.arena.copy(*e);
            }
            if let Some(hole) = pad_hole {
                // Release the builder's claim AFTER the claim loop: the slot now
                // has one claim per padded copy, which the array's drop
                // balances.
                world.arena.drop_ref(hole);
            }
            world.value_regs[dst] = alloc_owned(ir, world, Value::Array { bounds, elems });
        }
        ListRange => {
            // listRange (l, u): the inclusive `[l..=u]` as a flat Int list —
            // the single list-GENERATING primitive, and what `Ix.range` is
            // built on. `l > u` is the empty list (the canonical empty range is
            // `(0, -1)`, which therefore yields no elements). The size is
            // bounded by the arena's free slots up front, so an absurd computed
            // range is a clean fault rather than a long futile allocation.
            let Some(Value::Record(fields)) = world.value_regs[base + args[0]]
                .and_then(|r| world.arena.get(r))
                .cloned()
            else {
                latch_fault(ir, world, "listRange: bounds operand is not a Pair");
                world.value_regs[dst] = None;
                return;
            };
            if fields.len() != 2 {
                latch_fault(ir, world, "listRange: bounds operand is not a Pair");
                world.value_regs[dst] = None;
                return;
            }
            let lo = forced_i64(ir, world, fields[0], drops);
            let hi = forced_i64(ir, world, fields[1], drops);
            let (Some(lo), Some(hi)) = (lo, hi) else {
                latch_fault(ir, world, "listRange: bounds are not integral numbers");
                world.value_regs[dst] = None;
                return;
            };
            let n = if hi < lo {
                0usize
            } else {
                // `hi - lo + 1` cannot overflow for a sane bounds pair; a
                // pathological one saturates and is then rejected by the
                // free-slot bound below.
                usize::try_from((hi as i128) - (lo as i128) + 1).unwrap_or(usize::MAX)
            };
            let free_slots = world.arena.capacity().saturating_sub(world.arena.live());
            if n > free_slots {
                latch_fault(
                    ir,
                    world,
                    "listRange: range exceeds the value arena capacity",
                );
                world.value_regs[dst] = None;
                return;
            }
            let Some(mut elems) = (match world.arena.take_buf(n) {
                Ok(b) => Some(b),
                Err(_) => {
                    latch_exhaustion(ir, world, "value buffer pool");
                    None
                }
            }) else {
                world.value_regs[dst] = None;
                return;
            };
            let mut ok = true;
            for k in 0..n {
                match alloc_owned(ir, world, Value::Int(lo + k as i64)) {
                    Some(r) => elems.push(r),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                for e in &elems {
                    world.arena.drop_ref(*e);
                }
                world.arena.put_buf(elems);
                world.value_regs[dst] = None;
                return;
            }
            world.value_regs[dst] = alloc_owned(ir, world, Value::List { elems });
        }
        ArrayIndexFlat => {
            // arrayIndexFlat arr i: read a FLAT offset (the language computed
            // it via `Ix`). The three failure causes stay distinct — a
            // non-array operand, a non-Int offset, and an out-of-range offset;
            // the last is a clean runtime fault through the latched channel,
            // never a panic and never an index into the element Vec (design
            // §Fault policy).
            let len = match world.value_regs[base + args[0]].and_then(|r| world.arena.get(r)) {
                Some(Value::Array { elems, .. }) => elems.len(),
                _ => {
                    latch_fault(ir, world, "arrayIndexFlat: operand is not an array");
                    world.value_regs[dst] = None;
                    return;
                }
            };
            let offset = match world.value_regs[base + args[1]].and_then(|r| world.arena.get(r)) {
                Some(Value::Int(i)) => *i,
                _ => {
                    latch_fault(ir, world, "arrayIndexFlat: index is not an Int");
                    world.value_regs[dst] = None;
                    return;
                }
            };
            let Some(pos) = flat_index_in_range(offset, len) else {
                latch_fault(ir, world, "arrayIndexFlat: index out of range");
                world.value_regs[dst] = None;
                return;
            };
            let elem = match world.value_regs[base + args[0]].and_then(|r| world.arena.get(r)) {
                Some(Value::Array { elems, .. }) => Some(elems[pos]),
                _ => None,
            };
            world.value_regs[dst] = match elem.and_then(|e| copy_owned(ir, world, e)) {
                Some(e) => Some(e),
                None => {
                    latch_fault(ir, world, "arrayIndexFlat: element is unreadable");
                    None
                }
            };
        }
        ArrayUpdate | ArrayAccum => {
            // arrayUpdate arr assocs / arrayAccum arr f assocs: build a NEW
            // array (the source is never mutated). The source elements are
            // recounted into the result payload, then each INDEX-TYPED
            // (index, value) association — resolved against the source's STORED
            // bounds through the shared shape helper — replaces one element
            // (update) or folds the old value with the new one through `f`
            // (accum). Every failure keeps its own message (the op name plus
            // the specific cause): unsupported index shape, out-of-rectangle
            // index, malformed association, non-callable accumulator — all
            // through the clean fault channel.
            let op_name = if matches!(op, ArrayUpdate) {
                "arrayUpdate"
            } else {
                "arrayAccum"
            };
            let assoc_reg = if matches!(op, ArrayUpdate) {
                base + args[1]
            } else {
                base + args[2]
            };
            let Some(assocs) = pooled_elems_of(ir, world, assoc_reg) else {
                latch_fault(
                    ir,
                    world,
                    &format!("{op_name}: expected a list of associations"),
                );
                world.value_regs[dst] = None;
                return;
            };
            let Some(mut out) = copy_array_elems(ir, world, base + args[0]) else {
                world.arena.put_buf(assocs);
                latch_fault(ir, world, &format!("{op_name}: operand is not an array"));
                world.value_regs[dst] = None;
                return;
            };
            // The associations are INDEX-typed: resolve them against the
            // STORED bounds of the source array (not against a flat offset).
            let Some(stored_bounds) =
                world.value_regs[base + args[0]].and_then(|r| world.arena.get(r))
            else {
                world.arena.put_buf(assocs);
                for r in &out {
                    world.arena.drop_ref(*r);
                }
                world.arena.put_buf(out);
                latch_fault(ir, world, &format!("{op_name}: operand is not an array"));
                world.value_regs[dst] = None;
                return;
            };
            let shape = match stored_bounds {
                Value::Array { bounds, .. } => match array_shape(ir, world, *bounds, drops) {
                    Ok(s) => s,
                    Err(why) => {
                        world.arena.put_buf(assocs);
                        for r in &out {
                            world.arena.drop_ref(*r);
                        }
                        world.arena.put_buf(out);
                        latch_fault(ir, world, &format!("{op_name}: {why}"));
                        world.value_regs[dst] = None;
                        return;
                    }
                },
                _ => {
                    world.arena.put_buf(assocs);
                    for r in &out {
                        world.arena.drop_ref(*r);
                    }
                    world.arena.put_buf(out);
                    latch_fault(ir, world, &format!("{op_name}: operand is not an array"));
                    world.value_regs[dst] = None;
                    return;
                }
            };
            // `Some` carries a cause; `None` while `aborted` means the failure
            // was already latched (arena exhaustion).
            let mut fault: Option<String> = None;
            let mut aborted = false;
            for pair in &assocs {
                // Read the association in a short borrow, then mutate the
                // payload (an `accum` closure call may dispatch a fragment).
                let (index, val) = match indexed_assoc(world, *pair) {
                    Ok(a) => a,
                    Err(why) => {
                        fault = Some(format!("{op_name}: {why}"));
                        aborted = true;
                        break;
                    }
                };
                let Some(offset) = shape.offset(ir, world, index, drops) else {
                    fault = Some(format!(
                        "{op_name}: {}",
                        shape.index_rejection_cause(ir, world, index, drops)
                    ));
                    aborted = true;
                    break;
                };
                // `offset` is in `[0, size)` of the STORED bounds; the recounted
                // payload has exactly `size` claims, so it is in range.
                let Some(pos) = flat_index_in_range(offset, out.len()) else {
                    fault = Some(format!("{op_name}: index out of range"));
                    aborted = true;
                    break;
                };
                let fresh = if matches!(op, ArrayUpdate) {
                    // Update: the result claims the new value directly.
                    // (`copy_owned` latches exhaustion itself.)
                    copy_owned(ir, world, val)
                } else {
                    // Accum: `f old new`, applied through the closure register
                    // (a curried nest handles the two-argument application).
                    let closure = args[1];
                    call_closure_args(ir, world, base + closure, &[out[pos], val], dst, drops)
                };
                match fresh {
                    Some(f) => {
                        // Replace the result's claim on the old element; the
                        // source array keeps its own claim, so nothing is freed
                        // mid-tick and the original stays intact.
                        let old = out[pos];
                        out[pos] = f;
                        world.arena.drop_ref(old);
                    }
                    None => {
                        // An update only fails on arena exhaustion (already
                        // latched); an accum fails when `f` is not callable.
                        if matches!(op, ArrayAccum) {
                            fault = Some(format!("{op_name}: accumulator is not a function"));
                        }
                        aborted = true;
                        break;
                    }
                }
            }
            world.arena.put_buf(assocs);
            if aborted {
                // Release exactly the claims the result payload holds (the
                // source array's claims are untouched).
                for r in &out {
                    world.arena.drop_ref(*r);
                }
                world.arena.put_buf(out);
                if let Some(why) = fault {
                    latch_fault(ir, world, &why);
                }
                world.value_regs[dst] = None;
                return;
            }
            let Some(Value::Array { bounds, .. }) =
                world.value_regs[base + args[0]].and_then(|r| world.arena.get(r))
            else {
                unreachable!("the source was read as an array above");
            };
            let Some(bounds) = world.arena.copy(*bounds).ok() else {
                for r in &out {
                    world.arena.drop_ref(*r);
                }
                world.arena.put_buf(out);
                world.value_regs[dst] = None;
                return;
            };
            world.value_regs[dst] = alloc_owned(ir, world, Value::Array { bounds, elems: out });
        }
        ArrayFromBuffer => {
            // arrayFromBuffer buf: copy the live block (`block_len` samples)
            // into an immutable `Int`-indexed `Float` array with bounds
            // `(0, len - 1)`. The samples are read out first (a short buffer
            // borrow) so the per-element arena allocations happen afterwards.
            let si = buffer_index(ir, world, args[0], base);
            let n = world.block_len.min(world.buffers[si].len());
            let mut scratch = [T::ZERO; BUF];
            world.buffers[si].with_data(|s| scratch[..n].copy_from_slice(&s[..n]));
            let mut elems = match world.arena.take_buf(n) {
                Ok(b) => b,
                Err(_) => {
                    latch_exhaustion(ir, world, "value buffer pool");
                    world.value_regs[dst] = None;
                    return;
                }
            };
            for sample in &scratch[..n] {
                match world.arena.alloc(Value::Float(sample.to_f64())) {
                    Ok(r) => elems.push(r),
                    Err(_) => {
                        for r in &elems {
                            world.arena.drop_ref(*r);
                        }
                        world.arena.put_buf(elems);
                        latch_exhaustion(ir, world, "value arena capacity");
                        world.value_regs[dst] = None;
                        return;
                    }
                }
            }
            // `lo = 0`, `hi = len - 1` (an empty buffer yields `(0, -1)`, the
            // conventional empty range).
            let Some(bounds) = alloc_int_bounds(ir, world, 0, n as i64 - 1) else {
                for r in &elems {
                    world.arena.drop_ref(*r);
                }
                world.arena.put_buf(elems);
                world.value_regs[dst] = None;
                return;
            };
            world.value_regs[dst] = alloc_owned(ir, world, Value::Array { bounds, elems });
        }
        BufferFromArray => {
            // bufferFromArray arr: copy the array's samples into a program
            // buffer. The destination is the destination register's 1:1 buffer
            // (the whole-buffer value convention), so the result is a
            // `Value::Buffer` usable anywhere a signal buffer is.
            //
            // Buffers are allocated 1:1 with value registers
            // (`Ir::num_buffers == Ir::next_value_reg`) and `dst` arrives
            // ABSOLUTE (the caller already added `base`), so the destination
            // buffer index is `dst` itself. `buffer_index` must not be used
            // here: it would add `base` a second time.
            let di = dst;
            let elems = match world.value_regs[base + args[0]].and_then(|r| world.arena.get(r)) {
                Some(Value::Array { elems, .. }) => Some(elems),
                _ => None,
            };
            let Some(elems) = elems else {
                latch_fault(ir, world, "bufferFromArray: operand is not an array");
                world.value_regs[dst] = None;
                return;
            };
            let len = elems.len();
            if len > BUF {
                latch_fault(
                    ir,
                    world,
                    "bufferFromArray: array exceeds the buffer capacity",
                );
                world.value_regs[dst] = None;
                return;
            }
            let mut scratch = [T::ZERO; BUF];
            for (i, r) in elems.iter().enumerate() {
                if let Some(Value::Float(f)) = world.arena.get(*r) {
                    scratch[i] = T::from_f64(*f);
                }
            }
            let n = world.block_len.min(world.buffers[di].len());
            world.buffers[di].with_data_mut(|b| {
                let copy = len.min(n);
                b[..copy].copy_from_slice(&scratch[..copy]);
                // Zero-pad a shorter array so the live block is deterministic.
                if copy < n {
                    b[copy..n].fill(T::ZERO);
                }
            });
            world.value_regs[dst] = alloc_owned(ir, world, Value::Buffer(di));
        }
    }
}

/// Bind `elem` as the fragment's single argument register, run the callable, and
/// return the copied result ref (or `None` when the slot holds no callable).
///
/// Thin wrapper over [`call_closure_args`].
fn call_closure_single<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    closure_reg: usize,
    elem: ArenaRef,
    result_reg: usize,
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    call_closure_args(
        ir,
        world,
        closure_reg,
        std::slice::from_ref(&elem),
        result_reg,
        drops,
    )
}

/// Bind `args` as a callable call's argument registers and run the fragment,
/// returning the copied result ref (or `None` on any unbound/errored step).
///
/// The arg refs are placed in the pre-allocated call-scratch slots as raw
/// borrows — `run_call` copies each (RC++) into the same slots before running
/// and drains them on return, so the source containers keep their own ownership
/// and the borrow never leaks. `result_reg` is a program-level register (below
/// the scratch watermark), so the drained range cannot reclaim it; its previous
/// occupant is dropped first, because `run_call` overwrites `dst` without
/// releasing the old ref.
fn call_closure_args<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    closure_reg: usize,
    args: &[ArenaRef],
    result_reg: usize,
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    // A curried closure is a NEST of single-argument fragments (`\a -> \b -> …`
    // lowers to a one-parameter fragment whose body returns the inner closure),
    // so more arguments than the fragment declares are applied one batch at a
    // time: the intermediate result becomes the next callee.
    let mut callee_reg = closure_reg;
    let mut start = 0usize;
    loop {
        let (env_ref, fragment_id) = match world.value_regs.get(callee_reg).copied().flatten() {
            Some(r) => match world.arena.get(r) {
                Some(Value::Callable {
                    fragment,
                    env,
                    cell: None,
                }) => (*env, *fragment),
                _ => return None,
            },
            None => return None,
        };
        let frag = ir.fragments.get(fragment_id as usize).cloned()?;
        let base = world.value_regs_top;
        // Bounds-guard: the pre-sized call scratch reserves `max_call_regs`
        // slots, so a wrong-arity callable (e.g. `fold (fn a -> a) ...`) could
        // pass more args than the fragment declares and write the raw borrows
        // past the store. Clamp to `frag.sig.value_ins` and the store's tail;
        // the type checker rejects wrong-arity callables at compile time, this
        // is belt-and-suspenders so a bad callable cannot crash the RT path.
        let remaining = &args[start..];
        let n = remaining
            .len()
            .min(frag.sig.value_ins)
            .min(world.value_regs.len().saturating_sub(base));
        if n < remaining.len() && n < frag.sig.value_ins {
            // The scratch region cannot hold the arguments: nothing to do.
            return None;
        }
        for (i, e) in remaining.iter().take(n).enumerate() {
            world.value_regs[base + i] = Some(*e);
        }
        if let Some(old) = world.value_regs.get_mut(result_reg).and_then(|r| r.take()) {
            drops.push(old);
        }
        let arg_regs: Vec<usize> = (0..n).map(|i| base + i).collect();
        run_call(
            ir,
            world,
            &frag,
            env_ref,
            ArgsSource::Registers {
                regs: &arg_regs,
                base: 0,
            },
            Some(result_reg),
            drops,
        );
        let out = world.value_regs.get_mut(result_reg).and_then(|r| r.take());
        start += n;
        if start >= args.len() {
            return out;
        }
        // More arguments remain: the result must itself be a closure. Park it in
        // the (now free) result register and use it as the next callee.
        world.value_regs[result_reg] = Some(out?);
        callee_reg = result_reg;
    }
}

/// Where a fragment call's value arguments come from.
///
/// An eager call passes the caller's value-register indices
/// ([`ArgsSource::Registers`]); a lazy thunk carries no stored argument
/// registers — its env record IS the captured frame, and the LAST
/// `frag.sig.value_ins` fields of that record are the function's arguments
/// ([`ArgsSource::EnvTail`]).
#[derive(Clone, Copy)]
enum ArgsSource<'a> {
    /// Value args come from the caller's value registers (eager call). `base`
    /// is the caller fragment's register base, so each arg index resolves to
    /// `base + regs[i]` — the raw instruction args are fragment-local and must
    /// not be read as absolute registers.
    Registers { regs: &'a [usize], base: usize },
    /// Value args come from the env-record tail split by the fragment's
    /// `value_ins` (lazy thunk).
    EnvTail,
}

/// What an [`apply_callable`] callee resolves to after forcing.
enum CallableKind {
    /// A primitive effect closure with its captured argument record.
    Effect(crate::ir::ValueEffectOp, crate::arena::ArenaRef),
    /// An ordinary function closure: fragment id + captured env record.
    Func(u32, crate::arena::ArenaRef),
}

/// Apply a first-class callable to ONE argument and return the result as a
/// fresh owner.
///
/// Two callable kinds are handled uniformly:
/// - [`Value::Callable`]: dispatch to its fragment, binding the argument as the
///   fragment's single value parameter (the ordinary curried-call path).
/// - [`Value::BuiltinCallable`]: run the primitive effect it names, applied to
///   the argument (the STState token threading an ST region).
///
/// The argument ref is only BORROWED: the fragment call copies it into its
/// frame, and the effect handler copies whatever it stores. The caller keeps
/// its own ownership.
fn apply_callable<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    callee: crate::arena::ArenaRef,
    arg: crate::arena::ArenaRef,
    drops: &mut Vec<crate::arena::ArenaRef>,
) -> Option<crate::arena::ArenaRef> {
    // A complete-mode `let` may hold the callee as a lazy thunk: force to WHNF
    // first (a no-op for an eager closure/effect).
    let head = force(ir, world, callee, drops);
    let kind = match world.arena.get(head) {
        Some(Value::BuiltinCallable { op, env }) => CallableKind::Effect(*op, *env),
        Some(Value::Callable {
            fragment,
            env,
            cell: None,
        }) => CallableKind::Func(*fragment, *env),
        _ => return None,
    };
    match kind {
        CallableKind::Effect(op, env) => exec_effect(ir, world, op, env, arg, drops),
        CallableKind::Func(fragment, env) => {
            let frag = ir.fragments.get(fragment as usize).cloned()?;
            // Park the borrowed argument in its own scratch slot, claiming it
            // so `run_call`'s frame starts ABOVE it. `run_call` copies the
            // argument into the fragment frame, so the slot never owns it.
            let slot = world.value_regs_top;
            if slot >= world.value_regs.len() {
                latch_exhaustion(ir, world, "call scratch (max_call_regs)");
                return None;
            }
            world.value_regs[slot] = Some(arg);
            world.value_regs_top = slot + 1;
            let out = run_call(
                ir,
                world,
                &frag,
                env,
                ArgsSource::Registers {
                    regs: &[0],
                    base: slot,
                },
                None,
                drops,
            );
            world.value_regs_top = slot;
            world.value_regs[slot] = None;
            out
        }
    }
}

/// Force a call result to WHNF and return it as a fresh OWNER.
///
/// The applied result of a complete-mode computation may be a suspended call
/// (`Value::Callable` with a memo cell). Comparing the forced ref against the
/// input tells whether the caller's own reference is already the WHNF value
/// (then it stays the owner) or a borrowed memoized value (then it is copied).
fn force_owned<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    r: crate::arena::ArenaRef,
    drops: &mut Vec<crate::arena::ArenaRef>,
) -> Option<crate::arena::ArenaRef> {
    let head = force(ir, world, r, drops);
    if head == r {
        Some(r)
    } else {
        let owned = copy_owned(ir, world, head);
        world.arena.drop_ref(r);
        owned
    }
}

/// The value ref stored in an `STRef` cell (a short borrow, no recount).
///
/// `Some(None)` is an unset slot (an uninitialized cell reads as unit,
/// defensively); `None` latches a wrong-kind fault — a handle pointing at an
/// `STArray` cell, which typed code cannot produce.
fn st_ref_cell_value<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    h: usize,
) -> Option<Option<ArenaRef>> {
    match world.st_cells.get(h).and_then(|slot| slot.as_ref()) {
        Some(StCell::Ref(r)) => Some(Some(*r)),
        Some(StCell::Array { .. }) => {
            latch_fault(ir, world, "STRef operation on an STArray handle");
            None
        }
        None => Some(None),
    }
}

/// Store `stored` in an `STRef` cell, releasing the previous value's claim.
///
/// A handle that does not name a live `STRef` cell — a wrong-kind `STArray`
/// handle, or a released cell — latches its own fault and drops `stored`, so the
/// refcount stays balanced and the store is never a SILENT no-op. Typed code
/// cannot produce either handle.
fn st_ref_cell_store<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    h: usize,
    stored: ArenaRef,
) {
    // Classify BEFORE mutating: `latch_fault` borrows the whole program, so the
    // cell cannot stay borrowed across it.
    match world.st_cells.get(h).and_then(|slot| slot.as_ref()) {
        Some(StCell::Ref(_)) => {}
        Some(StCell::Array { .. }) => {
            latch_fault(ir, world, "STRef operation on an STArray handle");
            world.arena.drop_ref(stored);
            return;
        }
        None => {
            latch_fault(ir, world, "STRef store on a released cell");
            world.arena.drop_ref(stored);
            return;
        }
    }
    match world.st_cells.get_mut(h) {
        // The classification above guarantees this arm.
        Some(Some(StCell::Ref(slot))) => {
            let old = std::mem::replace(slot, stored);
            world.arena.drop_ref(old);
        }
        _ => world.arena.drop_ref(stored),
    }
}

/// The flat element count of an `STArray` cell, or `None` after latching a
/// wrong-kind/released-cell fault (typed code cannot reach either).
fn st_array_len<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    h: usize,
) -> Option<usize> {
    match world.st_cells.get(h).and_then(|slot| slot.as_ref()) {
        Some(StCell::Array { elems, .. }) => Some(elems.len()),
        Some(StCell::Ref(_)) => {
            latch_fault(ir, world, "STArray operation on an STRef handle");
            None
        }
        None => {
            latch_fault(ir, world, "STArray operation on a released cell");
            None
        }
    }
}

/// The stored bounds ref of an `STArray` cell (`None` latches a fault).
fn st_array_bounds<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    h: usize,
) -> Option<ArenaRef> {
    match world.st_cells.get(h).and_then(|slot| slot.as_ref()) {
        Some(StCell::Array { bounds, .. }) => Some(*bounds),
        Some(StCell::Ref(_)) => {
            latch_fault(ir, world, "STArray operation on an STRef handle");
            None
        }
        None => {
            latch_fault(ir, world, "STArray operation on a released cell");
            None
        }
    }
}

/// The element ref at `pos` of an `STArray` cell (a borrow, no recount).
///
/// Both failure causes are named explicitly: a wrong-kind `STRef` handle must not
/// be reported as a released cell (the callers' `st_array_len` guard happens to
/// run first, but the message must not depend on that ordering).
fn st_array_elem<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    h: usize,
    pos: usize,
) -> Option<ArenaRef> {
    match world.st_cells.get(h).and_then(|slot| slot.as_ref()) {
        Some(StCell::Array { elems, .. }) => elems.get(pos).copied(),
        Some(StCell::Ref(_)) => {
            latch_fault(ir, world, "STArray operation on an STRef handle");
            None
        }
        None => {
            latch_fault(ir, world, "STArray element read on a released cell");
            None
        }
    }
}

/// Store `stored` at `pos` of an `STArray` cell, releasing the previous
/// element's claim.
///
/// A handle/position that does not name a live element latches its own fault and
/// drops `stored`, so the refcount stays balanced and the store is never a
/// SILENT no-op (unreachable through typed code).
fn st_array_elem_store<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    h: usize,
    pos: usize,
    stored: ArenaRef,
) {
    // Classify BEFORE mutating: `latch_fault` borrows the whole program.
    match world.st_cells.get(h).and_then(|slot| slot.as_ref()) {
        Some(StCell::Array { elems, .. }) if pos < elems.len() => {}
        Some(StCell::Array { .. }) => {
            latch_fault(ir, world, "STArray element store: offset out of range");
            world.arena.drop_ref(stored);
            return;
        }
        Some(StCell::Ref(_)) => {
            latch_fault(ir, world, "STArray operation on an STRef handle");
            world.arena.drop_ref(stored);
            return;
        }
        None => {
            latch_fault(ir, world, "STArray element store on a released cell");
            world.arena.drop_ref(stored);
            return;
        }
    }
    match world.st_cells.get_mut(h) {
        // The classification above guarantees this arm.
        Some(Some(StCell::Array { elems, .. })) if pos < elems.len() => {
            let old = std::mem::replace(&mut elems[pos], stored);
            world.arena.drop_ref(old);
        }
        _ => world.arena.drop_ref(stored),
    }
}

/// Snapshot an `STArray` cell's element refs into a pooled buffer.
///
/// No recount: the cell keeps its own claims, and the caller `copy`s exactly
/// the ones its result claims.
fn st_array_elems_of<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    h: usize,
) -> Option<Vec<ArenaRef>> {
    let len = match world.st_cells.get(h).and_then(|slot| slot.as_ref()) {
        Some(StCell::Array { elems, .. }) => elems.len(),
        Some(StCell::Ref(_)) => {
            latch_fault(ir, world, "STArray operation on an STRef handle");
            return None;
        }
        None => {
            latch_fault(ir, world, "getElems on a released STArray cell");
            return None;
        }
    };
    let mut out = match world.arena.take_buf(len) {
        Ok(b) => b,
        Err(_) => {
            latch_exhaustion(ir, world, "value buffer pool");
            return None;
        }
    };
    if let Some(StCell::Array { elems, .. }) = world.st_cells.get(h).and_then(|slot| slot.as_ref())
    {
        out.extend_from_slice(elems);
    }
    Some(out)
}

/// Release one ST cell's arena claims (region exit, or a wrong-kind overwrite).
///
/// The element payload buffer returns to the pool, keeping the take/put pairs
/// balanced across ticks.
fn release_st_cell<T: Transcendental, const BUF: usize>(
    _ir: &Ir,
    world: &mut WorldState<T, BUF>,
    cell: StCell,
) {
    match cell {
        StCell::Ref(r) => world.arena.drop_ref(r),
        StCell::Array { bounds, elems } => {
            world.arena.drop_ref(bounds);
            for e in &elems {
                world.arena.drop_ref(*e);
            }
            world.arena.put_buf(elems);
        }
    }
}

/// Force `r` and read its numeric payload as an integer.
///
/// The static type is `Int`, but the runtime payload of a NEGATED literal
/// (`-1`) is a `Float`: value-track arithmetic (negation included) is
/// Float-only, so an `Int`-typed slot can hold either kind. An accepted `Float`
/// must be INTEGRAL (`fract() == 0.0`) — truncating `1.5` to `1` would silently
/// address the wrong element. A non-numeric or fractional operand yields `None`;
/// the caller latches the operation-specific fault.
fn forced_i64<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    r: ArenaRef,
    drops: &mut Vec<ArenaRef>,
) -> Option<i64> {
    let r = force(ir, world, r, drops);
    match world.arena.get(r) {
        Some(Value::Int(i)) => Some(*i),
        Some(Value::Float(f)) if f.is_finite() && f.fract() == 0.0 => Some(*f as i64),
        _ => None,
    }
}

/// Execute one primitive effect against the threading `state` token and return
/// `Pair token result` as a fresh owner.
///
/// `env` is the effect closure's captured argument record (in application
/// order); `state` is the current token (`Value::Opaque`): the region id for the
/// `STRef`/`STArray` operations, the realized `World` token for the io-streams
/// operations. The returned `Pair` holds a FRESH token and the operation's
/// result:
/// - `NewStRef` → the new cell handle,
/// - `ReadStRef` → the cell's value,
/// - `WriteStRef`/`ModifyStRef` → `Void`,
/// - `NewArrayFlat` → the new array cell handle,
/// - `ReadArrayFlat` → the element at the flat offset,
/// - `WriteArrayFlat`/`ModifyArrayFlat` → `Void`,
/// - `GetArrayBounds` → the cell's `Pair i i` bounds,
/// - `GetArrayElems` → the cell's current elements as a `List e`,
/// - `StreamFromList` → the stream handle,
/// - `StreamToList` → the drained `List`,
/// - `StreamWrite` → `Void`.
fn exec_effect<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    op: crate::ir::ValueEffectOp,
    env: crate::arena::ArenaRef,
    state: crate::arena::ArenaRef,
    drops: &mut Vec<crate::arena::ArenaRef>,
) -> Option<crate::arena::ArenaRef> {
    // The threading token: an opaque region id (ST) or the realized World
    // token (IO). Anything else is a bug upstream.
    let Some(Value::Opaque(id)) = world.arena.get(state) else {
        return None;
    };
    let token = *id;
    let region_id = token as usize;
    // The captured operands, borrowed from the env record.
    let args: Vec<crate::arena::ArenaRef> = match world.arena.get(env) {
        Some(Value::Record(fields)) => fields.clone(),
        _ => Vec::new(),
    };
    // The cell handle is the first captured operand of the ref operations. A
    // `do`-bound `r <- newSTRef …` is bound lazily, so the captured operand may
    // still be an unforced thunk: force it to WHNF before reading its payload.
    let handle =
        |ir: &Ir, world: &mut WorldState<T, BUF>, drops: &mut Vec<ArenaRef>| -> Option<usize> {
            let r = force(ir, world, *args.first()?, drops);
            match world.arena.get(r) {
                Some(Value::Opaque(h)) => Some(*h as usize),
                _ => None,
            }
        };
    let result: Option<crate::arena::ArenaRef> = match op {
        crate::ir::ValueEffectOp::NewStRef => {
            let init = *args.first()?;
            // The cell owns its own ref; the operand's owner keeps its.
            let stored = copy_owned(ir, world, init)?;
            let id = world.st_cells.len() as u32;
            world.st_cells.push(Some(StCell::Ref(stored)));
            let region = world.regions.get_mut(region_id)?;
            region.cells.push(id);
            alloc_owned(ir, world, Value::Opaque(id))
        }
        crate::ir::ValueEffectOp::ReadStRef => {
            let h = handle(ir, world, drops)?;
            match st_ref_cell_value(ir, world, h)? {
                Some(v) => copy_owned(ir, world, v),
                // An uninitialized cell reads as unit (defensive; typing makes
                // it unreachable).
                None => alloc_owned(ir, world, Value::Void),
            }
        }
        crate::ir::ValueEffectOp::WriteStRef => {
            let h = handle(ir, world, drops)?;
            let src = *args.get(1)?;
            let stored = copy_owned(ir, world, src)?;
            st_ref_cell_store(ir, world, h, stored);
            alloc_owned(ir, world, Value::Void)
        }
        crate::ir::ValueEffectOp::ModifyStRef => {
            let h = handle(ir, world, drops)?;
            let f = *args.get(1)?;
            let old = st_ref_cell_value(ir, world, h).flatten()?;
            // The stored value may be an unforced thunk: the modifier function
            // receives its WHNF form.
            let old = force(ir, world, old, drops);
            // Apply the function to a copy of the current value (the cell keeps
            // its own ref while the call runs).
            let arg = copy_owned(ir, world, old)?;
            let new_value = apply_callable(ir, world, f, arg, drops);
            world.arena.drop_ref(arg);
            let new_value = new_value?;
            let stored = copy_owned(ir, world, new_value);
            world.arena.drop_ref(new_value);
            let stored = stored?;
            st_ref_cell_store(ir, world, h, stored);
            alloc_owned(ir, world, Value::Void)
        }
        // ---- mutable-array (STArray) effects ----
        //
        // Both a flat SIZE and a flat OFFSET are computed by the LANGUAGE (the
        // `Ix` class lands in a later task), so the interpreter trusts a numeric
        // operand only after bounding it: a size is checked against the arena's
        // free slots, an offset against the cell's element count. Every failure
        // is a distinct, latched fault — never a panic and never an OOB index.
        crate::ir::ValueEffectOp::NewArrayFlat => {
            let bounds_src = *args.first()?;
            let Some(size) = forced_i64(ir, world, *args.get(1)?, drops) else {
                latch_fault(
                    ir,
                    world,
                    "newArrayFlat: flat size is not an integral number",
                );
                return None;
            };
            let init = *args.get(2)?;
            if size < 0 {
                latch_fault(ir, world, "newArrayFlat: negative flat size");
                return None;
            }
            let n = size as usize;
            // The flat size is not known at build time, so it is not part of the
            // pre-allocated arena budget: bound it by the FREE slots up front so
            // an absurd computed size is a clean fault instead of a long futile
            // allocation loop.
            let free_slots = world.arena.capacity().saturating_sub(world.arena.live());
            if n > free_slots {
                latch_fault(
                    ir,
                    world,
                    "newArrayFlat: flat size exceeds the value arena capacity",
                );
                return None;
            }
            let bounds = copy_owned(ir, world, bounds_src)?;
            let mut elems = match world.arena.take_buf(n) {
                Ok(b) => b,
                Err(_) => {
                    world.arena.drop_ref(bounds);
                    latch_exhaustion(ir, world, "value buffer pool");
                    return None;
                }
            };
            for _ in 0..n {
                match copy_owned(ir, world, init) {
                    Some(c) => elems.push(c),
                    None => {
                        world.arena.drop_ref(bounds);
                        for e in &elems {
                            world.arena.drop_ref(*e);
                        }
                        world.arena.put_buf(elems);
                        return None;
                    }
                }
            }
            let id = world.st_cells.len() as u32;
            world.st_cells.push(Some(StCell::Array { bounds, elems }));
            let region = world.regions.get_mut(region_id)?;
            region.cells.push(id);
            alloc_owned(ir, world, Value::Opaque(id))
        }
        crate::ir::ValueEffectOp::NewArrayFromListFlat => {
            // newArrayFromListFlat bounds rangeSize xs: allocate a cell seeded
            // from a list — the lenient `newListArray`, GHC-faithful. A list
            // SHORTER than `rangeSize` pads the tail with the ONE shared lazy
            // hole (the same fault-on-force object as `array`'s unlisted
            // positions); a LONGER list is silently truncated. The element
            // buffer holds BORROWED refs (no RC change) and the claim loop
            // below claims every slot exactly once, so padding pushes the RAW
            // hole ref and the builder's own claim (`rc 1` from `alloc_owned`)
            // is released only AFTER that loop — never before, when a freed
            // hole slot would make the loop's `copy` a dangling ref. The
            // operands arrive as ARENA refs (this is an effect, not a
            // register-indexed value builtin), so the element list is read
            // through its ref — the same convention as `ReadArrayFlat`.
            let bounds_src = *args.first()?;
            let Some(mut elems) = pooled_elems_of_ref(ir, world, *args.get(2)?) else {
                latch_fault(ir, world, "newListArray: expected a list of elements");
                return None;
            };
            let expected = match forced_i64(ir, world, *args.get(1)?, drops) {
                // A negative computed rangeSize means an empty domain (prelude
                // `Ix` returns 0 for inverted bounds; user instances could
                // return less — clamp defensively, mirroring GHC's
                // `safeRangeSize`).
                Some(n) => n.max(0),
                None => {
                    world.arena.put_buf(elems);
                    latch_fault(
                        ir,
                        world,
                        "newListArray: rangeSize is not an integral number",
                    );
                    return None;
                }
            };
            let mut pad_hole: Option<ArenaRef> = None;
            if elems.len() as i64 > expected {
                // Extras are silently dropped. They were BORROWED refs (owned by
                // the source list), so a plain truncate suffices — no per-ref
                // drop, no RC bookkeeping.
                elems.truncate(expected as usize);
            } else if (elems.len() as i64) < expected {
                // Ensure the buffer can hold the padded length WITHOUT
                // reallocating on the eval path: re-take a pooled buffer sized
                // for `expected`, copy the borrowed refs across (no RC), return
                // the old buffer.
                if elems.capacity() < expected as usize {
                    match world.arena.take_buf(expected as usize) {
                        Ok(mut bigger) => {
                            bigger.extend_from_slice(&elems);
                            world.arena.put_buf(elems);
                            elems = bigger;
                        }
                        Err(_) => {
                            world.arena.put_buf(elems);
                            latch_exhaustion(ir, world, "value buffer pool");
                            return None;
                        }
                    }
                }
                // Build the ONE shared hole exactly like `ArrayFromAssocs`: a
                // callable over the synthetic hole fragment with an empty env
                // and a memo cell (see that arm for the construction shape).
                let Some(frag) = ir.array_hole_frag else {
                    world.arena.put_buf(elems);
                    latch_fault(ir, world, "newListArray: no hole fragment");
                    return None;
                };
                let Some(cell) = alloc_owned(ir, world, Value::ThunkCell(ThunkState::Unforced))
                else {
                    world.arena.put_buf(elems);
                    return None;
                };
                let Some(env) = alloc_owned(ir, world, Value::Record(vec![])) else {
                    world.arena.drop_ref(cell);
                    world.arena.put_buf(elems);
                    return None;
                };
                let Some(hole) = alloc_owned(
                    ir,
                    world,
                    Value::Callable {
                        fragment: frag,
                        env,
                        cell: Some(cell),
                    },
                ) else {
                    world.arena.drop_ref(env);
                    world.arena.drop_ref(cell);
                    world.arena.put_buf(elems);
                    return None;
                };
                // Push the RAW hole ref (NO `copy`): the claim loop below gives
                // each padded slot exactly one claim; the builder's own claim
                // keeps the target alive until the loop has run.
                while elems.len() < expected as usize {
                    elems.push(hole);
                }
                pad_hole = Some(hole);
            }
            let bounds = copy_owned(ir, world, bounds_src)?;
            // The cell owns its elements independently of the source list.
            for e in &elems {
                _ = world.arena.copy(*e);
            }
            if let Some(hole) = pad_hole {
                // Release the builder's claim AFTER the claim loop: the slot now
                // has one claim per padded copy, which the cell's drop balances.
                world.arena.drop_ref(hole);
            }
            let id = world.st_cells.len() as u32;
            world.st_cells.push(Some(StCell::Array { bounds, elems }));
            let region = world.regions.get_mut(region_id)?;
            region.cells.push(id);
            alloc_owned(ir, world, Value::Opaque(id))
        }
        crate::ir::ValueEffectOp::ReadArrayFlat => {
            let h = handle(ir, world, drops)?;
            let Some(offset) = forced_i64(ir, world, *args.get(1)?, drops) else {
                latch_fault(
                    ir,
                    world,
                    "readArrayFlat: flat offset is not an integral number",
                );
                return None;
            };
            let len = st_array_len(ir, world, h)?;
            let Some(pos) = flat_index_in_range(offset, len) else {
                latch_fault(
                    ir,
                    world,
                    &format!("readArrayFlat: index {offset} out of range"),
                );
                return None;
            };
            let elem = st_array_elem(ir, world, h, pos)?;
            copy_owned(ir, world, elem)
        }
        crate::ir::ValueEffectOp::WriteArrayFlat => {
            let h = handle(ir, world, drops)?;
            let Some(offset) = forced_i64(ir, world, *args.get(1)?, drops) else {
                latch_fault(
                    ir,
                    world,
                    "writeArrayFlat: flat offset is not an integral number",
                );
                return None;
            };
            let src = *args.get(2)?;
            let len = st_array_len(ir, world, h)?;
            let Some(pos) = flat_index_in_range(offset, len) else {
                latch_fault(
                    ir,
                    world,
                    &format!("writeArrayFlat: index {offset} out of range"),
                );
                return None;
            };
            // The cell owns its own ref; the operand's owner keeps its.
            let stored = copy_owned(ir, world, src)?;
            st_array_elem_store(ir, world, h, pos, stored);
            alloc_owned(ir, world, Value::Void)
        }
        crate::ir::ValueEffectOp::ModifyArrayFlat => {
            let h = handle(ir, world, drops)?;
            let Some(offset) = forced_i64(ir, world, *args.get(1)?, drops) else {
                latch_fault(
                    ir,
                    world,
                    "modifyArrayFlat: flat offset is not an integral number",
                );
                return None;
            };
            let f = *args.get(2)?;
            let len = st_array_len(ir, world, h)?;
            let Some(pos) = flat_index_in_range(offset, len) else {
                latch_fault(
                    ir,
                    world,
                    &format!("modifyArrayFlat: index {offset} out of range"),
                );
                return None;
            };
            // The stored element may be an unforced thunk: the modifier function
            // receives its WHNF form.
            let elem = st_array_elem(ir, world, h, pos)?;
            let old = force(ir, world, elem, drops);
            // Apply to a copy: the cell keeps its own claim while the call runs.
            let arg = copy_owned(ir, world, old)?;
            let new_value = apply_callable(ir, world, f, arg, drops);
            world.arena.drop_ref(arg);
            let new_value = new_value?;
            let stored = copy_owned(ir, world, new_value);
            world.arena.drop_ref(new_value);
            let stored = stored?;
            st_array_elem_store(ir, world, h, pos, stored);
            alloc_owned(ir, world, Value::Void)
        }
        crate::ir::ValueEffectOp::GetArrayBounds => {
            let h = handle(ir, world, drops)?;
            let bounds = st_array_bounds(ir, world, h)?;
            copy_owned(ir, world, bounds)
        }
        crate::ir::ValueEffectOp::GetArrayElems => {
            let h = handle(ir, world, drops)?;
            let elems = st_array_elems_of(ir, world, h)?;
            // Recount every element: the new list owns them independently of the
            // cell (the cell keeps its own claims). `copy` shares the same ref
            // with a fresh claim.
            for e in &elems {
                _ = world.arena.copy(*e);
            }
            alloc_owned(ir, world, Value::List { elems })
        }
        // `RunST`/`RunSTArray` are emitted directly as instructions, never as
        // effect closures: they consume the whole `ST` value.
        crate::ir::ValueEffectOp::RunST | crate::ir::ValueEffectOp::RunSTArray => None,
        // ---- io-streams effects (the World token threads them) ----
        crate::ir::ValueEffectOp::StreamFromList => exec_stream_from_list(ir, world, &args, drops),
        crate::ir::ValueEffectOp::StreamToList => {
            exec_stream_to_list(ir, world, &args, token, drops)
        }
        crate::ir::ValueEffectOp::StreamWrite => exec_stream_write(ir, world, &args, token, drops),
        crate::ir::ValueEffectOp::StreamRead => exec_stream_read(ir, world, &args, token, drops),
        crate::ir::ValueEffectOp::StreamAtEnd => exec_stream_at_end(ir, world, &args, token, drops),
        crate::ir::ValueEffectOp::StreamUnread => exec_stream_unread(ir, world, &args, drops),
        crate::ir::ValueEffectOp::StreamConnect => {
            exec_stream_connect(ir, world, &args, token, drops)
        }
        crate::ir::ValueEffectOp::StreamMakeInput => {
            exec_stream_make_input(ir, world, &args, drops)
        }
        crate::ir::ValueEffectOp::StreamMakeOutput => {
            exec_stream_make_output(ir, world, &args, drops)
        }
    };
    let result = result?;
    // Thread a FRESH state token alongside the result. The payload identifies
    // the same region / the same realized world, so the successor is identical.
    let Some(next_state) = alloc_owned(ir, world, Value::Opaque(token)) else {
        latch_exhaustion(ir, world, "value arena (effect token)");
        return None;
    };
    let pair = alloc_owned(ir, world, Value::Record(vec![next_state, result]));
    if pair.is_none() {
        latch_exhaustion(ir, world, "value arena (effect result)");
    }
    pair
}

/// Collect a (possibly lazy) list value into the stream table as a memory input
/// stream, returning its opaque handle.
///
/// The list's spine is forced element by element, so an infinite `cons` chain
/// drains until the fixed arena budget is exhausted — that exhaustion is
/// latched as a user-facing capacity error rather than looping forever.
fn exec_stream_from_list<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    args: &[ArenaRef],
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    let src = *args.first()?;
    let mut elems: Vec<ArenaRef> = Vec::new();
    let mut cur = force(ir, world, src, drops);
    loop {
        // Snapshot the spine node before copying its elements: the copy borrows
        // the arena mutably, so the node's refs must outlive that borrow.
        let node = match world.arena.get(cur) {
            Some(Value::List { elems: items }) => Some(Ok(items.clone())),
            Some(Value::Cons(head, tail)) => Some(Err((*head, *tail))),
            _ => None,
        };
        match node {
            Some(Ok(items)) => {
                for e in items {
                    match copy_owned(ir, world, e) {
                        Some(c) => elems.push(c),
                        None => {
                            for r in elems.drain(..) {
                                world.arena.drop_ref(r);
                            }
                            latch_exhaustion(ir, world, "value arena (io stream)");
                            return None;
                        }
                    }
                }
                break;
            }
            Some(Err((head, tail))) => {
                match copy_owned(ir, world, head) {
                    Some(c) => elems.push(c),
                    None => {
                        for r in elems.drain(..) {
                            world.arena.drop_ref(r);
                        }
                        latch_exhaustion(ir, world, "value arena (io stream)");
                        return None;
                    }
                }
                let next = force(ir, world, tail, drops);
                if next == cur {
                    // A self-referential tail: treat it as the end of the list
                    // rather than spinning (the arena bound would eventually
                    // stop it, but this keeps the failure immediate).
                    break;
                }
                cur = next;
            }
            // An empty/unbound stream source reads as an empty stream.
            _ => break,
        }
    }
    let idx = alloc_stream(
        ir,
        world,
        StreamHandle::Input {
            elems,
            pos: 0,
            pushback: Vec::new(),
        },
    )?;
    alloc_owned(ir, world, Value::Opaque(STREAM_HANDLE_TAG | idx as u32))
}

/// Register a stream handle in the World's stream table, returning its index.
///
/// The index is tagged into the high half of the `Value::Opaque` payload space
/// ([`STREAM_HANDLE_TAG`]); the table bound keeps the two halves disjoint. A
/// handle that cannot be registered releases the refs it owns.
fn alloc_stream<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    handle: StreamHandle,
) -> Option<usize> {
    let idx = world.streams.len();
    if idx > (STREAM_HANDLE_TAG - 1) as usize {
        latch_exhaustion(ir, world, "stream table");
        handle.drop_refs(&mut world.arena);
        return None;
    }
    world.streams.push(handle);
    Some(idx)
}

/// Drain an input stream into a `List` value.
///
/// A memory stream (`fromList`) hands over its remaining elements directly and
/// releases the consumed prefix. A cursor/closure-backed stream (`stdin`, a
/// `makeInputStream` source) is drained through the ordinary read path until
/// EOF, exactly like io-streams' `toList`: the source's action runs once per
/// element, and the stream is spent afterwards.
fn exec_stream_to_list<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    args: &[ArenaRef],
    token: u32,
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    /// What the stream table says about draining this handle.
    enum Drain {
        /// The remaining memory-stream elements, already owned by the pull.
        Items(Vec<ArenaRef>),
        /// A cursor/closure-backed input: pull through `stream_read` until EOF.
        Read,
        /// A writing end (or no stream at all).
        Fault,
    }
    // The stream operand may arrive as an unforced thunk (a `do`-bound
    // `xs <- fromList …` is bound lazily): force it before reading its payload.
    let handle_ref = force(ir, world, *args.first()?, drops);
    let h = stream_handle_index(ir, world, handle_ref)?;
    // The consumed prefix is released only after the stream-table borrow ends:
    // the table and the arena both live in the world now, so they cannot be
    // borrowed at the same time.
    let mut stale: Vec<ArenaRef> = Vec::new();
    let drain = match world.streams.get_mut(h) {
        Some(StreamHandle::Input {
            elems,
            pos,
            pushback,
        }) => {
            // Pushed-back elements come first, most recently pushed first.
            let mut rest: Vec<ArenaRef> = pushback.drain(..).rev().collect();
            let tail = elems.split_off(*pos);
            // The consumed prefix is no longer reachable: release it.
            stale.append(elems);
            *pos = 0;
            rest.extend(tail);
            Drain::Items(rest)
        }
        Some(StreamHandle::Source { .. }) | Some(StreamHandle::Stdin { .. }) => Drain::Read,
        _ => Drain::Fault,
    };
    for r in stale {
        world.arena.drop_ref(r);
    }
    let items = match drain {
        Drain::Items(items) => items,
        Drain::Fault => {
            latch_fault(ir, world, "`toList` applied to a non-input stream");
            return None;
        }
        Drain::Read => {
            let mut items: Vec<ArenaRef> = Vec::new();
            loop {
                match stream_read(ir, world, h, token, drops) {
                    ReadOutcome::Elem(r) => items.push(r),
                    ReadOutcome::Eof => break,
                    ReadOutcome::Fault => {
                        // The read path latched the fault; release the prefix.
                        for r in items {
                            world.arena.drop_ref(r);
                        }
                        return None;
                    }
                }
            }
            items
        }
    };
    let n = items.len();
    let mut buf = match world.arena.take_buf(n) {
        Ok(b) => b,
        Err(_) => {
            for r in items {
                world.arena.drop_ref(r);
            }
            latch_exhaustion(ir, world, "value buffer pool (io stream)");
            return None;
        }
    };
    buf.extend(items);
    alloc_owned(ir, world, Value::List { elems: buf })
}

/// Perform `write mb out`: push `Just x` into an output stream, or close it on
/// `Nothing`. The stdio targets append the string to the captured console.
fn exec_stream_write<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    args: &[ArenaRef],
    token: u32,
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    let mb = force(ir, world, *args.first()?, drops);
    // `Maybe a`: constructor 0 is `Just`, constructor 1 is `Nothing`.
    let payload = match world.arena.get(mb) {
        Some(Value::Sum(0, fields)) => Some(fields.first().copied()),
        Some(Value::Sum(1, _)) => None,
        _ => {
            latch_fault(ir, world, "`write` expects a Maybe");
            return None;
        }
    };
    // Read the written item before touching the stream table so an allocation
    // failure cannot leave the table half-updated.
    let item = match payload {
        None => None,
        Some(Some(r)) => {
            let r = force(ir, world, r, drops);
            Some(copy_owned(ir, world, r)?)
        }
        Some(None) => {
            latch_fault(ir, world, "`write` expects a Maybe");
            return None;
        }
    };
    let handle_ref = force(ir, world, *args.get(1)?, drops);
    let h = stream_handle_index(ir, world, handle_ref)?;
    if !stream_write(ir, world, h, item, token, drops) {
        return None;
    }
    alloc_owned(ir, world, Value::Void)
}

/// Perform `read s`: pull the next element (`Just`) or report EOF (`Nothing`).
fn exec_stream_read<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    args: &[ArenaRef],
    token: u32,
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    let handle_ref = force(ir, world, *args.first()?, drops);
    let h = stream_handle_index(ir, world, handle_ref)?;
    match stream_read(ir, world, h, token, drops) {
        ReadOutcome::Elem(r) => alloc_owned(ir, world, Value::Sum(0, vec![r])),
        ReadOutcome::Eof => alloc_owned(ir, world, Value::Sum(1, vec![])),
        ReadOutcome::Fault => None,
    }
}

/// Perform `atEnd s`: report whether the input stream is at EOF, leaving the
/// pending element readable (`read` is followed by a push-back when the stream
/// is a wrapped source; a memory stream is checked by its cursor).
fn exec_stream_at_end<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    args: &[ArenaRef],
    token: u32,
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    /// What the stream table says without running any IO.
    enum Check {
        /// EOF status is known without touching the stream.
        Known(bool),
        /// A wrapped source: a pull is needed to know.
        Source,
        /// The handle names a writing end (or no stream at all).
        Fault,
    }
    let handle_ref = force(ir, world, *args.first()?, drops);
    let h = stream_handle_index(ir, world, handle_ref)?;
    let check = match world.streams.get(h) {
        Some(StreamHandle::Input {
            elems,
            pos,
            pushback,
            ..
        }) => Check::Known(!pushback.is_empty() || *pos >= elems.len()),
        Some(StreamHandle::Source { eof, pushback, .. }) => {
            if !pushback.is_empty() {
                Check::Known(false)
            } else if *eof {
                Check::Known(true)
            } else {
                Check::Source
            }
        }
        Some(StreamHandle::Stdin {
            lines,
            pos,
            pushback,
        }) => Check::Known(!pushback.is_empty() || *pos >= lines.len()),
        _ => Check::Fault,
    };
    let at_end = match check {
        Check::Known(b) => b,
        Check::Fault => {
            latch_fault(ir, world, "`atEnd` applied to a non-input stream");
            return None;
        }
        // io-streams `atEnd` on a closure-backed source reads one element and
        // pushes it back, leaving the stream unchanged.
        Check::Source => match stream_read(ir, world, h, token, drops) {
            ReadOutcome::Elem(r) => {
                if !push_back(ir, world, h, r) {
                    return None;
                }
                false
            }
            ReadOutcome::Eof => true,
            ReadOutcome::Fault => return None,
        },
    };
    alloc_owned(ir, world, Value::Bool(at_end))
}

/// Perform `unread x s`: push a value back onto an input stream so the next
/// `read` returns it.
fn exec_stream_unread<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    args: &[ArenaRef],
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    let value = force(ir, world, *args.first()?, drops);
    let owned = copy_owned(ir, world, value)?;
    let handle_ref = force(ir, world, *args.get(1)?, drops);
    let h = stream_handle_index(ir, world, handle_ref)?;
    if !push_back(ir, world, h, owned) {
        return None;
    }
    alloc_owned(ir, world, Value::Void)
}

/// Perform `connect input output`: pull from the input into the output until the
/// input reaches EOF, then close the output (`write Nothing`). Unequal lengths
/// end cleanly at input EOF.
fn exec_stream_connect<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    args: &[ArenaRef],
    token: u32,
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    let in_ref = force(ir, world, *args.first()?, drops);
    let input = stream_handle_index(ir, world, in_ref)?;
    let out_ref = force(ir, world, *args.get(1)?, drops);
    let output = stream_handle_index(ir, world, out_ref)?;
    loop {
        match stream_read(ir, world, input, token, drops) {
            ReadOutcome::Elem(r) => {
                if !stream_write(ir, world, output, Some(r), token, drops) {
                    return None;
                }
            }
            ReadOutcome::Eof => break,
            ReadOutcome::Fault => return None,
        }
    }
    // io-streams `connect` closes the output when the input ends.
    if !stream_write(ir, world, output, None, token, drops) {
        return None;
    }
    alloc_owned(ir, world, Value::Void)
}

/// Perform `makeInputStream act`: wrap the program's `IO (Maybe a)` action as a
/// memory source stream. The action is captured by the handle and applied to the
/// `World` token on every `read`.
fn exec_stream_make_input<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    args: &[ArenaRef],
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    let _ = drops;
    let action = copy_owned(ir, world, *args.first()?)?;
    let idx = alloc_stream(
        ir,
        world,
        StreamHandle::Source {
            action,
            eof: false,
            pushback: Vec::new(),
        },
    )?;
    alloc_owned(ir, world, Value::Opaque(STREAM_HANDLE_TAG | idx as u32))
}

/// Perform `makeOutputStream sink`: wrap the program's `Maybe a -> IO ()`
/// handler as a memory sink stream. The handler is captured by the handle and
/// applied to every written element (and to the closing `Nothing`).
fn exec_stream_make_output<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    args: &[ArenaRef],
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    let _ = drops;
    let handler = copy_owned(ir, world, *args.first()?)?;
    let idx = alloc_stream(
        ir,
        world,
        StreamHandle::Sink {
            handler,
            closed: false,
        },
    )?;
    alloc_owned(ir, world, Value::Opaque(STREAM_HANDLE_TAG | idx as u32))
}

/// One pull from an input stream.
enum ReadOutcome {
    /// An element, as a fresh owner the caller must release.
    Elem(ArenaRef),
    /// The stream is at EOF.
    Eof,
    /// A latched fault (already recorded as a `ProcessError`).
    Fault,
}

/// Pull one element from the input stream at `idx`, running a wrapped
/// `makeInputStream` action when the stream is a closure-backed source.
fn stream_read<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    idx: usize,
    token: u32,
    drops: &mut Vec<ArenaRef>,
) -> ReadOutcome {
    /// The stream table's verdict for one pull.
    enum Step {
        /// An element the table still owns (the cursor path): the pull copies it.
        Borrowed(ArenaRef),
        /// An element popped from the pushback stack: the pull owns it.
        Owned(ArenaRef),
        /// A console-input line: the pull allocates it as a `String` value.
        Line(String),
        /// A wrapped source: run this `IO (Maybe a)` action.
        Action(ArenaRef),
        Eof,
        Fault,
    }
    let step = match world.streams.get_mut(idx) {
        Some(StreamHandle::Input {
            elems,
            pos,
            pushback,
            ..
        }) => {
            if let Some(r) = pushback.pop() {
                Step::Owned(r)
            } else if let Some(r) = elems.get(*pos).copied() {
                *pos += 1;
                Step::Borrowed(r)
            } else {
                Step::Eof
            }
        }
        Some(StreamHandle::Source {
            action,
            eof,
            pushback,
        }) => {
            if let Some(r) = pushback.pop() {
                Step::Owned(r)
            } else if *eof {
                Step::Eof
            } else {
                Step::Action(*action)
            }
        }
        Some(StreamHandle::Stdin {
            lines,
            pos,
            pushback,
        }) => {
            if let Some(r) = pushback.pop() {
                Step::Owned(r)
            } else if let Some(line) = lines.get(*pos) {
                *pos += 1;
                Step::Line(line.clone())
            } else {
                Step::Eof
            }
        }
        _ => Step::Fault,
    };
    match step {
        // The popped pushback ref already transfers ownership to the caller.
        Step::Owned(r) => ReadOutcome::Elem(r),
        // A console line: the interpreter allocates the `String` element.
        Step::Line(line) => match alloc_owned(ir, world, Value::String(line)) {
            Some(r) => ReadOutcome::Elem(r),
            None => ReadOutcome::Fault,
        },
        Step::Borrowed(r) => match copy_owned(ir, world, r) {
            Some(c) => ReadOutcome::Elem(c),
            None => ReadOutcome::Fault,
        },
        Step::Eof => ReadOutcome::Eof,
        Step::Fault => {
            latch_fault(ir, world, "`read` applied to a non-input stream");
            ReadOutcome::Fault
        }
        Step::Action(action) => {
            let Some(mb) = run_io_action(ir, world, action, token, drops) else {
                return ReadOutcome::Fault;
            };
            let mb = force(ir, world, mb, drops);
            // The action yields `IO (Maybe a)`: `Just` = Sum(0, [payload]),
            // `Nothing` = Sum(1, []).
            enum Shape {
                Just(Option<ArenaRef>),
                Nothing,
                Invalid,
            }
            let shape = match world.arena.get(mb) {
                Some(Value::Sum(0, fields)) => Shape::Just(fields.first().copied()),
                Some(Value::Sum(1, _)) => Shape::Nothing,
                _ => Shape::Invalid,
            };
            match shape {
                Shape::Just(Some(r)) => {
                    let r = force(ir, world, r, drops);
                    let out = copy_owned(ir, world, r);
                    world.arena.drop_ref(mb);
                    match out {
                        Some(r) => ReadOutcome::Elem(r),
                        None => ReadOutcome::Fault,
                    }
                }
                Shape::Nothing => {
                    world.arena.drop_ref(mb);
                    if let Some(StreamHandle::Source { eof, .. }) = world.streams.get_mut(idx) {
                        *eof = true;
                    }
                    ReadOutcome::Eof
                }
                Shape::Just(None) | Shape::Invalid => {
                    world.arena.drop_ref(mb);
                    latch_fault(ir, world, "`makeInputStream` action must yield a Maybe");
                    ReadOutcome::Fault
                }
            }
        }
    }
}

/// Push `value` (an owned ref) back onto the input stream at `idx`. Returns
/// whether the push succeeded; a non-input stream is a latched fault.
fn push_back<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    idx: usize,
    value: ArenaRef,
) -> bool {
    let pushed = match world.streams.get_mut(idx) {
        Some(StreamHandle::Input { pushback, .. })
        | Some(StreamHandle::Source { pushback, .. })
        | Some(StreamHandle::Stdin { pushback, .. }) => {
            pushback.push(value);
            true
        }
        _ => false,
    };
    if !pushed {
        world.arena.drop_ref(value);
        latch_fault(ir, world, "`unread`/`atEnd` applied to a non-input stream");
        return false;
    }
    true
}

/// Push `item` (an owned ref, or `None` for the closing `Nothing`) into the
/// output stream at `idx`. Returns false on a latched fault.
fn stream_write<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    idx: usize,
    item: Option<ArenaRef>,
    token: u32,
    drops: &mut Vec<ArenaRef>,
) -> bool {
    /// Where a written element goes.
    enum Target {
        Stdout,
        Stderr,
        Sink { handler: ArenaRef, closed: bool },
        Input,
        Invalid,
    }
    let target = match world.streams.get(idx) {
        Some(StreamHandle::Stdout) => Target::Stdout,
        Some(StreamHandle::Stderr) => Target::Stderr,
        Some(StreamHandle::Sink { handler, closed }) => Target::Sink {
            handler: *handler,
            closed: *closed,
        },
        Some(StreamHandle::Input { .. }) | Some(StreamHandle::Source { .. }) => Target::Input,
        Some(StreamHandle::Stdin { .. }) => Target::Input,
        None => Target::Invalid,
    };
    match target {
        Target::Stdout => write_console(ir, world, item, false),
        Target::Stderr => write_console(ir, world, item, true),
        Target::Sink { handler, closed } => {
            // A closed sink ignores writes (io-streams close semantics): an
            // element is dropped and a repeated `Nothing` is inert.
            let closing = item.is_none();
            if closed {
                if let Some(r) = item {
                    world.arena.drop_ref(r);
                }
                return true;
            }
            // The handler receives an owned `Maybe a` argument.
            let arg = match item {
                Some(r) => alloc_owned(ir, world, Value::Sum(0, vec![r])),
                None => alloc_owned(ir, world, Value::Sum(1, vec![])),
            };
            let Some(arg) = arg else {
                return false;
            };
            let action = apply_callable(ir, world, handler, arg, drops);
            world.arena.drop_ref(arg);
            let Some(action) = action else {
                latch_fault(ir, world, "`makeOutputStream` handler is not callable");
                return false;
            };
            // The handler's `IO ()` payload is a fresh owner: release it, or a
            // sink write sequence leaks one arena slot per element and can
            // exhaust the (statically sized) arena.
            let out = run_io_action(ir, world, action, token, drops);
            world.arena.drop_ref(action);
            let ran = out.is_some();
            if let Some(r) = out {
                world.arena.drop_ref(r);
            }
            if !ran {
                return false;
            }
            // The closing `Nothing` (`item == None`) closes the sink; ordinary
            // elements leave it open.
            if closing {
                if let Some(StreamHandle::Sink { closed, .. }) = world.streams.get_mut(idx) {
                    *closed = true;
                }
            }
            true
        }
        Target::Input => {
            if let Some(r) = item {
                world.arena.drop_ref(r);
            }
            latch_fault(ir, world, "`write` applied to an input stream");
            false
        }
        Target::Invalid => {
            if let Some(r) = item {
                world.arena.drop_ref(r);
            }
            latch_fault(ir, world, "invalid stream handle");
            false
        }
    }
}

/// Append a `String` element to the captured console.
///
/// The console streams are output handles of the runner's `realWorld`, not
/// program-owned streams: writing `Nothing` to one is accepted as the (inert)
/// io-streams close signal but never closes the process console, so later
/// writes still appear. Program-owned sinks (`makeOutputStream`) do close.
fn write_console<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    item: Option<ArenaRef>,
    stderr: bool,
) -> bool {
    let Some(r) = item else {
        return true;
    };
    let text = match world.arena.get(r) {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    };
    world.arena.drop_ref(r);
    let Some(text) = text else {
        latch_fault(ir, world, "stdio stream expects a String element");
        return false;
    };
    if stderr {
        world.stderr.push(text);
    } else {
        world.stdout.push(text);
    }
    true
}

/// Run a `make*Stream`-captured `IO a` action against the `World` token and
/// return its payload as a fresh owner.
fn run_io_action<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    action: ArenaRef,
    token: u32,
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    // The action may be a lazy thunk (`let`-bound); force it to WHNF (the IO
    // record) before projecting `unIO`.
    let action = force(ir, world, action, drops);
    let un_io = match world.arena.get(action) {
        Some(Value::Record(fields)) => fields.first().copied(),
        _ => None,
    };
    let Some(un_io) = un_io else {
        latch_fault(ir, world, "`make*Stream` operand is not an IO action");
        return None;
    };
    // `IO a = { unIO: World -> Pair World a }`: apply to a fresh World token.
    let token_ref = alloc_owned(ir, world, Value::Opaque(token))?;
    let pair = apply_callable(ir, world, un_io, token_ref, drops);
    world.arena.drop_ref(token_ref);
    let Some(pair) = pair else {
        latch_fault(ir, world, "`make*Stream` action is not callable");
        return None;
    };
    let pair = force_owned(ir, world, pair, drops)?;
    let payload = match world.arena.get(pair) {
        Some(Value::Record(fields)) => fields.get(1).copied(),
        _ => None,
    };
    let out = payload.and_then(|p| {
        let p = force(ir, world, p, drops);
        copy_owned(ir, world, p)
    });
    world.arena.drop_ref(pair);
    if payload.is_none() {
        latch_fault(ir, world, "`make*Stream` action produced no result");
    }
    out
}

/// Resolve an opaque stream-handle value to its stream-table index.
fn stream_handle_index<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    value: ArenaRef,
) -> Option<usize> {
    match world.arena.get(value) {
        Some(Value::Opaque(payload)) if *payload >= STREAM_HANDLE_TAG => {
            Some((payload & !STREAM_HANDLE_TAG) as usize)
        }
        _ => {
            latch_fault(ir, world, "expected a stream handle");
            None
        }
    }
}

/// Run one `runST`/`runSTArray`: enter a fresh region, apply the `ST` value's
/// `unST` closure to the region token, discard the final state, and release
/// every cell the region allocated.
///
/// With `freeze_array` (`runSTArray`), the region's result must be an `STArray`
/// handle; the cell is frozen into an ordinary `Array i e` by a refcount
/// TRANSFER: one fresh claim per element and on the bounds record goes to the
/// frozen array, and the region's own cell claims are released by
/// [`finish_region`] — net refcount unchanged, no element copies. Without it
/// (`runST`) the region result is returned as a fresh owner.
fn exec_runst<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    st_value: crate::arena::ArenaRef,
    drops: &mut Vec<crate::arena::ArenaRef>,
    freeze_array: bool,
) -> Option<crate::arena::ArenaRef> {
    // The argument may arrive as a THUNK, not an eager record: lowering
    // re-inlines typeclass instance bodies, so a `let`-bound `ST` value lowers
    // to a `ValueLazy` thunk whose register is handed straight to `runST`.
    // Force it to WHNF before projecting `unST`. (`force` memoizes in place and
    // the caller keeps ownership of `st_value`, so no drop happens here.)
    let st_ref = force(ir, world, st_value, drops);
    // `ST s a` is `{ unST: STState -> Pair STState a }`: field 0 is the region
    // computation.
    let un_st = match world.arena.get(st_ref) {
        Some(Value::Record(fields)) => fields.first().copied(),
        _ => None,
    };
    let region_id = world.regions.len() as u32;
    let cell_base = world.st_cells.len();
    world.regions.push(crate::program::RegionState {
        cells: Vec::new(),
        cell_base,
    });
    let result = match un_st {
        Some(un_st) => {
            // Allocate the region token only once there is computation to run;
            // the `None` arm below then has nothing to release.
            let Some(state) = alloc_owned(ir, world, Value::Opaque(region_id)) else {
                finish_region(ir, world);
                return None;
            };
            // `unST` is a complete-mode computation: the applied result may be
            // a suspended call, so force it to WHNF before reading the Pair.
            let pair = apply_callable(ir, world, un_st, state, drops)
                .and_then(|p| force_owned(ir, world, p, drops));
            // The effect closure only borrowed the token.
            world.arena.drop_ref(state);
            // The region result is the pair's `second` field; the final state
            // (`first`) is discarded with the pair.
            match pair {
                Some(pair) => {
                    // Force the region result: leaving the region is a strict
                    // boundary, so the payload must be materialized while its
                    // cells are still alive.
                    let forced = match world.arena.get(pair) {
                        Some(Value::Record(fields)) => fields.get(1).copied(),
                        _ => None,
                    };
                    let forced = forced.map(|r| force(ir, world, r, drops));
                    let out = if freeze_array {
                        forced.and_then(|r| freeze_starray(ir, world, r))
                    } else {
                        forced.and_then(|r| copy_owned(ir, world, r))
                    };
                    world.arena.drop_ref(pair);
                    out
                }
                None => None,
            }
        }
        None => None,
    };
    finish_region(ir, world);
    result
}

/// Freeze an `STArray` region result into an immutable `Array i e`.
///
/// `payload` is the region result (the opaque array-cell handle). The cell's
/// bounds ref and every element ref receive ONE fresh claim for the frozen
/// array; the cell's own claims are released when the region unwinds
/// ([`finish_region`]), so the transfer is refcount-only — no element is
/// copied and no value slot is allocated for one.
fn freeze_starray<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    payload: ArenaRef,
) -> Option<ArenaRef> {
    let cell_id = match world.arena.get(payload) {
        Some(Value::Opaque(id)) => *id as usize,
        _ => {
            latch_fault(ir, world, "runSTArray: region result is not an STArray");
            return None;
        }
    };
    let bounds = st_array_bounds(ir, world, cell_id)?;
    let elems = st_array_elems_of(ir, world, cell_id)?;
    let Some(bounds) = copy_owned(ir, world, bounds) else {
        world.arena.put_buf(elems);
        return None;
    };
    // Refcount transfer: one fresh claim per element ref for the frozen array.
    // `copy` shares the same ref with a fresh claim (no element is copied).
    for e in &elems {
        _ = world.arena.copy(*e);
    }
    alloc_owned(ir, world, Value::Array { bounds, elems })
}

/// Leave the innermost `ST` region: release every cell it allocated and
/// truncate the cell table back to the region's mark. Regions nest LIFO, so no
/// outer cell is lost. The caller supplies the region's result; this only
/// unwinds.
fn finish_region<T: Transcendental, const BUF: usize>(ir: &Ir, world: &mut WorldState<T, BUF>) {
    if let Some(region) = world.regions.pop() {
        for id in region.cells {
            if let Some(slot) = world.st_cells.get_mut(id as usize) {
                if let Some(cell) = slot.take() {
                    release_st_cell(ir, world, cell);
                }
            }
        }
        world.st_cells.truncate(region.cell_base);
    }
}

/// Execute a fragment body for a callable value: binds the captured env fields
/// as cells in a temporary frame, runs the fragment's value instructions
/// against a scratch register slice, and returns the copied result ref.
///
/// Register-offset scheme: a fragment's instructions are reused across calls
/// and reference fragment-local registers `0..num_value_regs`. The interpreter
/// records `base = value_regs_top`, runs each instruction with every register
/// field offset by `base` (threaded into [`exec_value_instr`]), and unwinds
/// the watermark — the store is pre-sized to `num_value_regs + max_call_regs`
/// at construction, so no growth or reallocation happens on the RT path. Value
/// args are copied (RC++) into the fragment's leading registers; the caller
/// keeps its own ownership.
///
/// Capture scheme: the env Record's fields (the lambda's free variables, in
/// declaration order) become cells in the shared `frag_cells` store at the
/// current top; `frag_cells_base` marks the active frame so the fragment's
/// `ValueReadCell { cell: i }` resolves to `frag_cells[frag_cells_base + i]`.
/// A capture cell holds an INDEPENDENT copy of the field's value, so the cell
/// owns it and the env record is untouched. Deferred drops queue on the
/// pre-sized `drops` scratch threaded from the value track — every nesting
/// depth reuses the same reserved buffer.
///
/// `args` selects the arg source: caller registers (an eager closure call) or
/// the env-record tail (a lazy thunk). `dst`, when `Some`, receives the copied
/// result ref (`None` when the fragment declares no output register); `None`
/// when the caller wants the result purely via the return (the lazy forcing
/// path memoizes the returned ref).
///
/// Every call unwinds its scratch and cells on return — there is no
/// `no_unwind` path. The top-level entry is NOT a call: [`exec_entry`] walks the
/// program's own value blocks directly, so the flag has no site here.
fn run_call<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    frag: &std::sync::Arc<FragmentIr>,
    env_ref: crate::arena::ArenaRef,
    args: ArgsSource<'_>,
    dst: Option<usize>,
    drops: &mut Vec<crate::arena::ArenaRef>,
) -> Option<crate::arena::ArenaRef> {
    // 0. Reserve the call-scratch slice BEFORE binding capture cells, so a
    //    recursion depth beyond `max_call_regs` latches a ProcessError instead
    //    of a release-mode Vec OOB (deep-but-finite recursion can legitimately
    //    exceed the acyclic bound). The watermark grows into the reserved tail.
    let base = world.value_regs_top;
    let top = base + frag.num_value_regs;
    if top > world.value_regs.len() {
        latch_exhaustion(ir, world, "call scratch (max_call_regs)");
        return None;
    }
    let value_ins = frag.sig.value_ins;
    // 1. Bind the env Record fields as capture cells. For an eager call every
    //    field is a capture (the args come from the caller's registers); for a
    //    lazy call the FIRST `fields.len() - value_ins` fields are the lambda's
    //    free variables and the LAST `value_ins` are the function's arguments.
    let mut arg_refs: Vec<ArenaRef> = Vec::new();
    let cell_base = world.frag_cells.len();
    if let Some(Value::Record(fields)) = world.arena.get(env_ref) {
        // Clone the field refs so the arena can be borrowed mutably while the
        // capture cells are allocated below.
        let fields = fields.clone();
        let n_capture = match args {
            ArgsSource::Registers { .. } => fields.len(),
            ArgsSource::EnvTail => fields.len().saturating_sub(value_ins),
        };
        for (i, f) in fields.iter().enumerate() {
            if i < n_capture {
                // Clone the field value so the arena borrow ends before the
                // mutable alloc_copy below (mirrors the outer `fields.clone()`).
                let cell = match world.arena.get(*f).cloned() {
                    Some(v) => alloc_copy(ir, world, &v),
                    None => world.arena.alloc(Value::Void).ok(),
                };
                if let Some(c) = cell {
                    world.frag_cells.push(c);
                }
            } else {
                arg_refs.push(*f);
            }
        }
    }
    let saved_cells_base = world.frag_cells_base;
    world.frag_cells_base = cell_base;
    // 2. The pre-sized store guarantees the reserved slice is available here.
    world.value_regs_top = top;
    // 3. Value args: bind the fragment's leading registers. Each copy is a
    //    fresh owner (RC++) so the caller (or the thunk's env) keeps its own
    //    ref.
    match args {
        ArgsSource::Registers {
            regs,
            base: caller_base,
        } => {
            for (i, a) in regs.iter().take(value_ins.min(regs.len())).enumerate() {
                if let Some(sr) = world.value_regs.get(caller_base + *a).copied().flatten() {
                    world.value_regs[base + i] = copy_owned(ir, world, sr);
                }
            }
        }
        ArgsSource::EnvTail => {
            for (i, a) in arg_refs.iter().take(value_ins).enumerate() {
                if let Some(c) = copy_owned(ir, world, *a) {
                    world.value_regs[base + i] = Some(c);
                }
            }
        }
    }
    // 4. Run the fragment's value blocks with the register offset. Drops queue
    //    on the threaded scratch above this call's mark and are drained below,
    //    so a nested dispatch's drops never collide.
    let drops_mark = drops.len();
    run_blocks(ir, world, &frag.value_blocks, frag.entry, base, drops);
    for r in drops.drain(drops_mark..) {
        world.arena.drop_ref(r);
    }
    // 5. Copy the fragment's result out (a fresh owner via `copy`) and write it
    //    into `dst` when one is given and the fragment declares an output.
    let result: Option<ArenaRef> = match frag.output_value_regs.first() {
        Some(or) => match world.value_regs.get(base + *or).copied().flatten() {
            Some(sr) => copy_owned(ir, world, sr),
            None => None,
        },
        None => None,
    };
    if let (Some(d), true) = (dst, !frag.output_value_regs.is_empty()) {
        world.value_regs[d] = result;
    }
    // 6. Drain the scratch slice (dropping every fragment-local register's
    //    counted ref — value args and body temps own arena slots, so removing
    //    the slots without `drop_ref` would leak them on EVERY call and exhaust
    //    the fixed arena across runs), unwind the watermark, release the
    //    capture cells, and restore the caller's frame. There is no
    //    `no_unwind` path — the top-level entry is walked directly by
    //    `exec_entry` (not via `run_call`), so every call here unwinds fully.
    for i in base..world.value_regs_top {
        if let Some(r) = world.value_regs[i].take() {
            world.arena.drop_ref(r);
        }
    }
    world.value_regs_top = base;
    world.frag_cells_base = saved_cells_base;
    for c in world.frag_cells.drain(cell_base..) {
        world.arena.drop_ref(c);
    }
    result
}

/// Force `r` to WHNF and return the head ref. Idempotent via the memo cell.
///
/// The returned ref is a BORROWED view — the memo cell (or the caller's own
/// operand) owns it. A caller that stores it must `copy` it first; a caller
/// that only reads it must not drop it. `drops` is the shared deferred-drop
/// scratch threaded from the value track: forcing runs the body, whose
/// instructions may queue drops.
///
/// Loop semantics (call-by-need): a value that is not a lazy callable is
/// already WHNF. A lazy callable (`Value::Callable { cell: Some(c), .. }`)
/// inspects its memo cell:
/// - `Forced(res)`: memo hit — chase `res` (a result may itself be a thunk).
/// - `Unforced`: mark the cell `Forcing` (the black-hole marker), run the body
///   fragment with the captured env, memoize the result as `Forced(result)`.
///   A body that RE-ENTERS this cell while it is `Forcing` has hit a black
///   hole: the cell now memoizes a bottom slot and the callable is divergent.
/// - `Forcing`: re-entry — the black hole; the cell memoizes a bottom slot.
/// - `Bottom`: a previously-divergent cell; memoize a bottom slot.
///
/// A divergent cell is memoized as `Forced(bottom_slot)` (rather than the bare
/// `Bottom` state) so the bottom is borrowed like every other forced result,
/// the SAME bottom ref is returned on every force, and no slot leaks.
pub(crate) fn force<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    mut r: ArenaRef,
    drops: &mut Vec<ArenaRef>,
) -> ArenaRef {
    // Cells memoized into a `Forced` result during this chase. Re-entering one
    // is a self/mutual-recursion cycle (`a = a`, `a = b; b = a`), which is
    // bottom, not a hang.
    let mut seen_cells: Vec<ArenaRef> = Vec::new();
    loop {
        let v = match world.arena.get(r) {
            Some(v) => v,
            None => return r, // dangling: caller's bug; treat as-is
        };
        let Value::Callable {
            fragment,
            env,
            cell: Some(cell),
        } = v
        else {
            return r; // already WHNF (eager callable, or a non-callable value)
        };
        let (fragment, env, cell) = (*fragment, *env, *cell);
        let state: Option<Value> = world.arena.get(cell).cloned();
        match state {
            Some(Value::ThunkCell(ThunkState::Forced(res))) => {
                // Cycle guard: a memoized result that re-enters a cell already
                // forced in this chase is a self-referential loop.
                if let Some(Value::Callable { cell: Some(c2), .. }) = world.arena.get(res) {
                    if seen_cells.contains(c2) {
                        return memoize_bottom(ir, world, cell);
                    }
                }
                r = res;
                continue;
            }
            Some(Value::ThunkCell(ThunkState::Bottom)) => {
                // A previously-divergent cell: memoize a bottom slot and
                // return it (borrowed, cell-owned).
                return memoize_bottom(ir, world, cell);
            }
            Some(Value::ThunkCell(ThunkState::Unforced)) => {
                // Mark the cell Forcing (black hole) before dispatching.
                let _ = world.arena.mutate_slot(cell, |v| {
                    if let Value::ThunkCell(st) = v {
                        *st = ThunkState::Forcing;
                    }
                });
                let result: Option<ArenaRef> = match ir.fragments.get(fragment as usize).cloned() {
                    Some(frag) => run_call(ir, world, &frag, env, ArgsSource::EnvTail, None, drops),
                    None => {
                        debug_assert!(false, "thunk body fragment {fragment} does not exist");
                        None
                    }
                };
                let result = match result {
                    Some(r) => r,
                    None => {
                        // The body produced no result because a deeper recursion
                        // already exhausted the arena/pool (latched above) or the
                        // body legitimately yielded nothing — either way this
                        // thunk cannot compute, so it is bottom (a latched
                        // ProcessError surfaces at tick end).
                        latch_exhaustion(ir, world, "thunk body produced no result");
                        try_alloc_bottom(ir, world).unwrap_or(env)
                    }
                };
                // Re-entrancy guard: a body that forced ITSELF left a black-
                // hole bottom behind while its cell was Forcing. Such a thunk
                // is divergent — keep the bottom, discard the body result.
                match world.arena.get(cell).cloned() {
                    Some(Value::ThunkCell(ThunkState::Forcing)) => {
                        // Normal completion — unless the body produced a thunk
                        // that re-enters THIS cell (a self-cycle).
                        let cyclic = matches!(
                            world.arena.get(result),
                            Some(Value::Callable { cell: Some(c2), .. })
                                if *c2 == cell || seen_cells.contains(c2)
                        );
                        if cyclic {
                            world.arena.drop_ref(result);
                            return memoize_bottom(ir, world, cell);
                        }
                        seen_cells.push(cell);
                        let _ = world.arena.mutate_slot(cell, |v| {
                            if let Value::ThunkCell(st) = v {
                                *st = ThunkState::Forced(result);
                            }
                        });
                        r = result;
                        continue;
                    }
                    Some(Value::ThunkCell(ThunkState::Forced(b))) => {
                        world.arena.drop_ref(result);
                        r = b;
                        continue;
                    }
                    Some(Value::ThunkCell(ThunkState::Bottom)) => {
                        world.arena.drop_ref(result);
                        return memoize_bottom(ir, world, cell);
                    }
                    _ => {
                        world.arena.drop_ref(result);
                        return r;
                    }
                }
            }
            Some(Value::ThunkCell(ThunkState::Forcing)) => {
                // Black hole re-entry: divergent. Memoize a bottom slot.
                return memoize_bottom(ir, world, cell);
            }
            _ => return r,
        }
    }
}

/// Allocate a bottom slot, memoize it into `cell` as `Forced(bottom)`, and
/// return the bottom ref. The cell owns the bottom (kept alive via
/// `visit_refs`); the returned ref is borrowed by the caller.
fn memoize_bottom<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    cell: ArenaRef,
) -> ArenaRef {
    match try_alloc_bottom(ir, world) {
        Some(b) => {
            let _ = world.arena.mutate_slot(cell, |v| {
                if let Value::ThunkCell(st) = v {
                    *st = ThunkState::Forced(b);
                }
            });
            b
        }
        None => cell, // arena exhausted (build-time budget bug); caller treats as bottom-ish
    }
}

/// Execute a whole-buffer foreign built-in over `Value::Buffer` operands.
///
/// B3-2: the value-track `ValueCallForeign` arm. Reads each `srcs` value
/// register's `Value::Buffer` handle, calls `Algorithm::process`/
/// `MultichannelAlgorithm::process` over the handles' `RegionHandle` slices
/// (no copy at the FFI boundary), and writes the output buffer(s) into
/// `dst..dst+n_out` (+ the `Value::Buffer` markers).
fn exec_value_foreign<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
    first_dst: usize,
    srcs: &[usize],
    instance: usize,
    base: usize,
) {
    let bi = &ir.builtins[instance];
    let n_in = bi.signal_ins;
    let n_out = bi.signal_outs;
    assert!(
        n_in <= MAX_BUILTIN_CHANNELS && n_out <= MAX_BUILTIN_CHANNELS,
        "built-in '{0}' has {n_in}→{n_out} channels, exceeding MAX_BUILTIN_CHANNELS ({MAX_BUILTIN_CHANNELS})",
        bi.name
    );
    let n = world.block_len;
    let src_bufs: Vec<usize> = srcs
        .iter()
        .map(|&r| buffer_index(ir, world, r, base))
        .collect();
    let dst_buf = buffer_index(ir, world, first_dst, base);

    if n_in <= 1 && n_out == 1 {
        assert!(
            n_in == 0 || first_dst != srcs[0],
            "ForeignBlock register aliasing: input reg {} == output reg {}.",
            srcs[0],
            first_dst,
        );
        let mut scratch = [T::ZERO; BUF];
        let maybe_in = if n_in == 0 {
            None
        } else {
            world.buffers[src_bufs[0]].with_data(|s| scratch[..n].copy_from_slice(&s[..n]));
            Some(&scratch[..n])
        };
        match &mut world.builtins[instance] {
            crate::program::BuiltinInst::Block(b) => {
                world.buffers[dst_buf].with_data_mut(|out| {
                    let _ = b.process(maybe_in, &mut out[..n]);
                });
            }
            crate::program::BuiltinInst::MultichannelBlock(_) => {
                unreachable!("ForeignBlock fast path with multichannel builtin")
            }
        }
    } else {
        match &mut world.builtins[instance] {
            crate::program::BuiltinInst::MultichannelBlock(mb) => {
                let mut in_bufs: [FixedBuffer<T, BUF>; MAX_BUILTIN_CHANNELS] =
                    std::array::from_fn(|_| FixedBuffer::new());
                for ch in 0..n_in {
                    world.buffers[src_bufs[ch]].with_data(|s| {
                        in_bufs[ch][..n].copy_from_slice(&s[..n]);
                    });
                }
                let input_refs: [&[T]; MAX_BUILTIN_CHANNELS] =
                    std::array::from_fn(|i| &in_bufs[i][..n]);
                let mut out_bufs: [FixedBuffer<T, BUF>; MAX_BUILTIN_CHANNELS] =
                    std::array::from_fn(|_| FixedBuffer::new());
                let mut empties: [[T; 0]; MAX_BUILTIN_CHANNELS] = std::array::from_fn(|_| []);
                let mut it = out_bufs
                    .iter_mut()
                    .map(|b| &mut b[..n])
                    .chain(empties.iter_mut().map(|d| &mut d[..]));
                let mut out_refs: [&mut [T]; MAX_BUILTIN_CHANNELS] =
                    std::array::from_fn(|_| it.next().unwrap());
                let _ = MultichannelAlgorithm::process(
                    mb.as_mut(),
                    &input_refs[..n_in],
                    &mut out_refs[..n_out],
                );
                for (ch, ob) in out_bufs.iter().take(n_out).enumerate() {
                    world.buffers[dst_buf + ch].with_data_mut(|out| {
                        out[..n].copy_from_slice(&ob[..n]);
                    });
                }
            }
            crate::program::BuiltinInst::Block(b) => {
                let mut inp: [[T; BUF]; MAX_BUILTIN_CHANNELS] =
                    std::array::from_fn(|_| [T::ZERO; BUF]);
                for (ch, _) in srcs.iter().enumerate() {
                    world.buffers[src_bufs[ch]].with_data(|s| {
                        inp[ch][..n].copy_from_slice(&s[..n]);
                    });
                }
                let mut out_buf: [[T; BUF]; MAX_BUILTIN_CHANNELS] =
                    std::array::from_fn(|_| [T::ZERO; BUF]);
                let _ = b.process(
                    Some(&inp.as_flattened()[..n_in * n]),
                    &mut out_buf.as_flattened_mut()[..n_out * n],
                );
                for ch in 0..n_out {
                    world.buffers[dst_buf + ch].with_data_mut(|out| {
                        let start = ch * n;
                        out[..n].copy_from_slice(&out_buf.as_flattened()[start..start + n]);
                    });
                }
            }
        }
    }
    for i in 0..n_out {
        let reg = first_dst + i;
        world.value_regs[reg + base] = alloc_owned(ir, world, Value::Buffer(reg));
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
    use rill_core::math::vector::ops::SlicePair;
    match op {
        BinArith::Add => SlicePair::new(a, b).add_into::<4, ScalarVector4<T>>(out),
        BinArith::Sub => SlicePair::new(a, b).sub_into::<4, ScalarVector4<T>>(out),
        BinArith::Mul => SlicePair::new(a, b).mul_into::<4, ScalarVector4<T>>(out),
        BinArith::Div => SlicePair::new(a, b).div_into::<4, ScalarVector4<T>>(out),
        BinArith::Min => min_slice::<T, 4, ScalarVector4<T>>(a, b, out),
        BinArith::Max => max_slice::<T, 4, ScalarVector4<T>>(a, b, out),
        BinArith::Rem => SlicePair::new(a, b).rem_into::<4, ScalarVector4<T>>(out),
        BinArith::Atan2 => {
            for (o, (&y, &x)) in out.iter_mut().zip(a.iter().zip(b.iter())) {
                *o = y.atan2(x);
            }
        }
    }
}

#[cfg(test)]
mod closure_dispatch_tests {
    use crate::backend::interp::exec_entry;
    use crate::ir::{FragmentIr, FuncSig, Ir, ValueBlock, ValueInstr, ValueLayout, ValueTerm};
    use crate::program::{WorldConfig, WorldState};

    #[test]
    fn call_dispatch_runs_fragment() {
        // env reg 0 holds a Void cell (dummy env); MakeClosure binds it; the
        // fragment computes const int 7; CallFunc copies the result to the
        // output register.
        let ir = Ir {
            main_io_unit: false,
            num_inputs: 0,
            num_outputs: 0,
            state: Default::default(),
            builtins: Vec::new(),
            params: Vec::new(),
            value_blocks: vec![ValueBlock {
                instrs: vec![
                    ValueInstr::ValueBindCell { dst: 0 },
                    ValueInstr::ValueMakeClosure {
                        dst: 1,
                        env: 0,
                        fragment: 0,
                    },
                    ValueInstr::ValueCallFunc {
                        dst: 2,
                        closure_slot: 1,
                        args: vec![],
                    },
                ],
                term: ValueTerm::Halt,
            }],
            value_entry: 0,
            num_value_regs: 3,
            result_reg: 2,
            value_funcs: Vec::new(),
            max_call_regs: 2,
            value_state: ValueLayout {
                capacity: 16,
                buffer_budget: 0,
                value_state_slots: 0,
            },
            num_buffers: 0,

            input_buffers: Vec::new(),

            tapes: Vec::new(),
            fragments: vec![std::sync::Arc::new(FragmentIr {
                value_blocks: vec![ValueBlock {
                    instrs: vec![ValueInstr::ValueConstInt { dst: 0, value: 7 }],
                    term: ValueTerm::Halt,
                }],
                entry: 0,
                num_value_regs: 1,
                output_value_regs: vec![0],
                num_capture_cells: 0,
                sig: FuncSig {
                    value_ins: 0,
                    value_outs: 1,
                    signal_ins: 0,
                },
            })],
            array_hole_frag: None,
        };
        let mut world = WorldState::<f32, 256>::new(WorldConfig::default());
        world.seat(&ir).unwrap();
        exec_entry(&ir, &mut world).unwrap();
        let v = world.result().unwrap();
        assert_eq!(world.arena().get(v).unwrap(), &crate::arena::Value::Int(7));
    }

    #[test]
    fn lazy_call_forces_a_thunk_callee() {
        // A `ValueCallLazy` whose callee register holds a THUNK (a `ValueLazy`
        // binding, as produced by a lowering-inlined `let` or a recursive CAF)
        // must force the callee and apply it. The callee thunk's fragment 0
        // returns an EAGER closure over fragment 1, which echoes its argument;
        // register 3 is that argument (9).
        let ir = Ir {
            main_io_unit: false,
            num_inputs: 0,
            num_outputs: 0,
            state: Default::default(),
            builtins: Vec::new(),
            params: Vec::new(),
            value_blocks: vec![ValueBlock {
                instrs: vec![
                    // Dummy `Void` cell as the thunk's capture environment (the
                    // same shape `emit_env_snapshot` uses when nothing is
                    // captured).
                    ValueInstr::ValueBindCell { dst: 0 },
                    ValueInstr::ValueLazy {
                        dst: 1,
                        env: 0,
                        fragment: 0,
                    },
                    ValueInstr::ValueConstInt { dst: 3, value: 9 },
                    ValueInstr::ValueCallLazy {
                        dst: 2,
                        closure_slot: 1,
                        args: vec![3],
                    },
                ],
                term: ValueTerm::Halt,
            }],
            value_entry: 0,
            num_value_regs: 4,
            result_reg: 2,
            value_funcs: Vec::new(),
            max_call_regs: 2,
            value_state: ValueLayout {
                capacity: 16,
                buffer_budget: 0,
                value_state_slots: 0,
            },
            num_buffers: 0,
            input_buffers: Vec::new(),
            tapes: Vec::new(),
            fragments: vec![
                // Thunk body: return an eager closure over fragment 1.
                std::sync::Arc::new(FragmentIr {
                    value_blocks: vec![ValueBlock {
                        instrs: vec![
                            ValueInstr::ValueBindCell { dst: 0 },
                            ValueInstr::ValueMakeClosure {
                                dst: 1,
                                env: 0,
                                fragment: 1,
                            },
                        ],
                        term: ValueTerm::Halt,
                    }],
                    entry: 0,
                    num_value_regs: 2,
                    output_value_regs: vec![1],
                    num_capture_cells: 0,
                    sig: FuncSig {
                        value_ins: 0,
                        value_outs: 1,
                        signal_ins: 0,
                    },
                }),
                // Applied body: echo the single argument.
                std::sync::Arc::new(FragmentIr {
                    value_blocks: vec![ValueBlock {
                        instrs: vec![],
                        term: ValueTerm::Halt,
                    }],
                    entry: 0,
                    num_value_regs: 1,
                    output_value_regs: vec![0],
                    num_capture_cells: 0,
                    sig: FuncSig {
                        value_ins: 1,
                        value_outs: 1,
                        signal_ins: 0,
                    },
                }),
            ],
            array_hole_frag: None,
        };
        let mut world = WorldState::<f32, 256>::new(WorldConfig::default());
        world.seat(&ir).unwrap();
        exec_entry(&ir, &mut world).unwrap();
        let v = world.result().expect("the lazy call must produce a value");
        assert_eq!(world.arena().get(v).unwrap(), &crate::arena::Value::Int(9));
    }
}

#[cfg(test)]
mod value_track_tests {
    use super::*;
    use crate::ir::{Ir, StateLayout, ValueBlock, ValueInstr, ValueLayout, ValueTerm};
    use crate::program::WorldConfig;

    fn prog_with(
        value_blocks: Vec<ValueBlock>,
        value_entry: usize,
        num_value_regs: usize,
        value_state_slots: usize,
    ) -> (Ir, WorldState<f32, 256>) {
        let ir = Ir {
            main_io_unit: false,
            num_inputs: 0,
            num_outputs: 0,
            state: StateLayout::default(),
            builtins: Vec::new(),
            params: Vec::new(),
            value_blocks,
            value_entry,
            num_value_regs,
            result_reg: 0,
            value_funcs: Vec::new(),
            fragments: Vec::new(),
            array_hole_frag: None,
            max_call_regs: 0,
            value_state: ValueLayout {
                capacity: 16,
                buffer_budget: 64,
                value_state_slots,
            },
            num_buffers: 0,

            input_buffers: Vec::new(),

            tapes: Vec::new(),
        };
        let mut world = WorldState::<f32, 256>::new(WorldConfig::default());
        world.seat(&ir).unwrap();
        (ir, world)
    }

    #[test]
    fn const_int_and_const_float_run_per_tick() {
        let (ir, mut world) = prog_with(
            vec![ValueBlock {
                instrs: vec![
                    ValueInstr::ValueConstInt { dst: 0, value: 42 },
                    ValueInstr::ValueConstFloat { dst: 1, value: 1.5 },
                ],
                term: ValueTerm::Halt,
            }],
            0,
            2,
            0,
        );
        // Drive the value track directly (the full tick additionally clears
        // the per-tick registers at the end) so the registers and arena can be
        // inspected mid-tick.
        run_main_blocks_for_tests(&ir, &mut world).unwrap();
        assert_eq!(world.value_regs[0], Some(0));
        let slot0 = world.arena.get(0).unwrap().clone();
        assert_eq!(slot0, crate::arena::Value::Int(42));
        let slot1 = world.arena.get(1).unwrap().clone();
        assert_eq!(slot1, crate::arena::Value::Float(1.5));
    }

    #[test]
    fn cell_stack_bind_read_write() {
        let (ir, mut world) = prog_with(
            vec![ValueBlock {
                instrs: vec![
                    ValueInstr::ValuePushScope,
                    ValueInstr::ValueConstInt { dst: 0, value: 7 },
                    ValueInstr::ValueBindCell { dst: 1 },
                    ValueInstr::ValueWriteCell { cell: 1, src: 0 },
                    ValueInstr::ValueReadCell { dst: 2, cell: 1 },
                    ValueInstr::ValuePopScope,
                ],
                term: ValueTerm::Halt,
            }],
            0,
            3,
            0,
        );
        run_main_blocks_for_tests(&ir, &mut world).unwrap();
        let cell = world.value_regs[1].unwrap();
        let val = world.arena.get(cell).unwrap();
        assert_eq!(val, &crate::arena::Value::Int(7));
        let read = world.value_regs[2].unwrap();
        assert_eq!(world.arena.get(read).unwrap(), &crate::arena::Value::Int(7));
    }

    #[test]
    fn effect_closure_with_unbound_operand_is_a_latched_fault() {
        // Neither operand register is bound. The effect closure must NOT be
        // built from the remaining operands: `exec_effect` reads the
        // captured fields positionally, so a shortened record would turn the
        // stored value into the cell handle. The op yields no value and latches
        // a (user-facing) fault instead.
        let (ir, mut world) = prog_with(
            vec![ValueBlock {
                instrs: vec![ValueInstr::ValueEffectOp {
                    dst: 1,
                    op: crate::ir::ValueEffectOp::WriteStRef,
                    args: vec![0, 0],
                }],
                term: ValueTerm::Halt,
            }],
            0,
            2,
            0,
        );
        let err = run_main_blocks_for_tests(&ir, &mut world)
            .expect_err("an unbound operand must latch a fault");
        assert!(
            format!("{err}").contains("unbound operand"),
            "expected the effect-fault message, got: {err}"
        );
        assert_eq!(world.value_regs[1], None, "the effect must yield no value");
    }

    #[test]
    fn value_state_persists_one_run() {
        // Value-state is a 1-run delayed value: what run 1 writes into slot 0
        // must be readable in run 2. The read runs before the write, so slot 1
        // latches whatever the read saw; asserting it proves the delay.
        let (ir, mut world) = prog_with(
            vec![ValueBlock {
                instrs: vec![
                    ValueInstr::ValueStateRead { dst: 1, slot: 0 },
                    ValueInstr::ValueConstInt { dst: 0, value: 7 },
                    ValueInstr::ValueStateWrite { slot: 0, src: 0 },
                    ValueInstr::ValueStateWrite { slot: 1, src: 1 },
                ],
                term: ValueTerm::Halt,
            }],
            0,
            2,
            2,
        );
        exec_entry(&ir, &mut world).unwrap();
        // Run 1: the read saw the empty state (0.0); slot 0 was then written 7
        // and must survive until the next run.
        assert_eq!(
            world.arena.get(world.value_state[0].unwrap()).unwrap(),
            &crate::arena::Value::Int(7)
        );
        assert_eq!(
            world.arena.get(world.value_state[1].unwrap()).unwrap(),
            &crate::arena::Value::Float(0.0)
        );
        exec_entry(&ir, &mut world).unwrap();
        // Run 2: the read saw run 1's 7 (the 1-run delay) and latched it
        // into slot 1.
        assert_eq!(
            world.arena.get(world.value_state[1].unwrap()).unwrap(),
            &crate::arena::Value::Int(7)
        );
    }

    #[test]
    fn value_regs_are_cleared_per_run() {
        // Value registers are per-run scratch: after a run they must be
        // cleared and their refs released, so a repeated-run value program
        // cannot exhaust the fixed arena (one leaked slot per register per
        // run). The run's result is captured into `world.main` (a fresh owner
        // held across runs), so exactly one slot stays live.
        let (ir, mut world) = prog_with(
            vec![ValueBlock {
                instrs: vec![
                    ValueInstr::ValueConstInt { dst: 0, value: 42 },
                    ValueInstr::ValueConstFloat { dst: 1, value: 1.5 },
                ],
                term: ValueTerm::Halt,
            }],
            0,
            2,
            0,
        );
        exec_entry(&ir, &mut world).unwrap();
        assert_eq!(world.value_regs[0], None);
        assert_eq!(world.value_regs[1], None);
        assert_eq!(
            world.arena.live(),
            1,
            "only the captured result survives the run"
        );
        exec_entry(&ir, &mut world).unwrap();
        assert_eq!(world.arena.live(), 1, "second run leaks nothing");
    }

    #[test]
    fn value_arith_computes_float_result() {
        // Int and Float operands widen to f64; the result is always Float.
        let (ir, mut world) = prog_with(
            vec![ValueBlock {
                instrs: vec![
                    ValueInstr::ValueConstInt { dst: 0, value: 3 },
                    ValueInstr::ValueConstFloat { dst: 1, value: 2.0 },
                    ValueInstr::ValueAdd { dst: 2, a: 0, b: 1 },
                    ValueInstr::ValueMul { dst: 3, a: 1, b: 2 },
                    ValueInstr::ValueDiv { dst: 4, a: 3, b: 1 },
                ],
                term: ValueTerm::Halt,
            }],
            0,
            5,
            0,
        );
        run_main_blocks_for_tests(&ir, &mut world).unwrap();
        let get = |r: usize| {
            world
                .arena
                .get(world.value_regs[r].unwrap())
                .unwrap()
                .clone()
        };
        assert_eq!(get(2), crate::arena::Value::Float(5.0), "3 + 2.0");
        assert_eq!(get(3), crate::arena::Value::Float(10.0), "2.0 * 5.0");
        assert_eq!(get(4), crate::arena::Value::Float(5.0), "10.0 / 2.0");
    }

    #[test]
    fn array_alloc_copy_recounts_bounds_and_elems() {
        // RC round-trip for the `Value::Array` arms: a copied array slot must
        // own an independent claim on its bounds ref AND on every element ref.
        // Dropping that slot must release exactly those claims — the original
        // array's own ownership is untouched.
        let (ir, mut world) = prog_with(vec![], 0, 0, 0);
        let lo = world.arena.alloc(Value::Int(0)).unwrap();
        let hi = world.arena.alloc(Value::Int(2)).unwrap();
        let bounds = world.arena.alloc(Value::Record(vec![lo, hi])).unwrap();
        let e0 = world.arena.alloc(Value::Int(10)).unwrap();
        let e1 = world.arena.alloc(Value::Int(20)).unwrap();
        let arr = world
            .arena
            .alloc(Value::Array {
                bounds,
                elems: vec![e0, e1],
            })
            .unwrap();

        // Baseline: each child is owned exactly once, by the array.
        assert_eq!(world.arena.rc(bounds), 1);
        assert_eq!(world.arena.rc(e0), 1);
        assert_eq!(world.arena.rc(e1), 1);

        let value = world.arena.get(arr).unwrap().clone();
        let copy = alloc_copy(&ir, &mut world, &value).unwrap();
        assert_eq!(world.arena.rc(bounds), 2, "the copy claims the bounds ref");
        assert_eq!(world.arena.rc(e0), 2, "the copy claims element 0");
        assert_eq!(world.arena.rc(e1), 2, "the copy claims element 1");

        world.arena.drop_ref(copy);
        assert_eq!(world.arena.rc(bounds), 1, "the bounds claim is released");
        assert_eq!(world.arena.rc(e0), 1, "element 0's claim is released");
        assert_eq!(world.arena.rc(e1), 1, "element 1's claim is released");
    }

    #[test]
    fn st_cell_kind_mismatch_faults_instead_of_silently_dropping() {
        // Defensive paths: typed rill-lang code cannot pass an `STArray` handle
        // to an `STRef` operation (or the reverse) — the static types differ —
        // and a released cell is unreachable too. Both are still reachable from
        // the interpreter (a handle is a plain `Value::Opaque` id), so each must
        // LATCH a fault and keep the refcount balanced rather than quietly drop
        // the operand.
        let (ir, mut world) = prog_with(vec![], 0, 0, 0);
        let bounds = world.arena.alloc(Value::Int(0)).unwrap();
        let e0 = world.arena.alloc(Value::Int(10)).unwrap();
        let e1 = world.arena.alloc(Value::Int(20)).unwrap();
        let e2 = world.arena.alloc(Value::Int(30)).unwrap();
        // Each cell owns one claim per ref it holds: the STArray owns `e0`/`e1`,
        // the STRef cell owns `e2`. Every value also keeps the test's own claim,
        // so the baseline rc is 2 throughout.
        _ = world.arena.copy(e0);
        _ = world.arena.copy(e1);
        _ = world.arena.copy(e2);
        world.st_cells.push(Some(StCell::Array {
            bounds,
            elems: vec![e0, e1],
        }));
        world.st_cells.push(Some(StCell::Ref(e2)));
        assert_eq!(world.arena.rc(e0), 2);

        // Hand the store a FRESH claim (`copy`), which is what a lowering-side
        // operand owns: a rejected store must release exactly that claim.
        let operand = world.arena.copy(e0).unwrap();
        st_ref_cell_store(&ir, &mut world, 0, operand);
        assert_eq!(
            format!("{}", world.value_error.as_ref().unwrap()),
            "Processing error: value-track fault: STRef operation on an STArray handle"
        );
        assert_eq!(
            world.arena.rc(e0),
            2,
            "the rejected operand's own claim is released; the cell keeps its own"
        );

        // STArray operations onto an STRef cell: the KIND is named, not reported
        // as a released cell.
        world.value_error = None;
        assert!(st_array_len(&ir, &mut world, 1).is_none());
        assert_eq!(
            format!("{}", world.value_error.as_ref().unwrap()),
            "Processing error: value-track fault: STArray operation on an STRef handle"
        );
        world.value_error = None;
        assert!(st_array_elem(&ir, &mut world, 1, 0).is_none());
        assert_eq!(
            format!("{}", world.value_error.as_ref().unwrap()),
            "Processing error: value-track fault: STArray operation on an STRef handle"
        );
        world.value_error = None;
        let operand = world.arena.copy(e0).unwrap();
        st_array_elem_store(&ir, &mut world, 1, 0, operand);
        assert_eq!(
            format!("{}", world.value_error.as_ref().unwrap()),
            "Processing error: value-track fault: STArray operation on an STRef handle"
        );
        assert_eq!(
            world.arena.rc(e0),
            2,
            "the rejected operand is released once"
        );

        // A live STArray cell with an out-of-range position is its own cause.
        world.value_error = None;
        let operand = world.arena.copy(e0).unwrap();
        st_array_elem_store(&ir, &mut world, 0, 5, operand);
        assert_eq!(
            format!("{}", world.value_error.as_ref().unwrap()),
            "Processing error: value-track fault: STArray element store: offset out of range"
        );

        // A RELEASED cell is reported distinctly, on both families.
        world.value_error = None;
        let operand = world.arena.copy(e0).unwrap();
        st_ref_cell_store(&ir, &mut world, 99, operand);
        assert_eq!(
            format!("{}", world.value_error.as_ref().unwrap()),
            "Processing error: value-track fault: STRef store on a released cell"
        );
        world.value_error = None;
        let operand = world.arena.copy(e0).unwrap();
        st_array_elem_store(&ir, &mut world, 99, 0, operand);
        assert_eq!(
            format!("{}", world.value_error.as_ref().unwrap()),
            "Processing error: value-track fault: STArray element store on a released cell"
        );
        assert_eq!(
            world.arena.rc(e0),
            2,
            "every rejected store dropped its own operand claim"
        );
    }

    #[test]
    fn forced_i64_accepts_int_and_integral_float_only() {
        // `forced_i64` is the flat-size/offset reader. An `Int` slot is the
        // normal case; the documented wart is that a NEGATED literal arrives as
        // a Float on the value track, so an INTEGRAL float is accepted. A
        // fractional float must be REFUSED instead of truncating `1.5` to index
        // 1 — silently addressing the wrong element.
        let (ir, mut world) = prog_with(vec![], 0, 0, 0);
        let int = world.arena.alloc(Value::Int(3)).unwrap();
        let integral = world.arena.alloc(Value::Float(2.0)).unwrap();
        let negative = world.arena.alloc(Value::Float(-1.0)).unwrap();
        let fractional = world.arena.alloc(Value::Float(1.5)).unwrap();
        let text = world.arena.alloc(Value::String("nope".into())).unwrap();

        let mut drops = Vec::new();
        assert_eq!(forced_i64(&ir, &mut world, int, &mut drops), Some(3));
        assert_eq!(forced_i64(&ir, &mut world, integral, &mut drops), Some(2));
        assert_eq!(forced_i64(&ir, &mut world, negative, &mut drops), Some(-1));
        assert_eq!(
            forced_i64(&ir, &mut world, fractional, &mut drops),
            None,
            "a fractional flat offset must not be truncated"
        );
        assert_eq!(forced_i64(&ir, &mut world, text, &mut drops), None);
    }

    #[test]
    fn array_shape_size_and_offset_follow_the_prelude_ix_instances() {
        // The interpreter-side shape mirror must reproduce the prelude `Ix Int`
        // and `Ix (Pair Int Int)` bodies exactly (Sharing the shape helper).
        let (ir, mut world) = prog_with(vec![], 0, 0, 0);
        // Dim1 (0..=2): size 3; offset of 1 is 1; out-of-range 3 is None.
        let dim1 = ArrayShape::Dim1 { lo: 0, hi: 2 };
        assert_eq!(dim1.size(), 3);
        let idx1 = world.arena.alloc(Value::Int(1)).unwrap();
        let idx3 = world.arena.alloc(Value::Int(3)).unwrap();
        let mut drops = Vec::new();
        assert_eq!(dim1.offset(&ir, &mut world, idx1, &mut drops), Some(1));
        assert_eq!(dim1.offset(&ir, &mut world, idx3, &mut drops), None);
        // Dim2 ((0,0)..(2,1)): a 3x2 grid, size 6, row-major.
        let dim2 = ArrayShape::Dim2 {
            lo1: 0,
            lo2: 0,
            hi1: 2,
            hi2: 1,
        };
        assert_eq!(dim2.size(), 6);
        let (a, b) = (
            world.arena.alloc(Value::Int(1)).unwrap(),
            world.arena.alloc(Value::Int(1)).unwrap(),
        );
        let cell = world.arena.alloc(Value::Record(vec![a, b])).unwrap();
        assert_eq!(
            dim2.offset(&ir, &mut world, cell, &mut drops),
            Some(3),
            "row 1, column 1 of a 3x2 grid is flat offset 3"
        );
        // Out-of-rectangle cell whose FLAT offset lies in range: `(0, 2)` on
        // `((0,0)..(2,1))` (the flat offset 2 is inside the 6-slot buffer, but
        // column 2 exceeds the bounded rectangle).
        let (c1, c2) = (
            world.arena.alloc(Value::Int(0)).unwrap(),
            world.arena.alloc(Value::Int(2)).unwrap(),
        );
        let bad = world.arena.alloc(Value::Record(vec![c1, c2])).unwrap();
        assert_eq!(
            dim2.offset(&ir, &mut world, bad, &mut drops),
            None,
            "the rectangle check fires before flat-offset arithmetic"
        );
        // Inverted bounds: a valid EMPTY array (size negative).
        let empty = ArrayShape::Dim1 { lo: 4, hi: 2 };
        assert!(empty.size() < 0);
    }

    #[test]
    fn value_arith_reassignment_drops_old_occupant() {
        // dst 0 is reused: the previous Int(42) ref must be released (deferred
        // drop), not leaked — the arena holds exactly the two live slots.
        let (ir, mut world) = prog_with(
            vec![ValueBlock {
                instrs: vec![
                    ValueInstr::ValueConstInt { dst: 0, value: 42 },
                    ValueInstr::ValueConstFloat { dst: 1, value: 2.0 },
                    ValueInstr::ValueMul { dst: 0, a: 0, b: 1 },
                ],
                term: ValueTerm::Halt,
            }],
            0,
            2,
            0,
        );
        run_main_blocks_for_tests(&ir, &mut world).unwrap();
        assert_eq!(
            world.arena.get(world.value_regs[0].unwrap()).unwrap(),
            &crate::arena::Value::Float(84.0)
        );
        assert_eq!(world.arena.live(), 2, "old dst occupant must be dropped");
    }

    #[test]
    fn value_match_selects_payload_of_matching_ctor() {
        // Sum(0, [7]) matched against ctor 0: dst reg 2 receives a shared ref to
        // the payload (rc++), so both the sum and the dst reg own it.
        let (ir, mut world) = prog_with(
            vec![ValueBlock {
                instrs: vec![
                    ValueInstr::ValueConstInt { dst: 0, value: 7 },
                    ValueInstr::ValueConstructSum {
                        dst: 1,
                        ctor: 0,
                        payload: vec![0],
                    },
                    ValueInstr::ValueMatch {
                        dst: vec![2],
                        slot: 1,
                        ctor: 0,
                    },
                ],
                term: ValueTerm::Halt,
            }],
            0,
            3,
            0,
        );
        run_main_blocks_for_tests(&ir, &mut world).unwrap();
        let payload = world.value_regs[2].unwrap();
        assert_eq!(
            world.arena.get(payload).unwrap(),
            &crate::arena::Value::Int(7)
        );
        let sum = world.value_regs[1].unwrap();
        assert_eq!(world.arena.rc(sum), 1);
        assert_eq!(
            world.arena.rc(payload),
            3,
            "const reg + sum payload + match dst each own it"
        );
    }

    #[test]
    fn value_match_non_matching_ctor_writes_none() {
        // Sum(1, [7]) matched against ctor 0: no arm matches, so the dst reg is
        // written None (a detectable no-op under v1 static dispatch).
        let (ir, mut world) = prog_with(
            vec![ValueBlock {
                instrs: vec![
                    ValueInstr::ValueConstInt { dst: 0, value: 7 },
                    ValueInstr::ValueConstructSum {
                        dst: 1,
                        ctor: 1,
                        payload: vec![0],
                    },
                    ValueInstr::ValueMatch {
                        dst: vec![2],
                        slot: 1,
                        ctor: 0,
                    },
                ],
                term: ValueTerm::Halt,
            }],
            0,
            3,
            0,
        );
        run_main_blocks_for_tests(&ir, &mut world).unwrap();
        assert_eq!(world.value_regs[2], None);
    }

    #[test]
    fn trampoline_branch_and_move() {
        // A 5-block CFG: block 0 loads Bool(true) and two floats, moves the
        // then-path value into reg 3, then Branches on the Bool. Block 1 (the
        // `then` target) moves the else-path value into the out reg 4 and
        // falls through to Halt; block 2 (the `els` target) would overwrite
        // reg 4 with 999.0 if the branch dispatched wrongly. Block 3 is the
        // Halt block; block 4 is a SENTINEL after it that writes reg 5 —
        // reaching it means Halt was ignored (fall-off past the terminator),
        // so reg 5 must stay `None` after the tick.
        let (ir, mut world) = prog_with(
            vec![
                ValueBlock {
                    instrs: vec![
                        ValueInstr::ValueBool {
                            dst: 0,
                            value: true,
                        },
                        ValueInstr::ValueConstFloat { dst: 1, value: 1.0 },
                        ValueInstr::ValueConstFloat { dst: 2, value: 0.0 },
                        ValueInstr::ValueMove { dst: 3, src: 1 },
                    ],
                    term: ValueTerm::Branch {
                        cond: 0,
                        then: 1,
                        els: 2,
                    },
                },
                ValueBlock {
                    instrs: vec![ValueInstr::ValueMove { dst: 4, src: 2 }],
                    term: ValueTerm::Fallthrough(3),
                },
                ValueBlock {
                    instrs: vec![ValueInstr::ValueConstFloat {
                        dst: 4,
                        value: 999.0,
                    }],
                    term: ValueTerm::Fallthrough(3),
                },
                ValueBlock {
                    instrs: vec![],
                    term: ValueTerm::Halt,
                },
                // Sentinel: must never run.
                ValueBlock {
                    instrs: vec![ValueInstr::ValueConstFloat {
                        dst: 5,
                        value: 777.0,
                    }],
                    term: ValueTerm::Halt,
                },
            ],
            0,
            6,
            0,
        );
        run_main_blocks_for_tests(&ir, &mut world).unwrap();
        assert_eq!(
            world.value_regs[1], None,
            "ValueMove cleared the source reg"
        );
        assert_eq!(
            world.value_regs[2], None,
            "ValueMove cleared the source reg"
        );
        assert_eq!(
            world.arena.get(world.value_regs[3].unwrap()).unwrap(),
            &crate::arena::Value::Float(1.0),
            "block 0's move landed the then-path value"
        );
        assert_eq!(
            world.arena.get(world.value_regs[4].unwrap()).unwrap(),
            &crate::arena::Value::Float(0.0),
            "the Branch selected the then block, not the else (999.0)"
        );
        assert_eq!(
            world.value_regs[5], None,
            "the sentinel block after Halt must never execute"
        );
    }

    #[test]
    fn trampoline_branch_false_takes_els() {
        // A `Branch` with a `Bool(false)` condition must run the `els` block,
        // not the `then` block: block 0 loads Bool(false); block 1 (`then`)
        // would write 999.0 into reg 3, block 2 (`els`) writes 0.0. The sentinel
        // block after Halt must stay `None` (reg 4).
        let (ir, mut world) = prog_with(
            vec![
                ValueBlock {
                    instrs: vec![
                        ValueInstr::ValueBool {
                            dst: 0,
                            value: false,
                        },
                        ValueInstr::ValueConstFloat { dst: 1, value: 1.0 },
                        ValueInstr::ValueConstFloat { dst: 2, value: 0.0 },
                    ],
                    term: ValueTerm::Branch {
                        cond: 0,
                        then: 1,
                        els: 2,
                    },
                },
                ValueBlock {
                    instrs: vec![ValueInstr::ValueConstFloat {
                        dst: 3,
                        value: 999.0,
                    }],
                    term: ValueTerm::Fallthrough(3),
                },
                ValueBlock {
                    instrs: vec![ValueInstr::ValueConstFloat { dst: 3, value: 0.0 }],
                    term: ValueTerm::Fallthrough(3),
                },
                ValueBlock {
                    instrs: vec![],
                    term: ValueTerm::Halt,
                },
                // Sentinel: must never run.
                ValueBlock {
                    instrs: vec![ValueInstr::ValueConstFloat {
                        dst: 4,
                        value: 777.0,
                    }],
                    term: ValueTerm::Halt,
                },
            ],
            0,
            5,
            0,
        );
        run_main_blocks_for_tests(&ir, &mut world).unwrap();
        assert_eq!(
            world.arena.get(world.value_regs[3].unwrap()).unwrap(),
            &crate::arena::Value::Float(0.0),
            "the false Branch ran the els block, not the then (999.0)"
        );
        assert_eq!(
            world.value_regs[4], None,
            "the sentinel block after Halt must never execute"
        );
    }
}

#[cfg(test)]
mod value_cmp_tests {
    use super::*;

    #[test]
    fn value_cmp_orders_structural() {
        use crate::arena::{Arena, Value};
        let mut a = Arena::with_capacity(16);
        let r1 = a.alloc(Value::Float(1.0)).unwrap();
        let r2 = a.alloc(Value::Float(2.0)).unwrap();
        assert!(value_cmp(&a, r1, r2) < 0);
        assert_eq!(value_cmp(&a, r1, r1), 0);
        assert!(value_cmp(&a, r2, r1) > 0);
        let s1 = a.alloc(Value::String("a".into())).unwrap();
        let s2 = a.alloc(Value::String("b".into())).unwrap();
        assert!(value_cmp(&a, s1, s2) < 0);
        let b = a.alloc(Value::Bool(true)).unwrap();
        assert!(value_cmp(&a, b, r1) < 0, "Bool sorts before Int");
    }

    fn int_list(a: &mut Arena<Value>, len: usize) -> ArenaRef {
        let elems: Vec<ArenaRef> = (0..len).map(|_| a.alloc(Value::Int(0)).unwrap()).collect();
        a.alloc(Value::List { elems }).unwrap()
    }

    #[test]
    fn value_cmp_orders_containers() {
        use crate::arena::{Arena, Value};
        let mut a = Arena::with_capacity(32);
        let f1 = a.alloc(Value::Float(1.0)).unwrap();
        let f2 = a.alloc(Value::Float(2.0)).unwrap();

        // Sum: constructor index first, then payload.
        let s_1_1 = a.alloc(Value::Sum(0, vec![f1])).unwrap();
        let s_1_2 = a.alloc(Value::Sum(0, vec![f2])).unwrap();
        let s_2_0 = a.alloc(Value::Sum(1, vec![])).unwrap();
        assert!(value_cmp(&a, s_1_1, s_1_2) < 0, "same ctor, payload orders");
        assert!(
            value_cmp(&a, s_1_2, s_2_0) < 0,
            "ctor index dominates payload"
        );

        // Record: field order.
        let r1 = a.alloc(Value::Record(vec![f1])).unwrap();
        let r2 = a.alloc(Value::Record(vec![f2])).unwrap();
        assert!(value_cmp(&a, r1, r2) < 0, "record field order");

        // Newtype: unwraps to the inner value.
        let n1 = a.alloc(Value::Newtype(f1)).unwrap();
        let n2 = a.alloc(Value::Newtype(f2)).unwrap();
        assert!(value_cmp(&a, n1, n2) < 0, "newtype compares by inner value");

        // List: lexicographic, shorter is less when prefixes match.
        let l1 = a.alloc(Value::List { elems: vec![f1] }).unwrap();
        let l12 = a
            .alloc(Value::List {
                elems: vec![f1, f2],
            })
            .unwrap();
        assert!(
            value_cmp(&a, l1, l12) < 0,
            "list lexicographic, shorter first"
        );

        // Map: key equal, value differs; then key differs.
        let ka = a.alloc(Value::String("a".into())).unwrap();
        let kb = a.alloc(Value::String("b".into())).unwrap();
        let m_a1 = a
            .alloc(Value::Map {
                pairs: vec![(ka, f1)],
            })
            .unwrap();
        let m_a2 = a
            .alloc(Value::Map {
                pairs: vec![(ka, f2)],
            })
            .unwrap();
        let m_b1 = a
            .alloc(Value::Map {
                pairs: vec![(kb, f1)],
            })
            .unwrap();
        assert!(value_cmp(&a, m_a1, m_a2) < 0, "map value differs");
        assert!(value_cmp(&a, m_a1, m_b1) < 0, "map key differs");
    }

    #[test]
    fn value_cmp_length_tiebreak_survives_large_containers() {
        use crate::arena::Arena;
        // 200 identical elements compare equal across two lists; a 201-element
        // list with the same prefix orders after. The length tiebreak must not
        // wrap at i8 (127+ elements would otherwise invert the order).
        let mut a = Arena::with_capacity(900);
        let l200a = int_list(&mut a, 200);
        let l200b = int_list(&mut a, 200);
        let l201 = int_list(&mut a, 201);
        assert_eq!(
            value_cmp(&a, l200a, l200b),
            0,
            "identical prefixes, equal length"
        );
        assert!(
            value_cmp(&a, l200a, l201) < 0,
            "200-elem list sorts before 201-elem"
        );
        assert!(
            value_cmp(&a, l201, l200a) > 0,
            "201-elem list sorts after 200-elem"
        );

        // The wrap that motivated the fix: 127 must not order above 128/200.
        let l127 = int_list(&mut a, 127);
        let l128 = int_list(&mut a, 128);
        assert!(
            value_cmp(&a, l127, l128) < 0,
            "127 < 128 across the i8 wrap"
        );
        assert!(
            value_cmp(&a, l127, l200a) < 0,
            "127 < 200 across the i8 wrap"
        );
    }
}

#[cfg(test)]
mod lazy_track_tests {
    use super::*;
    use crate::arena::{ArenaRef, ThunkState, Value};
    use crate::ir::{
        FragmentIr, FuncSig, Ir, StateLayout, ValueBlock, ValueInstr, ValueLayout, ValueTerm,
    };
    use crate::program::WorldConfig;
    use std::sync::Arc;

    /// The shared thunk-body fragment: computes `ValueConstInt { value: 7 }`.
    /// A thunk body is a closure with no argument registers — the env IS the
    /// captured frame (`value_ins == 0`, one value out).
    fn const7_fragment() -> Arc<FragmentIr> {
        Arc::new(FragmentIr {
            value_blocks: vec![ValueBlock {
                instrs: vec![ValueInstr::ValueConstInt { dst: 0, value: 7 }],
                term: ValueTerm::Halt,
            }],
            entry: 0,
            num_value_regs: 1,
            output_value_regs: vec![0],
            num_capture_cells: 0,
            sig: FuncSig {
                value_ins: 0,
                value_outs: 1,
                signal_ins: 0,
            },
        })
    }

    fn prog_with(
        value_blocks: Vec<ValueBlock>,
        num_value_regs: usize,
        result_reg: usize,
        fragments: Vec<Arc<FragmentIr>>,
    ) -> (Ir, WorldState<f32, 256>) {
        let ir = Ir {
            main_io_unit: false,
            num_inputs: 0,
            num_outputs: 0,
            state: StateLayout::default(),
            builtins: Vec::new(),
            params: Vec::new(),
            value_blocks,
            value_entry: 0,
            num_value_regs,
            result_reg,
            value_funcs: Vec::new(),
            fragments,
            array_hole_frag: None,
            max_call_regs: 8,
            value_state: ValueLayout {
                capacity: 64,
                buffer_budget: 64,
                value_state_slots: 0,
            },
            num_buffers: 0,

            input_buffers: Vec::new(),

            tapes: Vec::new(),
        };
        let mut world = WorldState::<f32, 256>::new(WorldConfig::default());
        world.seat(&ir).unwrap();
        (ir, world)
    }

    fn empty_env(_ir: &Ir, world: &mut WorldState<f32, 256>) -> ArenaRef {
        world.arena.alloc(Value::Record(vec![])).unwrap()
    }

    fn alloc_thunk(
        ir: &Ir,
        world: &mut WorldState<f32, 256>,
        state: ThunkState,
    ) -> (ArenaRef, ArenaRef) {
        let env = empty_env(ir, world);
        let cell = world.arena.alloc(Value::ThunkCell(state)).unwrap();
        let t = world
            .arena
            .alloc(Value::Callable {
                fragment: 0,
                env,
                cell: Some(cell),
            })
            .unwrap();
        (t, cell)
    }

    #[test]
    fn force_unforced_thunk_runs_and_memoizes() {
        // Force an Unforced thunk whose body produces Int(7): the first force
        // runs the body and memoizes the result in the cell; the second force
        // returns the SAME head ref without re-running the body.
        let (ir, mut world) = prog_with(Vec::new(), 2, 0, vec![const7_fragment()]);
        let (t, cell) = alloc_thunk(&ir, &mut world, ThunkState::Unforced);
        let mut drops = Vec::new();
        let r1 = force(&ir, &mut world, t, &mut drops);
        let r2 = force(&ir, &mut world, t, &mut drops);
        assert_eq!(r1, r2, "memoization: both forces return the same head ref");
        assert_eq!(world.arena.get(r1).unwrap(), &Value::Int(7));
        assert_eq!(
            world.arena.get(cell).unwrap(),
            &Value::ThunkCell(ThunkState::Forced(r1)),
            "the body ran exactly once and the cell memoized its result"
        );
    }

    #[test]
    fn force_black_hole_is_bottom() {
        // A cell already in `Forcing` (the thunk was re-entered while forcing)
        // is a black hole: forcing yields bottom and the cell memoizes it.
        let (ir, mut world) = prog_with(Vec::new(), 2, 0, vec![const7_fragment()]);
        let (t, cell) = alloc_thunk(&ir, &mut world, ThunkState::Forcing);
        let mut drops = Vec::new();
        let b = force(&ir, &mut world, t, &mut drops);
        assert_eq!(
            world.arena.get(b).unwrap(),
            &Value::Bottom,
            "a black hole yields bottom"
        );
        assert_eq!(
            world.arena.get(cell).unwrap(),
            &Value::ThunkCell(ThunkState::Forced(b)),
            "the black hole is memoized as a bottom result"
        );
    }

    #[test]
    fn force_forced_returns_memo() {
        // Force a thunk, share the thunk VALUE (rc++ on the same slot — the
        // memo cell is shared, not copied), force the copy: the memoized ref
        // is returned and the cell's rc stays 1.
        let (ir, mut world) = prog_with(Vec::new(), 2, 0, vec![const7_fragment()]);
        let (t, cell) = alloc_thunk(&ir, &mut world, ThunkState::Unforced);
        let mut drops = Vec::new();
        let r1 = force(&ir, &mut world, t, &mut drops);
        let t2 = world.arena.copy(t).unwrap();
        assert_eq!(
            world.arena.rc(cell),
            1,
            "sharing the thunk shares the memo cell"
        );
        let r2 = force(&ir, &mut world, t2, &mut drops);
        assert_eq!(r1, r2, "the shared copy sees the same memoized result");
        assert_eq!(
            world.arena.get(cell).unwrap(),
            &Value::ThunkCell(ThunkState::Forced(r1))
        );
    }

    #[test]
    fn bottom_propagates_through_add() {
        // A `Value::Bottom` operand of a strict `ValueAdd` yields bottom.
        let (ir, mut world) = prog_with(
            vec![ValueBlock {
                instrs: vec![
                    ValueInstr::ValueConstInt { dst: 0, value: 3 },
                    ValueInstr::ValueAdd { dst: 2, a: 0, b: 1 },
                ],
                term: ValueTerm::Halt,
            }],
            3,
            0,
            Vec::new(),
        );
        let bottom = world.arena.alloc(Value::Bottom).unwrap();
        world.value_regs[1] = Some(bottom);
        run_main_blocks_for_tests(&ir, &mut world).unwrap();
        let res = world.value_regs[2].unwrap();
        assert_eq!(
            world.arena.get(res).unwrap(),
            &Value::Bottom,
            "strict ops propagate a bottom operand"
        );
    }

    #[test]
    fn strict_add_forces_thunk_operand() {
        // ValueLazy suspends the const-7 fragment; the strict Add forces the
        // operand to WHNF before reading it (7 + 1.5 = 8.5).
        let (ir, mut world) = prog_with(
            vec![ValueBlock {
                instrs: vec![
                    ValueInstr::ValueConstFloat { dst: 2, value: 1.5 },
                    ValueInstr::ValueLazy {
                        dst: 1,
                        env: 0,
                        fragment: 0,
                    },
                    ValueInstr::ValueAdd { dst: 3, a: 1, b: 2 },
                ],
                term: ValueTerm::Halt,
            }],
            4,
            0,
            vec![const7_fragment()],
        );
        let env = empty_env(&ir, &mut world);
        world.value_regs[0] = Some(env);
        run_main_blocks_for_tests(&ir, &mut world).unwrap();
        let res = world.value_regs[3].unwrap();
        assert_eq!(world.arena.get(res).unwrap(), &Value::Float(8.5), "7 + 1.5");
    }

    #[test]
    fn match_on_bottom_writes_bottom_to_all_arms() {
        // A Bottom scrutinee of ValueMatch writes bottom to every arm dst and
        // never selects a payload.
        let (ir, mut world) = prog_with(
            vec![ValueBlock {
                instrs: vec![ValueInstr::ValueMatch {
                    dst: vec![1, 2],
                    slot: 0,
                    ctor: 0,
                }],
                term: ValueTerm::Halt,
            }],
            3,
            0,
            Vec::new(),
        );
        let bottom = world.arena.alloc(Value::Bottom).unwrap();
        world.value_regs[0] = Some(bottom);
        run_main_blocks_for_tests(&ir, &mut world).unwrap();
        assert_eq!(
            world.arena.get(world.value_regs[1].unwrap()).unwrap(),
            &Value::Bottom
        );
        assert_eq!(
            world.arena.get(world.value_regs[2].unwrap()).unwrap(),
            &Value::Bottom
        );
    }

    #[test]
    fn thunk_body_can_create_and_force_nested_thunk() {
        // A thunk whose body itself allocates + forces a nested thunk: forcing
        // the outer runs fragment 0 (with remapped ValueLazy/ValueForce over an
        // unbound env → fresh Void fallback), which lazily builds the inner
        // thunk over fragment 1 and forces it to Int(7).
        let frag0 = Arc::new(FragmentIr {
            value_blocks: vec![ValueBlock {
                instrs: vec![
                    ValueInstr::ValueLazy {
                        dst: 1,
                        env: 0,
                        fragment: 1,
                    },
                    ValueInstr::ValueForce { dst: 2, src: 1 },
                ],
                term: ValueTerm::Halt,
            }],
            entry: 0,
            num_value_regs: 3,
            output_value_regs: vec![2],
            num_capture_cells: 0,
            sig: FuncSig {
                value_ins: 0,
                value_outs: 1,
                signal_ins: 0,
            },
        });
        let (ir, mut world) = prog_with(Vec::new(), 2, 0, vec![frag0, const7_fragment()]);
        let (t, cell) = alloc_thunk(&ir, &mut world, ThunkState::Unforced);
        let mut drops = Vec::new();
        let r = force(&ir, &mut world, t, &mut drops);
        assert_eq!(
            world.arena.get(r).unwrap(),
            &Value::Int(7),
            "the outer thunk's body forced the nested thunk to WHNF"
        );
        assert_eq!(
            world.arena.get(cell).unwrap(),
            &Value::ThunkCell(ThunkState::Forced(r))
        );
    }

    #[test]
    fn result_capture_forces_raw_thunk() {
        // A lazy result register (a raw thunk, never explicitly forced) is
        // forced to WHNF by the run-end result capture.
        let (ir, mut world) = prog_with(
            vec![ValueBlock {
                instrs: vec![ValueInstr::ValueLazy {
                    dst: 1,
                    env: 0,
                    fragment: 0,
                }],
                term: ValueTerm::Halt,
            }],
            2,
            1,
            vec![const7_fragment()],
        );
        let env = empty_env(&ir, &mut world);
        world.value_regs[0] = Some(env);
        exec_entry(&ir, &mut world).unwrap();
        let v = world.result().expect("the run must capture a result");
        assert_eq!(
            world.arena().get(v).unwrap(),
            &Value::Int(7),
            "the result capture forces the lazy result to WHNF"
        );
        assert_eq!(world.arena.live(), 1, "only the result survives the run");
    }

    /// The shared self-referential body: reads capture cell 0 (the thunk's own
    /// env field, which is the thunk itself) and returns a COW copy of it.
    /// This is the `a = a` shape — the body produces a fresh slot sharing the
    /// same memo cell.
    fn self_ref_fragment() -> Arc<FragmentIr> {
        Arc::new(FragmentIr {
            value_blocks: vec![ValueBlock {
                instrs: vec![ValueInstr::ValueReadCell { dst: 0, cell: 0 }],
                term: ValueTerm::Halt,
            }],
            entry: 0,
            num_value_regs: 1,
            output_value_regs: vec![0],
            num_capture_cells: 1,
            sig: FuncSig {
                value_ins: 0,
                value_outs: 1,
                signal_ins: 0,
            },
        })
    }

    #[test]
    fn force_self_cycle_returns_bottom_not_hang() {
        // A thunk whose body returns a COW copy of ITSELF (`a = a`): forcing
        // must yield bottom, not hang. The COW copy shares the memo cell (cell
        // rc > 1), so this also exercises the shared-cell mutation path that
        // used to debug-assert on rc == 1.
        let (ir, mut world) = prog_with(Vec::new(), 2, 0, vec![self_ref_fragment()]);
        let env = world.arena.alloc(Value::Record(vec![])).unwrap();
        let cell = world
            .arena
            .alloc(Value::ThunkCell(ThunkState::Unforced))
            .unwrap();
        let t = world
            .arena
            .alloc(Value::Callable {
                fragment: 0,
                env,
                cell: Some(cell),
            })
            .unwrap();
        // The env record holds the thunk itself (its field 0 is `t`), so the
        // body's capture read of field 0 produces a self-referential copy.
        if let Value::Record(fields) = world.arena.get_mut(env).unwrap() {
            fields.push(t);
        }
        let mut drops = Vec::new();
        let b = force(&ir, &mut world, t, &mut drops);
        assert_eq!(
            world.arena.get(b).unwrap(),
            &Value::Bottom,
            "a self-cycle is bottom, not a hang"
        );
        assert_eq!(
            world.arena.get(cell).unwrap(),
            &Value::ThunkCell(ThunkState::Forced(b)),
            "the self-cycle is memoized as a bottom result"
        );
        // The COW copy created during forcing was dropped; the cell is back to
        // a single reference (the thunk `t`).
        assert_eq!(world.arena.rc(cell), 1, "shared-cell mutation did not leak");
    }

    #[test]
    fn force_shared_cell_thunk_memoizes_without_panic() {
        // Two thunk VALUES sharing one memo cell (the STG update-frame shape):
        // forcing either must not assert on the cell's rc and both see the
        // memoized result.
        let (ir, mut world) = prog_with(Vec::new(), 2, 0, vec![const7_fragment()]);
        let env = world.arena.alloc(Value::Record(vec![])).unwrap();
        let cell = world
            .arena
            .alloc(Value::ThunkCell(ThunkState::Unforced))
            .unwrap();
        let _t1 = world
            .arena
            .alloc(Value::Callable {
                fragment: 0,
                env,
                cell: Some(cell),
            })
            .unwrap();
        // Second thunk value sharing env + cell; count its references.
        world.arena.copy(env).unwrap();
        world.arena.copy(cell).unwrap();
        let t2 = world
            .arena
            .alloc(Value::Callable {
                fragment: 0,
                env,
                cell: Some(cell),
            })
            .unwrap();
        let mut drops = Vec::new();
        let r = force(&ir, &mut world, t2, &mut drops);
        assert_eq!(
            world.arena.get(r).unwrap(),
            &Value::Int(7),
            "forcing the shared-cell thunk memoizes Int(7)"
        );
        assert_eq!(
            world.arena.get(cell).unwrap(),
            &Value::ThunkCell(ThunkState::Forced(r))
        );
        // A second force through the OTHER thunk returns the same memo.
        let mut drops2 = Vec::new();
        let r2 = force(&ir, &mut world, t2, &mut drops2);
        assert_eq!(r2, r, "the memo is shared across thunk values");
    }
}
