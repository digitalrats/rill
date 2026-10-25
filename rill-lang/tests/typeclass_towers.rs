//! General typeclass towers: superclass declarations and transitive entailment.
//!
//! System FC: a superclass constraint is a FIELD of the subclass dictionary.
//! `Ord a` therefore contains an `Eq a` dictionary, so an `Eq` method is
//! usable wherever `Ord a` is in scope; `Fractional a` contains `Num a`, and so
//! on down the tower.

use rill_lang::lexer;
use rill_lang::parser;
use rill_lang::types::ty::TypeEnv;

fn env() -> TypeEnv {
    TypeEnv::with_builtins()
}

fn parse_ok(src: &str) {
    let toks = lexer::tokenize(src).expect("lexes");
    parser::parse(&toks, src.as_bytes()).unwrap_or_else(|e| panic!("must parse: {e}\nsrc: {src}"));
}

#[test]
fn superclass_declaration_parses() {
    // Both the parenthesised multi-constraint form and the plain one.
    parse_ok("typeclass (Eq a) => Ord2 a where { cmp: a -> a -> Bool; };");
    parse_ok("typeclass (Eq a, Show a) => Both a where { m: a -> Bool; };");
    parse_ok("typeclass Plain a where { m: a -> Bool; };");
}

#[test]
fn operator_method_names_parse_in_a_class_body() {
    // Haskell 2010 §4.4.1: a class method may be spelled as an operator in
    // parentheses, which is the spelling the operator migration needs.
    parse_ok("typeclass Semigroupish a where { (<>): a -> a -> a; };");
    parse_ok("typeclass Ordish a where { (<): a -> a -> Bool; (<=): a -> a -> Bool; };");
}

#[test]
fn ord_is_declared_with_eq_as_its_superclass() {
    let env = env();
    let ord = env.typeclasses.get("Ord").expect("Ord is a builtin class");
    assert!(
        ord.superclasses.iter().any(|(c, _)| c == "Eq"),
        "Haskell 2010: `class Eq a => Ord a`; got {:?}",
        ord.superclasses
    );
}

#[test]
fn entailment_is_reflexive() {
    let env = env();
    assert!(env.entails("Ord", "Ord"));
    assert!(env.entails("Eq", "Eq"));
}

#[test]
fn entailment_is_transitive() {
    let env = env();
    // `Ord a` entails `Eq a` (direct).
    assert!(env.entails("Ord", "Eq"));
    // `Monad m` entails `Applicative m` entails `Functor f` (two levels).
    assert!(env.entails("Monad", "Applicative"));
    assert!(env.entails("Monad", "Functor"));
}

#[test]
fn entailment_does_not_invent_relationships() {
    let env = env();
    // `Eq a` does NOT entail `Ord a`: the tower points one way.
    assert!(!env.entails("Eq", "Ord"));
    // Unrelated classes stay unrelated.
    assert!(!env.entails("Monoid", "Ord"));
}

#[test]
fn the_entailed_closure_lists_every_class_once() {
    let env = env();
    let closure = env.entailed_classes("Monad");
    assert_eq!(
        closure.iter().filter(|c| *c == "Monad").count(),
        1,
        "the class appears once"
    );
    assert!(closure.iter().any(|c| c == "Applicative"));
    assert!(closure.iter().any(|c| c == "Functor"));
    assert!(
        closure.len() <= env.typeclasses.len(),
        "a cyclic declaration must not loop forever"
    );
}

#[test]
fn existing_monad_superclass_derivation_still_works() {
    // The general walk must reproduce the old hard-coded behaviour: an
    // `instance Monad T` yields `Applicative T` and `Functor T`.
    let env = env();
    let applicative = env.instances.get("Applicative").expect("derived");
    let functor = env.instances.get("Functor").expect("derived");
    for ty in ["Maybe", "List", "IO"] {
        assert!(
            applicative.contains_key(ty),
            "Applicative {ty} must be derived from Monad {ty}"
        );
        assert!(
            functor.contains_key(ty),
            "Functor {ty} must be derived from Monad {ty}"
        );
    }
}

#[test]
fn a_user_tower_is_registered_and_entailed() {
    // A user-declared class with a superclass participates in the same
    // entailment without any code change in the derivation pass.
    let mut env = env();
    let src = "typeclass (Eq a) => MyOrd a where { mycmp: a -> a -> Bool; };";
    let toks = lexer::tokenize(src).expect("lexes");
    let program = parser::parse(&toks, src.as_bytes()).expect("parses");
    env.register_decls(&program.defs);
    let info = env.typeclasses.get("MyOrd").expect("registered");
    assert_eq!(
        info.superclasses,
        vec![("Eq".to_string(), "a".to_string())],
        "the declared superclass is recorded"
    );
    assert!(env.entails("MyOrd", "Eq"), "and it is entailed");
}
