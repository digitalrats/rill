//! Lower a type-checked program to linear IR.

use std::collections::{HashMap, HashSet};

use crate::ast::{ArithOp, Def, Expr, MatchArm, Param, Pattern, Program};
use crate::builtin::{ParamType, SignatureSource};
use crate::error::{CompileError, Span};
use crate::ir::{
    BinArith, BuiltinInstance, CmpOp, FragmentIr, FuncSig, Instr, Ir, LogicOp, ParamDef,
    StateLayout, UnOp, ValueBlock, ValueBuiltinOp, ValueInstr, ValueLayout, ValueTerm,
};
use crate::types::infer::TypedProgram;
use crate::types::ty::{DataInfo, Rate, TypeEnv, ValueTy};

/// Whether a value instruction allocates a fresh arena slot when executed.
///
/// Drives the v1 `ValueLayout::capacity` heuristic: value registers are per-tick
/// scratch (cleared at tick end by `clear_value_regs`), so the maximum number of
/// simultaneously-live slots is bounded by the total number of allocations one
/// tick can issue. Every instruction below allocates at most one slot per tick;
/// `ValueUpdateField`'s COW copy is net-zero (a fresh slot replaces the
/// original), so counting it is a conservative over-approximation that also
/// covers the transient extra slot mid-COW. `ValueCallFunc` counts too: its
/// `dst` result is a fresh `copy` of the fragment's output.
fn is_alloc_producing(i: &ValueInstr) -> bool {
    matches!(
        i,
        ValueInstr::ValueConstInt { .. }
            | ValueInstr::ValueConstFloat { .. }
            | ValueInstr::ValueAdd { .. }
            | ValueInstr::ValueSub { .. }
            | ValueInstr::ValueMul { .. }
            | ValueInstr::ValueDiv { .. }
            | ValueInstr::ValueConstructRecord { .. }
            | ValueInstr::ValueConstructSum { .. }
            | ValueInstr::ValueNewtype { .. }
            | ValueInstr::ValueBindCell { .. }
            | ValueInstr::ValueReadCell { .. }
            | ValueInstr::ValueReadMainCell { .. }
            | ValueInstr::ValueStateRead { .. }
            | ValueInstr::ValueUpdateField { .. }
            | ValueInstr::ValueMakeClosure { .. }
            | ValueInstr::ValueCallFunc { .. }
            | ValueInstr::ValueBool { .. }
            | ValueInstr::ValueConstString { .. }
            | ValueInstr::ValueListLit { .. }
            | ValueInstr::ValueMapLit { .. }
            | ValueInstr::ValueCompare { .. }
            | ValueInstr::ValueLogic { .. }
            | ValueInstr::ValueNot { .. }
            | ValueInstr::ValueCallBuiltin { .. }
    )
}

/// The type arguments of a `List` value type (`[elem, Cap(n)]`), if `t` is one.
fn list_type_args(t: &ValueTy) -> Option<&Vec<ValueTy>> {
    match t {
        ValueTy::App(name, inner) if name == "List" => Some(inner),
        _ => None,
    }
}

/// The capacity field of a container value type (`List`, `Map`, `Set`), read
/// from its trailing `Cap(n)` argument; 0 for a non-container.
fn container_cap(t: &ValueTy) -> usize {
    match t {
        ValueTy::App(name, inner) if matches!(name.as_str(), "List" | "Map" | "Set") => {
            match inner.last() {
                Some(ValueTy::Cap(n)) => *n,
                _ => 0,
            }
        }
        _ => 0,
    }
}

/// Map a parsed comparison operator to its IR form (the two enums share their
/// variant names, so the match is mechanical).
fn cmp_op_from_ast(op: crate::ast::CmpOp) -> CmpOp {
    use crate::ast::CmpOp as A;
    use crate::ir::CmpOp as I;
    match op {
        A::Eq => I::Eq,
        A::Ne => I::Ne,
        A::Lt => I::Lt,
        A::Gt => I::Gt,
        A::Le => I::Le,
        A::Ge => I::Ge,
    }
}

/// Map a parsed logic operator to its IR form.
fn logic_op_from_ast(op: crate::ast::LogicOp) -> LogicOp {
    use crate::ast::LogicOp as A;
    use crate::ir::LogicOp as I;
    match op {
        A::And => I::And,
        A::Or => I::Or,
    }
}

struct Lowerer<'a> {
    defs: HashMap<String, Def>,
    sigs: &'a dyn SignatureSource,
    cafs: &'a HashSet<String>,
    /// Lowered registers of each lifted CAF, keyed by `(name, call-site args)`.
    /// Task 5 threads the caller's signal args into a value-CAF's body (a
    /// signal-lambda macro), so the lowered body is SITE-DEPENDENT: two sites
    /// applying the same CAF to different wires must each lower with their own
    /// wire. Genuine signal CAFs (empty signal-ins) never consume the threaded
    /// args, so identical-args sites share the entry; distinct-arg sites get
    /// separate (still correct) copies.
    caf_cache: HashMap<(String, Vec<usize>), Vec<usize>>,
    caf_lifting: HashSet<String>,
    instrs: Vec<Instr>,
    next_reg: usize,
    block_state_slots: usize,
    delay_lens: Vec<usize>,
    locals: Vec<HashMap<String, Vec<usize>>>,
    builtins: Vec<BuiltinInstance>,
    params: Vec<ParamDef>,
    param_names: HashMap<String, usize>,
    /// main λ-parameter cells: name → persistent cell index. These are NOT
    /// per-tick value bindings — the cells are allocated once at program
    /// construction ([`Ir::num_main_cells`]), so `SetParameter` writes persist
    /// across ticks. Block-track reads materialise the cell's float via
    /// [`Instr::ReadMainCell`]; value-track reads via `ValueReadMainCell`.
    main_cell_locals: HashMap<String, usize>,
    sample_rate: f32,
    /// Value-track blocks, executed once per tick (see `run_value_track`).
    value_blocks: Vec<ValueBlock>,
    /// The block currently being appended to.
    cur_value_block: usize,
    /// Entry block of the program's value track (always 0).
    value_entry: usize,
    /// Next value register index (SSA value registers are per-tick scratch).
    next_value_reg: usize,
    /// Value registers holding the program's value outputs (value-channel main).
    value_regs_out: Vec<usize>,
    /// Static value type of each value output, parallel to [`Self::value_regs_out`].
    /// The arena-capacity heuristic needs it: a compound output (record/sum/
    /// newtype) pins its whole subtree across ticks, so the bound must account
    /// for the output's subtree size, not just one slot per channel.
    value_out_tys: Vec<ValueTy>,
    /// Static types of every container-typed value subexpression (`List`,
    /// `Map`, `Set`) produced during lowering. A container pins its element
    /// slots while it lives, so the arena bound must sum each container's
    /// subtree size (the `is_alloc_producing` count alone credits one slot per
    /// op and undercounts `map`/`filter`/`cons`/literal containers by their
    /// element slots). With `value_builtin_ty` propagating source caps, these
    /// types carry the exact capacities the runtime allocates.
    container_tys: Vec<ValueTy>,
    /// First-class function values referenced by [`ValueInstr::ValueMakeClosure`].
    /// A bare reference to a user definition in value position allocates a
    /// [`Value::Closure`] referencing the entry's index.
    value_funcs: Vec<crate::ir::ValueFunc>,
    /// Scope stack of value locals: name → (value reg, value type). Match-arm
    /// bindings (and, in a later task, `main` λ-params) are aliased by name to
    /// per-tick value registers — a `Ref` to one returns the register directly.
    value_locals: Vec<HashMap<String, (usize, ValueTy)>>,
    /// Statically-known constructor of each value local, parallel to the scope
    /// stack [`Self::value_locals`]: a method param bound to `Just 1.0` records
    /// `Some("Just")`, so a `match` over the param's `Ref` selects its arm at
    /// compile time (v1 static dispatch). `None` when the local's ctor is not
    /// statically known.
    value_local_ctors: Vec<HashMap<String, Option<String>>>,
    /// The shared compile-time type environment (aliases, newtypes, data-type
    /// shapes) carried from inference — the single source of truth for name
    /// resolution in both infer and lower.
    env: &'a TypeEnv,
    /// Recursion guard while inlining value definitions.
    value_inline: HashSet<String>,
    /// Recursion guard for typeclass method inlining: resolved (class, type,
    /// method) calls currently on the expansion path.
    method_lifting: HashSet<(String, String, String)>,
    /// Compiled function bodies (lambda literals), indexed by
    /// [`ValueInstr::ValueMakeClosure`]'s `fragment` field.
    fragments: Vec<std::sync::Arc<FragmentIr>>,
    /// Free-variable names of the lambda currently being lowered into a
    /// fragment (empty at the program level). A `Ref` to one of these inside
    /// the fragment emits a `ValueReadCell { cell: capture_index }` reading the
    /// call's env frame.
    fragment_captures: Vec<String>,
    /// Resolved λ-parameter value types of every named lambda-literal
    /// definition (from inference). Lowering types fragment-local parameter
    /// registers with these: a higher-order parameter is a `Func` (so the
    /// callee dispatches), a record parameter its `Data` type (so field
    /// projection resolves indices).
    fn_param_tys: HashMap<String, Vec<ValueTy>>,
    /// Parameter types for the lambda literal currently being lowered, when it
    /// is a NAMED lambda-literal definition. Consumed by the `Expr::Lambda`
    /// arm; anonymous lambdas leave it unset and default to `Float`.
    pending_param_tys: Option<Vec<ValueTy>>,
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

    fn fresh_value_reg(&mut self) -> usize {
        let r = self.next_value_reg;
        self.next_value_reg += 1;
        r
    }

    fn emit_value(&mut self, i: ValueInstr) {
        self.value_blocks[self.cur_value_block].instrs.push(i);
    }

    fn new_value_block(&mut self) -> usize {
        self.value_blocks.push(ValueBlock::default());
        self.value_blocks.len() - 1
    }

    fn set_value_term(&mut self, block: usize, term: ValueTerm) {
        self.value_blocks[block].term = term;
    }

    /// Record a container-typed subexpression (`List`/`Map`/`Set`) for the
    /// arena-capacity heuristic: the container pins its element slots while it
    /// lives, so the bound must include its full subtree size.
    fn note_container(&mut self, vty: &ValueTy) {
        if matches!(
            vty,
            ValueTy::App(name, _) if matches!(name.as_str(), "List" | "Map" | "Set")
        ) {
            self.container_tys.push(vty.clone());
        }
    }

    /// Worst-case element estimate for open collections. Exact counts are
    /// data-dependent (a `concat_map` result length is unknown statically), so
    /// this is a conservative per-op estimate; the runtime safety net is pool
    /// exhaustion (default mode) or pool growth (`growable-arena`).
    const ELEM_EST: usize = 16;

    /// Lower a value expression to a value register, returning its static value
    /// type. Signal expressions lower to block registers through [`Self::lower`];
    /// value expressions (record/field/match/ctor/literal-in-value-position)
    /// lower here and the two tracks never mix.
    ///
    /// The returned type is the static `ValueTy` of the expression — inference
    /// keeps value channels monomorphic, so this is exact. It is threaded
    /// through because field indices (records) and constructor indices (sums)
    /// depend on the concrete data type of the value.
    fn lower_value(&mut self, e: &Expr) -> Result<(usize, ValueTy), CompileError> {
        match e {
            Expr::Int(v, _) => {
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueConstInt { dst, value: *v });
                Ok((dst, ValueTy::Int))
            }
            Expr::Float(v, _) => {
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueConstFloat { dst, value: *v });
                Ok((dst, ValueTy::Float))
            }
            Expr::Wire(_) => {
                // v1 has no runtime value inputs: a value wire (`_` in match
                // position) lowers to an unbound register, so any `ValueMatch`
                // against it is a detectable non-match (None). The type is
                // unknown until the surrounding match pins it.
                let dst = self.fresh_value_reg();
                Ok((dst, ValueTy::Var(0)))
            }
            Expr::Ref(name, span) => self.lower_value_ref(name, *span),
            Expr::FieldProject {
                record,
                field,
                span,
            } => {
                let (rec_reg, rec_vty) = self.lower_value(record)?;
                let rec_name = match &rec_vty {
                    ValueTy::Data(n, _) => n.clone(),
                    // Builtin records (`Pair a b`) are parameterized as `App`.
                    ValueTy::App(n, _) if self.env.ctor_arity(n).is_some() => n.clone(),
                    _ => {
                        return Err(CompileError::Type {
                            msg: "field projection requires a record value".into(),
                            span: *span,
                        });
                    }
                };
                let fields = match self.env.data_types.get(&rec_name) {
                    Some(DataInfo::Record(fields)) => fields.clone(),
                    _ => {
                        return Err(CompileError::Type {
                            msg: format!("`{rec_name}` is not a record type"),
                            span: *span,
                        });
                    }
                };
                let field_index = fields.iter().position(|(f, _)| f == field).ok_or_else(|| {
                    CompileError::Type {
                        msg: format!("no field `{field}` in `{rec_name}`"),
                        span: *span,
                    }
                })?;
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueProject {
                    dst,
                    slot: rec_reg,
                    field: field_index,
                });
                Ok((dst, fields[field_index].1.clone()))
            }
            Expr::FieldUpdate {
                record,
                field,
                value,
                span,
            } => {
                let (rec_reg, rec_vty) = self.lower_value(record)?;
                let rec_name = match &rec_vty {
                    ValueTy::Data(n, _) => n.clone(),
                    // Builtin records (`Pair a b`) are parameterized as `App`.
                    ValueTy::App(n, _) if self.env.ctor_arity(n).is_some() => n.clone(),
                    _ => {
                        return Err(CompileError::Type {
                            msg: "field update requires a data record".into(),
                            span: *span,
                        });
                    }
                };
                let fields = match self.env.data_types.get(&rec_name) {
                    Some(DataInfo::Record(fields)) => fields.clone(),
                    _ => {
                        return Err(CompileError::Type {
                            msg: format!("`{rec_name}` is not a record type"),
                            span: *span,
                        });
                    }
                };
                let field_index = fields.iter().position(|(f, _)| f == field).ok_or_else(|| {
                    CompileError::Type {
                        msg: format!("no field `{field}` in `{rec_name}`"),
                        span: *span,
                    }
                })?;
                let (src, _) = self.lower_value(value)?;
                self.emit_value(ValueInstr::ValueUpdateField {
                    slot: rec_reg,
                    field: field_index,
                    src,
                });
                Ok((rec_reg, rec_vty))
            }
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => {
                let (scrutinee_reg, scrutinee_vty) = self.lower_value(scrutinee)?;
                // The sum type: a concrete scrutinee type pins it; otherwise the
                // arm constructors determine it (mirroring inference's
                // intersection of candidate sum types). A scalar match (only
                // literal/wildcard/variable arms) has no sum.
                let sum_name = match &scrutinee_vty {
                    ValueTy::Data(n, _) => Some(n.clone()),
                    _ if arms
                        .iter()
                        .any(|a| matches!(a.pattern, Pattern::Ctor(_, _))) =>
                    {
                        Some(self.resolve_match_sum(arms, *span)?)
                    }
                    _ => None,
                };
                let ctors: Vec<(String, Vec<ValueTy>)> = match &sum_name {
                    Some(n) => match self.env.data_types.get(n) {
                        Some(DataInfo::Sum(ctors)) => ctors.clone(),
                        _ => {
                            return Err(CompileError::Type {
                                msg: format!("`{n}` is not a sum type"),
                                span: *span,
                            });
                        }
                    },
                    None => Vec::new(),
                };
                if arms.is_empty() {
                    return Err(CompileError::Type {
                        msg: "match requires at least one arm".into(),
                        span: *span,
                    });
                }
                // Builtin sums are parameterized (`Maybe a`, `Either a b`): the
                // arm payload shapes carry placeholder `Var(k)` type params that
                // resolve against the scrutinee's concrete type args by position
                // (mirroring inference). Lowering gives a direct builtin-sum
                // construction (`Just x`) the type `Data(name, [])`, so the args
                // are recovered from the constructor application when the static
                // type carries none.
                let is_builtin_sum = sum_name
                    .as_ref()
                    .map(|n| self.env.ctor_arity(n).is_some())
                    .unwrap_or(false);
                let scrutinee_args: Vec<ValueTy> = match &scrutinee_vty {
                    ValueTy::App(_, args) => args.clone(),
                    ValueTy::Data(_, args) if !args.is_empty() => args.clone(),
                    _ if is_builtin_sum => self.builtin_sum_scrutinee_args(scrutinee),
                    _ => vec![],
                };
                // A guarded arm can fail its guard at runtime, so the static
                // fast path (compile-time ctor selection) is only sound for
                // guard-free matches.
                let has_guards = arms.iter().any(|a| {
                    a.guards.len() > 1
                        || !matches!(a.guards.first(), Some((Expr::Bool(true, _), _)))
                });
                // Static fast path (guard-free, statically-known ctor): lower
                // ONLY the matching arm — preserves exact-capacity behavior.
                if !has_guards {
                    if let Some(cname) = self.static_scrutinee_ctor(scrutinee.as_ref()) {
                        // The static path binds flat `Ctor(_, [Var/Wild ...])`
                        // patterns (plus Var/Wild arms) in one pass; a
                        // nested-pattern arm needs recursive runtime payload
                        // extraction, so it is routed to runtime dispatch.
                        let statically_lowerable = arms.iter().all(|arm| match &arm.pattern {
                            Pattern::Ctor(_, args) => args
                                .iter()
                                .all(|a| matches!(a, Pattern::Var(_) | Pattern::Wild)),
                            Pattern::Var(_) | Pattern::Wild => true,
                            _ => true,
                        });
                        if statically_lowerable {
                            let static_sum = sum_name.clone().unwrap_or_default();
                            return self.lower_static_match_arm(
                                scrutinee_reg,
                                &scrutinee_vty,
                                &static_sum,
                                &ctors,
                                &scrutinee_args,
                                is_builtin_sum,
                                arms,
                                &cname,
                                *span,
                            );
                        }
                    }
                }
                // Runtime dispatch: a sequential test chain per arm ending in a
                // fail block that latches a ProcessError when no arm matched.
                let scrutinee_block = self.cur_value_block;
                let out = self.fresh_value_reg();
                let join = self.new_value_block();
                let fail = self.new_value_block();
                // fail block: latch the error, then halt the value track.
                self.cur_value_block = fail;
                self.emit_value(ValueInstr::ValueSetError);
                self.set_value_term(fail, ValueTerm::Halt);
                let n = arms.len();
                // All arm-entry blocks are created up front so the `else`
                // target of every arm is known before any body is lowered.
                let entries: Vec<usize> = (0..n).map(|_| self.new_value_block()).collect();
                // The scrutinee's block falls through into the first arm's
                // entry; the last arm's else is the fail block.
                self.set_value_term(scrutinee_block, ValueTerm::Fallthrough(entries[0]));
                let mut out_ty: Option<ValueTy> = None;
                for i in 0..n {
                    let else_t = if i + 1 < n { entries[i + 1] } else { fail };
                    let arm = &arms[i];
                    self.cur_value_block = entries[i];
                    // One scope per arm: pushed before the pattern test so the
                    // arm's `Var` bindings are visible to its guards + body, and
                    // popped after the body. Only one arm runs at runtime, but
                    // all arms are lowered at compile time.
                    self.value_locals.push(HashMap::new());
                    self.value_local_ctors.push(HashMap::new());
                    let pass = self.lower_pattern_test(
                        &arm.pattern,
                        scrutinee_reg,
                        &scrutinee_vty,
                        &scrutinee_args,
                        else_t,
                        arm.span,
                    )?;
                    self.cur_value_block = pass;
                    let arm_ty = self.lower_match_guards_and_body(arm, &out, join, else_t)?;
                    self.value_locals.pop();
                    self.value_local_ctors.pop();
                    if out_ty.is_none() {
                        out_ty = Some(arm_ty);
                    }
                }
                self.cur_value_block = join;
                Ok((out, out_ty.unwrap_or(ValueTy::Float)))
            }
            Expr::If {
                cond,
                then,
                els,
                span,
            } => {
                // Static fast path: a literal Bool condition selects one branch
                // at compile time (single block, no Branch term).
                if let Expr::Bool(b, _) = cond.as_ref() {
                    let (reg, ty) = if *b {
                        self.lower_value(then)?
                    } else {
                        self.lower_value(els)?
                    };
                    return Ok((reg, ty));
                }
                let (cond_reg, cond_ty) = self.lower_value(cond)?;
                if cond_ty != ValueTy::Bool {
                    return Err(CompileError::Type {
                        msg: "if condition must be a Bool value".into(),
                        span: *span,
                    });
                }
                let then_b = self.new_value_block();
                let els_b = self.new_value_block();
                let join = self.new_value_block();
                let out = self.fresh_value_reg();
                self.set_value_term(
                    self.cur_value_block,
                    ValueTerm::Branch {
                        cond: cond_reg,
                        then: then_b,
                        els: els_b,
                    },
                );
                self.cur_value_block = then_b;
                let (t_reg, t_ty) = self.lower_value(then)?;
                self.emit_value(ValueInstr::ValueMove {
                    dst: out,
                    src: t_reg,
                });
                self.set_value_term(self.cur_value_block, ValueTerm::Fallthrough(join));
                self.cur_value_block = els_b;
                let (e_reg, e_ty) = self.lower_value(els)?;
                self.emit_value(ValueInstr::ValueMove {
                    dst: out,
                    src: e_reg,
                });
                self.set_value_term(self.cur_value_block, ValueTerm::Fallthrough(join));
                if t_ty != e_ty {
                    return Err(CompileError::Type {
                        msg: "if branches must have the same type".into(),
                        span: *span,
                    });
                }
                self.cur_value_block = join;
                Ok((out, t_ty))
            }
            Expr::Apply {
                name,
                args: call_args,
                span,
            } => {
                // Boolean negation: `not b` — a prefix value-track builtin
                // (the `!` token stays the signal wire-cut combinator).
                if name == "not" {
                    if call_args.len() != 1 {
                        return Err(CompileError::Type {
                            msg: format!("`not` expects 1 argument, got {}", call_args.len()),
                            span: *span,
                        });
                    }
                    let (src, ty) = self.lower_value(&call_args[0])?;
                    if ty != ValueTy::Bool {
                        return Err(CompileError::Type {
                            msg: format!("`not` expects a Bool argument, got {ty:?}"),
                            span: call_args[0].span(),
                        });
                    }
                    let dst = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueNot { dst, src });
                    return Ok((dst, ValueTy::Bool));
                }
                // Collection operation (`length`, `cons`, `head`, `tail`, `map`,
                // `fold`, `filter`, `list`, `insert`, `lookup`, `member`,
                // `empty_map`, `empty_set`): a reserved name dispatched by
                // `ValueCallBuiltin`. Checked before the record/sum/newtype/
                // method/user-def resolution so a collection op can never be
                // shadowed by a definition of the same name.
                if let Some(op) = self.value_builtin(name, call_args.len()) {
                    let mut arg_regs = Vec::with_capacity(call_args.len());
                    let mut arg_tys = Vec::with_capacity(call_args.len());
                    for a in call_args {
                        let (r, t) = self.lower_value(a)?;
                        arg_regs.push(r);
                        arg_tys.push(t);
                    }
                    let ret = self.value_builtin_ty(name, &arg_tys, *span)?;
                    self.note_container(&ret);
                    let dst = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueCallBuiltin {
                        dst,
                        op,
                        args: arg_regs,
                    });
                    return Ok((dst, ret));
                }
                // Record constructor: `Point { x: 1.0 }`.
                if let Some(info) = self.env.data_types.get(name).cloned() {
                    match info {
                        DataInfo::Record(fields) => {
                            if call_args.len() != 1 {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "record constructor `{name}` expects one record literal, got {} arguments",
                                        call_args.len()
                                    ),
                                    span: *span,
                                });
                            }
                            let fields_expr = match &call_args[0] {
                                Expr::Record(f, _) => f,
                                _ => {
                                    return Err(CompileError::Type {
                                        msg: format!(
                                            "record constructor `{name}` expects a record literal"
                                        ),
                                        span: *span,
                                    });
                                }
                            };
                            // Lower each field value in declared-field order so the
                            // runtime record matches the type's field layout.
                            let mut field_regs = Vec::with_capacity(fields.len());
                            for (fname, _) in &fields {
                                let fexpr = fields_expr
                                    .iter()
                                    .find(|(n, _)| n == fname)
                                    .map(|(_, e)| e)
                                    .ok_or_else(|| CompileError::Type {
                                        msg: format!(
                                            "missing field `{fname}` in `{name}` constructor"
                                        ),
                                        span: *span,
                                    })?;
                                let (fr, _) = self.lower_value(fexpr)?;
                                field_regs.push(fr);
                            }
                            let dst = self.fresh_value_reg();
                            self.emit_value(ValueInstr::ValueConstructRecord {
                                dst,
                                fields: field_regs,
                            });
                            return Ok((dst, ValueTy::Data(name.clone(), vec![])));
                        }
                        DataInfo::Sum(_) => {
                            return Err(CompileError::Type {
                                msg: format!("`{name}` is a sum type; use one of its constructors"),
                                span: *span,
                            });
                        }
                    }
                }
                // Sum constructor: `Circle 1.5`.
                if let Some((sum_name, ctor_idx, payload)) = self.sum_ctor(name) {
                    if call_args.len() != payload.len() {
                        return Err(CompileError::Type {
                            msg: format!(
                                "constructor `{name}` expects {} argument(s), got {}",
                                payload.len(),
                                call_args.len()
                            ),
                            span: *span,
                        });
                    }
                    let mut payload_regs = Vec::with_capacity(call_args.len());
                    for a in call_args {
                        let (pr, _) = self.lower_value(a)?;
                        payload_regs.push(pr);
                    }
                    let dst = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueConstructSum {
                        dst,
                        ctor: ctor_idx as u32,
                        payload: payload_regs,
                    });
                    return Ok((dst, ValueTy::Data(sum_name, vec![])));
                }
                // Newtype constructor: `Hz 440.0` wraps its single argument.
                if self.env.newtypes.contains_key(name) {
                    if call_args.len() != 1 {
                        return Err(CompileError::Type {
                            msg: format!(
                                "newtype constructor `{name}` expects 1 argument, got {}",
                                call_args.len()
                            ),
                            span: *span,
                        });
                    }
                    let (src, _) = self.lower_value(&call_args[0])?;
                    let dst = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueNewtype { dst, src });
                    return Ok((dst, ValueTy::Newtype(name.clone(), vec![])));
                }
                // Typeclass method call: the argument's static type selects the
                // instance, and the method body is inlined with the parameters
                // bound to the argument's registers — β-substitution at compile
                // time, zero runtime dispatch. User definitions shadow class
                // methods (a user `eq`/`lt` is a plain function), so this only
                // fires when no user def of that name exists. Constructor
                // classes (`Functor f`) resolve by the class-var-applied
                // argument's constructor head (`App("List", ..)` → `List`).
                if !self.defs.contains_key(name) {
                    if let Some(class_name) = self.env.class_of_method(name) {
                        let class_info = self.env.typeclasses.get(&class_name).cloned().unwrap();
                        let class_var = class_info.var.clone();
                        let sig = class_info
                            .methods
                            .iter()
                            .find(|(m, _)| m == name)
                            .map(|(_, s)| s.clone());
                        if class_info.arity == 0 {
                            if call_args.len() != 1 {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "method `{name}` of `{class_name}` expects 1 argument, got {}",
                                        call_args.len()
                                    ),
                                    span: *span,
                                });
                            }
                            let (arg_reg, arg_vty) = self.lower_value(&call_args[0])?;
                            let ty_name = match self.env.type_name_of_vty(&arg_vty) {
                                Some(t) => t,
                                None => {
                                    return Err(CompileError::Type {
                                    msg: format!(
                                        "cannot resolve method `{name}` of `{class_name}`: the argument type is not concrete"
                                    ),
                                    span: call_args[0].span(),
                                });
                                }
                            };
                            let (_, params, body) =
                                match self.env.resolve_method(name, ty_name.as_str()) {
                                    Some(r) => r,
                                    None => {
                                        return Err(CompileError::Type {
                                            msg: format!(
                                            "no instance of `{class_name}` for type `{ty_name}`"
                                        ),
                                            span: *span,
                                        });
                                    }
                                };
                            // Recursion guard: a method that inlines itself (directly
                            // or transitively) is a compile error, not a stack overflow.
                            let key = (class_name, ty_name.clone(), name.to_string());
                            if self.method_lifting.contains(&key) {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "recursive typeclass method `{name}` for type `{ty_name}`"
                                    ),
                                    span: *span,
                                });
                            }
                            self.method_lifting.insert(key.clone());
                            let mut scope: HashMap<String, (usize, ValueTy)> = HashMap::new();
                            let mut ctor_scope: HashMap<String, Option<String>> = HashMap::new();
                            if let Some(p) = params.first() {
                                scope.insert(p.clone(), (arg_reg, arg_vty.clone()));
                                ctor_scope
                                    .insert(p.clone(), self.static_scrutinee_ctor(&call_args[0]));
                            }
                            self.value_locals.push(scope);
                            self.value_local_ctors.push(ctor_scope);
                            let res = self.lower_value(&body);
                            self.value_locals.pop();
                            self.value_local_ctors.pop();
                            self.method_lifting.remove(&key);
                            return res;
                        }
                        // Constructor class: the class-var-applied argument's
                        // concrete type head selects the instance. Lower all
                        // args, then bind each method param to its register.
                        let container_idx = self
                            .env
                            .class_var_arg_index(sig.as_ref().unwrap(), &class_var)
                            .ok_or_else(|| CompileError::Type {
                            msg: format!(
                                "method `{name}` of `{class_name}` has no class-var-applied argument"
                            ),
                            span: *span,
                        })?;
                        let n_sig_args = match sig.as_ref().unwrap() {
                            crate::ast::TypeExpr::TFunc(as_, _) => as_.len(),
                            _ => 1,
                        };
                        if call_args.len() != n_sig_args {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "method `{name}` of `{class_name}` expects {n_sig_args} argument(s), got {}",
                                    call_args.len()
                                ),
                                span: *span,
                            });
                        }
                        let mut arg_regs = Vec::with_capacity(call_args.len());
                        let mut arg_vtys = Vec::with_capacity(call_args.len());
                        for a in call_args {
                            let (r, t) = self.lower_value(a)?;
                            arg_regs.push(r);
                            arg_vtys.push(t);
                        }
                        let ctor = match &arg_vtys[container_idx] {
                            ValueTy::App(c, _) | ValueTy::Data(c, _) => c.clone(),
                            _ => {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "cannot resolve method `{name}` of `{class_name}`: the argument type is not a constructor application"
                                    ),
                                    span: call_args[container_idx].span(),
                                });
                            }
                        };
                        let (_, params, body) = match self.env.resolve_method(name, ctor.as_str()) {
                            Some(r) => r,
                            None => {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "no instance of `{class_name}` for constructor `{ctor}`"
                                    ),
                                    span: *span,
                                });
                            }
                        };
                        if params.len() != call_args.len() {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "method `{name}` of `{class_name}` expects {} argument(s), got {}",
                                    params.len(),
                                    call_args.len()
                                ),
                                span: *span,
                            });
                        }
                        let key = (class_name, ctor.clone(), name.to_string());
                        if self.method_lifting.contains(&key) {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "recursive typeclass method `{name}` for type `{ctor}`"
                                ),
                                span: *span,
                            });
                        }
                        self.method_lifting.insert(key.clone());
                        let mut scope: HashMap<String, (usize, ValueTy)> = HashMap::new();
                        let mut ctor_scope: HashMap<String, Option<String>> = HashMap::new();
                        for (p, (r, t)) in params.iter().zip(arg_regs.into_iter().zip(arg_vtys)) {
                            scope.insert(p.clone(), (r, t));
                        }
                        for (p, a) in params.iter().zip(call_args) {
                            ctor_scope.insert(p.clone(), self.static_scrutinee_ctor(a));
                        }
                        self.value_locals.push(scope);
                        self.value_local_ctors.push(ctor_scope);
                        let res = self.lower_value(&body);
                        self.value_locals.pop();
                        self.value_local_ctors.pop();
                        self.method_lifting.remove(&key);
                        return res;
                    }
                }
                // Value-function application: `add2 = adder 2.0` — the callee
                // is a definition whose body lowers to a closure value. Evaluate
                // the callee and the arguments, then emit a runtime dispatch;
                // the call's type is the callee's Func result type. (Calls to
                // named λ-parameter definitions are β-reduced by `reduce`
                // before lowering and never reach here.) Applying FEWER args
                // than the signature is partial application: the result is a
                // curry closure that captures the function and the applied
                // args, re-applying them with the remaining args on call.
                let callee_res = self.lower_value_ref(name, *span);
                let (callee_reg, callee_ty) = callee_res?;
                match &callee_ty {
                    ValueTy::Func(arg_tys, ret_tys) => {
                        if call_args.len() > arg_tys.len() {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "`{name}` expects {} argument(s), got {}",
                                    arg_tys.len(),
                                    call_args.len()
                                ),
                                span: *span,
                            });
                        }
                        let mut arg_regs = Vec::with_capacity(call_args.len());
                        for a in call_args {
                            let (ar, _) = self.lower_value(a)?;
                            arg_regs.push(ar);
                        }
                        let ret_ty = ret_tys.first().cloned().unwrap_or(ValueTy::Float);
                        if call_args.len() < arg_tys.len() {
                            // Partial application → a curry closure. Its env captures
                            // `[func, arg0, arg1, ...]` by value; on call it reads
                            // them out and dispatches `ValueCallFunc` with
                            // `(captured..., remaining...)`.
                            let remaining = arg_tys.len() - call_args.len();
                            let fragment_id =
                                self.apply_partial(arg_regs.len(), remaining, *span)?;
                            let env_reg = self.fresh_value_reg();
                            let mut fields = Vec::with_capacity(1 + arg_regs.len());
                            fields.push(callee_reg);
                            fields.extend(arg_regs.iter().copied());
                            self.emit_value(ValueInstr::ValueConstructRecord {
                                dst: env_reg,
                                fields,
                            });
                            let dst = self.fresh_value_reg();
                            self.emit_value(ValueInstr::ValueMakeClosure {
                                dst,
                                env: env_reg,
                                fragment: fragment_id,
                            });
                            let remaining_arg_tys: Vec<ValueTy> =
                                arg_tys[call_args.len()..].to_vec();
                            return Ok((dst, ValueTy::Func(remaining_arg_tys, vec![ret_ty])));
                        }
                        let dst = self.fresh_value_reg();
                        self.emit_value(ValueInstr::ValueCallFunc {
                            dst,
                            closure_slot: callee_reg,
                            args: arg_regs,
                        });
                        Ok((dst, ret_ty))
                    }
                    _ => {
                        // A higher-order function parameter (a `Func` whose
                        // static signature inference kept structural, or an
                        // unresolved parameter) applied at runtime: inference
                        // already validated the arity against the real `Func`
                        // type, so this is a full-application dispatch. v1
                        // function results are Float scalars.
                        let mut arg_regs = Vec::with_capacity(call_args.len());
                        for a in call_args {
                            let (ar, _) = self.lower_value(a)?;
                            arg_regs.push(ar);
                        }
                        let dst = self.fresh_value_reg();
                        self.emit_value(ValueInstr::ValueCallFunc {
                            dst,
                            closure_slot: callee_reg,
                            args: arg_regs,
                        });
                        Ok((dst, ValueTy::Float))
                    }
                }
            }
            Expr::Arith { op, lhs, rhs, span } => {
                // Value-track arithmetic: lower both operands as values, emit
                // the matching element-wise instruction, and yield a Float
                // value register (v1 always produces Float scalars here).
                let (a_reg, _a_ty) = self.lower_value(lhs)?;
                let (b_reg, _b_ty) = self.lower_value(rhs)?;
                let dst = self.fresh_value_reg();
                self.emit_value(match op {
                    ArithOp::Add => ValueInstr::ValueAdd {
                        dst,
                        a: a_reg,
                        b: b_reg,
                    },
                    ArithOp::Sub => ValueInstr::ValueSub {
                        dst,
                        a: a_reg,
                        b: b_reg,
                    },
                    ArithOp::Mul => ValueInstr::ValueMul {
                        dst,
                        a: a_reg,
                        b: b_reg,
                    },
                    ArithOp::Div => ValueInstr::ValueDiv {
                        dst,
                        a: a_reg,
                        b: b_reg,
                    },
                    ArithOp::Rem => {
                        return Err(CompileError::Type {
                            msg: "value `%` is not supported".into(),
                            span: *span,
                        });
                    }
                });
                Ok((dst, ValueTy::Float))
            }
            Expr::Lambda { params, body, span } => {
                // A lambda literal lowers to a FragmentIr (compiled once, shared
                // across every closure it creates) plus a by-value snapshot of
                // its free variables in an env Record. The closure references
                // the env; `ValueCallFunc` dispatches the fragment with the env
                // fields bound as a temporary cell frame.
                let param_names: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
                let free = self.free_vars(body, &param_names);
                // A named lambda-literal definition carries its resolved
                // parameter types from inference (set by `lower_value_ref`); an
                // anonymous lambda defaults to Float parameters.
                let param_tys: Vec<ValueTy> = self
                    .pending_param_tys
                    .take()
                    .unwrap_or_else(|| vec![ValueTy::Float; params.len()]);
                let (fragment_id, ret_ty) =
                    self.lower_fragment(params, body, &free, &param_tys, *span)?;
                let env_reg = self.emit_env_snapshot(&free, *span)?;
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueMakeClosure {
                    dst,
                    env: env_reg,
                    fragment: fragment_id,
                });
                Ok((dst, ValueTy::Func(param_tys, vec![ret_ty])))
            }
            Expr::Bool(b, _) => {
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueBool { dst, value: *b });
                Ok((dst, ValueTy::Bool))
            }
            Expr::Str(s, _) => {
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueConstString {
                    dst,
                    value: s.clone(),
                });
                Ok((dst, ValueTy::String))
            }
            Expr::ListLit(elems, _) => {
                let mut regs = Vec::with_capacity(elems.len());
                let mut elem_ty = ValueTy::Float;
                for e in elems {
                    let (r, t) = self.lower_value(e)?;
                    regs.push(r);
                    elem_ty = t;
                }
                let cap = regs.len();
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueListLit { dst, elems: regs });
                let ret = ValueTy::App("List".into(), vec![elem_ty, ValueTy::Cap(cap)]);
                self.note_container(&ret);
                Ok((dst, ret))
            }
            Expr::MapLit(entries, _) => {
                let mut keys = Vec::with_capacity(entries.len());
                let mut vals = Vec::with_capacity(entries.len());
                // Inference already unified every entry's value type, so the
                // entries share one static value type; it is carried on the
                // result Map (a List/record value is NOT pinned to Float).
                let mut val_ty = ValueTy::Float;
                for (k, v) in entries {
                    let (kr, _) = self.lower_value(&Expr::Str(k.clone(), Span::new(0, 0)))?;
                    let (vr, vt) = self.lower_value(v)?;
                    keys.push(kr);
                    vals.push(vr);
                    val_ty = vt;
                }
                let cap = keys.len();
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueMapLit { dst, keys, vals });
                let ret = ValueTy::App(
                    "Map".into(),
                    vec![ValueTy::String, val_ty, ValueTy::Cap(cap)],
                );
                self.note_container(&ret);
                Ok((dst, ret))
            }
            Expr::Cmp { op, lhs, rhs, .. } => {
                let (a, _) = self.lower_value(lhs)?;
                let (b, _) = self.lower_value(rhs)?;
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueCompare {
                    dst,
                    op: cmp_op_from_ast(*op),
                    a,
                    b,
                });
                Ok((dst, ValueTy::Bool))
            }
            Expr::Logic { op, lhs, rhs, .. } => {
                let (a, _) = self.lower_value(lhs)?;
                let (b, _) = self.lower_value(rhs)?;
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueLogic {
                    dst,
                    op: logic_op_from_ast(*op),
                    a,
                    b,
                });
                Ok((dst, ValueTy::Bool))
            }
            _ => Err(CompileError::Type {
                msg: "unsupported expression in value position".into(),
                span: e.span(),
            }),
        }
    }

    /// Resolve a collection-operation name to its IR op by arity. `insert`
    /// dispatches on argument count: 3 → map, 2 → set. Returns `None` for a
    /// non-collection name (the collection op names are reserved).
    fn value_builtin(&self, name: &str, nargs: usize) -> Option<ValueBuiltinOp> {
        use ValueBuiltinOp::*;
        Some(match (name, nargs) {
            ("cons", _) => Cons,
            ("head", _) => Head,
            ("tail", _) => Tail,
            ("length", _) => Length,
            ("map", _) => Map,
            ("fold", _) => Fold,
            ("filter", _) => Filter,
            ("list", _) => ListEmpty,
            ("insert", 3) => InsertMap,
            ("insert", 2) => InsertSet,
            ("lookup", _) => Lookup,
            ("member", _) => Member,
            ("empty_map", _) => MapEmpty,
            ("empty_set", _) => SetEmpty,
            _ => return None,
        })
    }

    /// Result static type of a collection op applied to the given argument
    /// types. Element/value types are read from the container argument, and
    /// the result capacity mirrors the SOURCE container's `Cap` (the runtime
    /// ops preserve the source cap, so the static type is exact). The
    /// empty-container constructors (`list`/`empty_map`/`empty_set`) carry
    /// `Cap(0)`: their value is a single container slot, and element slots are
    /// allocated per cons/insert/map op (each counted by `is_alloc_producing`).
    fn value_builtin_ty(
        &self,
        name: &str,
        args: &[ValueTy],
        span: Span,
    ) -> Result<ValueTy, CompileError> {
        // Enforce the `Ord` constraint on Map/Set keys (and Set elements): every
        // concrete key type must have a derived `Ord` instance (see
        // `derive_eq_ord`). `Func` gets no instance, so a function-typed key is
        // a compile error. Mirrors the infer-side check; lowering runs after
        // inference, so this is defense-in-depth for direct lower-only paths.
        let check_ord = |ty: &ValueTy, what: &str| -> Result<(), CompileError> {
            match self.env.type_name_of_vty(ty) {
                Some(name) => {
                    let has = self
                        .env
                        .instances
                        .get("Ord")
                        .map(|by_ty| by_ty.contains_key(name.as_str()))
                        .unwrap_or(false);
                    if has {
                        Ok(())
                    } else {
                        Err(CompileError::Type {
                            msg: format!("no Ord instance for {what} type `{name}`"),
                            span,
                        })
                    }
                }
                None => Err(CompileError::Type {
                    msg: format!("no Ord instance for {what} type (function or unresolved type)"),
                    span,
                }),
            }
        };
        match name {
            "cons" | "filter" => match args.get(1).and_then(list_type_args) {
                Some(inner) => Ok(ValueTy::App("List".into(), inner.clone())),
                _ => Err(CompileError::Type {
                    msg: format!("{name} expects a List, got {:?}", args.get(1)),
                    span,
                }),
            },
            "tail" => match args.first().and_then(list_type_args) {
                Some(inner) => Ok(ValueTy::App("List".into(), inner.clone())),
                _ => Err(CompileError::Type {
                    msg: format!("{name} expects a List, got {:?}", args.first()),
                    span,
                }),
            },
            "head" => {
                let elem = match args.first().and_then(list_type_args) {
                    Some(inner) => inner.first().cloned().unwrap_or(ValueTy::Float),
                    _ => ValueTy::Float,
                };
                Ok(ValueTy::App("Maybe".into(), vec![elem]))
            }
            "length" => Ok(ValueTy::Int),
            "map" => {
                // The result list's ELEMENT type is the closure's RETURN type
                // (`map : (a -> b) -> List a n -> List b n`), read from the
                // lowered `Func` signature of `args[0]`; a closure with no
                // known return falls back to the source element type. The
                // capacity mirrors the SOURCE list's cap: map allocates
                // cap(source) element slots + the result container in one op,
                // so a Cap(0) result type would undercount the arena bound.
                let elem = match args.first() {
                    Some(ValueTy::Func(_, rets)) => rets.first().cloned().unwrap_or(ValueTy::Float),
                    _ => match args.get(1).and_then(list_type_args) {
                        Some(inner) => inner.first().cloned().unwrap_or(ValueTy::Float),
                        _ => ValueTy::Float,
                    },
                };
                let cap = args.get(1).map(container_cap).unwrap_or(0);
                Ok(ValueTy::App("List".into(), vec![elem, ValueTy::Cap(cap)]))
            }
            // fold's result is the accumulator/seed type (mirrors inference).
            "fold" => Ok(args.get(1).cloned().unwrap_or(ValueTy::Float)),
            "list" => Ok(ValueTy::App(
                "List".into(),
                vec![ValueTy::Float, ValueTy::Cap(0)],
            )),
            "insert" => match args.len() {
                3 => {
                    check_ord(args.first().unwrap_or(&ValueTy::Float), "key")?;
                    Ok(ValueTy::App(
                        "Map".into(),
                        vec![
                            args.first().cloned().unwrap_or(ValueTy::Float),
                            args.get(1).cloned().unwrap_or(ValueTy::Float),
                            // The result map carries the SOURCE map's capacity (the
                            // COW insert keeps the source bound): a Cap(0) result
                            // type would undercount the arena bound.
                            ValueTy::Cap(args.get(2).map(container_cap).unwrap_or(0)),
                        ],
                    ))
                }
                _ => {
                    check_ord(args.first().unwrap_or(&ValueTy::Float), "element")?;
                    Ok(ValueTy::App(
                        "Set".into(),
                        vec![
                            args.first().cloned().unwrap_or(ValueTy::Float),
                            ValueTy::Cap(args.get(1).map(container_cap).unwrap_or(0)),
                        ],
                    ))
                }
            },
            "lookup" => {
                check_ord(args.first().unwrap_or(&ValueTy::Float), "key")?;
                let v = match args.get(1) {
                    Some(ValueTy::App(name, inner)) if name == "Map" => {
                        inner.get(1).cloned().unwrap_or(ValueTy::Float)
                    }
                    _ => ValueTy::Float,
                };
                Ok(ValueTy::App("Maybe".into(), vec![v]))
            }
            "member" => {
                check_ord(args.first().unwrap_or(&ValueTy::Float), "key")?;
                Ok(ValueTy::Bool)
            }
            "empty_map" => Ok(ValueTy::App(
                "Map".into(),
                vec![ValueTy::String, ValueTy::Float, ValueTy::Cap(0)],
            )),
            "empty_set" => Ok(ValueTy::App(
                "Set".into(),
                vec![ValueTy::Float, ValueTy::Cap(0)],
            )),
            _ => Err(CompileError::Unsupported(format!(
                "unknown collection op {name}"
            ))),
        }
    }

    /// Resolve a `Ref` in value position: a value local (match-arm binding) or a
    /// value definition (inlined at each use site).
    fn lower_value_ref(
        &mut self,
        name: &str,
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        for scope in self.value_locals.iter().rev() {
            if let Some(&(reg, ref vty)) = scope.get(name) {
                return Ok((reg, vty.clone()));
            }
        }
        // Bare zero-argument collection constructors: `list`, `empty_map`,
        // `empty_set` in value position are empty-container builtin calls (the
        // capacity argument was removed — open collections).
        if let Some(op) = match name {
            "list" => Some(ValueBuiltinOp::ListEmpty),
            "empty_map" => Some(ValueBuiltinOp::MapEmpty),
            "empty_set" => Some(ValueBuiltinOp::SetEmpty),
            _ => None,
        } {
            let dst = self.fresh_value_reg();
            self.emit_value(ValueInstr::ValueCallBuiltin {
                dst,
                op,
                args: vec![],
            });
            let ret = match op {
                ValueBuiltinOp::ListEmpty => ValueTy::App("List".into(), vec![ValueTy::Float]),
                ValueBuiltinOp::MapEmpty => {
                    ValueTy::App("Map".into(), vec![ValueTy::String, ValueTy::Float])
                }
                _ => ValueTy::App("Set".into(), vec![ValueTy::Float]),
            };
            self.note_container(&ret);
            return Ok((dst, ret));
        }
        if let Some(idx) = self.fragment_captures.iter().position(|f| f == name) {
            // A free variable of the enclosing lambda: read it from the call's
            // env frame. `idx` is the name's position in the fragment's capture
            // list, which `run_fragment` binds as frame cell `idx` (the env
            // Record field order == capture order). v1 captures are scalar
            // values, so the static type is Float.
            let dst = self.fresh_value_reg();
            self.emit_value(ValueInstr::ValueReadCell { dst, cell: idx });
            return Ok((dst, ValueTy::Float));
        }
        if let Some(&cell) = self.main_cell_locals.get(name) {
            // A main λ-parameter in value position: copy the persistent cell's
            // value into a fresh slot (a `Void` cell reads as `Float(0.0)`).
            let dst = self.fresh_value_reg();
            self.emit_value(ValueInstr::ValueReadMainCell { dst, cell });
            return Ok((dst, ValueTy::Float));
        }
        if self.env.data_types.contains_key(name) {
            return Err(CompileError::Type {
                msg: format!("`{name}` is a data type, not a value"),
                span,
            });
        }
        if self.env.newtypes.contains_key(name) {
            return Err(CompileError::Type {
                msg: format!("`{name}` is a newtype; use its constructor `{name} <value>`"),
                span,
            });
        }
        // Nullary sum constructor: a bare `Nothing` / `Red` in value position is
        // a complete value of its sum type — construct the empty sum directly.
        if let Some((sum_name, ctor_idx, payload)) = self.sum_ctor(name) {
            if payload.is_empty() {
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueConstructSum {
                    dst,
                    ctor: ctor_idx as u32,
                    payload: vec![],
                });
                return Ok((dst, ValueTy::Data(sum_name, vec![])));
            }
        }
        let def = self
            .defs
            .get(name)
            .cloned()
            .ok_or_else(|| CompileError::Type {
                msg: format!("unknown `{name}` in value lowering"),
                span,
            })?;
        match def {
            Def::Local { body, .. } => {
                // v1 value bindings are per-tick re-evaluated expressions (no
                // shared state), so inlining the body at each use site is
                // semantically identical to lifting it once.
                if self.value_inline.contains(name) {
                    return Err(CompileError::Type {
                        msg: format!("recursive value definition `{name}`"),
                        span,
                    });
                }
                self.value_inline.insert(name.to_string());
                // A named lambda-literal definition lowers with its resolved
                // parameter types (threaded through `pending_param_tys`).
                let saved_pending = self.pending_param_tys.take();
                self.pending_param_tys = self.fn_param_tys.get(name).cloned();
                let res = self.lower_value(&body);
                self.pending_param_tys = saved_pending;
                self.value_inline.remove(name);
                res
            }
            Def::Anchor {
                params: def_params,
                body,
                span: dspan,
                ..
            } => {
                // A bare reference to a user definition with λ-parameters in
                // value position is a first-class function value: compile its
                // body into a real FragmentIr and capture free variables by
                // value (mirrors the lambda-literal path). The closure's
                // fragment dispatches the body with the value args bound to
                // the fragment's leading registers, so named definitions
                // participate in currying (`add5 = add2 5.0`) and dispatch
                // just like lambdas.
                let param_names: HashSet<String> =
                    def_params.iter().map(|p| p.name.clone()).collect();
                let free = self.free_vars(&body, &param_names);
                let param_tys = vec![ValueTy::Float; def_params.len()];
                let (fragment_id, ret_ty) =
                    self.lower_fragment(&def_params, &body, &free, &param_tys, dspan)?;
                let env_reg = self.emit_env_snapshot(&free, dspan)?;
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueMakeClosure {
                    dst,
                    env: env_reg,
                    fragment: fragment_id,
                });
                let arg_tys = def_params.iter().map(|_| ValueTy::Float).collect();
                Ok((dst, ValueTy::Func(arg_tys, vec![ret_ty])))
            }
            _ => Err(CompileError::Type {
                msg: format!("`{name}` is not a value expression in v1"),
                span,
            }),
        }
    }

    /// Free variables of `e`: `Ref` names not in `bound` and not shadowed by
    /// an inner `let`/`match`/`lambda`. Returned in deterministic (encounter)
    /// order, deduplicated — the order is the env Record field order, which
    /// doubles as the capture-cell indices in the call frame.
    fn free_vars(&self, e: &Expr, bound: &HashSet<String>) -> Vec<String> {
        let mut free = Vec::new();
        let mut seen = HashSet::new();
        self.free_vars_impl(e, bound, &mut free, &mut seen);
        free
    }

    fn free_vars_impl(
        &self,
        e: &Expr,
        bound: &HashSet<String>,
        out: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) {
        match e {
            Expr::Ref(name, _) => {
                if !bound.contains(name) && seen.insert(name.clone()) {
                    out.push(name.clone());
                }
            }
            Expr::Int(_, _)
            | Expr::Float(_, _)
            | Expr::Imag(_, _)
            | Expr::Str(_, _)
            | Expr::Wire(_)
            | Expr::Cut(_) => {}
            Expr::Neg(inner, _) => self.free_vars_impl(inner, bound, out, seen),
            Expr::Apply { name, args, .. } => {
                // The applied callee is a reference too: `fn x -> f x` applies
                // a closure `f` from an enclosing scope, and the name is a free
                // variable even though it only appears in callee position
                // (without this, the env snapshot would miss it and lowering
                // would report "unknown `f` in value lowering"). Type and
                // constructor names (`Pair { ... }`, `Circle 1.5`) are NOT free
                // variables — a lambda body constructs data types without
                // capturing them.
                if !bound.contains(name) && seen.insert(name.clone()) {
                    let is_ctor = self.env.data_types.contains_key(name)
                        || self.env.newtypes.contains_key(name)
                        || self.sum_ctor(name).is_some();
                    if !is_ctor {
                        out.push(name.clone());
                    }
                }
                for a in args {
                    self.free_vars_impl(a, bound, out, seen);
                }
            }
            Expr::Arith { lhs, rhs, .. } => {
                self.free_vars_impl(lhs, bound, out, seen);
                self.free_vars_impl(rhs, bound, out, seen);
            }
            Expr::Seq(lhs, rhs, _)
            | Expr::Par(lhs, rhs, _)
            | Expr::Split(lhs, rhs, _)
            | Expr::Merge(lhs, rhs, _)
            | Expr::Loop(lhs, rhs, _)
            | Expr::Delay(lhs, rhs, _) => {
                self.free_vars_impl(lhs, bound, out, seen);
                self.free_vars_impl(rhs, bound, out, seen);
            }
            Expr::Let { defs, body, .. } => {
                // Inner `let` bindings shadow outer names inside the body; the
                // def bodies themselves may reference outer frees.
                let mut inner = bound.clone();
                for d in defs {
                    inner.insert(d.name().to_string());
                }
                for d in defs {
                    self.free_vars_impl(d.body(), bound, out, seen);
                }
                self.free_vars_impl(body, &inner, out, seen);
            }
            Expr::Record(fields, _) => {
                for (_, fe) in fields {
                    self.free_vars_impl(fe, bound, out, seen);
                }
            }
            Expr::FieldProject { record, .. } => self.free_vars_impl(record, bound, out, seen),
            Expr::FieldUpdate { record, value, .. } => {
                self.free_vars_impl(record, bound, out, seen);
                self.free_vars_impl(value, bound, out, seen);
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.free_vars_impl(scrutinee, bound, out, seen);
                for arm in arms {
                    let mut inner = bound.clone();
                    for v in crate::reduce::pattern_vars(&arm.pattern) {
                        inner.insert(v);
                    }
                    for (g, b) in &arm.guards {
                        self.free_vars_impl(g, &inner, out, seen);
                        self.free_vars_impl(b, &inner, out, seen);
                    }
                }
            }
            Expr::If {
                cond, then, els, ..
            } => {
                self.free_vars_impl(cond, bound, out, seen);
                self.free_vars_impl(then, bound, out, seen);
                self.free_vars_impl(els, bound, out, seen);
            }
            Expr::Lambda { params, body, .. } => {
                let mut inner = bound.clone();
                for p in params {
                    inner.insert(p.name.clone());
                }
                self.free_vars_impl(body, &inner, out, seen);
            }
            Expr::ActorParam { default, .. } => {
                if let Some(d) = default {
                    self.free_vars_impl(d, bound, out, seen);
                }
            }
            Expr::Bool(_, _) => {}
            Expr::ListLit(elems, _) => {
                for el in elems {
                    self.free_vars_impl(el, bound, out, seen);
                }
            }
            Expr::MapLit(entries, _) => {
                for (_, ve) in entries {
                    self.free_vars_impl(ve, bound, out, seen);
                }
            }
            Expr::Cmp { lhs, rhs, .. } | Expr::Logic { lhs, rhs, .. } => {
                self.free_vars_impl(lhs, bound, out, seen);
                self.free_vars_impl(rhs, bound, out, seen);
            }
        }
    }

    /// Compile a lambda body into a [`FragmentIr`], returning the fragment's
    /// index and the body's value type.
    ///
    /// Fragment-local registers: the body's value instructions use registers
    /// `0..num_value_regs`, unrelated to the program's register numbering —
    /// the interpreter offsets them by a per-call base at dispatch. The
    /// program-level value-track state (`value_blocks`, `cur_value_block`,
    /// `next_value_reg`, `value_locals`, `fragment_captures`) is saved around
    /// the body compile and restored afterwards.
    ///
    /// Within the fragment: parameters bind to fragment-local registers
    /// `0..params.len()` (value args are copied in by `run_fragment`), typed
    /// with the λ-parameter types threaded from inference; a `Ref` to a free
    /// name emits `ValueReadCell { cell: capture_index }` where
    /// `capture_index` is the name's position in `free` (the env Record field
    /// order, bound by `run_fragment` as temp-frame cells).
    fn lower_fragment(
        &mut self,
        params: &[Param],
        body: &Expr,
        free: &[String],
        param_tys: &[ValueTy],
        _span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        let saved_blocks = std::mem::take(&mut self.value_blocks);
        let saved_cur = self.cur_value_block;
        self.cur_value_block = self.new_value_block();
        let saved_next = self.next_value_reg;
        let saved_locals = std::mem::take(&mut self.value_locals);
        let saved_captures = std::mem::take(&mut self.fragment_captures);

        let mut scope: HashMap<String, (usize, ValueTy)> = HashMap::new();
        for (i, p) in params.iter().enumerate() {
            let ty = param_tys.get(i).cloned().unwrap_or(ValueTy::Float);
            scope.insert(p.name.clone(), (i, ty));
        }
        self.value_locals.push(scope);
        self.value_local_ctors.push(HashMap::new());
        self.fragment_captures = free.to_vec();
        self.next_value_reg = params.len();

        let res = self.lower_value(body);
        let body_result = match res {
            Ok(r) => r,
            Err(e) => {
                self.value_blocks = saved_blocks;
                self.cur_value_block = saved_cur;
                self.next_value_reg = saved_next;
                self.value_locals = saved_locals;
                self.value_local_ctors.pop();
                self.fragment_captures = saved_captures;
                return Err(e);
            }
        };
        let (result_reg, result_ty) = body_result;
        let frag = FragmentIr {
            value_blocks: std::mem::take(&mut self.value_blocks),
            entry: 0,
            steps: Vec::new(),
            num_value_regs: self.next_value_reg,
            num_block_regs: 0,
            output_value_regs: vec![result_reg],
            output_block_regs: Vec::new(),
            num_capture_cells: free.len(),
            sig: FuncSig {
                value_ins: params.len(),
                value_outs: 1,
                signal_ins: 0,
            },
        };
        let id = self.fragments.len();
        self.fragments.push(std::sync::Arc::new(frag));

        self.value_blocks = saved_blocks;
        self.cur_value_block = saved_cur;
        self.next_value_reg = saved_next;
        self.value_locals = saved_locals;
        self.value_local_ctors.pop();
        self.fragment_captures = saved_captures;
        Ok((id, result_ty))
    }

    /// Snapshot the current values of `free` into a new env Record, returning
    /// the record's value register. Each free name is resolved through
    /// `lower_value_ref`, so a top-level definition inlines its value while a
    /// name captured by an enclosing lambda reads its env-frame cell.
    fn emit_env_snapshot(&mut self, free: &[String], span: Span) -> Result<usize, CompileError> {
        let mut field_regs = Vec::with_capacity(free.len());
        for name in free {
            let (r, _) = self.lower_value_ref(name, span)?;
            field_regs.push(r);
        }
        let dst = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueConstructRecord {
            dst,
            fields: field_regs,
        });
        Ok(dst)
    }

    /// Compile a partial-application (curry) fragment for a call that applies
    /// fewer args than the callee's signature.
    ///
    /// The curry fragment's env capture cells are `[func, arg0, arg1, ...]` —
    /// the function value followed by the already-applied args, in capture
    /// order. Its value registers start with the `remaining_arity` call-site
    /// args (the fragment's own params). On call it reads the captured function
    /// and applied args out of the env frame (`ValueReadCell` capture reads),
    /// then dispatches `ValueCallFunc` with `[captured args..., remaining args...]`.
    ///
    /// Mirrors `lower_fragment`'s register scheme: fragment-local registers
    /// `0..num_value_regs`, offset by the interpreter per call.
    fn apply_partial(
        &mut self,
        applied: usize,
        remaining_arity: usize,
        _span: Span,
    ) -> Result<usize, CompileError> {
        let saved_blocks = std::mem::take(&mut self.value_blocks);
        let saved_cur = self.cur_value_block;
        self.cur_value_block = self.new_value_block();
        let saved_next = self.next_value_reg;
        let saved_locals = std::mem::take(&mut self.value_locals);
        let saved_captures = std::mem::take(&mut self.fragment_captures);

        let func_reg = remaining_arity;
        let arg0_reg = remaining_arity + 1;
        let call_dst = remaining_arity + 1 + applied;
        let num_value_regs = call_dst + 1;
        self.next_value_reg = num_value_regs;
        // Capture index 0 = the captured function value; 1.. = the applied args.
        self.emit_value(ValueInstr::ValueReadCell {
            dst: func_reg,
            cell: 0,
        });
        for i in 0..applied {
            self.emit_value(ValueInstr::ValueReadCell {
                dst: arg0_reg + i,
                cell: 1 + i,
            });
        }
        let mut call_args: Vec<usize> = (arg0_reg..arg0_reg + applied).collect();
        call_args.extend(0..remaining_arity);
        self.emit_value(ValueInstr::ValueCallFunc {
            dst: call_dst,
            closure_slot: func_reg,
            args: call_args,
        });

        let frag = FragmentIr {
            value_blocks: std::mem::take(&mut self.value_blocks),
            entry: 0,
            steps: Vec::new(),
            num_value_regs,
            num_block_regs: 0,
            output_value_regs: vec![call_dst],
            output_block_regs: Vec::new(),
            num_capture_cells: 1 + applied,
            sig: FuncSig {
                value_ins: remaining_arity,
                value_outs: 1,
                signal_ins: 0,
            },
        };
        let id = self.fragments.len();
        self.fragments.push(std::sync::Arc::new(frag));

        self.value_blocks = saved_blocks;
        self.cur_value_block = saved_cur;
        self.next_value_reg = saved_next;
        self.value_locals = saved_locals;
        self.fragment_captures = saved_captures;
        Ok(id)
    }

    /// Resolve the constructor name of a scrutinee expression when it is
    /// statically known at compile time: a literal sum construction
    /// (`Circle 1.5`) or a `Ref` to an inlined value definition whose body is
    /// one. Returns `None` when the constructor cannot be determined statically
    /// (an unbound `_` wire, a field projection, a nested match, ...).
    fn static_scrutinee_ctor(&self, e: &Expr) -> Option<String> {
        self.static_scrutinee_ctor_impl(e, &mut HashSet::new())
    }

    fn static_scrutinee_ctor_impl(
        &self,
        e: &Expr,
        visited: &mut HashSet<String>,
    ) -> Option<String> {
        match e {
            Expr::Apply { name, args, .. } => {
                if self.sum_ctor(name).is_some() {
                    return Some(name.clone());
                }
                // A typeclass method call whose result constructor is statically
                // known: resolve the instance via the class-var-applied argument
                // (constructor classes) and inline the body.
                if let Some(class_name) = self.env.class_of_method(name) {
                    let class_info = self.env.typeclasses.get(&class_name).cloned().unwrap();
                    let sig = class_info
                        .methods
                        .iter()
                        .find(|(m, _)| m == name)
                        .map(|(_, s)| s.clone());
                    if class_info.arity >= 1 {
                        if let Some(container_idx) = self
                            .env
                            .class_var_arg_index(sig.as_ref().unwrap(), &class_info.var)
                        {
                            // The container argument's static constructor selects
                            // the instance: `fmap g (Just 1.0)` → `Just` → the
                            // `Maybe` instance.
                            let container_ctor =
                                self.static_scrutinee_ctor_impl(args.get(container_idx)?, visited)?;
                            let ty_name = self.instance_type_of_ctor(&container_ctor)?;
                            if let Some((_, params, body)) = self.env.resolve_method(name, &ty_name)
                            {
                                // β-substitute the call args into the body and
                                // resolve the result's constructor.
                                let mut subst: HashMap<String, Expr> = HashMap::new();
                                for (p, a) in params.iter().zip(args.iter()) {
                                    subst.insert(p.clone(), a.clone());
                                }
                                let inlined = crate::reduce::substitute(&body, &subst);
                                return self.static_scrutinee_ctor_impl(&inlined, visited);
                            }
                        }
                    }
                }
                // Builtin Maybe-returning ops resolve statically on concrete
                // inputs: `head` of a non-empty list and `lookup` of a present
                // key are `Just`; an empty list / absent key is `Nothing`.
                match name.as_str() {
                    "head" => args
                        .first()
                        .and_then(|xs| self.static_list_nonempty(xs, visited))
                        .map(|ne| {
                            if ne {
                                "Just".to_string()
                            } else {
                                "Nothing".to_string()
                            }
                        }),
                    "lookup" => {
                        if let (Some(Expr::Str(k, _)), Some(map)) = (args.first(), args.get(1)) {
                            self.static_map_has_key(map, k, visited).map(|present| {
                                if present {
                                    "Just".to_string()
                                } else {
                                    "Nothing".to_string()
                                }
                            })
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }
            Expr::Ref(name, _) => {
                if visited.contains(name) {
                    return None;
                }
                if let Some(Def::Local { body, .. }) = self.defs.get(name).cloned() {
                    visited.insert(name.clone());
                    let r = self.static_scrutinee_ctor_impl(&body, visited);
                    visited.remove(name);
                    return r;
                }
                // A value local bound to a statically-known constructor (a
                // method parameter bound to `Just 1.0`): a `match` over the
                // param's `Ref` selects its arm at compile time.
                for scope in self.value_local_ctors.iter().rev() {
                    if let Some(ctor) = scope.get(name) {
                        return ctor.clone();
                    }
                }
                // A bare nullary constructor (`Nothing`, `Red`) is a complete
                // sum value: its constructor is statically known even though it
                // is not a `Def::Local` body.
                if let Some((_, _, payload)) = self.sum_ctor(name) {
                    if payload.is_empty() {
                        return Some(name.clone());
                    }
                }
                None
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                // A statically-known scrutinee constructor selects the arm at
                // compile time; the arm's body's constructor is the match's
                // result constructor.
                let scrut_ctor = self.static_scrutinee_ctor_impl(scrutinee, visited)?;
                for arm in arms {
                    if let Pattern::Ctor(c, _) = &arm.pattern {
                        if c == &scrut_ctor {
                            // A guarded arm's result constructor is NOT
                            // statically known (the guard may fail at runtime),
                            // so only a bare arm can claim a statically-known
                            // result.
                            return crate::reduce::unguarded_arm_body(arm)
                                .and_then(|body| self.static_scrutinee_ctor_impl(body, visited));
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// The type name bound to the class variable for a value constructor
    /// (`Just`/`Nothing` → `Maybe`; a builtin ctor like `List` maps to itself).
    fn instance_type_of_ctor(&self, ctor: &str) -> Option<String> {
        if let Some((sum_name, _, _)) = self.sum_ctor(ctor) {
            return Some(sum_name);
        }
        // A type constructor name used directly (`List`, `Box`, `Maybe`).
        if self.env.ctor_arity(ctor).is_some() || self.env.data_arities.contains_key(ctor) {
            return Some(ctor.to_string());
        }
        None
    }

    /// Statically determine a list expression's emptiness: `Some(true)` when
    /// non-empty, `Some(false)` when empty, `None` when unknown. Follows
    /// inlined value definitions (the visit guard breaks cycles).
    fn static_list_nonempty(&self, e: &Expr, visited: &mut HashSet<String>) -> Option<bool> {
        match e {
            Expr::ListLit(elems, _) => Some(!elems.is_empty()),
            Expr::Apply { name, args, .. } => match name.as_str() {
                // `cons x xs` always yields a non-empty list; `list n` always an
                // empty one. `map` preserves the source length; `tail` of an
                // empty list stays empty (a non-empty source may still produce
                // an empty tail, so that case is unknown).
                "cons" => Some(true),
                "list" => Some(false),
                "map" => args
                    .get(1)
                    .and_then(|xs| self.static_list_nonempty(xs, visited)),
                "tail" => match args
                    .first()
                    .and_then(|xs| self.static_list_nonempty(xs, visited))
                {
                    Some(false) => Some(false),
                    _ => None,
                },
                _ => None,
            },
            Expr::Ref(name, _) => {
                if visited.contains(name) {
                    return None;
                }
                if let Some(Def::Local { body, .. }) = self.defs.get(name).cloned() {
                    visited.insert(name.clone());
                    let r = self.static_list_nonempty(&body, visited);
                    visited.remove(name);
                    return r;
                }
                None
            }
            _ => None,
        }
    }

    /// Statically determine whether a map expression contains `key`:
    /// `Some(true)`/`Some(false)` when the keys are statically known, `None`
    /// otherwise. Follows inlined value definitions and `insert`/`empty_map`
    /// chains.
    fn static_map_has_key(
        &self,
        e: &Expr,
        key: &str,
        visited: &mut HashSet<String>,
    ) -> Option<bool> {
        match e {
            Expr::MapLit(entries, _) => {
                let present = entries.iter().any(|(k, _)| k == key);
                Some(present)
            }
            Expr::Apply { name, args, .. } if name == "insert" && args.len() == 3 => {
                // Resolve the insert key through the same Ref inlining used for
                // the map argument: a let-bound key (`k = "a"`) is a concrete
                // string the analysis can compare. A genuinely non-literal key
                // is unknown — the analysis must not claim it present/absent.
                match self.static_string_literal(&args[0], visited) {
                    Some(k) => {
                        if k == key {
                            Some(true)
                        } else {
                            self.static_map_has_key(&args[2], key, visited)
                        }
                    }
                    None => None,
                }
            }
            Expr::Apply { name, .. } if name == "empty_map" => Some(false),
            Expr::Ref(name, _) => {
                if visited.contains(name) {
                    return None;
                }
                if let Some(Def::Local { body, .. }) = self.defs.get(name).cloned() {
                    visited.insert(name.clone());
                    let r = self.static_map_has_key(&body, key, visited);
                    visited.remove(name);
                    return r;
                }
                None
            }
            _ => None,
        }
    }

    /// Resolve an expression to a string literal, following inlined value
    /// definitions (cycle-guarded). `Some(s)` when the string is statically
    /// known, `None` otherwise.
    fn static_string_literal(&self, e: &Expr, visited: &mut HashSet<String>) -> Option<String> {
        match e {
            Expr::Str(s, _) => Some(s.clone()),
            Expr::Ref(name, _) => {
                if visited.contains(name) {
                    return None;
                }
                if let Some(Def::Local { body, .. }) = self.defs.get(name).cloned() {
                    visited.insert(name.clone());
                    let r = self.static_string_literal(&body, visited);
                    visited.remove(name);
                    return r;
                }
                None
            }
            _ => None,
        }
    }

    /// Concrete type arguments of a builtin-sum scrutinee that is a direct
    /// constructor application (`Just x`, `Left x`, `Right x`), following
    /// inlined value definitions. Lowering gives such a scrutinee the static
    /// type `Data(name, [])`, so the placeholder payload types in the builtin
    /// shape must be resolved against the constructor arguments' concrete
    /// types (mirroring inference). The arguments are lowered in a sandbox —
    /// their instructions are discarded — to recover their static types.
    /// Returns an empty vector when the scrutinee is not a resolvable builtin
    /// construction (user sums already carry concrete payloads).
    fn builtin_sum_scrutinee_args(&mut self, scrutinee: &Expr) -> Vec<ValueTy> {
        self.builtin_sum_scrutinee_args_impl(scrutinee, &mut HashSet::new())
    }

    fn builtin_sum_scrutinee_args_impl(
        &mut self,
        e: &Expr,
        seen: &mut HashSet<String>,
    ) -> Vec<ValueTy> {
        match e {
            Expr::Ref(name, _) => {
                if !seen.insert(name.clone()) {
                    return vec![];
                }
                if let Some(Def::Local { body, .. }) = self.defs.get(name).cloned() {
                    let r = self.builtin_sum_scrutinee_args_impl(&body, seen);
                    seen.remove(name);
                    return r;
                }
                vec![]
            }
            Expr::Apply { name, args, .. } => {
                let Some((sum_name, _, payload)) = self.sum_ctor(name) else {
                    return vec![];
                };
                if self.env.ctor_arity(&sum_name).is_none() {
                    // User sum — its payloads are already concrete.
                    return vec![];
                }
                // Placeholder payloads reference type-param positions
                // (`Var(k)` ↔ k-th type arg); the ctor's argument at that
                // payload slot supplies the concrete type.
                let mut out: Vec<ValueTy> = vec![];
                for (i, pt) in payload.iter().enumerate() {
                    let ValueTy::Var(k) = pt else {
                        continue;
                    };
                    let Some(arg) = args.get(i) else {
                        continue;
                    };
                    let Some(ty) = self.sandbox_value_ty(arg) else {
                        return vec![];
                    };
                    let idx = k.saturating_sub(1) as usize;
                    while out.len() <= idx {
                        out.push(ValueTy::Float);
                    }
                    out[idx] = ty;
                }
                out
            }
            _ => vec![],
        }
    }

    /// Recover an expression's static value type by lowering it in a sandbox:
    /// the emitted instructions are discarded. Used to resolve builtin-sum
    /// scrutinee payload types that `lower_value` does not carry on the
    /// returned scrutinee type (`Data(name, [])`).
    fn sandbox_value_ty(&mut self, e: &Expr) -> Option<ValueTy> {
        let saved_blocks = std::mem::take(&mut self.value_blocks);
        let saved_cur = self.cur_value_block;
        self.cur_value_block = self.new_value_block();
        let saved_next = self.next_value_reg;
        let saved_locals = std::mem::take(&mut self.value_locals);
        let saved_captures = std::mem::take(&mut self.fragment_captures);
        let saved_inline = std::mem::take(&mut self.value_inline);
        let saved_pending = std::mem::take(&mut self.pending_param_tys);
        let r = self.lower_value(e).ok();
        self.value_blocks = saved_blocks;
        self.cur_value_block = saved_cur;
        self.next_value_reg = saved_next;
        self.value_locals = saved_locals;
        self.fragment_captures = saved_captures;
        self.value_inline = saved_inline;
        self.pending_param_tys = saved_pending;
        r.map(|(_, ty)| ty.clone())
    }

    /// Resolve the sum type of a `match` from its arm constructors, intersecting
    /// the candidate sum types per constructor (mirrors inference). Literal,
    /// wildcard, and variable arms do not pin the sum (a scalar match, or a sum
    /// match with a fallback arm); the caller only resolves when at least one
    /// constructor arm exists.
    fn resolve_match_sum(&self, arms: &[MatchArm], span: Span) -> Result<String, CompileError> {
        let mut candidates: Option<Vec<String>> = None;
        for arm in arms {
            let ctor = match &arm.pattern {
                Pattern::Ctor(c, _) => c,
                _ => continue,
            };
            let per_ctor: Vec<String> = self
                .env
                .data_types
                .iter()
                .filter_map(|(tname, info)| match info {
                    DataInfo::Sum(ctors) if ctors.iter().any(|(c, _)| c == ctor) => {
                        Some(tname.clone())
                    }
                    _ => None,
                })
                .collect();
            if per_ctor.is_empty() {
                return Err(CompileError::Type {
                    msg: format!("unknown constructor `{ctor}`"),
                    span,
                });
            }
            candidates = Some(match candidates {
                None => per_ctor,
                Some(acc) => acc.into_iter().filter(|n| per_ctor.contains(n)).collect(),
            });
        }
        match candidates {
            Some(v) if v.len() == 1 => Ok(v[0].clone()),
            _ => Err(CompileError::Type {
                msg: "match arms use constructors of ambiguous or different sum types".into(),
                span,
            }),
        }
    }

    /// Lower the guard-free arm of a match whose scrutinee constructor is
    /// statically known. Only the selected arm is lowered (its pattern matched
    /// at compile time), preserving exact-capacity behavior. The arm that would
    /// match at runtime is the `Ctor` arm for `cname`, or the first `Var`/`Wild`
    /// arm. Returns `(body_reg, body_ty)`.
    #[allow(clippy::too_many_arguments)]
    fn lower_static_match_arm(
        &mut self,
        scrutinee_reg: usize,
        scrutinee_vty: &ValueTy,
        sum_name: &str,
        ctors: &[(String, Vec<ValueTy>)],
        scrutinee_args: &[ValueTy],
        is_builtin_sum: bool,
        arms: &[MatchArm],
        cname: &str,
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        // The first arm that matches at runtime: the Ctor arm for `cname`, or a
        // Var/Wild arm (a Var binds the whole scrutinee, a Wild ignores it).
        let selected = arms
            .iter()
            .position(|arm| match &arm.pattern {
                Pattern::Ctor(c, _) => c == cname,
                Pattern::Var(_) | Pattern::Wild => true,
                _ => false,
            })
            .ok_or_else(|| CompileError::Type {
                msg: format!("match over `{cname}` has no matching arm"),
                span,
            })?;
        let arm = &arms[selected];
        // Reachable only from the guard-free static fast path (`!has_guards`),
        // so every arm here is a bare `pattern => body`; a guarded arm is a
        // defensive error rather than a silent drop.
        let body = crate::reduce::unguarded_arm_body(arm).ok_or_else(|| CompileError::Type {
            msg: "guarded arm reached the static match fast path".into(),
            span,
        })?;
        // Bind the arm's pattern leaves in a fresh scope, visible to the body.
        self.value_locals.push(HashMap::new());
        self.value_local_ctors.push(HashMap::new());
        let result = match &arm.pattern {
            Pattern::Var(name) => {
                // A variable arm binds the whole scrutinee (its ctor is
                // statically known, so a nested match over it resolves too).
                self.value_locals
                    .last_mut()
                    .unwrap()
                    .insert(name.clone(), (scrutinee_reg, scrutinee_vty.clone()));
                self.value_local_ctors
                    .last_mut()
                    .unwrap()
                    .insert(name.clone(), Some(cname.to_string()));
                self.lower_value(body)
            }
            Pattern::Wild => self.lower_value(body),
            Pattern::Ctor(ctor, args) => {
                let ctor_idx = ctors.iter().position(|(c, _)| c == ctor).ok_or_else(|| {
                    CompileError::Type {
                        msg: format!("unknown constructor `{ctor}` for `{sum_name}`"),
                        span,
                    }
                })?;
                let payload_tys = &ctors[ctor_idx].1;
                let mut payload_regs = Vec::with_capacity(payload_tys.len());
                for _ in 0..payload_tys.len() {
                    payload_regs.push(self.fresh_value_reg());
                }
                self.emit_value(ValueInstr::ValueMatch {
                    dst: payload_regs.clone(),
                    slot: scrutinee_reg,
                    ctor: ctor_idx as u32,
                });
                // Bind the arm's payload vars to the match's payload regs.
                // Builtin-sum payload shapes carry placeholder type params;
                // resolve them against the scrutinee's concrete type args
                // (`Var(k)` → `scrutinee_args[k-1]`, mirroring inference).
                for (idx, p) in args.iter().enumerate() {
                    let Pattern::Var(name) = p else {
                        continue;
                    };
                    let raw = payload_tys.get(idx).cloned().unwrap_or(ValueTy::Float);
                    let pty = if is_builtin_sum {
                        match raw {
                            ValueTy::Var(k) => scrutinee_args
                                .get(k.saturating_sub(1) as usize)
                                .cloned()
                                .unwrap_or(ValueTy::Float),
                            t => t,
                        }
                    } else {
                        raw
                    };
                    self.value_locals
                        .last_mut()
                        .unwrap()
                        .insert(name.clone(), (payload_regs[idx], pty));
                }
                self.lower_value(body)
            }
            _ => self.lower_value(body),
        };
        self.value_locals.pop();
        self.value_local_ctors.pop();
        result
    }

    /// Lower the runtime test for one pattern against `scrutinee_reg`, starting
    /// at `self.cur_value_block`. On a match, control flow reaches the returned
    /// block (where the arm's guards and body continue); on a mismatch it jumps
    /// to `else_t`. A `Ctor` pattern resolves its sum/ctor index by constructor
    /// name (`sum_ctor`) and extracts the payload via `ValueMatch`, then
    /// recurses over its argument patterns; `Lit` patterns emit a constant +
    /// `ValueCompare`/`Branch`. Variable leaves bind into the scope on top of
    /// `self.value_locals` (pushed by the caller around each arm). Builtin-sum
    /// payload placeholders (`Var(k)`) resolve against the scrutinee's concrete
    /// type args, falling back to `top_args` (recovered in the match arm) when
    /// `vty` carries none.
    fn lower_pattern_test(
        &mut self,
        pattern: &Pattern,
        scrutinee_reg: usize,
        vty: &ValueTy,
        top_args: &[ValueTy],
        else_t: usize,
        span: Span,
    ) -> Result<usize, CompileError> {
        match pattern {
            Pattern::Var(name) => {
                // Binds the whole matched value; no test is emitted.
                self.value_locals
                    .last_mut()
                    .unwrap()
                    .insert(name.clone(), (scrutinee_reg, vty.clone()));
                Ok(self.cur_value_block)
            }
            Pattern::Wild => Ok(self.cur_value_block),
            Pattern::LitInt(v) => {
                let lit_reg = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueConstInt {
                    dst: lit_reg,
                    value: *v,
                });
                Ok(self.emit_literal_test(scrutinee_reg, lit_reg, else_t))
            }
            Pattern::LitFloat(v) => {
                let lit_reg = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueConstFloat {
                    dst: lit_reg,
                    value: *v,
                });
                Ok(self.emit_literal_test(scrutinee_reg, lit_reg, else_t))
            }
            Pattern::LitBool(v) => {
                let lit_reg = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueBool {
                    dst: lit_reg,
                    value: *v,
                });
                Ok(self.emit_literal_test(scrutinee_reg, lit_reg, else_t))
            }
            Pattern::LitStr(v) => {
                let lit_reg = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueConstString {
                    dst: lit_reg,
                    value: v.clone(),
                });
                Ok(self.emit_literal_test(scrutinee_reg, lit_reg, else_t))
            }
            Pattern::Ctor(name, args) => {
                let (ctor_sum, ctor_idx, payload) =
                    self.sum_ctor(name).ok_or_else(|| CompileError::Type {
                        msg: format!("unknown constructor `{name}`"),
                        span,
                    })?;
                let is_builtin = self.env.ctor_arity(&ctor_sum).is_some();
                // The concrete type args of the scrutinee at this level: a
                // concrete `App`/`Data` type carries them; a bare construction
                // (`Data(name, [])`) or an unbound wire falls back to the
                // top-level recovered args.
                let scrutinee_args: Vec<ValueTy> = match vty {
                    ValueTy::App(_, args) | ValueTy::Data(_, args) if !args.is_empty() => {
                        args.clone()
                    }
                    _ => top_args.to_vec(),
                };
                let payload_regs: Vec<usize> =
                    (0..payload.len()).map(|_| self.fresh_value_reg()).collect();
                let bind_b = self.new_value_block();
                let cur = self.cur_value_block;
                self.set_value_term(
                    cur,
                    ValueTerm::BranchCtor {
                        slot: scrutinee_reg,
                        ctor: ctor_idx as u32,
                        then: bind_b,
                        els: else_t,
                    },
                );
                self.cur_value_block = bind_b;
                self.emit_value(ValueInstr::ValueMatch {
                    dst: payload_regs.clone(),
                    slot: scrutinee_reg,
                    ctor: ctor_idx as u32,
                });
                // Recurse per argument against its payload reg; the last
                // sub-test's matched block is where the arm continues.
                let mut pass = self.cur_value_block;
                for (i, arg) in args.iter().enumerate() {
                    let raw = payload.get(i).cloned().unwrap_or(ValueTy::Float);
                    let pty = if is_builtin {
                        match raw {
                            ValueTy::Var(k) => scrutinee_args
                                .get(k.saturating_sub(1) as usize)
                                .cloned()
                                .unwrap_or(ValueTy::Float),
                            t => t,
                        }
                    } else {
                        raw
                    };
                    pass = self.lower_pattern_test(
                        arg,
                        payload_regs[i],
                        &pty,
                        top_args,
                        else_t,
                        span,
                    )?;
                }
                Ok(pass)
            }
        }
    }

    /// Emit the literal-equality test for a matched scalar: `ValueCompare { Eq }`
    /// against `lit_reg`, branching to a fresh pass block on equality and
    /// `else_t` otherwise. Returns the pass block.
    fn emit_literal_test(&mut self, scrutinee_reg: usize, lit_reg: usize, else_t: usize) -> usize {
        let dst = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueCompare {
            dst,
            op: CmpOp::Eq,
            a: scrutinee_reg,
            b: lit_reg,
        });
        let pass_b = self.new_value_block();
        let cur = self.cur_value_block;
        self.set_value_term(
            cur,
            ValueTerm::Branch {
                cond: dst,
                then: pass_b,
                els: else_t,
            },
        );
        self.cur_value_block = pass_b;
        pass_b
    }

    /// Lower an arm's guard/body alternatives starting at `self.cur_value_block`
    /// (the block where the pattern test succeeded). Each body runs in its own
    /// block and ends with `ValueMove { dst: out, src: body_reg }` +
    /// `Fallthrough(join)`; a failing guard falls through to the next
    /// alternative (or `else_t` after the last). Returns the first
    /// alternative's body type.
    fn lower_match_guards_and_body(
        &mut self,
        arm: &MatchArm,
        out: &usize,
        join: usize,
        else_t: usize,
    ) -> Result<ValueTy, CompileError> {
        let mut first_ty: Option<ValueTy> = None;
        let m = arm.guards.len();
        for (idx, (g, body)) in arm.guards.iter().enumerate() {
            let next = if idx + 1 < m {
                self.new_value_block()
            } else {
                else_t
            };
            if idx == 0 && matches!(g, Expr::Bool(true, _)) {
                // Bare `pattern => body`: no guard to test.
                let (body_reg, body_ty) = self.lower_value(body)?;
                self.emit_value(ValueInstr::ValueMove {
                    dst: *out,
                    src: body_reg,
                });
                self.set_value_term(self.cur_value_block, ValueTerm::Fallthrough(join));
                if first_ty.is_none() {
                    first_ty = Some(body_ty);
                }
                break;
            }
            let (guard_reg, guard_ty) = self.lower_value(g)?;
            if guard_ty != ValueTy::Bool {
                return Err(CompileError::Type {
                    msg: "match guard must be a Bool value".into(),
                    span: g.span(),
                });
            }
            let body_b = self.new_value_block();
            self.set_value_term(
                self.cur_value_block,
                ValueTerm::Branch {
                    cond: guard_reg,
                    then: body_b,
                    els: next,
                },
            );
            self.cur_value_block = body_b;
            let (body_reg, body_ty) = self.lower_value(body)?;
            self.emit_value(ValueInstr::ValueMove {
                dst: *out,
                src: body_reg,
            });
            self.set_value_term(self.cur_value_block, ValueTerm::Fallthrough(join));
            if first_ty.is_none() {
                first_ty = Some(body_ty);
            }
            self.cur_value_block = next;
        }
        Ok(first_ty.unwrap_or(ValueTy::Float))
    }

    /// Resolve a bare constructor name to `(sum type, ctor index, payload)`.
    /// Returns `None` when the name is not a constructor or is ambiguous
    /// (declared in multiple sum types — inference already rejected that).
    fn sum_ctor(&self, name: &str) -> Option<(String, usize, Vec<ValueTy>)> {
        let mut found: Option<(String, usize, Vec<ValueTy>)> = None;
        for (tname, info) in &self.env.data_types {
            if let DataInfo::Sum(ctors) = info {
                for (idx, (c, payload)) in ctors.iter().enumerate() {
                    if c == name {
                        if found.is_some() {
                            return None;
                        }
                        found = Some((tname.clone(), idx, payload.clone()));
                    }
                }
            }
        }
        found
    }

    /// Number of arena slots a value of the given static type occupies when
    /// fully allocated: the root slot plus its transitive field/payload
    /// subtree. Drives the value-arena capacity bound — a compound value
    /// output pins its whole subtree across ticks (the output keeps the root
    /// at rc >= 1, so `drop_ref` never reaches 0 on the children), so the
    /// bound must add the subtree size per output channel, not one slot.
    ///
    /// Shapes: a record is `1 + Σ field subtrees`; a sum is `1 + the largest
    /// ctor's payload subtree` (each ctor's payload subtree is the sum of its
    /// entries); a newtype is `1 + inner subtree`; scalars, func values and
    /// unbound variables occupy exactly one slot. Builtin collections are
    /// exact too: `List`/`Set` are `1 + cap × elem subtree`, `Map` is
    /// `1 + cap × (key subtree + value subtree)`, `Pair` is `1 + both
    /// subtrees`, `Maybe` is `1 + the inner subtree` and `Either` is
    /// `1 + the larger arm's subtree` (the smaller arm can hold no value at
    /// runtime). Recursive data types are cycle-guarded (a re-entered type
    /// contributes one slot) — v1 values are finite literal constructions, so
    /// the bound stays finite.
    fn subtree_size(&self, vty: &ValueTy) -> usize {
        self.subtree_size_impl(vty, &mut HashSet::new())
    }

    fn subtree_size_impl(&self, vty: &ValueTy, visiting: &mut HashSet<String>) -> usize {
        match vty {
            ValueTy::Int
            | ValueTy::Float
            | ValueTy::Bool
            | ValueTy::String
            | ValueTy::Func(_, _)
            | ValueTy::Var(_)
            | ValueTy::TyConVar(_)
            | ValueTy::Cap(_) => 1,
            ValueTy::App(name, args) => match name.as_str() {
                "Maybe" => {
                    1 + args
                        .first()
                        .map(|t| self.subtree_size_impl(t, visiting))
                        .unwrap_or(1)
                }
                "Pair" => {
                    1 + args
                        .iter()
                        .map(|t| self.subtree_size_impl(t, visiting))
                        .sum::<usize>()
                }
                "Either" => {
                    1 + args
                        .iter()
                        .map(|t| self.subtree_size_impl(t, visiting))
                        .max()
                        .unwrap_or(1)
                }
                "List" | "Set" => {
                    let elem = &args[0];
                    1 + Self::ELEM_EST * self.subtree_size_impl(elem, visiting)
                }
                "Map" => {
                    let k = &args[0];
                    let v = &args[1];
                    1 + Self::ELEM_EST
                        * (self.subtree_size_impl(k, visiting)
                            + self.subtree_size_impl(v, visiting))
                }
                _ => {
                    1 + args
                        .iter()
                        .map(|t| self.subtree_size_impl(t, visiting))
                        .sum::<usize>()
                }
            },
            ValueTy::Newtype(name, _) => {
                if !visiting.insert(name.clone()) {
                    return 1;
                }
                let inner = self
                    .env
                    .newtypes
                    .get(name)
                    .map(|n| self.env.vty_of_name(n))
                    .unwrap_or(ValueTy::Float);
                let s = 1 + self.subtree_size_impl(&inner, visiting);
                visiting.remove(name);
                s
            }
            ValueTy::Data(name, _) => {
                if !visiting.insert(name.clone()) {
                    return 1;
                }
                let s = match self.env.data_types.get(name) {
                    Some(DataInfo::Record(fields)) => {
                        1 + fields
                            .iter()
                            .map(|(_, t)| self.subtree_size_impl(t, visiting))
                            .sum::<usize>()
                    }
                    Some(DataInfo::Sum(ctors)) => {
                        // The constructed ctor is a runtime choice; the static
                        // bound is the largest ctor's full payload subtree.
                        1 + ctors
                            .iter()
                            .map(|(_, payload)| {
                                payload
                                    .iter()
                                    .map(|t| self.subtree_size_impl(t, visiting))
                                    .sum::<usize>()
                            })
                            .max()
                            .unwrap_or(0)
                    }
                    None => 1,
                };
                visiting.remove(name);
                s
            }
        }
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
                // A data constructor in a signal position is a type error: value
                // expressions lower on the value track (see `lower_value`).
                if self.env.data_types.contains_key(name.as_str()) || self.sum_ctor(name).is_some()
                {
                    return Err(CompileError::Type {
                        msg: format!(
                            "`{name}` is a value constructor; it cannot be used in a signal expression"
                        ),
                        span: *span,
                    });
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
            Expr::FieldProject { span, .. } => Err(CompileError::Type {
                msg:
                    "field projection is a value expression; it cannot be used in a signal position"
                        .into(),
                span: *span,
            }),
            Expr::FieldUpdate { span, .. } => Err(CompileError::Type {
                msg: "field update is a value expression; it cannot be used in a signal position"
                    .into(),
                span: *span,
            }),
            Expr::Match { span, .. } => Err(CompileError::Type {
                msg: "match is a value expression; it cannot be used in a signal position".into(),
                span: *span,
            }),
            Expr::If { span, .. } => Err(CompileError::Type {
                msg: "if is a value expression; it cannot be used in a signal position".into(),
                span: *span,
            }),
            Expr::Lambda { span, .. } => Err(CompileError::Type {
                msg: "lambda is a value expression; it cannot be used in a signal position".into(),
                span: *span,
            }),
            Expr::Bool(_, _)
            | Expr::ListLit(_, _)
            | Expr::MapLit(_, _)
            | Expr::Cmp { .. }
            | Expr::Logic { .. } => Err(CompileError::Unsupported(
                "value expressions (bool/list/map literals, comparisons, logic) are not yet supported"
                    .into(),
            )),
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
        if let Some(&cell) = self.main_cell_locals.get(name) {
            // A main λ-parameter: materialise the persistent cell's float
            // value into a block register. The cell is read directly — it is
            // persistent, so the value set by `SetParameter` on the control
            // thread is visible here on the next block.
            let dst = self.fresh_reg();
            self.emit(Instr::ReadMainCell { dst, cell });
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
                if let Expr::Lambda {
                    params: lp,
                    body: lbody,
                    span: lspan,
                } = body
                {
                    // A named lambda applied in signal position binds its
                    // parameters to the caller's block registers (value args
                    // materialise as their block regs; a trailing signal wire is
                    // the caller's input block) and lowers the body INLINE —
                    // v1 signal-lambda semantics are call-site wire-binding
                    // (macro-style), consistent with the spec's "signal wires
                    // bind per call" rule. No fragment is involved: the value
                    // args are already block regs (Const/ReadMainCell/param),
                    // so `x * g` lowers as a block-track `Bin`.
                    let n = lp.len();
                    if args.len() < n {
                        return Err(CompileError::Type {
                            msg: format!(
                                "lambda `{name}` applied in signal position expects at least \
                                 {n} argument(s), got {}",
                                args.len()
                            ),
                            span: *lspan,
                        });
                    }
                    let mut scope = HashMap::new();
                    for (idx, p) in lp.iter().enumerate() {
                        scope.insert(p.name.clone(), vec![args[idx]]);
                    }
                    self.locals.push(scope);
                    let out = self.lower(lbody, &args[n..]);
                    self.locals.pop();
                    return out;
                }
                if self.cafs.contains(name) {
                    // The cache is keyed by (name, call-site args): Task 5
                    // threads the caller's signal args into a value-CAF's body
                    // (a signal-lambda macro), so the lowered body is
                    // SITE-DEPENDENT. Two sites applying the same CAF to
                    // different wires must each lower with their own wire
                    // (`main = (amp2 _) , (amp2 _)`), not reuse the first
                    // site's registers. Genuine signal CAFs (empty signal-ins)
                    // never consume the threaded args, so identical args reuse
                    // the shared entry and only distinct-arg sites duplicate
                    // the (still correct) body.
                    let key = (name.to_string(), args.to_vec());
                    if let Some(regs) = self.caf_cache.get(&key) {
                        return Ok(regs.clone());
                    }
                    if self.caf_lifting.contains(name) {
                        return Err(CompileError::Type {
                            msg: format!("recursive CAF definition `{name}`"),
                            span: _span,
                        });
                    }
                    // Lift the closed body once: no signal inputs, so lower it
                    // with the caller's argument registers threaded through
                    // (a closed value def applied in signal position, e.g.
                    // `amp2 = amp 2.0; main = amp2 _`, is a signal-lambda macro:
                    // the value args are consumed positionally and the caller's
                    // signal args flow into the body). A genuine signal CAF has
                    // an empty signal-input set, so the threaded args are simply
                    // never consumed. The output registers are cached for sharing.
                    self.caf_lifting.insert(name.to_string());
                    let res = self.lower(body, args);
                    self.caf_lifting.remove(name);
                    let out = res?;
                    self.caf_cache.insert(key, out.clone());
                    return Ok(out);
                }
                self.lower(body, args)
            }
            _ => Err(CompileError::Type {
                msg: format!("`{name}` is a type declaration, not a signal definition"),
                span: _span,
            }),
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
                        if def.is_decl() {
                            // Type declarations have no signal arity.
                            (0, 1)
                        } else if visited.contains(name) {
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
            // Value expressions are 0→1 value channels: they carry no signal
            // arity. (A combinator mixing value and signal channels is outside
            // v1 scope and errors elsewhere in lowering.)
            Expr::FieldProject { .. }
            | Expr::FieldUpdate { .. }
            | Expr::Match { .. }
            | Expr::If { .. }
            | Expr::Bool(..)
            | Expr::ListLit(..)
            | Expr::MapLit(..)
            | Expr::Cmp { .. }
            | Expr::Logic { .. } => (0, 1),
            Expr::Lambda { .. } => (0, 1),
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
        main_cell_locals: HashMap::new(),
        sample_rate,
        value_blocks: vec![ValueBlock::default()],
        cur_value_block: 0,
        value_entry: 0,
        next_value_reg: 0,
        value_regs_out: Vec::new(),
        value_out_tys: Vec::new(),
        container_tys: Vec::new(),
        value_funcs: Vec::new(),
        value_locals: Vec::new(),
        value_local_ctors: Vec::new(),
        env: &tp.type_env,
        value_inline: HashSet::new(),
        method_lifting: HashSet::new(),
        fragments: Vec::new(),
        fragment_captures: Vec::new(),
        fn_param_tys: tp.fn_param_tys.clone(),
        pending_param_tys: None,
    };

    for (cell_idx, p) in main.params().iter().enumerate() {
        // A main λ-param is both a named parameter (metadata + `param_index`/
        // `set_param` dispatch) and a persistent runtime cell (the actual
        // storage the signal/value tracks read). The cell index matches the
        // params index because main params are interned first.
        lw.intern_param(
            p.name.clone(),
            0.0,
            f64::NEG_INFINITY,
            f64::INFINITY,
            p.span,
        )?;
        lw.main_cell_locals.insert(p.name.clone(), cell_idx);
    }
    // Every main λ-parameter has exactly one persistent cell (cells are
    // indexed by param position, so the maps must line up).
    debug_assert_eq!(lw.main_cell_locals.len(), main.params().len());

    let mut main_args = Vec::with_capacity(num_inputs);
    for index in 0..num_inputs {
        let dst = lw.fresh_reg();
        lw.emit(Instr::LoadInput { dst, index });
        main_args.push(dst);
    }
    // Value-output programs lower through the value track; signal-output
    // programs through the block track. The two tracks never mix — inference
    // guarantees a value-typed body has arity_in() == 0, so `main_args` is
    // empty on the value path.
    let has_value_out = tp.process_ty.outs.iter().any(|c| c.rate == Rate::Value);
    let outs = if has_value_out {
        let (vr, vty) = lw.lower_value(main.body())?;
        lw.value_regs_out.push(vr);
        lw.value_out_tys.push(vty);
        Vec::new()
    } else {
        lw.lower(main.body(), &main_args)?
    };
    if outs.is_empty() && lw.value_regs_out.is_empty() {
        return Err(CompileError::Unsupported(
            "body lowered to 0 outputs, expected at least 1".into(),
        ));
    }
    let num_outputs = outs.len();
    let num_main_cells = lw.main_cell_locals.len();
    // v1 capacity heuristic: the arena never needs more slots than the number
    // of allocations one tick can issue (value registers are per-tick scratch,
    // dropped at tick end), so the count of alloc-producing value instructions
    // is a strict upper bound on simultaneously-live per-tick slots. On top of
    // that:
    //   * each value output channel pins its WHOLE subtree across ticks — the
    //     output keeps the root at rc >= 1, so `drop_ref` never frees the
    //     children and the previous tick's output tree is still fully live
    //     while the next tick allocates. The bound therefore adds the static
    //     subtree size of each output's value type (a scalar adds 1).
    //   * main λ-parameter cells are allocated once at construction and live
    //     for the program's whole lifetime, so they occupy `num_main_cells`
    //     permanent slots.
    // Value-state slots (`ValueStateRead`/`ValueStateWrite`) are not emitted by
    // lowering in v1; when a future task wires `~`/`@` over values, its slot
    // count must join the bound the same way.
    //
    // Fragments allocate too: each fragment's own instructions allocate at most
    // one slot per alloc-producing instruction, each fragment call binds
    // `num_capture_cells` temp-frame cells (one independent copy per env field),
    // copies its `value_ins` argument values into the scratch slice, and copies
    // its `value_outs` results out. The per-fragment totals are summed
    // conservatively — an upper bound on any single call path (fragments may
    // nest, but the sum over all fragments bounds the deepest nesting as well).
    let fragment_capacity = lw
        .fragments
        .iter()
        .map(|f| {
            f.value_blocks
                .iter()
                .flat_map(|b| &b.instrs)
                .filter(|i| is_alloc_producing(i))
                .count()
                + f.num_capture_cells
                + f.sig.value_ins
                + f.sig.value_outs
        })
        .sum::<usize>();
    // Container-typed subexpressions pin their element slots while live, so
    // each contributes its full subtree size (1 + cap × elem slots) to the
    // bound. `value_builtin_ty` propagates the source cap for map/filter/cons/
    // tail/insert, so these types carry the exact capacities the runtime
    // allocates (a Cap(0) map/filter result type would undercount by cap(elem)
    // element slots). Empty `list`/`empty_map`/`empty_set` legitimately have
    // Cap(0): the empty container occupies one slot and element slots are
    // allocated by the cons/insert ops themselves, each already counted.
    let container_capacity = lw
        .container_tys
        .iter()
        .map(|t| lw.subtree_size(t))
        .sum::<usize>();
    let value_capacity = lw
        .value_blocks
        .iter()
        .flat_map(|b| &b.instrs)
        .filter(|i| is_alloc_producing(i))
        .count()
        + lw.value_out_tys
            .iter()
            .map(|t| lw.subtree_size(t))
            .sum::<usize>()
        + container_capacity
        + num_main_cells
        + fragment_capacity;
    // Pre-allocated function-call scratch: the runtime call stack never holds
    // more than one frame per fragment (recursion is rejected at inference, so
    // no fragment can recur on a dispatch chain), so the total fragment count
    // is a strict upper bound on the deepest nesting. Each frame needs at most
    // `max_fragment_regs` value-register slots, giving the product below.
    let max_call_depth = lw.fragments.len();
    let max_fragment_regs = lw
        .fragments
        .iter()
        .map(|f| f.num_value_regs)
        .max()
        .unwrap_or(0);
    let max_call_regs = max_call_depth * max_fragment_regs;
    // Terminate the program's value track: the entry block's fallthrough chain
    // ends here. Every existing program is a single block ending in `Halt`.
    lw.set_value_term(lw.cur_value_block, ValueTerm::Halt);
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
        num_main_cells,
        value_blocks: std::mem::take(&mut lw.value_blocks),
        value_entry: lw.value_entry,
        num_value_regs: lw.next_value_reg,
        value_output_regs: lw.value_regs_out,
        value_funcs: lw.value_funcs,
        fragments: lw.fragments,
        max_call_regs,
        value_state: ValueLayout {
            capacity: value_capacity,
            value_state_slots: 0,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::BuiltinKind;
    use crate::ir::ValueInstr;
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
            type_env: typed.type_env.clone(),
            fn_param_tys: typed.fn_param_tys.clone(),
        };
        lower_with_cafs(&tp, &TestSigs, 44_100.0, &tp.cafs).unwrap()
    }

    /// Build a `Lowerer` for testing lowering helpers (`free_vars`) directly on
    /// a parsed AST, bypassing inference (which rejects higher-order programs
    /// that `free_vars` must still handle correctly).
    fn lw<'a>(env: &'a TypeEnv, cafs: &'a HashSet<String>) -> Lowerer<'a> {
        Lowerer {
            defs: HashMap::new(),
            sigs: &TestSigs,
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
            main_cell_locals: HashMap::new(),
            sample_rate: 44_100.0,
            value_blocks: vec![ValueBlock::default()],
            cur_value_block: 0,
            value_entry: 0,
            next_value_reg: 0,
            value_regs_out: Vec::new(),
            value_out_tys: Vec::new(),
            container_tys: Vec::new(),
            value_funcs: Vec::new(),
            value_locals: Vec::new(),
            value_local_ctors: Vec::new(),
            env,
            value_inline: HashSet::new(),
            method_lifting: HashSet::new(),
            fragments: Vec::new(),
            fragment_captures: Vec::new(),
            fn_param_tys: HashMap::new(),
            pending_param_tys: None,
        }
    }

    #[test]
    fn free_vars_walks_apply_callee() {
        // `outer = fn f -> fn x -> f x`: the inner lambda applies the closure
        // `f` from the ENCLOSING lambda. `f` appears only as the callee NAME of
        // an `Apply`, but it is a free variable of the body and must be
        // captured — otherwise the env snapshot misses it and lowering reports
        // "unknown `f` in value lowering" (Task 6 nested-HOF).
        let p = parse(
            &tokenize("outer = fn f -> fn x -> f x; main = 1.0").unwrap(),
            "outer = fn f -> fn x -> f x; main = 1.0".as_bytes(),
        )
        .unwrap();
        let outer = p.defs.iter().find(|d| d.name() == "outer").unwrap();
        let Expr::Lambda { body: inner, .. } = outer.body() else {
            panic!("outer body must be a lambda");
        };
        let Expr::Lambda { body: apply, .. } = inner.as_ref() else {
            panic!("outer body must be a nested lambda");
        };
        let apply = apply.as_ref();
        assert!(matches!(apply, Expr::Apply { .. }));

        let env = TypeEnv::default();
        let empty = HashSet::new();
        let lw = lw(&env, &empty);
        let mut bound = HashSet::new();
        bound.insert("x".to_string());
        assert_eq!(
            lw.free_vars(apply, &bound),
            vec!["f".to_string()],
            "the applied closure name `f` must be captured as a free variable"
        );
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
    fn main_param_lowers_to_cell_binding() {
        // main's λ-param `gain` becomes a persistent runtime-stack cell: the
        // signal context reads it via `Instr::ReadMainCell`, not a named
        // `ReadParam` (which would route through the params vec instead).
        let ir = ir_of("main gain = _ * gain");
        assert_eq!(ir.num_main_cells, 1);
        assert!(
            ir.instrs
                .iter()
                .any(|i| matches!(i, Instr::ReadMainCell { cell: 0, .. })),
            "expected a ReadMainCell instruction for `gain`"
        );
        assert!(
            !ir.instrs
                .iter()
                .any(|i| matches!(i, Instr::ReadParam { .. })),
            "main λ-params must not lower to ReadParam"
        );
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
            type_env: typed.type_env.clone(),
            fn_param_tys: typed.fn_param_tys.clone(),
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
            type_env: typed.type_env.clone(),
            fn_param_tys: typed.fn_param_tys.clone(),
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
            type_env: typed.type_env.clone(),
            fn_param_tys: typed.fn_param_tys.clone(),
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
            type_env: typed.type_env.clone(),
            fn_param_tys: typed.fn_param_tys.clone(),
        };
        let res = lower_with_cafs(&tp, &crate::builtin::NoSigs, 44100.0, &cafs);
        assert!(res.is_err());
    }

    #[test]
    fn record_construct_lowers_to_value_track() {
        let ir = ir_of("data Point = { x: Float, y: Float }; main = Point { x: 1.0, y: 2.0 }");
        assert!(ir.value_blocks[0]
            .instrs
            .iter()
            .any(|i| matches!(i, ValueInstr::ValueConstructRecord { .. })));
    }

    #[test]
    fn field_project_lowers_to_value_project() {
        let ir =
            ir_of("data Point = { x: Float, y: Float }; p = Point { x: 1.0, y: 2.0 }; main = p.x");
        assert!(ir.value_blocks[0]
            .instrs
            .iter()
            .any(|i| matches!(i, ValueInstr::ValueProject { .. })));
    }

    #[test]
    fn newtype_construct_lowers_to_value_newtype() {
        let ir = ir_of("newtype Hz = Float; main = Hz 440.0");
        assert!(ir.value_blocks[0]
            .instrs
            .iter()
            .any(|i| matches!(i, ValueInstr::ValueNewtype { .. })));
    }

    #[test]
    fn newtype_bare_ref_is_compile_error() {
        // `Hz` alone is not a value — the constructor requires its argument.
        let p = parse(
            &tokenize("newtype Hz = Float; main = Hz").unwrap(),
            "newtype Hz = Float; main = Hz".as_bytes(),
        )
        .unwrap();
        assert!(infer_program(&p).is_err());
    }

    #[test]
    fn match_lowers_to_value_match() {
        let ir = ir_of(
            "data Shape = Circle Float | Rect Float Float; s = Circle 1.5; main = match s of { Circle r => r; Rect w h => w; }",
        );
        assert!(ir.value_blocks[0]
            .instrs
            .iter()
            .any(|i| matches!(i, ValueInstr::ValueMatch { .. })));
    }

    #[test]
    fn match_selects_matching_arm_not_first() {
        // The matching arm is NOT first: static dispatch must select Circle's
        // arm, emitting a single ValueMatch for Circle's ctor index and routing
        // the output to the Circle arm's body register (not Rect's `w`, which
        // would be None at runtime).
        let ir = ir_of(
            "data Shape = Circle Float | Rect Float Float; s = Circle 1.5; main = match s of { Rect w h => w; Circle r => r; }",
        );
        let matches = ir.value_blocks[0]
            .instrs
            .iter()
            .filter(|i| matches!(i, ValueInstr::ValueMatch { .. }))
            .count();
        assert_eq!(
            matches, 1,
            "static dispatch must emit exactly one ValueMatch"
        );
        let vm = ir.value_blocks[0]
            .instrs
            .iter()
            .find(|i| matches!(i, ValueInstr::ValueMatch { .. }))
            .unwrap();
        let ctor = match vm {
            ValueInstr::ValueMatch { ctor, .. } => *ctor,
            _ => u32::MAX,
        };
        assert_eq!(
            ctor, 0,
            "Circle is the first declared ctor of Shape (index 0)"
        );
        assert_eq!(
            ir.value_output_regs,
            vec![2],
            "output must be the Circle arm's payload reg (r), not Rect's w"
        );
    }

    #[test]
    fn lower_match_runtime_dispatch_emits_branch_ctor() {
        let src = "data Shape = Circle Float | Rect Float Float; \
                   main = match _ of { Circle r => r; Rect w h => w; };";
        let toks = tokenize(src).unwrap();
        let p = parse(&toks, src.as_bytes()).unwrap();
        let tp = infer_program(&p).unwrap();
        let ir = lower(&tp).unwrap();
        assert!(
            ir.value_blocks
                .iter()
                .any(|b| matches!(b.term, ValueTerm::BranchCtor { .. })),
            "a non-static match must lower to BranchCtor dispatch"
        );
    }

    #[test]
    fn lower_match_literal_and_wildcard() {
        let src = "main = match 0 of { 0 => 1.0; _ => 2.0; };";
        let toks = tokenize(src).unwrap();
        let p = parse(&toks, src.as_bytes()).unwrap();
        let tp = infer_program(&p).unwrap();
        let ir = lower(&tp).unwrap();
        assert!(
            ir.value_blocks
                .iter()
                .any(|b| matches!(b.term, ValueTerm::Branch { .. })),
            "a literal match must lower to a Branch"
        );
    }

    #[test]
    fn match_wire_scrutinee_dispatches_at_runtime() {
        // An unbound `_` scrutinee has no statically-known constructor: the
        // match must lower to runtime BranchCtor dispatch instead of the old
        // static-only compile error.
        let p = parse(
            &tokenize(
                "data Shape = Circle Float | Rect Float Float; main = match _ of { Circle r => r; Rect w h => w; }",
            )
            .unwrap(),
            "data Shape = Circle Float | Rect Float Float; main = match _ of { Circle r => r; Rect w h => w; }"
                .as_bytes(),
        )
        .unwrap();
        let tp = infer_program(&p).unwrap();
        let ir = lower(&tp).unwrap();
        assert!(
            ir.value_blocks
                .iter()
                .any(|b| matches!(b.term, ValueTerm::BranchCtor { .. })),
            "a wire scrutinee must lower to BranchCtor dispatch"
        );
    }

    #[test]
    fn lower_if_emits_branch() {
        // A non-constant Bool condition (`g > 0.5` on a main λ-param, read per
        // tick) must lower to a runtime Branch term.
        let ir = ir_of("main g = if g > 0.5 then 1.0 else 2.0;");
        assert!(
            ir.value_blocks
                .iter()
                .any(|b| matches!(b.term, ValueTerm::Branch { .. })),
            "a non-constant if condition must lower to a Branch"
        );
    }

    #[test]
    fn lower_if_static_literal_skips_branch() {
        // A literal Bool condition selects one branch at compile time: a single
        // block, no Branch term.
        let ir = ir_of("main = if false then 1.0 else 2.0;");
        assert!(
            !ir.value_blocks
                .iter()
                .any(|b| matches!(b.term, ValueTerm::Branch { .. })),
            "a literal-Bool if must not lower to a Branch"
        );
    }

    #[test]
    fn collection_subtree_sizes_use_open_collection_estimate() {
        // Open collections: the estimator is ELEM_EST element slots per
        // container (element counts are data-dependent, not in the type).
        let env = TypeEnv::default();
        let empty = HashSet::new();
        let lw = lw(&env, &empty);
        assert_eq!(
            lw.subtree_size(&ValueTy::App("List".into(), vec![ValueTy::Float])),
            1 + Lowerer::ELEM_EST
        );
        assert_eq!(
            lw.subtree_size(&ValueTy::App(
                "Map".into(),
                vec![ValueTy::String, ValueTy::Float]
            )),
            1 + Lowerer::ELEM_EST * 2
        );
        assert_eq!(
            lw.subtree_size(&ValueTy::App("Set".into(), vec![ValueTy::Int])),
            1 + Lowerer::ELEM_EST
        );
        assert_eq!(
            lw.subtree_size(&ValueTy::App("Maybe".into(), vec![ValueTy::Float])),
            2
        );
        assert_eq!(
            lw.subtree_size(&ValueTy::App(
                "Pair".into(),
                vec![ValueTy::Float, ValueTy::Int]
            )),
            3
        );
        assert_eq!(
            lw.subtree_size(&ValueTy::App(
                "Either".into(),
                vec![ValueTy::Float, ValueTy::Int]
            )),
            2
        );
    }
}
