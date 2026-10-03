# SP-2: Kleisli + Value-Track Arrow — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `data Kleisli m a b = { unKleisli: a -> m b }`, a builtin `Arrow` typeclass (`arr`/`first`/`compose` + `second`/`both`/`fan` defaults), and `instance Monad m => Arrow (Kleisli m)` — the first **constraint-qualified instance** — all registered via `CATEGORY_PRELUDE`.

**Architecture:** This sub-project first closes an **HKT gap** (`ValueTy::TyConApp` — applying a data-type *parameter* as a constructor, `m b` in `data K m a b = { f: a -> m b }`), unifies `,` (Par) as a **channel tuple** (`signal,signal` → parallel; `value,value` → `Pair`; mixed → error) with zero parser migration, adds **tuple types** `(b, d)` → `Pair b d`, **field-projection application** (`(k.unKleisli) p.first`), **constraint-qualified instances** (`instance (Monad m) => Arrow (Kleisli m)`) with head-arg binding + constraint discharge, **default methods** (instance body > default > error), and a **single-field record constructed newtype-style** (`Kleisli (fn x -> …)`). Kleisli/Arrow/instance live in `CATEGORY_PRELUDE`.

**Tech Stack:** Rust, no new external dependencies. Branch `feature/rill-lang-categories`.

---

## File map (SP-2)

| File | Change |
|---|---|
| `rill-lang/src/types/ty.rs` | `ValueTy::TyConApp`; `resolve_value` for it; `InstanceInfo` + `head_args`/`constraints`; `TypeclassInfo::methods` optional defaults; `class_var_arity_loose`; prelude extension |
| `rill-lang/src/types/unify.rs` | `TyConApp` unification + `value_contains_var` |
| `rill-lang/src/types/infer.rs` | `data_field_vty` → `TyConApp`; `infer_par` value branch; `infer_apply`/`infer_ref` constraint-instance resolution; default-method fallback; record-ctor slot type-arg collection for `TyConApp` fields |
| `rill-lang/src/lower.rs` | `Expr::Par` value branch (Pair ctor); `TyConApp` in `subtree_size`; constraint-instance resolution; default-method fallback; single-field record newtype-style ctor; field-projection application |
| `rill-lang/src/lexer.rs` | (unchanged — `,` stays `Tok::Comma`) |
| `rill-lang/src/parser.rs` | tuple types `(b, d)`; field-projection application `(k.unKleisli) p.first`; constraint instance syntax; typeclass default bodies `= …` |
| `rill-lang/src/ast.rs` | `Def::Instance { constraints, head }`; `TypeclassInfo` default bodies in `Def::Typeclass`; `TypeExpr` tuple? (parser desugars to `Pair`, no AST change needed) |
| `rill-lang/src/reduce.rs` | (unchanged — parser desugars; reduction unchanged) |
| `rill-lang/tests/arrow.rs` | new integration tests |
| `rill-lang/README.md`, `docs/src/guides/rill-lang.md`, `CHANGELOG.md` | docs |

Verify: `cargo test -p rill-lang` per task; `cargo test --workspace`, `cargo clippy --all-features --workspace`, `cargo fmt` before finishing.

---

## Task 1: `ValueTy::TyConApp` — HKT in data fields

**Problem:** `data K m a b = { f: a -> m b }` fails today: `data_field_vty` (infer.rs:134) maps `TApp("m", [b])` to `ValueTy::App("m", …)` — a literal string head, not the type *parameter*. Unifying with `App("Maybe", …)` errors (`cannot unify … App("m", …) with App("Maybe", …)`).

**Files:** `types/ty.rs`, `types/unify.rs`, `types/infer.rs`, `lower.rs`

- [ ] **Step 1: Add the variant + resolution**

`ty.rs` `ValueTy`:

```rust
/// Application of a type-constructor *variable* to arguments: `m b` in a
/// data-field type (`data K m a b = { f: a -> m b }`). The head is the id of
/// the data-type's type parameter (same id space as `Var`/`TyConVar`); when it
/// resolves to a concrete constructor the application becomes `App(c, args)`.
TyConApp(TypeVarId, Vec<ValueTy>),
```

In `resolve_value_depth` (ty.rs:853), add a `TyConApp` arm **before** the compound `Data/Newtype/App` arm:

```rust
ValueTy::TyConApp(f, args) => {
    // Resolve the head variable; a bound head (a concrete constructor)
    // rewrites the application to `App(c, resolved args)`.
    let resolved = self.resolve_value_depth(&ValueTy::TyConVar(*f), depth + 1);
    match resolved {
        ValueTy::TyConVar(_) => ValueTy::TyConApp(*f, resolved_args),
        ValueTy::App(c, _) => ValueTy::App(c, resolved_args),
        other => ValueTy::TyConApp(*f, vec![other]),
    }
}
```

(compute `resolved_args` by mapping `self.resolve_value_depth(a, depth+1)` over `args` first).

- [ ] **Step 2: Unification**

`unify.rs` — add a case (before the fallback `_ =>`):

```rust
// `m b` (a type-constructor variable applied to args) unifies with a concrete
// application `Maybe Float`: bind the head var to the constructor, unify args.
(ValueTy::TyConApp(f, ax), other @ ValueTy::App(..))
| (other @ ValueTy::App(..), ValueTy::TyConApp(f, ax)) => {
    let ValueTy::App(c, ay) = other else { unreachable!() };
    if ax.len() != ay.len() {
        return Err(CompileError::Type {
            msg: format!("cannot unify type-constructor application {a:?} with {b:?}"),
            span,
        });
    }
    // Bind the head variable to the bare constructor (TyConVar ~ App-head).
    subst.value_map.insert(*f, ValueTy::App(c.clone(), vec![]));
    for (x, y) in ax.iter().zip(ay.iter()) {
        unify_value(x, y, subst, span)?;
    }
    Ok(())
}
// m b ~ n b' (two tycon-var applications).
(ValueTy::TyConApp(f, ax), ValueTy::TyConApp(g, ay)) => {
    unify_value(&ValueTy::TyConVar(*f), &ValueTy::TyConVar(*g), subst, span)?;
    if ax.len() != ay.len() {
        return Err(CompileError::Type {
            msg: format!("cannot unify type-constructor applications {a:?} with {b:?}"),
            span,
        });
    }
    for (x, y) in ax.iter().zip(ay.iter()) {
        unify_value(x, y, subst, span)?;
    }
    Ok(())
}
```

Update `value_contains_var_impl` (unify.rs:50): add `ValueTy::TyConApp(f, args)` — check the head id `*f` against `v`, then recurse into args.

- [ ] **Step 3: `data_field_vty` emits `TyConApp`**

`infer.rs:134` `data_field_vty` — the `TApp(head, args)` arm becomes:

```rust
crate::ast::TypeExpr::TApp(head, args) => {
    // A type PARAMETER applied as a constructor (`m b` in `data K m a b`)
    // becomes a type-constructor application whose head is the parameter's
    // var id. A concrete constructor stays `App`.
    if let Some(k) = tyvars.iter().position(|t| t == head) {
        ValueTy::TyConApp(
            (k + 1) as u32,
            args.iter().map(|a| data_field_vty(env, tyvars, a)).collect(),
        )
    } else {
        ValueTy::App(
            head.clone(),
            args.iter().map(|a| data_field_vty(env, tyvars, a)).collect(),
        )
    }
}
```

- [ ] **Step 4: `subtree_size` in lowering**

`lower.rs` `subtree_size_impl` (lower.rs:2792): the `ValueTy::Var(_) | ValueTy::TyConVar(_) => 1` arm gains `| ValueTy::TyConApp(_, args)` and sums its args:

```rust
ValueTy::TyConApp(_, args) => 1 + args
    .iter()
    .map(|t| self.subtree_size_impl(t, visiting))
    .sum::<usize>(),
```

- [ ] **Step 4b: record-ctor slot collection for parameterized fields**

The record-constructor type-arg collection (infer.rs:2295-2316) fills `slots` only from top-level `ValueTy::Var(k)` field types. A parameterized field that is *not* a bare var (`unKleisli: a -> m b` → `Func([Var(2)],[TyConApp(1,[Var(3)])])`) never populates its slots. Replace the field-scan with a per-parameter resolve from the subst after unifying the fields:

```rust
// Parameterized user data (`data Kleisli m a b`): resolve each type parameter
// from the substitution built by unifying the fields. `Var(k+1)` and the
// `TyConApp` head share one id space, so resolving `Var(k+1)` yields the
// concrete type (m → Maybe, a → Float, b → Float).
if let Some(arity) = ctx.env.data_arities.get(name) {
    if *arity > 0 {
        // … existing field unification loop first (unchanged) …
        let arg_tys: Vec<ValueTy> = (0..*arity)
            .map(|k| {
                let pty = ctx.subst.resolve_value(&ValueTy::Var((k + 1) as u32));
                match pty {
                    ValueTy::Var(_) => ctx.fresh_vty(),  // unconstrained param
                    t => t,
                }
            })
            .collect();
        return Ok(ArrowTy::value_channel(ValueTy::Data(name.into(), arg_tys)));
    }
}
```

This replaces the `slots` loop; regression: plain parameterized records (`data Box a = { value: a }`) must still produce `Data("Box", [Float])`.

- [ ] **Step 5: Test — HKT data field**

`tests/arrow.rs` (create):

```rust
#[test]
fn hkt_data_field_applies_type_parameter() {
    let src = r#"
        data K m a b = { f: a -> m b };
        k = K { f: fn x -> Just x };
        main = match (k.f 3.0) of { Just v => v; Nothing => 0.0; };
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 3.0);
}
```

(If `k.f 3.0` fails to parse yet, apply a `let u = k.f in u 3.0` form — the parser task is Task 4; the *type* behavior is what this task verifies. Use the let-form until Task 4 lands, then switch.)

Run: `cargo test -p rill-lang --test arrow`.
Commit: `git add -A && git commit -m 'feat(rill-lang): TyConApp — apply type params as constructors in data fields'`

---

## Task 2: Unify `,` as a channel tuple

**Problem:** `,` is `Expr::Par`, currently signal-only. We make it a **channel tuple**: `signal,signal` stays the block-diagram parallel; `value,value` becomes `Pair a b`; `signal,value` errors. Zero parser/lexer change (the lexer already emits `Tok::Comma` for `,`).

**Files:** `types/infer.rs`, `lower.rs`

- [ ] **Step 1: Test**

```rust
#[test]
fn value_tuple_is_pair() {
    // `(1.0, 2.0)` in value position is a `Pair { first, second }`.
    let mut prog = compile::<f32>("main = (1.0, 2.0);").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    match prog.arena().get(prog.value_outputs()[0].unwrap()).unwrap() {
        rill_lang::arena::Value::Record(fields) => {
            assert_eq!(fields.len(), 2);
            assert_eq!(prog.arena().get(fields[0]).unwrap(), &rill_lang::arena::Value::Float(1.0));
            assert_eq!(prog.arena().get(fields[1]).unwrap(), &rill_lang::arena::Value::Float(2.0));
        }
        other => panic!("expected Pair Record, got {other:?}"),
    }
}
```

- [ ] **Step 2: `infer_par` value branch**

`infer.rs:3364` `infer_par`:

```rust
fn infer_par(ctx, lhs, rhs, span) -> Result<ArrowTy, CompileError> {
    let a = infer_expr(ctx, lhs)?;
    let b = infer_expr(ctx, rhs)?;
    // Channel tuple: value,value → `Pair a b`; signal,signal → parallel
    // composition; mixed → compile error.
    let a_value = a.arity_in() == 0 && a.arity_out() == 1 && a.outs[0].rate == Rate::Value;
    let b_value = b.arity_in() == 0 && b.arity_out() == 1 && b.outs[0].rate == Rate::Value;
    if a_value && b_value {
        return Ok(ArrowTy::value_channel(ValueTy::App(
            "Pair".into(),
            vec![a.outs[0].vty.clone(), b.outs[0].vty.clone()],
        )));
    }
    if a_value != b_value {
        return Err(CompileError::Type {
            msg: "cannot mix a value and a signal in a tuple `,` (use one track)".into(),
            span,
        });
    }
    Ok(par(&a, &b))
}
```

- [ ] **Step 3: `lower_value_expected` value-Par branch**

`lower.rs` `Expr::Par` in value position — add a `Pair`-construction arm. `lower_value_expected` currently has no `Expr::Par` arm (it falls to `_ => Err("unsupported expression in value position")`). Add:

```rust
Expr::Par(lhs, rhs, span) => {
    // Channel tuple in value position: `value , value` is `Pair { first, second }`.
    let (lr, _) = self.lower_value(lhs)?;
    let (rr, _) = self.lower_value(rhs)?;
    let dst = self.fresh_value_reg();
    self.emit_value(ValueInstr::ValueConstructRecord {
        dst,
        fields: vec![lr, rr],
    });
    Ok((dst, ValueTy::App("Pair".into(), vec![ValueTy::Float, ValueTy::Float])))
}
```

(Value-track record field order matches declared `Pair { first, second }` order, so `fields[0]`/`fields[1]` are `first`/`second`. If the type needs the exact element types, resolve them from the two sub-lowerings' returned `ValueTy`.)

- [ ] **Step 4: Run + commit**

Run: `cargo test -p rill-lang --test arrow` + full `cargo test -p rill-lang` (regression: signal `,` untouched).
Commit: `git add -A && git commit -m 'feat(rill-lang): unify Par as channel tuple — value,value is Pair'`

---

## Task 3: Tuple types `(b, d)` → `Pair b d`

**Problem:** `Arrow`'s signatures use `(b, d)` pairs (spec §SP-2). The type parser (`parse_type_expr`, parser.rs:490) doesn't handle commas inside `(...)`.

**Files:** `parser.rs`

- [ ] **Step 1: Test**

```rust
#[test]
fn tuple_type_desugars_to_pair() {
    let src = r#"
        typeclass T a where { m: a (Float, Float) -> Float; }
        main = 1.0;
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
}
```

(Compilation alone proves `(Float, Float)` parsed as `Pair Float Float`.)

- [ ] **Step 2: `parse_type_single` / `parse_type_expr` tuple handling**

In `parse_type_expr` (parser.rs:490), after parsing the first atom, if the next token is `Tok::Comma`, parse a comma-separated list of atoms and desugar to `TApp("Pair", items)`:

```rust
// `(b, d)` tuple sugar → `Pair b d`.
if self.peek().tok == Tok::Comma {
    let mut items = vec![first];
    while self.peek().tok == Tok::Comma {
        self.bump();
        items.push(self.parse_type_single()?);
    }
    if items.len() != 2 {
        return Err(self.error("tuples in types are binary (use Pair)"));
    }
    return Ok(TypeExpr::TApp("Pair".into(), items));
}
```

(Only the 2-tuple is supported; `(a, b, c)` errors. The `(…)` grouping in `parse_type_atom` already unwraps parens, so `a (Pair b d)` parses `(Pair b d)` via `parse_type_expr` — the tuple branch must live inside the paren-unwrapping path. Place the check in `parse_type_single`'s `LParen` arm after unwrapping, so `(b, d)` in any position works.)

- [ ] **Step 3: Run + commit**

Run: `cargo test -p rill-lang --test arrow`.
Commit: `git add -A && git commit -m 'feat(rill-lang): tuple types (b, d) desugar to Pair b d'`

---

## Task 4: Field-projection application `(k.unKleisli) p.first`

**Problem:** the Arrow instance body must extract the monadic function (`k.unKleisli`) and apply it (`(k.unKleisli) p.first`). The parser only supports application of *names* (`Expr::Apply { name, args }`); a parenthesized projection isn't applied.

**Files:** `parser.rs`, `ast.rs`, `reduce.rs`, `types/infer.rs`, `lower.rs`

- [ ] **Step 1: Test**

```rust
#[test]
fn field_projection_applies_as_function() {
    // `(b.f) 3.0` applies the projected closure.
    let src = r#"
        data Box = { f: Float -> Float };
        b = Box { f: fn x -> x * 2.0 };
        main = (b.f) 3.0;
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 6.0);
}
```

- [ ] **Step 2: AST — `Expr::ApplyExpr`**

`ast.rs`:

```rust
/// Application of an arbitrary expression (not just a name) to arguments:
/// `(k.unKleisli) p.first`, `(fn x -> x) 1.0`.
ApplyExpr {
    /// The callee expression (a closure-valued projection, lambda, …).
    callee: Box<Expr>,
    /// Argument expressions.
    args: Vec<Expr>,
    /// Full span.
    span: Span,
},
```

Add a `span()` arm in the `span` match (ast.rs:287).

- [ ] **Step 3: Parser**

In `parse_atom` (parser.rs:1010), the `LParen` arm — after unwrapping the inner expression, if the next token starts an atom, collect arguments and build `Expr::ApplyExpr`:

```rust
Tok::LParen => {
    let start = t.span.start;
    let inner = self.parse_expr(0, false)?;
    self.eat(&Tok::RParen)?;
    if self.peek().tok == Tok::Dot {
        self.parse_field(inner, start)
    } else if is_atom_start(&self.peek().tok) {
        // `(expr) arg1 arg2` — apply a parenthesized expression.
        let mut args = Vec::new();
        while is_atom_start(&self.peek().tok) {
            args.push(self.parse_atom()?);
        }
        let span = self.span_from(start);
        Ok(Expr::ApplyExpr {
            callee: Box::new(inner),
            args,
            span,
        })
    } else {
        Ok(inner)
    }
}
```

- [ ] **Step 4: reduce.rs**

`substitute` (reduce.rs:42) and `reduce_expr` (reduce.rs:269): add passthrough arms that recurse into `callee` and `args` (mirror the `Expr::Apply` arms but recurse the callee as a sub-expression). `contains_name` (reduce.rs:577) and any other exhaustive `Expr` match: add `ApplyExpr` recursion. (Compiler-driven — fix all non-exhaustive matches.)

- [ ] **Step 5: inference**

`infer_expr_expected` (infer.rs:1359): add

```rust
Expr::ApplyExpr { callee, args, span } => infer_apply_expr(ctx, callee, args, *span),
```

New helper (mirrors the local-Func-apply path in `infer_apply_impl`):

```rust
/// `(expr) a b` — apply a closure-valued expression. Infer the callee, unify it
/// against a fresh `Func` signature, infer and check args, return the result.
fn infer_apply_expr(ctx, callee, args, span) -> Result<ArrowTy, CompileError> {
    let ct = infer_expr(ctx, callee)?;
    if ct.arity_out() != 1 || ct.outs[0].rate != Rate::Value {
        return Err(CompileError::Type {
            msg: "applied expression must be a value (function)".into(),
            span,
        });
    }
    let cty = ct.outs[0].vty.clone();
    let resolved = ctx.subst.resolve_value(&cty);
    let (arg_tys, ret_tys) = match resolved {
        ValueTy::Func(a, r) => (a, r),
        _ => {
            let a = (0..args.len()).map(|_| ctx.fresh_vty()).collect::<Vec<_>>();
            let r = vec![ctx.fresh_vty()];
            unify_value(&cty, &ValueTy::Func(a.clone(), r.clone()), &mut ctx.subst, span)?;
            (a, r)
        }
    };
    if args.len() != arg_tys.len() {
        return Err(CompileError::Type {
            msg: format!("expression expects {} argument(s), got {}", arg_tys.len(), args.len()),
            span,
        });
    }
    for (a, pt) in args.iter().zip(arg_tys.iter()) {
        let vt = infer_method_value_vty(ctx, a, "argument")?;
        unify_value(&vt, pt, &mut ctx.subst, a.span())?;
    }
    Ok(ArrowTy::value_channel(ret_tys.first().cloned().unwrap_or(ValueTy::Float)))
}
```

- [ ] **Step 6: lowering**

`lower_value_expected` (lower.rs:257): add

```rust
Expr::ApplyExpr { callee, args, span } => {
    let (cr, cty) = self.lower_value(callee)?;
    let (arg_tys, ret_tys) = match &cty {
        ValueTy::Func(a, r) => (a.clone(), r.clone()),
        _ => return Err(CompileError::Type {
            msg: "applied expression must be a function value".into(),
            span: *span,
        }),
    };
    if args.len() > arg_tys.len() {
        return Err(CompileError::Type {
            msg: format!("expression expects {} argument(s), got {}", arg_tys.len(), args.len()),
            span: *span,
        });
    }
    let mut arg_regs = Vec::with_capacity(args.len());
    for a in args {
        let (ar, _) = self.lower_value(a)?;
        arg_regs.push(ar);
    }
    let ret_ty = ret_tys.first().cloned().unwrap_or(ValueTy::Float);
    let dst = self.fresh_value_reg();
    self.emit_value(ValueInstr::ValueCallFunc {
        dst,
        closure_slot: cr,
        args: arg_regs,
    });
    Ok((dst, ret_ty))
}
```

(Partial application of an expression — `(f) 1.0` with `f` expecting 2 — is out of scope; require full application here.)

- [ ] **Step 7: Run + commit**

Run: `cargo test -p rill-lang --test arrow` + full suite (many `Expr` matches need the new arm — fix all compiler-driven exhaustiveness errors).
Commit: `git add -A && git commit -m 'feat(rill-lang): apply field projections (k.unKleisli) p.first'`

---

## Task 5: Constraint-qualified instances — syntax + AST + registration

**Problem:** `instance Monad m => Arrow (Kleisli m)` needs (a) a constraint list before the class name, (b) a parenthesized partial-application head.

**Files:** `parser.rs`, `ast.rs`, `types/ty.rs`

- [ ] **Step 1: AST**

`ast.rs` `Def::Instance`:

```rust
Instance {
    /// Class name.
    class: String,
    /// Concrete type the instance is for.
    ty: TypeName,
    /// Optional constraint list: (class, type variable) before `=>`,
    /// e.g. `Monad m` in `instance Monad m => Arrow (Kleisli m)`.
    constraints: Vec<(String, String)>,
    /// Head type-constructor args (partial application). `Kleisli m` →
    /// head `Kleisli`, `head_args = ["m"]`; a plain `List` → `head_args = []`.
    head_args: Vec<String>,
    /// Method bodies: (method name, parameter bindings, body expr).
    method_bodies: Vec<(String, Vec<Param>, Expr)>,
    /// Span.
    span: Span,
},
```

- [ ] **Step 2: Parser**

`parse_instance_def` (parser.rs:558). Current: `instance C T where { … }` (two idents). New grammar:

```
instance [ (Constraint)* => ] Class ( Head [args]* ) where { … }
```

```rust
fn parse_instance_def(&mut self) -> Result<Def, CompileError> {
    let start = self.bump().span.start;
    let mut constraints = Vec::new();
    if self.peek().tok == Tok::LParen {
        // `(Monad m) =>`
        self.bump();
        loop {
            let (class, _) = self.expect_ident()?;
            let (var, _) = self.expect_ident()?;
            constraints.push((class, var));
            if self.peek().tok == Tok::RParen {
                self.bump();
                break;
            }
            // comma-separated constraints inside parens: `(Monad m, Show a)`
            if self.peek().tok == Tok::Comma {
                self.bump();
                continue;
            }
            return Err(self.error("expected ')' or ',' in instance constraint list"));
        }
        self.eat(&Tok::FatArrow)?;
    }
    let (class, _) = self.expect_ident()?;
    // Head: either a bare type (`List`) or a parenthesized partial application
    // (`(Kleisli m)`).
    let (ty, head_args) = if self.peek().tok == Tok::LParen {
        self.bump();
        let (h, _) = self.expect_ident()?;
        let mut args = Vec::new();
        while matches!(self.peek().tok, Tok::Ident(_)) {
            let (a, _) = self.expect_ident()?;
            args.push(a);
        }
        self.eat(&Tok::RParen)?;
        (h, args)
    } else {
        let (t, _) = self.expect_ident()?;
        (t, Vec::new())
    };
    self.eat(&Tok::KwWhere)?;
    self.eat(&Tok::LBrace)?;
    // … method_bodies loop unchanged …
    Ok(Def::Instance {
        class,
        ty,
        constraints,
        head_args,
        method_bodies,
        span: self.span_from(start),
    })
}
```

- [ ] **Step 3: Registration**

`ty.rs` `register_decls` (ty.rs:410) `Def::Instance` arm — store `constraints`/`head_args`:

```rust
Def::Instance { class, ty, constraints, head_args, method_bodies, .. } => {
    let mut methods: HashMap<String, (Vec<String>, Expr)> = HashMap::new();
    for (mname, params, body) in method_bodies {
        let bindings = params.iter().map(|p| p.name.clone()).collect();
        methods.insert(mname.clone(), (bindings, body.clone()));
    }
    self.instances.entry(class.clone()).or_default().insert(
        ty.clone(),
        InstanceInfo {
            class: class.clone(),
            ty: ty.clone(),
            constraints: constraints.clone(),
            head_args: head_args.clone(),
            methods,
        },
    );
}
```

`InstanceInfo` (ty.rs:215) gains:

```rust
/// Constraint list: (class, type var), e.g. `(Monad, "m")`.
pub constraints: Vec<(String, String)>,
/// Type-constructor args bound by the instance head (partial application):
/// `Kleisli m` → `["m"]` (arity 3 total, 1 bound ⇒ 2 remaining).
pub head_args: Vec<String>,
```

Update `instance_from_template` (ty.rs:300) — the derived-instance builder constructs `InstanceInfo`; initialize `constraints: vec![]`, `head_args: vec![]`.

- [ ] **Step 4: `class_var_arity_loose`**

`ty.rs` — the kind check for a partial-application head needs "total arity − bound args". Add:

```rust
/// Arity remaining after the instance head's bound args: `Kleisli m` (total 3,
/// bound 1) → 2. For a non-partial head, `total`.
pub(crate) fn instance_head_arity(&self, ty: &str, head_args: &[String]) -> Option<usize> {
    let total = self.ctor_value_arity(ty)?;
    Some(total.saturating_sub(head_args.len()))
}
```

- [ ] **Step 5: Run + commit**

Add a parse-only test:

```rust
#[test]
fn parses_constraint_instance_head() {
    let src = r#"
        typeclass Arrow a where { arr: (b -> c) -> a b c; }
        instance (Monad m) => Arrow (Kleisli m) where { arr f = f; }
        main = 1.0;
    "#;
    // `Monad` is a prelude class; the instance may not resolve yet (Task 6) —
    // assert only that it PARSES and REGISTERS.
    let res = compile::<f32>(src);
    assert!(res.is_ok() || matches!(res.err(), Some(rill_lang::CompileError::Type { .. })));
}
```

Run: `cargo test -p rill-lang --test arrow`.
Commit: `git add -A && git commit -m 'feat(rill-lang): constraint-qualified instance syntax + AST + registration'`

---

## Task 6: Constraint-instance resolution (infer + lower) + default methods

**Problem:** method calls (`arr f`) must resolve `instance Monad m => Arrow (Kleisli m)`: bind the head args from the call-site type (`Kleisli Maybe Float Float` → `m := Maybe`), then discharge `Monad m` by instance lookup. And `second`/`both`/`fan` must fall back to class defaults when the instance omits them.

**Files:** `types/ty.rs`, `types/infer.rs`, `lower.rs`

- [ ] **Step 1: Default-method plumbing (ty.rs)**

`TypeclassInfo::methods` (ty.rs:209) stays `Vec<(String, TypeExpr)>`. Add:

```rust
/// Default method bodies: method name → (params, body). An instance that omits
/// a method uses its class default (precedence: instance body > default > error).
pub defaults: HashMap<String, (Vec<String>, Expr)>,
```

`Def::Typeclass` — the parser (Task 7) fills defaults. For this task, `register_decls` `Def::Typeclass` arm reads `defaults` from the AST if present (parser added in Task 7; for now initialize empty).

Add `TypeEnv::resolve_method` fallback: when no instance method exists, try `self.typeclasses[class].defaults.get(method)`.

- [ ] **Step 2: `match_ctor_pattern` partial-application head (ty.rs)**

`match_ctor_pattern` (ty.rs:689) currently requires `p_args.len() == c_args.len()`. For a constraint instance with `head_args`, the call-site concrete type is full (`Kleisli Maybe Float Float`, 3 args) while the pattern is `a b c` (2 args). Add: when the instance has `head_args.len() == 0`, current behavior; the resolution path (Task 6 Step 3/4) binds head args from the concrete's *leading* args *before* matching the remaining args against the pattern.

- [ ] **Step 3: Inference resolution**

In `infer_apply_impl` (infer.rs:2578) constructor-class branch, after `match_ctor_pattern` returns the ctor, add constraint-instance handling. Replace the direct `resolve_method(name, ctor)` with:

```rust
// Constraint instance: `instance Monad m => Arrow (Kleisli m)`.
// Bind head args from the concrete container's LEADING args, then discharge
// each constraint by instance lookup of the bound type.
let inst = ctx.env.instances.get(&class_name)
    .and_then(|by_ty| by_ty.get(&ctor)).cloned();
let (head_args, constraints) = match &inst {
    Some(i) => (i.head_args.clone(), i.constraints.clone()),
    None => (vec![], vec![]),
};
let concrete_args = match &container_vty {
    ValueTy::App(_, a) | ValueTy::Data(_, a) => a.clone(),
    _ => vec![],
};
// Bind head args: `Kleisli Maybe Float Float` with head `Kleisli m` →
// m := Maybe (the first `head_args.len()` concrete args).
for (k, hv) in head_args.iter().enumerate() {
    if let Some(ca) = concrete_args.get(k) {
        let pty = ValueTy::TyConVar(ctx.fresh());
        unify_value(&pty, ca, &mut ctx.subst, span)?;
        ctx.type_var_bindings.insert(hv.clone(), ca.clone());
    } else {
        return Err(CompileError::Type {
            msg: format!("head argument `{hv}` of `{ctor}` is not concrete at the call site"),
            span,
        });
    }
}
// Discharge constraints: `Monad m` → instance of `Monad` for the bound type.
for (cclass, cv) in &constraints {
    let bound = ctx.type_var_bindings.get(cv).cloned().ok_or_else(|| CompileError::Type {
        msg: format!("constraint `{cclass} {cv}` has an unbound type variable"),
        span,
    })?;
    let tname = ctx.env.type_name_of_vty(&bound).ok_or_else(|| CompileError::Type {
        msg: format!("constraint `{cclass} {cv}`: the bound type is not concrete"),
        span,
    })?;
    if !ctx.env.instances.get(cclass.as_str()).map(|m| m.contains_key(tname.as_str())).unwrap_or(false) {
        return Err(CompileError::Type {
            msg: format!("no instance of `{cclass}` for type `{tname}` (constraint of `{class_name}`)"),
            span,
        });
    }
}
```

(`ctx.type_var_bindings: HashMap<String, ValueTy>` records head-arg → concrete type for use when typing the instance body; add the field to `Ctx`.)

The container pattern matching then uses the *remaining* concrete args (skip `head_args.len()` leading ones). Adjust `class_var_pattern`/`match_ctor_pattern` usage: build the pattern from the class-var signature (`a b c` → 2 args), match against `concrete_args[head_args.len()..]`.

- [ ] **Step 4: Lowering resolution**

Mirror in `lower.rs` constructor-class branch (lower.rs:907): compute `head_args`/`constraints` from the resolved `InstanceInfo`, bind head args from the concrete container's leading args (record into a `self.head_arg_bindings: HashMap<String, ValueTy>`), discharge constraints (env instance lookup), then lower the remaining args and the body with `return`/`bind` resolving against the bound monad (the existing `signature_param_tys_conv` + expected-type threading from SP-1 does this once `m` maps to the concrete monad).

- [ ] **Step 5: Default-method fallback in infer/lower**

In both resolution paths, after `resolve_method` returns `None`, check the class default:

```rust
let (_, params, body) = match ctx.env.resolve_method(name, ctor.as_str()) {
    Some(r) => r,
    None => match ctx.env.class_default(&class_name, name) {
        Some((params, body)) => (params, body),
        None => return Err(CompileError::Type { /* no instance + no default */ }),
    },
};
```

Add `TypeEnv::class_default(class, method) -> Option<(Vec<String>, Expr)>`.

- [ ] **Step 6: Test**

```rust
#[test]
fn constraint_instance_arr_over_kleisli() {
    let src = r#"
        typeclass Arrow a where {
            arr: (b -> c) -> a b c;
            first: a b c -> a (Pair b d) (Pair c d);
            compose: a b c -> a c d -> a b d;
        }
        data Kleisli m a b = { unKleisli: a -> m b };
        instance (Monad m) => Arrow (Kleisli m) where {
            arr f = Kleisli { unKleisli: fn x -> return (f x) };
        }
        main = match (unKleisli2 (arr (fn x -> x * 10.0))) of { Just v => v; Nothing => 0.0; };
    "#;
    // NOTE: needs `unKleisli2` (projection+apply, Task 4) and Pair ctor (Task 2).
    // Simplest E2E: apply `arr`'s result via a helper.
}
```

Prefer a minimal E2E once Task 4/2 are in: define a top-level helper `apply k x = let u = k.unKleisli in u x;` and assert `apply (arr (fn x -> x * 2.0)) 3.0 == 6.0` for a `Maybe`-Kleisli. (The real Kleisli/Arrow live in the prelude in Task 8; this task validates the machinery with user-declared equivalents.)

Run: `cargo test -p rill-lang --test arrow`.
Commit: `git add -A && git commit -m 'feat(rill-lang): resolve constraint instances + default methods'`

---

## Task 7: Single-field record constructed newtype-style

**Problem:** the prelude instance writes `arr f = Kleisli (fn x -> …)` — a record constructor applied to the field VALUE, not a `{ … }` literal. Today `infer`/`lower` reject that (`"record constructor expects a record literal"`).

**Files:** `types/infer.rs`, `lower.rs`

- [ ] **Step 1: Test**

```rust
#[test]
fn single_field_record_newtype_style() {
    let src = r#"
        data Box = { f: Float -> Float };
        b = Box (fn x -> x * 2.0);
        main = 1.0;
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
}
```

- [ ] **Step 2: Inference**

`infer_apply_impl` record-constructor arm (infer.rs:2205): when the record has exactly **one** field and the argument is NOT a `{ … }` record literal, treat it as the single field's value:

```rust
// Single-field record constructed newtype-style: `Box (fn x -> …)` ≡
// `Box { f: fn x -> … }`. Only valid for exactly one field.
if fields.len() == 1 && !matches!(&args[0], Expr::Record(..)) {
    let (fname, fty) = &fields[0];
    let vt = infer_const_value(ctx, &args[0])?;
    unify_value(&vt, fty, &mut ctx.subst, args[0].span())?;
    // Reuse the slot-collection below with a synthesized single-field literal.
    let lit = Expr::Record(vec![(fname.clone(), args[0].clone())], args[0].span());
    return /* same as the record-literal path, lowering `lit` */;
}
```

To avoid duplicating the slot/type-arg collection, restructure: build `fields_expr = vec![(fname, value)]` and fall through to the existing record path.

- [ ] **Step 3: Lowering**

`lower.rs` record-constructor arm (lower.rs:606): same branch — single field, non-literal arg → `field_regs = vec![lower_value(&args[0])]`, `ValueConstructRecord`. (The `ValueTy::Data(name, [])` return stays; type args resolve through the record path.)

- [ ] **Step 4: Run + commit**

Run: `cargo test -p rill-lang --test arrow`.
Commit: `git add -A && git commit -m 'feat(rill-lang): single-field record constructor newtype-style'`

---

## Task 8: Parser — typeclass default bodies + tuple/field-apply polish

**Files:** `parser.rs`, `ast.rs`, `types/ty.rs`

- [ ] **Step 1: Typeclass defaults (with params)**

`Def::Typeclass` gains `defaults: Vec<(String, Vec<Param>, Expr)>`. `parse_typeclass_def` (parser.rs:532): after `mname : sig`, if the next token is `Tok::Eq`, parse a default body **with optional params**:

```rust
while self.peek().tok != Tok::RBrace {
    let (mname, _) = self.expect_ident()?;
    self.eat(&Tok::Colon)?;
    let sig = self.parse_type_expr()?;
    let mut default = None;
    if self.peek().tok == Tok::Eq {
        self.bump();
        let mut dparams = Vec::new();
        while matches!(self.peek().tok, Tok::Ident(_)) {
            let (p, ps) = self.expect_ident()?;
            dparams.push(Param { name: p, span: ps });
        }
        let body = self.parse_expr(0, true)?;
        default = Some((dparams, body));
    }
    methods.push((mname, sig));
    if let Some((p, b)) = default {
        defaults.push((mname, p, b));
    }
    self.eat(&Tok::Semi)?;
}
```

`register_decls` `Def::Typeclass` arm: populate `TypeclassInfo.defaults` (method → `(params, body)`) from the AST.

- [ ] **Step 2: Tuple-type + field-apply regression pass**

Re-run Task 3/Task 4 tests; fix any parser edge cases (nested parens `((b, d))`, `(k.f) x` inside larger expressions).

- [ ] **Step 3: Run + commit**

Run: `cargo test -p rill-lang --test arrow` + full `cargo test -p rill-lang`.
Commit: `git add -A && git commit -m 'feat(rill-lang): typeclass default bodies with params'`

---

## Task 9: `CATEGORY_PRELUDE` — Kleisli + Arrow + instance

**Files:** `types/ty.rs`

- [ ] **Step 1: Extend the prelude**

Append to `CATEGORY_PRELUDE` (ty.rs:263):

```rill
data Kleisli m a b = { unKleisli: a -> m b };

typeclass Arrow a where {
    arr: (b -> c) -> a b c;
    first: a b c -> a (Pair b d) (Pair c d);
    compose: a b c -> a c d -> a b d;
    second: a b c -> a (Pair d b) (Pair d c) =
        compose (compose (arr (fn p -> Pair { first: p.second, second: p.first })) (first k)) (arr (fn p -> Pair { first: p.second, second: p.first }));
    both: a b c -> a d e -> a (Pair b d) (Pair c e) =
        compose (first f) (second g);
    fan: a b c -> a b d -> a b (Pair c d) =
        compose (arr (fn x -> Pair { first: x, second: x })) (both f g);
}

instance (Monad m) => Arrow (Kleisli m) where {
    arr f = Kleisli (fn x -> return (f x));
    first k = Kleisli (fn p -> bind (k.unKleisli p.first) (fn z -> return (Pair { first: z, second: p.second })));
    compose k1 k2 = Kleisli (fn x -> bind (k1.unKleisli x) (fn y -> k2.unKleisli y));
}
```

**Note on default bodies:** `second k`, `both f g`, `fan f g` use the classic definitions via `arr`/`first`/`compose`. The `first k` inside `second`/`both` resolves against the same instance type through the SP-1 method-lifting path (a different method name than the one being inlined — not recursion).

- [ ] **Step 2: Registration order**

`with_builtins` (ty.rs:329) currently parses the prelude, `register_decls`, then `derive_superclass_instances`. `Kleisli` is a `data`; `Arrow` a `typeclass`; the `instance` a constraint instance. Ensure `register_decls` handles `Def::Data`/`Def::Typeclass`/`Def::Instance` from the prelude the same as user code: **extend `register_decls` with a `Def::Data` arm** that registers `data_types`/`data_arities` via `data_field_vty` (mirroring infer.rs:767-785), plus `Def::TypeAlias`/`Def::Newtype`/`Def::Sum` arms as needed.

- [ ] **Step 3: Test — full Kleisli Arrow**

```rust
#[test]
fn kleisli_arrow_end_to_end() {
    let src = r#"
        apply k x = let u = k.unKleisli in u x;
        main = match (apply (compose (arr (fn x -> x + 1.0)) (arr (fn y -> y * 2.0))) 3.0) of {
            Just v => v; Nothing => 0.0;
        };
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 8.0);  // (3+1)*2
}
```

- [ ] **Step 4: Run + commit**

Run: `cargo test -p rill-lang --test arrow` + full `cargo test -p rill-lang` (prelude adds reserved names `arr`/`first`/`compose`/`second`/`both`/`fan`/`Kleisli` — ensure no existing test breaks).
Commit: `git add -A && git commit -m 'feat(rill-lang): Kleisli + Arrow + constraint instance in category prelude'`

---

## Task 10: Docs + final verification

**Files:** `rill-lang/README.md`, `docs/src/guides/rill-lang.md`, `CHANGELOG.md`

- [ ] **Step 1: Document**

README: extend the category-typeclasses section — `Arrow`/`Kleisli`, constraint instances (`instance (Monad m) => Arrow (Kleisli m)`), default methods, the `,`-as-tuple unification (`value,value` → `Pair`), tuple types `(b, d)`, field-projection application, and reserved names (`arr`, `first`, `compose`, `second`, `both`, `fan`, `Kleisli`). Guide + CHANGELOG: same additions.

- [ ] **Step 2: Full verification**

Run: `cargo test -p rill-lang && cargo test --workspace && cargo clippy --all-features --workspace && cargo fmt`
Expected: zero failures, zero warnings.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m 'docs(rill-lang): Arrow, Kleisli, constraint instances, tuple unification'
```

---

## Self-review

**Spec coverage (SP-2 + §1.4 machinery):** `TyConApp` HKT gap (T1); `,` channel-tuple unification (T2); tuple types (T3); field-projection application (T4); constraint-qualified instance syntax/AST/registration (T5); head-arg binding + constraint discharge + default methods (T6); single-field record newtype-style ctor (T7); typeclass defaults (T8); Kleisli/Arrow/instance in `CATEGORY_PRELUDE` (T9). Kind check for partial application (`instance_head_arity`) in T5; default precedence (instance > default > error) in T6.

**Placeholder scan:** Task 6's E2E test is intentionally a user-declared equivalent (prelude lands in T9) — the concrete assert is written in Task 9. Default bodies are param-taking (T8) with the final prelude text in T9.

**Type consistency:** `unKleisli: a -> m b` is `Func([Var(a)],[TyConApp(m,[Var(b)])])` everywhere (T1 data_field_vty, T6 signature_param_tys_conv, T9 prelude). `first` result `a (Pair b d) (Pair c d)` matches the prelude body's `Pair { first, second }` record (T2/T3/T9). `(b, d)` type sugar → `Pair b d` consistent with the Pair record's `first`/`second` fields. `Kleisli (fn x -> …)` newtype-style (T7) matches the prelude instance bodies (T9).

**Note on default-method params:** v1 typeclass methods resolve by inlining with params bound from call-site args (SP-1). Default bodies with params work through the same path (T6 Step 5 binds the default's params from the call). No runtime dispatch added.