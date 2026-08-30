//! IR evaluator: the block-only executor.

use rill_core::math::vector::ScalarVector4;
use rill_core::math::Transcendental;
use rill_core::traits::MultichannelAlgorithm;

use crate::ir::{BinArith, Instr, UnOp};
use crate::program::RillProgram;
use crate::schedule::Step;

fn param_to_f64(pv: &rill_core::traits::ParamValue) -> f64 {
    match pv {
        rill_core::traits::ParamValue::Float(v) => *v as f64,
        rill_core::traits::ParamValue::Int(v) => *v as f64,
        _ => 0.0,
    }
}

pub(crate) fn push_builtin_params<T: Transcendental>(prog: &mut RillProgram<T>) {
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
pub fn run_block_mimo<T: Transcendental>(
    prog: &mut RillProgram<T>,
    inputs: &[&[T]],
    outputs: &mut [&mut [T]],
) {
    push_builtin_params(prog);
    let n = outputs.first().map(|o| o.len()).unwrap_or(0);
    prog.ensure_block_len(n);

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

    // Apply the block-level feedback shadow copy (double-buffer swap).
    prog.swap_block_state();

    for (i, out) in outputs.iter_mut().enumerate() {
        if let Some(&reg) = prog.ir.output_regs.get(i) {
            let m = out.len().min(n);
            out[..m].copy_from_slice(&prog.block_regs[reg][..m]);
        }
    }
}

/// Execute a single whole-buffer instruction.
fn exec_block_op<T: Transcendental>(
    prog: &mut RillProgram<T>,
    idx: usize,
    inputs: &[&[T]],
    n: usize,
) {
    match prog.ir.instrs[idx].clone() {
        Instr::Const { dst, value } => {
            let v = T::from_f64(value);
            prog.block_regs[dst][..n].fill(v);
        }
        Instr::LoadInput { dst, index } => {
            let reg = &mut prog.block_regs[dst];
            match inputs.get(index) {
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
            prog.block_regs[dst][..n].copy_from_slice(&prog.block_state[slot][..n]);
        }
        Instr::ReadDelay { dst, line } => {
            prog.delays[line].read_block(&mut prog.block_regs[dst][..n]);
        }
        Instr::Move { dst, src } => {
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
        Instr::WriteBlockState { slot, src } => {
            prog.block_state_next[slot][..n].copy_from_slice(&prog.block_regs[src][..n]);
        }
        Instr::WriteDelay { line, src } => {
            prog.delays[line].write_block(&prog.block_regs[src][..n]);
        }
        Instr::ReadParam { dst, idx } => {
            let v = T::from_f64(param_to_f64(&prog.params[idx]));
            prog.block_regs[dst][..n].fill(v);
        }
        Instr::ReadActorParam { dst, param_idx } => {
            let v = T::from_f64(param_to_f64(&prog.params[param_idx]));
            prog.block_regs[dst][..n].fill(v);
        }
        Instr::CallBlock { .. } => {
            unreachable!("block built-in scheduled as a block op (should be ForeignBlock)")
        }
        #[cfg(feature = "debug")]
        Instr::ProbePoint { dst, src, .. } => {
            let mut tmp = std::mem::take(&mut prog.block_regs[dst]);
            tmp[..n].copy_from_slice(&prog.block_regs[src][..n]);
            prog.block_regs[dst] = tmp;
        }
    }
}

/// Execute a whole-buffer foreign built-in (opaque `Algorithm`).
fn exec_foreign_block<T: Transcendental>(prog: &mut RillProgram<T>, idx: usize, n: usize) {
    if let Instr::CallBlock {
        dst: first_dst,
        srcs,
        instance,
    } = prog.ir.instrs[idx].clone()
    {
        let bi = &prog.ir.builtins[instance];
        let n_in = bi.signal_ins;
        let n_out = bi.signal_outs;

        if n_in <= 1 && n_out == 1 {
            assert!(
                n_in == 0 || first_dst != srcs[0],
                "ForeignBlock register aliasing: input reg {} == output reg {}.",
                srcs[0],
                first_dst,
            );
            let mut out = std::mem::take(&mut prog.block_regs[first_dst]);
            let maybe_in = if n_in == 0 {
                None
            } else {
                Some(&prog.block_regs[srcs[0]][..n] as &[T])
            };
            match &mut prog.builtins[instance] {
                crate::program::BuiltinInst::Block(b) => {
                    let _ = b.process(maybe_in, &mut out[..n]);
                }
                crate::program::BuiltinInst::MultichannelBlock(_) => {
                    unreachable!("ForeignBlock fast path with multichannel builtin")
                }
            }
            prog.block_regs[first_dst] = out;
        } else {
            match &mut prog.builtins[instance] {
                crate::program::BuiltinInst::MultichannelBlock(mb) => {
                    let inputs: Vec<&[T]> = (0..n_in)
                        .map(|ch| &prog.block_regs[srcs[ch]][..n])
                        .collect();
                    let mut out_bufs: Vec<Vec<T>> = (0..n_out).map(|_| vec![T::ZERO; n]).collect();
                    let mut out_slices: Vec<&mut [T]> =
                        out_bufs.iter_mut().map(|v| v.as_mut_slice()).collect();
                    let _ = MultichannelAlgorithm::process(mb.as_mut(), &inputs, &mut out_slices);
                    for (ch, out_buf) in out_bufs.iter().enumerate() {
                        let reg_idx = first_dst + ch;
                        prog.block_regs[reg_idx][..n].copy_from_slice(&out_buf[..n]);
                    }
                }
                crate::program::BuiltinInst::Block(b) => {
                    let inp: Vec<T> = (0..n_in)
                        .flat_map(|ch| {
                            let reg_idx = srcs[ch];
                            prog.block_regs[reg_idx][..n].iter().copied()
                        })
                        .collect();
                    let mut out_buf = vec![T::ZERO; n_out * n];
                    let _ = b.process(Some(&inp), &mut out_buf);
                    for ch in 0..n_out {
                        let reg_idx = first_dst + ch;
                        let start = ch * n;
                        prog.block_regs[reg_idx][..n].copy_from_slice(&out_buf[start..start + n]);
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
