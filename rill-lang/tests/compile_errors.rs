//! Compile-time rejection: bad conditions, mismatched branches, and
//! non-exhaustive / ill-typed matches.

use rill_lang::compile_eval;
use rill_lang::mode::Mode;

#[test]
fn if_non_bool_cond() {
    assert!(
        compile_eval::<f32, 256>("main = if 1.0 then 1.0 else 2.0;", "main", Mode::Strict).is_err()
    );
}

#[test]
fn if_branch_type_mismatch() {
    assert!(
        compile_eval::<f32, 256>("main = if true then 1.0 else \"s\";", "main", Mode::Strict)
            .is_err()
    );
}

#[test]
fn match_missing_ctor() {
    assert!(compile_eval::<f32, 256>(r"data Shape = Circle Double | Rect Double Double; main = case Circle 1.0 of { Circle r -> r; };", "main", Mode::Strict)
    .is_err());
}

#[test]
fn match_scalar_without_wildcard() {
    assert!(
        compile_eval::<f32, 256>(r"main = case 0 of { 0 -> 1.0; };", "main", Mode::Strict).is_err()
    );
}

#[test]
fn match_guarded_no_unguarded_fallback() {
    assert!(compile_eval::<f32, 256>(r"data Shape = Circle Double | Rect Double Double; main = case Circle 1.0 of { Circle r | r > 0.0 -> r; Rect w h -> w; };", "main", Mode::Strict)
    .is_err());
}

#[test]
fn match_ctor_arity_mismatch() {
    assert!(compile_eval::<f32, 256>(
        r"main = case Just 1.0 of { Just x y -> x; _ -> 0.0; };",
        "main",
        Mode::Strict
    )
    .is_err());
}

#[test]
fn match_unknown_ctor() {
    assert!(compile_eval::<f32, 256>(
        r"main = case Just 1.0 of { Nope x -> x; _ -> 0.0; };",
        "main",
        Mode::Strict
    )
    .is_err());
}

#[test]
fn match_signal_scrutinee() {
    // A signal-rate scrutinee is not a value channel.
    assert!(compile_eval::<f32, 256>(
        r"main = case (_ * 2.0) of { _ -> 0.0; };",
        "main",
        Mode::Strict
    )
    .is_err());
}
