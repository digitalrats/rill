//! Rank-2 polymorphism: `runST`'s phantom must not escape the region through
//! the two verified one-point-approach bypasses (rank-N generalization rejects
//! them automatically).

#[test]
fn phantom_chain_through_higher_id_var_rejected() {
    // `[t, s]` unifies the enclosing lambda's parameter `t` with the runST
    // phantom `s`: the result type of the region must NOT unify an outer
    // variable with the quantified phantom (the phantom leaks through `t`).
    let src = r"{-# MODE complete #-}
main = (\t -> runST (\s -> [t, s])) 1.0;";
    let p = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&p, src.as_bytes()).unwrap();
    let r = rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Complete);
    assert!(
        r.is_err(),
        "phantom s must not unify with an outer variable"
    );
}

#[test]
fn phantom_shadows_outer_value_binder() {
    // An outer value definition named `s` must be SHADOWED by the runST
    // phantom: `Ref("s")` in the body resolves to the phantom, so the result
    // escapes and is rejected (not silently bound to the outer Double).
    let src = r"{-# MODE complete #-}
s = 2.0;
main = runST (\s -> (s));";
    let p = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&p, src.as_bytes()).unwrap();
    let r = rill_lang::types::infer::infer_program(&prog, rill_lang::mode::Mode::Complete);
    assert!(r.is_err(), "phantom s must shadow an outer value named s");
}
