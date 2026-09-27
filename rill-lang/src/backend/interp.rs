//! IR evaluator: the block-only executor.

use rill_core::buffer::FixedBuffer;
use rill_core::math::vector::ScalarVector4;
use rill_core::math::Transcendental;
use rill_core::traits::MultichannelAlgorithm;
use rill_core::traits::ProcessError;

use crate::arena::{Arena, ArenaRef, Value};
use crate::ir::{BinArith, CmpOp, FragmentIr, Instr, LogicOp, UnOp, ValueBuiltinOp, ValueInstr};
use crate::program::{RillProgram, MAX_BUILTIN_CHANNELS};
use crate::schedule::Step;

pub(crate) fn param_to_f64(pv: &rill_core::traits::ParamValue) -> f64 {
    match pv {
        rill_core::traits::ParamValue::Float(v) => *v as f64,
        rill_core::traits::ParamValue::Int(v) => *v as f64,
        _ => 0.0,
    }
}

pub(crate) fn push_builtin_params<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
) {
    let n = prog.ir.builtins.len();
    for instance in 0..n {
        let blen = prog.ir.builtins[instance].param_bindings.len();
        for k in 0..blen {
            let (arg_pos, param_idx) = prog.ir.builtins[instance].param_bindings[k];
            if !prog.params_dirty[param_idx] {
                continue;
            }
            let v = prog.params[param_idx].clone();
            prog.params_dirty[param_idx] = false;
            match &mut prog.builtins[instance] {
                crate::program::BuiltinInst::Block(b) => b.set_param(arg_pos, &v),
                crate::program::BuiltinInst::MultichannelBlock(b) => b.set_param(arg_pos, &v),
            }
        }
    }
}

/// Run one block via the schedule. Every step is a whole-buffer operation.
/// Supports N inputs → M outputs. Returns a `ProcessError` when the value track
/// latched one (a collection capacity overflow — a user error, not a build bug).
pub fn run_block_mimo<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    inputs: &[&[T]],
    outputs: &mut [&mut [T]],
) -> Result<(), ProcessError> {
    push_builtin_params(prog);
    // Release the previous tick's value outputs BEFORE this tick allocates.
    // A value output pins its whole subtree across ticks (the output keeps the
    // root at rc >= 1), and a closure output additionally pins its captured env
    // record — so the old tree must be freed before the new tick's allocations
    // can reuse its slots (the build-time capacity bound assumes at most one
    // tick's live set at a time). The output is re-stored at tick end.
    for out in &mut prog.value_outputs {
        if let Some(r) = out.take() {
            prog.arena.drop_ref(r);
        }
    }
    let n = outputs.first().map(|o| o.len()).unwrap_or(0);
    debug_assert!(n <= BUF, "block length {n} exceeds BUF {BUF}");

    // Move the step list out of `prog` so we can borrow `prog`'s registers
    // mutably while iterating. `mem::take` leaves an empty `Vec` behind — no
    // allocation on the RT path — and we move the list back at the end.
    let steps = std::mem::take(&mut prog.schedule.steps);
    for step in &steps {
        match step {
            Step::Block(idx) => exec_block_op(prog, *idx, inputs, n),
            Step::ForeignBlock(idx) => exec_foreign_block(prog, *idx, n),
        }
    }
    prog.schedule.steps = steps;

    // Value-track phase (per-tick): allocate/free the program's values. A
    // latched value error (a collection capacity overflow) is propagated only
    // AFTER the tick-end cleanup below — the registers, block-state swap and
    // output release all still run, so a repeatedly erroring program cannot
    // leak the fixed arena or desync its feedback state.
    let value_res = run_value_track(prog);

    // Apply the block-level feedback shadow copy (double-buffer swap).
    prog.swap_block_state();

    for (i, out) in outputs.iter_mut().enumerate() {
        if let Some(&reg) = prog.ir.output_regs.get(i) {
            let m = out.len().min(n);
            out[..m].copy_from_slice(&prog.block_regs[reg][..m]);
        }
    }

    // Copy the value outputs (one per value output channel) into the program's
    // stable `value_outputs` store. Each output is an INDEPENDENT counted owner:
    // `copy` (rc++) so it survives the register clear below, and the previous
    // tick's output was already released at the start of this tick (see above)
    // so nothing leaks across ticks. `clear_value_regs` then drops the
    // register's refs (the per-tick scratch) while the output refs remain live.
    for (i, &r) in prog.ir.value_output_regs.iter().enumerate() {
        if let Some(Some(v)) = prog.value_regs.get(r) {
            if let Some(prev) = prog.value_outputs[i].take() {
                prog.arena.drop_ref(prev);
            }
            if let Ok(c) = prog.arena.copy(*v) {
                prog.value_outputs[i] = Some(c);
            }
        }
    }

    // Release this tick's per-tick value registers now that the value outputs
    // have been read (value outputs are copied after the block outputs above).
    // The value-state is NOT cleared: it is the 1-tick delay
    // store, and `ValueStateWrite` already drops the previous ref when
    // overwriting a slot.
    prog.clear_value_regs();
    value_res
}

/// Execute the per-tick value track: run every [`ValueInstr`] once per block,
/// in order, allocating and freeing values in the program's fixed arena.
///
/// Every value register, value-state slot, and cell holds ONE ownership of the
/// arena ref it stores (the RC is already counted for that ref). A store that
/// places an already-owned ref into a second location `copy`s it first
/// (RC++); a store that MOVES a ref clears the source (`None`). Drops are
/// deferred to the end of the track so a drop cannot free a slot mid-track
/// that a later instruction reuses, and the release order is deterministic.
pub(crate) fn run_value_track<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
) -> Result<(), ProcessError> {
    // Move the instruction list out of `prog` so we can borrow `prog`'s value
    // registers mutably while iterating (`mem::take` leaves an empty `Vec`
    // behind — no allocation on the RT path). Drops queue on the shared,
    // pre-sized `drops_scratch`, moved out ONCE here and threaded through the
    // whole dispatch tree (`exec_value_instr` → `run_fragment` → ...) as a
    // separate parameter, so every fragment at every nesting depth reuses the
    // same reserved buffer — no allocation in the RT path. The mark/drain
    // discipline keeps each frame's pending drops below its own mark.
    let mut drops = std::mem::take(&mut prog.drops_scratch);
    let mark = drops.len();
    let value_instrs = std::mem::take(&mut prog.ir.value_instrs);
    for instr in &value_instrs {
        exec_value_instr(prog, instr, &mut drops);
    }
    prog.ir.value_instrs = value_instrs;
    for r in drops.drain(mark..) {
        prog.arena.drop_ref(r);
    }
    prog.drops_scratch = drops;
    // The value-error latch: a collection op (e.g. `cons` past capacity) sets
    // it and the tick fails with a user-facing `ProcessError` rather than a
    // silent `None` (that channel is reserved for build-time arena exhaustion).
    if let Some(err) = prog.value_error.take() {
        return Err(err);
    }
    Ok(())
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
/// register is a detectable no-op in release.
fn alloc_owned<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    v: Value,
) -> Option<ArenaRef> {
    let children: Vec<ArenaRef> = match v {
        Value::Record(ref fields) | Value::Sum(_, ref fields) => fields.clone(),
        Value::Newtype(inner) => vec![inner],
        Value::Closure(env, _) => vec![env],
        Value::List { ref elems, .. } => elems.clone(),
        Value::Map { ref pairs, .. } => pairs.iter().flat_map(|(k, v)| [*k, *v]).collect(),
        Value::Set { ref elems, .. } => elems.clone(),
        _ => Vec::new(),
    };
    match prog.arena.alloc(v) {
        Ok(r) => Some(r),
        Err(_) => {
            for c in children {
                prog.arena.drop_ref(c);
            }
            debug_assert!(false, "value arena capacity exhausted at build time");
            None
        }
    }
}

/// Allocate a fresh slot holding an independent copy of `v`.
///
/// The new slot counts its own refs on `v`'s children (RC++ per child), so the
/// caller may keep `v` — its register retains its own ownership. Returns `None`
/// on exhaustion, undoing the recounts so nothing leaks.
fn alloc_copy<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    v: &Value,
) -> Option<ArenaRef> {
    let cloned = v.clone();
    match &cloned {
        Value::Record(fields) | Value::Sum(_, fields) => {
            for f in fields {
                // The fresh slot shares these refs with the original value:
                // count each one so both owners are balanced.
                _ = prog.arena.copy(*f);
            }
        }
        Value::Newtype(inner) => {
            _ = prog.arena.copy(*inner);
        }
        Value::Closure(env, _) => {
            _ = prog.arena.copy(*env);
        }
        Value::List { elems, .. } => {
            for e in elems {
                _ = prog.arena.copy(*e);
            }
        }
        Value::Map { pairs, .. } => {
            for (k, v) in pairs {
                _ = prog.arena.copy(*k);
                _ = prog.arena.copy(*v);
            }
        }
        Value::Set { elems, .. } => {
            for e in elems {
                _ = prog.arena.copy(*e);
            }
        }
        _ => {}
    }
    match prog.arena.alloc(cloned) {
        Ok(r) => Some(r),
        Err(_) => {
            // No slot was created; undo the recounts above so the original
            // value keeps exclusive ownership of its children. Never `Some(0)`.
            drop_value_children(prog, v);
            debug_assert!(false, "value arena capacity exhausted at build time");
            None
        }
    }
}

/// Share `r` into a new owner (RC++). `None` on arena error — never `Some(0)`.
fn copy_owned<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    r: ArenaRef,
) -> Option<ArenaRef> {
    match prog.arena.copy(r) {
        Ok(c) => Some(c),
        Err(_) => {
            debug_assert!(false, "copy on a freed or overflowing arena ref");
            None
        }
    }
}

/// Collect a copied ref for every register in `regs`.
///
/// Each source register keeps its own ownership, so every collected ref is a
/// fresh `copy` (RC++) — the constructed record/sum owns its fields
/// independently of the source registers (aliasing). Returns `None`, dropping
/// anything already copied, when any source is unbound or the arena cannot
/// count another ref.
fn read_field_refs<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    regs: &[usize],
) -> Option<Vec<ArenaRef>> {
    let mut refs: Vec<ArenaRef> = Vec::with_capacity(regs.len());
    for r in regs {
        match prog.value_regs[*r] {
            Some(src) => match prog.arena.copy(src) {
                Ok(c) => refs.push(c),
                Err(_) => {
                    for x in refs {
                        prog.arena.drop_ref(x);
                    }
                    return None;
                }
            },
            None => {
                for x in refs {
                    prog.arena.drop_ref(x);
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
    prog: &mut RillProgram<T, BUF>,
    v: &Value,
) {
    match v {
        Value::Record(fields) | Value::Sum(_, fields) => {
            for f in fields {
                prog.arena.drop_ref(*f);
            }
        }
        Value::Newtype(inner) => prog.arena.drop_ref(*inner),
        Value::Closure(env, _) => prog.arena.drop_ref(*env),
        Value::List { elems, .. } => {
            for e in elems {
                prog.arena.drop_ref(*e);
            }
        }
        Value::Map { pairs, .. } => {
            for (k, v) in pairs {
                prog.arena.drop_ref(*k);
                prog.arena.drop_ref(*v);
            }
        }
        Value::Set { elems, .. } => {
            for e in elems {
                prog.arena.drop_ref(*e);
            }
        }
        _ => {}
    }
}

/// Structural total order over acyclic arena values (the derived `Eq`/`Ord`).
/// Returns `< 0`, `== 0`, or `> 0`.
///
/// Leaves compare by value; a `Sum` by constructor index then payload; a
/// `Record` by field order; a `Newtype` by its inner value; `List`/`Set`
/// lexicographically over elements; `Map` lexicographically over the sorted
/// (key, value) pairs. Mixed kinds fall back to [`kind_rank`], so any two
/// acyclic values are comparable — this total cross-kind order is what makes
/// arbitrary-typed `Map` keys work.
///
/// `Closure` values are unordered — reaching one is a lowering bug (the type
/// checker rejects `Ord` over functions).
fn value_cmp(arena: &Arena, a: ArenaRef, b: ArenaRef) -> i8 {
    match (arena.get(a), arena.get(b)) {
        (Some(va), Some(vb)) => value_cmp_ref(arena, va, vb),
        _ => 0,
    }
}

fn value_cmp_ref(arena: &Arena, a: &Value, b: &Value) -> i8 {
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
        (Closure(..), Closure(..)) => 0, // unreachable for Ord-typed keys
        _ => kind_rank(a).cmp(&kind_rank(b)) as i8,
    }
}

fn cmp_ref_slices(arena: &Arena, xs: &[ArenaRef], ys: &[ArenaRef]) -> i8 {
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
        Closure(..) => 10,
        Void => 11,
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

/// Read a scalar value as `f64`: a Float reads directly, an Int widens.
/// Non-numeric values (records, sums, closures, `Void`) read as `None`.
fn value_to_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// Execute one value-track arithmetic instruction: read the two operand values
/// as scalars (an unbound or non-numeric operand reads as `0.0`), compute the
/// Float result, and store it as a freshly allocated slot in `dst`.
///
/// The operands are read BEFORE any `dst` occupant is released, so `dst` may
/// alias an operand (`dst == a` or `dst == b`) without losing its value.
/// Lowering emits SSA registers (dst is always fresh), but the aliasing-safe
/// order keeps hand-written IR correct. The old occupant, when present, is
/// taken out of the register and queued, so the per-tick register clear cannot
/// drop it a second time.
fn exec_value_arith<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    op: ValueArith,
    dst: usize,
    a: usize,
    b: usize,
    drops: &mut Vec<ArenaRef>,
) {
    let av = prog
        .value_regs
        .get(a)
        .copied()
        .flatten()
        .and_then(|r| match prog.arena.get(r) {
            Some(v) => value_to_f64(v),
            None => None,
        })
        .unwrap_or(0.0);
    let bv = prog
        .value_regs
        .get(b)
        .copied()
        .flatten()
        .and_then(|r| match prog.arena.get(r) {
            Some(v) => value_to_f64(v),
            None => None,
        })
        .unwrap_or(0.0);
    if let Some(r) = prog.value_regs.get_mut(dst).and_then(|r| r.take()) {
        drops.push(r);
    }
    let result = match op {
        ValueArith::Add => av + bv,
        ValueArith::Sub => av - bv,
        ValueArith::Mul => av * bv,
        ValueArith::Div => av / bv,
    };
    prog.value_regs[dst] = alloc_owned(prog, Value::Float(result));
}

fn exec_value_instr<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    instr: &ValueInstr,
    drops: &mut Vec<ArenaRef>,
) {
    match instr {
        ValueInstr::ValuePushScope => {
            prog.cell_stack.push(Vec::new());
        }
        ValueInstr::ValuePopScope => {
            // Frames are empty in v1 (cells are owned by their register); the
            // drop collection below is the future-proof hook for scoped cells.
            if let Some(frame) = prog.cell_stack.pop() {
                for (_, cell) in frame {
                    drops.push(cell);
                }
            }
        }
        ValueInstr::ValueBindCell { dst } => {
            // A cell is a fresh slot holding the cell's value directly; the
            // register owns the cell ref.
            prog.value_regs[*dst] = alloc_owned(prog, Value::Void);
        }
        ValueInstr::ValueReadCell { dst, cell } => {
            // Inside a fragment (`frag_cells_base < frag_cells.len()`), `cell`
            // is a CAPTURE index into the current call's env frame — the
            // lambda's free variables, resolved at `frag_cells_base + cell`.
            // Otherwise `cell` is a value register holding a cell ref
            // (main-track reads).
            let capture = if prog.frag_cells_base < prog.frag_cells.len() {
                Some(prog.frag_cells[prog.frag_cells_base + *cell])
            } else {
                None
            };
            match capture.or_else(|| prog.value_regs.get(*cell).copied().flatten()) {
                Some(cr) => match prog.arena.get(cr) {
                    Some(v) => {
                        // Copy the value OUT of the cell into a fresh slot: a
                        // read result is a new owner, it does not share. An
                        // uninitialised (Void) cell reads as 0.0.
                        let out = if matches!(v, Value::Void) {
                            Value::Float(0.0)
                        } else {
                            v.clone()
                        };
                        prog.value_regs[*dst] = alloc_copy(prog, &out);
                    }
                    None => prog.value_regs[*dst] = None,
                },
                None => prog.value_regs[*dst] = None,
            }
        }
        ValueInstr::ValueReadMainCell { dst, cell } => {
            // Read a persistent main λ-parameter cell (see `ReadMainCell` in the
            // block track). The result is a new owner; a `Void` cell reads 0.0.
            match prog.main_cells[*cell] {
                Some(cr) => match prog.arena.get(cr) {
                    Some(v) => {
                        let out = if matches!(v, Value::Void) {
                            Value::Float(0.0)
                        } else {
                            v.clone()
                        };
                        prog.value_regs[*dst] = alloc_copy(prog, &out);
                    }
                    None => prog.value_regs[*dst] = None,
                },
                _ => prog.value_regs[*dst] = None,
            }
        }
        ValueInstr::ValueWriteCell { cell, src } => {
            if let (Some(cr), Some(sr)) = (prog.value_regs[*cell], prog.value_regs[*src]) {
                let sv = prog.arena.get(sr).cloned();
                if let Some(v) = sv {
                    // The cell holds its value: allocate a fresh slot with a
                    // copy, then release the previous cell (a cell is
                    // re-assignable). The src register keeps its own ref —
                    // this is a copy, not a move.
                    let new_cell = alloc_copy(prog, &v);
                    prog.arena.drop_ref(cr);
                    prog.value_regs[*cell] = new_cell;
                }
            }
        }
        ValueInstr::ValueConstInt { dst, value } => {
            prog.value_regs[*dst] = alloc_owned(prog, Value::Int(*value));
        }
        ValueInstr::ValueConstFloat { dst, value } => {
            prog.value_regs[*dst] = alloc_owned(prog, Value::Float(*value));
        }
        ValueInstr::ValueAdd { dst, a, b } => {
            exec_value_arith(prog, ValueArith::Add, *dst, *a, *b, drops)
        }
        ValueInstr::ValueSub { dst, a, b } => {
            exec_value_arith(prog, ValueArith::Sub, *dst, *a, *b, drops)
        }
        ValueInstr::ValueMul { dst, a, b } => {
            exec_value_arith(prog, ValueArith::Mul, *dst, *a, *b, drops)
        }
        ValueInstr::ValueDiv { dst, a, b } => {
            exec_value_arith(prog, ValueArith::Div, *dst, *a, *b, drops)
        }
        ValueInstr::ValueConstructRecord { dst, fields } => {
            prog.value_regs[*dst] = if let Some(refs) = read_field_refs(prog, fields) {
                alloc_owned(prog, Value::Record(refs))
            } else {
                None
            };
        }
        ValueInstr::ValueConstructSum { dst, ctor, payload } => {
            prog.value_regs[*dst] = if let Some(refs) = read_field_refs(prog, payload) {
                alloc_owned(prog, Value::Sum(*ctor, refs))
            } else {
                None
            };
        }
        ValueInstr::ValueProject { dst, slot, field } => {
            match prog.value_regs[*slot] {
                Some(rec) => {
                    let fref: Option<ArenaRef> = match prog.arena.get(rec) {
                        Some(Value::Record(fields)) => fields.get(*field).cloned(),
                        _ => None,
                    };
                    // A project result is a new owner: share the field ref.
                    match fref {
                        Some(fr) => prog.value_regs[*dst] = copy_owned(prog, fr),
                        None => prog.value_regs[*dst] = None,
                    }
                }
                None => prog.value_regs[*dst] = None,
            }
        }
        ValueInstr::ValueUpdateField { slot, field, src } => {
            match (prog.value_regs[*slot], prog.value_regs[*src]) {
                (Some(rec), Some(sr)) => {
                    // COW: if the record is shared, mutate gives us a private
                    // copy; the slot register takes the (possibly new) ref.
                    let new_rec = match prog.arena.mutate(rec) {
                        Ok(r) => r,
                        Err(_) => {
                            prog.value_regs[*slot] = None;
                            return;
                        }
                    };
                    // Snapshot the old field ref and count the source ref
                    // before touching the record, so the field write below is
                    // a clean replacement.
                    let old_field: Option<ArenaRef> = match prog.arena.get(new_rec) {
                        Some(Value::Record(fields)) => fields.get(*field).cloned(),
                        _ => None,
                    };
                    let new_field = copy_owned(prog, sr);
                    let wrote = match new_field {
                        Some(nf) => match prog.arena.get_mut(new_rec) {
                            Some(Value::Record(fields)) => {
                                fields[*field] = nf;
                                true
                            }
                            _ => {
                                // The slot held no record after all: release
                                // the counted ref and fail the update.
                                prog.arena.drop_ref(nf);
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
                            prog.arena.drop_ref(of);
                        }
                        prog.value_regs[*slot] = Some(new_rec);
                    } else {
                        prog.value_regs[*slot] = None;
                    }
                }
                _ => prog.value_regs[*slot] = None,
            }
        }
        ValueInstr::ValueNewtype { dst, src } => {
            match prog.value_regs[*src] {
                // The newtype owns its inner value; the src register keeps its
                // own ref — copy first so both hold a counted ref.
                Some(sr) => {
                    let copied = copy_owned(prog, sr);
                    match copied {
                        Some(c) => prog.value_regs[*dst] = alloc_owned(prog, Value::Newtype(c)),
                        None => prog.value_regs[*dst] = None,
                    }
                }
                None => prog.value_regs[*dst] = None,
            }
        }
        ValueInstr::ValueUnwrap { dst, src } => {
            match prog.value_regs[*src] {
                Some(sr) => {
                    let inner: Option<ArenaRef> = match prog.arena.get(sr) {
                        Some(Value::Newtype(inner)) => Some(*inner),
                        _ => None,
                    };
                    // The unwrapped result is a new owner: share the inner ref.
                    match inner {
                        Some(i) => prog.value_regs[*dst] = copy_owned(prog, i),
                        None => prog.value_regs[*dst] = None,
                    }
                }
                None => prog.value_regs[*dst] = None,
            }
        }
        ValueInstr::ValueCallFunc {
            dst,
            closure_slot,
            args,
        } => {
            // Runtime dispatch: read the closure from `closure_slot`, run the
            // referenced fragment with a temporary register frame, and copy the
            // result into `dst`. An unbound slot or a non-closure value yields a
            // detectable `None`.
            let slot = prog.value_regs.get(*closure_slot).copied().flatten();
            match slot.and_then(|s| prog.arena.get(s)) {
                Some(Value::Closure(env_ref, fragment_id)) => {
                    // Share the fragment via `Arc` so `prog` can be borrowed
                    // mutably inside `run_fragment` without cloning the body
                    // (an atomic refcount bump, no heap allocation on the RT
                    // path). The `Arc::clone` ends the immutable IR borrow.
                    match prog.ir.fragments.get(*fragment_id as usize).cloned() {
                        Some(frag) => run_fragment(prog, &frag, *env_ref, args, dst, drops),
                        None => prog.value_regs[*dst] = None,
                    }
                }
                _ => prog.value_regs[*dst] = None,
            }
        }
        ValueInstr::ValueMakeClosure { dst, env, fragment } => {
            // A closure is a leaf holding the env record ref and the fragment
            // id. The env register is read by value; the closure COUNTS the env
            // as a child (like a Record counts its fields), so the creating
            // register keeps its own ownership and `drop_ref` releases the env
            // when the closure is freed. An unbound env (v1 named references)
            // falls back to a fresh Void slot the closure owns outright.
            let env_ref = match prog.value_regs.get(*env).copied().flatten() {
                Some(r) => match prog.arena.copy(r) {
                    Ok(c) => c,
                    Err(_) => {
                        prog.value_regs[*dst] = None;
                        return;
                    }
                },
                None => match prog.arena.alloc(Value::Void) {
                    Ok(v) => v,
                    Err(_) => {
                        prog.value_regs[*dst] = None;
                        return;
                    }
                },
            };
            prog.value_regs[*dst] = alloc_owned(prog, Value::Closure(env_ref, *fragment as u32));
        }
        ValueInstr::ValueCopy { dst, src } => match prog.value_regs[*src] {
            Some(sr) => {
                let copied = copy_owned(prog, sr);
                prog.value_regs[*dst] = copied;
            }
            None => prog.value_regs[*dst] = None,
        },
        ValueInstr::ValueDrop { src } => {
            if let Some(r) = prog.value_regs[*src].take() {
                drops.push(r);
            }
        }
        ValueInstr::ValueStateRead { dst, slot } => {
            match prog.value_state.get(*slot) {
                // The read result is a new owner of the stored value.
                Some(Some(stored)) => {
                    let sr = *stored;
                    prog.value_regs[*dst] = copy_owned(prog, sr);
                }
                // An empty state slot reads as 0.0.
                _ => prog.value_regs[*dst] = alloc_owned(prog, Value::Float(0.0)),
            }
        }
        ValueInstr::ValueStateWrite { slot, src } => {
            if let Some(sr) = prog.value_regs[*src] {
                // The state slot owns its value: copy the src ref, then
                // release the previous tick's value and store the fresh one.
                let stored = copy_owned(prog, sr);
                if let Some(old) = prog.value_state[*slot].take() {
                    prog.arena.drop_ref(old);
                }
                prog.value_state[*slot] = stored;
            }
        }
        ValueInstr::ValueMatch { dst, slot, ctor } => {
            // v1 static dispatch: the scrutinee's constructor is statically
            // known (a literal sum), so exactly one arm matches. A non-matching
            // arm (or an unbound scrutinee) writes None into every dst reg — a
            // detectable no-op. Each dst reg owns its payload ref via a copy
            // (rc++); the scrutinee keeps its own ownership of the sum.
            let payload: Option<Vec<ArenaRef>> = match prog.value_regs.get(*slot).copied().flatten()
            {
                Some(sr) => match prog.arena.get(sr) {
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
                            .and_then(|fr| prog.arena.copy(fr).ok());
                        if let Some(reg) = prog.value_regs.get_mut(*d) {
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
                        if let Some(reg) = prog.value_regs.get_mut(*d) {
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
            prog.value_regs[*dst] = alloc_owned(prog, Value::Bool(*value));
        }
        ValueInstr::ValueConstString { dst, value } => {
            prog.value_regs[*dst] = alloc_owned(prog, Value::String(value.clone()));
        }
        ValueInstr::ValueListLit { dst, elems, cap } => {
            prog.value_regs[*dst] = if let Some(refs) = read_field_refs(prog, elems) {
                alloc_owned(
                    prog,
                    Value::List {
                        elems: refs,
                        cap: *cap,
                    },
                )
            } else {
                None
            };
        }
        ValueInstr::ValueMapLit {
            dst,
            keys,
            vals,
            cap,
        } => {
            prog.value_regs[*dst] = if let (Some(ks), Some(vs)) =
                (read_field_refs(prog, keys), read_field_refs(prog, vals))
            {
                // Map entries are kept SORTED by the derived key order (the
                // insert/lookup/member binary search relies on it), so a map
                // literal written out of order is normalised here.
                let mut pairs: Vec<(ArenaRef, ArenaRef)> = ks.into_iter().zip(vs).collect();
                pairs.sort_by(|a, b| value_cmp(&prog.arena, a.0, b.0).cmp(&0));
                alloc_owned(prog, Value::Map { pairs, cap: *cap })
            } else {
                None
            };
        }
        ValueInstr::ValueCompare { dst, op, a, b } => {
            let res = match (prog.value_regs[*a], prog.value_regs[*b]) {
                (Some(x), Some(y)) => {
                    let c = value_cmp(&prog.arena, x, y);
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
                _ => None,
            };
            prog.value_regs[*dst] = res.and_then(|v| alloc_owned(prog, v));
        }
        ValueInstr::ValueLogic { dst, op, a, b } => {
            let res = match (prog.value_regs[*a], prog.value_regs[*b]) {
                (Some(x), Some(y)) => {
                    let (ax, ay) = match (prog.arena.get(x), prog.arena.get(y)) {
                        (Some(Value::Bool(p)), Some(Value::Bool(q))) => (*p, *q),
                        _ => (false, false),
                    };
                    Some(Value::Bool(match op {
                        LogicOp::And => ax && ay,
                        LogicOp::Or => ax || ay,
                    }))
                }
                _ => None,
            };
            prog.value_regs[*dst] = res.and_then(|v| alloc_owned(prog, v));
        }
        ValueInstr::ValueCallBuiltin { dst, op, args } => {
            exec_value_call_builtin(prog, *op, args, *dst, drops);
        }
    }
}

/// The sorted position at which `key` belongs among the sorted `keys`, via
/// binary search over the total `value_cmp` order — the first index whose key
/// is not less than `key` (a lower bound). Map/Set entries are kept sorted by
/// this derived order, so `insert` splices here, and `lookup`/`member` test
/// `value_cmp(key, keys[pos]) == 0` to distinguish hit from insert point.
fn sorted_insert_pos<T: Transcendental, const BUF: usize>(
    prog: &RillProgram<T, BUF>,
    key: ArenaRef,
    keys: &[ArenaRef],
) -> usize {
    let mut lo = 0;
    let mut hi = keys.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if value_cmp(&prog.arena, key, keys[mid]) > 0 {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Read an Int value register as a non-negative capacity (clamped), 0 when the
/// register holds no Int (a negative Int must not wrap into a huge `usize`).
fn int_cap_arg<T: Transcendental, const BUF: usize>(
    prog: &RillProgram<T, BUF>,
    reg: usize,
) -> usize {
    prog.value_regs
        .get(reg)
        .copied()
        .flatten()
        .and_then(|r| match prog.arena.get(r) {
            Some(Value::Int(n)) => Some((*n).max(0) as usize),
            _ => None,
        })
        .unwrap_or(0)
}

/// Dispatch a collection operation (`ValueCallBuiltin`).
///
/// Container reads copy (RC++) any child refs the result claims so both the
/// source and the new container own them. A capacity overflow latches a
/// `ProcessError` on `prog.value_error` instead of a silent `None` — the
/// register-`Option` channel is reserved for build-time arena exhaustion.
fn exec_value_call_builtin<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    op: ValueBuiltinOp,
    args: &[usize],
    dst: usize,
    drops: &mut Vec<ArenaRef>,
) {
    use ValueBuiltinOp::*;
    match op {
        Cons => {
            // cons x xs: build a new List whose head is x followed by the
            // source elems (`x : xs`). The result recounts (RC++) the source
            // elems and the new element so both the source container and the
            // result own them (the source register is dropped at tick end).
            let xs = prog.value_regs[args[1]].and_then(|r| prog.arena.get(r).cloned());
            match xs {
                Some(Value::List { mut elems, cap }) => {
                    if elems.len() >= cap {
                        prog.value_error = Some(ProcessError::processing("list capacity exceeded"));
                        return;
                    }
                    let x = prog.value_regs[args[0]].and_then(|r| prog.arena.copy(r).ok());
                    match x {
                        Some(xr) => {
                            // The result shares the source elems' refs: recount
                            // each so the source list keeps its own ownership
                            // until it is dropped and the result's refs survive.
                            for e in &elems {
                                _ = prog.arena.copy(*e);
                            }
                            elems.insert(0, xr);
                            prog.value_regs[dst] = alloc_owned(prog, Value::List { elems, cap });
                        }
                        None => prog.value_regs[dst] = None,
                    }
                }
                _ => prog.value_regs[dst] = None,
            }
        }
        Length => {
            let n = prog.value_regs[args[0]]
                .and_then(|r| prog.arena.get(r))
                .map(|v| match v {
                    Value::List { elems, .. } => elems.len() as i64,
                    _ => 0,
                })
                .unwrap_or(0);
            prog.value_regs[dst] = alloc_owned(prog, Value::Int(n));
        }
        Head => {
            let r = prog.value_regs[args[0]].and_then(|r| prog.arena.get(r).cloned());
            let m = match r {
                Some(Value::List { elems, .. }) => elems.first().copied(),
                _ => None,
            };
            prog.value_regs[dst] = match m {
                // `Just e`: the Sum owns a counted ref on the element (the list
                // keeps its own ownership). `Nothing`: an empty sum payload.
                Some(e) => match copy_owned(prog, e) {
                    Some(ce) => alloc_owned(prog, Value::Sum(0, vec![ce])),
                    None => None,
                },
                None => alloc_owned(prog, Value::Sum(1, vec![])),
            };
        }
        Map => {
            // map f xs: dispatch f per element via a one-arg closure call.
            let f = prog.value_regs[args[0]];
            let xs = prog.value_regs[args[1]].and_then(|r| prog.arena.get(r).cloned());
            if let (Some(_), Some(Value::List { elems, cap })) = (f, xs) {
                let mut out = Vec::with_capacity(elems.len());
                for e in &elems {
                    let out_e = call_closure_single(prog, args[0], *e, dst, drops);
                    if let Some(o) = out_e {
                        out.push(o);
                    }
                }
                prog.value_regs[dst] = alloc_owned(prog, Value::List { elems: out, cap });
            } else {
                prog.value_regs[dst] = None;
            }
        }
        Fold => {
            // fold f z xs: seed the accumulator with a copy of z, then for each
            // element call f with (acc, elem); the result becomes the new acc.
            let f = prog.value_regs[args[0]];
            let acc = prog.value_regs[args[1]];
            let xs = prog.value_regs[args[2]].and_then(|r| prog.arena.get(r).cloned());
            if let (Some(_), Some(accr), Some(Value::List { elems, .. })) = (f, acc, xs) {
                let mut cur = match copy_owned(prog, accr) {
                    Some(c) => c,
                    None => {
                        prog.value_regs[dst] = None;
                        return;
                    }
                };
                let mut failed = false;
                for e in &elems {
                    let pair = [cur, *e];
                    let next = call_closure_args(prog, args[0], &pair, dst, drops);
                    match next {
                        Some(n) => {
                            // Replace the previous accumulator copy with the
                            // fresh result: exactly one owner lives.
                            prog.arena.drop_ref(cur);
                            cur = n;
                        }
                        None => {
                            failed = true;
                            break;
                        }
                    }
                }
                if failed {
                    prog.arena.drop_ref(cur);
                    prog.value_regs[dst] = None;
                } else {
                    prog.value_regs[dst] = Some(cur);
                }
            } else {
                prog.value_regs[dst] = None;
            }
        }
        ListEmpty => {
            // list n: an empty List with capacity n. The capacity is read from
            // the runtime Int argument (`list 4`).
            let cap = int_cap_arg(prog, args[0]);
            prog.value_regs[dst] = alloc_owned(
                prog,
                Value::List {
                    elems: Vec::new(),
                    cap,
                },
            );
        }
        Tail => {
            // tail xs: the source without its head, capacity preserved. The
            // result claims elems[1..]: recount (RC++) every element the source
            // owns, allocate the tail list over the kept slice, then release
            // the extra count on the removed head — the source keeps its single
            // claim, and the head is never freed mid-tick (its slot cannot be
            // recycled while the source list still references it). An empty
            // list is its own tail.
            let xs = prog.value_regs[args[0]].and_then(|r| prog.arena.get(r).cloned());
            match xs {
                Some(Value::List { elems, cap }) => {
                    if elems.is_empty() {
                        prog.value_regs[dst] = alloc_owned(
                            prog,
                            Value::List {
                                elems: Vec::new(),
                                cap,
                            },
                        );
                        return;
                    }
                    for e in &elems {
                        _ = prog.arena.copy(*e);
                    }
                    let kept: Vec<ArenaRef> = elems[1..].to_vec();
                    let head = elems[0];
                    prog.value_regs[dst] = alloc_owned(prog, Value::List { elems: kept, cap });
                    prog.arena.drop_ref(head);
                }
                _ => prog.value_regs[dst] = None,
            }
        }
        Filter => {
            // filter p xs: keep the elements for which the unary predicate
            // returns Bool(true). A kept element is recounted (RC++) into the
            // result list so both the source and the result own it; the
            // predicate's Bool result is released after each call.
            let p = prog.value_regs[args[0]];
            let xs = prog.value_regs[args[1]].and_then(|r| prog.arena.get(r).cloned());
            if let (Some(_), Some(Value::List { elems, cap })) = (p, xs) {
                let mut kept: Vec<ArenaRef> = Vec::with_capacity(elems.len());
                for e in &elems {
                    let pred = call_closure_single(prog, args[0], *e, dst, drops);
                    let is_keep = match pred {
                        Some(pr) => match prog.arena.get(pr) {
                            Some(Value::Bool(b)) => *b,
                            _ => false,
                        },
                        None => false,
                    };
                    if is_keep {
                        if let Ok(c) = prog.arena.copy(*e) {
                            kept.push(c);
                        }
                    }
                    if let Some(pr) = pred {
                        prog.arena.drop_ref(pr);
                    }
                }
                prog.value_regs[dst] = alloc_owned(prog, Value::List { elems: kept, cap });
            } else {
                prog.value_regs[dst] = None;
            }
        }
        InsertMap => {
            // insert k v m: COW-insert (k, v) into the sorted map, or
            // COW-replace the value when an equal key already exists. The
            // capacity is a strict bound: a NEW key past capacity latches a
            // runtime `ProcessError`. Every ref the result map claims is
            // recounted (RC++) so the source keeps its own ownership.
            let k = prog.value_regs[args[0]];
            let v = prog.value_regs[args[1]];
            let m = prog.value_regs[args[2]].and_then(|r| prog.arena.get(r).cloned());
            if let (Some(kr), Some(vr), Some(Value::Map { pairs, cap })) = (k, v, m) {
                let keys: Vec<ArenaRef> = pairs.iter().map(|(pk, _)| *pk).collect();
                let pos = sorted_insert_pos(prog, kr, &keys);
                let dup = pos < pairs.len() && value_cmp(&prog.arena, kr, pairs[pos].0) == 0;
                if dup {
                    // Replace-on-duplicate: recount every pair ref, swap in the
                    // new value (counted once for the map), and release the
                    // replaced value's extra count — the source keeps its own
                    // claim, so the replaced value is never freed mid-tick.
                    let mut new_pairs = pairs.clone();
                    for (pk, pv) in &new_pairs {
                        _ = prog.arena.copy(*pk);
                        _ = prog.arena.copy(*pv);
                    }
                    let old_val = new_pairs[pos].1;
                    new_pairs[pos].1 = vr;
                    _ = prog.arena.copy(vr);
                    prog.arena.drop_ref(old_val);
                    prog.value_regs[dst] = alloc_owned(
                        prog,
                        Value::Map {
                            pairs: new_pairs,
                            cap,
                        },
                    );
                } else if pairs.len() >= cap {
                    prog.value_error = Some(ProcessError::processing("map capacity exceeded"));
                } else {
                    // Insert at the sorted position: splice (k, v) into the
                    // cloned pair list, then recount every key and value the
                    // new map claims (the source and the result each own them).
                    let mut new_pairs: Vec<(ArenaRef, ArenaRef)> =
                        Vec::with_capacity(pairs.len() + 1);
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
                        _ = prog.arena.copy(*pk);
                        _ = prog.arena.copy(*pv);
                    }
                    prog.value_regs[dst] = alloc_owned(
                        prog,
                        Value::Map {
                            pairs: new_pairs,
                            cap,
                        },
                    );
                }
            } else {
                prog.value_regs[dst] = None;
            }
        }
        Lookup => {
            // lookup k m: binary search the sorted map for the key; `Just v`
            // when found, `Nothing` otherwise (the same Sum encoding as
            // `head`). The found value ref is counted (RC++) so the Sum owns
            // it independently of the map.
            let k = prog.value_regs[args[0]];
            let m = prog.value_regs[args[1]].and_then(|r| prog.arena.get(r).cloned());
            if let (Some(kr), Some(Value::Map { pairs, .. })) = (k, m) {
                let keys: Vec<ArenaRef> = pairs.iter().map(|(pk, _)| *pk).collect();
                let pos = sorted_insert_pos(prog, kr, &keys);
                if pos < pairs.len() && value_cmp(&prog.arena, kr, pairs[pos].0) == 0 {
                    match copy_owned(prog, pairs[pos].1) {
                        Some(cv) => {
                            prog.value_regs[dst] = alloc_owned(prog, Value::Sum(0, vec![cv]))
                        }
                        None => prog.value_regs[dst] = None,
                    }
                } else {
                    prog.value_regs[dst] = alloc_owned(prog, Value::Sum(1, vec![]));
                }
            } else {
                prog.value_regs[dst] = None;
            }
        }
        Member => {
            // member k c: a shared op — the container's variant tells map
            // membership (among the sorted keys) from set membership (among
            // the sorted elements). A non-container reads as `false`.
            let k = prog.value_regs[args[0]];
            let c = prog.value_regs[args[1]].and_then(|r| prog.arena.get(r).cloned());
            if let (Some(kr), Some(cv)) = (k, c) {
                let found = match cv {
                    Value::Map { pairs, .. } => {
                        let keys: Vec<ArenaRef> = pairs.iter().map(|(pk, _)| *pk).collect();
                        let pos = sorted_insert_pos(prog, kr, &keys);
                        pos < pairs.len() && value_cmp(&prog.arena, kr, pairs[pos].0) == 0
                    }
                    Value::Set { elems, .. } => {
                        let pos = sorted_insert_pos(prog, kr, &elems);
                        pos < elems.len() && value_cmp(&prog.arena, kr, elems[pos]) == 0
                    }
                    _ => false,
                };
                prog.value_regs[dst] = alloc_owned(prog, Value::Bool(found));
            } else {
                prog.value_regs[dst] = None;
            }
        }
        InsertSet => {
            // insert k s: COW-insert the element into the sorted set; a
            // duplicate leaves the set unchanged (a fresh COW copy). The
            // capacity is a strict bound: a NEW element past capacity latches
            // a runtime `ProcessError`.
            let k = prog.value_regs[args[0]];
            let s = prog.value_regs[args[1]].and_then(|r| prog.arena.get(r).cloned());
            if let (Some(kr), Some(Value::Set { elems, cap })) = (k, s) {
                let pos = sorted_insert_pos(prog, kr, &elems);
                let dup = pos < elems.len() && value_cmp(&prog.arena, kr, elems[pos]) == 0;
                if dup {
                    let new_elems = elems.clone();
                    for e in &new_elems {
                        _ = prog.arena.copy(*e);
                    }
                    prog.value_regs[dst] = alloc_owned(
                        prog,
                        Value::Set {
                            elems: new_elems,
                            cap,
                        },
                    );
                } else if elems.len() >= cap {
                    prog.value_error = Some(ProcessError::processing("set capacity exceeded"));
                } else {
                    let mut new_elems: Vec<ArenaRef> = Vec::with_capacity(elems.len() + 1);
                    for (i, e) in elems.iter().enumerate() {
                        if i == pos {
                            new_elems.push(kr);
                        }
                        new_elems.push(*e);
                    }
                    if new_elems.len() == elems.len() {
                        new_elems.push(kr);
                    }
                    for e in &new_elems {
                        _ = prog.arena.copy(*e);
                    }
                    prog.value_regs[dst] = alloc_owned(
                        prog,
                        Value::Set {
                            elems: new_elems,
                            cap,
                        },
                    );
                }
            } else {
                prog.value_regs[dst] = None;
            }
        }
        MapEmpty => {
            // empty_map n: an empty Map with capacity n (the strict bound for
            // later inserts).
            let cap = int_cap_arg(prog, args[0]);
            prog.value_regs[dst] = alloc_owned(
                prog,
                Value::Map {
                    pairs: Vec::new(),
                    cap,
                },
            );
        }
        SetEmpty => {
            // empty_set n: an empty Set with capacity n.
            let cap = int_cap_arg(prog, args[0]);
            prog.value_regs[dst] = alloc_owned(
                prog,
                Value::Set {
                    elems: Vec::new(),
                    cap,
                },
            );
        }
    }
}

/// Bind `elem` as the fragment's single argument register, run the closure, and
/// return the copied result ref (or `None` when the slot holds no closure).
///
/// Thin wrapper over [`call_closure_args`].
fn call_closure_single<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    closure_reg: usize,
    elem: ArenaRef,
    result_reg: usize,
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    call_closure_args(
        prog,
        closure_reg,
        std::slice::from_ref(&elem),
        result_reg,
        drops,
    )
}

/// Bind `args` as a closure call's argument registers and run the fragment,
/// returning the copied result ref (or `None` on any unbound/errored step).
///
/// The arg refs are placed in the pre-allocated call-scratch slots as raw
/// borrows — `run_fragment` copies each (RC++) into the same slots before
/// running and drains them on return, so the source containers keep their own
/// ownership and the borrow never leaks. `result_reg` is a program-level
/// register (below the scratch watermark), so the drained range cannot reclaim
/// it; its previous occupant is dropped first, because `run_fragment`
/// overwrites `dst` without releasing the old ref.
fn call_closure_args<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    closure_reg: usize,
    args: &[ArenaRef],
    result_reg: usize,
    drops: &mut Vec<ArenaRef>,
) -> Option<ArenaRef> {
    let (env_ref, fragment_id) = match prog.value_regs.get(closure_reg).copied().flatten() {
        Some(r) => match prog.arena.get(r) {
            Some(Value::Closure(env, fid)) => (*env, *fid),
            _ => return None,
        },
        None => return None,
    };
    let frag = prog.ir.fragments.get(fragment_id as usize).cloned()?;
    let base = prog.value_regs_top;
    // Bounds-guard: the pre-sized call scratch reserves `max_call_regs` slots,
    // so a wrong-arity closure (e.g. `fold (fn a -> a) ...`) could pass more
    // args than the fragment declares and write the raw borrows past the store.
    // Clamp to `frag.sig.value_ins` and the store's tail; the type checker
    // rejects wrong-arity closures at compile time, this is belt-and-suspenders
    // so a bad closure cannot crash the RT path.
    let n = args
        .len()
        .min(frag.sig.value_ins)
        .min(prog.value_regs.len().saturating_sub(base));
    for (i, e) in args.iter().take(n).enumerate() {
        prog.value_regs[base + i] = Some(*e);
    }
    if let Some(old) = prog.value_regs.get_mut(result_reg).and_then(|r| r.take()) {
        drops.push(old);
    }
    let arg_regs: Vec<usize> = (0..n).map(|i| base + i).collect();
    run_fragment(prog, &frag, env_ref, &arg_regs, &result_reg, drops);
    prog.value_regs.get_mut(result_reg).and_then(|r| r.take())
}

/// Execute a function fragment: binds the captured env fields as cells in a
/// temporary frame, runs the fragment's value instructions against a scratch
/// register slice in the pre-allocated tail of `value_regs`, copies the result
/// into `dst`, and pops the frame.
///
/// Register-offset scheme: a fragment's instructions are reused across calls
/// and reference fragment-local registers `0..num_value_regs`. The interpreter
/// records `base = value_regs_top`, runs each instruction with every register
/// field offset by `base` ([`remap_value_instr`]), and unwinds the watermark —
/// the store is pre-sized to `num_value_regs + max_call_regs` at construction,
/// so no growth or reallocation happens on the RT path. Value args are copied
/// (RC++) into the fragment's leading registers; the caller keeps its own
/// ownership.
///
/// Capture scheme: the env Record's fields (the lambda's free variables, in
/// declaration order) become cells in the shared `frag_cells` store at the
/// current top; `frag_cells_base` marks the active frame so the fragment's
/// `ValueReadCell { cell: i }` resolves to `frag_cells[frag_cells_base + i]`.
/// A capture cell holds an INDEPENDENT copy of the field's value, so the cell
/// owns it and the env record is untouched. Deferred drops queue on the
/// pre-sized `drops` scratch threaded from the value track — every nesting
/// depth reuses the same reserved buffer.
fn run_fragment<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    frag: &std::sync::Arc<FragmentIr>,
    env_ref: crate::arena::ArenaRef,
    args: &[usize],
    dst: &usize,
    drops: &mut Vec<crate::arena::ArenaRef>,
) {
    // 1. Bind each env Record field as a capture cell: an INDEPENDENT copy of
    //    the field's value appended to the shared capture-cell store (pre-sized
    //    at construction — no allocation). The env record is untouched.
    let cell_base = prog.frag_cells.len();
    if let Some(Value::Record(fields)) = prog.arena.get(env_ref) {
        // Clone the field refs so the arena can be borrowed mutably while the
        // capture cells are allocated below.
        let fields = fields.clone();
        for f in &fields {
            // Clone the field value so the arena borrow ends before the mutable
            // alloc_copy below (mirrors the outer `fields.clone()`).
            let cell = match prog.arena.get(*f).cloned() {
                Some(v) => alloc_copy(prog, &v),
                None => prog.arena.alloc(Value::Void).ok(),
            };
            if let Some(c) = cell {
                prog.frag_cells.push(c);
            }
        }
    }
    let saved_cells_base = prog.frag_cells_base;
    prog.frag_cells_base = cell_base;
    // 2. Borrow the pre-allocated call-scratch slice for the fragment's local
    //    registers. The watermark grows into the reserved tail; the pre-sized
    //    store guarantees `max_call_regs` slots are available for any nesting.
    let base = prog.value_regs_top;
    let top = base + frag.num_value_regs;
    debug_assert!(
        top <= prog.value_regs.len(),
        "call scratch exceeds pre-allocated max_call_regs (lowering bound broken)"
    );
    prog.value_regs_top = top;
    // 3. Value args: copy the caller's arg values into the fragment's leading
    //    registers (the first `sig.value_ins` slice slots). Each copy is a
    //    fresh owner (RC++) so the caller keeps its own ref.
    for (i, a) in args
        .iter()
        .take(frag.sig.value_ins.min(args.len()))
        .enumerate()
    {
        if let Some(sr) = prog.value_regs.get(*a).copied().flatten() {
            prog.value_regs[base + i] = copy_owned(prog, sr);
        }
    }
    // 4. Run the fragment's value instructions with the register offset.
    //    Drops queue on the threaded scratch above this call's mark and are
    //    drained below, so a nested dispatch's drops never collide.
    let drops_mark = drops.len();
    for instr in &frag.value_instrs {
        let remapped = remap_value_instr(instr, base);
        exec_value_instr(prog, &remapped, drops);
    }
    for r in drops.drain(drops_mark..) {
        prog.arena.drop_ref(r);
    }
    // 5. Copy the fragment's result into `dst` (a fresh owner via `copy`).
    if let Some(or) = frag.output_value_regs.first() {
        prog.value_regs[*dst] = match prog.value_regs.get(base + *or).copied().flatten() {
            Some(sr) => copy_owned(prog, sr),
            None => None,
        };
    }
    // 6. Drain the scratch slice (dropping every fragment-local register's
    //    counted ref — value args and body temps own arena slots, so removing
    //    the slots without `drop_ref` would leak them on EVERY call and exhaust
    //    the fixed arena across ticks), unwind the watermark, release the
    //    capture cells, and restore the caller's frame.
    for i in base..prog.value_regs_top {
        if let Some(r) = prog.value_regs[i].take() {
            prog.arena.drop_ref(r);
        }
    }
    prog.value_regs_top = base;
    prog.frag_cells_base = saved_cells_base;
    for c in prog.frag_cells.drain(cell_base..) {
        prog.arena.drop_ref(c);
    }
}

/// Clone `instr`, adding `base` to every fragment-local register field so the
/// instruction runs against the scratch slice appended by [`run_fragment`].
///
/// Only true value-register fields are offset. Store indices that are global to
/// the program — `ValueReadMainCell::cell` (main-cell store), `ValueStateRead`/
/// `ValueStateWrite` `slot` (value-state store) — and non-register constants
/// (`value`, `ctor`, `field`, `fragment`) are left untouched.
fn remap_value_instr(instr: &ValueInstr, base: usize) -> ValueInstr {
    match instr {
        ValueInstr::ValuePushScope => ValueInstr::ValuePushScope,
        ValueInstr::ValuePopScope => ValueInstr::ValuePopScope,
        ValueInstr::ValueBindCell { dst } => ValueInstr::ValueBindCell { dst: dst + base },
        ValueInstr::ValueReadCell { dst, cell } => ValueInstr::ValueReadCell {
            // `cell` is a capture index into the call's env frame, NOT a
            // fragment-local register — leave it unoffset (a main-track read
            // of a cell register never reaches remap; only fragment instrs are
            // remapped, and a fragment's ReadCell cells are always captures).
            dst: dst + base,
            cell: *cell,
        },
        ValueInstr::ValueReadMainCell { dst, cell } => ValueInstr::ValueReadMainCell {
            dst: dst + base,
            cell: *cell,
        },
        ValueInstr::ValueWriteCell { cell, src } => ValueInstr::ValueWriteCell {
            cell: cell + base,
            src: src + base,
        },
        ValueInstr::ValueConstInt { dst, value } => ValueInstr::ValueConstInt {
            dst: dst + base,
            value: *value,
        },
        ValueInstr::ValueConstFloat { dst, value } => ValueInstr::ValueConstFloat {
            dst: dst + base,
            value: *value,
        },
        ValueInstr::ValueAdd { dst, a, b } => ValueInstr::ValueAdd {
            dst: dst + base,
            a: a + base,
            b: b + base,
        },
        ValueInstr::ValueSub { dst, a, b } => ValueInstr::ValueSub {
            dst: dst + base,
            a: a + base,
            b: b + base,
        },
        ValueInstr::ValueMul { dst, a, b } => ValueInstr::ValueMul {
            dst: dst + base,
            a: a + base,
            b: b + base,
        },
        ValueInstr::ValueDiv { dst, a, b } => ValueInstr::ValueDiv {
            dst: dst + base,
            a: a + base,
            b: b + base,
        },
        ValueInstr::ValueConstructRecord { dst, fields } => ValueInstr::ValueConstructRecord {
            dst: dst + base,
            fields: fields.iter().map(|f| f + base).collect(),
        },
        ValueInstr::ValueConstructSum { dst, ctor, payload } => ValueInstr::ValueConstructSum {
            dst: dst + base,
            ctor: *ctor,
            payload: payload.iter().map(|p| p + base).collect(),
        },
        ValueInstr::ValueProject { dst, slot, field } => ValueInstr::ValueProject {
            dst: dst + base,
            slot: slot + base,
            field: *field,
        },
        ValueInstr::ValueUpdateField { slot, field, src } => ValueInstr::ValueUpdateField {
            slot: slot + base,
            field: *field,
            src: src + base,
        },
        ValueInstr::ValueNewtype { dst, src } => ValueInstr::ValueNewtype {
            dst: dst + base,
            src: src + base,
        },
        ValueInstr::ValueUnwrap { dst, src } => ValueInstr::ValueUnwrap {
            dst: dst + base,
            src: src + base,
        },
        ValueInstr::ValueCallFunc {
            dst,
            closure_slot,
            args,
        } => ValueInstr::ValueCallFunc {
            dst: dst + base,
            closure_slot: closure_slot + base,
            args: args.iter().map(|a| a + base).collect(),
        },
        ValueInstr::ValueMakeClosure { dst, env, fragment } => ValueInstr::ValueMakeClosure {
            dst: dst + base,
            env: env + base,
            fragment: *fragment,
        },
        ValueInstr::ValueCopy { dst, src } => ValueInstr::ValueCopy {
            dst: dst + base,
            src: src + base,
        },
        ValueInstr::ValueDrop { src } => ValueInstr::ValueDrop { src: src + base },
        ValueInstr::ValueStateRead { dst, slot } => ValueInstr::ValueStateRead {
            dst: dst + base,
            slot: *slot,
        },
        ValueInstr::ValueStateWrite { slot, src } => ValueInstr::ValueStateWrite {
            slot: *slot,
            src: src + base,
        },
        ValueInstr::ValueMatch { dst, slot, ctor } => ValueInstr::ValueMatch {
            dst: dst.iter().map(|d| d + base).collect(),
            slot: slot + base,
            ctor: *ctor,
        },
        ValueInstr::ValueBool { dst, value } => ValueInstr::ValueBool {
            dst: dst + base,
            value: *value,
        },
        ValueInstr::ValueConstString { dst, value } => ValueInstr::ValueConstString {
            dst: dst + base,
            value: value.clone(),
        },
        ValueInstr::ValueListLit { dst, elems, cap } => ValueInstr::ValueListLit {
            dst: dst + base,
            elems: elems.iter().map(|e| e + base).collect(),
            cap: *cap,
        },
        ValueInstr::ValueMapLit {
            dst,
            keys,
            vals,
            cap,
        } => ValueInstr::ValueMapLit {
            dst: dst + base,
            keys: keys.iter().map(|k| k + base).collect(),
            vals: vals.iter().map(|v| v + base).collect(),
            cap: *cap,
        },
        ValueInstr::ValueCompare { dst, op, a, b } => ValueInstr::ValueCompare {
            dst: dst + base,
            op: *op,
            a: a + base,
            b: b + base,
        },
        ValueInstr::ValueLogic { dst, op, a, b } => ValueInstr::ValueLogic {
            dst: dst + base,
            op: *op,
            a: a + base,
            b: b + base,
        },
        ValueInstr::ValueCallBuiltin { dst, op, args } => ValueInstr::ValueCallBuiltin {
            dst: dst + base,
            op: *op,
            args: args.iter().map(|a| a + base).collect(),
        },
    }
}

/// Execute a single whole-buffer instruction.
fn exec_block_op<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    idx: usize,
    inputs: &[&[T]],
    n: usize,
) {
    match &prog.ir.instrs[idx] {
        Instr::Const { dst, value } => {
            let v = T::from_f64(*value);
            prog.block_regs[*dst][..n].fill(v);
        }
        Instr::LoadInput { dst, index } => {
            let reg = &mut prog.block_regs[*dst];
            match inputs.get(*index) {
                Some(buf) => {
                    let m = buf.len().min(n);
                    reg[..m].copy_from_slice(&buf[..m]);
                    for v in &mut reg[m..n] {
                        *v = T::ZERO;
                    }
                }
                None => {
                    for v in &mut reg[..n] {
                        *v = T::ZERO;
                    }
                }
            }
        }
        Instr::ReadBlockState { dst, slot } => {
            prog.block_regs[*dst][..n].copy_from_slice(&prog.block_state[*slot][..n]);
        }
        Instr::ReadDelay { dst, line } => {
            prog.delays[*line].read_block(&mut prog.block_regs[*dst][..n]);
        }
        Instr::Move { dst, src } => {
            let mut scratch = [T::ZERO; BUF];
            scratch[..n].copy_from_slice(&prog.block_regs[*src][..n]);
            prog.block_regs[*dst][..n].copy_from_slice(&scratch[..n]);
        }
        Instr::Un { dst, op, src } => {
            let mut scratch = [T::ZERO; BUF];
            scratch[..n].copy_from_slice(&prog.block_regs[*src][..n]);
            apply_un_slice(*op, &scratch[..n], &mut prog.block_regs[*dst][..n]);
        }
        Instr::Bin { dst, op, a, b } => {
            let mut sa = [T::ZERO; BUF];
            let mut sb = [T::ZERO; BUF];
            sa[..n].copy_from_slice(&prog.block_regs[*a][..n]);
            sb[..n].copy_from_slice(&prog.block_regs[*b][..n]);
            apply_bin_slice(*op, &sa[..n], &sb[..n], &mut prog.block_regs[*dst][..n]);
        }
        Instr::WriteBlockState { slot, src } => {
            prog.block_state_next[*slot][..n].copy_from_slice(&prog.block_regs[*src][..n]);
        }
        Instr::WriteDelay { line, src } => {
            prog.delays[*line].write_block(&prog.block_regs[*src][..n]);
        }
        Instr::ReadParam { dst, idx } => {
            let v = T::from_f64(param_to_f64(&prog.params[*idx]));
            prog.block_regs[*dst][..n].fill(v);
        }
        Instr::ReadActorParam { dst, param_idx } => {
            let v = T::from_f64(param_to_f64(&prog.params[*param_idx]));
            prog.block_regs[*dst][..n].fill(v);
        }
        Instr::ReadMainCell { dst, cell } => {
            // Materialise the persistent main λ-parameter cell's float value.
            // The cell was allocated at construction and survives across ticks,
            // so a value written by `SetParameter` on the control thread is
            // read here on every subsequent block. A `Void` (unset) cell is 0.0.
            let v = match prog.main_cells[*cell] {
                Some(r) => match prog.arena.get(r) {
                    Some(crate::arena::Value::Float(f)) => *f,
                    _ => 0.0,
                },
                _ => 0.0,
            };
            prog.block_regs[*dst][..n].fill(T::from_f64(v));
        }
        Instr::CallBlock { .. } => {
            unreachable!("block built-in scheduled as a block op (should be ForeignBlock)")
        }
        #[cfg(feature = "debug")]
        Instr::ProbePoint { dst, src, .. } => {
            let mut scratch = [T::ZERO; BUF];
            scratch[..n].copy_from_slice(&prog.block_regs[*src][..n]);
            prog.block_regs[*dst][..n].copy_from_slice(&scratch[..n]);
        }
    }
}

/// Execute a whole-buffer foreign built-in (opaque `Algorithm`).
fn exec_foreign_block<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    idx: usize,
    n: usize,
) {
    if let Instr::CallBlock {
        dst: first_dst,
        srcs,
        instance,
    } = &prog.ir.instrs[idx]
    {
        let bi = &prog.ir.builtins[*instance];
        let n_in = bi.signal_ins;
        let n_out = bi.signal_outs;
        assert!(
            n_in <= MAX_BUILTIN_CHANNELS && n_out <= MAX_BUILTIN_CHANNELS,
            "built-in '{0}' has {n_in}→{n_out} channels, exceeding MAX_BUILTIN_CHANNELS ({MAX_BUILTIN_CHANNELS})",
            bi.name
        );

        if n_in <= 1 && n_out == 1 {
            assert!(
                n_in == 0 || *first_dst != srcs[0],
                "ForeignBlock register aliasing: input reg {} == output reg {}.",
                srcs[0],
                *first_dst,
            );
            let mut scratch = [T::ZERO; BUF];
            let maybe_in = if n_in == 0 {
                None
            } else {
                scratch[..n].copy_from_slice(&prog.block_regs[srcs[0]][..n]);
                Some(&scratch[..n])
            };
            match &mut prog.builtins[*instance] {
                crate::program::BuiltinInst::Block(b) => {
                    let _ = b.process(maybe_in, &mut prog.block_regs[*first_dst][..n]);
                }
                crate::program::BuiltinInst::MultichannelBlock(_) => {
                    unreachable!("ForeignBlock fast path with multichannel builtin")
                }
            }
        } else {
            match &mut prog.builtins[*instance] {
                crate::program::BuiltinInst::MultichannelBlock(mb) => {
                    // Two-phase: snapshot all input channels into stack scratch
                    // (they may alias the destination registers), then write the
                    // outputs directly into the destination register store.
                    let mut in_bufs: [FixedBuffer<T, BUF>; MAX_BUILTIN_CHANNELS] =
                        std::array::from_fn(|_| FixedBuffer::new());
                    for ch in 0..n_in {
                        in_bufs[ch][..n].copy_from_slice(&prog.block_regs[srcs[ch]][..n]);
                    }
                    let input_refs: [&[T]; MAX_BUILTIN_CHANNELS] =
                        std::array::from_fn(|i| &in_bufs[i][..n]);
                    let dst_bufs = &mut prog.block_regs[*first_dst..*first_dst + n_out];
                    // Real output channels followed by zero-length dummies so the
                    // stack array always yields exactly MAX_BUILTIN_CHANNELS slots.
                    let mut empties: [[T; 0]; MAX_BUILTIN_CHANNELS] = std::array::from_fn(|_| []);
                    let mut it = dst_bufs
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
                }
                crate::program::BuiltinInst::Block(b) => {
                    // Interleaved path: a contiguous input/output pair, both on
                    // the stack (bounded by MAX_BUILTIN_CHANNELS). Nested arrays
                    // `[[T; BUF]; N]` are laid out contiguously, so the flattened
                    // view is a single `[T; BUF * N]` block.
                    let mut inp: [[T; BUF]; MAX_BUILTIN_CHANNELS] =
                        std::array::from_fn(|_| [T::ZERO; BUF]);
                    for (ch, &reg_idx) in srcs.iter().enumerate() {
                        inp[ch][..n].copy_from_slice(&prog.block_regs[reg_idx][..n]);
                    }
                    let mut out_buf: [[T; BUF]; MAX_BUILTIN_CHANNELS] =
                        std::array::from_fn(|_| [T::ZERO; BUF]);
                    let _ = b.process(
                        Some(&inp.as_flattened()[..n_in * n]),
                        &mut out_buf.as_flattened_mut()[..n_out * n],
                    );
                    for ch in 0..n_out {
                        let reg_idx = *first_dst + ch;
                        let start = ch * n;
                        prog.block_regs[reg_idx][..n]
                            .copy_from_slice(&out_buf.as_flattened()[start..start + n]);
                    }
                }
            }
        }
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
    }
}

#[cfg(test)]
mod closure_dispatch_tests {
    use crate::ir::{FragmentIr, FuncSig, Ir, ValueInstr, ValueLayout};
    use crate::program::RillProgram;
    use rill_core::traits::MultichannelAlgorithm;

    #[test]
    fn call_dispatch_runs_fragment() {
        // env reg 0 holds a Void cell (dummy env); MakeClosure binds it; the
        // fragment computes const int 7; CallFunc copies the result to the
        // output register.
        let ir = Ir {
            instrs: Vec::new(),
            num_regs: 0,
            output_regs: Vec::new(),
            num_inputs: 0,
            num_outputs: 0,
            state: Default::default(),
            builtins: Vec::new(),
            params: Vec::new(),
            num_main_cells: 0,
            value_instrs: vec![
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
            num_value_regs: 3,
            value_output_regs: vec![2],
            value_funcs: Vec::new(),
            max_call_regs: 2,
            value_state: ValueLayout {
                capacity: 16,
                value_state_slots: 0,
            },
            fragments: vec![std::sync::Arc::new(FragmentIr {
                value_instrs: vec![ValueInstr::ValueConstInt { dst: 0, value: 7 }],
                steps: Vec::new(),
                num_value_regs: 1,
                num_block_regs: 0,
                output_value_regs: vec![0],
                output_block_regs: Vec::new(),
                num_capture_cells: 0,
                sig: FuncSig {
                    value_ins: 0,
                    value_outs: 1,
                    signal_ins: 0,
                },
            })],
        };
        let mut prog = RillProgram::<f32, 256>::new(ir);
        let mut out = [0.0f32; 2];
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        let v = prog.value_outputs()[0].unwrap();
        assert_eq!(prog.arena().get(v).unwrap(), &crate::arena::Value::Int(7));
    }
}

#[cfg(test)]
mod value_track_tests {
    use super::*;
    use crate::ir::{Ir, StateLayout, ValueInstr, ValueLayout};
    use rill_core::traits::MultichannelAlgorithm;

    fn prog_with(
        value_instrs: Vec<ValueInstr>,
        num_value_regs: usize,
        value_state_slots: usize,
    ) -> RillProgram<f32, 256> {
        let ir = Ir {
            instrs: Vec::new(),
            num_regs: 0,
            output_regs: Vec::new(),
            num_inputs: 0,
            num_outputs: 0,
            state: StateLayout::default(),
            builtins: Vec::new(),
            params: Vec::new(),
            num_main_cells: 0,
            value_instrs,
            num_value_regs,
            value_output_regs: Vec::new(),
            value_funcs: Vec::new(),
            fragments: Vec::new(),
            max_call_regs: 0,
            value_state: ValueLayout {
                capacity: 16,
                value_state_slots,
            },
        };
        RillProgram::<f32, 256>::new(ir)
    }

    #[test]
    fn const_int_and_const_float_run_per_tick() {
        let mut prog = prog_with(
            vec![
                ValueInstr::ValueConstInt { dst: 0, value: 42 },
                ValueInstr::ValueConstFloat { dst: 1, value: 1.5 },
            ],
            2,
            0,
        );
        // Drive the value track directly (the full tick additionally clears
        // the per-tick registers at the end) so the registers and arena can be
        // inspected mid-tick.
        run_value_track(&mut prog).unwrap();
        assert_eq!(prog.value_regs[0], Some(0));
        let slot0 = prog.arena.get(0).unwrap().clone();
        assert_eq!(slot0, crate::arena::Value::Int(42));
        let slot1 = prog.arena.get(1).unwrap().clone();
        assert_eq!(slot1, crate::arena::Value::Float(1.5));
    }

    #[test]
    fn cell_stack_bind_read_write() {
        let mut prog = prog_with(
            vec![
                ValueInstr::ValuePushScope,
                ValueInstr::ValueConstInt { dst: 0, value: 7 },
                ValueInstr::ValueBindCell { dst: 1 },
                ValueInstr::ValueWriteCell { cell: 1, src: 0 },
                ValueInstr::ValueReadCell { dst: 2, cell: 1 },
                ValueInstr::ValuePopScope,
            ],
            3,
            0,
        );
        run_value_track(&mut prog).unwrap();
        let cell = prog.value_regs[1].unwrap();
        let val = prog.arena.get(cell).unwrap();
        assert_eq!(val, &crate::arena::Value::Int(7));
        let read = prog.value_regs[2].unwrap();
        assert_eq!(prog.arena.get(read).unwrap(), &crate::arena::Value::Int(7));
    }

    #[test]
    fn value_state_persists_one_tick() {
        // Value-state is a 1-tick delayed value: what tick 1 writes into slot 0
        // must be readable in tick 2. The read runs before the write, so slot 1
        // latches whatever the read saw; asserting it proves the delay.
        let mut prog = prog_with(
            vec![
                ValueInstr::ValueStateRead { dst: 1, slot: 0 },
                ValueInstr::ValueConstInt { dst: 0, value: 7 },
                ValueInstr::ValueStateWrite { slot: 0, src: 0 },
                ValueInstr::ValueStateWrite { slot: 1, src: 1 },
            ],
            2,
            2,
        );
        let mut out = [0.0f32; 2];
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        // Tick 1: the read saw the empty state (0.0); slot 0 was then written 7
        // and must survive until the next tick.
        assert_eq!(
            prog.arena.get(prog.value_state[0].unwrap()).unwrap(),
            &crate::arena::Value::Int(7)
        );
        assert_eq!(
            prog.arena.get(prog.value_state[1].unwrap()).unwrap(),
            &crate::arena::Value::Float(0.0)
        );
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        // Tick 2: the read saw tick 1's 7 (the 1-tick delay) and latched it
        // into slot 1.
        assert_eq!(
            prog.arena.get(prog.value_state[1].unwrap()).unwrap(),
            &crate::arena::Value::Int(7)
        );
    }

    #[test]
    fn value_regs_are_cleared_per_tick() {
        // Value registers are per-tick scratch: after a tick they must be
        // cleared and their refs released, so a multi-tick value program cannot
        // exhaust the fixed arena (one leaked slot per register per tick).
        let mut prog = prog_with(
            vec![
                ValueInstr::ValueConstInt { dst: 0, value: 42 },
                ValueInstr::ValueConstFloat { dst: 1, value: 1.5 },
            ],
            2,
            0,
        );
        let mut out = [0.0f32; 4];
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        assert_eq!(prog.value_regs[0], None);
        assert_eq!(prog.value_regs[1], None);
        assert_eq!(prog.arena.live(), 0);
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        assert_eq!(prog.arena.live(), 0, "second tick leaks nothing");
    }

    #[test]
    fn value_arith_computes_float_result() {
        // Int and Float operands widen to f64; the result is always Float.
        let mut prog = prog_with(
            vec![
                ValueInstr::ValueConstInt { dst: 0, value: 3 },
                ValueInstr::ValueConstFloat { dst: 1, value: 2.0 },
                ValueInstr::ValueAdd { dst: 2, a: 0, b: 1 },
                ValueInstr::ValueMul { dst: 3, a: 1, b: 2 },
                ValueInstr::ValueDiv { dst: 4, a: 3, b: 1 },
            ],
            5,
            0,
        );
        run_value_track(&mut prog).unwrap();
        let get = |r: usize| prog.arena.get(prog.value_regs[r].unwrap()).unwrap().clone();
        assert_eq!(get(2), crate::arena::Value::Float(5.0), "3 + 2.0");
        assert_eq!(get(3), crate::arena::Value::Float(10.0), "2.0 * 5.0");
        assert_eq!(get(4), crate::arena::Value::Float(5.0), "10.0 / 2.0");
    }

    #[test]
    fn value_arith_reassignment_drops_old_occupant() {
        // dst 0 is reused: the previous Int(42) ref must be released (deferred
        // drop), not leaked — the arena holds exactly the two live slots.
        let mut prog = prog_with(
            vec![
                ValueInstr::ValueConstInt { dst: 0, value: 42 },
                ValueInstr::ValueConstFloat { dst: 1, value: 2.0 },
                ValueInstr::ValueMul { dst: 0, a: 0, b: 1 },
            ],
            2,
            0,
        );
        run_value_track(&mut prog).unwrap();
        assert_eq!(
            prog.arena.get(prog.value_regs[0].unwrap()).unwrap(),
            &crate::arena::Value::Float(84.0)
        );
        assert_eq!(prog.arena.live(), 2, "old dst occupant must be dropped");
    }

    #[test]
    fn list_empty_clamps_negative_int_capacity() {
        // A negative Int capacity must clamp to 0, not wrap into a huge `usize`.
        let mut prog = prog_with(
            vec![
                ValueInstr::ValueConstInt { dst: 0, value: -3 },
                ValueInstr::ValueCallBuiltin {
                    dst: 1,
                    op: ValueBuiltinOp::ListEmpty,
                    args: vec![0],
                },
            ],
            2,
            0,
        );
        run_value_track(&mut prog).unwrap();
        match prog.arena.get(prog.value_regs[1].unwrap()).unwrap() {
            crate::arena::Value::List { cap, .. } => assert_eq!(*cap, 0),
            other => panic!("expected a List, got {other:?}"),
        }
    }

    #[test]
    fn value_match_selects_payload_of_matching_ctor() {
        // Sum(0, [7]) matched against ctor 0: dst reg 2 receives a shared ref to
        // the payload (rc++), so both the sum and the dst reg own it.
        let mut prog = prog_with(
            vec![
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
            3,
            0,
        );
        run_value_track(&mut prog).unwrap();
        let payload = prog.value_regs[2].unwrap();
        assert_eq!(
            prog.arena.get(payload).unwrap(),
            &crate::arena::Value::Int(7)
        );
        let sum = prog.value_regs[1].unwrap();
        assert_eq!(prog.arena.rc(sum), 1);
        assert_eq!(
            prog.arena.rc(payload),
            3,
            "const reg + sum payload + match dst each own it"
        );
    }

    #[test]
    fn value_match_non_matching_ctor_writes_none() {
        // Sum(1, [7]) matched against ctor 0: no arm matches, so the dst reg is
        // written None (a detectable no-op under v1 static dispatch).
        let mut prog = prog_with(
            vec![
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
            3,
            0,
        );
        run_value_track(&mut prog).unwrap();
        assert_eq!(prog.value_regs[2], None);
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

    fn int_list(a: &mut Arena, len: usize) -> ArenaRef {
        let elems: Vec<ArenaRef> = (0..len).map(|_| a.alloc(Value::Int(0)).unwrap()).collect();
        a.alloc(Value::List { elems, cap: len }).unwrap()
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
        let l1 = a
            .alloc(Value::List {
                elems: vec![f1],
                cap: 2,
            })
            .unwrap();
        let l12 = a
            .alloc(Value::List {
                elems: vec![f1, f2],
                cap: 2,
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
                cap: 1,
            })
            .unwrap();
        let m_a2 = a
            .alloc(Value::Map {
                pairs: vec![(ka, f2)],
                cap: 1,
            })
            .unwrap();
        let m_b1 = a
            .alloc(Value::Map {
                pairs: vec![(kb, f1)],
                cap: 1,
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
