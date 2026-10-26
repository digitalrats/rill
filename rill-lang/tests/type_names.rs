//! Type-NAME conformance (Haskell 2010 §3.1): a name denotes a type only if
//! something declared it, and a lowercase name is a type variable.
//!
//! rill used to accept any name in a type position, so `f :: Integer ->
//! Integer` compiled and ran although no `Integer` value exists. The registry
//! ([`TypeEnv::type_entry`]) is now the single answer to "does this name denote
//! a type": the declaration check, the resolver, the derived-instance seed, the
//! FFI classification and the diagnostic renderer all read it, so they cannot
//! disagree again.

pub mod common;

use common::*;
use rill_lang::ast::TypeExpr;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::types::ffi::ffi_sig_from_typeexpr;
use rill_lang::types::ty::{ScalarType, TypeEntry, TypeEnv, ValueTy};

/// Compile `src` (which must bind `main`) and report either the compile error
/// message or the value the run produced.
fn compile(src: &str) -> Result<String, String> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main`");
    let expr = expr.trim().trim_end_matches(';');
    match eval_ffi(defs, expr, ForeignRegistry::<f32>::new()) {
        Ok(world) => {
            let v = world.result().expect("the run must produce a result");
            Ok(format!("{:?}", world.arena().get(v).unwrap()))
        }
        Err(e) => Err(format!("{e}")),
    }
}

/// Compile `src` and return the compile error message, panicking when it
/// compiles.
fn compile_err(src: &str) -> String {
    match compile(src) {
        Ok(v) => panic!("expected the program to be rejected, but it produced {v}\nsrc:\n{src}"),
        Err(e) => e,
    }
}

/// Build the environment the compiler builds for `defs`: run inference and
/// hand back the resulting type environment. That is exactly the object every
/// consumer reads, so a registry assertion here is an assertion about the
/// compiler, not about a test-local copy.
fn env_of(defs: &str) -> TypeEnv {
    let src = format!("{defs}\nmain = 1.0;");
    let toks = rill_lang::lexer::tokenize(&src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Strict)
        .expect("the probe program must infer")
        .type_env
}

#[test]
fn an_undeclared_type_name_is_rejected() {
    // `Integer` is a Haskell 2010 Prelude type, but rill has no `Integer`
    // representation — so the name denotes nothing and the signature is an
    // unknown-type error (GHC's `Not in scope: type constructor or class
    // 'Integer'`). Before the registry this compiled and ran, returning `1`.
    let err = compile_err("f :: Integer -> Integer; f x = x; main = f 1;");
    assert!(
        err.contains("unknown type name `Integer`"),
        "the error must name the undeclared type, got: {err}"
    );

    // ... and the same holds for any other undeclared uppercase name, not just
    // the ones the brief happened to measure.
    for name in ["Zork", "Integer", "Double", "Maybe2", "Listy"] {
        let src = format!("f :: {name} -> Int; f x = 1; main = f 1;");
        let err = compile_err(&src);
        assert!(
            err.contains(&format!("unknown type name `{name}`")),
            "`{name}` must be rejected as an unknown type, got: {err}"
        );
    }
}

#[test]
fn char_is_still_undeclared_for_now() {
    // `Char` is a real Haskell 2010 type, but rill has no `Char` payload yet
    // (that is a later sub-task). Until it is REGISTERED it must be rejected,
    // exactly like `Integer`: accepting it would resurrect the phantom-type
    // defect for the one name the next task is about to make real.
    let err = compile_err("f :: Char -> Char; f x = x; main = f 1;");
    assert!(
        err.contains("unknown type name `Char`"),
        "`Char` must be rejected until it is a registered type, got: {err}"
    );

    // The registry agrees, and knows nothing about a `Char` value type.
    let env = TypeEnv::with_builtins();
    assert_eq!(env.type_entry("Char"), None);
    assert_eq!(env.vty_of_name("Char"), None);
    assert!(!env.is_type_name("Char"));
}

#[test]
fn a_type_variable_is_still_accepted() {
    // `Num a => a -> a`: `a` is lowercase, so it is a type VARIABLE, not a
    // type — the registry must return the variable arm for it, and the program
    // must compile and run.
    let env = TypeEnv::with_builtins();
    assert_eq!(env.type_entry("a"), Some(TypeEntry::Variable));
    assert!(env.is_type_variable("a"));
    assert!(!env.is_type_name("a"), "a type variable is not a type");
    assert_eq!(env.vty_of_name("a"), None);

    let out = compile("f :: Num a => a -> a; f x = x; main = f 1;").expect("must compile");
    assert!(out.contains("Int(1)"), "got {out}");

    // A `forall`-bound variable and a class type variable are the same lexical
    // category, and neither is a declared type.
    let out = compile("g :: forall b. b -> b; g x = x; main = g 2.0;").expect("must compile");
    assert!(out.contains("Float(2.0)"), "got {out}");
}

#[test]
fn a_lowercase_name_is_a_type_variable() {
    // `f32` in `FixedBuffer f32` is an ordinary lowercase identifier, which
    // Haskell's lexical rules make a TYPE VARIABLE (GHC accepts
    // `instance Num (Tape f32)` as an instance for all `a`). It must therefore
    // not be rejected as an unknown type, and must not be turned into a
    // nominal type either.
    let out = compile("f :: FixedBuffer f32 -> FixedBuffer f32; f x = x; main = 1.0;")
        .expect("`f32` must be accepted as a type variable");
    assert!(out.contains("Float(1.0)"), "got {out}");

    let env = env_of("");
    assert_eq!(env.type_entry("f32"), Some(TypeEntry::Variable));
    assert!(!env.is_type_name("f32"));
    assert_eq!(env.vty_of_name("f32"), None);

    // A user-declared parameterized type's own variable behaves the same way.
    compile(
        "data Box a = { unBox: a }; h :: Box b -> b; h x = x.unBox; main = h (Box { unBox: 3 });",
    )
    .expect("must compile");
}

#[test]
fn a_declared_data_type_is_accepted() {
    // A user `data` declaration IS a type, so a signature may mention it. The
    // registry must report a constructor entry with its arity.
    let env = env_of("data P = { x: Int }; data Pair3 a b c = { a1: a }");
    assert_eq!(env.type_entry("P"), Some(TypeEntry::TypeCtor { arity: 0 }));
    assert_eq!(
        env.type_entry("Pair3"),
        Some(TypeEntry::TypeCtor { arity: 3 })
    );
    assert!(env.is_type_name("P"));
    assert_eq!(
        env.vty_of_name("P"),
        Some(ValueTy::Data("P".into(), vec![]))
    );

    let out = compile("data P = { x: Int }; f :: P -> Int; f p = p.x; main = f (P { x: 7 });")
        .expect("a declared data type must be accepted");
    assert!(out.contains("Int(7)"), "got {out}");

    // The same for the other declaration forms: `newtype`, `type` synonym, and
    // an abstract type.
    let env = env_of(
        "newtype Hz = Float; type Angles = Float; type HzAlias = Hz; data Opaque; \
         data Box a = { unBox: a };",
    );
    assert_eq!(env.type_entry("Hz"), Some(TypeEntry::Newtype));
    assert_eq!(
        env.type_entry("Angles"),
        Some(TypeEntry::Alias("Float".into()))
    );
    assert_eq!(
        env.type_entry("HzAlias"),
        Some(TypeEntry::Alias("Hz".into()))
    );
    assert_eq!(
        env.type_entry("Opaque"),
        Some(TypeEntry::TypeCtor { arity: 0 })
    );
    assert_eq!(
        env.type_entry("Box"),
        Some(TypeEntry::TypeCtor { arity: 1 })
    );
    // Builtin constructors are registry entries too, with their arity.
    assert_eq!(
        env.type_entry("List"),
        Some(TypeEntry::TypeCtor { arity: 1 })
    );
    assert_eq!(
        env.type_entry("Map"),
        Some(TypeEntry::TypeCtor { arity: 2 })
    );

    // An unknown type in a `data` FIELD is rejected too (the same rule, at the
    // declaration site rather than a signature).
    let err = compile_err("data Bad = { n: Nope }; main = 1;");
    assert!(err.contains("unknown type name `Nope`"), "got {err}");
}

#[test]
fn the_unit_type_is_the_parentheses() {
    // `()` is the unit type (Haskell 2010 spells it as the empty tuple).
    let out = compile("f :: () -> (); f x = x; main = f ();").expect("`()` must be the unit type");
    assert!(out.contains("Void"), "got {out}");

    let env = TypeEnv::with_builtins();
    assert!(env.is_type_name("()"));
    assert_eq!(env.vty_of_name("()"), Some(ValueTy::Void));
    // ... and `Void` is NOT a source-level name: it was the old rill spelling
    // and Haskell 2010 has no such type name.
    assert_eq!(env.type_entry("Void"), None);
    let err = compile_err("f :: Void -> Int; f x = 1; main = f 1;");
    assert!(err.contains("unknown type name `Void`"), "got: {err}");
}

#[test]
fn the_registry_is_the_single_source() {
    // Add ONE name to the registry and assert every consumer sees it. Each
    // consumer that keeps its own list is a failing assertion below.
    //
    // `register_scalar` keeps one name per value type, so registering
    // `MyFloat` for `Float` RENAMES `Float` — otherwise a consumer could keep
    // answering for the old spelling and this test could pass trivially.
    let mut baseline = TypeEnv::with_builtins();
    baseline.derive_eq_ord();
    let mut env = TypeEnv::with_builtins();
    env.register_scalar(
        "MyFloat",
        ScalarType {
            vty: ValueTy::Float,
            ffi_scalar: true,
        },
    );

    // 1. The registry itself.
    assert!(matches!(
        env.type_entry("MyFloat"),
        Some(TypeEntry::Scalar(_))
    ));
    assert_eq!(env.type_entry("Float"), None, "the old spelling is gone");

    // 2. The declaration check (a signature mentioning the name must pass, and
    //    the old spelling must now fail).
    assert!(env.is_type_name("MyFloat"));
    assert!(!env.is_type_name("Float"));
    let defs = rill_lang::parser::parse(
        &rill_lang::lexer::tokenize("f :: MyFloat -> MyFloat; f x = x; main = x;").unwrap(),
        b"f :: MyFloat -> MyFloat; f x = x; main = x;",
    )
    .unwrap();
    assert!(
        env.validate_type_names(&defs.defs).is_ok(),
        "the declaration check must accept a registered name"
    );
    let old = rill_lang::parser::parse(
        &rill_lang::lexer::tokenize("f :: Float -> Float; f x = x; main = x;").unwrap(),
        b"f :: Float -> Float; f x = x; main = x;",
    )
    .unwrap();
    assert!(
        env.validate_type_names(&old.defs).is_err(),
        "the declaration check must reject a name the registry no longer has"
    );

    // 3. The resolver.
    assert_eq!(env.vty_of_name("MyFloat"), Some(ValueTy::Float));
    assert_eq!(env.vty_of_name("Float"), None);

    // 4. The derived `Eq`/`Ord` instance seed.
    env.derive_eq_ord();
    assert!(
        env.instances["Eq"].contains_key("MyFloat"),
        "the derived Eq seed must include a registered scalar"
    );
    assert!(
        env.instances["Ord"].contains_key("MyFloat"),
        "the derived Ord seed must include a registered scalar"
    );
    assert!(
        !env.instances["Eq"].contains_key("Float"),
        "the derived seed must not keep the old spelling: {:?}",
        env.instances["Eq"].keys().collect::<Vec<_>>()
    );
    // Control: the pre-existing entries are untouched, so the assertion above
    // is about the renamed name rather than about a broken seed.
    for keep in ["Int", "Bool", "String", "Buffer", "()"] {
        assert!(
            env.instances["Eq"].contains_key(keep),
            "`{keep}` must keep its derived instance: {:?}",
            env.instances["Eq"].keys().collect::<Vec<_>>()
        );
        assert!(
            baseline.instances["Eq"].contains_key(keep),
            "control: the unmodified environment derives `{keep}` too"
        );
    }

    // 5. The FFI scalar-vs-record classification.
    assert!(env.is_ffi_scalar("MyFloat"));
    assert!(!env.is_ffi_scalar("Float"));
    // The INVERSE question (is this name a type variable?) is the same registry
    // arm: `is_builtin_sig_var` — the builtin-signature converter — is one call
    // to `is_type_variable`, so a registered scalar is never mistaken for a
    // variable and a lowercase name always is. (A program cannot otherwise
    // observe that converter: it only reads compiler-owned builtin signatures,
    // so this is the closest reachable assertion. See the report.)
    assert!(!env.is_type_variable("MyFloat"));
    assert!(!env.is_type_variable("Float"));
    assert!(env.is_type_variable("a"));
    assert!(!env.is_type_variable("Int"));
    let te = TypeExpr::TFunc(
        vec![TypeExpr::TName("MyFloat".into())],
        Box::new(TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![TypeExpr::TName("z".into())],
        )),
    );
    let sig = ffi_sig_from_typeexpr(&env, "probe", &te).expect("FFI sig");
    assert_eq!(
        sig.params,
        vec![rill_lang::types::ffi::FfiParam::Scalar],
        "a registered FFI scalar must classify as a Scalar parameter"
    );
    let old_te = TypeExpr::TFunc(
        vec![TypeExpr::TName("Float".into())],
        Box::new(TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![TypeExpr::TName("z".into())],
        )),
    );
    let old_sig = ffi_sig_from_typeexpr(&env, "probe", &old_te).expect("FFI sig");
    assert_eq!(
        old_sig.params,
        vec![rill_lang::types::ffi::FfiParam::Record("Float".into())],
        "the old spelling must no longer classify as a Scalar"
    );

    // 6. The diagnostic renderer.
    assert_eq!(env.type_spelling(&ValueTy::Float), "MyFloat");
    assert_eq!(
        env.type_spelling(&ValueTy::Thunk(vec![ValueTy::Float], vec![ValueTy::Float])),
        "MyFloat -> MyFloat"
    );
    // The unit type's spelling is the registry's `()` (it has no other name).
    assert_eq!(env.type_spelling(&ValueTy::Void), "()");
}
