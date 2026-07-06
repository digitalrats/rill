//! Sample-by-sample IR evaluator.

use rill_core::math::Transcendental;

use crate::ir::{BinArith, Instr, UnOp};
use crate::program::RillProgram;

/// Run one block: for each output sample, evaluate the IR once.
pub fn run_block<T: Transcendental>(
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
        let y = eval_sample(prog, in_sample);
        output[i] = T::from_f64(y);
    }
}

fn eval_sample<T: Transcendental>(prog: &mut RillProgram<T>, in0: f64) -> f64 {
    for idx in 0..prog.ir.instrs.len() {
        match prog.ir.instrs[idx].clone() {
            Instr::Const { dst, value } => prog.regs[dst] = value,
            Instr::LoadInput { dst, index } => {
                prog.regs[dst] = if index == 0 { in0 } else { 0.0 };
            }
            Instr::ReadState { dst, slot } => prog.regs[dst] = prog.state[slot],
            Instr::ReadDelay { dst, line } => prog.regs[dst] = prog.delays[line].read(),
            Instr::Move { dst, src } => prog.regs[dst] = prog.regs[src],
            Instr::Un { dst, op, src } => {
                let x = prog.regs[src];
                prog.regs[dst] = apply_un(op, x);
            }
            Instr::Bin { dst, op, a, b } => {
                let x = prog.regs[a];
                let y = prog.regs[b];
                prog.regs[dst] = apply_bin(op, x, y);
            }
            Instr::WriteState { slot, src } => prog.state_next[slot] = prog.regs[src],
            Instr::WriteDelay { line, src } => {
                let v = prog.regs[src];
                prog.delays[line].write(v);
            }
        }
    }
    for (s, n) in prog.state.iter_mut().zip(prog.state_next.iter()) {
        *s = *n;
    }
    prog.regs[prog.ir.output_reg]
}

fn apply_un(op: UnOp, x: f64) -> f64 {
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

fn apply_bin(op: BinArith, x: f64, y: f64) -> f64 {
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
    fn gain_halves_input() {
        let mut prog = build("process = _ * 0.5;");
        let input = [1.0f32, 2.0, 4.0, 8.0];
        let mut out = [0.0f32; 4];
        prog.process(Some(&input), &mut out).unwrap();
        assert_eq!(out, [0.5, 1.0, 2.0, 4.0]);
    }

    #[test]
    fn integrator_accumulates() {
        let mut prog = build("process = + ~ _;");
        let input = [1.0f32, 1.0, 1.0, 1.0];
        let mut out = [0.0f32; 4];
        prog.process(Some(&input), &mut out).unwrap();
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn one_sample_delay() {
        let mut prog = build("process = _ @ 1;");
        let input = [5.0f32, 7.0, 9.0];
        let mut out = [0.0f32; 3];
        prog.process(Some(&input), &mut out).unwrap();
        assert_eq!(out, [0.0, 5.0, 7.0]);
    }

    #[test]
    fn split_merge_doubles() {
        let mut prog = build("process = _ <: (_ , _) :> + ;");
        let input = [1.0f32, 3.0];
        let mut out = [0.0f32; 2];
        prog.process(Some(&input), &mut out).unwrap();
        assert_eq!(out, [2.0, 6.0]);
    }
}
