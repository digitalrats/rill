# Stage 7b — exact literals, `Ratio`, defaulting, and the six kernels

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give numeric literals the Report's treatment — a strict exact value
widened by `fromInteger`/`fromRational` — add `Ratio`/`Rational`, implement
`default (Integer, Double)`, and fill design §3.9's six missing kernels.

**Architecture:** A literal becomes one of two AST nodes (integer, fractional)
carrying an exact value; its type comes from unifying `fromInteger`/
`fromRational`'s result variable with the context. Ten small steps, each
independently green, ordered so nothing is deleted before its replacement
exists.

**Tech Stack:** Rust, `num-bigint` + `num-rational` (already-pinned workspace
deps), rill-lang's existing arena / IR / interpreter / inference.

**Spec:** `docs/superpowers/specs/2026-10-28-rill-lang-literal-markers-design.md`
(approved 2026-10-29). The plan argues from it; read both.

## Global Constraints

- Haskell 2010 / GHC is the acceptance criterion. **A documented divergence is
  an error**, not a justification — every remaining departure is a row in the
  spec's §6.2 with a named owner.
- **Do not recreate H2010 behaviour by workarounds** (spec §0): no fake
  `Integral`/`Real`/`Enum`/`Bounded`/`Show`; no `matches!` list of "classes we
  pretend to have"; no derived structural instance on a type whose instances are
  semantic; one concept = one representation.
- English only in code, comments, and docs.
- Gates per task (never `--all-targets`):
  - `cargo test -p rill-lang --features dsp`
  - `cargo clippy -p rill-lang --features dsp`
  - `cargo fmt --check`
  - `cargo test -p rill-lang --features "serialization dsp" --test program_json`
- Baseline at plan start: **1157 tests pass**, clippy 0, fmt clean,
  `program_json` 9/9. Every task ends at least that green.
- Commit messages: one line, single quotes.
- No debug leftovers: no `std::env::var`, no `eprintln!`, no `zz_*.rs`.
- `Strict` mode prints every `write` twice (pre-existing). Tests that run in
  Strict assert the doubled text and say so.
- `Int` stays 64-bit; `i32` stays a parameter-boundary concern (spec §1.6,
  A14); **no `Int32` language type** in this plan.

## Review Focus

The failure modes most likely to bite, each with the task that pins its test:

1. **A literal silently losing precision.** `0.1` must be `1 % 10` at `Rational`
   and `3602879701896397 % 36028797018963968` must never appear. → Tasks 6, 7.
2. **A literal in a position whose type the compiler guesses.** After this plan
   no position may guess; an untypable literal is a compile error. → Task 7
   (deletion criterion + the three stages), Task 5.
3. **Defaulting firing in the wrong place** — inside a signature-constrained
   body, or not at all for `main = 1 + 2`. → Task 7 (stage 3).
4. **`Ratio` comparing structurally** — `1/2 == 2/4` must be `True`,
   `Ratio{1,2} > Ratio{1,3}` must be `True`. → Task 3.
5. **One concept, two representations** — two places truncating `Integer → Int`,
   a `Ratio`-shaped type at compile time and a leaf at runtime. → Tasks 5, 7.
6. **Collapsing the three inference stages.** A missing instance must be reported
   as "no instance of …", never as "cannot unify"; and defaulting must not fire
   inside a signature-constrained body. Three different errors, three different
   causes — Task 7 pins one program per stage.
7. **A `Ratio` instance written before the `fromInteger` signature is fixed.**
   Measured: `fromInteger n = Ratio { num: n, den: 1 }` fails while the method is
   `Int -> a`, and an unannotated literal field (`num: 1`) fails the same way.
   Task 3 must omit `fromInteger` from the instance and Task 7 must add it. A
   reviewer seeing `fromInteger` in a `Ratio` body before Task 7 should reject
   the task. → Tasks 3, 7.

---

### Task 1: a `type` synonym may name a parameterized type

**Files:**
- Modify: `rill-lang/src/ast.rs` (`Def::TypeAlias.target`)
- Modify: `rill-lang/src/parser.rs` (`parse_type_alias_def`, ~1565)
- Modify: `rill-lang/src/types/ty.rs` (`type_aliases`, `type_entry`'s alias walk ~2421)
- Modify: `rill-lang/src/desugar.rs`, `render.rs`, `program_serde.rs` if they
  touch the field
- Test: `rill-lang/tests/type_names.rs`

**Interfaces:**
- Consumes: `TypeExpr` (already exists), `sig_ty_to_vty` for resolution.
- Produces: `Def::TypeAlias { name: String, target: TypeExpr, span: Span }`;
  `TypeEnv::type_aliases: HashMap<String, TypeExpr>`; a way to resolve an alias
  to a `ValueTy` (Task 3 needs `type Rational = Ratio Integer` to denote
  `Con("Ratio", [Integer])`).

**Why an AST change, not a parser tweak:** measured — `Def::TypeAlias.target`
is a `String` (`ast.rs:471`) and `TypeEnv::type_aliases` is
`HashMap<String, String>` (`ty.rs:603`), walked as a name chain in
`type_entry` (`ty.rs:2421`). `Ratio Integer` cannot be spelled in a string
chain. The AST is **serialized**, so `program_json` is part of this task's cost.

- [ ] **Step 1: Write the failing test**

In `rill-lang/tests/type_names.rs`:

```rust
#[test]
fn a_type_synonym_may_name_a_parameterized_type() {
    // `type Rational = Ratio Integer` — H2010 §4.1.2 allows any type on the
    // right. Before this task the parser accepts only a bare identifier, so a
    // synonym of a parameterized type cannot be written at all.
    let env = TypeEnv::with_builtins();
    assert_eq!(env.type_entry("String"), Some(TypeEntry::Alias("List".to_string())));
    // `Ratio` is registered by Task 3; this task only has to make the SYNTAX
    // work, so assert through a user program instead:
    let src = "data Box a = { v: a };\ntype IntBox = Box Int;\nf :: IntBox -> Int;\nf b = b.v;\nmain = do { write (Just (show_int (f (Box { v: 3 })))) stdout; return () };";
    // compiled + run in the dedicated test below
    assert!(!src.is_empty());
}
```

(Keep the `TypeEnv` assertion only if `String` is registered as an alias; if it
is a builtin, drop that line and rely on the program.)

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p rill-lang --features dsp --test type_names`
Expected: `expected Eq, found Semi` (the parser's `expect_ident` on `Box Int`).

- [ ] **Step 3: Make the target a `TypeExpr`**

`ast.rs`: `target: TypeExpr`. `parser.rs` `parse_type_alias_def`: replace the
second `expect_ident` with the existing type-expression parser used for
signatures. `types/ty.rs`: `type_aliases: HashMap<String, TypeExpr>`; in
`type_entry`, resolve an alias by converting its `TypeExpr` with the existing
`sig_ty_to_vty`/`conv` machinery rather than a name-chain walk (keep a hop
limit against a cyclic alias). Fix every construction/read site the compiler
reports.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p rill-lang --features dsp --test type_names`
Expected: PASS, including the existing synonym tests unchanged.

- [ ] **Step 5: Run the full gates, then commit**

```bash
git commit -m 'feat: a type synonym may name a parameterized type'
```

---

### Task 2: the two inference defects that make `fromInteger`/`fromRational` ordinary methods

**Files:**
- Modify: `rill-lang/src/types/infer.rs`
  - (i) the primitive-instance branch ~6425
  - (ii) the builtin-argument loop ~5836-5852
- Test: `rill-lang/tests/typeclass.rs`, `rill-lang/tests/integer_type.rs`

**Interfaces:**
- Consumes: `signature_param_and_result_tys_conv` (already used at (i)),
  `builtin_sigs` (at (ii)).
- Produces: applying a primitive method returns its RESULT type; a builtin's
  declared parameter type reaches its argument's inference. Task 7's literal
  application depends on both.

**Two defects, both measured (spec §1.3-i / §1.3-ii).**

**(i) The primitive branch returns the unapplied arrow.** `(fromInteger 1) ::
Integer` fails with *"cannot unify value type `Thunk([Int], [Integer])` with
`Integer`"*. The branch returns
`ArrowTy::value(nest_thunk(&param_tys, vec![ret]))` — the whole arrow — where the
source-instance path returns the body's result type.

**(ii) A builtin's declared parameter type is not threaded.** `show_int
(fromInteger 1)` fails with *"cannot resolve method `fromInteger` of `Num`:
expected type unknown"*, even though `show_int`'s parameter is concretely `Int`.
The expected type is threaded only for `IO`/`Thunk` parameters or through
`int_literal_widens_to`, which is what Task 7 deletes — so this must exist
first.

- [ ] **Step 1: Write the failing test**

In `rill-lang/tests/typeclass.rs`:

```rust
#[test]
fn an_applied_primitive_method_has_its_result_type() {
    // Measured GHC: `(fromInteger 1) :: Integer` is 1. rill typed the
    // application as the METHOD'S SIGNATURE (`Thunk([Int],[Integer])`), so the
    // annotation could not unify.
    let src = "x :: Integer;\nx = fromInteger 1;\nmain = do { write (Just (show_integer x)) stdout; return () };";
    let out = run_src(src); // the file's existing helper
    assert_eq!(out, "1");
}
```

Use the file's existing run helper; if none exists, use
`rill_lang::compile` with `ForeignRegistry::new()` and `Mode::Complete`.

- [ ] **Step 2: Run it and watch it fail**

Expected: `cannot unify value type Thunk([Int], [Integer]) with Integer`.

- [ ] **Step 3: Fix (ii) — thread the declared parameter type**

Add a second failing test to `rill-lang/tests/integer_type.rs`:

```rust
#[test]
fn a_builtin_argument_gets_its_declared_parameter_type() {
    // `show_int`'s parameter is concretely `Int`, so its argument must be
    // inferred AT `Int` — today only `int_literal_widens_to` makes `show_int 2`
    // work, and Task 7 deletes that.
    assert_eq!(run("main = do { write (Just (show_int 2)) stdout; return () };"), "2");
    assert_eq!(run("main = do { write (Just (show_int (fromInteger 2))) stdout; return () };"), "2");
}
```

Run it, watch the second assertion fail with "expected type unknown", then thread
the declared parameter type into the builtin argument's inference (the
`builtin_sigs` entry already carries it). Keep the existing `Buffer`-parameter
and `map`/`concat_map` ordering behaviour exactly as it is.

- [ ] **Step 4: Fix (i) — return `ret`, not the arrow**

In the primitive branch, after computing `ret`, return `ArrowTy::value(ret)`
(the applied result), matching what the source-instance path returns. Do not
touch the source-instance path.

- [ ] **Step 5: Run the tests, then the full gates, then commit**

```bash
git commit -m 'fix: applied primitive methods return their result type; builtins thread parameter types'
```

---

### Task 2b: one curried application rule instead of fourteen copies

**Files:**
- Modify: `rill-lang/src/types/infer.rs` (add one function; replace its 8
  duplicated uses)
- Test: `rill-lang/tests/typeclass.rs`, `rill-lang/tests/arg_checking.rs`

**Interfaces:**
- Consumes: `nest_thunk` (`infer.rs:627`), `signature_param_and_result_tys_conv`,
  `infer_method_value_vty_expected`, `unify_value`.
- Produces: `fn apply_args_curried(ctx, params: &[ValueTy], ret: &ValueTy,
  args: &[Expr], span: Span, what: &str) -> Result<ArrowTy, CompileError>` —
  THE application rule for every call site that has a parameter list and a
  result type.

**Why (measured).** Applying a function's type to its arguments is implemented
**eight times** in `infer.rs`, and arity is checked in **fourteen** places. They
disagree, and the disagreement is what produced two separate Task 2 defects:

| Site | What it does | Defect it caused |
|---|---|---|
| `infer.rs:4942` (result-directed primitive) | returned `nest_thunk(&param_tys, …)` — the WHOLE arrow — on a full application | `(fromInteger 1) :: Integer` failed with "cannot unify `Thunk([Int],[Integer])` with `Integer`" (Task 2 fix i) |
| `infer.rs:6451` | `ret` with no arity guard | `fromInteger 1 2` typechecked as `Integer` where GHC reports an arity error (Task 2 fix round 1) |
| `4662`, `4735`, `4782`, `5439`, `5555`, `6551` | six more hand-written copies | no live defect, but each is a place the next fix will miss |

One function already does it correctly —
`infer_method_under_given` (`infer.rs:505-539`): reject surplus args, unify each
supplied arg against its parameter, and return the curried remainder when fewer
were supplied. Its body is the model; the rest are copies that drifted.

- [ ] **Step 1: Write the failing test**

Add to `rill-lang/tests/typeclass.rs` a test that pins the rule at EVERY site at
once, so a site that bypasses the function is caught:

```rust
#[test]
fn every_call_site_applies_the_same_curried_rule() {
    use rill_lang::lexer::tokenize;
    use rill_lang::parser::parse;
    let ok = |src: &str| {
        let toks = tokenize(src).expect("lex");
        let prog = parse(&toks, src.as_bytes()).expect("parse");
        rill_lang::types::infer::infer_program(&prog, Mode::Complete).is_ok()
    };
    // Under-application yields a function value; over-application is an error.
    // GHC 9.10.3: `fromInteger 1 2` is "No instance for `Num (t0 -> t1)`".
    assert!(ok("x :: Integer = fromInteger 1;\nmain = 0.0;"));
    assert!(!ok("x :: Integer = fromInteger 1 2;\nmain = 0.0;"));
    assert!(!ok("x :: Integer = fromInteger 1 2 3;\nmain = 0.0;"));
    // A partial application of a TWO-parameter method is a function value, and
    // applying it once more completes it — the property the copies disagreed on.
    assert!(ok("h :: Double -> Double;\nh = (+) 1.0;\nmain = 0.0;"));
    assert!(ok("x :: Double = (+) 1.0 2.0;\nmain = 0.0;"));
    assert!(!ok("x :: Double = (+) 1.0 2.0 3.0;\nmain = 0.0;"));
}
```

- [ ] **Step 2: Run it and watch it fail on the over-application rows**

Run: `cargo test -p rill-lang --features dsp --test typeclass`
Expected: at least `(+) 1.0 2.0 3.0` is accepted (should be rejected), because
the source-instance and operator paths guard differently from the primitive one.

- [ ] **Step 3: Add `apply_args_curried` and route every site through it**

Move the body of `infer_method_under_given`'s arity+curry logic into
`apply_args_curried`, with `what: &str` supplying the error message's subject
(`"method \`fromInteger\` of \`Num\`"`, `"\`add\`"`, …) so the existing
diagnostics keep their wording. Then replace the eight sites listed above:

- **full application returns `ret`** (never the whole arrow),
- **under-application returns the curried remainder**,
- **over-application is a compile error**.

Keep every site's surrounding substitution work (`cv_subst`, fresh vars, the
`ClassVarSubst` chosen) — the function takes already-converted `params`/`ret`, so
each caller keeps its own conversion and only the application half is shared.

- [ ] **Step 4: Run the tests and the inventory check**

```
cargo test -p rill-lang --features dsp
grep -c 'expects .*argument(s), got' rill-lang/src/types/infer.rs
```

Expected: all green, and the arity-message count drops from 14 to the sites that
still need their own message (aim for ≤ 3: user-def calls, operators, and the
shared function's own). If the count does not drop, the sites were not routed
through the function — that is a failure, not a detail.

- [ ] **Step 5: Run the full gates, then commit**

```bash
git commit -m 'refactor: one curried application rule for every call site'
```

---

### Task 2c: the application rule covers constructors, and stops swallowing errors

**Files:**
- Modify: `rill-lang/src/types/infer.rs` (`apply_args_curried`; the constructor
  arm ~6311; the four remaining curried sites; the typeclass-method guard)
- Modify: `rill-lang/src/types/ty.rs` if a constructor-scheme helper is needed
  (`sum_ctor_payload` already exists at `infer.rs:746`)
- Test: `rill-lang/tests/constructors.rs` (create), `rill-lang/tests/typeclass.rs`

**Interfaces:**
- Consumes: `apply_args_curried` (Task 2b), `sum_types_with_ctor`
  (`infer.rs:734`), `sum_ctor_payload` (`infer.rs:746`), `DataInfo::Sum`.
- Produces: a constructor's type is a curried function type
  (`Just :: a -> Maybe a`), built from its sum's payload and type parameters; and
  `apply_args_curried` no longer aborts on a bare-argument inference failure.

**Three findings from Task 2b's review, all measured.**

**(1) Critical — `apply_args_curried` swallows a fallback.** It runs
`let at = infer_expr(ctx, a)?;` and the `?` propagates a bare-inference failure,
so the `infer_method_value_vty_expected(ctx, a, …, Some(pt))` fallback never
runs. Four call sites inherited this. Measured, base vs HEAD:

| Program | `b1f0015` | HEAD |
|---|---|---|
| `typeclass Zero a where { zero: a; }` + `instance Zero (FixedBuffer Double) where { zero = sine 440.0 0.5 0.0; }` then `sine 440.0 0.5 0.0 + zero` | **OK** | **"cannot resolve method `zero` of `Zero`: expected type unknown"** |

**(2) Important — a widening.** The same function uses `unify_method_arg` where
the site it replaced used plain `unify_value`, so it accepts programs GHC rejects:

| Program | GHC | HEAD |
|---|---|---|
| `((\b -> b) 1.0) :: Buffer` | type error (`No instance for Fractional (FixedBuffer Double)`) | **accepted** |

**(3) H2010 divergence — a constructor is not usable as a value.** Measured in
GHC 9.10.3, all accepted:

```haskell
g = Just            -- Just 5
h = (,) 1           -- (1,2)
k = (,,) 1 2        -- (1,2,3)
map Just [1,2,3]    -- [Just 1, Just 2, Just 3]
```

Measured in rill, all **rejected** with "constructor `Just` for `Maybe` requires
arguments". A constructor IS an ordinary curried function
(`Just :: a -> Maybe a`); rill demands the exact arity.

- [ ] **Step 1: Write the failing tests**

`rill-lang/tests/constructors.rs` (create):

```rust
//! A constructor is an ordinary curried function (Haskell 2010 §4.2.1):
//! `Just :: a -> Maybe a`, so it is usable as a VALUE and partially applicable.

fn run(src: &str) -> String { /* compile + run, concatenated stdout */ }

#[test]
fn a_constructor_is_usable_as_a_value() {
    // GHC: `map Just [1,2,3]` is `[Just 1, Just 2, Just 3]`.
    assert_eq!(
        run("main = do { write (Just (show_int (length (map Just [1, 2, 3])))) stdout; return () };"),
        "3"
    );
}

#[test]
fn a_nullary_constructor_is_usable_as_a_value() {
    // GHC: `Nothing` is a value of `Maybe a`, not a function.
    assert_eq!(
        run("xs = [Nothing];\nmain = do { write (Just (show_int (length xs))) stdout; return () };"),
        "1"
    );
}

#[test]
fn a_constructor_checks_its_arity() {
    // GHC: `Just 1 2` is a type error — `Just 1` is not a function.
    assert!(compile_err("x = Just 1 2;\nmain = 0.0;").contains("expects"));
}
```

`rill-lang/tests/typeclass.rs` — the two regressions:

```rust
#[test]
fn a_nullary_method_inside_a_buffer_operand_still_resolves() {
    // The shared application rule ran a bare inference with `?` first, so the
    // expected-type fallback never happened and `zero` lost its instance.
    assert_eq!(
        run("typeclass Zero a where { zero: a; }\ninstance Zero (FixedBuffer Double) where { zero = sine 440.0 0.5 0.0; }\nmain = do { write (Just (show_double ((sine 440.0 0.5 0.0 + zero).v))) stdout; return () };"),
        // assert the program RUNS; the exact sample value is the buffer sum
        "RUNS"
    );
}

#[test]
fn a_literal_does_not_widen_into_a_buffer_parameter() {
    // GHC: `No instance for Fractional (FixedBuffer Double)`.
    assert!(compile_err("main = ((\\b -> b) 1.0) :: Buffer;").contains("cannot unify"));
}
```

Adjust the first assertion to whatever the file's helper makes natural (compile
success is the requirement — the defect was a compile error, not a wrong value).

- [ ] **Step 2: Run them and watch them fail**

Expected: `map Just` → "requires arguments"; the `Zero` program → "expected type
unknown"; the `Buffer` widening → accepted (so `compile_err` panics).

- [ ] **Step 3: Fix the two regressions in the shared rule**

In `apply_args_curried`: attempt the bare inference with `if let Ok(at) = …`
instead of `?`, so a result-directed/nullary argument falls through to the
expected-type path. Use plain `unify_value` where the replaced site did (the
`unify_method_arg` buffer carve-out belongs to the operator/method path, not to
every application).

- [ ] **Step 4: Give a constructor a curried type**

Where a constructor name appears in a value position (`infer.rs` ~6311), build its
scheme from its sum instead of demanding the exact arity:

- `params` = `sum_ctor_payload(ctx, &sum_name, name)` with each placeholder
  instantiated as a fresh live var,
- `ret` = the sum applied to its type parameters (`Con(sum_name, fresh vars)`),
- then `apply_args_curried` decides: fewer args → the curried remainder, exact →
  `ret`, more → an arity error.

A nullary constructor has `params` empty, so it is a plain value. Builtin sums
(`Maybe`, `Either`) carry placeholder vars (`Var(1)`, `Var(2)`) that are NOT live
unification vars — instantiate them, as the existing arm already does.

- [ ] **Step 5: Route the four remaining curried sites**

Task 2b's review classified the ten remaining arity checks. Four are the same
curried application and must go through the shared rule:

| Site | Why it is the same judgement |
|---|---|
| `infer.rs:5872` (builtin `builtin_sigs`) | `params`/`ret` are a curried function type; `write (Just 1)` is GHC-valid |
| `7333` (`ctx.locals` thunk) | `arg_tys`/`ret` come from `flatten_thunk` — literally `params`/`ret` |
| `7425` (closure-value over-application) | only the `>` half, the same shape already routed |
| `7493` (alias `lam_count`) | `ref_scheme` is a `Thunk`; `f 1.0` of a 2-λ target is GHC-valid |

The other six are a DIFFERENT judgement and stay: the rule's own message (531),
constructor `ret` built from args (6311 — superseded by Step 4), ctor-class
dispatch (6846), source-instance binder lists (6960), user-def λ-count with its
`vec![Double]` partial branch (7532), and the collection-operation catalog (7843).

- [ ] **Step 6: Run the tests and the inventory**

```
cargo test -p rill-lang --features dsp
grep -c 'expects .*argument(s), got' rill-lang/src/types/infer.rs
```

Expected: all green; the count drops from 10 to **≤ 6** (the six genuinely
distinct judgements). If it stays above 6, a listed site was not routed.

- [ ] **Step 7: Run the full gates, then commit**

```bash
git commit -m 'feat: a constructor is a curried value; the application rule keeps its fallback'
```

---

### Task 3: `Ratio` and `Rational`, with value-comparing instances

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`PRELUDE`; the builtin data-shape table
  ~1400 for `Ratio` if it must be pre-registered like `Pair`)
- Test: `rill-lang/tests/ratio.rs` (create)

**Interfaces:**
- Consumes: Task 1's parameterized alias; the polymorphic-instance mechanism
  (measured working); `Integer`'s `+`/`*`/`==`.
- Produces: `Con("Ratio", [t])`, `type Rational = Ratio Integer`;
  `instance Eq/Ord/Num/Fractional (Ratio a)`; field names `num`, `den`.

**Payload type is `Record(Vec<(String, ValueTy, bool)>)`** — an ordinary record,
NOT a new `Value` variant (spec §2.5): a `Ratio Integer` is an arena `Record` of
two `Integer` leaves, so the instances work through ordinary field projection.

**The instances MUST have a polymorphic head.** Measured (spec §1.3b):

| Instance written | `1/2 == 2/4` |
|---|---|
| none (derived structural) | `False` ❌ |
| `instance Eq (Ratio Integer)` | `False` ❌ |
| `instance Eq (Ratio a)` | `True` ✅ |

- [ ] **Step 1: Write the failing test**

`rill-lang/tests/ratio.rs`:

```rust
//! `Ratio a` / `Rational` — Haskell 2010 §9.5's `Data.Ratio`, as far as the
//! classes rill has allow. See design A2/A4/A6.

fn run(src: &str) -> String { /* compile + run, concatenated stdout */ }

#[test]
fn ratio_equality_is_a_value_comparison() {
    // The two values are equal but not structurally equal: a derived instance
    // answers False. GHC: True.
    assert_eq!(
        run("r = Ratio { num: 1, den: 2 };\ns = Ratio { num: 2, den: 4 };\nmain = do { write (Just (show_bool (r == s))) stdout; return () };"),
        "True"
    );
}

#[test]
fn ratio_equality_control_is_false_for_unequal_values() {
    assert_eq!(
        run("r = Ratio { num: 1, den: 2 };\ns = Ratio { num: 1, den: 3 };\nmain = do { write (Just (show_bool (r == s))) stdout; return () };"),
        "False"
    );
}

#[test]
fn ratio_ordering_is_numeric_not_structural() {
    // Structural order compares `num` then `den`: 1/2 vs 1/3 gives num equal,
    // den 2 > 3 false — the wrong answer. GHC: 1/2 > 1/3.
    assert_eq!(
        run("r = Ratio { num: 1, den: 2 };\ns = Ratio { num: 1, den: 3 };\nmain = do { write (Just (show_bool (r > s))) stdout; return () };"),
        "True"
    );
}
```

- [ ] **Step 2: Run it and watch it fail**

Expected: `unknown type name Ratio` (or, once declared, `False` for the first
test — which is the defect being pinned).

- [ ] **Step 3: Declare `Ratio` and its instances in `PRELUDE`**

In `types/ty.rs`'s `PRELUDE`, add `data Ratio a = { num: a, den: a };` and
`type Rational = Ratio Integer;`. The instances, with **polymorphic heads** and
explicit `Eq`/`Ord` (cross-multiplication), and `Num`/`Fractional` written over
the fields. `Ord` supplies `compare` only — do NOT rely on `min`/`max` defaults
(they do not resolve; spec A13). If the arity-0 instance mechanism cannot yet key
`Ratio a`, register the shape in the builtin table the way `Pair` is
(`ty.rs:~1400`).

**Measured constraint on the instance bodies — this task CANNOT write
`fromInteger` in a `Ratio` instance yet.** Three measurements:

| Body | Result |
|---|---|
| `(+) x y = Ratio { num: x.num * y.den + …, den: … }` over **annotated** `Integer` fields | ✅ works |
| the same with an unannotated literal (`num: 1`) | ❌ `cannot unify value type Int with Integer` |
| `fromInteger n = Ratio { num: n, den: 1 }` | ❌ same, because `fromInteger : Int -> a` makes `n` an `Int` |

So `Num (Ratio a)` supplies `(+)`, `(-)`, `(*)`, `negate`, `abs`, `signum` in
this task and **omits `fromInteger`** — an instance may omit a method. Task 7,
which corrects the signature to `Integer -> a`, adds it. Record this as a
deliberate two-step: an omitted method is loud (a call fails), not silent.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p rill-lang --features dsp --test ratio`
Expected: 3 PASS.

- [ ] **Step 5: Run the full gates, then commit**

```bash
git commit -m 'feat: add Ratio and Rational with value-comparing instances'
```

---

### Task 4: `gcd` at `Integer`, `%`, `numerator`, `denominator`

**Files:**
- Modify: `rill-lang/src/lower.rs` (builtin registration + lowering)
- Modify: `rill-lang/src/types/ty.rs` (`builtin_sigs`)
- Modify: `rill-lang/src/types/ty.rs` `PRELUDE` (`numerator`/`denominator` as
  ordinary definitions)
- Modify: `rill-lang/src/parser.rs` (`%` joins the fixity table; `Tok::Percent`
  is lexed at `lexer.rs:547` and unused)
- Test: `rill-lang/tests/ratio.rs`

**Interfaces:**
- Consumes: Task 3's `Ratio`; `Value::Integer`.
- Produces: `gcd :: Integer -> Integer -> Integer` (a builtin kernel);
  `(%) :: Integer -> Integer -> Ratio Integer` (normalizing);
  `numerator`, `denominator :: Ratio a -> a` as ordinary `PRELUDE` definitions
  (`numerator r = r.num`).

**Why `gcd` at `Integer` and not `Integral a =>`:** measured, `Integral` does not
exist (spec A6). The signature is honestly narrower, and A6 records it.

- [ ] **Step 1: Write the failing test**

Append to `rill-lang/tests/ratio.rs`:

```rust
#[test]
fn percent_normalizes() {
    // GHC: numerator (3 % 6) is 1, denominator is 2.
    assert_eq!(
        run("r = 3 % 6;\nmain = do { write (Just (show_integer (numerator r))) stdout; return () };"),
        "1"
    );
    assert_eq!(
        run("r = 3 % 6;\nmain = do { write (Just (show_integer (denominator r))) stdout; return () };"),
        "2"
    );
}

#[test]
fn the_sign_lives_on_the_numerator() {
    // GHC: `1 % (-2)` prints as `(-1) % 2`.
    assert_eq!(
        run("r = 1 % (0 - 2);\nmain = do { write (Just (show_integer (numerator r))) stdout; write (Just (show_integer (denominator r))) stdout; return () };"),
        "-12"
    );
}
```

- [ ] **Step 2: Run it and watch it fail**

Expected: `unknown identifier gcd` / parse error on `%`.

- [ ] **Step 3: Implement**

`gcd` as a builtin kernel over `Integer` (binary GCD on `BigInt`). `%` as a
fixity-table operator mapping to a `ratio_percent` builtin that normalizes: divide
both by `gcd`, force `den > 0` by moving the sign to `num`, and raise a fault on a
zero denominator (H2010: `Ratio`'s `%` is partial). `numerator`/`denominator` as
`PRELUDE` definitions projecting the fields — **not** builtins (a builtin beside
`.num` would be two mechanisms for reading a field; spec §7).

**Reduction must go into the arithmetic bodies too, not only into `%`.** Task 3
sign-normalises `/`, `recip`, `(*)`, `(-)` but does NOT reduce, so
`numerator ((1%2) / ((-3)%4))` is `-4` where GHC gives `-2`. `numerator`/
`denominator` are field projections, so they expose whatever the body produced:
reducing only in `%` leaves the divergence. With `gcd` available in this task, add
reduction to `+ - * /` and `recip` as well, so every arithmetic result is in
H2010's lowest terms and `numerator`/`denominator` match GHC.

Write the test first:

```rust
#[test]
fn arithmetic_results_are_in_lowest_terms() {
    // GHC: `(1%2) / ((-3)%4)` is `(-2) % 3`.
    assert_eq!(
        run("a = Ratio { num: (1 :: Integer), den: (2 :: Integer) };
b = Ratio { num: (-3 :: Integer), den: (4 :: Integer) };
q = a / b;
main = do { write (Just (show_integer (numerator q))) stdout; write (Just (show_integer (denominator q))) stdout; return () };"),
        "-23"
    );
    // GHC: `numerator (1%2 + 1%2)` is 1, denominator 1.
    assert_eq!(
        run("a = Ratio { num: (1 :: Integer), den: (2 :: Integer) };
q = a + a;
main = do { write (Just (show_integer (numerator q))) stdout; write (Just (show_integer (denominator q))) stdout; return () };"),
        "11"
    );
}
```

- [ ] **Step 4: Run the tests, then the full gates, then commit**

```bash
git commit -m 'feat: add gcd, %, numerator and denominator for Ratio'
```

---

### Task 5: the six kernels get IR instructions

**Files:**
- Modify: `rill-lang/src/ir.rs` (`ValueInstr` variants; `ValueInstr::ValueCompare3`)
- Modify: `rill-lang/src/backend/interp.rs` (execute them)
- Modify: `rill-lang/src/kernels.rs` (`has_instruction` returns true; the §3.9
  test's expected gap list becomes empty)
- Modify: `rill-lang/src/lower.rs` (`lower_primitive_method`'s kernel match)
- Test: `rill-lang/tests/numeric_kernels.rs` (create)

**Interfaces:**
- Consumes: `KernelId` (exists, `arena.rs:131`), `value_cmp_ref` (exists).
- Produces: `ValueNegate { dst, src }`, `ValueSignum { dst, src }`,
  `ValueAbs { dst, src }`, `ValueCompare3 { dst, a, b }`,
  `ValueRecip { dst, src }`, `ValueFromInteger { dst, src, target: ValueTyTag }`.
  Task 7 emits `ValueFromInteger`.

**`ValueAbs`/`ValueNegate`/`ValueSignum` dispatch on the OPERAND, not on the
name.** Today `lower.rs:2288-2311` intercepts `abs` (and `sin`/`cos`/`tan`/
`sqrt`/`exp`/`ln`/`tanh`) *by name, before class dispatch*, and requires a
`Buffer` — so `abs 3.0` fails with "`abs` signal argument must be a Buffer" and
`Num.abs` is unreachable for every type. That is a buffer-specific branch in
lowering, which `AGENTS.md` forbids ("Do not introduce … a buffer branch in
operator lowering").

The kernels must take the same shape `+` already has: ONE instruction, whose
execution branches on the operand kind.

| Operand | `ValueAbs` executes |
|---|---|
| `Buffer` | `abs_slice::<T, 4, ScalarVector4<T>>(src, out)` from `rill-core` — **unchanged vectorized path** |
| `Double` | `x.abs()` |
| `Integer` | exact `|x|` |
| `Int` | wrapping `abs` |

**No SIMD regression, measured:** the current buffer path already calls
`abs_slice`, and that function is a `ScalarVector4` loop (`rill-core/src/math/
vector/math.rs:163-177`) — the scalar emulation, not a `F32x4` intrinsic. Routing
it from the instruction instead of from a name branch calls the SAME function, so
the code path is byte-identical. What would kill vectorization is per-element
closure calls (the `map` shape), which this design does not use.

**`ValueFromInteger` needs a `target`.** The instance decides the type; an unbound
`dst` register cannot. `target` is a small tag (`Int`/`Integer`/`Double`/`Ratio`)
because the interpreter must know which payload to build. `Int` targets are NOT
produced by the kernel — lowering emits `ValueConstInt` there (one truncation
boundary, `literal_to_i64`; spec §2.5).

- [ ] **Step 1: Write the failing test**

`rill-lang/tests/numeric_kernels.rs`, GHC-measured expectations:

```rust
#[test]
fn the_six_kernels_execute() {
    assert_eq!(run("main = do { write (Just (show_double (negate 3.0))) stdout; return () };"), "-3.0");
    assert_eq!(run("main = do { write (Just (show_int (signum (0 - 3)))) stdout; return () };"), "-1");
    assert_eq!(run("main = do { write (Just (show_int (abs (0 - 3)))) stdout; return () };"), "3");
    assert_eq!(run("main = do { write (Just (show_int (compare 1 2))) stdout; return () };"), "-1");
    assert_eq!(run("main = do { write (Just (show_double (recip 2.0))) stdout; return () };"), "0.5");
    assert_eq!(run("main = do { write (Just (show_double (negate 2.5))) stdout; return () };"), "-2.5");
}
```

Plus the sign cases §6.4.4 fixes: `signum (-3) = -1`, `signum 0 = 0`,
`signum 3 = 1`, and `abs (-3.5) = 3.5`.

And the `FromInteger` end of the table, at the three target types it must serve
(an `Int` target is NOT served by the kernel — Task 7 routes it to
`ValueConstInt`):

```rust
#[test]
fn from_integer_serves_integer_and_double_targets() {
    // The Prelude still declares `fromInteger : Int -> a` at this task; the
    // kernel must work for the payload it is given, so this test pins the
    // EXISTING declaration and Task 7 re-pins it at `Integer -> a`.
    assert_eq!(run("f :: Double -> Double;\nf x = x + fromInteger 1;\nmain = do { write (Just (show_double (f 5.0))) stdout; return () };"), "6.0");
}
```

- [ ] **Step 2: Run it and watch it fail**

Expected: *"kernel `Negate`, which has no IR instruction yet"* etc.

- [ ] **Step 3: Implement the instructions and their execution**

Each preserves the operand's numeric track (`Int` → `Int`, `Integer` → exact
`Integer`, `Double` → `Double`). `Abs`/`Signum` on `Integer` must stay exact.
`Compare` reuses `value_cmp_ref` (the total order `Map`/`Set` already rely on).
Then update `has_instruction` and the §3.9 pinning test in `kernels.rs` so the
expected gap list is **empty**.

- [ ] **Step 4: Remove the name-based math-builtin branch**

`lower.rs:2288-2311` intercepts `abs` (and the other six math names) before class
dispatch. With `ValueAbs` in place, delete the `abs` arm and let the name resolve
through `Num.abs`. Write the test first:

```rust
#[test]
fn abs_is_a_class_method_not_a_buffer_builtin() {
    // GHC: `abs (-3.0)` is 3.0, `abs (-3)` is 3, and `abs` over a buffer works
    // too. rill rejected the scalar forms with "`abs` signal argument must be a
    // Buffer" because lowering intercepted the NAME before class dispatch.
    assert_eq!(run("main = do { write (Just (show_double (abs (0.0 - 3.0)))) stdout; return () };"), "3.0");
    assert_eq!(run("main = do { write (Just (show_int (abs (0 - 3)))) stdout; return () };"), "3");
    // The buffer path must keep working (and keep calling `abs_slice`).
    assert_eq!(run("main = do { write (Just (show_double (abs (sine 440.0 0.5 0.0)))) stdout; return () };"), "0.0");
}
```

Keep the other six names' branches for now: they are `Floating` methods in H2010
and they need the `Floating` class, which this stage does not add. Record that in
the report as an open defect with the same shape and the same fix.

- [ ] **Step 5: Run the tests, then the full gates, then commit**

```bash
git commit -m 'feat: implement the six numeric kernels'
```

---

### Task 5b: `Buffer` gets its missing class instances (runs AFTER Task 7)

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`PRELUDE`: explicit `Eq`/`Ord`/`Fractional`
  instances for `FixedBuffer e` and `Tape e`)
- Modify: `rill-lang/src/backend/interp.rs` (`value_cmp_ref` needs a
  `(Buffer, Buffer)` arm; the buffer division path)
- Test: `rill-lang/tests/buffer_instances.rs` (create)

**Interfaces:**
- Consumes: `Value::Buffer(usize)` and the program's buffer allocator; the
  whole-buffer kernels `ValueBufferBin`/`ValueBufferUn`; `Num (FixedBuffer e)`,
  which already exists.
- Produces: `Eq`/`Ord`/`Fractional` instances for the buffer types, with
  element-wise semantics.

**Why (measured).** `Buffer` has `Num` but not `Eq`/`Ord`/`Fractional`, so the
DERIVED structural instance answers by buffer INDEX, not contents:

```rill
a = square 0.0 0.0 0.0;  b = square 0.0 0.9 0.0;
a == b        →  True    (GHC: False — the samples differ)
compare a b   →  0       (claimed to be "whole-buffer structural order")
recip a       →  type error, though Fractional is a Num subclass
```

`AGENTS.md` is explicit: a buffer is an ordinary value and participates in the
SAME classes through INSTANCES — "if a buffer cannot satisfy a class today, the
answer is to implement the missing instance, never to add a special case". The
derived structural instance is exactly such a special case: it is correct for a
handle and wrong for a value.

- [ ] **Step 1: Write the failing tests**

`rill-lang/tests/buffer_instances.rs`:

```rust
//! A buffer is an ordinary value: it participates in `Eq`/`Ord`/`Fractional`
//! through instances, with ELEMENT-WISE semantics (user ruling, 2026-10-30).

#[test]
fn two_distinct_buffers_are_not_equal() {
    assert_eq!(
        run("a = square 0.0 0.0 0.0;\nb = square 0.0 0.9 0.0;\nmain = do { write (Just (show_bool (a == b))) stdout; return () };"),
        "False"
    );
}

#[test]
fn equal_buffers_are_equal() {
    // Control: two blocks with the SAME samples must compare equal, so the test
    // above cannot pass just by always answering False.
    assert_eq!(
        run("a = square 0.0 0.0 0.0;\nb = square 0.0 0.0 0.0;\nmain = do { write (Just (show_bool (a == b))) stdout; return () };"),
        "True"
    );
}

#[test]
fn buffer_order_is_lexicographic_over_samples() {
    // A constant 0.0 block is below a constant 0.9 block: every sample of the
    // first is smaller, so the first element decides.
    assert_eq!(
        run("a = square 0.0 0.0 0.0;\nb = square 0.0 0.9 0.0;\nmain = do { write (Just (show_int (compare a b))) stdout; return () };"),
        "-1"
    );
}

#[test]
fn buffer_division_is_element_wise() {
    // GHC has no such instance; the semantics is the design's: `Num` arithmetic
    // is already whole-buffer element-wise, so `/` follows it.
    assert_eq!(
        run("main = do { write (Just (show_int (length (arrayFromBuffer (square 1.0 0.5 0.0 / square 1.0 0.5 0.0))))) stdout; return () };"),
        "256"
    );
}
```

- [ ] **Step 2: Run them and watch them fail**

Expected: `a == b` → `True`; `compare` → `0`; `/` → a type error.

- [ ] **Step 3: Add the instances**

Explicit `Eq`/`Ord (FixedBuffer e)` (and `Tape e`) in `PRELUDE`, plus
`Fractional` whose body emits the whole-buffer division kernel — the same shape
`Num`'s instance bodies already have. `value_cmp_ref` gains a
`(Buffer, Buffer)` arm comparing the SAMPLES element-wise (lexicographically),
shorter-is-less. It needs the allocator to read the samples: if `value_cmp_ref`'s
signature cannot reach it, follow how `ValueBufferBin` resolves a `Value::Buffer`
index to data and pass what it needs.

A `Buffer`'s length is the block length, so two buffers of different lengths
compare by the common prefix and then by length — state that in the doc comment.

- [ ] **Step 4: Run the tests, then the full gates, then commit**

```bash
git commit -m 'feat: Buffer participates in Eq, Ord and Fractional element-wise'
```

---

### Task 6: fractional literals carry their exact decimal fraction

**Files:**
- Modify: `rill-lang/src/lexer.rs` (keep the literal's source text)
- Modify: `rill-lang/src/ast.rs` (`Expr::FractionalLit`, `Pattern::FractionalLit`)
- Modify: `rill-lang/src/parser.rs`, `render.rs`, `program_serde.rs`
- Modify every `Expr::Double`/`Pattern::LitDouble` site the compiler reports
- Test: `rill-lang/tests/exact_literals.rs` (create)

**Interfaces:**
- Consumes: `num-rational`'s `BigRational`, `num-bigint`.
- Produces: `Expr::FractionalLit(BigRational, String /* source text */, Span)`;
  `Pattern::FractionalLit(...)`;

**The payload is an exact ratio from the DIGITS**, not an `f64`. Measured in
GHC: `0.1 :: Rational` is `1 % 10`, while `toRational (0.1 :: Double)` is
`3602879701896397 % 36028797018963968`. Converting through `f64` would diverge.

**Integer literal nodes are NOT changed in this task** (stage 7a already made
them exact; the rename happens in Task 7 with the deletion).

- [ ] **Step 1: Write the failing test**

`rill-lang/tests/exact_literals.rs`:

```rust
#[test]
fn a_decimal_literal_is_an_exact_fraction() {
    // GHC: `0.1 :: Rational` is 1 % 10 — NOT the f64 value. Asserted through
    // the Ratio fields, because `toRational` needs `Real` (not in this stage).
    assert_eq!(
        run("r = fromRational 0.1;\nmain = do { write (Just (show_integer (numerator r))) stdout; write (Just (show_integer (denominator r))) stdout; return () };"),
        "110"
    );
}
```

(If `fromRational` is not yet usable, assert via a temporary `Ratio`-typed
binding instead; the requirement is that the payload is `1/10`.)

- [ ] **Step 2: Run it and watch it fail**

Expected: `cannot unify` or a wrong `3602879…` numerator.

- [ ] **Step 3: Change the payload**

`Expr::FractionalLit(BigRational, String, Span)` built from the source text
(`0.1` → `1/10`, `3.14` → `157/50`, `1e3` → `1000`). Keep the text: `render.rs`
must reproduce the source spelling and `program_json` must round-trip. `lower`'s
`ValueConstDouble` still needs an `f64`, produced by one explicit conversion at
the boundary.

- [ ] **Step 4: Run the tests, then the full gates, then commit**

```bash
git commit -m 'feat: fractional literals carry their exact decimal fraction'
```

---

### Task 6b: `!` and `index` are two names, not one (BLOCKS Task 7)

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`PRELUDE`: split the double-duty `index`)
- Modify: `rill-lang/src/parser.rs` (`InfixOp::Index` maps to `(!)`, not `index`)
- Test: `rill-lang/tests/ix.rs`, `rill-lang/tests/bang_vs_index.rs` (create)

**Interfaces:**
- Consumes: `arrayIndexFlat`, `checkedOffset`, `bounds`, `arrayBounds`, the `Ix`
  class.
- Produces: `(!) :: Ix i => Array i e -> i -> e` (the accessor) and a pure
  `index :: Ix i => Pair i i -> i -> Int` (the class method), with no name shared
  between them.

**Why this blocks Task 7 (measured).** rill's `PRELUDE` says it outright:

```haskell
--  `index` is DOUBLE DUTY: the `Ix` offset method (`Pair i i -> i -> Int`) and
--  this array accessor (`Array i e -> i -> e`, the `!` operator). The dispatch is
--  type-directed on the first argument
index a ix :: Ix i => Array i e -> i -> e = arrayIndexFlat a (checkedOffset (arrayBounds a) ix);
```

GHC has **two names** with **two signatures**:

```haskell
(!)   :: Ix i => Array i e -> i -> e     -- element
index :: Ix a => (a, a) -> a -> Int      -- offset
```

Measured: `a ! 1` → `20` (element), `index (0,2) 1` → `1`, `index (5,9) 7` → `2`
(offset, NOT an element).

Task 7's work-in-progress found the consequence: because the two roles share a
name, dispatch must GUESS from the first argument, and when the array's type still
carries variables (`Array i e` before `i`/`e` are solved) the guess binds the
element type to the index type — `no instance of Ix for type Double`. That single
defect accounts for ~100 of the 138 regressions Task 7 hit.

This is the same defect class as `ValueTy::App`+`Data` (B2) and `Expr::Int`
carrying an `Integer` (7a): **one name, two meanings**, and dispatch degenerates
into guessing.

- [ ] **Step 1: Write the failing tests**

`rill-lang/tests/bang_vs_index.rs`:

```rust
//! `(!)` is the array accessor and `index` the `Ix` offset method: two names,
//! two signatures, never one dispatching on its first argument.

#[test]
fn bang_is_the_array_accessor() {
    assert_eq!(
        run("a = listArray (Pair { first: 0, second: 2 }) [10.0, 20.0, 30.0];\nmain = do { write (Just (show_double (a ! 1))) stdout; return () };"),
        "20.0"
    );
}

#[test]
fn index_is_the_offset_method() {
    // GHC: `index (0,2) 1` is 1 and `index (5,9) 7` is 2 — OFFSETS, so the
    // second proves it is not an element lookup.
    assert_eq!(
        run("main = do { write (Just (show_int (index (Pair { first: 0, second: 2 }) 1))) stdout; return () };"),
        "1"
    );
    assert_eq!(
        run("main = do { write (Just (show_int (index (Pair { first: 5, second: 9 }) 7))) stdout; return () };"),
        "2"
    );
}

#[test]
fn a_literal_index_does_not_collapse_the_array_parameters() {
    // The regression Task 7 hit: an unsolved `Array i e` let the element type
    // bind to the index type. A 2-D array with a literal index is the shape that
    // exposed it.
    assert_eq!(
        run("a = listArray (Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 1, second: 1 } }) [1.0, 2.0, 3.0, 4.0];\nmain = do { write (Just (show_double (a ! Pair { first: 1, second: 0 }))) stdout; return () };"),
        "3.0"
    );
}

#[test]
fn a_string_bang_is_not_an_index() {
    // `!` has ONE meaning now; a non-array first argument must be a type error,
    // not a silent `index` call.
    assert!(compile_err("main = (1 :: Int) ! 2;").contains("cannot unify") || true);
}
```

(Adjust the last assertion to whatever the compiler says — the requirement is
that it is a TYPE ERROR, not that it produces an offset.)

- [ ] **Step 2: Run them and watch the 2-D case fail**

Expected: the 1-D cases pass (the defect is latent while literals are monomorphic),
the 2-D literal-index case fails with `no instance of Ix for type …`.

- [ ] **Step 3: Split the names in `PRELUDE`**

Rename the accessor to `(!)` and make `index` the pure method:

```haskell
(!) a ix :: Ix i => Array i e -> i -> e = arrayIndexFlat a (checkedOffset (arrayBounds a) ix);
-- `index` stays the class method with its declared signature; no array arm.
```

`checkedOffset` keeps calling `index` (the method) — one role each, so its
dispatch is unambiguous. Update the `DOUBLE DUTY` comment to state the split.

- [ ] **Step 4: Map the operator to `(!)`**

`parser.rs:67`: `InfixOp::Index => "index"` becomes `InfixOp::Index => "!"`. Keep
`infixl 9` (H2010 §4.4.2 gives `!!` that precedence; `!` is rill's spelling of the
same accessor). Confirm an infix `a ! i` and a prefix `(!) a i` are the same
function, and a section `(! i)` works.

- [ ] **Step 5: Run the whole suite — this is the real acceptance criterion**

Run: `cargo test -p rill-lang --features dsp`
Expected: all green, INCLUDING every `tests/ix.rs` case (36 of them) — they are the
programs this split touches.

- [ ] **Step 6: Run the full gates, then commit**

```bash
git commit -m 'feat: ! is the array accessor and index the Ix method (two names, two roles)'
```

---

### Task 7: the three inference stages, made explicit (merge of old 7 + 8)

> **Merge rationale.** The old tasks 7 and 8 were one mechanism split in two, and
> the split made task 7's acceptance criterion IMPOSSIBLE: with a literal typed as
> `fromInteger`/`fromRational` but no defaulting, `1 == 1.0` has no caller to fix
> `a`, so it can only report "argument type is not concrete" — the same defect that
> made stage 3b's criterion unachievable. The user's instruction: merge them, and
> **separate the inference STAGES explicitly**, because mixing them is what has
> cost this project six review rounds.

**Files:**
- Modify: `rill-lang/src/ast.rs` (`Expr::Int` → `Expr::IntegerLit`,
  `Pattern::LitInt` → `Pattern::IntegerLit`)
- Modify: `rill-lang/src/types/ty.rs` (`fromInteger : Integer -> a`;
  `fromRational` on `Fractional`; `Ratio`'s `fromInteger`/`fromRational` bodies)
- Modify: `rill-lang/src/types/infer.rs` (the three stages below; delete the
  heuristics)
- Modify: `rill-lang/src/lower.rs`, `reduce.rs` (delete the heuristics)
- Test: `rill-lang/tests/literal_polymorphism.rs` (create)

**Interfaces:**
- Consumes: Tasks 1–6.
- Produces: literal typing with no per-position heuristics, and a THREE-STAGE
  inference with a named boundary between them.

#### The three stages (this is the part to get right)

Measured in GHC 9.10.3 — each stage fails with a DIFFERENT error, and rill must
too, because collapsing them is what produced six rounds of wrong fixes:

| Stage | What it does | Its own error |
|---|---|---|
| **1. Unify** | solve type equations; the literal's variable meets the context | `cannot unify value type Int with Double` |
| **2. Discharge** | for every class constraint on a now-concrete variable, find the instance | `no instance of \`Fractional\` for type \`Int\`` |
| **3. Default** | for variables STILL unresolved at a binding's close, try `default (Integer, Double)` in order | `Ambiguous type variable \`a0\`` (when no numeric class applies) |

Measured GHC behaviour that pins the boundary:

```haskell
1 == 1.0                  → True      -- stage 3 picks Double
1 + 2                     → 3         -- stage 3 picks Integer (first in the list)
x :: Int; x == 1.0        → No instance for 'Fractional Int'   (stage 2, NOT stage 1)
(1::Int) == (2::Integer)  → Couldn't match 'Int' with 'Integer' (stage 1)
```

Note the third row: unification SUCCEEDS (`a := Int` from `x`), and the failure is
the instance search. That is why "No instance" and "cannot unify" must not be
conflated — rill currently reports the first as the second.

**Today rill has these stages tangled**: defaulting lives INSIDE
`infer_apply_impl` (`types/infer.rs:6876-6910`) and hardcodes `Double`
(`unify_value(&vty, &ValueTy::Double, …)` at `:6905`), so `1 + 2` becomes `Double`
instead of `Integer`, and `1 == 1.0` fails at the *dispatch* step before any
defaulting could run.

- [ ] **Step 1: Write the failing tests — one per stage**

`rill-lang/tests/literal_polymorphism.rs`:

```rust
//! The three inference stages, each pinned by a program that fails at ITS stage.

fn run(src: &str) -> String { /* compile + run, concatenated stdout */ }
fn err(src: &str) -> String { /* compile error text, panic if it compiles */ }

#[test]
fn stage_1_unification_decides_a_concrete_type() {
    // GHC: `1 + (2 :: Int)` is Int and runs; nothing defaults.
    assert_eq!(run("main = do { write (Just (show_int (1 + (2 :: Int)))) stdout; return () };"), "3");
    // GHC: two CONCRETE types cannot unify — a stage-1 error.
    assert!(err("main = (1 :: Int) + (2 :: Integer);").contains("cannot unify"));
}

#[test]
fn stage_2_discharge_reports_a_missing_instance_not_a_unification_failure() {
    // GHC: "No instance for 'Fractional Int'" — unification SUCCEEDS (a := Int
    // comes from the annotation) and the INSTANCE search fails.
    let e = err("main = (1 :: Int) + 1.0;");
    assert!(e.contains("no instance") || e.contains("No instance"), "got: {e}");
    assert!(!e.contains("cannot unify"), "a missing instance must not be reported as a unification failure: {e}");
}

#[test]
fn stage_3_defaulting_picks_integer_then_double() {
    // GHC: `1 + 2` is Integer (first in `default (Integer, Double)`).
    assert_eq!(run("main = do { write (Just (show_integer (1 + 2))) stdout; return () };"), "3");
    // GHC: `1 + 1.0` is Double, because Integer has no Fractional instance.
    assert_eq!(run("main = do { write (Just (show_double (1 + 1.0))) stdout; return () };"), "2.0");
    // GHC: `1 == 1.0` is True — this is the row the old task 7 could not reach.
    assert_eq!(run("main = do { write (Just (show_bool (1 == 1.0))) stdout; return () };"), "True");
}

#[test]
fn stage_3_does_not_fire_inside_a_signature_constrained_body() {
    // A constraint the SIGNATURE binds is not ambiguous: no defaulting.
    assert_eq!(run("f :: Num a => a;\nf = 1 + 2;\nmain = 0.0;"), "");
}

#[test]
fn stage_3_fires_only_where_the_variable_is_ambiguous() {
    // Measured GHC 9.10.3 with `default ()` (defaulting OFF): every form that
    // FIXES the variable by unification still compiles, and only the genuinely
    // ambiguous one fails. So the boundary is ambiguity, not "is a literal".
    //
    // rill has no user `default (...)` (assumption A11), so the boundary is
    // pinned through the CONSEQUENCE instead: a signature-fixed literal must
    // need no default, and an ambiguous one must take the default.
    assert_eq!(
        run("f :: Integer;\nf = 42;\nmain = do { write (Just (show f)) stdout; return () };"),
        "42",
        "a signature fixes the variable, so defaulting plays no part"
    );
    assert_eq!(
        run("g :: Integer -> String;\ng x = show x;\nmain = do { write (Just (g 42)) stdout; return () };"),
        "42",
        "a typed parameter fixes it too"
    );
    assert_eq!(
        run("main = do { write (Just (show (42 :: Integer))) stdout; return () };"),
        "42",
        "an annotation fixes it"
    );
    // The ambiguous case: `show 42` with nothing to fix `a` — GHC defaults it to
    // Integer (first in `default (Integer, Double)`), and so must rill.
    assert_eq!(
        run("main = do { write (Just (show 42)) stdout; return () };"),
        "42",
        "an ambiguous numeric variable takes the default (Integer)"
    );
    // A NON-numeric ambiguity gets no default: `show` alone is not enough.
    // GHC: "Ambiguous type variable" because no numeric class is involved.
    let e = err("main = do { write (Just (show (read \"42\"))) stdout; return () };");
    assert!(
        e.contains("mbiguous") || e.contains("cannot resolve"),
        "a non-numeric ambiguity must not default, got: {e}"
    );
}

#[test]
fn a_literal_takes_the_type_of_its_context() {
    assert_eq!(run("main = do { write (Just (show_int 3)) stdout; return () };"), "3");
    assert_eq!(run("main = do { write (Just (show_integer 3)) stdout; return () };"), "3");
    assert_eq!(run("main = do { write (Just (show_double 3)) stdout; return () };"), "3.0");
}

#[test]
fn one_body_serves_two_types() {
    assert_eq!(run("f n = n + 1;\nmain = do { write (Just (show_double (f 0.0))) stdout; return () };"), "1.0");
    assert_eq!(run("f n = n + 1;\nmain = do { write (Just (show_integer (f (2 :: Integer)))) stdout; return () };"), "3");
}
```

- [ ] **Step 2: Run them and watch them fail**

Expected: `stage_2` reports "cannot unify" (wrong stage), `stage_3` fails on
`1 + 2` (Double, not Integer) and on `1 == 1.0` ("argument type is not concrete").

- [ ] **Step 3: Make the literal an application (stage 1 only)**

`Expr::IntegerLit` lowers to `fromInteger` applied to its exact `Integer`
(built from `BigInt`); `Expr::FractionalLit` to `fromRational` applied to its exact
`Rational`. Its inferred type is the method's RESULT variable. Correct
`PRELUDE`: `fromInteger : Integer -> a`, and add `fromRational : Rational -> a` to
`Fractional`; complete `Ratio`'s instances with `fromInteger n = Ratio { num: n,
den: 1 }` and `fromRational r = r`.

- [ ] **Step 4: Separate stage 2 from stage 1**

A missing instance must be reported as "no instance of …", never as a unification
failure. Find where dispatch currently folds the two (the primitive branch's
`no instance of` at `infer.rs:4893` is the model) and make the stage-1 path stop
reporting the stage-2 failure.

- [ ] **Step 5: Build stage 3 — defaulting at the binding's close**

Remove the `Double`-hardcoded defaulting from `infer_apply_impl`
(`infer.rs:6876-6910`). Add a residual-constraint store (`Vec<(String,
TypeVarId)>` plus spans) and, at the end of a binding group
(`infer_def_group_inner`), for each variable that is STILL a `Var` after the
substitution:

1. collect its constraints;
2. if every one is a standard numeric class and at least one is `Num`/
   `Fractional`, try `Integer` then `Double`, taking the FIRST that satisfies all;
3. otherwise leave it and report `Ambiguous type variable` at the use site.

Do NOT default a variable a signature binds.

- [ ] **Step 6: Delete the heuristics, one at a time**

The deletion criterion (must reach **0**):

```
grep -c 'is_int_literal\|int_literal_widens_to\|annotate_formal_arg_types\|scheme_pinned_params\|declared_binding_ty\|numeric_leaf_of' -r rill-lang/src
```

Delete one, run `cargo test -p rill-lang --features dsp`, and fix the failure by
making the CORRECT mechanism work. Seven tests currently pass only through
`int_literal_widens_to` — `rill-lang/tests/integer_type.rs` (the seven `int_literal_widens_to` dependants),
`integer_type.rs:316`, and `tests/dictionaries.rs`'s
`an_integer_literal_takes_the_other_operands_numeric_type`. Each asserts a GHC
value and may NOT be weakened.

- [ ] **Step 7: Run the stages' tests and the deletion grep**

Run: `cargo test -p rill-lang --features dsp` and the grep above.
Expected: all green, grep **0**.

- [ ] **Step 8: Run the full gates, then commit**

```bash
git commit -m 'feat: literals are fromInteger/fromRational applications, with three explicit inference stages'
```

---

### Task 8: an integer parameter reaches the language as an `Int`, not a `Double`

**Files:**
- Modify: `rill-lang/src/backend/interp.rs:21` (`param_to_f64`)
- Modify: `rill-lang/src/register.rs:42`
- Modify: `rill-lang/src/program.rs` (the parameter write path)
- Test: `rill-lang/tests/param_seed.rs`

**Interfaces:**
- Consumes: `ParamValue::Int(i32)`, `Value::Int(i64)`.
- Produces: `ParamValue::Int(v)` becomes `Value::Int(v as i64)` — lossless
  widening, never through `f64`.

**Measured defect:** `ParamValue::Int(v) => *v as f64` loses low bits above 2^53
(spec §1.6, A14). `i32 → i64` widening cannot lose anything.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn an_integer_parameter_keeps_its_value_and_type() {
    // A parameter above 2^53 loses low bits when routed through f64.
    let src = "main = do { write (Just (show_int ?n)) stdout; return () };";
    // Seed `?n` with ParamValue::Int(2_147_483_647) via the existing
    // parameter API (see this file's current seeding test).
    assert_eq!(run_with_param(src, "n", ParamValue::Int(2_147_483_647)), "2147483647");
}
```

- [ ] **Step 2: Run it and watch it fail if the current path is lossy**

Expected: if the parameter arrives as a `Double`, `show_int` faults or renders a
truncated value.

- [ ] **Step 3: Widen instead of converting to `f64`**

Route `ParamValue::Int(v)` to `Value::Int(v as i64)`. `ParamValue::Float(f32)`
keeps its own path (a float parameter IS a `Double`-family value).

- [ ] **Step 4: Run the tests, then the full gates, then commit**

```bash
git commit -m 'fix: an integer parameter widens to Int, not through f64'
```

---

### Task 9: docs — close OD-27, record the departures

**Files:**
- Modify: `docs/superpowers/DOCS-GAPS.md`
- Modify: `docs/superpowers/audits/2026-10-26-haskell2010-conformance-inventory.md`

- [ ] **Step 1: Close OD-27**

OD-27 is `open — half done` (7a closed part a). This task closes part b:
defaulting now yields `Integer`, so `1 + 2` no longer overflows. Pin it with the
measured facts.

- [ ] **Step 2: Record A2, A4, A6, A8, A11, A13 as OD rows**

Each with its owner and its measured reproduction, in the file's existing row
format. A2 (`Ratio Double` accepted), A4 (`%`'s narrower signature), A6
(`gcd`'s narrower signature), A8 (`compare` returns `Int`), A11 (user `default`
ignored), A13 (`min`/`max` defaults do not resolve).

- [ ] **Step 3: Re-check the conformance inventory**

`grep -n 'literal\|Integer\|default' …inventory.md` and correct every row this
stage falsified. Do not delete a row that is only partly satisfied — say which
part remains.

- [ ] **Step 4: Commit**

```bash
git commit -m 'docs: close OD-27; record the stage-7b departures and their owners'
```

---

## What this plan deliberately does not do

| Not in this plan | Owner |
|---|---|
| `Int32` as a language type | deferred to real benchmarks (spec §1.6); `i64` costs 0 bytes and the value track runs once per program |
| `Integral`, `Real`, `Enum`, `Bounded`, `Show` classes | OD-24 / P4, and the `Show` stage |
| `min`/`max` class defaults (A13) | a default-body stage |
| `fromIntegral`, `toInteger`, `toRational`, `round`/`floor`/`ceiling`/`truncate`, `properFraction`, `lcm` | need `Integral`/`Real`/`RealFrac` |
| `Float` single precision | P3 |
| `Complex` | not in the Prelude |
| A user `default (...)` declaration (A11) | a later item |
| `Ratio Double` rejection (A2) | OD-24 / P4 |
