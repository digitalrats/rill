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
