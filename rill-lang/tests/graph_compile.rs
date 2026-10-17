//! Whole-buffer program compile/run smoke tests through the retained
//! value path: a 0-input generator program.

use rill_core::traits::ParamValue;
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
    let mut prog = compile_with_ffi::<f32>("main = sine 440.0 0.5 0.0", &ffi(), 44100.0).unwrap();

    let mut output = [0.0f32; 8];
    prog.run_value().unwrap();
    assert!(prog.copy_output_samples(0, &mut output));
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
    // `?level` is a control param (default 0.0): the generator starts silent
    // and the live SetParameter reaches its amplitude.
    let mut prog =
        compile_with_ffi::<f32>("main = sine 440.0 ?level 0.0", &ffi(), 44100.0).unwrap();

    let mut output = [0.0f32; 8];
    prog.run_value().unwrap();
    assert!(prog.copy_output_samples(0, &mut output));
    assert_eq!(output, [0.0; 8], "level default is 0.0 => silence");

    let idx = prog.param_index("level").unwrap();
    prog.set_param(idx, ParamValue::Float(0.5));
    prog.run_value().unwrap();
    assert!(prog.copy_output_samples(0, &mut output));
    // The oscillator continues its phase, so assert the live amplitude bounds
    // the output rather than pinning one sample.
    assert!(
        output.iter().any(|v| v.abs() > 1e-3),
        "the live `level` must reach the generator, got {output:?}"
    );
    assert!(output.iter().all(|v| v.abs() <= 0.5 + 1e-6));
}
