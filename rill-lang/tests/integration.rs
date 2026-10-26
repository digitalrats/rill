pub mod common;
use common::*;
use rill_lang::compile_eval;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;
use rill_lang::register;

/// Run a pure value program over literal data and read back its single output
/// as a list of floats.
fn run_values(src: &str) -> Vec<f64> {
    // A `where` block belongs to the `main` DEFINITION: `test_eval`'s entry
    // `__rill_eval = (expr)` cannot carry one (the parenthesised expression
    // must not contain a parse-level `where`), so keep the whole definition in
    // `defs` and evaluate `main` itself.
    let (defs, expr) = match src.rsplit_once("main = ") {
        Some((defs, tail)) if !tail.contains(" where ") => {
            (defs, tail.trim().trim_end_matches(';'))
        }
        _ => (src.trim().trim_end_matches(';'), "main"),
    };
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    let v = prog.result().expect("the run must produce a single result");
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::List { elems } => elems
            .iter()
            .map(|r| match prog.arena().get(*r).unwrap() {
                rill_lang::arena::Value::Double(f) => *f,
                other => panic!("expected a List of floats, got element {other:?}"),
            })
            .collect(),
        other => panic!("expected a List output, got {other:?}"),
    }
}

#[test]
fn dc_offset() {
    assert_eq!(
        run_values("main = map (\\x -> x + 1.0) [0.0, 1.0, 2.0]"),
        vec![1.0, 2.0, 3.0]
    );
}

#[test]
fn math_builtin_abs() {
    // `abs` is a buffer-algebra builtin: it is exercised over a constant -1.0
    // buffer (a zero-frequency square wave at amplitude -1).
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    let src = "main = abs (square 0.0 (-1.0) 0.0)";
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 4];
    assert!(prog.copy_buffer(r, &mut out));
    assert_eq!(out, [1.0, 1.0, 1.0, 1.0]);
}

/// The `_` input wire is gone from the unary value model, so "fanout" is now
/// expressed as a definition applied to literal data: the where-defined
/// `scale` maps its transform over the list.
#[test]
fn application_in_fanout_lowers_correctly() {
    assert_eq!(
        run_values("main = scale 0.5 where { scale k = map (\\x -> x * k) [2.0, 4.0, 8.0]; }"),
        vec![1.0, 2.0, 4.0]
    );
}

// Legacy application test — keep-compiled

#[test]
fn type_error_is_reported() {
    assert!(compile_eval::<f32, 256>("main = !", "main", Mode::Strict).is_err());
}

#[test]
fn parse_error_is_reported() {
    assert!(compile_eval::<f32, 256>("_", "main", Mode::Strict).is_err());
}

#[test]
fn plain_expression_rejected() {
    assert!(compile_eval::<f32, 256>("_ * 0.5", "main", Mode::Strict).is_err());
}
