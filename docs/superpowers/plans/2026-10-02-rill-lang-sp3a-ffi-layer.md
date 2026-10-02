# SP-3a: FFI Layer in rill-lang (`foreign fn` + `FixedBuffer`/`Buffer`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the FFI layer to rill-lang: `foreign fn name : TypeExpr;` declarations, a first-class signal-channel type `FixedBuffer a` + a `Buffer` typeclass skeleton in a new `SIGNAL_PRELUDE`, and a runtime factory registry (`rill_lang::ffi`) — so a foreign-declared builtin compiles and runs, **alongside** the existing `BuiltinSig` path (which stays intact until SP-3b).

**Architecture:** Additive, parallel mechanism. `TypeEnv` gains a `foreign_sigs: HashMap<String, TypeExpr>` table registered from `foreign fn` declarations. Inference and lowering resolve a foreign name by its declared signature (a `FfiSig` descriptor with signal channels / scalar params / variadic signal list), emitting the same `Instr::CallBlock` as today. `RillProgram::build` resolves the factory from a new `rill_lang::ffi::ForeignRegistry<T>` when the name is not in the existing rill-core `Registry<T>`. The block track, interpreter, and `ResourceRegistry` are untouched.

**Tech Stack:** Rust, no new external dependencies. Branch `feature/rill-lang-categories`.

---

## File map (SP-3a)

| File | Change |
|---|---|
| `rill-lang/src/types/ty.rs` | `SIGNAL_PRELUDE` const; `FixedBuffer` in `ctor_kinds`; `Buffer` typeclass; `TypeEnv::foreign_sigs`; `with_builtins` registration; `register_decls` `Def::Foreign` arm |
| `rill-lang/src/ast.rs` | `Def::Foreign { name, sig, span }`; `name()` + `is_decl()` arms |
| `rill-lang/src/lexer.rs` | `Tok::KwForeign` + keyword |
| `rill-lang/src/parser.rs` | `parse_foreign_def` + `parse_top_def` dispatch |
| `rill-lang/src/render.rs` | `Def::Foreign` render arm |
| `rill-lang/src/types/ffi.rs` (new) | `FfiParam`/`FfiSig` descriptors + `ffi_sig_from_typeexpr` |
| `rill-lang/src/types/infer.rs` | foreign resolution in `infer_ref` + `infer_apply_impl`; phase-loop registration of `Def::Foreign` (via `register_decls`) |
| `rill-lang/src/lower.rs` | foreign resolution → `CallBlock` (walk `FfiSig`); `Def::Foreign` passthrough |
| `rill-lang/src/ffi.rs` (new) | `ForeignRegistry<T>` + `register_block`/`register_multichannel_block`/`build` |
| `rill-lang/src/program.rs` | `RillProgram::build` foreign fallback |
| `rill-lang/src/lib.rs` | `compile_program_with_ffi` / `compile_graph_with_ffi` entry points |
| `rill-lang/tests/ffi.rs` (new) | integration tests |
| `rill-lang/README.md` | docs (SP-3a surface) |

Verify: `cargo test -p rill-lang` per task; `cargo test --workspace`, `cargo clippy --all-features --workspace`, `cargo fmt` before finishing.

---

## Task 1: `SIGNAL_PRELUDE` + `FixedBuffer` / `Buffer` builtin types

**Problem.** The signal track has no name in the language. Add the type layer first: a builtin `FixedBuffer a` (kind `* -> *`, the signal-channel type, strictly `FixedBuffer[BUF]`) and a `Buffer` typeclass skeleton, declared in a new `SIGNAL_PRELUDE`.

**Files:** `types/ty.rs`

- [ ] **Step 1: Write the failing test**

Add to `rill-lang/tests/ffi.rs` (create the file):

```rust
//! SP-3a: FFI layer — foreign fn declarations + FixedBuffer/Buffer types.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn buffer_typeclass_registers_from_signal_prelude() {
    // `Buffer` is a builtin typeclass over buffer types, `FixedBuffer a` the
    // signal-channel type. `main` is trivial — this proves the SIGNAL_PRELUDE
    // parses, registers, and the instance kind-checks.
    let src = r#"
        typeclass UsesBuffer a where { use_buf: FixedBuffer a -> FixedBuffer a; }
        main = 1.0;
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test ffi buffer_typeclass_registers_from_signal_prelude`
Expected: FAIL — `unknown identifier`/parse error for `FixedBuffer` (no such type yet).

- [ ] **Step 3: Add `SIGNAL_PRELUDE` + types**

In `rill-lang/src/types/ty.rs`, next to `CATEGORY_PRELUDE` (line ~277), add:

```rust
/// Built-in signal-track declarations: the first-class buffer type and the
/// `Buffer` typeclass. Registered by [`TypeEnv::with_builtins`]. The home of the
/// Faust combinators and buffer math in later stages.
pub(crate) const SIGNAL_PRELUDE: &str = r#"
typeclass Buffer b where { }
instance Buffer (FixedBuffer a);

main = _;
"#;
```

In `with_builtins` (`ty.rs:369`), add `FixedBuffer` to `ctor_kinds` and register the prelude. The `ctor_kinds` array becomes:

```rust
let ctor_kinds = [
    ("List".to_string(), 1usize),
    ("Maybe".to_string(), 1usize),
    ("Set".to_string(), 1usize),
    ("Map".to_string(), 2usize),
    ("Pair".to_string(), 2usize),
    ("Either".to_string(), 2usize),
    ("FixedBuffer".to_string(), 1usize),
]
.into_iter()
.collect();
```

At the end of `with_builtins`, after the existing `CATEGORY_PRELUDE` registration block (`ty.rs:430-436`), add a second block:

```rust
// Signal-track prelude: the `Buffer` typeclass + `FixedBuffer` type. Parsed the
// same way as `CATEGORY_PRELUDE` (a parse failure here is a compiler bug).
let stoks = crate::lexer::tokenize(SIGNAL_PRELUDE);
debug_assert!(stoks.is_ok(), "signal prelude must lex");
let sprogram = crate::parser::parse(&stoks.ok().unwrap(), SIGNAL_PRELUDE.as_bytes());
debug_assert!(sprogram.is_ok(), "signal prelude must parse");
env.register_decls(&sprogram.ok().unwrap().defs);
env.derive_superclass_instances();
env
```

**Registration note:** `SIGNAL_PRELUDE` contains a `typeclass` and an `instance` — both handled by `register_decls`. `FixedBuffer a` needs no `Def::Data` registration (it is a builtin constructor, present in `ctor_kinds`). If `register_decls`' `Def::Instance` arm or `validate_instances` rejects `instance Buffer (FixedBuffer a)` because `Buffer` has zero methods, confirm the failure is benign and, if it blocks compilation, keep `Buffer` declared with an empty-but-present method list is NOT required — instead note in the test comment that the instance is a kind-check-only declaration. The `main = _;` line makes the prelude a parseable program (same convention as `CATEGORY_PRELUDE`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-lang --test ffi buffer_typeclass_registers_from_signal_prelude`
Expected: PASS. Also run `cargo test -p rill-lang` — the prelude adds `FixedBuffer`/`Buffer` as reserved names; existing tests must not break.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/types/ty.rs rill-lang/tests/ffi.rs
git commit -m 'feat(rill-lang): SIGNAL_PRELUDE — FixedBuffer type + Buffer typeclass skeleton'
```

---

## Task 2: `foreign fn` syntax — lexer, AST, parser, render

**Problem.** The language has no way to declare a builtin signature. Add `foreign fn name : TypeExpr;`.

**Files:** `lexer.rs`, `ast.rs`, `parser.rs`, `render.rs`

- [ ] **Step 1: Write the failing test**

Append to `rill-lang/tests/ffi.rs`:

```rust
#[test]
fn foreign_fn_declaration_parses() {
    // `foreign fn name : TypeExpr;` — a carried signal signature. Parsing alone
    // is the bar here; resolution lands in Task 4.
    let src = r#"
        foreign fn biquad : FixedBuffer f32 -> Float -> Float -> Float -> FixedBuffer f32;
        main = 1.0;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    assert!(
        prog.defs.iter().any(|d| matches!(d, rill_lang::ast::Def::Foreign { name, .. } if name == "biquad"))
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test ffi foreign_fn_declaration_parses`
Expected: FAIL — compile error (`Def::Foreign` does not exist; `foreign` is an identifier).

- [ ] **Step 3: Lexer — `Tok::KwForeign`**

`rill-lang/src/lexer.rs`. Add to the `Token` enum near `KwInstance` (line ~67):

```rust
/// `foreign` keyword — foreign fn declaration.
KwForeign,
```

In the keyword match (line ~320), add (before the `_ => Tok::Ident` arm):

```rust
"foreign" if !followed_by_paren => Tok::KwForeign,
```

Add `Tok::KwForeign` to the keyword classifier at `lexer.rs:558` (the `matches!(t.tok, ...)` keyword list) so `followed_by_paren` detection treats it as a keyword.

- [ ] **Step 4: AST — `Def::Foreign`**

`rill-lang/src/ast.rs`. Add a variant to `Def` after `Def::Instance` (line ~432):

```rust
/// `foreign fn name : TypeExpr;` — a foreign (Rust-implemented) builtin whose
/// signature is declared in the language. The language owns the contract; a
/// runtime factory bound by name provides the implementation.
Foreign {
    /// Foreign function name (the builtin's registry name).
    name: String,
    /// Carried type signature: `FixedBuffer f32 -> Float -> ... -> FixedBuffer f32`.
    sig: TypeExpr,
    /// Span.
    span: Span,
},
```

Add arms:
- `name()` (line ~450): `Def::Foreign { name, .. } => name,`
- `is_decl()` (line ~487, the `matches!` list): add `| Def::Foreign { .. }`

- [ ] **Step 5: Parser — `parse_foreign_def`**

`rill-lang/src/parser.rs`. In `parse_top_def` (line ~316), add a dispatch arm after `Tok::KwInstance`:

```rust
Tok::KwForeign => return self.parse_foreign_def(),
```

Add the method after `parse_instance_def`:

```rust
/// `foreign fn name : TypeExpr;` — a foreign builtin signature declared in the
/// language. Parsed by the standard type parser; the carried arrow's `FixedBuffer`
/// channels are the signal-track parameters.
fn parse_foreign_def(&mut self) -> Result<Def, CompileError> {
    let start = self.bump().span.start;
    self.eat(&Tok::KwFn)?;
    let (name, _) = self.expect_ident()?;
    self.eat(&Tok::Colon)?;
    let sig = self.parse_type_expr()?;
    self.eat(&Tok::Semi)?;
    Ok(Def::Foreign {
        name,
        sig,
        span: self.span_from(start),
    })
}
```

- [ ] **Step 6: Render — `Def::Foreign`**

`rill-lang/src/render.rs`. Add an arm after `Def::Instance` (line ~119):

```rust
Def::Foreign { name, sig, .. } => {
    write!(buf, "{pad}foreign fn {name}: ").ok();
    render_type_expr(sig, buf);
    write!(buf, ";").ok();
    Ok(())
}
```

- [ ] **Step 7: Run test to verify it passes + regression**

Run: `cargo test -p rill-lang --test ffi foreign_fn_declaration_parses`
Expected: PASS. Run `cargo test -p rill-lang` — any exhaustive `Def` match the compiler flags (outside `ty.rs`, `reduce.rs`, `infer.rs` phase loops, `lower.rs`) must gain a `Def::Foreign { .. }` passthrough arm. Fix compiler-driven exhaustiveness errors.

- [ ] **Step 8: Commit**

```bash
git add rill-lang/src/lexer.rs rill-lang/src/ast.rs rill-lang/src/parser.rs rill-lang/src/render.rs rill-lang/tests/ffi.rs
git commit -m 'feat(rill-lang): foreign fn declaration syntax + Def::Foreign AST'
```

---

## Task 3: `FfiSig` descriptor + `TypeEnv::foreign_sigs` registration

**Problem.** Inference and lowering must consume a foreign signature uniformly. Introduce a language-level descriptor (`FfiSig`) converted from the declared `TypeExpr`, and register `Def::Foreign` into `TypeEnv::foreign_sigs`.

**Files:** `types/ffi.rs` (new), `types/ty.rs`, `types/infer.rs`

- [ ] **Step 1: Write the failing test**

Append to `rill-lang/tests/ffi.rs`:

```rust
#[test]
fn foreign_sig_describes_signal_and_scalar_params() {
    use rill_lang::types::ffi::{FfiParam, ffi_sig_from_typeexpr};
    use rill_lang::ast::TypeExpr;

    // `FixedBuffer f32 -> Float -> Float -> Float -> FixedBuffer f32`
    let te = TypeExpr::TFunc(
        vec![
            TypeExpr::TApp("FixedBuffer".into(), vec![TypeExpr::TName("Float".into())]),
            TypeExpr::TName("Float".into()),
            TypeExpr::TName("Float".into()),
            TypeExpr::TName("Float".into()),
        ],
        Box::new(TypeExpr::TApp("FixedBuffer".into(), vec![TypeExpr::TName("Float".into())])),
    );
    let sig = ffi_sig_from_typeexpr(&te).expect("FFI sig");
    assert_eq!(sig.params.len(), 4);
    assert!(matches!(sig.params[0], FfiParam::Signal));
    assert!(matches!(sig.params[1], FfiParam::Scalar(..)));
    assert_eq!(sig.signal_outs, 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test ffi foreign_sig_describes_signal_and_scalar_params`
Expected: FAIL — `rill_lang::types::ffi` does not exist.

- [ ] **Step 3: Create `types/ffi.rs`**

Create `rill-lang/src/types/ffi.rs`:

```rust
//! FFI signature descriptors: the language-side contract for foreign builtins.
//!
//! A `foreign fn name : TypeExpr;` declaration is converted into an [`FfiSig`]
//! — a flat list of parameter descriptors consumed by inference and lowering.
//! `FixedBuffer a` is the signal-channel type; scalars are compile-time params.

use crate::ast::TypeExpr;

/// One parameter of a foreign function signature.
#[derive(Debug, Clone, PartialEq)]
pub enum FfiParam {
    /// A signal channel: `FixedBuffer a`. Contributes one input arity.
    Signal,
    /// A compile-time scalar parameter: `Float`/`Int`/`Bool`/`String`.
    Scalar,
    /// Variadic signal channels: `List (FixedBuffer a)`. Consumes all remaining
    /// signal arguments.
    VariadicSignal,
    /// A `Data`-typed record parameter (e.g. a mixer config). SP-3b.
    Record(String),
}

/// A parsed foreign function signature.
#[derive(Debug, Clone, PartialEq)]
pub struct FfiSig {
    /// Parameter descriptors in declaration order.
    pub params: Vec<FfiParam>,
    /// Number of signal output channels (result `FixedBuffer`s).
    pub signal_outs: usize,
}

/// Convert a carried `TypeExpr` (`a -> b -> c -> r`) into an [`FfiSig`].
///
/// Supported parameter types:
/// - `FixedBuffer a` → [`FfiParam::Signal`]
/// - `Float`/`Int`/`Bool`/`String` → [`FfiParam::Scalar`]
/// - `List (FixedBuffer a)` → [`FfiParam::VariadicSignal`]
/// - any other `TName` (a `Data` record type) → [`FfiParam::Record(name)`]
///
/// The result type is one or more `FixedBuffer` channels (`FixedBuffer a` →
/// 1 out; `(FixedBuffer a, FixedBuffer b)` → 2 outs). Returns `None` for a
/// malformed signature (non-carried, unsupported result).
pub fn ffi_sig_from_typeexpr(te: &TypeExpr) -> Option<FfiSig> {
    // Unroll the carried arrows into a flat param list.
    let mut params = Vec::new();
    let mut ret = te;
    loop {
        match ret {
            TypeExpr::TFunc(args, r) => {
                if args.len() != 1 {
                    // A multi-arg TFunc is only produced by the parser's flat
                    // `a -> b -> c` form; treat each arg as one param.
                    for a in args {
                        params.push(param_from_typeexpr(a)?);
                    }
                } else {
                    params.push(param_from_typeexpr(&args[0])?);
                }
                ret = r;
            }
            other => {
                return Some(FfiSig {
                    params,
                    signal_outs: outs_from_typeexpr(other)?,
                });
            }
        }
    }
}

fn param_from_typeexpr(te: &TypeExpr) -> Option<FfiParam> {
    match te {
        TypeExpr::TApp(head, args) if head == "FixedBuffer" && args.len() == 1 => {
            Some(FfiParam::Signal)
        }
        TypeExpr::TApp(head, args) if head == "List" && args.len() == 1 => {
            // `List (FixedBuffer a)` — variadic signal channels.
            if matches!(&args[0], TypeExpr::TApp(h, a) if h == "FixedBuffer" && a.len() == 1) {
                Some(FfiParam::VariadicSignal)
            } else {
                None
            }
        }
        TypeExpr::TName(n) if matches!(n.as_str(), "Float" | "Int" | "Bool" | "String") => {
            Some(FfiParam::Scalar)
        }
        TypeExpr::TName(n) => Some(FfiParam::Record(n.clone())),
        _ => None,
    }
}

fn outs_from_typeexpr(te: &TypeExpr) -> Option<usize> {
    match te {
        TypeExpr::TApp(head, args) if head == "FixedBuffer" && args.len() == 1 => Some(1),
        // `(FixedBuffer a, FixedBuffer b)` desugars to `Pair (FixedBuffer a) (FixedBuffer b)`.
        TypeExpr::TApp(head, args) if head == "Pair" && args.len() == 2 => {
            if args.iter().all(|a| {
                matches!(a, TypeExpr::TApp(h, x) if h == "FixedBuffer" && x.len() == 1)
            }) {
                Some(2)
            } else {
                None
            }
        }
        _ => None,
    }
}
```

Declare the module in `rill-lang/src/types/mod.rs`:

```rust
pub mod ffi;
```

Re-export from `rill-lang/src/types/mod.rs` (or `lib.rs` prelude) so tests can reach it:

```rust
pub use ffi::{FfiParam, FfiSig, ffi_sig_from_typeexpr};
```

- [ ] **Step 4: `TypeEnv::foreign_sigs` + `register_decls` arm**

`rill-lang/src/types/ty.rs`. Add a field to `TypeEnv` (after `data_arities`, line ~270):

```rust
/// Foreign function declarations: name → declared `TypeExpr` signature.
/// The language-side contract for Rust-implemented builtins (SP-3a FFI layer).
pub foreign_sigs: HashMap<String, crate::ast::TypeExpr>,
```

`TypeEnv::default()` (derive) covers it. In `register_decls` (line ~443), add a `Def::Foreign` arm:

```rust
Def::Foreign { name, sig, .. } => {
    self.foreign_sigs.insert(name.clone(), sig.clone());
}
```

- [ ] **Step 5: Run test to verify it passes + regression**

Run: `cargo test -p rill-lang --test ffi foreign_sig_describes_signal_and_scalar_params`
Expected: PASS. Run `cargo test -p rill-lang` — `Def::Foreign` in `register_decls` (ty.rs) and the infer phase loop (`infer.rs:969` `_ => {}` covers it) must not break.

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/types/ffi.rs rill-lang/src/types/mod.rs rill-lang/src/types/ty.rs rill-lang/tests/ffi.rs
git commit -m 'feat(rill-lang): FfiSig descriptor + TypeEnv::foreign_sigs registration'
```

---

## Task 4: Inference resolves foreign names

**Problem.** A `Ref`/`Apply` to a foreign name must type through its declared signature (`FixedBuffer` args → signal channels, scalars → constants).

**Files:** `types/infer.rs`

- [ ] **Step 1: Write the failing test**

Append to `rill-lang/tests/ffi.rs`:

```rust
#[test]
fn foreign_fn_types_as_signal_arrow() {
    // `gain : FixedBuffer f32 -> Float -> FixedBuffer f32` applied to a wire and
    // a constant — types as a 1→1 signal arrow.
    let src = r#"
        foreign fn gain : FixedBuffer f32 -> Float -> FixedBuffer f32;
        main = gain _ 0.5;
    "#;
    // Compilation through inference; lowering + runtime land in Task 6, so this
    // test must pass at the type level (compile() runs the whole pipeline — the
    // RillProgram::build fallback lands in Task 6; until then this test is
    // skipped or asserts inference directly).
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    // `process_ty` is the diagram type of the whole program (main).
    assert_eq!(typed.process_ty.arity_in(), 1);
    assert_eq!(typed.process_ty.arity_out(), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test ffi foreign_fn_types_as_signal_arrow`
Expected: FAIL — `unknown identifier 'gain'`.

- [ ] **Step 3: `infer_ref` bare-name path**

`rill-lang/src/types/infer.rs`, in `infer_ref` (the builtin-name block at line ~2354), add before the `ctx.sigs.builtin_sig(name)` check:

```rust
// Foreign (FFI) declaration: type through the language-side signature. A bare
// ref is valid when every param is a signal channel (no scalar params yet).
if let Some(fsig) = ctx.env.foreign_sigs.get(name).cloned() {
    if let Some(sig) = crate::types::ffi::ffi_sig_from_typeexpr(&fsig) {
        if sig.params.iter().all(|p| matches!(p, crate::types::ffi::FfiParam::Signal))
            && sig.signal_outs > 0
        {
            return Ok(ArrowTy::uniform(
                sig.params.len(),
                sig.signal_outs,
                Scalar::Float,
            ));
        }
    }
}
```

- [ ] **Step 4: `infer_apply_impl` apply path**

`rill-lang/src/types/infer.rs`, in `infer_apply_impl`, before the `ctx.sigs.builtin_sig(name)` arm (line ~3247), add:

```rust
// Foreign (FFI) declaration: validate arity + scalar params from the language
// signature, then type as a signal arrow.
if let Some(fsig) = ctx.env.foreign_sigs.get(name).cloned() {
    if let Some(sig) = crate::types::ffi::ffi_sig_from_typeexpr(&fsig) {
        let mut signal_ins = 0usize;
        let mut param_pos = 0usize;
        for p in &sig.params {
            match p {
                crate::types::ffi::FfiParam::Signal => signal_ins += 1,
                crate::types::ffi::FfiParam::VariadicSignal => {
                    // All remaining args after the fixed params are signal wires.
                    let fixed_scalars: usize = sig
                        .params
                        .iter()
                        .filter(|q| matches!(q, crate::types::ffi::FfiParam::Scalar))
                        .count();
                    if args.len() >= fixed_scalars {
                        signal_ins += args.len() - fixed_scalars;
                    }
                }
                crate::types::ffi::FfiParam::Scalar => {
                    if param_pos >= args.len() {
                        break;
                    }
                    let at = infer_expr(ctx, &args[param_pos])?;
                    if at.arity_in() != 0 || at.arity_out() != 1 {
                        return Err(CompileError::Type {
                            msg: format!("param at position {param_pos} of `{name}` must be constant"),
                            span: args[param_pos].span(),
                        });
                    }
                    param_pos += 1;
                }
                crate::types::ffi::FfiParam::Record(_) => {
                    // SP-3b — not yet resolved at inference.
                    return Err(CompileError::Type {
                        msg: format!("foreign `{name}` has a record parameter (SP-3b)"),
                        span,
                    });
                }
            }
        }
        return Ok(ArrowTy::uniform(signal_ins, sig.signal_outs, Scalar::Float));
    }
}
```

**Note on arg counting:** the existing `builtin_sig` arm counts `signal_ins` over `ParamType::Signal` params and validates constants per-position. The foreign arm above mirrors that. Scalar args are checked as constant-or-value; `param_pos` walks `args` positionally — the existing code's `infer_apply` validates this way (see the `Float | Int` arm at `infer.rs:3279`).

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p rill-lang --test ffi foreign_fn_types_as_signal_arrow`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/types/infer.rs rill-lang/tests/ffi.rs
git commit -m 'feat(rill-lang): infer foreign fn calls through FfiSig descriptors'
```

---

## Task 5: Lowering emits `CallBlock` for foreign names

**Problem.** A foreign call must lower to `Instr::CallBlock` with signal sources, folded scalar params, and a `BuiltinInstance`, exactly like the builtin path.

**Files:** `lower.rs`

- [ ] **Step 1: Write the failing test**

Append to `rill-lang/tests/ffi.rs`:

```rust
#[test]
fn foreign_fn_lowers_to_callblock() {
    // Lower `gain _ 0.5` and assert the IR contains a CallBlock referencing the
    // `gain` builtin with one signal input and the folded param 0.5.
    use rill_lang::lower::lower_with_cafs;

    let src = r#"
        foreign fn gain : FixedBuffer f32 -> Float -> FixedBuffer f32;
        main = gain _ 0.5;
    "#;
let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog).unwrap();
    let ir = lower_with_cafs(&typed, &rill_lang::builtin::NoSigs, 44100.0, &typed.cafs).unwrap();
    assert!(
        ir.builtins.iter().any(|b| b.name == "gain" && b.signal_ins == 1 && b.params == vec![0.5]),
        "expected gain CallBlock with 1 signal in and folded param 0.5, got {:?}",
        ir.builtins
    );
}

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test ffi foreign_fn_lowers_to_callblock`
Expected: FAIL — `unknown identifier 'gain'` at lowering (inference may pass from Task 4, but lowering has no foreign arm yet).

- [ ] **Step 3: Lower foreign names to `CallBlock`**

`rill-lang/src/lower.rs`, in the `Expr::Apply` arm, before the `self.sigs.builtin_sig(name)` block (line ~3623), add:

```rust
// Foreign (FFI) declaration: walk the language-side signature, fold scalar
// params, consume signal wires, and emit a CallBlock — mirroring the builtin
// path. The factory is resolved at RillProgram::build from the FFI registry.
if let Some(fsig) = self.env.foreign_sigs.get(name.as_str()).cloned() {
    if let Some(sig) = crate::types::ffi::ffi_sig_from_typeexpr(&fsig) {
        let mut param_values = Vec::new();
        let mut param_bindings = Vec::new();
        let mut signal_srcs = Vec::new();
        let mut signal_pos = 0;
        let mut param_pos = 0;
        for p in &sig.params {
            match p {
                crate::types::ffi::FfiParam::Signal => {
                    if signal_pos >= args.len() {
                        return Err(CompileError::Type {
                            msg: format!("missing signal input for `{name}`"),
                            span: *span,
                        });
                    }
                    signal_srcs.push(args[signal_pos]);
                    signal_pos += 1;
                }
                crate::types::ffi::FfiParam::VariadicSignal => {
                    for &reg in &args[signal_pos..] {
                        signal_srcs.push(reg);
                    }
                    signal_pos = args.len();
                }
                crate::types::ffi::FfiParam::Scalar => {
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
                    let v = self.caf_const(&call_args[param_pos]).ok_or_else(|| {
                        CompileError::Type {
                            msg: format!(
                                "param at position {param_pos} of `{name}` must be a constant or parameter reference"
                            ),
                            span: call_args[param_pos].span(),
                        }
                    })?;
                    param_values.push(v);
                    param_pos += 1;
                }
                crate::types::ffi::FfiParam::Record(_) => {
                    return Err(CompileError::Type {
                        msg: format!("foreign `{name}` has a record parameter (SP-3b)"),
                        span: *span,
                    });
                }
            }
        }
        let instance = self.builtins.len();
        self.builtins.push(BuiltinInstance {
            name: name.clone(),
            params: param_values,
            resource: None,
            kind: crate::builtin::BuiltinKind::Block,
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
}
```

Also add a `Def::Foreign { .. }` passthrough arm to any exhaustive `Def` match the compiler flags in `lower.rs` (e.g. def-table registration loops) — follow the `Def::Local`/`Def::Anchor` handling pattern; a foreign decl carries no body, so it is skipped like `Def::Data`/`Def::Typeclass`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-lang --test ffi foreign_fn_lowers_to_callblock`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/lower.rs rill-lang/tests/ffi.rs
git commit -m 'feat(rill-lang): lower foreign fn calls to CallBlock via FfiSig'
```

---

## Task 6: `rill_lang::ffi::ForeignRegistry` + runtime wiring + E2E

**Problem.** A foreign call must run. Add a factory registry in rill-lang and resolve it in `RillProgram::build` when the name is not in the existing rill-core `Registry<T>`.

**Files:** `ffi.rs` (new), `program.rs`, `lib.rs`, `tests/ffi.rs`

- [ ] **Step 1: Write the failing test**

Append to `rill-lang/tests/ffi.rs`:

```rust
#[test]
fn foreign_fn_runs_end_to_end() {
    use rill_lang::ffi::ForeignRegistry;
    use rill_core::builtin::BlockBuiltin;
    use rill_core::traits::Algorithm;

    // A trivial gain builtin implemented in the test.
    struct Gain(f64);
    impl<T: rill_core::math::Transcendental> Algorithm<T> for Gain {
        fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> rill_core::ProcessResult<()> {
            let x = input.unwrap_or(&[]);
            for (o, &i) in output.iter_mut().zip(x.iter()) {
                *o = i * T::from_f64(self.0);
            }
            Ok(())
        }
    }
    impl<T: rill_core::math::Transcendental> BlockBuiltin<T> for Gain {}

    let mut ffi = ForeignRegistry::<f32>::new();
    ffi.register_block("gain", |params: &[f64], _sr: f32| {
        Box::new(Gain(params.first().copied().unwrap_or(1.0)))
    });

    let src = r#"
        foreign fn gain : FixedBuffer f32 -> Float -> FixedBuffer f32;
        main = gain _ 0.5;
    "#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0, 2.0, 3.0, 4.0]], &mut [&mut out]).unwrap();
    assert_eq!(out, [0.5, 1.0, 1.5, 2.0]);
}
```

(Check `rill_core::traits::Algorithm::process` signature at `rill-core/src/traits/algorithm.rs:117` — if `init` must be called before `process`, call `Algorithm::init(&mut prog, 44100.0)` via the `RillProgram` `BlockBuiltin` impl or note that `compile_with_ffi` handles it. The exact `Algorithm` import path is `rill_core::traits::Algorithm`.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test ffi foreign_fn_runs_end_to_end`
Expected: FAIL — `rill_lang::ffi` does not exist / `compile_with_ffi` undefined.

- [ ] **Step 3: Create `rill_lang::ffi::ForeignRegistry<T>`**

Create `rill-lang/src/ffi.rs`:

```rust
//! FFI factory registry: name → Rust implementation factory for foreign
//! builtins declared in the language (`foreign fn name : TypeExpr;`).
//!
//! The language owns the signature; this registry owns the implementation. A
//! foreign call's `Instr::CallBlock` is resolved here at `RillProgram::build`
//! when the name is not in the legacy rill-core `Registry<T>`.

use std::collections::HashMap;

use rill_core::builtin::{BlockBuiltin, MultichannelBlockBuiltin};
use rill_core::math::Transcendental;

type BlockFactory<T> =
    Box<dyn Fn(&[f64], f32) -> Box<dyn BlockBuiltin<T>> + Send + Sync>;
type MultichannelBlockFactory<T> =
    Box<dyn Fn(usize, &[f64], f32) -> Box<dyn MultichannelBlockBuiltin<T>> + Send + Sync>;

enum Factory<T: Transcendental> {
    Block(BlockFactory<T>),
    MultichannelBlock(MultichannelBlockFactory<T>),
}

/// A registry of Rust implementations for foreign-declared builtins.
pub struct ForeignRegistry<T: Transcendental> {
    entries: HashMap<String, Factory<T>>,
}

impl<T: Transcendental> Default for ForeignRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental> ForeignRegistry<T> {
    /// An empty registry.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Register a single-channel (1→1) block builtin.
    pub fn register_block(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(&[f64], f32) -> Box<dyn BlockBuiltin<T>> + Send + Sync + 'static,
    ) {
        self.entries
            .insert(name.into(), Factory::Block(Box::new(factory)));
    }

    /// Register a multi-channel block builtin.
    pub fn register_multichannel_block(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(usize, &[f64], f32) -> Box<dyn MultichannelBlockBuiltin<T>>
            + Send
            + Sync
            + 'static,
    ) {
        self.entries.insert(
            name.into(),
            Factory::MultichannelBlock(Box::new(factory)),
        );
    }

    /// Build an instance for `name`, if registered.
    pub(crate) fn build_block(
        &self,
        name: &str,
        signal_ins: usize,
        params: &[f64],
        sample_rate: f32,
    ) -> Option<Box<dyn MultichannelBlockBuiltin<T>>> {
        match self.entries.get(name)? {
            Factory::Block(f) => Some(BlockBuiltinAsMultichannel(f(params, sample_rate))),
            Factory::MultichannelBlock(f) => Some(f(signal_ins, params, sample_rate)),
        }
    }
}

/// Adapt a 1→1 `BlockBuiltin` to the multichannel dispatch used by the build
/// path. `RillProgram::build` selects the multichannel variant when
/// `signal_ins > 1 || signal_outs > 1`; a SISO block wraps cheaply.
struct BlockBuiltinAsMultichannel<T>(Box<dyn BlockBuiltin<T>>);

impl<T: Transcendental> rill_core::traits::MultichannelAlgorithm<T>
    for BlockBuiltinAsMultichannel<T>
{
    fn num_inputs(&self) -> usize {
        1
    }
    fn num_outputs(&self) -> usize {
        1
    }
    fn process(
        &mut self,
        inputs: &[&[T]],
        outputs: &mut [&mut [T]],
    ) -> rill_core::ProcessResult<()> {
        rill_core::traits::Algorithm::process(
            self.0.as_mut(),
            inputs.first().copied(),
            outputs
                .first_mut()
                .map(|o| &mut **o)
                .unwrap_or(&mut []),
        )
    }
}

impl<T: Transcendental> MultichannelBlockBuiltin<T> for BlockBuiltinAsMultichannel<T> {}
```

Declare the module in `rill-lang/src/lib.rs` (`pub mod ffi;`). If `MultichannelBlockBuiltin`'s supertrait requires `Send + Sync`, add those bounds to the adapter struct; check the trait definition at `rill-core/src/builtin.rs:20`.

**Note:** if the `MultichannelAlgorithm` trait's `process` signature differs (see `rill-core/src/traits/multichannel_algorithm.rs:20`), match it exactly. If the existing `RillProgram::build` path only uses `BlockBuiltin` for the SISO branch (see `program.rs:278`), then `build_block` should return the right box per `is_multi` instead of wrapping — in that case add a `build_siso_block` that returns `Box<dyn BlockBuiltin<T>>` and have `RillProgram::build` branch on `is_multi` exactly as it does for the legacy registry.

- [ ] **Step 4: Wire `RillProgram::build` foreign fallback**

`rill-lang/src/program.rs`. `RillProgram::build` (line ~236) currently takes `resources: Option<&mut ResourceRegistry<T>>`. Add a parameter `foreign: Option<&crate::ffi::ForeignRegistry<T>>` and, in the per-builtin loop, when `registry.get(&bi.name)` returns `None`, resolve from `foreign`:

```rust
let entry = registry.get(&bi.name).or_else(|| {
    // FFI: the factory is registered in rill-lang's ForeignRegistry, not the
    // legacy rill-core Registry. The signature came from the language.
    None // placeholder — resolution handled below
});
```

Replace the `entry`-based build with a two-path build:

```rust
let is_multi = bi.signal_ins > 1 || bi.signal_outs > 1;
let built: Result<BuiltinInst<T>, CompileError> = if let Some(entry) = registry.get(&bi.name) {
    // Legacy path (unchanged): build from the rill-core registry.
    if is_multi {
        let mut b = entry
            .build_multichannel_block(bi.signal_ins, &bi.params, sample_rate)
            .or_else(|| {
                entry.build_resource_multichannel_block(
                    bi.signal_ins, &bi.params, sample_rate,
                    resources.as_deref_mut()?, &bi.resource.as_deref()?,
                )
            })
            .ok_or_else(|| CompileError::Unsupported(format!("failed to build '{}'", bi.name)))?;
        MultichannelAlgorithm::reset(b.as_mut());
        Ok(BuiltinInst::MultichannelBlock(b))
    } else {
        let mut b = entry
            .build_block(&bi.params, sample_rate)
            .or_else(|| {
                entry.build_resource_block(
                    &bi.params, sample_rate, resources.as_deref_mut()?,
                    &bi.resource.as_deref()?,
                )
            })
            .ok_or_else(|| CompileError::Unsupported(format!("failed to build '{}'", bi.name)))?;
        Algorithm::init(b.as_mut(), sample_rate);
        Ok(BuiltinInst::Block(b))
    }
} else if let Some(ffi) = foreign {
    // FFI path: factory from the language-side registry.
    if let Some(mut b) = ffi.build_block(&bi.name, bi.signal_ins, &bi.params, sample_rate) {
        MultichannelAlgorithm::reset(b.as_mut());
        Ok(BuiltinInst::MultichannelBlock(b))
    } else {
        Err(CompileError::Unsupported(format!(
            "foreign builtin '{}' is not registered",
            bi.name
        )))
    }
} else {
    Err(CompileError::Unsupported(format!(
        "unknown built-in '{}'",
        bi.name
    )))
};
let mut b = built?;
```

**Caution:** the existing `build` body (`program.rs:242-299`) interleaves resource handling and the two branches. Preserve the exact current behavior for legacy names (including resource-backed tape heads) — only ADD the foreign fallback. Thread `foreign` through `new_with_resources`/`new`/`build` call sites in `program.rs`.

- [ ] **Step 5: Wire `compile_*` entry points**

`rill-lang/src/lib.rs`. `compile_program_inner` (line ~119) builds `RillProgram::new_with_resources(ir, registry, sample_rate, res)`. Add a foreign registry parameter and a public entry:

```rust
/// Compile source against a foreign registry. Foreign (`foreign fn`) builtins
/// resolve their Rust implementations here; legacy builtins still use `registry`.
pub fn compile_with_ffi<T: Transcendental, const BUF: usize>(
    src: &str,
    ffi: &crate::ffi::ForeignRegistry<T>,
    sample_rate: f32,
) -> Result<program_engine::ProgramEngine<T, BUF>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens, src.as_bytes())?;
    let registry = Registry::<T>::new();
    compile_program_inner(program, &registry, sample_rate, None, Some(ffi))
}
```

Thread `foreign: Option<&ForeignRegistry<T>>` through `compile_program_inner` → `RillProgram::new_with_resources` → `build`. Keep the existing entry points passing `None`.

- [ ] **Step 6: Run test to verify it passes + regression**

Run: `cargo test -p rill-lang --test ffi foreign_fn_runs_end_to_end`
Expected: PASS (out == `[0.5, 1.0, 1.5, 2.0]`).

Run: `cargo test -p rill-lang` (full — legacy builtin + tape + typeclass suites must be untouched) and `cargo clippy --all-features -p rill-lang` (zero warnings), `cargo fmt --check`.

- [ ] **Step 7: Commit**

```bash
git add rill-lang/src/ffi.rs rill-lang/src/program.rs rill-lang/src/lib.rs rill-lang/tests/ffi.rs
git commit -m 'feat(rill-lang): FFI registry + RillProgram foreign fallback — foreign fn runs'
```

---

## Task 7: Docs (SP-3a surface)

**Files:** `rill-lang/README.md`

- [ ] **Step 1: Document**

Add a section to `rill-lang/README.md` (English, matching the existing category-typeclasses section): `foreign fn name : TypeExpr;` syntax, the `FixedBuffer`/`Buffer` types in `SIGNAL_PRELUDE`, the FFI registry (`rill_lang::ffi` + `compile_with_ffi`), and the reserved names `foreign`/`FixedBuffer`/`Buffer`. Note that SP-3b migrates the builtin catalog and removes `BuiltinSig`.

- [ ] **Step 2: Verify**

Run: `cargo test -p rill-lang && cargo test --workspace && cargo clippy --all-features --workspace && cargo fmt`
Expected: zero failures, zero warnings.

- [ ] **Step 3: Commit**

```bash
git add rill-lang/README.md
git commit -m 'docs(rill-lang): FFI layer — foreign fn, FixedBuffer/Buffer, registry'
```

---

## Self-review

**Spec coverage (SP-3a from `docs/superpowers/specs/2026-10-02-rill-lang-signal-buffer-design.md`):**
- §1 FFI layer: `foreign fn` syntax (T2), inference from the declared signature (T4), lowering to `CallBlock` (T5) ✓
- §2 `FixedBuffer a`/`Buffer` in `SIGNAL_PRELUDE` (T1) ✓
- §3 FFI registry as runtime API of rill-lang (T6 `rill_lang::ffi`) ✓ — note: `BuiltinSig` removal is SP-3b, not here
- §4 parameter mapping: `FixedBuffer` → Signal (T3/T4/T5), scalars (T3/T4/T5), `List (FixedBuffer f32)` → VariadicSignal (T3/T4/T5); `Data`-record params explicitly deferred to SP-3b (T4/T5 return a clear error) ✓
- §5 tape stays outside FFI — unchanged, legacy path preserved (T6 keeps the resource path) ✓
- §6 long-term goal — SP-3a is the first step; documented in T7 ✓

**Placeholder scan:** all steps have concrete code; the two "Caution"/"Note" callouts in T6 flag adapter-shape decisions to match at implementation time (the exact `MultichannelAlgorithm::process` signature and the `is_multi` branch) — these are verification points, not TODOs.

**Type consistency:** `FfiSig { params: Vec<FfiParam>, signal_outs }` is used identically in infer (T4) and lower (T5). `FfiParam::{Signal, Scalar, VariadicSignal, Record}` names match across T3-T5. `ForeignRegistry::register_block`/`register_multichannel_block`/`build_block` names are consistent between T6's test and implementation. `Def::Foreign { name, sig, span }` is consistent across T2 (AST/parser), T3 (registration), T4/T5 (resolution), T6 (runtime).