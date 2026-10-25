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
fn buffer_uses_the_same_arithmetic_vocabulary() {
    let e = env_after_decls();
    let m = methods(&e, "Buffer");
    for expected in ["+", "-", "*", "/", "min", "max", "abs"] {
        assert!(
            m.contains(&expected.to_string()),
            "Buffer must reuse the shared arithmetic vocabulary (`{expected}`); got {m:?}"
        );
    }
    let insts = e.instances.get("Buffer").expect("Buffer has instances");
    assert!(insts.contains_key("FixedBuffer"), "FixedBuffer is a Buffer");
    assert!(insts.contains_key("Tape"), "Tape is a Buffer");
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
