//! `Bool` is an ordinary data type: `data Bool = False | True` (Haskell 2010
//! §4.1.1).
//!
//! There is no boolean primitve in the language: `Bool` is `Data("Bool", [])`,
//! a boolean value is the nullary constructor `Sum(0|1, [])`, `True`/`False` are
//! constructors usable in expression AND pattern position, and `&&`/`||` are
//! ordinary Prelude FUNCTIONS written with the Report's equations. Their
//! short-circuiting is not a mechanism — `False && _ = False` simply never
//! looks at its second argument, and rill evaluates arguments lazily.
//!
//! `true`/`false` remain accepted as ALIASES of the two constructors, so a
//! program written with the legacy lowercase spelling needs no respelling; the
//! Report's `True`/`False` is the canonical form the renderer quotes.

pub mod common;

use common::*;
use rill_lang::arena::{Value, BOOL_FALSE_TAG, BOOL_TRUE_TAG};
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;

/// Compile `defs` + `expr` in complete (lazy) mode and run it once.
fn run(defs: &str, expr: &str) -> Result<Value, String> {
    match eval_mode::<256>(defs, expr, ForeignRegistry::new(), Mode::Complete) {
        Ok(world) => Ok(out_value(&world)),
        Err(e) => Err(e.to_string()),
    }
}

/// The host boolean a run denotes.
fn boolean(defs: &str, expr: &str) -> bool {
    let v = run(defs, expr).unwrap_or_else(|e| panic!("`{expr}` must run: {e}"));
    v.as_bool()
        .unwrap_or_else(|| panic!("`{expr}` must be a Bool constructor, got {v:?}"))
}

// ---------------------------------------------------------------------------
// 1. The value model: `Bool` is a two-constructor sum, not a primitive.
// ---------------------------------------------------------------------------

#[test]
fn a_boolean_is_a_nullary_sum_constructor() {
    assert_eq!(
        run("", "True").unwrap(),
        Value::Sum(BOOL_TRUE_TAG, vec![]),
        "`True` is the sum constructor with tag 1"
    );
    assert_eq!(
        run("", "False").unwrap(),
        Value::Sum(BOOL_FALSE_TAG, vec![]),
        "`False` is the sum constructor with tag 0 (declaration order)"
    );
    // The tags are the declaration order `data Bool = False | True`, so a
    // comparison against `Ord`-free tag order is stable.
    assert_eq!((BOOL_FALSE_TAG, BOOL_TRUE_TAG), (0, 1));
}

#[test]
fn bool_is_unified_with_its_declared_data_type() {
    // `Bool` is `Data("Bool", [])`: a signature naming it type-checks a
    // constructor argument and a value definition of it, with no special type.
    assert!(boolean("idB x :: Bool -> Bool = x;", "idB True"));
    assert!(!boolean("idB x :: Bool -> Bool = x;", "idB False"));
    assert!(boolean("", "True"));
}

// ---------------------------------------------------------------------------
// 2. `True` / `False` work in BOTH expression and pattern position.
// ---------------------------------------------------------------------------

#[test]
fn constructors_work_in_expression_and_pattern_position() {
    // Expression position.
    assert!(boolean("", "True"));
    assert!(!boolean("", "False"));

    // Pattern position: `case` over both constructors, no wildcard.
    assert_eq!(
        run("", "case True of { True -> 1.0; False -> 0.0; }").unwrap(),
        Value::Double(1.0)
    );
    assert_eq!(
        run("", "case False of { True -> 1.0; False -> 0.0; }").unwrap(),
        Value::Double(0.0)
    );

    // Function equations: this is what made `&&` expressible at all.
    let my_and = "myAnd True y = y;\nmyAnd False _ = False;";
    assert!(boolean(my_and, "myAnd True True"));
    assert!(!boolean(my_and, "myAnd False True"));
    assert!(!boolean(my_and, "myAnd True False"));

    let my_or = "myOr True _ = True;\nmyOr False y = y;";
    assert!(boolean(my_or, "myOr True False"));
    assert!(boolean(my_or, "myOr False True"));
    assert!(!boolean(my_or, "myOr False False"));

    // A nullary constructor pattern in a later argument position parses as its
    // OWN argument (Haskell 2010 §4.4.3.1 gives each LHS position an `apat`),
    // so `f True False` is TWO arguments and reaches the first equation. The
    // second value distinguishes it from an `f True`-then-`False` reading.
    let two_args = "f True False = 1.0;\nf _ _ = 0.0;";
    assert_eq!(run(two_args, "f True False").unwrap(), Value::Double(1.0));
    assert_eq!(run(two_args, "f False True").unwrap(), Value::Double(0.0));
}

#[test]
fn a_boolean_match_is_checked_for_totality_as_a_sum() {
    // `Bool` is a sum type, so a `case` must cover BOTH constructors (or have a
    // wildcard). This is the sum rule, not a boolean-literal rule.
    assert!(
        run("", "case True of { True -> 1.0; }").is_err(),
        "a boolean match missing `False` must be non-exhaustive"
    );
    assert!(
        run("", "case True of { True -> 1.0; False -> 0.0; }").is_ok(),
        "both constructors cover the type"
    );
}

#[test]
fn the_lowercase_spellings_are_aliases_of_the_constructors() {
    // The Report's spelling is canonical and the two are the SAME values.
    assert_eq!(run("", "true").unwrap(), run("", "True").unwrap());
    assert_eq!(run("", "false").unwrap(), run("", "False").unwrap());

    // In pattern position too.
    assert_eq!(
        run("", "case True of { true -> 1.0; false -> 0.0; }").unwrap(),
        Value::Double(1.0)
    );
    assert_eq!(
        run("", "case false of { true -> 1.0; false -> 0.0; }").unwrap(),
        Value::Double(0.0)
    );

    // And in a function equation.
    assert!(boolean(
        "myNot true = false;\nmyNot false = true;",
        "myNot False"
    ));
}

// ---------------------------------------------------------------------------
// 3. `&&` / `||` are Prelude functions, and OD-25 is fixed.
// ---------------------------------------------------------------------------

#[test]
fn logic_operators_compute_the_haskell_table() {
    for (expr, want) in [
        ("True && True", true),
        ("True && False", false),
        ("False && True", false),
        ("False && False", false),
        ("True || True", true),
        ("True || False", true),
        ("False || True", true),
        ("False || False", false),
    ] {
        assert_eq!(boolean("", expr), want, "`{expr}`");
    }
}

#[test]
fn od_25_short_circuit_comes_from_laziness() {
    // OD-25: `&&`/`||` used to be an IR primitive that forced BOTH operands, so
    // a bottom right operand exhausted the run where GHC prints a value. With
    // `&&`/`||` written as the Report's equations over constructor patterns,
    // the short-circuit is ordinary laziness: `False && _` never evaluates its
    // second argument.
    let diverges = "loop x = loop x;";

    assert_eq!(
        run(diverges, "False && loop 1.0").unwrap(),
        Value::Sum(BOOL_FALSE_TAG, vec![]),
        "`False && loop 1.0` must be `False`, not divergence"
    );
    assert_eq!(
        run(diverges, "True || loop 1.0").unwrap(),
        Value::Sum(BOOL_TRUE_TAG, vec![]),
        "`True || loop 1.0` must be `True`, not divergence"
    );
    // The other two directions DO need the second operand, so they diverge —
    // proving the two passing cases are real short-circuits and not a blanket
    // skip.
    assert!(
        run(diverges, "True && loop 1.0").is_err(),
        "`True && loop 1.0` needs its right operand and must diverge"
    );
    assert!(
        run(diverges, "False || loop 1.0").is_err(),
        "`False || loop 1.0` needs its right operand and must diverge"
    );
}

#[test]
fn logic_operators_are_usable_as_first_class_functions() {
    // Haskell 2010 §4.4.2: an operator IS an ordinary function, so `(&&)` and
    // `(||)` are values.
    assert!(boolean("", "(&&) True True"));
    assert!(!boolean("", "(&&) True False"));
    assert!(boolean("", "(||) False True"));
    assert!(!boolean("", "(||) False False"));
    // And the fixity is §4.4.2's: `infixr 3 &&` binds tighter than `infixr 2
    // ||`, so `True || True && False` is `True || (True && False)` = True.
    assert!(boolean("", "True || True && False"));
    assert!(!boolean("", "(True || True) && False"));
}

// ---------------------------------------------------------------------------
// 4. `if` branches on the constructor tag of a `Bool` value.
// ---------------------------------------------------------------------------

#[test]
fn if_branches_on_the_constructor_tag() {
    assert_eq!(
        run("", "if True then 1.0 else 2.0").unwrap(),
        Value::Double(1.0)
    );
    assert_eq!(
        run("", "if False then 1.0 else 2.0").unwrap(),
        Value::Double(2.0)
    );
    // A runtime condition reaches the same constructor-tag branch.
    assert_eq!(
        run("x = 1.0;", "if x > 0.5 then 1.0 else 2.0").unwrap(),
        Value::Double(1.0)
    );
    // A boolean-valued EXPRESSION as the condition (bound first: a
    // parenthesized condition followed by an atom parses as an application).
    assert_eq!(
        run("c = (True && False) || True;", "if c then 1.0 else 2.0").unwrap(),
        Value::Double(1.0)
    );
}

// ---------------------------------------------------------------------------
// 5. `show_bool` keeps working and renders the CONSTRUCTOR.
// ---------------------------------------------------------------------------

#[test]
fn show_bool_renders_the_constructor_spelling() {
    let world = eval("", "show_bool True").unwrap();
    match out_value(&world) {
        Value::String(s) => assert_eq!(s, "True"),
        other => panic!("expected a String, got {other:?}"),
    }
    let world = eval("", "show_bool false").unwrap();
    match out_value(&world) {
        Value::String(s) => assert_eq!(s, "False"),
        other => panic!("expected a String, got {other:?}"),
    }
    // A computed boolean is still the constructor the renderer names.
    let world = eval("", "show_bool (1.0 > 2.0)").unwrap();
    match out_value(&world) {
        Value::String(s) => assert_eq!(s, "False"),
        other => panic!("expected a String, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 6. `not`, `otherwise` and the compiler's own boolean producers.
// ---------------------------------------------------------------------------

#[test]
fn not_is_the_constructor_flip() {
    assert!(!boolean("", "not True"));
    assert!(boolean("", "not False"));
    assert!(boolean("", "not (True && False)"));
}

#[test]
fn otherwise_is_the_true_constructor() {
    assert_eq!(run("", "otherwise").unwrap(), run("", "True").unwrap());
    // As a guard fallback it is the last alternative.
    assert_eq!(
        run("f x | x > 0.0 = 1.0 | otherwise = 2.0;", "f 0.0").unwrap(),
        Value::Double(2.0)
    );
}

#[test]
fn compiler_produced_booleans_are_constructors() {
    // `member` and a comparison both produce the `Bool` constructor.
    assert!(boolean("s = insert 5 (empty_set);", "member 5 s"));
    assert!(!boolean("s = insert 5 (empty_set);", "member 6 s"));
    assert!(boolean("", "1.0 < 2.0"));
    assert!(!boolean("", "1.0 > 2.0"));
}
