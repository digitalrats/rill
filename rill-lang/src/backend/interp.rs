//! IR evaluators: the reference per-sample interpreter and the hybrid
//! block/sample executor.

use rill_core::math::vector::ScalarVector4;
use rill_core::math::Transcendental;

use crate::ir::{BinArith, Instr, UnOp};
use crate::program::RillProgram;
use crate::schedule::Step;

// ============================================================================
// Reference (per-sample) interpreter — numerical oracle, MVP behavior.
// ============================================================================

/// Run one block sample-by-sample using the scalar `f64` register file.
pub fn run_block_reference<T: Transcendental>(
    prog: &mut RillProgram<T>,
    input: Option<&[T]>,
    output: &mut [T],
) {
    let n = output.len();
    for i in 0..n {
        let in_sample = match input {
            Some(buf) if i < buf.len() => buf[i].to_f64(),
            _ => 0.0,
        };
        let y = eval_sample_scalar(prog, in_sample);
        output[i] = T::from_f64(y);
    }
}

fn eval_sample_scalar<T: Transcendental>(prog: &mut RillProgram<T>, in0: f64) -> f64 {
    for idx in 0..prog.ir.instrs.len() {
        match prog.ir.instrs[idx].clone() {
            Instr::Const { dst, value } => prog.regs_scalar[dst] = value,
            Instr::LoadInput { dst, index } => {
                prog.regs_scalar[dst] = if index == 0 { in0 } else { 0.0 };
            }
            Instr::ReadState { dst, slot } => prog.regs_scalar[dst] = prog.state[slot],
            Instr::ReadDelay { dst, line } => prog.regs_scalar[dst] = prog.delays[line].read(),
            Instr::Move { dst, src } => prog.regs_scalar[dst] = prog.regs_scalar[src],
            Instr::Un { dst, op, src } => {
                let x = prog.regs_scalar[src];
                prog.regs_scalar[dst] = apply_un_f64(op, x);
            }
            Instr::Bin { dst, op, a, b } => {
                let x = prog.regs_scalar[a];
                let y = prog.regs_scalar[b];
                prog.regs_scalar[dst] = apply_bin_f64(op, x, y);
            }
            Instr::WriteState { slot, src } => prog.state_next[slot] = prog.regs_scalar[src],
            Instr::WriteDelay { line, src } => {
                let v = prog.regs_scalar[src];
                prog.delays[line].write(v);
            }
        }
    }
    for (s, nx) in prog.state.iter_mut().zip(prog.state_next.iter()) {
        *s = *nx;
    }
    prog.regs_scalar[prog.ir.output_reg]
}

fn apply_un_f64(op: UnOp, x: f64) -> f64 {
    match op {
        UnOp::Neg => -x,
        UnOp::Abs => x.abs(),
        UnOp::Sin => x.sin(),
        UnOp::Cos => x.cos(),
        UnOp::Tan => x.tan(),
        UnOp::Sqrt => x.sqrt(),
        UnOp::Exp => x.exp(),
        UnOp::Ln => x.ln(),
        UnOp::Tanh => x.tanh(),
    }
}

fn apply_bin_f64(op: BinArith, x: f64, y: f64) -> f64 {
    match op {
        BinArith::Add => x + y,
        BinArith::Sub => x - y,
        BinArith::Mul => x * y,
        BinArith::Div => x / y,
        BinArith::Rem => x % y,
        BinArith::Min => x.min(y),
        BinArith::Max => x.max(y),
    }
}

// ============================================================================
// Hybrid (block + sample-region) executor.
// ============================================================================

/// Run one block via the schedule: block steps whole-buffer, sample regions
/// per sample. All registers are computed in `T`.
pub fn run_block_hybrid<T: Transcendental>(
    prog: &mut RillProgram<T>,
    input: Option<&[T]>,
    output: &mut [T],
) {
    let n = output.len();
    prog.ensure_block_len(n);

    // Move the step list out of `prog` so we can borrow `prog`'s registers
    // mutably while iterating. `mem::take` leaves an empty `Vec` behind — no
    // allocation on the RT path — and we move the list back at the end.
    let steps = std::mem::take(&mut prog.schedule.steps);
    for step in &steps {
        match step {
            Step::Block(idx) => exec_block_op(prog, *idx, input, n),
            Step::Sample(instrs) => exec_sample_region(prog, instrs, input, n),
        }
    }
    prog.schedule.steps = steps;

    let out_reg = prog.ir.output_reg;
    output[..n].copy_from_slice(&prog.block_regs[out_reg][..n]);
}

/// Execute a single combinational instruction over the whole `[..n]` buffer.
fn exec_block_op<T: Transcendental>(
    prog: &mut RillProgram<T>,
    idx: usize,
    input: Option<&[T]>,
    n: usize,
) {
    match prog.ir.instrs[idx].clone() {
        Instr::Const { dst, value } => {
            let v = T::from_f64(value);
            prog.block_regs[dst][..n].fill(v);
        }
        Instr::LoadInput { dst, index } => {
            let reg = &mut prog.block_regs[dst];
            if index == 0 {
                if let Some(buf) = input {
                    let m = buf.len().min(n);
                    reg[..m].copy_from_slice(&buf[..m]);
                    for v in &mut reg[m..n] {
                        *v = T::ZERO;
                    }
                } else {
                    for v in &mut reg[..n] {
                        *v = T::ZERO;
                    }
                }
            } else {
                for v in &mut reg[..n] {
                    *v = T::ZERO;
                }
            }
        }
        Instr::Move { dst, src } => {
            // dst != src (SSA); move src out to satisfy the borrow checker.
            let mut tmp = std::mem::take(&mut prog.block_regs[dst]);
            tmp[..n].copy_from_slice(&prog.block_regs[src][..n]);
            prog.block_regs[dst] = tmp;
        }
        Instr::Un { dst, op, src } => {
            let mut out = std::mem::take(&mut prog.block_regs[dst]);
            apply_un_slice(op, &prog.block_regs[src][..n], &mut out[..n]);
            prog.block_regs[dst] = out;
        }
        Instr::Bin { dst, op, a, b } => {
            let mut out = std::mem::take(&mut prog.block_regs[dst]);
            apply_bin_slice(
                op,
                &prog.block_regs[a][..n],
                &prog.block_regs[b][..n],
                &mut out[..n],
            );
            prog.block_regs[dst] = out;
        }
        // Stateful instrs never appear as a Block step.
        Instr::ReadState { .. }
        | Instr::WriteState { .. }
        | Instr::ReadDelay { .. }
        | Instr::WriteDelay { .. } => {
            unreachable!("stateful instruction scheduled as a block op")
        }
    }
}

/// Execute a recurrent region per sample, indexing the shared block store.
#[allow(clippy::needless_range_loop)]
fn exec_sample_region<T: Transcendental>(
    prog: &mut RillProgram<T>,
    instrs: &[usize],
    input: Option<&[T]>,
    n: usize,
) {
    for i in 0..n {
        for &idx in instrs {
            match prog.ir.instrs[idx].clone() {
                Instr::Const { dst, value } => prog.block_regs[dst][i] = T::from_f64(value),
                Instr::LoadInput { dst, index } => {
                    let v = if index == 0 {
                        match input {
                            Some(buf) if i < buf.len() => buf[i],
                            _ => T::ZERO,
                        }
                    } else {
                        T::ZERO
                    };
                    prog.block_regs[dst][i] = v;
                }
                Instr::ReadState { dst, slot } => {
                    prog.block_regs[dst][i] = T::from_f64(prog.state[slot]);
                }
                Instr::ReadDelay { dst, line } => {
                    prog.block_regs[dst][i] = T::from_f64(prog.delays[line].read());
                }
                Instr::Move { dst, src } => {
                    prog.block_regs[dst][i] = prog.block_regs[src][i];
                }
                Instr::Un { dst, op, src } => {
                    let x = prog.block_regs[src][i];
                    prog.block_regs[dst][i] = apply_un_t(op, x);
                }
                Instr::Bin { dst, op, a, b } => {
                    let x = prog.block_regs[a][i];
                    let y = prog.block_regs[b][i];
                    prog.block_regs[dst][i] = apply_bin_t(op, x, y);
                }
                Instr::WriteState { slot, src } => {
                    prog.state_next[slot] = prog.block_regs[src][i].to_f64();
                }
                Instr::WriteDelay { line, src } => {
                    let v = prog.block_regs[src][i].to_f64();
                    prog.delays[line].write(v);
                }
            }
        }
        for (s, nx) in prog.state.iter_mut().zip(prog.state_next.iter()) {
            *s = *nx;
        }
    }
}

// ---- T-typed scalar ops (sample regions) ----

fn apply_un_t<T: Transcendental>(op: UnOp, x: T) -> T {
    match op {
        UnOp::Neg => T::ZERO - x,
        UnOp::Abs => x.abs(),
        UnOp::Sin => x.sin(),
        UnOp::Cos => x.cos(),
        UnOp::Tan => x.tan(),
        UnOp::Sqrt => x.sqrt(),
        UnOp::Exp => x.exp(),
        UnOp::Ln => x.ln(),
        UnOp::Tanh => x.tanh(),
    }
}

fn apply_bin_t<T: Transcendental>(op: BinArith, x: T, y: T) -> T {
    match op {
        BinArith::Add => x + y,
        BinArith::Sub => x - y,
        BinArith::Mul => x * y,
        BinArith::Div => x / y,
        BinArith::Rem => x % y,
        BinArith::Min => x.min(y),
        BinArith::Max => x.max(y),
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
    use rill_core::math::vector::ops::{add_slices, div_slices, mul_slices, sub_slices};
    match op {
        BinArith::Add => add_slices::<T, 4, ScalarVector4<T>>(a, b, out),
        BinArith::Sub => sub_slices::<T, 4, ScalarVector4<T>>(a, b, out),
        BinArith::Mul => mul_slices::<T, 4, ScalarVector4<T>>(a, b, out),
        BinArith::Div => div_slices::<T, 4, ScalarVector4<T>>(a, b, out),
        BinArith::Min => min_slice::<T, 4, ScalarVector4<T>>(a, b, out),
        BinArith::Max => max_slice::<T, 4, ScalarVector4<T>>(a, b, out),
        BinArith::Rem => {
            for ((o, &x), &y) in out.iter_mut().zip(a.iter()).zip(b.iter()) {
                *o = x % y;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::lexer::tokenize;
    use crate::lower::lower;
    use crate::parser::parse;
    use crate::program::RillProgram;
    use crate::types::infer::infer_program;
    use rill_core::traits::Algorithm;

    fn build(src: &str) -> RillProgram<f32> {
        let p = parse(&tokenize(src).unwrap()).unwrap();
        let tp = infer_program(&p).unwrap();
        let ir = lower(&tp).unwrap();
        RillProgram::<f32>::new(ir)
    }

    #[test]
    fn hybrid_gain_halves_input() {
        let mut prog = build("process = _ * 0.5;");
        let mut out = [0.0f32; 4];
        prog.process(Some(&[1.0, 2.0, 4.0, 8.0]), &mut out).unwrap();
        assert_eq!(out, [0.5, 1.0, 2.0, 4.0]);
    }

    #[test]
    fn hybrid_integrator_accumulates() {
        let mut prog = build("process = + ~ _;");
        let mut out = [0.0f32; 4];
        prog.process(Some(&[1.0, 1.0, 1.0, 1.0]), &mut out).unwrap();
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn hybrid_one_sample_delay() {
        let mut prog = build("process = _ @ 1;");
        let mut out = [0.0f32; 3];
        prog.process(Some(&[5.0, 7.0, 9.0]), &mut out).unwrap();
        assert_eq!(out, [0.0, 5.0, 7.0]);
    }

    #[test]
    fn hybrid_split_merge_doubles() {
        let mut prog = build("process = _ <: (_ , _) :> + ;");
        let mut out = [0.0f32; 2];
        prog.process(Some(&[1.0, 3.0]), &mut out).unwrap();
        assert_eq!(out, [2.0, 6.0]);
    }

    #[test]
    fn hybrid_matches_reference_on_mixed_program() {
        let mut a = build("process = (_ * 0.5) : (+ ~ (_ * 0.5));");
        let mut b = build("process = (_ * 0.5) : (+ ~ (_ * 0.5));");
        let input: Vec<f32> = (0..32).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut oa = vec![0.0f32; input.len()];
        let mut ob = vec![0.0f32; input.len()];
        a.process(Some(&input), &mut oa).unwrap();
        b.process_reference(Some(&input), &mut ob).unwrap();
        for (x, y) in oa.iter().zip(ob.iter()) {
            assert!((x - y).abs() < 1e-4, "hybrid {x} vs reference {y}");
        }
    }
}
