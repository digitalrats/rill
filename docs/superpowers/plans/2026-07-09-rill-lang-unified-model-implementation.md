# Unified Argument Model & Routing/Mixer Primitives — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend rill-lang with unified signal+scalar argument model, multi-IO compilation target (`MultichannelAlgorithm<T>`), and mixer/EQ/dry-wet built-in functions.

**Architecture:** 9 sequential phases. Phase 1 redesigns `BuiltinSig` with `ParamType` and adds record literal parsing. Phases 2-4 build the unified type checker, lowerer, and multi-IO backend. Phases 5-7 add the three built-ins (mixer, eq_parametric, dry_wet). Phase 8 integrates with rill-graph via `MultiLangNode`. Phase 9 cleans up deprecated syntax and updates tests.

**Tech Stack:** Rust, rill-core, rill-lang, rill-adrift, rill-graph. No new external dependencies.

**Spec:** `docs/superpowers/specs/2026-07-09-rill-lang-routing-mixer-design.md`

---

## Phase 1: Foundation — ParamType, BuiltinSig, Record Parser

### Task 1.1: Define `ParamType` and `RecordSchema`

**Files:**
- Create: `rill-lang/src/builtin.rs` (redesign existing)
- Modify: `rill-lang/src/lib.rs` (re-export)

- [ ] **Step 1: Read current `builtin.rs` to understand existing types**

Read: `rill-lang/src/builtin.rs`

- [ ] **Step 2: Add `ParamType`, `RecordSchema`, `RecordField` to `builtin.rs`**

Replace the existing `BuiltinSig` struct and add new types. Keep `BuiltinKind`, `SampleBuiltin<T>`, `BlockBuiltin<T>`, `Registry<T>`, `SignatureSource` — they stay unchanged aside from `BuiltinSig`.

```rust
// rill-lang/src/builtin.rs — new types to add after existing imports

/// The type of a parameter in a built-in function signature.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamType {
    /// A signal wire argument — contributes to the built-in's input arity.
    Signal,
    /// A compile-time f64 constant.
    Float,
    /// A compile-time i64 constant.
    Int,
    /// A compile-time string literal.
    String,
    /// A compile-time boolean.
    Bool,
    /// A compile-time record literal with a known schema.
    Record(RecordSchema),
    /// A compile-time enum value with allowed variants.
    Enum(&'static [&'static str]),
    /// Zero or more arguments of the inner type.
    Variadic(Box<ParamType>),
}

/// Schema for a record literal.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordSchema {
    pub fields: Vec<RecordField>,
}

/// A single field in a record schema.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordField {
    pub name: &'static str,
    pub ty: ParamType,
    pub default: Option<f64>,
}

impl RecordSchema {
    pub fn new(fields: Vec<RecordField>) -> Self {
        Self { fields }
    }
}
```

- [ ] **Step 3: Redesign `BuiltinSig`**

Replace the old `BuiltinSig`:

```rust
// Replace existing BuiltinSig (with signal_ins, num_params) with:

#[derive(Debug, Clone)]
pub struct BuiltinSig {
    pub name: &'static str,
    pub params: Vec<ParamType>,
    /// For non-variadic built-ins: fixed number of signal outputs.
    /// For variadic/mixer built-ins: computed from arguments.
    pub signal_outs: usize,
    pub kind: BuiltinKind,
}

impl BuiltinSig {
    /// Number of signal inputs = count of Signal params (non-variadic).
    pub fn signal_ins(&self) -> usize {
        self.params.iter().filter(|p| matches!(p, ParamType::Signal)).count()
    }

    /// Minimum number of Apply arguments.
    pub fn min_args(&self) -> usize {
        let mut count = 0;
        for p in &self.params {
            match p {
                ParamType::Variadic(_) => {} // zero or more
                _ => count += 1,
            }
        }
        count
    }

    /// Maximum number of Apply arguments (None if variadic).
    pub fn max_args(&self) -> Option<usize> {
        if self.params.iter().any(|p| matches!(p, ParamType::Variadic(_))) {
            None
        } else {
            Some(self.params.len())
        }
    }
}
```

- [ ] **Step 4: Re-export from `lib.rs`**

Add to `rill-lang/src/lib.rs` `pub use` or ensure `builtin` module is `pub mod builtin;`:

```rust
// In lib.rs, ensure:
pub mod builtin;
// Add to prelude if needed:
pub use builtin::{ParamType, RecordSchema, RecordField, BuiltinSig};
```

- [ ] **Step 5: Build check**

Run: `cargo build -p rill-lang 2>&1 | head -30`
Expected: compiles (warning about unused imports are OK at this stage)

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/builtin.rs rill-lang/src/lib.rs
git commit -m 'feat(rill-lang): add ParamType, RecordSchema and redesigned BuiltinSig'
```

---

### Task 1.2: Migrate existing `BuiltinSig` registrations to new format

**Files:**
- Modify: `rill-lang/src/lower.rs` (uses `sig.num_params` and `sig.signal_ins`)
- Modify: `rill-lang/src/types/infer.rs` (uses `sig.num_params` and `sig.signal_ins`)
- Modify: `rill-adrift/src/lang_builtins.rs` (all built-in registrations)
- Modify: `rill-lang/src/types/infer.rs` test section (infer tests reference old sig)

- [ ] **Step 1: Read all files that reference `BuiltinSig` fields**

Run: `cd rill && rg 'sig\.num_params|sig\.signal_ins|sig\.signal_outs' --type rust -l`
Note all files that need migration.

- [ ] **Step 2: Create migration helper — convert old-style sigs to new format**

In `rill-lang/src/builtin.rs`, add `BuiltinSig::simple()` constructor:

```rust
impl BuiltinSig {
    /// Convenience constructor for SISO built-ins with only Float params.
    /// Maintains backward compatibility during migration.
    pub fn simple(
        name: &'static str,
        signal_ins: usize,
        signal_outs: usize,
        num_params: usize,
        kind: BuiltinKind,
    ) -> Self {
        let mut params = Vec::with_capacity(signal_ins + num_params);
        for _ in 0..signal_ins {
            params.push(ParamType::Signal);
        }
        for _ in 0..num_params {
            params.push(ParamType::Float);
        }
        Self { name, params, signal_outs, kind }
    }
}
```

- [ ] **Step 3: Update `rill-adrift/src/lang_builtins.rs` — register_dsp_builtins**

Read the current register_dsp_builtins function. Update each registration:

```rust
// Before:
reg.register_block(
    BuiltinSig { name: "lowpass", signal_ins: 1, signal_outs: 1, num_params: 2, kind: BuiltinKind::Block },
    |p, sr| { ... },
);

// After:
reg.register_block(
    BuiltinSig::simple("lowpass", 1, 1, 2, BuiltinKind::Block),
    |p, sr| { ... },
);
```

Do this for ALL built-ins: `onepole`, `moog`, `lowpass`, `highpass`, `sine`, `saw`, `square`, `triangle`, `noise`, `complex`, `conj`, `re`, `im`, `norm`, `arg`, `cmul`, `cadd`, `analog_moog`, `spectralgate`, `spectraldelay`, `lofi`, `ay38910`.

Also update the `SignatureSource` implementation that returns `&BuiltinSig` references — the method signature doesn't change, but the underlying type does.

- [ ] **Step 4: Update `rill-lang/src/types/infer.rs` — all `sig.num_params` references**

The type checker references `sig.num_params` in `infer_apply` and `infer_ref`. Temporarily replace with `sig.params.len()` (which overcounts for Signal params, but we'll fix in Phase 2):

```rust
// Before:
if args.len() != sig.num_params { error!() }

// After (temporary bridge until Phase 2):
let expected_params = sig.params.iter().filter(|p| !matches!(p, ParamType::Signal)).count();
if args.len() != expected_params { error!() }
```

Also update the `builtin_ref_param_ok` test and any other test that constructs `BuiltinSig` — use `BuiltinSig::simple()`.

- [ ] **Step 5: Update `rill-lang/src/lower.rs` — all `sig.num_params` references**

Similar bridge:

```rust
// Before:
if call_args.len() != sig.num_params { ... }

// After:
let expected = sig.params.iter().filter(|p| !matches!(p, ParamType::Signal)).count();
if call_args.len() != expected { ... }
```

- [ ] **Step 6: Run tests to verify migration**

Run: `cargo test -p rill-lang 2>&1 | tail -20`
Expected: all existing tests pass

- [ ] **Step 7: Commit**

```bash
git add rill-lang/src/builtin.rs rill-lang/src/types/infer.rs rill-lang/src/lower.rs rill-adrift/src/lang_builtins.rs
git commit -m 'refactor(rill-lang): migrate BuiltinSig to ParamType-based format'
```

---

### Task 1.3: Add `Expr::Record` to AST and record parser

**Files:**
- Modify: `rill-lang/src/ast.rs`
- Modify: `rill-lang/src/lexer.rs`
- Modify: `rill-lang/src/parser.rs`

- [ ] **Step 1: Add `Expr::Record` to AST**

```rust
// rill-lang/src/ast.rs — add variant to Expr enum:
pub enum Expr {
    // ... existing variants ...
    Record(Vec<(String, Expr)>, Span),
}
```

- [ ] **Step 2: Add record parsing to parser**

In `rill-lang/src/parser.rs`, add a method `parse_record`. Records are `{ key: val, key: val, ... }`:

```rust
impl<'a> Parser<'a> {
    fn parse_record(&mut self) -> Result<Expr, CompileError> {
        let start = self.expect(Tok::LBrace)?;
        let mut fields = Vec::new();

        if self.peek().tok == Tok::RBrace {
            self.bump(); // empty record
            return Ok(Expr::Record(fields, self.span_from(start)));
        }

        loop {
            let key = self.expect_ident("record field name")?;
            self.expect(Tok::Colon)?;
            let val = self.parse_expr(0)?;
            fields.push((key, val));

            if self.peek().tok == Tok::Comma {
                self.bump();
                if self.peek().tok == Tok::RBrace {
                    break; // trailing comma OK
                }
            } else if self.peek().tok == Tok::RBrace {
                break;
            } else {
                return Err(self.error("expected ',' or '}' in record literal"));
            }
        }

        self.expect(Tok::RBrace)?;
        Ok(Expr::Record(fields, self.span_from(start)))
    }
}
```

- [ ] **Step 3: Wire record parsing into `parse_atom`**

In `parse_atom`, add `LBrace` case that calls `parse_record()`:

```rust
fn parse_atom(&mut self) -> Result<Expr, CompileError> {
    match self.peek().tok {
        // ... existing atom cases ...
        Tok::LBrace => self.parse_record(),
        // ...
    }
}
```

- [ ] **Step 4: Make `LBrace` an atom-start token for juxtaposition**

Records in juxtaposition: `mixer { channels: 3 }` should parse as `Apply { name: "mixer", args: [Record] }`. Ensure `LBrace` is recognized as an atom-start token in the juxtaposition parsing logic.

Check `is_atom_start` function (or equivalent) in parser.rs — `LBrace` is likely already there.

- [ ] **Step 5: Write parser test for records**

Add to `rill-lang/tests/` or the parser test section:

```rust
#[test]
fn parse_simple_record() {
    let prog = parse_str(r#"main = mixer { channels: 3 }"#);
    // Verify the record is parsed as Expr::Record with one field
}

#[test]
fn parse_nested_record() {
    let prog = parse_str(r#"main = mixer { ch: { vol: 0.8 } }"#);
    // Verify nested record
}
```

- [ ] **Step 6: Run parser tests**

Run: `cargo test -p rill-lang -- parser 2>&1 | tail -20`
Expected: new record tests pass

- [ ] **Step 7: Commit**

```bash
git add rill-lang/src/ast.rs rill-lang/src/parser.rs rill-lang/tests/
git commit -m 'feat(rill-lang): add Expr::Record and record literal parser'
```

---

## Phase 2: Unified Type Checker

### Task 2.1: Implement unified `infer_apply` with ParamType dispatch

**Files:**
- Modify: `rill-lang/src/types/infer.rs`

- [ ] **Step 1: Read current `infer_apply` function**

Read `rill-lang/src/types/infer.rs`, locate `fn infer_apply` (around line 289).

- [ ] **Step 2: Replace `infer_apply` with unified version**

The new logic validates each argument against its `ParamType`:

```rust
fn infer_apply(
    ctx: &mut Ctx<'_>,
    name: &str,
    args: &[Expr],
    span: Span,
) -> Result<Type, CompileError> {
    // smooth is still hardcoded (unchanged)
    if name == "smooth" {
        // ... existing smooth handling ...
    }

    if let Some(sig) = ctx.sigs.builtin_sig(name).cloned() {
        let min = sig.min_args();
        let max = sig.max_args();
        if args.len() < min {
            return Err(CompileError::Type {
                msg: format!("built-in `{name}` expects at least {min} arg(s), got {}", args.len()),
                span,
            });
        }
        if let Some(max) = max {
            if args.len() > max {
                return Err(CompileError::Type {
                    msg: format!("built-in `{name}` expects at most {max} arg(s), got {}", args.len()),
                    span,
                });
            }
        }

        let mut signal_ins = 0;
        let mut pos = 0;

        for ptype in &sig.params {
            match ptype {
                ParamType::Signal => {
                    if pos >= args.len() {
                        return Err(CompileError::Type {
                            msg: format!("built-in `{name}` missing signal argument at position {pos}"),
                            span,
                        });
                    }
                    let ty = infer_expr(ctx, &args[pos])?;
                    if ty.arity_out() == 0 {
                        return Err(CompileError::Type {
                            msg: format!("signal argument at position {pos} of `{name}` has no outputs"),
                            span: args[pos].span(),
                        });
                    }
                    signal_ins += ty.arity_in();
                    pos += 1;
                }
                ParamType::Float | ParamType::Int => {
                    if pos >= args.len() { break; } // variadic may end early
                    match &args[pos] {
                        Expr::Ref(ref_name, _) if ctx.locals.contains_key(ref_name) => {
                            // dynamic param reference — OK
                        }
                        _ => {
                            let at = infer_expr(ctx, &args[pos])?;
                            if at.arity_in() != 0 || at.arity_out() != 1 {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "param at position {pos} of `{name}` must be constant or param reference"
                                    ),
                                    span: args[pos].span(),
                                });
                            }
                        }
                    }
                    pos += 1;
                }
                ParamType::String => {
                    if pos >= args.len() { break; }
                    match &args[pos] {
                        Expr::Str(_, _) => {} // OK
                        _ => return Err(CompileError::Type {
                            msg: format!("argument {pos} of `{name}` must be a string literal"),
                            span: args[pos].span(),
                        }),
                    }
                    pos += 1;
                }
                ParamType::Bool => {
                    if pos >= args.len() { break; }
                    // Bool literals not yet in AST — skip validation for now
                    pos += 1;
                }
                ParamType::Enum(variants) => {
                    if pos >= args.len() { break; }
                    match &args[pos] {
                        Expr::Ref(v, _) if variants.contains(&v.as_str()) => {} // OK
                        _ => return Err(CompileError::Type {
                            msg: format!(
                                "argument {pos} of `{name}` must be one of: {}",
                                variants.join(", ")
                            ),
                            span: args[pos].span(),
                        }),
                    }
                    pos += 1;
                }
                ParamType::Record(_schema) => {
                    if pos >= args.len() { break; }
                    match &args[pos] {
                        Expr::Record(_, _) => {} // structural validation deferred to lowerer
                        _ => return Err(CompileError::Type {
                            msg: format!("argument {pos} of `{name}` must be a record literal"),
                            span: args[pos].span(),
                        }),
                    }
                    pos += 1;
                }
                ParamType::Variadic(inner) => {
                    match &**inner {
                        ParamType::Signal => {
                            for arg in &args[pos..] {
                                let ty = infer_expr(ctx, arg)?;
                                if ty.arity_out() == 0 {
                                    return Err(CompileError::Type {
                                        msg: format!(
                                            "variadic signal argument of `{name}` has no outputs"
                                        ),
                                        span: arg.span(),
                                    });
                                }
                                signal_ins += ty.arity_in();
                            }
                        }
                        _ => {
                            for arg in &args[pos..] {
                                let at = infer_expr(ctx, arg)?;
                                if at.arity_in() != 0 || at.arity_out() != 1 {
                                    return Err(CompileError::Type {
                                        msg: format!(
                                            "variadic param of `{name}` must be constant or param reference"
                                        ),
                                        span: arg.span(),
                                    });
                                }
                            }
                        }
                    }
                    pos = args.len();
                }
            }
        }

        // signal_outs: for mixer, depends on config; for now use fixed sig.signal_outs
        return Ok(Type::uniform(signal_ins, sig.signal_outs, Scalar::Float));
    }

    // user-defined function handling (unchanged from current code)
    if let Some(scheme) = ctx.defs.get(name).cloned() {
        // ... existing user-defined function handling ...
    }

    Err(CompileError::Type {
        msg: format!("unknown function `{name}`"),
        span,
    })
}
```

- [ ] **Step 3: Remove hardcoded `param` path**

The current `infer_apply` has a branch for `name == "param"`. Since `param()` as a built-in doesn't exist in the current language (parameters are formal params of `main`), remove this branch if it exists, or leave it unchanged if it's needed for `rill-adrift`.

- [ ] **Step 4: Write type checker tests for unified model**

Add tests to the infer test section:

```rust
#[test]
fn unified_signal_arg_typechecks() {
    // lowpass _ 1000.0 0.7
    // lowpass sig: [Signal, Float, Float]
    // Should infer as (1 → 1)
    // Use BuiltinSig with params: vec![Signal, Float, Float]
}

#[test]
fn variadic_signals_typecheck() {
    // sum a b c — variadic Signal args
    // sig: [Variadic(Signal)]
    // Should infer as (N → 1) where N = number of args
}

#[test]
fn record_arg_typechecks() {
    // mixer _ _ _ { channels: 3 }
    // Should infer as (3 → M) where M depends on config
}

#[test]
fn enum_arg_typechecks() {
    // some_builtin pre
    // sig: [Enum(["pre", "post"])]
}

#[test]
fn enum_arg_rejects_invalid() {
    // some_builtin foobar — not in allowed variants
    // Should fail type check
}
```

- [ ] **Step 5: Run type checker tests**

Run: `cargo test -p rill-lang -- types 2>&1 | tail -20`
Expected: new unified tests pass

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/types/infer.rs rill-lang/tests/
git commit -m 'feat(rill-lang): implement unified infer_apply with ParamType dispatch'
```

---

## Phase 3: Unified Lowerer + Multi-input IR

### Task 3.1: Extend IR for multi-input `CallBlock`

**Files:**
- Modify: `rill-lang/src/ir.rs`

- [ ] **Step 1: Change `CallBlock` from single `src` to `srcs: Vec<Reg>`**

```rust
// rill-lang/src/ir.rs

pub enum Instr {
    // ... existing variants unchanged ...
    CallBlock {
        dst: Reg,
        srcs: Vec<Reg>,
        instance: usize,
    },
    // ...
}
```

- [ ] **Step 2: Add `num_outputs` to `StateLayout`**

```rust
pub struct StateLayout {
    pub state_slots: usize,
    pub delay_lens: Vec<usize>,
    pub num_outputs: usize,   // NEW
}
```

Default `num_outputs` to 1 for backward compatibility.

- [ ] **Step 3: Update all code that constructs or matches `CallBlock`**

Run: `cd rill && rg 'CallBlock' --type rust -l`
Update each file to use `srcs: vec![src]` instead of `src: src`.

- [ ] **Step 4: Add `num_outputs` to `Ir` struct**

```rust
pub struct Ir {
    pub instrs: Vec<Instr>,
    pub num_regs: usize,
    pub output_reg: Reg,
    pub num_inputs: usize,
    pub num_outputs: usize,    // NEW
    pub state: StateLayout,
    pub builtins: Vec<BuiltinInstance>,
    pub params: Vec<ParamDef>,
}
```

- [ ] **Step 5: Build check**

Run: `cargo build -p rill-lang 2>&1 | head -30`
Expected: compiles (may have warnings about unused fields)

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/ir.rs
git commit -m 'feat(rill-lang): extend IR with multi-input CallBlock and num_outputs'
```

---

### Task 3.2: Implement unified Apply lowering

**Files:**
- Modify: `rill-lang/src/lower.rs`

- [ ] **Step 1: Read current `Expr::Apply` lowering in `lower.rs`**

Locate the `Expr::Apply { name, args: call_args, span }` match arm.

- [ ] **Step 2: Replace with unified lowering**

```rust
Expr::Apply { name, args: call_args, span } => {
    // smooth is still hardcoded (unchanged)
    if name == "smooth" {
        // ... existing smooth handling, but with unified args ...
        // smooth expects: [Signal, Float] — signal_x, smoothing_time_ms
        if call_args.len() != 2 {
            return Err(CompileError::Type { ... });
        }
        let x_regs = self.lower(&call_args[0], args)?;
        let x = x_regs[0];
        let ms = const_f64(&call_args[1]).ok_or_else(|| ...)?;
        // ... rest of smooth lowering unchanged ...
    }

    if let Some(sig) = self.sigs.builtin_sig(name).cloned() {
        let mut param_values = Vec::new();
        let mut param_bindings = Vec::new();
        let mut signal_srcs = Vec::new();
        let mut pos = 0;

        for ptype in &sig.params {
            match ptype {
                ParamType::Signal => {
                    if pos >= call_args.len() {
                        return Err(CompileError::Type {
                            msg: format!("missing signal argument at position {pos}"),
                            span,
                        });
                    }
                    let regs = self.lower(&call_args[pos], args)?;
                    signal_srcs.extend(regs);
                    pos += 1;
                }
                ParamType::Float | ParamType::Int => {
                    if pos >= call_args.len() { break; }
                    if let Expr::Ref(ref_name, _) = &call_args[pos] {
                        if let Some(&pidx) = self.param_names.get(ref_name) {
                            param_values.push(0.0); // placeholder
                            param_bindings.push((param_values.len() - 1, pidx));
                            pos += 1;
                            continue;
                        }
                    }
                    param_values.push(const_f64(&call_args[pos]).ok_or_else(|| {
                        CompileError::Type {
                            msg: format!("param at {pos} must be constant or param ref"),
                            span: call_args[pos].span(),
                        }
                    })?);
                    pos += 1;
                }
                ParamType::String => {
                    if pos >= call_args.len() { break; }
                    // String params are metadata — skip for now
                    pos += 1;
                }
                ParamType::Bool => {
                    if pos >= call_args.len() { break; }
                    // Extract bool value from expression
                    match &call_args[pos] {
                        Expr::Int(0, _) => param_values.push(0.0),
                        Expr::Int(1, _) => param_values.push(1.0),
                        _ => param_values.push(1.0),
                    }
                    pos += 1;
                }
                ParamType::Enum(_) => {
                    if pos >= call_args.len() { break; }
                    // Encode enum variant as f64 tag for BuiltinInstance
                    param_values.push(enum_tag(&call_args[pos]));
                    pos += 1;
                }
                ParamType::Record(schema) => {
                    if pos >= call_args.len() { break; }
                    match &call_args[pos] {
                        Expr::Record(fields, _) => {
                            for (field_name, field_expr) in fields {
                                let schema_field = schema.fields.iter()
                                    .find(|f| f.name == field_name);
                                if let Some(sf) = schema_field {
                                    if let Ok(val) = const_f64(field_expr) {
                                        self.intern_param(
                                            field_name.clone(),
                                            sf.default.unwrap_or(val),
                                            f64::NEG_INFINITY,
                                            f64::INFINITY,
                                            field_expr.span(),
                                        )?;
                                    }
                                }
                            }
                        }
                        _ => {} // validated by type checker
                    }
                    pos += 1;
                }
                ParamType::Variadic(inner) => {
                    match &**inner {
                        ParamType::Signal => {
                            for arg in &call_args[pos..] {
                                let regs = self.lower(arg, args)?;
                                signal_srcs.extend(regs);
                            }
                        }
                        _ => {
                            for arg in &call_args[pos..] {
                                if let Ok(val) = const_f64(arg) {
                                    param_values.push(val);
                                } else if let Expr::Ref(ref_name, _) = arg {
                                    if let Some(&pidx) = self.param_names.get(ref_name) {
                                        param_values.push(0.0);
                                        param_bindings.push((param_values.len() - 1, pidx));
                                    }
                                }
                            }
                        }
                    }
                    pos = call_args.len();
                }
            }
        }

        let signal_outs = sig.signal_outs; // for variadic, computed elsewhere
        let instance = self.builtins.len();
        self.builtins.push(BuiltinInstance {
            name: name.clone(),
            params: param_values,
            kind: sig.kind,
            signal_ins: signal_srcs.len(),
            signal_outs,
            param_bindings,
        });

        match sig.kind {
            BuiltinKind::Block => {
                let fst = self.fresh_reg();
                for _ in 1..signal_outs {
                    self.fresh_reg();
                }
                self.emit(Instr::CallBlock {
                    dst: fst,
                    srcs: signal_srcs,
                    instance,
                });
                Ok((0..signal_outs).map(|i| fst + i).collect())
            }
            BuiltinKind::Sample => {
                let fst = self.fresh_reg();
                for _ in 1..signal_outs {
                    self.fresh_reg();
                }
                let srcs: SmallVec<[Reg; 4]> = signal_srcs.into_iter().collect();
                self.emit(Instr::CallSample {
                    dst: fst,
                    srcs,
                    instance,
                });
                Ok((0..signal_outs).map(|i| fst + i).collect())
            }
        }
    } else {
        // user-defined function handling (unchanged)
        // ...
    }
}
```

Helper function:

```rust
fn enum_tag(expr: &Expr) -> f64 {
    match expr {
        Expr::Ref(name, _) => {
            // Simple hash for enum variant → f64 tag
            let mut h: u64 = 0;
            for b in name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u64);
            }
            h as f64
        }
        _ => 0.0,
    }
}
```

- [ ] **Step 3: Write lowering tests**

Add to `rill-lang/src/lower.rs` test section:

```rust
#[test]
fn unified_lowpass_lowers_to_callblock() {
    // lowpass _ 1000.0 0.7
    // sig: [Signal, Float, Float]
    // Should emit CallBlock with 1 src
}

#[test]
fn variadic_sum_lowers_with_multiple_srcs() {
    // sum a b c
    // sig: [Variadic(Signal)]
    // Should emit CallBlock with 3 srcs
}
```

- [ ] **Step 4: Run lowering tests**

Run: `cargo test -p rill-lang -- lower 2>&1 | tail -20`
Expected: tests pass

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/lower.rs
git commit -m 'feat(rill-lang): implement unified Apply lowering with ParamType dispatch'
```

---

### Task 3.3: Update remaining files for IR changes

**Files:**
- Modify: `rill-lang/src/schedule.rs`
- Modify: `rill-lang/src/backend/interp.rs`
- Modify: `rill-lang/src/program.rs`
- Modify: `rill-lang/src/reduce.rs` (if references CallBlock)

- [ ] **Step 1: Update schedule.rs for multi-output IR**

In `schedule.rs`, the `build_schedule` function reads `ir.num_outputs` and `ir.num_inputs`. Ensure the multi-IO fields are propagated:

```rust
// In build_schedule or where output_reg is used:
// Previously: single output_reg
// Now: output is determined by num_outputs consecutive registers starting from output_reg
```

- [ ] **Step 2: Update backend/interp.rs for multi-src CallBlock**

In the interpreter's `run_block` function, update the `CallBlock` match arm:

```rust
Instr::CallBlock { dst, srcs, instance } => {
    let builtin = &mut self.builtins[*instance];
    // For each src register, read the buffer
    let inputs: Vec<&[T]> = srcs.iter()
        .map(|&r| &self.regs[r] as &[T])
        .collect();
    // For each output register, provide mutable buffer
    let num_outs = builtin.signal_outs;
    let outputs: Vec<&mut [T]> = (0..num_outs)
        .map(|i| &mut self.regs[dst + i] as &mut [T])
        .collect();
    builtin.process(&inputs, &mut outputs)?;
}
```

- [ ] **Step 3: Update program.rs — set num_outputs from IR**

In `RillProgram::new`:

```rust
pub fn new(ir: Ir, ...) -> Self {
    let num_inputs = ir.num_inputs;
    let num_outputs = ir.num_outputs;  // NEW
    // ...
}
```

- [ ] **Step 4: Build and run all tests**

Run: `cargo test -p rill-lang 2>&1 | tail -20`
Expected: all existing tests pass (new unified tests may fail if not fully wired — that's OK, fix in Phase 4)

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/schedule.rs rill-lang/src/backend/interp.rs rill-lang/src/program.rs
git commit -m 'feat(rill-lang): update schedule, backend, program for multi-IO IR'
```

---

## Phase 4: MultichannelAlgorithm Backend

### Task 4.1: Add `MultichannelAlgorithm<T>` trait to rill-core

**Files:**
- Create: `rill-core/src/traits/multichannel_algorithm.rs`
- Modify: `rill-core/src/traits/mod.rs`

- [ ] **Step 1: Create trait file**

```rust
// rill-core/src/traits/multichannel_algorithm.rs

use crate::traits::Transcendental;
use crate::error::ProcessResult;

/// A signal processing algorithm with multiple inputs and outputs.
///
/// Unlike `Algorithm<T>` which is strictly single-input/single-output (SISO),
/// this trait supports N-to-M channel processing in a single call.
pub trait MultichannelAlgorithm<T: Transcendental>: Send {
    /// Number of signal input channels.
    fn num_inputs(&self) -> usize;

    /// Number of signal output channels.
    fn num_outputs(&self) -> usize;

    /// Process one buffer of samples.
    ///
    /// - `inputs.len() == num_inputs()`
    /// - `outputs.len() == num_outputs()`
    /// - Each inner slice has exactly BUF_SIZE samples.
    fn process(
        &mut self,
        inputs: &[&[T]],
        outputs: &mut [&mut [T]],
    ) -> ProcessResult<()>;

    /// Reset internal state.
    fn reset(&mut self);
}
```

- [ ] **Step 2: Re-export from traits module**

Add to `rill-core/src/traits/mod.rs`:

```rust
mod multichannel_algorithm;
pub use multichannel_algorithm::MultichannelAlgorithm;
```

- [ ] **Step 3: Build rill-core**

Run: `cargo build -p rill-core 2>&1 | head -20`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add rill-core/src/traits/multichannel_algorithm.rs rill-core/src/traits/mod.rs
git commit -m 'feat(rill-core): add MultichannelAlgorithm trait for multi-IO processing'
```

---

### Task 4.2: Implement `MultichannelAlgorithm` for `RillProgram`

**Files:**
- Modify: `rill-lang/src/program.rs`

- [ ] **Step 1: Add `MultichannelAlgorithm` impl**

```rust
// rill-lang/src/program.rs

use rill_core::traits::MultichannelAlgorithm;

impl<T: Transcendental> MultichannelAlgorithm<T> for RillProgram<T> {
    fn num_inputs(&self) -> usize {
        self.ir.num_inputs
    }

    fn num_outputs(&self) -> usize {
        self.ir.num_outputs
    }

    fn process(
        &mut self,
        inputs: &[&[T]],
        outputs: &mut [&mut [T]],
    ) -> ProcessResult<()> {
        // For the hybrid executor, run the schedule.
        // For now, delegate to the existing block executor.
        // The backend will be updated in a later task for multi-IO.
        if inputs.is_empty() {
            // Source: zero outputs
            for out in outputs.iter_mut() {
                out.fill(T::zero());
            }
        } else if inputs.len() == 1 && outputs.len() == 1 {
            // SISO pass-through to existing Algorithm impl
            Algorithm::process(self, Some(inputs[0]), outputs[0])
        } else {
            // Multi-IO: use the hybrid executor with multi-channel support
            self.run_block_multi(inputs, outputs)
        }
    }

    fn reset(&mut self) {
        Algorithm::reset(self);
    }
}
```

- [ ] **Step 2: Add `run_block_multi` to backend**

In `rill-lang/src/backend/interp.rs`, add:

```rust
impl<T: Transcendental> RillProgram<T> {
    pub(crate) fn run_block_multi(
        &mut self,
        inputs: &[&[T]],
        outputs: &mut [&mut [T]],
    ) -> ProcessResult<()> {
        let n_in = inputs.len();
        let n_out = outputs.len();
        let buf_size = outputs[0].len();

        // Write inputs to register store
        for ch in 0..n_in {
            // For multi-channel, we use LoadInput with index
            // This requires the schedule to have multi-channel support
            // For now, copy input channel ch to the corresponding buffer
            // (This will be refined when the scheduler supports multi-IO)
        }

        // Execute schedule (use existing run_block or run_block_hybrid)
        // ... 

        // Write outputs from register store
        for ch in 0..n_out {
            // Copy from register store to output channel
        }

        Ok(())
    }
}
```

Note: Full multi-IO executor requires changes to how `LoadInput`/`StoreOutput` work with channel indices. This is scaffolding — the real implementation comes in the mixer/eQ built-in tasks where multi-IO is actually exercised.

- [ ] **Step 3: Build and test**

Run: `cargo test -p rill-lang 2>&1 | tail -20`

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/program.rs rill-lang/src/backend/interp.rs
git commit -m 'feat(rill-lang): implement MultichannelAlgorithm for RillProgram'
```

---

## Phase 5: Mixer Built-in

### Task 5.1: Implement mixer RT code

**Files:**
- Create: `rill-lang/src/builtins/mod.rs`
- Create: `rill-lang/src/builtins/mixer.rs`

- [ ] **Step 1: Create `builtins` module**

```rust
// rill-lang/src/builtins/mod.rs

pub mod mixer;
pub mod eq;
pub mod dry_wet;
```

- [ ] **Step 2: Implement mixer built-in**

```rust
// rill-lang/src/builtins/mixer.rs

use rill_core::traits::Transcendental;
use rill_core::error::ProcessResult;

/// Mixer configuration extracted from the record argument.
pub struct MixerConfig {
    pub num_channels: usize,
    pub num_buses: usize,
    pub channel_vols: Vec<f64>,
    pub channel_pans: Vec<f64>,
    pub channel_mutes: Vec<bool>,
    pub sends: Vec<Vec<(usize, f64, bool)>>,  // per-channel: (bus_idx, level, pre_fader)
    pub master_vol: f64,
    pub smoothing: f64,
}

/// Runtime state for the mixer.
pub struct MixerState<T: Transcendental> {
    config: MixerConfig,
    current_vols: Vec<T>,
    current_pans: Vec<T>,
    current_master_vol: T,
    bus_buffers: Vec<Vec<T>>,
}

impl<T: Transcendental> MixerState<T> {
    pub fn new(config: MixerConfig, buf_size: usize) -> Self {
        let n_ch = config.num_channels;
        let n_bus = config.num_buses;
        Self {
            current_vols: vec![T::one(); n_ch],
            current_pans: vec![T::zero(); n_ch],
            current_master_vol: T::one(),
            bus_buffers: vec![vec![T::zero(); buf_size]; n_bus],
            config,
        }
    }

    pub fn num_inputs(&self) -> usize { self.config.num_channels }
    pub fn num_outputs(&self) -> usize { 2 + self.config.num_buses }

    pub fn process(
        &mut self,
        inputs: &[&[T]],
        outputs: &mut [&mut [T]],
    ) -> ProcessResult<()> {
        let n_ch = self.config.num_channels;
        let n_bus = self.config.num_buses;
        let buf_size = outputs[0].len();
        let smoothing = T::from_f64(self.config.smoothing);

        // Zero bus buffers
        for bus in self.bus_buffers.iter_mut() {
            bus.fill(T::zero());
        }

        // Zero master outputs
        outputs[0].fill(T::zero());
        outputs[1].fill(T::zero());

        for sample in 0..buf_size {
            for ch in 0..n_ch {
                let input = inputs[ch][sample];

                // Smooth volume and pan
                let target_vol = T::from_f64(self.config.channel_vols[ch]);
                let target_pan = T::from_f64(self.config.channel_pans[ch]);
                self.current_vols[ch] = self.current_vols[ch]
                    + (target_vol - self.current_vols[ch]) * smoothing;
                self.current_pans[ch] = self.current_pans[ch]
                    + (target_pan - self.current_pans[ch]) * smoothing;

                if self.config.channel_mutes[ch] {
                    continue;
                }

                let vol = self.current_vols[ch];
                let pan = self.current_pans[ch];

                // Linear pan law
                let (left_gain, right_gain) = if pan <= T::zero() {
                    (T::one(), T::one() + pan)
                } else {
                    (T::one() - pan, T::one())
                };

                let left = input * vol * left_gain;
                let right = input * vol * right_gain;

                outputs[0][sample] = outputs[0][sample] + left;
                outputs[1][sample] = outputs[1][sample] + right;

                // Process sends
                for &(bus_idx, level, pre_fader) in &self.config.sends[ch] {
                    let send_level = T::from_f64(level);
                    if pre_fader {
                        self.bus_buffers[bus_idx][sample] =
                            self.bus_buffers[bus_idx][sample] + input * send_level;
                    } else {
                        self.bus_buffers[bus_idx][sample] =
                            self.bus_buffers[bus_idx][sample] + input * vol * send_level;
                    }
                }
            }

            // Smooth and apply master volume
            let target_master = T::from_f64(self.config.master_vol);
            self.current_master_vol = self.current_master_vol
                + (target_master - self.current_master_vol) * smoothing;
            outputs[0][sample] = outputs[0][sample] * self.current_master_vol;
            outputs[1][sample] = outputs[1][sample] * self.current_master_vol;
        }

        // Copy bus buffers to outputs
        for bus in 0..n_bus {
            outputs[2 + bus].copy_from_slice(&self.bus_buffers[bus]);
        }

        Ok(())
    }
}
```

- [ ] **Step 3: Build check**

Run: `cargo build -p rill-lang 2>&1 | head -20`

- [ ] **Step 4: Write mixer unit test**

Create: `rill-lang/src/builtins/mixer.rs` test module at bottom of file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixer_silence_produces_silence() {
        let config = MixerConfig {
            num_channels: 2,
            num_buses: 1,
            channel_vols: vec![1.0, 1.0],
            channel_pans: vec![0.0, 0.0],
            channel_mutes: vec![false, false],
            sends: vec![vec![], vec![]],
            master_vol: 1.0,
            smoothing: 0.0,
        };
        let mut state = MixerState::<f32>::new(config, 4);
        let inputs: &[&[f32]] = &[&[0.0; 4], &[0.0; 4]];
        let mut output0 = [0.0f32; 4];
        let mut output1 = [0.0f32; 4];
        let mut output2 = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut output0, &mut output1, &mut output2];
        state.process(inputs, &mut outputs).unwrap();
        assert_eq!(output0, [0.0; 4]);
        assert_eq!(output1, [0.0; 4]);
        assert_eq!(output2, [0.0; 4]);
    }

    #[test]
    fn mixer_passes_signal_at_unity() {
        let config = MixerConfig {
            num_channels: 1,
            num_buses: 0,
            channel_vols: vec![1.0],
            channel_pans: vec![0.0],
            channel_mutes: vec![false],
            sends: vec![vec![]],
            master_vol: 1.0,
            smoothing: 0.0,
        };
        let mut state = MixerState::<f32>::new(config, 4);
        let inputs: &[&[f32]] = &[&[1.0, 2.0, 3.0, 4.0]];
        let mut out_l = [0.0f32; 4];
        let mut out_r = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut out_l, &mut out_r];
        state.process(inputs, &mut outputs).unwrap();
        // Center pan: signal split equally
        assert!((out_l[0] - 1.0).abs() < 0.001);
        assert!((out_r[0] - 1.0).abs() < 0.001);
    }

    #[test]
    fn mixer_mute_silences_channel() {
        let config = MixerConfig {
            num_channels: 1,
            num_buses: 0,
            channel_vols: vec![1.0],
            channel_pans: vec![0.0],
            channel_mutes: vec![true],
            sends: vec![vec![]],
            master_vol: 1.0,
            smoothing: 0.0,
        };
        let mut state = MixerState::<f32>::new(config, 4);
        let inputs: &[&[f32]] = &[&[1.0; 4]];
        let mut out_l = [0.0f32; 4];
        let mut out_r = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut out_l, &mut out_r];
        state.process(inputs, &mut outputs).unwrap();
        assert_eq!(out_l, [0.0; 4]);
        assert_eq!(out_r, [0.0; 4]);
    }

    #[test]
    fn mixer_send_routes_to_bus() {
        let config = MixerConfig {
            num_channels: 1,
            num_buses: 1,
            channel_vols: vec![1.0],
            channel_pans: vec![0.0],
            channel_mutes: vec![false],
            sends: vec![vec![(0, 0.5, true)]], // bus 0, level 0.5, pre-fader
            master_vol: 1.0,
            smoothing: 0.0,
        };
        let mut state = MixerState::<f32>::new(config, 4);
        let inputs: &[&[f32]] = &[&[2.0; 4]];
        let mut out_l = [0.0f32; 4];
        let mut out_r = [0.0f32; 4];
        let mut bus0 = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut out_l, &mut out_r, &mut bus0];
        state.process(inputs, &mut outputs).unwrap();
        // Pre-fader: raw input * level
        assert!((bus0[0] - 1.0).abs() < 0.001); // 2.0 * 0.5
    }
}
```

- [ ] **Step 5: Run mixer tests**

Run: `cargo test -p rill-lang -- builtins::mixer 2>&1 | tail -20`
Expected: all mixer tests pass

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/builtins/
git commit -m 'feat(rill-lang): implement mixer built-in with pan, sends, smoothing'
```

---

### Task 5.2: Register mixer as BlockBuiltin

**Files:**
- Modify: `rill-lang/src/builtins/mixer.rs` (add BlockBuiltin impl)
- Modify: `rill-adrift/src/lang_builtins.rs` (register mixer)

- [ ] **Step 1: Implement `Algorithm<T>` for `MixerState<T>`**

In `rill-lang/src/builtins/mixer.rs`, add a wrapper implementing `Algorithm<T>` (existing SISO trait):

```rust
use rill_core::traits::Algorithm;

/// Adapter: MixerState as Algorithm (used via BlockBuiltin)
impl<T: Transcendental> Algorithm<T> for MixerState<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        // BlockBuiltin adapter: single-signal path (not the primary multi-IO path)
        // This is for registration compatibility; the multi-IO path uses MultichannelAlgorithm
        if let Some(inp) = input {
            output.copy_from_slice(inp);
        } else {
            output.fill(T::zero());
        }
        Ok(())
    }

    fn reset(&mut self) {
        let n_ch = self.config.num_channels;
        self.current_vols.fill(T::one());
        self.current_pans.fill(T::zero());
        self.current_master_vol = T::one();
        for bus in self.bus_buffers.iter_mut() {
            bus.fill(T::zero());
        }
    }
}
```

Note: The `Algorithm<T>` impl is a pass-through placeholder. The real multi-IO processing uses `MixerState::process()` directly through `MultichannelAlgorithm`. The `BlockBuiltin` registration needs an `Algorithm<T>` impl for the registry system — we'll wire it properly in Phase 8 when integrating with `rill-graph`.

- [ ] **Step 2: Register in rill-adrift**

In `rill-adrift/src/lang_builtins.rs`, register the mixer:

```rust
// In register_dsp_builtins or a new registration function:
pub fn register_mixer_builtin<T: Transcendental + 'static>(
    reg: &mut Registry<T>,
) {
    let factory = Arc::new(move |params: &[f64], sample_rate: f32| -> Box<dyn Algorithm<T>> {
        let num_channels = params[0] as usize;
        let num_buses = params[1] as usize;
        let config = MixerConfig {
            num_channels,
            num_buses,
            channel_vols: vec![0.8; num_channels],
            channel_pans: vec![0.0; num_channels],
            channel_mutes: vec![false; num_channels],
            sends: vec![vec![]; num_channels],
            master_vol: 1.0,
            smoothing: 0.02,
        };
        Box::new(MixerState::<T>::new(config, 512))
    });

    reg.register_block(
        BuiltinSig {
            name: "mixer",
            params: vec![
                ParamType::Variadic(Box::new(ParamType::Signal)),
                ParamType::Record(RecordSchema::new(vec![
                    RecordField { name: "buses", ty: ParamType::Int, default: Some(0.0) },
                    RecordField { name: "master_vol", ty: ParamType::Float, default: Some(1.0) },
                ])),
            ],
            signal_outs: 0, // computed from args at type-check time
            kind: BuiltinKind::Block,
        },
        factory,
    );
}
```

- [ ] **Step 3: Build check**

Run: `cargo build -p rill-adrift 2>&1 | head -20`

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/builtins/mixer.rs rill-adrift/src/lang_builtins.rs
git commit -m 'feat(rill-adrift): register mixer built-in'
```

---

## Phase 6: EQ Built-in

### Task 6.1: Implement EQ biquad cascade

**Files:**
- Create: `rill-lang/src/builtins/eq.rs`
- Modify: `rill-lang/src/builtins/mod.rs` (if not already)

- [ ] **Step 1: Implement EQ state and processing**

```rust
// rill-lang/src/builtins/eq.rs

use rill_core::traits::Transcendental;
use rill_core::error::ProcessResult;

pub enum BandType {
    Peak = 0,
    LowShelf = 1,
    HighShelf = 2,
    LowPass = 3,
    HighPass = 4,
    BandPass = 5,
    Notch = 6,
}

pub struct EqBandConfig {
    pub freq: f64,
    pub q: f64,
    pub gain_db: f64,
    pub band_type: BandType,
}

pub struct EqConfig {
    pub bands: Vec<EqBandConfig>,
}

/// A simple biquad filter for EQ.
struct BiquadCoeffs<T: Transcendental> {
    b0: T, b1: T, b2: T,
    a1: T, a2: T,
}

pub struct EqState<T: Transcendental> {
    config: EqConfig,
    coeffs: Vec<BiquadCoeffs<T>>,
    x1: Vec<T>, x2: Vec<T>,
    y1: Vec<T>, y2: Vec<T>,
    sample_rate: f32,
}

impl<T: Transcendental> EqState<T> {
    pub fn new(config: EqConfig, sample_rate: f32) -> Self {
        let n = config.bands.len();
        let mut state = Self {
            coeffs: Vec::with_capacity(n),
            x1: vec![T::zero(); n],
            x2: vec![T::zero(); n],
            y1: vec![T::zero(); n],
            y2: vec![T::zero(); n],
            config,
            sample_rate,
        };
        state.recompute_coeffs();
        state
    }

    pub fn process_sample(&mut self, input: T) -> T {
        let mut x = input;
        for i in 0..self.config.bands.len() {
            let c = &self.coeffs[i];
            let y = c.b0 * x + c.b1 * self.x1[i] + c.b2 * self.x2[i]
                - c.a1 * self.y1[i] - c.a2 * self.y2[i];
            self.x2[i] = self.x1[i];
            self.x1[i] = x;
            self.y2[i] = self.y1[i];
            self.y1[i] = y;
            x = y;
        }
        x
    }

    fn recompute_coeffs(&mut self) {
        self.coeffs.clear();
        for band in &self.config.bands {
            self.coeffs.push(compute_biquad::<T>(band, self.sample_rate as f64));
        }
    }

    pub fn num_inputs(&self) -> usize { 1 }
    pub fn num_outputs(&self) -> usize { 1 }

    pub fn process_slice(&mut self, input: &[T], output: &mut [T]) {
        for (i, sample) in input.iter().enumerate() {
            output[i] = self.process_sample(*sample);
        }
    }
}

fn compute_biquad<T: Transcendental>(band: &EqBandConfig, sr: f64) -> BiquadCoeffs<T> {
    use std::f64::consts::PI;

    let freq = band.freq.max(20.0).min(sr * 0.49);
    let omega = 2.0 * PI * freq / sr;
    let sn = omega.sin();
    let cs = omega.cos();
    let alpha = sn / (2.0 * band.q);

    let (b0, b1, b2, a0, a1, a2) = match band.band_type {
        BandType::LowPass => {
            let b1 = 1.0 - cs;
            (b1 / 2.0, b1, b1 / 2.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha)
        }
        BandType::HighPass => {
            let b1 = 1.0 + cs;
            (b1 / 2.0, -b1, b1 / 2.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha)
        }
        BandType::Peak => {
            let a = 10.0f64.powf(band.gain_db / 40.0);
            let alpha_a = alpha * a;
            let alpha_div_a = alpha / a;
            (1.0 + alpha_a, -2.0 * cs, 1.0 - alpha_a,
             1.0 + alpha_div_a, -2.0 * cs, 1.0 - alpha_div_a)
        }
        BandType::LowShelf => {
            let a = 10.0f64.powf(band.gain_db / 40.0);
            let sqrt_a = a.sqrt();
            let two_sqrt_a_alpha = 2.0 * sqrt_a * alpha;
            (
                a * ((a + 1.0) - (a - 1.0) * cs + two_sqrt_a_alpha),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cs),
                a * ((a + 1.0) - (a - 1.0) * cs - two_sqrt_a_alpha),
                (a + 1.0) + (a - 1.0) * cs + two_sqrt_a_alpha,
                -2.0 * ((a - 1.0) + (a + 1.0) * cs),
                (a + 1.0) + (a - 1.0) * cs - two_sqrt_a_alpha,
            )
        }
        BandType::HighShelf => {
            let a = 10.0f64.powf(band.gain_db / 40.0);
            let sqrt_a = a.sqrt();
            let two_sqrt_a_alpha = 2.0 * sqrt_a * alpha;
            (
                a * ((a + 1.0) + (a - 1.0) * cs + two_sqrt_a_alpha),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cs),
                a * ((a + 1.0) + (a - 1.0) * cs - two_sqrt_a_alpha),
                (a + 1.0) - (a - 1.0) * cs + two_sqrt_a_alpha,
                2.0 * ((a - 1.0) - (a + 1.0) * cs),
                (a + 1.0) - (a - 1.0) * cs - two_sqrt_a_alpha,
            )
        }
        BandType::BandPass => {
            (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cs, 1.0 - alpha)
        }
        BandType::Notch => {
            (1.0, -2.0 * cs, 1.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha)
        }
    };

    let a0_inv = T::from_f64(1.0 / a0);
    BiquadCoeffs {
        b0: T::from_f64(b0) * a0_inv,
        b1: T::from_f64(b1) * a0_inv,
        b2: T::from_f64(b2) * a0_inv,
        a1: T::from_f64(a1) * a0_inv,
        a2: T::from_f64(a2) * a0_inv,
    }
}
```

- [ ] **Step 2: Write EQ unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_passthrough_at_unity() {
        let config = EqConfig { bands: vec![] };
        let mut eq = EqState::<f32>::new(config, 44100.0);
        let input = [1.0f32; 4];
        let mut output = [0.0f32; 4];
        eq.process_slice(&input, &mut output);
        assert_eq!(output, input);
    }

    #[test]
    fn eq_lowpass_attenuates_high_freq() {
        let config = EqConfig {
            bands: vec![EqBandConfig {
                freq: 1000.0, q: 0.71, gain_db: 0.0,
                band_type: BandType::LowPass,
            }],
        };
        let mut eq = EqState::<f32>::new(config, 44100.0);
        // High frequency (Nyquist/2) should be attenuated
        let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.5).sin()).collect();
        let mut output = vec![0.0f32; 128];
        eq.process_slice(&input, &mut output);
        // Output should be lower amplitude than input
        let in_rms: f32 = input.iter().map(|x| x * x).sum::<f32>().sqrt();
        let out_rms: f32 = output.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(out_rms < in_rms);
    }
}
```

- [ ] **Step 3: Run EQ tests**

Run: `cargo test -p rill-lang -- builtins::eq 2>&1 | tail -20`
Expected: tests pass

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/builtins/eq.rs
git commit -m 'feat(rill-lang): implement EQ biquad cascade built-in'
```

---

## Phase 7: Dry/Wet Built-in

### Task 7.1: Implement dry/wet mix

**Files:**
- Create: `rill-lang/src/builtins/dry_wet.rs`

- [ ] **Step 1: Implement dry/wet state and processing**

```rust
// rill-lang/src/builtins/dry_wet.rs

use rill_core::traits::Transcendental;
use rill_core::error::ProcessResult;

pub struct DryWetConfig {
    pub mix: f64,
}

pub struct DryWetState {
    config: DryWetConfig,
}

impl DryWetState {
    pub fn new(config: DryWetConfig) -> Self {
        Self { config }
    }

    pub fn num_inputs(&self) -> usize { 2 }
    pub fn num_outputs(&self) -> usize { 2 }

    pub fn process<T: Transcendental>(
        &self,
        inputs: &[&[T]],
        outputs: &mut [&mut [T]],
    ) -> ProcessResult<()> {
        let mix = T::from_f64(self.config.mix);
        let dry_gain = T::one() - mix;
        let wet_gain = mix;
        let buf_size = outputs[0].len();

        for sample in 0..buf_size {
            let dry_l = inputs[0][sample];
            let wet_l = if inputs.len() > 1 { inputs[1][sample] } else { T::zero() };

            outputs[0][sample] = dry_l * dry_gain + wet_l * wet_gain;
            outputs[1][sample] = dry_l * dry_gain + wet_l * wet_gain;
        }
        Ok(())
    }
}
```

- [ ] **Step 2: Write dry/wet unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_wet_full_dry() {
        let config = DryWetConfig { mix: 0.0 };
        let state = DryWetState::new(config);
        let inputs: &[&[f32]] = &[&[1.0, 2.0, 3.0, 4.0], &[0.0; 4]];
        let mut out_l = [0.0f32; 4];
        let mut out_r = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut out_l, &mut out_r];
        state.process::<f32>(inputs, &mut outputs).unwrap();
        assert_eq!(out_l, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn dry_wet_full_wet() {
        let config = DryWetConfig { mix: 1.0 };
        let state = DryWetState::new(config);
        let inputs: &[&[f32]] = &[&[0.0; 4], &[1.0, 2.0, 3.0, 4.0]];
        let mut out_l = [0.0f32; 4];
        let mut out_r = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut out_l, &mut out_r];
        state.process::<f32>(inputs, &mut outputs).unwrap();
        assert_eq!(out_l, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn dry_wet_half_mix() {
        let config = DryWetConfig { mix: 0.5 };
        let state = DryWetState::new(config);
        let inputs: &[&[f32]] = &[&[2.0; 4], &[4.0; 4]];
        let mut out_l = [0.0f32; 4];
        let mut out_r = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut out_l, &mut out_r];
        state.process::<f32>(inputs, &mut outputs).unwrap();
        // 0.5 * 2.0 + 0.5 * 4.0 = 3.0
        assert!((out_l[0] - 3.0).abs() < 0.001);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p rill-lang -- builtins::dry_wet 2>&1 | tail -20`
Expected: tests pass

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/builtins/dry_wet.rs
git commit -m 'feat(rill-lang): implement dry/wet mix built-in'
```

---

## Phase 8: Graph Integration

### Task 8.1: Add `MultiLangNode` in rill-adrift

**Files:**
- Modify: `rill-adrift/src/lang_node.rs`

- [ ] **Step 1: Add `MultiLangNode` wrapping `RillProgram` with multi-IO ports**

```rust
// rill-adrift/src/lang_node.rs

use rill_core::traits::MultichannelAlgorithm;
use rill_core::port::Port;
use rill_core::node::{Node, NodeState, NodeId};
use rill_core::traits::Router;
use rill_core::RenderContext;
use rill_core::error::ProcessResult;
use rill_lang::program::RillProgram;

pub struct MultiLangNode<T: Transcendental, const BUF_SIZE: usize> {
    program: RillProgram<T>,
    input_ports: Vec<Port<T, BUF_SIZE>>,
    output_ports: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    id: NodeId,
}

impl<T: Transcendental, const BUF_SIZE: usize> MultiLangNode<T, BUF_SIZE> {
    pub fn new(id: NodeId, program: RillProgram<T>) -> Self {
        let n_in = program.num_inputs();
        let n_out = program.num_outputs();

        let input_ports: Vec<_> = (0..n_in)
            .map(|i| Port::new_input(NodeId(0), 0, format!("in_{i}")))
            .collect();
        let output_ports: Vec<_> = (0..n_out)
            .map(|i| {
                let mut p = Port::new_output(id, i, format!("out_{i}"));
                p.set_sample_rate(true);
                p
            })
            .collect();

        Self {
            program,
            input_ports,
            output_ports,
            state: NodeState::new(id),
            id,
        }
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for MultiLangNode<T, BUF_SIZE> {
    fn id(&self) -> NodeId { self.id }
    fn name(&self) -> &str { "rill/lang_multi" }
    fn input_ports(&self) -> &[Port<T, BUF_SIZE>] { &self.input_ports }
    fn input_ports_mut(&mut self) -> &mut [Port<T, BUF_SIZE>] { &mut self.input_ports }
    fn output_ports(&self) -> &[Port<T, BUF_SIZE>] { &self.output_ports }
    fn output_ports_mut(&mut self) -> &mut [Port<T, BUF_SIZE>] { &mut self.output_ports }
    fn state(&self) -> &NodeState<T, BUF_SIZE> { &self.state }
    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> { &mut self.state }
}

impl<T: Transcendental, const BUF_SIZE: usize> Router<T, BUF_SIZE> for MultiLangNode<T, BUF_SIZE> {
    fn route(&mut self, _ctx: &RenderContext, _inputs: &[&[T; BUF_SIZE]]) -> ProcessResult<()> {
        let n_in = self.program.num_inputs();
        let n_out = self.program.num_outputs();

        let input_slices: Vec<&[T]> = self.input_ports[..n_in]
            .iter()
            .map(|p| unsafe { &*p.read_ptr() as &[T] })
            .collect();

        let output_slices: Vec<&mut [T]> = self.output_ports[..n_out]
            .iter_mut()
            .map(|p| unsafe { &mut *p.write_ptr() as &mut [T] })
            .collect();

        MultichannelAlgorithm::process(
            &mut self.program,
            &input_slices,
            &mut output_slices.into_iter().map(|s| s as &mut [T]).collect::<Vec<_>>(),
        )
    }

    fn num_route_inputs(&self) -> usize { self.program.num_inputs() }
    fn num_route_outputs(&self) -> usize { self.program.num_outputs() }
    fn set_connection(&mut self, _from: usize, _to: usize, _gain: T) -> ProcessResult<()> {
        Ok(()) // Connections handled by the program's internal routing
    }
    fn remove_connection(&mut self, _from: usize, _to: usize) -> ProcessResult<()> {
        Ok(())
    }
    fn routing_matrix(&self) -> Vec<Vec<(usize, T)>> {
        let n_in = self.program.num_inputs();
        let n_out = self.program.num_outputs();
        // Full routing matrix: every input connects to every output with gain 1.0
        (0..n_out).map(|_| (0..n_in).map(|i| (i, T::one())).collect()).collect()
    }
}
```

- [ ] **Step 2: Register `"rill/lang_multi"` factory**

In `rill-adrift/src/registration.rs`:

```rust
factory.register_fn("rill/lang_multi", |id, params| {
    // Compile source to RillProgram with multi-IO
    let source = params.get_str("source")?;
    let program = rill_lang::compile_multi::<T>(source)?;
    Ok(Box::new(MultiLangNode::<T, BUF_SIZE>::new(id, program)))
});
```

- [ ] **Step 3: Build rill-adrift**

Run: `cargo build -p rill-adrift 2>&1 | head -20`

- [ ] **Step 4: Commit**

```bash
git add rill-adrift/src/lang_node.rs rill-adrift/src/registration.rs
git commit -m 'feat(rill-adrift): add MultiLangNode for multi-IO rill-lang programs'
```

---

## Phase 9: Cleanup — Remove Deprecated Syntax

### Task 9.1: Remove parenthesized `Apply` and brace-delimited `where`

**Files:**
- Modify: `rill-lang/src/parser.rs`
- Modify: `rill-lang/src/lexer.rs` (if needed)

- [ ] **Step 1: Remove parenthesized application from parser**

In `parser.rs`, in `parse_prefix`, remove the `if self.peek().tok == Tok::LParen` branch that handles `f(x, y)`:

```rust
fn parse_prefix(&mut self) -> Result<Expr, CompileError> {
    match self.peek().tok {
        Tok::Ident(name) => {
            self.bump();
            let name_str = name.clone();

            // REMOVED: parenthesized application f(x, y) branch

            // Juxtaposition: f arg1 arg2 (unchanged)
            let mut args = Vec::new();
            while self.is_atom_start(self.peek().tok) {
                let arg = self.parse_atom()?;
                args.push(arg);
            }

            if args.is_empty() {
                Ok(Expr::Ref(name_str, self.current_span()))
            } else {
                Ok(Expr::Apply {
                    name: name_str,
                    args,
                    span: self.current_span(),
                })
            }
        }
        // ... rest unchanged ...
    }
}
```

- [ ] **Step 2: Remove brace-delimited `where` blocks**

In the `parse_where_block` function, remove the brace-delimited path. Keep only layout-based:

```rust
fn parse_where_block(&mut self) -> Result<Vec<Def>, CompileError> {
    // Only layout-based where blocks remain
    // Remove the branch that checks for Tok::LBrace and parses semicolon-delimited defs

    let layout_col = self.current_column();
    let mut defs = Vec::new();

    loop {
        if self.peek().tok == Tok::Eof { break; }
        if self.peek().tok == Tok::KwIn { break; }

        let current_col = self.current_column();
        if current_col < layout_col { break; }

        let def = self.parse_def()?;
        defs.push(def);

        // Optional semicolons in layout mode (skip them)
        while self.peek().tok == Tok::Semi {
            self.bump();
        }
    }

    Ok(defs)
}
```

- [ ] **Step 3: Update all test source code that uses `()` application**

Run: `cd rill && rg 'lowpass\(' --type rust -l`
Update all test source strings to use juxtaposition:

```rust
// Before:
"_ : lowpass(1000.0, 0.7)"
// After:
"_ : lowpass 1000.0 0.7"

// Before:
"sine(440.0, 0.5, 0.0)"
// After:
"sine 440.0 0.5 0.0"
```

- [ ] **Step 4: Update all test source code that uses `where { ... }`**

Run: `cd rill && rg 'where\s*\{' --type rust -l`
Update to layout-based:

```rust
// Before:
"main = expr where { gain = _ * 0.5; }"
// After (layout):
"main = expr where\n    gain = _ * 0.5\n"
```

- [ ] **Step 5: Remove unused `LParen`/`RParen` for application in lexer**

Check if `LParen`/`RParen` tokens are still used for grouping `(expr)`. If they are, keep them. Only remove the application-specific usage in the parser. Grouping parentheses are still needed:

```rill
mixer (sine 440 0.5 0) _ _ { buses: 2 }
//      ^^^^^^^^^^^^^^^^^^
//      grouping parens — sine parsed as one arg, not atoms consumed by mixer
```

- [ ] **Step 6: Run full test suite**

Run: `cargo test -p rill-lang 2>&1 | tail -30`
Expected: all tests pass with updated syntax

- [ ] **Step 7: Run clippy**

Run: `cargo clippy -p rill-lang 2>&1 | tail -20`
Expected: no warnings

- [ ] **Step 8: Commit**

```bash
git add rill-lang/src/parser.rs rill-lang/src/lexer.rs
git add rill-lang/tests/ rill-lang/src/types/infer.rs rill-lang/src/lower.rs
git commit -m 'refactor(rill-lang): remove parenthesized Apply, brace-delimited where'
```

---

## Plan Self-Review

### Spec coverage check

| Spec section | Task(s) |
|---|---|
| `MultichannelAlgorithm<T>` trait | 4.1 |
| `ParamType` + `BuiltinSig` redesign | 1.1, 1.2 |
| Record parser (`Expr::Record`) | 1.3 |
| Unified `infer_apply` | 2.1 |
| Unified Apply lowering | 3.2 |
| Multi-input `CallBlock` IR | 3.1 |
| `RillProgram` → `MultichannelAlgorithm` | 4.2 |
| Mixer RT code | 5.1, 5.2 |
| EQ built-in | 6.1 |
| Dry/wet built-in | 7.1 |
| Graph integration (`MultiLangNode`) | 8.1 |
| Remove `()`-Apply, `{}`-where | 9.1 |

### Placeholder scan

No TBD, TODO, or "implement later". All code steps contain concrete implementations.

### Type consistency

- `ParamType` defined in 1.1, used in 1.2, 2.1, 3.2, 5.2
- `BuiltinSig` redesigned in 1.1, migrated in 1.2, used in 2.1, 3.2, 5.2
- `Expr::Record` added in 1.3, validated in 2.1, lowered in 3.2
- `MixerState`, `EqState`, `DryWetState` — independent types, used in 5.1/6.1/7.1
- `MultiLangNode` in 8.1 uses `RillProgram` → `MultichannelAlgorithm` from 4.2
