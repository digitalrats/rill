# rill-lang Strict Data Fields (`data T = C !Int`, `{ x: !Float }`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete the strictness-annotations feature by adding Haskell 2010 strict data fields — `!` before a product field type (`data P = { x: !Float }`) and before a sum payload type (`data T = C !Int`) — forcing the argument to WHNF at construction via the existing `ValueInstr::ValueForce`.

**Architecture:** This is the missing Task 7 (plus the data parts of Tasks 2/3/4/8) of `docs/superpowers/plans/2026-10-14-rill-lang-strictness-annotations.md`, whose spec was approved 2026-10-14. Bang patterns for params/λ/case/let already landed; only strict `data` fields were skipped. The approved design decisions are inherited verbatim: the AST carries a strictness bit per field/payload (`Def::Data.fields: Vec<(String, TypeExpr, bool)>`, `Def::Sum.ctors: Vec<(String, Vec<(TypeExpr, bool)>)>`), the bit is plumbed into `DataInfo` (`DataInfo::Record(Vec<(String, ValueTy, bool)>)`, `DataInfo::Sum(Vec<(String, Vec<(ValueTy, bool)>)>)`) because ctor lowering reads `env.data_types` not the AST, and lowering emits `ValueForce { dst: fresh, src: arg_reg }` for each strict field before `ValueConstructRecord`/`ValueConstructSum`. Emission is unconditional in both modes — matching the already-merged bang-param force at fragment entry (`lower.rs:5577-5593`, not mode-gated); in `MODE strict` the force is a VM no-op (registers already hold WHNF).

**Tech Stack:** Rust, `rill-lang` crate (`--features dsp`), no new dependencies. Cargo workspace at `/home/mikek/Projects/digitalrats/rill`.

**Spec:** `docs/superpowers/specs/2026-10-14-rill-lang-strictness-annotations-design.md` (approved 2026-10-14, commit `9391283`) — §2 (AST/DataInfo shapes), §Data ctors (force at construction), §8 (render), plus the plan's Task 7 steps. Line anchors below are from the current tree (2026-10-19, branch `feature/rill-lang-arrays-fp` at `d60190d`); function and type names are the stable contract — re-grep for anchors if they moved.

**Execution branch:** create `feature/rill-lang-strict-data-fields` off `develop` (`git flow feature start strict-data-fields`) — this work is independent of the unmerged `feature/rill-lang-arrays-fp` (whose integration decision is still pending with the user).

## Global Constraints

- Inherited from the spec, verbatim: `not` remains logical negation; `!=` (`Tok::NotEq`) is unaffected; `!` in bare expression/atom position stays a parse error; `!` is **not a type** — strictness never changes `ValueTy`/`TypeExpr` inference.
- Only position added: `!` before a product field type and before a sum payload type in `data` declarations. **`newtype N = N !T` stays a parse error** (Haskell 2010: newtypes cannot have strict fields).
- `MODE strict`: strict fields must be behaviorally identical to no `!`.
- Naming precedent: the existing strictness bits on `Param`/`Def::Eval` are named `bang`; the new bits are positional tuple elements, documented as "strictness flag". New helper on `lower.rs`: `ctor_payload_bangs`.
- Gates at the end of every task: `cargo build -p rill-lang --features dsp` clean, `cargo test -p rill-lang --features dsp` green, zero warnings under `cargo clippy -p rill-lang --features dsp --lib --tests`, `cargo fmt` (max_width=100, tab_spaces=4). No blanket `#[allow]`.
- Commit messages in single quotes, conventional commits (`feat(lang): …`). Code comments and docs in English.
- Do not touch `rill-lang/src/arrow.rs` or `graph/reconstruct.rs` (legacy prose referencing the removed `Expr::Cut`).

## Review Focus

The five inputs a reasonable user would expect to work that no single task's tests exercise alone:

1. **A strict field whose argument diverges (`a = a`) must force to Bottom at construction in complete mode** — the whole point of the flag; a silent thunk store would make `!` a no-op. → Task 4 behavioral test.
2. **A LAZY field with the same diverging argument must store it unforced** — the record/sum is WHNF, only the field is a thunk. Strictness must be per-field, never match-top or whole-ctor. → Task 4 control test.
3. **`newtype Hz = Hz !Float` stays a parse error** (Haskell 2010). → Task 2 parser test.
4. **`MODE strict` + strict fields is behaviorally identical to no `!`** — the force must be a VM no-op, not a divergence or a compile error. → Task 6 strict-mode test.
5. **Render/parse round-trip identity for `!` in data declarations** — `!` in a data declaration is unambiguous (no expressions/patterns there), so render must print exactly what the parser accepts, with no whitespace sensitivity. → Task 5 render round-trip.

---

### Task 1: AST + `DataInfo` — strictness bits and mechanical plumbing

**Files:**
- Modify: `rill-lang/src/ast.rs` — `Def::Data` (`:392-402`), `Def::Sum` (`:403-413`)
- Modify: `rill-lang/src/types/ty.rs` — `DataInfo` (`:176-186`)
- Modify (compile-guided plumbing): `rill-lang/src/parser.rs`, `rill-lang/src/types/infer.rs`, `rill-lang/src/types/ty.rs`, `rill-lang/src/lower.rs`, `rill-lang/src/render.rs`, `rill-lang/src/types/ffi.rs`, test modules in each — every construction/destructuring site of the new shapes
- Test: `rill-lang/src/ast.rs` — new `#[cfg(test)] mod tests` (none exists today)

**Interfaces:**
- Produces (exact shapes, the contract for all later tasks):
  - `Def::Data { name, tyvars, fields: Vec<(String, TypeExpr, bool)>, span }` — third tuple element = strictness flag.
  - `Def::Sum { name, tyvars, ctors: Vec<(String, Vec<(TypeExpr, bool)>)>, span }`.
  - `DataInfo::Record(Vec<(String, ValueTy, bool)>)`, `DataInfo::Sum(Vec<(String, Vec<(ValueTy, bool)>)>)`.
- Every construction site sets the bit to `false` for now — nothing can parse `!` until Task 2, so no behavior is reachable.

- [ ] **Step 1: Write the failing serde test**

In `rill-lang/src/ast.rs`, add a `#[cfg(test)] mod tests` with:

```rust
#[cfg(feature = "serde")]
#[test]
fn data_strictness_ast_serde_roundtrips() {
    let d = Def::Data {
        name: "P".into(),
        tyvars: vec![],
        fields: vec![
            ("x".to_string(), TypeExpr::Float, true),
            ("y".to_string(), TypeExpr::Float, false),
        ],
        span: Span::new(0, 10),
    };
    let s = Def::Sum {
        name: "T".into(),
        tyvars: vec![],
        ctors: vec![("C".to_string(), vec![(TypeExpr::Int, true)])],
        span: Span::new(0, 10),
    };
    for def in [d, s] {
        let json = serde_json::to_string(&def).unwrap();
        assert_eq!(serde_json::from_str::<Def>(&json).unwrap(), def);
    }
}
```

(Check the actual `TypeExpr` enum variant names in `ast.rs`; the point is that the new tuple shapes round-trip through `serde_json` exactly.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p rill-lang --features dsp,serde --lib ast` — expected: FAIL (does not compile: the tuple shapes do not have the third element yet).

- [ ] **Step 3: Add the AST shapes and the `DataInfo` shapes**

`Def::Data.fields` → `Vec<(String, TypeExpr, bool)>`; `Def::Sum.ctors` → `Vec<(String, Vec<(TypeExpr, bool)>)>`; `DataInfo::Record` → `Vec<(String, ValueTy, bool)>`; `DataInfo::Sum` → `Vec<(String, Vec<(ValueTy, bool)>)>`. Update the doc comments (English): the third element is the Haskell 2010 strictness flag — the argument is forced to WHNF at construction.

- [ ] **Step 4: Compile-guided plumbing (all bits `false`)**

Run `cargo build -p rill-lang --features dsp` and fix every site the compiler flags:
- `parser.rs` `parse_data_def` (`:626-650`): field/payload pushes gain `false`.
- `types/infer.rs` DataInfo builders (`:1826-1834` Record, `:1855-1864` Sum): map `(f, t)` → `(f, ft, false)` / `(t, ft, false)`.
- `types/ty.rs` prelude builder (`:861-869`), builtin shapes (`:939-961`), consumers (`:2019`, `:2030`), test constructions (`:2531`, `:2535`).
- `lower.rs`: every `DataInfo::Record`/`Sum` destructuring (`:1281`, `:1317`, `:1381`, `:1443`, `:1514`, `:2250`, `:2347`, `:6421`, `:6806-6821`, `:6940`, `:6946`) — `(f, t)` patterns become `(f, t, _)` (or `(f, _, _)`), tuple-index accesses adapt; `sum_ctor` (`:6806`) maps payload tuples to types only (`payload.iter().map(|(t, _)| t.clone())`).
- `render.rs` (`:62-104`): field/payload destructuring gains `_` for the bit (rendering of `!` comes in Task 5).
- `types/ffi.rs:153`: `DataInfo::Record(fields) => fields` — adapt to the 3-tuple (map to `(String, ValueTy)` pairs or whatever the caller expects).

Do NOT change behavior — all bits are `false`, so no test may change meaning. Grep `rg -n 'DataInfo::Record|DataInfo::Sum'` to find any construction/destructuring site the compiler misses in `#[cfg(test)]` code.

- [ ] **Step 5: Run the serde test and the full suite**

Run: `cargo test -p rill-lang --features dsp,serde --lib ast` — expected: PASS. Then `cargo test -p rill-lang --features dsp` — expected: all green (behavior unchanged; no `!` can parse in data yet).

- [ ] **Step 6: Clippy, fmt, commit**

Run: `cargo clippy -p rill-lang --features dsp --lib --tests` (zero warnings), `cargo fmt`.
```bash
git add -A
git commit -m 'feat(lang): add strictness bits to data fields and DataInfo'
```

### Task 2: Parser — `!` strictness flags in `data` declarations

**Files:**
- Modify: `rill-lang/src/parser.rs` — `parse_data_def` product field loop (`:626-634`) and sum payload loop (`:643-650`); confirm `parse_newtype_def` rejects `Tok::Bang`
- Test: `rill-lang/src/parser.rs` tests mod (helpers `prog`/`body`), `rill-lang/tests/program_json.rs`

**Interfaces:**
- Consumes: `Tok::Bang`, Task 1 AST shapes.
- Produces: parsed `Def::Data.fields`/`Def::Sum.ctors` with real strictness bits; `newtype N = N !T` remains a parse error; `!` in expression/atom position remains a parse error (no parser change there).

- [ ] **Step 1: Write the failing parser tests**

In `parser.rs` tests mod:

```rust
#[test]
fn parse_strict_product_field() {
    // `data P = { x: !Float, y: Float }` -> fields [("x", Float, true), ("y", Float, false)]
    let p = prog("data P = { x: !Float, y: Float }; main = P { x: 1.0, y: 2.0 };");
    // assert the Def::Data fields tuple's third element is true only for "x"
}

#[test]
fn parse_strict_sum_payload() {
    // `data T = C !Int | D Float;` -> ctors [("C", [(Int, true)]), ("D", [(Float, false)])]
    let p = prog("data T = C !Int | D Float; main = C 1;");
}

#[test]
fn newtype_strict_field_is_parse_error() {
    let src = "newtype Hz = Hz !Float; main = Hz 1.0;";
    assert!(parse(&tokenize(src).unwrap(), src.as_bytes()).is_err());
}
```

(Use the existing `prog` helper's shape; assert exact AST tuples with `matches!`.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp --lib parser::tests::parse_strict` — expected: FAIL (a `!` after `:` or in a ctor payload is currently a parse error).

- [ ] **Step 3: Implement**

In `parse_data_def`:
- Product (`:626-634`): after `self.eat(&Tok::Colon)?`, peek `Tok::Bang` → `self.bump()` and set `strict = true`, else `strict = false`; then `let t = self.parse_type_expr()?;` and `fields.push((fname, t, strict))`.
- Sum (`:643-650`): extend the payload loop condition from `matches!(self.peek().tok, Tok::Ident(_) | Tok::Int(_))` to also match `Tok::Bang`; when `Tok::Bang` is seen, `self.bump()` and set `strict = true` before `parse_type_single`, else `strict = false`; `payload.push((t, strict))`.
- Do NOT touch `parse_newtype_def` — verify it currently rejects a `Bang` after `=`, which the Task 2 test pins. Do NOT touch `parse_prefix`/`parse_atom` (bare `!` expression stays an error).

- [ ] **Step 4: Run the parser tests**

Run: `cargo test -p rill-lang --features dsp --lib parser` — expected: PASS. Verify Review Focus #5 input for the parser half: a lone `!` in expression position (`main = !5;`) still errors.

- [ ] **Step 5: Add the JSON round-trip test**

In `rill-lang/tests/program_json.rs`, add a source with strict data to the `roundtrip()` family (the helper asserts parse → JSON → deserialize → exact equality):

```rust
#[test]
fn roundtrips_strict_data_declaration() {
    roundtrip("data P = { x: !Float, y: Float }; data T = C !Int | D Float; main = C 1");
}
```

- [ ] **Step 6: Full suite, clippy, fmt, commit**

Run: `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp --lib --tests`, `cargo fmt`.
```bash
git add rill-lang/src/parser.rs rill-lang/tests/program_json.rs
git commit -m 'feat(lang): parse strict data fields'
```

### Task 3: Inference — thread real bits into `DataInfo`

**Files:**
- Modify: `rill-lang/src/types/infer.rs` — `Def::Data`/`Def::Sum` registration (`:1818-1874`)
- Modify: `rill-lang/src/types/ty.rs` — prelude `Def::Data` registration (`:853-880`)
- Test: `rill-lang/src/types/infer.rs` tests mod

**Interfaces:**
- Consumes: Task 2 AST bits.
- Produces: `env.data_types` `DataInfo` entries with real strictness bits on BOTH registration paths (user defs in `infer`, prelude/catalog defs in `ty`). Builtin shapes (`Maybe`/`Pair`/`Either`, ty.rs:939-961) carry `false` — no builtin is strict.

- [ ] **Step 1: Write the failing inference test**

In `types/infer.rs` tests mod (it can inspect `env` directly):

```rust
#[test]
fn data_strictness_flows_into_data_info() {
    // after inferring `data T = C !Float; main = C 1.5;`,
    // env.data_types["T"] == DataInfo::Sum([("C", [(Float, true)])])
    // and `data P = { x: !Float, y: Float };` gives DataInfo::Record([("x", (Float, true)), ("y", (Float, false))])
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rill-lang --features dsp --lib types::infer::tests::data_strictness` — expected: FAIL (bits are still `false` from Task 1).

- [ ] **Step 3: Implement**

`types/infer.rs` `:1826-1834` (Record): map `(f, t, strict)` → `(f.clone(), ft, strict)`. `:1855-1864` (Sum): map `(t, strict)` → `(ft, strict)`. `types/ty.rs` `:861-869` (prelude Record): same — map the AST bits instead of `false`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rill-lang --features dsp --lib types` — expected: PASS.

- [ ] **Step 5: Full suite, clippy, fmt, commit**

```bash
git add rill-lang/src/types/infer.rs rill-lang/src/types/ty.rs
git commit -m 'feat(lang): thread data strictness into DataInfo'
```

### Task 4: Lowering — force strict `data` fields at construction

**Files:**
- Modify: `rill-lang/src/lower.rs` — record ctor path (`:2248-2345`, field loop `:2296-2324`), sum ctor path (`:2362-2385`), new helper near `sum_ctor` (`:6806`)
- Test: create `rill-lang/tests/data_strict_fields.rs`

**Interfaces:**
- Consumes: `DataInfo` strictness bits (Task 3), `ValueInstr::ValueForce { dst, src }` (exists).
- Produces:
  - In the record ctor path, each STRICT field's lowered register is forced into a fresh register before `ValueConstructRecord`.
  - In the sum ctor path, each STRICT payload register is forced into a fresh register before `ValueConstructSum`.
  - Emission is UNCONDITIONAL in both modes (matches the bang-param precedent at `lower.rs:5577-5593`); in `MODE strict` the force is a VM no-op.
  - New helper: `fn ctor_payload_bangs(&self, sum_name: &str, ctor_idx: usize) -> Vec<bool>` — strictness per payload position of a sum constructor.
  - Field UPDATE (`Expr::FieldUpdate`, `lower.rs:1458`) does NOT force — construction-only, matching the approved Task 7 scope. (Deferred minor; do not implement.)

- [ ] **Step 1: Write the failing behavioral tests**

In `tests/data_strict_fields.rs`, mirror `lazy_mode.rs`'s `eval_complete` helper (via `tests/common::*`, `eval_mode::<256>`, `compile_eval`, `WorldState::result`, `rill_lang::arena::Value`):

```rust
#[test]
fn strict_record_field_divergence_is_bottom() {
    // Complete mode. `a = a` diverges; a strict `x` must force it AT
    // CONSTRUCTION, so main's result is Bottom, never a Record holding a thunk.
    let (world, v) = eval_complete(
        "data P = { x: !Float, y: Float }\na = a\nmain = P { x: a, y: 1.0 }",
    );
    match world.arena.get(v) {
        rill_lang::arena::Value::Bottom => {}
        other => panic!("expected Bottom (strict field forced the divergence), got {other:?}"),
    }
}

#[test]
fn lazy_record_field_stores_divergence_unforced() {
    // The SAME program without `!` on `x` must store the thunk unforced: the
    // record is WHNF. This pins that strictness is per-field, not whole-ctor.
    let (world, v) = eval_complete("data P = { x: Float, y: Float }\na = a\nmain = P { x: a, y: 1.0 }");
    match world.arena.get(v) {
        rill_lang::arena::Value::Record(_) => {}
        other => panic!("expected an unforced Record, got {other:?}"),
    }
}

#[test]
fn strict_sum_ctor_divergence_is_bottom() {
    let (world, v) = eval_complete("data Opt = Just !Float | None\na = a\nmain = Just a");
    match world.arena.get(v) {
        rill_lang::arena::Value::Bottom => {}
        other => panic!("expected Bottom, got {other:?}"),
    }
}

#[test]
fn lazy_sum_ctor_stores_payload_unforced() {
    let (world, v) = eval_complete("data Opt = Just Float | None\na = a\nmain = Just a");
    match world.arena.get(v) {
        rill_lang::arena::Value::Sum(0, _) => {}
        other => panic!("expected Sum(0, [thunk]) unforced, got {other:?}"),
    }
}
```

(Adapt `eval_complete`'s exact signature to `tests/common`'s current helpers — the point is compile in `Mode::Complete` and read the result ref.)

- [ ] **Step 2: Write the failing IR tests** (same file)

Mirror `world_seat.rs:17` (`compile_ir(src, mode) -> Ir`) and `bang_patterns.rs:186-201` (`force_count(ir)`):

```rust
#[test]
fn strict_record_field_emits_value_force() {
    // `P { x: !Float, y: Float }`, x strict: exactly one ValueForce.
    let ir = compile_ir("data P = { x: !Float, y: Float }; main = P { x: 1.0, y: 2.0 }.y", Mode::Complete);
    assert_eq!(force_count(&ir), 1);
}

#[test]
fn lazy_record_field_emits_no_force() {
    let ir = compile_ir("data P = { x: Float, y: Float }; main = P { x: 1.0, y: 2.0 }.y", Mode::Complete);
    assert_eq!(force_count(&ir), 0);
}

#[test]
fn strict_sum_payload_emits_value_force() {
    assert_eq!(force_count(&compile_ir("data T = C !Float; main = C 1.5", Mode::Complete)), 1);
    assert_eq!(force_count(&compile_ir("data T = C Float; main = C 1.5", Mode::Complete)), 0);
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp --test data_strict_fields` — expected: FAIL (no force at construction sites yet; strict behavioral tests return a Record/Sum, not Bottom; force_count is 0).

- [ ] **Step 4: Implement**

- Record ctor path (`:2296-2324`): `fields` is now `Vec<(String, ValueTy, bool)>`; the field loop destructures `(fname, _, strict)`; after `(fr, ft)` is produced, if `strict` emit `ValueForce { dst: fresh, src: fr }` and push the FRESH register into `field_regs` (the force result feeds the `ValueConstructRecord` at `:2326`).
- Sum ctor path (`:2373-2377`): after `self.sum_ctor(name)` returns `(sum_name, ctor_idx, _)`, compute `let bangs = self.ctor_payload_bangs(&sum_name, ctor_idx);`; in the arg loop, when `bangs[i]`, force `pr` into a fresh register before pushing.
- Add `ctor_payload_bangs` near `sum_ctor` (`:6806`): look up `self.env.data_types[sum_name]`, map `DataInfo::Sum(ctors)` → `ctors[ctor_idx].1.iter().map(|(_, b)| *b).collect()`.
- Leave `sum_ctor`'s signature unchanged (Task 1 already mapped payload tuples to types inside it).

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p rill-lang --features dsp --test data_strict_fields` — expected: PASS (strict → Bottom / 1 force; lazy → Record/Sum / 0 forces). Then the full suite.

- [ ] **Step 6: Clippy, fmt, commit**

Run: `cargo clippy -p rill-lang --features dsp --lib --tests`, `cargo fmt`.
```bash
git add rill-lang/src/lower.rs rill-lang/tests/data_strict_fields.rs
git commit -m 'feat(lang): force strict data fields at construction'
```

### Task 5: Render — print `!`; round-trip identity

**Files:**
- Modify: `rill-lang/src/render.rs` — `Def::Data` (`:62-82`), `Def::Sum` (`:83-104`)
- Test: `rill-lang/src/render.rs` tests mod (existing round-trip helpers, e.g. `roundtrip_main`)

**Interfaces:**
- Consumes: Task 1 AST bits.
- Produces: `render` output the Task 2 parser accepts unchanged (round-trip identity) for strict product fields and strict sum payloads.

- [ ] **Step 1: Write the failing round-trip tests**

In `render.rs` tests mod, extend the round-trip test family:

```rust
#[test]
fn render_roundtrip_strict_product_data() {
    // `data P = { x: !Float, y: Float }; main = P { x: 1.0, y: 2.0 };`
    // render → reparse → identical AST.
}

#[test]
fn render_roundtrip_strict_sum_data() {
    // `data T = C !Int | D Float; main = C 1;`
}
```

(Use the existing program-level round-trip helper — parse, render, reparse, compare.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp --lib render::tests::render_roundtrip_strict` — expected: FAIL (renderer drops `!`; the reparse yields a different AST or errors).

- [ ] **Step 3: Implement**

In `render.rs`: `Def::Data` field rendering (`:73-79`) — when the tuple's third element is true, print `!` right before `render_type_expr(tname, buf)`; `Def::Sum` payload rendering (`:99-102`) — same, before each strict payload type. Match the Task 2 parser token-for-token (`{ x: !Float }`, `C !Int`).

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rill-lang --features dsp --lib render` — expected: PASS (all round-trips, including the new strict-data ones). Verify Review Focus #5 end-to-end: the render output of a strict data declaration reparses to the identical AST.

- [ ] **Step 5: Full suite, clippy, fmt, commit**

```bash
git add rill-lang/src/render.rs
git commit -m 'feat(lang): render strict data fields'
```

### Task 6: End-to-end mode identity, docs check, final verification

**Files:**
- Modify: `rill-lang/tests/data_strict_fields.rs` (add strict-mode and terminating-equality tests)
- Docs check: `rg -n 'Cut|strictness' rill-lang/ --glob '!src/arrow.rs' --glob '!src/graph/**'`

**Interfaces:**
- Consumes: the full chain from Tasks 1–5.

- [ ] **Step 1: Write the strict-mode identity and equality tests**

In `tests/data_strict_fields.rs`:

```rust
#[test]
fn strict_fields_are_behavioral_noop_in_strict_mode() {
    // `MODE strict`: `data P = { x: !Float, y: Float }; main = P { x: 1.0, y: 2.0 }.y`
    // compiles and runs to 2.0; `data T = C !Float; main = case C 2.0 of { C x -> x }` -> 2.0.
    // (Review Focus #4 — run in Mode::Strict via compile_eval.)
}

#[test]
fn strict_and_lazy_fields_agree_when_value_terminates() {
    // Complete mode: `main = P { x: 2.0, y: 3.0 }.y` -> 3.0 with and without `!`
    // on `x` — strictness changes WHEN evaluation happens, never the result.
}
```

- [ ] **Step 2: Run to verify they pass**

Run: `cargo test -p rill-lang --features dsp --test data_strict_fields` — expected: PASS (Tasks 1–5 already implement the behavior; these pin the mode-identity semantics).

- [ ] **Step 3: Docs check**

Run the `rg` above. If any live doc (lexer module doc, README token table) still describes `!` as a discard/Cut rather than a strictness marker, correct it. Do NOT touch `arrow.rs` or `graph/reconstruct.rs`. If no live doc is stale, make no change.

- [ ] **Step 4: Full verification**

Run: `cargo build -p rill-lang --features dsp`, `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp --lib --tests`, `cargo fmt`. Expected: clean build, all tests green, zero warnings, formatted. Confirm the spec checklist: no change to `not`, `!=`, or strict-mode output; a strict-data program compiles and runs on the complete-mode value track.

- [ ] **Step 5: Commit (only if docs changed)**

```bash
git add -A
git commit -m 'feat(lang): strict data fields end-to-end'
```
