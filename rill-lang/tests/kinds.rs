//! K1–K2: kinds are REAL, and checked wherever a type is required.
//!
//! Haskell 2010 §4.1: a type expression in a type position must have kind `*`.
//! A `* -> *` constructor used there (`f :: List -> Int`, `f :: Pair Int ->
//! Int`) is an error, exactly as GHC 9.10.3 reports:
//!
//! ```text
//! Expecting one more argument to '[]'; Expected a type, but '[]' has kind '* -> *'
//! ```
//!
//! The kind of a name is a PROJECTION of the arity the registry already
//! records ([`TypeEnv::kind_of`]): there is no second table of type names.
//! Instance heads are kind-checked against their class's variable kind for
//! EVERY instance, prelude ones included — the prelude's `from_prelude` trust
//! flag covers BODIES only (field-name shadowing), never a head.
//!
//! Each rejection test asserts a value that CHANGES when the rule is wrong: the
//! message names the kind (`* -> *`) and the missing-argument count, so a
//! check that merely rejected the program for another reason would not pass.

pub mod common;

use common::*;
use rill_lang::ast::TypeExpr;
use rill_lang::mode::Mode;
use rill_lang::types::ty::{Kind, TypeEnv};

/// Compile a program whose entry is `main`; report the error text or `Ok`.
fn try_compile(src: &str) -> Result<(), String> {
    match rill_lang::compile_eval::<f32, 256>(src, "main", Mode::Strict) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("{e}")),
    }
}

/// Assert the program is rejected AND the message is the KIND wording, naming
/// the kind and the missing argument count.
fn assert_kind_error(src: &str, kind: &str, missing: &str) {
    match try_compile(src) {
        Ok(()) => panic!("expected a kind error, but the program compiled:\n{src}"),
        Err(msg) => {
            assert!(
                msg.contains(&format!("has kind `{kind}`")),
                "expected the diagnostic to name kind `{kind}`, got: {msg}\n{src}"
            );
            assert!(
                msg.contains(missing),
                "expected the diagnostic to name `{missing}` missing, got: {msg}\n{src}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// K1: the four measured rows, plus field / payload / method / annotation.
// ---------------------------------------------------------------------------

#[test]
fn partially_applied_pair_in_a_signature_is_a_kind_error() {
    // GHC: `Expecting one more argument to '(,) Int'; Expected a type, but
    // '(,) Int' has kind '* -> *'`.
    assert_kind_error(
        "f :: Pair Int -> Int; f x = 1; main = 1.0;",
        "* -> *",
        "1 more argument",
    );
}

#[test]
fn bare_list_in_a_signature_is_a_kind_error() {
    // GHC: `Expecting one more argument to '[]'; … has kind '* -> *'`.
    assert_kind_error(
        "f :: List -> Int; f x = 1; main = 1.0;",
        "* -> *",
        "1 more argument",
    );
}

#[test]
fn bare_maybe_in_a_signature_is_a_kind_error() {
    assert_kind_error(
        "f :: Maybe -> Int; f x = 1; main = 1.0;",
        "* -> *",
        "1 more argument",
    );
}

#[test]
fn bare_fixedbuffer_in_a_signature_is_a_kind_error() {
    // rill-specific constructor, same rule: `FixedBuffer` is `* -> *`.
    assert_kind_error(
        "f :: FixedBuffer -> Int; f x = 1; main = 1.0;",
        "* -> *",
        "1 more argument",
    );
}

#[test]
fn a_bare_binary_constructor_reports_two_missing_arguments() {
    // The count comes from the KIND, not from a fixed message: `Pair` alone is
    // `* -> * -> *`, so GHC says "Expecting two more arguments".
    assert_kind_error(
        "f :: Pair -> Int; f x = 1; main = 1.0;",
        "* -> * -> *",
        "2 more arguments",
    );
}

#[test]
fn a_data_field_type_is_kind_checked() {
    // A field type is a type position, not only a signature.
    assert_kind_error(
        "data H = { h: List }; main = 1.0;",
        "* -> *",
        "1 more argument",
    );
}

#[test]
fn a_constructor_payload_type_is_kind_checked() {
    // A sum constructor's payload is a type position too.
    assert_kind_error("data S = Mk List; main = 1.0;", "* -> *", "1 more argument");
}

#[test]
fn a_class_method_argument_is_kind_checked() {
    // A class method's signature is a type position.
    assert_kind_error(
        "typeclass C a where { c: List -> a; } main = 1.0;",
        "* -> *",
        "1 more argument",
    );
}

#[test]
fn an_expression_annotation_is_kind_checked() {
    // `(e :: T)` is a type position (Haskell 2010 §3.12).
    assert_kind_error(
        "x = (1.0 :: List); main = 1.0;",
        "* -> *",
        "1 more argument",
    );
}

#[test]
fn an_over_applied_constructor_has_no_kind() {
    // `Pair Int Double Int`: `Pair` accepts exactly two arguments. GHC rejects
    // the head's over-application rather than the whole application's kind.
    match try_compile("f :: Pair Int Double Int -> Int; f x = 1; main = 1.0;") {
        Ok(()) => panic!("an over-applied constructor must be rejected"),
        Err(msg) => assert!(
            msg.contains("too many arguments") && msg.contains("* -> * -> *"),
            "expected an over-application diagnostic, got: {msg}"
        ),
    }
}

#[test]
fn well_kinded_types_still_compile() {
    // No behaviour change for well-kinded programs: the saturated forms are
    // exactly what the registry's arities accept.
    for src in [
        "f :: Pair Int Double -> Int; f x = 1; main = 1.0;",
        "f :: List Double -> Int; f x = 1; main = 1.0;",
        "f :: Maybe Double -> Int; f x = 1; main = 1.0;",
        "f :: FixedBuffer Double -> Int; f x = 1; main = 1.0;",
        // A type VARIABLE is never a kind error: its binder supplies the kind.
        "f :: FixedBuffer a -> FixedBuffer a; f x = x; main = 1.0;",
        // A higher-kinded VARIABLE head (`m a`) inside a class method.
        "typeclass F f where { fmap: (a -> b) -> f a -> f b; } main = 1.0;",
    ] {
        assert!(
            try_compile(src).is_ok(),
            "well-kinded program must compile: {src}"
        );
    }
}

// ---------------------------------------------------------------------------
// The kind is a PROJECTION of arity data — one answer, no second table.
// ---------------------------------------------------------------------------

#[test]
fn kind_of_is_a_projection_of_the_registry_arity() {
    let env = TypeEnv::with_builtins();
    // Scalar and newtype/alias leaves are `*`.
    assert_eq!(env.kind_of("Int"), Some(Kind::TYPE));
    assert_eq!(env.kind_of("Double"), Some(Kind::TYPE));
    assert_eq!(env.kind_of("()"), Some(Kind::TYPE));
    // Builtin containers, derived from `ctor_kinds`.
    assert_eq!(env.kind_of("List"), Some(Kind::of_arity(1)));
    assert_eq!(env.kind_of("Maybe"), Some(Kind::of_arity(1)));
    assert_eq!(env.kind_of("FixedBuffer"), Some(Kind::of_arity(1)));
    assert_eq!(env.kind_of("Pair"), Some(Kind::of_arity(2)));
    assert_eq!(env.kind_of("Array"), Some(Kind::of_arity(2)));
    assert_eq!(env.kind_of("STArray"), Some(Kind::of_arity(3)));
    // A prelude `data` declaration with parameters, from `data_arities`.
    assert_eq!(env.kind_of("Kleisli"), Some(Kind::of_arity(3)));
    assert_eq!(env.kind_of("ST"), Some(Kind::of_arity(2)));
    // A type VARIABLE has no registry kind: its binder supplies it.
    assert_eq!(env.kind_of("a"), None);
    // An undeclared uppercase name has no kind either (it is not a type).
    assert_eq!(env.kind_of("Nope"), None);
}

#[test]
fn kind_and_arity_never_disagree() {
    // The projection cannot drift from the tables it reads: for every name the
    // registry records an arity for, `kind_of` reports exactly that arity.
    let env = TypeEnv::with_builtins();
    let mut checked = 0;
    for (name, arity) in env.ctor_kinds.iter().chain(env.data_arities.iter()) {
        assert_eq!(
            env.kind_of(name),
            Some(Kind::of_arity(*arity)),
            "`{name}` records arity {arity} but kind_of disagrees"
        );
        checked += 1;
    }
    assert!(
        checked >= 10,
        "the registry must carry the builtin constructors"
    );
}

#[test]
fn kind_spelling_is_the_haskell_shape() {
    assert_eq!(Kind::TYPE.to_string(), "*");
    assert_eq!(Kind::of_arity(1).to_string(), "* -> *");
    assert_eq!(Kind::of_arity(2).to_string(), "* -> * -> *");
    assert_eq!(Kind::of_arity(2).applied_to(1), Some(Kind::of_arity(1)));
    assert_eq!(Kind::of_arity(1).applied_to(2), None);
}

// ---------------------------------------------------------------------------
// K2: instance heads, for EVERY instance (prelude included), in kind terms.
// ---------------------------------------------------------------------------

#[test]
fn a_binary_constructor_for_a_unary_class_is_a_kind_error() {
    // `Functor f` binds a `* -> *` constructor; `Pair` is `* -> * -> *`.
    match try_compile(
        "typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }\n\
         instance Functor Pair where { fmap g p = p; }\n\
         main = 1.0;",
    ) {
        Ok(()) => panic!("`instance Functor Pair` must be a kind error"),
        Err(msg) => assert!(
            msg.contains("`Functor` expects a type constructor of kind `* -> *`")
                && msg.contains("`Pair` has kind `* -> * -> *`"),
            "expected a kind mismatch, got: {msg}"
        ),
    }
}

#[test]
fn a_type_for_a_constructor_class_is_a_kind_error() {
    // `Int` has kind `*`, so it cannot bind `Functor`'s `* -> *` variable.
    match try_compile(
        "typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }\n\
         instance Functor Int where { fmap g p = p; }\n\
         main = 1.0;",
    ) {
        Ok(()) => panic!("`instance Functor Int` must be a kind error"),
        Err(msg) => assert!(
            msg.contains("`Functor` expects a type constructor of kind `* -> *`")
                && msg.contains("`Int` has kind `*`"),
            "expected a kind mismatch, got: {msg}"
        ),
    }
}

#[test]
fn a_partially_applied_head_has_the_remaining_kind() {
    // `instance Functor (Pair Int)` = GHC's `instance Functor ((,) Int)`: one
    // argument is bound, so the head is `* -> *` — exactly `Functor`'s kind —
    // and must pass the KIND check.
    //
    // Proved at the checker level: DISPATCH to a head whose bound argument is a
    // concrete type (`Pair Int`) is a separate, still-open concern (the
    // instance registry keys it by its full spelling while method dispatch
    // resolves `Pair`), so a source-level assertion here would test the
    // dispatcher, not the kind rule this task owns.
    let env = TypeEnv::with_builtins();
    let mut env = env;
    let src = "typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    env.register_decls(&prog.defs);
    env.check_instance_head(
        "Functor",
        &TypeExpr::TApp("Pair".into(), vec![TypeExpr::TName("Int".into())]),
        rill_lang::error::Span::new(0, 0),
    )
    .expect("`Pair Int` is `* -> *`, so `instance Functor (Pair Int)` kind-checks");

    // The same head with the arg left abstract (`Pair a`) is also `* -> *`.
    env.check_instance_head(
        "Functor",
        &TypeExpr::TApp("Pair".into(), vec![TypeExpr::TName("a".into())]),
        rill_lang::error::Span::new(0, 0),
    )
    .expect("`Pair a` is `* -> *`");

    // And the over-applied form is still rejected.
    assert!(
        env.check_instance_head(
            "Functor",
            &TypeExpr::TApp(
                "Pair".into(),
                vec![TypeExpr::TName("Int".into()), TypeExpr::TName("Int".into())],
            ),
            rill_lang::error::Span::new(0, 0),
        )
        .is_err(),
        "`Pair Int Int` is `*`, not `* -> *`"
    );
}

#[test]
fn a_constraint_head_leaves_its_remaining_kind() {
    // `instance (Monad m) => Arrow (Kleisli m)`: `Kleisli` is arity 3, one
    // argument is bound, so the head is `* -> * -> *` — `Arrow`'s kind.
    let src = r"
        typeclass Arrow a where { arr: (b -> c) -> a b c; }
        instance (Monad m) => Arrow (Kleisli m) where { arr f = Kleisli (\ x -> return (f x)); }
        main = 1.0;
    ";
    assert!(
        try_compile(src).is_ok(),
        "a constraint instance head must kind-check at its remaining kind"
    );
}

#[test]
fn the_prelude_loads_with_every_head_kind_checked() {
    // The prelude's head check is ON: `TypeEnv::with_builtins` validates each
    // prelude instance head through `check_instance_head`. If any prelude head
    // were ill-kinded this test would fail while building the environment.
    let env = TypeEnv::with_builtins();
    // The prelude declares these heads; every one must kind-check.
    for (class, ty) in [
        ("Functor", "List"),
        ("Functor", "Maybe"),
        ("Monad", "List"),
        ("Monad", "Maybe"),
        ("Monad", "IO"),
        ("Monad", "ST"),
        ("Monoid", "List"),
        ("Monoid", "Double"),
        ("Monoid", "String"),
        ("Num", "Int"),
        ("Num", "Double"),
        ("Num", "FixedBuffer Double"),
        ("Buffer", "FixedBuffer Double"),
        ("Ix", "Int"),
        ("Ix", "Pair Int Int"),
        ("Arrow", "Kleisli"),
    ] {
        let info = env
            .instances
            .get(class)
            .and_then(|by_ty| by_ty.get(ty))
            .unwrap_or_else(|| panic!("the prelude must declare `instance {class} {ty}`"));
        env.check_instance_head(class, &info.head, rill_lang::error::Span::new(0, 0))
            .unwrap_or_else(|e| {
                panic!("prelude head `instance {class} {ty}` must kind-check: {e}")
            });
    }
}

// ---------------------------------------------------------------------------
// K2: a VARIABLE-headed instance must kind-check (checker level).
//
// `instance Monoid [a]` (Haskell 2010) is not expressible through rill's
// surface yet — `[a]` type syntax is task 5.5c's job. The property is therefore
// proved against the type REPRESENTATION, not against source spelling: a head
// whose argument is a VARIABLE has the class variable's kind and must pass.
// ---------------------------------------------------------------------------

#[test]
fn a_variable_headed_instance_kind_checks() {
    let env = TypeEnv::with_builtins();
    // `instance Monoid [a]`: the class variable `m` is a TYPE (Monoid's arity
    // is 0), and the head is `List a`, a type applied to a variable.
    let head = TypeExpr::TApp("List".into(), vec![TypeExpr::TName("a".into())]);
    env.check_instance_head("Monoid", &head, rill_lang::error::Span::new(0, 0))
        .expect("`instance Monoid [a]` must kind-check: the head is `*`");

    // The bare-constructor spelling rill uses for the same instance
    // (`instance Monoid List`) is the same head, saturated.
    env.check_instance_head(
        "Monoid",
        &TypeExpr::TName("List".into()),
        rill_lang::error::Span::new(0, 0),
    )
    .expect("`instance Monoid List` must kind-check as `Monoid [a]`");

    // A head whose argument is a variable is NOT an over-application: `Pair a
    // b` is `*`, so a type-variable class accepts it (`instance C (Pair a b)`).
    let pair_head = TypeExpr::TApp(
        "Pair".into(),
        vec![TypeExpr::TName("a".into()), TypeExpr::TName("b".into())],
    );
    env.check_instance_head("Monoid", &pair_head, rill_lang::error::Span::new(0, 0))
        .expect("`instance Monoid (Pair a b)` must kind-check");
}

#[test]
fn a_variable_head_bound_by_the_class_kind_passes() {
    // A toy class whose variable is a CONSTRUCTOR (`C f` with `f a` in a
    // method) has arity 1; a variable head binds at `* -> *`, so `instance C f`
    // is well kinded (`instance Functor f` in GHC terms is the same shape).
    let mut env = TypeEnv::with_builtins();
    let src = "typeclass Probe f where { probe: f Int -> Double; }";
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    env.register_decls(&prog.defs);
    env.check_instance_head(
        "Probe",
        &TypeExpr::TName("f".into()),
        rill_lang::error::Span::new(0, 0),
    )
    .expect("a variable head binds at the class variable's kind");
}
