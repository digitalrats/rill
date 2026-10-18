//! Parameter defaults on the retained value path.
//!
//! A `?name` control parameter is a slot the host writes through
//! [`RillProgram::set_param`](rill_lang::program::RillProgram::set_param).
//! On the single-run value surface the program is evaluated exactly once
//! (inside `test_eval`), so the host cannot inject a value before that one
//! internal run: only the DEFAULT-reading behavior survives here. The
//! `set_param`-between-runs tests were Class 2 (no public re-run surface) and
//! are removed (ledger: rill-adrift/tests/engine_params.rs).

use rill_adrift::lang_builtins::full_ffi;
use rill_lang::test_eval;

/// Evaluate `src` once through the single-run value surface and return the
/// program, whose `result()` holds the run's output.
fn eval(src: &str) -> rill_lang::program::RillProgram<f32, 256> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = full_ffi::<f32>();
    test_eval::<f32, 256>(defs, expr, &ffi, 44_100.0)
        .unwrap_or_else(|e| panic!("program must compile: {e}\nsource:\n{src}"))
}

/// Read the single run's scalar output.
fn scalar_output(prog: &rill_lang::program::RillProgram<f32, 256>) -> f64 {
    match prog.arena().get(prog.result().unwrap()).unwrap() {
        rill_lang::arena::Value::Float(v) => *v,
        other => panic!("expected a Float output, got {other:?}"),
    }
}

#[test]
fn actor_param_default_applies() {
    // `?gain=0.25` keeps its source default on the single run.
    let prog = eval("main = (?gain=0.25) * 2.0");
    assert_eq!(scalar_output(&prog), 0.5);
}

#[test]
fn actor_param_no_default_applies() {
    // A parameter with no source default starts at 0.0 on the single run.
    let prog = eval("main = ?gain * 2.0");
    assert_eq!(scalar_output(&prog), 0.0);
}

#[test]
fn actor_param_default_reaches_a_block_builtin() {
    // A parameter default reaches an FFI block builtin: `sine`'s amplitude,
    // whose `?level 0.0` default yields silence on the single run.
    let prog = eval("main = sine 440.0 ?level 0.0");

    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 64];
    assert!(prog.copy_buffer(r, &mut out));
    assert_eq!(out, [0.0; 64], "level default is 0.0 => silence");
    // The live-parameter half (write `set_param(level, 0.5)`, re-run, assert
    // the amplitude) was Class 2: no re-run surface exists after the single
    // internal run, so the host cannot inject the value mid-program.
}

// Class 2 removals (ledger: rill-adrift/tests/engine_params.rs):
// - actor_param_set_param_affects_output :: sole assertion is value-from-set-param
//   between runs; no re-run surface :: delete
//   (the default-application half survives as `actor_param_default_applies`)
// - actor_param_persists_across_ticks :: set_param + two value runs, cross-tick
//   persistence; no re-run surface :: delete
// - actor_param_reaches_a_block_builtin :: set_param + re-run amplitude check;
//   reduced to the default-silence half (`actor_param_default_reaches_a_block_builtin`)
