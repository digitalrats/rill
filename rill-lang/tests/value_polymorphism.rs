//! Value-type-variable polymorphism: a value-typed definition whose type
//! mentions a free value-type variable must generalize it into `Scheme.vars`
//! and substitute it at each instantiation site (rank-N generalization).
//!
//! Without generalization, `id2 = fn x -> x` types as `Func([Var(v)],[Var(v)])`
//! with `v` un-quantified: every use shares the SAME variable, so the first
//! application binds `v` and the second use fails to unify.

use rill_lang::lexer::tokenize;
use rill_lang::mode::Mode;
use rill_lang::parser::parse;
use rill_lang::types::infer::infer_program;

#[test]
fn polymorphic_value_fn_instantiates_fresh() {
    let src = r"{-# MODE complete #-}
id2 = \x -> x;
foo = case (head (id2 [1.0, 2.0])) of { Nothing -> 0.0; Just x -> x };";
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes()).unwrap();
    assert!(
        infer_program(&prog, Mode::Complete).is_ok(),
        "a polymorphic value function must generalize its type variable"
    );
}

#[test]
fn polymorphic_value_fn_used_at_two_types() {
    let src = r"{-# MODE complete #-}
id2 = \x -> x;
foo = case (head (id2 [1.0, 2.0])) of { Nothing -> 0.0; Just x -> id2 x };";
    let toks = tokenize(src).unwrap();
    let prog = parse(&toks, src.as_bytes()).unwrap();
    assert!(infer_program(&prog, Mode::Complete).is_ok());
}
