# rill-lang: `Integer` — the arbitrary-precision integer type (stage 7a)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `Integer` a real type in rill — Haskell 2010's `Integral`-class
arbitrary-precision integer (`Report` §6.4, Table 6.1) — so that
`fromInteger :: Integer -> a` (stage 7b) has a target type and integer
defaulting can stop diverging.

**Architecture:** `Integer` joins `Int`/`Double` as a **registered scalar**
(`TypeEntry::Scalar`) with its own `ValueTy` leaf and its own `Value` payload
backed by `num_bigint::BigInt`. It is not a data type and gets no constructor:
it is a numeric leaf, exactly like `Int`. Its `Eq`/`Ord` instances are
**derived by the existing registry pass** (`TypeEnv::derive_eq_ord` walks
`scalars`), never hand-written; its `Num` instance is a primitive marker in
`PRELUDE`, exactly like `instance Num Int where { }`.

**Tech Stack:** Rust, `num-bigint` 0.4 (new workspace dependency),
rill-lang's existing arena / IR / interpreter.

**Spec:** `docs/superpowers/specs/2026-10-25-rill-lang-runtime-dictionaries-design.md`
(§3.6 literal typing, §3.9 missing kernels) and `docs/superpowers/DOCS-GAPS.md`
**OD-27** (the defect this plan removes).

## Global Constraints

- Haskell 2010 / GHC behaviour is the acceptance criterion. **No new divergence
  may be introduced, and an existing one may not be documented instead of
  fixed.**
- All crate-level documentation, doc comments and inline comments are in
  **English**.
- `Int` and `Integer` are **two distinct types**. `(+) :: Num a => a -> a -> a`
  takes ONE `a`, so `(1 :: Int) + (2 :: Integer)` stays a **type error**
  (GHC: `Couldn't match expected type 'Int' with actual type 'Integer'`).
  Do **not** add any implicit widening between them.
- `Integer` has **no `Bounded` instance** (GHC: `No instance for 'Bounded
  Integer'`). rill has no `Bounded` class at all, so nothing to add — but do not
  register `maxBound`-like machinery for `Integer`.
- `Integer` is **not** `Fractional`: `(/)` must not accept it.
- Gates for every task (never `--all-targets`):
  - `cargo test -p rill-lang --features dsp`
  - `cargo clippy -p rill-lang --features dsp`
  - `cargo fmt --check`
  - `cargo test -p rill-lang --features "serialization dsp" --test program_json`
- Commit messages are one line, in single quotes, e.g.
  `'feat: add Integer value variant and registry entry'`.
- Every new fact about a type name goes through the **one registry**
  (`TypeEnv::type_entry` / `scalars`). Do not add a `matches!` list of type
  names anywhere.
- No debug leftovers: no `std::env::var`, no `eprintln!`, no `zz_*.rs` test
  files. Delete every probe before committing.

## Review Focus

- **Overflow direction:** `Int` arithmetic wraps (`i64`); `Integer` arithmetic
  must be **exact**. A test must show `Int` wrapping and `Integer` not, from the
  same source shape.
- **Distinctness:** a program mixing `Int` and `Integer` at one operator must
  still be a type error. A test pins this; a helper that "helps" unification is
  the likely regression.
- **Rendering:** `show_integer` must print the exact decimal expansion (no `f64`,
  no scientific notation, no `e`/`inf`).
- **Cross-kind ordering:** `value_cmp_ref` is a total order used by `Map`/`Set`
  keys. Adding a variant requires a `kind_rank` rank, and two `Integer`s must
  order numerically (not lexicographically, not by sign-magnitude bytes).
- **Arena hygiene:** `Value::Integer` is a **leaf**. If it becomes an RC child,
  `drop_ref`/`clone_pooled`/`visit_refs`/`detach_payload` will leak or
  double-free.

---

### Task 1: `Integer` is a working numeric type

**Files:**
- Modify: `Cargo.toml` (workspace `[workspace.dependencies]`)
- Modify: `rill-lang/Cargo.toml`
- Modify: `rill-lang/src/arena.rs` (`ValueKind`, `Value`, `kind()`, `visit_refs`,
  `clone_pooled`, in-file tests)
- Modify: `rill-lang/src/types/ty.rs` (`ValueTy`, the scalar seed, `PRELUDE`,
  `value_type_key`, `scalar_name_of` coverage)
- Modify: `rill-lang/src/types/unify.rs` (the leaf-equality arm)
- Modify: `rill-lang/src/types/ffi.rs` (explicit `Integer` arm: not an FFI
  scalar)
- Modify: `rill-lang/src/types/infer.rs` (the `infer_expr_expected` literal arm;
  the `validate_type_names` comment at ~line 2600 still lists `Integer` as an
  example of an unknown type — it is a type now and that example must name a
  genuinely undeclared one)
- Modify: `rill-lang/src/ir.rs` (`ValueInstr::ValueConstInteger`,
  `ValueBuiltinOp::ShowInteger`, its existence test)
- Modify: `rill-lang/src/backend/interp.rs` (`ArithmeticOperand`,
  `arithmetic_operand`, `exec_value_arith`, `value_cmp_ref`, `kind_rank`,
  `ValueConstInteger`, `ShowInteger`)
- Modify: `rill-lang/src/lower.rs` (literal → `Integer` under an `:: Integer`
  annotation, the arithmetic result track, unary minus, `show_integer` builtin
  registration/dispatch/result type)
- Create: `rill-lang/tests/integer_type.rs`

**Interfaces:**
- Consumes: `TypeEnv::register_scalar(name, ScalarType)`; `ScalarType { vty,
  ffi_scalar, buffer_element }`; `TypeEnv::derive_eq_ord` (already iterates
  `self.scalars`, so a registered scalar gets `Eq`/`Ord` for free);
  `kernels::kernel_for_method("Num", "+")` → `KernelId::Add`.
- Produces:
  - `ValueTy::Integer` — the type leaf.
  - `Value::Integer(BigInt)` and `ValueKind::Integer`.
  - `ArithmeticOperand::Integer(BigInt)`.
  - `ValueInstr::ValueConstInteger { dst: usize, value: Box<BigInt> }`.
  - `ValueBuiltinOp::ShowInteger`.
  - Source-visible: type name `Integer`; `instance Num Integer`; builtin
    `show_integer : Integer -> String`.

**Regression risk to check by measurement, not assumption:** the `Value` enum is
48 bytes today (`Value::Dictionary` sizes it). `BigInt` is 32 bytes, so
`Value::Integer(BigInt)` must stay within 48 — the test in step 1 records the
measured size and asserts it does not grow.

- [ ] **Step 1: Write the failing arena test**

In `rill-lang/src/arena.rs`, inside the existing `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn integer_is_a_leaf_value_of_the_same_size_class() {
    use num_bigint::BigInt;
    // `Integer` is a numeric LEAF: it owns no arena children.
    let v = Value::Integer(BigInt::from(1_000_000_000_000_000_000_i64));
    assert_eq!(v.kind(), ValueKind::Integer);
    let mut seen: Vec<ArenaRef> = Vec::new();
    v.visit_refs(|r| seen.push(r));
    assert!(seen.is_empty(), "an Integer must own no arena refs");
    // The payload is 32 bytes; the enum must not grow past its current 48.
    assert!(
        std::mem::size_of::<Value>() <= 48,
        "Value must not grow: {} bytes",
        std::mem::size_of::<Value>()
    );
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p rill-lang --features dsp --lib arena::tests::integer_is_a_leaf_value_of_the_same_size_class`
Expected: FAIL — `no variant or associated item named 'Integer'`.

- [ ] **Step 3: Add the dependency**

`Cargo.toml` (workspace root), inside `[workspace.dependencies]`, next to
`num-traits = "0.2"`:

```toml
num-bigint = "0.4"
```

`rill-lang/Cargo.toml`, inside `[dependencies]`, next to `indexmap = "2"`:

```toml
num-bigint = { workspace = true, features = ["serde"] }
```

(The `serde` feature on `num-bigint` is required for Task 2's AST payload; it is
harmless in Task 1 and keeps the `serialization` gate green throughout.)

- [ ] **Step 4: Add `ValueKind::Integer` and `Value::Integer`**

In `arena.rs`, in `enum ValueKind`, after `Double`:

```rust
    /// Arbitrary-precision integer value (`Integer`).
    Integer,
```

In `enum Value`, after `Double(f64)`:

```rust
    /// Arbitrary-precision integer payload — Haskell 2010 `Integer`.
    ///
    /// A leaf: the payload is boxed-in-enum (`BigInt` is 32 bytes) and owns no
    /// arena children, exactly like [`Value::Int`]. `Int` and `Integer` are
    /// DISTINCT types; nothing widens one into the other implicitly.
    Integer(BigInt),
```

Add `use num_bigint::BigInt;` with the other `use` lines at the top of the file.

`Value::kind`: `Value::Integer(_) => ValueKind::Integer,` next to `Value::Int`.

`visit_refs`: `Integer` owns nothing. The existing catch-all `_ => {}` already
covers it; do **not** add an arm that visits anything. Add a comment beside
`Value::ThunkCell(_) | Value::Bottom | Value::Nil => {}` naming `Integer` as a
leaf so the next reader does not have to check.

`clone_pooled`: `Integer` takes no pooled payload buffer — the existing
`None` shape path returns `Ok(self.clone())`, which is correct. No edit needed;
verify by reading the function.

- [ ] **Step 5: Add `ValueTy::Integer`**

In `types/ty.rs`, in `enum ValueTy`, after `Double`:

```rust
    /// Arbitrary-precision integer value (`Integer`), Haskell 2010 §6.4.
    ///
    /// A numeric LEAF like [`ValueTy::Int`] — not a `Con`. It is registered in
    /// the one type registry, so its `Eq`/`Ord` instances are derived by the
    /// registry pass rather than declared anywhere.
    Integer,
```

The registry seed in `TypeEnv::with_builtins` (the `for (name, vty, ffi_scalar,
buffer_element)` loop) gains one row:

```rust
            // Haskell 2010 §6.4: `Integer` is the arbitrary-precision integer
            // type. It is NOT an FFI scalar (no host C type denotes a bignum)
            // and NOT a buffer element (samples are doubles).
            ("Integer", ValueTy::Integer, false, false),
```

`value_type_key`: add `ValueTy::Integer` to the
`ValueTy::Int | ValueTy::Double | ValueTy::String | ValueTy::Void` arm — the key
comes from `env.scalar_name_of`, so it resolves to `"Integer"` with no new
literal name in the code.

`type_name_of_vty`: same — add `ValueTy::Integer` to the scalar arm.

- [ ] **Step 6: One unification rule**

In `types/unify.rs`, extend the leaf-equality arm:

```rust
        (ValueTy::Int, ValueTy::Int)
        | (ValueTy::Double, ValueTy::Double)
        | (ValueTy::Integer, ValueTy::Integer) => Ok(()),
```

Add no rule between `Int` and `Integer` — they are distinct types, and their
absence is what makes `(1 :: Int) + (2 :: Integer)` a type error.

In `types/ffi.rs`, `ffi_record_schema`'s field match: `Integer` contributes **no**
FFI parameter (a record field of `Integer` type cannot be a `f32`/`i64` host
parameter). Add an explicit arm with the reason, rather than letting the
catch-all `_ => {}` hide it:

```rust
            // `Integer` has no host scalar representation: a bignum record
            // field cannot cross the FFI boundary as an f32/i64 parameter.
            ValueTy::Integer => {}
```

- [ ] **Step 7: `instance Num Integer` in the Prelude**

In `types/ty.rs`, beside `instance Num Int where { }`:

```rust
instance Num Integer where { }
```

Leave `Eq`/`Ord` derived (`derive_eq_ord` iterates `scalars`, so `Integer` is
included automatically). Do **not** write `instance Fractional Integer`.

- [ ] **Step 8: Integer-preserving arithmetic in the interpreter**

`backend/interp.rs`:

```rust
enum ArithmeticOperand {
    Int(i64),
    Integer(BigInt),
    Double(f64),
    Bottom,
    Absent,
}
```

`arithmetic_operand`: add
`Some(Value::Integer(i)) => ArithmeticOperand::Integer(i.clone()),`.

`exec_value_arith`: before the existing `(Int, Int)` fast path, add an exact
path:

```rust
    // Exact `Integer` `+`/`-`/`*`: no wrapping, no f64 round-trip. `/` is not
    // available — `Integer` has no `Fractional` instance, so the typed front
    // end never reaches here with `Div`.
    if let (ArithmeticOperand::Integer(x), ArithmeticOperand::Integer(y)) = (&av, &bv) {
        let exact = match op {
            ValueArith::Add => Some(x + y),
            ValueArith::Sub => Some(x - y),
            ValueArith::Mul => Some(x * y),
            ValueArith::Div => None,
        };
        if let Some(v) = exact {
            world.value_regs[dst] = alloc_owned(ir, world, Value::Integer(v));
            return;
        }
    }
```

The `unreachable!` arms that map `ArithmeticOperand` to `f64` must gain an
`Integer` arm. Do **not** silently widen it: an `Integer` reaching the `f64`
path is a typing bug, so make it a fault, not a number:

```rust
        // An `Integer` never mixes with a floating operand: the type system
        // keeps the two apart, so reaching here is a lowering bug. Latch a
        // fault rather than rounding a bignum into an f64.
        ArithmeticOperand::Integer(_) => {
            latch_fault(
                ir,
                world,
                "arithmetic mixes an Integer operand with a floating one",
            );
            world.value_regs[dst] = None;
            return;
        }
```

- [ ] **Step 9: `ValueConstInteger` and `ShowInteger`**

`ir.rs`, after `ValueConstInt`:

```rust
    /// Create an `Integer` value (Haskell 2010 arbitrary-precision integer).
    ValueConstInteger {
        /// Destination value register.
        dst: usize,
        /// The value. Boxed: an `Integer` payload is wider than a register.
        value: Box<BigInt>,
    },
```

`ValueBuiltinOp` gains `ShowInteger,` after `ShowInt`, and the
`value_builtin_ops_exist` test's array gains `ValueBuiltinOp::ShowInteger,` and
its `assert_eq!(ops.len(), 34)` becomes `35`.

`interp.rs`: `ValueInstr::ValueConstInteger { dst, value }` allocates
`Value::Integer((**value).clone())` into `dst` (drop the previous occupant into
`drops` first, exactly like `ValueConstInt`).

`ShowInteger` renders the exact decimal expansion:
`value_regs[base + args[0]]` → force → `Value::Integer(i)` → `Value::String(i.to_string())`;
a non-`Integer` operand is `None` (no truncation fallback — `show_integer` is
typed `Integer -> String`).

`kind_rank`: append

```rust
        // `Integer` is a fresh variant; appended so every existing rank keeps
        // its cross-kind meaning.
        Integer(_) => 20,
```

`value_cmp_ref`: add, beside `(Int(x), Int(y))`:

```rust
        // Numeric order, exactly like `Int` — not sign-magnitude byte order.
        (Integer(x), Integer(y)) => x.cmp(y) as i8,
```

`lower.rs` — **the arithmetic result track**. In `lower_primitive_method`'s
`KernelId::Add | Sub | Mul | Div` arm the result type is decided today by:

```rust
let ret = if matches!(arg_tys.first(), Some(ValueTy::Int))
    && matches!(arg_tys.get(1), Some(ValueTy::Int))
{
    ValueTy::Int
} else {
    ValueTy::Double
};
```

That `else` would type `(2 :: Integer) + (3 :: Integer)` as `Double` while the
kernel produces `Value::Integer` — a statically-`Double` slot holding a bignum.
Extend it so each numeric track keeps its own type:

```rust
                // The result track follows the operands' ONE numeric type: two
                // `Int`s stay `Int`, two `Integer`s stay `Integer`, everything
                // else is `Double`. A mixed pair is unreachable — `(+)`'s `Num a
                // => a -> a -> a` rejects it — so the fallback is the existing
                // Double case, not a widening.
                let ret = match (arg_tys.first(), arg_tys.get(1)) {
                    (Some(ValueTy::Int), Some(ValueTy::Int)) => ValueTy::Int,
                    (Some(ValueTy::Integer), Some(ValueTy::Integer)) => ValueTy::Integer,
                    _ => ValueTy::Double,
                };
```

`lower.rs` — **unary minus on `Integer`**. `Expr::Neg` has two paths: a constant
fold (`const_i64`/`const_f64`) and a runtime `zero - inner`. The runtime path
currently picks `ValueConstInt` vs `ValueConstDouble` from
`matches!(inner_ty, ValueTy::Int)`. An `Integer` operand must use
`ValueConstInteger(0)` and return `ValueTy::Integer`, otherwise the emitted
`ValueSub` pairs an `Int` zero with an `Integer` operand and the interpreter
latches its mixed-kind fault:

```rust
                        let (ir, inner_ty) = self.lower_value(inner)?;
                        let zero = self.fresh_value_reg();
                        match inner_ty {
                            ValueTy::Int => self.emit_value(ValueInstr::ValueConstInt {
                                dst: zero,
                                value: 0,
                            }),
                            ValueTy::Integer => self.emit_value(ValueInstr::ValueConstInteger {
                                dst: zero,
                                value: Box::new(0.into()),
                            }),
                            _ => self.emit_value(ValueInstr::ValueConstDouble {
                                dst: zero,
                                value: 0.0,
                            }),
                        }
                        let dst = self.fresh_value_reg();
                        self.emit_value(ValueInstr::ValueSub {
                            dst,
                            a: zero,
                            b: ir,
                        });
                        Ok((dst, inner_ty))
```

The constant fold stays `Int`/`Double` in this task: `(3 :: Integer)` is an
`Annot` node, not a bare `Expr::Neg` of a literal, so the fold does not see it.

**Every other place that reads a numeric payload must NOT widen `Integer`.**
These are the sites, and each keeps its behaviour by falling through to its
existing `_` arm — read each one and confirm it, do not guess:

| Site | Today | With `Integer` |
|---|---|---|
| `value_to_f64` | `Double` → some, `Int` → widen | `Integer` → `None` (never round a bignum into an f64) |
| `ValueBufferFill`'s scalar read | `Double`, `Int` | `Integer` → the "absent or non-numeric operand" fault |
| `forced_i64` (Ix offsets) | `Int`, integral `Double` | `Integer` → `None`, so the caller latches its index fault |
| `ArrayIndexFlat`/`ArrayUpdate`/`ArrayAccum` index reads | `Value::Int(i)` | unchanged — an `Integer` index is a type error upstream |
| `ShowInt` | `Int`, `Double` | `Integer` → `None`; `show_integer` is the integer renderer |

`Integer` is a **leaf with no implicit conversions**. The only additions are the
`Integer`-vs-`Integer` arms above.

- [ ] **Step 10: Literals under `:: Integer`, and `show_integer`**

`types/infer.rs`, in `infer_expr_expected`, the `Expr::Int` arm. The
`expected`-directed rule currently maps `Buffer` → buffer vty, everything else →
`Int`. Add `Integer` before the catch-all:

```rust
        Expr::Int(_, _) => Ok(ArrowTy::value(match expected {
            // A numeric literal in a Buffer-expected position is a signal
            // constant (`_ * 0.5` → `0.5 : Buffer`).
            Some(ValueTy::Buffer(_)) => ctx.env.buffer_vty(),
            // `(2 :: Integer)` — an integer literal at `Integer` type. This
            // narrow rule is stage 7a only: it disappears when stage 7b makes
            // every integer literal a `fromInteger`-polymorphic value.
            Some(ValueTy::Integer) => ValueTy::Integer,
            _ => ValueTy::Int,
        })),
```

`lower.rs`, in `lower_value_expected`, the `Expr::Int(v, _)` arm: when the
expected type is `Integer`, emit `ValueConstInteger` and return
`ValueTy::Integer`; otherwise keep today's `ValueConstInt`/`ValueTy::Int`.

`lower.rs`, builtin plumbing for `show_integer` — three existing lists:
`is_builtin_op`'s `collection` array, `builtin_op`'s match
(`("show_integer", 1) => ShowInteger,`), and `value_builtin_ty`
(`"show_integer" => Ok(ValueTy::String),`). Also register its signature in
`types/ty.rs` beside `show_int`:

```rust
        env.builtin_sigs.insert(
            "show_integer".to_string(),
            crate::ast::TypeExpr::TFunc(vec![tv("Integer")], Box::new(tv("String"))),
        );
```

- [ ] **Step 11: Write the conformance test file**

`rill-lang/tests/integer_type.rs`:

```rust
//! Haskell 2010 `Integer` (Report §6.4, Table 6.1): arbitrary-precision
//! integers. Removes OD-27's "Integer is absent" half.

use rill_lang::{compile, ffi::ForeignRegistry};

/// Compile and run `src`, returning the concatenated stdout.
fn run(src: &str) -> String {
    let mut prog = compile::<f32, 256>(src, ForeignRegistry::new())
        .unwrap_or_else(|e| panic!("compile failed for:\n{src}\n{e}"));
    prog.run().unwrap_or_else(|e| panic!("run failed for:\n{src}\n{e}"));
    prog.stdout()
}

/// A program that prints one `show_integer` result.
fn shows_int(expr: &str) -> String {
    format!(
        "main = do {{ write (Just (show_integer ({expr}))) stdout; return () }};"
    )
}

/// A program that prints one `show_int` result.
fn shows_i64(expr: &str) -> String {
    format!("main = do {{ write (Just (show_int ({expr}))) stdout; return () }};")
}

fn rejects(src: &str) {
    assert!(
        compile::<f32, 256>(src, ForeignRegistry::new()).is_err(),
        "must be a type error:\n{src}"
    );
}

#[test]
fn integer_is_a_distinct_type_name() {
    assert_eq!(run(&shows_int("2 :: Integer")), "2");
}

#[test]
fn integer_arithmetic_is_exact_and_does_not_overflow() {
    // 10^20 is far outside i64's range; `Int` would wrap, `Integer` must not.
    assert_eq!(
        run(&shows_int(
            "(100000000000000000000 :: Integer) * (100000000000000000000 :: Integer)"
        )),
        "10000000000000000000000000000000000000000"
    );
    assert_eq!(
        run(&shows_int(
            "(2 :: Integer) + (3 :: Integer)"
        )),
        "5"
    );
    assert_eq!(
        run(&shows_int("(2 :: Integer) - (5 :: Integer)")),
        "-3"
    );
}

#[test]
fn integer_comparison_works_and_is_numeric() {
    assert_eq!(
        run("main = do { write (Just (show_bool ((2 :: Integer) < (10 :: Integer)))) stdout; return () };"),
        "True"
    );
    assert_eq!(
        run("main = do { write (Just (show_bool ((2 :: Integer) == (2 :: Integer)))) stdout; return () };"),
        "True"
    );
}

#[test]
fn integer_and_int_do_not_mix() {
    // `(+) :: Num a => a -> a -> a` takes ONE `a`. GHC:
    // "Couldn't match expected type 'Int' with actual type 'Integer'".
    rejects("main = (1 :: Int) + (2 :: Integer);");
    rejects("main = (1 :: Integer) + (2 :: Int);");
    rejects("main = (1 :: Int) == (2 :: Integer);");
}

#[test]
fn integer_is_not_fractional() {
    // GHC: `No instance for 'Fractional Integer'`.
    rejects("main = (1 :: Integer) / (2 :: Integer);");
}

#[test]
fn integer_unary_minus_stays_integer() {
    // `negate` on the `Integer` track: the runtime path is `zero - x`, and the
    // zero must be an `Integer` zero, not an `Int` one.
    assert_eq!(run(&shows_int("-(3 :: Integer)")), "-3");
    assert_eq!(run(&shows_int("(0 :: Integer) - (3 :: Integer)")), "-3");
}

#[test]
fn int_still_wraps_where_integer_does_not() {
    // The CONTROL for the exactness test: the same shape on the Int track is
    // i64 arithmetic, and the report leaves overflow of fixed-width types
    // undefined (§6.4) — rill truncates. Pinned so nobody "fixes" Int by
    // routing it through Integer.
    let wrapped = run(&shows_i64("9223372036854775807 + 1"));
    assert_eq!(wrapped, "-9223372036854775808");
}
```

- [ ] **Step 12: Run the tests**

Run: `cargo test -p rill-lang --features dsp --test integer_type`
Expected: all 7 tests PASS.

- [ ] **Step 13: Run the full gates**

```bash
cargo test -p rill-lang --features dsp
cargo clippy -p rill-lang --features dsp
cargo fmt --check
cargo test -p rill-lang --features "serialization dsp" --test program_json
```

Expected: 1119 + 7 tests pass, clippy reports 0, fmt clean, `program_json` 8/8.
Any pre-existing test that now fails must be reported, not silenced.

- [ ] **Step 14: Commit**

```bash
git add -A
git commit -m 'feat: add Integer as a registered arbitrary-precision scalar type'
```

---

### Task 2: integer literals carry arbitrary precision

**Files:**
- Modify: `rill-lang/src/lexer.rs` (`Tok::Int`)
- Modify: `rill-lang/src/ast.rs` (`Expr::Int`, `Pattern::LitInt`)
- Modify: `rill-lang/src/parser.rs` (the `Expr::Int` and `Pattern::LitInt`
  construction sites)
- Modify: `rill-lang/src/render.rs` (the `Expr::Int` render arm — `BigInt`
  implements `Display`, so the arm is unchanged in shape)
- Modify: `rill-lang/src/lower.rs` (`const_i64`, `const_f64`, the two
  `Expr::Int` lowering arms, `Expr::Neg`'s literal fold, `Pattern::LitInt`
  lowering, the two `tape_loop` capacity sites that read `Expr::Int` as a
  `usize`)
- Modify: `rill-lang/src/lib.rs` (the `tape_loop` resource-capacity site)
- Modify: `rill-lang/src/types/infer.rs` (the `Pattern::LitInt` arms — type
  only)
- Modify: `rill-lang/tests/integer_type.rs`

**Interfaces:**
- Consumes: `Value::Integer(BigInt)` and `ValueInstr::ValueConstInteger` from
  Task 1.
- Produces: `Tok::Int(BigInt)`, `Expr::Int(BigInt)`, `Pattern::LitInt(BigInt)`.

**Why this is a separate task:** today the lexer parses integer literals with
`text.parse::<i64>()` and fails with `invalid int literal` outside i64. An
`Integer` type whose own literals cannot exceed i64 is not `Integer` — GHC
accepts `123456789012345678901234567890 :: Integer`. This is the literal
*representation*, independent of Task 1's *type*.

- [ ] **Step 1: Write the failing test**

Append to `rill-lang/tests/integer_type.rs`:

```rust
#[test]
fn integer_literals_are_arbitrary_precision() {
    // A literal far outside i64 must LEX and keep every digit.
    assert_eq!(
        run(&shows_int("123456789012345678901234567890 :: Integer")),
        "123456789012345678901234567890"
    );
}

#[test]
fn an_out_of_range_literal_on_the_int_track_truncates_like_ghc() {
    // GHC warns "Literal … is out of the Int range" and truncates. rill has no
    // warning channel; the value must still be the same i64 GHC produces.
    assert_eq!(
        run(&shows_i64("123456789012345678901234567890")),
        "-4362896299872285998"
    );
}
```

(That number is GHC's own output, measured on this machine — do not recompute it
by hand.)

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p rill-lang --features dsp --test integer_type`
Expected: both new tests FAIL — the first with `invalid int literal`.

- [ ] **Step 3: Widen the token and the AST leaf**

`lexer.rs`: `Tok::Int(BigInt)`; in the tokeniser replace
`let v: i64 = text.parse()` with `text.parse::<BigInt>()` (the error message
stays `invalid int literal`). Import `num_bigint::BigInt`.

`ast.rs`: `Expr::Int(BigInt, Span)`; `Pattern::LitInt(BigInt)`. Both derive
`PartialEq`, which `BigInt` implements; add `serde` support by keeping the
existing `cfg_attr(feature = "serde", derive(...))` — `BigInt`'s `serde` feature
is already enabled in Task 1's dependency line.

`parser.rs`: the three construction sites (`Expr::Int(*v, t.span)`,
`Expr::Int(v, next.span)` under `Neg`, `Pattern::LitInt(*v)`) keep their shape.

- [ ] **Step 4: Convert at the boundaries, not in the middle**

`lower.rs`:
- `const_i64` returns `Option<i64>`: `Expr::Int(v, _) =>
  Some(i64::try_from(v.clone()).unwrap_or_else(|_| v.to_i64_wrapping()))`. Use
  `num_bigint`'s `to_i64`/`ToPrimitive`, and for the out-of-range case reproduce
  GHC's truncation (`& 0xFFFF_FFFF_FFFF_FFFF` on the two's-complement low 64
  bits, then reinterpret as `i64`). Write that conversion as ONE small helper
  `fn literal_to_i64(v: &BigInt) -> i64` with a doc comment naming GHC's
  `-Woverflowed-literals` behaviour.
- `const_f64`: `Expr::Int(v, _) => v.to_f64()` (`ToPrimitive`), `None` if not
  representable.
- The `Expr::Int(v, _)` arms in `lower_value_expected` and `lower`: emit
  `ValueConstInteger` with `v.clone()` when the expected type is `Integer`, else
  `ValueConstInt { value: literal_to_i64(v) }`.
- `Expr::Neg`'s `const_i64` fold: negate the `BigInt` before converting, so
  `-9223372036854775808` still lands on the i64 minimum rather than wrapping
  twice.
- The array-literal and `tape_loop` capacity sites that read `Expr::Int` as a
  `usize` (`lower.rs` twice, `lib.rs` once): `literal_to_i64(v) as usize`.
- `Pattern::LitInt(v)` lowering emits `ValueConstInt { value:
  literal_to_i64(v) }` — a case-pattern integer is matched against an `Int`
  scrutinee (`infer.rs` unifies `Pattern::LitInt` with `ValueTy::Int`), so the
  pattern stays on the `Int` track.

`types/infer.rs`: `Pattern::LitInt(_)` arms are type-only and need no change
beyond compiling against `BigInt`.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p rill-lang --features dsp --test integer_type`
Expected: all 8 tests PASS.

- [ ] **Step 6: Run the full gates**

Same four commands as Task 1, Step 13. Expected: all green, `program_json` 8/8
(the AST round-trip must still be exact with a `BigInt` payload).

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m 'feat: integer literals carry arbitrary precision'
```

---

### Task 3: close OD-27's type half and record what stage 7b still owns

**Files:**
- Modify: `docs/superpowers/DOCS-GAPS.md` (OD-27)
- Modify: `docs/superpowers/audits/2026-10-26-haskell2010-conformance-inventory.md`
  (the `Integer` rows, if any remain)
- Modify: `.superpowers/sdd/2026-10-25-rill-lang-dictionaries-stage1/progress.md`
  (task registry — this file is git-ignored, so edit it but do not commit it)

- [ ] **Step 1: Update OD-27**

OD-27 is **not** closed by this plan: `Integer` now exists, but defaulting still
produces `Int` and literals are still monomorphic. Rewrite the row so each half
is stated with its owner:

- **(a) closed by stage 7a** — `Integer` exists as a type with exact arithmetic.
  Pin it with the two measured facts from the tests.
- **(b) still open, owned by stage 7b** — defaulting an unsolved numeric literal
  yields `Int` (`Num Int`), where GHC's `default (Integer, Double)` yields
  `Integer`; and a literal is monomorphic, so `1 + 1.0` is a type error where
  GHC gives `2.0`.

- [ ] **Step 2: Re-check the conformance inventory**

`grep -n 'Integer' docs/superpowers/audits/2026-10-26-haskell2010-conformance-inventory.md`.
For every row that says `Integer` is missing, restate it against the new
reality. Do not delete a row because it is now partially satisfied — say which
part remains.

- [ ] **Step 3: Commit the docs**

```bash
git add docs/superpowers
git commit -m 'docs: record stage 7a (Integer exists); defaulting still belongs to 7b'
```

---

## What this plan deliberately does NOT do

Each of these is a named follow-up, not an omission to be "documented later":

| Not in this plan | Owner | Why separate |
|---|---|---|
| `fromInteger` resolving against the **given** dictionary | 7b | needs literal polymorphism, which needs defaulting |
| `default (Integer, Double)` | 7b | changes what an unannotated literal *means* |
| `1 + 1.0` → `2.0` | 7b | the same mechanism as defaulting |
| `Integral` / `Real` / `Enum` classes, `div`/`mod`/`rem`/`quot` | P4 (`OD-24`) | `Integer` must exist before `Integral` can have it as a member |
| `Show Integer` as a class method | P2 (`Show`) | rill's `show_integer` is a builtin here, like `show_int` |
