# rill-lang Strictness Annotations (`!`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `!` strictness annotations to rill-lang — bang patterns in all pattern positions plus Haskell 2010 strict data fields — lowering every use to the existing `ValueInstr::ValueForce`, with `!` semantically inert in `MODE strict`.

**Architecture:** One compiler mechanism (`!` → a `ValueForce{ dst, src }` emitted at the binding or construction point of the generated value IR) serves all five positions. The VM already implements `ValueForce` and full thunk forcing; this feature is its first product-code emission. AST threads a strictness bit through `Pattern`, `Param`, `Def::Eval`, data-field shapes (AST `Def::Data`/`Def::Sum` and, critically, `DataInfo`), with lowering placing forces at fragment entry (params), let-bind sites, per-arm match dispatch, and constructor applications.

**Tech Stack:** Rust, `rill-lang` crate (`--features dsp`), no new dependencies. Cargo workspace at `/home/mikek/Projects/digitalrats/rill`.

**Spec:** `docs/superpowers/specs/2026-10-14-rill-lang-strictness-annotations-design.md` (approved 2026-10-14; commit `9391283`).

## Precondition (read before anything else)

This plan is written for the tree **after Task 14 of `docs/superpowers/plans/2026-10-12-rill-lang-haskell2010-unary-core.md`** (dead-code removal) is complete: only `Def::Eval` exists (no `Def::Anchor/Local/Fn`), def parameters live in `Expr::Lambda.params` (currying, Task 6), lowering is value-track-only. Line anchors below are taken from the pre-migration tree; **function and type names are the stable contract** — re-grep for anchors if they moved.

## Global Constraints

- Verbatim from the spec: `not` remains logical negation and is untouched; `!=` (`Tok::NotEq`) is unaffected; `!` in bare expression position stays a parse error; `!` is **not a type** (no `ValueTy`/`TypeExpr` change from strictness).
- Only positions in scope: def/λ-parameter, `case`/`match` arm pattern (whole-pattern `!(Just v)` and payload `Just !v`), `data` strict field (product and sum), `let !x = e` (expression and `do`). `newtype` strictness is a parse error (Haskell 2010: newtype cannot have strict fields).
- Strict mode (`MODE strict`) creates no thunks — `!` must be a behavioral no-op there.
- `cargo build -p rill-lang --features dsp` clean, `cargo test -p rill-lang --features dsp` green, zero warnings under `cargo clippy -p rill-lang --features dsp --tests`, `cargo fmt` (max_width=100, tab_spaces=4) at the end of every task.
- Commit messages in single quotes, conventional commits (`feat(rill-lang): …`). Do not touch `rill-lang/src/arrow.rs` or `graph/reconstruct.rs` (legacy prose/docs referencing the removed `Expr::Cut`).

## Review Focus

The five inputs a reasonable user would expect to work that no single task's tests exercise alone:

1. **A bang arm must never enter the static match fast path** — it must route to runtime dispatch and force per-arm; otherwise `statically_lowerable`'s `_ => true` silently miscompiles. → Task 6.
2. **`case bottom of { _ => 5 }` must still be 5** — forcing happens only when a bang arm is reached, never at match top, never by a wildcard arm. → Task 6.
3. **Strict `data` fields must actually force at construction** in complete mode — driven by `DataInfo`, which the ctor lowering reads, not the AST defs. → Task 7.
4. **`MODE strict` + `!` is behaviorally identical to no `!`** anywhere in the program. → Task 9.
5. **`!=` still lexes `NotEq` and a lone `!` in expression position is a parse error** — no accidental operator resurrection. → Tasks 1, 3.

---

### Task 1: Lexer — `Tok::Cut` → `Tok::Bang`

**Files:**
- Modify: `rill-lang/src/lexer.rs:19` (enum), `:467` (emitter), `:510` (test `lexes_combinators_and_ops`)

**Interfaces:**
- Produces: `Tok::Bang` replaces `Tok::Cut`. `Tok::NotEq` (lexed first, `:295`) unchanged.

- [ ] **Step 1: Rename the token**

Rename `Cut` → `Bang` in the `Tok` enum (doc comment: strictness marker for bang patterns and strict data fields) and the emitter `b'!' => Tok::Cut` → `Tok::Bang`. In `lexes_combinators_and_ops` (`lexer.rs:510`), the `Tok::Cut` expectation becomes `Tok::Bang`.

- [ ] **Step 2: Verify the rename is complete**

Run: `rg -n 'Tok::Cut' rill-lang/src` — expected: hits only in `arrow.rs:23` (prose table) and `graph/reconstruct.rs:377` (`E::Cut`, a different legacy enum) — both are out of scope and must remain untouched.

- [ ] **Step 3: Run the lexer tests**

Run: `cargo test -p rill-lang --features dsp --lib lexer` — expected: PASS (including `lexes_combinators_and_ops` and the existing `!=` → NotEq test).

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/lexer.rs
git commit -m 'feat(rill-lang): rename lexer Cut token to Bang'
```

### Task 2: AST + `DataInfo` — strictness nodes and invariant plumbing

**Files:**
- Modify: `rill-lang/src/ast.rs` — `Pattern` (`:106`), `Param` (`:319`), `Def::Eval` (`:342`+), `Def::Data` (`:373`), `Def::Sum` (`:384`)
- Modify: `rill-lang/src/types/ty.rs:139` — `DataInfo`
- Modify (compile-guided plumbing): `rill-lang/src/parser.rs`, `rill-lang/src/types/infer.rs`, `rill-lang/src/lower.rs`, `rill-lang/src/render.rs`, `rill-lang/src/reduce.rs`, test modules in each — every construction/destructuring site gains the new fields
- Test: `rill-lang/src/ast.rs` — add `#[cfg(test)] mod tests` (none exists today)

**Interfaces:**
- Produces (exact shapes, the contract for all later tasks):
  - `Pattern::Bang(Box<Pattern>)`
  - `Param { name: String, span: Span, strict: bool }`
  - `Def::Eval { name, params, body, where_defs, sig, span, strict: bool }` — `strict` means "this let-binding forces its RHS" (params live in `Expr::Lambda` post-Task 6, so `Def::Eval` never carries them for bang purposes)
  - `Def::Data { name, tyvars, fields: Vec<(String, TypeExpr, bool)>, span }`
  - `Def::Sum { name, tyvars, ctors: Vec<(String, Vec<(TypeExpr, bool)>)>, span }`
  - `DataInfo::Record(Vec<(String, ValueTy, bool)>)`, `DataInfo::Sum(Vec<(String, Vec<(ValueTy, bool)>)>)`
- Consumes: `Tok::Bang` (Task 1).

- [ ] **Step 1: Write the failing serde test**

In `rill-lang/src/ast.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "serde")]
    #[test]
    fn strictness_ast_serde_roundtrips() {
        let p: Pattern = Pattern::Bang(Box::new(Pattern::Var("x".into())));
        let defs = vec![Def::Eval {
            name: "f".into(),
            params: vec![Param { name: "x".into(), span: Span::new(1, 2), strict: true }],
            body: Box::new(Expr::Ref("x".into(), Span::new(3, 4))),
            where_defs: vec![],
            sig: None,
            span: Span::new(0, 4),
            strict: false,
        }];
        let _ = (&p, &defs);
        let json_p = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Pattern>(&json_p).unwrap(), p);
    }
}
```

(If `Def::Eval` gains extra fields during the migration, match the migration's final shape; the point is that every new strictness bit round-trips through `serde_json`.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p rill-lang --features dsp,serde --lib ast` — expected: FAIL (does not compile: `Pattern::Bang`, `Param.strict`, `Def::Eval.strict` do not exist).

- [ ] **Step 3: Add the AST nodes**

Add `Pattern::Bang(Box<Pattern>)`; add `strict: bool` to `Param` and `Def::Eval`; add a strictness bool per field/payload to `Def::Data.fields` and `Def::Sum.ctors` as in Interfaces.

- [ ] **Step 4: Plumb through `DataInfo`**

Change `DataInfo::Record(Vec<(String, ValueTy)>)` → `Vec<(String, ValueTy, bool)>` and `DataInfo::Sum(Vec<(String, Vec<ValueTy>)>)` → `Vec<(String, Vec<(ValueTy, bool)>)>` in `types/ty.rs`. Update the builders in `types/infer.rs` (`Record` at `:472`, `Sum` at `:526`/`:540`/`:1048-1060` — map the AST strictness bits, temporarily `false` until Task 4) and the consumers in `lower.rs` (`:1054`, `:1065`, `sum_ctor` `:4541`). Update `DataInfo` constructions in test code (`infer.rs:1354`, `:1358`).

- [ ] **Step 5: Compile-guided plumbing**

Fix every remaining construction/destructuring site the compiler flags: `Param { name, span }` literals become `Param { name, span, strict: false }` (parser.rs `:418`, `:740`, `:1008`, `:1021`, `:1127`, `:1156`; render.rs tests `:566`, `:592`); `Def::Eval { .. }` gains `strict: false`; `Def::Data`/`Def::Sum` builders in `parse_data_def` gain `false` bits; every exhaustive `Pattern::` match gains a `Pattern::Bang(p)` arm. Where the semantics of a site is not yet decided by later tasks, route `Pattern::Bang(p)` exactly like `p` **and only as an interim identity** — the parser cannot produce a `Bang` node yet, so no behavior is reachable; Tasks 5–7 redefine the real semantics.

- [ ] **Step 6: Run the test and the full suite**

Run: `cargo test -p rill-lang --features dsp,serde --lib ast` — expected: PASS. Then `cargo test -p rill-lang --features dsp` — expected: all green (behavior unchanged; nothing can produce a `Bang` yet).

- [ ] **Step 7: Clippy, fmt, commit**

Run: `cargo clippy -p rill-lang --features dsp --tests` (zero warnings), `cargo fmt`.
```bash
git add -A
git commit -m 'feat(rill-lang): add strictness AST nodes and DataInfo bits'
```

### Task 3: Parser — `!` in all five positions

**Files:**
- Modify: `rill-lang/src/parser.rs` — `is_pattern_start` (`:158`), `parse_pattern` (`:245`), `parse_top_def` param loop (`:408-419`), λ params (`:997-1014`), `parse_def` / `parse_where_block` (`:779`+) for `let`/where blocks, `parse_do_block` (`:1096`-`:1151`), `parse_data_def` (`:459`)
- Test: `rill-lang/src/parser.rs` (test helpers `prog(src)` and `body(src)`)

**Interfaces:**
- Consumes: `Tok::Bang`, the AST nodes from Task 2 (including `Def::Eval.strict` and the `Def::Data`/`Def::Sum` strictness bits), `Pattern::Bang`.
- Produces: parse trees in which every `!` position sets the corresponding strictness bit and one can match AST shapes in tests. `!` anywhere in expression/type-atom position still yields `CompileError::Parse`.

- [ ] **Step 1: Add `Top::Bang` to pattern and data parsing**

Add `Tok::Bang` to `is_pattern_start`. In `parse_pattern`, on `Tok::Bang`: bump, recurse `parse_pattern()`, return `Pattern::Bang(Box::new(inner))` — the ctor-arg recursion (`:284`) gives `Just !v` for free. In `parse_data_def`, accept an optional leading `Tok::Bang` before each product field type and before each sum payload type, and set the corresponding AST strictness bit.

- [ ] **Step 2: Add `!` to parameter and let parsing**

In the `parse_top_def` param loop and the λ-param parser (both currently `while let Tok::Ident(_)` and an `expect_ident`), peek for `Tok::Bang` first and set `Param { strict: true, .. }`. In `parse_def` (shared by expr-`let`/where blocks), accept a leading `Tok::Bang` before the binding name → `Def::Eval { strict: true, .. }`. In `parse_do_block`'s `let` statement, accept `let !x = e` and carry the strict bit into the created binding def.

- [ ] **Step 3: Write the parser tests**

```rust
#[test]
fn parse_bang_def_param() {
    // `step !x = x * 0.5` — top-level def param carries strict
    let p = prog("step !x = x * 0.5; main = step 5.0;");
    // assert the step def's Eval/Lambda param has strict == true
}

#[test]
fn parse_bang_lambda_param() {
    let b = body("main = (\\ !x -> x) 1.0;");
    // assert b is Apply whose lambda arg param has strict == true
}

#[test]
fn parse_bang_match_patterns() {
    // assert `main = case v of { !x => x; };` parses arm pattern Pattern::Bang(_)
    // and `main = case v of { Just !x => x; };` parses Ctor("Just", [Bang(Var "x")])
    // and `main = case v of { !(Just v) => v; };` parses Bang(Ctor("Just", [Var "v"]))
}

#[test]
fn parse_strict_data_fields() {
    // `data T = C !Int | D Float;` -> ctors = [("C", [(Int, true)]), ("D", [(Float, false)])]
    // `data P = P { cutoff :: !Float };` -> fields = [("cutoff", (Float, true))]
}

#[test]
fn parse_bang_let_bindings() {
    // expression let: `main = let { !x = 1.0 } in x;` -> Expr::Let def has strict == true
    // do-let: `main = do { let !x = 1.0; x; };` -> strict bit carried
}

#[test]
fn bang_wrong_positions_are_parse_errors() {
    for src in ["main = !5;", "newtype Hz = Hz !Float;"] {
        assert!(parse(&tokenize(src).unwrap(), src.as_bytes()).is_err());
    }
}
```

(Points, not transcripts: assert the exact AST shapes using the existing `prog`/`body` helpers and `matches!`.)

- [ ] **Step 4: Run to verify the new tests fail**

Run: `cargo test -p rill-lang --features dsp --lib parser::tests::parse_bang` — expected: FAIL (no `!` handling yet).

- [ ] **Step 5: Implement the parser changes**

One shared helper is the cleanest shape for the two optional-`!` probes (param and let-def) — e.g. `fn eat_bang(&mut self) -> bool`. Apply it in the param loops, the def-name position of `parse_def`, the do-`let`, and `parse_data_def`. Keep `!` a parse error everywhere else (no new prefix handling in `parse_prefix`/`parse_atom` — the existing "expected a pattern / expected definition name, found Bang" errors already fire).

- [ ] **Step 6: Run the parser tests**

Run: `cargo test -p rill-lang --features dsp --lib parser` — expected: PASS (new + roundtrip-neutral existing tests). Verify Review Focus #5: `cargo test -p rill-lang --features dsp --lib lexer::tests` still shows `!=` → NotEq.

- [ ] **Step 7: Full suite, clippy, fmt, commit**

Run: `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp --tests`, `cargo fmt`.
```bash
git add rill-lang/src/parser.rs
git commit -m 'feat(rill-lang): parse strictness annotations'
```

### Task 4: Inference — bang through pattern typing and `DataInfo`

**Files:**
- Modify: `rill-lang/src/types/infer.rs` — pattern typing/coverage sites (`:1761`, `:1860`, `:1897`, `:1937`, guarded-arm analysis `:1581`, ctor-pinning `:2335`/`:2347`), `DataInfo` builders carrying real bits (from Task 2's `false` placeholders)

**Interfaces:**
- Consumes: AST bits (Task 2), parsed trees (Task 3).
- Produces: `DataInfo` with real strictness bits; `Pattern::Bang` typed as its inner pattern everywhere; bang arms counted in coverage exactly like their inner patterns.

- [ ] **Step 1: Write the failing inference tests**

```rust
#[test]
fn bang_pattern_infers_inner_type() {
    // `f !x = x;` infers `f : Float -> Float` — Bang(Var x) types as x's type
}

#[test]
fn bang_does_not_change_data_info_types() {
    // `data T = C !Float;` -> DataInfo::Sum([("C", [(Float, true)])]) — type Float, strict true
    // `data T = C Float;`  -> strict == false
}
```

(Match the numeric/`Float` default expected types used by the existing `infer.rs` tests, which run under the strict-mode `Float` baseline.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp --lib types::infer::tests::bang` — expected: FAIL.

- [ ] **Step 3: Implement**

In every arm-pattern analysis that matches on `Pattern`, make `Pattern::Bang(p)` recurse into `p` (typing, coverage `has_wild`, ctor-pinning, `arm_result_vty`). In the `Def::Data`/`Def::Sum` handling at `:1026`/`:1045`/`:1048-1060`, thread the AST strictness bits into the `DataInfo` tuples (replacing the Task 2 placeholders).

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rill-lang --features dsp --lib types` — expected: PASS.

- [ ] **Step 5: Full suite, clippy, fmt, commit**

```bash
git commit -am 'feat(rill-lang): type bang patterns as their inner pattern'
```

### Task 5: Lowering — force on strict params and strict `let`

**Files:**
- Modify: `rill-lang/src/lower.rs` — `lower_fragment` (`:3313`), `Expr::Let` complete branch (`:2339`-`:2420`)
- Test: `rill-lang/src/lower.rs` (helpers `ir_of`/`ir_with_cafs`/`has_instr`)

**Interfaces:**
- Consumes: `Param.strict`, `Def::Eval.strict` (Task 2), `ValueInstr::ValueForce` (exists, `ir.rs:505`).
- Produces: in complete mode, a strict param's fragment emits `ValueForce` as one of its first instructions with the param's register as `src`; a strict `let` emits `ValueForce` immediately after the bind. In strict mode, no `ValueForce` is emitted for either.

- [ ] **Step 1: Write the failing lowering tests**

```rust
#[test]
fn bang_param_forces_at_fragment_entry() {
    let ir = ir_of("{-# MODE complete #-}\nstep !x = x * 0.5; main = step 5.0;");
    // fragment 0 (step's body λ) must contain ValueForce with src == the param reg (0)
    assert!(has_instr(ir.fragment(0), ValueInstr::ValueForce { .. }));
}

#[test]
fn strict_let_forces_at_bind_site() {
    let ir = ir_of("{-# MODE complete #-}\nmain = let { !x = 1.0 } in x;");
    // a ValueForce appears and its src feeds from the ValueLazy-produced register
}

#[test]
fn bang_is_plain_in_strict_mode() {
    // ir_of("step !x = x * 0.5; main = step 5.0;") — strict baseline:
    // NO ValueForce anywhere; identical instruction multiset to ir_of without the `!`.
}
```

(Match the established test shape — `ir_of` compiles the value-track IR; `has_instr` scans a fragment's sequence.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp --lib lower::tests::bang` — expected: FAIL.

- [ ] **Step 3: Implement**

In `lower_fragment`, where params bind to fragment-local registers `0..params.len()` and the body scope is built (`:3341-3347`), for a strict param allocate a fresh register and emit `ValueForce { dst: fresh, src: i }` immediately after binding, then map the param name to `(fresh, ty)` in the body scope so body refs read the forced head. In `Expr::Let`'s complete branch, after a bound def whose `strict` is set emits its `ValueLazy` thunk, emit `ValueForce` into the register the name maps to. Do nothing in `Mode::Strict` (no thunks → identity; emit no `ValueForce`).

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rill-lang --features dsp --lib lower` — expected: PASS.

- [ ] **Step 5: Full suite, clippy, fmt, commit**

```bash
git commit -am 'feat(rill-lang): lower strict params and strict let to ValueForce'
```

### Task 6: Lowering — bang match arms route to runtime dispatch with per-arm force

**Files:**
- Modify: `rill-lang/src/lower.rs` — static-path classifier (`statically_lowerable` `:817-823`, route decision `:734`/`:761`), runtime arm chain (`lower_static_match_arm` `:4218`, `lower_pattern_test` `:4331`, `lower_match_guards_and_body` `:4479`), payload binding (`let Pattern::Var = p else` `:4291`), arm selection fallbacks (`:4237`, `:4313`)
- Modify: `rill-lang/src/reduce.rs` — `pattern_vars` (`:24`)
- Test: `rill-lang/src/lower.rs`, `rill-lang/src/reduce.rs`

**Interfaces:**
- Consumes: `Pattern::Bang`, `pattern_has_bang` helper (below), `ValueInstr::ValueForce`.
- Produces:
  - `fn pattern_has_bang(p: &Pattern) -> bool` (recursive: true through `Bang` and through a `Ctor`'s args).
  - A match whose **any** arm's pattern has bang is compiled via the runtime sequential dispatch (the same route guarded arms take), never the static path.
  - In the runtime chain, each bang arm forces the scrutinee value register once (fresh `ValueForce` dst) before testing/binding that arm; a payload-position bang in `Just !x` forces the extracted payload register right after extraction.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn bang_arm_routes_to_runtime_dispatch() {
    let ir = ir_of("{-# MODE complete #-}\nmain = case 1.0 of { !x => x; _ => 0.0; };");
    // lowering must produce runtime BranchCtor-style dispatch (see the existing
    // "a guarded match must lower to BranchCtor dispatch" assertion pattern),
    // never the static fast path
}

#[test]
fn bang_arm_forces_scrutinee_per_arm_not_match_top() {
    // `case bottom of { _ => 5; !x => x }` — NO ValueForce before the first
    // (wildcard) arm; a ValueForce appears only at the bang arm. Assert with
    // block-scoped instruction inspection.
}

#[test]
fn payload_bang_forces_after_extraction() {
    // `case c of { Just !x => x; _ => 0.0 }` in complete mode — the payload
    // extraction is followed by ValueForce on the payload register
}

#[test]
fn pattern_vars_collects_through_bang() {
    // pattern_vars(&Pattern::Bang(Box::new(Pattern::Ctor("Just", [Var("x")])))) == ["x"]
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp --lib lower bang` and `cargo test -p rill-lang --features dsp --lib reduce pattern_vars` — expected: FAIL (Bang arms currently either miscompile via the static path or are silently dropped).

- [ ] **Step 3: Implement**

Add `pattern_has_bang`. In `statically_lowerable`, an arm whose pattern has bang returns `false`; the match-level route decision then sends the whole match through the runtime dispatch (add the check alongside the guarded-arm check at `:761`). In the runtime arm chain, for a bang arm emit `ValueForce` on the scrutinee register into a fresh dst before running that arm's `lower_pattern_test`/binding, and use the forced register for the arm. For payload-position bangs, force the payload register after extraction. Update the audit list from the design doc (`:822`, `:4237`, `:4291`, `:4313` in `lower.rs`) so no `_ =>`/`if let` arm silently swallows a `Bang`. In `reduce.rs`, extend `pattern_vars` with a `Pattern::Bang(p) => out.extend(pattern_vars(p))` arm.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rill-lang --features dsp --lib lower` then `cargo test -p rill-lang --features dsp --lib reduce` — expected: PASS. Re-run the existing guarded-match tests to confirm their route is unaffected.

- [ ] **Step 5: Full suite, clippy, fmt, commit**

```bash
git commit -am 'feat(rill-lang): route bang match arms through runtime dispatch with per-arm force'
```

### Task 7: Lowering — force strict `data` fields at construction

**Files:**
- Modify: `rill-lang/src/lower.rs` — record ctor (`:1220`-`:1310`), sum ctor (`:1319`-`:1343`), `sum_ctor` (`:4541`)
- Test: `rill-lang/src/lower.rs`

**Interfaces:**
- Consumes: `DataInfo` strictness bits (Task 4).
- Produces: in complete mode, a strict field's argument register is `ValueForce`'d into a fresh register before the constructor head is built.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn data_strict_field_forces_arg_at_construction() {
    let ir = ir_of("{-# MODE complete #-}\ndata T = C !Float; main = C 1.5;");
    // the ctor application's lowering contains ValueForce whose src is the
    // argument register, before the ctor head register is written
}

#[test]
fn data_lazy_field_emits_no_force() {
    let ir = ir_of("{-# MODE complete #-}\ndata T = C Float; main = C 1.5;");
    assert!(!has_instr(ir.fragment(0), ValueInstr::ValueForce { .. }));
}

#[test]
fn record_strict_field_forces() {
    // `data P = P { cutoff :: !Float }; main = P { cutoff = 1.5 };` — ValueForce present
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp --lib lower::tests::data_strict` — expected: FAIL (no construction-site force yet).

- [ ] **Step 3: Implement**

At the record/sum ctor sites, look up the type's `DataInfo` (record: per-field `(String, ValueTy, bool)`; sum: per-payload `(ValueTy, bool)`), and for each strict field/payload, emit `ValueForce` on the evaluated argument register into a fresh register and use the fresh register for the constructor head. Newtype has no strict fields (parser rejects them) — leave `parse_newtype_def`'s path alone.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rill-lang --features dsp --lib lower` — expected: PASS.

- [ ] **Step 5: Full suite, clippy, fmt, commit**

```bash
git commit -am 'feat(rill-lang): force strict data fields at construction'
```

### Task 8: Render — print `!` everywhere; roundtrip tests

**Files:**
- Modify: `rill-lang/src/render.rs` — pattern rendering (`:462`-`:480`), param rendering (`:371`-`:377`), `Def::Data` (`:87`), `Def::Sum` (`:108`), let-def rendering
- Test: `rill-lang/src/render.rs` (helper `roundtrip_main` `:616`; add a program-level roundtrip helper if the def/data positions need one)

**Interfaces:**
- Consumes: the AST strictness bits (Task 2).
- Produces: `render` output that the Task 3 parser accepts unchanged (roundtrip identity) for all five positions.

- [ ] **Step 1: Write the failing roundtrip tests**

Extend the existing `roundtrip_*` test family:

```rust
#[test]
fn render_roundtrip_bang_param()      // program with `step !x = x * 0.5;` round-trips
#[test]
fn render_roundtrip_bang_lambda()     // `main = (\ !x -> x) 1.0;`
#[test]
fn render_roundtrip_bang_match()      // `!(Just v)` and `Just !v` arms round-trip
#[test]
fn render_roundtrip_strict_data()     // `data T = C !Int | D Float;` and `{ cutoff :: !Float }`
#[test]
fn render_roundtrip_bang_let()        // expr-let `let { !x = 1.0 } in x` and do-let
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp --lib render::tests::render_roundtrip_bang` — expected: FAIL (renderer drops `!`; roundtrip output differs or reparses).

- [ ] **Step 3: Implement**

In render: `Pattern::Bang(p)` prints `!` then the inner pattern; a `Param` with `strict` prints a leading `!`; `Def::Data`/`Def::Sum` print `!` before each strict field/payload type; a let-bound def with `strict` prints `!name = e`. Match the Task 3 parser token-for-token.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rill-lang --features dsp --lib render` — expected: PASS (all roundtrips).

- [ ] **Step 5: Full suite, clippy, fmt, commit**

```bash
git commit -am 'feat(rill-lang): render strictness annotations'
```

### Task 9: End-to-end behavior, mode identity, docs, final verification

**Files:**
- Modify: `rill-lang/src/lib.rs` (public compile→run tests), `rill-lang/src/program_runner.rs` (finite-output tests), lexer module docs/README token table (replace any remaining `!` → Cut prose in *live* docs; `arrow.rs` stays untouched)
- Test: `rill-lang/src/lib.rs`, `rill-lang/src/program_runner.rs`

**Interfaces:**
- Consumes: the full chain from Tasks 1–8.

- [ ] **Step 1: Write the end-to-end tests**

```rust
#[test]
fn bang_annotated_program_runs_in_complete_mode() {
    // compile("{-# MODE complete #-}\nstep !x = x * 0.5; main = step 4.0;")
    // and run to a finite output asserting the expected value (use the
    // public_compile_* helper this module already uses for ir_tests)
}

#[test]
fn bang_is_behavioral_noop_in_strict_mode() {
    // strict-mode run of `step !x = x * 0.5; main = step 4.0;` produces the
    // same output as the identical program without `!` (Review Focus #4)
}

#[test]
fn strict_self_referential_let_is_bottom() {
    // complete-mode `main = let { !x = x } in 1.0;` runs to Bottom (a black hole
    // forced at bind), not an infinite loop — mirror the VM-level
    // force_self_cycle_returns_bottom_not_hang expectation
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp --lib ir_tests::bang` (or wherever the public-compile tests live) — expected: FAIL or no such symbols yet.

- [ ] **Step 3: Update stale live docs**

`rg -n 'Cut|strictness' rill-lang/ --glob '!src/arrow.rs' --glob '!src/graph/**'` — for any live doc (lexer module doc, README token table) that still says `!` is a Cut/discard, correct it to the strictness marker. Do **not** touch `arrow.rs` or `graph/reconstruct.rs`.

- [ ] **Step 4: Implement end-to-end coverage**

Wire whatever the failing tests reveal (typically nothing beyond Tasks 1–8; if a run-harness gap surfaces, fix the harness, not the semantics). Ensure `!x` on a `main` signal parameter (a buffer) is accepted and is a no-op.

- [ ] **Step 5: Full verification**

Run: `cargo build -p rill-lang --features dsp`, `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp --tests`, `cargo fmt`. Expected: clean build, all tests green, zero warnings, formatted. Confirm the spec's checklist: no change to `not`, `!=`, or strict-mode output; bang-annotated program compiles and runs on the complete-mode value track.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m 'feat(rill-lang): strictness annotations end-to-end and docs'
```