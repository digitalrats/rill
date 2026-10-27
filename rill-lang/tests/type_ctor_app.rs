//! B2 — one type-constructor application (`ValueTy::Con`, GHC's `TyConApp`).
//!
//! The seven proofs the plan requires, plus the `List Int`/`List Bool`
//! constructibility check that OD-18 leaves open. Every assertion is a VALUE
//! that changes when the representation, unification, or structural ordering is
//! wrong — an `==`/`<` on the two spellings of one type used to fail with
//! `cannot unify value type App(..) with Data(..)`.
//!
//! `compare` is NOT exercised here: it selects a kernel that has no IR
//! instruction yet (a general kernel gap on `Double`, tracked as B3). The
//! structural ordering for sums already exists in the interpreter — tags
//! first, then payloads — so `False < True` needs only the representation fix.
pub mod common;

use common::*;
use rill_lang::arena::Value;

/// Run `expr` and return the arena value it produced.
fn value_of(defs: &str, expr: &str) -> Value {
    let prog = eval_mode::<256>(
        defs,
        expr,
        rill_lang::ffi::ForeignRegistry::new(),
        rill_lang::mode::Mode::Strict,
    )
    .unwrap_or_else(|e| panic!("`{expr}` must compile and run, got: {e}"));
    out_value(&prog)
}

/// Whether `expr` evaluates to `True` (the Prelude `data Bool = False | True`;
/// `True` is tag 1).
fn is_true(defs: &str, expr: &str) -> bool {
    value_of(defs, expr) == Value::Sum(1, Vec::new())
}

/// Whether `expr` evaluates to `False` (tag 0).
fn is_false(defs: &str, expr: &str) -> bool {
    value_of(defs, expr) == Value::Sum(0, Vec::new())
}

// ---------------------------------------------------------------------------
// 1-3. `Bool` is an ordinary declared sum type: its two nullary constructors
// compare by tag through the one `Con` representation.
// ---------------------------------------------------------------------------

#[test]
fn false_equals_false() {
    // Before the merge this failed to COMPILE: the `False` literal typed as
    // `Data("Bool", [])` in one path and `App("Bool", [])` in another, and the
    // unifier compared the variants strictly.
    assert!(is_true("", "False == False"));
}

#[test]
fn false_does_not_equal_true() {
    // The discriminating half: a rule that equated any two `Bool`s would pass
    // the test above but fail this one.
    assert!(is_false("", "False == True"));
}

#[test]
fn false_is_less_than_true() {
    // Deriving order from the DECLARATION order (`False` tag 0, `True` tag 1);
    // a wrong tag assignment would make this False.
    assert!(is_true("", "False < True"));
}

// ---------------------------------------------------------------------------
// 4-6. A user sum orders by tag, then by payload.
// ---------------------------------------------------------------------------

#[test]
fn user_sum_orders_by_tag() {
    let defs = "data MyB = Off | On;";
    assert!(is_true(defs, "Off < On"));
    // The mirror, so a rule that answered `True` unconditionally fails.
    assert!(is_false(defs, "On < Off"));
    assert!(is_true(defs, "Off == Off"));
    assert!(is_false(defs, "Off == On"));
}

#[test]
fn parameterized_sum_payload_equality() {
    let defs = "data Opt a = Some a | None;";
    assert!(is_true(defs, "Some 1.0 == Some 1.0"));
    // A different payload is a different value — the payload really is
    // compared, not ignored.
    assert!(is_false(defs, "Some 1.0 == Some 2.0"));
    // A different TAG outranks the payload.
    assert!(is_false(defs, "Some 1.0 == None"));
}

#[test]
fn parameterized_sum_payload_ordering() {
    let defs = "data Opt a = Some a | None;";
    // Same tag: the payload decides.
    assert!(is_true(defs, "Some 1.0 < Some 2.0"));
    assert!(is_false(defs, "Some 2.0 < Some 1.0"));
    // Tag first: `Some` (tag 0) sorts before `None` (tag 1) whatever the
    // payload is — GHC's derived `Ord` for a sum type.
    assert!(is_true(defs, "Some 100.0 < None"));
}

// ---------------------------------------------------------------------------
// 7. Scalar regression guard: the derived `Ord Double` path is untouched.
// ---------------------------------------------------------------------------

#[test]
fn scalar_ordering_still_works() {
    assert!(is_true("", "1.0 < 2.0"));
    assert!(is_false("", "2.0 < 1.0"));
    assert!(is_true("", "1.0 <= 1.0"));
    assert!(is_true("", "1.0 == 1.0"));
}

// ---------------------------------------------------------------------------
// OD-18 remains open, but both applications must stay constructible.
// ---------------------------------------------------------------------------

#[test]
fn list_of_int_and_list_of_bool_are_both_constructible() {
    // `List` has no declaration in `data_types` (it is a builtin container) and
    // `Bool` does; both spell the same one `Con` application, so a list of
    // either element type type-checks. (Whether `List Int` and `List Bool` are
    // DISTINCT is OD-18, out of scope here.)
    assert_eq!(value_of("", "length [1.0, 2.0]"), Value::Int(2));
    let defs = "data Color = Red | Green;";
    assert_eq!(
        value_of(defs, "length [Red, Green, Red]"),
        Value::Int(3),
        "a list of a declared sum type is constructible through the same `Con`"
    );
}
