//! Compile-time rejection: bad conditions, mismatched branches, and
//! non-exhaustive / ill-typed matches.

use rill_lang::compile;

#[test]
fn if_non_bool_cond() {
    assert!(compile::<f32>("main = if 1.0 then 1.0 else 2.0;").is_err());
}

#[test]
fn if_branch_type_mismatch() {
    assert!(compile::<f32>("main = if true then 1.0 else \"s\";").is_err());
}

#[test]
fn match_missing_ctor() {
    assert!(compile::<f32>(
        "data Shape = Circle Float | Rect Float Float; \
         main = match Circle 1.0 of { Circle r => r; };"
    )
    .is_err());
}

#[test]
fn match_scalar_without_wildcard() {
    assert!(compile::<f32>("main = match 0 of { 0 => 1.0; };").is_err());
}

#[test]
fn match_guarded_no_unguarded_fallback() {
    assert!(compile::<f32>(
        "data Shape = Circle Float | Rect Float Float; \
         main = match Circle 1.0 of { Circle r | r > 0.0 => r; Rect w h => w; };"
    )
    .is_err());
}

#[test]
fn match_ctor_arity_mismatch() {
    assert!(compile::<f32>("main = match Just 1.0 of { Just x y => x; _ => 0.0; };").is_err());
}

#[test]
fn match_unknown_ctor() {
    assert!(compile::<f32>("main = match Just 1.0 of { Nope x => x; _ => 0.0; };").is_err());
}

#[test]
fn match_signal_scrutinee() {
    // A signal-rate scrutinee is not a value channel.
    assert!(compile::<f32>("main = match (_ * 2.0) of { _ => 0.0; };").is_err());
}
