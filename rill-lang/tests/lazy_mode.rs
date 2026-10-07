use rill_core::traits::MultichannelAlgorithm;
use rill_lang::mode::Mode;
use rill_lang::{compile, compile_with_mode};

#[test]
fn recursion_rejected_in_strict() {
    assert!(compile::<f32>("a = a; main = a").is_err());
}

#[test]
fn recursion_accepted_in_complete() {
    // Compiles; running `main = a` where `a = a` yields bottom at runtime
    // (tested in Task A6) — here we only require it to COMPILE.
    let r = compile_with_mode::<f32>("a = a; main = a", Mode::Complete);
    assert!(r.is_ok(), "complete mode must accept recursion");
}

#[test]
fn nonrecursive_value_func_types_as_value() {
    // A non-recursive function returning a cons (List) must type as a VALUE
    // function; if this fails, `from`-style recursion fails for the same
    // rate-resolution reason even before the recursion matters.
    let src = "{-# MODE complete #-}\nwrap n = cons n (list)\nmain = wrap 0.0";
    match rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete) {
        Ok(_) => {}
        Err(e) => panic!("non-recursive value func must compile in complete, got: {e}"),
    }
}

#[test]
fn recursive_float_value_func() {
    // Recursion alone (Float -> Float, no List) — isolates the rate issue.
    // (`+` is used because value-track infix `-` is a pre-existing parser
    // quirk — `+` in a lambda works, `-` does not.)
    let src = "{-# MODE complete #-}\ncountup n = if n == 3.0 then 3.0 else countup (n + 1.0)\nmain = countup 0.0";
    match rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete) {
        Ok(_) => {}
        Err(e) => panic!("recursive float func must compile in complete, got: {e}"),
    }
}

#[test]
fn countdown_compile_only() {
    let src = "{-# MODE complete #-}\ncountup n = if n == 3.0 then 3.0 else countup (n + 1.0)\nmain = countup 0.0";
    let _ = rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
}

#[test]
fn lambda_arith_plus_works() {
    let src = "{-# MODE complete #-}\nmain = (fn a b -> a + b) 5.0 2.0";
    match rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete) {
        Ok(_) => {}
        Err(e) => panic!("lambda plus must compile, got: {e}"),
    }
}
