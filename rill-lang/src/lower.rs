//! Lower a type-checked program to linear IR.

use std::collections::HashMap;

use crate::ast::{BinOp, Def, Expr, Program};
use crate::builtin::{BuiltinKind, SignatureSource};
use crate::error::{CompileError, Span};
use crate::ir::{BinArith, BuiltinInstance, Instr, Ir, ParamDef, StateLayout, UnOp};
use crate::types::infer::TypedProgram;

struct Lowerer<'a> {
    defs: HashMap<String, Def>,
    sigs: &'a dyn SignatureSource,
    instrs: Vec<Instr>,
    next_reg: usize,
    state_slots: usize,
    delay_lens: Vec<usize>,
    locals: Vec<HashMap<String, Vec<usize>>>,
    builtins: Vec<BuiltinInstance>,
    params: Vec<ParamDef>,
    param_names: HashMap<String, usize>,
    sample_rate: f32,
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

    fn lower(&mut self, e: &Expr, args: &[usize]) -> Result<Vec<usize>, CompileError> {
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
            Expr::Imag(v, _) => {
                let name = "complex".to_string();
                if let Some(sig) = self.sigs.builtin_sig(&name) {
                    let sig = sig.clone();
                    let instance = self.builtins.len();
                    self.builtins.push(BuiltinInstance {
                        name,
                        params: vec![0.0, *v],
                        kind: sig.kind,
                        signal_ins: sig.signal_ins(),
                        signal_outs: sig.signal_outs,
                        param_bindings: Vec::new(),
                    });
                    let fst = self.fresh_reg();
                    for _ in 1..sig.signal_outs {
                        self.fresh_reg();
                    }
                    self.emit(Instr::CallBlock {
                        dst: fst,
                        src: 0,
                        instance,
                    });
                    return Ok((0..sig.signal_outs).map(|i| fst + i).collect());
                }
                let dst = self.fresh_reg();
                self.emit(Instr::Const { dst, value: 0.0 });
                Ok(vec![dst])
            }
            Expr::Wire(_) => Ok(vec![args[0]]),
            Expr::Cut(_) => Ok(vec![]),
            Expr::Ref(name, span) => self.lower_ref(name, args, *span),
            Expr::Neg(inner, _) => {
                let outs = self.lower(inner, args)?;
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
            Expr::Apply {
                name,
                args: call_args,
                span,
            } => {
                if name == "smooth" {
                    let x_regs = self.lower(&call_args[0], args)?;
                    let x = x_regs[0];
                    let ms = const_f64(&call_args[1]).unwrap_or(0.0);
                    let sr = self.sample_rate as f64;
                    let a = if ms <= 0.0 {
                        1.0
                    } else {
                        let tau = ms / 1000.0;
                        1.0 - (-1.0 / (tau * sr)).exp()
                    };
                    let slot = self.state_slots;
                    self.state_slots += 1;
                    let prev = self.fresh_reg();
                    self.emit(Instr::ReadState { dst: prev, slot });
                    let diff = self.fresh_reg();
                    self.emit(Instr::Bin {
                        dst: diff,
                        op: BinArith::Sub,
                        a: x,
                        b: prev,
                    });
                    let acoef = self.fresh_reg();
                    self.emit(Instr::Const {
                        dst: acoef,
                        value: a,
                    });
                    let scaled = self.fresh_reg();
                    self.emit(Instr::Bin {
                        dst: scaled,
                        op: BinArith::Mul,
                        a: acoef,
                        b: diff,
                    });
                    let y = self.fresh_reg();
                    self.emit(Instr::Bin {
                        dst: y,
                        op: BinArith::Add,
                        a: prev,
                        b: scaled,
                    });
                    self.emit(Instr::WriteState { slot, src: y });
                    return Ok(vec![y]);
                }
                if let Some(sig) = self.sigs.builtin_sig(name) {
                    let sig = sig.clone();
                    let mut params = Vec::with_capacity(call_args.len());
                    let mut param_bindings = Vec::new();
                    for (pos, a) in call_args.iter().enumerate() {
                        if let Expr::Ref(ref_name, _) = a {
                            if let Some(&pidx) = self.param_names.get(ref_name) {
                                params.push(0.0);
                                param_bindings.push((pos, pidx));
                                continue;
                            }
                        }
                        let v = const_f64(a).ok_or_else(|| CompileError::Type {
                            msg: format!(
                                "param to `{name}` must be a constant expression or a declared parameter"
                            ),
                            span: a.span(),
                        })?;
                        params.push(v);
                    }
                    let instance = self.builtins.len();
                    self.builtins.push(BuiltinInstance {
                        name: name.clone(),
                        params,
                        kind: sig.kind,
                        signal_ins: sig.signal_ins(),
                        signal_outs: sig.signal_outs,
                        param_bindings,
                    });
                    match sig.kind {
                        BuiltinKind::Sample => {
                            let dst = self.fresh_reg();
                            let srcs = args.to_vec();
                            self.emit(Instr::CallSample {
                                dst,
                                srcs,
                                instance,
                            });
                            return Ok(vec![dst]);
                        }
                        BuiltinKind::Block => {
                            let fst = self.fresh_reg();
                            for _ in 1..sig.signal_outs {
                                self.fresh_reg();
                            }
                            let src = if args.is_empty() { 0 } else { args[0] };
                            self.emit(Instr::CallBlock {
                                dst: fst,
                                src,
                                instance,
                            });
                            return Ok((0..sig.signal_outs).map(|i| fst + i).collect());
                        }
                    }
                }
                let mut arg_regs = Vec::new();
                for a in call_args {
                    arg_regs.extend(self.lower(a, args)?);
                }
                // User-defined function: prepend λ-arguments to caller's args. The Anchor
                // splits by params().len() — first N entries fill the scope, remainder are
                // passed to the body. Builtins receive arg_regs directly (no λ-params).
                if self.defs.contains_key(name) {
                    let mut combined = arg_regs.clone();
                    combined.extend_from_slice(args);
                    self.lower_ref(name, &combined, *span)
                } else {
                    self.lower_ref(name, &arg_regs, *span)
                }
            }
            Expr::Str(_, span) => Err(CompileError::Type {
                msg: "string literal is only valid as a parameter name".into(),
                span: *span,
            }),
            Expr::Bin { op, lhs, rhs, span } => self.lower_bin(*op, lhs, rhs, args, *span),
            Expr::Let {
                defs,
                body,
                span: _,
            } => {
                let saved = self.defs.clone();
                for d in defs {
                    self.defs.insert(d.name().to_string(), d.clone());
                }
                let result = self.lower(body, args);
                self.defs = saved;
                result
            }
        }
    }

    /// Intern a named parameter, returning its slot index. Repeated uses of the
    /// same name share one slot but must declare an identical default and range —
    /// a conflicting redeclaration is a compile error (avoids silent first-wins).
    #[allow(clippy::float_cmp)]
    fn intern_param(
        &mut self,
        name: String,
        default: f64,
        min: f64,
        max: f64,
        span: Span,
    ) -> Result<usize, CompileError> {
        if let Some(&idx) = self.param_names.get(&name) {
            let existing = &self.params[idx];
            if existing.default != default || existing.min != min || existing.max != max {
                return Err(CompileError::Type {
                    msg: format!(
                        "parameter `{name}` is redeclared with a different default/range; \
                         all uses of the same name must match"
                    ),
                    span,
                });
            }
            Ok(idx)
        } else {
            let idx = self.params.len();
            self.params.push(ParamDef {
                name: name.clone(),
                default,
                min,
                max,
            });
            self.param_names.insert(name, idx);
            Ok(idx)
        }
    }

    fn lower_ref(
        &mut self,
        name: &str,
        args: &[usize],
        _span: Span,
    ) -> Result<Vec<usize>, CompileError> {
        if let Some(sig) = self.sigs.builtin_sig(name) {
            if sig.clone().params.len() == sig.clone().signal_ins() {
                let sig = sig.clone();
                let instance = self.builtins.len();
                self.builtins.push(BuiltinInstance {
                    name: name.to_string(),
                    params: Vec::new(),
                    kind: sig.kind,
                    signal_ins: sig.signal_ins(),
                    signal_outs: sig.signal_outs,
                    param_bindings: Vec::new(),
                });
                match sig.kind {
                    BuiltinKind::Sample => {
                        let dst = self.fresh_reg();
                        let srcs = args.to_vec();
                        self.emit(Instr::CallSample {
                            dst,
                            srcs,
                            instance,
                        });
                        return Ok(vec![dst]);
                    }
                    BuiltinKind::Block => {
                        let fst = self.fresh_reg();
                        for _ in 1..sig.signal_outs {
                            self.fresh_reg();
                        }
                        let src = if args.is_empty() { 0 } else { args[0] };
                        self.emit(Instr::CallBlock {
                            dst: fst,
                            src,
                            instance,
                        });
                        return Ok((0..sig.signal_outs).map(|i| fst + i).collect());
                    }
                }
            }
        }
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
                a: args[0],
                b: args[1],
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
                src: args[0],
            });
            return Ok(vec![dst]);
        }
        if let Some(&idx) = self.param_names.get(name) {
            let dst = self.fresh_reg();
            self.emit(Instr::ReadParam { dst, idx });
            return Ok(vec![dst]);
        }
        for scope in self.locals.iter().rev() {
            if let Some(regs) = scope.get(name) {
                return Ok(regs.clone());
            }
        }
        let def = self
            .defs
            .get(name)
            .cloned()
            .ok_or_else(|| CompileError::Type {
                msg: format!("unknown `{name}` in lowering"),
                span: _span,
            })?;
        match def {
            Def::Anchor {
                params: def_params,
                ref body,
                ..
            } => {
                let n = def_params.len();
                let mut scope = HashMap::new();
                for (idx, p) in def_params.iter().enumerate() {
                    scope.insert(p.name.clone(), vec![args[idx]]);
                }
                self.locals.push(scope);
                let out = self.lower(body, &args[n..])?;
                self.locals.pop();
                Ok(out)
            }
            Def::Local { ref body, .. } => self.lower(body, args),
        }
    }

    fn lower_bin(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        args: &[usize],
        span: Span,
    ) -> Result<Vec<usize>, CompileError> {
        match op {
            BinOp::Seq => {
                let mid = self.lower(lhs, args)?;
                self.lower(rhs, &mid)
            }
            BinOp::Par => {
                let li = arity_in(lhs, self.sigs)?;
                let (a_in, b_in) = args.split_at(li.min(args.len()));
                let mut out = self.lower(lhs, a_in)?;
                out.extend(self.lower(rhs, b_in)?);
                Ok(out)
            }
            BinOp::Split => {
                let a_out = self.lower(lhs, args)?;
                let bi = arity_in(rhs, self.sigs)?;
                let reps = bi / a_out.len().max(1);
                let mut fanned = Vec::with_capacity(bi);
                for _ in 0..reps {
                    fanned.extend(a_out.iter().copied());
                }
                self.lower(rhs, &fanned)
            }
            BinOp::Merge => {
                let a_out = self.lower(lhs, args)?;
                let bi = arity_in(rhs, self.sigs)?;
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
            BinOp::Feedback => self.lower_feedback(lhs, rhs, args, span),
            BinOp::Delay => self.lower_delay(lhs, rhs, args, span),
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                if matches!(op, BinOp::Add | BinOp::Sub) {
                    let re = match lhs {
                        Expr::Float(v, _) => Some(*v),
                        Expr::Int(v, _) => Some(*v as f64),
                        _ => None,
                    };
                    let im = match rhs {
                        Expr::Imag(v, _) => Some(if matches!(op, BinOp::Sub) { -*v } else { *v }),
                        _ => None,
                    };
                    if let (Some(re), Some(im)) = (re, im) {
                        let name = "complex".to_string();
                        if let Some(sig) = self.sigs.builtin_sig(&name) {
                            let sig = sig.clone();
                            let instance = self.builtins.len();
                            self.builtins.push(BuiltinInstance {
                                name,
                                params: vec![re, im],
                                kind: sig.kind,
                                signal_ins: sig.signal_ins(),
                                signal_outs: sig.signal_outs,
                                param_bindings: Vec::new(),
                            });
                            let fst = self.fresh_reg();
                            for _ in 1..sig.signal_outs {
                                self.fresh_reg();
                            }
                            self.emit(Instr::CallBlock {
                                dst: fst,
                                src: 0,
                                instance,
                            });
                            return Ok((0..sig.signal_outs).map(|i| fst + i).collect());
                        }
                    }
                }
                let a = self.lower(lhs, args)?;
                let b = self.lower(rhs, args)?;
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
        args: &[usize],
        _span: Span,
    ) -> Result<Vec<usize>, CompileError> {
        let bo = arity_out(rhs, self.sigs)?;
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
        a_in.extend_from_slice(args);
        let a_out = self.lower(lhs, &a_in)?;
        let bi = arity_in(rhs, self.sigs)?;
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
        args: &[usize],
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
        let signal = self.lower(lhs, args)?;
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

fn arity_out(e: &Expr, sigs: &dyn SignatureSource) -> Result<usize, CompileError> {
    Ok(arity(e, sigs)?.1)
}
fn arity_in(e: &Expr, sigs: &dyn SignatureSource) -> Result<usize, CompileError> {
    Ok(arity(e, sigs)?.0)
}

fn arity(e: &Expr, sigs: &dyn SignatureSource) -> Result<(usize, usize), CompileError> {
    let _unsupported = |m: &str| CompileError::Unsupported(m.to_string());
    Ok(match e {
        Expr::Int(_, _) | Expr::Float(_, _) => (0, 1),
        Expr::Imag(_, _) => (0, 2),
        Expr::Str(_, _) => (0, 1),
        Expr::Wire(_) => (1, 1),
        Expr::Cut(_) => (1, 0),
        Expr::Neg(inner, _) => arity(inner, sigs)?,
        Expr::Ref(name, _) => match name.as_str() {
            "+" | "-" | "*" | "/" | "%" | "min" | "max" => (2, 1),
            "sin" | "cos" | "tan" | "sqrt" | "exp" | "ln" | "tanh" | "abs" => (1, 1),
            _ => {
                if let Some(sig) = sigs.builtin_sig(name) {
                    (sig.signal_ins(), sig.signal_outs)
                } else {
                    (0, 1)
                }
            }
        },
        Expr::Apply { name, args, .. } => {
            if let Some(sig) = sigs.builtin_sig(name) {
                (sig.signal_ins(), sig.signal_outs)
            } else {
                let mut ins = 0;
                for a in args {
                    ins += arity(a, sigs)?.0;
                }
                (ins, 1)
            }
        }
        Expr::Bin { op, lhs, rhs, .. } => {
            let (ai, ao) = arity(lhs, sigs)?;
            let (bi, bo) = arity(rhs, sigs)?;
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
        Expr::Let { body, .. } => arity(body, sigs)?,
    })
}

fn const_f64(e: &Expr) -> Option<f64> {
    match e {
        Expr::Float(v, _) => Some(*v),
        Expr::Int(v, _) => Some(*v as f64),
        Expr::Neg(inner, _) => const_f64(inner).map(|v| -v),
        Expr::Bin { op, lhs, rhs, .. } => {
            let a = const_f64(lhs)?;
            let b = const_f64(rhs)?;
            Some(match op {
                BinOp::Add => a + b,
                BinOp::Sub => a - b,
                BinOp::Mul => a * b,
                BinOp::Div => a / b,
                _ => return None,
            })
        }
        _ => None,
    }
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

/// Back-compat: lower with no built-ins and a default sample rate of 44.1 kHz.
pub fn lower(tp: &TypedProgram) -> Result<Ir, CompileError> {
    lower_with(tp, &crate::builtin::NoSigs, 44_100.0)
}

/// Lower a fully type-checked program into IR with a signature source and sample rate.
pub fn lower_with(
    tp: &TypedProgram,
    sigs: &dyn SignatureSource,
    sample_rate: f32,
) -> Result<Ir, CompileError> {
    let program: &Program = &tp.program;
    let main = program
        .main_def()
        .ok_or_else(|| CompileError::Unsupported("program must have a `main` definition".into()))?;

    let mut defs: HashMap<String, Def> = HashMap::new();
    for d in &program.defs {
        defs.insert(d.name().to_string(), d.clone());
        for wd in d.where_defs() {
            defs.insert(wd.name().to_string(), wd.clone());
        }
    }

    let num_inputs = tp.process_ty.arity_in();
    let mut lw = Lowerer {
        defs,
        sigs,
        instrs: Vec::new(),
        next_reg: 0,
        state_slots: 0,
        delay_lens: Vec::new(),
        locals: Vec::new(),
        builtins: Vec::new(),
        params: Vec::new(),
        param_names: HashMap::new(),
        sample_rate,
    };

    for p in main.params() {
        lw.intern_param(
            p.name.clone(),
            0.0,
            f64::NEG_INFINITY,
            f64::INFINITY,
            p.span,
        )?;
    }

    let mut main_args = Vec::with_capacity(num_inputs);
    for index in 0..num_inputs {
        let dst = lw.fresh_reg();
        lw.emit(Instr::LoadInput { dst, index });
        main_args.push(dst);
    }
    let outs = lw.lower(main.body(), &main_args)?;
    if outs.len() != 1 {
        return Err(CompileError::Unsupported(format!(
            "body lowered to {} outputs, expected 1",
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
        builtins: lw.builtins,
        params: lw.params,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::types::infer::{infer_program, infer_program_with};

    fn ir_of(src: &str) -> Ir {
        let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
        let tp = infer_program(&p).unwrap();
        lower(&tp).unwrap()
    }

    struct TestSigs;
    impl crate::builtin::SignatureSource for TestSigs {
        fn builtin_sig(&self, name: &str) -> Option<&crate::builtin::BuiltinSig> {
            use crate::builtin::{BuiltinKind, BuiltinSig};
            match name {
                "lowpass" => Some(Box::leak(Box::new(BuiltinSig::simple(
                    "lowpass",
                    1,
                    1,
                    2,
                    BuiltinKind::Block,
                )))),
                "onepole" => Some(Box::leak(Box::new(BuiltinSig::simple(
                    "onepole",
                    1,
                    1,
                    2,
                    BuiltinKind::Sample,
                )))),
                _ => None,
            }
        }
    }

    fn ir_with(src: &str) -> Ir {
        let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
        let tp = infer_program_with(&p, &TestSigs).unwrap();
        lower_with(&tp, &TestSigs, 44_100.0).unwrap()
    }

    #[test]
    fn gain_lowers_to_const_and_mul() {
        let ir = ir_of("main = _ * 0.5");
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
        let ir = ir_of("main = + ~ _");
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
        let ir = ir_of("main = _ @ 3");
        assert_eq!(ir.state.delay_lens, vec![3]);
    }

    #[test]
    fn sample_builtin_lowers_to_callsample() {
        let ir = ir_with("main = _ : onepole(200.0, 0.5)");
        assert!(
            ir.instrs
                .iter()
                .any(|i| matches!(i, Instr::CallSample { .. })),
            "expected a CallSample instruction"
        );
        assert_eq!(ir.builtins.len(), 1);
        let bi = &ir.builtins[0];
        assert_eq!(bi.kind, BuiltinKind::Sample);
        assert_eq!(bi.params, vec![200.0, 0.5]);
    }

    #[test]
    fn block_builtin_lowers_to_callblock() {
        let ir = ir_with("main = _ : lowpass(1000.0, 0.7)");
        assert!(
            ir.instrs
                .iter()
                .any(|i| matches!(i, Instr::CallBlock { .. })),
            "expected a CallBlock instruction"
        );
        assert_eq!(ir.builtins.len(), 1);
        let bi = &ir.builtins[0];
        assert_eq!(bi.kind, BuiltinKind::Block);
        assert_eq!(bi.params, vec![1000.0, 0.7]);
    }

    #[test]
    fn smooth_allocates_state() {
        let ir = ir_of("main = smooth(_, 10.0)");
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
}
