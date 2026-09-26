# rill-lang HKT + First-Class Haskell-Style Collections — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add higher-kinded types (parameterized data types + kind polymorphism over type constructors) and six first-class Haskell-style collections (`List`, `Map`, `Set`, `Maybe`, `Pair`, `Either`) plus `Bool`/`String` value types to rill-lang, with `Eq`/`Ord`-constrained Map/Set keys, strict type-carried capacities, and a runtime capacity-overflow error.

**Architecture:** The type system (`types/ty.rs`, `types/unify.rs`, `types/infer.rs`) gains parameterized `ValueTy` (`App`, `Cap`, `TyConVar`), builtin constructor tables with kinds, and `Eq`/`Ord` typeclasses with compiler-derived instances. The arena (`arena.rs`) gains `Bool`/`String`/`List`/`Map`/`Set` values with RC/COW. Collection operations are one `ValueCallBuiltin` IR instruction dispatched in the interpreter (`backend/interp.rs`) using a structural `value_cmp`. Method resolution over constructors stays compile-time inline. All work is value-track (per-tick); the signal track (`Scalar`) is untouched.

**Tech Stack:** Rust, `rill-core` (traits, `ProcessError`), the existing rill-lang front-end (lexer → parser → infer → reduce → lower → interp). No new external dependencies.

**Spec:** `docs/superpowers/specs/2026-09-26-rill-lang-hkt-collections-design.md` (approved). Read it first.

---

## Conventions for all tasks

- Work on branch `feature/rill-lang-hkt` (already checked out). Never commit to `develop`/`master`.
- Verify per task: `cargo test -p rill-lang`, then `cargo fmt -p rill-lang` and `cargo clippy -p rill-lang --all-features` — zero warnings.
- Commit messages: single-quoted `git commit -m '...'` (double quotes break on backticks). Conventional commits: `feat(rill-lang): ...`.
- `#![deny(unsafe_code)]` is set — no `unsafe`.
- Existing test style: integration tests in `rill-lang/tests/*.rs` drive the whole pipeline via `rill_lang::compile::<f32>` and `rill_core::traits::MultichannelAlgorithm::process`.
- When a task references `compile::<f32>`, the input/output buffers are `[0.0f32; 4]`.

---

# Phase 1 — `TypeExpr` AST + declaration parser

The goal of this phase: the AST and parser accept parameterized type declarations (`data Box a = ...`) and full type-expression method signatures (`fmap: (a -> b) -> f a -> f b`). **No behavior change** — the new fields are stored but ignored by inference yet (phases 2–3 wire them up).

### Task 1.1: Add `TypeExpr` and extend `Def`

**Files:**
- Modify: `rill-lang/src/ast.rs`

- [ ] **Step 1: Write the failing unit test**

Add to `rill-lang/src/ast.rs`:

```rust
#[cfg(test)]
mod type_expr_tests {
    use super::*;

    #[test]
    fn type_expr_variants_construct() {
        let t = TypeExpr::TFunc(
            vec![TypeExpr::TName("a".into())],
            Box::new(TypeExpr::TApp("List".into(), vec![TypeExpr::TName("a".into()), TypeExpr::TCap(16)])),
        );
        assert!(matches!(t, TypeExpr::TFunc(..)));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang type_expr_variants_construct`
Expected: FAIL — `TypeExpr` not found.

- [ ] **Step 3: Add the `TypeExpr` enum and extend `Def`**

Add after `type TypeName = String;` in `ast.rs`:

```rust
/// A type expression in a declaration: concrete names, type variables,
/// constructor application, function types, and capacity literals.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TypeExpr {
    /// A concrete type or type variable name (`Float`, `a`).
    TName(String),
    /// Constructor application: `f a`, `List Float 16`.
    TApp(String, Vec<TypeExpr>),
    /// Curried function type: `(a -> b) -> f a -> f b`.
    TFunc(Vec<TypeExpr>, Box<TypeExpr>),
    /// Capacity literal (a `Nat` argument): `16` in `List Float 16`.
    TCap(usize),
}
```

Change `Def`:

```rust
    /// `data Name tv1 tv2 = { f1: T1, f2: T2 }` — product type.
    Data {
        name: String,
        /// Type parameters (e.g. `a` in `data Box a`).
        tyvars: Vec<String>,
        /// Fields: (field name, type expression).
        fields: Vec<(String, TypeExpr)>,
        span: Span,
    },
    /// `data Name tv = C1 T1 | C2 T2 T3` — sum type with constructors.
    Sum {
        name: String,
        tyvars: Vec<String>,
        ctors: Vec<(String, Vec<TypeExpr>)>,
        span: Span,
    },
```

Change `Def::Typeclass`:

```rust
    Typeclass {
        name: String,
        var: String,
        /// Methods: (method name, signature type expression).
        methods: Vec<(String, TypeExpr)>,
        span: Span,
    },
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-lang type_expr_variants_construct`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/ast.rs
git commit -m 'feat(rill-lang): TypeExpr AST for parameterized type declarations'
```

### Task 1.2: Parser — type parameters and field types

**Files:**
- Modify: `rill-lang/src/parser.rs`

- [ ] **Step 1: Write the failing parser test**

Add to the `mod tests` block in `parser.rs`:

```rust
#[test]
fn parses_parameterized_data_and_typeclass() {
    let p = prog("data Box a = { value: a }; typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }; main = _");
    match &p.defs[0] {
        Def::Data { name, tyvars, fields, .. } => {
            assert_eq!(name, "Box");
            assert_eq!(tyvars, &vec!["a".to_string()]);
            assert_eq!(fields[0], ("value".to_string(), TypeExpr::TName("a".into())));
        }
        other => panic!("expected Data, got {other:?}"),
    }
    match &p.defs[1] {
        Def::Typeclass { name, var, methods, .. } => {
            assert_eq!(name, "Functor");
            assert_eq!(var, "f");
            assert_eq!(methods.len(), 1);
            assert_eq!(methods[0].0, "fmap");
        }
        other => panic!("expected Typeclass, got {other:?}"),
    }
}

#[test]
fn parses_capacity_in_type_application() {
    let p = prog("data V = { xs: List Float 16 }; main = _");
    match &p.defs[0] {
        Def::Data { fields, .. } => {
            assert_eq!(
                fields[0].1,
                TypeExpr::TApp(
                    "List".into(),
                    vec![TypeExpr::TName("Float".into()), TypeExpr::TCap(16)]
                )
            );
        }
        other => panic!("expected Data, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rill-lang parses_parameterized_data_and_typeclass`
Expected: FAIL — `Def::Data` pattern no longer matches (`tyvars`/`fields` shape changed), or parse error.

- [ ] **Step 3: Add `parse_type_expr` and update declaration parsers**

Add a method to `Parser` (place it after `parse_data_def`):

```rust
    /// Parse a type expression: a chain of juxta-applied type names and type
    /// variables, `(T -> U -> V)` curried function types, and capacity ints.
    fn parse_type_expr(&mut self) -> Result<TypeExpr, CompileError> {
        let t = self.peek().clone();
        match t.tok {
            Tok::Int(n) => {
                self.bump();
                Ok(TypeExpr::TCap(n as usize))
            }
            Tok::LParen => {
                self.bump();
                let mut args = Vec::new();
                loop {
                    let arg = self.parse_type_expr()?;
                    if self.peek().tok == Tok::FatArrow {
                        args.push(arg);
                        self.bump();
                    } else {
                        args.push(arg);
                        break;
                    }
                }
                self.eat(&Tok::RParen)?;
                let ret = args.pop().expect("function type needs a result");
                Ok(TypeExpr::TFunc(args, Box::new(ret)))
            }
            Tok::Ident(name) => {
                self.bump();
                let mut args = Vec::new();
                while matches!(self.peek().tok, Tok::Ident(_) | Tok::Int(_)) {
                    args.push(self.parse_type_expr()?);
                }
                if args.is_empty() {
                    Ok(TypeExpr::TName(name))
                } else {
                    Ok(TypeExpr::TApp(name, args))
                }
            }
            other => Err(self.error(format!("expected type expression, found {other:?}"))),
        }
    }
```

Update `parse_data_def` — parse tyvars after the name, and type expressions for fields/payloads:

```rust
    fn parse_data_def(&mut self) -> Result<Def, CompileError> {
        let start = self.bump().span.start;
        let (name, _) = self.expect_ident()?;
        let mut tyvars = Vec::new();
        while matches!(self.peek().tok, Tok::Ident(_)) {
            let (tv, _) = self.expect_ident()?;
            tyvars.push(tv);
        }
        self.eat(&Tok::Eq)?;
        if self.peek().tok == Tok::LBrace {
            self.bump();
            let mut fields = Vec::new();
            while self.peek().tok != Tok::RBrace {
                let (fname, _) = self.expect_ident()?;
                self.eat(&Tok::Colon)?;
                let t = self.parse_type_expr()?;
                fields.push((fname, t));
                if self.peek().tok == Tok::Comma {
                    self.bump();
                }
            }
            self.eat(&Tok::RBrace)?;
            Ok(Def::Data { name, tyvars, fields, span: self.span_from(start) })
        } else {
            let mut ctors = Vec::new();
            while self.peek().tok != Tok::Semi && self.peek().tok != Tok::Eof {
                let (cname, _) = self.expect_ident()?;
                let mut payload = Vec::new();
                while matches!(self.peek().tok, Tok::Ident(_) | Tok::Int(_)) {
                    payload.push(self.parse_type_expr()?);
                }
                ctors.push((cname, payload));
                if self.peek().tok == Tok::Pipe {
                    self.bump();
                }
            }
            Ok(Def::Sum { name, tyvars, ctors, span: self.span_from(start) })
        }
    }
```

Update `parse_typeclass_def` — signatures become `parse_type_expr`:

```rust
    fn parse_typeclass_def(&mut self) -> Result<Def, CompileError> {
        let start = self.bump().span.start;
        let (name, _) = self.expect_ident()?;
        let (var, _) = self.expect_ident()?;
        self.eat(&Tok::KwWhere)?;
        self.eat(&Tok::LBrace)?;
        let mut methods = Vec::new();
        while self.peek().tok != Tok::RBrace {
            let (mname, _) = self.expect_ident()?;
            self.eat(&Tok::Colon)?;
            let sig = self.parse_type_expr()?;
            methods.push((mname, sig));
            self.eat(&Tok::Semi)?;
        }
        self.eat(&Tok::RBrace)?;
        Ok(Def::Typeclass { name, var, methods, span: self.span_from(start) })
    }
```

Add `use crate::ast::TypeExpr;` to the parser imports.

- [ ] **Step 4: Fix the rest of the compiler for the new `Def` shapes**

The `Def` variants changed shape; `data`/`sum` field types are now `TypeExpr`. Update every consumer to keep the workspace compiling (constructors with `tyvars: vec![]` / `fields: vec![]` where the value is unused):
- `src/render.rs` lines ~52–82 (declaration printing): add `tyvars`, render `TypeExpr` via a small `fmt_type_expr` helper.
- `src/reduce.rs` (`reduce` skips declarations — only `is_decl()` is affected; it already matches on the variants, so it still compiles).
- `src/types/infer.rs` (see Task 2.3): temporarily convert `TypeExpr` fields via `vty_of_name`-style fallback; this phase stores them but inference still reads **only the names** for the monomorphic (non-parameterized) case. Add a helper `type_expr_head(&TypeExpr) -> String` that returns the constructor/name for `TName`/`TApp` and panics (debug) / returns "Float" (release) for the rest — used by existing field-type resolution until phase 3.
- `src/types/ty.rs` `DataInfo`: keep as-is this phase; adapt at the `Def → TypeEnv` construction site in `infer.rs` (`build_type_env`) to read field heads.

The exact `Def → TypeEnv` construction lives in `infer.rs` (`build_type_env`). This phase, change it to store `DataInfo::Record(fields)` from `fields.iter().map(|(n, t)| (n.clone(), vty_of_type_expr_head(t)))` where `vty_of_type_expr_head` maps `TName(n) => env.vty_of_name(n)`, `TApp(n, _) => Data(n)`, else `Float`.

- [ ] **Step 5: Run all parser tests**

Run: `cargo test -p rill-lang parses_`
Expected: PASS (new + existing).

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/parser.rs rill-lang/src/render.rs rill-lang/src/types/infer.rs
git commit -m 'feat(rill-lang): parse parameterized type declarations and TypeExpr signatures'
```

---

# Phase 2 — `ValueTy` extension

### Task 2.1: Extend `ValueTy` with `Bool`, `String`, `App`, `Cap`, `TyConVar`

**Files:**
- Modify: `rill-lang/src/types/ty.rs`

- [ ] **Step 1: Write the failing unit test**

Add to `ty.rs`:

```rust
#[cfg(test)]
mod hkt_value_ty_tests {
    use super::*;

    #[test]
    fn app_and_cap_construct() {
        let t = ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(16)]);
        assert!(matches!(t, ValueTy::App(..)));
    }

    #[test]
    fn bool_string_are_leaves() {
        assert_ne!(ValueTy::Bool, ValueTy::Float);
        assert_ne!(ValueTy::String, ValueTy::Bool);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang app_and_cap_construct`
Expected: FAIL — `ValueTy::App`/`Cap`/`Bool`/`String` not found.

- [ ] **Step 3: Extend the enum**

In `types/ty.rs`, change `ValueTy`:

```rust
pub enum ValueTy {
    Int,
    Float,
    /// Boolean value type (value track only).
    Bool,
    /// String value type (value track only).
    String,
    Data(String, Vec<ValueTy>),
    Newtype(String, Vec<ValueTy>),
    /// Builtin constructor application: `List Float 16`.
    App(String, Vec<ValueTy>),
    /// Capacity literal (`Nat` argument).
    Cap(usize),
    Func(Vec<ValueTy>, Vec<ValueTy>),
    Var(TypeVarId),
    /// Type-constructor variable (kind `* -> *` or higher), bound by a
    /// typeclass class variable.
    TyConVar(TypeVarId),
}
```

- [ ] **Step 4: Fix the compile errors this introduces**

Add `Bool | String | App(..) | Cap(_) | TyConVar(_)` arms to the exhaustive matches over `ValueTy`:
- `Subst::resolve_value_depth` (`ty.rs:416`): recurse into `Data(_, args)`/`Newtype(_, args)`/`App(_, args)`; leaves otherwise.
- `TypeEnv::type_name_of_vty` (`ty.rs:275`): return `None` for the new non-named forms.
- `unify.rs` `value_contains_var_impl` and `unify_value` (Task 2.2).

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p rill-lang`
Expected: PASS (all tests; new variants compile).

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/types/ty.rs
git commit -m 'feat(rill-lang): extend ValueTy with Bool/String/App/Cap/TyConVar'
```

### Task 2.2: Unification for the new `ValueTy` variants

**Files:**
- Modify: `rill-lang/src/types/unify.rs`

- [ ] **Step 1: Write the failing unit test**

Add to `unify.rs` tests:

```rust
#[test]
fn unifies_app_with_matching_ctor() {
    let mut s = Subst::default();
    let a = ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(4)]);
    let b = ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(4)]);
    unify_value(&a, &b, &mut s, sp()).unwrap();
}

#[test]
fn unifies_app_with_cap_var_binding() {
    let mut s = Subst::default();
    let a = ValueTy::App("List".into(), vec![ValueTy::Var(1), ValueTy::Var(2)]);
    let b = ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(4)]);
    unify_value(&a, &b, &mut s, sp()).unwrap();
    assert_eq!(s.resolve_value(&ValueTy::Var(1)), ValueTy::Float);
    assert_eq!(s.resolve_value(&ValueTy::Var(2)), ValueTy::Cap(4));
}

#[test]
fn unifies_tyconvar_with_ctor_head() {
    let mut s = Subst::default();
    let pat = ValueTy::TyConVar(10);
    let ctor = ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(4)]);
    unify_value(&pat, &ctor, &mut s, sp()).unwrap();
    assert_eq!(
        s.resolve_value(&ValueTy::TyConVar(10)),
        ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(4)])
    );
}

#[test]
fn bool_string_unify_are_leaves() {
    let mut s = Subst::default();
    unify_value(&ValueTy::Bool, &ValueTy::Bool, &mut s, sp()).unwrap();
    unify_value(&ValueTy::String, &ValueTy::String, &mut s, sp()).unwrap();
    assert!(unify_value(&ValueTy::Bool, &ValueTy::Float, &mut s, sp()).is_err());
}

#[test]
fn app_ctor_head_mismatch_errors() {
    let mut s = Subst::default();
    let a = ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(4)]);
    let b = ValueTy::App("Map".into(), vec![ValueTy::Float, ValueTy::Cap(4)]);
    assert!(unify_value(&a, &b, &mut s, sp()).is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rill-lang unifies_app_with_matching_ctor`
Expected: FAIL — non-exhaustive match / not implemented.

- [ ] **Step 3: Extend `value_contains_var_impl`**

In `value_contains_var_impl`, walk the new compound shapes:

```rust
        ValueTy::Data(_, args) | ValueTy::Newtype(_, args) | ValueTy::App(_, args) => args
            .iter()
            .any(|a| value_contains_var_impl(subst, a, v, seen)),
        ValueTy::Bool | ValueTy::String | ValueTy::Cap(_) => false,
```

(`TyConVar` reaching this function is resolved before the match; treat a bare `TyConVar` as a leaf false to keep the occurs-check conservative.)

- [ ] **Step 4: Extend `unify_value`**

Add these match arms before the final `_ => Err(...)` arm:

```rust
        (ValueTy::Bool, ValueTy::Bool) => Ok(()),
        (ValueTy::String, ValueTy::String) => Ok(()),
        (ValueTy::Cap(x), ValueTy::Cap(y)) if x == y => Ok(()),
        (ValueTy::App(cx, ax), ValueTy::App(cy, ay)) if cx == cy && ax.len() == ay.len() => {
            for (x, y) in ax.iter().zip(ay.iter()) {
                unify_value(x, y, subst, span)?;
            }
            Ok(())
        }
        // A class-var (kind variable) unifies with a concrete constructor
        // application head, checking nothing here beyond the binding — arity is
        // checked at instance-resolution time (Phase 8).
        (ValueTy::TyConVar(f), other @ ValueTy::App(..)) | (other @ ValueTy::App(..), ValueTy::TyConVar(f)) => {
            if value_contains_var(subst, other, *f) {
                return Err(CompileError::Type {
                    msg: format!("recursive kind: type-constructor variable {f} cannot unify with {other:?}"),
                    span,
                });
            }
            subst.value_map.insert(*f, other.clone());
            Ok(())
        }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rill-lang unifies_`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/types/unify.rs rill-lang/src/types/ty.rs
git commit -m 'feat(rill-lang): unify App/Cap/TyConVar with occurs check'
```

### Task 2.3: `subtree_size` for collection value types

**Files:**
- Modify: `rill-lang/src/lower.rs` (`subtree_size_impl`, ~line 1146)

- [ ] **Step 1: Write the failing unit test**

Add to `lower.rs` tests (create a `mod tests` if absent; drive via a helper that builds a `TypeEnv`):

```rust
#[cfg(test)]
mod subtree_size_tests {
    use super::*;

    #[test]
    fn collection_subtree_sizes_are_exact() {
        let lw = Lowerer::dummy_for_test(); // see Step 3
        assert_eq!(lw.subtree_size(&ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(16)])), 1 + 16);
        assert_eq!(
            lw.subtree_size(&ValueTy::App("Map".into(), vec![ValueTy::String, ValueTy::Float, ValueTy::Cap(4)])),
            1 + 4 * 2
        );
        assert_eq!(lw.subtree_size(&ValueTy::App("Set".into(), vec![ValueTy::Int, ValueTy::Cap(8)])), 1 + 8);
        assert_eq!(
            lw.subtree_size(&ValueTy::App("Maybe".into(), vec![ValueTy::Float])),
            2
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang collection_subtree_sizes_are_exact`
Expected: FAIL — the App arm returns 1 today.

- [ ] **Step 3: Add the arms to `subtree_size_impl`**

Add before the closing `match` fallback in `subtree_size_impl`:

```rust
            ValueTy::Bool | ValueTy::String | ValueTy::Cap(_) => 1,
            ValueTy::Data(name, args) | ValueTy::Newtype(name, args) | ValueTy::App(name, args) => {
                let base = match name.as_str() {
                    "Maybe" => 1 + args.first().map(|t| self.subtree_size_impl(t, visiting)).unwrap_or(1),
                    "Pair" => {
                        1 + args.iter().map(|t| self.subtree_size_impl(t, visiting)).sum::<usize>()
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
                        let cap = match &args[1] {
                            ValueTy::Cap(n) => *n,
                            _ => 0,
                        };
                        1 + cap * self.subtree_size_impl(elem, visiting)
                    }
                    "Map" => {
                        let k = &args[0];
                        let v = &args[1];
                        let cap = match &args[2] {
                            ValueTy::Cap(n) => *n,
                            _ => 0,
                        };
                        1 + cap * (self.subtree_size_impl(k, visiting) + self.subtree_size_impl(v, visiting))
                    }
                    // User data types with parameters: recurse over arguments.
                    _ => 1 + args.iter().map(|t| self.subtree_size_impl(t, visiting)).sum::<usize>(),
                };
                base
            }
```

`Lowerer::dummy_for_test` is a test-only constructor that builds a `Lowerer` with an empty `env` (add it inside `#[cfg(test)]`; the struct's fields are visible in-module).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-lang collection_subtree_sizes_are_exact`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/lower.rs
git commit -m 'feat(rill-lang): exact subtree_size for collection value types'
```

---

# Phase 3 — Builtin constructors, kinds, `Eq`/`Ord` derived instances

### Task 3.1: Builtin type-constructor table in `TypeEnv`

**Files:**
- Modify: `rill-lang/src/types/ty.rs`

- [ ] **Step 1: Write the failing unit test**

Add to `ty.rs`:

```rust
#[cfg(test)]
mod ctor_table_tests {
    use super::*;

    #[test]
    fn builtin_ctor_kinds_and_capacity_flags() {
        let env = TypeEnv::with_builtins();
        assert!(env.ctor_arity("List") == Some(2));   // elem + cap
        assert!(env.ctor_has_cap("List") == Some(true));
        assert!(env.ctor_arity("Maybe") == Some(1));
        assert!(env.ctor_has_cap("Maybe") == Some(false));
        assert!(env.ctor_arity("Map") == Some(3));
        assert!(env.ctor_has_cap("Map") == Some(true));
        assert!(env.ctor_arity("Pair") == Some(2));
        assert!(env.ctor_arity("Either") == Some(2));
        assert!(env.ctor_arity("Nope") == None);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang builtin_ctor_kinds_and_capacity_flags`
Expected: FAIL — `ctor_arity`/`ctor_has_cap`/`with_builtins` not found.

- [ ] **Step 3: Add the constructor table**

Add to `TypeEnv`:

```rust
    /// Builtin type constructors: name → (value arity, whether the final
    /// argument is a capacity). `List a n` has arity 2, has-cap true.
    pub ctor_kinds: HashMap<String, (usize, bool)>,
```

In `TypeEnv`'s `Default` impl (it derives `Default` — add `ctor_kinds: HashMap::new()` via a custom `Default`), and add:

```rust
impl TypeEnv {
    /// A `TypeEnv` with the builtin constructor table and the builtin
    /// `Maybe`/`Pair`/`Either` type shapes registered.
    pub fn with_builtins() -> Self {
        let mut env = TypeEnv::default();
        env.ctor_kinds = [
            ("List".to_string(), (2usize, true)),
            ("Maybe".to_string(), (1usize, false)),
            ("Set".to_string(), (2usize, true)),
            ("Map".to_string(), (3usize, true)),
            ("Pair".to_string(), (2usize, false)),
            ("Either".to_string(), (2usize, false)),
        ]
        .into_iter()
        .collect();
        env.data_types.insert(
            "Maybe".to_string(),
            DataInfo::Sum(vec![
                ("Just".to_string(), vec![ValueTy::Var(1)]),
                ("Nothing".to_string(), vec![]),
            ]),
        );
        env.data_types.insert(
            "Pair".to_string(),
            DataInfo::Record(vec![
                ("first".to_string(), ValueTy::Var(1)),
                ("second".to_string(), ValueTy::Var(2)),
            ]),
        );
        env.data_types.insert(
            "Either".to_string(),
            DataInfo::Sum(vec![
                ("Left".to_string(), vec![ValueTy::Var(1)]),
                ("Right".to_string(), vec![ValueTy::Var(2)]),
            ]),
        );
        env
    }

    /// Value arity of a builtin constructor (`None` if not a builtin).
    pub fn ctor_arity(&self, name: &str) -> Option<usize> {
        self.ctor_kinds.get(name).map(|(a, _)| *a)
    }
    /// Whether the final argument of the constructor is a capacity.
    pub fn ctor_has_cap(&self, name: &str) -> Option<bool> {
        self.ctor_kinds.get(name).map(|(_, c)| *c)
    }
}
```

Change `TypeEnv` to `#[derive(Debug, Clone)]` and hand-write `Default` (or keep derive and add the field with `#[derive(Default)]` on the map — `HashMap` implements `Default`, so keeping `#[derive(Default)]` works as-is).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-lang builtin_ctor_kinds_and_capacity_flags`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/ty.rs
git commit -m 'feat(rill-lang): builtin constructor table and Maybe/Pair/Either shapes'
```

### Task 3.2: `Eq`/`Ord` typeclasses with derived instances

**Files:**
- Modify: `rill-lang/src/types/ty.rs`, `rill-lang/src/types/infer.rs`

- [ ] **Step 1: Write the failing unit test**

Add to `ty.rs`:

```rust
#[cfg(test)]
mod eq_ord_tests {
    use super::*;

    #[test]
    fn builtin_eq_ord_registered_and_derived() {
        let mut env = TypeEnv::with_builtins();
        env.derive_eq_ord();
        assert!(env.typeclasses.contains_key("Eq"));
        assert!(env.typeclasses.contains_key("Ord"));
        // Every concrete data type gets a derived Ord instance.
        let by_ty = &env.instances["Ord"];
        assert!(by_ty.contains_key("Float"));
        assert!(by_ty.contains_key("Int"));
        assert!(by_ty.contains_key("Bool"));
        assert!(by_ty.contains_key("String"));
        assert!(by_ty.contains_key("Point"));
        assert!(by_ty.contains_key("List"));
        assert!(!by_ty.contains_key("Func"), "Func has no derived Ord");
    }
}
```

Add a helper to construct a small env with `Point` and `List` data entries before calling `derive_eq_ord` (inline in the test: insert `env.data_types.insert("Point", DataInfo::Record(vec![("x".into(), ValueTy::Float)]))` and `env.data_types.insert("List", DataInfo::Sum(vec![("Cons".into(), vec![ValueTy::Var(1), ValueTy::Var(2)]), ("Nil".into(), vec![])]))`).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang builtin_eq_ord_registered_and_derived`
Expected: FAIL — `derive_eq_ord` not found.

- [ ] **Step 3: Register `Eq`/`Ord` and derive instances**

Add `typeclasses` entries in `with_builtins` (after the data shapes):

```rust
        env.typeclasses.insert(
            "Eq".to_string(),
            TypeclassInfo {
                var: "a".to_string(),
                arity: 0,
                methods: vec![("eq".to_string(), crate::ast::TypeExpr::TFunc(
                    vec![crate::ast::TypeExpr::TName("a".into())],
                    Box::new(crate::ast::TypeExpr::TName("Bool".into())),
                ))],
            },
        );
        env.typeclasses.insert(
            "Ord".to_string(),
            TypeclassInfo {
                var: "a".to_string(),
                arity: 0,
                methods: vec![("lt".to_string(), crate::ast::TypeExpr::TFunc(
                    vec![crate::ast::TypeExpr::TName("a".into())],
                    Box::new(crate::ast::TypeExpr::TName("Bool".into())),
                ))],
            },
        );
```

Add the derivation method on `TypeEnv`:

```rust
    /// Register a derived (structural) `Eq`/`Ord` instance for every concrete
    /// data type currently in the env, plus the scalar leaves. `Func` types
    /// get no instance. Derived instances are markers: their method bodies are
    /// not run — the interpreter's `value_cmp` implements the order.
    pub fn derive_eq_ord(&mut self) {
        let mut names: Vec<String> = self.data_types.keys().cloned().collect();
        names.extend(["Int", "Float", "Bool", "String"].iter().map(|s| s.to_string()));
        for class in ["Eq", "Ord"] {
            let by_ty = self.instances.entry(class.to_string()).or_default();
            for n in &names {
                by_ty.entry(n.clone()).or_insert_with(|| InstanceInfo {
                    class: class.to_string(),
                    ty: n.clone(),
                    methods: HashMap::new(), // derived marker — empty body
                });
            }
        }
    }
```

Call `derive_eq_ord()` at the end of `build_type_env` in `infer.rs` (where the env is assembled from the program's `Def`s). `TypeclassInfo` now has an `arity` field — update the `TypeclassInfo` struct definition (add `pub arity: usize` with the doc from the spec) and its construction site in `infer.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-lang builtin_eq_ord_registered_and_derived`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/ty.rs rill-lang/src/types/infer.rs
git commit -m 'feat(rill-lang): derived Eq/Ord instances over data types'
```

---

# Phase 4 — Arena values: `Bool`, `String`, `List`, `Map`, `Set`

### Task 4.1: New `Value` variants with RC/COW

**Files:**
- Modify: `rill-lang/src/arena.rs`

- [ ] **Step 1: Write the failing unit test**

Add to `arena.rs` tests:

```rust
#[test]
fn list_and_map_rc_cow() {
    let mut a = Arena::with_capacity(16);
    let e0 = a.alloc(Value::Float(1.0)).unwrap();
    let e1 = a.alloc(Value::Float(2.0)).unwrap();
    let l = a.alloc(Value::List { elems: vec![e0, e1], cap: 4 }).unwrap();
    // COW copies the list and recounts its elements.
    let l2 = a.copy(l).unwrap();
    let out = a.mutate(l).unwrap();
    assert_ne!(out, l2);
    assert_eq!(a.rc(e0), 2, "both list copies own the element");
    a.drop_ref(l);
    a.drop_ref(l2);
    assert_eq!(a.rc(e0), 0, "element freed when both lists dropped");
}

#[test]
fn drop_recurses_into_map_and_set() {
    let mut a = Arena::with_capacity(16);
    let k = a.alloc(Value::String("a".into())).unwrap();
    let v = a.alloc(Value::Float(1.0)).unwrap();
    let m = a.alloc(Value::Map { pairs: vec![(k, v)], cap: 4 }).unwrap();
    let s = a.alloc(Value::Set { elems: vec![k], cap: 4 }).unwrap();
    a.drop_ref(m);
    assert_eq!(a.rc(v), 0);
    a.drop_ref(s);
    assert_eq!(a.rc(k), 0);
}

#[test]
fn bool_string_are_leaf_values() {
    let mut a = Arena::with_capacity(4);
    let b = a.alloc(Value::Bool(true)).unwrap();
    let s = a.alloc(Value::String("hi".into())).unwrap();
    assert_eq!(a.get(b).unwrap(), &Value::Bool(true));
    assert_eq!(a.get(s).unwrap(), &Value::String("hi".into()));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rill-lang list_and_map_rc_cow`
Expected: FAIL — `Value::List`/`Map`/`Set`/`Bool`/`String` not found.

- [ ] **Step 3: Add the variants**

In `arena.rs`:

```rust
pub enum ValueKind {
    Int, Float, Bool, String, Record, Sum, Newtype, Closure, List, Map, Set, Void,
}
```

```rust
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Record(Vec<ArenaRef>),
    Sum(u32, Vec<ArenaRef>),
    Newtype(ArenaRef),
    Closure(ArenaRef, u32),
    /// A first-class list: element refs, current length = `elems.len()`, cap.
    List { elems: Vec<ArenaRef>, cap: usize },
    /// A first-class map: sorted (key, value) ref pairs.
    Map { pairs: Vec<(ArenaRef, ArenaRef)>, cap: usize },
    /// A first-class set: sorted element refs.
    Set { elems: Vec<ArenaRef>, cap: usize },
    Void,
}
```

Update `Value::kind()` to cover the new variants.

- [ ] **Step 4: Extend RC/COW helpers**

`drop_ref` — add to the recursion match:

```rust
            Value::List { elems, .. } => {
                for e in elems {
                    self.drop_ref(e);
                }
            }
            Value::Map { pairs, .. } => {
                for (k, v) in pairs {
                    self.drop_ref(k);
                    self.drop_ref(v);
                }
            }
            Value::Set { elems, .. } => {
                for e in elems {
                    self.drop_ref(e);
                }
            }
```

`mutate` — recount the children for the COW copy (mirror the `Record`/`Sum` arms):

```rust
            Value::List { elems, .. } => {
                for e in elems {
                    self.copy(*e)?;
                }
            }
            Value::Map { pairs, .. } => {
                for (k, v) in pairs {
                    self.copy(*k)?;
                    self.copy(*v)?;
                }
            }
            Value::Set { elems, .. } => {
                for e in elems {
                    self.copy(*e)?;
                }
            }
```

Also update `alloc_owned`/`alloc_copy`/`drop_value_children` in `backend/interp.rs` (they pattern-match on `Value` to recount children). Add the same child extraction for `List`/`Map`/`Set`. The interp helpers at `interp.rs:156-210` extract children as `Vec<ArenaRef>` — extend those match arms.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rill-lang`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/arena.rs rill-lang/src/backend/interp.rs
git commit -m 'feat(rill-lang): Bool/String/List/Map/Set arena values with RC/COW'
```

---

# Phase 5 — Parser: list/map/bool literals, comparisons, logic

### Task 5.1: Lexer tokens

**Files:**
- Modify: `rill-lang/src/lexer.rs`

- [ ] **Step 1: Write the failing test**

Add to `lexer.rs` tests:

```rust
#[test]
fn lexes_collection_and_comparison_tokens() {
    assert_eq!(
        kinds("[ ] == != < > <= >= && || true false"),
        vec![
            Tok::LBracket,
            Tok::RBracket,
            Tok::EqEq,
            Tok::NotEq,
            Tok::Lt,
            Tok::Gt,
            Tok::Le,
            Tok::Ge,
            Tok::AndAnd,
            Tok::OrOr,
            Tok::KwTrue,
            Tok::KwFalse,
            Tok::Eof,
        ]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang lexes_collection_and_comparison_tokens`
Expected: FAIL — tokens not found.

- [ ] **Step 3: Add the token variants**

Add to `Tok`:

```rust
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `==`
    EqEq,
    /// `!=`
    NotEq,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `&&`
    AndAnd,
    /// `||`
    OrOr,
    /// `true` keyword.
    KwTrue,
    /// `false` keyword.
    KwFalse,
```

In `tokenize`, add multi-char checks before the single-char match (after the `=>`/`->` branch):

```rust
        if c == b'=' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token { tok: Tok::EqEq, span: Span::new(start, i) });
            continue;
        }
        if c == b'!' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token { tok: Tok::NotEq, span: Span::new(start, i) });
            continue;
        }
        if c == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token { tok: Tok::Le, span: Span::new(start, i) });
            continue;
        }
        if c == b'>' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token { tok: Tok::Ge, span: Span::new(start, i) });
            continue;
        }
        if c == b'&' && i + 1 < bytes.len() && bytes[i + 1] == b'&' {
            i += 2;
            out.push(Token { tok: Tok::AndAnd, span: Span::new(start, i) });
            continue;
        }
        if c == b'|' && i + 1 < bytes.len() && bytes[i + 1] == b'|' {
            i += 2;
            out.push(Token { tok: Tok::OrOr, span: Span::new(start, i) });
            continue;
        }
```

Add `'[' => Tok::LBracket, ']' => Tok::RBracket,` and `'<' => Tok::Lt, '>' => Tok::Gt,` to the `single` match. IMPORTANT: `<:` (Split) and `:>` (Merge) are already handled earlier — the new `Lt`/`Gt` arms only fire for bare `<`/`>`. Add `"true"`/`"false"` to the keyword `match`:

```rust
                "true" if !followed_by_paren => Tok::KwTrue,
                "false" if !followed_by_paren => Tok::KwFalse,
```

- [ ] **Step 4: Fix exhaustiveness across the parser**

Every `match` on `Tok` is now non-exhaustive. Fix: `parser.rs` `is_atom_start` (add `Tok::LBracket | Tok::KwTrue | Tok::KwFalse`), `infix_binding_power` (add the comparison/logic ops in Task 5.2), and any other `Tok` matches in the parser.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p rill-lang lexes_collection_and_comparison_tokens`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/lexer.rs
git commit -m 'feat(rill-lang): lexer tokens for lists, comparisons, logic, bool literals'
```

### Task 5.2: AST expressions and parser for literals/comparisons/logic

**Files:**
- Modify: `rill-lang/src/ast.rs`, `rill-lang/src/parser.rs`

- [ ] **Step 1: Write the failing parser test**

Add to `parser.rs` tests:

```rust
#[test]
fn parses_list_map_bool_cmp_logic() {
    match body("main = [1.0, 2.0]") {
        Expr::ListLit(elems, _) => assert_eq!(elems.len(), 2),
        other => panic!("expected ListLit, got {other:?}"),
    }
    match body("main = { \"a\": 1.0 }") {
        Expr::MapLit(entries, _) => assert_eq!(entries.len(), 1),
        other => panic!("expected MapLit, got {other:?}"),
    }
    match body("main = true") {
        Expr::Bool(true, _) => {}
        other => panic!("expected Bool, got {other:?}"),
    }
    match body("main = 1.0 < 2.0 && 3.0 > 1.0") {
        Expr::Logic { .. } => {}
        other => panic!("expected Logic, got {other:?}"),
    }
}

#[test]
fn comparison_binds_tighter_than_logic() {
    match body("main = a < b || c > d") {
        Expr::Logic { lhs, rhs, .. } => {
            assert!(matches!(lhs.as_ref(), Expr::Cmp { .. }));
            assert!(matches!(rhs.as_ref(), Expr::Cmp { .. }));
        }
        other => panic!("expected Logic, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rill-lang parses_list_map_bool_cmp_logic`
Expected: FAIL — variants not found / parse errors.

- [ ] **Step 3: Add AST expressions**

Add to `ast.rs`:

```rust
/// A value-track comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CmpOp { Eq, Ne, Lt, Gt, Le, Ge }

/// A value-track boolean logic operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LogicOp { And, Or }
```

Add to `Expr`:

```rust
    /// Boolean literal `true` / `false`.
    Bool(bool, Span),
    /// List literal `[e1, e2]`.
    ListLit(Vec<Expr>, Span),
    /// Map literal `{ "k": v, ... }` (string keys).
    MapLit(Vec<(String, Expr)>, Span),
    /// Value-track comparison `a < b` (only valid in value position).
    Cmp { op: CmpOp, lhs: Box<Expr>, rhs: Box<Expr>, span: Span },
    /// Value-track logic `a && b` / `a || b`.
    Logic { op: LogicOp, lhs: Box<Expr>, rhs: Box<Expr>, span: Span },
```

Update `Expr::span()` for the new variants.

- [ ] **Step 4: Parser — precedence and atoms**

Add `Cmp`, `Logic` to `InfixOp` and binding powers (looser than arithmetic, tighter than nothing):

```rust
    Tok::AndAnd => (InfixOp::LogicAnd, 1, 2),
    Tok::OrOr => (InfixOp::LogicOr, 1, 2),
    Tok::EqEq => (InfixOp::CmpEq, 3, 4),
    Tok::NotEq => (InfixOp::CmpNe, 3, 4),
    Tok::Lt => (InfixOp::CmpLt, 3, 4),
    Tok::Gt => (InfixOp::CmpGt, 3, 4),
    Tok::Le => (InfixOp::CmpLe, 3, 4),
    Tok::Ge => (InfixOp::CmpGe, 3, 4),
```

In `parse_expr`, after the `is_arith` branch, add:

```rust
            } else if op.is_logic() {
                lhs = Expr::Logic {
                    op: op.to_logic(),
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span,
                };
            } else if op.is_cmp() {
                lhs = Expr::Cmp {
                    op: op.to_cmp(),
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span,
                };
```

Add the helper methods to `InfixOp` (`is_logic`, `to_logic`, `is_cmp`, `to_cmp`) mapping to the new variants.

Add atom parsing in `parse_atom` and `is_atom_start`:
- `Tok::KwTrue => Ok(Expr::Bool(true, t.span))`, `Tok::KwFalse => Ok(Expr::Bool(false, t.span))`.
- `Tok::LBracket`: parse `[` `expr (',' expr)*]` → `Expr::ListLit`. Add an `is_atom_start` arm for `Tok::LBracket`.
- `Tok::LBrace` currently rewinds to `parse_record`. Change `parse_record` to detect a string key: peek — if the token after `{` is `Tok::Str(..)`, parse a `MapLit` instead (keys are `Str` literals, values are expressions). Implement `parse_record_or_map`:

```rust
    fn parse_record_or_map(&mut self) -> Result<Expr, CompileError> {
        let start = self.eat(&Tok::LBrace)?.span.start;
        if self.peek().tok == Tok::RBrace {
            self.bump();
            return Ok(Expr::Record(Vec::new(), self.span_from(start)));
        }
        // A string literal key makes this a Map literal.
        let is_map = matches!(self.peek().tok, Tok::Str(_));
        let mut fields = Vec::new();
        let mut entries = Vec::new();
        loop {
            if is_map {
                let k = match self.bump().tok {
                    Tok::Str(s) => s,
                    other => return Err(self.error(format!("expected string map key, found {other:?}"))),
                };
                self.eat(&Tok::Colon)?;
                let v = self.parse_expr(0, true)?;
                entries.push((k, v));
            } else {
                let (key, _) = self.expect_ident()?;
                self.eat(&Tok::Colon)?;
                let val = self.parse_expr(0, true)?;
                fields.push((key, val));
            }
            if self.peek().tok == Tok::Comma {
                self.bump();
                if self.peek().tok == Tok::RBrace { break; }
            } else if self.peek().tok == Tok::RBrace {
                break;
            } else {
                return Err(self.error("expected ',' or '}' in literal"));
            }
        }
        self.eat(&Tok::RBrace)?;
        if is_map {
            Ok(Expr::MapLit(entries, self.span_from(start)))
        } else {
            Ok(Expr::Record(fields, self.span_from(start)))
        }
    }
```

Wire `Tok::LBrace` in `parse_atom` to `self.parse_record_or_map()`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rill-lang parses_list_map_bool_cmp_logic`
Expected: PASS.

- [ ] **Step 6: Fix the rest of the compiler for the new `Expr` variants**

`Expr::span()` updated in Step 3. Other exhaustive matches over `Expr` now need arms: `render.rs`, `reduce.rs` (`substitute`/`reduce` — for the new variants, `reduce` can treat them as opaque atoms; `substitute` must NOT descend — matches the spec's β-substitution gap), `infer.rs`, `lower.rs`. For this phase, add default/opaque arms returning errors or identity where the pipeline doesn't yet support them (infer/lower: `CompileError::Unsupported("list/map/bool/cmp expressions not yet supported")`). Phase 6/7 implement them.

- [ ] **Step 7: Commit**

```bash
git add rill-lang/src/ast.rs rill-lang/src/parser.rs rill-lang/src/render.rs rill-lang/src/reduce.rs rill-lang/src/types/infer.rs rill-lang/src/lower.rs
git commit -m 'feat(rill-lang): parse list/map/bool literals and comparisons/logic'
```

---

# Phase 6 — Value-track collection operations

### Task 6.1: IR instructions and `ValueBuiltinOp`

**Files:**
- Modify: `rill-lang/src/ir.rs`

- [ ] **Step 1: Write the failing unit test**

Add to `ir.rs`:

```rust
#[cfg(test)]
mod value_builtin_tests {
    use super::*;

    #[test]
    fn value_builtin_ops_exist() {
        let ops = [
            ValueBuiltinOp::Cons,
            ValueBuiltinOp::Head,
            ValueBuiltinOp::Tail,
            ValueBuiltinOp::Length,
            ValueBuiltinOp::Map,
            ValueBuiltinOp::Fold,
            ValueBuiltinOp::Filter,
            ValueBuiltinOp::ListEmpty,
            ValueBuiltinOp::InsertMap,
            ValueBuiltinOp::Lookup,
            ValueBuiltinOp::Member,
            ValueBuiltinOp::InsertSet,
            ValueBuiltinOp::MapEmpty,
            ValueBuiltinOp::SetEmpty,
        ];
        assert_eq!(ops.len(), 14);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang value_builtin_ops_exist`
Expected: FAIL — `ValueBuiltinOp` not found.

- [ ] **Step 3: Add the IR variants**

In `ir.rs`:

```rust
/// A value-track collection operation dispatched by the interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueBuiltinOp {
    Cons, Head, Tail, Length, Map, Fold, Filter, ListEmpty,
    InsertMap, Lookup, Member, InsertSet, MapEmpty, SetEmpty,
}

/// Value-track comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp { Eq, Ne, Lt, Gt, Le, Ge }

/// Value-track boolean logic operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicOp { And, Or }
```

Add to `ValueInstr`:

```rust
    /// Boolean literal.
    ValueBool { dst: usize, value: bool },
    /// String literal.
    ValueConstString { dst: usize, value: String },
    /// List literal: alloc the container + element refs.
    ValueListLit { dst: usize, elems: Vec<usize>, cap: usize },
    /// Map literal with string keys.
    ValueMapLit { dst: usize, keys: Vec<usize>, vals: Vec<usize>, cap: usize },
    /// Value-track comparison.
    ValueCompare { dst: usize, op: CmpOp, a: usize, b: usize },
    /// Value-track boolean logic.
    ValueLogic { dst: usize, op: LogicOp, a: usize, b: usize },
    /// Dispatch a collection operation.
    ValueCallBuiltin { dst: usize, op: ValueBuiltinOp, args: Vec<usize> },
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-lang value_builtin_ops_exist`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/ir.rs
git commit -m 'feat(rill-lang): IR instructions for collection ops, comparisons, literals'
```

### Task 6.2: Interpreter `value_cmp` and the builtin dispatcher

**Files:**
- Modify: `rill-lang/src/backend/interp.rs`

- [ ] **Step 1: Write the failing unit test**

Add a unit test in `interp.rs` (drives a full program through the pipeline):

```rust
#[test]
fn list_ops_flow_through_process() {
    let mut prog = crate::compile::<f32>(
        "main = length [1.0, 2.0, 3.0];",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &crate::arena::Value::Int(3));
}
```

This requires the *lowerer* to already lower `ListLit`+`length` (that is Task 6.3). If the lowerer is not yet wired, this test will fail at `compile()` — acceptable; implement 6.2 and 6.3 together for the first green test. Put this test in a new file `rill-lang/tests/collections_list.rs` instead (integration style), and keep this task focused on `value_cmp` unit tests:

```rust
#[test]
fn value_cmp_orders_structural() {
    use crate::arena::{Arena, Value};
    let mut a = Arena::with_capacity(8);
    let r1 = a.alloc(Value::Float(1.0)).unwrap();
    let r2 = a.alloc(Value::Float(2.0)).unwrap();
    assert!(value_cmp(&a, r1, r2) < 0);
    assert_eq!(value_cmp(&a, r1, r1), 0);
    assert!(value_cmp(&a, r2, r1) > 0);
    let s1 = a.alloc(Value::String("a".into())).unwrap();
    let s2 = a.alloc(Value::String("b".into())).unwrap();
    assert!(value_cmp(&a, s1, s2) < 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang value_cmp_orders_structural`
Expected: FAIL — `value_cmp` not found.

- [ ] **Step 3: Implement `value_cmp`**

Add a free function in `interp.rs`:

```rust
/// Structural total order over acyclic arena values (the derived `Eq`/`Ord`).
/// Returns `< 0`, `== 0`, or `> 0`. `Func` values are unordered — reaching one
/// is a lowering bug (the type checker rejects Ord over functions).
fn value_cmp<T: Transcendental, const BUF: usize>(
    prog: &RillProgram<T, BUF>,
    a: ArenaRef,
    b: ArenaRef,
) -> i8 {
    let va = prog.arena.get(a);
    let vb = prog.arena.get(b);
    match (va, vb) {
        (Some(va), Some(vb)) => value_cmp_ref(prog, va, vb),
        _ => 0,
    }
}

fn value_cmp_ref<T: Transcendental, const BUF: usize>(
    prog: &RillProgram<T, BUF>,
    a: &Value,
    b: &Value,
) -> i8 {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y)) => x.cmp(y) as i8,
        (Float(x), Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal) as i8,
        (Bool(x), Bool(y)) => x.cmp(y) as i8,
        (String(x), String(y)) => x.cmp(y) as i8,
        (Newtype(x), Newtype(y)) => value_cmp(prog, *x, *y),
        (Sum(i, px), Sum(j, py)) => {
            let c = i.cmp(j) as i8;
            if c != 0 { return c; }
            cmp_ref_slices(prog, px, py)
        }
        (Record(fx), Record(fy)) => cmp_ref_slices(prog, fx, fy),
        (List { elems: ex, .. }, List { elems: ey, .. }) => cmp_ref_slices(prog, ex, ey),
        (Set { elems: ex, .. }, Set { elems: ey, .. }) => cmp_ref_slices(prog, ex, ey),
        (Map { pairs: px, .. }, Map { pairs: py, .. }) => {
            for (i, (kx, vx)) in px.iter().enumerate() {
                let Some((ky, vy)) = py.get(i) else { return 1; };
                let c = value_cmp(prog, *kx, *ky);
                if c != 0 { return c; }
                let c = value_cmp(prog, *vx, *vy);
                if c != 0 { return c; }
            }
            (px.len() as i8).cmp(&(py.len() as i8)) as i8
        }
        (Closure(..), Closure(..)) => 0, // unreachable for Ord-typed keys
        _ => kind_rank(a).cmp(&kind_rank(b)) as i8,
    }
}

fn cmp_ref_slices<T: Transcendental, const BUF: usize>(
    prog: &RillProgram<T, BUF>,
    xs: &[ArenaRef],
    ys: &[ArenaRef],
) -> i8 {
    for (i, x) in xs.iter().enumerate() {
        let Some(y) = ys.get(i) else { return 1; };
        let c = value_cmp(prog, *x, *y);
        if c != 0 { return c; }
    }
    (xs.len() as i8).cmp(&(ys.len() as i8)) as i8
}

fn kind_rank(v: &Value) -> i8 {
    use Value::*;
    match v {
        Bool(_) => 0, Int(_) => 1, Float(_) => 2, String(_) => 3,
        Record(_) => 4, Sum(..) => 5, Newtype(_) => 6, List { .. } => 7,
        Map { .. } => 8, Set { .. } => 9, Closure(..) => 10, Void => 11,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-lang value_cmp_orders_structural`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/backend/interp.rs
git commit -m 'feat(rill-lang): structural value_cmp for derived Eq/Ord'
```

### Task 6.3: Lowering collection expressions + the `ValueCallBuiltin` dispatcher

**Files:**
- Modify: `rill-lang/src/lower.rs`, `rill-lang/src/backend/interp.rs`, `rill-lang/src/program.rs`

- [ ] **Step 1: Write the failing integration test**

Create `rill-lang/tests/collections_list.rs`:

```rust
//! End-to-end list-collection tests: literals, cons/head/tail/length/map/fold/filter.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn list_literal_length_and_head() {
    let mut prog = compile::<f32>("main = length [1.0, 2.0, 3.0];").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Int(3));
}

#[test]
fn cons_builds_list_and_head_is_maybe() {
    // `head (cons 1.0 (list 4))` -> Just 1.0; project `first` via match.
    let mut prog = compile::<f32>(
        "main = match (head (cons 1.0 (list 4))) of { Nothing => 0.0; Just x => x; };",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Float(1.0));
}

#[test]
fn map_fold_filter_over_list() {
    let mut prog = compile::<f32>(
        "main = fold (fn a b -> a + b) 0.0 (map (fn x -> x * 2.0) [1.0, 2.0, 3.0]);",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Float(12.0));
}

#[test]
fn cons_overflow_is_runtime_process_error() {
    // `[1.0, 2.0]` has cap 2, len 2; consing a third overflows.
    let mut prog = compile::<f32>("main = cons 3.0 [1.0, 2.0];").unwrap();
    let mut out = [0.0f32; 4];
    let res = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    assert!(res.is_err(), "cons past capacity must be a runtime error");
    let err = res.unwrap_err();
    assert!(
        format!("{err:?}").contains("capacity exceeded"),
        "expected capacity message, got {err:?}"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rill-lang --test collections_list`
Expected: FAIL — compile errors ("list expressions not yet supported").

- [ ] **Step 3: Lower the new value expressions**

In `lower.rs` `lower_value` (the value-expression lowering, `lower_value(&mut self, e: &Expr) -> Result<(usize, ValueTy), CompileError>`), add arms:

```rust
            Expr::Bool(b, _) => {
                let dst = self.fresh_value_reg();
                self.value_instrs.push(ValueInstr::ValueBool { dst, value: *b });
                Ok((dst, ValueTy::Bool))
            }
            Expr::Str(s, _) => {
                let dst = self.fresh_value_reg();
                self.value_instrs.push(ValueInstr::ValueConstString { dst, value: s.clone() });
                Ok((dst, ValueTy::String))
            }
            Expr::ListLit(elems, _) => {
                let mut regs = Vec::new();
                let mut elem_ty = None;
                for e in elems {
                    let (r, t) = self.lower_value(e)?;
                    regs.push(r);
                    elem_ty = Some(t);
                }
                let cap = regs.len();
                let dst = self.fresh_value_reg();
                self.value_instrs.push(ValueInstr::ValueListLit { dst, elems: regs, cap });
                Ok((dst, ValueTy::App("List".into(), vec![elem_ty.unwrap_or(ValueTy::Float), ValueTy::Cap(cap)])))
            }
            Expr::MapLit(entries, _) => {
                let mut keys = Vec::new();
                let mut vals = Vec::new();
                for (k, v) in entries {
                    let (kr, _) = self.lower_value(&Expr::Str(k.clone(), Span::new(0, 0)))?;
                    let (vr, _) = self.lower_value(v)?;
                    keys.push(kr);
                    vals.push(vr);
                }
                let cap = keys.len();
                let dst = self.fresh_value_reg();
                self.value_instrs.push(ValueInstr::ValueMapLit { dst, keys, vals, cap });
                Ok((dst, ValueTy::App("Map".into(), vec![ValueTy::String, ValueTy::Float, ValueTy::Cap(cap)])))
            }
            Expr::Cmp { op, lhs, rhs, span } => {
                let (a, _) = self.lower_value(lhs)?;
                let (b, _) = self.lower_value(rhs)?;
                let dst = self.fresh_value_reg();
                self.value_instrs.push(ValueInstr::ValueCompare { dst, op: cmp_op_from_ast(*op), a, b });
                Ok((dst, ValueTy::Bool))
            }
            Expr::Logic { op, lhs, rhs, span } => {
                let (a, _) = self.lower_value(lhs)?;
                let (b, _) = self.lower_value(rhs)?;
                let dst = self.fresh_value_reg();
                self.value_instrs.push(ValueInstr::ValueLogic { dst, op: logic_op_from_ast(*op), a, b });
                Ok((dst, ValueTy::Bool))
            }
```

Add helper conversions (`cmp_op_from_ast`, `logic_op_from_ast`) mapping `ast::CmpOp`/`ast::LogicOp` to `ir::CmpOp`/`ir::LogicOp`.

Add a lowering arm for `Expr::Apply { name, args, .. }` when `name` is one of the collection ops — emit `ValueCallBuiltin` with the mapped `ValueBuiltinOp` and its args, and compute the result `ValueTy` by the op's signature (a small `value_builtin_ty(&self, name, arg_tys) -> Result<ValueTy, CompileError>` helper — see Step 4). Place this check **before** the existing builtin/def resolution in `lower_value` (the collection op names are reserved).

- [ ] **Step 4: Add `value_builtin_ty` signatures**
Add a helper that maps op name → `(ValueBuiltinOp, fn(arg_tys) -> ValueTy)`; `insert`
resolves by arity (3 → Map, 2 → Set):

```rust
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
```

And a result-type function (used for both the type check and capacity accounting).
Element/value types are extracted from the container argument types:

```rust
    fn value_builtin_ty(&self, name: &str, args: &[ValueTy], span: Span) -> Result<ValueTy, CompileError> {
        match name {
            "cons" | "tail" | "filter" => match &args[1] {
                ValueTy::App("List".into(), inner) => Ok(ValueTy::App("List".into(), inner.clone())),
                other => Err(CompileError::Type { msg: format!("{name} expects a List, got {other:?}"), span }),
            },
            "head" => {
                let elem = match &args[0] {
                    ValueTy::App("List".into(), inner) => inner[0].clone(),
                    other => ValueTy::Float,
                };
                Ok(ValueTy::App("Maybe".into(), vec![elem]))
            }
            "length" => Ok(ValueTy::Int),
            "map" => {
                let elem = match &args[1] {
                    ValueTy::App("List".into(), inner) => inner[0].clone(),
                    other => ValueTy::Float,
                };
                Ok(ValueTy::App("List".into(), vec![elem, ValueTy::Cap(0)])) // cap refined in Phase 8
            }
            "fold" => Ok(args[0].clone()), // result type = the accumulator
            "list" => Ok(ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(0)])),
            "insert" => match args.len() {
                3 => Ok(ValueTy::App("Map".into(), args.to_vec())),
                _ => Ok(ValueTy::App("Set".into(), vec![args[0].clone(), ValueTy::Cap(0)])),
            },
            "lookup" => {
                let v = match &args[1] {
                    ValueTy::App("Map".into(), inner) => inner[1].clone(),
                    _ => ValueTy::Float,
                };
                Ok(ValueTy::App("Maybe".into(), vec![v]))
            }
            "member" => Ok(ValueTy::Bool),
            "empty_map" => Ok(ValueTy::App("Map".into(), vec![ValueTy::String, ValueTy::Float, ValueTy::Cap(0)])),
            "empty_set" => Ok(ValueTy::App("Set".into(), vec![ValueTy::Float, ValueTy::Cap(0)])),
            _ => Err(CompileError::Unsupported(format!("unknown collection op {name}"))),
        }
    }
```

The capacity in these result types is refined to the concrete cap by the
argument's `Cap` (see the cap-flow rule in Task 8.2); for the
`ValueListLit`/`ValueMapLit` paths the cap comes from the literal length.

- [ ] **Step 5: Interpreter dispatch + overflow latch**

In `program.rs`, add the latch field to `RillProgram`:

```rust
    /// Runtime value-track error (capacity overflow), cleared each tick.
    pub(crate) value_error: Option<ProcessError>,
```

Initialize `value_error: None` in both constructors (`new`, `new_with`). Add an accessor for the value-phase wrapper to read it.

In `interp.rs`, implement `exec_value_call_builtin` (dispatched from `exec_value_instr` for `ValueInstr::ValueCallBuiltin`), with the core cases:

```rust
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
            // cons x xs: build a new List sharing the source elems, append x.
            let xs = prog.value_regs[args[1]].and_then(|r| prog.arena.get(r).cloned());
            match xs {
                Some(Value::List { mut elems, cap }) => {
                    if elems.len() >= cap {
                        prog.value_error = Some(ProcessError::processing("list capacity exceeded"));
                        return;
                    }
                    let x = prog.value_regs[args[0]].and_then(|r| prog.arena.copy(r));
                    match x {
                        Some(xr) => {
                            elems.push(xr);
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
                .map(|v| match v { Value::List { elems, .. } => elems.len() as i64, _ => 0 })
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
                Some(e) => alloc_owned(prog, Value::Sum(0, vec![e])), // Just e
                None => alloc_owned(prog, Value::Sum(1, vec![])),     // Nothing
            };
        }
        Map => {
            // map f xs — dispatch f per element (run_fragment).
            let f = prog.value_regs[args[0]];
            let xs = prog.value_regs[args[1]].and_then(|r| prog.arena.get(r).cloned());
            if let (Some(fr), Some(Value::List { elems, cap })) = (f, xs) {
                let mut out = Vec::with_capacity(elems.len());
                for e in &elems {
                    // push a temp frame and call the closure fragment
                    let out_e = call_closure_single(prog, fr, *e, drops);
                    if let Some(o) = out_e { out.push(o); }
                }
                prog.value_regs[dst] = alloc_owned(prog, Value::List { elems: out, cap });
            } else {
                prog.value_regs[dst] = None;
            }
        }
        // Fold, Filter, Tail, ListEmpty, InsertMap, Lookup, Member, InsertSet,
        // MapEmpty, SetEmpty — analogous; see the follow-up task text.
        _ => prog.value_regs[dst] = None,
    }
}
```

`call_closure_single` is a thin wrapper over the existing `run_fragment` path: it binds the element as the fragment's first argument register, runs the fragment, and returns the result ref (implement it reusing `run_fragment`'s frame machinery — see `run_fragment` at `interp.rs:714`).

`ValueCompare`/`ValueLogic` handlers:

```rust
        ValueInstr::ValueCompare { dst, op, a, b } => {
            let res = match (prog.value_regs[*a], prog.value_regs[*b]) {
                (Some(x), Some(y)) => {
                    let c = value_cmp(prog, x, y);
                    let b = match op {
                        CmpOp::Eq => c == 0, CmpOp::Ne => c != 0,
                        CmpOp::Lt => c < 0,  CmpOp::Gt => c > 0,
                        CmpOp::Le => c <= 0, CmpOp::Ge => c >= 0,
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
                    Some(Value::Bool(match op { LogicOp::And => ax && ay, LogicOp::Or => ax || ay }))
                }
                _ => None,
            };
            prog.value_regs[*dst] = res.and_then(|v| alloc_owned(prog, v));
        }
        ValueInstr::ValueBool { dst, value } => {
            prog.value_regs[*dst] = alloc_owned(prog, Value::Bool(*value));
        }
        ValueInstr::ValueConstString { dst, value } => {
            prog.value_regs[*dst] = alloc_owned(prog, Value::String(value.clone()));
        }
        ValueInstr::ValueListLit { dst, elems, cap } => {
            prog.value_regs[*dst] = if let Some(refs) = read_field_refs(prog, elems) {
                alloc_owned(prog, Value::List { elems: refs, cap: *cap })
            } else { None };
        }
        ValueInstr::ValueMapLit { dst, keys, vals, cap } => {
            prog.value_regs[*dst] = if let (Some(ks), Some(vs)) = (read_field_refs(prog, keys), read_field_refs(prog, vals)) {
                alloc_owned(prog, Value::Map { pairs: ks.into_iter().zip(vs).collect(), cap: *cap })
            } else { None };
        }
```

The value-phase driver: find where the value instructions are executed per tick (the loop calling `exec_value_instr`), and after the loop check the latch:

```rust
    if let Some(err) = prog.value_error.take() {
        return Err(err);
    }
```

`ProcessError` import: `use rill_core::traits::ProcessError;`.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p rill-lang --test collections_list`
Expected: PASS (the four tests). If `fold`/`filter` still error ("not yet supported"), the remaining ops are wired in Task 6.4 — implement them then.

- [ ] **Step 7: Commit**

```bash
git add rill-lang/src/lower.rs rill-lang/src/backend/interp.rs rill-lang/src/program.rs rill-lang/tests/collections_list.rs
git commit -m 'feat(rill-lang): list ops end-to-end with strict capacity overflow error'
```

### Task 6.4: Complete the dispatcher — `tail`, `fold`, `filter`, `list`, `insert`, `lookup`, `member`, Map/Set empties

**Files:**
- Modify: `rill-lang/src/backend/interp.rs`

- [ ] **Step 1: Write the failing integration test**

Append to `rill-lang/tests/collections_map_set.rs` (create it):

```rust
//! End-to-end Map/Set tests: insert/lookup/member with structural ordering.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn map_insert_lookup_member() {
    let mut prog = compile::<f32>(
        "m = { \"a\": 1.0, \"b\": 2.0 }; main = match (lookup \"a\" m) of { Nothing => 0.0; Just x => x; };",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Float(1.0));
}

#[test]
fn set_member_after_insert() {
    let mut prog = compile::<f32>(
        "s = insert 5 (empty_set 8); main = member 5 s;",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Bool(true));
}

#[test]
fn tail_and_filter_preserve_capacity() {
    let mut prog = compile::<f32>(
        "main = length (filter (fn x -> x > 1.0) (tail [1.0, 2.0, 3.0]));",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Int(1));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rill-lang --test collections_map_set`
Expected: FAIL — `lookup`/`insert`/`member`/`empty_set` unsupported or wrong results.

- [ ] **Step 3: Implement the remaining ops**

In `exec_value_call_builtin`, replace the `_ =>` catch-all with concrete arms:

- `Tail`: read the source list, build a new `List` with `elems[1..]` (copy refs, then `drop_ref` the removed head from the NEW owner's perspective — allocate first, then drop the head ref by recounting: since the new list owns `elems[1..]` (RC++ each via `copy`), then `drop_ref` the original head once; keep `cap`).
- `Fold`: `fold f z xs` — seed `acc = z`; for each element call the closure `f` with `(elem, acc)` as a two-arg fragment call, result becomes `acc`. Returns `acc`.
- `Filter`: predicate `p` per element (one-arg closure call); keep those for which the result is `Value::Bool(true)`.
- `ListEmpty`: `list n` — args[0] is an Int value register; allocate `Value::List { elems: vec![], cap: n }`.
- `InsertMap`: `insert k v m` — find position in the sorted pairs via `value_cmp`; if a key compares equal, COW-replace the value (build a new Map with the pair replaced); else if `len >= cap` set `value_error` (`"map capacity exceeded"`), else COW-insert at the sorted position (copying all existing key/vals, RC++ each). Result: new `Map { pairs, cap }` with `cap` unchanged.
- `Lookup`: binary search `pairs` by `value_cmp` for the key; return `Just v` / `Nothing` (Same `Value::Sum(0,..)`/`Sum(1,..)` encoding as Head).
- `Member`: binary search; return `Value::Bool(found)`.
- `InsertSet`: same as InsertMap but over `elems` with no value.
- `MapEmpty`/`SetEmpty`: `Value::Map { pairs: vec![], cap }` / `Value::Set { elems: vec![], cap }` from the Int arg.

All child refs created by these ops must be `copy`ed so both the source and the new container own them (see the arena COW conventions).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rill-lang --test collections_map_set`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/backend/interp.rs rill-lang/tests/collections_map_set.rs
git commit -m 'feat(rill-lang): map/set insert/lookup/member and tail/fold/filter'
```

---

# Phase 7 — Builtin data types (`Maybe`/`Pair`/`Either`) and nullary constructors

### Task 7.1: Nullary-constructor construction and match

**Files:**
- Modify: `rill-lang/src/types/infer.rs`, `rill-lang/src/lower.rs`, `rill-lang/src/backend/interp.rs`

- [ ] **Step 1: Write the failing integration test**

Create `rill-lang/tests/collections_maybe_either.rs`:

```rust
//! Maybe/Pair/Either as builtin data types; nullary constructor support.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn nothing_and_just_via_match() {
    let mut prog = compile::<f32>(
        "main = match (Nothing) of { Nothing => 42.0; Just x => x; };",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Float(42.0));
}

#[test]
fn user_nullary_constructors_work() {
    let mut prog = compile::<f32>(
        "data Color = Red | Green Float; main = match (Red) of { Red => 1.0; Green g => g; };",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Float(1.0));
}

#[test]
fn pair_projection_and_either() {
    let mut prog = compile::<f32>(
        "p = Pair { first: 2.0, second: 3.0 }; e = Left 7.0; main = p.first;",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Float(2.0));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rill-lang --test collections_maybe_either`
Expected: FAIL — `Nothing`/`Red` bare-ref construction rejected, or `match` over them rejected.

- [ ] **Step 3: Allow bare-ref nullary construction**

In `infer.rs`, where a bare `Ref(name)` in value position is resolved: if `name` is a **sum constructor with an empty payload** (look up `sum_ctor_payload` across `env.data_types`), type it as that sum's type and record it as a nullary construction. Extend the `Def::Sum` ctor table in `build_type_env` so nullary ctors are known (they already are — payload `vec![]`).

In `lower.rs`, lower a `Ref(name)` that resolves to a nullary sum constructor as:

```rust
            // nullary sum constructor: Sum(idx, [])
            let dst = self.fresh_value_reg();
            self.value_instrs.push(ValueInstr::ValueConstructSum { dst, ctor: idx, payload: vec![] });
```

- [ ] **Step 4: Extend `match` lowering for nullary arms and builtin ctors**

`lower.rs` `lower_value` for `Expr::Match`: resolve ctor names to indices (already done for user sums). Ensure an arm with **zero bindings** (`Nothing => ...`) lowers to `ValueMatch { dst: vec![], slot, ctor }` — `ValueMatch` already supports `dst: Vec<usize>`; verify the interpreter arm returns an empty payload list for nullary ctors (`Value::Sum(i, [])`).

Also resolve ctor names against the builtin shapes (`Just`/`Nothing`/`Left`/`Right`) by looking up the injected `data_types` (Phase 3 added them).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rill-lang --test collections_maybe_either`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/types/infer.rs rill-lang/src/lower.rs rill-lang/src/backend/interp.rs rill-lang/tests/collections_maybe_either.rs
git commit -m 'feat(rill-lang): nullary constructors and builtin Maybe/Pair/Either match'
```

---

# Phase 8 — Kind polymorphism over constructors (`Functor`/`Foldable`)

### Task 8.1: Class-var arity + instance kind checking

**Files:**
- Modify: `rill-lang/src/types/ty.rs`, `rill-lang/src/types/infer.rs`

- [ ] **Step 1: Write the failing integration test**

Create `rill-lang/tests/hkt_typeclass.rs`:

```rust
//! Kind polymorphism: typeclasses over type constructors (Functor/Foldable).

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn fmap_over_list_is_compile_time_inlined() {
    let src = r#"
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        instance Functor List where { fmap g xs = map g xs; }
        main = fmap (fn x -> x * 2.0) [1.0, 2.0, 3.0];
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    // result: [2.0, 4.0, 6.0]; main outputs the list — project via head+Just.
}

#[test]
fn fmap_over_maybe_matches() {
    let src = r#"
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        instance Functor Maybe where { fmap g m = match m of { Nothing => Nothing; Just x => Just (g x); }; }
        main = match (fmap (fn x -> x + 1.0) (Just 1.0)) of { Nothing => 0.0; Just x => x; };
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Float(2.0));
}

#[test]
fn kind_mismatch_is_compile_error() {
    let src = r#"
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        instance Functor Pair where { fmap g p = p; }
        main = fmap (fn x -> x) (Pair { first: 1.0, second: 2.0 });
    "#;
    let res = compile::<f32>(src);
    assert!(res.is_err(), "Pair has arity 2; Functor needs arity 1");
}

#[test]
fn foldable_over_list() {
    let src = r#"
        typeclass Foldable f where { foldr: (a -> b -> b) -> b -> f a -> b; }
        instance Foldable List where { foldr f z xs = fold f z xs; }
        main = foldr (fn a b -> a + b) 0.0 [1.0, 2.0, 3.0];
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Float(6.0));
}

#[test]
fn capacity_flows_through_fmap() {
    // fmap must preserve the source capacity in the result type (List b n).
    let src = r#"
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        instance Functor List where { fmap g xs = map g xs; }
        main = length (fmap (fn x -> x * 2.0) [1.0, 2.0, 3.0, 4.0]);
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Int(4));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rill-lang --test hkt_typeclass`
Expected: FAIL — instance over a constructor not resolved / kind not checked.

- [ ] **Step 3: Infer class-var arity from signatures**

When building `TypeclassInfo` from a `Def::Typeclass`, compute `arity` = the maximum application depth of the class var in the method sigs (`TypeExpr::TApp` head == class var → 1 + nested depth; else 0).

- [ ] **Step 4: Kind-check instances**

In `validate_instances` (infer.rs) — when the instance's `ty` is a **builtin constructor**, check `env.ctor_arity(ty) - usize::from(env.ctor_has_cap(ty)) == class.arity` (capacity argument excluded from the value arity). `instance Functor Pair` → arity 2 vs 1 → `CompileError::Type` with a `kind` message.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/ty.rs rill-lang/src/types/infer.rs
git commit -m 'feat(rill-lang): class-var arity and kind-checked constructor instances'
```

### Task 8.2: Cap-aware constructor matching and method inlining

**Files:**
- Modify: `rill-lang/src/types/infer.rs`, `rill-lang/src/lower.rs`

- [ ] **Step 1: (test from Task 8.1 already covers this)**

Run: `cargo test -p rill-lang --test hkt_typeclass`
Expected: still FAIL — `fmap` call unresolved.

- [ ] **Step 2: Implement `match_ctor_pattern`**

Add to `infer.rs` (or `lower.rs`, shared via a helper on `TypeEnv`):

```rust
/// Match a signature type against a concrete value type for class-var
/// instantiation. `f a` (head is the class var) matches `App("List", [Int, Cap 4])`
/// by binding f := List, a := Int, and unifying the Cap slot with Cap 4.
pub fn match_ctor_pattern(
    pat: &ValueTy,
    concrete: &ValueTy,
    subst: &mut Subst,
) -> bool {
    match (pat, concrete) {
        (ValueTy::App(f, p_args), ValueTy::App(c, c_args))
            if c != f && matches!(f.as_str(), "f") => // f is the class-var head
        {
            // bind f := c, unify non-Cap args positionally, and bind the Cap slot.
            // (Kind-var binding is recorded out-of-band by the resolver.)
            let mut i = 0;
            let mut j = 0;
            while j < c_args.len() {
                if let ValueTy::Cap(_) = c_args[j] {
                    // capacity slot: unify with the pattern's Cap or leave free
                    if i < p_args.len() && !matches!(p_args[i], ValueTy::Cap(_)) {
                        return false;
                    }
                    j += 1;
                    i += 1;
                } else {
                    if i >= p_args.len() { return false; }
                    if !unify_value(&p_args[i], &c_args[j], subst, Span::new(0, 0)).is_ok() {
                        return false;
                    }
                    i += 1;
                    j += 1;
                }
            }
            true
        }
        (a, b) => unify_value(a, b, subst, Span::new(0, 0)).is_ok(),
    }
}
```

The concrete, precise routine is a method on `TypeEnv`: `match_ctor_pattern(&self, class_var: &str, pat: &ValueTy, concrete: &ValueTy, subst: &mut Subst) -> bool`. It binds the class var to the concrete constructor **name** (returned to the caller) and unifies the remaining pattern args with the concrete's non-Cap args; the capacity is preserved because the concrete arg's `Cap` is already part of `concrete`.

- [ ] **Step 3: Instantiate instance bodies over constructors**

In the typeclass method-call resolution path (`infer.rs` `infer_typeclass_call`, around line 1104 and `validate_instances`), when the instance `ty` is a constructor:
1. Unify the signature against the call-site arg types with `match_ctor_pattern` → yields `f := ctor`, `a := <arg>`, and the concrete capacity carried in the arg's `Cap`.
2. Substitute into the instance body: instantiate the body with `a`/`b` bound to the unified types, β-substitute the call arguments, and inline (existing `method_lifting` recursion guard applies).
3. In the body, the class-var type `f b` is re-expanded to `App(ctor, [b, <carried cap>])` — the **capacity flow**.

Extend `lower.rs` `resolve_method` usage to the same substitution for the constructor case, so `fmap g xs` lowers to the inlined body (`map g xs`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rill-lang --test hkt_typeclass`
Expected: PASS (all five tests).

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/infer.rs rill-lang/src/lower.rs
git commit -m 'feat(rill-lang): cap-aware constructor matching and compile-time fmap/foldr inlining'
```

---

# Phase 9 — Capacity-bound accounting and error-path polish

### Task 9.1: Container-typed subexpression accounting

**Files:**
- Modify: `rill-lang/src/lower.rs` (the capacity heuristic, ~line 2343)

- [ ] **Step 1: Write the failing integration test**

Create `rill-lang/tests/collections_capacity.rs`:

```rust
//! Arena capacity accounting for container-typed value subexpressions.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn container_outputs_pin_subtrees_across_ticks() {
    // A List output pins its full subtree across ticks; the arena bound must
    // include cap * elem slots or tick 2 exhausts the arena.
    let mut prog = compile::<f32>("main = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];").unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..3 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        let v = prog.value_outputs()[0].unwrap();
        assert!(matches!(
            prog.arena().get(v).unwrap(),
            &rill_lang::arena::Value::List { .. }
        ));
    }
}

#[test]
fn map_build_and_pin_across_ticks() {
    let mut prog = compile::<f32>(
        "m = insert \"a\" 1.0 { \"x\": 2.0 }; main = member \"x\" m;",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..3 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        let v = prog.value_outputs()[0].unwrap();
        assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Bool(true));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rill-lang --test collections_capacity`
Expected: FAIL — "value arena capacity exhausted at build time" panic on tick ≥ 2.

- [ ] **Step 3: Extend the heuristic**

In the capacity heuristic (`lower.rs`, the `value_capacity` computation), add the container subexpression accounting. Track, during `lower_value`, every value subexpression whose resulting `ValueTy` is a container (`List`/`Map`/`Set` — any `App` head in the builtin set) in a `Vec<ValueTy> container_tys`. Then:

```rust
    let container_capacity = lw.container_tys
        .iter()
        .map(|t| lw.subtree_size(t))
        .sum::<usize>();
```

and add `+ container_capacity` to `value_capacity`. `subtree_size` already returns exact container sizes (Phase 2).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rill-lang --test collections_capacity`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/lower.rs rill-lang/tests/collections_capacity.rs
git commit -m 'feat(rill-lang): arena capacity accounting for container values'
```

### Task 9.2: `Eq`/`Ord` constraint enforcement and error messages

**Files:**
- Modify: `rill-lang/src/types/infer.rs`, `rill-lang/src/error.rs`

- [ ] **Step 1: Write the failing integration test**

Append to `collections_map_set.rs`:

```rust
#[test]
fn func_key_is_compile_error() {
    // `Func` has no derived Ord — a Map with a function key must not compile.
    let mut src = "f = fn x -> x; main = lookup f { \"a\": 1.0 };";
    let res = compile::<f32>(src);
    assert!(res.is_err(), "function-typed map key must be rejected");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test collections_map_set func_key_is_compile_error`
Expected: FAIL — compiles (the constraint is not yet enforced).

- [ ] **Step 3: Enforce `Ord` at the collection-op call site**

In `lower_value`/inference for `insert`/`lookup`/`member`, after computing the key/element type, resolve `Ord <ty>`: `env.instances["Ord"].contains_key(ty_name)` where `ty_name = env.type_name_of_vty(key_ty)`. For a `Func` (or any unresolved/non-derivable) key type → `CompileError::Type { msg: "no Ord instance for function type", .. }`. Add a `span` parameter to `value_builtin_ty` and thread the call-site span through.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-lang --test collections_map_set`
Expected: PASS (all tests in the file).

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/infer.rs rill-lang/src/lower.rs rill-lang/tests/collections_map_set.rs
git commit -m 'feat(rill-lang): enforce Ord constraint on Map/Set keys'
```

---

# Phase 10 — Docs and final verification

### Task 10.1: Language reference and README

**Files:**
- Modify: `docs/src/guides/rill-lang.md`, `rill-lang/README.md`, `rill/CHANGELOG.md`

- [ ] **Step 1: Document the new features**

In `docs/src/guides/rill-lang.md` and `rill-lang/README.md`:
- Value types table: add `Bool`, `String`, `List a n`, `Map k v n`, `Set a n`, `Maybe a`, `Pair a b`, `Either a b`; the builtin type constructors with kinds; capacity-as-strict-bound semantics and the runtime `ProcessError` on overflow.
- New section "Higher-kinded types": `data Box a`, `typeclass Functor f`, `instance Functor List`, compile-time inline resolution, the cap-flow rule.
- New section "Collections": all ops (`cons`/`head`/`tail`/`length`/`map`/`fold`/`filter`, `list`/`empty_map`/`empty_set`, `insert`/`lookup`/`member`), list/map literals, `Eq`/`Ord` derived instances, value-track comparisons and logic.
- Update the "Known v1 limitations" section: remove "nullary constructors rejected"; add deferred items (user Eq/Ord instances, Hashable, string ops, concat/append/reverse).
- `CHANGELOG.md`: add the feature under `0.6.0-M2` (or the current version block).

- [ ] **Step 2: Verify the doc examples compile**

Run: `cargo test -p rill-lang` — doc examples in README/docs are `no_run` or `ignore` as appropriate; keep them as `no_run` if not self-contained.

- [ ] **Step 3: Commit**

```bash
git add docs/src/guides/rill-lang.md rill-lang/README.md CHANGELOG.md
git commit -m 'docs(rill-lang): document HKT and first-class collections'
```

### Task 10.2: Final verification

- [ ] **Step 1: Full workspace checks**

Run:
```bash
cargo test --workspace
cargo test --workspace --release
cargo clippy --workspace --all-features
cargo fmt --all
```
Expected: all green, zero warnings, zero fmt diffs.

- [ ] **Step 2: Update the design-settled memory / close out**

Add a memory record (`mind`) with the final file inventory and any deviations from the spec encountered during implementation.

- [ ] **Step 3: Final commit (if any fixes)**

```bash
git add -A
git commit -m 'chore(rill-lang): final polish after HKT + collections'
```

---

## Self-review notes (run after writing the plan)

- **Spec coverage:** every spec section maps to a phase: §2.1–2.2 (Phase 2), §2.3 (Phase 6/9), §2.4 (Phase 4/7), §2.5 (Phase 3/6/9.2), §2.6 (Phase 8), §3 (Phase 5), §5 (Phase 6.1), §6 (Phase 6.2/6.3), §7 (Phase 9.1), §10 (phases above), §11 (tests per phase), §12 (files per task).
- **Placeholder scan:** the dispatcher in Task 6.3 keeps `Fold/Filter/Tail/…` as `_ =>` stubs that are completed in Task 6.4 with concrete arms described there; `value_builtin_ty` returns conservative types refined in Task 8.2. These are intentional cross-task handoffs, each with a concrete completion task, not "TBD". `insert` resolves to `InsertMap`/`InsertSet` by arity inside `value_builtin(name, nargs)`.
- **Type consistency:** `ValueBuiltinOp`, `CmpOp`/`LogicOp` are defined once in `ir.rs` (6.1) and reused everywhere; `value_cmp` signature is stable across tasks; `ValueTy::App("List", [elem, Cap(n)])` is the canonical List type in all phases.