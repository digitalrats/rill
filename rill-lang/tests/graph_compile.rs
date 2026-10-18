//! Whole-buffer program compile/run smoke tests through the retained
//! value path: a 0-input generator program.

use rill_lang::compile_with_ffi;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::register;

fn ffi() -> ForeignRegistry<f32> {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    ffi
}

#[test]
fn compiles_simple_graph() {
    let result = compile_with_ffi::<f32>("main = sine 440.0 0.5 0.0", &ffi(), 44100.0);
    assert!(result.is_ok(), "should compile simple graph");
}

#[test]
fn single_algorithm_works() {
    let result = compile_with_ffi::<f32>("main = sine 440.0 0.5 0.0", &ffi(), 44100.0);
    assert!(result.is_ok(), "single algorithm should compile");
}

#[test]
fn engine_executes_and_produces_output() {
    let src = "main = sine 440.0 0.5 0.0";
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ffi();
    let prog = rill_lang::test_eval::<f32, 256>(defs, expr, &ffi, 44100.0).unwrap();

    let r = prog.result().expect("whole-buffer result expected");
    let mut output = [0.0f32; 8];
    assert!(prog.copy_buffer(r, &mut output));
    let expected = 0.5 * (std::f32::consts::PI * 2.0 * (440.0f32 / 44100.0f32)).sin();
    assert_eq!(output[0], 0.0, "sine starts at phase 0");
    assert!(
        (output[1] - expected).abs() < 1e-4,
        "sample 1 = {expected}, got {}",
        output[1]
    );
}

#[test]
fn engine_with_param_compiles_and_processes() {
    // `?level` is a control param (default 0.0): the generator starts silent.
    // The single-run surface cannot inject a `SetParameter` between runs (the
    // re-run surface is gone), so the original live-parameter re-run half is
    // deleted: only the default-param run — a single `test_eval` run — is
    // retained.
    let src = "main = sine 440.0 ?level 0.0";
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ffi();
    let prog = rill_lang::test_eval::<f32, 256>(defs, expr, &ffi, 44100.0).unwrap();

    let r = prog.result().expect("whole-buffer result expected");
    let mut output = [0.0f32; 8];
    assert!(prog.copy_buffer(r, &mut output));
    assert_eq!(output, [0.0; 8], "level default is 0.0 => silence");
}
