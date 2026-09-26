//! IR evaluator: the block-only executor.

use rill_core::buffer::FixedBuffer;
use rill_core::math::vector::ScalarVector4;
use rill_core::math::Transcendental;
use rill_core::traits::MultichannelAlgorithm;

use crate::arena::{ArenaRef, Value};
use crate::ir::{BinArith, FragmentIr, Instr, UnOp, ValueInstr};
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
/// Supports N inputs → M outputs.
pub fn run_block_mimo<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    inputs: &[&[T]],
    outputs: &mut [&mut [T]],
) {
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

    // Value-track phase (per-tick): allocate/free the program's values.
    run_value_track(prog);

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
pub(crate) fn run_value_track<T: Transcendental, const BUF: usize>(prog: &mut RillProgram<T, BUF>) {
    let mut drops: Vec<ArenaRef> = Vec::new();
    // Move the instruction list out of `prog` so we can borrow `prog`'s value
    // registers mutably while iterating (`mem::take` leaves an empty `Vec`
    // behind — no allocation on the RT path).
    let value_instrs = std::mem::take(&mut prog.ir.value_instrs);
    for instr in &value_instrs {
        exec_value_instr(prog, instr, &mut drops);
    }
    prog.ir.value_instrs = value_instrs;
    for r in drops {
        prog.arena.drop_ref(r);
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
/// register is a detectable no-op in release.
fn alloc_owned<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    v: Value,
) -> Option<ArenaRef> {
    let children: Vec<ArenaRef> = match v {
        Value::Record(ref fields) | Value::Sum(_, ref fields) => fields.clone(),
        Value::Newtype(inner) => vec![inner],
        Value::Closure(env, _) => vec![env],
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
        _ => {}
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
            // Inside a fragment, `cell < active_fragment_cells.len()` is a
            // CAPTURE index into the current call's env frame (the lambda's
            // free variables); otherwise `cell` is a value register holding a
            // cell ref (main-track reads).
            let capture = if !prog.active_fragment_cells.is_empty() {
                (*cell < prog.active_fragment_cells.len())
                    .then_some(prog.active_fragment_cells[*cell])
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
                    // Clone the fragment out of the IR so `prog` can be borrowed
                    // mutably inside `run_fragment` (the immutable IR borrow ends
                    // once the owned copy is in hand).
                    match prog.ir.fragments.get(*fragment_id as usize).cloned() {
                        Some(frag) => run_fragment(prog, &frag, *env_ref, args, dst),
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
    }
}

/// Execute a function fragment: binds the captured env fields as cells in a
/// temporary frame, runs the fragment's value instructions against a scratch
/// register slice appended to `value_regs`, copies the result into `dst`, and
/// pops the frame.
///
/// Register-offset scheme: a fragment's instructions are reused across calls
/// and reference fragment-local registers `0..num_value_regs`. The interpreter
/// records `base = value_regs.len()`, appends `num_value_regs` empty slots,
/// runs each instruction with every register field offset by `base`
/// ([`remap_value_instr`]), then truncates back — the fragment is never
/// rewritten. Value args are copied (RC++) into the fragment's leading
/// registers; the caller keeps its own ownership.
///
/// Capture scheme: the env Record's fields (the lambda's free variables, in
/// declaration order) become cells in the temporary frame at indices
/// `0..n-1`. The fragment's `ValueReadCell { cell: i }` reads frame cell `i`
/// via `active_fragment_cells`. A capture cell holds an INDEPENDENT copy of
/// the field's value, so the cell owns it and the env record is untouched.
fn run_fragment<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
    frag: &FragmentIr,
    env_ref: crate::arena::ArenaRef,
    args: &[usize],
    dst: &usize,
) {
    // 1. Push a temporary capture frame: each env Record field becomes a cell
    //    holding an INDEPENDENT COPY of the field's value (alloc_copy — the
    //    cell owns it, the env record is untouched). The cells are exposed as
    //    `active_fragment_cells` so the fragment's `ValueReadCell { cell: i }`
    //    capture reads resolve to frame cell i.
    prog.cell_stack.push(Vec::new());
    let mut cells: Vec<crate::arena::ArenaRef> = Vec::new();
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
                if let Some(frame) = prog.cell_stack.last_mut() {
                    frame.push((0, c));
                }
                cells.push(c);
            }
        }
    }
    // 2. Expose the capture cells for the duration of this call. Save/restore
    //    so a nested fragment call observes its own cells, not the caller's.
    let saved_cells = std::mem::take(&mut prog.active_fragment_cells);
    prog.active_fragment_cells = cells;
    // 3. Append a scratch register slice for the fragment's local registers.
    let base = prog.value_regs.len();
    prog.value_regs.extend(vec![None; frag.num_value_regs]);
    // 4. Value args: copy the caller's arg values into the fragment's leading
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
    // 5. Run the fragment's value instructions with the register offset.
    let mut drops: Vec<crate::arena::ArenaRef> = Vec::new();
    for instr in &frag.value_instrs {
        let remapped = remap_value_instr(instr, base);
        exec_value_instr(prog, &remapped, &mut drops);
    }
    for r in drops {
        prog.arena.drop_ref(r);
    }
    // 6. Copy the fragment's result into `dst` (a fresh owner via `copy`).
    if let Some(or) = frag.output_value_regs.first() {
        prog.value_regs[*dst] = match prog.value_regs.get(base + *or).copied().flatten() {
            Some(sr) => copy_owned(prog, sr),
            None => None,
        };
    }
    // 7. Drain the scratch slice (dropping every fragment-local register's
    //    counted ref — value args and body temps own arena slots, so removing
    //    the slots without `drop_ref` would leak them on EVERY call and exhaust
    //    the fixed arena across ticks), restore the caller's capture cells, and
    //    pop the frame (releasing the capture cells' counted refs).
    for r in prog.value_regs.drain(base..) {
        let Some(r) = r else { continue };
        prog.arena.drop_ref(r);
    }
    prog.active_fragment_cells = saved_cells;
    if let Some(frame) = prog.cell_stack.pop() {
        for (_, c) in frame {
            prog.arena.drop_ref(c);
        }
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
            value_state: ValueLayout {
                capacity: 16,
                value_state_slots: 0,
            },
            fragments: vec![FragmentIr {
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
            }],
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
        run_value_track(&mut prog);
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
        run_value_track(&mut prog);
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
        run_value_track(&mut prog);
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
        run_value_track(&mut prog);
        assert_eq!(
            prog.arena.get(prog.value_regs[0].unwrap()).unwrap(),
            &crate::arena::Value::Float(84.0)
        );
        assert_eq!(prog.arena.live(), 2, "old dst occupant must be dropped");
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
        run_value_track(&mut prog);
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
        run_value_track(&mut prog);
        assert_eq!(prog.value_regs[2], None);
    }
}
