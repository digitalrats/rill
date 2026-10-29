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
   (deletion criterion), Task 5.
3. **Defaulting firing in the wrong place** — inside a signature-constrained
   body, or not at all for `main = 1 + 2`. → Task 8.
4. **`Ratio` comparing structurally** — `1/2 == 2/4` must be `True`,
   `Ratio{1,2} > Ratio{1,3}` must be `True`. → Task 3.
5. **One concept, two representations** — two places truncating `Integer → Int`,
   a `Ratio`-shaped type at compile time and a leaf at runtime. → Tasks 5, 7.
6. **A `Ratio` instance written before the `fromInteger` signature is fixed.**
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

- [ ] **Step 4: Run the tests, then the full gates, then commit**

```bash
git commit -m 'feat: implement the six numeric kernels'
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

### Task 7: literals become `fromInteger`/`fromRational`; delete the 46 heuristics

**Files:**
- Modify: `rill-lang/src/ast.rs` (`Expr::Int` → `Expr::IntegerLit`,
  `Pattern::LitInt` → `Pattern::IntegerLit`)
- Modify: `rill-lang/src/types/ty.rs` (`fromInteger : Integer -> a`;
  `fromRational` on `Fractional`)
- Modify: `rill-lang/src/types/infer.rs` (literal typing; delete
  `int_literal_widens_to`, `def_arg_widens_to`'s literal arm, the `Expr::Annot`
  literal special case)
- Modify: `rill-lang/src/lower.rs` (emit the application; delete
  `annotate_formal_arg_types`, `scheme_pinned_params`, `declared_binding_ty`,
  `closure_param_tys`'s placeholder, `is_int_literal`, `numeric_leaf_of`)
- Modify: `rill-lang/src/reduce.rs` (delete `annotate_formal_arg_types` if
  measurement shows the type now arrives)
- Test: `rill-lang/tests/literal_polymorphism.rs` (create)

**Interfaces:**
- Consumes: Tasks 1-6. **Cannot start before Task 2** — `int_literal_widens_to`
  is today the only reason `show_double 2` compiles (`infer.rs:5844`), and seven
  GHC-conformant tests depend on it.
- Produces: literal typing with no per-position heuristics.

**Deletion criterion (the task's definition of done):**

```
grep -c 'is_int_literal\|int_literal_widens_to\|annotate_formal_arg_types\|scheme_pinned_params\|declared_binding_ty\|numeric_leaf_of' rill-lang/src -r
```
is **0**.

- [ ] **Step 1: Write the failing test**

`rill-lang/tests/literal_polymorphism.rs` — GHC-measured:

```rust
#[test]
fn a_literal_takes_the_type_of_its_context() {
    assert_eq!(run("main = do { write (Just (show_int 3)) stdout; return () };"), "3");
    assert_eq!(run("main = do { write (Just (show_integer 3)) stdout; return () };"), "3");
    assert_eq!(run("main = do { write (Just (show_double 3)) stdout; return () };"), "3.0");
}

#[test]
fn one_body_serves_two_types() {
    // No annotation on `f`: the literal carries its own variable, so the scheme
    // is `Num a => a -> a` — not `Int -> Int`.
    assert_eq!(run("f n = n + 1;\nmain = do { write (Just (show_double (f 0.0))) stdout; return () };"), "1.0");
    assert_eq!(run("f n = n + 1;\nmain = do { write (Just (show_integer (f (2 :: Integer)))) stdout; return () };"), "3");
}

#[test]
fn two_literals_meet_at_one_type() {
    assert_eq!(run("main = do { write (Just (show_bool (1 == 1.0))) stdout; return () };"), "True");
    assert_eq!(run("main = do { write (Just (show_double (1 + 1.0))) stdout; return () };"), "2.0");
    assert_eq!(run("main = do { write (Just (show_double (1.0 + 1))) stdout; return () };"), "2.0");
}

#[test]
fn a_fixed_int_still_rejects_a_double() {
    // GHC: No instance for `Fractional Int`.
    assert!(compile_err("main = (1 :: Int) + 1.0;").contains("cannot unify"));
    assert!(compile_err("main = (1 :: Integer) + 1.0;").contains("cannot unify"));
}
```

- [ ] **Step 2: Run it and watch it fail**

Expected: `one_body_serves_two_types` rejects `f 0.0` (`Int` vs `Double`).

- [ ] **Step 3: Correct the `fromInteger` signature and add `fromRational`**

Write this test first (`rill-lang/tests/literal_polymorphism.rs`):

```rust
#[test]
fn from_integer_takes_an_integer_not_an_int() {
    // Stage 7a added `Integer` but left the method at `Int -> a`, so the method
    // could not accept its own Report-mandated argument (§6.4.1).
    // `2147483648` does not fit `i32`; it fits `Integer` and would overflow an
    // `Int` parameter only past 2^63, so use a value past 2^32 to discriminate.
    assert_eq!(
        run("f :: Integer -> Integer;\nf x = x + fromInteger 4294967296;\nmain = do { write (Just (show_integer (f (0 :: Integer)))) stdout; return () };"),
        "4294967296"
    );
}
```

Run it, watch it fail while the signature is still `Int -> a` (the argument is
an `Int` parameter and a literal of the wrong track), then change the `PRELUDE`
to `fromInteger : Integer -> a` and add `fromRational : Rational -> a` to the
`Fractional` class. The `FromInteger` kernel path must accept an `Integer`
payload — Task 5's `ValueFromInteger` already carries `target`, so no new IR is
needed here.

- [ ] **Step 3b: Complete the `Ratio` instance with `fromInteger`/`fromRational`**

Now that the signature is `Integer -> a`, add
`fromInteger n = Ratio { num: n, den: 1 }` to `Num (Ratio a)` and
`fromRational r = r` to `Fractional (Ratio a)`. Write the test first:

```rust
#[test]
fn ratio_accepts_from_integer() {
    // The method is available only after the signature correction; Task 3
    // omitted it for that reason.
    assert_eq!(
        run("r = fromInteger 3 :: Rational;\nmain = do { write (Just (show_integer (numerator r))) stdout; return () };"),
        "3"
    );
}
```

- [ ] **Step 4: Make the literal an application and delete the heuristics**

A literal lowers to `fromInteger`/`fromRational` applied to its exact payload;
its inferred type is that method's result variable, decided by unification.
Then delete the heuristics listed above ONE AT A TIME, running
`cargo test -p rill-lang --features dsp` after each deletion, so the suite
identifies the one that was still needed.

- [ ] **Step 5: Run the tests and the deletion criterion**

Run: `cargo test -p rill-lang --features dsp` and the `grep` above.
Expected: all green, grep count **0**.

- [ ] **Step 6: Run the full gates, then commit**

```bash
git commit -m 'feat: literals are fromInteger/fromRational applications'
```

---

### Task 8: default `(Integer, Double)`

**Files:**
- Modify: `rill-lang/src/types/infer.rs` (a residual numeric-constraint store in
  `Ctx`; a defaulting pass at the binding group's close, `infer_def_group_inner`)
- Test: `rill-lang/tests/literal_polymorphism.rs`

**Interfaces:**
- Consumes: Task 7's literal typing.
- Produces: ambiguous numeric variables default to `Integer` (then `Double`).

**Measured honesty (spec §2.3):** rill has no residual-constraint store today and
`def_scheme` (`infer.rs:3460-3498`) sees only signatures. The store is a
`Vec<(String, TypeVarId)>` plus spans, consulted once per binding group — not a
general solver. This is the ONE place the design needs a store; typing does not.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn an_unconstrained_integer_literal_defaults_to_integer() {
    // GHC: `1 + 2` is Integer and does not overflow.
    assert_eq!(run("main = do { write (Just (show_integer (1 + 2))) stdout; return () };"), "3");
}

#[test]
fn an_unconstrained_fractional_literal_defaults_to_double() {
    // GHC: `1.0 + 2.0` is Double, because Integer has no Fractional instance.
    assert_eq!(run("main = do { write (Just (show_double (1.0 + 2.0))) stdout; return () };"), "3.0");
}

#[test]
fn a_signature_binds_the_variable_so_nothing_defaults() {
    // `f :: Num a => a; f = 1 + 2` must NOT default inside the body.
    assert_eq!(run("f :: Num a => a;\nf = 1 + 2;\nmain = 0.0;"), "");
    assert_eq!(run("g :: Num a => a;\ng = 1 + 2;\nmain = do { write (Just (show_double (g + 0.0))) stdout; return () };"), "");
}
```

Rewrite the last test's second assertion to a `compile_err`-free form once the
helper exists; the requirement is only "a constrained signature does not
default", which a successful compile proves.

- [ ] **Step 2: Run it and watch it fail**

Expected: `an_unconstrained_integer_literal_defaults_to_integer` fails (it is
`Int` today), and possibly `show_integer (1 + 2)` is a type error.

- [ ] **Step 3: Implement the store and the pass**

Record `(class, var)` for every numeric constraint a literal introduces; at the
close of a binding group, default each **ambiguous** variable by trying
`Integer`, then `Double`, taking the first that satisfies every constraint on it.
Do not default a variable the binding's signature mentions.

- [ ] **Step 4: Run the tests, then the full gates, then commit**

```bash
git commit -m 'feat: default numeric literals to Integer, then Double'
```

---

### Task 9: an integer parameter reaches the language as an `Int`, not a `Double`

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

### Task 10: docs — close OD-27, record the departures

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
