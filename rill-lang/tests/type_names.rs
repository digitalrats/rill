//! Type-NAME conformance (Haskell 2010 §3.1): a name denotes a type only if
//! something declared it, and a lowercase name is a type variable.
//!
//! rill used to accept any name in a type position, so `f :: Zork -> Zork`
//! compiled and ran although no `Zork` value exists. The registry
//! ([`TypeEnv::type_entry`]) is now the single answer to "does this name denote
//! a type": the declaration check, the resolver, the derived-instance seed, the
//! FFI classification and the diagnostic renderer all read it, so they cannot
//! disagree again.
//!
//! `Integer` used to be the example here. It is a REGISTERED scalar now
//! (Report §6.4), so the placeholder is `Zork` and the guarantee is unchanged.

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
    // `Integer` IS a registered Haskell 2010 Prelude type now (Report §6.4), so
    // the placeholder this test used to name is gone. The GUARANTEE it asserts
    // is unchanged: an uppercase name nothing declares denotes nothing, so a
    // signature mentioning it is an unknown-type error (GHC's `Not in scope:
    // type constructor or class 'Zork'`).
    let err = compile_err("f :: Zork -> Zork; f x = x; main = f 1;");
    assert!(
        err.contains("unknown type name `Zork`"),
        "the error must name the undeclared type, got: {err}"
    );

    // ... and the same holds for any other undeclared uppercase name.
    for name in ["Zork", "Word", "Maybe2", "Listy"] {
        let src = format!("f :: {name} -> Int; f x = 1; main = f 1;");
        let err = compile_err(&src);
        assert!(
            err.contains(&format!("unknown type name `{name}`")),
            "`{name}` must be rejected as an unknown type, got: {err}"
        );
    }

    // The other half, now that `Integer` is real: the name resolves in the
    // registry and a signature over it compiles — `f :: Integer -> Integer` is
    // no longer the phantom-type defect this test was written for.
    let env = TypeEnv::with_builtins();
    assert_eq!(env.vty_of_name("Integer"), Some(ValueTy::Integer));
    compile("f :: Integer -> Integer; f x = x; main = 1.0;")
        .expect("a registered type name must be accepted");
}

#[test]
fn char_is_still_undeclared_for_now() {
    // `Char` is a real Haskell 2010 type, but rill has no `Char` payload yet
    // (that is a later sub-task). Until it is REGISTERED it must be rejected —
    // accepting an undeclared name would resurrect the phantom-type defect for
    // the one name the next task is about to make real. `Integer` shows the
    // other half of the rule: registered, so accepted.
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
fn float_is_undeclared_after_the_rename() {
    // `Float` used to be rill's name for the double-precision type. The type is
    // now spelled `Double` (Haskell 2010 §6.4.1: `Float` is the
    // single-precision type and rill does not have one yet), so the old
    // spelling must denote NOTHING — exactly like `Char` above. A lingering
    // alias would keep the name lying about its precision.
    let err = compile_err("f :: Float -> Float; f x = x; main = f 1.0;");
    assert!(
        err.contains("unknown type name `Float`"),
        "`Float` must be rejected as an unknown type, got: {err}"
    );

    // The registry agrees: no entry, no value type, and `Double` took its place.
    let env = TypeEnv::with_builtins();
    assert_eq!(env.type_entry("Float"), None);
    assert_eq!(env.vty_of_name("Float"), None);
    assert!(!env.is_type_name("Float"));
    assert_eq!(env.vty_of_name("Double"), Some(ValueTy::Double));
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
    assert!(out.contains("Double(2.0)"), "got {out}");
}

#[test]
fn a_lowercase_name_is_a_type_variable() {
    // A lowercase name in a type position is an ordinary identifier, which
    // Haskell's lexical rules make a TYPE VARIABLE (GHC accepts
    // `instance Num (Tape a)` as an instance for all `a`). It must therefore not
    // be rejected as an unknown type, and must not be turned into a nominal type
    // either. A type-variable buffer ELEMENT is a genuine polymorphic signature
    // (identity is legal at every element); a CONCRETE wrong element is a type
    // error — see `buffer_element_is_checked`.
    let out = compile("f :: FixedBuffer a -> FixedBuffer a; f x = x; main = 1.0;")
        .expect("a lowercase name must be accepted as a type variable");
    assert!(out.contains("Double(1.0)"), "got {out}");

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
    assert_eq!(env.vty_of_name("P"), Some(ValueTy::Con("P".into(), vec![])));

    let out = compile("data P = { x: Int }; f :: P -> Int; f p = p.x; main = f (P { x: 7 });")
        .expect("a declared data type must be accepted");
    assert!(out.contains("Int(7)"), "got {out}");

    // The same for the other declaration forms: `newtype`, `type` synonym, and
    // an abstract type.
    let env = env_of(
        "newtype Hz = Double; type Angles = Double; type HzAlias = Hz; data Opaque; \
         data Box a = { unBox: a };",
    );
    assert_eq!(env.type_entry("Hz"), Some(TypeEntry::Newtype));
    assert_eq!(
        env.type_entry("Angles"),
        Some(TypeEntry::Alias("Double".into()))
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
fn a_type_synonym_may_name_a_parameterized_type() {
    // Haskell 2010 §4.1.2 allows any type on the right of `type`, so a synonym
    // may name a CONSTRUCTOR APPLICATION. Before this task the parser accepted
    // only a bare identifier (`expected Eq, found Semi`), so a synonym of a
    // parameterized type could not be written at all.
    //
    // `Ratio` is registered later (the numeric-literal stage); the syntax is
    // proved here through a user type, which is the same `TypeExpr::TApp`.
    let out = compile(
        "data Box a = { v: a };\ntype IntBox = Box Int;\nf :: IntBox -> Int;\nf b = b.v;\n\
         main = f (Box { v: 3 });",
    )
    .expect("a synonym may name a parameterized type");
    assert!(out.contains("Int(3)"), "got {out}");

    // The registry resolves the alias to the applied type, not to a name.
    // `Int` resolves to its LEAF value type, so the payload of `Box` is
    // `ValueTy::Int` — the same type a `Box Int` signature parameter has.
    let env = env_of("data Box a = { v: a }; type IntBox = Box Int;");
    assert_eq!(
        env.vty_of_name("IntBox"),
        Some(ValueTy::Con("Box".into(), vec![ValueTy::Int]))
    );
    assert_eq!(
        env.kind_of("IntBox"),
        Some(rill_lang::types::ty::Kind::TYPE)
    );

    // A synonym may name ANOTHER synonym of a parameterized type, and the
    // target may nest applications.
    let env = env_of(
        "data Box a = { v: a }; type IntBox = Box Int; type IntBox2 = IntBox; \
         type PairBox = Box (Box Int);",
    );
    assert_eq!(
        env.vty_of_name("IntBox2"),
        Some(ValueTy::Con("Box".into(), vec![ValueTy::Int]))
    );
    assert_eq!(
        env.vty_of_name("PairBox"),
        Some(ValueTy::Con(
            "Box".into(),
            vec![ValueTy::Con("Box".into(), vec![ValueTy::Int])]
        ))
    );

    // A synonym may name a FUNCTION type too (Haskell 2010 §4.1.2 puts no
    // restriction on the right-hand side).
    let out = compile("type Op = Int -> Int;\nf :: Op;\nf x = x;\nmain = f 5")
        .expect("a synonym may name a function type");
    assert!(out.contains("Int(5)"), "got {out}");
}

#[test]
fn a_cyclic_type_synonym_is_a_clean_error() {
    // A synonym cycle denotes no type. It must be a diagnostic — not a hang and
    // not a stack overflow: `type A = B; type B = A` (a bare-name cycle) and
    // `type A = Box A` (an expansion cycle through an application) are both
    // rejected.
    for src in [
        "type A = B; type B = A; main = 1.0;",
        "data Box a = { v: a }; type A = Box A; main = 1.0;",
    ] {
        let err = compile_err(src);
        assert!(
            err.contains("cyclic type synonym"),
            "expected a cyclic-synonym diagnostic for `{src}`, got: {err}"
        );
    }
}

#[test]
fn the_registry_is_the_single_source() {
    // Add ONE name to the registry and assert every consumer sees it. Each
    // consumer that keeps its own list is a failing assertion below.
    //
    // `register_scalar` keeps one name per value type, so registering
    // `MyDouble` for `Double` RENAMES `Double` — otherwise a consumer could keep
    // answering for the old spelling and this test could pass trivially.
    let mut baseline = TypeEnv::with_builtins();
    baseline.derive_eq_ord();
    let mut env = TypeEnv::with_builtins();
    env.register_scalar(
        "MyDouble",
        ScalarType {
            vty: ValueTy::Double,
            ffi_scalar: true,
            buffer_element: true,
        },
    );

    // 1. The registry itself.
    assert!(matches!(
        env.type_entry("MyDouble"),
        Some(TypeEntry::Scalar(_))
    ));
    assert_eq!(env.type_entry("Double"), None, "the old spelling is gone");

    // 2. The declaration check (a signature mentioning the name must pass, and
    //    the old spelling must now fail).
    assert!(env.is_type_name("MyDouble"));
    assert!(!env.is_type_name("Double"));
    let defs = rill_lang::parser::parse(
        &rill_lang::lexer::tokenize("f :: MyDouble -> MyDouble; f x = x; main = x;").unwrap(),
        b"f :: MyDouble -> MyDouble; f x = x; main = x;",
    )
    .unwrap();
    assert!(
        env.validate_type_names(&defs.defs).is_ok(),
        "the declaration check must accept a registered name"
    );
    let old = rill_lang::parser::parse(
        &rill_lang::lexer::tokenize("f :: Double -> Double; f x = x; main = x;").unwrap(),
        b"f :: Double -> Double; f x = x; main = x;",
    )
    .unwrap();
    assert!(
        env.validate_type_names(&old.defs).is_err(),
        "the declaration check must reject a name the registry no longer has"
    );

    // 3. The resolver.
    assert_eq!(env.vty_of_name("MyDouble"), Some(ValueTy::Double));
    assert_eq!(env.vty_of_name("Double"), None);

    // 4. The derived `Eq`/`Ord` instance seed.
    env.derive_eq_ord();
    assert!(
        env.instances["Eq"].contains_key("MyDouble"),
        "the derived Eq seed must include a registered scalar"
    );
    assert!(
        env.instances["Ord"].contains_key("MyDouble"),
        "the derived Ord seed must include a registered scalar"
    );
    assert!(
        !env.instances["Eq"].contains_key("Double"),
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
    assert!(env.is_ffi_scalar("MyDouble"));
    assert!(!env.is_ffi_scalar("Double"));
    // The INVERSE question (is this name a type variable?) is the same registry
    // arm: `is_builtin_sig_var` — the builtin-signature converter — is one call
    // to `is_type_variable`, so a registered scalar is never mistaken for a
    // variable and a lowercase name always is. (A program cannot otherwise
    // observe that converter: it only reads compiler-owned builtin signatures,
    // so this is the closest reachable assertion. See the report.)
    assert!(!env.is_type_variable("MyDouble"));
    assert!(!env.is_type_variable("Double"));
    assert!(env.is_type_variable("a"));
    assert!(!env.is_type_variable("Int"));
    // The buffer element is a registry property too, so the renamed scalar is
    // the only legal element here: `FixedBuffer MyDouble` converts, while the
    // old spelling `FixedBuffer Double` no longer names a legal element.
    let te = TypeExpr::TFunc(
        vec![TypeExpr::TName("MyDouble".into())],
        Box::new(TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![TypeExpr::TName("MyDouble".into())],
        )),
    );
    let sig = ffi_sig_from_typeexpr(&env, "probe", &te).expect("FFI sig");
    assert_eq!(
        sig.params,
        vec![rill_lang::types::ffi::FfiParam::Scalar],
        "a registered FFI scalar must classify as a Scalar parameter"
    );
    let old_te = TypeExpr::TFunc(
        vec![TypeExpr::TName("Double".into())],
        Box::new(TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![TypeExpr::TName("MyDouble".into())],
        )),
    );
    let old_sig = ffi_sig_from_typeexpr(&env, "probe", &old_te).expect("FFI sig");
    assert_eq!(
        old_sig.params,
        vec![rill_lang::types::ffi::FfiParam::Record("Double".into())],
        "the old spelling must no longer classify as a Scalar"
    );
    // ... and the old spelling is no longer a legal buffer element.
    assert!(!env.is_buffer_element("Double"));
    assert!(env.is_buffer_element("MyDouble"));
    let bad_elem = TypeExpr::TFunc(
        vec![],
        Box::new(TypeExpr::TApp(
            "FixedBuffer".into(),
            vec![TypeExpr::TName("Double".into())],
        )),
    );
    assert!(
        ffi_sig_from_typeexpr(&env, "probe", &bad_elem).is_none(),
        "a buffer element the registry no longer marks must not convert"
    );

    // 6. The diagnostic renderer.
    assert_eq!(env.type_spelling(&ValueTy::Double), "MyDouble");
    assert_eq!(
        env.type_spelling(&ValueTy::Thunk(
            vec![ValueTy::Double],
            vec![ValueTy::Double]
        )),
        "MyDouble -> MyDouble"
    );
    // The unit type's spelling is the registry's `()` (it has no other name).
    assert_eq!(env.type_spelling(&ValueTy::Void), "()");
}

#[test]
fn a_synonym_target_is_still_a_type_position() {
    // Widening the syntax must not widen the TYPE CHECK: a lowercase name is a
    // type variable, so `type A = a` names no type (GHC: `Not in scope`), and a
    // bare constructor target still fails the kind rule.
    let err = compile_err("type A = a; main = 1.0;");
    assert!(
        err.contains("type A = a") || err.contains("`a`"),
        "got {err}"
    );
    let err = compile_err("type A = List; main = 1.0;");
    assert!(err.contains("has kind `* -> *`"), "got {err}");
    let err = compile_err("type A = Nope; main = 1.0;");
    assert!(err.contains("unknown type name `Nope`"), "got {err}");
    let err = compile_err("type A = Pair Int; main = 1.0;");
    assert!(err.contains("has kind `* -> *`"), "got {err}");
    let err = compile_err("data Box a = { v: a }; type A = Box Int Int; main = 1.0;");
    assert!(err.contains("too many arguments"), "got {err}");
}

#[test]
fn a_synonym_chain_through_an_alias_resolves() {
    // A free `type` target must have kind `*`, so a synonym cannot be used as
    // a constructor HEAD (`type B = List` is `type B = []`, rejected by GHC).
    // The MEANINGFUL chain is name → name, where the final target is applied:
    // `type IntList = List Int; type L = IntList` is `List Int`.
    let env = env_of("type IntList = List Int; type L = IntList;");
    assert_eq!(
        env.vty_of_name("L"),
        Some(ValueTy::Con("List".into(), vec![ValueTy::Int]))
    );
    let out = compile(
        "type IntList = List Int; type L = IntList; f :: L -> Int; f xs = length xs; main = f [7]",
    )
    .expect("a synonym chain ending in an application must resolve");
    assert!(out.contains("Int(1)"), "got {out}");
    // And a bare-constructor target is still a kind error, unchanged.
    let err = compile_err("type B = List; main = 1.0;");
    assert!(err.contains("has kind `* -> *`"), "got {err}");
}

#[test]
fn an_expansion_cycle_through_nesting_is_a_clean_error() {
    // A cycle does not have to be a bare name chain: `type A = Int -> A` and
    // `type A = Pair Int A` both expand forever. Both are diagnostics with a
    // span, and neither hangs nor overflows the stack.
    for src in [
        "type A = Int -> A; main = 1.0;",
        "type A = Pair Int A; main = 1.0;",
        "type A = B; type B = C; type C = Box A; data Box a = { v: a }; main = 1.0;",
    ] {
        let err = compile_err(src);
        assert!(
            err.contains("cyclic type synonym"),
            "expected a cyclic-synonym diagnostic for `{src}`, got: {err}"
        );
    }
    // A LONG but acyclic chain still resolves (the hop limit is the alias
    // count, not a fixed small number).
    let chain: String = (0..40)
        .map(|i| format!("type T{i} = T{};\n", i + 1))
        .collect();
    let src = format!("{chain}type T40 = Double; f :: T0 -> Double; f x = x; main = f 2.0");
    let out = compile(&src);
    assert!(
        out.is_ok(),
        "a 40-link acyclic chain must resolve: {:?}",
        out.err()
    );
}

#[test]
fn a_synonym_target_ends_at_the_next_declaration() {
    // The offside guard must stop a type APPLICATION from swallowing the next
    // declaration's head. Without it, `type IntBox = Box Int` on its own line
    // would read `Box Int f` and fail with an unrelated message.
    for src in [
        // Semicolon-separated, one line.
        "data Box a = { v: a }; type IntBox = Box Int; f :: IntBox -> Int; f b = b.v; main = f (Box { v: 4 })",
        // Newline-separated without semicolons.
        "data Box a = { v: a }\ntype IntBox = Box Int\nf :: IntBox -> Int\nf b = b.v\nmain = f (Box { v: 4 })",
        // The target continuing onto the next line is still one application.
        "data Box a = { v: a }\ntype IntBox =\n    Box Int\nf :: IntBox -> Int\nf b = b.v\nmain = f (Box { v: 4 })",
    ] {
        let out = compile(src).unwrap_or_else(|e| panic!("`{src}` must compile: {e}"));
        assert!(out.contains("Int(4)"), "got {out} for:\n{src}");
    }
}
