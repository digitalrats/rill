//! Lower a type-checked program to linear IR.

use std::collections::HashMap;

use crate::ast::{BinOp, Def, Expr, Program};
use crate::error::{CompileError, Span};
use crate::ir::{BinArith, Instr, Ir, StateLayout, UnOp};
use crate::types::infer::TypedProgram;

struct Lowerer<'a> {
    defs: HashMap<String, &'a Def>,
    instrs: Vec<Instr>,
    next_reg: usize,
    state_slots: usize,
    delay_lens: Vec<usize>,
    locals: Vec<HashMap<String, Vec<usize>>>,
}

impl<'a> Lowerer<'a> {
    fn fresh_reg(&mut self) -> usize {
        let r = self.next_reg;
        self.next_reg += 1;
        r
    }

    fn emit(&mut self, i: Instr) {
        self.instrs.push(i);
    }

    fn lower(&mut self, e: &Expr, inputs: &[usize]) -> Result<Vec<usize>, CompileError> {
        match e {
            Expr::Int(v, _) => {
                let dst = self.fresh_reg();
                self.emit(Instr::Const {
                    dst,
                    value: *v as f64,
                });
                Ok(vec![dst])
            }
            Expr::Float(v, _) => {
                let dst = self.fresh_reg();
                self.emit(Instr::Const { dst, value: *v });
                Ok(vec![dst])
            }
            Expr::Wire(_) => Ok(vec![inputs[0]]),
            Expr::Cut(_) => Ok(vec![]),
            Expr::Ref(name, span) => self.lower_ref(name, inputs, *span),
            Expr::Neg(inner, _) => {
                let outs = self.lower(inner, inputs)?;
                Ok(outs
                    .into_iter()
                    .map(|src| {
                        let dst = self.fresh_reg();
                        self.emit(Instr::Un {
                            dst,
                            op: UnOp::Neg,
                            src,
                        });
                        dst
                    })
                    .collect())
            }
            Expr::Apply { name, args, span } => {
                let mut arg_regs = Vec::new();
                for a in args {
                    arg_regs.extend(self.lower(a, inputs)?);
                }
                self.lower_ref(name, &arg_regs, *span)
            }
            Expr::Bin { op, lhs, rhs, span } => self.lower_bin(*op, lhs, rhs, inputs, *span),
        }
    }

    fn lower_ref(
        &mut self,
        name: &str,
        inputs: &[usize],
        span: Span,
    ) -> Result<Vec<usize>, CompileError> {
        let bin = match name {
            "+" => Some(BinArith::Add),
            "-" => Some(BinArith::Sub),
            "*" => Some(BinArith::Mul),
            "/" => Some(BinArith::Div),
            "%" => Some(BinArith::Rem),
            "min" => Some(BinArith::Min),
            "max" => Some(BinArith::Max),
            _ => None,
        };
        if let Some(op) = bin {
            let dst = self.fresh_reg();
            self.emit(Instr::Bin {
                dst,
                op,
                a: inputs[0],
                b: inputs[1],
            });
            return Ok(vec![dst]);
        }
        let un = match name {
            "sin" => Some(UnOp::Sin),
            "cos" => Some(UnOp::Cos),
            "tan" => Some(UnOp::Tan),
            "sqrt" => Some(UnOp::Sqrt),
            "exp" => Some(UnOp::Exp),
            "ln" => Some(UnOp::Ln),
            "tanh" => Some(UnOp::Tanh),
            "abs" => Some(UnOp::Abs),
            _ => None,
        };
        if let Some(op) = un {
            let dst = self.fresh_reg();
            self.emit(Instr::Un {
                dst,
                op,
                src: inputs[0],
            });
            return Ok(vec![dst]);
        }
        for scope in self.locals.iter().rev() {
            if let Some(regs) = scope.get(name) {
                return Ok(regs.clone());
            }
        }
        let def = *self.defs.get(name).ok_or_else(|| CompileError::Type {
            msg: format!("unknown `{name}` in lowering"),
            span,
        })?;
        let mut scope = HashMap::new();
        for (idx, p) in def.params.iter().enumerate() {
            scope.insert(p.clone(), vec![inputs[idx]]);
        }
        self.locals.push(scope);
        let out = self.lower(&def.body, inputs)?;
        self.locals.pop();
        Ok(out)
    }

    fn lower_bin(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        inputs: &[usize],
        span: Span,
    ) -> Result<Vec<usize>, CompileError> {
        match op {
            BinOp::Seq => {
                let mid = self.lower(lhs, inputs)?;
                self.lower(rhs, &mid)
            }
            BinOp::Par => {
                let li = arity_in(lhs)?;
                let (a_in, b_in) = inputs.split_at(li.min(inputs.len()));
                let mut out = self.lower(lhs, a_in)?;
                out.extend(self.lower(rhs, b_in)?);
                Ok(out)
            }
            BinOp::Split => {
                let a_out = self.lower(lhs, inputs)?;
                let bi = arity_in(rhs)?;
                let reps = bi / a_out.len().max(1);
                let mut fanned = Vec::with_capacity(bi);
                for _ in 0..reps {
                    fanned.extend(a_out.iter().copied());
                }
                self.lower(rhs, &fanned)
            }
            BinOp::Merge => {
                let a_out = self.lower(lhs, inputs)?;
                let bi = arity_in(rhs)?;
                let groups = a_out.len() / bi.max(1);
                let mut merged = Vec::with_capacity(bi);
                for k in 0..bi {
                    let mut acc = a_out[k];
                    for g in 1..groups {
                        let dst = self.fresh_reg();
                        self.emit(Instr::Bin {
                            dst,
                            op: BinArith::Add,
                            a: acc,
                            b: a_out[g * bi + k],
                        });
                        acc = dst;
                    }
                    merged.push(acc);
                }
                self.lower(rhs, &merged)
            }
            BinOp::Feedback => self.lower_feedback(lhs, rhs, inputs, span),
            BinOp::Delay => self.lower_delay(lhs, rhs, inputs, span),
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                let a = self.lower(lhs, inputs)?;
                let b = self.lower(rhs, inputs)?;
                let arith = match op {
                    BinOp::Add => BinArith::Add,
                    BinOp::Sub => BinArith::Sub,
                    BinOp::Mul => BinArith::Mul,
                    BinOp::Div => BinArith::Div,
                    BinOp::Rem => BinArith::Rem,
                    _ => unreachable!(),
                };
                let dst = self.fresh_reg();
                self.emit(Instr::Bin {
                    dst,
                    op: arith,
                    a: a[0],
                    b: b[0],
                });
                Ok(vec![dst])
            }
        }
    }

    fn lower_feedback(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        inputs: &[usize],
        _span: Span,
    ) -> Result<Vec<usize>, CompileError> {
        let bo = arity_out(rhs)?;
        let mut fb_regs = Vec::with_capacity(bo);
        let mut slots = Vec::with_capacity(bo);
        for _ in 0..bo {
            let slot = self.state_slots;
            self.state_slots += 1;
            slots.push(slot);
            let dst = self.fresh_reg();
            self.emit(Instr::ReadState { dst, slot });
            fb_regs.push(dst);
        }
        let mut a_in = fb_regs.clone();
        a_in.extend_from_slice(inputs);
        let a_out = self.lower(lhs, &a_in)?;
        let bi = arity_in(rhs)?;
        let b_in: Vec<usize> = a_out.iter().copied().take(bi).collect();
        let b_out = self.lower(rhs, &b_in)?;
        for (k, slot) in slots.iter().enumerate() {
            self.emit(Instr::WriteState {
                slot: *slot,
                src: b_out[k],
            });
        }
        Ok(a_out)
    }

    fn lower_delay(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        inputs: &[usize],
        span: Span,
    ) -> Result<Vec<usize>, CompileError> {
        let len = const_int(rhs).ok_or_else(|| CompileError::Type {
            msg: "delay length must be a constant integer expression".into(),
            span,
        })?;
        if len < 0 {
            return Err(CompileError::Type {
                msg: "delay length must be non-negative".into(),
                span,
            });
        }
        let signal = self.lower(lhs, inputs)?;
        let src = signal[0];
        if len == 0 {
            return Ok(vec![src]);
        }
        let line = self.delay_lens.len();
        self.delay_lens.push(len as usize);
        let dst = self.fresh_reg();
        self.emit(Instr::ReadDelay { dst, line });
        self.emit(Instr::WriteDelay { line, src });
        Ok(vec![dst])
    }
}

fn arity_out(e: &Expr) -> Result<usize, CompileError> {
    Ok(arity(e)?.1)
}
fn arity_in(e: &Expr) -> Result<usize, CompileError> {
    Ok(arity(e)?.0)
}

fn arity(e: &Expr) -> Result<(usize, usize), CompileError> {
    let unsupported = |m: &str| CompileError::Unsupported(m.to_string());
    Ok(match e {
        Expr::Int(_, _) | Expr::Float(_, _) => (0, 1),
        Expr::Wire(_) => (1, 1),
        Expr::Cut(_) => (1, 0),
        Expr::Neg(inner, _) => arity(inner)?,
        Expr::Ref(name, _) => match name.as_str() {
            "+" | "-" | "*" | "/" | "%" | "min" | "max" => (2, 1),
            "sin" | "cos" | "tan" | "sqrt" | "exp" | "ln" | "tanh" | "abs" => (1, 1),
            _ => {
                return Err(unsupported(
                    "arity of bare user-def ref; wrap in application",
                ))
            }
        },
        Expr::Apply { args, .. } => {
            let mut ins = 0;
            for a in args {
                ins += arity(a)?.0;
            }
            (ins, 1)
        }
        Expr::Bin { op, lhs, rhs, .. } => {
            let (ai, ao) = arity(lhs)?;
            let (bi, bo) = arity(rhs)?;
            match op {
                BinOp::Seq => (ai, bo),
                BinOp::Par => (ai + bi, ao + bo),
                BinOp::Split => (ai, bo),
                BinOp::Merge => (ai, bo),
                BinOp::Feedback => (ai - bo, ao),
                BinOp::Delay => (ai, ao),
                _ => (ai + bi, 1),
            }
        }
    })
}

fn const_int(e: &Expr) -> Option<i64> {
    match e {
        Expr::Int(v, _) => Some(*v),
        Expr::Neg(inner, _) => const_int(inner).map(|v| -v),
        Expr::Bin { op, lhs, rhs, .. } => {
            let a = const_int(lhs)?;
            let b = const_int(rhs)?;
            Some(match op {
                BinOp::Add => a + b,
                BinOp::Sub => a - b,
                BinOp::Mul => a * b,
                BinOp::Div if b != 0 => a / b,
                BinOp::Rem if b != 0 => a % b,
                _ => return None,
            })
        }
        _ => None,
    }
}

/// Lower a fully type-checked program into IR.
pub fn lower(tp: &TypedProgram) -> Result<Ir, CompileError> {
    let program: &Program = &tp.program;
    let defs: HashMap<String, &Def> = program.defs.iter().map(|d| (d.name.clone(), d)).collect();
    let process = *defs.get("process").ok_or_else(|| CompileError::Type {
        msg: "no `process` definition".into(),
        span: Span::new(0, 0),
    })?;

    let num_inputs = tp.process_ty.arity_in();
    let mut lw = Lowerer {
        defs,
        instrs: Vec::new(),
        next_reg: 0,
        state_slots: 0,
        delay_lens: Vec::new(),
        locals: Vec::new(),
    };
    let mut input_regs = Vec::with_capacity(num_inputs);
    for index in 0..num_inputs {
        let dst = lw.fresh_reg();
        lw.emit(Instr::LoadInput { dst, index });
        input_regs.push(dst);
    }
    let outs = lw.lower(&process.body, &input_regs)?;
    if outs.len() != 1 {
        return Err(CompileError::Unsupported(format!(
            "process lowered to {} outputs, expected 1",
            outs.len()
        )));
    }
    Ok(Ir {
        instrs: lw.instrs,
        num_regs: lw.next_reg,
        output_reg: outs[0],
        num_inputs,
        state: StateLayout {
            state_slots: lw.state_slots,
            delay_lens: lw.delay_lens,
        },
        builtins: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::types::infer::infer_program;

    fn ir_of(src: &str) -> Ir {
        let p = parse(&tokenize(src).unwrap()).unwrap();
        let tp = infer_program(&p).unwrap();
        lower(&tp).unwrap()
    }

    #[test]
    fn gain_lowers_to_const_and_mul() {
        let ir = ir_of("process = _ * 0.5;");
        assert_eq!(ir.num_inputs, 1);
        assert!(ir.instrs.iter().any(|i| matches!(
            i,
            Instr::Bin {
                op: BinArith::Mul,
                ..
            }
        )));
        assert!(ir
            .instrs
            .iter()
            .any(|i| matches!(i, Instr::Const { value, .. } if (*value - 0.5).abs() < 1e-9)));
    }

    #[test]
    fn integrator_allocates_one_state_slot() {
        let ir = ir_of("process = + ~ _;");
        assert_eq!(ir.state.state_slots, 1);
        assert!(ir
            .instrs
            .iter()
            .any(|i| matches!(i, Instr::ReadState { .. })));
        assert!(ir
            .instrs
            .iter()
            .any(|i| matches!(i, Instr::WriteState { .. })));
    }

    #[test]
    fn delay_allocates_line() {
        let ir = ir_of("process = _ @ 3;");
        assert_eq!(ir.state.delay_lens, vec![3]);
    }
}
