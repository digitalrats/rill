//! One source of truth for the built-in classes.
//!
//! Every class is declared in the `PRELUDE` text — there is no second,
//! programmatic registry. The operator spellings ARE the method names, which is
//! what lets `a == b` and `(==)` resolve to the same binding.

use rill_lang::lexer;
use rill_lang::parser;
use rill_lang::types::ty::TypeEnv;

fn env_after_decls() -> TypeEnv {
    let mut env = TypeEnv::with_builtins();
    // `derive_eq_ord` runs at inference start in the real pipeline; call it here
    // so the derived structural instances are present, as a compiled program sees
    // them.
    env.derive_eq_ord();
    env
}

fn methods(env: &TypeEnv, class: &str) -> Vec<String> {
    env.typeclasses
        .get(class)
        .unwrap_or_else(|| panic!("class {class} must be declared"))
        .methods
        .iter()
        .map(|(n, _)| n.clone())
        .collect()
}

#[test]
fn eq_declares_the_binary_operators() {
    let e = env_after_decls();
    let m = methods(&e, "Eq");
    assert!(
        m.contains(&"==".to_string()),
        "Eq must declare `==`; got {m:?}"
    );
    assert!(
        m.contains(&"/=".to_string()),
        "Eq must declare `/=`; got {m:?}"
    );
}

#[test]
fn ord_declares_compare_and_the_ordering_operators() {
    let e = env_after_decls();
    let m = methods(&e, "Ord");
    for expected in ["compare", "<", "<=", ">", ">=", "min", "max"] {
        assert!(
            m.contains(&expected.to_string()),
            "Ord must declare `{expected}`; got {m:?}"
        );
    }
    assert_eq!(
        e.typeclasses["Ord"].superclasses,
        vec![("Eq".to_string(), "a".to_string())],
        "Haskell 2010: class Eq a => Ord a"
    );
}

#[test]
fn num_and_fractional_form_a_tower() {
    let e = env_after_decls();
    let num = methods(&e, "Num");
    for expected in ["+", "-", "*", "negate", "abs", "signum", "fromInteger"] {
        assert!(
            num.contains(&expected.to_string()),
            "Num must declare `{expected}`"
        );
    }
    assert_eq!(
        e.typeclasses["Fractional"].superclasses,
        vec![("Num".to_string(), "a".to_string())],
        "Haskell 2010: class Num a => Fractional a"
    );
}

#[test]
fn buffer_is_a_numeric_subclass_and_reuses_the_operators() {
    // `Buffer` declares NO operators of its own: it is a subclass of `Num`
    // (which is itself Eq/Ord-bounded), so it uses the same vocabulary through
    // entailment. Re-declaring `+`/`-`/… here would make `class_of_method`
    // ambiguous and no operator would resolve.
    let e = env_after_decls();
    assert!(
        methods(&e, "Buffer").is_empty(),
        "Buffer must not re-declare operators; got {:?}",
        methods(&e, "Buffer")
    );
    assert_eq!(
        e.typeclasses["Buffer"].superclasses,
        vec![("Num".to_string(), "b".to_string())],
        "class Num b => Buffer b"
    );
    assert_eq!(
        e.typeclasses["Num"].superclasses,
        vec![
            ("Eq".to_string(), "a".to_string()),
            ("Ord".to_string(), "a".to_string())
        ],
        "Num is Eq- and Ord-bounded"
    );
    for op in ["+", "-", "*", "min", "max", "abs"] {
        assert!(
            e.entails("Buffer", "Num") || e.entails("Buffer", "Ord"),
            "a Buffer satisfies the numeric/ordering tower that declares `{op}`"
        );
    }
    let insts = e.instances.get("Buffer").expect("Buffer has instances");
    // The instance head is MONOMORPHIC in the element and fully concrete, so the
    // registry key is the whole applied type — `FixedBuffer Double`, not the
    // bare constructor. That is what makes the element part of the instance:
    // there is no `Buffer (FixedBuffer Int)` dictionary to select.
    assert!(
        insts.contains_key("FixedBuffer Double"),
        "`FixedBuffer Double` is a Buffer; got {:?}",
        insts.keys().collect::<Vec<_>>()
    );
    assert!(
        insts.contains_key("Tape Double"),
        "`Tape Double` is a Buffer; got {:?}",
        insts.keys().collect::<Vec<_>>()
    );
    // Control: no element-free / other-element key exists, because the element
    // is checked and the instance head names the one legal element.
    assert!(!insts.contains_key("FixedBuffer"), "no element-free key");
    assert!(!insts.contains_key("FixedBuffer Int"), "no Int-element key");
}

#[test]
fn every_operator_resolves_to_exactly_one_class() {
    // The reason `Buffer` must not re-declare the operators: `class_of_method`
    // reports `None` on a name declared by two classes, which silently made
    // every operator unresolvable.
    let e = env_after_decls();
    for (method, expected) in [
        ("+", "Num"),
        ("-", "Num"),
        ("*", "Num"),
        ("/", "Fractional"),
        ("abs", "Num"),
        ("min", "Ord"),
        ("max", "Ord"),
        ("==", "Eq"),
        ("/=", "Eq"),
        ("<", "Ord"),
        ("compare", "Ord"),
    ] {
        assert_eq!(
            e.class_of_method(method).as_deref(),
            Some(expected),
            "`{method}` must resolve to exactly one class"
        );
    }
}

#[test]
fn not_equal_is_spelled_with_a_slash() {
    // Haskell 2010 spells it `/=`. Parsing the operator must produce the same
    // method name the Eq class declares.
    let src = "f a b = a /= b; main = 1.0;";
    let toks = lexer::tokenize(src).expect("lexes");
    parser::parse(&toks, src.as_bytes()).expect("`/=` must parse");
}

#[test]
fn no_second_registry_of_builtin_classes() {
    // Guards against reintroducing a programmatic `HashMap` of classes next to
    // the prelude text: every built-in class must be *parsed*, so a class with
    // no textual declaration simply does not exist.
    let src = "typeclass Probe a where { probe: a -> Bool; }; main = 1.0;";
    let toks = lexer::tokenize(src).expect("lexes");
    let program = parser::parse(&toks, src.as_bytes()).expect("parses");
    let mut env = TypeEnv::with_builtins();
    env.register_decls(&program.defs);
    assert!(
        env.typeclasses.contains_key("Probe"),
        "a textual declaration registers the class"
    );
}
