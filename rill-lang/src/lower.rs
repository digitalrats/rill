//! Lower a type-checked program to linear IR.

use std::collections::{HashMap, HashSet};

use crate::ast::{ArithOp, Def, Expr, Program};
use crate::builtin::{ParamType, SignatureSource};
use crate::error::{CompileError, Span};
use crate::ir::{BinArith, BuiltinInstance, Instr, Ir, ParamDef, StateLayout, UnOp};
use crate::types::infer::TypedProgram;

struct Lowerer<'a> {
    defs: HashMap<String, Def>,
    sigs: &'a dyn SignatureSource,
    cafs: &'a HashSet<String>,
    caf_cache: HashMap<String, Vec<usize>>,
    caf_lifting: HashSet<String>,
    instrs: Vec<Instr>,
    next_reg: usize,
    block_state_slots: usize,
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
                        resource: None,
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
                        srcs: vec![],
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
                    let ms = self.caf_const(&call_args[1]).unwrap_or(0.0);
                    let sr = self.sample_rate as f64;
                    let a = if ms <= 0.0 {
                        1.0
                    } else {
                        let tau = ms / 1000.0;
                        1.0 - (-1.0 / (tau * sr)).exp()
                    };
                    let slot = self.block_state_slots;
                    self.block_state_slots += 1;
                    let prev = self.fresh_reg();
                    self.emit(Instr::ReadBlockState { dst: prev, slot });
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
                    self.emit(Instr::WriteBlockState { slot, src: y });
                    return Ok(vec![y]);
                }
                if let Some(sig) = self.sigs.builtin_sig(name).cloned() {
                    let mut param_values = Vec::new();
                    let mut param_bindings = Vec::new();
                    let mut signal_srcs = Vec::new();
                    let mut signal_pos = 0;
                    let mut param_pos = 0;
                    let mut resource: Option<String> = None;

                    for ptype in &sig.params {
                        match ptype {
                            ParamType::Signal => {
                                if signal_pos >= args.len() {
                                    return Err(CompileError::Type {
                                        msg: format!("missing signal input for `{name}`"),
                                        span: *span,
                                    });
                                }
                                signal_srcs.push(args[signal_pos]);
                                signal_pos += 1;
                            }
                            ParamType::Resource => {
                                if param_pos >= call_args.len() {
                                    break;
                                }
                                match &call_args[param_pos] {
                                    Expr::Ref(res_name, _) => {
                                        resource = Some(res_name.clone());
                                    }
                                    other => {
                                        return Err(CompileError::Type {
                                            msg: format!(
                                                "resource argument of `{name}` must be a symbolic reference",
                                            ),
                                            span: other.span(),
                                        });
                                    }
                                }
                                param_pos += 1;
                            }
                            ParamType::Float | ParamType::Int => {
                                if param_pos >= call_args.len() {
                                    break;
                                }
                                if let Expr::Ref(ref_name, _) = &call_args[param_pos] {
                                    if let Some(&pidx) = self.param_names.get(ref_name) {
                                        param_values.push(0.0);
                                        param_bindings.push((param_values.len() - 1, pidx));
                                        param_pos += 1;
                                        continue;
                                    }
                                }
                                if let Expr::ActorParam {
                                    name,
                                    default,
                                    span,
                                } = &call_args[param_pos]
                                {
                                    let default_val = if let Some(d) = default {
                                        const_f64(d).unwrap_or(0.0)
                                    } else {
                                        0.0
                                    };
                                    let idx = self.intern_param(
                                        name.clone(),
                                        default_val,
                                        f64::NEG_INFINITY,
                                        f64::INFINITY,
                                        *span,
                                    )?;
                                    param_values.push(0.0);
                                    param_bindings.push((param_values.len() - 1, idx));
                                    param_pos += 1;
                                    continue;
                                }
                                let v = self.caf_const(&call_args[param_pos]).ok_or_else(|| {
                                    CompileError::Type {
                                        msg: format!(
                                            "param at position {param_pos} of `{name}` \
                                                 must be a constant or parameter reference"
                                        ),
                                        span: call_args[param_pos].span(),
                                    }
                                })?;
                                param_values.push(v);
                                param_pos += 1;
                            }
                            ParamType::String => {
                                if param_pos >= call_args.len() {
                                    break;
                                }
                                param_pos += 1;
                            }
                            ParamType::Bool => {
                                if param_pos >= call_args.len() {
                                    break;
                                }
                                match &call_args[param_pos] {
                                    Expr::Int(0, _) => param_values.push(0.0),
                                    Expr::Int(1, _) => param_values.push(1.0),
                                    _ => param_values.push(1.0),
                                }
                                param_pos += 1;
                            }
                            ParamType::Enum(_) => {
                                if param_pos >= call_args.len() {
                                    break;
                                }
                                param_pos += 1;
                            }
                            ParamType::Record(schema) => {
                                if param_pos >= call_args.len() {
                                    break;
                                }
                                if let Expr::Record(fields, field_span) = &call_args[param_pos] {
                                    let mut field_values: HashMap<&str, f64> = HashMap::new();
                                    for (field_name, field_expr) in fields {
                                        if let Some(val) = self.caf_const(field_expr) {
                                            field_values.insert(field_name.as_str(), val);
                                        }
                                    }
                                    // Push schema field values in schema order so the
                                    // built-in factory can read its configuration.
                                    for field in &schema.fields {
                                        let val = field_values
                                            .get(field.name)
                                            .copied()
                                            .unwrap_or(field.default.unwrap_or(0.0));
                                        param_values.push(val);
                                    }
                                    for (field_name, field_expr) in fields {
                                        if let Some(val) = self.caf_const(field_expr) {
                                            self.intern_param(
                                                field_name.clone(),
                                                val,
                                                f64::NEG_INFINITY,
                                                f64::INFINITY,
                                                *field_span,
                                            )?;
                                        }
                                    }
                                }
                                param_pos += 1;
                            }
                            ParamType::Variadic(inner) => match &**inner {
                                ParamType::Signal => {
                                    for &reg in &args[signal_pos..] {
                                        signal_srcs.push(reg);
                                    }
                                    signal_pos = args.len();
                                }
                                _ => {
                                    for arg in &call_args[param_pos..] {
                                        if let Expr::Ref(ref_name, _) = arg {
                                            if let Some(&pidx) = self.param_names.get(ref_name) {
                                                param_values.push(0.0);
                                                param_bindings.push((param_values.len() - 1, pidx));
                                                continue;
                                            }
                                        }
                                        if let Some(val) = self.caf_const(arg) {
                                            param_values.push(val);
                                        }
                                    }
                                    param_pos = call_args.len();
                                }
                            },
                        }
                    }

                    let instance = self.builtins.len();
                    self.builtins.push(BuiltinInstance {
                        name: name.clone(),
                        params: param_values,
                        resource,
                        kind: sig.kind,
                        signal_ins: signal_srcs.len(),
                        signal_outs: sig.signal_outs,
                        param_bindings,
                    });
                    let fst = self.fresh_reg();
                    for _ in 1..sig.signal_outs {
                        self.fresh_reg();
                    }
                    self.emit(Instr::CallBlock {
                        dst: fst,
                        srcs: signal_srcs,
                        instance,
                    });
                    return Ok((0..sig.signal_outs).map(|i| fst + i).collect());
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
            Expr::Seq(lhs, rhs, _) => self.lower_seq(lhs, rhs, args),
            Expr::Par(lhs, rhs, _) => self.lower_par(lhs, rhs, args),
            Expr::Split(lhs, rhs, _) => self.lower_split(lhs, rhs, args),
            Expr::Merge(lhs, rhs, _) => self.lower_merge(lhs, rhs, args),
            Expr::Loop(lhs, rhs, span) => self.lower_feedback(lhs, rhs, args, *span),
            Expr::Delay(lhs, rhs, span) => self.lower_delay(lhs, rhs, args, *span),
            Expr::Arith { op, lhs, rhs, .. } => self.lower_arith(*op, lhs, rhs, args),
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
            Expr::Record(..) => unreachable!("Record should be desugared before lowering"),
            Expr::ActorParam {
                name,
                default,
                span,
            } => {
                let default_val = if let Some(d) = default {
                    const_f64(d).unwrap_or(0.0)
                } else {
                    0.0
                };
                let full_name = self.actor_param_prefix(name);
                let idx = self.intern_param(
                    full_name,
                    default_val,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    *span,
                )?;
                let dst = self.fresh_reg();
                self.emit(Instr::ReadActorParam {
                    dst,
                    param_idx: idx,
                });
                Ok(vec![dst])
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

    /// Build composit param name from current scope context + local name.
    fn actor_param_prefix(&self, local_name: &str) -> String {
        local_name.to_string()
    }

    /// Fold a compile-time parameter argument to a constant, resolving a
    /// reference to a closed CAF by const-folding its reduced body. Returns
    /// `None` when the argument is not a constant expression.
    fn caf_const(&self, e: &Expr) -> Option<f64> {
        self.caf_const_impl(e, &mut HashSet::new())
    }

    /// Workhorse for [`caf_const`]: follows CAF→CAF reference chains
    /// transitively, tracking visited names so a cycle folds to `None` rather
    /// than recursing forever.
    fn caf_const_impl(&self, e: &Expr, visited: &mut HashSet<String>) -> Option<f64> {
        if let Expr::Ref(ref_name, _) = e {
            if self.cafs.contains(ref_name) {
                if visited.contains(ref_name) {
                    return None;
                }
                if let Some(Def::Local { body, .. }) = self.defs.get(ref_name) {
                    visited.insert(ref_name.clone());
                    let v = self.caf_const_impl(body, visited);
                    visited.remove(ref_name);
                    return v;
                }
            }
        }
        const_f64(e)
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
                    resource: None,
                    kind: sig.kind,
                    signal_ins: sig.signal_ins(),
                    signal_outs: sig.signal_outs,
                    param_bindings: Vec::new(),
                });
                let fst = self.fresh_reg();
                for _ in 1..sig.signal_outs {
                    self.fresh_reg();
                }
                let srcs = args.to_vec();
                self.emit(Instr::CallBlock {
                    dst: fst,
                    srcs,
                    instance,
                });
                return Ok((0..sig.signal_outs).map(|i| fst + i).collect());
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
            Def::Local { ref body, .. } => {
                if self.cafs.contains(name) {
                    if let Some(regs) = self.caf_cache.get(name) {
                        return Ok(regs.clone());
                    }
                    if self.caf_lifting.contains(name) {
                        return Err(CompileError::Type {
                            msg: format!("recursive CAF definition `{name}`"),
                            span: _span,
                        });
                    }
                    // Lift the closed body once: no signal inputs, so lower it with
                    // no arguments and cache the output registers for sharing.
                    self.caf_lifting.insert(name.to_string());
                    let res = self.lower(body, &[]);
                    self.caf_lifting.remove(name);
                    let out = res?;
                    self.caf_cache.insert(name.to_string(), out.clone());
                    return Ok(out);
                }
                self.lower(body, args)
            }
        }
    }

    /// `A : B` — lower lhs, feed its outputs into rhs.
    fn lower_seq(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        args: &[usize],
    ) -> Result<Vec<usize>, CompileError> {
        let mid = self.lower(lhs, args)?;
        self.lower(rhs, &mid)
    }

    /// `A , B` — split the input registers between the two sides, concatenate outputs.
    fn lower_par(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        args: &[usize],
    ) -> Result<Vec<usize>, CompileError> {
        let li = self.arity_in(lhs)?;
        let (a_in, b_in) = args.split_at(li.min(args.len()));
        let mut out = self.lower(lhs, a_in)?;
        out.extend(self.lower(rhs, b_in)?);
        Ok(out)
    }

    /// `A <: B` — fan out A's outputs to fill B's inputs, then lower B.
    fn lower_split(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        args: &[usize],
    ) -> Result<Vec<usize>, CompileError> {
        let a_out = self.lower(lhs, args)?;
        let bi = if self.rhs_variadic(rhs) {
            a_out.len()
        } else {
            self.arity_in(rhs)?
        };
        let reps = bi / a_out.len().max(1);
        let mut fanned = Vec::with_capacity(bi);
        for _ in 0..reps {
            fanned.extend(a_out.iter().copied());
        }
        self.lower(rhs, &fanned)
    }

    /// `A :> B` — sum groups of A's outputs into B's inputs, then lower B.
    fn lower_merge(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        args: &[usize],
    ) -> Result<Vec<usize>, CompileError> {
        let a_out = self.lower(lhs, args)?;
        let bi = if self.rhs_variadic(rhs) {
            a_out.len()
        } else {
            self.arity_in(rhs)?
        };
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

    /// `A op B` — elementwise arithmetic on the single output wire of each side.
    fn lower_arith(
        &mut self,
        op: ArithOp,
        lhs: &Expr,
        rhs: &Expr,
        args: &[usize],
    ) -> Result<Vec<usize>, CompileError> {
        if matches!(op, ArithOp::Add | ArithOp::Sub) {
            let re = match lhs {
                Expr::Float(v, _) => Some(*v),
                Expr::Int(v, _) => Some(*v as f64),
                _ => None,
            };
            let im = match rhs {
                Expr::Imag(v, _) => Some(if matches!(op, ArithOp::Sub) { -*v } else { *v }),
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
                        resource: None,
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
                        srcs: vec![],
                        instance,
                    });
                    return Ok((0..sig.signal_outs).map(|i| fst + i).collect());
                }
            }
        }
        let a = self.lower(lhs, args)?;
        let b = self.lower(rhs, args)?;
        let arith = match op {
            ArithOp::Add => BinArith::Add,
            ArithOp::Sub => BinArith::Sub,
            ArithOp::Mul => BinArith::Mul,
            ArithOp::Div => BinArith::Div,
            ArithOp::Rem => BinArith::Rem,
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

    /// `A ~ B` — B's output feeds A's feedback input (1-tick delay), while B is
    /// evaluated independently (it does not consume A's output). This models a
    /// unidirectional feedback edge.
    fn lower_feedback(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        args: &[usize],
        _span: Span,
    ) -> Result<Vec<usize>, CompileError> {
        let bo = self.arity_out(rhs)?;
        let mut fb_regs = Vec::with_capacity(bo);
        let mut slots = Vec::with_capacity(bo);
        for _ in 0..bo {
            let slot = self.block_state_slots;
            self.block_state_slots += 1;
            slots.push(slot);
            let dst = self.fresh_reg();
            self.emit(Instr::ReadBlockState { dst, slot });
            fb_regs.push(dst);
        }
        let mut a_in = args.to_vec();
        a_in.extend(fb_regs);
        let a_out = self.lower(lhs, &a_in)?;
        let b_out = self.lower(rhs, args)?;
        for (k, slot) in slots.iter().enumerate() {
            self.emit(Instr::WriteBlockState {
                slot: *slot,
                src: b_out[k],
            });
        }
        Ok(a_out)
    }

    /// Whether `rhs` is a built-in that takes variadic signal inputs.
    fn rhs_variadic(&self, rhs: &Expr) -> bool {
        match rhs {
            Expr::Apply { name, .. } | Expr::Ref(name, _) => self
                .sigs
                .builtin_sig(name)
                .map(|s| s.has_variadic_signal())
                .unwrap_or(false),
            _ => false,
        }
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
        if len as usize > crate::program::MAX_DELAY_LEN {
            return Err(CompileError::Type {
                msg: format!(
                    "delay length {len} exceeds MAX_DELAY_LEN ({})",
                    crate::program::MAX_DELAY_LEN
                ),
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

    fn arity_in(&self, e: &Expr) -> Result<usize, CompileError> {
        Ok(self.arity(e)?.0)
    }

    fn arity_out(&self, e: &Expr) -> Result<usize, CompileError> {
        Ok(self.arity(e)?.1)
    }

    /// Signal-arity `(ins, outs)` of an expression. User-defined references are
    /// resolved through `self.defs` so combinator fan-out/split uses the true
    /// arity of an open block.
    fn arity(&self, e: &Expr) -> Result<(usize, usize), CompileError> {
        self.arity_with(e, &mut HashSet::new())
    }

    fn arity_with(
        &self,
        e: &Expr,
        visited: &mut HashSet<String>,
    ) -> Result<(usize, usize), CompileError> {
        let _unsupported = |m: &str| CompileError::Unsupported(m.to_string());
        Ok(match e {
            Expr::Int(_, _) | Expr::Float(_, _) => (0, 1),
            Expr::Imag(_, _) => (0, 2),
            Expr::Str(_, _) => (0, 1),
            Expr::Wire(_) => (1, 1),
            Expr::Cut(_) => (1, 0),
            Expr::Neg(inner, _) => self.arity_with(inner, visited)?,
            Expr::Ref(name, _) => match name.as_str() {
                "+" | "-" | "*" | "/" | "%" | "min" | "max" => (2, 1),
                "sin" | "cos" | "tan" | "sqrt" | "exp" | "ln" | "tanh" | "abs" => (1, 1),
                _ => {
                    if let Some(sig) = self.sigs.builtin_sig(name) {
                        (sig.signal_ins(), sig.signal_outs)
                    } else if let Some(def) = self.defs.get(name) {
                        if visited.contains(name) {
                            (0, 1)
                        } else {
                            visited.insert(name.clone());
                            let a = self.arity_with(def.body(), visited)?;
                            visited.remove(name);
                            a
                        }
                    } else {
                        (0, 1)
                    }
                }
            },
            Expr::Apply { name, args, .. } => {
                if let Some(sig) = self.sigs.builtin_sig(name) {
                    (sig.signal_ins(), sig.signal_outs)
                } else {
                    let mut ins = 0;
                    for a in args {
                        ins += self.arity_with(a, visited)?.0;
                    }
                    (ins, 1)
                }
            }
            Expr::Seq(lhs, rhs, _) => {
                let (ai, _) = self.arity_with(lhs, visited)?;
                let (_, bo) = self.arity_with(rhs, visited)?;
                (ai, bo)
            }
            Expr::Par(lhs, rhs, _) => {
                let (ai, ao) = self.arity_with(lhs, visited)?;
                let (bi, bo) = self.arity_with(rhs, visited)?;
                (ai + bi, ao + bo)
            }
            Expr::Split(lhs, rhs, _) => {
                let (ai, _) = self.arity_with(lhs, visited)?;
                let (_, bo) = self.arity_with(rhs, visited)?;
                (ai, bo)
            }
            Expr::Merge(lhs, rhs, _) => {
                let (ai, _) = self.arity_with(lhs, visited)?;
                let (_, bo) = self.arity_with(rhs, visited)?;
                (ai, bo)
            }
            Expr::Loop(lhs, rhs, _) => {
                let (ai, ao) = self.arity_with(lhs, visited)?;
                let (_, bo) = self.arity_with(rhs, visited)?;
                (ai - bo, ao)
            }
            Expr::Delay(lhs, _rhs, _) => self.arity_with(lhs, visited)?,
            Expr::Arith { lhs, rhs, .. } => {
                let (ai, _) = self.arity_with(lhs, visited)?;
                let (bi, _) = self.arity_with(rhs, visited)?;
                (ai + bi, 1)
            }
            Expr::Let { body, .. } => self.arity_with(body, visited)?,
            Expr::Record(..) => unreachable!("Record should be desugared before arity check"),
            Expr::ActorParam { .. } => (0, 1),
        })
    }
}

fn const_f64(e: &Expr) -> Option<f64> {
    match e {
        Expr::Float(v, _) => Some(*v),
        Expr::Int(v, _) => Some(*v as f64),
        Expr::Neg(inner, _) => const_f64(inner).map(|v| -v),
        Expr::Arith { op, lhs, rhs, .. } => {
            let a = const_f64(lhs)?;
            let b = const_f64(rhs)?;
            Some(match op {
                ArithOp::Add => a + b,
                ArithOp::Sub => a - b,
                ArithOp::Mul => a * b,
                ArithOp::Div => a / b,
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
        Expr::Arith { op, lhs, rhs, .. } => {
            let a = const_int(lhs)?;
            let b = const_int(rhs)?;
            Some(match op {
                ArithOp::Add => a + b,
                ArithOp::Sub => a - b,
                ArithOp::Mul => a * b,
                ArithOp::Div if b != 0 => a / b,
                ArithOp::Rem if b != 0 => a % b,
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
    lower_with_cafs(tp, sigs, sample_rate, &HashSet::new())
}

/// Like [`lower_with`], but treats the given names as closed CAFs that are
/// lifted once and shared across reference sites.
pub fn lower_with_cafs(
    tp: &TypedProgram,
    sigs: &dyn SignatureSource,
    sample_rate: f32,
    cafs: &HashSet<String>,
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
        cafs,
        caf_cache: HashMap::new(),
        caf_lifting: HashSet::new(),
        instrs: Vec::new(),
        next_reg: 0,
        block_state_slots: 0,
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
    if outs.is_empty() {
        return Err(CompileError::Unsupported(
            "body lowered to 0 outputs, expected at least 1".into(),
        ));
    }
    let num_outputs = outs.len();
    Ok(Ir {
        instrs: lw.instrs,
        num_regs: lw.next_reg,
        output_regs: outs,
        num_inputs,
        num_outputs,
        state: StateLayout {
            block_state_slots: lw.block_state_slots,
            delay_lens: lw.delay_lens,
            num_outputs,
        },
        builtins: lw.builtins,
        params: lw.params,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::BuiltinKind;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::reduce::reduce_with_cafs;
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
                    BuiltinKind::Block,
                )))),
                "sine" => Some(Box::leak(Box::new(BuiltinSig::simple(
                    "sine",
                    0,
                    1,
                    3,
                    BuiltinKind::Block,
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

    fn ir_with_cafs(src: &str) -> Ir {
        let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
        let typed = infer_program_with(&p, &TestSigs).unwrap();
        let cafs = typed.cafs.clone();
        let reduced = reduce_with_cafs(&typed.program, &cafs);
        let tp = crate::types::infer::TypedProgram {
            program: reduced,
            process_ty: typed.process_ty,
            cafs,
        };
        lower_with_cafs(&tp, &TestSigs, 44_100.0, &tp.cafs).unwrap()
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
        assert_eq!(ir.state.block_state_slots, 1);
        assert!(ir
            .instrs
            .iter()
            .any(|i| matches!(i, Instr::ReadBlockState { .. })));
        assert!(ir
            .instrs
            .iter()
            .any(|i| matches!(i, Instr::WriteBlockState { .. })));
    }

    #[test]
    fn delay_allocates_line() {
        let ir = ir_of("main = _ @ 3");
        assert_eq!(ir.state.delay_lens, vec![3]);
    }

    #[test]
    fn delay_over_max_is_compile_error() {
        // `@ 70000` exceeds MAX_DELAY_LEN (65536): must be a compile error, not
        // a construction-time panic in DelayRing::new.
        let p = parse(
            &tokenize("main = _ @ 70000").unwrap(),
            "main = _ @ 70000".as_bytes(),
        )
        .unwrap();
        let tp = infer_program(&p).unwrap();
        assert!(lower(&tp).is_err());
    }

    #[test]
    fn onepole_lowers_to_callblock() {
        let ir = ir_with("main = _ : onepole 200.0 0.5");
        assert!(
            ir.instrs
                .iter()
                .any(|i| matches!(i, Instr::CallBlock { .. })),
            "expected a CallBlock instruction"
        );
        assert_eq!(ir.builtins.len(), 1);
        let bi = &ir.builtins[0];
        assert_eq!(bi.kind, BuiltinKind::Block);
        assert_eq!(bi.params, vec![200.0, 0.5]);
    }

    #[test]
    fn block_builtin_lowers_to_callblock() {
        let ir = ir_with("main = _ : lowpass 1000.0 0.7");
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
        let ir = ir_of("main = smooth _ 10.0");
        assert_eq!(ir.state.block_state_slots, 1);
        assert!(ir
            .instrs
            .iter()
            .any(|i| matches!(i, Instr::ReadBlockState { .. })));
        assert!(ir
            .instrs
            .iter()
            .any(|i| matches!(i, Instr::WriteBlockState { .. })));
    }

    #[test]
    fn closed_caf_lowers_to_single_instance() {
        // osc = sine 440 0.5 0; main = osc , osc  -> ONE sine builtin
        let ir = ir_with_cafs("osc = sine 440 0.5 0; main = osc , osc");
        let sines = ir.builtins.iter().filter(|b| b.name == "sine").count();
        assert_eq!(sines, 1);
    }

    #[test]
    fn open_block_stays_macro() {
        // integ = + ~ _; main = integ , integ  -> 2 state slots (two independent integrators)
        let ir = ir_of("integ = + ~ _; main = integ , integ");
        assert_eq!(ir.state.block_state_slots, 2);
    }

    #[test]
    fn caf_const_param_still_folds() {
        // cutoff = 1000.0; main = _ : lowpass cutoff 0.7 -> param 1000.0
        let ir = ir_with_cafs("cutoff = 1000.0; main = _ : lowpass cutoff 0.7");
        let lp = ir.builtins.iter().find(|b| b.name == "lowpass").unwrap();
        assert!((lp.params[0] - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn transitive_caf_const_fold() {
        // b = 1000.0; a = b; main = _ : lowpass a 0.7 -> a resolves through b to 1000.0
        let ir = ir_with_cafs("b = 1000.0; a = b; main = _ : lowpass a 0.7");
        let lp = ir.builtins.iter().find(|b| b.name == "lowpass").unwrap();
        assert!((lp.params[0] - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn non_folding_caf_in_builtin_param_is_error() {
        // osc = sine 440 0.5 0 is a CAF whose body is not a constant: using it in
        // a builtin param position must error, not silently become 0.
        let p = parse(
            &tokenize("osc = sine 440 0.5 0; main = _ : lowpass osc 0.7").unwrap(),
            "osc = sine 440 0.5 0; main = _ : lowpass osc 0.7".as_bytes(),
        )
        .unwrap();
        let typed = infer_program_with(&p, &TestSigs).unwrap();
        let cafs = typed.cafs.clone();
        let reduced = reduce_with_cafs(&typed.program, &cafs);
        let tp = crate::types::infer::TypedProgram {
            program: reduced,
            process_ty: typed.process_ty,
            cafs: cafs.clone(),
        };
        let res = lower_with_cafs(&tp, &TestSigs, 44100.0, &cafs);
        assert!(res.is_err());
    }

    #[test]
    fn mutually_recursive_cafs_are_errors() {
        // a = b; b = a  -> the reference cycle must surface as a compile error,
        // not a stack overflow.
        let p = parse(
            &tokenize("a = b; b = a; main = a").unwrap(),
            "a = b; b = a; main = a".as_bytes(),
        )
        .unwrap();
        let typed = infer_program(&p).unwrap();
        let cafs = typed.cafs.clone();
        let reduced = reduce_with_cafs(&typed.program, &cafs);
        let tp = crate::types::infer::TypedProgram {
            program: reduced,
            process_ty: typed.process_ty,
            cafs: cafs.clone(),
        };
        let res = lower_with_cafs(&tp, &crate::builtin::NoSigs, 44100.0, &cafs);
        assert!(res.is_err());
    }

    #[test]
    fn cyclic_caf_in_builtin_param_is_error() {
        // a = b; b = a; main = _ : lowpass a 0.7
        // The cycle is reached through a BUILTIN PARAM, exercising caf_const_impl's
        // visited-set guard (not the caf_lifting guard in lower_ref).
        let p = parse(
            &tokenize("a = b; b = a; main = _ : lowpass a 0.7").unwrap(),
            "a = b; b = a; main = _ : lowpass a 0.7".as_bytes(),
        )
        .unwrap();
        let typed = infer_program_with(&p, &TestSigs).unwrap();
        let cafs = typed.cafs.clone();
        let reduced = reduce_with_cafs(&typed.program, &cafs);
        let tp = crate::types::infer::TypedProgram {
            program: reduced,
            process_ty: typed.process_ty,
            cafs: cafs.clone(),
        };
        let res = lower_with_cafs(&tp, &TestSigs, 44100.0, &cafs);
        assert!(res.is_err());
    }

    #[test]
    fn unreferenced_caf_is_not_lowered() {
        // dead = sine 440 0.5 0; main = _ * 0.5  -> no sine builtin
        let ir = ir_with_cafs("dead = sine 440 0.5 0; main = _ * 0.5");
        assert!(!ir.builtins.iter().any(|b| b.name == "sine"));
    }

    #[test]
    fn recursive_caf_is_error_not_overflow() {
        // a = a  -> compile error, not stack overflow
        let p = parse(
            &tokenize("a = a; main = a").unwrap(),
            "a = a; main = a".as_bytes(),
        )
        .unwrap();
        let typed = infer_program(&p).unwrap();
        let cafs = typed.cafs.clone();
        let reduced = reduce_with_cafs(&typed.program, &cafs);
        let tp = crate::types::infer::TypedProgram {
            program: reduced,
            process_ty: typed.process_ty,
            cafs: cafs.clone(),
        };
        let res = lower_with_cafs(&tp, &crate::builtin::NoSigs, 44100.0, &cafs);
        assert!(res.is_err());
    }
}
